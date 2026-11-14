// SPDX-License-Identifier: GPL-3.0-only

//! The settings drawer on GPUI (gpui-chrome Task 2): the Kit's
//! Sheet over the live terminal — the surface never hides or resizes
//! while it is open, the libcosmic drawer's overlay rule. The pages,
//! controls, and write contract are the libcosmic drawer's (the
//! chunk's ruling: ported, not redesigned): every control maps to its
//! config key through core's moved model, every commit is one
//! write-through at its commit point — toggles and dropdowns on
//! change, texts on submit, sliders on release, pickers on pick;
//! never an Apply button, never a batch — and the config watcher's
//! echo drives every apply, so drawer writes and hand edits remain
//! one event stream.
//!
//! The drawer's own state (the open page, the drafts buffer, the
//! profile draft, the keybind capture) lives on the shell, the
//! libcosmic app's own shape; this entity holds the Kit's stateful
//! halves — the text inputs, sliders, and pickers, built once at open
//! — and renders the open page from the shell's live state each
//! frame. The Profiles and Keybindings pages arrive with this task's
//! later increments; their tabs ride [`SettingsTab::ALL`] in the
//! strip with an honest placeholder page until then.

use std::rc::Rc;

use dropterm_core::config::{
    DropTermConfig, MAX_HEIGHT_PERCENT, MAX_WIDTH_PERCENT, MIN_HEIGHT_PERCENT, MIN_WIDTH_PERCENT,
};
use dropterm_core::font_list;
use dropterm_core::settings::{
    self, ConfigKey, EDGE_OPTIONS, FONT_SIZE_MAX, FONT_SIZE_MIN, H_ALIGN_OPTIONS,
    KEYBOARD_MODE_OPTIONS, LAYER_OPTIONS, NEW_TAB_CWD_OPTIONS, SettingsBuffer, SettingsTab,
    TAB_POSITION_OPTIONS, TAB_TITLE_MODE_OPTIONS, THEME_PREFERENCE_OPTIONS, TextField,
    drawer_description, dropdown_selected, etc_shells_content, shell_dropdown_selected,
    shell_label, shell_picker_options, tab_header, tab_sections, toggle_write,
};
use gpui::prelude::*;
use gpui::{
    App, Context, Entity, InteractiveElement, Render, SharedString, StatefulInteractiveElement,
    Styled, Window,
};
use gpui_component::ActiveTheme as _;
use gpui_component::input::{Input, InputState};
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::switch::Switch;
use gpui_component::{h_flex, v_flex};

use crate::app_shell::Shell;
use crate::pickers::{self, FontPickerHandle, SchemePickerHandle};

// --- the control map's rendering face ---

/// Which Kit control a key renders — the dispatch the libcosmic view
/// folded into its per-control builders, made one named table here so
/// the section pages render every key through it and the map's
/// rendering face is pinned headlessly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlKind {
    /// A switch, committed on change.
    Toggle,
    /// A slider, committed on release (moves are display-only).
    Slider,
    /// A closed-list dropdown over an option table, committed on
    /// change.
    Dropdown,
    /// A text input, committed on submit.
    Text,
    /// The font picker (Task 1's component).
    FontPicker,
    /// A scheme picker (Task 1's component) — the dark or light
    /// global control.
    SchemePicker,
    /// The shell picker: `/etc/shells` rows plus the custom-entry
    /// text input, one control (the libcosmic `shell_control`'s
    /// shape).
    ShellPicker,
}

