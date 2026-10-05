// SPDX-License-Identifier: GPL-3.0-only

//! The settings drawer: the shared config-write contract —
//! [`WriteKind`], the control→key map, the planned, range-clamped
//! writes both writers persist through the app's single write path
//! — plus the instant-apply drawer content (`widget::settings`
//! sections hosted by the framework context drawer over the
//! terminal) and its pure half: event planning, parsing, timing
//! copy, and the display-only draft buffer. Everything except the
//! view builders themselves is plain data and pure functions over
//! it, so the write contract and commit rules are
//! headless-testable.

use std::collections::HashMap;
use std::path::PathBuf;

use cosmic::Element;
use cosmic::theme;
use cosmic::widget;

use cosmic::cosmic_config::{self, ConfigSet as _};

use crate::config::{
    DropTermConfig, Edge, HAlign, KeyboardMode, Layer, MAX_HEIGHT_PERCENT, MAX_WIDTH_PERCENT,
    MIN_HEIGHT_PERCENT, MIN_WIDTH_PERCENT, TabPosition, TabTitleMode, ThemePreference,
};
use crate::profile::Profile;
use crate::tabs::command_line_parts;
use crate::terminal::theme_source::scheme_names;

/// The write kind a config change came from, deciding echo handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteKind {
    /// Geometry was already applied before the write (drag release):
    /// absorb the echo, skip the reshape.
    PreApplied,
    /// Nothing applied yet (settings drawer): let the echo reshape.
    Bare,
}

/// A settings section: the grouping of the window's cards. The
/// sections' [`keys`](Section::keys) together are the
/// control→key map — every exposed key exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Geometry,
    Behavior,
    Terminal,
    Advanced,
}

impl Section {
    /// Every section, in display order.
    // Test-facing since the tab split: runtime iterates
    // `tab_sections`, and ALL is the registry whose partition the
    // inline tests pin (every section exactly once, every exposed
    // key exactly once). The expectation self-expires the moment
    // runtime code consumes ALL again.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the tests' registry pin; runtime iterates tab_sections"
        )
    )]
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
    pub const fn keys(self) -> &'static [ConfigKey] {
        match self {
            Section::Appearance => &[
                ConfigKey::FontName,
                ConfigKey::FontSize,
                ConfigKey::Opacity,
                ConfigKey::ColorSchemeDark,
                ConfigKey::ColorSchemeLight,
                ConfigKey::ThemePreference,
            ],
            Section::Geometry => &[
                ConfigKey::HeightPercent,
                ConfigKey::WidthPercent,
                ConfigKey::HAlign,
                ConfigKey::Edge,
                ConfigKey::TabPosition,
            ],
            Section::Behavior => &[
                ConfigKey::HideOnFocusLoss,
                ConfigKey::SessionPersistenceEnabled,
                ConfigKey::SlideAnimationEnabled,
                ConfigKey::BellNotificationEnabled,
                ConfigKey::TabTitleMode,
                ConfigKey::NewTabAfterCurrent,
            ],
            Section::Terminal => &[
                ConfigKey::Shell,
                ConfigKey::Args,
                ConfigKey::ScrollbackLines,
            ],
            Section::Advanced => &[
                ConfigKey::RespectPanel,
                ConfigKey::Layer,
                ConfigKey::KeyboardMode,
            ],
        }
    }
}

/// Every config key the settings window exposes: all of
/// `DropTermConfig` except `output`, which is dead at runtime and
/// unreachable from the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigKey {
    HeightPercent,
    WidthPercent,
    HAlign,
    Edge,
    TabPosition,
    FontName,
    FontSize,
    Opacity,
    HideOnFocusLoss,
    SessionPersistenceEnabled,
    SlideAnimationEnabled,
    BellNotificationEnabled,
    TabTitleMode,
    NewTabAfterCurrent,
    Shell,
    Args,
    ScrollbackLines,
    Layer,
    KeyboardMode,
    RespectPanel,
    ColorSchemeDark,
    ColorSchemeLight,
    ThemePreference,
}

/// A settings drawer tab: a view-only regrouping of the same
/// sections into the everyday controls and the power controls —
/// never a second copy of the registry ([`Section::keys`], the
/// control→key map, is untouched by the grouping). No `Keybindings`
/// variant ships here: the shell defines only the tabs it serves;
/// the editor belongs to `keybinds-editor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Advanced,
}

/// The sections a tab's page hosts, in display order. Every
/// [`Section`] appears in exactly one tab and the union is
/// [`Section::ALL`] — pinned by test, so regrouping can never drop
/// or duplicate a card.
pub const fn tab_sections(tab: SettingsTab) -> &'static [Section] {
    match tab {
        SettingsTab::General => &[
            Section::Appearance,
            Section::Geometry,
            Section::Behavior,
            Section::Terminal,
        ],
        SettingsTab::Advanced => &[Section::Advanced],
    }
}

/// The tab's strip label — hardcoded English like every label in
/// this drawer.
pub const fn tab_header(tab: SettingsTab) -> &'static str {
    match tab {
        SettingsTab::General => "General",
        SettingsTab::Advanced => "Advanced",
    }
}

/// Whether a tab's page carries the Profiles entry row. The
/// drill-down's only door lives on General, so the Back affordance
/// always has one page to return to; Advanced never shows a second
/// door — pinned by test.
pub const fn tab_has_profiles_entry(tab: SettingsTab) -> bool {
    matches!(tab, SettingsTab::General)
}

impl SettingsTab {
    /// Every tab, in strip order — the strip's rows come from this
    /// list and nothing else, so no row can exist without a page to
    /// serve it (no dead `Keybindings` variant; that editor belongs
    /// to `keybinds-editor`).
    pub const ALL: [SettingsTab; 2] = [SettingsTab::General, SettingsTab::Advanced];
}

/// The tab strip's model: one row per tab of [`SettingsTab::ALL`],
/// labeled [`tab_header`] and carrying its `SettingsTab` as the row
/// data — the payload the activation handler resolves back to a tab
/// (criterion 7's entity↔tab lockstep). Built once at init and
/// stored on the app; the strip renders from the stored model, never
/// a per-frame rebuild.
/// The stored tab-strip model type — libcosmic's single-select
/// segmented-button model, aliased so the app field and the strip
/// builders below share one name.
pub type TabStripModel = widget::segmented_button::SingleSelectModel;

pub fn tab_strip_model() -> TabStripModel {
    let mut model = widget::segmented_button::SingleSelectModel::default();
    for tab in SettingsTab::ALL {
        model
            .insert()
            .text(tab_header(tab))
            .data::<SettingsTab>(tab);
    }
    model
}

/// Move the strip's selection to the row carrying `tab` — the
/// lockstep sync applied after every `settings_page` mutation (a
/// pick, the Profiles flip and its back, the explicit close's reset;
/// criterion 7). A tab with no row — impossible while the model is
/// built from [`SettingsTab::ALL`] — selects nothing; no row is
/// invented.
pub fn activate_tab_row(model: &mut TabStripModel, tab: SettingsTab) {
    // Collect first: `iter` holds the borrow the activation needs
    // to mutate.
    let entities: Vec<_> = model.iter().collect();
    for entity in entities {
        if model.data::<SettingsTab>(entity) == Some(&tab) {
            model.activate(entity);
        }
    }
}

impl ConfigKey {
    /// The cosmic-config entry field name this key persists under —
    /// the exact spelling `DropTermConfig`'s field serializes as, and
    /// therefore the file-format contract a write must match.
    pub const fn key_name(self) -> &'static str {
        match self {
            ConfigKey::HeightPercent => "height_percent",
            ConfigKey::WidthPercent => "width_percent",
            ConfigKey::HAlign => "h_align",
            ConfigKey::Edge => "edge",
            ConfigKey::TabPosition => "tab_position",
            ConfigKey::FontName => "font_name",
            ConfigKey::FontSize => "font_size",
            ConfigKey::Opacity => "opacity",
            ConfigKey::HideOnFocusLoss => "hide_on_focus_loss",
            ConfigKey::SessionPersistenceEnabled => "session_persistence_enabled",
            ConfigKey::SlideAnimationEnabled => "slide_animation_enabled",
            ConfigKey::BellNotificationEnabled => "bell_notification_enabled",
            ConfigKey::TabTitleMode => "tab_title_mode",
            ConfigKey::NewTabAfterCurrent => "new_tab_after_current",
            ConfigKey::Shell => "shell",
            ConfigKey::Args => "args",
            ConfigKey::ScrollbackLines => "scrollback_lines",
            ConfigKey::Layer => "layer",
            ConfigKey::KeyboardMode => "keyboard_mode",
            ConfigKey::RespectPanel => "respect_panel",
            ConfigKey::ColorSchemeDark => "color_scheme_dark",
            ConfigKey::ColorSchemeLight => "color_scheme_light",
            ConfigKey::ThemePreference => "theme_preference",
        }
    }
}

/// A planned config write: the key a control commits and the value
/// it persists — the pure half of the shared write path. One
/// variant per [`ConfigKey`], each carrying its key's typed value,
/// so the key↔value pairing cannot drift. Ranged keys clamp through
/// [`PlannedWrite::clamped`], which the write path applies before
/// persisting, so no out-of-range value is ever written.
#[derive(Debug, Clone, PartialEq)]
pub enum PlannedWrite {
    HeightPercent(u8),
    WidthPercent(u8),
    HAlign(HAlign),
    Edge(Edge),
    TabPosition(TabPosition),
    FontName(String),
    FontSize(u16),
    Opacity(u8),
    HideOnFocusLoss(bool),
    SessionPersistenceEnabled(bool),
    SlideAnimationEnabled(bool),
    BellNotificationEnabled(bool),
    TabTitleMode(TabTitleMode),
    NewTabAfterCurrent(bool),
    Shell(String),
    Args(Vec<String>),
    ScrollbackLines(u32),
    Layer(Layer),
    KeyboardMode(KeyboardMode),
    RespectPanel(bool),
    ColorSchemeDark(String),
    ColorSchemeLight(String),
    ThemePreference(ThemePreference),
}

impl PlannedWrite {
    /// The config key this write targets.
    pub const fn key(&self) -> ConfigKey {
        match self {
            PlannedWrite::HeightPercent(_) => ConfigKey::HeightPercent,
            PlannedWrite::WidthPercent(_) => ConfigKey::WidthPercent,
            PlannedWrite::HAlign(_) => ConfigKey::HAlign,
            PlannedWrite::Edge(_) => ConfigKey::Edge,
            PlannedWrite::TabPosition(_) => ConfigKey::TabPosition,
            PlannedWrite::FontName(_) => ConfigKey::FontName,
            PlannedWrite::FontSize(_) => ConfigKey::FontSize,
            PlannedWrite::Opacity(_) => ConfigKey::Opacity,
            PlannedWrite::HideOnFocusLoss(_) => ConfigKey::HideOnFocusLoss,
            PlannedWrite::SessionPersistenceEnabled(_) => ConfigKey::SessionPersistenceEnabled,
            PlannedWrite::SlideAnimationEnabled(_) => ConfigKey::SlideAnimationEnabled,
            PlannedWrite::BellNotificationEnabled(_) => ConfigKey::BellNotificationEnabled,
            PlannedWrite::TabTitleMode(_) => ConfigKey::TabTitleMode,
            PlannedWrite::NewTabAfterCurrent(_) => ConfigKey::NewTabAfterCurrent,
            PlannedWrite::Shell(_) => ConfigKey::Shell,
            PlannedWrite::Args(_) => ConfigKey::Args,
            PlannedWrite::ScrollbackLines(_) => ConfigKey::ScrollbackLines,
            PlannedWrite::Layer(_) => ConfigKey::Layer,
            PlannedWrite::KeyboardMode(_) => ConfigKey::KeyboardMode,
            PlannedWrite::RespectPanel(_) => ConfigKey::RespectPanel,
            PlannedWrite::ColorSchemeDark(_) => ConfigKey::ColorSchemeDark,
            PlannedWrite::ColorSchemeLight(_) => ConfigKey::ColorSchemeLight,
            PlannedWrite::ThemePreference(_) => ConfigKey::ThemePreference,
        }
    }

    /// Clamp a ranged write into its key's band, passing every other
    /// key's value through untouched: the write path's guarantee that
    /// a malformed or stale control value can never reach the config
    /// file.
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
    /// path. The failure is the caller's to log; the typed values
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
            PlannedWrite::TabPosition(value) => {
                handler.set(ConfigKey::TabPosition.key_name(), value)
            }
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
            PlannedWrite::BellNotificationEnabled(value) => {
                handler.set(ConfigKey::BellNotificationEnabled.key_name(), *value)
            }
            PlannedWrite::TabTitleMode(value) => {
                handler.set(ConfigKey::TabTitleMode.key_name(), value)
            }
            PlannedWrite::NewTabAfterCurrent(value) => {
                handler.set(ConfigKey::NewTabAfterCurrent.key_name(), *value)
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
            PlannedWrite::RespectPanel(value) => {
                handler.set(ConfigKey::RespectPanel.key_name(), *value)
            }
            PlannedWrite::ColorSchemeDark(value) => {
                handler.set(ConfigKey::ColorSchemeDark.key_name(), value)
            }
            PlannedWrite::ColorSchemeLight(value) => {
                handler.set(ConfigKey::ColorSchemeLight.key_name(), value)
            }
            PlannedWrite::ThemePreference(value) => {
                handler.set(ConfigKey::ThemePreference.key_name(), value)
            }
        }
    }
}

/// Clamp a height percent into the config band (10–100).
pub fn clamp_height_percent(raw: u8) -> u8 {
    raw.clamp(MIN_HEIGHT_PERCENT, MAX_HEIGHT_PERCENT)
}

/// Clamp a width percent into the config band (30–100).
pub fn clamp_width_percent(raw: u8) -> u8 {
    raw.clamp(MIN_WIDTH_PERCENT, MAX_WIDTH_PERCENT)
}

/// Clamp an opacity into its range (0–100), mirroring
/// `DropTermConfig::clamped`'s upper-bound only check: zero is a
/// valid, fully transparent surface.
pub fn clamp_opacity(raw: u8) -> u8 {
    raw.min(100)
}

/// Whether a bare write of `key` must cancel a pending drag
/// write-back (newest intent wins): the drag path persists exactly
/// the height percent, so the two writers race only on that key. A
/// bare height write supersedes the pending settle; a bare write of
/// any other key leaves a pending height write alone to land.
pub fn bare_write_cancels_pending_drag(key: ConfigKey, drag_write_pending: bool) -> bool {
    drag_write_pending && key == ConfigKey::HeightPercent
}

// --- the view's pure half: parsing, per-key commit planning, timing
// copy, and the in-window buffer ---

/// Which text input a settings draft belongs to. Drafts live in
/// [`SettingsBuffer`] display-only until the input is submitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextField {
    FontName,
    Shell,
    Args,
    ScrollbackLines,
}

/// Split an `args` draft into config tokens: whitespace-separated, no
/// quoting (the documented limitation — args containing spaces stay
/// config-file-only). Whitespace runs collapse; empty text is no
/// arguments.
pub fn parse_args(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(str::to_string).collect()
}

/// Parse a `scrollback_lines` draft (≥ 0): plain numbers land,
/// negatives clamp to zero, and anything without a number in it
/// rejects (`None` — no write, the draft stays for the user to fix).
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to [0, u32::MAX] first, so the narrowing to u32 is lossless by construction"
)]
pub fn parse_scrollback(raw: &str) -> Option<u32> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    match raw.parse::<i128>() {
        // i128 so an absurdly large entry clamps instead of failing
        // to parse — the same "clamp, don't reject" call negatives
        // get.
        Ok(n) => Some(n.clamp(0, i128::from(u32::MAX)) as u32),
        Err(_) => None,
    }
}

/// The write a submitted text input commits (text inputs commit on
/// submit, never per keystroke). `None` means the draft did not parse
/// and nothing is written.
pub fn submit_write(field: TextField, raw: &str) -> Option<PlannedWrite> {
    match field {
        TextField::FontName => Some(PlannedWrite::FontName(raw.trim().to_string())),
        // Empty shell text commits `""` — `resolved_shell` filters it
        // back to the `$SHELL` fallback, same effect as a hand-edited
        // empty value in the file.
        TextField::Shell => Some(PlannedWrite::Shell(raw.trim().to_string())),
        TextField::Args => Some(PlannedWrite::Args(parse_args(raw))),
        TextField::ScrollbackLines => parse_scrollback(raw).map(PlannedWrite::ScrollbackLines),
    }
}

