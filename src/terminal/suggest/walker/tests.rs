use super::super::{SpecCommand, parse_completions};
use super::*;

/// One lexer row's expectation: the token's text, whether whitespace (or
/// `=`) closed it, and whether it opened with `-`.
type Row = (&'static str, bool, bool);

/// Asserts the lexed table in full — text, `complete`, `is_flag` — the
/// shape every walker test keys on.
fn assert_lex(input: &str, expected: &[Row]) {
    let lexed: Vec<(String, bool, bool)> = lex(input)
        .into_iter()
        .map(|token| (token.text.into_owned(), token.complete, token.is_flag))
        .collect();
    let expected: Vec<(String, bool, bool)> = expected
        .iter()
        .map(|(text, complete, is_flag)| ((*text).to_string(), *complete, *is_flag))
        .collect();
    assert_eq!(lexed, expected, "lexing {input:?}");
}

#[test]
fn empty_input_lexes_no_tokens() {
    // No text, no command: the walker sees nothing, and the seats
    // answer silence before that.
    assert_lex("", &[]);
}

#[test]
fn whitespace_only_lexes_no_tokens() {
    // Leading whitespace never survives the carve; whitespace alone is
    // no word at all.
    assert_lex("   ", &[]);
}

#[test]
fn a_bare_word_is_the_incomplete_partial() {
    // A token is complete only when whitespace follows it — the whole
    // point of the partial the seats filter on.
    assert_lex("git", &[("git", false, false)]);
}

#[test]
fn trailing_whitespace_closes_the_token_and_yields_no_partial() {
    // The empty-word rule: `git ` (the space echoed) carries no partial
    // — the menu's whole-table seat, the ghost's silence.
    assert_lex("git ", &[("git", true, false)]);
}

#[test]
fn interior_whitespace_splits_complete_words() {
    // The ordinary shape: complete command word, open trailing word.
    assert_lex("git che", &[("git", true, false), ("che", false, false)]);
}

#[test]
fn multiple_spaces_do_not_create_empty_tokens() {
    // Ground state eats runs of whitespace; only words become tokens.
    assert_lex(
        "git   che  st",
        &[
            ("git", true, false),
            ("che", true, false),
            ("st", false, false),
        ],
    );
}

#[test]
fn the_text_after_the_last_command_delimiter_is_kept() {
    // `parser.ts:20`: `||`/`&&`/`;`/`|` all end a command; everything
    // before the last one is another command's business.
    assert_lex(
        "echo hi && git che",
        &[("git", true, false), ("che", false, false)],
    );
    assert_lex(
        "vim file | grep no",
        &[("grep", true, false), ("no", false, false)],
    );
    assert_lex(
        "make ; make test",
        &[("make", true, false), ("test", false, false)],
    );
    assert_lex("a || b c", &[("b", true, false), ("c", false, false)]);
    // A delimiter with nothing after it is no command at all.
    assert_lex("git che &&", &[]);
}

#[test]
fn a_leading_dash_opens_a_flag_token() {
    // The flag state: `-` opens it, and the walker will look the token
    // up among flags by name or alias.
    assert_lex("--am", &[("--am", false, true)]);
    assert_lex("git --am", &[("git", true, false), ("--am", false, true)]);
    // A single dash is still a flag-shaped token.
    assert_lex("mkdir -m", &[("mkdir", true, false), ("-m", false, true)]);
}

#[test]
fn equals_closes_a_flag_token_and_starts_its_value() {
    // `parser.ts:105`: `=` terminates a flag — `--emit=asm` is the
    // flag (complete) followed by its value as a fresh word, so the
    // walker meets `asm` as the option's argument.
    assert_lex(
        "--emit=asm",
        &[("--emit", true, true), ("asm", false, false)],
    );
    assert_lex(
        "rustc --emit=asm --crate-type",
        &[
            ("rustc", true, false),
            ("--emit", true, true),
            ("asm", true, false),
            ("--crate-type", false, true),
        ],
    );
    // Short flags close on `=` alike.
    assert_lex("-m=755", &[("-m", true, true), ("755", false, false)]);
}

#[test]
fn a_trailing_equals_yields_no_value_partial() {
    // `--emit=` (nothing after the `=`): the flag is complete, the
    // value word never opened — the walker stops at the option's
    // argument with an empty partial, the value seat.
    assert_lex(
        "rustc --emit=",
        &[("rustc", true, false), ("--emit", true, true)],
    );
}

#[test]
fn equals_inside_a_word_is_literal() {
    // Only flags close on `=`; a word keeps its `=` (a value like
    // `a=b` is one token).
    assert_lex("--set a=b", &[("--set", true, true), ("a=b", false, false)]);
    assert_lex(
        "grep key=val",
        &[("grep", true, false), ("key=val", false, false)],
    );
}

#[test]
fn a_quoted_token_unwraps_its_quotes() {
    // Quote characters never enter the text (the unwrap post-pass,
    // inlined): double, single, and backtick quotes all open the quoted
    // state, and spaces inside keep the word whole.
    assert_lex("\"git che\"", &[("git che", false, false)]);
    assert_lex(
        "'git che' st",
        &[("git che", true, false), ("st", false, false)],
    );
    assert_lex(
        "`git che` st",
        &[("git che", true, false), ("st", false, false)],
    );
}

#[test]
fn an_unterminated_quote_leaves_the_partial_open() {
    // The quote-continued state at end of input: the token is still
    // being typed, quotes unwrapped, incomplete.
    assert_lex("\"git ch", &[("git ch", false, false)]);
    assert_lex("'doc", &[("doc", false, false)]);
}

#[test]
fn a_quote_continued_token_unwraps_to_one_word() {
    // `"a"b` (the quote-continued state, `parser.ts` post-pass): a
    // non-space after the closing quote continues the same word.
    assert_lex("\"a\"b st", &[("ab", true, false), ("st", false, false)]);
    assert_lex("'doc's ", &[("docs", true, false)]);
}

#[test]
fn a_quoted_flag_opens_a_word_not_a_flag() {
    // The dash must open the token itself: a quoted `--amend` is a
    // word (engine-faithful — `isOption` is set only by the flag
    // state), so the walker treats it as a subcommand-or-arg word.
    assert_lex("\"--amend\" ", &[("--amend", true, false)]);
}

#[test]
fn a_backslash_escaped_space_keeps_the_word_open() {
    // One escape character (backslash — the POSIX shell answer,
    // `parser.ts:35`): `\ ` unwraps into a literal space inside the
    // token, the same carve `is_command_position` performs.
    assert_lex("my\\ file", &[("my file", false, false)]);
    assert_lex(
        "cat my\\ file.txt gi",
        &[
            ("cat", true, false),
            ("my file.txt", true, false),
            ("gi", false, false),
        ],
    );
}

#[test]
fn an_escaped_closing_quote_stays_literal() {
    // The escape unwraps whatever follows it, quotes included —
    // `\"git\"` is the literal text `\"git\"` minus the escapes.
    assert_lex("\\\"git\\\" ", &[("\"git\"", true, false)]);
}

#[test]
fn unicode_rides_whole_inside_words() {
    // Char boundaries hold through the unwrap paths: multibyte text
    // and multibyte escapes never split a character.
    assert_lex("git café", &[("git", true, false), ("café", false, false)]);
    assert_lex(
        "echo caf\\é ",
        &[("echo", true, false), ("café", true, false)],
    );
    assert_lex("\"café\" ", &[("café", true, false)]);
}

// --- the walk (suggest-walker Task 2; engine-rea §3, §7) ---

/// The walker fixture's table, parsed once per run (the shared-fixture
/// rule): git with per-subcommand flags and persistence, mkdir with a
/// mode flag and a folders arg, rustc with `--emit`'s literal
/// suggestions, the sudo shape beside apt, gh's two-level nesting, and
/// flit's depleted-positional pair.
static TREES: std::sync::LazyLock<Vec<SpecCommand>> = std::sync::LazyLock::new(|| {
    parse_completions(include_str!("../fixtures/walker.toml")).expect("the walker fixture parses")
});

/// The schema fixture's docker tree (suggest-schema's own shape
/// exercise), parsed once — nesting, persistence, exclusivity,
/// hidden, priority, and the exec arg pair live here.
static NESTED: std::sync::LazyLock<Vec<SpecCommand>> = std::sync::LazyLock::new(|| {
    parse_completions(include_str!("../fixtures/nested.toml")).expect("the nested fixture parses")
});

/// Walks `typed` (command word and all) against `table` and answers
/// the stop point's row labels — the shape every walker-table
/// assertion reads. `None` is no entry for the command word, or a
/// command-restart onto an unknown entry — silence, never a guess.
fn row_labels(typed: &str, table: &[SpecCommand]) -> Option<Vec<String>> {
    let tokens = lex(typed);
    let entry = table.iter().find(|command| {
        command.name == tokens[0].text
            || command.aliases.iter().any(|alias| *alias == tokens[0].text)
    })?;
    let stop = walk(&tokens[1..], entry, table)?;
    Some(stop.rows().into_iter().map(|row| row.label).collect())
}

/// The same walk's row suffixes — what an accept writes for each row.
fn row_suffixes(typed: &str, table: &[SpecCommand]) -> Option<Vec<String>> {
    let tokens = lex(typed);
    let entry = table.iter().find(|command| {
        command.name == tokens[0].text
            || command.aliases.iter().any(|alias| *alias == tokens[0].text)
    })?;
    let stop = walk(&tokens[1..], entry, table)?;
    Some(stop.rows().into_iter().map(|row| row.suffix).collect())
}

#[test]
fn per_subcommand_flags_answer_their_own_node() {
    // §7.1: `git commit --am` lists commit's flags, not git's whole
    // flag table — a non-persistent root flag never rides down, and a
    // persistent one does.
    let trees = &*TREES;
    assert_eq!(
        row_labels("git commit --am", trees),
        Some(vec!["--amend".to_string()])
    );
    assert_eq!(
        row_labels("git commit --", trees),
        Some(
            ["--amend", "--no-edit", "--no-pager"]
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>()
        ),
        "the persistent root flag rides; --version does not"
    );
    assert_eq!(
        row_labels("git --", trees),
        Some(
            ["--no-pager", "--version"]
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>()
        )
    );
}

#[test]
fn a_flags_argument_takes_the_value_seat() {
    // §7.2: after `--emit` or `--emit=` the next seat is the option's
    // argument — its literal suggestions, never another flag.
    let trees = &*TREES;
    assert_eq!(
        row_labels("rustc --emit=", trees),
        Some(
            ["asm", "link", "metadata", "mir"]
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>()
        ),
        "the = form answers the value seat with the whole vocabulary"
    );
    assert_eq!(
        row_labels("rustc --emit=a", trees),
        Some(vec!["asm".to_string()])
    );
    assert_eq!(
        row_labels("rustc --emit lin", trees),
        Some(vec!["link".to_string()]),
        "the space form is the same seat"
    );
    // The value seat never writes an `=` — the suffix is the bare
    // remainder of the row's name beyond the partial.
    assert_eq!(
        row_suffixes("rustc --emit=a", trees),
        Some(vec!["sm".to_string()])
    );
    // A flag's argument with no suggestions is silence: `mkdir -m `
    // lists no flags, no subcommands, nothing.
    assert_eq!(row_labels("mkdir -m ", trees), Some(Vec::new()));
}

#[test]
fn nested_subcommands_descend_to_any_depth() {
    // §7.3: `docker compose up`, `gh pr create` — the walk descends
    // one node per complete token, to any depth.
    let trees = &*TREES;
    let nested = &*NESTED;
    assert_eq!(
        row_labels("docker compose u", nested),
        Some(vec!["up".to_string()])
    );
    assert_eq!(
        row_labels("gh pr cre", trees),
        Some(vec!["create".to_string()])
    );
}

#[test]
fn persistent_flags_ride_into_children() {
    // §7.4: `git --no-pager log --one` still finds `--oneline` — the
    // accepted persistent flag rode through the descent.
    let trees = &*TREES;
    assert_eq!(
        row_labels("git --no-pager log --one", trees),
        Some(vec!["--oneline".to_string()])
    );
}

#[test]
fn variadic_args_keep_the_path_seat_until_a_flag_appears() {
    // §7.5: a variadic filepaths arg keeps handing the seat to the
    // Path provider (no spec rows) until a flag-shaped word appears —
    // then the node's flags answer.
    let trees = &*TREES;
    assert_eq!(
        row_labels("git add a b src/", trees),
        Some(Vec::new()),
        "the template arg answers no spec rows — the Path provider's seat"
    );
    assert_eq!(
        row_labels("git add src/ --", trees),
        Some(
            ["--force", "--no-pager", "--patch"]
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>()
        ),
        "the flag breaks the variadic and takes the seat (the persistent root flag rides)"
    );
}

#[test]
fn optional_args_retry_flags_and_suggestions_first() {
    // The optional re-try (engine-rea §3): when every remaining arg is
    // optional, a flag word is tried as a flag — and at a node stop
    // the first arg's literal suggestions answer word partials beside
    // the node's own names.
    let nested = &*NESTED;
    assert_eq!(
        row_labels("docker compose up --det", nested),
        Some(vec!["--detach".to_string()]),
        "the optional arg yields to a flag"
    );
    assert_eq!(
        row_labels("docker compose up we", nested),
        Some(vec!["web".to_string()]),
        "the service arg's suggestions answer the word partial"
    );
}

#[test]
fn a_command_arg_restarts_the_walk_on_its_own_entry() {
    // §7.6: the sudo shape — a `command` arg restarts the walk on the
    // named command's own spec; a command with no entry is silence,
    // never a guess.
    let trees = &*TREES;
    assert_eq!(
        row_labels("sudo apt ins", trees),
        Some(vec!["install".to_string()])
    );
    assert_eq!(
        row_labels("sudo frobnicate ins", trees),
        None,
        "the restart onto an unknown command silences"
    );
}

#[test]
fn an_unknown_flag_mid_line_is_skipped_as_a_bare_flag() {
    // The locked tolerance: the flat seats never inspected
    // intermediate tokens, and `git --whatever commit --am` keeps
    // answering — an unknown flag is skipped, not fatal.
    let trees = &*TREES;
    assert_eq!(
        row_labels("git --whatever commit --am", trees),
        Some(vec!["--amend".to_string()])
    );
}

#[test]
fn a_word_the_node_does_not_know_is_skipped_when_no_args() {
    // Today's flat tolerance kept: a word that names neither flag nor
    // subcommand at an arg-less node is skipped and the walk stays.
    let trees = &*TREES;
    assert_eq!(
        row_labels("git xyz commit --am", trees),
        Some(vec!["--amend".to_string()])
    );
}

#[test]
fn the_root_entry_answers_by_name_or_alias() {
    // The entry lookup honors the table's own aliases (the engine's
    // root prefix-match over names plus alias names).
    let nested = &*NESTED;
    assert_eq!(
        row_labels("dock compose u", nested),
        Some(vec!["up".to_string()])
    );
}

#[test]
fn a_depleted_positional_silences_the_later_seats() {
    // The engine's slots-full rule (§4.1): once a node's own
    // positional args are exhausted by values, later seats answer
    // nothing — the positional slots are full. A flag's argument
    // depleting does not: names still answer.
    let trees = &*TREES;
    assert_eq!(
        row_labels("flit one bu", trees),
        Some(Vec::new()),
        "the one positional consumed by `one` — slots full, silence"
    );
    assert_eq!(
        row_labels("flit bu", trees),
        Some(vec!["build".to_string()]),
        "no positional consumed — the subcommands answer"
    );
    let nested = &*NESTED;
    assert_eq!(
        row_labels("docker --config x exe", nested),
        Some(vec!["exec".to_string()]),
        "a flag's argument depleting keeps the names answering"
    );
}
