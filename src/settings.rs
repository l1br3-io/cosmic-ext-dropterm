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

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use cosmic::cosmic_config::{self, ConfigSet as _};

use crate::config::{
    DropTermConfig, Edge, HAlign, KeyboardMode, Layer, MAX_HEIGHT_PERCENT, MAX_WIDTH_PERCENT,
    MIN_HEIGHT_PERCENT, MIN_WIDTH_PERCENT, NewTabCwd, TabPosition, TabTitleMode, ThemePreference,
};
use crate::profile::Profile;
use crate::tabs::command_line_parts;
use crate::terminal::theme_source::{SchemePreview, scheme_names, scheme_previews};

mod font_picker;
mod scheme_picker;

pub mod view;

use self::view::SettingsEvent;

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
    Window,
    Tabs,
    /// The status band's one-toggle card (status-bar spec): sits on
    /// the Behavior page right after Tabs.
    StatusBar,
    Dropdown,
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
    pub const ALL: [Section; 7] = [
        Section::Appearance,
        Section::Window,
        Section::Tabs,
        Section::StatusBar,
        Section::Dropdown,
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
            Section::Window => &[
                ConfigKey::HeightPercent,
                ConfigKey::WidthPercent,
                ConfigKey::HAlign,
                ConfigKey::Edge,
            ],
            Section::Tabs => &[
                ConfigKey::TabPosition,
                ConfigKey::TabTitleMode,
                ConfigKey::TabBarBorder,
                ConfigKey::NewTabAfterCurrent,
                ConfigKey::NewTabCwd,
                ConfigKey::CloseConfirmationEnabled,
                ConfigKey::SessionPersistenceEnabled,
            ],
            Section::StatusBar => &[
                ConfigKey::StatusBarEnabled,
                ConfigKey::StatusBarBorder,
                ConfigKey::StatusBarShowProcess,
                ConfigKey::StatusBarShowPath,
                ConfigKey::StatusBarShowBranch,
                ConfigKey::StatusBarShowProfile,
                ConfigKey::StatusBarShowGrid,
                ConfigKey::StatusBarShowTab,
                ConfigKey::StatusBarShowZoom,
                ConfigKey::StatusBarShowClock,
                ConfigKey::StatusBarShowDuration,
                ConfigKey::StatusBarShowScrollback,
            ],
            Section::Dropdown => &[ConfigKey::HideOnFocusLoss, ConfigKey::SlideAnimationEnabled],
            Section::Terminal => &[
                ConfigKey::Shell,
                ConfigKey::Args,
                ConfigKey::DefaultCwd,
                ConfigKey::ScrollbackLines,
                ConfigKey::BellNotificationEnabled,
                ConfigKey::CommandFinishNotificationEnabled,
                ConfigKey::CommandFinishNotificationAfterSecs,
                ConfigKey::AutosuggestionsEnabled,
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
/// unreachable from the UI — plus `SettingsDrawerWidth`, which is
/// written by the drawer's drag handle (drawer-resize spec), not by
/// any settings control, and `SettingsDrawerClickClose`, whose row is
/// hidden while the drawer-click-close module is parked (the
/// `context_drawer` overlay captures every surface mouse event while
/// open, so no base-tree wrapper can observe a press — see that
/// spec's fallback clause): no `Section` lists either, so the
/// exposed-key exactly-once pin above stays at the section-hosted
/// count (42 — default-spawn's two rows included).
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
    /// The finished-command notification's kill switch
    /// (command-notify spec) — the Terminal card's toggle beside the
    /// bell, honored at the next finish.
    CommandFinishNotificationEnabled,
    /// The finished-command notification's minimum duration in
    /// seconds (command-notify spec, OQ-1) — the Terminal card's
    /// numeric row, the scrollback parse shape.
    CommandFinishNotificationAfterSecs,
    /// The autosuggest kill switch (autosuggest spec) — the Terminal
    /// card's toggle beside the bell, flipping live on the echo.
    AutosuggestionsEnabled,
    TabTitleMode,
    NewTabAfterCurrent,
    NewTabCwd,
    CloseConfirmationEnabled,
    SettingsDrawerClickClose,
    Shell,
    Args,
    DefaultCwd,
    ScrollbackLines,
    Layer,
    KeyboardMode,
    RespectPanel,
    ColorSchemeDark,
    ColorSchemeLight,
    ThemePreference,
    SettingsDrawerWidth,
    StatusBarEnabled,
    /// Show the state disc + process name — one per band segment, in
    /// band order (status-segments spec); the resolver drops a
    /// hidden segment, and the app never gathers its inputs.
    StatusBarShowProcess,
    StatusBarShowPath,
    StatusBarShowBranch,
    StatusBarShowProfile,
    StatusBarShowGrid,
    StatusBarShowTab,
    StatusBarShowZoom,
    StatusBarShowClock,
    /// The command-duration segment (command-lifecycle spec): the
    /// toggle hides the segment and stands down its 1 s tick — the
    /// engine's timestamps keep running, so re-enabling mid-command
    /// shows the live elapsed immediately.
    StatusBarShowDuration,
    /// The scrolled-back chip (status-scrolled spec): the toggle
    /// hides the chip outright — the offset read rides the same
    /// view-assembly lock either way, so the flip costs nothing
    /// beyond the absent segment.
    StatusBarShowScrollback,
    /// The strip's border hairline (bar-borders spec) — one per
    /// bar, each flipping live through the single planned-write
    /// path.
    TabBarBorder,
    /// The band's border hairline — the strip key's twin.
    StatusBarBorder,
}

/// A settings drawer tab: a view-only regrouping of the same
/// sections into the everyday controls and the power controls —
/// never a second copy of the registry ([`Section::keys`], the
/// control→key map, is untouched by the grouping) — plus the
/// Profiles manager's and the Keybindings editor's tabs, which host
/// no `Section` card at all (their pages are [`view::profiles_view`] and
/// [`view::keybindings_view`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    /// How the terminal looks: the look card and the window's shape.
    Appearance,
    /// How `DropTerm` acts: the tabs, dropdown mechanics, and
    /// terminal cards.
    Behavior,
    /// The profile manager ([`view::profiles_view`]) — no sections, so it
    /// never perturbs the section partition.
    Profiles,
    /// The keybinding editor ([`view::keybindings_view`]) — no sections, so
    /// it never perturbs the section partition.
    Keybindings,
    Advanced,
}

/// The sections a tab's page hosts, in display order. Every
/// [`Section`] appears in exactly one tab and the union is
/// [`Section::ALL`] — pinned by test, so regrouping can never drop
/// or duplicate a card. The Profiles and Keybindings tabs host
/// none: their pages are the manager and the editor, not section
/// cards.
pub const fn tab_sections(tab: SettingsTab) -> &'static [Section] {
    match tab {
        SettingsTab::Appearance => &[Section::Appearance, Section::Window],
        SettingsTab::Behavior => &[
            Section::Tabs,
            Section::StatusBar,
            Section::Dropdown,
            Section::Terminal,
        ],
        SettingsTab::Profiles | SettingsTab::Keybindings => &[],
        SettingsTab::Advanced => &[Section::Advanced],
    }
}

/// The tab's strip label — hardcoded English like every label in
/// this drawer.
pub const fn tab_header(tab: SettingsTab) -> &'static str {
    match tab {
        SettingsTab::Appearance => "Appearance",
        SettingsTab::Behavior => "Behavior",
        SettingsTab::Profiles => "Profiles",
        SettingsTab::Keybindings => "Keybindings",
        SettingsTab::Advanced => "Advanced",
    }
}

