// SPDX-License-Identifier: GPL-3.0-only

use cosmic::iced::{Background, Color, Length};
use cosmic::{Element, theme, widget};

use super::scheme_picker::{profile_scheme_control, scheme_chip, scheme_picker_control};
use super::{
    ConfigKey, EDGE_OPTIONS, FONT_SIZE_MAX, FONT_SIZE_MIN, H_ALIGN_OPTIONS, KEYBOARD_MODE_OPTIONS,
    LAYER_OPTIONS, NEW_TAB_CWD_OPTIONS, ProfileDraft, ProfileDraftMode, ProfileField,
    ReorderDirection, SchemePickerId, Section, SettingsBuffer, SettingsTab, TAB_POSITION_OPTIONS,
    TAB_TITLE_MODE_OPTIONS, THEME_PREFERENCE_OPTIONS, TextField, apply_timing, can_move,
    default_picker_options, default_picker_selected, dropdown_selected, effective_default_row,
    etc_shells_content, shell_dropdown_selected, shell_label, shell_picker_options, tab_header,
    tab_sections,
};
use crate::config::{
    DropTermConfig, MAX_HEIGHT_PERCENT, MAX_WIDTH_PERCENT, MIN_HEIGHT_PERCENT, MIN_WIDTH_PERCENT,
};
use crate::profile::Profile;

/// The tab strip's model: one row per tab of [`SettingsTab::ALL`],
/// labeled [`tab_header`] and carrying its `SettingsTab` as the row
/// data — the payload the activation handler resolves back to a tab
/// (criterion 7's entity↔tab lockstep). Built once at init and
/// stored on the app; the strip renders from the stored model, never
/// a per-frame rebuild.
/// The stored tab-strip model type — libcosmic's single-select
/// segmented-button model, aliased so the app field and the strip
/// builders below share one name.
pub(crate) type TabStripModel = widget::segmented_button::SingleSelectModel;

pub(crate) fn tab_strip_model() -> TabStripModel {
    let mut model = widget::segmented_button::SingleSelectModel::default();
    for tab in SettingsTab::ALL {
        model
            .insert()
            .text(tab_header(tab))
            .data::<SettingsTab>(tab);
    }
    model
}

/// Move the strip's selection to the row carrying `tab` — the
/// lockstep sync applied after every `settings_page` mutation (a
/// pick, the Profiles flip and its back, the explicit close's reset;
/// criterion 7). A tab with no row — impossible while the model is
/// built from [`SettingsTab::ALL`] — selects nothing; no row is
/// invented.
pub(crate) fn activate_tab_row(model: &mut TabStripModel, tab: SettingsTab) {
    // Collect first: `iter` holds the borrow the activation needs
    // to mutate.
    let entities: Vec<_> = model.iter().collect();
    for entity in entities {
        if model.data::<SettingsTab>(entity) == Some(&tab) {
            model.activate(entity);
        }
    }
}

/// An event from the settings view. Events carry inputs, never
/// [`PlannedWrite`](super::PlannedWrite)s: the view stays dumb about validity, and the
/// app resolves each event through the pure planners
/// ([`submit_write`](super::submit_write), [`dropdown_write`](super::dropdown_write), [`toggle_write`],
/// [`slider_release_write`](super::slider_release_write)) so every commit is clamped and every
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
    /// A dropdown option was picked: commit on change.
    Picked(ConfigKey, usize),
    /// A scheme picker's closed control was pressed: flip that
    /// picker's popover open state (the app owns the flag; one
    /// picker is open at most — the two global controls and the
    /// profile form's picker share the one-open rule and the
    /// Escape ordering).
    ToggleSchemePicker(SchemePickerId),
    /// The open scheme picker should close without picking — Escape
    /// or a press outside its bubble (the dropdown's dismissal
    /// contract).
    CloseSchemePicker,
    /// A toggler flipped: commit on change.
    Toggled(ConfigKey, bool),
    /// The Keybindings tab's search box changed — display only; the
    /// view re-filters its rows.
    KeybindSearchChanged(String),
    /// A Keybindings row's binding was pressed: arm the capture for
    /// that action — from here the app's capture routing owns every
    /// press until the chord commits, Escape or Cancel ends it, or
    /// the page turns away.
    CaptureStart(crate::terminal::Action),
    /// The capture hint's Cancel affordance: disarm without changes.
    CaptureCancel,
    /// A row's revert affordance: drop the action's custom entry; its
    /// defaults come back on the next key press.
    RevertKeybind(crate::terminal::Action),
    /// A profile row was pressed: make it the selection driving the
    /// edit form.
    SelectProfile(usize),
    /// A default-profile picker row was picked: commit on change.
    PickDefaultProfile(usize),
    /// The Add affordance: open an empty draft form.
    OpenProfileDraft,
    /// A row's Clone action: seed a draft from that profile.
    CloneProfile(usize),
    /// A form field's content changed — display only (no write until
    /// Save).
    DraftProfileField(ProfileField, String),
    /// The form's Save: an Enter in any input or the footer's Save
    /// button — validate the whole draft, write once if it passes,
    /// close the form on the row it wrote.
    SaveProfileDraft,
    /// The form's Cancel: the footer's Cancel button (Escape routes
    /// here through the drawer's key funnel) — drop the draft, write
    /// nothing.
    CancelProfileDraft,
    /// The form's scheme picker was picked: the pick buffers in
    /// every mode — the recolor lands on Save with the rest of the
    /// draft (the drawer's commit-on-change rule does not reach
    /// inside the transactional form). Row 0 is the Inherit row.
    PickProfileScheme(usize),
    /// The env rows editor's add affordance: buffer one blank row.
    /// Display-only like a keystroke — the form writes nothing until
    /// Save.
    AddProfileEnvRow,
    /// The env rows editor's remove affordance: drop the buffered
    /// row at `index`. Display-only — the write rides Save, exactly
    /// like buffered text.
    RemoveProfileEnvRow(usize),
    /// A row's Delete press: arm that row's inline Confirm/Keep
    /// pair (arming another row disarms the first — single-row
    /// arming; the pair replaces the bare click the row's delete
    /// used to be).
    ArmProfileDelete(usize),
    /// The armed row's Confirm press: delete it — one whole-list
    /// write plus the paired auto-clear when the deleted profile
    /// was the default. Immediate from Confirm, never a dialog.
    ConfirmProfileDelete(usize),
    /// The armed row's Keep press: disarm, delete nothing.
    KeepProfileDelete,
    /// A row's reorder press: move that profile one slot.
    ReorderProfile(usize, ReorderDirection),
    /// The drawer's resize handle is dragging: the pointer's
    /// window-absolute x (drawer-resize spec criterion 2).
    /// Display-only until release — nothing writes mid-drag.
    DrawerResize(f32),
    /// The handle's press ended: commit the live width if the drag
    /// changed it (criterion 3).
    DrawerResizeReleased,
    /// The handle was double-clicked: reset to the built-in
    /// default width and clear the stored value (criterion 5).
    DrawerReset,
}

