// SPDX-License-Identifier: GPL-3.0-only

//! The applet's `cosmic::Application`: the live panel citizen —
//! state from the D-Bus watches, clicks routed through the pure
//! core's decision, and a tooltip from live state.

use cosmic::Element;
use cosmic::app::{Core, Task};
use cosmic::applet::token::subscription::{
    TokenRequest, TokenUpdate, activation_token_subscription,
};
use cosmic::iced::Length;
use cosmic::iced::widget::svg;
use std::rc::Rc;

use crate::dbus;
use crate::subscriptions::{self, Update};
use cosmic_ext_dropterm::dbus_state;

/// The symbolic icon the panel surface shows; installed alongside the
/// binary and theme-recolorable. `utilities-terminal-symbolic` exists
/// as a fallback in Pop's theme if ours is missing.
const ICON_NAME: &str = "io.github.l1br3_io.CosmicExtDropterm-symbolic";

/// Identity for the xdg-activation-token subscription.
const TOKEN_SUB_ID: &str = "io.github.l1br3_io.CosmicExtDropterm.Applet.token";

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

/// Tooltip text: state plus tab count when running.
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
    target: TargetState,
    tab_count: u32,
    /// The watch subscription's shared session bus; commands (the
    /// toggle) ride the same connection.
    dbus_conn: Option<zbus::Connection>,
    /// The token-request capability handed over by the token
    /// subscription's `Init` message, kept as the variant itself
    /// because libcosmic does not re-export the calloop sender type.
    token_tx: Option<TokenUpdate>,
}

#[derive(Clone, Debug)]
pub(crate) enum Message {
    Click,
    Dbus(Update),
    Token(TokenUpdate),
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
        (
            Self {
                core,
                target: TargetState::NotRunning,
                tab_count: 0,
                dbus_conn: None,
                token_tx: None,
            },
            Task::none(),
        )
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::Click => self.on_click(),
            Message::Dbus(Update::Connected(conn)) => self.dbus_conn = Some(conn),
            Message::Dbus(Update::Event(event)) => {
                if let Event::TabCount(tab_count) = event {
                    self.tab_count = tab_count;
                }
                self.target = transition(self.target, event);
            }
            Message::Token(TokenUpdate::Init(tx)) => self.token_tx = Some(TokenUpdate::Init(tx)),
            Message::Token(TokenUpdate::Finished) => self.token_tx = None,
            Message::Token(TokenUpdate::ActivationToken { token, exec }) => {
                self.launch(&exec, token);
            }
            Message::Surface(a) => {
                return cosmic::task::message(cosmic::Action::Surface(a));
            }
        }
        Task::none()
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Self::Message> {
        cosmic::iced::Subscription::batch([
            subscriptions::watch().map(Message::Dbus),
            activation_token_subscription(TOKEN_SUB_ID).map(Message::Token),
        ])
    }

    fn view(&self) -> Element<'_, Self::Message> {
        // Opacity tiers: dimmest when the terminal is gone, dimmed
        // while hidden, full while visible.
        let alpha = match self.target {
            TargetState::NotRunning => 0.4,
            TargetState::Running { visible: false } => 0.65,
            TargetState::Running { visible: true } => 1.0,
        };
        let suggested = self.core.applet.suggested_size(true);
        let icon = cosmic::widget::icon(
            cosmic::widget::icon::from_name(ICON_NAME)
                .symbolic(true)
                .into(),
        )
        .class(cosmic::theme::Svg::Custom(Rc::new(move |theme| {
            // The applet-button tint from libcosmic's own
            // `icon_button`, with only the alpha adjusted.
            svg::Style {
                color: Some({
                    let base: cosmic::iced::Color =
                        theme.cosmic().background(theme.transparent).on.into();
                    cosmic::iced::Color { a: alpha, ..base }
                }),
            }
        })))
        .width(Length::Fixed(suggested.0 as f32))
        .height(Length::Fixed(suggested.1 as f32));
        let button = self
            .core
            .applet
            .button_from_element(icon, true)
            .on_press(Message::Click);
        Element::from(self.core.applet.applet_tooltip(
            button,
            tooltip(&self.target, self.tab_count),
            false,
            Message::Surface,
            None,
        ))
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

impl Applet {
    /// Routes a left-click through the pure decision.
    fn on_click(&mut self) {
        match click_action(&self.target) {
            ClickAction::SendToggle => {
                // Fire-and-forget on the shared connection; debounce
                // is the terminal's job, not the applet's.
                if let Some(conn) = self.dbus_conn.clone() {
                    tokio::spawn(async move {
                        if let Err(err) = dbus::send_toggle(&conn).await {
                            log::warn!("toggle send failed: {err}");
                        }
                    });
                }
            }
            ClickAction::LaunchWithToken => {
                // The time-applet pattern: ask the panel for an
                // xdg-activation token for the terminal, spawn it with
                // the token when it arrives.
                if let Some(TokenUpdate::Init(tx)) = &self.token_tx {
                    let _ = tx.send(TokenRequest {
                        // The terminal's app id — the same string as
                        // its bus name.
                        app_id: dbus_state::BUS_NAME.to_string(),
                        exec: "cosmic-ext-dropterm".to_string(),
                    });
                }
            }
        }
    }

    /// Spawns the terminal with the activation token in its
    /// environment. Caveat, deliberately still-correct: DropTerm
    /// currently receives a forwarded activation token and drops it
    /// (nothing applies it yet) — sending one remains the right path
    /// for when it does.
    fn launch(&self, exec: &str, token: Option<String>) {
        let mut command = tokio::process::Command::new(exec);
        if let Some(token) = token {
            command.env("XDG_ACTIVATION_TOKEN", &token);
            command.env("DESKTOP_STARTUP_ID", &token);
        }
        // tokio's child reaping means the applet never zombies the
        // terminal it launched.
        if let Err(err) = command.spawn() {
            log::warn!("failed to launch {exec}: {err}");
        }
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