/// When a field's edit takes effect — the copy the view states next
/// to every control so instant-apply never misleads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyTiming {
    /// Reshapes or re-renders a shown surface immediately.
    Live,
    /// Affects terminals spawned after the edit.
    NewTab,
    /// Applies when the surface is next created.
    NextShow,
    /// Honored at the feature's next trigger (save/restore, show/hide).
    NextTrigger,
}

impl ApplyTiming {
    /// The human-readable copy the view shows beside the control.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Live => "applies live",
            Self::NewTab => "new tabs only",
            Self::NextShow => "next show",
            Self::NextTrigger => "next trigger",
        }
    }
}

/// The documented apply timing of an exposed key.
pub const fn apply_timing(key: ConfigKey) -> ApplyTiming {
    match key {
        ConfigKey::HeightPercent
        | ConfigKey::WidthPercent
        | ConfigKey::HAlign
        | ConfigKey::Edge
        | ConfigKey::TabPosition
        | ConfigKey::TabTitleMode
        | ConfigKey::FontName
        | ConfigKey::FontSize
        | ConfigKey::Opacity
        | ConfigKey::ColorSchemeDark
        | ConfigKey::ColorSchemeLight
        | ConfigKey::ThemePreference => ApplyTiming::Live,
        ConfigKey::Layer | ConfigKey::KeyboardMode | ConfigKey::RespectPanel => {
            ApplyTiming::NextShow
        }
        ConfigKey::Shell | ConfigKey::Args | ConfigKey::ScrollbackLines => ApplyTiming::NewTab,
        ConfigKey::HideOnFocusLoss
        | ConfigKey::SessionPersistenceEnabled
        | ConfigKey::SlideAnimationEnabled
        | ConfigKey::BellNotificationEnabled
        | ConfigKey::NewTabAfterCurrent => ApplyTiming::NextTrigger,
    }
}

const H_ALIGN_OPTIONS: &[(HAlign, &str)] = &[
    (HAlign::Left, "Left"),
    (HAlign::Center, "Center"),
    (HAlign::Right, "Right"),
];
const EDGE_OPTIONS: &[(Edge, &str)] = &[(Edge::Top, "Top"), (Edge::Bottom, "Bottom")];
const TAB_POSITION_OPTIONS: &[(TabPosition, &str)] = &[
    (TabPosition::Auto, "Follow edge"),
    (TabPosition::Top, "Top"),
    (TabPosition::Bottom, "Bottom"),
];
const TAB_TITLE_MODE_OPTIONS: &[(TabTitleMode, &str)] = &[
    (TabTitleMode::Shell, "Shell title"),
    (TabTitleMode::AbbreviatedPath, "Abbreviated path"),
    (TabTitleMode::LastSegment, "Last segment"),
    (TabTitleMode::Numbered, "Terminal number"),
];
const LAYER_OPTIONS: &[(Layer, &str)] = &[(Layer::Overlay, "Overlay"), (Layer::Top, "Top")];
const KEYBOARD_MODE_OPTIONS: &[(KeyboardMode, &str)] = &[
    (KeyboardMode::Exclusive, "Exclusive"),
    (KeyboardMode::OnDemand, "OnDemand"),
];
const THEME_PREFERENCE_OPTIONS: &[(ThemePreference, &str)] = &[
    (ThemePreference::System, "Follow system"),
    (ThemePreference::Dark, "Dark"),
    (ThemePreference::Light, "Light"),
];

/// A scheme dropdown's selection: the config name's position in the
/// registry options. An unknown name selects nothing — the control
/// never invents a value; `active_colors()` keeps serving the COSMIC
/// fallback meanwhile.
fn scheme_index(name: &str) -> Option<usize> {
    scheme_names().iter().position(|option| *option == name)
}

/// The currently-selected dropdown index for `key` under `config`, so
/// the control opens on the file's value. `None` for keys without a
/// dropdown.
pub fn dropdown_selected(key: ConfigKey, config: &DropTermConfig) -> Option<usize> {
    match key {
        ConfigKey::HAlign => H_ALIGN_OPTIONS
            .iter()
            .position(|(value, _)| *value == config.h_align),
        ConfigKey::Edge => EDGE_OPTIONS
            .iter()
            .position(|(value, _)| *value == config.edge),
        ConfigKey::TabPosition => TAB_POSITION_OPTIONS
            .iter()
            .position(|(value, _)| *value == config.tab_position),
        ConfigKey::TabTitleMode => TAB_TITLE_MODE_OPTIONS
            .iter()
            .position(|(value, _)| *value == config.tab_title_mode),
        ConfigKey::Layer => LAYER_OPTIONS
            .iter()
            .position(|(value, _)| *value == config.layer),
        ConfigKey::KeyboardMode => KEYBOARD_MODE_OPTIONS
            .iter()
            .position(|(value, _)| *value == config.keyboard_mode),
        ConfigKey::ColorSchemeDark => scheme_index(&config.color_scheme_dark),
        ConfigKey::ColorSchemeLight => scheme_index(&config.color_scheme_light),
        ConfigKey::ThemePreference => THEME_PREFERENCE_OPTIONS
            .iter()
            .position(|(value, _)| *value == config.theme_preference),
        _ => None,
    }
}

/// The write a dropdown pick commits (dropdowns commit on change).
/// `None` when `key` has no dropdown or `index` is out of its range —
/// a pick can never invent a value.
pub fn dropdown_write(key: ConfigKey, index: usize) -> Option<PlannedWrite> {
    match key {
        ConfigKey::HAlign => H_ALIGN_OPTIONS
            .get(index)
            .map(|(value, _)| PlannedWrite::HAlign(*value)),
        ConfigKey::Edge => EDGE_OPTIONS
            .get(index)
            .map(|(value, _)| PlannedWrite::Edge(*value)),
        ConfigKey::TabPosition => TAB_POSITION_OPTIONS
            .get(index)
            .map(|(value, _)| PlannedWrite::TabPosition(*value)),
        ConfigKey::TabTitleMode => TAB_TITLE_MODE_OPTIONS
            .get(index)
            .map(|(value, _)| PlannedWrite::TabTitleMode(*value)),
        ConfigKey::Layer => LAYER_OPTIONS
            .get(index)
            .map(|(value, _)| PlannedWrite::Layer(*value)),
        ConfigKey::KeyboardMode => KEYBOARD_MODE_OPTIONS
            .get(index)
            .map(|(value, _)| PlannedWrite::KeyboardMode(*value)),
        ConfigKey::ColorSchemeDark => scheme_names()
            .get(index)
            .map(|name| PlannedWrite::ColorSchemeDark((*name).to_string())),
        ConfigKey::ColorSchemeLight => scheme_names()
            .get(index)
            .map(|name| PlannedWrite::ColorSchemeLight((*name).to_string())),
        ConfigKey::ThemePreference => THEME_PREFERENCE_OPTIONS
            .get(index)
            .map(|(value, _)| PlannedWrite::ThemePreference(*value)),
        _ => None,
    }
}

/// The write a slider release commits (sliders commit on release;
/// `on_change` is display-only). Ranged keys clamp — float-to-int
/// `as` casts saturate in Rust, so a negative buffered value clamps
/// up to the band's floor, not wrap. `None` for keys without a
/// slider.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "rounded slider values clamped into each key's band before the narrowing cast"
)]
pub fn slider_release_write(key: ConfigKey, value: f32) -> Option<PlannedWrite> {
    let value = value.round();
    match key {
        ConfigKey::HeightPercent => Some(PlannedWrite::HeightPercent(clamp_height_percent(
            value as u8,
        ))),
        ConfigKey::WidthPercent => {
            Some(PlannedWrite::WidthPercent(clamp_width_percent(value as u8)))
        }
        ConfigKey::Opacity => Some(PlannedWrite::Opacity(clamp_opacity(value as u8))),
        ConfigKey::FontSize => Some(PlannedWrite::FontSize(value.max(0.0) as u16)),
        _ => None,
    }
}

/// A drawer lifecycle transition (spec criterion 8): which of the two
/// paths that ends a drawer-over-terminal moment fired. The rule they
/// encode — [`SettingsBuffer::apply_drawer_transition`] — is the
/// close-clears vs hide-preserves decision, headless-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawerTransition {
    /// An explicit close: Escape, the toggle's repeat press, or the
    /// drawer's Close button. The next open starts fresh from
    /// config.
    ExplicitClose,
    /// The surface hid — focus loss per `hide_on_focus_loss`, or an
    /// explicit hide — with the drawer open. The drawer and its
    /// in-flight drafts survive untouched for the next show.
    FocusLossHide,
}

/// The settings window's in-flight state: text drafts buffered only
/// until submit, slider positions buffered for the live value
/// readout until the next change. Both are display-only — the view
/// renders from `DropTermConfig` with a draft winning over the
/// file's value while one exists — and [`SettingsBuffer::clear`] on
/// window close is what makes reopening fresh.
#[derive(Debug, Default)]
pub struct SettingsBuffer {
    texts: HashMap<TextField, String>,
    sliders: HashMap<ConfigKey, f32>,
}

impl SettingsBuffer {
    /// The text a field's input shows: its draft if one is buffered,
    /// else the config file's value (empty for an unset shell — the
    /// placeholder carries `resolved_shell()`).
    pub fn text(&self, field: TextField, config: &DropTermConfig) -> String {
        if let Some(draft) = self.texts.get(&field) {
            return draft.clone();
        }
        match field {
            TextField::FontName => config.font_name.clone(),
            TextField::Shell => config.shell.clone().unwrap_or_default(),
            TextField::Args => config.args.join(" "),
            TextField::ScrollbackLines => config.scrollback_lines.to_string(),
        }
    }

    /// Buffer a field's draft (display-only until submit).
    pub fn set_text(&mut self, field: TextField, draft: String) {
        self.texts.insert(field, draft);
    }

    /// Drop a field's draft — after a submit, so the input falls back
    /// to the (echo-updated) config value.
    pub fn clear_text(&mut self, field: TextField) {
        self.texts.remove(&field);
    }

    /// Buffer a slider's position (display-only until release).
    pub fn set_slider(&mut self, key: ConfigKey, value: f32) {
        self.sliders.insert(key, value);
    }

    /// The value a slider shows: its buffered position if it has
    /// moved, else the config file's value.
    pub fn slider_value(&self, key: ConfigKey, config: &DropTermConfig) -> f32 {
        self.sliders.get(&key).copied().unwrap_or(match key {
            ConfigKey::HeightPercent => f32::from(config.height_percent),
            ConfigKey::WidthPercent => f32::from(config.width_percent),
            ConfigKey::Opacity => f32::from(config.opacity),
            ConfigKey::FontSize => f32::from(config.font_size),
            _ => 0.0,
        })
    }

    /// Drop everything (window close): the next window renders purely
    /// from config — reopen freshness by construction.
    pub fn clear(&mut self) {
        self.texts.clear();
        self.sliders.clear();
    }

    /// Apply a drawer lifecycle transition (spec criterion 8): an
    /// explicit close clears — the reopen-fresh rule the settings
    /// window carried — while a focus-loss hide preserves everything
    /// for the next show. Hides never call this with `FocusLossHide`
    /// and touch nothing (preservation is the default); the method
    /// exists so the rule has one named, headless-tested home.
    pub fn apply_drawer_transition(&mut self, transition: DrawerTransition) {
        if matches!(transition, DrawerTransition::ExplicitClose) {
            self.clear();
        }
    }
}

// --- the Profiles page's pure half: the write-time contract over the
// profile model (cascades, seeding, list transforms, draft gating) ---

/// Which direction a reorder press moves a profile: one slot toward
/// the list's head (`Up`) or tail (`Down`) — pure data for
/// [`move_profile`], one write per press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReorderDirection {
    /// Move one slot toward index 0.
    Up,
    /// Move one slot toward the list's end.
    Down,
}

/// Whether `name` is free to commit under the model's id rule: exact,
/// case-sensitive equality against every profile except the one being
/// edited (`editing`), whose own name is self-excluded — re-committing
/// a profile's own name is not a collision. The write-time half of
/// name-as-id; read time tolerates hand-edited duplicates.
pub fn name_unique(profiles: &[Profile], editing: Option<usize>, name: &str) -> bool {
    !profiles
        .iter()
        .enumerate()
        .any(|(index, profile)| Some(index) != editing && profile.name == name)
}

/// The `default_profile` value after renaming `old_name` to
/// `new_name`: the key follows the rename when it named the renamed
/// profile — the cascade's paired write — and is untouched otherwise
/// (an absent default stays absent).
pub fn default_after_rename(
    default: Option<&str>,
    old_name: &str,
    new_name: &str,
) -> Option<String> {
    match default {
        Some(default_name) if default_name == old_name => Some(new_name.to_string()),
        Some(default_name) => Some(default_name.to_string()),
        None => None,
    }
}

/// The `default_profile` value after deleting `name`: the delete
/// proceeds and the key auto-clears when it named the deleted
/// profile — a write must never dangle (owner ruling 2026-10-05).
pub fn default_after_delete(default: Option<&str>, name: &str) -> Option<String> {
    match default {
        Some(default_name) if default_name != name => Some(default_name.to_string()),
        _ => None,
    }
}

/// The name a cloned draft seeds: the source's name suffixed
/// `(copy)`, escalating to `(copy 2)`, `(copy 3)`, … past collisions,
/// so the seed is always unique against the list it will join — the
/// draft still validates before any write.
pub fn clone_name(profiles: &[Profile], source: &Profile) -> String {
    let mut candidate = format!("{} (copy)", source.name);
    let mut suffix = 2;
    while crate::profile::find(profiles, &candidate).is_some() {
        candidate = format!("{} (copy {suffix})", source.name);
        suffix += 1;
    }
    candidate
}

/// Append `addition` at the end of the list: config order is the `+`
/// dropdown / context-menu order, so add lands last by construction.
pub fn add_profile(profiles: &[Profile], addition: Profile) -> Vec<Profile> {
    let mut list = profiles.to_vec();
    list.push(addition);
    list
}

/// Replace the profile at `index` with `edited`, every other slot
/// byte-identical. A stale out-of-bounds index (the config shrank
/// under the click) is a pure no-op, never a panic.
pub fn apply_edit(profiles: &[Profile], index: usize, edited: Profile) -> Vec<Profile> {
    match profiles.get(index) {
        Some(_) => {
            let mut list = profiles.to_vec();
            list[index] = edited;
            list
        }
        None => profiles.to_vec(),
    }
}

/// Drop the profile at `index`; a stale out-of-bounds index is a pure
/// no-op (deleting what is already gone writes nothing).
pub fn remove_profile(profiles: &[Profile], index: usize) -> Vec<Profile> {
    match profiles.get(index) {
        Some(_) => {
            let mut list = profiles.to_vec();
            list.remove(index);
            list
        }
        None => profiles.to_vec(),
    }
}

/// Move the profile at `index` one slot in `direction`, swapping with
/// its neighbor. Boundary rows (and stale out-of-bounds indices) are
/// pure no-ops — the view disables the direction with no neighbor to
/// swap into.
pub fn move_profile(
    profiles: &[Profile],
    index: usize,
    direction: ReorderDirection,
) -> Vec<Profile> {
    let neighbor = match direction {
        ReorderDirection::Up => index.checked_sub(1),
        ReorderDirection::Down => Some(index + 1),
    };
    match neighbor.filter(|neighbor| *neighbor < profiles.len()) {
        Some(neighbor) => {
            let mut list = profiles.to_vec();
            list.swap(index, neighbor);
            list
        }
        None => profiles.to_vec(),
    }
}

/// Parse a `cwd` draft: empty (or whitespace-only) text clears to
/// `None` — the model's default cwd — and any other path is accepted
/// verbatim. Existence is never checked at define time; spawn-time
/// fallback is `profile-launch`'s.
pub fn parse_cwd(raw: &str) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        None
    } else {
        Some(PathBuf::from(raw))
    }
}

