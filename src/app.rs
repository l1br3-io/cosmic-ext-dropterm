// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;
use std::path::PathBuf;

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::term::{self as aterm, Config as TermConfig};
use alacritty_terminal::tty::{Options as PtyOptions, Shell as PtyShell};
use alacritty_terminal::vte::ansi::NamedColor;
use cosmic::app::{Core, Task};
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::Length;
use cosmic::iced::event::listen_raw;
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
use cosmic::{Application, ApplicationExt, Element};
use tokio::sync::mpsc;

use crate::config::{CONFIG_VERSION, DropTermConfig, Edge, ThemePreference};
use crate::context_menu;
use crate::geometry::{self, Geometry};
use crate::grab_bar::GrabBar;
use crate::keybinds::{self, default_key_binds};
use crate::notify;
use crate::session;
use crate::settings;
use crate::slide::{self, SlideBox};
use crate::surface;
use crate::tab_bar::TabBar;
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
    /// explicit command line (remote-newtab spec). Implies showing the
    /// overlay.
    NewTab {
        /// Working directory for the new tab; defaults to the active
        /// tab's cwd.
        #[arg(long)]
        cwd: Option<std::path::PathBuf>,
        /// Command line to run instead of the shell — whitespace-split,
        /// no quoting support.
        #[arg(long, value_parser = clap::builder::NonEmptyStringValueParser::new())]
        cmd: Option<String>,
    },
}

impl std::fmt::Display for ToggleAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Toggle => write!(f, "toggle"),
            Self::Show => write!(f, "show"),
            Self::Hide => write!(f, "hide"),
            Self::NewTab { .. } => write!(f, "new-tab"),
        }
    }
}

/// What a new tab runs (remote-newtab spec): the user's shell —
/// config-resolved or per-tab recorded — paired with the config's shell
/// arguments, or an explicit whitespace-split command line whose exit
/// closes the tab and whose tabs are session-transient.
#[derive(Debug, Clone)]
enum SpawnCommand {
    Shell(String),
    CommandLine(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SurfaceState {
    Hidden,
    /// A surface stretched across the whole active output, waiting for its
    /// `Window::Opened` event so we learn that output's real size.
    Measuring(window::Id),
    Shown(window::Id),
    /// A shown surface sliding out toward its anchored edge
    /// (slide-animation spec, A4): the content still renders —
    /// translated by the live `Out` slide — and the layer surface is
    /// destroyed only when that slide completes (criterion 2).
    /// Keyboard interactivity was already released at slide-out start
    /// (criterion 5), so focus events find no `Shown` surface to act
    /// on; the drag handlers and live reshape ignore this state by
    /// construction (they match `Shown` exclusively).
    Hiding(window::Id),
}

#[derive(Debug, Clone)]
pub enum Message {
    ToggleAction(ToggleAction),
    ConfigChanged(DropTermConfig),
    /// The `set_theme` task from a theme-preference change landed
    /// (theme-preference spec, criterion 6): re-run the color apply
    /// so the `WINDOW_BG_COLOR` derivation — which reads the
    /// process-global theme — sees the new mode. Cell colors were
    /// already same-pass correct through the `effective_is_dark`
    /// seam when the change arrived.
    ReapplyColors,
    TermEventTx(mpsc::UnboundedSender<(TabId, TermEvent)>),
    TermEvent(TabId, TermEvent),
    SurfaceOpened(window::Id, cosmic::iced::Size),
    /// The granted size of the surface with `id` changed (compositor
    /// configure after a `set_size`, or the initial configure): the new
    /// height in logical pixels, used to pin the terminal's height so the
    /// tab bar keeps its strip. The id gates the stream to the tracked
    /// layer surface (`resize_acts_on`): a second app window (the
    /// settings window, settings-ui spec U1) resizes too, and its
    /// heights must never feed the pin.
    WindowResized(window::Id, f32),
    LayerFocused(window::Id),
    LayerUnfocused(window::Id),
    KeyPressed(Modifiers, Key, cosmic::iced::keyboard::key::Physical),
    WindowFocused,
    WindowUnfocused,
    MiddleClick,
    /// A tab bar button was left-clicked (tabs spec, T5).
    TabSelected(TabId),
    /// A tab bar button was closed via its `×` or a middle-click (tabs
    /// spec, T5).
    TabClosed(TabId),
    /// A tab body was double-clicked: start an inline rename (tabs spec,
    /// T6).
    RenameStarted(TabId),
    /// The rename input's draft changed (T6).
    RenameDraftChanged(String),
    /// The rename should commit now (T6): Enter was pressed, a press
    /// landed outside the input, or the surface lost focus.
    RenameCommitted,
    /// The rename input was dismissed with Escape or Tab (T6): cancel,
    /// keeping the previous title.
    RenameCancelled,
    /// A tab was dragged to a new position in the bar (tabs spec, T7):
    /// the dragged tab's id and the index it was dropped at.
    TabMoved(TabId, usize),
    /// The grab bar was dragged (geometry spec, G3): the pointer's
    /// surface-local y (window-absolute, not a delta and not bar-local)
    /// while a press is held. Targets recompute absolutely from it, so
    /// the moving window frame of a bottom-anchored surface cannot
    /// compound drift into the drag.
    GrabBarDragged(f32),
    /// A grab bar drag ended (geometry spec, G3): the surface keeps the
    /// height it settled at; persisting it is G4.
    GrabBarReleased,
    /// The trailing apply of a throttled live drag fired (geometry
    /// spec, G3): a target dropped inside the apply interval still
    /// lands — the pointer pausing mid-drag must not leave the
    /// surface behind. A stale fire (drag ended, surface hidden) is a
    /// no-op: the apply path re-checks the cadence gate.
    DragApplyDue,
    /// The trailing adopt of a debounced terminal-pin height fired
    /// (geometry spec, G3): heights the adoption debounce skipped —
    /// the double-reported granted stream's echoes — land once the
    /// stream settles. A stale fire is a no-op.
    WindowPinDue,
    /// The debounce window for a drag-release height write elapsed
    /// (geometry spec, G4): the generation that scheduled it, checked
    /// against the current pending write so a newer drag's schedule
    /// cancels this stale one.
    WriteDue(u32),
    /// The session-save debounce window elapsed
    /// (session-persistence spec, S3): the generation that armed it,
    /// checked against the armed window so a stale fire — superseded
    /// by a newer window, or a duplicate delivery after the write —
    /// writes nothing.
    SessionSaveDue(u32),
    /// Slide clock tick (slide-animation spec, A3 + A4): redraw-only
    /// while a slide is live — the eased offset is evaluated at draw
    /// time inside `SlideBox`, so the tick exists to keep redraw
    /// requests pending (see `SLIDE_TICK`). The first tick past the
    /// duration parks the slide at its endpoint — rest for an `In`
    /// slide, the anchored edge (then destroy) for an `Out` one —
    /// and drops it (and with it the subscription).
    SlideTick,
    OpenHyperlink(String),
    ClipboardPasted(Option<String>),
    ClipboardPastedPrimary(Option<String>),
    /// The settings window with `id` should close (settings-ui spec,
    /// U1): its ✕ was pressed (`CloseRequested`, matched in the
    /// subscription — the window never closes itself,
    /// `exit_on_close_request: false`) or it closed without a handled
    /// request and `on_close_requested` sends this as the defensive
    /// cleanup so a dead id can never be `gain_focus`ed by a later
    /// press. Both clearing the tracked id and `window::close` are
    /// idempotent, so a duplicate delivery is inert.
    SettingsCloseRequested(window::Id),
    /// An event from the settings window's controls (settings-ui
    /// spec, U4). Inputs only — the update side resolves each through
    /// the pure planners and commits through the shared write seam as
    /// a `Bare` write, whose `ConfigChanged` echo is the only apply
    /// path.
    Settings(settings::SettingsEvent),
    /// A context-menu item was picked (context-menu module): the
    /// item's [`terminal::Action`], funneled to `perform_action` — the
    /// same entry point the keybind map uses, so menu and keybind
    /// behavior cannot fork.
    Action(Action),
    /// The terminal's context menu should open or close
    /// (context-menu module). Published by the `TerminalBox` press
    /// seam: `true` on a right-press with the menu closed, `false` on
    /// any default-mode press while open (toggle-close). The
    /// wrapper's `on_close` also sends `false` — it fires on every
    /// close path (Escape, click-away, item pick, compositor
    /// dismiss), so duplicate `false`s are the norm and harmless:
    /// the arm is a plain state set.
    ContextMenu(bool),
    /// A surface action from a widget (context-menu module): the
    /// context-menu popup's create/destroy requests, forwarded to
    /// libcosmic verbatim — popups are created and destroyed by the
    /// runtime, never by the app (the cosmic-term one-liner).
    Surface(SurfaceAction<Message>),
}

/// An in-flight inline tab rename (tabs spec, T6): the tab being renamed
/// and the current draft. The input itself is an overlay the tab bar
/// hosts over the renamed tab's button.
struct RenameEdit {
    tab: TabId,
    draft: String,
}

/// An in-progress grab-bar drag's runtime state (geometry spec, G3):
/// the pure anchor every target recomputes from (`geometry`), the
/// latest reported pointer y, and the last live apply (when, which
/// target) for the change-gated apply cadence — see
/// `geometry::drag_apply_due`. Applies are bounded because each one
/// costs the runtime two window resizes plus terminal and pty resizes
/// (its `set_size` handling double-reports each apply), so per-move
/// applies at pointer rate saturate the single runtime thread — the
/// drag froze input on hardware.
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
    /// Config backend, consumed by the drag-resize write-back (geometry
    /// spec, G4): `set("height_percent", …)` persists a settled drag.
    /// `None` when the config failed to open at init — writes are
    /// skipped, everything else works.
    config_handler: Option<cosmic_config::Config>,
    config: DropTermConfig,
    /// The overlay's terminals in tab order. Never empty while a surface
    /// is shown: closing the last tab respawns a fresh one (see the tabs
    /// spec).
    tabs: Vec<Tab>,
    /// Index into `tabs` of the focused tab.
    active_tab: usize,
    terminal_id: widget::Id,
    term_event_tx: Option<mpsc::UnboundedSender<(TabId, TermEvent)>>,
    /// Source of monotonically increasing tab ids, never reused within a
    /// run (see the tabs spec).
    next_tab_id: u32,
    /// The startup session stash (session-persistence spec, S2): the
    /// restore plan loaded from the state file in `init`, consumed by
    /// the spawn funnel's first productive run.
    restore: session::RestoreStash,
    /// A remote `new-tab` request that arrived before the terminal
    /// event channel existed (first-launch startup, remote-newtab
    /// spec): stashed here, applied the moment the channel
    /// bootstraps and the session restore has landed, so the
    /// requested tab never races — or is lost to — the restore.
    pending_remote_tab: Option<ToggleAction>,
    /// Debounced session-save state (session-persistence spec, S3):
    /// which change window is armed and whether a write is owed (see
    /// `session::SessionSave`).
    session_save: session::SessionSave,
    /// Each live tab's working directory as last seen, seeded at
    /// spawn/restore with the spawn cwd (session-persistence spec,
    /// S3): title events compare against this cache so only a real
    /// cwd change — not title churn — marks the session dirty.
    tab_cwds: HashMap<TabId, Option<PathBuf>>,
    /// The active inline rename, if any (tabs spec, T6): which tab is
    /// being renamed and the current draft, mirrored into the input the
    /// tab bar overlays on the renamed button.
    rename: Option<RenameEdit>,
    /// An in-progress grab-bar height drag, if any (geometry spec, G3):
    /// the absolute anchor every target recomputes from, plus the
    /// runtime state of the apply cadence (see `Drag`).
    drag: Option<Drag>,
    /// Debounced drag-release config-write state (geometry spec, G4):
    /// which release's persist is still pending.
    write_back: geometry::WriteBack,
    /// The geometry of our last config write, kept to recognize and
    /// absorb its `ConfigChanged` echo (geometry spec, G4). Cleared on
    /// absorption.
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
    /// debounce (geometry spec, G3 — see `geometry::window_pin_due`).
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
    /// The live slide (slide-animation spec, A3 + A4): the eased
    /// content offset while the surface animates — in from the
    /// anchored edge after a show, out toward it before a hide's
    /// destroy. `None` at rest — renders byte-identically to the
    /// unwrapped column — and cleared when the parking tick completes
    /// the slide (`SlideTick`).
    slide: Option<slide::Playing>,
    /// The settings window, when one is open (settings-ui spec, U1):
    /// `Some(id)` routes a repeat Ctrl+Shift+I to `gain_focus`
    /// instead of opening a second window. Cleared when the window
    /// closes — never left holding a dead id.
    settings_window: Option<window::Id>,
    /// The settings window's in-flight drafts (settings-ui spec,
    /// U4): display-only text and slider state, cleared on window
    /// close so a reopened window renders purely from config.
    settings_buffer: settings::SettingsBuffer,
    /// Whether the terminal's context menu is open (context-menu
    /// module): the `TerminalBox` press seam's latch, read by the
    /// view through `context_menu::open_trees` and fed back to the
    /// box so any press while open toggle-closes it. Cleared by
    /// `hide()` — the popup dies with the surface and `on_close`
    /// never fires there.
    context_menu_open: bool,
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
        // The config preference is the source of truth (theme-preference
        // spec, criterion 5): the seam, not a direct global read, decides
        // the mode, so a pick applies colors in this same pass — before
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
        self.refresh_window_bg_color(&colors);
        for tab in &self.tabs {
            tab.terminal.lock().unwrap().set_config(&options, colors);
        }
    }

