use super::*;

#[test]
fn after_current_placement_puts_the_new_tab_after_active() {
    let mut list = vec![10, 20, 30];
    let new_active = insert_new_tab(&mut list, 1, true, 99);
    assert_eq!(list, vec![10, 20, 99, 30]);
    assert_eq!(new_active, 2);
}

#[test]
fn after_current_placement_appends_when_active_is_last() {
    let mut list = vec![10, 20];
    let new_active = insert_new_tab(&mut list, 1, true, 99);
    assert_eq!(list, vec![10, 20, 99]);
    assert_eq!(new_active, 2);
}

#[test]
fn after_current_placement_fills_an_empty_list() {
    let mut list = Vec::new();
    let new_active = insert_new_tab(&mut list, 0, true, 7);
    assert_eq!(list, vec![7]);
    assert_eq!(new_active, 0);
}

#[test]
fn at_end_placement_appends_past_a_mid_list_active() {
    let mut list = vec![10, 20, 30];
    let new_active = insert_new_tab(&mut list, 1, false, 99);
    assert_eq!(list, vec![10, 20, 30, 99]);
    assert_eq!(new_active, 3);
}

#[test]
fn at_end_placement_appends_when_active_is_last() {
    let mut list = vec![10, 20];
    let new_active = insert_new_tab(&mut list, 1, false, 99);
    assert_eq!(list, vec![10, 20, 99]);
    assert_eq!(new_active, 2);
}

#[test]
fn at_end_placement_fills_an_empty_list() {
    let mut list = Vec::new();
    let new_active = insert_new_tab(&mut list, 0, false, 7);
    assert_eq!(list, vec![7]);
    assert_eq!(new_active, 0);
}

#[test]
fn close_tab_active_middle_focuses_successor() {
    let mut list = vec![10, 20, 30];
    let decision = close_tab(&mut list, 1, 1).expect("tab should close");
    assert_eq!(list, vec![10, 30]);
    assert_eq!(decision.next_active, 1);
    assert!(!decision.respawn);
}

#[test]
fn close_tab_active_last_focuses_predecessor() {
    let mut list = vec![10, 20, 30];
    let decision = close_tab(&mut list, 2, 2).expect("tab should close");
    assert_eq!(list, vec![10, 20]);
    assert_eq!(decision.next_active, 1);
    assert!(!decision.respawn);
}

#[test]
fn close_tab_before_active_shifts_active_down() {
    let mut list = vec![10, 20, 30];
    let decision = close_tab(&mut list, 0, 2).expect("tab should close");
    assert_eq!(list, vec![20, 30]);
    assert_eq!(decision.next_active, 1);
    assert!(!decision.respawn);
}

#[test]
fn close_tab_after_active_keeps_active() {
    let mut list = vec![10, 20, 30];
    let decision = close_tab(&mut list, 2, 0).expect("tab should close");
    assert_eq!(list, vec![10, 20]);
    assert_eq!(decision.next_active, 0);
    assert!(!decision.respawn);
}

#[test]
fn close_tab_last_tab_decides_to_respawn() {
    let mut list = vec![10];
    let decision = close_tab(&mut list, 0, 0).expect("tab should close");
    assert!(list.is_empty(), "not empty: {list:?}");
    assert_eq!(decision.next_active, 0);
    assert!(decision.respawn);
}

#[test]
fn close_tab_out_of_bounds_is_none_and_keeps_list() {
    let mut list = vec![10];
    assert!(close_tab(&mut list, 5, 0).is_none());
    assert_eq!(list, vec![10]);
}

#[test]
fn cycle_next_advances_one() {
    assert_eq!(cycle_next(&[10, 20, 30], 0), 1);
}

#[test]
fn cycle_next_wraps_around_to_first() {
    assert_eq!(cycle_next(&[10, 20, 30], 2), 0);
}

#[test]
fn cycle_prev_steps_back() {
    assert_eq!(cycle_prev(&[10, 20, 30], 2), 1);
}

#[test]
fn cycle_prev_wraps_around_to_last() {
    assert_eq!(cycle_prev(&[10, 20, 30], 0), 2);
}

