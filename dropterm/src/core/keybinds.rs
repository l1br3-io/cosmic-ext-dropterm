// SPDX-License-Identifier: GPL-3.0-only

//! The persisted keybind model — the config file's shape (the
//! `keybinds.ron` section's binding type). The libcosmic action
//! mapping — default chords, `Action` ids, and the runtime `KeyBind`
//! conversion — stays in the terminal frontend's `keybinds` module,
//! which re-exports these types: the model is shared, the
//! toolkit-typed half is not (shared-core spec).

use serde::{Deserialize, Serialize};

/// The serde-friendly modifier names a persisted [`Binding`] carries —
/// cosmic's own `Modifier` is not serializable, so the config shape
/// spells its own four and the terminal frontend converts at the
/// boundary. Ported from cosmic-term.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ModifierName {
    Ctrl,
    Shift,
    Alt,
    Super,
}

/// A persisted key binding: modifier names plus a key name string
/// (`"C"`, `"Tab"`, `"PageUp"`, `"Space"`, …) — the config file's
/// shape. The terminal frontend converts it to its runtime matching
/// type, yielding nothing when the key name does not parse (a
/// hand-edited config can hold one).
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Binding {
    pub modifiers: Vec<ModifierName>,
    pub key: String,
}

// --- script-registered commands (script-commands spec) ---

/// The `custom_key_binds` namespace script commands bind under —
/// `script/<name>`. No builtin id carries the prefix (a file-format
/// contract pinned by test), so the namespace cannot collide.
pub const SCRIPT_ID_PREFIX: &str = "script/";

/// A command slug's length bound (the spec's "bounded length", pinned
/// here): the id becomes a persisted `custom_key_binds` key and a row
/// label, both of which want short names; descriptions stay unbounded
/// (the row layout truncates them).
pub const MAX_COMMAND_NAME_LEN: usize = 64;

/// Whether `name` is a registrable command slug: `[a-z0-9-]+`, at
/// least one character, at most [`MAX_COMMAND_NAME_LEN`]. The port
/// rejects anything else with one log line — the slug is a persisted
/// id, so the charset is part of the file-format contract.
pub fn valid_command_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_COMMAND_NAME_LEN
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

/// The persisted id a command binds under: `script/<name>`.
pub fn script_command_id(name: &str) -> String {
    format!("{SCRIPT_ID_PREFIX}{name}")
}

/// The command slug an id carries, or `None` when the id is not in
/// the script namespace (or carries no slug).
pub fn command_name_from_id(id: &str) -> Option<&str> {
    id.strip_prefix(SCRIPT_ID_PREFIX)
        .filter(|name| !name.is_empty())
}

/// One script-registered command, as the host's sync reports it and
/// the editor renders it: the slug (the registry key, the row's
/// label), the description shown under it, and — through [`Self::id`]
/// — the persisted `custom_key_binds` key the capture flow commits
/// under. A live command is runtime state, so the strings are owned;
/// a command ships unbound, its row's binding always a custom entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandRow {
    pub name: String,
    pub description: String,
}

impl CommandRow {
    /// The persisted id — `script/<name>` (the file-format contract).
    pub fn id(&self) -> String {
        script_command_id(&self.name)
    }
}

// --- the id-keyed binding registry (gpui-chrome) ---

/// One keybind-editor row: the action's stable id (the
/// `custom_key_binds` key, the file-format contract), its row label,
/// its group title, and its default chords in persisted form — the
/// registry both frontends' editors render from, one definition site
/// for the id↔chord↔label set the libcosmic frontend's
/// `default_key_binds`/`action_groups`/`action_label` tables hold.
pub struct BindingRow {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    /// The default chords, persisted-form key names (single ASCII
    /// letters uppercase, `Space` for the space bar, named keys in
    /// their canonical spellings).
    pub defaults: &'static [(&'static [ModifierName], &'static str)],
}

