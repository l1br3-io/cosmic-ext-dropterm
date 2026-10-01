// SPDX-License-Identifier: GPL-3.0-only

//! The settings window's pure half (settings-ui spec): the shared
//! config-write contract — [`WriteKind`], the control→key map, and
//! the planned, range-clamped writes both writers persist through the
//! app's single write seam (U3). The window's view arrives with U4;
//! everything here is plain data and pure functions over it, so the
//! write contract is headless-testable (the established
//! `config_edit_reshapes`-style seam discipline).

use cosmic::cosmic_config::{self, ConfigSet as _};

use crate::config::{
    Edge, HAlign, KeyboardMode, Layer, MAX_HEIGHT_PERCENT, MAX_WIDTH_PERCENT, MIN_HEIGHT_PERCENT,
    MIN_WIDTH_PERCENT,
};

/// The write kind a config change came from, deciding echo handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteKind {
    /// Geometry was already applied before the write (drag release):
    /// absorb the echo, skip the reshape.
    PreApplied,
    /// Nothing applied yet (settings window): let the echo reshape.
    Bare,
}

/// A settings section: the criterion-2 grouping of the window's
/// cards. The sections' [`keys`](Section::keys) together are the
/// control→key map — every exposed key exactly once.
///
/// Not yet read outside its tests: the U4 view builds its cards from
/// these sections, so the map ships complete — and coverage-pinned —
/// one slice early.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Geometry,
    Behavior,
    Terminal,
    Advanced,
}

// The impl rides the same U4 bridge as the enum: the view reads
// `ALL`/`keys` to build its cards.
#[allow(dead_code)]
impl Section {
    /// Every section, in display order.
    pub const ALL: [Section; 5] = [
        Section::Appearance,
        Section::Geometry,
        Section::Behavior,
        Section::Terminal,
        Section::Advanced,
    ];

    /// The config keys this section's controls write, in display
    /// order. Across all sections every exposed key appears exactly
    /// once — pinned by test.
    pub const fn keys(&self) -> &'static [ConfigKey] {
        match self {
            Section::Appearance => &[ConfigKey::FontName, ConfigKey::FontSize, ConfigKey::Opacity],
            Section::Geometry => &[
                ConfigKey::HeightPercent,
                ConfigKey::WidthPercent,
                ConfigKey::HAlign,
                ConfigKey::Edge,
            ],
            Section::Behavior => &[
                ConfigKey::HideOnFocusLoss,
                ConfigKey::SessionPersistenceEnabled,
                ConfigKey::SlideAnimationEnabled,
            ],
            Section::Terminal => &[
                ConfigKey::Shell,
                ConfigKey::Args,
                ConfigKey::ScrollbackLines,
            ],
            Section::Advanced => &[
                ConfigKey::Layer,
                ConfigKey::KeyboardMode,
                ConfigKey::ColorSchemeDark,
                ConfigKey::ColorSchemeLight,
            ],
        }
    }
}

/// Every config key the settings window exposes (settings-ui spec,
/// criterion 2): all of `DropTermConfig` except `output`, which is
/// dead at runtime since the `monitor-selection` cut and unreachable
/// from the UI by a spec boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigKey {
    HeightPercent,
    WidthPercent,
    HAlign,
    Edge,
    FontName,
    FontSize,
    Opacity,
    HideOnFocusLoss,
    SessionPersistenceEnabled,
    SlideAnimationEnabled,
    Shell,
    Args,
    ScrollbackLines,
    Layer,
    KeyboardMode,
    ColorSchemeDark,
    ColorSchemeLight,
}

impl ConfigKey {
    /// The cosmic-config entry field name this key persists under —
    /// the exact spelling `DropTermConfig`'s field serializes as, and
    /// therefore the file-format contract a write must match.
    pub const fn key_name(&self) -> &'static str {
        match self {
            ConfigKey::HeightPercent => "height_percent",
            ConfigKey::WidthPercent => "width_percent",
            ConfigKey::HAlign => "h_align",
            ConfigKey::Edge => "edge",
            ConfigKey::FontName => "font_name",
            ConfigKey::FontSize => "font_size",
            ConfigKey::Opacity => "opacity",
            ConfigKey::HideOnFocusLoss => "hide_on_focus_loss",
            ConfigKey::SessionPersistenceEnabled => "session_persistence_enabled",
            ConfigKey::SlideAnimationEnabled => "slide_animation_enabled",
            ConfigKey::Shell => "shell",
            ConfigKey::Args => "args",
            ConfigKey::ScrollbackLines => "scrollback_lines",
            ConfigKey::Layer => "layer",
            ConfigKey::KeyboardMode => "keyboard_mode",
            ConfigKey::ColorSchemeDark => "color_scheme_dark",
            ConfigKey::ColorSchemeLight => "color_scheme_light",
        }
    }
}

