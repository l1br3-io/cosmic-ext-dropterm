// SPDX-License-Identifier: GPL-3.0-only

//! The drop-down slide animation (slide-animation spec): the content
//! column slides inside the fixed surface, clipped and translated at
//! draw time, so the surface's geometry — anchor, size, margins, layer,
//! exclusive zone — never changes while animating and a slide costs the
//! compositor nothing (no layer-shell traffic at all).
//!
//! The drawing mechanism is iced's own scrollable pattern (pinned rev
//! `08a09e3`, `iced/widget/src/scrollable.rs`):
//! [`Renderer::with_layer`](cosmic::iced::core::Renderer::with_layer)
//! clips to the surface bounds and
//! [`Renderer::with_translation`](cosmic::iced::core::Renderer::with_translation)
//! shifts every primitive the child records. [`SlideBox`] is the
//! wrapper the whole module renders through; the pure
//! offset/easing/reversal math joins it here as plain functions over
//! plain data — the same separation as `geometry.rs`.

use std::time::Duration;

use crate::config::Edge;
use cosmic::iced::core::Shell;
use cosmic::iced::core::clipboard::Clipboard;
use cosmic::iced::core::layout::{self, Layout};
use cosmic::iced::core::overlay;
use cosmic::iced::core::renderer::{self, Renderer as _};
use cosmic::iced::core::widget::{Operation, Tree, Widget, tree};
use cosmic::iced::event::Event;
use cosmic::iced::mouse::{self, Cursor};
use cosmic::iced::{Length, Rectangle, Size, Vector};
use cosmic::theme::Theme;
use cosmic::{Element, Renderer};

/// Window-space Y translation for a content slide at `offset_px` from
/// its resting position, on a surface anchored at `edge`
/// (slide-animation spec): the slide always moves content toward the
/// anchored screen edge — `Edge::Top` translates upward (negative),
/// `Edge::Bottom` downward (positive) — so a show slides in from the
/// anchored edge and a hide slides back out to it. Horizontal
/// placement is never animated. `0.0` is the resting position.
pub fn translation_y(edge: Edge, offset_px: f32) -> f32 {
    match edge {
        Edge::Top => -offset_px,
        Edge::Bottom => offset_px,
    }
}

/// Duration of one slide, in milliseconds (slide-animation spec,
/// locked 2026-09-30): mid-range of the 120–150 ms band, ease-out on
/// show / ease-in on hide. A compile-time constant — no config surface
/// until `settings-ui`.
pub const SLIDE_DURATION_MS: u64 = 130;

/// Tick cadence of the animation clock (slide-animation spec plan
/// decision): ~60 Hz, about 8 frames per 130 ms slide — redraw-only
/// ticks, no layer-shell traffic. Safe by the G3 evidence (the drag
/// incident saturated the runtime on `set_size` storms, not redraws).
pub const SLIDE_TICK: Duration = Duration::from_millis(16);

/// The slide wrapper: draws its child translated by `offset_y` pixels
/// — positive is down in window space — clipped to the child's own
/// bounds, so nothing paints outside the surface while it slides.
///
/// The wrapper is *transparent*: layout, size hints, events, focus
/// operations, and overlays all pass straight through to the child (the
/// forwarding pattern of iced's container, pinned rev
/// `iced/widget/src/container.rs`), so wrapping changes nothing but
/// where the child paints. Input keeps mapping to the child's resting
/// layout — during a 130 ms slide the pointer-to-content mapping is off
/// by the translation (keyboard input is unaffected), the module's
/// accepted limitation.
pub struct SlideBox<'a, Message> {
    child: Element<'a, Message>,
    offset_y: f32,
}

impl<'a, Message> SlideBox<'a, Message> {
    /// Wraps `child`, painting it `offset_y` pixels from its resting
    /// position. An offset of `0.0` renders byte-identically to the
    /// unwrapped child — the module's no-animation regression
    /// guarantee.
    pub fn new(child: impl Into<Element<'a, Message>>, offset_y: f32) -> Self {
        Self {
            child: child.into(),
            offset_y,
        }
    }
}

impl<Message> Widget<Message, Theme, Renderer> for SlideBox<'_, Message>
where
    Message: Clone,
{
    // Transparent in the state tree (the container pattern): the
    // wrapper forwards its own tag, state, children, and diff to the
    // child, so the tree — and every diff against it — is shaped as if
    // the wrapper did not exist.

    fn tag(&self) -> tree::Tag {
        self.child.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.child.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.child.as_widget().children()
    }

    fn diff(&mut self, tree: &mut Tree) {
        self.child.as_widget_mut().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.child.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // Passthrough: the child lays out under the wrapper's limits
        // and the wrapper adopts its node, so the wrapper occupies
        // exactly the child's resting bounds.
        self.child.as_widget_mut().layout(tree, renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        // Focus and other id-addressed operations must reach the
        // terminal and the rename input through the wrapper, or every
        // `text_input::focus` task would stop at it.
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            self.child
                .as_widget_mut()
                .operate(tree, layout, renderer, operation);
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.child.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.child
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        // The tab bar hosts the inline rename input as an overlay
        // (tabs spec, T6); without forwarding, the rename would be
        // unreachable under the wrapper.
        self.child
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
    ) {
        // The only behavior the wrapper adds: clip to the surface
        // bounds, translate, and let the child paint. The child's
        // viewport follows the translation — the child paints
        // `offset_y` lower in window space, so a window-space visible
        // region maps to child-local space shifted up by the same
        // amount; the scrollable passes its child the same shifted
        // viewport, and TerminalBox sizes its visible region from it.
        let bounds = layout.bounds();
        let child_viewport = Rectangle {
            y: viewport.y - self.offset_y,
            ..*viewport
        };
        renderer.with_layer(bounds, |renderer| {
            renderer.with_translation(Vector::new(0.0, self.offset_y), |renderer| {
                self.child.as_widget().draw(
                    tree,
                    renderer,
                    theme,
                    style,
                    layout,
                    cursor,
                    &child_viewport,
                );
            });
        });
    }
}

impl<'a, Message> From<SlideBox<'a, Message>> for Element<'a, Message>
where
    Message: Clone + 'a,
{
    fn from(slide_box: SlideBox<'a, Message>) -> Self {
        Self::new(slide_box)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_edge_translates_upward_toward_the_anchored_edge() {
        // A slide on a top-anchored surface moves content up — toward
        // the anchored screen edge — never down into the screen.
        assert_eq!(translation_y(Edge::Top, 200.0), -200.0);
    }

    #[test]
    fn bottom_edge_translates_downward_toward_the_anchored_edge() {
        assert_eq!(translation_y(Edge::Bottom, 200.0), 200.0);
    }

    #[test]
    fn resting_offset_translates_nowhere() {
        // Offset 0 is the resting position: byte-identical to the
        // unwrapped column.
        assert_eq!(translation_y(Edge::Top, 0.0), 0.0);
        assert_eq!(translation_y(Edge::Bottom, 0.0), 0.0);
    }
}