/// The control a section-hosted key renders. Every key a `Section`
/// lists resolves — pinned against `tab_sections`'s partition below —
/// and the two keys no section hosts (`SettingsDrawerWidth`, the
/// drag handle's writer; `SettingsDrawerClickClose`, its row hidden
/// while its module is parked) resolve to nothing.
pub(crate) fn control_kind(key: ConfigKey) -> Option<ControlKind> {
    let kind = match key {
        ConfigKey::FontName => ControlKind::FontPicker,
        ConfigKey::ColorSchemeDark | ConfigKey::ColorSchemeLight => ControlKind::SchemePicker,
        ConfigKey::Shell => ControlKind::ShellPicker,
        ConfigKey::HeightPercent
        | ConfigKey::WidthPercent
        | ConfigKey::Opacity
        | ConfigKey::FontSize => ControlKind::Slider,
        ConfigKey::HAlign
        | ConfigKey::Edge
        | ConfigKey::TabPosition
        | ConfigKey::TabTitleMode
        | ConfigKey::NewTabCwd
        | ConfigKey::Layer
        | ConfigKey::KeyboardMode
        | ConfigKey::ThemePreference => ControlKind::Dropdown,
        ConfigKey::Args
        | ConfigKey::DefaultCwd
        | ConfigKey::ScrollbackLines
        | ConfigKey::CommandFinishNotificationAfterSecs
        | ConfigKey::AutosuggestionsDelayMs => ControlKind::Text,
        ConfigKey::SettingsDrawerWidth | ConfigKey::SettingsDrawerClickClose => return None,
        // Every remaining exposed key is a toggle — the section map's
        // own exactly-once pin (core's tests) guarantees this arm is
        // exactly the toggle set.
        _ => ControlKind::Toggle,
    };
    Some(kind)
}

/// The option rows a dropdown key lists, in table order — the labels
/// the closed control and its popover both render.
fn dropdown_options(key: ConfigKey) -> Vec<SharedString> {
    let labels: Vec<&str> = match key {
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
        ConfigKey::NewTabCwd => NEW_TAB_CWD_OPTIONS
            .iter()
            .map(|(_, label)| *label)
            .collect(),
        ConfigKey::Layer => LAYER_OPTIONS.iter().map(|(_, label)| *label).collect(),
        ConfigKey::KeyboardMode => KEYBOARD_MODE_OPTIONS
            .iter()
            .map(|(_, label)| *label)
            .collect(),
        ConfigKey::ThemePreference => THEME_PREFERENCE_OPTIONS
            .iter()
            .map(|(_, label)| *label)
            .collect(),
        _ => Vec::new(),
    };
    labels.iter().map(|label| (*label).into()).collect()
}

/// A slider's band — the control's own range; the write path's
/// clamps are the model's (`clamped`), applied on commit whatever
/// the control produced.
fn slider_band(key: ConfigKey) -> (f32, f32) {
    match key {
        ConfigKey::HeightPercent => (f32::from(MIN_HEIGHT_PERCENT), f32::from(MAX_HEIGHT_PERCENT)),
        ConfigKey::WidthPercent => (f32::from(MIN_WIDTH_PERCENT), f32::from(MAX_WIDTH_PERCENT)),
        ConfigKey::FontSize => (f32::from(FONT_SIZE_MIN), f32::from(FONT_SIZE_MAX)),
        // Opacity's band is the model's clamp band, like every key
        // without its own arm here.
        _ => (0.0, 100.0),
    }
}

/// The slider's live value: the buffered position while one exists,
/// else the file's — the buffer rule, read once for the control and
/// the readout.
fn slider_value(key: ConfigKey, buffer: &SettingsBuffer, config: &DropTermConfig) -> f32 {
    buffer.slider_value(key, config)
}

// --- the Kit-state bundle built once at open ---

/// The text inputs (one per `TextField`, the six text keys' control).
type InputMap = Vec<(TextField, Entity<InputState>)>;

/// The dropdowns (one per closed-list key).
type DropdownMap = Vec<(ConfigKey, Entity<SelectState<Vec<SharedString>>>)>;

/// The shell field's control: the picker beside the custom-entry
/// input (one subscription keeping the picker's picks writing the
/// same key the input submits to).
struct ShellControl {
    picker: Entity<SelectState<Vec<SharedString>>>,
    input: Entity<InputState>,
    _subscription: gpui::Subscription,
}