/// A planned config write (settings-ui spec, U3): the key a control
/// commits and the value it persists — the pure half of the shared
/// write seam. One variant per [`ConfigKey`], each carrying its key's
/// typed value, so the key↔value pairing cannot drift. Ranged keys
/// clamp through [`PlannedWrite::clamped`], which the seam applies
/// before persisting, so no out-of-range value is ever written (spec
/// criterion 9).
///
/// Only the height variant is constructed until U4's controls commit
/// the rest; the full variant set ships now so the control→key map
/// and the clamps are pinned by test one slice early.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum PlannedWrite {
    HeightPercent(u8),
    WidthPercent(u8),
    HAlign(HAlign),
    Edge(Edge),
    FontName(String),
    FontSize(u16),
    Opacity(u8),
    HideOnFocusLoss(bool),
    SessionPersistenceEnabled(bool),
    SlideAnimationEnabled(bool),
    Shell(String),
    Args(Vec<String>),
    ScrollbackLines(u32),
    Layer(Layer),
    KeyboardMode(KeyboardMode),
    ColorSchemeDark(String),
    ColorSchemeLight(String),
}

impl PlannedWrite {
    /// The config key this write targets.
    pub const fn key(&self) -> ConfigKey {
        match self {
            PlannedWrite::HeightPercent(_) => ConfigKey::HeightPercent,
            PlannedWrite::WidthPercent(_) => ConfigKey::WidthPercent,
            PlannedWrite::HAlign(_) => ConfigKey::HAlign,
            PlannedWrite::Edge(_) => ConfigKey::Edge,
            PlannedWrite::FontName(_) => ConfigKey::FontName,
            PlannedWrite::FontSize(_) => ConfigKey::FontSize,
            PlannedWrite::Opacity(_) => ConfigKey::Opacity,
            PlannedWrite::HideOnFocusLoss(_) => ConfigKey::HideOnFocusLoss,
            PlannedWrite::SessionPersistenceEnabled(_) => ConfigKey::SessionPersistenceEnabled,
            PlannedWrite::SlideAnimationEnabled(_) => ConfigKey::SlideAnimationEnabled,
            PlannedWrite::Shell(_) => ConfigKey::Shell,
            PlannedWrite::Args(_) => ConfigKey::Args,
            PlannedWrite::ScrollbackLines(_) => ConfigKey::ScrollbackLines,
            PlannedWrite::Layer(_) => ConfigKey::Layer,
            PlannedWrite::KeyboardMode(_) => ConfigKey::KeyboardMode,
            PlannedWrite::ColorSchemeDark(_) => ConfigKey::ColorSchemeDark,
            PlannedWrite::ColorSchemeLight(_) => ConfigKey::ColorSchemeLight,
        }
    }

    /// Clamp a ranged write into its key's band, passing every other
    /// key's value through untouched (spec criterion 9): the write
    /// path's guarantee that a malformed or stale control value can
    /// never reach the config file.
    pub fn clamped(&self) -> Self {
        match self {
            PlannedWrite::HeightPercent(raw) => {
                PlannedWrite::HeightPercent(clamp_height_percent(*raw))
            }
            PlannedWrite::WidthPercent(raw) => {
                PlannedWrite::WidthPercent(clamp_width_percent(*raw))
            }
            PlannedWrite::Opacity(raw) => PlannedWrite::Opacity(clamp_opacity(*raw)),
            other => other.clone(),
        }
    }

    /// Persist through the config backend — the I/O edge of the write
    /// seam. The failure is the caller's to log; the typed values
    /// serialize exactly as `DropTermConfig`'s fields do.
    pub fn persist(&self, handler: &cosmic_config::Config) -> Result<(), cosmic_config::Error> {
        match self {
            PlannedWrite::HeightPercent(value) => {
                handler.set(ConfigKey::HeightPercent.key_name(), *value)
            }
            PlannedWrite::WidthPercent(value) => {
                handler.set(ConfigKey::WidthPercent.key_name(), *value)
            }
            PlannedWrite::HAlign(value) => handler.set(ConfigKey::HAlign.key_name(), value),
            PlannedWrite::Edge(value) => handler.set(ConfigKey::Edge.key_name(), value),
            PlannedWrite::FontName(value) => handler.set(ConfigKey::FontName.key_name(), value),
            PlannedWrite::FontSize(value) => handler.set(ConfigKey::FontSize.key_name(), *value),
            PlannedWrite::Opacity(value) => handler.set(ConfigKey::Opacity.key_name(), *value),
            PlannedWrite::HideOnFocusLoss(value) => {
                handler.set(ConfigKey::HideOnFocusLoss.key_name(), *value)
            }
            PlannedWrite::SessionPersistenceEnabled(value) => {
                handler.set(ConfigKey::SessionPersistenceEnabled.key_name(), *value)
            }
            PlannedWrite::SlideAnimationEnabled(value) => {
                handler.set(ConfigKey::SlideAnimationEnabled.key_name(), *value)
            }
            PlannedWrite::Shell(value) => handler.set(ConfigKey::Shell.key_name(), value),
            PlannedWrite::Args(value) => handler.set(ConfigKey::Args.key_name(), value),
            PlannedWrite::ScrollbackLines(value) => {
                handler.set(ConfigKey::ScrollbackLines.key_name(), *value)
            }
            PlannedWrite::Layer(value) => handler.set(ConfigKey::Layer.key_name(), value),
            PlannedWrite::KeyboardMode(value) => {
                handler.set(ConfigKey::KeyboardMode.key_name(), value)
            }
            PlannedWrite::ColorSchemeDark(value) => {
                handler.set(ConfigKey::ColorSchemeDark.key_name(), value)
            }
            PlannedWrite::ColorSchemeLight(value) => {
                handler.set(ConfigKey::ColorSchemeLight.key_name(), value)
            }
        }
    }
}

