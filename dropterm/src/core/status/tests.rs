// SPDX-License-Identifier: GPL-3.0-only

//! The status model's contract pins (gpui-status Task 1): the
//! resolver's hidden/absent indistinguishability, the OSC-fed seats'
//! precedence rules, the fit math's drop order, the clock's
//! minute-boundary arithmetic, and the display/copy split — the
//! behaviors both frontends' bands render from. The libcosmic
//! frontend's own `status_bar` suite stays the fuller record; these
//! are the pins that outlive it (cutover deletes that frontend).

use super::*;
use crate::core::config::{Edge, TabPosition};
use crate::core::terminal::{
    ProgramState, ProgramStatus, ProgressState, ReportedCwd, ReportedProgress,
};
use std::path::Path;
use std::time::Duration;

/// The all-on inputs floor every test mutates — a plain-shell local
/// tab with a cwd and nothing else standing.
fn inputs<'a>() -> StatusInputs<'a> {
    StatusInputs {
        home: Some(Path::new("/home/u")),
        cwd: Some(Path::new("/home/u/src")),
        reported_cwd: None,
        hostname: Some("here"),
        shell: "/usr/bin/fish",
        profile_name: None,
        zoom_adj: 0,
        zoom_step_mul_100: 125,
        foreground: None,
        branch: None,
        grid: Some((80, 24)),
        spawn_grid: Some((80, 24)),
        tab_position: Some((1, 1)),
        clock: Some("14:32"),
        running_elapsed: None,
        frozen: None,
        exit_status: None,
        scrolled_back: None,
        pasted: None,
        progress: None,
        program_status: None,
        ssh_destination: None,
        missed_activity: false,
        attention_red: None,
        attention_bright_red: None,
    }
}

fn toggles() -> StatusToggles {
    StatusToggles::all()
}

#[test]
fn hidden_and_absent_segments_resolve_indistinguishably() {
    let mut with_path = inputs();
    with_path.branch = Some("main");
    let hidden = {
        let mut toggles = toggles();
        toggles.branch = false;
        resolve_segments(&with_path, toggles)
    };
    let absent = resolve_segments(&inputs(), toggles());
    assert_eq!(hidden.branch, absent.branch);
    assert_eq!(hidden.branch, None);

    // The process seat's own shape: hidden empties the name but the
    // disc's fill keeps its own truth — `running` is ungated.
    let mut with_foreground = inputs();
    with_foreground.foreground = Some("cargo");
    let mut process_off = toggles();
    process_off.process = false;
    let resolved = resolve_segments(&with_foreground, process_off);
    assert_eq!(resolved.process, "");
    assert!(resolved.running);
}

#[test]
fn reported_cwd_outranks_both_layers_and_collapses_only_locally() {
    // No report: the /proc cwd, `~`-collapsed.
    let local = resolve_segments(&inputs(), toggles());
    assert_eq!(local.path.as_deref(), Some("~/src"));

    // A local report replaces the /proc cwd wholesale.
    let mut reported = inputs();
    reported.reported_cwd = Some(ReportedCwd {
        host: None,
        path: "/home/u/docs".to_string(),
    });
    assert_eq!(
        resolve_segments(&reported, toggles()).path.as_deref(),
        Some("~/docs")
    );

    // A remote report renders host-qualified and never collapses — no
    // guessed remote `$HOME`.
    let mut remote = inputs();
    remote.reported_cwd = Some(ReportedCwd {
        host: Some("other".to_string()),
        path: "/home/u/docs".to_string(),
    });
    assert_eq!(
        resolve_segments(&remote, toggles()).path.as_deref(),
        Some("other:/home/u/docs")
    );

    // The ssh stand-in sits under the report it loses to.
    let mut ssh = inputs();
    ssh.reported_cwd = Some(ReportedCwd {
        host: None,
        path: "/home/u/docs".to_string(),
    });
    ssh.ssh_destination = Some("web-prod");
    assert_eq!(
        resolve_segments(&ssh, toggles()).path.as_deref(),
        Some("~/docs")
    );
    // And outranks the /proc cwd when no report stands.
    let mut ssh_alone = inputs();
    ssh_alone.ssh_destination = Some("web-prod");
    assert_eq!(
        resolve_segments(&ssh_alone, toggles()).path.as_deref(),
        Some("web-prod")
    );
}

