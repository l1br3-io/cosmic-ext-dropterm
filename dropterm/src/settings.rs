// SPDX-License-Identifier: GPL-3.0-only

//! The settings window on GPUI (settings-window SW3): a plain
//! toplevel hosted by the Kit's Root — the drop-down hides while it
//! is open and returns when it closes, the exclusive pair. The pages,
//! controls, and write contract are the drawer's (ported, not
//! redesigned): every control maps to its config key through core's
//! model, every commit is one write-through at its commit point —
//! toggles and dropdowns on change, texts on submit, sliders on
//! release, pickers on pick; never an Apply button, never a batch —
//! and the config watcher's echo drives every apply, so window
//! writes and hand edits remain one event stream. The view keeps its
//! header and strip until SW5's nav column.
//!
//! The window's own state (the open page, the drafts buffer, the
//! profile draft, the keybind capture) lives on the shell, the
//! libcosmic app's own shape; this entity holds the Kit's stateful
//! halves — the text inputs, sliders, and pickers, built once at
//! open — and renders the open page from the shell's live state each
//! frame. Their pages ride [`SettingsPage::ALL`] in the strip.

use std::iter::once;
use std::rc::Rc;

use dropterm::core::config::{
    DropTermConfig, MAX_HEIGHT_PERCENT, MAX_WIDTH_PERCENT, MIN_HEIGHT_PERCENT, MIN_WIDTH_PERCENT,
};
use dropterm::core::font_list;
use dropterm::core::profile::Profile;
use dropterm::core::settings::{
    self, BlurDecision, ConfigKey, EDGE_OPTIONS, FONT_SIZE_MAX, FONT_SIZE_MIN, H_ALIGN_OPTIONS,
    KEYBOARD_MODE_OPTIONS, LAYER_OPTIONS, NEW_TAB_CWD_OPTIONS, ProfileField, SettingsBuffer,
    SettingsPage, TAB_POSITION_OPTIONS, TAB_TITLE_MODE_OPTIONS, THEME_PREFERENCE_OPTIONS,
    TextField, blur_decision, drawer_description, dropdown_selected, etc_shells_content,
    page_section, page_title, profile_scheme_selected, shell_dropdown_selected, shell_label,
    shell_picker_options, toggle_write,
};
use gpui::prelude::*;
use gpui::{
    App, Context, Entity, InteractiveElement, Render, SharedString, StatefulInteractiveElement,
    Styled, Window,
};
use gpui_component::ActiveTheme as _;
use gpui_component::accordion::Accordion;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputState};
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::switch::Switch;
use gpui_component::{Disableable, Sizable, h_flex, v_flex};

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
/// lists resolves — pinned against `page_section`'s partition below —
/// and the one key no section hosts (`SettingsDrawerWidth`, the
/// drag handle's writer) resolves to nothing.
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
        | ConfigKey::AutosuggestionsDelayMs
        | ConfigKey::TabBarMinTabs => ControlKind::Text,
        ConfigKey::SettingsDrawerWidth => return None,
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

/// The global-summon snapshot (global-toggle-guidance spec): the
/// environment, the custom map's own-stem toggle chords in their
/// display form, and whether the writer's own marker entry is
/// present (the Global row's revert affordance), loaded once at cold
/// startup. The Global row reads it, the first-run prompt's decision
/// reads it, and the writer module's commits refresh it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SummonSnapshot {
    pub(crate) environment: dropterm::core::global_shortcut::Environment,
    pub(crate) display: Vec<String>,
    /// The writer's marker entry (the `DropTerm`-described toggle
    /// spawn of the running stem) exists, so the Global row offers
    /// the revert verb.
    pub(crate) marker: bool,
}

/// The drawer's shell-owned state (the libcosmic app's own shape):
/// the open flag, the open page, the display-only drafts buffer
/// (core's), and the guard that keeps an echo's programmatic input
/// refresh from counting as typing.
pub(crate) struct SettingsState {
    pub(crate) open: bool,
    pub(crate) page: SettingsPage,
    pub(crate) buffer: SettingsBuffer,
    /// The section cards' accordion (core's model): which cards
    /// stand collapsed. Session state, never a config key — it
    /// rides the drawer across page turns and closes within one
    /// app run, and every app start opens with every card expanded
    /// (the fresh state's default).
    pub(crate) accordion: dropterm::core::settings::AccordionState,
    pub(crate) refreshing: bool,
    /// Fields awaiting a blur-revert (the audit's F1): an invalid
    /// blur'd edit's stored text. The input subscriber has no
    /// `Window`, so the reseed rides the render path's echo pass —
    /// the one place that already reseeds Kit states.
    pub(crate) pending_reverts: Vec<(TextField, String)>,
    /// Bumped on every config reload the shell drains: the drawer's
    /// cue to re-seed its un-drafted Kit states from the new file —
    /// a hand edit mid-open reaches the controls the next frame.
    pub(crate) config_epoch: u64,
    /// The Profiles page's selection (the row the edit form seeds
    /// from), the open transactional draft, and the armed delete row
    /// (`armed_profile_delete_row`'s state machine over presses).
    pub(crate) profile_selection: Option<usize>,
    pub(crate) profile_draft: Option<dropterm::core::settings::ProfileDraft>,
    pub(crate) profile_delete_armed: Option<usize>,
    /// The Keybindings page's search (display-only; the page
    /// re-filters its rows), the armed capture's row id, and the
    /// standing conflict (the arming row and the holder). Owned ids:
    /// a script-command row's id (`script/<name>`) is runtime state.
    pub(crate) keybind_search: String,
    pub(crate) keybind_capture: Option<String>,
    pub(crate) keybind_conflict: Option<(String, String)>,
    /// The global-summon snapshot (global-toggle-guidance spec):
    /// loaded at cold startup, read-only here — the writer module's
    /// commits are the one future refresher.
    pub(crate) summon: SummonSnapshot,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            open: false,
            page: SettingsPage::Appearance,
            buffer: SettingsBuffer::default(),
            accordion: dropterm::core::settings::AccordionState::default(),
            refreshing: false,
            pending_reverts: Vec::new(),
            config_epoch: 0,
            profile_selection: None,
            profile_draft: None,
            profile_delete_armed: None,
            keybind_search: String::new(),
            keybind_capture: None,
            keybind_conflict: None,
            summon: SummonSnapshot::default(),
        }
    }
}