    /// Mirrors cosmic-term's own `update_config`: the effective window
    /// background color is a process-global (`terminal::WINDOW_BG_COLOR`)
    /// used as a fallback wherever a color scheme leaves "background"
    /// unset, so it must be refreshed whenever the active scheme or
    /// COSMIC theme changes.
    fn refresh_window_bg_color(&self, colors: &aterm::color::Colors) {
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

    /// Spawns one terminal tab from `spec` — a session restore's
    /// resolved spawn parameters, with its recorded shell and
    /// re-validated cwd — or, with `None`, from the config's resolved
    /// shell with the pty's default working directory
    /// (session-persistence spec, S2).
    fn spawn_tab(&mut self, spec: Option<&session::SpawnSpec>) -> Option<Tab> {
        let (shell, cwd) = match spec {
            Some(spec) => (spec.shell.clone(), spec.cwd.clone()),
            None => (self.config.resolved_shell(), None),
        };
        self.spawn_shell(SpawnCommand::Shell(shell), cwd)
    }

    /// Spawns one terminal tab running `command` in `cwd` — the common
    /// path behind session restore ([`Self::spawn_tab`]), the new-tab
    /// keybind (which inherits the active tab's cwd), and the CLI's
    /// `new-tab` action. `cwd: None` means the pty's default directory.
    /// The shell is recorded per tab at spawn, so a mid-run config
    /// change never rewrites a live tab's shell.
    fn spawn_shell(&mut self, command: SpawnCommand, cwd: Option<PathBuf>) -> Option<Tab> {
        let Some(tx) = self.term_event_tx.clone() else {
            log::warn!("no terminal event channel yet; deferring terminal spawn");
            return None;
        };

        // A shell path pairs with the config's shell arguments; an
        // explicit command line carries its own trailing arguments and
        // never inherits them (remote-newtab spec) — `--cmd htop`
        // must not run `htop --login`. A command tab is also
        // session-ephemeral (never saved, never replayed).
        let ephemeral = matches!(command, SpawnCommand::CommandLine(_));
        let (shell, args) = match command {
            SpawnCommand::Shell(shell) => (shell, self.config.args.clone()),
            SpawnCommand::CommandLine(line) => {
                match tabs::command_line_parts(&line) {
                    Some((program, trailing)) => (program, trailing),
                    // Unreachable in practice — the CLI rejects an
                    // empty --cmd — but a degraded empty line still
                    // spawns the default shell rather than a dying
                    // exec of "".
                    None => (self.config.resolved_shell(), self.config.args.clone()),
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
            working_directory: cwd.clone(),
            drain_on_exit: true,
            ..PtyOptions::default()
        };

        let tab_id = TabId(self.next_tab_id);
        self.next_tab_id += 1;

        match Terminal::new(
            tab_id,
            tx,
            term_config,
            pty_options,
            &self.terminal_options(),
            self.active_colors(),
        ) {
            Ok(terminal) => {
                let mut tab = Tab::new(tab_id, shell, terminal);
                tab.ephemeral = ephemeral;
                // Seed the cwd cache with the spawn cwd
                // (session-persistence spec, S3): a restored tab seeds
                // its re-validated saved cwd — so the fresh shell's
                // first title report reads as unchanged — and a
                // default spawn seeds `None`; an inherited cwd seeds
                // the directory the tab opened in, for the same
                // reason.
                self.tab_cwds.insert(tab.id, cwd);
                Some(tab)
            }
            Err(err) => {
                log::error!("failed to spawn shell: {err}");
                None
            }
        }
    }

    /// Guarantees at least one tab exists — restoring the saved
    /// session on the funnel's first productive run, spawning a fresh
    /// default tab every other time (session-persistence spec, S2).
    /// Every path that needs a tab (the terminal channel arriving,
    /// showing the surface, the last tab's respawn) lands here, so
    /// the session wins exactly once and nothing is
    /// spawned-then-deleted; runs before the channel exists defer,
    /// leaving the stash armed.
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
                    // keeps the fresh shell's first title report at
                    // bay (the standing tabs rule).
                    tab.title = spec.title.clone();
                    tab.title_override = spec.title_override.clone();
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
            // Every spawn failed: fall through to today's
            // single-default-tab path.
        }
        if let Some(tab) = self.spawn_tab(None) {
            self.active_tab = tabs::insert_after_active(&mut self.tabs, self.active_tab, tab);
        }
    }

    fn show(&mut self) -> Task<Message> {
        if !matches!(self.surface, SurfaceState::Hidden) {
            return Task::none();
        }
        // The user can see the terminal again: re-arm every tab's bell
        // notification latch (bell-notify spec), so a bell from here on
        // is fresh news worth one notification per tab again.
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
        // Hiding the surface is a focus loss for the rename input (T6):
        // commit before the content slides out.
        let commit = self.commit_pending_rename();
        // The slide toggle rides in with the decision (settings-ui
        // U2): a disabled slide turns this hide into an immediate
        // destroy inside `toggle_step` — the executor arms below
        // then simply never runs.
        let task = match toggle_step(
            ToggleAction::Hide,
            &self.surface,
            self.config.slide_animation_enabled,
        ) {
            ToggleStep::DestroyNow(id) => {
                // A measuring surface has no reshaped content to
                // animate (criterion 8): destroy immediately, exactly
                // as before animated hides existed.
                self.slide = None;
                self.surface = SurfaceState::Hidden;
                destroy_layer_surface(id)
            }
            ToggleStep::SlideOut(id) => {
                // A shown surface slides out and dies on completion
                // (criterion 2). The out-slide starts from wherever
                // the content stands — the live eased offset of an
                // in-flight slide-in, or rest — so nothing teleports.
                // Its distance is the plan's pinned source (the same
                // deterministic formula the show used), kept from the
                // in-flight slide when one exists: a config edit that
                // arrived mid-slide updates the config but not the
                // frozen geometry, so the in-flight distance is the
                // one the visible surface still has.
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
                // Keyboard interactivity releases at slide-out START
                // (criterion 5), not at destroy: the previously
                // focused window regains focus while the slide plays.
                // The compositor answers with an `Unfocused` layer
                // event, which the `Shown(id)` guard in its handler
                // already rejects — the surface is `Hiding` by the
                // time it arrives.
                set_keyboard_interactivity(id, KeyboardInteractivity::None)
            }
            // `Hide` never decides to show (pinned by the matrix
            // tests), and a hiding or hidden surface leaves with
            // nothing: no restart, no double destroy.
            ToggleStep::Measure | ToggleStep::ReverseShow(_) | ToggleStep::Leave => Task::none(),
        };
        // The surface is leaving on every destructive path, and the
        // cleanup is idempotent for `Leave`: any live grab-bar drag is
        // moot (geometry spec, G3), the terminal pin's debounce state
        // starts over so the next show's first granted size adopts
        // immediately, and the active terminal reports the focus loss
        // now — its PTY keeps running regardless (criterion 2: PTYs
        // live in tabs, not the window).
        self.drag = None;
        self.pending_height = None;
        self.pin_applied = None;
        // The context menu's popup dies with the surface and its
        // `on_close` never fires (context-menu module) — clear the
        // latch here, or the next show's first right-click would be
        // consumed as a toggle-close of a menu that is not there.
        self.context_menu_open = false;
        if let Some(tab) = self.active() {
            tab.terminal.lock().unwrap().set_focused(false);
        }
        Task::batch([commit, task])
    }

    /// Reverses a hiding slide back to shown (slide-animation spec,
    /// A5, criterion 6): the live surface is reused — no new
    /// measuring pass, `output_size` is retained (the `geometry`
    /// module's guarantee) — the slide re-targets `In` from the
    /// current offset (`slide::reverse`: no teleport, a fresh fixed
    /// 130 ms), keyboard interactivity is re-grabbed per config, the
    /// terminal reports back the focus it lost at hide start, and
    /// `surface_focused` resets to `false` pending the
    /// `LayerFocused` the re-grab synthesizes. PTYs never noticed
    /// the hide (criterion 2) and keep running throughout.
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
            self.slide = Some(slide::Playing {
                started: std::time::Instant::now(),
                active: slide::reverse(
                    &playing.active,
                    playing.started.elapsed().as_millis() as u64,
                ),
            });
        }
        // The mirror of hide's focus bookkeeping (criterion 5 in
        // reverse): report the terminal focused again — `set_focused`
        // only reports real changes, so the `LayerFocused` this
        // re-grab synthesizes can repeat it harmlessly.
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

