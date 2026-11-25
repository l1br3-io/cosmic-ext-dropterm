// SPDX-License-Identifier: GPL-3.0-only

//! The rich-input strip's pane-side halves (rich-input spec, RI-4):
//! the chord table that claims the v1 keys for the pure editor model
//! ([`dropterm::core::terminal::editor`]) and the strip's paint
//! snapshot — both pure over GPUI's keystroke data, headless-tested.
//! The pane owns the [`Editor`] instance and renders [`StripLine`]
//! in its docked row (the view's own paint).

use dropterm::core::terminal::editor::Key;
// Re-exported for the view, which owns the pane-side instance.
pub(crate) use dropterm::core::terminal::editor::Editor;
use gpui::Keystroke;

/// Translate a keystroke into the editor's canonical key vocabulary —
/// the v1 chord table from the record (every documented chord, the
/// synonyms collapsed: Ctrl+H is Backspace, Ctrl+A/E the edges,
/// Ctrl+B/F/N/P the arrows and the history lane, Ctrl+Left/Right and
/// Alt+Left/Right the word motions, Ctrl+Backspace/Delete the word
/// kills beside Ctrl+W/Alt+D). `None` is not the editor's — the press
/// falls through to the byte translation exactly as the native path
/// sends it. Tab is deliberately absent (SP-D10: Tab is the shell's
/// key, every state), and so are the F-keys, PageUp/Down, and Escape
/// (the menu keys arrive with the parity replay, RI-5).
pub(crate) fn editor_key(keystroke: &Keystroke) -> Option<Key> {
    let modifiers = &keystroke.modifiers;
    let key = keystroke.key.as_str();
    // Super is never the line editor's.
    if modifiers.platform {
        return None;
    }
    // Alt (no Ctrl): the word motions and Alt+D's kill.
    if !modifiers.control && modifiers.alt && !modifiers.shift {
        return match key {
            "left" => Some(Key::WordLeft),
            "right" => Some(Key::WordRight),
            "d" => Some(Key::KillWordRight),
            _ => None,
        };
    }
    if modifiers.control && !modifiers.alt {
        if modifiers.shift {
            return match key {
                "z" => Some(Key::Redo),
                "enter" => Some(Key::Newline),
                _ => None,
            };
        }
        return match key {
            "a" => Some(Key::Home),
            "e" => Some(Key::End),
            "b" => Some(Key::Left),
            "f" => Some(Key::Right),
            "p" => Some(Key::Up),
            "n" => Some(Key::Down),
            "h" => Some(Key::Backspace),
            "w" | "backspace" => Some(Key::KillWordLeft),
            "u" => Some(Key::KillLineLeft),
            "k" => Some(Key::KillLineRight),
            "l" => Some(Key::ClearScreen),
            "c" => Some(Key::Interrupt),
            "d" => Some(Key::EndOfFile),
            "z" => Some(Key::Undo),
            "j" => Some(Key::Newline),
            "delete" => Some(Key::KillWordRight),
            "left" => Some(Key::WordLeft),
            "right" => Some(Key::WordRight),
            _ => None,
        };
    }
    // Ctrl+Alt: not the editor's.
    if modifiers.control && modifiers.alt {
        return None;
    }
    match key {
        "backspace" => Some(Key::Backspace),
        "delete" => Some(Key::Delete),
        "left" => Some(Key::Left),
        "right" => Some(Key::Right),
        "up" => Some(Key::Up),
        "down" => Some(Key::Down),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "enter" if modifiers.shift => Some(Key::Newline),
        "enter" => Some(Key::Enter),
        _ => {
            // A printable: the typed character (Shift's case from
            // `key_char`, the space's char included). Control
            // characters are never inserts.
            let typed = keystroke
                .key_char
                .as_deref()
                .and_then(|c| c.chars().next())?;
            (!typed.is_control()).then_some(Key::Char(typed))
        }
    }
}

/// The routing claim (rich-input RI-4): `Some` only when the editor
/// owns the line right now — native mode and every stand-aside state
/// forward verbatim through today's translation.
pub(crate) fn claimed_key(keystroke: &Keystroke, editor_owns: bool) -> Option<Key> {
    editor_owns.then(|| editor_key(keystroke)).flatten()
}