/// Every keybindable action's editor row, in display order — the
/// groups in `action_groups`' order, the rows in each group's order.
pub fn binding_rows() -> &'static [BindingRow] {
    BINDING_ROWS
}

const CS: &[ModifierName] = &[ModifierName::Ctrl, ModifierName::Shift];
const CTRL: &[ModifierName] = &[ModifierName::Ctrl];
const SHIFT: &[ModifierName] = &[ModifierName::Shift];

#[rustfmt::skip]
const BINDING_ROWS: &[BindingRow] = &[
    // Clipboard
    BindingRow { id: "select_all", label: "Select all", group: "Clipboard",
        defaults: &[(CS, "A")] },
    BindingRow { id: "copy", label: "Copy", group: "Clipboard",
        defaults: &[(CS, "C")] },
    // Copy (primary selection) ships unbound — the row reads None.
    BindingRow { id: "copy_primary", label: "Copy (primary selection)", group: "Clipboard",
        defaults: &[] },
    BindingRow { id: "paste", label: "Paste", group: "Clipboard",
        defaults: &[(CS, "V")] },
    BindingRow { id: "paste_primary", label: "Paste primary selection", group: "Clipboard",
        defaults: &[(SHIFT, "Insert")] },
    // Tabs
    BindingRow { id: "new_tab", label: "New tab", group: "Tabs",
        defaults: &[(CS, "T")] },
    BindingRow { id: "new_tab_home", label: "New tab (home directory)", group: "Tabs",
        defaults: &[(CS, "H")] },
    BindingRow { id: "duplicate_tab", label: "Duplicate tab", group: "Tabs",
        defaults: &[(CS, "D")] },
    BindingRow { id: "close_tab", label: "Close tab", group: "Tabs",
        defaults: &[(CS, "W")] },
    BindingRow { id: "rename_tab", label: "Rename tab", group: "Tabs",
        defaults: &[(CS, "R")] },
    BindingRow { id: "next_tab", label: "Next tab", group: "Tabs",
        defaults: &[(CTRL, "PageDown"), (CTRL, "Tab")] },
    BindingRow { id: "prev_tab", label: "Previous tab", group: "Tabs",
        defaults: &[(CTRL, "PageUp"), (CS, "Tab")] },
    BindingRow { id: "move_tab_left", label: "Move tab left", group: "Tabs",
        defaults: &[(CS, "PageUp")] },
    BindingRow { id: "move_tab_right", label: "Move tab right", group: "Tabs",
        defaults: &[(CS, "PageDown")] },
    BindingRow { id: "activate_tab_1", label: "Activate tab 1", group: "Tabs",
        defaults: &[(CS, "1")] },
    BindingRow { id: "activate_tab_2", label: "Activate tab 2", group: "Tabs",
        defaults: &[(CS, "2")] },
    BindingRow { id: "activate_tab_3", label: "Activate tab 3", group: "Tabs",
        defaults: &[(CS, "3")] },
    BindingRow { id: "activate_tab_4", label: "Activate tab 4", group: "Tabs",
        defaults: &[(CS, "4")] },
    BindingRow { id: "activate_tab_5", label: "Activate tab 5", group: "Tabs",
        defaults: &[(CS, "5")] },
    BindingRow { id: "activate_tab_6", label: "Activate tab 6", group: "Tabs",
        defaults: &[(CS, "6")] },
    BindingRow { id: "activate_tab_7", label: "Activate tab 7", group: "Tabs",
        defaults: &[(CS, "7")] },
    BindingRow { id: "activate_tab_8", label: "Activate tab 8", group: "Tabs",
        defaults: &[(CS, "8")] },
    BindingRow { id: "activate_last_tab", label: "Activate last tab", group: "Tabs",
        defaults: &[(CS, "9")] },
    // Zoom
    BindingRow { id: "zoom_in", label: "Zoom in", group: "Zoom",
        defaults: &[(CTRL, "="), (CTRL, "+"), (CS, "+"), (CS, "=")] },
    BindingRow { id: "zoom_out", label: "Zoom out", group: "Zoom",
        defaults: &[(CTRL, "-"), (CS, "-")] },
    BindingRow { id: "zoom_reset", label: "Zoom reset", group: "Zoom",
        defaults: &[(CTRL, "0")] },
    // Other
    BindingRow { id: "clear_scrollback", label: "Clear scrollback", group: "Other",
        defaults: &[(CS, "K")] },
    // The scrolled chip's jump verb (the band's one deliberate
    // action chip) as a chord: the click has a keyboard sibling.
    // Ctrl+Shift+End stays clear of the widget-layer Shift+End
    // scroll the key translation already owns.
    BindingRow { id: "scroll_to_bottom", label: "Scroll to bottom", group: "Other",
        defaults: &[(CS, "End")] },
    BindingRow { id: "hide", label: "Hide the overlay", group: "Other",
        defaults: &[(CS, "Q")] },
    BindingRow { id: "settings", label: "Open settings", group: "Other",
        defaults: &[(CS, "I")] },
    // The command palette's chord (command-palette spec): the peers'
    // P — `images_panel` moved to O (one chord, one action; the
    // registry's `capture_verdict` refuses a held chord).
    BindingRow { id: "command_palette", label: "Open the command palette", group: "Other",
        defaults: &[(CS, "P")] },
    BindingRow { id: "images_panel", label: "Open the images panel", group: "Other",
        defaults: &[(CS, "O")] },
    BindingRow { id: "toggle_status_bar", label: "Toggle the status bar", group: "Other",
        defaults: &[(CS, "B")] },
    BindingRow { id: "show_completions", label: "Show completions", group: "Other",
        defaults: &[(CTRL, "Space")] },
];