#[test]
fn new_tab_cwd_falls_back_to_the_active_tabs_cwd() {
    let inherited = PathBuf::from("/home/jpt/src");
    assert_eq!(
        resolve_new_tab_cwd(None, Some(inherited.clone())),
        Some(inherited)
    );
}

#[test]
fn new_tab_cwd_without_inheritance_spawns_in_the_pty_default() {
    assert_eq!(resolve_new_tab_cwd(None, None), None);
}

#[test]
fn new_tab_cwd_an_explicit_request_wins_over_inheritance() {
    let explicit = PathBuf::from("/tmp");
    let inherited = PathBuf::from("/home/jpt/src");
    assert_eq!(
        resolve_new_tab_cwd(Some(explicit.clone()), Some(inherited)),
        Some(explicit)
    );
}

// --- the mode-threaded plain-spawn cwd (default-spawn DS-1) ---

#[test]
fn inherit_mode_is_today_exact_chain_and_never_consults_the_default() {
    // Ruling 2: `Inherit` byte-identical — the default leg drops
    // out entirely, set or not, so a dead `default_cwd` can never
    // move (or warn about) an inherit-mode spawn.
    let inherited = PathBuf::from("/home/jpt/src");
    let default = PathBuf::from("/srv");
    assert_eq!(
        resolve_plain_spawn_cwd(
            NewTabCwd::Inherit,
            None,
            Some(inherited.clone()),
            Some(default.clone())
        ),
        Some(inherited)
    );
    assert_eq!(
        resolve_plain_spawn_cwd(NewTabCwd::Inherit, None, None, Some(default)),
        None
    );
}

#[test]
fn default_mode_replaces_the_inherited_leg() {
    // Ruling 2: `Default` *replaces* inheritance — the configured
    // directory wins over the active tab's cwd, not under it.
    let inherited = PathBuf::from("/home/jpt/src");
    let default = PathBuf::from("/srv");
    assert_eq!(
        resolve_plain_spawn_cwd(
            NewTabCwd::Default,
            None,
            Some(inherited),
            Some(default.clone())
        ),
        Some(default)
    );
}

#[test]
fn default_mode_without_a_default_leg_spawns_in_the_pty_default() {
    // The inherited leg is replaced, never kept as a fallback: an
    // unset — or upstream-degraded dead — `default_cwd` means the
    // pty's default directory, not the active tab's cwd.
    let inherited = PathBuf::from("/home/jpt/src");
    assert_eq!(
        resolve_plain_spawn_cwd(NewTabCwd::Default, None, Some(inherited), None),
        None
    );
}

#[test]
fn an_explicit_request_wins_in_default_mode_too() {
    // Explicit `--cwd` wins in both modes — ruling 2's constant.
    let explicit = PathBuf::from("/tmp");
    assert_eq!(
        resolve_plain_spawn_cwd(
            NewTabCwd::Default,
            Some(explicit.clone()),
            Some(PathBuf::from("/home/jpt/src")),
            Some(PathBuf::from("/srv"))
        ),
        Some(explicit)
    );
}

#[test]
fn numbered_activation_targets_the_requested_index() {
    assert_eq!(activate_index_target(3, 0), Some(0));
    assert_eq!(activate_index_target(3, 2), Some(2));
}

#[test]
fn numbered_activation_out_of_range_is_a_no_op() {
    assert_eq!(activate_index_target(3, 3), None);
    assert_eq!(activate_index_target(0, 0), None);
}

#[test]
fn last_tab_activation_targets_the_final_index() {
    assert_eq!(activate_last_target(3), Some(2));
    assert_eq!(activate_last_target(1), Some(0));
}

#[test]
fn last_tab_activation_with_no_tabs_is_a_no_op() {
    assert_eq!(activate_last_target(0), None);
}

#[test]
fn remote_target_absent_resolves_to_the_active_tab() {
    assert_eq!(remote_target_index(3, None, 1), Some(1));
}

#[test]
fn remote_target_index_is_one_based() {
    // The wire's N=1 is the list's index 0; N=len is the last tab.
    assert_eq!(remote_target_index(3, Some(1), 0), Some(0));
    assert_eq!(remote_target_index(3, Some(3), 1), Some(2));
}

#[test]
fn remote_target_beyond_the_list_is_none() {
    assert_eq!(remote_target_index(3, Some(4), 1), None);
}

