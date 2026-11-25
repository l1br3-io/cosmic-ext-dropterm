// SPDX-License-Identifier: GPL-3.0-only

//! The suggest view-model (gpui-parity Task 1): the view-shaped
//! policy the pane consumes — the libcosmic widget's own pure
//! helpers (`accept_kind` and the visible-row window) re-stated over
//! GPUI's keystroke shape. The grid overlay's routing table and
//! panel geometry retired with its paint (rich-input RI-10): the
//! editor lane owns the menu's keys and its panel now. Core owns the
//! model ([`dropterm::core::terminal::suggest`]); this module owns
//! only the view-shaped decisions, headless-tested beside the input
//! translation's pins.
//!
//! The ordering contract these helpers serve (the libcosmic
//! widget's, verbatim): the menu's routing runs before the keybind
//! swallow and every mapping — a showing menu outranks them all —
//! then the accept keys check the engine's showing, and only then
//! does the press become bytes.

use std::sync::Arc;

use dropterm::core::config::DropTermConfig;
use dropterm::core::terminal::TerminalOptions;
use dropterm::core::terminal::suggest::{self, AcceptKind};
use gpui::Modifiers;

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
        input_position: config.input_position,
        input_highlight: config.input_highlight,
        input_descriptions: config.input_descriptions,
        input_badges: config.input_badges,
        input_icons: config.input_icons,
        input_argument_hint: config.input_argument_hint,
        input_compact_rows: config.input_compact_rows,
        // The folded surfaces read the decoupled rich-input keys
        // (rich-input RI-1's cull): the internal option names keep
        // their shape — the editor's own wiring (RI-3/RI-4) renames
        // them — but their truth is the new keys', roles unchanged.
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
