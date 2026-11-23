// SPDX-License-Identifier: GPL-3.0-only

//! The command palette on GPUI (command-palette spec): core's
//! [`dropterm::core::palette`] model presented as the close-confirm's
//! in-surface modal — a full-surface scrim whose press dismisses,
//! carrying a top-centered panel over the live terminal, the Kit
//! `List`'s own escape/enter/up/down key context as the innards, the
//! List's own search input holding the keyboard (the pane's claim
//! defers: typing filters the catalog through core's scorer and never
//! reaches the pane). Every pick closes the palette first, then
//! lands in the funnel its keybind or menu twin already drives —
//! [`route`] is the bin-side Verb→funnel table, `bin/context_menu`
//! .rs's discipline: exhaustive over core's `Verb`, so a new variant
//! is a compile error away from an unrouted row. The palette is
//! in-window chrome, so it composes identically in layer-shell mode
//! and the GNOME plain-window mode.

use std::rc::Rc;

use dropterm::core::keybinds::Binding;
use dropterm::core::palette::{Command, Section, Verb};
use dropterm::core::profile::Profile;
use dropterm::core::settings::SettingsTab;
use dropterm::core::tabs::{CloseSweep, select_tab_target};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    Keystroke, Modifiers, MouseButton, ParentElement as _, Render, Styled as _, div, px, rgba,
};
use gpui_component::kbd::Kbd;
use gpui_component::list::{List, ListDelegate, ListItem, ListState};
use gpui_component::{ActiveTheme as _, IndexPath};

use crate::app_shell::DismissPalette;
use crate::terminal::input::{
    Chord, TermAction, chord_matches, effective_key_binds, gpui_key_name,
};

// --- the routing table (the bin-side Verb→funnel half) ---

/// One route's landing spot: the funnel its keybind or menu twin
/// drives. `Pane`/`ShellVerb` split exactly on the keybind map's own
/// gate (`TermAction::is_shell_verb`), so a registry pick rides the
/// same path its chord produces — the context menu's rule verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Route {
    /// The active pane's own action funnel (`perform_term_action`).
    Pane(TermAction),
    /// A shell verb through `shell_verb` — the keyboard's own
    /// handoff, the tab menu's shell rows' funnel.
    ShellVerb(TermAction),
    /// The tab menu's close sweep, keyed on the active tab through
    /// the gated front door.
    CloseSweep(CloseSweep),
    /// The context menu's profile launch, by config index.
    NewProfileTab(usize),
    /// The context menu's active-tab respawn, by config index.
    SwitchProfile(usize),
    /// The zero-based fleet index — resolved from the one-based verb
    /// through `select_tab_target`, the stale pick's boundary.
    SelectTab(usize),
    /// The settings drawer at its page — the palette's open-at-page
    /// seam (`open_settings_at`).
    OpenSettings(SettingsTab),
    /// The named scheme applied live to the picker slot matching the
    /// resolved theme mode — the drawer picker's own write.
    ApplyScheme(String),
}

/// The Verb→funnel table. `None` is a swallowed no-op by design: a
/// registry id nothing owns (a stale `extra` row), a profile name the
/// config no longer carries, a tab index past the fleet — every
/// dynamic family's staleness dies here, exactly the keybind map's
/// unknown-id rule and the profile pick's stale-index warn.
pub(crate) fn route(verb: Verb, profiles: &[Profile], tab_len: usize) -> Option<Route> {
    match verb {
        Verb::Action(id) => {
            let action = crate::terminal::input::action_from_id(id)?;
            Some(if action.is_shell_verb() {
                Route::ShellVerb(action)
            } else {
                Route::Pane(action)
            })
        }
        Verb::CloseOtherTabs => Some(Route::CloseSweep(CloseSweep::Others)),
        Verb::CloseTabsRight => Some(Route::CloseSweep(CloseSweep::Right)),
        Verb::NewProfileTab(name) => profiles
            .iter()
            .position(|profile| profile.name == name)
            .map(Route::NewProfileTab),
        Verb::SwitchProfile(name) => profiles
            .iter()
            .position(|profile| profile.name == name)
            .map(Route::SwitchProfile),
        Verb::SelectTab(one_based) => select_tab_target(tab_len, one_based).map(Route::SelectTab),
        Verb::OpenSettings(page) => Some(Route::OpenSettings(page)),
        Verb::ApplyScheme(name) => Some(Route::ApplyScheme(name)),
    }
}

// --- the surface (the close-confirm's shape, the Kit List's innards) ---

/// The chord that dismisses the palette: the one that opens it. The
/// palette holds the keyboard while it stands, so the open chord never
/// reaches the pane's key listener — the surface answers the chord
/// itself, and it answers to exactly the press the pane's own
/// effective map does, a `custom_key_binds` override included. `None`
/// only if the registry lacked the row.
pub(crate) fn toggle_chord(
    custom_binds: &std::collections::BTreeMap<String, Binding>,
) -> Option<Chord> {
    effective_key_binds(custom_binds)
        .into_iter()
        .find_map(|(chord, action)| (action == TermAction::CommandPalette).then_some(chord))
}

/// The palette's pick callback: the row's command, with the window
/// and app the List's confirm hands. The shell's own closure closes
/// the palette, then routes — the context-menu `PickFn`'s shape.
pub(crate) type PickFn = Rc<dyn Fn(Command, &mut gpui::Window, &mut App)>;
/// The palette's dismissal callback: Esc (the List's Cancel or the
/// container's bound action), the scrim's press, the toggle chord.
pub(crate) type DismissFn = Rc<dyn Fn(&mut gpui::Window, &mut App)>;

