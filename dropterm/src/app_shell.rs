// SPDX-License-Identifier: GPL-3.0-only

//! The app shell (gpui-scaffold Task 1, grown by gpui-surface): the
//! surface lifecycle's app-side glue — the decision layer
//! ([`dropterm::core::surface_lifecycle::SurfaceMachine`]) runs
//! unchanged from core, and this shell drives it with the dbus
//! verbs, feeds it the measuring window's granted bounds, and applies
//! the [`GpuiExecutor`](crate::executor::GpuiExecutor)'s actions
//! against live windows.
//!
//! The coexistence seam stays the scaffold's: the dbus drainer
//! forwards verbs into a queue this shell drains on its 100 ms tick
//! (the drainer cannot touch this context), and the shell publishes
//! its machine phase for the drainer's served-state mirror.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt, App, Bounds, Context, Entity, Pixels, Window, div, prelude::*, px,
};

use dropterm::core::config::DropTermConfig;
use dropterm::core::config::Edge;
use dropterm::core::config::TabTitleMode;
use dropterm::core::dbus_state::SurfacePhase;
use dropterm::core::intent::SurfaceExecutor;
use dropterm::core::slide::{self, SLIDE_DURATION_MS};
use dropterm::core::spawn;
use dropterm::core::status::{self, StatusInputs, StatusSegments, Strip};
use dropterm::core::surface_lifecycle::{SurfaceInputs, SurfaceMachine, SurfaceState};
use dropterm::core::tabs::TabId;
use gpui::Focusable as _;
use gpui_component::ActiveTheme as _;
use gpui_component::Root as KitRoot;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::menu::ContextMenuExt as _;
use notify::Watcher as _;

use crate::cli;
use crate::executor::{Action, GpuiExecutor};
use crate::status_bar;
use crate::tab_bar;
use crate::terminal::view::{self, TerminalPane};

// The close-confirm modal's Escape (bound in the `close-confirm`
// key context; every other key the modal holds is swallowed — no
// binds fire, nothing types).
gpui::actions!(dropterm_gpui, [DismissCloseConfirm, DismissSettings]);

/// The modal callback shape: the dismissal paths hand the shell both
/// halves the strip's callbacks get.
type ModalFn = std::rc::Rc<dyn Fn(&mut Window, &mut App)>;

/// The evidence walks' deterministic exit (a scaffold affordance, not
/// a feature): `DROPTERM_EXIT_AFTER_MS`, read once at boot. Unset —
/// and every non-positive or unparsable value — means a real app's
/// lifetime (run until the session ends); a positive value quits
/// through the event loop, so "maps and exits clean" is a logged,
/// replayable row on the rig and the VM.
pub(crate) fn exit_after_from_env(raw: Option<&str>) -> Option<Duration> {
    let ms: u64 = raw?.trim().parse().ok()?;
    (ms > 0).then(|| Duration::from_millis(ms))
}

/// Whether the drag handlers may act: the decision layer's
/// [`dropterm::core::surface_lifecycle::grab_bar_acts_on`] guard, read
/// through (shown and at rest — no drag arms while a slide animates).
/// The copied segment's `✓` window — the reference band's own
/// 800 ms, owned here like the libcosmic app owns it.
const STATUS_COPY_FLASH: Duration = Duration::from_millis(800);

/// The paste-refusal verdict's window (the audit's F7) — readable,
/// unlike the copy flash: a verdict the user must read to learn
/// their paste failed.
const STATUS_REFUSAL_FLASH: Duration = Duration::from_millis(3000);

/// The drag edge's grab strip height: the reference grab bar's own
/// `BAR_HEIGHT` — high enough to grab, thin enough to read as a
/// handle's footprint rather than chrome.
const DRAG_EDGE_PX: f32 = 8.0;

fn grab_bar_at_rest(machine: &SurfaceMachine) -> bool {
    dropterm::core::surface_lifecycle::grab_bar_acts_on(&machine.state, machine.slide.is_some())
}

/// The machine state as the served contract's phase vocabulary.
fn phase_of(state: &SurfaceState) -> SurfacePhase {
    match state {
        SurfaceState::Hidden => SurfacePhase::Hidden,
        SurfaceState::Measuring(_) => SurfacePhase::Measuring,
        SurfaceState::Shown(_) => SurfacePhase::Shown,
        SurfaceState::Hiding(_) => SurfacePhase::Hiding,
    }
}

/// An open inline tab rename: the tab being renamed and the Kit
/// input state editing its draft (the input hosts in the row's label
/// slot). The subscription that routes its events lives as long as
/// the edit.
struct RenameEdit {
    tab: TabId,
    state: Entity<InputState>,
    _events: gpui::Subscription,
}

/// One resolved scheme apply (gpui-theme Task 2): everything the
/// generator and the pane read from a config — the glue chain's name,
/// mode, and colors in one value, resolved at the echo so the tick's
/// apply is pure application.
struct ResolvedTheme {
    name: String,
    dark: bool,
    colors: alacritty_terminal::term::color::Colors,
}

/// Resolve a config's theme (gpui-theme Task 2): the same glue chain
/// the pane's [`view::boot_colors`] runs — `System` resolves dark
/// until the live system-mode read lands (the drawer default) — plus
/// the scheme name and mode [`crate::theme_gen`] wants.
fn resolve_theme(config: &DropTermConfig) -> ResolvedTheme {
    let dark = dropterm::core::theme_glue::effective_is_dark(config.theme_preference, true);
    ResolvedTheme {
        name: dropterm::core::profile::effective_scheme_name(
            None,
            &config.color_scheme_dark,
            &config.color_scheme_light,
            dark,
        )
        .to_string(),
        dark,
        colors: dropterm::core::theme_glue::effective_colors(config, None, dark),
    }
}

/// The applied-theme identity a config resolves to (gpui-theme Task
/// 2's change detector): scheme name plus mode. A config echo whose
/// identity matches the applied one re-themes nothing; geometry-only
/// edits never touch the theme.
fn theme_identity(theme: &ResolvedTheme) -> (String, bool) {
    (theme.name.clone(), theme.dark)
}

/// Apply a resolved theme through C4's sanctioned route (gpui-theme
/// Task 2): generated JSON → `from_value::<ThemeConfig>` →
/// `Theme::update(apply_config)` — the `#[non_exhaustive]` struct's
/// one sanctioned construction. Requires `gpui_component::init` (the
/// theme global the Kit's surfaces read); logs the token read-back
/// the walks evidence.
fn apply_resolved_theme(cx: &mut App, theme: &ResolvedTheme) {
    let json = crate::theme_gen::kit_theme_json(&theme.colors, &theme.name, theme.dark);
    let config: gpui_component::ThemeConfig = serde_json::from_value(json)
        .expect("the generated theme parses — pinned by theme_gen's own tests");
    gpui_component::Theme::update(cx, |t| t.apply_config(&std::rc::Rc::new(config)));
    let applied = cx.theme();
    println!(
        "dropterm: theme applied ({}, {}) — tokens: background {:?} primary {:?} \
         foreground {:?}",
        theme.name,
        if theme.dark { "dark" } else { "light" },
        applied.background,
        applied.primary,
        applied.foreground,
    );
}

/// The boot-time apply (gpui-theme Task 2): resolve the config's
/// theme and apply it before any window opens — also the kill
/// check's entry onto the same route, so its chrome evidences the
/// real path.
pub(crate) fn apply_kit_theme(cx: &mut App, config: &DropTermConfig) {
    apply_resolved_theme(cx, &resolve_theme(config));
}

/// The shell view: the shaped window's root. Owns the decision layer,
/// the executor, and the live terminal pane (gpui-terminal Task 4:
/// the pane replaced the scaffold's stand-in band — the drawer's
/// whole content is a terminal), and logs the mapped-bounds evidence
/// rows (first frame, every change) the walks read.
pub(crate) struct Shell {
    pub(crate) boot: Instant,
    /// When the current surface's shaped window opened (the
    /// OPEN->FIRST_FRAME budget row's clock).
    show_open: Instant,
    frames: u64,
    last_bounds: Option<Bounds<Pixels>>,
    machine: SurfaceMachine,
    executor: GpuiExecutor,
    inputs: SurfaceInputs,
    /// The drag-walk driver's remaining pointer positions (see
    /// [`drag_walk_positions`]); fed one per tick once the surface is
    /// shown and at rest, with the settle release a beat after the
    /// last.
    drag_walk: VecDeque<f32>,
    drag_release_at: Option<Instant>,
    /// External config edits, signaled by the watcher thread; drained
    /// and coalesced per tick into one reload-and-reshape.
    config_changes: Receiver<()>,
    /// A no-change reload's quiet window: one write lands as a burst
    /// of inotify events across several ticks, and re-logging an
    /// absorbed echo per tick is log spam, not evidence — signals
    /// inside the window are dropped without a reload.
    config_quiet_until: Option<Instant>,
    /// The last logged activation state (the keyboard re-grab's
    /// evidence rows ride on its transitions).
    last_active: Option<bool>,
    /// An activation loss observed in render, waiting for the tick's
    /// focus-loss decision (window work never runs inside render —
    /// sr-1's re-entrancy lesson).
    focus_lost: bool,
    /// The machine phase this shell publishes for the dbus mirror.
    phase: Arc<Mutex<SurfacePhase>>,
    /// The pasted-images panel (py-6): the open door, the armed
    /// delete row (the single-arm seat), and the decoded thumbnails
    /// by path — the drawer's own state shape.
    images_open: bool,
    images_armed: Option<std::path::PathBuf>,
    images_thumbs: HashMap<std::path::PathBuf, Arc<gpui::RenderImage>>,
    /// The fleet's size, published for the dbus mirror's `TabCount`
    /// (py-5): written at every fleet change, mirrored by the served
    /// loop's poller.
    tab_count: Arc<Mutex<u32>>,
    /// The forwarded actions, drained on the tick (py-5: the surface
    /// verbs and the tab verbs one queue).
    verbs: Receiver<cli::Action>,
    /// The first launch's startup action (py-5), applied by the
    /// boot's first pump and never again: the surface verbs and a
    /// `new-tab` keep startup application (the degraded tab verbs
    /// never reach here — `startup_action` already collapsed them).
    pending_startup: Option<cli::Action>,
    /// The drawer's tabs (gpui-tabs Task 1): plain records through
    /// core's model — the strip renders from this list.
    tabs: tab_bar::TabList,
    /// The panes by tab id: one live terminal per record; the active
    /// pane is the shaped window's content, mounted beside the strip
    /// (each pane renders into whatever surface the executor maps it
    /// into, resizes itself to the granted bounds, and holds the
    /// keyboard by construction — so a tab switch moves the keyboard
    /// with the mounted pane).
    panes: HashMap<TabId, Entity<TerminalPane>>,
    /// The live config snapshot (reloaded on every config echo): the
    /// strip's placement, border, and title mode read from it.
    pub(crate) config: DropTermConfig,
    /// The hide channel's sender half — every spawned pane shares it.
    signals_tx: std::sync::mpsc::Sender<view::PaneSignal>,
    /// The title mode the tab titles were last resolved under (the
    /// live mode-flip detector).
    applied_title_mode: TabTitleMode,
    /// The pane's Hide action, handed up through the channel and run
    /// through the machine's hide funnel on the tick — window work
    /// never runs inside a pane's event handler (sr-1's lesson).
    pane_signals: Receiver<view::PaneSignal>,
    /// The applied theme identity (gpui-theme Task 2): the config
    /// echo's change detector — an edit whose identity matches the
    /// applied one re-themes nothing.
    theme: (String, bool),
    /// A scheme edit awaiting the tick's apply point (the Kit's
    /// tokens and the pane's colors re-apply before the next frame).
    pending_theme: Option<ResolvedTheme>,
    /// The measuring probe's granted-bounds slot: the probe writes
    /// the configured output size (never from inside a render does it
    /// touch this shell — queued entity updates flush inside the
    /// caller's borrow), and the tick takes it into the
    /// first-configure decision.
    measured: Arc<Mutex<Option<(u32, u32)>>>,
    /// The single-slot pending close (the confirm gate): the tab the
    /// in-surface modal names while it waits for a human decision; a
    /// new request replaces whatever the slot held.
    pending_close: Option<TabId>,
    /// The open inline rename, if any (double-click or the menu's
    /// Rename row).
    rename: Option<RenameEdit>,
    /// The settings drawer's shell-owned state (gpui-chrome Task 2):
    /// open flag, page, drafts buffer, and the echo-refresh guard in
    /// one cohesive state; the Kit's stateful controls live on the
    /// drawer entity built at open and held here (the capture's focus
    /// hand-off needs the handle).
    pub(crate) settings: crate::drawer::SettingsState,
    pub(crate) settings_drawer: Option<gpui::Entity<crate::drawer::SettingsDrawer>>,
    /// The drawer's capture-focus handle, owned here so an armed
    /// capture's focus hand-off never reads the drawer entity from
    /// inside a shell update (the gc2 walk's reentrancy class).
    pub(crate) settings_capture_focus: gpui::FocusHandle,
    /// A departed rename input's focus handle: the render's repair
    /// hands the keyboard back to the active pane once the input's
    /// handle stops holding it (the modal's own handle repairs the
    /// same way through `modal_focus`).
    rename_focus_repair: Option<gpui::FocusHandle>,
    /// The confirm modal's keyboard handle — holds the keyboard while
    /// the modal waits (the pane's render-time claim defers to it),
    /// handed back to the active pane when the decision resolves.
    modal_focus: gpui::FocusHandle,
    exit_at: Option<Instant>,
    /// The status band's resolved segments (gpui-status Task 1):
    /// assembled on the tick from the active pane's engine and the
    /// config — the render path performs zero filesystem reads by
    /// construction (the reference band's own pin).
    status: dropterm::core::status::StatusSegments,
    /// The stored wall-clock string — the tick recomputes it under
    /// the clock gate (the band never formats time at render; the
    /// text flips exactly at the minute boundary).
    status_clock: Option<String>,
    /// The band's font: the terminal's own resolved family (the
    /// reference band's rule — the band is the terminal speaking
    /// about itself).
    band_font: gpui::Font,
    /// The copied segment's `✓` flash (gn-2): the seat and its
    /// deadline — the tick clears it when the 800 ms window passes
    /// (the reference band's own `COPY_FLASH` timing, owned here
    /// like the libcosmic app owns it).
    status_copied: Option<(dropterm::core::status::SegmentId, Instant)>,
    /// The paste road's standing refusal verdict (the audit's F7):
    /// the cause and its deadline — the tick clears the chip when
    /// the 3 s window passes, a successful paste clears it early
    /// with its row.
    status_paste_refusal: Option<(dropterm::core::status::PasteRefusal, Instant)>,
    /// Our own hostname, read once at construction — the OSC 7
    /// report's host-qualification base (a reported host renders
    /// only when it differs from this).
    hostname: Option<String>,
    /// The last tick's hidden reading — the show transition's
    /// detector (every show clears the missed-activity latches).
    was_hidden: bool,
    /// The command timer's last logged state (the boundary rows'
    /// change detector).
    status_timer_logged: (bool, Option<(String, std::time::Duration)>, Option<u32>),
    /// The completions table's shared load (gpui-parity Task 1): the
    /// loaded table beside the channel its background loads land on —
    /// `None` is the loading placeholder the panes carry.
    specs: SpecsWatch,
    /// The debounced session save's machine (gpui-parity Task 3,
    /// core's own clock-free state).
    session_save: dropterm::core::session::SessionSave,
    /// The armed debounce window — its generation and the instant its
    /// write is due (the tick is the wake; the `drag_release_at`
    /// pattern).
    session_due: Option<(u32, Instant)>,
    /// The last written session shape (the change detector's
    /// baseline, seeded at boot so a restored fleet writes nothing
    /// until it truly changes).
    session_written: Option<SessionShape>,
    /// The dock-presence body's handle (shell-presence spec, item 9
    /// ruled replicate 2026-11-15): `Some` while the 1×1 toplevel
    /// stands, `None` in the hidden mode (the drawer's layer
    /// surfaces are invisible to the toplevel listings by protocol —
    /// no body needed there).
    dock_anchor: Option<gpui::WindowHandle<DockAnchor>>,
}

/// The fleet's structural truth the session save detects changes
/// against: ids in strip order with each tab's pin, rename override,
/// cwd, and profile id, plus the active index. Shell-title churn
/// stays out (the libcosmic cwd gate's Guake cadence — a vim
/// statusline is not a save trigger), and so does everything the
/// write recomputes fresh.
type SessionShape = (
    Vec<(
        TabId,
        bool,
        Option<String>,
        Option<std::path::PathBuf>,
        Option<String>,
    )>,
    usize,
);

/// One tab's session-write inputs, collected before the snapshots
/// borrow them (the libcosmic `write_session` shape): the resolved
/// title, the rename pin, the pin, the fresh cwd readlink, the
/// spawn truth's program word, and the spawn facts (the profile id,
/// the `--cmd` ephemeral flag).
type SessionRow<'a> = (
    &'a str,
    Option<&'a str>,
    bool,
    Option<std::path::PathBuf>,
    String,
    Option<&'a str>,
    bool,
);

/// The shape of the fleet as it stands.
fn session_shape(tabs: &tab_bar::TabList) -> SessionShape {
    (
        tabs.records()
            .iter()
            .map(|tab| {
                (
                    tab.id,
                    tab.pinned,
                    tab.title_override.clone(),
                    tab.cwd.clone(),
                    tab.profile.clone(),
                )
            })
            .collect(),
        tabs.active_index(),
    )
}

/// What the tick's dock-anchor sync owes (shell-presence spec, item 9
/// ruled replicate 2026-11-15): the config's `show_in_dock` against
/// the live body decides — open the presence, close it, or keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnchorAction {
    /// `show_in_dock` turned on with no body live: open the toplevel.
    Open,
    /// It turned off with a body live: close it.
    Close,
    /// The config and the body already agree.
    Keep,
}

/// The swap rule itself — pure, pinned headless.
fn anchor_action(show_in_dock: bool, live: bool) -> AnchorAction {
    match (show_in_dock, live) {
        (true, false) => AnchorAction::Open,
        (false, true) => AnchorAction::Close,
        _ => AnchorAction::Keep,
    }
}

/// The dock-presence body's root view (shell-presence spec, the
/// replicate ruling): an empty div — the presence is the point, a
/// mapped 1×1 toplevel the listing protocols (and only they) can
/// see.
struct DockAnchor;

impl gpui::Render for DockAnchor {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div()
    }
}

/// The dock body's window shape (the libcosmic
/// `clipboard_anchor_settings` port): 1×1, undecorated, immovable,
/// never stealing focus — mapped, because mapping is what the dock
/// and switcher list.
fn anchor_window_options() -> gpui::WindowOptions {
    gpui::WindowOptions {
        titlebar: None,
        window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds {
            origin: gpui::point(px(0.), px(0.)),
            size: gpui::Size::new(px(1.), px(1.)),
        })),
        app_id: Some(dropterm::core::APP_ID.to_string()),
        focus: false,
        show: true,
        kind: gpui::WindowKind::Normal,
        is_movable: false,
        is_resizable: false,
        ..Default::default()
    }
}

/// The specs provider's table (the app-owned completions load):
/// built-in floor overlaid by `completions.toml` and the extended
/// tier, as one shared slice every pane renders from.
type SpecsTable = Arc<[dropterm::core::terminal::suggest::SpecCommand]>;

/// The completions table's shared load (gpui-parity Task 1; the
/// libcosmic `completions_reload_task` port): the CPU-bound read +
/// parse runs on a plain thread (the config watcher's own shape) and
/// lands on the channel the shell's tick drains — the tick stores it
/// and pushes it to every pane through the live options pass, never
/// blocking startup or a config echo on the parse. Armed at boot and
/// re-armed on every absorbed config echo (an edited
/// `completions.toml` and the extended toggle's flip both apply
/// live).
struct SpecsWatch {
    table: Option<SpecsTable>,
    tx: std::sync::mpsc::Sender<SpecsTable>,
    rx: std::sync::mpsc::Receiver<SpecsTable>,
}

impl SpecsWatch {
    /// The boot watch: the channel plus the initial load thread.
    fn spawn_initial(config: &DropTermConfig) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<SpecsTable>();
        Self::arm_load(&tx, config.autosuggestions_completions_extended);
        Self {
            table: None,
            tx,
            rx,
        }
    }

    /// One background load into the channel (the watcher-thread
    /// precedent: a plain thread, parked away when done).
    fn arm_load(tx: &std::sync::mpsc::Sender<SpecsTable>, extended: bool) {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let dir = dropterm::core::config::store::store_dir();
            let table = dropterm::core::terminal::suggest::load_completions(Some(&dir), extended);
            let _ = tx.send(table);
        });
    }

    /// Arms a reload after a config echo (the store dir holds
    /// `completions.toml`, and the watcher fires on the whole dir).
    fn arm_reload(&self, config: &DropTermConfig) {
        Self::arm_load(&self.tx, config.autosuggestions_completions_extended);
    }

    /// Takes a landed table, if any load finished since the last
    /// drain.
    fn drain(&mut self) -> Option<SpecsTable> {
        let mut landed = None;
        while let Ok(table) = self.rx.try_recv() {
            landed = Some(table);
        }
        if landed.is_some() {
            self.table.clone_from(&landed);
        }
        landed
    }
}

