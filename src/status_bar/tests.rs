use super::*;

fn home() -> std::path::PathBuf {
    std::path::PathBuf::from("/home/jpt")
}

/// The v1 positional shape over the grown resolver — the grown
/// inputs stay absent, exactly how they arrive until the app
/// wires them.
fn resolve(
    home: Option<&Path>,
    cwd: Option<&Path>,
    shell: &str,
    profile_name: Option<&str>,
    zoom_adj: i8,
    zoom_step_mul_100: u16,
    foreground: Option<&str>,
) -> StatusSegments {
    let inputs = StatusInputs {
        home,
        cwd,
        shell,
        profile_name,
        zoom_adj,
        zoom_step_mul_100,
        foreground,
        branch: None,
        grid: None,
        spawn_grid: None,
        tab_position: None,
        clock: None,
    };
    resolve_segments(&inputs, StatusToggles::all())
}

// --- segment assembly (running/idle × profile × zoom × cwd) ---

#[test]
fn a_running_child_names_the_process_and_fills_the_disc() {
    let segments = resolve(
        Some(&home()),
        Some(Path::new("/home/jpt/src/oss/l1br3/ced2")),
        "/usr/bin/fish",
        Some("dev"),
        2,
        100,
        Some("cargo"),
    );
    assert!(segments.running);
    assert_eq!(segments.process, "cargo");
    assert_eq!(segments.path.as_deref(), Some("~/src/oss/l1br3/ced2"));
    assert_eq!(segments.profile.as_deref(), Some("dev"));
    assert_eq!(segments.zoom_percent, Some(200));
}

#[test]
fn an_idle_tab_falls_back_to_the_shell_basename_and_a_hollow_disc() {
    let segments = resolve(
        Some(&home()),
        Some(Path::new("/home/jpt")),
        "/usr/bin/fish",
        None,
        0,
        100,
        None,
    );
    assert!(!segments.running);
    assert_eq!(segments.process, "fish");
    assert_eq!(segments.path.as_deref(), Some("~"));
    assert_eq!(segments.profile, None);
    assert_eq!(segments.zoom_percent, None);
}

#[test]
fn an_unreadable_comm_falls_back_like_idle() {
    // A failed /proc read is indistinguishable from idle in its
    // inputs — the band stays honest either way.
    let segments = resolve(
        Some(&home()),
        Some(Path::new("/srv")),
        "/bin/bash",
        Some("ops"),
        1,
        100,
        None,
    );
    assert!(!segments.running);
    assert_eq!(segments.process, "bash");
    assert_eq!(segments.profile.as_deref(), Some("ops"));
}

#[test]
fn a_missing_cwd_drops_only_the_path_segment() {
    let segments = resolve(
        Some(&home()),
        None,
        "/usr/bin/zsh",
        Some("dev"),
        1,
        100,
        Some("vim"),
    );
    assert_eq!(segments.path, None);
    assert!(segments.running);
    assert_eq!(segments.process, "vim");
    assert_eq!(segments.profile.as_deref(), Some("dev"));
    assert_eq!(segments.zoom_percent, Some(100));
}

#[test]
fn a_dangling_profile_id_hides_the_segment() {
    // The app resolves the id before feeding the resolver; a
    // missing profile arrives as `None`, never a stale name.
    let segments = resolve(Some(&home()), None, "/usr/bin/zsh", None, 0, 100, None);
    assert_eq!(segments.profile, None);
    assert_eq!(segments.zoom_percent, None);
}

// --- the ~ collapse ---

#[test]
fn a_home_prefixed_path_collapses_to_the_tilde() {
    assert_eq!(
        collapse_home(Path::new("/home/jpt/src/oss"), Some(&home())),
        "~/src/oss"
    );
}

#[test]
fn home_itself_collapses_to_a_bare_tilde() {
    assert_eq!(collapse_home(&home(), Some(&home())), "~");
}

#[test]
fn a_foreign_path_stays_verbatim() {
    assert_eq!(
        collapse_home(Path::new("/srv/web"), Some(&home())),
        "/srv/web"
    );
}

#[test]
fn a_missing_home_keeps_paths_verbatim() {
    // An unset `HOME` — never in a real desktop session — must
    // not mangle paths into a bogus tilde form.
    assert_eq!(
        collapse_home(Path::new("/home/jpt/src"), None),
        "/home/jpt/src"
    );
}

