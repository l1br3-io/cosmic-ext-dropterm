// SPDX-License-Identifier: GPL-3.0-only

//! The band's view-model pins (gpui-status Task 1): segment state →
//! presentation, headless — which seats compose rows, their cluster
//! order and dimming, the attention seats, the drop/elide fit over
//! measured widths, the separators, and the disc/halo geometry. The
//! measurer is a fake (a monospace 8 px cell); the render path feeds
//! the real shaped widths through the same pure pass.

use super::*;
use dropterm::core::status::PAD_X;
use dropterm::core::terminal::{ProgressState, ReportedProgress};
use std::time::Duration;

/// The fixture engine: the builtin layer's `band.right` dim — the
/// break's rule shape, the base this suite's `band_vm` calls run
/// against.
fn fixture_engine() -> dropterm::core::style_engine::StyleSet {
    use dropterm::core::style_engine::{Rule, RuleOrigin, StyleSet, parse_selector};
    let mut engine = StyleSet::default();
    engine.set_builtin(vec![Rule {
        origin: RuleOrigin::Builtin,
        selector: parse_selector("band.right").expect("parses"),
        declarations: dropterm::core::style_engine::parse_declarations(&serde_json::json!(
            { "color": "bright_black" }
        ))
        .declarations,
    }]);
    engine
}

/// The monospace fake: 8 px per character, 0 for the empty string.
fn fake_measure(text: &str) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "test strings, exact in f32")]
    let width = text.chars().count() as f32 * 8.0;
    width
}

/// A band with every seat standing: the left cluster's narration and
/// the right cluster's identity, all present at once.
fn rich() -> StatusSegments {
    StatusSegments {
        running: true,
        process: "cargo".to_string(),
        path: Some("~/src".to_string()),
        branch: Some("main".to_string()),
        profile: Some("dev".to_string()),
        zoom_percent: Some(250),
        grid: Some((100, 24)),
        tab_position: Some((2, 5)),
        clock: Some("14:32".to_string()),
        running_elapsed: Some(Duration::from_secs(41)),
        exit_code: None,
        scrolled: Some(12),
        pasted: Some(3),
        ..StatusSegments::default()
    }
}

#[test]
fn rows_compose_in_cluster_order_with_the_disc_leftmost_and_clock_rightmost() {
    let vm = band_vm(
        &rich(),
        2000.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    );
    let ids: Vec<SegmentId> = vm.rows.iter().map(|row| row.id).collect();
    assert_eq!(
        ids,
        vec![
            SegmentId::Process,
            SegmentId::Duration,
            SegmentId::Path,
            SegmentId::Branch,
            SegmentId::Profile,
            SegmentId::Grid,
            SegmentId::Tab,
            SegmentId::Zoom,
            SegmentId::Scrolled,
            SegmentId::Pasted,
            SegmentId::Clock,
        ]
    );
    // The left cluster starts at the band's side padding.
    assert_eq!(vm.rows[0].x, PAD_X);
    // The clock is the rightmost seat's right edge: the band width
    // less the side padding.
    let clock = vm.rows.last().expect("clock row");
    assert_eq!(clock.x + clock.width, 2000.0 - PAD_X);
    // The right cluster reads dimmer; the left cluster full — except
    // the duration's own dimmed narration.
    for row in &vm.rows {
        assert_eq!(
            row.dimmed,
            matches!(
                row.id,
                SegmentId::Duration
                    | SegmentId::Profile
                    | SegmentId::Grid
                    | SegmentId::Tab
                    | SegmentId::Zoom
                    | SegmentId::Scrolled
                    | SegmentId::Pasted
                    | SegmentId::Clock
            ),
            "{:?}",
            row.id
        );
    }
}

#[test]
fn row_texts_carry_their_glyphs_and_owned_marks() {
    let vm = band_vm(
        &rich(),
        2000.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    );
    let text = |id: SegmentId| {
        vm.rows
            .iter()
            .find(|row| row.id == id)
            .map(|row| row.text.as_str())
            .unwrap_or_default()
    };
    assert_eq!(text(SegmentId::Process), "cargo");
    assert_eq!(text(SegmentId::Duration), "⧗ 0:41");
    assert_eq!(text(SegmentId::Path), "⌂ ~/src");
    assert_eq!(text(SegmentId::Branch), "⎇ main");
    assert_eq!(text(SegmentId::Profile), "◉ dev");
    assert_eq!(text(SegmentId::Grid), "⊞ 100×24");
    assert_eq!(text(SegmentId::Tab), "▤ 2/5");
    assert_eq!(text(SegmentId::Zoom), "⇕ +250%");
    assert_eq!(text(SegmentId::Scrolled), "↑12");
    assert_eq!(text(SegmentId::Pasted), "▦ 3 images");
    assert_eq!(text(SegmentId::Clock), "◷ 14:32");
}