impl SettingsTab {
    /// Every tab, in strip order — the strip's rows come from this
    /// list and nothing else, so no row can exist without a page to
    /// serve it.
    pub const ALL: [SettingsTab; 5] = [
        SettingsTab::Appearance,
        SettingsTab::Behavior,
        SettingsTab::Profiles,
        SettingsTab::Keybindings,
        SettingsTab::Advanced,
    ];
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
            ConfigKey::CommandFinishNotificationEnabled => "command_finish_notification_enabled",
            ConfigKey::CommandFinishNotificationAfterSecs => {
                "command_finish_notification_after_secs"
            }
            ConfigKey::AutosuggestionsEnabled => "autosuggestions_enabled",
            ConfigKey::TabTitleMode => "tab_title_mode",
            ConfigKey::NewTabAfterCurrent => "new_tab_after_current",
            ConfigKey::NewTabCwd => "new_tab_cwd",
            ConfigKey::CloseConfirmationEnabled => "close_confirmation_enabled",
            ConfigKey::SettingsDrawerClickClose => "settings_drawer_click_close",
            ConfigKey::Shell => "shell",
            ConfigKey::Args => "args",
            ConfigKey::DefaultCwd => "default_cwd",
            ConfigKey::ScrollbackLines => "scrollback_lines",
            ConfigKey::Layer => "layer",
            ConfigKey::KeyboardMode => "keyboard_mode",
            ConfigKey::RespectPanel => "respect_panel",
            ConfigKey::ColorSchemeDark => "color_scheme_dark",
            ConfigKey::ColorSchemeLight => "color_scheme_light",
            ConfigKey::ThemePreference => "theme_preference",
            ConfigKey::SettingsDrawerWidth => "settings_drawer_width",
            ConfigKey::StatusBarEnabled => "status_bar_enabled",
            ConfigKey::StatusBarShowProcess => "status_bar_show_process",
            ConfigKey::StatusBarShowPath => "status_bar_show_path",
            ConfigKey::StatusBarShowBranch => "status_bar_show_branch",
            ConfigKey::StatusBarShowProfile => "status_bar_show_profile",
            ConfigKey::StatusBarShowGrid => "status_bar_show_grid",
            ConfigKey::StatusBarShowTab => "status_bar_show_tab",
            ConfigKey::StatusBarShowZoom => "status_bar_show_zoom",
            ConfigKey::StatusBarShowClock => "status_bar_show_clock",
            ConfigKey::StatusBarShowDuration => "status_bar_show_duration",
            ConfigKey::StatusBarShowScrollback => "status_bar_show_scrollback",
            ConfigKey::TabBarBorder => "tab_bar_border",
            ConfigKey::StatusBarBorder => "status_bar_border",
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
    CommandFinishNotificationEnabled(bool),
    CommandFinishNotificationAfterSecs(u16),
    AutosuggestionsEnabled(bool),
    TabTitleMode(TabTitleMode),
    NewTabAfterCurrent(bool),
    NewTabCwd(NewTabCwd),
    CloseConfirmationEnabled(bool),
    SettingsDrawerClickClose(bool),
    Shell(String),
    Args(Vec<String>),
    /// The default directory `new_tab_cwd: Default` opens plain new
    /// tabs in (`None` clears the key back to the pty default) — the
    /// profile form's `parse_cwd` shape, `Option` on the wire like
    /// `default_profile`'s.
    DefaultCwd(Option<PathBuf>),
    ScrollbackLines(u32),
    Layer(Layer),
    KeyboardMode(KeyboardMode),
    RespectPanel(bool),
    ColorSchemeDark(String),
    ColorSchemeLight(String),
    ThemePreference(ThemePreference),
    /// The drawer pane's dragged width (drawer-resize spec): the
    /// px the drag released at, or `None` for the double-click
    /// reset back to the built-in default. Floors at
    /// [`DRAWER_MIN_WIDTH`] in `clamped`; the surface-relative cap
    /// is render-time (`drawer_width`).
    SettingsDrawerWidth(Option<u16>),
    /// Whether the status band renders (status-bar spec): the band
    /// leaves the column and the terminal's pinned height reclaims
    /// its strip on the config echo.
    StatusBarEnabled(bool),
    /// The per-segment "Show …" rows (status-segments spec): the
    /// hidden segment is absent from the next render and its inputs
    /// are never gathered — the branch read and the clock tick stop
    /// at the gate.
    StatusBarShowProcess(bool),
    StatusBarShowPath(bool),
    StatusBarShowBranch(bool),
    StatusBarShowProfile(bool),
    StatusBarShowGrid(bool),
    StatusBarShowTab(bool),
    StatusBarShowZoom(bool),
    StatusBarShowClock(bool),
    /// The duration segment's own row (command-lifecycle spec).
    StatusBarShowDuration(bool),
    /// The scrolled chip's own row (status-scrolled spec).
    StatusBarShowScrollback(bool),
    /// The bars' border hairlines (bar-borders spec): pure display
    /// flips — the echo's next view pass rebuilds each widget with
    /// its flag, no reshape anywhere.
    TabBarBorder(bool),
    /// The band's twin of the strip's border write.
    StatusBarBorder(bool),
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
            PlannedWrite::CommandFinishNotificationEnabled(_) => {
                ConfigKey::CommandFinishNotificationEnabled
            }
            PlannedWrite::CommandFinishNotificationAfterSecs(_) => {
                ConfigKey::CommandFinishNotificationAfterSecs
            }
            PlannedWrite::AutosuggestionsEnabled(_) => ConfigKey::AutosuggestionsEnabled,
            PlannedWrite::TabTitleMode(_) => ConfigKey::TabTitleMode,
            PlannedWrite::NewTabAfterCurrent(_) => ConfigKey::NewTabAfterCurrent,
            PlannedWrite::NewTabCwd(_) => ConfigKey::NewTabCwd,
            PlannedWrite::CloseConfirmationEnabled(_) => ConfigKey::CloseConfirmationEnabled,
            PlannedWrite::SettingsDrawerClickClose(_) => ConfigKey::SettingsDrawerClickClose,
            PlannedWrite::Shell(_) => ConfigKey::Shell,
            PlannedWrite::Args(_) => ConfigKey::Args,
            PlannedWrite::DefaultCwd(_) => ConfigKey::DefaultCwd,
            PlannedWrite::ScrollbackLines(_) => ConfigKey::ScrollbackLines,
            PlannedWrite::Layer(_) => ConfigKey::Layer,
            PlannedWrite::KeyboardMode(_) => ConfigKey::KeyboardMode,
            PlannedWrite::RespectPanel(_) => ConfigKey::RespectPanel,
            PlannedWrite::ColorSchemeDark(_) => ConfigKey::ColorSchemeDark,
            PlannedWrite::ColorSchemeLight(_) => ConfigKey::ColorSchemeLight,
            PlannedWrite::ThemePreference(_) => ConfigKey::ThemePreference,
            PlannedWrite::SettingsDrawerWidth(_) => ConfigKey::SettingsDrawerWidth,
            PlannedWrite::StatusBarEnabled(_) => ConfigKey::StatusBarEnabled,
            PlannedWrite::StatusBarShowProcess(_) => ConfigKey::StatusBarShowProcess,
            PlannedWrite::StatusBarShowPath(_) => ConfigKey::StatusBarShowPath,
            PlannedWrite::StatusBarShowBranch(_) => ConfigKey::StatusBarShowBranch,
            PlannedWrite::StatusBarShowProfile(_) => ConfigKey::StatusBarShowProfile,
            PlannedWrite::StatusBarShowGrid(_) => ConfigKey::StatusBarShowGrid,
            PlannedWrite::StatusBarShowTab(_) => ConfigKey::StatusBarShowTab,
            PlannedWrite::StatusBarShowZoom(_) => ConfigKey::StatusBarShowZoom,
            PlannedWrite::StatusBarShowClock(_) => ConfigKey::StatusBarShowClock,
            PlannedWrite::StatusBarShowDuration(_) => ConfigKey::StatusBarShowDuration,
            PlannedWrite::StatusBarShowScrollback(_) => ConfigKey::StatusBarShowScrollback,
            PlannedWrite::TabBarBorder(_) => ConfigKey::TabBarBorder,
            PlannedWrite::StatusBarBorder(_) => ConfigKey::StatusBarBorder,
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
            // Floor only — the cap needs the surface width the
            // render clamp owns; `None` (reset) passes through.
            PlannedWrite::SettingsDrawerWidth(raw) => {
                PlannedWrite::SettingsDrawerWidth(raw.map(|px| px.max(DRAWER_MIN_WIDTH)))
            }
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
            PlannedWrite::CommandFinishNotificationEnabled(value) => handler.set(
                ConfigKey::CommandFinishNotificationEnabled.key_name(),
                *value,
            ),
            PlannedWrite::CommandFinishNotificationAfterSecs(value) => handler.set(
                ConfigKey::CommandFinishNotificationAfterSecs.key_name(),
                *value,
            ),
            PlannedWrite::AutosuggestionsEnabled(value) => {
                handler.set(ConfigKey::AutosuggestionsEnabled.key_name(), *value)
            }
            PlannedWrite::TabTitleMode(value) => {
                handler.set(ConfigKey::TabTitleMode.key_name(), value)
            }
            PlannedWrite::NewTabAfterCurrent(value) => {
                handler.set(ConfigKey::NewTabAfterCurrent.key_name(), *value)
            }
            PlannedWrite::NewTabCwd(value) => handler.set(ConfigKey::NewTabCwd.key_name(), value),
            PlannedWrite::CloseConfirmationEnabled(value) => {
                handler.set(ConfigKey::CloseConfirmationEnabled.key_name(), *value)
            }
            PlannedWrite::SettingsDrawerClickClose(value) => {
                handler.set(ConfigKey::SettingsDrawerClickClose.key_name(), *value)
            }
            PlannedWrite::Shell(value) => handler.set(ConfigKey::Shell.key_name(), value),
            PlannedWrite::Args(value) => handler.set(ConfigKey::Args.key_name(), value),
            // `Option<PathBuf>` serializes like `default_profile`'s
            // `Option<String>` — absent/None merges cleanly with a
            // hand-edited file.
            PlannedWrite::DefaultCwd(value) => handler.set(ConfigKey::DefaultCwd.key_name(), value),
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
            // `Option<u16>` serializes like `default_profile`'s
            // `Option<String>` — absent/None merges cleanly with a
            // hand-edited file.
            PlannedWrite::SettingsDrawerWidth(value) => {
                handler.set(ConfigKey::SettingsDrawerWidth.key_name(), value)
            }
            PlannedWrite::StatusBarEnabled(value) => {
                handler.set(ConfigKey::StatusBarEnabled.key_name(), *value)
            }
            PlannedWrite::StatusBarShowProcess(value) => {
                handler.set(ConfigKey::StatusBarShowProcess.key_name(), *value)
            }
            PlannedWrite::StatusBarShowPath(value) => {
                handler.set(ConfigKey::StatusBarShowPath.key_name(), *value)
            }
            PlannedWrite::StatusBarShowBranch(value) => {
                handler.set(ConfigKey::StatusBarShowBranch.key_name(), *value)
            }
            PlannedWrite::StatusBarShowProfile(value) => {
                handler.set(ConfigKey::StatusBarShowProfile.key_name(), *value)
            }
            PlannedWrite::StatusBarShowGrid(value) => {
                handler.set(ConfigKey::StatusBarShowGrid.key_name(), *value)
            }
            PlannedWrite::StatusBarShowTab(value) => {
                handler.set(ConfigKey::StatusBarShowTab.key_name(), *value)
            }
            PlannedWrite::StatusBarShowZoom(value) => {
                handler.set(ConfigKey::StatusBarShowZoom.key_name(), *value)
            }
            PlannedWrite::StatusBarShowClock(value) => {
                handler.set(ConfigKey::StatusBarShowClock.key_name(), *value)
            }
            PlannedWrite::StatusBarShowDuration(value) => {
                handler.set(ConfigKey::StatusBarShowDuration.key_name(), *value)
            }
            PlannedWrite::StatusBarShowScrollback(value) => {
                handler.set(ConfigKey::StatusBarShowScrollback.key_name(), *value)
            }
            PlannedWrite::TabBarBorder(value) => {
                handler.set(ConfigKey::TabBarBorder.key_name(), *value)
            }
            PlannedWrite::StatusBarBorder(value) => {
                handler.set(ConfigKey::StatusBarBorder.key_name(), *value)
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
/// [`SettingsBuffer`] display-only until the input is submitted. The
/// font control is a picker now (font-picker spec): `FontName` has no
/// text field, its write rides `font_pick_write`'s rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextField {
    Shell,
    Args,
    ScrollbackLines,
    CommandFinishNotificationAfterSecs,
    DefaultCwd,
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

/// Parse a `command_finish_notification_after_secs` draft
/// (command-notify spec): scrollback's parse shape over `u16` —
/// plain numbers land, negatives clamp to zero (every finish
/// notifies), an absurd entry saturates, and anything without a
/// number in it rejects (`None` — no write, the draft stays).
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to [0, u16::MAX] first, so the narrowing to u16 is lossless by construction"
)]
pub fn parse_command_notify_secs(raw: &str) -> Option<u16> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    match raw.parse::<i128>() {
        Ok(n) => Some(n.clamp(0, i128::from(u16::MAX)) as u16),
        Err(_) => None,
    }
}