impl Section {
    /// The card's heading text.
    pub(super) const fn title(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Window => "Window",
            Section::Tabs => "Tabs",
            Section::StatusBar => "Status bar",
            Section::Dropdown => "Dropdown",
            Section::Terminal => "Terminal",
            Section::Advanced => "Advanced",
        }
    }
}

impl ConfigKey {
    /// The control's human-facing label (its file field is
    /// [`ConfigKey::key_name`]).
    pub(super) const fn label(self) -> &'static str {
        match self {
            ConfigKey::HeightPercent => "Height",
            ConfigKey::WidthPercent => "Width",
            ConfigKey::HAlign => "Alignment",
            ConfigKey::Edge => "Edge",
            ConfigKey::TabPosition => "Tab bar position",
            ConfigKey::FontName => "Font",
            ConfigKey::FontSize => "Font size",
            ConfigKey::Opacity => "Opacity",
            ConfigKey::HideOnFocusLoss => "Hide on focus loss",
            ConfigKey::SessionPersistenceEnabled => "Session persistence",
            ConfigKey::SlideAnimationEnabled => "Slide animation",
            ConfigKey::BellNotificationEnabled => "Bell notifications",
            ConfigKey::AutosuggestionsEnabled => "Autosuggestions",
            ConfigKey::TabTitleMode => "Tab titles",
            ConfigKey::NewTabAfterCurrent => "New tabs after current tab",
            ConfigKey::NewTabCwd => "New tab directory",
            ConfigKey::CloseConfirmationEnabled => "Confirm closing a tab with a running process",
            ConfigKey::SettingsDrawerClickClose => {
                "Close the settings drawer by clicking the terminal"
            }
            ConfigKey::RespectPanel => "Respect panel",
            ConfigKey::Shell => "Shell",
            ConfigKey::Args => "Arguments",
            ConfigKey::DefaultCwd => "Default directory",
            ConfigKey::ScrollbackLines => "Scrollback lines",
            ConfigKey::Layer => "Layer",
            ConfigKey::KeyboardMode => "Keyboard mode",
            ConfigKey::ColorSchemeDark => "Dark color scheme",
            ConfigKey::ColorSchemeLight => "Light color scheme",
            ConfigKey::ThemePreference => "Application theme",
            // No control renders this label (no section lists the
            // key); the arm keeps the match exhaustive for the
            // drag-only writer.
            ConfigKey::SettingsDrawerWidth => "Drawer width",
            ConfigKey::StatusBarEnabled => "Show the status band",
            ConfigKey::StatusBarShowProcess => "Show the process name",
            ConfigKey::StatusBarShowPath => "Show the working directory",
            ConfigKey::StatusBarShowBranch => "Show the git branch",
            ConfigKey::StatusBarShowProfile => "Show the profile",
            ConfigKey::StatusBarShowGrid => "Show the terminal size",
            ConfigKey::StatusBarShowTab => "Show the tab position",
            ConfigKey::StatusBarShowZoom => "Show the zoom level",
            ConfigKey::StatusBarShowClock => "Show the clock",
            ConfigKey::TabBarBorder => "Show the tab bar border",
            ConfigKey::StatusBarBorder => "Show the status band border",
        }
    }
}

/// The font-size slider's display range. The slider constrains what
/// the control can produce; the write path applies no extra clamp.
pub(super) const FONT_SIZE_SLIDER_MIN: f32 = FONT_SIZE_MIN as f32;
pub(super) const FONT_SIZE_SLIDER_MAX: f32 = FONT_SIZE_MAX as f32;

/// Wrap a drawer page with the resize handle (spec criterion 2):
/// the handle leads the page in a row, full height, so the drag
/// edge sits at the pane's leading side for whichever page is
/// showing. The handle emits [`SettingsEvent::DrawerResize`] &
/// kin directly — the events ride the page's existing
/// `Message::Settings` funnel, so both pages share one drag path.
pub(crate) fn drawer_with_handle(
    content: Element<'static, SettingsEvent>,
) -> Element<'static, SettingsEvent> {
    widget::row![
        crate::drawer_handle::DrawerHandle::new(
            SettingsEvent::DrawerResize,
            || SettingsEvent::DrawerResizeReleased,
            || SettingsEvent::DrawerReset,
        ),
        content,
    ]
    .width(cosmic::iced::Length::Fill)
    .height(cosmic::iced::Length::Fill)
    .into()
}

/// The Appearance tab's page: how the terminal looks — the look
/// card (font, size, opacity, the two scheme pickers, theme) and the
/// Window card (the surface's shape). `scheme_picker` names the
/// scheme control whose popover is open (view state the app owns).
/// Every control renders from `config` with a buffered draft winning
/// while one exists and emits [`SettingsEvent`]s — nothing here
/// applies anything itself; commits land through the app's write
/// path and the `ConfigChanged` echo.
pub(crate) fn appearance_view(
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
    scheme_picker: Option<SchemePickerId>,
) -> Element<'static, SettingsEvent> {
    tab_page(SettingsTab::Appearance, config, buffer, scheme_picker)
}

/// The Behavior tab's page: how `DropTerm` acts — the Tabs card (the
/// tab strip's settings), the Dropdown card (the drop-down's own
/// mechanics), and the Terminal card (the global shell defaults and
/// the bell).
pub(crate) fn behavior_view(
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
) -> Element<'static, SettingsEvent> {
    tab_page(SettingsTab::Behavior, config, buffer, None)
}

/// The Advanced tab's page: exactly the Advanced card as it hosted
/// in the one-column drawer — the power controls.
pub(crate) fn advanced_view(
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
) -> Element<'static, SettingsEvent> {
    tab_page(SettingsTab::Advanced, config, buffer, None)
}

