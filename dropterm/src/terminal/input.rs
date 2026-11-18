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

use dropterm::core::keybinds::{Binding, ModifierName};
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
    /// Open (or close) the completion menu at the prompt
    /// (`Ctrl+Space`, suggest-menu spec — the libcosmic
    /// `ShowCompletions`): a soft trigger owned by the pane's key
    /// routing, where a declined chord's shell bytes live; the
    /// action arm itself is a documented no-op (the engine's menu
    /// state is the action).
    ShowCompletions,
    /// Scroll the viewport to the bottom (`Ctrl+Shift+End`) — the
    /// status band's scrolled chip's jump verb as a chord, the same
    /// engine call the chip's click routes.
    ScrollToBottom,
    // The shell-owned family (the keyboard route the menus always
    // had — the reference app's whole table, wired through the
    // pane's key handler because the pane is where every keystroke
    // lands): each verb hands up to the shell's own funnels, the
    // ones the menus call; the pane owns only the match.
    NewTab,
    NewTabHome,
    DuplicateTab,
    CloseTab,
    RenameTab,
    NextTab,
    PrevTab,
    MoveTabLeft,
    MoveTabRight,
    /// Activate tab N (1-based, 1..=8 — 9 is [`Self::ActivateLastTab`]).
    ActivateTab(u8),
    ActivateLastTab,
    Settings,
    ImagesPanel,
    ToggleStatusBar,
}

impl TermAction {
    /// Whether the verb belongs to the shell (its funnels are the
    /// menus' own) rather than the pane — the key handler's routing
    /// gate between `perform_action` and the shell handoff.
    pub(crate) fn is_shell_verb(self) -> bool {
        matches!(
            self,
            TermAction::NewTab
                | TermAction::NewTabHome
                | TermAction::DuplicateTab
                | TermAction::CloseTab
                | TermAction::RenameTab
                | TermAction::NextTab
                | TermAction::PrevTab
                | TermAction::MoveTabLeft
                | TermAction::MoveTabRight
                | TermAction::ActivateTab(_)
                | TermAction::ActivateLastTab
                | TermAction::Settings
                | TermAction::ImagesPanel
                | TermAction::ToggleStatusBar
        )
    }
}

/// The stable string id of an action — the `custom_key_binds` key,
/// identical to the libcosmic frontend's ids (the file-format
/// contract one id set across frontends). The numbered seats are
/// per-slot ids (`activate_tab_1` … `activate_tab_8`) and resolve
/// through [`activate_slot_id`]; the payload form carries no single
/// id, so it answers `None`.
// Not consumed outside the tests yet: the id vocabulary's other
// consumers (the settings editor) arrive with their slice, and the
// round-trip test holds the contract until then.
#[allow(dead_code)]
pub(crate) fn action_id(action: TermAction) -> Option<&'static str> {
    match action {
        TermAction::Copy => Some("copy"),
        TermAction::CopyPrimary => Some("copy_primary"),
        TermAction::Paste => Some("paste"),
        TermAction::PastePrimary => Some("paste_primary"),
        TermAction::SelectAll => Some("select_all"),
        TermAction::ClearScrollback => Some("clear_scrollback"),
        TermAction::ZoomIn => Some("zoom_in"),
        TermAction::ZoomOut => Some("zoom_out"),
        TermAction::ZoomReset => Some("zoom_reset"),
        TermAction::Hide => Some("hide"),
        TermAction::ShowCompletions => Some("show_completions"),
        TermAction::ScrollToBottom => Some("scroll_to_bottom"),
        TermAction::NewTab => Some("new_tab"),
        TermAction::NewTabHome => Some("new_tab_home"),
        TermAction::DuplicateTab => Some("duplicate_tab"),
        TermAction::CloseTab => Some("close_tab"),
        TermAction::RenameTab => Some("rename_tab"),
        TermAction::NextTab => Some("next_tab"),
        TermAction::PrevTab => Some("prev_tab"),
        TermAction::MoveTabLeft => Some("move_tab_left"),
        TermAction::MoveTabRight => Some("move_tab_right"),
        TermAction::ActivateTab(_) => None,
        TermAction::ActivateLastTab => Some("activate_last_tab"),
        TermAction::Settings => Some("settings"),
        TermAction::ImagesPanel => Some("images_panel"),
        TermAction::ToggleStatusBar => Some("toggle_status_bar"),
    }
}

/// The numbered seat's per-slot id (`activate_tab_4` for seat 4).
#[cfg(test)]
pub(crate) fn activate_slot_id(slot: u8) -> String {
    format!("activate_tab_{slot}")
}

