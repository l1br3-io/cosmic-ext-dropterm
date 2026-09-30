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

/// Which way a slide is moving (slide-animation spec): `In` shows the
/// content (offset decays to rest), `Out` hides it (offset grows to
/// the distance). `Out` slides are constructed by the animated-hide
/// path (A4's `hide()`) and by tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Showing: ease-out from `from_offset_px` to rest.
    In,
    /// Hiding: ease-in from `from_offset_px` to `distance_px`.
    Out,
}

/// A slide in progress (slide-animation spec, A2): plain data, no
/// clocks — elapsed time is injected at the call site. Offsets are
/// edge-space: `0.0` is the resting position, growing toward
/// `distance_px` as the content hides; the window-space sign is
/// applied only at the draw site, by [`translation_y`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Active {
    pub direction: Direction,
    /// Where this slide started, captured at start or reversal.
    pub from_offset_px: f32,
    /// The fully-hidden offset — the slide distance,
    /// `geometry::surface_height(output.h, height_percent)`, fixed at
    /// `SurfaceOpened` time (the plan's pinned distance source).
    pub distance_px: f32,
}

/// The content's edge-space offset `elapsed_ms` into `active`
/// (slide-animation spec, A2): eased from `from_offset_px` toward the
/// direction's endpoint over exactly [`SLIDE_DURATION_MS`] —
/// quadratic ease-out for [`Direction::In`] (fast in, settling at
/// rest), quadratic ease-in for [`Direction::Out`] (leaving gently,
/// accelerating off-screen). At or past completion the endpoint is
/// returned *directly*, not recomputed — `from + (distance - from)`
/// is not float-exact, and the completed pose must be byte-identical
/// to the resting (In) or fully-hidden (Out) one. Pure over injected
/// milliseconds: A3's `SlideBox::draw` feeds the wall clock at frame
/// presentation; tests feed fakes.
pub fn offset(active: &Active, elapsed_ms: u64) -> f32 {
    let progress = (elapsed_ms as f32 / SLIDE_DURATION_MS as f32).clamp(0.0, 1.0);
    if progress >= 1.0 {
        return match active.direction {
            Direction::In => 0.0,
            Direction::Out => active.distance_px,
        };
    }
    match active.direction {
        Direction::In => active.from_offset_px * (1.0 - progress).powi(2),
        Direction::Out => {
            active.from_offset_px + (active.distance_px - active.from_offset_px) * progress.powi(2)
        }
    }
}

/// Whether `active` has run its full [`SLIDE_DURATION_MS`]
/// (slide-animation spec, A2): time-based on purpose — the endpoint
/// offset at completion is already exact, and "the first tick past
/// the duration parks the slide" is a clock decision, not a pose
/// one. Monotone in `elapsed_ms`: a slide completes exactly once.
pub fn finished(_active: &Active, elapsed_ms: u64) -> bool {
    elapsed_ms >= SLIDE_DURATION_MS
}

/// Re-target `active` from wherever it stands at `elapsed_ms`
/// (slide-animation spec, A2 + the plan's reversal decision): capture
/// the current offset as the new `from_offset_px`, flip the
/// direction, keep the distance, and run a fresh fixed
/// [`SLIDE_DURATION_MS`] — the re-target is NOT proportional to the
/// remaining distance (plan decision, approved 2026-09-30). No
/// teleport: the reversed slide's frame-0 offset equals the old
/// slide's offset at the reversal instant.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "consumed by A5's mid-animation reversal; the spike never reverses"
    )
)]
pub fn reverse(active: &Active, elapsed_ms: u64) -> Active {
    Active {
        direction: match active.direction {
            Direction::In => Direction::Out,
            Direction::Out => Direction::In,
        },
        from_offset_px: offset(active, elapsed_ms),
        distance_px: active.distance_px,
    }
}

