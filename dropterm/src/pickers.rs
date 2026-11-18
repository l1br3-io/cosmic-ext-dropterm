// SPDX-License-Identifier: GPL-3.0-only

//! The chrome pickers on GPUI (gpui-chrome Task 1): the font picker
//! and the scheme picker on the Kit's `Select` — the closed control
//! opening a popover list — each fed by its core seam and carrying
//! the libcosmic picker's preview fidelity: a font row's label is set
//! in its own family (the font-picker spec's ruling — the row shows
//! the real thing, not a name-as-proxy), a scheme row is the scheme
//! itself (the background and foreground chips plus the sixteen ANSI
//! colors the terminal renders with, resolved through
//! [`scheme_previews`]).
//!
//! The pickers deliver their picks as values through the handles'
//! callbacks; the write contract is the host's (the settings drawer,
//! gpui-chrome Task 2, routes them through the ported control map —
//! write-through per commit point, never an Apply button). The pure
//! halves here are the libcosmic planner trio's semantics
//! (`font_picker_rows`/`_selected`/`_write` and the scheme
//! dropdown's selected/write) with the planned-write layer left to
//! its owning task.
//!
//! [`scheme_previews`]: dropterm::core::theme_source::scheme_previews

// The components await their host (the settings drawer, Task 2) —
// the same holding pattern `input.rs`'s `action_id` sat in: the
// module's consumers arrive with the drawer, and the allow comes
// off with them.
#![allow(dead_code)]

use std::rc::Rc;

use dropterm::core::theme_source::{SchemePreview, scheme_names, scheme_previews};
use gpui::prelude::*;
use gpui::{App, Entity, SharedString, px};
use gpui_component::IndexPath;
use gpui_component::h_flex;
use gpui_component::searchable_list::{SearchableListDelegate, SearchableListItem};
use gpui_component::select::{SelectEvent, SelectState};

use crate::terminal::grid::hsla;

// --- the pure model (the libcosmic planner trio's semantics) ---

/// The picker's rows under a stored `font_name`: the families in
/// list order, preceded by the stored name itself when no family
/// matches — a hand-edited or uninstalled value stays visible and
/// selectable, never an empty picker.
pub(crate) fn font_rows(families: &[String], stored: &str) -> Vec<String> {
    if families.iter().any(|family| family == stored) {
        return families.to_vec();
    }
    // The synthetic leader: the stored value's own row.
    let mut rows = Vec::with_capacity(families.len() + 1);
    rows.push(stored.to_string());
    rows.extend(families.iter().cloned());
    rows
}

/// The picker's selection row for a stored `font_name`: the matching
/// family's row, else the synthetic leading row — the stored value is
/// always the selected one. The match is exact and case-sensitive
/// (`Family::Name`'s own match is exact; a case variant is a
/// different family).
pub(crate) fn font_selected(families: &[String], stored: &str) -> usize {
    families
        .iter()
        .position(|family| family == stored)
        .unwrap_or(0)
}

/// The value a font-picker pick commits: the row's family. An index
/// past the rows (the system's list changed under the open popover)
/// rejects — the `dropdown_write` rule, never an invented value.
pub(crate) fn font_pick(families: &[String], stored: &str, index: usize) -> Option<String> {
    font_rows(families, stored).into_iter().nth(index)
}

/// The scheme picker's selection row for a stored scheme name: the
/// registry row when the name resolves, else none — the closed
/// control shows the stored name as its placeholder rather than
/// inventing a position for a name the registry lacks.
pub(crate) fn scheme_selected(stored: &str) -> Option<usize> {
    scheme_names().iter().position(|name| *name == stored)
}

/// The registry name a scheme-picker pick commits; `None` for an
/// out-of-range row (stale popover, the `dropdown_write` rule).
pub(crate) fn scheme_pick(index: usize) -> Option<&'static str> {
    scheme_names().into_iter().nth(index)
}

// --- the delegates ---

/// One font-family row: the family's name, rendered in the family
/// itself wherever the row draws (the closed control's display and
/// the popover row share the item).
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct FontFamilyRow(pub String);

