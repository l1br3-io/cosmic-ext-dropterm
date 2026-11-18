// SPDX-License-Identifier: GPL-3.0-only

//! The drop-down slide animation's model: the offset/easing/reversal
//! math as plain functions over plain data (shared-core spec, moved
//! with the surface decision layer that carries a live slide in its
//! state). The window-space half — the `SlideBox` wrapper that clips
//! and translates the content column at draw time — stays with the
//! frontend that renders it; offsets here are edge-space, `0.0` is
//! the resting position, and the window-space sign is applied only at
//! the draw site.

/// Duration of one slide in milliseconds: mid-range of the 120–150 ms
/// band, ease-out on show / ease-in on hide. A compile-time constant —
/// not user-configurable.
pub const SLIDE_DURATION_MS: u64 = 130;

/// Which way a slide is moving: `In` shows the content (offset decays
/// to rest), `Out` hides it (offset grows to the distance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Showing: ease-out from `from_offset_px` to rest.
    In,
    /// Hiding: ease-in from `from_offset_px` to `distance_px`.
    Out,
}

/// A slide in progress: plain data, no clocks — elapsed time is
/// injected at the call site. Offsets are edge-space: `0.0` is the
/// resting position, growing toward `distance_px` as the content
/// hides; the window-space sign is applied only at the draw site.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Active {
    pub direction: Direction,
    /// Where this slide started, captured at start or reversal.
    pub from_offset_px: f32,
    /// The fully-hidden offset — the slide distance,
    /// `geometry::surface_height(output.h, height_percent)`, fixed at
    /// `SurfaceOpened` time.
    pub distance_px: f32,
}

/// The content's edge-space offset `elapsed_ms` into `active`: eased
/// from `from_offset_px` toward the direction's endpoint over exactly
/// [`SLIDE_DURATION_MS`] — quadratic ease-out for [`Direction::In`]
/// (fast in, settling at rest), quadratic ease-in for
/// [`Direction::Out`] (leaving gently, accelerating off-screen). At or
/// past completion the endpoint is returned *directly*, not
/// recomputed — `from + (distance - from)` is not float-exact, and
/// the completed pose must be byte-identical to the resting (In) or
/// fully-hidden (Out) one. Pure over injected milliseconds: the
/// frontend's draw path feeds the wall clock at frame presentation;
/// tests feed fakes.
pub fn offset(active: &Active, elapsed_ms: u64) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "progress ratio only: f32 precision at any elapsed value still clamps into [0, 1]"
    )]
    let progress = (elapsed_ms as f32 / SLIDE_DURATION_MS as f32).clamp(0.0, 1.0);
    if progress >= 1.0 {
        return match active.direction {
            Direction::In => 0.0,
            Direction::Out => active.distance_px,
        };
    }
    match active.direction {
        Direction::In => active.from_offset_px * (1.0 - progress).powi(2),
        Direction::Out => {
            active.from_offset_px + (active.distance_px - active.from_offset_px) * progress.powi(2)
        }
    }
}

/// Whether `active` has run its full [`SLIDE_DURATION_MS`]:
/// time-based on purpose — the endpoint offset at completion is
/// already exact, and parking on the first tick past the duration is
/// a clock decision, not a pose one. Monotone in `elapsed_ms`: a
/// slide completes exactly once.
pub fn finished(_active: &Active, elapsed_ms: u64) -> bool {
    elapsed_ms >= SLIDE_DURATION_MS
}

/// Re-target `active` from wherever it stands at `elapsed_ms`:
/// capture the current offset as the new `from_offset_px`, flip the
/// direction, keep the distance, and run a fresh fixed
/// [`SLIDE_DURATION_MS`] — the re-target is not proportional to the
/// remaining distance. No teleport: the reversed slide's frame-0
/// offset equals the old slide's offset at the reversal instant.
pub fn reverse(active: &Active, elapsed_ms: u64) -> Active {
    Active {
        direction: match active.direction {
            Direction::In => Direction::Out,
            Direction::Out => Direction::In,
        },
        from_offset_px: offset(active, elapsed_ms),
        distance_px: active.distance_px,
    }
}

/// The slide-in a freshly shown surface starts: the full surface
/// height is both the from-offset and the distance, pinned at
/// `SurfaceOpened` time. `None` when `slide_animation_enabled` is
/// off: the surface opens at rest, byte-identically to the unwrapped
/// column.
///
/// The toggle is honored at transition *entry* only — this function
/// takes no mid-flight state, so a running slide can never be aborted
/// by its own toggle: it always runs to completion (or reverses
/// through [`reverse`], which equally ignores the flag).
pub fn show_slide(enabled: bool, distance_px: f32) -> Option<Active> {
    enabled.then_some(Active {
        direction: Direction::In,
        from_offset_px: distance_px,
        distance_px,
    })
}

