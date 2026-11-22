// SPDX-License-Identifier: GPL-3.0-only

//! The command palette on GPUI (command-palette spec): core's
//! [`dropterm::core::palette`] model presented as the close-confirm's
//! in-surface modal — a full-surface scrim whose press dismisses,
//! carrying a top-centered panel over the live terminal, the Kit
//! `List`'s own escape/enter/up/down key context as the innards, the
//! list holding the keyboard (the pane's claim defers, typing never
//! reaches the pane). Every pick closes the palette first, then
//! lands in the funnel its keybind or menu twin already drives —
//! [`route`] is the bin-side Verb→funnel table, `bin/context_menu`
//! .rs's discipline: exhaustive over core's `Verb`, so a new variant
//! is a compile error away from an unrouted row. Task 3's search
//! rides the same List (searchable off for now); the palette is
//! in-window chrome, so it composes identically in layer-shell mode
//! and the GNOME plain-window mode.

use std::rc::Rc;

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

use crate::app_shell::{DismissPalette, Shell};
use crate::terminal::input::{TermAction, gpui_key_name};

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

/// The palette's pick callback: the row's command, with the window
/// and app the List's confirm hands. The shell's own closure closes
/// the palette, then routes — the context-menu `PickFn`'s shape.
type PickFn = Rc<dyn Fn(Command, &mut gpui::Window, &mut App)>;
/// The palette's dismissal callback: Esc (the List's Cancel or the
/// container's bound action), the scrim's press, the toggle chord.
type DismissFn = Rc<dyn Fn(&mut gpui::Window, &mut App)>;

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

/// The List's delegate: the composed catalog, sectioned by
/// contiguous [`Section`] runs (core's composition guarantees
/// contiguity, pinned). The empty query's render is this delegate
/// whole; Task 3's search swaps in the scored flat list through the
/// same trait.
struct PaletteDelegate {
    catalog: Vec<Command>,
    /// One `(section, catalog bounds)` pair per contiguous run, in
    /// composition order — the IndexPath→row mapping's table.
    sections: Vec<(Section, std::ops::Range<usize>)>,
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
            selected: None,
            on_pick,
            on_dismiss,
        }
    }

    fn command_at(&self, ix: IndexPath) -> Option<&Command> {
        self.catalog
            .get(self.sections.get(ix.section)?.1.start + ix.row)
    }
}

impl ListDelegate for PaletteDelegate {
    type Item = ListItem;

    fn sections_count(&self, _cx: &App) -> usize {
        self.sections.len().max(1)
    }

    fn items_count(&self, section: usize, _cx: &App) -> usize {
        self.sections
            .get(section)
            .map_or(0, |(_, bounds)| bounds.len())
    }

    fn render_section_header(
        &mut self,
        section: usize,
        _window: &mut gpui::Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
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
        // The row id is the flat catalog index — unique across every
        // section, the ElementId's own usize form.
        let flat = self.sections.get(ix.section)?.1.start + ix.row;
        Some(
            ListItem::new(flat).h(px(32.)).px_2().child(
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
    /// the shell's close.
    fn cancel(&mut self, window: &mut gpui::Window, cx: &mut Context<ListState<Self>>) {
        let dismiss = Rc::clone(&self.on_dismiss);
        dismiss(window, cx);
    }
}

/// The open palette: the modal entity the shell holds while it
/// stands — its own focus handle (the list's, focused at open, the
/// keyboard's home while the palette stands), the blur dismissal
/// with the `Select::on_blur` containment shape, and the top-centered
/// panel over the full-surface scrim.
pub(crate) struct Palette {
    list: Entity<ListState<PaletteDelegate>>,
    /// The palette's own container handle — the within-palette move
    /// guard's containment target (a blur that lands here is a move
    /// inside the palette, not a dismissal).
    container_focus: gpui::FocusHandle,
    /// The blur dismissal's subscription — held, never read: dropping
    /// the palette drops it (the status menu's own shape).
    _blur: gpui::Subscription,
    on_dismiss: DismissFn,
}

impl Palette {
    pub(crate) fn new(
        shell: Entity<Shell>,
        catalog: Vec<Command>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // A strong handle, the drawer's own precedent: the shell →
        // palette → shell cycle lives exactly while the palette
        // stands, and every dismissal path drops it.
        let pick_shell = shell.clone();
        let pick: PickFn = Rc::new(move |command, window, cx| {
            // Close first, then execute — activate-then-close, never
            // act-under-the-modal (the peers' rule the spec pins).
            pick_shell.update(cx, |shell, cx| {
                shell.close_palette(cx);
                shell.run_pick(command, window, cx);
            });
        });
        let dismiss: DismissFn = Rc::new(move |_window, cx| {
            shell.update(cx, Shell::close_palette);
        });
        let delegate =
            PaletteDelegate::from_catalog(catalog, Rc::clone(&pick), Rc::clone(&dismiss));
        let list = cx.new(|cx| ListState::new(delegate, window, cx));
        let container_focus = cx.focus_handle();
        // The blur dismissal: the list's handle losing focus to
        // anything outside the palette dismisses it — with the
        // Select's containment shape, a move that stays inside (the
        // container's own handle holding it; Task 3's search input
        // gains its arm when it lands) does not.
        let list_focus = list.read(cx).focus_handle(cx).clone();
        Self {
            list: list.clone(),
            container_focus,
            _blur: cx.on_blur(&list_focus, window, |this, window, cx| {
                if this.container_focus.is_focused(window) {
                    return;
                }
                let dismiss = Rc::clone(&this.on_dismiss);
                dismiss(window, cx);
            }),
            on_dismiss: Rc::clone(&dismiss),
        }
    }

    /// The keyboard's hand to the palette: the list holds it (Task
    /// 3's searchable input takes over automatically — the Kit's own
    /// `focus` focuses the input when searchable).
    pub(crate) fn focus(&self, window: &mut gpui::Window, cx: &mut App) {
        self.list.update(cx, |list, cx| list.focus(window, cx));
    }

    /// The list's focus handle — the shell's render-time repair reads
    /// it to hand the keyboard back to the active pane once a
    /// dismissed palette's handle dangles.
    pub(crate) fn focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        self.list.read(cx).focus_handle(cx)
    }
}

impl Render for Palette {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let card = theme.background;
        let hairline = theme.border;
        let fg = theme.foreground;
        let dismiss = Rc::clone(&self.on_dismiss);
        div().size_full().child(
            div()
                .absolute()
                .inset_0()
                .bg(rgba(0x0000_0080))
                .flex()
                .justify_center()
                .items_start()
                // The palette's key context: Escape rides the bound
                // dismissal while the container holds focus; while the
                // list holds it, the List's own Cancel (deeper in the
                // context stack) wins — both paths close.
                .key_context("palette")
                .track_focus(&self.container_focus)
                .on_action(move |_: &DismissPalette, window, cx| dismiss(window, cx))
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
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
