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

/// Fixed pixel height to request for the shown surface: `height_percent`
/// of the output, bar or no bar. The surface height never changes while
/// shown — when the tab bar appears, the terminal widget is pinned to
/// the window height minus the bar (see `terminal_view_height`), so the
/// bar takes its strip out of the terminal's rows, Yakuake-style, rather
/// than out of the screen.
pub fn surface_height(output_height: f32, height_percent: u8) -> u32 {
    (output_height * f32::from(height_percent) / 100.0)
        .round()
        .max(1.0) as u32
}

/// Height to pin the terminal widget to inside a granted surface of
/// `window_height` pixels: the whole window with a single tab, minus the
/// bar's strip once the bar is visible. Pinned as a `Fixed` length (see
/// `TerminalBox::height`) so the surrounding flex can't hand the terminal
/// the bar's space and push the bar off the surface.
pub fn terminal_view_height(window_height: f32, tab_count: usize) -> f32 {
    let bar = if tab_count > 1 {
        crate::tab_bar::BAR_HEIGHT
    } else {
        0.0
    };
    (window_height - bar).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_height_is_the_configured_share_of_the_output() {
        assert_eq!(surface_height(1440.0, 40), 576);
        assert_eq!(surface_height(1440.0, 100), 1440);
    }

    #[test]
    fn surface_height_never_rounds_below_one_pixel() {
        assert_eq!(surface_height(1440.0, 0), 1);
    }

    #[test]
    fn terminal_view_height_yields_the_bar_its_strip() {
        assert_eq!(terminal_view_height(563.0, 1), 563.0);
        assert_eq!(terminal_view_height(563.0, 2), 563.0 - 32.0);
        assert_eq!(terminal_view_height(595.0, 7), 563.0);
    }

    #[test]
    fn terminal_view_height_never_pinches_below_one_pixel() {
        assert_eq!(terminal_view_height(20.0, 2), 1.0);
    }
}