/// A live slide: the animation state ([`Active`]) paired with its
/// *true* start instant — carried through view rebuilds, never
/// re-captured. The pair exists so the frontend's draw path can
/// evaluate the pose against the frame's own clock: the runtime
/// builds the view in its `AboutToWait` pass but presents the frame
/// on the compositor's frame callback, so an offset baked at view
/// time is up to a full frame stale at presentation — random
/// per-frame error that reads as judder. Evaluating at draw makes
/// every presented frame carry the pose for its presentation instant.
#[derive(Debug, Clone, Copy)]
pub struct Playing {
    /// When the slide started (the app's slide clock).
    pub started: std::time::Instant,
    /// What slides, from where, toward which endpoint.
    pub active: Active,
    /// Which slide instance this is — the machine's monotone counter,
    /// bumped on every start and re-target. The frontend folds it into
    /// the animation element's id: GPUI keys the animation element's
    /// per-element state on the id, and a oneshot element that already
    /// flipped `done` at its own mount+duration never schedules another
    /// frame — an epoch-keyed id remounts the driver fresh on every
    /// re-target instead of riding a finished one.
    pub epoch: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Direction::In (show): quadratic ease-out from -> rest ---

    #[test]
    fn show_starts_at_the_from_offset_exactly() {
        let active = Active {
            direction: Direction::In,
            from_offset_px: 835.0,
            distance_px: 835.0,
        };
        assert_eq!(offset(&active, 0), 835.0);
    }

    #[test]
    fn show_rests_exactly_at_completion() {
        let active = Active {
            direction: Direction::In,
            from_offset_px: 835.0,
            distance_px: 835.0,
        };
        assert_eq!(offset(&active, SLIDE_DURATION_MS), 0.0);
        // A draw after the parking tick clamps to rest, never
        // overshoots.
        assert_eq!(offset(&active, SLIDE_DURATION_MS * 10), 0.0);
    }

    #[test]
    fn show_decays_monotonically_from_full_and_mid_offsets() {
        // Full slide and a reversed-in mid-offset: both only ever
        // move toward 0.0, sampled finer than the tick so the curve,
        // not the cadence, is under test.
        for from in [835.0, 412.5] {
            let active = Active {
                direction: Direction::In,
                from_offset_px: from,
                distance_px: 835.0,
            };
            let mut last = from;
            for ms in (0..=SLIDE_DURATION_MS).step_by(8) {
                let now = offset(&active, ms);
                assert!(
                    now <= last,
                    "offset grew at {ms} ms from {from}: {now} > {last}"
                );
                assert!(
                    now >= 0.0,
                    "offset flipped sign at {ms} ms from {from}: {now}"
                );
                last = now;
            }
        }
    }

    #[test]
    fn show_midpoint_quarters_the_offset() {
        // Quadratic ease-out at half the duration: (1 - 1/2)^2 = 1/4.
        let active = Active {
            direction: Direction::In,
            from_offset_px: 800.0,
            distance_px: 800.0,
        };
        assert_eq!(offset(&active, SLIDE_DURATION_MS / 2), 200.0);
    }

    // --- Direction::Out (hide): quadratic ease-in from -> distance ---

    #[test]
    fn hide_starts_at_the_from_offset_exactly() {
        let active = Active {
            direction: Direction::Out,
            from_offset_px: 0.0,
            distance_px: 835.0,
        };
        assert_eq!(offset(&active, 0), 0.0);
    }

    #[test]
    fn hide_reaches_the_distance_exactly_at_completion() {
        // No sub-pixel residue: from + (distance - from) * 1.0 is not
        // float-exact in general, so completion must return the
        // endpoint directly.
        let active = Active {
            direction: Direction::Out,
            from_offset_px: 0.1,
            distance_px: 835.7,
        };
        assert_eq!(offset(&active, SLIDE_DURATION_MS), 835.7);
        assert_eq!(offset(&active, SLIDE_DURATION_MS * 10), 835.7);
    }

    #[test]
    fn hide_grows_monotonically_toward_the_distance() {
        for from in [0.0, 412.5] {
            let active = Active {
                direction: Direction::Out,
                from_offset_px: from,
                distance_px: 835.0,
            };
            let mut last = from;
            for ms in (0..=SLIDE_DURATION_MS).step_by(8) {
                let now = offset(&active, ms);
                assert!(
                    now >= last,
                    "offset fell at {ms} ms from {from}: {now} < {last}"
                );
                assert!(
                    now <= 835.0,
                    "offset overshot the distance at {ms} ms: {now}"
                );
                last = now;
            }
        }
    }

