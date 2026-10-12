use super::*;

// --- the settings drawer's key routing (settings-drawer Task 0) ---

#[test]
fn escape_with_the_drawer_open_closes_it_and_only_it() {
    // Criterion 6: with the drawer open and no draft in it, the
    // press closes the drawer — it never hides the surface and
    // never reaches the terminal (disabled while the drawer is
    // open, criterion 4).
    assert_eq!(
        drawer_escape_routing(true, false, &Key::Named(Named::Escape)),
        DrawerKeyRouting::CloseDrawer
    );
}

#[test]
fn escape_with_the_drawer_closed_keeps_today_s_routing() {
    // Criterion 6's second half: with the drawer closed, Escape
    // behaves exactly as today — terminal input, the rule never
    // fires, draft or no draft (a draft cannot outlive the drawer
    // anyway; the pin is the precedence).
    assert_eq!(
        drawer_escape_routing(false, false, &Key::Named(Named::Escape)),
        DrawerKeyRouting::FallThrough
    );
    assert_eq!(
        drawer_escape_routing(false, true, &Key::Named(Named::Escape)),
        DrawerKeyRouting::FallThrough
    );
}

#[test]
fn every_other_key_falls_through_even_with_the_drawer_open() {
    // The Ctrl+Shift+I toggle resolves through the normal bind
    // match (criterion 5), and typing never trips the escape
    // rule — only Escape is the drawer's key, draft or no draft.
    for key in [
        Key::Character("i".into()),
        Key::Character("a".into()),
        Key::Named(Named::Enter),
    ] {
        assert_eq!(
            drawer_escape_routing(true, false, &key),
            DrawerKeyRouting::FallThrough
        );
        assert_eq!(
            drawer_escape_routing(true, true, &key),
            DrawerKeyRouting::FallThrough
        );
    }
}

// --- the draft-first Escape rule (profile-form-commit PF-3) ---

#[test]
fn escape_with_a_profile_draft_open_cancels_it_and_keeps_the_drawer() {
    // The draft-first rule: with the drawer open and a profile
    // draft present, Escape cancels the draft — the drawer stays
    // open beneath (a second Escape, draft now gone, closes it as
    // today). The cancel writes nothing and keeps the selection.
    assert_eq!(
        drawer_escape_routing(true, true, &Key::Named(Named::Escape)),
        DrawerKeyRouting::CancelDraft
    );
}

#[test]
fn the_modal_owns_escape_ahead_of_the_draft_rule() {
    // The precedence stack, pinned end to end: while a close
    // confirmation is pending, the modal's gate answers first —
    // its Escape cancels the modal (not the draft, not the
    // drawer), and only a later Escape reaches the draft rule.
    // `modal_key_routing` sits ahead of `drawer_escape_routing` in
    // the key funnel, so its verdict is the one that fires.
    assert_eq!(
        modal_key_routing(true, &Key::Named(Named::Escape)),
        ModalKeyRouting::Cancel
    );
}

// --- the close-confirmation gate and modal key routing
// (close-confirm T3) ---

#[test]
fn confirm_needed_requires_both_the_toggle_and_a_running_child() {
    // Criterion 3's truth table as one pin: either input false →
    // immediate close. The toggle-off row is criterion 5
    // (byte-identical to today); the idle-shell row is criterion
    // 2 (a tab at its prompt never confirms).
    assert!(!confirm_needed(false, false));
    assert!(!confirm_needed(false, true));
    assert!(!confirm_needed(true, false));
    assert!(confirm_needed(true, true));
}

#[test]
fn escape_while_a_close_confirms_cancels_the_modal_not_the_drawer() {
    // Criterion 4's precedence: the modal's routing sits ahead of
    // `drawer_escape_routing`, so Escape with both the drawer and
    // a pending close cancels the modal only — the drawer stays
    // open beneath (a second Escape, unpending now, closes it as
    // today).
    assert_eq!(
        modal_key_routing(true, &Key::Named(Named::Escape)),
        ModalKeyRouting::Cancel
    );
}