    /// The slide distance for the currently shown surface
    /// (slide-animation spec, the plan's pinned distance source): the
    /// same deterministic `geometry::surface_height` formula
    /// `SurfaceOpened` used — deliberately not the granted window
    /// height, which double-reports through `WindowResized`. A shown
    /// surface always has a measured output beside it; the `0.0`
    /// fallback covers a state that cannot occur and still hides (a
    /// degenerate at-rest out-slide that parks, then destroys).
    fn slide_distance_px(&self) -> f32 {
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
    /// here (settings-ui spec, U5b): while that window is open the surface
    /// is hidden by construction, so focus-loss events cannot arrive for a
    /// shown surface in that state.
    fn focus_lost(&mut self) -> Task<Message> {
        // Losing keyboard focus commits a rename (T6): the input's own
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

    /// The geometry a settled grab-bar drag leaves behind (geometry
    /// spec, G3): the current config's geometry with the height
    /// quantized once from where the pointer released — the *latest*
    /// target, not the last one the cadence gate landed, so throttling
    /// never decides the final height. The settled surface then matches
    /// the percent G4 persists, at most half a percent off the dragged
    /// pixels (accepted tolerance).
    fn settled_geometry(&self, drag: &Drag, output: (u32, u32)) -> Geometry {
        let final_target =
            geometry::drag_target_px(self.config.edge, output.1, drag.anchor, drag.pointer_y);
        let mut geom = Geometry::from_config(&self.config);
        geom.height_percent = geometry::percent_from_px(output.1, final_target);
        geom
    }

    /// Apply a live drag target now (geometry spec, G3): the cadence
    /// gate has passed. Records the apply, offers the height to the
    /// terminal pin — adoptions are debounced (see
    /// `offer_window_height`), because the runtime double-reports
    /// every `set_size` (once stale, once real) and each adoption
    /// costs a terminal reflow plus a pty resize that made the drag
    /// sluggish on hardware — and sends the single-path triple with
    /// the pixel height.
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
        let pin_task = self.offer_window_height(target as f32);
        let geom = Geometry::from_config(&self.config);
        let apply = geometry::apply_drag_geometry(id, &geom, output, target);
        match pin_task {
            Some(t) => Task::batch([apply, t]),
            None => apply,
        }
    }

    /// Offer a height to the terminal pin (geometry spec, G3): adopt
    /// immediately when the debounce allows (first size, re-report of
    /// the adopted height, or interval elapsed), else remember it and
    /// return the trailing-adopt timer that lands it once the granted
    /// stream settles. Every granted-size follower funnels through
    /// here — `WindowResized` echoes outside drags and the drag's own
    /// applies — so the double-reported stream can never reflow the
    /// terminal at the apply rate.
    fn offer_window_height(&mut self, height: f32) -> Option<Task<Message>> {
        let now = std::time::Instant::now();
        if geometry::window_pin_due(self.pin_applied, now, height) {
            self.window_height = Some(height);
            // Re-adopting the height the pin already has must not
            // refresh the adoption clock (see `window_pin_due`): the
            // echo stream would otherwise starve changed heights.
            self.pin_applied = Some(match self.pin_applied {
                Some((at, adopted)) if adopted == height => (at, height),
                _ => (now, height),
            });
            self.pending_height = None;
            return None;
        }
        if self.pending_height != Some(height) {
            self.pending_height = Some(height);
            return Some(Task::perform(
                tokio::time::sleep(geometry::WINDOW_PIN_INTERVAL),
                |_| cosmic::Action::App(Message::WindowPinDue),
            ));
        }
        None
    }

    /// The one path every config write goes through (settings-ui
    /// spec, U3): the drag release persists here as `PreApplied` —
    /// the set plus the `last_written` record whose echo
    /// `ConfigChanged` absorbs, byte-identical with G4 — and every
    /// settings control (from U4) persists here as `Bare`, recording
    /// nothing so its echo flows into the live-apply path. The
    /// newest-intent race rule rides the seam: a bare write of the
    /// drag's key first cancels any pending debounced write-back, so
    /// the stale `WriteDue` finds nothing and the bare write's value
    /// is what lands — while a drag release after it simply schedules
    /// fresh and lands last. Values clamp before persisting (spec
    /// criterion 9); a missing config backend (it failed to open at
    /// init) logs and skips — the drag itself still resized live.
    fn write_config(&mut self, kind: settings::WriteKind, write: settings::PlannedWrite) {
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
                // height-only drag release (G3 scope). A bare write
                // records nothing: its echo *is* the apply.
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

        // While the settings window is open the terminal is hidden by
        // construction (settings-ui spec, U5b): a toggle arriving now
        // (second launch, D-Bus activation, the startup flag) closes
        // the window first — a re-shown terminal must never slide
        // over the settings window, which can never stack above the
        // layer surface. Uniform across actions: `Hide`'s funnel
        // result is nothing anyway (the surface is already hidden),
        // but the close still runs so no toggle ever leaves the
        // window stranded open behind a hidden terminal.
        let close = if self.settings_window.is_some() {
            self.close_settings()
        } else {
            Task::none()
        };

        // A remote new-tab is not a toggle step (remote-newtab spec):
        // the tab opens — now or once the channel bootstraps — and
        // the overlay shows, whatever state it was in. The surface
        // half rides the show-intent cells of the funnel below, so a
        // request arriving mid-slide-out reverses on the live surface
        // exactly like a `show` would. Everything above (debounce,
        // settings close-first) applies to it like any other action.
        if let ToggleAction::NewTab { .. } = action {
            let open = self.open_remote_tab(action);
            let surface = match toggle_step(
                ToggleAction::Show,
                &self.surface,
                self.config.slide_animation_enabled,
            ) {
                ToggleStep::Measure => self.show(),
                ToggleStep::ReverseShow(id) => self.reverse_show(id),
                _ => Task::none(),
            };
            return Task::batch([close, open, surface]);
        }

        // The toggle funnel (slide-animation spec, A5): every matrix
        // cell executes here — most importantly, a mid-hide toggle or
        // `Show` reverses on the live surface instead of being
        // dropped. The debounce above already collapses true spam
        // (~6–7 effective toggles per second at 150 ms, each a full
        // reversal); the state machine itself never drops or queues.
        // The slide toggle (settings-ui U2) rides in with the
        // decision: with it off, a shown surface destroys now instead
        // of sliding out — day-one equality holds with it on.
        match toggle_step(action, &self.surface, self.config.slide_animation_enabled) {
            ToggleStep::Measure => Task::batch([close, self.show()]),
            ToggleStep::DestroyNow(_) | ToggleStep::SlideOut(_) => {
                Task::batch([close, self.hide()])
            }
            ToggleStep::ReverseShow(id) => Task::batch([close, self.reverse_show(id)]),
            ToggleStep::Leave => close,
        }
    }

    /// Opens the tab a remote `new-tab` asked for. At first launch the
    /// terminal event channel may not exist yet — the spawn funnel
    /// defers, the request stashes, and the channel's arrival applies
    /// it right after the session restore (remote-newtab spec).
    fn open_remote_tab(&mut self, action: ToggleAction) -> Task<Message> {
        let ToggleAction::NewTab { .. } = action else {
            return Task::none();
        };
        if self.term_event_tx.is_none() {
            self.pending_remote_tab = Some(action);
            return Task::none();
        }
        self.spawn_remote_tab(action)
    }

    /// Spawns the requested remote tab: an explicit live `--cwd` wins,
    /// then the active tab's cwd, then the pty default; a `--cmd` runs
    /// as the tab's program instead of the shell. The tab lands after
    /// the active one, becomes active, and — being a layout change —
    /// marks the session dirty.
    fn spawn_remote_tab(&mut self, action: ToggleAction) -> Task<Message> {
        let ToggleAction::NewTab { cwd, cmd } = action else {
            return Task::none();
        };
        self.ensure_tabs();
        // A nonexistent --cwd degrades to the inherited/default cwd
        // with a warning, never an error surface (remote-newtab spec).
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
        let command = match cmd {
            Some(line) => SpawnCommand::CommandLine(line),
            None => SpawnCommand::Shell(self.config.resolved_shell()),
        };
        if let Some(tab) = self.spawn_shell(command, tabs::resolve_new_tab_cwd(explicit, inherited))
        {
            let at = tabs::insert_after_active(&mut self.tabs, self.active_tab, tab);
            let switch = self.set_active_tab(at);
            return Task::batch([switch, self.mark_session_dirty()]);
        }
        Task::none()
    }

    /// Opens the settings window, or focuses the existing one
    /// (settings-ui spec, U1): one window per process, tracked by id
    /// — every repeat press routes to `window::gain_focus`, never a
    /// second window.
    fn open_settings(&mut self) -> Task<Message> {
        if let Some(id) = self.settings_window {
            return window::gain_focus(id);
        }
        // The window never closes itself: `exit_on_close_request:
        // false` turns the ✕ into a `CloseRequested` event the app
        // handles (`SettingsCloseRequested`) — closing is the app's
        // decision, never the runtime's, in a layer-shell-only app
        // that must keep running underneath (settings-ui spec,
        // criterion 8).
        let (id, open) = window::open(window::Settings {
            exit_on_close_request: false,
            size: cosmic::iced::Size::new(700.0, 760.0),
            resizable: true,
            ..window::Settings::default()
        });
        self.settings_window = Some(id);
        // The terminal and the settings window are never shown at
        // once (settings-ui spec, U5b): the settings window is a
        // normal window and can never stack above the terminal's
        // layer surface, so opening it hides the drop-down first —
        // the normal hide path, slide-out when enabled. Unconditional,
        // independent of `hide_on_focus_loss` (which keeps governing
        // ordinary losses). Every surface state is safe: a shown
        // surface slides out, a measuring one destroys now, and a
        // hidden/hiding one is a no-op (`hide()` guards).
        let hide = self.hide();
        // The open task must be returned for the window to appear —
        // it carries the runtime's window-open command. Its
        // completion value (the id, already tracked synchronously
        // above) is discarded; the multi-window example maps it the
        // same way (a first-focus message this window leaves to the
        // compositor — nothing here needs a focus task).
        Task::batch([
            open.then(|_| Task::none()),
            self.set_window_title("DropTerm Settings".to_string(), id),
            // Blur parity for the window's frosted base layer:
            // libcosmic enables blur itself only for the main window
            // and layer surfaces on theme changes — a `window::open`
            // window is neither, so without this its translucency
            // composited the raw, un-blurred backdrop (part of the
            // move-glitter). A compositor with frost disabled treats
            // the request as a no-op.
            window::enable_blur(id),
            hide,
        ])
    }

    /// The settings window's single close funnel (settings-ui spec,
    /// U1 + U5b): clears the draft buffer (so a reopened window
    /// starts purely from config), drops the tracked id (never left
    /// holding a dead one), and issues the close. Idempotent — both
    /// the take and `window::close` on a dead id are inert — so the
    /// ✕ arm, the defensive `on_close_requested` net, and the
    /// toggle-while-open rule can all funnel here with a duplicate
    /// delivery doing nothing. The terminal is NOT re-shown:
    /// closing the window leaves it hidden until the next toggle
    /// (spec criterion 6, no auto-restore — symmetric with ordinary
    /// focus-loss hiding, where closing the other window doesn't
    /// reopen the drop-down either).
    fn close_settings(&mut self) -> Task<Message> {
        self.settings_buffer.clear();
        match self.settings_window.take() {
            Some(id) => window::close(id),
            None => Task::none(),
        }
    }

    fn perform_action(&mut self, action: Action) -> Task<Message> {
        // Hiding must work even before a terminal exists (the first-run dead
        // end this fixes: `cargo run` opens a surface with no way to close it).
        if action == Action::Hide {
            return self.hide();
        }
        // The settings window opens from the keybind alone — no active
        // tab required, and it never targets the active terminal.
        if action == Action::Settings {
            return self.open_settings();
        }
        // The tab actions reshape the tab list itself — including keeping
        // it non-empty — so they run before the active-tab borrow below
        // and never reach the per-terminal actions.
        if matches!(
            action,
            Action::NewTab
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
                // A new tab inherits the active tab's cwd (cwd-inherit
                // spec): the same `/proc/<pid>/cwd` readlink the
                // session save samples, so `Ctrl+Shift+T` after a `cd`
                // lands in the same directory. A failed read — no
                // active tab, non-Linux, an unreadable `/proc` entry —
                // degrades to the pty's default directory.
                let inherited = self
                    .active()
                    .and_then(|tab| tab.terminal.lock().unwrap().working_directory());
                let shell = self.config.resolved_shell();
                if let Some(tab) = self.spawn_shell(
                    SpawnCommand::Shell(shell),
                    tabs::resolve_new_tab_cwd(None, inherited),
                ) {
                    // Yakuake-style placement: directly after the active
                    // tab, which the new tab immediately becomes.
                    let at = tabs::insert_after_active(&mut self.tabs, self.active_tab, tab);
                    let switch = self.set_active_tab(at);
                    // The open itself is a save trigger too
                    // (session-persistence spec, S3); when the switch
                    // already armed a window this second mark coalesces
                    // into it.
                    return Task::batch([switch, self.mark_session_dirty()]);
                }
            }
            Action::CloseTab => return self.close_tab_at(self.active_tab),
            Action::NextTab => {
                return self.set_active_tab(tabs::cycle_next(&self.tabs, self.active_tab));
            }
            Action::PrevTab => {
                return self.set_active_tab(tabs::cycle_prev(&self.tabs, self.active_tab));
            }
            Action::ActivateTab(requested) => {
                // Strict numbered activation: out of range never wraps
                // (tab-binds spec); set_active_tab no-ops when the tab
                // is already active.
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
                // dirty (session-persistence spec, S3).
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

    /// Closes the tab at `index` and activates its successor (or, when
    /// it closed last, its predecessor). A close that took the final
    /// tab immediately spawns a fresh one — the overlay is never left
    /// empty (see the tabs spec). Shared by the close-tab keybind and
    /// by shell exits; `set_focused` reports only real focus changes,
    /// so refocusing a tab that already had focus is a no-op.
    fn close_tab_at(&mut self, index: usize) -> Task<Message> {
        let closed = self.tabs.get(index).map(|tab| tab.id);
        if let Some(decision) = tabs::close_tab(&mut self.tabs, index, self.active_tab) {
            self.active_tab = decision.next_active;
            // The tab activation a close implies also retires the
            // newly active tab's attention marker, exactly like a
            // click or a numbered-switch bind would (bell-indicator
            // spec).
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
            // Every close funnels through here — keybind, tab-bar `×`,
            // middle-click, and a shell exit (`TermEvent::Exit`) — so
            // this is the close trigger's single save-marking site
            // (session-persistence spec, S3).
            let mark = self.mark_session_dirty();
            // A close can orphan the rename input (T6): its tab may be
            // gone, or the bar may have vanished with a single tab
            // left. End the session so the terminal regains keyboard
            // focus.
            return Task::batch([mark, self.prune_rename_session()]);
        }
        self.prune_rename_session()
    }

    /// Commits the active inline rename, if any (T6): the draft becomes
    /// the tab's user rename, the session ends, and focus returns to the
    /// terminal. Every committing focus-loss path funnels here — Enter,
    /// a press outside the input, the surface hiding or losing keyboard
    /// focus — while Escape and Tab cancel instead
    /// ([`Message::RenameCancelled`]).
    fn commit_pending_rename(&mut self) -> Task<Message> {
        if let Some(edit) = self.rename.take() {
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == edit.tab) {
                tabs::commit_rename(&mut tab.title, &mut tab.title_override, &edit.draft);
            }
            // A rename commit is a save trigger — but only when a
            // rename session actually existed (session-persistence
            // spec, S3); every committing path funnels through here,
            // so this is its single marking site.
            let mark = self.mark_session_dirty();
            return Task::batch([mark, widget::text_input::focus(self.terminal_id.clone())]);
        }
        Task::none()
    }

    /// Ends the active rename when its input can no longer be shown: the
    /// renamed tab is gone, or the bar is hidden now that a single tab
    /// remains (T6). Returns the task refocusing the terminal, if a
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
    /// both shells (FOCUS_OUT to the outgoing one, FOCUS_IN to the
    /// incoming one), the same way a window focus change does. A real
    /// switch marks the session dirty — the focused index is saved
    /// state (session-persistence spec, S3); the index-unchanged early
    /// return never marks.
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
        // Activation retires whatever attention marker the tab wore
        // (bell-indicator spec): the user is now looking at it.
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

    /// Marks the session dirty and arms the debounce window
    /// (session-persistence spec, S3): at most one `tokio::sleep` is
    /// ever in flight — a mark while a window is armed coalesces into
    /// that window's write — and only a mark after a write re-arms.
    /// Returns the window's timer task when this call armed it, the
    /// `WindowPinDue`/`WriteDue` pattern.
    fn mark_session_dirty(&mut self) -> Task<Message> {
        // The session toggle gates every save trigger (settings-ui
        // U2): a disabled trigger no-ops — no dirty mark, no debounce
        // window, no timer task — so a mid-run disable stops further
        // saves immediately.
        match self
            .session_save
            .mark_gated(self.config.session_persistence_enabled)
        {
            Some(generation) => Task::perform(
                tokio::time::sleep(session::SESSION_WRITE_INTERVAL),
                move |_| cosmic::Action::App(Message::SessionSaveDue(generation)),
            ),
            None => Task::none(),
        }
    }

    /// Captures the live tab list and writes the session file
    /// atomically (session-persistence spec, S3). The cwd readlinks
    /// happen here — at write time, not schedule time — so saved cwds
    /// are as fresh as the debounce allows; `capture` then
    /// staleness-filters each one. A failed write logs and leaves the
    /// old file in place: the next dirty window retries.
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
            })
            .collect();
        let state = session::capture(&snapshots, self.active_tab);
        if let Err(err) = session::save_atomic(&session::state_file_path(), &state) {
            log::warn!("failed to write session file: {err}");
        }
    }

    /// The cwd-change gate on a shell title event
    /// (session-persistence spec, S3): one readlink, compared against
    /// the tab's cached cwd. A moved cwd marks the session dirty and
    /// updates the cache; a title churn with the cwd holding — a vim
    /// statusline — stays quiet (Guake's change-gated cadence).
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
}