/// The Keybindings tab's page: the search input on top, then one
/// card per group of [`crate::keybinds::action_groups`] — every
/// bindable action exactly once, each row the action's label with
/// its effective binding text ([`crate::keybinds::action_binding_display`]:
/// the custom binding, or the joined defaults, or "None") as the
/// arm-capture affordance, plus the revert affordance while a custom
/// entry exists. The armed capture shows its hint row under the
/// action's row; a conflict shows the inline error naming the holder
/// (the `danger_text_style` precedent) — never a dialog, which
/// cannot stack above the layer surface. The search filters rows
/// case-insensitively by label; a group whose rows all fell away
/// drops out, and a query nothing matches renders the empty state.
/// Nothing here applies anything itself — commits and reverts land
/// through the app's write path and the `ConfigChanged` echo.
pub(crate) fn keybindings_view(
    custom: &std::collections::BTreeMap<String, crate::keybinds::Binding>,
    search: &str,
    capture: Option<crate::terminal::Action>,
    conflict: Option<(crate::terminal::Action, crate::terminal::Action)>,
) -> Element<'static, SettingsEvent> {
    let mut cards: Vec<Element<'static, SettingsEvent>> = vec![
        widget::text_input::search_input("Search", search.to_string())
            .on_input(SettingsEvent::KeybindSearchChanged)
            .into(),
    ];
    for group in crate::keybinds::action_groups() {
        let mut list = widget::list::list_column();
        let mut found = false;
        for action in group.actions {
            if !crate::keybinds::action_matches_search(action, search) {
                continue;
            }
            found = true;
            let armed = capture == Some(action);
            let customized =
                crate::keybinds::action_id(action).is_some_and(|id| custom.contains_key(id));
            // The binding text is the arm affordance — press it, then
            // press the chord; the app's capture routing owns every
            // press from there. The revert affordance rides beside it
            // only while a custom entry exists.
            let mut control = widget::row![
                widget::button::custom(widget::text(crate::keybinds::action_binding_display(
                    action, custom
                )))
                .on_press(SettingsEvent::CaptureStart(action)),
            ]
            .spacing(4);
            if customized {
                control = control.push(widget::tooltip(
                    widget::button::custom(
                        widget::icon(
                            widget::icon::from_name("edit-undo-symbolic")
                                .size(16)
                                .handle(),
                        )
                        .size(16),
                    )
                    .on_press(SettingsEvent::RevertKeybind(action)),
                    widget::text::body("Reset to default"),
                    widget::tooltip::Position::Top,
                ));
            }
            list = list.add(
                widget::settings::item::builder(crate::keybinds::action_label(action))
                    .control(control),
            );
            if armed {
                // The capture-state rule made visible: while armed,
                // capture consumes every press — the terminal never
                // sees it. Escape cancels; so does the Cancel button.
                list = list.add(
                    widget::settings::item_row(vec![
                        widget::text::body("Press keys to bind — Escape cancels")
                            .width(cosmic::iced::Length::Fill)
                            .into(),
                        widget::button::text("Cancel")
                            .on_press(SettingsEvent::CaptureCancel)
                            .into(),
                    ])
                    .spacing(4),
                );
            }
            if let Some((_, holder)) = conflict.filter(|(conflicted, _)| *conflicted == action) {
                list = list.add(widget::settings::item_row(vec![
                    widget::text::body(format!(
                        "Already bound to “{}”",
                        crate::keybinds::action_label(holder)
                    ))
                    .width(cosmic::iced::Length::Fill)
                    .class(cosmic::theme::Text::Custom(danger_text_style))
                    .into(),
                ]));
            }
        }
        if found {
            cards.push(
                widget::settings::section::with_column(list)
                    .title(group.title)
                    .into(),
            );
        }
    }
    if cards.len() == 1 {
        // Only the search input survived the filter — the empty
        // result state.
        cards.push(
            widget::settings::section()
                .add(widget::text("No matching actions"))
                .into(),
        );
    }
    widget::settings::view_column(cards).into()
}

/// A tab's page: one `widget::settings` card per section the tab
/// hosts, one labeled item per exposed key with its apply timing as
/// the description, in one column the framework drawer pane already
/// wraps in a scrollable. The `drawer_section`/`drawer_item`/
/// `drawer_control` chain is reused verbatim from the one-column
/// drawer.
fn tab_page(
    tab: SettingsTab,
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
    scheme_picker: Option<SchemePickerId>,
) -> Element<'static, SettingsEvent> {
    let cards: Vec<Element<'static, SettingsEvent>> = tab_sections(tab)
        .iter()
        .map(|section| drawer_section(*section, config, buffer, scheme_picker))
        .collect();
    widget::settings::view_column(cards).into()
}

/// The Profiles tab's page: the list — one row per profile in
/// config order (the `+` dropdown / context-menu order), each row a
/// thumbnail of the tab it opens (see [`profile_row`]) — the edit
/// form below it, and the default picker above. While a draft is
/// open the list is locked (see [`profile_event_reachable`](super::profile_event_reachable)): every
/// row control and the add affordance render disabled, and the
/// armed delete pair waits hidden until the draft closes. `armed`
/// names the row rendering its inline Confirm/Keep pair; `is_dark`
/// resolves the Inherit row's swatch the way the terminal renders.
/// An empty list shows the empty state with the add affordance.
pub(crate) fn profiles_view(
    config: &DropTermConfig,
    selection: Option<usize>,
    draft: Option<&ProfileDraft>,
    armed: Option<usize>,
    picker_open: bool,
    is_dark: bool,
) -> Element<'static, SettingsEvent> {
    // The lock is the draft's presence: the list mutates nothing
    // while the form below it holds an uncommitted draft.
    let locked = draft.is_some();
    let mut cards: Vec<Element<'static, SettingsEvent>> = Vec::new();
    // The default picker: commit-on-change, the drawer's dropdown
    // rule. Selection mirrors `effective_default`, so it never opens
    // on a name the list lacks. Not a list mutation — it stays live
    // under the draft lock.
    cards.push(
        widget::settings::section()
            .title("Default profile")
            .add(
                widget::settings::item::builder("Plain new tabs run")
                    .description("next spawn")
                    .control(widget::dropdown(
                        default_picker_options(&config.profiles),
                        Some(default_picker_selected(
                            &config.profiles,
                            config.default_profile.as_deref(),
                        )),
                        SettingsEvent::PickDefaultProfile,
                    )),
            )
            .into(),
    );
    // The row the default tag marks — the picker's own resolution,
    // so the tag can never name a profile the list lacks.
    let default_row = effective_default_row(&config.profiles, config.default_profile.as_deref());
    if config.profiles.is_empty() {
        cards.push(
            widget::settings::section()
                .title("Profiles")
                .add(
                    widget::settings::item::builder("No profiles yet")
                        .description(
                            "A profile is a named preset — the command, directory, colors, \
                             and environment a new tab opens with.",
                        )
                        .control(add_profile_button(locked)),
                )
                .into(),
        );
    } else {
        let mut list = widget::settings::section().title("Profiles");
        for (index, profile) in config.profiles.iter().enumerate() {
            // The armed pair renders only on an unlocked row: the
            // lock degrades an armed row to the idle trash until the
            // draft closes.
            let delete = if armed == Some(index) && !locked {
                RowDelete::Armed
            } else {
                RowDelete::Idle
            };
            list = list.add(profile_row(
                config.profiles.len(),
                index,
                profile,
                selection == Some(index),
                default_row == Some(index),
                delete,
                locked,
            ));
        }
        cards.push(list.into());
        // The list's add affordance, below the rows it appends after.
        cards.push(add_profile_button(locked).into());
    }
    // The form: one draft at a time — an Add/Clone draft the buttons
    // opened, or the Edit draft a row selection seeded.
    if let Some(draft) = draft {
        cards.push(profile_form(config, draft, picker_open, is_dark));
    }
    widget::settings::view_column(cards).into()
}

/// The page's add affordance: the one text button among the row
/// actions, so creation stays the loud gesture (the `+` matches the
/// tab bar's own new-tab affordance). Disabled while the draft lock
/// holds — an open draft is the page's one transaction.
fn add_profile_button(locked: bool) -> widget::button::TextButton<'static, SettingsEvent> {
    widget::button::standard("Add profile")
        .leading_icon(widget::icon::from_name("list-add-symbolic"))
        .on_press_maybe((!locked).then_some(SettingsEvent::OpenProfileDraft))
}

