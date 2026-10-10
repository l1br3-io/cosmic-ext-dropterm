use super::*;

#[test]
fn readable_foreground_is_dark_on_light_panes() {
    // The light COSMIC scheme's lavender background.
    let fg = readable_foreground(Color::from_rgba8(0xCC, 0xD0, 0xDE, 1.0));
    assert!(
        (fg.r + fg.g + fg.b) / 3.0 < 0.2,
        "expected near-black, got {fg:?}"
    );
}

#[test]
fn readable_foreground_is_light_on_dark_panes() {
    // A dark scheme background.
    let fg = readable_foreground(Color::from_rgba8(0x1B, 0x1B, 0x22, 1.0));
    assert!(
        (fg.r + fg.g + fg.b) / 3.0 > 0.8,
        "expected near-white, got {fg:?}"
    );
}

#[test]
fn pane_colors_derives_text_from_the_stored_pane_background() {
    // Drive the process-global with a known light pane (the COSMIC
    // light scheme's lavender) so the assertions are deterministic.
    let saved = WINDOW_BG_COLOR.swap(0xAACC_D0DE, std::sync::atomic::Ordering::SeqCst);
    let (strip, fg) = pane_colors();
    WINDOW_BG_COLOR.store(saved, std::sync::atomic::Ordering::SeqCst);

    let pane = Color::from_rgba8(0xCC, 0xD0, 0xDE, 1.0);
    let mean = |c: Color| (c.r + c.g + c.b) / 3.0;
    assert!(
        mean(strip) < mean(pane),
        "strip {strip:?} should be darker than pane {pane:?}"
    );
    assert!(
        mean(fg) < 0.2,
        "expected near-black text on a light pane, got {fg:?}"
    );
}

/// Tests run against a small, easy-to-reason-about metrics set.
const M: BarMetrics = BarMetrics {
    bar_height: 32.0,
    min_button_width: 48.0,
    max_button_width: 200.0,
    button_padding_x: 10.0,
    button_gap: 4.0,
    close_size: 24.0,
    text_gap: 6.0,
};

fn geometry(text_widths: &[f32], available_width: f32) -> Vec<ButtonGeometry> {
    layout_buttons(available_width, text_widths, &M)
}

