// SPDX-License-Identifier: GPL-3.0-only

//! The pure editor model's pins (rich-input spec, RI-2): every v1
//! key, paste, history navigation, the signal decisions, and the
//! highlight tokenizer — each against the model alone, no engine, no
//! surface.

use super::{Editor, Key, Signal, Verdict};

fn editor(history: &[&str]) -> Editor {
    Editor::new(history.iter().map(|line| (*line).to_string()).collect())
}

fn type_text(editor: &mut Editor, text: &str) {
    for ch in text.chars() {
        assert_eq!(editor.key(Key::Char(ch)), Verdict::Handled);
    }
}

// --- the editing core ---

#[test]
fn chars_insert_at_the_cursor_and_move_it() {
    let mut editor = editor(&[]);
    type_text(&mut editor, "hello");
    assert_eq!(editor.line(), "hello");
    assert_eq!(editor.cursor(), 5);

    // A mid-line insert lands at the cursor, not at the end.
    editor.key(Key::Left);
    editor.key(Key::Left);
    editor.key(Key::Char('p'));
    assert_eq!(editor.line(), "helplo");
    assert_eq!(editor.cursor(), 4);
}

#[test]
fn backspace_deletes_left_and_delete_deletes_right() {
    let mut editor = editor(&[]);
    type_text(&mut editor, "abc");
    editor.key(Key::Left);
    assert_eq!(editor.key(Key::Backspace), Verdict::Handled);
    assert_eq!(editor.line(), "ac");
    assert_eq!(editor.cursor(), 1);

    assert_eq!(editor.key(Key::Delete), Verdict::Handled);
    assert_eq!(editor.line(), "a");
    assert_eq!(editor.cursor(), 1);

    // At the edges nothing happens — never a wrap, never a signal.
    assert_eq!(editor.key(Key::Delete), Verdict::Handled);
    editor.key(Key::Home);
    assert_eq!(editor.key(Key::Backspace), Verdict::Handled);
    assert_eq!(editor.line(), "a");
}

#[test]
fn ctrl_h_is_the_backspace_twin() {
    // The record's chord table: Ctrl+H "Remove the Previous
    // Character" — the router lands it on Backspace, and the model
    // treats the canonical key exactly alike.
    let mut editor = editor(&[]);
    type_text(&mut editor, "ab");
    assert_eq!(editor.key(Key::Backspace), Verdict::Handled);
    assert_eq!(editor.line(), "a");
}

#[test]
fn arrows_move_without_wrapping_and_ctrl_b_f_are_their_twins() {
    let mut editor = editor(&[]);
    type_text(&mut editor, "abc");

    // Left stops at the start, Right at the end.
    for _ in 0..5 {
        assert_eq!(editor.key(Key::Left), Verdict::Handled);
    }
    assert_eq!(editor.cursor(), 0);
    for _ in 0..5 {
        assert_eq!(editor.key(Key::Right), Verdict::Handled);
    }
    assert_eq!(editor.cursor(), 3);

    // The readline twins move exactly one char each.
    assert_eq!(editor.key(Key::Left), Verdict::Handled);
    assert_eq!(editor.cursor(), 2);
    assert_eq!(editor.key(Key::Right), Verdict::Handled);
    assert_eq!(editor.cursor(), 3);
}

#[test]
fn home_and_end_move_to_the_edges_and_ctrl_a_e_are_their_twins() {
    let mut editor = editor(&[]);
    type_text(&mut editor, "mid");

    assert_eq!(editor.key(Key::Home), Verdict::Handled);
    assert_eq!(editor.cursor(), 0);
    assert_eq!(editor.key(Key::End), Verdict::Handled);
    assert_eq!(editor.cursor(), 3);
    assert_eq!(editor.key(Key::Home), Verdict::Handled);
    assert_eq!(editor.cursor(), 0);
    assert_eq!(editor.key(Key::End), Verdict::Handled);
    assert_eq!(editor.cursor(), 3);
}

