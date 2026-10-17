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
        running_elapsed: None,
        frozen: None,
        scrolled_back: None,
        ssh_destination: None,
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
/// 20-char path at 10 px per char (200 px whole), and all eight
/// droppable segments measured (gap-inclusive costs 42/42/52/42/32/
/// 42/42/42 with `SEGMENT_GAP` = 12 — 336 total).
fn fit_widths() -> Vec<(SegmentId, f32)> {
    vec![
        (SegmentId::Branch, 30.0),
        (SegmentId::Duration, 30.0),
        (SegmentId::Profile, 40.0),
        (SegmentId::Grid, 30.0),
        (SegmentId::Tab, 20.0),
        (SegmentId::Zoom, 30.0),
        (SegmentId::Scrolled, 30.0),
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
    // the branch beside its path, the duration after its command,
    // the scrolled chip after the duration (it answers "what's
    // happening now" — it must survive while the command's own
    // narration stands), the clock to the bitter end. Process and
    // path are absent by construction: the disc never yields, and
    // the path elides before anything else gives way.
    assert_eq!(
        DROP_ORDER,
        [
            SegmentId::Grid,
            SegmentId::Tab,
            SegmentId::Zoom,
            SegmentId::Profile,
            SegmentId::Branch,
            SegmentId::Duration,
            SegmentId::Scrolled,
            SegmentId::Clock,
        ]
    );
}

#[test]
fn duration_drops_after_branch_and_before_clock() {
    // The seat pin (command-lifecycle criterion 5): with only
    // Branch, Duration, and Clock measured, starvation drops the
    // branch first, the duration second, and the clock survives —
    // DROP_ORDER's precedence restricted to the present.
    let widths = vec![
        (SegmentId::Branch, 30.0),
        (SegmentId::Duration, 30.0),
        (SegmentId::Clock, 30.0),
    ];
    // fixed 100; all three consumed 126 → room -74 past Gone; the
    // branch's 42 and the duration's 42 free 10 px of room — one
    // character's worth, beside the surviving clock.
    let fit = band_fit(152.0, 100.0, &widths, fit_path());
    assert_eq!(fit.dropped, vec![SegmentId::Branch, SegmentId::Duration]);
    assert_eq!(fit.path, PathFit::Elided(1));
}

#[test]
fn scrolled_drops_after_duration_and_before_clock() {
    // The seat pin (status-scrolled criterion 3): with only
    // Duration, Scrolled, and Clock measured, starvation drops the
    // duration first, the chip second, and the clock survives —
    // DROP_ORDER's precedence restricted to the present (the chip
    // rides the order only when present, like every segment).
    let widths = vec![
        (SegmentId::Duration, 30.0),
        (SegmentId::Scrolled, 30.0),
        (SegmentId::Clock, 30.0),
    ];
    // fixed 100; all three consumed 126 → room -74 past Gone; the
    // duration's 42 and the chip's 42 free 10 px of room — one
    // character's worth, beside the surviving clock.
    let fit = band_fit(152.0, 100.0, &widths, fit_path());
    assert_eq!(fit.dropped, vec![SegmentId::Duration, SegmentId::Scrolled]);
    assert_eq!(fit.path, PathFit::Elided(1));
}

#[test]
fn zero_pressure_renders_everything() {
    // 636 px of consumption (fixed + eight gaps-and-widths + the
    // whole path) under 700 px: nothing drops, nothing elides.
    let fit = band_fit(700.0, 100.0, &fit_widths(), fit_path());
    assert_eq!(fit.path, PathFit::Full);
    assert!(fit.dropped.is_empty(), "not empty: {:?}", fit.dropped);
}

#[test]
fn pressure_elides_the_path_before_any_segment_drops() {
    // 500 px leaves 64 px of room for the 200 px path: elide to
    // 6 chars — but nothing drops while any of the path survives.
    let fit = band_fit(500.0, 100.0, &fit_widths(), fit_path());
    assert_eq!(fit.path, PathFit::Elided(6));
    assert!(fit.dropped.is_empty(), "not empty: {:?}", fit.dropped);
}

#[test]
fn past_full_elision_segments_drop_one_at_a_time_in_order() {
    // 427 px cannot host even a fully elided path (room below one
    // character): the grid goes first, and the path re-expands
    // into the freed 42 px — three characters' worth.
    let fit = band_fit(427.0, 100.0, &fit_widths(), fit_path());
    assert_eq!(fit.dropped, vec![SegmentId::Grid]);
    assert_eq!(fit.path, PathFit::Elided(3));

    // 387 px costs a second drop: tab follows grid, and the path
    // re-expands again (25 px of room, two characters).
    let fit = band_fit(387.0, 100.0, &fit_widths(), fit_path());
    assert_eq!(fit.dropped, vec![SegmentId::Grid, SegmentId::Tab]);
    assert_eq!(fit.path, PathFit::Elided(2));
}

#[test]
fn absent_segments_are_skipped_in_the_drop_order() {
    // No grid segment (toggle off, or at spawn dimensions): 342 px
    // starves the band past full elision, and the first yield is
    // the tab — the absent grid is skipped, not waited on.
    let widths: Vec<(SegmentId, f32)> = fit_widths()
        .into_iter()
        .filter(|(id, _)| *id != SegmentId::Grid)
        .collect();
    let fit = band_fit(342.0, 100.0, &widths, fit_path());
    assert_eq!(fit.dropped, vec![SegmentId::Tab, SegmentId::Zoom]);
    assert_eq!(fit.path, PathFit::Elided(2));
}

#[test]
fn the_path_re_expands_monotonically_as_drops_free_width() {
    // Widths chosen so exactly `k` drops are forced (the k-th
    // survivor's room sits at 10 + 5(k-1) px — one character plus a
    // growing slack that stays below the next drop's freed width,
    // while the (k-1)-th sits below one character): the path's char
    // budget never shrinks as drops free width.
    let mut prev_chars = 0;
    let fixture = fit_widths();
    for k in 1..=8 {
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
            reason = "a loop index of eight, exact in f32"
        )]
        let room = 10.0 + 5.0 * (k as f32 - 1.0);
        let width = 100.0 + 336.0 + room - freed;
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
    // Zero width: all eight droppables give way in order and the
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
            SegmentId::Duration,
            SegmentId::Scrolled,
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
    assert!(fit.dropped.is_empty(), "not empty: {:?}", fit.dropped);
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
        running_elapsed: Some(Duration::from_secs(161)),
        frozen: None,
        scrolled_back: Some(12),
        ssh_destination: None,
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
        SegmentId::Duration => segments.running_elapsed.is_some() || segments.frozen.is_some(),
        SegmentId::Scrolled => segments.scrolled.is_some(),
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
        SegmentId::Duration => &mut toggles.duration,
        SegmentId::Scrolled => &mut toggles.scrolled,
    };
    *field = false;
}

