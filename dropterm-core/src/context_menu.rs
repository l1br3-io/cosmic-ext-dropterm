// SPDX-License-Identifier: GPL-3.0-only

//! The terminal context-menu model (moved here by the gpui-chrome
//! spec, framework-free): plain data and pure functions — the row
//! list, labels, and the profile-folder composition. Item picks land
//! in each frontend's `perform_action` path — the same path the
//! keybind map uses — so the menu routes to existing actions rather
//! than re-implementing them; each frontend builds its menu
//! presentation from [`rows`]. Order follows cosmic-term, with
//! `DropTerm`'s Rename Tab clustering after New Tab, and Settings…
//! last behind the divider (GNOME HIG). Right-click opens the menu
//! only in default mode; in mouse-reporting mode it reaches the
//! child process.

use crate::profile::Profile;

/// One rendered row of the context menu: an actionable item, the
/// profiles submenu (only while ≥1 profile exists), or the divider
/// separating the terminal actions from Settings….
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// A clickable item, rendered with its keybind as a hint.
    Item(Item),
    /// The New Tab slot while ≥1 profile exists: a submenu whose
    /// first child is the plain New Tab, followed by one row per
    /// profile in config order (ruling 2 — zero profiles keep
    /// today's flat menu).
    ProfilesFolder,
    /// The Switch Profile slot while ≥1 profile exists: a submenu of
    /// one row per profile in config order, each respawning the
    /// active tab under that profile. Rides the same non-empty
    /// ruling as the New Tab slot (zero profiles keep today's flat
    /// menu) and slots in after `DuplicateTab` — it acts on the active
    /// tab like the tab cluster it joins.
    SwitchProfileFolder,
    /// A horizontal separator.
    Divider,
}

/// A context-menu item. Every variant routes to an existing
/// terminal action with existing apply handling — the menu
/// introduces no behavior of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    NewTab,
    /// Start an inline rename of the active tab — the menu's twin of
    /// the `Ctrl+Shift+R` bind, down to the shared `start_rename`
    /// path and the below-two-tabs no-op.
    RenameTab,
    /// Duplicate the active tab — the menu's twin of the
    /// `Ctrl+Shift+D` bind: the same fresh-shell-in-its-cwd spawn,
    /// after-active landing, and rename-pin carry-over, all inherited
    /// from the shared `open_duplicate_tab` path.
    DuplicateTab,
    Copy,
    Paste,
    SelectAll,
    ClearScrollback,
    Settings,
}

/// Every row in display order. The tab cluster leads — New Tab,
/// Rename Tab, Duplicate Tab all act on tabs; Settings is last, behind
/// the divider — cosmic-term's composition, matching the GNOME HIG
/// "standard items at the end" placement.
pub const ROWS: &[Row] = &[
    Row::Item(Item::NewTab),
    Row::Item(Item::RenameTab),
    Row::Item(Item::DuplicateTab),
    Row::Item(Item::Copy),
    Row::Item(Item::Paste),
    Row::Item(Item::SelectAll),
    Row::Item(Item::ClearScrollback),
    Row::Divider,
    Row::Item(Item::Settings),
];

impl Item {
    /// The item's display label.
    pub fn label(self) -> &'static str {
        match self {
            Item::NewTab => "New Tab",
            Item::RenameTab => "Rename Tab",
            Item::DuplicateTab => "Duplicate Tab",
            Item::Copy => "Copy",
            Item::Paste => "Paste",
            Item::SelectAll => "Select All",
            Item::ClearScrollback => "Clear Scrollback",
            // The ellipsis (U+2026) marks an item that opens a window,
            // matching cosmic-term's "Settings…" label.
            Item::Settings => "Settings\u{2026}",
        }
    }
}