#[test]
fn a_sibling_sharing_the_home_prefix_does_not_collapse() {
    // Component boundaries, not string prefixes: `/home/jpt-x`
    // does not live inside `/home/jpt`.
    assert_eq!(
        collapse_home(Path::new("/home/jpt-x/thing"), Some(&home())),
        "/home/jpt-x/thing"
    );
}

// --- left elision ---

#[test]
fn short_paths_pass_through_untouched() {
    assert_eq!(elide_path_left("~/src", 100), "~/src");
    assert_eq!(elide_path_left("~/src", 5), "~/src");
}

#[test]
fn long_paths_keep_their_tail_under_a_leading_ellipsis() {
    assert_eq!(elide_path_left("~/src/oss/l1br3/ced2", 12), "…/l1br3/ced2");
    assert_eq!(
        elide_path_left("/srv/web/prod/current", 13),
        "…prod/current"
    );
}

#[test]
fn degenerate_widths_elide_to_the_floor() {
    assert_eq!(elide_path_left("~/src/oss", 0), "");
    assert_eq!(elide_path_left("~/src/oss", 1), "…");
}

// --- the left cluster's squeeze (path elides before the branch
// yields) ---

#[test]
fn a_roomy_cluster_shows_the_path_and_branch_whole() {
    // 120 px of path + 12 px gap + 30 px of branch under 200 px.
    assert_eq!(left_squeeze(200.0, 12, 120.0, 30.0), (PathFit::Full, true));
}

#[test]
fn the_path_elides_before_the_branch_yields() {
    // 100 px budget: the branch keeps its 42 px (gap included),
    // the path elides into the remaining 58 px (5 chars at
    // 10 px each).
    assert_eq!(
        left_squeeze(100.0, 12, 120.0, 30.0),
        (PathFit::Elided(5), true)
    );
}

#[test]
fn the_branch_yields_only_after_full_elision_of_the_path() {
    // 40 px cannot host the branch beside even an empty path —
    // the branch drops and the path elides into the whole 40 px.
    assert_eq!(
        left_squeeze(40.0, 12, 120.0, 30.0),
        (PathFit::Elided(4), false)
    );
}

#[test]
fn a_starved_cluster_loses_the_path_entirely() {
    assert_eq!(left_squeeze(5.0, 12, 120.0, 30.0), (PathFit::Gone, false));
}

// --- the band-wide fit pass (status-glance criterion 2) ---
//
// One pure decision over the whole band: the path elides first,
// progressively; past full elision segments drop one at a time in
// DROP_ORDER, skipping absent ones, and the path re-expands into
// each freed width. Process and path never drop — the disc cluster
// rides `fixed`, the path elides.

/// The standard fit fixture: 100 px of fixed disc cluster, a
/// 20-char path at 10 px per char (200 px whole), and all six
/// droppable segments measured (gap-inclusive costs 52/42/42/32/
/// 42/42 with `SEGMENT_GAP` = 12).
fn fit_widths() -> Vec<(SegmentId, f32)> {
    vec![
        (SegmentId::Branch, 30.0),
        (SegmentId::Profile, 40.0),
        (SegmentId::Grid, 30.0),
        (SegmentId::Tab, 20.0),
        (SegmentId::Zoom, 30.0),
        (SegmentId::Clock, 30.0),
    ]
}

fn fit_path() -> PathMeasure {
    PathMeasure {
        chars: 20,
        full_width: 200.0,
    }
}

#[test]
fn the_drop_order_pins_the_precedence() {
    // Facts the shell knows go first, then slow-moving identity,
    // the branch beside its path, the clock to the bitter end.
    // Process and path are absent by construction: the disc never
    // yields, and the path elides before anything else gives way.
    assert_eq!(
        DROP_ORDER,
        [
            SegmentId::Grid,
            SegmentId::Tab,
            SegmentId::Zoom,
            SegmentId::Profile,
            SegmentId::Branch,
            SegmentId::Clock,
        ]
    );
}

#[test]
fn zero_pressure_renders_everything() {
    // 552 px of consumption (fixed + six gaps-and-widths + the
    // whole path) under 600 px: nothing drops, nothing elides.
    let fit = band_fit(600.0, 100.0, &fit_widths(), fit_path());
    assert_eq!(fit.path, PathFit::Full);
    assert!(fit.dropped.is_empty());
}

