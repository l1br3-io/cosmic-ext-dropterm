// SPDX-License-Identifier: GPL-3.0-only

//! The terminal context-menu model: plain data and pure functions —
//! the row list, labels, and the item→[`terminal::Action`] mapping.
//! Item picks land in the same `perform_action` path the keybind map
//! uses, so the menu routes to existing actions rather than
//! re-implementing them; `app`'s `tab_actions` module builds the
//! libcosmic menu from [`items`]. Order follows cosmic-term, with
//! `DropTerm`'s Rename Tab
//! clustering after New Tab, and Settings… last behind the divider
//! (GNOME HIG). Right-click opens the menu only in default mode; in
//! mouse-reporting mode it reaches the child process.

use cosmic::widget::menu;

use crate::profile::Profile;
use crate::terminal::Action;

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
    /// A horizontal separator.
    Divider,
}

/// A context-menu item. Every variant routes to an existing
/// [`terminal::Action`] with existing `perform_action` handling — the
/// menu introduces no behavior of its own.
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

    /// The existing terminal action this item routes to — the same
    /// action its keybind produces.
    pub fn action(self) -> Action {
        match self {
            Item::NewTab => Action::NewTab,
            Item::RenameTab => Action::RenameTab,
            Item::DuplicateTab => Action::DuplicateTab,
            Item::Copy => Action::Copy,
            Item::Paste => Action::Paste,
            Item::SelectAll => Action::SelectAll,
            Item::ClearScrollback => Action::ClearScrollback,
            Item::Settings => Action::Settings,
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
    ROWS.iter()
        .map(|row| match row {
            Row::Item(Item::NewTab) => Row::ProfilesFolder,
            row => *row,
        })
        .collect()
}

/// The menu's item list, pre-keybind: one item per row of
/// [`rows(profiles)`]; the submenu slot assembles its children here —
/// the plain New Tab child first, then one button per profile in
/// config order, each carrying its index for pick-time resolution.
pub fn items(profiles: &[Profile]) -> Vec<menu::Item<Action, String>> {
    rows(profiles)
        .into_iter()
        .map(|row| match row {
            Row::Item(item) => menu::Item::Button(item.label().to_owned(), None, item.action()),
            Row::ProfilesFolder => {
                // The folder label repeats the slot's own label, so
                // the submenu reads as "New Tab ▸".
                let mut children = vec![menu::Item::Button(
                    Item::NewTab.label().to_owned(),
                    None,
                    Item::NewTab.action(),
                )];
                children.extend(profiles.iter().enumerate().map(|(index, profile)| {
                    menu::Item::Button(profile.name.clone(), None, Action::NewProfileTab(index))
                }));
                menu::Item::Folder(Item::NewTab.label().to_owned(), children)
            }
            Row::Divider => menu::Item::Divider,
        })
        .collect()
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

    #[test]
    fn item_actions_map_to_their_keybound_terminal_actions() {
        let expected = [
            (Item::NewTab, Action::NewTab),
            (Item::RenameTab, Action::RenameTab),
            (Item::DuplicateTab, Action::DuplicateTab),
            (Item::Copy, Action::Copy),
            (Item::Paste, Action::Paste),
            (Item::SelectAll, Action::SelectAll),
            (Item::ClearScrollback, Action::ClearScrollback),
            (Item::Settings, Action::Settings),
        ];
        for (item, action) in expected {
            assert_eq!(item.action(), action, "{item:?} maps to the wrong action");
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
        let profiles = [profile("ssh-web")];
        let with_profiles = rows(&profiles);
        assert_eq!(with_profiles.len(), ROWS.len());
        assert_eq!(with_profiles[0], Row::ProfilesFolder);
        assert_eq!(&with_profiles[1..], &ROWS[1..]);
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
    fn items_with_no_profiles_stay_flat() {
        // No folder unless a profile exists: the first item is the
        // plain New Tab button, exactly today's shape.
        let list = items(&[]);
        let menu::Item::Button(label, _, action) = &list[0] else {
            panic!("zero profiles must keep the flat New Tab button");
        };
        assert_eq!(label, "New Tab");
        assert_eq!(*action, Action::NewTab);
    }

    #[test]
    fn items_with_profiles_fold_new_tab_into_a_folder_with_plain_child_first() {
        let profiles = [profile("ssh-web"), profile("htop")];
        let list = items(&profiles);
        assert_eq!(
            list.len(),
            ROWS.len(),
            "the folder replaces the slot, it does not add a row"
        );
        let menu::Item::Folder(label, children) = &list[0] else {
            panic!("the New Tab slot must become the submenu");
        };
        assert_eq!(label, "New Tab");
        assert_eq!(children.len(), 3, "plain New Tab plus one row per profile");
        let menu::Item::Button(label, _, action) = &children[0] else {
            panic!("the folder's first child is the plain New Tab");
        };
        assert_eq!(label, "New Tab");
        assert_eq!(*action, Action::NewTab);
        // One row per profile, config order, index-actioned.
        for (index, child) in children[1..].iter().enumerate() {
            let menu::Item::Button(label, _, action) = child else {
                panic!("profile rows are buttons");
            };
            assert_eq!(label, profiles[index].name.as_str());
            assert_eq!(*action, Action::NewProfileTab(index));
        }
    }

    #[test]
    fn a_profile_named_like_a_static_item_tolerates_the_label_collision() {
        // "Copy" names both a static row and a profile row — different
        // menu levels, different actions; the menu tolerates the
        // duplicate rather than rejecting it.
        let profiles = [profile("Copy")];
        let list = items(&profiles);
        let menu::Item::Folder(_, children) = &list[0] else {
            panic!("the New Tab slot must become the submenu");
        };
        let menu::Item::Button(label, _, action) = &children[1] else {
            panic!("the profile row is a button");
        };
        assert_eq!(label, "Copy");
        assert_eq!(*action, Action::NewProfileTab(0));
        // The static Copy row is untouched by the collision (one row
        // past the tab cluster, which precedes the clipboard
        // cluster).
        let menu::Item::Button(label, _, action) = &list[3] else {
            panic!("the static Copy row is a button");
        };
        assert_eq!(label, "Copy");
        assert_eq!(*action, Action::Copy);
    }
}
