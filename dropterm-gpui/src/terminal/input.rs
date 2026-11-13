// SPDX-License-Identifier: GPL-3.0-only

//! The input halves (gpui-terminal Task 2): GPUI keystrokes through
//! core's keybind model (the default chords plus the
//! `custom_key_binds` overlay) to the pane's actions, and the
//! key-to-bytes translation the libcosmic widget owns — both pure
//! over GPUI's keystroke data, headless-tested. The pointer half
//! lives in the view.
//!
//! The action set is this slice's: clipboard, selection,
//! scrollback-clearing, zoom, and the surface hide. The tab, menu,
//! settings, and completion actions arrive with their own slices;
//! their chords stay unbound here, falling through to the shell
//! exactly as an unbound chord does in the libcosmic build.

use std::collections::{BTreeMap, HashMap, HashSet};

use dropterm_core::keybinds::{Binding, ModifierName};
use gpui::{Keystroke, Modifiers};

/// The pane's actions — the terminal-view slice of the libcosmic
/// frontend's `Action` set, the same stable ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TermAction {
    Copy,
    CopyPrimary,
    Paste,
    PastePrimary,
    SelectAll,
    ClearScrollback,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Hide,
}

/// The stable string id of an action — the `custom_key_binds` key,
/// identical to the libcosmic frontend's ids (the file-format
/// contract one id set across frontends).
// Not consumed outside the tests yet: the id vocabulary's other
// consumers (the settings editor) arrive with their slice, and the
// round-trip test holds the contract until then.
#[allow(dead_code)]
pub(crate) fn action_id(action: TermAction) -> &'static str {
    match action {
        TermAction::Copy => "copy",
        TermAction::CopyPrimary => "copy_primary",
        TermAction::Paste => "paste",
        TermAction::PastePrimary => "paste_primary",
        TermAction::SelectAll => "select_all",
        TermAction::ClearScrollback => "clear_scrollback",
        TermAction::ZoomIn => "zoom_in",
        TermAction::ZoomOut => "zoom_out",
        TermAction::ZoomReset => "zoom_reset",
        TermAction::Hide => "hide",
    }
}

/// The action an id string resolves to, for the actions this slice
/// owns; `None` for anything else (another slice's action id or a
/// stale hand-edited entry).
pub(crate) fn action_from_id(id: &str) -> Option<TermAction> {
    match id {
        "copy" => Some(TermAction::Copy),
        "copy_primary" => Some(TermAction::CopyPrimary),
        "paste" => Some(TermAction::Paste),
        "paste_primary" => Some(TermAction::PastePrimary),
        "select_all" => Some(TermAction::SelectAll),
        "clear_scrollback" => Some(TermAction::ClearScrollback),
        "zoom_in" => Some(TermAction::ZoomIn),
        "zoom_out" => Some(TermAction::ZoomOut),
        "zoom_reset" => Some(TermAction::ZoomReset),
        "hide" => Some(TermAction::Hide),
        _ => None,
    }
}

/// One runtime chord: the modifier state and the GPUI key name.
/// Equality is exact (the libcosmic `KeyBind`'s rule), so the
/// Shift-held forms of the zoom cluster are their own chords.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Chord {
    pub modifiers: Modifiers,
    pub key: String,
}

/// A chord builder for the defaults and the tests, in the
/// libcosmic vocabulary.
pub(crate) fn chord(modifiers: &[ModifierName], key: &str) -> Chord {
    let mut state = Modifiers::default();
    for modifier in modifiers {
        match modifier {
            ModifierName::Ctrl => state.control = true,
            ModifierName::Shift => state.shift = true,
            ModifierName::Alt => state.alt = true,
            ModifierName::Super => state.platform = true,
        }
    }
    Chord {
        modifiers: state,
        key: key.to_string(),
    }
}

