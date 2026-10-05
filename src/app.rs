// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::term::{self as aterm, Config as TermConfig};
use alacritty_terminal::tty::{Options as PtyOptions, Shell as PtyShell};
use alacritty_terminal::vte::ansi::NamedColor;
use cosmic::app::{Core, Task};
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::Length;
use cosmic::iced::event::listen_raw;
use cosmic::iced::keyboard::key::Named;
use cosmic::iced::keyboard::{Event as KeyEvent, Key, Modifiers};
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    KeyboardInteractivity, destroy_layer_surface, get_layer_surface, set_exclusive_zone,
    set_keyboard_interactivity, set_layer,
};
use cosmic::iced::runtime::core::event::PlatformSpecific;
use cosmic::iced::runtime::core::event::wayland::LayerEvent;
use cosmic::iced::{Event, Subscription, clipboard, window};
use cosmic::surface::Action as SurfaceAction;
use cosmic::widget;
use cosmic::{Application, Element};
use tokio::sync::mpsc;

use crate::config::{CONFIG_VERSION, DropTermConfig, Edge, TabPosition, ThemePreference};
use crate::context_menu;
use crate::dbus_state;
use crate::geometry::{self, Geometry};
use crate::grab_bar::GrabBar;
use crate::keybinds::{self, default_key_binds};
use crate::notify;
use crate::profile;
use crate::session;
use crate::settings;
use crate::slide::{self, SlideBox};
use crate::surface;
use crate::tab_bar::{TabBar, TooltipSide};
use crate::tabs::{self, Tab, TabId};
use crate::terminal::{self, Action, Terminal, TerminalBox, TerminalOptions, theme_source};

pub const APP_ID: &str = "io.github.l1br3_io.CosmicExtDropterm";

/// Debounce window for toggle requests, guarding against the double
/// key-press events a compositor occasionally delivers for one physical
/// press (see the README's known-risks section).
const TOGGLE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(150);

/// What the running instance should do on startup, mirrored by a second
/// launch's D-Bus activation (see `main.rs`).
#[derive(Debug, Clone, PartialEq, Eq, Default, clap::Subcommand)]
pub enum ToggleAction {
    #[default]
    Toggle,
    Show,
    Hide,
    /// Open a new tab — remotely (a second launch) or at first startup —
    /// optionally in an explicit working directory and/or running an
    /// explicit command line. Implies showing the overlay.
    NewTab {
        /// Working directory for the new tab; defaults to the active
        /// tab's cwd.
        #[arg(long)]
        cwd: Option<std::path::PathBuf>,
        /// Command line to run instead of the shell — whitespace-split,
        /// no quoting support.
        #[arg(long, value_parser = clap::builder::NonEmptyStringValueParser::new())]
        cmd: Option<String>,
        /// Name of the profile to launch (see the `profile` module):
        /// resolved against the running instance's config; a name that
        /// no longer exists warns and spawns the plain shell. An
        /// explicit `--cwd` composes with it and wins over the
        /// profile's cwd.
        #[arg(
            long,
            value_parser = clap::builder::NonEmptyStringValueParser::new(),
            conflicts_with = "cmd"
        )]
        profile: Option<String>,
    },
    /// Close a tab remotely — the active tab, or the 1-based `--tab-index`
    /// target. Out-of-range targets no-op with a warning; the overlay
    /// is not shown.
    CloseTab {
        /// 1-based index of the tab to close; defaults to the active tab.
        #[arg(long, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        tab_index: Option<usize>,
    },
    /// Activate a tab remotely — the strict 1-based `N`, no wrap and no
    /// `9 = last` keybind idiom. Out-of-range indices no-op with a
    /// warning; a resolved activation shows the overlay, exactly like
    /// a `new-tab`.
    SelectTab {
        /// 1-based index of the tab to activate.
        #[arg(value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        index: usize,
    },
    /// Rename a tab remotely — the active tab, or the 1-based
    /// `--tab-index` target. A non-empty TITLE becomes the sticky
    /// override shell OSC titles never clobber; the empty TITLE is the
    /// documented reset, clearing the override so shell titles resume.
    /// Quiet: the overlay is not shown.
    RenameTab {
        /// New title; the empty string clears the override.
        title: String,
        /// 1-based index of the tab to rename; defaults to the active tab.
        #[arg(long, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        tab_index: Option<usize>,
    },
    /// Duplicate a tab remotely — the active tab, or the 1-based
    /// `--tab-index` target: a fresh shell opens in that tab's cwd —
    /// the running command is never copied — lands after the active
    /// tab, and becomes active. Implies showing the overlay; an
    /// unresolvable target degrades to the bare new-tab inheritance
    /// with a warning.
    DuplicateTab {
        /// 1-based index of the tab to duplicate; defaults to the active tab.
        #[arg(long, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        tab_index: Option<usize>,
    },
    /// Switch a live tab's profile remotely — the active tab, or the
    /// 1-based `--tab-index` target: the tab's terminal is replaced
    /// in place by a fresh one running `--profile`'s resolved
    /// command/cwd (the running command and scrollback are lost, the
    /// same close+reopen cost), keeping the tab's slot, activation
    /// state, and sticky user rename; the tab is a persistent profile
    /// tab from then on. Implies showing the overlay; an unknown
    /// profile name or unresolvable target warns and edits nothing.
    SwitchProfileTab {
        /// Name of the profile to switch to (see the `profile`
        /// module): exact, case-sensitive, first-match against the
        /// running instance's config.
        #[arg(long, value_parser = clap::builder::NonEmptyStringValueParser::new())]
        profile: String,
        /// 1-based index of the tab to switch; defaults to the active tab.
        #[arg(long, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        tab_index: Option<usize>,
    },
}

impl std::fmt::Display for ToggleAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Toggle => write!(f, "toggle"),
            Self::Show => write!(f, "show"),
            Self::Hide => write!(f, "hide"),
            Self::NewTab { .. } => write!(f, "new-tab"),
            Self::CloseTab { .. } => write!(f, "close-tab"),
            Self::SelectTab { .. } => write!(f, "select-tab"),
            Self::RenameTab { .. } => write!(f, "rename-tab"),
            Self::DuplicateTab { .. } => write!(f, "duplicate-tab"),
            Self::SwitchProfileTab { .. } => write!(f, "switch-profile-tab"),
        }
    }
}

/// What a new tab runs: the user's shell — config-resolved or per-tab
/// recorded — paired with the config's shell arguments, an explicit
/// whitespace-split command line whose exit closes the tab and whose
/// tabs are session-transient, or a profile: a named preset resolved
/// before spawn so the tab can record the id it launched under.
/// Profile tabs persist in the session and re-resolve by id at
/// restore; that is the explicit opt-in a raw `--cmd` tab
/// deliberately is not.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SpawnCommand {
    Shell(String),
    CommandLine(String),
    /// The resolved preset: the id (recorded on the tab, saved by the
    /// session) plus the command line and cwd the id resolves to at
    /// spawn time. Handed to [`DropTerm::spawn_shell`] already
    /// resolved — spawn applies a profile, it never looks one up.
    Profile {
        name: String,
        command: String,
        cwd: Option<PathBuf>,
    },
}

/// One freshly spawned terminal plus the tab-level facts recorded
/// beside it at spawn: the id the terminal is tagged with (allocated
/// only once the spawn is actually attempted — a deferred spawn never
/// burns one, so `Terminal N` numbering cannot drift), the shell
/// actually running (a command line's program, not the config shell),
/// the profile id the spawn ran under — set only when the preset's
/// command line was runnable — and the session-ephemeral flag a
/// command-line spawn carries. [`DropTerm::spawn_shell`] wraps this
/// into a [`Tab`] and records the facts on it.
struct SpawnedTerminal {
    tab_id: TabId,
    terminal: Terminal,
    shell: String,
    profile_id: Option<String>,
    ephemeral: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SurfaceState {
    Hidden,
    /// A surface stretched across the whole active output, waiting for its
    /// `Window::Opened` event so we learn that output's real size.
    Measuring(window::Id),
    Shown(window::Id),
    /// A shown surface sliding out toward its anchored edge: the
    /// content still renders — translated by the live `Out` slide —
    /// and the layer surface is destroyed only when that slide
    /// completes. Keyboard interactivity was already released at
    /// slide-out start, so focus events find no `Shown` surface to
    /// act on; the drag handlers and live reshape ignore this state
    /// by construction (they match `Shown` exclusively).
    Hiding(window::Id),
}

#[derive(Debug, Clone)]
pub enum Message {
    ToggleAction(ToggleAction),
    ConfigChanged(DropTermConfig),
    /// The `set_theme` task from a theme-preference change landed:
    /// re-run the color apply so the `WINDOW_BG_COLOR` derivation —
    /// which reads the process-global theme — sees the new mode. Cell
    /// colors were already resolved correctly in the same pass via
    /// `effective_is_dark` when the change arrived.
    ReapplyColors,
    TermEventTx(mpsc::UnboundedSender<(TabId, TermEvent)>),
    TermEvent(TabId, TermEvent),
    SurfaceOpened(window::Id, cosmic::iced::Size),
    /// The granted size of the surface with `id` changed (compositor
    /// configure after a `set_size`, or the initial configure): the new
    /// height in logical pixels, used to pin the terminal's height so the
    /// tab bar keeps its strip. The id gates the stream to the tracked
    /// layer surface (`resize_acts_on`): a foreign window's heights
    /// must never feed the pin.
    WindowResized(window::Id, f32),
    LayerFocused(window::Id),
    LayerUnfocused(window::Id),
    KeyPressed(Modifiers, Key, cosmic::iced::keyboard::key::Physical),
    WindowFocused,
    WindowUnfocused,
    MiddleClick,
    /// A tab bar button was left-clicked.
    TabSelected(TabId),
    /// A tab bar button was closed via its `×` or a middle-click.
    TabClosed(TabId),
    /// Switch a live tab's profile in place: the tab id (the bar's
    /// stable handle, like every tab message) and the config profile
    /// index — resolved at apply time, the `NewProfileTab`
    /// precedent, so a config edit between snapshot and pick can
    /// still be caught as stale. The apply respawns the tab's
    /// terminal under the profile (see [`DropTerm::switch_tab_profile`]).
    SwitchTabProfile(TabId, usize),
    /// A tab body was double-clicked: start an inline rename.
    RenameStarted(TabId),
    /// The rename input's draft changed.
    RenameDraftChanged(String),
    /// The rename should commit now: Enter was pressed, a press
    /// landed outside the input, or the surface lost focus.
    RenameCommitted,
    /// The rename input was dismissed with Escape or Tab: cancel,
    /// keeping the previous title.
    RenameCancelled,
    /// A tab was dragged to a new position in the bar: the dragged
    /// tab's id and the index it was dropped at.
    TabMoved(TabId, usize),
    /// The grab bar was dragged: the pointer's surface-local y
    /// (window-absolute, not a delta and not bar-local) while a press
    /// is held. Targets recompute absolutely from it, so the moving
    /// window frame of a bottom-anchored surface cannot compound
    /// drift into the drag.
    GrabBarDragged(f32),
    /// A grab bar drag ended: the surface keeps the height it settled
    /// at; persisting it is the debounced config write's job.
    GrabBarReleased,
    /// The trailing apply of a throttled live drag fired: a target
    /// dropped inside the apply interval still lands — the pointer
    /// pausing mid-drag must not leave the surface behind. A stale
    /// fire (drag ended, surface hidden) is a no-op: the apply path
    /// re-checks the cadence gate.
    DragApplyDue,
    /// The trailing adopt of a debounced terminal-pin height fired:
    /// heights the adoption debounce skipped — the double-reported
    /// granted stream's echoes — land once the stream settles. A
    /// stale fire is a no-op.
    WindowPinDue,
    /// The debounce window for a drag-release height write elapsed:
    /// the generation that scheduled it, checked against the current
    /// pending write so a newer drag's schedule cancels this stale
    /// one.
    WriteDue(u32),
    /// The session-save debounce window elapsed: the generation that
    /// armed it, checked against the armed window so a stale fire —
    /// superseded by a newer window, or a duplicate delivery after
    /// the write — writes nothing.
    SessionSaveDue(u32),
    /// Slide clock tick: redraw-only while a slide is live — the
    /// eased offset is evaluated at draw time inside `SlideBox`, so
    /// the tick exists to keep redraw requests pending (see
    /// `SLIDE_TICK`). The first tick past the duration parks the
    /// slide at its endpoint — rest for an `In` slide, the anchored
    /// edge (then destroy) for an `Out` one — and drops it (and with
    /// it the subscription).
    SlideTick,
    OpenHyperlink(String),
    ClipboardPasted(Option<String>),
    ClipboardPastedPrimary(Option<String>),
    /// An event from the settings drawer's controls. Inputs only —
    /// the update side resolves each through the pure planners and
    /// commits it as a `Bare` write through [`Self::write_config`],
    /// whose `ConfigChanged` echo is the only apply path.
    Settings(settings::SettingsEvent),
    /// A context-menu item was picked: the item's
    /// [`terminal::Action`], dispatched through `perform_action` —
    /// the same entry point the keybind map uses, so menu and keybind
    /// behavior cannot fork.
    Action(Action),
    /// The terminal's context menu should open or close. Published
    /// by the `TerminalBox` press handler: `true` on a right-press
    /// with the menu closed, `false` on
    /// any default-mode press while open (toggle-close). The
    /// wrapper's `on_close` also sends `false` — it fires on every
    /// close path (Escape, click-away, item pick, compositor
    /// dismiss), so duplicate `false`s are the norm and harmless:
    /// the arm is a plain state set.
    ContextMenu(bool),
    /// A surface action from a widget: the context-menu popup's
    /// create/destroy requests, forwarded to libcosmic verbatim —
    /// popups are created and destroyed by the runtime, never by the
    /// app (as in cosmic-term).
    Surface(SurfaceAction<Message>),
}

/// An in-flight inline tab rename: the tab being renamed and the
/// current draft. The input itself is an overlay the tab bar hosts
/// over the renamed tab's button.
struct RenameEdit {
    tab: TabId,
    draft: String,
}

/// An in-progress grab-bar drag's runtime state: the pure anchor
/// every target recomputes from (`geometry`), the latest reported
/// pointer y, and the last live apply (when, which target) for the
/// change-gated apply cadence — see `geometry::drag_apply_due`.
/// Applies are bounded because each one costs the runtime two window
/// resizes plus terminal and pty resizes (its `set_size` handling
/// double-reports each apply), so per-move applies at pointer rate
/// saturate the single runtime thread — the drag froze input on
/// hardware.
struct Drag {
    anchor: geometry::DragAnchor,
    /// The pointer's window y as of the last `GrabBarDragged`, so the
    /// trailing apply and the release settle at where the pointer
    /// actually is, not where the last apply landed.
    pointer_y: f32,
    /// The last live apply, for the cadence gate.
    applied: Option<(std::time::Instant, u32)>,
}

pub struct DropTerm {
    core: Core,
    /// Config backend, consumed by the drag-resize write-back:
    /// `set("height_percent", …)` persists a settled drag. `None`
    /// when the config failed to open at init — writes are skipped,
    /// everything else works.
    config_handler: Option<cosmic_config::Config>,
    config: DropTermConfig,
    /// The overlay's terminals in tab order. Never empty while a
    /// surface is shown: closing the last tab respawns a fresh one
    /// (see `ensure_tabs`).
    tabs: Vec<Tab>,
    /// Index into `tabs` of the focused tab.
    active_tab: usize,
    terminal_id: widget::Id,
    term_event_tx: Option<mpsc::UnboundedSender<(TabId, TermEvent)>>,
    /// Source of monotonically increasing tab ids, never reused within
    /// a run.
    next_tab_id: u32,
    /// The startup session stash: the restore plan loaded from the
    /// state file in `init`, consumed by the first productive run of
    /// `ensure_tabs`.
    restore: session::RestoreStash,
    /// A remote `new-tab` request that arrived before the terminal
    /// event channel was created (first-launch startup): stashed
    /// here, applied the moment the channel bootstraps and the
    /// session restore has landed, so the requested tab never races —
    /// or is lost to — the restore.
    pending_remote_tab: Option<ToggleAction>,
    /// Debounced session-save state: which change window is armed and
    /// whether a write is owed (see `session::SessionSave`).
    session_save: session::SessionSave,
    /// Each live tab's working directory as last seen, seeded at
    /// spawn/restore with the spawn cwd: title events compare against
    /// this cache so only a real cwd change — not title churn — marks
    /// the session dirty.
    tab_cwds: HashMap<TabId, Option<PathBuf>>,
    /// The active inline rename, if any: which tab is being renamed
    /// and the current draft, mirrored into the input the tab bar
    /// overlays on the renamed button.
    rename: Option<RenameEdit>,
    /// An in-progress grab-bar height drag, if any: the absolute
    /// anchor every target recomputes from, plus the runtime state of
    /// the apply cadence (see `Drag`).
    drag: Option<Drag>,
    /// Debounced drag-release config-write state: which release's
    /// persist is still pending.
    write_back: geometry::WriteBack,
    /// The geometry of our last config write, kept to recognize and
    /// absorb its `ConfigChanged` echo. Cleared on absorption.
    last_written: Option<Geometry>,
    /// Widget id of the rename input, so focus tasks can target it.
    rename_input_id: widget::Id,
    surface: SurfaceState,
    /// The output size measured by the last `SurfaceOpened`, kept so
    /// later reshapes (config edits, drag) can recompute pixels without
    /// a new measuring pass (see `geometry.rs`).
    output_size: Option<(u32, u32)>,
    /// Granted height of the shown surface, from `WindowResized` events.
    /// The basis for pinning the terminal widget's height so the tab bar
    /// gets its strip inside the fixed-size surface.
    window_height: Option<f32>,
    /// The last pin adoption (when, which height), for the adoption
    /// debounce (see `geometry::window_pin_due`).
    pin_applied: Option<(std::time::Instant, f32)>,
    /// A height the debounce skipped, landing via the trailing adopt
    /// (`WindowPinDue`) once the granted stream settles.
    pending_height: Option<f32>,
    /// Whether the surface has received keyboard focus at least once since it
    /// was shown. Guards `hide_on_focus_loss` so a stray `Unfocused` delivered
    /// before any `Focused` (possible with `keyboard_mode: OnDemand`) can't
    /// make the surface open-then-instantly-close.
    surface_focused: bool,
    key_binds: HashMap<cosmic::widget::menu::key_bind::KeyBind, Action>,
    last_toggle: Option<std::time::Instant>,
    /// The live slide: the eased content offset while the surface
    /// animates — in from the anchored edge after a show, out toward
    /// it before a hide's destroy. `None` at rest — renders
    /// byte-identically to the unwrapped column — and cleared when
    /// the parking tick completes the slide (`SlideTick`).
    slide: Option<slide::Playing>,
    /// Whether the settings drawer is open over the surface (spec:
    /// settings-drawer). Overlay state only — the terminal keeps its
    /// size and the pty its grid (criterion 1); the drawer survives
    /// focus-loss hides with its drafts intact (criterion 8), and an
    /// explicit close clears `settings_buffer`.
    settings_open: bool,
    /// The settings drawer's in-flight drafts: display-only text and
    /// slider state, cleared on an explicit drawer close so a
    /// reopened drawer renders purely from config (spec criterion 8).
    settings_buffer: settings::SettingsBuffer,
    /// Whether the terminal's context menu is open: the `TerminalBox`
    /// press handler's latch, read by the view through
    /// `context_menu::open_trees` and fed back to the box so any
    /// press while open toggle-closes it. Cleared by `hide()` — the
    /// popup dies with the surface and `on_close` never fires there.
    context_menu_open: bool,
    /// Queryable state on the owned bus name: the connection the
    /// `dbus_connection` hook handed over, the shared snapshot the
    /// served V1 interface reads, and the last-served snapshot the
    /// sync step dedupes against. Inert until the hook fires —
    /// headless, all three stay exactly as initialized.
    dbus_conn: Option<zbus::Connection>,
    dbus_shared: Arc<Mutex<dbus_state::State>>,
    dbus_last: dbus_state::State,
}

impl DropTerm {
    /// The active tab, when any tab exists.
    fn active(&self) -> Option<&Tab> {
        self.tabs.get(self.active_tab)
    }

    fn terminal_options(&self) -> TerminalOptions {
        TerminalOptions {
            font_size: self.config.font_size,
            ..TerminalOptions::default()
        }
    }

    fn active_colors(&self) -> aterm::color::Colors {
        // The config preference is the source of truth:
        // `effective_is_dark`, not a direct global read, decides the
        // mode, so a pick applies colors in this same pass — before
        // the async `set_theme` task lands and moves the global.
        let is_dark = effective_is_dark(self.config.theme_preference, cosmic::theme::is_dark());
        let scheme_name = if is_dark {
            &self.config.color_scheme_dark
        } else {
            &self.config.color_scheme_light
        };
        theme_source::by_name(scheme_name).unwrap_or_else(|| {
            if is_dark {
                theme_source::cosmic_dark()
            } else {
                theme_source::cosmic_light()
            }
        })
    }

    /// Applies the current config and theme to every tab's terminal.
    /// Colors resolve once, before the window-background refresh, so
    /// the global and the tabs can never disagree on which scheme is
    /// active.
    fn apply_config_to_tabs(&mut self) {
        let options = self.terminal_options();
        let colors = self.active_colors();
        Self::refresh_window_bg_color(&colors);
        for tab in &self.tabs {
            tab.terminal.lock().unwrap().set_config(&options, &colors);
        }
    }