/// The profile form's Kit states, rebuilt whenever the draft's
/// identity changes (a fresh Add/Clone/Edit, or an env-row count
/// change): every field input seeded from the draft, the scheme
/// picker with its leading Inherit row.
struct FormInputs {
    name: Entity<InputState>,
    command: Entity<InputState>,
    cwd: Entity<InputState>,
    font_size: Entity<InputState>,
    scrollback: Entity<InputState>,
    env: Vec<(Entity<InputState>, Entity<InputState>)>,
    scheme: Entity<SelectState<Vec<SharedString>>>,
    _scheme_subscription: gpui::Subscription,
}

/// Which draft the form's inputs were built for — the mode/index
/// discriminator plus the env-row count (a row edit rebuilds); `None`
/// while no draft is open.
type FormIdentity = Option<(String, usize)>;

/// The drawer's Kit halves: the stateful controls, built once per
/// open from the live config, seeded on the file's values (drafts
/// are empty at open — the buffer's reopen-fresh rule).
pub(crate) struct SettingsWindow {
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
    /// The Keybindings page's search box.
    keybind_search: Entity<InputState>,
    /// The profile form's inputs, with the draft identity they were
    /// built for (mode/index hash and env-row count).
    form: (FormIdentity, Option<FormInputs>),
    /// The default-profile picker.
    default_picker: Entity<SelectState<Vec<SharedString>>>,
    _default_subscription: gpui::Subscription,
    _observe: gpui::Subscription,
}

impl SettingsWindow {
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
        let default_options: Vec<SharedString> = settings::default_picker_options(&config.profiles)
            .into_iter()
            .map(SharedString::from)
            .collect();
        let default_selected =
            settings::default_picker_selected(&config.profiles, config.default_profile.as_deref());
        let default_picker = cx.new(|cx| {
            SelectState::new(
                default_options,
                Some(gpui_component::IndexPath::default().row(default_selected)),
                window,
                cx,
            )
        });
        let keybind_search = {
            let target = shell.clone();
            let state = cx.new(|cx| InputState::new(window, cx));
            cx.subscribe(
                &state,
                move |emitter: Entity<InputState>,
                      event: &gpui_component::input::InputEvent,
                      cx| {
                    if matches!(event, gpui_component::input::InputEvent::Change) {
                        let value = emitter.read(cx).value().to_string();
                        target.update(cx, |shell, cx| shell.keybind_search(value, cx));
                    }
                },
            )
            .detach();
            state
        };
        let default_target = shell.clone();
        let default_subscription = cx.subscribe(
            &default_picker,
            move |_emitter: Entity<SelectState<Vec<SharedString>>>,
                  event: &SelectEvent<Vec<SharedString>>,
                  cx| {
                if let SelectEvent::Confirm(Some(label)) = event {
                    // The row resolves against the authoritative
                    // options list, never the popover's snapshot.
                    let options =
                        settings::default_picker_options(&default_target.read(cx).config.profiles);
                    if let Some(row) = options.iter().position(|option| option == label.as_ref()) {
                        default_target.update(cx, |shell, cx| {
                            shell.profile_pick_default(row, cx);
                        });
                    }
                }
            },
        );
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
                keybind_search,
                seen_epoch: 0,
                form: (None, None),
                default_picker,
                _default_subscription: default_subscription,
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
            TextField::TabBarMinTabs,
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
                        // The blur commit (the audit's F1): focus
                        // leaving the field commits a valid edit
                        // through Enter's own path; an invalid edit
                        // parks its revert — the field must never
                        // show a value the app doesn't hold.
                        gpui_component::input::InputEvent::Blur => {
                            let raw = emitter.read(cx).value().to_string();
                            let revert = target.update(cx, |shell, _| {
                                if shell.settings.refreshing {
                                    return None;
                                }
                                let stored = SettingsBuffer::default().text(field, &shell.config);
                                match blur_decision(field, &raw, &stored) {
                                    BlurDecision::Noop => {
                                        shell.settings.buffer.clear_text(field);
                                        None
                                    }
                                    BlurDecision::Commit(write) => {
                                        shell.write_settings(&write);
                                        shell.settings.buffer.clear_text(field);
                                        None
                                    }
                                    BlurDecision::Revert => {
                                        shell.settings.buffer.clear_text(field);
                                        Some(stored)
                                    }
                                }
                            });
                            if let Some(live) = revert {
                                target.update(cx, |shell, _| {
                                    shell.settings.pending_reverts.push((field, live));
                                });
                            }
                        }
                        gpui_component::input::InputEvent::Focus => {}
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
        // The merged scheme vocabulary the scheme-keyed arms resolve
        // against (theme-files F3): the shell's holder at build time.
        let user_schemes = shell.read(cx).user_schemes.clone();
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
            let selected = Some(dropdown_selected(key, config, &user_schemes).unwrap_or(0))
                .map(|row| gpui_component::IndexPath::default().row(row));
            let state = cx.new(|cx| SelectState::new(options, selected, window, cx));
            let target = shell.clone();
            let dropdown_user_schemes = user_schemes.clone();
            cx.subscribe(
                &state,
                move |_emitter: Entity<SelectState<Vec<SharedString>>>,
                      event: &SelectEvent<Vec<SharedString>>,
                      cx| {
                    if let SelectEvent::Confirm(Some(label)) = event {
                        let row = dropdown_row_of(key, label);
                        target.update(cx, |shell, _| {
                            if let Some(write) =
                                settings::dropdown_write(key, row, &dropdown_user_schemes)
                                    .map(|w| w.clamped())
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
                ConfigKey::ColorSchemeDark => settings::PlannedWrite::ColorSchemeDark(name),
                _ => settings::PlannedWrite::ColorSchemeLight(name),
            };
            target.update(cx, |shell, _| shell.write_settings(&write));
        });
        let user_schemes = shell.read(cx).user_schemes.clone();
        pickers::scheme_picker(stored, &user_schemes, &on_pick, window, cx)
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

impl SettingsWindow {
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
                h_flex().justify_end().child(toggle).into_any_element()
            }
            ControlKind::Slider => {
                let Some((_, state)) = self.sliders.iter().find(|(k, _)| *k == key) else {
                    return placeholder_row(key).into_any_element();
                };
                let value = slider_value(key, &self.shell.read(cx).settings.buffer, config);
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(Slider::new(state))
                    .child(
                        h_flex()
                            .justify_end()
                            .text_size(gpui::px(12.))
                            .child(format!("{value:.0}")),
                    )
                    .into_any_element()
            }
            ControlKind::Dropdown => {
                let Some((_, state)) = self.dropdowns.iter().find(|(k, _)| *k == key) else {
                    return placeholder_row(key).into_any_element();
                };
                Select::new(state).w_full().into_any_element()
            }
            ControlKind::Text => {
                let Some(field) = text_field_of(key) else {
                    return placeholder_row(key).into_any_element();
                };
                let Some((_, state)) = self.inputs.iter().find(|(f, _)| *f == field) else {
                    return placeholder_row(key).into_any_element();
                };
                Input::new(state).w_full().into_any_element()
            }
            ControlKind::FontPicker => Select::new(&self.font_picker.state)
                .w_full()
                .into_any_element(),
            ControlKind::SchemePicker => {
                let state = if key == ConfigKey::ColorSchemeDark {
                    &self.scheme_dark.state
                } else {
                    &self.scheme_light.state
                };
                // The menu is wider than the trigger column: the
                // scheme row's strip plus its name don't share the
                // 208 px control (the name clipped away entirely —
                // pickers.rs's geometry test pins the budget).
                Select::new(state)
                    .menu_width(gpui::px(pickers::SCHEME_MENU_WIDTH_PX))
                    .w_full()
                    .into_any_element()
            }
            ControlKind::ShellPicker => v_flex()
                .w_full()
                .gap_1()
                .child(Select::new(&self.shell_control.picker).w_full())
                .child(Input::new(&self.shell_control.input).w_full())
                .into_any_element(),
        };
        let _ = window;
        row_card(label, description, control, cx).into_any_element()
    }

