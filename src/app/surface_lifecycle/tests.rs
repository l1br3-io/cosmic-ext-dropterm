// SPDX-License-Identifier: GPL-3.0-only

//! The surface lifecycle's decision tests: the pure guards, the
//! toggle matrix, and the machine executors' intent sequences — all
//! over the toolkit-neutral types (`SurfaceHandle`, never
//! `window::Id`), so the whole lifecycle is testable without a
//! runtime.

use super::*;
use crate::config::{Edge, HAlign};

/// Inputs of a stay-open, sliding, exclusive surface at half height
/// on the test output — the ordinary configuration.
fn plain_inputs(slide_enabled: bool) -> SurfaceInputs {
    SurfaceInputs {
        geometry: Geometry {
            edge: Edge::Top,
            height_percent: 50,
            width_percent: 100,
            h_align: HAlign::Center,
        },
        keyboard_mode: KeyboardMode::Exclusive,
        hide_on_focus_loss: true,
        slide_animation_enabled: slide_enabled,
    }
}

/// A machine resting hidden, with a measured output beside it for the
/// paths that read pixels.
fn hidden_machine(output: Option<(u32, u32)>) -> SurfaceMachine {
    SurfaceMachine {
        output_size: output,
        ..SurfaceMachine::default()
    }
}

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
// 130 ms window must neither arm a drag nor send `set_size` on the
// animating surface. With arming gated, `DragApplyDue` and
// `GrabBarReleased` are covered by construction: they only act on
// an armed drag, and `hide()` clears any live drag before a
// slide can start.

#[test]
fn grab_bar_drag_at_rest_acts() {
    let handle = SurfaceHandle::next();
    assert!(grab_bar_acts_on(&SurfaceState::Shown(handle), false));
}

#[test]
fn grab_bar_drag_mid_slide_in_does_not_act() {
    // A slide-in runs in `Shown`, so the surface state alone
    // cannot gate the drag — the guard needs the slide's
    // existence too.
    let handle = SurfaceHandle::next();
    assert!(!grab_bar_acts_on(&SurfaceState::Shown(handle), true));
}

#[test]
fn grab_bar_drag_ignores_every_non_shown_state() {
    // Pinned: a `Hiding` surface rejects drag messages (no
    // interleave with an out-slide or its reversal);
    // Measuring/Hidden never arm one.
    let handle = SurfaceHandle::next();
    assert!(!grab_bar_acts_on(&SurfaceState::Hiding(handle), false));
    assert!(!grab_bar_acts_on(&SurfaceState::Measuring(handle), false));
    assert!(!grab_bar_acts_on(&SurfaceState::Hidden, false));
}

// --- the toggle decision matrix ---
//
// `toggle_step` decides over intent × surface state; slide
// activity needs no separate input because it is implied by the
// state — an `Out` slide only exists while `Hiding`, an `In`
// slide only rides a `Shown` surface — and the executors read
// the live slide for their offsets (`decide_hide`,
// `decide_reverse_show` via `slide::reverse`).

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
    let handle = SurfaceHandle::next();
    assert_eq!(
        toggle_step(ToggleAction::Toggle, &SurfaceState::Measuring(handle), true),
        ToggleStep::DestroyNow(handle)
    );
    assert_eq!(
        toggle_step(ToggleAction::Hide, &SurfaceState::Measuring(handle), true),
        ToggleStep::DestroyNow(handle)
    );
}

#[test]
fn toggle_or_hide_from_shown_slides_out() {
    // Slide out, destroy on completion. Mid-slide-in the same
    // decision applies — the executor starts the out-slide from
    // the live eased offset, so nothing teleports.
    let handle = SurfaceHandle::next();
    assert_eq!(
        toggle_step(ToggleAction::Toggle, &SurfaceState::Shown(handle), true),
        ToggleStep::SlideOut(handle)
    );
    assert_eq!(
        toggle_step(ToggleAction::Hide, &SurfaceState::Shown(handle), true),
        ToggleStep::SlideOut(handle)
    );
}

#[test]
fn toggle_or_show_mid_hiding_reverses_on_the_same_surface() {
    // A mid-hide toggle reverses instead of being dropped — and
    // on the SAME handle: the live surface is reused (no new
    // measuring pass) and the reversal re-holds keyboard
    // interactivity on it per config.
    let handle = SurfaceHandle::next();
    assert_eq!(
        toggle_step(ToggleAction::Toggle, &SurfaceState::Hiding(handle), true),
        ToggleStep::ReverseShow(handle)
    );
    assert_eq!(
        toggle_step(ToggleAction::Show, &SurfaceState::Hiding(handle), true),
        ToggleStep::ReverseShow(handle)
    );
}

