// SPDX-License-Identifier: GPL-3.0-only

//! The on-disk config: the [`DropTermConfig`] entry cosmic-config
//! persists one key per file, its setting enums, and the clamping
//! that guards stale or hand-edited values.

use std::collections::BTreeMap;
use std::path::Path;

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
    /// Post a desktop notification when a command finishes while the
    /// terminal is hidden (command-notify spec) — one notification per
    /// tab per hide period, carrying the command's duration and its
    /// name when one was sampled. Defaults to `true`; `false` silences
    /// them (the frozen-duration segment and the activity marker are
    /// unaffected).
    pub command_finish_notification_enabled: bool,
    /// The minimum command duration, in seconds, that posts a
    /// finished-command notification (command-notify spec, OQ-1) — a
    /// hidden `ls` never pings. A finish exactly at the threshold
    /// counts (the gate's `>=`, OQ-3). Defaults to 5 s.
    pub command_finish_notification_after_secs: u16,
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
    /// Whether the status band renders at the surface's free edge
    /// (status-bar spec): the 28 px strip adjacent to the grab bar
    /// narrating the active terminal. Defaults to `true` — the
    /// identity feature should be seen; the Behavior toggle and
    /// `Ctrl+Shift+B` make it cheap to hide. Flips apply instantly:
    /// the band leaves the column and the terminal's pinned height
    /// grows to reclaim its strip on the config echo.
    pub status_bar_enabled: bool,
    /// The per-segment visibility switches for the status band
    /// (status-segments spec): one additive bool per segment, each
    /// its own key file, all defaulting `true` — a hidden segment
    /// is never computed (no `.git` read, no clock tick). Flips
    /// apply live on the config echo. In band order: process,
    /// command duration, working directory, git branch, profile,
    /// terminal size, tab position, zoom level, clock.
    pub status_bar_show_process: bool,
    /// The `~`-collapsed cwd segment.
    pub status_bar_show_path: bool,
    /// The git-branch segment.
    pub status_bar_show_branch: bool,
    /// The profile-name segment.
    pub status_bar_show_profile: bool,
    /// The terminal's cell dimensions (`80×24`).
    pub status_bar_show_grid: bool,
    /// The tab position (`2/5`).
    pub status_bar_show_tab: bool,
    /// The signed zoom percent.
    pub status_bar_show_zoom: bool,
    /// The wall clock (`HH:MM`).
    pub status_bar_show_clock: bool,
    /// The command-duration segment (command-lifecycle spec): the
    /// live elapsed while a foreground child runs, the frozen
    /// `(name, duration)` pair after it finishes — computed from
    /// the two foreground edges the engine already diffs, no shell
    /// integration. The toggle gates the segment and its 1 s tick
    /// alike; the engine's timestamps keep running either way, so a
    /// mid-command re-enable shows the live elapsed immediately.
    pub status_bar_show_duration: bool,
    /// The scrolled-back chip (status-scrolled spec): the
    /// viewport's lines back from the bottom — `↑12` — shown only
    /// off-bottom, its click jumping to the bottom (the band's one
    /// action chip). An in-memory grid read at view assembly; the
    /// toggle hides the chip outright.
    pub status_bar_show_scrollback: bool,
    /// Whether the tab strip renders its border hairline
    /// (bar-borders spec): a 1 px line on the strip's
    /// terminal-facing edge, in the pane-derived foreground at low
    /// alpha. Defaults to `true` — the feature ships visible (the
    /// `status_bar_enabled` precedent). Flips apply on the config
    /// echo's next view pass: pure paint, no reshape.
    pub tab_bar_border: bool,
    /// The band's twin: whether the status band renders the same
    /// hairline on its side away from the grab bar. Independent of
    /// the band's own enable — a hidden band renders nothing, and
    /// re-enabling it restores the border per this key.
    pub status_bar_border: bool,
    /// Whether fish-style autosuggestions compute (autosuggest
    /// spec): the rest of the most recent matching command from this
    /// tab's own history, as dim ghost text after the cursor; Right
    /// accepts it, Alt+Right takes one word, anything else dismisses
    /// it, and nothing executes without an accept. Defaults to
    /// `true` (the fish/Warp default-on precedent); `false` is the
    /// kill switch — zero visual difference. Flips apply live on the
    /// next update.
    pub autosuggestions_enabled: bool,
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
            command_finish_notification_enabled: true,
            command_finish_notification_after_secs: 5,
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
            status_bar_enabled: true,
            status_bar_show_process: true,
            status_bar_show_path: true,
            status_bar_show_branch: true,
            status_bar_show_profile: true,
            status_bar_show_grid: true,
            status_bar_show_tab: true,
            status_bar_show_zoom: true,
            status_bar_show_clock: true,
            status_bar_show_duration: true,
            status_bar_show_scrollback: true,
            tab_bar_border: true,
            status_bar_border: true,
            autosuggestions_enabled: true,
        }
    }
}

impl DropTermConfig {
    /// The status band's per-segment visibility, folded from the
    /// ten additive bools — the resolver's toggle set.
    pub fn status_toggles(&self) -> crate::status_bar::StatusToggles {
        crate::status_bar::StatusToggles {
            process: self.status_bar_show_process,
            duration: self.status_bar_show_duration,
            path: self.status_bar_show_path,
            branch: self.status_bar_show_branch,
            profile: self.status_bar_show_profile,
            grid: self.status_bar_show_grid,
            tab: self.status_bar_show_tab,
            zoom: self.status_bar_show_zoom,
            scrolled: self.status_bar_show_scrollback,
            clock: self.status_bar_show_clock,
        }
    }

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

    /// Resolve the shell to run: the configured shell, else `$SHELL`,
    /// else `/bin/sh` — each leg kept only if its program can exec
    /// today, so a stale or hand-edited value degrades down the chain
    /// instead of arming every spawn to fail. The skip itself is
    /// silent: resolution feeds placeholder text as well as spawns,
    /// and the spawn-side degradation carries the warning.
    pub fn resolved_shell(&self) -> String {
        self.shell
            .clone()
            .filter(|s| program_is_runnable(s))
            .or_else(|| {
                std::env::var("SHELL")
                    .ok()
                    .filter(|s| program_is_runnable(s))
            })
            .unwrap_or_else(|| "/bin/sh".to_string())
    }
}

/// Whether a command program could exec right now: a slash-bearing
/// path is checked directly, a bare name is searched on `PATH`, and
/// either way the file must carry an execute bit. Best-effort by
/// design — it lets config- and spawn-resolved programs degrade
/// before they reach a pty whose failure would kill the whole tab,
/// not predict every exec failure (races and permission oddities
/// still land in `Terminal::new`'s error log).
pub(crate) fn program_is_runnable(program: &str) -> bool {
    let runnable_file = |path: &Path| {
        use std::os::unix::fs::PermissionsExt;

        path.is_file()
            && std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
    };
    if program.contains('/') {
        runnable_file(Path::new(program))
    } else {
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths).any(|dir| runnable_file(&dir.join(program)))
        })
    }
}

#[cfg(test)]
mod tests;
