// SPDX-License-Identifier: GPL-3.0-only

//! The terminal context menu on GPUI (gpui-chrome Task 1, py-5
//! landing the profile folders): core's
//! [`dropterm::core::context_menu`] model presented through the Kit's
//! popover path — the same C2b-proven route the per-tab menu rides —
//! with every pick landing in the path its keybind uses: the pane's
//! four actions through the pane's own action entry (the keybind
//! funnel), the tab and settings verbs through the shell calls the
//! tab menu's rows already use. The profile folders (py-5) ride the
//! Kit's submenu item: the New Tab slot becomes a submenu — plain New
//! Tab first, then one row per profile in config order — exactly
//! while ≥1 profile exists, core's own flat-otherwise ruling.

pub(crate) use dropterm::core::context_menu::Item;
use dropterm::core::context_menu::Row;
use dropterm::core::profile::Profile;
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

/// What a composed row publishes on a press (py-5): a static item, or
/// one of the two profile-folder picks — the New Tab submenu's
/// profile row (launch the preset) and the Switch Profile submenu's
/// row (respawn the active tab under it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pick {
    Item(Item),
    /// Launch the profile at the config index as a new tab.
    NewProfileTab(usize),
    /// Respawn the active tab in place under the profile at the
    /// config index.
    SwitchProfile(usize),
}

/// The pick callback's shape: the row's pick, with the window and app
/// the Kit's click hands its rows.
pub(crate) type PickFn = std::rc::Rc<dyn Fn(Pick, &mut gpui::Window, &mut gpui::App)>;

/// One clickable static row routed through `pick`.
fn item_row(menu: PopupMenu, item: Item, pick: &PickFn) -> PopupMenu {
    let pick = std::rc::Rc::clone(pick);
    menu.item(
        PopupMenuItem::new(item.label()).on_click(move |_event, window, cx| {
            pick(Pick::Item(item), window, cx);
        }),
    )
}

/// The New Tab folder's submenu (py-5, core's `ProfilesFolder`
/// ruling): plain New Tab first — the default spawn, the `+`'s own
/// funnel — then one row per profile in config order, each launching
/// the preset.
fn new_tab_folder(profiles: &[Profile], pick: &PickFn, menu: PopupMenu) -> PopupMenu {
    let mut menu = menu;
    let plain = std::rc::Rc::clone(pick);
    menu = menu.item(PopupMenuItem::new(Item::NewTab.label()).on_click(
        move |_event, window, cx| {
            plain(Pick::Item(Item::NewTab), window, cx);
        },
    ));
    for (index, profile) in profiles.iter().enumerate() {
        let pick = std::rc::Rc::clone(pick);
        let label = profile.name.clone();
        menu = menu.item(
            PopupMenuItem::new(label).on_click(move |_event, window, cx| {
                pick(Pick::NewProfileTab(index), window, cx);
            }),
        );
    }
    menu
}

/// The Switch Profile folder's submenu (py-5, core's
/// `SwitchProfileFolder` ruling): one row per profile in config
/// order, each respawning the active tab under it — the pane the menu
/// opened on is the mounted, active one.
fn switch_profile_folder(profiles: &[Profile], pick: &PickFn, menu: PopupMenu) -> PopupMenu {
    let mut menu = menu;
    for (index, profile) in profiles.iter().enumerate() {
        let pick = std::rc::Rc::clone(pick);
        let label = profile.name.clone();
        menu = menu.item(
            PopupMenuItem::new(label).on_click(move |_event, window, cx| {
                pick(Pick::SwitchProfile(index), window, cx);
            }),
        );
    }
    menu
}

/// Compose core's rows into the Kit's popup menu (py-5: the profile
/// folders whole) — one clickable row per [`Item`], the divider
/// between the terminal actions and Settings…, every click routed
/// through `pick`; with ≥1 profile configured the New Tab slot
/// becomes the launch submenu and the Switch Profile submenu takes
/// its seat (core's own row composition — zero profiles keep the flat
/// menu byte-identical).
pub(crate) fn build_menu(
    menu: PopupMenu,
    profiles: &[Profile],
    pick: &PickFn,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> PopupMenu {
    let mut menu = menu;
    for row in dropterm::core::context_menu::rows(profiles) {
        match row {
            Row::Item(item) => menu = item_row(menu, item, pick),
            Row::Divider => menu = menu.separator(),
            Row::ProfilesFolder => {
                let folder = PopupMenu::build(window, cx, |menu, _window, _cx| {
                    new_tab_folder(profiles, pick, menu)
                });
                menu = menu.item(PopupMenuItem::submenu("New Tab", folder));
            }
            Row::SwitchProfileFolder => {
                let folder = PopupMenu::build(window, cx, |menu, _window, _cx| {
                    switch_profile_folder(profiles, pick, menu)
                });
                menu = menu.item(PopupMenuItem::submenu("Switch Profile", folder));
            }
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

    // --- the profile folders' composition (py-5) ---

    fn profile(name: &str) -> Profile {
        Profile {
            name: name.to_string(),
            command: format!("echo {name}"),
            cwd: None,
            color_scheme: None,
            env: None,
            font_size: None,
            scrollback_lines: None,
        }
    }

    #[test]
    fn the_flat_menu_composes_with_zero_profiles() {
        // Zero profiles keep today's menu byte-identical: core's rows
        // for an empty list are exactly the static items, so the
        // composition renders no submenu (core's own pin).
        let rows = dropterm::core::context_menu::rows(&[]);
        assert!(
            rows.iter()
                .all(|row| !matches!(row, Row::ProfilesFolder | Row::SwitchProfileFolder)),
            "the empty list keeps the flat menu"
        );
    }

    #[test]
    fn the_folders_compose_over_the_configured_list() {
        // ≥1 profile swaps the New Tab slot for the submenu and adds
        // the Switch Profile folder — core's composition, which the
        // Kit presentation follows one-for-one.
        let profiles = [profile("ssh-web"), profile("top")];
        let rows = dropterm::core::context_menu::rows(&profiles);
        assert!(rows.contains(&Row::ProfilesFolder));
        assert!(rows.contains(&Row::SwitchProfileFolder));
    }
}