#[test]
fn every_other_key_is_swallowed_while_a_close_confirms() {
    // While the decision waits, the modal owns the keyboard:
    // binds never fire and nothing reaches the terminal (pointer
    // blocking comes free from the scrim).
    for key in [
        Key::Character("w".into()),
        Key::Named(Named::Enter),
        Key::Named(Named::Tab),
    ] {
        assert_eq!(
            modal_key_routing(true, &key),
            ModalKeyRouting::Swallow,
            "{key:?} must be swallowed while a close confirms"
        );
    }
}

#[test]
fn modal_key_routing_is_transparent_without_a_pending_close() {
    // No pending close: every key — Escape included — keeps
    // today's routing; the rule never fires.
    assert_eq!(
        modal_key_routing(false, &Key::Named(Named::Escape)),
        ModalKeyRouting::FallThrough
    );
    assert_eq!(
        modal_key_routing(false, &Key::Character("w".into())),
        ModalKeyRouting::FallThrough
    );
}

// --- the armed capture's key routing (keybinds-editor KE-3) ---

#[test]
fn a_disarmed_capture_gate_is_transparent() {
    // No capture armed: no verdict, so the press keeps today's
    // routing — rename guard, drawer Escape, bind match — exactly
    // as before the editor existed (criterion: disarmed behavior
    // byte-identical).
    assert_eq!(
        capture_verdict_for(None, Modifiers::empty(), &Key::Named(Named::Escape)),
        None
    );
    assert_eq!(
        capture_verdict_for(
            None,
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("c".into())
        ),
        None
    );
}

#[test]
fn an_armed_capture_owns_escape_before_the_drawer_close_rule() {
    // While armed, the capture gate sits ahead of
    // `drawer_escape_routing`: Escape cancels the capture instead
    // of closing the drawer (a later Escape — disarmed again —
    // closes as today).
    assert_eq!(
        capture_verdict_for(
            Some(Action::Copy),
            Modifiers::empty(),
            &Key::Named(Named::Escape)
        ),
        Some(keybinds::CaptureVerdict::Cancel)
    );
}

#[test]
fn an_armed_capture_consumes_bound_chords_instead_of_matching_them() {
    // Ctrl+Shift+C is Copy's live binding, but while a capture is
    // armed the gate takes it as the captured chord — bind
    // matching never runs (criterion: capture consumes every
    // press, even a bound keybind).
    assert_eq!(
        capture_verdict_for(
            Some(Action::ZoomIn),
            Modifiers::CTRL | Modifiers::SHIFT,
            &Key::Character("c".into())
        ),
        Some(keybinds::CaptureVerdict::Commit(keybinds::Binding {
            modifiers: vec![keybinds::ModifierName::Ctrl, keybinds::ModifierName::Shift],
            key: "C".to_string(),
        }))
    );
    // A bare modifier never commits — the capture stays armed.
    assert_eq!(
        capture_verdict_for(
            Some(Action::ZoomIn),
            Modifiers::CTRL,
            &Key::Named(Named::Shift)
        ),
        Some(keybinds::CaptureVerdict::Ignore)
    );
}

// --- the drawer's page turns (settings-ia Task 1) ---

#[test]
fn a_pick_away_from_the_profiles_tab_clears_its_page_state() {
    // The drill-down's draft rule, carried over to the tab: only
    // a turn *away* from Profiles drops the page's draft state —
    // re-picking the Profiles row is not a leave, and no other
    // turn ever had a draft to clear. Focus-loss hides never
    // route here (preservation is their rule).
    for to in [
        SettingsTab::Appearance,
        SettingsTab::Behavior,
        SettingsTab::Keybindings,
        SettingsTab::Advanced,
    ] {
        assert!(leaves_profiles_tab(SettingsTab::Profiles, to));
    }
    assert!(!leaves_profiles_tab(
        SettingsTab::Profiles,
        SettingsTab::Profiles
    ));
    assert!(!leaves_profiles_tab(
        SettingsTab::Appearance,
        SettingsTab::Profiles
    ));
    assert!(!leaves_profiles_tab(
        SettingsTab::Advanced,
        SettingsTab::Appearance
    ));
}

