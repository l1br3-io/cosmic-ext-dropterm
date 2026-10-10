use super::*;

const OUTPUT: (u32, u32) = (1920, 1080);

fn vertical_of(edge: Edge) -> Anchor {
    match edge {
        Edge::Top => Anchor::TOP,
        Edge::Bottom => Anchor::BOTTOM,
    }
}

#[test]
fn height_percent_maps_to_pixels_with_rounding() {
    assert_eq!(surface_height(1440.0, 40), 576);
    assert_eq!(surface_height(1440.0, 100), 1440);
}

#[test]
fn height_never_rounds_below_one_pixel() {
    assert_eq!(surface_height(1440.0, 0), 1);
}

#[test]
fn width_percent_maps_to_pixels_with_rounding() {
    assert_eq!(surface_width(1920.0, 100), 1920);
    assert_eq!(surface_width(1000.0, 60), 600);
    assert_eq!(surface_width(1440.0, 33), 475);
}

#[test]
fn width_never_rounds_below_one_pixel() {
    assert_eq!(surface_width(1920.0, 0), 1);
}

#[test]
fn full_width_keeps_both_anchors_and_zero_margins_whatever_the_alignment() {
    // At full width the alignment is moot — both anchors, zero
    // margins.
    for edge in [Edge::Top, Edge::Bottom] {
        for h_align in [HAlign::Left, HAlign::Center, HAlign::Right] {
            let geom = Geometry {
                edge,
                height_percent: 40,
                width_percent: 100,
                h_align,
            };
            let (anchor, margins) = anchor_margins(geom, OUTPUT);
            assert_eq!(anchor, vertical_of(edge) | Anchor::LEFT | Anchor::RIGHT);
            assert_eq!(margins, Margins::default());
        }
    }
}

#[test]
fn left_alignment_drops_the_right_anchor_below_full_width() {
    for edge in [Edge::Top, Edge::Bottom] {
        let geom = Geometry {
            edge,
            height_percent: 40,
            width_percent: 60,
            h_align: HAlign::Left,
        };
        let (anchor, margins) = anchor_margins(geom, OUTPUT);
        assert_eq!(anchor, vertical_of(edge) | Anchor::LEFT);
        assert_eq!(margins, Margins::default());
    }
}

#[test]
fn right_alignment_drops_the_left_anchor_below_full_width() {
    for edge in [Edge::Top, Edge::Bottom] {
        let geom = Geometry {
            edge,
            height_percent: 40,
            width_percent: 60,
            h_align: HAlign::Right,
        };
        let (anchor, margins) = anchor_margins(geom, OUTPUT);
        assert_eq!(anchor, vertical_of(edge) | Anchor::RIGHT);
        assert_eq!(margins, Margins::default());
    }
}

#[test]
fn center_alignment_keeps_both_anchors_with_equal_margins() {
    // 1920 * 60% = 1152 px, leaving (1920 - 1152) / 2 = 384 per side.
    for edge in [Edge::Top, Edge::Bottom] {
        let geom = Geometry {
            edge,
            height_percent: 40,
            width_percent: 60,
            h_align: HAlign::Center,
        };
        let (anchor, margins) = anchor_margins(geom, OUTPUT);
        assert_eq!(anchor, vertical_of(edge) | Anchor::LEFT | Anchor::RIGHT);
        assert_eq!(margins.left, 384);
        assert_eq!(margins.right, 384);
        assert_eq!(margins.top, 0);
        assert_eq!(margins.bottom, 0);
    }
}

#[test]
fn center_margin_split_floors_left_and_keeps_total_width_exact() {
    // 1001 * 41% = 410.41 -> 410 px, leaving 591 px of slack: 295
    // left (floor), 296 right, so left + width + right == 1001.
    let geom = Geometry {
        edge: Edge::Top,
        height_percent: 40,
        width_percent: 41,
        h_align: HAlign::Center,
    };
    let (_, margins) = anchor_margins(geom, (1001, 800));
    assert_eq!(margins.left, 295);
    assert_eq!(margins.right, 296);
    let width = surface_width(1001.0, 41);
    #[expect(
        clippy::cast_sign_loss,
        reason = "fixture margins are non-negative px by construction"
    )]
    let total_width = margins.left as u32 + width + margins.right as u32;
    assert_eq!(total_width, 1001);
}

