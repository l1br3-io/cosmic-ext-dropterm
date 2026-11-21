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
//!
//! The lifecycle machine (task L2): every unit of Lua work runs under
//! a soft bound and a hard cap, enforced by a debug hook that counts
//! VM instructions. Past the soft bound a dispatch is stale — one log
//! line at the moment, its eventual result discarded at the drain once
//! query surfaces exist to ask for one. Past the hard cap the
//! interpreter is torn down and recreated: registrations lost by
//! design, scripts reloaded lazily on the next command — the
//! runaway-script escape hatch. The bounds cover Lua running on the
//! host's main thread; a script blocked inside a C call (`io.read` on
//! a pipe) or spinning inside a coroutine it created executes no
//! main-thread VM instructions to trip the hook. Full trust means the
//! docs say so plainly rather than claim a sandbox.
//!
//! Hot reload (task L3): a non-recursive `notify` watch on the
//! plugins dir queues a `Reload` on the command channel — the watcher
//! never touches the UI channel and never spawns the host. A
//! `Reload` recreates the interpreter whole: fresh registrations, the
//! old ones gone with the old generation (under the registry-table
//! design a re-run file could not un-register its old handlers, so
//! the whole interpreter is the only sound reload unit), scripts
//! rescanned at once.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use notify::Watcher as _;

use mlua::{
    Function, HookTriggers, Lua, LuaOptions, LuaSerdeExt, MultiValue, StdLib, Table, Value, VmState,
};

/// The soft bound every dispatch runs under (task L2): past it the
/// dispatch is stale — one log line at the moment, and the eventual
/// result, once query surfaces exist to ask, is discarded at the drain
/// so the asking surface keeps its prior value (stale-degrade, never
/// block). A constant, not a config key (spec assumption 3).
const SOFT_BOUND: Duration = Duration::from_millis(100);

/// The hard cap past which the interpreter is torn down and recreated:
/// registrations lost by design (spec assumption 7), scripts reloaded
/// lazily on the next command — the runaway-script escape hatch. One
/// log line; the host thread and the terminal never notice.
const HARD_CAP: Duration = Duration::from_secs(1);

/// How often the debug hook gets a look: every N VM instructions the
/// frame's clock is checked against the bounds. Small enough that the
/// bounds land within slack of their marks, large enough that the
/// check is noise next to the work between fires.
const HOOK_EVERY_N_INSTRUCTIONS: u32 = 2_000;

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
    /// Rescan the plugins dir and reload: the dir watcher's wake on
    /// any event (task L3), recreating the interpreter whole — fresh
    /// registrations, the old ones gone with the old generation. That
    /// whole-interpreter unit is the picked reload semantics: under
    /// the registry-table design a re-run file could not un-register
    /// its old handlers, so there is no sound smaller unit.
    Reload,
}

/// One observation out of the host thread. v1 carries only the log;
/// result tokens join as new arms with the query surfaces that ask
/// for them (the same closed-extension rule).
pub enum HostEvent {
    /// One `dropterm:` evidence line: host up, each script loaded,
    /// each error with its file, each script's forwarded `print`.
    Log(String),
}

/// The pre-spawn halves of both channels, waiting inside the gate
/// for the first `send` to hand them to the host thread.
type DormantChannels = Option<(Receiver<HostCommand>, Sender<HostEvent>)>;

/// The Lua host handle. Construction is free — channels, nothing
/// else — and the first [`LuaHost::send`] materializes the thread:
/// no command, no thread, no scan, no log (the lazy pin, spec
/// assumption 4). One owner (the shell, task L4); the thread lives
/// until the handle drops, then takes its interpreter with it.
pub struct LuaHost {
    /// The plugins dir the first scan (and every future reload)
    /// reads — flat, `.lua` files only.
    plugins_dir: PathBuf,
    /// The gate: the channel halves sit here until the first `send`
    /// takes them (never replaces them); `None` is the spawned
    /// marker. Arc-shared with the dir watcher (task L3) so its
    /// feed can stay dormant-preserving.
    dormant: Arc<Mutex<DormantChannels>>,
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
            dormant: Arc::new(Mutex::new(Some((command_rx, event_tx)))),
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

