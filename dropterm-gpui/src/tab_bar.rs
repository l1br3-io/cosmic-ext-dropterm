// SPDX-License-Identifier: GPL-3.0-only

//! The tab strip on GPUI (gpui-tabs): the bar's view model over
//! [`dropterm_core::tabs`]' unchanged model — every behavior (title
//! resolution and fallback, the title prefix's pin and attention
//! markers, insert positions, activation, attention clearing) is a
//! core call; this module only maps tab state to presentation and
//! renders it. The presentation rules port `src/tab_bar.rs` (the
//! libcosmic bar, read-only reference): the strip is always present
//! carrying the `+` and gear affordances at its right corner, tab
//! rows render from two tabs on, the strip's paint derives from the
//! terminal pane's background (a slightly darkened strip, a
//! luminance-readable foreground), and the border hairline sits on
//! the terminal-facing edge.

use std::path::PathBuf;

use gpui::prelude::*;

use dropterm_core::config::{Edge, TabPosition, TabTitleMode};
use dropterm_core::tabs::{
    Attention, CloseDecision, TabId, close_tab, insert_new_tab, next_default_title, resolve_title,
    shell_title_from_report, title_prefix,
};

/// The strip's height in logical pixels — the pane yields this much of
/// the surface to the bar (the surface itself never grows for it; the
/// reference bar's own `BAR_HEIGHT` rule, ported).
pub(crate) const BAR_HEIGHT: f32 = 32.0;

/// Whether the strip sits at the surface's bottom — the
/// `TabPosition` layout rule ported from the libcosmic app's
/// `effective_bar_at_bottom`: `Auto` places the strip opposite the
/// anchored edge (at the surface's free side), while `Top`/`Bottom`
/// pin it to that side regardless of anchor.
pub(crate) fn bar_at_bottom(edge: Edge, tab_position: TabPosition) -> bool {
    match tab_position {
        TabPosition::Auto => matches!(edge, Edge::Top),
        TabPosition::Top => false,
        TabPosition::Bottom => true,
    }
}

/// Whether the tab row renders its buttons: the strip is always
/// present, but tab rows — and their close marks — only from two tabs
/// on; the single-tab strip carries the corner affordances alone (the
/// reference bar's rule).
pub(crate) fn shows_tab_buttons(tab_count: usize) -> bool {
    tab_count >= 2
}

/// A color's relative luminance (the WCAG definition) — the reference
/// bar's own one flip formula, ported: the strip's foreground picks by
/// it over the pane background.
fn relative_luminance(r: f32, g: f32, b: f32) -> f32 {
    fn channel(c: f32) -> f32 {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// An 8-bit channel triple as GPUI's color.
fn hsla(r: u8, g: u8, b: u8) -> gpui::Hsla {
    gpui::Rgba {
        r: f32::from(r) / 255.,
        g: f32::from(g) / 255.,
        b: f32::from(b) / 255.,
        a: 1.,
    }
    .into()
}

/// The strip's paint pair — (background, foreground) — derived from
/// the terminal pane's effective background exactly like the reference
/// bar's `pane_colors`: the strip slightly darkened (×0.92, so the
/// bar reads as its own band instead of blending into the terminal),
/// and a foreground picked near-black or near-white by the
/// *background's* luminance, so labels and glyphs stay readable in
/// either scheme. `None` (no scheme background) falls back to
/// black/near-white — the dark-drawer default.
pub(crate) fn strip_pair(
    background: Option<alacritty_terminal::vte::ansi::Rgb>,
) -> (gpui::Hsla, gpui::Hsla) {
    let Some(bg) = background else {
        return (hsla(0, 0, 0), hsla(0xF5, 0xF5, 0xFF));
    };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a darkened 0..=255 channel: round keeps the nearest u8 and the value stays in range by construction"
    )]
    let dimmed = |channel: u8| (f32::from(channel) * 0.92).round() as u8;
    let luminance = relative_luminance(
        f32::from(bg.r) / 255.,
        f32::from(bg.g) / 255.,
        f32::from(bg.b) / 255.,
    );
    let foreground = if luminance > 0.5 {
        hsla(0x1B, 0x1B, 0x22)
    } else {
        hsla(0xF5, 0xF5, 0xFF)
    };
    (hsla(dimmed(bg.r), dimmed(bg.g), dimmed(bg.b)), foreground)
}

