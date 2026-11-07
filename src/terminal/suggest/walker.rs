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

use super::{MenuCandidate, SpecArg, SpecCommand, SpecFlag, SuggestionSource, escape_for_shell};

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

/// Where the walk stopped (suggest-walker spec, engine-rea §4): the
/// node the line sits at, the trailing partial (owned — the stop
/// point outlives the tokens it came from), the persistent flags
/// carried into the node, the flags accepted on the line (dressed out
/// of the rows at Task 3's step), the argument whose value seat the
/// stop occupies — the one being consumed, or the node's first
/// unconsumed positional at a node stop — whether that argument is
/// the stop's whole answer (a value seat lists its suggestions
/// alone), and whether the node's own positional slots are full.
pub(super) struct StopPoint<'s> {
    node: &'s SpecCommand,
    partial: String,
    persistent: Vec<&'s SpecFlag>,
    /// The walk's dressing input, landed ahead of its reader (the
    /// same module run): Task 3's step reads it out of the rows.
    #[expect(
        dead_code,
        reason = "the row dressing (Task 3 of this module run) consumes it"
    )]
    accepted: Vec<&'s SpecFlag>,
    active_arg: Option<&'s SpecArg>,
    arg_driven: bool,
    positional_slots_full: bool,
}

impl StopPoint<'_> {
    /// The trailing partial the surfaces filter on — the incomplete
    /// token's unwrapped text, or empty at a fresh node stop (the
    /// empty-word rule: a trailing space is no word at all).
    pub(super) fn partial(&self) -> &str {
        &self.partial
    }

    /// The stop point's candidate rows (engine-rea §4): a `-`-shaped
    /// partial is the flag seat — the node's flags plus the persistent
    /// ones carried in; a value seat (the walk stopped inside argument
    /// consumption) lists the argument's literal suggestions alone —
    /// a template arg answers nothing, the Path provider's seat; a
    /// node stop lists the subcommands, the flags once a `-` partial
    /// discriminates (an empty partial lists the scoped next level
    /// alone), and the first unconsumed positional's suggestions.
    /// Every row extends the partial (strict prefix, must extend),
    /// every suffix is the escaped remainder an accept writes, and
    /// positional slots already full answer nothing. Ordered
    /// names-first — Fig's own 50/45 default split, the priority knob
    /// rides on top at Task 3's step — and deduplicated by label.
    pub(super) fn rows(&self) -> Vec<MenuCandidate> {
        if self.positional_slots_full {
            return Vec::new();
        }
        let partial = self.partial.as_str();
        let mut rows = Vec::new();
        if partial.starts_with('-') {
            self.flag_rows(partial, &mut rows);
        } else if self.arg_driven {
            if let Some(arg) = self.active_arg {
                arg_rows(arg, partial, &mut rows);
            }
        } else {
            self.subcommand_rows(partial, &mut rows);
            if !partial.is_empty() {
                self.flag_rows(partial, &mut rows);
            }
            if let Some(arg) = self.active_arg {
                arg_rows(arg, partial, &mut rows);
            }
        }
        rows.sort_by(|a, b| menu_order(a).cmp(&menu_order(b)));
        rows.dedup_by(|a, b| a.label == b.label);
        rows
    }

    /// The node's subcommands as candidate rows.
    fn subcommand_rows(&self, partial: &str, rows: &mut Vec<MenuCandidate>) {
        entry_rows(
            partial,
            self.node
                .subcommands
                .iter()
                .map(|command| (&command.name, command.description.as_deref())),
            rows,
        );
    }

    /// The flag seat's rows: the node's own flags plus the persistent
    /// ones carried in — a carried flag shadowed by an own flag of the
    /// same name lists once (the engine's dedup-by-name).
    fn flag_rows(&self, partial: &str, rows: &mut Vec<MenuCandidate>) {
        entry_rows(
            partial,
            self.node
                .flags
                .iter()
                .map(|flag| (&flag.name, flag.description.as_deref())),
            rows,
        );
        for flag in &self.persistent {
            if self.node.flags.iter().any(|own| own.name == flag.name) {
                continue;
            }
            if let Some(row) = entry_row(partial, &flag.name, flag.description.as_deref()) {
                rows.push(row);
            }
        }
    }
}

/// The value seat's rows: the active argument's literal suggestions —
/// a template arg contributes none of its own (the Path provider owns
/// that seat).
fn arg_rows(arg: &SpecArg, partial: &str, rows: &mut Vec<MenuCandidate>) {
    if arg.template.is_some() {
        return;
    }
    entry_rows(
        partial,
        arg.suggestions
            .iter()
            .map(|entry| (&entry.name, entry.description.as_deref())),
        rows,
    );
}

/// One candidate's row order: non-flag names first, then flags,
/// alphabetical within each — Fig's own default priorities (50 for
/// names, 45 for options) ahead of the spec-declared knob.
fn menu_order(row: &MenuCandidate) -> (bool, &str) {
    (row.label.starts_with('-'), row.label.as_str())
}

