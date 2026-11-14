// SPDX-License-Identifier: GPL-3.0-only

//! The app shell (gpui-scaffold Task 1, grown by gpui-surface): the
//! surface lifecycle's app-side glue — the decision layer
//! ([`dropterm_core::surface_lifecycle::SurfaceMachine`]) runs
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

use dropterm_core::config::DropTermConfig;
use dropterm_core::config::Edge;
use dropterm_core::config::TabTitleMode;
use dropterm_core::dbus_state::SurfacePhase;
use dropterm_core::intent::SurfaceExecutor;
use dropterm_core::slide::{self, SLIDE_DURATION_MS};
use dropterm_core::surface_lifecycle::{SurfaceInputs, SurfaceMachine, SurfaceState};
use dropterm_core::tabs::TabId;
use gpui_component::ActiveTheme as _;
use notify::Watcher as _;

use crate::dbus::Verb;
use crate::executor::{Action, GpuiExecutor};
use crate::tab_bar;
use crate::terminal::view::{self, TerminalPane};

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
/// [`dropterm_core::surface_lifecycle::grab_bar_acts_on`] guard, read
/// through (shown and at rest — no drag arms while a slide animates).
fn grab_bar_at_rest(machine: &SurfaceMachine) -> bool {
    dropterm_core::surface_lifecycle::grab_bar_acts_on(&machine.state, machine.slide.is_some())
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
    let dark = dropterm_core::theme_glue::effective_is_dark(config.theme_preference, true);
    ResolvedTheme {
        name: dropterm_core::profile::effective_scheme_name(
            None,
            &config.color_scheme_dark,
            &config.color_scheme_light,
            dark,
        )
        .to_string(),
        dark,
        colors: dropterm_core::theme_glue::effective_colors(config, None, dark),
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
        "dropterm-gpui: theme applied ({}, {}) — tokens: background {:?} primary {:?} \
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
    /// The forwarded verbs, drained on the tick.
    verbs: Receiver<Verb>,
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
    config: DropTermConfig,
    /// The hide channel's sender half — every spawned pane shares it.
    hide_tx: std::sync::mpsc::Sender<()>,
    /// The title mode the tab titles were last resolved under (the
    /// live mode-flip detector).
    applied_title_mode: TabTitleMode,
    /// The pane's Hide action, handed up through the channel and run
    /// through the machine's hide funnel on the tick — window work
    /// never runs inside a pane's event handler (sr-1's lesson).
    hide_requests: Receiver<()>,
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
    exit_at: Option<Instant>,
}

impl Shell {
    #[allow(clippy::too_many_arguments)]
    fn new(
        boot: Instant,
        inputs: SurfaceInputs,
        phase: Arc<Mutex<SurfacePhase>>,
        verbs: Receiver<Verb>,
        tabs: tab_bar::TabList,
        panes: HashMap<TabId, Entity<TerminalPane>>,
        config: DropTermConfig,
        hide_tx: std::sync::mpsc::Sender<()>,
        hide_requests: Receiver<()>,
        theme: (String, bool),
        drag_walk: Option<Vec<f32>>,
        config_changes: Receiver<()>,
        exit_after: Option<Duration>,
    ) -> Self {
        let applied_title_mode = config.tab_title_mode;
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
            hide_tx,
            applied_title_mode,
            hide_requests,
            theme,
            pending_theme: None,
            drag_walk: drag_walk.unwrap_or_default().into(),
            drag_release_at: None,
            config_changes,
            config_quiet_until: None,
            last_active: None,
            focus_lost: false,
            phase,
            verbs,
            measured: Arc::new(Mutex::new(None)),
            exit_at: exit_after.map(|after| boot + after),
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
    fn pump(&mut self) -> Pumped {
        let mut actions = Vec::new();
        while let Ok(verb) = self.verbs.try_recv() {
            self.verb(verb, &mut actions);
        }
        // The pane's Hide action: one drain (repeat sends coalesce —
        // a second hide on a hidden surface decides nothing), run
        // through the machine's hide funnel exactly like the verb's.
        while self.hide_requests.try_recv().is_ok() {
            println!(
                "dropterm-gpui: pane hide action -> the hide funnel at +{}ms",
                self.boot.elapsed().as_millis()
            );
            let intents = self.machine.decide_hide(self.inputs);
            self.execute_into(intents, &mut actions);
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
                && dropterm_core::surface_lifecycle::layer_unfocused_acts_on(
                    &self.machine.state,
                    handle,
                )
            {
                let (report_unfocus, hide) = self.machine.decide_focus_loss(self.inputs);
                println!(
                    "dropterm-gpui: activation lost while shown -> focus-loss decision \
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
                    "dropterm-gpui: exit timer fired at +{}ms",
                    self.boot.elapsed().as_millis()
                );
                true
            }
            _ => false,
        };
        Pumped { actions, quit }
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
                    "dropterm-gpui: drag walk trailing apply landed target={applied}px at +{}ms",
                    self.boot.elapsed().as_millis()
                );
            }
            self.execute_into(trailing.intents, actions);
        }
        let at_rest = grab_bar_at_rest(&self.machine);
        if at_rest && let Some(pointer_y) = self.drag_walk.pop_front() {
            println!(
                "dropterm-gpui: drag walk move pointer_y={pointer_y:.0} at +{}ms",
                self.boot.elapsed().as_millis()
            );
            let outcome = self
                .machine
                .drag_move(pointer_y, Instant::now(), self.inputs);
            if let Some(target) = outcome.applied {
                println!(
                    "dropterm-gpui: drag walk cadence applied target={target}px at +{}ms",
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
                "dropterm-gpui: drag walk release settled at {}% at +{}ms",
                settled.height_percent,
                self.boot.elapsed().as_millis()
            );
            if let Some(percent) = write {
                // The persist itself is the config-write slice's;
                // the walk logs the decision the libcosmic app would
                // write.
                println!(
                    "dropterm-gpui: drag settle would persist height_percent={percent} \
                     (persistence arrives with the config-write slice)"
                );
            }
            self.inputs.geometry = settled;
            self.execute_into(intents, actions);
        }
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
        let config = DropTermConfig::load_from(&dropterm_core::config::store::store_dir());
        // gpui-tabs Task 1: the strip's placement, border, and title
        // mode read the live config at render/sync time, so the echo
        // lands the snapshot the next frame reads.
        self.config = config.clone();
        // gpui-theme Task 2: a scheme edit re-themes — resolved here,
        // applied at the tick's apply point before the next frame.
        // The identity compare absorbs echoes (an unchanged scheme is
        // not a re-theme) and keeps geometry-only edits off the theme
        // path.
        let resolved = resolve_theme(&config);
        let identity = theme_identity(&resolved);
        if identity != self.theme {
            println!(
                "dropterm-gpui: config changed -> theme: {} ({})",
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
            "dropterm-gpui: config changed -> reshape decision: {} intent(s) \
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

    /// One forwarded surface verb through the toggle funnel: the
    /// debounce first, then the state-matched decision — show from
    /// hidden (the two-surface show), reversal from mid-slide, hide
    /// from measuring or shown (the slide-out wire, or destroy-now
    /// for a measuring surface).
    fn verb(&mut self, verb: Verb, actions: &mut Vec<Action>) {
        if !self.machine.toggle_accepted(Instant::now()) {
            println!(
                "dropterm-gpui: verb {verb:?} dropped by the toggle debounce at +{}ms",
                self.boot.elapsed().as_millis()
            );
            return;
        }
        println!(
            "dropterm-gpui: verb {verb:?} accepted at +{}ms",
            self.boot.elapsed().as_millis()
        );
        match (verb, &self.machine.state) {
            (Verb::Show | Verb::Toggle, SurfaceState::Hidden) => {
                let intents = self.machine.decide_show(self.inputs);
                self.execute_into(intents, actions);
            }
            // A mid-slide reversal: the live surface is reused, the
            // slide re-targets `In`, and the keyboard re-holds at the
            // resolved mode — task 3's wire, served.
            (Verb::Show | Verb::Toggle, SurfaceState::Hiding(handle)) => {
                let handle = *handle;
                let intents = self.machine.decide_reverse_show(handle, self.inputs);
                self.execute_into(intents, actions);
            }
            // The hide: a measuring surface has no reshaped content
            // to animate (destroy-now); a shown one slides out —
            // keyboard released at slide-out start, destroyed at the
            // slide clock's completion tick.
            (Verb::Hide | Verb::Toggle, SurfaceState::Measuring(_) | SurfaceState::Shown(_)) => {
                let intents = self.machine.decide_hide(self.inputs);
                self.execute_into(intents, actions);
            }
            // Show while already on its way, hide while hidden: the
            // decision layer's own guards leave these with nothing.
            _ => {}
        }
        self.publish_phase();
    }

    /// The measuring window's granted bounds: the output's real size,
    /// read off the surface the compositor just configured — never
    /// `cx.displays()`, so fractional outputs mis-size nothing. Runs
    /// the first-configure decision and lands the shaped window.
    fn measured(&mut self, output: (u32, u32), actions: &mut Vec<Action>) {
        println!(
            "dropterm-gpui: measuring window granted {}x{} at +{}ms — pass 1 complete",
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
        intents: Vec<dropterm_core::intent::SurfaceIntent>,
        actions: &mut Vec<Action>,
    ) {
        for intent in intents {
            println!(
                "dropterm-gpui: surface intent {intent:?} at +{}ms",
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
    quit: bool,
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
                        "dropterm-gpui: two-surface show pass 1: measuring window opened at \
                         +{}ms",
                        shell.read(cx).boot.elapsed().as_millis()
                    );
                }
                Err(err) => {
                    eprintln!("dropterm-gpui: measuring window open failed: {err:?}");
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
            match cx.open_window(options, |_, _| shell.clone()) {
                Ok(window) => {
                    shell.update(cx, |shell, _| shell.executor.adopt(handle, window.into()));
                    println!(
                        "dropterm-gpui: window swap ({reason}) opened at +{}ms",
                        shell.read(cx).boot.elapsed().as_millis()
                    );
                }
                Err(err) => {
                    eprintln!("dropterm-gpui: swapped window open failed: {err:?}");
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
                "dropterm-gpui: resized to {}x{height} at +{}ms",
                width.map_or_else(|| "fill".to_string(), |w| w.to_string()),
                shell.read(cx).boot.elapsed().as_millis()
            );
        }
        Action::Remove { handle } => {
            let window = shell.read(cx).executor.window_of(handle);
            let _ = window.update(cx, |_, window, _| window.remove_window());
            shell.update(cx, |shell, _| shell.executor.retire(handle));
            println!(
                "dropterm-gpui: surface destroyed at +{}ms",
                shell.read(cx).boot.elapsed().as_millis()
            );
        }
        Action::FocusTerminal { handle } => {
            // The pane holds the keyboard by construction (its focus
            // handle grabs on first render inside the surface), so the
            // intent applies as the pane already standing; the row is
            // the focus decision landing.
            println!(
                "dropterm-gpui: terminal focus for {handle:?} (the pane holds the \
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
        if self.tabs.select(id) {
            let title = self
                .tabs
                .records()
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| tab.title.clone());
            println!(
                "dropterm-gpui: tab select -> {id:?} ({}) at +{}ms",
                title.as_deref().unwrap_or("?"),
                self.boot.elapsed().as_millis()
            );
            cx.notify();
        }
    }

    /// The `+` affordance's press: spawn the pane, record it through
    /// core's insert, and activate — the drawer's never-empty rule
    /// keeps at least this tab alive whatever closes later.
    fn new_tab(&mut self, cx: &mut Context<Self>) {
        let id = self.tabs.spawn(self.config.new_tab_after_current);
        let Some((_terminal, pane)) =
            view::boot_pane(cx, &self.config, None, id, Some(self.hide_tx.clone()))
        else {
            eprintln!("dropterm-gpui: new tab's terminal failed to spawn; closing its record");
            self.tabs.close(id);
            return;
        };
        self.panes.insert(id, pane);
        println!(
            "dropterm-gpui: tab + -> {id:?} ({} tabs) at +{}ms",
            self.tabs.records().len(),
            self.boot.elapsed().as_millis()
        );
        cx.notify();
    }

    /// The gear affordance's press: the settings entry point arrives
    /// with gpui-chrome; the row is the affordance's evidence until
    /// then.
    fn settings_requested(&mut self) {
        println!(
            "dropterm-gpui: gear pressed (settings entry arrives with gpui-chrome) at +{}ms",
            self.boot.elapsed().as_millis()
        );
    }

    /// One title sync (the tick's apply point): re-title every tab
    /// through core under the live mode — the mode-flip detector
    /// first, then each pane's polled OSC 0/2 report and cwd read.
    fn sync_tab_titles(&mut self, cx: &mut Context<Self>) {
        let mode = self.config.tab_title_mode;
        if mode != self.applied_title_mode {
            println!("dropterm-gpui: tab title mode -> {mode} (re-titling live tabs)");
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
                println!("dropterm-gpui: tab {id:?} retitled -> {}", tab.title);
            }
        }
    }

    /// The band, translated by the live slide: GPUI's animation
    /// element drives the frames (the chunk's ruling — GPUI's
    /// animation replaces the libcosmic draw-time translation), while
    /// the position is core's own slide model, one clock for pixels
    /// and wire alike — a mid-slide window swap (the keyboard-hold
    /// recreate) remounts the frame driver but the translation stays
    /// continuous off the machine's `started`.
    fn sliding_band(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        // The drawer's content is the strip plus the active pane
        // (gpui-tabs Task 1): the strip at the config's side of the
        // surface, the pane yielding the strip's height — and the
        // same slide translation carries the whole column.
        let at_bottom = tab_bar::bar_at_bottom(self.inputs.geometry.edge, self.config.tab_position);
        let pane_background = resolve_theme(&self.config).colors
            [alacritty_terminal::vte::ansi::NamedColor::Background];
        let (strip, fg) = tab_bar::strip_pair(pane_background);
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
            .on_new_tab({
                let shell = shell.clone();
                move |_window, cx: &mut App| {
                    shell.update(cx, Shell::new_tab);
                }
            })
            .on_settings({
                let shell = shell.clone();
                move |_window, cx: &mut App| {
                    shell.update(cx, |shell, _| shell.settings_requested());
                }
            });
        let Some(pane) = self.active_pane() else {
            return div().size_full();
        };
        let pane_area = div().flex_1().min_h_0().overflow_hidden().child(pane);
        let column = if at_bottom {
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(pane_area)
                .child(bar)
        } else {
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(bar)
                .child(pane_area)
        };
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
                "dropterm-gpui: frame {} mapped origin=({},{}) size={:.0}x{:.0} at +{}ms",
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
                "dropterm-gpui: OPEN->FIRST_FRAME {}ms (slide budget 130ms)",
                self.show_open.elapsed().as_millis()
            );
        }
        let active = window.is_window_active();
        if self.last_active != Some(active) {
            println!(
                "dropterm-gpui: activation {} at +{}ms (frame {}) — the keyboard re-grab \
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
        self.sliding_band(cx)
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
                "dropterm-gpui: measure pass granted {}x{} at +{}ms",
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
    hide_tx: std::sync::mpsc::Sender<()>,
    hide_requests: Receiver<()>,
}

/// The drawer's tab fleet (gpui-tabs Task 1's boot half): N panes —
/// the walk's `DROPTERM_TABS` arm, the real app's one — one record
/// per pane through core's model, every pane's Hide action on the one
/// shared channel. The PTY program is `DROPTERM_TERM_CMD`'s shell
/// line when a walk sets it (seeding every tab alike), the tty
/// default otherwise. `None` when any pane's PTY fails to spawn — the
/// drawer is never left half-fleeted.
fn boot_tab_fleet(cx: &mut App, config: &DropTermConfig, edge: Edge) -> Option<TabFleet> {
    let tab_count = tab_walk_count(std::env::var("DROPTERM_TABS").ok().as_deref());
    let pty_shell = std::env::var("DROPTERM_TERM_CMD").ok().map(|raw| {
        let raw = raw.trim().to_string();
        if raw.is_empty() {
            alacritty_terminal::tty::Shell::new("/bin/sh".to_string(), Vec::new())
        } else {
            alacritty_terminal::tty::Shell::new("/bin/sh".to_string(), vec!["-c".to_string(), raw])
        }
    });
    let (hide_tx, hide_requests) = std::sync::mpsc::channel::<()>();
    let mut tabs = tab_bar::TabList::default();
    let mut panes = HashMap::new();
    for _ in 0..tab_count {
        let id = tabs.spawn(false);
        let Some((_terminal, pane)) =
            view::boot_pane(cx, config, pty_shell.clone(), id, Some(hide_tx.clone()))
        else {
            eprintln!("dropterm-gpui: the drawer's terminal failed to spawn; exiting");
            return None;
        };
        panes.insert(id, pane);
    }
    println!(
        "dropterm-gpui: tab bar: {} tab(s) ([{}]), strip at {} (tab_position {}, edge {:?}), \
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
        hide_tx,
        hide_requests,
    })
}

/// Boot the shell: load the config, create the shell entity, and run
/// the tick that drains verbs and keeps frames flowing. The surface
/// itself starts hidden — the machine's resting state — exactly like
/// the libcosmic app; the first show verb runs the two-surface show.
pub(crate) fn boot(
    cx: &mut App,
    exit_after: Option<Duration>,
    phase: &Arc<Mutex<SurfacePhase>>,
    verbs: Receiver<Verb>,
) {
    let boot = Instant::now();
    let config_dir = dropterm_core::config::store::store_dir();
    let config = DropTermConfig::load_from(&config_dir);
    let inputs = SurfaceInputs::from_config(&config);
    println!(
        "dropterm-gpui: surface shell booted hidden (edge {:?}, {}% height, {}% width, align \
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
        println!("dropterm-gpui: drag walk armed (positions fed once shown and at rest)");
    }
    // gpui-theme Task 2: the Kit initialized and the resolved scheme
    // applied before any window exists — every Kit surface and token
    // reads the DropTerm scheme from its first frame (C4's route).
    gpui_component::init(cx);
    let boot_theme = resolve_theme(&config);
    let boot_theme_identity = theme_identity(&boot_theme);
    apply_resolved_theme(cx, &boot_theme);
    // The drawer's tab fleet (gpui-tabs Task 1).
    let Some(fleet) = boot_tab_fleet(cx, &config, inputs.geometry.edge) else {
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
            Err(err) => return eprintln!("dropterm-gpui: config watcher failed: {err:?}"),
        };
        if let Err(err) = watcher.watch(&watch_dir, notify::RecursiveMode::NonRecursive) {
            return eprintln!("dropterm-gpui: config watch failed: {err:?}");
        }
        println!(
            "dropterm-gpui: config watcher armed on {}",
            watch_dir.display()
        );
        std::thread::park();
    });
    let shell = cx.new(|_| {
        Shell::new(
            boot,
            inputs,
            Arc::clone(phase),
            verbs,
            fleet.tabs,
            fleet.panes,
            config,
            fleet.hide_tx,
            fleet.hide_requests,
            boot_theme_identity,
            drag_walk,
            config_changes,
            exit_after,
        )
    });
    // The tick: 100 ms of verb drain + frame pump, holding the shell
    // entity strongly for the app's lifetime (the surface's windows
    // come and go; the shell owns the machine and must not).
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            cx.update(|cx| {
                let pumped = shell.update(cx, |shell, _| shell.pump());
                // gpui-theme Task 2: a scheme edit re-themes before
                // the next frame — the App context lives here, not in
                // pump (the same split every window action takes).
                shell.update(cx, Shell::apply_pending_theme);
                // gpui-tabs Task 1: the tab titles sync on the same
                // apply point — each pane's report and cwd polled,
                // re-resolved through core only on a move.
                shell.update(cx, Shell::sync_tab_titles);
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
        use dropterm_core::intent::SurfaceHandle;
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
        use dropterm_core::config::ThemePreference;

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