#[test]
fn the_disc_fills_with_the_foreground_and_tints_on_attention() {
    let vm = band_vm(
        &rich(),
        2000.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    );
    let disc = vm.disc.expect("a running foreground paints the disc");
    assert!(disc.filled);
    assert!(!disc.attention);
    // The disc sits at the cluster's head; the name follows its gap.
    assert_eq!(disc.x, PAD_X);

    let mut idle = rich();
    idle.running = false;
    let disc = band_vm(
        &idle,
        2000.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    )
    .disc
    .expect("idle disc");
    assert!(!disc.filled);
}

#[test]
fn absent_seats_never_compose_rows_or_hairlines() {
    let sparse = StatusSegments {
        process: "fish".to_string(),
        path: Some("~/src".to_string()),
        ..StatusSegments::default()
    };
    let vm = band_vm(
        &sparse,
        2000.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    );
    let ids: Vec<SegmentId> = vm.rows.iter().map(|row| row.id).collect();
    assert_eq!(ids, vec![SegmentId::Process, SegmentId::Path]);
    // Two left rows → exactly one separator, between them; none
    // beside an absent segment, none between the clusters.
    assert_eq!(vm.separators.len(), 1);
    let process_end = vm.rows[0].x + vm.rows[0].width;
    let path_start = vm.rows[1].x;
    assert_eq!(vm.separators[0], f32::midpoint(process_end, path_start));
}

#[test]
fn the_fit_drops_lowest_value_seats_first_under_width_pressure() {
    let vm = band_vm(
        &rich(),
        2000.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    );
    assert!(vm.rows.iter().any(|row| row.id == SegmentId::Pasted));
    // Tight: at 8 px/char the rich band consumes well over 300 px —
    // squeezing to 340 drops the paste chip (and only it if the
    // elided path absorbs the rest), the reference DROP_ORDER's
    // first present seat.
    let tight = band_vm(
        &rich(),
        340.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    );
    assert!(
        !tight.rows.iter().any(|row| row.id == SegmentId::Pasted),
        "the paste chip yields first"
    );
    assert!(
        tight.rows.iter().any(|row| row.id == SegmentId::Clock),
        "the clock survives to the bitter end"
    );
    // Survivors keep their cluster seats: the clock stays rightmost.
    let clock = tight
        .rows
        .iter()
        .find(|row| row.id == SegmentId::Clock)
        .expect("clock survives");
    assert_eq!(clock.x + clock.width, 340.0 - PAD_X);
}

#[test]
fn the_path_elides_from_the_left_under_squeeze() {
    let mut long = rich();
    long.path = Some("~/a-very-long-directory-name/src".to_string());
    let roomy = band_vm(
        &long,
        2000.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    );
    let path = roomy
        .rows
        .iter()
        .find(|row| row.id == SegmentId::Path)
        .expect("path row");
    assert_eq!(path.text, "⌂ ~/a-very-long-directory-name/src");

    let squeezed = band_vm(
        &long,
        300.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    );
    let path = squeezed
        .rows
        .iter()
        .find(|row| row.id == SegmentId::Path)
        .expect("the path survives squeezed, elided");
    assert!(path.text.starts_with('…'), "elided: {}", path.text);
    assert!(path.text.len() < "⌂ ~/a-very-long-directory-name/src".len());
    // The glyph is eaten first by the eliding `…`.
    assert!(!path.text.contains('⌂'));
}

