// SPDX-License-Identifier: GPL-3.0-only

//! The overlay lane's view-model (gpui-parity Task 1; suggest-overlay
//! spec): the view-shaped policy the pane consumes — the libcosmic
//! widget's own pure helpers (`accept_kind` and the visible-row
//! window) re-stated over GPUI's keystroke shape, the lane's routing
//! table, the opener's key gate, the menu's chrome and its paint
//! bundle, and the panel's placement. Core owns the model
//! ([`dropterm::core::terminal::suggest`]); this module owns only the
//! view-shaped decisions, headless-tested beside the input
//! translation's pins.
//!
//! The ordering contract these helpers serve (the libcosmic
//! widget's, verbatim): the menu's routing runs before the keybind
//! swallow and every mapping — a showing menu outranks them all —
//! then the accept keys check the engine's showing, and only then
//! does the press become bytes.

use std::sync::Arc;

use dropterm::core::config::DropTermConfig;
use dropterm::core::terminal::Terminal;
use dropterm::core::terminal::TerminalOptions;
use dropterm::core::terminal::suggest::{self, AcceptKind, MenuKey, SuggestionSource};
use gpui::{Keystroke, Modifiers};

/// The autosuggest soft keys (the libcosmic `accept_kind`): bare
/// Right accepts the whole showing suggestion, Alt+Right one
/// fish-shaped word — only those two chords, and only while the
/// engine actually shows a suggestion (checked by the caller).
/// Every other Right chord — Ctrl/Shift/Super-bearing, the shells'
/// word-jump family — and every other key reads `None` and falls
/// through to the existing mapping untouched.
pub(crate) fn accept_kind(key: &str, modifiers: Modifiers) -> Option<AcceptKind> {
    if key != "right" {
        return None;
    }
    let bare = !(modifiers.control || modifiers.alt || modifiers.shift || modifiers.platform);
    let alt_only = modifiers.alt && !(modifiers.control || modifiers.shift || modifiers.platform);
    if bare {
        Some(AcceptKind::Full)
    } else if alt_only {
        Some(AcceptKind::Word)
    } else {
        None
    }
}

/// The completion menu's scoped-key verdict (the libcosmic
/// `MenuRouting`, same rules): the claimed keys — bare up/down/enter/
/// escape, Tab, and Shift+Tab — are the menu's alone while it shows;
/// every other press closes it first and then processes normally
/// (the ghost's accepts included). `None` is no menu routing at all —
/// no menu shows — and the press is the shell's verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuRouting {
    /// The press is the menu's: navigate, fill, accept, or close.
    Consume(MenuKey),
    /// Bare Tab: the fill-or-cycle decision.
    FillOrCycle,
    /// The press is the shell's and the menu stays: an unmodified
    /// printable while the menu shows — the as-you-type edit keeps
    /// it alive, the rows recompute engine-side (suggest-parity
    /// spec, record finding 4).
    KeepOpen,
    /// The press closes the menu, then processes normally.
    CloseThenProcess,
}

/// The scoped routing itself — pure, pinned headless against the
/// libcosmic rules (the same table in GPUI's key names: `"up"`,
/// `"down"`, `"enter"`, `"escape"`). `key_char` is the
/// keystroke's produced character — the printable test's honest
/// input, shift-bearing capitals included (an edit is an edit; only
/// the command modifiers ctrl/alt/super exempt a key). `equal_line`
/// is the engine's two-stage Enter pre-check — the typed line
/// already equals the selection — consulted only for a bare Enter,
/// which then closes-and-processes so the bytes run the command.
pub(crate) fn menu_routing(
    key: &str,
    modifiers: Modifiers,
    open: bool,
    key_char: Option<char>,
    equal_line: bool,
) -> Option<MenuRouting> {
    if !open {
        return None;
    }
    // The command modifiers exempt a key from the menu's claims —
    // chords are the shell's. Shift on a printable is not a command
    // modifier: an edit is an edit.
    let no_command_mods = !(modifiers.control || modifiers.alt || modifiers.platform);
    let unmodified = no_command_mods && !modifiers.shift;
    if unmodified {
        match key {
            "up" => return Some(MenuRouting::Consume(MenuKey::Up)),
            "down" => return Some(MenuRouting::Consume(MenuKey::Down)),
            "enter" => {
                // The two-stage Enter (suggest-parity spec, record
                // finding 4): an equal line closes-and-processes so
                // the Enter bytes run the command; a differing line
                // accepts the suffix and closes.
                return Some(if equal_line {
                    MenuRouting::CloseThenProcess
                } else {
                    MenuRouting::Consume(MenuKey::Accept)
                });
            }
            "escape" => return Some(MenuRouting::Consume(MenuKey::Other)),
            _ => {}
        }
    }
    // Tab is the menu's while it shows (the lift, tab-surface OQ-1):
    // bare Tab fills or cycles, Shift+Tab steps backwards. The
    // shell's own completion has the press only while the menu is
    // closed, where the routing reads None and the bytes flow.
    if no_command_mods && key == "tab" {
        return Some(if modifiers.shift {
            MenuRouting::Consume(MenuKey::Up)
        } else {
            MenuRouting::FillOrCycle
        });
    }
    // The as-you-type edit (record finding 4): a printable keeps the
    // menu alive — the key's bytes flow to the PTY and the rows
    // recompute on the echo's update (SP3's gate).
    if no_command_mods && key_char.is_some() {
        return Some(MenuRouting::KeepOpen);
    }
    Some(MenuRouting::CloseThenProcess)
}

/// The conditioned opener's key gate (tab-surface OQ-1): bare Tab
/// alone may open the closed menu — the opener is SP2's own, and
/// Shift+Tab stays the shell's back-tab here (its backwards claim is
/// the showing menu's, in [`menu_routing`]). Whether the line
/// qualifies is the engine's answer, not this gate's.
pub(crate) fn opener_claims_tab(keystroke: &Keystroke) -> bool {
    let modifiers = &keystroke.modifiers;
    keystroke.key == "tab"
        && !(modifiers.control || modifiers.alt || modifiers.shift || modifiers.platform)
}

/// The menu overlay's visible-row window (the libcosmic shape over
/// the pane's row budget, suggest-overlay SO-10): at most
/// `visible_rows` rows starting at the scroll offset, fewer only when
/// the list itself is shorter.
pub(crate) fn menu_visible_window(
    scroll: usize,
    rows: usize,
    visible_rows: usize,
) -> std::ops::Range<usize> {
    let start = scroll.min(rows);
    start..(start + visible_rows.max(1)).min(rows)
}

/// The menu chrome's toggles (rich-input RI-7): the RI-1 keys as
/// one pure bundle, the pane's options folded once per paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the RI-1 keys' fold: each bool is its own serialized config field, regrouping changes the settings contract"
)]
pub(crate) struct MenuChrome {
    pub(crate) descriptions: bool,
    pub(crate) badges: bool,
    pub(crate) icons: bool,
    pub(crate) argument_hint: bool,
    pub(crate) compact_rows: bool,
}