/// The default chords this slice owns — the libcosmic frontend's
/// map filtered to the pane's actions (every default chord of an
/// unimplemented action stays absent, falling through to the shell).
pub(crate) fn default_key_binds() -> HashMap<Chord, TermAction> {
    let mut map = HashMap::new();
    let cs = &[ModifierName::Ctrl, ModifierName::Shift];
    map.insert(chord(cs, "c"), TermAction::Copy);
    map.insert(chord(cs, "v"), TermAction::Paste);
    map.insert(
        chord(&[ModifierName::Shift], "insert"),
        TermAction::PastePrimary,
    );
    map.insert(chord(cs, "a"), TermAction::SelectAll);
    map.insert(chord(cs, "k"), TermAction::ClearScrollback);
    // The zoom cluster: the physical keys with Ctrl, their
    // Shift-held forms (kitty's bind, the layouts where `=` and `-`
    // survive Shift), and the literal plus chord.
    map.insert(chord(&[ModifierName::Ctrl], "="), TermAction::ZoomIn);
    map.insert(chord(&[ModifierName::Ctrl], "+"), TermAction::ZoomIn);
    map.insert(chord(cs, "+"), TermAction::ZoomIn);
    map.insert(chord(cs, "="), TermAction::ZoomIn);
    map.insert(chord(&[ModifierName::Ctrl], "-"), TermAction::ZoomOut);
    map.insert(chord(cs, "-"), TermAction::ZoomOut);
    map.insert(chord(&[ModifierName::Ctrl], "0"), TermAction::ZoomReset);
    map.insert(chord(cs, "q"), TermAction::Hide);
    map
}

/// The config format's key name onto GPUI's — the persisted
/// vocabulary (`"C"`, `"PageUp"`, `"Space"`, `"Insert"`, …) onto the
/// platform's (`"c"`, `"pageup"`, `"space"`, `"insert"`, …). Single
/// ASCII letters canonicalize to lowercase (the default map's form;
/// `Chord`'s derived equality is case-sensitive while matching
/// presses arrive lowercase).
fn gpui_key_name(name: &str) -> String {
    let mapped = match name {
        "Insert" => "insert",
        "Delete" => "delete",
        "Home" => "home",
        "End" => "end",
        "PageUp" => "pageup",
        "PageDown" => "pagedown",
        "Tab" => "tab",
        "Backspace" => "backspace",
        "Enter" => "enter",
        "Escape" => "escape",
        "ArrowLeft" | "Left" => "left",
        "ArrowRight" | "Right" => "right",
        "ArrowUp" | "Up" => "up",
        "ArrowDown" | "Down" => "down",
        "Space" | "space" => "space",
        _ => return name.to_lowercase(),
    };
    mapped.to_string()
}

/// A persisted [`Binding`] as a runtime chord, or `None` when a key
/// name is empty (the hand-edited-config case the libcosmic
/// resolution skips with a warn).
fn to_chord(binding: &Binding) -> Option<Chord> {
    if binding.key.is_empty() {
        return None;
    }
    Some(chord(&binding.modifiers, &gpui_key_name(&binding.key)))
}

/// The effective map the pane matches against: the defaults with
/// every customized action's chords replaced by its one custom
/// binding — the libcosmic `effective_key_binds` rule. Entries for
/// actions this slice does not own resolve to nothing and are
/// skipped with a warn (their owners' slices land their own
/// defaults); invalid key names skip the same way, leaving the
/// action's defaults standing.
pub(crate) fn effective_key_binds(
    custom: &BTreeMap<String, Binding>,
) -> HashMap<Chord, TermAction> {
    let resolved: Vec<(TermAction, Chord)> = custom
        .iter()
        .filter_map(|(id, binding)| {
            if let (Some(action), Some(pressed)) = (action_from_id(id), to_chord(binding)) {
                Some((action, pressed))
            } else {
                println!("dropterm-gpui: ignoring custom key bind {id:?}: {binding:?}");
                None
            }
        })
        .collect();
    let customized: HashSet<TermAction> = resolved.iter().map(|(action, _)| *action).collect();

    let mut binds = default_key_binds();
    binds.retain(|_, action| !customized.contains(action));
    for (action, pressed) in resolved {
        binds.insert(pressed, action);
    }
    binds
}