impl Shell {
    #[allow(clippy::too_many_arguments)]
    fn new(
        boot: Instant,
        inputs: SurfaceInputs,
        phase: Arc<Mutex<SurfacePhase>>,
        tab_count: Arc<Mutex<u32>>,
        verbs: Receiver<cli::Action>,
        startup: cli::Action,
        tabs: tab_bar::TabList,
        panes: HashMap<TabId, Entity<TerminalPane>>,
        config: DropTermConfig,
        signals_tx: std::sync::mpsc::Sender<view::PaneSignal>,
        pane_signals: Receiver<view::PaneSignal>,
        theme: (String, bool),
        drag_walk: Option<Vec<f32>>,
        config_changes: Receiver<()>,
        modal_focus: gpui::FocusHandle,
        exit_after: Option<Duration>,
        band_font: gpui::Font,
        settings_focus: gpui::FocusHandle,
        specs: SpecsWatch,
    ) -> Self {
        let applied_title_mode = config.tab_title_mode;
        // The booted fleet is the session baseline (computed before
        // the struct literal moves the list).
        let session_written = session_shape(&tabs);
        Self {
            boot,
            show_open: boot,
            frames: 0,
            last_bounds: None,
            machine: SurfaceMachine::default(),
            executor: GpuiExecutor::new(),
            inputs,
            tabs,
            panes,
            config,
            signals_tx,
            applied_title_mode,
            pane_signals,
            theme,
            pending_theme: None,
            drag_walk: drag_walk.unwrap_or_default().into(),
            drag_release_at: None,
            config_changes,
            config_quiet_until: None,
            last_active: None,
            focus_lost: false,
            phase,
            images_open: false,
            images_armed: None,
            images_thumbs: HashMap::new(),
            tab_count,
            verbs,
            pending_startup: Some(startup),
            measured: Arc::new(Mutex::new(None)),
            pending_close: None,
            rename: None,
            settings: crate::drawer::SettingsState::default(),
            settings_drawer: None,
            settings_capture_focus: settings_focus,
            rename_focus_repair: None,
            modal_focus,
            status: dropterm::core::status::StatusSegments::default(),
            status_clock: None,
            band_font,
            status_copied: None,
            status_paste_refusal: None,
            was_hidden: true,
            status_timer_logged: (false, None, None),
            hostname: std::fs::read_to_string("/proc/sys/kernel/hostname")
                .ok()
                .map(|host| host.trim().to_string()),
            exit_at: exit_after.map(|after| boot + after),
            specs,
            session_save: dropterm::core::session::SessionSave::default(),
            session_due: None,
            dock_anchor: None,
            // The booted fleet is the baseline: a restored session
            // writes nothing until it truly changes.
            session_written: Some(session_written),
        }
    }

    /// The probe's granted-bounds slot, cloned into each measuring
    /// window this shell opens.
    fn measured_slot(&self) -> Arc<Mutex<Option<(u32, u32)>>> {
        Arc::clone(&self.measured)
    }

    /// Publishes the machine's phase for the served-state mirror (the
    /// drainer reads it on its own tick and emits `PropertiesChanged`
    /// on the change).
    fn publish_phase(&self) {
        let phase = phase_of(&self.machine.state);
        if let Ok(mut published) = self.phase.lock() {
            *published = phase;
        }
    }

    /// One pump: drains the forwarded verbs and the measuring slot
    /// through the decision layer, executing every intent into
    /// actions — pure decision work, no window touched. The tick
    /// applies the returned actions at App level: opening a window
    /// draws its root synchronously, and drawing the Shell while this
    /// entity is mid-update panics ("already being updated" — the
    /// first walk's lesson, made structural).
    fn pump(&mut self, cx: &mut Context<Self>) -> Pumped {
        let mut actions = Vec::new();
        let mut exited = Vec::new();
        let mut tab_verbs = Vec::new();
        // The startup action applies once, on the first pump — the
        // fleet stands by then (restore included), so a first-launch
        // `new-tab` opens beside restored tabs, and the bare launch's
        // default Toggle shows the overlay, exactly the libcosmic
        // init's `debounced_toggle(startup)` (the checklist's
        // first-launch row).
        if let Some(startup) = self.pending_startup.take() {
            println!(
                "dropterm: startup action {startup} at +{}ms",
                self.boot.elapsed().as_millis()
            );
            self.dispatch(cx, startup, &mut actions, &mut tab_verbs);
        }
        while let Ok(action) = self.verbs.try_recv() {
            self.dispatch(cx, action, &mut actions, &mut tab_verbs);
        }
        // The panes' handed-up signals: one drain (repeat Hide sends
        // coalesce — a second hide on a hidden surface decides
        // nothing), the Hide actions run through the machine's hide
        // funnel exactly like the verb's.
        while let Ok(signal) = self.pane_signals.try_recv() {
            match signal {
                view::PaneSignal::Hide => {
                    println!(
                        "dropterm: pane hide action -> the hide funnel at +{}ms",
                        self.boot.elapsed().as_millis()
                    );
                    self.dump_visible_grid("pane-hide", cx);
                    let intents = self.machine.decide_hide(self.inputs);
                    self.execute_into(intents, &mut actions);
                }
                view::PaneSignal::Exited(id) => {
                    // A shell exit closes its tab ungated (never the
                    // confirm gate); the close touches panes and may
                    // respawn, so it applies at the tick's App-level
                    // point like every window-adjacent action.
                    println!(
                        "dropterm: pane {id:?} exited -> closing its tab at +{}ms",
                        self.boot.elapsed().as_millis()
                    );
                    exited.push(id);
                }
                view::PaneSignal::ImagePasted(id, path, bytes) => {
                    // The paste's ledger row (paste-ledger spec): the
                    // pane already pasted the cache file's path at its
                    // prompt; the shell records the row on the pane's
                    // tab — the pasted-images chip's input. The panel
                    // that lists the rows rides the chrome slices.
                    println!(
                        "dropterm: pane {id:?} pasted {} ({} bytes) -> the tab's ledger",
                        path.display(),
                        bytes.len()
                    );
                    self.tabs.note_pasted_image(id, path, &bytes);
                    // A paste succeeded: any standing refusal verdict
                    // is stale news — its chip yields early.
                    self.status_paste_refusal = None;
                }
                view::PaneSignal::PasteRefused(refusal) => {
                    // The paste verdict (the audit's F7): the band's
                    // flash channel narrates the refusal for its 3 s
                    // window; the log warn already fired at the read.
                    println!("dropterm: paste refused ({refusal:?}) -> the band's verdict chip");
                    self.status_paste_refusal =
                        Some((refusal, Instant::now() + STATUS_REFUSAL_FLASH));
                }
            }
        }
        let granted = self.measured.lock().map_or(None, |mut slot| slot.take());
        if let Some(output) = granted {
            self.measured(output, &mut actions);
        }
        // An activation loss while shown: the focus-loss decision
        // (the config gate plus the machine's focus latch — only a
        // `LayerFocused`-class GAINED ever armed it), then the hide
        // funnel when it fires.
        if self.focus_lost {
            self.focus_lost = false;
            if let SurfaceState::Shown(handle) = self.machine.state
                && dropterm::core::surface_lifecycle::layer_unfocused_acts_on(
                    &self.machine.state,
                    handle,
                )
            {
                let (report_unfocus, hide) = self.machine.decide_focus_loss(self.inputs);
                println!(
                    "dropterm: activation lost while shown -> focus-loss decision \
                     (report_unfocus={report_unfocus}, hide={hide}) at +{}ms",
                    self.boot.elapsed().as_millis()
                );
                if hide {
                    let intents = self.machine.decide_hide(self.inputs);
                    self.execute_into(intents, &mut actions);
                }
            }
        }
        // The slide clock: parking finished slides is the machine's
        // own truth (an `In` slide rides every shown surface until it
        // parks, and an unparked slide would block reshapes and drags
        // by the decision layer's own guards). The animated
        // translation of an `Out` slide is task 3's; the clock is
        // this shell's to tick.
        let slide_intents = self.machine.decide_slide_tick();
        if !slide_intents.is_empty() {
            self.execute_into(slide_intents, &mut actions);
            // The completion tick's destroy is a state change the
            // served mirror must see: Hiding became Hidden with no
            // verb anywhere in the path.
            self.publish_phase();
        }
        self.drag_walk_step(&mut actions);
        self.drain_config_changes(&mut actions);
        let quit = match self.exit_at {
            Some(deadline) if Instant::now() >= deadline => {
                println!(
                    "dropterm: exit timer fired at +{}ms",
                    self.boot.elapsed().as_millis()
                );
                true
            }
            _ => false,
        };
        // The fleet's size, published for the dbus mirror's
        // `TabCount` (py-5): one write per pump, the mirror's poller
        // derives and emits on the real flip.
        if let Ok(mut count) = self.tab_count.lock() {
            *count = u32::try_from(self.tabs.records().len()).unwrap_or(u32::MAX);
        }
        Pumped {
            actions,
            exited,
            tab_verbs,
            quit,
        }
    }

    /// One tick of the drag-walk driver: the trailing apply first (a
    /// position the cadence gate skipped lands here), then one fed
    /// position — only while the grab-bar guard holds (shown, at
    /// rest) — and the settle release a beat after the last position.
    fn drag_walk_step(&mut self, actions: &mut Vec<Action>) {
        if self.machine.drag.is_some() {
            let trailing = self.machine.drag_trailing(Instant::now(), self.inputs);
            if let Some(applied) = trailing.applied {
                println!(
                    "dropterm: drag walk trailing apply landed target={applied}px at +{}ms",
                    self.boot.elapsed().as_millis()
                );
            }
            self.execute_into(trailing.intents, actions);
        }
        let at_rest = grab_bar_at_rest(&self.machine);
        if at_rest && let Some(pointer_y) = self.drag_walk.pop_front() {
            println!(
                "dropterm: drag walk move pointer_y={pointer_y:.0} at +{}ms",
                self.boot.elapsed().as_millis()
            );
            let outcome = self
                .machine
                .drag_move(pointer_y, Instant::now(), self.inputs);
            if let Some(target) = outcome.applied {
                println!(
                    "dropterm: drag walk cadence applied target={target}px at +{}ms",
                    self.boot.elapsed().as_millis()
                );
            }
            self.execute_into(outcome.intents, actions);
            if self.drag_walk.is_empty() && self.drag_release_at.is_none() {
                // A beat after the last move: the release settle.
                self.drag_release_at = Some(Instant::now() + Duration::from_millis(300));
            }
        }
        if let Some(when) = self.drag_release_at
            && Instant::now() >= when
        {
            self.drag_release_at = None;
            let (intents, settled, write) = self.machine.drag_release(self.inputs);
            println!(
                "dropterm: drag walk release settled at {}% at +{}ms",
                settled.height_percent,
                self.boot.elapsed().as_millis()
            );
            if let Some(percent) = write {
                // The persist itself is the config-write slice's;
                // the walk logs the decision the libcosmic app would
                // write.
                println!(
                    "dropterm: drag settle would persist height_percent={percent} \
                     (persistence arrives with the config-write slice)"
                );
            }
            self.inputs.geometry = settled;
            self.execute_into(intents, actions);
        }
    }

    /// The real drag edge's press (the grab-bar port): a left press
    /// on the free-edge strip arms the machine's drag at the
    /// pointer's window y — the walk's synthetic arm, fed a real
    /// event. The guard is the walk's own: shown, at rest.
    pub(crate) fn drag_edge_press(&mut self, pointer_y: f32, actions: &mut Vec<Action>) {
        if !grab_bar_at_rest(&self.machine) {
            return;
        }
        println!(
            "dropterm: drag edge press pointer_y={pointer_y:.0} at +{}ms",
            self.boot.elapsed().as_millis()
        );
        let outcome = self
            .machine
            .drag_move(pointer_y, Instant::now(), self.inputs);
        self.execute_into(outcome.intents, actions);
    }

    /// The drag edge's live move: only while the machine holds the
    /// drag (a hover with no press arming does nothing) — the
    /// cadence-gated applies and the trailing timer ride the same
    /// machinery the walk drove.
    pub(crate) fn drag_edge_move(&mut self, pointer_y: f32, actions: &mut Vec<Action>) {
        if self.machine.drag.is_none() {
            return;
        }
        let outcome = self
            .machine
            .drag_move(pointer_y, Instant::now(), self.inputs);
        if let Some(target) = outcome.applied {
            println!(
                "dropterm: drag edge applied target={target}px at +{}ms",
                self.boot.elapsed().as_millis()
            );
        }
        self.execute_into(outcome.intents, actions);
    }

    /// The drag edge's release — the settle: the dragged pixels
    /// quantize to a whole percent once, and a changed percent
    /// persists through the one write path (the walk logged this
    /// decision as "would persist" until the config-write slice
    /// landed; the real edge rides the slice).
    pub(crate) fn drag_edge_release(&mut self, actions: &mut Vec<Action>) {
        if self.machine.drag.is_none() {
            return;
        }
        let (intents, settled, write) = self.machine.drag_release(self.inputs);
        println!(
            "dropterm: drag edge release settled at {}% at +{}ms",
            settled.height_percent,
            self.boot.elapsed().as_millis()
        );
        if let Some(percent) = write {
            self.write_settings(&dropterm::core::settings::PlannedWrite::HeightPercent(
                percent,
            ));
        }
        self.inputs.geometry = settled;
        self.execute_into(intents, actions);
    }

    /// External config edits: coalesce every signaled change into one
    /// reload, recompute the decision inputs, and reshape the live
    /// surface if the geometry actually changed (the machine's own
    /// guards defer while a drag is armed or a slide animates).
    fn drain_config_changes(&mut self, actions: &mut Vec<Action>) {
        let mut signaled = false;
        while self.config_changes.try_recv().is_ok() {
            signaled = true;
        }
        if !signaled {
            return;
        }
        if let Some(quiet_until) = self.config_quiet_until
            && Instant::now() < quiet_until
        {
            // An echo of an already-absorbed write, inside the quiet
            // window: nothing to reload, nothing to log.
            return;
        }
        let config = DropTermConfig::load_from(&dropterm::core::config::store::store_dir());
        // gpui-tabs Task 1: the strip's placement, border, and title
        // mode read the live config at render/sync time, so the echo
        // lands the snapshot the next frame reads.
        self.config = config.clone();
        self.settings.config_epoch += 1;
        // gpui-parity Task 1: the config echo reloads the
        // completions table on a background thread — an edited
        // `completions.toml` or the extended toggle's flip applies
        // live at the pane pass below (the libcosmic echo's own
        // re-arm).
        self.specs.arm_reload(&config);
        // gpui-theme Task 2: a scheme edit re-themes — resolved here,
        // applied at the tick's apply point before the next frame.
        // The identity compare absorbs echoes (an unchanged scheme is
        // not a re-theme) and keeps geometry-only edits off the theme
        // path.
        let resolved = resolve_theme(&config);
        let identity = theme_identity(&resolved);
        if identity != self.theme {
            println!(
                "dropterm: config changed -> theme: {} ({})",
                resolved.name,
                if resolved.dark { "dark" } else { "light" }
            );
            self.theme = identity;
            self.pending_theme = Some(resolved);
        }
        let new_inputs = SurfaceInputs::from_config(&config);
        let old_geometry = self.inputs.geometry;
        let drag_armed = self.machine.drag.is_some();
        let intents = self
            .machine
            .decide_reshape(old_geometry, new_inputs.geometry, drag_armed);
        if intents.is_empty() && old_geometry == new_inputs.geometry {
            // The write (or its echo) changed nothing the surface's
            // geometry reads: absorb the rest of the burst quietly —
            // a pending theme (a scheme-only edit) still rides the
            // tick's apply.
            self.config_quiet_until = Some(Instant::now() + Duration::from_millis(300));
            return;
        }
        println!(
            "dropterm: config changed -> reshape decision: {} intent(s) \
             (geometry {}%/{}% {:?} {:?} -> {}%/{}% {:?} {:?})",
            intents.len(),
            old_geometry.height_percent,
            old_geometry.width_percent,
            old_geometry.h_align,
            old_geometry.edge,
            new_inputs.geometry.height_percent,
            new_inputs.geometry.width_percent,
            new_inputs.geometry.h_align,
            new_inputs.geometry.edge,
        );
        self.inputs = new_inputs;
        self.execute_into(intents, actions);
    }

    /// The tick's theme apply point (gpui-theme Task 2): the Kit's
    /// tokens and the pane's colors from one resolution, applied here
    /// where the App context lives — before the frame the tick's own
    /// notify asks for, so a scheme edit lands without a restart.
    fn apply_pending_theme(&mut self, cx: &mut Context<Self>) {
        let Some(theme) = self.pending_theme.take() else {
            return;
        };
        apply_resolved_theme(cx, &theme);
        // Every pane re-themes on the same resolution (gpui-tabs: the
        // panes are a fleet now).
        for pane in self.panes.values() {
            pane.update(cx, |pane, cx| pane.retheme(&theme.colors, cx));
        }
    }

    /// The tick's session save point (gpui-parity Task 3): the
    /// fleet's structural shape against the last written baseline
    /// detects every save trigger — opens, closes, reorders, renames,
    /// pins, switches, cwd moves (the record the title sync
    /// maintains) — and the debounce coalesces bursts into at most
    /// one write per interval (core's own [`SessionSave`] machine,
    /// the tick the wake the libcosmic sleep task is). The toggle
    /// gates the whole path: disabled, nothing marks and a pending
    /// window's write stops, the file on disk waiting untouched.
    fn pump_session_save(&mut self, cx: &mut Context<Self>) {
        let shape = session_shape(&self.tabs);
        let enabled = self.config.session_persistence_enabled;
        if let Some((generation, due_at)) = self.session_due
            && Instant::now() >= due_at
        {
            self.session_due = None;
            if self.session_save.due_writes(generation, enabled) {
                self.write_session(cx);
                self.session_written = Some(shape.clone());
            }
        }
        if self.session_written.as_ref() != Some(&shape)
            && let Some(generation) = self.session_save.mark_gated(enabled)
        {
            self.session_due = Some((
                generation,
                Instant::now() + dropterm::core::session::SESSION_WRITE_INTERVAL,
            ));
        }
    }

    /// Captures the live fleet and writes the session file atomically
    /// (the libcosmic `write_session` port): the cwd readlinks happen
    /// here — at write time, not schedule time — so saved cwds are as
    /// fresh as the debounce allows, `capture` staleness-filtering
    /// each one. A failed write logs and leaves the old file in
    /// place: the next dirty window retries.
    fn write_session(&self, cx: &Context<Self>) {
        // Collected first (the libcosmic shape): the snapshots borrow
        // these locals, and capture's staleness filter reads them
        // whole.
        let collected: Vec<SessionRow<'_>> = self
            .tabs
            .records()
            .iter()
            .filter_map(|tab| {
                let pane = self.panes.get(&tab.id)?;
                let pane = pane.read(cx);
                Some((
                    tab.title.as_str(),
                    tab.title_override.as_deref(),
                    tab.pinned,
                    pane.working_directory(),
                    pane.spawn_program().to_string(),
                    tab.profile.as_deref(),
                    tab.ephemeral,
                ))
            })
            .collect();
        let snapshots: Vec<dropterm::core::session::TabSnapshot<'_>> = collected
            .iter()
            .map(
                |(title, title_override, pinned, cwd, shell, profile, ephemeral)| {
                    dropterm::core::session::TabSnapshot {
                        title,
                        title_override: *title_override,
                        shell,
                        cwd: cwd.as_deref(),
                        // The spawn facts (py-5): the profile id the tab
                        // launched under — never on a plain or degraded
                        // arm — and the `--cmd` ephemeral flag, the one
                        // fact the session saves and the other it never
                        // does.
                        profile: *profile,
                        ephemeral: *ephemeral,
                        pinned: *pinned,
                    }
                },
            )
            .collect();
        let state = dropterm::core::session::capture(&snapshots, self.tabs.active_index());
        println!(
            "dropterm: session write ({} tab(s), active {}) -> {}",
            state.tabs.len(),
            self.tabs.active_index(),
            dropterm::core::session::state_file_path().display()
        );
        if let Err(err) = dropterm::core::session::save_atomic(
            &dropterm::core::session::state_file_path(),
            &state,
        ) {
            println!("dropterm: failed to write session file: {err}");
        }
    }