#[test]
fn remote_target_zero_is_none_even_though_the_parser_rejects_it() {
    // Defense in depth: clap already rejects --tab-index 0, but the
    // helper is the boundary any future wire path goes through.
    assert_eq!(remote_target_index(3, Some(0), 1), None);
}

#[test]
fn remote_target_absent_with_no_tabs_is_none() {
    assert_eq!(remote_target_index(0, None, 0), None);
}

#[test]
fn select_tab_target_is_strictly_one_based() {
    // The wire's N=1 is the list's index 0; N=len is the last tab —
    // there is no `9 = last` idiom and no wrap.
    assert_eq!(select_tab_target(3, 1), Some(0));
    assert_eq!(select_tab_target(3, 3), Some(2));
}

#[test]
fn select_tab_target_beyond_the_list_is_none() {
    assert_eq!(select_tab_target(3, 4), None);
}

#[test]
fn select_tab_target_zero_is_none_even_though_the_parser_rejects_it() {
    // Defense in depth: clap already rejects `select-tab 0`, but the
    // helper is the boundary any future wire path goes through.
    assert_eq!(select_tab_target(3, 0), None);
}

#[test]
fn select_tab_target_with_no_tabs_is_none() {
    assert_eq!(select_tab_target(0, 1), None);
}

#[test]
fn move_target_clamps_at_the_list_edges() {
    // Moving left from index 0 or right from the last index stays
    // put — move binds clamp, they do not wrap.
    assert_eq!(move_target(0, 3, MoveDirection::Left), None);
    assert_eq!(move_target(2, 3, MoveDirection::Right), None);
    assert_eq!(move_target(1, 3, MoveDirection::Left), Some(0));
    assert_eq!(move_target(1, 3, MoveDirection::Right), Some(2));
}

#[test]
fn a_bell_raises_attention_to_the_bell_marker() {
    assert_eq!(Attention::None.on_bell(), Attention::Bell);
    assert_eq!(Attention::Activity.on_bell(), Attention::Bell);
}

#[test]
fn output_in_an_inactive_tab_raises_attention_to_activity_at_least() {
    assert_eq!(Attention::None.on_activity(), Attention::Activity);
}

#[test]
fn output_does_not_demote_an_existing_bell_marker() {
    assert_eq!(Attention::Bell.on_activity(), Attention::Bell);
}

#[test]
fn activating_a_tab_clears_its_attention() {
    assert_eq!(Attention::Bell.on_activated(), Attention::None);
    assert_eq!(Attention::Activity.on_activated(), Attention::None);
}

#[test]
fn attention_markers_prefix_the_tab_title() {
    assert_eq!(Attention::None.marker(), None);
    assert_eq!(Attention::Activity.marker(), Some("• "));
    assert_eq!(Attention::Bell.marker(), Some("\u{1f514} "));
}

#[test]
fn a_pin_prefixes_the_title_ahead_of_any_attention_marker() {
    // Ruling 3: pin, then marker, then title — the pin indicator
    // composes ahead of everything, exactly the attention markers'
    // shape (glyph plus a leading space).
    assert_eq!(title_prefix(true, Attention::None), "\u{1f4cc} ");
    assert_eq!(title_prefix(true, Attention::Activity), "\u{1f4cc} • ");
    assert_eq!(title_prefix(true, Attention::Bell), "\u{1f4cc} \u{1f514} ");
}

#[test]
fn an_unpinned_tab_prefixes_the_attention_marker_alone() {
    assert_eq!(title_prefix(false, Attention::None), "");
    assert_eq!(title_prefix(false, Attention::Activity), "• ");
    assert_eq!(title_prefix(false, Attention::Bell), "\u{1f514} ");
}

#[test]
fn sweep_targets_cover_the_sweep_neighborhood() {
    // Today's shape with every tab loose: Others hits every tab
    // but the anchor on both sides, Right only the tabs after it.
    let loose = [false; 5];
    let is_pinned = |pinned: &bool| *pinned;
    assert_eq!(
        sweep_targets(&loose, 1, CloseSweep::Others, is_pinned),
        vec![0, 2, 3, 4]
    );
    assert_eq!(
        sweep_targets(&loose, 1, CloseSweep::Right, is_pinned),
        vec![2, 3, 4]
    );
}

