// SPDX-License-Identifier: GPL-3.0-only

use std::time::Instant;

use cosmic::app::Task;
use cosmic::iced::window;

use super::{DropTerm, Message, ToggleAction};
use crate::config::{DropTermConfig, KeyboardMode};
use crate::geometry::{self, Geometry};
use crate::intent::{KeyboardHold, SurfaceExecutor, SurfaceHandle, SurfaceIntent};
use crate::settings;
use crate::slide;
use crate::surface;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum SurfaceState {
    #[default]
    Hidden,
    /// A surface stretched across the whole active output, waiting for its
    /// `Window::Opened` event so we learn that output's real size.
    Measuring(SurfaceHandle),
    Shown(SurfaceHandle),
    /// A shown surface sliding out toward its anchored edge: the
    /// content still renders — translated by the live `Out` slide —
    /// and the layer surface is destroyed only when that slide
    /// completes. Keyboard interactivity was already released at
    /// slide-out start, so focus events find no `Shown` surface to
    /// act on; the drag handlers and live reshape ignore this state
    /// by construction (they match `Shown` exclusively).
    Hiding(SurfaceHandle),
}

/// The config subset every surface decision reads, resolved once per
/// decision so the machine never sees the whole config — and stays
/// constructible headless (a `DropTerm` needs a live `Core`).
#[derive(Debug, Clone, Copy)]
pub(super) struct SurfaceInputs {
    pub(super) geometry: Geometry,
    pub(super) keyboard_mode: KeyboardMode,
    pub(super) hide_on_focus_loss: bool,
    pub(super) slide_animation_enabled: bool,
}

impl SurfaceInputs {
    pub(super) fn from_config(config: &DropTermConfig) -> Self {
        Self {
            geometry: Geometry::from_config(config),
            keyboard_mode: config.keyboard_mode,
            hide_on_focus_loss: config.hide_on_focus_loss,
            slide_animation_enabled: config.slide_animation_enabled,
        }
    }
}

/// The surface lifecycle's decision core: the state (handle-carrying,
/// so no toolkit id crosses into decisions), the live slide, the
/// focus latch, the measured output, and the toggle debounce — plain
/// data over plain types, headless-constructible. The executors below
/// return intents; `DropTerm`'s thin glue applies them through the
/// libcosmic executor and keeps the app-side halves (tabs, drawer,
/// rename) beside them.
#[derive(Debug, Default)]
pub(super) struct SurfaceMachine {
    /// The lifecycle state. `Default::default` is `Hidden` — the
    /// resting state every run and every test starts from.
    pub(super) state: SurfaceState,
    /// The live slide, if content is animating (`Out` while `Hiding`,
    /// `In` riding a `Shown` surface).
    pub(super) slide: Option<slide::Playing>,
    /// The keyboard-focus latch: set by the `LayerFocused` a keyboard
    /// hold synthesizes, guarding the focus-loss hide so a stray
    /// unfocus before any focus cannot close a freshly-opened surface.
    pub(super) focused: bool,
    /// The output size measured by the last measuring pass, kept so
    /// every later reshape recomputes pixels without a new pass.
    pub(super) output_size: Option<(u32, u32)>,
    /// The last accepted toggle, for the debounce.
    last_toggle: Option<Instant>,
}

impl SurfaceMachine {
    /// Whether a toggle intent escapes the debounce window; records
    /// the acceptance when it does. The debounce collapses true spam
    /// (~6–7 effective toggles per second at 150 ms, each a full
    /// reversal) — the state machine itself never drops or queues.
    pub(super) fn toggle_accepted(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last_toggle
            && now.duration_since(last) < super::TOGGLE_DEBOUNCE
        {
            return false;
        }
        self.last_toggle = Some(now);
        true
    }

    /// The show decision: only a hidden surface starts the measuring
    /// pass — a measuring or shown one is already on its way and
    /// leaves with nothing. The intent carries the keyboard mode
    /// already resolved for a shown surface (`shown_keyboard_mode`):
    /// the map-time focus grant is the one path that focuses the
    /// surface on an empty desktop, and the executor's translation
    /// heals a lost clipboard anchor strictly ahead of the probe.
    pub(super) fn decide_show(&mut self, inputs: SurfaceInputs) -> Vec<SurfaceIntent> {
        if !matches!(self.state, SurfaceState::Hidden) {
            return Vec::new();
        }
        let handle = SurfaceHandle::next();
        self.state = SurfaceState::Measuring(handle);
        self.focused = false;
        vec![SurfaceIntent::Measure {
            handle,
            keyboard: surface::shown_keyboard_mode(inputs.keyboard_mode, inputs.hide_on_focus_loss),
        }]
    }

