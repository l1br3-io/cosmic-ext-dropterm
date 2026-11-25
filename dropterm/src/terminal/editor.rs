// SPDX-License-Identifier: GPL-3.0-only

//! The rich-input strip's pane-side halves (rich-input spec, RI-4):
//! the chord table that claims the v1 keys for the pure editor model
//! ([`dropterm::core::terminal::editor`]) and the strip's paint
//! snapshot — both pure over GPUI's keystroke data, headless-tested.
//! The pane owns the [`Editor`] instance and renders [`StripLine`]
//! in its docked row (the view's own paint).

use dropterm::core::terminal::editor::Key;
use dropterm::core::terminal::suggest::SuggestionSource;
// Re-exported for the view, which owns the pane-side instance and
// routes the lane's menu outcomes and paints the token classes.
pub(crate) use dropterm::core::terminal::editor::{Editor, MenuPress, TokenClass};
use gpui::Keystroke;

/// The strip's colored segments under the toggle (rich-input
/// RI-6): the tokenizer's tiling when highlight is on, one plain
/// segment when it is off — the paint's whole decision, pure for
/// the pin.
pub(crate) fn segments_for(
    line: &str,
    highlight: bool,
    command_known: impl Fn(&str) -> bool,
) -> Vec<(TokenClass, String)> {
    if highlight {
        tile_segments(line, command_known)
    } else {
        vec![(TokenClass::Word, line.to_string())]
    }
}

/// Tile the whole line into colored segments (rich-input RI-6): the
/// tokenizer's spans with the whitespace gaps as plain pieces, so
/// the paint walks one contiguous list.
pub(crate) fn tile_segments(
    line: &str,
    command_known: impl Fn(&str) -> bool,
) -> Vec<(TokenClass, String)> {
    let mut segments = Vec::new();
    let mut cursor = 0usize;
    for span in dropterm::core::terminal::editor::highlight_spans(line, command_known) {
        if span.range.start > cursor {
            segments.push((TokenClass::Word, line[cursor..span.range.start].to_string()));
        }
        segments.push((span.class, line[span.range.clone()].to_string()));
        cursor = span.range.end;
    }
    if cursor < line.len() {
        segments.push((TokenClass::Word, line[cursor..].to_string()));
    }
    segments
}

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
#[cfg(test)]
pub(crate) fn claimed_key(keystroke: &Keystroke, editor_owns: bool) -> Option<Key> {
    editor_owns.then(|| editor_key(keystroke)).flatten()
}

/// The strip's paint snapshot: the owned line split around the caret
/// with the ghost suffix after it — pure data the tests pin (the
/// RI-6 segment paint replaced the shape at the view; the pin keeps
/// the caret-split contract).
#[cfg(test)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StripLine {
    pub(crate) before_caret: String,
    pub(crate) after_caret: String,
    pub(crate) ghost: String,
}

#[cfg(test)]
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

/// The editor's menu routing (rich-input RI-5, the restored parity
/// contract): while the menu shows inside the editor, Up/Down
/// navigate, Enter two-stages (the equal line runs, a differing one
/// accepts), Escape closes, printables keep the menu open — the
/// insert plus the lane's step re-collecting the rows — and Tab,
/// the shell's key in the native lane, restored to the menu inside
/// the editor by the rich-input lift: the LCP fill of the filtered
/// case-sensitive prefix rows (closing the last) else a cycle of
/// the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorMenuRouting {
    /// Up/Down (wrap is the transition's own).
    Navigate(dropterm::core::terminal::suggest::MenuKey),
    /// The menu's Accept press (a differing line; the equal line is
    /// [`Self::CloseThenRun`]).
    Accept,
    /// The equal-line Enter (two-stage): close, insert nothing, run.
    CloseThenRun,
    /// Escape: close, insert nothing, nothing else.
    Close,
    /// Tab: the LCP-fill-else-cycle decision.
    FillOrCycle,
    /// A printable edit: the press flows to the buffer, the menu
    /// stays, the rows re-collect on the lane's step.
    KeepOpen,
    /// Not the menu's: close it, then the press's own meaning runs.
    CloseThenProcess,
}

