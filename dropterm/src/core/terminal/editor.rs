// SPDX-License-Identifier: GPL-3.0-only

//! The rich-input editor's pure model (rich-input spec, RI-2): the
//! owned line's buffer, cursor, history navigation, the signal
//! decisions, verbatim paste, and the show/hide machine. Headless by
//! design — no gpui, no PTY, no I/O; the engine feeds it gates and a
//! history slice (RI-3), the strip paints its state (RI-4).
//!
//! Behavior copied is Warp's, behavior-only (the
//! warp-editor-behavior record): the chord surface is readline's
//! (the router translates chords into [`Key`] — Ctrl+B/F/N/P/H and
//! Ctrl+A/E land on their arrow/Home/End twins), kills delete
//! without a kill ring (yank is not in the v1 table), and undo is
//! buffer-scoped — a committed (executed) line is never undoable.

/// A keystroke in the editor's own vocabulary — the canonical form
/// the router translates every chord into (one [`Key`] per v1 table
/// row; chord synonyms collapse here, never in the model).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A printable insert.
    Char(char),
    /// Delete left — Backspace and Ctrl+H.
    Backspace,
    /// Delete right — Delete and non-empty Ctrl+D.
    Delete,
    /// Left / Ctrl+B.
    Left,
    /// Right / Ctrl+F.
    Right,
    /// Up / Ctrl+P — history: one entry into the past.
    Up,
    /// Down / Ctrl+N — history: one entry back toward the present.
    Down,
    /// Home / Ctrl+A.
    Home,
    /// End / Ctrl+E.
    End,
    /// Alt+Left — a word left.
    WordLeft,
    /// Alt+Right — a word right.
    WordRight,
    /// Ctrl+W — kill the word left.
    KillWordLeft,
    /// Alt+D — kill the word right.
    KillWordRight,
    /// Ctrl+U — kill to the start.
    KillLineLeft,
    /// Ctrl+K — kill to the end.
    KillLineRight,
    /// Ctrl+L.
    ClearScreen,
    /// Shift+Enter / Ctrl+J — a real newline in the buffer.
    Newline,
    /// Enter — run the buffer.
    Enter,
    /// Ctrl+C.
    Interrupt,
    /// Ctrl+D on an empty buffer.
    EndOfFile,
    /// Ctrl+Z.
    Undo,
    /// Ctrl+Shift+Z.
    Redo,
}

/// The shell-bound signals the editor can ask the engine to send —
/// the empty-buffer cases where the owned line has nothing to clear
/// or delete, plus the screen clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// The ^C byte (empty-line Ctrl+C).
    Interrupt,
    /// The ^D byte (empty-buffer Ctrl+D).
    EndOfFile,
    /// Ctrl+L.
    ClearScreen,
}

/// The editor's verdict for a keystroke — the one enum the router
/// needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Buffer changed / cursor moved; no PTY byte.
    Handled,
    /// Enter: the engine PTY-writes the line plus a `'\n'` byte (see
    /// [`Verdict::pty_write`]).
    Execute {
        /// The whole buffer at Enter — interior newlines included
        /// (a pasted block runs as one write).
        line: String,
    },
    /// A shell-bound signal the engine forwards.
    Signal(Signal),
    /// Not the editor's: the keystroke forwards verbatim.
    Passthrough,
}

impl Verdict {
    /// The exact bytes an [`Verdict::Execute`] verdict PTY-writes —
    /// the line plus one newline byte. Every other verdict writes
    /// nothing.
    #[must_use]
    pub fn pty_write(&self) -> Option<String> {
        match self {
            Self::Execute { line } => Some(format!("{line}\n")),
            _ => None,
        }
    }
}

/// One editor-lane menu press's outcome (rich-input RI-5): the grid
/// lane's menu outcome with the PTY write replaced by the insert the
/// pane owes the buffer — the editor owns the line, so an accept or
/// fill gains text there, never bytes here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuPress {
    /// Navigation or a close: the buffer is untouched.
    Noop,
    /// The suffix an accept or fill gained — the pane inserts it at
    /// the caret, then steps the suggest lane.
    Insert(String),
    /// The two-stage Enter's equal line (suggest-parity spec): the
    /// menu closed, nothing was inserted — the line runs.
    Execute,
}