#[test]
fn sweep_targets_skip_pinned_tabs() {
    // A pin refuses the sweeps as surely as a direct close: pinned
    // tabs drop out of the target list while loose neighbors close.
    let flags = [false, false, true, false, true];
    let is_pinned = |pinned: &bool| *pinned;
    assert_eq!(
        sweep_targets(&flags, 1, CloseSweep::Others, is_pinned),
        vec![0, 3]
    );
    assert_eq!(
        sweep_targets(&flags, 1, CloseSweep::Right, is_pinned),
        vec![3]
    );
}

#[test]
fn a_command_line_splits_into_program_and_args() {
    assert_eq!(
        command_line_parts("htop -u root"),
        Some((
            "htop".to_string(),
            vec!["-u".to_string(), "root".to_string()]
        ))
    );
}

#[test]
fn a_bare_command_line_has_no_trailing_args() {
    assert_eq!(
        command_line_parts("htop"),
        Some(("htop".to_string(), Vec::new()))
    );
}

#[test]
fn an_empty_command_line_is_rejected() {
    assert_eq!(command_line_parts(""), None);
    assert_eq!(command_line_parts("   "), None);
}

#[test]
fn cycling_an_empty_list_stays_at_zero() {
    let empty: [u32; 0] = [];
    assert_eq!(cycle_next(&empty, 0), 0);
    assert_eq!(cycle_prev(&empty, 0), 0);
}

#[test]
fn move_tab_forward_returns_target_index() {
    let mut list = vec![10, 20, 30];
    let new_index = move_tab(&mut list, 0, 2);
    assert_eq!(list, vec![20, 30, 10]);
    assert_eq!(new_index, 2);
}

#[test]
fn move_tab_backward_returns_target_index() {
    let mut list = vec![10, 20, 30];
    let new_index = move_tab(&mut list, 2, 0);
    assert_eq!(list, vec![30, 10, 20]);
    assert_eq!(new_index, 0);
}

#[test]
fn reorder_tabs_moves_the_dragged_tab_and_keeps_it_active() {
    let mut list = vec![10, 20, 30];
    let active = reorder_tabs(&mut list, 0, 0, 2);
    assert_eq!(list, vec![20, 30, 10]);
    assert_eq!(active, 2);
}

#[test]
fn reorder_tabs_keeps_a_later_active_tab_its_index() {
    // Dragging tab 0 between tabs 1 and 2 does not disturb tab 30.
    let mut list = vec![10, 20, 30];
    let active = reorder_tabs(&mut list, 2, 0, 1);
    assert_eq!(list, vec![20, 10, 30]);
    assert_eq!(active, 2);
}

#[test]
fn reorder_tabs_shifts_active_when_a_later_tab_lands_before_it() {
    // Dragging tab 2 to the front pushes the active tab 10 to index 1.
    let mut list = vec![10, 20, 30];
    let active = reorder_tabs(&mut list, 0, 2, 0);
    assert_eq!(list, vec![30, 10, 20]);
    assert_eq!(active, 1);
}

#[test]
fn reorder_tabs_shifts_active_when_an_earlier_tab_lands_after_it() {
    // Dragging tab 0 to the end pulls the active tab 20 up to index 0.
    let mut list = vec![10, 20, 30];
    let active = reorder_tabs(&mut list, 1, 0, 2);
    assert_eq!(list, vec![20, 30, 10]);
    assert_eq!(active, 0);
}

#[test]
fn reorder_tabs_to_the_same_index_is_a_no_op() {
    let mut list = vec![10, 20, 30];
    let active = reorder_tabs(&mut list, 1, 1, 1);
    assert_eq!(list, vec![10, 20, 30]);
    assert_eq!(active, 1);
}

#[test]
fn reorder_tabs_ignores_out_of_range_indices() {
    let mut list = vec![10, 20];
    assert_eq!(reorder_tabs(&mut list, 0, 5, 0), 0);
    assert_eq!(list, vec![10, 20]);
}

#[test]
fn next_default_title_numbers_from_tab_id() {
    assert_eq!(next_default_title(TabId(0)), "Terminal 1");
    assert_eq!(next_default_title(TabId(4)), "Terminal 5");
}