/// One name/description list's rows: every entry that extends the
/// partial, dressed as specs-provenance rows with the escaped
/// remainder as the suffix.
fn entry_rows<'a, 'd>(
    partial: &str,
    entries: impl Iterator<Item = (&'a String, Option<&'d str>)>,
    rows: &mut Vec<MenuCandidate>,
) {
    for (name, description) in entries {
        if let Some(row) = entry_row(partial, name, description) {
            rows.push(row);
        }
    }
}

/// One entry's row — `None` when it does not extend the partial or
/// its remainder cannot be safely written (silence, never a wrong
/// suffix).
fn entry_row(partial: &str, name: &str, description: Option<&str>) -> Option<MenuCandidate> {
    if !name.starts_with(partial) || name.len() <= partial.len() {
        return None;
    }
    Some(MenuCandidate {
        label: name.to_string(),
        suffix: escape_for_shell(&name[partial.len()..])?,
        description: description.map(str::to_string),
        source: SuggestionSource::Specs,
    })
}

/// Walks the complete tokens into the tree and stops at the first
/// incomplete or absent one (engine-rea §3). Returns `None` only when
/// a `command` arg restarts the walk on a name with no entry — an
/// unknown command is silence, never a guess.
pub(super) fn walk<'s>(
    tokens: &[Token<'_>],
    root: &'s SpecCommand,
    table: &'s [SpecCommand],
) -> Option<StopPoint<'s>> {
    run_subcommand(root, tokens, Walk::new(table), false)
}

/// The walk's carried state (engine-rea §3): the table a `command`
/// arg restarts on, the persistent flags riding in, and the flags
/// accepted on the line — the stop point's dressing input.
struct Walk<'s> {
    table: &'s [SpecCommand],
    persistent: Vec<&'s SpecFlag>,
    accepted: Vec<&'s SpecFlag>,
}

impl<'s> Walk<'s> {
    /// Fresh state at a root entry: nothing carried, nothing accepted.
    fn new(table: &'s [SpecCommand]) -> Self {
        Self {
            table,
            persistent: Vec::new(),
            accepted: Vec::new(),
        }
    }

    /// Crosses into a subcommand (`runtime.ts:417-418`): a persistent
    /// flag lives on its command and every descendant, so the parent's
    /// own persistent flags join the carried set here (deduplicated by
    /// name, ancestors riding transitively), and only the persistent
    /// acceptances ride — a plain accepted flag does not dress rows
    /// inside the child.
    fn descend(&mut self, parent: &'s SpecCommand) {
        for flag in &parent.flags {
            if flag.persistent
                && !self
                    .persistent
                    .iter()
                    .any(|carried| carried.name == flag.name)
            {
                self.persistent.push(flag);
            }
        }
        self.accepted.retain(|flag| flag.persistent);
    }
}

/// At a subcommand node with the remaining tokens (engine-rea §3,
/// `runtime.ts:378-435`): an incomplete or absent first token stops at
/// the node; a known flag — the node's plus the persistent carried
/// in, by name or alias — hands off to [`run_option`]; an unknown
/// flag is skipped as a bare flag (the locked tolerance: the flat
/// seats never inspected intermediate tokens, so `git --whatever
/// commit --am` keeps answering); a subcommand's name or alias
/// descends; any other token is the node's arguments' to consume, or
/// — at an arg-less node — skipped and the walk stays (today's flat
/// tolerance, kept).
fn run_subcommand<'s>(
    node: &'s SpecCommand,
    mut tokens: &[Token<'_>],
    mut state: Walk<'s>,
    positional_slots_full: bool,
) -> Option<StopPoint<'s>> {
    loop {
        let Some(first) = tokens.first() else {
            return Some(node_stop(node, "", state, positional_slots_full));
        };
        if !first.complete {
            return Some(node_stop(
                node,
                first.text.as_ref(),
                state,
                positional_slots_full,
            ));
        }
        let rest = &tokens[1..];
        if first.is_flag {
            if let Some(flag) = lookup_flag(first.text.as_ref(), node, &state.persistent) {
                return run_option(flag, rest, node, state);
            }
        } else if let Some(child) = lookup_subcommand(node, first.text.as_ref()) {
            state.descend(node);
            return run_subcommand(child, rest, state, false);
        } else if !node.args.is_empty() {
            return run_arg(node, &node.args, 0, false, tokens, state);
        }
        tokens = rest;
    }
}

/// A flag's step (engine-rea §3, `runtime.ts:264-299`): a flag that
/// declares arguments hands its tokens to [`run_arg`] as the value
/// seat; a bare flag is accepted — joining the accepted set the stop
/// point dresses out of its rows — and the walk continues at the
/// node.
fn run_option<'s>(
    flag: &'s SpecFlag,
    tokens: &[Token<'_>],
    node: &'s SpecCommand,
    mut state: Walk<'s>,
) -> Option<StopPoint<'s>> {
    if !flag.args.is_empty() {
        return run_arg(node, &flag.args, 0, true, tokens, state);
    }
    state.accepted.push(flag);
    run_subcommand(node, tokens, state, false)
}