    /// The hide decision — every path through `toggle_step`'s `Hide`
    /// column. A measuring surface has no reshaped content to
    /// animate and dies now; a shown one slides out from wherever the
    /// content stands (the live eased offset of an in-flight
    /// slide-in, or rest — nothing teleports; the in-flight distance
    /// is kept when one exists, because a config edit that arrived
    /// mid-slide updated the config but not the frozen geometry); a
    /// hiding or hidden one leaves with nothing (no restart, no
    /// double destroy).
    pub(super) fn decide_hide(&mut self, inputs: SurfaceInputs) -> Vec<SurfaceIntent> {
        match toggle_step(
            ToggleAction::Hide,
            &self.state,
            inputs.slide_animation_enabled,
        ) {
            ToggleStep::DestroyNow(handle) => {
                self.slide = None;
                self.state = SurfaceState::Hidden;
                vec![SurfaceIntent::Destroy { handle }]
            }
            ToggleStep::SlideOut(handle) => {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "elapsed ms of one slide; u64 range is exhausted after 58M years"
                )]
                let (from_offset_px, distance_px) = match self.slide {
                    Some(playing) => (
                        slide::offset(
                            &playing.active,
                            playing.started.elapsed().as_millis() as u64,
                        ),
                        playing.active.distance_px,
                    ),
                    None => (0.0, self.slide_distance_px(inputs)),
                };
                self.state = SurfaceState::Hiding(handle);
                self.slide = Some(slide::Playing {
                    started: Instant::now(),
                    active: slide::Active {
                        direction: slide::Direction::Out,
                        from_offset_px,
                        distance_px,
                    },
                });
                // Keyboard interactivity releases at slide-out START,
                // not at destroy: the previously focused window regains
                // focus while the slide plays. The compositor answers
                // with an `Unfocused` layer event, which the
                // `Shown(handle)` guard in its handler already rejects —
                // the surface is `Hiding` by the time it arrives.
                vec![SurfaceIntent::Hold {
                    handle,
                    keyboard: KeyboardHold::Released,
                }]
            }
            // `Hide` never decides to show, and a hiding or hidden
            // surface leaves with nothing.
            ToggleStep::Measure | ToggleStep::ReverseShow(_) | ToggleStep::Leave => Vec::new(),
        }
    }

    /// Reverses a hiding slide back to shown: the live surface is
    /// reused — no new measuring pass, `output_size` is retained (the
    /// `geometry` module's guarantee) — the slide re-targets `In`
    /// from the current offset (`slide::reverse`: no teleport, a
    /// fresh fixed 130 ms), keyboard interactivity is re-held per the
    /// resolved mode ([`surface::shown_keyboard_mode`]) with the
    /// terminal input focused, and the focus latch resets pending the
    /// `LayerFocused` the re-hold synthesizes.
    pub(super) fn decide_reverse_show(
        &mut self,
        handle: SurfaceHandle,
        inputs: SurfaceInputs,
    ) -> Vec<SurfaceIntent> {
        self.state = SurfaceState::Shown(handle);
        self.focused = false;
        // `Hiding` always carries a live `Out` slide — the parking
        // tick destroys the surface and leaves `Hidden` in the same
        // handler — so there is always something to re-target; a
        // slide-less degenerate would simply show at rest. An `Out`
        // slide that already ran past its duration reverses from the
        // endpoint (the full distance), i.e. a fresh full slide-in.
        if let Some(playing) = self.slide {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "elapsed ms of one slide; u64 range is exhausted after 58M years"
            )]
            let reversed = slide::Playing {
                started: Instant::now(),
                active: slide::reverse(
                    &playing.active,
                    playing.started.elapsed().as_millis() as u64,
                ),
            };
            self.slide = Some(reversed);
        }
        // The mirror of hide's focus bookkeeping: none — hide reports
        // nothing, so nothing returns here; the `LayerFocused` this
        // re-hold synthesizes repeats the terminal's standing focus
        // harmlessly (`set_focused` reports only real changes). The
        // re-hold runs the resolved mode: a stay-open surface reversed
        // mid-slide comes back as `OnDemand`, which commits no grab of
        // its own — the window that regained focus at slide-out start
        // keeps the keyboard, and a click on the terminal wins it back
        // (the sharing rule of `shown_keyboard_mode`).
        vec![
            SurfaceIntent::Hold {
                handle,
                keyboard: KeyboardHold::from(surface::shown_keyboard_mode(
                    inputs.keyboard_mode,
                    inputs.hide_on_focus_loss,
                )),
            },
            SurfaceIntent::Focus { handle },
        ]
    }

    /// The focus-loss decision: whether the terminal must report the
    /// unfocus (only a shown surface's — unfocus events trailing our
    /// own hide must not report, or a focus-reporting program repaints
    /// its prompt per toggle), and whether the focus-loss hide fires
    /// (the config gate plus the focus latch; the latch clears only
    /// when the hide fires — a loss that doesn't hide leaves it
    /// armed). The hide itself runs through the caller's full hide
    /// path; deciding here only reads the latch.
    pub(super) fn decide_focus_loss(&mut self, inputs: SurfaceInputs) -> (bool, bool) {
        let report_unfocus = matches!(self.state, SurfaceState::Shown(_));
        let hide = focus_loss_hides(inputs.hide_on_focus_loss) && self.focused;
        if hide {
            self.focused = false;
        }
        (report_unfocus, hide)
    }

    /// The slide distance for the currently shown surface: the same
    /// deterministic `geometry::surface_height` formula `SurfaceOpened`
    /// used — deliberately not the granted window height, which
    /// double-reports through `WindowResized`. A shown surface always
    /// has a measured output beside it; the `0.0` fallback covers a
    /// state that cannot occur and still hides (a degenerate at-rest
    /// out-slide that parks, then destroys).
    fn slide_distance_px(&self, inputs: SurfaceInputs) -> f32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "output px are < 2^24, exact in f32"
        )]
        self.output_size.map_or(0.0, |output| {
            geometry::surface_height(output.1 as f32, inputs.geometry.height_percent) as f32
        })
    }
}

