// SPDX-License-Identifier: GPL-3.0-only

//! The suggest view-model (gpui-parity Task 1): the routing and
//! geometry policy the pane's key handler and paint walk consume —
//! the libcosmic widget's own pure helpers
//! (`menu_key`/`accept_kind`/`menu_panel_bounds`), re-stated over
//! GPUI's keystroke shape so both frontends carry the same
//! suggest-menu/autosuggest rules with no shared framework types.
//! Core owns the model ([`dropterm::core::terminal::suggest`]); this
//! module owns only the view-shaped decisions, headless-tested
//! beside the input translation's pins.
//!
//! The ordering contract these helpers serve (the libcosmic
//! widget's, verbatim): the menu's routing runs before the keybind
//! swallow and every mapping — a showing menu outranks them all —
//! then the accept keys check the engine's showing, and only then
//! does the press become bytes.

use std::sync::Arc;

use dropterm::core::config::DropTermConfig;
use dropterm::core::terminal::TerminalOptions;
use dropterm::core::terminal::suggest::{self, AcceptKind, MenuKey};
use gpui::{Bounds, Modifiers, Pixels, Point};

/// The completion menu's scoped-key verdict (the libcosmic
/// `MenuRouting`, same rules): the claimed keys — bare
/// up/down/tab/enter/escape, plus the `show_completions` chord (its
/// toggle-close) — are the menu's alone while it shows; every other
/// press closes it first and then processes normally (the ghost's
/// accepts included). `None` is no menu routing at all — no menu
/// shows — and the press is the shell's verbatim, the trigger
/// chord's open attempt included (the caller tries the open on a
/// `None` + trigger-chord press and forwards the chord's own bytes
/// on the decline).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuRouting {
    /// The press is the menu's: navigate, fill, accept, or close.
    Consume(MenuKey),
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
/// `"down"`, `"tab"`, `"enter"`, `"escape"`). `key_char` is the
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
    trigger_chord: bool,
    key_char: Option<char>,
    equal_line: bool,
) -> Option<MenuRouting> {
    if !open {
        return None;
    }
    if trigger_chord {
        return Some(MenuRouting::Consume(MenuKey::Other));
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
            "tab" => return Some(MenuRouting::Consume(MenuKey::Tab)),
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
    // Warp's select-prev (record finding 4): Shift+Tab cycles up
    // while the menu shows.
    if no_command_mods && modifiers.shift && key == "tab" {
        return Some(MenuRouting::Consume(MenuKey::Up));
    }
    // The as-you-type edit (record finding 4): a printable keeps the
    // menu alive — the key's bytes flow to the PTY and the rows
    // recompute on the echo's update (SP3's gate).
    if no_command_mods && key_char.is_some() {
        return Some(MenuRouting::KeepOpen);
    }
    Some(MenuRouting::CloseThenProcess)
}

/// The Tab-while-closed resolution's action (suggest-parity spec,
/// Warp's `explicit_tab_completion` over OQ-1's conditioned open):
/// what the collected rows resolve a bare Tab into. `None` — the
/// caller's gate failure or nothing qualifying — is not an action:
/// the Tab bytes flow to the shell verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TabAction {
    /// The single prefix row's remainder inserts directly and the
    /// menu stays closed.
    Insert {
        /// The row's suffix — the exact bytes the typed line gains.
        suffix: String,
    },
    /// A case-sensitive common prefix longer than the typed token
    /// inserts, and the menu opens over the fresh collection.
    InsertOpen {
        /// The LCP's remainder — the exact bytes the typed line
        /// gains.
        suffix: String,
    },
    /// Rows exist with nothing to insert: the menu opens.
    Open,
}