/// The human form of one chord — `Ctrl + Shift + C` — over the
/// persisted shape.
pub fn chord_display(modifiers: &[ModifierName], key: &str) -> String {
    let mut parts: Vec<String> = modifiers
        .iter()
        .map(|modifier| match modifier {
            ModifierName::Ctrl => "Ctrl".to_string(),
            ModifierName::Shift => "Shift".to_string(),
            ModifierName::Alt => "Alt".to_string(),
            ModifierName::Super => "Super".to_string(),
        })
        .collect();
    parts.push(key.to_string());
    parts.join(" + ")
}

/// A [`Binding`]'s human form (a key name that does not parse shows
/// the raw string, exactly the libcosmic display's fallback).
pub fn binding_display(binding: &Binding) -> String {
    chord_display(&binding.modifiers, &binding.key)
}

/// The row's effective binding in human form, or `None` when the row
/// is unbound or unknown — the Option-shaped half of the display
/// below, for consumers that render absence differently than the
/// editor's `"None"` sentinel (the palette's chord hints). The
/// custom check comes first for any id: script-command rows (which
/// have no defaults and never appear in [`binding_rows`]) serve
/// their custom entry through the same function.
pub fn effective_binding(
    id: &str,
    custom: &std::collections::BTreeMap<String, Binding>,
) -> Option<String> {
    if let Some(binding) = custom.get(id) {
        return Some(binding_display(binding));
    }
    let row = binding_rows().iter().find(|row| row.id == id)?;
    let mut chords: Vec<String> = row
        .defaults
        .iter()
        .map(|(modifiers, key)| chord_display(modifiers, key))
        .collect();
    if chords.is_empty() {
        return None;
    }
    chords.sort();
    Some(chords.join(" / "))
}

/// The row's effective binding display: its custom entry if one
/// exists, else its defaults joined `" / "` in sorted order — the
/// editor row's binding text. Unbound and unknown rows read `"None"`.
pub fn effective_binding_display(
    id: &str,
    custom: &std::collections::BTreeMap<String, Binding>,
) -> String {
    effective_binding(id, custom).unwrap_or_else(|| "None".to_string())
}

