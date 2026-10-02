// SPDX-License-Identifier: GPL-3.0-only

//! The terminal context-menu model (context-menu module).
//!
//! Everything menu-shaped is plain data and pure functions here — the
//! row list, labels, and the item→[`terminal::Action`] mapping —
//! pinned by unit tests, while runtime behavior stays where it
//! already lives: item picks arrive as [`app::Message`] and land in
//! the same `perform_action` funnel the keybind map uses, so the menu
//! is a route to existing actions, never a second implementation of
//! them. The view wiring in `app.rs` composes libcosmic's
//! `widget::context_menu` from [`trees`] and contains no menu logic.
//!
//! Composition and order follow cosmic-term's terminal context menu
//! (research §6), with Settings… last behind the divider — the GNOME
//! HIG "standard items at the end" placement. Right-click reaches the
//! menu only in default mode; in mouse-reporting mode (vim, htop) it
//! still reaches the child process, because the open signal is
//! published only from the widget's default-mode branch.

// Nothing here is called outside tests until the app wiring
// (context-menu module, Task 3) consumes `trees` in the view — the
// same API-parity allowance as `terminal_box` in `terminal/widget.rs`.
// Remove when Task 3 lands.
#![allow(dead_code)]

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
/// the cosmic-term composition (research §6; GNOME HIG "standard items
/// at the end").
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
    pub fn label(&self) -> &'static str {
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
    pub fn action(&self) -> Action {
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
/// funnel entry the keybind path uses, so menu behavior cannot fork
/// from keybind behavior.
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
}