#[test]
fn full_width_requests_compositor_fill_height_stays_fixed() {
    // Width None at 100% means *fill*, not *unchanged*; height is
    // always a fixed pixel request.
    let geom = Geometry {
        edge: Edge::Top,
        height_percent: 40,
        width_percent: 100,
        h_align: HAlign::Center,
    };
    assert_eq!(requested_size(geom, OUTPUT), (None, 432));
}

#[test]
fn narrow_width_requests_fixed_pixels() {
    let geom = Geometry {
        edge: Edge::Top,
        height_percent: 40,
        width_percent: 60,
        h_align: HAlign::Left,
    };
    assert_eq!(requested_size(geom, OUTPUT), (Some(1152), 432));
}

#[test]
fn geometry_changed_is_false_for_identical_geometry() {
    let geom = Geometry::from_config(&DropTermConfig::default());
    assert!(!geometry_changed(geom, geom));
}

#[test]
fn geometry_changed_fires_for_each_geometry_field() {
    let base = Geometry {
        edge: Edge::Top,
        height_percent: 40,
        width_percent: 60,
        h_align: HAlign::Left,
    };
    let mutations = [
        Geometry {
            edge: Edge::Bottom,
            ..base
        },
        Geometry {
            height_percent: 41,
            ..base
        },
        Geometry {
            width_percent: 61,
            ..base
        },
        Geometry {
            h_align: HAlign::Right,
            ..base
        },
    ];
    for mutated in mutations {
        assert!(
            geometry_changed(base, mutated),
            "expected a change for {mutated:?}"
        );
    }
}

#[test]
fn geometry_changed_ignores_non_geometry_config_fields() {
    // Font, opacity, scrollback, … edits must not reshape the
    // surface: only the four geometry fields count.
    let old = Geometry::from_config(&DropTermConfig::default());
    let edited = DropTermConfig {
        font_name: "Fira Code".to_string(),
        font_size: 20,
        opacity: 50,
        scrollback_lines: 1_000,
        ..DropTermConfig::default()
    };
    let new = Geometry::from_config(&edited);
    assert!(!geometry_changed(old, new));
}

#[test]
fn geometry_from_config_clamps_into_range() {
    let raw = DropTermConfig {
        height_percent: 200,
        width_percent: 5,
        ..DropTermConfig::default()
    };
    let geom = Geometry::from_config(&raw);
    assert_eq!(geom.height_percent, crate::config::MAX_HEIGHT_PERCENT);
    assert_eq!(geom.width_percent, crate::config::MIN_WIDTH_PERCENT);
    assert_eq!(geom.edge, Edge::Top);
    assert_eq!(geom.h_align, HAlign::Center);
}

#[test]
fn live_drag_tracks_the_pointer_pixel_by_pixel_instead_of_percent_jumps() {
    // 1080p output anchored at 40 % (432 px): one pixel of pointer
    // travel must move the surface one pixel; percent quantization
    // happens once, at settle.
    let drag = DragAnchor {
        anchored_px: 432,
        applied_px: 432,
        press_y: 432.0,
    };
    assert_eq!(drag_target_px(Edge::Top, 1080, drag, 433.0), 433);
    assert_eq!(drag_target_px(Edge::Top, 1080, drag, 434.0), 434);
    assert_eq!(drag_target_px(Edge::Top, 1080, drag, 435.0), 435);
    assert_eq!(percent_from_px(1080, 435), 40);
}

#[test]
fn the_arming_move_targets_the_height_the_drag_started_at() {
    // Arming must not jump the surface: the target at the pointer's
    // arming y is the height the surface already has, whichever
    // edge — and wherever inside the bar the grab happened.
    let top = DragAnchor {
        anchored_px: 432,
        applied_px: 432,
        press_y: 432.0,
    };
    assert_eq!(drag_target_px(Edge::Top, 1080, top, 432.0), 432);
    let bottom = DragAnchor {
        anchored_px: 400,
        applied_px: 400,
        press_y: 5.0,
    };
    assert_eq!(drag_target_px(Edge::Bottom, 1000, bottom, 5.0), 400);
}

#[test]
fn top_edge_targets_track_absolute_pointer_travel() {
    // The window frame of a top-anchored surface is fixed, so the
    // target is anchored height plus absolute travel since the
    // arming y: +100 px of travel is +100 px of height, −150 px is
    // −150 px.
    let drag = DragAnchor {
        anchored_px: 432,
        applied_px: 432,
        press_y: 432.0,
    };
    assert_eq!(drag_target_px(Edge::Top, 1080, drag, 532.0), 532);
    assert_eq!(drag_target_px(Edge::Top, 1080, drag, 282.0), 282);
}