/// An argument's consumption (engine-rea §3, `runtime.ts:301-376`):
/// args exhausted return to the node — the slots marked full only
/// when they were the node's own positionals, never a flag's; no
/// tokens, or an incomplete one, stops at the value seat; when every
/// remaining arg is optional the token is retried as a flag or a
/// subcommand first; a variadic consumes until a flag or a subcommand
/// appears — either breaks out and re-presents the token to the node;
/// a `command` arg restarts the walk on the named command's own entry
/// (the sudo shape); otherwise one token advances one arg.
fn run_arg<'s>(
    node: &'s SpecCommand,
    args: &'s [SpecArg],
    mut idx: usize,
    from_option: bool,
    mut tokens: &[Token<'_>],
    mut state: Walk<'s>,
) -> Option<StopPoint<'s>> {
    loop {
        let Some(arg) = args.get(idx) else {
            return run_subcommand(node, tokens, state, !from_option);
        };
        let Some(first) = tokens.first() else {
            return Some(arg_stop(node, "", arg, state));
        };
        if !first.complete {
            return Some(arg_stop(node, first.text.as_ref(), arg, state));
        }
        let rest = &tokens[1..];
        // The optional re-try: every remaining arg optional means the
        // token may still be a flag or a subcommand (`runtime.ts:325-348`).
        if args[idx..].iter().all(|arg| arg.optional) {
            if first.is_flag
                && let Some(flag) = lookup_flag(first.text.as_ref(), node, &state.persistent)
            {
                return run_option(flag, rest, node, state);
            }
            if let Some(child) = lookup_subcommand(node, first.text.as_ref()) {
                state.descend(node);
                return run_subcommand(child, rest, state, false);
            }
        }
        if arg.variadic {
            if first.is_flag {
                return run_subcommand(node, tokens, state, false);
            }
            if let Some(child) = lookup_subcommand(node, first.text.as_ref()) {
                state.descend(node);
                return run_subcommand(child, rest, state, false);
            }
            tokens = rest;
            continue;
        }
        if arg.command {
            let entry = state.table.iter().find(|command| {
                matches_entry(&command.name, &command.aliases, first.text.as_ref())
            })?;
            return run_subcommand(entry, rest, Walk::new(state.table), false);
        }
        tokens = rest;
        idx += 1;
    }
}

/// A stop at a node's own seat (engine-rea §4): the partial is the
/// trailing token's text — empty when the line ended in whitespace —
/// and the node's first unconsumed positional is the active argument
/// whose suggestions answer word partials beside the node's names.
fn node_stop<'s>(
    node: &'s SpecCommand,
    partial: &str,
    state: Walk<'s>,
    positional_slots_full: bool,
) -> StopPoint<'s> {
    let Walk {
        persistent,
        accepted,
        ..
    } = state;
    StopPoint {
        node,
        partial: partial.to_string(),
        persistent,
        accepted,
        active_arg: node.args.first(),
        arg_driven: false,
        positional_slots_full,
    }
}

/// A stop at a value seat (engine-rea §4): the argument being consumed
/// is the whole answer — its suggestions (or the Path provider's seat,
/// for a template), never the node's names.
fn arg_stop<'s>(
    node: &'s SpecCommand,
    partial: &str,
    arg: &'s SpecArg,
    state: Walk<'s>,
) -> StopPoint<'s> {
    let Walk {
        persistent,
        accepted,
        ..
    } = state;
    StopPoint {
        node,
        partial: partial.to_string(),
        persistent,
        accepted,
        active_arg: Some(arg),
        arg_driven: true,
        positional_slots_full: false,
    }
}

/// Finds a token's flag among a node's own flags then the persistent
/// ones carried in — by name or by alias (a typed short form is
/// recognized, never offered: the rows' short-form rule).
fn lookup_flag<'s>(
    text: &str,
    node: &'s SpecCommand,
    persistent: &[&'s SpecFlag],
) -> Option<&'s SpecFlag> {
    node.flags
        .iter()
        .chain(persistent.iter().copied())
        .find(|flag| matches_entry(&flag.name, &flag.aliases, text))
}

/// Finds a token's subcommand child by name or alias.
fn lookup_subcommand<'s>(node: &'s SpecCommand, text: &str) -> Option<&'s SpecCommand> {
    node.subcommands
        .iter()
        .find(|child| matches_entry(&child.name, &child.aliases, text))
}

/// Whether a table entry answers a typed word: its name, or one of
/// its aliases.
fn matches_entry(name: &str, aliases: &[String], text: &str) -> bool {
    name == text || aliases.iter().any(|alias| alias == text)
}

#[cfg(test)]
mod tests;
