// SPDX-License-Identifier: GPL-3.0-only

//! The spec walker (suggest-walker spec): the engine's lexer and
//! three-function walk over the [`SpecCommand`] tree, ported from
//! microsoft/inshellisense's runtime as recorded in
//! `docs/research/completions-engine-rea.md` §2–§4. Pure over tokens
//! and the tree — no I/O, no clock, no spawn — and the one source of
//! the stop point both surfaces consume: the menu lists its rows, the
//! ghost takes its single choice from the same rows under its
//! unchanged policy.

use std::borrow::Cow;

/// One lexed shell word (suggest-walker spec, engine-rea §2): the
/// text, whether whitespace closed it (`complete`), and whether it
/// opened with `-` (`is_flag`). `=` closes a flag token — `--emit=asm`
/// is `--emit` complete followed by `asm` — so the walker meets the
/// value as the flag's argument (inshellisense `parser.ts:105-112`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Token<'a> {
    pub(super) text: Cow<'a, str>,
    pub(super) complete: bool,
    pub(super) is_flag: bool,
}

/// The lexer's states (engine-rea §2, `parser.ts:62`): the quoted and
/// quote-continued states unwrap quotes, the flag state closes on `=`,
/// and the word state is everything else. Ground is the implicit
/// between-words state.
enum State {
    /// Between words: whitespace is skipped, and the next character
    /// decides which state — and token — opens.
    Ground,
    /// Inside an unquoted word.
    Word,
    /// Inside a `-`-opened flag.
    Flag,
    /// Inside quotes; the matching quote closes into [`State::QuoteTail`].
    Quoted { quote: char },
    /// After a closing quote: a non-space continues the same word
    /// (`"a"b` unwraps to `ab`), whitespace completes it.
    QuoteTail,
}

/// The token under construction: where it began in the command text —
/// the zero-copy path, a token no quote or escape touched borrows the
/// input whole — beside the unwrapped text once one did.
struct Open<'a> {
    command: &'a str,
    start: usize,
    text: Option<String>,
    is_flag: bool,
}

impl<'a> Open<'a> {
    /// One token opening at `start`; `unwrapped` marks the paths
    /// (quote-opened, escaped) whose text can never be a raw slice.
    fn new(command: &'a str, start: usize, is_flag: bool, unwrapped: bool) -> Self {
        Self {
            command,
            start,
            text: unwrapped.then(String::new),
            is_flag,
        }
    }

    /// Forces the owned path, seeding it with the raw bytes walked so
    /// far — the quote or escape at `at` never enters the text.
    fn unwrap_from(&mut self, at: usize) -> &mut String {
        self.text
            .get_or_insert_with(|| self.command[self.start..at].to_string())
    }

    /// Appends a literal character, on the owned path only — a token
    /// the unwrap never touched keeps borrowing the input whole.
    fn push(&mut self, c: char) {
        if let Some(text) = self.text.as_mut() {
            text.push(c);
        }
    }

    /// Closes the token at `end` (the closing character's offset, not
    /// included, or the text's end): the finished word.
    fn close_at(self, end: usize, complete: bool) -> Token<'a> {
        Token {
            text: match self.text {
                Some(text) => Cow::Owned(text),
                None => Cow::Borrowed(&self.command[self.start..end]),
            },
            complete,
            is_flag: self.is_flag,
        }
    }
}