/// Whether a draft triple may commit: a non-blank name unique against
/// the list (self-excluded on edit) plus a command line that survives
/// [`command_line_parts`] — [`Profile::validate`] plus the uniqueness
/// this page adds at write time. The cwd is not gated: any path or
/// none is definable.
pub fn draft_valid(
    profiles: &[Profile],
    editing: Option<usize>,
    name: &str,
    command: &str,
) -> bool {
    !name.trim().is_empty()
        && name_unique(profiles, editing, name)
        && command_line_parts(command).is_some()
}

// --- the profile form: draft, per-field commits, lifecycle ---

/// One of the form's three text inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileField {
    /// The profile's name — also its id.
    Name,
    /// The command line to run.
    Command,
    /// The optional working directory (raw path text).
    Cwd,
}

/// Why a form commit rejected, for the inline error under the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileFieldError {
    /// The name is empty or whitespace.
    BlankName,
    /// Another profile already carries the exact name.
    DuplicateName,
    /// The command line has no program to run.
    BlankCommand,
}

impl ProfileFieldError {
    /// The inline error copy shown under the field.
    pub const fn message(self) -> &'static str {
        match self {
            Self::BlankName => "Name is required",
            Self::DuplicateName => "That name is already taken",
            Self::BlankCommand => "Command needs a program to run",
        }
    }
}

/// What the draft is committing as: a fresh profile (Add), a copy of
/// one (Clone), or an edit of the list profile at `index` (Edit).
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileDraftMode {
    /// The Add button: empty fields, appended on first valid commit.
    Add,
    /// A row's Clone button: seeded from `source`.
    Clone { source: Profile },
    /// A row selection: seeded from the profile at `index`.
    Edit { index: usize },
}

/// The form's one-at-a-time draft: the three raw text fields plus the
/// per-field error state of the last rejected commit. Not a
/// [`SettingsBuffer`] extension — the page owns its lifecycle
/// (see [`ProfileDraftTransition`]), and [`TextField`]'s four arms
/// stay pinned to the Settings page's keys.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileDraft {
    /// What a commit writes this draft as.
    pub mode: ProfileDraftMode,
    /// The name input's buffered text.
    pub name: String,
    /// The command input's buffered text.
    pub command: String,
    /// The cwd input's buffered raw path text.
    pub cwd: String,
    /// The last rejected commit's field and error, if any.
    pub error: Option<(ProfileField, ProfileFieldError)>,
}

impl ProfileDraft {
    /// The Add button's empty draft.
    pub fn add() -> Self {
        Self {
            mode: ProfileDraftMode::Add,
            name: String::new(),
            command: String::new(),
            cwd: String::new(),
            error: None,
        }
    }

    /// A row's Clone draft: the source's fields with the name
    /// re-seeded unique by [`clone_name`]. Seeding writes nothing.
    pub fn clone_of(source: &Profile, profiles: &[Profile]) -> Self {
        Self {
            mode: ProfileDraftMode::Clone {
                source: source.clone(),
            },
            name: clone_name(profiles, source),
            command: source.command.clone(),
            cwd: cwd_text(source),
            error: None,
        }
    }

    /// A row selection's Edit draft, seeded from the profile at
    /// `index`.
    pub fn edit(index: usize, profile: &Profile) -> Self {
        Self {
            mode: ProfileDraftMode::Edit { index },
            name: profile.name.clone(),
            command: profile.command.clone(),
            cwd: cwd_text(profile),
            error: None,
        }
    }

    /// Buffer a field's text (a keystroke): display-only, and any
    /// stale error clears — it described text that no longer exists.
    pub fn set_field_text(&mut self, field: ProfileField, text: String) {
        self.error = None;
        match field {
            ProfileField::Name => self.name = text,
            ProfileField::Command => self.command = text,
            ProfileField::Cwd => self.cwd = text,
        }
    }

    /// After `field`'s commit landed as `written`: reset that field's
    /// buffered text to the written value (the input falls back to
    /// what the echo will serve) and clear the error — the other
    /// fields' buffered drafts persist, per-field commits.
    pub fn field_committed(&mut self, field: ProfileField, written: &Profile) {
        self.error = None;
        match field {
            ProfileField::Name => self.name.clone_from(&written.name),
            ProfileField::Command => self.command.clone_from(&written.command),
            ProfileField::Cwd => self.cwd = cwd_text(written),
        }
    }
}

/// The cwd input's text for a profile's cwd: the path verbatim, empty
/// for `None`.
fn cwd_text(profile: &Profile) -> String {
    profile
        .cwd
        .as_ref()
        .map_or_else(String::new, |path| path.display().to_string())
}

/// What an Enter in one form field produced: the write to land, the
/// rejection to show, or the stale-edit drop.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileCommitPlan {
    /// A valid commit: one whole-list write, plus the paired
    /// `default_profile` write when the commit renamed the
    /// default-named profile ([`default_after_rename`] — `None` means
    /// no paired write).
    Write {
        /// The new `profiles` list.
        profiles: Vec<Profile>,
        /// The rename cascade's new default, if the commit hit it.
        default_after_rename: Option<String>,
    },
    /// An invalid commit: the field at fault and why. Nothing is
    /// written; the draft keeps its text.
    Invalid {
        /// The field the inline error renders under.
        field: ProfileField,
        /// The rejection's reason.
        error: ProfileFieldError,
    },
    /// The edited profile is gone (the list changed under the open
    /// draft): nothing to commit — the draft is stale and drops.
    StaleEdit,
}

/// Plan the commit an Enter in `field` with `raw` produces under
/// `draft` — the pure heart of the form.
///
/// Per-field commits: an Edit-mode Enter validates that field and
/// writes the base profile with only it replaced (buffered but
/// un-Enter'd edits to other fields ride along nowhere). Add and
/// Clone drafts are whole-triple gated — a profile enters the list
/// only on its first fully valid commit, because [`Profile::validate`]
/// rejects a blank command — with the error pointing at whichever
/// field is invalid. Name commits trim and enforce uniqueness
/// (self-excluded on edit); command commits go through
/// [`command_line_parts`]; cwd commits go through [`parse_cwd`] and
/// can never reject.
pub fn plan_profile_commit(
    draft: &ProfileDraft,
    field: ProfileField,
    raw: &str,
    profiles: &[Profile],
    default: Option<&str>,
) -> ProfileCommitPlan {
    match &draft.mode {
        ProfileDraftMode::Edit { index } => {
            let Some(base) = profiles.get(*index) else {
                return ProfileCommitPlan::StaleEdit;
            };
            let mut edited = base.clone();
            match field {
                ProfileField::Name => {
                    let name = raw.trim();
                    if name.is_empty() {
                        return invalid_name(ProfileFieldError::BlankName);
                    }
                    if !name_unique(profiles, Some(*index), name) {
                        return invalid_name(ProfileFieldError::DuplicateName);
                    }
                    edited.name = name.to_string();
                }
                ProfileField::Command => {
                    let command = raw.trim();
                    if command_line_parts(command).is_none() {
                        return ProfileCommitPlan::Invalid {
                            field: ProfileField::Command,
                            error: ProfileFieldError::BlankCommand,
                        };
                    }
                    edited.command = command.to_string();
                }
                ProfileField::Cwd => edited.cwd = parse_cwd(raw),
            }
            // The cascade: only a real rename of the default-named
            // profile re-points the key (a same-name re-commit is not
            // a rename), and only when the key's value actually
            // changes — an unchanged default writes nothing extra.
            let default_after_rename = match field {
                ProfileField::Name if edited.name != base.name => {
                    match default_after_rename(default, &base.name, &edited.name) {
                        Some(new_default) if Some(new_default.as_str()) != default => {
                            Some(new_default)
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            ProfileCommitPlan::Write {
                profiles: apply_edit(profiles, *index, edited),
                default_after_rename,
            }
        }
        ProfileDraftMode::Add | ProfileDraftMode::Clone { .. } => {
            // The Enter'd raw text replaces its field; the rest of
            // the triple is the draft's buffered text.
            let name = match field {
                ProfileField::Name => raw.trim(),
                _ => draft.name.trim(),
            };
            let command = match field {
                ProfileField::Command => raw.trim(),
                _ => draft.command.trim(),
            };
            let cwd_raw = match field {
                ProfileField::Cwd => raw,
                _ => draft.cwd.as_str(),
            };
            // The whole-triple gate is `draft_valid`; the checks
            // inside pinpoint the field for the inline error.
            if !draft_valid(profiles, None, name, command) {
                if name.is_empty() {
                    return invalid_name(ProfileFieldError::BlankName);
                }
                if !name_unique(profiles, None, name) {
                    return invalid_name(ProfileFieldError::DuplicateName);
                }
                return ProfileCommitPlan::Invalid {
                    field: ProfileField::Command,
                    error: ProfileFieldError::BlankCommand,
                };
            }
            let added = Profile {
                name: name.to_string(),
                command: command.to_string(),
                cwd: parse_cwd(cwd_raw),
            };
            ProfileCommitPlan::Write {
                profiles: add_profile(profiles, added),
                default_after_rename: None,
            }
        }
    }
}

/// An invalid-name rejection (the field arm is always `Name`).
fn invalid_name(error: ProfileFieldError) -> ProfileCommitPlan {
    ProfileCommitPlan::Invalid {
        field: ProfileField::Name,
        error,
    }
}

/// A Profiles-page draft lifecycle transition — the page-local
/// half of [`DrawerTransition`]'s rule: focus-loss hides preserve the
/// open draft (and its buffered text) for the next show; the drawer's
/// explicit close and the page's back affordance both clear it, so a
/// reopened page starts from config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileDraftTransition {
    /// The surface hid with the drawer open: keep everything.
    Hide,
    /// An explicit drawer close, or leaving the page: drop the draft.
    Clear,
}

/// Apply a draft lifecycle transition to the page's optional draft.
pub fn apply_profile_draft_transition(
    draft: &mut Option<ProfileDraft>,
    transition: ProfileDraftTransition,
) {
    if matches!(transition, ProfileDraftTransition::Clear) {
        *draft = None;
    }
}

// --- row actions: delete + auto-clear, reorder ---

/// The plan a delete produces: the list without the profile at
/// `index`, plus the paired `default_profile` write when the deleted
/// profile was the one the key named — the owner's auto-clear ruling
/// (a write must never dangle; the picker can never show a stale
/// name). A stale out-of-bounds index plans nothing. Delete is
/// immediate — no dialog (OQ-1's standing ruling).
pub fn plan_profile_delete(
    profiles: &[Profile],
    index: usize,
    default: Option<&str>,
) -> Option<(Vec<Profile>, Option<ProfileWrite>)> {
    let removed = profiles.get(index)?;
    let paired = (default == Some(removed.name.as_str()))
        .then(|| ProfileWrite::DefaultProfile(default_after_delete(default, &removed.name)));
    Some((remove_profile(profiles, index), paired))
}

/// The plan a reorder press produces: the list with the profile moved
/// one slot in `direction`. Boundary presses (the disabled
/// direction) plan nothing — no same-value write.
pub fn plan_profile_reorder(
    profiles: &[Profile],
    index: usize,
    direction: ReorderDirection,
) -> Option<Vec<Profile>> {
    let moved = move_profile(profiles, index, direction);
    (moved != profiles).then_some(moved)
}

/// Whether a row's affordance for `direction` does anything: an
/// interior neighbor exists to swap with. Boundary rows render the
/// out-of-bounds direction disabled.
pub const fn can_move(profiles_len: usize, index: usize, direction: ReorderDirection) -> bool {
    match direction {
        ReorderDirection::Up => index > 0,
        ReorderDirection::Down => index + 1 < profiles_len,
    }
}

/// The selection after deleting `index`: the deleted row's selection
/// drops (its draft with it), a later selection shifts down to keep
/// naming the same profile, anything else is untouched.
pub const fn selection_after_delete(selection: Option<usize>, index: usize) -> Option<usize> {
    match selection {
        Some(selected) if selected == index => None,
        Some(selected) if selected > index => Some(selected - 1),
        other => other,
    }
}

/// The edit-draft mode after deleting `index`: an edit of the deleted
/// row drops the whole draft; a later edit shifts; non-edit drafts
/// and no-draft pass through untouched.
pub fn draft_after_delete(draft: Option<ProfileDraft>, index: usize) -> Option<ProfileDraft> {
    let mut draft = draft?;
    if let ProfileDraftMode::Edit {
        index: edited_index,
    } = draft.mode
    {
        draft.mode = match edited_index.cmp(&index) {
            std::cmp::Ordering::Equal => return None,
            std::cmp::Ordering::Greater => ProfileDraftMode::Edit {
                index: edited_index - 1,
            },
            std::cmp::Ordering::Less => ProfileDraftMode::Edit {
                index: edited_index,
            },
        };
    }
    Some(draft)
}

/// The selection after a reorder press on `index`: the swap trades
/// the two involved slots' selections (the selection follows the
/// moved profile); every other selection is untouched.
pub const fn selection_after_move(
    selection: Option<usize>,
    index: usize,
    direction: ReorderDirection,
) -> Option<usize> {
    let neighbor = match direction {
        ReorderDirection::Up => index.saturating_sub(1),
        ReorderDirection::Down => index + 1,
    };
    match selection {
        Some(selected) if selected == index => Some(neighbor),
        Some(selected) if selected == neighbor => Some(index),
        other => other,
    }
}

// --- the profile persist path: the module's write contract ---

/// A planned profile-key write — the list-shaped analog of
/// [`PlannedWrite`]. The store persists one key per file, so the
/// `profiles` Vec is the unit of write (every mutation — add, edit,
/// delete, reorder, clone — lands as one whole-list write) and the
/// default picker writes `default_profile` alone; the rename cascade
/// and the delete auto-clear are the only two-key interactions, each
/// a pair of writes through this one path.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileWrite {
    /// The whole `profiles` key.
    Profiles(Vec<Profile>),
    /// The `default_profile` key (`None` clears it to plain shell).
    DefaultProfile(Option<String>),
}

impl ProfileWrite {
    /// Persist through the config backend — the I/O edge of the
    /// profile write path, `PlannedWrite::persist`'s discipline
    /// verbatim. The failure is the caller's to log.
    pub fn persist(&self, handler: &cosmic_config::Config) -> Result<(), cosmic_config::Error> {
        match self {
            ProfileWrite::Profiles(profiles) => handler.set("profiles", profiles.clone()),
            ProfileWrite::DefaultProfile(default) => {
                handler.set("default_profile", default.clone())
            }
        }
    }
}

/// The Keybindings tab's whole-map write: the complete
/// `custom_key_binds` map, persisted in one `set` — the
/// [`ProfileWrite`] precedent for a map-shaped key. A rebind and a
/// revert (entry removal) are the same whole-map write, and the
/// scalar [`PlannedWrite`]/[`ConfigKey`] surface stays untouched —
/// the 23 exposed keys never grow a 24th.
#[derive(Debug, Clone, PartialEq)]
pub struct KeybindsWrite(pub std::collections::BTreeMap<String, crate::keybinds::Binding>);

impl KeybindsWrite {
    /// Persist through the config backend — [`ProfileWrite::persist`]'s
    /// discipline verbatim. The failure is the caller's to log.
    pub fn persist(&self, handler: &cosmic_config::Config) -> Result<(), cosmic_config::Error> {
        handler.set("custom_key_binds", self.0.clone())
    }
}

// --- the default-profile picker ---

/// The picker's row 0 label: the cleared key, plain shell.
pub const DEFAULT_PICKER_NONE_ROW: &str = "None (plain shell)";

/// The picker's rows: [`DEFAULT_PICKER_NONE_ROW`] first, then one
/// entry per profile in `profiles` Vec order — the same order the `+`
/// dropdown and the context-menu submenu serve.
pub fn default_picker_options(profiles: &[Profile]) -> Vec<String> {
    std::iter::once(DEFAULT_PICKER_NONE_ROW.to_string())
        .chain(profiles.iter().map(|profile| profile.name.clone()))
        .collect()
}

/// The picker's selection row: the row
/// [`crate::profile::effective_default`] resolves to — row 0 when no
/// default resolves (unset, or dangling: the picker never displays a
/// name the list lacks; read-side tolerance, no rewrite). Always a
/// row; the caller's `Some` is the dropdown API's shape.
pub fn default_picker_selected(profiles: &[Profile], default: Option<&str>) -> usize {
    // The position is found by name (not pointer identity) against
    // the same list `effective_default` resolved it from, so it
    // always exists.
    let index = crate::profile::effective_default(profiles, default)
        .and_then(|resolved| profiles.iter().position(|p| p.name == resolved.name));
    index.map_or(0, |position| 1 + position)
}

/// The write a picker row commits: row 0 clears the key (`None`), a
/// profile row commits its name. Any other index is stale (the list
/// changed under the open dropdown) and rejects — no write, never an
/// invented value.
pub fn default_picker_write(profiles: &[Profile], index: usize) -> Option<ProfileWrite> {
    match index {
        0 => Some(ProfileWrite::DefaultProfile(None)),
        _ => profiles
            .get(index - 1)
            .map(|profile| ProfileWrite::DefaultProfile(Some(profile.name.clone()))),
    }
}

/// An event from the settings view. Events carry inputs, never
/// [`PlannedWrite`]s: the view stays dumb about validity, and the
/// app resolves each event through the pure planners
/// ([`submit_write`], [`dropdown_write`], [`toggle_write`],
/// [`slider_release_write`]) so every commit is clamped and every
/// invalid input is a no-op — never a panic in a view closure.
#[derive(Debug, Clone)]
pub enum SettingsEvent {
    /// A text input's content changed — display only (no write until
    /// submit).
    Draft(TextField, String),
    /// A text input was submitted (Enter): parse, write if it parses.
    Submitted(TextField, String),
    /// A slider moved — display only (no write until release).
    SliderMoved(ConfigKey, f32),
    /// A slider was released: commit its buffered position.
    SliderReleased(ConfigKey),
    /// A dropdown option was picked: commit on change.
    Picked(ConfigKey, usize),
    /// A toggler flipped: commit on change.
    Toggled(ConfigKey, bool),
    /// The Settings page's Profiles entry row: flip the drawer to the
    /// Profiles page.
    OpenProfiles,
    /// The Profiles page's back affordance: return to the Settings
    /// page (a page turn, never a drawer close).
    BackFromProfiles,
    /// A profile row was pressed: make it the selection driving the
    /// edit form.
    SelectProfile(usize),
    /// A default-profile picker row was picked: commit on change.
    PickDefaultProfile(usize),
    /// The Add affordance: open an empty draft form.
    OpenProfileDraft,
    /// A row's Clone action: seed a draft from that profile.
    CloneProfile(usize),
    /// A form field's content changed — display only (no write until
    /// its Enter).
    DraftProfileField(ProfileField, String),
    /// A form field was submitted (Enter): validate that field, write
    /// the whole list if it commits.
    CommitProfileField(ProfileField, String),
    /// A row's Delete action: immediate, no dialog (OQ-1's standing
    /// ruling).
    DeleteProfile(usize),
    /// A row's reorder press: move that profile one slot.
    ReorderProfile(usize, ReorderDirection),
}

/// The write a toggle flip commits. `None` for keys without a toggle.
pub const fn toggle_write(key: ConfigKey, checked: bool) -> Option<PlannedWrite> {
    match key {
        ConfigKey::HideOnFocusLoss => Some(PlannedWrite::HideOnFocusLoss(checked)),
        ConfigKey::SessionPersistenceEnabled => {
            Some(PlannedWrite::SessionPersistenceEnabled(checked))
        }
        ConfigKey::SlideAnimationEnabled => Some(PlannedWrite::SlideAnimationEnabled(checked)),
        ConfigKey::BellNotificationEnabled => Some(PlannedWrite::BellNotificationEnabled(checked)),
        ConfigKey::NewTabAfterCurrent => Some(PlannedWrite::NewTabAfterCurrent(checked)),
        ConfigKey::RespectPanel => Some(PlannedWrite::RespectPanel(checked)),
        _ => None,
    }
}

impl Section {
    /// The card's heading text.
    pub const fn title(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Geometry => "Geometry",
            Section::Behavior => "Behavior",
            Section::Terminal => "Terminal",
            Section::Advanced => "Advanced",
        }
    }
}

impl ConfigKey {
    /// The control's human-facing label (its file field is
    /// [`ConfigKey::key_name`]).
    pub const fn label(self) -> &'static str {
        match self {
            ConfigKey::HeightPercent => "Height",
            ConfigKey::WidthPercent => "Width",
            ConfigKey::HAlign => "Alignment",
            ConfigKey::Edge => "Edge",
            ConfigKey::TabPosition => "Tab bar position",
            ConfigKey::FontName => "Font",
            ConfigKey::FontSize => "Font size",
            ConfigKey::Opacity => "Opacity",
            ConfigKey::HideOnFocusLoss => "Hide on focus loss",
            ConfigKey::SessionPersistenceEnabled => "Session persistence",
            ConfigKey::SlideAnimationEnabled => "Slide animation",
            ConfigKey::BellNotificationEnabled => "Bell notifications",
            ConfigKey::TabTitleMode => "Tab titles",
            ConfigKey::NewTabAfterCurrent => "New tabs after current tab",
            ConfigKey::RespectPanel => "Respect panel",
            ConfigKey::Shell => "Shell",
            ConfigKey::Args => "Arguments",
            ConfigKey::ScrollbackLines => "Scrollback lines",
            ConfigKey::Layer => "Layer",
            ConfigKey::KeyboardMode => "Keyboard mode",
            ConfigKey::ColorSchemeDark => "Dark color scheme",
            ConfigKey::ColorSchemeLight => "Light color scheme",
            ConfigKey::ThemePreference => "Application theme",
        }
    }
}