/// The drawer's shell-owned state (the libcosmic app's own shape):
/// the open flag, the open page, the display-only drafts buffer
/// (core's), and the guard that keeps an echo's programmatic input
/// refresh from counting as typing.
pub(crate) struct SettingsState {
    pub(crate) open: bool,
    pub(crate) page: SettingsTab,
    pub(crate) buffer: SettingsBuffer,
    pub(crate) refreshing: bool,
    /// Bumped on every config reload the shell drains: the drawer's
    /// cue to re-seed its un-drafted Kit states from the new file —
    /// a hand edit mid-open reaches the controls the next frame.
    pub(crate) config_epoch: u64,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            open: false,
            page: SettingsTab::Appearance,
            buffer: SettingsBuffer::default(),
            refreshing: false,
            config_epoch: 0,
        }
    }
}

/// The drawer's Kit halves: the stateful controls, built once per
/// open from the live config, seeded on the file's values (drafts
/// are empty at open — the buffer's reopen-fresh rule).
pub(crate) struct SettingsDrawer {
    shell: Entity<Shell>,
    inputs: InputMap,
    sliders: Vec<(ConfigKey, Entity<SliderState>)>,
    dropdowns: DropdownMap,
    font_picker: FontPickerHandle,
    scheme_dark: SchemePickerHandle,
    scheme_light: SchemePickerHandle,
    shell_control: ShellControl,
    /// The epoch the Kit states were last seeded from; a change
    /// triggers the echo-refresh pass in render.
    seen_epoch: u64,
    _observe: gpui::Subscription,
}