/// The write a submitted text input commits (text inputs commit on
/// submit, never per keystroke). `None` means the draft did not parse
/// and nothing is written.
pub fn submit_write(field: TextField, raw: &str) -> Option<PlannedWrite> {
    match field {
        // Empty shell text commits `""` — `resolved_shell` filters it
        // back to the `$SHELL` fallback, same effect as a hand-edited
        // empty value in the file.
        TextField::Shell => Some(PlannedWrite::Shell(raw.trim().to_string())),
        TextField::Args => Some(PlannedWrite::Args(parse_args(raw))),
        TextField::ScrollbackLines => parse_scrollback(raw).map(PlannedWrite::ScrollbackLines),
        TextField::CommandFinishNotificationAfterSecs => {
            parse_command_notify_secs(raw).map(PlannedWrite::CommandFinishNotificationAfterSecs)
        }
        // The profile form's `parse_cwd` rule: empty clears to `None`
        // (the pty default), any other path lands verbatim — a cwd can
        // never reject.
        TextField::DefaultCwd => Some(PlannedWrite::DefaultCwd(parse_cwd(raw))),
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
        // The drag handle's key — not a settings control, but its
        // echo re-renders the pane immediately: live by
        // construction. The status band's keys join it: the band
        // leaves or joins the column on the config echo (the
        // terminal's pinned height follows the same frame), and the
        // eight "Show …" rows drop or restore their segment on the
        // next render — with the hidden segment's inputs never
        // gathered.
        | ConfigKey::SettingsDrawerWidth
        | ConfigKey::StatusBarEnabled
        | ConfigKey::TabBarBorder
        | ConfigKey::StatusBarBorder
        | ConfigKey::StatusBarShowProcess
        | ConfigKey::StatusBarShowPath
        | ConfigKey::StatusBarShowBranch
        | ConfigKey::StatusBarShowProfile
        | ConfigKey::StatusBarShowGrid
        | ConfigKey::StatusBarShowTab
        | ConfigKey::StatusBarShowZoom
        | ConfigKey::StatusBarShowClock
        | ConfigKey::StatusBarShowDuration
        | ConfigKey::StatusBarShowScrollback
        // A showing suggestion disappears the moment the kill switch
        // lands: the echo runs set_config, the next update's guard
        // chain yields None, and re-enabling suggests again at the
        // next typed prefix (autosuggest spec).
        | ConfigKey::AutosuggestionsEnabled
        | ConfigKey::ThemePreference => ApplyTiming::Live,
        ConfigKey::Layer | ConfigKey::KeyboardMode | ConfigKey::RespectPanel => {
            ApplyTiming::NextShow
        }
        ConfigKey::Shell
        | ConfigKey::Args
        | ConfigKey::ScrollbackLines
        | ConfigKey::NewTabCwd
        | ConfigKey::DefaultCwd => ApplyTiming::NewTab,
        ConfigKey::HideOnFocusLoss
        | ConfigKey::SessionPersistenceEnabled
        | ConfigKey::SlideAnimationEnabled
        | ConfigKey::BellNotificationEnabled
        | ConfigKey::CommandFinishNotificationEnabled
        | ConfigKey::CommandFinishNotificationAfterSecs
        | ConfigKey::NewTabAfterCurrent
        | ConfigKey::CloseConfirmationEnabled
        | ConfigKey::SettingsDrawerClickClose => ApplyTiming::NextTrigger,
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
    (TabTitleMode::Profile, "Profile name"),
];
const NEW_TAB_CWD_OPTIONS: &[(NewTabCwd, &str)] = &[
    (NewTabCwd::Inherit, "Inherit active tab"),
    (NewTabCwd::Default, "Default directory"),
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
        ConfigKey::NewTabCwd => NEW_TAB_CWD_OPTIONS
            .iter()
            .position(|(value, _)| *value == config.new_tab_cwd),
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
        ConfigKey::NewTabCwd => NEW_TAB_CWD_OPTIONS
            .get(index)
            .map(|(value, _)| PlannedWrite::NewTabCwd(*value)),
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
            TextField::Shell => config.shell.clone().unwrap_or_default(),
            TextField::Args => config.args.join(" "),
            TextField::ScrollbackLines => config.scrollback_lines.to_string(),
            TextField::CommandFinishNotificationAfterSecs => {
                config.command_finish_notification_after_secs.to_string()
            }
            // The profile form's `cwd_text` shape: the path verbatim,
            // empty for `None`.
            TextField::DefaultCwd => config
                .default_cwd
                .as_ref()
                .map_or_else(String::new, |path| path.display().to_string()),
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

/// Parse a font-size override draft: empty (or whitespace-only) text
/// clears to `None` — inherit the global size — and any other text
/// parses as the number it must be for Save to have passed the band
/// check. `None` is also the floor for text that does not parse:
/// Save's gate rejects that first, so this arm never carries a
/// planned write.
fn parse_profile_font_size(raw: &str) -> Option<u16> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    raw.parse().ok()
}

// --- the profile form: draft, transactional save, lifecycle ---

/// One of the form's text inputs: the three whole-profile fields,
/// plus the env rows' key and value inputs at their row index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileField {
    /// The profile's name — also its id.
    Name,
    /// The command line to run.
    Command,
    /// The optional working directory (raw path text).
    Cwd,
    /// The scheme pick. No text input exists — the arm is where a
    /// rejected Save's scheme error lives (under the picker).
    Scheme,
    /// The font-size override input (raw text; empty = inherit).
    FontSize,
    /// The scrollback-override input (raw text; empty = inherit).
    Scrollback,
    /// Env row `index`'s key input. Also where a rejected Save's row
    /// error renders — the key is the row's name, so the error
    /// belongs under it.
    EnvKey(usize),
    /// Env row `index`'s value input.
    EnvValue(usize),
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
    /// The scheme name is not one of the built-in registry's.
    UnknownScheme,
    /// The font size falls outside the global band ([`FONT_SIZE_MIN`]
    /// – [`FONT_SIZE_MAX`]) or is not a whole number.
    FontOutOfRange,
    /// The scrollback text has no number in it (the parse's clamp
    /// rules absorb everything else, like the global input).
    UnparsableScrollback,
    /// An env row carries a value but no key.
    BlankEnvKey,
    /// An env row's key contains `=` — a character environ entry
    /// names cannot carry; splitting a pair is the shell's job.
    EnvKeyEquals,
}

