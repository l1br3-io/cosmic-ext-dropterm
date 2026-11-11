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
        drawer_escape_routing(true, false, false, &Key::Named(Named::Escape)),
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
        drawer_escape_routing(false, false, false, &Key::Named(Named::Escape)),
        DrawerKeyRouting::FallThrough
    );
    assert_eq!(
        drawer_escape_routing(false, false, true, &Key::Named(Named::Escape)),
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
            drawer_escape_routing(true, false, false, &key),
            DrawerKeyRouting::FallThrough
        );
        assert_eq!(
            drawer_escape_routing(true, false, true, &key),
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
        drawer_escape_routing(true, false, true, &Key::Named(Named::Escape)),
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

#[test]
fn the_default_config_hides_the_app_from_the_shell() {
    // OQ 1: the reported bug becomes the fixed default — hidden
    // mode, the layer-surface anchor; the dock body is opt-in.
    assert!(!crate::config::DropTermConfig::default().show_in_dock);
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

#[test]
fn the_local_hostname_reads_trimmed_or_absent() {
    // (osc7-cwd spec) A trim regression here flips every report —
    // local ones included — to host-qualified rendering: an untrimmed
    // name never equals the reported host. The `None` arm (an
    // unreadable /proc) is engine-pinned — named hosts read remote.
    if let Some(name) = read_local_hostname() {
        assert!(!name.is_empty());
        assert_eq!(name, name.trim(), "the /proc read must land trimmed");
    }
}

// --- the images panel drawer (paste-panel spec, PP-2) ---

#[test]
fn escape_closes_the_images_drawer() {
    // The images panel owns Escape exactly as the settings drawer
    // does — its own close path, never the surface's.
    assert_eq!(
        drawer_escape_routing(false, true, false, &Key::Named(Named::Escape)),
        DrawerKeyRouting::CloseDrawer
    );
}

#[test]
fn settings_wins_the_escape_when_both_drawers_could_be_open() {
    // Both open is impossible by construction (opening one closes
    // the other); the pin makes the defensive order explicit — the
    // settings drawer's richer close (buffer clears, page reset)
    // is the one that runs.
    assert_eq!(
        drawer_escape_routing(true, true, false, &Key::Named(Named::Escape)),
        DrawerKeyRouting::CloseDrawer
    );
}

#[test]
fn images_panel_rows_list_newest_first() {
    // The panel is a view over the ledger, newest first: the most
    // recent paste is the row the eye lands on.
    use crate::tabs::PastedImage;
    let rows = vec![
        PastedImage::new(std::path::PathBuf::from("/c/paste-1.png"), b"png"),
        PastedImage::new(std::path::PathBuf::from("/c/paste-2.png"), b"png"),
        PastedImage::new(std::path::PathBuf::from("/c/paste-3.png"), b"png"),
    ];
    let listed = images_panel_rows(&rows);
    assert_eq!(
        listed
            .iter()
            .map(|row| row.stamp.as_str())
            .collect::<Vec<_>>(),
        vec!["paste-3", "paste-2", "paste-1"]
    );
}

#[test]
fn forgetting_a_row_drops_it_and_its_thumbnail() {
    // The panel's Delete verb (paste-panel spec, PP-3): the row and
    // its cached thumbnail leave together; a path no row owns is a
    // no-op answered false.
    use crate::tabs::PastedImage;
    use cosmic::iced::widget::image::Handle;
    use std::collections::HashMap;
    let path = std::path::PathBuf::from("/c/paste-1.png");
    let mut rows = vec![PastedImage::new(path.clone(), b"png")];
    let mut thumbs = HashMap::from([(path.clone(), Handle::from_rgba(1, 1, vec![255, 0, 0, 255]))]);
    assert!(forget_pasted_image(&mut rows, &mut thumbs, &path));
    assert!(rows.is_empty());
    assert!(thumbs.is_empty());
    assert!(!forget_pasted_image(
        &mut rows,
        &mut thumbs,
        std::path::Path::new("/c/other.png")
    ));
}

// --- the panel's thumbnail decode (the ship audit's guard ride) ---

/// A tiny solid-color PNG of the given dimensions, as bytes — the
/// tabs suite's own fixture shape.
fn panel_png_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(width, height, image::Rgba([200, 60, 60, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode the test png");
    bytes.into_inner()
}

/// A fresh temp dir per test, cleared first so re-runs start clean
/// (the clipboard suite's own fixture convention).
fn thumb_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("dropterm-thumb-tests-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_valid_png_decodes_into_a_thumbnail_handle() {
    let dir = thumb_dir("valid");
    std::fs::create_dir_all(&dir).expect("fixture dir");
    let path = dir.join("paste-20261108-140309-1.png");
    std::fs::write(&path, panel_png_bytes(64, 32)).expect("fixture png");
    let (back, handle) = decode_panel_thumb(path.clone());
    assert_eq!(back, path);
    assert!(handle.is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_non_image_payload_answers_none() {
    // A cache file that is not a decodable image (planted, or a
    // refused-decode leftover) renders its row text-only: the decode
    // answers None after its one warn.
    let dir = thumb_dir("garbage");
    std::fs::create_dir_all(&dir).expect("fixture dir");
    let path = dir.join("paste-20261108-140309-1.png");
    std::fs::write(&path, b"not a png at all").expect("fixture file");
    let (_, handle) = decode_panel_thumb(path);
    assert!(handle.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_over_cap_file_is_refused_before_its_bytes_are_read() {
    // The payload gate rides the clipboard read's own cap and fires
    // before the read: a sparse file over the cap (cheap to plant,
    // expensive to read) answers None without a decode attempt.
    let dir = thumb_dir("cap");
    std::fs::create_dir_all(&dir).expect("fixture dir");
    let path = dir.join("paste-20261108-140309-1.png");
    let file = std::fs::File::create(&path).expect("fixture file");
    file.set_len((crate::clipboard::PASTE_PAYLOAD_CAP + 1) as u64)
        .expect("extend the fixture sparsely");
    drop(file);
    let (_, handle) = decode_panel_thumb(path);
    assert!(handle.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

// --- the config store's watcher (config-store T4) ---

/// A fresh store directory per test, cleared first so re-runs start
/// clean (this file's fixture convention), the process id keeping
/// parallel suites apart.
fn store_fixture(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "dropterm-watcher-tests-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    dir
}

/// A synthetic watcher event naming `file` in `dir`.
fn event_naming(dir: &std::path::Path, file: &str) -> notify::Event {
    notify::Event::new(notify::EventKind::Any).add_path(dir.join(file))
}

#[test]
fn the_watcher_step_ignores_files_that_are_not_the_store_s() {
    // An atomic save's `.tmp` sibling, a quarantine's `.bak`, and a
    // stranger's file are noise: only the store's own names echo.
    let dir = store_fixture("noise");
    std::fs::write(dir.join("config.ron"), "(height_percent: 55)").expect("seed");
    let last = DropTermConfig::default();
    for noise in ["config.ron.tmp", "config.ron.bak", "notes.txt"] {
        assert_eq!(
            config_after(&event_naming(&dir, noise), &dir, &last),
            None,
            "{noise}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_watcher_step_delivers_a_changed_section() {
    let dir = store_fixture("changed");
    std::fs::write(dir.join("config.ron"), "(height_percent: 55)").expect("seed");
    let next = config_after(
        &event_naming(&dir, "config.ron"),
        &dir,
        &DropTermConfig::default(),
    );
    assert_eq!(next.map(|config| config.height_percent), Some(55));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_watcher_step_stays_silent_when_nothing_changed() {
    // A same-value reload is no echo — here a half-written section
    // that keeps the last-known values: nothing to apply, and the
    // rename's second event after a real change lands the same way.
    // The file-state assertions pin the silent path's non-destructive
    // half too: silence means the half-written file is left exactly
    // where the editor's in-flight save left it — never quarantined
    // — so a reload implementation that renamed on the peek would
    // fail here even when its Option answer stays silent.
    let dir = store_fixture("unchanged");
    std::fs::write(dir.join("config.ron"), "(height_perc").expect("half a section");
    let last = DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    };
    assert_eq!(
        config_after(&event_naming(&dir, "config.ron"), &dir, &last),
        None
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("config.ron")).expect("the half file survives"),
        "(height_perc",
        "the editor's in-flight file is never moved aside"
    );
    assert!(
        !dir.join("config.ron.bak").exists(),
        "a silent reload quarantines nothing"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_watcher_step_skips_events_that_mutate_no_data() {
    // Access and metadata events, and a rename's both-paths summary
    // (its to-half already delivered), change no bytes: they never
    // echo, even naming a store file whose content would.
    let dir = store_fixture("no-data");
    std::fs::write(dir.join("config.ron"), "(height_percent: 55)").expect("seed");
    let last = DropTermConfig::default();
    for kind in [
        // The open is the load-bearing one: the reload itself opens
        // the file, so an echoing open would re-trigger its own
        // reload without end.
        notify::EventKind::Access(notify::event::AccessKind::Open(
            notify::event::AccessMode::Any,
        )),
        notify::EventKind::Access(notify::event::AccessKind::Close(
            notify::event::AccessMode::Write,
        )),
        notify::EventKind::Modify(notify::event::ModifyKind::Metadata(
            notify::event::MetadataKind::Any,
        )),
        notify::EventKind::Modify(notify::event::ModifyKind::Name(
            notify::event::RenameMode::Both,
        )),
    ] {
        let event = notify::Event::new(kind).add_path(dir.join("config.ron"));
        assert_eq!(config_after(&event, &dir, &last), None, "{kind:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_watcher_step_echoes_a_completions_edit_with_the_config_unchanged() {
    // `completions.toml` rides the config echo: its edit delivers
    // the (unchanged) config so the echo's reload pass picks the
    // new table up.
    let dir = store_fixture("completions");
    let last = DropTermConfig::default();
    assert_eq!(
        config_after(&event_naming(&dir, "completions.toml"), &dir, &last),
        Some(last)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A real `notify` watcher over `dir`, its events on a std channel:
/// the subscription's machinery without the iced stream around it.
fn watch_store(
    dir: &std::path::Path,
) -> (
    notify::RecommendedWatcher,
    std::sync::mpsc::Receiver<notify::Event>,
) {
    use notify::Watcher as _;

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            let _ = tx.send(event);
        }
    })
    .expect("start a watcher");
    watcher
        .watch(dir, notify::RecursiveMode::NonRecursive)
        .expect("watch the store dir");
    (watcher, rx)
}

/// Feeds real watcher events through the step for up to `window`,
/// tracking `last` as the subscription does, and returns every
/// config it delivered — stopping early once `done` accepts one.
fn echoes(
    events: &std::sync::mpsc::Receiver<notify::Event>,
    dir: &std::path::Path,
    last: &mut DropTermConfig,
    window: std::time::Duration,
    done: impl Fn(&DropTermConfig) -> bool,
) -> Vec<DropTermConfig> {
    let deadline = std::time::Instant::now() + window;
    let mut delivered = Vec::new();
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        let Ok(event) = events.recv_timeout(left) else {
            break;
        };
        if let Some(next) = config_after(&event, dir, last) {
            *last = next.clone();
            let finished = done(&next);
            delivered.push(next);
            if finished {
                break;
            }
        }
    }
    delivered
}

#[test]
fn an_external_edit_applies_live_through_the_watcher() {
    // A hand edit while the app runs lands as a config echo. The
    // edit is staged the way a non-atomic save really unfolds — the
    // watcher reports the truncate before the data — with the
    // half-written state held on disk so the test never depends on
    // winning that race.
    let dir = store_fixture("external-edit");
    let section = dir.join("config.ron");
    let (_watcher, events) = watch_store(&dir);
    let mut last = DropTermConfig::default();

    // Mid-write: a section that does not parse yet. Nothing may
    // echo, and the file must stay the editor's — never moved aside.
    std::fs::write(&section, "(height_perc").expect("the edit's first half");
    let mid_write = echoes(
        &events,
        &dir,
        &mut last,
        std::time::Duration::from_millis(300),
        |_| false,
    );
    assert!(
        mid_write.is_empty(),
        "a half-written section echoed: {mid_write:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&section).expect("the editor's file stays"),
        "(height_perc"
    );
    assert!(
        !dir.join("config.ron.bak").exists(),
        "the edit was quarantined mid-write"
    );

    // The write completes: now it echoes.
    std::fs::write(&section, "(height_percent: 55)").expect("the edit's second half");
    let delivered = echoes(
        &events,
        &dir,
        &mut last,
        std::time::Duration::from_secs(5),
        |config| config.height_percent == 55,
    );
    assert_eq!(
        delivered.last().map(|config| config.height_percent),
        Some(55),
        "the edit never echoed"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_app_s_own_write_echoes_exactly_once() {
    // A drawer write is an atomic save — a `.tmp` sibling renamed
    // onto the section file — and the watcher reports that rename
    // more than once. Exactly one echo may come of it: the echo is
    // the apply.
    let dir = store_fixture("own-write");
    let (_watcher, events) = watch_store(&dir);
    let mut last = DropTermConfig::default();

    settings::PlannedWrite::FontSize(19)
        .persist(&dir, &last)
        .expect("the app's own write");

    let first = echoes(
        &events,
        &dir,
        &mut last,
        std::time::Duration::from_secs(5),
        |config| config.font_size == 19,
    );
    assert_eq!(first.len(), 1, "the write's echo: {first:?}");
    // The settle window: every remaining event of the same save.
    let again = echoes(
        &events,
        &dir,
        &mut last,
        std::time::Duration::from_millis(300),
        |_| false,
    );
    assert!(again.is_empty(), "the write echoed again: {again:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