/// Whether a row matches the editor's search — case-insensitive
/// substring over the label; an empty query matches everything.
pub fn row_matches_search(row: &BindingRow, query: &str) -> bool {
    query.is_empty() || row.label.to_lowercase().contains(&query.to_lowercase())
}

/// What committing a captured chord would do — the pure half of the
/// capture-commit path, resolved against the effective map (a default
/// chord retired by someone's custom is free again, a custom chord as
/// owned as a default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureVerdict {
    /// The chord is free: commit it as the row's custom binding.
    Commit,
    /// The chord is already the row's own effective binding: a silent
    /// disarm, nothing written.
    Own,
    /// Another row holds the chord effectively: nothing writes; the
    /// holder's id names it in the inline error. Owned, because the
    /// holder may be a script-command row (`script/<name>`).
    Conflict(String),
    /// The chord cannot round-trip (an empty key name — only a
    /// hand-edited config can hold one): disarm, never persist.
    Unsupported,
}

/// One chord in persisted form equals a (modifiers, key) pair.
fn chord_eq(binding: &Binding, modifiers: &[ModifierName], key: &str) -> bool {
    binding.key == key && binding.modifiers.as_slice() == modifiers
}

/// The capture verdict for `id` capturing `binding`, given the
/// current custom map and the live script-command rows (which hold
/// chords through their custom entries only — a command ships
/// unbound).
pub fn capture_verdict(
    id: &str,
    binding: &Binding,
    custom: &std::collections::BTreeMap<String, Binding>,
    commands: &[CommandRow],
) -> CaptureVerdict {
    if binding.key.is_empty() {
        return CaptureVerdict::Unsupported;
    }
    // A custom binding replaces ALL of its row's defaults; defaults
    // stay live only for rows without a custom entry.
    for row in binding_rows() {
        let holds = match custom.get(row.id) {
            Some(their_custom) => chord_eq(their_custom, &binding.modifiers, &binding.key),
            None => row
                .defaults
                .iter()
                .any(|(modifiers, key)| chord_eq(binding, modifiers, key)),
        };
        if holds {
            return if row.id == id {
                CaptureVerdict::Own
            } else {
                CaptureVerdict::Conflict(row.id.to_string())
            };
        }
    }
    for command in commands {
        let Some(their_custom) = custom.get(command.id().as_str()) else {
            continue;
        };
        if chord_eq(their_custom, &binding.modifiers, &binding.key) {
            return if command.id().as_str() == id {
                CaptureVerdict::Own
            } else {
                CaptureVerdict::Conflict(command.id())
            };
        }
    }
    CaptureVerdict::Commit
}

/// The custom map with `id`'s entry replaced by `binding` — the
/// commit planner (whole map in, whole map out).
pub fn with_custom_binding(
    custom: &std::collections::BTreeMap<String, Binding>,
    id: &str,
    binding: Binding,
) -> std::collections::BTreeMap<String, Binding> {
    let mut updated = custom.clone();
    updated.insert(id.to_string(), binding);
    updated
}

/// The custom map with `id`'s entry removed — the revert planner:
/// the row's defaults come back on the next reload.
pub fn without_custom_binding(
    custom: &std::collections::BTreeMap<String, Binding>,
    id: &str,
) -> std::collections::BTreeMap<String, Binding> {
    let mut updated = custom.clone();
    updated.remove(id);
    updated
}