fn center_of(rect: &Rectangle<f32>) -> Point {
    Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

#[test]
fn layout_places_buttons_left_to_right_with_gaps() {
    let buttons = geometry(&[40.0, 60.0], 1000.0);
    assert_eq!(buttons.len(), 2);

    let w0 = 10.0 + 40.0 + 6.0 + 24.0 + 10.0;
    let w1 = 10.0 + 60.0 + 6.0 + 24.0 + 10.0;
    assert_eq!(buttons[0].bounds.x, 0.0);
    assert_eq!(buttons[0].bounds.width, w0);
    assert_eq!(buttons[1].bounds.x, w0 + 4.0);
    assert_eq!(buttons[1].bounds.width, w1);
}

#[test]
fn layout_buttons_fill_the_bar_height() {
    let buttons = geometry(&[40.0], 1000.0);
    assert_eq!(buttons[0].bounds.y, 0.0);
    assert_eq!(buttons[0].bounds.height, M.bar_height);
}

#[test]
fn layout_anchors_close_button_at_each_buttons_right_edge() {
    let buttons = geometry(&[40.0], 1000.0);
    let button = &buttons[0];
    assert_eq!(button.close.width, M.close_size);
    assert_eq!(button.close.height, M.close_size);
    assert_eq!(
        button.close.x + button.close.width + M.button_padding_x,
        button.bounds.x + button.bounds.width
    );
    assert_eq!(
        button.close.y,
        button.bounds.y + (M.bar_height - M.close_size) / 2.0
    );
}

#[test]
fn layout_clamps_long_titles_to_the_max_button_width() {
    let buttons = geometry(&[500.0], 1000.0);
    assert_eq!(buttons[0].bounds.width, M.max_button_width);
}

#[test]
fn layout_raises_buttons_to_the_min_button_width() {
    // A minimum above the natural padding+close width (50) clamps the
    // narrowest possible button up, so it stays a usable target.
    let metrics = BarMetrics {
        min_button_width: 60.0,
        ..M
    };
    let buttons = layout_buttons(1000.0, &[0.0], &metrics);
    assert_eq!(buttons[0].bounds.width, 60.0);
}

#[test]
fn layout_squeezes_buttons_proportionally_when_the_row_overflows() {
    // Three titles whose natural buttons are 100 wide (300 total, plus
    // 8 of gaps) squeezed into 158: buttons budget 150, scale 0.5, so
    // every button lands at 50 and the row exactly fits.
    let buttons = geometry(&[50.0, 50.0, 50.0], 158.0);
    assert_eq!(buttons.len(), 3);
    assert_eq!(buttons[0].bounds.width, 50.0);
    assert_eq!(buttons[1].bounds.width, 50.0);
    assert_eq!(buttons[2].bounds.width, 50.0);
    assert_eq!(buttons[2].bounds.x + buttons[2].bounds.width, 158.0);
}

#[test]
fn layout_never_squeezes_below_the_min_button_width() {
    // Five tabs that cannot fit: the proportional squeeze hits the
    // floor, so every button keeps its minimum width.
    let buttons = geometry(&[50.0; 5], 100.0);
    for (i, button) in buttons.iter().enumerate() {
        assert_eq!(button.bounds.width, M.min_button_width, "button {i}");
    }
}

#[test]
fn layout_of_zero_tabs_is_empty() {
    assert!(geometry(&[], 500.0).is_empty());
}

#[test]
fn hit_test_lands_on_the_tab_body() {
    let buttons = geometry(&[40.0, 60.0], 1000.0);
    // Inside the first button, left of its close target.
    let point = Point::new(buttons[0].bounds.x + 5.0, 10.0);
    assert_eq!(hit_test(&buttons, point), Some((0, HitTarget::Tab)));
}

#[test]
fn hit_test_lands_on_the_close_button() {
    let buttons = geometry(&[40.0, 60.0], 1000.0);
    assert_eq!(
        hit_test(&buttons, center_of(&buttons[1].close)),
        Some((1, HitTarget::Close))
    );
}

#[test]
fn hit_test_misses_gaps_and_outside_the_row() {
    let buttons = geometry(&[40.0, 60.0], 1000.0);
    // In the gap between the two buttons.
    let gap_x = buttons[0].bounds.x + buttons[0].bounds.width + 2.0;
    assert_eq!(hit_test(&buttons, Point::new(gap_x, 5.0)), None);
    // Left of the first and beyond the last button.
    assert_eq!(hit_test(&buttons, Point::new(-5.0, 5.0)), None);
    assert_eq!(
        hit_test(
            &buttons,
            Point::new(buttons[1].bounds.x + buttons[1].bounds.width + 5.0, 5.0)
        ),
        None
    );
    // Vertically outside the bar.
    assert_eq!(hit_test(&buttons, Point::new(5.0, 100.0)), None);
}

#[test]
fn hit_test_on_an_empty_bar_misses() {
    assert_eq!(hit_test(&[], Point::new(5.0, 5.0)), None);
}

// Corner target geometry, row budget, routing precedence, and the
// tab-button visibility rule.

#[test]
fn settings_target_anchors_at_the_strip_s_right_corner() {
    // The target takes no tab count: the identical right-corner
    // rect for 1, 2, and many tabs.
    let target = settings_target(1000.0, &M);
    assert_eq!(target.width, M.close_size);
    assert_eq!(target.height, M.close_size);
    assert_eq!(target.x + target.width, 1000.0 - M.button_padding_x);
    assert_eq!(target.y, (M.bar_height - M.close_size) / 2.0);
    assert_eq!(target.y * 2.0 + target.height, M.bar_height);
}

#[test]
fn corner_zone_spans_inset_targets_and_gaps() {
    // The zone reserves inset + `+` + gap + gear + gap: both corner
    // targets and the gaps around them.
    assert_eq!(
        corner_zone(&M),
        M.button_padding_x + M.close_size * 2.0 + M.button_gap * 2.0
    );
    assert_eq!(row_budget(1000.0, &M), 1000.0 - corner_zone(&M));
}

#[test]
fn newtab_target_anchors_one_gap_left_of_the_settings_target() {
    // Like the settings target, the `+` takes no tab count: the
    // identical right-corner rect in every tab state — the 1-tab
    // strip carries `+` and gear alone.
    for bar_width in [1000.0, 400.0] {
        let gear = settings_target(bar_width, &M);
        let plus = newtab_target(bar_width, &M);
        assert_eq!(plus.width, M.close_size, "width at {bar_width}");
        assert_eq!(plus.height, M.close_size, "height at {bar_width}");
        assert_eq!(
            plus.x + plus.width,
            gear.x - M.button_gap,
            "one gap left of the gear at {bar_width}"
        );
        assert_eq!(plus.y, gear.y, "vertically centered like the gear");
        assert_eq!(plus.y * 2.0 + plus.height, M.bar_height);
    }
}

#[test]
fn row_budget_clears_both_corner_targets() {
    // A row inside the budget ends at least one gap left of the `+`
    // target, so it can never underlap the `+` or the gear.
    let bar_width = 1000.0;
    let plus = newtab_target(bar_width, &M);
    let budget = row_budget(bar_width, &M);
    assert!(budget <= plus.x - M.button_gap);
    let gear = settings_target(bar_width, &M);
    assert!(plus.x + plus.width < gear.x);
}

#[test]
fn row_budget_never_drops_below_zero() {
    assert_eq!(row_budget(20.0, &M), 0.0);
}

#[test]
fn squeezed_rows_end_exactly_at_the_row_budget() {
    // A bar whose budget leaves 158 px of row: the squeezed row
    // ends exactly at the budget, left of both corner targets.
    let bar_width = 158.0 + corner_zone(&M);
    let buttons = layout_buttons(row_budget(bar_width, &M), &[50.0, 50.0, 50.0], &M);
    let last = buttons.last().unwrap();
    assert_eq!(last.bounds.x + last.bounds.width, 158.0);
}

#[test]
fn floor_overflow_still_routes_the_settings_target_first() {
    // Five at-48 floor buttons geometrically overflow a tiny budget
    // and reach under both corner targets; routing checks the gear
    // ahead of the `+` and the row, so a press inside it is
    // Settings.
    let bar_width = 100.0 + corner_zone(&M);
    let buttons = layout_buttons(row_budget(bar_width, &M), &[50.0; 5], &M);
    let gear = settings_target(bar_width, &M);
    let plus = newtab_target(bar_width, &M);
    let last = buttons.last().unwrap();
    assert!(last.bounds.x + last.bounds.width > gear.x);
    assert_eq!(
        press_target(&buttons, gear, plus, center_of(&gear)),
        Some(PressTarget::Settings)
    );
}

#[test]
fn floor_overflow_routes_the_newtab_target_ahead_of_the_row() {
    // The same floor-overflow row reaching under the `+`: routing
    // checks the gear, then the `+`, then the row.
    let bar_width = 100.0 + corner_zone(&M);
    let buttons = layout_buttons(row_budget(bar_width, &M), &[50.0; 5], &M);
    let gear = settings_target(bar_width, &M);
    let plus = newtab_target(bar_width, &M);
    let last = buttons.last().unwrap();
    assert!(last.bounds.x + last.bounds.width > plus.x);
    assert_eq!(
        press_target(&buttons, gear, plus, center_of(&plus)),
        Some(PressTarget::NewTab)
    );
}

#[test]
fn press_target_routes_settings_newtab_tab_and_close() {
    let bar_width = 1000.0;
    let buttons = layout_buttons(row_budget(bar_width, &M), &[40.0, 60.0], &M);
    let gear = settings_target(bar_width, &M);
    let plus = newtab_target(bar_width, &M);
    assert_eq!(
        press_target(&buttons, gear, plus, center_of(&gear)),
        Some(PressTarget::Settings)
    );
    assert_eq!(
        press_target(&buttons, gear, plus, center_of(&plus)),
        Some(PressTarget::NewTab)
    );
    assert_eq!(
        press_target(
            &buttons,
            gear,
            plus,
            Point::new(buttons[0].bounds.x + 5.0, 10.0)
        ),
        Some(PressTarget::Tab(0, HitTarget::Tab))
    );
    assert_eq!(
        press_target(&buttons, gear, plus, center_of(&buttons[1].close)),
        Some(PressTarget::Tab(1, HitTarget::Close))
    );
    // The zone's gap, now left of the `+` target, hits nothing.
    assert_eq!(
        press_target(&buttons, gear, plus, Point::new(plus.x - 2.0, 10.0)),
        None
    );
}

#[test]
fn tab_buttons_show_only_from_two_tabs_on() {
    // With fewer than two tabs the strip carries the settings
    // button alone: no tab button, no close mark.
    assert!(!shows_tab_buttons(0));
    assert!(!shows_tab_buttons(1));
    assert!(shows_tab_buttons(2));
    assert!(shows_tab_buttons(9));
}

// Gear geometry, tooltip bounds, and the hover-punch composite.

#[test]
fn gear_quads_stay_inside_the_settings_target() {
    let target = settings_target(1000.0, &M);
    let gear = gear_glyph(target);
    let inside = |quad: &Rectangle| {
        target.contains(quad.position())
            && target.contains(Point::new(quad.x + quad.width, quad.y + quad.height))
    };
    assert!(inside(&gear.body), "body escapes: {:?}", gear.body);
    assert!(inside(&gear.punch), "punch escapes: {:?}", gear.punch);
    for (i, tooth) in gear.teeth.iter().enumerate() {
        assert!(inside(tooth), "tooth {i} escapes: {tooth:?}");
    }
}

#[test]
fn gear_teeth_are_rotationally_symmetric() {
    let target = settings_target(1000.0, &M);
    let gear = gear_glyph(target);
    let center = target.center();
    assert_eq!(gear.teeth.len(), GEAR_TEETH);

    // Every tooth is identical and rides the same radius.
    let (side, radius) = {
        let first = &gear.teeth[0];
        let dx = first.center_x() - center.x;
        let dy = first.center_y() - center.y;
        (first.width, (dx * dx + dy * dy).sqrt())
    };
    for (i, tooth) in gear.teeth.iter().enumerate() {
        assert_eq!(tooth.width, side, "tooth {i} width differs");
        assert_eq!(tooth.height, side, "tooth {i} height differs");
        let dx = tooth.center_x() - center.x;
        let dy = tooth.center_y() - center.y;
        assert!(
            ((dx * dx + dy * dy).sqrt() - radius).abs() < 1e-4,
            "tooth {i} rides a different radius"
        );
    }

    // Equal angular spacing about the target's center: N-fold.
    #[expect(
        clippy::cast_precision_loss,
        reason = "GEAR_TEETH = 8, so usize -> f32 is exact"
    )]
    let step = std::f32::consts::TAU / GEAR_TEETH as f32;
    let angles: Vec<f32> = gear
        .teeth
        .iter()
        .map(|tooth| (tooth.center_y() - center.y).atan2(tooth.center_x() - center.x))
        .collect();
    for (i, &angle) in angles.iter().enumerate() {
        #[expect(
            clippy::cast_precision_loss,
            reason = "i indexes teeth (< GEAR_TEETH = 8); usize -> f32 is exact"
        )]
        let expected = angles[0] + step * i as f32;
        let drift = (angle - expected).rem_euclid(std::f32::consts::TAU);
        assert!(
            !(1e-4..=std::f32::consts::TAU - 1e-4).contains(&drift),
            "tooth {i} breaks the angular spacing (drift {drift})"
        );
    }
}