/// Whether a `ConfigChanged` may reshape a shown surface in place
/// (geometry spec, G2 + G3; slide-animation spec, A3): pure so the
/// guard is headless-testable (constructing `DropTerm` needs a live
/// `Core`). The geometry must actually have changed; a drag armed
/// mid-edit defers the reshape to the release settle (G3, so config
/// reshapes and drag deltas never interleave `set_size` calls); a
/// slide animating defers it to the next show — geometry stays frozen
/// while animating, the module's boundary. The in-memory config still
/// updates on every path (the caller does that first); only the live
/// reshape is gated.
fn config_edit_reshapes(geometry_changed: bool, drag_armed: bool, slide_animating: bool) -> bool {
    geometry_changed && !drag_armed && !slide_animating
}

/// What a toggle-intent request must do with the surface
/// (slide-animation spec, A5; settings-ui spec, U2): pure so the
/// decision is headless-testable (constructing `DropTerm` needs a
/// live `Core`). Every toggle caller funnels through here — the
/// toggle hotkey and D-Bus activation (`debounced_toggle`), plus
/// `Ctrl+Shift+Q` and focus loss through `hide`'s `Hide` column —
/// so the full state × intent matrix lives in one place and
/// reversibility (criterion 6) is just another cell. Slide activity
/// needs no separate input: it is implied by the state — an `Out`
/// slide only exists while `Hiding`, an `In` slide only rides a
/// `Shown` surface — and the executors read the live slide for
/// offsets.
///
/// `slide_animation_enabled` (settings-ui U2) gates transition
/// *entry* only: it swaps the `Shown`-hide cell between sliding out
/// and destroying now — the destroy *style*, never the direction.
/// The helper takes no mid-flight input by construction, so a
/// running slide is never aborted by its own toggle: the `Hiding`
/// cells (destroy-on-completion, reversal) key on state alone and
/// ignore the flag entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToggleStep {
    /// Run the measuring show pass (a hidden surface only): measure
    /// → reshape → shown, then slide in (A3).
    Measure,
    /// Destroy immediately: a measuring surface has no reshaped
    /// content to animate (criterion 8), so it keeps the immediate
    /// destroy the app had before animated hides existed.
    DestroyNow(window::Id),
    /// Slide out from the current offset and destroy on the slide's
    /// completion tick (criterion 2). Keyboard interactivity releases
    /// at slide-out start (criterion 5).
    SlideOut(window::Id),
    /// Reverse a hiding slide back to shown on the same live surface
    /// (criterion 6): no new measuring pass — `output_size` is
    /// retained — the slide re-targets `In` from the current offset,
    /// keyboard interactivity is re-grabbed per config, and
    /// `surface_focused` resets pending the next `LayerFocused`.
    ReverseShow(window::Id),
    /// Nothing: the intent is already satisfied — a hide while
    /// hiding or hidden (no restart, no double destroy), a show
    /// while measuring or shown.
    Leave,
}