impl SettingsDrawer {
    /// Build every Kit state against the shell's live config. The
    /// event subscriptions route to the shell's funnels (the buffer,
    /// the write path) — the drawer never applies anything itself,
    /// the echo drives every apply.
    pub(crate) fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let config = shell.read(cx).config.clone();
        let families = font_list::load_monospace_families();
        let inputs = Self::build_inputs(&shell, &config, window, cx);
        let sliders = Self::build_sliders(&shell, &config, window, cx);
        let dropdowns = Self::build_dropdowns(&shell, &config, window, cx);
        let font_picker = Self::build_font_picker(&shell, &families, &config, window, cx);
        let scheme_dark = Self::build_scheme_picker(
            &shell,
            &config.color_scheme_dark,
            ConfigKey::ColorSchemeDark,
            window,
            cx,
        );
        let scheme_light = Self::build_scheme_picker(
            &shell,
            &config.color_scheme_light,
            ConfigKey::ColorSchemeLight,
            window,
            cx,
        );
        let shell_control = Self::build_shell_control(&shell, &config, &inputs, window, cx);
        cx.new(|cx| {
            let observe = cx.observe(&shell, |_, _, cx| cx.notify());
            Self {
                shell,
                inputs,
                sliders,
                dropdowns,
                font_picker,
                scheme_dark,
                scheme_light,
                shell_control,
                seen_epoch: 0,
                _observe: observe,
            }
        })
    }

    /// The text inputs: drafts buffer on change (guarded — an echo's
    /// programmatic refresh must not count as typing), submits commit
    /// through the planner.
    fn build_inputs(
        shell: &Entity<Shell>,
        config: &DropTermConfig,
        window: &mut Window,
        cx: &mut App,
    ) -> InputMap {
        let mut inputs = Vec::new();
        for field in [
            TextField::Shell,
            TextField::Args,
            TextField::ScrollbackLines,
            TextField::CommandFinishNotificationAfterSecs,
            TextField::AutosuggestionsDelayMs,
            TextField::DefaultCwd,
        ] {
            let seeded = SettingsBuffer::default().text(field, config);
            let state = cx.new(|cx| InputState::new(window, cx).default_value(seeded));
            let target = shell.clone();
            cx.subscribe(
                &state,
                move |emitter: Entity<InputState>,
                      event: &gpui_component::input::InputEvent,
                      cx| {
                    match event {
                        gpui_component::input::InputEvent::Change => {
                            let value = emitter.read(cx).value().to_string();
                            target.update(cx, |shell, _| {
                                if !shell.settings.refreshing {
                                    shell.settings.buffer.set_text(field, value);
                                }
                            });
                        }
                        gpui_component::input::InputEvent::PressEnter { .. } => {
                            let raw = emitter.read(cx).value().to_string();
                            target.update(cx, |shell, _| {
                                if let Some(write) =
                                    settings::submit_write(field, &raw).map(|w| w.clamped())
                                {
                                    shell.write_settings(&write);
                                }
                                shell.settings.buffer.clear_text(field);
                            });
                        }
                        _ => {}
                    }
                },
            )
            .detach();
            inputs.push((field, state));
        }
        inputs
    }

    /// The sliders: moves buffer (display-only), release commits the
    /// buffered position (the libcosmic release rule — the buffer
    /// keeps it, cleared only at drawer close).
    fn build_sliders(
        shell: &Entity<Shell>,
        config: &DropTermConfig,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<(ConfigKey, Entity<SliderState>)> {
        let mut sliders = Vec::new();
        for key in [
            ConfigKey::HeightPercent,
            ConfigKey::WidthPercent,
            ConfigKey::Opacity,
            ConfigKey::FontSize,
        ] {
            let (min, max) = slider_band(key);
            let value = slider_value(key, &SettingsBuffer::default(), config);
            let state = cx.new(|cx| {
                let mut slider = SliderState::new().min(min).max(max);
                slider.set_value(value, window, cx);
                slider
            });
            let target = shell.clone();
            cx.subscribe(&state, move |_, event: &SliderEvent, cx| match event {
                SliderEvent::Change(value) => {
                    if let gpui_component::slider::SliderValue::Single(position) = value {
                        target.update(cx, |shell, _| {
                            if !shell.settings.refreshing {
                                shell.settings.buffer.set_slider(key, *position);
                            }
                        });
                    }
                }
                SliderEvent::Release(_) => {
                    target.update(cx, |shell, _| {
                        let buffered = shell.settings.buffer.slider_value(key, &shell.config);
                        if let Some(write) =
                            settings::slider_release_write(key, buffered).map(|w| w.clamped())
                        {
                            shell.write_settings(&write);
                        }
                    });
                }
            })
            .detach();
            sliders.push((key, state));
        }
        sliders
    }

    /// The dropdowns: picks commit on change through the planner.
    fn build_dropdowns(
        shell: &Entity<Shell>,
        config: &DropTermConfig,
        window: &mut Window,
        cx: &mut App,
    ) -> DropdownMap {
        let mut dropdowns = Vec::new();
        for key in [
            ConfigKey::HAlign,
            ConfigKey::Edge,
            ConfigKey::TabPosition,
            ConfigKey::TabTitleMode,
            ConfigKey::NewTabCwd,
            ConfigKey::Layer,
            ConfigKey::KeyboardMode,
            ConfigKey::ThemePreference,
        ] {
            let options = dropdown_options(key);
            let selected = Some(dropdown_selected(key, config).unwrap_or(0))
                .map(|row| gpui_component::IndexPath::default().row(row));
            let state = cx.new(|cx| SelectState::new(options, selected, window, cx));
            let target = shell.clone();
            cx.subscribe(
                &state,
                move |_emitter: Entity<SelectState<Vec<SharedString>>>,
                      event: &SelectEvent<Vec<SharedString>>,
                      cx| {
                    if let SelectEvent::Confirm(Some(label)) = event {
                        let row = dropdown_row_of(key, label);
                        target.update(cx, |shell, _| {
                            if let Some(write) =
                                settings::dropdown_write(key, row).map(|w| w.clamped())
                            {
                                shell.write_settings(&write);
                            }
                        });
                    }
                },
            )
            .detach();
            dropdowns.push((key, state));
        }
        dropdowns
    }

    /// The font picker over the startup family scan.
    fn build_font_picker(
        shell: &Entity<Shell>,
        families: &[String],
        config: &DropTermConfig,
        window: &mut Window,
        cx: &mut App,
    ) -> FontPickerHandle {
        let target = shell.clone();
        let on_pick: pickers::FontPickFn = Rc::new(move |family, cx| {
            target.update(cx, |shell, _| {
                shell.write_settings(&settings::PlannedWrite::FontName(family));
            });
        });
        pickers::font_picker(families, &config.font_name, &on_pick, window, cx)
    }

    /// One global scheme picker over the registry.
    fn build_scheme_picker(
        shell: &Entity<Shell>,
        stored: &str,
        key: ConfigKey,
        window: &mut Window,
        cx: &mut App,
    ) -> SchemePickerHandle {
        let target = shell.clone();
        let on_pick: pickers::SchemePickFn = Rc::new(move |name, cx| {
            let write = match key {
                ConfigKey::ColorSchemeDark => {
                    settings::PlannedWrite::ColorSchemeDark(name.to_string())
                }
                _ => settings::PlannedWrite::ColorSchemeLight(name.to_string()),
            };
            target.update(cx, |shell, _| shell.write_settings(&write));
        });
        pickers::scheme_picker(stored, &on_pick, window, cx)
    }

    /// The shell field's control: the `/etc/shells` picker beside the
    /// custom-entry input.
    fn build_shell_control(
        shell: &Entity<Shell>,
        config: &DropTermConfig,
        inputs: &InputMap,
        window: &mut Window,
        cx: &mut App,
    ) -> ShellControl {
        let shell_rows = shell_picker_options(etc_shells_content().as_deref());
        let shell_options: Vec<SharedString> = shell_rows
            .iter()
            .map(|path| SharedString::from(shell_label(path)))
            .collect();
        let shell_selected = shell_dropdown_selected(&shell_rows, config.shell.as_deref())
            .map(|row| gpui_component::IndexPath::default().row(row));
        let picker = cx.new(|cx| SelectState::new(shell_options, shell_selected, window, cx));
        let input = inputs
            .iter()
            .find(|(field, _)| *field == TextField::Shell)
            .map(|(_, state)| state.clone())
            .expect("the shell input is built beside it");
        let target = shell.clone();
        let subscription = cx.subscribe(
            &picker,
            move |_emitter: Entity<SelectState<Vec<SharedString>>>,
                  event: &SelectEvent<Vec<SharedString>>,
                  cx| {
                if let SelectEvent::Confirm(Some(label)) = event {
                    let options = shell_picker_options(etc_shells_content().as_deref());
                    if let Some(row) = options
                        .iter()
                        .position(|path| shell_label(path) == label.as_ref())
                        && let Some(write) = settings::shell_pick_write(row).map(|w| w.clamped())
                    {
                        target.update(cx, |shell, _| shell.write_settings(&write));
                    }
                }
            },
        );
        ShellControl {
            picker,
            input,
            _subscription: subscription,
        }
    }
}