/// The owned line's token classes (rich-input RI-6): the mock's
/// painted set — command (known vs unknown), flag, string — plus the
/// record's finding-6 ceiling's variable class; `Word` is the plain
/// default everything else lands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenClass {
    /// The line's command position: the name the known-command
    /// predicate answers for (the unknown answer carries the
    /// record's dashed-red underline at paint).
    Command { known: bool },
    /// A `-`-prefixed option.
    Flag,
    /// A quoted token.
    String,
    /// A `$`-prefixed variable reference.
    Variable,
    /// Everything else, painted plain.
    Word,
}

/// One highlighted slice of the owned line — a byte range with its
/// class (the ranges tile the line's tokens; the whitespace between
/// stays unpainted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    /// The token's byte range in the line.
    pub range: std::ops::Range<usize>,
    /// The token's class.
    pub class: TokenClass,
}

/// Tokenize the owned line for highlight (rich-input RI-6): one
/// pass, whitespace-separated tokens, each line of a multi-line
/// buffer restarting at the command position (a pasted block's
/// second line is a command too). The known-command predicate is
/// injected — the paint consumer decides what "known" means (the
/// engine's specs-and-catalog rule).
#[must_use]
pub fn highlight_spans(line: &str, command_known: impl Fn(&str) -> bool) -> Vec<HighlightSpan> {
    let mut spans = Vec::new();
    let mut token_index_on_line = 0;
    let mut token_start: Option<usize> = None;
    for (offset, ch) in line.char_indices() {
        if ch.is_whitespace() {
            if let Some(start) = token_start.take() {
                spans.push(classify_span(
                    line,
                    start,
                    offset,
                    token_index_on_line,
                    &command_known,
                ));
                token_index_on_line += 1;
            }
            if ch == '\n' {
                token_index_on_line = 0;
            }
        } else if token_start.is_none() {
            token_start = Some(offset);
        }
    }
    if let Some(start) = token_start {
        spans.push(classify_span(
            line,
            start,
            line.len(),
            token_index_on_line,
            &command_known,
        ));
    }
    spans
}

/// One token's class: the mock's rules (finding 6's shape) — the
/// line's first token starting `[a-z_]` is the command, `-` a flag,
/// quotes a string — plus `$` a variable.
fn classify_span(
    line: &str,
    start: usize,
    end: usize,
    token_index_on_line: usize,
    command_known: &impl Fn(&str) -> bool,
) -> HighlightSpan {
    let token = &line[start..end];
    let class = if token_index_on_line == 0
        && token.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
    {
        TokenClass::Command {
            known: command_known(token),
        }
    } else if token.starts_with('-') {
        TokenClass::Flag
    } else if token.starts_with('$') {
        TokenClass::Variable
    } else if token.starts_with(['"', '\'']) || token.ends_with(['"', '\'']) {
        TokenClass::String
    } else {
        TokenClass::Word
    };
    HighlightSpan {
        range: start..end,
        class,
    }
}

/// What the grid-read gates concluded about the prompt (rich-input
/// OQ-4): `Live` is a prompt the gates can see; `Unsure` is anything
/// they cannot decide — and unsure hides (a wrongly-shown editor
/// eats a program's input; a hidden one only costs richness).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptDetection {
    Live,
    Unsure,
}

/// The live-prompt gates the engine feeds the model (today's ghost
/// gates, restated as contract): all four must hold for the editor
/// to own the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gates {
    /// The surface holds keyboard focus.
    pub focused: bool,
    /// No foreground child owns the screen (the `tcgetpgrp` check).
    pub no_foreground_child: bool,
    /// The viewport sits at the bottom of the scrollback.
    pub at_bottom: bool,
    /// What the grid read concluded about the prompt.
    pub prompt_detection: PromptDetection,
}