    /// Watch the plugins dir (non-recursive, the config watcher's
    /// shape) and queue a [`HostCommand::Reload`] on any event (task
    /// L3): the watcher feeds the host's command channel — never the
    /// UI's event channel — and never spawns the host. While the host
    /// has never spawned there is nothing to reload: the first
    /// command's boot scan reads what is on disk, so a dir event
    /// before that point is dropped rather than queued as a stale
    /// duplicate rescan. Returns the watcher guard (dropping it stops
    /// watching); a dir that cannot be watched is `None` — missing is
    /// the silent first-run norm (the theme-files posture), anything
    /// else costs one line.
    pub fn watch_plugins_dir(&self) -> Option<notify::RecommendedWatcher> {
        let commands = self
            .commands
            .lock()
            .expect("the lua host gate is only contended briefly")
            .clone();
        let dormant = Arc::clone(&self.dormant);
        let mut watcher = match notify::recommended_watcher(move |event| {
            let _ = event; // any event in the plugins dir: reload whole
            if dormant
                .lock()
                .expect("the lua host gate is only contended briefly")
                .is_some()
            {
                return;
            }
            let _ = commands.send(HostCommand::Reload);
        }) {
            Ok(watcher) => watcher,
            Err(err) => {
                println!("dropterm: plugins: watcher failed: {err:?}");
                return None;
            }
        };
        match watcher.watch(&self.plugins_dir, notify::RecursiveMode::NonRecursive) {
            Ok(()) => Some(watcher),
            Err(err) => {
                if matches!(
                    &err.kind,
                    notify::ErrorKind::Io(io_err) if io_err.kind() == io::ErrorKind::NotFound
                ) {
                    return None;
                }
                println!("dropterm: plugins: watch failed: {err:?}");
                None
            }
        }
    }
}

/// The host thread's whole life: generations of interpreter, each
/// built, ported, bounded, then served strictly in arrival order — one
/// thread, one interpreter at a time, no dispatch racing another (spec
/// assumption 8). Generation 1 pays the first scan eagerly (the spawn
/// was itself the first work); a watcher `Reload` recreates eagerly
/// too (the host is otherwise idle — the rescan's lines land at edit
/// time, not at some later dispatch); only the hard cap defers its
/// rescan to the next command, so a load-time runaway costs one
/// interpreter cycle per command, never a free-spinning reload loop.
/// Returns when the handle drops (the command channel dies) and drops
/// the interpreter on the way out.
fn host_thread(plugins_dir: &Path, commands: &Receiver<HostCommand>, events: &Sender<HostEvent>) {
    let frame = Rc::new(TimeoutFrame::new());
    let mut first_generation = true;
    // Set by a `Reload` command: this generation loads eagerly (the
    // watcher's wake — the rescan is the work).
    let mut rescan_now = false;
    // Set by a hard cap: the next command rescans and reloads the
    // scripts before it dispatches — the lazy half of the escape
    // hatch.
    let mut reload_pending = false;
    loop {
        let Some(generation) = Generation::build(events, &frame) else {
            return;
        };
        if first_generation {
            log(events, "host thread up");
        }
        if first_generation || rescan_now {
            load_scripts(&generation.lua, plugins_dir, events, &frame);
            first_generation = false;
            rescan_now = false;
            if frame.hard_tripped.get() {
                reload_pending = true;
                continue;
            }
        }
        let mut reload_requested = false;
        while let Ok(command) = commands.recv() {
            match command {
                HostCommand::Emit { event, payload } => {
                    if reload_pending {
                        load_scripts(&generation.lua, plugins_dir, events, &frame);
                        reload_pending = false;
                        if frame.hard_tripped.get() {
                            break;
                        }
                    }
                    match generation.lua.to_value(&payload) {
                        Ok(value) => {
                            frame.begin(true, format_args!("a handler for '{event}'"));
                            dispatch(events, &generation.registry, &event, &value, &frame);
                        }
                        Err(err) => log(
                            events,
                            format!("payload for '{event}' would not convert: {err}"),
                        ),
                    }
                    if frame.hard_tripped.get() {
                        break;
                    }
                }
                HostCommand::Reload => {
                    reload_requested = true;
                    break;
                }
            }
        }
        if reload_requested {
            rescan_now = true;
            continue;
        }
        // The inner loop exits only on a tripped hard cap (recreate)
        // or a dead command channel (the handle dropped: shut down).
        if !frame.hard_tripped.get() {
            return;
        }
        reload_pending = true;
    }
}