/// The scoped routing itself — [`super::suggest::menu_routing`]'s
/// editor twin, pure over the keystroke shape. The differences from
/// the grid lane are the lift itself: Tab is the menu's fill here,
/// and the equal-line Enter closes-and-runs (the editor executes;
/// the grid lane forwarded bytes).
pub(crate) fn editor_menu_routing(keystroke: &Keystroke, equal_line: bool) -> EditorMenuRouting {
    let modifiers = &keystroke.modifiers;
    let key = keystroke.key.as_str();
    let no_command_mods = !(modifiers.control || modifiers.alt || modifiers.platform);
    if no_command_mods {
        if !modifiers.shift {
            match key {
                "up" => {
                    return EditorMenuRouting::Navigate(
                        dropterm::core::terminal::suggest::MenuKey::Up,
                    );
                }
                "down" => {
                    return EditorMenuRouting::Navigate(
                        dropterm::core::terminal::suggest::MenuKey::Down,
                    );
                }
                "enter" => {
                    return if equal_line {
                        EditorMenuRouting::CloseThenRun
                    } else {
                        EditorMenuRouting::Accept
                    };
                }
                "escape" => return EditorMenuRouting::Close,
                _ => {}
            }
        }
        // The lift (rich-input spec): Tab fills or cycles inside the
        // editor, bare and shifted alike.
        if key == "tab" {
            return EditorMenuRouting::FillOrCycle;
        }
        // The as-you-type edit: printables keep the menu open.
        if keystroke
            .key_char
            .as_deref()
            .and_then(|c| c.chars().next())
            .is_some_and(|c| !c.is_control())
        {
            return EditorMenuRouting::KeepOpen;
        }
    }
    EditorMenuRouting::CloseThenProcess
}

/// Tab's decision (the restored LCP-fill-else-cycle): the longest
/// common prefix of the rows whose label starts with the typed
/// segment case-sensitively — extending the segment means the fill
/// (the insert is the extension beyond the typed text, the last row
/// standing closing the menu), anything else a plain cycle. Pure
/// over the rows the engine exposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TabFill {
    /// The insert beyond the typed text, and whether the menu
    /// closes after (one row left standing).
    Insert { beyond: String, close: bool },
    /// No common extension: cycle the selection.
    Cycle,
}

pub(crate) fn tab_fill(labels: &[&str], typed: &str) -> TabFill {
    let prefix_rows: Vec<&str> = labels
        .iter()
        .copied()
        .filter(|label| label.starts_with(typed))
        .collect();
    if prefix_rows.is_empty() {
        return TabFill::Cycle;
    }
    let mut lcp = prefix_rows[0].to_string();
    for label in &prefix_rows[1..] {
        let common = lcp
            .chars()
            .zip(label.chars())
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a)
            .collect::<String>();
        lcp = common;
    }
    if lcp.len() > typed.len() && lcp.starts_with(typed) {
        let beyond = lcp[typed.len()..].to_string();
        let standing = prefix_rows
            .iter()
            .filter(|label| label.starts_with(lcp.as_str()))
            .count();
        TabFill::Insert {
            beyond,
            close: standing == 1,
        }
    } else {
        TabFill::Cycle
    }
}

// --- the menu chrome (rich-input RI-7) ---

/// The menu chrome's toggles (rich-input RI-7): the RI-1 keys as
/// one pure bundle, the pane's options folded once per paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the RI-1 keys' fold: each bool is its own serialized config field, regrouping changes the settings contract"
)]
pub(crate) struct MenuChrome {
    pub(crate) descriptions: bool,
    pub(crate) badges: bool,
    pub(crate) icons: bool,
    pub(crate) argument_hint: bool,
    pub(crate) compact_rows: bool,
}

