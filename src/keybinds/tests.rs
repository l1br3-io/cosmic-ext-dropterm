use cosmic::iced::keyboard::key::Code;

use super::*;

#[test]
fn the_shift_held_plus_cluster_zooms_in() {
    // Matching is exact on modifiers, and on US-style layouts typing
    // `+` (or holding Shift out of habit) puts Shift in the press:
    // without Shift-form chords the literal Ctrl+Plus press arrives
    // dead — no zoom, nothing to the shell (zoom-shift-binds). The
    // cluster owns four default chords: two unshifted, two held.
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("+".into()),
            Some(&Physical::Code(Code::Equal)),
        ),
        Some(Action::ZoomIn)
    );
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("=".into()),
            Some(&Physical::Code(Code::Equal)),
        ),
        Some(Action::ZoomIn)
    );
    assert_eq!(
        binds
            .values()
            .filter(|action| **action == Action::ZoomIn)
            .count(),
        4
    );
}

#[test]
fn the_shift_held_minus_zooms_out() {
    // The minus cluster mirrors the plus one: numpad minus with
    // Shift held, and the layouts where `-` survives Shift, arrive
    // as their own chord — a dead press before this bind existed.
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("-".into()),
            Some(&Physical::Code(Code::Minus)),
        ),
        Some(Action::ZoomOut)
    );
}

#[test]
fn ctrl_shift_underscore_stays_free_for_the_terminal() {
    // Ctrl+Shift+Minus on a US layout arrives as "_" and the widget
    // sends the shell 0x1F (readline undo, xterm parity) — a byte
    // the shell owns, so the chord is never bound: the same rule
    // that keeps bare Ctrl+I/H/D/R/B free.
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("_".into()),
            Some(&Physical::Code(Code::Minus)),
        ),
        None
    );
}

#[test]
fn copy_and_paste_are_bound() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("c".into()),
            None,
        ),
        Some(Action::Copy)
    );
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("v".into()),
            None,
        ),
        Some(Action::Paste)
    );
}

#[test]
fn unbound_key_does_not_match() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(&binds, Modifiers::CTRL, &Key::Character("c".into()), None),
        None
    );
}

#[test]
fn ctrl_shift_q_hides() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("q".into()),
            None,
        ),
        Some(Action::Hide)
    );
}

#[test]
fn bare_escape_is_not_bound() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(&binds, Modifiers::empty(), &Key::Named(Named::Escape), None,),
        None
    );
}

#[test]
fn ctrl_shift_i_opens_settings() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("i".into()),
            None,
        ),
        Some(Action::Settings)
    );
}

#[test]
fn bare_and_ctrl_i_stay_free_for_the_terminal() {
    // Ctrl+I is the TAB byte (0x09) in a terminal — swallowing it
    // would break tab completion — and a bare `i` is ordinary
    // typing: only the full Ctrl+Shift+I combo is the settings
    // bind.
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::empty(),
            &Key::Character("i".into()),
            None,
        ),
        None
    );
    assert_eq!(
        matched_action(&binds, Modifiers::CTRL, &Key::Character("i".into()), None),
        None
    );
}

#[test]
fn ctrl_shift_b_toggles_the_status_bar_alone() {
    // OQ-3's ruling: the one action on the chord is the band's
    // toggle, and the toggle's only default chord is this one —
    // no collision in either direction (status-bar Task 3 pin).
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("b".into()),
            None,
        ),
        Some(Action::ToggleStatusBar)
    );
    assert_eq!(
        binds
            .values()
            .filter(|action| **action == Action::ToggleStatusBar)
            .count(),
        1
    );
}

#[test]
fn bare_and_ctrl_b_stay_free_for_the_terminal() {
    // Ctrl+B is readline's cursor-back byte — swallowing it would
    // break shell line editing — and a bare `b` is ordinary
    // typing: only the full Ctrl+Shift+B combo is the toggle.
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::empty(),
            &Key::Character("b".into()),
            None,
        ),
        None
    );
    assert_eq!(
        matched_action(&binds, Modifiers::CTRL, &Key::Character("b".into()), None),
        None
    );
}

