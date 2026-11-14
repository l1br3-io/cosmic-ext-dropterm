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
    BindingRow { id: "hide", label: "Hide the overlay", group: "Other",
        defaults: &[(CS, "Q")] },
    BindingRow { id: "settings", label: "Open settings", group: "Other",
        defaults: &[(CS, "I")] },
    BindingRow { id: "images_panel", label: "Open the images panel", group: "Other",
        defaults: &[(CS, "P")] },
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

/// The row's effective binding display: its custom entry if one
/// exists, else its defaults joined `" / "` in sorted order — the
/// editor row's binding text.
pub fn effective_binding_display(
    id: &str,
    custom: &std::collections::BTreeMap<String, Binding>,
) -> String {
    let Some(row) = binding_rows().iter().find(|row| row.id == id) else {
        return "None".to_string();
    };
    if let Some(binding) = custom.get(id) {
        return binding_display(binding);
    }
    let mut chords: Vec<String> = row
        .defaults
        .iter()
        .map(|(modifiers, key)| chord_display(modifiers, key))
        .collect();
    if chords.is_empty() {
        return "None".to_string();
    }
    chords.sort();
    chords.join(" / ")
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
    /// holder's id names it in the inline error.
    Conflict(&'static str),
    /// The chord cannot round-trip (an empty key name — only a
    /// hand-edited config can hold one): disarm, never persist.
    Unsupported,
}

/// One chord in persisted form equals a (modifiers, key) pair.
fn chord_eq(binding: &Binding, modifiers: &[ModifierName], key: &str) -> bool {
    binding.key == key && binding.modifiers.as_slice() == modifiers
}

/// The capture verdict for `id` capturing `binding`, given the
/// current custom map.
pub fn capture_verdict(
    id: &str,
    binding: &Binding,
    custom: &std::collections::BTreeMap<String, Binding>,
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
                CaptureVerdict::Conflict(row.id)
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
                &empty
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
                &empty
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
                &empty
            ),
            CaptureVerdict::Conflict("new_tab")
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
                &map
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
                &map
            ),
            CaptureVerdict::Conflict("new_tab")
        );
        // An empty key never persists.
        assert_eq!(
            capture_verdict(
                "copy",
                &Binding {
                    modifiers: vec![],
                    key: String::new()
                },
                &empty
            ),
            CaptureVerdict::Unsupported
        );
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
}