#[cfg(test)]
impl MenuChrome {
    /// Every element on, comfortable rows — the defaults' fold.
    pub(crate) const fn all_on() -> Self {
        Self {
            descriptions: true,
            badges: true,
            icons: true,
            argument_hint: true,
            compact_rows: false,
        }
    }
}

/// One rendered menu row (rich-input RI-7): the mock's row model —
/// the marked label (the typed prefix, matched case-insensitively,
/// the paint bolds), the badge vocabulary word, the corpus
/// description, and the icon glyph — pure data for the paint and
/// the pins; each element is `None` when its toggle is off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MenuRowModel {
    pub(crate) label: String,
    /// How many leading CHARS of the label the typed segment
    /// matched (the mock's marking rule).
    pub(crate) marked: usize,
    /// The badge vocabulary: cmd/flag/path/hist/env/sub.
    pub(crate) badge: Option<&'static str>,
    pub(crate) description: Option<String>,
    pub(crate) icon: Option<char>,
}

/// The badge vocabulary word a row carries (rich-input RI-7): a
/// `-`-prefixed label is a flag whatever its source; otherwise the
/// provider decides — catalog cmd, specs subcommand sub, path,
/// history hist, env; the generator's value sets (git branches,
/// tags) read as the command's own sub vocabulary.
pub(crate) fn badge_of(source: SuggestionSource, label: &str) -> &'static str {
    if label.starts_with('-') {
        return "flag";
    }
    match source {
        SuggestionSource::Catalog => "cmd",
        SuggestionSource::Specs | SuggestionSource::Generator => "sub",
        SuggestionSource::Path => "path",
        SuggestionSource::History => "hist",
        SuggestionSource::Env => "env",
    }
}

/// The badge vocabulary [`badge_of`] draws from. The menu's badge
/// column is the widest of these words, so the vocabulary has one
/// home and the column can never miss a word.
pub(crate) const BADGE_WORDS: [&str; 6] = ["cmd", "flag", "path", "hist", "env", "sub"];

/// The icon glyph per badge (rich-input RI-7): the mock's lucide
/// set as single glyphs — refined against the mock at the RI-C
/// review.
pub(crate) fn icon_for(badge: &str) -> char {
    match badge {
        "cmd" => '\u{276f}',
        "flag" => '\u{2013}',
        "path" => '/',
        "hist" => '\u{21bb}',
        "env" => '$',
        _ => '\u{25b8}',
    }
}

/// The menu's rendered rows (rich-input RI-7): the engine's
/// candidates into the mock's row model under the toggles.
pub(crate) fn menu_row_models(
    rows: &[dropterm::core::terminal::suggest::MenuCandidate],
    typed: &str,
    chrome: MenuChrome,
) -> Vec<MenuRowModel> {
    rows.iter()
        .map(|row| {
            let badge = badge_of(row.source, &row.label);
            let marked = if !typed.is_empty()
                && row.label.to_lowercase().starts_with(&typed.to_lowercase())
            {
                typed.chars().count()
            } else {
                0
            };
            MenuRowModel {
                label: row.label.clone(),
                marked,
                badge: chrome.badges.then_some(badge),
                description: chrome
                    .descriptions
                    .then(|| row.description.clone())
                    .flatten(),
                icon: chrome.icons.then(|| icon_for(badge)),
            }
        })
        .collect()
}

/// The overlay menu's paint bundle (suggest-overlay SO-6; the chrome
/// rich-input RI-7 built): the rendered rows under the toggles, the
/// selection and scroll the engine holds, and the hint line —
/// captured in the pane's paint under one engine read, painted over
/// the prompt row.
pub(crate) struct MenuPaint {
    pub(crate) rows: Vec<MenuRowModel>,
    pub(crate) selected: usize,
    pub(crate) scroll: usize,
    pub(crate) hint: Option<(String, Option<String>)>,
    /// The pane's row budget the window follows
    /// ([`Terminal::menu_visible_rows`]).
    pub(crate) visible_rows: usize,
    pub(crate) chrome: MenuChrome,
}

impl MenuPaint {
    /// Capture the menu's paint state off the grid: `None` when the
    /// overlays stand aside or the menu stands closed (or is empty),
    /// the paint's whole gate. The menu is the live prompt's: a
    /// foreground program, an unsure prompt, an unfocused or
    /// scrolled-back pane take it down with the ghost. The typed
    /// text's last word marks each label, its first word asks the
    /// hint.
    pub(crate) fn capture(engine: &Terminal, chrome: MenuChrome) -> Option<Self> {
        if !engine.overlays_live() || !engine.menu_is_open() {
            return None;
        }
        let typed_text = engine.typed_text();
        let typed = suggest::trailing_token(&typed_text);
        let rows = menu_row_models(engine.menu_rows(), typed, chrome);
        if rows.is_empty() {
            return None;
        }
        Some(Self {
            rows,
            selected: engine.menu_selected(),
            scroll: engine.menu_scroll(),
            hint: engine.menu_hint_line(),
            visible_rows: engine.menu_visible_rows(),
            chrome,
        })
    }
}

// --- the menu's layout (suggest-overlay SO-11): one pure function ---
//
// Everything the paint knows about where the menu goes comes out of
// [`layout_menu`]: the panel, the hint line, the selection's tint, and
// every run's box. Every size is a function of the cell, so the menu
// scales with the font and carries no pixel of its own. Logical pixels
// in and out as plain `f32`; the paint wraps them in `Pixels`.

/// The panel's width in cells (OQ-5): sixty-seven cells is about 560
/// px at the default font, and a multiple of the cell follows zoom.
const PANEL_CELLS: f32 = 67.;

/// A rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

/// Where one text run goes: its box and the line it paints on. A
/// right-aligned run hugs the box's right edge (the paint offsets a
/// narrower run by the difference); a run wider than its box is cut
/// with an ellipsis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RunBox {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) line_h: f32,
    pub(crate) align_right: bool,
}

/// The two fixed columns, in logical pixels: the widest glyph of the
/// icon vocabulary and the widest word of the badge vocabulary, each
/// shaped once in the pane's font. Labels start past the first and
/// stop before the second, so they line up under one icon column
/// whatever their icons, and every badge ends on one right edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MenuMetrics {
    pub(crate) icon_column: f32,
    pub(crate) badge_column: f32,
}

/// One row's natural shaped widths: the typed prefix, the label's
/// remainder, and the description.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MenuRowWidths {
    pub(crate) marked: f32,
    pub(crate) label: f32,
    pub(crate) description: f32,
}

/// Everything the layout depends on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MenuLayoutInput<'a> {
    pub(crate) cell_w: f32,
    pub(crate) cell_h: f32,
    /// The chrome toggles: the density and which elements show.
    pub(crate) chrome: MenuChrome,
    pub(crate) metrics: MenuMetrics,
    /// The rows of the visible window, in display order.
    pub(crate) rows: &'a [MenuRowModel],
    /// Their widths, in the same order.
    pub(crate) widths: &'a [MenuRowWidths],
    /// The argument hint's shaped widths, when it shows: its name
    /// and its description.
    pub(crate) hint: Option<(f32, Option<f32>)>,
    /// The pane.
    pub(crate) pane: Rect,
    /// The prompt row's top edge.
    pub(crate) prompt_row_top: f32,
    /// The pane's row budget (SO-10): no more rows than this get a
    /// box, whatever the window carries.
    pub(crate) budget: usize,
    /// The selected row's index into `rows`.
    pub(crate) selected: Option<usize>,
}

