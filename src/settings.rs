// SPDX-License-Identifier: GPL-3.0-only

//! The settings window (settings-ui spec): the shared config-write
//! contract — [`WriteKind`], the control→key map, the planned,
//! range-clamped writes both writers persist through the app's
//! single write seam (U3) — plus the instant-apply window view and
//! its pure half (U4): event planning, parsing, timing copy, and the
//! display-only draft buffer. Everything except the view builder
//! itself is plain data and pure functions over it, so the write
//! contract and commit rules are headless-testable (the established
//! `config_edit_reshapes`-style seam discipline).

use std::collections::HashMap;

use cosmic::iced::Length;
use cosmic::widget;
use cosmic::{Element, theme};

use cosmic::cosmic_config::{self, ConfigSet as _};

use crate::config::{
    DropTermConfig, Edge, HAlign, KeyboardMode, Layer, MAX_HEIGHT_PERCENT, MAX_WIDTH_PERCENT,
    MIN_HEIGHT_PERCENT, MIN_WIDTH_PERCENT,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

// --- settings-ui U4: the view's pure half — parsing, per-key commit
// planning, timing copy, and the in-window buffer ---

/// Which text input a settings draft belongs to. Drafts live in
/// [`SettingsBuffer`] display-only until the input is submitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextField {
    FontName,
    Shell,
    Args,
    ScrollbackLines,
    ColorSchemeDark,
    ColorSchemeLight,
}

/// Split an `args` draft into config tokens: whitespace-separated, no
/// quoting (the spec's documented limitation — args containing spaces
/// stay config-file-only). Whitespace runs collapse; empty text is no
/// arguments.
pub fn parse_args(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(str::to_string).collect()
}

/// Parse a `scrollback_lines` draft (spec criterion 9, ≥ 0): plain
/// numbers land, negatives clamp to zero, and anything without a
/// number in it rejects (`None` — no write, the draft stays for the
/// user to fix).
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

/// The write a submitted text input commits (plan decision: text
/// inputs commit on submit, never per keystroke). `None` means the
/// draft did not parse and nothing is written.
pub fn submit_write(field: TextField, raw: &str) -> Option<PlannedWrite> {
    match field {
        TextField::FontName => Some(PlannedWrite::FontName(raw.trim().to_string())),
        // Empty shell text commits `""` — `resolved_shell` filters it
        // back to the `$SHELL` fallback, same effect as a hand-edited
        // empty value in the file.
        TextField::Shell => Some(PlannedWrite::Shell(raw.trim().to_string())),
        TextField::Args => Some(PlannedWrite::Args(parse_args(raw))),
        TextField::ScrollbackLines => parse_scrollback(raw).map(PlannedWrite::ScrollbackLines),
        TextField::ColorSchemeDark => Some(PlannedWrite::ColorSchemeDark(raw.trim().to_string())),
        TextField::ColorSchemeLight => Some(PlannedWrite::ColorSchemeLight(raw.trim().to_string())),
    }
}

/// When a field's edit takes effect (spec criterion 4) — the copy the
/// view states next to every control so instant-apply never
/// misleads.
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

/// The documented apply timing of an exposed key (spec criterion 4).
pub const fn apply_timing(key: ConfigKey) -> ApplyTiming {
    match key {
        ConfigKey::HeightPercent
        | ConfigKey::WidthPercent
        | ConfigKey::HAlign
        | ConfigKey::Edge
        | ConfigKey::FontName
        | ConfigKey::FontSize
        | ConfigKey::Opacity
        | ConfigKey::ColorSchemeDark
        | ConfigKey::ColorSchemeLight => ApplyTiming::Live,
        ConfigKey::Layer | ConfigKey::KeyboardMode => ApplyTiming::NextShow,
        ConfigKey::Shell | ConfigKey::Args | ConfigKey::ScrollbackLines => ApplyTiming::NewTab,
        ConfigKey::HideOnFocusLoss
        | ConfigKey::SessionPersistenceEnabled
        | ConfigKey::SlideAnimationEnabled => ApplyTiming::NextTrigger,
    }
}

