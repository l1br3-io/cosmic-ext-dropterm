// SPDX-License-Identifier: GPL-3.0-only

//! The app-level keyboard shortcuts: a fixed default map plus a
//! per-action customization overlay.
//!
//! The defaults ([`default_key_binds`]) stay in force for every action
//! without a custom binding. A custom binding — one per action, keyed
//! by the stable [`action_id`] string — replaces *all* of that action's
//! default chords; removing the entry reverts to the defaults
//! ([`effective_key_binds`] resolves the overlay). The `TerminalBox`
//! widget only uses the effective map to decide which key presses to
//! swallow instead of sending to the shell; the actual behavior for a
//! matched [`Action`] is applied in `App::update`.

use std::collections::{BTreeMap, HashMap, HashSet};

use cosmic::iced::keyboard::key::{Named, Physical};
use cosmic::iced::keyboard::{Key, Modifiers};
use cosmic::widget::menu::key_bind::{KeyBind, Modifier};
use serde::{Deserialize, Serialize};

use crate::terminal::Action;

fn bind(modifiers: &[Modifier], key: &str) -> KeyBind {
    KeyBind {
        modifiers: modifiers.to_vec(),
        key: Key::Character(key.into()),
    }
}

fn bind_named(modifiers: &[Modifier], key: Named) -> KeyBind {
    KeyBind {
        modifiers: modifiers.to_vec(),
        key: Key::Named(key),
    }
}

/// The default app-level bindings, mapped to the [`Action`] they trigger.
pub fn default_key_binds() -> HashMap<KeyBind, Action> {
    let mut map = HashMap::new();
    map.insert(bind(&[Modifier::Ctrl, Modifier::Shift], "c"), Action::Copy);
    map.insert(bind(&[Modifier::Ctrl, Modifier::Shift], "v"), Action::Paste);
    map.insert(
        bind_named(&[Modifier::Shift], Named::Insert),
        Action::PastePrimary,
    );
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "a"),
        Action::SelectAll,
    );
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "k"),
        Action::ClearScrollback,
    );
    map.insert(bind(&[Modifier::Ctrl], "="), Action::ZoomIn);
    map.insert(bind(&[Modifier::Ctrl], "+"), Action::ZoomIn);
    // The Shift-held plus chords: matching is exact on modifiers,
    // so the literal Ctrl+Plus press (Shift held to type `+` on
    // US-style layouts) is its own chord or it arrives dead, and
    // "=" covers the layouts where it survives Shift (kitty's
    // bind).
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "+"),
        Action::ZoomIn,
    );
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "="),
        Action::ZoomIn,
    );
    map.insert(bind(&[Modifier::Ctrl], "-"), Action::ZoomOut);
    map.insert(bind(&[Modifier::Ctrl], "0"), Action::ZoomReset);
    map.insert(bind(&[Modifier::Ctrl, Modifier::Shift], "q"), Action::Hide);
    // Settings. Bare Ctrl+I must stay unbound: in a terminal it is
    // the TAB byte (0x09), so swallowing it would break tab
    // completion.
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "i"),
        Action::Settings,
    );
    // The status band's toggle (status-bar OQ-3, ruled 2026-10-10).
    // Bare Ctrl+B must stay unbound: readline's cursor-back byte.
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "b"),
        Action::ToggleStatusBar,
    );
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "t"),
        Action::NewTab,
    );
    // A new tab in the home directory — the plain keybind's settings
    // composition, home in place of the inherited cwd. Bare Ctrl+H
    // must stay unbound: in a terminal it is the backspace byte
    // (0x08), so swallowing it would break backspace in shells that
    // treat it literally.
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "h"),
        Action::NewTabHome,
    );
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "w"),
        Action::CloseTab,
    );
    // Rename the active tab inline. Bare Ctrl+R must stay unbound:
    // the shell owns it for reverse history search.
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "r"),
        Action::RenameTab,
    );
    // Duplicate the active tab. Bare Ctrl+D must stay unbound: the
    // shell owns it for end-of-file (EOF).
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "d"),
        Action::DuplicateTab,
    );
    map.insert(
        bind_named(&[Modifier::Ctrl], Named::PageUp),
        Action::PrevTab,
    );
    map.insert(
        bind_named(&[Modifier::Ctrl], Named::PageDown),
        Action::NextTab,
    );
    // Browser-style cycling (Firefox): Ctrl+Tab / Ctrl+Shift+Tab walk
    // forward and backward through the tab list, alongside the
    // Ctrl+PageUp/PageDown binds above.
    map.insert(bind_named(&[Modifier::Ctrl], Named::Tab), Action::NextTab);
    map.insert(
        bind_named(&[Modifier::Ctrl, Modifier::Shift], Named::Tab),
        Action::PrevTab,
    );
    // Numbered activation and keyboard moves, cosmic-term's exact
    // binds: digits 1-8 activate that tab strictly, 9 always takes
    // the last tab; the moves shift the active tab one slot and clamp
    // at the edges. Bare Ctrl+digits stay unbound (only =/-/0 zoom).
    for (digit, index) in ('1'..='8').zip(0..8) {
        map.insert(
            bind(&[Modifier::Ctrl, Modifier::Shift], &digit.to_string()),
            Action::ActivateTab(index),
        );
    }
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "9"),
        Action::ActivateLastTab,
    );
    map.insert(
        bind_named(&[Modifier::Ctrl, Modifier::Shift], Named::PageUp),
        Action::MoveTabLeft,
    );
    map.insert(
        bind_named(&[Modifier::Ctrl, Modifier::Shift], Named::PageDown),
        Action::MoveTabRight,
    );
    map
}