fn toggle_step(action: ToggleAction, surface: &SurfaceState, slide_enabled: bool) -> ToggleStep {
    match (action, surface) {
        // Show intents: only a hidden surface starts the measuring
        // pass; a hiding one reverses (criterion 6); a measuring or
        // shown one is already on its way. None of these cells key on
        // the slide toggle — a hiding surface always reverses on the
        // live slide, whatever the flag says now (never aborted).
        (ToggleAction::Show | ToggleAction::Toggle, SurfaceState::Hidden) => ToggleStep::Measure,
        (ToggleAction::Show | ToggleAction::Toggle, SurfaceState::Hiding(id)) => {
            ToggleStep::ReverseShow(*id)
        }
        (ToggleAction::Show, SurfaceState::Measuring(_) | SurfaceState::Shown(_)) => {
            ToggleStep::Leave
        }
        // Hide intents — and a toggle on any visible surface: a
        // measuring surface dies now (criterion 8), a hiding or hidden
        // one stays put (A4 idempotence — no restart, no double
        // destroy). A shown one slides out (criterion 2; mid-slide-in
        // the executor starts from the live eased offset) — or, with
        // the slide toggle off (settings-ui U2), destroys now: instant
        // hide, the pre-slide-animation behavior (spec criterion 7).
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
        // Structurally unreachable: `debounced_toggle` opens a remote
        // new-tab outside the toggle funnel, so no NewTab ever gets
        // here. Loud rather than silently wrong.
        (ToggleAction::NewTab { .. }, _) => unreachable!("new-tab opens outside the funnel"),
    }
}

/// Whether a `LayerUnfocused` event for `id` should reach
/// [`DropTerm::focus_lost`] (slide-animation spec, A5 guards audit):
/// only a shown surface reacts to losing keyboard focus — one
/// arriving while `Hiding` (the synthesized unfocus of the hide's
/// own keyboard release, or a racing focus loss) is a no-op, so it
/// can never restart or double-drive the out-slide.
fn layer_unfocused_acts_on(surface: &SurfaceState, id: window::Id) -> bool {
    matches!(surface, SurfaceState::Shown(shown) if *shown == id)
}

/// Whether losing keyboard focus should hide the surface: plain
/// `hide_on_focus_loss`, the pre-U5 shape restored by the U5b
/// reversal. No settings-window input is needed: while that window
/// is open no shown surface exists by construction — opening it
/// hides the terminal (`DropTerm::open_settings`), and a toggle
/// arriving while it is open closes it first
/// (`DropTerm::debounced_toggle`) — so the gate never runs in the
/// settings-open state. Only the automatic focus-loss path consults
/// it: explicit hides (the toggle keybind, `Action::Hide`) call
/// `hide()` directly and ignore the gate entirely. Both unfocused
/// routes — the layer surface's `LayerUnfocused` and the
/// `WindowUnfocused` callback — funnel into
/// [`DropTerm::focus_lost`], so one gate covers both.
fn focus_loss_hides(hide_on_focus_loss: bool) -> bool {
    hide_on_focus_loss
}

/// Whether the grab-bar drag handlers may act (slide-animation spec,
/// A6 frozen-geometry audit): pure so the guard is headless-testable,
/// the [`config_edit_reshapes`]/[`layer_unfocused_acts_on`] seam
/// style. A drag must only ever run on a shown surface at rest: the
/// `Hiding` arm excludes out-slides and their reversals by state, and
/// the slide arm excludes slide-ins — which ride a `Shown` surface,
/// so the surface state alone cannot gate them (the audit's catch:
/// a press inside the 130 ms window must not arm a drag, or the
/// first cadence-eligible move would fire `set_size` on the
/// animating surface). Gating the *arming* here covers the trailing
/// `DragApplyDue` and the `GrabBarReleased` settle by construction:
/// both only act on an armed drag, no drag can arm while a slide
/// animates, and `hide()` clears any live drag before any slide can
/// start. A throttled press is simply inert — the surface rests in
/// at most 130 ms and the next press drags normally.
fn grab_bar_acts_on(surface: &SurfaceState, slide_animating: bool) -> bool {
    matches!(surface, SurfaceState::Shown(_)) && !slide_animating
}

/// Whether a `WindowResized` event for `id` may feed the
/// terminal-height pin (settings-ui spec, U1): only the tracked layer
/// surface's granted sizes count — the settings window is resizable
/// too, and its heights must never pin the terminal's. For
/// layer-surface events this is byte-identical with the single-window
/// behavior (every state that tracks the surface accepted them; the
/// settings window is the only new source), the
/// [`layer_unfocused_acts_on`] seam style.
fn resize_acts_on(surface: &SurfaceState, id: window::Id) -> bool {
    matches!(
        surface,
        SurfaceState::Measuring(shown) | SurfaceState::Shown(shown) | SurfaceState::Hiding(shown)
            if *shown == id
    )
}

/// The window-background source decision (color-schemes spec,
/// criterion 3): a scheme that resolves a background owns the window
/// background — the tab-bar/grab-bar chrome and the unset-bg cell
/// fallback all read `WINDOW_BG_COLOR`, so they follow by
/// construction. A scheme without one (the COSMIC pair, and any
/// unknown name that fell back to it) defers to the system-theme
/// derivation, passed in already packed so this stays a pure,
/// headless-testable decision in the [`grab_bar_acts_on`] seam style.
fn window_bg_color(colors: &aterm::color::Colors, system_bg: u32) -> u32 {
    match colors[NamedColor::Background] {
        Some(rgb) => {
            0xFF00_0000 | (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b)
        }
        None => system_bg,
    }
}

/// Whether the app resolves colors for dark mode right now
/// (theme-preference spec, criterion 5). The config preference is the
/// source of truth; the global theme (what widgets read) follows via
/// the `set_theme` task, so the seams must not wait for it. Call
/// sites inject the current system value — the [`window_bg_color`]
/// shape — keeping the decision pure and headless-testable.
fn effective_is_dark(preference: ThemePreference, system_is_dark: bool) -> bool {
    match preference {
        ThemePreference::System => system_is_dark,
        ThemePreference::Dark => true,
        ThemePreference::Light => false,
    }
}

/// What a theme-preference transition asks the runtime to do
/// (theme-preference spec, criterion 6).
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
/// calls `set_theme` (criterion 2) and unrelated config echoes never
/// storm one.
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
/// constructors produce), and hands it to the runtime's global (the
/// cosmic-edit pattern). Everything decidable is decided in the pure
/// fns above; the chained [`Message::ReapplyColors`] re-runs the
/// color apply once the global has landed, so the window-background
/// derivation — the one consumer that must read the global — sees
/// the new mode.
fn theme_override_task(kind: ThemeOverrideKind) -> Task<Message> {
    let (mut theme, prefer_dark) = match kind {
        ThemeOverrideKind::PinDark => (cosmic::theme::system_dark(), Some(true)),
        ThemeOverrideKind::PinLight => (cosmic::theme::system_light(), Some(false)),
        // Restore: re-read the system mode from cosmic-config fresh
        // and clear the marker, so system flips resume driving the
        // app (criterion 7).
        ThemeOverrideKind::FollowSystem => (cosmic::theme::system_preference(), None),
    };
    theme.theme_type.prefer_dark(prefer_dark);
    let set_theme: Task<Message> = cosmic::command::set_theme(theme);
    set_theme.then(|_| Task::done(cosmic::Action::App(Message::ReapplyColors)))
}

