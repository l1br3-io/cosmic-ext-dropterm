// SPDX-License-Identifier: GPL-3.0-only

//! The terminal context-menu's libcosmic front: the model lives in
//! [`dropterm_core::context_menu`] (moved there by the gpui-chrome
//! spec, framework-free); this module maps it onto libcosmic's menu
//! widgets — every item to the existing [`terminal::Action`] its
//! keybind produces, so picks land in the same `perform_action` path
//! the keybind map uses. `app`'s `tab_actions` module builds the
//! libcosmic menu from [`items`]. Order follows cosmic-term, with
//! `DropTerm`'s Rename Tab clustering after New Tab, and Settings…
//! last behind the divider (GNOME HIG). Right-click opens the menu
//! only in default mode; in mouse-reporting mode it reaches the
//! child process.

use cosmic::widget::menu;

pub use dropterm_core::context_menu::rows;
use dropterm_core::context_menu::{Item, Row};
// The tab-actions tests' historical `crate::context_menu::ROWS`
// reach: the bin itself composes through `rows`.
#[cfg(test)]
pub use dropterm_core::context_menu::ROWS;

use crate::profile::Profile;
use crate::terminal::Action;

/// The existing terminal action an item routes to — the same action
/// its keybind produces. The model's twin of the GPUI front's
/// dispatch map: one mapping per frontend, both over core's items.
pub fn action(item: Item) -> Action {
    match item {
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

/// The menu's item list, pre-keybind: one item per row of
/// [`rows(profiles)`]; the submenu slot assembles its children here —
/// the plain New Tab child first, then one button per profile in
/// config order, each carrying its index for pick-time resolution.
pub fn items(profiles: &[Profile]) -> Vec<menu::Item<Action, String>> {
    rows(profiles)
        .into_iter()
        .map(|row| match row {
            Row::Item(item) => menu::Item::Button(item.label().to_owned(), None, action(item)),
            Row::ProfilesFolder => {
                // The folder label repeats the slot's own label, so
                // the submenu reads as "New Tab ▸".
                let mut children = vec![menu::Item::Button(
                    Item::NewTab.label().to_owned(),
                    None,
                    action(Item::NewTab),
                )];
                children.extend(profiles.iter().enumerate().map(|(index, profile)| {
                    menu::Item::Button(profile.name.clone(), None, Action::NewProfileTab(index))
                }));
                menu::Item::Folder(Item::NewTab.label().to_owned(), children)
            }
            Row::SwitchProfileFolder => menu::Item::Folder(
                // One button per profile in config order,
                // index-actioned — the New Tab folder's pattern: the
                // index resolves at pick time, so a config edited
                // while the menu was open degrades to the warn+no-op
                // rather than switching to the wrong profile.
                "Switch Profile".to_owned(),
                profiles
                    .iter()
                    .enumerate()
                    .map(|(index, profile)| {
                        menu::Item::Button(
                            profile.name.clone(),
                            None,
                            Action::SwitchProfileTab(index),
                        )
                    })
                    .collect(),
            ),
            Row::Divider => menu::Item::Divider,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        for (item, expected_action) in expected {
            assert_eq!(
                action(item),
                expected_action,
                "{item:?} maps to the wrong action"
            );
        }
    }

    // --- the libcosmic menu assembly over core's rows ---

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
    fn items_with_profiles_fold_switch_profile_into_its_own_folder() {
        // The folder's children: one button per profile in config
        // order, index-actioned — the New Tab folder's pattern; the
        // plain New Tab folder beside it is unaffected.
        let profiles = [profile("ssh-web"), profile("htop")];
        let list = items(&profiles);
        assert_eq!(list.len(), ROWS.len() + 1, "one folder row added");
        let menu::Item::Folder(label, children) = &list[3] else {
            panic!("the slot after Duplicate Tab is the Switch Profile folder");
        };
        assert_eq!(label, "Switch Profile");
        assert_eq!(children.len(), 2, "one row per profile, no plain child");
        for (index, child) in children.iter().enumerate() {
            let menu::Item::Button(label, _, action) = child else {
                panic!("profile rows are buttons");
            };
            assert_eq!(label, profiles[index].name.as_str());
            assert_eq!(*action, Action::SwitchProfileTab(index));
        }
        // The New Tab folder keeps its own shape: the plain child
        // plus one row per profile.
        let menu::Item::Folder(_, new_tab_children) = &list[0] else {
            panic!("the New Tab slot stays the submenu");
        };
        assert_eq!(new_tab_children.len(), 3);
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
            ROWS.len() + 1,
            "the New Tab folder replaces its slot; the extra row is the Switch Profile folder"
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
        // The same profile names a row under the Switch Profile
        // folder too — the collision is tolerated at every level:
        // distinct folders, distinct actions.
        let menu::Item::Folder(switch_label, switch_children) = &list[3] else {
            panic!("the Switch Profile slot is a folder");
        };
        assert_eq!(switch_label, "Switch Profile");
        let menu::Item::Button(label, _, action) = &switch_children[0] else {
            panic!("the switch row is a button");
        };
        assert_eq!(label, "Copy");
        assert_eq!(*action, Action::SwitchProfileTab(0));
        // The static Copy row is untouched by the collision (past the
        // tab cluster and the switch folder, which precede the
        // clipboard cluster).
        let menu::Item::Button(label, _, action) = &list[4] else {
            panic!("the static Copy row is a button");
        };
        assert_eq!(label, "Copy");
        assert_eq!(*action, Action::Copy);
    }
}