/// Whether the given key press matches a bound [`Action`], returning it if so.
///
/// `physical_key` should be forwarded from the same key-press event as
/// `key`: it lets a bind still match on non-Latin keyboard layouts, the
/// same fallback `TerminalBox` itself uses to decide which presses to
/// swallow instead of sending to the shell (see `KeyBind::matches`).
/// Passing `None` here while the widget passes `Some` would let it swallow
/// a key press that this function then fails to match, silently eating
/// the keystroke.
pub fn matched_action(
    key_binds: &HashMap<KeyBind, Action>,
    modifiers: Modifiers,
    key: &Key,
    physical_key: Option<&Physical>,
) -> Option<Action> {
    key_binds
        .iter()
        .find(|(bind, _)| bind.matches(modifiers, key, physical_key))
        .map(|(_, action)| *action)
}

/// The serde-friendly modifier names a persisted [`Binding`] carries —
/// cosmic's own [`Modifier`] is not serializable, so the config shape
/// spells its own four and converts at the boundary
/// ([`Binding::to_key_bind`]). Ported from cosmic-term.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ModifierName {
    Ctrl,
    Shift,
    Alt,
    Super,
}

impl ModifierName {
    fn to_modifier(self) -> Modifier {
        match self {
            Self::Ctrl => Modifier::Ctrl,
            Self::Shift => Modifier::Shift,
            Self::Alt => Modifier::Alt,
            Self::Super => Modifier::Super,
        }
    }
}

/// A persisted key binding: modifier names plus a key name string
/// (`"C"`, `"Tab"`, `"PageUp"`, `"Space"`, …) — the config file's
/// shape, converted to a runtime [`KeyBind`] by [`to_key_bind`].
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Binding {
    pub modifiers: Vec<ModifierName>,
    pub key: String,
}

impl Binding {
    /// The runtime matching shape, or `None` when the key name does
    /// not parse (a hand-edited config can hold one).
    pub fn to_key_bind(&self) -> Option<KeyBind> {
        let key = key_from_string(&self.key)?;
        let mut modifiers = Vec::new();
        for modifier in [
            ModifierName::Ctrl,
            ModifierName::Shift,
            ModifierName::Alt,
            ModifierName::Super,
        ] {
            if self.modifiers.contains(&modifier) {
                modifiers.push(modifier.to_modifier());
            }
        }

        Some(KeyBind { modifiers, key })
    }
}

/// The ids of the numbered activate-tab rows: 1-based display number
/// at position `index` — the editor exposes one row per number, and
/// the id set ends at 8 (9 is [`Action::ActivateLastTab`], its own
/// row).
const ACTIVATE_TAB_IDS: [&str; 8] = [
    "activate_tab_1",
    "activate_tab_2",
    "activate_tab_3",
    "activate_tab_4",
    "activate_tab_5",
    "activate_tab_6",
    "activate_tab_7",
    "activate_tab_8",
];

