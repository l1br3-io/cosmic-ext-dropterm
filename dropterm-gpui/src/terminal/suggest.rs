// SPDX-License-Identifier: GPL-3.0-only

//! The suggest view-model (gpui-parity Task 1): the routing and
//! geometry policy the pane's key handler and paint walk consume —
//! the libcosmic widget's own pure helpers
//! (`menu_key`/`accept_kind`/`menu_panel_bounds`), re-stated over
//! GPUI's keystroke shape so both frontends carry the same
//! suggest-menu/autosuggest rules with no shared framework types.
//! Core owns the model ([`dropterm_core::terminal::suggest`]); this
//! module owns only the view-shaped decisions, headless-tested
//! beside the input translation's pins.
//!
//! The ordering contract these helpers serve (the libcosmic
//! widget's, verbatim): the menu's routing runs before the keybind
//! swallow and every mapping — a showing menu outranks them all —
//! then the accept keys check the engine's showing, and only then
//! does the press become bytes.

use std::sync::Arc;

use dropterm_core::config::DropTermConfig;
use dropterm_core::terminal::TerminalOptions;
use dropterm_core::terminal::suggest::{self, AcceptKind, MenuKey};
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
    /// The press is the menu's: navigate, accept, or close (Escape,
    /// the trigger chord's toggle).
    Consume(MenuKey),
    /// The press closes the menu, then processes normally.
    CloseThenProcess,
}

/// The scoped routing itself — pure, pinned headless against the
/// libcosmic rules (the same table in GPUI's key names: `"up"`,
/// `"down"`, `"tab"`, `"enter"`, `"escape"`).
pub(crate) fn menu_routing(
    key: &str,
    modifiers: Modifiers,
    open: bool,
    trigger_chord: bool,
) -> Option<MenuRouting> {
    if !open {
        return None;
    }
    if trigger_chord {
        return Some(MenuRouting::Consume(MenuKey::Other));
    }
    if !(modifiers.control || modifiers.alt || modifiers.shift || modifiers.platform) {
        match key {
            "up" => return Some(MenuRouting::Consume(MenuKey::Up)),
            "down" => return Some(MenuRouting::Consume(MenuKey::Down)),
            "tab" | "enter" => return Some(MenuRouting::Consume(MenuKey::Accept)),
            "escape" => return Some(MenuRouting::Consume(MenuKey::Other)),
            _ => {}
        }
    }
    Some(MenuRouting::CloseThenProcess)
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
        autosuggestions_delay_ms: config.autosuggestions_delay_ms,
        autosuggestions_aliases: config.autosuggestions_aliases.clone(),
        autosuggestions_specs_table: specs_table.cloned().unwrap_or_else(suggest::empty_specs),
        autosuggestions_specs_loading: specs_table.is_none(),
        spawn_home: std::env::var("HOME").ok(),
        spawn_path: std::env::var("PATH").ok(),
        ..TerminalOptions::default()
    }
}

// The spawn truth lives in core now (py-5): `dropterm_core::spawn::
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
                menu_routing(key, modifiers, false, true),
                None,
                "closed menu routes nothing: {key}"
            );
        }
    }

    #[test]
    fn an_open_menu_claims_its_bare_keys() {
        // The menu's own keys: navigate, accept, close — bare only.
        assert_eq!(
            menu_routing("up", bare(), true, false),
            Some(MenuRouting::Consume(MenuKey::Up))
        );
        assert_eq!(
            menu_routing("down", bare(), true, false),
            Some(MenuRouting::Consume(MenuKey::Down))
        );
        assert_eq!(
            menu_routing("tab", bare(), true, false),
            Some(MenuRouting::Consume(MenuKey::Accept))
        );
        assert_eq!(
            menu_routing("enter", bare(), true, false),
            Some(MenuRouting::Consume(MenuKey::Accept))
        );
        assert_eq!(
            menu_routing("escape", bare(), true, false),
            Some(MenuRouting::Consume(MenuKey::Other))
        );
        // The trigger chord's toggle-close outranks them all.
        assert_eq!(
            menu_routing("space", ctrl(), true, true),
            Some(MenuRouting::Consume(MenuKey::Other))
        );
    }

    #[test]
    fn every_press_an_open_menu_does_not_claim_closes_it_first() {
        // VS Code's HasFocusedSuggestion shape: a modified claimed
        // key, a bare unclaimed key, and a bound action chord all
        // read CloseThenProcess — the press still processes normally
        // below (the ghost's accepts included).
        for (key, modifiers) in [
            ("up", ctrl()),
            ("up", shift()),
            ("a", bare()),
            ("c", ctrl()),
        ] {
            assert_eq!(
                menu_routing(key, modifiers, true, false),
                Some(MenuRouting::CloseThenProcess),
                "unclaimed press closes: {key}"
            );
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