impl SearchableListItem for FontFamilyRow {
    type Value = String;

    fn title(&self) -> SharedString {
        SharedString::from(self.0.clone())
    }

    fn value(&self) -> &String {
        &self.0
    }

    /// The closed control shows the configured family set in that
    /// family — the preview the picker is.
    fn display_title(&self) -> Option<gpui::AnyElement> {
        Some(
            h_flex()
                .font_family(self.0.clone())
                .child(self.0.clone())
                .into_any_element(),
        )
    }
}

/// The font picker's delegate: the family rows.
#[derive(Clone, Default)]
pub(crate) struct FontFamilies(Vec<FontFamilyRow>);

impl SearchableListDelegate for FontFamilies {
    type Item = FontFamilyRow;

    fn items_count(&self, _section: usize) -> usize {
        self.0.len()
    }

    fn item(&self, ix: IndexPath) -> Option<&FontFamilyRow> {
        self.0.get(ix.row)
    }

    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        FontFamilyRow: SearchableListItem<Value = V>,
        V: PartialEq,
    {
        self.0
            .iter()
            .position(|row| row.value() == value)
            .map(|row| IndexPath::default().row(row))
    }

    /// Every popover row's label is set in its own family — the row
    /// shows the real thing, not a name-as-proxy (the custom row
    /// carries its own check mark; the adapter's default indicator
    /// is suppressed with the default layout).
    fn render_item(
        &self,
        _ix: IndexPath,
        item: &FontFamilyRow,
        checked: bool,
        _window: &mut gpui::Window,
        _cx: &mut App,
    ) -> Option<gpui::AnyElement> {
        let mut row = h_flex()
            .px_2()
            .py_1()
            .gap_2()
            .font_family(item.0.clone())
            .child(item.0.clone());
        if checked {
            row = row.child("✓");
        }
        Some(row.into_any_element())
    }
}

/// One scheme row: the registry name, carried beside the preview
/// that draws it.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SchemeRow(pub &'static str);

/// The scheme strip's chip footprint: a full strip is the background
/// and foreground chips plus the sixteen ANSI colors — eighteen
/// [`SCHEME_CHIP_W_PX`]-wide chips separated by
/// [`SCHEME_CHIP_GAP_PX`] gaps. The drawer's select column is narrow
/// (`CONTROL_COLUMN_PX`), so the chips are narrower than tall: the
/// strip must leave the row's name room or the name clips away and
/// the rows turn unidentifiable (the no-text regression the geometry
/// test below pins).
const SCHEME_CHIP_W_PX: f32 = 6.;
const SCHEME_CHIP_H_PX: f32 = 10.;
const SCHEME_CHIP_GAP_PX: f32 = 1.;
/// The full strip's chip count (background, foreground, 16 ANSI); a
/// preview with an unset slot draws one chip fewer.
const SCHEME_CHIP_COUNT: usize = 18;
/// The scheme select's menu width: wider than the trigger column so
/// the full strip and the registry names share the row (the popover
/// positioner clamps into the viewport). The strip's own width is
/// [`SCHEME_CHIP_COUNT`] × [`SCHEME_CHIP_W_PX`] + the gaps.
pub(crate) const SCHEME_MENU_WIDTH_PX: f32 = 320.;

/// The full strip's intrinsic width, chips and gaps together.
#[cfg(test)]
#[expect(
    clippy::cast_precision_loss,
    reason = "an eighteen-chip count is exact in f32's 23-bit mantissa"
)]
fn scheme_strip_px() -> f32 {
    SCHEME_CHIP_COUNT as f32 * SCHEME_CHIP_W_PX
        + (SCHEME_CHIP_COUNT - 1) as f32 * SCHEME_CHIP_GAP_PX
}

/// The scheme picker's delegate: the registry's rows in registry
/// order, each with its preview resolved once at build.
#[derive(Clone, Default)]
pub(crate) struct Schemes(Vec<(SchemeRow, SchemePreview)>);

