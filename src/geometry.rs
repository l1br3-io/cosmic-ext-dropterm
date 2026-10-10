// SPDX-License-Identifier: GPL-3.0-only

//! Pure geometry math plus the single layer-surface application path.
//!
//! All percent/anchor/margin math lives here as plain functions over
//! plain data — no compositor, no iced runtime state. Every transition
//! that shapes the surface (show, config reshape, drag) goes through
//! [`apply_geometry`], so no path can leave a stale anchor, margin, or
//! size behind.

use std::time::{Duration, Instant};

use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    Anchor, set_anchor, set_margin, set_size,
};
use cosmic::iced::runtime::Task;
use cosmic::iced::window;

use crate::config::{
    DropTermConfig, Edge, HAlign, MAX_HEIGHT_PERCENT, MAX_WIDTH_PERCENT, MIN_HEIGHT_PERCENT,
    MIN_WIDTH_PERCENT,
};

/// Fixed pixel height of the shown surface: `height_percent` of the
/// output. The surface never shrinks for the tab bar — the terminal
/// widget does (see `surface::terminal_view_height`).
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "rounded percent share floored by max(1.0): positive px, truncation is the size contract"
)]
pub fn surface_height(output_height: f32, height_percent: u8) -> u32 {
    (output_height * f32::from(height_percent) / 100.0)
        .round()
        .max(1.0) as u32
}

/// Fixed pixel width to request: `width_percent` of the measured output
/// width. Never rounds below one pixel.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "rounded percent share floored by max(1.0): positive px, truncation is the size contract"
)]
pub fn surface_width(output_width: f32, width_percent: u8) -> u32 {
    (output_width * f32::from(width_percent) / 100.0)
        .round()
        .max(1.0) as u32
}

/// Layer-shell margins, in the top/right/bottom/left order
/// [`set_margin`](cosmic::iced::platform_specific::shell::commands::layer_surface::set_margin)
/// takes them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Margins {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

/// The geometry subset of the config that shapes the layer surface.
/// Construction from a config clamps the percents into range, so a
/// `Geometry` can never carry an out-of-band value onward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub edge: Edge,
    pub height_percent: u8,
    pub width_percent: u8,
    pub h_align: HAlign,
}

impl Geometry {
    /// The geometry of a config, with percents clamped into their
    /// bands: always clamp before applying, whoever calls.
    pub fn from_config(config: &DropTermConfig) -> Self {
        Self {
            edge: config.edge,
            height_percent: config
                .height_percent
                .clamp(MIN_HEIGHT_PERCENT, MAX_HEIGHT_PERCENT),
            width_percent: config
                .width_percent
                .clamp(MIN_WIDTH_PERCENT, MAX_WIDTH_PERCENT),
            h_align: config.h_align,
        }
    }
}

/// Compose the layer-shell anchor set and margins for a geometry.
///
/// Full width keeps both horizontal anchors and zero margins. Narrower
/// widths drop the anchor on the aligned side; `Center` keeps both
/// anchors and offsets with equal margins, the odd leftover pixel
/// going right so the total width stays exact.
pub fn anchor_margins(geom: Geometry, output: (u32, u32)) -> (Anchor, Margins) {
    let vertical = match geom.edge {
        Edge::Top => Anchor::TOP,
        Edge::Bottom => Anchor::BOTTOM,
    };
    #[expect(
        clippy::cast_possible_wrap,
        reason = "center margins are half the output slack (< 2^31 px by any display standard)"
    )]
    let (horizontal, margins) = if geom.width_percent >= MAX_WIDTH_PERCENT {
        (Anchor::LEFT | Anchor::RIGHT, Margins::default())
    } else {
        match geom.h_align {
            HAlign::Left => (Anchor::LEFT, Margins::default()),
            HAlign::Right => (Anchor::RIGHT, Margins::default()),
            HAlign::Center => {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "output px are < 2^24, exact in f32"
                )]
                let width_px = surface_width(output.0 as f32, geom.width_percent);
                let slack = output.0.saturating_sub(width_px);
                let left = slack / 2;
                (
                    Anchor::LEFT | Anchor::RIGHT,
                    Margins {
                        left: left as i32,
                        right: (slack - left) as i32,
                        ..Margins::default()
                    },
                )
            }
        }
    };
    (vertical | horizontal, margins)
}