/// Settings for the clipboard-anchor window (the copy-out fix).
///
/// libcosmic binds its single runtime [`Clipboard`] to the first
/// window created, and when that surface is destroyed the binding is
/// dropped — taking the smithay-clipboard connection, and with it
/// any selection this app copied, down with it. DropTerm's only
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
        // A3's frozen-geometry boundary: an edit arriving while the
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

    // --- A6: the grab-bar guard (slide-animation spec, criterion 4) ---
    //
    // `GrabBarDragged` was the one geometry-call site reachable while
    // a slide animates: a slide-in rides a `Shown` surface, so
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
        // The frozen-geometry audit's catch: a slide-in runs in
        // `Shown`, so the surface state alone cannot gate the drag —
        // the guard needs the slide's existence too.
        let id = window::Id::unique();
        assert!(!grab_bar_acts_on(&SurfaceState::Shown(id), true));
    }

    #[test]
    fn grab_bar_drag_ignores_every_non_shown_state() {
        // A5's by-construction exclusion, now pinned: a `Hiding`
        // surface rejects drag messages (no interleave with an
        // out-slide or its reversal); Measuring/Hidden never arm one.
        let id = window::Id::unique();
        assert!(!grab_bar_acts_on(&SurfaceState::Hiding(id), false));
        assert!(!grab_bar_acts_on(&SurfaceState::Measuring(id), false));
        assert!(!grab_bar_acts_on(&SurfaceState::Hidden, false));
    }

    // --- A5: the toggle decision matrix (slide-animation spec) ---
    //
    // `toggle_step` decides over intent × surface state; slide
    // activity needs no separate input because it is implied by the
    // state — an `Out` slide only exists while `Hiding`, an `In`
    // slide only rides a `Shown` surface — and the executors read
    // the live slide for their offsets (A4's `hide`, A5's
    // `reverse_show` via `slide::reverse`).

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
        // Criterion 8: a measuring surface has no reshaped content
        // to animate — the surface dies now, not in 130 ms, exactly
        // as before animated hides existed.
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
        // Criterion 2: slide out, destroy on completion. Mid-slide-in
        // the same cell applies — the executor starts the out-slide
        // from the live eased offset (A4's `hide`), so nothing
        // teleports.
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
        // Criterion 6: a mid-hide toggle reverses instead of being
        // dropped — and on the SAME window::Id: the live surface is
        // reused (no new measuring pass) and `reverse_show` re-grabs
        // keyboard interactivity on it per config.
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
        // A4 idempotence carried into the matrix: `WindowUnfocused`
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
        // or leaves — under either slide-toggle value (settings-ui
        // U2: the flag swaps the destroy *style*, never the
        // direction).
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

    // --- settings-ui U2: the slide toggle in the toggle matrix ---

    #[test]
    fn shown_hide_with_slides_disabled_destroys_immediately() {
        // The one cell the slide toggle owns: a hide from `Shown`
        // with the key off destroys now — instant hide with
        // immediate destroy, the pre-slide-animation behavior (spec
        // criterion 7). Applies to a plain hide and a toggle alike.
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
        // The never-aborted reading (spec criterion 4): a running
        // out-slide belongs to the enabled machine that started it —
        // a show intent mid-hide still reverses on the live surface
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
        // Only the Shown-hide cell keys on the flag: a measuring
        // surface always died immediately (criterion 8, no content
        // to animate), hidden always measures, and a disabled hide
        // while hidden still leaves.
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
        // The A5 guards audit: only a shown surface reacts to losing
        // keyboard focus — during `Hiding` (the synthesized unfocus
        // of the hide's own keyboard release) the event must be a
        // no-op.
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
        // The ordinary path (settings-ui spec, U5b): with
        // `hide_on_focus_loss` on, losing keyboard focus hides —
        // unchanged behavior. No settings-window input is needed:
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
        // `focus_loss_hides`. Asserted at the seams: the gate's most
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
        // belongs to the layer surface alone (settings-ui spec, U1):
        // every state tracking that surface accepts its resizes,
        // matching the pre-settings-window behavior for the only
        // window that existed then.
        let id = window::Id::unique();
        assert!(resize_acts_on(&SurfaceState::Measuring(id), id));
        assert!(resize_acts_on(&SurfaceState::Shown(id), id));
        assert!(resize_acts_on(&SurfaceState::Hiding(id), id));
    }

    #[test]
    fn resize_from_a_foreign_window_never_feeds_the_pin() {
        // The settings window is resizable too, and its heights must
        // never pin the terminal's; a hidden surface tracks no window
        // at all.
        let surface_id = window::Id::unique();
        let settings_id = window::Id::unique();
        assert!(!resize_acts_on(
            &SurfaceState::Shown(surface_id),
            settings_id
        ));
        assert!(!resize_acts_on(&SurfaceState::Hidden, settings_id));
        assert!(!resize_acts_on(&SurfaceState::Hidden, surface_id));
    }

    // --- color-schemes C3: the window-background source decision
    // (spec criterion 3) ---

    #[test]
    fn scheme_background_wins_over_the_system_derivation() {
        // Dracula owns its background (#282a36, alacritty-theme
        // Dracula.toml): the resolved slot must land in
        // WINDOW_BG_COLOR's packed layout (0xAARRGGBB, opaque), not
        // the system theme's — chrome and the unset-bg cell fallback
        // then follow by construction.
        let dracula = theme_source::by_name("Dracula").unwrap();
        assert_eq!(window_bg_color(&dracula, 0xFF1E_1E2E), 0xFF28_2A36);
    }

    #[test]
    fn unset_background_passes_the_system_derivation_through() {
        // The COSMIC pair owns no background (the system theme
        // supplies it), so the decision is a byte-identical
        // passthrough — the default-config chrome behavior is
        // unchanged, and a bogus scheme name keeps it too (the
        // `active_colors` fallback resolves to this same pair).
        let cosmic_dark = theme_source::by_name("COSMIC Dark").unwrap();
        assert_eq!(window_bg_color(&cosmic_dark, 0xFF1E_1E2E), 0xFF1E_1E2E);
    }

    // --- the clipboard anchor (copy-out fix) ---
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

    // --- theme-preference P1: the pure decision seams ---

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
        // `window_bg_color` shape): the seam stays pure, and tests
        // never depend on the process-wide THEME state.
        assert!(effective_is_dark(ThemePreference::System, true));
        assert!(!effective_is_dark(ThemePreference::System, false));
    }

    #[test]
    fn theme_override_is_none_when_preference_is_unchanged() {
        // Steady state issues nothing (criterion 2): a default
        // `System` config never calls `set_theme`, and unrelated
        // config echoes — including our own theme echo — never storm
        // the task.
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
}

#[derive(Debug, Clone, Default)]
pub struct Flags {
    pub action: ToggleAction,
}

impl cosmic::app::CosmicFlags for Flags {
    type SubCommand = ToggleAction;
    type Args = Vec<String>;

    fn action(&self) -> Option<&ToggleAction> {
        Some(&self.action)
    }

    /// The arguments forwarded alongside the action name (remote-newtab
    /// spec): a `new-tab` request carries its `--cwd`/`--cmd` so the
    /// running instance can reconstruct the exact action — everything
    /// else forwards no args, exactly as before.
    fn args(&self) -> Vec<&str> {
        let ToggleAction::NewTab { cwd, cmd } = &self.action else {
            return Vec::new();
        };
        let mut forwarded = Vec::new();
        if let Some(dir) = cwd.as_deref().and_then(std::path::Path::to_str) {
            forwarded.push("--cwd");
            forwarded.push(dir);
        }
        // A non-UTF-8 path cannot cross this borrowed-str seam: the
        // forwarded action then simply carries no --cwd, and the
        // receiving instance opens the tab in the inherited directory.
        if let Some(cmd) = cmd {
            forwarded.push("--cmd");
            forwarded.push(cmd);
        }
        forwarded
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

        // The session file is one small synchronous read
        // (session-persistence spec, S2): stashed here — before any
        // message can spawn a tab — so restore structurally precedes
        // the first `ensure_tabs`, whichever startup message reaches
        // the funnel first. A missing or invalid file stashes
        // nothing (and was already quarantined with a warning inside
        // `load`), keeping first-run startup silent. The session
        // toggle gates the whole entry (settings-ui U2): disabled, the
        // file is not even read — nothing restores, and the file on
        // disk waits untouched for a future launch with the key on.
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let restore = session::restore_entry(
            &session::state_file_path(),
            home.as_deref(),
            config.session_persistence_enabled,
        );

        // The clipboard anchor (the copy-out fix) opens here, before
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
            settings_window: None,
            settings_buffer: settings::SettingsBuffer::default(),
            context_menu_open: false,
        };
        let colors = app.active_colors();
        app.refresh_window_bg_color(&colors);