#[test]
fn pressure_elides_the_path_before_any_segment_drops() {
    // 500 px leaves 248 px of room for the 200 px path: elide to
    // 14 chars — but nothing drops while any of the path survives.
    let fit = band_fit(500.0, 100.0, &fit_widths(), fit_path());
    assert_eq!(fit.path, PathFit::Elided(14));
    assert!(fit.dropped.is_empty());
}

#[test]
fn past_full_elision_segments_drop_one_at_a_time_in_order() {
    // 350 px cannot host even a fully elided path (room below one
    // character): the grid goes first, and the path re-expands
    // into the freed 42 px — four characters' worth.
    let fit = band_fit(350.0, 100.0, &fit_widths(), fit_path());
    assert_eq!(fit.dropped, vec![SegmentId::Grid]);
    assert_eq!(fit.path, PathFit::Elided(4));

    // 300 px costs a second drop: tab follows grid, and the path
    // re-expands again (22 px of room, two characters).
    let fit = band_fit(300.0, 100.0, &fit_widths(), fit_path());
    assert_eq!(fit.dropped, vec![SegmentId::Grid, SegmentId::Tab]);
    assert_eq!(fit.path, PathFit::Elided(2));
}

#[test]
fn absent_segments_are_skipped_in_the_drop_order() {
    // No grid segment (toggle off, or at spawn dimensions): 300 px
    // starves the band past full elision, and the first yield is
    // the tab — the absent grid is skipped, not waited on.
    let widths: Vec<(SegmentId, f32)> = fit_widths()
        .into_iter()
        .filter(|(id, _)| *id != SegmentId::Grid)
        .collect();
    let fit = band_fit(300.0, 100.0, &widths, fit_path());
    assert_eq!(fit.dropped, vec![SegmentId::Tab]);
    assert_eq!(fit.path, PathFit::Elided(2));
}

#[test]
fn the_path_re_expands_monotonically_as_drops_free_width() {
    // Widths chosen so exactly `k` drops are forced (the k-th
    // survivor's room sits at 10 + 8(k-1) px — one character plus
    // a growing slack, while the (k-1)-th sits below one
    // character): the path's char budget never shrinks as drops
    // free width.
    let mut prev_chars = 0;
    let fixture = fit_widths();
    for k in 1..=6 {
        let freed: f32 = DROP_ORDER
            .iter()
            .take(k)
            .map(|id| {
                let (_, w) = fixture
                    .iter()
                    .find(|(wid, _)| wid == id)
                    .expect("every DROP_ORDER id is measured in the fixture");
                SEGMENT_GAP + w
            })
            .sum();
        #[expect(
            clippy::cast_precision_loss,
            reason = "a loop index of six, exact in f32"
        )]
        let room = 10.0 + 8.0 * (k as f32 - 1.0);
        let width = 100.0 + 252.0 + room - freed;
        let fit = band_fit(width, 100.0, &fit_widths(), fit_path());
        assert_eq!(
            fit.dropped.len(),
            k,
            "width {width} must force exactly {k} drops"
        );
        let chars = match fit.path {
            PathFit::Elided(n) => n,
            PathFit::Gone => 0,
            PathFit::Full => panic!("a starved band cannot render the whole path"),
        };
        assert!(
            chars >= prev_chars,
            "the path must not re-elide after a drop (got {chars}, had {prev_chars})"
        );
        prev_chars = chars;
    }
}

#[test]
fn the_disc_never_yields_even_when_everything_drops() {
    // Zero width: all six droppables give way in order and the
    // path is gone — but `fixed` (the paddings and the disc
    // cluster) is untouchable by construction, and the path never
    // appears in the dropped set: the type has no say in it.
    let fit = band_fit(0.0, 100.0, &fit_widths(), fit_path());
    assert_eq!(
        fit.dropped,
        vec![
            SegmentId::Grid,
            SegmentId::Tab,
            SegmentId::Zoom,
            SegmentId::Profile,
            SegmentId::Branch,
            SegmentId::Clock,
        ]
    );
    assert_eq!(fit.path, PathFit::Gone);
}

#[test]
fn an_absent_path_never_forces_a_drop() {
    // No path characters: nothing to squeeze for — every segment
    // keeps its seat at any width.
    let fit = band_fit(
        100.0,
        100.0,
        &fit_widths(),
        PathMeasure {
            chars: 0,
            full_width: 0.0,
        },
    );
    assert_eq!(fit.path, PathFit::Full);
    assert!(fit.dropped.is_empty());
}

