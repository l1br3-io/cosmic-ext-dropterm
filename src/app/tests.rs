use alacritty_terminal::vte::ansi::NamedColor;

use super::theme_glue::effective_colors;
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

/// Decode a packed `0xRRGGBB` literal into an `Rgb` (mirrors
/// `theme_source`'s test helper).
#[expect(
    clippy::cast_possible_truncation,
    reason = "low-byte extraction is the 0xRRGGBB decode contract"
)]
pub(super) fn rgb(data: u32) -> alacritty_terminal::vte::ansi::Rgb {
    alacritty_terminal::vte::ansi::Rgb {
        r: (data >> 16) as u8,
        g: (data >> 8) as u8,
        b: data as u8,
    }
}

pub(super) fn chain_profile(name: &str, scheme: Option<&str>) -> crate::profile::Profile {
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

pub(super) fn chain_config(profiles: &[crate::profile::Profile]) -> DropTermConfig {
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

#[test]
fn bar_border_lines_never_share_a_seam() {
    // Bar-borders spec: through every stacking, the strip's line
    // (terminal-facing — `bar_tooltip_side`) and the band's line
    // (away from the grab bar — `band_menu_side`, the side the
    // widgets already hold) land in different seams: no boundary
    // between strips ever carries two hairlines. Where the two
    // barred strips adjoin, only the band's line renders, because
    // the strip's always faces the terminal.
    for edge in [Edge::Top, Edge::Bottom] {
        for tab_position in [TabPosition::Auto, TabPosition::Top, TabPosition::Bottom] {
            let stacking = crate::status_bar::column_stacking(edge, tab_position);
            let index_of = |strip: crate::status_bar::Strip| {
                stacking.iter().position(|s| *s == strip).expect("stacked")
            };
            // The seam a bar's line owns: the ordered index pair of
            // the bar and the neighbor its side points to.
            let seam = |from: crate::status_bar::Strip, side: TooltipSide| {
                let index = index_of(from);
                let neighbor = match side {
                    TooltipSide::Above => index.checked_sub(1),
                    TooltipSide::Below => index.checked_add(1),
                }
                .expect("the line side always points at a stacked neighbor");
                (index.min(neighbor), index.max(neighbor))
            };
            let tab_seam = seam(
                crate::status_bar::Strip::Tabs,
                bar_tooltip_side(effective_bar_at_bottom(edge, tab_position)),
            );
            let band_seam = seam(crate::status_bar::Strip::StatusBand, band_menu_side(edge));
            assert_ne!(
                tab_seam, band_seam,
                "doubled hairline in the same seam at {edge:?}/{tab_position:?}"
            );
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn clipboard_anchor_carries_the_app_id() {
    // The anchor is the app's only mapped xdg toplevel — Wayland
    // cannot unmap it — so the shell's window switcher names it by
    // this application_id alone. With the empty default the switcher
    // matched no desktop file and labeled the window "unknown"; the
    // id must stay the app id so it resolves to DropTerm's entry.
    assert_eq!(
        clipboard_anchor_settings().platform_specific.application_id,
        APP_ID
    );
}
