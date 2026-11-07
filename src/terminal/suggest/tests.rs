use super::*;

/// The alias map's empty default (suggest-aliases spec): the
/// no-declaration case every pre-existing test rides — an empty map
/// resolves every word to itself, today bit-for-bit.
static NO_ALIASES: std::sync::LazyLock<HashMap<String, String>> =
    std::sync::LazyLock::new(HashMap::new);

/// The clear case's defaults: an anchored idle prompt, "gi"
/// typed, and a two-entry ring. Every guard test mutates exactly
/// one field away from this.
fn obs() -> Observation<'static> {
    Observation {
        enabled: true,
        paths_eligible: true,
        paths_enabled: true,
        path_listing: None,
        commands_enabled: true,
        specs_enabled: true,
        aliases: &NO_ALIASES,
        // The loadable table's built-in default (suggest-specs-data
        // spec): every pre-existing test rides today's table,
        // bit-for-bit — no file, the curated five.
        specs_table: builtin_specs(),
        command_catalog: &[],
        // The generator stopgap (suggest-generators spec): no cached
        // sets by default — every pre-existing test rides today's
        // bit-for-bit silence.
        generator_candidates: None,
        focused: true,
        foreground_child: false,
        display_offset: 0,
        cursor_on_bottom_row: true,
        columns: 80,
        cursor_column: 4,
        before_cursor: "$ gi",
        after_cursor_blank: true,
        fresh_prompt: false,
        // The idle-delay gate's clear case (suggest-delay spec):
        // no delay, a line stable forever — today, bit-for-bit.
        delay_ms: 0,
        typed_idle_ms: u16::MAX,
    }
}

/// A suggest adopted at the "$ " settle prompt, seeded with
/// history: "git push" older, "git status" most recent.
fn adopted() -> Suggest {
    let mut suggest = Suggest::new();
    let settle = Observation {
        before_cursor: "$ ",
        cursor_column: 2,
        fresh_prompt: true,
        ..obs()
    };
    step(&settle, &mut suggest);
    let mut ring = VecDeque::new();
    ring.push_back("git push".to_string());
    ring.push_back("git status".to_string());
    suggest.ring = ring;
    suggest
}

// --- the guard table: each guard alone yields None ---

#[test]
fn guard_config_off_yields_none() {
    let mut suggest = adopted();
    let o = Observation {
        enabled: false,
        ..obs()
    };
    assert_eq!(step(&o, &mut suggest), None);
}

#[test]
fn guard_unfocused_yields_none() {
    let mut suggest = adopted();
    let o = Observation {
        focused: false,
        ..obs()
    };
    assert_eq!(step(&o, &mut suggest), None);
}

#[test]
fn guard_foreground_child_yields_none() {
    let mut suggest = adopted();
    let o = Observation {
        foreground_child: true,
        ..obs()
    };
    assert_eq!(step(&o, &mut suggest), None);
}

#[test]
fn guard_scrolled_back_yields_none() {
    let mut suggest = adopted();
    let o = Observation {
        display_offset: 3,
        ..obs()
    };
    assert_eq!(step(&o, &mut suggest), None);
}

#[test]
fn a_prompt_above_the_bottom_row_still_suggests() {
    // The fresh-terminal ruling (hardware trace, 2026-10-24): a new
    // tab's first prompt sits near the TOP of an empty grid — the
    // cursor is nowhere near the bottom row until a screen of output
    // scrolls it there — and the widget draws the ghost at the
    // cursor's own row (cursor-relative, widget.rs), so the
    // bottom-row guard silenced every fresh (and `clear`-ed)
    // terminal for no drawing reason. The chain's real geometry
    // needs are the not-scrolled-back fact (display_offset) and the
    // cursor-at-line-end fact (after_cursor_blank), both checked.
    let mut suggest = adopted();
    let o = Observation {
        cursor_on_bottom_row: false,
        ..obs()
    };
    let shown = step(&o, &mut suggest).expect("a mid-grid prompt suggests");
    assert_eq!(shown.suffix, "t status");
}

#[test]
fn guard_unanchored_yields_none() {
    // A suggest that never adopted: no anchor, so no typed
    // segment can ever be carved out of the line.
    let mut suggest = Suggest::new();
    assert_eq!(step(&obs(), &mut suggest), None);
}

#[test]
fn guard_line_left_the_anchor_yields_none() {
    // The honest degradation for multi-line and right-side
    // prompts (and `read`-style prompts): the line no longer
    // starts with the adopted anchor, so nothing is certain.
    let mut suggest = adopted();
    let o = Observation {
        before_cursor: "root@host:~# gi",
        ..obs()
    };
    assert_eq!(step(&o, &mut suggest), None);
}

#[test]
fn guard_empty_typed_segment_yields_none() {
    let mut suggest = adopted();
    let o = Observation {
        before_cursor: "$ ",
        cursor_column: 2,
        ..obs()
    };
    assert_eq!(step(&o, &mut suggest), None);
}

#[test]
fn guard_no_history_match_yields_none() {
    let mut suggest = adopted();
    let mut ring = VecDeque::new();
    ring.push_back("vim notes".to_string());
    suggest.ring = ring;
    assert_eq!(step(&obs(), &mut suggest), None);
}

#[test]
fn guard_empty_suffix_yields_none() {
    // Typed equals the full most-recent entry: there is nothing
    // left to suggest.
    let mut suggest = adopted();
    let mut ring = VecDeque::new();
    ring.push_back("gi".to_string());
    suggest.ring = ring;
    assert_eq!(step(&obs(), &mut suggest), None);
}

#[test]
fn a_suffix_crossing_the_line_end_clips_to_fit_with_an_ellipsis() {
    // Five columns remain after the cursor; "t status" needs eight.
    // Fish's exact rule (ghost-fit spec): the display clips to the
    // longest prefix fitting free-minus-two — "t s" at three cells —
    // then the two-cell `…`, and the accept still writes the whole
    // suffix. The old drop rule never survives this test.
    let mut suggest = adopted();
    let o = Observation {
        columns: 10,
        ..obs()
    };
    let shown = step(&o, &mut suggest).expect("a too-wide suffix still shows, clipped");
    assert_eq!(shown.suffix, "t status");
    assert_eq!(shown.visible, "t s…");
}

#[test]
fn below_three_free_columns_nothing_draws_but_the_showing_survives_for_accept() {
    // Two columns remain: below fish's three-column threshold the
    // ghost renders not at all (a cut with no marker is never
    // shipped), yet the showing stays alive — Right still writes the
    // full suffix; the widget skips the empty fill.
    let mut suggest = adopted();
    let o = Observation {
        columns: 7,
        ..obs()
    };
    let shown = step(&o, &mut suggest).expect("the showing survives below the draw threshold");
    assert_eq!(shown.suffix, "t status");
    assert_eq!(shown.visible, "");
    assert_eq!(suggest.take_suffix().as_deref(), Some("t status"));
}

#[test]
fn an_accept_stays_whole_when_only_a_prefix_renders() {
    // The clip is display-only: with the visible part truncated,
    // `take_suffix` still answers the full suffix and the word
    // splitter splits the real one — Right writes everything, and
    // Alt+Right takes a fish-shaped word of what is actually there.
    let mut suggest = adopted();
    let o = Observation {
        columns: 10,
        ..obs()
    };
    let shown = step(&o, &mut suggest).expect("the clipped showing exists");
    assert_eq!(shown.visible, "t s…");
    assert_eq!(suggest.take_suffix().as_deref(), Some("t status"));
    assert_eq!(split_word("t status"), ("t", " status"));
}

#[test]
fn clear_case_yields_the_most_recent_match_suffix() {
    let mut suggest = adopted();
    let showing = step(&obs(), &mut suggest).expect("the clear case shows");
    // "git status" is the most recent match, not "git push".
    assert_eq!(showing.suffix, "t status");
    assert_eq!(showing.source, SuggestionSource::History);
}

#[test]
fn suffix_fit_measures_grid_cells_not_chars() {
    // "中文" is two chars but four cells: it fits four remaining
    // columns whole — and at three the clip's budget is one cell, so
    // the two-cell 中 drops whole rather than half-drawn: the
    // visible is the two-cell `…` alone, and the accept still
    // carries both characters. (In-run ruling: this test's drops
    // case is the third flip beyond the chunk's two — its remaining
    // columns sit at exactly three, where the ruled clip draws the
    // marker instead of nothing.)
    let mut suggest = adopted();
    let mut ring = VecDeque::new();
    ring.push_back("echo中文".to_string());
    suggest.ring = ring;
    let typing = Observation {
        before_cursor: "$ echo",
        cursor_column: 6,
        ..obs()
    };
    let fits = Observation {
        columns: 11,
        ..typing
    };
    let clips = Observation {
        columns: 10,
        ..typing
    };
    let whole = step(&fits, &mut suggest).expect("four cells fit four columns");
    assert_eq!(whole.suffix, "中文");
    assert_eq!(
        whole.visible, "中文",
        "an exact fit draws whole, marker-free"
    );
    let clipped = step(&clips, &mut suggest).expect("a too-wide suffix still shows, clipped");
    assert_eq!(clipped.suffix, "中文");
    assert_eq!(clipped.visible, "…");
}

#[test]
fn take_suffix_yields_once_and_clears_the_showing_state() {
    // The accept-once contract: an accept key writes the bytes
    // exactly once; what shows next is re-derived by the next
    // step, never re-read from a stale suffix.
    let mut suggest = adopted();
    let shown = step(&obs(), &mut suggest).expect("the clear case shows");
    assert_eq!(
        suggest.showing().map(|s| s.suffix.as_str()),
        Some("t status")
    );
    assert_eq!(suggest.take_suffix().as_deref(), Some("t status"));
    assert_eq!(suggest.take_suffix(), None, "the suffix is spent");
    assert_eq!(suggest.showing(), None);
    assert_eq!(shown.suffix, "t status");
}

// --- the idle-delay display gate (suggest-delay) ---

#[test]
fn delay_zero_shows_on_the_same_update_as_today() {
    // The default is fish parity, bit-for-bit: delay 0 opens the
    // gate on the same update the candidate appears — even a
    // zero-idle observation (the keystroke's own echo update)
    // shows, and nothing is ever withheld.
    let mut suggest = adopted();
    let just_typed = Observation {
        delay_ms: 0,
        typed_idle_ms: 0,
        ..obs()
    };
    let shown = step(&just_typed, &mut suggest).expect("delay 0 shows immediately");
    assert_eq!(shown.suffix, "t status");
    assert!(!suggest.withheld());
}

#[test]
fn a_candidate_younger_than_the_delay_is_withheld_not_shown() {
    // The gate's silence: 100 ms into a 300 ms wait the ghost is
    // None — yet the withheld fact tells the engine a candidate
    // waits, so it can expose a reveal deadline. Silence with no
    // candidate at all carries no such fact.
    let mut suggest = adopted();
    let young = Observation {
        delay_ms: 300,
        typed_idle_ms: 100,
        ..obs()
    };
    assert_eq!(step(&young, &mut suggest), None);
    assert_eq!(suggest.showing(), None);
    assert!(suggest.withheld(), "a qualifying candidate waits");
    let mut silent = adopted();
    let mut ring = VecDeque::new();
    ring.push_back("vim notes".to_string());
    silent.ring = ring;
    let no_match = Observation {
        delay_ms: 300,
        typed_idle_ms: 100,
        ..obs()
    };
    assert_eq!(step(&no_match, &mut silent), None);
    assert!(!silent.withheld(), "no candidate exists to withhold");
}

#[test]
fn the_withheld_candidate_shows_once_idle_reaches_the_delay() {
    // The gate opens at idle == delay — the same candidate, whole:
    // the delay changes when the ghost is seen, never which
    // suggestion wins.
    let mut suggest = adopted();
    let mature = Observation {
        delay_ms: 300,
        typed_idle_ms: 300,
        ..obs()
    };
    let shown = step(&mature, &mut suggest).expect("the deadline opens the gate");
    assert_eq!(shown.suffix, "t status");
    assert_eq!(shown.source, SuggestionSource::History);
    assert!(!suggest.withheld());
}

#[test]
fn a_keystroke_on_an_established_showing_dismisses_instantly() {
    // No grace period: the ghost shown at the pause dismisses on
    // the very update whose line turned young again — and the new
    // candidate starts its own wait.
    let mut suggest = adopted();
    let paused = Observation {
        delay_ms: 300,
        typed_idle_ms: 400,
        ..obs()
    };
    assert!(step(&paused, &mut suggest).is_some());
    let typed_again = Observation {
        before_cursor: "$ git",
        cursor_column: 5,
        delay_ms: 300,
        typed_idle_ms: 10,
        ..obs()
    };
    assert_eq!(step(&typed_again, &mut suggest), None);
    assert_eq!(suggest.showing(), None);
    assert!(suggest.withheld(), "the new candidate waits its delay");
}

// --- the ring ---

#[test]
fn ring_evicts_the_oldest_past_the_cap() {
    let mut ring = VecDeque::new();
    for i in 0..=RING_CAP {
        commit(&mut ring, &format!("cmd-{i}"));
    }
    assert_eq!(ring.len(), RING_CAP);
    assert_eq!(
        ring.front().map(String::as_str),
        Some("cmd-1"),
        "the oldest entry (cmd-0) evicted"
    );
}

#[test]
fn ring_promotes_a_retyped_command_to_most_recent() {
    let mut ring = VecDeque::new();
    commit(&mut ring, "ls");
    commit(&mut ring, "cd");
    commit(&mut ring, "ls");
    assert_eq!(ring.len(), 2, "the retype promoted, never duplicated");
    assert_eq!(ring.back().map(String::as_str), Some("ls"));
}

#[test]
fn ring_never_records_blank_lines() {
    let mut ring = VecDeque::new();
    commit(&mut ring, "");
    commit(&mut ring, " \t ");
    assert!(ring.is_empty());
}

#[test]
fn commit_fires_on_the_idle_to_running_transition_with_the_freshest_capture() {
    let mut suggest = adopted();
    // Typing refreshes the capture but records nothing.
    step(
        &Observation {
            before_cursor: "$ git stash",
            cursor_column: 11,
            ..obs()
        },
        &mut suggest,
    );
    assert_eq!(
        suggest.ring.back().map(String::as_str),
        Some("git status"),
        "idle typing never commits"
    );
    // Enter: the child takes the foreground, and the freshest
    // capture lands in the ring as the most recent entry.
    step(
        &Observation {
            foreground_child: true,
            ..obs()
        },
        &mut suggest,
    );
    assert_eq!(suggest.ring.back().map(String::as_str), Some("git stash"));
}

// --- the anchor ---

#[test]
fn anchor_adopts_at_the_settle_fresh_prompt() {
    let mut suggest = Suggest::new();
    step(
        &Observation {
            before_cursor: "$ ",
            cursor_column: 2,
            fresh_prompt: true,
            ..obs()
        },
        &mut suggest,
    );
    assert_eq!(suggest.anchor.as_deref(), Some("$ "));
}

#[test]
fn anchor_re_adopts_after_each_finish_replacing_the_old_one() {
    let mut suggest = Suggest::new();
    // The settle adopts "$ ".
    step(
        &Observation {
            before_cursor: "$ ",
            cursor_column: 2,
            fresh_prompt: true,
            ..obs()
        },
        &mut suggest,
    );
    // A command runs, then finishes under a different prompt:
    // the running→idle diff arms, the fresh-prompt observation
    // adopts, and the old anchor is replaced.
    step(
        &Observation {
            before_cursor: "$ git status",
            cursor_column: 11,
            ..obs()
        },
        &mut suggest,
    );
    step(
        &Observation {
            foreground_child: true,
            ..obs()
        },
        &mut suggest,
    );
    step(
        &Observation {
            before_cursor: "% ",
            cursor_column: 2,
            fresh_prompt: true,
            ..obs()
        },
        &mut suggest,
    );
    assert_eq!(suggest.anchor.as_deref(), Some("% "));
    // And the new anchor keys suggestions immediately: ring
    // entries are commands, anchor-less.
    let mut ring = VecDeque::new();
    ring.push_back("echo hi".to_string());
    suggest.ring = ring;
    let shown = step(
        &Observation {
            before_cursor: "% ec",
            cursor_column: 4,
            ..obs()
        },
        &mut suggest,
    )
    .expect("the replaced anchor keys the new prompt");
    assert_eq!(shown.suffix, "ho hi");
}

