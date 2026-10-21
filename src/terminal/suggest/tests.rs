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
fn guard_suffix_crossing_the_line_end_yields_none() {
    // Three columns remain after the cursor; "t status" needs
    // eight. Clip, don't wrap — wrong-cell ghost text is worse
    // than none.
    let mut suggest = adopted();
    let o = Observation {
        columns: 8,
        ..obs()
    };
    assert_eq!(step(&o, &mut suggest), None);
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
    // columns and drops at three — by chars both would fit.
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
    let drops = Observation {
        columns: 10,
        ..typing
    };
    let shown = step(&fits, &mut suggest).expect("four cells fit four columns");
    assert_eq!(shown.suffix, "中文");
    assert_eq!(step(&drops, &mut suggest), None);
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
    // then the command catalog. The specs variant lands with the
    // flag-specs module and slots between path and catalog
    // (renumbering there, never reordering). Each later module
    // appends its variant here; the ranks must keep strictly
    // increasing in exactly this order.
    let ruled_order = [
        SuggestionSource::History,
        SuggestionSource::Path,
        SuggestionSource::Catalog,
    ];
    assert_eq!(source_rank(SuggestionSource::History), 0);
    assert_eq!(source_rank(SuggestionSource::Path), 1);
    assert_eq!(source_rank(SuggestionSource::Catalog), 2);
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
    let entries = listing(&[("src", true), ("srv", true)]);
    let shown = step_path("~/sr", Some(&entries), &mut suggest)
        .expect("the first sorted match completes");
    assert_eq!(shown.suffix, "c/");
    assert_eq!(shown.source, SuggestionSource::Path);
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
    let file = step_path("/usr/a", Some(&entries), &mut suggest)
        .expect("the file matches the component");
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
    let dot =
        step_path("~/.", Some(&entries), &mut suggest).expect("the dot reaches the dotfile");
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
    let spaced =
        step_path("~/my", Some(&entries), &mut suggest).expect("a space escapes safely");
    assert_eq!(spaced.suffix, "\\ file.txt");
    let parens = step_path("~/a", Some(&entries), &mut suggest)
        .expect("the metacharacters escape safely");
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
fn anything_outside_the_whitelist_and_the_escape_set_is_silence() {
    // A newline, a control character, a backtick (deliberately
    // not in the ruled sets), and non-ASCII: none is certainly
    // safe to write, so the entry silences.
    for (name, typed) in [
        ("bad\nname", "~/bad"),
        ("bad\x07bell", "~/bad"),
        ("bad`tick", "~/bad"),
        ("café", "~/caf"),
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
fn history_outranks_the_path_provider_when_both_match() {
    let mut suggest = adopted();
    let mut ring = VecDeque::new();
    ring.push_back("~/src/project".to_string());
    suggest.ring = ring;
    let entries = listing(&[("src", true)]);
    let shown = step_path("~/sr", Some(&entries), &mut suggest)
        .expect("someone answers the path token");
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
fn the_escaped_suffix_fits_in_grid_cells_or_drops() {
    // "\ file.txt" is ten cells; the cursor sits at column 6, so
    // seventeen columns fit and sixteen drop — the fit rule
    // measures the escaped form.
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
    let drops = Observation {
        columns: 16,
        ..fits
    };
    assert_eq!(
        step(&fits, &mut suggest).map(|s| s.suffix),
        Some("\\ file.txt".to_string())
    );
    assert_eq!(step(&drops, &mut suggest), None);
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
    // word is open, nothing it knows applies.
    let names = catalog(&["git", "echo"]);
    let mut suggest = catalog_suggest();
    assert_eq!(step_word("git che", &names, &mut suggest), None);
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
fn a_catalog_name_the_shell_would_mangle_escapes_or_silences() {
    // The suffix rides the path provider's escape rule: a
    // spaces-named executable completes to its backslashed form,
    // and a name outside the safe sets is silence, never a wrong
    // write.
    let spaced = catalog(&["my cmd"]);
    let mut suggest = catalog_suggest();
    let shown = step_word("my", &spaced, &mut suggest).expect("the space escapes safely");
    assert_eq!(shown.suffix, "\\ cmd");
    let odd = catalog(&["caf\u{e9}"]);
    let mut suggest = catalog_suggest();
    assert_eq!(step_word("caf", &odd, &mut suggest), None);
}