impl ProfileFieldError {
    /// The inline error copy shown under the field.
    pub const fn message(self) -> &'static str {
        match self {
            Self::BlankName => "Name is required",
            Self::DuplicateName => "That name is already taken",
            Self::BlankCommand => "Command needs a program to run",
            Self::UnknownScheme => "Not one of the built-in schemes",
            Self::FontOutOfRange => "Font size must be between 6 and 32",
            Self::UnparsableScrollback => "Scrollback must be a number",
            Self::BlankEnvKey => "Variable needs a key",
            Self::EnvKeyEquals => "Keys can't contain '='",
        }
    }
}

/// One env row's buffered text: the key input and the value input.
/// Rows commit as one map — an Enter in either input writes the
/// whole list through [`env_map_from_rows`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileEnvRow {
    /// The key input's buffered text.
    pub key: String,
    /// The value input's buffered text.
    pub value: String,
}

/// The draft rows a stored map seeds: one row per entry, in the
/// map's `BTree` order — the same profile opens the same form every
/// time. `None` (no variables) seeds no rows.
pub fn env_rows_from_map(map: Option<&BTreeMap<String, String>>) -> Vec<ProfileEnvRow> {
    map.map(|entries| {
        entries
            .iter()
            .map(|(key, value)| ProfileEnvRow {
                key: key.clone(),
                value: value.clone(),
            })
            .collect()
    })
    .unwrap_or_default()
}

/// The env map a write lands from the draft's rows. Wholly blank
/// rows drop silently (an abandoned row holds nothing); keys and
/// values trim (the form's uniform write rule — interior characters
/// are verbatim, `=` in a value included); a repeated key keeps its
/// last row (`BTreeMap` insert — the editor offers discrete rows, so
/// duplicates are a non-concern, ruling 1). Rows whose trimmed key is
/// empty or `=`-bearing also drop: [`env_row_errors`] rejects them
/// at Save for the user, and this floor keeps any path that
/// somehow skips the gate from landing a broken environ entry in the
/// store. An empty result is `None` — "no variables" is the field's
/// unset shape, never an empty map.
pub fn env_map_from_rows(rows: &[ProfileEnvRow]) -> Option<BTreeMap<String, String>> {
    let map: BTreeMap<String, String> = rows
        .iter()
        .filter(|row| {
            let key = row.key.trim();
            !key.is_empty() && !key.contains('=')
        })
        .map(|row| (row.key.trim().to_string(), row.value.trim().to_string()))
        .collect();
    (!map.is_empty()).then_some(map)
}