#[test]
fn branch_hides_while_stood_in_and_under_remote_reports() {
    let mut stood_in = inputs();
    stood_in.branch = Some("main");
    stood_in.ssh_destination = Some("web-prod");
    assert_eq!(resolve_segments(&stood_in, toggles()).branch, None);

    let mut remote = inputs();
    remote.branch = Some("main");
    remote.reported_cwd = Some(ReportedCwd {
        host: Some("other".to_string()),
        path: "/srv".to_string(),
    });
    assert_eq!(resolve_segments(&remote, toggles()).branch, None);

    // A local report leaves the branch to its own cwd read.
    let mut local_report = inputs();
    local_report.branch = Some("main");
    local_report.reported_cwd = Some(ReportedCwd {
        host: None,
        path: "/home/u".to_string(),
    });
    assert_eq!(
        resolve_segments(&local_report, toggles()).branch.as_deref(),
        Some("main")
    );
}

#[test]
fn exit_success_and_absent_are_the_same_none() {
    let mut failed = inputs();
    failed.exit_status = Some(3);
    assert_eq!(resolve_segments(&failed, toggles()).exit_code, Some(3));

    let mut success = inputs();
    success.exit_status = Some(0);
    assert_eq!(resolve_segments(&success, toggles()).exit_code, None);
    assert_eq!(resolve_segments(&inputs(), toggles()).exit_code, None);
}

#[test]
fn grid_hides_at_its_spawn_default() {
    let at_default = resolve_segments(&inputs(), toggles());
    assert_eq!(at_default.grid, None);

    let mut resized = inputs();
    resized.grid = Some((100, 24));
    assert_eq!(resolve_segments(&resized, toggles()).grid, Some((100, 24)));

    // No record ever witnessed: the live grid is the honest reading.
    let mut unwitnessed = inputs();
    unwitnessed.spawn_grid = None;
    assert_eq!(
        resolve_segments(&unwitnessed, toggles()).grid,
        Some((80, 24))
    );
}

#[test]
fn zoom_signs_its_steps_and_hides_at_base() {
    let mut zoomed = inputs();
    zoomed.zoom_adj = 2;
    assert_eq!(resolve_segments(&zoomed, toggles()).zoom_percent, Some(250));
    let mut out = inputs();
    out.zoom_adj = -1;
    assert_eq!(resolve_segments(&out, toggles()).zoom_percent, Some(-125));
    assert_eq!(resolve_segments(&inputs(), toggles()).zoom_percent, None);
    // Saturation, never wrap: a maxed adjustment clamps into i16.
    assert_eq!(zoom_percent(i8::MAX, 32_767), Some(i16::MAX));
    assert_eq!(zoom_percent(i8::MIN, 32_767), Some(i16::MIN));
}

#[test]
fn scrolled_exists_only_off_the_bottom() {
    let mut scrolled = inputs();
    scrolled.scrolled_back = Some(12);
    assert_eq!(resolve_segments(&scrolled, toggles()).scrolled, Some(12));

    let mut bottom = inputs();
    bottom.scrolled_back = Some(0);
    assert_eq!(resolve_segments(&bottom, toggles()).scrolled, None);
}

