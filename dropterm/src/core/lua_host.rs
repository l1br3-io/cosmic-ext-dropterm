// SPDX-License-Identifier: GPL-3.0-only

//! The Tier-1 Lua runtime every scriptable surface stands on
//! (lua-host spec, the plugins initiative's foundation): one
//! dedicated host thread owning one mlua interpreter for its life,
//! speaking `std::sync::mpsc` both ways — commands in, `dropterm:`
//! log lines out. The UI thread never executes Lua and never blocks
//! on the host (the pty-test-gate divergence from `WezTerm`'s GUI-thread
//! Lua is this module's reason to exist), and no tokio channel crosses
//! the bridge, so no foreign-thread waker can reach the foreground
//! executor's pump.
//!
//! Inside scripts, `require('dropterm')` yields the bus port:
//! `dropterm.on(event, handler)` registers into a registry table
//! (`WezTerm`'s design) and `dropterm.emit(event, payload)` dispatches
//! to every handler registered for that event — script-to-script on
//! the host thread. Host-side delivery arrives as
//! [`HostCommand::Emit`] on the same dispatch path (`event-hooks`
//! later). Scripts are the user's own config executing as the user:
//! the complete Lua 5.4 stdlib, no sandbox, stated plainly in the
//! docs.
//!
//! Lazy by design: constructing [`LuaHost`] builds channels and
//! nothing else — no thread, no scan, no log. The first command
//! materializes the host thread, which then scans the plugins dir
//! (flat, `.lua` files only, sorted by name) and loads each script; a
//! load error or a handler raise costs exactly one log line and never
//! takes a sibling script or the host down with it.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use mlua::{Function, Lua, LuaOptions, LuaSerdeExt, MultiValue, StdLib, Table, Value};

/// One item of work for the host thread. The UI never blocks on the
/// answer: a dispatch is fire-and-forget, and a future query
/// surface's reply lands as a new [`HostEvent`] arm matched by token
/// — stale until it arrives (the soft-timeout contract, task L2).
/// The vocabulary stays closed: new commands join as new arms with
/// the module that needs them, never by widening these semantics.
pub enum HostCommand {
    /// Deliver an event to every handler registered for it —
    /// host-side injections now, scripted `emit` and `event-hooks`
    /// both riding this same path.
    Emit {
        /// The event name `dropterm.on` registered under.
        event: String,
        /// The payload, converted to a Lua value at the port via
        /// mlua's serde bridge (the spec's "payloads are Lua tables"
        /// rule).
        payload: serde_json::Value,
    },
}

/// One observation out of the host thread. v1 carries only the log;
/// result tokens join as new arms with the query surfaces that ask
/// for them (the same closed-extension rule).
pub enum HostEvent {
    /// One `dropterm:` evidence line: host up, each script loaded,
    /// each error with its file, each script's forwarded `print`.
    Log(String),
}

/// The Lua host handle. Construction is free — channels, nothing
/// else — and the first [`LuaHost::send`] materializes the thread:
/// no command, no thread, no scan, no log (the lazy pin, spec
/// assumption 4). One owner (the shell, task L4); the thread lives
/// until the handle drops, then takes its interpreter with it.
pub struct LuaHost {
    /// The plugins dir the first scan (and every future reload)
    /// reads — flat, `.lua` files only.
    plugins_dir: PathBuf,
    /// The pre-spawn halves of both channels, taken (never
    /// replaced) by the first `send`: `None` is the spawned marker.
    dormant: Mutex<Option<(Receiver<HostCommand>, Sender<HostEvent>)>>,
    /// The command inlet for the host thread's life.
    commands: Mutex<Sender<HostCommand>>,
    /// The host's outbound stream, drained without blocking by
    /// [`LuaHost::try_event`] on the shell's tick (task L4).
    events: Receiver<HostEvent>,
}