#[test]
fn plus_glyph_crosses_at_the_targets_center_and_stays_inside() {
    let target = newtab_target(1000.0, &M);
    let plus = plus_glyph(target);
    let center = target.center();

    // Containment: both bars stay inside the target.
    for (name, bar) in [("horizontal", plus.horizontal), ("vertical", plus.vertical)] {
        assert!(
            target.contains(bar.position())
                && target.contains(Point::new(bar.x + bar.width, bar.y + bar.height)),
            "{name} bar escapes the target: {bar:?}"
        );
    }

    // Symmetry: both bars cross exactly at the target's center,
    // with equal arm lengths and a shared thickness.
    let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
    assert!(close(plus.horizontal.center_x(), center.x));
    assert!(close(plus.horizontal.center_y(), center.y));
    assert!(close(plus.vertical.center_x(), center.x));
    assert!(close(plus.vertical.center_y(), center.y));
    assert_eq!(plus.horizontal.width, plus.vertical.height);
    assert_eq!(plus.horizontal.height, plus.vertical.width);
    assert!(plus.horizontal.height < plus.horizontal.width);
}

// The profile dropdown: row model, bounds, and the dismissal
// classification — all pure, space-agnostic (a rect and a point
// in one space).

fn profile(name: &str) -> crate::profile::Profile {
    crate::profile::Profile {
        name: name.to_string(),
        command: "htop".to_string(),
        cwd: None,
        color_scheme: None,
        env: None,
        font_size: None,
        scrollback_lines: None,
    }
}