#[test]
fn ctrl_shift_t_opens_a_new_tab() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("t".into()),
            None,
        ),
        Some(Action::NewTab)
    );
}

#[test]
fn ctrl_shift_h_opens_a_new_tab_at_home() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("h".into()),
            None,
        ),
        Some(Action::NewTabHome)
    );
}

#[test]
fn bare_h_and_ctrl_h_stay_free_for_the_shell() {
    // Ctrl+H is the backspace byte (0x08) in a terminal and a bare
    // `h` is ordinary typing: only the full Ctrl+Shift+H combo is
    // the home new-tab bind.
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::empty(),
            &Key::Character("h".into()),
            None,
        ),
        None
    );
    assert_eq!(
        matched_action(&binds, Modifiers::CTRL, &Key::Character("h".into()), None),
        None
    );
    assert_eq!(
        matched_action(&binds, Modifiers::SHIFT, &Key::Character("h".into()), None),
        None
    );
}

#[test]
fn ctrl_shift_w_closes_the_active_tab() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("w".into()),
            None,
        ),
        Some(Action::CloseTab)
    );
}

#[test]
fn ctrl_shift_r_starts_a_rename() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("r".into()),
            None,
        ),
        Some(Action::RenameTab)
    );
}

#[test]
fn bare_r_and_ctrl_r_stay_free_for_the_shell() {
    // Ctrl+R is the shell's reverse history search and a bare `r`
    // is ordinary typing: only the full Ctrl+Shift+R combo is the
    // rename bind.
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::empty(),
            &Key::Character("r".into()),
            None
        ),
        None
    );
    assert_eq!(
        matched_action(&binds, Modifiers::CTRL, &Key::Character("r".into()), None),
        None
    );
    assert_eq!(
        matched_action(&binds, Modifiers::SHIFT, &Key::Character("r".into()), None),
        None
    );
}

#[test]
fn ctrl_shift_d_duplicates_the_active_tab() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("d".into()),
            None,
        ),
        Some(Action::DuplicateTab)
    );
}

#[test]
fn bare_d_and_ctrl_d_stay_free_for_the_shell() {
    // Ctrl+D is the shell's end-of-file (EOF) and a bare `d` is
    // ordinary typing: only the full Ctrl+Shift+D combo is the
    // duplicate bind.
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::empty(),
            &Key::Character("d".into()),
            None
        ),
        None
    );
    assert_eq!(
        matched_action(&binds, Modifiers::CTRL, &Key::Character("d".into()), None),
        None
    );
    assert_eq!(
        matched_action(&binds, Modifiers::SHIFT, &Key::Character("d".into()), None),
        None
    );
}

#[test]
fn ctrl_page_up_cycles_to_the_previous_tab() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(&binds, Modifiers::CTRL, &Key::Named(Named::PageUp), None,),
        Some(Action::PrevTab)
    );
}

#[test]
fn ctrl_page_down_cycles_to_the_next_tab() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(&binds, Modifiers::CTRL, &Key::Named(Named::PageDown), None,),
        Some(Action::NextTab)
    );
}

#[test]
fn ctrl_shift_digits_activate_numbered_tabs() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("1".into()),
            None,
        ),
        Some(Action::ActivateTab(0))
    );
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("8".into()),
            None,
        ),
        Some(Action::ActivateTab(7))
    );
}

#[test]
fn ctrl_shift_nine_activates_the_last_tab() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("9".into()),
            None,
        ),
        Some(Action::ActivateLastTab)
    );
}

#[test]
fn bare_ctrl_digits_are_not_tab_binds() {
    // Without Shift the digits stay free for the shell (only
    // Ctrl+=/-/0 zoom); they must not collide with the numbered
    // tab binds.
    let binds = default_key_binds();
    assert_eq!(
        matched_action(&binds, Modifiers::CTRL, &Key::Character("2".into()), None),
        None
    );
}

#[test]
fn ctrl_shift_page_keys_move_the_active_tab() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Named(Named::PageUp),
            None,
        ),
        Some(Action::MoveTabLeft)
    );
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Named(Named::PageDown),
            None,
        ),
        Some(Action::MoveTabRight)
    );
}