    /// The open page: the section cards (the Profiles and
    /// Keybindings pages carry their own bodies — their increments).
    fn page_element(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let config = self.shell.read(cx).config.clone();
        let page = self.shell.read(cx).settings.page;
        let mut content = v_flex()
            .id("settings-page")
            .gap_4()
            .p_4()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll();
        match page {
            SettingsPage::Profiles => {
                content = content.child(self.profiles_page(window, cx));
            }
            SettingsPage::Keybindings => {
                content = content.child(self.keybindings_page(cx));
            }
            _ => {
                for (index, section) in page_section(page).into_iter().enumerate() {
                    content = content.child(self.section_card(section, index, &config, window, cx));
                }
            }
        }
        content.into_any_element()
    }

    /// One section card as an accordion item (the Kit's accordion,
    /// rendered one card per section so the pages keep their flat
    /// separate-cards look): the header is the trigger — title left,
    /// disclosure chevron right — and the panel below holds the
    /// card's control rows. The open state is controlled from the
    /// shell's core `AccordionState` (core's model; this view is
    /// its rendering face), so a fold survives page turns and the
    /// drawer's own reopen. Collapsed panels unmount once the
    /// reveal settles — a folded card costs no rows, and the Kit
    /// states behind its controls are the drawer's own entities,
    /// untouched by the unmount.
    fn section_card(
        &self,
        section: dropterm::core::settings::Section,
        index: usize,
        config: &DropTermConfig,
        window: &mut Window,
        cx: &App,
    ) -> gpui::AnyElement {
        let open = self.shell.read(cx).settings.accordion.is_expanded(section);
        let target = self.shell.clone();
        let mut rows = v_flex().gap_3().min_w_0();
        for key in section.keys() {
            rows = rows.child(self.control_row(*key, config, window, cx));
        }
        // The auto-height wrapper: the Kit accordion's own root
        // pins `size_full`, and a percentage height against an
        // auto-height parent resolves as content height — without
        // it every card would stretch to the page's viewport.
        v_flex()
            .min_w_0()
            .child(
                Accordion::new(("settings-section", index))
                    // The bordered treatment is the grouping
                    // indicator itself: one outlined, rounded card
                    // whose header — semibold title left, rotating
                    // disclosure chevron right (the Kit's own, now
                    // painted through the boot-registered icon
                    // bundle) — reads as a heading above the rows it
                    // folds.
                    .bordered(true)
                    .on_toggle_click(move |open_indices, _, cx| {
                        target.update(cx, |shell, _| {
                            shell
                                .settings
                                .accordion
                                .set_expanded(section, open_indices.contains(&0));
                        });
                    })
                    .item(|item| {
                        // The scheme-derived themes set no
                        // accordion.background, so the Kit's default
                        // would paint white/near-black over the
                        // drawer — the popover surface is the card.
                        item.open(open)
                            .bg(cx.theme().popover)
                            // The hover affordance is an outline ring,
                            // never a fill: the theme generator rides
                            // BrightBlack for both secondary.background
                            // and muted.foreground, so the Kit's dim
                            // chevron would paint dim-on-dim and
                            // vanish inside any filled hover. The ring
                            // is pre-laid (border_1, transparent) and
                            // only its color swaps on hover — no
                            // layout shift at the transition.
                            .hover(|hover| {
                                hover.rounded_sm().border_color(cx.theme().muted_foreground)
                            })
                            .title_style(
                                gpui::StyleRefinement::default()
                                    .px_4()
                                    .py_3()
                                    .border_1()
                                    .rounded_sm()
                                    .border_color(gpui::transparent_black()),
                            )
                            // The pt keeps the ring (or the resting
                            // header row) from sitting flush against
                            // the first control row — the hover box
                            // carries the header's own py_3, so
                            // without a content gap the highlight's
                            // bottom edge lands on the row.
                            .content_style(gpui::StyleRefinement::default().pt_2().px_4().pb_3())
                            .title(
                                gpui::div()
                                    .min_w_0()
                                    .truncate()
                                    // One step above the 13px row labels:
                                    // the card's heading, below the
                                    // header bar's 15px "Settings".
                                    .text_size(gpui::px(14.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(cx.theme().popover_foreground)
                                    .child(section.title()),
                            )
                            .child(rows)
                    }),
            )
            .into_any_element()
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
        ConfigKey::TabBarMinTabs => TextField::TabBarMinTabs,
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

/// The control column's width: every row's control — switch, slider,
/// dropdown, picker, text — sits in one right-hand column this wide,
/// so the rows share one geometry (aligned left labels, aligned
/// right controls) whatever the control's own intrinsic width.
const CONTROL_COLUMN_PX: f32 = 208.;
/// The close glyph's size (user-ruled 2026-11-20 at a 16x16 floor,
/// raised to 24px 2026-11-22): every icon glyph paints at 24px,
/// pinned here because the ✕ would otherwise inherit whatever text
/// size an ancestor later sets.
const CLOSE_GLYPH_FONT_SIZE: f32 = 24.;

/// One row of a section card, the settings-item discipline the
/// libcosmic drawer drew and the Kit's own `setting` module still
/// draws: the label and its description in a wrapping left column,
/// the control in the fixed right column — vertically centered,
/// mutually truncating, never overflowing the card.
fn row_card(
    label: &str,
    description: String,
    control: gpui::AnyElement,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .justify_between()
        .gap_4()
        .min_w_0()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    gpui::div()
                        .min_w_0()
                        .truncate()
                        .text_size(gpui::px(13.))
                        .child(label.to_string()),
                )
                .child(
                    gpui::div()
                        .min_w_0()
                        .text_size(gpui::px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                ),
        )
        .child(
            h_flex()
                .flex_none()
                .w(gpui::px(CONTROL_COLUMN_PX))
                .justify_end()
                .child(control),
        )
}

/// One small affordance button carrying a shell-routed verb — the
/// Kit's Button in its quiet secondary style, so hover, focus, and
/// sizing come from the component instead of a hand-rolled div.
fn small_button(
    id: impl Into<gpui::ElementId>,
    label: &str,
    on_press: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .label(label)
        .small()
        .secondary()
        .on_click(move |_: &gpui::ClickEvent, window, cx| on_press(window, cx))
}

/// `small_button` in the primary style — a page's one standard verb.
fn primary_button(
    id: impl Into<gpui::ElementId>,
    label: &str,
    on_press: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .label(label)
        .small()
        .primary()
        .on_click(move |_: &gpui::ClickEvent, window, cx| on_press(window, cx))
}

/// `small_button` in the danger style — an armed destructive
/// confirm.
fn danger_button(
    id: impl Into<gpui::ElementId>,
    label: &str,
    on_press: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .label(label)
        .small()
        .danger()
        .on_click(move |_: &gpui::ClickEvent, window, cx| on_press(window, cx))
}

/// One labeled field row with its control and, when a Save rejected
/// it, the inline error under the control: the label in a fixed
/// left column, the control filling the rest, the error indented to
/// the control's column.
fn field_row(
    label: &str,
    control: impl IntoElement,
    error: Option<String>,
    cx: &App,
) -> impl IntoElement {
    let mut row = v_flex().gap_1().min_w_0().child(
        h_flex()
            .gap_3()
            .child(
                gpui::div()
                    .w(gpui::px(112.))
                    .flex_none()
                    .text_size(gpui::px(13.))
                    .child(label.to_string()),
            )
            .child(h_flex().flex_1().min_w_0().child(control)),
    );
    if let Some(error) = error {
        row = row.child(
            h_flex()
                .gap_3()
                .child(gpui::div().w(gpui::px(112.)).flex_none())
                .child(
                    gpui::div()
                        .flex_1()
                        .min_w_0()
                        .text_size(gpui::px(12.))
                        .text_color(cx.theme().danger)
                        .child(error),
                ),
        );
    }
    row
}

fn placeholder_row(key: ConfigKey) -> impl IntoElement {
    h_flex().child(key.label())
}

/// One profile row's action cluster, right of the name: Clone, the
/// delete pair (Arm → Confirm/Keep, the confirm in the danger
/// style), and the reorder pair — one Vec so [`SettingsWindow::
/// profile_row`] stays a layout function.
fn profile_action_buttons(
    target: &Entity<Shell>,
    index: usize,
    armed: bool,
) -> Vec<gpui_component::button::Button> {
    let mut buttons = Vec::new();
    let clone = target.clone();
    buttons.push(small_button(
        ("profile-clone", index),
        "Clone",
        move |_, cx| {
            clone.update(cx, |shell, cx| shell.profile_clone(index, cx));
        },
    ));
    if armed {
        let confirm = target.clone();
        let keep = target.clone();
        buttons.push(danger_button(
            ("profile-delete-confirm", index),
            "Confirm delete",
            move |_, cx| {
                confirm.update(cx, |shell, cx| shell.profile_delete_confirm(index, cx));
            },
        ));
        buttons.push(small_button(
            ("profile-delete-keep", index),
            "Keep",
            move |_, cx| {
                keep.update(cx, Shell::profile_delete_keep);
            },
        ));
    } else {
        let arm = target.clone();
        buttons.push(small_button(
            ("profile-delete-arm", index),
            "Delete",
            move |_, cx| {
                arm.update(cx, |shell, cx| shell.profile_delete_arm(index, cx));
            },
        ));
    }
    let up = target.clone();
    let down = target.clone();
    buttons.push(small_button(
        ("profile-reorder-up", index),
        "Up",
        move |_, cx| {
            up.update(cx, |shell, cx| {
                shell.profile_reorder(index, settings::ReorderDirection::Up, cx);
            });
        },
    ));
    buttons.push(small_button(
        ("profile-reorder-down", index),
        "Down",
        move |_, cx| {
            down.update(cx, |shell, cx| {
                shell.profile_reorder(index, settings::ReorderDirection::Down, cx);
            });
        },
    ));
    buttons
}

impl SettingsWindow {
    /// The Profiles page: the default-profile picker and Add button
    /// (disabled while the draft lock holds), one row per profile
    /// (name, prompt line, the edit-select/clone/delete/reorder
    /// affordances), and the open draft's form below — every gesture
    /// landing in the shell's methods over core's model.
    fn profiles_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let (locked, profiles, selection, armed, draft) = {
            let shell = self.shell.read(cx);
            (
                shell.settings.profile_draft.is_some(),
                shell.config.profiles.clone(),
                shell.settings.profile_selection,
                shell.settings.profile_delete_armed,
                shell.settings.profile_draft.clone(),
            )
        };

        let mut page = v_flex().gap_3().min_w_0();
        let add_target = self.shell.clone();
        page = page.child(
            h_flex()
                .gap_3()
                .child(Select::new(&self.default_picker).flex_1().min_w_0())
                .child(
                    primary_button("profile-add", "Add profile", move |_, cx| {
                        add_target.update(cx, Shell::profile_add);
                    })
                    .disabled(locked),
                ),
        );
        for (index, profile) in profiles.iter().enumerate() {
            page = page.child(self.profile_row(
                index,
                profile,
                selection == Some(index),
                armed == Some(index),
                locked,
                cx,
            ));
        }
        if let Some(draft) = draft {
            self.ensure_form(&draft, window, cx);
            let form = self.form_inputs_element(&draft, cx);
            page = page.child(form);
        }
        page.into_any_element()
    }

    /// One profile row: the name and prompt line in a filling,
    /// truncating column, the select (a press opens the edit draft),
    /// and the action cluster pinned to the right edge.
    fn profile_row(
        &self,
        index: usize,
        profile: &Profile,
        selected: bool,
        armed: bool,
        locked: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let target = self.shell.clone();
        let mut row = h_flex()
            .gap_2()
            .px_2()
            .py_2()
            .rounded_sm()
            .min_w_0()
            .bg(cx.theme().popover)
            .when(selected, |el| el.border_1().border_color(cx.theme().accent))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        gpui::div()
                            .min_w_0()
                            .truncate()
                            .text_size(gpui::px(13.))
                            .child(profile.name.clone()),
                    )
                    .child(
                        gpui::div()
                            .min_w_0()
                            .truncate()
                            .text_size(gpui::px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child(settings::shell_label(&profile.command)),
                    ),
            )
            .when(!locked, |el| {
                let t = target.clone();
                el.on_mouse_down(gpui::MouseButton::Left, move |_, _window, cx| {
                    t.update(cx, |shell, cx| shell.profile_select(index, cx));
                })
            });
        if !locked {
            for button in profile_action_buttons(&target, index, armed) {
                row = row.child(button);
            }
        }
        row
    }

    /// Build the form's Kit inputs when the draft identity changed.
    fn ensure_form(
        &mut self,
        draft: &settings::ProfileDraft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use settings::ProfileDraftMode;
        let identity = match &draft.mode {
            ProfileDraftMode::Add => "add".to_string(),
            ProfileDraftMode::Clone { .. } => "clone".to_string(),
            ProfileDraftMode::Edit { index } => format!("edit-{index}"),
        };
        let wanted = Some((identity, draft.env_rows.len()));
        if self.form.0 == wanted && self.form.1.is_some() {
            return;
        }
        let shell = self.shell.clone();
        let input = |seed: &str, field: ProfileField, window: &mut Window, cx: &mut App| {
            let state = cx.new(|cx| InputState::new(window, cx).default_value(seed.to_string()));
            let target = shell.clone();
            cx.subscribe(
                &state,
                move |emitter: Entity<InputState>,
                      event: &gpui_component::input::InputEvent,
                      cx| {
                    if matches!(
                        event,
                        gpui_component::input::InputEvent::Change
                            | gpui_component::input::InputEvent::PressEnter { .. }
                    ) {
                        let value = emitter.read(cx).value().to_string();
                        target.update(cx, |shell, cx| {
                            shell.profile_draft_field(field, value, cx);
                        });
                    }
                },
            )
            .detach();
            state
        };
        let mut env = Vec::new();
        for row in &draft.env_rows {
            let key = input(&row.key, ProfileField::EnvKey(env.len()), window, cx);
            let value = input(&row.value, ProfileField::EnvValue(env.len()), window, cx);
            env.push((key, value));
        }
        // The scheme select's vocabulary is the merged registry
        // (theme-files F3): Inherit, then the builtins, then the
        // loaded user schemes — the shell's holder at form-build time.
        let scheme_user_schemes = shell.read(cx).user_schemes.clone();
        let scheme_options: Vec<SharedString> = once("Inherit".to_string())
            .chain(dropterm::core::theme_source::merged_scheme_names(
                &scheme_user_schemes,
            ))
            .map(SharedString::from)
            .collect();
        let scheme_selected = gpui_component::IndexPath::default().row(profile_scheme_selected(
            draft.scheme.as_deref(),
            &scheme_user_schemes,
        ));
        let scheme =
            cx.new(|cx| SelectState::new(scheme_options, Some(scheme_selected), window, cx));
        let scheme_target = shell.clone();
        let scheme_subscription = cx.subscribe(
            &scheme,
            move |_this: &mut SettingsWindow,
                  _emitter: Entity<SelectState<Vec<SharedString>>>,
                  event: &SelectEvent<Vec<SharedString>>,
                  cx: &mut Context<Self>| {
                if let SelectEvent::Confirm(Some(label)) = event {
                    // Row 0 is Inherit; the scheme rows offset by
                    // one — the model's own row space, resolved
                    // against the merged registry (never the
                    // popover's snapshot).
                    let names =
                        dropterm::core::theme_source::merged_scheme_names(&scheme_user_schemes);
                    let row = if label.as_ref() == "Inherit" {
                        Some(0)
                    } else {
                        names
                            .iter()
                            .position(|name| *name == label.as_ref())
                            .map(|row| row + 1)
                    };
                    if let Some(row) = row {
                        scheme_target.update(cx, |shell, cx| {
                            shell.profile_draft_scheme(row, cx);
                        });
                    }
                }
            },
        );
        self.form = (
            wanted,
            Some(FormInputs {
                name: input(&draft.name, ProfileField::Name, window, cx),
                command: input(&draft.command, ProfileField::Command, window, cx),
                cwd: input(&draft.cwd, ProfileField::Cwd, window, cx),
                font_size: input(&draft.font_size, ProfileField::FontSize, window, cx),
                scrollback: input(&draft.scrollback, ProfileField::Scrollback, window, cx),
                env,
                scheme,
                _scheme_subscription: scheme_subscription,
            }),
        );
    }

    /// Render the form from the draft (the inputs are the built Kit
    /// states; the errors render under their fields).
    fn form_inputs_element(&self, draft: &settings::ProfileDraft, cx: &App) -> gpui::AnyElement {
        let Some(form) = &self.form.1 else {
            return v_flex().into_any_element();
        };
        let error_for = |field: ProfileField| {
            draft
                .errors
                .iter()
                .find(|(at, _)| *at == field)
                .map(|(_, error)| error.message().to_string())
        };
        let mut card = v_flex()
            .gap_3()
            .p_4()
            .rounded_lg()
            .min_w_0()
            .bg(cx.theme().popover);
        for (label, state, field) in [
            ("Name", &form.name, ProfileField::Name),
            ("Command", &form.command, ProfileField::Command),
            ("Working directory", &form.cwd, ProfileField::Cwd),
            ("Font size", &form.font_size, ProfileField::FontSize),
            ("Scrollback", &form.scrollback, ProfileField::Scrollback),
        ] {
            card = card.child(field_row(
                label,
                Input::new(state).w_full(),
                error_for(field),
                cx,
            ));
        }
        card = card.child(field_row(
            "Color scheme",
            Select::new(&form.scheme).w_full(),
            error_for(ProfileField::Scheme),
            cx,
        ));
        card = card.children(self.env_rows_element(form, &error_for, cx));
        let target = self.shell.clone();
        card = card.child(small_button(
            "profile-env-add",
            "+ env row",
            move |_, cx| {
                target.update(cx, Shell::profile_draft_env_add);
            },
        ));
        let cancel = self.shell.clone();
        let save = self.shell.clone();
        card = card.child(
            h_flex()
                .gap_2()
                .justify_end()
                .child(small_button("profile-cancel", "Cancel", move |_, cx| {
                    cancel.update(cx, Shell::profile_draft_cancel);
                }))
                .child(primary_button("profile-save", "Save", move |_, cx| {
                    save.update(cx, Shell::profile_draft_save);
                })),
        );
        card.into_any_element()
    }

    /// The env rows editor: one key/value pair per row with the
    /// remove affordance, the row errors under their keys.
    fn env_rows_element(
        &self,
        form: &FormInputs,
        error_for: &impl Fn(ProfileField) -> Option<String>,
        cx: &App,
    ) -> Vec<gpui::AnyElement> {
        form.env
            .iter()
            .enumerate()
            .map(|(index, (key, value))| {
                let target = self.shell.clone();
                let mut env = v_flex().gap_1().min_w_0().child(
                    h_flex()
                        .gap_2()
                        .min_w_0()
                        .child(Input::new(key).flex_1().min_w_0())
                        .child(Input::new(value).flex_1().min_w_0())
                        .child(small_button(
                            ("profile-env-remove", index),
                            "−",
                            move |_, cx| {
                                target.update(cx, |shell, cx| {
                                    shell.profile_draft_env_remove(index, cx);
                                });
                            },
                        )),
                );
                if let Some(error) = error_for(ProfileField::EnvKey(index)) {
                    env = env.child(
                        gpui::div()
                            .text_size(gpui::px(12.))
                            .text_color(cx.theme().danger)
                            .child(error),
                    );
                }
                env.into_any_element()
            })
            .collect()
    }

    /// The Keybindings page: the search box, the grouped rows (a
    /// row's binding text is the arm affordance; the revert rides
    /// beside it while a custom entry exists), the armed hint, and
    /// the conflict's inline error — core's registry rendering,
    /// core's verdicts writing.
    fn keybindings_page(&self, cx: &App) -> gpui::AnyElement {
        let shell = self.shell.read(cx);
        let custom = shell.config.custom_key_binds.clone();
        let search = shell.settings.keybind_search.clone();
        let armed = shell.settings.keybind_capture.clone();
        let conflict = shell.settings.keybind_conflict.clone();
        let mut page = v_flex().gap_3().min_w_0();
        page = page.child(Input::new(&self.keybind_search).w_full());
        if armed.is_some() {
            let target = self.shell.clone();
            page = page.child(
                h_flex()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(cx.theme().secondary)
                    .text_size(gpui::px(13.))
                    .child("Press the chord to bind — Escape cancels")
                    .child(small_button(
                        "keybind-capture-cancel",
                        "Cancel",
                        move |window, cx| {
                            target.update(cx, |shell, cx| shell.keybind_capture_end(window, cx));
                        },
                    )),
            );
        }
        if let Some((id, holder)) = conflict {
            let holder_label = dropterm::core::keybinds::binding_rows()
                .iter()
                .find(|row| row.id == holder)
                .map_or_else(
                    || {
                        dropterm::core::keybinds::command_name_from_id(&holder)
                            .unwrap_or(&holder)
                            .to_string()
                    },
                    |row| row.label.to_string(),
                );
            page = page.child(
                gpui::div()
                    .text_size(gpui::px(13.))
                    .text_color(cx.theme().danger)
                    .child(format!("That chord is held by {holder_label} ({id})")),
            );
        }
        // The Global group (global-toggle-guidance spec) above the
        // in-app groups, answering the page's search like every row.
        if search.is_empty() || "toggle the overlay".contains(&search.to_lowercase()) {
            page = page.child(self.summon_group(cx));
        }
        let mut row_ix = 0;
        for row in dropterm::core::keybinds::binding_rows() {
            if !dropterm::core::keybinds::row_matches_search(row, &search) {
                continue;
            }
            page = page.child(self.keybind_row(
                row,
                row_ix,
                armed.as_deref() == Some(row.id),
                custom.contains_key(row.id),
                &custom,
                cx,
            ));
            row_ix += 1;
        }
        // The runtime section (script-commands spec): one row per live
        // command, beside the builtins — the capture flow serves it
        // exactly as the builtin rows (a command ships unbound, so
        // only a custom entry can hold its chord). The search matches
        // a command's name or description, the builtin rows' own rule.
        let commands = shell.script_commands.clone();
        let query = search.to_lowercase();
        let matches = |command: &dropterm::core::keybinds::CommandRow| {
            query.is_empty()
                || command.name.to_lowercase().contains(&query)
                || command.description.to_lowercase().contains(&query)
        };
        if commands.iter().any(&matches) {
            page = page.child(
                gpui::div()
                    .px_2()
                    .pt_2()
                    .text_size(gpui::px(12.))
                    .text_color(cx.theme().muted_foreground)
                    .child("Script commands"),
            );
            for command in &commands {
                if !matches(command) {
                    continue;
                }
                page = page.child(self.command_row(
                    command,
                    row_ix,
                    armed.as_deref() == Some(command.id().as_str()),
                    &custom,
                    cx,
                ));
                row_ix += 1;
            }
        }
        page.into_any_element()
    }

    /// One script-command row (script-commands spec): the command's
    /// slug and its description stacked in the label column, the
    /// binding chip beside it — the capture and revert affordances
    /// identical to [`Self::keybind_row`]'s, over the owned
    /// `script/<name>` id.
    fn command_row(
        &self,
        command: &dropterm::core::keybinds::CommandRow,
        row_ix: usize,
        is_armed: bool,
        custom: &std::collections::BTreeMap<String, dropterm::core::keybinds::Binding>,
        cx: &App,
    ) -> impl IntoElement {
        let target = self.shell.clone();
        let id = command.id();
        let capture_id = id.clone();
        let mut element = h_flex()
            .gap_2()
            .px_2()
            .py_2()
            .rounded_sm()
            .min_w_0()
            .bg(cx.theme().popover)
            .child(
                gpui::div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        gpui::div()
                            .truncate()
                            .text_size(gpui::px(13.))
                            .child(command.name.clone()),
                    )
                    .child(
                        gpui::div()
                            .truncate()
                            .text_size(gpui::px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child(command.description.clone()),
                    ),
            )
            .child(
                gpui::div()
                    .id(("keybind-bind", row_ix))
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(cx.theme().accent)
                    .text_color(cx.theme().accent_foreground)
                    .text_size(gpui::px(12.))
                    .child(dropterm::core::keybinds::effective_binding_display(
                        &id, custom,
                    ))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        move |_, window: &mut gpui::Window, cx: &mut gpui::App| {
                            target.update(cx, |shell, cx| {
                                shell.keybind_capture_start(capture_id.clone(), window, cx);
                            });
                        },
                    ),
            );
        if custom.contains_key(id.as_str()) {
            let target = self.shell.clone();
            let revert_id = id.clone();
            element = element.child(small_button(
                ("keybind-revert", row_ix),
                "Revert",
                move |_, cx| {
                    target.update(cx, |shell, cx| shell.keybind_revert(&revert_id, cx));
                },
            ));
        }
        if is_armed {
            element = element.child(
                gpui::div()
                    .text_size(gpui::px(12.))
                    .text_color(cx.theme().muted_foreground)
                    .child("capturing…"),
            );
        }
        element
    }