    /// The dock-anchor sync (shell-presence spec, item 9 ruled
    /// replicate 2026-11-15; the tick's apply point so a config echo
    /// flips the entry at the echo itself): `show_in_dock` on opens
    /// the 1×1 toplevel the dock and switcher list; off closes it.
    /// GPUI's clipboard needs no anchor either way (check 8: the
    /// gate is focus+serial, the terminal satisfies it, and
    /// unfocused copies ride data-control) — presence is this
    /// body's whole duty here.
    fn sync_dock_anchor(&mut self, cx: &mut Context<Self>) {
        match anchor_action(self.config.show_in_dock, self.dock_anchor.is_some()) {
            AnchorAction::Open => {
                match cx.open_window(anchor_window_options(), |_, cx| cx.new(|_| DockAnchor)) {
                    Ok(handle) => {
                        println!("dropterm: dock presence opened (the 1x1 toplevel stands)");
                        self.dock_anchor = Some(handle);
                    }
                    Err(err) => {
                        println!("dropterm: dock presence open failed: {err:?}");
                    }
                }
            }
            AnchorAction::Close => {
                if let Some(handle) = self.dock_anchor.take() {
                    println!("dropterm: dock presence closed (hidden mode)");
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                }
            }
            AnchorAction::Keep => {}
        }
    }

    /// The tick's completions apply point (gpui-parity Task 1): a
    /// landed background load stores the shared table and pushes it
    /// to every pane through the live options pass — the loading row
    /// the menu carried clears, and the specs tier answers from the
    /// next typed prefix.
    fn drain_completions(&mut self, cx: &mut Context<Self>) {
        if let Some(table) = self.specs.drain() {
            println!(
                "dropterm: completions table loaded ({} command(s)) -> every pane",
                table.len()
            );
            for pane in self.panes.values() {
                let options = crate::terminal::suggest::pane_options(&self.config, Some(&table));
                pane.update(cx, |pane, cx| pane.apply_options(options, cx));
            }
        }
    }

    /// One forwarded surface verb through the toggle funnel: the
    /// debounce first, then the state-matched decision — show from
    /// hidden (the two-surface show), reversal from mid-slide, hide
    /// from measuring or shown (the slide-out wire, or destroy-now
    /// for a measuring surface).
    /// One forwarded action's dispatch (py-5): the toggle debounce
    /// guards every verb — surface and tab alike, a compositor
    /// double-delivery must neither toggle twice nor spawn two tabs
    /// (the libcosmic `debounced_toggle` rule) — then the surface
    /// verbs run the machine funnel and the tab verbs queue for the
    /// tick's App-level apply point.
    fn dispatch(
        &mut self,
        cx: &Context<Self>,
        action: cli::Action,
        actions: &mut Vec<Action>,
        tab_verbs: &mut Vec<cli::Action>,
    ) {
        if !self.machine.toggle_accepted(Instant::now()) {
            println!(
                "dropterm: action {action} dropped by the toggle debounce at +{}ms",
                self.boot.elapsed().as_millis()
            );
            return;
        }
        match action {
            cli::Action::Toggle => {
                println!(
                    "dropterm: action toggle accepted at +{}ms",
                    self.boot.elapsed().as_millis()
                );
                let label = match self.machine.state {
                    SurfaceState::Hidden => "toggle->show",
                    SurfaceState::Hiding(_) => "toggle->reverse-show",
                    SurfaceState::Measuring(_) | SurfaceState::Shown(_) => "toggle->hide",
                };
                self.dump_visible_grid(label, cx);
                match self.machine.state {
                    SurfaceState::Hidden => {
                        let intents = self.machine.decide_show(self.inputs);
                        self.execute_into(intents, actions);
                    }
                    SurfaceState::Hiding(handle) => {
                        let intents = self.machine.decide_reverse_show(handle, self.inputs);
                        self.execute_into(intents, actions);
                    }
                    SurfaceState::Measuring(_) | SurfaceState::Shown(_) => {
                        let intents = self.machine.decide_hide(self.inputs);
                        self.execute_into(intents, actions);
                    }
                }
                self.publish_phase();
            }
            cli::Action::Show => {
                println!(
                    "dropterm: action show accepted at +{}ms",
                    self.boot.elapsed().as_millis()
                );
                self.dump_visible_grid("show", cx);
                self.show_intent(actions);
            }
            cli::Action::Hide => {
                println!(
                    "dropterm: action hide accepted at +{}ms",
                    self.boot.elapsed().as_millis()
                );
                self.dump_visible_grid("hide", cx);
                if matches!(
                    self.machine.state,
                    SurfaceState::Measuring(_) | SurfaceState::Shown(_)
                ) {
                    let intents = self.machine.decide_hide(self.inputs);
                    self.execute_into(intents, actions);
                }
                self.publish_phase();
            }
            tab => {
                println!(
                    "dropterm: action {tab} accepted at +{}ms",
                    self.boot.elapsed().as_millis()
                );
                tab_verbs.push(tab);
            }
        }
    }

    /// The toggle-dump trace (`DROPTERM_TOGGLE_DEBUG`): the active
    /// pane's visible grid at every accepted surface action — the
    /// prompt-duplication repro's missing half. The log the app
    /// already writes narrates what it drove (each pane resize with
    /// its grid); this narrates what the shell did with it —
    /// duplicated prompts present in the dump arrived through the
    /// PTY (grid-side), prompts seen on screen but absent from the
    /// dump are a paint-side artifact. Appended to
    /// /tmp/dropterm-toggle.log and echoed to stdout, one walk's
    /// evidence per file.
    fn dump_visible_grid(&self, label: &str, cx: &Context<Self>) {
        if std::env::var_os("DROPTERM_TOGGLE_DEBUG").is_none() {
            return;
        }
        let ms = self.boot.elapsed().as_millis();
        let block = match self.active_pane() {
            Some(pane) => {
                use std::fmt::Write as _;
                let rows = pane.read(cx).visible_rows();
                let mut block = format!(
                    "dropterm: toggle-dump {label} at +{ms}ms — {} rows",
                    rows.len()
                );
                for (n, row) in rows.iter().enumerate() {
                    let _ = write!(block, "\n  {n:>3}| {row}");
                }
                block
            }
            None => format!("dropterm: toggle-dump {label} at +{ms}ms — no pane yet"),
        };
        println!("{block}");
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/dropterm-toggle.log")
            .and_then(|mut f| {
                use std::io::Write as _;
                writeln!(f, "{block}")
            });
    }

    /// The show half every tab-facing verb rides (the libcosmic
    /// `show_intent_surface`): show-from-hidden, a mid-slide-out
    /// reversal on the live surface, and nothing while already on its
    /// way — the machine's own guards decide.
    fn show_intent(&mut self, actions: &mut Vec<Action>) {
        match self.machine.state {
            SurfaceState::Hidden => {
                let intents = self.machine.decide_show(self.inputs);
                self.execute_into(intents, actions);
            }
            // A mid-slide reversal: the live surface is reused, the
            // slide re-targets `In`, and the keyboard re-holds at the
            // resolved mode — task 3's wire, served.
            SurfaceState::Hiding(handle) => {
                let intents = self.machine.decide_reverse_show(handle, self.inputs);
                self.execute_into(intents, actions);
            }
            // Already shown or on its way: the decision layer's own
            // guards leave these with nothing.
            SurfaceState::Measuring(_) | SurfaceState::Shown(_) => {}
        }
        self.publish_phase();
    }

    /// The measuring window's granted bounds: the output's real size,
    /// read off the surface the compositor just configured — never
    /// `cx.displays()`, so fractional outputs mis-size nothing. Runs
    /// the first-configure decision and lands the shaped window.
    fn measured(&mut self, output: (u32, u32), actions: &mut Vec<Action>) {
        println!(
            "dropterm: measuring window granted {}x{} at +{}ms — pass 1 complete",
            output.0,
            output.1,
            self.boot.elapsed().as_millis()
        );
        let intents = self.machine.decide_configure(output, self.inputs);
        self.execute_into(intents, actions);
        self.publish_phase();
    }

    /// Executes intents through the GPUI executor, logging each intent
    /// row (the wire evidence the walks read) and collecting the
    /// actions for the tick to apply.
    fn execute_into(
        &mut self,
        intents: Vec<dropterm::core::intent::SurfaceIntent>,
        actions: &mut Vec<Action>,
    ) {
        for intent in intents {
            println!(
                "dropterm: surface intent {intent:?} at +{}ms",
                self.boot.elapsed().as_millis()
            );
            actions.extend(self.executor.execute(intent));
        }
    }
}

/// One pump's outcome: the window actions to apply at App level, and
/// whether the deterministic exit fired.
struct Pumped {
    actions: Vec<Action>,
    /// Tabs whose shells exited this pump — closed ungated at the
    /// tick's apply point.
    exited: Vec<TabId>,
    /// The forwarded tab verbs (py-5) — applied at the tick's apply
    /// point, where pane spawning's App context lives.
    tab_verbs: Vec<cli::Action>,
    quit: bool,
}

/// One mouse-handler step against the shell: mutate under the
/// shell's update (collecting window actions), then apply them at
/// App level — the same split the tick takes. A mouse handler runs
/// inside the window's event dispatch, so no shell update may be on
/// the stack when it builds elements (the gc-2 lesson, structural).
fn run_shell_actions(
    shell: &Entity<Shell>,
    cx: &mut App,
    step: impl FnOnce(&mut Shell, &mut Vec<Action>),
) {
    let actions = shell.update(cx, |shell, _| {
        let mut actions = Vec::new();
        step(shell, &mut actions);
        actions
    });
    for action in actions {
        apply_action(cx, shell, action);
    }
    shell.update(cx, |_, cx| cx.notify());
}

/// Applies one window action against live windows — App-level work by
/// necessity (see [`Shell::pump`]): opens draw their root view
/// synchronously, so no shell update may be on the stack.
fn apply_action(cx: &mut App, shell: &Entity<Shell>, action: Action) {
    match action {
        Action::OpenMeasuring { handle, options } => {
            let slot = shell.read(cx).measured_slot();
            let opened = Instant::now();
            match cx.open_window(options, |_, cx| {
                cx.new(|_| MeasureProbe {
                    slot,
                    opened,
                    fired: false,
                })
            }) {
                Ok(window) => {
                    shell.update(cx, |shell, _| shell.executor.adopt(handle, window.into()));
                    println!(
                        "dropterm: two-surface show pass 1: measuring window opened at \
                         +{}ms",
                        shell.read(cx).boot.elapsed().as_millis()
                    );
                }
                Err(err) => {
                    eprintln!("dropterm: measuring window open failed: {err:?}");
                    cx.quit();
                }
            }
        }
        Action::SwapWindow {
            handle,
            options,
            reason,
        } => {
            let old = shell.read(cx).executor.window_of(handle);
            let _ = old.update(cx, |_, window, _| window.remove_window());
            shell.update(cx, |shell, _| {
                shell.show_open = Instant::now();
                shell.frames = 0;
                shell.last_bounds = None;
            });
            match cx.open_window(options, |window, cx| {
                // The Kit's Root hosts its overlays (the per-tab
                // menu's popover path) — the manual wrap the kit-pass
                // C3 ruling names, the kill check's own shape.
                cx.new(|cx| KitRoot::new(shell.clone(), window, cx))
            }) {
                Ok(window) => {
                    shell.update(cx, |shell, _| shell.executor.adopt(handle, window.into()));
                    println!(
                        "dropterm: window swap ({reason}) opened at +{}ms",
                        shell.read(cx).boot.elapsed().as_millis()
                    );
                }
                Err(err) => {
                    eprintln!("dropterm: swapped window open failed: {err:?}");
                    cx.quit();
                }
            }
        }
        Action::Resize {
            handle,
            width,
            height,
        } => {
            let window = shell.read(cx).executor.window_of(handle);
            #[expect(
                clippy::cast_precision_loss,
                reason = "px of a <= 2^24 output share: exact in f32"
            )]
            let _ = window.update(cx, |_, window, _| {
                // A fill surface's width is compositor-controlled:
                // carry the live width so the resize never asks for a
                // degenerate zero width.
                let w = width.map_or_else(|| window.viewport_size().width, |w| gpui::px(w as f32));
                window.resize(gpui::Size::new(w, gpui::px(height as f32)));
            });
            println!(
                "dropterm: resized to {}x{height} at +{}ms",
                width.map_or_else(|| "fill".to_string(), |w| w.to_string()),
                shell.read(cx).boot.elapsed().as_millis()
            );
        }
        Action::Remove { handle } => {
            let window = shell.read(cx).executor.window_of(handle);
            let _ = window.update(cx, |_, window, _| window.remove_window());
            shell.update(cx, |shell, _| shell.executor.retire(handle));
            println!(
                "dropterm: surface destroyed at +{}ms",
                shell.read(cx).boot.elapsed().as_millis()
            );
        }
        Action::FocusTerminal { handle } => {
            // The pane holds the keyboard by construction (its focus
            // handle grabs on first render inside the surface), so the
            // intent applies as the pane already standing; the row is
            // the focus decision landing.
            println!(
                "dropterm: terminal focus for {handle:?} (the pane holds the \
                 keyboard by construction) at +{}ms",
                shell.read(cx).boot.elapsed().as_millis()
            );
        }
    }
}

impl Shell {
    /// The active tab's pane, if the list holds one.
    fn active_pane(&self) -> Option<Entity<TerminalPane>> {
        self.tabs
            .active_id()
            .and_then(|id| self.panes.get(&id).cloned())
    }