/// Every row that could not enter a map — each as its field-and-
/// error pair, in row order — for the whole-draft gate: a Save
/// rejects with every offending row at once, where the per-field
/// commits stopped at the first. Wholly blank
/// rows never reject (the transform drops them); an empty value is
/// legitimate (`FOO=` is a real variable).
pub fn env_row_errors(rows: &[ProfileEnvRow]) -> Vec<(ProfileField, ProfileFieldError)> {
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let key = row.key.trim();
            if key.is_empty() {
                (!row.value.trim().is_empty())
                    .then_some((ProfileField::EnvKey(index), ProfileFieldError::BlankEnvKey))
            } else if key.contains('=') {
                Some((ProfileField::EnvKey(index), ProfileFieldError::EnvKeyEquals))
            } else {
                None
            }
        })
        .collect()
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

/// The form's one-at-a-time draft: the six raw fields (name,
/// command, cwd, scheme, font size, scrollback — the last two as
/// text, empty = inherit), the env rows, plus the per-field error
/// state of the last rejected commit. Not a [`SettingsBuffer`]
/// extension — the page owns its lifecycle (see
/// [`ProfileDraftTransition`]), and [`TextField`]'s arms stay pinned
/// to the Settings page's keys.
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
    /// The scheme dropdown's buffered pick: `None` = follow the
    /// global scheme. The pick buffers in every mode — the recolor
    /// lands on Save with the rest of the draft (a profile's running
    /// tabs take it when the written config echoes).
    pub scheme: Option<String>,
    /// The font-size input's buffered text: empty = inherit the
    /// global size; any other text must parse into the global band
    /// at Save.
    pub font_size: String,
    /// The scrollback input's buffered text: empty = inherit the
    /// global capacity; any other text must parse at Save.
    pub scrollback: String,
    /// The env rows' buffered text ([`env_rows_from_map`] seeds them
    /// from the profile). The whole row list rides Save
    /// ([`env_map_from_rows`]); add/remove are buffered list edits,
    /// not writes — the form has no gesture but Save that writes.
    pub env_rows: Vec<ProfileEnvRow>,
    /// Every offending field from the last rejected Save — one pair
    /// per input, all rendered at once, each under its own field.
    pub errors: Vec<(ProfileField, ProfileFieldError)>,
}

impl ProfileDraft {
    /// The Add button's empty draft.
    pub fn add() -> Self {
        Self {
            mode: ProfileDraftMode::Add,
            name: String::new(),
            command: String::new(),
            cwd: String::new(),
            scheme: None,
            font_size: String::new(),
            scrollback: String::new(),
            env_rows: Vec::new(),
            errors: Vec::new(),
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
            scheme: source.color_scheme.clone(),
            font_size: override_text(source.font_size),
            scrollback: override_text(source.scrollback_lines),
            env_rows: env_rows_from_map(source.env.as_ref()),
            errors: Vec::new(),
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
            scheme: profile.color_scheme.clone(),
            font_size: override_text(profile.font_size),
            scrollback: override_text(profile.scrollback_lines),
            env_rows: env_rows_from_map(profile.env.as_ref()),
            errors: Vec::new(),
        }
    }

    /// Buffer a field's text (a keystroke): display-only, and that
    /// field's stale error clears — it described text that no longer
    /// exists. The other fields' errors stay: they describe text
    /// that still does.
    pub fn set_field_text(&mut self, field: ProfileField, text: String) {
        self.errors.retain(|(at, _)| *at != field);
        match field {
            ProfileField::Name => self.name = text,
            ProfileField::Command => self.command = text,
            ProfileField::Cwd => self.cwd = text,
            // No text input buffers the scheme — the pick's own
            // handler clears a stale scheme error when it re-picks.
            ProfileField::Scheme => {}
            ProfileField::FontSize => self.font_size = text,
            ProfileField::Scrollback => self.scrollback = text,
            ProfileField::EnvKey(index) => {
                if let Some(row) = self.env_rows.get_mut(index) {
                    row.key = text;
                }
            }
            ProfileField::EnvValue(index) => {
                if let Some(row) = self.env_rows.get_mut(index) {
                    row.value = text;
                }
            }
        }
    }
}

/// The override input's text for a profile's set override: the
/// number verbatim, empty for `None` (inherit) — the seeding half of
/// empty-means-inherit, so an untouched Edit draft round-trips.
fn override_text<T: std::fmt::Display>(override_value: Option<T>) -> String {
    override_value.map_or_else(String::new, |value| value.to_string())
}

/// The cwd input's text for a profile's cwd: the path verbatim, empty
/// for `None`.
fn cwd_text(profile: &Profile) -> String {
    profile
        .cwd
        .as_ref()
        .map_or_else(String::new, |path| path.display().to_string())
}

// --- the transactional save: one write per Save, whole-draft
// validation (profile-form-commit) ---

/// What a Save of the whole draft produced: the write to land, the
/// rejections to show, the empty transaction, or the stale-edit
/// drop — the per-field commits' plan vocabulary, evaluated over
/// the whole draft at once.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileSavePlan {
    /// A valid save: one whole-list write, plus the paired
    /// `default_profile` write when the save renamed the
    /// default-named profile ([`default_after_rename`] — `None`
    /// means no paired write).
    Write {
        /// The new `profiles` list.
        profiles: Vec<Profile>,
        /// The rename cascade's new default, if the save hit it.
        default_after_rename: Option<String>,
    },
    /// An invalid save: every offending field and why, one pair per
    /// input. Nothing is written; the draft keeps its text and every
    /// error renders under its own field simultaneously.
    Invalid {
        /// Each offending field with its rejection reason.
        errors: Vec<(ProfileField, ProfileFieldError)>,
    },
    /// An Edit draft whose fields all compare equal against the
    /// committed profile: the transaction is empty — no write, the
    /// draft simply closes (the `echo_retitles_tabs` anti-churn
    /// idiom).
    Unchanged,
    /// The edited profile is gone (the list changed under the open
    /// draft): nothing to save — the draft is stale and drops.
    StaleEdit,
}