/// Whether a keystroke matches a chord: exact on the four binding
/// modifiers and the key name (function is not a binding modifier).
pub(crate) fn chord_matches(chord: &Chord, keystroke: &Keystroke) -> bool {
    let m = &keystroke.modifiers;
    let state = Modifiers {
        control: m.control,
        shift: m.shift,
        alt: m.alt,
        platform: m.platform,
        function: false,
    };
    state == chord.modifiers && keystroke.key == chord.key
}

/// The action a keystroke fires, if any — the entry the view's key
/// handler consults before the byte translation (a matched action
/// swallows the press).
pub(crate) fn matched_action(
    key_binds: &HashMap<Chord, TermAction>,
    keystroke: &Keystroke,
) -> Option<TermAction> {
    key_binds
        .iter()
        .find(|(chord, _)| chord_matches(chord, keystroke))
        .map(|(_, action)| *action)
}

/// What one keystroke becomes on the pane: bytes for the PTY, an
/// action, or nothing (swallowed or unmapped).
#[derive(Debug, Clone)]
pub(crate) enum KeyInput {
    /// Bytes for the PTY.
    Bytes(Vec<u8>),
    /// The matched action — the press is the app's, not the shell's.
    Action(TermAction),
    /// The press maps to nothing the pane sends (a bare Super chord,
    /// the app's own Escape-with-selection case): swallowed.
    Swallowed,
    /// A scroll the keys themselves ask for (Shift+PageUp/Down,
    /// Shift+Home/End — the libcosmic widget's own special cases).
    Scroll(dropterm_core::terminal::TerminalScroll),
}

/// The terminal mode flags the translation consults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KeyModes {
    pub app_cursor: bool,
    pub has_selection: bool,
}