#[test]
fn bottom_edge_tracks_repeated_window_ys_through_frame_shifts() {
    // A bottom-anchored surface grows upward, so its window origin
    // moves up under the pointer with every applied resize: 5 px
    // drags report window ys [5, 0, 0, 0] once the frame follows
    // each apply. Retargeting from the height actually applied
    // tracks every pixel.
    let mut drag = DragAnchor {
        anchored_px: 400,
        applied_px: 400,
        press_y: 5.0,
    };
    let mut targets = Vec::new();
    for y in [5.0, 0.0, 0.0, 0.0] {
        let target = drag_target_px(Edge::Bottom, 1000, drag, y);
        drag.applied_px = target;
        targets.push(target);
    }
    assert_eq!(targets, vec![400, 405, 410, 415]);
}

#[test]
fn drag_targets_clamp_to_the_band_in_pixels() {
    // However violent the drag, the pixel target stays inside the
    // 10–100 % band of the output.
    let drag = DragAnchor {
        anchored_px: 432,
        applied_px: 432,
        press_y: 0.0,
    };
    assert_eq!(drag_target_px(Edge::Top, 1080, drag, 100_000.0), 1080);
    assert_eq!(drag_target_px(Edge::Bottom, 1080, drag, 100_000.0), 108);
}

#[test]
fn drag_targets_never_yield_a_zero_height_surface() {
    // On a tiny output the band floor is a couple of pixels; a
    // toward-edge drag must still target a positive height (the
    // `max(1px)` rule).
    let drag = DragAnchor {
        anchored_px: 2,
        applied_px: 2,
        press_y: 0.0,
    };
    let target = drag_target_px(Edge::Top, 20, drag, -1_000.0);
    assert_eq!(target, 2);
    assert!(target >= 1);
}

#[test]
fn settled_percent_rounds_once_from_pixels() {
    // 405 px of 1000 is 40.5 % -> 41 (half away from zero); 404 px
    // is 40.4 % -> 40.
    assert_eq!(percent_from_px(1000, 405), 41);
    assert_eq!(percent_from_px(1000, 404), 40);
}

#[test]
fn settled_percent_clamps_to_the_band() {
    assert_eq!(percent_from_px(1000, 5_000), MAX_HEIGHT_PERCENT);
    assert_eq!(percent_from_px(1000, 0), MIN_HEIGHT_PERCENT);
}

#[test]
fn the_first_move_of_a_drag_applies_immediately() {
    // No apply yet in this drag: the arming move must land with no
    // interval wait, or every grab would feel dead.
    let now = Instant::now();
    assert!(drag_apply_due(None, now, 500));
}

#[test]
fn unchanged_targets_skip_the_apply() {
    // A re-report of the position the surface already has (sub-pixel
    // move, or a synthetic echo) must not re-send the size triple.
    let at = Instant::now();
    let now = at + DRAG_APPLY_INTERVAL + Duration::from_millis(1);
    assert!(!drag_apply_due(Some((at, 500)), now, 500));
}

#[test]
fn small_target_changes_wait_for_the_deadband() {
    // Sub-deadband changes are held back even when the interval has
    // fully elapsed: each apply costs the double-reported resize
    // pipeline, so sub-deadband drift skips redundant applies and
    // the surface simply sits at most a deadband behind the hand.
    let at = Instant::now();
    let now = at + DRAG_APPLY_INTERVAL + Duration::from_millis(1);
    assert!(!drag_apply_due(Some((at, 500)), now, 510));
}

#[test]
fn deadband_sized_changes_still_wait_for_the_interval() {
    let at = Instant::now();
    let now = at + Duration::from_millis(10);
    assert!(!drag_apply_due(Some((at, 500)), now, 530));
}

#[test]
fn deadband_sized_changes_apply_once_the_interval_elapses() {
    let at = Instant::now();
    let now = at + DRAG_APPLY_INTERVAL;
    assert!(drag_apply_due(Some((at, 500)), now, 530));
}

#[test]
fn the_first_granted_size_adopts_immediately() {
    // Before any adoption there is nothing to debounce: the
    // terminal pin must exist for the first layout.
    let now = Instant::now();
    assert!(window_pin_due(None, now, 500.0));
}

#[test]
fn re_reported_heights_adopt_for_free() {
    // The runtime double-reports every size change and the
    // compositor re-reports eased steps: re-offering the height
    // the pin already has must never wait out the interval.
    let at = Instant::now();
    let now = at + Duration::from_millis(1);
    assert!(window_pin_due(Some((at, 500.0)), now, 500.0));
}