#[test]
fn anchor_adoption_requires_a_fresh_prompt_observation() {
    // Armed (fresh state) but no fresh-prompt moment: no
    // adoption, so typing stays silent.
    let mut suggest = Suggest::new();
    step(
        &Observation {
            before_cursor: "$ ",
            cursor_column: 2,
            ..obs()
        },
        &mut suggest,
    );
    assert_eq!(suggest.anchor, None);
}

#[test]
fn anchor_adoption_requires_the_cursor_at_line_end() {
    // The post-finish update caught a line with text after the
    // cursor: not certainly a bare prompt, so no adoption.
    let mut suggest = Suggest::new();
    step(
        &Observation {
            before_cursor: "$ vi src",
            cursor_column: 8,
            after_cursor_blank: false,
            fresh_prompt: true,
            ..obs()
        },
        &mut suggest,
    );
    assert_eq!(suggest.anchor, None);
}

#[test]
fn anchor_adoption_declines_a_non_whitespace_terminated_line() {
    // In-run ruling (build record): a type-ahead user's buffered
    // keystrokes can echo into the same update as the post-finish
    // prompt redraw, leaving a half-typed line that still has the
    // cursor at its end. A line that does not end in whitespace
    // is not certainly a bare prompt — adoption waits rather than
    // key suggestions on the half-typed tail (never a wrong
    // suffix).
    let mut suggest = Suggest::new();
    step(
        &Observation {
            before_cursor: "$ l",
            cursor_column: 3,
            fresh_prompt: true,
            ..obs()
        },
        &mut suggest,
    );
    assert_eq!(suggest.anchor, None);
}

// --- the splitter ---

#[test]
fn splitter_takes_leading_spaces_through_the_next_boundary() {
    assert_eq!(split_word(" -la --color"), (" -la", " --color"));
}

#[test]
fn splitter_takes_a_bare_word_and_leaves_the_rest_suggestable() {
    assert_eq!(split_word("src/main.rs -la"), ("src/main.rs", " -la"));
}

#[test]
fn splitter_consumes_a_lone_word_entirely() {
    assert_eq!(split_word(" -la"), (" -la", ""));
}

// --- the tint ---

#[test]
fn ghost_color_is_the_fg_bg_channel_average_never_the_background() {
    let mut colors = Colors::default();
    colors[NamedColor::Foreground as usize] = Some(Rgb {
        r: 200,
        g: 100,
        b: 50,
    });
    colors[NamedColor::Background as usize] = Some(Rgb { r: 0, g: 20, b: 40 });
    let ghost = ghost_color(&colors);
    assert_eq!(ghost.as_rgba(), [100, 60, 45, 255]);
    assert_ne!(ghost, cosmic_text::Color::rgb(0, 20, 40));
}

// --- the token shape (the merged seam's input vocabulary) ---

#[test]
fn command_position_holds_while_the_first_word_is_open() {
    assert!(is_command_position("gi"));
    // Leading whitespace never starts an argument.
    assert!(is_command_position("  car"));
    // The escaped space keeps the word open — still command
    // position.
    assert!(is_command_position("git\\ myp"));
}

#[test]
fn command_position_ends_at_the_first_unescaped_whitespace() {
    assert!(!is_command_position("git che"));
    assert!(!is_command_position("echo he"));
}

#[test]
fn the_trailing_token_carves_the_word_being_entered() {
    assert_eq!(trailing_token("git che"), "che");
    // The escaped space keeps the word open: the carve yields the
    // whole still-open word.
    assert_eq!(trailing_token("git\\ myp"), "git\\ myp");
    assert_eq!(trailing_token("  car"), "car");
    assert_eq!(trailing_token("git che -la"), "-la");
}

#[test]
fn a_path_shaped_token_is_the_home_root_or_carries_a_slash() {
    for token in ["~", "~/sr", "../co", "./te", "/usr/bi", "sr/x"] {
        assert!(is_path_shaped(token), "{token:?} is path-shaped");
    }
}

#[test]
fn a_plain_word_or_flag_is_not_path_shaped() {
    // A lone `.`/`..`, a `~user` reference, and a dotfile name
    // are plain words until a slash appears; flags never are.
    for token in ["src", "-la", "--flag", "che", ".", "..", ".hidden", "~jpt"] {
        assert!(!is_path_shaped(token), "{token:?} is not path-shaped");
    }
}

// --- the path provider's spawn gate (path-suggest) ---

#[test]
fn the_shell_allowlist_admits_the_etc_shells_regulars() {
    // The ruled default set, pinned as data: the eight
    // `/etc/shells` regulars. `nu` and `elvish` are deliberately
    // absent — an unknown shell degrades to silence, never a
    // guess.
    for shell in SHELL_ALLOWLIST {
        assert!(is_allowlisted_shell(shell), "{shell} is allowlisted");
    }
}

#[test]
fn the_spawn_gate_reads_the_spawned_programs_basename() {
    // Spawn truth arrives as the resolved program word — a path
    // for configured and recorded shells, a bare name for a
    // profile command's leading program; the gate reads the
    // basename either way.
    assert!(is_allowlisted_shell("/bin/bash"));
    assert!(is_allowlisted_shell("/usr/bin/fish"));
}

#[test]
fn the_spawn_gate_rejects_non_shells_unknown_shells_and_nothing_at_all() {
    // `ssh` and `python3` are the honesty line — those tabs never
    // suggest local paths. `nu`/`elvish` are unknown shells; a
    // custom-path shell outside the list is not a regular; the
    // empty string is the options default. All silent.
    for program in ["ssh", "python3", "nu", "elvish", "/opt/fancysh", ""] {
        assert!(
            !is_allowlisted_shell(program),
            "{program} is not allowlisted"
        );
    }
}

// --- the merged seam's dispatch ---

#[test]
fn source_rank_pins_the_ruled_priority() {
    // The ruled priority, stated as data: history answers first —
    // the tab's own truth — then path, then the curated specs,
    // then the command catalog (the specs variant slots between
    // path and catalog, renumbering there, never reordering). Each
    // later module appends its variant here; the ranks must keep
    // strictly increasing in exactly this order.
    let ruled_order = [
        SuggestionSource::History,
        SuggestionSource::Path,
        SuggestionSource::Specs,
        SuggestionSource::Catalog,
        SuggestionSource::Generator,
    ];
    assert_eq!(source_rank(SuggestionSource::History), 0);
    assert_eq!(source_rank(SuggestionSource::Path), 1);
    assert_eq!(source_rank(SuggestionSource::Specs), 2);
    assert_eq!(source_rank(SuggestionSource::Catalog), 3);
    assert_eq!(source_rank(SuggestionSource::Generator), 4);
    for pair in ruled_order.windows(2) {
        assert!(
            source_rank(pair[0]) < source_rank(pair[1]),
            "the order above is the merge's order"
        );
    }
}

#[test]
fn menu_source_rank_pins_the_scoped_first_order() {
    // The menu's own order, stated as data (suggest-menu spec,
    // amended 2026-11-06): the seat-scoped providers — path, specs,
    // generator — above history, the command catalog last (the
    // scope-less command seat keeps history as its leading
    // context). The ghost's `source_rank` stays locked
    // history-first; only the list leads with the keyword's scope.
    assert_eq!(menu_source_rank(SuggestionSource::Path), 0);
    assert_eq!(menu_source_rank(SuggestionSource::Specs), 1);
    assert_eq!(menu_source_rank(SuggestionSource::Generator), 2);
    assert_eq!(menu_source_rank(SuggestionSource::History), 3);
    assert_eq!(menu_source_rank(SuggestionSource::Catalog), 4);
    // The two orders are genuinely distinct seams: history is first
    // in the ghost's table and demoted below the scoped tier in the
    // menu's alone — the amendment would silently undo itself if
    // either rank table collapsed into the other.
    assert_eq!(
        source_rank(SuggestionSource::History),
        0,
        "the ghost keeps history first"
    );
    assert!(
        menu_source_rank(SuggestionSource::History) > menu_source_rank(SuggestionSource::Generator),
        "history demoted below the scoped tier in the menu alone"
    );
}

#[test]
fn the_dispatch_asks_history_and_carries_its_provenance() {
    let suggest = adopted();
    let (suffix, source) =
        merged_candidate("gi", &suggest, &obs()).expect("history answers the clear case");
    assert_eq!(suffix, "t status");
    assert_eq!(source, SuggestionSource::History);
}

// --- the path provider (path-suggest) ---

/// A reduced listing from (&str, is-dir) pairs.
fn listing(entries: &[(&str, bool)]) -> Vec<(String, bool)> {
    entries
        .iter()
        .map(|(name, dir)| (name.to_string(), *dir))
        .collect()
}

/// One anchored step with `typed` after the "$ " anchor and the
/// given parent listing — the provider's end-to-end shape.
fn step_path(
    typed: &str,
    listing: Option<&[(String, bool)]>,
    suggest: &mut Suggest,
) -> Option<Showing> {
    let mut before = String::from("$ ");
    before.push_str(typed);
    let observation = Observation {
        before_cursor: &before,
        cursor_column: before.len(),
        path_listing: listing,
        ..obs()
    };
    step(&observation, suggest)
}

#[test]
fn the_path_provider_completes_the_final_component_with_the_dir_slash() {
    let mut suggest = adopted();
    let unique = listing(&[("src", true)]);
    let shown = step_path("~/sr", Some(&unique), &mut suggest).expect("the unique match completes");
    assert_eq!(shown.suffix, "c/");
    assert_eq!(shown.source, SuggestionSource::Path);
}

#[test]
fn an_ambiguous_field_completes_its_shared_prefix_not_a_guess() {
    // src and srv both live ahead of `~/sr`: guessing `c/` is wrong
    // half the time, so the shared prefix answers — nothing new at
    // `sr`, `r` at `s` (no slash: the boundary itself is ambiguous).
    let entries = listing(&[("src", true), ("srv", true)]);
    let mut suggest = adopted();
    assert_eq!(
        step_path("~/sr", Some(&entries), &mut suggest),
        None,
        "no shared extension beyond the typed component"
    );
    let mut suggest = adopted();
    let shown =
        step_path("~/s", Some(&entries), &mut suggest).expect("the shared prefix completes");
    assert_eq!(shown.suffix, "r");
}

#[test]
fn a_name_that_prefixes_another_extends_only_to_the_prefix() {
    // `src` and `src-cache`: the shared prefix is `src`, so `~/sr`
    // completes `c` — without the slash, since the boundary between
    // the two names is ambiguous.
    let entries = listing(&[("src", true), ("src-cache", false)]);
    let mut suggest = adopted();
    let shown =
        step_path("~/sr", Some(&entries), &mut suggest).expect("the shared prefix completes");
    assert_eq!(shown.suffix, "c");
}

#[test]
fn the_trailing_token_completes_from_any_word_of_the_line() {
    // `~/sr`, `../co`, `./te`, `/usr/bi`, `sr/x` — the trailing
    // token is what completes, wherever it sits; the listing is
    // whatever the engine reduced for that token's parent.
    let entries = listing(&[("xt", true)]);
    let mut suggest = adopted();
    let shown = step_path("cd sr/x", Some(&entries), &mut suggest)
        .expect("the trailing token reaches the provider");
    assert_eq!(shown.suffix, "t/");
}

#[test]
fn plain_words_and_flags_never_reach_the_path_provider() {
    let entries = listing(&[("src", true), ("la", true)]);
    let mut suggest = adopted();
    assert_eq!(step_path("vim src", Some(&entries), &mut suggest), None);
    assert_eq!(step_path("ls -la", Some(&entries), &mut suggest), None);
}

#[test]
fn a_file_match_carries_no_slash() {
    let entries = listing(&[("alpha.txt", false), ("beta", true)]);
    let mut suggest = adopted();
    let file =
        step_path("/usr/a", Some(&entries), &mut suggest).expect("the file matches the component");
    assert_eq!(file.suffix, "lpha.txt");
    let dir = step_path("/usr/b", Some(&entries), &mut suggest)
        .expect("the directory matches the component");
    assert_eq!(dir.suffix, "eta/");
}

#[test]
fn a_fully_typed_directory_suggests_its_slash_a_file_suggests_nothing() {
    let dirs = listing(&[("src", true)]);
    let mut suggest = adopted();
    assert_eq!(
        step_path("~/src", Some(&dirs), &mut suggest).map(|s| s.suffix),
        Some("/".to_string()),
        "the slash alone completes a fully typed directory"
    );
    let files = listing(&[("src", false)]);
    assert_eq!(
        step_path("~/src", Some(&files), &mut suggest),
        None,
        "a fully typed file has nothing left to add"
    );
}

#[test]
fn dotfiles_match_only_a_dot_prefixed_component() {
    let entries = listing(&[(".hidden", true), ("afile", false)]);
    let mut suggest = adopted();
    let dot = step_path("~/.", Some(&entries), &mut suggest).expect("the dot reaches the dotfile");
    assert_eq!(dot.suffix, "hidden/");
    let plain = step_path("~/a", Some(&entries), &mut suggest)
        .expect("the plain component sees the plain entry");
    assert_eq!(plain.suffix, "file");
    assert_eq!(
        step_path("~/h", Some(&entries), &mut suggest),
        None,
        "h cannot see .hidden without the leading dot"
    );
}

#[test]
fn the_bare_home_root_completes_the_first_plain_entry() {
    // The empty component honors the dotfile convention too: the
    // first non-dot entry wins.
    let entries = listing(&[(".config", true), ("Documents", true)]);
    let mut suggest = adopted();
    let shown =
        step_path("~", Some(&entries), &mut suggest).expect("the HOME root lists its entries");
    assert_eq!(shown.suffix, "Documents/");
}

#[test]
fn escapable_characters_take_a_backslash() {
    let entries = listing(&[("my file.txt", false), ("a(b)c", true)]);
    let mut suggest = adopted();
    let spaced = step_path("~/my", Some(&entries), &mut suggest).expect("a space escapes safely");
    assert_eq!(spaced.suffix, "\\ file.txt");
    let parens =
        step_path("~/a", Some(&entries), &mut suggest).expect("the metacharacters escape safely");
    assert_eq!(parens.suffix, "\\(b\\)c/");
}

#[test]
fn the_whitelist_rides_verbatim() {
    // Every whitelisted punctuation mark in one entry, plus the
    // directory slash.
    let entries = listing(&[("a-_./:=,+@%^{}z", true)]);
    let mut suggest = adopted();
    let shown = step_path("~/a", Some(&entries), &mut suggest)
        .expect("whitelisted bytes are safe to write");
    assert_eq!(shown.suffix, "-_./:=,+@%^{}z/");
}

#[test]
fn anything_the_quote_tier_cannot_save_is_silence() {
    // A newline and a control character stay unwritable in every
    // form — backslash, quote — so the entry silences.
    for (name, typed) in [
        ("bad\nname", "~/bad"),
        ("bad\x07bell", "~/bad"),
        // A name needing the quote tier that itself carries a quote:
        // the `\''` dance is voodoo, so silence.
        ("it's naïve", "~/i"),
    ] {
        let entries = listing(&[(name, true)]);
        let mut suggest = adopted();
        assert_eq!(
            step_path(typed, Some(&entries), &mut suggest),
            None,
            "{name:?} must not be suggested"
        );
    }
}

#[test]
fn printable_unusual_bytes_complete_single_quoted() {
    // The quote tier: a remainder reaching past the backslash sets,
    // all printable and quote-free, rides whole inside single quotes
    // — certainly safe to write in the POSIX shells and fish alike.
    let files = listing(&[("café", false)]);
    let mut suggest = adopted();
    let shown =
        step_path("~/caf", Some(&files), &mut suggest).expect("the printable name quotes safely");
    assert_eq!(shown.suffix, "'é'");
    let dirs = listing(&[("bad`tick", true)]);
    let mut suggest = adopted();
    let ticked =
        step_path("~/bad", Some(&dirs), &mut suggest).expect("a backtick is literal inside quotes");
    assert_eq!(ticked.suffix, "'`tick/'");
}

#[test]
fn history_outranks_the_path_provider_when_both_match() {
    let mut suggest = adopted();
    let mut ring = VecDeque::new();
    ring.push_back("~/src/project".to_string());
    suggest.ring = ring;
    let entries = listing(&[("src", true)]);
    let shown =
        step_path("~/sr", Some(&entries), &mut suggest).expect("someone answers the path token");
    assert_eq!(shown.suffix, "c/project", "the tab's own truth first");
    assert_eq!(shown.source, SuggestionSource::History);
}