/// Translate one keystroke, the libcosmic widget's mapping onto
/// GPUI's keystroke shape. A matched chord fires its action; the
/// named keys carry their CSI/SS3 sequences with the xterm modifier
/// number; Escape cancels a selection before it ever reaches the
/// shell; character keys follow the ctrl/alt/shift rules (including
/// Ctrl+Space's NUL and Ctrl+Shift+`_`'s 0x1F).
/// One mapping table in one function: the xterm rules read together
/// or not at all (the libcosmic widget's own shape — splitting the
/// named-key table from the modifier rules scatters one protocol).
#[expect(
    clippy::too_many_lines,
    reason = "the xterm key translation is one protocol table; the named-key arm and the modifier arms read together"
)]
pub(crate) fn translate(
    keystroke: &Keystroke,
    binds: &HashMap<Chord, TermAction>,
    modes: KeyModes,
) -> KeyInput {
    // The keybind swallow first: a matched action owns the press.
    if let Some(action) = matched_action(binds, keystroke) {
        return KeyInput::Action(action);
    }

    let modifiers = &keystroke.modifiers;
    let mod_no = modifier_number(*modifiers);
    let alt_prefix = if modifiers.alt { "\x1b" } else { "" };
    let key = keystroke.key.as_str();

    // The named keys.
    let named = match key {
        "insert" => Some(csi("2", "~", mod_no)),
        "delete" => Some(csi("3", "~", mod_no)),
        "pageup" => {
            if modifiers.shift {
                return KeyInput::Scroll(dropterm_core::terminal::TerminalScroll::PageUp);
            }
            Some(csi("5", "~", mod_no))
        }
        "pagedown" => {
            if modifiers.shift {
                return KeyInput::Scroll(dropterm_core::terminal::TerminalScroll::PageDown);
            }
            Some(csi("6", "~", mod_no))
        }
        "end" => {
            if modifiers.shift {
                return KeyInput::Scroll(dropterm_core::terminal::TerminalScroll::Bottom);
            }
            Some(if modes.app_cursor {
                ss3("F", mod_no)
            } else {
                csi2("F", mod_no)
            })
        }
        "home" => {
            if modifiers.shift {
                return KeyInput::Scroll(dropterm_core::terminal::TerminalScroll::Top);
            }
            Some(if modes.app_cursor {
                ss3("H", mod_no)
            } else {
                csi2("H", mod_no)
            })
        }
        "up" => Some(arrow("A", modes.app_cursor, mod_no)),
        "down" => Some(arrow("B", modes.app_cursor, mod_no)),
        "right" => Some(arrow("C", modes.app_cursor, mod_no)),
        "left" => Some(arrow("D", modes.app_cursor, mod_no)),
        "f1" => Some(ss3("P", mod_no)),
        "f2" => Some(ss3("Q", mod_no)),
        "f3" => Some(ss3("R", mod_no)),
        "f4" => Some(ss3("S", mod_no)),
        "f5" => Some(csi("15", "~", mod_no)),
        "f6" => Some(csi("17", "~", mod_no)),
        "f7" => Some(csi("18", "~", mod_no)),
        "f8" => Some(csi("19", "~", mod_no)),
        "f9" => Some(csi("20", "~", mod_no)),
        "f10" => Some(csi("21", "~", mod_no)),
        "f11" => Some(csi("23", "~", mod_no)),
        "f12" => Some(csi("24", "~", mod_no)),
        "backspace" => Some(
            format!(
                "{alt_prefix}{}",
                if modifiers.control { "\x08" } else { "\x7f" }
            )
            .into_bytes(),
        ),
        "enter" => Some(format!("{alt_prefix}\r").into_bytes()),
        "escape" => {
            // Escape with any modifier cancels a selection first;
            // with none standing it is the shell's own byte.
            if modes.has_selection {
                return KeyInput::Swallowed;
            }
            Some(format!("{alt_prefix}\x1b").into_bytes())
        }
        "tab" => Some(
            format!(
                "{alt_prefix}{}",
                if modifiers.shift { "\x1b[Z" } else { "\x09" }
            )
            .into_bytes(),
        ),
        _ => None,
    };
    if let Some(bytes) = named {
        return KeyInput::Bytes(bytes);
    }

    // The character keys: `key` is the lowercase printed key,
    // `key_char` the typed character (Shift's case, Alt's symbol).
    let Some(typed) = keystroke.key_char.as_deref().and_then(|c| c.chars().next()) else {
        return KeyInput::Swallowed;
    };
    match (
        modifiers.platform,
        modifiers.control,
        modifiers.alt,
        modifiers.shift,
    ) {
        // Super chords are never the shell's.
        (true, _, _, _) => KeyInput::Swallowed,
        // Ctrl+Alt: the control-character or literal pairing,
        // ESC-prefixed.
        (false, true, true, _) => {
            if !typed.is_control() || (u32::from(typed) < 32) {
                let mut buf = vec![0x1b];
                let mut char_buf = [0; 4];
                buf.extend_from_slice(typed.encode_utf8(&mut char_buf).as_bytes());
                KeyInput::Bytes(buf)
            } else {
                KeyInput::Swallowed
            }
        }
        // Ctrl (no shift): the typed control character itself
        // (Ctrl-A's 0x01 — the platform carries the control code in
        // `key_char` while `key` holds the printed letter).
        (false, true, false, false) => {
            if typed.is_control() {
                let mut buf = [0; 4];
                KeyInput::Bytes(typed.encode_utf8(&mut buf).as_bytes().to_vec())
            } else {
                KeyInput::Swallowed
            }
        }
        // Ctrl+Shift: the one pairing the libcosmic build honors —
        // Ctrl+Shift+_ sends 0x1F (xterm's rule; bare Ctrl+_ is the
        // same byte through the control-character path above on the
        // layouts where `_` survives Shift-less).
        (false, true, false, true) => {
            if keystroke.key == "_" {
                KeyInput::Bytes(b"\x1f".to_vec())
            } else {
                KeyInput::Swallowed
            }
        }
        // Alt (no ctrl): the typed character, ESC-prefixed.
        (false, false, true, _) => KeyInput::Bytes(no_ctrl_input(typed, true)),
        // Bare: the typed character, Shift's case included.
        (false, false, false, _) => KeyInput::Bytes(no_ctrl_input(typed, false)),
    }
}