/// One timeout frame per top-level unit of Lua work — a command's
/// dispatch, a file's load — shared between the host loop that begins
/// it and the debug hook that watches it. Same thread, an `Rc`:
/// interior mutability only, no synchronization (the serial host, spec
/// assumption 8).
#[derive(Debug)]
struct TimeoutFrame {
    /// When this unit of work began.
    started: Cell<Instant>,
    /// What the timeout lines call the work ("a handler for 'x'",
    /// "loading x.lua").
    label: RefCell<String>,
    /// The stale line fires once per unit.
    soft_logged: Cell<bool>,
    /// Set by the hook at the hard cap; the host loop's signal to
    /// recreate the interpreter.
    hard_tripped: Cell<bool>,
    /// Dispatches arm the soft bound; loads run under the hard cap
    /// only — a slow load has no result to go stale.
    soft_armed: Cell<bool>,
}

impl TimeoutFrame {
    fn new() -> Self {
        Self {
            started: Cell::new(Instant::now()),
            label: RefCell::new(String::new()),
            soft_logged: Cell::new(false),
            hard_tripped: Cell::new(false),
            soft_armed: Cell::new(false),
        }
    }

    /// Begin one unit of work: restart the clock, reset the flags,
    /// name the unit for the timeout lines.
    fn begin(&self, soft_armed: bool, label: impl fmt::Display) {
        self.started.set(Instant::now());
        self.soft_armed.set(soft_armed);
        self.soft_logged.set(false);
        self.hard_tripped.set(false);
        *self.label.borrow_mut() = label.to_string();
    }
}

/// One interpreter generation: the `Lua` instance plus its port's
/// registry. Generation 1 is born at spawn; every later one is a
/// hard-cap recreate — fresh state, no registrations (spec assumption
/// 7).
struct Generation {
    lua: Lua,
    registry: Table,
}

impl Generation {
    /// Build a fresh interpreter with the port and the timeout bounds
    /// installed. `None` is the nothing-scriptable-remains case: the
    /// port or the bounds refusing to install costs one line and ends
    /// the host thread.
    fn build(events: &Sender<HostEvent>, frame: &Rc<TimeoutFrame>) -> Option<Generation> {
        // SAFETY: `StdLib::ALL` loads the unsafe io/os/debug libraries.
        // That is the spec's ratified trust model (assumption 2):
        // plugins are the user's own config executing as the user, no
        // sandbox claimed anywhere — the docs state it plainly.
        // Nothing untrusted crosses this state from the host's side.
        let lua = unsafe { Lua::unsafe_new_with(StdLib::ALL, LuaOptions::new()) };
        let registry = match install_port(&lua, events, frame) {
            Ok(registry) => registry,
            Err(err) => {
                log(
                    events,
                    format!("the dropterm port would not install: {err}"),
                );
                return None;
            }
        };
        if let Err(err) = install_bounds(&lua, events, frame) {
            log(
                events,
                format!("the timeout bounds would not install: {err}"),
            );
            return None;
        }
        Some(Generation { lua, registry })
    }
}

/// Arm the lifecycle machine on the interpreter (task L2): a debug
/// hook fires every [`HOOK_EVERY_N_INSTRUCTIONS`] VM instructions and
/// checks the frame's clock. Past the soft bound (when armed) it logs
/// the stale line once and lets the work run on; past the hard cap it
/// logs the reload line and raises, the error propagating out of the
/// executing Lua — `mlua` documents exactly this execution-limit
/// technique. Instruction counting is the mechanism the chunk named
/// as sound: cooperative bounds cannot stop `while true do end`. Its
/// honest limits — a script blocked inside a C call, or spinning
/// inside a coroutine it created, runs no main-thread VM instructions
/// to trip the hook — belong to the full-trust model and are stated in
/// the docs, not solved here.
fn install_bounds(
    lua: &Lua,
    events: &Sender<HostEvent>,
    frame: &Rc<TimeoutFrame>,
) -> mlua::Result<()> {
    let hook_events = events.clone();
    let hook_frame = frame.clone();
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(HOOK_EVERY_N_INSTRUCTIONS),
        move |_lua, _debug| {
            let elapsed = hook_frame.started.get().elapsed();
            if elapsed >= HARD_CAP {
                hook_frame.hard_tripped.set(true);
                log(
                    &hook_events,
                    format!(
                        "{} ran past the hard cap ({HARD_CAP:?}), reloading the interpreter",
                        hook_frame.label.borrow(),
                    ),
                );
                return Err(mlua::Error::RuntimeError(
                    "the dropterm hard cap was exceeded".into(),
                ));
            }
            if hook_frame.soft_armed.get() && !hook_frame.soft_logged.get() && elapsed >= SOFT_BOUND
            {
                hook_frame.soft_logged.set(true);
                log(
                    &hook_events,
                    format!(
                        "{} ran past the soft bound ({SOFT_BOUND:?}), going stale",
                        hook_frame.label.borrow(),
                    ),
                );
            }
            Ok(VmState::Continue)
        },
    )
}