        // A second launch never reaches this: `run_single_instance` forwards
        // its action over D-Bus to the already-running instance instead
        // (see `Application::dbus_activation` below), so the first launch's
        // own action has to be applied here. The anchor window opens first
        // in the batch: the runtime binds its clipboard to the first window
        // created, and the anchor must win that race before any layer
        // surface exists — afterwards the binding never moves again.
        // A preference pinned on disk applies at startup
        // (theme-preference P1): the runtime's pre-init state is
        // "following the system", so the transition input is
        // `(System, loaded)` — one matrix serves startup and
        // `ConfigChanged`. `System → System` is `None`, so default
        // configs never call `set_theme` (criterion 2). This is a
        // `no_main_window` app: nothing is visible to flash — the
        // surface opens only on the first toggle, after the task
        // lands. Startup colors above already resolved through the
        // config-driven seam, so they are correct even before it
        // does.
        let theme_task = theme_override(ThemePreference::System, app.config.theme_preference)
            .map(theme_override_task)
            .unwrap_or_else(Task::none);
        let task = Task::batch([
            open_anchor.then(|_| Task::none()),
            app.debounced_toggle(flags.action),
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
            // The settings window's ✕ (settings-ui spec, U1): it never
            // closes itself (`exit_on_close_request: false`), so the
            // request lands here for the app to act on — the same
            // listen seam the multi-window example uses. A foreign id
            // (the layer surface never requests a close) matches
            // nothing in the handler and is inert.
            Event::Window(cosmic::iced::window::Event::CloseRequested) => {
                Some(Message::SettingsCloseRequested(id))
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

        // Slide clock (slide-animation spec, A3): redraw-only ticks
        // while a slide is live — 1 ms saturation so every compositor
        // frame window contains a pending redraw request (see
        // `SLIDE_TICK`). The stream is dropped the moment the slide
        // parks — idle time costs no ticks.
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
        match message {
            Message::ToggleAction(action) => return self.debounced_toggle(action),
            Message::ConfigChanged(config) => {
                let old_geom = Geometry::from_config(&self.config);
                let new_geom = Geometry::from_config(&config);
                let old_preference = self.config.theme_preference;
                self.config = config;
                self.apply_config_to_tabs();
                // A changed theme preference overrides (or restores)
                // the process theme (theme-preference P1). Sits after
                // `apply_config_to_tabs` — cells are already correct
                // in this pass through the `effective_is_dark` seam —
                // and before the geometry exits below: a preference
                // echo can never be a geometry own-write
                // (`last_written` records geometry writes only), so
                // the two returns cannot collide. The task chains a
                // `ReapplyColors` for the one global-reading
                // consumer, the window-background derivation.
                if let Some(kind) = theme_override(old_preference, self.config.theme_preference) {
                    return theme_override_task(kind);
                }
                // Our own write's echo (geometry spec, G4): geometry
                // matching the last write is absorbed — the in-memory
                // config already agrees (updated at release), so no
                // rewrite and no reshape; only the record is cleared.
                // A stale record that never matches is harmless: an
                // echo matching all four fields needs no reshape
                // anyway.
                if let Some(written) = self.last_written
                    && geometry::is_own_write(&written, &new_geom)
                {
                    self.last_written = None;
                    return Task::none();
                }
                // Geometry edits reshape a shown surface in place
                // (geometry spec, G2) — no destroy/recreate; a hidden
                // surface just opens with the new geometry. Non-geometry
                // edits leave the surface alone. A geometry edit that
                // arrives mid-drag (G3+) is deferred until release, so
                // config reshapes and drag deltas never interleave
                // `set_size` calls on one surface; the release's apply
                // picks the config's other fields up (see
                // `GrabBarReleased`). A geometry edit arriving mid-slide
                // (slide-animation A3) skips the in-place reshape too —
                // geometry stays frozen while animating; the in-memory
                // config above already updated, so the next show opens
                // with the edit applied (a 130 ms exposure window,
                // cheaper than a deferred-reshape queue). A5 audit:
                // `config_edit_reshapes` keys on the slide's
                // existence, not its direction, so this holds
                // mid-slide-out and mid-reversal alike.
                if config_edit_reshapes(
                    geometry::geometry_changed(&old_geom, &new_geom),
                    self.drag.is_some(),
                    self.slide.is_some(),
                ) && let (SurfaceState::Shown(id), Some(output)) =
                    (&self.surface, self.output_size)
                {
                    return geometry::apply_geometry(*id, &new_geom, output);
                }
            }
            Message::ReapplyColors => {
                // The theme override landed (theme-preference P1):
                // re-run the idempotent color apply so
                // `refresh_window_bg_color` derives the fallback
                // background from the new global mode; cells were
                // already correct through the seam.
                self.apply_config_to_tabs();
            }
            Message::TermEventTx(tx) => {
                if self.term_event_tx.is_none() {
                    self.term_event_tx = Some(tx);
                    self.ensure_tabs();
                    // A first-launch remote new-tab stashed while the
                    // channel was missing applies now, after the
                    // session restore has landed (remote-newtab spec).
                    if let Some(request) = self.pending_remote_tab.take() {
                        return self.spawn_remote_tab(request);
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
                    let output = (size.width.round() as u32, size.height.round() as u32);
                    self.output_size = Some(output);
                    let geom = Geometry::from_config(&self.config);
                    self.surface = SurfaceState::Shown(id);
                    // The slide-in starts here, with the shown surface
                    // (slide-animation spec, A3): the full surface
                    // height is both the from-offset and the distance
                    // — deterministic and known at open time, unlike
                    // the granted window height, which arrives later
                    // through the double-reported `WindowResized`
                    // stream (the plan's pinned distance source). The
                    // slide toggle gates this entry (settings-ui U2):
                    // disabled, the surface opens at rest — the
                    // pre-slide-animation behavior, byte-equal render.
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
                        geometry::apply_geometry(id, &geom, output),
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
                // Only the tracked layer surface's granted stream feeds
                // the pin (`resize_acts_on`): the settings window
                // resizes too (settings-ui spec, U1) and its heights
                // must never pin the terminal's.
                //
                // While a drag is armed, the granted size only echoes
                // the applies the drag itself made — each one
                // double-reported, once with the previous height (the
                // runtime synthesizes a configure per set_size whose
                // height slot is stale), so following the echoes would
                // bounce the pin stale/new on every move: the applies
                // offer their targets directly instead. Outside a
                // drag, every granted height is offered through the
                // adoption debounce — each size arrives twice and the
                // applies arrive at cadence rate, and reflowing the
                // terminal on every echo made the drag sluggish on
                // hardware.
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
                // While an inline rename is active (T6) the rename input
                // owns the keyboard: without this guard a bound combo
                // like Ctrl+Shift+V would act on the terminal in
                // addition to the input. The input handles Enter and
                // Escape itself; the binds resume once the rename ends.
                if self.rename.is_some() {
                    return Task::none();
                }
                if let Some(action) =
                    keybinds::matched_action(&self.key_binds, modifiers, &key, Some(&physical_key))
                {
                    return self.perform_action(action);
                }
            }
            Message::Action(action) => {
                // The context-menu funnel (context-menu module): menu
                // picks reuse the keybind entry point above, so an
                // item behaves exactly like its keybind.
                return self.perform_action(action);
            }
            Message::ContextMenu(open) => {
                // The open-state latch (context-menu module): the
                // press seam arms it, every close path clears it, and
                // the view reads it through
                // `context_menu::open_trees`.
                self.context_menu_open = open;
            }
            Message::Surface(action) => {
                // Popup plumbing (context-menu module): the wrapper's
                // popup create/destroy requests, handed to libcosmic
                // untouched — the runtime owns popup surfaces.
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
            Message::RenameStarted(id) => {
                if let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) {
                    let draft = tab.title.clone();
                    self.rename = Some(RenameEdit { tab: id, draft });
                    // Focus the input with its value selected,
                    // Yakuake-style.
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
                    // reorder is a save trigger
                    // (session-persistence spec, S3).
                    return self.mark_session_dirty();
                }
            }
            Message::GrabBarDragged(pointer_y) => {
                // Guards audit (A5, corrected by A6): the drag
                // handlers (`GrabBarDragged`, `DragApplyDue`,
                // `GrabBarReleased`) all match `Shown` exclusively,
                // so a `Hiding` surface ignores every drag message by
                // construction — no drag can interleave with an
                // out-slide or its reversal. A slide-in, though,
                // rides a `Shown` surface, so `Shown` alone cannot
                // gate it: `grab_bar_acts_on` additionally requires
                // the slide to be idle (the A6 frozen-geometry
                // audit's catch — a press inside the 130 ms window
                // neither arms a drag nor resizes the animating
                // surface; the pointer-to-content mapping is offset
                // mid-slide anyway).
                //
                // Live drag (geometry spec, G3): arm on the first move
                // — anchored at the height the surface has right now —
                // then retarget absolutely per move in pixels: one
                // pixel of pointer travel is one pixel of surface, and
                // the target recomputes from the height actually
                // applied, so the moving window frame of a
                // bottom-anchored surface cannot eat travel. Width and
                // alignment always come from the config (a drag can
                // never snap a narrow surface to fill). Applies pass a
                // deadbanded, interval-gated cadence
                // (geometry::drag_apply_due) — every apply reaches the
                // app twice (the runtime double-reports each set_size)
                // and reflows the terminal, so a fast drag cannot be
                // allowed to storm the pipeline; skipped targets land
                // via a trailing apply, and the release settle is
                // exact and immediate. Config is never touched while
                // dragging (a "never" boundary of the module);
                // persistence is G4's release path.
                if grab_bar_acts_on(&self.surface, self.slide.is_some())
                    && let (SurfaceState::Shown(id), Some(output)) =
                        (&self.surface, self.output_size)
                {
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
                        |_| cosmic::Action::App(Message::DragApplyDue),
                    );
                }
            }
            Message::DragApplyDue => {
                // Trailing apply of a throttled live drag (geometry
                // spec, G3): recompute from the latest reported
                // pointer position and apply if the gate allows; a
                // fire after the drag ended (or with nothing changed)
                // does nothing.
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
                // percent once — the surface may snap by at most half a
                // percent, the accepted tolerance, and then matches the
                // percent G4 persists. The settle targets where the
                // pointer released (the latest move), not the last
                // target the cadence landed: throttling must never
                // decide the final height. The in-memory config updates
                // immediately — so a hide/show inside the debounce
                // window reopens at the new height — and one write
                // follows after the window, canceled by any newer drag
                // that settles first. Re-applying also flushes a config
                // edit that arrived mid-drag and was deferred (see
                // `ConfigChanged`): width and alignment come from the
                // current config, the height from the drag — the hand
                // outranks the file.
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
                            geometry::apply_geometry(*id, &geom, output),
                            Task::perform(
                                tokio::time::sleep(geometry::WRITE_BACK_DEBOUNCE),
                                move |_| cosmic::Action::App(Message::WriteDue(generation)),
                            ),
                        ]);
                    }
                    return geometry::apply_geometry(*id, &geom, output);
                }
            }
            Message::WriteDue(generation) => {
                // The debounce window elapsed: persist the settled
                // height once through the shared write seam
                // (settings-ui spec, U3) — unless a newer drag
                // rescheduled or a bare settings write canceled (the
                // stale generation finds nothing pending).
                // `PreApplied` because the release already applied the
                // geometry: the seam records the written geometry so
                // the resulting `ConfigChanged` echo is absorbed
                // instead of reshaping (see `ConfigChanged`) — G4's
                // behavior, byte-identical.
                if let Some(pending) = self.write_back.due(generation) {
                    self.write_config(
                        settings::WriteKind::PreApplied,
                        settings::PlannedWrite::HeightPercent(pending.height_percent),
                    );
                }
            }
            Message::SessionSaveDue(generation) => {
                // The debounce window elapsed: capture the live tabs
                // now and write atomically — cwds are read at write
                // time, so a window always persists what is true at
                // its close, not what was true when it was armed
                // (session-persistence spec, S3). A stale generation —
                // superseded, or a duplicate delivery after the write —
                // writes nothing. The session toggle gates the write
                // itself (settings-ui U2): a window armed before a
                // mid-run disable fires into a no-write, but the
                // machine still clears, so no zombie window ever
                // blocks the next armed one.
                if self
                    .session_save
                    .due_writes(generation, self.config.session_persistence_enabled)
                {
                    self.write_session();
                }
            }
            Message::SlideTick => {
                // Redraw-only clock (slide-animation spec, A3): the
                // eased offset is evaluated at draw time inside
                // `SlideBox`, so the tick's only job is parking the
                // slide once the duration has fully elapsed — which
                // also drops the tick subscription until the next
                // slide. Between completion and this parking tick the
                // draw already clamps to exactly the endpoint, so
                // there is no pop.
                if let Some(playing) = self.slide
                    && slide::finished(
                        &playing.active,
                        playing.started.elapsed().as_millis() as u64,
                    )
                {
                    self.slide = None;
                    // The animated hide's destroy point (slide-animation
                    // spec, A4): an `Out` slide's completion is the only
                    // path that destroys a hiding surface — the app's
                    // single `destroy_layer_surface` seam for shown
                    // surfaces, now reached through the state machine's
                    // completion event. Teardown note (criterion 8):
                    // app teardown has no graceful path — process exit
                    // destroys the surface compositor-side, inherently
                    // immediate; any future quit action must bypass the
                    // slide and destroy straight away, like this does.
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
                    tab.terminal.lock().unwrap().paste(text);
                }
            }
            // The settings window's ✕ (settings-ui spec, U1): it
            // arrives as `CloseRequested` (the window never closes
            // itself) and funnels through `close_settings`, which
            // clears the draft buffer, drops the tracking, and
            // issues the close. A request whose id is not the
            // tracked one (the defensive `on_close_requested` net
            // after a handled close) finds nothing and no-ops — both
            // the take and `window::close` on a dead id are inert, so
            // duplicate delivery is harmless. The app itself keeps
            // running: this is not the main window and
            // `exit_on_close(false)` stands (criterion 8). U5b: the
            // terminal stays hidden — the next toggle shows it with
            // every edit applied.
            Message::SettingsCloseRequested(id) => {
                if self.settings_window.is_some_and(|open| open == id) {
                    return self.close_settings();
                }
            }
            Message::Settings(event) => {
                // The settings window's commit funnel (settings-ui
                // spec, U4): every event resolves through a pure
                // planner and lands as one `Bare` write on the shared
                // seam — the window never applies anything itself, the
                // `ConfigChanged` echo drives every apply (criterion
                // 3). Display-only events (drafts, slider moves) touch
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
                            self.write_config(settings::WriteKind::Bare, write);
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
                            self.write_config(settings::WriteKind::Bare, write);
                        }
                    }
                    settings::SettingsEvent::Picked(key, index) => {
                        if let Some(write) = settings::dropdown_write(key, index) {
                            self.write_config(settings::WriteKind::Bare, write);
                        }
                    }
                    settings::SettingsEvent::Toggled(key, checked) => {
                        if let Some(write) = settings::toggle_write(key, checked) {
                            self.write_config(settings::WriteKind::Bare, write);
                        }
                    }
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        widget::text("").into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Self::Message> {
        // The settings window (settings-ui spec, U4): the sectioned
        // instant-apply view — rendered from config with buffered
        // drafts winning while they exist, events mapped into the
        // app's `Message::Settings` funnel.
        if self.settings_window == Some(id) {
            return settings::view(&self.config, &self.settings_buffer).map(Message::Settings);
        }
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
        // Pin the terminal's height to the granted window minus the bar's
        // strip: a Fill terminal would claim the bar's space and push it
        // below the surface (see `surface::terminal_view_height`).
        let terminal_height = self
            .window_height
            .map(|h| Length::Fixed(surface::terminal_view_height(h, self.tabs.len())))
            .unwrap_or(Length::Fill);
        let terminal: Element<'_, Message> = TerminalBox::new(&tab.terminal, &self.key_binds)
            .id(self.terminal_id.clone())
            .height(terminal_height)
            .opacity(opacity)
            .show_headerbar(false)
            .on_middle_click(|| Message::MiddleClick)
            .on_open_hyperlink(Some(Box::new(Message::OpenHyperlink)))
            .on_window_focused(|| Message::WindowFocused)
            .on_window_unfocused(|| Message::WindowUnfocused)
            .context_menu_open(self.context_menu_open)
            .on_context_menu(Message::ContextMenu)
            .into();

        // The context menu (context-menu module): wraps ONLY the
        // terminal element — the wrapper triggers anywhere over its
        // bounds, so the tab bar and grab bar must stay outside it
        // (spec criterion 7). Trees are supplied only while open
        // (`context_menu::open_trees`): closed, the wrapper is a
        // plain pass-through — the pixel-identical view (criterion
        // 6). The popup plumbing is the cosmic-term pattern
        // (`main.rs:3444-3452`): `on_surface_action` is mandatory —
        // `create_popup` is a no-op without it — and `window_id` must
        // be this layer surface's id (the `view_window` parameter),
        // never the main window's.
        let terminal: Element<'_, Message> =
            widget::context_menu(terminal, context_menu::open_trees(self.context_menu_open))
                .on_close(Message::ContextMenu(false))
                .on_surface_action(Message::Surface)
                .window_id(id)
                .into();

        // Stacking from the screen edge (geometry spec, criterion 9):
        // content, tab bar (only with more than one tab, tabs spec),
        // grab bar on the free edge. The bars take their strips out of
        // the terminal's pinned height (see
        // `surface::terminal_view_height`), so neither pushes the other
        // off the surface. Fill dimensions keep iced's flex from
        // deferring the bars past the terminal's allocation: a Shrink
        // cross-axis makes a fixed-height, fill-width bar a fourth-pass
        // child, so the Fill terminal would take the whole surface and
        // push the bars below its bottom edge.
        let grab_bar = GrabBar::new(Message::GrabBarDragged, || Message::GrabBarReleased);

        // The tab bar appears only with more than one tab and sits on
        // the surface edge opposite the screen edge (tabs spec):
        // between the terminal and the grab bar.
        let column: Element<'_, Message> = if self.tabs.len() > 1 {
            let mut bar = TabBar::new(&self.tabs, self.active_tab)
                .on_select(Message::TabSelected)
                .on_close(Message::TabClosed)
                .on_move(Message::TabMoved)
                .on_rename(Message::RenameStarted)
                .on_rename_end(|| Message::RenameCommitted);
            if let Some(edit) = &self.rename
                && self.tabs.iter().any(|tab| tab.id == edit.tab)
            {
                // The inline rename input (T6), overlaid by the bar on
                // the renamed tab's button. Its value mirrors the app's
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
            match self.config.edge {
                Edge::Top => widget::column![terminal, bar, grab_bar]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                Edge::Bottom => widget::column![grab_bar, bar, terminal]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
            }
        } else {
            match self.config.edge {
                Edge::Top => widget::column![terminal, grab_bar]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                Edge::Bottom => widget::column![grab_bar, terminal]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
            }
        };

        // The slide (slide-animation spec, A3 + A4): while the surface
        // animates — ease-in from the anchored edge after a show,
        // ease-out back to it before a hide's destroy — the content
        // column's pose is NOT computed here: the view runs in the
        // runtime's AboutToWait pass but the frame presents on the
        // compositor's frame callback, so a view-baked offset is up to
        // one frame stale at presentation — random per-frame error
        // that read as judder on hardware (the round-4 finding).
        // Instead the live slide state and its true start instant are
        // handed to `SlideBox`, which evaluates the easing inside
        // `draw` against the frame's own clock. No animation in
        // flight → offset 0 → the column renders byte-identically to
        // the unwrapped one.
        let mut slide_box = SlideBox::new(column, 0.0);
        if let Some(playing) = self.slide {
            slide_box = slide_box.playing(self.config.edge, playing);
        }
        slide_box.into()
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        // Fires on a window's `Closed` event (cosmic routes it through
        // its internal `SurfaceClosed` action), i.e. after the fact:
        // the settings window's ✕ is already handled by the
        // `CloseRequested` subscription arm, which clears the id and
        // issues the close. This override is the defensive net for a
        // settings window that closed without a handled request —
        // returning the default `None` there would silently leave the
        // dead id in app state for a later press to `gain_focus` into
        // nothing. Layer surfaces keep the default: their `Done`
        // closes are the hide path's own business.
        (self.settings_window == Some(id)).then(|| Message::SettingsCloseRequested(id))
    }

