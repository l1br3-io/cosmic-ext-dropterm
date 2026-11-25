// SPDX-License-Identifier: GPL-3.0-only

//! The on-disk config: the [`DropTermConfig`] model — itself the
//! store's settings section — with the profiles and keybinds
//! sections beside it (one plain-RON file per writer, see
//! [`store`]), its setting enums, and the clamping that guards stale
//! or hand-edited values.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::keybinds::Binding;
use crate::core::profile::Profile;

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
/// `Top` — the default — sits below fullscreen windows but tends to behave
/// better with input methods; `Overlay` sits above them (see the README's
/// known-risks section).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layer {
    Overlay,
    #[default]
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
    /// A plugin script derives the title (tab-titles spec): the last
    /// script-resolved answer, else the `Terminal N` default — never a
    /// block, never a spinner. The answer lands on the tick's drain
    /// (the shell asks through the Lua host), so an unanswered or
    /// raised-against tab keeps its prior title; a user rename still
    /// wins, as in every mode.
    Script,
}

impl std::fmt::Display for TabTitleMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shell => write!(f, "shell"),
            Self::AbbreviatedPath => write!(f, "abbreviated_path"),
            Self::LastSegment => write!(f, "last_segment"),
            Self::Numbered => write!(f, "numbered"),
            Self::Profile => write!(f, "profile"),
            Self::Script => write!(f, "script"),
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

/// Who owns the prompt line (rich-input spec): `Native` — the
/// default — is the bare shell, every keystroke forwarded to the PTY
/// verbatim and the whole suggest lane dark; `Rich` is the editor:
/// at a live prompt `DropTerm` buffers, edits, highlights, completes,
/// and executes the line. Per-profile through
/// [`Profile::input_mode`](crate::core::profile::Profile::input_mode),
/// `None` following the global pick (the `color_scheme` pattern).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputMode {
    #[default]
    Native,
    Rich,
}

/// Where the rich-mode input strip docks (rich-input spec, OQ-7):
/// `Bottom` — the default — sits at the grid's foot where the prompt
/// is; `Top` pins the strip at the head of the grid region while the
/// grid keeps scrolling normally beneath it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputPosition {
    #[default]
    Bottom,
    Top,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// Whole-struct serde is the store's settings section (`config.ron`):