#[test]
fn column_stacking_keeps_the_band_at_the_free_edge() {
    let stacking = |edge, tabs| {
        column_stacking(edge, tabs)
            .iter()
            .map(|strip| format!("{strip:?}"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        stacking(Edge::Top, TabPosition::Top),
        ["Tabs", "Terminal", "StatusBand", "GrabBar"]
    );
    assert_eq!(
        stacking(Edge::Top, TabPosition::Auto),
        ["Terminal", "Tabs", "StatusBand", "GrabBar"]
    );
    assert_eq!(
        stacking(Edge::Bottom, TabPosition::Top),
        ["GrabBar", "StatusBand", "Tabs", "Terminal"]
    );
    assert_eq!(
        stacking(Edge::Bottom, TabPosition::Bottom),
        ["GrabBar", "StatusBand", "Terminal", "Tabs"]
    );
}

#[test]
fn fit_drops_present_segments_in_precedence_order() {
    // Every droppable seat present at 30 px; the path measures
    // 20 chars at 8 px/char; the never-yielding consumption is 60 px.
    let widths: Vec<(SegmentId, f32)> = [(SegmentId::Branch, 30.0)]
        .into_iter()
        .chain(RIGHT_CLUSTER.map(|id| (id, 30.0)))
        .collect();
    let path = PathMeasure {
        chars: 20,
        full_width: 160.0,
    };
    // Roomy: nothing gives way.
    let roomy = band_fit(2000.0, 60.0, &widths, path);
    assert!(roomy.dropped.is_empty());
    assert_eq!(roomy.path, PathFit::Full);
    // Tight (310 px leaves under one elided path-cell of room): the
    // paste chip yields before the grid, the grid before the tab —
    // the DROP_ORDER prefix restricted to present seats, the path
    // re-expanding into each freed width.
    let tight = band_fit(310.0, 60.0, &widths, path);
    assert_eq!(
        tight.dropped,
        vec![SegmentId::Pasted, SegmentId::Grid, SegmentId::Tab]
    );
    // Absent seats are skipped, never phantom-dropped.
    let mut no_pasted = widths.clone();
    no_pasted.retain(|(id, _)| *id != SegmentId::Pasted);
    let tight_without = band_fit(310.0, 60.0, &no_pasted, path);
    assert_eq!(tight_without.dropped, vec![SegmentId::Grid, SegmentId::Tab]);
}

#[test]
fn squeeze_elides_the_path_from_the_left_keeping_the_tail() {
    assert_eq!(elide_path_left("abcdefgh", 8), "abcdefgh");
    assert_eq!(elide_path_left("abcdefgh", 3), "…gh");
    assert_eq!(elide_path_left("any", 1), "…");
    assert_eq!(elide_path_left("any", 0), "");
    // The branch yields only when even a fully elided path cannot
    // fit beside it.
    let (fit, branch) = left_squeeze(122.0, 10, 80.0, 30.0);
    assert_eq!(fit, PathFit::Full);
    assert!(branch);
    let (fit, branch) = left_squeeze(100.0, 10, 80.0, 30.0);
    assert!(matches!(fit, PathFit::Elided(max) if max == 7));
    assert!(branch);
    let (fit, branch) = left_squeeze(20.0, 10, 80.0, 30.0);
    assert!(matches!(fit, PathFit::Elided(max) if max == 2));
    assert!(!branch);
}

#[test]
fn collapse_home_cuts_on_component_boundaries() {
    let home = Path::new("/home/u");
    assert_eq!(collapse_home(Path::new("/home/u"), Some(home)), "~");
    assert_eq!(
        collapse_home(Path::new("/home/u/src/oss"), Some(home)),
        "~/src/oss"
    );
    // A sibling sharing the string prefix never collapses.
    assert_eq!(
        collapse_home(Path::new("/home/u-x"), Some(home)),
        "/home/u-x"
    );
    assert_eq!(collapse_home(Path::new("/tmp"), None), "/tmp");
}

#[test]
fn displays_carry_their_owned_marks() {
    assert_eq!(exit_display(3), "✗3");
    assert_eq!(
        progress_display(ReportedProgress {
            state: ProgressState::InProgress,
            value: Some(42),
            refreshed_at: std::time::Instant::now(),
        }),
        "42%"
    );
    assert_eq!(
        progress_display(ReportedProgress {
            state: ProgressState::Indeterminate,
            value: Some(42),
            refreshed_at: std::time::Instant::now(),
        }),
        "…"
    );
    assert_eq!(
        progress_display(ReportedProgress {
            state: ProgressState::Error,
            value: None,
            refreshed_at: std::time::Instant::now(),
        }),
        "!"
    );
    assert_eq!(
        process_display(
            "cargo",
            Some(ProgramStatus {
                state: ProgramState::Working,
                progress: Some(40),
            })
        ),
        "cargo 40%"
    );
    assert_eq!(
        process_display(
            "cargo",
            Some(ProgramStatus {
                state: ProgramState::Idle,
                progress: None,
            })
        ),
        "cargo"
    );
    assert_eq!(scrolled_display(12), "↑12");
    assert_eq!(pasted_display(1), "1 image");
    assert_eq!(pasted_display(3), "3 images");
    assert_eq!(format_duration(Duration::from_secs(41)), "0:41");
    assert_eq!(format_duration(Duration::from_secs(61)), "1:01");
    assert_eq!(format_duration(Duration::from_secs(3661)), "1:01:01");
    // The riding glyphs: on the display, never the payload.
    assert_eq!(prefixed_display(SegmentId::Branch, "main"), "⎇ main");
    assert_eq!(prefixed_display(SegmentId::Branch, ""), "");
    assert_eq!(segment_icon(SegmentId::Process), None);
    assert_eq!(segment_icon(SegmentId::Exit), None);
    assert_eq!(segment_icon(SegmentId::Scrolled), None);
}

#[test]
fn frozen_duration_pairs_its_name_and_lives_verbatim() {
    let mut finished = inputs();
    finished.frozen = Some(("cargo", Duration::from_secs(161)));
    let resolved = resolve_segments(&finished, toggles());
    assert_eq!(duration_display(&resolved), "cargo 2:41");

    // The honest floor: a never-sampled run keeps its duration with
    // no name to lie about.
    let mut unnamed = inputs();
    unnamed.frozen = Some(("", Duration::from_secs(161)));
    assert_eq!(
        duration_display(&resolve_segments(&unnamed, toggles())),
        "2:41"
    );

    // Live elapsed outranks the frozen pair.
    let mut running = inputs();
    running.running_elapsed = Some(Duration::from_secs(5));
    running.frozen = Some(("cargo", Duration::from_secs(161)));
    assert_eq!(
        duration_display(&resolve_segments(&running, toggles())),
        "0:05"
    );
}

#[test]
fn copy_payloads_stay_bare_values() {
    let mut rich = inputs();
    rich.branch = Some("main");
    rich.grid = Some((100, 24));
    rich.tab_position = Some((2, 5));
    rich.zoom_adj = 2;
    rich.exit_status = Some(3);
    rich.progress = Some(ReportedProgress {
        state: ProgressState::InProgress,
        value: Some(42),
        refreshed_at: std::time::Instant::now(),
    });
    let segments = resolve_segments(&rich, toggles());
    assert_eq!(copy_text(SegmentId::Grid, &segments), "100×24");
    assert_eq!(copy_text(SegmentId::Path, &segments), "~/src");
    assert_eq!(copy_text(SegmentId::Branch, &segments), "main");
    assert_eq!(copy_text(SegmentId::Tab, &segments), "2/5");
    assert_eq!(copy_text(SegmentId::Zoom, &segments), "+250%");
    assert_eq!(copy_text(SegmentId::Clock, &segments), "14:32");
    assert_eq!(copy_text(SegmentId::Exit, &segments), "3");
    // The bare number, never the glyph or the percent.
    assert_eq!(copy_text(SegmentId::Progress, &segments), "42");
    // The two action chips have no copyable value at all.
    assert!(!click_jumps(SegmentId::Path));
    assert!(click_jumps(SegmentId::Scrolled));
    assert!(click_jumps(SegmentId::Pasted));
}

#[test]
fn the_refusal_verdict_takes_the_pasted_seat_then_the_clock() {
    // With pastes standing the verdict takes their chip's seat —
    // the position where the user's paste news lives.
    let with_pastes = StatusSegments {
        clock: Some("14:32".to_owned()),
        pasted: Some(3),
        ..Default::default()
    };
    assert_eq!(refusal_seat(&with_pastes), SegmentId::Pasted);
    // Without one, the clock — the right cluster's tail a rendered
    // band always carries.
    let without = StatusSegments {
        pasted: None,
        ..with_pastes
    };
    assert_eq!(refusal_seat(&without), SegmentId::Clock);
}

#[test]
fn the_refusal_display_names_the_cap_in_whole_mib() {
    assert_eq!(
        refusal_display(PasteRefusal::OverCap),
        "✗ image over 32 MiB cap"
    );
    assert_eq!(refusal_display(PasteRefusal::Failed), "✗ paste failed");
}

#[test]
fn program_attention_is_only_blocked_and_error() {
    for state in [
        ProgramState::Idle,
        ProgramState::Working,
        ProgramState::Done,
    ] {
        assert!(!program_needs_attention(Some(ProgramStatus {
            state,
            progress: None,
        })));
    }
    for state in [ProgramState::Blocked, ProgramState::Error] {
        assert!(program_needs_attention(Some(ProgramStatus {
            state,
            progress: None,
        })));
    }
    assert!(!program_needs_attention(None));
}

#[test]
fn shell_basename_takes_the_last_component() {
    assert_eq!(shell_basename("/usr/bin/fish"), "fish");
    assert_eq!(shell_basename("fish"), "fish");
    assert_eq!(shell_basename("/"), "/");
}

fn zoned(at: &str) -> jiff::Zoned {
    at.parse().expect("fixed zoned fixture")
}

#[test]
fn clock_text_formats_twenty_four_hour_minutes() {
    assert_eq!(
        clock_text(&zoned("2026-11-14T14:32:00+00:00[UTC]")),
        "14:32"
    );
    assert_eq!(
        clock_text(&zoned("2026-11-14T09:05:59+00:00[UTC]")),
        "09:05"
    );
}

#[test]
fn next_tick_delay_lands_on_the_minute_boundary() {
    // An on-boundary now waits a full minute.
    assert_eq!(
        next_tick_delay(&zoned("2026-11-14T14:32:00+00:00[UTC]")),
        Duration::from_secs(60)
    );
    // Mid-minute arithmetic: the boundary minus the subsecond now.
    assert_eq!(
        next_tick_delay(&zoned("2026-11-14T14:32:30.500+00:00[UTC]")),
        Duration::from_millis(29_500)
    );
}

#[test]
fn due_gates_are_the_band_and_segment_keys() {
    assert!(clock_due(true, true));
    assert!(!clock_due(true, false));
    assert!(!clock_due(false, true));

    assert!(duration_due(true, true, true, true));
    assert!(!duration_due(true, true, false, true));
    assert!(!duration_due(true, true, true, false));

    assert!(ssh_read_due(true, true, true));
    assert!(!ssh_read_due(true, true, false));
    assert!(!ssh_read_due(true, false, true));

    assert!(activity_mark_due(true, true));
    assert!(!activity_mark_due(true, false));
    assert!(!activity_mark_due(false, true));
}

#[test]
fn git_branch_reads_heads_never_guessing() {
    let stamp = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("post-epoch clock")
            .as_nanos()
            % 1_000_000,
    )
    .expect("a small nanos remainder fits u64");
    let dir = std::env::temp_dir().join(format!(
        "dropterm-status-git-{}",
        u64::from(std::process::id()) + stamp
    ));
    let repo = dir.join("repo");
    std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");

    // A ref HEAD names its branch.
    std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").expect("write HEAD");
    assert_eq!(git_branch(Some(&repo)).as_deref(), Some("main"));

    // A detached HEAD shortens to its first 7 hex characters.
    std::fs::write(repo.join(".git/HEAD"), "0123456789abcdef\n").expect("rewrite HEAD");
    assert_eq!(git_branch(Some(&repo)).as_deref(), Some("0123456"));

    // Malformed input resolves None — never a wrong name.
    std::fs::write(repo.join(".git/HEAD"), "not a sha\n").expect("rewrite HEAD");
    assert_eq!(git_branch(Some(&repo)), None);

    // A worktree's gitdir file: one indirection, absolute or
    // relative, then the same HEAD read.
    let worktree = dir.join("worktree");
    std::fs::create_dir_all(&worktree).expect("mkdir worktree");
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", repo.join(".git").display()),
    )
    .expect("write gitdir");
    std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/topic\n").expect("rewrite HEAD");
    assert_eq!(git_branch(Some(&worktree)).as_deref(), Some("topic"));

    assert_eq!(git_branch(None), None);
    let _ = std::fs::remove_dir_all(&dir);
}