/// One rendered tab row: the pure presentation mapping of one tab
/// record (the headless-testable half of the bar).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RowVm {
    pub(crate) id: TabId,
    /// The row's whole label: core's title prefix (pin, then
    /// attention marker) composed ahead of the resolved title.
    pub(crate) label: String,
    pub(crate) is_active: bool,
    pub(crate) is_pinned: bool,
    pub(crate) attention: Attention,
}

/// One tab's plain state — the GPUI-side record the shell's tab list
/// carries. Deliberately not core's `Tab`: that record owns a live
/// `Terminal` plus the session/paste state the libcosmic app tracks;
/// this frontend's pane owns the engine, so the record keeps only the
/// presentation state, with every transition resolved through core's
/// own calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabRecord {
    pub(crate) id: TabId,
    /// The resolved display title (core's `resolve_title` output,
    /// maintained at every apply point).
    pub(crate) title: String,
    /// The last OSC 0/2 report (`None` until the first, cleared by a
    /// reset or an empty payload) — the retitle input, stored by
    /// core's own rule.
    pub(crate) shell_title: Option<String>,
    /// The tab's working directory (the engine's `/proc` cwd read,
    /// the same input the libcosmic retitle path caches) — the cwd
    /// title modes' input.
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) pinned: bool,
    pub(crate) attention: Attention,
}

/// Resolves one record's display title through core — the one resolve
/// call every apply point shares (a report landing or clearing, a cwd
/// move, a mode flip, a fresh spawn). The profile and rename inputs
/// stay `None` until the interactions land them (Task 2's profile
/// launches and inline rename); core's own fallbacks cover both.
fn resolve_record_title(tab: &TabRecord, mode: TabTitleMode) -> String {
    resolve_title(
        mode,
        tab.shell_title.as_deref(),
        tab.cwd.as_deref(),
        None,
        tab.id,
        None,
    )
}

/// The shell's tab list: plain records plus the active index, with
/// every list behavior a [`dropterm_core::tabs`] call — insert
/// positions through `insert_new_tab`, activation and attention
/// clearing through core's `Attention` transition, titles through
/// `resolve_title`. The bar renders from it; nothing here duplicates
/// the model.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TabList {
    tabs: Vec<TabRecord>,
    active: usize,
    next_id: u32,
}

impl TabList {
    /// Spawns a fresh tab record (the id-numbered `Terminal N`
    /// default title) at the configured position and activates it,
    /// returning its id. Insertion and activation are core's
    /// `insert_new_tab` semantics: `after_current` inserts beside the
    /// active tab (clamped to the end), else appends.
    pub(crate) fn spawn(&mut self, after_current: bool) -> TabId {
        let id = TabId(self.next_id);
        self.next_id += 1;
        let tab = TabRecord {
            id,
            title: next_default_title(id),
            shell_title: None,
            cwd: None,
            pinned: false,
            attention: Attention::None,
        };
        self.active = insert_new_tab(&mut self.tabs, self.active, after_current, tab);
        id
    }

    /// The active tab's id, `None` while the list is empty.
    pub(crate) fn active_id(&self) -> Option<TabId> {
        self.tabs.get(self.active).map(|tab| tab.id)
    }