/// A character's bytes, optionally ESC-prefixed — the plain and Alt
/// character paths.
fn no_ctrl_input(character: char, alt: bool) -> Vec<u8> {
    let mut input = Vec::with_capacity(5);
    if alt {
        input.push(0x1b);
    }
    let mut buf = [0; 4];
    input.extend_from_slice(character.encode_utf8(&mut buf).as_bytes());
    input
}

/// The xterm modifier number off the modifier state (1 = none).
fn modifier_number(modifiers: Modifiers) -> u8 {
    let mut number = 1;
    if modifiers.shift {
        number += 1;
    }
    if modifiers.alt {
        number += 2;
    }
    if modifiers.control {
        number += 4;
    }
    if modifiers.platform {
        number += 8;
    }
    number
}

fn csi(code: &str, suffix: &str, modifiers: u8) -> Vec<u8> {
    if modifiers == 1 {
        format!("\x1b[{code}{suffix}").into_bytes()
    } else {
        format!("\x1b[{code};{modifiers}{suffix}").into_bytes()
    }
}

/// CSI 1 ; modifier {ABCDEFHPQS} — the legacy functional keys.
fn csi2(code: &str, modifiers: u8) -> Vec<u8> {
    if modifiers == 1 {
        format!("\x1b[{code}").into_bytes()
    } else {
        format!("\x1b[1;{modifiers}{code}").into_bytes()
    }
}

fn ss3(code: &str, modifiers: u8) -> Vec<u8> {
    if modifiers == 1 {
        format!("\x1bO{code}").into_bytes()
    } else {
        format!("\x1b[1;{modifiers}{code}").into_bytes()
    }
}

fn arrow(code: &str, app_cursor: bool, modifiers: u8) -> Vec<u8> {
    if app_cursor {
        ss3(code, modifiers)
    } else {
        csi2(code, modifiers)
    }
}