/// A live slide (slide-animation spec, A3): the animation state
/// ([`Active`]) paired with its *true* start instant — carried
/// through view rebuilds, never re-captured. The pair exists so
/// [`SlideBox::draw`] can evaluate the pose against the frame's own
/// clock: the runtime builds the view in its `AboutToWait` pass but
/// presents the frame on the compositor's frame callback (pinned rev
/// `08a09e3`, `iced/winit/src/lib.rs` `AboutToWait` →
/// `request_redraw` → `RedrawRequested`), so an offset baked at view
/// time is up to a full frame stale at presentation — random
/// per-frame error that reads as judder (the round-4 spike finding).
/// Evaluating in `draw` makes every presented frame carry the pose
/// for its presentation instant.
#[derive(Debug, Clone, Copy)]
pub struct Playing {
    /// When the slide started (the app's slide clock).
    pub started: std::time::Instant,
    /// What slides, from where, toward which endpoint.
    pub active: Active,
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
    /// A live slide (slide-animation spec, A3): when set, `draw`
    /// evaluates the eased offset itself (see [`Playing`]) translated
    /// per `edge`; `offset_y` is ignored until the slide completes.
    playing: Option<(Edge, Playing)>,
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
            playing: None,
        }
    }

    /// Animate the child per `playing`'s easing (slide-animation
    /// spec, A3): the eased edge-space offset is computed inside
    /// `draw` against the frame's own clock and signed toward `edge`
    /// by [`translation_y`] — never baked here in the view (the
    /// round-4 lesson; see [`Playing`]). `started` is the slide's
    /// true start — the same instant across every view rebuild.
    pub fn playing(mut self, edge: Edge, playing: Playing) -> Self {
        self.playing = Some((edge, playing));
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
        // clock — when a slide is live: the view that built this
        // widget ran in the runtime's AboutToWait pass, up to a full
        // frame before this draw executes on the compositor's frame
        // callback, so a view-baked offset judders (the round-4
        // finding; see `Playing`). The eased edge-space offset is
        // signed toward the anchored edge only here, at the last
        // possible moment (`translation_y` is the sign boundary).
        // The child's viewport follows the translation — the child
        // paints `offset_y` lower in window space, so a window-space
        // visible region maps to child-local space shifted up by the
        // same amount; the scrollable passes its child the same
        // shifted viewport, and TerminalBox sizes its visible region
        // from it.
        let offset_y = self.playing.map_or(self.offset_y, |(edge, playing)| {
            translation_y(
                edge,
                offset(
                    &playing.active,
                    playing.started.elapsed().as_millis() as u64,
                ),
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

    // --- Direction::In (show): quadratic ease-out from -> rest ---

    #[test]
    fn show_starts_at_the_from_offset_exactly() {
        let active = Active {
            direction: Direction::In,
            from_offset_px: 835.0,
            distance_px: 835.0,
        };
        assert_eq!(offset(&active, 0), 835.0);
    }

    #[test]
    fn show_rests_exactly_at_completion() {
        let active = Active {
            direction: Direction::In,
            from_offset_px: 835.0,
            distance_px: 835.0,
        };
        assert_eq!(offset(&active, SLIDE_DURATION_MS), 0.0);
        // A draw after the parking tick clamps to rest, never
        // overshoots.
        assert_eq!(offset(&active, SLIDE_DURATION_MS * 10), 0.0);
    }

    #[test]
    fn show_decays_monotonically_from_full_and_mid_offsets() {
        // Full slide and a reversed-in mid-offset: both only ever
        // move toward 0.0, sampled finer than the tick so the curve,
        // not the cadence, is under test.
        for from in [835.0, 412.5] {
            let active = Active {
                direction: Direction::In,
                from_offset_px: from,
                distance_px: 835.0,
            };
            let mut last = from;
            for ms in (0..=SLIDE_DURATION_MS).step_by(8) {
                let now = offset(&active, ms);
                assert!(
                    now <= last,
                    "offset grew at {ms} ms from {from}: {now} > {last}"
                );
                assert!(
                    now >= 0.0,
                    "offset flipped sign at {ms} ms from {from}: {now}"
                );
                last = now;
            }
        }
    }

    #[test]
    fn show_midpoint_quarters_the_offset() {
        // Quadratic ease-out at half the duration: (1 - 1/2)^2 = 1/4.
        let active = Active {
            direction: Direction::In,
            from_offset_px: 800.0,
            distance_px: 800.0,
        };
        assert_eq!(offset(&active, SLIDE_DURATION_MS / 2), 200.0);
    }

    // --- Direction::Out (hide): quadratic ease-in from -> distance ---

    #[test]
    fn hide_starts_at_the_from_offset_exactly() {
        let active = Active {
            direction: Direction::Out,
            from_offset_px: 0.0,
            distance_px: 835.0,
        };
        assert_eq!(offset(&active, 0), 0.0);
    }

    #[test]
    fn hide_reaches_the_distance_exactly_at_completion() {
        // No sub-pixel residue: from + (distance - from) * 1.0 is not
        // float-exact in general, so completion must return the
        // endpoint directly.
        let active = Active {
            direction: Direction::Out,
            from_offset_px: 0.1,
            distance_px: 835.7,
        };
        assert_eq!(offset(&active, SLIDE_DURATION_MS), 835.7);
        assert_eq!(offset(&active, SLIDE_DURATION_MS * 10), 835.7);
    }

    #[test]
    fn hide_grows_monotonically_toward_the_distance() {
        for from in [0.0, 412.5] {
            let active = Active {
                direction: Direction::Out,
                from_offset_px: from,
                distance_px: 835.0,
            };
            let mut last = from;
            for ms in (0..=SLIDE_DURATION_MS).step_by(8) {
                let now = offset(&active, ms);
                assert!(
                    now >= last,
                    "offset fell at {ms} ms from {from}: {now} < {last}"
                );
                assert!(
                    now <= 835.0,
                    "offset overshot the distance at {ms} ms: {now}"
                );
                last = now;
            }
        }
    }

    #[test]
    fn hide_midpoint_covers_a_quarter_of_the_gap() {
        // Quadratic ease-in at half the duration: (1/2)^2 = 1/4 of
        // the from->distance gap.
        let active = Active {
            direction: Direction::Out,
            from_offset_px: 0.0,
            distance_px: 800.0,
        };
        assert_eq!(offset(&active, SLIDE_DURATION_MS / 2), 200.0);
    }

    // --- finished ---

    #[test]
    fn finished_flips_exactly_once_at_the_duration() {
        let active = Active {
            direction: Direction::In,
            from_offset_px: 835.0,
            distance_px: 835.0,
        };
        assert!(!finished(&active, 0));
        assert!(!finished(&active, SLIDE_DURATION_MS - 1));
        assert!(finished(&active, SLIDE_DURATION_MS));
        assert!(finished(&active, SLIDE_DURATION_MS * 10));
        // Monotone in elapsed: once finished, always finished.
        for ms in 0..=SLIDE_DURATION_MS {
            assert_eq!(
                finished(&active, ms),
                ms >= SLIDE_DURATION_MS,
                "finished not monotone at {ms} ms"
            );
        }
    }

    // --- reverse ---

    #[test]
    fn reverse_starts_exactly_where_the_old_slide_stood() {
        // No teleport: the reversed slide's frame-0 offset equals the
        // old slide's offset at the reversal instant (exact float
        // equality — the same value is stored and returned).
        let showing = Active {
            direction: Direction::In,
            from_offset_px: 835.0,
            distance_px: 835.0,
        };
        let reversed = reverse(&showing, 40);
        assert_eq!(offset(&reversed, 0), offset(&showing, 40));

        let hiding = Active {
            direction: Direction::Out,
            from_offset_px: 0.0,
            distance_px: 835.0,
        };
        let reversed = reverse(&hiding, 72);
        assert_eq!(offset(&reversed, 0), offset(&hiding, 72));
    }

    #[test]
    fn reverse_flips_direction_and_preserves_distance() {
        let showing = Active {
            direction: Direction::In,
            from_offset_px: 835.0,
            distance_px: 835.0,
        };
        let reversed = reverse(&showing, 40);
        assert_eq!(reversed.direction, Direction::Out);
        assert_eq!(reversed.distance_px, 835.0);

        let hiding = Active {
            direction: Direction::Out,
            from_offset_px: 0.0,
            distance_px: 835.0,
        };
        let reversed = reverse(&hiding, 40);
        assert_eq!(reversed.direction, Direction::In);
        assert_eq!(reversed.distance_px, 835.0);
    }

    #[test]
    fn reversed_slide_completes_exactly_once_on_its_own_clock() {
        // The re-target is a fresh fixed 130 ms — measured from the
        // reversal, not the original start.
        let showing = Active {
            direction: Direction::In,
            from_offset_px: 835.0,
            distance_px: 835.0,
        };
        let reversed = reverse(&showing, 40);
        assert!(!finished(&reversed, SLIDE_DURATION_MS - 1));
        assert!(finished(&reversed, SLIDE_DURATION_MS));
    }

    #[test]
    fn reverse_at_rest_targets_the_full_distance() {
        // Reversing a completed show (at rest, offset 0) hides from
        // rest — the standard A4/A5 entry point.
        let showing = Active {
            direction: Direction::In,
            from_offset_px: 835.0,
            distance_px: 835.0,
        };
        let reversed = reverse(&showing, SLIDE_DURATION_MS);
        assert_eq!(offset(&reversed, 0), 0.0);
        assert_eq!(reversed.direction, Direction::Out);
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
