// SPDX-License-Identifier: GPL-3.0-only

//! The on-disk config: the [`DropTermConfig`] entry cosmic-config
//! persists one key per file, its setting enums, and the clamping
//! that guards stale or hand-edited values.

use std::collections::BTreeMap;

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

use crate::keybinds::Binding;
use crate::profile::Profile;

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

/// How a tab's displayed title is generated from the shell's OSC 0/2
/// reports and the tab's cwd — Guake's `use-vte-titles` checkbox and
/// `display-tab-names` dropdown collapsed into one enum.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabTitleMode {
    /// The raw OSC 0/2 report, verbatim — today's behavior.
    #[default]
    Shell,
    /// The report with a trailing-cwd suffix abbreviated Guake-style:
    /// every parent path component cut to its first character, the
    /// last kept whole.
    AbbreviatedPath,
    /// The cwd's last path component, `"(root)"` for `/`.
    LastSegment,
    /// Reports never land; the title parks at `Terminal N`.
    Numbered,
    /// The name of the profile the tab runs — the id-resolved name at
    /// retitle time, so renames and deletes reflect immediately. A
    /// user rename still wins (as in every mode); a tab running no
    /// profile — or whose profile id dangles — falls back to exactly
    /// [`TabTitleMode::Shell`]'s behavior: the raw report, else
    /// `Terminal N`.
    Profile,
}

impl std::fmt::Display for TabTitleMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shell => write!(f, "shell"),
            Self::AbbreviatedPath => write!(f, "abbreviated_path"),
            Self::LastSegment => write!(f, "last_segment"),
            Self::Numbered => write!(f, "numbered"),
            Self::Profile => write!(f, "profile"),
        }
    }
}

/// Where a plain new tab's working directory comes from (the
/// `new_tab_cwd` Behavior dropdown): `Inherit` — the default — keeps
/// today's exact chain (explicit `--cwd` → the active tab's cwd → the
/// pty's default directory), while `Default` *replaces* the inherited
/// leg with the live-checked `default_cwd` (dead or unset → the pty
/// default). An explicit `--cwd` wins in both modes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NewTabCwd {
    #[default]
    Inherit,
    Default,
}

impl std::fmt::Display for NewTabCwd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inherit => write!(f, "inherit"),
            Self::Default => write!(f, "default"),
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
    /// How a tab's displayed title is generated (see
    /// [`TabTitleMode`]). Defaults to `Shell` — today's behavior; a
    /// user rename wins in every mode, and a mode change re-titles
    /// live tabs immediately.
    pub tab_title_mode: TabTitleMode,
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
    /// Whether closing a tab whose terminal runs a foreground process
    /// asks first through an in-surface modal (close-confirm spec).
    /// Defaults to `true` — the guardrail ships on for fresh and
    /// pre-existing configs alike (criterion 1); `false` restores
    /// instant, unconfirmed closes. Shell exits never confirm
    /// regardless of this key (`TermEvent::Exit` closes are never
    /// gated), and the decision reads this key at request time, so a
    /// mid-session flip applies to the very next close.
    pub close_confirmation_enabled: bool,
    /// Whether a left-click in the terminal's bounds dismisses an
    /// open settings drawer (drawer-click-close spec). **Parked**
    /// (2026-10-08): the behavior does not ship — the drawer's
    /// `context_drawer` overlay captures every mouse event over the
    /// surface while it is open, so a dismiss press can never be
    /// observed from the base tree (see that spec's fallback clause)
    /// — and the settings row is hidden. The key keeps loading
    /// (default `true`) and persisting for the day a capture
    /// mechanism exists; a hand-edited value is inert.
    pub settings_drawer_click_close: bool,
    /// Whether the app follows the system light/dark mode or pins
    /// one. Defaults to `System`: no override is ever issued and
    /// system flips keep driving the app.
    pub theme_preference: ThemePreference,
    /// Named spawn presets (the `profile` module). Defaults to
    /// empty — managed on the settings drawer's Profiles page.
    pub profiles: Vec<Profile>,
    /// Name of the profile plain new tabs launch. `None` (the
    /// default) keeps the configured shell; a name the `profiles`
    /// list no longer carries also resolves to the plain shell —
    /// loading never rewrites the config to drop it.
    pub default_profile: Option<String>,
    /// Where a plain new tab's working directory comes from (see
    /// [`NewTabCwd`]). Defaults to `Inherit` — today's chain
    /// byte-identical. Applies only to the no-`default_profile`
    /// plain spawns: a set, existing default profile still owns the
    /// cwd outright.
    pub new_tab_cwd: NewTabCwd,
    /// The directory [`NewTabCwd::Default`] opens plain new tabs in.
    /// `None` (the default) leaves the pty's default directory. The
    /// path is live-checked at spawn — a dead value warns once and
    /// degrades to the pty default, never an error surface, and the
    /// store is never rewritten to drop it.
    pub default_cwd: Option<std::path::PathBuf>,
    /// Custom key bindings, one per action, keyed by the stable action
    /// id (`keybinds::action_id` — the id set is this field's
    /// vocabulary, the field name its spelling; both are file-format
    /// contract, pinned by test). A binding replaces *all* of that
    /// action's default chords; removing the entry reverts to the
    /// defaults. Absent (the default) means all defaults. Entries
    /// with stale ids or unparsable keys skip at load with a warn —
    /// never fatal (`keybinds::effective_key_binds`).
    pub custom_key_binds: BTreeMap<String, Binding>,
    /// The settings drawer pane's width in px, as the user last
    /// dragged it. `None` (the default) renders the built-in
    /// default width; a stored value clamps at render into
    /// [344px, 50% of surface width] — the render-time clamp is the
    /// policy, so any stale or hand-edited value loads safe.
    /// Written once per drag release; double-clicking the drawer's
    /// drag handle resets it to `None`.
    pub settings_drawer_width: Option<u16>,
}