#[cfg(test)]
impl MenuChrome {
    /// Every element on, comfortable rows — the defaults' fold.
    pub(crate) const fn all_on() -> Self {
        Self {
            descriptions: true,
            badges: true,
            icons: true,
            argument_hint: true,
            compact_rows: false,
        }
    }
}

/// One rendered menu row (rich-input RI-7): the mock's row model —
/// the marked label (the typed prefix, matched case-insensitively,
/// the paint bolds), the badge vocabulary word, the corpus
/// description, and the icon glyph — pure data for the paint and
/// the pins; each element is `None` when its toggle is off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MenuRowModel {
    pub(crate) label: String,
    /// How many leading CHARS of the label the typed segment
    /// matched (the mock's marking rule).
    pub(crate) marked: usize,
    /// The badge vocabulary: cmd/flag/path/hist/env/sub.
    pub(crate) badge: Option<&'static str>,
    pub(crate) description: Option<String>,
    pub(crate) icon: Option<char>,
}

/// The badge vocabulary word a row carries (rich-input RI-7): a
/// `-`-prefixed label is a flag whatever its source; otherwise the
/// provider decides — catalog cmd, specs subcommand sub, path,
/// history hist, env; the generator's value sets (git branches,
/// tags) read as the command's own sub vocabulary.
fn badge_of(source: SuggestionSource, label: &str) -> &'static str {
    if label.starts_with('-') {
        return "flag";
    }
    match source {
        SuggestionSource::Catalog => "cmd",
        SuggestionSource::Specs | SuggestionSource::Generator => "sub",
        SuggestionSource::Path => "path",
        SuggestionSource::History => "hist",
        SuggestionSource::Env => "env",
    }
}

/// The icon glyph per badge (rich-input RI-7): the mock's lucide
/// set as single glyphs — refined against the mock at the RI-C
/// review.
fn icon_for(badge: &str) -> char {
    match badge {
        "cmd" => '\u{276f}',
        "flag" => '\u{2013}',
        "path" => '/',
        "hist" => '\u{21bb}',
        "env" => '$',
        _ => '\u{25b8}',
    }
}

/// The menu's rendered rows (rich-input RI-7): the engine's
/// candidates into the mock's row model under the toggles.
pub(crate) fn menu_row_models(
    rows: &[dropterm::core::terminal::suggest::MenuCandidate],
    typed: &str,
    chrome: MenuChrome,
) -> Vec<MenuRowModel> {
    rows.iter()
        .map(|row| {
            let badge = badge_of(row.source, &row.label);
            let marked = if !typed.is_empty()
                && row.label.to_lowercase().starts_with(&typed.to_lowercase())
            {
                typed.chars().count()
            } else {
                0
            };
            MenuRowModel {
                label: row.label.clone(),
                marked,
                badge: chrome.badges.then_some(badge),
                description: chrome
                    .descriptions
                    .then(|| row.description.clone())
                    .flatten(),
                icon: chrome.icons.then(|| icon_for(badge)),
            }
        })
        .collect()
}

/// One menu row's height under the density switch (rich-input
/// RI-7): comfortable carries the description on its own line,
/// compact inlines it beside the label — the mock's padding pair.
#[must_use]
pub(crate) fn menu_row_height_px(compact: bool) -> f32 {
    if compact { 22. } else { 34. }
}

/// The editor menu's paint bundle (rich-input RI-7): the rendered
/// rows under the toggles, the selection and scroll the engine
/// holds, and the hint line — built in the render pass under one
/// engine read, painted by the pane above the strip.
pub(crate) struct EditorMenuPaint {
    pub(crate) rows: Vec<MenuRowModel>,
    pub(crate) selected: usize,
    pub(crate) scroll: usize,
    pub(crate) hint: Option<(String, Option<String>)>,
    pub(crate) chrome: MenuChrome,
}

