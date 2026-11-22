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
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use notify::Watcher as _;

use crate::core::status::{ScriptSide, SegmentAnswer, StatusInputs};
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

/// The reserved bus event name (status-segments spec, proposed at
/// intake): a scripted `emit` of this name never dispatches — the
/// host translates it into an outbound [`HostEvent::SegmentsDirty`]
/// the tick drains into a requery, so a segment can pull its own
/// refresh after computing new state.
pub const STATUS_REFRESH_EVENT: &str = "status-refresh";

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
    /// Re-evaluate every registered script segment against the
    /// current band inputs (status-segments spec). One dispatch
    /// covers all segments — one interpreter trip, the soft/hard
    /// bounds over the whole trip — and the reply lands as
    /// [`HostEvent::SegmentValues`] matched by token: stale until
    /// then, and the band keeps each segment's prior text (the
    /// runtime contract, never block).
    QuerySegments {
        /// The drain-side match key; the reply carries it back.
        token: u64,
        /// The [`StatusInputs`] mirror ([`status_ctx`]) — the ctx
        /// table every handler receives.
        ctx: serde_json::Value,
    },
}

/// One observation out of the host thread. v1 carried only the log;
/// the query surfaces' replies and reports join as new arms with the
/// modules that ask for them (the same closed-extension rule).
#[derive(Debug)]
pub enum HostEvent {
    /// One `dropterm:` evidence line: host up, each script loaded,
    /// each error with its file, each script's forwarded `print`.
    Log(String),
    /// Every registered segment's answers to one
    /// [`HostCommand::QuerySegments`], matched by token, in
    /// registration order. A raising or unusable answer omits its id
    /// — the asking surface keeps that segment's prior value; a nil
    /// answer is carried as [`SegmentAnswer::Hidden`], a fresh
    /// decision to render nothing. `stale` marks a trip that
    /// outlived the soft bound: the drain drops the whole reply
    /// (stale-degrade, never block).
    SegmentValues {
        token: u64,
        stale: bool,
        values: Vec<SegmentAnswer>,
    },
    /// A script pulled its own refresh: [`STATUS_REFRESH_EVENT`],
    /// translated — never dispatched — into this outbound request
    /// the tick drains into a requery.
    SegmentsDirty,
    /// The live script-segment id set, in registration order, at
    /// every load/reload (empty included — a reload into an empty
    /// dir clears the asking surface's registry with it). The
    /// out-arm shape the later script surfaces reuse: one report at
    /// every registry change, tab-titles and script-commands
    /// extending the same arm.
    RegistrySync { segments: Vec<String> },
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
        spawn_from_gate(&self.dormant, &self.plugins_dir);
    }

    /// Watch the plugins dir (non-recursive, the config watcher's
    /// shape) and queue a [`HostCommand::Reload`] on any event (task
    /// L3): the watcher feeds the host's command channel — never the
    /// UI's event channel. A dir event is a first work item (spec
    /// assumption 4: "first event to deliver, first query, first
    /// watcher wake"), so the wake spawns the host if it has never
    /// spawned — the owning wake's scan is the load, with no
    /// duplicate beside it. Returns the watcher guard (dropping it
    /// stops watching); a dir that cannot be watched is `None` —
    /// missing is the silent first-run norm (the theme-files posture),
    /// anything else costs one line.
    pub fn watch_plugins_dir(&self) -> Option<notify::RecommendedWatcher> {
        let commands = self
            .commands
            .lock()
            .expect("the lua host gate is only contended briefly")
            .clone();
        let dormant = Arc::clone(&self.dormant);
        let plugins_dir = self.plugins_dir.clone();
        let mut watcher =
            match notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
                // Only content-changing events reload. notify's inotify
                // mask includes OPEN, so the host's own scan — one
                // read_dir plus a read_to_string per script — would feed
                // itself a Reload per read and loop forever (found live:
                // a self-sustaining rescan flood pinned the CPU and
                // starved the quiet-window drain). Reads are Access
                // events; writes, drops, and removals are
                // Create/Modify/Remove.
                let changes_content = match event {
                    Ok(event) => matches!(
                        event.kind,
                        notify::EventKind::Create(_)
                            | notify::EventKind::Modify(_)
                            | notify::EventKind::Remove(_)
                    ),
                    Err(_) => false, // a watcher error is not a change; the next real event reloads
                };
                if changes_content {
                    spawn_from_gate(&dormant, &plugins_dir);
                    let _ = commands.send(HostCommand::Reload);
                }
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

/// Take the dormant channel halves and spawn the host thread — the
/// shared spawner for the first [`LuaHost::send`] and the watcher's
/// wake (both are first work items, spec assumption 4). A no-op once
/// spawned: the gate is spent.
fn spawn_from_gate(dormant: &Arc<Mutex<DormantChannels>>, plugins_dir: &Path) {
    let Some((commands, events)) = dormant
        .lock()
        .expect("the lua host gate is only contended briefly")
        .take()
    else {
        return;
    };
    let plugins_dir = plugins_dir.to_owned();
    let spawned = thread::Builder::new()
        .name("dropterm-lua-host".to_owned())
        .spawn(move || host_thread(&plugins_dir, &commands, &events));
    if let Err(err) = spawned {
        println!("dropterm: plugins: host thread failed to spawn: {err}");
    }
}

/// The host thread's whole life: generations of interpreter, each
/// built, ported, bounded, then served strictly in arrival order — one
/// thread, one interpreter at a time, no dispatch racing another (spec
/// assumption 8). A generation serves from a scanned state: the
/// watcher's `Reload` on a live generation recreates and rescans
/// eagerly (the host is otherwise idle — the rescan's lines land at
/// edit time, not at some later dispatch); a generation born mid-work
/// (the spawn, or a hard-cap recreate) scans before its first
/// dispatch, so a load-time runaway costs one interpreter cycle per
/// command, never a free-spinning reload loop. Returns when the
/// handle drops (the command channel dies) and drops the interpreter
/// on the way out.
fn host_thread(plugins_dir: &Path, commands: &Receiver<HostCommand>, events: &Sender<HostEvent>) {
    let frame = Rc::new(TimeoutFrame::new());
    let mut first_generation = true;
    // Set by a `Reload` on a live generation: the next generation
    // loads eagerly at build (the rescan is the work).
    let mut rescan_now = false;
    // An Emit found while folding a Reload burst: served first on
    // the reloaded generation — the host is serial, nothing
    // reorders, nothing drops.
    let mut held: Option<HostCommand> = None;
    loop {
        let Some(generation) = Generation::build(events, &frame) else {
            return;
        };
        if first_generation {
            log(events, "host thread up");
            first_generation = false;
        }
        // Whether this generation's scripts have been loaded: a fresh
        // generation scans before it first serves, so the spawn, the
        // hard cap, and the wake all share one lazy path.
        let mut scanned = false;
        if rescan_now {
            load_scripts(
                &generation.lua,
                &generation.segments,
                plugins_dir,
                events,
                &frame,
            );
            // The eager rescan IS this generation's load — mark it
            // scanned so the first dispatch never loads a second
            // copy beside it (found at status-segments SG1: a doubled
            // load doubled every registration).
            scanned = true;
            rescan_now = false;
            if frame.hard_tripped.get() {
                // The rescan's own load ran away: recreate WITHOUT the
                // eager rescan — the fresh generation scans before its
                // next dispatch (the spin guard).
                continue;
            }
        }
        let mut reload_requested = false;
        while let Some(command) = held.take().or_else(|| commands.recv().ok()) {
            match command {
                HostCommand::Emit { event, payload } => {
                    if !scanned {
                        load_scripts(
                            &generation.lua,
                            &generation.segments,
                            plugins_dir,
                            events,
                            &frame,
                        );
                        scanned = true;
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
                HostCommand::QuerySegments { token, ctx } => {
                    if !scanned {
                        load_scripts(
                            &generation.lua,
                            &generation.segments,
                            plugins_dir,
                            events,
                            &frame,
                        );
                        scanned = true;
                        if frame.hard_tripped.get() {
                            break;
                        }
                    }
                    query_segments(&generation, token, &ctx, events, &frame);
                    if frame.hard_tripped.get() {
                        break;
                    }
                }
                HostCommand::Reload => {
                    // One edit lands as a burst of events (a
                    // truncate-write is two Modifys on inotify): fold
                    // the burst into this one reload — the rescan
                    // reads current disk, so every queued follower is
                    // redundant, and an Emit hiding among them is
                    // held and served on the reloaded generation. The
                    // short window catches followers still crossing
                    // the channel.
                    loop {
                        match commands.recv_timeout(Duration::from_millis(10)) {
                            Ok(HostCommand::Reload) => {}
                            Ok(emit @ HostCommand::Emit { .. }) => {
                                held = Some(emit);
                                break;
                            }
                            Ok(query @ HostCommand::QuerySegments { .. }) => {
                                held = Some(query);
                                break;
                            }
                            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                                break;
                            }
                        }
                    }
                    if scanned {
                        // A live generation: recreate whole (the
                        // picked reload unit, task L3).
                        reload_requested = true;
                        break;
                    }
                    // The spawning wake itself: this generation has
                    // never loaded anything, so the scan IS the
                    // reload — no duplicate beside it.
                    load_scripts(
                        &generation.lua,
                        &generation.segments,
                        plugins_dir,
                        events,
                        &frame,
                    );
                    scanned = true;
                    if frame.hard_tripped.get() {
                        break;
                    }
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
/// two registries — the event-bus table behind `dropterm.on`/`emit`
/// and the segment registry behind `dropterm.status_segment`.
/// Generation 1 is born at spawn; every later one is a hard-cap
/// recreate — fresh state, no registrations (spec assumption 7).
struct Generation {
    lua: Lua,
    registry: Table,
    segments: Table,
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
        let (registry, segments) = match install_port(&lua, events, frame) {
            Ok(registries) => registries,
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
        Some(Generation {
            lua,
            registry,
            segments,
        })
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
/// registry-table design), the segment registry behind
/// `dropterm.status_segment` (status-segments spec), the module
/// served to `require('dropterm')` through `package.loaded` (the
/// stdlib's own module cache — no require-path widening), and the
/// `print` forwarder. Returns both registries for host-side use.
fn install_port(
    lua: &Lua,
    events: &Sender<HostEvent>,
    frame: &Rc<TimeoutFrame>,
) -> mlua::Result<(Table, Table)> {
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
        if event == STATUS_REFRESH_EVENT {
            // The reserved name is translated, not dispatched: the
            // dirty request the tick drains into a requery
            // (status-segments spec).
            let _ = emit_events.send(HostEvent::SegmentsDirty);
            return Ok(());
        }
        dispatch(&emit_events, &emit_registry, &event, &payload, &emit_frame);
        Ok(())
    })?;

    // The segment registry: an array of `{name, handler}` entries —
    // an array, because registration order is render order and a
    // keyed table would lose it. A repeated name replaces its slot's
    // handler in place: one live id, one answer, the first position
    // kept.
    let segment_table = lua.create_table()?;
    let segments_for_register = segment_table.clone();
    let status_segment = lua.create_function(move |lua, (name, handler): (String, Function)| {
        for slot in 1..=segments_for_register.raw_len() {
            let entry: Table = segments_for_register.get(slot)?;
            if entry.get::<String>("name")? == name {
                entry.set("handler", handler)?;
                return Ok(());
            }
        }
        let entry = lua.create_table()?;
        entry.set("name", name)?;
        entry.set("handler", handler)?;
        segments_for_register.set(segments_for_register.raw_len() + 1, entry)?;
        Ok(())
    })?;

    let module = lua.create_table()?;
    module.set("on", on)?;
    module.set("emit", emit)?;
    module.set("status_segment", status_segment)?;
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.set("dropterm", module)?;

    install_print(lua, events)?;
    Ok((registry, segment_table))
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
/// with the recreated interpreter's deferred reload. Every completed
/// pass — a reload into an empty dir included — ends with the
/// registry-sync report (status-segments spec): the live id set is
/// the asking surface's source of truth.
fn load_scripts(
    lua: &Lua,
    segments: &Table,
    dir: &Path,
    events: &Sender<HostEvent>,
    frame: &TimeoutFrame,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            sync_registry(segments, events);
            return;
        }
        Err(err) => {
            log(events, format!("cannot read {}: {err}", dir.display()));
            sync_registry(segments, events);
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
                    // The hook logged the reload line already; this
                    // generation is dying — its successor's load
                    // carries the next sync.
                    return;
                }
                log(events, format!("skipped {name}: {err}"));
            }
        }
    }
    sync_registry(segments, events);
}

/// Report the live segment registry — the id set in registration
/// order — at every load/reload (status-segments spec): the asking
/// surface's source of truth for what renders, empty after a reload
/// into an empty dir (the stale slots clear with it).
fn sync_registry(segments: &Table, events: &Sender<HostEvent>) {
    let mut ids = Vec::new();
    for slot in 1..=segments.raw_len() {
        match segments
            .get::<Table>(slot)
            .and_then(|entry| entry.get::<String>("name"))
        {
            Ok(name) => ids.push(name),
            Err(err) => log(
                events,
                format!("the segment registry would not read: {err}"),
            ),
        }
    }
    let _ = events.send(HostEvent::RegistrySync { segments: ids });
}

/// Evaluate every registered segment handler once (status-segments
/// spec): one interpreter trip under one frame — the soft/hard bounds
/// cover the trip whole — with the answers in registration order. A
/// handler that raises, or answers without a usable shape, costs one
/// line and omits its id: the asking surface keeps that segment's
/// prior value. Past the soft bound the reply is flagged stale — the
/// drain drops it whole; a hard-cap abort sends no reply at all.
fn query_segments(
    generation: &Generation,
    token: u64,
    ctx: &serde_json::Value,
    events: &Sender<HostEvent>,
    frame: &Rc<TimeoutFrame>,
) {
    let ctx = match generation.lua.to_value(ctx) {
        Ok(value) => value,
        Err(err) => {
            // Unreachable for JSON payloads; honest if it ever fires:
            // no reply, prior values kept.
            log(events, format!("the segment ctx would not convert: {err}"));
            return;
        }
    };
    frame.begin(true, format_args!("the segment query"));
    let mut values = Vec::new();
    for slot in 1..=generation.segments.raw_len() {
        let Ok(entry) = generation.segments.get::<Table>(slot) else {
            continue;
        };
        let Ok(name) = entry.get::<String>("name") else {
            continue;
        };
        let Ok(handler) = entry.get::<Function>("handler") else {
            continue;
        };
        let answer = match handler.call::<Option<Table>>(ctx.clone()) {
            Ok(answer) => answer,
            Err(err) => {
                if frame.hard_tripped.get() {
                    // The hook logged the reload line; the abort
                    // unwinds — no reply, the slots keep their priors.
                    return;
                }
                log(events, format!("segment '{name}' raised: {err}"));
                continue;
            }
        };
        let Some(answer) = answer else {
            values.push(SegmentAnswer::Hidden { id: name });
            continue;
        };
        match read_answer(&answer, &name) {
            Ok(value) => values.push(value),
            Err(err) => {
                log(
                    events,
                    format!("segment '{name}' answered without a usable value: {err}"),
                );
            }
        }
    }
    let stale = frame.started.get().elapsed() >= SOFT_BOUND;
    let _ = events.send(HostEvent::SegmentValues {
        token,
        stale,
        values,
    });
}

/// Read one handler's answer table: `{ text = …, side = "left" |
/// "right" }` — both fields required, the side's words the only two
/// accepted. Anything else is an error the caller logs once.
fn read_answer(answer: &Table, name: &str) -> mlua::Result<SegmentAnswer> {
    let text: String = answer.get("text")?;
    let side: String = answer.get("side")?;
    let side = match side.as_str() {
        "left" => ScriptSide::Left,
        "right" => ScriptSide::Right,
        other => {
            return Err(mlua::Error::RuntimeError(format!(
                "the side '{other}' is neither left nor right"
            )));
        }
    };
    Ok(SegmentAnswer::Shown {
        id: name.to_owned(),
        side,
        text,
    })
}

/// Build the segment query's ctx payload: the [`StatusInputs`]
/// mirror (status-segments spec assumption 1) — every band input the
/// tick already gathers, and nothing more (no engine read rides a
/// query). Absent inputs are absent keys; durations cross as seconds
/// (Lua numbers); the engine's standing reports carry their
/// protocol's own words. The scheme's red pair does not cross: v1
/// segments are plain text (assumption 2) — render tokens stay
/// shell-side.
pub fn status_ctx(inputs: &StatusInputs) -> serde_json::Value {
    use crate::core::terminal::{ProgramState, ProgressState};
    use serde_json::json;

    let grid =
        |pair: Option<(u16, u16)>| pair.map(|(cols, rows)| json!({ "cols": cols, "rows": rows }));
    let mut ctx = serde_json::Map::new();
    let mut put = |key: &str, value: serde_json::Value| {
        ctx.insert(key.to_owned(), value);
    };
    if let Some(home) = inputs.home {
        put("home", json!(home.display().to_string()));
    }
    if let Some(cwd) = inputs.cwd {
        put("cwd", json!(cwd.display().to_string()));
    }
    if let Some(report) = inputs.reported_cwd.as_ref() {
        put(
            "reported_cwd",
            json!({ "host": report.host.clone(), "path": report.path.clone() }),
        );
    }
    if let Some(hostname) = inputs.hostname {
        put("hostname", json!(hostname));
    }
    put("shell", json!(inputs.shell));
    if let Some(profile) = inputs.profile_name {
        put("profile_name", json!(profile));
    }
    put("zoom_adj", json!(inputs.zoom_adj));
    put("zoom_step_mul_100", json!(inputs.zoom_step_mul_100));
    if let Some(foreground) = inputs.foreground {
        put("foreground", json!(foreground));
    }
    if let Some(branch) = inputs.branch {
        put("branch", json!(branch));
    }
    if let Some(grid) = grid(inputs.grid) {
        put("grid", grid);
    }
    if let Some(spawn_grid) = grid(inputs.spawn_grid) {
        put("spawn_grid", spawn_grid);
    }
    if let Some((index, total)) = inputs.tab_position {
        put("tab_position", json!({ "index": index, "total": total }));
    }
    if let Some(clock) = inputs.clock {
        put("clock", json!(clock));
    }
    if let Some(elapsed) = inputs.running_elapsed {
        put("running_elapsed", json!(elapsed.as_secs_f64()));
    }
    if let Some((name, duration)) = inputs.frozen {
        put(
            "frozen",
            json!({ "name": name, "duration_seconds": duration.as_secs_f64() }),
        );
    }
    if let Some(exit_status) = inputs.exit_status {
        put("exit_status", json!(exit_status));
    }
    if let Some(scrolled_back) = inputs.scrolled_back {
        put("scrolled_back", json!(scrolled_back));
    }
    if let Some(pasted) = inputs.pasted {
        put("pasted", json!(pasted));
    }
    if let Some(progress) = inputs.progress {
        let state = match progress.state {
            ProgressState::InProgress => "in_progress",
            ProgressState::Error => "error",
            ProgressState::Indeterminate => "indeterminate",
            ProgressState::Paused => "paused",
        };
        put(
            "progress",
            json!({ "state": state, "value": progress.value }),
        );
    }
    if let Some(program) = inputs.program_status {
        let state = match program.state {
            ProgramState::Idle => "idle",
            ProgramState::Working => "working",
            ProgramState::Done => "done",
            ProgramState::Blocked => "blocked",
            ProgramState::Error => "error",
        };
        put(
            "program_status",
            json!({ "state": state, "progress": program.progress }),
        );
    }
    if let Some(ssh_destination) = inputs.ssh_destination {
        put("ssh_destination", json!(ssh_destination));
    }
    // A bool fact the band always has — always present, like `shell`.
    put("missed_activity", json!(inputs.missed_activity));
    serde_json::Value::Object(ctx)
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
    /// channel, never a fixed sleep. Non-log events (the query
    /// surfaces' replies) pass through unread.
    fn logs_until_quiet(host: &LuaHost) -> Vec<String> {
        let mut lines = Vec::new();
        loop {
            match host.events.recv_timeout(Duration::from_millis(200)) {
                Ok(HostEvent::Log(line)) => lines.push(line),
                Ok(_) => {}
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
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return lines,
            }
        }
        lines
    }

    /// Collect the host's events until one matches `wanted` (the
    /// match included) or `timeout` passes — the query surfaces'
    /// companion to [`logs_until`]: a reply is read by waiting for
    /// the reply, never for silence.
    fn events_until(
        host: &LuaHost,
        wanted: impl Fn(&HostEvent) -> bool,
        timeout: Duration,
    ) -> Vec<HostEvent> {
        let mut events = Vec::new();
        let deadline = Instant::now() + timeout;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match host.events.recv_timeout(remaining) {
                Ok(event) => {
                    if wanted(&event) {
                        events.push(event);
                        return events;
                    }
                    events.push(event);
                }
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return events,
            }
        }
        events
    }

    /// Collect the host's events until it goes quiet (200 ms with no
    /// event) — the whole reaction, replies and lines together.
    fn events_until_quiet(host: &LuaHost) -> Vec<HostEvent> {
        let mut events = Vec::new();
        loop {
            match host.events.recv_timeout(Duration::from_millis(200)) {
                Ok(event) => events.push(event),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return events,
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
    fn a_watcher_wake_spawns_the_host_and_loads_once() {
        let dir = TempDir::new("wake");
        script(
            &dir,
            "hello.lua",
            &format!("{PORT}\nprint('awake')\nd.on('x', function() print('answered') end)"),
        );
        let host = LuaHost::new(&dir.0);
        let _watcher = host.watch_plugins_dir().expect("arm the plugins watcher");
        assert!(
            !host.spawned(),
            "arming the watcher alone spawns nothing — the lazy pin holds"
        );
        // The wake: a dir event is the first work item (spec
        // assumption 4 — "first event to deliver, first query, first
        // watcher wake"), so the touch spawns the host, scans, and
        // loads. Rewriting the file with identical content is still
        // an event; the pin demands the load happens EXACTLY once
        // (the spawning wake is the scan, not a duplicate beside it).
        script(
            &dir,
            "hello.lua",
            &format!("{PORT}\nprint('awake')\nd.on('x', function() print('answered') end)"),
        );
        // Wait for the load line (a script's own print lands before
        // it), then drain the quiet tail so a duplicate rescan — if
        // one existed — would be collected rather than cut off.
        let mut lines = logs_until(&host, |line| line.contains("loaded hello.lua"), LONG_WAIT);
        lines.extend(logs_until_quiet(&host));
        assert!(host.spawned(), "the wake must spawn the host: {lines:?}");
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("loaded hello.lua"))
                .count(),
            1,
            "the spawning wake is the one scan — no duplicate reload beside it: {lines:?}"
        );
        assert_eq!(
            lines.iter().filter(|line| line.contains("awake")).count(),
            1,
            "the script's print lands once: {lines:?}"
        );
        emit(&host, "x");
        let second = logs_until(&host, |line| line.contains("answered"), LONG_WAIT);
        assert!(
            second.iter().any(|line| line.contains("answered")),
            "the woken host serves later dispatches: {second:?}"
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

    // --- script segments (status-segments spec, task SG1) ---

    /// One query's ctx, the shape the shell assembles from the tick's
    /// `StatusInputs` — the minimal fixture the echoing handler reads.
    fn segment_ctx() -> serde_json::Value {
        serde_json::json!({ "cwd": "/home/u/src" })
    }

    /// Send one segment query under `token`.
    fn query(host: &LuaHost, token: u64) {
        host.send(HostCommand::QuerySegments {
            token,
            ctx: segment_ctx(),
        });
    }

    #[test]
    fn status_segment_registration_syncs_and_answers_a_query() {
        let dir = TempDir::new("segments");
        script(
            &dir,
            "segs.lua",
            &format!(
                "{PORT}\n\
                 d.status_segment('net', function(ctx)\n\
                   return {{ text = 'cwd is ' .. tostring(ctx.cwd), side = 'right' }}\n\
                 end)\n\
                 d.status_segment('git', function()\n\
                   return {{ text = 'branch ok', side = 'left' }}\n\
                 end)"
            ),
        );
        let host = LuaHost::new(&dir.0);
        query(&host, 7);
        let events = events_until(
            &host,
            |event| matches!(event, HostEvent::SegmentValues { token: 7, .. }),
            LONG_WAIT,
        );
        // The load reports the live registry, in registration order.
        assert!(
            events.iter().any(|event| matches!(
                event,
                HostEvent::RegistrySync { segments }
                    if segments == &["net".to_owned(), "git".to_owned()]
            )),
            "the load must report the live segment ids in order: {events:?}"
        );
        // The reply carries every segment's answer, in registration
        // order, matched by token — and the ctx table crossed: the
        // echoing handler answered with the queried cwd.
        let reply = events
            .iter()
            .find_map(|event| match event {
                HostEvent::SegmentValues {
                    token: 7,
                    stale,
                    values,
                } => Some((*stale, values.clone())),
                _ => None,
            })
            .expect("the reply lands");
        assert!(!reply.0, "a swift query replies fresh: {events:?}");
        assert_eq!(
            reply.1,
            vec![
                SegmentAnswer::Shown {
                    id: "net".to_owned(),
                    side: ScriptSide::Right,
                    text: "cwd is /home/u/src".to_owned(),
                },
                SegmentAnswer::Shown {
                    id: "git".to_owned(),
                    side: ScriptSide::Left,
                    text: "branch ok".to_owned(),
                },
            ],
            "the answers land in registration order: {events:?}"
        );
    }

    #[test]
    fn a_nil_answer_hides_a_raise_omits_and_a_bad_side_costs_one_line() {
        let dir = TempDir::new("answers");
        script(
            &dir,
            "answers.lua",
            &format!(
                "{PORT}\n\
                 d.status_segment('good', function()\n\
                   return {{ text = 'fine', side = 'right' }}\n\
                 end)\n\
                 d.status_segment('mute', function() return nil end)\n\
                 d.status_segment('boom', function() error('kaboom') end)\n\
                 d.status_segment('odd', function()\n\
                   return {{ text = 'x', side = 'up' }}\n\
                 end)"
            ),
        );
        let host = LuaHost::new(&dir.0);
        query(&host, 1);
        let events = events_until(
            &host,
            |event| matches!(event, HostEvent::SegmentValues { token: 1, .. }),
            LONG_WAIT,
        );
        let values = events
            .iter()
            .find_map(|event| match event {
                HostEvent::SegmentValues {
                    token: 1,
                    stale: _,
                    values,
                } => Some(values.clone()),
                _ => None,
            })
            .expect("the reply lands");
        assert_eq!(
            values,
            vec![
                SegmentAnswer::Shown {
                    id: "good".to_owned(),
                    side: ScriptSide::Right,
                    text: "fine".to_owned(),
                },
                SegmentAnswer::Hidden {
                    id: "mute".to_owned()
                },
            ],
            "a nil answer is carried (a fresh hide); a raise or an unusable side omits the id: {events:?}"
        );
        // One line per failure, by name — and the host answers again.
        let lines: Vec<String> = events
            .iter()
            .filter_map(|event| match event {
                HostEvent::Log(line) => Some(line.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            lines.iter().filter(|line| line.contains("boom")).count(),
            1,
            "the raising handler costs exactly one line: {lines:?}"
        );
        assert_eq!(
            lines.iter().filter(|line| line.contains("odd")).count(),
            1,
            "the unusable side costs exactly one line: {lines:?}"
        );
        query(&host, 2);
        let second = events_until(
            &host,
            |event| matches!(event, HostEvent::SegmentValues { token: 2, .. }),
            LONG_WAIT,
        );
        assert!(
            second.iter().any(|event| matches!(
                event,
                HostEvent::SegmentValues { values, .. }
                    if values.iter().any(|answer| matches!(
                        answer,
                        SegmentAnswer::Shown { id, text, .. }
                            if id == "good" && text == "fine"
                    ))
            )),
            "the host still answers after the failures: {second:?}"
        );
    }

    #[test]
    fn the_registry_sync_rereports_after_a_reload() {
        let dir = TempDir::new("resync");
        script(
            &dir,
            "seg.lua",
            &format!(
                "{PORT}\n\
                 d.status_segment('net', function()\n\
                   return {{ text = 'v1', side = 'right' }}\n\
                 end)"
            ),
        );
        let host = LuaHost::new(&dir.0);
        query(&host, 1);
        let first = events_until(
            &host,
            |event| matches!(event, HostEvent::SegmentValues { token: 1, .. }),
            LONG_WAIT,
        );
        assert!(
            first.iter().any(|event| matches!(
                event,
                HostEvent::RegistrySync { segments } if segments == &["net".to_owned()]
            )),
            "the first load reports ['net']: {first:?}"
        );
        script(
            &dir,
            "seg.lua",
            &format!(
                "{PORT}\n\
                 d.status_segment('git', function()\n\
                   return {{ text = 'v2', side = 'left' }}\n\
                 end)"
            ),
        );
        host.send(HostCommand::Reload);
        let reloaded = events_until(
            &host,
            |event| {
                matches!(
                    event,
                    HostEvent::RegistrySync { segments } if segments == &["git".to_owned()]
                )
            },
            LONG_WAIT,
        );
        assert!(
            reloaded
                .iter()
                .filter(|event| matches!(event, HostEvent::RegistrySync { .. }))
                .all(|event| matches!(
                    event,
                    HostEvent::RegistrySync { segments } if segments == &["git".to_owned()]
                )),
            "no sync ever reports a doubled registration: {reloaded:?}"
        );
        // The reloaded registry answers with the new segment alone.
        query(&host, 2);
        let second = events_until(
            &host,
            |event| matches!(event, HostEvent::SegmentValues { token: 2, .. }),
            LONG_WAIT,
        );
        let values = second
            .iter()
            .find_map(|event| match event {
                HostEvent::SegmentValues {
                    token: 2, values, ..
                } => Some(values.clone()),
                _ => None,
            })
            .expect("the reloaded host answers");
        assert_eq!(
            values.as_slice(),
            &[SegmentAnswer::Shown {
                id: "git".to_owned(),
                side: ScriptSide::Left,
                text: "v2".to_owned(),
            }],
            "only the new registration answers, exactly once: {second:?}"
        );
    }

    #[test]
    fn a_scripted_status_refresh_emit_lands_as_a_segments_dirty_request() {
        let dir = TempDir::new("dirty");
        script(
            &dir,
            "pull.lua",
            &format!(
                "{PORT}\n\
                 d.status_segment('net', function()\n\
                   return {{ text = 'x', side = 'right' }}\n\
                 end)\n\
                 d.emit('status-refresh', {{}})\n\
                 d.emit('unrelated', {{}})"
            ),
        );
        let host = LuaHost::new(&dir.0);
        // The emits fire at load, and the load needs a first command
        // under the lazy pin; the drain to quiet collects the whole
        // reaction so a second dirty (if one existed) would be
        // collected, not cut off.
        emit(&host, "boot");
        let all = events_until_quiet(&host);
        assert_eq!(
            all.iter()
                .filter(|event| matches!(event, HostEvent::SegmentsDirty))
                .count(),
            1,
            "the reserved emit translates into exactly one dirty request: {all:?}"
        );
    }

    #[test]
    fn a_query_past_the_soft_bound_replies_stale() {
        let dir = TempDir::new("stale-query");
        // The slow handler sleeps wall-clock time (os.clock spins
        // count the whole process's CPU — under the parallel suite
        // other threads would advance that clock and the trip would
        // flake short), then spins a few instructions so the debug
        // hook gets its look past the bound.
        script(
            &dir,
            "slow.lua",
            &format!(
                "{PORT}\n\
                 d.status_segment('slow', function()\n\
                   io.popen('sleep 0.2'):close()\n\
                   local i = 0\n\
                   while i < 200000 do i = i + 1 end\n\
                   return {{ text = 'late', side = 'right' }}\n\
                 end)"
            ),
        );
        let host = LuaHost::new(&dir.0);
        query(&host, 1);
        let events = events_until(
            &host,
            |event| matches!(event, HostEvent::SegmentValues { token: 1, .. }),
            LONG_WAIT,
        );
        let reply = events
            .iter()
            .find_map(|event| match event {
                HostEvent::SegmentValues {
                    token: 1,
                    stale,
                    values,
                } => Some((stale, values)),
                _ => None,
            })
            .expect("the stale reply still lands");
        assert!(
            *reply.0,
            "a trip past the soft bound replies stale: {events:?}"
        );
        assert!(
            reply.1.iter().any(|answer| matches!(
                answer,
                SegmentAnswer::Shown { id, text, .. } if id == "slow" && text == "late"
            )),
            "the stale reply carries its values; the drain side decides to drop them: {events:?}"
        );
        assert_eq!(
            events
                .iter()
                .filter_map(|event| match event {
                    HostEvent::Log(line) => Some(line),
                    _ => None,
                })
                .filter(|line| line.contains("ran past the soft bound"))
                .count(),
            1,
            "the stale trip costs exactly one line"
        );
    }

    #[test]
    fn segment_values_land_in_the_band_view_model_ordered() {
        use crate::core::status::{ScriptSegmentValue, StatusSegments};

        fn eight_px(text: &str) -> f32 {
            #[expect(clippy::cast_precision_loss, reason = "test strings, exact in f32")]
            let width = text.chars().count() as f32 * 8.0;
            width
        }
        let dir = TempDir::new("compose");
        script(
            &dir,
            "pair.lua",
            &format!(
                "{PORT}\n\
                 d.status_segment('net', function()\n\
                   return {{ text = 'net:1.2M', side = 'right' }}\n\
                 end)\n\
                 d.status_segment('git', function()\n\
                   return {{ text = 'git:1', side = 'left' }}\n\
                 end)"
            ),
        );
        let host = LuaHost::new(&dir.0);
        query(&host, 3);
        let events = events_until(
            &host,
            |event| matches!(event, HostEvent::SegmentValues { token: 3, .. }),
            LONG_WAIT,
        );
        let values = events
            .iter()
            .find_map(|event| match event {
                HostEvent::SegmentValues {
                    token: 3, values, ..
                } => Some(values.clone()),
                _ => None,
            })
            .expect("the reply lands");
        // The drain side's apply: every Shown answer becomes a slot —
        // the shape SG2's shell state keeps, proven here against the
        // real reply.
        let mut segments = StatusSegments {
            clock: Some("14:32".to_owned()),
            ..StatusSegments::default()
        };
        for answer in values {
            if let SegmentAnswer::Shown { id, side, text } = answer {
                segments.script.push(ScriptSegmentValue {
                    id,
                    side,
                    text: Some(text),
                });
            }
        }
        let vm = crate::core::status::band_vm(&segments, 2000.0, &eight_px, None, None, None);
        let laid: Vec<(&str, &str)> = vm
            .script_rows
            .iter()
            .map(|row| (row.id.as_str(), row.text.as_str()))
            .collect();
        assert_eq!(
            laid,
            vec![("git", "git:1"), ("net", "net:1.2M")],
            "the reply's values render: the left group before the right, per side in slot order"
        );
    }

    #[test]
    fn status_ctx_mirrors_the_pinned_status_inputs_fields() {
        use crate::core::status::StatusInputs;
        use crate::core::terminal::{
            ProgramState, ProgramStatus, ProgressState, ReportedCwd, ReportedProgress,
        };
        let full = StatusInputs {
            home: Some(Path::new("/home/u")),
            cwd: Some(Path::new("/home/u/src")),
            reported_cwd: Some(ReportedCwd {
                host: Some("other".to_owned()),
                path: "/srv".to_owned(),
            }),
            hostname: Some("here"),
            shell: "/usr/bin/fish",
            profile_name: Some("dev"),
            zoom_adj: 2,
            zoom_step_mul_100: 125,
            foreground: Some("cargo"),
            branch: Some("main"),
            grid: Some((100, 24)),
            spawn_grid: Some((80, 24)),
            tab_position: Some((2, 5)),
            clock: Some("14:32"),
            running_elapsed: Some(Duration::from_millis(41_500)),
            frozen: Some(("cargo", Duration::from_secs(161))),
            exit_status: Some(3),
            scrolled_back: Some(12),
            pasted: Some(2),
            progress: Some(ReportedProgress {
                state: ProgressState::InProgress,
                value: Some(40),
                refreshed_at: Instant::now(),
            }),
            program_status: Some(ProgramStatus {
                state: ProgramState::Blocked,
                progress: Some(55),
            }),
            ssh_destination: Some("web-prod"),
            missed_activity: true,
            attention_red: None,
            attention_bright_red: None,
        };
        assert_eq!(
            status_ctx(&full),
            serde_json::json!({
                "home": "/home/u",
                "cwd": "/home/u/src",
                "reported_cwd": { "host": "other", "path": "/srv" },
                "hostname": "here",
                "shell": "/usr/bin/fish",
                "profile_name": "dev",
                "zoom_adj": 2,
                "zoom_step_mul_100": 125,
                "foreground": "cargo",
                "branch": "main",
                "grid": { "cols": 100, "rows": 24 },
                "spawn_grid": { "cols": 80, "rows": 24 },
                "tab_position": { "index": 2, "total": 5 },
                "clock": "14:32",
                "running_elapsed": 41.5,
                "frozen": { "name": "cargo", "duration_seconds": 161.0 },
                "exit_status": 3,
                "scrolled_back": 12,
                "pasted": 2,
                "progress": { "state": "in_progress", "value": 40 },
                "program_status": { "state": "blocked", "progress": 55 },
                "ssh_destination": "web-prod",
                "missed_activity": true,
            }),
            "the ctx is the StatusInputs mirror — the pinned field list"
        );
        // Absent inputs mirror as absent keys: the minimal-payload
        // rule (nothing stands in for a fact the band does not have).
        let bare = StatusInputs {
            home: None,
            cwd: None,
            reported_cwd: None,
            hostname: None,
            shell: "",
            profile_name: None,
            zoom_adj: 0,
            zoom_step_mul_100: 0,
            foreground: None,
            branch: None,
            grid: None,
            spawn_grid: None,
            tab_position: None,
            clock: None,
            running_elapsed: None,
            frozen: None,
            exit_status: None,
            scrolled_back: None,
            pasted: None,
            progress: None,
            program_status: None,
            ssh_destination: None,
            missed_activity: false,
            attention_red: None,
            attention_bright_red: None,
        };
        assert_eq!(
            status_ctx(&bare),
            serde_json::json!({
                "shell": "",
                "zoom_adj": 0,
                "zoom_step_mul_100": 0,
                "missed_activity": false,
            })
        );
    }
}