/// The form section: the draft's five text inputs (Enter is Save,
/// the form's one commit gesture) with every rejected field's
/// inline error under it, the scheme picker with its leading
/// Inherit row (buffered — the drawer's commit-on-change rule does
/// not reach inside the transactional form), the font-size and
/// scrollback inputs with their `inherits <global>` placeholders
/// naming what empty means, the env rows editor, and the Save/Cancel
/// footer. The Edit title names the profile the row selection
/// seeded — the seed row may sit above the fold — and follows the
/// committed name, never the uncommitted buffer.
fn profile_form(
    config: &DropTermConfig,
    draft: &ProfileDraft,
    picker_open: bool,
    is_dark: bool,
) -> Element<'static, SettingsEvent> {
    let title = match &draft.mode {
        ProfileDraftMode::Add | ProfileDraftMode::Clone { .. } => "New profile".to_string(),
        ProfileDraftMode::Edit { index } => match config.profiles.get(*index) {
            Some(profile) => format!("Edit {}", profile.name),
            // A stale edit (the list changed under the draft) has no
            // profile to name; the generic title covers the gap until
            // the save rejects it.
            None => "Edit profile".to_string(),
        },
    };
    // The scheme item carries the picker plus, on a rejected Save,
    // the scheme's own inline error under it — the `form_field`
    // rule, picker edition.
    let scheme_control: Element<'static, SettingsEvent> = {
        let control = profile_scheme_control(config, draft, picker_open, is_dark);
        match draft
            .errors
            .iter()
            .copied()
            .find(|(at, _)| *at == ProfileField::Scheme)
        {
            Some((_, error)) => widget::column![
                control,
                widget::text(error.message())
                    .size(12)
                    .class(cosmic::theme::Text::Custom(danger_text_style)),
            ]
            .spacing(4)
            .into(),
            None => control,
        }
    };
    widget::settings::section()
        .title(title)
        .add(form_field(
            ProfileField::Name,
            "Name",
            &draft.name,
            "web-prod".to_string(),
            draft,
        ))
        .add(form_field(
            ProfileField::Command,
            "Command",
            &draft.command,
            "ssh web-prod".to_string(),
            draft,
        ))
        .add(form_field(
            ProfileField::Cwd,
            "Working directory",
            &draft.cwd,
            "optional".to_string(),
            draft,
        ))
        .add(widget::settings::item::builder("Color scheme").control(scheme_control))
        .add(form_field(
            ProfileField::FontSize,
            "Font size",
            &draft.font_size,
            format!("inherits {}", config.font_size),
            draft,
        ))
        .add(form_field(
            ProfileField::Scrollback,
            "Scrollback lines",
            &draft.scrollback,
            format!("inherits {}", config.scrollback_lines),
            draft,
        ))
        .add(env_rows_item(draft))
        .add(profile_form_footer())
        .into()
}

/// The form's footer: the transaction's two affordances in one
/// right-aligned row under the env editor — Save as the standard
/// button (the primary action, the one an Enter also presses) with
/// Cancel's quiet text button beside it. Every draft carries it:
/// Add, Clone, and Edit alike.
fn profile_form_footer() -> Element<'static, SettingsEvent> {
    widget::row![
        widget::space::Space::new().width(Length::Fill),
        widget::button::text("Cancel").on_press(SettingsEvent::CancelProfileDraft),
        widget::button::standard("Save").on_press(SettingsEvent::SaveProfileDraft),
    ]
    .spacing(8)
    .into()
}

/// The env rows editor: one key + value input pair per buffered row
/// with its remove affordance, an add affordance beneath, and a
/// rejected Save's inline error under every offending row's key
/// input. Enter in any row input is Save, like every other field;
/// add/remove are buffered list edits, not writes — no gesture but
/// Save writes.
fn env_rows_item(draft: &ProfileDraft) -> Element<'static, SettingsEvent> {
    let mut rows = widget::column(Vec::<Element<'static, SettingsEvent>>::new()).spacing(4);
    for (index, row) in draft.env_rows.iter().enumerate() {
        let key_input = widget::text_input("Key", row.key.clone())
            .on_input(move |text| {
                SettingsEvent::DraftProfileField(ProfileField::EnvKey(index), text)
            })
            .on_submit(move |_| SettingsEvent::SaveProfileDraft);
        let value_input = widget::text_input("Value", row.value.clone())
            .on_input(move |text| {
                SettingsEvent::DraftProfileField(ProfileField::EnvValue(index), text)
            })
            .on_submit(move |_| SettingsEvent::SaveProfileDraft);
        let inputs = widget::row!(key_input, value_input, remove_row_button(index)).spacing(8);
        // A rejected Save's row error renders under the offending
        // row's key input — the row's name (the `form_field` rule,
        // row edition).
        let pair = match draft
            .errors
            .iter()
            .copied()
            .find(|(at, _)| *at == ProfileField::EnvKey(index))
        {
            Some((_, error)) => widget::column![
                inputs,
                widget::text(error.message())
                    .size(12)
                    .class(cosmic::theme::Text::Custom(danger_text_style)),
            ]
            .spacing(4),
            None => widget::column![inputs].spacing(4),
        };
        rows = rows.push(pair);
    }
    rows =
        rows.push(widget::button::text("Add variable").on_press(SettingsEvent::AddProfileEnvRow));
    let control: Element<'static, SettingsEvent> = rows.into();
    widget::settings::item::builder("Environment variables")
        .control(control)
        .into()
}

/// One env row's remove affordance (the reorder cluster's plain-text
/// button idiom).
fn remove_row_button(index: usize) -> Element<'static, SettingsEvent> {
    widget::button::text("✕")
        .on_press(SettingsEvent::RemoveProfileEnvRow(index))
        .into()
}

/// One form field: the labeled text input with its inline error,
/// when the last Save's rejection named this field. Enter is Save —
/// the form's one commit gesture, so the submit carries no payload.
/// The placeholder is owned text: the override fields name the
/// global value they'd inherit (`inherits 14`).
fn form_field(
    field: ProfileField,
    label: &'static str,
    text: &str,
    placeholder: String,
    draft: &ProfileDraft,
) -> Element<'static, SettingsEvent> {
    let input = widget::text_input(placeholder, text.to_string())
        .on_input(move |text| SettingsEvent::DraftProfileField(field, text))
        .on_submit(move |_| SettingsEvent::SaveProfileDraft);
    let control = match draft.errors.iter().copied().find(|(at, _)| *at == field) {
        Some((_, error)) => widget::column![
            input,
            widget::text(error.message())
                .size(12)
                .class(cosmic::theme::Text::Custom(danger_text_style)),
        ]
        .spacing(4),
        None => widget::column![input],
    };
    widget::settings::item::builder(label)
        .control(control)
        .into()
}

/// The inline-error text style: the theme's destructive color — the
/// same red the destructive-button class carries (the pinned rev has
/// no `Text::Danger` variant).
fn danger_text_style(theme: &cosmic::theme::Theme) -> cosmic::iced::widget::text::Style {
    let cosmic = theme.cosmic();
    cosmic::iced::widget::text::Style {
        color: Some(cosmic.destructive.base.into()),
        selected_fill: cosmic.accent.base.into(),
        selected_text_color: Some(cosmic.on_accent_color().into()),
    }
}