#[test]
fn the_explicit_close_resets_to_the_appearance_tab() {
    // The landing rule: an explicit close lands on the first
    // tab, so a reopened drawer always starts on the everyday
    // controls.
    assert_eq!(home_settings_page(), SettingsTab::Appearance);
}

// --- the window-background source decision ---

#[test]
fn scheme_background_wins_over_the_system_derivation() {
    // Dracula owns its background (#282a36, alacritty-theme
    // Dracula.toml): the resolved slot must land in
    // WINDOW_BG_COLOR's packed layout (0xAARRGGBB, opaque), not
    // the system theme's — chrome and the unset-bg fallback then
    // follow by construction.
    let dracula = theme_source::by_name("Dracula").unwrap();
    assert_eq!(window_bg_color(&dracula, 0xFF1E_1E2E), 0xFF28_2A36);
}

#[test]
fn unset_background_passes_the_system_derivation_through() {
    // The COSMIC pair owns no background (the system theme
    // supplies it), so the decision is a plain passthrough —
    // default-config chrome follows the system theme, and a
    // bogus scheme name keeps that too (the `active_colors`
    // fallback resolves to this same pair).
    let cosmic_dark = theme_source::by_name("COSMIC Dark").unwrap();
    assert_eq!(window_bg_color(&cosmic_dark, 0xFF1E_1E2E), 0xFF1E_1E2E);
}

#[test]
fn the_fallback_palette_matches_the_effective_mode() {
    // The window-background fallback derives from the palette of the
    // mode the caller decided (`effective_is_dark`), never from the
    // process-global active theme — a post-pin startup race can leave
    // the global on the opposite palette with the pin still "set"
    // (hardware walk 2026-10-12: dark preference, light global,
    // pale pane). Both arms resolve a palette whose own `is_dark`
    // agrees with the requested mode, whatever cosmic-config returns
    // (built-in fallbacks agree too), so a drifted global cannot
    // leak into the fallback.
    assert!(system_palette_for_mode(true).cosmic().is_dark);
    assert!(!system_palette_for_mode(false).cosmic().is_dark);
}

// --- the per-tab appearance resolution chain ---
//
// `resolved_profile` + `effective_colors` is the one chain both
// application points consume (spawn and the config-apply pass):
// the tab's recorded profile id → the profile → the seams → the
// built-in registry. Every miss falls back to the global pick;
// the chain never errors. Nord plays the global below — its
// slots differ from Dracula's everywhere asserted, so a silent
// fallback to the scheme (or a scheme leaking into a fallback)
// cannot pass.

/// Decode a packed `0xRRGGBB` literal into an `Rgb` (mirrors
/// `theme_source`'s test helper).
#[expect(
    clippy::cast_possible_truncation,
    reason = "low-byte extraction is the 0xRRGGBB decode contract"
)]
fn rgb(data: u32) -> alacritty_terminal::vte::ansi::Rgb {
    alacritty_terminal::vte::ansi::Rgb {
        r: (data >> 16) as u8,
        g: (data >> 8) as u8,
        b: data as u8,
    }
}

fn chain_profile(name: &str, scheme: Option<&str>) -> crate::profile::Profile {
    crate::profile::Profile {
        name: name.to_string(),
        command: "ssh host".to_string(),
        cwd: None,
        color_scheme: scheme.map(str::to_owned),
        env: None,
        font_size: None,
        scrollback_lines: None,
    }
}

fn chain_config(profiles: &[crate::profile::Profile]) -> DropTermConfig {
    // Nord as the global pair — a real registry scheme whose
    // slots differ from Dracula's everywhere asserted below.
    DropTermConfig {
        profiles: profiles.to_vec(),
        color_scheme_dark: "Nord".to_string(),
        color_scheme_light: "Nord".to_string(),
        ..DropTermConfig::default()
    }
}

#[test]
fn effective_colors_resolves_a_known_scheme_name() {
    let config = chain_config(&[chain_profile("ssh-prod", Some("Dracula"))]);
    let profile = resolved_profile(&config, Some("ssh-prod"));
    let colors = effective_colors(&config, profile, true);
    // Dracula owns bg #282a36 and red #ff5555 (alacritty-theme).
    assert_eq!(colors[NamedColor::Background], Some(rgb(0x28_2A36)));
    assert_eq!(colors[NamedColor::Red], Some(rgb(0xFF_5555)));
}