#[test]
fn shell_mode_lands_the_raw_report() {
    let title = resolve_title(
        TabTitleMode::Shell,
        Some("vim — src/main.rs"),
        None,
        None,
        TabId(0),
        None,
    );
    assert_eq!(title, "vim — src/main.rs");
}

#[test]
fn a_missing_report_shows_the_default_number() {
    // Also the RIS story: a reset clears the stored report and
    // the default comes back.
    let title = resolve_title(TabTitleMode::Shell, None, None, None, TabId(4), None);
    assert_eq!(title, "Terminal 5");
}

#[test]
fn an_empty_report_stores_as_no_report() {
    // A TUI clearing the title it set (opencode's Ctrl+C is the
    // field report) has one move: OSC 0/2 with an empty payload.
    // Stored as no report — never `Some("")` — the clear lands in
    // the same state a shell reset returns to.
    assert_eq!(
        shell_title_from_report("OpenCode".to_owned()),
        Some("OpenCode".to_owned())
    );
    assert_eq!(shell_title_from_report(String::new()), None);
    // Composed as the Title handler stores then resolves: the
    // cleared tab shows the default number again, never a blank.
    let stored = shell_title_from_report(String::new());
    assert_eq!(
        resolve_title(
            TabTitleMode::Shell,
            stored.as_deref(),
            None,
            None,
            TabId(4),
            None
        ),
        "Terminal 5"
    );
}

#[test]
fn a_rename_wins_in_every_mode() {
    for mode in [
        TabTitleMode::Shell,
        TabTitleMode::AbbreviatedPath,
        TabTitleMode::LastSegment,
        TabTitleMode::Numbered,
        TabTitleMode::Profile,
    ] {
        assert_eq!(
            resolve_title(
                mode,
                Some("jpt@box:~/src"),
                Some(Path::new("/home/jpt/src")),
                None,
                TabId(0),
                Some("work")
            ),
            "work"
        );
    }
}

// --- the Profile arm (profile-title) ---

#[test]
fn profile_mode_shows_the_tab_recorded_profile_name() {
    // The mode's whole point: the tab says `ssh-prod` no matter
    // what the shell's OSC title reports underneath.
    assert_eq!(
        resolve_title(
            TabTitleMode::Profile,
            Some("user@host: ~"),
            None,
            Some("ssh-prod"),
            TabId(0),
            None
        ),
        "ssh-prod"
    );
}

#[test]
fn profile_mode_shows_the_name_before_any_report() {
    // The name needs nothing from the shell: a fresh profile tab
    // is labeled from its first paint.
    assert_eq!(
        resolve_title(
            TabTitleMode::Profile,
            None,
            None,
            Some("top"),
            TabId(0),
            None
        ),
        "top"
    );
}

#[test]
fn profile_mode_rename_pin_wins_over_the_name() {
    // The rename pin is the last word in every mode — Profile is
    // no exception.
    assert_eq!(
        resolve_title(
            TabTitleMode::Profile,
            Some("user@host"),
            None,
            Some("ssh-prod"),
            TabId(0),
            Some("prod box")
        ),
        "prod box"
    );
}

#[test]
fn profile_mode_falls_back_to_shell_behavior_without_a_profile() {
    // A plain tab looks exactly as it does in Shell mode: the raw
    // report when one exists...
    assert_eq!(
        resolve_title(
            TabTitleMode::Profile,
            Some("zsh"),
            None,
            None,
            TabId(0),
            None
        ),
        "zsh"
    );
    // ...and the `Terminal N` default when none does.
    assert_eq!(
        resolve_title(TabTitleMode::Profile, None, None, None, TabId(2), None),
        "Terminal 3"
    );
}

#[test]
fn profile_mode_dangling_id_renders_the_shell_fallback() {
    // Ruling 2: the call site resolves the id at retitle time; a
    // profile deleted under a running tab yields no name, and the
    // mode degrades to Shell behavior — never a stale name.
    assert_eq!(
        resolve_title(
            TabTitleMode::Profile,
            Some("zsh"),
            None,
            None,
            TabId(0),
            None
        ),
        "zsh"
    );
    assert_eq!(
        resolve_title(TabTitleMode::Profile, None, None, None, TabId(4), None),
        "Terminal 5"
    );
}