    /// The Global group: the summon key's row — the custom map's
    /// own-stem toggle chords read-only (`None` while unbound), the
    /// environment's walk beneath, and on COSMIC the keyboard page's
    /// deep link. The writer module's capture replaces the read-only
    /// chip with the arm affordance (global-toggle-cosmic-writer).
    fn summon_group(&self, cx: &App) -> gpui::AnyElement {
        let summon = &self.shell.read(cx).settings.summon;
        let binding_text = if summon.display.is_empty() {
            "None".to_string()
        } else {
            summon.display.join(" / ")
        };
        let mut group = v_flex().gap_1().min_w_0();
        group = group.child(
            gpui::div()
                .text_size(gpui::px(12.))
                .text_color(cx.theme().muted_foreground)
                .child("Global"),
        );
        group = group.child(
            h_flex()
                .gap_2()
                .px_2()
                .py_2()
                .rounded_sm()
                .min_w_0()
                .bg(cx.theme().popover)
                .child(
                    gpui::div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(gpui::px(13.))
                        .child("Toggle the overlay"),
                )
                .child(
                    gpui::div()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(cx.theme().accent)
                        .text_color(cx.theme().accent_foreground)
                        .text_size(gpui::px(12.))
                        .child(binding_text),
                ),
        );
        for step in dropterm::core::global_shortcut::summon_walk(summon.environment) {
            group = group.child(
                gpui::div()
                    .text_size(gpui::px(12.))
                    .text_color(cx.theme().muted_foreground)
                    .child(*step),
            );
        }
        if summon.environment == dropterm::core::global_shortcut::Environment::Cosmic {
            group = group.child(small_button(
                "summon-open-keyboard",
                "Open keyboard settings",
                |_, _| {
                    // The COSMIC deep link: the keyboard page the walk
                    // describes. A detached spawn — the settings app
                    // outlives DropTerm as a matter of course, and a
                    // failure is a logged line, never a surface.
                    if let Err(err) = std::process::Command::new("cosmic-settings")
                        .arg("keyboard")
                        .spawn()
                    {
                        eprintln!("dropterm: cosmic-settings spawn failed: {err}");
                    }
                },
            ));
        }
        if summon.marker {
            group = group.child(self.summon_revert_button());
        }
        // The re-trigger (summon-onboarding spec): the same step the
        // cold boot opens, from the settings panel. Closing from this
        // origin persists nothing. The dropdown leaves first (the
        // onboarding-over-drawer shape): this surface's Overlay layer
        // always stacks above a toplevel, so the flow's window only
        // stands above everything once the drawer is down.
        let shell = self.shell.clone();
        group = group.child(small_button(
            "summon-open-onboarding",
            "Set up the summon key…",
            move |_, cx| {
                shell.update(cx, |shell, _| shell.dismiss_for_summon_setup());
                crate::onboarding::open_onboarding(
                    cx,
                    shell.clone(),
                    crate::onboarding::Origin::Settings,
                );
            },
        ));
        group.into_any_element()
    }