    fn dbus_activation(&mut self, msg: cosmic::dbus_activation::Message) -> Task<Self::Message> {
        use cosmic::dbus_activation::Details;
        match msg.msg {
            Details::Activate => Task::done(cosmic::Action::App(Message::ToggleAction(
                ToggleAction::Toggle,
            ))),
            Details::ActivateAction { action, args } => {
                // The forwarded name plus its args re-parse through the
                // clap definitions (remote-newtab spec: single source of
                // truth for the surface). A malformed forward degrades
                // to a plain toggle, never an error surface.
                let mut words = vec![action];
                words.extend(args);
                let parsed = crate::parse_forwarded_action(&words).unwrap_or_default();
                Task::done(cosmic::Action::App(Message::ToggleAction(parsed)))
            }
            Details::Open { .. } => Task::none(),
        }
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
            // The active tab never accrues a marker (bell-indicator
            // spec): the user is already looking at it.
            TermEvent::Bell => {
                // Bell-indicator spec: only an inactive tab accrues a
                // marker — the user is already looking at the active
                // one.
                if tab_index != self.active_tab {
                    let tab = &mut self.tabs[tab_index];
                    tab.attention = tab.attention.on_bell();
                }
                // Bell-notify spec: a bell while the overlay is hidden
                // posts one desktop notification per tab per hide
                // period (the latch arms here and clears on every
                // show). While visible the marker above alone carries
                // the signal.
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
                let tab = &mut self.tabs[tab_index];
                tabs::apply_shell_title(&mut tab.title, tab.title_override.as_deref(), &title);
                // Title events double as the cwd-change sample point
                // (session-persistence spec, S3); the title itself is
                // not a trigger — churn alone writes nothing.
                return self.check_tab_cwd(tab_index);
            }
            TermEvent::ResetTitle => {
                // A shell reset (RIS) restores the default title, unless
                // the user renamed the tab.
                let tab = &mut self.tabs[tab_index];
                let default_title = tabs::next_default_title(tab.id);
                tabs::apply_shell_title(
                    &mut tab.title,
                    tab.title_override.as_deref(),
                    &default_title,
                );
                return self.check_tab_cwd(tab_index);
            }
            TermEvent::Wakeup => {
                // Output arrived: an inactive tab accrues an activity
                // marker (bell-indicator spec); the active tab shows
                // the output itself, so nothing to signal.
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
                // last, a fresh tab spawns in its place so the overlay
                // is never left without a terminal (see the tabs spec).
                return self.close_tab_at(tab_index);
            }
        }
        Task::none()
    }
}