/// The row's delete cluster state: the idle trash (arming the
/// delete; disabled under the draft lock) or the armed inline
/// Confirm/Keep pair replacing it. An armed row under the lock
/// degrades to the idle trash until the draft closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowDelete {
    /// The trash icon; its press arms the pair.
    Idle,
    /// The armed pair — Confirm deletes, Keep disarms.
    Armed,
}

/// One list row: the profile as a thumbnail of the tab it opens.
/// The leading edge carries the scheme chip ([`scheme_chip`]), the
/// identity column the name — with the dimmed `default` tag when the
/// picker's resolution ([`effective_default_row`](super::effective_default_row)) names this row —
/// above the command as a prompt line ([`prompt_line`]) in the
/// terminal's own mono. The whole identity column is the select
/// button, filling the row's remaining width so the press target is
/// the row, not just the text; the edit pencil emits the same
/// selection event, making the edit path visible (the identity
/// column absorbs the cluster's width cost at the drawer's minimum).
/// The action cluster is tooltip'd icon buttons
/// ([`row_icon_button`]): Clone, the two-step delete (the trash arms
/// the row's inline Confirm/Keep pair — [`RowDelete::Armed`]), and
/// up/down reorder with the boundary direction disabled. While a
/// draft is open (`locked`) every one of them — the identity select
/// included — renders disabled: the draft's origin index is only
/// valid because nothing can shift it (the draft lock). The selected
/// row carries the accent edge-and-tint treatment
/// ([`row_button_class`]) — visible on light and dark panes alike
/// (an OQ-3 hardware item) without spending the accent fill, which
/// stays the primary-action color.
fn profile_row(
    profiles_len: usize,
    index: usize,
    profile: &Profile,
    selected: bool,
    is_default: bool,
    delete: RowDelete,
    locked: bool,
) -> Element<'static, SettingsEvent> {
    let mut name_line = widget::row!(widget::text(profile.name.clone()))
        .spacing(6)
        .align_y(cosmic::iced::Alignment::Center);
    if is_default {
        name_line = name_line.push(
            widget::text("default")
                .size(12)
                .class(theme::Text::Custom(dim_text_style)),
        );
    }
    let identity = widget::column![name_line, prompt_line(&profile.command)].spacing(2);
    // The delete cluster: an armed row trades its trash for the
    // inline Confirm/Keep pair (single-row arming); under the draft
    // lock the cluster is just one more disabled list control — the
    // arm waits out the draft, then the pair returns.
    let delete: Element<'static, SettingsEvent> = if matches!(delete, RowDelete::Armed) {
        widget::row![
            widget::button::text("Confirm").on_press(SettingsEvent::ConfirmProfileDelete(index)),
            widget::button::text("Keep").on_press(SettingsEvent::KeepProfileDelete),
        ]
        .spacing(4)
        .into()
    } else {
        row_icon_button("user-trash-symbolic", "Delete profile")
            .on_press_maybe((!locked).then_some(SettingsEvent::ArmProfileDelete(index)))
            .into()
    };
    widget::row![
        scheme_chip(profile.color_scheme.as_deref()),
        widget::button::custom(identity)
            .class(row_button_class(selected))
            .width(Length::Fill)
            .on_press_maybe((!locked).then_some(SettingsEvent::SelectProfile(index))),
        row_icon_button("edit-symbolic", "Edit profile")
            .on_press_maybe((!locked).then_some(SettingsEvent::SelectProfile(index))),
        row_icon_button("edit-copy-symbolic", "Clone profile")
            .on_press_maybe((!locked).then_some(SettingsEvent::CloneProfile(index))),
        delete,
        row_icon_button("go-up-symbolic", "Move up").on_press_maybe(
            (!locked && can_move(profiles_len, index, ReorderDirection::Up))
                .then_some(SettingsEvent::ReorderProfile(index, ReorderDirection::Up)),
        ),
        row_icon_button("go-down-symbolic", "Move down").on_press_maybe(
            (!locked && can_move(profiles_len, index, ReorderDirection::Down))
                .then_some(SettingsEvent::ReorderProfile(index, ReorderDirection::Down)),
        ),
    ]
    .spacing(8)
    .into()
}

/// One row-action icon button: the icon carries the action, the
/// tooltip names it (the keybinds page's undo-affordance idiom). All
/// five share the plain icon class — the cluster stays quiet so the
/// chip and the selected edge are the row's two signals.
fn row_icon_button(
    name: &'static str,
    tip: &'static str,
) -> widget::button::IconButton<'static, SettingsEvent> {
    widget::button::icon(widget::icon::from_name(name)).tooltip(tip)
}

/// The row's command preview: a dimmed prompt glyph and the command
/// line in the terminal's own mono — the line the tab will run, in
/// the vernacular it will run in.
fn prompt_line(command: &str) -> Element<'static, SettingsEvent> {
    widget::row![
        widget::text::monotext("$")
            .size(12)
            .class(theme::Text::Custom(dim_text_style)),
        widget::text::monotext(command.to_string())
            .size(12)
            .class(theme::Text::Custom(dim_text_style)),
    ]
    .spacing(4)
    .into()
}

/// The dimmed text style: the on-background color at reduced alpha —
/// the row's secondary line and the default tag, on light and dark
/// panes alike.
fn dim_text_style(theme: &cosmic::theme::Theme) -> cosmic::iced::widget::text::Style {
    let cosmic = theme.cosmic();
    cosmic::iced::widget::text::Style {
        color: Some(Color::from(cosmic.background(true).on).scale_alpha(0.65)),
        selected_fill: cosmic.accent.base.into(),
        selected_text_color: Some(cosmic.on_accent_color().into()),
    }
}

/// The select button's class: the standard button when unselected;
/// the accent edge-and-tint treatment when selected — an accent
/// border plus a light accent tint over the standard background,
/// deepening on hover and press so the interaction states stay
/// legible.
fn row_button_class(selected: bool) -> theme::Button {
    const SELECTED_TINT: f32 = 0.12;
    if !selected {
        return theme::Button::Standard;
    }
    theme::Button::Custom {
        active: Box::new(move |_, theme| selected_row_appearance(theme, SELECTED_TINT)),
        disabled: Box::new(move |theme| selected_row_appearance(theme, SELECTED_TINT)),
        hovered: Box::new(move |_, theme| selected_row_appearance(theme, SELECTED_TINT + 0.06)),
        pressed: Box::new(move |_, theme| selected_row_appearance(theme, SELECTED_TINT + 0.12)),
    }
}

/// The selected row's appearance: the standard button's own
/// background blended toward the accent, edged in accent — the
/// `Button::Image` selected idiom (an accent border on a themed
/// surface) carried onto the profile row.
fn selected_row_appearance(
    theme: &cosmic::theme::Theme,
    tint: f32,
) -> cosmic::widget::button::Style {
    let cosmic = theme.cosmic();
    let accent = Color::from(cosmic.accent.base);
    cosmic::widget::button::Style {
        background: Some(Background::Color(blend(
            Color::from(cosmic.button.base),
            accent,
            tint,
        ))),
        border_width: 1.5,
        border_color: accent,
        border_radius: cosmic.corner_radii.radius_xl.into(),
        ..Default::default()
    }
}