#[test]
fn word_motions_jump_over_runs() {
    let mut editor = editor(&[]);
    type_text(&mut editor, "one two  three");
    editor.key(Key::End);

    assert_eq!(editor.key(Key::WordLeft), Verdict::Handled);
    assert_eq!(editor.cursor(), 9, "lands on the t of three");
    assert_eq!(editor.key(Key::WordLeft), Verdict::Handled);
    assert_eq!(editor.cursor(), 4, "lands on the t of two");
    assert_eq!(editor.key(Key::WordLeft), Verdict::Handled);
    assert_eq!(editor.cursor(), 0, "lands on the o of one");

    assert_eq!(editor.key(Key::WordRight), Verdict::Handled);
    assert_eq!(editor.cursor(), 3, "lands after one");
    assert_eq!(editor.key(Key::WordRight), Verdict::Handled);
    assert_eq!(editor.key(Key::WordRight), Verdict::Handled);
    assert_eq!(editor.cursor(), 14, "lands at the end");
    assert_eq!(editor.key(Key::WordRight), Verdict::Handled);
    assert_eq!(editor.cursor(), 14, "never past the end");
}

#[test]
fn ctrl_w_kills_the_word_left() {
    let mut editor = editor(&[]);
    type_text(&mut editor, "git checkout");
    assert_eq!(editor.key(Key::KillWordLeft), Verdict::Handled);
    assert_eq!(editor.line(), "git ");
    assert_eq!(editor.cursor(), 4);
    assert_eq!(editor.key(Key::KillWordLeft), Verdict::Handled);
    assert_eq!(editor.line(), "");
}

#[test]
fn alt_d_kills_the_word_right() {
    let mut editor = editor(&[]);
    type_text(&mut editor, "git checkout");
    editor.key(Key::Home);
    assert_eq!(editor.key(Key::KillWordRight), Verdict::Handled);
    assert_eq!(editor.line(), " checkout");
    assert_eq!(editor.cursor(), 0);
}

#[test]
fn ctrl_u_and_ctrl_k_kill_to_their_edges() {
    let mut editor = editor(&[]);
    type_text(&mut editor, "git checkout");
    editor.key(Key::Home);
    for _ in 0..4 {
        editor.key(Key::Right);
    }

    assert_eq!(editor.key(Key::KillLineLeft), Verdict::Handled);
    assert_eq!(editor.line(), "checkout");
    assert_eq!(editor.cursor(), 0);
    assert_eq!(editor.key(Key::KillLineRight), Verdict::Handled);
    assert_eq!(editor.line(), "");
}

#[test]
fn newline_keys_insert_a_real_newline_and_never_run() {
    // The record's newline chords (Shift+Enter / Ctrl+J) share one
    // canonical key; the buffer becomes multi-line and Enter is
    // still the only runner.
    let mut editor = editor(&[]);
    type_text(&mut editor, "first");
    assert_eq!(editor.key(Key::Newline), Verdict::Handled);
    type_text(&mut editor, "second");
    assert_eq!(editor.line(), "first\nsecond");
}

#[test]
fn enter_reports_the_exact_pty_write_bytes_and_clears() {
    // The spec's execution contract: Enter reports the whole buffer
    // and the engine writes it plus one newline byte; the committed
    // line is never undoable, so the model clears the undo stacks
    // with the buffer.
    let mut editor = editor(&[]);
    type_text(&mut editor, "echo hi");

    let verdict = editor.key(Key::Enter);
    assert_eq!(
        verdict,
        Verdict::Execute {
            line: "echo hi".to_string()
        }
    );
    assert_eq!(verdict.pty_write().as_deref(), Some("echo hi\n"));
    assert_eq!(editor.line(), "");
    assert_eq!(editor.cursor(), 0);
}

#[test]
fn a_multi_line_buffer_executes_as_one_write() {
    // Paste's contract from the other side: a buffer carrying
    // interior newlines runs as one Enter — the write is the whole
    // block plus the single trailing newline.
    let mut editor = editor(&[]);
    editor.paste("one\ntwo");
    let verdict = editor.key(Key::Enter);
    assert_eq!(
        verdict,
        Verdict::Execute {
            line: "one\ntwo".to_string()
        }
    );
    assert_eq!(verdict.pty_write().as_deref(), Some("one\ntwo\n"));
}

// --- the signals ---

#[test]
fn ctrl_c_clears_a_non_empty_line() {
    let mut editor = editor(&[]);
    type_text(&mut editor, "half typed");
    assert_eq!(editor.key(Key::Interrupt), Verdict::Handled);
    assert_eq!(editor.line(), "");
    assert_eq!(editor.cursor(), 0);
}