const H_ALIGN_OPTIONS: &[(HAlign, &str)] = &[
    (HAlign::Left, "Left"),
    (HAlign::Center, "Center"),
    (HAlign::Right, "Right"),
];
const EDGE_OPTIONS: &[(Edge, &str)] = &[(Edge::Top, "Top"), (Edge::Bottom, "Bottom")];
const LAYER_OPTIONS: &[(Layer, &str)] = &[(Layer::Overlay, "Overlay"), (Layer::Top, "Top")];
const KEYBOARD_MODE_OPTIONS: &[(KeyboardMode, &str)] = &[
    (KeyboardMode::Exclusive, "Exclusive"),
    (KeyboardMode::OnDemand, "OnDemand"),
];

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
        ConfigKey::Layer => LAYER_OPTIONS
            .iter()
            .position(|(value, _)| *value == config.layer),
        ConfigKey::KeyboardMode => KEYBOARD_MODE_OPTIONS
            .iter()
            .position(|(value, _)| *value == config.keyboard_mode),
        _ => None,
    }
}

/// The write a dropdown pick commits (plan decision: dropdowns commit
/// on change). `None` when `key` has no dropdown or `index` is out of
/// its range — a pick can never invent a value.
pub fn dropdown_write(key: ConfigKey, index: usize) -> Option<PlannedWrite> {
    match key {
        ConfigKey::HAlign => H_ALIGN_OPTIONS
            .get(index)
            .map(|(value, _)| PlannedWrite::HAlign(*value)),
        ConfigKey::Edge => EDGE_OPTIONS
            .get(index)
            .map(|(value, _)| PlannedWrite::Edge(*value)),
        ConfigKey::Layer => LAYER_OPTIONS
            .get(index)
            .map(|(value, _)| PlannedWrite::Layer(*value)),
        ConfigKey::KeyboardMode => KEYBOARD_MODE_OPTIONS
            .get(index)
            .map(|(value, _)| PlannedWrite::KeyboardMode(*value)),
        _ => None,
    }
}

/// The write a slider release commits (plan decision: sliders commit
/// on release; `on_change` is display-only). Ranged keys clamp (spec
/// criterion 9) — float-to-int `as` casts saturate in Rust, so a
/// negative buffered value clamps up to the band's floor, not
/// wrap. `None` for keys without a slider.
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

/// The settings window's in-flight state (plan decision): text drafts
/// buffered only until submit, slider positions buffered for the
/// live value readout until the next change. Both are display-only —
/// the view renders from `DropTermConfig` with a draft winning over
/// the file's value while one exists — and [`SettingsBuffer::clear`]
/// on window close is what makes reopening fresh (spec criterion 8).
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
            TextField::ColorSchemeDark => config.color_scheme_dark.clone(),
            TextField::ColorSchemeLight => config.color_scheme_light.clone(),
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
}

/// An event from the settings view (settings-ui spec, U4). Events
/// carry inputs, never [`PlannedWrite`]s: the view stays dumb about
/// validity, and the app resolves each event through the pure
/// planners ([`submit_write`], [`dropdown_write`], [`toggle_write`],
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
    /// A dropdown option was picked: commit on change (plan decision).
    Picked(ConfigKey, usize),
    /// A toggler flipped: commit on change (plan decision).
    Toggled(ConfigKey, bool),
}

/// The write a toggle flip commits. `None` for keys without a toggle.
pub const fn toggle_write(key: ConfigKey, checked: bool) -> Option<PlannedWrite> {
    match key {
        ConfigKey::HideOnFocusLoss => Some(PlannedWrite::HideOnFocusLoss(checked)),
        ConfigKey::SessionPersistenceEnabled => {
            Some(PlannedWrite::SessionPersistenceEnabled(checked))
        }
        ConfigKey::SlideAnimationEnabled => Some(PlannedWrite::SlideAnimationEnabled(checked)),
        _ => None,
    }
}

impl Section {
    /// The card's heading text.
    pub const fn title(&self) -> &'static str {
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
    pub const fn label(&self) -> &'static str {
        match self {
            ConfigKey::HeightPercent => "Height",
            ConfigKey::WidthPercent => "Width",
            ConfigKey::HAlign => "Alignment",
            ConfigKey::Edge => "Edge",
            ConfigKey::FontName => "Font",
            ConfigKey::FontSize => "Font size",
            ConfigKey::Opacity => "Opacity",
            ConfigKey::HideOnFocusLoss => "Hide on focus loss",
            ConfigKey::SessionPersistenceEnabled => "Session persistence",
            ConfigKey::SlideAnimationEnabled => "Slide animation",
            ConfigKey::Shell => "Shell",
            ConfigKey::Args => "Arguments",
            ConfigKey::ScrollbackLines => "Scrollback lines",
            ConfigKey::Layer => "Layer",
            ConfigKey::KeyboardMode => "Keyboard mode",
            ConfigKey::ColorSchemeDark => "Dark color scheme",
            ConfigKey::ColorSchemeLight => "Light color scheme",
        }
    }
}