/// Plan the Save a whole draft produces — the pure heart of the
/// transactional form.
///
/// Every rule the form knows evaluates at once — the name (blank,
/// then unique), the command line ([`command_line_parts`]), the
/// scheme name (registry), the font size (the global band), the
/// scrollback text ([`parse_scrollback`]), and every env row
/// ([`env_row_errors`]) — so a rejection names every
/// offending field, not the first. A valid Edit replaces its profile
/// with the whole draft and cascades `default_profile` iff the
/// rename hit the default-named profile; a valid Add/Clone appends
/// (the clone seed already derives its unique name). An Edit that
/// compares equal against the committed profile plans no write at
/// all.
pub fn plan_profile_save(
    draft: &ProfileDraft,
    profiles: &[Profile],
    default: Option<&str>,
) -> ProfileSavePlan {
    match &draft.mode {
        ProfileDraftMode::Edit { index } => {
            let Some(base) = profiles.get(*index) else {
                return ProfileSavePlan::StaleEdit;
            };
            let errors = whole_draft_errors(profiles, draft, Some(*index));
            if !errors.is_empty() {
                return ProfileSavePlan::Invalid { errors };
            }
            let mut edited = base.clone();
            edited.name = draft.name.trim().to_string();
            edited.command = draft.command.trim().to_string();
            edited.cwd = parse_cwd(&draft.cwd);
            edited.color_scheme.clone_from(&draft.scheme);
            edited.font_size = parse_profile_font_size(&draft.font_size);
            edited.scrollback_lines = parse_scrollback(&draft.scrollback);
            edited.env = env_map_from_rows(&draft.env_rows);
            if edited == *base {
                return ProfileSavePlan::Unchanged;
            }
            // The cascade: only a real rename of the default-named
            // profile re-points the key (a same-name save is not a
            // rename), and only when the key's value actually
            // changes — an unchanged default writes nothing extra.
            let default_after_rename = if edited.name == base.name {
                None
            } else {
                match default_after_rename(default, &base.name, &edited.name) {
                    Some(new_default) if Some(new_default.as_str()) != default => Some(new_default),
                    _ => None,
                }
            };
            ProfileSavePlan::Write {
                profiles: apply_edit(profiles, *index, edited),
                default_after_rename,
            }
        }
        ProfileDraftMode::Add | ProfileDraftMode::Clone { .. } => {
            let errors = whole_draft_errors(profiles, draft, None);
            if !errors.is_empty() {
                return ProfileSavePlan::Invalid { errors };
            }
            let added = Profile {
                name: draft.name.trim().to_string(),
                command: draft.command.trim().to_string(),
                cwd: parse_cwd(&draft.cwd),
                color_scheme: draft.scheme.clone(),
                env: env_map_from_rows(&draft.env_rows),
                // The override text is the draft's own in every mode:
                // seeding copied the source's values into a Clone,
                // so the round-trip preserves them without a
                // mode-special case here.
                font_size: parse_profile_font_size(&draft.font_size),
                scrollback_lines: parse_scrollback(&draft.scrollback),
            };
            ProfileSavePlan::Write {
                profiles: add_profile(profiles, added),
                default_after_rename: None,
            }
        }
    }
}

/// The whole-draft gate: every form rule evaluated at once — the
/// name (blank, then unique), the command line, the scheme name
/// (registry), the font size (the global band), the scrollback
/// text, and every env row — so one Save rejects with every
/// offending field, in form order.
fn whole_draft_errors(
    profiles: &[Profile],
    draft: &ProfileDraft,
    editing: Option<usize>,
) -> Vec<(ProfileField, ProfileFieldError)> {
    let mut errors = Vec::new();
    let name = draft.name.trim();
    if name.is_empty() {
        errors.push((ProfileField::Name, ProfileFieldError::BlankName));
    } else if !name_unique(profiles, editing, name) {
        errors.push((ProfileField::Name, ProfileFieldError::DuplicateName));
    }
    if command_line_parts(draft.command.trim()).is_none() {
        errors.push((ProfileField::Command, ProfileFieldError::BlankCommand));
    }
    if draft
        .scheme
        .as_deref()
        .is_some_and(|name| scheme_index(name).is_none())
    {
        errors.push((ProfileField::Scheme, ProfileFieldError::UnknownScheme));
    }
    let font = draft.font_size.trim();
    if !font.is_empty()
        && !font
            .parse::<u16>()
            .is_ok_and(|size| (FONT_SIZE_MIN..=FONT_SIZE_MAX).contains(&size))
    {
        errors.push((ProfileField::FontSize, ProfileFieldError::FontOutOfRange));
    }
    if !draft.scrollback.trim().is_empty() && parse_scrollback(&draft.scrollback).is_none() {
        errors.push((
            ProfileField::Scrollback,
            ProfileFieldError::UnparsableScrollback,
        ));
    }
    errors.extend(env_row_errors(&draft.env_rows));
    errors
}

/// The selection after a Save lands: an Edit keeps the row it
/// rewrote; an Add/Clone selects the row the append created —
/// `profiles_len` is the pre-append length (the Save's own write
/// owns the +1). A `Write` plan guarantees the index is live.
pub const fn selection_after_save(mode: &ProfileDraftMode, profiles_len: usize) -> usize {
    match mode {
        ProfileDraftMode::Edit { index } => *index,
        ProfileDraftMode::Add | ProfileDraftMode::Clone { .. } => profiles_len,
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

/// One press of a row's delete cluster — the armed-delete state
/// machine's whole vocabulary. `Arm` is the row's Delete press (the
/// row then renders its inline Confirm/Keep pair); `Confirm` deletes
/// through [`plan_profile_delete`] (default auto-clear intact);
/// `Keep` disarms without deleting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileDeletePress {
    /// The row's Delete press: arm this row's inline pair.
    Arm(usize),
    /// The armed row's Confirm press: delete it.
    Confirm(usize),
    /// The armed row's Keep press: disarm, delete nothing.
    Keep,
}

/// The armed row after `press` — the armed-delete state machine's
/// one pure transition: arming a row arms exactly that row (the
/// pair renders inline on one row, so arming another row disarms the
/// first), and both ends of the pair clear the arm (Confirm's delete
/// lands through [`plan_profile_delete`] in the same interaction).
pub const fn armed_profile_delete_row(press: ProfileDeletePress) -> Option<usize> {
    match press {
        ProfileDeletePress::Arm(index) => Some(index),
        ProfileDeletePress::Confirm(_) | ProfileDeletePress::Keep => None,
    }
}

/// Whether `event` may act on the Profiles page while a profile
/// draft is open — the draft lock: no list mutation and no draft
/// switch is reachable while a draft is open, so the draft's origin
/// index cannot shift under it (the stale-index class dies at the
/// root instead of by index bookkeeping). The locked view renders
/// the list's controls disabled; the wiring consults this rule so a
/// stale press — a race between a render and a click — is a no-op
/// even if a control flashed enabled. Pure over the event/state
/// pair, headless-pinned.
pub fn profile_event_reachable(event: &SettingsEvent, draft_open: bool) -> bool {
    if !draft_open {
        return true;
    }
    !matches!(
        event,
        SettingsEvent::SelectProfile(_)
            | SettingsEvent::OpenProfileDraft
            | SettingsEvent::CloneProfile(_)
            | SettingsEvent::ArmProfileDelete(_)
            | SettingsEvent::ConfirmProfileDelete(_)
            | SettingsEvent::KeepProfileDelete
            | SettingsEvent::ReorderProfile(..)
    )
}

/// The plan a delete produces: the list without the profile at
/// `index`, plus the paired `default_profile` write when the deleted
/// profile was the one the key named — the owner's auto-clear ruling
/// (a write must never dangle; the picker can never show a stale
/// name). A stale out-of-bounds index plans nothing. This is the
/// armed row's Confirm step — the delete lands immediately from
/// Confirm, never a dialog.
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
/// the 23 exposed keys never grow a 24th — close-confirm and
/// drawer-click-close since grew the count to 25, default-spawn's
/// two rows to 27, the band's per-segment Show rows to 35.
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

/// The list row [`crate::profile::effective_default`] resolves to —
/// the row the page's `default` tag marks. `None` when no default
/// resolves (unset, or dangling: read-side tolerance, no rewrite) —
/// the picker's row 0, plain shell.
pub fn effective_default_row(profiles: &[Profile], default: Option<&str>) -> Option<usize> {
    // The position is found by name (not pointer identity) against
    // the same list `effective_default` resolved it from, so it
    // always exists.
    crate::profile::effective_default(profiles, default)
        .and_then(|resolved| profiles.iter().position(|p| p.name == resolved.name))
}

/// The picker's selection row: the row
/// [`crate::profile::effective_default`] resolves to — row 0 when no
/// default resolves (unset, or dangling: the picker never displays a
/// name the list lacks; read-side tolerance, no rewrite). Always a
/// row; the caller's `Some` is the dropdown API's shape.
pub fn default_picker_selected(profiles: &[Profile], default: Option<&str>) -> usize {
    // Row 0 is the None row; a resolved default's list row follows.
    effective_default_row(profiles, default).map_or(0, |position| 1 + position)
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

// --- the profile form's scheme picker: the shared swatch popover
// with a leading Inherit row ---

/// Which scheme picker the app holds open — the two global controls
/// (Appearance) and the profile form's override picker (Profiles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemePickerId {
    /// A global scheme control — dark or light.
    Global(ConfigKey),
    /// The profile form's override picker.
    ProfileForm,
}

/// Which picker the app holds open — the scheme pickers (the two
/// global Appearance controls and the profile form's override picker)
/// and the Appearance card's font picker — so the one-open-picker
/// rule and the Escape ordering (popover first, then draft, then
/// drawer) cover every popover through one flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerId {
    /// A scheme picker — a global Appearance control or the profile
    /// form's override picker.
    Scheme(SchemePickerId),
    /// The Appearance card's font picker.
    Font,
}