impl EditorMenuPaint {
    /// Capture the menu's paint state, `None` when the editor's
    /// menu stands closed (or empty) — the paint's whole gate.
    pub(crate) fn capture(
        engine: &dropterm::core::terminal::Terminal,
        editor_state: &Editor,
        chrome: MenuChrome,
    ) -> Option<Self> {
        if !engine.rich_input() || !engine.menu_is_open() {
            return None;
        }
        let (to_caret, _) = editor_state.step_inputs();
        let typed = to_caret
            .split_whitespace()
            .next_back()
            .unwrap_or_default()
            .to_string();
        let rows = menu_row_models(engine.menu_rows(), &typed, chrome);
        if rows.is_empty() {
            return None;
        }
        let hint = chrome
            .argument_hint
            .then(|| {
                to_caret
                    .split_whitespace()
                    .next()
                    .and_then(|command| engine.editor_menu_hint(command))
            })
            .flatten();
        Some(Self {
            rows,
            selected: engine.menu_selected(),
            scroll: engine.menu_scroll(),
            hint,
            chrome,
        })
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

    // --- the menu chrome (rich-input RI-7) ---

    fn candidate(
        label: &str,
        source: SuggestionSource,
        description: Option<&str>,
    ) -> dropterm::core::terminal::suggest::MenuCandidate {
        dropterm::core::terminal::suggest::MenuCandidate {
            label: label.to_string(),
            suffix: String::new(),
            description: description.map(str::to_string),
            source,
        }
    }

    #[test]
    fn rows_carry_the_badge_vocabulary() {
        use super::{MenuChrome, menu_row_models};
        use dropterm::core::terminal::suggest::SuggestionSource as S;

        let rows = vec![
            candidate("git", S::Catalog, None),
            candidate("status", S::Specs, None),
            candidate("--short", S::Specs, None),
            candidate("src/", S::Path, None),
            candidate("sleep 30", S::History, None),
            candidate("$HOME", S::Env, None),
            candidate("feature/x", S::Generator, None),
        ];
        let models = menu_row_models(&rows, "", MenuChrome::all_on());
        let badges: Vec<&str> = models.iter().filter_map(|row| row.badge).collect();
        assert_eq!(
            badges,
            vec!["cmd", "sub", "flag", "path", "hist", "env", "sub"]
        );
        // A flag label wins over its source: the specs' flags are
        // flags, not subs.
        assert_eq!(models[2].badge, Some("flag"));
    }

    #[test]
    fn the_typed_prefix_marks_case_insensitively() {
        use super::{MenuChrome, menu_row_models};

        let rows = vec![candidate("Git-Status", SuggestionSource::Specs, None)];
        let models = menu_row_models(&rows, "GIT", MenuChrome::all_on());
        assert_eq!(models[0].marked, 3);
        // A non-matching prefix marks nothing.
        let models = menu_row_models(&rows, "zzz", MenuChrome::all_on());
        assert_eq!(models[0].marked, 0);
    }

    #[test]
    fn each_chrome_element_disappears_with_its_toggle() {
        use super::{MenuChrome, menu_row_models};

        let rows = &[candidate(
            "git",
            SuggestionSource::Catalog,
            Some("the thing"),
        )];
        let on = &menu_row_models(rows, "gi", MenuChrome::all_on())[0];
        assert_eq!(on.badge, Some("cmd"));
        assert_eq!(on.description.as_deref(), Some("the thing"));
        assert_eq!(on.icon, Some('\u{276f}'));

        let off = &menu_row_models(
            rows,
            "gi",
            MenuChrome {
                descriptions: false,
                badges: false,
                icons: false,
                ..MenuChrome::all_on()
            },
        )[0];
        assert_eq!(off.badge, None);
        assert_eq!(off.description, None);
        assert_eq!(off.icon, None);
        // The label and its mark survive every toggle.
        assert_eq!(off.label, "git");
        assert_eq!(off.marked, 2);
    }

    #[test]
    fn the_density_switch_flips_the_row_height() {
        use super::menu_row_height_px;
        let comfortable = menu_row_height_px(false);
        let compact = menu_row_height_px(true);
        assert!(
            compact < comfortable,
            "compact rows are the tighter density"
        );
    }

    #[test]
    fn the_highlight_toggle_switches_tiled_for_plain() {
        use super::{TokenClass, segments_for};
        use dropterm::core::terminal::editor as core_editor;

        // On: the line tiles into its classes (the known command and
        // its flag).
        let on = segments_for("git status", true, |name| name == "git");
        assert_eq!(
            on,
            vec![
                (TokenClass::Command { known: true }, "git".to_string()),
                (TokenClass::Word, " ".to_string()),
                (TokenClass::Word, "status".to_string()),
            ]
        );
        // Off: one plain segment carries the whole line.
        let off = segments_for("git status", false, |name| name == "git");
        assert_eq!(off, vec![(TokenClass::Word, "git status".to_string())]);
        let _ = core_editor::TokenClass::Word;
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

    // --- the editor's menu routing and Tab fill (RI-5 part 2) ---

    #[test]
    fn the_editor_menu_routing_restores_the_parity_table() {
        use super::{EditorMenuRouting, editor_menu_routing};
        use dropterm::core::terminal::suggest::MenuKey;

        let equal = editor_menu_routing(&key(plain(), "enter", Some("\r")), true);
        assert_eq!(equal, EditorMenuRouting::CloseThenRun);
        let differing = editor_menu_routing(&key(plain(), "enter", Some("\r")), false);
        assert_eq!(differing, EditorMenuRouting::Accept);
        assert_eq!(
            editor_menu_routing(&key(plain(), "up", None), false),
            EditorMenuRouting::Navigate(MenuKey::Up)
        );
        assert_eq!(
            editor_menu_routing(&key(plain(), "down", None), false),
            EditorMenuRouting::Navigate(MenuKey::Down)
        );
        assert_eq!(
            editor_menu_routing(&key(plain(), "escape", None), false),
            EditorMenuRouting::Close
        );
        // The lift: Tab is the menu's fill inside the editor, bare
        // and shifted alike (the native lane's verbatim forward is
        // the claim's own exclusion, not this table's).
        assert_eq!(
            editor_menu_routing(&key(plain(), "tab", None), false),
            EditorMenuRouting::FillOrCycle
        );
        assert_eq!(
            editor_menu_routing(&key(shift(), "tab", None), false),
            EditorMenuRouting::FillOrCycle
        );
        // Printables keep the menu open; command chords close it.
        assert_eq!(
            editor_menu_routing(&key(plain(), "a", Some("a")), false),
            EditorMenuRouting::KeepOpen
        );
        assert_eq!(
            editor_menu_routing(&key(shift(), "a", Some("A")), false),
            EditorMenuRouting::KeepOpen
        );
        assert_eq!(
            editor_menu_routing(&key(ctrl(), "a", None), false),
            EditorMenuRouting::CloseThenProcess
        );
    }

    #[test]
    fn the_tab_fill_extends_the_lcp_else_cycles() {
        use super::{TabFill, tab_fill};

        // The LCP of the case-sensitive prefix rows extends "gi"
        // (the rows that share it — gimp diverges and would cap the
        // LCP at the query itself, the Cycle case below).
        let rows = vec!["git", "git-status"];
        assert_eq!(
            tab_fill(&rows, "gi"),
            TabFill::Insert {
                beyond: "t".to_string(),
                close: false
            }
        );
        // One row left standing closes the menu.
        assert_eq!(
            tab_fill(&rows, "git-st"),
            TabFill::Insert {
                beyond: "atus".to_string(),
                close: true
            }
        );
        // No common extension: cycle.
        assert_eq!(tab_fill(&["git", "gimp"], "gi"), TabFill::Cycle);
        // Case-sensitive: a capital row never fills a lowercase query.
        assert_eq!(tab_fill(&["Git", "Gimp"], "gi"), TabFill::Cycle);
        // No prefix rows at all: cycle.
        assert_eq!(tab_fill(&["cargo"], "gi"), TabFill::Cycle);
    }
}
