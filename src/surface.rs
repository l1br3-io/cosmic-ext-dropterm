// SPDX-License-Identifier: GPL-3.0-only

//! Builds the layer-shell surface for the drop-down.
//!
//! There is no API to ask for the active output's size before creating
//! a surface on it, so every `Show` takes two steps: [`measuring`]
//! stretches a surface to the full output and reads its size from the
//! first `Window::Opened`; `app.rs` then reshapes that surface in place
//! per the config instead of recreating it.

use cosmic::iced::platform_specific::runtime::wayland::layer_surface::{
    IcedOutput, SctkLayerSurfaceSettings,
};
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    Anchor, KeyboardInteractivity, Layer as WlrLayer,
};
use cosmic::iced::runtime::core::layout::Limits;
use cosmic::iced::window;

use crate::config::{KeyboardMode, Layer};

/// Layer-shell namespace identifying this app's surfaces: the app id,
/// matching the desktop file's stem, so shell components that resolve
/// a layer surface by namespace find `DropTerm`'s entry instead of a
/// generic "unknown".
pub const NAMESPACE: &str = crate::app::APP_ID;

/// Maps the configured [`Layer`] to its wlr-layer-shell equivalent.
pub fn wlr_layer(layer: Layer) -> WlrLayer {
    match layer {
        Layer::Overlay => WlrLayer::Overlay,
        Layer::Top => WlrLayer::Top,
    }
}

/// Maps the configured [`KeyboardMode`] to its compositor equivalent.
pub fn keyboard_interactivity(mode: KeyboardMode) -> KeyboardInteractivity {
    match mode {
        KeyboardMode::Exclusive => KeyboardInteractivity::Exclusive,
        KeyboardMode::OnDemand => KeyboardInteractivity::OnDemand,
    }
}

/// Exclusive-zone request for the shown surface: `-1` ignores other
/// surfaces' reserved space (covers the panel); `0` asks the compositor
/// to place the surface inside the usable area, below panels and docks.
pub fn exclusive_zone(respect_panel: bool) -> i32 {
    if respect_panel { 0 } else { -1 }
}

/// No `max_width`/`max_height` cap: the library default caps a layer
/// surface at 1920x1080, which would clip a full-width drop-down on a
/// larger or ultrawide output.
pub fn uncapped_size_limits() -> Limits {
    Limits::NONE.min_width(1.0).min_height(1.0)
}

/// Settings for the one-shot measuring surface (see the module docs).
///
/// Always targets [`IcedOutput::Active`]: pinning to a named output is
/// not configurable. The configured [`KeyboardMode`] rides creation
/// rather than the post-open re-assert in `SurfaceOpened`: the
/// compositor hands a layer surface keyboard focus at map time only
/// when it maps asking for it (cosmic-comp's `map_layer` →
/// `Shell::set_focus`), and its later focus fixup is skipped outright
/// when no window holds focus and the workspace's focus stack is
/// empty — an empty desktop, exactly the session a drop-terminal
/// exists for. Mapping with `None` and committing `Exclusive` after
/// the first configure therefore never focused on an empty desktop;
/// mapping with the mode grabs focus on the first frame everywhere.
pub fn measuring(id: window::Id, mode: KeyboardMode) -> SctkLayerSurfaceSettings {
    SctkLayerSurfaceSettings {
        id,
        layer: WlrLayer::Overlay,
        keyboard_interactivity: keyboard_interactivity(mode),
        anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        output: IcedOutput::Active,
        // The app id, unadorned: the measuring surface is reshaped in
        // place into the shown terminal (never recreated), so any
        // "-measure" suffix would stick to the visible surface forever.
        namespace: NAMESPACE.to_string(),
        size: Some((None, None)),
        // DontCare (-1), not the default neutral zone (0): a neutral
        // surface arranges inside the space exclusive layers like the
        // COSMIC panel carve out, so the measured height came back
        // panel-short. The shown surface is DontCare too — match it.
        exclusive_zone: -1,
        size_limits: uncapped_size_limits(),
        ..SctkLayerSurfaceSettings::default()
    }
}