/// The size request for a geometry: height is always a fixed pixel
/// value; width is `None` at 100% — which the compositor reads as
/// *fill*, not *unchanged* — and fixed pixels below.
#[expect(
    clippy::cast_precision_loss,
    reason = "output px are < 2^24, exact in f32"
)]
pub fn requested_size(geom: Geometry, output: (u32, u32)) -> (Option<u32>, u32) {
    let width = (geom.width_percent < MAX_WIDTH_PERCENT)
        .then(|| surface_width(output.0 as f32, geom.width_percent));
    (width, surface_height(output.1 as f32, geom.height_percent))
}

/// Send the `set_anchor` + `set_size` + `set_margin` triple, always all
/// three — zero margins included — so no transition can leave stale
/// anchors, margins, or sizes behind (e.g. width 60 -> 100 must re-send
/// zero margins, or the full-width surface stays shrunk).
fn surface_tasks<M: 'static>(
    id: window::Id,
    geom: Geometry,
    output: (u32, u32),
    height: u32,
) -> Task<M> {
    let (anchor, margins) = anchor_margins(geom, output);
    let (width, _) = requested_size(geom, output);
    Task::batch([
        set_anchor(id, anchor),
        set_size(id, width, Some(height)),
        set_margin(id, margins.top, margins.right, margins.bottom, margins.left),
    ])
}

/// The single path every settled geometry change goes through: anchors,
/// margins, and width composed from `geom`; height from its
/// (percent-quantized) config field.
pub fn apply_geometry<M: 'static>(id: window::Id, geom: Geometry, output: (u32, u32)) -> Task<M> {
    let (_, height) = requested_size(geom, output);
    surface_tasks::<M>(id, geom, output, height)
}

/// [`apply_geometry`], but with the height pinned to live drag pixels:
/// while a grab-bar drag is in progress the height follows the pointer
/// one rounding step per move (see [`drag_target_px`]) instead of
/// percent-quantized jumps. Anchors, margins, and width still compose
/// from `geom`, through the same triple as every other transition; the
/// release path re-applies through [`apply_geometry`], so the settled
/// surface matches the percent that gets persisted.
pub fn apply_drag_geometry<M: 'static>(
    id: window::Id,
    geom: Geometry,
    output: (u32, u32),
    height_px: u32,
) -> Task<M> {
    surface_tasks::<M>(id, geom, output, height_px)
}

/// Whether two geometries differ in any field that shapes the surface.
///
/// The gate for in-place reshapes on config edits: non-geometry config
/// changes (font, colors, …) compare equal, so they never touch the
/// surface; a hidden surface just applies the new geometry on its next
/// show.
pub fn geometry_changed(old: Geometry, new: Geometry) -> bool {
    old != new
}

/// The fixed references of an in-progress grab-bar drag: everything a
/// live drag target recomputes from, per move, in pixels.
///
/// Targets are recomputed absolutely — never by accumulating deltas —
/// because percent quantization would turn sub-percent travel into
/// dead zones and ~1 %-of-output jumps, and the window frame of a
/// bottom-anchored surface moves as it grows upward, so surface-local
/// deltas would mix pointer travel with the frame shift. Absolute
/// targets self-correct: every move lands on the exact height the
/// pointer implies, whichever edge, whatever the frame did last event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragAnchor {
    /// The surface height (px) when the drag armed.
    pub anchored_px: u32,
    /// The height (px) this drag last applied — the live base for
    /// `Edge::Bottom` targets, which rebase against the height
    /// actually applied because that is what positions the window
    /// frame.
    pub applied_px: u32,
    /// The pointer's surface-local y when the drag armed. Window y, not
    /// bar-local y: `CursorMoved` positions are window-absolute.
    pub press_y: f32,
}