// a key the file lacks loads as its default, so the key list stays
// additive across versions — an older file simply lacks the newer
// keys. The three keys owned by the other two sections are skipped
// below and overlaid by `load_from`.
#[serde(default)]
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
    /// The minimum number of tabs before the strip renders tab rows
    /// (kitty's own knob, name and default): `2` — the default —
    /// keeps the single tab's row hidden, the strip carrying only its
    /// corner affordances (`+`, gear); `1` keeps that row on, so the
    /// one tab reads, renames, and closes from the strip. The strip
    /// itself never hides either way, and rows always render from the
    /// minimum on. Pure paint: flips apply on the config echo's next
    /// view pass.
    pub tab_bar_min_tabs: u16,
    /// Layer-shell layer the surface is created on.
    pub layer: Layer,
    /// Keyboard interactivity mode requested for the surface. While
    /// `hide_on_focus_loss` is `false` the shown surface always runs
    /// `OnDemand` regardless (the `surface` module's
    /// `shown_keyboard_mode` resolution): cosmic-comp's focus fixup
    /// hands the keyboard back to an exclusive layer surface on every
    /// refresh cycle, so `Exclusive` held by a stay-open surface
    /// would stop any other input from ever keeping focus.
    pub keyboard_mode: KeyboardMode,
    /// Place the drop-down below panels and docks instead of covering them,
    /// by requesting exclusive zone `0` (see docs/research/panel-visibility.md).
    /// Applies on the next show. Defaults to `true`.
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
    /// the overlay open when it loses focus — it then shares keyboard
    /// focus like a normal window (click away to type elsewhere,
    /// click the terminal to take focus back) instead of holding an
    /// exclusive grab.
    pub hide_on_focus_loss: bool,
    /// Save the session on change and restore it on launch.
    /// Defaults to `true` — always-on persistence. `false` is
    /// symmetric: saves stop and launch restore is skipped, while the
    /// state file itself is left untouched to wait for a re-enable.
    pub session_persistence_enabled: bool,
    /// Animate show/hide with the 150 ms slide. Defaults to `true`.
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
    /// Whether the Settings window and the drop-down run as
    /// independent windows instead of the exclusive pair
    /// (settings-window SW2–SW4). Defaults to `false` — the shipped
    /// pairing: opening Settings explicitly hides the drop-down, a
    /// summon supersedes the open Settings window, and a user close
    /// returns the drop-down. `true` severs all three: Settings
    /// opens over whatever the drop-down is doing, a summon leaves
    /// the Settings window standing, and a close returns nothing (the
    /// drop-down never left — the return fires only when the open
    /// itself hid it). Watching an edit live (opacity, font size)
    /// wants this paired with `hide_on_focus_loss: false`: with the
    /// focus-loss hide on (the default), focusing the Settings window
    /// still retracts the shown drop-down — that rule is its own key,
    /// untouched here.
    pub settings_coexist: bool,
    /// Whether the app follows the system light/dark mode or pins
    /// one. Defaults to `System`: no override is ever issued and
    /// system flips keep driving the app.
    pub theme_preference: ThemePreference,
    /// Named spawn presets (the `profile` module). Defaults to
    /// empty — managed on the settings drawer's Profiles page.
    /// Persisted in the profiles section ([`ProfilesSection`]), not
    /// the settings section.
    #[serde(skip)]
    pub profiles: Vec<Profile>,
    /// Name of the profile plain new tabs launch. `None` (the
    /// default) keeps the configured shell; a name the `profiles`
    /// list no longer carries also resolves to the plain shell —
    /// loading never rewrites the config to drop it. Persisted in
    /// the profiles section ([`ProfilesSection`]).
    #[serde(skip)]
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
    /// never fatal (`keybinds::effective_key_binds`). Persisted in
    /// the keybinds section ([`KeybindsSection`]), not the settings
    /// section.
    #[serde(skip)]
    pub custom_key_binds: BTreeMap<String, Binding>,
    /// Whether the status band renders at the surface's free edge
    /// (status-bar spec): the 28 px strip narrating the active
    /// terminal, adjacent to the grab bar under `tab_position: Auto`
    /// and one slot inward when a pin claims the grab side. Defaults to `true` — the
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
    /// The exit-status chip (status-exit spec): the last finished
    /// command's non-zero exit code — `✗1` in the scheme's own
    /// attention red, beside its frozen duration — sourced solely
    /// from the shell's own OSC 133;D report (opt-in shell
    /// integration), never a guess: without integration the band
    /// renders exactly what it renders without the chip. Key off
    /// means the verdict is never gathered and never rendered; the
    /// engine's timer keeps recording either way, so a re-enable
    /// shows whatever verdict still stands.
    pub status_bar_show_exit_status: bool,
    /// The progress chip (osc94-progress spec): the running
    /// command's own OSC 9;4 report — `42%` beside the exit chip,
    /// `…` when indeterminate, the value in the scheme's own
    /// attention red when the reporter says error (`!` when none
    /// ever stood) — opt-in shell integration like the 133;D
    /// verdict, never a guess: without integration the band renders
    /// exactly what it renders without the chip. Three deaths keep
    /// the number honest (the protocol's state 0, the foreground
    /// handoff, and ~15 s without a refresh), so a dead reporter
    /// never leaves its last number lying. Key off means the report
    /// is never gathered and never rendered.
    pub status_bar_show_progress: bool,
    /// The program status seats (osc7501-status spec): the program's
    /// own OSC 7501 report — `blocked` (waiting on you) and `error`
    /// (stopped on failure) tint the process disc's fill with the
    /// scheme's own attention red, and a progress percent rides the
    /// name seat beside the process name (`cargo 40%`) — opt-in
    /// shell integration like the 133;D verdict, never a guess:
    /// without integration the band renders exactly what it renders
    /// without the report. The root record alone stands (child
    /// records and the `app`/`title`/`msg`/`kind` keys are ignored
    /// by design — one disc, one name seat), and its lifetime rides
    /// `DropTerm`'s own observed edges: the foreground handoff drops
    /// `working`/`blocked`, `done`/`error` persist until replaced or
    /// cleared, and a full reset (RIS) clears everything. Key off
    /// means the report is never gathered and never rendered.
    pub status_bar_show_program_status: bool,
    /// The scrolled-back chip (status-scrolled spec): the
    /// viewport's lines back from the bottom — `↑12` — shown only
    /// off-bottom, its click jumping to the bottom (the band's one
    /// action chip). An in-memory grid read at view assembly; the
    /// toggle hides the chip outright.
    pub status_bar_show_scrollback: bool,
    /// The script segments' ordered-id key (status-segments spec):
    /// `None` (the default) renders every registered script segment
    /// in registration order; `Some(list)` renders exactly the
    /// listed ids, in list order — omission is the toggle, so an
    /// unlisted registered segment hides without unregistering. A
    /// listed id no script registers renders nothing and costs at
    /// most one quiet log line at the sync that reveals it. Applies
    /// live on the config echo.
    pub status_script_segments: Option<Vec<String>>,
    /// Whether the right edge rail stands (panel-containers PC3): a
    /// host-owned panel container at the drawer's right edge, the
    /// seam `panel-slot` docks plugin views into. Defaults to
    /// `false` — dormant until a tenant exists; with the gate off,
    /// nothing in the composition changes (the byte-identical pin),
    /// with it on, an empty rail stands beside the column. The
    /// rail's width is the shared per-edge size memory (the model's
    /// own default; nothing resizes it until a tenant needs to).
    /// Applies on the config echo's next view pass.
    pub panel_rail_enabled: bool,
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
    /// Whether the band arms its missed-activity halo
    /// (status-activity spec): a thin attention ring around the
    /// process disc while output landed on the active tab under a
    /// hidden surface and nothing has acknowledged it — input,
    /// paste, scroll, a pane click, or a tab switch. Key off means
    /// the fact is never gathered and never rendered; the engine's
    /// latch keeps arming either way, so a later flip-on shows
    /// whatever the surface still holds.
    pub status_bar_activity_mark: bool,
    /// Who owns the prompt line (rich-input spec): `Native` — the
    /// default, the no-regression ruling (no v1 yet) — forwards
    /// every keystroke to the PTY verbatim with the whole suggest
    /// lane dark; `Rich` gives `DropTerm` the line at a live prompt
    /// (buffer, edit, highlight, complete, execute; unsure prompts
    /// hide the editor and pass keys through). Per-profile through
    /// [`Profile::input_mode`](crate::core::profile::Profile::input_mode),
    /// `None` following this global. Flips apply live on the config
    /// echo.
    pub input_mode: InputMode,
    /// Where the rich-mode input strip docks (rich-input spec, OQ-7):
    /// `Bottom` — the default — at the grid's foot where the prompt
    /// is; `Top` at the head of the grid region, every Enter dropping
    /// the executed command into the flow while the grid scrolls
    /// normally. Flips apply live on the config echo.
    pub input_position: InputPosition,
    /// Whether ghost text computes inside the editor (rich-input
    /// spec): the dim rest-of-command after the cursor, Right
    /// accepting it whole and Alt+Right one word — the old
    /// `autosuggestions_enabled` role under its decoupled name.
    /// Defaults to `true` (the fish/Warp default-on precedent);
    /// `false` darkens the ghost alone. Flips apply live.
    pub input_ghost: bool,
    /// Whether the completion menu computes (rich-input spec): the
    /// candidate list over the owned line, opened by the chord and —
    /// per `input_auto_popup` — as you type — the old
    /// `autosuggestions_menu_enabled` role. Defaults to `true`;
    /// `false` removes the menu surface exactly, the ghost untouched.
    /// Flips apply live: a showing menu closes the moment the switch
    /// lands.
    pub input_menu: bool,
    /// Whether the menu also opens as you type (rich-input spec, the
    /// parity gates: single row, two-char floor, line-end cursor) —
    /// the old `autosuggestions_menu_auto` role. Defaults to `true`
    /// — the suggest-parity OQ-4 ruling carried over (the record
    /// keeps our default, rejecting Warp's
    /// `completions_open_while_typing: false`); `false` leaves the
    /// chord the only opener. Flips apply live.
    pub input_auto_popup: bool,
    /// Whether the owned line tokenizes and colors live (rich-input
    /// spec): command known-vs-unknown, flags, strings, variables,
    /// the unknown-command mark a dashed underline. Defaults to
    /// `true`; `false` renders the line plain. Flips apply live.
    pub input_highlight: bool,
    /// Whether menu rows carry the corpus descriptions (rich-input
    /// spec). Defaults to `true`; `false` hides the description
    /// column. Flips apply live.
    pub input_descriptions: bool,
    /// Whether menu rows carry their type badges — cmd/flag/path/
    /// hist/env/sub (rich-input spec). Defaults to `true`; `false`
    /// hides the badges. Flips apply live.
    pub input_badges: bool,
    /// Whether menu rows carry their icons (rich-input spec).
    /// Defaults to `true`; `false` hides the icons. Flips apply
    /// live.
    pub input_icons: bool,
    /// Whether the argument hint line renders above the menu
    /// (rich-input spec). Defaults to `true`; `false` hides the hint.
    /// Flips apply live.
    pub input_argument_hint: bool,
    /// The menu's density switch (rich-input spec): `false` (the
    /// default) renders comfortable rows, `true` compact — the
    /// approved mock's two densities. Flips apply live.
    pub input_compact_rows: bool,
    /// Whether the path provider joins the autosuggest merge
    /// (path-suggest spec): filesystem completion of the typed
    /// token's final component — first sorted match in the parent's
    /// listing, directories suffixed `/`, backslash-escaped where the
    /// shell needs it, silent over ssh and on any entry that cannot
    /// be safely written. Local-machine knowledge only; history
    /// suggestions are unaffected. Defaults to `true`; `false`
    /// restores history-only behavior exactly. Flips apply live on
    /// the next typed prefix.
    pub autosuggestions_paths_enabled: bool,
    /// Whether the command-name provider joins the autosuggest merge
    /// (command-catalog spec): the command word at the command
    /// position completes from the executables on the tab's
    /// spawn-time `$PATH` — shortest match wins, ties alphabetical,
    /// the match must extend the word — so a fresh tab with an empty
    /// history ring still finishes `gi` → `git`, and after one real
    /// run history takes over invisibly. A snapshot taken at spawn,
    /// never rescanned; local-machine knowledge, silent over ssh;
    /// history suggestions are unaffected. Defaults to `true`;
    /// `false` restores history-only behavior exactly. Flips apply
    /// live at the next typed prefix.
    pub autosuggestions_commands_enabled: bool,
    /// Whether history commits remember their working directory and
    /// persist DropTerm-side (suggest-depth spec, OQ-1/OQ-2): with
    /// the key on, an Enter-committed command records the shell's
    /// cwd beside it, merges into the app-global `history.ron` under
    /// the XDG state seat (capped and deduped like the ring), and
    /// seeds back at boot before the shell's own history file — so
    /// suggestions from history can rank by where you are (the
    /// context-first ranking rides its own task). Defaults to `true`
    /// (OQ-1); `false` restores the recency-only ring exactly: no
    /// cwd capture, and an off run never touches the store file (a
    /// corrupt one waits untouched for the key to come back). Flips
    /// apply live at the next commit.
    pub autosuggestions_history_context: bool,
    /// The idle-delay display gate for autosuggestions (suggest-delay
    /// spec): a qualifying suggestion is withheld until the typed
    /// segment has been stable this many milliseconds — the
    /// twice-proven ghost-text knob (VS Code's `minShowDelay`,
    /// ble.sh's `complete_auto_delay`), fish parity at `0` (the
    /// default, today's behavior bit-for-bit: the ghost appears on
    /// the same update the candidate computes). Any keystroke
    /// restarts the wait and dismisses a showing ghost instantly;
    /// Right/Alt+Right are unchanged the moment the ghost shows. The
    /// knob changes *when* a suggestion is seen, never *which*
    /// suggestion wins. Applies live; 200–300 ms suits the churn
    /// complaint.
    pub autosuggestions_delay_ms: u16,
    /// The user-declared alias map (suggest-aliases spec): alias →
    /// expansion (`gc = "git checkout"`), re-targeting the specs
    /// and catalog providers at the expansion's words — a mapped
    /// alias ghosts what its expansion ghosts (`gc --dr` ghosts
    /// git's `--dry-run`). Authoritative for exactly what the user
    /// declares; nothing is guessed past it, no shell rc file is
    /// ever read or parsed, and nothing runs in the session — the
    /// expansion is completion-time knowledge, never input: an
    /// accept still writes only the suggestion's suffix. Chains
    /// resolve recursively (depth cap 3, cycle-safe); history
    /// outranks the map always; silent on a tab spawned as ssh.
    /// Config-file first (the map is author-sized, not
    /// toggle-sized); defaults to empty — no alias re-targets, today
    /// byte-for-bit. Applies live at the next typed prefix.
    pub autosuggestions_aliases: HashMap<String, String>,
    /// The installed extended completions tier (suggest-embed spec,
    /// the 2026-11-07 generous-default amendment): the packaged
    /// `completions-extended.toml` — every corpus command beyond the
    /// embedded floor's core tranche, seated in the XDG data dirs —
    /// adds beside the floor while the key is on. Defaults to `true`
    /// (the file ships inside the package, so generosity costs no
    /// download and no network; the toggle is the kill switch, not
    /// the opt-in); `false` answers the embedded floor and the
    /// user's `completions.toml` alone. An absent, unreadable, or
    /// invalid file answers the floor whatever the key says.
    /// Flips apply live on the config echo.
    pub autosuggestions_completions_extended: bool,
    /// Whether inline images render (iterm-images spec): OSC 1337
    /// sequences — iTerm2's `imgcat` both forms, `wezterm imgcat`,
    /// forced-mode `chafa`/`timg`/`viu` — decode in place and ride
    /// the grid like text, scrolling and reflowing with it. Defaults
    /// to `true` (kitty and Ghostty ship images on); `false` makes
    /// the whole pipeline inert: the fork's arm stays parsed but
    /// nothing ever decodes or places. Sixel stays deferred — DA1
    /// answers honestly without `;4` — and `inline=0` sequences
    /// render nothing either way. Flips apply live on the config
    /// echo.
    pub images_enabled: bool,
    /// The first-run summon prompt's retirement flag (global-toggle-
    /// guidance spec): set when the user ticks "Don't ask again" on
    /// the cold-start prompt that offers to set the summon key. The
    /// binding itself lives in the system's shortcut config — this
    /// key owns only the nag. Defaults to `false`: the prompt shows
    /// at cold startup while no summon binding exists, and a bound
    /// key retires it regardless of this flag.
    pub onboarding_dismissed: bool,
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
            tab_bar_min_tabs: 2,
            layer: Layer::default(),
            keyboard_mode: KeyboardMode::default(),
            respect_panel: true,
            output: None,
            shell: None,
            args: Vec::new(),
            font_name: "Noto Sans Mono".to_string(),
            font_size: 14,
            color_scheme_dark: crate::core::theme_source::GITHUB_THEME_DARK.to_string(),
            color_scheme_light: crate::core::theme_source::GITHUB_THEME_LIGHT.to_string(),
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
            settings_coexist: false,
            theme_preference: ThemePreference::default(),
            profiles: Vec::new(),
            default_profile: None,
            new_tab_cwd: NewTabCwd::default(),
            default_cwd: None,
            custom_key_binds: BTreeMap::new(),
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
            status_bar_show_exit_status: true,
            status_bar_show_progress: true,
            status_bar_show_program_status: true,
            status_bar_show_scrollback: true,
            status_script_segments: None,
            panel_rail_enabled: false,
            tab_bar_border: true,
            status_bar_border: true,
            status_bar_activity_mark: true,
            input_mode: InputMode::default(),
            input_position: InputPosition::default(),
            input_ghost: true,
            input_menu: true,
            input_auto_popup: true,
            input_highlight: true,
            input_descriptions: true,
            input_badges: true,
            input_icons: true,
            input_argument_hint: true,
            input_compact_rows: false,
            autosuggestions_paths_enabled: true,
            autosuggestions_commands_enabled: true,
            autosuggestions_history_context: true,
            autosuggestions_delay_ms: 0,
            autosuggestions_aliases: HashMap::new(),
            autosuggestions_completions_extended: true,
            images_enabled: true,
            onboarding_dismissed: false,
        }
    }
}