/// The Tab-while-closed decision itself — pure, pinned headless
/// beside [`menu_routing`]: the gates (menu enabled, idle, focused,
/// line-end, a non-empty typed segment) are the caller's; this reads
/// only the collected rows and the typed segment, the token carved
/// here so the two stay consistent. `None` is nothing qualifying —
/// no rows, or no row extending the line — and the Tab bytes flow.
/// Warp's `explicit_tab_completion` shape (suggest-parity spec,
/// record finding 5): exactly one extending row inserts its
/// remainder directly (the menu stays closed); a shared continuation
/// longer than the line inserts and opens; rows with nothing to
/// insert open. The continuation anchor is each row's own (the
/// owner's 2026-11-24 report: a token-anchored slice over whole-line
/// history rows re-inserted bytes already typed, growing the line
/// every press).
pub(crate) fn tab_while_closed(rows: &[suggest::MenuCandidate], typed: &str) -> Option<TabAction> {
    let token = suggest::trailing_token(typed);
    // A row with no cs-extension never qualifies (the honest-write
    // rule, suggest-depth spec): the ladder's exact-ci and fuzzy
    // rows carry no continuation at all. A line-equal row's empty
    // continuation DOES qualify — it blocks the single-insert and
    // poisons the common prefix, so Tab over an already-filled line
    // opens or forwards rather than grows (the owner's multi-word
    // report's own rule).
    let matching: Vec<&suggest::MenuCandidate> = rows
        .iter()
        .filter(|row| suggest::fill_continuation(row, typed, token).is_some())
        .collect();
    if let [only] = &matching[..] {
        if let Some(suffix) = suggest::fill_continuation(only, typed, token)
            .filter(|continuation| !continuation.is_empty())
        {
            return Some(TabAction::Insert {
                suffix: suffix.to_string(),
            });
        }
        // Rows exist with nothing to insert: the menu opens, never a
        // zero-byte Insert that swallows the Tab.
        return Some(TabAction::Open);
    }
    let continuations: Vec<&str> = matching
        .iter()
        .filter_map(|row| suggest::fill_continuation(row, typed, token))
        .collect();
    let lcp = suggest::longest_common_prefix(&continuations);
    if !lcp.is_empty() {
        return Some(TabAction::InsertOpen {
            suffix: lcp.to_string(),
        });
    }
    if rows.is_empty() || matching.is_empty() {
        return None;
    }
    Some(TabAction::Open)
}

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

/// The menu overlay's visible-row window (the libcosmic shape over
/// core's locked budget): at most [`suggest::MENU_VISIBLE_ROWS`]
/// rows starting at the scroll offset, fewer only when the list
/// itself is shorter.
pub(crate) fn menu_visible_window(scroll: usize, rows: usize) -> std::ops::Range<usize> {
    let start = scroll.min(rows);
    start..(start + suggest::MENU_VISIBLE_ROWS).min(rows)
}