#[test]
fn the_exit_chip_and_error_progress_carry_the_attention_seat() {
    let mut failed = rich();
    failed.running_elapsed = None;
    failed.exit_code = Some(3);
    let vm = band_vm(
        &failed,
        2000.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    );
    let exit = vm
        .rows
        .iter()
        .find(|row| row.id == SegmentId::Exit)
        .expect("exit");
    assert_eq!(exit.text, "✗3");
    assert!(exit.attention);
    // Every other seat stays unmarked.
    assert!(
        vm.rows
            .iter()
            .all(|row| row.id == SegmentId::Exit || !row.attention)
    );

    let mut errored = rich();
    errored.progress = Some(ReportedProgress {
        state: ProgressState::Error,
        value: None,
        refreshed_at: std::time::Instant::now(),
    });
    let vm = band_vm(
        &errored,
        2000.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    );
    let progress = vm
        .rows
        .iter()
        .find(|row| row.id == SegmentId::Progress)
        .expect("progress row");
    assert_eq!(progress.text, "◐ !");
    assert!(progress.attention);
}

#[test]
fn the_halo_rings_the_disc_or_stands_alone_at_the_edge() {
    let mut marked = rich();
    marked.missed_activity = true;
    let vm = band_vm(
        &marked,
        2000.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    );
    let disc = vm.disc.expect("disc present");
    assert_eq!(vm.halo_center_x, Some(disc.x + DISC_SIZE / 2.0));

    // No process seat: the mark stands alone, centered in the
    // leading padding zone.
    let mut alone = marked;
    alone.process = String::new();
    alone.running = false;
    let vm = band_vm(
        &alone,
        2000.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    );
    assert!(vm.disc.is_none());
    assert_eq!(vm.halo_center_x, Some(PAD_X / 2.0));

    // No news: no halo.
    assert_eq!(
        band_vm(
            &rich(),
            2000.0,
            &fake_measure,
            None,
            None,
            None,
            &fixture_engine()
        )
        .halo_center_x,
        None
    );
}

#[test]
fn an_empty_band_composes_nothing() {
    let vm = band_vm(
        &StatusSegments::default(),
        1280.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    );
    assert!(vm.rows.is_empty());
    assert!(vm.separators.is_empty());
    assert!(vm.disc.is_none());
    assert_eq!(vm.halo_center_x, None);
}

#[test]
fn the_flash_swaps_only_the_copied_rows_display() {
    // The ✓ takes the copied segment's seat alone — the copy payload
    // (settled at the click) stays the full value; the path's own
    // fit measure keeps the steady text (the reference's rule: the
    // flash never re-fits the path).
    let vm = band_vm(
        &rich(),
        2000.0,
        &fake_measure,
        Some(SegmentId::Path),
        None,
        None,
        &fixture_engine(),
    );
    let path = vm
        .rows
        .iter()
        .find(|row| row.id == SegmentId::Path)
        .expect("path row");
    assert_eq!(path.text, "\u{2713}");
    for row in &vm.rows {
        if row.id != SegmentId::Path {
            assert!(!row.text.is_empty(), "{:?} keeps its display", row.id);
            assert_ne!(row.text, "\u{2713}", "{:?} does not flash", row.id);
        }
    }
}

#[test]
fn attention_red_picks_by_strip_luminance_and_degrades_honestly() {
    let plain = gpui::Hsla {
        h: 0.,
        s: 1.,
        l: 0.4,
        a: 1.,
    };
    let bright = gpui::Hsla {
        h: 0.,
        s: 1.,
        l: 0.6,
        a: 1.,
    };
    let fg = gpui::Hsla {
        h: 0.9,
        s: 0.1,
        l: 0.8,
        a: 1.,
    };
    let dark_strip = gpui::Hsla {
        h: 0.,
        s: 0.,
        l: 0.1,
        a: 1.,
    };
    let light_strip = gpui::Hsla {
        h: 0.,
        s: 0.,
        l: 0.9,
        a: 1.,
    };
    // The bright red on dark strips, the plain on light — the band
    // foreground's own luminance flip.
    assert_eq!(
        attention_color(Some(plain), Some(bright), fg, dark_strip),
        bright
    );
    assert_eq!(
        attention_color(Some(plain), Some(bright), fg, light_strip),
        plain
    );
    // A missing chosen variant falls back to its sibling.
    assert_eq!(attention_color(None, Some(bright), fg, dark_strip), bright);
    assert_eq!(attention_color(Some(plain), None, fg, light_strip), plain);
    // A scheme with no red at all degrades to the pane foreground —
    // monochrome, never an invented hue.
    assert_eq!(attention_color(None, None, fg, dark_strip), fg);
}