    #[test]
    fn hide_midpoint_covers_a_quarter_of_the_gap() {
        // Quadratic ease-in at half the duration: (1/2)^2 = 1/4 of
        // the from->distance gap.
        let active = Active {
            direction: Direction::Out,
            from_offset_px: 0.0,
            distance_px: 800.0,
        };
        assert_eq!(offset(&active, SLIDE_DURATION_MS / 2), 200.0);
    }

    // --- finished ---

    #[test]
    fn finished_flips_exactly_once_at_the_duration() {
        let active = Active {
            direction: Direction::In,
            from_offset_px: 835.0,
            distance_px: 835.0,
        };
        assert!(!finished(&active, 0));
        assert!(!finished(&active, SLIDE_DURATION_MS - 1));
        assert!(finished(&active, SLIDE_DURATION_MS));
        assert!(finished(&active, SLIDE_DURATION_MS * 10));
        // Monotone in elapsed: once finished, always finished.
        for ms in 0..=SLIDE_DURATION_MS {
            assert_eq!(
                finished(&active, ms),
                ms >= SLIDE_DURATION_MS,
                "finished not monotone at {ms} ms"
            );
        }
    }

    // --- reverse ---

    #[test]
    fn reverse_starts_exactly_where_the_old_slide_stood() {
        // No teleport: the reversed slide's frame-0 offset equals the
        // old slide's offset at the reversal instant (exact float
        // equality — the same value is stored and returned).
        let showing = Active {
            direction: Direction::In,
            from_offset_px: 835.0,
            distance_px: 835.0,
        };
        let reversed = reverse(&showing, 40);
        assert_eq!(offset(&reversed, 0), offset(&showing, 40));

        let hiding = Active {
            direction: Direction::Out,
            from_offset_px: 0.0,
            distance_px: 835.0,
        };
        let reversed = reverse(&hiding, 72);
        assert_eq!(offset(&reversed, 0), offset(&hiding, 72));
    }

    #[test]
    fn reverse_flips_direction_and_preserves_distance() {
        let showing = Active {
            direction: Direction::In,
            from_offset_px: 835.0,
            distance_px: 835.0,
        };
        let reversed = reverse(&showing, 40);
        assert_eq!(reversed.direction, Direction::Out);
        assert_eq!(reversed.distance_px, 835.0);

        let hiding = Active {
            direction: Direction::Out,
            from_offset_px: 0.0,
            distance_px: 835.0,
        };
        let reversed = reverse(&hiding, 40);
        assert_eq!(reversed.direction, Direction::In);
        assert_eq!(reversed.distance_px, 835.0);
    }

    #[test]
    fn reversed_slide_completes_exactly_once_on_its_own_clock() {
        // The re-target is a fresh fixed 130 ms — measured from the
        // reversal, not the original start.
        let showing = Active {
            direction: Direction::In,
            from_offset_px: 835.0,
            distance_px: 835.0,
        };
        let reversed = reverse(&showing, 40);
        assert!(!finished(&reversed, SLIDE_DURATION_MS - 1));
        assert!(finished(&reversed, SLIDE_DURATION_MS));
    }

    #[test]
    fn reverse_at_rest_targets_the_full_distance() {
        // Reversing a completed show (at rest, offset 0) hides from
        // rest.
        let showing = Active {
            direction: Direction::In,
            from_offset_px: 835.0,
            distance_px: 835.0,
        };
        let reversed = reverse(&showing, SLIDE_DURATION_MS);
        assert_eq!(offset(&reversed, 0), 0.0);
        assert_eq!(reversed.direction, Direction::Out);
    }

    // --- the slide toggle at transition entry ---

    #[test]
    fn show_slide_starts_the_full_height_slide_in_when_enabled() {
        // Day-one equality: enabled, the show path starts exactly the
        // slide `SurfaceOpened` always started — the full surface
        // height as both the from-offset and the distance.
        assert_eq!(
            show_slide(true, 835.0),
            Some(Active {
                direction: Direction::In,
                from_offset_px: 835.0,
                distance_px: 835.0,
            })
        );
    }

    #[test]
    fn show_slide_starts_nothing_when_disabled() {
        // The toggle is honored at transition entry: a disabled show
        // opens at rest — no slide, the column renders
        // byte-identically to the unwrapped one.
        assert_eq!(show_slide(false, 835.0), None);
    }
}