/// Linear blend of `tint` into `base` at `factor` (0 = `base`, 1 =
/// `tint`) — the selected row's tint math.
fn blend(base: Color, tint: Color, factor: f32) -> Color {
    Color::from_rgba(
        base.r + (tint.r - base.r) * factor,
        base.g + (tint.g - base.g) * factor,
        base.b + (tint.b - base.b) * factor,
        1.0,
    )
}

/// One drawer section: a `widget::settings` section carrying the
/// section's title and one item per key it owns, in `keys()` display
/// order.
fn drawer_section(
    section: Section,
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
    scheme_picker: Option<SchemePickerId>,
) -> Element<'static, SettingsEvent> {
    let mut built = widget::settings::section().title(section.title());
    for key in section.keys() {
        built = built.add(drawer_item(*key, config, buffer, scheme_picker));
    }
    built.into()
}

/// The item description a key renders: its apply-timing label, with
/// the three plain-spawn rows carrying the `default_profile` note
/// (ruling 4) — a set, existing default still owns command, cwd, env,
/// and colors outright; the picker and `default_cwd` govern only the
/// no-default path.
pub(super) fn drawer_description(key: ConfigKey) -> String {
    match key {
        ConfigKey::Shell | ConfigKey::NewTabCwd | ConfigKey::DefaultCwd => format!(
            "{} — a default profile still owns plain spawns",
            apply_timing(key).label()
        ),
        _ => apply_timing(key).label().to_string(),
    }
}

/// One drawer item: the key's label with its apply timing as the
/// item description (the honesty rule carried into the drawer), and
/// the key's control on the drawer's natural widths — the window's
/// fixed `CONTROL_WIDTH` is gone, the pane owns width now.
fn drawer_item(
    key: ConfigKey,
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
    scheme_picker: Option<SchemePickerId>,
) -> Element<'static, SettingsEvent> {
    let control = drawer_control(key, config, buffer, scheme_picker)
        .expect("every exposed key has exactly one control (section map)");
    widget::settings::item::builder(key.label())
        .description(drawer_description(key))
        .control(control)
        .into()
}

/// The key's control widget: the scheme pickers, then sliders (the
/// buffered readout above), then dropdowns, then toggles, then text
/// inputs. Each builder claims only its own keys; the `expect` in
/// [`drawer_item`] is totality — every exposed key has exactly one
/// control, pinned by the section-map test.
fn drawer_control(
    key: ConfigKey,
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
    scheme_picker: Option<SchemePickerId>,
) -> Option<Element<'static, SettingsEvent>> {
    // The two scheme controls are pickers, not stock dropdowns: the
    // palette is the preview (see the scheme-picker section).
    if matches!(
        key,
        ConfigKey::ColorSchemeDark | ConfigKey::ColorSchemeLight
    ) {
        return Some(scheme_picker_control(
            key,
            config,
            scheme_picker == Some(SchemePickerId::Global(key)),
        ));
    }
    // The Shell item is the one two-control setting (default-spawn
    // DS-2): the detected-shells dropdown with the custom-path text
    // input beneath, claimed ahead of the single-control chain.
    if key == ConfigKey::Shell {
        return Some(shell_control(config, buffer));
    }
    slider_control(key, config, buffer)
        .or_else(|| dropdown_control(key, config))
        .or_else(|| toggle_control(key, config))
        .or_else(|| text_control(key, config, buffer))
}

/// The Shell item's control (default-spawn DS-2): the detected-shells
/// dropdown — options built per view from `/etc/shells`, basename
/// labels, full-path writes under the existing `shell` key — with the
/// free-text input beneath as the custom path's editor (placeholder
/// still `resolved_shell()`; an off-list stored value selects no
/// dropdown row and shows here, as always).
fn shell_control(
    config: &DropTermConfig,
    buffer: &SettingsBuffer,
) -> Element<'static, SettingsEvent> {
    let options = shell_picker_options(etc_shells_content().as_deref());
    let labels: Vec<String> = options.iter().map(|path| shell_label(path)).collect();
    let field = TextField::Shell;
    widget::column![
        widget::dropdown(
            labels,
            shell_dropdown_selected(&options, config.shell.as_deref()),
            move |index| SettingsEvent::Picked(ConfigKey::Shell, index),
        ),
        widget::text_input(config.resolved_shell(), buffer.text(field, config))
            .on_input(move |text| SettingsEvent::Draft(field, text))
            .on_submit(move |text| SettingsEvent::Submitted(field, text)),
    ]
    .spacing(4)
    .into()
}

/// A slider control (commit on release). `on_change` only buffers
/// the position for the live readout; the release commits through
/// [`slider_release_write`](super::slider_release_write), clamped.
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
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "integer readout of a range-bounded slider value; non-negative by the control's range"
    )]
    let readout = value.round() as u32;
    Some(
        widget::column![
            widget::text(format!("{readout}")).size(12),
            widget::slider(range, value, move |v| SettingsEvent::SliderMoved(key, v))
                .on_release(SettingsEvent::SliderReleased(key)),
        ]
        .spacing(4)
        .into(),
    )
}

/// A dropdown control (commit on change). The pick resolves through
/// [`dropdown_write`](super::dropdown_write); labels and selection come from the same
/// tables the write path reads.
fn dropdown_control(
    key: ConfigKey,
    config: &DropTermConfig,
) -> Option<Element<'static, SettingsEvent>> {
    let labels: Vec<&'static str> = match key {
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
        _ => return None,
    };
    Some(
        widget::dropdown(labels, dropdown_selected(key, config), move |index| {
            SettingsEvent::Picked(key, index)
        })
        .into(),
    )
}

/// A toggle control (commit on change). The flip resolves through
/// [`toggle_write`](super::toggle_write).
fn toggle_control(
    key: ConfigKey,
    config: &DropTermConfig,
) -> Option<Element<'static, SettingsEvent>> {
    let checked = match key {
        ConfigKey::HideOnFocusLoss => config.hide_on_focus_loss,
        ConfigKey::SessionPersistenceEnabled => config.session_persistence_enabled,
        ConfigKey::SlideAnimationEnabled => config.slide_animation_enabled,
        ConfigKey::BellNotificationEnabled => config.bell_notification_enabled,
        ConfigKey::AutosuggestionsEnabled => config.autosuggestions_enabled,
        ConfigKey::NewTabAfterCurrent => config.new_tab_after_current,
        ConfigKey::CloseConfirmationEnabled => config.close_confirmation_enabled,
        ConfigKey::SettingsDrawerClickClose => config.settings_drawer_click_close,
        ConfigKey::RespectPanel => config.respect_panel,
        ConfigKey::StatusBarEnabled => config.status_bar_enabled,
        ConfigKey::TabBarBorder => config.tab_bar_border,
        ConfigKey::StatusBarBorder => config.status_bar_border,
        ConfigKey::StatusBarShowProcess => config.status_bar_show_process,
        ConfigKey::StatusBarShowPath => config.status_bar_show_path,
        ConfigKey::StatusBarShowBranch => config.status_bar_show_branch,
        ConfigKey::StatusBarShowProfile => config.status_bar_show_profile,
        ConfigKey::StatusBarShowGrid => config.status_bar_show_grid,
        ConfigKey::StatusBarShowTab => config.status_bar_show_tab,
        ConfigKey::StatusBarShowZoom => config.status_bar_show_zoom,
        ConfigKey::StatusBarShowClock => config.status_bar_show_clock,
        _ => return None,
    };
    Some(
        widget::toggler(checked)
            .on_toggle(move |checked| SettingsEvent::Toggled(key, checked))
            .into(),
    )
}