#[test]
fn the_paths_config_gate_off_restores_phase_one_exactly() {
    // One mutated field: the path provider stays silent for a
    // path token — and history keeps suggesting "gi" exactly as
    // phase 1 did, listing or not.
    let entries = listing(&[("src", true)]);
    let mut suggest = adopted();
    let before = String::from("$ ~/sr");
    let gated = Observation {
        paths_enabled: false,
        before_cursor: &before,
        cursor_column: before.len(),
        path_listing: Some(&entries),
        ..obs()
    };
    assert_eq!(step(&gated, &mut suggest), None);
    let mut suggest = adopted();
    let history = Observation {
        paths_enabled: false,
        path_listing: Some(&entries),
        ..obs()
    };
    let shown = step(&history, &mut suggest).expect("history is unaffected");
    assert_eq!(shown.suffix, "t status");
    assert_eq!(shown.source, SuggestionSource::History);
}

#[test]
fn a_spawn_gate_shut_silences_the_path_provider() {
    // The ssh honesty line, model-side: an ineligible tab never
    // suggests paths, whatever listing an observation carries.
    let entries = listing(&[("src", true)]);
    let mut suggest = adopted();
    let before = String::from("$ ~/sr");
    let gated = Observation {
        paths_eligible: false,
        before_cursor: &before,
        cursor_column: before.len(),
        path_listing: Some(&entries),
        ..obs()
    };
    assert_eq!(step(&gated, &mut suggest), None);
}

#[test]
fn the_clip_measures_the_escaped_form() {
    // "\ file.txt" is ten cells; the cursor sits at column 6, so
    // seventeen columns fit it whole and sixteen clip it — the clip
    // measures the escaped form, budgeting seven cells of prefix
    // plus the two-cell `…`.
    let entries = listing(&[("my file.txt", false)]);
    let mut suggest = adopted();
    let before = String::from("$ ~/my");
    let fits = Observation {
        before_cursor: &before,
        cursor_column: before.len(),
        columns: 17,
        path_listing: Some(&entries),
        ..obs()
    };
    let clips = Observation {
        columns: 16,
        ..fits
    };
    let whole = step(&fits, &mut suggest).expect("the escaped suffix fits whole");
    assert_eq!(whole.suffix, "\\ file.txt");
    assert_eq!(whole.visible, "\\ file.txt");
    let clipped =
        step(&clips, &mut suggest).expect("a too-wide escaped suffix still shows, clipped");
    assert_eq!(clipped.suffix, "\\ file.txt");
    assert_eq!(clipped.visible, "\\ file.…");
}

#[test]
fn no_listing_is_no_path_context() {
    // The engine reduces no listing (a plain word, an ineligible
    // tab): the provider has nothing to say.
    let mut suggest = adopted();
    assert_eq!(step_path("~/sr", None, &mut suggest), None);
}

// --- the command-name provider (command-catalog) ---

/// A catalog snapshot from names — sorted, as the engine's walk
/// leaves it.
fn catalog(names: &[&str]) -> Vec<String> {
    names.iter().map(ToString::to_string).collect()
}

/// The catalog provider's anchored suggest: adopted with a ring
/// that never matches, so history's seat stays empty and the
/// catalog is the one answering.
fn catalog_suggest() -> Suggest {
    let mut suggest = adopted();
    let mut ring = VecDeque::new();
    ring.push_back("vim notes".to_string());
    suggest.ring = ring;
    suggest
}

/// One anchored step with `typed` after the "$ " anchor and a
/// catalog snapshot — the provider's end-to-end shape.
fn step_word(typed: &str, names: &[String], suggest: &mut Suggest) -> Option<Showing> {
    let mut before = String::from("$ ");
    before.push_str(typed);
    let observation = Observation {
        before_cursor: &before,
        cursor_column: before.len(),
        command_catalog: names,
        ..obs()
    };
    step(&observation, suggest)
}

#[test]
fn the_catalog_completes_a_command_word_at_command_position() {
    // The empty-ring story: `gi` finishes to the shortest local
    // match with the catalog's provenance, and leading
    // whitespace never starts an argument.
    let names = catalog(&["cargo", "git", "giftopnm", "gitk"]);
    let mut suggest = catalog_suggest();
    let shown =
        step_word("gi", &names, &mut suggest).expect("the catalog answers the command word");
    assert_eq!(shown.suffix, "t");
    assert_eq!(shown.source, SuggestionSource::Catalog);
    let mut suggest = catalog_suggest();
    let spaced = step_word("  car", &names, &mut suggest)
        .expect("a space-led line is still command position");
    assert_eq!(spaced.suffix, "go");
}

#[test]
fn argument_position_never_reaches_the_catalog() {
    // The catalog completes the command word only: once a second
    // word is open, nothing it knows applies. `git che` does
    // answer — from the specs table (flag-specs' headline case),
    // never the catalog; `echo he` stays silent end to end.
    let names = catalog(&["git", "echo"]);
    let mut suggest = catalog_suggest();
    let shown = step_word("git che", &names, &mut suggest).expect("the specs table answers");
    assert_eq!(shown.suffix, "ckout");
    assert_eq!(shown.source, SuggestionSource::Specs);
    let mut suggest = catalog_suggest();
    assert_eq!(step_word("echo he", &names, &mut suggest), None);
}

#[test]
fn the_shortest_match_wins_ties_alphabetical() {
    // The ruled policy: `gi` suggests `git` ahead of `giftopnm`
    // (shortest wins), and equal lengths order alphabetically.
    let names = catalog(&["giftopnm", "git", "gitk"]);
    let mut suggest = catalog_suggest();
    let shown = step_word("gi", &names, &mut suggest).expect("someone matches");
    assert_eq!(shown.suffix, "t", "git is the shortest match");
    let tied = catalog(&["gita", "gitb"]);
    let mut suggest = catalog_suggest();
    let won = step_word("gi", &tied, &mut suggest).expect("a tie still answers");
    assert_eq!(won.suffix, "ta", "the alphabetically-first of equal length");
}

#[test]
fn a_one_character_command_word_is_silence_from_the_catalog() {
    // The no-information floor (suggest-accuracy spec): a
    // one-character strict prefix over the uncurated PATH namespace
    // carries no intent — shortest-wins among hundreds of names is a
    // coin flip. Two characters is the flagship's own threshold:
    // `gi` → `git` survives, pinned beside the silence.
    let names = catalog(&["g++", "git", "grep"]);
    let mut suggest = catalog_suggest();
    assert_eq!(
        step_word("g", &names, &mut suggest),
        None,
        "a one-character word carries no intent over the PATH catalog"
    );
    let mut suggest = catalog_suggest();
    let shown = step_word("gi", &names, &mut suggest).expect("two characters still answers");
    assert_eq!(shown.suffix, "t");
    assert_eq!(shown.source, SuggestionSource::Catalog);
}

#[test]
fn a_history_backed_one_character_word_still_suggests() {
    // History is untouched by the floor: on a lived-in tab `g`
    // completes from the tab's own ring — only the fresh-tab
    // one-character catalog flash goes quiet.
    let names = catalog(&["g++"]);
    let mut suggest = adopted();
    let shown = step_word("g", &names, &mut suggest).expect("history answers one character");
    assert_eq!(shown.suffix, "it status");
    assert_eq!(shown.source, SuggestionSource::History);
}

#[test]
fn an_exactly_typed_command_word_yields_no_catalog_candidate() {
    // The match must extend the word: `git` with `git` present
    // suggests nothing — and a bare space is no word at all.
    let names = catalog(&["git"]);
    let mut suggest = catalog_suggest();
    assert_eq!(step_word("git", &names, &mut suggest), None);
    let mut suggest = catalog_suggest();
    assert_eq!(step_word(" ", &names, &mut suggest), None);
}

#[test]
fn history_outranks_the_catalog_when_both_match() {
    // The merge's ruled priority: the tab's own past answers
    // first — after one real `git status`, the catalog's `gitk`
    // never gets a seat.
    let mut suggest = adopted();
    let names = catalog(&["gitk"]);
    let shown = step_word("gi", &names, &mut suggest).expect("someone answers");
    assert_eq!(shown.suffix, "t status", "the tab's own truth first");
    assert_eq!(shown.source, SuggestionSource::History);
}

#[test]
fn the_catalog_rides_the_same_spawn_gate_as_paths() {
    // The ssh honesty line, model-side: an ineligible tab never
    // suggests command names, whatever catalog an observation
    // carries — the catalog describes the local machine.
    let names = catalog(&["git"]);
    let mut suggest = catalog_suggest();
    let before = String::from("$ gi");
    let gated = Observation {
        paths_eligible: false,
        before_cursor: &before,
        cursor_column: before.len(),
        command_catalog: &names,
        ..obs()
    };
    assert_eq!(step(&gated, &mut suggest), None);
}

#[test]
fn the_commands_gate_off_restores_phase_one_exactly() {
    // One mutated field: the catalog stays silent for a command
    // word — and history keeps suggesting "gi" exactly as phase 1
    // did, catalog or not.
    let names = catalog(&["git"]);
    let mut suggest = catalog_suggest();
    let before = String::from("$ gi");
    let gated = Observation {
        commands_enabled: false,
        before_cursor: &before,
        cursor_column: before.len(),
        command_catalog: &names,
        ..obs()
    };
    assert_eq!(step(&gated, &mut suggest), None);
    let mut suggest = adopted();
    let history = Observation {
        commands_enabled: false,
        command_catalog: &names,
        ..obs()
    };
    let shown = step(&history, &mut suggest).expect("history is unaffected");
    assert_eq!(shown.suffix, "t status");
    assert_eq!(shown.source, SuggestionSource::History);
}

#[test]
fn a_catalog_name_the_shell_would_mangle_escapes_quotes_or_silences() {
    // The suffix rides the path provider's escape rule: a
    // spaces-named executable completes to its backslashed form, a
    // printable non-ASCII name rides the quote tier, and a name
    // neither tier saves is silence, never a wrong write.
    let spaced = catalog(&["my cmd"]);
    let mut suggest = catalog_suggest();
    let shown = step_word("my", &spaced, &mut suggest).expect("the space escapes safely");
    assert_eq!(shown.suffix, "\\ cmd");
    let odd = catalog(&["caf\u{e9}"]);
    let mut suggest = catalog_suggest();
    let quoted = step_word("caf", &odd, &mut suggest).expect("the printable name quotes safely");
    assert_eq!(quoted.suffix, "'é'");
}

// --- the curated-specs provider (flag-specs) ---

/// The specs provider's suggest: adopted with a ring that never
/// matches, so history's seat stays empty and the specs table is
/// the one answering.
fn specs_suggest() -> Suggest {
    let mut suggest = adopted();
    let mut ring = VecDeque::new();
    ring.push_back("vim notes".to_string());
    suggest.ring = ring;
    suggest
}

/// One anchored step with `typed` after the "$ " anchor — the
/// specs provider's end-to-end shape (no listing, no catalog: the
/// specs table is the only provider that can answer).
fn step_typed(typed: &str, suggest: &mut Suggest) -> Option<Showing> {
    let mut before = String::from("$ ");
    before.push_str(typed);
    let observation = Observation {
        before_cursor: &before,
        cursor_column: before.len(),
        ..obs()
    };
    step(&observation, suggest)
}

#[test]
fn the_specs_complete_a_subcommand_prefix_with_provenance() {
    // `git che` finishes to the table's checkout — the strict
    // prefix, the must-extend, and shortest-wins all met — with
    // the specs provenance the merge carries. The suffix rides
    // `escape_for_shell` like every provider; the table's
    // alphanumeric entries pass it verbatim.
    let mut suggest = specs_suggest();
    let shown = step_typed("git che", &mut suggest).expect("the specs answer the subcommand seat");
    assert_eq!(shown.suffix, "ckout");
    assert_eq!(shown.source, SuggestionSource::Specs);
}

#[test]
fn the_command_word_stays_the_catalogs_seat() {
    // `gi` is the command word being entered: the specs table
    // never completes it — the spawn catalog answers as before.
    let names = catalog(&["git"]);
    let mut suggest = specs_suggest();
    let shown = step_word("gi", &names, &mut suggest).expect("the catalog answers its seat");
    assert_eq!(shown.suffix, "t");
    assert_eq!(shown.source, SuggestionSource::Catalog);
}

#[test]
fn the_specs_gates_each_silence_the_provider() {
    // One mutated field each: the config gate off, and the spawn
    // gate shut — the ssh honesty line, riding the same gate as
    // paths and catalog by ruling (OQ 1).
    let before = String::from("$ git che");
    let gated = Observation {
        specs_enabled: false,
        before_cursor: &before,
        cursor_column: before.len(),
        ..obs()
    };
    let mut suggest = specs_suggest();
    assert_eq!(step(&gated, &mut suggest), None);
    let ineligible = Observation {
        paths_eligible: false,
        before_cursor: &before,
        cursor_column: before.len(),
        ..obs()
    };
    let mut suggest = specs_suggest();
    assert_eq!(step(&ineligible, &mut suggest), None);
}

#[test]
fn history_outranks_specs_when_both_match() {
    // The merge's ruled priority: after one real `git checkout
    // main`, the specs table never gets the seat.
    let mut suggest = adopted();
    let mut ring = VecDeque::new();
    ring.push_back("git checkout main".to_string());
    suggest.ring = ring;
    let shown = step_typed("git che", &mut suggest).expect("someone answers");
    assert_eq!(shown.suffix, "ckout main", "the tab's own truth first");
    assert_eq!(shown.source, SuggestionSource::History);
}

#[test]
fn subcommand_seats_follow_the_walked_tree() {
    // The corpus era (suggest-embed): the walked tree answers at
    // every depth it seats — `git stash dr` finishes to stash's own
    // nested `drop` — while a word seat a leaf does not seat stays
    // silent (checkout takes a ref argument, not subcommands).
    let mut suggest = specs_suggest();
    let nested =
        step_typed("git stash dr", &mut suggest).expect("stash's own subcommand answers its depth");
    assert_eq!(nested.suffix, "op");
    assert_eq!(nested.source, SuggestionSource::Specs);
    let mut suggest = specs_suggest();
    assert_eq!(
        step_typed("git checkout che", &mut suggest),
        None,
        "a leaf's argument seat consults nothing it does not seat"
    );
}

#[test]
fn a_bare_trailing_space_is_no_subcommand_word_at_all() {
    // The catalog's empty-word rule, one policy across providers:
    // `git ` with the space echoed carries no word to match.
    let mut suggest = specs_suggest();
    assert_eq!(step_typed("git ", &mut suggest), None);
}

#[test]
fn flags_answer_at_the_node_that_seats_them() {
    // The corpus era (suggest-embed): the walked tree seats each
    // flag where upstream seats it — `git commit --am` finishes to
    // commit's own `--amend`, and git's root flags are not in that
    // list — while the root's own flags answer at the root
    // (`git --ba` → `--bare`), the hyphens riding verbatim through
    // the escape rule's whitelist.
    let mut suggest = specs_suggest();
    let own =
        step_typed("git commit --am", &mut suggest).expect("commit's own flag answers its node");
    assert_eq!(own.suffix, "end");
    assert_eq!(own.source, SuggestionSource::Specs);
    let mut suggest = specs_suggest();
    let root =
        step_typed("git --ba", &mut suggest).expect("the root's own flag answers the root seat");
    assert_eq!(root.suffix, "re");
}

#[test]
fn flag_and_subcommand_matches_follow_the_ruled_policy() {
    // The same policy over the corpus's own names: `docker pu` lands
    // on the alphabetically-first of the equal-length pull and push
    // (shortest wins, ties alphabetical), and a fully typed
    // subcommand has nothing left to add (the match must extend).
    let mut suggest = specs_suggest();
    let tied = step_typed("docker pu", &mut suggest).expect("a tie still answers");
    assert_eq!(tied.suffix, "ll");
    assert_eq!(tied.source, SuggestionSource::Specs);
    let mut suggest = specs_suggest();
    assert_eq!(
        step_typed("git checkout", &mut suggest),
        None,
        "the exactly-typed subcommand suggests nothing"
    );
}

