// SPDX-License-Identifier: GPL-3.0-only

//! The terminal context-menu model: plain data and pure functions —
//! the row list, labels, and the item→[`terminal::Action`] mapping.
//! Item picks land in the same `perform_action` path the keybind map
//! uses, so the menu routes to existing actions rather than
//! re-implementing them; `app.rs` builds the libcosmic menu from
//! [`trees`]. Order follows cosmic-term, Settings… last behind the
//! divider (GNOME HIG). Right-click opens the menu only in default
//! mode; in mouse-reporting mode it reaches the child process.

use cosmic::widget::menu::{self, action::MenuAction};

use crate::app;
use crate::keybinds;
use crate::terminal::Action;

/// One rendered row of the context menu: an actionable item, or the
/// divider separating the terminal actions from Settings….
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// A clickable item, rendered with its keybind as a hint.
    Item(Item),
    /// A horizontal separator.
    Divider,
}

/// A context-menu item. Every variant routes to an existing
/// [`terminal::Action`] with existing `perform_action` handling — the
/// menu introduces no behavior of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    NewTab,
    Copy,
    Paste,
    SelectAll,
    ClearScrollback,
    Settings,
}

/// Every row in display order. Settings is last, behind the divider —
/// cosmic-term's composition, matching the GNOME HIG "standard items
/// at the end" placement.
pub const ROWS: &[Row] = &[
    Row::Item(Item::NewTab),
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
            Item::Copy => Action::Copy,
            Item::Paste => Action::Paste,
            Item::SelectAll => Action::SelectAll,
            Item::ClearScrollback => Action::ClearScrollback,
            Item::Settings => Action::Settings,
        }
    }
}

/// Turns a menu pick into the app message that carries it: the same
/// entry the keybind path uses, so menu behavior cannot fork from
/// keybind behavior.
impl MenuAction for Action {
    type Message = app::Message;

    fn message(&self) -> Self::Message {
        app::Message::Action(*self)
    }
}

/// The menu as libcosmic trees, one per row of [`ROWS`], each item
/// rendered with its keybind as a hint (every menu action is bound in
/// `default_key_binds`). Called by the view only while the menu is
/// open.
pub fn trees() -> Vec<menu::Tree<app::Message>> {
    let items: Vec<menu::Item<Action, String>> = ROWS
        .iter()
        .map(|row| match row {
            Row::Item(item) => menu::Item::Button(item.label().to_owned(), None, item.action()),
            Row::Divider => menu::Item::Divider,
        })
        .collect();
    menu::items(&keybinds::default_key_binds(), items)
}

/// The trees the view hands `widget::context_menu` for the current
/// open state: `None` while closed — the wrapper is then a plain
/// pass-through of the terminal — and the full [`ROWS`] trees while
/// open, which arms the wrapper's right-release trigger. Holding the
/// open-state decision here keeps the view wiring free of menu logic.
pub fn open_trees(menu_open: bool) -> Option<Vec<menu::Tree<app::Message>>> {
    menu_open.then(trees)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_items() -> [Item; 6] {
        [
            Item::NewTab,
            Item::Copy,
            Item::Paste,
            Item::SelectAll,
            Item::ClearScrollback,
            Item::Settings,
        ]
    }

    #[test]
    fn rows_are_new_tab_copy_paste_select_all_clear_scrollback_divider_settings() {
        assert_eq!(
            ROWS,
            &[
                Row::Item(Item::NewTab),
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

    #[test]
    fn trees_yields_one_tree_per_row() {
        assert_eq!(trees().len(), ROWS.len());
    }

    #[test]
    fn open_trees_is_none_while_the_menu_is_closed() {
        // A closed menu supplies no trees, so `widget::context_menu`
        // renders and passes events through as a plain wrapper — the
        // pixel-identical closed view.
        assert!(open_trees(false).is_none());
    }

    #[test]
    fn open_trees_carries_every_row_while_the_menu_is_open() {
        let trees = open_trees(true).expect("the open menu supplies its trees");
        assert_eq!(trees.len(), ROWS.len());
    }
}