/// The pixel height a drag currently implies: `pointer_y` is the
/// pointer's *window-absolute* y this move.
///
/// `Edge::Top` targets `anchored_px + (pointer_y − press_y)` — the
/// window frame of a top-anchored surface is fixed, so travel since the
/// arm is exact. `Edge::Bottom` targets `applied_px + (press_y −
/// pointer_y)` — rebased on the height actually applied, so the moving
/// frame of an upward-growing surface cannot compound drift. Both
/// pull away from the screen edge to grow, clamp to the 10–100 % band
/// in pixels, and never target below one pixel.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "px-band drag math: output/drag px < 2^24 exact in f32; round+clamp+max(1.0) keeps px positive"
)]
pub fn drag_target_px(edge: Edge, output_height: u32, drag: DragAnchor, pointer_y: f32) -> u32 {
    let (base, grow) = match edge {
        // Free edge at the bottom: dragging down grows; frame fixed.
        Edge::Top => (drag.anchored_px as f32, pointer_y - drag.press_y),
        // Free edge at the top: dragging up grows; frame moves with
        // each apply, so the base is the live applied height.
        Edge::Bottom => (drag.applied_px as f32, drag.press_y - pointer_y),
    };
    let min_px = surface_height(output_height as f32, MIN_HEIGHT_PERCENT) as f32;
    let max_px = surface_height(output_height as f32, MAX_HEIGHT_PERCENT) as f32;
    (base + grow).round().clamp(min_px, max_px).max(1.0) as u32
}

/// The whole-percent config value a dragged pixel height settles at:
/// percent quantization happens once, at
/// release — the settled surface may snap by up to half a percent from
/// the dragged pixels, the accepted tolerance — rounded to nearest and
/// clamped to the band.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "px -> whole percent: < 2^24 px exact in f32; round+clamp keeps the result in the u8 band"
)]
pub fn percent_from_px(output_height: u32, height_px: u32) -> u8 {
    (height_px as f32 / output_height.max(1) as f32 * 100.0)
        .round()
        .clamp(f32::from(MIN_HEIGHT_PERCENT), f32::from(MAX_HEIGHT_PERCENT)) as u8
}

/// Minimum spacing between live drag applies.
///
/// The compositor grants each `set_size` exactly and promptly, but the
/// runtime double-reports every apply — it synthesizes a second
/// configure per `set_size` carrying the previous height — so each
/// apply becomes two window resizes, terminal reflows, and pty resizes
/// in the app. Per-move applies at pointer rate saturate the single
/// runtime thread, so applies are change-gated and spaced at least
/// this far apart; the release settle is immediate regardless.
/// Tunable: raise if a drag still starves input, lower (toward one
/// frame) if drags feel steppy.
pub const DRAG_APPLY_INTERVAL: Duration = Duration::from_millis(80);

/// Minimum pixel distance between live drag applies: each apply costs
/// the double-reported resize pipeline, so a drag only retargets once
/// the surface has been given room to move — it trails the hand by at
/// most this much mid-drag while the applied height stays the true
/// target, and the release settle is exact regardless. Slow drags
/// (under one deadband per interval) skip redundant applies entirely.
pub const DRAG_APPLY_DEADBAND: u32 = 16;

/// Whether a live drag target should be applied now: `last` is the
/// previous apply (when, which target). The first move of a drag
/// always applies — no grab latency — and later applies need the
/// target beyond the last applied one by [`DRAG_APPLY_DEADBAND`] *and*
/// an elapsed [`DRAG_APPLY_INTERVAL`], bounding the resize/reflow
/// storm. Skipped targets land via the trailing apply or the release
/// settle, so throttling never loses the final height.
pub fn drag_apply_due(last: Option<(Instant, u32)>, now: Instant, target: u32) -> bool {
    match last {
        None => true,
        Some((at, applied)) => {
            target.abs_diff(applied) >= DRAG_APPLY_DEADBAND
                && now.duration_since(at) >= DRAG_APPLY_INTERVAL
        }
    }
}

/// How often the terminal's pinned height may follow a *changing*
/// granted size: every `set_size` reaches the app twice — once as the
/// stale synthetic configure, once as the real granted size — and a
/// drag applies every [`DRAG_APPLY_INTERVAL`], so the granted stream
/// outpaces what the terminal can absorb: each adoption costs a
/// terminal reflow plus a pty resize whose SIGWINCH makes the shell
/// redraw. The pin adopts a changed height at most this often (a
/// trailing adopt lands the final size once the stream settles), so a
/// drag reflows the terminal a handful of times instead of on every
/// echo. Identical heights always pass: a re-report of the size the
/// pin already has is free.
pub const WINDOW_PIN_INTERVAL: Duration = Duration::from_millis(150);

