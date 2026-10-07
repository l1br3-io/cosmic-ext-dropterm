// SPDX-License-Identifier: GPL-3.0-only

//! The settings drawer's drag handle: a thin vertical strip on the
//! pane's leading edge by which the drawer's width is resized live
//! (drawer-resize spec). `GrabBar`'s discipline verbatim, turned
//! 90°: iced has no first-class drag API, so the handle tracks
//! press/move/release and reports the pointer's window-absolute x
//! (the space `CursorMoved` carries); pointer-to-width math lives
//! in [`crate::settings::drawer_drag_px`]. Moves track the cursor
//! wherever it roams; config is written only at release, and a
//! double-click (two presses within iced's 6px/300ms click window)
//! reports a reset instead.

use cosmic::iced::core::Border;
use cosmic::iced::core::Shell;
use cosmic::iced::core::clipboard::Clipboard;
use cosmic::iced::core::layout::{self, Layout};
use cosmic::iced::core::mouse::{Click, click};
use cosmic::iced::core::renderer::{self, Quad, Renderer as _};
use cosmic::iced::core::widget::{Tree, Widget, tree};
use cosmic::iced::event::Event;
use cosmic::iced::mouse::{self, Button, Cursor, Event as MouseEvent};
use cosmic::iced::{Color, Length, Point, Rectangle, Size};
use cosmic::theme::Theme;
use cosmic::{Element, Renderer};

/// Width of the handle in logical pixels: wide enough to grab, thin
/// enough to read as an edge rather than a column of chrome.
const HANDLE_WIDTH: f32 = 10.0;

/// Width of the centered grip line — the affordance that the strip
/// is a handle.
const GRIP_WIDTH: f32 = 2.0;

/// Vertical inset of the grip line, so it reads as a handle detail
/// rather than a pane border.
const GRIP_INSET: f32 = 12.0;

/// Per-widget state: whether a left press is held on the handle,
/// the last pointer x published (window space) so identical
/// positions skip republishing, and the previous release's click
/// for double-click detection. Nothing is seeded at press — the
/// press position is available handle-local (`position_in`) while
/// moves carry window-absolute x, and the two spaces must not be
/// mixed; the app targets absolutely from the first move instead.
#[derive(Default)]
struct State {
    dragging: bool,
    last_x: Option<f32>,
    previous_click: Option<Click>,
}

/// The drawer's resize handle: drag it to resize the pane live.
///
/// `on_drag` reports the pointer's window-absolute x on every
/// changed move while a press is held; `on_release` reports a drag
/// ending; `on_double_click` reports a double-click (a reset).
/// All three are required — a handle that can't report is dead
/// weight.
#[expect(
    clippy::struct_field_names,
    reason = "the on_ prefixes are the callback convention this widget mirrors (grab_bar, iced MouseArea)"
)]
pub struct DrawerHandle<'a, Message> {
    on_drag: Box<dyn Fn(f32) -> Message + 'a>,
    on_release: Box<dyn Fn() -> Message + 'a>,
    on_double_click: Box<dyn Fn() -> Message + 'a>,
}

impl<'a, Message> DrawerHandle<'a, Message>
where
    Message: Clone,
{
    /// Creates a handle reporting the pointer's window x through
    /// `on_drag`, the drag's end through `on_release`, and a
    /// double-click through `on_double_click`.
    pub fn new(
        on_drag: impl Fn(f32) -> Message + 'a,
        on_release: impl Fn() -> Message + 'a,
        on_double_click: impl Fn() -> Message + 'a,
    ) -> Self {
        Self {
            on_drag: Box::new(on_drag),
            on_release: Box::new(on_release),
            on_double_click: Box::new(on_double_click),
        }
    }
}

impl<Message> Widget<Message, Theme, Renderer> for DrawerHandle<'_, Message>
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
        Size::new(Length::Fixed(HANDLE_WIDTH), Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits
            .width(Length::Fixed(HANDLE_WIDTH))
            .height(Length::Fill);
        let size = limits.resolve(
            Length::Fixed(HANDLE_WIDTH),
            Length::Fill,
            Size::new(HANDLE_WIDTH, limits.max().height),
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
            mouse::Interaction::ResizingHorizontally
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
                // Arm on a press inside the handle. There is
                // nothing to click here, so — like the grab bar —
                // no drag threshold is needed: any held travel is
                // a resize.
                if cursor.position_in(layout.bounds()).is_none() {
                    return;
                }
                let state = tree.state.downcast_mut::<State>();
                state.dragging = true;
                shell.capture_event();
            }
            Event::Mouse(MouseEvent::CursorMoved { position }) => {
                // Drag progress: report the pointer's
                // window-absolute x — the app targets the width
                // absolutely from it. Tracked wherever the cursor
                // roams; the handle may lag the pointer while the
                // pane catches up.
                let state = tree.state.downcast_mut::<State>();
                if !state.dragging {
                    return;
                }
                if state.last_x != Some(position.x) {
                    state.last_x = Some(position.x);
                    shell.publish((self.on_drag)(position.x));
                }
                shell.capture_event();
            }
            Event::Mouse(MouseEvent::ButtonReleased(Button::Left)) => {
                // Drag end. The release may land outside the handle
                // after a fast fling; it still ends the drag. Two
                // releases within iced's click window (6px,
                // 300ms) are a double-click — a reset, not a
                // commit — so the second release reports through
                // `on_double_click` alone.
                let state = tree.state.downcast_mut::<State>();
                if !state.dragging {
                    return;
                }
                state.dragging = false;
                let message = if let Some(position) = cursor.position() {
                    let released = Click::new(position, Button::Left, state.previous_click);
                    state.previous_click = Some(released);
                    if released.kind() == click::Kind::Double {
                        (self.on_double_click)()
                    } else {
                        (self.on_release)()
                    }
                } else {
                    (self.on_release)()
                };
                shell.publish(message);
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

        // The grip: a tall centered accent line — the affordance
        // that this strip is a handle. The accent color is
        // readable on the pane by construction (theme guarantee),
        // so no luminance-derived foreground is needed.
        let grip_height = (bounds.height - 2.0 * GRIP_INSET).max(0.0);
        let grip = Rectangle::new(
            Point::new(bounds.center_x() - GRIP_WIDTH / 2.0, bounds.y + GRIP_INSET),
            Size::new(GRIP_WIDTH, grip_height),
        );
        renderer.fill_quad(
            Quad {
                bounds: grip,
                border: Border {
                    radius: (GRIP_WIDTH / 2.0).into(),
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

impl<'a, Message> From<DrawerHandle<'a, Message>> for Element<'a, Message>
where
    Message: Clone + 'a,
{
    fn from(handle: DrawerHandle<'a, Message>) -> Self {
        Self::new(handle)
    }
}