    /// A tab row's press: activate through the list (which clears the
    /// tab's attention marker through core's transition) — the
    /// newly active pane mounts on the next frame and takes the
    /// keyboard by construction.
    fn select_tab(&mut self, id: TabId, cx: &mut Context<Self>) {
        // A press away from an open rename commits it (the reference
        // bar's rule), then lands on the pressed row.
        self.commit_open_rename(cx);
        let previous = self.tabs.active_id();
        if self.tabs.select(id) {
            let title = self
                .tabs
                .records()
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| tab.title.clone());
            println!(
                "dropterm: tab select -> {id:?} ({}) at +{}ms",
                title.as_deref().unwrap_or("?"),
                self.boot.elapsed().as_millis()
            );
            cx.notify();
        }
        // Both missed-activity latches clear at the switch
        // (status-activity spec): the outgoing tab's because it
        // ceased to be active, the incoming's because activation is
        // a touch verb — the user is now looking at it.
        for tab_id in [Some(id), previous].into_iter().flatten() {
            if let Some(pane) = self.panes.get(&tab_id) {
                let engine = pane.read(cx).terminal.clone();
                engine.lock().expect("engine lock").clear_missed_activity();
            }
        }
    }

    /// The `+` affordance's press: spawn the pane, record it through
    /// core's insert, and activate — the drawer's never-empty rule
    /// keeps at least this tab alive whatever closes later.
    fn new_tab(&mut self, cx: &mut Context<Self>) {
        self.commit_open_rename(cx);
        if let Some(id) = self.open_new_tab(cx) {
            println!(
                "dropterm: tab + -> {id:?} ({} tabs) at +{}ms",
                self.tabs.records().len(),
                self.boot.elapsed().as_millis()
            );
        }
    }

    /// The shared tail of every plain new-tab press — the `+`, the
    /// menu's New Tab row, the keybind (py-5): the configured default
    /// owns the spawn (a set, existing `default_profile` runs its
    /// preset), and the cwd composes the libcosmic `open_new_tab`
    /// way — the preset's live cwd over the active tab's cwd for a
    /// profile, the mode-threaded plain composition otherwise.
    fn open_new_tab(&mut self, cx: &mut Context<Self>) -> Option<TabId> {
        let inherited = self.active_cwd(cx);
        let command = spawn::default_spawn_command(&self.config);
        let cwd = self.default_cwd_for(&command, inherited);
        self.spawn_command_tab(cx, &command, cwd)
    }

    /// The active tab's working directory (the pane's own read), the
    /// inheritance leg of every new-tab composition.
    fn active_cwd(&self, cx: &Context<Self>) -> Option<std::path::PathBuf> {
        self.tabs
            .active_id()
            .and_then(|id| self.panes.get(&id))
            .and_then(|pane| pane.read(cx).working_directory())
    }

    /// The cwd a plain default spawn opens in — the libcosmic
    /// `ensure_tabs`/`open_new_tab` chain (py-5): the preset's live
    /// cwd when the default resolved to a profile, else the
    /// mode-threaded plain composition over `inherited`.
    fn default_cwd_for(
        &self,
        command: &spawn::SpawnCommand,
        inherited: Option<std::path::PathBuf>,
    ) -> Option<std::path::PathBuf> {
        match command {
            spawn::SpawnCommand::Profile {
                name, cwd: preset, ..
            } => spawn::live_preset_cwd(name, preset.as_ref()).or(inherited),
            _ => spawn::plain_spawn_cwd(&self.config, None, inherited),
        }
    }

    /// The one pane-spawning body every command spawn rides (py-5) —
    /// the `+`/menu/keybind's default composition, the remote
    /// `new-tab`'s explicit one, the never-empty respawn: the record
    /// through core's insert (the config's placement), the pane
    /// booted from core's spawn resolution (the wrap, the env merge,
    /// the per-profile appearance), the spawn facts recorded on the
    /// tab — the profile id, the `--cmd` ephemeral flag — and a
    /// failed PTY closing the record back (the rollback).
    fn spawn_command_tab(
        &mut self,
        cx: &mut Context<Self>,
        command: &spawn::SpawnCommand,
        cwd: Option<std::path::PathBuf>,
    ) -> Option<TabId> {
        let id = self.tabs.spawn(self.config.new_tab_after_current);
        let request = view::PaneRequest::for_command(command, cwd, &self.config);
        let profile_id = request.profile_id.clone();
        let ephemeral = matches!(command, spawn::SpawnCommand::CommandLine(_));
        let Some((_terminal, pane)) = view::boot_pane(
            cx,
            &self.config,
            request,
            self.specs.table.as_ref(),
            id,
            Some(self.signals_tx.clone()),
        ) else {
            eprintln!("dropterm: new tab's terminal failed to spawn; closing its record");
            self.tabs.close(id);
            return None;
        };
        self.panes.insert(id, pane);
        self.tabs.note_spawn_facts(id, profile_id, ephemeral);
        cx.notify();
        Some(id)
    }

    /// The gated front door every user-initiated close rides — the
    /// strip's `×` and middle-click, the menu's Close rows and sweeps
    /// (the sweeps call it per target): a pinned tab refuses (warn
    /// and no-op, no dialog); with the confirm toggle on and the
    /// tab's foreground child running, the single-slot pending close
    /// arms and the in-surface modal waits for a human decision;
    /// otherwise the close runs now. The gate reads the config and
    /// the pane at request time, so a mid-session toggle flip applies
    /// to the very next close.
    fn request_close(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_open_rename(cx);
        let Some(tab) = self.tabs.records().iter().find(|tab| tab.id == id) else {
            return;
        };
        if tab.pinned {
            println!(
                "dropterm: refusing to close pinned tab {id:?}; unpin it first at +{}ms",
                self.boot.elapsed().as_millis()
            );
            return;
        }
        let running = self
            .panes
            .get(&id)
            .is_some_and(|pane| pane.read(cx).foreground_child_running());
        if tab_bar::confirm_needed(self.config.close_confirmation_enabled, running) {
            self.pending_close = Some(id);
            self.modal_focus.focus(window, cx);
            println!(
                "dropterm: close {id:?} gated -> the confirm modal armed at +{}ms",
                self.boot.elapsed().as_millis()
            );
            cx.notify();
        } else {
            self.close_now(id, cx);
        }
    }

    /// The shared close tail every close path funnels into — the
    /// gated front door's immediate arm, the modal's Close Tab, and a
    /// shell exit (never the gate): the record closes through core's
    /// decision, the pane tears down, a pending confirm naming the
    /// tab retires (the stale guard — no later Close may act on it),
    /// and the never-empty respawn answers a last-tab close.
    fn close_now(&mut self, id: TabId, cx: &mut Context<Self>) {
        if self.pending_close == Some(id) {
            self.pending_close = None;
        }
        let Some(decision) = self.tabs.close(id) else {
            return;
        };
        self.panes.remove(&id);
        println!(
            "dropterm: tab {id:?} closed -> {} left, active {:?} at +{}ms",
            self.tabs.records().len(),
            self.tabs.active_id(),
            self.boot.elapsed().as_millis()
        );
        if decision.respawn {
            println!(
                "dropterm: last tab closed -> the never-empty respawn at +{}ms",
                self.boot.elapsed().as_millis()
            );
            // The respawn is a no-user-context plain spawn: the
            // configured default owns it (a `default_profile` preset
            // included), exactly the libcosmic `ensure_tabs` chain.
            let command = spawn::default_spawn_command(&self.config);
            let cwd = self.default_cwd_for(&command, None);
            self.spawn_command_tab(cx, &command, cwd);
        }
        cx.notify();
    }

    /// Cancels the pending close (the modal's Escape, scrim press, or
    /// Cancel): the slot clears and the keyboard returns to the
    /// active pane — the tab, its process, and its selection state
    /// stay untouched (the render's focus repair hands the keyboard
    /// back).
    fn cancel_pending_close(&mut self) {
        if let Some(id) = self.pending_close.take() {
            println!(
                "dropterm: close {id:?} canceled (modal dismissed) at +{}ms",
                self.boot.elapsed().as_millis()
            );
        }
    }

    /// Starts an inline rename of the tab with `id` — the row's
    /// double-click and the menu's Rename row meet here: the Kit
    /// input seeds from the tab's current title, selects its value,
    /// and takes the keyboard (the pane's cooperative claim defers);
    /// Enter commits, Escape cancels, a blur cancels (the reference
    /// bar's own routing). Below two tabs there is no row to host
    /// the input: a swallowed no-op (the reference rule).
    fn start_rename(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if !tab_bar::shows_tab_buttons(self.tabs.records().len()) {
            return;
        }
        let Some(tab) = self.tabs.records().iter().find(|tab| tab.id == id) else {
            return;
        };
        let title = tab.title.clone();
        let state = cx.new(|cx| InputState::new(window, cx).default_value(title));
        state.update(cx, |state, cx| state.select_all(window, cx));
        state.read(cx).focus_handle(cx).focus(window, cx);
        let events = cx.subscribe(
            &state,
            |shell, _state, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } => shell.commit_open_rename(cx),
                InputEvent::Blur => shell.cancel_open_rename(cx),
                _ => {}
            },
        );
        self.rename = Some(RenameEdit {
            tab: id,
            state,
            _events: events,
        });
        println!(
            "dropterm: tab {id:?} rename started at +{}ms",
            self.boot.elapsed().as_millis()
        );
        cx.notify();
    }

    /// Commits the open rename, if any — the input's Enter and every
    /// click-away path (a press on another row, a corner affordance,
    /// a menu pick, a drop): the draft becomes the tab's user rename
    /// through core, shown at once.
    fn commit_open_rename(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.rename.take() else {
            return;
        };
        self.rename_focus_repair = Some(edit.state.read(cx).focus_handle(cx));
        let draft = edit.state.read(cx).value().to_string();
        if self
            .tabs
            .commit_rename(edit.tab, &draft, self.config.tab_title_mode)
        {
            println!(
                "dropterm: tab {:?} renamed -> {draft:?} at +{}ms",
                edit.tab,
                self.boot.elapsed().as_millis()
            );
        }
        cx.notify();
    }

    /// Cancels the open rename, if any — the input's Escape (through
    /// the strip's key context) and its blur: the draft is discarded,
    /// it must not commit over a title arriving from outside.
    fn cancel_open_rename(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = self.rename.take() {
            self.rename_focus_repair = Some(edit.state.read(cx).focus_handle(cx));
            println!(
                "dropterm: tab {:?} rename canceled at +{}ms",
                edit.tab,
                self.boot.elapsed().as_millis()
            );
            cx.notify();
        }
    }

    /// Ends the open rename when its input can no longer be shown
    /// (the reference bar's prune rule): the renamed tab is gone, or
    /// the row vanished with a single tab left. Runs on the tick's
    /// tab-state pass.
    fn prune_rename(&mut self, cx: &mut Context<Self>) {
        let hosted = self.rename.as_ref().is_some_and(|edit| {
            tab_bar::shows_tab_buttons(self.tabs.records().len())
                && self.tabs.records().iter().any(|tab| tab.id == edit.tab)
        });
        if !hosted {
            self.cancel_open_rename(cx);
        }
    }

    /// A completed tab drag's drop (reorder): the dragged tab moves
    /// to the dropped row's index through core's reorder (the click
    /// away from an open rename commits first — the reference bar's
    /// rule).
    fn tab_moved(&mut self, dragged: TabId, to: usize, cx: &mut Context<Self>) {
        self.commit_open_rename(cx);
        if self.tabs.move_to(dragged, to) {
            println!(
                "dropterm: tab {dragged:?} dragged to index {to} at +{}ms",
                self.boot.elapsed().as_millis()
            );
            cx.notify();
        }
    }

    /// One per-tab menu pick — every row's verb routed to its shell
    /// call, scoped to the clicked tab (the click away from an open
    /// rename commits first).
    fn tab_menu_pick(
        &mut self,
        id: TabId,
        pick: tab_bar::TabMenuPick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use dropterm::core::tabs::CloseSweep;
        use tab_bar::TabMenuPick;
        self.commit_open_rename(cx);
        match pick {
            TabMenuPick::NewTab => self.new_tab(cx),
            TabMenuPick::Rename => self.start_rename(id, window, cx),
            TabMenuPick::Duplicate => self.duplicate_tab(id, cx),
            TabMenuPick::Pin => {
                if self.tabs.toggle_pin(id) {
                    println!(
                        "dropterm: tab {id:?} pin toggled at +{}ms",
                        self.boot.elapsed().as_millis()
                    );
                    cx.notify();
                }
            }
            TabMenuPick::Move(direction) => {
                if self.tabs.move_by(id, direction) {
                    println!(
                        "dropterm: tab {id:?} moved {direction:?} at +{}ms",
                        self.boot.elapsed().as_millis()
                    );
                    cx.notify();
                }
            }
            TabMenuPick::Close => self.request_close(id, window, cx),
            TabMenuPick::CloseOthers | TabMenuPick::CloseRight => {
                // Every sweep target rides the gated front door, one
                // at a time (the running ones prompt — the single-slot
                // confirmation ends armed on the leftmost of them).
                let sweep = if pick == TabMenuPick::CloseOthers {
                    CloseSweep::Others
                } else {
                    CloseSweep::Right
                };
                for target in self.tabs.close_sweep(id, sweep) {
                    self.request_close(target, window, cx);
                }
            }
            // The section header's muted press only dismisses (the
            // GNOME rule) — nothing routes.
            TabMenuPick::SwitchProfileHeader => {}
            TabMenuPick::SwitchProfile(profile_index) => {
                self.switch_tab_profile(id, profile_index, cx);
            }
        }
    }

    /// A menu profile launch (py-5, the reference `NewProfileTab`):
    /// the submenu pick resolves through core's `profile_pick` — a
    /// stale index (a config edited while the menu was open) warns and
    /// no-ops — and the spawn composes the preset's live cwd over the
    /// active tab's, the `open_new_tab` chain's profile arm.
    fn open_profile_tab(&mut self, profile_index: usize, cx: &mut Context<Self>) {
        self.commit_open_rename(cx);
        let Some(command) = spawn::profile_pick(&self.config, profile_index) else {
            println!(
                "dropterm: profile launch: no profile at index {profile_index} \
                 ({} configured)",
                self.config.profiles.len()
            );
            return;
        };
        let inherited = self.active_cwd(cx);
        let cwd = match &command {
            spawn::SpawnCommand::Profile {
                name, cwd: preset, ..
            } => spawn::live_preset_cwd(name, preset.as_ref()).or(inherited),
            // profile_pick only ever builds the Profile arm.
            _ => inherited,
        };
        if let Some(id) = self.spawn_command_tab(cx, &command, cwd) {
            println!(
                "dropterm: profile tab -> {id:?} ({} tabs) at +{}ms",
                self.tabs.records().len(),
                self.boot.elapsed().as_millis()
            );
        }
    }

    /// One terminal context-menu pick (gpui-chrome, py-5 the profile
    /// folders) — every row's verb routed to the path its keybind
    /// uses: the clipboard and scrollback cluster through the active
    /// pane's own action funnel, the tab and settings verbs through
    /// the shell calls the tab menu's rows already use, and the two
    /// folder picks through the profile paths py-5 opened (a launch
    /// and the active tab's in-place respawn). The click away from an
    /// open rename commits first, exactly like a tab-menu pick.
    fn context_menu_pick(
        &mut self,
        pick: crate::context_menu::Pick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::context_menu::{PickRoute, ShellPick};
        println!(
            "dropterm: context menu pick {pick:?} at +{}ms",
            self.boot.elapsed().as_millis()
        );
        self.commit_open_rename(cx);
        match pick {
            crate::context_menu::Pick::Item(item) => match crate::context_menu::route(item) {
                PickRoute::Pane(action) => {
                    let Some(pane) = self.active_pane() else {
                        return;
                    };
                    pane.update(cx, |pane, cx| pane.perform_term_action(action, cx));
                }
                PickRoute::Shell(ShellPick::NewTab) => self.new_tab(cx),
                PickRoute::Shell(ShellPick::RenameTab) => {
                    if let Some(id) = self.tabs.active_id() {
                        self.start_rename(id, window, cx);
                    }
                }
                PickRoute::Shell(ShellPick::DuplicateTab) => {
                    if let Some(id) = self.tabs.active_id() {
                        self.duplicate_tab(id, cx);
                    }
                }
                // The Settings row never reaches the in-update arm: the
                // drawer's build reads the shell (the gc2 finding), so
                // the pick closure below routes it outside the update
                // borrow — this arm is unreachable for it.
                PickRoute::Shell(ShellPick::Settings) => {
                    println!("dropterm: context menu pick Settings (routed outside the update)");
                }
            },
            crate::context_menu::Pick::NewProfileTab(profile_index) => {
                self.open_profile_tab(profile_index, cx);
            }
            crate::context_menu::Pick::SwitchProfile(profile_index) => {
                if let Some(id) = self.tabs.active_id() {
                    self.switch_tab_profile(id, profile_index, cx);
                }
            }
        }
    }

    /// The one path every profile write takes (`write_config`'s
    /// discipline for the list-shaped key): persist through core,
    /// the echo drives every apply.
    pub(crate) fn write_profile(&mut self, write: &dropterm::core::settings::ProfileWrite) {
        println!(
            "dropterm: profile write at +{}ms",
            self.boot.elapsed().as_millis()
        );
        if let Err(err) = write.persist(&dropterm::core::config::store::store_dir(), &self.config) {
            eprintln!("dropterm: failed to persist profile write: {err}");
        }
    }

    /// The Profiles page's gestures, each one landing in core's
    /// model and (at most) one write — the libcosmic arms' rules
    /// verbatim, the draft lock held by the disabled rendering (no
    /// list gesture is reachable while a draft is open).
    pub(crate) fn profile_select(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(profile) = self.config.profiles.get(index) {
            self.settings.profile_selection = Some(index);
            self.settings.profile_draft =
                Some(dropterm::core::settings::ProfileDraft::edit(index, profile));
            cx.notify();
        }
    }

    pub(crate) fn profile_add(&mut self, cx: &mut Context<Self>) {
        self.settings.profile_draft = Some(dropterm::core::settings::ProfileDraft::add());
        cx.notify();
    }

    pub(crate) fn profile_clone(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(source) = self.config.profiles.get(index) {
            self.settings.profile_draft = Some(dropterm::core::settings::ProfileDraft::clone_of(
                source,
                &self.config.profiles,
            ));
            cx.notify();
        }
    }

    pub(crate) fn profile_draft_field(
        &mut self,
        field: dropterm::core::settings::ProfileField,
        text: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(draft) = self.settings.profile_draft.as_mut() {
            draft.set_field_text(field, text);
            cx.notify();
        }
    }

    /// A scheme pick buffers into the draft (the recolor lands on
    /// Save) and retires the scheme's stale error.
    pub(crate) fn profile_draft_scheme(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some(pick) = dropterm::core::settings::profile_scheme_write(row)
            && let Some(draft) = self.settings.profile_draft.as_mut()
        {
            draft.scheme = pick.field_value();
            draft
                .errors
                .retain(|(at, _)| *at != dropterm::core::settings::ProfileField::Scheme);
            cx.notify();
        }
    }

    pub(crate) fn profile_draft_env_add(&mut self, cx: &mut Context<Self>) {
        if let Some(draft) = self.settings.profile_draft.as_mut() {
            draft
                .env_rows
                .push(dropterm::core::settings::ProfileEnvRow::default());
            draft.errors.clear();
            cx.notify();
        }
    }

    pub(crate) fn profile_draft_env_remove(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(draft) = self.settings.profile_draft.as_mut() {
            draft.env_rows.remove(index);
            draft.errors.clear();
            cx.notify();
        }
    }

    /// The transactional commit: one plan, at most one whole-list
    /// write plus the paired rename cascade, then the form closes on
    /// the row it wrote; an invalid Save writes nothing and errors
    /// every offending field at once.
    pub(crate) fn profile_draft_save(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.settings.profile_draft.clone() else {
            return;
        };
        let pre_append_len = self.config.profiles.len();
        let plan = dropterm::core::settings::plan_profile_save(
            &draft,
            &self.config.profiles,
            self.config.default_profile.as_deref(),
        );
        match plan {
            dropterm::core::settings::ProfileSavePlan::Write {
                profiles,
                default_after_rename,
            } => {
                self.write_profile(&dropterm::core::settings::ProfileWrite::Profiles(profiles));
                if let Some(new_default) = default_after_rename {
                    self.write_profile(&dropterm::core::settings::ProfileWrite::DefaultProfile(
                        Some(new_default),
                    ));
                }
                self.settings.profile_selection = Some(
                    dropterm::core::settings::selection_after_save(&draft.mode, pre_append_len),
                );
                self.settings.profile_draft = None;
                println!(
                    "dropterm: profile saved at +{}ms",
                    self.boot.elapsed().as_millis()
                );
            }
            dropterm::core::settings::ProfileSavePlan::Invalid { errors } => {
                if let Some(open) = self.settings.profile_draft.as_mut() {
                    open.errors = errors;
                }
                println!(
                    "dropterm: profile save rejected (fields erroring) at +{}ms",
                    self.boot.elapsed().as_millis()
                );
            }
            // A stale edit or an unchanged draft closes clean — the
            // list moved under the form, or Save had nothing to do.
            _ => {
                self.settings.profile_draft = None;
            }
        }
        cx.notify();
    }

    pub(crate) fn profile_draft_cancel(&mut self, cx: &mut Context<Self>) {
        self.settings.profile_draft = None;
        cx.notify();
    }

    /// The delete cluster: Arm swaps the armed row (single-row
    /// arming), Confirm deletes through the plan (one whole-list
    /// write plus the paired auto-clear), Keep disarms.
    pub(crate) fn profile_delete_arm(&mut self, index: usize, cx: &mut Context<Self>) {
        self.settings.profile_delete_armed =
            (self.settings.profile_delete_armed != Some(index)).then_some(index);
        cx.notify();
    }

    pub(crate) fn profile_delete_confirm(&mut self, index: usize, cx: &mut Context<Self>) {
        self.settings.profile_delete_armed = None;
        if let Some((profiles, paired)) = dropterm::core::settings::plan_profile_delete(
            &self.config.profiles,
            index,
            self.config.default_profile.as_deref(),
        ) {
            self.write_profile(&dropterm::core::settings::ProfileWrite::Profiles(profiles));
            if let Some(write) = paired {
                self.write_profile(&write);
            }
        }
        cx.notify();
    }

    pub(crate) fn profile_delete_keep(&mut self, cx: &mut Context<Self>) {
        self.settings.profile_delete_armed = None;
        cx.notify();
    }

    pub(crate) fn profile_reorder(
        &mut self,
        index: usize,
        direction: dropterm::core::settings::ReorderDirection,
        cx: &mut Context<Self>,
    ) {
        // A boundary press plans nothing (the model's own no-op).
        if let Some(write) =
            dropterm::core::settings::plan_profile_reorder(&self.config.profiles, index, direction)
        {
            self.write_profile(&dropterm::core::settings::ProfileWrite::Profiles(write));
        }
        cx.notify();
    }

    /// The default-profile picker's pick: commit on change through
    /// the module's planner (a stale index rejects inside).
    pub(crate) fn profile_pick_default(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some(write) =
            dropterm::core::settings::default_picker_write(&self.config.profiles, row)
        {
            self.write_profile(&write);
        }
        cx.notify();
    }

    /// The one path every keybinds write takes: persist the whole
    /// `custom_key_binds` map, the echo drives the apply.
    fn write_keybinds(&mut self, write: &dropterm::core::settings::KeybindsWrite) {
        println!(
            "dropterm: keybinds write ({} custom entr{}) at +{}ms",
            write.0.len(),
            if write.0.len() == 1 { "y" } else { "ies" },
            self.boot.elapsed().as_millis()
        );
        if let Err(err) = write.persist(&dropterm::core::config::store::store_dir()) {
            eprintln!("dropterm: failed to persist keybinds write: {err}");
        }
    }

    /// The Keybindings page's gestures (the libcosmic arms' rules):
    /// arm a row's capture (clearing any standing conflict), disarm,
    /// revert a row's custom entry (its defaults come back on the
    /// echo), and buffer the search box.
    pub(crate) fn keybind_capture_start(
        &mut self,
        id: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.keybind_capture = Some(id);
        self.settings.keybind_conflict = None;
        println!("dropterm: keybind capture armed for {id}");
        // The drawer holds the keyboard while armed: its focus handle
        // takes it, the pane's claim resumes when the capture ends.
        self.settings_capture_focus.focus(window, cx);
        cx.notify();
    }

    pub(crate) fn keybind_capture_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.keybind_capture = None;
        // The keyboard returns to the active pane (the modal repair's
        // rule).
        if let Some(pane) = self.active_pane() {
            pane.read(cx).focus_handle().focus(window, cx);
        }
        cx.notify();
    }

    /// The armed capture's key gate — every press the drawer sees
    /// while armed: Escape cancels, bare modifiers stay armed, any
    /// other chord commits through the verdict (the conflict shown
    /// inline, the write only on Commit).
    pub(crate) fn keybind_capture_key(
        &mut self,
        keystroke: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.settings.keybind_capture else {
            return;
        };
        if keystroke.key == "escape" {
            println!("dropterm: keybind capture canceled");
            self.keybind_capture_end(window, cx);
            return;
        }
        let Some(key) = dropterm::core::keybinds::capture_key_name(&keystroke.key) else {
            return;
        };
        let mut modifiers = Vec::new();
        if keystroke.modifiers.control {
            modifiers.push(dropterm::core::keybinds::ModifierName::Ctrl);
        }
        if keystroke.modifiers.shift {
            modifiers.push(dropterm::core::keybinds::ModifierName::Shift);
        }
        if keystroke.modifiers.alt {
            modifiers.push(dropterm::core::keybinds::ModifierName::Alt);
        }
        if keystroke.modifiers.platform {
            modifiers.push(dropterm::core::keybinds::ModifierName::Super);
        }
        let binding = dropterm::core::keybinds::Binding { modifiers, key };
        match dropterm::core::keybinds::capture_verdict(id, &binding, &self.config.custom_key_binds)
        {
            dropterm::core::keybinds::CaptureVerdict::Commit => {
                let write = dropterm::core::keybinds::with_custom_binding(
                    &self.config.custom_key_binds,
                    id,
                    binding.clone(),
                );
                self.write_keybinds(&dropterm::core::settings::KeybindsWrite(write));
                println!(
                    "dropterm: keybind captured for {id}: {}",
                    dropterm::core::keybinds::binding_display(&binding)
                );
                self.keybind_capture_end(window, cx);
            }
            // Already the row's own: a silent disarm.
            dropterm::core::keybinds::CaptureVerdict::Own => {
                self.keybind_capture_end(window, cx);
            }
            dropterm::core::keybinds::CaptureVerdict::Conflict(holder) => {
                self.settings.keybind_conflict = Some((id, holder));
                cx.notify();
            }
            dropterm::core::keybinds::CaptureVerdict::Unsupported => {
                eprintln!("dropterm: captured chord cannot be stored; dropping capture");
                self.keybind_capture_end(window, cx);
            }
        }
    }

    pub(crate) fn keybind_revert(&mut self, id: &'static str, cx: &mut Context<Self>) {
        self.settings.keybind_conflict = None;
        let write =
            dropterm::core::keybinds::without_custom_binding(&self.config.custom_key_binds, id);
        self.write_keybinds(&dropterm::core::settings::KeybindsWrite(write));
        println!("dropterm: keybind reverted for {id}");
        cx.notify();
    }

    pub(crate) fn keybind_search(&mut self, text: String, cx: &mut Context<Self>) {
        self.settings.keybind_search = text;
        cx.notify();
    }

    /// Duplicates the tab with `source` (the menu's row): a fresh
    /// plain-shell pane in its cwd carrying its rename pin, landing
    /// after the active tab and activated (the record through core;
    /// the pane through the one spawning body — always a plain shell,
    /// never the default profile, ruling 1's duplicate leg).
    fn duplicate_tab(&mut self, source: TabId, cx: &mut Context<Self>) {
        let cwd = self
            .panes
            .get(&source)
            .and_then(|pane| pane.read(cx).working_directory());
        let id = self
            .tabs
            .spawn_duplicate(source, self.config.tab_title_mode);
        let command = spawn::SpawnCommand::Shell(self.config.resolved_shell());
        let Some((_terminal, pane)) = view::boot_pane(
            cx,
            &self.config,
            view::PaneRequest::for_command(&command, cwd, &self.config),
            self.specs.table.as_ref(),
            id,
            Some(self.signals_tx.clone()),
        ) else {
            eprintln!("dropterm: duplicate tab's terminal failed to spawn; closing its record");
            self.tabs.close(id);
            return;
        };
        self.panes.insert(id, pane);
        self.tabs.note_spawn_facts(id, None, false);
        println!(
            "dropterm: tab {source:?} duplicated -> {id:?} at +{}ms",
            self.boot.elapsed().as_millis()
        );
        cx.notify();
    }

    /// One forwarded tab verb's application (py-5), at the tick's
    /// apply point where pane spawning's App context lives — the
    /// libcosmic dispatch's semantics verbatim: close and rename are
    /// quiet state edits (nothing re-shows), while new-tab, select,
    /// duplicate, and switch-profile show the surface through the
    /// same show path a `new-tab` takes (mid-slide-out included).
    fn apply_tab_verb(
        &mut self,
        action: cli::Action,
        actions: &mut Vec<Action>,
        cx: &mut Context<Self>,
    ) {
        match action {
            cli::Action::NewTab { cwd, cmd, profile } => {
                self.open_remote_tab(cwd.as_ref(), cmd, profile, actions, cx);
            }
            cli::Action::CloseTab { tab_index } => self.close_remote_tab(tab_index, cx),
            cli::Action::SelectTab { index } => self.select_remote_tab(index, actions),
            cli::Action::RenameTab { title, tab_index } => {
                self.rename_remote_tab(&title, tab_index, cx);
            }
            cli::Action::DuplicateTab { tab_index } => {
                self.duplicate_remote_tab(tab_index, actions, cx);
            }
            cli::Action::SwitchProfileTab { profile, tab_index } => {
                self.switch_remote_profile_tab(&profile, tab_index, actions, cx);
            }
            // The surface verbs never queue here.
            _ => {}
        }
    }

    /// The remote `new-tab`'s spawn (the libcosmic `spawn_remote_tab`
    /// port): `--cmd` wins its arm outright, `--profile` resolves
    /// against the live config (a gone name warns and spawns the
    /// plain shell), an explicit `--cwd` keeps the spawn a plain
    /// shell even under a default profile (the user overrode the
    /// preset wholesale), and the bare remainder is the one branch
    /// the configured default owns. The cwd composes per arm — the
    /// explicit live directory over the preset's over the active
    /// tab's — and the spawn rides the one spawning body.
    fn open_remote_tab(
        &mut self,
        cwd: Option<&std::path::PathBuf>,
        cmd: Option<String>,
        profile: Option<String>,
        actions: &mut Vec<Action>,
        cx: &mut Context<Self>,
    ) {
        // Flag presence, not live-ness: an explicit --cwd guards the
        // plain-shell branch even when the directory turns out dead
        // (the user asked for an override; a dead one warns and
        // degrades within that override, never back to the default
        // profile).
        let explicit_cwd = cwd.is_some();
        let explicit = spawn::live_dir("--cwd", cwd);
        let inherited = self.active_cwd(cx);
        let command = match (cmd, profile) {
            (Some(line), _) => spawn::SpawnCommand::CommandLine(line),
            (None, Some(name)) => dropterm::core::profile::find(&self.config.profiles, &name)
                .map_or_else(
                    || {
                        println!("dropterm: no profile named {name}; spawning the shell instead");
                        spawn::SpawnCommand::Shell(self.config.resolved_shell())
                    },
                    |profile| spawn::SpawnCommand::Profile {
                        name: profile.name.clone(),
                        command: profile.command.clone(),
                        cwd: profile.cwd.clone(),
                        env: profile.env.clone(),
                    },
                ),
            (None, None) if explicit_cwd => {
                spawn::SpawnCommand::Shell(self.config.resolved_shell())
            }
            (None, None) => spawn::default_spawn_command(&self.config),
        };
        let spawn_cwd = match &command {
            spawn::SpawnCommand::Profile {
                name, cwd: preset, ..
            } => explicit
                .clone()
                .or_else(|| spawn::live_preset_cwd(name, preset.as_ref()))
                .or(inherited.clone()),
            spawn::SpawnCommand::CommandLine(_) => {
                dropterm::core::tabs::resolve_new_tab_cwd(explicit, inherited)
            }
            spawn::SpawnCommand::Shell(_) => {
                spawn::plain_spawn_cwd(&self.config, explicit, inherited)
            }
        };
        if let Some(id) = self.spawn_command_tab(cx, &command, spawn_cwd) {
            println!(
                "dropterm: remote new-tab -> {id:?} ({} tabs) at +{}ms",
                self.tabs.records().len(),
                self.boot.elapsed().as_millis()
            );
        }
        self.show_intent(actions);
    }

    /// The remote `close-tab` (the libcosmic `close_remote_tab`
    /// port): the 1-based `--tab-index` resolves through core —
    /// absent means the active tab — and the close rides the same
    /// gated decision the front door runs, windowless (the modal's
    /// render arm-completion takes the keyboard when it paints). An
    /// unresolvable target warns and no-ops: the fire-and-forget call
    /// has no reply to carry the failure.
    fn close_remote_tab(&mut self, tab_index: Option<usize>, cx: &mut Context<Self>) {
        let len = self.tabs.records().len();
        match dropterm::core::tabs::remote_target_index(len, tab_index, self.tabs.active_index()) {
            Some(index) => {
                let id = self.tabs.records()[index].id;
                self.request_close_windowless(id, cx);
            }
            None => {
                println!("dropterm: close-tab: no tab at --tab-index {tab_index:?} ({len} open)");
            }
        }
    }

    /// The gated close's windowless arm (py-5): the same decision the
    /// front door runs — a pinned tab refuses (warn, no-op), a
    /// running foreground child under the confirm toggle arms the
    /// modal, everything else closes now — but with no window on the
    /// stack the modal's focus claim cannot run here; the render's
    /// arm-completion takes the keyboard when the modal first paints.
    fn request_close_windowless(&mut self, id: TabId, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.records().iter().find(|tab| tab.id == id) else {
            return;
        };
        if tab.pinned {
            println!(
                "dropterm: refusing to close pinned tab {id:?}; unpin it first at +{}ms",
                self.boot.elapsed().as_millis()
            );
            return;
        }
        let running = self
            .panes
            .get(&id)
            .is_some_and(|pane| pane.read(cx).foreground_child_running());
        if tab_bar::confirm_needed(self.config.close_confirmation_enabled, running) {
            self.pending_close = Some(id);
            println!(
                "dropterm: close {id:?} gated -> the confirm modal armed (windowless) \
                 at +{}ms",
                self.boot.elapsed().as_millis()
            );
            cx.notify();
        } else {
            self.close_now(id, cx);
        }
    }

    /// The remote `select-tab`: the strict 1-based `N` resolves
    /// through core, the activation rides the list's own select
    /// (attention retirement included), and the surface shows — an
    /// unresolvable target is a quiet no-op (nothing re-shows; a
    /// script's failed select must not disturb what the user is
    /// looking at).
    fn select_remote_tab(&mut self, one_based: usize, actions: &mut Vec<Action>) {
        match dropterm::core::tabs::select_tab_target(self.tabs.records().len(), one_based) {
            Some(index) => {
                let id = self.tabs.records()[index].id;
                if self.tabs.select(id) {
                    println!(
                        "dropterm: select-tab {one_based} -> {id:?} at +{}ms",
                        self.boot.elapsed().as_millis()
                    );
                }
                self.show_intent(actions);
            }
            None => println!(
                "dropterm: select-tab: no tab at index {one_based} ({} open)",
                self.tabs.records().len()
            ),
        }
    }

    /// The remote `rename-tab`: the target resolves like close-tab's,
    /// any inline rename open on that tab is discarded first (a
    /// pending draft must not commit over the remote title), then
    /// core's `commit_rename` applies the payload — a non-empty TITLE
    /// pins the override, the empty TITLE clears it. Quiet.
    fn rename_remote_tab(&mut self, title: &str, tab_index: Option<usize>, cx: &mut Context<Self>) {
        let len = self.tabs.records().len();
        match dropterm::core::tabs::remote_target_index(len, tab_index, self.tabs.active_index()) {
            Some(index) => {
                let id = self.tabs.records()[index].id;
                if self.rename.as_ref().is_some_and(|edit| edit.tab == id) {
                    self.rename = None;
                }
                self.tabs
                    .commit_rename(id, title, self.config.tab_title_mode);
                println!(
                    "dropterm: rename-tab -> {id:?} {:?} at +{}ms",
                    title,
                    self.boot.elapsed().as_millis()
                );
                cx.notify();
            }
            None => {
                println!("dropterm: rename-tab: no tab at --tab-index {tab_index:?} ({len} open)");
            }
        }
    }

    /// The remote `duplicate-tab`: the target — absent means the
    /// active tab — resolves through core; an explicit but
    /// unresolvable index warns and degrades to the active tab (the
    /// same source the warning names). The spawn rides the menu's own
    /// `duplicate_tab` body (after-active landing, activation, the
    /// fresh-shell-in-cwd rule), and the surface shows.
    fn duplicate_remote_tab(
        &mut self,
        tab_index: Option<usize>,
        actions: &mut Vec<Action>,
        cx: &mut Context<Self>,
    ) {
        let len = self.tabs.records().len();
        let target =
            dropterm::core::tabs::remote_target_index(len, tab_index, self.tabs.active_index());
        if target.is_none() && tab_index.is_some() {
            println!(
                "dropterm: duplicate-tab: no tab at --tab-index {tab_index:?} ({len} open); \
                 duplicating the active tab instead"
            );
        }
        let source = target
            .or(Some(self.tabs.active_index()))
            .and_then(|index| self.tabs.records().get(index))
            .map(|tab| tab.id);
        if let Some(source) = source {
            self.duplicate_tab(source, cx);
        }
        self.show_intent(actions);
    }

    /// The remote `switch-profile-tab`: the name resolves exactly and
    /// case-sensitively against the live config, the target like
    /// every remote tab verb's; the respawn rides the shared in-place
    /// body. Unresolvable inputs warn and edit nothing — unlike
    /// duplicate-tab there is no sensible bare fallback — while the
    /// surface half still shows.
    fn switch_remote_profile_tab(
        &mut self,
        profile: &str,
        tab_index: Option<usize>,
        actions: &mut Vec<Action>,
        cx: &mut Context<Self>,
    ) {
        let Some(profile_index) = self
            .config
            .profiles
            .iter()
            .position(|candidate| candidate.name == profile)
        else {
            println!("dropterm: switch-profile-tab: no profile named {profile}");
            self.show_intent(actions);
            return;
        };
        let len = self.tabs.records().len();
        let Some(target) =
            dropterm::core::tabs::remote_target_index(len, tab_index, self.tabs.active_index())
        else {
            println!(
                "dropterm: switch-profile-tab: no tab at --tab-index {tab_index:?} \
                 ({len} open)"
            );
            self.show_intent(actions);
            return;
        };
        let id = self.tabs.records()[target].id;
        self.switch_tab_profile(id, profile_index, cx);
        self.show_intent(actions);
    }

    /// The in-place profile respawn (py-5, the libcosmic
    /// `switch_tab_profile` port): the tab keeps its slot, activation
    /// state, and sticky user rename while its pane is replaced by a
    /// fresh PTY running the preset — the respawn cwd is the preset's
    /// live directory when it has one, else the tab's own — and the
    /// field half lands through core's `plan_respawn`. A failed PTY
    /// keeps the live tab untouched.
    fn switch_tab_profile(&mut self, id: TabId, profile_index: usize, cx: &mut Context<Self>) {
        let Some(command) = spawn::profile_pick(&self.config, profile_index) else {
            return;
        };
        let spawn::SpawnCommand::Profile {
            name, cwd: preset, ..
        } = &command
        else {
            return;
        };
        let respawn_cwd = spawn::live_preset_cwd(name, preset.as_ref()).or_else(|| {
            self.panes
                .get(&id)
                .and_then(|pane| pane.read(cx).working_directory())
        });
        let old_override = self
            .tabs
            .records()
            .iter()
            .find(|tab| tab.id == id)
            .and_then(|tab| tab.title_override.clone());
        let request = view::PaneRequest::for_command(&command, respawn_cwd.clone(), &self.config);
        let Some((_terminal, pane)) = view::boot_pane(
            cx,
            &self.config,
            request,
            self.specs.table.as_ref(),
            id,
            Some(self.signals_tx.clone()),
        ) else {
            eprintln!("dropterm: profile respawn's terminal failed to spawn; keeping the live tab");
            return;
        };
        // The swap: the old pane drops with its entry (the child dies
        // the close path's own way), and the respawn's field half —
        // the resolved title, the kept pin, the persistent profile
        // seat — lands through core's plan.
        drop(self.panes.insert(id, pane));
        let fields = dropterm::core::tabs::plan_respawn(
            self.config.tab_title_mode,
            id,
            respawn_cwd.as_deref(),
            old_override.as_deref(),
            name,
        );
        self.tabs.apply_respawn(id, fields);
        println!(
            "dropterm: tab {id:?} respawned under profile {name:?} at +{}ms",
            self.boot.elapsed().as_millis()
        );
        cx.notify();
    }

    /// The gear affordance's press and the menu's Settings… row: the
    /// one settings trigger — a repeat press closes. A free function,
    /// not a shell method, by necessity (the gc2 walk's structural
    /// finding): the drawer's construction reads the shell, and a
    /// read while the shell is being updated panics — so the build
    /// happens outside the update borrow, the state flip inside it.
    pub(crate) fn toggle_settings(shell: &Entity<Shell>, window: &mut Window, cx: &mut App) {
        let open = shell.read(cx).settings.open;
        if open {
            shell.update(cx, |shell, cx| shell.close_settings(window, cx));
            return;
        }
        let drawer = crate::drawer::SettingsDrawer::new(shell.clone(), window, cx);
        shell.update(cx, |shell, cx| {
            // The press commits an open rename first, exactly like
            // the reference bar's corner affordances.
            shell.commit_open_rename(cx);
            shell.settings_drawer = Some(drawer);
            shell.settings.open = true;
            println!(
                "dropterm: settings drawer opened at +{}ms (the surface stays live beneath)",
                shell.boot.elapsed().as_millis()
            );
            cx.notify();
        });
    }

    /// Close the drawer: the explicit close clears the buffer (the
    /// reopen-fresh rule; a focus-loss hide preserves — the shell
    /// hides with the surface and this path never runs for a hide).
    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window;
        self.settings.open = false;
        self.settings_drawer = None;
        self.settings
            .buffer
            .apply_drawer_transition(dropterm::core::settings::DrawerTransition::ExplicitClose);
        println!(
            "dropterm: settings drawer closed at +{}ms",
            self.boot.elapsed().as_millis()
        );
        cx.notify();
    }

    /// The one write path every drawer commit rides (the libcosmic
    /// `write_config`'s discipline): clamp, persist through the
    /// store, and let the watcher's echo drive every apply — the
    /// drawer never applies anything itself, and drawer writes and
    /// hand edits remain one event stream.
    pub(crate) fn write_settings(&mut self, write: &dropterm::core::settings::PlannedWrite) {
        let write = write.clamped();
        let key = write.key();
        println!(
            "dropterm: settings write {} at +{}ms",
            key.key_name(),
            self.boot.elapsed().as_millis()
        );
        if let Err(err) = write.persist(&dropterm::core::config::store::store_dir(), &self.config) {
            eprintln!(
                "dropterm: failed to persist settings write {}: {err}",
                key.key_name()
            );
        }
    }

    /// One title sync (the tick's apply point): re-title every tab
    /// through core under the live mode — the mode-flip detector
    /// first, then each pane's polled OSC 0/2 report and cwd read.
    fn sync_tab_titles(&mut self, cx: &mut Context<Self>) {
        let mode = self.config.tab_title_mode;
        if mode != self.applied_title_mode {
            println!("dropterm: tab title mode -> {mode} (re-titling live tabs)");
            self.tabs.refresh_titles(mode);
            self.applied_title_mode = mode;
        }
        let ids: Vec<TabId> = self.tabs.records().iter().map(|tab| tab.id).collect();
        for id in ids {
            let Some(pane) = self.panes.get(&id) else {
                continue;
            };
            let (report, cwd) = {
                let pane = pane.read(cx);
                (pane.shell_title(), pane.working_directory())
            };
            if self.tabs.sync_title(id, report, cwd, mode)
                && let Some(tab) = self.tabs.records().iter().find(|tab| tab.id == id)
            {
                println!("dropterm: tab {id:?} retitled -> {}", tab.title);
            }
        }
    }

    /// One attention poll (the tick's apply point beside the title
    /// sync): each pane's latched bell/output signals land on its tab
    /// through core's transitions — inactive tabs only; the active
    /// tab's signals drop, the user is already looking at it (the
    /// reference app's own rule, decided here at the poll because the
    /// pane cannot know which tab is active).
    fn poll_tab_attention(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<TabId> = self.tabs.records().iter().map(|tab| tab.id).collect();
        for id in ids {
            let Some(pane) = self.panes.get(&id) else {
                continue;
            };
            let (bell, output) = pane.update(cx, |pane, _| pane.take_attention());
            if !bell && !output {
                continue;
            }
            // A bell while the surface is hidden posts one desktop
            // notification per tab per hide period (gpui-parity Task
            // 3; the libcosmic event arm's port — any tab, the active
            // one included, because a hidden surface shows no marker
            // the user can see). While visible the marker alone
            // carries the signal.
            if bell
                && crate::notify::should_notify(
                    matches!(self.machine.state, SurfaceState::Hidden),
                    self.config.bell_notification_enabled,
                    self.tabs.bell_latched(id),
                )
            {
                self.tabs.note_bell_notified(id);
                let title = self
                    .tabs
                    .records()
                    .iter()
                    .find(|tab| tab.id == id)
                    .map_or_else(|| "Terminal".to_string(), |tab| tab.title.clone());
                println!("dropterm: bell in {title:?} while hidden -> one notification");
                crate::notify::post_bell_notification(title);
            }
            let active = self.tabs.active_id() == Some(id);
            self.tabs.note_attention(id, bell, output);
            if active {
                // Output on the active tab while the surface is
                // hidden leaves no other trace — the markers accrue
                // only for inactive tabs — so the engine's
                // missed-activity latch is that trace
                // (status-activity spec), armed here at the poll the
                // reference arms at the wakeup.
                if output && matches!(self.machine.state, SurfaceState::Hidden) {
                    let engine = pane.read(cx).terminal.clone();
                    engine.lock().expect("engine lock").note_missed_activity();
                }
                continue;
            }
            let marker = self
                .tabs
                .records()
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| tab.attention);
            println!(
                "dropterm: tab {id:?} attention -> {} ({}) at +{}ms",
                marker.map_or("none", |state| state.marker().unwrap_or("none")),
                if bell { "bell" } else { "output" },
                self.boot.elapsed().as_millis(),
            );
        }
        // An open rename whose host vanished ends here (the close
        // paths' orphan rule).
        self.prune_rename(cx);
    }

    /// The copy verb (gn-2): the payload lands on GPUI's clipboard
    /// and the segment's `✓` flashes the reference band's own
    /// 800 ms — a clicked surface holds the keyboard and the serial,
    /// so GPUI's own clipboard path carries it.
    fn status_copy(
        &mut self,
        id: dropterm::core::status::SegmentId,
        payload: String,
        cx: &mut Context<Self>,
    ) {
        println!(
            "dropterm: status copy {id:?} ({} bytes) at +{}ms",
            payload.len(),
            self.boot.elapsed().as_millis()
        );
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(payload));
        self.status_copied = Some((id, Instant::now() + STATUS_COPY_FLASH));
        cx.notify();
    }

    /// The scrolled chip's jump verb (gn-2): the active pane's
    /// engine scrolls to the bottom — the reference band's one
    /// deliberate action chip, the same engine call the libcosmic
    /// click rides.
    fn scroll_active_to_bottom(&mut self, cx: &mut Context<Self>) {
        let Some(pane) = self.active_pane() else {
            return;
        };
        let engine = pane.read(cx).terminal.clone();
        engine
            .lock()
            .expect("engine lock")
            .scroll(dropterm::core::terminal::TerminalScroll::Bottom);
        println!(
            "dropterm: status scrolled-chip jump -> bottom at +{}ms",
            self.boot.elapsed().as_millis()
        );
    }

    /// The pasted chip's door (py-6): the panel opens over the active
    /// tab's ledger, its missing thumbnails decoding off the UI
    /// thread's clock — the reference panel's batch-open arm.
    fn open_images_panel(&mut self, cx: &mut Context<Self>) {
        self.images_open = true;
        println!(
            "dropterm: images panel open at +{}ms",
            self.boot.elapsed().as_millis()
        );
        let Some(id) = self.tabs.active_id() else {
            cx.notify();
            return;
        };
        let missing: Vec<std::path::PathBuf> = self
            .tabs
            .pasted_images(id)
            .map(|rows| {
                rows.iter()
                    .map(|row| row.path.clone())
                    .filter(|path| !self.images_thumbs.contains_key(path))
                    .collect()
            })
            .unwrap_or_default();
        if missing.is_empty() {
            cx.notify();
            return;
        }
        cx.spawn(async move |shell, cx| {
            let thumbs = cx
                .background_spawn(async move {
                    missing
                        .iter()
                        .filter_map(|path| {
                            crate::images_panel::decode_thumb(path).map(|handle| {
                                println!("dropterm: panel thumb decoded {}", path.display());
                                (path.clone(), handle)
                            })
                        })
                        .collect::<HashMap<_, _>>()
                })
                .await;
            if let Err(err) = shell.update(cx, |shell, cx| {
                shell.images_thumbs.extend(thumbs);
                cx.notify();
            }) {
                println!("dropterm: panel thumb store dropped ({err})");
            }
        })
        .detach();
        cx.notify();
    }

    /// The panel's close door: the open drops with the armed pair
    /// (an armed pair never survives the close).
    fn close_images_panel(&mut self, cx: &mut Context<Self>) {
        self.images_open = false;
        self.images_armed = None;
        println!(
            "dropterm: images panel closed at +{}ms",
            self.boot.elapsed().as_millis()
        );
        cx.notify();
    }

    /// One images-panel verb (py-6, the reference handlers' port):
    /// the row verbs through the paths the reference rides, the
    /// cluster's arm/disarm through the single-arm seat, and the
    /// doors.
    fn images_verb(
        &mut self,
        verb: crate::images_panel::PanelVerb,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::images_panel::PanelVerb;
        match verb {
            PanelVerb::Open(path) => {
                // The OpenHyperlink precedent: hand it to the system
                // viewer, one warn on refusal.
                if let Err(err) = open::that_detached(&path) {
                    println!("dropterm: failed to open {}: {err}", path.display());
                }
            }
            PanelVerb::CopyPath(path) => {
                let text = path.to_string_lossy().into_owned();
                println!(
                    "dropterm: panel copy path {} at +{}ms",
                    path.display(),
                    self.boot.elapsed().as_millis()
                );
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }
            PanelVerb::CopyImage(path) => {
                // The read runs off-thread (the cache file can be
                // megabytes); only a real read reaches the write, in
                // the sniffed format.
                std::thread::spawn(move || match std::fs::read(&path) {
                    Ok(bytes) => {
                        let mime = dropterm::core::paste_cache::sniff_image_format(&bytes)
                            .map_or("application/octet-stream", |format| format.mime());
                        crate::terminal::clipboard::write_image_data_control(bytes, mime);
                    }
                    Err(err) => {
                        println!(
                            "dropterm: panel image read failed for {}: {err}",
                            path.display()
                        );
                    }
                });
            }
            PanelVerb::Repaste(path) => {
                // The same channel a text paste uses — bracketed
                // paste rides it unchanged.
                if let Some(pane) = self.active_pane() {
                    let engine = pane.read(cx).terminal.clone();
                    let text = path.to_string_lossy();
                    engine.lock().expect("engine lock").paste(&text);
                    println!(
                        "dropterm: panel re-paste {} at +{}ms",
                        path.display(),
                        self.boot.elapsed().as_millis()
                    );
                }
            }
            PanelVerb::ArmDelete(path) => {
                // The pair swaps to the pressed row: the single-arm
                // seat both disarms any other armed row and never
                // leaves two pairs standing.
                self.images_armed = Some(path);
                cx.notify();
            }
            PanelVerb::KeepDelete => {
                self.images_armed = None;
                cx.notify();
            }
            PanelVerb::ConfirmDelete(path) => {
                let id = self.tabs.active_id();
                let dropped = id.is_some_and(|id| self.tabs.forget_pasted_image(id, &path));
                self.images_armed = None;
                self.images_thumbs.remove(&path);
                if dropped {
                    println!(
                        "dropterm: panel deleted {} at +{}ms",
                        path.display(),
                        self.boot.elapsed().as_millis()
                    );
                    // The row's file, best-effort off-thread — the
                    // cache file is the paste's only durable copy, so
                    // the delete owns it too.
                    std::thread::spawn(move || {
                        if let Err(err) = std::fs::remove_file(&path) {
                            println!(
                                "dropterm: panel file removal failed for {}: {err}",
                                path.display()
                            );
                        }
                    });
                }
                cx.notify();
            }
            PanelVerb::OpenCache => {
                // The OpenHyperlink precedent for the folder: the file
                // manager, one warn on refusal — including the no-XDG
                // case.
                match dropterm::core::paste_cache::paste_cache_dir() {
                    Some(dir) => {
                        if let Err(err) = open::that_detached(&dir) {
                            println!("dropterm: failed to open {}: {err}", dir.display());
                        }
                    }
                    None => println!(
                        "dropterm: paste cache unavailable: no XDG cache directory resolves"
                    ),
                }
            }
            PanelVerb::Close => self.close_images_panel(cx),
        }
    }

    /// Every show clears the missed-activity latches
    /// (status-activity spec): showing is a touch verb — the user is
    /// looking at the surface again, and the halo's news is spent.
    /// Detected at the tick off the hidden reading's flip.
    fn clear_activity_on_show(&mut self, cx: &mut Context<Self>) {
        let hidden = matches!(self.machine.state, SurfaceState::Hidden);
        if self.was_hidden && !hidden {
            for pane in self.panes.values() {
                let engine = pane.read(cx).terminal.clone();
                engine.lock().expect("engine lock").clear_missed_activity();
            }
            // The show re-arms the bell-notification latches: the next
            // hide period starts clean, a bell after the next hide
            // notifying again.
            self.tabs.clear_bell_latches();
        }
        self.was_hidden = hidden;
    }

    /// The status band element: the stored segments through the
    /// view, in the strip's own pane-derived colors — derived, not
    /// decorated (the reference band's rule). The border hairline
    /// faces the terminal: the band's top edge when it sits at the
    /// column's bottom (a top-anchored drawer), its bottom edge
    /// otherwise. The click verbs route back through the shell
    /// entity (the strip's own callback pattern), and the token's
    /// one resolved red rides the scheme's pair through the strip's
    /// own luminance flip.
    fn status_band(
        &self,
        cx: &mut Context<Self>,
        pane_background: Option<alacritty_terminal::vte::ansi::Rgb>,
    ) -> status_bar::StatusBandView {
        let (strip, fg) = tab_bar::strip_pair(pane_background);
        let width = self
            .last_bounds
            .map_or(0.0, |bounds| bounds.size.width.as_f32());
        let red = self
            .status
            .attention_red
            .map(|(r, g, b)| gpui::Rgba { r, g, b, a: 1. }.into());
        let bright = self
            .status
            .attention_bright_red
            .map(|(r, g, b)| gpui::Rgba { r, g, b, a: 1. }.into());
        let attention = status_bar::attention_color(red, bright, fg, strip);
        let shell = cx.entity();
        let on_copy = {
            let shell = shell.clone();
            move |id, payload, cx: &mut gpui::App| {
                shell.update(cx, |shell, cx| shell.status_copy(id, payload, cx));
            }
        };
        let on_scroll_bottom = {
            let shell = shell.clone();
            move |id: dropterm::core::status::SegmentId, cx: &mut gpui::App| {
                // The two action chips' own verbs (py-6 grew the
                // pair): the scrolled chip's jump, the pasted chip's
                // panel door.
                shell.update(cx, |shell, cx| match id {
                    dropterm::core::status::SegmentId::Pasted => {
                        shell.open_images_panel(cx);
                    }
                    _ => Shell::scroll_active_to_bottom(shell, cx),
                });
            }
        };
        status_bar::StatusBandView::new(self.status.clone(), strip, fg)
            .font(self.band_font.clone())
            .show_border(self.config.status_bar_border)
            .at_bottom(self.inputs.geometry.edge == Edge::Top)
            .width(width)
            .copied(self.status_copied.map(|(id, _)| id))
            .paste_refusal(self.status_paste_refusal.map(|(r, _)| r))
            .attention(Some(attention))
            .on_copy(on_copy)
            .on_scroll_bottom(on_scroll_bottom)
    }

    /// The status band's refresh (gpui-status Task 1): the clock's
    /// text under its gate, then the active tab's segments assembled
    /// from the engine's own reads plus the config — one snapshot
    /// per tick, so the render path performs zero filesystem reads
    /// by construction (the reference band's own pin). The OSC-fed
    /// seats (the reported cwd, the command timer's verdicts,
    /// progress, program status) ride Task 2's wiring; every seat
    /// the plain state feeds resolves today, and the profile seat
    /// stays honestly absent until a profile spawn path exists.
    #[expect(
        clippy::too_many_lines,
        reason = "the reference app's own assembly shape: every seat's read beside its gate, one StatusInputs build"
    )]
    fn refresh_status_band(&mut self, cx: &mut Context<Self>) {
        // The copy flash's expiry (gn-2): the tick clears the seat
        // when its 800 ms window passes — the value returns on the
        // next frame.
        if let Some((_, until)) = self.status_copied
            && Instant::now() >= until
        {
            self.status_copied = None;
            cx.notify();
        }
        // The refusal verdict's expiry (the audit's F7): the tick
        // clears the chip when its 3 s window passes.
        if let Some((_, until)) = self.status_paste_refusal
            && Instant::now() >= until
        {
            self.status_paste_refusal = None;
            cx.notify();
        }
        let toggles = self.config.status_toggles();
        // The clock's gate: band and segment keys both hold — the
        // stored text recomputes each tick and flips exactly at the
        // minute boundary (a pure format of now, never a timer of
        // this shell's own; the tick is the wake).
        let clock = if status::clock_due(
            self.config.status_bar_enabled,
            self.config.status_bar_show_clock,
        ) {
            let text = status::clock_text(&jiff::Zoned::now());
            if self.status_clock.as_deref() != Some(text.as_str()) {
                // The minute-boundary row (gn-1's walk evidence): the
                // text flips only at a boundary, so each logged flip
                // names the tick the band keeps time on.
                println!(
                    "dropterm: status clock flipped to {text} at +{}ms",
                    self.boot.elapsed().as_millis()
                );
            }
            Some(text)
        } else {
            None
        };
        self.status_clock = clock;
        let Some(pane) = self.active_pane() else {
            self.status = StatusSegments::default();
            return;
        };
        // Every engine read the inputs need, under one guard — then
        // the two filesystem reads outside it (the reference's own
        // split: the comm read and the `.git` walk are the band's
        // lone syscalls, refreshed at event points, never per
        // frame). The OSC-fed seats ride the same guard (gn-2): the
        // standing report the engine holds, the timer's verdicts,
        // the missed-activity latch, and the scheme's red pair —
        // each in-memory read gathered under its own key's gate,
        // key off meaning never gathered (the resolver's
        // hidden/absent indistinguishability).
        let engine = pane.read(cx).terminal.clone();
        let (
            foreground,
            cwd,
            zoom_adj,
            grid,
            spawn_grid,
            scrolled_back,
            reported_cwd,
            exit_status,
            (live_running, running_elapsed),
            frozen_duration,
            progress,
            program_status,
            missed_activity,
            attention_red,
            attention_bright_red,
        ) = {
            let mut engine = engine.lock().expect("engine lock");
            // A dead reporter's last number never outlives its
            // window: the decay lands at the tick within 100 ms of
            // the deadline (the reference sleeps to the instant;
            // this shell's tick is the wake).
            engine.expire_progress();
            let frozen_duration = engine.command_frozen();
            let colors = engine.colors();
            let scheme_red = |rgb: alacritty_terminal::vte::ansi::Rgb| {
                (
                    f32::from(rgb.r) / 255.0,
                    f32::from(rgb.g) / 255.0,
                    f32::from(rgb.b) / 255.0,
                )
            };
            (
                engine.foreground_process_name(),
                engine.working_directory(),
                engine.zoom_adj(),
                engine.grid(),
                engine.spawn_grid(),
                engine.scrolled_back_lines(),
                // The path seat's authoritative source (osc7-cwd
                // spec), gathered ungated like the ssh stand-in it
                // outranks: the branch rule reads the fact even when
                // the path key hides the seat.
                engine.reported_cwd(),
                toggles.exit.then(|| engine.command_exit()).flatten(),
                engine.command_live(),
                frozen_duration,
                toggles
                    .progress
                    .then(|| engine.reported_progress())
                    .flatten(),
                toggles
                    .program_status
                    .then(|| engine.program_status())
                    .flatten(),
                status::activity_mark_due(
                    self.config.status_bar_enabled,
                    self.config.status_bar_activity_mark,
                ) && engine.missed_activity(),
                colors[alacritty_terminal::vte::ansi::NamedColor::Red as usize].map(scheme_red),
                colors[alacritty_terminal::vte::ansi::NamedColor::BrightRed as usize]
                    .map(scheme_red),
            )
        };
        // The ssh stand-in (status-remote spec): the cmdline read
        // waits for the band, the path seat, and an ssh foreground —
        // the engine re-verifies the comm before the read itself.
        let ssh_destination = status::ssh_read_due(
            self.config.status_bar_enabled,
            self.config.status_bar_show_path,
            foreground.as_deref() == Some("ssh"),
        )
        .then(|| engine.lock().expect("engine lock").ssh_destination())
        .flatten();
        // The narration name for the frozen pairing (command-lifecycle
        // OQ-1): the last comm sampled running on this tab, kept
        // per-tab so a switch can never pair one tab's name with
        // another's duration.
        if let (Some(id), Some(name)) = (self.tabs.active_id(), foreground.as_deref()) {
            self.tabs.note_running_comm(id, name.to_string());
        }
        let frozen_name = self
            .tabs
            .active_id()
            .and_then(|id| self.tabs.running_comm(id));
        // Hidden means never gathered: the `.git` walk stops at the
        // gate, before the filesystem.
        let branch = (self.config.status_bar_enabled && self.config.status_bar_show_branch)
            .then(|| status::git_branch(cwd.as_deref()))
            .flatten();
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let shell = self.config.resolved_shell();
        let frozen =
            frozen_duration.map(|duration| (frozen_name.clone().unwrap_or_default(), duration));
        let inputs = StatusInputs {
            home: home.as_deref(),
            cwd: cwd.as_deref(),
            reported_cwd,
            hostname: self.hostname.as_deref(),
            shell: &shell,
            // The profile seat (py-5): the active tab's id resolved
            // against the live config — a deleted or renamed profile
            // reads absent, never a stale name.
            profile_name: self
                .tabs
                .active_id()
                .and_then(|id| self.tabs.profile_id_of(id))
                .and_then(|id| dropterm::core::profile::find(&self.config.profiles, id))
                .map(|profile| profile.name.as_str()),
            zoom_adj,
            // The zoom step every spawn builds from — core's own
            // default, the same leg the libcosmic assembly reads.
            zoom_step_mul_100: dropterm::core::terminal::TerminalOptions::default()
                .font_size_zoom_step_mul_100,
            foreground: foreground.as_deref(),
            branch: branch.as_deref(),
            grid: Some(grid),
            spawn_grid: Some(spawn_grid),
            tab_position: self.tabs.position().map(|(index, total)| {
                (
                    u16::try_from(index + 1).unwrap_or(u16::MAX),
                    u16::try_from(total).unwrap_or(u16::MAX),
                )
            }),
            clock: self.status_clock.as_deref(),
            running_elapsed: live_running.then_some(running_elapsed).flatten(),
            frozen: frozen
                .as_ref()
                .map(|(name, duration)| (name.as_str(), *duration)),
            exit_status,
            scrolled_back: Some(scrolled_back),
            // The pasted chip (py-6): the active tab's ledger count —
            // the chip's visibility is its conditionality, zero
            // pastes reads the same as absent.
            pasted: self
                .tabs
                .active_id()
                .and_then(|id| self.tabs.pasted_images(id))
                .map(<[dropterm::core::tabs::PastedImage]>::len),
            progress,
            program_status,
            ssh_destination: ssh_destination.as_deref(),
            missed_activity,
            attention_red,
            attention_bright_red,
        };
        let resolved = status::resolve_segments(&inputs, toggles);
        // The command timer's boundary rows (the duration rows' walk
        // evidence, the clock flip's own class): one row at each
        // start (the live elapsed's first stand) and each finish
        // (the frozen pair's landing), never per tick.
        let timer_state = (
            resolved.running_elapsed.is_some(),
            resolved.frozen.clone(),
            resolved.exit_code,
        );
        if timer_state != self.status_timer_logged {
            println!(
                "dropterm: status timer live={} frozen={:?} exit={:?} at +{}ms",
                timer_state.0,
                timer_state.1,
                timer_state.2,
                self.boot.elapsed().as_millis()
            );
            self.status_timer_logged = timer_state;
        }
        self.status = resolved;
    }

    /// The strip element with every interaction wired (the strip is
    /// view-only: each callback routes one shell call, itself routed
    /// through core).
    #[expect(
        clippy::let_and_return,
        reason = "the one binding names the built element between the long builder chain and the return"
    )]
    fn tab_strip(
        &self,
        cx: &mut Context<Self>,
        at_bottom: bool,
        strip: gpui::Hsla,
        fg: gpui::Hsla,
    ) -> tab_bar::TabBarView {
        let shell = cx.entity();
        let bar = tab_bar::TabBarView::new(self.tabs.rows(), strip, fg)
            .show_border(self.config.tab_bar_border)
            .at_bottom(at_bottom)
            .on_select({
                let shell = shell.clone();
                move |id, _window, cx: &mut App| {
                    shell.update(cx, |shell, cx| shell.select_tab(id, cx));
                }
            })
            .on_close({
                let shell = shell.clone();
                move |id, window, cx: &mut App| {
                    shell.update(cx, |shell, cx| shell.request_close(id, window, cx));
                }
            })
            .on_rename({
                let shell = shell.clone();
                move |id, window, cx: &mut App| {
                    shell.update(cx, |shell, cx| shell.start_rename(id, window, cx));
                }
            })
            .on_move({
                let shell = shell.clone();
                move |dragged, to, _window, cx: &mut App| {
                    shell.update(cx, |shell, cx| shell.tab_moved(dragged, to, cx));
                }
            })
            .on_menu_pick({
                let shell = shell.clone();
                move |id, pick, window, cx: &mut App| {
                    shell.update(cx, |shell, cx| shell.tab_menu_pick(id, pick, window, cx));
                }
            })
            .on_cancel_rename({
                let shell = shell.clone();
                move |_window, cx: &mut App| {
                    shell.update(cx, Shell::cancel_open_rename);
                }
            })
            .on_new_tab({
                let shell = shell.clone();
                move |_window, cx: &mut App| {
                    shell.update(cx, Shell::new_tab);
                }
            })
            // The `+` dropdown's pick (py-5): the plain row rides the
            // `+`'s own funnel, a profile row launches the preset.
            .on_plus_menu({
                let shell = shell.clone();
                move |profile, _window, cx: &mut App| match profile {
                    None => shell.update(cx, Shell::new_tab),
                    Some(index) => {
                        shell.update(cx, |shell, cx| shell.open_profile_tab(index, cx));
                    }
                }
            })
            .profile_names(
                self.config
                    .profiles
                    .iter()
                    .map(|profile| profile.name.clone())
                    .collect(),
            )
            .on_settings({
                let shell = shell.clone();
                move |window: &mut gpui::Window, cx: &mut App| {
                    Self::toggle_settings(&shell, window, cx);
                }
            })
            .when_some(
                self.rename
                    .as_ref()
                    .map(|edit| (edit.tab, edit.state.clone())),
                |bar, (tab, state)| bar.renaming(tab, state),
            );
        bar
    }

    /// The band, translated by the live slide: GPUI's animation
    /// element drives the frames (the chunk's ruling — GPUI's
    /// animation replaces the libcosmic draw-time translation), while
    /// the position is core's own slide model, one clock for pixels
    /// and wire alike — a mid-slide window swap (the keyboard-hold
    /// recreate) remounts the frame driver but the translation stays
    /// continuous off the machine's `started`.
    /// The drag edge's grab strip: the reference bar's `BAR_HEIGHT`
    /// (8 px) at the drawer's free edge — thin enough to read as a
    /// handle's footprint rather than chrome, tall enough to grab.
    /// The press arms the drag; the strip paints nothing itself (the
    /// band's border hairline already draws the edge).
    fn drag_edge_strip(cx: &mut Context<Self>) -> gpui::AnyElement {
        let shell = cx.entity();
        gpui::div()
            .w_full()
            .flex_none()
            .h(gpui::px(DRAG_EDGE_PX))
            .on_mouse_down(gpui::MouseButton::Left, move |event, _window, cx| {
                run_shell_actions(&shell, cx, |shell, actions| {
                    shell.drag_edge_press(event.position.y.as_f32(), actions);
                });
            })
            .into_any_element()
    }

    /// The pane's slot in the column (gpui-tabs Task 1's content
    /// half): the flex-grown pane, wrapped for the terminal context
    /// menu (gpui-chrome, py-5 the profile folders) — the pane's
    /// default-mode right press bubbles to this wrap and opens
    /// core's rows through the Kit's popover path, the same route
    /// the per-tab menu rides, over the live profile list (the
    /// folders' own ruling: zero profiles keep the flat menu).
    fn pane_area(
        &mut self,
        pane: Entity<TerminalPane>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let shell = cx.entity();
        let pick: crate::context_menu::PickFn =
            std::rc::Rc::new(move |pick, window: &mut Window, cx: &mut App| {
                shell.update(cx, |shell, cx| shell.context_menu_pick(pick, window, cx));
            });
        let menu_profiles = self.config.profiles.clone();
        div()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(pane)
            .context_menu(move |menu, window, cx| {
                crate::context_menu::build_menu(menu, &menu_profiles, &pick, window, cx)
            })
            .into_any_element()
    }

    fn sliding_band(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        // The drawer's content is the strip plus the active pane
        // (gpui-tabs Task 1): the strip at the config's side of the
        // surface, the pane yielding the strip's height — and the
        // same slide translation carries the whole column.
        let at_bottom = tab_bar::bar_at_bottom(self.inputs.geometry.edge, self.config.tab_position);
        let pane_background = resolve_theme(&self.config).colors
            [alacritty_terminal::vte::ansi::NamedColor::Background];
        let (strip, fg) = tab_bar::strip_pair(pane_background);
        let Some(pane) = self.active_pane() else {
            return div().size_full();
        };
        let bar = self.tab_strip(cx, at_bottom, strip, fg).into_any_element();
        let pane_area = self.pane_area(pane, cx);

        // The column through core's own stacking rule (gpui-status
        // Task 1): the status band always takes the slot adjacent to
        // the grab bar — the free-edge-most band, whatever the tab
        // strip's own pinning. This drawer has no grab bar element
        // (the layer surface's edges are its own), so the band takes
        // that slot directly; the master key drops the band whole.
        let mut bar = Some(bar);
        let mut pane_area = Some(pane_area);
        let status_band = (self.config.status_bar_enabled)
            .then(|| self.status_band(cx, pane_background).into_any_element());
        let mut status_band = status_band;
        let mut children: Vec<gpui::AnyElement> = Vec::new();
        for strip in status::column_stacking(self.inputs.geometry.edge, self.config.tab_position) {
            match strip {
                Strip::Tabs => children.extend(bar.take()),
                Strip::Terminal => children.extend(pane_area.take()),
                Strip::StatusBand => children.extend(status_band.take()),
                Strip::GrabBar => {}
            }
        }
        // The drag edge (the grab bar's port, sr-2's funnel fed real
        // input): a thin grab strip at the drawer's free edge, the
        // press arming the machine's drag; the live move and release
        // ride the column itself so the drag tracks the pointer
        // wherever it roams — the reference bar's own capture shape.
        let drag_edge = Self::drag_edge_strip(cx);
        if self.inputs.geometry.edge == Edge::Bottom {
            children.insert(0, drag_edge);
        } else {
            children.push(drag_edge);
        }
        let shell = cx.entity();
        let on_drag_move = {
            let shell = shell.clone();
            move |event: &gpui::MouseMoveEvent, _window: &mut Window, cx: &mut gpui::App| {
                // Gate before any update: a hover with no drag armed
                // must not so much as notify (a redraw per mouse move
                // would burn the frame budget).
                if shell.read(cx).machine.drag.is_some() {
                    run_shell_actions(&shell, cx, |shell, actions| {
                        shell.drag_edge_move(event.position.y.as_f32(), actions);
                    });
                }
            }
        };
        let on_drag_release = {
            let shell = shell.clone();
            move |_event: &gpui::MouseUpEvent, _window: &mut Window, cx: &mut gpui::App| {
                if shell.read(cx).machine.drag.is_some() {
                    run_shell_actions(&shell, cx, |shell, actions| {
                        shell.drag_edge_release(actions);
                    });
                }
            }
        };
        let column = div()
            .size_full()
            .flex()
            .flex_col()
            .children(children)
            .on_mouse_move(on_drag_move)
            .on_mouse_up(gpui::MouseButton::Left, on_drag_release);
        let Some(playing) = self.machine.slide else {
            return column;
        };
        let (active, started) = (playing.active, playing.started);
        let edge = self.inputs.geometry.edge;
        div().size_full().child(
            div()
                .absolute()
                .size_full()
                .with_animation(
                    "slide",
                    Animation::new(std::time::Duration::from_millis(SLIDE_DURATION_MS)),
                    move |band, _progress| {
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "elapsed ms of one slide; u64 range is exhausted after 58M years"
                        )]
                        let offset =
                            slide::offset(&active, started.elapsed().as_millis() as u64);
                        match edge {
                            // Sliding out toward the free edge: a
                            // top-anchored drawer drops down, a
                            // bottom-anchored one rises.
                            Edge::Top => band.top(px(offset)),
                            Edge::Bottom => band.top(px(-offset)),
                        }
                    },
                )
                .child(column),
        )
    }
}