#[test]
fn dropdown_rows_is_empty_without_profiles() {
    // Zero profiles: no dropdown at all — the `+` stays a plain
    // new-tab button (the context menu's flat-menu ruling).
    assert!(dropdown_rows(&[]).is_empty());
}

#[test]
fn dropdown_rows_lead_with_the_plain_row_then_profiles_in_order() {
    // OQ-1's standing composition: row 0 is the plain New Tab —
    // the default spawn — then one row per profile in config
    // order (the context-menu submenu's composition).
    let profiles = [profile("ssh-web"), profile("htop")];
    let rows = dropdown_rows(&profiles);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].label, "New Tab");
    assert_eq!(rows[0].profile, None);
    assert_eq!(rows[1].label, "ssh-web");
    assert_eq!(rows[1].profile, Some(0));
    assert_eq!(rows[2].label, "htop");
    assert_eq!(rows[2].profile, Some(1));
}

#[test]
fn dropdown_bounds_pin_the_right_edge_and_grow_leftward() {
    // The tooltip rule: the right edge is the strip's corner
    // whatever the widest label, growth entirely leftward — the
    // dropdown can never overflow the surface's right edge.
    let narrow = dropdown_bounds(1000.0, 60.0, 3, &M, TooltipSide::Below);
    let wide = dropdown_bounds(1000.0, 200.0, 3, &M, TooltipSide::Below);
    assert_eq!(narrow.x + narrow.width, 1000.0);
    assert_eq!(wide.x + wide.width, 1000.0);
    assert_eq!(narrow.width, 60.0 + DROPDOWN_PADDING_X * 2.0);
    assert!(wide.x < narrow.x);
}

#[test]
fn dropdown_bounds_stack_rows_on_the_row_height() {
    let bounds = dropdown_bounds(1000.0, 60.0, 3, &M, TooltipSide::Below);
    assert_eq!(bounds.height, 3.0 * DROPDOWN_ROW_HEIGHT);
}

#[test]
fn dropdown_bounds_clear_the_strip_toward_the_terminal_side() {
    // Below: the dropdown starts one gap under the strip; Above:
    // it ends one gap over it — never over the strip itself.
    let below = dropdown_bounds(1000.0, 60.0, 3, &M, TooltipSide::Below);
    assert_eq!(below.y, M.bar_height + DROPDOWN_GAP);
    let above = dropdown_bounds(1000.0, 60.0, 3, &M, TooltipSide::Above);
    assert_eq!(above.y + above.height, -DROPDOWN_GAP);
    assert_eq!(above.width, below.width);
}

#[test]
fn dropdown_row_at_lands_on_each_row_in_turn() {
    let dropdown = dropdown_bounds(1000.0, 60.0, 3, &M, TooltipSide::Below);
    for index in 0..3 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "index < 3, so usize -> f32 is exact"
        )]
        let point = Point::new(
            dropdown.x + 5.0,
            dropdown.y + DROPDOWN_ROW_HEIGHT * index as f32 + 5.0,
        );
        assert_eq!(dropdown_row_at(dropdown, point), Some(index));
    }
}