    /// Mirrors cosmic-term's own `update_config`: the effective window
    /// background color is a process-global (`terminal::WINDOW_BG_COLOR`)
    /// used as a fallback wherever a color scheme leaves "background"
    /// unset, so it must be refreshed whenever the active scheme or
    /// COSMIC theme changes.
    fn refresh_window_bg_color(colors: &aterm::color::Colors) {
        let theme = cosmic::theme::active();
        let color = cosmic::iced::Color::from(theme.cosmic().background(theme.transparent).base);
        let bytes = color.into_rgba8();
        let system_bg = u32::from(bytes[2])
            | (u32::from(bytes[1]) << 8)
            | (u32::from(bytes[0]) << 16)
            | 0xFF00_0000;
        let data = window_bg_color(colors, system_bg);
        terminal::WINDOW_BG_COLOR.store(data, std::sync::atomic::Ordering::SeqCst);
    }

    /// Spawns one terminal tab from a session restore's resolved
    /// spawn parameters ([`session::SpawnSpec`], with its recorded
    /// shell or re-resolved profile and re-validated cwd) — or, with
    /// `None`, from the config's resolved shell with the pty's
    /// default working directory.
    fn spawn_tab(&mut self, spec: Option<&session::SpawnSpec>) -> Option<Tab> {
        let Some(spec) = spec else {
            // The plain startup/respawn spawns ride the configured
            // default (ruling 1 — the never-empty respawn is the same
            // no-user-context plain spawn). Nothing to inherit here:
            // a default preset's dead cwd warns and degrades to the
            // pty's default directory.
            let command = default_spawn_command(&self.config);
            let cwd = match &command {
                SpawnCommand::Profile {
                    name, cwd: preset, ..
                } => live_preset_cwd(name, preset.as_ref()),
                _ => None,
            };
            return self.spawn_shell(&command, cwd);
        };
        if let Some(profile) = &spec.profile {
            // The id re-resolved at plan time: the current definition
            // runs, and `spec.cwd` — the ruled chain result — already
            // is the spawn directory, so the arm mirrors it and spawn
            // re-derives nothing.
            self.spawn_shell(
                &SpawnCommand::Profile {
                    name: profile.name.clone(),
                    command: profile.command.clone(),
                    cwd: spec.cwd.clone(),
                },
                spec.cwd.clone(),
            )
        } else {
            let shell = spec.shell.clone();
            let cwd = spec.cwd.clone();
            self.spawn_shell(&SpawnCommand::Shell(shell), cwd)
        }
    }

    /// Spawns one terminal tab running `command` in `cwd` — the common
    /// path behind session restore ([`Self::spawn_tab`]), the new-tab
    /// keybind (which inherits the active tab's cwd), and the CLI's
    /// `new-tab` action. `cwd: None` means the pty's default directory.
    /// The shell is recorded per tab at spawn, so a mid-run config
    /// change never rewrites a live tab's shell. The spawn itself is
    /// [`Self::spawn_terminal`]'s; this wrapper only wraps the result
    /// into a [`Tab`] and seeds the cwd cache.
    fn spawn_shell(&mut self, command: &SpawnCommand, cwd: Option<PathBuf>) -> Option<Tab> {
        let SpawnedTerminal {
            tab_id,
            terminal,
            shell,
            profile_id,
            ephemeral,
        } = self.spawn_terminal(command, cwd.as_deref())?;
        let mut tab = Tab::new(tab_id, shell, terminal);
        tab.ephemeral = ephemeral;
        // The profile id a tab launched under (if any), recorded beside
        // the ephemeral flag it is the opposite of: the session saves
        // one and never the other.
        tab.profile = profile_id;
        // Seed the cwd cache with the spawn cwd: a restored tab seeds
        // its re-validated saved cwd — so the fresh shell's first title
        // report reads as unchanged — and a default spawn seeds `None`;
        // an inherited cwd seeds the directory the tab opened in, for
        // the same reason.
        self.tab_cwds.insert(tab.id, cwd);
        Some(tab)
    }

    /// Spawns one terminal running `command` in `cwd` — the bottom
    /// half of every spawn path, below the [`Tab`] wrapping
    /// [`Self::spawn_shell`] adds: resolve the shell/args pair, build
    /// the pty options, allocate the tab's id, and construct the
    /// [`Terminal`] tagged with it. `cwd: None` means the pty's
    /// default directory. The id is allocated only once the spawn is
    /// actually attempted — after the channel guard — so a deferred
    /// spawn never burns one and `Terminal N` numbering cannot drift.
    /// Returns `None` the two ways the old single fn did: no terminal
    /// event channel yet (the caller defers), or `Terminal::new`
    /// erroring (logged).
    fn spawn_terminal(
        &mut self,
        command: &SpawnCommand,
        cwd: Option<&Path>,
    ) -> Option<SpawnedTerminal> {
        let Some(tx) = self.term_event_tx.clone() else {
            log::warn!("no terminal event channel yet; deferring terminal spawn");
            return None;
        };

        // A shell path pairs with the config's shell arguments; an
        // explicit command line — like a profile's stored one —
        // carries its own trailing arguments and never inherits them:
        // `--cmd htop` must not run `htop --login`. A command tab is
        // also session-ephemeral (never saved, never replayed); a
        // profile tab is the explicit opt-in for persistence, so it
        // never is.
        let ephemeral = matches!(command, SpawnCommand::CommandLine(_));
        let mut profile_id: Option<String> = None;
        let (shell, args) = match command {
            SpawnCommand::Shell(shell) => (shell.clone(), self.config.args.clone()),
            SpawnCommand::CommandLine(line) => {
                match tabs::command_line_parts(line) {
                    Some((program, trailing)) => (program, trailing),
                    // Unreachable in practice — the CLI rejects an
                    // empty --cmd — but a degraded empty line still
                    // spawns the default shell rather than a dying
                    // exec of "".
                    None => (self.config.resolved_shell(), self.config.args.clone()),
                }
            }
            SpawnCommand::Profile { name, command, .. } => {
                if let Some((program, trailing)) = tabs::command_line_parts(command) {
                    // The id is recorded only when the spawn actually
                    // runs the preset: the degraded line below spawns
                    // a plain-shell tab, which must not save an id
                    // restore would keep re-degrading.
                    profile_id = Some(name.clone());
                    (program, trailing)
                } else {
                    // A hand-edited config can store a blank command
                    // line; warn and spawn the plain shell —
                    // degradation, never an error surface.
                    log::warn!(
                        "profile {name} stores no runnable command line; \
                         spawning the shell instead"
                    );
                    (self.config.resolved_shell(), self.config.args.clone())
                }
            }
        };

        let term_config = TermConfig {
            scrolling_history: self.config.scrollback_lines as usize,
            ..TermConfig::default()
        };
        let pty_options = PtyOptions {
            shell: Some(PtyShell::new(shell.clone(), args)),
            // Alacritty chdirs before exec and silently keeps this
            // process's cwd on failure, so only a re-validated
            // directory ever reaches here (dead cwd → `$HOME` → unset
            // — see `plan_restore`; a new tab passes either an
            // inherited- or explicitly-requested live cwd).
            working_directory: cwd.map(Path::to_owned),
            drain_on_exit: true,
            ..PtyOptions::default()
        };

        let tab_id = TabId(self.next_tab_id);
        self.next_tab_id += 1;

        match Terminal::new(
            tab_id,
            tx,
            term_config,
            &pty_options,
            &self.terminal_options(),
            &self.active_colors(),
        ) {
            Ok(terminal) => Some(SpawnedTerminal {
                tab_id,
                terminal,
                shell,
                profile_id,
                ephemeral,
            }),
            Err(err) => {
                log::error!("failed to spawn shell: {err}");
                None
            }
        }
    }

    /// Guarantees at least one tab exists — restoring the saved
    /// session on the first productive run, spawning a fresh default
    /// tab every other time. Every path that needs a tab (the
    /// terminal channel arriving, showing the surface, the last tab's
    /// respawn) lands here, so the session is restored exactly once
    /// and nothing is spawned-then-deleted; runs before the channel
    /// exists defer, leaving the stash armed.
    fn ensure_tabs(&mut self) {
        if !self.tabs.is_empty() {
            return;
        }
        if let Some(plan) = self.restore.take_for_run(self.term_event_tx.is_some()) {
            let mut spawned = 0usize;
            for spec in &plan.tabs {
                if let Some(mut tab) = self.spawn_tab(Some(spec)) {
                    // Saved chrome reapplied onto the fresh tab: plain
                    // append keeps the saved order, and the override
                    // keeps the fresh shell's first OSC title from
                    // clobbering the saved one.
                    spec.title.clone_into(&mut tab.title);
                    spec.title_override.clone_into(&mut tab.title_override);
                    self.tabs.push(tab);
                    spawned += 1;
                }
            }
            if let session::RestoreOutcome::Restored { active } =
                session::restore_outcome(plan.active, spawned)
            {
                self.active_tab = active;
                return;
            }
            // Every spawn failed: fall through to the
            // single-default-tab path below.
        }
        if let Some(tab) = self.spawn_tab(None) {
            // Placement is moot for the never-empty respawn's first tab:
            // the list is empty by construction, so both positions
            // coincide — `true` just names the shipped default.
            self.active_tab = tabs::insert_new_tab(&mut self.tabs, self.active_tab, true, tab);
        }
    }

    fn show(&mut self) -> Task<Message> {
        if !matches!(self.surface, SurfaceState::Hidden) {
            return Task::none();
        }
        // The user can see the terminal again: re-arm every tab's bell
        // notification latch, so a bell from here on is fresh news
        // worth one notification per tab again.
        for tab in &mut self.tabs {
            tab.bell_notified = false;
        }
        self.ensure_tabs();
        let id = window::Id::unique();
        self.surface = SurfaceState::Measuring(id);
        self.surface_focused = false;
        get_layer_surface(surface::measuring(id))
    }

