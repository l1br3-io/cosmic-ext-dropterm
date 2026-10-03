// SPDX-License-Identifier: GPL-3.0-only

//! The applet's `cosmic::Application`: static shell for now — an
//! icon button with a placeholder tooltip. State tracking, click
//! behavior, and live tooltip copy land with the live-wiring step.

use cosmic::Element;
use cosmic::app::{Core, Task};

/// The symbolic icon the panel surface shows; installed alongside the
/// binary and theme-recolorable. `utilities-terminal-symbolic` exists
/// as a fallback in Pop's theme if ours is missing.
const ICON_NAME: &str = "io.github.l1br3_io.CosmicExtDropterm-symbolic";

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
