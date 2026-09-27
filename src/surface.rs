// SPDX-License-Identifier: GPL-3.0-only

//! Builds the layer-shell surface for the drop-down.
//!
//! Every `Show` goes through two steps, because there's no API to ask "how
//! tall is the active output" before creating a surface on it:
//!
//! 1. [`measuring`] creates a surface anchored to all four edges with no
//!    fixed size, so the compositor stretches it to the active output's
//!    full size and reports that size back through the first
//!    `Window::Opened` event delivered for it (see `app.rs`).
//! 2. `app.rs` then reshapes that same surface in place - anchored to one
//!    screen edge plus both sides, at a fixed pixel height - with the
//!    granular `set_anchor`/`set_size`/`set_keyboard_interactivity`/
//!    `set_exclusive_zone` commands, rather than destroying and recreating
//!    it.

use cosmic::iced::platform_specific::runtime::wayland::layer_surface::{
    IcedOutput, SctkLayerSurfaceSettings,
};
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    Anchor, KeyboardInteractivity, Layer as WlrLayer,
};
use cosmic::iced::runtime::core::layout::Limits;
use cosmic::iced::window;

use crate::config::{Edge, KeyboardMode, Layer};

pub const NAMESPACE: &str = "cosmic-ext-dropterm";

pub fn wlr_layer(layer: Layer) -> WlrLayer {
    match layer {
        Layer::Overlay => WlrLayer::Overlay,
        Layer::Top => WlrLayer::Top,
    }
}

pub fn keyboard_interactivity(mode: KeyboardMode) -> KeyboardInteractivity {
    match mode {
        KeyboardMode::Exclusive => KeyboardInteractivity::Exclusive,
        KeyboardMode::OnDemand => KeyboardInteractivity::OnDemand,
    }
}

pub fn edge_anchor(edge: Edge) -> Anchor {
    let edge = match edge {
        Edge::Top => Anchor::TOP,
        Edge::Bottom => Anchor::BOTTOM,
    };
    edge | Anchor::LEFT | Anchor::RIGHT
}

/// No `max_width`/`max_height` cap: the library default caps a layer
/// surface at 1920x1080, which would clip a full-width drop-down on a
/// larger or ultrawide output.
pub fn uncapped_size_limits() -> Limits {
    Limits::NONE.min_width(1.0).min_height(1.0)
}

/// Settings for the one-shot measuring surface (see the module doc comment).
///
/// Always targets [`IcedOutput::Active`]: dropterm v1 doesn't yet support
/// pinning to a named output (that's the v2 "monitor selection" feature).
pub fn measuring(id: window::Id) -> SctkLayerSurfaceSettings {
    SctkLayerSurfaceSettings {
        id,
        layer: WlrLayer::Overlay,
        keyboard_interactivity: KeyboardInteractivity::None,
        anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        output: IcedOutput::Active,
        namespace: format!("{NAMESPACE}-measure"),
        size: Some((None, None)),
        exclusive_zone: 0,
        size_limits: uncapped_size_limits(),
        ..SctkLayerSurfaceSettings::default()
    }
}