    fn hide(&mut self) -> Task<Message> {
        // Hiding the surface is a focus loss for the rename input:
        // commit before the content slides out.
        let commit = self.commit_pending_rename();
        // The drawer survives the hide (spec criterion 8): its open
        // state and in-flight drafts stay exactly as they were for
        // the next show. The named no-op documents that at the seam
        // a hide would otherwise silently forget; only an explicit
        // close ever clears (see `close_settings_drawer`).
        self.settings_buffer
            .apply_drawer_transition(settings::DrawerTransition::FocusLossHide);
        // The slide toggle feeds the decision: a disabled slide turns
        // this hide into an immediate destroy inside `toggle_step` —
        // the executor arms below then simply never runs.
        let task = match toggle_step(
            ToggleAction::Hide,
            &self.surface,
            self.config.slide_animation_enabled,
        ) {
            ToggleStep::DestroyNow(id) => {
                // A measuring surface has no reshaped content to
                // animate: destroy immediately, no slide.
                self.slide = None;
                self.surface = SurfaceState::Hidden;
                destroy_layer_surface(id)
            }
            ToggleStep::SlideOut(id) => {
                // A shown surface slides out and its layer is
                // destroyed on completion. The out-slide starts from
                // wherever the content stands — the live eased offset
                // of an in-flight slide-in, or rest — so nothing
                // teleports. Its distance is the same deterministic
                // formula the show used, kept from the in-flight slide
                // when one exists: a config edit that arrived
                // mid-slide updates the config but not the frozen
                // geometry, so the in-flight distance is the one the
                // visible surface still has.
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "elapsed ms of one slide; u64 range is exhausted after 58M years"
                )]
                let (from_offset_px, distance_px) = match self.slide {
                    Some(playing) => (
                        slide::offset(
                            &playing.active,
                            playing.started.elapsed().as_millis() as u64,
                        ),
                        playing.active.distance_px,
                    ),
                    None => (0.0, self.slide_distance_px()),
                };
                self.surface = SurfaceState::Hiding(id);
                self.slide = Some(slide::Playing {
                    started: std::time::Instant::now(),
                    active: slide::Active {
                        direction: slide::Direction::Out,
                        from_offset_px,
                        distance_px,
                    },
                });
                // Keyboard interactivity releases at slide-out START,
                // not at destroy: the previously focused window
                // regains focus while the slide plays. The compositor
                // answers with an `Unfocused` layer event, which the
                // `Shown(id)` guard in its handler already rejects —
                // the surface is `Hiding` by the time it arrives.
                set_keyboard_interactivity(id, KeyboardInteractivity::None)
            }
            // `Hide` never decides to show, and a hiding or hidden
            // surface leaves with nothing: no restart, no double
            // destroy.
            ToggleStep::Measure | ToggleStep::ReverseShow(_) | ToggleStep::Leave => Task::none(),
        };
        // The surface is leaving on every destructive path, and the
        // cleanup is idempotent for `Leave`: any live grab-bar drag
        // is moot, the terminal pin's debounce state starts over so
        // the next show's first granted size adopts immediately, and
        // the active terminal reports the focus loss now — its PTY
        // keeps running regardless, because PTYs live in tabs, not
        // the window.
        self.drag = None;
        self.pending_height = None;
        self.pin_applied = None;
        // The context menu's popup dies with the surface and its
        // `on_close` never fires — clear the latch here, or the next
        // show's first right-click would be consumed as a
        // toggle-close of a menu that is not there.
        self.context_menu_open = false;
        if let Some(tab) = self.active() {
            tab.terminal.lock().unwrap().set_focused(false);
        }
        Task::batch([commit, task])
    }

    /// Reverses a hiding slide back to shown: the live surface is
    /// reused — no new measuring pass, `output_size` is retained (the
    /// `geometry` module's guarantee) — the slide re-targets `In`
    /// from the current offset (`slide::reverse`: no teleport, a
    /// fresh fixed 130 ms), keyboard interactivity is re-grabbed per
    /// config, the terminal reports back the focus it lost at hide
    /// start, and `surface_focused` resets to `false` pending the
    /// `LayerFocused` the re-grab synthesizes. PTYs never noticed the
    /// hide and keep running throughout.
    fn reverse_show(&mut self, id: window::Id) -> Task<Message> {
        self.surface = SurfaceState::Shown(id);
        self.surface_focused = false;
        // `Hiding` always carries a live `Out` slide — the parking
        // tick destroys the surface and leaves `Hidden` in the same
        // handler — so there is always something to re-target; a
        // slide-less degenerate would simply show at rest. An `Out`
        // slide that already ran past its duration reverses from the
        // endpoint (the full distance), i.e. a fresh full slide-in.
        if let Some(playing) = self.slide {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "elapsed ms of one slide; u64 range is exhausted after 58M years"
            )]
            let reversed = slide::Playing {
                started: std::time::Instant::now(),
                active: slide::reverse(
                    &playing.active,
                    playing.started.elapsed().as_millis() as u64,
                ),
            };
            self.slide = Some(reversed);
        }
        // The mirror of hide's focus bookkeeping: report the terminal
        // focused again — `set_focused` only reports real changes, so
        // the `LayerFocused` this re-grab synthesizes can repeat it
        // harmlessly.
        if let Some(tab) = self.active() {
            tab.terminal.lock().unwrap().set_focused(true);
        }
        Task::batch([
            set_keyboard_interactivity(
                id,
                surface::keyboard_interactivity(self.config.keyboard_mode),
            ),
            widget::text_input::focus(self.terminal_id.clone()),
        ])
    }

    /// The slide distance for the currently shown surface: the same
    /// deterministic `geometry::surface_height` formula `SurfaceOpened`
    /// used — deliberately not the granted window height, which
    /// double-reports through `WindowResized`. A shown surface always
    /// has a measured output beside it; the `0.0` fallback covers a
    /// state that cannot occur and still hides (a degenerate at-rest
    /// out-slide that parks, then destroys).
    fn slide_distance_px(&self) -> f32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "output px are < 2^24, exact in f32"
        )]
        self.output_size.map_or(0.0, |output| {
            geometry::surface_height(
                output.1 as f32,
                Geometry::from_config(&self.config).height_percent,
            ) as f32
        })
    }

    /// Handles the surface losing keyboard focus: marks the terminal unfocused
    /// and, when configured, hides the overlay. The hide is guarded by the
    /// focus latch so a stray unfocus before any focus (possible with
    /// `keyboard_mode: OnDemand`) can't close a freshly-opened surface. The
    /// latch is cleared only when the hide fires; a loss that doesn't hide
    /// (the config off) leaves it armed. No settings-window case exists
    /// here: while that window is open the surface is hidden by
    /// construction, so focus-loss events cannot arrive for a shown
    /// surface in that state.
    fn focus_lost(&mut self) -> Task<Message> {
        // Losing keyboard focus commits a rename: the input's own
        // unfocus signal only covers Escape and Tab, which cancel.
        let commit = self.commit_pending_rename();
        if let Some(tab) = self.active() {
            tab.terminal.lock().unwrap().set_focused(false);
        }
        if focus_loss_hides(self.config.hide_on_focus_loss) && self.surface_focused {
            self.surface_focused = false;
            return Task::batch([commit, self.hide()]);
        }
        commit
    }

    /// The geometry a settled grab-bar drag leaves behind: the current
    /// config's geometry with the height quantized once from where the
    /// pointer released — the *latest* target, not the last one the
    /// cadence gate landed, so throttling never decides the final
    /// height. The settled surface then matches the persisted percent,
    /// at most half a percent off the dragged pixels (accepted
    /// tolerance).
    fn settled_geometry(&self, drag: &Drag, output: (u32, u32)) -> Geometry {
        let final_target =
            geometry::drag_target_px(self.config.edge, output.1, drag.anchor, drag.pointer_y);
        let mut geom = Geometry::from_config(&self.config);
        geom.height_percent = geometry::percent_from_px(output.1, final_target);
        geom
    }

    /// Apply a live drag target now: the cadence gate has passed.
    /// Records the apply, offers the height to the terminal pin —
    /// adoptions are debounced (see `offer_window_height`), because
    /// the runtime double-reports every `set_size` (once stale, once
    /// real) and each adoption costs a terminal reflow plus a pty
    /// resize that made the drag sluggish on hardware — and sends the
    /// single-path triple with the pixel height.
    fn apply_drag_target(
        &mut self,
        id: window::Id,
        output: (u32, u32),
        target: u32,
    ) -> Task<Message> {
        if let Some(drag) = self.drag.as_mut() {
            drag.applied = Some((std::time::Instant::now(), target));
            drag.anchor.applied_px = target;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "surface px are < 2^24, exact in f32"
        )]
        let pin_task = self.offer_window_height(target as f32);
        let geom = Geometry::from_config(&self.config);
        let apply = geometry::apply_drag_geometry(id, geom, output, target);
        match pin_task {
            Some(t) => Task::batch([apply, t]),
            None => apply,
        }
    }

    /// Offer a height to the terminal pin: adopt immediately when the
    /// debounce allows (first size, re-report of the adopted height,
    /// or interval elapsed), else remember it and return the
    /// trailing-adopt timer that lands it once the granted stream
    /// settles. Every granted-size follower goes through here —
    /// `WindowResized` echoes outside drags and the drag's own
    /// applies — so the double-reported stream can never reflow the
    /// terminal at the apply rate.
    fn offer_window_height(&mut self, height: f32) -> Option<Task<Message>> {
        let now = std::time::Instant::now();
        if geometry::window_pin_due(self.pin_applied, now, height) {
            self.window_height = Some(height);
            // Re-adopting the height the pin already has must not
            // refresh the adoption clock (see `window_pin_due`): the
            // echo stream would otherwise starve changed heights.
            #[expect(
                clippy::float_cmp,
                reason = "echo detection: the compositor returns the exact f32 it was granted"
            )]
            let pinned = match self.pin_applied {
                Some((at, adopted)) if adopted == height => (at, height),
                _ => (now, height),
            };
            self.pin_applied = Some(pinned);
            self.pending_height = None;
            return None;
        }
        if self.pending_height != Some(height) {
            self.pending_height = Some(height);
            return Some(Task::perform(
                tokio::time::sleep(geometry::WINDOW_PIN_INTERVAL),
                |()| cosmic::Action::App(Message::WindowPinDue),
            ));
        }
        None
    }

    /// The one path every config write goes through: the drag release
    /// persists here as `PreApplied` — the set plus the `last_written`
    /// record whose `ConfigChanged` echo the update loop absorbs — and
    /// every settings control persists here as `Bare`, recording
    /// nothing so its echo flows into the live-apply path.
    /// Newest intent wins: a bare write of the drag's key first
    /// cancels any pending debounced write-back, so the stale
    /// `WriteDue` finds nothing and the bare write's value is what
    /// lands — while a drag release after it simply schedules fresh
    /// and lands last. Values clamp before persisting; a missing
    /// config backend (it failed to open at init) logs and skips —
    /// the drag itself still resized live.
    fn write_config(&mut self, kind: settings::WriteKind, write: &settings::PlannedWrite) {
        if kind == settings::WriteKind::Bare
            && settings::bare_write_cancels_pending_drag(write.key(), self.write_back.has_pending())
        {
            self.write_back.cancel_pending();
        }
        let write = write.clamped();
        let key = write.key();
        let Some(handler) = self.config_handler.as_ref() else {
            log::warn!("no config handler; skipping {} write", key.key_name());
            return;
        };
        match write.persist(handler) {
            Ok(()) => {
                // Only a pre-applied write absorbs its echo — geometry
                // was already applied at release, so the echo must not
                // reshape. The pre-applied writer is exactly the
                // height-only drag release. A bare write records
                // nothing: its echo *is* the apply.
                if let (
                    settings::WriteKind::PreApplied,
                    settings::PlannedWrite::HeightPercent(height),
                ) = (kind, &write)
                {
                    let mut written = Geometry::from_config(&self.config);
                    written.height_percent = *height;
                    self.last_written = Some(written);
                }
            }
            Err(err) => log::warn!("failed to persist {}: {err}", key.key_name()),
        }
    }

    fn debounced_toggle(&mut self, action: ToggleAction) -> Task<Message> {
        let now = std::time::Instant::now();
        if let Some(last) = self.last_toggle
            && now.duration_since(last) < TOGGLE_DEBOUNCE
        {
            return Task::none();
        }
        self.last_toggle = Some(now);

        // A remote close-tab is a quiet state edit, not a toggle
        // step: it applies through [`Self::close_tab_at`] and shows
        // nothing — no overlay, and nothing re-shows — so
        // a script closing a tab never disturbs what the user is
        // looking at. The debounce above still guards it: a
        // compositor double-delivery must not close two tabs.
        if let ToggleAction::CloseTab { tab_index } = action {
            return self.close_remote_tab(tab_index);
        }

        // A remote rename-tab is a quiet state edit like close-tab:
        // the title lands on the target — overriding or clearing —
        // with no overlay and none of the settings close-first rule
        // below. The debounce above still guards it: a compositor
        // double-delivery must not apply the rename twice against a
        // stale target.
        if let ToggleAction::RenameTab { title, tab_index } = action {
            return self.rename_remote_tab(&title, tab_index);
        }

        // A remote select-tab is a tab-facing verb like a new-tab: a
        // resolved target activates — riding set_active_tab's
        // attention retirement, session-dirty mark, and
        // already-active no-op — and the overlay shows through the
        // same path a `new-tab` takes, mid-slide-out included. An
        // unresolvable target — no tabs, out of range — is a quiet
        // no-op like close-tab's: no activation and no surface half
        // (nothing re-shows; a script's failed select must not
        // disturb what the user is looking at). The debounce above
        // still guards it: a compositor double-delivery must not
        // count twice.
        if let ToggleAction::SelectTab { index } = action {
            return match self.select_remote_tab(index) {
                Some(activate) => Task::batch([activate, self.show_intent_surface()]),
                None => Task::none(),
            };
        }

        // A remote new-tab is not a toggle step: the tab opens — now
        // or once the channel bootstraps — and the overlay shows,
        // whatever state it was in. The surface half rides the show
        // path below, so a request arriving mid-slide-out reverses on
        // the live surface exactly like a `show` would.
        if let ToggleAction::NewTab { .. } = action {
            let open = self.open_remote_tab(action, self.config.new_tab_after_current);
            return Task::batch([open, self.show_intent_surface()]);
        }

        // A remote duplicate-tab is a tab-facing verb like a new-tab:
        // a fresh shell tab opens in the target's cwd — now or once
        // the channel bootstraps — and the overlay shows through the
        // same path. An unresolvable target never blocks it: the
        // spawn degrades to the bare new-tab inheritance.
        if let ToggleAction::DuplicateTab { tab_index } = action {
            let open = self.duplicate_remote_tab(tab_index);
            return Task::batch([open, self.show_intent_surface()]);
        }

        // A remote switch-profile-tab is a tab-facing verb like a
        // duplicate-tab: the result — the tab now running the
        // profile — is meant to be seen, so the overlay shows through
        // the same path whatever state it was in. The verb resolves
        // its inputs up front and rides the `Message::SwitchTabProfile`
        // seam — the same apply the tab context menu will use, and
        // the construction that keeps the variant live under
        // `-D warnings`. Unresolvable inputs warn and edit nothing —
        // unlike duplicate-tab there is no sensible bare fallback —
        // while the surface half still shows.
        if let ToggleAction::SwitchProfileTab { profile, tab_index } = action {
            // `profile::find`'s exact, case-sensitive, first-match
            // rule, resolved to the index the apply wants.
            let Some(profile_index) = self
                .config
                .profiles
                .iter()
                .position(|candidate| candidate.name == profile)
            else {
                log::warn!("switch-profile-tab: no profile named {profile}");
                return self.show_intent_surface();
            };
            // The target — absent means the active tab — resolves
            // like every remote tab verb's, strict 1-based.
            let Some(target) =
                tabs::remote_target_index(self.tabs.len(), tab_index, self.active_tab)
            else {
                log::warn!(
                    "switch-profile-tab: no tab at --tab-index {tab_index:?} ({} open)",
                    self.tabs.len()
                );
                return self.show_intent_surface();
            };
            let tab_id = self.tabs[target].id;
            return Task::batch([
                cosmic::task::message(cosmic::Action::App(Message::SwitchTabProfile(
                    tab_id,
                    profile_index,
                ))),
                self.show_intent_surface(),
            ]);
        }

        // Every toggle decision executes through `toggle_step` — most
        // importantly, a mid-hide toggle or `Show` reverses on the
        // live surface instead of being dropped. The debounce above
        // already collapses true spam (~6–7 effective toggles per
        // second at 150 ms, each a full reversal); the state machine
        // itself never drops or queues. The slide toggle rides in
        // with the decision: with it off, a shown surface destroys
        // now instead of sliding out; with it on, every hide slides.
        // The settings drawer, if open, rides along: it lives inside
        // the surface, so a hide preserves it and the next show
        // uncovers it exactly as it was (spec criterion 8).
        match toggle_step(action, &self.surface, self.config.slide_animation_enabled) {
            ToggleStep::Measure => self.show(),
            ToggleStep::DestroyNow(_) | ToggleStep::SlideOut(_) => self.hide(),
            ToggleStep::ReverseShow(id) => self.reverse_show(id),
            ToggleStep::Leave => Task::none(),
        }
    }

    /// Opens the tab a remote `new-tab` asked for, placing it per
    /// `after_current` (the `new_tab_after_current` config). At first
    /// launch the terminal event channel may not exist yet — the
    /// spawn defers, the request stashes, and the channel's arrival
    /// applies it right after the session restore (re-reading the
    /// config then, so a flipped toggle still applies to the stash).
    fn open_remote_tab(&mut self, action: ToggleAction, after_current: bool) -> Task<Message> {
        let ToggleAction::NewTab { .. } = action else {
            return Task::none();
        };
        if self.term_event_tx.is_none() {
            self.pending_remote_tab = Some(action);
            return Task::none();
        }
        self.spawn_remote_tab(action, after_current)
    }

    /// Spawns the requested remote tab: an explicit live `--cwd` wins,
    /// then the active tab's cwd, then the pty default; a `--cmd` runs
    /// as the tab's program instead of the shell; a `--profile`
    /// resolves against the config and runs under its id. The tab
    /// lands at the position `after_current` picks — after the active
    /// one, or at the end — becomes active, and — being a layout
    /// change — marks the session dirty.
    fn spawn_remote_tab(&mut self, action: ToggleAction, after_current: bool) -> Task<Message> {
        let ToggleAction::NewTab { cwd, cmd, profile } = action else {
            return Task::none();
        };
        self.ensure_tabs();
        // Flag presence, not live-ness: an explicit --cwd guards the
        // plain-shell branch below even when the directory turns out
        // dead (the user asked for an override; a dead one warns and
        // degrades within that override, it does not fall back to the
        // default profile).
        let explicit_cwd = cwd.is_some();
        // A nonexistent --cwd degrades to the inherited/default cwd
        // with a warning, never an error surface.
        let explicit = cwd.filter(|dir| {
            let live = dir.is_dir();
            if !live {
                log::warn!("ignoring --cwd {}: not a directory", dir.display());
            }
            live
        });
        let inherited = self
            .active()
            .and_then(|tab| tab.terminal.lock().unwrap().working_directory());
        // `--cmd` wins its arm outright (the clap conflict keeps a
        // --cmd/--profile pairing off the wire); a profile resolves
        // against the live config before spawn — a name that no
        // longer exists warns and spawns the plain shell (ruling 3:
        // the fire-and-forget forward has no reply surface). An
        // explicit --cwd keeps the spawn a plain shell even with a
        // default profile set: the user overrode the preset
        // wholesale, and duplicate-tab rides exactly here (a fresh
        // shell in the target's cwd, never the default — ruling 1).
        // The bare remainder is the one branch the configured default
        // owns.
        let command = match (cmd, profile) {
            (Some(line), _) => SpawnCommand::CommandLine(line),
            (None, Some(name)) => {
                if let Some(profile) = profile::find(&self.config.profiles, &name) {
                    SpawnCommand::Profile {
                        name: profile.name.clone(),
                        command: profile.command.clone(),
                        cwd: profile.cwd.clone(),
                    }
                } else {
                    log::warn!("no profile named {name}; spawning the shell instead");
                    SpawnCommand::Shell(self.config.resolved_shell())
                }
            }
            (None, None) if explicit_cwd => SpawnCommand::Shell(self.config.resolved_shell()),
            (None, None) => default_spawn_command(&self.config),
        };
        // A profile tab's directory — by name or by default: the
        // explicit --cwd over the preset's live cwd over inheritance,
        // the preset degrading the way --cwd does (warn when dead,
        // then the inherited/default directory applies). Every other
        // arm resolves exactly as before.
        let spawn_cwd = match &command {
            SpawnCommand::Profile {
                name, cwd: preset, ..
            } => explicit
                .clone()
                .or_else(|| live_preset_cwd(name, preset.as_ref()))
                .or(inherited.clone()),
            _ => tabs::resolve_new_tab_cwd(explicit, inherited),
        };
        if let Some(tab) = self.spawn_shell(&command, spawn_cwd) {
            let at = tabs::insert_new_tab(&mut self.tabs, self.active_tab, after_current, tab);
            let switch = self.set_active_tab(at);
            return Task::batch([switch, self.mark_session_dirty()]);
        }
        Task::none()
    }

    /// Applies a remote `close-tab`: the 1-based `--tab-index`
    /// resolves through [`tabs::remote_target_index`] — absent means
    /// the active tab — and the close rides the shared
    /// [`Self::close_tab_at`], inheriting successor activation,
    /// respawn-when-last, attention retirement, the session-dirty
    /// mark, and the rename-session prune. An unresolvable target —
    /// no tabs, out of range — warns and no-ops: the fire-and-forget
    /// D-Bus call has no reply to carry the failure, so degradation
    /// is quiet.
    fn close_remote_tab(&mut self, tab_index: Option<usize>) -> Task<Message> {
        if let Some(index) = tabs::remote_target_index(self.tabs.len(), tab_index, self.active_tab)
        {
            self.close_tab_at(index)
        } else {
            log::warn!(
                "close-tab: no tab at --tab-index {tab_index:?} ({} open)",
                self.tabs.len()
            );
            Task::none()
        }
    }

    /// Applies a remote `select-tab`: the strict 1-based `N` resolves
    /// through [`tabs::select_tab_target`] — the wire's 1-based
    /// indices converted once, at this boundary — and the activation
    /// rides the shared [`Self::set_active_tab`], inheriting attention
    /// retirement, the session-dirty mark, and the already-active
    /// no-op. An unresolvable target — no tabs, out of range — warns
    /// and no-ops: the fire-and-forget D-Bus call has no reply to
    /// carry the failure, so degradation is quiet. The `None` return
    /// tells the caller to skip the surface half too.
    fn select_remote_tab(&mut self, one_based: usize) -> Option<Task<Message>> {
        if let Some(index) = tabs::select_tab_target(self.tabs.len(), one_based) {
            Some(self.set_active_tab(index))
        } else {
            log::warn!(
                "select-tab: no tab at index {one_based} ({} open)",
                self.tabs.len()
            );
            None
        }
    }

    /// Applies a remote `rename-tab`: the target resolves like
    /// close-tab's — absent means the active tab — through
    /// [`tabs::remote_target_index`]. Any inline rename session on
    /// that tab ends first with its draft discarded — a pending draft
    /// must not commit over the remote title — then
    /// [`tabs::commit_rename`] applies the payload: a non-empty TITLE
    /// sets the sticky override shell OSC titles never clobber, the
    /// empty TITLE clears it so shell titles resume. Quiet like
    /// close-tab, and the session-dirty mark persists titles. An
    /// unresolvable target warns and no-ops.
    fn rename_remote_tab(&mut self, title: &str, tab_index: Option<usize>) -> Task<Message> {
        if let Some(index) = tabs::remote_target_index(self.tabs.len(), tab_index, self.active_tab)
        {
            let refocus = self.end_rename_on(index);
            self.commit_rename_on(index, title);
            Task::batch([refocus, self.mark_session_dirty()])
        } else {
            log::warn!(
                "rename-tab: no tab at --tab-index {tab_index:?} ({} open)",
                self.tabs.len()
            );
            Task::none()
        }
    }

    /// Applies a remote `duplicate-tab`: the target — absent means the
    /// active tab — resolves through [`tabs::remote_target_index`],
    /// and a fresh shell tab opens in that tab's cwd; the running
    /// command is never copied. An explicit but unresolvable
    /// `--tab-index` warns, and the spawn degrades to the bare
    /// new-tab inheritance — no explicit cwd, so resolution takes the
    /// active tab's cwd, then the pty default. The spawn itself rides
    /// the existing new-tab path end to end
    /// ([`Self::open_remote_tab`]): the channel-bootstrap deferral,
    /// landing after the active tab, activation, and the session-dirty
    /// mark are all inherited. The overlay half is the caller's.
    fn duplicate_remote_tab(&mut self, tab_index: Option<usize>) -> Task<Message> {
        let target = tabs::remote_target_index(self.tabs.len(), tab_index, self.active_tab);
        if target.is_none() && tab_index.is_some() {
            log::warn!(
                "duplicate-tab: no tab at --tab-index {tab_index:?} ({} open); duplicating the active tab instead",
                self.tabs.len()
            );
        }
        let cwd = target.and_then(|index| {
            self.tabs
                .get(index)
                .and_then(|tab| tab.terminal.lock().unwrap().working_directory())
        });
        // `true` pins the duplicate's documented after-active landing:
        // the `new_tab_after_current` setting moves new tabs, never
        // duplicates.
        self.open_remote_tab(
            ToggleAction::NewTab {
                cwd,
                cmd: None,
                profile: None,
            },
            true,
        )
    }

    /// Applies an in-place profile switch: the tab identified by `tab`
    /// keeps its slot in the list, its activation state, and its sticky
    /// user rename, while its terminal is replaced by a fresh PTY
    /// running the profile `profile_index` resolves to — the seam the
    /// remote `switch-profile-tab` rides through
    /// [`Message::SwitchTabProfile`]. Unresolvable inputs warn and
    /// no-op, never degrading into a spawn: an unknown tab id, or a
    /// stale profile index (a config edited while a menu snapshot was
    /// open — the `NewProfileTab` precedent). The respawn cwd composes
    /// like a profile launch's — the preset's live cwd
    /// (warn-and-degrade when dead), else the tab's own working
    /// directory read before any teardown — with the tab itself
    /// replacing the "active tab" as the inheritance source. The
    /// replacement takes a fresh [`TabId`]: `Terminal::drop` sends
    /// `Msg::Shutdown` asynchronously, so the dying PTY can still have
    /// events in flight, and those late events then find no tab
    /// carrying the old id (`handle_term_event` drops unknown ids —
    /// the `close_tab` safety). The cosmetic cost — an unrenamed tab's
    /// default title renumbers — is accepted. A respawn is a save
    /// trigger: the next session save records the profile (and the
    /// surviving rename), and a restore re-resolves the tab as that
    /// profile.
    fn switch_tab_profile(&mut self, tab: TabId, profile_index: usize) -> Task<Message> {
        let Some(index) = self.tabs.iter().position(|candidate| candidate.id == tab) else {
            log::warn!(
                "switch profile: no tab with id {:?} ({} open)",
                tab,
                self.tabs.len()
            );
            return Task::none();
        };
        let Some(command) = profile_pick(&self.config, profile_index) else {
            log::warn!(
                "switch profile: no profile at index {profile_index} ({} configured)",
                self.config.profiles.len()
            );
            return Task::none();
        };
        let SpawnCommand::Profile {
            name, cwd: preset, ..
        } = &command
        else {
            unreachable!("profile_pick returns the Profile arm");
        };
        // The respawn cwd, read before any teardown: the preset's live
        // directory when it has one, else the tab's own — the
        // NewProfileTab composition with the target tab as the
        // inheritance source.
        let respawn_cwd = live_preset_cwd(name, preset.as_ref()).or_else(|| {
            self.tabs[index]
                .terminal
                .lock()
                .unwrap()
                .working_directory()
        });
        // An open inline rename on the target is discarded (the
        // rename-tab precedent): the draft must not commit over the
        // respawn, and the explicit end gives the deterministic
        // refocus below.
        let refocus = self.end_rename_on(index);
        let old_override = self.tabs[index].title_override.clone();
        let Some(SpawnedTerminal {
            tab_id,
            terminal,
            shell,
            ..
        }) = self.spawn_terminal(&command, respawn_cwd.as_deref())
        else {
            // Deferred (no event channel yet) or the spawn errored —
            // both already logged; the tab is untouched.
            return Task::none();
        };
        let plan = tabs::plan_respawn(
            self.config.tab_title_mode,
            tab_id,
            respawn_cwd.as_deref(),
            old_override.as_deref(),
            name,
        );
        let target = &mut self.tabs[index];
        // Every field the old terminal's life touched comes from the
        // plan — the profile id included: the tab is a profile tab
        // from now on, whatever it was before. (A degraded blank
        // command line still records the pick; the next restore
        // re-derives the pair through `spawn_shell`, which drops the
        // id for a degraded line, so a broken preset heals itself
        // after one cycle.)
        target.title = plan.title;
        target.shell_title = plan.shell_title;
        target.title_override = plan.title_override;
        target.attention = plan.attention;
        target.bell_notified = plan.bell_notified;
        target.ephemeral = plan.ephemeral;
        target.profile = plan.profile;
        target.shell = shell;
        // The id swap and terminal swap land together: assigning the
        // field drops the old `Terminal`, whose `Drop` sends
        // `Msg::Shutdown`, and late events from the dying PTY find no
        // tab carrying the old id.
        target.id = tab_id;
        target.terminal = Mutex::new(terminal);
        // The cwd cache follows the id swap: drop the dead tab's
        // entry, seed the new one with the respawn cwd so the fresh
        // shell's first OSC title report reads as unchanged.
        self.tab_cwds.remove(&tab);
        self.tab_cwds.insert(tab_id, respawn_cwd);
        Task::batch([refocus, self.mark_session_dirty()])
    }

    /// Ends an inline rename session sitting on the tab at `index`:
    /// the draft is discarded — it must not commit over a title
    /// arriving from outside — and focus returns to the terminal as
    /// the input overlay goes away. No-op when no session targets
    /// that tab.
    fn end_rename_on(&mut self, index: usize) -> Task<Message> {
        let session_on_target = self
            .tabs
            .get(index)
            .is_some_and(|tab| self.rename.as_ref().is_some_and(|edit| edit.tab == tab.id));
        if session_on_target {
            self.rename = None;
            return widget::text_input::focus(self.terminal_id.clone());
        }
        Task::none()
    }

    /// The surface half a tab-facing remote verb takes: the same show
    /// decision a `new-tab` gets, so the verb shows the overlay
    /// whatever state it was in — and a request arriving
    /// mid-slide-out reverses on the live surface exactly like a
    /// `show` would.
    fn show_intent_surface(&mut self) -> Task<Message> {
        match toggle_step(
            ToggleAction::Show,
            &self.surface,
            self.config.slide_animation_enabled,
        ) {
            ToggleStep::Measure => self.show(),
            ToggleStep::ReverseShow(id) => self.reverse_show(id),
            _ => Task::none(),
        }
    }

    /// The one settings trigger (spec criteria 5 and 7): the
    /// keybind, the context-menu "Settings…" item, the tab-bar
    /// settings button, and the drawer's own Close button all route
    /// through `Action::Settings` into here. Opening flips the
    /// drawer over the live terminal — the surface never hides or
    /// resizes (criterion 1); a repeat press closes (criterion 5).
    fn toggle_settings(&mut self) -> Task<Message> {
        if self.settings_open {
            self.close_settings_drawer()
        } else {
            self.settings_open = true;
            Task::none()
        }
    }

    /// Closes the settings drawer: the explicit close path (Escape,
    /// the toggle's repeat press, the drawer's Close button), so the
    /// buffer clears per the close-clears-vs-hide-preserves rule
    /// (spec criterion 8 — a focus-loss hide preserves everything for
    /// the next show; hides touch nothing) and keyboard focus returns
    /// to the terminal the drawer had borrowed (cosmic-term's
    /// `on_context_drawer` refocus).
    fn close_settings_drawer(&mut self) -> Task<Message> {
        self.settings_open = false;
        self.settings_buffer
            .apply_drawer_transition(settings::DrawerTransition::ExplicitClose);
        widget::text_input::focus(self.terminal_id.clone())
    }

    fn perform_action(&mut self, action: Action) -> Task<Message> {
        // Hiding must work even before a terminal exists, or a fresh
        // `cargo run` opens a surface with no way to close it.
        if action == Action::Hide {
            return self.hide();
        }
        // The settings drawer toggles from the keybind alone — no
        // active tab required — and never targets the active
        // terminal.
        if action == Action::Settings {
            return self.toggle_settings();
        }
        // The tab actions reshape the tab list itself — including keeping
        // it non-empty — so they run before the active-tab borrow below
        // and never reach the per-terminal actions.
        if matches!(
            action,
            Action::NewTab
                | Action::NewProfileTab(_)
                | Action::CloseTab
                | Action::NextTab
                | Action::PrevTab
                | Action::ActivateTab(_)
                | Action::ActivateLastTab
                | Action::MoveTabLeft
                | Action::MoveTabRight
        ) {
            return self.perform_tab_action(action);
        }
        let Some(tab) = self.active() else {
            return Task::none();
        };
        match action {
            Action::Hide
            | Action::Settings
            | Action::NewTab
            | Action::NewProfileTab(_)
            | Action::CloseTab
            | Action::NextTab
            | Action::PrevTab
            | Action::ActivateTab(_)
            | Action::ActivateLastTab
            | Action::MoveTabLeft
            | Action::MoveTabRight => unreachable!("handled above"),
            Action::Copy => {
                let selected = tab
                    .terminal
                    .lock()
                    .unwrap()
                    .term
                    .lock()
                    .selection_to_string();
                if let Some(text) = selected {
                    return clipboard::write(text);
                }
            }
            Action::CopyPrimary => {
                let selected = tab
                    .terminal
                    .lock()
                    .unwrap()
                    .term
                    .lock()
                    .selection_to_string();
                if let Some(text) = selected {
                    return clipboard::write_primary(text);
                }
            }
            Action::Paste => {
                return clipboard::read()
                    .map(|text| cosmic::Action::App(Message::ClipboardPasted(text)));
            }
            Action::PastePrimary => {
                return clipboard::read_primary()
                    .map(|text| cosmic::Action::App(Message::ClipboardPastedPrimary(text)));
            }
            Action::SelectAll => {
                tab.terminal.lock().unwrap().select_all();
            }
            Action::ClearScrollback => {
                tab.terminal
                    .lock()
                    .unwrap()
                    .term
                    .lock()
                    .grid_mut()
                    .clear_history();
            }
            Action::ZoomIn | Action::ZoomOut | Action::ZoomReset => {
                let mut terminal = tab.terminal.lock().unwrap();
                let adj = match action {
                    Action::ZoomIn => terminal.zoom_adj().saturating_add(1),
                    Action::ZoomOut => terminal.zoom_adj().saturating_sub(1),
                    _ => 0,
                };
                terminal.set_zoom_adj(adj);
                terminal.update_cell_size();
            }
        }
        Task::none()
    }

    /// Applies a tab-list keybind action: open, close, or cycle tabs.
    ///
    /// These reshape `tabs` — upholding the never-empty invariant by
    /// respawning after the last tab closes — rather than targeting the
    /// active terminal, so they live apart from the per-terminal actions
    /// in [`Self::perform_action`].
    fn perform_tab_action(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::NewTab => {
                // A new tab inherits the active tab's cwd: the same
                // `/proc/<pid>/cwd` readlink the session save samples,
                // so `Ctrl+Shift+T` after a `cd` lands in the same
                // directory. A failed read — no active tab, non-Linux,
                // an unreadable `/proc` entry — degrades to the pty's
                // default directory.
                let inherited = self
                    .active()
                    .and_then(|tab| tab.terminal.lock().unwrap().working_directory());
                // The plain keybind rides the configured default
                // (ruling 1): with one set and existing, the new tab
                // is a full profile tab — id recorded, session-
                // persisted — in the inherited directory unless the
                // preset's own live cwd applies. No effective default:
                // exactly today's shell spawn.
                let command = default_spawn_command(&self.config);
                let spawn_cwd = match &command {
                    SpawnCommand::Profile {
                        name, cwd: preset, ..
                    } => live_preset_cwd(name, preset.as_ref()).or(inherited),
                    _ => tabs::resolve_new_tab_cwd(None, inherited),
                };
                return self.open_new_tab(&command, spawn_cwd);
            }
            Action::NewProfileTab(index) => {
                // A submenu pick: the index came from the menu-open
                // snapshot of `config.profiles`, so a config edited
                // while the menu was open can leave it stale — warn
                // and no-op rather than launch the wrong profile.
                if let Some(command) = profile_pick(&self.config, index) {
                    let inherited = self
                        .active()
                        .and_then(|tab| tab.terminal.lock().unwrap().working_directory());
                    // The preset's cwd composes exactly like the
                    // keybind arm's: live or warn-and-degrade, then
                    // the inherited directory.
                    let spawn_cwd = match &command {
                        SpawnCommand::Profile {
                            name, cwd: preset, ..
                        } => live_preset_cwd(name, preset.as_ref()).or(inherited),
                        // profile_pick only ever builds the Profile arm.
                        _ => unreachable!("profile_pick returns the Profile arm"),
                    };
                    return self.open_new_tab(&command, spawn_cwd);
                }
                log::warn!(
                    "new profile tab: no profile at submenu index {index} \
                     ({} configured)",
                    self.config.profiles.len()
                );
            }
            Action::CloseTab => return self.close_tab_at(self.active_tab),
            Action::NextTab => {
                return self.set_active_tab(tabs::cycle_next(&self.tabs, self.active_tab));
            }
            Action::PrevTab => {
                return self.set_active_tab(tabs::cycle_prev(&self.tabs, self.active_tab));
            }
            Action::ActivateTab(requested) => {
                // Strict numbered activation: out of range never
                // wraps; set_active_tab no-ops when the tab is already
                // active.
                if let Some(index) = tabs::activate_index_target(self.tabs.len(), requested) {
                    return self.set_active_tab(index);
                }
            }
            Action::ActivateLastTab => {
                // Ctrl+Shift+9 takes the last tab however many exist
                // (cosmic-term TabActivate8 semantics).
                if let Some(index) = tabs::activate_last_target(self.tabs.len()) {
                    return self.set_active_tab(index);
                }
            }
            Action::MoveTabLeft | Action::MoveTabRight => {
                // One slot toward the direction, clamping at the edge;
                // the moved tab stays active, so no focus handoff.
                // Like every layout change, the move marks the session
                // dirty.
                let direction = match action {
                    Action::MoveTabLeft => tabs::MoveDirection::Left,
                    _ => tabs::MoveDirection::Right,
                };
                if let Some(to) = tabs::move_target(self.active_tab, self.tabs.len(), direction) {
                    self.active_tab = tabs::move_tab(&mut self.tabs, self.active_tab, to);
                    return self.mark_session_dirty();
                }
            }
            _ => unreachable!("not a tab action: {action:?}"),
        }
        Task::none()
    }

    /// The shared tail of every new-tab spawn — keybind, submenu
    /// pick, remote action: spawn, place per `new_tab_after_current`,
    /// activate, and mark the session dirty (the open itself is a
    /// save trigger; when the activation's switch already armed a
    /// window, this second mark coalesces into it).
    fn open_new_tab(&mut self, command: &SpawnCommand, cwd: Option<PathBuf>) -> Task<Message> {
        if let Some(tab) = self.spawn_shell(command, cwd) {
            // Placement: after the active tab or at the end, per
            // `new_tab_after_current`; the new tab becomes active
            // either way.
            let at = tabs::insert_new_tab(
                &mut self.tabs,
                self.active_tab,
                self.config.new_tab_after_current,
                tab,
            );
            let switch = self.set_active_tab(at);
            Task::batch([switch, self.mark_session_dirty()])
        } else {
            Task::none()
        }
    }

    /// Closes the tab at `index` and activates its successor (or, when
    /// it closed last, its predecessor). A close that took the final
    /// tab immediately spawns a fresh one — the overlay is never left
    /// empty (see `ensure_tabs`). Shared by the close-tab keybind and
    /// by shell exits; `set_focused` reports only real focus changes,
    /// so refocusing a tab that already had focus is a no-op.
    fn close_tab_at(&mut self, index: usize) -> Task<Message> {
        let closed = self.tabs.get(index).map(|tab| tab.id);
        if let Some(decision) = tabs::close_tab(&mut self.tabs, index, self.active_tab) {
            self.active_tab = decision.next_active;
            // The tab activation a close implies also retires the
            // newly active tab's attention marker, exactly like a
            // click or a numbered-switch bind would.
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.attention = tab.attention.on_activated();
            }
            if decision.respawn {
                self.ensure_tabs();
            }
            self.focus_active_tab();
            if let Some(id) = closed {
                self.tab_cwds.remove(&id);
            }
            // Every close comes through here — keybind, tab-bar `×`,
            // middle-click, and a shell exit (`TermEvent::Exit`) — so
            // this is the close trigger's single save-marking site.
            let mark = self.mark_session_dirty();
            // A close can orphan the rename input: its tab may be
            // gone, or the bar may have vanished with a single tab
            // left. End the session so the terminal regains keyboard
            // focus.
            return Task::batch([mark, self.prune_rename_session()]);
        }
        self.prune_rename_session()
    }

    /// Commits the active inline rename, if any: the draft becomes
    /// the tab's user rename, the session ends, and focus returns to
    /// the terminal. Every committing focus-loss path comes through
    /// here — Enter, a press outside the input, the surface hiding or
    /// losing keyboard focus — while Escape and Tab cancel instead
    /// ([`Message::RenameCancelled`]).
    fn commit_pending_rename(&mut self) -> Task<Message> {
        if let Some(edit) = self.rename.take() {
            if let Some(index) = self.tabs.iter().position(|tab| tab.id == edit.tab) {
                self.commit_rename_on(index, &edit.draft);
            }
            // A rename commit is a save trigger — but only when a
            // rename session actually existed; every committing path
            // comes through here, so this is its single marking site.
            let mark = self.mark_session_dirty();
            return Task::batch([mark, widget::text_input::focus(self.terminal_id.clone())]);
        }
        Task::none()
    }

    /// Ends the active rename when its input can no longer be shown: the
    /// renamed tab is gone, or the bar is hidden now that a single tab
    /// remains. Returns the task refocusing the terminal, if a
    /// session ended.
    fn prune_rename_session(&mut self) -> Task<Message> {
        if let Some(edit) = &self.rename {
            let shown = self.tabs.len() > 1 && self.tabs.iter().any(|tab| tab.id == edit.tab);
            if !shown {
                self.rename = None;
                return widget::text_input::focus(self.terminal_id.clone());
            }
        }
        Task::none()
    }

    /// Switches the active tab to `index`, reporting the focus change to
    /// both shells (`FOCUS_OUT` to the outgoing one, `FOCUS_IN` to the
    /// incoming one), the same way a window focus change does. A real
    /// switch marks the session dirty — the focused index is saved
    /// state; the index-unchanged early return never marks.
    fn set_active_tab(&mut self, index: usize) -> Task<Message> {
        // Cycling with a single tab is a no-op; refocusing it anyway would
        // send the shell a spurious FOCUS_OUT/FOCUS_IN pair.
        if index == self.active_tab {
            return Task::none();
        }
        if let Some(old) = self.active() {
            old.terminal.lock().unwrap().set_focused(false);
        }
        self.active_tab = index;
        // Activation retires whatever attention marker the tab wore:
        // the user is now looking at it.
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.attention = tab.attention.on_activated();
        }
        self.focus_active_tab();
        self.mark_session_dirty()
    }

    /// Marks the active tab's terminal focused. Used after the tab list
    /// changed shape, where the tab that lost focus is already gone.
    fn focus_active_tab(&mut self) {
        if let Some(tab) = self.active() {
            tab.terminal.lock().unwrap().set_focused(true);
        }
    }

    /// Marks the session dirty and arms the debounce window: at most
    /// one `tokio::sleep` is ever in flight — a mark while a window
    /// is armed coalesces into that window's write — and only a mark
    /// after a write re-arms. Returns the window's timer task when
    /// this call armed it, the `WindowPinDue`/`WriteDue` pattern.
    fn mark_session_dirty(&mut self) -> Task<Message> {
        // The session toggle gates every save trigger: a disabled
        // trigger no-ops — no dirty mark, no debounce window, no
        // timer task — so a mid-run disable stops further saves
        // immediately.
        match self
            .session_save
            .mark_gated(self.config.session_persistence_enabled)
        {
            Some(generation) => Task::perform(
                tokio::time::sleep(session::SESSION_WRITE_INTERVAL),
                move |()| cosmic::Action::App(Message::SessionSaveDue(generation)),
            ),
            None => Task::none(),
        }
    }

    /// Captures the live tab list and writes the session file
    /// atomically. The cwd readlinks happen here — at write time, not
    /// schedule time — so saved cwds are as fresh as the debounce
    /// allows; `capture` then staleness-filters each one. A failed
    /// write logs and leaves the old file in place: the next dirty
    /// window retries.
    fn write_session(&mut self) {
        let cwds: Vec<Option<PathBuf>> = self
            .tabs
            .iter()
            .map(|tab| tab.terminal.lock().unwrap().working_directory())
            .collect();
        let snapshots: Vec<session::TabSnapshot<'_>> = self
            .tabs
            .iter()
            .zip(&cwds)
            .map(|(tab, cwd)| session::TabSnapshot {
                title: &tab.title,
                title_override: tab.title_override.as_deref(),
                shell: &tab.shell,
                cwd: cwd.as_deref(),
                ephemeral: tab.ephemeral,
                profile: tab.profile.as_deref(),
            })
            .collect();
        let state = session::capture(&snapshots, self.active_tab);
        if let Err(err) = session::save_atomic(&session::state_file_path(), &state) {
            log::warn!("failed to write session file: {err}");
        }
    }

    /// The cwd-change gate on a shell title event: one readlink,
    /// compared against the tab's cached cwd. A moved cwd marks the
    /// session dirty and updates the cache; a title churn with the
    /// cwd holding — a vim statusline — stays quiet.
    fn check_tab_cwd(&mut self, tab_index: usize) -> Task<Message> {
        let tab_id = self.tabs[tab_index].id;
        let cwd = self.tabs[tab_index]
            .terminal
            .lock()
            .unwrap()
            .working_directory();
        if session::cwd_changed(
            self.tab_cwds.get(&tab_id).and_then(Option::as_deref),
            cwd.as_deref(),
        ) {
            self.tab_cwds.insert(tab_id, cwd);
            return self.mark_session_dirty();
        }
        Task::none()
    }

    /// Re-resolves one tab's displayed title from its stored state —
    /// the shared tail of every title apply point: a report arriving
    /// or resetting, a rename clearing, and the mode-flip retitle
    /// loop. The cwd comes from the `tab_cwds` cache, seeded at spawn
    /// and refreshed by [`Self::check_tab_cwd`] — which is why the
    /// report handlers sample the cwd *before* resolving.
    fn retitle_tab(&mut self, tab_index: usize) {
        let mode = self.config.tab_title_mode;
        if let Some(tab) = self.tabs.get_mut(tab_index) {
            let cwd = self.tab_cwds.get(&tab.id).and_then(Option::as_deref);
            tab.title = tabs::resolve_title(
                mode,
                tab.shell_title.as_deref(),
                cwd,
                tab.id,
                tab.title_override.as_deref(),
            );
        }
    }

    /// A tab-title-mode change re-resolves every live tab's title
    /// from stored state (Guake `recompute_tabs_titles`): renamed
    /// tabs are left to [`tabs::resolve_title`]'s override rule,
    /// which keeps them untouched. Called only on an old-vs-new mode
    /// delta, so unrelated config echoes never churn titles.
    fn retitle_all_tabs(&mut self) {
        for index in 0..self.tabs.len() {
            self.retitle_tab(index);
        }
    }

    /// Applies a rename commit on one tab — both rename callers, the
    /// tab-bar inline rename and the remote `rename-tab`, meet here:
    /// the override is set or cleared and the displayed title
    /// re-resolves per the live mode (see [`tabs::commit_rename`]).
    fn commit_rename_on(&mut self, tab_index: usize, new_title: &str) {
        let mode = self.config.tab_title_mode;
        if let Some(tab) = self.tabs.get_mut(tab_index) {
            let cwd = self.tab_cwds.get(&tab.id).and_then(Option::as_deref);
            tabs::commit_rename(
                mode,
                tab.shell_title.as_deref(),
                cwd,
                tab.id,
                &mut tab.title,
                &mut tab.title_override,
                new_title,
            );
        }
    }
}