/// The font-size slider's display range. The slider constrains what
/// the control can produce; the write path applies no extra clamp.
const FONT_SIZE_SLIDER_MIN: f32 = 6.0;
const FONT_SIZE_SLIDER_MAX: f32 = 32.0;

/// The drawer pane's maximum width, mirroring libcosmic's
/// `Core::context_width(has_nav = false)` clamp (344–480px with the
/// reserved content width subtracted) so the drawer composed by hand
/// in `view_window` sizes exactly like a framework-hosted one. The
/// overlay clamps to the surface below the floor, so the pane never
/// overflows a narrow drop-down — how much terminal it covers there
/// is the spec's hardware-checklist item (OQ-3).
pub fn drawer_max_width(surface_width: f32) -> f32 {
    // Content width (360px) + padding (8px) — the same reservation
    // `context_width` makes for an app without a nav bar.
    let reserved = 360.0 + 8.0;
    (surface_width - reserved).clamp(344.0, 480.0)
}

/// The General tab's page: the everyday sections in [`tab_sections`]
/// display order plus the Profiles entry row — the drill-down's only
/// door. Every control renders from `config` with a buffered draft
/// winning while one exists and emits [`SettingsEvent`]s — nothing
/// here applies anything itself; commits land through the app's
/// write path and the `ConfigChanged` echo.
pub fn general_view(
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
) -> Element<'static, SettingsEvent> {
    tab_page(SettingsTab::General, config, buffer)
}

/// The Advanced tab's page: exactly the Advanced card as it hosted
/// in the one-column drawer — the power controls.
pub fn advanced_view(
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
) -> Element<'static, SettingsEvent> {
    tab_page(SettingsTab::Advanced, config, buffer)
}

/// A tab's page: one `widget::settings` card per section the tab
/// hosts, one labeled item per exposed key with its apply timing as
/// the description, in one column the framework drawer pane already
/// wraps in a scrollable — plus the Profiles entry row where the tab
/// carries it. The `drawer_section`/`drawer_item`/`drawer_control`
/// chain is reused verbatim from the one-column drawer.
fn tab_page(
    tab: SettingsTab,
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
) -> Element<'static, SettingsEvent> {
    let mut cards: Vec<Element<'static, SettingsEvent>> = tab_sections(tab)
        .iter()
        .map(|section| drawer_section(*section, config, buffer))
        .collect();
    if tab_has_profiles_entry(tab) {
        // The Profiles entry card — the one row on these pages that
        // writes no key itself.
        cards.push(profiles_entry(&config.profiles));
    }
    widget::settings::view_column(cards).into()
}

/// The Settings page's Profiles entry card (OQ-2's drafted
/// composition, ratified at the hardware STOP): the profile count as
/// the row's description, a `Manage…` button as the flip affordance.
/// No key is written from here — the row's whole job is the page
/// turn.
fn profiles_entry(profiles: &[Profile]) -> Element<'static, SettingsEvent> {
    widget::settings::section()
        .title("Profiles")
        .add(
            widget::settings::item::builder("Profile presets")
                .description(profiles_entry_description(profiles.len()))
                .control(widget::button::standard("Manage…").on_press(SettingsEvent::OpenProfiles)),
        )
        .into()
}

/// The entry card's count copy — pure so the wording is pinned.
fn profiles_entry_description(count: usize) -> String {
    match count {
        0 => "No profiles".to_string(),
        1 => "1 profile".to_string(),
        n => format!("{n} profiles"),
    }
}

/// The Profiles page: the read-only list — one row per profile in
/// config order (the `+` dropdown / context-menu order), name plus a
/// command preview, selection driving the edit form — with a back
/// affordance above it. An empty list shows the empty state with the
/// add affordance placed but inert until the form ships. Read-only by
/// construction: the rows emit selection events only; every mutation
/// control joins in the later tasks.
pub fn profiles_view(
    config: &DropTermConfig,
    selection: Option<usize>,
    draft: Option<&ProfileDraft>,
) -> Element<'static, SettingsEvent> {
    let mut cards: Vec<Element<'static, SettingsEvent>> = vec![
        widget::button::text("Back")
            .on_press(SettingsEvent::BackFromProfiles)
            .into(),
    ];
    // The default picker: commit-on-change, the drawer's dropdown
    // rule. Selection mirrors `effective_default`, so it never opens
    // on a name the list lacks.
    cards.push(
        widget::settings::section()
            .title("Default profile")
            .add(
                widget::settings::item::builder("Plain new tabs run")
                    .description("next spawn")
                    .control(widget::dropdown(
                        default_picker_options(&config.profiles),
                        Some(default_picker_selected(
                            &config.profiles,
                            config.default_profile.as_deref(),
                        )),
                        SettingsEvent::PickDefaultProfile,
                    )),
            )
            .into(),
    );
    if config.profiles.is_empty() {
        cards.push(
            widget::settings::section()
                .title("Profiles")
                .add(
                    widget::settings::item::builder("No profiles")
                        .description("Named presets of what a tab runs.")
                        .control(
                            widget::button::standard("Add profile")
                                .on_press(SettingsEvent::OpenProfileDraft),
                        ),
                )
                .into(),
        );
    } else {
        let mut list = widget::settings::section().title("Profiles");
        for (index, profile) in config.profiles.iter().enumerate() {
            list = list.add(profile_row(
                config.profiles.len(),
                index,
                profile,
                selection == Some(index),
            ));
        }
        cards.push(list.into());
        // The list's add affordance, below the rows it appends after.
        cards.push(
            widget::button::standard("Add profile")
                .on_press(SettingsEvent::OpenProfileDraft)
                .into(),
        );
    }
    // The form: one draft at a time — an Add/Clone draft the buttons
    // opened, or the Edit draft a row selection seeded.
    if let Some(draft) = draft {
        cards.push(profile_form(draft));
    }
    widget::settings::view_column(cards).into()
}

/// The form section: the draft's three text inputs (commit on Enter,
/// the house text-input rule) with the last rejection's inline error
/// under its field.
fn profile_form(draft: &ProfileDraft) -> Element<'static, SettingsEvent> {
    let title = match &draft.mode {
        ProfileDraftMode::Add | ProfileDraftMode::Clone { .. } => "New profile",
        ProfileDraftMode::Edit { .. } => "Edit profile",
    };
    widget::settings::section()
        .title(title)
        .add(form_field(
            ProfileField::Name,
            "Name",
            &draft.name,
            "web-prod",
            draft,
        ))
        .add(form_field(
            ProfileField::Command,
            "Command",
            &draft.command,
            "ssh web-prod",
            draft,
        ))
        .add(form_field(
            ProfileField::Cwd,
            "Working directory",
            &draft.cwd,
            "optional",
            draft,
        ))
        .into()
}

/// One form field: the labeled text input with its inline error, when
/// the last rejection pointed here.
fn form_field(
    field: ProfileField,
    label: &'static str,
    text: &str,
    placeholder: &'static str,
    draft: &ProfileDraft,
) -> Element<'static, SettingsEvent> {
    let input = widget::text_input(placeholder, text.to_string())
        .on_input(move |text| SettingsEvent::DraftProfileField(field, text))
        .on_submit(move |text| SettingsEvent::CommitProfileField(field, text));
    let control = match draft.error {
        Some((at, error)) if at == field => widget::column![
            input,
            widget::text(error.message())
                .size(12)
                .class(cosmic::theme::Text::Custom(danger_text_style)),
        ]
        .spacing(4),
        _ => widget::column![input],
    };
    widget::settings::item::builder(label)
        .control(control)
        .into()
}

/// The inline-error text style: the theme's destructive color — the
/// same red the destructive-button class carries (the pinned rev has
/// no `Text::Danger` variant).
fn danger_text_style(theme: &cosmic::theme::Theme) -> cosmic::iced::widget::text::Style {
    let cosmic = theme.cosmic();
    cosmic::iced::widget::text::Style {
        color: Some(cosmic.destructive.base.into()),
        selected_fill: cosmic.accent.base.into(),
        selected_text_color: Some(cosmic.on_accent_color().into()),
    }
}

/// One list row: the profile's name with its command line as the
/// preview beneath, the whole row a button selecting it. The selected
/// row carries the suggested style — selection is the edit form's
/// seed, so it must be visible on light and dark panes alike (an
/// OQ-3 hardware item). The action cluster: Clone, an immediate
/// Delete (no dialog — OQ-1), and up/down reorder with the boundary
/// direction disabled.
fn profile_row(
    profiles_len: usize,
    index: usize,
    profile: &Profile,
    selected: bool,
) -> Element<'static, SettingsEvent> {
    let class = if selected {
        theme::Button::Suggested
    } else {
        theme::Button::Standard
    };
    widget::row!(
        widget::button::custom(widget::column![
            widget::text(profile.name.clone()),
            widget::text(profile.command.clone()).size(12),
        ])
        .class(class)
        .on_press(SettingsEvent::SelectProfile(index)),
        widget::button::text("Clone").on_press(SettingsEvent::CloneProfile(index)),
        widget::button::destructive("Delete").on_press(SettingsEvent::DeleteProfile(index)),
        widget::button::text("↑").on_press_maybe(
            can_move(profiles_len, index, ReorderDirection::Up)
                .then_some(SettingsEvent::ReorderProfile(index, ReorderDirection::Up)),
        ),
        widget::button::text("↓").on_press_maybe(
            can_move(profiles_len, index, ReorderDirection::Down)
                .then_some(SettingsEvent::ReorderProfile(index, ReorderDirection::Down)),
        ),
    )
    .spacing(8)
    .into()
}

/// One drawer section: a `widget::settings` section carrying the
/// section's title and one item per key it owns, in `keys()` display
/// order.
fn drawer_section(
    section: Section,
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
) -> Element<'static, SettingsEvent> {
    let mut built = widget::settings::section().title(section.title());
    for key in section.keys() {
        built = built.add(drawer_item(*key, config, buffer));
    }
    built.into()
}

/// One drawer item: the key's label with its apply timing as the
/// item description (the honesty rule carried into the drawer), and
/// the key's control on the drawer's natural widths — the window's
/// fixed `CONTROL_WIDTH` is gone, the pane owns width now.
fn drawer_item(
    key: ConfigKey,
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
) -> Element<'static, SettingsEvent> {
    let control = drawer_control(key, config, buffer)
        .expect("every exposed key has exactly one control (section map)");
    widget::settings::item::builder(key.label())
        .description(apply_timing(key).label())
        .control(control)
        .into()
}