#[test]
fn dropdown_row_at_classifies_the_dismissal_presses() {
    // Away-press classification: outside the rect — below the
    // last row, beside the dropdown, above it — closes.
    let dropdown = dropdown_bounds(1000.0, 60.0, 3, &M, TooltipSide::Below);
    let beyond_last = Point::new(
        dropdown.x + 5.0,
        dropdown.y + dropdown.height + DROPDOWN_GAP,
    );
    assert_eq!(dropdown_row_at(dropdown, beyond_last), None);
    assert_eq!(
        dropdown_row_at(dropdown, Point::new(dropdown.x - 5.0, dropdown.y + 5.0)),
        None
    );
    assert_eq!(
        dropdown_row_at(dropdown, Point::new(dropdown.x + 5.0, dropdown.y - 5.0)),
        None
    );
    // A point exactly at the bottom edge is outside (half-open).
    let bottom_edge = Point::new(dropdown.x + 5.0, dropdown.y + dropdown.height);
    assert_eq!(dropdown_row_at(dropdown, bottom_edge), None);
}

#[test]
fn tooltip_bounds_anchor_the_right_edge_and_grow_leftward() {
    // The gear tip anchors at the strip's right corner whatever
    // the label's width — growth is entirely leftward, so the tip
    // can never overflow the corner it anchors to.
    let narrow = tooltip_bounds(1000.0, 60.0, &M, TooltipSide::Below);
    let wide = tooltip_bounds(1000.0, 200.0, &M, TooltipSide::Below);
    assert_eq!(narrow.x + narrow.width, 1000.0);
    assert_eq!(wide.x + wide.width, 1000.0);
    assert_eq!(narrow.width, 60.0 + TOOLTIP_PADDING_X * 2.0);
    assert!(wide.x < narrow.x);
    assert_eq!(wide.x, 1000.0 - wide.width);
}

#[test]
fn tooltip_bounds_anchor_the_newtab_tip_at_its_targets_right_edge() {
    // The `+` tip anchors at the `+` target's right edge — inboard
    // of the strip's corner, growth entirely leftward so it can
    // never run under the gear.
    let bar_width = 1000.0;
    let plus = newtab_target(bar_width, &M);
    let right_edge = plus.x + plus.width;
    let tip = tooltip_bounds(right_edge, 60.0, &M, TooltipSide::Below);
    assert_eq!(tip.x + tip.width, right_edge);
    assert!(tip.x + tip.width < bar_width);
    let wide = tooltip_bounds(right_edge, 200.0, &M, TooltipSide::Below);
    assert!(wide.x < tip.x);
    assert_eq!(wide.x + wide.width, right_edge);
}

#[test]
fn tooltip_bounds_stay_inside_the_bar_for_the_real_label() {
    // "Settings…" at the title font size measures well under 100 px;
    // even at that generous width the tip fits the strip.
    let tip = tooltip_bounds(1000.0, 100.0, &M, TooltipSide::Below);
    assert!(tip.x >= 0.0);
    assert!(tip.x + tip.width <= 1000.0);
}

#[test]
fn tooltip_bounds_clear_the_strip_toward_the_terminal_side() {
    let below = tooltip_bounds(1000.0, 60.0, &M, TooltipSide::Below);
    assert_eq!(below.y, M.bar_height + TOOLTIP_GAP);
    let above = tooltip_bounds(1000.0, 60.0, &M, TooltipSide::Above);
    assert_eq!(above.y + above.height, -TOOLTIP_GAP);
    assert_eq!(above.height, below.height);
    assert_eq!(above.width, below.width);
}

#[test]
fn composite_matches_its_blend_endpoints() {
    let base = Color::from_rgba8(0x20, 0x40, 0x60, 1.0);
    let over = Color::from_rgba8(0xFF, 0x00, 0x80, 1.0);
    let c = composite(Color { a: 0.0, ..over }, base);
    assert!((c.r - base.r).abs() < 1e-6);
    assert!((c.g - base.g).abs() < 1e-6);
    assert!((c.b - base.b).abs() < 1e-6);
    let c = composite(over, base);
    assert!((c.r - over.r).abs() < 1e-6);
    assert!((c.g - over.g).abs() < 1e-6);
    let c = composite(Color { a: 0.5, ..over }, base);
    assert!((c.r - (over.r * 0.5 + base.r * 0.5)).abs() < 1e-6);
}

#[test]
fn drop_index_left_of_the_row_drops_first() {
    let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
    assert_eq!(drop_index(&buttons, 0, -10.0), 0);
}

#[test]
fn drop_index_holding_a_neighbors_left_half_keeps_the_slot() {
    let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
    // x = 100 sits inside button 1, left of its center (149): the
    // cursor has not passed any other button's center yet.
    assert_eq!(drop_index(&buttons, 0, 100.0), 0);
}

#[test]
fn drop_index_counts_only_the_neighbors_passed() {
    let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
    // Past button 1's center (149) but not button 2's (253).
    assert_eq!(drop_index(&buttons, 0, 180.0), 1);
    // Past every other button's center.
    assert_eq!(drop_index(&buttons, 0, 260.0), 2);
}