// --- the interaction families (gpui-status Task 4) ---
//
// The reference band's pins ported: the drag selection's threshold
// and per-char WYSIWYG, the menu's live enablement, the reveal's
// seat loan, and the seam rule's stacking input. The laid rows are
// built directly (the measurer-free half of band_vm's output).

/// Three laid rows at the reference's fixture seats: x=12 (8 wide),
/// x=64 (10 wide), x=300 (5 wide) — gaps on both sides of the last.
fn laid_rows() -> Vec<BandRowVm> {
    vec![
        BandRowVm {
            id: SegmentId::Process,
            text: "cargooo".to_owned(),
            x: 12.0,
            width: 8.0,
            text_x: 12.0,
            dimmed: false,
            attention: false,
        },
        BandRowVm {
            id: SegmentId::Path,
            text: "…/ced2".to_owned(),
            x: 64.0,
            width: 10.0,
            text_x: 64.0,
            dimmed: false,
            attention: false,
        },
        BandRowVm {
            id: SegmentId::Clock,
            text: "14:3".to_owned(),
            x: 300.0,
            width: 5.0,
            text_x: 300.0,
            dimmed: true,
            attention: false,
        },
    ]
}

#[test]
fn the_four_px_threshold_discriminates_click_from_drag() {
    let anchor = (50.0, 10.0);
    assert!(selection_is_click(anchor, (52.0, 11.0)));
    assert!(selection_is_click(anchor, anchor));
    // Past the threshold in either axis — squared compare, both.
    assert!(!selection_is_click(anchor, (55.0, 10.0)));
    assert!(!selection_is_click(anchor, (50.0, 15.0)));
    assert!(!selection_is_click(anchor, (53.0, 13.0)));
}