/// Resolves the effective default for a plain new tab (ruling 1): a
/// set, existing `default_profile` becomes the full profile spawn —
/// id recorded, session-persisted — while an unset or dangling one
/// keeps today's plain shell (the module-1 seam's contract: the load
/// never rewrites the config, and nothing chains onto another
/// profile). The plain-spawn sites — the new-tab keybind, a bare
/// remote `new-tab`, and the never-empty startup/respawn — all route
/// through here; `duplicate-tab` and every explicit CLI flag do not.
fn default_spawn_command(config: &DropTermConfig) -> SpawnCommand {
    match profile::effective_default(&config.profiles, config.default_profile.as_deref()) {
        Some(profile) => SpawnCommand::Profile {
            name: profile.name.clone(),
            command: profile.command.clone(),
            cwd: profile.cwd.clone(),
        },
        None => SpawnCommand::Shell(config.resolved_shell()),
    }
}

/// Resolves a context-menu profile pick — the submenu's index into
/// the config's profile list — into the spawn it runs: the Profile
/// arm with the preset's stored cwd (the call site composes the live
/// filter and inheritance on top). `None` when the index is stale, a
/// config edited while the menu was open: the caller warns and no-ops.
fn profile_pick(config: &DropTermConfig, index: usize) -> Option<SpawnCommand> {
    config
        .profiles
        .get(index)
        .map(|profile| SpawnCommand::Profile {
            name: profile.name.clone(),
            command: profile.command.clone(),
            cwd: profile.cwd.clone(),
        })
}

/// The live-directory filter a profile preset's cwd passes at spawn:
/// dead → warn and `None` — the `--cwd` degradation, never an error
/// surface — so the inherited/default directory applies instead.
fn live_preset_cwd(name: &str, cwd: Option<&PathBuf>) -> Option<PathBuf> {
    cwd.cloned().filter(|dir| {
        let live = dir.is_dir();
        if !live {
            log::warn!(
                "ignoring profile {name} cwd {}: not a directory",
                dir.display()
            );
        }
        live
    })
}

/// Whether a `ConfigChanged` may reshape a shown surface in place:
/// pure so the guard is headless-testable (constructing `DropTerm`
/// needs a live `Core`). The geometry must actually have changed; a
/// drag armed mid-edit defers the reshape to the release settle (so
/// config reshapes and drag deltas never interleave `set_size`
/// calls); a slide animating defers it to the next show — geometry
/// stays frozen while animating. The in-memory config still updates
/// on every path (the caller does that first); only the live reshape
/// is gated.
fn config_edit_reshapes(geometry_changed: bool, drag_armed: bool, slide_animating: bool) -> bool {
    geometry_changed && !drag_armed && !slide_animating
}

/// What a toggle-intent request must do with the surface: pure so
/// the decision is headless-testable (constructing `DropTerm` needs
/// a live `Core`). Every toggle caller goes through here — the
/// toggle hotkey and D-Bus activation (`debounced_toggle`), plus
/// `Ctrl+Shift+Q` and focus loss through `hide`'s `Hide` column —
/// so the full state × intent matrix lives in one place and
/// reversibility is just another cell. Slide activity needs no
/// separate input: it is implied by the state — an `Out` slide only
/// exists while `Hiding`, an `In` slide only rides a `Shown`
/// surface — and the executors read the live slide for offsets.
///
/// `slide_animation_enabled` gates transition *entry* only: it
/// swaps the `Shown`-hide cell between sliding out and destroying
/// now — the destroy *style*, never the direction. The helper takes
/// no mid-flight input by construction, so a running slide is never
/// aborted by its own toggle: the `Hiding` cells (destroy-on-
/// completion, reversal) key on state alone and ignore the flag
/// entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToggleStep {
    /// Run the measuring show pass (a hidden surface only): measure
    /// → reshape → shown, then slide in.
    Measure,
    /// Destroy immediately: a measuring surface has no reshaped
    /// content to animate, so there is nothing to slide.
    DestroyNow(window::Id),
    /// Slide out from the current offset and destroy on the slide's
    /// completion tick. Keyboard interactivity releases at slide-out
    /// start.
    SlideOut(window::Id),
    /// Reverse a hiding slide back to shown on the same live surface:
    /// no new measuring pass — `output_size` is retained — the slide
    /// re-targets `In` from the current offset, keyboard
    /// interactivity is re-grabbed per config, and `surface_focused`
    /// resets pending the next `LayerFocused`.
    ReverseShow(window::Id),
    /// Nothing: the intent is already satisfied — a hide while
    /// hiding or hidden (no restart, no double destroy), a show
    /// while measuring or shown.
    Leave,
}