#[test]
fn changed_heights_within_the_interval_wait() {
    // The eased-convergence storm bound: intermediate granted
    // sizes adopt at most once per interval.
    let at = Instant::now();
    let now = at + Duration::from_millis(20);
    assert!(!window_pin_due(Some((at, 500.0)), now, 505.0));
}

#[test]
fn changed_heights_adopt_once_the_interval_elapses() {
    let at = Instant::now();
    let now = at + WINDOW_PIN_INTERVAL;
    assert!(window_pin_due(Some((at, 500.0)), now, 505.0));
}

#[test]
fn release_schedules_one_write_due_at_its_generation() {
    // Release -> window -> write: the settled percent is captured
    // when the drag is released — so a hide/show inside the window
    // cannot change what lands — and exactly the `WriteDue`
    // carrying the scheduled generation consumes it.
    let mut write_back = WriteBack::default();
    let pending = write_back.schedule(42);
    assert_eq!(pending.height_percent, 42);
    assert_eq!(write_back.due(pending.generation), Some(pending));
    assert_eq!(write_back.due(pending.generation), None);
}

#[test]
fn newer_release_cancels_the_stale_write() {
    // A second drag settling inside the window reschedules: the
    // first generation's `WriteDue` goes stale and must not write
    // the first drag's percent (rapid re-drags coalesce into the
    // newest settle).
    let mut write_back = WriteBack::default();
    let stale = write_back.schedule(42);
    let current = write_back.schedule(50);
    assert_ne!(stale.generation, current.generation);
    assert_eq!(write_back.due(stale.generation), None);
    assert_eq!(
        write_back
            .due(current.generation)
            .map(|pending| pending.height_percent),
        Some(50)
    );
}

#[test]
fn bare_write_cancel_takes_the_pending_and_ages_out_its_due() {
    // Newest intent wins: a bare settings write of the height key
    // supersedes a pending drag write-back,
    // so the canceled generation's `WriteDue` must find nothing and
    // the drag's percent must never land after the bare write's.
    let mut write_back = WriteBack::default();
    let pending = write_back.schedule(42);
    assert!(write_back.has_pending());
    assert_eq!(write_back.cancel_pending(), Some(pending));
    assert!(!write_back.has_pending());
    assert_eq!(write_back.due(pending.generation), None);
}

#[test]
fn drag_release_after_a_cancel_schedules_and_lands_last() {
    // The other half of newest-intent-wins: after a bare write
    // canceled a settle, a fresh drag
    // release schedules a new generation whose `WriteDue` lands —
    // the grab bar stays usable the whole time the settings window
    // is open, so both paths genuinely interleave.
    let mut write_back = WriteBack::default();
    let canceled = write_back.schedule(42);
    write_back.cancel_pending();
    let landed = write_back.schedule(50);
    assert_ne!(landed.generation, canceled.generation);
    assert_eq!(
        write_back
            .due(landed.generation)
            .map(|pending| pending.height_percent),
        Some(50)
    );
}

#[test]
fn cancel_with_nothing_pending_is_inert() {
    // Cancelling with no pending write must not disturb the
    // machine: the next schedule works exactly as before.
    let mut write_back = WriteBack::default();
    assert_eq!(write_back.cancel_pending(), None);
    let pending = write_back.schedule(60);
    assert_eq!(write_back.due(pending.generation), Some(pending));
}

#[test]
fn own_write_echo_matches_the_written_geometry() {
    let written = Geometry {
        edge: Edge::Top,
        height_percent: 42,
        width_percent: 80,
        h_align: HAlign::Center,
    };
    assert!(is_own_write(written, written));
}

#[test]
fn external_edit_does_not_match_the_own_write_record() {
    // Any of the four geometry fields differing means the
    // `ConfigChanged` is someone else's write (a human editing the
    // file): absorb nothing and reshape live.
    let written = Geometry {
        edge: Edge::Top,
        height_percent: 42,
        width_percent: 80,
        h_align: HAlign::Center,
    };
    for edited in [
        Geometry {
            edge: Edge::Bottom,
            ..written
        },
        Geometry {
            height_percent: 43,
            ..written
        },
        Geometry {
            width_percent: 81,
            ..written
        },
        Geometry {
            h_align: HAlign::Left,
            ..written
        },
    ] {
        assert!(
            !is_own_write(written, edited),
            "{edited:?} must not match {written:?}"
        );
    }
}