/// An in-progress grab-bar drag's runtime state: the pure anchor
/// every target recomputes from (`geometry`), the latest reported
/// pointer y, and the last live apply (when, which target) for the
/// change-gated apply cadence — see `geometry::drag_apply_due`.
/// Applies are bounded because each one costs the runtime two window
/// resizes plus terminal and pty resizes (its `set_size` handling
/// double-reports each apply), so per-move applies at pointer rate
/// saturate the single runtime thread — the drag froze input on
/// hardware.
pub(super) struct Drag {
    pub(super) anchor: geometry::DragAnchor,
    /// The pointer's window y as of the last `GrabBarDragged`, so the
    /// trailing apply and the release settle at where the pointer
    /// actually is, not where the last apply landed.
    pub(super) pointer_y: f32,
    /// The last live apply, for the cadence gate.
    pub(super) applied: Option<(std::time::Instant, u32)>,
}

/// Whether a `ConfigChanged` may reshape a shown surface in place:
/// pure so the guard is headless-testable (constructing `DropTerm`
/// needs a live `Core`). The geometry must actually have changed; a
/// drag armed mid-edit defers the reshape to the release settle (so
/// config reshapes and drag deltas never interleave `set_size`
/// calls); a slide animating defers it to the next show — geometry
/// stays frozen while animating. The in-memory config still updates
/// on every path (the caller does that first); only the live reshape
/// is gated.
pub(super) fn config_edit_reshapes(
    geometry_changed: bool,
    drag_armed: bool,
    slide_animating: bool,
) -> bool {
    geometry_changed && !drag_armed && !slide_animating
}

/// What a toggle-intent request must do with the surface: pure so
/// the decision is headless-testable (constructing `DropTerm` needs
/// a live `Core`). Every toggle caller goes through here — the
/// toggle hotkey and D-Bus activation (`debounced_toggle`), plus
/// `Ctrl+Shift+Q` and focus loss through `hide`'s `Hide` column —
/// so the full state × intent matrix lives in one place and
/// reversibility is just another cell. Slide activity needs no
/// separate input: it is implied by the state — an `Out` slide only
/// exists while `Hiding`, an `In` slide only rides a `Shown`
/// surface — and the executors read the live slide for offsets.
///
/// `slide_animation_enabled` gates transition *entry* only: it
/// swaps the `Shown`-hide cell between sliding out and destroying
/// now — the destroy *style*, never the direction. The helper takes
/// no mid-flight input by construction, so a running slide is never
/// aborted by its own toggle: the `Hiding` cells (destroy-on-
/// completion, reversal) key on state alone and ignore the flag
/// entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToggleStep {
    /// Run the measuring show pass (a hidden surface only): measure
    /// → reshape → shown, then slide in.
    Measure,
    /// Destroy immediately: a measuring surface has no reshaped
    /// content to animate, so there is nothing to slide.
    DestroyNow(SurfaceHandle),
    /// Slide out from the current offset and destroy on the slide's
    /// completion tick. Keyboard interactivity releases at slide-out
    /// start.
    SlideOut(SurfaceHandle),
    /// Reverse a hiding slide back to shown on the same live surface:
    /// no new measuring pass — `output_size` is retained — the slide
    /// re-targets `In` from the current offset, keyboard
    /// interactivity is re-grabbed per config, and `surface_focused`
    /// resets pending the next `LayerFocused`.
    ReverseShow(SurfaceHandle),
    /// Nothing: the intent is already satisfied — a hide while
    /// hiding or hidden (no restart, no double destroy), a show
    /// while measuring or shown.
    Leave,
}