impl SettingsDrawer {
    /// One row of a section card: the key's label, its description,
    /// and its control.
    fn control_row(
        &self,
        key: ConfigKey,
        config: &DropTermConfig,
        window: &mut Window,
        cx: &App,
    ) -> gpui::AnyElement {
        let shell = self.shell.clone();
        let label = key.label();
        let description = drawer_description(key);
        let control: gpui::AnyElement = match control_kind(key).expect("section keys resolve") {
            ControlKind::Toggle => {
                let checked = toggle_state(key, config);
                let toggle = Switch::new(key.key_name()).checked(checked).on_click(
                    move |next: &bool, _window, cx| {
                        if let Some(write) = toggle_write(key, *next) {
                            shell.update(cx, |shell, _| shell.write_settings(&write));
                        }
                    },
                );
                h_flex().child(toggle).into_any_element()
            }
            ControlKind::Slider => {
                let Some((_, state)) = self.sliders.iter().find(|(k, _)| *k == key) else {
                    return placeholder_row(key).into_any_element();
                };
                let value = slider_value(key, &self.shell.read(cx).settings.buffer, config);
                v_flex()
                    .child(Slider::new(state))
                    .child(h_flex().justify_end().child(format!("{value:.0}")))
                    .into_any_element()
            }
            ControlKind::Dropdown => {
                let Some((_, state)) = self.dropdowns.iter().find(|(k, _)| *k == key) else {
                    return placeholder_row(key).into_any_element();
                };
                Select::new(state).into_any_element()
            }
            ControlKind::Text => {
                let Some(field) = text_field_of(key) else {
                    return placeholder_row(key).into_any_element();
                };
                let Some((_, state)) = self.inputs.iter().find(|(f, _)| *f == field) else {
                    return placeholder_row(key).into_any_element();
                };
                Input::new(state).into_any_element()
            }
            ControlKind::FontPicker => Select::new(&self.font_picker.state).into_any_element(),
            ControlKind::SchemePicker => {
                let state = if key == ConfigKey::ColorSchemeDark {
                    &self.scheme_dark.state
                } else {
                    &self.scheme_light.state
                };
                Select::new(state).into_any_element()
            }
            ControlKind::ShellPicker => v_flex()
                .child(Select::new(&self.shell_control.picker))
                .child(Input::new(&self.shell_control.input))
                .into_any_element(),
        };
        let _ = window;
        row_card(label, description, control).into_any_element()
    }

