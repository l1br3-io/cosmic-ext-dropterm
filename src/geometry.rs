// SPDX-License-Identifier: GPL-3.0-only

//! The layer-surface application path — the libcosmic half of the
//! geometry module (shared-core spec). The pure model and math (the
//! [`Geometry`] type, percent math, drag throttling, the write-back
//! machine) live in `dropterm_core::geometry`, re-exported whole
//! below, so every `crate::geometry::…` path keeps resolving; this
//! file adds only what turns a geometry into iced layer-surface
//! requests. Every transition that shapes the surface (show, config
//! reshape, drag) still goes through the one triple in
//! [`apply_drag_geometry`], so no path can leave a stale anchor,
//! margin, or size behind.

pub use dropterm_core::geometry::*;

use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    Anchor, set_anchor, set_margin, set_size,
};
use cosmic::iced::runtime::Task;
use cosmic::iced::window;

use crate::config::{Edge, HAlign, MAX_WIDTH_PERCENT};

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

/// [`apply_drag_geometry`], but with the height pinned to live drag pixels:
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

#[cfg(test)]
mod tests;