#[test]
fn a_bare_double_dash_is_silence_at_the_flag_seat() {
    // The no-information floor (suggest-accuracy spec): a flag-seat
    // token of exactly `--` matches every long flag in the table
    // equally — the shortest-wins tiebreak picks one with no
    // evidence. One character past the dashes discriminates, and
    // stays ruled behavior.
    let mut suggest = specs_suggest();
    assert_eq!(
        step_typed("git --", &mut suggest),
        None,
        "a bare `--` carries no discrimination among flags"
    );
    let mut suggest = specs_suggest();
    let shown = step_typed("git --b", &mut suggest).expect("one character past the dashes answers");
    assert_eq!(shown.suffix, "are");
    assert_eq!(shown.source, SuggestionSource::Specs);
}

#[test]
fn the_subcommand_seat_stays_strong_at_one_character() {
    // Ruled unchanged (suggest-accuracy spec): the subcommand seat is
    // curated and small — strong even at one character — so the
    // floors never touch it. `git a` → `add` is the spec's pinned
    // example.
    let mut suggest = specs_suggest();
    let shown = step_typed("git a", &mut suggest).expect("the curated subcommand seat answers");
    assert_eq!(shown.suffix, "dd");
    assert_eq!(shown.source, SuggestionSource::Specs);
}

#[test]
fn the_core_tiers_commands_subcommands_answer_their_seats() {
    // One behavioral sample per old-five command over the embedded
    // floor (suggest-embed): the corpus's own trees answer the seats
    // — docker's `ps` is the shortest `p` word, npm's `it` the
    // shortest `i`, systemctl's `stop` the shortest `st` — the same
    // policy over the wider pool.
    let mut suggest = specs_suggest();
    let shortest = step_typed("docker p", &mut suggest).expect("docker's seat answers");
    assert_eq!(shortest.suffix, "s");
    let mut suggest = specs_suggest();
    let stopped = step_typed("systemctl st", &mut suggest).expect("systemctl's seat answers");
    assert_eq!(
        stopped.suffix, "op",
        "stop is the shortest of start/status/stop"
    );
    let mut suggest = specs_suggest();
    let installed = step_typed("npm i", &mut suggest).expect("npm's seat answers");
    assert_eq!(installed.suffix, "t");
}

#[test]
fn a_command_outside_the_table_stays_silent() {
    // The provider is silent, never a guess: zzz is not in the
    // embedded floor, so its argument seats answer nothing (python3
    // IS seated now — the corpus era's generosity).
    let mut suggest = specs_suggest();
    assert_eq!(step_typed("zzz wha", &mut suggest), None);
}

// --- the spec walker (suggest-walker; engine-rea §7 end to end) ---

/// The walker fixture's table, parsed once per run (the shared-fixture
/// rule): the §7 shapes as one table, seated beside the built-in five
/// the way a user's file seats them.
static WALKER_TABLE: LazyLock<Vec<SpecCommand>> = LazyLock::new(|| {
    parse_completions(include_str!("fixtures/walker.toml")).expect("the walker fixture parses")
});

/// The schema fixture's docker tree, parsed once — the nesting and
/// persistence shapes its file seats at every depth.
static NESTED_TABLE: LazyLock<Vec<SpecCommand>> =
    LazyLock::new(|| parse_completions(NESTED_FILE).expect("the nested fixture parses"));

/// One anchored step with `typed` after the "$ " anchor against a
/// walker fixture table — the walker's end-to-end ghost shape.
fn step_typed_table(typed: &str, table: &[SpecCommand], suggest: &mut Suggest) -> Option<Showing> {
    let mut before = String::from("$ ");
    before.push_str(typed);
    let observation = Observation {
        before_cursor: &before,
        cursor_column: before.len(),
        specs_table: table,
        ..obs()
    };
    step(&observation, suggest)
}

/// The menu's whole collection against a walker fixture table — the
/// walker's end-to-end list shape.
fn menu_table(typed: &str, table: &[SpecCommand]) -> Vec<MenuCandidate> {
    let suggest = specs_suggest();
    let observation = Observation {
        specs_table: table,
        ..obs()
    };
    candidates(typed, &suggest, &observation)
}

/// One collection's labels, in display order.
fn menu_labels(rows: &[MenuCandidate]) -> Vec<&str> {
    rows.iter().map(|row| row.label.as_str()).collect()
}

#[test]
fn a_subcommands_own_flags_complete_at_its_seat() {
    // §7.1: `git commit --am` finishes to commit's own `--amend` —
    // not git's root table. A persistent root flag rides into the
    // child; a plain one stays at the root.
    let table = &*WALKER_TABLE;
    let mut suggest = specs_suggest();
    let shown = step_typed_table("git commit --am", table, &mut suggest)
        .expect("commit's own flag answers");
    assert_eq!(shown.suffix, "end");
    assert_eq!(shown.source, SuggestionSource::Specs);
    assert_eq!(
        menu_labels(&menu_table("git commit --", table)),
        ["--amend", "--no-edit", "--no-pager"],
        "the persistent --no-pager rides; --version does not"
    );
    assert_eq!(
        menu_labels(&menu_table("git --", table)),
        ["--no-pager", "--version"]
    );
}

#[test]
fn a_flags_argument_takes_the_value_seat() {
    // §7.2: after `--emit` — `=` or space — the next seat is the
    // option's argument: its literal suggestions, and an accept
    // writes the bare value (never an `=`). A flag whose argument
    // carries no suggestions is silence (`mkdir -m `), and the
    // depleted mode seat hands `mkdir -m 755 ` to the Path provider.
    let table = &*WALKER_TABLE;
    let mut suggest = specs_suggest();
    let shown = step_typed_table("rustc --emit=a", table, &mut suggest)
        .expect("the value seat answers from the suggestions");
    assert_eq!(shown.suffix, "sm");
    assert_eq!(shown.source, SuggestionSource::Specs);
    assert_eq!(
        menu_labels(&menu_table("rustc --emit=", table)),
        ["asm", "link", "metadata", "mir"],
        "the whole vocabulary lists at the empty value partial"
    );
    let rows = menu_table("rustc --emit=a", table);
    assert_eq!(rows[0].suffix, "sm", "the suffix is the bare remainder");
    let mut suggest = specs_suggest();
    assert_eq!(
        step_typed_table("mkdir -m ", table, &mut suggest),
        None,
        "the mode seat has nothing to say"
    );
    assert!(
        menu_table("mkdir -m ", table).is_empty(),
        "no flags list at a value seat"
    );
    assert!(
        menu_table("mkdir -m 755 ", table).is_empty(),
        "the folder seat is the Path provider's to answer"
    );
}

#[test]
fn nested_subcommands_complete_to_any_depth() {
    // §7.3: `docker compose u` finishes to `up`, `gh pr cre` to
    // `create` — the menu's rows carrying the nested entries'
    // descriptions through.
    let table = &*WALKER_TABLE;
    let mut suggest = specs_suggest();
    let shown = step_typed_table("docker compose u", &NESTED_TABLE, &mut suggest)
        .expect("the nested subcommand answers");
    assert_eq!(shown.suffix, "p");
    let rows = menu_table("docker compose u", &NESTED_TABLE);
    assert_eq!(
        rows[0].description.as_deref(),
        Some("Create and start containers")
    );
    let mut suggest = specs_suggest();
    let shown =
        step_typed_table("gh pr cre", table, &mut suggest).expect("the second level answers");
    assert_eq!(shown.suffix, "ate");
}

#[test]
fn persistent_flags_ride_into_children() {
    // §7.4: `git --no-pager log --one` still finds `--oneline`
    // because `--no-pager` rode through the descent.
    let mut suggest = specs_suggest();
    let shown = step_typed_table("git --no-pager log --one", &WALKER_TABLE, &mut suggest)
        .expect("the persistent flag rode through");
    assert_eq!(shown.suffix, "line");
}

#[test]
fn variadic_args_keep_the_path_seat_until_a_flag_appears() {
    // §7.5: `git add src/ --` answers add's own flags (the persistent
    // root flag riding with them); the variadic path seat between
    // them answers no spec rows — the Path provider's seat alone.
    let table = &*WALKER_TABLE;
    assert_eq!(
        menu_labels(&menu_table("git add src/ --", table)),
        ["--force", "--no-pager", "--patch"]
    );
    assert!(
        menu_table("git add a b src/", table).is_empty(),
        "the path seat is not the specs' to answer"
    );
}

#[test]
fn a_command_arg_restarts_the_walk_on_its_own_entry() {
    // §7.6: the sudo shape — `sudo apt ins` walks apt's own entry.
    let mut suggest = specs_suggest();
    let shown = step_typed_table("sudo apt ins", &WALKER_TABLE, &mut suggest)
        .expect("the restarted walk answers");
    assert_eq!(shown.suffix, "tall");
    assert_eq!(shown.source, SuggestionSource::Specs);
}

#[test]
fn an_unknown_flag_mid_line_is_skipped_as_a_bare_flag() {
    // The locked tolerance: `git --whatever st` keeps answering the
    // subcommand seat — the flat seats never inspected intermediate
    // tokens, and the walker keeps that. Over the corpus floor the
    // `st` words are stage, stash, and status: the shortest tie
    // resolves alphabetically to `stage`.
    let mut suggest = specs_suggest();
    let shown = step_typed_table("git --whatever st", builtin_specs(), &mut suggest)
        .expect("the tolerance holds");
    assert_eq!(shown.suffix, "age");
    let suggest = specs_suggest();
    let rows = candidates("git --whatever st", &suggest, &obs());
    assert_eq!(menu_labels(&rows), ["stage", "stash", "status"]);
}

// --- the alias map (suggest-aliases) ---

/// One anchored step with `typed` after the "$ " anchor and a
/// declared alias map — the alias tier's end-to-end shape (the
/// specs table re-targeted, every other seat as `step_typed`
/// builds it).
fn step_typed_aliased(
    typed: &str,
    aliases: &HashMap<String, String>,
    suggest: &mut Suggest,
) -> Option<Showing> {
    let mut before = String::from("$ ");
    before.push_str(typed);
    let observation = Observation {
        before_cursor: &before,
        cursor_column: before.len(),
        aliases,
        ..obs()
    };
    step(&observation, suggest)
}

#[test]
fn a_mapped_alias_ghosts_its_expansion_subcommand() {
    // The flagship (the record's ruling): `gc = "git checkout"`
    // declared, `gc che` ghosts `ckout` — the specs table re-targeted
    // at git, the token keeping its own typed seat, the same ghost
    // and provenance `git che` gives.
    let aliases = HashMap::from([("gc".to_string(), "git checkout".to_string())]);
    let mut suggest = specs_suggest();
    let shown = step_typed_aliased("gc che", &aliases, &mut suggest)
        .expect("the mapped alias re-targets the specs table");
    assert_eq!(shown.suffix, "ckout");
    assert_eq!(shown.source, SuggestionSource::Specs);
}

#[test]
fn a_mapped_alias_ghosts_its_expansion_flags() {
    // The record's other flagship, corpus era: `gc --ba` ghosts
    // git's root `--bare` — the flag seat consults the expansion
    // command's own flag list, as for the typed command.
    let aliases = HashMap::from([("gc".to_string(), "git checkout".to_string())]);
    let mut suggest = specs_suggest();
    let shown = step_typed_aliased("gc --ba", &aliases, &mut suggest)
        .expect("the flag seat rides the expansion command");
    assert_eq!(shown.suffix, "re");
    assert_eq!(shown.source, SuggestionSource::Specs);
}

#[test]
fn an_unmapped_alias_behaves_exactly_as_today() {
    // `gc che` with no `gc` declared: the table never consults a
    // guess — an unknown command is silence, the shipped rule, and
    // the empty default is this case for every word.
    let mut suggest = specs_suggest();
    assert_eq!(step_typed("gc che", &mut suggest), None);
}

#[test]
fn resolve_alias_returns_the_input_unmapped() {
    let aliases = HashMap::from([("gc".to_string(), "git checkout".to_string())]);
    assert_eq!(resolve_alias("git", &aliases), "git");
    assert_eq!(resolve_alias("vim", &aliases), "vim");
}

#[test]
fn resolve_alias_follows_a_chain_to_its_command() {
    // `a → gc → git checkout`: two hops, each expansion's first
    // word — the chain resolves to the command the user's
    // declaration ultimately names.
    let aliases = HashMap::from([
        ("a".to_string(), "gc".to_string()),
        ("gc".to_string(), "git checkout".to_string()),
    ]);
    assert_eq!(resolve_alias("a", &aliases), "git");
    assert_eq!(resolve_alias("gc", &aliases), "git");
}

#[test]
fn resolve_alias_terminates_a_cycle_unmapped() {
    // `a → b → a`: the walk stops at the word it would re-enter —
    // the input, as unmapped as if nothing were declared.
    let aliases = HashMap::from([
        ("a".to_string(), "b".to_string()),
        ("b".to_string(), "a".to_string()),
    ]);
    assert_eq!(resolve_alias("a", &aliases), "a");
}

#[test]
fn resolve_alias_stops_at_three_expansions() {
    // Depth 4 stops at 3 (OQ-2, locked): `a1→a2→a3→a4→git` resolves
    // a1 to a4, never following the fourth expansion to git.
    let aliases = HashMap::from([
        ("a1".to_string(), "a2".to_string()),
        ("a2".to_string(), "a3".to_string()),
        ("a3".to_string(), "a4".to_string()),
        ("a4".to_string(), "git".to_string()),
    ]);
    assert_eq!(resolve_alias("a1", &aliases), "a4");
}

#[test]
fn history_outranks_a_mapped_alias() {
    // The shipped rule verbatim: after one real `gc cheese`, the
    // tab's own truth answers before the re-targeted table ever
    // gets the seat.
    let aliases = HashMap::from([("gc".to_string(), "git checkout".to_string())]);
    let mut suggest = adopted();
    let mut ring = VecDeque::new();
    ring.push_back("gc cheese".to_string());
    suggest.ring = ring;
    let shown = step_typed_aliased("gc che", &aliases, &mut suggest).expect("someone answers");
    assert_eq!(shown.suffix, "ese", "the tab's own truth first");
    assert_eq!(shown.source, SuggestionSource::History);
}

#[test]
fn the_ssh_gate_silences_the_alias_tier() {
    // The map is local-machine truth the user declared for their
    // local shell: on a tab spawned as ssh it stays silent, riding
    // the same `paths_eligible` gate paths/catalog/specs already
    // ride.
    let aliases = HashMap::from([("gc".to_string(), "git checkout".to_string())]);
    let before = String::from("$ gc che");
    let observation = Observation {
        paths_eligible: false,
        before_cursor: &before,
        cursor_column: before.len(),
        aliases: &aliases,
        ..obs()
    };
    let mut suggest = specs_suggest();
    assert_eq!(step(&observation, &mut suggest), None);
}

// --- the generator provider (suggest-generators) ---

/// One anchored step with `typed` after the "$ " anchor and injected
/// generator caches — the provider's end-to-end shape, the sets
/// arriving exactly as the engine-side runner will cache them:
/// post-processed, already-escaped names, in the runner's own
/// (recency) order.
fn step_generator(
    typed: &str,
    branches: &[&str],
    tags: &[&str],
    files: &[&str],
    suggest: &mut Suggest,
) -> Option<Showing> {
    let branches: Vec<String> = branches.iter().map(|s| (*s).to_string()).collect();
    let tags: Vec<String> = tags.iter().map(|s| (*s).to_string()).collect();
    let files: Vec<String> = files.iter().map(|s| (*s).to_string()).collect();
    let sets = GeneratorSets {
        branches: &branches,
        tags: &tags,
        files: &files,
    };
    let mut before = String::from("$ ");
    before.push_str(typed);
    let observation = Observation {
        before_cursor: &before,
        cursor_column: before.len(),
        generator_candidates: Some(sets),
        ..obs()
    };
    step(&observation, suggest)
}

#[test]
fn the_generator_ghosts_a_cached_branch_at_the_checkout_seat() {
    // The spec's flagship: `git checkout ma` ghosts `in` from a
    // cached `main` — the fifth source surfacing through the same
    // seam, provenance carried.
    let mut suggest = adopted();
    let shown = step_generator(
        "git checkout ma",
        &["feature/x", "main"],
        &[],
        &[],
        &mut suggest,
    )
    .expect("the cached branch answers its prefix");
    assert_eq!(shown.suffix, "in");
    assert_eq!(shown.source, SuggestionSource::Generator);
}