    /// The open page: the section cards (the Profiles and
    /// Keybindings pages carry their own bodies — their increments).
    fn page_element(&self, window: &mut Window, cx: &App) -> gpui::AnyElement {
        let config = self.shell.read(cx).config.clone();
        let page = self.shell.read(cx).settings.page;
        let mut content = v_flex()
            .id("settings-page")
            .gap_4()
            .p_4()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        match page {
            SettingsTab::Profiles => {
                content = content.child(placeholder_page(
                    "The profile manager arrives with this task's next increment.",
                ));
            }
            SettingsTab::Keybindings => {
                content = content.child(placeholder_page(
                    "The keybind editor arrives with this task's next increment.",
                ));
            }
            _ => {
                for section in tab_sections(page) {
                    let mut card = v_flex().gap_2().p_4().rounded_lg().bg(cx.theme().popover);
                    card = card.child(
                        h_flex()
                            .child(section.title())
                            .text_color(cx.theme().popover_foreground),
                    );
                    for key in section.keys() {
                        card = card.child(self.control_row(*key, &config, window, cx));
                    }
                    content = content.child(card);
                }
            }
        }
        content.into_any_element()
    }
}

/// The toggle keys' file state — the libcosmic `toggle_state`'s table
/// over the moved model.
fn toggle_state(key: ConfigKey, config: &DropTermConfig) -> bool {
    match key {
        ConfigKey::HideOnFocusLoss => config.hide_on_focus_loss,
        ConfigKey::SessionPersistenceEnabled => config.session_persistence_enabled,
        ConfigKey::SlideAnimationEnabled => config.slide_animation_enabled,
        ConfigKey::BellNotificationEnabled => config.bell_notification_enabled,
        ConfigKey::CommandFinishNotificationEnabled => config.command_finish_notification_enabled,
        ConfigKey::AutosuggestionsEnabled => config.autosuggestions_enabled,
        ConfigKey::AutosuggestionsPathsEnabled => config.autosuggestions_paths_enabled,
        ConfigKey::AutosuggestionsCommandsEnabled => config.autosuggestions_commands_enabled,
        ConfigKey::AutosuggestionsCompletionsExtended => {
            config.autosuggestions_completions_extended
        }
        ConfigKey::ImagesEnabled => config.images_enabled,
        ConfigKey::AutosuggestionsMenuEnabled => config.autosuggestions_menu_enabled,
        ConfigKey::AutosuggestionsMenuAuto => config.autosuggestions_menu_auto,
        ConfigKey::NewTabAfterCurrent => config.new_tab_after_current,
        ConfigKey::CloseConfirmationEnabled => config.close_confirmation_enabled,
        ConfigKey::SettingsDrawerClickClose => config.settings_drawer_click_close,
        ConfigKey::RespectPanel => config.respect_panel,
        ConfigKey::ShowInDock => config.show_in_dock,
        ConfigKey::StatusBarEnabled => config.status_bar_enabled,
        ConfigKey::TabBarBorder => config.tab_bar_border,
        ConfigKey::StatusBarBorder => config.status_bar_border,
        ConfigKey::StatusBarActivityMark => config.status_bar_activity_mark,
        ConfigKey::StatusBarShowProcess => config.status_bar_show_process,
        ConfigKey::StatusBarShowPath => config.status_bar_show_path,
        ConfigKey::StatusBarShowBranch => config.status_bar_show_branch,
        ConfigKey::StatusBarShowProfile => config.status_bar_show_profile,
        ConfigKey::StatusBarShowGrid => config.status_bar_show_grid,
        ConfigKey::StatusBarShowTab => config.status_bar_show_tab,
        ConfigKey::StatusBarShowZoom => config.status_bar_show_zoom,
        ConfigKey::StatusBarShowClock => config.status_bar_show_clock,
        ConfigKey::StatusBarShowDuration => config.status_bar_show_duration,
        ConfigKey::StatusBarShowExitStatus => config.status_bar_show_exit_status,
        ConfigKey::StatusBarShowProgress => config.status_bar_show_progress,
        ConfigKey::StatusBarShowProgramStatus => config.status_bar_show_program_status,
        ConfigKey::StatusBarShowScrollback => config.status_bar_show_scrollback,
        _ => false,
    }
}

