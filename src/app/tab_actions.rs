// SPDX-License-Identifier: GPL-3.0-only

use cosmic::widget::menu::{self, action::MenuAction};

use super::Message;
use crate::context_menu::items;
use crate::keybinds;
use crate::profile::Profile;
use crate::terminal::Action;

/// Turns a menu pick into the app message that carries it: the same
/// entry the keybind path uses, so menu behavior cannot fork from
/// keybind behavior.
impl MenuAction for Action {
    type Message = Message;

    fn message(&self) -> Self::Message {
        Message::Action(*self)
    }
}

/// The menu as libcosmic trees, one per row of [`rows(profiles)`],
/// each item rendered with its keybind as a hint (every static action
/// is bound in `default_key_binds`; profile rows carry none — the
/// lookup tolerates unbound actions with an empty hint). Called by
/// the view only while the menu is open.
pub(super) fn trees(profiles: &[Profile]) -> Vec<menu::Tree<Message>> {
    menu::items(&keybinds::default_key_binds(), items(profiles))
}

/// The trees the view hands `widget::context_menu` for the current
/// open state: `None` while closed — the wrapper is then a plain
/// pass-through of the terminal — and the full row trees while open,
/// which arms the wrapper's right-release trigger. Holding the
/// open-state decision here keeps the view wiring free of menu logic.
pub(super) fn open_trees(
    menu_open: bool,
    profiles: &[Profile],
) -> Option<Vec<menu::Tree<Message>>> {
    menu_open.then(|| trees(profiles))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_menu::{ROWS, rows};

    #[test]
    fn trees_yield_one_tree_per_row_with_no_profiles() {
        assert_eq!(trees(&[]).len(), ROWS.len());
    }

    #[test]
    fn open_trees_is_none_while_the_menu_is_closed() {
        // A closed menu supplies no trees, so `widget::context_menu`
        // renders and passes events through as a plain wrapper — the
        // pixel-identical closed view.
        assert!(open_trees(false, &[]).is_none());
    }

    #[test]
    fn open_trees_carries_every_row_while_the_menu_is_open() {
        let trees = open_trees(true, &[]).expect("the open menu supplies its trees");
        assert_eq!(trees.len(), ROWS.len());
    }

    fn profile(name: &str) -> Profile {
        Profile {
            name: name.to_owned(),
            command: format!("run {name}"),
            cwd: None,
        }
    }

    #[test]
    fn open_trees_stays_none_while_closed_even_with_profiles() {
        assert!(open_trees(false, &[profile("ssh-web")]).is_none());
    }

    #[test]
    fn trees_carry_one_tree_per_row_with_profiles() {
        let profiles = [profile("ssh-web"), profile("htop")];
        assert_eq!(trees(&profiles).len(), rows(&profiles).len());
    }
}