#[test]
fn hit_testing_names_the_row_and_never_a_gap() {
    let rows = laid_rows();
    assert_eq!(segment_at(&rows, 65.0, 5.0), Some(SegmentId::Path));
    assert_eq!(segment_at(&rows, 30.0, 5.0), None);
    assert_eq!(segment_at(&rows, 76.0, 5.0), None);
    assert_eq!(segment_at(&rows, 299.0, 27.0), None);
}

#[test]
fn the_selection_span_painting_covers_the_intersection_only() {
    let rows = laid_rows();
    // Backwards and forwards drags select the same span.
    let forward = selection_rects((66.0, 10.0), (302.0, 10.0), &rows);
    let backward = selection_rects((302.0, 10.0), (66.0, 10.0), &rows);
    assert_eq!(forward, backward);
    // The middle row's intersection, clamped to the span's edges.
    assert_eq!(forward.len(), 2);
    assert!((forward[0].x - 66.0).abs() < f32::EPSILON);
    assert!((forward[0].width - 8.0).abs() < f32::EPSILON);
    assert!((forward[1].x - 300.0).abs() < f32::EPSILON);
    assert!((forward[1].width - 2.0).abs() < f32::EPSILON);
    // A gap-confined drag selects nothing.
    assert!(selection_rects((30.0, 10.0), (40.0, 10.0), &rows).is_empty());
}