#[test]
fn history_outranks_the_generator() {
    // The merge's ruling holds for the fifth source: the tab's own
    // ring answers first, the cached sets only when history cannot.
    let mut suggest = adopted();
    let mut ring = VecDeque::new();
    ring.push_back("git checkout maintain".to_string());
    suggest.ring = ring;
    let shown = step_generator("git checkout ma", &["main"], &[], &[], &mut suggest)
        .expect("someone answers the prefix");
    assert_eq!(shown.suffix, "intain");
    assert_eq!(shown.source, SuggestionSource::History);
}

#[test]
fn the_generator_stays_silent_without_a_cache() {
    // The engine stopgap (Task 1): no runner exists yet, the
    // observation carries no sets, and today's behavior holds
    // bit-for-bit — silence, never a guess.
    let mut suggest = adopted();
    let before = String::from("$ git checkout ma");
    let observation = Observation {
        before_cursor: &before,
        cursor_column: before.len(),
        generator_candidates: None,
        ..obs()
    };
    assert_eq!(step(&observation, &mut suggest), None);
}

#[test]
fn the_generator_rides_the_shell_gate() {
    // The standing ssh gate: cached or not, a tab spawned as ssh (or
    // any non-shell program) never serves generator candidates — the
    // readable cwd is the wrong machine's.
    let mut suggest = adopted();
    let branches: Vec<String> = vec!["main".to_string()];
    let sets = GeneratorSets {
        branches: &branches,
        tags: &[],
        files: &[],
    };
    let before = String::from("$ git checkout ma");
    let observation = Observation {
        paths_eligible: false,
        before_cursor: &before,
        cursor_column: before.len(),
        generator_candidates: Some(sets),
        ..obs()
    };
    assert_eq!(step(&observation, &mut suggest), None);
}

#[test]
fn switch_consults_branches_only() {
    // `git switch` takes a local branch, never a tag — the seat
    // picks its set, and a tag the token would match stays silent.
    let mut suggest = adopted();
    assert_eq!(
        step_generator("git switch v", &["main"], &["v2"], &[], &mut suggest),
        None,
        "a tag never answers the switch seat"
    );
    let shown = step_generator("git switch ma", &["main"], &["v2"], &[], &mut suggest)
        .expect("the branch answers its own seat");
    assert_eq!(shown.suffix, "in");
}

#[test]
fn checkout_consults_branches_then_tags() {
    // `git checkout` takes either a branch or a tag: branches first
    // (the common case), tags after, and an equal-length tie falls to
    // the first name in that order — the newest branch, then the
    // newest tag.
    let mut suggest = adopted();
    let shown = step_generator(
        "git checkout v1.",
        &["main"],
        &["v1.1", "v1.0"],
        &[],
        &mut suggest,
    )
    .expect("the tag set answers when no branch does");
    assert_eq!(shown.suffix, "1", "the first tag of the tie wins");
}

#[test]
fn add_and_restore_consult_the_changed_files() {
    // The working-tree file seat: `status --short` names answer the
    // argument token of the commands that take changed files.
    let mut suggest = adopted();
    let shown = step_generator(
        "git add src/ma",
        &["main"],
        &[],
        &["src/main.rs", "readme"],
        &mut suggest,
    )
    .expect("a changed file answers its prefix");
    assert_eq!(shown.suffix, "in.rs");
    assert_eq!(shown.source, SuggestionSource::Generator);
    let shown = step_generator(
        "git restore rea",
        &["main"],
        &[],
        &["src/main.rs", "readme"],
        &mut suggest,
    )
    .expect("restore takes changed files too");
    assert_eq!(shown.suffix, "dme");
}

#[test]
fn command_position_and_later_words_never_consult_the_generator() {
    // The seat is exactly the first argument token after the git
    // subcommand: the command word itself is the catalog's seat, and
    // a third completed word (`git checkout -b <name>`) is naming
    // something new, not completing a cached ref. An empty ring
    // keeps every other provider out of the assertion's way.
    let mut suggest = adopted();
    suggest.ring = VecDeque::new();
    assert_eq!(
        step_generator("gi", &["main"], &[], &[], &mut suggest),
        None,
        "the command word is not the generator's seat"
    );
    assert_eq!(
        step_generator("git checkout -b ma", &["main"], &[], &[], &mut suggest),
        None,
        "a later word never consults the ref sets"
    );
}

#[test]
fn an_unknown_subcommand_or_empty_token_is_silence() {
    // Only the ruled seats map to sets (`checkout`, `switch`, `add`,
    // `restore`); a bare trailing space is no token at all — the
    // specs provider's own empty-word rule, one policy across
    // providers.
    let mut suggest = adopted();
    assert_eq!(
        step_generator("git commit ma", &["main"], &[], &[], &mut suggest),
        None,
        "an unmapped subcommand consults nothing"
    );
    assert_eq!(
        step_generator("git checkout ", &["main"], &[], &[], &mut suggest),
        None,
        "an empty token matches every name equally — the floor"
    );
}

#[test]
fn a_non_git_command_never_consults_the_generator() {
    // The argv set is git-only (the record's first slice); every
    // other command's arguments are another provider's or nobody's.
    // `zzz bu` is a shape no other provider answers, so any answer
    // here could only be the generator's.
    let mut suggest = adopted();
    suggest.ring = VecDeque::new();
    assert_eq!(
        step_generator("zzz bu", &["main"], &[], &["build.rs"], &mut suggest),
        None,
        "the generator has no program beyond git"
    );
}

#[test]
fn an_aliased_git_re_targets_the_generator_seat() {
    // Re-target, never rewrite (the aliases ruling): a declared
    // `gc = "git checkout"` makes `gc checkout ma` a generator seat,
    // while `gc ma` alone keeps the specs subcommand seat the map
    // already rules.
    let aliases = HashMap::from([("gc".to_string(), "git checkout".to_string())]);
    let run = |typed: &str, suggest: &mut Suggest| {
        let branches: Vec<String> = vec!["main".to_string()];
        let sets = GeneratorSets {
            branches: &branches,
            tags: &[],
            files: &[],
        };
        let mut before = String::from("$ ");
        before.push_str(typed);
        let observation = Observation {
            before_cursor: &before,
            cursor_column: before.len(),
            aliases: &aliases,
            generator_candidates: Some(sets),
            ..obs()
        };
        step(&observation, suggest)
    };
    let mut suggest = adopted();
    let shown = run("gc checkout ma", &mut suggest)
        .expect("the alias's expansion command re-targets the seat");
    assert_eq!(shown.suffix, "in");
    assert_eq!(shown.source, SuggestionSource::Generator);
    assert_eq!(
        run("gc ma", &mut suggest),
        None,
        "the alias alone is the specs seat, not a ref seat"
    );
}

#[test]
fn escaped_generator_names_carve_their_typed_suffix() {
    // The runner caches already-escaped names (the record's
    // post-filter): the typed token prefix-matches the escaped form
    // itself — the user typed their own backslash — and the carved
    // remainder is exactly the completion of what they typed. A
    // quote-tier name rides whole: `'c` completes inside the quotes.
    let mut suggest = adopted();
    let shown = step_generator(
        "git checkout my\\ b",
        &["my\\ branch"],
        &[],
        &[],
        &mut suggest,
    )
    .expect("the escaped form matches its typed prefix");
    assert_eq!(shown.suffix, "ranch");
    let shown = step_generator("git checkout 'c", &["'café'"], &[], &[], &mut suggest)
        .expect("the quote tier rides whole");
    assert_eq!(shown.suffix, "afé'");
}

#[test]
fn the_menu_lists_the_whole_generator_set_in_set_order() {
    // The menu's whole-set rule: every cached name matching the
    // token, in the runner's own (recency) order — no shortest-wins
    // tiebreak for the ghost to impose on a list.
    let suggest = adopted();
    let branches: Vec<String> = vec!["feature/x".to_string()];
    let tags: Vec<String> = vec!["v1".to_string()];
    let files: Vec<String> = vec!["readme".to_string()];
    let sets = GeneratorSets {
        branches: &branches,
        tags: &tags,
        files: &files,
    };
    let observation = Observation {
        before_cursor: "$ git checkout f",
        cursor_column: 16,
        generator_candidates: Some(sets),
        ..obs()
    };
    let menu = candidates("git checkout f", &suggest, &observation);
    assert_eq!(
        menu.iter()
            .map(|row| (row.label.as_str(), row.suffix.as_str(), row.source))
            .collect::<Vec<_>>(),
        vec![("feature/x", "eature/x", SuggestionSource::Generator)],
    );
    assert!(menu[0].description.is_none());
}

#[test]
fn checkout_menu_rows_dedupe_across_branches_and_tags() {
    // refs/heads/release and refs/tags/release share a short name:
    // the checkout seat concatenates the sets, and the same escaped
    // name never lists twice.
    let suggest = adopted();
    let branches: Vec<String> = vec!["release".to_string()];
    let tags: Vec<String> = vec!["release".to_string(), "v1".to_string()];
    let sets = GeneratorSets {
        branches: &branches,
        tags: &tags,
        files: &[],
    };
    let observation = Observation {
        before_cursor: "$ git checkout re",
        cursor_column: 17,
        generator_candidates: Some(sets),
        ..obs()
    };
    let menu = candidates("git checkout re", &suggest, &observation);
    assert_eq!(
        menu.iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        vec!["release"],
    );
}

#[test]
fn generator_rows_lead_history_in_the_menu() {
    // The collection's source order is the display order, amended
    // 2026-11-06: the seat's scope leads, so the generator's cached
    // branch — git's own names, the keyword's context — rides above
    // the history line, where the single-choice ghost keeps its
    // locked history-first merge.
    let mut suggest = adopted();
    let mut ring = VecDeque::new();
    ring.push_back("git checkout main2".to_string());
    suggest.ring = ring;
    let branches: Vec<String> = vec!["main".to_string()];
    let sets = GeneratorSets {
        branches: &branches,
        tags: &[],
        files: &[],
    };
    let observation = Observation {
        before_cursor: "$ git checkout ma",
        cursor_column: 17,
        generator_candidates: Some(sets),
        ..obs()
    };
    let menu = candidates("git checkout ma", &suggest, &observation);
    assert_eq!(
        menu.iter()
            .map(|row| (row.label.as_str(), row.source))
            .collect::<Vec<_>>(),
        vec![
            ("main", SuggestionSource::Generator),
            ("git checkout main2", SuggestionSource::History),
        ],
    );
}

// --- the generator runner's post-filter (suggest-generators, Task 2) ---
// The fixtures below are pinned from real git output shapes: a
// `for-each-ref --format` line carries
// `shortname|objectname|subject`; `tag --list` prints bare names;
// `status --short` prints porcelain `XY path` lines with rename
// arrows. VS Code's discipline, the record's pinning: `warning:`/
// `error:` lines drop, `fatal:` output yields the empty set, names
// dedupe, and every name rides `escape_for_shell`.

#[test]
fn generator_argv_pins_the_git_set() {
    // The contract as data (the record's verbatim VS Code set): the
    // three DropTerm-owned argv arrays — never config, never a data
    // file, never interpolated. `--sort=-committerdate` is the
    // record's own spelling (the spec's `commercedate` is its typo —
    // git rejects unknown sort atoms with a `fatal:` line the
    // post-filter would rightly silence into).
    use crate::terminal::engine::{GIT_BRANCHES, GIT_FILES, GIT_TAGS};
    assert_eq!(
        GIT_BRANCHES,
        &[
            "git",
            "--no-optional-locks",
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)|%(objectname:short)|%(subject)",
            "refs/heads/",
        ]
    );
    assert_eq!(
        GIT_TAGS,
        &[
            "git",
            "--no-optional-locks",
            "tag",
            "--list",
            "--sort=-committerdate",
        ]
    );
    assert_eq!(
        GIT_FILES,
        &["git", "--no-optional-locks", "status", "--short"]
    );
}

#[test]
fn the_post_filter_carves_short_ref_names() {
    // Real for-each-ref output: one ref per line, the template's
    // first `|`-field the short name.
    use crate::terminal::engine::{branch_name, post_filter_names};
    let output = "main|4ab2f6f|Merge pull request #12\n\
                  feature/x|9df217c|the runner lands\n";
    assert_eq!(
        post_filter_names(output, branch_name),
        vec!["main", "feature/x"]
    );
}

#[test]
fn warning_and_error_lines_drop_from_generator_output() {
    // Reality noise (an advice line before the refs, an fsck error):
    // dropped, the real names kept.
    use crate::terminal::engine::{branch_name, post_filter_names};
    let output = "warning: unable to access '...'\n\
                  error: bad signature\n\
                  main|abc1234|subject\n";
    assert_eq!(post_filter_names(output, branch_name), vec!["main"]);
}

#[test]
fn fatal_generator_output_yields_the_empty_set() {
    // A non-repo directory, a missing objectdb: the command itself
    // failed — silence, never a half-parsed remainder.
    use crate::terminal::engine::{branch_name, post_filter_names};
    let output = "fatal: not a git repository (or any of the parent directories): .git\n";
    assert!(post_filter_names(output, branch_name).is_empty());
}

#[test]
fn duplicate_generator_names_dedupe() {
    // The record's `seen` Set: the same name twice lists once.
    use crate::terminal::engine::{branch_name, post_filter_names};
    let output = "main|abc1234|one\nmain|def5678|two\n";
    assert_eq!(post_filter_names(output, branch_name), vec!["main"]);
}

#[test]
fn tag_lines_are_whole_names() {
    use crate::terminal::engine::{post_filter_names, tag_name};
    let output = "v1.1\nv1.0\n";
    assert_eq!(post_filter_names(output, tag_name), vec!["v1.1", "v1.0"]);
}

#[test]
fn status_lines_carve_the_changed_path() {
    // Porcelain v1 short: `XY path`, a rename's new path after the
    // arrow, untracked `??` — and a carved path with a space rides
    // the escape tier like every name.
    use crate::terminal::engine::{post_filter_names, status_path};
    let output = " M src/main.rs\n?? notes.md\nR  old name -> new name\nA  staged.txt\n";
    assert_eq!(
        post_filter_names(output, status_path),
        vec!["src/main.rs", "notes.md", "new\\ name", "staged.txt"]
    );
}

#[test]
fn quoted_status_paths_drop() {
    // Git C-quotes paths carrying non-ASCII or quotes
    // (core.quotePath's default): the quoted form is not the name,
    // and unquoting git's escapes is a parser this contract refuses
    // to ship — the line drops alone, never poisons the set.
    use crate::terminal::engine::{post_filter_names, status_path};
    let output = "?? \"src/caf\\303\\251.rs\"\n M plain.txt\n";
    assert_eq!(post_filter_names(output, status_path), vec!["plain.txt"]);
}

#[test]
fn generator_names_ride_the_escape_tier() {
    // The shared quoting tier, pinned: a space backslash-escapes, a
    // plain quote backslash-escapes (the first tier covers it), a
    // non-ASCII name rides whole in single quotes — and a name that
    // needs the quote tier while carrying a quote drops, the tier's
    // own never-a-wrong-suffix rule.
    use crate::terminal::engine::{branch_name, post_filter_names};
    let output = concat!(
        "my branch|abc|subject\n",
        "it's|ghi|subject\n",
        "café|def|subject\n",
        "caf'é|xyz|subject\n",
    );
    assert_eq!(
        post_filter_names(output, branch_name),
        vec!["my\\ branch", "it\\'s", "'café'"]
    );
}

// --- the common-prefix helper ---

#[test]
fn the_shared_prefix_cuts_on_a_char_boundary() {
    // "café" and "cafe" share only "caf": the é's lead byte never
    // survives the cut.
    assert_eq!(shared_prefix("café", "cafe"), "caf");
    assert_eq!(shared_prefix("src", "src-cache"), "src");
    assert_eq!(shared_prefix("src", "srv"), "sr");
    assert_eq!(shared_prefix("alpha", "beta"), "");
}

// --- the history seed (history-seed) ---

#[test]
fn only_the_three_persistent_shells_map_a_history_file() {
    // The spawn truth arrives basename-shaped: the three shells
    // with a standard persistent file seed; the rest — the
    // allowlist's sh family included, plus non-shells — stay
    // scrape-only.
    assert_eq!(
        shell_history_file("/bin/bash").as_deref(),
        Some(std::path::Path::new(".bash_history"))
    );
    assert_eq!(
        shell_history_file("zsh").as_deref(),
        Some(std::path::Path::new(".zsh_history"))
    );
    assert_eq!(
        shell_history_file("fish").as_deref(),
        Some(std::path::Path::new(".local/share/fish/fish_history"))
    );
    for program in ["sh", "dash", "ksh", "csh", "tcsh", "ssh", "python3", ""] {
        assert_eq!(
            shell_history_file(program),
            None,
            "{program} has no default history file to read"
        );
    }
}