fn toggle_step(action: ToggleAction, surface: &SurfaceState, slide_enabled: bool) -> ToggleStep {
    match (action, surface) {
        // Show intents: only a hidden surface starts the measuring
        // pass; a hiding one reverses; a measuring or shown one is
        // already on its way. None of these arms key on the slide
        // toggle — a hiding surface always reverses on the live
        // slide, whatever the flag says now (never aborted).
        (ToggleAction::Show | ToggleAction::Toggle, SurfaceState::Hidden) => ToggleStep::Measure,
        (ToggleAction::Show | ToggleAction::Toggle, SurfaceState::Hiding(id)) => {
            ToggleStep::ReverseShow(*id)
        }
        (ToggleAction::Show, SurfaceState::Measuring(_) | SurfaceState::Shown(_)) => {
            ToggleStep::Leave
        }
        // Hide intents — and a toggle on any visible surface: a
        // measuring surface dies now, a hiding or hidden one stays
        // put (idempotent — no restart, no double destroy). A shown
        // one slides out (mid-slide-in the executor starts from the
        // live eased offset) — or, with the slide toggle off,
        // destroys now: instant hide.
        (ToggleAction::Hide | ToggleAction::Toggle, SurfaceState::Measuring(id)) => {
            ToggleStep::DestroyNow(*id)
        }
        (ToggleAction::Hide | ToggleAction::Toggle, SurfaceState::Shown(id)) => {
            if slide_enabled {
                ToggleStep::SlideOut(*id)
            } else {
                ToggleStep::DestroyNow(*id)
            }
        }
        (ToggleAction::Hide, SurfaceState::Hiding(_) | SurfaceState::Hidden) => ToggleStep::Leave,
        // Structurally unreachable: `debounced_toggle` applies the
        // remote verbs — opening a new-tab, closing a close-tab,
        // selecting a select-tab, renaming a rename-tab, duplicating
        // a duplicate-tab, switching a switch-profile-tab — outside
        // the toggle step, so none ever gets here. Loud rather than
        // silently wrong.
        (
            ToggleAction::NewTab { .. }
            | ToggleAction::CloseTab { .. }
            | ToggleAction::SelectTab { .. }
            | ToggleAction::RenameTab { .. }
            | ToggleAction::DuplicateTab { .. }
            | ToggleAction::SwitchProfileTab { .. },
            _,
        ) => {
            unreachable!("remote verbs apply outside the funnel")
        }
    }
}

/// Whether a `LayerUnfocused` event for `id` should reach
/// [`DropTerm::focus_lost`]: only a shown surface reacts to losing
/// keyboard focus — one arriving while `Hiding` (the synthesized
/// unfocus of the hide's own keyboard release, or a racing focus
/// loss) is a no-op, so it can never restart or double-drive the
/// out-slide.
fn layer_unfocused_acts_on(surface: &SurfaceState, id: window::Id) -> bool {
    matches!(surface, SurfaceState::Shown(shown) if *shown == id)
}

/// Whether losing keyboard focus should hide the surface: exactly
/// the `hide_on_focus_loss` config flag. No settings-drawer input
/// is needed: focus loss may hide the surface with the drawer open —
/// the drawer and its drafts survive for the next show (spec
/// criterion 8) — so the gate runs the same in the drawer-open
/// state. Only the automatic focus-loss path consults it: explicit
/// hides (the toggle keybind, `Action::Hide`) call `hide()` directly
/// and ignore the gate entirely. Both unfocused routes — the layer
/// surface's `LayerUnfocused` and the `WindowUnfocused` callback —
/// route into [`DropTerm::focus_lost`], so one gate covers both.
fn focus_loss_hides(hide_on_focus_loss: bool) -> bool {
    hide_on_focus_loss
}

/// What the app does with a key press before any other key role
/// while the settings drawer is open (spec criterion 6): Escape
/// closes the drawer — and only that. The surface does not hide,
/// and the press is never forwarded to the terminal (which is
/// disabled while the drawer is open anyway, criterion 4). Every
/// other key — including the Ctrl+Shift+I toggle, which resolves
/// through the normal bind match into [`DropTerm::toggle_settings`]
/// — keeps the normal routing; with the drawer closed the rule
/// never fires and Escape is terminal input exactly as before.
/// Pure so the drawer-first rule is headless-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawerKeyRouting {
    /// The drawer is open and this press closes it: swallow it here.
    CloseDrawer,
    /// No drawer-first role: the press takes the normal routing.
    FallThrough,
}

fn drawer_escape_routing(settings_open: bool, key: &Key) -> DrawerKeyRouting {
    if settings_open && matches!(key, Key::Named(Named::Escape)) {
        DrawerKeyRouting::CloseDrawer
    } else {
        DrawerKeyRouting::FallThrough
    }
}

/// Whether the grab-bar drag handlers may act: pure so the guard is
/// headless-testable, in the same style as
/// [`config_edit_reshapes`] and [`layer_unfocused_acts_on`]. A drag
/// must only ever run on a shown surface at rest: the `Hiding` arm
/// excludes out-slides and their reversals by state, and the slide
/// arm excludes slide-ins — which ride a `Shown` surface, so the
/// surface state alone cannot gate them (a press inside the 130 ms
/// window must not arm a drag, or the first cadence-eligible move
/// would fire `set_size` on the animating surface). Gating the
/// *arming* here covers the trailing `DragApplyDue` and the
/// `GrabBarReleased` settle by construction: both only act on an
/// armed drag, no drag can arm while a slide animates, and `hide()`
/// clears any live drag before any slide can start. A throttled
/// press is simply inert — the surface rests in at most 130 ms and
/// the next press drags normally.
fn grab_bar_acts_on(surface: &SurfaceState, slide_animating: bool) -> bool {
    matches!(surface, SurfaceState::Shown(_)) && !slide_animating
}

/// Whether a `WindowResized` event for `id` may feed the
/// terminal-height pin: only the tracked layer surface's granted
/// sizes count — a foreign window (the clipboard anchor) must never
/// pin the terminal's. Pure, in the same style
/// as [`layer_unfocused_acts_on`].
fn resize_acts_on(surface: &SurfaceState, id: window::Id) -> bool {
    matches!(
        surface,
        SurfaceState::Measuring(shown) | SurfaceState::Shown(shown) | SurfaceState::Hiding(shown)
            if *shown == id
    )
}

/// The window-background source decision: a scheme that resolves a
/// background owns the window background — the tab-bar/grab-bar
/// chrome and the unset-bg cell fallback all read `WINDOW_BG_COLOR`,
/// so they follow by construction. A scheme without one (the COSMIC
/// pair, and any unknown name that fell back to it) defers to the
/// system-theme derivation, passed in already packed so this stays a
/// pure, headless-testable decision.
fn window_bg_color(colors: &aterm::color::Colors, system_bg: u32) -> u32 {
    match colors[NamedColor::Background] {
        Some(rgb) => {
            0xFF00_0000 | (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b)
        }
        None => system_bg,
    }
}

/// Whether the app resolves colors for dark mode right now. The
/// config preference is the source of truth; the global theme (what
/// widgets read) follows via the `set_theme` task, so color
/// decisions must not wait for it. Call sites inject the current
/// system value — the [`window_bg_color`] pattern — keeping the
/// decision pure and headless-testable.
fn effective_is_dark(preference: ThemePreference, system_is_dark: bool) -> bool {
    match preference {
        ThemePreference::System => system_is_dark,
        ThemePreference::Dark => true,
        ThemePreference::Light => false,
    }
}

/// What a theme-preference transition asks the runtime to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemeOverrideKind {
    /// Pin the app to the user's dark system palette.
    PinDark,
    /// Pin the app to the user's light system palette.
    PinLight,
    /// Clear the pin and follow the system mode again.
    FollowSystem,
}

/// The one pure transition matrix shared by `init` and
/// `ConfigChanged`: `None` when the preference did not change —
/// steady state issues no task, so a default `System` config never
/// calls `set_theme` and unrelated config echoes never storm one.
fn theme_override(old: ThemePreference, new: ThemePreference) -> Option<ThemeOverrideKind> {
    match (old, new) {
        (ThemePreference::System, ThemePreference::System)
        | (ThemePreference::Dark, ThemePreference::Dark)
        | (ThemePreference::Light, ThemePreference::Light) => None,
        (_, ThemePreference::Dark) => Some(ThemeOverrideKind::PinDark),
        (_, ThemePreference::Light) => Some(ThemeOverrideKind::PinLight),
        (ThemePreference::Dark | ThemePreference::Light, ThemePreference::System) => {
            Some(ThemeOverrideKind::FollowSystem)
        }
    }
}

/// The impure shell over [`theme_override`]: builds the override
/// theme from the user's own system palettes — never libcosmic's
/// built-in constants — marks it through `ThemeType::prefer_dark`
/// (which only writes through on `System`, the only theme type these
/// constructors produce), and hands it to the runtime's global.
/// Everything decidable is decided in the pure fns above; the
/// chained [`Message::ReapplyColors`] re-runs the color apply once
/// the global has landed, so the window-background derivation — the
/// one consumer that must read the global — sees the new mode.
///
/// The follow-up rides [`Task::chain`], never [`Task::then`]: `then`
/// CONSUMES the first task's output and replaces it with the
/// continuation's, so `set_theme(...).then(...)` would silently drop
/// the `Action::AppThemeChange` message — the pin would never reach
/// the runtime and the theme would stay whatever it was (the
/// "dark mode does nothing" bug). `chain` forwards every output of
/// the first task and only then runs the second, preserving both
/// messages and their order. Pinned by
/// `theme_override_task_delivers_set_theme_then_reapply`.
fn theme_override_task(kind: ThemeOverrideKind) -> Task<Message> {
    let (mut theme, prefer_dark) = match kind {
        ThemeOverrideKind::PinDark => (cosmic::theme::system_dark(), Some(true)),
        ThemeOverrideKind::PinLight => (cosmic::theme::system_light(), Some(false)),
        // Restore: re-read the system mode from cosmic-config fresh
        // and clear the marker, so system flips resume driving the
        // app.
        ThemeOverrideKind::FollowSystem => (cosmic::theme::system_preference(), None),
    };
    theme.theme_type.prefer_dark(prefer_dark);
    cosmic::command::set_theme(theme).chain(Task::done(cosmic::Action::App(Message::ReapplyColors)))
}

/// Whether the tab strip sits at the surface's bottom in this
/// config. `Auto` places it at the leading edge beside the grab
/// bar, i.e. opposite the screen edge; `Top`/`Bottom` pin it
/// regardless of anchor.
fn effective_bar_at_bottom(edge: Edge, tab_position: TabPosition) -> bool {
    match tab_position {
        TabPosition::Auto => matches!(edge, Edge::Top),
        TabPosition::Top => false,
        TabPosition::Bottom => true,
    }
}

/// Which side the strip's tooltips pop to: the terminal's side of the
/// bar, wherever the bar sits — the bar widget itself stays
/// edge-agnostic.
fn bar_tooltip_side(bar_at_bottom: bool) -> TooltipSide {
    if bar_at_bottom {
        TooltipSide::Above
    } else {
        TooltipSide::Below
    }
}