#[test]
fn bare_page_keys_stay_free_for_the_terminal() {
    // Pagers and shell readers use PageUp/PageDown themselves, so
    // only the Ctrl-combined forms may be swallowed.
    let binds = default_key_binds();
    assert_eq!(
        matched_action(&binds, Modifiers::empty(), &Key::Named(Named::PageUp), None),
        None
    );
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::empty(),
            &Key::Named(Named::PageDown),
            None
        ),
        None
    );
}

#[test]
fn ctrl_tab_cycles_to_the_next_tab_browser_style() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(&binds, Modifiers::CTRL, &Key::Named(Named::Tab), None),
        Some(Action::NextTab)
    );
}

#[test]
fn ctrl_shift_tab_cycles_to_the_previous_tab_browser_style() {
    let binds = default_key_binds();
    assert_eq!(
        matched_action(
            &binds,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Named(Named::Tab),
            None
        ),
        Some(Action::PrevTab)
    );
}

#[test]
fn bare_tab_stays_free_for_the_terminal() {
    // Tab completion must keep reaching the shell.
    let binds = default_key_binds();
    assert_eq!(
        matched_action(&binds, Modifiers::empty(), &Key::Named(Named::Tab), None),
        None
    );
    assert_eq!(
        matched_action(&binds, Modifiers::SHIFT, &Key::Named(Named::Tab), None),
        None
    );
}

// --- the customization overlay (keybinds-editor KE-1) ---

use std::collections::BTreeMap;

fn custom(pairs: &[(&str, &[ModifierName], &str)]) -> BTreeMap<String, Binding> {
    pairs
        .iter()
        .map(|(id, modifiers, key)| {
            (
                (*id).to_string(),
                Binding {
                    modifiers: modifiers.to_vec(),
                    key: key.to_string(),
                },
            )
        })
        .collect()
}

#[test]
fn empty_customs_degenerate_to_the_default_map() {
    assert_eq!(effective_key_binds(&BTreeMap::new()), default_key_binds());
}

#[test]
fn a_custom_binding_replaces_all_of_an_actions_defaults() {
    // ZoomIn owns four default chords (Ctrl+=, Ctrl+Plus, and their
    // Shift-held forms); one custom binding retires all four — it
    // never sits beside them. An untouched action keeps its defaults
    // verbatim.
    let customs = custom(&[("zoom_in", &[ModifierName::Ctrl, ModifierName::Alt], "q")]);
    let effective = effective_key_binds(&customs);
    assert_eq!(
        effective.get(&bind(&[Modifier::Ctrl, Modifier::Alt], "q")),
        Some(&Action::ZoomIn)
    );
    assert_eq!(effective.get(&bind(&[Modifier::Ctrl], "=")), None);
    assert_eq!(effective.get(&bind(&[Modifier::Ctrl], "+")), None);
    assert_eq!(
        effective.get(&bind(&[Modifier::Ctrl, Modifier::Shift], "+")),
        None
    );
    assert_eq!(
        effective.get(&bind(&[Modifier::Ctrl, Modifier::Shift], "=")),
        None
    );
    assert_eq!(
        effective.get(&bind(&[Modifier::Ctrl, Modifier::Shift], "c")),
        Some(&Action::Copy)
    );
}

#[test]
fn removing_an_entry_reverts_its_action_and_keeps_the_others() {
    // Revert is entry removal (KE-5 wires the row's control): the
    // reverted action's defaults come back while other customs keep
    // applying.
    let mut customs = custom(&[
        ("copy", &[ModifierName::Ctrl, ModifierName::Alt], "c"),
        ("zoom_out", &[ModifierName::Ctrl, ModifierName::Alt], "q"),
    ]);
    let effective = effective_key_binds(&customs);
    assert_eq!(
        effective.get(&bind(&[Modifier::Ctrl, Modifier::Shift], "c")),
        None,
        "the custom replaced the default while applied"
    );
    customs.remove("copy");
    let reverted = effective_key_binds(&customs);
    assert_eq!(
        reverted.get(&bind(&[Modifier::Ctrl, Modifier::Shift], "c")),
        Some(&Action::Copy),
        "revert restores the default chord"
    );
    assert_eq!(
        reverted.get(&bind(&[Modifier::Ctrl, Modifier::Alt], "q")),
        Some(&Action::ZoomOut),
        "the untouched custom still applies"
    );
}