/// The Inherit row's swatch: the global scheme the terminal renders
/// for the current system mode — the dark/light pair resolved for
/// `is_dark`, the same resolution the terminal's color chain applies
/// — previewed as the popover rows preview themselves. `None` when
/// the configured global name is one the registry lacks (the closed
/// control's unknown-name rule: no strip, never an invented
/// palette).
pub fn inherit_scheme_preview(config: &DropTermConfig, is_dark: bool) -> Option<SchemePreview> {
    let name = crate::profile::effective_scheme_name(
        None,
        &config.color_scheme_dark,
        &config.color_scheme_light,
        is_dark,
    );
    scheme_preview_by_name(name)
}

/// The registry preview a name resolves to, `None` for a name the
/// registry lacks — the picker's single find-by-name seam (the
/// Inherit row's resolution and a picked scheme's strip share it).
fn scheme_preview_by_name(name: &str) -> Option<SchemePreview> {
    scheme_previews()
        .iter()
        .find(|(option, _)| *option == name)
        .map(|(_, preview)| *preview)
}

/// The picker's selection row for a draft's scheme: row 0 (Inherit)
/// when unset — or when the stored name is unknown (a hand-edited
/// config; the control shows the fallback the resolution chain is
/// already serving, never a value it invents) — and the name's
/// option row when set. Mirrors [`default_picker_selected`].
pub fn profile_scheme_selected(scheme: Option<&str>) -> usize {
    scheme
        .and_then(|name| scheme_index(name).map(|position| position + 1))
        .unwrap_or(0)
}

/// What a picker row resolves to: the cleared field, a registry
/// name, or nothing for an out-of-range row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileSchemePick {
    /// Row 0 (Inherit): unset the field — follow the global scheme.
    None,
    /// A registry row: the name to set.
    Scheme(String),
}

impl ProfileSchemePick {
    /// The `color_scheme` field value the pick writes.
    pub fn field_value(self) -> Option<String> {
        match self {
            Self::None => None,
            Self::Scheme(name) => Some(name),
        }
    }
}

/// The field value a picker row commits: row 0 (Inherit) clears to
/// `None` — follow the global pick — and a registry row sets its
/// name. Any other index is stale and rejects — no write, never an
/// invented value (the [`dropdown_write`] rule).
pub fn profile_scheme_write(index: usize) -> Option<ProfileSchemePick> {
    if index == 0 {
        return Some(ProfileSchemePick::None);
    }
    scheme_names()
        .get(index - 1)
        .map(|name| ProfileSchemePick::Scheme((*name).to_string()))
}

/// The fallback rows offered when `/etc/shells` cannot be read (or
/// parses to nothing): the conventional quartet, static by ruling 1 —
/// a picker is always offered, whatever the file's state.
const SHELL_FALLBACK: &[&str] = &["/bin/bash", "/bin/zsh", "/bin/fish", "/bin/sh"];

/// A picker row's label: the path's basename (`zsh`), the whole path
/// when it has no final component. Basenames only, no suffixes —
/// OQ-1's default ruling.
pub fn shell_label(path: &str) -> String {
    Path::new(path).file_name().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The shell picker's options from `/etc/shells` content (`None` =
/// unreadable): one path per non-comment line, deduped by basename
/// (first occurrence wins — `/bin/sh` keeps its file-order priority
/// over `/usr/bin/sh`), then filtered to entries that exist on this
/// system. Unreadable or barren content degrades to the static
/// fallback quartet — never an empty dropdown. Pure over the content
/// string; the existence check is the one machine-dependent leg.
pub fn shell_picker_options(content: Option<&str>) -> Vec<String> {
    let Some(content) = content else {
        return SHELL_FALLBACK
            .iter()
            .map(|path| (*path).to_string())
            .collect();
    };
    let mut basenames: Vec<String> = Vec::new();
    let mut options: Vec<String> = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let basename = shell_label(line);
        if basenames.contains(&basename) {
            continue;
        }
        if !Path::new(line).exists() {
            continue;
        }
        basenames.push(basename);
        options.push(line.to_string());
    }
    if options.is_empty() {
        return SHELL_FALLBACK
            .iter()
            .map(|path| (*path).to_string())
            .collect();
    }
    options
}

/// The write a shell-dropdown pick commits: the option's full path
/// under the existing `shell` key. An index past the list (the file
/// changed under the open dropdown) rejects — the `dropdown_write`
/// rule, dynamic-table edition.
pub fn shell_dropdown_write(options: &[String], index: usize) -> Option<PlannedWrite> {
    options
        .get(index)
        .map(|path| PlannedWrite::Shell(path.clone()))
}

/// The stored shell's dropdown row: the option carrying the exact
/// path. `None` for an off-list value — unset, a custom path, a stale
/// name — so the free-text row remains its editor and the control
/// never invents a selection.
pub fn shell_dropdown_selected(options: &[String], stored: Option<&str>) -> Option<usize> {
    stored.and_then(|path| options.iter().position(|option| option == path))
}

/// `/etc/shells` content, `None` when the file cannot be read — the
/// picker's fallback trigger. Options are built per view (ruling 1),
/// so the view and the pick handler each read it fresh.
fn etc_shells_content() -> Option<String> {
    std::fs::read_to_string("/etc/shells").ok()
}

/// The update-path half of a shell-picker pick: rebuild the per-view
/// options from `/etc/shells` and map the row through
/// [`shell_dropdown_write`]. The rebuild is the point — a stale index
/// against a changed file rejects instead of writing the wrong
/// shell.
pub fn shell_pick_write(index: usize) -> Option<PlannedWrite> {
    shell_dropdown_write(
        &shell_picker_options(etc_shells_content().as_deref()),
        index,
    )
}