/// Settings for the clipboard-anchor window.
///
/// libcosmic binds its single runtime [`Clipboard`] to the first
/// window created, and when that surface is destroyed the binding is
/// dropped — taking the smithay-clipboard connection, and with it
/// any selection this app copied, down with it. `DropTerm`'s only
/// surface is the layer shell, which every hide destroys, so
/// copying out could never survive the focus switch to the paste
/// target. This window — invisible, never closed, created before
/// any layer surface — owns the binding for the whole process
/// lifetime instead: unmapped, so it appears nowhere and paints
/// nothing, and exempt from close requests, so nothing the user or
/// the runtime does can drop the connection mid-session.
fn clipboard_anchor_settings() -> window::Settings {
    window::Settings {
        visible: false,
        exit_on_close_request: false,
        resizable: false,
        decorations: false,
        size: cosmic::iced::Size::new(1.0, 1.0),
        ..window::Settings::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_edit_mid_slide_skips_the_in_place_reshape() {
        // Frozen-geometry boundary: an edit arriving while the
        // content slides must not fire `set_size` on the animating
        // surface — the edit lands on the next show instead.
        assert!(!config_edit_reshapes(true, false, true));
    }

    #[test]
    fn geometry_edit_at_rest_reshapes_in_place() {
        assert!(config_edit_reshapes(true, false, false));
    }

    #[test]
    fn geometry_edit_mid_drag_still_defers_to_release() {
        assert!(!config_edit_reshapes(true, true, false));
    }

    #[test]
    fn non_geometry_edit_never_reshapes() {
        assert!(!config_edit_reshapes(false, false, false));
    }

    #[test]
    fn drag_and_slide_together_still_skip_the_reshape() {
        assert!(!config_edit_reshapes(true, true, true));
    }

    // --- the grab-bar guard ---
    //
    // `GrabBarDragged` is the one geometry-call site that could run
    // while a slide animates: a slide-in rides a `Shown` surface, so
    // matching `Shown` alone does not exclude it — a press inside the
    // 130 ms window must neither arm a drag nor send `set_size` on
    // the animating surface. With arming gated, `DragApplyDue` and
    // `GrabBarReleased` are covered by construction: they only act on
    // an armed drag, and `hide()` clears any live drag before a
    // slide can start.

    #[test]
    fn grab_bar_drag_at_rest_acts() {
        let id = window::Id::unique();
        assert!(grab_bar_acts_on(&SurfaceState::Shown(id), false));
    }

    #[test]
    fn grab_bar_drag_mid_slide_in_does_not_act() {
        // A slide-in runs in `Shown`, so the surface state alone
        // cannot gate the drag — the guard needs the slide's
        // existence too.
        let id = window::Id::unique();
        assert!(!grab_bar_acts_on(&SurfaceState::Shown(id), true));
    }

    #[test]
    fn grab_bar_drag_ignores_every_non_shown_state() {
        // Pinned: a `Hiding` surface rejects drag messages (no
        // interleave with an out-slide or its reversal);
        // Measuring/Hidden never arm one.
        let id = window::Id::unique();
        assert!(!grab_bar_acts_on(&SurfaceState::Hiding(id), false));
        assert!(!grab_bar_acts_on(&SurfaceState::Measuring(id), false));
        assert!(!grab_bar_acts_on(&SurfaceState::Hidden, false));
    }

    // --- the toggle decision matrix ---
    //
    // `toggle_step` decides over intent × surface state; slide
    // activity needs no separate input because it is implied by the
    // state — an `Out` slide only exists while `Hiding`, an `In`
    // slide only rides a `Shown` surface — and the executors read
    // the live slide for their offsets (`hide`, `reverse_show` via
    // `slide::reverse`).

    #[test]
    fn toggle_or_show_from_hidden_runs_the_measuring_pass() {
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Hidden, true),
            ToggleStep::Measure
        );
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Hidden, true),
            ToggleStep::Measure
        );
    }

    #[test]
    fn toggle_or_hide_from_measuring_destroys_immediately() {
        // A measuring surface has no reshaped content to animate —
        // the surface dies now, not in 130 ms.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Measuring(id), true),
            ToggleStep::DestroyNow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Measuring(id), true),
            ToggleStep::DestroyNow(id)
        );
    }

    #[test]
    fn toggle_or_hide_from_shown_slides_out() {
        // Slide out, destroy on completion. Mid-slide-in the same
        // decision applies — the executor starts the out-slide from
        // the live eased offset, so nothing teleports.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Shown(id), true),
            ToggleStep::SlideOut(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Shown(id), true),
            ToggleStep::SlideOut(id)
        );
    }

    #[test]
    fn toggle_or_show_mid_hiding_reverses_on_the_same_surface() {
        // A mid-hide toggle reverses instead of being dropped — and
        // on the SAME window::Id: the live surface is reused (no new
        // measuring pass) and `reverse_show` re-grabs keyboard
        // interactivity on it per config.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Hiding(id), true),
            ToggleStep::ReverseShow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Hiding(id), true),
            ToggleStep::ReverseShow(id)
        );
    }

    #[test]
    fn show_while_measuring_or_shown_is_already_satisfied() {
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Measuring(id), true),
            ToggleStep::Leave
        );
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Shown(id), true),
            ToggleStep::Leave
        );
    }

    #[test]
    fn hide_while_hiding_neither_restarts_nor_double_destroys() {
        // Idempotence carried into the matrix: `WindowUnfocused`
        // reaches `focus_lost` unguarded, and the keyboard release
        // synthesizes its own `Unfocused` — a second hide request
        // must neither restart the out-slide nor queue a second
        // destroy task.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Hiding(id), true),
            ToggleStep::Leave
        );
    }

    #[test]
    fn hide_while_hidden_destroys_nothing_in_the_matrix() {
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Hidden, true),
            ToggleStep::Leave
        );
    }

    #[test]
    fn a_hide_intent_never_decides_to_show() {
        // Pins `hide()`'s unreachable arms as intended behavior: the
        // hide column of the matrix only ever destroys, slides out,
        // or leaves — under either slide-toggle value (the flag
        // swaps the destroy *style*, never the direction).
        let id = window::Id::unique();
        for slide_enabled in [true, false] {
            for surface in [
                SurfaceState::Hidden,
                SurfaceState::Measuring(id),
                SurfaceState::Shown(id),
                SurfaceState::Hiding(id),
            ] {
                assert!(matches!(
                    toggle_step(ToggleAction::Hide, &surface, slide_enabled),
                    ToggleStep::DestroyNow(_) | ToggleStep::SlideOut(_) | ToggleStep::Leave
                ));
            }
        }
    }

    // --- the slide toggle in the toggle matrix ---

    #[test]
    fn shown_hide_with_slides_disabled_destroys_immediately() {
        // The one decision the slide toggle owns: a hide from
        // `Shown` with the key off destroys now — instant hide.
        // Applies to a plain hide and a toggle alike.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Shown(id), false),
            ToggleStep::DestroyNow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Shown(id), false),
            ToggleStep::DestroyNow(id)
        );
    }

    #[test]
    fn hiding_reversal_ignores_the_slide_toggle() {
        // A running out-slide is never aborted by its own toggle: a
        // show intent mid-hide still reverses on the live surface
        // with the key off. The toggle gates transition ENTRY only;
        // `toggle_step` takes no mid-flight input by construction.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Hiding(id), false),
            ToggleStep::ReverseShow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Hiding(id), false),
            ToggleStep::ReverseShow(id)
        );
    }

    #[test]
    fn measuring_and_hidden_cells_ignore_the_slide_toggle() {
        // Only the Shown-hide decision keys on the flag: a measuring
        // surface always dies immediately (no content to animate),
        // hidden always measures, and a disabled hide while hidden
        // still leaves.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Measuring(id), false),
            ToggleStep::DestroyNow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Hidden, false),
            ToggleStep::Measure
        );
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Hidden, false),
            ToggleStep::Leave
        );
    }

    #[test]
    fn layer_unfocused_while_hiding_is_rejected_by_the_shown_guard() {
        // Only a shown surface reacts to losing keyboard focus —
        // during `Hiding` (the synthesized unfocus of the hide's own
        // keyboard release) the event must be a no-op.
        let id = window::Id::unique();
        assert!(layer_unfocused_acts_on(&SurfaceState::Shown(id), id));
        assert!(!layer_unfocused_acts_on(&SurfaceState::Hiding(id), id));
        assert!(!layer_unfocused_acts_on(
            &SurfaceState::Shown(window::Id::unique()),
            id
        ));
    }

    #[test]
    fn focus_loss_hides_when_configured() {
        // The ordinary path: with `hide_on_focus_loss` on, losing
        // keyboard focus hides. No settings-window input is needed:
        // while that window is open no shown surface exists (opening
        // it hides the terminal; a toggle while it is open closes it
        // first), so the gate never runs in the settings-open state.
        assert!(focus_loss_hides(true));
    }

    #[test]
    fn focus_loss_never_hides_when_focus_loss_hiding_is_off() {
        // `hide_on_focus_loss` off means no focus-loss hide, period —
        // the gate only ever narrows the hide, never widens it.
        assert!(!focus_loss_hides(false));
    }

    #[test]
    fn explicit_hides_never_consult_the_focus_loss_gate() {
        // The gate covers the automatic focus-loss path only:
        // explicit hide intents (the toggle keybind's `toggle_step`,
        // `Action::Hide`) take no gate input and never pass through
        // `focus_loss_hides`. Asserted side by side: the gate's most
        // restrictive input yields no hide, yet the explicit hide
        // step still steps its hide.
        assert!(!focus_loss_hides(false));
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Shown(id), true),
            ToggleStep::SlideOut(id)
        );
    }

    #[test]
    fn resize_from_the_tracked_surface_feeds_the_pin() {
        // The granted-size stream that pins the terminal height
        // belongs to the layer surface alone: every state tracking
        // that surface accepts its resizes.
        let id = window::Id::unique();
        assert!(resize_acts_on(&SurfaceState::Measuring(id), id));
        assert!(resize_acts_on(&SurfaceState::Shown(id), id));
        assert!(resize_acts_on(&SurfaceState::Hiding(id), id));
    }

    // --- the settings drawer's key routing (settings-drawer Task 0) ---

    #[test]
    fn escape_with_the_drawer_open_closes_it_and_only_it() {
        // Criterion 6: with the drawer open the press closes the
        // drawer — it never hides the surface and never reaches the
        // terminal (disabled while the drawer is open, criterion 4).
        assert_eq!(
            drawer_escape_routing(true, &Key::Named(Named::Escape)),
            DrawerKeyRouting::CloseDrawer
        );
    }

    #[test]
    fn escape_with_the_drawer_closed_keeps_today_s_routing() {
        // Criterion 6's second half: with the drawer closed, Escape
        // behaves exactly as today — terminal input, the rule never
        // fires.
        assert_eq!(
            drawer_escape_routing(false, &Key::Named(Named::Escape)),
            DrawerKeyRouting::FallThrough
        );
    }

    #[test]
    fn every_other_key_falls_through_even_with_the_drawer_open() {
        // The Ctrl+Shift+I toggle resolves through the normal bind
        // match (criterion 5), and typing never trips the escape
        // rule — only Escape is the drawer's key.
        for key in [
            Key::Character("i".into()),
            Key::Character("a".into()),
            Key::Named(Named::Enter),
        ] {
            assert_eq!(
                drawer_escape_routing(true, &key),
                DrawerKeyRouting::FallThrough
            );
        }
    }

    #[test]
    fn resize_from_a_foreign_window_never_feeds_the_pin() {
        // A foreign window (the clipboard anchor) must never pin the
        // terminal's height; a hidden surface tracks no window at
        // all.
        let surface_id = window::Id::unique();
        let settings_id = window::Id::unique();
        assert!(!resize_acts_on(
            &SurfaceState::Shown(surface_id),
            settings_id
        ));
        assert!(!resize_acts_on(&SurfaceState::Hidden, settings_id));
        assert!(!resize_acts_on(&SurfaceState::Hidden, surface_id));
    }

    // --- the window-background source decision ---

    #[test]
    fn scheme_background_wins_over_the_system_derivation() {
        // Dracula owns its background (#282a36, alacritty-theme
        // Dracula.toml): the resolved slot must land in
        // WINDOW_BG_COLOR's packed layout (0xAARRGGBB, opaque), not
        // the system theme's — chrome and the unset-bg fallback then
        // follow by construction.
        let dracula = theme_source::by_name("Dracula").unwrap();
        assert_eq!(window_bg_color(&dracula, 0xFF1E_1E2E), 0xFF28_2A36);
    }

    #[test]
    fn unset_background_passes_the_system_derivation_through() {
        // The COSMIC pair owns no background (the system theme
        // supplies it), so the decision is a plain passthrough —
        // default-config chrome follows the system theme, and a
        // bogus scheme name keeps that too (the `active_colors`
        // fallback resolves to this same pair).
        let cosmic_dark = theme_source::by_name("COSMIC Dark").unwrap();
        assert_eq!(window_bg_color(&cosmic_dark, 0xFF1E_1E2E), 0xFF1E_1E2E);
    }

    // --- the clipboard anchor ---
    //
    // libcosmic binds its single runtime Clipboard to the first
    // window created and drops that binding — and with it the
    // smithay-clipboard connection holding any copied selection —
    // when the owning surface is destroyed. DropTerm's only surface
    // is the layer shell, which every hide destroys, so a copy could
    // never survive the focus switch to wherever it would be pasted.
    // An invisible, never-closed normal window created first owns
    // the binding forever; these pin the two invariants that keep it
    // both invisible and immortal.

    #[test]
    fn clipboard_anchor_window_is_invisible() {
        // Unmapped: absent from the dock and Alt-Tab, and it never
        // paints — it exists only to hold the clipboard binding.
        assert!(!clipboard_anchor_settings().visible);
    }

    #[test]
    fn clipboard_anchor_window_cannot_be_closed() {
        // No close request is ever honored (and `exit_on_close(false)`
        // in main means the runtime cannot exit through it either),
        // so the binding outlives every hide/show cycle.
        assert!(!clipboard_anchor_settings().exit_on_close_request);
    }

    // --- the pure theme-preference decisions ---

    #[test]
    fn effective_is_dark_pins_dark_and_light() {
        // A pin wins regardless of the system mode — that is the
        // whole point of the feature.
        assert!(effective_is_dark(ThemePreference::Dark, false));
        assert!(effective_is_dark(ThemePreference::Dark, true));
        assert!(!effective_is_dark(ThemePreference::Light, false));
        assert!(!effective_is_dark(ThemePreference::Light, true));
    }

    #[test]
    fn effective_is_dark_system_follows_the_injected_mode() {
        // The system value is a parameter, not a global read (the
        // same pattern as `window_bg_color`): the decision stays
        // pure, and tests never depend on the process-wide THEME
        // state.
        assert!(effective_is_dark(ThemePreference::System, true));
        assert!(!effective_is_dark(ThemePreference::System, false));
    }

    #[test]
    fn theme_override_is_none_when_preference_is_unchanged() {
        // Steady state issues nothing: a default `System` config
        // never calls `set_theme`, and unrelated config echoes —
        // including our own theme echo — never storm the task.
        assert_eq!(
            theme_override(ThemePreference::System, ThemePreference::System),
            None
        );
        assert_eq!(
            theme_override(ThemePreference::Dark, ThemePreference::Dark),
            None
        );
        assert_eq!(
            theme_override(ThemePreference::Light, ThemePreference::Light),
            None
        );
    }

    #[test]
    fn theme_override_pins_on_any_transition_to_dark_or_light() {
        assert_eq!(
            theme_override(ThemePreference::System, ThemePreference::Dark),
            Some(ThemeOverrideKind::PinDark)
        );
        assert_eq!(
            theme_override(ThemePreference::Light, ThemePreference::Dark),
            Some(ThemeOverrideKind::PinDark)
        );
        assert_eq!(
            theme_override(ThemePreference::System, ThemePreference::Light),
            Some(ThemeOverrideKind::PinLight)
        );
        assert_eq!(
            theme_override(ThemePreference::Dark, ThemePreference::Light),
            Some(ThemeOverrideKind::PinLight)
        );
    }

    #[test]
    fn theme_override_restores_follow_system_from_a_pin() {
        assert_eq!(
            theme_override(ThemePreference::Dark, ThemePreference::System),
            Some(ThemeOverrideKind::FollowSystem)
        );
        assert_eq!(
            theme_override(ThemePreference::Light, ThemePreference::System),
            Some(ThemeOverrideKind::FollowSystem)
        );
    }

    /// Drives a [`Task`](cosmic::app::Task)'s stream to completion on
    /// the current thread, collecting every runtime
    /// [`Action::Output`] message in delivery order. `Task::then`
    /// consumes the first task's output and replaces it with the
    /// continuation's, so only a stream-level check can prove a
    /// message actually ships.
    fn delivered_outputs<T: Send + 'static>(task: cosmic::app::Task<T>) -> Vec<cosmic::Action<T>> {
        use cosmic::iced::futures::StreamExt;
        use cosmic::iced::futures::executor::block_on;

        let mut stream = cosmic::iced::runtime::task::into_stream(task)
            .expect("theme override task has a stream");
        let mut outputs = Vec::new();
        block_on(async {
            while let Some(action) = stream.next().await {
                if let cosmic::iced::runtime::Action::Output(message) = action {
                    outputs.push(message);
                }
            }
        });
        outputs
    }

    #[test]
    fn theme_override_task_delivers_set_theme_then_reapply() {
        // `Task::then` would CONSUME the `set_theme` output — the
        // pin message would never reach the runtime, the process
        // theme would never flip, and pinning Dark would leave a
        // light, frosted terminal. The task must deliver BOTH
        // messages: the theme override first, the `ReapplyColors`
        // echo second — `Task::chain`'s contract.
        for (kind, expected_prefer) in [
            (ThemeOverrideKind::PinDark, Some(true)),
            (ThemeOverrideKind::PinLight, Some(false)),
            // FollowSystem's palette is whatever the system prefers —
            // only the cleared marker is environment-independent.
            (ThemeOverrideKind::FollowSystem, None),
        ] {
            let outputs = delivered_outputs(theme_override_task(kind));
            assert_eq!(
                outputs.len(),
                2,
                "expected exactly the set_theme + ReapplyColors pair for {kind:?}"
            );
            let cosmic::Action::Cosmic(cosmic::app::Action::AppThemeChange(theme)) = &outputs[0]
            else {
                panic!("first delivered message must be the theme override for {kind:?}");
            };
            let cosmic::theme::ThemeType::System { prefer_dark, .. } = &theme.theme_type else {
                panic!("override theme must be System-typed for {kind:?}");
            };
            assert_eq!(*prefer_dark, expected_prefer, "marker for {kind:?}");
            assert!(
                matches!(&outputs[1], cosmic::Action::App(Message::ReapplyColors)),
                "second delivered message must be the ReapplyColors echo for {kind:?}"
            );
        }
    }

    // --- bar placement + tooltip side ---

    #[test]
    fn effective_bar_at_bottom_auto_keeps_the_strip_at_the_leading_edge() {
        // Auto placement: the strip stacks opposite the screen edge
        // — surface bottom under a Top anchor, surface top under a
        // Bottom anchor.
        assert!(effective_bar_at_bottom(Edge::Top, TabPosition::Auto));
        assert!(!effective_bar_at_bottom(Edge::Bottom, TabPosition::Auto));
    }

    #[test]
    fn effective_bar_at_bottom_top_pin_places_the_bar_at_the_top() {
        // A pin wins regardless of anchor.
        assert!(!effective_bar_at_bottom(Edge::Top, TabPosition::Top));
        assert!(!effective_bar_at_bottom(Edge::Bottom, TabPosition::Top));
    }

    #[test]
    fn effective_bar_at_bottom_bottom_pin_places_the_bar_at_the_bottom() {
        assert!(effective_bar_at_bottom(Edge::Top, TabPosition::Bottom));
        assert!(effective_bar_at_bottom(Edge::Bottom, TabPosition::Bottom));
    }

    #[test]
    fn tooltip_side_pops_terminal_ward_wherever_the_bar_sits() {
        // The tip pops to the terminal's side of the strip: below a
        // top bar, above a bottom bar — the anchor edge is not an
        // input.
        assert_eq!(bar_tooltip_side(false), TooltipSide::Below);
        assert_eq!(bar_tooltip_side(true), TooltipSide::Above);
    }

    #[test]
    fn tooltip_side_auto_layouts_match_today_on_both_edges() {
        // Regression pins: the derived side must match the fixed
        // `Auto`-layout values on both edges.
        assert_eq!(
            bar_tooltip_side(effective_bar_at_bottom(Edge::Top, TabPosition::Auto)),
            TooltipSide::Above
        );
        assert_eq!(
            bar_tooltip_side(effective_bar_at_bottom(Edge::Bottom, TabPosition::Auto)),
            TooltipSide::Below
        );
    }

    #[test]
    fn tooltip_side_pinned_layouts_follow_the_bar_not_the_anchor() {
        // A strip pinned to the screen edge still tips terminal-ward.
        assert_eq!(
            bar_tooltip_side(effective_bar_at_bottom(Edge::Top, TabPosition::Top)),
            TooltipSide::Below
        );
        assert_eq!(
            bar_tooltip_side(effective_bar_at_bottom(Edge::Bottom, TabPosition::Bottom)),
            TooltipSide::Above
        );
    }

    // --- the profile flag's forward encoding ---

    #[test]
    fn forward_words_encodes_the_profile_flag() {
        // The forwarded words must carry `--profile NAME`, or the
        // receiving instance would re-parse a plain new-tab.
        let action = ToggleAction::NewTab {
            cwd: None,
            cmd: None,
            profile: Some("ssh-web".to_string()),
        };
        assert_eq!(
            forward_words(&action),
            vec!["--profile".to_string(), "ssh-web".to_string()]
        );
    }

    #[test]
    fn forward_words_composes_cwd_and_profile() {
        // --cwd + --profile compose on the wire (OQ-2): both flags
        // survive the forward, the explicit --cwd winning at spawn.
        let action = ToggleAction::NewTab {
            cwd: Some(PathBuf::from("/tmp")),
            cmd: None,
            profile: Some("ssh-web".to_string()),
        };
        assert_eq!(
            forward_words(&action),
            vec![
                "--cwd".to_string(),
                "/tmp".to_string(),
                "--profile".to_string(),
                "ssh-web".to_string(),
            ]
        );
    }

    // --- the default-profile application policy ---

    fn test_config(profiles: &[profile::Profile], default_profile: Option<&str>) -> DropTermConfig {
        DropTermConfig {
            shell: Some("/bin/sh".to_string()),
            profiles: profiles.to_vec(),
            default_profile: default_profile.map(str::to_owned),
            ..DropTermConfig::default()
        }
    }

    fn named_profile(name: &str, command: &str, cwd: Option<PathBuf>) -> profile::Profile {
        profile::Profile {
            name: name.to_owned(),
            command: command.to_owned(),
            cwd,
        }
    }

    #[test]
    fn default_spawn_command_is_the_shell_without_a_configured_default() {
        // Unset default (every default config): plain shell, exactly
        // today's plain-spawn behavior.
        let config = test_config(&[named_profile("ssh", "ssh web", None)], None);
        assert_eq!(
            default_spawn_command(&config),
            SpawnCommand::Shell("/bin/sh".to_string())
        );
    }

    #[test]
    fn default_spawn_command_resolves_a_set_and_existing_default_into_the_profile() {
        // A set, existing default turns every plain spawn into a full
        // profile tab (ruling 1): id, command, and cwd cloned from the
        // resolved profile — the spawn sites compose the live cwd
        // filter and inheritance on top.
        let config = test_config(
            &[
                named_profile("ssh", "ssh web", None),
                named_profile("top", "htop", None),
            ],
            Some("top"),
        );
        assert_eq!(
            default_spawn_command(&config),
            SpawnCommand::Profile {
                name: "top".to_string(),
                command: "htop".to_string(),
                cwd: None,
            }
        );
    }

    #[test]
    fn default_spawn_command_is_the_shell_on_a_dangling_default() {
        // The default names a profile the list no longer carries:
        // plain shell, never a config rewrite (the module-1 seam's
        // contract) — and never a chain onto another profile.
        let config = test_config(&[named_profile("ssh", "ssh web", None)], Some("ghost"));
        assert_eq!(
            default_spawn_command(&config),
            SpawnCommand::Shell("/bin/sh".to_string())
        );
    }

    #[test]
    fn profile_pick_resolves_the_submenu_index_into_the_profile_arm() {
        // The pick carries the preset verbatim; the call site composes
        // the live-cwd filter and inheritance on top.
        let config = test_config(
            &[
                named_profile("ssh", "ssh web", Some(PathBuf::from("/srv"))),
                named_profile("top", "htop", None),
            ],
            None,
        );
        assert_eq!(
            profile_pick(&config, 1),
            Some(SpawnCommand::Profile {
                name: "top".to_string(),
                command: "htop".to_string(),
                cwd: None,
            })
        );
        assert_eq!(
            profile_pick(&config, 0),
            Some(SpawnCommand::Profile {
                name: "ssh".to_string(),
                command: "ssh web".to_string(),
                cwd: Some(PathBuf::from("/srv")),
            })
        );
    }

    #[test]
    fn profile_pick_is_none_on_a_stale_submenu_index() {
        // The index came from the menu-open snapshot; a config edited
        // while the menu was open can leave it out of range — the
        // caller's warn + no-op, never a panic.
        let config = test_config(&[named_profile("ssh", "ssh web", None)], None);
        assert_eq!(profile_pick(&config, 1), None);
        assert_eq!(profile_pick(&config, usize::MAX), None);
    }
}

#[derive(Debug, Clone, Default)]
pub struct Flags {
    pub action: ToggleAction,
    /// The words the action forwards alongside its Display name,
    /// pre-encoded by [`Flags::new`]: `CosmicFlags::args` returns
    /// `&str`s borrowed from `self`, which cannot hand out a formatted
    /// `--tab-index 3` — so the encoding runs once per launch instead
    /// of leaking per call, with [`forward_words`] as the single
    /// encoder the D-Bus forward path and the round-trip test both
    /// ride.
    forwarded: Vec<String>,
}

/// Encodes the arguments a second launch forwards alongside the action's
/// Display name: everything the receiving instance needs to
/// reconstruct the exact action through `parse_forwarded_action`.
/// Actions without payloads forward nothing.
fn forward_words(action: &ToggleAction) -> Vec<String> {
    let mut forwarded = Vec::new();
    match action {
        ToggleAction::NewTab { cwd, cmd, profile } => {
            // A non-UTF-8 path cannot cross this borrowed-str
            // boundary: the forwarded action then simply carries no
            // --cwd, and the receiving instance opens the tab in the
            // inherited directory.
            if let Some(dir) = cwd.as_deref().and_then(std::path::Path::to_str) {
                forwarded.push("--cwd".to_string());
                forwarded.push(dir.to_string());
            }
            if let Some(cmd) = cmd {
                forwarded.push("--cmd".to_string());
                forwarded.push(cmd.clone());
            }
            if let Some(name) = profile {
                forwarded.push("--profile".to_string());
                forwarded.push(name.clone());
            }
        }
        ToggleAction::CloseTab {
            tab_index: Some(index),
        }
        | ToggleAction::DuplicateTab {
            tab_index: Some(index),
        } => {
            forwarded.push("--tab-index".to_string());
            forwarded.push(index.to_string());
        }
        ToggleAction::SelectTab { index } => {
            forwarded.push(index.to_string());
        }
        ToggleAction::RenameTab { title, tab_index } => {
            forwarded.push(title.clone());
            if let Some(index) = tab_index {
                forwarded.push("--tab-index".to_string());
                forwarded.push(index.to_string());
            }
        }
        ToggleAction::SwitchProfileTab { profile, tab_index } => {
            forwarded.push("--profile".to_string());
            forwarded.push(profile.clone());
            if let Some(index) = tab_index {
                forwarded.push("--tab-index".to_string());
                forwarded.push(index.to_string());
            }
        }
        _ => {}
    }
    forwarded
}

impl Flags {
    /// Wraps `action` for launch, pre-encoding the words it forwards
    /// to the running instance (see [`Flags::forwarded`]).
    pub(crate) fn new(action: ToggleAction) -> Self {
        let forwarded = forward_words(&action);
        Self { action, forwarded }
    }
}

impl cosmic::app::CosmicFlags for Flags {
    type SubCommand = ToggleAction;
    type Args = Vec<String>;

    fn action(&self) -> Option<&ToggleAction> {
        Some(&self.action)
    }

    /// The arguments forwarded alongside the action name: a
    /// `new-tab` request carries its `--cwd`/`--cmd`, a `close-tab`
    /// or `rename-tab` its 1-based `--tab-index`, a `select-tab` its
    /// 1-based index, a `rename-tab` its TITLE word — everything else
    /// forwards no args.
    fn args(&self) -> Vec<&str> {
        self.forwarded.iter().map(String::as_str).collect()
    }
}

impl Application for DropTerm {
    type Executor = cosmic::executor::single::Executor;
    type Flags = Flags;
    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let (config_handler, config) =
            match cosmic_config::Config::new(Self::APP_ID, CONFIG_VERSION) {
                Ok(handler) => {
                    let config = match DropTermConfig::get_entry(&handler) {
                        Ok(config) => config,
                        Err((errs, config)) => {
                            log::warn!("errors loading config: {errs:?}");
                            config
                        }
                    }
                    .clamped();
                    (Some(handler), config)
                }
                Err(err) => {
                    log::warn!("failed to open config: {err}");
                    (None, DropTermConfig::default().clamped())
                }
            };

        // The session file is one small synchronous read: stashed
        // here — before any message can spawn a tab — so restore
        // structurally precedes the first `ensure_tabs`, whichever
        // startup message reaches it first. A missing or invalid
        // file stashes nothing (and was already quarantined with a
        // warning inside `load`), keeping first-run startup silent.
        // The session toggle gates the whole entry: disabled, the
        // file is not even read — nothing restores, and the file on
        // disk waits untouched for a future launch with the key on.
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let restore = session::restore_entry(
            &session::state_file_path(),
            home.as_deref(),
            config.session_persistence_enabled,
            &config.profiles,
        );

        // The clipboard anchor opens here, before
        // `debounced_toggle` can request the first layer surface: the
        // runtime binds its clipboard to the first window created, so
        // the anchor — not the terminal's layer surface — must win
        // that binding, or every hide would destroy the connection
        // owning whatever the user copied. Its id is deliberately
        // not kept: nothing references the window again (view events
        // for it fall through the foreign-window guards), and it is
        // never closed.
        let (_anchor_window, open_anchor) = window::open(clipboard_anchor_settings());

        let mut app = Self {
            core,
            config_handler,
            config,
            tabs: Vec::new(),
            active_tab: 0,
            terminal_id: widget::Id::unique(),
            term_event_tx: None,
            next_tab_id: 0,
            restore,
            pending_remote_tab: None,
            session_save: session::SessionSave::default(),
            tab_cwds: HashMap::new(),
            rename: None,
            drag: None,
            write_back: geometry::WriteBack::default(),
            last_written: None,
            rename_input_id: widget::Id::unique(),
            surface: SurfaceState::Hidden,
            output_size: None,
            window_height: None,
            pin_applied: None,
            pending_height: None,
            surface_focused: false,
            key_binds: default_key_binds(),
            last_toggle: None,
            slide: None,
            settings_open: false,
            settings_buffer: settings::SettingsBuffer::default(),
            context_menu_open: false,
            dbus_conn: None,
            dbus_shared: Arc::new(Mutex::new(dbus_state::State {
                visible: false,
                tab_count: 0,
            })),
            dbus_last: dbus_state::State {
                visible: false,
                tab_count: 0,
            },
        };
        let colors = app.active_colors();
        DropTerm::refresh_window_bg_color(&colors);