#[test]
fn a_custom_chord_wins_the_chord_it_lands_on() {
    // Only hand-edited configs can do this (capture-time verdicts
    // keep the editor from creating it on purpose): the custom owns
    // the chord and the default holder loses it.
    let customs = custom(&[("copy", &[ModifierName::Ctrl, ModifierName::Shift], "v")]);
    let effective = effective_key_binds(&customs);
    assert_eq!(
        effective.get(&bind(&[Modifier::Ctrl, Modifier::Shift], "v")),
        Some(&Action::Copy)
    );
    assert!(
        !effective.values().any(|action| *action == Action::Paste),
        "Paste lost its only default chord to the custom"
    );
}

#[test]
fn colliding_customs_resolve_deterministically_by_id_order() {
    // Two customs claiming one chord: the BTreeMap's id order fixes
    // the winner — the later id inserts last and keeps the chord.
    // ("copy" sorts after "clear_scrollback".)
    let customs = custom(&[
        (
            "clear_scrollback",
            &[ModifierName::Ctrl, ModifierName::Alt],
            "c",
        ),
        ("copy", &[ModifierName::Ctrl, ModifierName::Alt], "c"),
    ]);
    let effective = effective_key_binds(&customs);
    assert_eq!(
        effective.get(&bind(&[Modifier::Ctrl, Modifier::Alt], "c")),
        Some(&Action::Copy),
        "\"copy\" sorts after \"clear_scrollback\" and wins"
    );
    assert!(
        !effective
            .values()
            .any(|action| *action == Action::ClearScrollback),
        "the colliding loser ends up unbound, never double-bound"
    );
}

#[test]
fn every_default_action_has_a_stable_id_that_round_trips() {
    for (key_bind, action) in default_key_binds() {
        let id = action_id(action).expect("every default bind is keybindable");
        assert_eq!(action_from_id(id), Some(action), "{key_bind} → {id}");
    }
}

#[test]
fn new_profile_tab_is_deliberately_not_keybindable() {
    assert_eq!(action_id(Action::NewProfileTab(0)), None);
}

#[test]
fn switch_profile_tab_is_deliberately_not_keybindable() {
    // The NewProfileTab contract verbatim: a binding over a
    // config-list index is a binding over an unstable id — the
    // menu pick is the only surface.
    assert_eq!(action_id(Action::SwitchProfileTab(0)), None);
}

#[test]
fn ids_are_the_pinned_file_format_literals() {
    // These strings are the config file's contract — spelled here
    // once so a rename cannot slip through silently.
    assert_eq!(action_id(Action::Copy), Some("copy"));
    assert_eq!(action_id(Action::ZoomIn), Some("zoom_in"));
    assert_eq!(action_id(Action::Settings), Some("settings"));
    assert_eq!(action_id(Action::ActivateTab(0)), Some("activate_tab_1"));
    assert_eq!(action_id(Action::ActivateTab(7)), Some("activate_tab_8"));
    assert_eq!(
        action_id(Action::ActivateLastTab),
        Some("activate_last_tab")
    );
    assert_eq!(
        action_from_id("activate_tab_3"),
        Some(Action::ActivateTab(2))
    );
    assert_eq!(action_from_id("activate_tab_9"), None);
    assert_eq!(action_from_id("ghost"), None);
}