#[test]
fn bash_history_parses_plain_lines_skipping_hash_led_ones() {
    // The `#`-led lines are HISTTIMEFORMAT timestamps (or
    // comments): never commands. Blank lines never seed.
    assert_eq!(
        parse_shell_history("bash", "git status\n#1690000000\necho hi\n\n   \nls -la\n"),
        ["git status", "echo hi", "ls -la"]
    );
}

#[test]
fn zsh_history_strips_the_extended_metadata_prefix_and_passes_plain_lines() {
    // Extended history writes `: <start>:<elapsed>;command`; a
    // plain-format file has bare lines, and a pathological
    // `: `-led command without the exact meta shape passes whole.
    assert_eq!(
        parse_shell_history(
            "zsh",
            ": 1690000000:5;git push\nls -la\n: not meta;echo hi\n"
        ),
        ["git push", "ls -la", ": not meta;echo hi"]
    );
}

#[test]
fn fish_history_takes_cmd_records_and_drops_multiline_heads() {
    // A `- cmd:` record is one command; an indented continuation
    // marks a multiline command whose head would be half a
    // command, so the whole entry is dropped. `paths:` blocks
    // never match the prefix.
    assert_eq!(
        parse_shell_history(
            "fish",
            "- cmd: git status\n  paths:\n    - foo\n- cmd: ls\n- cmd: echo done\n"
        ),
        ["ls", "echo done"]
    );
}

#[test]
fn an_unknown_shell_parses_to_nothing() {
    assert!(parse_shell_history("nu", "git status").is_empty());
    assert!(parse_shell_history("", "git status").is_empty());
}

#[test]
fn the_parse_dedupes_keeping_the_most_recent_and_caps_the_tail() {
    // A re-run command sits once, at its newest position; past
    // the cap the oldest tail drops (file order is oldest first).
    let contents = "first\nsecond\nfirst\nthird\n";
    assert_eq!(
        parse_shell_history("bash", contents),
        ["second", "first", "third"]
    );
    let many = (0..=RING_CAP)
        .map(|i| format!("cmd-{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let seeded = parse_shell_history("bash", &many);
    assert_eq!(seeded.len(), RING_CAP);
    assert_eq!(seeded.first().map(String::as_str), Some("cmd-1"));
    assert_eq!(seeded.last().map(String::as_str), Some("cmd-256"));
}

#[test]
fn seed_history_commits_in_order_deduping_and_capping() {
    // Seeding is committing: oldest first lands newest at the
    // back, duplicates never double-seat, the cap holds.
    let mut suggest = Suggest::new();
    suggest.seed_history(["ls".to_string(), "git status".to_string(), "ls".to_string()]);
    assert_eq!(suggest.ring.len(), 2);
    assert_eq!(suggest.ring.front().map(String::as_str), Some("git status"));
    assert_eq!(suggest.ring.back().map(String::as_str), Some("ls"));
    let seeded: Vec<String> = (0..=RING_CAP).map(|i| format!("cmd-{i}")).collect();
    let mut suggest = Suggest::new();
    suggest.seed_history(seeded);
    assert_eq!(suggest.ring.len(), RING_CAP);
}

// --- the menu's full candidate collection (suggest-menu) ---

#[test]
fn a_two_match_path_token_lists_both_entries_where_the_ghost_yields_the_common_prefix() {
    // The menu's reason to exist (suggest-menu spec): at `~/s` over
    // src and srv the ghost must settle for the one shared prefix
    // `r`, but the menu carries both entries whole — each with the
    // exact escaped suffix an accept writes, and no description (no
    // source carries one yet; the specs-data file is the supplier).
    let entries = listing(&[("src", true), ("srv", true)]);
    let mut suggest = adopted();
    let ghost =
        step_path("~/s", Some(&entries), &mut suggest).expect("the ghost answers the field");
    assert_eq!(
        ghost.suffix, "r",
        "the single-choice ghost settles for the shared prefix"
    );
    let observation = Observation {
        path_listing: Some(&entries),
        ..obs()
    };
    let menu = candidates("~/s", &suggest, &observation);
    let labels: Vec<&str> = menu.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["src/", "srv/"], "the menu lists both entries");
    let suffixes: Vec<&str> = menu.iter().map(|c| c.suffix.as_str()).collect();
    assert_eq!(suffixes, ["rc/", "rv/"]);
    assert!(menu.iter().all(|c| c.source == SuggestionSource::Path));
    assert!(menu.iter().all(|c| c.description.is_none()));
}

#[test]
fn the_menu_ranks_the_seats_scope_above_history() {
    // The menu's own order (suggest-menu spec, amended 2026-11-06 by
    // the maintainer's scoping report): the keyword's scope leads —
    // the seat-scoped providers above history — where the
    // single-choice ghost keeps its locked history-first
    // `source_rank`. One token cannot seat every provider at once
    // (the seats are mutually exclusive), so the pin composes three
    // tokens that each seat two: `git st` (specs over history), `~/s`
    // (path over history), and `gi` (the command seat, where no
    // keyword has settled and history still leads the catalog).
    let suggest = adopted();
    let specs_over_history = candidates("git st", &suggest, &obs());
    let sources: Vec<SuggestionSource> = specs_over_history.iter().map(|c| c.source).collect();
    assert_eq!(
        sources,
        [
            SuggestionSource::Specs,
            SuggestionSource::Specs,
            SuggestionSource::Specs,
            SuggestionSource::History
        ]
    );
    let labels: Vec<&str> = specs_over_history
        .iter()
        .map(|c| c.label.as_str())
        .collect();
    assert_eq!(labels, ["stage", "stash", "status", "git status"]);
    assert!(
        specs_over_history
            .windows(2)
            .all(|pair| menu_source_rank(pair[0].source) <= menu_source_rank(pair[1].source)),
        "the whole list is non-decreasing in menu_source_rank"
    );

    let mut suggest = adopted();
    let mut ring = VecDeque::new();
    ring.push_back("~/src/project".to_string());
    suggest.ring = ring;
    let entries = listing(&[("src", true), ("srv", true)]);
    let observation = Observation {
        path_listing: Some(&entries),
        ..obs()
    };
    let path_over_history = candidates("~/s", &suggest, &observation);
    let sources: Vec<SuggestionSource> = path_over_history.iter().map(|c| c.source).collect();
    assert_eq!(
        sources,
        [
            SuggestionSource::Path,
            SuggestionSource::Path,
            SuggestionSource::History
        ]
    );
    let labels: Vec<&str> = path_over_history.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["src/", "srv/", "~/src/project"]);

    // The command seat is the scope-less one: no command word has
    // completed, so history stays the leading context and the
    // catalog follows (the amendment moves nothing here).
    let names = catalog(&["giftopnm", "git", "gitk"]);
    let suggest = adopted();
    let observation = Observation {
        command_catalog: &names,
        ..obs()
    };
    let history_then_catalog = candidates("gi", &suggest, &observation);
    let sources: Vec<SuggestionSource> = history_then_catalog.iter().map(|c| c.source).collect();
    assert_eq!(
        sources,
        [
            SuggestionSource::History,
            SuggestionSource::History,
            SuggestionSource::Catalog,
            SuggestionSource::Catalog,
            SuggestionSource::Catalog
        ]
    );
    let labels: Vec<&str> = history_then_catalog
        .iter()
        .map(|c| c.label.as_str())
        .collect();
    assert_eq!(
        labels,
        ["git status", "git push", "giftopnm", "git", "gitk"]
    );
}

#[test]
fn empty_typed_input_collects_no_menu_candidates() {
    // A bare prompt has no token to complete: the collection is
    // silence — even a ring that would prefix-match everything stays
    // quiet (the open path treats an empty set as ineligible).
    let suggest = adopted();
    assert!(candidates("", &suggest, &obs()).is_empty());
}

#[test]
fn the_menu_lists_every_history_prefix_match_most_recent_first() {
    // The ghost answers with the newest match alone; the menu carries
    // the whole set, most recent first — the ring walked backwards,
    // bounded by the ring's own cap. `echo h` seats history alone
    // (echo is outside the curated table, the word is past the
    // command seat), so the order pin reads without composition.
    let mut suggest = adopted();
    let mut ring = VecDeque::new();
    ring.push_back("echo hello".to_string());
    ring.push_back("echo hi".to_string());
    suggest.ring = ring;
    let menu = candidates("echo h", &suggest, &obs());
    let labels: Vec<&str> = menu.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["echo hi", "echo hello"], "most recent first");
    let suffixes: Vec<&str> = menu.iter().map(|c| c.suffix.as_str()).collect();
    assert_eq!(suffixes, ["i", "ello"]);
    assert!(menu.iter().all(|c| c.source == SuggestionSource::History));
}

#[test]
fn the_menu_lists_every_catalog_prefix_name() {
    // No shortest-wins at the menu (suggest-menu spec): the
    // tiebreak exists to pick one of many for the single-choice
    // ghost, and a list needs no tiebreak — `gi` carries giftopnm,
    // git, and gitk whole, in the snapshot's sorted order.
    let names = catalog(&["giftopnm", "git", "gitk"]);
    let suggest = catalog_suggest();
    let observation = Observation {
        command_catalog: &names,
        ..obs()
    };
    let menu = candidates("gi", &suggest, &observation);
    let labels: Vec<&str> = menu.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["giftopnm", "git", "gitk"]);
    let suffixes: Vec<&str> = menu.iter().map(|c| c.suffix.as_str()).collect();
    assert_eq!(suffixes, ["ftopnm", "t", "tk"]);
    assert!(menu.iter().all(|c| c.source == SuggestionSource::Catalog));
}

#[test]
fn the_menu_lists_every_spec_entry_for_the_seat() {
    // The specs table's whole strict-prefix set at each seat — and
    // the bare `--` the single-choice ghost floors to silence lists
    // the flag table whole (a list is information, not a guess; the
    // floor is the ghost's alone). Over the embedded floor the
    // expected lists are read from it, never hand-written.
    let suggest = specs_suggest();
    let subcommands = candidates("git st", &suggest, &obs());
    let labels: Vec<&str> = subcommands.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["stage", "stash", "status"]);
    let suggest = specs_suggest();
    let flags = candidates("git --", &suggest, &obs());
    let git = builtin_specs()
        .iter()
        .find(|command| command.name == "git")
        .expect("git is seated in the floor");
    let long_flags: Vec<&str> = git
        .flags
        .iter()
        .filter(|flag| flag.name.starts_with("--"))
        .map(|flag| flag.name.as_str())
        .collect();
    let labels: Vec<&str> = flags.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels, long_flags,
        "the bare `--` lists the whole long-flag table"
    );
}

#[test]
fn the_menu_lists_the_whole_subcommand_table_at_a_bare_command_word() {
    // The ghost's empty-word floor is the ghost's alone (the bare
    // `--` rule's twin): `git ` with the space echoed and nothing
    // after lists git's whole subcommand table above the history
    // lines (the 2026-11-06 amendment: the keyword's scope leads) —
    // the scoped next level the command word settles the menu into,
    // where an unscoped provider or a later seat contributes
    // nothing. Over the embedded floor the expected lists are read
    // from it, never hand-written.
    let suggest = adopted();
    let menu = candidates("git ", &suggest, &obs());
    let git = builtin_specs()
        .iter()
        .find(|command| command.name == "git")
        .expect("git is seated in the floor");
    let subcommands: Vec<&str> = git
        .subcommands
        .iter()
        .map(|command| command.name.as_str())
        .collect();
    let labels: Vec<&str> = menu.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        &labels[..subcommands.len()],
        &subcommands[..],
        "the whole subcommand table leads, the table's own order"
    );
    assert_eq!(
        &labels[subcommands.len()..],
        ["git status", "git push"],
        "then the history lines, most recent first"
    );
    let suffixes: Vec<&str> = menu
        .iter()
        .take(subcommands.len())
        .map(|c| c.suffix.as_str())
        .collect();
    assert_eq!(
        suffixes, subcommands,
        "an empty token accepts the whole word"
    );
    assert!(
        menu.iter()
            .take(subcommands.len())
            .all(|c| c.source == SuggestionSource::Specs),
        "the table rows carry the specs provenance"
    );
    // The seat rules hold at the boundary: a later seat with an
    // empty token never consults the subcommand table — checkout's
    // empty word seat answers its own argument's literal
    // suggestions and nothing else — and an unseated command's bare
    // word collects nothing (the ring holds no match either —
    // specs_suggest's `vim notes`).
    let suggest = specs_suggest();
    let checkout = candidates("git checkout ", &suggest, &obs());
    let suggestions: Vec<&str> = git
        .subcommands
        .iter()
        .find(|command| command.name == "checkout")
        .and_then(|checkout| checkout.args.first())
        .map(|arg| {
            arg.suggestions
                .iter()
                .map(|entry| entry.name.as_str())
                .collect()
        })
        .unwrap_or_default();
    let labels: Vec<&str> = checkout.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels, suggestions,
        "the leaf's empty word seat answers its argument's suggestions alone"
    );
    assert!(
        candidates("echo ", &suggest, &obs()).is_empty(),
        "an unseated command's bare word is silence"
    );
}

// --- the menu state machine (suggest-menu) ---

/// Menu rows from (label, suffix) pairs — the state machine's
/// fixtures; Specs provenance throughout (the source is not the
/// state machine's business).
fn menu_rows(entries: &[(&str, &str)]) -> Vec<MenuCandidate> {
    entries
        .iter()
        .map(|(label, suffix)| MenuCandidate {
            label: (*label).to_string(),
            suffix: (*suffix).to_string(),
            description: None,
            source: SuggestionSource::Specs,
        })
        .collect()
}

#[test]
fn open_lands_the_collection_in_source_order_and_declines_an_empty_one() {
    // The record's ruled contents: the merged collection's own
    // source order — history on top — is the display order, the
    // selection at the top, no re-ranking at open. An empty
    // collection declines to open: no candidates, no menu.
    let mut menu = MenuState::new();
    assert!(!menu.is_open());
    assert!(
        !menu.open(menu_rows(&[])),
        "an empty collection never opens the menu"
    );
    assert!(!menu.is_open());
    let rows = menu_rows(&[("git status", "atus"), ("stash", "ash")]);
    assert!(menu.open(rows.clone()));
    assert!(menu.is_open());
    assert_eq!(menu.rows(), rows.as_slice());
    assert_eq!(menu.selected(), 0, "selection lands at the top");
    assert_eq!(menu.scroll(), 0);
}

#[test]
fn move_clamps_at_the_edges_and_the_scroll_window_follows() {
    // Ten rows over the eight-row budget (OQ-2, locked): walking
    // past the bottom clamps at the last row while the scroll
    // window follows (the selection always visible), and walking
    // past the top clamps at the first with the window back at the
    // top.
    let rows: Vec<MenuCandidate> = (0..10)
        .map(|i| MenuCandidate {
            label: format!("row-{i}"),
            suffix: format!("-{i}"),
            description: None,
            source: SuggestionSource::Specs,
        })
        .collect();
    let mut menu = MenuState::new();
    menu.open(rows);
    for _ in 0..12 {
        menu.press(MenuKey::Down);
    }
    assert_eq!(menu.selected(), 9, "clamped at the last row");
    assert_eq!(menu.scroll(), 2, "the window follows: rows 2..10 visible");
    for _ in 0..12 {
        menu.press(MenuKey::Up);
    }
    assert_eq!(menu.selected(), 0);
    assert_eq!(menu.scroll(), 0);
}

#[test]
fn accept_returns_only_the_selected_suffix_then_closes() {
    // The accept's whole payload is the suffix string — the exact
    // bytes input_no_scroll writes; the label never reaches the
    // PTY. After the accept the menu is closed and spent.
    let mut menu = MenuState::new();
    menu.open(menu_rows(&[("git status", "atus"), ("stash", "ash")]));
    menu.press(MenuKey::Down);
    assert_eq!(menu.press(MenuKey::Accept).as_deref(), Some("ash"));
    assert!(!menu.is_open());
    assert!(menu.rows().is_empty());
    assert_eq!(
        menu.press(MenuKey::Accept),
        None,
        "a closed menu accepts nothing"
    );
}

