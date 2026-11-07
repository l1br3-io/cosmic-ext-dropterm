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