impl gpui::Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frames += 1;
        let b = window.bounds();
        if self.last_bounds.as_ref() != Some(&b) {
            println!(
                "dropterm: frame {} mapped origin=({},{}) size={:.0}x{:.0} at +{}ms",
                self.frames,
                b.origin.x.as_f32(),
                b.origin.y.as_f32(),
                b.size.width.as_f32(),
                b.size.height.as_f32(),
                self.boot.elapsed().as_millis()
            );
            self.last_bounds = Some(b);
        }
        if self.frames == 1 {
            println!(
                "dropterm: OPEN->FIRST_FRAME {}ms (slide budget 130ms)",
                self.show_open.elapsed().as_millis()
            );
        }
        let active = window.is_window_active();
        if self.last_active != Some(active) {
            println!(
                "dropterm: activation {} at +{}ms (frame {}) — the keyboard re-grab \
                 evidence",
                if active { "GAINED" } else { "LOST" },
                self.boot.elapsed().as_millis(),
                self.frames
            );
            // The transition itself carries both halves of the
            // focus-loss wiring: GAINED arms the machine's focus latch
            // (the `LayerFocused` analog), a LOST that leaves a shown
            // surface queues the focus-loss decision for the tick —
            // never the seatless rest state, which reads LOST every
            // frame without ever having gained.
            if active {
                self.machine.focused = true;
            } else if self.last_active == Some(true) {
                self.focus_lost = true;
            }
            self.last_active = Some(active);
        }
        // The modal's focus repair: a resolved or vanished
        // confirmation leaves its handle dangling (an armed modal
        // dismissed without a keyboard-carrying path — its tab exited
        // underneath it), and the keyboard must return to the active
        // pane. The pane's own render-time claim defers to any holder
        // (the cooperative rule), so the repair runs here — the same
        // class of render-time focus work the pane itself does.
        if self.pending_close.is_none()
            && self.modal_focus.is_focused(window)
            && let Some(pane) = self.active_pane()
        {
            pane.read(cx).focus_handle().focus(window, cx);
        }
        // The windowless arm's completion (py-5): a remote close
        // armed the modal with no window on the stack to focus it on;
        // the first paint that carries it takes the keyboard, exactly
        // the front door's arm.
        if self.pending_close.is_some() && !self.modal_focus.is_focused(window) {
            self.modal_focus.focus(window, cx);
        }
        // The rename's own repair: a departed input's handle hands
        // the keyboard to the active pane the same way, and the slot
        // drops once anything else holds the keyboard.
        if let Some(departed) = self.rename_focus_repair.take()
            && departed.is_focused(window)
        {
            if let Some(pane) = self.active_pane() {
                pane.read(cx).focus_handle().focus(window, cx);
            }
            // Keep watching until the repair lands (anything else
            // holding the keyboard needs no repair).
            if departed.is_focused(window) {
                self.rename_focus_repair = Some(departed);
            }
        }
        let pending = self.pending_close;
        let mut band = self.sliding_band(cx).into_any_element();
        if self.settings.open {
            band = self.settings_overlay(band, cx);
        } else if self.images_open {
            band = self.images_overlay(band, cx);
        }
        let Some(pending) = pending else {
            return band;
        };
        self.close_confirm_modal(pending, band, cx)
            .into_any_element()
    }
}