#[test]
fn narration_is_fit_accounted_so_the_clusters_never_overlap() {
    // The left cluster's narration seats are present and wide at
    // once. `band_fit`'s contract reserves every present segment
    // but the path and the process; before the narration joined the
    // sum, the path and branch were laid straight through the right
    // cluster at every width below ~800 px, and even at 600 the
    // grid and profile collided with them. No two rows may overlap
    // at any width.
    let mut s = rich();
    s.exit_code = Some(3);
    s.progress = Some(ReportedProgress {
        state: ProgressState::InProgress,
        value: Some(42),
        refreshed_at: std::time::Instant::now(),
    });
    let mut failures = Vec::new();
    for width in [
        200.0_f32, 260.0, 300.0, 340.0, 420.0, 500.0, 600.0, 800.0, 1280.0,
    ] {
        let vm = band_vm(
            &s,
            width,
            &fake_measure,
            None,
            None,
            None,
            &fixture_engine(),
        );
        let mut spans: Vec<(f32, f32, SegmentId)> =
            vm.rows.iter().map(|r| (r.x, r.x + r.width, r.id)).collect();
        spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        for pair in spans.windows(2) {
            if pair[0].1 > pair[1].0 + 0.01 {
                failures.push(format!(
                    "width {width}: {:?} ends {} > {:?} starts {}",
                    pair[0].2, pair[0].1, pair[1].2, pair[1].0
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_refusal_verdict_takes_the_pasted_seat_and_wears_attention() {
    // With pastes standing, the verdict swaps the pasted chip's
    // display for its 3 s window — measured like text, the row
    // re-lays, and the seat wears the attention red (the exit
    // chip's own rule).
    let vm = band_vm(
        &rich(),
        2000.0,
        &fake_measure,
        None,
        Some(PasteRefusal::OverCap),
        None,
        &fixture_engine(),
    );
    let row = vm
        .rows
        .iter()
        .find(|row| row.id == SegmentId::Pasted)
        .expect("the pasted seat stands in the rich band");
    assert_eq!(row.text, "✗ image over 32 MiB cap");
    assert!(row.attention);

    // Without pastes the verdict takes the clock's seat.
    let mut sparse = rich();
    sparse.pasted = None;
    let vm = band_vm(
        &sparse,
        2000.0,
        &fake_measure,
        None,
        Some(PasteRefusal::Failed),
        None,
        &fixture_engine(),
    );
    let row = vm
        .rows
        .iter()
        .find(|row| row.id == SegmentId::Clock)
        .expect("the clock is the right cluster's tail");
    assert_eq!(row.text, "✗ paste failed");
    assert!(row.attention);

    // The steady values return once the verdict expires.
    let vm = band_vm(
        &rich(),
        2000.0,
        &fake_measure,
        None,
        None,
        None,
        &fixture_engine(),
    );
    let row = vm
        .rows
        .iter()
        .find(|row| row.id == SegmentId::Pasted)
        .expect("the pasted seat stands in the rich band");
    assert_eq!(row.text, "▦ 3 images");
    assert!(!row.attention);
}

// --- the styled painter (styled-status-slot, task ST2) ---

/// A palette over a fixed scheme: every ANSI word a distinct gray
/// step, so a resolved run's color is identifiable by value.
fn fixture_palette() -> ScriptPalette {
    use alacritty_terminal::term::color::Colors;
    use alacritty_terminal::vte::ansi::{NamedColor, Rgb};
    let mut colors = Colors::default();
    let mut set = |named: NamedColor, r: u8, g: u8, b: u8| {
        colors[named] = Some(Rgb { r, g, b });
    };
    set(NamedColor::BrightBlue, 16, 16, 16); // "accent"
    set(NamedColor::Foreground, 240, 240, 240); // "fg"'s scheme word
    set(NamedColor::Red, 200, 40, 40);
    set(NamedColor::Green, 40, 200, 40);
    let fg = tab_bar::hsla(245, 245, 245);
    ScriptPalette::resolve(&colors, fg, tab_bar::hsla(10, 10, 10))
}

#[test]
fn the_palette_resolves_the_scheme_words_and_the_accent() {
    let palette = fixture_palette();
    assert_eq!(palette.color("red"), Some(tab_bar::hsla(200, 40, 40)));
    assert_eq!(palette.color("green"), Some(tab_bar::hsla(40, 200, 40)));
    assert_eq!(
        palette.color("accent"),
        Some(tab_bar::hsla(16, 16, 16)),
        "the accent rides the scheme's BrightBlue (theme_gen's own mapping)"
    );
    assert_eq!(palette.color("fg"), Some(palette.fg));
    assert_eq!(
        palette.color("nope"),
        None,
        "a word outside the palette is unknown, not a fallback"
    );
}

#[test]
fn paint_script_segment_maps_the_resolved_cascade() {
    use dropterm::core::slots::{SlotColor, SlotContent, Span};
    use dropterm::core::style_engine::Declaration;

    let palette = fixture_palette();
    let styled = SlotContent::Styled {
        spans: vec![
            Span {
                text: "▼".to_owned(),
                class: None,
                id: None,
                style: vec![
                    Declaration::Color(SlotColor::Token("red".to_owned())),
                    Declaration::Icon("↓".to_owned()),
                ],
            },
            Span {
                text: " 1.2 MB/s".to_owned(),
                class: None,
                id: None,
                style: vec![Declaration::Color(SlotColor::Hex("#1f9e7b".to_owned()))],
            },
            Span {
                text: " plain".to_owned(),
                class: None,
                id: None,
                style: Vec::new(),
            },
        ],
    };

    // Per-run colors resolve through the palette; the glyph prefix
    // rode the composition (script_runs).
    let runs = dropterm::core::status::script_runs(&styled, &fixture_engine(), None);
    let painted = paint_script_segment(&runs, &palette);
    assert_eq!(painted.len(), 3, "one styled run per span, none dropped");
    assert_eq!(painted[0].text, "↓ ▼", "the glyph prefix rides the run");
    assert_eq!(painted[0].color, tab_bar::hsla(200, 40, 40), "token red");
    assert_eq!(painted[1].text, " 1.2 MB/s");
    assert_eq!(
        painted[1].color,
        tab_bar::hsla(0x1f, 0x9e, 0x7b),
        "hex parses direct"
    );
    assert_eq!(painted[2].text, " plain");
    assert_eq!(painted[2].color, palette.fg, "an uncolored span is fg");

    // An unknown token renders the span unstyled (fg) and never
    // drops it.
    let unknown = SlotContent::Styled {
        spans: vec![
            Span {
                text: "a".to_owned(),
                class: None,
                id: None,
                style: vec![Declaration::Color(SlotColor::Token("nope".to_owned()))],
            },
            Span {
                text: "b".to_owned(),
                class: None,
                id: None,
                style: Vec::new(),
            },
        ],
    };
    let painted = paint_script_segment(
        &dropterm::core::status::script_runs(&unknown, &fixture_engine(), None),
        &palette,
    );
    assert_eq!(painted.len(), 2, "no span drops for a color failure");
    assert_eq!(
        painted[0].color, palette.fg,
        "unknown token renders unstyled"
    );

    // The string arm: one run, the row's own fg — today's paint.
    let painted = paint_script_segment(
        &dropterm::core::status::script_runs(
            &SlotContent::Text("git:1".to_owned()),
            &fixture_engine(),
            None,
        ),
        &palette,
    );
    assert_eq!(painted.len(), 1);
    assert_eq!(painted[0].text, "git:1");
    assert_eq!(painted[0].color, palette.fg);
}

#[test]
fn the_side_dim_is_the_builtin_band_right_color_and_a_rule_replaces_it() {
    use dropterm::core::slots::{SlotContent, Span};
    use dropterm::core::style_engine::{Declaration, Rule, RuleOrigin, parse_selector};

    let palette = fixture_palette();

    // The builtin dim: a right-side span with no color inherits the
    // side's builtin color; a span with its own color keeps it (the
    // ruled break — the dim is a color declaration, never alpha).
    let plain = SlotContent::Styled {
        spans: vec![Span {
            text: "idle".to_owned(),
            class: None,
            id: None,
            style: Vec::new(),
        }],
    };
    let right = dropterm::core::status::script_runs(
        &plain,
        &fixture_engine(),
        Some(dropterm::core::status::ScriptSide::Right),
    );
    let painted = paint_script_segment(&right, &palette);
    assert_eq!(
        painted[0].color,
        palette
            .color("bright_black")
            .expect("the fixture's dim word"),
        "the right side's runs inherit the builtin band.right color"
    );
    let left = dropterm::core::status::script_runs(
        &plain,
        &fixture_engine(),
        Some(dropterm::core::status::ScriptSide::Left),
    );
    let painted = paint_script_segment(&left, &palette);
    assert_eq!(painted[0].color, palette.fg, "the left side is fg");

    // A span's own color paints whole on the right side (no alpha
    // multiplies on top — the spec's dim story is the builtin color).
    let colored = SlotContent::Styled {
        spans: vec![Span {
            text: "on".to_owned(),
            class: None,
            id: None,
            style: vec![Declaration::Color(SlotColor::Token("red".to_owned()))],
        }],
    };
    let runs = dropterm::core::status::script_runs(
        &colored,
        &fixture_engine(),
        Some(dropterm::core::status::ScriptSide::Right),
    );
    let painted = paint_script_segment(&runs, &palette);
    assert_eq!(
        painted[0].color,
        tab_bar::hsla(200, 40, 40),
        "the span's color wins over the side's, undimmed"
    );

    // A fixture rule on band.right replaces the builtin dim by
    // ordinary cascade (equal specificity, later source order).
    let mut engine = fixture_engine();
    engine.replace_runtime_sheet(
        "plugin",
        vec![Rule {
            origin: RuleOrigin::Runtime {
                plugin: "plugin".to_owned(),
            },
            selector: parse_selector("band.right").expect("parses"),
            declarations: dropterm::core::style_engine::parse_declarations(&serde_json::json!(
                { "color": "green" }
            ))
            .declarations,
        }],
    );
    let runs = dropterm::core::status::script_runs(
        &plain,
        &engine,
        Some(dropterm::core::status::ScriptSide::Right),
    );
    let painted = paint_script_segment(&runs, &palette);
    assert_eq!(
        painted[0].color,
        tab_bar::hsla(40, 200, 40),
        "the plugin's band.right color replaced the builtin dim"
    );
}

#[test]
fn unknown_tokens_name_themselves_once_per_segment_and_reply() {
    use dropterm::core::slots::{SlotColor, SlotContent, Span};
    use dropterm::core::status::SegmentAnswer;

    let answers = vec![
        SegmentAnswer::Shown {
            id: "net".to_owned(),
            side: dropterm::core::status::ScriptSide::Left,
            content: SlotContent::Styled {
                spans: vec![
                    Span {
                        text: "a".to_owned(),
                        class: None,
                        id: None,
                        style: vec![dropterm::core::style_engine::Declaration::Color(
                            SlotColor::Token("nope".to_owned()),
                        )],
                    },
                    Span {
                        text: "b".to_owned(),
                        class: None,
                        id: None,
                        style: vec![dropterm::core::style_engine::Declaration::Color(
                            SlotColor::Token("nope".to_owned()),
                        )],
                    },
                    Span {
                        text: "c".to_owned(),
                        class: None,
                        id: None,
                        style: vec![dropterm::core::style_engine::Declaration::Color(
                            SlotColor::Token("nah".to_owned()),
                        )],
                    },
                ],
            },
        },
        SegmentAnswer::Shown {
            id: "git".to_owned(),
            side: dropterm::core::status::ScriptSide::Right,
            content: SlotContent::Text("fine".to_owned()),
        },
    ];
    let lines = unknown_token_lines(&answers);
    assert_eq!(
        lines.len(),
        2,
        "one line per distinct unknown token per segment: {lines:?}"
    );
    assert!(lines[0].contains("net") && lines[0].contains("nope"));
    assert!(lines[1].contains("net") && lines[1].contains("nah"));
}

/// The palette's own docs pin (the `every_..._is_documented`
/// pattern, styled-status-slot ST3): every color word the band
/// resolves appears backticked in docs/plugins.md's typed-answers
/// section — the closed wordset cannot drift from its docs.
#[test]
fn every_slot_token_word_is_documented() {
    let docs = include_str!("../../../docs/plugins.md");
    let (_, rest) = docs
        .split_once("## Typed answers")
        .expect("docs/plugins.md carries the typed-answers section");
    let section = rest
        .split_once("\n## ")
        .map_or(rest, |(section, _)| section);
    for word in SLOT_TOKENS {
        assert!(
            section.contains(&format!("`{word}`")),
            "the palette word `{word}` is not documented in docs/plugins.md"
        );
    }
}