/// The strip's paint snapshot: the owned line split around the caret
/// with the ghost suffix after it — pure data the view renders and
/// the tests pin (the highlight tokenizer, RI-6, restyles this shape
/// into spans; the geometry stays the view's).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StripLine {
    pub(crate) before_caret: String,
    pub(crate) after_caret: String,
    pub(crate) ghost: String,
}

impl StripLine {
    /// Capture the editor's line at its caret, with the engine's
    /// current ghost as the dim suffix.
    pub(crate) fn capture(editor: &Editor, ghost: Option<&str>) -> Self {
        let line = editor.line();
        let byte = line
            .char_indices()
            .nth(editor.cursor())
            .map_or(line.len(), |(byte, _)| byte);
        let (before, after) = line.split_at(byte);
        Self {
            before_caret: before.to_string(),
            after_caret: after.to_string(),
            ghost: ghost.unwrap_or_default().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Modifiers;

    fn key(modifiers: Modifiers, key: &str, typed: Option<&str>) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.to_string(),
            key_char: typed.map(str::to_string),
        }
    }

    fn plain() -> Modifiers {
        Modifiers::default()
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            control: true,
            ..Modifiers::default()
        }
    }

    fn ctrl_shift() -> Modifiers {
        Modifiers {
            control: true,
            shift: true,
            ..Modifiers::default()
        }
    }

    fn alt() -> Modifiers {
        Modifiers {
            alt: true,
            ..Modifiers::default()
        }
    }

    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Modifiers::default()
        }
    }

    #[test]
    fn the_v1_chord_table_maps_every_documented_chord() {
        // The record's chord table onto the canonical keys: the
        // editing core, the readline twins, the word and line kills,
        // the newline keys, the signals, and undo/redo.
        let cases: [(Modifiers, &str, Option<&str>, Key); 26] = [
            (plain(), "a", Some("a"), Key::Char('a')),
            (shift(), "a", Some("A"), Key::Char('A')),
            (plain(), "space", Some(" "), Key::Char(' ')),
            (plain(), "backspace", None, Key::Backspace),
            (ctrl(), "h", None, Key::Backspace),
            (plain(), "delete", None, Key::Delete),
            (plain(), "left", None, Key::Left),
            (ctrl(), "b", None, Key::Left),
            (plain(), "right", None, Key::Right),
            (ctrl(), "f", None, Key::Right),
            (plain(), "up", None, Key::Up),
            (ctrl(), "p", None, Key::Up),
            (plain(), "down", None, Key::Down),
            (ctrl(), "n", None, Key::Down),
            (plain(), "home", None, Key::Home),
            (ctrl(), "a", None, Key::Home),
            (plain(), "end", None, Key::End),
            (ctrl(), "e", None, Key::End),
            (alt(), "left", None, Key::WordLeft),
            (alt(), "right", None, Key::WordRight),
            (ctrl(), "w", None, Key::KillWordLeft),
            (alt(), "d", None, Key::KillWordRight),
            (ctrl(), "u", None, Key::KillLineLeft),
            (ctrl(), "k", None, Key::KillLineRight),
            (ctrl(), "c", None, Key::Interrupt),
            (ctrl(), "d", None, Key::EndOfFile),
        ];
        for (modifiers, name, typed, want) in cases {
            assert_eq!(
                editor_key(&key(modifiers, name, typed)),
                Some(want),
                "{name}"
            );
        }
        // The rest of the table the loop above could not carry
        // (duplicate key names with different modifiers).
        assert_eq!(editor_key(&key(ctrl(), "l", None)), Some(Key::ClearScreen));
        assert_eq!(editor_key(&key(ctrl(), "z", None)), Some(Key::Undo));
        assert_eq!(editor_key(&key(ctrl_shift(), "z", None)), Some(Key::Redo));
        assert_eq!(editor_key(&key(ctrl(), "j", None)), Some(Key::Newline));
        assert_eq!(
            editor_key(&key(shift(), "enter", Some("\r"))),
            Some(Key::Newline)
        );
        assert_eq!(
            editor_key(&key(plain(), "enter", Some("\r"))),
            Some(Key::Enter)
        );
        assert_eq!(
            editor_key(&key(ctrl(), "backspace", None)),
            Some(Key::KillWordLeft)
        );
        assert_eq!(
            editor_key(&key(ctrl(), "delete", None)),
            Some(Key::KillWordRight)
        );
    }

    #[test]
    fn tab_super_and_the_function_keys_stay_the_shells() {
        // SP-D10 holds inside the editor: Tab (and Shift+Tab) is the
        // shell's key in every state; Super chords, the F-keys, and
        // PageUp/Down never join the table; Escape waits for the
        // parity replay (RI-5).
        assert_eq!(editor_key(&key(plain(), "tab", None)), None);
        assert_eq!(editor_key(&key(shift(), "tab", None)), None);
        let super_a = Modifiers {
            platform: true,
            ..Modifiers::default()
        };
        assert_eq!(editor_key(&key(super_a, "a", Some("a"))), None);
        assert_eq!(editor_key(&key(plain(), "f5", None)), None);
        assert_eq!(editor_key(&key(plain(), "pageup", None)), None);
        assert_eq!(editor_key(&key(plain(), "escape", None)), None);
        let ctrl_alt = Modifiers {
            control: true,
            alt: true,
            ..Modifiers::default()
        };
        assert_eq!(editor_key(&key(ctrl_alt, "b", None)), None);
    }

    #[test]
    fn the_claim_is_gated_by_ownership() {
        // The routing half (RI-4's own acceptance): the same press
        // claims when the editor owns the line and forwards when it
        // does not — native mode and every stand-aside state read the
        // same None and fall through to today's byte translation.
        let press = key(plain(), "a", Some("a"));
        assert_eq!(claimed_key(&press, true), Some(Key::Char('a')));
        assert_eq!(claimed_key(&press, false), None);

        let interrupt = key(ctrl(), "c", None);
        assert_eq!(claimed_key(&interrupt, true), Some(Key::Interrupt));
        assert_eq!(claimed_key(&interrupt, false), None);
    }

    #[test]
    fn an_unowned_press_translates_to_exactly_the_native_bytes() {
        // The verbatim-forward guarantee, spot-checked at the
        // translation the native path runs: with the claim off, the
        // editor's own chords are the shell's bytes unchanged.
        use super::super::input::{self, KeyInput, KeyModes};

        let binds = input::default_key_binds();
        let modes = KeyModes {
            app_cursor: false,
            has_selection: false,
        };
        // A printable, Enter, and the signal bytes.
        assert_eq!(
            input::translate(&key(plain(), "a", Some("a")), &binds, modes),
            KeyInput::Bytes(b"a".to_vec())
        );
        assert_eq!(
            input::translate(&key(plain(), "enter", Some("\r")), &binds, modes),
            KeyInput::Bytes(b"\r".to_vec())
        );
        assert_eq!(
            input::translate(&key(ctrl(), "c", None), &binds, modes),
            KeyInput::Bytes(vec![0x03])
        );
        assert_eq!(
            input::translate(&key(ctrl(), "d", None), &binds, modes),
            KeyInput::Bytes(vec![0x04])
        );
        // Every editor chord with the claim off resolves through the
        // translation without panicking or mutating — the table and
        // the translation coexist on the same press.
        for (modifiers, name, typed) in [
            (ctrl(), "a", None),
            (ctrl(), "e", None),
            (alt(), "left", None),
            (ctrl(), "w", None),
            (shift(), "enter", Some("\r")),
        ] {
            let _ = input::translate(&key(modifiers, name, typed), &binds, modes);
        }
    }

    #[test]
    fn strip_line_splits_at_the_caret_and_carries_the_ghost() {
        // The paint snapshot: the line halves around the caret (the
        // cursor mid-word), the ghost suffix after the whole line.
        let mut editor = Editor::new(Vec::new());
        for ch in "git status".chars() {
            editor.key(dropterm::core::terminal::editor::Key::Char(ch));
        }
        editor.key(dropterm::core::terminal::editor::Key::Left);
        editor.key(dropterm::core::terminal::editor::Key::Left);
        editor.key(dropterm::core::terminal::editor::Key::Left);

        let line = StripLine::capture(&editor, Some("u --short"));
        assert_eq!(line.before_caret, "git sta");
        assert_eq!(line.after_caret, "tus");
        assert_eq!(line.ghost, "u --short");

        // Without a ghost the suffix is empty, and an empty buffer
        // splits into two empty halves (killed from the end).
        editor.key(dropterm::core::terminal::editor::Key::End);
        editor.key(dropterm::core::terminal::editor::Key::KillLineLeft);
        let empty = StripLine::capture(&editor, None);
        assert_eq!(empty.before_caret, "");
        assert_eq!(empty.after_caret, "");
        assert_eq!(empty.ghost, "");
    }
}