impl Shell {
    /// Composes the pending close-confirmation over the band —
    /// armed-only (no pending close passes the band through
    /// untouched): a full-surface scrim whose press cancels, carrying
    /// a centered card naming the tab by its title only — never the
    /// process name — with the modal holding the keyboard (its key
    /// context swallows everything but Escape, the reference bar's
    /// own routing).
    fn close_confirm_modal(
        &self,
        pending: TabId,
        band: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let title = self
            .tabs
            .records()
            .iter()
            .find(|tab| tab.id == pending)
            .map(|tab| tab.title.clone())
            .unwrap_or_default();
        let shell = cx.entity();
        let cancel: ModalFn = std::rc::Rc::new({
            let shell = shell.clone();
            move |window: &mut Window, cx: &mut App| {
                shell.update(cx, |shell, _| shell.cancel_pending_close());
                let _ = window;
            }
        });
        let proceed: ModalFn = std::rc::Rc::new({
            let shell = shell.clone();
            move |_window: &mut Window, cx: &mut App| {
                shell.update(cx, |shell, cx| shell.close_now(pending, cx));
            }
        });
        let modal_focus = self.modal_focus.clone();
        let pane_background = resolve_theme(&self.config).colors
            [alacritty_terminal::vte::ansi::NamedColor::Background];
        let (card, fg) = tab_bar::strip_pair(pane_background);
        let hairline = gpui::Hsla {
            a: tab_bar::BORDER_ALPHA,
            ..fg
        };
        let button = |label: &'static str, on_press: ModalFn| {
            gpui::div()
                .px(gpui::px(12.))
                .py(gpui::px(5.))
                .rounded_sm()
                .text_color(gpui::Hsla { a: 0.8, ..fg })
                .child(label)
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    move |_, window: &mut Window, cx| on_press(window, cx),
                )
        };
        let card = gpui::div()
            .w(gpui::px(320.))
            .rounded_md()
            .border_1()
            .border_color(hairline)
            .bg(card)
            .text_color(fg)
            .text_size(gpui::px(14.))
            .p(gpui::px(16.))
            .flex()
            .flex_col()
            .gap(gpui::px(10.))
            .child(gpui::div().text_size(gpui::px(15.)).child("Close tab?"))
            .child(gpui::div().child(format!(
                "\u{201c}{title}\u{201d} is running a process. Close it anyway?"
            )))
            .child(
                gpui::div()
                    .flex()
                    .flex_row()
                    .gap(gpui::px(8.))
                    .justify_end()
                    .child(button("Cancel", std::rc::Rc::clone(&cancel)))
                    .child(button("Close Tab", std::rc::Rc::clone(&proceed))),
            );
        div().size_full().relative().child(band).child(
            div()
                .absolute()
                .inset_0()
                .bg(gpui::rgba(0x0000_0080))
                .flex()
                .items_center()
                .justify_center()
                // The modal's keyboard: it holds focus while
                // pending (the pane's claim defers), its context
                // binds Escape to the dismissal, and every other
                // key dies on the path — no binds fire, nothing
                // types.
                .key_context("close-confirm")
                .track_focus(&modal_focus)
                .on_action({
                    let shell = shell.clone();
                    move |_: &DismissCloseConfirm, _window: &mut Window, cx: &mut App| {
                        shell.update(cx, |shell, _| shell.cancel_pending_close());
                    }
                })
                // The scrim's press cancels (the card's buttons
                // capture their own presses first).
                .on_mouse_down(gpui::MouseButton::Left, {
                    let cancel = std::rc::Rc::clone(&cancel);
                    move |_, window: &mut Window, cx: &mut App| cancel(window, cx)
                })
                .child(card),
        )
    }
}