    /// Activates the tab with `id` — clearing its attention marker
    /// through core's transition — returning whether the active tab
    /// changed. An unknown id, or the already-active tab, changes
    /// nothing.
    pub(crate) fn select(&mut self, id: TabId) -> bool {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };
        if index == self.active {
            return false;
        }
        self.active = index;
        let tab = &mut self.tabs[index];
        tab.attention = tab.attention.on_activated();
        true
    }

    /// Stores the tab's current title inputs (the pane's own OSC 0/2
    /// tracker and the engine's cwd read, both polled) and re-resolves
    /// its display title when either moved, returning whether a
    /// re-resolution ran — the title itself may land on the same value
    /// (Numbered mode ignores reports, so a report arriving still
    /// re-resolves to the standing `Terminal N`). The shell feeds this
    /// on its tick.
    pub(crate) fn sync_title(
        &mut self,
        id: TabId,
        report: Option<String>,
        cwd: Option<PathBuf>,
        mode: TabTitleMode,
    ) -> bool {
        // An empty report is a title clear — core's own storage rule.
        let shell_title = report.and_then(shell_title_from_report);
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return false;
        };
        if tab.shell_title == shell_title && tab.cwd == cwd {
            return false;
        }
        tab.shell_title = shell_title;
        tab.cwd = cwd;
        tab.title = resolve_record_title(tab, mode);
        true
    }

    /// Re-resolves every tab's display title under `mode` — a live
    /// mode flip re-titles the whole row immediately, from the stored
    /// inputs.
    pub(crate) fn refresh_titles(&mut self, mode: TabTitleMode) {
        for tab in &mut self.tabs {
            tab.title = resolve_record_title(tab, mode);
        }
    }

    /// The bar's view model: one row per tab, active flagged, labels
    /// composed through core's `title_prefix`.
    pub(crate) fn rows(&self) -> Vec<RowVm> {
        let active = self.active;
        self.tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| RowVm {
                id: tab.id,
                label: format!("{}{}", title_prefix(tab.pinned, tab.attention), tab.title),
                is_active: index == active,
                is_pinned: tab.pinned,
                attention: tab.attention,
            })
            .collect()
    }

    /// The tab records in strip order (the shell maps them to their
    /// panes).
    pub(crate) fn records(&self) -> &[TabRecord] {
        &self.tabs
    }

    /// Closes the tab with `id` through core's close decision — the
    /// successor activation and the never-empty respawn signal —
    /// returning `None` for an unknown id. Task 1's use is the
    /// spawn-failure rollback; Task 2's close flow layers the confirm
    /// gate and pane teardown around the same core call.
    pub(crate) fn close(&mut self, id: TabId) -> Option<CloseDecision> {
        let index = self.tabs.iter().position(|tab| tab.id == id)?;
        let decision = close_tab(&mut self.tabs, index, self.active)?;
        self.active = decision.next_active;
        Some(decision)
    }

    /// The records mutably — the interactions' apply point (Task 2's
    /// rename/pin writes; the tests' state seeding).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Task 2's interactions' apply point; the tests seed through it meanwhile"
        )
    )]
    pub(crate) fn tabs_mut(&mut self) -> &mut Vec<TabRecord> {
        &mut self.tabs
    }
}

// -- the strip element ------------------------------------------------
//
// The presentation constants port the reference bar's `METRICS` (the
// libcosmic bar keeps them constants rather than theme values so its
// pure layout stays deterministic; the same numbers keep the two
// frontends' strips the same size).

/// Horizontal padding inside a tab button, on each side.
const BUTTON_PADDING_X: f32 = 10.0;
/// Gap between consecutive tab buttons, and around the corner
/// affordances.
const BUTTON_GAP: f32 = 4.0;
/// Side length of the square close/`+`/gear glyph targets.
const GLYPH_SIZE: f32 = 24.0;
/// Title text size in logical pixels.
const TITLE_FONT_SIZE: f32 = 14.0;
/// Narrowest a tab button may get under squeeze or with an empty
/// title; widest no matter how long the title (longer titles
/// ellipsize at the button edge).
const MIN_BUTTON_WIDTH: f32 = 48.0;
const MAX_BUTTON_WIDTH: f32 = 200.0;
/// The border hairline's alpha — the strip foreground at low
/// strength, hierarchy by alpha, never a new hue (the reference bar's
/// `BORDER_ALPHA`).
const BORDER_ALPHA: f32 = 0.3;

/// The strip's select callback: one tab id into the shell.
type SelectFn = std::rc::Rc<dyn Fn(TabId, &mut gpui::Window, &mut gpui::App)>;
/// The strip's corner-affordance callbacks (`+`, gear).
type CornerFn = std::rc::Rc<dyn Fn(&mut gpui::Window, &mut gpui::App)>;

/// The tab strip element (Task 1's render half): one bar-height row —
/// tab buttons while two or more tabs exist, the `+` and gear
/// affordances at the right corner, the border hairline on the
/// terminal-facing edge. Built per frame by the shell from the tab
/// list's view model; every behavior is a callback into the shell.
#[derive(IntoElement)]
pub(crate) struct TabBarView {
    rows: Vec<RowVm>,
    strip: gpui::Hsla,
    fg: gpui::Hsla,
    show_border: bool,
    at_bottom: bool,
    on_select: SelectFn,
    on_new_tab: CornerFn,
    on_settings: CornerFn,
}

impl TabBarView {
    pub(crate) fn new(rows: Vec<RowVm>, strip: gpui::Hsla, fg: gpui::Hsla) -> Self {
        Self {
            rows,
            strip,
            fg,
            show_border: true,
            at_bottom: false,
            on_select: std::rc::Rc::new(|_, _, _| {}),
            on_new_tab: std::rc::Rc::new(|_, _| {}),
            on_settings: std::rc::Rc::new(|_, _| {}),
        }
    }