/// The persisted key name a pressed key captures, or `None` when the
/// key is not bindable: a bare modifier (capture stays armed) or an
/// empty name. Single ASCII letters normalize to uppercase (`c` and
/// `C` capture identically), the space bar persists as `Space`.
pub fn capture_key_name(key: &str) -> Option<String> {
    let canonical = match key {
        " " | "space" => "Space".to_string(),
        _ if key.len() == 1 && key.chars().all(|ch| ch.is_ascii_alphabetic()) => key.to_uppercase(),
        _ if !key.is_empty() => key.to_string(),
        _ => return None,
    };
    let is_modifier = matches!(
        canonical.as_str(),
        "control" | "ctrl" | "shift" | "alt" | "super" | "meta" | "logo"
    );
    (!is_modifier).then_some(canonical)
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    fn custom(
        entries: &[(&str, &[ModifierName], &str)],
    ) -> std::collections::BTreeMap<String, Binding> {
        entries
            .iter()
            .map(|(id, modifiers, key)| {
                (
                    (*id).to_string(),
                    Binding {
                        modifiers: modifiers.to_vec(),
                        key: (*key).to_string(),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn every_row_id_is_unique_and_the_groups_cover_the_set() {
        let rows = binding_rows();
        let mut ids: Vec<&str> = rows.iter().map(|row| row.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate row ids");
        // Four groups, every row in one, the groups in display order.
        let groups: Vec<&str> = rows.iter().map(|row| row.group).collect();
        let mut seen: Vec<&str> = Vec::new();
        for group in &groups {
            assert!(
                seen.last() == Some(group) || !seen.contains(group),
                "group {group} is not contiguous"
            );
            if seen.last() != Some(group) {
                seen.push(group);
            }
        }
        assert_eq!(seen, vec!["Clipboard", "Tabs", "Zoom", "Other"]);
    }

    #[test]
    fn the_copy_primary_row_ships_unbound_and_displays_none() {
        let row = binding_rows()
            .iter()
            .find(|row| row.id == "copy_primary")
            .unwrap();
        assert!(row.defaults.is_empty());
        assert_eq!(
            effective_binding_display("copy_primary", &std::collections::BTreeMap::default()),
            "None"
        );
    }

    #[test]
    fn the_scroll_to_bottom_row_defaults_to_ctrl_shift_end() {
        let row = binding_rows()
            .iter()
            .find(|row| row.id == "scroll_to_bottom")
            .unwrap();
        assert_eq!(row.group, "Other");
        assert_eq!(
            effective_binding_display("scroll_to_bottom", &std::collections::BTreeMap::default()),
            "Ctrl + Shift + End"
        );
    }

    #[test]
    fn effective_binding_is_the_option_shaped_twin_of_the_display() {
        let empty = std::collections::BTreeMap::default();
        // A bound row carries its joined defaults.
        assert_eq!(
            effective_binding("copy", &empty),
            Some("Ctrl + Shift + C".to_string())
        );
        // The unbound row is None — the display's "None" sentinel,
        // Option-shaped.
        assert_eq!(effective_binding("copy_primary", &empty), None);
        // A custom binding wins wholesale.
        let map = custom(&[("copy", &[ModifierName::Alt], "X")]);
        assert_eq!(effective_binding("copy", &map), Some("Alt + X".to_string()));
        // An unknown id is None, matching the display's sentinel.
        assert_eq!(effective_binding("no_such_row", &empty), None);
    }

    #[test]
    fn every_default_chord_collides_with_nothing() {
        // The registry is one-chord-one-action: every row's every
        // default chord resolves through the capture gate as Commit
        // (a free chord) or Own (the row's own), never Conflict —
        // the palette row's addition re-runs the whole registry
        // through the same gate.
        let empty = std::collections::BTreeMap::default();
        for row in binding_rows() {
            for (modifiers, key) in row.defaults {
                let binding = Binding {
                    modifiers: modifiers.to_vec(),
                    key: (*key).to_string(),
                };
                let verdict = capture_verdict(row.id, &binding, &empty, &[]);
                assert!(
                    matches!(verdict, CaptureVerdict::Commit | CaptureVerdict::Own),
                    "{}'s default {modifiers:?}+{key} conflicts: {verdict:?}",
                    row.id
                );
            }
        }
    }

    #[test]
    fn the_command_palette_row_defaults_to_ctrl_shift_p() {
        let row = binding_rows()
            .iter()
            .find(|row| row.id == "command_palette")
            .expect("the palette row is in the registry");
        assert_eq!(row.group, "Other");
        assert_eq!(
            effective_binding_display("command_palette", &std::collections::BTreeMap::default()),
            "Ctrl + Shift + P"
        );
    }

    #[test]
    fn effective_display_prefers_the_custom_and_joins_defaults_sorted() {
        // The default display: Ctrl + Shift + C.
        assert_eq!(
            effective_binding_display("copy", &std::collections::BTreeMap::default()),
            "Ctrl + Shift + C"
        );
        // The zoom cluster's four chords join sorted.
        let zoom = effective_binding_display("zoom_in", &std::collections::BTreeMap::default());
        assert!(zoom.contains(" / "), "multiple defaults join: {zoom}");
        // A custom wins wholesale.
        let map = custom(&[("copy", &[ModifierName::Alt], "X")]);
        assert_eq!(effective_binding_display("copy", &map), "Alt + X");
    }

    #[test]
    fn capture_verdicts_resolve_against_the_effective_map() {
        let empty = std::collections::BTreeMap::default();
        // A free chord commits.
        assert_eq!(
            capture_verdict(
                "copy",
                &Binding {
                    modifiers: vec![ModifierName::Alt],
                    key: "X".into()
                },
                &empty,
                &[],
            ),
            CaptureVerdict::Commit
        );
        // A row's own default is Own.
        assert_eq!(
            capture_verdict(
                "copy",
                &Binding {
                    modifiers: vec![ModifierName::Ctrl, ModifierName::Shift],
                    key: "C".into()
                },
                &empty,
                &[],
            ),
            CaptureVerdict::Own
        );
        // Another row's default conflicts, the holder named.
        assert_eq!(
            capture_verdict(
                "copy",
                &Binding {
                    modifiers: vec![ModifierName::Ctrl, ModifierName::Shift],
                    key: "T".into()
                },
                &empty,
                &[],
            ),
            CaptureVerdict::Conflict("new_tab".to_string())
        );
        // A custom chord is as owned as a default, and the default it
        // retired is free again.
        let map = custom(&[("new_tab", &[ModifierName::Alt], "N")]);
        assert_eq!(
            capture_verdict(
                "copy",
                &Binding {
                    modifiers: vec![ModifierName::Ctrl, ModifierName::Shift],
                    key: "T".into()
                },
                &map,
                &[],
            ),
            CaptureVerdict::Commit,
            "the retired default is free"
        );
        assert_eq!(
            capture_verdict(
                "copy",
                &Binding {
                    modifiers: vec![ModifierName::Alt],
                    key: "N".into()
                },
                &map,
                &[],
            ),
            CaptureVerdict::Conflict("new_tab".to_string())
        );
        // An empty key never persists.
        assert_eq!(
            capture_verdict(
                "copy",
                &Binding {
                    modifiers: vec![],
                    key: String::new()
                },
                &empty,
                &[],
            ),
            CaptureVerdict::Unsupported
        );
    }

    /// The defaults table in docs/keybindings.md names every registry
    /// row's action id — the docs gate's twin of
    /// `every_config_key_is_documented`: a registry row without a doc
    /// row fails here.
    #[test]
    fn every_binding_row_is_documented() {
        let docs = include_str!("../../../docs/keybindings.md");
        // The numbered activation family ships one collapsed doc row
        // (`activate_tab_1` … `activate_tab_8`), so the family's
        // endpoints stand in for its middle ids.
        let collapsed = |id: &str| {
            id.starts_with("activate_tab_")
                && docs.contains("`activate_tab_1`")
                && docs.contains("`activate_tab_8`")
        };
        for row in binding_rows() {
            let id = row.id;
            assert!(
                docs.contains(&format!("`{id}`")) || collapsed(id),
                "binding row `{id}` is not documented in docs/keybindings.md"
            );
        }
    }

    #[test]
    fn captured_key_names_normalize_the_file_format() {
        assert_eq!(capture_key_name("c").as_deref(), Some("C"));
        assert_eq!(capture_key_name("C").as_deref(), Some("C"));
        assert_eq!(capture_key_name(" ").as_deref(), Some("Space"));
        assert_eq!(capture_key_name("space").as_deref(), Some("Space"));
        assert_eq!(capture_key_name("PageUp").as_deref(), Some("PageUp"));
        // Bare modifiers stay armed (the capture keeps waiting).
        assert_eq!(capture_key_name("shift"), None);
        assert_eq!(capture_key_name(""), None);
    }

    // --- script-registered commands (script-commands spec, task SC1) ---

    #[test]
    fn command_slug_rules_and_the_id_namespace_round_trip() {
        // Valid slugs: lowercase letters, digits, hyphens.
        for name in ["paste-uptime", "a", "x9-y"] {
            assert!(valid_command_name(name), "{name:?} must be a valid slug");
        }
        // Invalid: empty, uppercase, spaces, underscores, slashes, and
        // past the length bound.
        let overlong = "x".repeat(MAX_COMMAND_NAME_LEN + 1);
        for name in [
            "",
            "Foo",
            "with space",
            "under_score",
            "slash/name",
            &overlong,
        ] {
            assert!(!valid_command_name(name), "{name:?} must reject");
        }
        assert!(
            valid_command_name(&"x".repeat(MAX_COMMAND_NAME_LEN)),
            "the bound itself is valid"
        );
        // The id namespace: prefix + slug, inverted by name lookup,
        // and uncollidable with every builtin id (none carries the
        // prefix).
        assert_eq!(script_command_id("paste-uptime"), "script/paste-uptime");
        assert_eq!(
            command_name_from_id("script/paste-uptime"),
            Some("paste-uptime")
        );
        assert_eq!(command_name_from_id("paste-uptime"), None);
        assert_eq!(command_name_from_id("script/"), None);
        for row in binding_rows() {
            assert!(
                !row.id.starts_with(SCRIPT_ID_PREFIX),
                "{} carries the script namespace",
                row.id
            );
        }
    }

    #[test]
    fn capture_verdicts_see_script_command_rows() {
        let commands = [CommandRow {
            name: "paste-uptime".to_owned(),
            description: "Paste system uptime into the pane".to_owned(),
        }];
        // The command row ships no defaults: unbound displays None,
        // and its custom entry is as owned as any builtin's.
        let empty = std::collections::BTreeMap::default();
        assert_eq!(
            effective_binding_display("script/paste-uptime", &empty),
            "None"
        );
        let map = custom(&[("script/paste-uptime", &[ModifierName::Alt], "U")]);
        assert_eq!(
            effective_binding_display("script/paste-uptime", &map),
            "Alt + U"
        );
        // A chord another row's script binding holds conflicts, the
        // holder named by its script id; a free chord commits for a
        // script id exactly as for a builtin (rows are generic).
        assert_eq!(
            capture_verdict(
                "copy",
                &Binding {
                    modifiers: vec![ModifierName::Alt],
                    key: "U".into()
                },
                &map,
                &commands,
            ),
            CaptureVerdict::Conflict("script/paste-uptime".to_string())
        );
        assert_eq!(
            capture_verdict(
                "script/paste-uptime",
                &Binding {
                    modifiers: vec![ModifierName::Ctrl, ModifierName::Alt],
                    key: "U".into()
                },
                &map,
                &commands,
            ),
            CaptureVerdict::Commit
        );
        // The command's own held chord is Own, the silent disarm.
        assert_eq!(
            capture_verdict(
                "script/paste-uptime",
                &Binding {
                    modifiers: vec![ModifierName::Alt],
                    key: "U".into()
                },
                &map,
                &commands,
            ),
            CaptureVerdict::Own
        );
    }
}