pub(super) fn toggle_step(
    action: ToggleAction,
    surface: &SurfaceState,
    slide_enabled: bool,
) -> ToggleStep {
    match (action, surface) {
        // Show intents: only a hidden surface starts the measuring
        // pass; a hiding one reverses; a measuring or shown one is
        // already on its way. None of these arms key on the slide
        // toggle — a hiding surface always reverses on the live
        // slide, whatever the flag says now (never aborted).
        (ToggleAction::Show | ToggleAction::Toggle, SurfaceState::Hidden) => ToggleStep::Measure,
        (ToggleAction::Show | ToggleAction::Toggle, SurfaceState::Hiding(id)) => {
            ToggleStep::ReverseShow(*id)
        }
        (ToggleAction::Show, SurfaceState::Measuring(_) | SurfaceState::Shown(_)) => {
            ToggleStep::Leave
        }
        // Hide intents — and a toggle on any visible surface: a
        // measuring surface dies now, a hiding or hidden one stays
        // put (idempotent — no restart, no double destroy). A shown
        // one slides out (mid-slide-in the executor starts from the
        // live eased offset) — or, with the slide toggle off,
        // destroys now: instant hide.
        (ToggleAction::Hide | ToggleAction::Toggle, SurfaceState::Measuring(id)) => {
            ToggleStep::DestroyNow(*id)
        }
        (ToggleAction::Hide | ToggleAction::Toggle, SurfaceState::Shown(id)) => {
            if slide_enabled {
                ToggleStep::SlideOut(*id)
            } else {
                ToggleStep::DestroyNow(*id)
            }
        }
        (ToggleAction::Hide, SurfaceState::Hiding(_) | SurfaceState::Hidden) => ToggleStep::Leave,
        // Structurally unreachable: `debounced_toggle` applies the
        // remote verbs — opening a new-tab, closing a close-tab,
        // selecting a select-tab, renaming a rename-tab, duplicating
        // a duplicate-tab, switching a switch-profile-tab — outside
        // the toggle step, so none ever gets here. Loud rather than
        // silently wrong.
        (
            ToggleAction::NewTab { .. }
            | ToggleAction::CloseTab { .. }
            | ToggleAction::SelectTab { .. }
            | ToggleAction::RenameTab { .. }
            | ToggleAction::DuplicateTab { .. }
            | ToggleAction::SwitchProfileTab { .. },
            _,
        ) => {
            unreachable!("remote verbs apply outside the funnel")
        }
    }
}

/// Whether a `LayerUnfocused` event for `id` should reach
/// [`DropTerm::focus_lost`]: only a shown surface reacts to losing
/// keyboard focus — one arriving while `Hiding` (the synthesized
/// unfocus of the hide's own keyboard release, or a racing focus
/// loss) is a no-op, so it can never restart or double-drive the
/// out-slide.
pub(super) fn layer_unfocused_acts_on(surface: &SurfaceState, id: SurfaceHandle) -> bool {
    matches!(surface, SurfaceState::Shown(shown) if *shown == id)
}

/// Whether losing keyboard focus should hide the surface: exactly
/// the `hide_on_focus_loss` config flag. No settings-drawer input
/// is needed: focus loss may hide the surface with the drawer open —
/// the drawer and its drafts survive for the next show (spec
/// criterion 8) — so the gate runs the same in the drawer-open
/// state. Only the automatic focus-loss path consults it: explicit
/// hides (the toggle keybind, `Action::Hide`) call `hide()` directly
/// and ignore the gate entirely. Both unfocused routes — the layer
/// surface's `LayerUnfocused` and the `WindowUnfocused` callback —
/// route into [`DropTerm::focus_lost`], so one gate covers both.
pub(super) fn focus_loss_hides(hide_on_focus_loss: bool) -> bool {
    hide_on_focus_loss
}

/// Whether the grab-bar drag handlers may act: pure so the guard is
/// headless-testable, in the same style as
/// [`config_edit_reshapes`] and [`layer_unfocused_acts_on`]. A drag
/// must only ever run on a shown surface at rest: the `Hiding` arm
/// excludes out-slides and their reversals by state, and the slide
/// arm excludes slide-ins — which ride a `Shown` surface, so the
/// surface state alone cannot gate them (a press inside the 130 ms
/// window must not arm a drag, or the first cadence-eligible move
/// would fire `set_size` on the animating surface). Gating the
/// *arming* here covers the trailing `DragApplyDue` and the
/// `GrabBarReleased` settle by construction: both only act on an
/// armed drag, no drag can arm while a slide animates, and `hide()`
/// clears any live drag before any slide can start. A throttled
/// press is simply inert — the surface rests in at most 130 ms and
/// the next press drags normally.
pub(super) fn grab_bar_acts_on(surface: &SurfaceState, slide_animating: bool) -> bool {
    matches!(surface, SurfaceState::Shown(_)) && !slide_animating
}