/// The show/hide machine's decision: whether the editor owns the
/// line under these gates (every gate must hold; unsure hides).
#[must_use]
pub fn owns_line(gates: &Gates) -> bool {
    gates.focused
        && gates.no_foreground_child
        && gates.at_bottom
        && gates.prompt_detection == PromptDetection::Live
}

/// The owned line's editor state: the buffer with a char-index
/// cursor, the history slice (newest-first, the caller's ordering —
/// the store's context ranking is applied before it reaches here),
/// the in-flight draft history navigation preserves, and the
/// buffer-scoped undo/redo stacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    /// The owned line's full text — interior newlines included once
    /// newline keys or a paste put them there.
    text: String,
    /// The cursor's position, in chars from the buffer's start.
    cursor: usize,
    /// The history slice, newest-first.
    history: Vec<String>,
    /// Where navigation stands: `None` is the live (draft) line,
    /// `Some(i)` is `history[i]`.
    history_position: Option<usize>,
    /// The live line as it stood when navigation left it — what Down
    /// walks back to.
    draft: String,
    undo: Vec<String>,
    redo: Vec<String>,
    /// Whether consecutive `Char` inserts are coalescing into one
    /// undo step (a text editor's typing run; any other key breaks
    /// it, and breaking it is what starts the next step's snapshot).
    typing_run: bool,
}

