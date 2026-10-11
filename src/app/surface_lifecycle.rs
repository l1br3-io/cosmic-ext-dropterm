// SPDX-License-Identifier: GPL-3.0-only

use cosmic::app::Task;
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    KeyboardInteractivity, destroy_layer_surface, get_layer_surface, set_keyboard_interactivity,
};
use cosmic::iced::window;
use cosmic::widget;

use super::{DropTerm, Message, TOGGLE_DEBOUNCE, ToggleAction};
use crate::geometry::{self, Geometry};
use crate::settings;
use crate::slide;
use crate::surface;
use crate::tabs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SurfaceState {
    Hidden,
    /// A surface stretched across the whole active output, waiting for its
    /// `Window::Opened` event so we learn that output's real size.
    Measuring(window::Id),
    Shown(window::Id),
    /// A shown surface sliding out toward its anchored edge: the
    /// content still renders — translated by the live `Out` slide —
    /// and the layer surface is destroyed only when that slide
    /// completes. Keyboard interactivity was already released at
    /// slide-out start, so focus events find no `Shown` surface to
    /// act on; the drag handlers and live reshape ignore this state
    /// by construction (they match `Shown` exclusively).
    Hiding(window::Id),
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
    DestroyNow(window::Id),
    /// Slide out from the current offset and destroy on the slide's
    /// completion tick. Keyboard interactivity releases at slide-out
    /// start.
    SlideOut(window::Id),
    /// Reverse a hiding slide back to shown on the same live surface:
    /// no new measuring pass — `output_size` is retained — the slide
    /// re-targets `In` from the current offset, keyboard
    /// interactivity is re-grabbed per config, and `surface_focused`
    /// resets pending the next `LayerFocused`.
    ReverseShow(window::Id),
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
pub(super) fn layer_unfocused_acts_on(surface: &SurfaceState, id: window::Id) -> bool {
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
/// terminal-height pin: only the tracked layer surface's granted
/// sizes count — a foreign window (the clipboard anchor) must never
/// pin the terminal's. Pure, in the same style
/// as [`layer_unfocused_acts_on`].
pub(super) fn resize_acts_on(surface: &SurfaceState, id: window::Id) -> bool {
    matches!(
        surface,
        SurfaceState::Measuring(shown) | SurfaceState::Shown(shown) | SurfaceState::Hiding(shown)
            if *shown == id
    )
}

impl DropTerm {
    pub(super) fn show(&mut self) -> Task<Message> {
        if !matches!(self.surface, SurfaceState::Hidden) {
            return Task::none();
        }
        // The user can see the terminal again: re-arm every tab's bell
        // notification latch, so a bell from here on is fresh news
        // worth one notification per tab again.
        for tab in &mut self.tabs {
            tab.bell_notified = false;
        }
        self.ensure_tabs();
        let id = window::Id::unique();
        self.surface = SurfaceState::Measuring(id);
        self.surface_focused = false;
        get_layer_surface(surface::measuring(id))
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
        // The slide toggle feeds the decision: a disabled slide turns
        // this hide into an immediate destroy inside `toggle_step` —
        // the executor arms below then simply never runs.
        let task = match toggle_step(
            ToggleAction::Hide,
            &self.surface,
            self.config.slide_animation_enabled,
        ) {
            ToggleStep::DestroyNow(id) => {
                // A measuring surface has no reshaped content to
                // animate: destroy immediately, no slide.
                self.slide = None;
                self.surface = SurfaceState::Hidden;
                destroy_layer_surface(id)
            }
            ToggleStep::SlideOut(id) => {
                // A shown surface slides out and its layer is
                // destroyed on completion. The out-slide starts from
                // wherever the content stands — the live eased offset
                // of an in-flight slide-in, or rest — so nothing
                // teleports. Its distance is the same deterministic
                // formula the show used, kept from the in-flight slide
                // when one exists: a config edit that arrived
                // mid-slide updates the config but not the frozen
                // geometry, so the in-flight distance is the one the
                // visible surface still has.
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
                    None => (0.0, self.slide_distance_px()),
                };
                self.surface = SurfaceState::Hiding(id);
                self.slide = Some(slide::Playing {
                    started: std::time::Instant::now(),
                    active: slide::Active {
                        direction: slide::Direction::Out,
                        from_offset_px,
                        distance_px,
                    },
                });
                // Keyboard interactivity releases at slide-out START,
                // not at destroy: the previously focused window
                // regains focus while the slide plays. The compositor
                // answers with an `Unfocused` layer event, which the
                // `Shown(id)` guard in its handler already rejects —
                // the surface is `Hiding` by the time it arrives.
                set_keyboard_interactivity(id, KeyboardInteractivity::None)
            }
            // `Hide` never decides to show, and a hiding or hidden
            // surface leaves with nothing: no restart, no double
            // destroy.
            ToggleStep::Measure | ToggleStep::ReverseShow(_) | ToggleStep::Leave => Task::none(),
        };
        // The surface is leaving on every destructive path, and the
        // cleanup is idempotent for `Leave`: any live grab-bar drag
        // is moot, the terminal pin's debounce state starts over so
        // the next show's first granted size adopts immediately, and
        // the active terminal keeps its reported focus — the
        // dropdown's own visibility toggle is not a focus event for
        // the PTY (a focus-reporting program — fish, vim, tmux —
        // would repaint its prompt per toggle), and the PTY keeps
        // running regardless, because PTYs live in tabs, not the
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

    /// Reverses a hiding slide back to shown: the live surface is
    /// reused — no new measuring pass, `output_size` is retained (the
    /// `geometry` module's guarantee) — the slide re-targets `In`
    /// from the current offset (`slide::reverse`: no teleport, a
    /// fresh fixed 130 ms), keyboard interactivity is re-grabbed per
    /// config, and `surface_focused` resets to `false` pending the
    /// `LayerFocused` the re-grab synthesizes. No focus report
    /// returns here — none left at hide start (the toggle is not a
    /// focus event), so the PTY never noticed the hide and keeps
    /// running throughout.
    pub(super) fn reverse_show(&mut self, id: window::Id) -> Task<Message> {
        self.surface = SurfaceState::Shown(id);
        self.surface_focused = false;
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
                started: std::time::Instant::now(),
                active: slide::reverse(
                    &playing.active,
                    playing.started.elapsed().as_millis() as u64,
                ),
            };
            self.slide = Some(reversed);
        }
        // The mirror of hide's focus bookkeeping: none — hide reports
        // nothing, so nothing returns here; the `LayerFocused` this
        // re-grab synthesizes repeats the terminal's standing focus
        // harmlessly (`set_focused` reports only real changes).
        Task::batch([
            set_keyboard_interactivity(
                id,
                surface::keyboard_interactivity(self.config.keyboard_mode),
            ),
            widget::text_input::focus(self.terminal_id.clone()),
        ])
    }

    /// The slide distance for the currently shown surface: the same
    /// deterministic `geometry::surface_height` formula `SurfaceOpened`
    /// used — deliberately not the granted window height, which
    /// double-reports through `WindowResized`. A shown surface always
    /// has a measured output beside it; the `0.0` fallback covers a
    /// state that cannot occur and still hides (a degenerate at-rest
    /// out-slide that parks, then destroys).
    pub(super) fn slide_distance_px(&self) -> f32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "output px are < 2^24, exact in f32"
        )]
        self.output_size.map_or(0.0, |output| {
            geometry::surface_height(
                output.1 as f32,
                Geometry::from_config(&self.config).height_percent,
            ) as f32
        })
    }

    /// Handles the surface losing keyboard focus: marks the terminal unfocused
    /// and, when configured, hides the overlay. The hide is guarded by the
    /// focus latch so a stray unfocus before any focus (possible with
    /// `keyboard_mode: OnDemand`) can't close a freshly-opened surface. The
    /// latch is cleared only when the hide fires; a loss that doesn't hide
    /// (the config off) leaves it armed. No settings-window case exists
    /// here: while that window is open the surface is hidden by
    /// construction, so focus-loss events cannot arrive for a shown
    /// surface in that state.
    pub(super) fn focus_lost(&mut self) -> Task<Message> {
        // Losing keyboard focus commits a rename: the input's own
        // unfocus signal only covers Escape and Tab, which cancel.
        let commit = self.commit_pending_rename();
        // Only a shown surface's focus loss reaches the PTY: unfocus
        // events trailing our own hide (`Hiding`/`Hidden` by the time
        // they arrive) must not report, or a focus-reporting program
        // repaints its prompt per toggle.
        if matches!(self.surface, SurfaceState::Shown(_))
            && let Some(tab) = self.active()
        {
            tab.terminal.lock().unwrap().set_focused(false);
        }
        if focus_loss_hides(self.config.hide_on_focus_loss) && self.surface_focused {
            self.surface_focused = false;
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
        #[expect(
            clippy::cast_precision_loss,
            reason = "surface px are < 2^24, exact in f32"
        )]
        let pin_task = self.offer_window_height(target as f32);
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
        let now = std::time::Instant::now();
        if geometry::window_pin_due(self.pin_applied, now, height) {
            self.window_height = Some(height);
            // The spawn-grid settle's adoption arm (status-glance
            // criterion 1): the granted stream just settled, so any
            // terminal still waiting records its resting grid on the
            // next resize — the draw at this adopted height. A no-op
            // on settled records, so a re-show's adoption can never
            // absorb a live deviation into the record.
            for tab in &self.tabs {
                tab.terminal.lock().unwrap().arm_spawn_grid_next_resize();
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

    /// The surface's current width in px: the value both the drawer
    /// width policy (render + drag) and the height drag target
    /// against. 480 while no output has reported a size — the same
    /// fallback the drawer's first frame renders at.
    pub(super) fn current_surface_width(&self) -> f32 {
        self.output_size.map_or(480.0, |(width, _)| {
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
        let now = std::time::Instant::now();
        if let Some(last) = self.last_toggle
            && now.duration_since(last) < TOGGLE_DEBOUNCE
        {
            return Task::none();
        }
        self.last_toggle = Some(now);

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
                tabs::remote_target_index(self.tabs.len(), tab_index, self.active_tab)
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
        match toggle_step(action, &self.surface, self.config.slide_animation_enabled) {
            ToggleStep::Measure => self.show(),
            ToggleStep::DestroyNow(_) | ToggleStep::SlideOut(_) => self.hide(),
            ToggleStep::ReverseShow(id) => self.reverse_show(id),
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
            &self.surface,
            self.config.slide_animation_enabled,
        ) {
            ToggleStep::Measure => self.show(),
            ToggleStep::ReverseShow(id) => self.reverse_show(id),
            _ => Task::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_edit_mid_slide_skips_the_in_place_reshape() {
        // Frozen-geometry boundary: an edit arriving while the
        // content slides must not fire `set_size` on the animating
        // surface — the edit lands on the next show instead.
        assert!(!config_edit_reshapes(true, false, true));
    }

    #[test]
    fn geometry_edit_at_rest_reshapes_in_place() {
        assert!(config_edit_reshapes(true, false, false));
    }

    #[test]
    fn geometry_edit_mid_drag_still_defers_to_release() {
        assert!(!config_edit_reshapes(true, true, false));
    }

    #[test]
    fn non_geometry_edit_never_reshapes() {
        assert!(!config_edit_reshapes(false, false, false));
    }

    #[test]
    fn drag_and_slide_together_still_skip_the_reshape() {
        assert!(!config_edit_reshapes(true, true, true));
    }

    // --- the grab-bar guard ---
    //
    // `GrabBarDragged` is the one geometry-call site that could run
    // while a slide animates: a slide-in rides a `Shown` surface, so
    // matching `Shown` alone does not exclude it — a press inside the
    // 130 ms window must neither arm a drag nor send `set_size` on
    // the animating surface. With arming gated, `DragApplyDue` and
    // `GrabBarReleased` are covered by construction: they only act on
    // an armed drag, and `hide()` clears any live drag before a
    // slide can start.

    #[test]
    fn grab_bar_drag_at_rest_acts() {
        let id = window::Id::unique();
        assert!(grab_bar_acts_on(&SurfaceState::Shown(id), false));
    }

    #[test]
    fn grab_bar_drag_mid_slide_in_does_not_act() {
        // A slide-in runs in `Shown`, so the surface state alone
        // cannot gate the drag — the guard needs the slide's
        // existence too.
        let id = window::Id::unique();
        assert!(!grab_bar_acts_on(&SurfaceState::Shown(id), true));
    }

    #[test]
    fn grab_bar_drag_ignores_every_non_shown_state() {
        // Pinned: a `Hiding` surface rejects drag messages (no
        // interleave with an out-slide or its reversal);
        // Measuring/Hidden never arm one.
        let id = window::Id::unique();
        assert!(!grab_bar_acts_on(&SurfaceState::Hiding(id), false));
        assert!(!grab_bar_acts_on(&SurfaceState::Measuring(id), false));
        assert!(!grab_bar_acts_on(&SurfaceState::Hidden, false));
    }

    // --- the toggle decision matrix ---
    //
    // `toggle_step` decides over intent × surface state; slide
    // activity needs no separate input because it is implied by the
    // state — an `Out` slide only exists while `Hiding`, an `In`
    // slide only rides a `Shown` surface — and the executors read
    // the live slide for their offsets (`hide`, `reverse_show` via
    // `slide::reverse`).

    #[test]
    fn toggle_or_show_from_hidden_runs_the_measuring_pass() {
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Hidden, true),
            ToggleStep::Measure
        );
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Hidden, true),
            ToggleStep::Measure
        );
    }

    #[test]
    fn toggle_or_hide_from_measuring_destroys_immediately() {
        // A measuring surface has no reshaped content to animate —
        // the surface dies now, not in 130 ms.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Measuring(id), true),
            ToggleStep::DestroyNow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Measuring(id), true),
            ToggleStep::DestroyNow(id)
        );
    }

    #[test]
    fn toggle_or_hide_from_shown_slides_out() {
        // Slide out, destroy on completion. Mid-slide-in the same
        // decision applies — the executor starts the out-slide from
        // the live eased offset, so nothing teleports.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Shown(id), true),
            ToggleStep::SlideOut(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Shown(id), true),
            ToggleStep::SlideOut(id)
        );
    }

    #[test]
    fn toggle_or_show_mid_hiding_reverses_on_the_same_surface() {
        // A mid-hide toggle reverses instead of being dropped — and
        // on the SAME window::Id: the live surface is reused (no new
        // measuring pass) and `reverse_show` re-grabs keyboard
        // interactivity on it per config.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Hiding(id), true),
            ToggleStep::ReverseShow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Hiding(id), true),
            ToggleStep::ReverseShow(id)
        );
    }

    #[test]
    fn show_while_measuring_or_shown_is_already_satisfied() {
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Measuring(id), true),
            ToggleStep::Leave
        );
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Shown(id), true),
            ToggleStep::Leave
        );
    }

    #[test]
    fn hide_while_hiding_neither_restarts_nor_double_destroys() {
        // Idempotence carried into the matrix: `WindowUnfocused`
        // reaches `focus_lost` unguarded, and the keyboard release
        // synthesizes its own `Unfocused` — a second hide request
        // must neither restart the out-slide nor queue a second
        // destroy task.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Hiding(id), true),
            ToggleStep::Leave
        );
    }

    #[test]
    fn hide_while_hidden_destroys_nothing_in_the_matrix() {
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Hidden, true),
            ToggleStep::Leave
        );
    }

    #[test]
    fn a_hide_intent_never_decides_to_show() {
        // Pins `hide()`'s unreachable arms as intended behavior: the
        // hide column of the matrix only ever destroys, slides out,
        // or leaves — under either slide-toggle value (the flag
        // swaps the destroy *style*, never the direction).
        let id = window::Id::unique();
        for slide_enabled in [true, false] {
            for surface in [
                SurfaceState::Hidden,
                SurfaceState::Measuring(id),
                SurfaceState::Shown(id),
                SurfaceState::Hiding(id),
            ] {
                assert!(matches!(
                    toggle_step(ToggleAction::Hide, &surface, slide_enabled),
                    ToggleStep::DestroyNow(_) | ToggleStep::SlideOut(_) | ToggleStep::Leave
                ));
            }
        }
    }

    // --- the slide toggle in the toggle matrix ---

    #[test]
    fn shown_hide_with_slides_disabled_destroys_immediately() {
        // The one decision the slide toggle owns: a hide from
        // `Shown` with the key off destroys now — instant hide.
        // Applies to a plain hide and a toggle alike.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Shown(id), false),
            ToggleStep::DestroyNow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Shown(id), false),
            ToggleStep::DestroyNow(id)
        );
    }

    #[test]
    fn hiding_reversal_ignores_the_slide_toggle() {
        // A running out-slide is never aborted by its own toggle: a
        // show intent mid-hide still reverses on the live surface
        // with the key off. The toggle gates transition ENTRY only;
        // `toggle_step` takes no mid-flight input by construction.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Hiding(id), false),
            ToggleStep::ReverseShow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Hiding(id), false),
            ToggleStep::ReverseShow(id)
        );
    }

    #[test]
    fn measuring_and_hidden_cells_ignore_the_slide_toggle() {
        // Only the Shown-hide decision keys on the flag: a measuring
        // surface always dies immediately (no content to animate),
        // hidden always measures, and a disabled hide while hidden
        // still leaves.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Measuring(id), false),
            ToggleStep::DestroyNow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Hidden, false),
            ToggleStep::Measure
        );
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Hidden, false),
            ToggleStep::Leave
        );
    }

    #[test]
    fn layer_unfocused_while_hiding_is_rejected_by_the_shown_guard() {
        // Only a shown surface reacts to losing keyboard focus —
        // during `Hiding` (the synthesized unfocus of the hide's own
        // keyboard release) the event must be a no-op.
        let id = window::Id::unique();
        assert!(layer_unfocused_acts_on(&SurfaceState::Shown(id), id));
        assert!(!layer_unfocused_acts_on(&SurfaceState::Hiding(id), id));
        assert!(!layer_unfocused_acts_on(
            &SurfaceState::Shown(window::Id::unique()),
            id
        ));
    }

    #[test]
    fn focus_loss_hides_when_configured() {
        // The ordinary path: with `hide_on_focus_loss` on, losing
        // keyboard focus hides. No settings-window input is needed:
        // while that window is open no shown surface exists (opening
        // it hides the terminal; a toggle while it is open closes it
        // first), so the gate never runs in the settings-open state.
        assert!(focus_loss_hides(true));
    }

    #[test]
    fn focus_loss_never_hides_when_focus_loss_hiding_is_off() {
        // `hide_on_focus_loss` off means no focus-loss hide, period —
        // the gate only ever narrows the hide, never widens it.
        assert!(!focus_loss_hides(false));
    }

    #[test]
    fn explicit_hides_never_consult_the_focus_loss_gate() {
        // The gate covers the automatic focus-loss path only:
        // explicit hide intents (the toggle keybind's `toggle_step`,
        // `Action::Hide`) take no gate input and never pass through
        // `focus_loss_hides`. Asserted side by side: the gate's most
        // restrictive input yields no hide, yet the explicit hide
        // step still steps its hide.
        assert!(!focus_loss_hides(false));
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Shown(id), true),
            ToggleStep::SlideOut(id)
        );
    }

    #[test]
    fn resize_from_the_tracked_surface_feeds_the_pin() {
        // The granted-size stream that pins the terminal height
        // belongs to the layer surface alone: every state tracking
        // that surface accepts its resizes.
        let id = window::Id::unique();
        assert!(resize_acts_on(&SurfaceState::Measuring(id), id));
        assert!(resize_acts_on(&SurfaceState::Shown(id), id));
        assert!(resize_acts_on(&SurfaceState::Hiding(id), id));
    }

    #[test]
    fn resize_from_a_foreign_window_never_feeds_the_pin() {
        // A foreign window (the clipboard anchor) must never pin the
        // terminal's height; a hidden surface tracks no window at
        // all.
        let surface_id = window::Id::unique();
        let settings_id = window::Id::unique();
        assert!(!resize_acts_on(
            &SurfaceState::Shown(surface_id),
            settings_id
        ));
        assert!(!resize_acts_on(&SurfaceState::Hidden, settings_id));
        assert!(!resize_acts_on(&SurfaceState::Hidden, surface_id));
    }
}