#[test]
fn groups_cover_every_bindable_action_exactly_once() {
    let groups = action_groups().collect::<Vec<_>>();
    let titles: Vec<&str> = groups.iter().map(|group| group.title).collect();
    assert_eq!(
        titles,
        vec!["Clipboard", "Tabs", "Zoom", "Other"],
        "the section titles are part of the tab's shape"
    );
    let rows: Vec<Action> = groups.into_iter().flat_map(|group| group.actions).collect();
    // 22 payload-free variants plus the eight numbered activate
    // rows — NewProfileTab is the one deliberate exclusion.
    assert_eq!(rows.len(), 30);
    let unique: std::collections::HashSet<Action> = rows.iter().copied().collect();
    assert_eq!(unique.len(), rows.len(), "no action appears twice");
    assert!(rows.iter().all(|action| action_id(*action).is_some()));
}

#[test]
fn labels_are_readable_english() {
    assert_eq!(action_label(Action::Copy), "Copy");
    assert_eq!(action_label(Action::ActivateTab(0)), "Activate tab 1");
    assert_eq!(action_label(Action::ActivateLastTab), "Activate last tab");
    assert_eq!(action_label(Action::Settings), "Open settings");
    assert_eq!(
        action_label(Action::SwitchProfileTab(0)),
        "Switch profile tab"
    );
}

#[test]
fn binding_from_key_ignores_modifier_only_presses() {
    // A bare modifier can't be a binding — capture stays armed on
    // them (KE-5's "ignore" verdict rides this None).
    assert_eq!(
        binding_from_key(Modifiers::CTRL, &Key::Named(Named::Shift)),
        None
    );
}

#[test]
fn a_captured_binding_survives_the_round_trip_through_matching() {
    // Capture normalizes to a Binding, persistence round-trips it,
    // and the effective map must still match the original press —
    // the whole feature in one line.
    let binding = binding_from_key(
        Modifiers::CTRL | Modifiers::SHIFT,
        &Key::Character("c".into()),
    )
    .expect("an ordinary chord parses");
    let customs = BTreeMap::from([("zoom_in".to_string(), binding)]);
    let effective = effective_key_binds(&customs);
    assert_eq!(
        matched_action(
            &effective,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("c".into()),
            None
        ),
        Some(Action::ZoomIn)
    );
}

#[test]
fn binding_display_uses_the_keybind_format() {
    let binding = Binding {
        modifiers: vec![ModifierName::Ctrl, ModifierName::Shift],
        key: "C".to_string(),
    };
    assert_eq!(binding_display(&binding), "Ctrl + Shift + C");
}

#[test]
fn unknown_ids_and_unparsable_bindings_are_skipped_not_fatal() {
    // A hand-edited config can carry stale ids or empty keys;
    // both skip with a warn and the action keeps its defaults.
    let customs = custom(&[
        ("ghost_action", &[ModifierName::Ctrl], "x"),
        ("copy", &[], ""),
    ]);
    let effective = effective_key_binds(&customs);
    assert_eq!(
        effective.get(&bind(&[Modifier::Ctrl, Modifier::Shift], "c")),
        Some(&Action::Copy),
        "the broken copy entry leaves the default in place"
    );
    assert_eq!(effective.get(&bind(&[Modifier::Ctrl], "x")), None);
}

#[test]
fn capture_verdict_escape_cancels_even_with_modifiers() {
    // The drawer's own Escape rule ignores modifiers; the capture
    // rule matches that, so Ctrl+Escape also cancels.
    assert_eq!(
        capture_verdict(Modifiers::empty(), &Key::Named(Named::Escape)),
        CaptureVerdict::Cancel
    );
    assert_eq!(
        capture_verdict(Modifiers::CTRL, &Key::Named(Named::Escape)),
        CaptureVerdict::Cancel
    );
}

#[test]
fn capture_verdict_bare_modifiers_stay_armed() {
    assert_eq!(
        capture_verdict(Modifiers::CTRL, &Key::Named(Named::Shift)),
        CaptureVerdict::Ignore
    );
    assert_eq!(
        capture_verdict(Modifiers::CTRL | Modifiers::SHIFT, &Key::Named(Named::Alt)),
        CaptureVerdict::Ignore
    );
}

#[test]
fn capture_verdict_any_other_chord_commits() {
    assert_eq!(
        capture_verdict(
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("c".into())
        ),
        CaptureVerdict::Commit(Binding {
            modifiers: vec![ModifierName::Ctrl, ModifierName::Shift],
            key: "C".to_string(),
        })
    );
}