        // A second launch never reaches this: `run_single_instance` forwards
        // its action over D-Bus to the already-running instance instead
        // (see `Application::dbus_activation` below), so the first launch's
        // own action has to be applied here. The anchor window opens first
        // in the batch: the runtime binds its clipboard to the first window
        // created, and the anchor must win that race before any layer
        // surface exists — afterwards the binding never moves again.
        // A preference pinned on disk applies at startup: the
        // runtime's pre-init state is "following the system", so the
        // transition input is `(System, loaded)` — one matrix serves
        // startup and `ConfigChanged`. `System → System` is `None`,
        // so default configs never call `set_theme`. This is a
        // `no_main_window` app: nothing is visible to flash — the
        // surface opens only on the first toggle, after the task
        // lands. Startup colors above already resolved through
        // `effective_is_dark`, so they are correct even before it
        // does.
        let theme_task = theme_override(ThemePreference::System, app.config.theme_preference)
            .map_or_else(Task::none, theme_override_task);
        // First launch has no running state to act on remotely: the
        // tab verbs degrade to a plain start — the default `Toggle` —
        // with an info log, never a fabricated close or selection on
        // a restored-or-fresh session; only new-tab keeps startup
        // application.
        let startup_action = match flags.action {
            action @ (ToggleAction::CloseTab { .. }
            | ToggleAction::SelectTab { .. }
            | ToggleAction::RenameTab { .. }
            | ToggleAction::DuplicateTab { .. }) => {
                log::info!("{action} with no running instance: starting plain");
                ToggleAction::Toggle
            }
            action => action,
        };
        let task = Task::batch([
            open_anchor.then(|_| Task::none()),
            app.debounced_toggle(startup_action),
            theme_task,
        ]);
        (app, task)
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        struct TermEventSubscription;
        struct ConfigSubscription;

        let mut subs = vec![listen_raw(|event, _status, id| match event {
            Event::PlatformSpecific(PlatformSpecific::Wayland(
                cosmic::iced::runtime::core::event::wayland::Event::Layer(
                    LayerEvent::Focused,
                    _,
                    layer_id,
                ),
            )) => Some(Message::LayerFocused(layer_id)),
            Event::PlatformSpecific(PlatformSpecific::Wayland(
                cosmic::iced::runtime::core::event::wayland::Event::Layer(
                    LayerEvent::Unfocused,
                    _,
                    layer_id,
                ),
            )) => Some(Message::LayerUnfocused(layer_id)),
            Event::Window(cosmic::iced::window::Event::Opened { size, .. }) => {
                Some(Message::SurfaceOpened(id, size))
            }
            Event::Window(cosmic::iced::window::Event::Resized(size)) => {
                Some(Message::WindowResized(id, size.height))
            }
            Event::Keyboard(KeyEvent::KeyPressed {
                key,
                modifiers,
                physical_key,
                ..
            }) => Some(Message::KeyPressed(modifiers, key, physical_key)),
            _ => None,
        })];

        // Always kept alive (same key every recompute, so iced never
        // restarts the underlying stream): it both bootstraps
        // `term_event_tx` on first run and keeps draining terminal events
        // for as long as the app lives, independent of whether a surface -
        // or even a terminal - currently exists, so PTY output, title and
        // clipboard requests keep being processed while hidden.
        subs.push(Subscription::run_with(
            std::any::TypeId::of::<TermEventSubscription>(),
            |_| {
                cosmic::iced::stream::channel(
                    100,
                    |mut output: cosmic::iced::futures::channel::mpsc::Sender<Message>| async move {
                        use cosmic::iced::futures::SinkExt;

                        let (tx, mut rx) = mpsc::unbounded_channel();
                        if output.send(Message::TermEventTx(tx)).await.is_err() {
                            return;
                        }
                        while let Some((tab_id, event)) = rx.recv().await {
                            if output
                                .send(Message::TermEvent(tab_id, event))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    },
                )
            },
        ));

        subs.push(
            cosmic_config::config_subscription::<_, DropTermConfig>(
                std::any::TypeId::of::<ConfigSubscription>(),
                Self::APP_ID.into(),
                CONFIG_VERSION,
            )
            .map(|update| {
                if !update.errors.is_empty() {
                    log::warn!("config errors: {:?}", update.errors);
                }
                Message::ConfigChanged(update.config.clamped())
            }),
        );

        // Slide clock: redraw-only ticks while a slide is live —
        // 1 ms saturation so every compositor frame window contains
        // a pending redraw request (see `SLIDE_TICK`). The stream is
        // dropped the moment the slide parks — idle time costs no
        // ticks.
        if self.slide.is_some() {
            subs.push(cosmic::iced::time::every(slide::SLIDE_TICK).map(|_| Message::SlideTick));
        }

        Subscription::batch(subs)
    }

    fn system_theme_update(
        &mut self,
        _keys: &[&'static str],
        _new_theme: &cosmic::cosmic_theme::Theme,
    ) -> Task<Self::Message> {
        self.apply_config_to_tabs();
        Task::none()
    }

    fn system_theme_mode_update(
        &mut self,
        _keys: &[&'static str],
        _new_theme: &cosmic::cosmic_theme::ThemeMode,
    ) -> Task<Self::Message> {
        self.apply_config_to_tabs();
        Task::none()
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        let task = self.update_inner(message);
        // Every surface and tab transition flows through this wrapper,
        // so one sync after the arm — not per-arm calls — keeps the
        // served V1 properties current; an arm's early returns are
        // irrelevant to it. A no-op message dedupes to no emission.
        self.sync_dbus_state();
        task
    }

    fn view(&self) -> Element<'_, Self::Message> {
        widget::text("").into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Self::Message> {
        let Some(tab) = self.active() else {
            return widget::text("").into();
        };
        if !matches!(
            &self.surface,
            SurfaceState::Measuring(shown)
                | SurfaceState::Shown(shown)
                | SurfaceState::Hiding(shown)
                if *shown == id
        ) {
            return widget::text("").into();
        }

        let opacity = f32::from(self.config.opacity) / 100.0;
        // Pin the terminal's height to the granted window minus the bars'
        // strips — both always reserved, so the 1↔2 tab crossing never
        // resizes the terminal (see `surface::terminal_view_height`).
        let terminal_height = self.window_height.map_or(Length::Fill, |h| {
            Length::Fixed(surface::terminal_view_height(h))
        });
        let terminal: Element<'_, Message> = TerminalBox::new(&tab.terminal, &self.key_binds)
            .id(self.terminal_id.clone())
            .height(terminal_height)
            .opacity(opacity)
            .show_headerbar(false)
            // Inert while the settings drawer is open (spec
            // criterion 4): cosmic-term's discipline — the drawer
            // overlay already captures pointer events over its
            // bounds, and `disabled` keeps keyboard (and the rest of
            // the pointer stream) away from the pty.
            .disabled(self.settings_open)
            .on_middle_click(|| Message::MiddleClick)
            .on_open_hyperlink(Some(Box::new(Message::OpenHyperlink)))
            .on_window_focused(|| Message::WindowFocused)
            .on_window_unfocused(|| Message::WindowUnfocused)
            .context_menu_open(self.context_menu_open)
            .on_context_menu(Message::ContextMenu)
            .into();

        // The context menu wraps ONLY the terminal element — the
        // wrapper triggers anywhere over its bounds, so the tab bar
        // and grab bar must stay outside it. Trees are supplied only
        // while open (`context_menu::open_trees`): closed, the
        // wrapper is a plain pass-through with a pixel-identical
        // view. The popup plumbing is the cosmic-term pattern:
        // `on_surface_action` is mandatory — `create_popup` is a
        // no-op without it — and `window_id` must be this layer
        // surface's id (the `view_window` parameter), never the main
        // window's.
        let terminal: Element<'_, Message> = widget::context_menu(
            terminal,
            context_menu::open_trees(self.context_menu_open, &self.config.profiles),
        )
        .on_close(Message::ContextMenu(false))
        .on_surface_action(Message::Surface)
        .window_id(id)
        .into();

        // Stacking from the screen edge: content, tab strip, grab
        // bar on the free edge. Both bars take their strips out of
        // the terminal's pinned height (see
        // `surface::terminal_view_height`), so neither pushes the
        // other off the surface. Fill dimensions keep iced's flex
        // from deferring the bars past the terminal's allocation: a
        // Shrink cross-axis makes a fixed-height, fill-width bar a
        // fourth-pass child, so the Fill terminal would take the
        // whole surface and push the bars below its bottom edge.
        let grab_bar = GrabBar::new(Message::GrabBarDragged, || Message::GrabBarReleased);

        // The tab strip mounts unconditionally and its side comes
        // from `effective_bar_at_bottom`: `Auto` keeps it between
        // the terminal and the grab bar — on the surface edge
        // opposite the screen edge — while a pin can move it to the
        // screen edge itself. With fewer than two tabs the strip
        // carries only its settings button (the bar's own visibility
        // rule). The inline rename input overlays the renamed tab's
        // button — a rename can only start from a tab-button press,
        // i.e. with 2+ tabs, so the wiring is unchanged by the
        // unconditional mount. The tooltip pops to the terminal's
        // side of the strip, derived from the bar's side, not the
        // anchor edge.
        let bar_at_bottom = effective_bar_at_bottom(self.config.edge, self.config.tab_position);
        let tooltip_side = bar_tooltip_side(bar_at_bottom);
        let mut bar = TabBar::new(&self.tabs, self.active_tab)
            .on_select(Message::TabSelected)
            .on_close(Message::TabClosed)
            .on_move(Message::TabMoved)
            .on_rename(Message::RenameStarted)
            .on_rename_end(|| Message::RenameCommitted)
            // The same handler the context menu's `Settings…` row
            // uses: one click away in every tab state.
            .on_settings(|| Message::Action(Action::Settings))
            // The same handler `Ctrl+Shift+T` uses: the click rides
            // the default-profile/cwd-inheritance/placement funnel
            // unchanged.
            .on_new_tab(|| Message::Action(Action::NewTab))
            // The dropdown rides the same profile list the context
            // menu reads, and its picks the same funnels: the plain
            // row is the default spawn, a profile row the submenu's
            // apply-time-resolved `NewProfileTab` pick.
            .profiles(&self.config.profiles)
            .on_profile_pick(|index| Message::Action(Action::NewProfileTab(index)))
            .tooltip_side(tooltip_side);
        if let Some(edit) = &self.rename
            && self.tabs.iter().any(|tab| tab.id == edit.tab)
        {
            // The inline rename input, overlaid by the bar on the
            // renamed tab's button. Its value mirrors the app's
            // draft so it survives every re-render.
            bar = bar.renaming(
                edit.tab,
                widget::text_input("", edit.draft.clone())
                    .id(self.rename_input_id.clone())
                    .size(14.0)
                    .on_input(Message::RenameDraftChanged)
                    .on_submit(|_| Message::RenameCommitted)
                    .on_unfocus(Message::RenameCancelled),
            );
        }
        // Stacking: the grab bar is fixed at the leading edge —
        // opposite the screen edge, where all of `geometry.rs`'s
        // drag/anchor math expects it — the terminal fills the
        // middle, and the tab strip takes whichever side
        // `effective_bar_at_bottom` picks. Both bars take their
        // strips out of the terminal's pinned height (see
        // `surface::terminal_view_height` — subtracted
        // order-agnostically), so neither layout changes the
        // terminal's pixel height. Fill dimensions keep iced's flex
        // from deferring the bars past the terminal's allocation: a
        // Shrink cross-axis makes a fixed-height, fill-width bar a
        // fourth-pass child, so the Fill terminal would take the
        // whole surface and push the bars below its bottom edge.
        let column: Element<'_, Message> = match (self.config.edge, bar_at_bottom) {
            (Edge::Top, false) => widget::column![bar, terminal, grab_bar]
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            (Edge::Top, true) => widget::column![terminal, bar, grab_bar]
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            (Edge::Bottom, false) => widget::column![grab_bar, bar, terminal]
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            (Edge::Bottom, true) => widget::column![grab_bar, terminal, bar]
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        };

        // The slide: while the surface animates — ease-in from the
        // anchored edge after a show, ease-out back to it before a
        // hide's destroy — the content column's pose is NOT computed
        // here: the view runs in the runtime's AboutToWait pass but
        // the frame presents on the compositor's frame callback, so
        // a view-baked offset is up to one frame stale at
        // presentation — random per-frame error that read as judder
        // on hardware. Instead the live slide state and its true
        // start instant are handed to `SlideBox`, which evaluates
        // the easing inside `draw` against the frame's own clock. No
        // animation in flight → offset 0 → the column renders
        // byte-identically to the unwrapped one.
        let mut slide_box = SlideBox::new(column, 0.0);
        if let Some(playing) = self.slide {
            slide_box = slide_box.playing(self.config.edge, playing);
        }
        let content: Element<'_, Message> = slide_box.into();

        // The settings drawer rides as an overlay on the same layer
        // surface (spec criterion 1): `widget::context_drawer` wraps
        // the whole content column — terminal, tab strip, grab bar —
        // and pins its pane to the right edge without resizing
        // anything beneath (overlay mode, never the side-by-side
        // layout that would reflow the pty grid). Composed by hand
        // because the framework's `view_main` serves only the main
        // window — this surface is a layer-shell window whose view
        // comes from `view_window`, so the `context_drawer()` trait
        // hook cannot reach it (spike decision, settings-drawer
        // Task 0). The Close button routes through the same
        // `Action::Settings` toggle as the keybind (criterion 7).
        if !self.settings_open {
            return content;
        }
        let surface_width = self.output_size.map_or(480.0, |(width, _)| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "output px are < 2^24, exact in f32"
            )]
            {
                geometry::surface_width(width as f32, self.config.width_percent) as f32
            }
        });
        widget::context_drawer(
            Some("Settings".into()),
            None,
            None,
            None,
            Message::Action(Action::Settings),
            content,
            settings::drawer_view(&self.config, &self.settings_buffer).map(Message::Settings),
            settings::drawer_max_width(surface_width),
        )
        .into()
    }

    fn dbus_activation(&mut self, msg: cosmic::dbus_activation::Message) -> Task<Self::Message> {
        use cosmic::dbus_activation::Details;
        match msg.msg {
            Details::Activate => Task::done(cosmic::Action::App(Message::ToggleAction(
                ToggleAction::Toggle,
            ))),
            Details::ActivateAction { action, args } => {
                // The forwarded name plus its args re-parse through
                // the clap definitions — a single source of truth for
                // the action surface. A malformed forward degrades to
                // a plain toggle, never an error surface.
                let mut words = vec![action];
                words.extend(args);
                let parsed = crate::parse_forwarded_action(&words).unwrap_or_default();
                Task::done(cosmic::Action::App(Message::ToggleAction(parsed)))
            }
            Details::Open { .. } => Task::none(),
        }
    }

    /// Serves the V1 state interface on the bus name single-instance
    /// activation already owns. The snapshot is seeded from the
    /// app's current fields, so a first read is correct even before
    /// any later sync lands.
    fn dbus_connection(&mut self, conn: zbus::Connection) -> Task<Self::Message> {
        let state = self.derive_dbus_state();
        *self
            .dbus_shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
        self.dbus_last = state;
        self.dbus_conn = Some(conn.clone());
        let shared = self.dbus_shared.clone();
        // Serving is async and nothing waits on it: property reads
        // through the object server simply don't find the interface
        // until it lands, and a sync raced in front of it fails its
        // emission lookup, logs, and is otherwise harmless.
        tokio::spawn(async move {
            match conn
                .object_server()
                .at(dbus_state::OBJECT_PATH, dbus_state::V1::new(shared))
                .await
            {
                Ok(true) => {}
                Ok(false) => log::warn!("V1 state interface already served"),
                Err(err) => log::warn!("failed to serve V1 state interface: {err}"),
            }
        });
        Task::none()
    }
}

impl DropTerm {
    fn handle_term_event(&mut self, tab_id: TabId, event: TermEvent) -> Task<Message> {
        let Some(tab_index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            // Events for a tab that is already gone (e.g. a trailing
            // Wakeup after its shell exited) have no target.
            return Task::none();
        };
        match event {
            // The active tab never accrues a marker: the user is
            // already looking at it.
            TermEvent::Bell => {
                // Only an inactive tab accrues a marker — the user
                // is already looking at the active one.
                if tab_index != self.active_tab {
                    let tab = &mut self.tabs[tab_index];
                    tab.attention = tab.attention.on_bell();
                }
                // A bell while the overlay is hidden posts one
                // desktop notification per tab per hide period (the
                // latch arms here and clears on every show). While
                // visible the marker above alone carries the signal.
                let tab = &mut self.tabs[tab_index];
                if notify::should_notify(
                    matches!(self.surface, SurfaceState::Hidden),
                    self.config.bell_notification_enabled,
                    tab.bell_notified,
                ) {
                    tab.bell_notified = true;
                    let title = tab.title.clone();
                    notify::post_bell_notification(title);
                }
            }
            TermEvent::CursorBlinkingChange | TermEvent::ChildExit(_) => {}
            TermEvent::ClipboardLoad(kind, format) => match kind {
                aterm::ClipboardType::Clipboard => {
                    return clipboard::read().map(move |data| {
                        format(&data.unwrap_or_default());
                        cosmic::action::none()
                    });
                }
                aterm::ClipboardType::Selection => {
                    return clipboard::read_primary().map(move |data| {
                        format(&data.unwrap_or_default());
                        cosmic::action::none()
                    });
                }
            },
            TermEvent::ClipboardStore(kind, data) => match kind {
                aterm::ClipboardType::Clipboard => return clipboard::write(data),
                aterm::ClipboardType::Selection => return clipboard::write_primary(data),
            },
            TermEvent::ColorRequest(index, format) => {
                let terminal = self.tabs[tab_index].terminal.lock().unwrap();
                let rgb = terminal.effective_color(index);
                terminal.input_no_scroll(format(rgb).into_bytes());
            }
            TermEvent::PtyWrite(text) => {
                self.tabs[tab_index]
                    .terminal
                    .lock()
                    .unwrap()
                    .input_no_scroll(text.into_bytes());
            }
            TermEvent::TextAreaSizeRequest(format) => {
                let terminal = self.tabs[tab_index].terminal.lock().unwrap();
                let text = format(terminal.size().into());
                terminal.input_no_scroll(text.into_bytes());
            }
            TermEvent::Title(title) => {
                // The raw report is stored in every mode — retitle
                // inputs stay fresh, so a mode flip or a cleared
                // rename re-resolves from current state.
                self.tabs[tab_index].shell_title = Some(title);
                // Title events double as the cwd-change sample point,
                // and the path modes resolve against the fresh cwd:
                // sample first, then re-resolve. The title itself is
                // not a save trigger — churn alone writes nothing.
                let cwd_task = self.check_tab_cwd(tab_index);
                self.retitle_tab(tab_index);
                return cwd_task;
            }
            TermEvent::ResetTitle => {
                // A shell reset (RIS) clears the stored report;
                // resolution restores the mode's title — the default
                // in Shell mode — unless the user renamed the tab.
                self.tabs[tab_index].shell_title = None;
                let cwd_task = self.check_tab_cwd(tab_index);
                self.retitle_tab(tab_index);
                return cwd_task;
            }
            TermEvent::Wakeup => {
                // Output arrived: an inactive tab accrues an
                // activity marker; the active tab shows the output
                // itself, so nothing to signal.
                if tab_index != self.active_tab {
                    let tab = &mut self.tabs[tab_index];
                    tab.attention = tab.attention.on_activity();
                }
                self.tabs[tab_index].terminal.lock().unwrap().needs_update = true;
            }
            TermEvent::MouseCursorDirty => {
                self.tabs[tab_index].terminal.lock().unwrap().needs_update = true;
            }
            TermEvent::Exit => {
                // A tab whose shell exits is closed; when it was the
                // last, a fresh tab spawns in its place so the
                // overlay is never left without a terminal (see
                // `ensure_tabs`).
                return self.close_tab_at(tab_index);
            }
        }
        Task::none()
    }
}

