// SPDX-License-Identifier: GPL-3.0-only

//! The on-disk config: the [`DropTermConfig`] entry cosmic-config
//! persists one key per file, its setting enums, and the clamping
//! that guards stale or hand-edited values.

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

/// Version of the on-disk config format.
pub const CONFIG_VERSION: u64 = 1;

/// Default surface height, in percent of the output height.
pub const DEFAULT_HEIGHT_PERCENT: u8 = 40;
/// Inclusive lower bound for `height_percent`.
pub const MIN_HEIGHT_PERCENT: u8 = 10;
/// Inclusive upper bound for `height_percent`.
pub const MAX_HEIGHT_PERCENT: u8 = 100;

/// Default surface width, in percent of the output width.
pub const DEFAULT_WIDTH_PERCENT: u8 = 100;
/// Inclusive lower bound for `width_percent`.
pub const MIN_WIDTH_PERCENT: u8 = 30;
/// Inclusive upper bound for `width_percent`.
pub const MAX_WIDTH_PERCENT: u8 = 100;

/// Name of the stock COSMIC dark theme (the dark-mode scheme default).
pub const COSMIC_THEME_DARK: &str = "COSMIC Dark";
/// Name of the stock COSMIC light theme (the light-mode scheme default).
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

/// Horizontal placement of the surface within the output.
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

/// Whether the app follows the system light/dark mode or pins one.
///
/// `System` — the default — lets the OS setting drive scheme
/// selection, the window background, and chrome. `Dark`/`Light` pin
/// the app to the user's own system palette for that mode,
/// independent of the OS setting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemePreference {
    #[default]
    System,
    Dark,
    Light,
}

/// Where the tab strip sits on the surface.
///
/// `Auto` — the default — keeps the strip at the leading edge beside
/// the grab bar, i.e. opposite the screen edge. `Top`/`Bottom` pin
/// the strip to that side of the surface regardless of anchor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabPosition {
    #[default]
    Auto,
    Top,
    Bottom,
}

impl std::fmt::Display for TabPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Top => write!(f, "top"),
            Self::Bottom => write!(f, "bottom"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
#[expect(
    clippy::struct_excessive_bools,
    reason = "config-file schema contract: each bool is its own serialized RON field; regrouping changes the on-disk format"
)]
pub struct DropTermConfig {
    /// Height of the surface as a percentage of the output height.
    pub height_percent: u8,
    /// Width of the surface as a percentage of the output width,
    /// 30-100. 100 (the default) fills the output.
    pub width_percent: u8,
    /// Horizontal placement within the output (moot at full width).
    pub h_align: HAlign,
    /// Screen edge the surface is anchored to.
    pub edge: Edge,
    /// Placement of the tab strip: `auto` stacks it at the leading
    /// edge beside the grab bar; `top`/`bottom` pin it to that side
    /// of the surface regardless of anchor. Defaults to `auto`.
    pub tab_position: TabPosition,
    /// Layer-shell layer the surface is created on.
    pub layer: Layer,
    /// Keyboard interactivity mode requested for the surface.
    pub keyboard_mode: KeyboardMode,
    /// Place the drop-down below panels and docks instead of covering them,
    /// by requesting exclusive zone `0` (see docs/research/panel-visibility.md).
    /// Applies on the next show. Default: cover.
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
    /// Defaults to `true` — a deliberate choice for a hotkey-driven
    /// drop-down: focus is the whole interaction model, so losing it
    /// retracts the overlay. Note this is the *minority* default among
    /// peers (Yakuake `KeepOpen=true`, Guake `window-losefocus=false`,
    /// Ghostty `quick-terminal-autohide=false` on Linux — see
    /// docs/research/terminal-de-integration.md); set `false` to keep
    /// the overlay open when it loses focus.
    pub hide_on_focus_loss: bool,
    /// Save the session on change and restore it on launch.
    /// Defaults to `true` — always-on persistence. `false` is
    /// symmetric: saves stop and launch restore is skipped, while the
    /// state file itself is left untouched to wait for a re-enable.
    pub session_persistence_enabled: bool,
    /// Animate show/hide with the 130 ms slide. Defaults to `true`.
    /// `false` restores instant show/hide with immediate destroy; a
    /// running animation is never aborted by the toggle.
    pub slide_animation_enabled: bool,
    /// Post a desktop notification when a bell rings while the
    /// terminal is hidden — one notification per
    /// tab per hide period, delivered over the freedesktop
    /// notification interface. Defaults to `true`; `false` silences
    /// notifications (the tab-bar 🔔 marker is unaffected).
    pub bell_notification_enabled: bool,
    /// Whether a new tab lands directly after the active tab (`true`,
    /// the default — `DropTerm`'s shipped behavior, so existing users
    /// keep it) or at the end of the tab list (Guake's default, its
    /// `new-tab-after` false). Applies to the keybind new-tab and the
    /// remote `new-tab`; `duplicate-tab` always lands after the active
    /// tab regardless, and session restore keeps saved order.
    pub new_tab_after_current: bool,
    /// Whether the app follows the system light/dark mode or pins
    /// one. Defaults to `System`: no override is ever issued and
    /// system flips keep driving the app.
    pub theme_preference: ThemePreference,
}