#[test]
fn the_clusters_pin_the_ten_segments_order() {
    // The duration sits in the left cluster's second seat — right
    // beside the process it times (OQ-2) — but carries the dimmed
    // alpha of a meta narration, not identity (the draw's rule).
    // The scrolled chip sits between Zoom and Clock
    // (status-scrolled OQ-2): state of the view, beside time.
    assert_eq!(
        LEFT_CLUSTER,
        [
            SegmentId::Process,
            SegmentId::Duration,
            SegmentId::Path,
            SegmentId::Branch
        ]
    );
    assert_eq!(
        RIGHT_CLUSTER,
        [
            SegmentId::Profile,
            SegmentId::Grid,
            SegmentId::Tab,
            SegmentId::Zoom,
            SegmentId::Scrolled,
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
    assert_eq!(segments.running_elapsed, Some(Duration::from_secs(161)));
    assert_eq!(segments.scrolled, Some(12));
}

// --- the duration segment (command-lifecycle) ---
//
// The ninth segment: live elapsed beside the process name while a
// witnessed start runs, the frozen (name, duration) pair after it
// finishes (OQ-1), and nothing when the timer has nothing honest —
// the both-edges rule, resolved like every other segment.

fn resolve_duration(
    foreground: Option<&str>,
    running_elapsed: Option<Duration>,
    frozen: Option<(&str, Duration)>,
    toggles: StatusToggles,
) -> StatusSegments {
    let inputs = StatusInputs {
        home: None,
        cwd: None,
        shell: "/usr/bin/fish",
        profile_name: None,
        zoom_adj: 0,
        zoom_step_mul_100: 100,
        foreground,
        branch: None,
        grid: None,
        spawn_grid: None,
        tab_position: None,
        clock: None,
        running_elapsed,
        frozen,
        scrolled_back: None,
        ssh_destination: None,
    };
    resolve_segments(&inputs, toggles)
}

#[test]
fn a_running_command_resolves_its_live_elapsed() {
    let segments = resolve_duration(
        Some("cargo"),
        Some(Duration::from_secs(161)),
        None,
        StatusToggles::all(),
    );
    assert_eq!(segments.running_elapsed, Some(Duration::from_secs(161)));
    assert_eq!(segments.frozen, None);
    assert_eq!(copy_text(SegmentId::Duration, &segments), "2:41");
}

#[test]
fn a_finished_command_resolves_the_frozen_pair() {
    // OQ-1's pairing: after the finish the process segment shows the
    // shell again, so the frozen duration carries the finished
    // command's name — `cargo 2:41`, never a bare duration that
    // would read as the shell's.
    let segments = resolve_duration(
        None,
        None,
        Some(("cargo", Duration::from_secs(161))),
        StatusToggles::all(),
    );
    assert_eq!(segments.running_elapsed, None);
    assert_eq!(
        segments
            .frozen
            .as_ref()
            .map(|(name, duration)| (name.as_str(), *duration)),
        Some(("cargo", Duration::from_secs(161)))
    );
    assert_eq!(copy_text(SegmentId::Duration, &segments), "cargo 2:41");
}

#[test]
fn an_unnamed_frozen_command_degrades_to_the_bare_duration() {
    // The pairing's honest floor: a tab whose run was never sampled
    // (a background tab's finished command, first shown on switch)
    // still reports its duration — the engine timed it — with no
    // name to lie about.
    let segments = resolve_duration(
        None,
        None,
        Some(("", Duration::from_secs(4))),
        StatusToggles::all(),
    );
    assert_eq!(copy_text(SegmentId::Duration, &segments), "0:04");
}

#[test]
fn an_unwitnessed_start_resolves_no_duration_segment() {
    // Born mid-run (the profile-tab spawn case): the tab runs and
    // the disc fills, but the timer never learned that command's
    // start — no elapsed, no frozen pair, never a guess.
    let segments = resolve_duration(Some("cargo"), None, None, StatusToggles::all());
    assert!(segments.running);
    assert_eq!(segments.running_elapsed, None);
    assert_eq!(segments.frozen, None);
    assert_eq!(copy_text(SegmentId::Duration, &segments), "");
}

#[test]
fn the_duration_toggle_hides_the_segment_and_a_mid_run_on_shows_live() {
    // Hidden ≡ absent, like every segment; the engine's timestamps
    // keep running regardless (Task 2's always-on assumption), so a
    // mid-command toggle-on resolves the live elapsed immediately —
    // the resolver's inputs are fresh at the echo's view pass.
    let mut toggles = StatusToggles::all();
    toggles.duration = false;
    let hidden = resolve_duration(Some("cargo"), Some(Duration::from_secs(4)), None, toggles);
    assert_eq!(hidden.running_elapsed, None);
    assert_eq!(hidden.frozen, None);
    let shown = resolve_duration(
        Some("cargo"),
        Some(Duration::from_secs(4)),
        None,
        StatusToggles::all(),
    );
    assert_eq!(shown.running_elapsed, Some(Duration::from_secs(4)));
}

#[test]
fn the_duration_tick_exists_only_while_every_gate_holds() {
    // The truth table (criterion 4): band × segment × active tab
    // running × surface visible — any false kills the tick.
    for band in [false, true] {
        for show in [false, true] {
            for running in [false, true] {
                for visible in [false, true] {
                    assert_eq!(
                        duration_due(band, show, running, visible),
                        band && show && running && visible,
                        "duration_due({band}, {show}, {running}, {visible})"
                    );
                }
            }
        }
    }
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
        running_elapsed: None,
        frozen: None,
        scrolled_back: None,
        ssh_destination: None,
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

// --- the scrolled chip (status-scrolled) ---
//
// The tenth segment: the viewport's lines back from the bottom —
// `↑12` — absent at the bottom (the same indistinguishable `None`
// as hidden), and the band's one deliberate action chip: its
// click jumps to the bottom instead of copying (a ruling
// recorded, not an erosion — the chunk's boundary).

fn resolve_scrolled(scrolled_back: Option<u16>, toggles: StatusToggles) -> StatusSegments {
    let inputs = StatusInputs {
        home: None,
        cwd: None,
        shell: "/usr/bin/fish",
        profile_name: None,
        zoom_adj: 0,
        zoom_step_mul_100: 100,
        foreground: None,
        branch: None,
        grid: None,
        spawn_grid: None,
        tab_position: None,
        clock: None,
        running_elapsed: None,
        frozen: None,
        scrolled_back,
        ssh_destination: None,
    };
    resolve_segments(&inputs, toggles)
}

#[test]
fn an_at_bottom_viewport_resolves_no_chip_even_toggle_on() {
    // The filter: zero lines back is the bottom — the resting
    // state shows nothing, the conditional-appearance rule the
    // zoom and grid segments already follow.
    let segments = resolve_scrolled(Some(0), StatusToggles::all());
    assert_eq!(segments.scrolled, None);
}

#[test]
fn an_off_bottom_viewport_resolves_its_line_count() {
    let segments = resolve_scrolled(Some(12), StatusToggles::all());
    assert_eq!(segments.scrolled, Some(12));
    // The display (OQ-1): the arrow, then the count — "12 up".
    assert_eq!(scrolled_display(12), "↑12");
}

#[test]
fn the_scrolled_toggle_hides_the_chip_whatever_the_offset() {
    // Hidden ≡ absent, like every segment.
    let mut toggles = StatusToggles::all();
    toggles.scrolled = false;
    let segments = resolve_scrolled(Some(12), toggles);
    assert_eq!(segments.scrolled, None);
}

#[test]
fn an_absurd_saturated_offset_still_resolves() {
    // The getter saturates into `u16`; a count at the ceiling (a
    // scrollback no human made) still resolves — the filter only
    // ever rejects zero.
    let segments = resolve_scrolled(Some(u16::MAX), StatusToggles::all());
    assert_eq!(segments.scrolled, Some(u16::MAX));
    assert_eq!(scrolled_display(u16::MAX), format!("↑{}", u16::MAX));
}

#[test]
fn only_the_scrolled_chip_is_an_action_chip() {
    // The click routing (criterion 2): the chip's press takes the
    // jump branch ahead of the copy path; every other segment
    // keeps the copy verb.
    assert!(click_jumps(SegmentId::Scrolled));
    for id in LEFT_CLUSTER.iter().chain(RIGHT_CLUSTER.iter()) {
        if *id != SegmentId::Scrolled {
            assert!(!click_jumps(*id), "{id:?} must keep the copy verb");
        }
    }
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
    assert_eq!(copy_text(SegmentId::Duration, &segments), "2:41");
    // The action chip has no copyable value — the jump is the verb
    // (status-scrolled spec); the empty payload is defensive.
    assert_eq!(copy_text(SegmentId::Scrolled, &segments), "");
}

#[test]
fn copy_text_of_an_absent_segment_is_empty() {
    let empty = StatusSegments::default();
    for id in LEFT_CLUSTER.iter().chain(RIGHT_CLUSTER.iter()) {
        assert_eq!(copy_text(*id, &empty), String::new());
    }
}

#[test]
fn the_scrolled_chip_press_never_reaches_copy_text() {
    // The branch-order pin from the payload side: even a present,
    // counted chip carries no copy value, so a mis-routed press
    // would still write nothing — the jump is the only verb.
    let segments = resolve_full(StatusToggles::all());
    assert_eq!(segments.scrolled, Some(12));
    assert_eq!(copy_text(SegmentId::Scrolled, &segments), "");
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

// --- the ssh stand-in (status-remote) ---

/// The stand-in's resolver shape: the path seat and branch fed
/// beside the token the engine's argv read produced (or `None` —
/// not ssh, unreadable, unparseable — the states the band cannot
/// tell apart, by design).
fn resolve_ssh(
    ssh_destination: Option<&str>,
    cwd: Option<&Path>,
    branch: Option<&str>,
    toggles: StatusToggles,
) -> StatusSegments {
    let inputs = StatusInputs {
        home: Some(&home()),
        cwd,
        shell: "/bin/bash",
        profile_name: None,
        zoom_adj: 0,
        zoom_step_mul_100: 100,
        foreground: Some("ssh"),
        branch,
        grid: None,
        spawn_grid: None,
        tab_position: None,
        clock: None,
        running_elapsed: None,
        frozen: None,
        scrolled_back: None,
        ssh_destination,
    };
    resolve_segments(&inputs, toggles)
}

#[test]
fn a_bare_host_token_wins() {
    assert_eq!(ssh_destination(&["ssh", "box"]), Some("box".to_string()));
}

#[test]
fn a_user_at_host_token_wins() {
    assert_eq!(
        ssh_destination(&["ssh", "git@github.com"]),
        Some("git@github.com".to_string())
    );
}

#[test]
fn a_separate_option_value_is_consumed_not_won() {
    assert_eq!(
        ssh_destination(&["ssh", "-p", "2222", "box"]),
        Some("box".to_string())
    );
}

#[test]
fn a_key_option_leaves_the_destination_standing() {
    assert_eq!(
        ssh_destination(&["ssh", "-i", "key", "git@host"]),
        Some("git@host".to_string())
    );
}

#[test]
fn options_after_the_host_let_it_still_win() {
    // The pinned trailing-options shape: `-W`'s `%h:%p` is a
    // consumed value — the %-pattern class never wins (the spec's
    // own rule).
    assert_eq!(
        ssh_destination(&["ssh", "box", "-W", "%h:%p"]),
        Some("box".to_string())
    );
}

#[test]
fn a_url_shaped_token_passes_verbatim() {
    assert_eq!(
        ssh_destination(&["ssh", "ssh://user@host:22"]),
        Some("ssh://user@host:22".to_string())
    );
}

#[test]
fn options_only_yields_none() {
    assert_eq!(ssh_destination(&["ssh", "-v", "-A", "-4"]), None);
}

#[test]
fn a_program_only_or_empty_argv_yields_none() {
    assert_eq!(ssh_destination(&["ssh"]), None);
    assert_eq!(ssh_destination(&[]), None);
}

#[test]
fn an_attached_option_value_never_wins() {
    // `-p2222` carries its value inside the cluster: one option
    // token, nothing separate consumed, the host still stands.
    assert_eq!(
        ssh_destination(&["ssh", "-p2222", "box"]),
        Some("box".to_string())
    );
}

#[test]
fn every_value_taking_short_consumes_its_argument() {
    // The grounded ssh(1) set (9.6's usage line — the superset the
    // chunk's table rides in): any of these ahead of the host
    // leaves the host standing; a miss would let the option's
    // value masquerade as the destination when options trail.
    for flag in [
        "-B", "-b", "-c", "-D", "-E", "-e", "-F", "-I", "-i", "-J", "-L", "-l", "-m", "-O", "-o",
        "-P", "-p", "-Q", "-R", "-S", "-W", "-w",
    ] {
        let argv = ["ssh", flag, "value", "box"];
        assert_eq!(
            ssh_destination(&argv),
            Some("box".to_string()),
            "the consuming flag {flag}"
        );
    }
}

#[test]
fn an_equals_long_form_is_self_contained() {
    assert_eq!(
        ssh_destination(&["ssh", "--port=2222", "box"]),
        Some("box".to_string())
    );
}

#[test]
fn a_bare_long_form_consumes_rather_than_misreads_its_value() {
    // This ssh (9.6) has no long forms; 9.8's flag-form spellings
    // are unknown from here — the degrade-safe bias consumes, so an
    // uncertifiable value is never shown as the host (the spec's
    // floor: None, the local path stands).
    assert_eq!(ssh_destination(&["ssh", "--no-such-flag", "value"]), None);
    // A host ahead of the long form still wins — nothing follows
    // for it to consume.
    assert_eq!(
        ssh_destination(&["ssh", "box", "--verbose"]),
        Some("box".to_string())
    );
}

#[test]
fn a_double_dash_ends_the_option_walk() {
    // The argv convention: after `--`, every token is an operand —
    // even one shaped like a flag (a host literally named `-x`).
    assert_eq!(
        ssh_destination(&["ssh", "--", "box"]),
        Some("box".to_string())
    );
    assert_eq!(
        ssh_destination(&["ssh", "--", "-x"]),
        Some("-x".to_string())
    );
}

#[test]
fn a_stood_in_path_seat_shows_and_copies_the_token() {
    let segments = resolve_ssh(
        Some("box"),
        Some(Path::new("/home/jpt/src")),
        Some("main"),
        StatusToggles::all(),
    );
    assert_eq!(segments.path.as_deref(), Some("box"));
    // The full-value rule holds: the token is the value.
    assert_eq!(copy_text(SegmentId::Path, &segments), "box");
    assert_eq!(
        segments.branch, None,
        "the branch hides while stood-in (OQ-1: the stale-name rule)"
    );
}

#[test]
fn the_stood_in_token_passes_verbatim_no_tilde_collapse() {
    // OQ-2: the token exactly as typed — the `~` collapse is the
    // local path's treatment, never the stand-in's.
    let segments = resolve_ssh(
        Some("/home/jpt"),
        Some(Path::new("/srv")),
        None,
        StatusToggles::all(),
    );
    assert_eq!(segments.path.as_deref(), Some("/home/jpt"));
}

#[test]
fn an_unparseable_argv_degrades_to_todays_path_and_branch() {
    // Not-ssh is the same inputs — indistinguishable from a failed
    // read, which is the point: degradation is today's exact
    // resolution, never a wrong host.
    let segments = resolve_ssh(
        None,
        Some(Path::new("/home/jpt/src")),
        Some("main"),
        StatusToggles::all(),
    );
    assert_eq!(segments.path.as_deref(), Some("~/src"));
    assert_eq!(segments.branch.as_deref(), Some("main"));
}

#[test]
fn a_path_toggle_off_tab_hides_the_stand_in_and_keeps_the_branch_hidden() {
    // Hidden means never computed: the app's read gate keeps this
    // state unreachable (no token can arrive while the path seat is
    // off), and the resolver's suppression keys on the ssh fact —
    // the branch data is stale whether or not the seat displays.
    let toggles = StatusToggles {
        path: false,
        ..StatusToggles::all()
    };
    let segments = resolve_ssh(Some("box"), Some(Path::new("/home/jpt/src")), None, toggles);
    assert_eq!(segments.path, None);
    assert_eq!(segments.branch, None);
}

#[test]
fn the_ssh_read_gate_requires_band_path_and_an_ssh_comm() {
    // `clock_due`'s shape with the comm gate: each gate alone drops
    // the read — hidden means never gathered, and only the ssh comm
    // certifies a destination exists to read.
    assert!(ssh_read_due(true, true, true));
    assert!(!ssh_read_due(false, true, true));
    assert!(!ssh_read_due(true, false, true));
    assert!(!ssh_read_due(true, true, false));
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

// --- the command timer (command-lifecycle) ---
//
// Pure policy over the foreground diff the engine already computes:
// a start counts only when the timer witnessed the idle→running
// crossing, a finish freezes the elapsed, and a command born
// mid-observation never yields a duration — the both-edges honesty
// rule.

#[test]
fn a_witnessed_start_sets_the_start_and_the_elapsed_grows_with_now() {
    let t0 = Instant::now();
    let mut timer = CommandTimer::default();
    timer.step(false, t0);
    timer.step(true, t0 + Duration::from_secs(1));
    assert_eq!(
        timer.live_elapsed(t0 + Duration::from_secs(3)),
        Some(Duration::from_secs(2))
    );
    assert_eq!(
        timer.live_elapsed(t0 + Duration::from_secs(11)),
        Some(Duration::from_secs(10))
    );
}

#[test]
fn a_witnessed_finish_freezes_the_elapsed_and_ends_the_live_read() {
    let t0 = Instant::now();
    let mut timer = CommandTimer::default();
    timer.step(false, t0);
    timer.step(true, t0);
    timer.step(false, t0 + Duration::from_secs(3));
    assert_eq!(timer.last_duration(), Some(Duration::from_secs(3)));
    assert_eq!(timer.live_elapsed(t0 + Duration::from_secs(10)), None);
}

#[test]
fn the_next_start_clears_the_freeze_and_re_arms() {
    let t0 = Instant::now();
    let mut timer = CommandTimer::default();
    timer.step(false, t0);
    timer.step(true, t0);
    timer.step(false, t0 + Duration::from_secs(3));
    timer.step(true, t0 + Duration::from_secs(10));
    assert_eq!(
        timer.last_duration(),
        None,
        "the next start clears the freeze"
    );
    assert_eq!(
        timer.live_elapsed(t0 + Duration::from_secs(12)),
        Some(Duration::from_secs(2))
    );
}

#[test]
fn same_state_repeats_are_no_ops() {
    let t0 = Instant::now();
    let mut timer = CommandTimer::default();
    // Idle repeats on a fresh timer arm nothing.
    timer.step(false, t0);
    timer.step(false, t0 + Duration::from_secs(5));
    assert_eq!(timer.live_elapsed(t0 + Duration::from_secs(6)), None);
    // Running repeats keep the original start — the elapsed measures
    // from the crossing, not the last observation.
    timer.step(true, t0 + Duration::from_secs(10));
    timer.step(true, t0 + Duration::from_secs(15));
    assert_eq!(
        timer.live_elapsed(t0 + Duration::from_secs(20)),
        Some(Duration::from_secs(10))
    );
}

#[test]
fn a_timer_born_mid_run_never_reports_that_command() {
    // The profile-tab spawn case: the first observation is already
    // running, so the start went unwitnessed — no live elapsed, and
    // the finish freezes nothing rather than guessing.
    let t0 = Instant::now();
    let mut timer = CommandTimer::default();
    timer.step(true, t0);
    assert_eq!(timer.live_elapsed(t0 + Duration::from_secs(4)), None);
    timer.step(false, t0 + Duration::from_secs(5));
    assert_eq!(timer.last_duration(), None);
}

#[test]
fn default_is_a_clean_idle_timer() {
    let timer = CommandTimer::default();
    assert_eq!(timer.live_elapsed(Instant::now()), None);
    assert_eq!(timer.last_duration(), None);
}

#[test]
fn format_duration_renders_mss_under_an_hour() {
    assert_eq!(format_duration(Duration::from_secs(4)), "0:04");
    assert_eq!(format_duration(Duration::from_secs(161)), "2:41");
    assert_eq!(format_duration(Duration::from_secs(3599)), "59:59");
}

#[test]
fn format_duration_renders_hmmss_at_and_over_an_hour() {
    assert_eq!(format_duration(Duration::from_secs(3600)), "1:00:00");
    assert_eq!(format_duration(Duration::from_secs(3723)), "1:02:03");
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
        running_elapsed: None,
        frozen: None,
        scrolled: None,
    };
    let band: StatusBar<()> = StatusBar::new(
        segments.clone(),
        cosmic::iced::Font::default(),
        None,
        |_| (),
    );
    assert_eq!(band.segments, segments);
}

// --- the inline reveal (status-glance criterion 3, the OQ-1
// inline ruling) ---
//
// Hovering the elided path swaps its display to the expanded form
// in place — the ✓-flash text-swap mechanism driven by the arming
// transition — with the branch lending its seat. The press target
// is the displayed strip itself (wherever it currently extends),
// and the payload stays `copy_text(Path)`'s full value whatever
// the display shows.

fn reveal_path_rect() -> Rectangle {
    Rectangle::new(Point::new(72.0, 0.0), Size::new(50.0, BAND_HEIGHT))
}

#[test]
fn the_reveal_arms_only_on_the_current_path_rect_while_elided() {
    let rect = reveal_path_rect();
    assert!(reveal_armed(
        Some(rect),
        Some(Point::new(100.0, 14.0)),
        true
    ));
    // The armed (expanded) rect is a superset of the elided one —
    // the swap never disarms itself mid-rest.
    let expanded = Rectangle::new(rect.position(), Size::new(200.0, BAND_HEIGHT));
    assert!(reveal_armed(
        Some(expanded),
        Some(Point::new(250.0, 14.0)),
        true
    ));
}

#[test]
fn an_un_elided_path_never_arms() {
    // An un-elided path's display already is its value.
    assert!(!reveal_armed(
        Some(reveal_path_rect()),
        Some(Point::new(100.0, 14.0)),
        false
    ));
}

#[test]
fn other_segments_gaps_and_absence_arm_nothing() {
    let rect = reveal_path_rect();
    for point in [
        Some(Point::new(20.0, 14.0)),         // the process strip
        Some(Point::new(65.0, 14.0)),         // the gap before the path
        Some(Point::new(150.0, 14.0)),        // the branch beyond the rect
        Some(Point::new(100.0, -1.0)),        // above the band
        Some(Point::new(100.0, BAND_HEIGHT)), // below the band
        None,                                 // no cursor at all
    ] {
        assert!(
            !reveal_armed(Some(rect), point, true),
            "{point:?} must not arm the reveal"
        );
    }
}

#[test]
fn a_missing_path_rect_never_arms() {
    // A Gone path yields no rect at all — nothing to rest on.
    assert!(!reveal_armed(None, Some(Point::new(100.0, 14.0)), true));
}

#[test]
fn the_armed_display_borrows_the_branchs_seat() {
    // 602 px: the resting fit elides the path to 16 chars beside
    // the branch; lending the branch's 42 px frees the whole value
    // — the armed display is the un-elided path.
    let resting = band_fit(602.0, 100.0, &fit_widths(), fit_path());
    assert_eq!(resting.path, PathFit::Elided(16));
    let display = reveal_display(
        "~/src/oss/l1br3/ced2",
        602.0,
        100.0,
        &fit_widths(),
        fit_path(),
    );
    assert_eq!(display, "~/src/oss/l1br3/ced2");
}

#[test]
fn a_band_too_narrow_even_for_the_loan_shows_the_longest_elision() {
    // 362 px: the resting fit drops the grid, tab, and zoom and
    // elides to four characters; the armed fit — the branch's seat
    // lent on top of the resting drops — still shows only a longer
    // elision, never an overflow.
    let resting = band_fit(362.0, 100.0, &fit_widths(), fit_path());
    assert_eq!(
        resting.dropped,
        vec![SegmentId::Grid, SegmentId::Tab, SegmentId::Zoom]
    );
    assert_eq!(resting.path, PathFit::Elided(4));
    let survivors: Vec<(SegmentId, f32)> = fit_widths()
        .into_iter()
        .filter(|(id, _)| !resting.dropped.contains(id))
        .collect();
    let display = reveal_display("~/src/oss/l1br3/ced2", 362.0, 100.0, &survivors, fit_path());
    assert_eq!(display, "…r3/ced2");
}

#[test]
fn the_press_payload_stays_the_full_value_whatever_the_display() {
    // The display may itself be an elision on a truly narrow band;
    // the press payload never follows it — `copy_text(Path)` is
    // the whole value, the v1 contract the reveal rides on.
    let segments = StatusSegments {
        path: Some("~/src/oss/l1br3/ced2".to_string()),
        ..StatusSegments::default()
    };
    assert_eq!(
        copy_text(SegmentId::Path, &segments),
        "~/src/oss/l1br3/ced2"
    );
}

// --- text selection (the user-ruled interaction, 2026-10-11) ---
//
// Press-and-drag across the band selects the covered characters of
// the row's display text — WYSIWYG, the elided display included —
// and the release copies them (clipboard + primary). A click (no
// meaningful drag) keeps the shipped verb: the segment's full
// un-elided value and the ✓ flash.

fn row_segments() -> Vec<RowSegment> {
    vec![
        RowSegment {
            rect: Rectangle::new(Point::new(12.0, 0.0), Size::new(40.0, BAND_HEIGHT)),
            text: "fish".to_string(),
        },
        RowSegment {
            rect: Rectangle::new(Point::new(64.0, 0.0), Size::new(100.0, BAND_HEIGHT)),
            text: "~/src/oss/l1br3/ced2".to_string(),
        },
        RowSegment {
            rect: Rectangle::new(Point::new(300.0, 0.0), Size::new(40.0, BAND_HEIGHT)),
            text: "14:32".to_string(),
        },
    ]
}

#[test]
fn a_full_row_drag_selects_every_segment_joined_by_spaces() {
    let text = selection_text(
        Point::new(12.0, 14.0),
        Point::new(340.0, 14.0),
        &row_segments(),
    );
    assert_eq!(text, "fish ~/src/oss/l1br3/ced2 14:32");
}

#[test]
fn a_partial_drag_selects_the_covered_characters_of_both_sides() {
    // A character is covered when its cell intersects the span:
    // from x=20 (inside `fish`'s first cell) to x=100 (8 cells
    // into the path at 5 px per char); the untouched clock
    // contributes nothing, not even a joining space.
    let text = selection_text(
        Point::new(20.0, 14.0),
        Point::new(100.0, 14.0),
        &row_segments(),
    );
    assert_eq!(text, "fish ~/src/os");
    // Dragging backwards selects the same span.
    let text = selection_text(
        Point::new(100.0, 14.0),
        Point::new(20.0, 14.0),
        &row_segments(),
    );
    assert_eq!(text, "fish ~/src/os");
}

#[test]
fn a_drag_confined_to_a_gap_selects_nothing() {
    let text = selection_text(
        Point::new(52.0, 14.0),
        Point::new(60.0, 14.0),
        &row_segments(),
    );
    assert_eq!(text, "");
}

#[test]
fn selection_rects_intersect_each_segment_horizontally() {
    let rects = selection_rects(
        Point::new(20.0, 14.0),
        Point::new(100.0, 14.0),
        &row_segments(),
    );
    assert_eq!(rects.len(), 2);
    assert_eq!(
        rects[0],
        Rectangle::new(Point::new(20.0, 0.0), Size::new(32.0, BAND_HEIGHT))
    );
    assert_eq!(
        rects[1],
        Rectangle::new(Point::new(64.0, 0.0), Size::new(36.0, BAND_HEIGHT))
    );
}

#[test]
fn a_still_press_is_a_click_and_a_moving_press_selects() {
    assert!(selection_is_click(
        Point::new(20.0, 14.0),
        Point::new(20.0, 14.0)
    ));
    assert!(selection_is_click(
        Point::new(20.0, 14.0),
        Point::new(23.0, 15.0)
    ));
    assert!(!selection_is_click(
        Point::new(20.0, 14.0),
        Point::new(28.0, 14.0)
    ));
    assert!(!selection_is_click(
        Point::new(20.0, 14.0),
        Point::new(60.0, 14.0)
    ));
}

#[test]
fn selection_copies_the_display_wysiwyg_click_copies_the_full_value() {
    // An elided path's selection yields its elided characters —
    // what is seen is what is selected; the click verb remains the
    // full un-elided value through `copy_text`.
    let row = vec![RowSegment {
        rect: Rectangle::new(Point::new(64.0, 0.0), Size::new(50.0, BAND_HEIGHT)),
        text: "…/l1br3/ced2".to_string(),
    }];
    let text = selection_text(Point::new(64.0, 14.0), Point::new(94.0, 14.0), &row);
    assert_eq!(text, "…/l1br3/");
    let segments = StatusSegments {
        path: Some("~/src/oss/l1br3/ced2".to_string()),
        ..StatusSegments::default()
    };
    assert_eq!(
        copy_text(SegmentId::Path, &segments),
        "~/src/oss/l1br3/ced2"
    );
}

// --- double-click select (status-select spec) ---
//
// A second left press within the 500 ms window and 4 px of the
// previous (the drag threshold's own value, one constant fewer)
// completes a double-click: the pressed segment's whole span
// becomes the selection, and the release copies its display text
// WYSIWYG through the drag release's exact path — the click verb
// (full value plus ✓) never fires. The verbs compose by order: the
// double-click's first release is still a click.

#[test]
fn a_second_press_within_the_window_and_px_is_a_double_click() {
    let first = (Instant::now(), Point::new(100.0, 14.0));
    let soon = first.0 + Duration::from_millis(300);
    assert!(is_double_click(first, soon, Point::new(102.0, 15.0)));
}

#[test]
fn a_press_past_the_window_is_not_a_double_click() {
    let first = (Instant::now(), Point::new(100.0, 14.0));
    let late = first.0 + Duration::from_millis(501);
    assert!(!is_double_click(first, late, Point::new(100.0, 14.0)));
}

#[test]
fn a_press_too_far_from_the_previous_is_not_a_double_click() {
    let first = (Instant::now(), Point::new(100.0, 14.0));
    let soon = first.0 + Duration::from_millis(100);
    assert!(!is_double_click(first, soon, Point::new(106.0, 14.0)));
    assert!(!is_double_click(first, soon, Point::new(100.0, 20.0)));
}

#[test]
fn the_window_and_px_boundaries_are_inclusive() {
    // The rulings' exact edges: 500 ms on the dot and 4 px on the
    // dot (either axis) still complete the double-click.
    let first = (Instant::now(), Point::new(100.0, 14.0));
    let at_the_window = first.0 + Duration::from_millis(500);
    assert!(is_double_click(
        first,
        at_the_window,
        Point::new(100.0, 14.0)
    ));
    let soon = first.0 + Duration::from_millis(100);
    assert!(is_double_click(first, soon, Point::new(104.0, 14.0)));
    assert!(is_double_click(first, soon, Point::new(100.0, 18.0)));
}

#[test]
fn a_double_click_span_selects_the_whole_segment_wysiwyg() {
    // The span a double-click arms is the pressed segment's laid
    // rect end to end: through the existing selection functions it
    // highlights exactly that rect and copies exactly its display
    // text — no new selection code, the row is the source.
    let row = row_segments();
    let rect = row[1].rect;
    let span = (
        Point::new(rect.x, rect.y),
        Point::new(rect.x + rect.width, rect.y),
    );
    assert_eq!(
        selection_rects(span.0, span.1, &row),
        vec![Rectangle::new(
            Point::new(64.0, 0.0),
            Size::new(100.0, BAND_HEIGHT)
        )]
    );
    assert_eq!(selection_text(span.0, span.1, &row), "~/src/oss/l1br3/ced2");
}

#[test]
fn the_span_over_an_armed_revealed_path_selects_the_expanded_display() {
    // While the inline reveal holds the path expanded, the row text
    // is the expanded display — the span selects it verbatim, no
    // special case.
    let row = vec![RowSegment {
        rect: Rectangle::new(Point::new(64.0, 0.0), Size::new(100.0, BAND_HEIGHT)),
        text: "~/src/oss/l1br3/ced2".to_string(),
    }];
    let span = (Point::new(64.0, 14.0), Point::new(164.0, 14.0));
    assert_eq!(selection_text(span.0, span.1, &row), "~/src/oss/l1br3/ced2");
}

#[test]
fn a_double_click_release_copies_the_display_never_the_click_value() {
    // The routing, pinned by construction: the span-armed press
    // keeps the release off the click verb, so its payload comes
    // from `selection_text` — the *display* text — never
    // `copy_text`'s full un-elided value, and no ✓ flashes (the
    // flash is the click verb's acknowledgment alone).
    let row = vec![RowSegment {
        rect: Rectangle::new(Point::new(64.0, 0.0), Size::new(50.0, BAND_HEIGHT)),
        text: "…/l1br3/ced2".to_string(),
    }];
    let span = (Point::new(64.0, 14.0), Point::new(114.0, 14.0));
    assert_eq!(selection_text(span.0, span.1, &row), "…/l1br3/ced2");
    let segments = StatusSegments {
        path: Some("~/src/oss/l1br3/ced2".to_string()),
        ..StatusSegments::default()
    };
    assert_ne!(
        selection_text(span.0, span.1, &row),
        copy_text(SegmentId::Path, &segments),
        "the double-click's payload is the display, not the click value"
    );
}

// --- the context menu (the per-tab menu's pattern) ---

/// A rich band — every segment present — the menu pins' fixture.
fn rich_segments() -> StatusSegments {
    StatusSegments {
        running: true,
        process: "cargo".to_string(),
        path: Some("~/src/oss/l1br3/ced2".to_string()),
        branch: Some("ced2".to_string()),
        profile: Some("dev".to_string()),
        zoom_percent: Some(120),
        grid: Some((120, 30)),
        tab_position: Some((1, 3)),
        clock: Some("14:32".to_string()),
        running_elapsed: Some(Duration::from_secs(161)),
        frozen: None,
        scrolled: Some(7),
    }
}

#[test]
fn menu_rows_compose_clipboard_zoom_and_band_clusters_in_order() {
    // Copy · Copy All · Paste │ Zoom In · Zoom Out · Reset Zoom │
    // Hide Status Bar · Settings… — the clipboard cluster over the
    // band's own values, the zoom cluster the Zoom segment
    // narrates, the band's lifecycle rows last, Settings behind
    // its divider (the GNOME end placement).
    let rows = menu_rows(&rich_segments(), Some(SegmentId::Path));
    let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
    assert_eq!(
        labels,
        vec![
            "Copy",
            "Copy All",
            "Paste",
            "Zoom In",
            "Zoom Out",
            "Reset Zoom",
            "Hide Status Bar",
            "Settings\u{2026}",
        ]
    );
    let separated: Vec<bool> = rows.iter().map(|row| row.separated).collect();
    assert_eq!(
        separated,
        vec![false, false, false, true, false, false, true, false]
    );
    let enabled: Vec<bool> = rows.iter().map(|row| row.enabled).collect();
    assert!(
        enabled.iter().all(|&enabled| enabled),
        "every row is live on a rich band"
    );
}

#[test]
fn menu_rows_labels_are_unique_and_non_empty() {
    let rows = menu_rows(&rich_segments(), Some(SegmentId::Path));
    for (i, left) in rows.iter().enumerate() {
        assert!(
            !left.label.is_empty(),
            "{:?} has an empty label",
            left.label
        );
        for right in &rows[i + 1..] {
            assert_ne!(left.label, right.label);
        }
    }
}

#[test]
fn the_copy_row_carries_the_pressed_segment_and_mutes_on_a_gap_press() {
    // A segment press scopes Copy to it; a gap press mutes the row
    // and leaves it nothing to publish.
    let rows = menu_rows(&rich_segments(), Some(SegmentId::Path));
    assert_eq!(rows[0].pick, Some(MenuPick::CopySegment(SegmentId::Path)));
    assert!(rows[0].enabled);
    let gap = menu_rows(&rich_segments(), None);
    assert_eq!(gap[0].pick, None);
    assert!(!gap[0].enabled, "a gap press's Copy is muted, never picked");
}

#[test]
fn the_copy_row_mutes_when_the_pressed_segment_has_no_value() {
    // The live value decides: a pressed-but-absent segment is an
    // empty payload, and an empty payload never copies.
    let segments = StatusSegments {
        process: "fish".to_string(),
        ..StatusSegments::default()
    };
    let rows = menu_rows(&segments, Some(SegmentId::Clock));
    assert!(
        !rows[0].enabled,
        "a pressed segment with no value mutes Copy"
    );
    assert_eq!(rows[0].pick, Some(MenuPick::CopySegment(SegmentId::Clock)));
}

#[test]
fn copy_all_mutes_when_nothing_is_present() {
    let rows = menu_rows(&StatusSegments::default(), None);
    assert!(!rows[1].enabled, "an empty band has nothing to Copy All");
    let rich = menu_rows(&rich_segments(), None);
    assert!(rich[1].enabled);
}

#[test]
fn paste_never_mutes_so_gaps_middle_click_paste_too() {
    // The middle-click verb rides the menu's own Paste funnel
    // (status-select spec) — the one row that never mutes, not on
    // an empty band, not on a gap press — so the verb pastes
    // anywhere on the band: the band is chrome, and its Paste is
    // the terminal's own, always live.
    let rows = menu_rows(&StatusSegments::default(), None);
    assert_eq!(rows[2].pick, Some(MenuPick::Action(Action::Paste)));
    assert!(rows[2].enabled, "Paste never mutes — the band is chrome");
}

#[test]
fn reset_zoom_mutes_at_base_size_and_lives_while_zoomed() {
    // `zoom_percent`'s own condition, restated as the row's
    // enablement — Reset at base size would always no-op.
    let base = StatusSegments {
        process: "fish".to_string(),
        ..StatusSegments::default()
    };
    assert!(!menu_rows(&base, None)[5].enabled);
    assert!(menu_rows(&rich_segments(), None)[5].enabled);
}

#[test]
fn every_action_row_routes_to_its_keybinds_own_action() {
    let rows = menu_rows(&rich_segments(), Some(SegmentId::Path));
    let picks: Vec<Option<MenuPick>> = rows.iter().map(|row| row.pick).collect();
    assert_eq!(
        picks,
        vec![
            Some(MenuPick::CopySegment(SegmentId::Path)),
            Some(MenuPick::CopyAll),
            Some(MenuPick::Action(Action::Paste)),
            Some(MenuPick::Action(Action::ZoomIn)),
            Some(MenuPick::Action(Action::ZoomOut)),
            Some(MenuPick::Action(Action::ZoomReset)),
            Some(MenuPick::Action(Action::ToggleStatusBar)),
            Some(MenuPick::Action(Action::Settings)),
        ]
    );
}

#[test]
fn copy_all_joins_every_present_full_value_with_single_spaces() {
    // Full values through `copy_text` in row order — left cluster
    // then right — never displays, empties skipped.
    assert_eq!(
        copy_all_text(&rich_segments()),
        "cargo 2:41 ~/src/oss/l1br3/ced2 ced2 dev 120×30 1/3 +120% 14:32"
    );
}

#[test]
fn copy_all_skips_absent_segments_and_empty_in_empty_out() {
    let sparse = StatusSegments {
        process: "vim".to_string(),
        path: Some("~/src".to_string()),
        ..StatusSegments::default()
    };
    assert_eq!(copy_all_text(&sparse), "vim ~/src");
    assert_eq!(copy_all_text(&StatusSegments::default()), "");
}

#[test]
fn menu_bounds_anchors_at_the_press_and_clamps_into_the_band() {
    // The bubble pops to the terminal's side of the band with its
    // left edge at the press — unless that would overflow the
    // band's right end, where it shifts left to fit (the per-tab
    // menu's both-edges rule).
    let beside = menu_bounds(1000.0, 120.0, 80.0, 8, TooltipSide::Below);
    assert_eq!(beside.x, 120.0);
    assert_eq!(
        beside.y,
        BAND_HEIGHT + DROPDOWN_GAP,
        "a Below bubble starts under the band"
    );
    let above = menu_bounds(1000.0, 120.0, 80.0, 8, TooltipSide::Above);
    assert_eq!(
        above.y + above.height,
        -DROPDOWN_GAP,
        "an Above bubble clears the band upward"
    );
    let clamped = menu_bounds(300.0, 280.0, 80.0, 8, TooltipSide::Below);
    assert_eq!(
        clamped.x + clamped.width,
        300.0,
        "an anchored-too-far-right bubble ends at the band's right edge"
    );
    assert_eq!(
        clamped.width,
        80.0 + DROPDOWN_PADDING_X * 2.0,
        "width is the widest label plus the shared menu padding"
    );
    assert_eq!(
        clamped.height,
        8.0 * DROPDOWN_ROW_HEIGHT,
        "height is the row count on the shared menu row height"
    );
}
