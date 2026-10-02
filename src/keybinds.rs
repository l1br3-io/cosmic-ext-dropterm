// SPDX-License-Identifier: GPL-3.0-only

//! The fixed set of app-level keyboard shortcuts.
//!
//! Unlike cosmic-term, these are not user-customizable in v1 (there is no
//! shortcuts settings page yet - see the ROADMAP's "Later" section). The
//! `TerminalBox` widget only uses this map to decide which key presses to
//! swallow instead of sending to the shell; the actual behavior for a
//! matched [`Action`] is applied in `App::update`.

use std::collections::HashMap;

use cosmic::iced::keyboard::key::{Named, Physical};
use cosmic::iced::keyboard::{Key, Modifiers};
use cosmic::widget::menu::key_bind::{KeyBind, Modifier};

use crate::terminal::Action;

fn bind(modifiers: &[Modifier], key: &str) -> KeyBind {
    KeyBind {
        modifiers: modifiers.to_vec(),
        key: Key::Character(key.into()),
    }
}

fn bind_named(modifiers: &[Modifier], key: Named) -> KeyBind {
    KeyBind {
        modifiers: modifiers.to_vec(),
        key: Key::Named(key),
    }
}

pub fn default_key_binds() -> HashMap<KeyBind, Action> {
    let mut map = HashMap::new();
    map.insert(bind(&[Modifier::Ctrl, Modifier::Shift], "c"), Action::Copy);
    map.insert(bind(&[Modifier::Ctrl, Modifier::Shift], "v"), Action::Paste);
    map.insert(
        bind_named(&[Modifier::Shift], Named::Insert),
        Action::PastePrimary,
    );
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "a"),
        Action::SelectAll,
    );
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "k"),
        Action::ClearScrollback,
    );
    map.insert(bind(&[Modifier::Ctrl], "="), Action::ZoomIn);
    map.insert(bind(&[Modifier::Ctrl], "+"), Action::ZoomIn);
    map.insert(bind(&[Modifier::Ctrl], "-"), Action::ZoomOut);
    map.insert(bind(&[Modifier::Ctrl], "0"), Action::ZoomReset);
    map.insert(bind(&[Modifier::Ctrl, Modifier::Shift], "q"), Action::Hide);
    // The settings window (settings-ui spec, U1; moved from
    // Ctrl+Shift+, to Ctrl+Shift+I 2026-10-02, owner preference).
    // Bare Ctrl+I must stay unbound: in a terminal it is the TAB byte
    // (0x09), so swallowing it would break tab completion.
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "i"),
        Action::Settings,
    );
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "t"),
        Action::NewTab,
    );
    map.insert(
        bind(&[Modifier::Ctrl, Modifier::Shift], "w"),
        Action::CloseTab,
    );
    map.insert(
        bind_named(&[Modifier::Ctrl], Named::PageUp),
        Action::PrevTab,
    );
    map.insert(
        bind_named(&[Modifier::Ctrl], Named::PageDown),
        Action::NextTab,
    );
    // Browser-style cycling (Firefox): Ctrl+Tab / Ctrl+Shift+Tab walk
    // forward and backward through the tab list, alongside the
    // Ctrl+PageUp/PageDown binds above.
    map.insert(bind_named(&[Modifier::Ctrl], Named::Tab), Action::NextTab);
    map.insert(
        bind_named(&[Modifier::Ctrl, Modifier::Shift], Named::Tab),
        Action::PrevTab,
    );
    map
}

