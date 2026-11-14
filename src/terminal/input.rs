// SPDX-License-Identifier: GPL-3.0-only
// Copyright 2023 System76 <info@system76.com>
//
// Adapted from cosmic-term
// (<https://github.com/pop-os/cosmic-term>) src/mouse_reporter.rs at
// commit 9129277; the protocol logic itself now lives in
// dropterm-core's `mouse_reporter` (gpui-terminal Task 3's move —
// both frontends consume one implementation), and this file is the
// libcosmic half's translation: iced's pointer events onto core's
// toolkit-neutral shape.

use cosmic::{
    iced::mouse::{Event as MouseEvent, ScrollDelta},
    iced::{Event, keyboard::Modifiers, mouse::Button},
};

pub use dropterm_core::terminal::mouse_reporter::MouseReporter;
use dropterm_core::terminal::mouse_reporter::{
    PointerButton, PointerEvent, PointerModifiers, PointerScrollDelta,
    report_mouse as core_report_mouse, scroll_as_arrows as core_scroll_as_arrows,
    scroll_mouse as core_scroll_mouse,
};

use super::Terminal;

/// One iced mouse button onto the neutral button the encodings
/// number; `None` for the buttons xterm does not encode (back,
/// forward, …).
fn neutral_button(button: Button) -> Option<PointerButton> {
    match button {
        Button::Left => Some(PointerButton::Left),
        Button::Middle => Some(PointerButton::Middle),
        Button::Right => Some(PointerButton::Right),
        _ => None,
    }
}

/// One iced event onto the neutral pointer shape; `None` for every
/// non-pointer event and the unencodable buttons (the encoder's own
/// answer for them was always nothing).
fn neutral_event(event: &Event) -> Option<PointerEvent> {
    match event {
        Event::Mouse(MouseEvent::ButtonPressed(button)) => {
            neutral_button(*button).map(PointerEvent::Pressed)
        }
        Event::Mouse(MouseEvent::ButtonReleased(button)) => {
            neutral_button(*button).map(PointerEvent::Released)
        }
        Event::Mouse(MouseEvent::CursorMoved { .. }) => Some(PointerEvent::Moved),
        _ => None,
    }
}

/// iced's modifier set onto the three the encodings fold into the
/// button number.
fn neutral_modifiers(modifiers: Modifiers) -> PointerModifiers {
    PointerModifiers {
        shift: modifiers.shift(),
        alt: modifiers.alt(),
        control: modifiers.control(),
    }
}

/// One iced wheel delta onto the neutral shape.
fn neutral_delta(delta: ScrollDelta) -> PointerScrollDelta {
    match delta {
        ScrollDelta::Lines { x, y } => PointerScrollDelta::Lines { x, y },
        ScrollDelta::Pixels { x, y } => PointerScrollDelta::Pixels { x, y },
    }
}

/// Forward one iced mouse event to the pty as xterm tracking bytes
/// when the program asked for them.
pub fn report_mouse(
    terminal: &Terminal,
    reporter: &mut MouseReporter,
    event: &Event,
    modifiers: Modifiers,
    x: u32,
    y: u32,
) {
    if let Some(pointer) = neutral_event(event) {
        core_report_mouse(
            terminal,
            reporter,
            &pointer,
            neutral_modifiers(modifiers),
            x,
            y,
        );
    }
}

/// Forward one iced wheel delta to the program when it tracks the
/// mouse.
pub fn scroll_mouse(
    terminal: &Terminal,
    reporter: &mut MouseReporter,
    delta: ScrollDelta,
    modifiers: Modifiers,
    x: u32,
    y: u32,
) {
    core_scroll_mouse(
        terminal,
        reporter,
        neutral_delta(delta),
        neutral_modifiers(modifiers),
        x,
        y,
    );
}

/// Scroll by arrow-key bytes — the shell's own scrollback.
pub fn scroll_as_arrows(terminal: &Terminal, reporter: &mut MouseReporter, delta: ScrollDelta) {
    core_scroll_as_arrows(terminal, reporter, neutral_delta(delta));
}