/// Clamp a height percent into the config band (settings-ui spec,
/// criterion 9 — height 10–100).
pub fn clamp_height_percent(raw: u8) -> u8 {
    raw.clamp(MIN_HEIGHT_PERCENT, MAX_HEIGHT_PERCENT)
}

/// Clamp a width percent into the config band (settings-ui spec,
/// criterion 9 — width 30–100).
pub fn clamp_width_percent(raw: u8) -> u8 {
    raw.clamp(MIN_WIDTH_PERCENT, MAX_WIDTH_PERCENT)
}

/// Clamp an opacity into its range (settings-ui spec, criterion 9 —
/// opacity 0–100), mirroring `DropTermConfig::clamped`'s upper-bound
/// only check: zero is a valid, fully transparent surface.
pub fn clamp_opacity(raw: u8) -> u8 {
    raw.min(100)
}

/// Whether a bare write of `key` must cancel a pending drag
/// write-back (settings-ui spec, U3 — the newest-intent race rule):
/// the drag path persists exactly the height percent (G3's
/// height-only scope), so the two writers race only on that key. A
/// bare height write supersedes the pending settle; a bare write of
/// any other key leaves a pending height write alone to land.
pub fn bare_write_cancels_pending_drag(key: ConfigKey, drag_write_pending: bool) -> bool {
    drag_write_pending && key == ConfigKey::HeightPercent
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section_keys() -> Vec<ConfigKey> {
        Section::ALL
            .iter()
            .flat_map(|section| section.keys().iter().copied())
            .collect()
    }

    #[test]
    fn the_control_map_covers_every_exposed_key_exactly_once_and_never_output() {
        // One planned-write variant per config key — the writer side of
        // the control→key map — each landing in exactly one section.
        let writers = [
            PlannedWrite::HeightPercent(40),
            PlannedWrite::WidthPercent(100),
            PlannedWrite::HAlign(HAlign::Center),
            PlannedWrite::Edge(Edge::Top),
            PlannedWrite::FontName(String::new()),
            PlannedWrite::FontSize(14),
            PlannedWrite::Opacity(100),
            PlannedWrite::HideOnFocusLoss(true),
            PlannedWrite::SessionPersistenceEnabled(true),
            PlannedWrite::SlideAnimationEnabled(true),
            PlannedWrite::Shell(String::new()),
            PlannedWrite::Args(Vec::new()),
            PlannedWrite::ScrollbackLines(10_000),
            PlannedWrite::Layer(Layer::Overlay),
            PlannedWrite::KeyboardMode(KeyboardMode::Exclusive),
            PlannedWrite::ColorSchemeDark(String::new()),
            PlannedWrite::ColorSchemeLight(String::new()),
        ];
        let keys = section_keys();
        assert_eq!(keys.len(), writers.len());
        for writer in &writers {
            let occurrences = keys.iter().filter(|key| **key == writer.key()).count();
            assert_eq!(
                occurrences,
                1,
                "{} missing from or duplicated in the section map",
                writer.key().key_name()
            );
        }
        // The spec boundary, pinned: `output` is not writable from the
        // UI.
        assert!(keys.iter().all(|key| key.key_name() != "output"));
    }

    #[test]
    fn key_names_match_the_config_file_fields() {
        // The file-format contract: each key's name is the spelling
        // its `DropTermConfig` field serializes as, so a
        // `handler.set(key_name(), …)` merges with a hand-edited file.
        let expected = [
            (ConfigKey::HeightPercent, "height_percent"),
            (ConfigKey::WidthPercent, "width_percent"),
            (ConfigKey::HAlign, "h_align"),
            (ConfigKey::Edge, "edge"),
            (ConfigKey::FontName, "font_name"),
            (ConfigKey::FontSize, "font_size"),
            (ConfigKey::Opacity, "opacity"),
            (ConfigKey::HideOnFocusLoss, "hide_on_focus_loss"),
            (
                ConfigKey::SessionPersistenceEnabled,
                "session_persistence_enabled",
            ),
            (ConfigKey::SlideAnimationEnabled, "slide_animation_enabled"),
            (ConfigKey::Shell, "shell"),
            (ConfigKey::Args, "args"),
            (ConfigKey::ScrollbackLines, "scrollback_lines"),
            (ConfigKey::Layer, "layer"),
            (ConfigKey::KeyboardMode, "keyboard_mode"),
            (ConfigKey::ColorSchemeDark, "color_scheme_dark"),
            (ConfigKey::ColorSchemeLight, "color_scheme_light"),
        ];
        for (key, name) in expected {
            assert_eq!(key.key_name(), name);
        }
    }

    #[test]
    fn height_writes_clamp_into_the_band() {
        assert_eq!(clamp_height_percent(0), MIN_HEIGHT_PERCENT);
        assert_eq!(clamp_height_percent(9), MIN_HEIGHT_PERCENT);
        assert_eq!(clamp_height_percent(10), MIN_HEIGHT_PERCENT);
        assert_eq!(clamp_height_percent(60), 60);
        assert_eq!(clamp_height_percent(100), MAX_HEIGHT_PERCENT);
        assert_eq!(clamp_height_percent(255), MAX_HEIGHT_PERCENT);
    }

    #[test]
    fn width_writes_clamp_into_the_band() {
        assert_eq!(clamp_width_percent(0), MIN_WIDTH_PERCENT);
        assert_eq!(clamp_width_percent(29), MIN_WIDTH_PERCENT);
        assert_eq!(clamp_width_percent(30), MIN_WIDTH_PERCENT);
        assert_eq!(clamp_width_percent(60), 60);
        assert_eq!(clamp_width_percent(100), MAX_WIDTH_PERCENT);
        assert_eq!(clamp_width_percent(255), MAX_WIDTH_PERCENT);
    }

    #[test]
    fn opacity_writes_clamp_to_at_most_100() {
        // Zero is valid (fully transparent), so only the upper bound
        // clamps — mirroring `DropTermConfig::clamped`.
        assert_eq!(clamp_opacity(0), 0);
        assert_eq!(clamp_opacity(50), 50);
        assert_eq!(clamp_opacity(100), 100);
        assert_eq!(clamp_opacity(101), 100);
        assert_eq!(clamp_opacity(255), 100);
    }

    #[test]
    fn planned_writes_clamp_the_ranged_keys_and_pass_others_through() {
        assert_eq!(
            PlannedWrite::HeightPercent(255).clamped(),
            PlannedWrite::HeightPercent(MAX_HEIGHT_PERCENT)
        );
        assert_eq!(
            PlannedWrite::WidthPercent(0).clamped(),
            PlannedWrite::WidthPercent(MIN_WIDTH_PERCENT)
        );
        assert_eq!(
            PlannedWrite::Opacity(255).clamped(),
            PlannedWrite::Opacity(100)
        );
        // Keys without a documented range pass through byte-identical.
        let unchanged = PlannedWrite::FontSize(999);
        assert_eq!(unchanged.clamped(), unchanged);
    }

    #[test]
    fn a_bare_height_write_cancels_a_pending_drag_write_back() {
        assert!(bare_write_cancels_pending_drag(
            ConfigKey::HeightPercent,
            true
        ));
    }

    #[test]
    fn a_bare_write_without_a_pending_drag_write_cancels_nothing() {
        assert!(!bare_write_cancels_pending_drag(
            ConfigKey::HeightPercent,
            false
        ));
    }

    #[test]
    fn a_bare_write_of_a_non_geometry_key_never_touches_pending_drag_state() {
        for key in [
            ConfigKey::FontName,
            ConfigKey::Opacity,
            ConfigKey::SlideAnimationEnabled,
        ] {
            assert!(
                !bare_write_cancels_pending_drag(key, true),
                "{} must not cancel a pending height write",
                key.key_name()
            );
        }
    }

    #[test]
    fn a_bare_width_write_leaves_a_pending_height_settle_alone() {
        // Width is a geometry key but not *the drag's* key: the race
        // rule cancels per key ("for the same key"), so moving the
        // width slider while a height settle is debouncing does not
        // drop it — the settle still lands.
        assert!(!bare_write_cancels_pending_drag(
            ConfigKey::WidthPercent,
            true
        ));
    }
}