/// Whether a `WindowResized` event for `id` may feed the
/// terminal-height pin: the tracked layer surface's granted sizes
/// alone — never a `Measuring` probe's (the full-output stretch
/// the pass exists to *read*, whose sizes must never become the
/// terminal's — adopting them grows the grid once per toggle and
/// returns every grown line as scrollback rot on the shrink) and
/// never a foreign window's (the clipboard anchor). Pure, in the
/// same style as [`layer_unfocused_acts_on`].
pub(super) fn resize_acts_on(surface: &SurfaceState, id: SurfaceHandle) -> bool {
    matches!(
        surface,
        SurfaceState::Shown(shown) | SurfaceState::Hiding(shown) if *shown == id
    )
}

impl DropTerm {
    /// Applies a decision's intents through the libcosmic executor,
    /// batched — exactly today's composition (the measure pass's
    /// anchor heal chains strictly ahead of its probe inside the
    /// executor's own translation; no other intent sequences).
    fn apply_surface_intents(&mut self, intents: &[SurfaceIntent]) -> Task<Message> {
        let tasks: Vec<Task<Message>> = intents
            .iter()
            .copied()
            .map(|intent| self.surface_exec.execute(intent))
            .collect();
        Task::batch(tasks)
    }

    pub(super) fn show(&mut self) -> Task<Message> {
        // The user can see the terminal again: re-arm every tab's
        // notification latches — bell and finished-command — so a
        // bell or a finished long command from here on is fresh news
        // worth one notification per tab again.
        for tab in &mut self.tabs {
            tab.bell_notified = false;
            tab.command_notified = false;
        }
        self.ensure_tabs();
        let intents = self
            .surface
            .decide_show(SurfaceInputs::from_config(&self.config));
        self.apply_surface_intents(&intents)
    }

    pub(super) fn hide(&mut self) -> Task<Message> {
        // Hiding the surface is a focus loss for the rename input:
        // commit before the content slides out.
        let commit = self.commit_pending_rename();
        // The drawer survives the hide (spec criterion 8): its open
        // state and in-flight drafts stay exactly as they were for
        // the next show. The named no-op documents that at the seam
        // a hide would otherwise silently forget; only an explicit
        // close ever clears (see `close_settings_drawer`).
        self.settings_buffer
            .apply_drawer_transition(settings::DrawerTransition::FocusLossHide);
        // The page-local form draft rides the same rule: a hide
        // preserves it (the named no-op; only the explicit close's
        // `turn_settings_page` away from the Profiles tab clears).
        settings::apply_profile_draft_transition(
            &mut self.profile_draft,
            settings::ProfileDraftTransition::Hide,
        );
        let intents = self
            .surface
            .decide_hide(SurfaceInputs::from_config(&self.config));
        let task = self.apply_surface_intents(&intents);
        // The surface is leaving on every destructive path, and the
        // cleanup is idempotent for a leave-with-nothing: any live
        // grab-bar drag is moot, the terminal pin's debounce state
        // starts over so the next show's first granted size adopts
        // immediately, and the active terminal keeps its reported
        // focus — the dropdown's own visibility toggle is not a focus
        // event for the PTY (a focus-reporting program — fish, vim,
        // tmux — would repaint its prompt per toggle), and the PTY
        // keeps running regardless, because PTYs live in tabs, not the
        // window.
        self.drag = None;
        self.pending_height = None;
        self.pin_applied = None;
        // The context menu's popup dies with the surface and its
        // `on_close` never fires — clear the latch here, or the next
        // show's first right-click would be consumed as a
        // toggle-close of a menu that is not there.
        self.context_menu_open = false;
        Task::batch([commit, task])
    }

    /// Reverses a hiding slide back to shown (see
    /// [`SurfaceMachine::decide_reverse_show`]); the executor's
    /// translation re-holds keyboard per the resolved mode and
    /// focuses the terminal input.
    pub(super) fn reverse_show(&mut self, handle: SurfaceHandle) -> Task<Message> {
        let intents = self
            .surface
            .decide_reverse_show(handle, SurfaceInputs::from_config(&self.config));
        self.apply_surface_intents(&intents)
    }