#[test]
fn drop_index_skips_the_dragged_button_itself() {
    let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
    // Dragging the middle tab from within its own span: only button 0
    // was passed, so the drop slot still holds its own position.
    assert_eq!(drop_index(&buttons, 1, 180.0), 1);
    // Past button 2's center, the middle tab drops last.
    assert_eq!(drop_index(&buttons, 1, 260.0), 2);
}

#[test]
fn drop_index_beyond_the_row_drops_last() {
    let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
    assert_eq!(drop_index(&buttons, 0, 10_000.0), 2);
}

#[test]
fn displaced_row_holds_every_button_home_when_the_drop_slot_is_its_own() {
    // Dragging the middle tab without passing a neighbor: the row
    // it leaves behind is the row itself.
    let widths = [90.0, 110.0, 90.0];
    let (targets, indicator) = displaced_row(&widths, 1, 1, 4.0);
    assert_eq!(targets, vec![Some(0.0), None, Some(208.0)]);
    // The dragged tab's own slot starts at 94; the indicator sits
    // in the gap before its left edge.
    assert_eq!(indicator, 94.0 - 2.0);
}

#[test]
fn displaced_row_shifts_followers_left_when_the_drag_moves_right() {
    let widths = [90.0, 110.0, 90.0];
    // Dragging tab 0 past tab 1: tab 1 takes the row's head, and
    // tab 2 — already past the swap — keeps its slot.
    let (targets, _) = displaced_row(&widths, 0, 1, 4.0);
    assert_eq!(targets, vec![None, Some(0.0), Some(208.0)]);
}

#[test]
fn displaced_row_appends_the_dragged_tab_at_the_rows_new_end() {
    let widths = [90.0, 110.0, 90.0];
    // Dragging tab 0 last: the displaced row ends at 204, so the
    // indicator lands half a gap past it.
    let (targets, indicator) = displaced_row(&widths, 0, 2, 4.0);
    assert_eq!(targets, vec![None, Some(0.0), Some(114.0)]);
    assert_eq!(indicator, 114.0 + 90.0 + 2.0);
}

#[test]
fn displaced_row_precedes_the_row_when_dropping_first() {
    let widths = [90.0, 110.0, 90.0];
    let (_, indicator) = displaced_row(&widths, 1, 0, 4.0);
    assert_eq!(indicator, 0.0 - 2.0);
}

#[test]
fn followed_x_tracks_the_cursor_at_the_grab_offset() {
    assert_eq!(followed_x(1000.0, 100.0, 300.0, 30.0), 270.0);
}

#[test]
fn followed_x_clamps_to_the_row() {
    // Neither edge of the followed button may leave the row.
    assert_eq!(followed_x(300.0, 100.0, 10_000.0, 30.0), 200.0);
    assert_eq!(followed_x(300.0, 100.0, -50.0, 30.0), 0.0);
}

#[test]
fn drop_from_followed_decides_by_the_button_not_the_cursor() {
    let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
    // Button 0 spans 0..90, button 1's center is at 149. Grabbed
    // 80 into button 0, the cursor at 155 has crossed button 1's
    // center — but the followed button's center (120) has not, so
    // the slot holds.
    assert_eq!(drop_from_followed(&buttons, 0, 155.0, 80.0), 0);
    assert_eq!(drop_index(&buttons, 0, 155.0), 1);
    // Deeper in, the button's own center passes 149 and the drop
    // advances.
    assert_eq!(drop_from_followed(&buttons, 0, 240.0, 80.0), 1);
}

#[test]
fn slot_slides_settle_on_their_target_and_stay_put() {
    let mut slot = SlotAnimation::new(50.0);
    slot.retarget(120.0);
    assert!(slot.is_animating());
    // Let the slide run out; it settles exactly on its target.
    let deadline = Instant::now() + DRAG_ANIMATION + Duration::from_millis(50);
    while slot.is_animating() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
        slot.settle();
    }
    assert!(!slot.is_animating());
    assert_eq!(slot.current(), 120.0);
    // Retargeting a settled slot to the same spot restarts nothing.
    slot.retarget(120.0);
    assert!(!slot.is_animating());
    assert_eq!(slot.current(), 120.0);
}

#[test]
fn slot_retarget_mid_slide_starts_from_where_it_paints() {
    let mut slot = SlotAnimation::new(0.0);
    slot.retarget(100.0);
    assert!(slot.is_animating());
    // Mid-slide, retarget back toward the start: the new slide
    // begins from wherever the first one currently paints, not from
    // either endpoint.
    slot.retarget(0.0);
    assert!(slot.is_animating());
    let current = slot.current();
    assert!(
        current > 0.0 && current < 100.0,
        "retarget should start mid-slide, got {current}"
    );
}

// --- the per-tab context menu's rows (the consolidation surface) ---

fn menu_profile(name: &str) -> Profile {
    Profile {
        name: name.to_owned(),
        command: format!("run {name}"),
        cwd: None,
        color_scheme: None,
        env: None,
        font_size: None,
        scrollback_lines: None,
    }
}