/// The per-segment visibility switches of the status band, folded
/// from the config's `status_bar_show_*` bools
/// ([`DropTermConfig::status_toggles`]). A hidden segment is absent
/// downstream — and its costly inputs are never gathered. (Moved
/// from the frontend's status bar by the shared-core spec: it is the
/// config model's fold target; the band that renders it stays
/// frontend-side.)
#[expect(
    clippy::struct_excessive_bools,
    reason = "thirteen independent visibility switches — one per config key, by design"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusToggles {
    /// The state disc + process name.
    pub process: bool,
    /// The command timer's reading.
    pub duration: bool,
    /// The exit chip.
    pub exit: bool,
    /// The progress chip.
    pub progress: bool,
    /// The program status's disc tint + name-seat percent
    /// (osc7501-status) — one switch for the report's two seats,
    /// riding the process segment's own visibility.
    pub program_status: bool,
    /// The `~`-collapsed cwd.
    pub path: bool,
    /// The git branch of the cwd.
    pub branch: bool,
    /// The profile name.
    pub profile: bool,
    /// The terminal's cell dimensions.
    pub grid: bool,
    /// The tab position.
    pub tab: bool,
    /// The zoom percent.
    pub zoom: bool,
    /// The scrolled-back chip.
    pub scrolled: bool,
    /// The wall clock.
    pub clock: bool,
}