/// The key's control widget: sliders (the buffered readout above),
/// then dropdowns, then toggles, then text inputs. Each builder
/// claims only its own keys; the `expect` in [`drawer_item`] is
/// totality — every exposed key has exactly one control, pinned by
/// the section-map test.
fn drawer_control(
    key: ConfigKey,
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
) -> Option<Element<'static, SettingsEvent>> {
    slider_control(key, config, buffer)
        .or_else(|| dropdown_control(key, config))
        .or_else(|| toggle_control(key, config))
        .or_else(|| text_control(key, config, buffer))
}

/// A slider control (commit on release). `on_change` only buffers
/// the position for the live readout; the release commits through
/// [`slider_release_write`], clamped.
fn slider_control(
    key: ConfigKey,
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
) -> Option<Element<'static, SettingsEvent>> {
    let value = buffer.slider_value(key, config);
    let range = match key {
        ConfigKey::HeightPercent => f32::from(MIN_HEIGHT_PERCENT)..=f32::from(MAX_HEIGHT_PERCENT),
        ConfigKey::WidthPercent => f32::from(MIN_WIDTH_PERCENT)..=f32::from(MAX_WIDTH_PERCENT),
        ConfigKey::Opacity => 0.0..=100.0,
        ConfigKey::FontSize => FONT_SIZE_SLIDER_MIN..=FONT_SIZE_SLIDER_MAX,
        _ => return None,
    };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "integer readout of a range-bounded slider value; non-negative by the control's range"
    )]
    let readout = value.round() as u32;
    Some(
        widget::column![
            widget::text(format!("{readout}")).size(12),
            widget::slider(range, value, move |v| SettingsEvent::SliderMoved(key, v))
                .on_release(SettingsEvent::SliderReleased(key)),
        ]
        .spacing(4)
        .into(),
    )
}

/// A dropdown control (commit on change). The pick resolves through
/// [`dropdown_write`]; labels and selection come from the same
/// tables the write path reads.
fn dropdown_control(
    key: ConfigKey,
    config: &DropTermConfig,
) -> Option<Element<'static, SettingsEvent>> {
    let labels: Vec<&'static str> = match key {
        ConfigKey::HAlign => H_ALIGN_OPTIONS.iter().map(|(_, label)| *label).collect(),
        ConfigKey::Edge => EDGE_OPTIONS.iter().map(|(_, label)| *label).collect(),
        ConfigKey::TabPosition => TAB_POSITION_OPTIONS
            .iter()
            .map(|(_, label)| *label)
            .collect(),
        ConfigKey::TabTitleMode => TAB_TITLE_MODE_OPTIONS
            .iter()
            .map(|(_, label)| *label)
            .collect(),
        ConfigKey::Layer => LAYER_OPTIONS.iter().map(|(_, label)| *label).collect(),
        ConfigKey::KeyboardMode => KEYBOARD_MODE_OPTIONS
            .iter()
            .map(|(_, label)| *label)
            .collect(),
        // The scheme rows offer the registry itself — names in
        // registry order, not a local table.
        ConfigKey::ColorSchemeDark | ConfigKey::ColorSchemeLight => scheme_names(),
        ConfigKey::ThemePreference => THEME_PREFERENCE_OPTIONS
            .iter()
            .map(|(_, label)| *label)
            .collect(),
        _ => return None,
    };
    Some(
        widget::dropdown(labels, dropdown_selected(key, config), move |index| {
            SettingsEvent::Picked(key, index)
        })
        .into(),
    )
}

/// A toggle control (commit on change). The flip resolves through
/// [`toggle_write`].
fn toggle_control(
    key: ConfigKey,
    config: &DropTermConfig,
) -> Option<Element<'static, SettingsEvent>> {
    let checked = match key {
        ConfigKey::HideOnFocusLoss => config.hide_on_focus_loss,
        ConfigKey::SessionPersistenceEnabled => config.session_persistence_enabled,
        ConfigKey::SlideAnimationEnabled => config.slide_animation_enabled,
        ConfigKey::BellNotificationEnabled => config.bell_notification_enabled,
        ConfigKey::NewTabAfterCurrent => config.new_tab_after_current,
        ConfigKey::RespectPanel => config.respect_panel,
        _ => return None,
    };
    Some(
        widget::toggler(checked)
            .on_toggle(move |checked| SettingsEvent::Toggled(key, checked))
            .into(),
    )
}