/// The rows for a middle tab of five, no profiles: the flat
/// composition every positional test derives from.
fn flat_rows() -> Vec<TabMenuRow> {
    tab_menu_rows(&[], 5, 2, false)
}

fn labels(rows: &[TabMenuRow]) -> Vec<&str> {
    rows.iter().map(|row| row.label.as_str()).collect()
}

#[test]
fn tab_menu_rows_lead_with_new_tab_then_the_tab_cluster() {
    // New Tab first (every peer's tab menu opens with it), then
    // the clicked-tab cluster: Rename, Duplicate, Pin/Unpin.
    assert_eq!(
        labels(&flat_rows()),
        vec![
            "New Tab",
            "Rename Tab",
            "Duplicate Tab",
            "Pin Tab",
            "Move Tab Left",
            "Move Tab Right",
            "Close Tab",
            "Close Other Tabs",
            "Close Tabs to the Right",
        ]
    );
}

#[test]
fn tab_menu_rows_route_every_enabled_row_to_its_pick() {
    let expected = [
        ("New Tab", TabMenuPick::NewTab),
        ("Rename Tab", TabMenuPick::Rename),
        ("Duplicate Tab", TabMenuPick::Duplicate),
        ("Pin Tab", TabMenuPick::Pin),
        ("Move Tab Left", TabMenuPick::Move(MoveDirection::Left)),
        ("Move Tab Right", TabMenuPick::Move(MoveDirection::Right)),
        ("Close Tab", TabMenuPick::Close),
        ("Close Other Tabs", TabMenuPick::CloseOthers),
        ("Close Tabs to the Right", TabMenuPick::CloseRight),
    ];
    let rows = flat_rows();
    for (label, pick) in expected {
        let row = rows
            .iter()
            .find(|row| row.label == label)
            .unwrap_or_else(|| panic!("{label} missing from the rows"));
        assert_eq!(row.pick, Some(pick), "{label} carries the wrong pick");
        assert!(row.enabled, "{label} must be enabled for a middle tab");
    }
}

#[test]
fn tab_menu_rows_mute_the_moves_at_the_edges_they_clamp_at() {
    // The first tab cannot move left; the last cannot move right;
    // a middle tab can move both ways — the binds' `move_target`
    // rule rendered as muting, never a clickable no-op.
    let first = tab_menu_rows(&[], 3, 0, false);
    let last = tab_menu_rows(&[], 3, 2, false);
    let middle = tab_menu_rows(&[], 3, 1, false);
    let by_label = |rows: &[TabMenuRow], label: &str| {
        rows.iter()
            .find(|row| row.label == label)
            .unwrap_or_else(|| panic!("{label} missing"))
            .enabled
    };
    assert!(!by_label(&first, "Move Tab Left"));
    assert!(by_label(&first, "Move Tab Right"));
    assert!(by_label(&last, "Move Tab Left"));
    assert!(!by_label(&last, "Move Tab Right"));
    assert!(by_label(&middle, "Move Tab Left"));
    assert!(by_label(&middle, "Move Tab Right"));
}

#[test]
fn tab_menu_rows_mute_both_moves_for_a_pinned_tab() {
    // Ruling 4's menu half: a pinned tab stays put, so both Move
    // rows render muted whatever the position — `move_tab_by`
    // refuses the pick, and a row that always no-ops never renders
    // clickable (the edge-muting rule, `pinned` consulted before
    // the edge). A loose middle tab keeps both enabled.
    let pinned = tab_menu_rows(&[], 5, 2, true);
    let loose = tab_menu_rows(&[], 5, 2, false);
    let by_label = |rows: &[TabMenuRow], label: &str| {
        rows.iter()
            .find(|row| row.label == label)
            .unwrap_or_else(|| panic!("{label} missing"))
            .enabled
    };
    assert!(!by_label(&pinned, "Move Tab Left"));
    assert!(!by_label(&pinned, "Move Tab Right"));
    assert!(by_label(&loose, "Move Tab Left"));
    assert!(by_label(&loose, "Move Tab Right"));
}

#[test]
fn tab_menu_rows_mute_close_tab_for_a_pinned_tab() {
    // The close refusal rendered, not just logged: a pinned tab's
    // Close Tab row mutes — the ×'s refusal has no row to mute,
    // the menu's does, with Unpin Tab in arm's reach above it —
    // while the sweep rows stay live: they aim at the *other*
    // tabs, and pinned neighbors drop out of their target lists
    // silently (the OQ-2 default).
    let pinned = tab_menu_rows(&[], 5, 2, true);
    let loose = tab_menu_rows(&[], 5, 2, false);
    let by_label = |rows: &[TabMenuRow], label: &str| {
        rows.iter()
            .find(|row| row.label == label)
            .unwrap_or_else(|| panic!("{label} missing"))
            .enabled
    };
    assert!(!by_label(&pinned, "Close Tab"));
    assert!(by_label(&pinned, "Close Other Tabs"));
    assert!(by_label(&pinned, "Close Tabs to the Right"));
    assert!(by_label(&loose, "Close Tab"));
}