impl Schemes {
    /// The registry's rows, previews resolved — one pass over
    /// [`scheme_previews`], the same table the rows draw from.
    pub(crate) fn from_registry() -> Self {
        Self(
            scheme_previews()
                .into_iter()
                .map(|(name, preview)| (SchemeRow(name), preview))
                .collect(),
        )
    }
}

impl SearchableListDelegate for Schemes {
    type Item = SchemeRow;

    fn items_count(&self, _section: usize) -> usize {
        self.0.len()
    }

    fn item(&self, ix: IndexPath) -> Option<&SchemeRow> {
        self.0.get(ix.row).map(|(row, _)| row)
    }

    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        SchemeRow: SearchableListItem<Value = V>,
        V: PartialEq,
    {
        self.0
            .iter()
            .position(|(row, _)| row.value() == value)
            .map(|row| IndexPath::default().row(row))
    }

    /// A scheme row is the scheme itself: the background and
    /// foreground chips, then the sixteen ANSI colors the terminal
    /// renders with, then the name (the custom row carries its own
    /// check mark; the adapter's default indicator is suppressed
    /// with the default layout).
    fn render_item(
        &self,
        ix: IndexPath,
        item: &SchemeRow,
        checked: bool,
        _window: &mut gpui::Window,
        _cx: &mut App,
    ) -> Option<gpui::AnyElement> {
        let preview = self.0.get(ix.row).map_or(
            SchemePreview {
                background: None,
                foreground: None,
                ansi: [None; 16],
            },
            |(_, preview)| *preview,
        );
        let mut strip = h_flex().gap(px(SCHEME_CHIP_GAP_PX)).flex_shrink_0();
        for color in preview
            .background
            .into_iter()
            .chain(preview.foreground)
            .chain(preview.ansi.iter().flatten().copied())
        {
            strip = strip.child(
                gpui::div()
                    .w(px(SCHEME_CHIP_W_PX))
                    .h(px(SCHEME_CHIP_H_PX))
                    .rounded_sm()
                    .bg(hsla(color)),
            );
        }
        let mut row = h_flex()
            .w_full()
            .min_w_0()
            .px_2()
            .py_1()
            .gap_2()
            .child(strip)
            // The name truncates with an ellipsis inside its room
            // rather than clipping away — the row always identifies
            // its scheme.
            .child(gpui::div().min_w_0().truncate().child(item.0));
        if checked {
            row = row.child("✓");
        }
        Some(row.into_any_element())
    }
}

impl SearchableListItem for SchemeRow {
    type Value = &'static str;

    fn title(&self) -> SharedString {
        SharedString::from(self.0)
    }

    fn value(&self) -> &&'static str {
        &self.0
    }
}

// --- the components ---

/// The font pick callback's shape: the family chosen.
pub(crate) type FontPickFn = Rc<dyn Fn(String, &mut App)>;

