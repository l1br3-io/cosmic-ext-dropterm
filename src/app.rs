// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::term::{self as aterm, Config as TermConfig};
use alacritty_terminal::tty::{Options as PtyOptions, Shell as PtyShell};
use cosmic::app::{Core, Task};
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::Length;
use cosmic::iced::event::listen_raw;
use cosmic::iced::keyboard::{Event as KeyEvent, Key, Modifiers};
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    destroy_layer_surface, get_layer_surface, set_exclusive_zone, set_keyboard_interactivity,
    set_layer,
};
use cosmic::iced::runtime::core::event::PlatformSpecific;
use cosmic::iced::runtime::core::event::wayland::LayerEvent;
use cosmic::iced::{Event, Subscription, clipboard, window};
use cosmic::widget;
use cosmic::{Application, Element};
use tokio::sync::mpsc;

use crate::config::{CONFIG_VERSION, DropTermConfig, Edge};
use crate::geometry::{self, Geometry};
use crate::keybinds::{self, default_key_binds};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::Subcommand)]
pub enum ToggleAction {
    #[default]
    Toggle,
    Show,
    Hide,
}

impl std::fmt::Display for ToggleAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Toggle => write!(f, "toggle"),
            Self::Show => write!(f, "show"),
            Self::Hide => write!(f, "hide"),
        }
    }
}