/// The row's chord chip keystroke: the first chord of the display
/// form ("Ctrl + Shift + O", or the first of a joined default set)
/// through the pane's own persisted-name mapping — one spelling
/// authority per persisted name. `None` renders no chip.
fn chord_keystroke(chord: &str) -> Option<Keystroke> {
    let first = chord.split(" / ").next()?;
    let mut modifiers = Modifiers::default();
    let mut key = None;
    for part in first.split(" + ") {
        match part {
            "Ctrl" => modifiers.control = true,
            "Shift" => modifiers.shift = true,
            "Alt" => modifiers.alt = true,
            "Super" => modifiers.platform = true,
            name => key = Some(gpui_key_name(name)),
        }
    }
    Some(Keystroke {
        modifiers,
        key: key?,
        key_char: None,
    })
}

/// The List's delegate: the composed catalog in two shapes. The
/// empty query renders it sectioned by contiguous [`Section`] runs
/// (core's composition guarantees contiguity, pinned); a query
/// renders one flat list of the scored rows, score order, no headers.
/// Which rows show is a function of the query alone — the same query
/// gives the same rows, Backspace and re-type alike — so no
/// incremental state can drift.
struct PaletteDelegate {
    catalog: Vec<Command>,
    /// One `(section, catalog bounds)` pair per contiguous run, in
    /// composition order — the sectioned shape's IndexPath→row table.
    sections: Vec<(Section, std::ops::Range<usize>)>,
    /// The query, trimmed; empty selects the sectioned shape.
    query: String,
    /// The scored shape: catalog indices in score order for `query`.
    hits: Vec<usize>,
    selected: Option<IndexPath>,
    on_pick: PickFn,
    on_dismiss: DismissFn,
}

impl PaletteDelegate {
    fn from_catalog(catalog: Vec<Command>, on_pick: PickFn, on_dismiss: DismissFn) -> Self {
        let mut sections: Vec<(Section, std::ops::Range<usize>)> = Vec::new();
        for (index, command) in catalog.iter().enumerate() {
            match sections.last_mut() {
                Some((section, bounds)) if *section == command.section => bounds.end = index + 1,
                _ => sections.push((command.section, index..index + 1)),
            }
        }
        Self {
            catalog,
            sections,
            query: String::new(),
            hits: Vec::new(),
            selected: None,
            on_pick,
            on_dismiss,
        }
    }

    /// Whether the list is in its flat, scored shape.
    fn is_scored(&self) -> bool {
        !self.query.is_empty()
    }

    /// Scores `query` over the catalog through core's scorer: the
    /// hits in score order, registry order on ties. A blank query
    /// selects the sectioned shape.
    fn apply_query(&mut self, query: &str) {
        let query = query.trim();
        self.hits.clear();
        if !query.is_empty() {
            self.hits.extend(
                dropterm::core::palette::score(query, &self.catalog)
                    .into_iter()
                    .map(|(index, _)| index),
            );
        }
        self.query.clear();
        self.query.push_str(query);
    }

    /// Where the selection sits after a query change: the first
    /// scored row, or none on the sectioned catalog and on a query
    /// with no hits. A function of the query — never of the rows that
    /// showed before it, which is what the Kit's own reset reads.
    fn reset_selection(&self) -> Option<IndexPath> {
        (!self.hits.is_empty()).then_some(IndexPath::default())
    }

    /// The catalog index at a list position in the current shape —
    /// the row's id, unique across both.
    fn catalog_index(&self, ix: IndexPath) -> Option<usize> {
        if self.is_scored() {
            (ix.section == 0)
                .then(|| self.hits.get(ix.row))
                .flatten()
                .copied()
        } else {
            self.sections
                .get(ix.section)
                .map(|(_, bounds)| bounds.start + ix.row)
        }
    }

    /// The list's section count in the current shape.
    fn section_count(&self) -> usize {
        if self.is_scored() {
            1
        } else {
            self.sections.len().max(1)
        }
    }

    /// One section's row count in the current shape.
    fn row_count(&self, section: usize) -> usize {
        if self.is_scored() {
            self.hits.len()
        } else {
            self.sections
                .get(section)
                .map_or(0, |(_, bounds)| bounds.len())
        }
    }

    fn command_at(&self, ix: IndexPath) -> Option<&Command> {
        self.catalog.get(self.catalog_index(ix)?)
    }
}

impl ListDelegate for PaletteDelegate {
    type Item = ListItem;

    fn sections_count(&self, _cx: &App) -> usize {
        self.section_count()
    }

    fn items_count(&self, section: usize, _cx: &App) -> usize {
        self.row_count(section)
    }

    /// The Kit's hook on every query change — the search half's one
    /// seam: the Kit renders the input and the list, core's scorer
    /// decides the rows.
    fn perform_search(
        &mut self,
        query: &str,
        window: &mut gpui::Window,
        cx: &mut Context<ListState<Self>>,
    ) -> gpui::Task<()> {
        self.apply_query(query);
        // The Kit repaints the list only through its own selection
        // reset, which skips its empty branch; every query change
        // repaints here.
        cx.notify();
        // The Kit's reset lands on the first row only when the rows
        // before the change were non-empty. It runs after this
        // returns, so the query's own value is deferred behind it.
        let reset = self.reset_selection();
        cx.defer_in(window, move |list, window, cx| {
            list.set_selected_index(reset, window, cx);
        });
        gpui::Task::ready(())
    }

