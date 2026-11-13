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

use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::{App, Bounds, Context, Entity, Pixels, Window, div, prelude::*, rems, rgba, white};

use dropterm_core::config::DropTermConfig;
use dropterm_core::dbus_state::SurfacePhase;
use dropterm_core::intent::SurfaceExecutor;
use dropterm_core::surface_lifecycle::{SurfaceInputs, SurfaceMachine, SurfaceState};
use notify::Watcher as _;

use crate::dbus::Verb;
use crate::executor::{Action, GpuiExecutor};

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

/// The clock band the shaped surface renders while shown — the
/// scaffold's stand-in content, proving the render loop stays alive
/// until the terminal slice replaces it.
fn band() -> impl IntoElement {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(rems(4.5))
        .text_color(white())
        .bg(rgba(0x0000_0044))
        .child(format!(
            "{:02}:{:02}:{:02}",
            (now / 3600) % 24,
            (now / 60) % 60,
            now % 60
        ))
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

/// The shell view: the shaped window's root. Owns the decision layer
/// and the executor, renders the band, and logs the mapped-bounds
/// evidence rows (first frame, every change) the walks read.
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
    /// The machine phase this shell publishes for the dbus mirror.
    phase: Arc<Mutex<SurfacePhase>>,
    /// The forwarded verbs, drained on the tick.
    verbs: Receiver<Verb>,
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
        drag_walk: Option<Vec<f32>>,
        config_changes: Receiver<()>,
        exit_after: Option<Duration>,
    ) -> Self {
        Self {
            boot,
            show_open: boot,
            frames: 0,
            last_bounds: None,
            machine: SurfaceMachine::default(),
            executor: GpuiExecutor::new(),
            inputs,
            drag_walk: drag_walk.unwrap_or_default().into(),
            drag_release_at: None,
            config_changes,
            config_quiet_until: None,
            last_active: None,
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
        let granted = self.measured.lock().map_or(None, |mut slot| slot.take());
        if let Some(output) = granted {
            self.measured(output, &mut actions);
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
        let new_inputs = SurfaceInputs::from_config(&config);
        let old_geometry = self.inputs.geometry;
        let drag_armed = self.machine.drag.is_some();
        let intents = self
            .machine
            .decide_reshape(old_geometry, new_inputs.geometry, drag_armed);
        if intents.is_empty() && old_geometry == new_inputs.geometry {
            // The write (or its echo) changed nothing the surface
            // reads: absorb silently, and open the quiet window so
            // the rest of the write's event burst costs no rows.
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

    /// One forwarded surface verb through the toggle funnel: the
    /// debounce first, then the state-matched decision. The show path
    /// runs the two-surface show; a hide on a shown surface is the
    /// slide's to walk (gpui-surface Task 3) — until then it logs
    /// loudly and the surface (truthfully) stays.
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
            // A measuring surface has no reshaped content to animate:
            // its hide is the machine's destroy-now, applied whole.
            (Verb::Hide | Verb::Toggle, SurfaceState::Measuring(_)) => {
                let intents = self.machine.decide_hide(self.inputs);
                self.execute_into(intents, actions);
            }
            (Verb::Hide | Verb::Toggle, SurfaceState::Shown(_)) => {
                println!(
                    "dropterm-gpui: hide on a shown surface arrives with the slide (task 3); \
                     the surface stays shown"
                );
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
            println!(
                "dropterm-gpui: surface destroyed at +{}ms",
                shell.read(cx).boot.elapsed().as_millis()
            );
        }
        Action::FocusTerminal { handle } => {
            // The terminal input arrives with the gpui-terminal slice;
            // the intent's presence stays a logged row so the walks see
            // the focus decision land.
            println!(
                "dropterm-gpui: terminal focus for {handle:?} (input arrives with \
                 gpui-terminal) at +{}ms",
                shell.read(cx).boot.elapsed().as_millis()
            );
        }
    }
}

impl gpui::Render for Shell {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
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
            self.last_active = Some(active);
        }
        band()
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
}