impl Default for DropTermConfig {
    fn default() -> Self {
        Self {
            height_percent: DEFAULT_HEIGHT_PERCENT,
            width_percent: DEFAULT_WIDTH_PERCENT,
            h_align: HAlign::default(),
            edge: Edge::default(),
            tab_position: TabPosition::default(),
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
            bell_notification_enabled: true,
            new_tab_after_current: true,
            theme_preference: ThemePreference::default(),
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
        // Both keys ship `true` — the default is the always-on
        // behavior, so existing configs keep it.
        let config = DropTermConfig::default();
        assert!(config.session_persistence_enabled);
        assert!(config.slide_animation_enabled);
    }

    #[test]
    fn bell_notifications_default_to_on() {
        // The one DE feature a hidden drop-down uniquely needs ships
        // enabled; the Behavior toggle turns it off.
        assert!(DropTermConfig::default().bell_notification_enabled);
    }

    #[test]
    fn new_tabs_after_current_default_on() {
        // The shipped behavior — insert after the active tab — is the
        // default, so existing users keep it; turning the toggle off
        // appends at the end instead (deliberately the inverse of
        // Guake `new-tab-after`, whose default is at-end).
        assert!(DropTermConfig::default().new_tab_after_current);
    }

    #[test]
    fn missing_new_tab_after_current_key_loads_as_true() {
        // A config directory from a version that did not yet write
        // `new_tab_after_current` loads as `true` through the same
        // default-entry leniency pinned for `respect_panel` — the
        // key list is additive, `CONFIG_VERSION` stays 1.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-missing-newtabafter-key-{}",
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
        // Simulate the pre-`new_tab_after_current` version by deleting
        // the one key it never wrote.
        std::fs::remove_file(
            root.join("cosmic")
                .join("dropterm-config-test")
                .join(format!("v{CONFIG_VERSION}"))
                .join("new_tab_after_current"),
        )
        .expect("remove new_tab_after_current key file");

        let loaded =
            DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
        assert!(loaded.new_tab_after_current);
        // The directory is live, not ignored: the written key comes back.
        assert_eq!(loaded.height_percent, 70);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn new_tab_after_current_key_on_disk_is_honored() {
        // Control for `missing_new_tab_after_current_key_loads_as_true`:
        // the same read path honors a key that does exist, so the
        // leniency result above cannot be a vacuous pass.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-present-newtabafter-key-{}",
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
            new_tab_after_current: false,
            ..DropTermConfig::default()
        }
        .write_entry(&config)
        .expect("write config keys");

        let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
        assert!(!loaded.new_tab_after_current);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn config_version_stays_at_one() {
        // New boolean keys merge into pre-existing files through the
        // CosmicConfigEntry default-entry fallback — no version bump,
        // no per-field serde attributes.
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

        // SAFETY: single-threaded test, no other test reads/writes SHELL.
        unsafe {
            std::env::remove_var("SHELL");
        }
        let no_env = DropTermConfig::default();
        assert_eq!(no_env.resolved_shell(), "/bin/sh");
    }

    #[test]
    fn missing_respect_panel_key_loads_as_false() {
        // A config directory from a version that did not yet write
        // `respect_panel`: every legacy key on disk, no `respect_panel`
        // file. cosmic-config reads
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

    #[test]
    fn theme_preference_defaults_to_system() {
        // The default is Follow system — a fresh config never issues
        // a theme override.
        assert_eq!(
            DropTermConfig::default().theme_preference,
            ThemePreference::System
        );
    }

    #[test]
    fn theme_preference_serde_round_trips_all_variants() {
        // The enum must survive the serde path cosmic-config uses for
        // key values; every variant, not just the default.
        for variant in [
            ThemePreference::System,
            ThemePreference::Dark,
            ThemePreference::Light,
        ] {
            let serialized = ron::to_string(&variant).expect("serialize ThemePreference");
            let deserialized: ThemePreference =
                ron::from_str(&serialized).expect("deserialize ThemePreference");
            assert_eq!(deserialized, variant);
        }
    }

    #[test]
    fn missing_theme_preference_key_loads_as_system() {
        // A config directory from a version that did not yet write
        // `theme_preference` loads as `System` through the same
        // default-entry leniency pinned for `respect_panel` — the
        // key list is additive, `CONFIG_VERSION` stays 1.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-missing-theme-key-{}",
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
        // Simulate the pre-`theme_preference` version by deleting the
        // one key it never wrote.
        std::fs::remove_file(
            root.join("cosmic")
                .join("dropterm-config-test")
                .join(format!("v{CONFIG_VERSION}"))
                .join("theme_preference"),
        )
        .expect("remove theme_preference key file");

        let loaded =
            DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
        assert_eq!(loaded.theme_preference, ThemePreference::System);
        // The directory is live, not ignored: the written key comes back.
        assert_eq!(loaded.height_percent, 70);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn theme_preference_key_on_disk_is_honored() {
        // Control for `missing_theme_preference_key_loads_as_system`:
        // the same read path honors a key that does exist, so the
        // leniency result above cannot be a vacuous pass.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-present-theme-key-{}",
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
            theme_preference: ThemePreference::Dark,
            ..DropTermConfig::default()
        }
        .write_entry(&config)
        .expect("write config keys");

        let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
        assert_eq!(loaded.theme_preference, ThemePreference::Dark);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tab_position_defaults_to_auto() {
        // `Auto` keeps the strip at the leading edge beside the grab
        // bar on both anchors.
        assert_eq!(DropTermConfig::default().tab_position, TabPosition::Auto);
    }

    #[test]
    fn tab_position_serde_round_trips_all_variants() {
        // The enum must survive the serde path cosmic-config uses for
        // key values; every variant, not just the default.
        for variant in [TabPosition::Auto, TabPosition::Top, TabPosition::Bottom] {
            let serialized = ron::to_string(&variant).expect("serialize TabPosition");
            let deserialized: TabPosition =
                ron::from_str(&serialized).expect("deserialize TabPosition");
            assert_eq!(deserialized, variant);
        }
    }

    #[test]
    fn missing_tab_position_key_loads_as_auto() {
        // A config directory from a version that did not yet write
        // `tab_position` loads as `Auto` through the same
        // default-entry leniency pinned for `respect_panel` — the
        // key list is additive, `CONFIG_VERSION` stays 1.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-missing-tabpos-key-{}",
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
        // Simulate the pre-`tab_position` version by deleting the one
        // key it never wrote.
        std::fs::remove_file(
            root.join("cosmic")
                .join("dropterm-config-test")
                .join(format!("v{CONFIG_VERSION}"))
                .join("tab_position"),
        )
        .expect("remove tab_position key file");

        let loaded =
            DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
        assert_eq!(loaded.tab_position, TabPosition::Auto);
        // The directory is live, not ignored: the written key comes back.
        assert_eq!(loaded.height_percent, 70);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tab_position_key_on_disk_is_honored() {
        // Control for `missing_tab_position_key_loads_as_auto`: the
        // same read path honors a key that does exist, so the leniency
        // result above cannot be a vacuous pass.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-present-tabpos-key-{}",
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
            tab_position: TabPosition::Top,
            ..DropTermConfig::default()
        }
        .write_entry(&config)
        .expect("write config keys");

        let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
        assert_eq!(loaded.tab_position, TabPosition::Top);

        let _ = std::fs::remove_dir_all(&root);
    }
}
