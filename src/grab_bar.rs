// SPDX-License-Identifier: GPL-3.0-only

//! The drag-to-resize grab bar (geometry spec, G3): a thin visible
//! strip on the surface's free edge — outside the tab bar, which sits
//! between it and the content — by which the terminal's height is
//! resized live.
//!
//! iced has no first-class drag API, so the bar tracks
//! press/move/release itself — the same state machine as the tab bar's
//! drag reorder (tabs spec, T7), itself the one of iced's own text
//! editor (pinned rev `08a09e3`, `widget/text_editor.rs`). It reports
//! raw surface-local y *deltas* and stays otherwise dumb: everything
//! that turns a delta into a height — the `Edge::Bottom` sign flip,
//! the 10–100 % band, the `max(1px)` rule — lives in
//! [`crate::geometry::drag_height_percent`] as pure functions,
//! unit-tested headless. Rendering and drag feel need real COSMIC
//! hardware and are covered by the README manual checklist.
//!
//! While a press is held, moves are tracked wherever the cursor roams
//! — the bar itself resizes under the pointer as the surface follows,
//! and iced's column routes events to every child whatever the cursor
//! is over (see the tab bar's release handling) — and no config is
//! touched while dragging (a "never" boundary of the module;
//! persistence is G4).

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

/// Per-widget state: whether a left press is held on the bar and the
/// last cursor y seen (window space), to diff per move. y differences
/// are bar-position-independent — resizing the surface under the
/// pointer moves the bar's origin, not the pointer — so window-space
/// deltas are exactly the pointer's travel.
#[derive(Default)]
struct State {
    dragging: bool,
    last_y: f32,
}

/// The grab bar: drag it to resize the terminal's height live.
///
/// `on_drag` reports each move's y delta while a press is held
/// (positive is downward, surface space); `on_release` reports the
/// press ending. Both are required — a bar that can't report is dead
/// weight.
pub struct GrabBar<'a, Message> {
    on_drag: Box<dyn Fn(f32) -> Message + 'a>,
    on_release: Box<dyn Fn() -> Message + 'a>,
}

impl<'a, Message> GrabBar<'a, Message>
where
    Message: Clone,
{
    /// Creates a bar reporting move deltas through `on_drag` and the
    /// drag's end through `on_release`.
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
        // An active drag grabs the cursor wherever it roams, like the
        // tab bar's reorder (T7); otherwise the strip announces what
        // it resizes. `ResizingVertically` verified at the pinned rev
        // (`iced/core/src/mouse/interaction.rs`, `08a09e3`).
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
                let Some(point) = cursor.position_in(layout.bounds()) else {
                    return;
                };
                let state = tree.state.downcast_mut::<State>();
                state.dragging = true;
                state.last_y = point.y;
                shell.capture_event();
            }
            Event::Mouse(MouseEvent::CursorMoved { position }) => {
                // Drag progress: report the move's delta. Tracked
                // wherever the cursor roams — the bar may lag the
                // pointer while the surface catches up.
                let state = tree.state.downcast_mut::<State>();
                if !state.dragging {
                    return;
                }
                let dy = position.y - state.last_y;
                state.last_y = position.y;
                if dy != 0.0 {
                    shell.publish((self.on_drag)(dy));
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