#[test]
fn show_while_measuring_or_shown_is_already_satisfied() {
    let handle = SurfaceHandle::next();
    assert_eq!(
        toggle_step(ToggleAction::Show, &SurfaceState::Measuring(handle), true),
        ToggleStep::Leave
    );
    assert_eq!(
        toggle_step(ToggleAction::Show, &SurfaceState::Shown(handle), true),
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
    let handle = SurfaceHandle::next();
    assert_eq!(
        toggle_step(ToggleAction::Hide, &SurfaceState::Hiding(handle), true),
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
    let handle = SurfaceHandle::next();
    for slide_enabled in [true, false] {
        for surface in [
            SurfaceState::Hidden,
            SurfaceState::Measuring(handle),
            SurfaceState::Shown(handle),
            SurfaceState::Hiding(handle),
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
    let handle = SurfaceHandle::next();
    assert_eq!(
        toggle_step(ToggleAction::Hide, &SurfaceState::Shown(handle), false),
        ToggleStep::DestroyNow(handle)
    );
    assert_eq!(
        toggle_step(ToggleAction::Toggle, &SurfaceState::Shown(handle), false),
        ToggleStep::DestroyNow(handle)
    );
}

#[test]
fn hiding_reversal_ignores_the_slide_toggle() {
    // A running out-slide is never aborted by its own toggle: a
    // show intent mid-hide still reverses on the live surface
    // with the key off. The toggle gates transition ENTRY only;
    // `toggle_step` takes no mid-flight input by construction.
    let handle = SurfaceHandle::next();
    assert_eq!(
        toggle_step(ToggleAction::Show, &SurfaceState::Hiding(handle), false),
        ToggleStep::ReverseShow(handle)
    );
    assert_eq!(
        toggle_step(ToggleAction::Toggle, &SurfaceState::Hiding(handle), false),
        ToggleStep::ReverseShow(handle)
    );
}

#[test]
fn measuring_and_hidden_cells_ignore_the_slide_toggle() {
    // Only the Shown-hide decision keys on the flag: a measuring
    // surface always dies immediately (no content to animate),
    // hidden always measures, and a disabled hide while hidden
    // still leaves.
    let handle = SurfaceHandle::next();
    assert_eq!(
        toggle_step(ToggleAction::Hide, &SurfaceState::Measuring(handle), false),
        ToggleStep::DestroyNow(handle)
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
    let handle = SurfaceHandle::next();
    assert!(layer_unfocused_acts_on(
        &SurfaceState::Shown(handle),
        handle
    ));
    assert!(!layer_unfocused_acts_on(
        &SurfaceState::Hiding(handle),
        handle
    ));
    assert!(!layer_unfocused_acts_on(
        &SurfaceState::Shown(SurfaceHandle::next()),
        handle
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
    let handle = SurfaceHandle::next();
    assert_eq!(
        toggle_step(ToggleAction::Hide, &SurfaceState::Shown(handle), true),
        ToggleStep::SlideOut(handle)
    );
}

#[test]
fn resize_from_the_shown_and_hiding_surface_feeds_the_pin() {
    // The granted-size stream that pins the terminal height
    // belongs to the layer surface alone: the settled states
    // tracking that surface accept its resizes.
    let handle = SurfaceHandle::next();
    assert!(resize_acts_on(&SurfaceState::Shown(handle), handle));
    assert!(resize_acts_on(&SurfaceState::Hiding(handle), handle));
}

#[test]
fn resize_while_measuring_never_feeds_the_pin() {
    // The measuring surface is the probe that exists to READ
    // the output size — stretched full-output by design, never
    // a geometry the terminal should adopt. Its sizes must
    // never reach the pin: adopting them grows the grid once
    // per toggle, and every grown line returns as scrollback
    // rot on the shrink (toggle-grid-stability).
    let handle = SurfaceHandle::next();
    assert!(!resize_acts_on(&SurfaceState::Measuring(handle), handle));
}

#[test]
fn resize_from_a_foreign_window_never_feeds_the_pin() {
    // A foreign window (the clipboard anchor) must never pin the
    // terminal's height; a hidden surface tracks no window at
    // all.
    let surface_handle = SurfaceHandle::next();
    let foreign = SurfaceHandle::next();
    assert!(!resize_acts_on(
        &SurfaceState::Shown(surface_handle),
        foreign
    ));
    assert!(!resize_acts_on(&SurfaceState::Hidden, foreign));
    assert!(!resize_acts_on(&SurfaceState::Hidden, surface_handle));
}

// --- the machine executors' intent sequences ---
//
// The decision layer returns intents (never `cosmic::app::Task`),
// so its tests assert the intent sequences directly — the
// recording executor of the full-lifecycle test (Task 4) replays
// these same decisions through a real executor seam.

#[test]
fn show_from_hidden_measures_with_the_resolved_keyboard_mode() {
    // The intent carries the mode already resolved for a shown
    // surface — the map-time focus grant — and the machine moves
    // to Measuring on the fresh handle.
    let mut machine = hidden_machine(Some((1920, 1080)));
    let intents = machine.decide_show(plain_inputs(true));
    let Some(SurfaceIntent::Measure { handle, keyboard }) = intents.first() else {
        panic!("the show decision measures");
    };
    assert_eq!(*keyboard, KeyboardMode::Exclusive);
    assert_eq!(machine.state, SurfaceState::Measuring(*handle));
    assert!(!machine.focused);
}

#[test]
fn show_from_a_stay_open_config_measures_as_on_demand() {
    // The resolved mode of a stay-open surface (`hide_on_focus_loss`
    // off): `Exclusive` is never held while the surface stays open —
    // the intent must carry the downgraded `OnDemand`.
    let mut machine = hidden_machine(Some((1920, 1080)));
    let mut inputs = plain_inputs(true);
    inputs.hide_on_focus_loss = false;
    let intents = machine.decide_show(inputs);
    assert!(matches!(
        intents.as_slice(),
        [SurfaceIntent::Measure {
            keyboard: KeyboardMode::OnDemand,
            ..
        }]
    ));
}

#[test]
fn show_while_visible_leaves_the_surface_alone() {
    // A measuring or shown surface is already on its way: the
    // show decision emits nothing.
    let handle = SurfaceHandle::next();
    for state in [SurfaceState::Measuring(handle), SurfaceState::Shown(handle)] {
        let mut machine = SurfaceMachine {
            state,
            ..SurfaceMachine::default()
        };
        assert!(machine.decide_show(plain_inputs(true)).is_empty());
    }
}

#[test]
fn hide_from_shown_releases_the_keyboard_and_slides_out() {
    // The out-slide's start: keyboard releases now (the previously
    // focused window regains focus while the slide plays), the
    // state moves to Hiding with the out-slide armed — the destroy
    // lands on the slide's completion, not here.
    let handle = SurfaceHandle::next();
    let mut machine = SurfaceMachine {
        state: SurfaceState::Shown(handle),
        output_size: Some((1920, 1080)),
        ..SurfaceMachine::default()
    };
    let intents = machine.decide_hide(plain_inputs(true));
    assert_eq!(
        intents,
        vec![SurfaceIntent::Hold {
            handle,
            keyboard: KeyboardHold::Released,
        }]
    );
    assert_eq!(machine.state, SurfaceState::Hiding(handle));
    let playing = machine.slide.expect("the out-slide is armed");
    assert_eq!(playing.active.direction, slide::Direction::Out);
    assert_eq!(playing.active.from_offset_px, 0.0);
    // Half the 1080-px output at 50%: the deterministic distance.
    assert_eq!(playing.active.distance_px, 540.0);
}

#[test]
fn hide_from_shown_with_slides_off_destroys_now() {
    // Instant hide: no slide, the state rests Hidden at once.
    let handle = SurfaceHandle::next();
    let mut machine = SurfaceMachine {
        state: SurfaceState::Shown(handle),
        output_size: Some((1920, 1080)),
        slide: Some(slide::Playing {
            started: std::time::Instant::now(),
            active: slide::Active {
                direction: slide::Direction::In,
                from_offset_px: 100.0,
                distance_px: 540.0,
            },
        }),
        ..SurfaceMachine::default()
    };
    let intents = machine.decide_hide(plain_inputs(false));
    assert_eq!(intents, vec![SurfaceIntent::Destroy { handle }]);
    assert_eq!(machine.state, SurfaceState::Hidden);
    assert!(machine.slide.is_none(), "the destroy-now clears the slide");
}

#[test]
fn hide_from_measuring_destroys_the_probe() {
    // A measuring surface has no reshaped content to animate: it
    // dies now, never in 130 ms.
    let handle = SurfaceHandle::next();
    let mut machine = SurfaceMachine {
        state: SurfaceState::Measuring(handle),
        ..SurfaceMachine::default()
    };
    assert_eq!(
        machine.decide_hide(plain_inputs(true)),
        vec![SurfaceIntent::Destroy { handle }]
    );
    assert_eq!(machine.state, SurfaceState::Hidden);
}

#[test]
fn hide_while_hidden_or_hiding_leaves_nothing() {
    // Idempotence: no restart, no double destroy.
    let handle = SurfaceHandle::next();
    let mut hidden = hidden_machine(None);
    assert!(hidden.decide_hide(plain_inputs(true)).is_empty());
    let mut hiding = SurfaceMachine {
        state: SurfaceState::Hiding(handle),
        ..SurfaceMachine::default()
    };
    assert!(hiding.decide_hide(plain_inputs(true)).is_empty());
    assert_eq!(hiding.state, SurfaceState::Hiding(handle));
}

#[test]
fn reverse_show_reholds_keyboard_and_focuses_the_input() {
    // The reversal on the same live surface: the resolved hold
    // plus the terminal input focus, the state back to Shown, the
    // slide re-targeted In from wherever it stands.
    let handle = SurfaceHandle::next();
    let mut machine = SurfaceMachine {
        state: SurfaceState::Hiding(handle),
        output_size: Some((1920, 1080)),
        slide: Some(slide::Playing {
            started: std::time::Instant::now(),
            active: slide::Active {
                direction: slide::Direction::Out,
                from_offset_px: 0.0,
                distance_px: 540.0,
            },
        }),
        ..SurfaceMachine::default()
    };
    let intents = machine.decide_reverse_show(handle, plain_inputs(true));
    assert_eq!(
        intents,
        vec![
            SurfaceIntent::Hold {
                handle,
                keyboard: KeyboardHold::Exclusive,
            },
            SurfaceIntent::Focus { handle },
        ]
    );
    assert_eq!(machine.state, SurfaceState::Shown(handle));
    assert!(!machine.focused, "the latch resets pending LayerFocused");
    let playing = machine.slide.expect("the slide re-targets");
    assert_eq!(playing.active.direction, slide::Direction::In);
}

#[test]
fn focus_loss_reports_unfocus_only_from_shown() {
    // Unfocus events trailing our own hide (`Hiding`/`Hidden` by
    // the time they arrive) must not reach the PTY, or a
    // focus-reporting program repaints its prompt per toggle.
    let handle = SurfaceHandle::next();
    let mut shown = SurfaceMachine {
        state: SurfaceState::Shown(handle),
        ..SurfaceMachine::default()
    };
    let (report, _) = shown.decide_focus_loss(plain_inputs(true));
    assert!(report);
    let mut hiding = SurfaceMachine {
        state: SurfaceState::Hiding(handle),
        ..SurfaceMachine::default()
    };
    let (report, _) = hiding.decide_focus_loss(plain_inputs(true));
    assert!(!report);
}

#[test]
fn focus_loss_hides_only_when_configured_and_focused() {
    // The gate plus the latch: a focus loss with the config off
    // leaves the latch armed; with it on, an unfocused surface
    // (a stray unfocus before any focus, possible with
    // `OnDemand`) never closes a freshly-opened surface.
    let handle = SurfaceHandle::next();
    let mut machine = SurfaceMachine {
        state: SurfaceState::Shown(handle),
        focused: true,
        ..SurfaceMachine::default()
    };
    let (_, hides) = machine.decide_focus_loss(plain_inputs(true));
    assert!(hides, "configured and focused: the hide fires");
    assert!(!machine.focused, "the latch cleared with the hide");

    let mut unfocused = SurfaceMachine {
        state: SurfaceState::Shown(handle),
        focused: false,
        ..SurfaceMachine::default()
    };
    let (_, hides) = unfocused.decide_focus_loss(plain_inputs(true));
    assert!(!hides, "no focus ever latched: no hide");
    assert!(
        !unfocused.focused,
        "a non-hiding loss leaves the latch as it was"
    );

    let mut configured_off = SurfaceMachine {
        state: SurfaceState::Shown(handle),
        focused: true,
        ..SurfaceMachine::default()
    };
    let mut inputs = plain_inputs(true);
    inputs.hide_on_focus_loss = false;
    let (_, hides) = configured_off.decide_focus_loss(inputs);
    assert!(!hides, "the config off: no hide however focused");
    assert!(configured_off.focused, "the latch stays armed");
}

#[test]
fn toggles_inside_the_debounce_window_are_dropped() {
    // The debounce collapses compositor double-delivery and true
    // spam; the state machine itself never drops or queues.
    let mut machine = hidden_machine(None);
    assert!(machine.toggle_accepted(std::time::Instant::now()));
    assert!(!machine.toggle_accepted(std::time::Instant::now()));
}