/// Whether the terminal pin should adopt a granted height now:
/// `last` is the previous adoption (when, which
/// height). The first size adopts immediately — there is nothing to
/// debounce before the terminal exists — and later changed heights
/// wait out [`WINDOW_PIN_INTERVAL`], with the caller landing skipped
/// heights via a trailing adopt.
#[expect(
    clippy::float_cmp,
    reason = "echo detection: the compositor returns the exact f32 it was granted; equality is the re-report test"
)]
pub fn window_pin_due(last: Option<(Instant, f32)>, now: Instant, height: f32) -> bool {
    match last {
        None => true,
        // Re-reports of the adopted height always pass — adopting them
        // is free — and the caller must not refresh the adoption
        // clock on them, or an echo stream would starve changed
        // heights of their interval forever.
        Some((at, adopted)) => height == adopted || now.duration_since(at) >= WINDOW_PIN_INTERVAL,
    }
}

/// How long after a grab-bar drag settles before its height percent is
/// persisted to config: long enough that rapid
/// re-drags coalesce into one write, short enough that a hide/show
/// right after a drag soon reopens at the persisted height.
pub const WRITE_BACK_DEBOUNCE: Duration = Duration::from_millis(400);

/// A drag-release config persist that has been scheduled but not yet
/// written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingWrite {
    /// Which drag release owns this write. Every new release bumps the
    /// generation, orphaning the `WriteDue` messages of older ones.
    pub generation: u32,
    /// The settled height percent, captured at release so whatever
    /// happens inside the debounce window — including a hide/show —
    /// cannot change what lands in the file.
    pub height_percent: u8,
}

/// State of the debounced drag-release config write: pure bookkeeping
/// over generations. The wait itself is a
/// `tokio::time::sleep` the app tasks in, so this machine needs no
/// clock and stays headless-testable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteBack {
    /// Source of monotonically increasing write generations, never
    /// reused within a run.
    next_generation: u32,
    pending: Option<PendingWrite>,
}

impl WriteBack {
    /// Schedules the delayed persist of a drag release, returning the
    /// pending write its `WriteDue` message must carry. Scheduling
    /// again — a newer drag settling inside the window — supersedes the
    /// earlier write, whose `WriteDue` turns stale.
    pub fn schedule(&mut self, height_percent: u8) -> PendingWrite {
        let pending = PendingWrite {
            generation: self.next_generation,
            height_percent,
        };
        self.next_generation += 1;
        self.pending = Some(pending);
        pending
    }

    /// Takes the pending write when `generation` still owns it. `None`
    /// for a stale generation — a newer drag rescheduled — or one
    /// already written.
    pub fn due(&mut self, generation: u32) -> Option<PendingWrite> {
        match self.pending {
            Some(pending) if pending.generation == generation => self.pending.take(),
            _ => None,
        }
    }

    /// Whether a drag write-back is still scheduled; the app consults
    /// this before canceling on a bare settings write.
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Cancels the pending write, if any: a bare settings write of the
    /// height key supersedes a pending
    /// drag write-back — newest intent wins — so the stale `WriteDue`
    /// finds nothing pending and the drag's percent never lands after
    /// the bare write's. A drag release after the cancel simply
    /// schedules a fresh generation and lands last. Returns the
    /// canceled write, if one was pending.
    pub fn cancel_pending(&mut self) -> Option<PendingWrite> {
        self.pending.take()
    }
}

/// Whether a [`ConfigChanged`] echo is our own geometry write coming
/// back: all four geometry fields equal to the last write's. Compares
/// config fields, never pixels — percent↔px quantization makes pixel
/// comparison flaky. A match is absorbed (applied silently, no
/// rewrite, no reshape, record cleared); anything else is an external
/// edit and reshapes live.
///
/// [`ConfigChanged`]: crate::app::Message::ConfigChanged
pub fn is_own_write(written: Geometry, echo: Geometry) -> bool {
    written == echo
}

#[cfg(test)]
mod tests;