/// The measuring probe: the two-surface show's first pass. A
/// transparent full-output stretch whose granted bounds carry the
/// output's real size, reported through a slot the shell's tick
/// polls. Two timing facts shape the report: `open_window` draws once
/// before the compositor's configure arrives (the first render sees
/// the degenerate pre-configure bounds, so the probe waits for bounds
/// that grew past it), and the probe must never touch the shell from
/// inside its render — queued entity updates flush inside the
/// caller's borrow, so the report rides plain shared state instead.
impl Shell {
    /// The open drawer's overlay composition: a full-surface layer
    /// holding every pointer event (the libcosmic `context_drawer`'s
    /// capture rule — the band beneath never sees a press), the
    /// right-anchored panel at core's width math over the live drag
    /// (display-only until release), and the resize handle on the
    /// pane's leading edge — drag tracks the pointer's window-absolute
    /// x through `drawer_drag_px`, the release writes the width, a
    /// double-click resets to the built-in default and clears the
    /// stored value.
    fn settings_overlay(
        &mut self,
        band: gpui::AnyElement,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let shell = cx.entity();
        let drawer = self
            .settings_drawer
            .clone()
            .expect("an open drawer holds its entity");
        let surface = self
            .last_bounds
            .map_or(1280.0, |bounds| bounds.size.width.as_f32());
        let live = self.settings.drawer_drag;
        let width = dropterm::core::settings::drawer_width(
            live.or(self.config.settings_drawer_width),
            surface,
        );

        // The handle's press arms the drag (or resets on a
        // double-click); the overlay's moves track it, its release
        // commits.
        let on_handle_down = {
            let shell = shell.clone();
            move |event: &gpui::MouseDownEvent, _window: &mut Window, cx: &mut gpui::App| {
                shell.update(cx, |shell, cx| {
                    if event.button == gpui::MouseButton::Left && event.click_count >= 2 {
                        // The reset: the built-in default, the stored
                        // value cleared.
                        shell.write_settings(
                            &dropterm::core::settings::PlannedWrite::SettingsDrawerWidth(None),
                        );
                        println!(
                            "dropterm: drawer width reset at +{}ms",
                            shell.boot.elapsed().as_millis()
                        );
                        cx.notify();
                    } else if event.button == gpui::MouseButton::Left {
                        shell.settings.drawer_drag =
                            Some(dropterm::core::settings::drawer_drag_px(
                                event.position.x.as_f32(),
                                surface,
                            ));
                        cx.notify();
                    }
                });
            }
        };
        let on_move = {
            let shell = shell.clone();
            move |event: &gpui::MouseMoveEvent, _window: &mut Window, cx: &mut gpui::App| {
                shell.update(cx, |shell, cx| {
                    if shell.settings.drawer_drag.is_some() {
                        shell.settings.drawer_drag =
                            Some(dropterm::core::settings::drawer_drag_px(
                                event.position.x.as_f32(),
                                surface,
                            ));
                        cx.notify();
                    }
                });
            }
        };
        let on_release = {
            let shell = shell.clone();
            move |_event: &gpui::MouseUpEvent, _window: &mut Window, cx: &mut gpui::App| {
                shell.update(cx, |shell, cx| {
                    if let Some(px) = shell.settings.drawer_drag.take() {
                        shell.write_settings(
                            &dropterm::core::settings::PlannedWrite::SettingsDrawerWidth(Some(px)),
                        );
                        println!(
                            "dropterm: drawer width released at {px}px (+{}ms)",
                            shell.boot.elapsed().as_millis()
                        );
                        cx.notify();
                    }
                });
            }
        };

        let panel = gpui::div()
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(width))
            .flex()
            .flex_row()
            .bg(gpui::transparent_black())
            // The handle leads the panel: a thin grab strip on the
            // pane's leading edge.
            .child(
                gpui::div()
                    .w(px(10.))
                    .h_full()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .bg(cx.theme().border)
                    .on_mouse_down(gpui::MouseButton::Left, on_handle_down),
            )
            .child(gpui::div().flex_1().min_w_0().child(drawer));

        gpui::div()
            .absolute()
            .inset_0()
            .on_mouse_move(on_move)
            .on_mouse_up(gpui::MouseButton::Left, on_release)
            .child(band)
            .child(panel)
            .into_any_element()
    }

    /// The pasted-images panel's overlay mount (py-6): the live band
    /// beneath the module's own composition — the rows over the
    /// active tab's ledger with the armed cluster per row, the verb
    /// routed through the shell handler — at the drawer's width rule.
    fn images_overlay(
        &mut self,
        band: gpui::AnyElement,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let shell = cx.entity();
        let verb: crate::images_panel::VerbFn =
            std::rc::Rc::new(move |verb, window: &mut Window, cx: &mut App| {
                shell.update(cx, |shell, cx| shell.images_verb(verb, window, cx));
            });
        let armed = self.images_armed.clone();
        let rows: Vec<crate::images_panel::PanelRow<'_>> = self
            .tabs
            .active_id()
            .and_then(|id| self.tabs.pasted_images(id))
            .map(|ledger| {
                crate::images_panel::panel_rows(ledger)
                    .into_iter()
                    .map(|row| crate::images_panel::PanelRow {
                        cluster: crate::images_panel::delete_cluster(armed.as_deref(), &row.path),
                        row,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let surface = self
            .last_bounds
            .map_or(1280.0, |bounds| bounds.size.width.as_f32());
        crate::images_panel::overlay(band, rows, &self.images_thumbs, &verb, surface, cx)
    }
}

struct MeasureProbe {
    slot: Arc<Mutex<Option<(u32, u32)>>>,
    opened: Instant,
    fired: bool,
}

impl gpui::Render for MeasureProbe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let b = window.bounds().size;
        let granted = b.width.as_f32() > 1. && b.height.as_f32() > 1.;
        if !self.fired && granted {
            self.fired = true;
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "granted logical px of an output: round keeps the compositor's size; positive by the guard above"
            )]
            let output = (
                b.width.as_f32().round().max(1.) as u32,
                b.height.as_f32().round().max(1.) as u32,
            );
            println!(
                "dropterm: measure pass granted {}x{} at +{}ms",
                output.0,
                output.1,
                self.opened.elapsed().as_millis()
            );
            if let Ok(mut slot) = self.slot.lock() {
                *slot = Some(output);
            }
        } else if !self.fired {
            // Keep frames coming until the configure lands (the
            // pre-configure draw alone never re-renders on its own).
            cx.notify();
        }
        div()
    }
}

/// `DROPTERM_DRAG_WALK`'s parse: a strictly-parsed comma-separated
/// list of absolute window-y pointer positions the walk feeds through
/// the machine's live-drag funnel once the surface is shown — one
/// position per tick, the machine's own cadence gate deciding which
/// apply (the drag-class evidence rows; a real pointer drives the same
/// funnel through the grab bar once the chrome slices land it).
pub(crate) fn drag_walk_positions(raw: Option<&str>) -> Option<Vec<f32>> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let positions = raw.split(',').map(str::trim).collect::<Vec<_>>();
    let parsed = positions
        .iter()
        .map(|p| p.parse::<f32>())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some(parsed)
}

/// `DROPTERM_TABS`' parse (gpui-tabs Task 1's walk arm): how many
/// tabs the drawer boots with. Unset, empty, non-numeric, or
/// non-positive means the real app's single tab; a runaway count
/// clamps at sixteen (enough rows for a render walk, never a
/// PTY-storm).
pub(crate) fn tab_walk_count(raw: Option<&str>) -> usize {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 16)
}

/// The booted tab fleet ([`boot_tab_fleet`]'s product): the list, the
/// panes by id, and the shared hide channel's halves.
struct TabFleet {
    tabs: tab_bar::TabList,
    panes: HashMap<TabId, Entity<TerminalPane>>,
    signals_tx: std::sync::mpsc::Sender<view::PaneSignal>,
    pane_signals: Receiver<view::PaneSignal>,
}