/// One row's boxes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MenuRowLayout {
    pub(crate) icon: Option<RunBox>,
    /// The typed prefix.
    pub(crate) marked: RunBox,
    /// The label past the typed prefix: `None` when the prefix is
    /// the whole label or takes the whole width.
    pub(crate) label: Option<RunBox>,
    pub(crate) badge: Option<RunBox>,
    pub(crate) description: Option<RunBox>,
}

/// The menu's whole layout.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MenuLayout {
    pub(crate) panel: Rect,
    pub(crate) corner_radius: f32,
    pub(crate) border_width: f32,
    /// The argument hint line over the rows: its name and its
    /// description.
    pub(crate) hint: Option<(RunBox, Option<RunBox>)>,
    /// The selected row's tint.
    pub(crate) selection: Option<Rect>,
    pub(crate) rows: Vec<MenuRowLayout>,
}

/// The panel's fixed columns, the same for every row: the padding's
/// inner edges, the icon column and the badge column, and the text's
/// extent between them.
#[derive(Debug, Clone, Copy)]
struct Columns {
    inner_x: f32,
    inner_right: f32,
    icon_col: f32,
    badge_col: f32,
    text_x: f32,
    text_right: f32,
}

/// The menu's layout, pure. The cell sizes everything: a cell of
/// padding inside the panel and of margin outside it, a cell between
/// columns, an eighth of a cell over and under a row's lines (the
/// quarter cell of padding SO-10's row height carries), a cell per
/// text line. The panel is a multiple of the cell width, clamped to
/// the pane less its margins; it sits above the prompt row when it
/// fits there and otherwise below it, never outside the pane. At most
/// `budget` rows get a box.
#[must_use]
pub(crate) fn layout_menu(input: &MenuLayoutInput<'_>) -> MenuLayout {
    let MenuLayoutInput {
        cell_w,
        cell_h,
        chrome,
        metrics,
        rows,
        widths,
        hint,
        pane,
        prompt_row_top,
        budget,
        selected,
    } = *input;
    let pad_x = cell_w;
    let pad_y = cell_h * 0.125;
    let gap = cell_w;
    let line_h = cell_h;
    let row_h = dropterm::core::terminal::suggest::menu_row_height(cell_h, chrome.compact_rows);
    let hint_h = hint.map_or(0., |_| {
        dropterm::core::terminal::suggest::menu_hint_height(cell_h)
    });
    let count = rows.len().min(budget);
    #[expect(
        clippy::cast_precision_loss,
        reason = "the row count is the menu's budget (at most eight): exact in f32"
    )]
    let height = hint_h + row_h * count as f32;

    let panel = place_panel(
        pane,
        prompt_row_top,
        height,
        PanelSizes {
            cell_w,
            cell_h,
            pad_x,
        },
    );

    let columns = columns_of(&panel, chrome, metrics, pad_x, gap);
    let Columns {
        inner_x,
        inner_right,
        ..
    } = columns;

    let selection = selected.filter(|index| *index < count).map(|index| {
        #[expect(
            clippy::cast_precision_loss,
            reason = "the selected row's index is below the budget (at most eight): exact in f32"
        )]
        let y = panel.y + hint_h + row_h * index as f32;
        Rect {
            x: panel.x,
            y,
            w: panel.w,
            h: row_h,
        }
    });
    let hint = hint.map(|widths| {
        layout_hint(
            widths,
            panel.y + pad_y,
            inner_right,
            LineSizes {
                x: inner_x,
                line_h,
                gap,
            },
        )
    });

    #[expect(
        clippy::cast_precision_loss,
        reason = "a row's index is below the budget (at most eight): exact in f32"
    )]
    let rows_out = rows
        .iter()
        .zip(widths.iter())
        .take(count)
        .enumerate()
        .map(|(index, (row, widths))| {
            layout_row(
                row,
                widths,
                panel.y + hint_h + row_h * index as f32,
                LayoutSizes { pad_y, gap, line_h },
                chrome,
                &columns,
            )
        })
        .collect();

    MenuLayout {
        panel,
        corner_radius: cell_h * 0.3,
        border_width: cell_h * 0.05,
        hint,
        selection,
        rows: rows_out,
    }
}

/// The vertical sizes a row's lines sit by.
#[derive(Debug, Clone, Copy)]
struct LayoutSizes {
    pad_y: f32,
    gap: f32,
    line_h: f32,
}

/// One row's boxes under the panel's columns, pure. A comfortable
/// row is two lines, the label over the description; a compact row is
/// one, the description right-aligned before the badge. The typed
/// prefix takes the text's width first and the remainder has what is
/// left; an element whose toggle is off, or that has no room, gets no
/// box.
fn layout_row(
    row: &MenuRowModel,
    widths: &MenuRowWidths,
    row_top: f32,
    sizes: LayoutSizes,
    chrome: MenuChrome,
    columns: &Columns,
) -> MenuRowLayout {
    let LayoutSizes { pad_y, gap, line_h } = sizes;
    let first_line = row_top + pad_y;
    let text_w = (columns.text_right - columns.text_x).max(0.);
    let icon = row.icon.filter(|_| chrome.icons).map(|_| RunBox {
        x: columns.inner_x,
        y: first_line,
        width: columns.icon_col,
        line_h,
        align_right: false,
    });
    let marked_w = widths.marked.min(text_w);
    let marked = RunBox {
        x: columns.text_x,
        y: first_line,
        width: marked_w,
        line_h,
        align_right: false,
    };
    let label = (marked_w < text_w && row.label.chars().count() > row.marked).then_some(RunBox {
        x: columns.text_x + marked_w,
        y: first_line,
        width: text_w - marked_w,
        line_h,
        align_right: false,
    });
    let badge = row.badge.filter(|_| chrome.badges).map(|_| RunBox {
        x: columns.inner_right - columns.badge_col,
        y: first_line,
        width: columns.badge_col,
        line_h,
        align_right: true,
    });
    let description = row
        .description
        .as_ref()
        .filter(|_| chrome.descriptions)
        .and_then(|_| {
            if chrome.compact_rows {
                // Right-aligned before the badge: the room between the
                // label's visible end and the text's right edge, a gap
                // apart.
                let left = columns.text_x + (widths.marked + widths.label).min(text_w) + gap;
                let width = columns.text_right - left;
                (width > 0.).then_some(RunBox {
                    x: left,
                    y: first_line,
                    width,
                    line_h,
                    align_right: true,
                })
            } else {
                Some(RunBox {
                    x: columns.text_x,
                    y: first_line + line_h,
                    width: text_w,
                    line_h,
                    align_right: false,
                })
            }
        });
    MenuRowLayout {
        icon,
        marked,
        label,
        badge,
        description,
    }
}

/// The sizes the panel's box is placed by.
#[derive(Debug, Clone, Copy)]
struct PanelSizes {
    cell_w: f32,
    cell_h: f32,
    pad_x: f32,
}