impl StatusToggles {
    /// Every segment shown — the all-on default the config's
    /// `true` keys fold into.
    pub const fn all() -> Self {
        Self {
            process: true,
            duration: true,
            exit: true,
            progress: true,
            program_status: true,
            path: true,
            branch: true,
            profile: true,
            grid: true,
            tab: true,
            zoom: true,
            scrolled: true,
            clock: true,
        }
    }
}

impl DropTermConfig {
    /// The status band's per-segment visibility, folded from the
    /// additive bools — the resolver's toggle set.
    pub fn status_toggles(&self) -> StatusToggles {
        StatusToggles {
            process: self.status_bar_show_process,
            duration: self.status_bar_show_duration,
            exit: self.status_bar_show_exit_status,
            progress: self.status_bar_show_progress,
            program_status: self.status_bar_show_program_status,
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
    #[must_use]
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

    /// Loads the whole config from the three section files under
    /// `dir`: the settings keys from `config.ron`, the profile keys
    /// from `profiles.ron`, the binding map from `keybinds.ron`. A
    /// missing section file is that section's silent first run and
    /// an unreadable one is quarantined to a `.bak` sibling
    /// ([`store::load`]) — either way the section's defaults apply
    /// and the other two sections are unaffected. Clamping is the
    /// caller's step.
    pub fn load_from(dir: &Path) -> Self {
        Self::composed(
            store::load(&dir.join(store::SETTINGS_FILE)).unwrap_or_default(),
            store::load(&dir.join(store::PROFILES_FILE)).unwrap_or_default(),
            store::load(&dir.join(store::KEYBINDS_FILE)).unwrap_or_default(),
        )
    }

    /// The live-reload twin of [`Self::load_from`], for a store
    /// being edited while the app runs: each section that parses
    /// right now replaces `last`'s, and a section that is missing or
    /// unreadable at this instant — an editor mid-write, a
    /// delete-then-recreate save — keeps `last`'s values and is left
    /// untouched on disk ([`store::peek`]). Nothing is quarantined
    /// and nothing flashes to defaults; a deleted section resets at
    /// the next start. Clamping is the caller's step.
    pub fn reload_from(dir: &Path, last: &Self) -> Self {
        Self::composed(
            store::peek(&dir.join(store::SETTINGS_FILE)).unwrap_or_else(|| last.clone()),
            store::peek(&dir.join(store::PROFILES_FILE)).unwrap_or_else(|| last.profiles_section()),
            store::peek(&dir.join(store::KEYBINDS_FILE)).unwrap_or_else(|| last.keybinds_section()),
        )
    }

    /// One config out of its three sections: the settings section's
    /// own keys with the other two sections' overlaid.
    fn composed(settings: Self, profiles: ProfilesSection, keybinds: KeybindsSection) -> Self {
        Self {
            profiles: profiles.profiles,
            default_profile: profiles.default_profile,
            custom_key_binds: keybinds.custom_key_binds,
            ..settings
        }
    }

    /// This config's profiles section — what a profile write
    /// serializes into `profiles.ron`.
    pub fn profiles_section(&self) -> ProfilesSection {
        ProfilesSection {
            profiles: self.profiles.clone(),
            default_profile: self.default_profile.clone(),
        }
    }

    /// This config's keybinds section — what a binding write
    /// serializes into `keybinds.ron`.
    pub fn keybinds_section(&self) -> KeybindsSection {
        KeybindsSection {
            custom_key_binds: self.custom_key_binds.clone(),
        }
    }
}

/// The store's profiles section (`profiles.ron`, the profile
/// writer's file): the two profile keys under their unchanged names,
/// documented on [`DropTermConfig`]'s same-named fields. A key the
/// file lacks loads as its default; the derived `Default` is the
/// config's own (no profiles, no default profile).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfilesSection {
    /// See [`DropTermConfig::profiles`].
    pub profiles: Vec<Profile>,
    /// See [`DropTermConfig::default_profile`].
    pub default_profile: Option<String>,
}

/// The store's keybinds section (`keybinds.ron`, the keybinds
/// writer's file): the one binding-map key under its unchanged name.
/// A missing key loads as the empty map — all defaults, the config's
/// own default.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindsSection {
    /// See [`DropTermConfig::custom_key_binds`].
    pub custom_key_binds: BTreeMap<String, Binding>,
}

/// Whether a command program could exec right now: a slash-bearing
/// path is checked directly, a bare name is searched on `PATH`, and
/// either way the file must carry an execute bit. Best-effort by
/// design — it lets config- and spawn-resolved programs degrade
/// before they reach a pty whose failure would kill the whole tab,
/// not predict every exec failure (races and permission oddities
/// still land in `Terminal::new`'s error log).
pub fn program_is_runnable(program: &str) -> bool {
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

pub mod store;

#[cfg(test)]
mod tests;