#[test]
fn the_selection_text_is_wysiwyg_joined_by_single_spaces() {
    let rows = laid_rows();
    // Per-char cells floor/ceil from the span's edges: the first
    // row's tail chars plus the second row whole.
    let text = selection_text((16.0, 10.0), (75.0, 10.0), &rows);
    assert_eq!(text, "gooo …/ced2");
    // All three rows covered partly: joined by exactly one space,
    // the elided display's `…` riding the payload — WYSIWYG.
    let both = selection_text((14.0, 10.0), (302.0, 10.0), &rows);
    assert_eq!(both, "argooo …/ced2 14");
}

#[test]
fn the_menu_mutes_exactly_its_two_conditional_rows() {
    let mut rich = inputs();
    rich.branch = Some("main");
    rich.grid = Some((100, 24));
    rich.zoom_adj = 2;
    let segments = resolve_segments(&rich, toggles());

    let pressed = menu_rows(&segments, Some(SegmentId::Path));
    let labels: Vec<&str> = pressed.iter().map(|row| row.label.as_str()).collect();
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
    // Separators before the zoom cluster and the band lifecycle
    // cluster — the checklist's `·`/`│` composition.
    assert!(pressed[3].separated && pressed[6].separated);
    assert!(!pressed[0].separated);
    // Copy lives (a value stands), Reset Zoom lives (zoomed).
    assert!(pressed[0].enabled && pressed[5].enabled);
    assert_eq!(
        pressed[0].pick,
        Some(MenuPick::CopySegment(SegmentId::Path))
    );
    assert!(
        pressed
            .iter()
            .all(|row| row.pick.is_some() || row.label == "Copy")
    );

    // The gap press mutes Copy; base zoom mutes Reset Zoom.
    let gap = menu_rows(&segments, None);
    assert!(!gap[0].enabled && gap[0].pick.is_none());
    let base = resolve_segments(&inputs(), toggles());
    let at_base = menu_rows(&base, Some(SegmentId::Path));
    assert!(!at_base[5].enabled);
    // Paste never mutes — the band is chrome.
    assert!(gap[2].enabled && at_base[2].enabled);
}