/// The rows for the current profile list: exactly [`ROWS`] when no
/// profile is configured — today's byte-identical flat menu, the
/// empty-list pin — else the New Tab slot becomes the submenu slot
/// (ruling 2).
pub fn rows(profiles: &[Profile]) -> Vec<Row> {
    if profiles.is_empty() {
        return ROWS.to_vec();
    }
    let mut with_profiles = Vec::with_capacity(ROWS.len() + 1);
    for row in ROWS {
        match row {
            Row::Item(Item::NewTab) => with_profiles.push(Row::ProfilesFolder),
            // The switch folder slots in after DuplicateTab, before
            // the clipboard cluster begins at Copy.
            Row::Item(Item::DuplicateTab) => {
                with_profiles.push(*row);
                with_profiles.push(Row::SwitchProfileFolder);
            }
            row => with_profiles.push(*row),
        }
    }
    with_profiles
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_items() -> [Item; 8] {
        [
            Item::NewTab,
            Item::RenameTab,
            Item::DuplicateTab,
            Item::Copy,
            Item::Paste,
            Item::SelectAll,
            Item::ClearScrollback,
            Item::Settings,
        ]
    }

    #[test]
    fn rows_are_new_tab_rename_tab_duplicate_tab_copy_paste_select_all_clear_scrollback_divider_settings()
     {
        // The tab cluster — New Tab, Rename Tab, Duplicate Tab — leads:
        // all three act on tabs, and the clipboard cluster follows.
        assert_eq!(
            ROWS,
            &[
                Row::Item(Item::NewTab),
                Row::Item(Item::RenameTab),
                Row::Item(Item::DuplicateTab),
                Row::Item(Item::Copy),
                Row::Item(Item::Paste),
                Row::Item(Item::SelectAll),
                Row::Item(Item::ClearScrollback),
                Row::Divider,
                Row::Item(Item::Settings),
            ]
        );
    }

    #[test]
    fn every_label_is_non_empty() {
        for item in all_items() {
            assert!(!item.label().is_empty(), "{item:?} has an empty label");
        }
    }

    #[test]
    fn labels_are_unique() {
        let items = all_items();
        for (i, left) in items.iter().enumerate() {
            for right in &items[i + 1..] {
                assert_ne!(
                    left.label(),
                    right.label(),
                    "{left:?} and {right:?} share the label {:?}",
                    left.label()
                );
            }
        }
    }

    #[test]
    fn settings_label_ends_with_an_ellipsis() {
        assert!(
            Item::Settings.label().ends_with('\u{2026}'),
            "the Settings label must end with U+2026, got {:?}",
            Item::Settings.label()
        );
    }

    #[test]
    fn every_item_appears_in_rows_exactly_once() {
        for item in all_items() {
            let occurrences = ROWS.iter().filter(|row| **row == Row::Item(item)).count();
            assert_eq!(
                occurrences, 1,
                "{item:?} missing from or duplicated in ROWS"
            );
        }
    }

    // --- the profiles submenu (ruling 2) ---

    fn profile(name: &str) -> Profile {
        Profile {
            name: name.to_owned(),
            command: format!("run {name}"),
            cwd: None,
            color_scheme: None,
            env: None,
            font_size: None,
            scrollback_lines: None,
        }
    }

    #[test]
    fn rows_with_no_profiles_is_exactly_the_static_rows() {
        // The empty-list pin: zero profiles render today's menu,
        // byte-identical — ruling 2's flat half.
        assert_eq!(rows(&[]), ROWS.to_vec());
    }

    #[test]
    fn rows_with_profiles_swap_the_new_tab_slot_for_the_submenu() {
        // One row beyond the static list: the New Tab slot swaps for
        // the submenu and the Switch Profile folder adds its own —
        // the placement test below pins the full sequence.
        let profiles = [profile("ssh-web")];
        let with_profiles = rows(&profiles);
        assert_eq!(with_profiles.len(), ROWS.len() + 1);
        assert_eq!(with_profiles[0], Row::ProfilesFolder);
        assert_eq!(with_profiles[1], Row::Item(Item::RenameTab));
        assert_eq!(with_profiles[2], Row::Item(Item::DuplicateTab));
    }

    #[test]
    fn rows_with_profiles_keep_the_tab_cluster_right_after_the_folder() {
        // The folder replaces only the New Tab slot: Rename Tab stays
        // the second row and Duplicate Tab the third, immediately
        // after the submenu.
        let profiles = [profile("ssh-web"), profile("htop")];
        let with_profiles = rows(&profiles);
        assert_eq!(with_profiles[1], Row::Item(Item::RenameTab));
        assert_eq!(with_profiles[2], Row::Item(Item::DuplicateTab));
    }

    #[test]
    fn rows_with_profiles_gain_the_switch_folder_after_duplicate_tab() {
        // Ruling 1: the Switch Profile folder slots in after
        // DuplicateTab — it acts on the active tab like the cluster
        // it joins — and before Copy; the pin is the full sequence,
        // not just the neighbors.
        let profiles = [profile("ssh-web")];
        let with = rows(&profiles);
        assert_eq!(with.len(), ROWS.len() + 1, "the switch folder adds one row");
        let duplicate = with
            .iter()
            .position(|row| *row == Row::Item(Item::DuplicateTab))
            .expect("Duplicate Tab stays in the list");
        assert_eq!(with[duplicate + 1], Row::SwitchProfileFolder);
        assert_eq!(with[duplicate + 2], Row::Item(Item::Copy));
        let mut expected = ROWS.to_vec();
        expected[0] = Row::ProfilesFolder;
        expected.insert(3, Row::SwitchProfileFolder);
        assert_eq!(with, expected);
    }
}
