// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;
use std::sync::Mutex;

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::term::{self as aterm, Config as TermConfig};
use alacritty_terminal::tty::{Options as PtyOptions, Shell as PtyShell};
use cosmic::app::{Core, Task};
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::event::listen_raw;
use cosmic::iced::keyboard::{Event as KeyEvent, Key, Modifiers};
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    destroy_layer_surface, get_layer_surface, set_anchor, set_exclusive_zone,
    set_keyboard_interactivity, set_layer, set_size,
};
use cosmic::iced::runtime::core::event::PlatformSpecific;
use cosmic::iced::runtime::core::event::wayland::LayerEvent;
use cosmic::iced::{Event, Subscription, clipboard, window};
use cosmic::widget;
use cosmic::{Application, Element};
use tokio::sync::mpsc;

use crate::config::{CONFIG_VERSION, DropTermConfig};
use crate::keybinds::{self, default_key_binds};
use crate::surface;
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
    TermEventTx(mpsc::UnboundedSender<TermEvent>),
    TermEvent(TermEvent),
    SurfaceOpened(window::Id, cosmic::iced::Size),
    LayerFocused(window::Id),
    KeyPressed(Modifiers, Key, cosmic::iced::keyboard::key::Physical),
    WindowFocused,
    WindowUnfocused,
    MiddleClick,
    OpenHyperlink(String),
    ClipboardPasted(Option<String>),
    ClipboardPastedPrimary(Option<String>),
}

pub struct DropTerm {
    core: Core,
    /// Kept for a future settings page (see the ROADMAP's v2 "Settings
    /// page UI" item); v1 only reads config, live-reloaded through
    /// `subscription()`, and never writes it back.
    #[allow(dead_code)]
    config_handler: Option<cosmic_config::Config>,
    config: DropTermConfig,
    terminal: Option<Mutex<Terminal>>,
    terminal_id: widget::Id,
    term_event_tx: Option<mpsc::UnboundedSender<TermEvent>>,
    surface: SurfaceState,
    key_binds: HashMap<cosmic::widget::menu::key_bind::KeyBind, Action>,
    last_toggle: Option<std::time::Instant>,
}

impl DropTerm {
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

    fn apply_config_to_terminal(&mut self) {
        self.refresh_window_bg_color();
        let options = self.terminal_options();
        let colors = self.active_colors();
        if let Some(terminal) = &self.terminal {
            terminal.lock().unwrap().set_config(&options, colors);
        }
    }

    fn spawn_terminal(&mut self) -> Option<Mutex<Terminal>> {
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

        match Terminal::new(
            tx,
            term_config,
            pty_options,
            &self.terminal_options(),
            self.active_colors(),
        ) {
            Ok(terminal) => Some(Mutex::new(terminal)),
            Err(err) => {
                log::error!("failed to spawn shell: {err}");
                None
            }
        }
    }

    fn ensure_terminal(&mut self) {
        if self.terminal.is_none() {
            self.terminal = self.spawn_terminal();
        }
    }

    fn show(&mut self) -> Task<Message> {
        if !matches!(self.surface, SurfaceState::Hidden) {
            return Task::none();
        }
        self.ensure_terminal();
        let id = window::Id::unique();
        self.surface = SurfaceState::Measuring(id);
        get_layer_surface(surface::measuring(id))
    }