impl Default for DropTermConfig {
    fn default() -> Self {
        Self {
            height_percent: DEFAULT_HEIGHT_PERCENT,
            width_percent: DEFAULT_WIDTH_PERCENT,
            h_align: HAlign::default(),
            edge: Edge::default(),
            tab_position: TabPosition::default(),
            tab_title_mode: TabTitleMode::default(),
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
            close_confirmation_enabled: true,
            settings_drawer_click_close: true,
            theme_preference: ThemePreference::default(),
            profiles: Vec::new(),
            default_profile: None,
            new_tab_cwd: NewTabCwd::default(),
            default_cwd: None,
            custom_key_binds: BTreeMap::new(),
            settings_drawer_width: None,
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
        // Never dragged: the drawer renders its built-in default
        // width (drawer-resize spec criterion 1) until the first
        // drag release stores a px value.
        assert!(config.settings_drawer_width.is_none());
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
    fn close_confirmation_defaults_to_on() {
        // Close-confirm criterion 1: the guardrail ships enabled — a
        // fresh config never lets a close misfire kill a foreground
        // vim/ssh/pipeline unasked. The Behavior toggle turns it off.
        assert!(DropTermConfig::default().close_confirmation_enabled);
    }

    #[test]
    fn missing_close_confirmation_key_loads_as_true() {
        // A config directory from a version that did not yet write
        // `close_confirmation_enabled` loads as `true` through the
        // same default-entry leniency pinned for
        // `new_tab_after_current` — existing users get the guardrail
        // on (criterion 1); the key list is additive,
        // `CONFIG_VERSION` stays 1.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-missing-closeconfirm-key-{}",
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
        // Simulate the pre-`close_confirmation_enabled` version by
        // deleting the one key it never wrote.
        std::fs::remove_file(
            root.join("cosmic")
                .join("dropterm-config-test")
                .join(format!("v{CONFIG_VERSION}"))
                .join("close_confirmation_enabled"),
        )
        .expect("remove close_confirmation_enabled key file");

        let loaded =
            DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
        assert!(loaded.close_confirmation_enabled);
        // The directory is live, not ignored: the written key comes back.
        assert_eq!(loaded.height_percent, 70);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn close_confirmation_key_on_disk_is_honored() {
        // Control for `missing_close_confirmation_key_loads_as_true`:
        // the same read path honors a key that does exist (the toggle
        // off persists), so the leniency result above cannot be a
        // vacuous pass.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-present-closeconfirm-key-{}",
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
            close_confirmation_enabled: false,
            ..DropTermConfig::default()
        }
        .write_entry(&config)
        .expect("write config keys");

        let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
        assert!(!loaded.close_confirmation_enabled);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn drawer_click_close_defaults_to_on() {
        // Drawer-click-close criterion 5: the gesture ships enabled —
        // a fresh config dismisses the drawer on a terminal click
        // without any opt-in. The Behavior toggle turns it off.
        assert!(DropTermConfig::default().settings_drawer_click_close);
    }

    #[test]
    fn missing_settings_drawer_click_close_key_loads_as_true() {
        // A config directory from a version that did not yet write
        // `settings_drawer_click_close` loads as `true` through the
        // same default-entry leniency pinned for
        // `close_confirmation_enabled` — existing users get the
        // gesture on (criterion 5); the key list is additive,
        // `CONFIG_VERSION` stays 1.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-missing-drawerclick-key-{}",
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
        // Simulate the pre-`settings_drawer_click_close` version by
        // deleting the one key it never wrote.
        std::fs::remove_file(
            root.join("cosmic")
                .join("dropterm-config-test")
                .join(format!("v{CONFIG_VERSION}"))
                .join("settings_drawer_click_close"),
        )
        .expect("remove settings_drawer_click_close key file");

        let loaded =
            DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
        assert!(loaded.settings_drawer_click_close);
        // The directory is live, not ignored: the written key comes back.
        assert_eq!(loaded.height_percent, 70);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn settings_drawer_click_close_key_on_disk_is_honored() {
        // Control for
        // `missing_settings_drawer_click_close_key_loads_as_true`: the
        // same read path honors a key that does exist (the toggle off
        // persists), so the leniency result above cannot be a vacuous
        // pass.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-present-drawerclick-key-{}",
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
            settings_drawer_click_close: false,
            ..DropTermConfig::default()
        }
        .write_entry(&config)
        .expect("write config keys");

        let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
        assert!(!loaded.settings_drawer_click_close);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_profile_keys_load_as_empty_and_unset() {
        // A config directory from a version that did not yet write
        // `profiles` or `default_profile` loads as "no profiles, no
        // default" — byte-identical to today's behavior — through the
        // same default-entry leniency pinned for the other additive
        // keys; the key list grows, `CONFIG_VERSION` stays 1.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-missing-profile-keys-{}",
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
        // Simulate the pre-profile version by deleting the two keys
        // it never wrote.
        let version_dir = root
            .join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"));
        std::fs::remove_file(version_dir.join("profiles")).expect("remove profiles key file");
        std::fs::remove_file(version_dir.join("default_profile"))
            .expect("remove default_profile key file");

        let loaded =
            DropTermConfig::get_entry(&config).expect("profile-less v1 config loads without error");
        assert!(loaded.profiles.is_empty());
        assert_eq!(loaded.default_profile, None);
        // The directory is live, not ignored: the written key comes back.
        assert_eq!(loaded.height_percent, 70);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn profile_keys_round_trip_through_the_config_store() {
        // Two distinct profiles — one with a cwd, one without — plus
        // a set default survive a write→load cycle with equality.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-profile-roundtrip-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let config = cosmic_config::Config::with_custom_path(
            "dropterm-config-test",
            CONFIG_VERSION,
            root.clone(),
        )
        .expect("temp config context");
        let stored = DropTermConfig {
            profiles: vec![
                Profile {
                    name: "web-prod".to_string(),
                    command: "ssh web-prod".to_string(),
                    cwd: Some(std::path::PathBuf::from("/srv/web")),
                    color_scheme: None,
                    env: None,
                    font_size: None,
                    scrollback_lines: None,
                },
                Profile {
                    name: "top".to_string(),
                    command: "htop".to_string(),
                    cwd: None,
                    color_scheme: None,
                    env: None,
                    font_size: None,
                    scrollback_lines: None,
                },
            ],
            default_profile: Some("web-prod".to_string()),
            ..DropTermConfig::default()
        };
        stored.write_entry(&config).expect("write config keys");

        let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
        assert_eq!(loaded, stored);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn profile_color_scheme_round_trips_through_the_config_store() {
        // A set scheme and an unset one — the two shapes a mixed
        // profile list writes — survive a write→load cycle with
        // equality, riding the existing `profiles` key unchanged.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-profile-scheme-roundtrip-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let config = cosmic_config::Config::with_custom_path(
            "dropterm-config-test",
            CONFIG_VERSION,
            root.clone(),
        )
        .expect("temp config context");
        let stored = DropTermConfig {
            profiles: vec![
                Profile {
                    name: "web-prod".to_string(),
                    command: "ssh web-prod".to_string(),
                    cwd: None,
                    color_scheme: Some("Dracula".to_string()),
                    env: None,
                    font_size: None,
                    scrollback_lines: None,
                },
                Profile {
                    name: "top".to_string(),
                    command: "htop".to_string(),
                    cwd: None,
                    color_scheme: None,
                    env: None,
                    font_size: None,
                    scrollback_lines: None,
                },
            ],
            ..DropTermConfig::default()
        };
        stored.write_entry(&config).expect("write config keys");

        let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
        assert_eq!(loaded, stored);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn profile_env_round_trips_through_the_config_store() {
        // A set env map and an unset one — the two shapes a mixed
        // profile list writes — survive a write→load cycle with
        // equality, riding the existing `profiles` key unchanged.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-profile-env-roundtrip-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let config = cosmic_config::Config::with_custom_path(
            "dropterm-config-test",
            CONFIG_VERSION,
            root.clone(),
        )
        .expect("temp config context");
        let stored = DropTermConfig {
            profiles: vec![
                Profile {
                    name: "ssh-prod".to_string(),
                    command: "ssh web-prod".to_string(),
                    cwd: None,
                    color_scheme: None,
                    env: Some(BTreeMap::from([
                        ("DROPTERM_PROFILE".to_string(), "prod".to_string()),
                        ("TERM_PROGRAM".to_string(), "dropterm-ssh".to_string()),
                    ])),
                    font_size: None,
                    scrollback_lines: None,
                },
                Profile {
                    name: "top".to_string(),
                    command: "htop".to_string(),
                    cwd: None,
                    color_scheme: None,
                    env: None,
                    font_size: None,
                    scrollback_lines: None,
                },
            ],
            ..DropTermConfig::default()
        };
        stored.write_entry(&config).expect("write config keys");

        let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
        assert_eq!(loaded, stored);

        // The map serializes in BTree order — the deterministic-
        // round-trip property that made the field a BTreeMap (the
        // `custom_key_binds` precedent): the same profile always
        // writes the same bytes, so the store never churns.
        let version_dir = root
            .join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"));
        let text = std::fs::read_to_string(version_dir.join("profiles"))
            .expect("profiles key still on disk");
        let first = text.find("DROPTERM_PROFILE").expect("first key present");
        let second = text.find("TERM_PROGRAM").expect("second key present");
        assert!(first < second, "env keys serialize in BTree order: {text}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn new_tab_cwd_defaults_to_inherit_with_no_default_directory() {
        // Default-spawn ruling 2: `Inherit` is today's chain
        // byte-identical, and no default directory is configured — a
        // fresh config never moves a new tab's cwd.
        assert_eq!(DropTermConfig::default().new_tab_cwd, NewTabCwd::Inherit);
        assert_eq!(DropTermConfig::default().default_cwd, None);
    }

    #[test]
    fn new_tab_cwd_serde_round_trips_all_variants() {
        // The enum must survive the serde path cosmic-config uses for
        // key values; every variant, not just the default.
        for variant in [NewTabCwd::Inherit, NewTabCwd::Default] {
            let serialized = ron::to_string(&variant).expect("serialize NewTabCwd");
            let deserialized: NewTabCwd =
                ron::from_str(&serialized).expect("deserialize NewTabCwd");
            assert_eq!(deserialized, variant);
        }
    }

    #[test]
    fn missing_default_spawn_keys_load_as_inherit_and_unset() {
        // A config directory from a version that did not yet write
        // `new_tab_cwd` or `default_cwd` loads as "inherit, unset" —
        // byte-identical to today's plain-spawn chain — through the
        // same default-entry leniency pinned for the profile keys;
        // the key list is additive, `CONFIG_VERSION` stays 1.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-missing-default-spawn-keys-{}",
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
        // Simulate the pre-`default-spawn` version by deleting the
        // two keys it never wrote.
        let version_dir = root
            .join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"));
        std::fs::remove_file(version_dir.join("new_tab_cwd")).expect("remove new_tab_cwd key file");
        std::fs::remove_file(version_dir.join("default_cwd")).expect("remove default_cwd key file");

        let loaded = DropTermConfig::get_entry(&config)
            .expect("default-spawn-less v1 config loads without error");
        assert_eq!(loaded.new_tab_cwd, NewTabCwd::Inherit);
        assert_eq!(loaded.default_cwd, None);
        // The directory is live, not ignored: the written key comes back.
        assert_eq!(loaded.height_percent, 70);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn default_spawn_keys_round_trip_through_the_config_store() {
        // Default mode plus a set directory survive a write→load cycle
        // with equality — the `default_profile` precedent.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-default-spawn-roundtrip-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let config = cosmic_config::Config::with_custom_path(
            "dropterm-config-test",
            CONFIG_VERSION,
            root.clone(),
        )
        .expect("temp config context");
        let stored = DropTermConfig {
            new_tab_cwd: NewTabCwd::Default,
            default_cwd: Some(std::path::PathBuf::from("/srv/work")),
            ..DropTermConfig::default()
        };
        stored.write_entry(&config).expect("write config keys");

        let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
        assert_eq!(loaded, stored);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_pre_initiative_profiles_file_loads_with_the_scheme_unset() {
        // The version that shipped before `color_scheme` existed
        // wrote profile entries without the field. Seeding exactly
        // that shape and loading must succeed with the scheme unset
        // — and must leave the file byte-identical: the store is
        // never rewritten to "fix" an older format.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-pre-initiative-profiles-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let config = cosmic_config::Config::with_custom_path(
            "dropterm-config-test",
            CONFIG_VERSION,
            root.clone(),
        )
        .expect("temp config context");
        DropTermConfig::default()
            .write_entry(&config)
            .expect("write config keys");

        let version_dir = root
            .join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"));
        let profiles_key = version_dir.join("profiles");
        // The pre-initiative shape: a plain Vec<Profile> with no
        // `color_scheme` field anywhere (cwd spelled out so nothing
        // rides serde defaults but the new field itself).
        let pre_initiative = "[\n    (\n        name: \"ssh\",\n        command: \"ssh web\",\n        cwd: None,\n    ),\n]";
        std::fs::write(&profiles_key, pre_initiative)
            .expect("seed pre-initiative profiles key file");

        let loaded =
            DropTermConfig::get_entry(&config).expect("pre-initiative profiles file loads");
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profiles[0].name, "ssh");
        assert_eq!(loaded.profiles[0].color_scheme, None);
        // The same leniency covers every later additive field: `env`
        // loads unset when the key never existed.
        assert_eq!(loaded.profiles[0].env, None);

        // Byte-stable: the load rewrote nothing.
        assert_eq!(
            std::fs::read_to_string(&profiles_key).expect("profiles key still on disk"),
            pre_initiative
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn custom_key_binds_round_trip_through_the_config_store() {
        // Two entries — an ordinary rebind with modifiers and a
        // numbered activate row without — survive a write→load cycle
        // with equality. The key file's name is the field's
        // file-format spelling; the Binding shape is the rest of the
        // contract.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-keybinds-roundtrip-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let config = cosmic_config::Config::with_custom_path(
            "dropterm-config-test",
            CONFIG_VERSION,
            root.clone(),
        )
        .expect("temp config context");
        let stored = DropTermConfig {
            custom_key_binds: BTreeMap::from([
                (
                    "copy".to_string(),
                    crate::keybinds::Binding {
                        modifiers: vec![
                            crate::keybinds::ModifierName::Ctrl,
                            crate::keybinds::ModifierName::Shift,
                        ],
                        key: "C".to_string(),
                    },
                ),
                (
                    "activate_tab_3".to_string(),
                    crate::keybinds::Binding {
                        modifiers: Vec::new(),
                        key: "F2".to_string(),
                    },
                ),
            ]),
            ..DropTermConfig::default()
        };
        stored.write_entry(&config).expect("write config keys");
        let version_dir = root
            .join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"));
        assert!(
            version_dir.join("custom_key_binds").is_file(),
            "the key file's name is the field's on-disk spelling"
        );

        let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
        assert_eq!(loaded, stored);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_custom_key_binds_loads_as_the_empty_map() {
        // A config directory from a version that did not yet write
        // `custom_key_binds` loads as "all defaults" — the same
        // default-entry leniency as every additive key before it.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-missing-keybinds-{}",
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
        let version_dir = root
            .join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"));
        std::fs::remove_file(version_dir.join("custom_key_binds"))
            .expect("remove custom_key_binds key file");

        let loaded =
            DropTermConfig::get_entry(&config).expect("keybinds-less v1 config loads clean");
        assert!(loaded.custom_key_binds.is_empty());
        // The directory is live, not ignored: the written key comes back.
        assert_eq!(loaded.height_percent, 70);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn keybinds_write_persists_the_whole_map_through_the_store() {
        // The settings module's whole-map write lands as one `set` of
        // the `custom_key_binds` key — the ProfileWrite precedent —
        // and the next load reads exactly the written map back.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-keybinds-write-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let config = cosmic_config::Config::with_custom_path(
            "dropterm-config-test",
            CONFIG_VERSION,
            root.clone(),
        )
        .expect("temp config context");
        let write = crate::settings::KeybindsWrite(BTreeMap::from([(
            "copy".to_string(),
            crate::keybinds::Binding {
                modifiers: vec![crate::keybinds::ModifierName::Alt],
                key: "C".to_string(),
            },
        )]));
        write.persist(&config).expect("persist the whole map");

        let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
        assert_eq!(loaded.custom_key_binds, write.0);

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
    fn tab_title_mode_defaults_to_shell() {
        // `Shell` is today's behavior: OSC 0/2 reports land verbatim,
        // so a config without the key never changes a title.
        assert_eq!(
            DropTermConfig::default().tab_title_mode,
            TabTitleMode::Shell
        );
    }

    #[test]
    fn tab_title_mode_serde_round_trips_all_variants() {
        // The enum must survive the serde path cosmic-config uses for
        // key values; every variant, not just the default.
        for variant in [
            TabTitleMode::Shell,
            TabTitleMode::AbbreviatedPath,
            TabTitleMode::LastSegment,
            TabTitleMode::Numbered,
            TabTitleMode::Profile,
        ] {
            let serialized = ron::to_string(&variant).expect("serialize TabTitleMode");
            let deserialized: TabTitleMode =
                ron::from_str(&serialized).expect("deserialize TabTitleMode");
            assert_eq!(deserialized, variant);
        }
    }

    #[test]
    fn missing_tab_title_mode_key_loads_as_shell() {
        // A config directory from a version that did not yet write
        // `tab_title_mode` loads as `Shell` through the same
        // default-entry leniency pinned for `respect_panel` — the
        // key list is additive, `CONFIG_VERSION` stays 1.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-missing-titlemode-key-{}",
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
        // Simulate the pre-`tab_title_mode` version by deleting the
        // one key it never wrote.
        std::fs::remove_file(
            root.join("cosmic")
                .join("dropterm-config-test")
                .join(format!("v{CONFIG_VERSION}"))
                .join("tab_title_mode"),
        )
        .expect("remove tab_title_mode key file");

        let loaded =
            DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
        assert_eq!(loaded.tab_title_mode, TabTitleMode::Shell);
        // The directory is live, not ignored: the written key comes back.
        assert_eq!(loaded.height_percent, 70);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tab_title_mode_key_on_disk_is_honored() {
        // Control for `missing_tab_title_mode_key_loads_as_shell`: the
        // same read path honors a key that does exist, so the leniency
        // result above cannot be a vacuous pass.
        let root = std::env::temp_dir().join(format!(
            "dropterm-config-test-present-titlemode-key-{}",
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
            tab_title_mode: TabTitleMode::LastSegment,
            ..DropTermConfig::default()
        }
        .write_entry(&config)
        .expect("write config keys");

        let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
        assert_eq!(loaded.tab_title_mode, TabTitleMode::LastSegment);

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