impl std::str::FromStr for ToggleAction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "toggle" => Ok(Self::Toggle),
            "show" => Ok(Self::Show),
            "hide" => Ok(Self::Hide),
            other => Err(format!("unknown action {other:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SurfaceState {
    Hidden,
    /// A surface stretched across the whole active output, waiting for its
    /// `Window::Opened` event so we learn that output's real size.
    Measuring(window::Id),
    Shown(window::Id),
}

#[derive(Debug, Clone)]
pub enum Message {
    ToggleAction(ToggleAction),
    ConfigChanged(DropTermConfig),
    TermEventTx(mpsc::UnboundedSender<(TabId, TermEvent)>),
    TermEvent(TabId, TermEvent),
    SurfaceOpened(window::Id, cosmic::iced::Size),
    /// The granted size of the shown surface changed (compositor
    /// configure after a `set_size`, or the initial configure): the new
    /// height in logical pixels, used to pin the terminal's height so the
    /// tab bar keeps its strip.
    WindowResized(f32),
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
    OpenHyperlink(String),
    ClipboardPasted(Option<String>),
    ClipboardPastedPrimary(Option<String>),
}

/// An in-flight inline tab rename (tabs spec, T6): the tab being renamed
/// and the current draft. The input itself is an overlay the tab bar
/// hosts over the renamed tab's button.
struct RenameEdit {
    tab: TabId,
    draft: String,
}

pub struct DropTerm {
    core: Core,
    /// Kept for a future settings page (see the ROADMAP's v2 "Settings
    /// page UI" item); v1 only reads config, live-reloaded through
    /// `subscription()`, and never writes it back.
    #[allow(dead_code)]
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
    /// The active inline rename, if any (tabs spec, T6): which tab is
    /// being renamed and the current draft, mirrored into the input the
    /// tab bar overlays on the renamed button.
    rename: Option<RenameEdit>,
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
    /// Whether the surface has received keyboard focus at least once since it
    /// was shown. Guards `hide_on_focus_loss` so a stray `Unfocused` delivered
    /// before any `Focused` (possible with `keyboard_mode: OnDemand`) can't
    /// make the surface open-then-instantly-close.
    surface_focused: bool,
    key_binds: HashMap<cosmic::widget::menu::key_bind::KeyBind, Action>,
    last_toggle: Option<std::time::Instant>,
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
        let is_dark = cosmic::theme::is_dark();
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
    fn apply_config_to_tabs(&mut self) {
        self.refresh_window_bg_color();
        let options = self.terminal_options();
        let colors = self.active_colors();
        for tab in &self.tabs {
            tab.terminal.lock().unwrap().set_config(&options, colors);
        }
    }

    /// Mirrors cosmic-term's own `update_config`: the effective window
    /// background color is a process-global (`terminal::WINDOW_BG_COLOR`)
    /// used as a fallback wherever a color scheme leaves "background"
    /// unset, so it must be refreshed whenever the active COSMIC theme
    /// changes.
    fn refresh_window_bg_color(&self) {
        let theme = cosmic::theme::active();
        let color = cosmic::iced::Color::from(theme.cosmic().background(theme.transparent).base);
        let bytes = color.into_rgba8();
        let data = u32::from(bytes[2])
            | (u32::from(bytes[1]) << 8)
            | (u32::from(bytes[0]) << 16)
            | 0xFF00_0000;
        terminal::WINDOW_BG_COLOR.store(data, std::sync::atomic::Ordering::SeqCst);
    }

    fn spawn_tab(&mut self) -> Option<Tab> {
        let Some(tx) = self.term_event_tx.clone() else {
            log::warn!("no terminal event channel yet; deferring terminal spawn");
            return None;
        };

        let term_config = TermConfig {
            scrolling_history: self.config.scrollback_lines as usize,
            ..TermConfig::default()
        };
        let pty_options = PtyOptions {
            shell: Some(PtyShell::new(
                self.config.resolved_shell(),
                self.config.args.clone(),
            )),
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
            Ok(terminal) => Some(Tab::new(tab_id, terminal)),
            Err(err) => {
                log::error!("failed to spawn shell: {err}");
                None
            }
        }
    }

    /// Guarantees at least one tab exists, spawning a fresh one if needed.
    fn ensure_tabs(&mut self) {
        if self.tabs.is_empty()
            && let Some(tab) = self.spawn_tab()
        {
            self.active_tab = tabs::insert_after_active(&mut self.tabs, self.active_tab, tab);
        }
    }

    fn show(&mut self) -> Task<Message> {
        if !matches!(self.surface, SurfaceState::Hidden) {
            return Task::none();
        }
        self.ensure_tabs();
        let id = window::Id::unique();
        self.surface = SurfaceState::Measuring(id);
        self.surface_focused = false;
        get_layer_surface(surface::measuring(id))
    }

    fn hide(&mut self) -> Task<Message> {
        // Hiding the surface is a focus loss for the rename input (T6):
        // commit before the surface goes away.
        let commit = self.commit_pending_rename();
        let task = match self.surface {
            SurfaceState::Measuring(id) | SurfaceState::Shown(id) => destroy_layer_surface(id),
            SurfaceState::Hidden => Task::none(),
        };
        self.surface = SurfaceState::Hidden;
        if let Some(tab) = self.active() {
            tab.terminal.lock().unwrap().set_focused(false);
        }
        Task::batch([commit, task])
    }

    /// Handles the surface losing keyboard focus: marks the terminal unfocused
    /// and, when configured, hides the overlay. The hide is guarded by the
    /// focus latch so a stray unfocus before any focus (possible with
    /// `keyboard_mode: OnDemand`) can't close a freshly-opened surface.
    fn focus_lost(&mut self) -> Task<Message> {
        // Losing keyboard focus commits a rename (T6): the input's own
        // unfocus signal only covers Escape and Tab, which cancel.
        let commit = self.commit_pending_rename();
        if let Some(tab) = self.active() {
            tab.terminal.lock().unwrap().set_focused(false);
        }
        if self.config.hide_on_focus_loss && self.surface_focused {
            self.surface_focused = false;
            return Task::batch([commit, self.hide()]);
        }
        commit
    }

    fn debounced_toggle(&mut self, action: ToggleAction) -> Task<Message> {
        let now = std::time::Instant::now();
        if let Some(last) = self.last_toggle
            && now.duration_since(last) < TOGGLE_DEBOUNCE
        {
            return Task::none();
        }
        self.last_toggle = Some(now);

        let showing = !matches!(self.surface, SurfaceState::Hidden);
        match action {
            ToggleAction::Toggle => {
                if showing {
                    self.hide()
                } else {
                    self.show()
                }
            }
            ToggleAction::Show => {
                if showing {
                    Task::none()
                } else {
                    self.show()
                }
            }
            ToggleAction::Hide => self.hide(),
        }
    }

    fn perform_action(&mut self, action: Action) -> Task<Message> {
        // Hiding must work even before a terminal exists (the first-run dead
        // end this fixes: `cargo run` opens a surface with no way to close it).
        if action == Action::Hide {
            return self.hide();
        }
        // The tab actions reshape the tab list itself — including keeping
        // it non-empty — so they run before the active-tab borrow below
        // and never reach the per-terminal actions.
        if matches!(
            action,
            Action::NewTab | Action::CloseTab | Action::NextTab | Action::PrevTab
        ) {
            return self.perform_tab_action(action);
        }
        let Some(tab) = self.active() else {
            return Task::none();
        };
        match action {
            Action::Hide
            | Action::NewTab
            | Action::CloseTab
            | Action::NextTab
            | Action::PrevTab => unreachable!("handled above"),
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
                if let Some(tab) = self.spawn_tab() {
                    // Yakuake-style placement: directly after the active
                    // tab, which the new tab immediately becomes.
                    let at = tabs::insert_after_active(&mut self.tabs, self.active_tab, tab);
                    self.set_active_tab(at);
                }
            }
            Action::CloseTab => return self.close_tab_at(self.active_tab),
            Action::NextTab => {
                self.set_active_tab(tabs::cycle_next(&self.tabs, self.active_tab));
            }
            Action::PrevTab => {
                self.set_active_tab(tabs::cycle_prev(&self.tabs, self.active_tab));
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
        if let Some(decision) = tabs::close_tab(&mut self.tabs, index, self.active_tab) {
            self.active_tab = decision.next_active;
            if decision.respawn {
                self.ensure_tabs();
            }
            self.focus_active_tab();
        }
        // A close can orphan the rename input (T6): its tab may be gone,
        // or the bar may have vanished with a single tab left. End the
        // session so the terminal regains keyboard focus.
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
            return widget::text_input::focus(self.terminal_id.clone());
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
    /// incoming one), the same way a window focus change does.
    fn set_active_tab(&mut self, index: usize) {
        // Cycling with a single tab is a no-op; refocusing it anyway would
        // send the shell a spurious FOCUS_OUT/FOCUS_IN pair.
        if index == self.active_tab {
            return;
        }
        if let Some(old) = self.active() {
            old.terminal.lock().unwrap().set_focused(false);
        }
        self.active_tab = index;
        self.focus_active_tab();
    }

    /// Marks the active tab's terminal focused. Used after the tab list
    /// changed shape, where the tab that lost focus is already gone.
    fn focus_active_tab(&mut self) {
        if let Some(tab) = self.active() {
            tab.terminal.lock().unwrap().set_focused(true);
        }
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

        let mut app = Self {
            core,
            config_handler,
            config,
            tabs: Vec::new(),
            active_tab: 0,
            terminal_id: widget::Id::unique(),
            term_event_tx: None,
            next_tab_id: 0,
            rename: None,
            rename_input_id: widget::Id::unique(),
            surface: SurfaceState::Hidden,
            output_size: None,
            window_height: None,
            surface_focused: false,
            key_binds: default_key_binds(),
            last_toggle: None,
        };
        app.refresh_window_bg_color();

        // A second launch never reaches this: `run_single_instance` forwards
        // its action over D-Bus to the already-running instance instead
        // (see `Application::dbus_activation` below), so the first launch's
        // own action has to be applied here.
        let task = app.debounced_toggle(flags.action);
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
                Some(Message::WindowResized(size.height))
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
                self.config = config;
                self.apply_config_to_tabs();
                // Geometry edits reshape a shown surface in place
                // (geometry spec, G2) — no destroy/recreate; a hidden
                // surface just opens with the new geometry. Non-geometry
                // edits leave the surface alone.
                if geometry::geometry_changed(&old_geom, &new_geom)
                    && let (SurfaceState::Shown(id), Some(output)) =
                        (&self.surface, self.output_size)
                {
                    return geometry::apply_geometry(*id, &new_geom, output);
                }
            }
            Message::TermEventTx(tx) => {
                if self.term_event_tx.is_none() {
                    self.term_event_tx = Some(tx);
                    self.ensure_tabs();
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
                    return Task::batch([
                        geometry::apply_geometry(id, &geom, output),
                        set_layer(id, surface::wlr_layer(self.config.layer)),
                        set_keyboard_interactivity(
                            id,
                            surface::keyboard_interactivity(self.config.keyboard_mode),
                        ),
                        set_exclusive_zone(id, -1),
                        widget::text_input::focus(self.terminal_id.clone()),
                    ]);
                }
            }
            Message::WindowResized(height) => {
                self.window_height = Some(height);
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
                if self.surface == SurfaceState::Shown(id) {
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
                    self.set_active_tab(index);
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
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        widget::text("").into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Self::Message> {
        let Some(tab) = self.active() else {
            return widget::text("").into();
        };
        if !matches!(&self.surface, SurfaceState::Measuring(shown) | SurfaceState::Shown(shown) if *shown == id)
        {
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
            .into();

        // The bar appears only with more than one tab and sits on the
        // surface edge opposite the screen edge (tabs spec): below the
        // terminal when top-anchored, above it when bottom-anchored.
        if self.tabs.len() > 1 {
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
            // Fill dimensions keep iced's flex from deferring the bar
            // past the terminal's allocation: a Shrink cross-axis makes
            // the fixed-height, fill-width bar a fourth-pass child, so
            // the Fill terminal would take the whole surface and push
            // the bar below its bottom edge.
            match self.config.edge {
                Edge::Top => widget::column![terminal, bar]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                Edge::Bottom => widget::column![bar, terminal]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
            }
        } else {
            terminal
        }
    }

    fn dbus_activation(&mut self, msg: cosmic::dbus_activation::Message) -> Task<Self::Message> {
        use cosmic::dbus_activation::Details;
        match msg.msg {
            Details::Activate => Task::done(cosmic::Action::App(Message::ToggleAction(
                ToggleAction::Toggle,
            ))),
            Details::ActivateAction { action, .. } => {
                let action = action.parse().unwrap_or_default();
                Task::done(cosmic::Action::App(Message::ToggleAction(action)))
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
            TermEvent::Bell | TermEvent::CursorBlinkingChange | TermEvent::ChildExit(_) => {}
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
            }
            TermEvent::MouseCursorDirty | TermEvent::Wakeup => {
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