#[test]
fn effective_colors_falls_back_to_global_for_a_plain_tab() {
    // No recorded id — plain shell tabs, and the degraded preset
    // arm whose blank command line records none (the shared
    // `profile_id = None` rule): the global pick, unchanged.
    let config = chain_config(&[chain_profile("ssh-prod", Some("Dracula"))]);
    let colors = effective_colors(&config, resolved_profile(&config, None), true);
    assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
}

#[test]
fn effective_colors_falls_back_to_global_on_a_dangling_profile_id() {
    // The tab records a profile the config no longer carries
    // (deleted mid-run): the lookup is `None`, the global pick —
    // never an error, never a rewrite.
    let config = chain_config(&[chain_profile("ssh-prod", Some("Dracula"))]);
    assert_eq!(resolved_profile(&config, Some("ghost")), None);
    let colors = effective_colors(&config, resolved_profile(&config, Some("ghost")), true);
    assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
}

#[test]
fn effective_colors_falls_back_to_global_when_the_profile_sets_no_scheme() {
    // A profile without a scheme override: its tabs follow the
    // global pick.
    let config = chain_config(&[chain_profile("ssh-prod", None)]);
    let profile = resolved_profile(&config, Some("ssh-prod"));
    let colors = effective_colors(&config, profile, true);
    assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
}

#[test]
fn effective_colors_falls_back_to_global_on_an_unknown_scheme_name() {
    // A hand-edited config can name a scheme the registry does
    // not ship ("Catppuccin Frappe" is a real upstream variant):
    // resolve global, silently — exactly how an unknown global
    // name falls to the COSMIC pair — never an error, never a
    // rewrite.
    let config = chain_config(&[chain_profile("ssh-prod", Some("Catppuccin Frappe"))]);
    let profile = resolved_profile(&config, Some("ssh-prod"));
    let colors = effective_colors(&config, profile, true);
    assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
}

#[test]
fn one_config_pass_resolves_mixed_tabs_to_different_colors() {
    // The apply criterion's headless pin: one config, one global
    // pick, two recorded ids (a profile tab and a plain tab) —
    // the per-tab resolutions differ, so one `apply_config_to_tabs`
    // pass paints them differently.
    let config = chain_config(&[chain_profile("ssh-prod", Some("Dracula"))]);
    let profile_tab = effective_colors(&config, resolved_profile(&config, Some("ssh-prod")), true);
    let plain_tab = effective_colors(&config, resolved_profile(&config, None), true);
    assert_ne!(
        profile_tab[NamedColor::Background],
        plain_tab[NamedColor::Background]
    );
    assert_ne!(profile_tab[NamedColor::Red], plain_tab[NamedColor::Red]);
}

#[test]
fn a_respawn_starts_from_the_new_profile_s_effective_values() {
    // The composition `switch_tab_profile` respawns through (the
    // same spawn internals every spawn path uses): the new
    // profile's id resolves, and every appearance value the
    // fresh terminal starts from — colors at creation, font,
    // scrollback capacity — comes from that resolution. A
    // respawn onto a no-override profile starts global instead:
    // the rule is per field, never per profile.
    let overridden = crate::profile::Profile {
        color_scheme: Some("Dracula".to_string()),
        font_size: Some(11),
        scrollback_lines: Some(50_000),
        ..chain_profile("web-prod", None)
    };
    let config = chain_config(&[overridden, chain_profile("plain", None)]);

    let web = resolved_profile(&config, Some("web-prod"));
    assert_eq!(
        effective_colors(&config, web, true)[NamedColor::Background],
        Some(rgb(0x28_2A36))
    );
    assert_eq!(
        crate::profile::effective_font_size(web, config.font_size),
        11
    );
    assert_eq!(
        crate::profile::effective_scrollback(web, config.scrollback_lines),
        50_000
    );

    let plain = resolved_profile(&config, Some("plain"));
    assert_eq!(
        effective_colors(&config, plain, true)[NamedColor::Background],
        Some(rgb(0x2E_3440))
    );
    assert_eq!(
        crate::profile::effective_font_size(plain, config.font_size),
        config.font_size
    );
    assert_eq!(
        crate::profile::effective_scrollback(plain, config.scrollback_lines),
        config.scrollback_lines
    );
}