/// A text key's field — the model's own pairings; `None` for a key
/// no text field backs (the dispatch never routes one here).
fn text_field_of(key: ConfigKey) -> Option<TextField> {
    let field = match key {
        ConfigKey::Shell => TextField::Shell,
        ConfigKey::Args => TextField::Args,
        ConfigKey::ScrollbackLines => TextField::ScrollbackLines,
        ConfigKey::CommandFinishNotificationAfterSecs => {
            TextField::CommandFinishNotificationAfterSecs
        }
        ConfigKey::AutosuggestionsDelayMs => TextField::AutosuggestionsDelayMs,
        ConfigKey::DefaultCwd => TextField::DefaultCwd,
        _ => return None,
    };
    Some(field)
}

/// A dropdown's selected row from its picked label — the option
/// tables' order is the row space.
fn dropdown_row_of(key: ConfigKey, label: &SharedString) -> usize {
    dropdown_options(key)
        .iter()
        .position(|option| option == label)
        .unwrap_or(0)
}

fn row_card(label: &str, description: String, control: gpui::AnyElement) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .justify_between()
                .child(label.to_string())
                .child(control),
        )
        .child(description)
}

fn placeholder_row(key: ConfigKey) -> impl IntoElement {
    h_flex().child(key.label())
}

fn placeholder_page(note: &str) -> impl IntoElement {
    v_flex().p_4().child(note.to_string())
}

impl SettingsDrawer {
    /// The echo-refresh pass: after a config reload (a write's echo
    /// or a hand edit), re-seed every Kit state that has no draft
    /// buffered for it — the drawer's un-drafted controls always
    /// read the file, exactly the per-frame re-render the libcosmic
    /// drawer's stateless controls get by construction.
    fn refresh_from_echo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let epoch = self.shell.read(cx).settings.config_epoch;
        if epoch == self.seen_epoch {
            return;
        }
        self.seen_epoch = epoch;
        let (texts, sliders_live) = {
            let shell = self.shell.read(cx);
            let config = shell.config.clone();
            let buffer = &shell.settings.buffer;
            let texts: Vec<(TextField, String)> = self
                .inputs
                .iter()
                .map(|(field, _)| (*field, buffer.text(*field, &config)))
                .collect();
            let sliders_live: Vec<(ConfigKey, f32)> = self
                .sliders
                .iter()
                .map(|(key, _)| (*key, buffer.slider_value(*key, &config)))
                .collect();
            (texts, sliders_live)
        };
        self.shell
            .update(cx, |shell, _| shell.settings.refreshing = true);
        for (field, state) in &self.inputs {
            let Some(live) = texts
                .iter()
                .find(|(f, _)| f == field)
                .map(|(_, t)| t.clone())
            else {
                continue;
            };
            if state.read(cx).value() != live.as_str() {
                state.update(cx, |state, cx| state.set_value(live, window, cx));
            }
        }
        for (key, state) in &self.sliders {
            let Some(live) = sliders_live.iter().find(|(k, _)| k == key).map(|(_, v)| *v) else {
                continue;
            };
            if state.read(cx).value().to_string() != format!("{live:.0}") {
                state.update(cx, |state, cx| state.set_value(live, window, cx));
            }
        }
        self.shell
            .update(cx, |shell, _| shell.settings.refreshing = false);
    }
}