    /// Handles the surface losing keyboard focus: marks the terminal
    /// unfocused and, when configured, hides the overlay. The hide is
    /// guarded by the focus latch so a stray unfocus before any focus
    /// (possible with `keyboard_mode: OnDemand`) can't close a
    /// freshly-opened surface. The latch is cleared only when the
    /// hide fires; a loss that doesn't hide (the config off) leaves
    /// it armed. No settings-window case exists here: while that
    /// window is open the surface is hidden by construction, so
    /// focus-loss events cannot arrive for a shown surface in that
    /// state.
    pub(super) fn focus_lost(&mut self) -> Task<Message> {
        // Losing keyboard focus commits a rename: the input's own
        // unfocus signal only covers Escape and Tab, which cancel.
        let commit = self.commit_pending_rename();
        let (report_unfocus, hide) = self
            .surface
            .decide_focus_loss(SurfaceInputs::from_config(&self.config));
        // Only a shown surface's focus loss reaches the PTY.
        if report_unfocus && let Some(tab) = self.active() {
            tab.terminal.lock().unwrap().set_focused(false);
        }
        if hide {
            return Task::batch([commit, self.hide()]);
        }
        commit
    }

    /// The geometry a settled grab-bar drag leaves behind: the current
    /// config's geometry with the height quantized once from where the
    /// pointer released — the *latest* target, not the last one the
    /// cadence gate landed, so throttling never decides the final
    /// height. The settled surface then matches the persisted percent,
    /// at most half a percent off the dragged pixels (accepted
    /// tolerance).
    pub(super) fn settled_geometry(&self, drag: &Drag, output: (u32, u32)) -> Geometry {
        let final_target =
            geometry::drag_target_px(self.config.edge, output.1, drag.anchor, drag.pointer_y);
        let mut geom = Geometry::from_config(&self.config);
        geom.height_percent = geometry::percent_from_px(output.1, final_target);
        geom
    }