#[test]
fn tab_menu_rows_hide_close_right_at_the_rightmost_tab() {
    // Firefox's hide rule: with no right neighbors the row would
    // always no-op, so it is omitted outright — Close Other Tabs
    // stays (the menu needs ≥2 tabs to open, others always exist).
    let last = tab_menu_rows(&[], 4, 3, false);
    assert!(!labels(&last).contains(&"Close Tabs to the Right"));
    assert!(labels(&last).contains(&"Close Other Tabs"));
    let inner = tab_menu_rows(&[], 4, 2, false);
    assert!(labels(&inner).contains(&"Close Tabs to the Right"));
}

#[test]
fn tab_menu_rows_separate_the_sections() {
    // The first row carries no separator; Rename leads the
    // clicked-tab cluster, Move the reorder section, Close the
    // close cluster — the profile section's separation is pinned
    // by its own test below.
    let rows = flat_rows();
    assert!(!rows[0].separated, "New Tab leads without a separator");
    let by_label = |label: &str| {
        rows.iter()
            .find(|row| row.label == label)
            .unwrap_or_else(|| panic!("{label} missing"))
            .separated
    };
    assert!(by_label("Rename Tab"));
    assert!(!by_label("Duplicate Tab"));
    assert!(!by_label("Pin Tab"));
    assert!(by_label("Move Tab Left"));
    assert!(!by_label("Move Tab Right"));
    assert!(by_label("Close Tab"));
    assert!(!by_label("Close Other Tabs"));
    assert!(!by_label("Close Tabs to the Right"));
}

#[test]
fn tab_menu_rows_gain_a_pin_row_after_duplicate_whose_label_flips_with_state() {
    // Ruling 1: the menu-only pin verb slots in after Duplicate Tab
    // inside the clicked-tab cluster, and the label names the
    // action the pick will perform — "Pin Tab" on a loose tab,
    // "Unpin Tab" on a pinned one.
    let unpinned = tab_menu_rows(&[], 5, 2, false);
    let duplicate = unpinned
        .iter()
        .position(|row| row.label == "Duplicate Tab")
        .expect("Duplicate Tab stays");
    assert_eq!(unpinned[duplicate + 1].label, "Pin Tab");
    assert_eq!(unpinned[duplicate + 1].pick, Some(TabMenuPick::Pin));
    assert!(unpinned[duplicate + 1].enabled);
    assert!(!unpinned[duplicate + 1].separated);
    let pinned = tab_menu_rows(&[], 5, 2, true);
    assert_eq!(pinned[duplicate + 1].label, "Unpin Tab");
    assert_eq!(pinned[duplicate + 1].pick, Some(TabMenuPick::Pin));
}

#[test]
fn tab_menu_rows_gain_the_switch_profile_section_only_with_profiles() {
    // Zero profiles: exactly the flat composition (the context
    // menu's zero-profiles flat ruling). With profiles: the
    // section slots in after the Pin row — a non-clickable
    // header, then one enabled row per profile in config order,
    // index-picked for apply-time resolution.
    let flat = flat_rows().len();
    let profiles = [menu_profile("ssh-web"), menu_profile("htop")];
    let with = tab_menu_rows(&profiles, 5, 2, false);
    assert_eq!(with.len(), flat + 3, "one header plus two profile rows");
    let pin = with
        .iter()
        .position(|row| row.pick == Some(TabMenuPick::Pin))
        .expect("the pin row stays");
    let header = &with[pin + 1];
    assert_eq!(header.label, "Switch Profile");
    assert_eq!(header.pick, None, "the header is not clickable");
    assert!(!header.enabled);
    assert!(header.separated, "the section separates from the cluster");
    let profile_rows = &with[pin + 2..pin + 2 + profiles.len()];
    assert_eq!(profile_rows.len(), profiles.len());
    for (index, row) in profile_rows.iter().enumerate() {
        assert_eq!(row.label, profiles[index].name);
        assert_eq!(row.pick, Some(TabMenuPick::SwitchProfile(index)));
        assert!(row.enabled);
        assert!(!row.separated);
    }
}

#[test]
fn tab_menu_bounds_anchors_at_the_button_and_clamps_to_the_strip() {
    // The bubble pops below the strip (the terminal side) with
    // its left edge at the button's — unless that would overflow
    // the strip's right end, where it shifts left to fit (the
    // dropdown's rule, both edges).
    let metrics = &METRICS;
    let beside = tab_menu_bounds(1000.0, 120.0, 80.0, 8, metrics, TooltipSide::Below);
    assert_eq!(beside.x, 120.0);
    assert_eq!(
        beside.y,
        metrics.bar_height + DROPDOWN_GAP,
        "the terminal side of a top strip is below it"
    );
    let above = tab_menu_bounds(1000.0, 120.0, 80.0, 8, metrics, TooltipSide::Above);
    assert_eq!(
        above.y + above.height,
        -DROPDOWN_GAP,
        "an above bubble clears the strip upward"
    );
    let clamped = tab_menu_bounds(300.0, 280.0, 80.0, 8, metrics, TooltipSide::Below);
    assert_eq!(
        clamped.x + clamped.width,
        300.0,
        "an anchored-too-far-right bubble ends at the strip's right edge"
    );
}
