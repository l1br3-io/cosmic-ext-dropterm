// SPDX-License-Identifier: GPL-3.0-only

//! The terminal context menu on GPUI (gpui-chrome Task 1): core's
//! [`dropterm_core::context_menu`] model presented through the Kit's
//! popover path — the same C2b-proven route the per-tab menu rides —
//! with every pick landing in the path its keybind uses: the pane's
//! four actions through the pane's own action entry (the keybind
//! funnel), the tab and settings verbs through the shell calls the
//! tab menu's rows already use. The model introduces no behavior;
//! this module maps [`Item`]s onto those existing paths.
//!
//! The profile folders stay honestly absent on GPUI: the front
//! composes the model over an empty profile list — the flat shape —
//! until a profile spawn path exists (the gpui-tabs scope-note
//! precedent; core's folder logic is fully pinned by its own tests,
//! so the front flips one input when that path lands).

pub(crate) use dropterm_core::context_menu::Item;
use dropterm_core::context_menu::Row;
use dropterm_core::profile::Profile;
use gpui_component::menu::{PopupMenu, PopupMenuItem};

use crate::terminal::input::TermAction;

/// The shell verb a pick routes to — one variant per shell call the
/// tab menu's rows and the gear already make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellPick {
    NewTab,
    RenameTab,
    DuplicateTab,
    Settings,
}

/// Where a menu item's pick lands: the pane's action path (the same
/// `perform_action` the keybind map drives) or a shell call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PickRoute {
    /// The pane's own action — the keybind funnel.
    Pane(TermAction),
    /// A shell-level verb, the tab menu's route.
    Shell(ShellPick),
}

/// The item→path mapping: the GPUI twin of the libcosmic front's
/// `action(Item)` — each frontend maps core's items onto the actions
/// its own paths carry.
pub(crate) fn route(item: Item) -> PickRoute {
    match item {
        Item::NewTab => PickRoute::Shell(ShellPick::NewTab),
        Item::RenameTab => PickRoute::Shell(ShellPick::RenameTab),
        Item::DuplicateTab => PickRoute::Shell(ShellPick::DuplicateTab),
        Item::Copy => PickRoute::Pane(TermAction::Copy),
        Item::Paste => PickRoute::Pane(TermAction::Paste),
        Item::SelectAll => PickRoute::Pane(TermAction::SelectAll),
        Item::ClearScrollback => PickRoute::Pane(TermAction::ClearScrollback),
        Item::Settings => PickRoute::Shell(ShellPick::Settings),
    }
}

/// The pick callback's shape: the item picked, with the window and
/// app the Kit's click hands its rows.
pub(crate) type PickFn = std::rc::Rc<dyn Fn(Item, &mut gpui::Window, &mut gpui::App)>;

/// Compose core's rows into the Kit's popup menu — one clickable row
/// per [`Item`], the divider between the terminal actions and
/// Settings…, every click routed through `pick`. The profile folders
/// stay absent on GPUI until a profile spawn path exists (the
/// module's scope note): the front composes over the empty list, so
/// the folder arms never render — they are skipped, not dropped from
/// the model, which core's own tests keep pinning.
pub(crate) fn build_menu(menu: PopupMenu, profiles: &[Profile], pick: &PickFn) -> PopupMenu {
    let mut menu = menu;
    for row in dropterm_core::context_menu::rows(profiles) {
        match row {
            Row::Item(item) => {
                let pick = std::rc::Rc::clone(pick);
                menu = menu.item(PopupMenuItem::new(item.label()).on_click(
                    move |_event, window, cx| {
                        pick(item, window, cx);
                    },
                ));
            }
            Row::Divider => menu = menu.separator(),
            // Unreachable while the front passes the empty profile
            // list; the arms stay so the composition is exhaustive
            // against core's model the day the spawn path lands.
            Row::ProfilesFolder | Row::SwitchProfileFolder => {}
        }
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_routes_to_the_path_its_keybind_uses() {
        // The clipboard/scrollback cluster lands in the pane's action
        // path — the exact TermAction its chord produces; the tab and
        // settings verbs land in the shell calls the tab menu uses.
        let expected = [
            (Item::NewTab, PickRoute::Shell(ShellPick::NewTab)),
            (Item::RenameTab, PickRoute::Shell(ShellPick::RenameTab)),
            (
                Item::DuplicateTab,
                PickRoute::Shell(ShellPick::DuplicateTab),
            ),
            (Item::Copy, PickRoute::Pane(TermAction::Copy)),
            (Item::Paste, PickRoute::Pane(TermAction::Paste)),
            (Item::SelectAll, PickRoute::Pane(TermAction::SelectAll)),
            (
                Item::ClearScrollback,
                PickRoute::Pane(TermAction::ClearScrollback),
            ),
            (Item::Settings, PickRoute::Shell(ShellPick::Settings)),
        ];
        for (item, route) in expected {
            assert_eq!(super::route(item), route, "{item:?} routes wrong");
        }
    }
}