// --- the clipboard anchor ---
//
// libcosmic binds its single runtime Clipboard to the first
// window created and drops that binding — and with it the
// smithay-clipboard connection holding any copied selection —
// when the owning surface is destroyed. DropTerm's only surface
// is the layer shell, which every hide destroys, so a copy could
// never survive the focus switch to wherever it would be pasted.
// An invisible, never-closed normal window created first owns
// the binding forever; these pin the two invariants that keep it
// both invisible and immortal.

#[test]
fn clipboard_anchor_window_is_invisible() {
    // Unmapped: absent from the dock and Alt-Tab, and it never
    // paints — it exists only to hold the clipboard binding.
    assert!(!clipboard_anchor_settings().visible);
}

#[test]
fn clipboard_anchor_window_cannot_be_closed() {
    // No close request is ever honored (and `exit_on_close(false)`
    // in main means the runtime cannot exit through it either),
    // so the binding outlives every hide/show cycle.
    assert!(!clipboard_anchor_settings().exit_on_close_request);
}

// --- the pure theme-preference decisions ---

#[test]
fn effective_is_dark_pins_dark_and_light() {
    // A pin wins regardless of the system mode — that is the
    // whole point of the feature.
    assert!(effective_is_dark(ThemePreference::Dark, false));
    assert!(effective_is_dark(ThemePreference::Dark, true));
    assert!(!effective_is_dark(ThemePreference::Light, false));
    assert!(!effective_is_dark(ThemePreference::Light, true));
}

#[test]
fn effective_is_dark_system_follows_the_injected_mode() {
    // The system value is a parameter, not a global read (the
    // same pattern as `window_bg_color`): the decision stays
    // pure, and tests never depend on the process-wide THEME
    // state.
    assert!(effective_is_dark(ThemePreference::System, true));
    assert!(!effective_is_dark(ThemePreference::System, false));
}

#[test]
fn echo_retitles_on_a_title_mode_flip_regardless_of_profiles() {
    // The mode-flip guard's standing rule: every mode reads
    // different inputs, so a flip always re-resolves every live
    // tab — even when the profiles list did not move.
    assert!(echo_retitles_tabs(
        TabTitleMode::Shell,
        TabTitleMode::Profile,
        &[],
        &[]
    ));
    assert!(echo_retitles_tabs(
        TabTitleMode::Profile,
        TabTitleMode::Shell,
        &[chain_profile("ssh", None)],
        &[chain_profile("ssh", None)]
    ));
}

#[test]
fn echo_retitles_on_a_profiles_change_only_in_profile_mode() {
    // Ruling 1: a profile renamed or deleted under live tabs is a
    // stale name the moment it happens — while the (new) mode is
    // Profile, a profiles-list change retitles. In every other
    // mode the titles read no profile name, so the same change
    // retitles nothing.
    let old = vec![chain_profile("ssh", None)];
    let renamed = vec![chain_profile("server", None)];
    assert!(echo_retitles_tabs(
        TabTitleMode::Profile,
        TabTitleMode::Profile,
        &old,
        &renamed
    ));
    assert!(!echo_retitles_tabs(
        TabTitleMode::Shell,
        TabTitleMode::Shell,
        &old,
        &renamed
    ));
    assert!(!echo_retitles_tabs(
        TabTitleMode::LastSegment,
        TabTitleMode::LastSegment,
        &old,
        &renamed
    ));
}