/// Whether the given key press matches a bound [`Action`], returning it if so.
///
/// `physical_key` should be forwarded from the same key-press event as
/// `key`: it lets a bind still match on non-Latin keyboard layouts, the
/// same fallback `TerminalBox` itself uses to decide which presses to
/// swallow instead of sending to the shell (see `KeyBind::matches`).
/// Passing `None` here while the widget passes `Some` would let it swallow
/// a key press that this function then fails to match, silently eating
/// the keystroke.
pub fn matched_action(
    key_binds: &HashMap<KeyBind, Action>,
    modifiers: Modifiers,
    key: &Key,
    physical_key: Option<&Physical>,
) -> Option<Action> {
    key_binds
        .iter()
        .find(|(bind, _)| bind.matches(modifiers, key, physical_key))
        .map(|(_, action)| *action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_and_paste_are_bound() {
        let binds = default_key_binds();
        assert_eq!(
            matched_action(
                &binds,
                Modifiers::CTRL | Modifiers::SHIFT,
                &Key::Character("c".into()),
                None,
            ),
            Some(Action::Copy)
        );
        assert_eq!(
            matched_action(
                &binds,
                Modifiers::CTRL | Modifiers::SHIFT,
                &Key::Character("v".into()),
                None,
            ),
            Some(Action::Paste)
        );
    }

    #[test]
    fn unbound_key_does_not_match() {
        let binds = default_key_binds();
        assert_eq!(
            matched_action(&binds, Modifiers::CTRL, &Key::Character("c".into()), None),
            None
        );
    }

    #[test]
    fn ctrl_shift_q_hides() {
        let binds = default_key_binds();
        assert_eq!(
            matched_action(
                &binds,
                Modifiers::CTRL | Modifiers::SHIFT,
                &Key::Character("q".into()),
                None,
            ),
            Some(Action::Hide)
        );
    }

    #[test]
    fn bare_escape_is_not_bound() {
        let binds = default_key_binds();
        assert_eq!(
            matched_action(&binds, Modifiers::empty(), &Key::Named(Named::Escape), None,),
            None
        );
    }

    #[test]
    fn ctrl_shift_i_opens_settings() {
        let binds = default_key_binds();
        assert_eq!(
            matched_action(
                &binds,
                Modifiers::CTRL | Modifiers::SHIFT,
                &Key::Character("i".into()),
                None,
            ),
            Some(Action::Settings)
        );
    }

    #[test]
    fn bare_and_ctrl_i_stay_free_for_the_terminal() {
        // Ctrl+I is the TAB byte (0x09) in a terminal — swallowing it
        // would break tab completion — and a bare `i` is ordinary
        // typing: only the full Ctrl+Shift+I combo is the settings
        // bind.
        let binds = default_key_binds();
        assert_eq!(
            matched_action(
                &binds,
                Modifiers::empty(),
                &Key::Character("i".into()),
                None
            ),
            None
        );
        assert_eq!(
            matched_action(&binds, Modifiers::CTRL, &Key::Character("i".into()), None),
            None
        );
    }

    #[test]
    fn ctrl_shift_t_opens_a_new_tab() {
        let binds = default_key_binds();
        assert_eq!(
            matched_action(
                &binds,
                Modifiers::CTRL | Modifiers::SHIFT,
                &Key::Character("t".into()),
                None,
            ),
            Some(Action::NewTab)
        );
    }

    #[test]
    fn ctrl_shift_w_closes_the_active_tab() {
        let binds = default_key_binds();
        assert_eq!(
            matched_action(
                &binds,
                Modifiers::CTRL | Modifiers::SHIFT,
                &Key::Character("w".into()),
                None,
            ),
            Some(Action::CloseTab)
        );
    }

    #[test]
    fn ctrl_page_up_cycles_to_the_previous_tab() {
        let binds = default_key_binds();
        assert_eq!(
            matched_action(&binds, Modifiers::CTRL, &Key::Named(Named::PageUp), None,),
            Some(Action::PrevTab)
        );
    }

    #[test]
    fn ctrl_page_down_cycles_to_the_next_tab() {
        let binds = default_key_binds();
        assert_eq!(
            matched_action(&binds, Modifiers::CTRL, &Key::Named(Named::PageDown), None,),
            Some(Action::NextTab)
        );
    }

    #[test]
    fn bare_page_keys_stay_free_for_the_terminal() {
        // Pagers and shell readers use PageUp/PageDown themselves, so
        // only the Ctrl-combined forms may be swallowed.
        let binds = default_key_binds();
        assert_eq!(
            matched_action(&binds, Modifiers::empty(), &Key::Named(Named::PageUp), None),
            None
        );
        assert_eq!(
            matched_action(
                &binds,
                Modifiers::empty(),
                &Key::Named(Named::PageDown),
                None
            ),
            None
        );
    }

    #[test]
    fn ctrl_tab_cycles_to_the_next_tab_browser_style() {
        let binds = default_key_binds();
        assert_eq!(
            matched_action(&binds, Modifiers::CTRL, &Key::Named(Named::Tab), None),
            Some(Action::NextTab)
        );
    }

    #[test]
    fn ctrl_shift_tab_cycles_to_the_previous_tab_browser_style() {
        let binds = default_key_binds();
        assert_eq!(
            matched_action(
                &binds,
                Modifiers::CTRL | Modifiers::SHIFT,
                &Key::Named(Named::Tab),
                None
            ),
            Some(Action::PrevTab)
        );
    }

    #[test]
    fn bare_tab_stays_free_for_the_terminal() {
        // Tab completion must keep reaching the shell.
        let binds = default_key_binds();
        assert_eq!(
            matched_action(&binds, Modifiers::empty(), &Key::Named(Named::Tab), None),
            None
        );
        assert_eq!(
            matched_action(&binds, Modifiers::SHIFT, &Key::Named(Named::Tab), None),
            None
        );
    }
}