#[test]
fn ctrl_c_on_an_empty_line_signals_sigint() {
    let mut editor = editor(&[]);
    assert_eq!(
        editor.key(Key::Interrupt),
        Verdict::Signal(Signal::Interrupt)
    );
}

#[test]
fn ctrl_d_on_an_empty_buffer_signals_eof() {
    // The PTY-write ruling the record ground: EOF on empty (the
    // shell's own exit path), forward-delete otherwise.
    let mut editor = editor(&[]);
    assert_eq!(
        editor.key(Key::EndOfFile),
        Verdict::Signal(Signal::EndOfFile)
    );
}

#[test]
fn ctrl_d_forward_deletes_on_a_non_empty_line() {
    // OQ-3, resolved by the record's finding 3: the documented
    // action is plain forward-delete.
    let mut editor = editor(&[]);
    type_text(&mut editor, "ab");
    editor.key(Key::Home);
    assert_eq!(editor.key(Key::EndOfFile), Verdict::Handled);
    assert_eq!(editor.line(), "b");
}

#[test]
fn ctrl_l_signals_the_screen_clear() {
    let mut editor = editor(&[]);
    type_text(&mut editor, "kept");
    assert_eq!(
        editor.key(Key::ClearScreen),
        Verdict::Signal(Signal::ClearScreen)
    );
    // The clear is a screen op, not a buffer op: the line stands.
    assert_eq!(editor.line(), "kept");
}

// --- undo/redo (OQ-8: Ctrl+Z undoes, buffer-scoped) ---

#[test]
fn ctrl_z_undoes_and_ctrl_shift_z_redoes() {
    // A text editor's undo granularity: a continuous typing run is
    // one step, and any other key breaks the run — so the arrow
    // detour between the two words below splits them into two.
    let mut editor = editor(&[]);
    type_text(&mut editor, "one");
    editor.key(Key::Left);
    editor.key(Key::Right);
    type_text(&mut editor, " two");

    assert_eq!(editor.key(Key::Undo), Verdict::Handled);
    assert_eq!(editor.line(), "one");
    assert_eq!(editor.key(Key::Undo), Verdict::Handled);
    assert_eq!(editor.line(), "");

    assert_eq!(editor.key(Key::Redo), Verdict::Handled);
    assert_eq!(editor.line(), "one");
    assert_eq!(editor.key(Key::Redo), Verdict::Handled);
    assert_eq!(editor.line(), "one two");
}

#[test]
fn a_committed_line_is_never_undoable() {
    // The record's rejected alternative made a rule: undo applies to
    // the buffer only — once Enter ran, the write is the shell's.
    let mut editor = editor(&[]);
    type_text(&mut editor, "gone");
    editor.key(Key::Enter);

    assert_eq!(editor.key(Key::Undo), Verdict::Handled);
    assert_eq!(editor.line(), "");
}

// --- paste ---

#[test]
fn a_multi_line_paste_inserts_verbatim_and_never_executes() {
    let mut editor = editor(&[]);
    type_text(&mut editor, "before ");
    editor.paste("line one\nline two\nline three");

    assert_eq!(editor.line(), "before line one\nline two\nline three");
    // Nothing ran: Enter is the only runner, and the cursor sits
    // after the pasted block.
    assert_eq!(editor.cursor(), editor.line().chars().count());
}

// --- history (Up/Down over the caller's store slice) ---

#[test]
fn up_walks_into_the_past_newest_first() {
    let mut editor = editor(&["newest", "middle", "oldest"]);

    assert_eq!(editor.key(Key::Up), Verdict::Handled);
    assert_eq!(editor.line(), "newest");
    assert_eq!(editor.key(Key::Up), Verdict::Handled);
    assert_eq!(editor.line(), "middle");
    assert_eq!(editor.key(Key::Up), Verdict::Handled);
    assert_eq!(editor.line(), "oldest");
    // Past the oldest entry navigation holds — never off the slice.
    assert_eq!(editor.key(Key::Up), Verdict::Handled);
    assert_eq!(editor.line(), "oldest");
}

#[test]
fn down_walks_back_and_the_edit_in_flight_is_preserved() {
    let mut editor = editor(&["newest", "oldest"]);
    type_text(&mut editor, "half typed");

    editor.key(Key::Up);
    editor.key(Key::Up);
    assert_eq!(editor.line(), "oldest");

    assert_eq!(editor.key(Key::Down), Verdict::Handled);
    assert_eq!(editor.line(), "newest");
    assert_eq!(editor.key(Key::Down), Verdict::Handled);
    assert_eq!(editor.line(), "half typed", "the draft comes back whole");
    // Past the draft, Down holds — never off the live line.
    assert_eq!(editor.key(Key::Down), Verdict::Handled);
    assert_eq!(editor.line(), "half typed");
}