#[test]
fn with_custom_binding_sets_and_replaces_whole_entries() {
    // The capture-commit planner: whole map in, whole map out —
    // setting again replaces (single binding per action), a
    // numbered row lands under its per-number id, and an
    // unkeybindable action never enters the map.
    let empty = BTreeMap::new();
    let once = with_custom_binding(
        &empty,
        Action::Copy,
        Binding {
            modifiers: vec![ModifierName::Ctrl, ModifierName::Alt],
            key: "Q".to_string(),
        },
    );
    assert_eq!(
        once,
        custom(&[("copy", &[ModifierName::Ctrl, ModifierName::Alt], "Q")])
    );
    let twice = with_custom_binding(
        &once,
        Action::Copy,
        Binding {
            modifiers: vec![],
            key: "F2".to_string(),
        },
    );
    assert_eq!(twice.len(), 1, "re-setting replaces, never duplicates");
    assert_eq!(
        twice,
        custom(&[("copy", &[], "F2")]),
        "the replaced entry is the new binding"
    );
    let numbered = with_custom_binding(
        &twice,
        Action::ActivateTab(2),
        Binding {
            modifiers: vec![],
            key: "F3".to_string(),
        },
    );
    assert_eq!(
        numbered,
        custom(&[("copy", &[], "F2"), ("activate_tab_3", &[], "F3")])
    );
    assert_eq!(
        with_custom_binding(
            &numbered,
            Action::NewProfileTab(0),
            Binding {
                modifiers: vec![],
                key: "F4".to_string(),
            }
        ),
        numbered,
        "an unkeybindable action leaves the map unchanged"
    );
}

#[test]
fn binding_text_shows_joined_defaults_custom_or_none() {
    // The row's binding text (keybinds-editor KE-4): a custom
    // binding replaces the display wholesale; uncustomized actions
    // show every default chord, sorted for determinism ("+" sorts
    // before "=", and the unshifted chords before their Shift
    // forms); an action with no bindings at all shows "None".
    let custom = custom(&[("zoom_in", &[ModifierName::Alt], "z")]);
    assert_eq!(
        action_binding_display(Action::ZoomIn, &BTreeMap::new()),
        "Ctrl + + / Ctrl + = / Ctrl + Shift + + / Ctrl + Shift + ="
    );
    assert_eq!(action_binding_display(Action::ZoomIn, &custom), "Alt + Z");
    assert_eq!(
        action_binding_display(Action::ZoomOut, &BTreeMap::new()),
        "Ctrl + - / Ctrl + Shift + -"
    );
    assert_eq!(
        action_binding_display(Action::Copy, &BTreeMap::new()),
        "Ctrl + Shift + C"
    );
    assert_eq!(
        action_binding_display(Action::CopyPrimary, &BTreeMap::new()),
        "None",
        "CopyPrimary ships unbound by default"
    );
}

#[test]
fn search_matches_labels_case_insensitively() {
    // The Keybindings tab's filter: case-insensitive substring over
    // labels; an empty query matches everything.
    assert!(action_matches_search(Action::Copy, ""));
    assert!(action_matches_search(Action::Copy, "cop"));
    assert!(action_matches_search(Action::Copy, "COPY"));
    assert!(!action_matches_search(Action::Copy, "zoom"));
    assert!(action_matches_search(Action::ZoomIn, "zoom"));
    assert!(action_matches_search(Action::ActivateTab(0), "tab 1"));
    assert!(!action_matches_search(Action::Copy, "xyzzy"));
}

// --- the capture-commit verdict and revert (keybinds-editor KE-5) ---

#[test]
fn a_free_chord_commits() {
    let verdict = commit_verdict(
        Action::Copy,
        &Binding {
            modifiers: vec![ModifierName::Ctrl, ModifierName::Alt],
            key: "Q".to_string(),
        },
        &BTreeMap::new(),
    );
    assert_eq!(verdict, CommitVerdict::Commit);
}