/// The panel's box, pure: a multiple of the cell width clamped to the
/// pane less its margins, above the prompt row when it fits there and
/// otherwise below it, and never outside the pane.
fn place_panel(pane: Rect, prompt_row_top: f32, height: f32, sizes: PanelSizes) -> Rect {
    let PanelSizes {
        cell_w,
        cell_h,
        pad_x,
    } = sizes;
    let width = (PANEL_CELLS * cell_w).min(pane.w - pad_x * 2.).max(0.);
    let above = prompt_row_top - height;
    let wanted = if above >= pane.y {
        above
    } else {
        prompt_row_top + cell_h
    };
    let lowest = (pane.y + pane.h - height).max(pane.y);
    Rect {
        x: pane.x + pad_x,
        y: wanted.clamp(pane.y, lowest),
        w: width,
        h: height,
    }
}

/// The horizontal sizes a text line lays out by: where it starts, how
/// tall it is, and the gap that follows it.
#[derive(Debug, Clone, Copy)]
struct LineSizes {
    x: f32,
    line_h: f32,
    gap: f32,
}

/// The argument hint line, pure: the command's name from the panel's
/// inner left edge, its description a gap after it, each cut to the
/// panel's inner width.
fn layout_hint(
    widths: (f32, Option<f32>),
    line_y: f32,
    inner_right: f32,
    sizes: LineSizes,
) -> (RunBox, Option<RunBox>) {
    let LineSizes { x, line_h, gap } = sizes;
    let inner_w = (inner_right - x).max(0.);
    let name = RunBox {
        x,
        y: line_y,
        width: widths.0.min(inner_w),
        line_h,
        align_right: false,
    };
    let description = widths.1.map(|_| {
        let x = x + name.width + gap;
        RunBox {
            x,
            y: line_y,
            width: (inner_right - x).max(0.),
            line_h,
            align_right: false,
        }
    });
    (name, description)
}

/// The terminal options a pane renders with (the libcosmic
/// `terminal_options_for` port, profile-less — the GPUI build has no
/// profile spawn path): the global suggest config keys and the
/// app-loaded completions table ride every construction and every
/// live `set_config` pass, so a toggle flip applies at the next
/// update without a respawn.
pub(crate) fn pane_options(
    config: &DropTermConfig,
    specs_table: Option<&Arc<[suggest::SpecCommand]>>,
) -> TerminalOptions {
    TerminalOptions {
        font_size: config.font_size,
        // The mode truth rides every construction and set_config
        // pass like the toggles (rich-input RI-3); the per-profile
        // resolution lands where the spawn knows its profile.
        input_mode: config.input_mode,
        input_descriptions: config.input_descriptions,
        input_badges: config.input_badges,
        input_icons: config.input_icons,
        input_argument_hint: config.input_argument_hint,
        input_compact_rows: config.input_compact_rows,
        // The folded surfaces read the decoupled rich-input keys
        // (rich-input RI-1's cull): the internal option names keep
        // their shape, their truth is the keys', roles unchanged.
        autosuggestions_enabled: config.input_ghost,
        images_enabled: config.images_enabled,
        autosuggestions_paths_enabled: config.autosuggestions_paths_enabled,
        autosuggestions_commands_enabled: config.autosuggestions_commands_enabled,
        autosuggestions_menu_enabled: config.input_menu,
        autosuggestions_menu_auto: config.input_auto_popup,
        autosuggestions_history_context: config.autosuggestions_history_context,
        autosuggestions_delay_ms: config.autosuggestions_delay_ms,
        autosuggestions_aliases: config.autosuggestions_aliases.clone(),
        autosuggestions_specs_table: specs_table.cloned().unwrap_or_else(suggest::empty_specs),
        autosuggestions_specs_loading: specs_table.is_none(),
        spawn_home: std::env::var("HOME").ok(),
        spawn_path: std::env::var("PATH").ok(),
        // The env-var tier's name set (suggest-depth spec, LG4):
        // the pty exec's env keys, sorted and deduped — names only,
        // values never read. Spawn truth: the engine copies it once
        // at construction and never re-reads it.
        spawn_env_names: {
            let mut names: Vec<String> = std::env::vars().map(|(name, _)| name).collect();
            names.sort_unstable();
            names.dedup();
            Arc::from(names)
        },
        ..TerminalOptions::default()
    }
}

// The spawn truth lives in core now (py-5): `dropterm::core::spawn::
// spawned_as_program` resolves the command's program — wrapper-free —
// through the one resolution both frontends share, superseding this
// module's explicit-or-default shape.