#[test]
fn any_other_key_closes_the_menu() {
    // The any-other-key rule (suggest-menu spec): a press the menu
    // does not claim closes it — the press itself then flows on
    // through the normal handler (the widget's routing, Task 3).
    let mut menu = MenuState::new();
    menu.open(menu_rows(&[("stash", "ash")]));
    assert_eq!(menu.press(MenuKey::Other), None);
    assert!(!menu.is_open());
}

#[test]
fn the_ranker_puts_prefix_hits_above_subsequence_hits() {
    // Warp's pinned policy: case-insensitive prefix first, the
    // subsequence fallback second — `drill` (prefix) outranks
    // `--dry-run`, `--dream`, and `--dr` (subsequence) whatever the
    // collection order, and the subsequence ties keep their
    // provenance order (the stable sort).
    let mut menu = MenuState::new();
    menu.open(menu_rows(&[
        ("--dry-run", "y-run"),
        ("--dream", "eam"),
        ("--dr", "r"),
        ("drill", "ill"),
    ]));
    menu.filter("dr");
    let labels: Vec<&str> = menu.rows().iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["drill", "--dry-run", "--dream", "--dr"]);
    assert_eq!(menu.selected(), 0, "a re-rank resets the selection");
}

#[test]
fn the_ranker_matches_case_insensitively() {
    let mut menu = MenuState::new();
    menu.open(menu_rows(&[("git", "it"), ("--dry-run", "y-run")]));
    menu.filter("GI");
    let labels: Vec<&str> = menu.rows().iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["git"], "the uppercase query still prefixes");
}

#[test]
fn the_ranker_prefers_contiguous_runs_over_scattered_matches() {
    // Contiguity-aware: `--dyx` matches d-then-y adjacently (a
    // two-character run), `--day` matches scattered — the contiguous
    // run outranks, provenance order notwithstanding.
    let mut menu = MenuState::new();
    menu.open(menu_rows(&[("--day", "ay"), ("--dyx", "yx")]));
    menu.filter("dy");
    let labels: Vec<&str> = menu.rows().iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["--dyx", "--day"]);
}

#[test]
fn a_punctuation_only_query_never_falls_back_to_subsequence() {
    // Warp's own gate (matchers.rs): the subsequence fallback exists
    // only for a query with at least one alphanumeric — fuzzy
    // matches on punctuation produce poor results. At the bare `--`
    // flag seat the prefix tier answers alone.
    let mut menu = MenuState::new();
    menu.open(menu_rows(&[("x--y", "y"), ("--all", "ll")]));
    menu.filter("--");
    let labels: Vec<&str> = menu.rows().iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["--all"]);
}

#[test]
fn a_filter_that_empties_the_set_closes_the_menu() {
    // An open menu with no rows is a state the machine refuses: a
    // query nothing matches closes rather than showing an empty box.
    let mut menu = MenuState::new();
    menu.open(menu_rows(&[("stash", "ash")]));
    menu.filter("zzz");
    assert!(!menu.is_open());
    assert!(menu.rows().is_empty());
}

#[test]
fn an_empty_query_keeps_the_collection_order() {
    // The vacuous prefix: an empty filter query matches everything
    // as a prefix hit, and the stable sort keeps the merge's source
    // order untouched.
    let rows = menu_rows(&[("git status", "atus"), ("stash", "ash")]);
    let mut menu = MenuState::new();
    menu.open(rows.clone());
    menu.filter("");
    assert_eq!(menu.rows(), rows.as_slice());
}

// --- the loadable spec table (suggest-specs-data) ---

/// A fixture directory under the temp root, unique per name — the
/// engine tests' std-only convention, wiped for the next run.
fn completions_fixture(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("dropterm-specs-data-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

/// The format's whole shape in miniature: one new command, one
/// described subcommand beside a bare one, one described flag —
/// every tier the schema owns.
const GH_FILE: &str = r#"
[[commands]]
name = "gh"

[[commands.subcommands]]
name = "pr"
description = "Manage pull requests"

[[commands.subcommands]]
name = "pr-check"

[[commands.flags]]
name = "--repo"
description = "Select another repository"
"#;

#[test]
fn a_minimal_file_parses_descriptions_opt_in() {
    // One command, one described subcommand, one bare flag: the
    // format's whole shape. A missing description is `None` — the
    // menu renders clean without it; a present one parses verbatim.
    let table = parse_completions(GH_FILE).expect("a minimal valid file parses");
    assert_eq!(table.len(), 1, "one command declared");
    assert_eq!(table[0].name, "gh");
    assert_eq!(
        table[0].subcommands[0].description.as_deref(),
        Some("Manage pull requests")
    );
    assert_eq!(
        table[0].subcommands[1].description, None,
        "a missing description is None"
    );
    assert_eq!(
        table[0].flags[0].description.as_deref(),
        Some("Select another repository")
    );
}

#[test]
fn a_script_key_rejects_the_file_whole() {
    // The no-scripts boundary, made structural (the generators
    // record's permanent rejection): a `script` key is unknown to
    // the schema and `deny_unknown_fields` rejects the whole file —
    // pinned by name at the entry tier where the Fig format
    // carries it, and joined by the `generators` shape at the
    // tiers above.
    let entry_tier = r#"
[[commands]]
name = "gh"

[[commands.subcommands]]
name = "pr"
script = "gh pr list"
"#;
    assert!(
        parse_completions(entry_tier).is_err(),
        "an entry-level `script` rejects the file"
    );
    let command_tier = r#"
[[commands]]
name = "gh"
script = "echo hi"
"#;
    assert!(
        parse_completions(command_tier).is_err(),
        "a command-level unknown key rejects the file"
    );
    let root_tier = r#"
generators = []

[[commands]]
name = "gh"
"#;
    assert!(
        parse_completions(root_tier).is_err(),
        "a root-level `generators` key rejects the file"
    );
}

#[test]
fn a_parsed_table_normalizes_to_the_ruled_order() {
    // The match policy rides sorted lists — shortest wins, ties
    // alphabetical, both falling out of `min_by_key` the way the
    // built-in's do — so parsing normalizes: commands and every
    // entry list sort by name, stable among equals.
    let unsorted = r#"
[[commands]]
name = "zb"

[[commands]]
name = "gh"

[[commands.subcommands]]
name = "zebra"

[[commands.subcommands]]
name = "pr"

[[commands.flags]]
name = "--b"

[[commands.flags]]
name = "--a"
"#;
    let table = parse_completions(unsorted).expect("an unsorted file still parses");
    let names: Vec<&str> = table.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["gh", "zb"], "commands sort by name");
    let subcommands: Vec<&str> = table[0]
        .subcommands
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(subcommands, ["pr", "zebra"], "subcommands sort by name");
    let flags: Vec<&str> = table[0].flags.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(flags, ["--a", "--b"], "flags sort by name");
}

#[test]
fn the_embedded_floor_is_the_core_tier() {
    // The compressed floor IS the generated core tier (suggest-embed
    // spec, the 2026-11-07 amendment): the `.gz` carries gzip's magic
    // and stays under the 1 MiB pin on its compressed bytes, it
    // inflates to exactly the raw file's bytes — the two cannot drift
    // apart — the parsed table is byte-identical to the corpus
    // fixture's own parse of the same text, and the old five all
    // keep their seats in it.
    use std::io::Read as _;

    let raw = include_str!("../../../data/completions.toml");
    let compressed: &[u8] = include_bytes!("../../../data/completions.toml.gz");
    assert_eq!(
        &compressed[..2],
        &[0x1f, 0x8b],
        "the embedded floor carries gzip's magic bytes"
    );
    assert!(
        !compressed.is_empty() && compressed.len() <= 1024 * 1024,
        "the compressed floor stays under the 1 MiB pin ({} bytes)",
        compressed.len()
    );
    let mut inflated = String::new();
    flate2::read::GzDecoder::new(compressed)
        .read_to_string(&mut inflated)
        .expect("the embedded bytes inflate");
    assert_eq!(inflated, raw, "the .gz and the raw file cannot drift apart");
    assert_eq!(
        builtin_specs(),
        &*CORE_TABLE,
        "the floor is the core tier's own parse"
    );
    for name in ["cargo", "docker", "git", "npm", "systemctl"] {
        assert!(
            builtin_specs().iter().any(|command| command.name == name),
            "{name} keeps its seat in the embedded floor"
        );
    }
}

#[test]
fn the_file_extends_the_builtin_and_redeclarations_replace() {
    // The merge ruling: the built-in five are the floor — a file
    // command adds beside them, and a re-declared command replaces
    // its built-in whole (no per-entry merging: a re-declaration is
    // the user's table for that command).
    let file = parse_completions(
        r#"
[[commands]]
name = "gh"

[[commands]]
name = "git"

[[commands.subcommands]]
name = "worktree"
"#,
    )
    .expect("a valid file parses");
    let merged = merge_completions(file);
    let names: Vec<&str> = merged.iter().map(|c| c.name.as_str()).collect();
    let floor: Vec<&str> = builtin_specs().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names, floor,
        "the floor keeps every seat (gh already seated — re-declared whole)"
    );
    let git = merged
        .iter()
        .find(|c| c.name == "git")
        .expect("git is re-declared");
    assert_eq!(
        git.subcommands.len(),
        1,
        "a re-declaration replaces the built-in whole"
    );
    assert_eq!(git.subcommands[0].name, "worktree");
}

#[test]
fn merge_whole_leaks_nothing_either_direction_at_depth() {
    // The merge rule against the grown tree (suggest-schema spec,
    // criterion 4, pinned for the three-layer merge suggest-embed
    // builds on it): a re-declared command with nested content
    // replaces its built-in whole — the built-in's flags never leak
    // into the re-declaration, the re-declaration's nested tiers
    // arrive intact, and no other command loses a leaf. Whole-tree
    // equality is the pin: PartialEq now reaches every depth.
    let file = parse_completions(
        r#"
[[commands]]
name = "git"

[[commands.subcommands]]
name = "worktree"
description = "Manage multiple working trees"

[[commands.subcommands.flags]]
name = "--quiet"

[[commands.subcommands.subcommands]]
name = "add"
"#,
    )
    .expect("a valid nested file parses");
    let merged = merge_completions(file);
    let git = merged
        .iter()
        .find(|c| c.name == "git")
        .expect("git is re-declared");
    assert!(
        git.flags.is_empty(),
        "the built-in's flags never leak into the re-declaration"
    );
    assert_eq!(
        git.subcommands.len(),
        1,
        "the built-in's subcommands are gone whole"
    );
    let worktree = &git.subcommands[0];
    assert_eq!(worktree.name, "worktree");
    assert_eq!(
        worktree.description.as_deref(),
        Some("Manage multiple working trees")
    );
    assert_eq!(worktree.flags.len(), 1, "a flag on the nested tier arrives");
    assert_eq!(worktree.flags[0].name, "--quiet");
    assert_eq!(
        worktree.subcommands.len(),
        1,
        "a third level arrives through the merge intact"
    );
    assert_eq!(worktree.subcommands[0].name, "add");
    let untouched = |name: &str| {
        let merged_command = merged
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("{name} keeps its seat"));
        let builtin_command = builtin_specs()
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("{name} is built in"));
        assert_eq!(
            *merged_command, *builtin_command,
            "{name} is untouched to the last leaf"
        );
    };
    untouched("docker");
    untouched("cargo");
}

#[test]
fn no_file_and_no_config_dir_answer_the_builtin_table() {
    // The fallback contract: an absent directory and an absent
    // file are both today's table — the default state is
    // byte-equivalent to the built-in (criterion 2).
    let dir = completions_fixture("absent");
    assert_eq!(
        load_completions(Some(&dir)).as_ref(),
        builtin_specs(),
        "no file answers the built-in"
    );
    assert_eq!(
        load_completions(None).as_ref(),
        builtin_specs(),
        "no config directory answers the built-in"
    );
}

#[test]
fn an_invalid_or_scripted_file_answers_the_builtin_table() {
    // A file that fails to parse — broken TOML, or a `script` key
    // the schema structurally rejects — never half-loads: the
    // whole file falls back to the built-in table.
    let dir = completions_fixture("invalid");
    std::fs::write(dir.join("completions.toml"), b"[[commands\nname = broken")
        .expect("fixture file");
    assert_eq!(
        load_completions(Some(&dir)).as_ref(),
        builtin_specs(),
        "broken TOML answers the built-in"
    );
    let dir = completions_fixture("scripted");
    std::fs::write(
        dir.join("completions.toml"),
        b"[[commands]]\nname = \"gh\"\n\n[[commands.subcommands]]\nname = \"pr\"\nscript = \"gh pr list\"\n",
    )
    .expect("fixture file");
    assert_eq!(
        load_completions(Some(&dir)).as_ref(),
        builtin_specs(),
        "a scripted entry answers the built-in"
    );
}

#[test]
fn a_valid_file_answers_the_merged_table() {
    let dir = completions_fixture("valid");
    std::fs::write(dir.join("completions.toml"), GH_FILE).expect("fixture file");
    let table = load_completions(Some(&dir));
    let names: Vec<&str> = table.iter().map(|c| c.name.as_str()).collect();
    let floor: Vec<&str> = builtin_specs().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names, floor,
        "a valid file's table is the merge over the floor"
    );
}

#[test]
fn the_ghosts_spec_tier_answers_from_a_wider_table() {
    // The data file widens the table; it changes no rules
    // (criterion 1): the file-declared `gh` completes at the
    // subcommand seat under the unchanged strict-prefix,
    // must-extend, shortest-wins policy — `gh pr` finishing to
    // `pr-check` past the exactly-typed `pr` — and the flag seat
    // answers from the file's flags alike.
    let table = merge_completions(parse_completions(GH_FILE).expect("valid"));
    let mut suggest = specs_suggest();
    let before = String::from("$ gh pr");
    let observation = Observation {
        before_cursor: &before,
        cursor_column: before.len(),
        specs_table: &table,
        ..obs()
    };
    let shown = step(&observation, &mut suggest).expect("the wider table answers");
    assert_eq!(shown.suffix, "-check");
    assert_eq!(shown.source, SuggestionSource::Specs);
    let mut suggest = specs_suggest();
    let before = String::from("$ gh --re");
    let observation = Observation {
        before_cursor: &before,
        cursor_column: before.len(),
        specs_table: &table,
        ..obs()
    };
    let shown = step(&observation, &mut suggest).expect("the flag seat answers");
    assert_eq!(shown.suffix, "po");
}

#[test]
fn file_descriptions_reach_the_menu_rows() {
    // End-to-end (suggest-specs-data spec, criterion 1): a
    // description declared in the file rides the table into the
    // menu's Specs rows — the dim second column the renderer
    // already shapes — while an entry without one stays clean
    // (`None`), and every other provider's rows stay as bare as
    // ever.
    let table = merge_completions(parse_completions(GH_FILE).expect("valid"));
    let suggest = specs_suggest();
    let observation = Observation {
        specs_table: &table,
        ..obs()
    };
    // `p` is a strict prefix of both entries: the described `pr`
    // lists beside the bare `pr-check`, in the table's sorted order.
    let menu = candidates("gh p", &suggest, &observation);
    let descriptions: Vec<Option<&str>> = menu.iter().map(|c| c.description.as_deref()).collect();
    assert_eq!(
        descriptions,
        [Some("Manage pull requests"), None],
        "the described entry carries it, the bare one stays None"
    );
    let menu = candidates("gh --rep", &suggest, &observation);
    assert_eq!(
        menu[0].description.as_deref(),
        Some("Select another repository"),
        "flag entries carry descriptions alike"
    );
}

// --- the generated corpus tiers (suggest-corpus) ---

/// The extended tier, never embedded: installed as a data file behind
/// `autosuggestions_completions_extended` (suggest-embed).
const EXTENDED_TIER: &str = include_str!("../../../data/completions-extended.toml");

/// The core tier's committed bytes, named beside its companion: the
/// header pin reads this text, every parse-reading pin the fixtures
/// below, so each raw path seats in exactly one place.
const CORE_TIER: &str = include_str!("../../../data/completions.toml");

/// The tiers parsed once per run and shared: a 4 MiB parse costs
/// seconds and the corpus and seed pins all read the same two tables
/// (the shared-fixture rule, testing.md §2), so each tier parses here
/// exactly once. Each `expect` is the schema gate — the committed
/// bytes failing it fails every consumer together, the tier named in
/// the panic.
static CORE_TABLE: std::sync::LazyLock<Vec<SpecCommand>> = std::sync::LazyLock::new(|| {
    parse_completions(CORE_TIER).expect("the generated core tier parses under the schema")
});
static EXTENDED_TABLE: std::sync::LazyLock<Vec<SpecCommand>> = std::sync::LazyLock::new(|| {
    parse_completions(EXTENDED_TIER).expect("the generated extended tier parses under the schema")
});

