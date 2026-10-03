// SPDX-License-Identifier: GPL-3.0-only

//! The drag-to-resize grab bar: a thin strip on the surface's free
//! edge by which the terminal's height is resized live. iced has no
//! first-class drag API, so the bar tracks press/move/release and
//! reports the pointer's window-absolute y (the space `CursorMoved`
//! carries); pointer-to-height math lives in
//! [`crate::geometry::drag_target_px`]. Moves track the cursor
//! wherever it roams; config is written only at release.

use std::sync::atomic::Ordering;

use cosmic::iced::core::Border;
use cosmic::iced::core::Shell;
use cosmic::iced::core::clipboard::Clipboard;
use cosmic::iced::core::layout::{self, Layout};
use cosmic::iced::core::renderer::{self, Quad, Renderer as _};
use cosmic::iced::core::widget::{Tree, Widget, tree};
use cosmic::iced::event::Event;
use cosmic::iced::mouse::{self, Button, Cursor, Event as MouseEvent};
use cosmic::iced::{Color, Length, Point, Rectangle, Size};
use cosmic::theme::Theme;
use cosmic::{Element, Renderer};

use crate::terminal::WINDOW_BG_COLOR;

/// Height of the bar in logical pixels: high enough to grab, thin
/// enough to read as a handle rather than chrome. A constant rather
/// than a theme value so the pinned-height math stays deterministic
/// (see `surface::terminal_view_height`).
const BAR_HEIGHT: f32 = 8.0;

/// The bar's height in logical pixels, for callers that must reserve
/// room for it outside the widget tree (the app pins the terminal
/// widget to the window height minus this — see
/// `surface::terminal_view_height`).
pub const GRAB_BAR_HEIGHT: f32 = BAR_HEIGHT;

/// Height of the centered grip line — the affordance that the strip is
/// a handle.
const GRIP_HEIGHT: f32 = 2.0;

/// Longest the grip line gets, so it stays a handle on wide outputs.
const GRIP_MAX_WIDTH: f32 = 96.0;

/// Per-widget state: whether a left press is held on the bar, and the
/// last pointer y published (window space) so identical positions skip
/// republishing. Nothing is seeded at press — the press position is
/// available bar-local (`position_in`) while moves carry
/// window-absolute y, and the two spaces must not be mixed; the app
/// arms its absolute anchor from the first move instead.
#[derive(Default)]
struct State {
    dragging: bool,
    last_y: Option<f32>,
}

/// The grab bar: drag it to resize the terminal's height live.
///
/// `on_drag` reports the pointer's window-absolute y on every changed
/// move while a press is held; `on_release` reports the press ending.
/// Both are required — a bar that can't report is dead weight.
pub struct GrabBar<'a, Message> {
    on_drag: Box<dyn Fn(f32) -> Message + 'a>,
    on_release: Box<dyn Fn() -> Message + 'a>,
}

impl<'a, Message> GrabBar<'a, Message>
where
    Message: Clone,
{
    /// Creates a bar reporting the pointer's window y through `on_drag`
    /// and the drag's end through `on_release`.
    pub fn new(
        on_drag: impl Fn(f32) -> Message + 'a,
        on_release: impl Fn() -> Message + 'a,
    ) -> Self {
        Self {
            on_drag: Box::new(on_drag),
            on_release: Box::new(on_release),
        }
    }
}

impl<Message> Widget<Message, Theme, Renderer> for GrabBar<'_, Message>
where
    Message: Clone,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(BAR_HEIGHT))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(Length::Fill).height(Length::Fixed(BAR_HEIGHT));
        let size = limits.resolve(
            Length::Fill,
            Length::Fixed(BAR_HEIGHT),
            Size::new(limits.max().width, BAR_HEIGHT),
        );
        layout::Node::new(size)
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        // An active drag grabs the cursor wherever it roams;
        // otherwise the strip announces what it resizes.
        if tree.state.downcast_ref::<State>().dragging {
            return mouse::Interaction::Grabbing;
        }
        if cursor.position_in(layout.bounds()).is_some() {
            mouse::Interaction::ResizingVertically
        } else {
            mouse::Interaction::Idle
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle<f32>,
    ) {
        match event {
            Event::Mouse(MouseEvent::ButtonPressed(Button::Left)) => {
                // Arm on a press inside the bar. There is nothing to
                // click here, so — unlike the tab bar — no drag
                // threshold is needed: any held travel is a resize.
                if cursor.position_in(layout.bounds()).is_none() {
                    return;
                }
                let state = tree.state.downcast_mut::<State>();
                state.dragging = true;
                shell.capture_event();
            }
            Event::Mouse(MouseEvent::CursorMoved { position }) => {
                // Drag progress: report the pointer's window-absolute
                // y — the app retargets absolutely from it. Tracked
                // wherever the cursor roams; the bar may lag the
                // pointer while the surface catches up.
                let state = tree.state.downcast_mut::<State>();
                if !state.dragging {
                    return;
                }
                if state.last_y != Some(position.y) {
                    state.last_y = Some(position.y);
                    shell.publish((self.on_drag)(position.y));
                }
                shell.capture_event();
            }
            Event::Mouse(MouseEvent::ButtonReleased(Button::Left)) => {
                // Drag end. The release may land outside the bar after
                // a fast fling; it still ends the drag.
                let state = tree.state.downcast_mut::<State>();
                if !state.dragging {
                    return;
                }
                state.dragging = false;
                shell.publish((self.on_release)());
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        // The strip: the terminal pane's effective background, slightly
        // darkened — the same treatment as the tab bar, so both read as
        // chrome of the same surface rather than strips of window clear
        // color (which follows the system theme and can disagree with
        // the terminal scheme).
        let packed = WINDOW_BG_COLOR.load(Ordering::SeqCst);
        let background = Color::from_rgba8(
            ((packed >> 16) & 0xFF) as u8,
            ((packed >> 8) & 0xFF) as u8,
            (packed & 0xFF) as u8,
            1.0,
        );
        let strip = Color {
            r: background.r * 0.92,
            g: background.g * 0.92,
            b: background.b * 0.92,
            a: 1.0,
        };
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    radius: 0.0.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                snap: true,
                ..Default::default()
            },
            strip,
        );

        // The grip: a short centered accent line — the affordance that
        // this strip is a handle. The accent color is readable on the
        // pane by construction (theme guarantee), so no
        // luminance-derived foreground is needed.
        let grip_width = (bounds.width / 3.0).clamp(GRIP_HEIGHT, GRIP_MAX_WIDTH);
        let grip = Rectangle::new(
            Point::new(
                bounds.center_x() - grip_width / 2.0,
                bounds.y + (bounds.height - GRIP_HEIGHT) / 2.0,
            ),
            Size::new(grip_width, GRIP_HEIGHT),
        );
        renderer.fill_quad(
            Quad {
                bounds: grip,
                border: Border {
                    radius: (GRIP_HEIGHT / 2.0).into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                snap: true,
                ..Default::default()
            },
            Color::from(theme.cosmic().accent_color()),
        );
    }
}

impl<'a, Message> From<GrabBar<'a, Message>> for Element<'a, Message>
where
    Message: Clone + 'a,
{
    fn from(grab_bar: GrabBar<'a, Message>) -> Self {
        Self::new(grab_bar)
    }
}