/// A text input (commit on submit — per-keystroke writes would
/// persist half-typed values and fight the echo, which re-renders
/// the input from config). The input shows the buffered draft while
/// one exists, else the file's value; the shell field's placeholder
/// carries `resolved_shell()`.
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
        ConfigKey::DefaultCwd => TextField::DefaultCwd,
        _ => return None,
    };
    let placeholder = match field {
        TextField::Shell => config.resolved_shell(),
        TextField::ScrollbackLines => "10000".to_string(),
        TextField::DefaultCwd => "optional".to_string(),
        _ => String::new(),
    };
    Some(
        widget::text_input(placeholder, buffer.text(field, config))
            .on_input(move |text| SettingsEvent::Draft(field, text))
            .on_submit(move |text| SettingsEvent::Submitted(field, text))
            .into(),
    )
}
#[cfg(test)]
mod tests {
    use super::super::ProfileFieldError;
    use super::super::tests::{env_row, profile, sourced_profile};
    use super::*;
    use std::collections::BTreeMap;

    // --- the drawer's migrated content (settings-drawer Task 1) ---

    #[test]
    fn every_section_page_builds_with_drafts_and_from_config() {
        // The section-hosted pages (Appearance, Behavior, Advanced)
        // over the six-card map. Construction is the assertion: the
        // item builder's `expect` totality panics if a key resolves
        // to no control, and the widget builders must tolerate the
        // states the file can be in — buffered drafts winning over
        // file values (including a scrollback draft that does not
        // parse — the draft simply renders; rejection is the update
        // path's job) — and the plain default config. Exactly-once
        // across the pages is tab_sections' partition, pinned below.
        let config = DropTermConfig::default();
        let mut buffer = SettingsBuffer::default();
        buffer.set_text(TextField::FontName, "Draft Font".to_string());
        buffer.set_text(TextField::Shell, "/bin/tcsh".to_string());
        buffer.set_text(TextField::Args, "-l -i".to_string());
        buffer.set_text(TextField::ScrollbackLines, "garbage".to_string());
        buffer.set_text(TextField::DefaultCwd, "/srv/work".to_string());
        buffer.set_slider(ConfigKey::Opacity, 42.0);
        let _ = appearance_view(&config, &buffer, None);
        let _ = behavior_view(&config, &buffer);
        let _ = advanced_view(&config, &buffer);
        let _ = appearance_view(&config, &SettingsBuffer::default(), None);
        let _ = behavior_view(&config, &SettingsBuffer::default());
        let _ = advanced_view(&config, &SettingsBuffer::default());
    }

    #[test]
    fn every_section_item_builds_a_control_for_every_key() {
        // Criterion 3's construction side: every key of every section
        // resolves to exactly one item control — the builder's
        // `expect` totality panics otherwise — with every text field
        // holding a draft (the buffer's display-wins rule) and a
        // scrollback draft that does not parse (the draft simply
        // renders; rejection is the update path's job).
        let config = DropTermConfig::default();
        let mut buffer = SettingsBuffer::default();
        buffer.set_text(TextField::FontName, "Draft Font".to_string());
        buffer.set_text(TextField::Shell, "/bin/tcsh".to_string());
        buffer.set_text(TextField::Args, "-l -i".to_string());
        buffer.set_text(TextField::ScrollbackLines, "garbage".to_string());
        for section in Section::ALL {
            for key in section.keys() {
                let _ = drawer_item(*key, &config, &buffer, None);
            }
        }
    }

    #[test]
    fn the_appearance_page_builds_with_a_picker_open_and_closed() {
        // The open flag is view state threaded through the page
        // builder; the page must construct in both states.
        let config = DropTermConfig::default();
        let buffer = SettingsBuffer::default();
        let _ = appearance_view(
            &config,
            &buffer,
            Some(SchemePickerId::Global(ConfigKey::ColorSchemeDark)),
        );
        let _ = appearance_view(&config, &buffer, None);
    }

    #[test]
    fn the_keybindings_tab_builds_with_and_without_matches() {
        // The third tab's view (keybinds-editor criterion 1):
        // construction smoke — every group renders with the empty
        // query, a query nothing matches still builds (the
        // empty-result state), and the armed-capture and conflict
        // states build too. Row inventory and filtering are pinned in
        // `keybinds` (the groups and search tests).
        let _unfiltered = keybindings_view(&BTreeMap::new(), "", None, None);
        let _no_matches =
            keybindings_view(&BTreeMap::new(), "xyzzy-no-action-has-this", None, None);
        let custom = BTreeMap::from([(
            "copy".to_string(),
            crate::keybinds::Binding {
                modifiers: vec![crate::keybinds::ModifierName::Alt],
                key: "C".to_string(),
            },
        )]);
        let _filtered = keybindings_view(&custom, "copy", None, None);
        let _armed = keybindings_view(&custom, "", Some(crate::terminal::Action::Copy), None);
        let _conflict = keybindings_view(
            &custom,
            "",
            None,
            Some((
                crate::terminal::Action::Copy,
                crate::terminal::Action::Paste,
            )),
        );
    }

    // --- the tab strip's model (settings-tabs ST-3) ---

    #[test]
    fn the_strip_model_carries_one_row_per_tab_in_order() {
        // Criterion 7's entity↔tab lockstep, construction side:
        // exactly the five tabs of `SettingsTab::ALL` in strip
        // order, each row labeled with its `tab_header` and carrying
        // its `SettingsTab` as data — the payload the activation
        // handler resolves.
        let mut model = tab_strip_model();
        assert_eq!(model.iter().count(), SettingsTab::ALL.len());
        for (position, tab) in SettingsTab::ALL.iter().enumerate() {
            let entity = model
                .entity_at(u16::try_from(position).expect("five tabs, position 0-4"))
                .expect("one row per tab, in ALL order");
            assert_eq!(model.data::<SettingsTab>(entity), Some(tab));
            assert_eq!(model.text(entity), Some(tab_header(*tab)));
        }
    }

