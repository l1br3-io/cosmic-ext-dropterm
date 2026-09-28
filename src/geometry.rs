// SPDX-License-Identifier: GPL-3.0-only

//! Pure geometry math plus the single layer-surface application path
//! (geometry spec).
//!
//! All percent/anchor/margin math lives here as plain functions over
//! plain data — no compositor, no iced runtime state — the same
//! separation as `tabs.rs`. Every transition that shapes the surface
//! (show, config reshape, drag) funnels through [`apply_geometry`], so
//! no path can leave a stale anchor, margin, or size behind.

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

/// The single path every geometry change goes through: one
/// `set_anchor` + `set_size` + `set_margin` triple, always sending all
/// three — zero margins included — so no transition can leave stale
/// anchors, margins, or sizes behind (e.g. width 60 -> 100 must
/// re-send zero margins, or the "full-width" surface stays shrunk).
pub fn apply_geometry<M: 'static>(id: window::Id, geom: &Geometry, output: (u32, u32)) -> Task<M> {
    let (anchor, margins) = anchor_margins(geom, output);
    let (width, height) = requested_size(geom, output);
    Task::batch([
        set_anchor(id, anchor),
        set_size(id, width, Some(height)),
        set_margin(id, margins.top, margins.right, margins.bottom, margins.left),
    ])
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

/// The height percent a grab-bar drag produces (geometry spec, G3):
/// `anchored_percent` grown by `total_dy` logical pixels of pointer
/// travel, clamped to the 10–100 % band.
///
/// `total_dy` is the *accumulated* travel since the press, in surface
/// space (y grows downward); the `Edge::Bottom` sign flip makes pulling
/// away from the screen edge always grow the terminal, whichever edge
/// the surface hangs from. Recomputing every move from the same anchor
/// — rather than growing the last applied percent — keeps slow drags
/// alive: one percent of a 1080 px output is ~11 px, so per-move
/// quantization would eat any gentler drag piece by piece.
///
/// The band clamp subsumes the `max(1px)` rule — the floor percent of
/// any output is at least one pixel — but the pixel target is still
/// floored at 1 px so a degenerate (zero-height) output can't produce
/// a zero or NaN request.
pub fn drag_height_percent(
    edge: Edge,
    output_height: u32,
    anchored_percent: u8,
    total_dy: f32,
) -> u8 {
    let grow = match edge {
        // Free edge at the bottom: dragging down grows.
        Edge::Top => total_dy,
        // Free edge at the top: dragging up grows.
        Edge::Bottom => -total_dy,
    };
    let anchored_px = surface_height(output_height as f32, anchored_percent) as f32;
    let target_px = (anchored_px + grow).max(1.0);
    let percent = target_px / output_height.max(1) as f32 * 100.0;
    percent
        .round()
        .clamp(f32::from(MIN_HEIGHT_PERCENT), f32::from(MAX_HEIGHT_PERCENT)) as u8
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
    fn drag_away_from_the_screen_edge_grows_whichever_edge() {
        // Anchored at 40 % of a 1000 px output (400 px): 100 px of
        // pointer travel away from the screen edge is +10 % — dragging
        // down on `Top`, up on `Bottom` (the sign flip).
        assert_eq!(drag_height_percent(Edge::Top, 1000, 40, 100.0), 50);
        assert_eq!(drag_height_percent(Edge::Bottom, 1000, 40, -100.0), 50);
    }

    #[test]
    fn drag_toward_the_screen_edge_shrinks_whichever_edge() {
        assert_eq!(drag_height_percent(Edge::Top, 1000, 40, -150.0), 25);
        assert_eq!(drag_height_percent(Edge::Bottom, 1000, 40, 150.0), 25);
    }

    #[test]
    fn drag_is_clamped_to_the_height_band() {
        // However violent the drag, the result stays inside 10-100 %.
        assert_eq!(
            drag_height_percent(Edge::Top, 1000, 95, 100_000.0),
            MAX_HEIGHT_PERCENT
        );
        assert_eq!(
            drag_height_percent(Edge::Bottom, 1000, 12, 100_000.0),
            MIN_HEIGHT_PERCENT
        );
    }

    #[test]
    fn drag_never_yields_a_zero_height_surface() {
        // On a tiny output the band floor is a couple of pixels; a
        // toward-edge drag must still request a positive height (the
        // `max(1px)` rule, criterion 4).
        let percent = drag_height_percent(Edge::Top, 20, 10, -1_000.0);
        assert_eq!(percent, MIN_HEIGHT_PERCENT);
        assert!(surface_height(20.0, percent) >= 1);
    }

    #[test]
    fn drag_rounds_to_the_nearest_percent() {
        // 400 px + 5 px of 1000 is 40.5 % -> 41 (half away from zero);
        // +4 px is 40.4 % -> 40.
        assert_eq!(drag_height_percent(Edge::Top, 1000, 40, 5.0), 41);
        assert_eq!(drag_height_percent(Edge::Top, 1000, 40, 4.0), 40);
    }

    #[test]
    fn drag_targets_recompute_from_the_anchor_so_quantization_cannot_compound() {
        // The caller passes *accumulated* travel from the press, so a
        // slow drag of many sub-percent moves still adds up: 4 px alone
        // rounds away, but the same 4 px carried into the next 100 px
        // of travel lands at 50.4 %, not the 50 % a per-move recompute
        // from the last applied percent would settle on.
        assert_eq!(drag_height_percent(Edge::Top, 1000, 40, 4.0), 40);
        assert_eq!(drag_height_percent(Edge::Top, 1000, 40, 104.0), 50);
        assert_eq!(drag_height_percent(Edge::Top, 1000, 40, 105.0), 51);
    }
}