    fn render_section_header(
        &mut self,
        section: usize,
        _window: &mut gpui::Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        // The scored shape is one flat list: no headers while querying.
        if self.is_scored() {
            return None;
        }
        let (section, _) = self.sections.get(section)?;
        Some(
            div()
                .h(px(28.))
                .px_2()
                .flex()
                .items_center()
                .text_size(px(11.))
                .text_color(cx.theme().muted_foreground)
                .child(section.title()),
        )
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut gpui::Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let command = self.command_at(ix)?.clone();
        // The chord chip: the Kit's Kbd over the row's effective
        // chord; the unbound rows render none.
        let chip = command
            .chord
            .as_deref()
            .and_then(chord_keystroke)
            .map(|stroke| Kbd::new(stroke).into_any_element());
        // The row id is the catalog index — unique across every
        // section and every scored position, the ElementId's own
        // usize form.
        Some(
            ListItem::new(self.catalog_index(ix)?)
                .h(px(32.))
                .px_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .flex_1()
                        .overflow_hidden()
                        .child(
                            div()
                                .text_size(px(13.))
                                .truncate()
                                .child(command.label.clone()),
                        )
                        .when_some(chip, gpui::ParentElement::child),
                ),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut gpui::Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix;
    }

    /// Enter, or a row's click — the List's own confirm (the Kit
    /// handles both). The pick defers out of the List's dispatch so
    /// the shell's funnel runs at the effect cycle's end, the select
    /// popover's own `on_cancel` shape.
    fn confirm(
        &mut self,
        _secondary: bool,
        window: &mut gpui::Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let Some(ix) = self.selected else {
            return;
        };
        let Some(command) = self.command_at(ix).cloned() else {
            return;
        };
        let pick = Rc::clone(&self.on_pick);
        cx.defer_in(window, move |_, window, cx| pick(command, window, cx));
    }

    /// Escape — the List's own Cancel binding; the dismissal rides
    /// the shell's close. The Kit's handler propagates before it calls
    /// the delegate, so the key is kept here: the palette's own
    /// Escape binding sits above on the same path, and unkept it would
    /// close a second time.
    fn cancel(&mut self, window: &mut gpui::Window, cx: &mut Context<ListState<Self>>) {
        cx.stop_propagation();
        let dismiss = Rc::clone(&self.on_dismiss);
        dismiss(window, cx);
    }
}

/// The open palette: the modal entity the shell holds while it
/// stands — the Kit List's own search input holding the keyboard,
/// the blur dismissal with the `Select::on_blur` containment shape,
/// and the top-centered panel over the full-surface scrim.
pub(crate) struct Palette {
    list: Entity<ListState<PaletteDelegate>>,
    /// The palette's own container handle: the keyboard rests here
    /// only under a press on the panel's own chrome.
    container_focus: gpui::FocusHandle,
    /// The search input's handle — the keyboard's home while the
    /// palette stands.
    input_focus: gpui::FocusHandle,
    /// The List's own handle: a press on the list's chrome (a row,
    /// the scrollbar) moves the keyboard here.
    list_focus: gpui::FocusHandle,
    /// The blur dismissals' subscriptions — held, never read: dropping
    /// the palette drops them (the status menu's own shape).
    _blur: [gpui::Subscription; 2],
    /// The input's refocus subscription — held, never read, as above.
    _refocus: gpui::Subscription,
    /// The first paint's evidence line, once per open — the walk's
    /// ground truth that the overlay's render actually runs.
    painted_once: bool,
    /// The chord that opened the palette — the toggle's key.
    toggle: Option<Chord>,
    on_dismiss: DismissFn,
}

impl Palette {
    /// The palette over a fresh catalog. The shell hands in the two
    /// callbacks every path ends in — the pick and the dismissal —
    /// so the surface holds no shell reference and the same
    /// construction mounts it headless.
    pub(crate) fn new(
        catalog: Vec<Command>,
        toggle: Option<Chord>,
        on_pick: PickFn,
        on_dismiss: DismissFn,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let delegate = PaletteDelegate::from_catalog(catalog, on_pick, Rc::clone(&on_dismiss));
        let list = cx.new(|cx| ListState::new(delegate, window, cx));
        // The List's own handle, read while the list is still
        // unsearchable: once it is, the List's focus API hands out
        // the search input's handle instead (carried list.rs,
        // `Focusable for ListState`).
        let list_focus = list.read(cx).focus_handle(cx).clone();
        list.update(cx, |list, cx| list.set_searchable(true, cx));
        let input_focus = list.read(cx).focus_handle(cx);
        let container_focus = cx.focus_handle();
        // The blur dismissal: either handle losing the keyboard to
        // anything outside the palette dismisses it — with the
        // Select's containment shape, a move that stays inside (the
        // input and the list passing it between them, the container's
        // own handle holding it) does not.
        let blur_input = cx.on_blur(&input_focus, window, Self::blur_dismissal);
        let blur_list = cx.on_blur(&list_focus, window, Self::blur_dismissal);
        // The keyboard comes back to the input whenever the list's
        // chrome takes it, so typing always filters.
        let refocus = cx.on_focus(&list_focus, window, |this, window, cx| {
            this.input_focus.focus(window, cx);
        });
        Self {
            list,
            container_focus,
            input_focus,
            list_focus,
            _blur: [blur_input, blur_list],
            _refocus: refocus,
            painted_once: false,
            toggle,
            on_dismiss,
        }
    }