impl Editor {
    /// An editor over a history slice, newest-first (the caller
    /// applies the store's ranking; the model only walks the order
    /// it is given).
    #[must_use]
    pub fn new(history: Vec<String>) -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            history,
            history_position: None,
            draft: String::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            typing_run: false,
        }
    }

    /// The buffer's full text.
    #[must_use]
    pub fn line(&self) -> &str {
        &self.text
    }

    /// The cursor's position, in chars from the buffer's start.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// A keystroke's verdict. Pure: the only effects are on self.
    pub fn key(&mut self, key: Key) -> Verdict {
        match key {
            Key::Char(ch) => {
                self.edit(true, |editor| {
                    editor.insert(ch);
                });
                Verdict::Handled
            }
            Key::Newline => {
                self.edit(false, |editor| {
                    editor.insert('\n');
                });
                Verdict::Handled
            }
            Key::Backspace => {
                self.edit(false, |editor| {
                    if editor.cursor > 0 {
                        editor.cursor -= 1;
                        editor.remove_at(editor.cursor);
                    }
                });
                Verdict::Handled
            }
            Key::Delete => {
                self.edit(false, |editor| {
                    if editor.cursor < editor.char_len() {
                        editor.remove_at(editor.cursor);
                    }
                });
                Verdict::Handled
            }
            Key::Left => {
                self.typing_run = false;
                self.cursor = self.cursor.saturating_sub(1);
                Verdict::Handled
            }
            Key::Right => {
                self.typing_run = false;
                if self.cursor < self.char_len() {
                    self.cursor += 1;
                }
                Verdict::Handled
            }
            Key::Home => {
                self.typing_run = false;
                self.cursor = 0;
                Verdict::Handled
            }
            Key::End => {
                self.typing_run = false;
                self.cursor = self.char_len();
                Verdict::Handled
            }
            Key::WordLeft => {
                self.typing_run = false;
                self.cursor = self.word_left_of(self.cursor);
                Verdict::Handled
            }
            Key::WordRight => {
                self.typing_run = false;
                self.cursor = self.word_right_of(self.cursor);
                Verdict::Handled
            }
            Key::KillWordLeft => {
                self.edit(false, |editor| {
                    let start = editor.word_left_of(editor.cursor);
                    editor.drain(start, editor.cursor);
                    editor.cursor = start;
                });
                Verdict::Handled
            }
            Key::KillWordRight => {
                self.edit(false, |editor| {
                    let end = editor.word_right_of(editor.cursor);
                    editor.drain(editor.cursor, end);
                });
                Verdict::Handled
            }
            Key::KillLineLeft => {
                self.edit(false, |editor| {
                    editor.drain(0, editor.cursor);
                    editor.cursor = 0;
                });
                Verdict::Handled
            }
            Key::KillLineRight => {
                self.edit(false, |editor| {
                    let len = editor.char_len();
                    editor.drain(editor.cursor, len);
                });
                Verdict::Handled
            }
            Key::Up => {
                self.typing_run = false;
                self.recall(Recall::Older)
            }
            Key::Down => {
                self.typing_run = false;
                self.recall(Recall::Newer)
            }
            Key::Undo => {
                self.typing_run = false;
                if let Some(prior) = self.undo.pop() {
                    self.redo.push(std::mem::take(&mut self.text));
                    self.adopt(prior);
                }
                Verdict::Handled
            }
            Key::Redo => {
                self.typing_run = false;
                if let Some(later) = self.redo.pop() {
                    self.undo.push(std::mem::take(&mut self.text));
                    self.adopt(later);
                }
                Verdict::Handled
            }
            Key::Enter => {
                self.typing_run = false;
                let line = std::mem::take(&mut self.text);
                self.cursor = 0;
                self.history_position = None;
                self.draft.clear();
                // A committed line is never undoable (the record's
                // rejected alternative made the rule): the stacks die
                // with the buffer they described.
                self.undo.clear();
                self.redo.clear();
                Verdict::Execute { line }
            }
            Key::Interrupt => {
                if self.text.is_empty() {
                    Verdict::Signal(Signal::Interrupt)
                } else {
                    self.edit(false, |editor| {
                        editor.text.clear();
                        editor.cursor = 0;
                        editor.history_position = None;
                        editor.draft.clear();
                    });
                    Verdict::Handled
                }
            }
            Key::EndOfFile => {
                if self.text.is_empty() {
                    Verdict::Signal(Signal::EndOfFile)
                } else {
                    self.key(Key::Delete)
                }
            }
            Key::ClearScreen => Verdict::Signal(Signal::ClearScreen),
        }
    }

    /// Paste: insert the clipboard text verbatim — newlines included
    /// — at the cursor. Paste never executes; only Enter runs (the
    /// record's finding 4).
    pub fn paste(&mut self, text: &str) {
        self.edit(false, |editor| {
            let at = editor.byte_of(editor.cursor);
            editor.text.insert_str(at, text);
            editor.cursor += text.chars().count();
        });
    }

    // --- the machinery, all char-indexed ---

    /// Run a buffer mutation under the undo discipline: snapshot
    /// before (a coalescing keystroke joins the open typing run
    /// instead), clear the redo stack (a fresh edit forks history),
    /// and leave navigation on the live line — an edit while
    /// recalled adopts the recalled text as the working line.
    fn edit<T>(&mut self, coalesce: bool, mutate: impl FnOnce(&mut Self) -> T) -> T {
        if !coalesce || !self.typing_run {
            self.undo.push(self.text.clone());
            if self.undo.len() > UNDO_CAP {
                self.undo.remove(0);
            }
        }
        self.typing_run = coalesce;
        self.redo.clear();
        self.history_position = None;
        self.draft.clear();
        mutate(self)
    }

    /// Adopt a snapshot as the buffer, clamping the cursor into it.
    fn adopt(&mut self, text: String) {
        self.cursor = self.cursor.min(text.chars().count());
        self.text = text;
    }

    /// Recall one entry over the navigation lane. Entering navigation
    /// from the live line saves the draft; walking past the newest
    /// entry returns to it; past the oldest entry navigation holds —
    /// the lane never leaves the slice on its own. The cursor lands
    /// at the recalled text's end (readline's own placement), so an
    /// edit extends the recall instead of splicing into its start.
    fn recall(&mut self, direction: Recall) -> Verdict {
        match direction {
            Recall::Older => {
                let next = match self.history_position {
                    None => (!self.history.is_empty()).then_some(0),
                    Some(index) => {
                        let older = index + 1;
                        (older < self.history.len()).then_some(older)
                    }
                };
                let Some(next) = next else {
                    return Verdict::Handled;
                };
                if self.history_position.is_none() {
                    self.draft = self.text.clone();
                }
                self.history_position = Some(next);
                let recalled = self.history[next].clone();
                self.adopt(recalled);
                self.cursor = self.char_len();
            }
            Recall::Newer => match self.history_position {
                // At the live line nothing newer exists: a no-op that
                // cannot clobber the line with a consumed draft.
                None => {}
                Some(0) => {
                    self.history_position = None;
                    let draft = std::mem::take(&mut self.draft);
                    self.adopt(draft);
                    self.cursor = self.char_len();
                }
                Some(index) => {
                    let newer = index - 1;
                    self.history_position = Some(newer);
                    let recalled = self.history[newer].clone();
                    self.adopt(recalled);
                    self.cursor = self.char_len();
                }
            },
        }
        Verdict::Handled
    }

    /// Whether navigation stands on the live line (rich-input
    /// RI-5): the history slice's refresh window — the pane feeds
    /// the store fresh at each entry, never mid-navigation.
    #[must_use]
    pub fn at_live_line(&self) -> bool {
        self.history_position.is_none()
    }

    /// Swap the history slice (rich-input RI-5): the pane feeds the
    /// store's newest-first commands here, read fresh at each
    /// navigation entry (the first Up of a line) — navigation in
    /// flight is never disturbed (the caller swaps only from the
    /// live line).
    pub fn set_history(&mut self, history: Vec<String>) {
        self.history = history;
    }

    /// Whether the caret sits at the buffer's end — the ghost's
    /// accept gate (the ghost paints after the buffer; a mid-line
    /// Right moves).
    #[must_use]
    pub fn caret_at_end(&self) -> bool {
        self.cursor == self.char_len()
    }

    /// The buffer's text up to the caret, and whether the caret ends
    /// the line — the lane's step inputs ([`crate::core::terminal::
    /// Terminal::editor_step`]).
    #[must_use]
    pub fn step_inputs(&self) -> (&str, bool) {
        let byte = self.byte_of(self.cursor);
        (&self.text[..byte], byte == self.text.len())
    }

    /// The buffer's length in chars.
    fn char_len(&self) -> usize {
        self.text.chars().count()
    }

    /// The byte offset of a char index.
    fn byte_of(&self, char_index: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_index)
            .map_or(self.text.len(), |(byte, _)| byte)
    }

    /// Insert one char at the cursor.
    fn insert(&mut self, ch: char) {
        let at = self.byte_of(self.cursor);
        self.text.insert(at, ch);
        self.cursor += 1;
    }

    /// Remove the char at a char index.
    fn remove_at(&mut self, char_index: usize) {
        let at = self.byte_of(char_index);
        self.text.remove(at);
    }

    /// Remove the char range `[start, end)` (char indices).
    fn drain(&mut self, start: usize, end: usize) {
        if start < end {
            let from = self.byte_of(start);
            let to = self.byte_of(end);
            self.text.replace_range(from..to, "");
        }
    }

    /// The start of the word left of a char index — skip whitespace
    /// leftward, then the word's own run (readline's unix-word:
    /// words are maximal non-whitespace runs, Ctrl+W's own rule).
    fn word_left_of(&self, from: usize) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut index = from;
        while index > 0 && chars[index - 1].is_whitespace() {
            index -= 1;
        }
        while index > 0 && !chars[index - 1].is_whitespace() {
            index -= 1;
        }
        index
    }

    /// The end of the word right of a char index.
    fn word_right_of(&self, from: usize) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let len = chars.len();
        let mut index = from;
        while index < len && chars[index].is_whitespace() {
            index += 1;
        }
        while index < len && !chars[index].is_whitespace() {
            index += 1;
        }
        index
    }
}

/// The recall direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recall {
    Older,
    Newer,
}

/// The undo stacks' entry cap — a line editor's history is short;
/// the cap only stops an unbounded session from growing without
/// bound.
const UNDO_CAP: usize = 100;

#[cfg(test)]
mod tests;
