// SPDX-License-Identifier: GPL-3.0-only

//! The applet's `cosmic::Application`: static shell for now — an
//! icon button with a placeholder tooltip. State tracking, click
//! behavior, and live tooltip copy land with the live-wiring step.

// The pure core below is exercised headlessly and wired into the
// Application by the live-wiring step; until then the non-test build
// sees it as dead. Allow rather than expect: `expect` goes
// unfulfilled in the cfg(test) compilation, where the tests do use
// these. The live-wiring step removes this attribute.
#![allow(dead_code)]

use cosmic::Element;
use cosmic::app::{Core, Task};

/// The symbolic icon the panel surface shows; installed alongside the
/// binary and theme-recolorable. `utilities-terminal-symbolic` exists
/// as a fallback in Pop's theme if ours is missing.
const ICON_NAME: &str = "io.github.l1br3_io.CosmicExtDropterm-symbolic";

/// The terminal's status as the applet models it: gone, or running
/// with its drop-down surface in one of two legible states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetState {
    NotRunning,
    Running { visible: bool },
}

/// What the D-Bus subscriptions deliver to the core — one serial
/// stream, so event order is trusted and replays simply re-apply.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Event {
    /// The terminal's bus name gained an owner and its current
    /// properties were read (startup, or restart of either process).
    Appeared { visible: bool },
    /// The bus name lost its owner: the terminal exited.
    Vanished,
    /// `Visible` flipped while the name was owned.
    Visible(bool),
    /// `TabCount` changed while the name was owned (tooltip-only).
    TabCount(u32),
}

/// Folds one subscription event into the target state. Events that
/// presume an owner (`Visible`, `TabCount`) are ignored while
/// `NotRunning`: a stale property flip racing the name's demise must
/// not resurrect a dead terminal.
pub(crate) fn transition(state: TargetState, event: Event) -> TargetState {
    match (state, event) {
        // Owner-presuming events while ownerless are stale by
        // construction; dropping them is the zombie-safety rule.
        (TargetState::NotRunning, Event::Visible(_) | Event::TabCount(_)) => state,
        (_, Event::Appeared { visible }) | (_, Event::Visible(visible)) => {
            TargetState::Running { visible }
        }
        (_, Event::Vanished) => TargetState::NotRunning,
        (_, Event::TabCount(_)) => state,
    }
}

/// The click decision, pure: what a left-click must do in each state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClickAction {
    LaunchWithToken,
    SendToggle,
}

pub(crate) fn click_action(state: &TargetState) -> ClickAction {
    match state {
        TargetState::NotRunning => ClickAction::LaunchWithToken,
        TargetState::Running { .. } => ClickAction::SendToggle,
    }
}

/// Tooltip text: state plus tab count when running. Copy shape per
/// the spec's example; the owner finalizes wording at review.
pub(crate) fn tooltip(state: &TargetState, tab_count: u32) -> String {
    match *state {
        TargetState::NotRunning => "DropTerm — not running".to_string(),
        TargetState::Running { visible } => {
            let word = if visible { "visible" } else { "hidden" };
            let noun = if tab_count == 1 { "tab" } else { "tabs" };
            format!("DropTerm — {word}, {tab_count} {noun}")
        }
    }
}

pub(crate) struct Applet {
    core: Core,
}

#[derive(Clone, Debug)]
pub(crate) enum Message {
    /// Surface bookkeeping raised by the tooltip helper (popup-less,
    /// so only its no-op paths ever fire).
    Surface(cosmic::surface::Action<Message>),
}

impl cosmic::Application for Applet {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "io.github.l1br3_io.CosmicExtDropterm.Applet";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
        (Self { core }, Task::none())
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::Surface(a) => cosmic::task::message(cosmic::Action::Surface(a)),
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        // Sizing comes from the panel's COSMIC_PANEL_SIZE hint via
        // suggested_size(true) inside icon_button — never hardcoded
        // pixels.
        let button = self.core.applet.icon_button(ICON_NAME);
        // Placeholder copy; the owner finalizes tooltip wording with
        // real rendering.
        Element::from(self.core.applet.applet_tooltip(
            button,
            "DropTerm",
            false,
            Message::Surface,
            None,
        ))
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appeared_seeds_running_state_from_initial_props() {
        assert_eq!(
            transition(TargetState::NotRunning, Event::Appeared { visible: false }),
            TargetState::Running { visible: false }
        );
        assert_eq!(
            transition(TargetState::NotRunning, Event::Appeared { visible: true }),
            TargetState::Running { visible: true }
        );
    }

    #[test]
    fn visible_flip_updates_running_state() {
        let hidden = TargetState::Running { visible: false };
        assert_eq!(
            transition(hidden, Event::Visible(true)),
            TargetState::Running { visible: true }
        );
        assert_eq!(
            transition(
                TargetState::Running { visible: true },
                Event::Visible(false)
            ),
            hidden
        );
    }

    #[test]
    fn tab_count_change_preserves_target_state() {
        assert_eq!(
            transition(TargetState::Running { visible: true }, Event::TabCount(9)),
            TargetState::Running { visible: true }
        );
    }

    #[test]
    fn vanished_returns_to_not_running() {
        assert_eq!(
            transition(TargetState::Running { visible: true }, Event::Vanished),
            TargetState::NotRunning
        );
    }

    #[test]
    fn visible_flip_while_not_running_is_ignored() {
        // A stale PropertiesChanged racing the name's demise must not
        // resurrect a dead terminal.
        assert_eq!(
            transition(TargetState::NotRunning, Event::Visible(true)),
            TargetState::NotRunning
        );
    }

    #[test]
    fn reappearance_reseeds_state_from_fresh_props() {
        assert_eq!(
            transition(
                TargetState::Running { visible: true },
                Event::Appeared { visible: false }
            ),
            TargetState::Running { visible: false }
        );
    }

    #[test]
    fn click_launches_when_not_running() {
        assert_eq!(
            click_action(&TargetState::NotRunning),
            ClickAction::LaunchWithToken
        );
    }

    #[test]
    fn click_toggles_when_running_hidden() {
        assert_eq!(
            click_action(&TargetState::Running { visible: false }),
            ClickAction::SendToggle
        );
    }

    #[test]
    fn click_toggles_when_running_visible() {
        assert_eq!(
            click_action(&TargetState::Running { visible: true }),
            ClickAction::SendToggle
        );
    }

    #[test]
    fn tooltip_names_the_state_when_not_running() {
        assert_eq!(
            tooltip(&TargetState::NotRunning, 0),
            "DropTerm — not running"
        );
    }

    #[test]
    fn tooltip_pairs_hidden_with_tab_count() {
        assert_eq!(
            tooltip(&TargetState::Running { visible: false }, 3),
            "DropTerm — hidden, 3 tabs"
        );
    }

    #[test]
    fn tooltip_pairs_visible_with_tab_count() {
        assert_eq!(
            tooltip(&TargetState::Running { visible: true }, 2),
            "DropTerm — visible, 2 tabs"
        );
    }

    #[test]
    fn tooltip_pluralizes_a_single_tab() {
        assert_eq!(
            tooltip(&TargetState::Running { visible: true }, 1),
            "DropTerm — visible, 1 tab"
        );
    }
}
