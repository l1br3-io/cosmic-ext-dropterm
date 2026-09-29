// SPDX-License-Identifier: GPL-3.0-only

//! Pure geometry math plus the single layer-surface application path
//! (geometry spec).
//!
//! All percent/anchor/margin math lives here as plain functions over
//! plain data — no compositor, no iced runtime state — the same
//! separation as `tabs.rs`. Every transition that shapes the surface
//! (show, config reshape, drag) funnels through [`apply_geometry`], so
//! no path can leave a stale anchor, margin, or size behind.

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

/// Fixed pixel height to request for the shown surface: `height_percent`
/// of the output, bar or no bar. The surface height never changes while
/// shown — when the tab bar appears, the terminal widget is pinned to
/// the window height minus the bar (see
/// `surface::terminal_view_height`), so the bar takes its strip out of
/// the terminal's rows, Yakuake-style, rather than out of the screen.
pub fn surface_height(output_height: f32, height_percent: u8) -> u32 {
    (output_height * f32::from(height_percent) / 100.0)
        .round()
        .max(1.0) as u32
}

/// Fixed pixel width to request: `width_percent` of the measured output
/// width. Never rounds below one pixel.
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
    /// The geometry of a config, percents clamped into their bands (the
    /// module's standing rule: clamp before applying, whoever calls).
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
/// A width of 100% keeps both horizontal anchors and zero margins —
/// identical to the v1 surface. Narrower widths drop the anchor on the
/// aligned side; `Center` keeps both anchors and offsets with equal
/// margins, the odd leftover pixel going right so the total width stays
/// exact.
pub fn anchor_margins(geom: &Geometry, output: (u32, u32)) -> (Anchor, Margins) {
    let vertical = match geom.edge {
        Edge::Top => Anchor::TOP,
        Edge::Bottom => Anchor::BOTTOM,
    };
    let (horizontal, margins) = if geom.width_percent >= MAX_WIDTH_PERCENT {
        (Anchor::LEFT | Anchor::RIGHT, Margins::default())
    } else {
        match geom.h_align {
            HAlign::Left => (Anchor::LEFT, Margins::default()),
            HAlign::Right => (Anchor::RIGHT, Margins::default()),
            HAlign::Center => {
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
pub fn requested_size(geom: &Geometry, output: (u32, u32)) -> (Option<u32>, u32) {
    let width = (geom.width_percent < MAX_WIDTH_PERCENT)
        .then(|| surface_width(output.0 as f32, geom.width_percent));
    (width, surface_height(output.1 as f32, geom.height_percent))
}

/// Send the `set_anchor` + `set_size` + `set_margin` triple, always all
/// three — zero margins included — so no transition can leave stale
/// anchors, margins, or sizes behind (e.g. width 60 -> 100 must re-send
/// zero margins, or the "full-width" surface stays shrunk). Every
/// geometry transition funnels through here.
fn surface_tasks<M: 'static>(
    id: window::Id,
    geom: &Geometry,
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
pub fn apply_geometry<M: 'static>(id: window::Id, geom: &Geometry, output: (u32, u32)) -> Task<M> {
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
    geom: &Geometry,
    output: (u32, u32),
    height_px: u32,
) -> Task<M> {
    surface_tasks::<M>(id, geom, output, height_px)
}

/// Whether two geometries differ in any field that shapes the surface.
///
/// The gate for in-place reshapes on config edits (geometry spec, G2):
/// non-geometry config changes (font, colors, …) compare equal, so they
/// never touch the surface; a hidden surface just applies the new
/// geometry on its next show.
pub fn geometry_changed(old: &Geometry, new: &Geometry) -> bool {
    old != new
}

/// The fixed references of an in-progress grab-bar drag (geometry
/// spec, G3): everything a live drag target recomputes from, per move,
/// in pixels.
///
/// Targets are recomputed absolutely — never by accumulating deltas —
/// because both culprits of the original steppy drag were delta
/// artifacts: percent quantization turned sub-percent travel into
/// dead zones and ~1 %-of-output jumps, and the window frame of a
/// bottom-anchored surface *moves* as it grows upward, so surface-local
/// deltas mixed pointer travel with the frame shift. Absolute targets
/// self-correct: every move lands on the exact height the pointer
/// implies, whichever edge, whatever the frame did last event.
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

/// The pixel height a drag currently implies (geometry spec, G3):
/// `pointer_y` is the pointer's *window-absolute* y this move.
///
/// `Edge::Top` targets `anchored_px + (pointer_y − press_y)` — the
/// window frame of a top-anchored surface is fixed, so travel since the
/// arm is exact. `Edge::Bottom` targets `applied_px + (press_y −
/// pointer_y)` — rebased on the height actually applied, so the moving
/// frame of an upward-growing surface cannot compound drift. Both
/// pull away from the screen edge to grow, clamp to the 10–100 % band
/// in pixels, and never target below one pixel.
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

/// The whole-percent config value a dragged pixel height settles at
/// (geometry spec, G3/G4): percent quantization happens once, at
/// release — the settled surface may snap by up to half a percent from
/// the dragged pixels, the accepted tolerance — rounded to nearest and
/// clamped to the band.
pub fn percent_from_px(output_height: u32, height_px: u32) -> u8 {
    (height_px as f32 / output_height.max(1) as f32 * 100.0)
        .round()
        .clamp(f32::from(MIN_HEIGHT_PERCENT), f32::from(MAX_HEIGHT_PERCENT)) as u8
}

/// Minimum spacing between live drag applies (geometry spec, G3):
/// the compositor grants each `set_size` exactly and promptly (hardware
/// measured: ~10–40 ms, no easing), but the runtime double-reports
/// every apply — it synthesizes a second configure per `set_size`
/// carrying the *previous* height (see the G3 hardware notes) — so each
/// apply becomes two window resizes, terminal reflows, and pty resizes
/// in the app. Per-move applies at pointer rate saturate the single
/// runtime thread — the drag froze input and the surface flickered
/// stale/new on hardware — so applies are change-gated and spaced at
/// least this far apart. The release settle is immediate regardless.
/// Tunable: raise if a drag still starves input, lower (toward one
/// frame) if drags feel steppy on calm hardware.
pub const DRAG_APPLY_INTERVAL: Duration = Duration::from_millis(80);

/// Minimum pixel distance between live drag applies (geometry spec,
/// G3): each apply costs the double-reported resize pipeline above,
/// so a drag may only retarget once the surface has been given room
/// to actually move — the surface trails the hand by at most this
/// much mid-drag (about one percent of a typical output) while the
/// applied height is the true live target, and the release settle is
/// exact regardless. Slow drags (under one deadband per interval)
/// skip redundant applies entirely instead of re-reporting sizes the
/// surface already has.
pub const DRAG_APPLY_DEADBAND: u32 = 16;

/// Whether a live drag target should be applied now (geometry spec,
/// G3): `last` is the previous apply (when, which target). The first
/// move of a drag always applies — no grab latency — and later
/// applies need the target far enough beyond the last applied one
/// ([`DRAG_APPLY_DEADBAND`], keeping the double-reported resize
/// pipeline off the runtime thread for sub-deadband drift) *and* an
/// elapsed [`DRAG_APPLY_INTERVAL`], bounding the resize/reflow storm.
/// Skipped targets land via the trailing apply or the release settle,
/// so throttling never loses the final height.
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
/// granted size (geometry spec, G3): every `set_size` reaches the app
/// twice — once as the stale synthetic configure, once as the real
/// granted size (see the G3 hardware notes) — and a drag applies every
/// [`DRAG_APPLY_INTERVAL`], so the granted stream outpaces what the
/// terminal can absorb: each adoption costs a terminal reflow plus a
/// pty resize whose SIGWINCH makes the shell redraw — the storm behind
/// seconds of post-drag sluggishness. The pin adopts a changed height
/// at most this often (a trailing adopt lands the final size once the
/// stream settles), so a drag reflows the terminal a handful of times
/// instead of on every echo. Identical heights always pass: a
/// re-report of the size the pin already has is free.
pub const WINDOW_PIN_INTERVAL: Duration = Duration::from_millis(150);

/// Whether the terminal pin should adopt a granted height now
/// (geometry spec, G3): `last` is the previous adoption (when, which
/// height). The first size adopts immediately — there is nothing to
/// debounce before the terminal exists — and later changed heights
/// wait out [`WINDOW_PIN_INTERVAL`], with the caller landing skipped
/// heights via a trailing adopt.
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
/// persisted to config (geometry spec, G4): long enough that rapid
/// re-drags coalesce into one write, short enough that a hide/show
/// right after a drag soon reopens at the persisted height.
pub const WRITE_BACK_DEBOUNCE: Duration = Duration::from_millis(400);

/// A drag-release config persist that has been scheduled but not yet
/// written (geometry spec, G4).
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

/// State of the debounced drag-release config write (geometry spec,
/// G4): pure bookkeeping over generations. The wait itself is a
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
}

/// Whether a [`ConfigChanged`] echo is our own geometry write coming
/// back (geometry spec, G4): all four geometry fields equal to the last
/// write's. Compares config fields, never pixels — percent↔px
/// quantization makes pixel comparison flaky. A match is absorbed
/// (applied silently, no rewrite, no reshape, record cleared); anything
/// else is an external edit and reshapes live via G2.
///
/// [`ConfigChanged`]: crate::app::Message::ConfigChanged
pub fn is_own_write(written: &Geometry, echo: &Geometry) -> bool {
    written == echo
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: (u32, u32) = (1920, 1080);

    fn vertical_of(edge: Edge) -> Anchor {
        match edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        }
    }

    #[test]
    fn height_percent_maps_to_pixels_with_rounding() {
        assert_eq!(surface_height(1440.0, 40), 576);
        assert_eq!(surface_height(1440.0, 100), 1440);
    }

    #[test]
    fn height_never_rounds_below_one_pixel() {
        assert_eq!(surface_height(1440.0, 0), 1);
    }

    #[test]
    fn width_percent_maps_to_pixels_with_rounding() {
        assert_eq!(surface_width(1920.0, 100), 1920);
        assert_eq!(surface_width(1000.0, 60), 600);
        assert_eq!(surface_width(1440.0, 33), 475);
    }

    #[test]
    fn width_never_rounds_below_one_pixel() {
        assert_eq!(surface_width(1920.0, 0), 1);
    }

    #[test]
    fn full_width_keeps_both_anchors_and_zero_margins_whatever_the_alignment() {
        // 100% width must stay byte-for-byte the v1 surface; the
        // alignment is moot at full width.
        for edge in [Edge::Top, Edge::Bottom] {
            for h_align in [HAlign::Left, HAlign::Center, HAlign::Right] {
                let geom = Geometry {
                    edge,
                    height_percent: 40,
                    width_percent: 100,
                    h_align,
                };
                let (anchor, margins) = anchor_margins(&geom, OUTPUT);
                assert_eq!(anchor, vertical_of(edge) | Anchor::LEFT | Anchor::RIGHT);
                assert_eq!(margins, Margins::default());
            }
        }
    }

    #[test]
    fn left_alignment_drops_the_right_anchor_below_full_width() {
        for edge in [Edge::Top, Edge::Bottom] {
            let geom = Geometry {
                edge,
                height_percent: 40,
                width_percent: 60,
                h_align: HAlign::Left,
            };
            let (anchor, margins) = anchor_margins(&geom, OUTPUT);
            assert_eq!(anchor, vertical_of(edge) | Anchor::LEFT);
            assert_eq!(margins, Margins::default());
        }
    }

    #[test]
    fn right_alignment_drops_the_left_anchor_below_full_width() {
        for edge in [Edge::Top, Edge::Bottom] {
            let geom = Geometry {
                edge,
                height_percent: 40,
                width_percent: 60,
                h_align: HAlign::Right,
            };
            let (anchor, margins) = anchor_margins(&geom, OUTPUT);
            assert_eq!(anchor, vertical_of(edge) | Anchor::RIGHT);
            assert_eq!(margins, Margins::default());
        }
    }

    #[test]
    fn center_alignment_keeps_both_anchors_with_equal_margins() {
        // 1920 * 60% = 1152 px, leaving (1920 - 1152) / 2 = 384 per side.
        for edge in [Edge::Top, Edge::Bottom] {
            let geom = Geometry {
                edge,
                height_percent: 40,
                width_percent: 60,
                h_align: HAlign::Center,
            };
            let (anchor, margins) = anchor_margins(&geom, OUTPUT);
            assert_eq!(anchor, vertical_of(edge) | Anchor::LEFT | Anchor::RIGHT);
            assert_eq!(margins.left, 384);
            assert_eq!(margins.right, 384);
            assert_eq!(margins.top, 0);
            assert_eq!(margins.bottom, 0);
        }
    }

    #[test]
    fn center_margin_split_floors_left_and_keeps_total_width_exact() {
        // 1001 * 41% = 410.41 -> 410 px, leaving 591 px of slack: 295
        // left (floor), 296 right, so left + width + right == 1001.
        let geom = Geometry {
            edge: Edge::Top,
            height_percent: 40,
            width_percent: 41,
            h_align: HAlign::Center,
        };
        let (_, margins) = anchor_margins(&geom, (1001, 800));
        assert_eq!(margins.left, 295);
        assert_eq!(margins.right, 296);
        let width = surface_width(1001.0, 41);
        assert_eq!(margins.left as u32 + width + margins.right as u32, 1001);
    }

    #[test]
    fn full_width_requests_compositor_fill_height_stays_fixed() {
        // Width None at 100% means *fill*, not *unchanged*; height is
        // always a fixed pixel request.
        let geom = Geometry {
            edge: Edge::Top,
            height_percent: 40,
            width_percent: 100,
            h_align: HAlign::Center,
        };
        assert_eq!(requested_size(&geom, OUTPUT), (None, 432));
    }

    #[test]
    fn narrow_width_requests_fixed_pixels() {
        let geom = Geometry {
            edge: Edge::Top,
            height_percent: 40,
            width_percent: 60,
            h_align: HAlign::Left,
        };
        assert_eq!(requested_size(&geom, OUTPUT), (Some(1152), 432));
    }

    #[test]
    fn geometry_changed_is_false_for_identical_geometry() {
        let geom = Geometry::from_config(&DropTermConfig::default());
        assert!(!geometry_changed(&geom, &geom));
    }

    #[test]
    fn geometry_changed_fires_for_each_geometry_field() {
        let base = Geometry {
            edge: Edge::Top,
            height_percent: 40,
            width_percent: 60,
            h_align: HAlign::Left,
        };
        let mutations = [
            Geometry {
                edge: Edge::Bottom,
                ..base
            },
            Geometry {
                height_percent: 41,
                ..base
            },
            Geometry {
                width_percent: 61,
                ..base
            },
            Geometry {
                h_align: HAlign::Right,
                ..base
            },
        ];
        for mutated in mutations {
            assert!(
                geometry_changed(&base, &mutated),
                "expected a change for {mutated:?}"
            );
        }
    }

    #[test]
    fn geometry_changed_ignores_non_geometry_config_fields() {
        // Font, opacity, scrollback, … edits must not reshape the
        // surface (geometry spec, G2): only the four geometry fields
        // count.
        let old = Geometry::from_config(&DropTermConfig::default());
        let edited = DropTermConfig {
            font_name: "Fira Code".to_string(),
            font_size: 20,
            opacity: 50,
            scrollback_lines: 1_000,
            ..DropTermConfig::default()
        };
        let new = Geometry::from_config(&edited);
        assert!(!geometry_changed(&old, &new));
    }

    #[test]
    fn geometry_from_config_clamps_into_range() {
        let raw = DropTermConfig {
            height_percent: 200,
            width_percent: 5,
            ..DropTermConfig::default()
        };
        let geom = Geometry::from_config(&raw);
        assert_eq!(geom.height_percent, crate::config::MAX_HEIGHT_PERCENT);
        assert_eq!(geom.width_percent, crate::config::MIN_WIDTH_PERCENT);
        assert_eq!(geom.edge, Edge::Top);
        assert_eq!(geom.h_align, HAlign::Center);
    }

    #[test]
    fn live_drag_tracks_the_pointer_pixel_by_pixel_instead_of_percent_jumps() {
        // 1080p output anchored at 40 % (432 px): one pixel of pointer
        // travel must move the surface one pixel. The percent-quantized
        // path held 432 px for the first ~5 px of travel, then jumped
        // ~11 px at once — the steppiness the hardware pass flagged.
        // Percent quantization now happens once, at settle.
        let drag = DragAnchor {
            anchored_px: 432,
            applied_px: 432,
            press_y: 432.0,
        };
        assert_eq!(drag_target_px(Edge::Top, 1080, drag, 433.0), 433);
        assert_eq!(drag_target_px(Edge::Top, 1080, drag, 434.0), 434);
        assert_eq!(drag_target_px(Edge::Top, 1080, drag, 435.0), 435);
        assert_eq!(percent_from_px(1080, 435), 40);
    }

    #[test]
    fn the_arming_move_targets_the_height_the_drag_started_at() {
        // Arming must not jump the surface: the target at the pointer's
        // arming y is the height the surface already has, whichever
        // edge — and wherever inside the bar the grab happened. (The
        // old delta tracker seeded itself with bar-local y while moves
        // carry window y, making the first delta of every drag garbage.)
        let top = DragAnchor {
            anchored_px: 432,
            applied_px: 432,
            press_y: 432.0,
        };
        assert_eq!(drag_target_px(Edge::Top, 1080, top, 432.0), 432);
        let bottom = DragAnchor {
            anchored_px: 400,
            applied_px: 400,
            press_y: 5.0,
        };
        assert_eq!(drag_target_px(Edge::Bottom, 1000, bottom, 5.0), 400);
    }

    #[test]
    fn top_edge_targets_track_absolute_pointer_travel() {
        // The window frame of a top-anchored surface is fixed, so the
        // target is anchored height plus absolute travel since the
        // arming y: +100 px of travel is +100 px of height, −150 px is
        // −150 px.
        let drag = DragAnchor {
            anchored_px: 432,
            applied_px: 432,
            press_y: 432.0,
        };
        assert_eq!(drag_target_px(Edge::Top, 1080, drag, 532.0), 532);
        assert_eq!(drag_target_px(Edge::Top, 1080, drag, 282.0), 282);
    }

    #[test]
    fn bottom_edge_tracks_repeated_window_ys_through_frame_shifts() {
        // A bottom-anchored surface grows upward, so its window origin
        // moves up under the pointer with every applied resize: 5 px
        // drags report window ys [5, 0, 0, 0] once the frame follows
        // each apply. Delta accumulation stalled on the repeats — half
        // -speed tracking, the bar escaping the pointer. Retargeting
        // from the height actually applied tracks every pixel.
        let mut drag = DragAnchor {
            anchored_px: 400,
            applied_px: 400,
            press_y: 5.0,
        };
        let mut targets = Vec::new();
        for y in [5.0, 0.0, 0.0, 0.0] {
            let target = drag_target_px(Edge::Bottom, 1000, drag, y);
            drag.applied_px = target;
            targets.push(target);
        }
        assert_eq!(targets, vec![400, 405, 410, 415]);
    }

    #[test]
    fn drag_targets_clamp_to_the_band_in_pixels() {
        // However violent the drag, the pixel target stays inside the
        // 10–100 % band of the output.
        let drag = DragAnchor {
            anchored_px: 432,
            applied_px: 432,
            press_y: 0.0,
        };
        assert_eq!(drag_target_px(Edge::Top, 1080, drag, 100_000.0), 1080);
        assert_eq!(drag_target_px(Edge::Bottom, 1080, drag, 100_000.0), 108);
    }

    #[test]
    fn drag_targets_never_yield_a_zero_height_surface() {
        // On a tiny output the band floor is a couple of pixels; a
        // toward-edge drag must still target a positive height (the
        // `max(1px)` rule).
        let drag = DragAnchor {
            anchored_px: 2,
            applied_px: 2,
            press_y: 0.0,
        };
        let target = drag_target_px(Edge::Top, 20, drag, -1_000.0);
        assert_eq!(target, 2);
        assert!(target >= 1);
    }

    #[test]
    fn settled_percent_rounds_once_from_pixels() {
        // 405 px of 1000 is 40.5 % -> 41 (half away from zero); 404 px
        // is 40.4 % -> 40.
        assert_eq!(percent_from_px(1000, 405), 41);
        assert_eq!(percent_from_px(1000, 404), 40);
    }

    #[test]
    fn settled_percent_clamps_to_the_band() {
        assert_eq!(percent_from_px(1000, 5_000), MAX_HEIGHT_PERCENT);
        assert_eq!(percent_from_px(1000, 0), MIN_HEIGHT_PERCENT);
    }

    #[test]
    fn the_first_move_of_a_drag_applies_immediately() {
        // No apply yet in this drag: the arming move must land with no
        // interval wait, or every grab would feel dead.
        let now = Instant::now();
        assert!(drag_apply_due(None, now, 500));
    }

    #[test]
    fn unchanged_targets_skip_the_apply() {
        // A re-report of the position the surface already has (sub-pixel
        // move, or a synthetic echo) must not re-send the size triple.
        let at = Instant::now();
        let now = at + DRAG_APPLY_INTERVAL + Duration::from_millis(1);
        assert!(!drag_apply_due(Some((at, 500)), now, 500));
    }

    #[test]
    fn small_target_changes_wait_for_the_deadband() {
        // Sub-deadband changes are held back even when the interval has
        // fully elapsed: each apply costs the double-reported resize
        // pipeline, so sub-deadband drift skips redundant applies and
        // the surface simply sits at most a deadband behind the hand.
        let at = Instant::now();
        let now = at + DRAG_APPLY_INTERVAL + Duration::from_millis(1);
        assert!(!drag_apply_due(Some((at, 500)), now, 510));
    }

    #[test]
    fn deadband_sized_changes_still_wait_for_the_interval() {
        let at = Instant::now();
        let now = at + Duration::from_millis(10);
        assert!(!drag_apply_due(Some((at, 500)), now, 530));
    }

    #[test]
    fn deadband_sized_changes_apply_once_the_interval_elapses() {
        let at = Instant::now();
        let now = at + DRAG_APPLY_INTERVAL;
        assert!(drag_apply_due(Some((at, 500)), now, 530));
    }

    #[test]
    fn the_first_granted_size_adopts_immediately() {
        // Before any adoption there is nothing to debounce: the
        // terminal pin must exist for the first layout.
        let now = Instant::now();
        assert!(window_pin_due(None, now, 500.0));
    }

    #[test]
    fn re_reported_heights_adopt_for_free() {
        // The runtime double-reports every size change and the
        // compositor re-reports eased steps: re-offering the height
        // the pin already has must never wait out the interval.
        let at = Instant::now();
        let now = at + Duration::from_millis(1);
        assert!(window_pin_due(Some((at, 500.0)), now, 500.0));
    }

    #[test]
    fn changed_heights_within_the_interval_wait() {
        // The eased-convergence storm bound: intermediate granted
        // sizes adopt at most once per interval.
        let at = Instant::now();
        let now = at + Duration::from_millis(20);
        assert!(!window_pin_due(Some((at, 500.0)), now, 505.0));
    }

    #[test]
    fn changed_heights_adopt_once_the_interval_elapses() {
        let at = Instant::now();
        let now = at + WINDOW_PIN_INTERVAL;
        assert!(window_pin_due(Some((at, 500.0)), now, 505.0));
    }

    #[test]
    fn release_schedules_one_write_due_at_its_generation() {
        // Release -> window -> write: the settled percent is captured
        // when the drag is released — so a hide/show inside the window
        // cannot change what lands — and exactly the `WriteDue`
        // carrying the scheduled generation consumes it.
        let mut write_back = WriteBack::default();
        let pending = write_back.schedule(42);
        assert_eq!(pending.height_percent, 42);
        assert_eq!(write_back.due(pending.generation), Some(pending));
        assert_eq!(write_back.due(pending.generation), None);
    }

    #[test]
    fn newer_release_cancels_the_stale_write() {
        // A second drag settling inside the window reschedules: the
        // first generation's `WriteDue` goes stale and must not write
        // the first drag's percent (rapid re-drags coalesce into the
        // newest settle).
        let mut write_back = WriteBack::default();
        let stale = write_back.schedule(42);
        let current = write_back.schedule(50);
        assert_ne!(stale.generation, current.generation);
        assert_eq!(write_back.due(stale.generation), None);
        assert_eq!(
            write_back
                .due(current.generation)
                .map(|pending| pending.height_percent),
            Some(50)
        );
    }

    #[test]
    fn own_write_echo_matches_the_written_geometry() {
        let written = Geometry {
            edge: Edge::Top,
            height_percent: 42,
            width_percent: 80,
            h_align: HAlign::Center,
        };
        assert!(is_own_write(&written, &written));
    }

    #[test]
    fn external_edit_does_not_match_the_own_write_record() {
        // Any of the four geometry fields differing means the
        // `ConfigChanged` is someone else's write (a human editing the
        // file): absorb nothing and let G2 reshape live.
        let written = Geometry {
            edge: Edge::Top,
            height_percent: 42,
            width_percent: 80,
            h_align: HAlign::Center,
        };
        for edited in [
            Geometry {
                edge: Edge::Bottom,
                ..written
            },
            Geometry {
                height_percent: 43,
                ..written
            },
            Geometry {
                width_percent: 81,
                ..written
            },
            Geometry {
                h_align: HAlign::Left,
                ..written
            },
        ] {
            assert!(
                !is_own_write(&written, &edited),
                "{edited:?} must not match {written:?}"
            );
        }
    }
}