/// Lexes the typed segment into shell words (engine-rea §2): the text
/// after the last `||`/`&&`/`;`/`|` is the command being completed;
/// four states (quoted, quote-continued, flag, word) walk its
/// characters; a token is `complete` only when whitespace (or `=`,
/// for a flag) follows it; the trailing unterminated token is the
/// partial. Quotes unwrap (`"git che"` is one word, `git che`), a
/// backslash escapes the next character (`my\ file` stays one word),
/// and `"a"b` continues to one unwrapped word. A trailing space
/// yields no partial — the empty-word rule both seats already ride.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "suggest-walker lands in tasks: the lexer's table first, the seats' wiring in Task 2 (the same module run)"
    )
)]
pub(super) fn lex(typed: &str) -> Vec<Token<'_>> {
    let command = match typed.rfind(['|', '&', ';']) {
        Some(idx) => &typed[idx + 1..],
        None => typed,
    };
    let mut tokens = Vec::new();
    let mut state = State::Ground;
    let mut open: Option<Open<'_>> = None;
    let mut chars = command.char_indices().peekable();
    while let Some((idx, c)) = chars.next() {
        // The one escape character, uniform across the in-word states:
        // the escaped character rides verbatim and the backslash never
        // enters the text (a space keeps the word open, a quote stays
        // literal, and at ground the escaped character opens the word).
        if c == '\\' {
            if let Some(&(_, escaped)) = chars.peek() {
                if matches!(state, State::Ground) {
                    let mut token = Open::new(command, idx, false, true);
                    token.unwrap_from(idx).push(escaped);
                    open = Some(token);
                    state = State::Word;
                } else if let Some(token) = open.as_mut() {
                    token.unwrap_from(idx).push(escaped);
                }
                chars.next();
            }
            continue;
        }
        match state {
            State::Ground => {
                if c.is_whitespace() {
                    continue;
                }
                match c {
                    '\'' | '"' | '`' => {
                        open = Some(Open::new(command, idx, false, true));
                        state = State::Quoted { quote: c };
                    }
                    '-' => {
                        open = Some(Open::new(command, idx, true, false));
                        state = State::Flag;
                    }
                    _ => {
                        open = Some(Open::new(command, idx, false, false));
                        state = State::Word;
                    }
                }
            }
            State::Word => match c {
                _ if c.is_whitespace() => {
                    tokens.push(
                        open.take()
                            .expect("a word state holds an open token")
                            .close_at(idx, true),
                    );
                    state = State::Ground;
                }
                '\'' | '"' | '`' => {
                    open.as_mut()
                        .expect("a word state holds an open token")
                        .unwrap_from(idx);
                    state = State::Quoted { quote: c };
                }
                _ => {
                    if let Some(token) = open.as_mut() {
                        token.push(c);
                    }
                }
            },
            State::Flag => match c {
                _ if c.is_whitespace() => {
                    tokens.push(
                        open.take()
                            .expect("a flag state holds an open token")
                            .close_at(idx, true),
                    );
                    state = State::Ground;
                }
                // `parser.ts:105`: `=` closes the flag (complete), and
                // the characters after it start a fresh word — the
                // walker meets them as the flag's argument.
                '=' => {
                    tokens.push(
                        open.take()
                            .expect("a flag state holds an open token")
                            .close_at(idx, true),
                    );
                    state = State::Ground;
                }
                _ => {
                    if let Some(token) = open.as_mut() {
                        token.push(c);
                    }
                }
            },
            State::Quoted { quote } => {
                if c == quote {
                    state = State::QuoteTail;
                } else if let Some(token) = open.as_mut() {
                    token.push(c);
                }
            }
            State::QuoteTail => match c {
                _ if c.is_whitespace() => {
                    tokens.push(
                        open.take()
                            .expect("a quote-tail state holds an open token")
                            .close_at(idx, true),
                    );
                    state = State::Ground;
                }
                '\'' | '"' | '`' => {
                    open.as_mut()
                        .expect("a quote-tail state holds an open token")
                        .unwrap_from(idx);
                    state = State::Quoted { quote: c };
                }
                _ => {
                    if let Some(token) = open.as_mut() {
                        token.push(c);
                    }
                    state = State::Word;
                }
            },
        }
    }
    // End of input: a token still open is the partial — the one
    // trailing unterminated word both seats filter on. Ground here
    // (the text ended in whitespace) holds none: the empty-word rule.
    if let Some(token) = open.take() {
        tokens.push(token.close_at(command.len(), false));
    }
    tokens
}

#[cfg(test)]
mod tests;