/// The stable string id of an action — the `custom_key_binds` config
/// key and the file-format contract (pinned by test). `None` marks an
/// action that is deliberately not keybindable: only
/// [`Action::NewProfileTab`], whose indices are not a stable user
/// surface (its enum doc is the contract).
pub fn action_id(action: Action) -> Option<&'static str> {
    let id = match action {
        Action::Copy => "copy",
        Action::CopyPrimary => "copy_primary",
        Action::Paste => "paste",
        Action::PastePrimary => "paste_primary",
        Action::SelectAll => "select_all",
        Action::ClearScrollback => "clear_scrollback",
        Action::ZoomIn => "zoom_in",
        Action::ZoomOut => "zoom_out",
        Action::ZoomReset => "zoom_reset",
        Action::Hide => "hide",
        Action::NewTab => "new_tab",
        Action::NewTabHome => "new_tab_home",
        // The two index-actioned menu picks share the exclusion: a
        // config-list index is not a stable user-facing id.
        Action::NewProfileTab(_) | Action::SwitchProfileTab(_) => return None,
        Action::CloseTab => "close_tab",
        Action::NextTab => "next_tab",
        Action::PrevTab => "prev_tab",
        Action::ActivateTab(index) => return ACTIVATE_TAB_IDS.get(index).copied(),
        Action::ActivateLastTab => "activate_last_tab",
        Action::MoveTabLeft => "move_tab_left",
        Action::MoveTabRight => "move_tab_right",
        Action::RenameTab => "rename_tab",
        Action::DuplicateTab => "duplicate_tab",
        Action::Settings => "settings",
        Action::ToggleStatusBar => "toggle_status_bar",
    };
    Some(id)
}

/// The action an id string resolves to — the inverse of [`action_id`]
/// (round-trip pinned by test); `None` for anything else, including
/// out-of-range activate-tab numbers.
pub fn action_from_id(id: &str) -> Option<Action> {
    if let Some(number) = id.strip_prefix("activate_tab_") {
        let index = number.parse::<usize>().ok()?;
        // 1-based display number, at most 8 — 9 is ActivateLastTab,
        // its own id.
        if index == 0 || index > 8 {
            return None;
        }
        return Some(Action::ActivateTab(index - 1));
    }

    let action = match id {
        "copy" => Action::Copy,
        "copy_primary" => Action::CopyPrimary,
        "paste" => Action::Paste,
        "paste_primary" => Action::PastePrimary,
        "select_all" => Action::SelectAll,
        "clear_scrollback" => Action::ClearScrollback,
        "zoom_in" => Action::ZoomIn,
        "zoom_out" => Action::ZoomOut,
        "zoom_reset" => Action::ZoomReset,
        "hide" => Action::Hide,
        "new_tab" => Action::NewTab,
        "new_tab_home" => Action::NewTabHome,
        "close_tab" => Action::CloseTab,
        "next_tab" => Action::NextTab,
        "prev_tab" => Action::PrevTab,
        "activate_last_tab" => Action::ActivateLastTab,
        "move_tab_left" => Action::MoveTabLeft,
        "move_tab_right" => Action::MoveTabRight,
        "rename_tab" => Action::RenameTab,
        "duplicate_tab" => Action::DuplicateTab,
        "settings" => Action::Settings,
        "toggle_status_bar" => Action::ToggleStatusBar,
        _ => return None,
    };
    Some(action)
}

/// The effective map the terminal matches against: the defaults with
/// every customized action's chords replaced by its one custom
/// binding. Entries that fail to resolve — a stale id from a
/// hand-edited config, a key name that does not parse — are skipped
/// with a warn and leave the action's defaults in place; a custom
/// always wins the chord it lands on, so when two customs collide the
/// later id (the `BTreeMap`'s order) keeps the chord — one
/// deterministic winner, never a double binding.
pub fn effective_key_binds(custom: &BTreeMap<String, Binding>) -> HashMap<KeyBind, Action> {
    let resolved: Vec<(Action, KeyBind)> = custom
        .iter()
        .filter_map(|(id, binding)| {
            if let (Some(action), Some(key_bind)) = (action_from_id(id), binding.to_key_bind()) {
                Some((action, key_bind))
            } else {
                log::warn!("ignoring invalid custom key bind {id:?}: {binding:?}");
                None
            }
        })
        .collect();
    let customized: HashSet<Action> = resolved.iter().map(|(action, _)| *action).collect();

    let mut binds = default_key_binds();
    binds.retain(|_, action| !customized.contains(action));
    for (action, key_bind) in resolved {
        binds.insert(key_bind, action);
    }
    binds
}