/// The scheme pick callback's shape: the registry name chosen.
pub(crate) type SchemePickFn = Rc<dyn Fn(&'static str, &mut App)>;

/// The font picker's handle: the Kit select's state with the pick
/// subscription owned beside it (a dropped subscription is a dead
/// picker). The host renders `Select::new(&handle.state)`.
pub(crate) struct FontPickerHandle {
    pub state: Entity<SelectState<FontFamilies>>,
    /// Kept alive: dropping the subscription is a dead picker.
    subscription: gpui::Subscription,
}

/// The scheme picker's handle, [`FontPickerHandle`]'s shape.
pub(crate) struct SchemePickerHandle {
    pub state: Entity<SelectState<Schemes>>,
    /// Kept alive: dropping the subscription is a dead picker.
    subscription: gpui::Subscription,
}

/// The font picker: the Kit select over the family rows, seeded on
/// the stored family (the synthetic leading row covering a value the
/// scan lacks), every confirm delivered through `on_pick`.
pub(crate) fn font_picker(
    families: &[String],
    stored: &str,
    on_pick: &FontPickFn,
    window: &mut gpui::Window,
    cx: &mut App,
) -> FontPickerHandle {
    let delegate = FontFamilies(
        font_rows(families, stored)
            .into_iter()
            .map(FontFamilyRow)
            .collect(),
    );
    let selected = IndexPath::default().row(font_selected(families, stored));
    let state = cx.new(|cx| SelectState::new(delegate, Some(selected), window, cx));
    let on_pick = Rc::clone(on_pick);
    let subscription = cx.subscribe(&state, move |_, event: &SelectEvent<FontFamilies>, cx| {
        if let SelectEvent::Confirm(Some(family)) = event {
            on_pick(family.clone(), cx);
        }
    });
    FontPickerHandle {
        state,
        subscription,
    }
}

/// The scheme picker: the Kit select over the registry's rows,
/// seeded on the stored name when the registry knows it, every
/// confirm delivered through `on_pick`.
pub(crate) fn scheme_picker(
    stored: &str,
    on_pick: &SchemePickFn,
    window: &mut gpui::Window,
    cx: &mut App,
) -> SchemePickerHandle {
    let selected = scheme_selected(stored).map(|row| IndexPath::default().row(row));
    let state = cx.new(|cx| SelectState::new(Schemes::from_registry(), selected, window, cx));
    let on_pick = Rc::clone(on_pick);
    let subscription = cx.subscribe(&state, move |_, event: &SelectEvent<Schemes>, cx| {
        if let SelectEvent::Confirm(Some(name)) = event {
            on_pick(name, cx);
        }
    });
    SchemePickerHandle {
        state,
        subscription,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- the pure model ---

    // The scheme row's geometry budget: the strip plus the row chrome
    // must leave the name its room inside the scheme menu's width, or
    // the name clips away and the picker turns unidentifiable (the
    // drawer's no-text regression). The chrome numbers are the row's
    // own paddings plus the item wrapper's (its px_2) and the check
    // icon's reserved slot — each pinned here so a change that
    // re-balances them updates this test with it.
    #[test]
    fn the_scheme_row_leaves_the_name_room_inside_the_menu_width() {
        // The row's chrome: our px_2 pair and gap_2, the item
        // wrapper's px_2 pair, and the reserved check-icon slot
        // (xsmall icon plus the item's gap_x_1).
        const ROW_AND_ITEM_CHROME_PX: f32 = 8. * 2. + 8. + 8. * 2. + 14. + 4.;
        // The name's floor: the registry's longest name ("Tokyo
        // Night Storm") at the rows' text size, with slack.
        const NAME_FLOOR_PX: f32 = 120.;
        assert!(
            scheme_strip_px() + ROW_AND_ITEM_CHROME_PX + NAME_FLOOR_PX <= SCHEME_MENU_WIDTH_PX,
            "the strip ({}) plus chrome ({}) plus the name's floor ({}) exceeds the menu width ({}); \
             shrink the chips or widen SCHEME_MENU_WIDTH_PX",
            scheme_strip_px(),
            ROW_AND_ITEM_CHROME_PX,
            NAME_FLOOR_PX,
            SCHEME_MENU_WIDTH_PX,
        );
    }

    #[test]
    fn font_rows_list_the_families_with_the_stored_value_as_a_synthetic_leader_when_unmatched() {
        let families = vec!["DejaVu Sans Mono".to_string(), "Noto Sans Mono".to_string()];
        // A matching stored name: no leader, the families alone.
        assert_eq!(font_rows(&families, "Noto Sans Mono"), families);
        // An unmatched stored value (hand-edited, uninstalled): its
        // own leading row — visible and selectable, never dropped.
        let rows = font_rows(&families, "TerminessTTF Mono");
        assert_eq!(
            rows,
            vec![
                "TerminessTTF Mono".to_string(),
                "DejaVu Sans Mono".to_string(),
                "Noto Sans Mono".to_string(),
            ]
        );
    }

    #[test]
    fn font_selected_is_the_matching_row_else_the_leader() {
        let families = vec!["DejaVu Sans Mono".to_string(), "Noto Sans Mono".to_string()];
        assert_eq!(font_selected(&families, "Noto Sans Mono"), 1);
        // Unmatched selects the synthetic leader at 0; the match is
        // exact and case-sensitive (a case variant is a different
        // family).
        assert_eq!(font_selected(&families, "Zed Mono"), 0);
        assert_eq!(font_selected(&families, "noto sans mono"), 0);
    }

    #[test]
    fn font_pick_commits_its_row_and_rejects_past_the_rows() {
        let families = vec!["DejaVu Sans Mono".to_string(), "Noto Sans Mono".to_string()];
        // A family row commits its family.
        assert_eq!(
            font_pick(&families, "Noto Sans Mono", 1),
            Some("Noto Sans Mono".to_string())
        );
        // The synthetic leader commits the stored name itself.
        assert_eq!(
            font_pick(&families, "Zed Mono", 0),
            Some("Zed Mono".to_string())
        );
        // Past the rows (the system's list changed under the open
        // popover): no write, never an invented value.
        assert_eq!(font_pick(&families, "Zed Mono", 3), None);
    }

    #[test]
    fn scheme_selected_resolves_the_registry_row_exactly() {
        let names = scheme_names();
        assert_eq!(scheme_selected(names[0]), Some(0));
        // A name the registry lacks selects nothing — the control
        // falls to its placeholder, never an invented position.
        assert_eq!(scheme_selected("not a scheme"), None);
    }

    #[test]
    fn scheme_pick_commits_the_registry_name_and_rejects_past_the_registry() {
        let names = scheme_names();
        assert_eq!(scheme_pick(0), Some(names[0]));
        assert_eq!(scheme_pick(names.len()), None);
    }

    #[test]
    fn the_schemes_delegate_lists_the_whole_registry_in_registry_order() {
        let names = scheme_names();
        let delegate = Schemes::from_registry();
        assert_eq!(delegate.items_count(0), names.len());
        for (row, name) in names.iter().enumerate() {
            assert_eq!(
                delegate
                    .item(IndexPath::default().row(row))
                    .map(|item| item.0),
                Some(*name)
            );
        }
    }

    // --- the components (the Kit's own test harness) ---

    #[gpui::test]
    fn the_font_picker_seeds_on_the_stored_family_and_delivers_picks(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let families = vec!["DejaVu Sans Mono".to_string(), "Noto Sans Mono".to_string()];
        let picked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let sink = std::rc::Rc::clone(&picked);
        let on_pick: FontPickFn = std::rc::Rc::new(move |family: String, _| {
            sink.borrow_mut().push(family);
        });
        let handle =
            cx.update(|window, cx| font_picker(&families, "Noto Sans Mono", &on_pick, window, cx));
        // The subscription activates on the deferred pass after its
        // creation — park first, exactly the frames the popover's own
        // confirm rides in production.
        cx.run_until_parked();
        cx.update(|_, cx| {
            // Seeded on the stored family's row.
            assert_eq!(
                handle.state.read(cx).selected_value(),
                Some(&"Noto Sans Mono".to_string())
            );
            // A confirm reaches the pick callback with the value —
            // the emit is delivered on the update's closing effect
            // flush, so the assertion waits for the update to end.
            handle.state.update(cx, |_, cx| {
                cx.emit(SelectEvent::Confirm(Some("DejaVu Sans Mono".to_string())));
            });
        });
        assert_eq!(
            *picked.borrow(),
            vec!["DejaVu Sans Mono".to_string()],
            "the confirm must reach the pick callback"
        );
    }

    #[gpui::test]
    fn the_scheme_picker_seeds_on_the_stored_name_and_delivers_picks(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let names = scheme_names();
        let picked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<&'static str>::new()));
        let sink = std::rc::Rc::clone(&picked);
        let on_pick: SchemePickFn = std::rc::Rc::new(move |name: &'static str, _| {
            sink.borrow_mut().push(name);
        });
        let handle = cx.update(|window, cx| scheme_picker(names[0], &on_pick, window, cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(handle.state.read(cx).selected_value(), Some(&names[0]));
            handle.state.update(cx, |_, cx| {
                cx.emit(SelectEvent::Confirm(Some(names[1])));
            });
        });
        assert_eq!(*picked.borrow(), vec![names[1]]);
    }
}