/// The restore arm of [`boot_tab_fleet`] (gpui-parity Task 3; py-5
/// added the profile re-resolution): one pane per saved tab in plan
/// order, each in its re-validated cwd with its chrome replayed (the
/// resolved title, the rename pin, the pin) — a saved profile id
/// that still resolves spawns the current definition's preset (the
/// command-then-shell wrap, env, and per-tab appearance through the
/// one request resolution), a plain spec its recorded shell — the
/// saved active seat activated when any tab survived. A failed PTY
/// closes its record (the rollback); the count returns for the
/// fallback decision.
fn restore_fleet(
    cx: &mut App,
    config: &DropTermConfig,
    specs_table: Option<&SpecsTable>,
    plan: Option<&dropterm::core::session::RestorePlan>,
    signals_tx: &std::sync::mpsc::Sender<view::PaneSignal>,
    tabs: &mut tab_bar::TabList,
    panes: &mut HashMap<TabId, Entity<TerminalPane>>,
) -> usize {
    let Some(plan) = plan else {
        return 0;
    };
    let mut restored = 0usize;
    for spec in &plan.tabs {
        let id = tabs.spawn_restored(
            spec.title.clone(),
            spec.title_override.clone(),
            spec.pinned,
            spec.cwd.clone(),
            spec.profile.as_ref().map(|profile| profile.name.clone()),
        );
        // The libcosmic `spawn_tab(Some(spec))` shape: the plan's
        // re-resolved profile runs the current definition with
        // `spec.cwd` (already the ruled chain result), a plain spec
        // its recorded shell — the degradations core's own.
        let command = if let Some(profile) = &spec.profile {
            dropterm::core::spawn::SpawnCommand::Profile {
                name: profile.name.clone(),
                command: profile.command.clone(),
                cwd: spec.cwd.clone(),
                env: profile.env.clone(),
            }
        } else {
            dropterm::core::spawn::SpawnCommand::Shell(spec.shell.clone())
        };
        let request = view::PaneRequest::for_command(&command, spec.cwd.clone(), config);
        let Some((_terminal, pane)) = view::boot_pane(
            cx,
            config,
            request,
            specs_table,
            id,
            Some(signals_tx.clone()),
        ) else {
            eprintln!("dropterm: restored tab's terminal failed to spawn; closing its record");
            tabs.close(id);
            continue;
        };
        panes.insert(id, pane);
        restored += 1;
    }
    if let dropterm::core::session::RestoreOutcome::Restored { active } =
        dropterm::core::session::restore_outcome(plan.active, restored)
    {
        tabs.activate_index(active);
        println!(
            "dropterm: session restored — {restored} tab(s) live, active {active} ({} saved)",
            plan.tabs.len()
        );
    }
    restored
}

fn boot_tab_fleet(
    cx: &mut App,
    config: &DropTermConfig,
    specs_table: Option<&SpecsTable>,
    restore: &mut dropterm::core::session::RestoreStash,
    edge: Edge,
) -> Option<TabFleet> {
    // The session restore (gpui-parity Task 3): the plan wins unless
    // a walk owns the fleet through `DROPTERM_TABS` — one pane per
    // saved tab, in order, each in its re-validated cwd, the saved
    // chrome replayed onto the record (the resolved title, the rename
    // pin, the pin) and the saved active seat activated. A saved
    // shell the system no longer carries degrades onto the configured
    // resolution (the libcosmic row); a saved profile spawns the
    // plain shell — the GPUI build has no profile spawn path (the
    // gpui-tabs scope note), the cwd still honored.
    let plan = if std::env::var_os("DROPTERM_TABS").is_none() {
        restore.take_for_run(true)
    } else {
        None
    };
    let tab_count = tab_walk_count(std::env::var("DROPTERM_TABS").ok().as_deref());
    // The walk's seeding arm keeps its exact pty shape (`sh -c
    // <line>`); the real app's fresh tabs ride the configured default
    // through the one request resolution (a `default_profile` preset
    // included, py-5).
    let walk_request = std::env::var("DROPTERM_TERM_CMD").ok().map(|raw| {
        let raw = raw.trim().to_string();
        let mut request = view::PaneRequest::for_command(
            &dropterm::core::spawn::SpawnCommand::Shell("/bin/sh".to_string()),
            None,
            config,
        );
        if !raw.is_empty() {
            request.args = vec!["-c".to_string(), raw];
        }
        request
    });
    let (signals_tx, pane_signals) = std::sync::mpsc::channel::<view::PaneSignal>();
    let mut tabs = tab_bar::TabList::default();
    let mut panes = HashMap::new();
    let restored = restore_fleet(
        cx,
        config,
        specs_table,
        plan.as_ref(),
        &signals_tx,
        &mut tabs,
        &mut panes,
    );
    if restored == 0 {
        for _ in 0..tab_count {
            let id = tabs.spawn(false);
            let command = spawn::default_spawn_command(config);
            let request = walk_request
                .clone()
                .unwrap_or_else(|| view::PaneRequest::for_command(&command, None, config));
            let profile_id = request.profile_id.clone();
            let ephemeral = matches!(command, spawn::SpawnCommand::CommandLine(_));
            let Some((_terminal, pane)) = view::boot_pane(
                cx,
                config,
                request,
                specs_table,
                id,
                Some(signals_tx.clone()),
            ) else {
                eprintln!("dropterm: the drawer's terminal failed to spawn; exiting");
                return None;
            };
            tabs.note_spawn_facts(id, profile_id, ephemeral);
            panes.insert(id, pane);
        }
    }
    println!(
        "dropterm: tab bar: {} tab(s) ([{}]), strip at {} (tab_position {}, edge {:?}), \
         border {}",
        tabs.records().len(),
        tabs.records()
            .iter()
            .map(|tab| tab.title.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        if tab_bar::bar_at_bottom(edge, config.tab_position) {
            "bottom"
        } else {
            "top"
        },
        config.tab_position,
        edge,
        if config.tab_bar_border { "on" } else { "off" },
    );
    Some(TabFleet {
        tabs,
        panes,
        signals_tx,
        pane_signals,
    })
}

/// Boot the shell: load the config, create the shell entity, and run
/// the tick that drains verbs and keeps frames flowing. The surface
/// itself starts hidden — the machine's resting state — exactly like
/// the libcosmic app; the first show verb runs the two-surface show.
// One line over the lint's bar (the settings focus handle joined
// the constructor's inputs — the modal-focus pattern); the pass to
// re-bucket boot's inputs is its own refactor, not this fix's.
#[expect(
    clippy::too_many_lines,
    reason = "one param over; re-bucketing is its own refactor"
)]
pub(crate) fn boot(
    cx: &mut App,
    exit_after: Option<Duration>,
    phase: &Arc<Mutex<SurfacePhase>>,
    tab_count: &Arc<Mutex<u32>>,
    verbs: Receiver<cli::Action>,
    startup: cli::Action,
) {
    let boot = Instant::now();
    let config_dir = dropterm::core::config::store::store_dir();
    let config = DropTermConfig::load_from(&config_dir);
    let inputs = SurfaceInputs::from_config(&config);
    println!(
        "dropterm: surface shell booted hidden (edge {:?}, {}% height, {}% width, align \
         {:?}, layer {:?}, kb {:?})",
        inputs.geometry.edge,
        inputs.geometry.height_percent,
        inputs.geometry.width_percent,
        inputs.geometry.h_align,
        inputs.layer,
        inputs.keyboard_mode,
    );
    let drag_walk = drag_walk_positions(std::env::var("DROPTERM_DRAG_WALK").ok().as_deref());
    if drag_walk.is_some() {
        println!("dropterm: drag walk armed (positions fed once shown and at rest)");
    }
    // gpui-theme Task 2: the Kit initialized and the resolved scheme
    // applied before any window exists — every Kit surface and token
    // reads the DropTerm scheme from its first frame (C4's route).
    gpui_component::init(cx);
    // The close-confirm modal's Escape (gpui-tabs Task 2): bound in
    // the modal's own key context, so the key reaches the dismissal
    // while the modal holds the keyboard and dies there otherwise.
    cx.bind_keys([
        gpui::KeyBinding::new("escape", DismissCloseConfirm, Some("close-confirm")),
        gpui::KeyBinding::new("escape", DismissSettings, Some("settings-drawer")),
        gpui::KeyBinding::new("escape", tab_bar::CancelTabRename, Some("tab-rename")),
    ]);
    let boot_theme = resolve_theme(&config);
    let boot_theme_identity = theme_identity(&boot_theme);
    apply_resolved_theme(cx, &boot_theme);
    // The drawer's tab fleet (gpui-tabs Task 1). The completions
    // load starts before it (gpui-parity Task 1): the first panes
    // carry the loading placeholder, and the tick's drain pushes the
    // landed table the moment the thread finishes.
    let specs = SpecsWatch::spawn_initial(&config);
    // The session's one small synchronous read (gpui-parity Task 3):
    // stashed here — before the fleet spawns — so restore structurally
    // precedes the first tab, exactly as the libcosmic init. A missing
    // or invalid file stashes nothing (already quarantined with a
    // warning inside `load`), keeping first-run startup silent; the
    // toggle gates the whole entry — disabled, the file is not even
    // read, and what is on disk waits untouched for a future launch.
    let mut restore = {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        dropterm::core::session::restore_entry(
            &dropterm::core::session::state_file_path(),
            home.as_deref(),
            config.session_persistence_enabled,
            &config.profiles,
        )
    };
    let Some(fleet) = boot_tab_fleet(
        cx,
        &config,
        specs.table.as_ref(),
        &mut restore,
        inputs.geometry.edge,
    ) else {
        cx.quit();
        return;
    };
    let (config_tx, config_changes) = std::sync::mpsc::channel::<()>();
    let watch_dir = config_dir.clone();
    std::thread::spawn(move || {
        let mut watcher = match notify::recommended_watcher(move |event| {
            let _ = event; // any event in the config dir: reload and compare
            let _ = config_tx.send(());
        }) {
            Ok(watcher) => watcher,
            Err(err) => return eprintln!("dropterm: config watcher failed: {err:?}"),
        };
        if let Err(err) = watcher.watch(&watch_dir, notify::RecursiveMode::NonRecursive) {
            return eprintln!("dropterm: config watch failed: {err:?}");
        }
        println!("dropterm: config watcher armed on {}", watch_dir.display());
        std::thread::park();
    });
    // The band's font (gpui-status Task 1): the terminal's own
    // resolved family — the same candidate resolution every pane's
    // host performs, resolved once here (the band is the terminal
    // speaking about itself, in its own voice).
    let band_font = gpui::Font {
        family: crate::terminal::host::resolve_monospace_family(cx.text_system()).into(),
        ..gpui::Font::default()
    };
    let shell = cx.new(|cx| {
        Shell::new(
            boot,
            inputs,
            Arc::clone(phase),
            Arc::clone(tab_count),
            verbs,
            startup,
            fleet.tabs,
            fleet.panes,
            config,
            fleet.signals_tx,
            fleet.pane_signals,
            boot_theme_identity,
            drag_walk,
            config_changes,
            cx.focus_handle(),
            exit_after,
            band_font,
            cx.focus_handle(),
            specs,
        )
    });
    // The dock presence (item 9's replicate ruling): the boot-time
    // sync opens the body for a dock-mode config without waiting for
    // the tick's first pass.
    shell.update(cx, Shell::sync_dock_anchor);
    // The tick: 100 ms of verb drain + frame pump, holding the shell
    // entity strongly for the app's lifetime (the surface's windows
    // come and go; the shell owns the machine and must not).
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            cx.update(|cx| {
                let pumped = shell.update(cx, Shell::pump);
                // gpui-theme Task 2: a scheme edit re-themes before
                // the next frame — the App context lives here, not in
                // pump (the same split every window action takes).
                shell.update(cx, Shell::apply_pending_theme);
                // gpui-parity Task 1: a landed completions load
                // stores the table and pushes it to the panes.
                shell.update(cx, Shell::drain_completions);
                // gpui-parity Task 3: the debounced session save.
                shell.update(cx, Shell::pump_session_save);
                // The dock presence rides the same tick (item 9's
                // replicate ruling): a config echo flips the entry.
                shell.update(cx, Shell::sync_dock_anchor);
                // gpui-tabs Task 1: the tab titles sync on the same
                // apply point — each pane's report and cwd polled,
                // re-resolved through core only on a move.
                shell.update(cx, Shell::sync_tab_titles);
                // gpui-tabs Task 2: each pane's latched attention
                // signals land on the same apply point.
                shell.update(cx, Shell::poll_tab_attention);
                // gpui-status Task 1: the band's snapshot (clock
                // and segments) lands on the same apply point — the
                // render reads the stored state only.
                shell.update(cx, Shell::refresh_status_band);
                // gn-2: a show transition spends the missed-activity
                // news (the latches clear; the halo goes dark).
                shell.update(cx, Shell::clear_activity_on_show);
                // A shell exit closes its tab ungated — pane teardown
                // and the never-empty respawn apply here, where the
                // App context lives.
                for id in pumped.exited {
                    shell.update(cx, |shell, cx| shell.close_now(id, cx));
                }
                // The forwarded tab verbs (py-5) apply at the same
                // point — pane spawning needs the App context, and the
                // verbs' own show halves ride the window actions the
                // same funnel produces.
                for verb in pumped.tab_verbs {
                    let mut actions = Vec::new();
                    shell.update(cx, |shell, cx| shell.apply_tab_verb(verb, &mut actions, cx));
                    for action in actions {
                        apply_action(cx, &shell, action);
                    }
                }
                for action in pumped.actions {
                    apply_action(cx, &shell, action);
                }
                if pumped.quit {
                    cx.quit();
                } else {
                    // Keep frames flowing (the clock band) — the
                    // scaffold's coexistence seam.
                    shell.update(cx, |_, cx| cx.notify());
                }
            });
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dock_anchor_swap_rule_is_exact() {
        // The replicate ruling's flip rule: on-without opens,
        // off-with closes, agreement keeps — both agreements, so an
        // echo never churns the body.
        assert_eq!(anchor_action(true, false), AnchorAction::Open);
        assert_eq!(anchor_action(false, true), AnchorAction::Close);
        assert_eq!(anchor_action(true, true), AnchorAction::Keep);
        assert_eq!(anchor_action(false, false), AnchorAction::Keep);
    }

    #[test]
    fn the_session_shape_detects_every_structural_change() {
        // The change detector's coverage (gpui-parity Task 3): every
        // structural truth the save triggers on moves the shape — a
        // reorder, a pin, a rename pin, an active switch, a cwd move
        // — while shell-title churn stays quiet. Ids are deterministic
        // from a default list, so a fresh two-tab pair is the shared
        // baseline each mutation compares against.
        use dropterm::core::config::TabTitleMode;
        use dropterm::core::tabs::MoveDirection;

        fn two_tabs() -> tab_bar::TabList {
            let mut tabs = tab_bar::TabList::default();
            let first = tabs.spawn(false);
            let _ = tabs.spawn(false);
            // The spawns leave the second active; the baseline holds
            // the first so the switch arm has a direction.
            tabs.select(first);
            tabs
        }
        let baseline = session_shape(&two_tabs());

        let mut moved = two_tabs();
        let second = moved.records()[1].id;
        moved.move_by(second, MoveDirection::Left);
        assert_ne!(session_shape(&moved), baseline, "a reorder marks");

        let mut pinned = two_tabs();
        let first = pinned.records()[0].id;
        pinned.toggle_pin(first);
        assert_ne!(session_shape(&pinned), baseline, "a pin marks");

        let mut renamed = two_tabs();
        renamed.commit_rename(first, "work", TabTitleMode::Shell);
        assert_ne!(session_shape(&renamed), baseline, "a rename pin marks");

        let mut switched = two_tabs();
        switched.select(second);
        assert_ne!(session_shape(&switched), baseline, "a switch marks");

        let mut retitled = two_tabs();
        retitled.sync_title(first, Some("churn".to_string()), None, TabTitleMode::Shell);
        assert_eq!(
            session_shape(&retitled),
            baseline,
            "a shell title without a cwd move stays quiet (the Guake cadence)"
        );

        let mut cd = two_tabs();
        cd.sync_title(
            first,
            Some("elsewhere".to_string()),
            Some(std::path::PathBuf::from("/elsewhere")),
            TabTitleMode::Shell,
        );
        assert_ne!(session_shape(&cd), baseline, "a cwd move marks");
    }

    #[test]
    fn exit_after_parses_positive_milliseconds_only() {
        use std::time::Duration;
        assert_eq!(
            exit_after_from_env(Some("4000")),
            Some(Duration::from_millis(4000))
        );
        assert_eq!(
            exit_after_from_env(Some("  4000\t")),
            Some(Duration::from_secs(4))
        );
        // Unset, empty, non-numeric, and non-positive all mean "run
        // like a real app" — zero especially, so a stray `=0` can't
        // quit at boot.
        assert_eq!(exit_after_from_env(None), None);
        assert_eq!(exit_after_from_env(Some("")), None);
        assert_eq!(exit_after_from_env(Some("soon")), None);
        assert_eq!(exit_after_from_env(Some("0")), None);
        assert_eq!(exit_after_from_env(Some("-5")), None);
    }

    #[test]
    fn the_machine_states_map_onto_the_served_phases() {
        use dropterm::core::intent::SurfaceHandle;
        assert_eq!(phase_of(&SurfaceState::Hidden), SurfacePhase::Hidden);
        assert_eq!(
            phase_of(&SurfaceState::Measuring(SurfaceHandle::next())),
            SurfacePhase::Measuring
        );
        assert_eq!(
            phase_of(&SurfaceState::Shown(SurfaceHandle::next())),
            SurfacePhase::Shown
        );
        assert_eq!(
            phase_of(&SurfaceState::Hiding(SurfaceHandle::next())),
            SurfacePhase::Hiding
        );
    }

    #[test]
    fn drag_walk_positions_parse_strictly() {
        // The walk's affordance: a comma-separated list of absolute
        // window-y pointer positions. Every entry must parse — a
        // malformed list boots no drag walk at all (visibly wrong in
        // a walk log, never silently half-armed).
        assert_eq!(drag_walk_positions(None), None);
        assert_eq!(drag_walk_positions(Some("")), None);
        assert_eq!(drag_walk_positions(Some("   ")), None);
        assert_eq!(drag_walk_positions(Some("216")), Some(vec![216.]));
        assert_eq!(
            drag_walk_positions(Some("216, 240,270 , 305")),
            Some(vec![216., 240., 270., 305.])
        );
        assert_eq!(drag_walk_positions(Some("216,soon,270")), None);
    }

    #[test]
    fn tab_walk_count_defaults_to_one_and_clamps() {
        // The real app's drawer boots one tab; the walk arm adds rows
        // only when it parses, and a runaway count clamps.
        assert_eq!(tab_walk_count(None), 1);
        assert_eq!(tab_walk_count(Some("")), 1);
        assert_eq!(tab_walk_count(Some("soon")), 1);
        assert_eq!(tab_walk_count(Some("0")), 1);
        assert_eq!(tab_walk_count(Some("-3")), 1);
        assert_eq!(tab_walk_count(Some(" 4 ")), 4);
        assert_eq!(tab_walk_count(Some("99")), 16);
    }

    #[test]
    fn theme_identity_tracks_the_scheme_and_mode_not_geometry() {
        use dropterm::core::config::ThemePreference;

        let base = DropTermConfig::default();
        let base_id = theme_identity(&resolve_theme(&base));
        assert_eq!(theme_identity(&resolve_theme(&base)), base_id);

        // A geometry-only edit never re-themes.
        let geometry_only = DropTermConfig {
            height_percent: 70,
            ..DropTermConfig::default()
        };
        assert_eq!(theme_identity(&resolve_theme(&geometry_only)), base_id);

        // A scheme-name edit does.
        let scheme = DropTermConfig {
            color_scheme_dark: "Dracula".to_string(),
            ..DropTermConfig::default()
        };
        assert_ne!(theme_identity(&resolve_theme(&scheme)), base_id);

        // A preference flip changes the mode (System resolves dark
        // until the live system-mode read lands; Light resolves
        // light).
        let pref = DropTermConfig {
            theme_preference: ThemePreference::Light,
            ..DropTermConfig::default()
        };
        assert_ne!(theme_identity(&resolve_theme(&pref)), base_id);
    }

    #[test]
    fn resolved_theme_colors_agree_with_the_panes_boot_colors() {
        use alacritty_terminal::vte::ansi::NamedColor;

        // One resolution chain: the theme the Kit applies and the
        // colors the pane paints must come from the same glue
        // decisions, or the chrome and the terminal drift apart on a
        // live edit.
        let config = DropTermConfig::default();
        let resolved = resolve_theme(&config).colors;
        let boot = view::boot_colors(&config);
        for nc in [
            NamedColor::Background,
            NamedColor::Foreground,
            NamedColor::BrightBlack,
            NamedColor::Blue,
            NamedColor::BrightBlue,
            NamedColor::Red,
            NamedColor::Green,
            NamedColor::Yellow,
            NamedColor::Cyan,
        ] {
            assert_eq!(resolved[nc], boot[nc], "{nc:?} must agree — one chain");
        }
    }
}