    #[test]
    fn activating_a_tab_row_selects_it_and_only_it() {
        // Criterion 7's lockstep, selection side: `activate_tab_row`
        // moves the single selection to the row carrying the tab —
        // the sync every page mutation applies after assigning
        // `settings_page` (pick, explicit close).
        let mut model = tab_strip_model();
        // The strip's selection only ever moves through
        // `activate_tab_row`: a fresh model selects nothing.
        assert_eq!(model.active_data::<SettingsTab>(), None);
        activate_tab_row(&mut model, SettingsTab::Advanced);
        assert_eq!(
            model.active_data::<SettingsTab>(),
            Some(&SettingsTab::Advanced)
        );
        activate_tab_row(&mut model, SettingsTab::Appearance);
        assert_eq!(
            model.active_data::<SettingsTab>(),
            Some(&SettingsTab::Appearance)
        );
    }

    // --- the Profiles page's view (construction, profile-settings T1) ---

    #[test]
    fn the_profiles_page_builds_in_every_read_state() {
        // Construction is the assertion: empty list (the empty state
        // with its add affordance), a populated list, a stale
        // selection index, and every draft shape — an erroring add
        // draft, a clone draft, an edit draft — must all build
        // without panicking.
        let mut config = DropTermConfig::default();
        let _ = profiles_view(&config, None, None, None, false, true);
        let _ = profiles_view(&config, Some(0), None, None, false, true);
        config.profiles = vec![profile("ssh", "ssh web-prod"), profile("top", "htop")];
        let _ = profiles_view(&config, None, None, None, false, true);
        let _ = profiles_view(&config, Some(1), None, None, false, true);
        let _ = profiles_view(&config, Some(9), None, None, false, true);
        let mut erroring = ProfileDraft::add();
        erroring.errors = vec![(ProfileField::Name, ProfileFieldError::DuplicateName)];
        let _ = profiles_view(&config, None, Some(&erroring), None, false, true);
        let cloning = ProfileDraft::clone_of(&config.profiles[0], &config.profiles);
        let _ = profiles_view(&config, Some(0), Some(&cloning), None, false, true);
        let editing = ProfileDraft::edit(0, &config.profiles[0]);
        let _ = profiles_view(&config, Some(0), Some(&editing), None, false, true);
    }

    #[test]
    fn profile_row_builds_at_the_boundaries_with_the_edit_cluster() {
        // The action cluster — pencil, Clone, the delete cluster, and
        // the two reorder directions with the boundary one disabled —
        // builds for the first and last rows alike, in every cluster
        // state: the idle trash, the armed inline Confirm/Keep pair, and
        // the draft lock (everything disabled); construction is the
        // assertion (a malformed cluster panics here, not on hardware).
        // A name the theme lacks renders blank instead of panicking —
        // that failure belongs to
        // `every_static_icon_name_resolves_in_the_theme_chain` below.
        // The lock-plus-arm combination never reaches the row — the
        // page degrades an armed row to the idle trash while a draft is
        // open — and the page-level pin covers that instead.
        let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
        let last = profiles.len() - 1;
        let _ = profile_row(
            profiles.len(),
            0,
            &profiles[0],
            false,
            true,
            RowDelete::Idle,
            false,
        );
        let _ = profile_row(
            profiles.len(),
            last,
            &profiles[last],
            true,
            false,
            RowDelete::Idle,
            false,
        );
        let _ = profile_row(
            profiles.len(),
            0,
            &profiles[0],
            false,
            true,
            RowDelete::Armed,
            false,
        );
        let _ = profile_row(
            profiles.len(),
            last,
            &profiles[last],
            true,
            false,
            RowDelete::Armed,
            false,
        );
        let _ = profile_row(
            profiles.len(),
            0,
            &profiles[0],
            false,
            true,
            RowDelete::Idle,
            true,
        );
        let _ = profile_row(
            profiles.len(),
            last,
            &profiles[last],
            true,
            false,
            RowDelete::Idle,
            true,
        );
    }

    #[test]
    fn the_form_builds_with_the_save_footer_in_every_draft_shape() {
        // Construction is the assertion: every draft shape — Add,
        // Clone, Edit — and a draft erroring in several fields at once
        // builds the transactional form (footer, per-field errors)
        // without panicking.
        let config = DropTermConfig::default();
        let profiles = [sourced_profile()];
        let _ = profile_form(&config, &ProfileDraft::add(), false, true);
        let _ = profile_form(
            &config,
            &ProfileDraft::clone_of(&profiles[0], &profiles),
            false,
            true,
        );
        let _ = profile_form(&config, &ProfileDraft::edit(0, &profiles[0]), false, true);
        let mut erroring = ProfileDraft::add();
        erroring.errors = vec![
            (ProfileField::Name, ProfileFieldError::BlankName),
            (ProfileField::Command, ProfileFieldError::BlankCommand),
            (ProfileField::EnvKey(0), ProfileFieldError::EnvKeyEquals),
        ];
        erroring.env_rows = vec![env_row("A=B", "v")];
        let _ = profile_form(&config, &erroring, false, true);
    }

    #[test]
    fn the_profiles_page_builds_with_armed_rows_and_a_locked_list() {
        // Construction is the assertion: the page builds with a row
        // armed (the inline Confirm/Keep pair), with the list locked by
        // an open draft (every list control disabled), and with both at
        // once (the lock hides the pair for the moment).
        let config = DropTermConfig {
            profiles: vec![profile("ssh", "ssh web-prod"), profile("top", "htop")],
            ..DropTermConfig::default()
        };
        let _ = profiles_view(&config, None, None, None, false, true);
        let _ = profiles_view(&config, Some(0), None, Some(0), false, true);
        let _ = profiles_view(&config, Some(1), None, Some(1), false, false);
        let draft = ProfileDraft::edit(0, &config.profiles[0]);
        let _ = profiles_view(&config, Some(0), Some(&draft), None, false, true);
        let _ = profiles_view(&config, Some(0), Some(&draft), Some(1), false, true);
        let _ = profiles_view(&config, None, Some(&ProfileDraft::add()), None, true, true);
        // An armed index past the list (the list shrank under it) renders
        // as no armed row.
        let _ = profiles_view(&config, None, None, Some(9), false, true);
    }

    #[test]
    fn the_form_builds_with_the_override_fields_and_the_inherit_picker() {
        // Construction is the assertion: the six-field form builds with
        // the inherit picker open and closed, in both system modes, with
        // a scheme picked and inherited, and with the new fields
        // erroring alongside their peers.
        let config = DropTermConfig {
            font_size: 14,
            scrollback_lines: 10_000,
            ..DropTermConfig::default()
        };
        let profiles = [sourced_profile()];
        let mut picked = ProfileDraft::edit(0, &profiles[0]);
        picked.scheme = Some("Dracula".to_string());
        let _ = profile_form(&config, &ProfileDraft::add(), false, true);
        let _ = profile_form(&config, &picked, true, false);
        let mut erroring = ProfileDraft::add();
        erroring.errors = vec![
            (ProfileField::Scheme, ProfileFieldError::UnknownScheme),
            (ProfileField::FontSize, ProfileFieldError::FontOutOfRange),
            (
                ProfileField::Scrollback,
                ProfileFieldError::UnparsableScrollback,
            ),
        ];
        let _ = profile_form(&config, &erroring, false, true);
    }
}