#[test]
fn copy_all_joins_every_present_value_in_row_order() {
    let mut rich = inputs();
    rich.branch = Some("main");
    rich.grid = Some((100, 24));
    rich.tab_position = Some((2, 5));
    rich.zoom_adj = 2;
    rich.exit_status = Some(3);
    let segments = resolve_segments(&rich, toggles());
    // Full values, never displays: no glyphs, no `✗`, no `+`.
    assert_eq!(
        copy_all_text(&segments),
        "fish 3 ~/src main 100×24 2/5 +250% 14:32"
    );
    // The empty band copies nothing.
    assert_eq!(copy_all_text(&StatusSegments::default()), "");
}

#[test]
fn the_reveal_arms_only_on_an_elided_path_under_the_cursor() {
    let elided = Some(BandRect {
        x: 64.0,
        y: 0.0,
        width: 10.0,
        height: BAND_HEIGHT,
    });
    let on_it = Some((70.0, 5.0));
    assert!(reveal_armed(elided, on_it, true));
    // An un-elided path never arms, whatever the cursor.
    assert!(!reveal_armed(elided, on_it, false));
    // The cursor away or absent disarms.
    assert!(!reveal_armed(elided, Some((30.0, 5.0)), true));
    assert!(!reveal_armed(elided, None, true));
    // No rect (a gone path), no arming.
    assert!(!reveal_armed(None, on_it, true));
}

#[test]
fn band_vm_lends_the_branch_seat_to_an_armed_reveal() {
    // The measurer: 8 px per char, the view tests' own fake.
    fn fake_measure(text: &str) -> f32 {
        #[expect(clippy::cast_precision_loss, reason = "test strings, exact in f32")]
        let width = text.chars().count() as f32 * 8.0;
        width
    }
    let mut segments = StatusSegments {
        process: "sh".to_owned(),
        path: Some("~/src/oss/l1br3/ced2".to_owned()),
        branch: Some("main".to_owned()),
        ..StatusSegments::default()
    };
    segments.scrolled = None;
    // Narrow enough that the path elides at rest.
    let width = 260.0;
    let resting = band_vm(&segments, width, &fake_measure, None, None, None);
    assert!(
        matches!(resting.path_fit, PathFit::Elided(_)),
        "the fixture must elide at rest"
    );
    assert!(resting.rows.iter().any(|row| row.id == SegmentId::Branch));

    // The cursor on the resting path's rect arms the reveal: the
    // path row expands (longer text than the resting elision) and
    // the branch row vanishes for the loan.
    let path_row = resting
        .rows
        .iter()
        .find(|row| row.id == SegmentId::Path)
        .expect("the path row lays");
    let hover = (path_row.x + 1.0, 5.0);
    let armed = band_vm(&segments, width, &fake_measure, None, None, Some(hover));
    let armed_path = armed
        .rows
        .iter()
        .find(|row| row.id == SegmentId::Path)
        .expect("the armed path row lays");
    assert!(
        armed_path.text.chars().count() > path_row.text.chars().count(),
        "the reveal expands the display"
    );
    assert!(
        armed
            .rows
            .iter()
            .all(|row| row.id != SegmentId::Branch || row.text.is_empty()),
        "the branch yields its seat while the path is examined"
    );

    // The cursor elsewhere disarms — the resting layout stands.
    let away = band_vm(
        &segments,
        width,
        &fake_measure,
        None,
        None,
        Some((8.0, 5.0)),
    );
    assert_eq!(
        away.rows
            .iter()
            .find(|row| row.id == SegmentId::Path)
            .map(|row| row.text.clone()),
        Some(path_row.text.clone())
    );
}

#[test]
fn the_bands_terminal_facing_side_matches_the_stacking_in_every_combination() {
    // The border hairline's side rule's input: the band's
    // terminal-facing side is the side the terminal sits on, so
    // `at_bottom == (edge == Top)` must agree with the stacking in
    // every combination — the seam never doubles because each bar
    // draws exactly its own facing line.
    for edge in [Edge::Top, Edge::Bottom] {
        for tab_position in [TabPosition::Auto, TabPosition::Top, TabPosition::Bottom] {
            let stacking = column_stacking(edge, tab_position);
            let terminal_index = stacking
                .iter()
                .position(|strip| *strip == Strip::Terminal)
                .expect("the terminal lays");
            let band_index = stacking
                .iter()
                .position(|strip| *strip == Strip::StatusBand)
                .expect("the band lays");
            assert_eq!(
                terminal_index < band_index,
                edge == Edge::Top,
                "the band's terminal-facing side must flip with the anchor edge: {edge:?}/{tab_position:?}"
            );
        }
    }
}