#[test]
fn echo_never_retitles_on_an_unrelated_change() {
    // The anti-churn pin: an echo that moves neither the mode nor
    // (in Profile mode) the profiles list — an opacity slide, a
    // keybind, our own profile write's echo before and after —
    // retitles nothing.
    let profiles = vec![chain_profile("ssh", None)];
    assert!(!echo_retitles_tabs(
        TabTitleMode::Shell,
        TabTitleMode::Shell,
        &profiles,
        &profiles.clone()
    ));
    assert!(!echo_retitles_tabs(
        TabTitleMode::Profile,
        TabTitleMode::Profile,
        &profiles,
        &profiles.clone()
    ));
    // The empty list is a value like any other: a config that
    // gains its first profile retitles in Profile mode...
    assert!(echo_retitles_tabs(
        TabTitleMode::Profile,
        TabTitleMode::Profile,
        &[],
        &profiles
    ));
    // ...and loses its last without one.
    assert!(echo_retitles_tabs(
        TabTitleMode::Profile,
        TabTitleMode::Profile,
        &profiles,
        &[]
    ));
}

#[test]
fn theme_override_is_none_when_preference_is_unchanged() {
    // Steady state issues nothing: a default `System` config
    // never calls `set_theme`, and unrelated config echoes —
    // including our own theme echo — never storm the task.
    assert_eq!(
        theme_override(ThemePreference::System, ThemePreference::System),
        None
    );
    assert_eq!(
        theme_override(ThemePreference::Dark, ThemePreference::Dark),
        None
    );
    assert_eq!(
        theme_override(ThemePreference::Light, ThemePreference::Light),
        None
    );
}

#[test]
fn theme_override_pins_on_any_transition_to_dark_or_light() {
    assert_eq!(
        theme_override(ThemePreference::System, ThemePreference::Dark),
        Some(ThemeOverrideKind::PinDark)
    );
    assert_eq!(
        theme_override(ThemePreference::Light, ThemePreference::Dark),
        Some(ThemeOverrideKind::PinDark)
    );
    assert_eq!(
        theme_override(ThemePreference::System, ThemePreference::Light),
        Some(ThemeOverrideKind::PinLight)
    );
    assert_eq!(
        theme_override(ThemePreference::Dark, ThemePreference::Light),
        Some(ThemeOverrideKind::PinLight)
    );
}

#[test]
fn theme_override_restores_follow_system_from_a_pin() {
    assert_eq!(
        theme_override(ThemePreference::Dark, ThemePreference::System),
        Some(ThemeOverrideKind::FollowSystem)
    );
    assert_eq!(
        theme_override(ThemePreference::Light, ThemePreference::System),
        Some(ThemeOverrideKind::FollowSystem)
    );
}

/// Drives a [`Task`](cosmic::app::Task)'s stream to completion on
/// the current thread, collecting every runtime
/// [`Action::Output`] message in delivery order. `Task::then`
/// consumes the first task's output and replaces it with the
/// continuation's, so only a stream-level check can prove a
/// message actually ships.
fn delivered_outputs<T: Send + 'static>(task: cosmic::app::Task<T>) -> Vec<cosmic::Action<T>> {
    use cosmic::iced::futures::StreamExt;
    use cosmic::iced::futures::executor::block_on;

    let mut stream =
        cosmic::iced::runtime::task::into_stream(task).expect("theme override task has a stream");
    let mut outputs = Vec::new();
    block_on(async {
        while let Some(action) = stream.next().await {
            if let cosmic::iced::runtime::Action::Output(message) = action {
                outputs.push(message);
            }
        }
    });
    outputs
}