/// The font-size slider's display range. No spec range exists for
/// `font_size` (criterion 9 lists height, width, opacity, scrollback
/// only); the slider constrains what the control can produce, and
/// the write path applies no extra clamp.
const FONT_SIZE_SLIDER_MIN: f32 = 6.0;
const FONT_SIZE_SLIDER_MAX: f32 = 32.0;

/// A control's rough width, keeping the window's rows aligned.
const CONTROL_WIDTH: f32 = 260.0;

/// The settings window's view (settings-ui spec, U4): one scrollable
/// column, a card per [`Section`] (plan decision — no nav sidebar for
/// five sections). Every control renders from `config` with a
/// buffered draft winning while one exists, states its apply timing
/// (criterion 4), and emits [`SettingsEvent`]s — the window never
/// applies anything itself; commits land through the app's write seam
/// and the existing `ConfigChanged` echo.
///
/// The scrollable rides in a [`theme::Container::WindowBackground`]
/// wrapper filling the surface: libcosmic renders `view_window`
/// elements bare (no window template wraps them) and styles the
/// window background transparent (the frosted-glass design), so
/// without a base layer the desktop showed through the padding gaps
/// and the window glittered while moved — the same wrap libcosmic's
/// multi-window example gives its secondary windows.
pub fn view(config: &DropTermConfig, buffer: &SettingsBuffer) -> Element<'static, SettingsEvent> {
    let cards = Section::ALL
        .iter()
        .map(|section| section_card(*section, config, buffer));
    let content = widget::scrollable(
        widget::column(cards)
            .spacing(16)
            .padding(16)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill);
    widget::container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .class(theme::Container::WindowBackground)
        .into()
}

/// One section's card: heading plus its controls, one per exposed
/// key, in display order. (The `Vec` annotation anchors the iced
/// re-exports' `Theme` type parameter to cosmic's — with no children
/// to infer from, a bare `Column::new()` would default to iced's
/// theme and stop converting to cosmic `Element`s.)
fn section_card(
    section: Section,
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
) -> Element<'static, SettingsEvent> {
    let mut rows: Vec<Element<'static, SettingsEvent>> =
        vec![widget::text(section.title()).size(18).into()];
    for key in section.keys() {
        rows.push(control_row(*key, config, buffer));
    }
    widget::container(widget::column(rows))
        .padding(16)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

/// One labeled control row: the key's label, its apply-timing copy
/// (criterion 4), and the control itself. Each builder claims only
/// its own keys; the `expect` is totality — every exposed key has
/// exactly one control, pinned by the section-map test.
fn control_row(
    key: ConfigKey,
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
) -> Element<'static, SettingsEvent> {
    let caption = widget::column![
        widget::text(key.label()).width(Length::Fill),
        widget::text(apply_timing(key).label()).size(12),
    ]
    .spacing(2);
    let control = slider_control(key, config, buffer)
        .or_else(|| dropdown_control(key, config))
        .or_else(|| toggle_control(key, config))
        .or_else(|| text_control(key, config, buffer))
        .expect("every exposed key has exactly one control (section map)");
    widget::row![caption, control]
        .spacing(16)
        .align_y(cosmic::iced::alignment::Vertical::Center)
        .into()
}

/// A slider control (plan decision: commit on release). `on_change`
/// only buffers the position for the live readout; the release
/// commits through [`slider_release_write`], clamped.
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
    Some(
        widget::column![
            widget::text(format!("{}", value.round() as u32)).size(12),
            widget::slider(range, value, move |v| SettingsEvent::SliderMoved(key, v))
                .on_release(SettingsEvent::SliderReleased(key))
                .width(Length::Fixed(CONTROL_WIDTH)),
        ]
        .spacing(4)
        .into(),
    )
}

/// A dropdown control (plan decision: commit on change). The pick
/// resolves through [`dropdown_write`]; labels and selection come
/// from the same tables the write path reads. The widget has no
/// width setter at the pinned rev, so a container sizes it.
fn dropdown_control(
    key: ConfigKey,
    config: &DropTermConfig,
) -> Option<Element<'static, SettingsEvent>> {
    let labels: Vec<&'static str> = match key {
        ConfigKey::HAlign => H_ALIGN_OPTIONS.iter().map(|(_, label)| *label).collect(),
        ConfigKey::Edge => EDGE_OPTIONS.iter().map(|(_, label)| *label).collect(),
        ConfigKey::Layer => LAYER_OPTIONS.iter().map(|(_, label)| *label).collect(),
        ConfigKey::KeyboardMode => KEYBOARD_MODE_OPTIONS
            .iter()
            .map(|(_, label)| *label)
            .collect(),
        _ => return None,
    };
    Some(
        widget::container(widget::dropdown(
            labels,
            dropdown_selected(key, config),
            move |index| SettingsEvent::Picked(key, index),
        ))
        .width(Length::Fixed(CONTROL_WIDTH))
        .into(),
    )
}

