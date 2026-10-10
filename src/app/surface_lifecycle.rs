// SPDX-License-Identifier: GPL-3.0-only

use cosmic::iced::window;

use super::ToggleAction;
use crate::geometry;

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