    /// Whether the border hairline renders (`tab_bar_border`).
    pub(crate) fn show_border(mut self, show: bool) -> Self {
        self.show_border = show;
        self
    }

    /// Whether the strip sits at the surface's bottom (the hairline
    /// faces the terminal: the strip's top when at the bottom, its
    /// bottom when at the top).
    pub(crate) fn at_bottom(mut self, at_bottom: bool) -> Self {
        self.at_bottom = at_bottom;
        self
    }

    pub(crate) fn on_select(
        mut self,
        on_select: impl Fn(TabId, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_select = std::rc::Rc::new(on_select);
        self
    }

    pub(crate) fn on_new_tab(
        mut self,
        on_new_tab: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_new_tab = std::rc::Rc::new(on_new_tab);
        self
    }

    pub(crate) fn on_settings(
        mut self,
        on_settings: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_settings = std::rc::Rc::new(on_settings);
        self
    }

    /// One tab button: pill background and brighter text when active,
    /// the composed label, and the close glyph (its click wiring and
    /// close decisions arrive with Task 2's interactions).
    fn row_element(row: &RowVm, fg: gpui::Hsla, on_select: SelectFn) -> gpui::Div {
        let active_pill = gpui::Hsla { a: 0.18, ..fg };
        let text = if row.is_active {
            fg
        } else {
            gpui::Hsla { a: 0.65, ..fg }
        };
        let id = row.id;
        gpui::div()
            .h_full()
            .min_w(gpui::px(MIN_BUTTON_WIDTH))
            .max_w(gpui::px(MAX_BUTTON_WIDTH))
            .px(gpui::px(BUTTON_PADDING_X))
            .rounded_sm()
            .flex()
            .items_center()
            .gap(gpui::px(6.0))
            .when(row.is_active, |button| button.bg(active_pill))
            .text_color(text)
            .overflow_hidden()
            .child(
                gpui::div()
                    .flex_shrink(1.)
                    .overflow_hidden()
                    .child(row.label.clone()),
            )
            .child(
                gpui::div()
                    .size(gpui::px(GLYPH_SIZE))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .text_color(gpui::Hsla { a: 0.7, ..fg })
                    .child("×"),
            )
            .on_mouse_down(
                gpui::MouseButton::Left,
                move |_, window: &mut gpui::Window, cx: &mut gpui::App| on_select(id, window, cx),
            )
    }

    /// One corner affordance — the `+` or the gear — a square glyph
    /// target at the strip's right corner.
    fn glyph_button(glyph: &'static str, fg: gpui::Hsla, on_press: CornerFn) -> gpui::Div {
        gpui::div()
            .size(gpui::px(GLYPH_SIZE))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .text_color(fg)
            .child(glyph)
            .on_mouse_down(
                gpui::MouseButton::Left,
                move |_, window: &mut gpui::Window, cx: &mut gpui::App| on_press(window, cx),
            )
    }
}

impl gpui::RenderOnce for TabBarView {
    fn render(self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> impl gpui::IntoElement {
        let fg = self.fg;
        let mut bar = gpui::div()
            .w_full()
            .h(gpui::px(BAR_HEIGHT))
            .flex()
            .flex_none()
            .flex_row()
            .items_center()
            .gap(gpui::px(BUTTON_GAP))
            .px(gpui::px(BUTTON_PADDING_X))
            .bg(self.strip)
            .text_size(gpui::px(TITLE_FONT_SIZE))
            .overflow_hidden();
        if self.show_border {
            // The hairline faces the terminal: the strip's top edge
            // when it sits at the bottom, its bottom edge otherwise.
            let hairline = gpui::Hsla {
                a: BORDER_ALPHA,
                ..fg
            };
            bar = if self.at_bottom {
                bar.border_t(gpui::px(1.)).border_color(hairline)
            } else {
                bar.border_b(gpui::px(1.)).border_color(hairline)
            };
        }
        if shows_tab_buttons(self.rows.len()) {
            let on_select = std::rc::Rc::clone(&self.on_select);
            bar = bar.children(
                self.rows
                    .iter()
                    .map(|row| Self::row_element(row, fg, std::rc::Rc::clone(&on_select))),
            );
        }
        bar.child(gpui::div().flex_1())
            .child(Self::glyph_button("+", fg, self.on_new_tab))
            .child(Self::glyph_button("⚙", fg, self.on_settings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dropterm_core::config::{Edge, TabTitleMode};

    #[test]
    fn auto_places_the_strip_opposite_the_anchor_and_pins_override() {
        // Auto: opposite the anchored edge — a top-anchored drawer
        // carries the strip at its bottom, a bottom-anchored one at
        // its top.
        assert!(bar_at_bottom(Edge::Top, TabPosition::Auto));
        assert!(!bar_at_bottom(Edge::Bottom, TabPosition::Auto));
        // Top/Bottom pin regardless of anchor.
        assert!(!bar_at_bottom(Edge::Top, TabPosition::Top));
        assert!(!bar_at_bottom(Edge::Bottom, TabPosition::Top));
        assert!(bar_at_bottom(Edge::Top, TabPosition::Bottom));
        assert!(bar_at_bottom(Edge::Bottom, TabPosition::Bottom));
    }

    #[test]
    fn tab_rows_render_from_two_tabs_on() {
        assert!(!shows_tab_buttons(1));
        assert!(shows_tab_buttons(2));
        assert!(shows_tab_buttons(7));
    }

    #[test]
    fn strip_pair_darkens_the_pane_background_and_picks_readable_foreground() {
        use alacritty_terminal::vte::ansi::Rgb;
        // A dark pane background (Nord's #282A36-ish): the strip
        // darkens a step further and the foreground reads near-white.
        let (strip, fg) = strip_pair(Some(Rgb {
            r: 40,
            g: 42,
            b: 54,
        }));
        assert_eq!(strip, rgb_hsla(37, 39, 50));
        assert_eq!(fg, rgb_hsla(0xF5, 0xF5, 0xFF));
        // A light background flips the foreground near-black.
        let (_, fg) = strip_pair(Some(Rgb {
            r: 235,
            g: 235,
            b: 245,
        }));
        assert_eq!(fg, rgb_hsla(0x1B, 0x1B, 0x22));
        // No scheme background: the fallback pair.
        let (strip, fg) = strip_pair(None);
        assert_eq!((strip, fg), (rgb_hsla(0, 0, 0), rgb_hsla(0xF5, 0xF5, 0xFF)));
    }

    #[test]
    fn row_labels_compose_cores_prefix_ahead_of_the_resolved_title() {
        // The no-duplication pin (the chunk's planning ruling): for
        // every pinned × attention combination the row's label is
        // exactly core's title_prefix output ahead of the stored
        // resolved title — the view composes, never re-derives.
        for pinned in [false, true] {
            for attention in [Attention::None, Attention::Activity, Attention::Bell] {
                let mut list = TabList::default();
                list.spawn(false);
                let tabs = list.tabs_mut();
                tabs[0].pinned = pinned;
                tabs[0].attention = attention;
                tabs[0].title = "My Title".to_string();
                let [row] = list.rows().try_into().expect("one row");
                assert_eq!(
                    row.label,
                    format!("{}My Title", title_prefix(pinned, attention))
                );
            }
        }
    }

    #[test]
    fn rows_flag_the_active_tab_and_carry_state() {
        let mut list = TabList::default();
        let first = list.spawn(false);
        let second = list.spawn(false);
        let rows = list.rows();
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].is_active);
        assert!(rows[1].is_active);
        assert_eq!(rows[0].id, first);
        assert_eq!(rows[1].id, second);

        assert!(list.select(first));
        let rows = list.rows();
        assert!(rows[0].is_active);
        assert!(!rows[1].is_active);
    }

    #[test]
    fn spawns_insert_through_cores_positions_and_activate() {
        let mut list = TabList::default();
        let first = list.spawn(false);
        assert_eq!(list.active_id(), Some(first));

        // after_current: right of the active tab, and active.
        let second = list.spawn(true);
        assert_eq!(
            list.records().iter().map(|tab| tab.id).collect::<Vec<_>>(),
            vec![first, second]
        );
        assert_eq!(list.active_id(), Some(second));

        // append: at the end regardless of the active tab.
        let third = list.spawn(false);
        assert_eq!(
            list.records().iter().map(|tab| tab.id).collect::<Vec<_>>(),
            vec![first, second, third]
        );
        assert_eq!(list.active_id(), Some(third));

        // Fresh spawns title through the id-numbered default.
        assert_eq!(list.records()[0].title, "Terminal 1");
        assert_eq!(list.records()[2].title, "Terminal 3");

        // The empty list's first after_current spawn clamps to append.
        let mut fresh = TabList::default();
        let only = fresh.spawn(true);
        assert_eq!(
            fresh.records().iter().map(|tab| tab.id).collect::<Vec<_>>(),
            vec![only]
        );
    }

    #[test]
    fn select_clears_attention_through_cores_transition_and_ignores_unknowns() {
        let mut list = TabList::default();
        let first = list.spawn(false);
        list.spawn(false);

        // An inactive tab wearing the bell marker.
        list.tabs_mut()[0].attention = Attention::Bell;
        assert!(list.select(first));
        assert_eq!(list.rows()[0].attention, Attention::None);

        // Re-selecting the active tab changes nothing (and reports
        // no change).
        assert!(!list.select(first));
        // An unknown id is a no-op.
        assert!(!list.select(TabId(99)));
        assert_eq!(list.active_id(), Some(first));
    }

    #[test]
    fn titles_follow_the_configured_mode_through_cores_resolution() {
        let mut list = TabList::default();
        let first = list.spawn(false);

        // Shell mode: the report shows, a cleared report falls back
        // to the id-numbered default (core's own storage rule — an
        // empty report is a clear, stored as no-report).
        assert!(list.sync_title(
            first,
            Some("jpt@rig:~".to_string()),
            None,
            TabTitleMode::Shell
        ));
        assert_eq!(list.records()[0].title, "jpt@rig:~");
        assert!(list.sync_title(first, Some(String::new()), None, TabTitleMode::Shell));
        assert_eq!(list.records()[0].title, "Terminal 1");

        // Numbered: reports never land — the title parks at the
        // default whatever the report says.
        list.sync_title(
            first,
            Some("never shown".to_string()),
            None,
            TabTitleMode::Numbered,
        );
        assert_eq!(list.records()[0].title, "Terminal 1");

        // LastSegment: the cwd's final component, with the report as
        // core's documented fallback while no cwd is readable.
        list.sync_title(
            first,
            Some("fallback".to_string()),
            None,
            TabTitleMode::LastSegment,
        );
        assert_eq!(list.records()[0].title, "fallback");
        list.sync_title(
            first,
            Some("fallback".to_string()),
            Some(PathBuf::from("/home/jpt/src")),
            TabTitleMode::LastSegment,
        );
        assert_eq!(list.records()[0].title, "src");

        // A live mode flip re-titles from the stored inputs: the
        // stored report shows again under Shell.
        list.refresh_titles(TabTitleMode::Shell);
        assert_eq!(list.records()[0].title, "fallback");
    }

    #[test]
    fn close_runs_through_cores_close_decision() {
        let mut list = TabList::default();
        let first = list.spawn(false);
        let second = list.spawn(false);
        let third = list.spawn(false);

        // Closing the active middle tab: its successor activates.
        let decision = list.close(second).expect("a decision");
        assert_eq!(decision.next_active, 1);
        assert!(!decision.respawn);
        assert_eq!(list.active_id(), Some(third));

        // Closing the last tab standing signals the never-empty
        // respawn.
        list.close(first);
        let decision = list.close(third).expect("a decision");
        assert!(decision.respawn);
        assert_eq!(list.active_id(), None);

        // An unknown id decides nothing.
        assert!(list.close(TabId(99)).is_none());
    }

    #[test]
    fn sync_title_reports_no_change_for_standing_inputs() {
        let mut list = TabList::default();
        let first = list.spawn(false);
        assert!(list.sync_title(
            first,
            Some("standing".to_string()),
            Some(PathBuf::from("/tmp")),
            TabTitleMode::Shell
        ));
        // The same inputs again: nothing moves.
        assert!(!list.sync_title(
            first,
            Some("standing".to_string()),
            Some(PathBuf::from("/tmp")),
            TabTitleMode::Shell
        ));
        // An unknown tab is a no-op.
        assert!(!list.sync_title(TabId(99), Some("x".to_string()), None, TabTitleMode::Shell));
    }

    // -- helpers ------------------------------------------------------

    fn rgb_hsla(r: u8, g: u8, b: u8) -> gpui::Hsla {
        gpui::Rgba {
            r: f32::from(r) / 255.,
            g: f32::from(g) / 255.,
            b: f32::from(b) / 255.,
            a: 1.,
        }
        .into()
    }
}