/// A titled group of editor rows — the Keybindings tab's sections, in
/// display order.
pub struct ActionGroup {
    pub title: &'static str,
    pub actions: Vec<Action>,
}

/// The Keybindings tab's row inventory: every bindable action exactly
/// once, grouped for display (coverage pinned by test). The numbered
/// activate rows are one entry per digit, 1 through 8, with 9 as
/// [`Action::ActivateLastTab`].
pub fn action_groups() -> impl Iterator<Item = ActionGroup> {
    let activate_rows = (0..8).map(Action::ActivateTab);
    [
        ActionGroup {
            title: "Clipboard",
            actions: vec![
                Action::SelectAll,
                Action::Copy,
                Action::CopyPrimary,
                Action::Paste,
                Action::PastePrimary,
            ],
        },
        ActionGroup {
            title: "Tabs",
            actions: vec![
                Action::NewTab,
                Action::NewTabHome,
                Action::DuplicateTab,
                Action::CloseTab,
                Action::RenameTab,
                Action::NextTab,
                Action::PrevTab,
                Action::MoveTabLeft,
                Action::MoveTabRight,
            ]
            .into_iter()
            .chain(activate_rows)
            .chain([Action::ActivateLastTab])
            .collect(),
        },
        ActionGroup {
            title: "Zoom",
            actions: vec![Action::ZoomIn, Action::ZoomOut, Action::ZoomReset],
        },
        ActionGroup {
            title: "Other",
            actions: vec![
                Action::ClearScrollback,
                Action::Hide,
                Action::Settings,
                Action::ToggleStatusBar,
            ],
        },
    ]
    .into_iter()
}

/// The labels of the numbered activate rows: display number 1-based,
/// matching the id suffixes.
const ACTIVATE_TAB_LABELS: [&str; 8] = [
    "Activate tab 1",
    "Activate tab 2",
    "Activate tab 3",
    "Activate tab 4",
    "Activate tab 5",
    "Activate tab 6",
    "Activate tab 7",
    "Activate tab 8",
];

/// An action's row label — hardcoded English like every label in this
/// app.
pub fn action_label(action: Action) -> &'static str {
    match action {
        Action::Copy => "Copy",
        Action::CopyPrimary => "Copy (primary selection)",
        Action::Paste => "Paste",
        Action::PastePrimary => "Paste primary selection",
        Action::SelectAll => "Select all",
        Action::ClearScrollback => "Clear scrollback",
        Action::ZoomIn => "Zoom in",
        Action::ZoomOut => "Zoom out",
        Action::ZoomReset => "Zoom reset",
        Action::Hide => "Hide the overlay",
        Action::NewTab => "New tab",
        Action::NewTabHome => "New tab (home directory)",
        Action::NewProfileTab(_) => "New profile tab",
        Action::SwitchProfileTab(_) => "Switch profile tab",
        Action::CloseTab => "Close tab",
        Action::NextTab => "Next tab",
        Action::PrevTab => "Previous tab",
        Action::ActivateTab(index) => ACTIVATE_TAB_LABELS
            .get(index)
            .copied()
            .unwrap_or("Activate tab"),
        Action::ActivateLastTab => "Activate last tab",
        Action::MoveTabLeft => "Move tab left",
        Action::MoveTabRight => "Move tab right",
        Action::RenameTab => "Rename tab",
        Action::DuplicateTab => "Duplicate tab",
        Action::Settings => "Open settings",
        Action::ToggleStatusBar => "Toggle the status bar",
    }
}

/// The human form of a binding (`Ctrl + Shift + C`), via the runtime
/// [`KeyBind`]'s display; a key that does not parse falls back to the
/// raw key string.
pub fn binding_display(binding: &Binding) -> String {
    binding
        .to_key_bind()
        .map_or_else(|| binding.key.clone(), |key_bind| key_bind.to_string())
}