impl LuaHost {
    /// Build the host handle over `plugins_dir`. Nothing spawns,
    /// nothing scans, nothing logs — boot stays byte-identical to a
    /// no-plugins run until the first command arrives.
    pub fn new(plugins_dir: impl Into<PathBuf>) -> Self {
        let (commands, command_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        Self {
            plugins_dir: plugins_dir.into(),
            dormant: Mutex::new(Some((command_rx, event_tx))),
            commands: Mutex::new(commands),
            events,
        }
    }

    /// Deliver one command, spawning the host first if it has never
    /// been spawned. Never blocks on the answer — the command
    /// crosses the channel and control returns (the spec's
    /// fire-and-forget rule).
    pub fn send(&self, command: HostCommand) {
        self.spawn_if_dormant();
        let alive = self
            .commands
            .lock()
            .expect("the lua host gate is only contended briefly")
            .send(command)
            .is_ok();
        if !alive {
            // The host thread is gone (spawn failure or a panic) and
            // the channel that would carry the word died with it.
            println!("dropterm: plugins: host thread is gone; command dropped");
        }
    }

    /// Drain one outbound event without blocking — the shell's tick
    /// calls this in a loop beside `tray_actions`/`pane_signals`. An
    /// empty or dead channel reads as no event: the drain never
    /// waits, so no foreign-thread wake can ever reach the UI.
    pub fn try_event(&self) -> Option<HostEvent> {
        self.events.try_recv().ok()
    }

    /// Whether the first command has materialized the host thread —
    /// the lazy pin's observation point.
    pub fn spawned(&self) -> bool {
        self.dormant
            .lock()
            .expect("the lua host gate is only contended briefly")
            .is_none()
    }

    /// Materialize the host thread if it never has been: the first
    /// command pays the interpreter. Taking the dormant halves under
    /// the lock makes the first caller the only spawner.
    fn spawn_if_dormant(&self) {
        let dormant = self
            .dormant
            .lock()
            .expect("the lua host gate is only contended briefly")
            .take();
        let Some((commands, events)) = dormant else {
            return;
        };
        let plugins_dir = self.plugins_dir.clone();
        let spawned = thread::Builder::new()
            .name("dropterm-lua-host".to_owned())
            .spawn(move || host_thread(&plugins_dir, &commands, &events));
        if let Err(err) = spawned {
            println!("dropterm: plugins: host thread failed to spawn: {err}");
        }
    }
}

/// The host thread's whole life: build the interpreter, install the
/// port, pay the first scan, then serve commands in arrival order —
/// one thread, one interpreter, strictly serial (spec assumption 8),
/// so no dispatch can race another. Returns when the handle drops
/// (the command channel dies) and drops the interpreter on the way
/// out.
fn host_thread(plugins_dir: &Path, commands: &Receiver<HostCommand>, events: &Sender<HostEvent>) {
    // SAFETY: `StdLib::ALL` loads the unsafe io/os/debug libraries.
    // That is the spec's ratified trust model (assumption 2): plugins
    // are the user's own config executing as the user, no sandbox
    // claimed anywhere — the docs state it plainly. Nothing untrusted
    // crosses this state from the host's side.
    let lua = unsafe { Lua::unsafe_new_with(StdLib::ALL, LuaOptions::new()) };
    let registry = match install_port(&lua, events) {
        Ok(registry) => registry,
        Err(err) => {
            log(
                events,
                format!("the dropterm port would not install: {err}"),
            );
            return;
        }
    };
    log(events, "host thread up");
    load_scripts(&lua, plugins_dir, events);
    while let Ok(command) = commands.recv() {
        let HostCommand::Emit { event, payload } = command;
        match lua.to_value(&payload) {
            Ok(value) => dispatch(events, &registry, &event, &value),
            Err(err) => log(
                events,
                format!("payload for '{event}' would not convert: {err}"),
            ),
        }
    }
}

/// Install the `dropterm` port into the interpreter: the registry
/// table behind `dropterm.on`/`dropterm.emit` (`WezTerm`'s
/// registry-table design), the module served to
/// `require('dropterm')` through `package.loaded` (the stdlib's own
/// module cache — no require-path widening), and the `print`
/// forwarder. Returns the registry for host-side dispatch.
fn install_port(lua: &Lua, events: &Sender<HostEvent>) -> mlua::Result<Table> {
    let registry = lua.create_table()?;

    let on_table = registry.clone();
    let on = lua.create_function(move |lua, (event, handler): (String, Function)| {
        let handlers: Table =
            if let Some(existing) = on_table.get::<Option<Table>>(event.clone())? {
                existing
            } else {
                let fresh = lua.create_table()?;
                on_table.set(event, fresh.clone())?;
                fresh
            };
        // The sequence's next slot: registration order is dispatch
        // order.
        handlers.set(handlers.raw_len() + 1, handler)?;
        Ok(())
    })?;

    let emit_registry = registry.clone();
    let emit_events = events.clone();
    let emit = lua.create_function(move |_lua, (event, payload): (String, Value)| {
        dispatch(&emit_events, &emit_registry, &event, &payload);
        Ok(())
    })?;

    let module = lua.create_table()?;
    module.set("on", on)?;
    module.set("emit", emit)?;
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.set("dropterm", module)?;

    install_print(lua, events)?;
    Ok(registry)
}

/// Deliver one event to every handler registered under it, on this
/// thread, in registration order. An event nobody registered for
/// dispatches to nobody, silently (the bus contract). A raising
/// handler costs one log line and never stops its siblings.
fn dispatch(events: &Sender<HostEvent>, registry: &Table, event: &str, payload: &Value) {
    let handlers: Option<Table> = match registry.get(event) {
        Ok(handlers) => handlers,
        Err(err) => {
            log(
                events,
                format!("the registry read for '{event}' failed: {err}"),
            );
            return;
        }
    };
    let Some(handlers) = handlers else {
        return;
    };
    for slot in 1..=handlers.raw_len() {
        let handler: Function = match handlers.get(slot) {
            Ok(handler) => handler,
            Err(err) => {
                log(
                    events,
                    format!("a handler for '{event}' would not read: {err}"),
                );
                continue;
            }
        };
        if let Err(err) = handler.call::<()>(payload.clone()) {
            log(events, format!("a handler for '{event}' raised: {err}"));
        }
    }
}

/// Forward scripts' `print` to the host's log — the only channel a
/// headless host has (spec OQ-4's ruling): one line per call,
/// arguments tab-joined like Lua's own `print`.
fn install_print(lua: &Lua, events: &Sender<HostEvent>) -> mlua::Result<()> {
    let print = lua.create_function({
        let events = events.clone();
        move |lua, args: MultiValue| {
            let mut parts = Vec::new();
            for value in args {
                parts.push(display_value(lua, value));
            }
            log(&events, parts.join("\t"));
            Ok(())
        }
    })?;
    lua.globals().set("print", print)
}

/// Render one `print` argument the way the log can carry it: Lua's
/// own coercions for strings and numbers, the literal for nil and
/// booleans, and a plain "value" for everything else (real `print`
/// shows an address there; the log settles for honesty).
fn display_value(lua: &Lua, value: Value) -> String {
    match value {
        Value::Nil => "nil".to_owned(),
        Value::Boolean(flag) => flag.to_string(),
        other => match lua.coerce_string(other) {
            Ok(Some(text)) => text.to_string_lossy(),
            _ => "value".to_owned(),
        },
    }
}

/// Scan `dir` for `.lua` plugin scripts and load each into the
/// interpreter, sorted by file name so registration order is
/// deterministic. The loader is read-only — nothing under the dir is
/// written, moved, or renamed — and a missing dir is the silent
/// first run. A script that fails to parse or raises at load costs
/// one log line and never stops its siblings.
fn load_scripts(lua: &Lua, dir: &Path, events: &Sender<HostEvent>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return,
        Err(err) => {
            log(events, format!("cannot read {}: {err}", dir.display()));
            return;
        }
    };
    let mut scripts: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "lua"))
        .collect();
    scripts.sort();
    for path in scripts {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => {
                log(events, format!("skipped {name}: unreadable: {err}"));
                continue;
            }
        };
        match lua.load(text).set_name(name.clone()).exec() {
            Ok(()) => log(events, format!("loaded {name}")),
            Err(err) => log(events, format!("skipped {name}: {err}")),
        }
    }
}