/// The menu overlay panel's pixel geometry (the libcosmic
/// `menu_panel_bounds`, same rules): anchored at the cursor cell —
/// `anchor` the cursor column and row's top-left in view pixels —
/// dropping below the cursor's row, flipping above it when the rows
/// would overflow the view's floor (a drop-down's prompt almost
/// always sits at the grid's bottom), clamped at the view's top
/// when even the flip cannot fit, and clamped horizontally so the
/// panel never leaves the view. Pure pixel math, headless-testable.
pub(crate) fn menu_panel_bounds(
    anchor: Point<Pixels>,
    row_height: Pixels,
    rows: usize,
    width: Pixels,
    view: Bounds<Pixels>,
) -> Bounds<Pixels> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "the row count is the visible budget (≤8), exact in f32"
    )]
    let height = row_height * rows as f32;
    let below_top = anchor.y + row_height;
    let y = if below_top + height <= view.origin.y + view.size.height {
        below_top
    } else {
        (anchor.y - height).max(view.origin.y)
    };
    let width = width.min(view.size.width);
    let x = anchor.x.clamp(
        view.origin.x,
        (view.origin.x + view.size.width - width).max(view.origin.x),
    );
    Bounds {
        origin: Point::new(x, y),
        size: gpui::size(width, height),
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
        autosuggestions_enabled: config.autosuggestions_enabled,
        images_enabled: config.images_enabled,
        autosuggestions_paths_enabled: config.autosuggestions_paths_enabled,
        autosuggestions_commands_enabled: config.autosuggestions_commands_enabled,
        autosuggestions_menu_enabled: config.autosuggestions_menu_enabled,
        autosuggestions_menu_auto: config.autosuggestions_menu_auto,
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
    use gpui::px;

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
    fn a_closed_menu_routes_nothing_not_even_the_trigger_chord() {
        // No menu shows: every press — the claimed keys, the trigger
        // chord, anything — reads None. The caller then tries the
        // open on a trigger-chord press; the decline forwards the
        // chord's own shell bytes.
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
                menu_routing(key, modifiers, false, true, None, false),
                None,
                "closed menu routes nothing: {key}"
            );
        }
    }

    #[test]
    fn an_open_menu_claims_its_bare_keys() {
        // The menu's own keys: navigate, accept, close — bare only.
        assert_eq!(
            menu_routing("up", bare(), true, false, None, false),
            Some(MenuRouting::Consume(MenuKey::Up))
        );
        assert_eq!(
            menu_routing("down", bare(), true, false, None, false),
            Some(MenuRouting::Consume(MenuKey::Down))
        );
        assert_eq!(
            menu_routing("tab", bare(), true, false, None, false),
            Some(MenuRouting::Consume(MenuKey::Tab))
        );
        assert_eq!(
            menu_routing("enter", bare(), true, false, None, false),
            Some(MenuRouting::Consume(MenuKey::Accept))
        );
        assert_eq!(
            menu_routing("escape", bare(), true, false, None, false),
            Some(MenuRouting::Consume(MenuKey::Other))
        );
        // The trigger chord's toggle-close outranks them all.
        assert_eq!(
            menu_routing("space", ctrl(), true, true, None, false),
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
                menu_routing(key, modifiers, true, false, key_char, false),
                Some(MenuRouting::CloseThenProcess),
                "unclaimed press closes: {key}"
            );
        }
    }

    /// Menu rows from (label, suffix) pairs — the routing-side
    /// fixtures for the Tab-while-closed decision.
    fn menu_rows(entries: &[(&str, &str)]) -> Vec<suggest::MenuCandidate> {
        entries
            .iter()
            .map(|(label, suffix)| suggest::MenuCandidate {
                label: (*label).to_string(),
                suffix: (*suffix).to_string(),
                description: None,
                source: suggest::SuggestionSource::Specs,
            })
            .collect()
    }

    fn history_menu_rows(entries: &[(&str, &str)]) -> Vec<suggest::MenuCandidate> {
        entries
            .iter()
            .map(|(label, suffix)| suggest::MenuCandidate {
                label: (*label).to_string(),
                suffix: (*suffix).to_string(),
                description: None,
                source: suggest::SuggestionSource::History,
            })
            .collect()
    }

    fn path_menu_rows(entries: &[(&str, &str)]) -> Vec<suggest::MenuCandidate> {
        entries
            .iter()
            .map(|(label, suffix)| suggest::MenuCandidate {
                label: (*label).to_string(),
                suffix: (*suffix).to_string(),
                description: None,
                source: suggest::SuggestionSource::Path,
            })
            .collect()
    }

    #[test]
    fn a_printable_keeps_the_menu_open() {
        // The as-you-type rule (suggest-parity spec, record finding
        // 4): an unmodified printable is an edit, not a close — the
        // menu stays and the key's bytes flow to the PTY; the rows
        // recompute on the echo's update.
        assert_eq!(
            menu_routing("a", bare(), true, false, Some('a'), false),
            Some(MenuRouting::KeepOpen)
        );
        assert_eq!(
            menu_routing("space", bare(), true, false, Some(' '), false),
            Some(MenuRouting::KeepOpen)
        );
    }

    #[test]
    fn a_capital_is_still_a_printable_edit() {
        // Shift is not a command modifier on a printable: typing a
        // capital keeps the menu (an edit is an edit); only
        // ctrl/alt/super exempt a key.
        assert_eq!(
            menu_routing("a", shift(), true, false, Some('A'), false),
            Some(MenuRouting::KeepOpen)
        );
    }

    #[test]
    fn shift_tab_selects_previous_while_open() {
        // Warp's completions menu claims Shift+Tab as the
        // select-prev (suggest-parity spec, record finding 4) — no
        // longer the close-then-process fall-through.
        assert_eq!(
            menu_routing("tab", shift(), true, false, None, false),
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
            menu_routing("enter", bare(), true, false, Some('\r'), true),
            Some(MenuRouting::CloseThenProcess)
        );
    }

    #[test]
    fn enter_on_a_differing_line_accepts() {
        // The accept stage: a differing line's Enter is the menu's —
        // the suffix lands and the menu closes.
        assert_eq!(
            menu_routing("enter", bare(), true, false, Some('\r'), false),
            Some(MenuRouting::Consume(MenuKey::Accept))
        );
    }

    #[test]
    fn a_single_prefix_row_inserts_directly_and_stays_closed() {
        // Warp's explicit_tab_completion over OQ-1 (suggest-parity
        // spec): exactly one prefix row — its remainder inserts
        // through the funnel and no menu opens.
        let rows = menu_rows(&[("gitk", "tk")]);
        assert_eq!(
            tab_while_closed(&rows, "gi"),
            Some(TabAction::Insert {
                suffix: "tk".to_string()
            })
        );
    }

    #[test]
    fn a_shared_prefix_longer_than_typed_inserts_and_opens() {
        // Rows `git`/`gitk` over `gi`: the case-sensitive LCP `git`
        // extends the token — `t` inserts and the menu opens.
        let rows = menu_rows(&[("git", "t"), ("gitk", "tk")]);
        assert_eq!(
            tab_while_closed(&rows, "gi"),
            Some(TabAction::InsertOpen {
                suffix: "t".to_string()
            })
        );
    }

    #[test]
    fn rows_without_an_extending_prefix_open() {
        // Rows `git`/`gut` over `g`: the LCP is the token itself —
        // nothing to insert — so the menu opens.
        let rows = menu_rows(&[("git", "it"), ("gut", "ut")]);
        assert_eq!(tab_while_closed(&rows, "g"), Some(TabAction::Open));
    }

    #[test]
    fn nothing_qualifying_forwards_the_tab() {
        // No rows, or no row extending the token: nothing qualifies,
        // the Tab bytes flow to the shell verbatim.
        assert_eq!(tab_while_closed(&[], "gi"), None);
        let rows = menu_rows(&[("git", "t"), ("gitk", "tk")]);
        assert_eq!(tab_while_closed(&rows, "zz"), None, "no prefix row");
    }

    #[test]
    fn rows_with_only_empty_continuations_open() {
        // The ladder's exact and fuzzy rows (suggest-depth spec): a
        // row that does not cs-extend the line carries no
        // continuation, so a menu of only such rows inserts nothing —
        // rows exist with nothing to insert, and the menu opens
        // (never a zero-byte Insert that swallows the Tab).
        let rows = vec![suggest::MenuCandidate {
            label: "cargo t".to_string(),
            suffix: String::new(),
            description: None,
            source: suggest::SuggestionSource::History,
        }];
        assert_eq!(tab_while_closed(&rows, "cargo t"), Some(TabAction::Open));
    }

    #[test]
    fn an_already_filled_line_opens_without_inserting_again() {
        // The owner's 2026-11-24 report: history rows `cd cd` /
        // `cd cd cd` over the typed line `cd cd` — the rows' shared
        // prefix is already on the line, so nothing qualifies as an
        // insert and the menu opens. The token-anchored slice used to
        // return InsertOpen ` cd` again, growing the line every press.
        let rows = history_menu_rows(&[("cd cd", " cd"), ("cd cd cd", " cd cd")]);
        assert_eq!(tab_while_closed(&rows, "cd cd"), Some(TabAction::Open));
    }

    #[test]
    fn a_path_rows_shared_component_inserts_and_opens() {
        // The owner's 2026-11-24 second report: path rows carry the
        // bare listing entry as their label (`Documents/`), so the
        // fill's anchor is the token's final component, the tier's own
        // carve — rows `Documents/`/`Documentary` over the typed
        // `~/Doc` share `ument`, it inserts and the menu opens. The
        // token-anchored test matched nothing here and the Tab bytes
        // flowed to the shell instead.
        let rows = path_menu_rows(&[("Documents/", "uments/"), ("Documentary", "umentary")]);
        assert_eq!(
            tab_while_closed(&rows, "~/Doc"),
            Some(TabAction::InsertOpen {
                suffix: "ument".to_string()
            })
        );
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

    #[test]
    fn the_menu_panel_drops_below_the_cursor() {
        // The common shape with room under it: the panel drops one
        // row below the anchor.
        let view = Bounds {
            origin: Point::new(px(0.), px(0.)),
            size: gpui::size(px(400.), px(300.)),
        };
        let panel = menu_panel_bounds(Point::new(px(100.), px(150.)), px(20.), 5, px(120.), view);
        assert_eq!(panel.origin, Point::new(px(100.), px(170.)));
        assert_eq!(panel.size, gpui::size(px(120.), px(100.)));
    }

    #[test]
    fn the_menu_panel_flips_above_at_the_floor_and_clamps() {
        // The prompt at the view's floor: the drop cannot fit, the
        // panel flips above the anchor row.
        let view = Bounds {
            origin: Point::new(px(0.), px(0.)),
            size: gpui::size(px(400.), px(300.)),
        };
        let panel = menu_panel_bounds(Point::new(px(100.), px(290.)), px(20.), 5, px(120.), view);
        assert_eq!(panel.origin, Point::new(px(100.), px(190.)));
        // A flip that would leave the view clamps at the view's top:
        // a 100px panel anchored 40px into a 120px-tall view.
        let short_view = Bounds {
            origin: Point::new(px(0.), px(0.)),
            size: gpui::size(px(400.), px(120.)),
        };
        let tall = menu_panel_bounds(
            Point::new(px(100.), px(40.)),
            px(20.),
            5,
            px(120.),
            short_view,
        );
        assert_eq!(tall.origin.y, px(0.));
        // Horizontally the panel never leaves the view: a wide panel
        // at the right edge clamps its overflow.
        let wide = menu_panel_bounds(Point::new(px(390.), px(10.)), px(20.), 3, px(200.), view);
        assert_eq!(wide.origin.x, px(200.));
        assert_eq!(wide.size.width, px(200.));
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
            autosuggestions_enabled: false,
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