/// The panel's columns, pure: the icon column at the inner left edge
/// (nothing with the icons off), the badge column at the inner right
/// (nothing with the badges off), and the text between them, a gap
/// clear of each.
fn columns_of(
    panel: &Rect,
    chrome: MenuChrome,
    metrics: MenuMetrics,
    pad_x: f32,
    gap: f32,
) -> Columns {
    let inner_x = panel.x + pad_x;
    let inner_right = panel.x + panel.w - pad_x;
    let icon_col = if chrome.icons {
        metrics.icon_column
    } else {
        0.
    };
    let badge_col = if chrome.badges {
        metrics.badge_column
    } else {
        0.
    };
    let text_x = inner_x + if icon_col > 0. { icon_col + gap } else { 0. };
    let text_right = if badge_col > 0. {
        inner_right - badge_col - gap
    } else {
        inner_right
    };
    Columns {
        inner_x,
        inner_right,
        icon_col,
        badge_col,
        text_x,
        text_right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare() -> Modifiers {
        Modifiers::default()
    }

    fn alt() -> Modifiers {
        Modifiers {
            alt: true,
            ..Modifiers::default()
        }
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            control: true,
            ..Modifiers::default()
        }
    }

    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Modifiers::default()
        }
    }

    fn super_key() -> Modifiers {
        Modifiers {
            platform: true,
            ..Modifiers::default()
        }
    }

    #[test]
    fn accept_keys_are_exactly_bare_and_alt_right() {
        assert_eq!(accept_kind("right", bare()), Some(AcceptKind::Full));
        assert_eq!(accept_kind("right", alt()), Some(AcceptKind::Word));
        for modifiers in [ctrl(), shift(), super_key()] {
            assert_eq!(
                accept_kind("right", modifiers),
                None,
                "modified Right is the shell's"
            );
        }
        assert_eq!(
            accept_kind(
                "right",
                Modifiers {
                    alt: true,
                    shift: true,
                    ..Modifiers::default()
                }
            ),
            None,
            "Alt+Shift+Right is the shells' word-jump family"
        );
        for key in ["left", "up", "a", "space", "enter"] {
            assert_eq!(accept_kind(key, bare()), None, "only Right accepts: {key}");
        }
    }

    fn key(modifiers: Modifiers, key: &str, typed: Option<&str>) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.to_string(),
            key_char: typed.map(str::to_string),
        }
    }

    #[test]
    fn a_closed_menu_routes_nothing() {
        // No menu shows: every press reads None, and the press is
        // the shell's verbatim.
        for (key, modifiers) in [
            ("up", bare()),
            ("down", bare()),
            ("tab", bare()),
            ("enter", bare()),
            ("escape", bare()),
            ("space", ctrl()),
            ("a", bare()),
        ] {
            assert_eq!(
                menu_routing(key, modifiers, false, None, false),
                None,
                "closed menu routes nothing: {key}"
            );
        }
    }

    #[test]
    fn an_open_menu_claims_its_bare_keys() {
        // The menu's own keys: navigate, accept, close — bare only.
        assert_eq!(
            menu_routing("up", bare(), true, None, false),
            Some(MenuRouting::Consume(MenuKey::Up))
        );
        assert_eq!(
            menu_routing("down", bare(), true, None, false),
            Some(MenuRouting::Consume(MenuKey::Down))
        );
        assert_eq!(
            menu_routing("enter", bare(), true, None, false),
            Some(MenuRouting::Consume(MenuKey::Accept))
        );
        assert_eq!(
            menu_routing("escape", bare(), true, None, false),
            Some(MenuRouting::Consume(MenuKey::Other))
        );
    }

    #[test]
    fn every_press_an_open_menu_does_not_claim_closes_it_first() {
        // VS Code's HasFocusedSuggestion shape: a modified claimed
        // key, a bare unclaimed non-printable, and a bound action
        // chord all read CloseThenProcess — the press still processes
        // normally below (the ghost's accepts included).
        for (key, modifiers, key_char) in [
            ("up", ctrl(), None),
            ("up", shift(), None),
            ("home", bare(), None),
            ("c", ctrl(), None),
        ] {
            assert_eq!(
                menu_routing(key, modifiers, true, key_char, false),
                Some(MenuRouting::CloseThenProcess),
                "unclaimed press closes: {key}"
            );
        }
    }

    #[test]
    fn a_printable_keeps_the_menu_open() {
        // The as-you-type rule (suggest-parity spec, record finding
        // 4): an unmodified printable is an edit, not a close — the
        // menu stays and the key's bytes flow to the PTY; the rows
        // recompute on the echo's update.
        assert_eq!(
            menu_routing("a", bare(), true, Some('a'), false),
            Some(MenuRouting::KeepOpen)
        );
        assert_eq!(
            menu_routing("space", bare(), true, Some(' '), false),
            Some(MenuRouting::KeepOpen)
        );
    }

    #[test]
    fn a_capital_is_still_a_printable_edit() {
        // Shift is not a command modifier on a printable: typing a
        // capital keeps the menu (an edit is an edit); only
        // ctrl/alt/super exempt a key.
        assert_eq!(
            menu_routing("a", shift(), true, Some('A'), false),
            Some(MenuRouting::KeepOpen)
        );
    }

    #[test]
    fn a_bare_tab_fills_or_cycles_while_the_menu_shows() {
        // The lift (rich-input spec; on the overlay lane from
        // suggest-overlay SO-5): Tab is the menu's fill while it
        // shows — the longest common prefix of the rows' own
        // continuations, else a cycle of the selection. The shell's
        // completion is the press's only while the menu is closed.
        assert_eq!(
            menu_routing("tab", bare(), true, None, false),
            Some(MenuRouting::FillOrCycle)
        );
    }

    #[test]
    fn shift_tab_steps_backwards_while_the_menu_shows() {
        // Shift+Tab is the backwards step while the menu shows
        // (tab-surface OQ-1, SP2's shape); its bytes are the shell's
        // back-tab only while the menu is closed.
        assert_eq!(
            menu_routing("tab", shift(), true, None, false),
            Some(MenuRouting::Consume(MenuKey::Up))
        );
    }

    #[test]
    fn enter_on_an_equal_line_runs_instead() {
        // The two-stage Enter (suggest-parity spec, record finding
        // 4): the typed line already equals the selection — the
        // routing closes-and-processes so the Enter bytes reach the
        // shell and the command runs.
        assert_eq!(
            menu_routing("enter", bare(), true, Some('\r'), true),
            Some(MenuRouting::CloseThenProcess)
        );
    }

    #[test]
    fn enter_on_a_differing_line_accepts() {
        // The accept stage: a differing line's Enter is the menu's —
        // the suffix lands and the menu closes.
        assert_eq!(
            menu_routing("enter", bare(), true, Some('\r'), false),
            Some(MenuRouting::Consume(MenuKey::Accept))
        );
    }

    #[test]
    fn the_conditioned_opener_is_bare_tab_alone() {
        // Bare Tab is the opener's key while the menu is closed...
        assert!(opener_claims_tab(&key(bare(), "tab", None)));
        // ...and nothing else claims: Shift+Tab is the shell's
        // back-tab here (tab-surface OQ-1 — its backwards claim is
        // the showing menu's), and the command modifiers were never
        // the opener's.
        assert!(!opener_claims_tab(&key(shift(), "tab", None)));
        assert!(!opener_claims_tab(&key(ctrl(), "tab", None)));
        assert!(!opener_claims_tab(&key(alt(), "tab", None)));
        assert!(!opener_claims_tab(&key(bare(), "a", Some("a"))));
    }

    #[test]
    fn the_menu_window_covers_the_visible_budget() {
        // The pane's budget (suggest-overlay SO-10) from the scroll
        // offset, fewer only when the list is shorter. A scroll at
        // the list's own length (the engine's bounds never produce
        // it) is the ported shape's empty window, not a clamp of its
        // own; a zero budget reads as one, the model's own floor.
        assert_eq!(menu_visible_window(0, 12, 8), 0..8);
        assert_eq!(menu_visible_window(4, 12, 8), 4..12);
        assert_eq!(
            menu_visible_window(0, 12, 3),
            0..3,
            "a budget of three shows three"
        );
        assert_eq!(menu_visible_window(3, 12, 3), 3..6);
        assert_eq!(menu_visible_window(3, 5, 3), 3..5);
        assert_eq!(menu_visible_window(9, 9, 8), 9..9);
        assert_eq!(menu_visible_window(0, 12, 0), 0..1);
    }

    // --- the menu's layout (suggest-overlay SO-11) ---

    /// The default cell: 8.4 by 20 px at the 14 px font.
    const CELL: (f32, f32) = (8.4, 20.);

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    /// Four rows whose icons, badges and descriptions all differ: the
    /// vocabularies the columns have to line up across.
    fn four_rows() -> Vec<MenuRowModel> {
        use dropterm::core::terminal::suggest::SuggestionSource as S;
        menu_row_models(
            &[
                candidate("git", S::Catalog, Some("the stupid content tracker")),
                candidate("--short", S::Specs, None),
                candidate("src/", S::Path, Some("the source tree")),
                candidate("sleep 30", S::History, None),
            ],
            "",
            MenuChrome::all_on(),
        )
    }

    /// A row's widths at a cell, from a monospace estimate: every
    /// character one cell wide.
    fn widths_at(rows: &[MenuRowModel], cell_w: f32) -> Vec<MenuRowWidths> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a label's character count is far below f32's mantissa: exact"
        )]
        let width_of = |text: &str| text.chars().count() as f32 * cell_w;
        rows.iter()
            .map(|row| {
                let marked: String = row.label.chars().take(row.marked).collect();
                let label: String = row.label.chars().skip(row.marked).collect();
                MenuRowWidths {
                    marked: width_of(&marked),
                    label: width_of(&label),
                    description: row.description.as_deref().map_or(0., width_of),
                }
            })
            .collect()
    }

    /// The layout's inputs at a cell, scaled by `scale` whole: a 900
    /// by 600 pane whose prompt row starts 400 px down, every toggle
    /// on, no hint, the budget left to the caller.
    fn input_over<'a>(
        rows: &'a [MenuRowModel],
        widths: &'a [MenuRowWidths],
        cell: (f32, f32),
        compact: bool,
        scale: f32,
    ) -> MenuLayoutInput<'a> {
        MenuLayoutInput {
            cell_w: cell.0 * scale,
            cell_h: cell.1 * scale,
            chrome: MenuChrome {
                compact_rows: compact,
                ..MenuChrome::all_on()
            },
            metrics: MenuMetrics {
                icon_column: cell.0 * scale,
                badge_column: 4. * cell.0 * scale,
            },
            rows,
            widths,
            hint: None,
            pane: rect(0., 0., 900. * scale, 600. * scale),
            prompt_row_top: 400. * scale,
            budget: usize::MAX,
            selected: None,
        }
    }

    /// The layout of `rows` over the default pane at `cell`.
    fn layout_over(rows: &[MenuRowModel], cell: (f32, f32), compact: bool) -> MenuLayout {
        let widths = widths_at(rows, cell.0);
        layout_menu(&input_over(rows, &widths, cell, compact, 1.))
    }

    #[test]
    fn a_comfortable_rows_label_and_description_never_intersect_at_any_cell_height() {
        use dropterm::core::terminal::suggest::menu_row_height;
        let rows = four_rows();

        for cell in [(8.4, 20.), (11., 26.5), (6., 15.), (16.8, 40.)] {
            let layout = layout_over(&rows, cell, false);
            let row_h = menu_row_height(cell.1, false);
            for (index, row) in layout.rows.iter().enumerate() {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a row's index is below the budget (at most eight): exact in f32"
                )]
                let top = layout.panel.y + row_h * index as f32;
                let label = row.marked;
                assert!(
                    label.y >= top && label.y + label.line_h <= top + row_h,
                    "the label's line sits inside its row at cell {cell:?}"
                );
                assert_eq!(
                    label.line_h, cell.1,
                    "a text line is one cell tall at cell {cell:?}"
                );
                if let Some(description) = row.description {
                    assert!(
                        description.y >= label.y + label.line_h,
                        "the description starts at or past the label's bottom at cell {cell:?}: \
                         label {label:?}, description {description:?}"
                    );
                    assert_eq!(
                        description.y,
                        label.y + label.line_h,
                        "the description takes the very next line at cell {cell:?}"
                    );
                    let text_width = label.width + row.label.map_or(0., |rest| rest.width);
                    assert_eq!(
                        (description.x, description.width),
                        (label.x, text_width),
                        "the description runs under the label, over the label's whole width"
                    );
                    assert!(
                        description.y + description.line_h <= top + row_h,
                        "the description's line sits inside its row at cell {cell:?}"
                    );
                }
            }
        }

        // The control, today's offsets at the default cell: a 34 px
        // row, the label at (row_h - font_px) / 2 and the description
        // at row_h - font_px - 4. They share fifteen pixels of height,
        // the owner's "the description is overlapping"; a layout built
        // on them fails the assertion above.
        let (row_h, font_px) = (34.0_f32, 14.0_f32 * 4.0 / 3.0);
        let label_top = (row_h - font_px) / 2.0;
        let description_top = row_h - font_px - 4.0;
        let overlap = label_top + font_px - description_top;
        assert!(
            description_top < label_top + font_px && (overlap - 15.0).abs() < 0.5,
            "the old offsets overlap by {overlap} px: the case the assertion above must fail"
        );
    }

    #[test]
    fn labels_line_up_under_one_icon_column_whatever_their_icons() {
        let rows = four_rows();
        let layout = layout_over(&rows, CELL, false);
        let one_x = layout.panel.x + CELL.0 + CELL.0 + CELL.0;
        for row in &layout.rows {
            let icon = row.icon.expect("every row carries its icon");
            assert_eq!(
                (icon.x, icon.width),
                (layout.panel.x + CELL.0, CELL.0),
                "one icon column: every glyph in the same box"
            );
            assert_eq!(
                row.marked.x, one_x,
                "every label starts at one x, its icon's glyph notwithstanding"
            );
        }
        let compact = layout_over(&rows, CELL, true);
        assert!(
            compact.rows.iter().all(|row| row.marked.x == one_x),
            "one label x at the compact density too"
        );
    }

    #[test]
    fn the_badge_ends_on_the_panels_right_edge_across_rows() {
        let layout = layout_over(&four_rows(), CELL, false);
        let badge_col = 4. * CELL.0;
        let right_edge = layout.panel.x + layout.panel.w - CELL.0;
        for row in &layout.rows {
            let badge = row.badge.expect("every row carries its badge");
            assert!(badge.align_right, "the badge hugs its column's right edge");
            assert_eq!(
                badge.x + badge.width,
                right_edge,
                "one badge edge across rows, whatever the word"
            );
            assert_eq!(badge.x, right_edge - badge_col, "a fixed badge column");
            let text_right = row
                .description
                .map_or(row.marked.x + row.marked.width, |description| {
                    description.x + description.width
                });
            assert!(
                text_right <= badge.x - CELL.0 + 0.001,
                "no label or description runs into the badge column"
            );
        }
    }

    #[test]
    fn the_panel_is_about_560_px_as_a_multiple_of_the_cell() {
        let rows = four_rows();
        let layout = layout_over(&rows, CELL, false);
        assert_eq!(layout.panel.w, 67. * CELL.0, "the width is whole cells");
        assert!(
            (layout.panel.w - 560.).abs() < 5.,
            "about 560 px at the default font: {}",
            layout.panel.w
        );
        assert_eq!(
            layout.panel.x, CELL.0,
            "the pane's left edge plus the margin"
        );
        // A pane narrower than the panel: clamped to the pane less its
        // margins, the margin holding on the left.
        let widths = widths_at(&rows, CELL.0);
        let narrow = layout_menu(&MenuLayoutInput {
            pane: rect(10., 0., 300., 600.),
            ..input_over(&rows, &widths, CELL, false, 1.)
        });
        assert_eq!(narrow.panel.w, 300. - 2. * CELL.0);
        assert_eq!(narrow.panel.x, 10. + CELL.0);
    }

    #[test]
    fn the_panel_sits_above_the_prompt_row_when_it_fits_and_below_otherwise() {
        use dropterm::core::terminal::suggest::menu_row_height;
        let rows = four_rows();
        let widths = widths_at(&rows, CELL.0);
        let row_h = menu_row_height(CELL.1, false);

        // Over a prompt row 400 px down: the panel fits above it, its
        // foot on the row's top.
        let above = layout_menu(&MenuLayoutInput {
            rows: &rows[..2],
            ..input_over(&rows[..2], &widths, CELL, false, 1.)
        });
        assert_eq!(above.panel.y, 400. - 2. * row_h);

        // A top prompt: no room over the screen's first row, the panel
        // under the prompt row.
        let below = layout_menu(&MenuLayoutInput {
            prompt_row_top: 0.,
            ..input_over(&rows[..2], &widths, CELL, false, 1.)
        });
        assert_eq!(below.panel.y, CELL.1, "the row under the prompt row");

        // Control: exactly the room above (the panel's height down the
        // pane) still goes above.
        let exact = layout_menu(&MenuLayoutInput {
            prompt_row_top: 2. * row_h,
            ..input_over(&rows[..2], &widths, CELL, false, 1.)
        });
        assert_eq!(exact.panel.y, 0.);

        // Never outside the pane: a pane without the room on either
        // side holds the panel at its top.
        let squeezed = layout_menu(&MenuLayoutInput {
            pane: rect(0., 0., 900., 40.),
            prompt_row_top: 0.,
            ..input_over(&rows, &widths, CELL, false, 1.)
        });
        assert!(
            squeezed.panel.y >= 0. && squeezed.panel.h > 40.,
            "a panel taller than the pane stays at its top: {:?}",
            squeezed.panel
        );
        assert_eq!(squeezed.panel.y, 0.);
    }

    #[test]
    fn the_layout_doubles_when_the_cell_does() {
        let rows = four_rows();
        let small_widths = widths_at(&rows, CELL.0);
        let small = layout_menu(&input_over(&rows, &small_widths, CELL, false, 1.));
        let big_widths = widths_at(&rows, 2. * CELL.0);
        let big = layout_menu(&input_over(&rows, &big_widths, CELL, false, 2.));
        let doubled = |value: f32| 2. * value;
        assert_eq!(big.panel.h, doubled(small.panel.h));
        assert_eq!(big.panel.y, doubled(small.panel.y));
        assert_eq!(big.corner_radius, doubled(small.corner_radius));
        assert_eq!(big.border_width, doubled(small.border_width));
        for (small_row, big_row) in small.rows.iter().zip(&big.rows) {
            for (small_run, big_run) in [
                (small_row.icon, big_row.icon),
                (Some(small_row.marked), Some(big_row.marked)),
                (small_row.label, big_row.label),
                (small_row.badge, big_row.badge),
                (small_row.description, big_row.description),
            ] {
                match (small_run, big_run) {
                    (Some(small_run), Some(big_run)) => {
                        assert_eq!(
                            big_run.y,
                            doubled(small_run.y),
                            "every vertical size doubles"
                        );
                        assert_eq!(big_run.line_h, doubled(small_run.line_h));
                        assert_eq!(big_run.x, doubled(small_run.x));
                        assert_eq!(big_run.width, doubled(small_run.width));
                    }
                    (None, None) => {}
                    other => panic!("a run appears or disappears with the cell: {other:?}"),
                }
            }
        }
    }

    #[test]
    fn the_layout_lays_out_no_more_rows_than_the_budget() {
        use dropterm::core::terminal::suggest::menu_row_height;
        let rows = four_rows();
        let widths = widths_at(&rows, CELL.0);
        let layout = layout_menu(&MenuLayoutInput {
            budget: 2,
            selected: Some(3),
            ..input_over(&rows, &widths, CELL, false, 1.)
        });
        let row_h = menu_row_height(CELL.1, false);
        assert_eq!(layout.rows.len(), 2, "four rows carried, two afforded");
        assert_eq!(
            layout.panel.h,
            2. * row_h,
            "the panel is the afforded rows tall"
        );
        assert!(
            layout.selection.is_none(),
            "a selection past the budget has no box to tint"
        );
        // Control: a selection inside the budget tints its row.
        let inside = layout_menu(&MenuLayoutInput {
            budget: 3,
            selected: Some(2),
            ..input_over(&rows, &widths, CELL, false, 1.)
        });
        assert_eq!(
            inside.selection,
            Some(rect(
                inside.panel.x,
                inside.panel.y + 2. * row_h,
                inside.panel.w,
                row_h
            ))
        );
    }

    #[test]
    fn a_compact_row_keeps_one_line_with_the_description_right_aligned_before_the_badge() {
        let rows = four_rows();
        let layout = layout_over(&rows, CELL, true);
        for row in &layout.rows {
            if let Some(description) = row.description {
                assert_eq!(
                    description.y, row.marked.y,
                    "the description shares the label's line"
                );
                assert!(description.align_right, "right-aligned before the badge");
                let badge = row.badge.expect("the badge shows beside it");
                assert_eq!(
                    description.x + description.width,
                    badge.x - CELL.0,
                    "its right edge a gap off the badge column"
                );
                assert!(
                    description.x >= row.marked.x + row.marked.width + CELL.0,
                    "its left edge a gap past the label"
                );
            }
        }
        // No room: a label that fills the text width leaves the
        // description out.
        let candidates = [candidate(
            "a label long enough to fill the text width on its own",
            dropterm::core::terminal::suggest::SuggestionSource::Catalog,
            Some("a description with no room left"),
        )];
        let long = menu_row_models(&candidates, "", MenuChrome::all_on());
        let widths = [MenuRowWidths {
            marked: 0.,
            label: 2000.,
            description: 10. * CELL.0,
        }];
        let layout = layout_menu(&MenuLayoutInput {
            ..input_over(&long, &widths, CELL, true, 1.)
        });
        // The text width: the panel less its padding (two cells), the
        // icon and badge columns (one and four), and the two gaps
        // around the text (two).
        assert_eq!(
            layout.rows[0].marked.width + layout.rows[0].label.map_or(0., |rest| rest.width),
            layout.panel.w - 9. * CELL.0,
            "the label's box is the whole text width"
        );
        assert!(
            layout.rows[0].description.is_none(),
            "no description box without the room"
        );
    }

    #[test]
    fn every_chrome_toggle_removes_its_runs() {
        let rows = four_rows();
        let widths = widths_at(&rows, CELL.0);
        let base = layout_over(&rows, CELL, false);
        let text_width =
            |row: &MenuRowLayout| row.marked.width + row.label.map_or(0., |label| label.width);

        let no_icons = layout_menu(&MenuLayoutInput {
            chrome: MenuChrome {
                icons: false,
                ..MenuChrome::all_on()
            },
            ..input_over(&rows, &widths, CELL, false, 1.)
        });
        assert!(no_icons.rows.iter().all(|row| row.icon.is_none()));
        assert_eq!(
            no_icons.rows[0].marked.x,
            base.rows[0].marked.x - CELL.0 - CELL.0,
            "the labels move into the icon column's room, the gap with it"
        );
        assert!(
            (text_width(&no_icons.rows[0]) - (text_width(&base.rows[0]) + CELL.0 + CELL.0)).abs()
                < 0.001,
            "the text grows by the icon column and its gap"
        );

        let no_badges = layout_menu(&MenuLayoutInput {
            chrome: MenuChrome {
                badges: false,
                ..MenuChrome::all_on()
            },
            ..input_over(&rows, &widths, CELL, false, 1.)
        });
        assert!(no_badges.rows.iter().all(|row| row.badge.is_none()));
        assert!(
            (text_width(&no_badges.rows[0]) - (text_width(&base.rows[0]) + 4. * CELL.0 + CELL.0))
                .abs()
                < 0.001,
            "the text takes the badge column's room and the gap"
        );

        let no_descriptions = layout_menu(&MenuLayoutInput {
            chrome: MenuChrome {
                descriptions: false,
                ..MenuChrome::all_on()
            },
            ..input_over(&rows, &widths, CELL, false, 1.)
        });
        assert!(
            no_descriptions
                .rows
                .iter()
                .all(|row| row.description.is_none())
        );
    }

    #[test]
    fn the_hint_line_sits_over_the_rows_and_the_selection_tints_its_row() {
        use dropterm::core::terminal::suggest::{menu_hint_height, menu_row_height};
        let rows = four_rows();
        let widths = widths_at(&rows, CELL.0);
        let layout = layout_menu(&MenuLayoutInput {
            hint: Some((9. * CELL.0, Some(40. * CELL.0))),
            selected: Some(1),
            ..input_over(&rows, &widths, CELL, false, 1.)
        });
        let (name, description) = layout.hint.expect("the hint lays out");
        let pad_y = CELL.1 * 0.125;
        assert_eq!(
            (name.x, name.y),
            (layout.panel.x + CELL.0, layout.panel.y + pad_y)
        );
        let description = description.expect("the hint's description lays out");
        assert_eq!(description.x, name.x + name.width + CELL.0);
        let (hint_h, row_h) = (menu_hint_height(CELL.1), menu_row_height(CELL.1, false));
        assert_eq!(
            layout.rows[0].marked.y,
            layout.panel.y + hint_h + pad_y,
            "the rows start under the hint"
        );
        assert_eq!(layout.panel.h, hint_h + row_h * 4.);
        assert_eq!(
            layout.selection,
            Some(rect(
                layout.panel.x,
                layout.panel.y + hint_h + row_h,
                layout.panel.w,
                row_h
            ))
        );
    }

    #[test]
    fn the_badge_words_are_the_vocabulary_every_badge_comes_from() {
        use super::{BADGE_WORDS, badge_of, icon_for};
        use dropterm::core::terminal::suggest::SuggestionSource as S;
        let mut distinct = Vec::new();
        for source in [
            S::Catalog,
            S::Specs,
            S::Path,
            S::History,
            S::Env,
            S::Generator,
        ] {
            let badge = badge_of(source, "x");
            assert!(
                BADGE_WORDS.contains(&badge),
                "{source:?}'s badge {badge:?} is in the vocabulary"
            );
            distinct.push(badge);
        }
        distinct.push(badge_of(S::Specs, "--flag"));
        distinct.sort_unstable();
        distinct.dedup();
        let mut vocabulary = BADGE_WORDS;
        vocabulary.sort_unstable();
        assert_eq!(
            distinct, vocabulary,
            "the vocabulary is exactly the words the badges yield, each once"
        );
        assert!(
            BADGE_WORDS
                .iter()
                .all(|badge| !icon_for(badge).is_control()),
            "every word has its icon glyph"
        );
    }

    fn candidate(
        label: &str,
        source: SuggestionSource,
        description: Option<&str>,
    ) -> dropterm::core::terminal::suggest::MenuCandidate {
        dropterm::core::terminal::suggest::MenuCandidate {
            label: label.to_string(),
            suffix: String::new(),
            description: description.map(str::to_string),
            source,
        }
    }

    #[test]
    fn rows_carry_the_badge_vocabulary() {
        use super::{MenuChrome, menu_row_models};
        use dropterm::core::terminal::suggest::SuggestionSource as S;

        let rows = vec![
            candidate("git", S::Catalog, None),
            candidate("status", S::Specs, None),
            candidate("--short", S::Specs, None),
            candidate("src/", S::Path, None),
            candidate("sleep 30", S::History, None),
            candidate("$HOME", S::Env, None),
            candidate("feature/x", S::Generator, None),
        ];
        let models = menu_row_models(&rows, "", MenuChrome::all_on());
        let badges: Vec<&str> = models.iter().filter_map(|row| row.badge).collect();
        assert_eq!(
            badges,
            vec!["cmd", "sub", "flag", "path", "hist", "env", "sub"]
        );
        // A flag label wins over its source: the specs' flags are
        // flags, not subs.
        assert_eq!(models[2].badge, Some("flag"));
    }

    #[test]
    fn the_typed_prefix_marks_case_insensitively() {
        use super::{MenuChrome, menu_row_models};

        let rows = vec![candidate("Git-Status", SuggestionSource::Specs, None)];
        let models = menu_row_models(&rows, "GIT", MenuChrome::all_on());
        assert_eq!(models[0].marked, 3);
        // A non-matching prefix marks nothing.
        let models = menu_row_models(&rows, "zzz", MenuChrome::all_on());
        assert_eq!(models[0].marked, 0);
    }

    #[test]
    fn each_chrome_element_disappears_with_its_toggle() {
        use super::{MenuChrome, menu_row_models};

        let rows = &[candidate(
            "git",
            SuggestionSource::Catalog,
            Some("the thing"),
        )];
        let on = &menu_row_models(rows, "gi", MenuChrome::all_on())[0];
        assert_eq!(on.badge, Some("cmd"));
        assert_eq!(on.description.as_deref(), Some("the thing"));
        assert_eq!(on.icon, Some('\u{276f}'));

        let off = &menu_row_models(
            rows,
            "gi",
            MenuChrome {
                descriptions: false,
                badges: false,
                icons: false,
                ..MenuChrome::all_on()
            },
        )[0];
        assert_eq!(off.badge, None);
        assert_eq!(off.description, None);
        assert_eq!(off.icon, None);
        // The label and its mark survive every toggle.
        assert_eq!(off.label, "git");
        assert_eq!(off.marked, 2);
    }

    #[test]
    fn pane_options_carry_the_suggest_config_and_the_loaded_table() {
        // The config→options flow: every suggest key rides the
        // options (the libcosmic `terminal_options_for` port), the
        // completions table hands down loaded-or-loading, and the
        // font size keeps its global pick.
        let mut aliases = std::collections::HashMap::new();
        aliases.insert("gc".to_string(), "git checkout".to_string());
        let config = DropTermConfig {
            input_ghost: false,
            autosuggestions_paths_enabled: false,
            autosuggestions_delay_ms: 250,
            autosuggestions_aliases: aliases.clone(),
            ..DropTermConfig::default()
        };
        let options = pane_options(&config, None);
        assert!(!options.autosuggestions_enabled);
        assert!(!options.autosuggestions_paths_enabled);
        assert_eq!(options.autosuggestions_delay_ms, 250);
        assert_eq!(options.autosuggestions_aliases, aliases);
        assert!(options.autosuggestions_specs_loading, "no table yet");
        assert!(
            options.autosuggestions_specs_table.is_empty(),
            "the loading placeholder is the empty table"
        );
        // A landed table rides as-is and clears the loading flag.
        let table: Arc<[suggest::SpecCommand]> = Arc::from(vec![]);
        let options = pane_options(&config, Some(&table));
        assert!(!options.autosuggestions_specs_loading);
        assert!(std::ptr::eq(
            options.autosuggestions_specs_table.as_ptr(),
            table.as_ptr()
        ));
    }
}