/// Install the `dropterm` port into the interpreter: the registry
/// table behind `dropterm.on`/`dropterm.emit` (`WezTerm`'s
/// registry-table design), the module served to
/// `require('dropterm')` through `package.loaded` (the stdlib's own
/// module cache — no require-path widening), and the `print`
/// forwarder. Returns the registry for host-side dispatch.
fn install_port(
    lua: &Lua,
    events: &Sender<HostEvent>,
    frame: &Rc<TimeoutFrame>,
) -> mlua::Result<Table> {
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
    let emit_frame = frame.clone();
    let emit = lua.create_function(move |_lua, (event, payload): (String, Value)| {
        dispatch(&emit_events, &emit_registry, &event, &payload, &emit_frame);
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
/// handler costs one log line and never stops its siblings — unless
/// the raise is the hard cap's abort, which the hook already logged
/// and which unwinds every nested dispatch: a scripted `emit` inside a
/// handler shares the enclosing dispatch's frame, so the bounds
/// measure the whole command's work, nested emits included.
fn dispatch(
    events: &Sender<HostEvent>,
    registry: &Table,
    event: &str,
    payload: &Value,
    frame: &TimeoutFrame,
) {
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
            if frame.hard_tripped.get() {
                // The hook logged the reload line; this interpreter is
                // about to be torn down and the abort unwinds silently.
                return;
            }
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
/// one log line and never stops its siblings. Each file's load runs
/// under the hard cap (never the soft bound — a load has no result to
/// go stale); a tripped cap aborts the remaining files, which load
/// with the recreated interpreter's deferred reload.
fn load_scripts(lua: &Lua, dir: &Path, events: &Sender<HostEvent>, frame: &TimeoutFrame) {
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
        frame.begin(false, format_args!("loading {name}"));
        match lua.load(text).set_name(name.clone()).exec() {
            Ok(()) => log(events, format!("loaded {name}")),
            Err(err) => {
                if frame.hard_tripped.get() {
                    // The hook logged the reload line already.
                    return;
                }
                log(events, format!("skipped {name}: {err}"));
            }
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
    use std::time::{Duration, Instant};

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

    /// The ceiling for waits that end on a specific line (a timeout
    /// firing, a watcher's reaction): generous next to the ~1s the
    /// hard cap takes, so scheduling slack never flakes the suite.
    const LONG_WAIT: Duration = Duration::from_secs(5);

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

    /// Collect the host's log lines until one matches `wanted`
    /// (everything collected so far, the match included) or `timeout`
    /// passes. The quiet-window drain above cannot bridge the gap
    /// between the soft bound and the hard cap — 900 ms of expected
    /// silence — so the runaway tests wait for the line that ends the
    /// phase instead of waiting for silence.
    fn logs_until(host: &LuaHost, wanted: impl Fn(&str) -> bool, timeout: Duration) -> Vec<String> {
        let mut lines = Vec::new();
        let deadline = Instant::now() + timeout;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match host.events.recv_timeout(remaining) {
                Ok(HostEvent::Log(line)) => {
                    if wanted(&line) {
                        lines.push(line);
                        return lines;
                    }
                    lines.push(line);
                }
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return lines,
            }
        }
        lines
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

    // --- the lifecycle machine (task L2: soft stale, hard reload) ---

    /// A marker file the runaway fixtures check: absent, the fixture
    /// spins; present, it behaves. The interpreter cannot carry state
    /// across a reload (registrations lost by design, spec assumption
    /// 7), so the file is the only thing that survives to tell the
    /// re-run script the runaway was already paid for. Extension-less,
    /// so the scan never tries to load it.
    fn marker(dir: &TempDir) -> PathBuf {
        dir.join("marker")
    }

    #[test]
    fn a_runaway_handler_goes_stale_reloads_and_answers_again() {
        let dir = TempDir::new("runaway");
        let marker = marker(&dir);
        script(
            &dir,
            "runaway.lua",
            &format!(
                "{PORT}\n\
                 local recovered = io.open('{}', 'r')\n\
                 if recovered then recovered:close() end\n\
                 d.on('x', function()\n\
                   if not recovered then while true do end end\n\
                   print('recovered')\n\
                 end)",
                marker.display(),
            ),
        );
        let host = LuaHost::new(&dir.0);
        emit(&host, "x");
        let first = logs_until(
            &host,
            |line| line.contains("ran past the hard cap"),
            Duration::from_secs(5),
        );
        assert_eq!(
            first
                .iter()
                .filter(|line| line.contains("ran past the soft bound"))
                .count(),
            1,
            "the soft bound costs exactly one stale line: {first:?}"
        );
        assert_eq!(
            first
                .iter()
                .filter(|line| line.contains("ran past the hard cap"))
                .count(),
            1,
            "the hard cap costs exactly one reload line: {first:?}"
        );
        let soft = first
            .iter()
            .position(|line| line.contains("ran past the soft bound"))
            .expect("counted above");
        let hard = first
            .iter()
            .position(|line| line.contains("ran past the hard cap"))
            .expect("counted above");
        assert!(
            soft < hard,
            "the stale line lands before the reload line: {first:?}"
        );
        assert!(
            !first.iter().any(|line| line.contains("raised")),
            "the hard cap's abort must not double as a raise line: {first:?}"
        );
        assert!(
            !first.iter().any(|line| line.contains("recovered")),
            "the runaway handler must never finish: {first:?}"
        );
        // The escape hatch: with the runaway paid for, the next
        // dispatch re-runs the registration scripts and succeeds.
        fs::write(&marker, "").expect("write recovery marker");
        emit(&host, "x");
        let second = logs_until_quiet(&host);
        assert!(
            second
                .iter()
                .any(|line| line.contains("loaded runaway.lua")),
            "the reload re-runs the registration scripts: {second:?}"
        );
        assert!(
            second.iter().any(|line| line.contains("recovered")),
            "the recovered handler answers on the next dispatch: {second:?}"
        );
    }

    #[test]
    fn a_handler_under_the_soft_bound_costs_no_line() {
        let dir = TempDir::new("swift");
        script(
            &dir,
            "swift.lua",
            &format!(
                "{PORT}\n\
                 d.on('x', function()\n\
                   local deadline = os.clock() + 0.03\n\
                   while os.clock() < deadline do end\n\
                   print('swift done')\n\
                 end)"
            ),
        );
        let host = LuaHost::new(&dir.0);
        emit(&host, "x");
        let lines = logs_until_quiet(&host);
        assert!(
            lines.iter().any(|line| line.contains("swift done")),
            "the sub-bound handler still runs to completion: {lines:?}"
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("soft bound") || line.contains("hard cap")),
            "a sub-bound handler costs no timeout line: {lines:?}"
        );
    }

    #[test]
    fn a_runaway_load_hits_the_hard_cap_and_the_host_answers_again() {
        let dir = TempDir::new("loopload");
        let marker = marker(&dir);
        script(
            &dir,
            "loopload.lua",
            &format!(
                "if not io.open('{}', 'r') then while true do end end\n\
                 {PORT}\n\
                 d.on('x', function() print('load recovered') end)",
                marker.display(),
            ),
        );
        let host = LuaHost::new(&dir.0);
        emit(&host, "x");
        let first = logs_until(
            &host,
            |line| line.contains("ran past the hard cap"),
            Duration::from_secs(5),
        );
        assert!(
            first
                .iter()
                .any(|line| line.contains("ran past the hard cap") && line.contains("loopload")),
            "a runaway load must hit the hard cap by name: {first:?}"
        );
        assert!(
            !first
                .iter()
                .any(|line| line.contains("ran past the soft bound")),
            "loads carry no stale concept (no result to discard): {first:?}"
        );
        assert!(
            !first.iter().any(|line| line.contains("loaded loopload")),
            "the runaway load never completes: {first:?}"
        );
        fs::write(&marker, "").expect("write recovery marker");
        emit(&host, "x");
        let second = logs_until_quiet(&host);
        assert!(
            second.iter().any(|line| line.contains("loaded loopload")),
            "the reload loads the recovered script: {second:?}"
        );
        assert!(
            second.iter().any(|line| line.contains("load recovered")),
            "the recovered script's handler answers: {second:?}"
        );
    }

    // --- hot reload (task L3: the watcher feed) ---

    #[test]
    fn a_reload_recreates_the_interpreter_and_the_new_handler_answers() {
        let dir = TempDir::new("reload");
        script(
            &dir,
            "gen.lua",
            &format!("{PORT}\nd.on('x', function() print('v1') end)"),
        );
        let host = LuaHost::new(&dir.0);
        emit(&host, "x");
        let first = logs_until(&host, |line| line.contains("v1"), LONG_WAIT);
        assert!(
            first.iter().any(|line| line.contains("v1")),
            "the original handler answers before the edit: {first:?}"
        );
        script(
            &dir,
            "gen.lua",
            &format!("{PORT}\nd.on('x', function() print('v2') end)"),
        );
        host.send(HostCommand::Reload);
        let second = logs_until(&host, |line| line.contains("loaded gen.lua"), LONG_WAIT);
        assert_eq!(
            second
                .iter()
                .filter(|line| line.contains("loaded gen.lua"))
                .count(),
            1,
            "the reload rescans the edited script: {second:?}"
        );
        assert!(
            !second.iter().any(|line| line.contains("host thread up")),
            "the thread persists; only the interpreter changed: {second:?}"
        );
        emit(&host, "x");
        let third = logs_until(&host, |line| line.contains("v2"), LONG_WAIT);
        assert!(
            third.iter().any(|line| line.contains("v2")),
            "the new handler answers after the reload: {third:?}"
        );
        assert!(
            !third.iter().any(|line| line.contains("v1")),
            "the old registration is gone with the old interpreter: {third:?}"
        );
    }

    #[test]
    fn the_watcher_feeds_a_reload_on_dir_changes() {
        let dir = TempDir::new("watch");
        script(&dir, "one.lua", &format!("{PORT}\nprint('one loaded')"));
        let host = LuaHost::new(&dir.0);
        let _watcher = host.watch_plugins_dir().expect("arm the plugins watcher");
        emit(&host, "boot");
        let _ = logs_until(&host, |line| line.contains("one loaded"), LONG_WAIT);
        // A script dropped into the running host's dir: the watcher
        // must queue the Reload and the rescan must load it — its
        // top-level print makes the reload observable without needing
        // a dispatch first.
        script(
            &dir,
            "two.lua",
            &format!("{PORT}\nprint('two loaded')\nd.on('x', function() print('from two') end)"),
        );
        let reloaded = logs_until(&host, |line| line.contains("two loaded"), LONG_WAIT);
        assert!(
            reloaded.iter().any(|line| line.contains("two loaded")),
            "a dropped script lands without restart: {reloaded:?}"
        );
        emit(&host, "x");
        let third = logs_until(&host, |line| line.contains("from two"), LONG_WAIT);
        assert!(
            third.iter().any(|line| line.contains("from two")),
            "the dropped script's handler answers: {third:?}"
        );
    }

    #[test]
    fn a_removed_dir_leaves_the_host_idle_quiet_and_alive() {
        let dir = TempDir::new("gone");
        script(
            &dir,
            "hello.lua",
            &format!("{PORT}\nd.on('x', function() print('hi') end)"),
        );
        let host = LuaHost::new(&dir.0);
        emit(&host, "x");
        // Wait out the dispatch too ("hi"), not just the load line —
        // a stray in-flight print would otherwise pollute the
        // quiet-after-removal assertion below.
        let _ = logs_until(&host, |line| line.contains("hi"), LONG_WAIT);
        fs::remove_dir_all(&dir.0).expect("remove the plugins dir");
        host.send(HostCommand::Reload);
        let after = logs_until_quiet(&host);
        assert!(
            after.is_empty(),
            "a missing dir is the silent first run — no crash, no spam: {after:?}"
        );
        // Alive: a script back in the dir loads and answers on the
        // next reload.
        fs::create_dir_all(&dir.0).expect("recreate the plugins dir");
        script(
            &dir,
            "back.lua",
            &format!("{PORT}\nd.on('x', function() print('back') end)"),
        );
        host.send(HostCommand::Reload);
        let second = logs_until(&host, |line| line.contains("loaded back.lua"), LONG_WAIT);
        assert!(
            second.iter().any(|line| line.contains("loaded back.lua")),
            "the host survives the removed-dir reload: {second:?}"
        );
        emit(&host, "x");
        let third = logs_until(&host, |line| line.contains("back"), LONG_WAIT);
        assert!(
            third.iter().any(|line| line.contains("back")),
            "the recovered script's handler answers: {third:?}"
        );
    }
}