impl Render for SettingsDrawer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_from_echo(window, cx);
        let shell = self.shell.clone();
        let page = self.shell.read(cx).settings.page;
        let mut strip = h_flex().gap_1().p_2();
        for tab in SettingsTab::ALL {
            let selected = tab == page;
            let target = shell.clone();
            strip = strip.child(
                gpui::div()
                    .px_3()
                    .py_1()
                    .rounded_sm()
                    .when(selected, |el| el.bg(cx.theme().accent))
                    .child(tab_header(tab))
                    .on_mouse_down(gpui::MouseButton::Left, move |_, _window, cx| {
                        target.update(cx, |shell, cx| {
                            shell.settings.page = tab;
                            cx.notify();
                        });
                    }),
            );
        }
        v_flex()
            .size_full()
            .child(strip)
            .child(self.page_element(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every section-hosted key resolves to a control, and the two
    /// keys no section hosts resolve to none — the rendering face of
    /// the ported control map, pinned against the section partition.
    #[test]
    fn every_section_key_resolves_a_control_and_no_others_do() {
        for tab in SettingsTab::ALL {
            for section in tab_sections(tab) {
                for key in section.keys() {
                    assert!(
                        control_kind(*key).is_some(),
                        "{key:?} (section {section:?}) renders no control"
                    );
                }
            }
        }
        assert_eq!(control_kind(ConfigKey::SettingsDrawerWidth), None);
        assert_eq!(control_kind(ConfigKey::SettingsDrawerClickClose), None);
    }

    /// The dispatch's classes spot-pinned: the pickers and the shell
    /// control, the four sliders, the eight dropdowns, the six texts,
    /// and one toggle representative — the full partition is the
    /// section-map tests' own exactly-once pin.
    #[test]
    fn the_control_classes_dispatch_as_the_libcosmic_drawer_drew_them() {
        assert_eq!(
            control_kind(ConfigKey::FontName),
            Some(ControlKind::FontPicker)
        );
        assert_eq!(
            control_kind(ConfigKey::ColorSchemeDark),
            Some(ControlKind::SchemePicker)
        );
        assert_eq!(
            control_kind(ConfigKey::Shell),
            Some(ControlKind::ShellPicker)
        );
        for key in [
            ConfigKey::HeightPercent,
            ConfigKey::WidthPercent,
            ConfigKey::Opacity,
            ConfigKey::FontSize,
        ] {
            assert_eq!(control_kind(key), Some(ControlKind::Slider));
        }
        for key in [
            ConfigKey::HAlign,
            ConfigKey::Edge,
            ConfigKey::TabPosition,
            ConfigKey::TabTitleMode,
            ConfigKey::NewTabCwd,
            ConfigKey::Layer,
            ConfigKey::KeyboardMode,
            ConfigKey::ThemePreference,
        ] {
            assert_eq!(control_kind(key), Some(ControlKind::Dropdown));
        }
        for key in [
            ConfigKey::Args,
            ConfigKey::DefaultCwd,
            ConfigKey::ScrollbackLines,
            ConfigKey::CommandFinishNotificationAfterSecs,
            ConfigKey::AutosuggestionsDelayMs,
        ] {
            assert_eq!(control_kind(key), Some(ControlKind::Text));
        }
        assert_eq!(
            control_kind(ConfigKey::HideOnFocusLoss),
            Some(ControlKind::Toggle)
        );
        assert_eq!(
            control_kind(ConfigKey::StatusBarShowClock),
            Some(ControlKind::Toggle)
        );
    }
}