#[test]
fn both_generated_tiers_parse_through_the_schema() {
    // The extractor's contract (suggest-corpus spec): the Rust parser
    // is the gate — if either generated file fails the schema, the
    // script is wrong, not the schema. Both files are committed, so
    // this pin holds the committed bytes to it. The parses live in
    // the shared fixtures above, whose expects are that gate —
    // touching either table here runs it — and the non-empty floors
    // ride along.
    assert!(!CORE_TABLE.is_empty(), "the core tier carries its commands");
    assert!(
        !EXTENDED_TABLE.is_empty(),
        "the extended tier carries its commands"
    );
}

#[test]
fn the_core_tier_never_shrinks_a_command() {
    // The no-shrink golden, generalized per regeneration (suggest-
    // corpus spec): every command of the previous core file — the
    // hand-converted seed's own list — still seats in the generated
    // core tier. A regeneration that loses a command fails here; the
    // list grows when a regeneration adds names worth keeping.
    const PREVIOUS_CORE: &[&str] = &[
        "apt",
        "cargo",
        "curl",
        "docker",
        "gh",
        "git",
        "kubectl",
        "make",
        "npm",
        "pip",
        "rsync",
        "systemctl",
        "tar",
    ];
    let core = &*CORE_TABLE;
    for name in PREVIOUS_CORE {
        assert!(
            core.iter().any(|command| command.name == *name),
            "'{name}' survives every regeneration"
        );
    }
}

#[test]
fn the_two_generated_tiers_are_disjoint() {
    // The extractor refuses a name in both tiers at run time; this
    // pin holds the committed bytes to the same rule, so a hand edit
    // or a bad regeneration cannot seat one command twice.
    let core = &*CORE_TABLE;
    let extended = &*EXTENDED_TABLE;
    for command in extended {
        assert!(
            !core.iter().any(|seat| seat.name == command.name),
            "'{}' sits in both tiers — the extractor's disjointness broke",
            command.name
        );
    }
}

/// Collects every flag name in a spec tree at every depth — the
/// command's own flags, then each subcommand's whole tree — into
/// `sink` in walk order.
fn flag_names(command: &SpecCommand, sink: &mut Vec<String>) {
    for flag in &command.flags {
        sink.push(flag.name.clone());
    }
    for sub in &command.subcommands {
        flag_names(sub, sink);
    }
}

#[test]
fn every_generated_flag_name_keeps_its_dash() {
    // The seed audit's conversion rule, enforced (suggest-corpus
    // spec): every flag `name` in either tier starts with `-` — the
    // extractor asserts it at run time over the reduced trees; this
    // holds the committed bytes to the same truth.
    for (tier, table) in [("core", &*CORE_TABLE), ("extended", &*EXTENDED_TABLE)] {
        let mut flags = Vec::new();
        for command in table {
            flag_names(command, &mut flags);
        }
        for name in flags {
            assert!(
                name.starts_with('-'),
                "the {tier} tier's flag '{name}' lost its dash"
            );
        }
    }
}

#[test]
fn both_generated_headers_carry_the_notice_and_the_pin() {
    // The provenance the spec demands in both headers: the MIT notice
    // verbatim, the pinned version, the pinned sha256 — so a file
    // separated from its recipe still tells where it came from.
    for (tier, text) in [("core", CORE_TIER), ("extended", EXTENDED_TIER)] {
        assert!(
            text.contains("MIT License"),
            "the {tier} header carries the notice"
        );
        assert!(
            text.contains("2.692.3"),
            "the {tier} header carries the pinned version"
        );
        assert!(
            text.contains("b07159ef0811bc8cec28e3a3016e6ba9568d774b75b8333dc6a5f2acf7d2c3af"),
            "the {tier} header carries the pinned sha256"
        );
    }
}

#[test]
fn the_ghost_ignores_descriptions() {
    // Locked (suggest-specs-data spec, OQ 3): descriptions are the
    // menu's — the ghost is single-choice text, and a described
    // entry ghosts exactly the suffix it would bare. `Showing`
    // carries no description field, so the lock is structural;
    // this pins the behavior it buys: a re-declared, described
    // checkout ghosts the same `ckout` the bare built-in does.
    let table = merge_completions(
        parse_completions(
            r#"
[[commands]]
name = "git"

[[commands.subcommands]]
name = "checkout"
description = "Switch branches or restore working tree files"
"#,
        )
        .expect("valid"),
    );
    let mut suggest = specs_suggest();
    let before = String::from("$ git che");
    let observation = Observation {
        before_cursor: &before,
        cursor_column: before.len(),
        specs_table: &table,
        ..obs()
    };
    let shown = step(&observation, &mut suggest).expect("the ghost answers the same");
    assert_eq!(shown.suffix, "ckout");
    assert_eq!(shown.source, SuggestionSource::Specs);
}

// --- the recursive spec tree (suggest-schema) ---

/// The seed's flat projection — every command, then its subcommand
/// and flag names beside their descriptions, one tab-separated line
/// per row in parse order: the shape today's parse answered before
/// the schema grew (suggest-schema spec, criterion 1), pinned in
/// `fixtures/seed-golden.txt`. Reads only `name` and `description`,
/// the two keys every tier kept, so the pin is the superset proof.
fn seed_projection(table: &[SpecCommand]) -> Vec<String> {
    let mut lines = Vec::new();
    // The golden is line-oriented; some upstream descriptions carry
    // literal newlines (`less`'s help text), so a row escapes them or
    // `golden.lines()` would split one row into many.
    let flat = |description: &str| description.replace(['\n', '\r'], "\\n");
    for command in table {
        lines.push(format!("command\t{}", command.name));
        for entry in &command.subcommands {
            lines.push(format!(
                "sub\t{}\t{}",
                entry.name,
                flat(entry.description.as_deref().unwrap_or_default())
            ));
        }
        for entry in &command.flags {
            lines.push(format!(
                "flag\t{}\t{}",
                entry.name,
                flat(entry.description.as_deref().unwrap_or_default())
            ));
        }
    }
    lines
}

/// The schema's whole shape at three levels, every new key seated
/// (criterion 2); the file writes its lists out of order so the
/// expected tree below is the normalized one.
const NESTED_FILE: &str = include_str!("fixtures/nested.toml");

/// A `script` key on a third-level subcommand (criterion 3) — valid
/// but for that one line.
const DEEP_SCRIPT_FILE: &str = include_str!("fixtures/deep-script.toml");

#[test]
fn the_seed_parses_golden_equal_to_todays_table() {
    // The superset proof, corpus era (suggest-corpus spec): the seed
    // is now GENERATED, and it exercises the grown schema by design —
    // so the golden pins what must stay stable across a regeneration,
    // the flat names-and-descriptions rows (the row diff reads
    // first), while the every-grown-key-at-default proof moved to
    // `a_nested_fixture_round_trips_every_new_key`, which seats each
    // key explicitly. The rows were captured from the generated
    // file's own parse.
    let golden = include_str!("fixtures/seed-golden.txt");
    let table = &*CORE_TABLE;
    let rows: Vec<&str> = golden.lines().collect();
    assert_eq!(
        seed_projection(table),
        rows,
        "the seed's names and descriptions are today's"
    );
}

#[test]
fn a_nested_fixture_round_trips_every_new_key() {
    // Three levels (docker → compose → up) carrying every key the
    // engine-rea §5 census admits plus `aliases` and
    // `filter_strategy`: a flag's short-form aliases and persistence,
    // `exclusive_on`, `hidden` on a flag and on a subcommand,
    // `priority` on two tiers, a fuzzy `filter_strategy`, arguments
    // with `template`, `optional`, `variadic`, literal `suggestions`,
    // and the sudo shape (`command = true`). Lists arrive normalized;
    // arguments keep their positional order.
    let table = parse_completions(NESTED_FILE).expect("the nested fixture parses");
    let expected = vec![SpecCommand {
        name: "docker".into(),
        description: Some("A self-sufficient runtime for containers".into()),
        aliases: vec!["dock".into()],
        priority: Some(60),
        filter_strategy: FilterStrategy::Fuzzy,
        flags: vec![SpecFlag {
            name: "--config".into(),
            description: Some("Location of client config files".into()),
            aliases: vec!["-c".into()],
            persistent: true,
            args: vec![SpecArg {
                name: "path".into(),
                template: Some(ArgTemplate::Folders),
                ..Default::default()
            }],
            ..Default::default()
        }],
        subcommands: vec![
            SpecCommand {
                name: "compose".into(),
                description: Some("Define and run multi-container applications".into()),
                flags: vec![SpecFlag {
                    name: "--file".into(),
                    description: Some("Compose configuration files".into()),
                    aliases: vec!["-f".into()],
                    args: vec![SpecArg {
                        name: "file".into(),
                        template: Some(ArgTemplate::Filepaths),
                        variadic: true,
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                subcommands: vec![SpecCommand {
                    name: "up".into(),
                    description: Some("Create and start containers".into()),
                    priority: Some(80),
                    flags: vec![
                        SpecFlag {
                            name: "--attach".into(),
                            description: Some("Attach to dependent containers".into()),
                            hidden: true,
                            ..Default::default()
                        },
                        SpecFlag {
                            name: "--detach".into(),
                            description: Some(
                                "Detached mode: run containers in the background".into(),
                            ),
                            aliases: vec!["-d".into()],
                            exclusive_on: vec!["--attach".into()],
                            ..Default::default()
                        },
                    ],
                    args: vec![SpecArg {
                        name: "service".into(),
                        optional: true,
                        variadic: true,
                        suggestions: vec![
                            SpecEntry {
                                name: "db".into(),
                                description: None,
                            },
                            SpecEntry {
                                name: "web".into(),
                                description: Some("The web service".into()),
                            },
                        ],
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            },
            SpecCommand {
                name: "exec".into(),
                description: Some("Execute a command in a running container".into()),
                args: vec![
                    SpecArg {
                        name: "container".into(),
                        ..Default::default()
                    },
                    SpecArg {
                        name: "command".into(),
                        command: true,
                        variadic: true,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            SpecCommand {
                name: "trust".into(),
                hidden: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    }];
    assert_eq!(table, expected);
}

#[test]
fn an_unknown_key_at_any_depth_rejects_the_file_whole() {
    // The no-scripts boundary, as deep as the tree (suggest-schema
    // spec, criterion 3): `deny_unknown_fields` on every tier means
    // a `script` key on a third-level subcommand, a `generators` key
    // on an argument, and an unknown key on a suggestion each fail
    // the whole file. Each case carries its positive control — the
    // same file without the offending line parses — so the rejection
    // is the key's, never a tier the schema lacks.
    let valid_deep = DEEP_SCRIPT_FILE
        .lines()
        .filter(|line| !line.starts_with("script"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        parse_completions(&valid_deep).is_ok(),
        "the deep fixture is valid but for its script line"
    );
    assert!(
        parse_completions(DEEP_SCRIPT_FILE).is_err(),
        "a third-level `script` rejects the file"
    );
    let arg_tier = r#"
[[commands]]
name = "gh"

[[commands.subcommands]]
name = "pr"

[[commands.subcommands.args]]
name = "number"
"#;
    assert!(parse_completions(arg_tier).is_ok(), "the arg tier is valid");
    assert!(
        parse_completions(&format!("{arg_tier}generators = [\"gh pr list\"]\n")).is_err(),
        "a `generators` key on an arg rejects the file"
    );
    let suggestion_tier = format!(
        "{arg_tier}
[[commands.subcommands.args.suggestions]]
name = \"42\"
"
    );
    assert!(
        parse_completions(&suggestion_tier).is_ok(),
        "the suggestion tier is valid"
    );
    assert!(
        parse_completions(&format!("{suggestion_tier}icon = \"🔢\"\n")).is_err(),
        "an unknown key on a suggestion rejects the file"
    );
    let dir = completions_fixture("deep-script");
    std::fs::write(dir.join("completions.toml"), DEEP_SCRIPT_FILE).expect("fixture file");
    assert_eq!(
        load_completions(Some(&dir)).as_ref(),
        builtin_specs(),
        "a deep script answers the built-in floor"
    );
}

#[test]
fn sort_normalization_holds_at_every_tier() {
    // The ruled order, recursive (suggest-schema spec, criterion 5):
    // nested subcommands, nested flags, and literal suggestions —
    // under a command's and a flag's arguments alike — sort by name
    // at parse, stable among equals (two `--a` keep their file
    // order, told apart by description); arguments are positional and
    // keep their file order whole.
    let unsorted = r#"
[[commands]]
name = "zz"

[[commands]]
name = "aa"

[[commands.subcommands]]
name = "sub-b"

[[commands.subcommands.subcommands]]
name = "deep-z"

[[commands.subcommands.subcommands]]
name = "deep-a"

[[commands.subcommands.subcommands.flags]]
name = "--z"

[[commands.subcommands.subcommands.flags.args]]
name = "level"

[[commands.subcommands.subcommands.flags.args.suggestions]]
name = "zf"

[[commands.subcommands.subcommands.flags.args.suggestions]]
name = "af"

[[commands.subcommands.subcommands.flags]]
name = "--a"

[[commands.subcommands.subcommands.flags]]
name = "--a"
description = "the second --a"

[[commands.subcommands.subcommands.args]]
name = "zeta"

[[commands.subcommands.subcommands.args]]
name = "alpha"

[[commands.subcommands.subcommands.args.suggestions]]
name = "zs"

[[commands.subcommands.subcommands.args.suggestions]]
name = "as"

[[commands.subcommands]]
name = "sub-a"
"#;
    let table = parse_completions(unsorted).expect("an unsorted nested file parses");
    let names = |commands: &[SpecCommand]| -> Vec<String> {
        commands.iter().map(|c| c.name.clone()).collect()
    };
    assert_eq!(names(&table), ["aa", "zz"], "commands sort by name");
    let aa = &table[0];
    assert_eq!(
        names(&aa.subcommands),
        ["sub-a", "sub-b"],
        "subcommands sort"
    );
    let sub_b = &aa.subcommands[1];
    assert_eq!(
        names(&sub_b.subcommands),
        ["deep-a", "deep-z"],
        "nested subcommands sort"
    );
    let deep_a = &sub_b.subcommands[0];
    let flags: Vec<(&str, Option<&str>)> = deep_a
        .flags
        .iter()
        .map(|f| (f.name.as_str(), f.description.as_deref()))
        .collect();
    assert_eq!(
        flags,
        [
            ("--a", None),
            ("--a", Some("the second --a")),
            ("--z", None)
        ],
        "nested flags sort by name, stable among equals"
    );
    let args: Vec<&str> = deep_a.args.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        args,
        ["zeta", "alpha"],
        "arguments keep positional file order"
    );
    let suggestions: Vec<&str> = deep_a.args[1]
        .suggestions
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(suggestions, ["as", "zs"], "an argument's suggestions sort");
    let flag_suggestions: Vec<&str> = deep_a.flags[2].args[0]
        .suggestions
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        flag_suggestions,
        ["af", "zf"],
        "a flag argument's suggestions sort alike"
    );
}

#[test]
fn an_accepted_flag_never_suggests_again() {
    // The dressing, end to end: `git --no-pager log --no-p` — the
    // persistent flag accepted before the descent never lists inside
    // the child, so the ghost stays silent on its own name.
    let mut suggest = specs_suggest();
    assert_eq!(
        step_typed_table("git --no-pager log --no-p", &WALKER_TABLE, &mut suggest),
        None,
        "the accepted flag does not complete itself"
    );
}

#[test]
fn a_fuzzy_node_routes_its_rows_through_filter_rank() {
    // `filter_strategy = "fuzzy"` (locked at the map gate): the docker
    // fixture's node routes through the pinned filter_rank — the
    // ranker's first caller — with the partial as the query. The
    // honest-write discipline holds under it too: a subsequence hit
    // that does not extend the typed word has no append-only suffix,
    // so it never lists (an accept writes only what extends).
    assert!(
        menu_table("docker cse", &NESTED_TABLE).is_empty(),
        "a subsequence hit that does not extend the word never carries a suffix"
    );
    assert_eq!(
        menu_labels(&menu_table("docker co", &NESTED_TABLE)),
        ["compose"],
        "an extending prefix at the fuzzy node still answers"
    );
}