#[test]
fn numbered_mode_ignores_reports_and_parks_at_the_number() {
    // Guake `use-vte-titles=false`: the title never moves after
    // creation, whatever the shell reports.
    let title = resolve_title(
        TabTitleMode::Numbered,
        Some("jpt@box:~/src"),
        Some(Path::new("/home/jpt/src")),
        None,
        TabId(0),
        None,
    );
    assert_eq!(title, "Terminal 1");
}

#[test]
fn abbreviated_path_shortens_a_tilde_suffix() {
    // SAFETY: the only test that writes HOME; every other
    // resolve_title test uses cwds and reports whose outcome is
    // HOME-independent, so the swap cannot change their results.
    let saved_home = std::env::var("HOME").ok();
    unsafe { std::env::set_var("HOME", "/home/jpt") };
    let title = resolve_title(
        TabTitleMode::AbbreviatedPath,
        Some("jpt@box:~/src/oss/l1br3/ced"),
        Some(Path::new("/home/jpt/src/oss/l1br3/ced")),
        None,
        TabId(0),
        None,
    );
    // The no-report spawn form prefers the same `~` spelling: a
    // fresh tab — or a shell that never titles — shows the
    // abbreviated cwd itself, never the default number, while
    // the cwd is known.
    let spawned = resolve_title(
        TabTitleMode::AbbreviatedPath,
        None,
        Some(Path::new("/home/jpt/src/oss/l1br3/ced")),
        None,
        TabId(0),
        None,
    );
    let spawned_home = resolve_title(
        TabTitleMode::AbbreviatedPath,
        None,
        Some(Path::new("/home/jpt")),
        None,
        TabId(0),
        None,
    );
    match &saved_home {
        Some(home) => unsafe { std::env::set_var("HOME", home) },
        None => unsafe { std::env::remove_var("HOME") },
    }
    assert_eq!(title, "jpt@box:~/s/o/l/ced");
    assert_eq!(spawned, "~/s/o/l/ced");
    assert_eq!(spawned_home, "~");
}

#[test]
fn abbreviated_path_shortens_an_absolute_suffix_identically() {
    // HOME-independent: the absolute spelling is always the first
    // candidate, tilde or not.
    let title = resolve_title(
        TabTitleMode::AbbreviatedPath,
        Some("jpt@box:/home/jpt/src/oss/l1br3/ced"),
        Some(Path::new("/home/jpt/src/oss/l1br3/ced")),
        None,
        TabId(0),
        None,
    );
    assert_eq!(title, "jpt@box:/h/j/s/o/l/ced");
}

#[test]
fn abbreviated_path_leaves_a_non_directory_title_verbatim() {
    let title = resolve_title(
        TabTitleMode::AbbreviatedPath,
        Some("vim — src/main.rs"),
        Some(Path::new("/home/jpt/src")),
        None,
        TabId(0),
        None,
    );
    assert_eq!(title, "vim — src/main.rs");
}

#[test]
fn abbreviated_path_without_a_cwd_shows_the_report_verbatim() {
    let title = resolve_title(
        TabTitleMode::AbbreviatedPath,
        Some("jpt@box:~/src"),
        None,
        None,
        TabId(0),
        None,
    );
    assert_eq!(title, "jpt@box:~/src");
}

#[test]
fn abbreviated_path_without_a_report_abbreviates_the_cwd_itself() {
    // HOME-independent: `/tmp` is no test's HOME, so only the
    // absolute spelling exists and the outcome cannot drift with
    // the environment.
    let title = resolve_title(
        TabTitleMode::AbbreviatedPath,
        None,
        Some(Path::new("/tmp/a/bc/de")),
        None,
        TabId(0),
        None,
    );
    assert_eq!(title, "/t/a/b/de");
}

#[test]
fn abbreviated_path_without_a_report_or_cwd_shows_the_default_number() {
    let title = resolve_title(
        TabTitleMode::AbbreviatedPath,
        None,
        None,
        None,
        TabId(2),
        None,
    );
    assert_eq!(title, "Terminal 3");
}