    /// Apply a live drag target now: the cadence gate has passed.
    /// Records the apply, offers the height to the terminal pin —
    /// adoptions are debounced (see `offer_window_height`), because
    /// the runtime double-reports every `set_size` (once stale, once
    /// real) and each adoption costs a terminal reflow plus a pty
    /// resize that made the drag sluggish on hardware — and sends the
    /// single-path triple with the pixel height.
    pub(super) fn apply_drag_target(
        &mut self,
        id: window::Id,
        output: (u32, u32),
        target: u32,
    ) -> Task<Message> {
        if let Some(drag) = self.drag.as_mut() {
            drag.applied = Some((std::time::Instant::now(), target));
            drag.anchor.applied_px = target;
        }
        // Declare the request before offering it: the cadence-gated
        // adoption below is the drag's own grant path, and the echo
        // gate needs to know this height was asked for.
        #[expect(
            clippy::cast_precision_loss,
            reason = "surface px are < 2^24, exact in f32"
        )]
        let requested = target as f32;
        self.requested_height = Some(requested);
        let pin_task = self.offer_window_height(requested);
        let geom = Geometry::from_config(&self.config);
        let apply = geometry::apply_drag_geometry(id, geom, output, target);
        match pin_task {
            Some(t) => Task::batch([apply, t]),
            None => apply,
        }
    }

    /// Offer a height to the terminal pin: adopt immediately when the
    /// debounce allows (first size, re-report of the adopted height,
    /// or interval elapsed), else remember it and return the
    /// trailing-adopt timer that lands it once the granted stream
    /// settles. Every granted-size follower goes through here —
    /// `WindowResized` echoes outside drags and the drag's own
    /// applies — so the double-reported stream can never reflow the
    /// terminal at the apply rate.
    pub(super) fn offer_window_height(&mut self, height: f32) -> Option<Task<Message>> {
        // The stale-echo gate (toggle-grid-stability, round 2): a
        // granted height may only reach the debounce when it is the
        // grant of a request we made or a re-report of the adopted
        // pin. Anything else is the synthetic pre-request echo every
        // `set_size` double-reports — the full-output stretch at
        // show, an intermediate drag height after a release — and
        // the interval debounce only defers it while it lands
        // inside the window; on hardware the startup echo arrived
        // after wgpu init, outwaited the interval, and the
        // pre-armed pin adopted it. Dropped here, timing-free.
        if geometry::offer_is_stale_echo(self.requested_height, self.window_height, height) {
            return None;
        }
        let now = std::time::Instant::now();
        if geometry::window_pin_due(self.pin_applied, now, height) {
            self.window_height = Some(height);
            // The spawn-grid settle's close (status-glance criterion
            // 1): the granted stream just settled — and the grant
            // already applied (the draw resized before the echo
            // arrived), while `Terminal::resize` no-ops on unchanged
            // sizes, so waiting for the next changed resize would
            // wait forever and the fresh-prompt latch would never
            // arm (the 2026-10-24 fresh-start trace). Close at the
            // grid already held. A no-op on settled records, so a
            // re-show's adoption can never absorb a live deviation
            // into the record.
            for tab in &self.tabs {
                tab.terminal.lock().unwrap().settle_spawn_grid_now();
            }
            // Re-adopting the height the pin already has must not
            // refresh the adoption clock (see `window_pin_due`): the
            // echo stream would otherwise starve changed heights.
            #[expect(
                clippy::float_cmp,
                reason = "echo detection: the compositor returns the exact f32 it was granted"
            )]
            let pinned = match self.pin_applied {
                Some((at, adopted)) if adopted == height => (at, height),
                _ => (now, height),
            };
            self.pin_applied = Some(pinned);
            self.pending_height = None;
            return None;
        }
        if self.pending_height != Some(height) {
            self.pending_height = Some(height);
            return Some(Task::perform(
                tokio::time::sleep(geometry::WINDOW_PIN_INTERVAL),
                |()| cosmic::Action::App(Message::WindowPinDue),
            ));
        }
        None
    }

    /// Pre-arm the terminal-height pin at the height the show just
    /// requested: [`SurfaceOpened`] calls this before its reshape
    /// tasks run, so the granted-size echoes those tasks produce
    /// meet an armed pin, not an empty one. The load-bearing case
    /// is the stale synthetic configure every `set_size` carries —
    /// at show time it reports the measuring pass's full-output
    /// height, and an empty pin adopts immediately
    /// (`window_pin_due(None, ..)`), growing the grid once per
    /// toggle and pushing the grown lines into scrollback on the
    /// shrink back. Armed, that echo matches neither the request
    /// nor the adopted pin and drops at the stale-echo gate
    /// ([`geometry::offer_is_stale_echo`]) whatever the clock says;
    /// the real grant equals the armed height and re-adopts for
    /// free. The pre-arm only ever names the request, never a
    /// grant: under the gate, a grant that differed from the
    /// request would drop as echo noise too — the compositor
    /// grants each `set_size` exactly ([`geometry::DRAG_APPLY_INTERVAL`]'s
    /// contract), and the pin would rest on the request until the
    /// next show's measuring pass re-reads the output and re-arms.
    pub(super) fn prearm_show_pin(&mut self, output: (u32, u32), geom: Geometry) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "output px are < 2^24, exact in f32"
        )]
        let height = geometry::surface_height(output.1 as f32, geom.height_percent) as f32;
        self.window_height = Some(height);
        self.pin_applied = Some((std::time::Instant::now(), height));
        self.pending_height = None;
        self.requested_height = Some(height);
    }

    /// Record the height a geometry reshape (`set_size`) is about to
    /// request, without touching the pin: the config-edit reshape
    /// and the drag release call this beside their `apply_geometry`,
    /// so the stale-echo gate can tell their grant from the
    /// synthetic pre-request echo the double-report carries. The
    /// show path pre-arms instead ([`Self::prearm_show_pin`]) —
    /// arming the pin ahead of the request is what makes the show
    /// pass inert; the later reshapes keep the live pin and only
    /// declare their request.
    pub(super) fn request_geometry_height(&mut self, output: (u32, u32), geom: Geometry) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "output px are < 2^24, exact in f32"
        )]
        let height = geometry::surface_height(output.1 as f32, geom.height_percent) as f32;
        self.requested_height = Some(height);
    }

    /// The surface's current width in px: the value both the drawer
    /// width policy (render + drag) and the height drag target
    /// against. 480 while no output has reported a size — the same
    /// fallback the drawer's first frame renders at.
    pub(super) fn current_surface_width(&self) -> f32 {
        self.surface.output_size.map_or(480.0, |(width, _)| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "output px are < 2^24, exact in f32"
            )]
            {
                geometry::surface_width(width as f32, self.config.width_percent) as f32
            }
        })
    }

    pub(super) fn debounced_toggle(&mut self, action: ToggleAction) -> Task<Message> {
        if !self.surface.toggle_accepted(std::time::Instant::now()) {
            return Task::none();
        }

        // A remote close-tab is a quiet state edit, not a toggle
        // step: it applies through [`Self::close_tab_at`] and shows
        // nothing — no overlay, and nothing re-shows — so
        // a script closing a tab never disturbs what the user is
        // looking at. The debounce above still guards it: a
        // compositor double-delivery must not close two tabs.
        if let ToggleAction::CloseTab { tab_index } = action {
            return self.close_remote_tab(tab_index);
        }

        // A remote rename-tab is a quiet state edit like close-tab:
        // the title lands on the target — overriding or clearing —
        // with no overlay and none of the settings close-first rule
        // below. The debounce above still guards it: a compositor
        // double-delivery must not apply the rename twice against a
        // stale target.
        if let ToggleAction::RenameTab { title, tab_index } = action {
            return self.rename_remote_tab(&title, tab_index);
        }

        // A remote select-tab is a tab-facing verb like a new-tab: a
        // resolved target activates — riding set_active_tab's
        // attention retirement, session-dirty mark, and
        // already-active no-op — and the overlay shows through the
        // same path a `new-tab` takes, mid-slide-out included. An
        // unresolvable target — no tabs, out of range — is a quiet
        // no-op like close-tab's: no activation and no surface half
        // (nothing re-shows; a script's failed select must not
        // disturb what the user is looking at). The debounce above
        // still guards it: a compositor double-delivery must not
        // count twice.
        if let ToggleAction::SelectTab { index } = action {
            return match self.select_remote_tab(index) {
                Some(activate) => Task::batch([activate, self.show_intent_surface()]),
                None => Task::none(),
            };
        }

        // A remote new-tab is not a toggle step: the tab opens — now
        // or once the channel bootstraps — and the overlay shows,
        // whatever state it was in. The surface half rides the show
        // path below, so a request arriving mid-slide-out reverses on
        // the live surface exactly like a `show` would.
        if let ToggleAction::NewTab { .. } = action {
            let open = self.open_remote_tab(action, self.config.new_tab_after_current);
            return Task::batch([open, self.show_intent_surface()]);
        }

        // A remote duplicate-tab is a tab-facing verb like a new-tab:
        // a fresh shell tab opens in the target's cwd — now or once
        // the channel bootstraps — and the overlay shows through the
        // same path. An unresolvable target never blocks it: the
        // spawn degrades to the bare new-tab inheritance.
        if let ToggleAction::DuplicateTab { tab_index } = action {
            let open = self.duplicate_remote_tab(tab_index);
            return Task::batch([open, self.show_intent_surface()]);
        }

        // A remote switch-profile-tab is a tab-facing verb like a
        // duplicate-tab: the result — the tab now running the
        // profile — is meant to be seen, so the overlay shows through
        // the same path whatever state it was in. The verb resolves
        // its inputs up front and rides the `Message::SwitchTabProfile`
        // seam — the same apply the tab context menu will use, and
        // the construction that keeps the variant live under
        // `-D warnings`. Unresolvable inputs warn and edit nothing —
        // unlike duplicate-tab there is no sensible bare fallback —
        // while the surface half still shows.
        if let ToggleAction::SwitchProfileTab { profile, tab_index } = action {
            // `profile::find`'s exact, case-sensitive, first-match
            // rule, resolved to the index the apply wants.
            let Some(profile_index) = self
                .config
                .profiles
                .iter()
                .position(|candidate| candidate.name == profile)
            else {
                log::warn!("switch-profile-tab: no profile named {profile}");
                return self.show_intent_surface();
            };
            // The target — absent means the active tab — resolves
            // like every remote tab verb's, strict 1-based.
            let Some(target) =
                crate::tabs::remote_target_index(self.tabs.len(), tab_index, self.active_tab)
            else {
                log::warn!(
                    "switch-profile-tab: no tab at --tab-index {tab_index:?} ({} open)",
                    self.tabs.len()
                );
                return self.show_intent_surface();
            };
            let tab_id = self.tabs[target].id;
            return Task::batch([
                cosmic::task::message(cosmic::Action::App(Message::SwitchTabProfile(
                    tab_id,
                    profile_index,
                ))),
                self.show_intent_surface(),
            ]);
        }

        // Every toggle decision executes through `toggle_step` — most
        // importantly, a mid-hide toggle or `Show` reverses on the
        // live surface instead of being dropped. The debounce above
        // already collapses true spam (~6–7 effective toggles per
        // second at 150 ms, each a full reversal); the state machine
        // itself never drops or queues. The slide toggle rides in
        // with the decision: with it off, a shown surface destroys
        // now instead of sliding out; with it on, every hide slides.
        // The settings drawer, if open, rides along: it lives inside
        // the surface, so a hide preserves it and the next show
        // uncovers it exactly as it was (spec criterion 8).
        match toggle_step(
            action,
            &self.surface.state,
            self.config.slide_animation_enabled,
        ) {
            ToggleStep::Measure => self.show(),
            ToggleStep::DestroyNow(_) | ToggleStep::SlideOut(_) => self.hide(),
            ToggleStep::ReverseShow(handle) => self.reverse_show(handle),
            ToggleStep::Leave => Task::none(),
        }
    }

    /// The surface half a tab-facing remote verb takes: the same show
    /// decision a `new-tab` gets, so the verb shows the overlay
    /// whatever state it was in — and a request arriving
    /// mid-slide-out reverses on the live surface exactly like a
    /// `show` would.
    pub(super) fn show_intent_surface(&mut self) -> Task<Message> {
        match toggle_step(
            ToggleAction::Show,
            &self.surface.state,
            self.config.slide_animation_enabled,
        ) {
            ToggleStep::Measure => self.show(),
            ToggleStep::ReverseShow(handle) => self.reverse_show(handle),
            _ => Task::none(),
        }
    }
}

#[cfg(test)]
mod tests;