// --- the font picker's planner trio: the shell dropdown's
// dynamic-list pattern over the app-owned family scan ---

/// The picker's rows under a stored `font_name`: the families in
/// list order, preceded by the stored name itself when no family
/// matches — a hand-edited or uninstalled value stays visible and
/// selectable, never an empty picker.
pub fn font_picker_rows(families: &[String], stored: &str) -> Vec<String> {
    if families.iter().any(|family| family == stored) {
        return families.to_vec();
    }
    // The synthetic leader: the stored value's own row.
    let mut rows = Vec::with_capacity(families.len() + 1);
    rows.push(stored.to_string());
    rows.extend(families.iter().cloned());
    rows
}

/// The picker's selection row for a stored `font_name`: the matching
/// family's row, else the synthetic leading row — the stored value is
/// always the selected one. The match is exact and case-sensitive
/// (`Family::Name`'s own match is exact; a case variant is a
/// different family).
pub fn font_picker_selected(families: &[String], stored: &str) -> usize {
    // A match selects its family's row (no leader exists to offset
    // past); anything else selects the leader at 0 — one expression
    // covers both row spaces.
    families
        .iter()
        .position(|family| family == stored)
        .unwrap_or(0)
}

/// The write a font-picker pick commits: the synthetic leading row
/// commits the stored name itself, a family row its family. An index
/// past the rows (the system's list changed under the open popover)
/// rejects — the [`dropdown_write`] rule, [`shell_pick_write`]'s
/// dynamic-list edition over the app-owned family scan.
pub fn font_pick_write(families: &[String], stored: &str, index: usize) -> Option<PlannedWrite> {
    font_picker_rows(families, stored)
        .get(index)
        .map(|family| PlannedWrite::FontName(family.clone()))
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
        ConfigKey::CommandFinishNotificationEnabled => {
            Some(PlannedWrite::CommandFinishNotificationEnabled(checked))
        }
        ConfigKey::AutosuggestionsEnabled => Some(PlannedWrite::AutosuggestionsEnabled(checked)),
        ConfigKey::NewTabAfterCurrent => Some(PlannedWrite::NewTabAfterCurrent(checked)),
        ConfigKey::CloseConfirmationEnabled => {
            Some(PlannedWrite::CloseConfirmationEnabled(checked))
        }
        ConfigKey::SettingsDrawerClickClose => {
            Some(PlannedWrite::SettingsDrawerClickClose(checked))
        }
        ConfigKey::RespectPanel => Some(PlannedWrite::RespectPanel(checked)),
        ConfigKey::StatusBarEnabled => Some(PlannedWrite::StatusBarEnabled(checked)),
        ConfigKey::TabBarBorder => Some(PlannedWrite::TabBarBorder(checked)),
        ConfigKey::StatusBarBorder => Some(PlannedWrite::StatusBarBorder(checked)),
        ConfigKey::StatusBarShowProcess => Some(PlannedWrite::StatusBarShowProcess(checked)),
        ConfigKey::StatusBarShowPath => Some(PlannedWrite::StatusBarShowPath(checked)),
        ConfigKey::StatusBarShowBranch => Some(PlannedWrite::StatusBarShowBranch(checked)),
        ConfigKey::StatusBarShowProfile => Some(PlannedWrite::StatusBarShowProfile(checked)),
        ConfigKey::StatusBarShowGrid => Some(PlannedWrite::StatusBarShowGrid(checked)),
        ConfigKey::StatusBarShowTab => Some(PlannedWrite::StatusBarShowTab(checked)),
        ConfigKey::StatusBarShowZoom => Some(PlannedWrite::StatusBarShowZoom(checked)),
        ConfigKey::StatusBarShowClock => Some(PlannedWrite::StatusBarShowClock(checked)),
        ConfigKey::StatusBarShowDuration => Some(PlannedWrite::StatusBarShowDuration(checked)),
        ConfigKey::StatusBarShowScrollback => Some(PlannedWrite::StatusBarShowScrollback(checked)),
        _ => None,
    }
}

/// The font-size band every font input shares: the global slider's
/// range, which the profile form's Save enforces on its override
/// text as "the global range" the profile-manager spec names.
pub const FONT_SIZE_MIN: u16 = 6;
pub const FONT_SIZE_MAX: u16 = 32;

/// The drawer pane's default width in px (spec drawer-resize
/// criterion 1): what a never-dragged drawer renders — `None` in
/// the config — wherever the surface is wide enough that half of it
/// reaches this. Deliberately above libcosmic's 480px framework
/// band; the drawer-resize initiative widened it.
pub const DEFAULT_DRAWER_WIDTH: u16 = 560;

/// The drawer pane's minimum width in px: the settings content's
/// floor, carried over from the framework band the settings-drawer
/// initiative sized against.
pub const DRAWER_MIN_WIDTH: u16 = 344;

/// The drawer pane's maximum width as a fraction of the surface
/// width (spec criterion 3): the drawer never covers more than half
/// the drop-down, on any output.
pub const WIDTH_CAP_FRACTION: f32 = 0.5;

/// The drawer pane's width in px from the user's stored preference
/// and the current surface width (spec criterion 4): `None` (never
/// dragged) renders the wider default; any stored value clamps into
/// [344px, 50% of surface] so a stale or hand-edited config can
/// never overflow the surface or crush the settings content. This
/// render-time clamp is the one place width policy lives — the
/// write path only floors, because the cap needs the surface width
/// this pure seam never sees.
pub fn drawer_width(preferred: Option<u16>, surface_width: f32) -> f32 {
    let preferred = preferred.map_or(f32::from(DEFAULT_DRAWER_WIDTH), f32::from);
    drawer_width_clamp(preferred, surface_width)
}

/// The band every width passes through — render's stored/default
/// path and the drag's live path share it, so a live drag can never
/// leave the band render would clamp back into anyway.
fn drawer_width_clamp(preferred: f32, surface_width: f32) -> f32 {
    let cap = (surface_width * WIDTH_CAP_FRACTION).max(f32::from(DRAWER_MIN_WIDTH));
    preferred.clamp(f32::from(DRAWER_MIN_WIDTH), cap)
}

/// The drawer drag's live width from the pointer's window-absolute
/// x (spec criterion 2): the pane is pinned to the surface's right
/// edge, so its width is the pointer's distance from that edge —
/// rounded to whole px and clamped through the shared band. The
/// `surface_width - x` target can be negative on a fling past the
/// edge; the band floors it, never a panic.
pub fn drawer_drag_px(pointer_x: f32, surface_width: f32) -> u16 {
    px_u16(drawer_width_clamp(surface_width - pointer_x, surface_width))
}

/// Round a banded px value into the `u16` domain the live state and
/// the config field share. Safe by construction: callers clamp
/// below `u16::MAX` first (the band's cap is half a surface width).
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "rounded and clamped into the u16 band before the cast"
)]
fn px_u16(px: f32) -> u16 {
    px.round() as u16
}

/// The write a drag release commits (spec criterion 3): only a real
/// drag writes — `None` (a plain click armed no live value) and a
/// drag that lands back on the stored px both commit nothing, so
/// the file never churns without a change.
pub fn drawer_release_write(live: Option<u16>, stored: Option<u16>) -> Option<PlannedWrite> {
    live.filter(|&px| Some(px) != stored)
        .map(|px| PlannedWrite::SettingsDrawerWidth(Some(px)))
}

#[cfg(test)]
mod tests;
