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
use gpui::{Bounds, Keystroke, Modifiers, Pixels, Point, px};

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
/// core's locked budget): at most [`suggest::MENU_VISIBLE_ROWS`]
/// rows starting at the scroll offset, fewer only when the list
/// itself is shorter.
pub(crate) fn menu_visible_window(scroll: usize, rows: usize) -> std::ops::Range<usize> {
    let start = scroll.min(rows);
    start..(start + suggest::MENU_VISIBLE_ROWS).min(rows)
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
fn badge_of(source: SuggestionSource, label: &str) -> &'static str {
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

/// The icon glyph per badge (rich-input RI-7): the mock's lucide
/// set as single glyphs — refined against the mock at the RI-C
/// review.
fn icon_for(badge: &str) -> char {
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

/// One menu row's height under the density switch (rich-input
/// RI-7): comfortable carries the description on its own line,
/// compact inlines it beside the label — the mock's padding pair.
/// Suggest-overlay SO-11 derives it from the cell instead.
#[must_use]
pub(crate) fn menu_row_height_px(compact: bool) -> f32 {
    if compact { 22. } else { 34. }
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
        let hint = chrome
            .argument_hint
            .then(|| {
                typed_text
                    .split_whitespace()
                    .next()
                    .and_then(|command| engine.menu_hint(command))
            })
            .flatten();
        Some(Self {
            rows,
            selected: engine.menu_selected(),
            scroll: engine.menu_scroll(),
            hint,
            chrome,
        })
    }
}

/// The menu panel's box over the prompt row (suggest-overlay SO-6):
/// the pane's left edge plus the margin, above the prompt row when
/// the panel fits in the room over it, otherwise below the row, and
/// never outside the pane. The panel's own geometry (the width, the
/// height it is handed) is the paint's today; SO-11 rebuilds it from
/// the cell on this seat.
#[must_use]
pub(crate) fn menu_panel_bounds(
    pane: Bounds<Pixels>,
    prompt_row_top: Pixels,
    cell_height: Pixels,
    panel_height: Pixels,
) -> Bounds<Pixels> {
    let margin = px(8.);
    let width = px(460.).min(pane.size.width - margin * 2.);
    let above = prompt_row_top - panel_height;
    let wanted = if above >= pane.origin.y {
        above
    } else {
        prompt_row_top + cell_height
    };
    let lowest = (pane.origin.y + pane.size.height - panel_height).max(pane.origin.y);
    Bounds {
        origin: Point {
            x: pane.origin.x + margin,
            y: wanted.clamp(pane.origin.y, lowest),
        },
        size: gpui::Size {
            width,
            height: panel_height,
        },
    }
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
        // The locked budget: eight rows from the scroll offset,
        // fewer only when the list is shorter. A scroll at the list's
        // own length (the engine's bounds never produce it) is the
        // ported shape's empty window, not a clamp of its own.
        assert_eq!(menu_visible_window(0, 12), 0..8);
        assert_eq!(menu_visible_window(4, 12), 4..12);
        assert_eq!(menu_visible_window(3, 5), 3..5);
        assert_eq!(menu_visible_window(9, 9), 9..9);
    }

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds {
            origin: Point { x: px(x), y: px(y) },
            size: gpui::size(px(width), px(height)),
        }
    }

    #[test]
    fn the_menu_panel_sits_above_the_prompt_row_when_it_fits() {
        use super::menu_panel_bounds;

        let panel = menu_panel_bounds(bounds(0., 0., 800., 600.), px(400.), px(20.), px(100.));
        assert_eq!(
            panel.origin,
            Point {
                x: px(8.),
                y: px(300.)
            },
            "the pane's left edge plus the margin, the panel's foot on the prompt row's top"
        );
        assert_eq!(panel.size, gpui::size(px(460.), px(100.)));
    }

    #[test]
    fn the_menu_panel_drops_below_a_top_prompt_row() {
        use super::menu_panel_bounds;

        let pane = bounds(0., 0., 800., 600.);
        assert_eq!(
            menu_panel_bounds(pane, px(0.), px(20.), px(100.)).origin.y,
            px(20.),
            "no room over the first row: the panel sits under the prompt row"
        );
        // Control: with the room, the same panel goes above.
        assert_eq!(
            menu_panel_bounds(pane, px(120.), px(20.), px(100.))
                .origin
                .y,
            px(20.)
        );
        assert_eq!(
            menu_panel_bounds(pane, px(140.), px(20.), px(100.))
                .origin
                .y,
            px(40.),
            "exactly the room: above wins"
        );
        assert_eq!(
            menu_panel_bounds(pane, px(100.), px(20.), px(100.))
                .origin
                .y,
            px(0.),
            "a prompt row a whole panel down the pane still fits it above"
        );
    }

    #[test]
    fn the_menu_panel_never_leaves_the_pane() {
        use super::menu_panel_bounds;

        // A panel taller than the room on both sides: inside the
        // pane, its top on the pane's.
        assert_eq!(
            menu_panel_bounds(bounds(0., 0., 800., 120.), px(0.), px(20.), px(200.))
                .origin
                .y,
            px(0.)
        );
        // A pane narrower than the panel: the width clamps to the
        // pane less its margins, and the margin holds on the left.
        let narrow = menu_panel_bounds(bounds(10., 0., 300., 600.), px(400.), px(20.), px(100.));
        assert_eq!(narrow.size.width, px(284.));
        assert_eq!(
            narrow.origin.x,
            px(18.),
            "the pane's own left edge, plus the margin"
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
    fn the_density_switch_flips_the_row_height() {
        use super::menu_row_height_px;
        let comfortable = menu_row_height_px(false);
        let compact = menu_row_height_px(true);
        assert!(
            compact < comfortable,
            "compact rows are the tighter density"
        );
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