#[test]
fn last_segment_shows_the_cwds_final_component() {
    let title = resolve_title(
        TabTitleMode::LastSegment,
        Some("jpt@box:~/src/oss/l1br3/ced"),
        Some(Path::new("/home/jpt/src/oss/l1br3/ced")),
        None,
        TabId(0),
        None,
    );
    assert_eq!(title, "ced");
}

#[test]
fn last_segment_renders_root_for_the_root_directory() {
    // Guake verbatim: `/` is "(root)", not the empty string.
    let title = resolve_title(
        TabTitleMode::LastSegment,
        Some("jpt@box:/"),
        Some(Path::new("/")),
        None,
        TabId(0),
        None,
    );
    assert_eq!(title, "(root)");
}

#[test]
fn last_segment_without_a_cwd_shows_the_report_verbatim() {
    // Guake's OSError fallback: an unreadable cwd degrades to the
    // raw report, never an error surface.
    let title = resolve_title(
        TabTitleMode::LastSegment,
        Some("jpt@box:~/src"),
        None,
        None,
        TabId(0),
        None,
    );
    assert_eq!(title, "jpt@box:~/src");
}

#[test]
fn last_segment_at_spawn_shows_the_spawn_cwd_before_any_report() {
    // The form a fresh tab opens with: no shell report exists
    // yet, and the mode must not wait for one — the spawn cwd
    // (seeded in the app's cache at spawn) is already known.
    let title = resolve_title(
        TabTitleMode::LastSegment,
        None,
        Some(Path::new("/home/jpt")),
        None,
        TabId(4),
        None,
    );
    assert_eq!(title, "jpt");
}

#[test]
fn last_segment_with_neither_report_nor_cwd_shows_the_default_number() {
    let title = resolve_title(TabTitleMode::LastSegment, None, None, None, TabId(0), None);
    assert_eq!(title, "Terminal 1");
}

#[test]
fn committed_rename_sets_override_and_displayed_title() {
    let mut title = String::from("vim — src/main.rs");
    let mut title_override = None;
    commit_rename(
        TabTitleMode::Shell,
        Some("vim — src/main.rs"),
        None,
        None,
        TabId(1),
        &mut title,
        &mut title_override,
        "work",
    );
    assert_eq!(title, "work");
    assert_eq!(title_override.as_deref(), Some("work"));
}

#[test]
fn committed_rename_survives_shell_title_changes() {
    let mut title = String::from("Terminal 1");
    let mut title_override = None;
    commit_rename(
        TabTitleMode::Shell,
        None,
        None,
        None,
        TabId(0),
        &mut title,
        &mut title_override,
        "work",
    );
    // A later report resolves under the override and cannot clobber it.
    let resolved = resolve_title(
        TabTitleMode::Shell,
        Some("vim — src/main.rs"),
        None,
        None,
        TabId(0),
        title_override.as_deref(),
    );
    assert_eq!(resolved, "work");
}

#[test]
fn committing_an_empty_rename_clears_the_override_and_re_resolves() {
    let mut title = String::from("work");
    let mut title_override = Some(String::from("work"));
    // The cleared rename falls back to the stored report —
    // immediately, not on the next shell report.
    commit_rename(
        TabTitleMode::Shell,
        Some("vim — src/main.rs"),
        None,
        None,
        TabId(1),
        &mut title,
        &mut title_override,
        "",
    );
    assert_eq!(title_override, None);
    assert_eq!(title, "vim — src/main.rs");
}

#[test]
fn clearing_a_rename_in_numbered_mode_parks_at_the_terminal_number() {
    let mut title = String::from("work");
    let mut title_override = Some(String::from("work"));
    commit_rename(
        TabTitleMode::Numbered,
        Some("jpt@box:~"),
        None,
        None,
        TabId(2),
        &mut title,
        &mut title_override,
        "",
    );
    assert_eq!(title_override, None);
    assert_eq!(title, "Terminal 3");
}

#[test]
fn plan_respawn_keeps_a_user_rename_verbatim_in_title_and_override() {
    // The stickiness rule: a respawn is not a title action, so the
    // rename survives in every mode — displayed title and stored
    // override both.
    for mode in [
        TabTitleMode::Shell,
        TabTitleMode::AbbreviatedPath,
        TabTitleMode::LastSegment,
        TabTitleMode::Numbered,
    ] {
        let plan = plan_respawn(mode, TabId(6), None, Some("work"), "htop");
        assert_eq!(plan.title, "work");
        assert_eq!(plan.title_override.as_deref(), Some("work"));
    }
}