    /// The revert verb (global-toggle-cosmic-writer spec): offered
    /// only while the writer's own marker entry exists; it removes
    /// exactly that entry through the seam and refreshes this row's
    /// read in the same pass.
    fn summon_revert_button(&self) -> gpui::AnyElement {
        let shell = self.shell.clone();
        small_button(
            "summon-remove-marker",
            "Remove the key DropTerm set",
            move |_, cx| {
                let Some(stem) = dropterm::core::global_shortcut::own_exe_stem() else {
                    return;
                };
                let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
                    return;
                };
                let path = dropterm::core::global_shortcut::custom_shortcuts_path(&home);
                match dropterm::core::global_shortcut::write_custom(&path, |custom| {
                    dropterm::core::global_shortcut::revert_plan(custom, &stem)
                }) {
                    dropterm::core::global_shortcut::WriteOutcome::Written(_) => {
                        let snapshot = crate::app_shell::load_summon_snapshot();
                        shell.update(cx, |shell, _| shell.settings.summon = snapshot);
                    }
                    outcome => {
                        eprintln!("dropterm: summon revert refused or failed: {outcome:?}");
                    }
                }
            },
        )
        .into_any_element()
    }

    /// One keybindings row: the action's label in a filling,
    /// truncating column, the binding chip (the arm affordance) at
    /// the right edge, the revert beside it while a custom entry
    /// exists, the armed hint while it captures.
    fn keybind_row(
        &self,
        row: &dropterm::core::keybinds::BindingRow,
        row_ix: usize,
        is_armed: bool,
        customized: bool,
        custom: &std::collections::BTreeMap<String, dropterm::core::keybinds::Binding>,
        cx: &App,
    ) -> impl IntoElement {
        let target = self.shell.clone();
        let id = row.id;
        let mut element = h_flex()
            .gap_2()
            .px_2()
            .py_2()
            .rounded_sm()
            .min_w_0()
            .bg(cx.theme().popover)
            .child(
                gpui::div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(gpui::px(13.))
                    .child(row.label),
            )
            .child(
                gpui::div()
                    .id(("keybind-bind", row_ix))
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(cx.theme().accent)
                    .text_color(cx.theme().accent_foreground)
                    .text_size(gpui::px(12.))
                    .child(dropterm::core::keybinds::effective_binding_display(
                        row.id, custom,
                    ))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        move |_, window: &mut gpui::Window, cx: &mut gpui::App| {
                            target.update(cx, |shell, cx| {
                                shell.keybind_capture_start(id.to_string(), window, cx);
                            });
                        },
                    ),
            );
        if customized {
            let target = self.shell.clone();
            let id = row.id;
            element = element.child(small_button(
                ("keybind-revert", row_ix),
                "Revert",
                move |_, cx| {
                    target.update(cx, |shell, cx| shell.keybind_revert(id, cx));
                },
            ));
        }
        if is_armed {
            element = element.child(
                gpui::div()
                    .text_size(gpui::px(12.))
                    .text_color(cx.theme().muted_foreground)
                    .child("capturing…"),
            );
        }
        element
    }

    /// The echo-refresh pass: after a config reload (a write's echo
    /// or a hand edit), re-seed every Kit state that has no draft
    /// buffered for it — the drawer's un-drafted controls always
    /// read the file, exactly the per-frame re-render the libcosmic
    /// drawer's stateless controls get by construction.
    fn refresh_from_echo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The blur-revert pass (the audit's F1): the subscriber has
        // no `Window`, so an invalid blur'd edit's stored text rides
        // this pass — the same guarded reseed the echo uses.
        let reverts = self.shell.update(cx, |shell, _| {
            std::mem::take(&mut shell.settings.pending_reverts)
        });
        if !reverts.is_empty() {
            self.shell
                .update(cx, |shell, _| shell.settings.refreshing = true);
            for (field, live) in &reverts {
                if let Some((_, state)) = self.inputs.iter().find(|(f, _)| f == field) {
                    state.update(cx, |state, cx| state.set_value(live.clone(), window, cx));
                }
            }
            self.shell
                .update(cx, |shell, _| shell.settings.refreshing = false);
        }
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

impl Render for SettingsWindow {
    #[expect(
        clippy::too_many_lines,
        reason = "the window's whole face — header, strip, and page — in one render pass"
    )]
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_from_echo(window, cx);
        let shell = self.shell.clone();
        let page = self.shell.read(cx).settings.page;
        // The window's header: the title at the left, the explicit
        // close's ✕ at the right — a user close made visible. The
        // route is the window's one close routine: the cause set in
        // one short shell update, `remove_window` at App level (the
        // observer then updates the shell with no borrow on the
        // stack).
        let header = {
            let target = shell.clone();
            h_flex()
                .w_full()
                .flex_none()
                .justify_between()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    gpui::div()
                        .text_size(gpui::px(15.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("Settings"),
                )
                .child(
                    gpui::div()
                        .id("settings-close")
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .text_color(cx.theme().muted_foreground)
                        .text_size(gpui::px(CLOSE_GLYPH_FONT_SIZE))
                        .child("✕")
                        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                            target.update(cx, |shell, _| {
                                shell.settings_close_cause =
                                    Some(dropterm::core::settings::CloseCause::User);
                            });
                            window.remove_window();
                        }),
                )
        };
        // The tab strip as an even-split segmented row: each tab
        // takes an equal share of the pane's width and truncates
        // inside it, so the strip always fits the panel — never a
        // ragged left-packed row that overruns a narrow drawer.
        let mut strip = h_flex()
            .flex_none()
            .gap_1()
            .px_2()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border);
        for (index, listed) in SettingsPage::ALL.into_iter().enumerate() {
            let selected = listed == page;
            let target = shell.clone();
            strip = strip.child(
                gpui::div()
                    .id(("settings-page", index))
                    .flex_1()
                    .min_w_0()
                    .h_7()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .text_size(gpui::px(13.))
                    .when(selected, |el| {
                        el.bg(cx.theme().accent)
                            .text_color(cx.theme().accent_foreground)
                    })
                    .when(!selected, |el| el.text_color(cx.theme().muted_foreground))
                    .child(
                        gpui::div()
                            .min_w_0()
                            .max_w_full()
                            .truncate()
                            .child(page_title(listed)),
                    )
                    .on_mouse_down(gpui::MouseButton::Left, move |_, _window, cx| {
                        target.update(cx, |shell, cx| {
                            shell.settings.page = listed;
                            cx.notify();
                        });
                    }),
            );
        }
        let armed = self.shell.read(cx).settings.keybind_capture.is_some();
        let target = self.shell.clone();
        v_flex()
            .id("settings-window")
            .key_context("settings-window")
            .track_focus(&self.shell.read(cx).settings_capture_focus)
            .on_action({
                let target = self.shell.clone();
                move |_: &crate::app_shell::DismissSettings,
                      window: &mut gpui::Window,
                      cx: &mut gpui::App| {
                    // A user close: the cause set in one short shell
                    // update, the removal at App level — the
                    // observer then updates the shell with no borrow
                    // on the stack (SW3's rule).
                    target.update(cx, |shell, _| {
                        shell.settings_close_cause =
                            Some(dropterm::core::settings::CloseCause::User);
                    });
                    window.remove_window();
                }
            })
            .on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
                target.update(cx, |shell, cx| {
                    shell.keybind_capture_key(&event.keystroke, window, cx);
                });
            })
            .when(armed, gpui::Styled::border_1)
            .size_full()
            .min_w_0()
            .child(header)
            .child(strip)
            .child(self.page_element(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The close glyph paints at the icon size (user-ruled
    /// 2026-11-20 at a 16x16 floor, raised to 24x24 2026-11-22):
    /// 24x24 or larger, never inherited smaller. The pin is
    /// compile-time: a size regression fails the build, not a run.
    #[test]
    fn the_close_glyph_paints_at_the_icon_floor() {
        const { assert!(CLOSE_GLYPH_FONT_SIZE >= 24.0) }
    }

    /// Every section-hosted key resolves to a control, and the one
    /// key no section hosts resolves to none — the rendering face of
    /// the ported control map, pinned against the section partition.
    #[test]
    fn every_section_key_resolves_a_control_and_no_others_do() {
        for page in SettingsPage::ALL {
            if let Some(section) = page_section(page) {
                for key in section.keys() {
                    assert!(
                        control_kind(*key).is_some(),
                        "{key:?} (section {section:?}) renders no control"
                    );
                }
            }
        }
        assert_eq!(control_kind(ConfigKey::SettingsDrawerWidth), None);
    }

    /// The drawer's fresh state opens every card — the shell-owned
    /// accordion default, wired to core's all-expanded default. The
    /// model's own behavior is core's tests; this pin is the wiring.
    #[test]
    fn the_fresh_drawer_state_expands_every_section_card() {
        let state = SettingsState::default();
        for section in dropterm::core::settings::Section::ALL {
            assert!(
                state.accordion.is_expanded(section),
                "{section:?} starts collapsed"
            );
        }
    }

    /// The dispatch's classes spot-pinned: the pickers and the shell
    /// control, the four sliders, the eight dropdowns, the seven texts,
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
            ConfigKey::TabBarMinTabs,
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