/// Height to pin the terminal widget to: the window minus the grab
/// bar and tab strips, both always shown (the tab strip carries the
/// settings button even with one tab), so the terminal's area never
/// changes across the 1↔2 tab crossing — minus the status band's own
/// strip while it is enabled (status-bar criterion 1). Pinned `Fixed`
/// (see `TerminalBox::height`) so flex cannot push the bars
/// off-surface.
pub fn terminal_view_height(window_height: f32, status_bar_enabled: bool) -> f32 {
    let band = if status_bar_enabled {
        crate::status_bar::BAND_HEIGHT
    } else {
        0.0
    };
    (window_height - crate::tab_bar::BAR_HEIGHT - crate::grab_bar::GRAB_BAR_HEIGHT - band).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_view_height_reserves_both_strips_while_the_band_is_hidden() {
        // Both strips are always shown — the tab strip carries the
        // settings button even with one tab — so the terminal's
        // height never jumps across the 1↔2 tab crossing.
        assert_eq!(
            terminal_view_height(563.0, false),
            563.0 - 32.0 - crate::grab_bar::GRAB_BAR_HEIGHT
        );
        assert_eq!(
            terminal_view_height(563.0 + 32.0 + crate::grab_bar::GRAB_BAR_HEIGHT, false),
            563.0
        );
    }

    #[test]
    fn terminal_view_height_reserves_the_band_while_it_is_enabled() {
        // The band takes its 28 px out of the terminal's pin exactly
        // like the two strips — subtracted order-agnostically, never
        // pushing a strip off-surface (status-bar criterion 1).
        assert_eq!(
            terminal_view_height(563.0, true),
            563.0 - 32.0 - crate::grab_bar::GRAB_BAR_HEIGHT - 28.0
        );
        assert_eq!(
            terminal_view_height(563.0 + 32.0 + crate::grab_bar::GRAB_BAR_HEIGHT + 28.0, true),
            563.0
        );
    }

    #[test]
    fn terminal_view_height_never_pinches_below_one_pixel() {
        assert_eq!(terminal_view_height(20.0, false), 1.0);
        assert_eq!(terminal_view_height(20.0, true), 1.0);
        assert_eq!(terminal_view_height(0.0, true), 1.0);
    }

    #[test]
    fn measuring_namespace_is_the_app_id() {
        // One surface, one identity: the measuring surface is reshaped in
        // place into the shown terminal, so its namespace is what shell
        // components resolve against desktop files — the app id, never a
        // placeholder, or the surface names as "unknown".
        assert_eq!(
            measuring(window::Id::unique(), KeyboardMode::default()).namespace,
            crate::app::APP_ID
        );
    }

    #[test]
    fn measuring_maps_asking_for_the_configured_keyboard_focus() {
        // The empty-desktop focus bug: a surface that maps with `None`
        // and only commits `Exclusive` after the first configure is
        // never focused when no other window holds focus — the
        // compositor's map-time grant only fires for a surface asking
        // at map, and its later focus fixup is skipped when the focus
        // stack is empty. The configured mode must therefore ride
        // creation, both modes pinning it (proven failing against the
        // old `None`).
        assert_eq!(
            measuring(window::Id::unique(), KeyboardMode::default()).keyboard_interactivity,
            KeyboardInteractivity::Exclusive
        );
        assert_eq!(
            measuring(window::Id::unique(), KeyboardMode::OnDemand).keyboard_interactivity,
            KeyboardInteractivity::OnDemand
        );
    }

    #[test]
    fn exclusive_zone_covers_reserved_space_by_default() {
        assert_eq!(exclusive_zone(false), -1);
    }

    #[test]
    fn exclusive_zone_respects_reserved_space_when_enabled() {
        assert_eq!(exclusive_zone(true), 0);
    }
}