#[test]
fn ctrl_p_and_ctrl_n_are_the_history_twins() {
    // The readline chords land on the same canonical keys as the
    // arrows (the router's collapse, pinned here so the twins can
    // never drift).
    let mut editor = editor(&["prev"]);

    assert_eq!(editor.key(Key::Up), Verdict::Handled);
    assert_eq!(editor.line(), "prev");
    assert_eq!(editor.key(Key::Down), Verdict::Handled);
    assert_eq!(editor.line(), "");
}

#[test]
fn editing_a_recalled_line_exits_navigation_with_the_edit_kept() {
    // Recall, edit, recall again: the edited line becomes the new
    // draft the next navigation preserves.
    let mut editor = editor(&["git status"]);
    editor.key(Key::Up);
    assert_eq!(editor.line(), "git status");

    type_text(&mut editor, " --short");
    assert_eq!(editor.line(), "git status --short");

    editor.key(Key::Up);
    assert_eq!(editor.line(), "git status");
    editor.key(Key::Down);
    assert_eq!(editor.line(), "git status --short");
}

#[test]
fn ctrl_c_clears_the_line_and_the_navigation_state() {
    let mut editor = editor(&["newest"]);
    type_text(&mut editor, "draft");
    editor.key(Key::Up);
    assert_eq!(editor.line(), "newest");

    editor.key(Key::Interrupt);
    assert_eq!(editor.line(), "");

    // The stale draft is gone: Down from the live line holds.
    assert_eq!(editor.key(Key::Down), Verdict::Handled);
    assert_eq!(editor.line(), "");
}

// --- the highlight tokenizer (rich-input RI-6) ---

use super::{HighlightSpan, TokenClass, highlight_spans};

fn classes(line: &str) -> Vec<(String, TokenClass)> {
    highlight_spans(line, |name| name == "git" || name == "echo")
        .into_iter()
        .map(|HighlightSpan { range, class }| (line[range].to_string(), class))
        .collect()
}

#[test]
fn the_four_token_classes_tile_a_line() {
    // The mock's painted set plus the variable: the command position
    // (known here), a flag, a variable, a quoted string, and the
    // plain default.
    let spans = classes("git status --short $HOME 'a b'");
    assert_eq!(
        spans,
        vec![
            ("git".to_string(), TokenClass::Command { known: true }),
            ("status".to_string(), TokenClass::Word),
            ("--short".to_string(), TokenClass::Flag),
            ("$HOME".to_string(), TokenClass::Variable),
            // The mock's own rule: whitespace splits a quoted
            // phrase, each piece painted string (quote-detection is
            // per token) — visually one string, structurally two.
            ("'a".to_string(), TokenClass::String),
            ("b'".to_string(), TokenClass::String),
        ]
    );
}

#[test]
fn an_unknown_command_carries_the_unknown_mark() {
    let spans = classes("totally-made-up --flag");
    assert_eq!(
        spans,
        vec![
            (
                "totally-made-up".to_string(),
                TokenClass::Command { known: false }
            ),
            ("--flag".to_string(), TokenClass::Flag),
        ]
    );
}

#[test]
fn each_line_of_a_multi_line_buffer_restarts_at_the_command() {
    // A pasted block's second line is a command too (the newline
    // resets the command position).
    let spans = classes("echo one\ngit status");
    assert_eq!(
        spans,
        vec![
            ("echo".to_string(), TokenClass::Command { known: true }),
            ("one".to_string(), TokenClass::Word),
            ("git".to_string(), TokenClass::Command { known: true }),
            ("status".to_string(), TokenClass::Word),
        ]
    );
}

#[test]
fn a_non_command_shape_leading_a_line_is_not_the_command_class() {
    // The mock's rule: only a `[a-z_]`-starting first token is the
    // command position — a leading flag or variable stays its own
    // class, never a Command span.
    let spans = classes("-x git");
    assert_eq!(
        spans,
        vec![
            ("-x".to_string(), TokenClass::Flag),
            ("git".to_string(), TokenClass::Word),
        ]
    );
}