/// A toggle control (plan decision: commit on change). The flip
/// resolves through [`toggle_write`].
fn toggle_control(
    key: ConfigKey,
    config: &DropTermConfig,
) -> Option<Element<'static, SettingsEvent>> {
    let checked = match key {
        ConfigKey::HideOnFocusLoss => config.hide_on_focus_loss,
        ConfigKey::SessionPersistenceEnabled => config.session_persistence_enabled,
        ConfigKey::SlideAnimationEnabled => config.slide_animation_enabled,
        _ => return None,
    };
    Some(
        widget::toggler(checked)
            .on_toggle(move |checked| SettingsEvent::Toggled(key, checked))
            .into(),
    )
}

/// A text input (plan decision: commit on submit — per-keystroke
/// writes would persist half-typed values and fight the echo, which
/// re-renders the input from config). The input shows the buffered
/// draft while one exists, else the file's value; the shell field's
/// placeholder carries `resolved_shell()`.
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
        ConfigKey::ColorSchemeDark => TextField::ColorSchemeDark,
        ConfigKey::ColorSchemeLight => TextField::ColorSchemeLight,
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
            .width(Length::Fixed(CONTROL_WIDTH))
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

    // --- settings-ui U4: parsing, timing copy, and buffer semantics ---

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
        // Criterion 9: scrollback >= 0 — a negative entry never
        // persists, clamping instead of rejecting so the intent
        // ("as small as possible") still lands.
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
        assert_eq!(
            submit_write(TextField::ColorSchemeDark, " Tokyo Night "),
            Some(PlannedWrite::ColorSchemeDark("Tokyo Night".to_string()))
        );
        assert_eq!(
            submit_write(TextField::ColorSchemeLight, "Tokyo Light"),
            Some(PlannedWrite::ColorSchemeLight("Tokyo Light".to_string()))
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
        // Criterion 4's copy, per key: geometry/font/colors/opacity
        // live; layer/keyboard_mode at next show; shell/args/
        // scrollback on new tabs; the toggles on their next trigger.
        for key in [
            ConfigKey::HeightPercent,
            ConfigKey::WidthPercent,
            ConfigKey::HAlign,
            ConfigKey::Edge,
            ConfigKey::FontName,
            ConfigKey::FontSize,
            ConfigKey::Opacity,
            ConfigKey::ColorSchemeDark,
            ConfigKey::ColorSchemeLight,
        ] {
            assert_eq!(apply_timing(key), ApplyTiming::Live, "{key:?}");
        }
        for key in [ConfigKey::Layer, ConfigKey::KeyboardMode] {
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
        ] {
            assert_eq!(apply_timing(key), ApplyTiming::NextTrigger, "{key:?}");
        }
    }

    #[test]
    fn apply_timing_copy_names_every_timing() {
        // The UI copy itself (criterion 4): each timing has its
        // human-readable label, and they are all distinct.
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
            ConfigKey::Layer,
            ConfigKey::KeyboardMode,
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
            ConfigKey::Layer,
            ConfigKey::KeyboardMode,
        ] {
            assert!(dropdown_write(key, 99).is_none(), "{key:?}");
        }
    }

    #[test]
    fn dropdown_writes_only_serve_the_four_enum_keys() {
        assert!(dropdown_write(ConfigKey::Shell, 0).is_none());
        assert!(dropdown_write(ConfigKey::Opacity, 0).is_none());
        assert!(dropdown_selected(ConfigKey::Shell, &DropTermConfig::default()).is_none());
    }

    #[test]
    fn slider_release_commits_the_ranged_keys_clamped() {
        // A stale or malformed buffered slider value can never
        // persist out of range (criterion 9).
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
        assert_eq!(
            buffer.text(TextField::ColorSchemeDark, &config),
            crate::config::COSMIC_THEME_DARK
        );
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
        // Reopen freshness (criterion 8): nothing a previous window
        // session buffered survives into a reopened window.
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
}