/// Equality for the tests (the engine's `TerminalScroll` carries no
/// `PartialEq` of its own): the value arms compare exactly, the
/// scroll arm by its variant and delta.
impl PartialEq for KeyInput {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::Action(a), Self::Action(b)) => a == b,
            (Self::Swallowed, Self::Swallowed) => true,
            (Self::Scroll(a), Self::Scroll(b)) => {
                use dropterm_core::terminal::TerminalScroll;
                matches!(
                    (a, b),
                    (TerminalScroll::PageUp, TerminalScroll::PageUp)
                        | (TerminalScroll::PageDown, TerminalScroll::PageDown)
                        | (TerminalScroll::Top, TerminalScroll::Top)
                        | (TerminalScroll::Bottom, TerminalScroll::Bottom)
                ) || matches!(
                    (a, b),
                    (TerminalScroll::Delta(x), TerminalScroll::Delta(y)) if x == y
                )
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Keystroke;

    /// A keystroke in tests: modifier flags and the platform's key
    /// name, with `key_char` the typed character when the key is a
    /// printable (the platform's shape: `key` the lowercase printed
    /// key, `key_char` the case-carrying typed char).
    fn key(modifiers: Modifiers, key: &str, typed: Option<&str>) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.to_string(),
            key_char: typed.map(str::to_string),
        }
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

    fn shift() -> Modifiers {
        Modifiers {
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

    fn plain_modes() -> KeyModes {
        KeyModes {
            app_cursor: false,
            has_selection: false,
        }
    }

    #[test]
    fn the_action_ids_round_trip_across_frontends() {
        // The file-format contract: one id set across frontends. The
        // round-trip pins the vocabulary this slice owns.
        for action in [
            TermAction::Copy,
            TermAction::CopyPrimary,
            TermAction::Paste,
            TermAction::PastePrimary,
            TermAction::SelectAll,
            TermAction::ClearScrollback,
            TermAction::ZoomIn,
            TermAction::ZoomOut,
            TermAction::ZoomReset,
            TermAction::Hide,
        ] {
            assert_eq!(action_from_id(action_id(action)), Some(action));
        }
        assert_eq!(action_from_id("new_tab"), None, "another slice's id");
    }

    #[test]
    fn every_default_chord_fires_its_action() {
        // The acceptance row: each chord in the default map, pressed
        // exactly, fires its action and nothing else.
        for (chord, action) in default_key_binds() {
            let keystroke = key(chord.modifiers, &chord.key, None);
            assert_eq!(
                translate(&keystroke, &default_key_binds(), plain_modes()),
                KeyInput::Action(action),
                "chord {chord:?} fires {action:?}"
            );
        }
        // And the counts stay honest: the clipboard pair, the
        // primary paste, select-all, clear-scrollback, the zoom
        // cluster's six, and the hide.
        assert_eq!(default_key_binds().len(), 13);
    }

    #[test]
    fn a_custom_overlay_replaces_an_actions_chords() {
        // The libcosmic overlay rule: one custom binding replaces
        // ALL of the action's default chords; removing the entry
        // reverts. Here: copy moves to Ctrl+Alt+C, its defaults die,
        // and the freed default chord falls through to the shell.
        let mut custom = BTreeMap::new();
        custom.insert(
            "copy".to_string(),
            Binding {
                modifiers: vec![ModifierName::Ctrl, ModifierName::Alt],
                key: "C".to_string(),
            },
        );
        let binds = effective_key_binds(&custom);
        let moved = key(
            Modifiers {
                control: true,
                alt: true,
                ..Modifiers::default()
            },
            "c",
            None,
        );
        assert_eq!(
            translate(&moved, &binds, plain_modes()),
            KeyInput::Action(TermAction::Copy)
        );
        // The freed default chord reaches the widget's own rules:
        // Ctrl+Shift on a control character is swallowed there too
        // (only `_` sends its byte), so the chord dies with no
        // action and no shell bytes — the libcosmic behavior row.
        let old = key(ctrl_shift(), "c", Some("\u{3}"));
        assert_eq!(translate(&old, &binds, plain_modes()), KeyInput::Swallowed);
    }

    #[test]
    fn another_slices_action_id_and_a_bad_key_skip_cleanly() {
        // A tab action's id (a later slice's) and an empty key name
        // both skip, leaving every default standing.
        let mut custom = BTreeMap::new();
        custom.insert(
            "new_tab".to_string(),
            Binding {
                modifiers: vec![ModifierName::Ctrl, ModifierName::Shift],
                key: "T".to_string(),
            },
        );
        custom.insert(
            "copy".to_string(),
            Binding {
                modifiers: vec![ModifierName::Ctrl],
                key: String::new(),
            },
        );
        assert_eq!(effective_key_binds(&custom), default_key_binds());
    }

    #[test]
    fn named_keys_carry_their_sequences() {
        let cases: [(&str, Vec<u8>); 8] = [
            ("up", b"\x1b[A".to_vec()),
            ("down", b"\x1b[B".to_vec()),
            ("right", b"\x1b[C".to_vec()),
            ("left", b"\x1b[D".to_vec()),
            ("home", b"\x1b[H".to_vec()),
            ("end", b"\x1b[F".to_vec()),
            ("f5", b"\x1b[15~".to_vec()),
            ("delete", b"\x1b[3~".to_vec()),
        ];
        for (name, bytes) in cases {
            assert_eq!(
                translate(
                    &key(Modifiers::default(), name, None),
                    &default_key_binds(),
                    plain_modes()
                ),
                KeyInput::Bytes(bytes),
                "{name}"
            );
        }
        // App-cursor mode swaps arrows to SS3.
        let app = KeyModes {
            app_cursor: true,
            has_selection: false,
        };
        assert_eq!(
            translate(
                &key(Modifiers::default(), "up", None),
                &default_key_binds(),
                app
            ),
            KeyInput::Bytes(b"\x1bOA".to_vec())
        );
        // Modified arrows carry the xterm modifier number.
        assert_eq!(
            translate(
                &key(ctrl(), "up", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(b"\x1b[1;5A".to_vec())
        );
    }

    #[test]
    fn the_special_keys_map_verbatim() {
        assert_eq!(
            translate(
                &key(Modifiers::default(), "enter", Some("\r"),),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(b"\r".to_vec())
        );
        assert_eq!(
            translate(
                &key(Modifiers::default(), "backspace", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(b"\x7f".to_vec())
        );
        // Ctrl+Backspace sends BS, Alt+Enter the ESC-prefixed CR,
        // Shift+Tab the back-tab sequence.
        assert_eq!(
            translate(
                &key(ctrl(), "backspace", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(b"\x08".to_vec())
        );
        assert_eq!(
            translate(
                &key(alt(), "enter", Some("\r")),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(b"\x1b\r".to_vec())
        );
        assert_eq!(
            translate(
                &key(shift(), "tab", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(b"\x1b[Z".to_vec())
        );
    }

    #[test]
    fn escape_cancels_a_selection_before_reaching_the_shell() {
        let selecting = KeyModes {
            app_cursor: false,
            has_selection: true,
        };
        assert_eq!(
            translate(
                &key(Modifiers::default(), "escape", None),
                &default_key_binds(),
                selecting
            ),
            KeyInput::Swallowed
        );
        assert_eq!(
            translate(
                &key(Modifiers::default(), "escape", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(b"\x1b".to_vec())
        );
    }

    #[test]
    fn characters_follow_the_modifier_rules() {
        // Bare: the typed character, case carried by key_char.
        assert_eq!(
            translate(
                &key(Modifiers::default(), "a", Some("a")),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(b"a".to_vec())
        );
        assert_eq!(
            translate(
                &key(shift(), "a", Some("A")),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(b"A".to_vec())
        );
        // Ctrl: the control character (key already lowercase).
        assert_eq!(
            translate(
                &key(ctrl(), "a", Some("\u{1}")),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(b"\x01".to_vec())
        );
        // Ctrl+Shift+_: 0x1F (xterm's rule).
        assert_eq!(
            translate(
                &key(ctrl_shift(), "_", Some("_")),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(b"\x1f".to_vec())
        );
        // Ctrl+Shift on another key: swallowed (never a guess).
        assert_eq!(
            translate(
                &key(ctrl_shift(), "x", Some("X")),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Swallowed
        );
        // Alt: ESC-prefixed typed character.
        assert_eq!(
            translate(
                &key(alt(), "a", Some("a")),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(b"\x1ba".to_vec())
        );
        // Super: never the shell's.
        assert_eq!(
            translate(
                &key(
                    Modifiers {
                        platform: true,
                        ..Modifiers::default()
                    },
                    "a",
                    Some("a")
                ),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Swallowed
        );
    }

    #[test]
    fn the_scroll_keys_carry_their_page_semantics() {
        // Shift+PageUp/Down scroll the scrollback; Shift+Home/End go
        // to the top and bottom — the libcosmic special cases.
        assert!(matches!(
            translate(
                &key(shift(), "pageup", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Scroll(dropterm_core::terminal::TerminalScroll::PageUp)
        ));
        assert!(matches!(
            translate(
                &key(shift(), "pagedown", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Scroll(dropterm_core::terminal::TerminalScroll::PageDown)
        ));
        assert!(matches!(
            translate(
                &key(shift(), "home", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Scroll(dropterm_core::terminal::TerminalScroll::Top)
        ));
        assert!(matches!(
            translate(
                &key(shift(), "end", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Scroll(dropterm_core::terminal::TerminalScroll::Bottom)
        ));
        // Unshifted they are the shell's own sequences.
        assert_eq!(
            translate(
                &key(Modifiers::default(), "pageup", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(b"\x1b[5~".to_vec())
        );
    }
}