impl DropTerm {
    fn update_inner(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleAction(action) => return self.debounced_toggle(action),
            Message::ConfigChanged(config) => {
                let old_geom = Geometry::from_config(&self.config);
                let new_geom = Geometry::from_config(&config);
                let old_preference = self.config.theme_preference;
                let old_title_mode = self.config.tab_title_mode;
                self.config = config;
                self.apply_config_to_tabs();
                // A tab-title-mode flip retitles every live tab from
                // stored state, renamed ones excepted; the delta guard
                // keeps unrelated config echoes from churning titles.
                if old_title_mode != self.config.tab_title_mode {
                    self.retitle_all_tabs();
                }
                // A changed theme preference overrides (or restores)
                // the process theme. Sits after
                // `apply_config_to_tabs` — cells are already correct
                // in this pass through `effective_is_dark` — and
                // before the geometry exits below: a preference echo
                // can never be a geometry own-write (`last_written`
                // records geometry writes only), so the two returns
                // cannot collide. The task chains a `ReapplyColors`
                // for the one global-reading consumer, the
                // window-background derivation.
                if let Some(kind) = theme_override(old_preference, self.config.theme_preference) {
                    return theme_override_task(kind);
                }
                // Our own write's echo: geometry matching the last
                // write is absorbed — the in-memory config already
                // agrees (updated at release), so no rewrite and no
                // reshape; only the record is cleared. A stale
                // record that never matches is harmless: an echo
                // matching all four fields needs no reshape anyway.
                if let Some(written) = self.last_written
                    && geometry::is_own_write(written, new_geom)
                {
                    self.last_written = None;
                    return Task::none();
                }
                // Geometry edits reshape a shown surface in place —
                // no destroy/recreate; a hidden surface just opens
                // with the new geometry. Non-geometry edits leave
                // the surface alone. A geometry edit that arrives
                // mid-drag is deferred until release, so config
                // reshapes and drag deltas never interleave
                // `set_size` calls on one surface; the release's
                // apply picks the config's other fields up (see
                // `GrabBarReleased`). A geometry edit arriving
                // mid-slide skips the in-place reshape too —
                // geometry stays frozen while animating; the
                // in-memory config above already updated, so the
                // next show opens with the edit applied (a 130 ms
                // exposure window, cheaper than a deferred-reshape
                // queue). `config_edit_reshapes` keys on the slide's
                // existence, not its direction, so this holds
                // mid-slide-out and mid-reversal alike.
                if config_edit_reshapes(
                    geometry::geometry_changed(old_geom, new_geom),
                    self.drag.is_some(),
                    self.slide.is_some(),
                ) && let (SurfaceState::Shown(id), Some(output)) =
                    (&self.surface, self.output_size)
                {
                    return geometry::apply_geometry(*id, new_geom, output);
                }
            }
            Message::ReapplyColors => {
                // The theme override landed: re-run the idempotent
                // color apply so `refresh_window_bg_color` derives
                // the fallback background from the new global mode;
                // cells were already correct via `effective_is_dark`.
                self.apply_config_to_tabs();
            }
            Message::TermEventTx(tx) => {
                if self.term_event_tx.is_none() {
                    self.term_event_tx = Some(tx);
                    self.ensure_tabs();
                    // A first-launch remote new-tab stashed while the
                    // channel was missing applies now, after the
                    // session restore has landed — with the placement
                    // the config holds at replay time.
                    if let Some(request) = self.pending_remote_tab.take() {
                        return self.spawn_remote_tab(request, self.config.new_tab_after_current);
                    }
                }
            }
            Message::TermEvent(tab_id, event) => return self.handle_term_event(tab_id, event),
            Message::SurfaceOpened(id, size) => {
                if self.surface == SurfaceState::Measuring(id) {
                    // The measuring pass stretched across the output, so
                    // its first `Opened` size *is* the output size. Keep
                    // it: every later reshape recomputes pixels from it
                    // without a new measuring pass.
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "rounded surface px are non-negative and far below u32::MAX"
                    )]
                    let output = (size.width.round() as u32, size.height.round() as u32);
                    self.output_size = Some(output);
                    let geom = Geometry::from_config(&self.config);
                    self.surface = SurfaceState::Shown(id);
                    // The slide-in starts here, with the shown
                    // surface: the full surface height is both the
                    // from-offset and the distance — deterministic
                    // and known at open time, unlike the granted
                    // window height, which arrives later through the
                    // double-reported `WindowResized` stream. The
                    // slide toggle gates this entry: disabled, the
                    // surface opens at rest with a byte-equal
                    // render.
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "output px are < 2^24, exact in f32"
                    )]
                    let distance_px =
                        geometry::surface_height(output.1 as f32, geom.height_percent) as f32;
                    self.slide =
                        slide::show_slide(self.config.slide_animation_enabled, distance_px).map(
                            |active| slide::Playing {
                                started: std::time::Instant::now(),
                                active,
                            },
                        );
                    return Task::batch([
                        geometry::apply_geometry(id, geom, output),
                        set_layer(id, surface::wlr_layer(self.config.layer)),
                        set_keyboard_interactivity(
                            id,
                            surface::keyboard_interactivity(self.config.keyboard_mode),
                        ),
                        set_exclusive_zone(id, surface::exclusive_zone(self.config.respect_panel)),
                        widget::text_input::focus(self.terminal_id.clone()),
                    ]);
                }
            }
            Message::WindowResized(id, height) => {
                // Only the tracked layer surface's granted stream
                // feeds the pin (`resize_acts_on`): the settings
                // window resizes too and its heights must never pin
                // the terminal's.
                //
                // While a drag is armed, the granted size only echoes
                // the applies the drag itself made — each one
                // double-reported, once with the previous height (the
                // runtime synthesizes a configure per set_size whose
                // height slot is stale), so following the echoes
                // would bounce the pin stale/new on every move: the
                // applies offer their targets directly instead.
                // Outside a drag, every granted height is offered
                // through the adoption debounce — each size arrives
                // twice and the applies arrive at cadence rate, and
                // reflowing the terminal on every echo made the drag
                // sluggish on hardware.
                if resize_acts_on(&self.surface, id)
                    && self.drag.is_none()
                    && let Some(task) = self.offer_window_height(height)
                {
                    return task;
                }
            }
            Message::WindowPinDue => {
                // Trailing adopt: a height the debounce skipped lands
                // once the granted stream settles. The timer waited
                // out the interval, so the gate passes; a stale fire
                // with nothing pending does nothing.
                if let Some(height) = self.pending_height
                    && let Some(task) = self.offer_window_height(height)
                {
                    return task;
                }
            }
            Message::LayerFocused(id) => {
                if self.surface == SurfaceState::Shown(id) {
                    self.surface_focused = true;
                    if let Some(tab) = self.active() {
                        tab.terminal.lock().unwrap().set_focused(true);
                    }
                    return widget::text_input::focus(self.terminal_id.clone());
                }
            }
            Message::LayerUnfocused(id) => {
                if layer_unfocused_acts_on(&self.surface, id) {
                    return self.focus_lost();
                }
            }
            Message::KeyPressed(modifiers, key, physical_key) => {
                // While an inline rename is active the rename input
                // owns the keyboard: without this guard a bound combo
                // like Ctrl+Shift+V would act on the terminal in
                // addition to the input. The input handles Enter and
                // Escape itself; the binds resume once the rename
                // ends.
                if self.rename.is_some() {
                    return Task::none();
                }
                // The drawer-first Escape rule (spec criterion 6):
                // with the drawer open, Escape closes it — only
                // that. The press never reaches the terminal and the
                // surface does not hide; with the drawer closed the
                // pure rule returns `FallThrough` and the bind match
                // below runs exactly as today.
                if drawer_escape_routing(self.settings_open, &key) == DrawerKeyRouting::CloseDrawer
                {
                    return self.close_settings_drawer();
                }
                if let Some(action) =
                    keybinds::matched_action(&self.key_binds, modifiers, &key, Some(&physical_key))
                {
                    return self.perform_action(action);
                }
            }
            Message::Action(action) => {
                // Context-menu picks reuse the keybind entry point
                // above, so an item behaves exactly like its keybind.
                return self.perform_action(action);
            }
            Message::ContextMenu(open) => {
                // The open-state latch: the press handler arms it,
                // every close path clears it, and the view reads it
                // through `context_menu::open_trees`.
                self.context_menu_open = open;
            }
            Message::Surface(action) => {
                // Popup plumbing: the wrapper's popup create/destroy
                // requests, handed to libcosmic untouched — the
                // runtime owns popup surfaces.
                return cosmic::task::message(cosmic::Action::Surface(action));
            }
            Message::WindowFocused => {
                self.surface_focused = true;
                if let Some(tab) = self.active() {
                    tab.terminal.lock().unwrap().set_focused(true);
                }
            }
            Message::WindowUnfocused => return self.focus_lost(),
            Message::MiddleClick => {
                return clipboard::read_primary()
                    .map(|text| cosmic::Action::App(Message::ClipboardPastedPrimary(text)));
            }
            Message::TabSelected(id) => {
                // The bar reports the stable tab id; resolve it against
                // the current list so a list change between redraw and
                // click cannot act on the wrong tab.
                if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
                    return self.set_active_tab(index);
                }
            }
            Message::TabClosed(id) => {
                if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
                    return self.close_tab_at(index);
                }
            }
            Message::SwitchTabProfile(id, profile_index) => {
                return self.switch_tab_profile(id, profile_index);
            }
            Message::RenameStarted(id) => {
                if let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) {
                    let draft = tab.title.clone();
                    self.rename = Some(RenameEdit { tab: id, draft });
                    // Focus the input with its value selected.
                    return Task::batch([
                        widget::text_input::focus(self.rename_input_id.clone()),
                        widget::text_input::select_all(self.rename_input_id.clone()),
                    ]);
                }
            }
            Message::RenameDraftChanged(value) => {
                if let Some(edit) = &mut self.rename {
                    edit.draft = value;
                }
            }
            Message::RenameCommitted => return self.commit_pending_rename(),
            Message::RenameCancelled => {
                self.rename = None;
                return widget::text_input::focus(self.terminal_id.clone());
            }
            Message::TabMoved(id, to) => {
                // The bar reports the drop as (stable id, target index),
                // resolved against the current list so a list change
                // between redraw and release cannot move the wrong tab.
                // Reordering never changes which tab is active, so no
                // focus handoff is needed.
                if let Some(from) = self.tabs.iter().position(|tab| tab.id == id) {
                    self.active_tab = tabs::reorder_tabs(&mut self.tabs, self.active_tab, from, to);
                    // The saved tab order is tab-bar order, so a
                    // reorder is a save trigger.
                    return self.mark_session_dirty();
                }
            }
            Message::GrabBarDragged(pointer_y) => {
                // The drag handlers (`GrabBarDragged`, `DragApplyDue`,
                // `GrabBarReleased`) all match `Shown` exclusively,
                // so a `Hiding` surface ignores every drag message by
                // construction — no drag can interleave with an
                // out-slide or its reversal. A slide-in, though,
                // rides a `Shown` surface, so `Shown` alone cannot
                // gate it: `grab_bar_acts_on` additionally requires
                // the slide to be idle (a press inside the 130 ms
                // window neither arms a drag nor resizes the
                // animating surface; the pointer-to-content mapping
                // is offset mid-slide anyway).
                //
                // Live drag: arm on the first move — anchored at the
                // height the surface has right now — then retarget
                // absolutely per move in pixels: one pixel of pointer
                // travel is one pixel of surface, and the target
                // recomputes from the height actually applied, so the
                // moving window frame of a bottom-anchored surface
                // cannot eat travel. Width and alignment always come
                // from the config (a drag can never snap a narrow
                // surface to fill). Applies pass a deadbanded,
                // interval-gated cadence (geometry::drag_apply_due) —
                // every apply reaches the app twice (the runtime
                // double-reports each set_size) and reflows the
                // terminal, so a fast drag cannot be allowed to storm
                // the pipeline; skipped targets land via a trailing
                // apply, and the release settle is exact and
                // immediate. Config is never touched while dragging;
                // persistence is the release path.
                if grab_bar_acts_on(&self.surface, self.slide.is_some())
                    && let (SurfaceState::Shown(id), Some(output)) =
                        (&self.surface, self.output_size)
                {
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "output px are < 2^24, exact in f32"
                    )]
                    let anchored_px = geometry::surface_height(
                        output.1 as f32,
                        Geometry::from_config(&self.config).height_percent,
                    );
                    let drag = self.drag.get_or_insert(Drag {
                        anchor: geometry::DragAnchor {
                            anchored_px,
                            applied_px: anchored_px,
                            press_y: pointer_y,
                        },
                        pointer_y,
                        applied: None,
                    });
                    drag.pointer_y = pointer_y;
                    let target = geometry::drag_target_px(
                        self.config.edge,
                        output.1,
                        drag.anchor,
                        pointer_y,
                    );
                    if geometry::drag_apply_due(drag.applied, std::time::Instant::now(), target) {
                        return self.apply_drag_target(*id, output, target);
                    }
                    // Throttled: land the latest target even if the
                    // pointer pauses inside the interval. A stale fire
                    // is a no-op — the apply path re-checks the gate.
                    return Task::perform(
                        tokio::time::sleep(geometry::DRAG_APPLY_INTERVAL),
                        |()| cosmic::Action::App(Message::DragApplyDue),
                    );
                }
            }
            Message::DragApplyDue => {
                // Trailing apply of a throttled live drag: recompute
                // from the latest reported pointer position and apply
                // if the gate allows; a fire after the drag ended (or
                // with nothing changed) does nothing.
                if let (SurfaceState::Shown(id), Some(output)) = (&self.surface, self.output_size)
                    && let Some(drag) = self.drag.as_ref()
                {
                    let target = geometry::drag_target_px(
                        self.config.edge,
                        output.1,
                        drag.anchor,
                        drag.pointer_y,
                    );
                    if geometry::drag_apply_due(drag.applied, std::time::Instant::now(), target) {
                        return self.apply_drag_target(*id, output, target);
                    }
                }
            }
            Message::GrabBarReleased => {
                // The settle: the dragged pixels quantize to a whole
                // percent once — the surface may snap by at most half
                // a percent, the accepted tolerance, and then matches
                // the persisted percent. The settle targets where the
                // pointer released (the latest move), not the last
                // target the cadence landed: throttling must never
                // decide the final height. The in-memory config
                // updates immediately — so a hide/show inside the
                // debounce window reopens at the new height — and one
                // write follows after the window, canceled by any
                // newer drag that settles first. Re-applying also
                // flushes a config edit that arrived mid-drag and was
                // deferred (see `ConfigChanged`): width and alignment
                // come from the current config, the height from the
                // drag — the hand outranks the file.
                if let (Some(drag), SurfaceState::Shown(id), Some(output)) =
                    (self.drag.take(), &self.surface, self.output_size)
                {
                    let geom = self.settled_geometry(&drag, output);
                    // Write-only-if-changed: a drag that settles back
                    // on the config's current percent persists nothing.
                    if geom.height_percent != self.config.height_percent {
                        self.config.height_percent = geom.height_percent;
                        let generation = self.write_back.schedule(geom.height_percent).generation;
                        return Task::batch([
                            geometry::apply_geometry(*id, geom, output),
                            Task::perform(
                                tokio::time::sleep(geometry::WRITE_BACK_DEBOUNCE),
                                move |()| cosmic::Action::App(Message::WriteDue(generation)),
                            ),
                        ]);
                    }
                    return geometry::apply_geometry(*id, geom, output);
                }
            }
            Message::WriteDue(generation) => {
                // The debounce window elapsed: persist the settled
                // height once through [`Self::write_config`] —
                // unless a newer drag rescheduled or a bare settings
                // write canceled (the stale generation finds nothing
                // pending). `PreApplied` because the release already
                // applied the geometry: the write records the written
                // geometry so the resulting `ConfigChanged` echo is
                // absorbed instead of reshaping (see `ConfigChanged`).
                if let Some(pending) = self.write_back.due(generation) {
                    self.write_config(
                        settings::WriteKind::PreApplied,
                        &settings::PlannedWrite::HeightPercent(pending.height_percent),
                    );
                }
            }
            Message::SessionSaveDue(generation) => {
                // The debounce window elapsed: capture the live tabs
                // now and write atomically — cwds are read at write
                // time, so a window always persists what is true at
                // its close, not what was true when it was armed. A
                // stale generation — superseded, or a duplicate
                // delivery after the write — writes nothing. The
                // session toggle gates the write itself: a window
                // armed before a mid-run disable fires into a
                // no-write, but the machine still clears, so no
                // zombie window ever blocks the next armed one.
                if self
                    .session_save
                    .due_writes(generation, self.config.session_persistence_enabled)
                {
                    self.write_session();
                }
            }
            Message::SlideTick => {
                // Redraw-only clock: the eased offset is evaluated at
                // draw time inside `SlideBox`, so the tick's only job
                // is parking the slide once the duration has fully
                // elapsed — which also drops the tick subscription
                // until the next slide. Between completion and this
                // parking tick the draw already clamps to exactly the
                // endpoint, so there is no pop.
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "elapsed ms of one slide; u64 range is exhausted after 58M years"
                )]
                if let Some(playing) = self.slide
                    && slide::finished(
                        &playing.active,
                        playing.started.elapsed().as_millis() as u64,
                    )
                {
                    self.slide = None;
                    // The animated hide's destroy point: an `Out`
                    // slide's completion is the only path that
                    // destroys a hiding surface — the app's single
                    // `destroy_layer_surface` call for shown
                    // surfaces, reached through the state machine's
                    // completion event. Teardown note: app teardown
                    // has no graceful path — process exit destroys
                    // the surface compositor-side, inherently
                    // immediate; any future quit action must bypass
                    // the slide and destroy straight away, like this
                    // does.
                    if let SurfaceState::Hiding(id) = self.surface {
                        self.surface = SurfaceState::Hidden;
                        return destroy_layer_surface(id);
                    }
                }
            }
            Message::OpenHyperlink(url) => {
                if let Err(err) = open::that_detached(&url) {
                    log::warn!("failed to open {url:?}: {err}");
                }
            }
            Message::ClipboardPasted(text) | Message::ClipboardPastedPrimary(text) => {
                if let (Some(text), Some(tab)) = (text, self.active()) {
                    tab.terminal.lock().unwrap().paste(&text);
                }
            }
            Message::Settings(event) => {
                // The settings drawer's commit path: every event
                // resolves through a pure planner and lands as one
                // `Bare` write via [`Self::write_config`] — the
                // drawer never applies anything itself, the
                // `ConfigChanged` echo drives every apply.
                // Display-only events (drafts, slider moves) touch
                // just the buffer; commits that fail to parse are
                // no-ops that leave the draft for the user to fix.
                match event {
                    settings::SettingsEvent::Draft(field, text) => {
                        self.settings_buffer.set_text(field, text);
                    }
                    settings::SettingsEvent::Submitted(field, text) => {
                        if let Some(write) = settings::submit_write(field, &text) {
                            // A parsed submit clears its draft so the
                            // input falls back to the echoed config
                            // value; a rejected one keeps the draft.
                            self.settings_buffer.clear_text(field);
                            self.write_config(settings::WriteKind::Bare, &write);
                        }
                    }
                    settings::SettingsEvent::SliderMoved(key, value) => {
                        self.settings_buffer.set_slider(key, value);
                    }
                    settings::SettingsEvent::SliderReleased(key) => {
                        // `on_release` carries no value (the pinned
                        // rev's API), so the release commits the
                        // buffered position the readout showed.
                        let value = self.settings_buffer.slider_value(key, &self.config);
                        if let Some(write) = settings::slider_release_write(key, value) {
                            self.write_config(settings::WriteKind::Bare, &write);
                        }
                    }
                    settings::SettingsEvent::Picked(key, index) => {
                        if let Some(write) = settings::dropdown_write(key, index) {
                            self.write_config(settings::WriteKind::Bare, &write);
                        }
                    }
                    settings::SettingsEvent::Toggled(key, checked) => {
                        if let Some(write) = settings::toggle_write(key, checked) {
                            self.write_config(settings::WriteKind::Bare, &write);
                        }
                    }
                }
            }
        }
        Task::none()
    }
    /// The app's surface state as the V1 interface sees it:
    /// `window::Id` payloads dropped, so no app type crosses into
    /// `dbus_state`. Exhaustive on purpose — a new surface state must
    /// pick a phase here.
    fn surface_phase(&self) -> dbus_state::SurfacePhase {
        match self.surface {
            SurfaceState::Hidden => dbus_state::SurfacePhase::Hidden,
            SurfaceState::Measuring(_) => dbus_state::SurfacePhase::Measuring,
            SurfaceState::Shown(_) => dbus_state::SurfacePhase::Shown,
            SurfaceState::Hiding(_) => dbus_state::SurfacePhase::Hiding,
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "tab count is bounded far below u32::MAX"
    )]
    fn derive_dbus_state(&self) -> dbus_state::State {
        dbus_state::derive(self.surface_phase(), self.tabs.len() as u32)
    }

    /// Re-derives the served snapshot and emits `PropertiesChanged`
    /// for the properties that actually moved. No-op without a bus
    /// connection (headless) and when nothing changed.
    fn sync_dbus_state(&mut self) {
        let Some(conn) = self.dbus_conn.clone() else {
            return;
        };
        let new = self.derive_dbus_state();
        let changed = dbus_state::changed(self.dbus_last, new);
        if !(changed.visible || changed.tab_count) {
            return;
        }
        *self
            .dbus_shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = new;
        self.dbus_last = new;
        // The emission rides a spawned task so `update` never awaits
        // the bus; the single executor serializes messages, so a
        // subscriber may at worst see older-then-newer values, never a
        // wrong final state.
        tokio::spawn(async move {
            let iface_ref = match conn
                .object_server()
                .interface::<_, dbus_state::V1>(dbus_state::OBJECT_PATH)
                .await
            {
                Ok(iface_ref) => iface_ref,
                Err(err) => {
                    log::warn!("V1 state interface lookup failed: {err}");
                    return;
                }
            };
            let iface = iface_ref.get().await;
            if changed.visible
                && let Err(err) = iface.visible_changed(iface_ref.signal_emitter()).await
            {
                log::warn!("failed to emit Visible change: {err}");
            }
            if changed.tab_count
                && let Err(err) = iface.tab_count_changed(iface_ref.signal_emitter()).await
            {
                log::warn!("failed to emit TabCount change: {err}");
            }
        });
    }
}