#[test]
fn theme_override_task_delivers_set_theme_then_reapply() {
    // `Task::then` would CONSUME the `set_theme` output — the
    // pin message would never reach the runtime, the process
    // theme would never flip, and pinning Dark would leave a
    // light, frosted terminal. The task must deliver BOTH
    // messages: the theme override first, the `ReapplyColors`
    // echo second — `Task::chain`'s contract.
    for (kind, expected_prefer) in [
        (ThemeOverrideKind::PinDark, Some(true)),
        (ThemeOverrideKind::PinLight, Some(false)),
        // FollowSystem's palette is whatever the system prefers —
        // only the cleared marker is environment-independent.
        (ThemeOverrideKind::FollowSystem, None),
    ] {
        let outputs = delivered_outputs(theme_override_task(kind));
        assert_eq!(
            outputs.len(),
            2,
            "expected exactly the set_theme + ReapplyColors pair for {kind:?}"
        );
        let cosmic::Action::Cosmic(cosmic::app::Action::AppThemeChange(theme)) = &outputs[0] else {
            panic!("first delivered message must be the theme override for {kind:?}");
        };
        // The pins construct their System theme outright, so they
        // must always be System-typed with the exact marker.
        // FollowSystem re-reads the system mode, and on a host
        // with no theme-mode source (headless CI)
        // `system_preference()` falls back to a plain
        // `Theme::dark()` — no System typing, no marker to clear.
        match (&theme.theme_type, expected_prefer) {
            (cosmic::theme::ThemeType::System { prefer_dark, .. }, _) => {
                assert_eq!(*prefer_dark, expected_prefer, "marker for {kind:?}");
            }
            (_, None) => {}
            _ => panic!("override theme must be System-typed for {kind:?}"),
        }
        assert!(
            matches!(&outputs[1], cosmic::Action::App(Message::ReapplyColors)),
            "second delivered message must be the ReapplyColors echo for {kind:?}"
        );
    }
}

// --- bar placement + tooltip side ---

#[test]
fn effective_bar_at_bottom_auto_keeps_the_strip_at_the_leading_edge() {
    // Auto placement: the strip stacks opposite the screen edge
    // — surface bottom under a Top anchor, surface top under a
    // Bottom anchor.
    assert!(effective_bar_at_bottom(Edge::Top, TabPosition::Auto));
    assert!(!effective_bar_at_bottom(Edge::Bottom, TabPosition::Auto));
}

#[test]
fn effective_bar_at_bottom_top_pin_places_the_bar_at_the_top() {
    // A pin wins regardless of anchor.
    assert!(!effective_bar_at_bottom(Edge::Top, TabPosition::Top));
    assert!(!effective_bar_at_bottom(Edge::Bottom, TabPosition::Top));
}

#[test]
fn effective_bar_at_bottom_bottom_pin_places_the_bar_at_the_bottom() {
    assert!(effective_bar_at_bottom(Edge::Top, TabPosition::Bottom));
    assert!(effective_bar_at_bottom(Edge::Bottom, TabPosition::Bottom));
}

#[test]
fn tooltip_side_pops_terminal_ward_wherever_the_bar_sits() {
    // The tip pops to the terminal's side of the strip: below a
    // top bar, above a bottom bar — the anchor edge is not an
    // input.
    assert_eq!(bar_tooltip_side(false), TooltipSide::Below);
    assert_eq!(bar_tooltip_side(true), TooltipSide::Above);
}

#[test]
fn tooltip_side_auto_layouts_match_today_on_both_edges() {
    // Regression pins: the derived side must match the fixed
    // `Auto`-layout values on both edges.
    assert_eq!(
        bar_tooltip_side(effective_bar_at_bottom(Edge::Top, TabPosition::Auto)),
        TooltipSide::Above
    );
    assert_eq!(
        bar_tooltip_side(effective_bar_at_bottom(Edge::Bottom, TabPosition::Auto)),
        TooltipSide::Below
    );
}

#[test]
fn tooltip_side_pinned_layouts_follow_the_bar_not_the_anchor() {
    // A strip pinned to the screen edge still tips terminal-ward.
    assert_eq!(
        bar_tooltip_side(effective_bar_at_bottom(Edge::Top, TabPosition::Top)),
        TooltipSide::Below
    );
    assert_eq!(
        bar_tooltip_side(effective_bar_at_bottom(Edge::Bottom, TabPosition::Bottom)),
        TooltipSide::Above
    );
}

#[test]
fn band_menu_side_pops_terminal_ward_on_both_edges() {
    // The band hugs the free edge beside the grab bar, so its
    // terminal side is opposite the tab strip's for the same edge:
    // above a top-anchored band, below a bottom-anchored one — the
    // only side with surface under the bubble.
    assert_eq!(band_menu_side(Edge::Top), TooltipSide::Above);
    assert_eq!(band_menu_side(Edge::Bottom), TooltipSide::Below);
}