/// The binding a pressed chord captures — or `None` when the press is
/// not bindable: a bare modifier key (capture stays armed on those).
pub fn binding_from_key(modifiers: Modifiers, key: &Key) -> Option<Binding> {
    if is_modifier_only_key(key) {
        return None;
    }
    let key = key_to_string(key)?;
    let mut binding_modifiers = Vec::new();
    if modifiers.control() {
        binding_modifiers.push(ModifierName::Ctrl);
    }
    if modifiers.shift() {
        binding_modifiers.push(ModifierName::Shift);
    }
    if modifiers.alt() {
        binding_modifiers.push(ModifierName::Alt);
    }
    if modifiers.logo() {
        binding_modifiers.push(ModifierName::Super);
    }
    Some(Binding {
        modifiers: binding_modifiers,
        key,
    })
}

/// The verdict for a press while a capture is armed — the routing has
/// exactly three outcomes; there is no fall-through (capture consumes
/// every press, keybinds-editor criterion 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureVerdict {
    /// The chord to bind: commit it as the action's custom binding.
    Commit(Binding),
    /// Escape: end the capture, keep the bindings as they are.
    Cancel,
    /// A bare modifier (or a key that cannot parse): stay armed.
    Ignore,
}

/// Route a press for the armed capture — pure, so the routing policy
/// (Escape cancels even with modifiers, matching the drawer's own
/// Escape rule; only a real chord commits) is headless-testable. The
/// caller owns the ordering: this runs before the drawer's Escape rule
/// and before bind matching.
pub fn capture_verdict(modifiers: Modifiers, key: &Key) -> CaptureVerdict {
    if matches!(key, Key::Named(Named::Escape)) {
        return CaptureVerdict::Cancel;
    }
    binding_from_key(modifiers, key).map_or(CaptureVerdict::Ignore, CaptureVerdict::Commit)
}

/// The custom map with `action`'s entry set to `binding` — the
/// capture-commit planner: whole map in, whole map out, because the
/// write path persists maps whole. Re-setting an action replaces its
/// entry (single binding per action); an unkeybindable action — none
/// can arm, but the signature admits it — leaves the map unchanged.
pub fn with_custom_binding(
    custom: &BTreeMap<String, Binding>,
    action: Action,
    binding: Binding,
) -> BTreeMap<String, Binding> {
    let mut updated = custom.clone();
    if let Some(id) = action_id(action) {
        updated.insert(id.to_string(), binding);
    }
    updated
}

/// The row's binding text for the Keybindings tab: the action's
/// custom binding when it has one, else every default chord joined
/// (`" / "`, sorted for determinism), else "None" for actions that
/// ship unbound and have no custom.
pub fn action_binding_display(action: Action, custom: &BTreeMap<String, Binding>) -> String {
    if let Some(id) = action_id(action)
        && let Some(binding) = custom.get(id)
    {
        return binding_display(binding);
    }
    let mut chords: Vec<String> = default_key_binds()
        .into_iter()
        .filter(|(_, bound)| *bound == action)
        .map(|(key_bind, _)| key_bind.to_string())
        .collect();
    if chords.is_empty() {
        return "None".to_string();
    }
    chords.sort();
    chords.join(" / ")
}

/// Whether a row's label matches the Keybindings tab's search box —
/// case-insensitive substring; an empty query matches everything.
pub fn action_matches_search(action: Action, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    action_label(action)
        .to_lowercase()
        .contains(&query.to_lowercase())
}

/// What committing a captured chord would do — the pure half of the
/// capture-commit path (keybinds-editor criterion 4). The app writes
/// only on [`CommitVerdict::Commit`]; every other outcome is a disarm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitVerdict {
    /// The chord is free: commit it as the action's custom binding.
    Commit,
    /// The chord is already the action's effective binding: a silent
    /// disarm, nothing written (the spec's open-question-2 ruling).
    Own,
    /// Another action holds the chord effectively: nothing writes; the
    /// holder names it in the inline error row.
    Conflict(Action),
    /// The chord cannot round-trip into a runtime [`KeyBind`] (an
    /// empty key name — only a hand-edited config can hold one):
    /// disarm with a warn, never persist an entry resolution would
    /// skip.
    Unsupported,
}

/// The commit verdict for `action` capturing `binding`, given the
/// current custom map — resolved against the *effective* map, so a
/// default chord retired by someone's custom is free again and a
/// custom chord is as owned as a default.
pub fn commit_verdict(
    action: Action,
    binding: &Binding,
    custom: &BTreeMap<String, Binding>,
) -> CommitVerdict {
    let Some(key_bind) = binding.to_key_bind() else {
        return CommitVerdict::Unsupported;
    };
    match effective_key_binds(custom).get(&key_bind) {
        Some(holder) if *holder == action => CommitVerdict::Own,
        Some(holder) => CommitVerdict::Conflict(*holder),
        None => CommitVerdict::Commit,
    }
}