#[test]
fn plan_respawn_without_a_rename_titles_from_the_fresh_terminal_number() {
    let plan = plan_respawn(TabTitleMode::Shell, TabId(6), None, None, "htop");
    assert_eq!(plan.title, "Terminal 7");
}

#[test]
fn plan_respawn_in_last_segment_mode_titles_from_the_seeded_cwd() {
    // The cwd the app hands over is the spawn directory, so the
    // cwd modes resolve from it before any shell report exists.
    let plan = plan_respawn(
        TabTitleMode::LastSegment,
        TabId(6),
        Some(Path::new("/home/jpt/src/ced")),
        None,
        "htop",
    );
    assert_eq!(plan.title, "ced");
}

#[test]
fn plan_respawn_in_abbreviated_mode_titles_from_the_seeded_cwd() {
    // The OSC tracker resets with the new PTY, so AbbreviatedPath
    // has no report to shorten until the new shell emits one; the
    // seeded cwd itself is abbreviated meanwhile — HOME-
    // independent: `/tmp` is no test's HOME, so only the absolute
    // spelling exists.
    let plan = plan_respawn(
        TabTitleMode::AbbreviatedPath,
        TabId(6),
        Some(Path::new("/tmp/src/ced")),
        None,
        "htop",
    );
    assert_eq!(plan.title, "/t/s/ced");
}

#[test]
fn plan_respawn_resets_attention_and_bell_notification_even_from_a_bell() {
    // The plan clears both unconditionally — a Bell the old
    // terminal accrued dies with it, and the new program starts
    // with a clean attention slate.
    let plan = plan_respawn(TabTitleMode::Shell, TabId(6), None, None, "htop");
    assert_eq!(plan.attention, Attention::None);
    assert!(!plan.bell_notified);
}

#[test]
fn plan_respawn_clears_the_ephemeral_flag_a_cmd_tab_carried() {
    // Whatever the tab was before — a `--cmd` tab is ephemeral —
    // a profile respawn makes it a persistent profile tab.
    let plan = plan_respawn(TabTitleMode::Shell, TabId(6), None, None, "htop");
    assert!(!plan.ephemeral);
}

#[test]
fn plan_respawn_records_the_profile_name() {
    let plan = plan_respawn(TabTitleMode::Shell, TabId(6), None, None, "htop");
    assert_eq!(plan.profile.as_deref(), Some("htop"));
}

#[test]
fn plan_respawn_resets_the_shell_title_tracker() {
    // The stored OSC 0/2 report belongs to the dead PTY; the new
    // one has reported nothing yet.
    let plan = plan_respawn(TabTitleMode::Shell, TabId(6), None, None, "htop");
    assert_eq!(plan.shell_title, None);
}

#[test]
fn tab_records_its_spawn_shell() {
    // The heaviest test in the module — the only one constructing
    // a real Terminal, hence a real /bin/sh PTY — because the
    // point under test is exactly what `Tab::new` captures off a
    // live spawn: the shell it was spawned with, recorded so a
    // later session save replays the right one.
    use alacritty_terminal::term::{self as aterm, Config as TermConfig};
    use alacritty_terminal::tty::{Options as PtyOptions, Shell as PtyShell};
    use tokio::sync::mpsc;

    use crate::terminal::TerminalOptions;

    let (tx, _rx) = mpsc::unbounded_channel();
    let terminal = Terminal::new(
        TabId(0),
        tx,
        TermConfig::default(),
        &PtyOptions {
            shell: Some(PtyShell::new("/bin/sh".to_owned(), Vec::new())),
            drain_on_exit: true,
            ..PtyOptions::default()
        },
        &TerminalOptions::default(),
        &aterm::color::Colors::default(),
    )
    .expect("spawn a /bin/sh test terminal");
    let tab = Tab::new(TabId(3), "/bin/bash".to_owned(), terminal);
    assert_eq!(tab.shell, "/bin/bash");
    // A fresh tab is never pinned: only the menu verb — or a
    // restore replaying a saved pin — sets the flag.
    assert!(!tab.pinned);
}