#[test]
fn an_absent_path_never_elides() {
    // No path characters: nothing to squeeze — the branch either
    // fits or it does not.
    assert_eq!(left_squeeze(50.0, 0, 0.0, 30.0), (PathFit::Full, true));
    assert_eq!(left_squeeze(20.0, 0, 0.0, 30.0), (PathFit::Full, false));
    assert_eq!(left_squeeze(20.0, 0, 0.0, 0.0), (PathFit::Full, false));
}

// --- zoom arithmetic ---

#[test]
fn zoom_percent_multiplies_the_step_signed() {
    assert_eq!(zoom_percent(2, 100), Some(200));
    assert_eq!(zoom_percent(-1, 100), Some(-100));
    assert_eq!(zoom_percent(3, 50), Some(150));
}

#[test]
fn zoom_percent_hides_at_zero_adjustment() {
    assert_eq!(zoom_percent(0, 100), None);
    assert_eq!(zoom_percent(0, 0), None);
}

#[test]
fn zoom_percent_saturates_rather_than_wrapping() {
    assert_eq!(zoom_percent(i8::MAX, u16::MAX), Some(i16::MAX));
    assert_eq!(zoom_percent(i8::MIN, u16::MAX), Some(i16::MIN));
}

// --- the stacking rule ---

#[test]
fn top_anchor_with_a_top_pinned_strip_keeps_the_band_beside_the_grab_bar() {
    assert_eq!(
        column_stacking(Edge::Top, TabPosition::Top),
        [
            Strip::Tabs,
            Strip::Terminal,
            Strip::StatusBand,
            Strip::GrabBar
        ]
    );
}

#[test]
fn top_anchor_with_an_auto_or_bottom_strip_stacks_the_strip_beside_the_grab_bar() {
    // Auto keeps the strip opposite the screen edge — surface
    // bottom under a Top anchor, beside the grab bar; an explicit
    // Bottom pin lands the same way.
    let expected = [
        Strip::Terminal,
        Strip::Tabs,
        Strip::StatusBand,
        Strip::GrabBar,
    ];
    assert_eq!(column_stacking(Edge::Top, TabPosition::Auto), expected);
    assert_eq!(column_stacking(Edge::Top, TabPosition::Bottom), expected);
}

#[test]
fn bottom_anchor_with_an_auto_or_top_strip_stacks_the_band_under_the_grab_bar() {
    // The grab bar leads the column at the Bottom anchor's free
    // edge (the surface top); the band takes the slot right under
    // it.
    let expected = [
        Strip::GrabBar,
        Strip::StatusBand,
        Strip::Tabs,
        Strip::Terminal,
    ];
    assert_eq!(column_stacking(Edge::Bottom, TabPosition::Auto), expected);
    assert_eq!(column_stacking(Edge::Bottom, TabPosition::Top), expected);
}

#[test]
fn bottom_anchor_with_a_bottom_pinned_strip_keeps_the_band_beside_the_grab_bar() {
    assert_eq!(
        column_stacking(Edge::Bottom, TabPosition::Bottom),
        [
            Strip::GrabBar,
            Strip::StatusBand,
            Strip::Terminal,
            Strip::Tabs,
        ]
    );
}

#[test]
fn the_band_is_adjacent_to_the_grab_bar_in_every_stacking() {
    for (edge, position) in [
        (Edge::Top, TabPosition::Auto),
        (Edge::Top, TabPosition::Top),
        (Edge::Top, TabPosition::Bottom),
        (Edge::Bottom, TabPosition::Auto),
        (Edge::Bottom, TabPosition::Top),
        (Edge::Bottom, TabPosition::Bottom),
    ] {
        let stacking = column_stacking(edge, position);
        let band = stacking
            .iter()
            .position(|strip| *strip == Strip::StatusBand)
            .expect("the band stacks in every order");
        let grab = stacking
            .iter()
            .position(|strip| *strip == Strip::GrabBar)
            .expect("the grab bar stacks in every order");
        assert_eq!(
            band.abs_diff(grab),
            1,
            "band must sit beside the grab bar: {edge:?}/{position:?} -> {stacking:?}"
        );
        // The grab bar stays at the free edge — the surface edge
        // opposite the screen anchor — wherever the strips pin.
        let expected_grab = if matches!(edge, Edge::Bottom) {
            0
        } else {
            stacking.len() - 1
        };
        assert_eq!(
            grab, expected_grab,
            "grab bar pins to the free edge: {edge:?}/{position:?} -> {stacking:?}"
        );
    }
}

// --- the idle name ---