/// One `dropterm:` evidence line onto the outbound channel. A send
/// failure means the drain side is gone (shutdown); panicking a host
/// thread over a log line would be worse.
fn log(events: &Sender<HostEvent>, text: impl fmt::Display) {
    let _ = events.send(HostEvent::Log(format!("dropterm: plugins: {text}")));
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    use super::*;

    /// A per-test scratch directory under the system temp dir with a
    /// process- and test-unique name, removed (best-effort) on drop
    /// even when an assertion fails. Std only — no dev-dependency.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let unique = format!(
                "dropterm-lua-host-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir_all(&path).expect("create scratch dir");
            Self(path)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    /// The `require('dropterm')` line every fixture script opens
    /// with.
    const PORT: &str = "local d = require('dropterm')";

    fn script(dir: &TempDir, name: &str, text: &str) {
        fs::write(dir.join(name), text).expect("write fixture script");
    }

    /// Collect the host's log lines until it goes quiet (200 ms with
    /// no event): the host thread works asynchronously off its
    /// command channel, so a test reads its whole reaction to a
    /// command by draining to the silence — a bounded wait on the
    /// channel, never a fixed sleep.
    fn logs_until_quiet(host: &LuaHost) -> Vec<String> {
        let mut lines = Vec::new();
        loop {
            match host.events.recv_timeout(Duration::from_millis(200)) {
                Ok(HostEvent::Log(line)) => lines.push(line),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return lines,
            }
        }
    }

    fn emit(host: &LuaHost, event: &str) {
        host.send(HostCommand::Emit {
            event: event.to_owned(),
            payload: serde_json::json!({}),
        });
    }

    // --- the lazy pin ---

    #[test]
    fn no_command_spawns_nothing() {
        let dir = TempDir::new("lazy");
        script(
            &dir,
            "hello.lua",
            &format!("{PORT}\nd.on('x', function() print('hi') end)"),
        );
        let host = LuaHost::new(&dir.0);
        assert!(!host.spawned());
        // No thread, no scan, no log: the only honest observation of
        // "nothing happened" is a channel that stays empty.
        assert!(matches!(
            host.events.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Timeout)
        ));
    }

    #[test]
    fn a_missing_dir_costs_only_the_host_up_line() {
        let dir = TempDir::new("absent");
        let host = LuaHost::new(dir.join("plugins"));
        emit(&host, "x");
        assert_eq!(
            logs_until_quiet(&host),
            ["dropterm: plugins: host thread up".to_owned()]
        );
    }

    // --- the first command: spawn, scan, load, deliver ---

    #[test]
    fn the_first_command_spawns_scans_loads_and_delivers() {
        let dir = TempDir::new("hello");
        script(
            &dir,
            "hello.lua",
            &format!("{PORT}\nd.on('x', function() print('hi from lua') end)"),
        );
        let host = LuaHost::new(&dir.0);
        emit(&host, "x");
        assert!(host.spawned());
        let lines = logs_until_quiet(&host);
        assert!(
            lines.iter().any(|line| line.contains("loaded hello.lua")),
            "the scan must load the fixture: {lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("hi from lua")),
            "the host-injected emit must reach the handler: {lines:?}"
        );
    }

    #[test]
    fn print_joins_arguments_with_tabs_and_coerces_numbers() {
        let dir = TempDir::new("print");
        script(&dir, "chatty.lua", &format!("{PORT}\nprint('n', 42)"));
        let host = LuaHost::new(&dir.0);
        emit(&host, "anything");
        let lines = logs_until_quiet(&host);
        assert!(
            lines.iter().any(|line| line.ends_with("n\t42")),
            "print must forward one tab-joined line: {lines:?}"
        );
    }

    // --- the bus ---

    #[test]
    fn a_scripts_emit_reaches_another_scripts_handler() {
        let dir = TempDir::new("bus");
        // Sorted file order runs the registration before the emit;
        // the pin holds the loader to that order.
        script(
            &dir,
            "a-registers.lua",
            &format!("{PORT}\nd.on('y', function() print('from-y') end)"),
        );
        script(&dir, "b-emits.lua", &format!("{PORT}\nd.emit('y', {{}})"));
        let host = LuaHost::new(&dir.0);
        emit(&host, "unrelated");
        let lines = logs_until_quiet(&host);
        assert!(
            lines.iter().any(|line| line.contains("from-y")),
            "scripted emit must cross scripts: {lines:?}"
        );
    }

    #[test]
    fn unregistered_events_reach_nobody() {
        let dir = TempDir::new("silent");
        script(
            &dir,
            "hello.lua",
            &format!("{PORT}\nd.on('x', function() print('hi') end)"),
        );
        let host = LuaHost::new(&dir.0);
        emit(&host, "other");
        let lines = logs_until_quiet(&host);
        assert!(lines.iter().any(|line| line.contains("loaded hello.lua")));
        assert!(
            !lines.iter().any(|line| line.contains("hi")),
            "an unregistered event dispatches to nobody, silently: {lines:?}"
        );
    }

    // --- one log line per failure, survivors survive ---

    #[test]
    fn a_broken_script_costs_one_line_and_its_siblings_load() {
        let dir = TempDir::new("broken");
        script(&dir, "broken.lua", "this is not ( lua");
        script(
            &dir,
            "good.lua",
            &format!("{PORT}\nd.on('x', function() print('ok') end)"),
        );
        let host = LuaHost::new(&dir.0);
        emit(&host, "x");
        let lines = logs_until_quiet(&host);
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("broken.lua"))
                .count(),
            1,
            "a load error costs exactly one line: {lines:?}"
        );
        assert!(lines.iter().any(|line| line.contains("loaded good.lua")));
        assert!(
            lines.iter().any(|line| line.contains(" ok")),
            "the sibling script's handler still answers: {lines:?}"
        );
    }

    #[test]
    fn a_raising_handler_costs_one_line_and_the_host_survives() {
        let dir = TempDir::new("raising");
        script(
            &dir,
            "boom.lua",
            &format!(
                "{PORT}\nd.on('boom', function() error('kaboom') end)\n\
                 d.on('fine', function() print('still here') end)"
            ),
        );
        let host = LuaHost::new(&dir.0);
        emit(&host, "boom");
        let first = logs_until_quiet(&host);
        assert_eq!(
            first.iter().filter(|line| line.contains("kaboom")).count(),
            1,
            "a handler raise costs exactly one line: {first:?}"
        );
        emit(&host, "fine");
        let second = logs_until_quiet(&host);
        assert!(
            second.iter().any(|line| line.contains("still here")),
            "the host must survive the raise: {second:?}"
        );
    }
}