/// A text input (commit on submit — per-keystroke writes would
/// persist half-typed values and fight the echo, which re-renders
/// the input from config). The input shows the buffered draft while
/// one exists, else the file's value; the shell field's placeholder
/// carries `resolved_shell()`.
fn text_control(
    key: ConfigKey,
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
) -> Option<Element<'static, SettingsEvent>> {
    let field = match key {
        ConfigKey::FontName => TextField::FontName,
        ConfigKey::Shell => TextField::Shell,
        ConfigKey::Args => TextField::Args,
        ConfigKey::ScrollbackLines => TextField::ScrollbackLines,
        _ => return None,
    };
    let placeholder = match field {
        TextField::Shell => config.resolved_shell(),
        TextField::ScrollbackLines => "10000".to_string(),
        _ => String::new(),
    };
    Some(
        widget::text_input(placeholder, buffer.text(field, config))
            .on_input(move |text| SettingsEvent::Draft(field, text))
            .on_submit(move |text| SettingsEvent::Submitted(field, text))
            .into(),
    )
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

    // --- the drawer pane's width (settings-drawer Task 0) ---

    #[test]
    fn drawer_width_clamps_into_the_framework_band() {
        // Mirrors libcosmic `Core::context_width(has_nav = false)`:
        // the reserved content width (360px + 8px) subtracted, then
        // clamped to the 344–480 band — a hand-composed drawer sizes
        // exactly like a framework-hosted one.
        assert_eq!(drawer_max_width(2000.0), 480.0);
        assert_eq!(drawer_max_width(800.0), 432.0);
        assert_eq!(drawer_max_width(728.0), 360.0);
    }

    #[test]
    fn drawer_width_floors_at_344_on_narrow_surfaces() {
        // Below the band the pane stays at the 344px floor; the
        // overlay clamps to the surface, so the drawer never
        // overflows it. How much terminal it covers on a narrow
        // output is the spec's hardware-checklist item (OQ-3).
        assert_eq!(drawer_max_width(500.0), 344.0);
        assert_eq!(drawer_max_width(300.0), 344.0);
        assert_eq!(drawer_max_width(0.0), 344.0);
    }

    // --- the drawer's migrated content (settings-drawer Task 1) ---

    #[test]
    fn both_tab_pages_build_with_drafts_and_from_config() {
        // ST-2 reorganizes the one five-section column into the two
        // per-tab pages. Construction is the assertion: the item
        // builder's `expect` totality panics if a key resolves to no
        // control, and the widget builders must tolerate the states
        // the file can be in — buffered drafts winning over file
        // values (including a scrollback draft that does not parse —
        // the draft simply renders; rejection is the update path's
        // job) — and the plain default config. Exactly-once across
        // the two pages is tab_sections' partition, pinned above.
        let config = DropTermConfig::default();
        let mut buffer = SettingsBuffer::default();
        buffer.set_text(TextField::FontName, "Draft Font".to_string());
        buffer.set_text(TextField::Shell, "/bin/tcsh".to_string());
        buffer.set_text(TextField::Args, "-l -i".to_string());
        buffer.set_text(TextField::ScrollbackLines, "garbage".to_string());
        buffer.set_slider(ConfigKey::Opacity, 42.0);
        let _ = general_view(&config, &buffer);
        let _ = advanced_view(&config, &buffer);
        let _ = general_view(&config, &SettingsBuffer::default());
        let _ = advanced_view(&config, &SettingsBuffer::default());
    }

    #[test]
    fn every_section_item_builds_a_control_for_all_21_keys() {
        // Criterion 3's construction side: every key of every section
        // resolves to exactly one item control — the builder's
        // `expect` totality panics otherwise — including the toggle
        // keys Tasks 1's chain did not yet resolve, and with every
        // text field holding a draft (the buffer's display-wins rule)
        // and a scrollback draft that does not parse (the draft
        // simply renders; rejection is the update path's job).
        let config = DropTermConfig::default();
        let mut buffer = SettingsBuffer::default();
        buffer.set_text(TextField::FontName, "Draft Font".to_string());
        buffer.set_text(TextField::Shell, "/bin/tcsh".to_string());
        buffer.set_text(TextField::Args, "-l -i".to_string());
        buffer.set_text(TextField::ScrollbackLines, "garbage".to_string());
        for section in Section::ALL {
            for key in section.keys() {
                let _ = drawer_item(*key, &config, &buffer);
            }
        }
    }

    // --- the tab regrouping (settings-tabs ST-1) ---

    #[test]
    fn the_two_tabs_partition_every_section_exactly_once() {
        // The tabs are a view-only regrouping (criterion 2): every
        // Section appears in exactly one tab, the union is
        // Section::ALL, and each tab's display order is pinned so
        // the page composition cannot silently reshuffle.
        assert_eq!(
            tab_sections(SettingsTab::General),
            &[
                Section::Appearance,
                Section::Geometry,
                Section::Behavior,
                Section::Terminal,
            ]
        );
        assert_eq!(tab_sections(SettingsTab::Advanced), &[Section::Advanced]);
        let across_tabs: Vec<Section> = [SettingsTab::General, SettingsTab::Advanced]
            .iter()
            .flat_map(|tab| tab_sections(*tab).iter().copied())
            .collect();
        assert_eq!(across_tabs.len(), Section::ALL.len());
        for section in Section::ALL {
            assert_eq!(
                across_tabs
                    .iter()
                    .filter(|listed| **listed == section)
                    .count(),
                1,
                "{section:?} must appear in exactly one tab"
            );
        }
    }

    #[test]
    fn the_profiles_entry_row_lives_on_general_only() {
        // The drill-down's only door lives on General (criterion 2),
        // so the Back affordance can always return to it; Advanced
        // never grows a second door.
        assert!(tab_has_profiles_entry(SettingsTab::General));
        assert!(!tab_has_profiles_entry(SettingsTab::Advanced));
    }

    #[test]
    fn tab_headers_are_the_strip_labels() {
        // Hardcoded English like every label in this drawer — no
        // fluent/i18n in this repo.
        assert_eq!(tab_header(SettingsTab::General), "General");
        assert_eq!(tab_header(SettingsTab::Advanced), "Advanced");
    }

    // --- the tab strip's model (settings-tabs ST-3) ---

    #[test]
    fn the_strip_model_carries_one_row_per_tab_in_order() {
        // Criterion 7's entity↔tab lockstep, construction side:
        // exactly the two tabs of `SettingsTab::ALL` in order (no
        // dead Keybindings row — that variant belongs to
        // keybinds-editor), each row labeled with its `tab_header`
        // and carrying its `SettingsTab` as data — the payload the
        // activation handler resolves.
        let mut model = tab_strip_model();
        assert_eq!(model.iter().count(), SettingsTab::ALL.len());
        for (position, tab) in SettingsTab::ALL.iter().enumerate() {
            let entity = model
                .entity_at(u16::try_from(position).expect("two tabs, position 0-1"))
                .expect("one row per tab, in ALL order");
            assert_eq!(model.data::<SettingsTab>(entity), Some(tab));
            assert_eq!(model.text(entity), Some(tab_header(*tab)));
        }
    }

    #[test]
    fn activating_a_tab_row_selects_it_and_only_it() {
        // Criterion 7's lockstep, selection side: `activate_tab_row`
        // moves the single selection to the row carrying the tab —
        // the sync every page mutation applies after assigning
        // `settings_page` (pick, Profiles back, explicit close).
        let mut model = tab_strip_model();
        // The strip's selection only ever moves through
        // `activate_tab_row`: a fresh model selects nothing.
        assert_eq!(model.active_data::<SettingsTab>(), None);
        activate_tab_row(&mut model, SettingsTab::Advanced);
        assert_eq!(
            model.active_data::<SettingsTab>(),
            Some(&SettingsTab::Advanced)
        );
        activate_tab_row(&mut model, SettingsTab::General);
        assert_eq!(
            model.active_data::<SettingsTab>(),
            Some(&SettingsTab::General)
        );
    }

    // --- the drawer's lifecycle transitions (settings-drawer T3) ---

    #[test]
    fn an_explicit_drawer_close_clears_the_buffer_for_a_fresh_reopen() {
        // Criterion 8's close rule: Escape, the repeat toggle, and
        // the Close button are explicit closes — the next open
        // renders purely from config, the reopen-fresh rule the
        // settings window carried.
        let config = DropTermConfig::default();
        let mut buffer = SettingsBuffer::default();
        buffer.set_text(TextField::Shell, "/bin/tcsh".to_string());
        buffer.set_slider(ConfigKey::Opacity, 42.0);
        buffer.apply_drawer_transition(DrawerTransition::ExplicitClose);
        assert_eq!(
            buffer.text(TextField::Shell, &config),
            config.shell.clone().unwrap_or_default()
        );
        assert_eq!(
            buffer.slider_value(ConfigKey::Opacity, &config),
            f32::from(config.opacity)
        );
    }

    #[test]
    fn a_focus_loss_hide_preserves_drafts_for_the_next_show() {
        // Criterion 8's hide rule: the surface hiding — focus loss
        // or an explicit hide, whatever re-shows later — never
        // touches the drawer's state; the next show renders the
        // drawer exactly as it was, drafts intact.
        let config = DropTermConfig::default();
        let mut buffer = SettingsBuffer::default();
        buffer.set_text(TextField::Shell, "/bin/tcsh".to_string());
        buffer.set_slider(ConfigKey::Opacity, 42.0);
        buffer.apply_drawer_transition(DrawerTransition::FocusLossHide);
        assert_eq!(buffer.text(TextField::Shell, &config), "/bin/tcsh");
        assert_eq!(buffer.slider_value(ConfigKey::Opacity, &config), 42.0);
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
            PlannedWrite::TabPosition(TabPosition::Auto),
            PlannedWrite::FontName(String::new()),
            PlannedWrite::FontSize(14),
            PlannedWrite::Opacity(100),
            PlannedWrite::HideOnFocusLoss(true),
            PlannedWrite::SessionPersistenceEnabled(true),
            PlannedWrite::SlideAnimationEnabled(true),
            PlannedWrite::BellNotificationEnabled(true),
            PlannedWrite::TabTitleMode(TabTitleMode::Shell),
            PlannedWrite::NewTabAfterCurrent(true),
            PlannedWrite::Shell(String::new()),
            PlannedWrite::Args(Vec::new()),
            PlannedWrite::ScrollbackLines(10_000),
            PlannedWrite::Layer(Layer::Overlay),
            PlannedWrite::KeyboardMode(KeyboardMode::Exclusive),
            PlannedWrite::RespectPanel(true),
            PlannedWrite::ColorSchemeDark(String::new()),
            PlannedWrite::ColorSchemeLight(String::new()),
            PlannedWrite::ThemePreference(ThemePreference::System),
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
        // `output` is not writable from the UI.
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
            (ConfigKey::TabPosition, "tab_position"),
            (ConfigKey::FontName, "font_name"),
            (ConfigKey::FontSize, "font_size"),
            (ConfigKey::Opacity, "opacity"),
            (ConfigKey::HideOnFocusLoss, "hide_on_focus_loss"),
            (
                ConfigKey::SessionPersistenceEnabled,
                "session_persistence_enabled",
            ),
            (ConfigKey::SlideAnimationEnabled, "slide_animation_enabled"),
            (
                ConfigKey::BellNotificationEnabled,
                "bell_notification_enabled",
            ),
            (ConfigKey::NewTabAfterCurrent, "new_tab_after_current"),
            (ConfigKey::Shell, "shell"),
            (ConfigKey::Args, "args"),
            (ConfigKey::ScrollbackLines, "scrollback_lines"),
            (ConfigKey::Layer, "layer"),
            (ConfigKey::KeyboardMode, "keyboard_mode"),
            (ConfigKey::RespectPanel, "respect_panel"),
            (ConfigKey::ColorSchemeDark, "color_scheme_dark"),
            (ConfigKey::ColorSchemeLight, "color_scheme_light"),
            (ConfigKey::ThemePreference, "theme_preference"),
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
        let enum_key = PlannedWrite::TabPosition(TabPosition::Bottom);
        assert_eq!(enum_key.clamped(), enum_key);
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

    // --- parsing, timing copy, and buffer semantics ---

    #[test]
    fn empty_args_text_parses_to_no_arguments() {
        assert!(parse_args("").is_empty());
        assert!(parse_args("   ").is_empty());
        assert!(parse_args("\t \n").is_empty());
    }

    #[test]
    fn args_text_splits_on_whitespace_runs() {
        assert_eq!(
            parse_args("  --login     -i\tverbose  "),
            vec![
                "--login".to_string(),
                "-i".to_string(),
                "verbose".to_string()
            ]
        );
    }

    #[test]
    fn args_text_keeps_embedded_tokens_verbatim() {
        // No quoting (the documented limitation): every token lands
        // exactly as typed, spaces being the only separator.
        assert_eq!(
            parse_args("--command=echo hi"),
            vec!["--command=echo".to_string(), "hi".to_string()]
        );
    }

    #[test]
    fn scrollback_text_parses_plain_numbers() {
        assert_eq!(parse_scrollback("10000"), Some(10_000));
        assert_eq!(parse_scrollback("  250 \n"), Some(250));
        assert_eq!(parse_scrollback("0"), Some(0));
    }

    #[test]
    fn scrollback_text_clamps_negatives_to_zero() {
        // Scrollback is >= 0 — a negative entry never persists,
        // clamping instead of rejecting so the intent ("as small as
        // possible") still lands.
        assert_eq!(parse_scrollback("-1"), Some(0));
        assert_eq!(parse_scrollback("-5000"), Some(0));
    }

    #[test]
    fn scrollback_text_rejects_garbage_and_emptiness() {
        // Nothing numeric to commit: no write at all (the draft stays
        // for the user to fix).
        assert_eq!(parse_scrollback(""), None);
        assert_eq!(parse_scrollback("   "), None);
        assert_eq!(parse_scrollback("10k"), None);
        assert_eq!(parse_scrollback("1e4"), None);
    }

    #[test]
    fn scrollback_text_clamps_above_u32_range() {
        assert_eq!(parse_scrollback("99999999999999999999"), Some(u32::MAX));
    }

    #[test]
    fn submitted_text_fields_commit_trimmed_values() {
        assert_eq!(
            submit_write(TextField::FontName, " Fira Code "),
            Some(PlannedWrite::FontName("Fira Code".to_string()))
        );
        assert_eq!(
            submit_write(TextField::Shell, " /usr/bin/fish "),
            Some(PlannedWrite::Shell("/usr/bin/fish".to_string()))
        );
    }

    #[test]
    fn submitted_shell_text_commits_empty_as_unset() {
        // An emptied shell field writes "": `resolved_shell` filters
        // it back to the $SHELL fallback — same effect as a
        // hand-edited empty value in the file.
        assert_eq!(
            submit_write(TextField::Shell, "   "),
            Some(PlannedWrite::Shell(String::new()))
        );
    }

    #[test]
    fn submitted_args_text_parses_through_the_args_rules() {
        assert_eq!(
            submit_write(TextField::Args, " --login  --noprofile "),
            Some(PlannedWrite::Args(vec![
                "--login".to_string(),
                "--noprofile".to_string()
            ]))
        );
        assert_eq!(
            submit_write(TextField::Args, ""),
            Some(PlannedWrite::Args(Vec::new()))
        );
    }

    #[test]
    fn submitted_scrollback_follows_the_scrollback_rules() {
        assert_eq!(
            submit_write(TextField::ScrollbackLines, "5000"),
            Some(PlannedWrite::ScrollbackLines(5000))
        );
        assert_eq!(
            submit_write(TextField::ScrollbackLines, "-3"),
            Some(PlannedWrite::ScrollbackLines(0))
        );
        assert_eq!(submit_write(TextField::ScrollbackLines, "lots"), None);
    }

    #[test]
    fn apply_timing_states_each_field_s_documented_semantics() {
        // The apply-timing copy, per key: geometry/font/colors/opacity
        // live; layer/keyboard_mode/respect_panel at next show;
        // shell/args/scrollback on new tabs; the toggles on their next
        // trigger.
        for key in [
            ConfigKey::HeightPercent,
            ConfigKey::WidthPercent,
            ConfigKey::HAlign,
            ConfigKey::Edge,
            ConfigKey::TabPosition,
            ConfigKey::FontName,
            ConfigKey::FontSize,
            ConfigKey::Opacity,
            ConfigKey::ColorSchemeDark,
            ConfigKey::ColorSchemeLight,
            ConfigKey::ThemePreference,
        ] {
            assert_eq!(apply_timing(key), ApplyTiming::Live, "{key:?}");
        }
        for key in [
            ConfigKey::Layer,
            ConfigKey::KeyboardMode,
            ConfigKey::RespectPanel,
        ] {
            assert_eq!(apply_timing(key), ApplyTiming::NextShow, "{key:?}");
        }
        for key in [
            ConfigKey::Shell,
            ConfigKey::Args,
            ConfigKey::ScrollbackLines,
        ] {
            assert_eq!(apply_timing(key), ApplyTiming::NewTab, "{key:?}");
        }
        for key in [
            ConfigKey::HideOnFocusLoss,
            ConfigKey::SessionPersistenceEnabled,
            ConfigKey::SlideAnimationEnabled,
            ConfigKey::NewTabAfterCurrent,
        ] {
            assert_eq!(apply_timing(key), ApplyTiming::NextTrigger, "{key:?}");
        }
    }

    #[test]
    fn apply_timing_copy_names_every_timing() {
        // The UI copy itself: each timing has its human-readable
        // label, and they are all distinct.
        let labels = [
            ApplyTiming::Live.label(),
            ApplyTiming::NewTab.label(),
            ApplyTiming::NextShow.label(),
            ApplyTiming::NextTrigger.label(),
        ];
        for pair in labels.windows(2) {
            assert_ne!(pair[0], pair[1]);
        }
        assert_eq!(ApplyTiming::NewTab.label(), "new tabs only");
        assert_eq!(ApplyTiming::NextShow.label(), "next show");
        assert_eq!(ApplyTiming::NextTrigger.label(), "next trigger");
    }

    #[test]
    fn dropdown_selections_round_trip_through_their_options() {
        let config = DropTermConfig::default();
        for key in [
            ConfigKey::HAlign,
            ConfigKey::Edge,
            ConfigKey::TabPosition,
            ConfigKey::Layer,
            ConfigKey::KeyboardMode,
            ConfigKey::ColorSchemeDark,
            ConfigKey::ColorSchemeLight,
            ConfigKey::ThemePreference,
        ] {
            let selected = dropdown_selected(key, &config)
                .unwrap_or_else(|| panic!("{key:?} has no selection"));
            let write =
                dropdown_write(key, selected).unwrap_or_else(|| panic!("{key:?} pick vanished"));
            // Re-selecting the current index reproduces the current
            // config value — the dropdown can never invent one.
            let back = dropdown_selected(key, &config).unwrap();
            assert_eq!(
                dropdown_write(key, back).unwrap(),
                write,
                "{key:?} round trip"
            );
        }
    }

    #[test]
    fn dropdown_picks_out_of_range_reject_instead_of_panicking() {
        for key in [
            ConfigKey::HAlign,
            ConfigKey::Edge,
            ConfigKey::TabPosition,
            ConfigKey::Layer,
            ConfigKey::KeyboardMode,
            ConfigKey::ColorSchemeDark,
            ConfigKey::ColorSchemeLight,
            ConfigKey::ThemePreference,
        ] {
            assert!(dropdown_write(key, 99).is_none(), "{key:?}");
        }
    }

    #[test]
    fn dropdown_writes_reject_keys_without_dropdowns() {
        assert!(dropdown_write(ConfigKey::Shell, 0).is_none());
        assert!(dropdown_write(ConfigKey::Opacity, 0).is_none());
        assert!(dropdown_selected(ConfigKey::Shell, &DropTermConfig::default()).is_none());
    }

    // --- registry-fed scheme dropdowns ---

    #[test]
    fn scheme_keys_sit_in_appearance_after_opacity_and_leave_advanced() {
        // The two scheme rows land in Appearance after Opacity;
        // Advanced keeps only the compositor-negotiation knobs.
        assert_eq!(
            Section::Appearance.keys(),
            &[
                ConfigKey::FontName,
                ConfigKey::FontSize,
                ConfigKey::Opacity,
                ConfigKey::ColorSchemeDark,
                ConfigKey::ColorSchemeLight,
                ConfigKey::ThemePreference,
            ]
        );
        assert_eq!(
            Section::Advanced.keys(),
            &[
                ConfigKey::RespectPanel,
                ConfigKey::Layer,
                ConfigKey::KeyboardMode,
            ]
        );
    }

    #[test]
    fn new_tab_after_current_sits_in_behavior_after_bell_notifications() {
        // The "New tabs after current tab" row lands after "Bell
        // notifications" (and the "Tab titles" row) in the Behavior
        // card; its flip commits the config bool through the single
        // PlannedWrite path.
        assert_eq!(
            Section::Behavior.keys(),
            &[
                ConfigKey::HideOnFocusLoss,
                ConfigKey::SessionPersistenceEnabled,
                ConfigKey::SlideAnimationEnabled,
                ConfigKey::BellNotificationEnabled,
                ConfigKey::TabTitleMode,
                ConfigKey::NewTabAfterCurrent,
            ]
        );
        assert_eq!(
            ConfigKey::NewTabAfterCurrent.label(),
            "New tabs after current tab"
        );
        assert_eq!(
            toggle_write(ConfigKey::NewTabAfterCurrent, false),
            Some(PlannedWrite::NewTabAfterCurrent(false))
        );
    }

    // --- the Appearance theme dropdown ---

    #[test]
    fn theme_preference_sits_in_appearance_after_the_scheme_rows() {
        // The "Application theme" row lands after the scheme rows.
        assert_eq!(
            Section::Appearance.keys(),
            &[
                ConfigKey::FontName,
                ConfigKey::FontSize,
                ConfigKey::Opacity,
                ConfigKey::ColorSchemeDark,
                ConfigKey::ColorSchemeLight,
                ConfigKey::ThemePreference,
            ]
        );
        assert_eq!(ConfigKey::ThemePreference.label(), "Application theme");
    }

    #[test]
    fn theme_preference_options_are_exactly_follow_system_dark_light() {
        // Follow system first, so the default config's selection is
        // index 0.
        assert_eq!(
            THEME_PREFERENCE_OPTIONS,
            &[
                (ThemePreference::System, "Follow system"),
                (ThemePreference::Dark, "Dark"),
                (ThemePreference::Light, "Light"),
            ]
        );
    }

    #[test]
    fn theme_preference_picks_round_trip_and_out_of_range_rejects() {
        // A pick commits exactly its variant — the control can never
        // invent a value; indices past the table (including exactly
        // len) reject instead of panicking.
        assert_eq!(
            dropdown_write(ConfigKey::ThemePreference, 0),
            Some(PlannedWrite::ThemePreference(ThemePreference::System))
        );
        assert_eq!(
            dropdown_write(ConfigKey::ThemePreference, 1),
            Some(PlannedWrite::ThemePreference(ThemePreference::Dark))
        );
        assert_eq!(
            dropdown_write(ConfigKey::ThemePreference, 2),
            Some(PlannedWrite::ThemePreference(ThemePreference::Light))
        );
        assert_eq!(dropdown_write(ConfigKey::ThemePreference, 3), None);
        assert_eq!(dropdown_write(ConfigKey::ThemePreference, 99), None);
    }

    #[test]
    fn theme_preference_selection_mirrors_the_config_value() {
        // Default config → Follow system → Some(0); every committed
        // variant re-selects its own position.
        let mut config = DropTermConfig::default();
        assert_eq!(
            dropdown_selected(ConfigKey::ThemePreference, &config),
            Some(0)
        );
        config.theme_preference = ThemePreference::Dark;
        assert_eq!(
            dropdown_selected(ConfigKey::ThemePreference, &config),
            Some(1)
        );
        config.theme_preference = ThemePreference::Light;
        assert_eq!(
            dropdown_selected(ConfigKey::ThemePreference, &config),
            Some(2)
        );
    }

    // --- the Geometry tab-position dropdown ---

    #[test]
    fn tab_position_sits_in_geometry_after_edge() {
        // The "Tab bar position" row lands directly after Edge in the
        // Geometry card.
        assert_eq!(
            Section::Geometry.keys(),
            &[
                ConfigKey::HeightPercent,
                ConfigKey::WidthPercent,
                ConfigKey::HAlign,
                ConfigKey::Edge,
                ConfigKey::TabPosition,
            ]
        );
        assert_eq!(ConfigKey::TabPosition.label(), "Tab bar position");
    }

    #[test]
    fn tab_position_options_are_exactly_follow_edge_top_bottom() {
        // Follow edge first, so the default config's selection is
        // index 0 (the Follow system pattern).
        assert_eq!(
            TAB_POSITION_OPTIONS,
            &[
                (TabPosition::Auto, "Follow edge"),
                (TabPosition::Top, "Top"),
                (TabPosition::Bottom, "Bottom"),
            ]
        );
    }

    #[test]
    fn tab_position_picks_round_trip_and_out_of_range_rejects() {
        // A pick commits exactly its variant — the control can never
        // invent a value; indices past the table (including exactly
        // len) reject instead of panicking.
        assert_eq!(
            dropdown_write(ConfigKey::TabPosition, 0),
            Some(PlannedWrite::TabPosition(TabPosition::Auto))
        );
        assert_eq!(
            dropdown_write(ConfigKey::TabPosition, 1),
            Some(PlannedWrite::TabPosition(TabPosition::Top))
        );
        assert_eq!(
            dropdown_write(ConfigKey::TabPosition, 2),
            Some(PlannedWrite::TabPosition(TabPosition::Bottom))
        );
        assert_eq!(dropdown_write(ConfigKey::TabPosition, 3), None);
        assert_eq!(dropdown_write(ConfigKey::TabPosition, 99), None);
    }

    #[test]
    fn tab_position_selection_mirrors_the_config_value() {
        // Default config → Auto → Some(0); every committed variant
        // re-selects its own position.
        let mut config = DropTermConfig::default();
        assert_eq!(dropdown_selected(ConfigKey::TabPosition, &config), Some(0));
        config.tab_position = TabPosition::Top;
        assert_eq!(dropdown_selected(ConfigKey::TabPosition, &config), Some(1));
        config.tab_position = TabPosition::Bottom;
        assert_eq!(dropdown_selected(ConfigKey::TabPosition, &config), Some(2));
    }

    // --- the Behavior tab-titles dropdown ---

    #[test]
    fn tab_title_mode_sits_in_behavior_after_bell_notifications() {
        // The "Tab titles" row lands after "Bell notifications" (with
        // only the "New tabs after current tab" row after it) in the
        // Behavior card, and the mode flip applies live through the
        // retitle loop.
        assert_eq!(
            Section::Behavior.keys(),
            &[
                ConfigKey::HideOnFocusLoss,
                ConfigKey::SessionPersistenceEnabled,
                ConfigKey::SlideAnimationEnabled,
                ConfigKey::BellNotificationEnabled,
                ConfigKey::TabTitleMode,
                ConfigKey::NewTabAfterCurrent,
            ]
        );
        assert_eq!(ConfigKey::TabTitleMode.label(), "Tab titles");
        assert_eq!(ConfigKey::TabTitleMode.key_name(), "tab_title_mode");
        assert_eq!(apply_timing(ConfigKey::TabTitleMode), ApplyTiming::Live);
    }

    #[test]
    fn tab_title_mode_options_are_exactly_the_four_modes() {
        // Shell title first, so the default config's selection is
        // index 0 (the Follow system pattern).
        assert_eq!(
            TAB_TITLE_MODE_OPTIONS,
            &[
                (TabTitleMode::Shell, "Shell title"),
                (TabTitleMode::AbbreviatedPath, "Abbreviated path"),
                (TabTitleMode::LastSegment, "Last segment"),
                (TabTitleMode::Numbered, "Terminal number"),
            ]
        );
    }

    #[test]
    fn tab_title_mode_picks_round_trip_and_out_of_range_rejects() {
        // A pick commits exactly its variant — the control can never
        // invent a value; indices past the table (including exactly
        // len) reject instead of panicking.
        assert_eq!(
            dropdown_write(ConfigKey::TabTitleMode, 0),
            Some(PlannedWrite::TabTitleMode(TabTitleMode::Shell))
        );
        assert_eq!(
            dropdown_write(ConfigKey::TabTitleMode, 1),
            Some(PlannedWrite::TabTitleMode(TabTitleMode::AbbreviatedPath))
        );
        assert_eq!(
            dropdown_write(ConfigKey::TabTitleMode, 2),
            Some(PlannedWrite::TabTitleMode(TabTitleMode::LastSegment))
        );
        assert_eq!(
            dropdown_write(ConfigKey::TabTitleMode, 3),
            Some(PlannedWrite::TabTitleMode(TabTitleMode::Numbered))
        );
        assert_eq!(dropdown_write(ConfigKey::TabTitleMode, 4), None);
        assert_eq!(dropdown_write(ConfigKey::TabTitleMode, 99), None);
        // The write targets exactly its own key, nothing else.
        assert_eq!(
            PlannedWrite::TabTitleMode(TabTitleMode::Numbered).key(),
            ConfigKey::TabTitleMode
        );
    }

    #[test]
    fn tab_title_mode_selection_mirrors_the_config_value() {
        // Default config → Shell → Some(0); every committed variant
        // re-selects its own position, so the dropdown opens on the
        // file's current value.
        let mut config = DropTermConfig::default();
        assert_eq!(dropdown_selected(ConfigKey::TabTitleMode, &config), Some(0));
        config.tab_title_mode = TabTitleMode::AbbreviatedPath;
        assert_eq!(dropdown_selected(ConfigKey::TabTitleMode, &config), Some(1));
        config.tab_title_mode = TabTitleMode::LastSegment;
        assert_eq!(dropdown_selected(ConfigKey::TabTitleMode, &config), Some(2));
        config.tab_title_mode = TabTitleMode::Numbered;
        assert_eq!(dropdown_selected(ConfigKey::TabTitleMode, &config), Some(3));
    }

    #[test]
    fn scheme_dropdown_options_are_the_registry_and_every_option_resolves() {
        let options = crate::terminal::theme_source::scheme_names();
        // Twelve shipped schemes, COSMIC pair first — the picker's
        // order is the registry's order.
        assert_eq!(options.len(), 12);
        assert_eq!(options.first(), Some(&crate::config::COSMIC_THEME_DARK));
        assert_eq!(options.get(1), Some(&crate::config::COSMIC_THEME_LIGHT));
        // What the picker offers is exactly what `by_name` serves, so
        // a pick can never name a scheme lookup would reject.
        for name in &options {
            assert!(
                crate::terminal::theme_source::by_name(name).is_some(),
                "{name:?} does not resolve"
            );
        }
    }

    #[test]
    fn scheme_dropdown_picks_commit_registry_names_only() {
        // A pick commits exactly the option's name — the control can
        // never invent or misspell a value.
        assert_eq!(
            dropdown_write(ConfigKey::ColorSchemeDark, 0),
            Some(PlannedWrite::ColorSchemeDark("COSMIC Dark".to_string()))
        );
        assert_eq!(
            dropdown_write(ConfigKey::ColorSchemeLight, 4),
            Some(PlannedWrite::ColorSchemeLight("Dracula".to_string()))
        );
        // Out of range (including exactly len) rejects instead of
        // panicking — no index past the table commits anything.
        let len = crate::terminal::theme_source::scheme_names().len();
        assert_eq!(dropdown_write(ConfigKey::ColorSchemeDark, len), None);
    }

    #[test]
    fn scheme_selection_opens_on_the_config_name_and_unknowns_select_nothing() {
        let mut config = DropTermConfig::default();
        // Defaults are the COSMIC pair — the registry's first two
        // entries — so both dropdowns open on a real selection.
        assert_eq!(
            dropdown_selected(ConfigKey::ColorSchemeDark, &config),
            Some(0)
        );
        assert_eq!(
            dropdown_selected(ConfigKey::ColorSchemeLight, &config),
            Some(1)
        );
        // A committed name selects its registry position.
        config.color_scheme_dark = "Dracula".to_string();
        assert_eq!(
            dropdown_selected(ConfigKey::ColorSchemeDark, &config),
            Some(4)
        );
        // An unknown name (stale, hand-typed, renamed upstream) shows
        // no selection — the control never invents one, while
        // `active_colors` keeps serving the COSMIC fallback.
        config.color_scheme_dark = "Tokyo Night".to_string();
        assert_eq!(dropdown_selected(ConfigKey::ColorSchemeDark, &config), None);
    }

    #[test]
    fn slider_release_commits_the_ranged_keys_clamped() {
        // A stale or malformed buffered slider value can never
        // persist out of range.
        assert_eq!(
            slider_release_write(ConfigKey::HeightPercent, 250.0),
            Some(PlannedWrite::HeightPercent(MAX_HEIGHT_PERCENT))
        );
        assert_eq!(
            slider_release_write(ConfigKey::WidthPercent, 1.0),
            Some(PlannedWrite::WidthPercent(MIN_WIDTH_PERCENT))
        );
        assert_eq!(
            slider_release_write(ConfigKey::Opacity, 300.0),
            Some(PlannedWrite::Opacity(100))
        );
        assert_eq!(
            slider_release_write(ConfigKey::FontSize, 13.6),
            Some(PlannedWrite::FontSize(14))
        );
    }

    #[test]
    fn slider_release_rejects_keys_without_sliders() {
        assert!(slider_release_write(ConfigKey::Shell, 1.0).is_none());
        assert!(slider_release_write(ConfigKey::Layer, 0.0).is_none());
    }

    #[test]
    fn buffer_text_falls_back_to_the_config_value() {
        let config = DropTermConfig::default();
        let buffer = SettingsBuffer::default();
        assert_eq!(buffer.text(TextField::FontName, &config), "Noto Sans Mono");
        assert_eq!(buffer.text(TextField::ScrollbackLines, &config), "10000");
    }

    #[test]
    fn buffer_text_draft_wins_until_it_is_cleared() {
        let config = DropTermConfig::default();
        let mut buffer = SettingsBuffer::default();
        buffer.set_text(TextField::FontName, "JetBrains Mono".to_string());
        assert_eq!(buffer.text(TextField::FontName, &config), "JetBrains Mono");
        buffer.clear_text(TextField::FontName);
        assert_eq!(buffer.text(TextField::FontName, &config), "Noto Sans Mono");
    }

    #[test]
    fn buffer_slider_value_falls_back_to_the_config_value() {
        let config = DropTermConfig::default();
        let mut buffer = SettingsBuffer::default();
        assert_eq!(buffer.slider_value(ConfigKey::HeightPercent, &config), 40.0);
        buffer.set_slider(ConfigKey::HeightPercent, 77.0);
        assert_eq!(buffer.slider_value(ConfigKey::HeightPercent, &config), 77.0);
    }

    #[test]
    fn clearing_the_buffer_restores_full_config_fallback() {
        // Reopen freshness: nothing a previous window session
        // buffered survives into a reopened window.
        let config = DropTermConfig::default();
        let mut buffer = SettingsBuffer::default();
        buffer.set_text(TextField::Shell, "/bin/tcsh".to_string());
        buffer.set_slider(ConfigKey::Opacity, 12.0);
        buffer.clear();
        assert_eq!(buffer.text(TextField::Shell, &config), "");
        assert_eq!(buffer.slider_value(ConfigKey::Opacity, &config), 100.0);
    }

    #[test]
    fn buffer_shell_text_shows_empty_for_an_unconfigured_shell() {
        // The *placeholder* carries resolved_shell(); the input's own
        // content stays faithful to the file — empty means unset.
        let config = DropTermConfig::default();
        assert_eq!(
            SettingsBuffer::default().text(TextField::Shell, &config),
            ""
        );
    }

    // --- the Profiles page's pure seams (profile-settings T1) ---

    fn profile(name: &str, command: &str) -> Profile {
        Profile {
            name: name.to_string(),
            command: command.to_string(),
            cwd: None,
        }
    }

    #[test]
    fn name_unique_rejects_an_exact_duplicate_and_only_an_exact_one() {
        // Name matching is exact and case-sensitive (the model's id
        // rule): "SSH" does not collide with "ssh".
        let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
        assert!(!name_unique(&profiles, None, "ssh"));
        assert!(name_unique(&profiles, None, "SSH"));
        assert!(name_unique(&profiles, None, "new"));
    }

    #[test]
    fn name_unique_excludes_the_edited_profile_itself() {
        // Re-committing a profile's own name is not a collision (the
        // self-exclusion on edit); any *other* holder still blocks.
        let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
        assert!(name_unique(&profiles, Some(0), "ssh"));
        assert!(!name_unique(&profiles, Some(0), "top"));
    }

    #[test]
    fn default_after_rename_follows_a_renamed_default() {
        // The cascade: renaming the profile `default_profile` names
        // re-points the key in the same interaction — a paired write.
        assert_eq!(
            default_after_rename(Some("ssh"), "ssh", "server"),
            Some("server".to_string())
        );
    }

    #[test]
    fn default_after_rename_leaves_every_other_rename_alone() {
        // Non-default renames and an unset default write nothing
        // extra — no paired write outside the cascade.
        assert_eq!(
            default_after_rename(Some("top"), "ssh", "server"),
            Some("top".to_string())
        );
        assert_eq!(default_after_rename(None, "ssh", "server"), None);
    }

    #[test]
    fn default_after_delete_clears_when_it_named_the_deleted_profile() {
        // The owner's auto-clear ruling: the delete proceeds and the
        // key clears — a write must never dangle.
        assert_eq!(default_after_delete(Some("ssh"), "ssh"), None);
    }

    #[test]
    fn default_after_delete_keeps_a_differently_named_default() {
        assert_eq!(
            default_after_delete(Some("top"), "ssh"),
            Some("top".to_string())
        );
        assert_eq!(default_after_delete(None, "ssh"), None);
    }

    #[test]
    fn clone_name_seeds_the_copy_suffix() {
        let profiles = [profile("web-prod", "ssh web-prod")];
        assert_eq!(clone_name(&profiles, &profiles[0]), "web-prod (copy)");
    }

    #[test]
    fn clone_name_escalates_the_suffix_on_collision() {
        // "(copy)" taken → "(copy 2)", then "(copy 3)" — the seed is
        // always unique against the list it will join.
        let profiles = [
            profile("web-prod", "ssh web-prod"),
            profile("web-prod (copy)", "ssh web-prod"),
        ];
        assert_eq!(
            clone_name(&profiles, &profiles[0]),
            "web-prod (copy 2)".to_string()
        );
        let deeper = [
            profile("web-prod", "ssh web-prod"),
            profile("web-prod (copy)", "ssh web-prod"),
            profile("web-prod (copy 2)", "ssh web-prod"),
        ];
        assert_eq!(clone_name(&deeper, &deeper[0]), "web-prod (copy 3)");
    }

    #[test]
    fn add_profile_appends_at_the_end() {
        // Config order is the `+` dropdown / context-menu order, so
        // the list transform keeps append semantics explicit.
        let profiles = [profile("ssh", "ssh web-prod")];
        let added = add_profile(&profiles, profile("top", "htop"));
        assert_eq!(added.len(), 2);
        assert_eq!(added[1].name, "top");
    }

    #[test]
    fn apply_edit_replaces_in_place_and_ignores_a_stale_index() {
        let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
        let edited = apply_edit(&profiles, 1, profile("monitor", "btop"));
        assert_eq!(edited[0].name, "ssh");
        assert_eq!(edited[1].name, "monitor");
        assert_eq!(edited.len(), 2);
        // An out-of-bounds index (config shrank under a stale click)
        // is a pure no-op, never a panic.
        assert_eq!(apply_edit(&profiles, 9, profile("ghost", "true")), profiles);
    }

    #[test]
    fn remove_profile_drops_only_the_index_and_ignores_a_stale_one() {
        let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
        let removed = remove_profile(&profiles, 0);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].name, "top");
        assert_eq!(remove_profile(&profiles, 9), profiles);
    }

    #[test]
    fn move_profile_swaps_exactly_one_slot_per_press() {
        let profiles = [
            profile("a", "true"),
            profile("b", "true"),
            profile("c", "true"),
        ];
        let up = move_profile(&profiles, 1, ReorderDirection::Up);
        assert_eq!(
            up.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["b", "a", "c"]
        );
        let down = move_profile(&profiles, 1, ReorderDirection::Down);
        assert_eq!(
            down.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["a", "c", "b"]
        );
    }

    #[test]
    fn move_profile_is_a_no_op_at_the_boundaries_and_beyond() {
        // Boundary rows have no out-of-bounds neighbor: the transform
        // is a pure no-op (the view disables the direction instead).
        let profiles = [profile("a", "true"), profile("b", "true")];
        assert_eq!(move_profile(&profiles, 0, ReorderDirection::Up), profiles);
        assert_eq!(move_profile(&profiles, 1, ReorderDirection::Down), profiles);
        assert_eq!(move_profile(&profiles, 9, ReorderDirection::Up), profiles);
    }

    #[test]
    fn parse_cwd_clears_empty_text_to_none() {
        assert_eq!(parse_cwd(""), None);
        assert_eq!(parse_cwd("   "), None);
    }

    #[test]
    fn parse_cwd_accepts_a_path_without_existence_checks() {
        // The model's define-time rule: a profile may be written
        // before its directory exists; spawn-time fallback is
        // `profile-launch`'s.
        assert_eq!(
            parse_cwd("/dropterm/profile-settings/not/yet/created"),
            Some(PathBuf::from("/dropterm/profile-settings/not/yet/created"))
        );
        assert_eq!(parse_cwd(" ~/src "), Some(PathBuf::from("~/src")));
    }

    #[test]
    fn draft_valid_rejects_blank_and_duplicate_names() {
        let profiles = [profile("ssh", "ssh web-prod")];
        assert!(!draft_valid(&profiles, None, "   ", "htop"));
        assert!(!draft_valid(&profiles, None, "ssh", "htop"));
        // The duplicate check is self-excluded on edit.
        assert!(draft_valid(&profiles, Some(0), "ssh", "htop"));
    }

    #[test]
    fn draft_valid_rejects_a_command_line_without_a_program() {
        let profiles = [profile("ssh", "ssh web-prod")];
        assert!(!draft_valid(&profiles, None, "top", ""));
        assert!(!draft_valid(&profiles, None, "top", "   "));
    }

    #[test]
    fn draft_valid_accepts_a_complete_triple() {
        let profiles = [profile("ssh", "ssh web-prod")];
        assert!(draft_valid(&profiles, None, "top", "htop"));
    }

    // --- the Profiles page's view (construction, profile-settings T1) ---

    #[test]
    fn the_settings_entry_card_reports_the_profile_count() {
        // OQ-2's drafted composition: count as the row description.
        assert_eq!(profiles_entry_description(0), "No profiles");
        assert_eq!(profiles_entry_description(1), "1 profile");
        assert_eq!(profiles_entry_description(2), "2 profiles");
    }

    #[test]
    fn the_profiles_page_builds_in_every_read_state() {
        // Construction is the assertion: empty list (the empty state
        // with its add affordance), a populated list, a stale
        // selection index, and every draft shape — an erroring add
        // draft, a clone draft, an edit draft — must all build
        // without panicking.
        let mut config = DropTermConfig::default();
        let _ = profiles_view(&config, None, None);
        let _ = profiles_view(&config, Some(0), None);
        config.profiles = vec![profile("ssh", "ssh web-prod"), profile("top", "htop")];
        let _ = profiles_view(&config, None, None);
        let _ = profiles_view(&config, Some(1), None);
        let _ = profiles_view(&config, Some(9), None);
        let mut erroring = ProfileDraft::add();
        erroring.error = Some((ProfileField::Name, ProfileFieldError::DuplicateName));
        let _ = profiles_view(&config, None, Some(&erroring));
        let cloning = ProfileDraft::clone_of(&config.profiles[0], &config.profiles);
        let _ = profiles_view(&config, Some(0), Some(&cloning));
        let editing = ProfileDraft::edit(0, &config.profiles[0]);
        let _ = profiles_view(&config, Some(0), Some(&editing));
        // And General carries the entry card whatever the list
        // state; Advanced never does.
        let _ = general_view(&config, &SettingsBuffer::default());
        let _ = advanced_view(&config, &SettingsBuffer::default());
    }

    // --- the default-profile picker (profile-settings T2) ---

    #[test]
    fn default_picker_options_are_plain_shell_then_config_order() {
        // Row 0 is the None row; the profiles follow in `profiles`
        // Vec order — the `+` dropdown / context-menu order.
        assert_eq!(
            default_picker_options(&[]),
            vec!["None (plain shell)".to_string()]
        );
        let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
        assert_eq!(
            default_picker_options(&profiles),
            vec![
                "None (plain shell)".to_string(),
                "ssh".to_string(),
                "top".to_string()
            ]
        );
    }

    #[test]
    fn default_picker_selection_mirrors_effective_default_never_a_missing_name() {
        // Unset → row 0; a set, existing name → its row; a dangling
        // name (hand-edited) falls back to row 0 — the picker never
        // displays a name the list lacks.
        let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
        assert_eq!(default_picker_selected(&profiles, None), 0);
        assert_eq!(default_picker_selected(&profiles, Some("top")), 2);
        assert_eq!(default_picker_selected(&profiles, Some("ghost")), 0);
        assert_eq!(default_picker_selected(&[], Some("ghost")), 0);
    }

    #[test]
    fn default_picker_picks_resolve_to_the_key_value_or_reject() {
        // Row 0 commits the cleared key (`None` = plain shell); a
        // profile row commits its name; anything else is stale and
        // rejects — a pick can never invent a value.
        let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
        assert_eq!(
            default_picker_write(&profiles, 0),
            Some(ProfileWrite::DefaultProfile(None))
        );
        assert_eq!(
            default_picker_write(&profiles, 2),
            Some(ProfileWrite::DefaultProfile(Some("top".to_string())))
        );
        assert_eq!(default_picker_write(&profiles, 3), None);
        // The None row exists even on an empty list; profile rows do
        // not.
        assert_eq!(
            default_picker_write(&[], 0),
            Some(ProfileWrite::DefaultProfile(None))
        );
        assert_eq!(default_picker_write(&[], 1), None);
    }

    // --- the profile form: drafts, commits, lifecycle (T3) ---

    fn sourced_profile() -> Profile {
        Profile {
            name: "web-prod".to_string(),
            command: "ssh web-prod".to_string(),
            cwd: Some(PathBuf::from("/srv/web")),
        }
    }

    #[test]
    fn an_add_draft_opens_empty() {
        let draft = ProfileDraft::add();
        assert_eq!(draft.name, "");
        assert_eq!(draft.command, "");
        assert_eq!(draft.cwd, "");
        assert_eq!(draft.error, None);
    }

    #[test]
    fn a_clone_draft_seeds_from_the_source_with_a_unique_name() {
        // The seed is T1's clone_name output; command and cwd copy
        // verbatim; no write happens here — seeding only.
        let profiles = [sourced_profile()];
        let draft = ProfileDraft::clone_of(&profiles[0], &profiles);
        assert_eq!(draft.name, "web-prod (copy)");
        assert_eq!(draft.command, "ssh web-prod");
        assert_eq!(draft.cwd, "/srv/web");
        assert_eq!(draft.error, None);
    }

    #[test]
    fn an_edit_draft_seeds_from_the_profile() {
        let draft = ProfileDraft::edit(0, &sourced_profile());
        assert_eq!(draft.name, "web-prod");
        assert_eq!(draft.command, "ssh web-prod");
        assert_eq!(draft.cwd, "/srv/web");
        // An unset cwd seeds as empty text.
        let bare = ProfileDraft::edit(0, &profile("top", "htop"));
        assert_eq!(bare.cwd, "");
    }

    #[test]
    fn typing_into_a_draft_field_clears_the_stale_error() {
        let mut draft = ProfileDraft::add();
        draft.error = Some((ProfileField::Name, ProfileFieldError::DuplicateName));
        draft.set_field_text(ProfileField::Name, "server".to_string());
        assert_eq!(draft.error, None);
        assert_eq!(draft.name, "server");
    }

    #[test]
    fn an_add_commit_appends_only_on_a_valid_triple() {
        // Draft-gated: with a valid name but a blank command the Enter
        // is invalid on the command field — nothing is written.
        let profiles = [profile("ssh", "ssh web-prod")];
        let mut draft = ProfileDraft::add();
        draft.name = "monitor".to_string();
        assert_eq!(
            plan_profile_commit(&draft, ProfileField::Name, "monitor", &profiles, None,),
            ProfileCommitPlan::Invalid {
                field: ProfileField::Command,
                error: ProfileFieldError::BlankCommand,
            }
        );
        // The first valid commit appends — and writes no default.
        draft.command = "btop".to_string();
        let ProfileCommitPlan::Write {
            profiles: written,
            default_after_rename,
        } = plan_profile_commit(&draft, ProfileField::Command, "btop", &profiles, None)
        else {
            panic!("a valid triple must commit");
        };
        assert_eq!(written.len(), 2);
        assert_eq!(written[1].name, "monitor");
        assert_eq!(written[1].command, "btop");
        assert_eq!(default_after_rename, None);
    }

    #[test]
    fn an_add_commit_rejects_blank_and_duplicate_names_at_the_name_field() {
        let profiles = [profile("ssh", "ssh web-prod")];
        let mut draft = ProfileDraft::add();
        draft.command = "htop".to_string();
        assert_eq!(
            plan_profile_commit(&draft, ProfileField::Name, "  ", &profiles, None),
            ProfileCommitPlan::Invalid {
                field: ProfileField::Name,
                error: ProfileFieldError::BlankName,
            }
        );
        assert_eq!(
            plan_profile_commit(&draft, ProfileField::Name, "ssh", &profiles, None),
            ProfileCommitPlan::Invalid {
                field: ProfileField::Name,
                error: ProfileFieldError::DuplicateName,
            }
        );
    }

    #[test]
    fn an_add_commit_parses_the_cwd_field() {
        // The Enter'd raw text replaces the draft's cwd; empty clears
        // to None, a path lands verbatim (never existence-checked).
        let profiles = [profile("ssh", "ssh web-prod")];
        let mut draft = ProfileDraft::add();
        draft.name = "monitor".to_string();
        draft.command = "btop".to_string();
        let ProfileCommitPlan::Write {
            profiles: written, ..
        } = plan_profile_commit(&draft, ProfileField::Cwd, "/srv/mon", &profiles, None)
        else {
            panic!("expected a write plan")
        };
        assert_eq!(written[1].cwd, Some(PathBuf::from("/srv/mon")));
        let ProfileCommitPlan::Write {
            profiles: cleared, ..
        } = plan_profile_commit(&draft, ProfileField::Cwd, "   ", &profiles, None)
        else {
            panic!("expected a write plan")
        };
        assert_eq!(cleared[1].cwd, None);
    }

    #[test]
    fn an_edit_commit_rewrites_only_the_entered_field() {
        // Per-field commits: Enter on the command field writes the
        // base profile with only the command replaced — a buffered
        // but un-Enter'd name edit rides along nowhere.
        let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
        let mut draft = ProfileDraft::edit(0, &profiles[0]);
        draft.name = "garbage-unentered".to_string();
        let ProfileCommitPlan::Write {
            profiles: written,
            default_after_rename,
        } = plan_profile_commit(&draft, ProfileField::Command, "btop", &profiles, None)
        else {
            panic!("expected a write plan")
        };
        assert_eq!(written[0].name, "ssh");
        assert_eq!(written[0].command, "btop");
        assert_eq!(written[1].command, "htop");
        assert_eq!(default_after_rename, None);
    }

    #[test]
    fn a_rename_commit_cascades_only_when_it_hits_the_default() {
        // The paired write: renaming the default-named profile
        // re-points `default_profile` in the same interaction;
        // non-default renames, absent defaults, and a same-name
        // re-commit write nothing extra.
        let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
        let draft = ProfileDraft::edit(0, &profiles[0]);
        let ProfileCommitPlan::Write {
            profiles: renamed,
            default_after_rename,
        } = plan_profile_commit(&draft, ProfileField::Name, "server", &profiles, Some("ssh"))
        else {
            panic!("expected a write plan")
        };
        assert_eq!(renamed[0].name, "server");
        assert_eq!(default_after_rename, Some("server".to_string()));

        let draft = ProfileDraft::edit(0, &profiles[0]);
        let ProfileCommitPlan::Write {
            default_after_rename: non_default,
            ..
        } = plan_profile_commit(&draft, ProfileField::Name, "server", &profiles, Some("top"))
        else {
            panic!("expected a write plan")
        };
        assert_eq!(non_default, None);

        let draft = ProfileDraft::edit(0, &profiles[0]);
        let ProfileCommitPlan::Write {
            default_after_rename: absent,
            ..
        } = plan_profile_commit(&draft, ProfileField::Name, "server", &profiles, None)
        else {
            panic!("expected a write plan")
        };
        assert_eq!(absent, None);

        let draft = ProfileDraft::edit(0, &profiles[0]);
        let ProfileCommitPlan::Write {
            default_after_rename: same_name,
            ..
        } = plan_profile_commit(&draft, ProfileField::Name, "ssh", &profiles, Some("ssh"))
        else {
            panic!("expected a write plan")
        };
        assert_eq!(same_name, None);
    }

    #[test]
    fn an_edit_commit_clears_the_cwd_on_empty_text() {
        let profiles = [sourced_profile()];
        let draft = ProfileDraft::edit(0, &profiles[0]);
        let ProfileCommitPlan::Write {
            profiles: written, ..
        } = plan_profile_commit(&draft, ProfileField::Cwd, "", &profiles, None)
        else {
            panic!("expected a write plan")
        };
        assert_eq!(written[0].cwd, None);
        let ProfileCommitPlan::Write {
            profiles: repathed, ..
        } = plan_profile_commit(&draft, ProfileField::Cwd, "~/src", &profiles, None)
        else {
            panic!("expected a write plan")
        };
        assert_eq!(repathed[0].cwd, Some(PathBuf::from("~/src")));
    }

    #[test]
    fn an_edit_commit_rejects_a_duplicate_name_self_excluded() {
        let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
        let draft = ProfileDraft::edit(0, &profiles[0]);
        assert_eq!(
            plan_profile_commit(&draft, ProfileField::Name, "top", &profiles, None),
            ProfileCommitPlan::Invalid {
                field: ProfileField::Name,
                error: ProfileFieldError::DuplicateName,
            }
        );
        // The profile's own name re-commits cleanly.
        assert!(matches!(
            plan_profile_commit(&draft, ProfileField::Name, "ssh", &profiles, None),
            ProfileCommitPlan::Write { .. }
        ));
    }

    #[test]
    fn a_stale_edit_index_plans_a_drop_not_a_write() {
        // The list shrank under an open edit draft (an external config
        // edit echoed in): the planner reports the staleness — never
        // a panic, never a write.
        let profiles = [profile("ssh", "ssh web-prod")];
        let draft = ProfileDraft {
            mode: ProfileDraftMode::Edit { index: 9 },
            ..ProfileDraft::add()
        };
        assert_eq!(
            plan_profile_commit(&draft, ProfileField::Name, "server", &profiles, None),
            ProfileCommitPlan::StaleEdit
        );
    }

    #[test]
    fn a_focus_loss_hide_preserves_the_profile_draft_and_its_text() {
        // The DrawerTransition rule, page-local half: a hide keeps
        // the open draft and its buffered text for the next show.
        let mut draft = Some(ProfileDraft::add());
        draft.as_mut().unwrap().name = "half-typed".to_string();
        apply_profile_draft_transition(&mut draft, ProfileDraftTransition::Hide);
        let kept = draft.expect("the draft survives the hide");
        assert_eq!(kept.name, "half-typed");
    }

    #[test]
    fn an_explicit_close_or_page_leave_clears_the_profile_draft() {
        // One transition, two call sites (the drawer's explicit
        // close, the page's back affordance): both drop the draft —
        // a reopened page starts from config.
        let mut draft = Some(ProfileDraft::add());
        apply_profile_draft_transition(&mut draft, ProfileDraftTransition::Clear);
        assert!(draft.is_none());
        let mut draft = Some(ProfileDraft::add());
        apply_profile_draft_transition(&mut draft, ProfileDraftTransition::Clear);
        assert!(draft.is_none());
    }

    // --- row actions: delete + auto-clear, reorder (T4) ---

    #[test]
    fn a_delete_plan_removes_and_auto_clears_only_the_named_default() {
        // The owner's auto-clear ruling: deleting the profile the
        // key names clears `default_profile` in the same
        // interaction; any other delete writes nothing extra.
        let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
        let (list, paired) =
            plan_profile_delete(&profiles, 0, Some("ssh")).expect("a live index deletes");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "top");
        assert_eq!(paired, Some(ProfileWrite::DefaultProfile(None)));

        let (_, untouched) =
            plan_profile_delete(&profiles, 0, Some("top")).expect("a live index deletes");
        assert_eq!(untouched, None);

        let (_, absent) = plan_profile_delete(&profiles, 0, None).expect("a live index deletes");
        assert_eq!(absent, None);
    }

    #[test]
    fn a_stale_delete_index_plans_nothing() {
        let profiles = [profile("ssh", "ssh web-prod")];
        assert_eq!(plan_profile_delete(&profiles, 9, None), None);
    }

    #[test]
    fn a_reorder_plan_moves_one_slot_or_rejects_the_boundary() {
        // A move with a neighbor plans the swapped list; a boundary
        // press (the disabled direction) plans nothing — no
        // same-value write.
        let profiles = [
            profile("a", "true"),
            profile("b", "true"),
            profile("c", "true"),
        ];
        let moved = plan_profile_reorder(&profiles, 1, ReorderDirection::Up)
            .expect("an interior row moves");
        assert_eq!(
            moved.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["b", "a", "c"]
        );
        assert_eq!(
            plan_profile_reorder(&profiles, 0, ReorderDirection::Up),
            None
        );
        assert_eq!(
            plan_profile_reorder(&profiles, 2, ReorderDirection::Down),
            None
        );
    }

    #[test]
    fn the_selection_after_a_delete_drops_or_shifts() {
        // The deleted row's selection drops (its draft with it); a
        // later selection shifts down to keep naming the same
        // profile; earlier selections and none are untouched.
        assert_eq!(selection_after_delete(Some(1), 1), None);
        assert_eq!(selection_after_delete(Some(2), 1), Some(1));
        assert_eq!(selection_after_delete(Some(0), 1), Some(0));
        assert_eq!(selection_after_delete(None, 1), None);
    }

    #[test]
    fn an_edit_draft_after_a_delete_drops_or_shifts() {
        // An edit of the deleted row drops; a later edit shifts;
        // non-edit drafts pass through untouched.
        let later = ProfileDraft::edit(2, &profile("c", "true"));
        let shifted = draft_after_delete(Some(later), 1).expect("a later edit shifts");
        assert_eq!(shifted.mode, ProfileDraftMode::Edit { index: 1 });

        let deleted = ProfileDraft::edit(1, &profile("b", "true"));
        assert_eq!(draft_after_delete(Some(deleted), 1), None);

        let adding = ProfileDraft::add();
        let kept = draft_after_delete(Some(adding.clone()), 1).expect("an add draft survives");
        assert_eq!(kept.mode, adding.mode);
    }

    #[test]
    fn the_selection_after_a_reorder_follows_the_moved_profile() {
        // The swap trades the two involved slots' selections (moving
        // row 1 down swaps slots 1 and 2: each selection follows its
        // profile to the other slot); every other selection is
        // untouched.
        assert_eq!(
            selection_after_move(Some(1), 1, ReorderDirection::Up),
            Some(0)
        );
        assert_eq!(
            selection_after_move(Some(0), 1, ReorderDirection::Up),
            Some(1)
        );
        assert_eq!(
            selection_after_move(Some(2), 1, ReorderDirection::Down),
            Some(1)
        );
        assert_eq!(
            selection_after_move(Some(0), 1, ReorderDirection::Down),
            Some(0)
        );
        assert_eq!(selection_after_move(None, 1, ReorderDirection::Up), None);
    }
}