#[test]
fn the_shell_basename_strips_the_directory() {
    assert_eq!(shell_basename("/usr/bin/fish"), "fish");
    assert_eq!(shell_basename("zsh"), "zsh");
}

// --- the grown contract (status-segments) ---

fn resolve_full(toggles: StatusToggles) -> StatusSegments {
    let home = home();
    let cwd = std::path::PathBuf::from("/home/jpt/src/oss/l1br3/ced2");
    let inputs = StatusInputs {
        home: Some(&home),
        cwd: Some(&cwd),
        shell: "/usr/bin/fish",
        profile_name: Some("dev"),
        zoom_adj: 2,
        zoom_step_mul_100: 100,
        foreground: Some("cargo"),
        branch: Some("ced2"),
        grid: Some((80, 24)),
        spawn_grid: None,
        tab_position: Some((2, 5)),
        clock: Some("14:32"),
    };
    resolve_segments(&inputs, toggles)
}

fn segment_present(segments: &StatusSegments, id: SegmentId) -> bool {
    match id {
        SegmentId::Process => !segments.process.is_empty(),
        SegmentId::Path => segments.path.is_some(),
        SegmentId::Branch => segments.branch.is_some(),
        SegmentId::Profile => segments.profile.is_some(),
        SegmentId::Grid => segments.grid.is_some(),
        SegmentId::Tab => segments.tab_position.is_some(),
        SegmentId::Zoom => segments.zoom_percent.is_some(),
        SegmentId::Clock => segments.clock.is_some(),
    }
}

fn hide(toggles: &mut StatusToggles, id: SegmentId) {
    let field = match id {
        SegmentId::Process => &mut toggles.process,
        SegmentId::Path => &mut toggles.path,
        SegmentId::Branch => &mut toggles.branch,
        SegmentId::Profile => &mut toggles.profile,
        SegmentId::Grid => &mut toggles.grid,
        SegmentId::Tab => &mut toggles.tab,
        SegmentId::Zoom => &mut toggles.zoom,
        SegmentId::Clock => &mut toggles.clock,
    };
    *field = false;
}

#[test]
fn the_clusters_pin_the_eight_segments_order() {
    assert_eq!(
        LEFT_CLUSTER,
        [SegmentId::Process, SegmentId::Path, SegmentId::Branch]
    );
    assert_eq!(
        RIGHT_CLUSTER,
        [
            SegmentId::Profile,
            SegmentId::Grid,
            SegmentId::Tab,
            SegmentId::Zoom,
            SegmentId::Clock
        ]
    );
}

#[test]
fn each_toggle_hides_exactly_its_segment() {
    let every: Vec<SegmentId> = LEFT_CLUSTER
        .iter()
        .chain(RIGHT_CLUSTER.iter())
        .copied()
        .collect();
    for hidden in &every {
        let mut toggles = StatusToggles::all();
        hide(&mut toggles, *hidden);
        let segments = resolve_full(toggles);
        assert!(
            !segment_present(&segments, *hidden),
            "{hidden:?} must hide behind its toggle"
        );
        for shown in every.iter().filter(|id| **id != *hidden) {
            assert!(
                segment_present(&segments, *shown),
                "{shown:?} must survive {hidden:?}'s toggle"
            );
        }
    }
}

#[test]
fn toggles_compose() {
    let mut toggles = StatusToggles::all();
    hide(&mut toggles, SegmentId::Branch);
    hide(&mut toggles, SegmentId::Clock);
    let segments = resolve_full(toggles);
    assert_eq!(segments.branch, None);
    assert_eq!(segments.clock, None);
    assert!(segment_present(&segments, SegmentId::Path));
    assert!(segment_present(&segments, SegmentId::Grid));
}

#[test]
fn the_new_segments_resolve_from_their_inputs() {
    let segments = resolve_full(StatusToggles::all());
    assert_eq!(segments.branch.as_deref(), Some("ced2"));
    assert_eq!(segments.grid, Some((80, 24)));
    assert_eq!(segments.tab_position, Some((2, 5)));
    assert_eq!(segments.clock.as_deref(), Some("14:32"));
}

// --- the grid condition (status-glance) ---

fn resolve_grid(
    spawn_grid: Option<(u16, u16)>,
    grid: Option<(u16, u16)>,
    toggles: StatusToggles,
) -> StatusSegments {
    let inputs = StatusInputs {
        home: None,
        cwd: None,
        shell: "/usr/bin/fish",
        profile_name: None,
        zoom_adj: 0,
        zoom_step_mul_100: 100,
        foreground: None,
        branch: None,
        grid,
        spawn_grid,
        tab_position: None,
        clock: None,
    };
    resolve_segments(&inputs, toggles)
}

