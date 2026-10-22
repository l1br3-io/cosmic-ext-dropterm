use super::*;

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
        command_catalog: &[],
        focused: true,
        foreground_child: false,
        display_offset: 0,
        cursor_on_bottom_row: true,
        columns: 80,
        cursor_column: 4,
        before_cursor: "$ gi",
        after_cursor_blank: true,
        fresh_prompt: false,
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
fn guard_cursor_off_bottom_row_yields_none() {
    let mut suggest = adopted();
    let o = Observation {
        cursor_on_bottom_row: false,
        ..obs()
    };
    assert_eq!(step(&o, &mut suggest), None);
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
    ];
    assert_eq!(source_rank(SuggestionSource::History), 0);
    assert_eq!(source_rank(SuggestionSource::Path), 1);
    assert_eq!(source_rank(SuggestionSource::Specs), 2);
    assert_eq!(source_rank(SuggestionSource::Catalog), 3);
    for pair in ruled_order.windows(2) {
        assert!(
            source_rank(pair[0]) < source_rank(pair[1]),
            "the order above is the merge's order"
        );
    }
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
fn subcommands_complete_only_at_the_first_argument_seat() {
    // Once a subcommand (or any argument) holds the first seat, a
    // later trailing word never consults the subcommand table.
    let mut suggest = specs_suggest();
    assert_eq!(
        step_typed("git checkout che", &mut suggest),
        None,
        "a later argument seat never consults subcommands"
    );
    let mut suggest = specs_suggest();
    assert_eq!(step_typed("git stash dr", &mut suggest), None);
}

#[test]
fn a_bare_trailing_space_is_no_subcommand_word_at_all() {
    // The catalog's empty-word rule, one policy across providers:
    // `git ` with the space echoed carries no word to match.
    let mut suggest = specs_suggest();
    assert_eq!(step_typed("git ", &mut suggest), None);
}

#[test]
fn a_flag_prefixed_token_completes_from_any_argument_seat() {
    // OQ 2: git genuinely accepts flags before and after
    // subcommands alike, so a `--`-prefixed trailing token consults
    // the command's flag list from any argument seat — and the
    // hyphens ride verbatim through the escape rule's whitelist.
    let mut suggest = specs_suggest();
    let after = step_typed("git checkout --dr", &mut suggest)
        .expect("the flag seat answers past a subcommand");
    assert_eq!(after.suffix, "y-run");
    assert_eq!(after.source, SuggestionSource::Specs);
    let mut suggest = specs_suggest();
    let before =
        step_typed("git --dr", &mut suggest).expect("the flag seat answers at the first seat");
    assert_eq!(before.suffix, "y-run");
}

#[test]
fn flag_matches_follow_the_ruled_policy() {
    // The same policy the subcommand seat pins: `--v` lands on the
    // alphabetically-first of git's equal-length --verbose and
    // --version (shortest wins, ties alphabetical), and a fully
    // typed --dry-run has nothing left to add (the match must
    // extend).
    let mut suggest = specs_suggest();
    let tied = step_typed("git checkout --v", &mut suggest).expect("a tie still answers");
    assert_eq!(tied.suffix, "erbose");
    let mut suggest = specs_suggest();
    assert_eq!(
        step_typed("git checkout --dry-run", &mut suggest),
        None,
        "the exactly-typed flag suggests nothing"
    );
}

#[test]
fn the_five_commands_subcommands_answer_their_seats() {
    // One behavioral sample per command beyond git — the table pin
    // below holds the membership; these prove the entries reach the
    // seat (`docker p` also re-pins the tie rule on fresh data:
    // pull and push tie, pull wins alphabetically).
    let mut suggest = specs_suggest();
    let tied = step_typed("docker p", &mut suggest).expect("docker's seat answers");
    assert_eq!(tied.suffix, "ull");
    let mut suggest = specs_suggest();
    let stopped = step_typed("systemctl st", &mut suggest).expect("systemctl's seat answers");
    assert_eq!(
        stopped.suffix, "op",
        "stop is the shortest of start/status/stop"
    );
    let mut suggest = specs_suggest();
    let installed = step_typed("npm i", &mut suggest).expect("npm's seat answers");
    assert_eq!(installed.suffix, "nstall");
}

#[test]
fn a_command_outside_the_table_stays_silent() {
    // The provider is silent, never a guess: python3 is not one of
    // the five, so its argument seats answer nothing.
    let mut suggest = specs_suggest();
    assert_eq!(step_typed("python3 wha", &mut suggest), None);
}

#[test]
fn the_table_pins_the_five_curated_commands_and_their_shape() {
    // The ruled v1 set, pinned as data: exactly the five, no
    // others. Every entry ASCII, non-empty, trimmed — safe to
    // write by construction, so a careless future entry fails
    // here, never a shell — every list strictly sorted (which is
    // what makes ties-alphabetical hold), and every flag
    // `--`-shaped while no subcommand is dash-led.
    let mut commands: Vec<_> = SPECS.iter().map(|spec| spec.command).collect();
    commands.sort_unstable();
    assert_eq!(commands, ["cargo", "docker", "git", "npm", "systemctl"]);
    for spec in SPECS {
        pin_shape(spec.command, spec.subcommands, "subcommand");
        pin_shape(spec.command, spec.flags, "flag");
    }
}

/// One table list's shape pin: ASCII, non-empty, trimmed, strictly
/// sorted, and `--`-shaped exactly when it is a flag list.
fn pin_shape(command: &str, entries: &[&str], list: &str) {
    for entry in entries {
        assert!(!entry.is_empty(), "{command} {list}: an empty entry");
        assert!(entry.is_ascii(), "{command} {list}: {entry} leaves ASCII");
        assert_eq!(
            entry.trim(),
            *entry,
            "{command} {list}: {entry} carries whitespace"
        );
        assert_eq!(
            entry.starts_with("--"),
            list == "flag",
            "{command} {list}: {entry} sits in the wrong list"
        );
    }
    for pair in entries.windows(2) {
        assert!(
            pair[0] < pair[1],
            "{command} {list}: {} must sort ahead of {}",
            pair[0],
            pair[1]
        );
    }
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
