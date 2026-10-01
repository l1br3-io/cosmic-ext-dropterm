// SPDX-License-Identifier: GPL-3.0-only

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

pub const CONFIG_VERSION: u64 = 1;

pub const DEFAULT_HEIGHT_PERCENT: u8 = 40;
pub const MIN_HEIGHT_PERCENT: u8 = 10;
pub const MAX_HEIGHT_PERCENT: u8 = 100;

pub const COSMIC_THEME_DARK: &str = "COSMIC Dark";
pub const COSMIC_THEME_LIGHT: &str = "COSMIC Light";

/// Which screen edge the drop-down is anchored to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Edge {
    #[default]
    Top,
    Bottom,
}

impl std::fmt::Display for Edge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Top => write!(f, "top"),
            Self::Bottom => write!(f, "bottom"),
        }
    }
}

/// Which wlr-layer-shell layer the surface is created on.
///
/// `Overlay` sits above fullscreen windows; `Top` sits below them but tends
/// to behave better with input methods (see the README's known-risks section).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layer {
    #[default]
    Overlay,
    Top,
}

/// How the surface asks for keyboard focus.
///
/// `Exclusive` always grabs the keyboard while shown, even over a fullscreen
/// window. `OnDemand` is gentler but may not be focused automatically by
/// every compositor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyboardMode {
    #[default]
    Exclusive,
    OnDemand,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
pub struct DropTermConfig {
    /// Height of the surface as a percentage of the output height.
    pub height_percent: u8,
    /// Screen edge the surface is anchored to.
    pub edge: Edge,
    /// Layer-shell layer the surface is created on.
    pub layer: Layer,
    /// Keyboard interactivity mode requested for the surface.
    pub keyboard_mode: KeyboardMode,
    /// Place the drop-down below panels and docks instead of covering them,
    /// by requesting exclusive zone `0` (see docs/research/panel-visibility.md).
    /// Applies on the next show. Default: cover, Yakuake-style.
    pub respect_panel: bool,
    /// Name of the Wayland output to show on. `None` follows the
    /// compositor's notion of the "active" output (usually the one under
    /// the cursor).
    pub output: Option<String>,
    /// Shell binary to run. `None` falls back to `$SHELL`, then `/bin/sh`.
    pub shell: Option<String>,
    /// Extra arguments passed to the shell.
    pub args: Vec<String>,
    /// Font family name.
    pub font_name: String,
    /// Font size in points.
    pub font_size: u16,
    /// Color scheme name used in dark mode.
    pub color_scheme_dark: String,
    /// Color scheme name used in light mode.
    pub color_scheme_light: String,
    /// Background opacity, 0-100.
    pub opacity: u8,
    /// Number of scrollback lines kept per terminal.
    pub scrollback_lines: u32,
    /// Hide the terminal automatically when it loses keyboard focus.
    /// Defaults to `true` so a fresh run behaves like Yakuake/Guake
    /// (click away to hide).
    pub hide_on_focus_loss: bool,
}

impl Default for DropTermConfig {
    fn default() -> Self {
        Self {
            height_percent: DEFAULT_HEIGHT_PERCENT,
            edge: Edge::default(),
            layer: Layer::default(),
            keyboard_mode: KeyboardMode::default(),
            respect_panel: false,
            output: None,
            shell: None,
            args: Vec::new(),
            font_name: "Noto Sans Mono".to_string(),
            font_size: 14,
            color_scheme_dark: COSMIC_THEME_DARK.to_string(),
            color_scheme_light: COSMIC_THEME_LIGHT.to_string(),
            opacity: 100,
            scrollback_lines: 10_000,
            hide_on_focus_loss: true,
        }
    }
}

impl DropTermConfig {
    /// Clamp fields that come from a config file (which may hold stale or
    /// hand-edited values) into their valid ranges.
    pub fn clamped(mut self) -> Self {
        self.height_percent = self
            .height_percent
            .clamp(MIN_HEIGHT_PERCENT, MAX_HEIGHT_PERCENT);
        self.opacity = self.opacity.min(100);
        self
    }

    /// Resolve the shell to run: the configured shell, else `$SHELL`, else
    /// `/bin/sh`.
    pub fn resolved_shell(&self) -> String {
        self.shell
            .clone()
            .or_else(|| std::env::var("SHELL").ok())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let config = DropTermConfig::default();
        assert_eq!(config.height_percent, DEFAULT_HEIGHT_PERCENT);
        assert_eq!(config.edge, Edge::Top);
        assert_eq!(config.layer, Layer::Overlay);
        assert_eq!(config.keyboard_mode, KeyboardMode::Exclusive);
        assert!(config.output.is_none());
        assert!(config.shell.is_none());
        assert_eq!(config.opacity, 100);
        assert!(config.hide_on_focus_loss);
        assert!(!config.respect_panel);
    }

    #[test]
    fn height_percent_is_clamped_into_range() {
        let mut config = DropTermConfig {
            height_percent: 0,
            ..DropTermConfig::default()
        };
        config = config.clamped();
        assert_eq!(config.height_percent, MIN_HEIGHT_PERCENT);

        let mut config = DropTermConfig {
            height_percent: 255,
            ..DropTermConfig::default()
        };
        config = config.clamped();
        assert_eq!(config.height_percent, MAX_HEIGHT_PERCENT);

        let mut config = DropTermConfig {
            height_percent: 60,
            ..DropTermConfig::default()
        };
        config = config.clamped();
        assert_eq!(config.height_percent, 60);
    }

    #[test]
    fn opacity_is_clamped_to_100() {
        let config = DropTermConfig {
            opacity: 255,
            ..DropTermConfig::default()
        }
        .clamped();
        assert_eq!(config.opacity, 100);
    }

    #[test]
    fn shell_falls_back_to_env_then_bin_sh() {
        let configured = DropTermConfig {
            shell: Some("/usr/bin/fish".to_string()),
            ..DropTermConfig::default()
        };
        assert_eq!(configured.resolved_shell(), "/usr/bin/fish");

        // SAFETY: single-threaded test, no other test reads/writes SHELL.
        unsafe {
            std::env::set_var("SHELL", "/usr/bin/zsh");
        }
        let unconfigured = DropTermConfig::default();
        assert_eq!(unconfigured.resolved_shell(), "/usr/bin/zsh");

        unsafe {
            std::env::remove_var("SHELL");
        }
        let no_env = DropTermConfig::default();
        assert_eq!(no_env.resolved_shell(), "/bin/sh");
    }
}