#[test]
fn a_live_grid_equal_to_spawn_hides_the_segment_even_toggle_on() {
    // tmux renders its zoom flag only when true: a tab still sitting
    // at the grid it started at has nothing to say — hidden ≡
    // absent ≡ default, so the formatter never runs downstream.
    let segments = resolve_grid(Some((80, 24)), Some((80, 24)), StatusToggles::all());
    assert_eq!(segments.grid, None);
}

#[test]
fn either_dimension_off_spawn_shows_the_grid() {
    for live in [(81, 24), (80, 25), (120, 38)] {
        let segments = resolve_grid(Some((80, 24)), Some(live), StatusToggles::all());
        assert_eq!(
            segments.grid,
            Some(live),
            "live {live:?} must read as resized"
        );
    }
}

#[test]
fn the_grid_toggle_still_wins_over_the_resize_condition() {
    // Eligibility is the toggle's, appearance the condition's: an
    // off toggle hides the segment whatever the grids say. Zoom's
    // own condition is untouched by this module — still purely
    // `zoom_adj`-gated (zero adjustment hides, as pinned in the
    // zoom-arithmetic section above).
    let mut toggles = StatusToggles::all();
    toggles.grid = false;
    let segments = resolve_grid(Some((80, 24)), Some((120, 38)), toggles);
    assert_eq!(segments.grid, None);
}

#[test]
fn an_absent_spawn_record_shows_the_live_grid() {
    // No evidence of default means show — the honest reading for a
    // record that never landed, never a silently hidden segment.
    let segments = resolve_grid(None, Some((80, 24)), StatusToggles::all());
    assert_eq!(segments.grid, Some((80, 24)));
}

#[test]
fn copy_text_carries_each_segments_full_value() {
    // Copy pays no attention to display: the payload is the
    // segment's whole value — the path whole even where the band
    // would elide it (hit-testing feeds ids; elision never
    // reaches the payload).
    let segments = resolve_full(StatusToggles::all());
    assert_eq!(copy_text(SegmentId::Process, &segments), "cargo");
    assert_eq!(
        copy_text(SegmentId::Path, &segments),
        "~/src/oss/l1br3/ced2"
    );
    assert_eq!(copy_text(SegmentId::Branch, &segments), "ced2");
    assert_eq!(copy_text(SegmentId::Profile, &segments), "dev");
    assert_eq!(copy_text(SegmentId::Grid, &segments), "80×24");
    assert_eq!(copy_text(SegmentId::Tab, &segments), "2/5");
    assert_eq!(copy_text(SegmentId::Zoom, &segments), "+200%");
    assert_eq!(copy_text(SegmentId::Clock, &segments), "14:32");
}

#[test]
fn copy_text_of_an_absent_segment_is_empty() {
    let empty = StatusSegments::default();
    for id in LEFT_CLUSTER.iter().chain(RIGHT_CLUSTER.iter()) {
        assert_eq!(copy_text(*id, &empty), String::new());
    }
}

#[test]
fn segment_at_hits_only_the_rect_containing_the_point() {
    let rects = [
        (
            SegmentId::Process,
            Rectangle::new(Point::new(0.0, 0.0), Size::new(60.0, BAND_HEIGHT)),
        ),
        (
            SegmentId::Path,
            Rectangle::new(Point::new(72.0, 0.0), Size::new(90.0, BAND_HEIGHT)),
        ),
    ];
    assert_eq!(
        segment_at(&rects, Point::new(10.0, 14.0)),
        Some(SegmentId::Process)
    );
    assert_eq!(
        segment_at(&rects, Point::new(100.0, 27.9)),
        Some(SegmentId::Path)
    );
    // The gap between segments — and everything outside the row
    // (the bottom edge included; `contains` is half-open) —
    // belongs to no segment.
    assert_eq!(segment_at(&rects, Point::new(65.0, 14.0)), None);
    assert_eq!(segment_at(&rects, Point::new(-1.0, 14.0)), None);
    assert_eq!(segment_at(&rects, Point::new(10.0, BAND_HEIGHT)), None);
}

// --- the clock helpers ---

fn zoned(at: &str) -> jiff::Zoned {
    at.parse().expect("fixed zoned fixture")
}