    fn hide(&mut self) -> Task<Message> {
        let task = match self.surface {
            SurfaceState::Measuring(id) | SurfaceState::Shown(id) => destroy_layer_surface(id),
            SurfaceState::Hidden => Task::none(),
        };
        self.surface = SurfaceState::Hidden;
        if let Some(terminal) = &self.terminal {
            terminal.lock().unwrap().set_focused(false);
        }
        task
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
        let Some(terminal) = &self.terminal else {
            return Task::none();
        };
        match action {
            Action::Copy => {
                let selected = terminal.lock().unwrap().term.lock().selection_to_string();
                if let Some(text) = selected {
                    return clipboard::write(text);
                }
            }
            Action::CopyPrimary => {
                let selected = terminal.lock().unwrap().term.lock().selection_to_string();
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
                terminal.lock().unwrap().select_all();
            }
            Action::ClearScrollback => {
                terminal
                    .lock()
                    .unwrap()
                    .term
                    .lock()
                    .grid_mut()
                    .clear_history();
            }
            Action::ZoomIn | Action::ZoomOut | Action::ZoomReset => {
                let mut terminal = terminal.lock().unwrap();
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
            terminal: None,
            terminal_id: widget::Id::unique(),
            term_event_tx: None,
            surface: SurfaceState::Hidden,
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
            Event::Window(cosmic::iced::window::Event::Opened { size, .. }) => {
                Some(Message::SurfaceOpened(id, size))
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
                        while let Some(event) = rx.recv().await {
                            if output.send(Message::TermEvent(event)).await.is_err() {
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
        self.apply_config_to_terminal();
        Task::none()
    }

    fn system_theme_mode_update(
        &mut self,
        _keys: &[&'static str],
        _new_theme: &cosmic::cosmic_theme::ThemeMode,
    ) -> Task<Self::Message> {
        self.apply_config_to_terminal();
        Task::none()
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::ToggleAction(action) => return self.debounced_toggle(action),
            Message::ConfigChanged(config) => {
                self.config = config;
                self.apply_config_to_terminal();
            }
            Message::TermEventTx(tx) => {
                if self.term_event_tx.is_none() {
                    self.term_event_tx = Some(tx);
                    self.ensure_terminal();
                }
            }
            Message::TermEvent(event) => return self.handle_term_event(event),
            Message::SurfaceOpened(id, size) => {
                if self.surface == SurfaceState::Measuring(id) {
                    let height_px = ((size.height * f32::from(self.config.height_percent)) / 100.0)
                        .round()
                        .max(1.0) as u32;
                    self.surface = SurfaceState::Shown(id);
                    return Task::batch([
                        set_anchor(id, surface::edge_anchor(self.config.edge)),
                        set_size(id, None, Some(height_px)),
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
            Message::LayerFocused(id) => {
                if self.surface == SurfaceState::Shown(id) {
                    if let Some(terminal) = &self.terminal {
                        terminal.lock().unwrap().set_focused(true);
                    }
                    return widget::text_input::focus(self.terminal_id.clone());
                }
            }
            Message::KeyPressed(modifiers, key, physical_key) => {
                if let Some(action) =
                    keybinds::matched_action(&self.key_binds, modifiers, &key, Some(&physical_key))
                {
                    return self.perform_action(action);
                }
            }
            Message::WindowFocused => {
                if let Some(terminal) = &self.terminal {
                    terminal.lock().unwrap().set_focused(true);
                }
            }
            Message::WindowUnfocused => {
                if let Some(terminal) = &self.terminal {
                    terminal.lock().unwrap().set_focused(false);
                }
                if self.config.hide_on_focus_loss {
                    return self.hide();
                }
            }
            Message::MiddleClick => {
                return clipboard::read_primary()
                    .map(|text| cosmic::Action::App(Message::ClipboardPastedPrimary(text)));
            }
            Message::OpenHyperlink(url) => {
                if let Err(err) = open::that_detached(&url) {
                    log::warn!("failed to open {url:?}: {err}");
                }
            }
            Message::ClipboardPasted(text) | Message::ClipboardPastedPrimary(text) => {
                if let (Some(text), Some(terminal)) = (text, &self.terminal) {
                    terminal.lock().unwrap().paste(text);
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        widget::text("").into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Self::Message> {
        let Some(terminal) = &self.terminal else {
            return widget::text("").into();
        };
        if !matches!(&self.surface, SurfaceState::Measuring(shown) | SurfaceState::Shown(shown) if *shown == id)
        {
            return widget::text("").into();
        }

        let opacity = f32::from(self.config.opacity) / 100.0;
        TerminalBox::new(terminal, &self.key_binds)
            .id(self.terminal_id.clone())
            .opacity(opacity)
            .show_headerbar(false)
            .on_middle_click(|| Message::MiddleClick)
            .on_open_hyperlink(Some(Box::new(Message::OpenHyperlink)))
            .on_window_focused(|| Message::WindowFocused)
            .on_window_unfocused(|| Message::WindowUnfocused)
            .into()
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
    fn handle_term_event(&mut self, event: TermEvent) -> Task<Message> {
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
                if let Some(terminal) = &self.terminal {
                    let terminal = terminal.lock().unwrap();
                    let rgb = terminal.effective_color(index);
                    terminal.input_no_scroll(format(rgb).into_bytes());
                }
            }
            TermEvent::PtyWrite(text) => {
                if let Some(terminal) = &self.terminal {
                    terminal.lock().unwrap().input_no_scroll(text.into_bytes());
                }
            }
            TermEvent::TextAreaSizeRequest(format) => {
                if let Some(terminal) = &self.terminal {
                    let terminal = terminal.lock().unwrap();
                    let text = format(terminal.size().into());
                    terminal.input_no_scroll(text.into_bytes());
                }
            }
            TermEvent::Title(_) | TermEvent::ResetTitle => {
                // No window decorations to set a title on (see the README's
                // known-risks section on layer-shell surfaces).
            }
            TermEvent::MouseCursorDirty | TermEvent::Wakeup => {
                if let Some(terminal) = &self.terminal {
                    terminal.lock().unwrap().needs_update = true;
                }
            }
            TermEvent::Exit => {
                // Respawn immediately rather than waiting for the next
                // Show, so a shell exited while visible doesn't leave a
                // dead terminal on screen.
                self.terminal = self.spawn_terminal();
            }
        }
        Task::none()
    }
}