    /// The blur dismissal's listener.
    fn blur_dismissal(this: &mut Self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        if this.holds_keyboard(window) {
            return;
        }
        (this.on_dismiss)(window, cx);
    }

    /// Whether the keyboard sits on one of the palette's own handles:
    /// the input, the list, or the container. A blur that lands here
    /// is a move inside the palette, never a dismissal.
    fn holds_keyboard(&self, window: &gpui::Window) -> bool {
        self.input_focus.is_focused(window)
            || self.list_focus.is_focused(window)
            || self.container_focus.is_focused(window)
    }

    /// The keyboard's hand to the palette: the search input takes it
    /// (the Kit's own `focus` lands on the input of a searchable list).
    pub(crate) fn focus(&self, window: &mut gpui::Window, cx: &mut App) {
        self.list.update(cx, |list, cx| list.focus(window, cx));
    }

    /// The search input's focus handle — the shell's render-time
    /// repair reads it to hand the keyboard back to the active pane
    /// once a dismissed palette's handle dangles.
    pub(crate) fn focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        self.list.read(cx).focus_handle(cx)
    }
}

impl Render for Palette {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let card = theme.background;
        let hairline = theme.border;
        let fg = theme.foreground;
        // The keyboard's first-paint repair (the close-confirm's own
        // windowless-arm pattern): focus applied at open races the
        // element's first mount, and a lost race strands the surface
        // chord-dead under an invisible palette. The first frame
        // that carries the list takes the keyboard.
        if !self.painted_once && !self.holds_keyboard(window) {
            println!("dropterm: palette focus repaired on first paint");
            self.input_focus.focus(window, cx);
        }
        if !self.painted_once {
            self.painted_once = true;
            println!("dropterm: palette overlay painted");
        }
        let dismiss = Rc::clone(&self.on_dismiss);
        // The overlay's root is out-of-flow, the settings drawer's
        // and close-confirm's own rule: an in-flow full-size sibling
        // under the band lays out past the window's visible area —
        // mounted, focusable, hit-testable, and nowhere on screen
        // (the invisible open this fixes). Absolute against the
        // shell's positioned overlay host puts the layer on top.
        div()
            .absolute()
            .inset_0()
            .bg(rgba(0x0000_0080))
            .flex()
            .justify_center()
            .items_start()
            // The palette's key context: the container's own Escape,
            // for a keyboard resting on the container. While the list
            // holds it, the List's own Cancel is deeper in the context
            // stack, closes, and keeps the key — one dismissal.
            .key_context("palette")
            .track_focus(&self.container_focus)
            .on_action(move |_: &DismissPalette, window, cx| dismiss(window, cx))
            // The toggle: the palette holds the keyboard while it
            // stands, so the open chord never reaches the pane's key
            // map — the root answers it itself, through the pane's
            // own matcher, and keeps it (an owned chord never leaves
            // its surface).
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                let owned = this
                    .toggle
                    .as_ref()
                    .is_some_and(|chord| chord_matches(chord, &event.keystroke));
                if !owned {
                    return;
                }
                cx.stop_propagation();
                (this.on_dismiss)(window, cx);
            }))
            // The scrim's press dismisses (the panel's occlusion
            // keeps its own presses — the List's rows included —
            // off this listener).
            .on_mouse_down(MouseButton::Left, {
                let dismiss = Rc::clone(&self.on_dismiss);
                move |_, window, cx| dismiss(window, cx)
            })
            .child(
                div()
                    .occlude()
                    .mt(px(48.))
                    .w(px(520.))
                    .h(px(360.))
                    .rounded_md()
                    .border_1()
                    .border_color(hairline)
                    .bg(card)
                    .text_color(fg)
                    .overflow_hidden()
                    .child(List::new(&self.list).size_full()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dropterm::core::keybinds::ModifierName;
    use std::cell::RefCell;

    fn profile(name: &str) -> Profile {
        Profile {
            name: name.to_string(),
            command: format!("run {name}"),
            cwd: None,
            color_scheme: None,
            env: None,
            font_size: None,
            scrollback_lines: None,
        }
    }

    fn profiles() -> Vec<Profile> {
        vec![profile("ssh-web"), profile("htop")]
    }

    fn no_binds() -> std::collections::BTreeMap<String, Binding> {
        std::collections::BTreeMap::default()
    }

    /// `command_palette` rebound to Alt+P — a chord no default holds.
    fn alt_p_palette() -> std::collections::BTreeMap<String, Binding> {
        std::collections::BTreeMap::from([(
            "command_palette".to_string(),
            Binding {
                modifiers: vec![ModifierName::Alt],
                key: "P".to_string(),
            },
        )])
    }

    /// The surface pins' catalog: the registry, two profiles, and two
    /// schemes over the given binds — `drac` finds Dracula here.
    fn surface_catalog(custom_binds: &std::collections::BTreeMap<String, Binding>) -> Vec<Command> {
        dropterm::core::palette::compose(
            custom_binds,
            &profiles(),
            &[],
            &["Dracula".to_string(), "Nord".to_string()],
            &[],
        )
    }

    #[test]
    fn registry_ids_split_exactly_on_the_keybind_gate() {
        // The pane-owned verbs land in the pane's own funnel — the
        // exact TermAction their chords produce.
        assert_eq!(
            route(Verb::Action("copy"), &[], 0),
            Some(Route::Pane(TermAction::Copy))
        );
        assert_eq!(
            route(Verb::Action("paste_primary"), &[], 0),
            Some(Route::Pane(TermAction::PastePrimary))
        );
        // The shell family lands in the keyboard's own handoff.
        assert_eq!(
            route(Verb::Action("new_tab"), &[], 0),
            Some(Route::ShellVerb(TermAction::NewTab))
        );
        assert_eq!(
            route(Verb::Action("settings"), &[], 0),
            Some(Route::ShellVerb(TermAction::Settings))
        );
        // The numbered seats ride their parse-back.
        assert_eq!(
            route(Verb::Action("activate_tab_3"), &[], 5),
            Some(Route::ShellVerb(TermAction::ActivateTab(3)))
        );
    }

    #[test]
    fn an_unknown_registry_id_swallows() {
        assert_eq!(route(Verb::Action("no_such_row"), &[], 0), None);
    }

    #[test]
    fn the_close_verbs_route_through_the_tab_menu_s_sweep() {
        assert_eq!(
            route(Verb::CloseOtherTabs, &[], 0),
            Some(Route::CloseSweep(CloseSweep::Others))
        );
        assert_eq!(
            route(Verb::CloseTabsRight, &[], 0),
            Some(Route::CloseSweep(CloseSweep::Right))
        );
    }

    #[test]
    fn profile_rows_resolve_by_name_to_the_config_index() {
        assert_eq!(
            route(Verb::NewProfileTab("htop".to_string()), &profiles(), 0),
            Some(Route::NewProfileTab(1))
        );
        assert_eq!(
            route(Verb::SwitchProfile("ssh-web".to_string()), &profiles(), 0),
            Some(Route::SwitchProfile(0))
        );
    }

    #[test]
    fn a_stale_profile_name_swallows() {
        assert_eq!(
            route(Verb::NewProfileTab("ghost".to_string()), &profiles(), 0),
            None
        );
        assert_eq!(
            route(Verb::SwitchProfile("ghost".to_string()), &profiles(), 0),
            None
        );
    }

    #[test]
    fn tab_rows_resolve_one_based_through_the_select_funnel() {
        // One-based in, zero-based out — `select_tab_target`'s own
        // boundary conversion.
        assert_eq!(route(Verb::SelectTab(1), &[], 3), Some(Route::SelectTab(0)));
        assert_eq!(route(Verb::SelectTab(3), &[], 3), Some(Route::SelectTab(2)));
    }

    #[test]
    fn a_dead_tab_index_swallows() {
        assert_eq!(route(Verb::SelectTab(4), &[], 3), None);
        assert_eq!(route(Verb::SelectTab(0), &[], 3), None);
        assert_eq!(route(Verb::SelectTab(1), &[], 0), None);
    }

    #[test]
    fn settings_and_scheme_rows_route_by_name() {
        assert_eq!(
            route(Verb::OpenSettings(SettingsTab::Keybindings), &[], 0),
            Some(Route::OpenSettings(SettingsTab::Keybindings))
        );
        assert_eq!(
            route(Verb::ApplyScheme("Dracula".to_string()), &[], 0),
            Some(Route::ApplyScheme("Dracula".to_string()))
        );
    }

    // --- the chord chip's keystroke parse ---

    #[test]
    fn the_chord_chip_parses_the_display_form() {
        let stroke = chord_keystroke("Ctrl + Shift + O").expect("a bound row parses");
        assert!(stroke.modifiers.control && stroke.modifiers.shift);
        assert_eq!(stroke.key, "o");
        // Named keys ride the pane's own spelling table.
        let stroke = chord_keystroke("Ctrl + Shift + End").expect("a named key parses");
        assert_eq!(stroke.key, "end");
        // A joined default set chips its first chord.
        let stroke = chord_keystroke("Ctrl + PageDown / Ctrl + Tab").expect("a joined set parses");
        assert_eq!(stroke.key, "pagedown");
        // The bare-letter and modifier-less forms parse.
        let stroke = chord_keystroke("Insert").expect("a bare key parses");
        assert_eq!(stroke.key, "insert");
        assert!(!stroke.modifiers.control && !stroke.modifiers.shift);
    }

    // --- the surface, mounted headless ---

    /// The real palette as a test window's root with its two
    /// callbacks wired to a log — the images panel's probe shape. The
    /// log records `"dismiss"` and `"pick <id>"` in call order.
    ///
    /// Element blur listeners do not fire in this harness: gpui gives
    /// a focus event its paths only while the window is active, and
    /// the test window never is. The blur dismissal stays an
    /// owner-walk row.
    #[allow(clippy::type_complexity)]
    fn mount(
        cx: &mut gpui::TestAppContext,
        custom_binds: &std::collections::BTreeMap<String, Binding>,
    ) -> (
        gpui::VisualTestContext,
        Entity<Palette>,
        Rc<RefCell<Vec<String>>>,
    ) {
        cx.update(gpui_component::init);
        // The boot's palette binding: the container's own Escape.
        cx.update(|cx| {
            cx.bind_keys([gpui::KeyBinding::new(
                "escape",
                DismissPalette,
                Some("palette"),
            )]);
        });
        let log = Rc::new(RefCell::new(Vec::new()));
        let pick_log = Rc::clone(&log);
        let dismiss_log = Rc::clone(&log);
        let handle = cx
            .update(|cx| {
                cx.open_window(
                    gpui::WindowOptions {
                        window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds {
                            origin: gpui::Point::default(),
                            size: gpui::size(px(1000.), px(800.)),
                        })),
                        ..Default::default()
                    },
                    |window, cx| {
                        cx.new(|cx| {
                            Palette::new(
                                surface_catalog(custom_binds),
                                toggle_chord(custom_binds),
                                Rc::new(move |command, _window, _cx| {
                                    pick_log.borrow_mut().push(format!("pick {}", command.id));
                                }),
                                Rc::new(move |_window, _cx| {
                                    dismiss_log.borrow_mut().push("dismiss".to_string());
                                }),
                                window,
                                cx,
                            )
                        })
                    },
                )
            })
            .expect("open the palette's window");
        let mut cx = gpui::VisualTestContext::from_window(handle.into(), cx);
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let palette: Entity<Palette> = cx.update(|window, _| {
            window
                .root()
                .expect("the palette is the window's root type")
                .expect("the window has a root")
        });
        cx.update(|window, cx| palette.update(cx, |palette, cx| palette.focus(window, cx)));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        (cx, palette, log)
    }

    #[gpui::test]
    fn the_open_palette_hands_the_keyboard_to_its_search(cx: &mut gpui::TestAppContext) {
        let (mut cx, palette, log) = mount(cx, &no_binds());
        assert!(
            cx.update(|window, cx| palette.read(cx).focus_handle(cx).is_focused(window)),
            "the search input holds the keyboard at open"
        );
        assert!(log.borrow().is_empty(), "opening dismisses nothing");
    }

    #[gpui::test]
    fn the_palette_s_three_handles_count_as_its_own_keyboard(cx: &mut gpui::TestAppContext) {
        let (mut cx, palette, log) = mount(cx, &no_binds());
        let (input, list, container, outside) = cx.update(|_, cx| {
            let palette = palette.read(cx);
            (
                palette.input_focus.clone(),
                palette.list_focus.clone(),
                palette.container_focus.clone(),
                cx.focus_handle(),
            )
        });
        for (name, handle) in [("input", input), ("list", list), ("container", container)] {
            cx.update(|window, cx| handle.focus(window, cx));
            assert!(
                cx.update(|window, cx| palette.read(cx).holds_keyboard(window)),
                "the keyboard on the palette's {name} handle is inside the palette"
            );
        }
        cx.update(|window, cx| outside.focus(window, cx));
        assert!(
            !cx.update(|window, cx| palette.read(cx).holds_keyboard(window)),
            "the keyboard anywhere else is outside"
        );
        assert!(log.borrow().is_empty(), "moving inside dismisses nothing");
    }

    #[gpui::test]
    fn a_scrim_press_dismisses(cx: &mut gpui::TestAppContext) {
        let (mut cx, _palette, log) = mount(cx, &no_binds());
        // Bare scrim: below the top-centered panel (48px down, 360
        // tall in the 800px window).
        cx.simulate_click(gpui::point(px(20.), px(700.)), gpui::Modifiers::default());
        assert_eq!(*log.borrow(), ["dismiss"], "the scrim's press dismisses");
    }

    #[gpui::test]
    fn a_row_click_picks(cx: &mut gpui::TestAppContext) {
        let first = surface_catalog(&no_binds())[0].id.clone();
        let (mut cx, _palette, log) = mount(cx, &no_binds());
        // The first row's center: the panel 48px down, the Kit's
        // 32px search input, the first section header 28px, half of a
        // 32px row.
        cx.simulate_click(gpui::point(px(500.), px(124.)), gpui::Modifiers::default());
        cx.run_until_parked();
        // The surface hands the shell its pick callback; closing
        // first is that callback's own act (app_shell.rs).
        assert_eq!(
            *log.borrow(),
            [format!("pick {first}").as_str()],
            "a row's click picks it"
        );
    }

    #[gpui::test]
    fn enter_picks_the_selected_row(cx: &mut gpui::TestAppContext) {
        let first = surface_catalog(&no_binds())[0].id.clone();
        let (mut cx, _palette, log) = mount(cx, &no_binds());
        cx.simulate_keystrokes("down");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(
            *log.borrow(),
            [format!("pick {first}").as_str()],
            "Down selects the first row, Enter picks it"
        );
    }

    #[gpui::test]
    fn escape_dismisses_once(cx: &mut gpui::TestAppContext) {
        let (mut cx, _palette, log) = mount(cx, &no_binds());
        cx.simulate_keystrokes("escape");
        assert_eq!(
            *log.borrow(),
            ["dismiss"],
            "Escape closes the palette through the List's own Cancel, once"
        );
    }

    // --- the search half, on the surface ---

    /// The mounted list after the test's keystrokes: whether it is in
    /// its scored shape, the top row's label, the shown row count,
    /// and the selection.
    #[allow(clippy::type_complexity)]
    fn shown_state(
        cx: &mut gpui::VisualTestContext,
        palette: &Entity<Palette>,
    ) -> (bool, Option<String>, usize, Option<IndexPath>) {
        cx.update(|_, cx| {
            let list = palette.read(cx).list.read(cx);
            let delegate = list.delegate();
            (
                delegate.is_scored(),
                labels(delegate).first().cloned(),
                labels(delegate).len(),
                list.selected_index(),
            )
        })
    }

    #[gpui::test]
    fn typing_filters_the_list_through_the_core_scorer(cx: &mut gpui::TestAppContext) {
        let (mut cx, palette, _log) = mount(cx, &no_binds());
        let catalog_len = surface_catalog(&no_binds()).len();
        let (_, _, rows, _) = shown_state(&mut cx, &palette);
        assert_eq!(rows, catalog_len, "the catalog shows whole at open");

        cx.simulate_keystrokes("s c r o l l space c l e a r");
        cx.run_until_parked();
        let (scored, top, rows, _) = shown_state(&mut cx, &palette);
        assert!(scored, "a query puts the list in its scored shape");
        assert_eq!(
            top.as_deref(),
            Some("Clear scrollback"),
            "the top score leads"
        );
        assert!(rows < catalog_len, "the non-matching rows are gone");
    }

    #[gpui::test]
    fn enter_picks_the_top_scored_row(cx: &mut gpui::TestAppContext) {
        let (mut cx, _palette, log) = mount(cx, &no_binds());
        cx.simulate_keystrokes("s c r o l l space c l e a r");
        cx.run_until_parked();
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(
            *log.borrow(),
            ["pick clear_scrollback"],
            "Enter runs the selected scored row"
        );
    }

    #[gpui::test]
    fn the_selection_resets_on_every_query_change(cx: &mut gpui::TestAppContext) {
        let (mut cx, palette, _log) = mount(cx, &no_binds());
        cx.simulate_keystrokes("d");
        cx.run_until_parked();
        let (_, _, _, selected) = shown_state(&mut cx, &palette);
        assert_eq!(
            selected,
            Some(IndexPath::default()),
            "a query starts on its top row"
        );

        cx.simulate_keystrokes("down");
        let (_, _, _, selected) = shown_state(&mut cx, &palette);
        assert_eq!(
            selected.map(|ix| ix.row),
            Some(1),
            "Down moves off the top row"
        );

        cx.simulate_keystrokes("r");
        cx.run_until_parked();
        let (_, _, _, selected) = shown_state(&mut cx, &palette);
        assert_eq!(
            selected,
            Some(IndexPath::default()),
            "the next letter is a new query: back to the top row"
        );
    }

    #[gpui::test]
    fn clearing_the_query_restores_the_sectioned_catalog(cx: &mut gpui::TestAppContext) {
        let (mut cx, palette, _log) = mount(cx, &no_binds());
        let catalog_len = surface_catalog(&no_binds()).len();

        cx.simulate_keystrokes("d r a c");
        cx.run_until_parked();
        let (scored, top, rows, _) = shown_state(&mut cx, &palette);
        assert!(scored);
        assert_eq!(top.as_deref(), Some("Scheme: Dracula"));
        assert_eq!(rows, 1);

        for _ in 0..4 {
            cx.simulate_keystrokes("backspace");
        }
        cx.run_until_parked();
        let (scored, _, rows, selected) = shown_state(&mut cx, &palette);
        assert!(!scored, "the blank query is the sectioned shape again");
        assert_eq!(rows, catalog_len, "the whole catalog is back");
        assert_eq!(selected, None, "and nothing is selected, as at open");
    }

    #[gpui::test]
    fn up_and_down_move_the_selection_from_the_input(cx: &mut gpui::TestAppContext) {
        let (mut cx, palette, _log) = mount(cx, &no_binds());
        cx.simulate_keystrokes("down");
        cx.simulate_keystrokes("down");
        let (_, _, _, selected) = shown_state(&mut cx, &palette);
        assert_eq!(
            selected.map(|ix| ix.row),
            Some(1),
            "Down twice reaches the second row"
        );
        cx.simulate_keystrokes("up");
        let (_, _, _, selected) = shown_state(&mut cx, &palette);
        assert_eq!(selected.map(|ix| ix.row), Some(0), "Up returns");
    }

    #[gpui::test]
    fn escape_from_the_input_dismisses_once(cx: &mut gpui::TestAppContext) {
        let (mut cx, _palette, log) = mount(cx, &no_binds());
        cx.simulate_keystrokes("x");
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        assert_eq!(
            *log.borrow(),
            ["dismiss"],
            "Escape leaves the input, closes the palette, and closes it once"
        );
    }

    // --- the search half: the delegate's two shapes ---

    /// A delegate over a catalog with no callbacks — the shape pins
    /// need no window.
    fn plain_delegate(catalog: Vec<Command>) -> PaletteDelegate {
        PaletteDelegate::from_catalog(
            catalog,
            Rc::new(|_: Command, _: &mut gpui::Window, _: &mut App| {}),
            Rc::new(|_: &mut gpui::Window, _: &mut App| {}),
        )
    }

    /// Every shown row's label, top to bottom — the pins' flattened
    /// view of either shape.
    fn labels(delegate: &PaletteDelegate) -> Vec<String> {
        (0..delegate.section_count())
            .flat_map(|section| 0..delegate.row_count(section))
            .filter_map(|row| delegate.catalog_index(IndexPath::new(row)))
            .map(|index| delegate.catalog[index].label.clone())
            .collect()
    }

    #[test]
    fn a_query_flattens_the_catalog_in_score_order() {
        let mut delegate = plain_delegate(surface_catalog(&no_binds()));
        delegate.apply_query("scroll clear");
        assert!(delegate.is_scored(), "a query selects the scored shape");
        assert_eq!(
            delegate.section_count(),
            1,
            "one flat list, no sections while querying"
        );
        assert_eq!(
            labels(&delegate)[0],
            "Clear scrollback",
            "the top scored row leads the flat list"
        );
        assert!(
            labels(&delegate).len() < surface_catalog(&no_binds()).len(),
            "the non-matching rows are gone"
        );
    }

    #[test]
    fn a_typo_still_matches() {
        let mut delegate = plain_delegate(surface_catalog(&no_binds()));
        delegate.apply_query("scrllback");
        assert_eq!(
            labels(&delegate)[0],
            "Clear scrollback",
            "a dropped letter still finds its row"
        );
    }

    #[test]
    fn a_query_with_no_hits_leaves_an_empty_list() {
        let mut delegate = plain_delegate(surface_catalog(&no_binds()));
        delegate.apply_query("zzzq");
        assert!(delegate.is_scored());
        assert_eq!(delegate.row_count(0), 0, "no row matches");
        assert_eq!(
            delegate.reset_selection(),
            None,
            "nothing to select on an empty list"
        );
    }

    #[test]
    fn the_empty_query_restores_the_sectioned_catalog() {
        let catalog = surface_catalog(&no_binds());
        let sectioned = labels(&plain_delegate(catalog.clone()));
        let mut delegate = plain_delegate(catalog);
        delegate.apply_query("drac");
        assert_eq!(labels(&delegate), ["Scheme: Dracula"]);
        delegate.apply_query("");
        assert!(
            !delegate.is_scored(),
            "the blank query is the sectioned shape"
        );
        assert_eq!(labels(&delegate), sectioned, "the catalog comes back whole");
        assert_eq!(
            delegate.reset_selection(),
            None,
            "the sectioned catalog opens with nothing selected, as at open"
        );
    }

    #[test]
    fn the_rows_are_a_function_of_the_query_alone() {
        let mut delegate = plain_delegate(surface_catalog(&no_binds()));
        delegate.apply_query("drac");
        let typed = labels(&delegate);
        assert_eq!(typed, ["Scheme: Dracula"], "the query under test matches");
        // Backspace to blank, then the same query again: identical rows.
        delegate.apply_query("");
        delegate.apply_query("drac");
        assert_eq!(labels(&delegate), typed, "re-typing gives the same rows");
        // Surrounding whitespace is not part of the query.
        delegate.apply_query("  drac  ");
        assert_eq!(labels(&delegate), typed, "whitespace never narrows a query");
        delegate.apply_query("   ");
        assert!(
            !delegate.is_scored(),
            "a blank query selects the sectioned shape"
        );
    }

    #[test]
    fn a_scored_query_selects_its_first_row() {
        let mut delegate = plain_delegate(surface_catalog(&no_binds()));
        delegate.apply_query("drac");
        assert_eq!(
            delegate.reset_selection(),
            Some(IndexPath::default()),
            "every query with hits starts on its top row"
        );
    }

    // --- the toggle chord ---

    #[test]
    fn the_toggle_chord_is_the_keybind_map_s_own_entry() {
        // The default: the registry's Ctrl+Shift+P, matched the way
        // the pane matches it.
        let chord = toggle_chord(&no_binds()).expect("the registry carries command_palette");
        assert!(chord_matches(
            &chord,
            &Keystroke::parse("ctrl-shift-p").expect("a keystroke")
        ));
        assert!(!chord_matches(
            &chord,
            &Keystroke::parse("ctrl-shift-o").expect("a keystroke")
        ));

        // A custom bind moves the toggle with it.
        let chord = toggle_chord(&alt_p_palette()).expect("the custom bind resolves");
        assert!(chord_matches(
            &chord,
            &Keystroke::parse("alt-p").expect("a keystroke")
        ));
        assert!(!chord_matches(
            &chord,
            &Keystroke::parse("ctrl-shift-p").expect("a keystroke")
        ));
    }

    #[gpui::test]
    fn the_toggle_chord_dismisses_from_the_held_keyboard(cx: &mut gpui::TestAppContext) {
        let (mut cx, _palette, log) = mount(cx, &no_binds());
        cx.simulate_keystrokes("ctrl-shift-p");
        assert_eq!(
            *log.borrow(),
            ["dismiss"],
            "the open chord, pressed again while the palette holds the keyboard, closes it"
        );
    }

    #[gpui::test]
    fn another_owned_chord_stays_in_the_palette(cx: &mut gpui::TestAppContext) {
        let (mut cx, _palette, log) = mount(cx, &no_binds());
        cx.simulate_keystrokes("ctrl-shift-o");
        assert!(
            log.borrow().is_empty(),
            "the images panel's chord is not the palette's to act on"
        );
    }

    #[gpui::test]
    fn a_custom_bind_moves_the_toggle(cx: &mut gpui::TestAppContext) {
        let (mut cx, _palette, log) = mount(cx, &alt_p_palette());
        cx.simulate_keystrokes("ctrl-shift-p");
        assert!(
            log.borrow().is_empty(),
            "the released default no longer toggles"
        );
        cx.simulate_keystrokes("alt-p");
        assert_eq!(*log.borrow(), ["dismiss"], "the custom chord toggles");
    }
}