/// The action an id string resolves to, for the actions this slice
/// owns; `None` for anything else (a stale hand-edited entry).
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
        "show_completions" => Some(TermAction::ShowCompletions),
        "scroll_to_bottom" => Some(TermAction::ScrollToBottom),
        "new_tab" => Some(TermAction::NewTab),
        "new_tab_home" => Some(TermAction::NewTabHome),
        "duplicate_tab" => Some(TermAction::DuplicateTab),
        "close_tab" => Some(TermAction::CloseTab),
        "rename_tab" => Some(TermAction::RenameTab),
        "next_tab" => Some(TermAction::NextTab),
        "prev_tab" => Some(TermAction::PrevTab),
        "move_tab_left" => Some(TermAction::MoveTabLeft),
        "move_tab_right" => Some(TermAction::MoveTabRight),
        "activate_last_tab" => Some(TermAction::ActivateLastTab),
        "settings" => Some(TermAction::Settings),
        "images_panel" => Some(TermAction::ImagesPanel),
        "toggle_status_bar" => Some(TermAction::ToggleStatusBar),
        // The numbered seats (`activate_tab_1` … `activate_tab_8`);
        // a seat past 8 is not this table's (9 is activate_last_tab).
        _ => id
            .strip_prefix("activate_tab_")
            .and_then(|slot| slot.parse::<u8>().ok())
            .filter(|slot| (1..=8).contains(slot))
            .map(TermAction::ActivateTab),
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
    map.insert(chord(cs, "end"), TermAction::ScrollToBottom);
    // The completion menu's soft trigger (suggest-menu spec): the
    // pane's key routing owns the open/toggle before this map's
    // swallow sees the chord; the map entry carries a custom
    // `show_completions` binding into the same routing.
    map.insert(
        chord(&[ModifierName::Ctrl], "space"),
        TermAction::ShowCompletions,
    );
    // The shell-owned family — the reference table's own defaults
    // (core `keybinds::BINDING_ROWS`), one chord for one verb, the
    // pane matching and the shell executing. The pair audit against
    // the pane's own fifteen above: every letter seat (t h d w r i
    // p b) is fresh, the numbered seats ride Ctrl+Shift+1..9 where
    // only the Ctrl-held 0/-/=/+ cluster lives, and the PageUp/Down
    // seats split by modifier exactly as the reference does —
    // Ctrl+PageUp/Down cycles, Ctrl+Shift+PageUp/Down moves, the
    // bare Shift+PageUp/Down scroll staying the translation's own.
    map.insert(chord(cs, "t"), TermAction::NewTab);
    map.insert(chord(cs, "h"), TermAction::NewTabHome);
    map.insert(chord(cs, "d"), TermAction::DuplicateTab);
    map.insert(chord(cs, "w"), TermAction::CloseTab);
    map.insert(chord(cs, "r"), TermAction::RenameTab);
    map.insert(
        chord(&[ModifierName::Ctrl], "pagedown"),
        TermAction::NextTab,
    );
    map.insert(chord(&[ModifierName::Ctrl], "tab"), TermAction::NextTab);
    map.insert(chord(&[ModifierName::Ctrl], "pageup"), TermAction::PrevTab);
    map.insert(chord(cs, "tab"), TermAction::PrevTab);
    map.insert(chord(cs, "pageup"), TermAction::MoveTabLeft);
    map.insert(chord(cs, "pagedown"), TermAction::MoveTabRight);
    for slot in 1..=8u8 {
        map.insert(chord(cs, &slot.to_string()), TermAction::ActivateTab(slot));
    }
    map.insert(chord(cs, "9"), TermAction::ActivateLastTab);
    map.insert(chord(cs, "i"), TermAction::Settings);
    map.insert(chord(cs, "p"), TermAction::ImagesPanel);
    map.insert(chord(cs, "b"), TermAction::ToggleStatusBar);
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
                println!("dropterm: ignoring custom key bind {id:?}: {binding:?}");
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
    Scroll(dropterm::core::terminal::TerminalScroll),
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
                return KeyInput::Scroll(dropterm::core::terminal::TerminalScroll::PageUp);
            }
            Some(csi("5", "~", mod_no))
        }
        "pagedown" => {
            if modifiers.shift {
                return KeyInput::Scroll(dropterm::core::terminal::TerminalScroll::PageDown);
            }
            Some(csi("6", "~", mod_no))
        }
        "end" => {
            if modifiers.shift {
                return KeyInput::Scroll(dropterm::core::terminal::TerminalScroll::Bottom);
            }
            Some(if modes.app_cursor {
                ss3("F", mod_no)
            } else {
                csi2("F", mod_no)
            })
        }
        "home" => {
            if modifiers.shift {
                return KeyInput::Scroll(dropterm::core::terminal::TerminalScroll::Top);
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
    // `key_char` the typed character (Shift's case, Alt's symbol)
    // — where the platform reports one. Real hardware reports
    // `None` for every modified press (the rig's synthesized keys
    // always carried it, so the walks never met this shape — the
    // first `just dev` hardware loop's catch): Ctrl and Alt still
    // own the press, so the character derives from `key` itself
    // through [`bare_key_char`]. Shift-held presses guess nothing —
    // the typed case is the layout's, never ours.
    let typed = keystroke
        .key_char
        .as_deref()
        .and_then(|c| c.chars().next())
        .or_else(|| bare_key_char(key, *modifiers));
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
            if let Some(typed) = typed
                && (!typed.is_control() || (u32::from(typed) < 32))
            {
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
        // `key_char` while `key` holds the printed letter, and the
        // key_char-less press derives the same code from `key`).
        (false, true, false, false) => {
            if let Some(typed) = typed
                && typed.is_control()
            {
                let mut buf = [0; 4];
                KeyInput::Bytes(typed.encode_utf8(&mut buf).as_bytes().to_vec())
            } else {
                KeyInput::Swallowed
            }
        }
        // Ctrl+Shift: the one pairing the libcosmic build honors —
        // Ctrl+Shift+_ sends 0x1F (xterm's rule; bare Ctrl+_ is the
        // same byte through the control-character path above on the
        // layouts where `_` survives Shift-less). The arm reads
        // `key`, never `key_char`, so it stands on the key_char-less
        // platform shape too.
        (false, true, false, true) => {
            if keystroke.key == "_" {
                KeyInput::Bytes(b"\x1f".to_vec())
            } else {
                KeyInput::Swallowed
            }
        }
        // Alt (no ctrl): the typed character, ESC-prefixed.
        (false, false, true, _) => typed.map_or(KeyInput::Swallowed, |typed| {
            KeyInput::Bytes(no_ctrl_input(typed, true))
        }),
        // Bare: the typed character, Shift's case included.
        (false, false, false, _) => typed.map_or(KeyInput::Swallowed, |typed| {
            KeyInput::Bytes(no_ctrl_input(typed, false))
        }),
    }
}

/// The character a `key_char`-less press stands for, if any: the
/// platform reported no typed character (every modified press on
/// real hardware), so Ctrl derives the control code from the
/// printed letter (`l` → 0x0C, xterm's rule) and Alt takes the key
/// itself for the ESC prefix. Shift-held and Super presses, and
/// keys that are not a single ASCII character, stay `None` — the
/// swallow stands (the layout's case is never ours to guess, and
/// Ctrl's non-letter pairings have no xterm byte to send).
fn bare_key_char(key: &str, modifiers: Modifiers) -> Option<char> {
    if modifiers.shift || modifiers.platform {
        return None;
    }
    let mut chars = key.chars();
    let printed = chars.next()?;
    if chars.next().is_some() || !printed.is_ascii_lowercase() {
        return None;
    }
    if modifiers.control {
        // `a`..=`z` → 0x01..=0x1A, the same byte the platform carries
        // in `key_char` where it reports one.
        char::from_u32(u32::from(printed) - u32::from(b'a') + 1)
    } else {
        modifiers.alt.then_some(printed)
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
                use dropterm::core::terminal::TerminalScroll;
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
        // round-trip pins the whole vocabulary — the pane's own verbs
        // and the shell family — plus the numbered seats through
        // their per-slot ids.
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
            TermAction::ShowCompletions,
            TermAction::ScrollToBottom,
            TermAction::NewTab,
            TermAction::NewTabHome,
            TermAction::DuplicateTab,
            TermAction::CloseTab,
            TermAction::RenameTab,
            TermAction::NextTab,
            TermAction::PrevTab,
            TermAction::MoveTabLeft,
            TermAction::MoveTabRight,
            TermAction::ActivateLastTab,
            TermAction::Settings,
            TermAction::ImagesPanel,
            TermAction::ToggleStatusBar,
        ] {
            let id = action_id(action).expect("the payload-less verbs carry ids");
            assert_eq!(action_from_id(id), Some(action));
        }
        for slot in 1..=8u8 {
            assert_eq!(
                action_from_id(&activate_slot_id(slot)),
                Some(TermAction::ActivateTab(slot))
            );
        }
        // The payload form answers None (no single id), and the seat
        // past 8 is not this table's — 9 is activate_last_tab.
        assert_eq!(action_id(TermAction::ActivateTab(4)), None);
        assert_eq!(action_from_id("activate_tab_9"), None);
        assert_eq!(action_from_id("not_an_action"), None);
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
        // And the counts stay honest: the pane's fifteen (the
        // clipboard pair, the primary paste, select-all,
        // clear-scrollback, the scroll-to-bottom jump, the zoom
        // cluster's six, the hide, the completion trigger) plus the
        // shell family's twenty-three (the five letter verbs, the
        // cycle pair's four, the moves' two, the eight numbered
        // seats, the last seat, settings, images, the band).
        assert_eq!(default_key_binds().len(), 38);
    }

    #[test]
    fn the_settings_chord_is_ours_not_the_kit_inspectors() {
        // The collision the first `just dev` hardware walk caught:
        // the Kit's debug inspector binds Ctrl+Shift+I globally in
        // every debug build — the table's own Settings chord. The
        // match makes the press ours (and the key handler stops
        // propagation), so the chord fires Settings, never the
        // inspector.
        let cs_i = key(ctrl_shift(), "i", None);
        assert_eq!(
            translate(&cs_i, &default_key_binds(), plain_modes()),
            KeyInput::Action(TermAction::Settings)
        );
        // A chord the table does not own still swallows into
        // nothing here — what falls through to the keymap (the
        // inspector's dev reachability) is the handler's propagation
        // boundary, not the table's.
        let cs_y = key(ctrl_shift(), "y", None);
        assert_eq!(
            translate(&cs_y, &default_key_binds(), plain_modes()),
            KeyInput::Swallowed
        );
    }

    #[test]
    fn key_char_less_ctrl_letters_send_their_control_codes() {
        // The real-hardware shape (the first `just dev` hardware
        // walk, 2026-11-18): every modified press arrives with
        // `key_char: None` — the synthesized rig keys always carried
        // the control code, so only the live keyboard met the gap.
        // Ctrl+C must interrupt (`^C`), Ctrl+L must clear.
        assert_eq!(
            translate(&key(ctrl(), "c", None), &default_key_binds(), plain_modes()),
            KeyInput::Bytes(vec![0x03])
        );
        assert_eq!(
            translate(&key(ctrl(), "l", None), &default_key_binds(), plain_modes()),
            KeyInput::Bytes(vec![0x0c])
        );
    }

    #[test]
    fn key_char_less_alt_letters_take_the_esc_prefix() {
        assert_eq!(
            translate(&key(alt(), "b", None), &default_key_binds(), plain_modes()),
            KeyInput::Bytes(vec![0x1b, b'b'])
        );
        // Ctrl+Alt pairs the pair: ESC before the control code.
        let ctrl_alt = Modifiers {
            control: true,
            alt: true,
            ..Modifiers::default()
        };
        assert_eq!(
            translate(
                &key(ctrl_alt, "b", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(vec![0x1b, 0x02])
        );
    }

    #[test]
    fn key_char_less_ctrl_shift_underscore_still_sends_0x1f() {
        // The arm reads `key`, never `key_char` — readline's undo
        // survives on the key_char-less platform shape.
        assert_eq!(
            translate(
                &key(ctrl_shift(), "_", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Bytes(vec![0x1f])
        );
    }

    #[test]
    fn key_char_less_presses_still_guess_nothing() {
        // The boundary the fallback keeps: Shift-held presses (the
        // typed case is the layout's) and non-letter Ctrl pairings
        // (no xterm byte). The shell family's chords are binds now
        // (the shell-chords slice) — Ctrl+Shift+T fires NewTab in
        // the key_char-less shape like every other chord, covered by
        // the every-chord row above.
        assert_eq!(
            translate(&key(ctrl(), "1", None), &default_key_binds(), plain_modes()),
            KeyInput::Swallowed
        );
        let shift_b = shift();
        assert_eq!(
            translate(
                &key(shift_b, "b", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Swallowed
        );
        assert_eq!(
            translate(
                &key(ctrl_shift(), "t", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Action(TermAction::NewTab)
        );
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
            KeyInput::Scroll(dropterm::core::terminal::TerminalScroll::PageUp)
        ));
        assert!(matches!(
            translate(
                &key(shift(), "pagedown", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Scroll(dropterm::core::terminal::TerminalScroll::PageDown)
        ));
        assert!(matches!(
            translate(
                &key(shift(), "home", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Scroll(dropterm::core::terminal::TerminalScroll::Top)
        ));
        assert!(matches!(
            translate(
                &key(shift(), "end", None),
                &default_key_binds(),
                plain_modes()
            ),
            KeyInput::Scroll(dropterm::core::terminal::TerminalScroll::Bottom)
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