/// The custom map with `action`'s entry removed — the revert planner:
/// the action's defaults come back on the next rebuild (whole map in,
/// whole map out, like every write here).
pub fn without_custom_binding(
    custom: &BTreeMap<String, Binding>,
    action: Action,
) -> BTreeMap<String, Binding> {
    let mut updated = custom.clone();
    if let Some(id) = action_id(action) {
        updated.remove(id);
    }
    updated
}

/// The [`Key`] a persisted key name parses back to. Ported from
/// cosmic-term — the names it accepts are the format's vocabulary.
fn key_from_string(value: &str) -> Option<Key> {
    match value {
        "Backspace" => Some(Key::Named(Named::Backspace)),
        "Enter" => Some(Key::Named(Named::Enter)),
        "Escape" => Some(Key::Named(Named::Escape)),
        "Insert" => Some(Key::Named(Named::Insert)),
        "Delete" => Some(Key::Named(Named::Delete)),
        "Tab" => Some(Key::Named(Named::Tab)),
        "F1" => Some(Key::Named(Named::F1)),
        "F2" => Some(Key::Named(Named::F2)),
        "F3" => Some(Key::Named(Named::F3)),
        "F4" => Some(Key::Named(Named::F4)),
        "F5" => Some(Key::Named(Named::F5)),
        "F6" => Some(Key::Named(Named::F6)),
        "F7" => Some(Key::Named(Named::F7)),
        "F8" => Some(Key::Named(Named::F8)),
        "F9" => Some(Key::Named(Named::F9)),
        "F10" => Some(Key::Named(Named::F10)),
        "F11" => Some(Key::Named(Named::F11)),
        "F12" => Some(Key::Named(Named::F12)),
        "Home" => Some(Key::Named(Named::Home)),
        "End" => Some(Key::Named(Named::End)),
        "ArrowLeft" | "Left" => Some(Key::Named(Named::ArrowLeft)),
        "ArrowRight" | "Right" => Some(Key::Named(Named::ArrowRight)),
        "ArrowUp" | "Up" => Some(Key::Named(Named::ArrowUp)),
        "ArrowDown" | "Down" => Some(Key::Named(Named::ArrowDown)),
        "PageUp" => Some(Key::Named(Named::PageUp)),
        "PageDown" => Some(Key::Named(Named::PageDown)),
        "Space" | "space" => Some(Key::Character(" ".into())),
        // Single ASCII letters canonicalize to lowercase — the default
        // map's form. `KeyBind`'s derived `Eq`/`Hash` is case-sensitive
        // while its `matches` is not, so one canonical form is what
        // lets a custom land *on* the default chord it replaces
        // instead of beside it (two case-variants would both match
        // every press).
        _ if value.len() == 1 && value.chars().all(|ch| ch.is_ascii_alphabetic()) => {
            Some(Key::Character(value.to_lowercase().into()))
        }
        _ if !value.is_empty() => Some(Key::Character(value.into())),
        _ => None,
    }
}

/// The persisted key name for a pressed key — single ASCII letters
/// normalize to uppercase so `c` and `C` capture identically.
fn key_to_string(key: &Key) -> Option<String> {
    match key {
        Key::Character(c) => {
            if c == " " {
                Some("Space".to_string())
            } else if c.len() == 1 && c.chars().all(|ch| ch.is_ascii_alphabetic()) {
                Some(c.to_uppercase())
            } else {
                Some(c.to_string())
            }
        }
        Key::Named(named) => Some(format!("{named:?}")),
        Key::Unidentified => None,
    }
}

/// Whether a key press is only a modifier being held — never a
/// bindable chord by itself.
fn is_modifier_only_key(key: &Key) -> bool {
    matches!(
        key,
        Key::Named(
            Named::Alt
                | Named::AltGraph
                | Named::CapsLock
                | Named::Control
                | Named::Fn
                | Named::FnLock
                | Named::NumLock
                | Named::ScrollLock
                | Named::Shift
                | Named::Symbol
                | Named::SymbolLock
                | Named::Meta
                | Named::Hyper
                | Named::Super
        )
    )
}

#[cfg(test)]
mod tests;
