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
/// decision, amended round 5): the ticks exist only to provoke redraw
/// requests — the eased offset itself is evaluated in
/// [`SlideBox::draw`] at frame presentation time, so tick timing never
/// distorts the pose. The cadence must guarantee that EVERY compositor
/// frame window contains at least one pending redraw request: a frame
/// callback that finds none draws nothing (the surface re-presents its
/// previous buffer — a held frame, then a jump: the residual jitter of
/// rounds 3–4), so the tick period must be shorter than the frame
/// period at any refresh rate the output may run. 1 ms covers panels
/// up to 1000 Hz and absorbs runtime scheduling noise; it is
/// redraw-only and a state no-op (~130 messages per 130 ms slide,
/// batched by the runtime's AboutToWait pass — no layer-shell
/// traffic). Safe by the G3 evidence (the drag incident saturated the
/// runtime on `set_size` storms, not redraws).
pub const SLIDE_TICK: Duration = Duration::from_millis(1);

/// The A1 spike's eased offset (slide-animation spec, temporary — the
/// A2 state machine generalizes this): quadratic ease-out from
/// `from_offset_px` to rest (`0.0`) over [`SLIDE_DURATION_MS`],
/// clamped so a completed slide rests at exactly `0.0` — the final
/// frame is identical to the resting one, no sub-pixel residue. Pure
/// over injected milliseconds: `draw` feeds wall elapsed, tests feed
/// fakes.
pub fn spike_offset(from_offset_px: f32, elapsed_ms: u64) -> f32 {
    let progress = (elapsed_ms as f32 / SLIDE_DURATION_MS as f32).clamp(0.0, 1.0);
    let remaining = 1.0 - progress;
    from_offset_px * remaining * remaining
}

/// Draw-time easing state for the A1 spike (temporary — A3's state
/// machine generalizes this): the animation's *true* start instant —
/// carried through view rebuilds, never re-captured — and the signed
/// window-space offset the content eases from. The pair exists so
/// [`SlideBox::draw`] can evaluate the pose against the frame's own
/// clock: the runtime builds the view in its `AboutToWait` pass but
/// presents the frame on the compositor's frame callback (pinned rev
/// `08a09e3`, `iced/winit/src/lib.rs` `AboutToWait` → `request_redraw`
/// → `RedrawRequested`), so an offset baked at view time is up to a
/// full frame stale at presentation — random per-frame error that
/// reads as judder (round 4 finding). Evaluating in `draw` makes every
/// presented frame carry the pose for its presentation instant.
pub struct Spike {
    /// When the animation started (the spike clock, `spike_shown_at`).
    pub started: std::time::Instant,
    /// The signed window-space offset at the animation's start:
    /// `translation_y(edge, distance_px)` — the full slide distance,
    /// so the content eases in from fully off-rest toward `0.0`.
    pub from_offset_y: f32,
}

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
    /// The A1 spike animation (temporary): when set, `draw` evaluates
    /// the eased offset itself (see [`Spike`]); `offset_y` is ignored
    /// until the animation completes.
    spike: Option<Spike>,
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
            spike: None,
        }
    }

    /// A1 spike (slide-animation spec, temporary): ease from
    /// `from_offset_y` to rest, evaluated at draw time. `started` is
    /// the animation's true start — the same instant across every view
    /// rebuild — so `draw` can compute the frame-accurate pose.
    pub fn spike_from(mut self, started: std::time::Instant, from_offset_y: f32) -> Self {
        self.spike = Some(Spike {
            started,
            from_offset_y,
        });
        self
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
        // bounds, translate, and let the child paint. The offset is
        // evaluated HERE — at frame presentation, on the frame's own
        // clock — when a spike animation is live: the view that built
        // this widget ran in the runtime's AboutToWait pass, up to a
        // full frame before this draw executes on the compositor's
        // frame callback, so a view-baked offset judders (round 4
        // finding; see `Spike`). The child's viewport follows the
        // translation — the child paints `offset_y` lower in window
        // space, so a window-space visible region maps to child-local
        // space shifted up by the same amount; the scrollable passes
        // its child the same shifted viewport, and TerminalBox sizes
        // its visible region from it.
        let offset_y = self.spike.as_ref().map_or(self.offset_y, |spike| {
            spike_offset(
                spike.from_offset_y,
                spike.started.elapsed().as_millis() as u64,
            )
        });
        let bounds = layout.bounds();
        let child_viewport = Rectangle {
            y: viewport.y - offset_y,
            ..*viewport
        };
        renderer.with_layer(bounds, |renderer| {
            renderer.with_translation(Vector::new(0.0, offset_y), |renderer| {
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
    fn spike_offset_starts_at_the_from_offset() {
        // At the animation's first frame the content sits exactly at
        // the from-offset — no teleport before the easing begins.
        assert_eq!(spike_offset(835.0, 0), 835.0);
        assert_eq!(spike_offset(-835.0, 0), -835.0);
    }

    #[test]
    fn spike_offset_rests_exactly_at_completion() {
        // The completed slide is exactly rest — no sub-pixel residue
        // that would make the final frame differ from the resting one.
        assert_eq!(spike_offset(835.0, SLIDE_DURATION_MS), 0.0);
        assert_eq!(spike_offset(-835.0, SLIDE_DURATION_MS), 0.0);
        // Late evaluation (a draw after the parking tick dropped the
        // clock) clamps to rest, never overshoots.
        assert_eq!(spike_offset(835.0, SLIDE_DURATION_MS * 10), 0.0);
    }

    #[test]
    fn spike_offset_decays_monotonically_toward_rest() {
        // Sampled over fake timestamps spanning the whole slide (and a
        // step finer than the tick so the curve, not the cadence, is
        // under test): the offset only ever moves toward 0.0, from
        // either sign — the show curve is quadratic ease-out.
        let mut last = 835.0;
        for ms in (0..=SLIDE_DURATION_MS).step_by(8) {
            let offset = spike_offset(835.0, ms);
            assert!(offset <= last, "offset grew at {ms} ms: {offset} > {last}");
            assert!(offset >= 0.0, "offset flipped sign at {ms} ms: {offset}");
            last = offset;
        }
        let mut last = -835.0;
        for ms in (0..=SLIDE_DURATION_MS).step_by(8) {
            let offset = spike_offset(-835.0, ms);
            assert!(
                offset >= last,
                "offset grew in magnitude at {ms} ms: {offset} < {last}"
            );
            assert!(offset <= 0.0, "offset flipped sign at {ms} ms: {offset}");
            last = offset;
        }
    }

    #[test]
    fn spike_offset_midpoint_quarters_the_distance() {
        // Quadratic ease-out at half the duration: remaining = 1/2,
        // offset = from * (1/2)^2 = from/4.
        let midpoint = SLIDE_DURATION_MS / 2;
        assert_eq!(spike_offset(800.0, midpoint), 200.0);
        assert_eq!(spike_offset(-800.0, midpoint), -200.0);
    }

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
