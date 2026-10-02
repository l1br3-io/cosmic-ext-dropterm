// SPDX-License-Identifier: GPL-3.0-only

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

pub const CONFIG_VERSION: u64 = 1;

pub const DEFAULT_HEIGHT_PERCENT: u8 = 40;
pub const MIN_HEIGHT_PERCENT: u8 = 10;
pub const MAX_HEIGHT_PERCENT: u8 = 100;

pub const DEFAULT_WIDTH_PERCENT: u8 = 100;
pub const MIN_WIDTH_PERCENT: u8 = 30;
pub const MAX_WIDTH_PERCENT: u8 = 100;

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

/// Horizontal placement of the surface within the output (geometry spec).
///
/// Moot while `width_percent` is 100 (the surface fills the output);
/// meaningful the moment the width is lowered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HAlign {
    Left,
    #[default]
    Center,
    Right,
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
    /// Width of the surface as a percentage of the output width,
    /// 30-100. 100 (the default) keeps the v1 full-width surface.
    pub width_percent: u8,
    /// Horizontal placement within the output (moot at full width).
    pub h_align: HAlign,
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
    /// Save the session on change and restore it on launch
    /// (settings-ui spec, U2). Defaults to `true` — the always-on
    /// behavior before the key existed. `false` is symmetric: saves
    /// stop and launch restore is skipped, while the state file
    /// itself is left untouched to wait for a re-enable.
    pub session_persistence_enabled: bool,
    /// Animate show/hide with the 130 ms slide (settings-ui spec,
    /// U2). Defaults to `true`. `false` restores the instant
    /// show/hide with immediate destroy the app had before slides
    /// existed; a running animation is never aborted by the toggle.
    pub slide_animation_enabled: bool,
}

impl Default for DropTermConfig {
    fn default() -> Self {
        Self {
            height_percent: DEFAULT_HEIGHT_PERCENT,
            width_percent: DEFAULT_WIDTH_PERCENT,
            h_align: HAlign::default(),
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
            session_persistence_enabled: true,
            slide_animation_enabled: true,
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
        self.width_percent = self
            .width_percent
            .clamp(MIN_WIDTH_PERCENT, MAX_WIDTH_PERCENT);
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
        assert_eq!(config.width_percent, DEFAULT_WIDTH_PERCENT);
        assert_eq!(config.h_align, HAlign::Center);
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
    fn session_and_slide_toggles_default_on() {
        // settings-ui U2: both keys ship `true` — day-one equality
        // with the always-on behavior these features had before the
        // keys existed.
        let config = DropTermConfig::default();
        assert!(config.session_persistence_enabled);
        assert!(config.slide_animation_enabled);
    }

    #[test]
    fn config_version_stays_at_one() {
        // settings-ui U2: the two booleans merge into pre-existing
        // files through the CosmicConfigEntry default-entry fallback
        // (the G1 precedent) — no version bump, no per-field serde
        // attributes.
        assert_eq!(CONFIG_VERSION, 1);
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
    fn width_percent_is_clamped_into_range() {
        let mut config = DropTermConfig {
            width_percent: 0,
            ..DropTermConfig::default()
        };
        config = config.clamped();
        assert_eq!(config.width_percent, MIN_WIDTH_PERCENT);

        let mut config = DropTermConfig {
            width_percent: 255,
            ..DropTermConfig::default()
        };
        config = config.clamped();
        assert_eq!(config.width_percent, MAX_WIDTH_PERCENT);

        let mut config = DropTermConfig {
            width_percent: 60,
            ..DropTermConfig::default()
        };
        config = config.clamped();
        assert_eq!(config.width_percent, 60);
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

    #[test]
    fn missing_respect_panel_key_loads_as_false() {
        // A config directory written before `respect_panel` existed: every
        // legacy key on disk, no `respect_panel` file. cosmic-config reads
        // each key independently and leaves missing keys at their default
        // (`Error::is_err` treats NotFound/NoConfigDirectory as lenient).
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-missing-key-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let config = cosmic_config::Config::with_custom_path(
            "dropterm-config-test",
            CONFIG_VERSION,
            root.clone(),
        )
        .expect("temp config context");
        DropTermConfig {
            height_percent: 70,
            ..DropTermConfig::default()
        }
        .write_entry(&config)
        .expect("write config keys");
        // Simulate the pre-`respect_panel` version by deleting the one key
        // it never wrote. On-disk layout is <root>/cosmic/<name>/v<version>
        // (cosmic-config rev 08a09e3, `Config::with_custom_path`).
        std::fs::remove_file(
            root.join("cosmic")
                .join("dropterm-config-test")
                .join(format!("v{CONFIG_VERSION}"))
                .join("respect_panel"),
        )
        .expect("remove respect_panel key file");

        let loaded =
            DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
        assert!(!loaded.respect_panel);
        // The directory is live, not ignored: the written key comes back.
        assert_eq!(loaded.height_percent, 70);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn respect_panel_key_on_disk_is_honored() {
        // Control for `missing_respect_panel_key_loads_as_false`: the same
        // read path honors a key that does exist, so the leniency result
        // above cannot be a vacuous pass.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-present-key-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let config = cosmic_config::Config::with_custom_path(
            "dropterm-config-test",
            CONFIG_VERSION,
            root.clone(),
        )
        .expect("temp config context");
        DropTermConfig {
            respect_panel: true,
            ..DropTermConfig::default()
        }
        .write_entry(&config)
        .expect("write config keys");

        let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
        assert!(loaded.respect_panel);

        let _ = std::fs::remove_dir_all(&root);
    }
}