#[test]
fn clock_text_formats_twenty_four_hour_minutes() {
    assert_eq!(
        clock_text(&zoned("2026-10-10T14:32:00+00:00[UTC]")),
        "14:32"
    );
    assert_eq!(
        clock_text(&zoned("2026-10-10T09:05:59+00:00[UTC]")),
        "09:05"
    );
}

#[test]
fn next_tick_delay_lands_on_the_minute_boundary() {
    assert_eq!(
        next_tick_delay(&zoned("2026-10-10T14:32:00+00:00[UTC]")),
        std::time::Duration::from_mins(1)
    );
    assert_eq!(
        next_tick_delay(&zoned("2026-10-10T14:32:30.500+00:00[UTC]")),
        std::time::Duration::from_millis(29_500)
    );
}

#[test]
fn the_clock_ticks_only_while_both_gates_hold() {
    assert!(clock_due(true, true));
    assert!(!clock_due(true, false));
    assert!(!clock_due(false, true));
    assert!(!clock_due(false, false));
}

// --- the git branch (real fixture trees under temp) ---

fn temp_root(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "dropterm-status-git-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temp repo root");
    root
}

fn write_fixture(path: &std::path::Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("fixture parent");
    }
    std::fs::write(path, content).expect("fixture write");
}

#[test]
fn a_ref_head_names_the_branch() {
    let root = temp_root("ref");
    write_fixture(&root.join(".git/HEAD"), "ref: refs/heads/main\n");
    assert_eq!(git_branch(Some(&root)), Some("main".to_string()));
}

#[test]
fn a_detached_head_shortens_to_seven_hex_characters() {
    let root = temp_root("detached");
    write_fixture(
        &root.join(".git/HEAD"),
        "0123456789abcdef0123456789abcdef01234567\n",
    );
    assert_eq!(git_branch(Some(&root)), Some("0123456".to_string()));
}

#[test]
fn a_gitdir_file_indirection_resolves_the_worktree_head() {
    let root = temp_root("indirect");
    write_fixture(
        &root.join("wt/.git"),
        &format!("gitdir: {}\n", root.join("real.git").display()),
    );
    write_fixture(&root.join("real.git/HEAD"), "ref: refs/heads/ced2\n");
    assert_eq!(git_branch(Some(&root.join("wt"))), Some("ced2".to_string()));
}

#[test]
fn a_relative_gitdir_resolves_against_the_cwd() {
    // Submodule-style gitdir files are relative to the working
    // copy's directory, not the process cwd.
    let root = temp_root("relative");
    write_fixture(&root.join("work/.git"), "gitdir: ../real.git\n");
    write_fixture(&root.join("real.git/HEAD"), "ref: refs/heads/main\n");
    assert_eq!(
        git_branch(Some(&root.join("work"))),
        Some("main".to_string())
    );
}

#[test]
fn outside_a_repo_or_without_a_cwd_there_is_no_branch() {
    let root = temp_root("bare");
    assert_eq!(git_branch(Some(&root)), None);
    assert_eq!(git_branch(None), None);
}

#[test]
fn malformed_heads_resolve_to_none() {
    let root = temp_root("malformed");
    for (label, head) in [
        ("empty", ""),
        ("ref-without-a-name", "ref: refs/heads/\n"),
        ("short-garbage", "abc\n"),
        ("non-hex-garbage", "not-a-sha!!\n"),
    ] {
        write_fixture(&root.join(".git/HEAD"), head);
        assert_eq!(git_branch(Some(&root)), None, "{label} must not parse");
    }
}

// --- the widget's construction pin (Task 2) ---

#[test]
fn the_band_constructs_from_plain_resolved_data_only() {
    // The construction pin: the widget takes the resolved
    // segments and a font — no terminal handle, no engine import
    // — so the render path cannot read `/proc` by construction;
    // the app-side snapshot refresh stays the lone caller of the
    // engine's comm read (criterion 5).
    let segments = StatusSegments {
        running: true,
        process: "cargo".to_string(),
        path: Some("~/src".to_string()),
        profile: Some("dev".to_string()),
        zoom_percent: Some(20),
        branch: None,
        grid: None,
        tab_position: None,
        clock: None,
    };
    let band: StatusBar<()> = StatusBar::new(
        segments.clone(),
        cosmic::iced::Font::default(),
        None,
        BandInward::from_edge(Edge::Top),
        |_| (),
    );
    assert_eq!(band.segments, segments);
}

// --- the hover reveal (status-glance criterion 3) ---