#[test]
fn the_action_s_own_effective_chord_disarms_silently() {
    // Spec open-question 2's ruling: re-capturing the chord the
    // action already holds writes nothing — capture just ends.
    // Both flavors: the default chord with no custom, and the
    // custom chord itself.
    assert_eq!(
        commit_verdict(
            Action::Copy,
            &Binding {
                modifiers: vec![ModifierName::Ctrl, ModifierName::Shift],
                key: "C".to_string(),
            },
            &BTreeMap::new(),
        ),
        CommitVerdict::Own
    );
    let custom = custom(&[("copy", &[ModifierName::Alt], "Q")]);
    assert_eq!(
        commit_verdict(
            Action::Copy,
            &Binding {
                modifiers: vec![ModifierName::Alt],
                key: "Q".to_string(),
            },
            &custom,
        ),
        CommitVerdict::Own
    );
}

#[test]
fn a_chord_another_action_holds_is_a_conflict_naming_the_holder() {
    // Default holder: Paste owns Ctrl+Shift+V by default; capturing
    // it for Copy is a Conflict(Paste) — never a write.
    assert_eq!(
        commit_verdict(
            Action::Copy,
            &Binding {
                modifiers: vec![ModifierName::Ctrl, ModifierName::Shift],
                key: "V".to_string(),
            },
            &BTreeMap::new(),
        ),
        CommitVerdict::Conflict(Action::Paste)
    );
    // Custom holder: ZoomOut's custom chord is just as owned.
    let custom = custom(&[("zoom_out", &[ModifierName::Alt], "Q")]);
    assert_eq!(
        commit_verdict(
            Action::Copy,
            &Binding {
                modifiers: vec![ModifierName::Alt],
                key: "Q".to_string(),
            },
            &custom,
        ),
        CommitVerdict::Conflict(Action::ZoomOut)
    );
}

#[test]
fn a_retired_default_chord_is_free_again_for_its_owner() {
    // ZoomIn customized to Alt+Q retires its Ctrl+= / Ctrl+Plus
    // defaults; re-capturing Ctrl+= for ZoomIn is a fresh Commit
    // (it becomes the custom), not Own — the effective binding is
    // Alt+Q, and a Commit here is what restores "=" as the custom.
    let custom = custom(&[("zoom_in", &[ModifierName::Alt], "Q")]);
    assert_eq!(
        commit_verdict(
            Action::ZoomIn,
            &Binding {
                modifiers: vec![ModifierName::Ctrl],
                key: "=".to_string(),
            },
            &custom,
        ),
        CommitVerdict::Commit
    );
    // ...and for anyone else, Ctrl+= is free too (ZoomIn retired
    // it) — the overlay's replace semantics keep no spare claims.
    assert_eq!(
        commit_verdict(
            Action::Copy,
            &Binding {
                modifiers: vec![ModifierName::Ctrl],
                key: "=".to_string(),
            },
            &custom,
        ),
        CommitVerdict::Commit
    );
}

#[test]
fn an_unparsable_chord_is_unsupported() {
    // Only a hand-edited config can hold one: capture never
    // produces an empty key, and every non-empty name parses
    // (cosmic-term's Character fallthrough — a stored "KanaMode"
    // becomes an unmatchable Character key, dead but harmless).
    assert_eq!(
        commit_verdict(
            Action::Copy,
            &Binding {
                modifiers: vec![],
                key: String::new(),
            },
            &BTreeMap::new(),
        ),
        CommitVerdict::Unsupported
    );
}

#[test]
fn revert_removes_only_its_own_entry() {
    let map = custom(&[("copy", &[], "F2"), ("zoom_out", &[], "F3")]);
    assert_eq!(
        without_custom_binding(&map, Action::Copy),
        custom(&[("zoom_out", &[], "F3")])
    );
    // Reverting an action with no entry is the same map.
    assert_eq!(without_custom_binding(&map, Action::Paste), map);
    // Unkeybindable actions never had an entry either.
    assert_eq!(without_custom_binding(&map, Action::NewProfileTab(0)), map);
}