fn reveal_rects() -> Vec<(SegmentId, Rectangle)> {
    vec![
        (
            SegmentId::Process,
            Rectangle::new(Point::new(0.0, 0.0), Size::new(60.0, BAND_HEIGHT)),
        ),
        (
            SegmentId::Path,
            Rectangle::new(Point::new(72.0, 0.0), Size::new(50.0, BAND_HEIGHT)),
        ),
        (
            SegmentId::Branch,
            Rectangle::new(Point::new(134.0, 0.0), Size::new(40.0, BAND_HEIGHT)),
        ),
    ]
}

fn reveal_bounds() -> Rectangle {
    Rectangle::new(Point::new(0.0, 0.0), Size::new(400.0, BAND_HEIGHT))
}

#[test]
fn the_reveal_arms_only_on_the_path_rect_while_elided() {
    let reveal = hover_reveal(
        &reveal_rects(),
        reveal_bounds(),
        Some(Point::new(100.0, 14.0)),
        true,
        "~/src/oss/l1br3/ced2",
    )
    .expect("an elided path under the cursor arms the reveal");
    assert_eq!(reveal.anchor, reveal_rects()[1].1);
    assert_eq!(reveal.value, "~/src/oss/l1br3/ced2");
}

#[test]
fn an_un_elided_path_arms_nothing() {
    // An un-elided path's display already is its value.
    assert_eq!(
        hover_reveal(
            &reveal_rects(),
            reveal_bounds(),
            Some(Point::new(100.0, 14.0)),
            false,
            "~/src"
        ),
        None
    );
}

#[test]
fn other_segments_and_the_gaps_arm_nothing() {
    for point in [
        Point::new(20.0, 14.0),  // the process rect
        Point::new(150.0, 14.0), // the branch rect
        Point::new(65.0, 14.0),  // the gap between segments
    ] {
        assert_eq!(
            hover_reveal(
                &reveal_rects(),
                reveal_bounds(),
                Some(point),
                true,
                "~/src/oss"
            ),
            None,
            "{point:?} must not arm the reveal"
        );
    }
}

#[test]
fn a_cursor_outside_the_band_or_absent_arms_nothing() {
    for point in [
        Some(Point::new(100.0, -1.0)),
        Some(Point::new(100.0, BAND_HEIGHT)),
        Some(Point::new(-1.0, 14.0)),
        Some(Point::new(500.0, 14.0)),
        None,
    ] {
        assert_eq!(
            hover_reveal(&reveal_rects(), reveal_bounds(), point, true, "~/src/oss"),
            None,
            "{point:?} must not arm the reveal"
        );
    }
}

#[test]
fn a_missing_path_rect_arms_nothing() {
    // A Gone path yields no rect at all — nothing to rest on.
    let rects: Vec<(SegmentId, Rectangle)> = reveal_rects()
        .into_iter()
        .filter(|(id, _)| *id != SegmentId::Path)
        .collect();
    assert_eq!(
        hover_reveal(
            &rects,
            reveal_bounds(),
            Some(Point::new(100.0, 14.0)),
            true,
            "~/src"
        ),
        None
    );
}

#[test]
fn the_reveal_carries_the_press_payload_and_survives_the_flash() {
    // The reveal's value is `copy_text(Path)` — the full
    // un-elided value a press copies — and the flash coexists
    // with it by construction: arming never consults `copied`, so
    // the reveal keeps the full value while the display shows ✓.
    let segments = StatusSegments {
        path: Some("~/src/oss/l1br3/ced2".to_string()),
        ..StatusSegments::default()
    };
    let reveal = hover_reveal(
        &reveal_rects(),
        reveal_bounds(),
        Some(Point::new(100.0, 14.0)),
        true,
        &copy_text(SegmentId::Path, &segments),
    )
    .expect("the reveal arms");
    assert_eq!(reveal.value, copy_text(SegmentId::Path, &segments));
    assert_eq!(
        display_text(Some(SegmentId::Path), SegmentId::Path, &reveal.value),
        "✓",
        "the display flashes while the reveal keeps the value"
    );
}

#[test]
fn the_inward_direction_follows_the_anchor_edge() {
    // Away from the screen edge the surface drops from: the band
    // sits at the free edge, the reveal floats into the surface.
    assert_eq!(BandInward::from_edge(Edge::Top), BandInward::Up);
    assert_eq!(BandInward::from_edge(Edge::Bottom), BandInward::Down);
}
