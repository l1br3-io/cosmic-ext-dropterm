use super::*;
use std::collections::BTreeMap;

fn section_keys() -> Vec<ConfigKey> {
    Section::ALL
        .iter()
        .flat_map(|section| section.keys().iter().copied())
        .collect()
}

// --- the drawer pane's width (drawer-resize T1) ---

#[test]
fn drawer_width_defaults_to_560_where_the_cap_allows() {
    // Spec drawer-resize criterion 1: never dragged (`None`)
    // renders the wider default — 560px on any surface wide
    // enough that half of it reaches 560 (surfaces ≥ 1120px).
    assert_eq!(drawer_width(None, 2000.0), 560.0);
    assert_eq!(drawer_width(None, 1120.0), 560.0);
}

#[test]
fn drawer_width_caps_the_default_at_half_the_surface() {
    // Between 688px and 1120px the 50% cap wins over the
    // default: the drawer never covers more than half the
    // drop-down surface.
    assert_eq!(drawer_width(None, 900.0), 450.0);
    assert_eq!(drawer_width(None, 728.0), 364.0);
    assert_eq!(drawer_width(None, 688.0), 344.0);
}

#[test]
fn drawer_width_floors_at_344_on_narrow_surfaces() {
    // Below 688px (2 × the floor) the cap falls under the
    // floor; the floor wins and the overlay clamps the pane to
    // the surface — the settings-drawer OQ-3 behavior carries
    // over unchanged.
    assert_eq!(drawer_width(None, 600.0), 344.0);
    assert_eq!(drawer_width(None, 0.0), 344.0);
}

#[test]
fn drawer_width_clamps_a_stored_preference_into_the_band() {
    // Criterion 4: a stale or hand-edited stored width can
    // never overflow the surface or crush the settings content
    // — render clamps whatever it finds, both directions.
    // 65_535 is `u16::MAX`, the largest a stored value can be.
    assert_eq!(drawer_width(Some(100), 2000.0), 344.0);
    assert_eq!(drawer_width(Some(65_535), 2000.0), 1000.0);
    assert_eq!(drawer_width(Some(65_535), 728.0), 364.0);
    assert_eq!(drawer_width(Some(400), 2000.0), 400.0);
}

#[test]
fn drawer_width_writes_floor_at_the_minimum_only() {
    // The write path's half of the width policy: the floor is
    // static, so `clamped` enforces it; the cap needs the
    // surface width this pure seam never sees, so render owns
    // it (see `drawer_width`). `None` — the double-click reset
    // — passes through untouched.
    assert_eq!(
        PlannedWrite::SettingsDrawerWidth(Some(100)).clamped(),
        PlannedWrite::SettingsDrawerWidth(Some(DRAWER_MIN_WIDTH))
    );
    assert_eq!(
        PlannedWrite::SettingsDrawerWidth(Some(600)).clamped(),
        PlannedWrite::SettingsDrawerWidth(Some(600))
    );
    assert_eq!(
        PlannedWrite::SettingsDrawerWidth(None).clamped(),
        PlannedWrite::SettingsDrawerWidth(None)
    );
}

#[test]
fn drawer_drag_targets_the_pointer_absolutely() {
    // Criterion 2: the pane is pinned to the surface's right
    // edge, so the drag's width is the pointer's window-absolute
    // distance from that edge — rounded to whole px, clamped
    // through the same policy as render, so a live drag can
    // never leave the band either.
    assert_eq!(drawer_drag_px(1440.0, 2000.0), 560);
    // Sub-px motion rounds, it doesn't tear.
    assert_eq!(drawer_drag_px(1440.4, 2000.0), 560);
    assert_eq!(drawer_drag_px(1439.6, 2000.0), 560);
    // Beyond the cap (width 1100 > 1000) and below the floor
    // (width 100) clamp exactly like render.
    assert_eq!(drawer_drag_px(900.0, 2000.0), 1000);
    assert_eq!(drawer_drag_px(1900.0, 2000.0), 344);
    // A fling past the surface's right edge saturates low and
    // floors — never negative, never a panic.
    assert_eq!(drawer_drag_px(2100.0, 2000.0), 344);
}

#[test]
fn drawer_release_commits_only_real_drags() {
    // Criterion 3: one write per drag release — and no write at
    // all without one. A plain click on the handle arms no live
    // value; a drag that lands back on the stored px rewrites
    // nothing (no file churn); only a real change persists.
    assert_eq!(drawer_release_write(None, None), None);
    assert_eq!(drawer_release_write(None, Some(480)), None);
    assert_eq!(
        drawer_release_write(Some(600), Some(600)),
        None,
        "dragged back to the stored width — no write"
    );
    assert_eq!(
        drawer_release_write(Some(600), None),
        Some(PlannedWrite::SettingsDrawerWidth(Some(600)))
    );
    assert_eq!(
        drawer_release_write(Some(600), Some(500)),
        Some(PlannedWrite::SettingsDrawerWidth(Some(600)))
    );
}

// --- the drawer's migrated content (settings-drawer Task 1) ---

#[test]
fn every_section_page_builds_with_drafts_and_from_config() {
    // The section-hosted pages (Appearance, Behavior, Advanced)
    // over the six-card map. Construction is the assertion: the
    // item builder's `expect` totality panics if a key resolves
    // to no control, and the widget builders must tolerate the
    // states the file can be in — buffered drafts winning over
    // file values (including a scrollback draft that does not
    // parse — the draft simply renders; rejection is the update
    // path's job) — and the plain default config. Exactly-once
    // across the pages is tab_sections' partition, pinned below.
    let config = DropTermConfig::default();
    let mut buffer = SettingsBuffer::default();
    buffer.set_text(TextField::FontName, "Draft Font".to_string());
    buffer.set_text(TextField::Shell, "/bin/tcsh".to_string());
    buffer.set_text(TextField::Args, "-l -i".to_string());
    buffer.set_text(TextField::ScrollbackLines, "garbage".to_string());
    buffer.set_text(TextField::DefaultCwd, "/srv/work".to_string());
    buffer.set_slider(ConfigKey::Opacity, 42.0);
    let _ = appearance_view(&config, &buffer, None);
    let _ = behavior_view(&config, &buffer);
    let _ = advanced_view(&config, &buffer);
    let _ = appearance_view(&config, &SettingsBuffer::default(), None);
    let _ = behavior_view(&config, &SettingsBuffer::default());
    let _ = advanced_view(&config, &SettingsBuffer::default());
}

#[test]
fn every_section_item_builds_a_control_for_every_key() {
    // Criterion 3's construction side: every key of every section
    // resolves to exactly one item control — the builder's
    // `expect` totality panics otherwise — with every text field
    // holding a draft (the buffer's display-wins rule) and a
    // scrollback draft that does not parse (the draft simply
    // renders; rejection is the update path's job).
    let config = DropTermConfig::default();
    let mut buffer = SettingsBuffer::default();
    buffer.set_text(TextField::FontName, "Draft Font".to_string());
    buffer.set_text(TextField::Shell, "/bin/tcsh".to_string());
    buffer.set_text(TextField::Args, "-l -i".to_string());
    buffer.set_text(TextField::ScrollbackLines, "garbage".to_string());
    for section in Section::ALL {
        for key in section.keys() {
            let _ = drawer_item(*key, &config, &buffer, None);
        }
    }
}

// --- the tab regrouping (settings-tabs ST-1) ---

#[test]
fn the_tabs_partition_every_section_exactly_once() {
    // The tabs are a view-only regrouping (criterion 2): every
    // Section appears in exactly one tab, the union is
    // Section::ALL, and each tab's display order is pinned so
    // the page composition cannot silently reshuffle. The
    // Profiles and Keybindings tabs host no Section card — their
    // pages are the manager and the editor.
    assert_eq!(
        tab_sections(SettingsTab::Appearance),
        &[Section::Appearance, Section::Window]
    );
    assert_eq!(
        tab_sections(SettingsTab::Behavior),
        &[
            Section::Tabs,
            Section::StatusBar,
            Section::Dropdown,
            Section::Terminal,
        ]
    );
    assert_eq!(tab_sections(SettingsTab::Profiles), &[]);
    assert_eq!(tab_sections(SettingsTab::Keybindings), &[]);
    assert_eq!(tab_sections(SettingsTab::Advanced), &[Section::Advanced]);
    let across_tabs: Vec<Section> = SettingsTab::ALL
        .iter()
        .flat_map(|tab| tab_sections(*tab).iter().copied())
        .collect();
    assert_eq!(across_tabs.len(), Section::ALL.len());
    for section in Section::ALL {
        assert_eq!(
            across_tabs
                .iter()
                .filter(|listed| **listed == section)
                .count(),
            1,
            "{section:?} must appear in exactly one tab"
        );
    }
}

#[test]
fn tab_headers_are_the_strip_labels() {
    // Hardcoded English like every label in this drawer — no
    // fluent/i18n in this repo.
    assert_eq!(tab_header(SettingsTab::Appearance), "Appearance");
    assert_eq!(tab_header(SettingsTab::Behavior), "Behavior");
    assert_eq!(tab_header(SettingsTab::Profiles), "Profiles");
    assert_eq!(tab_header(SettingsTab::Keybindings), "Keybindings");
    assert_eq!(tab_header(SettingsTab::Advanced), "Advanced");
}

// --- the scheme pickers (scheme-picker T2) ---

#[test]
fn the_scheme_picker_control_builds_in_every_config_state() {
    // Construction is the assertion for both halves: the closed
    // control (name + mini chip strip) and the popover host it
    // becomes while open — both keys, open and closed, over a
    // default, set, and unknown stored scheme name.
    let mut config = DropTermConfig::default();
    for open in [false, true] {
        let _ = scheme_picker_control(ConfigKey::ColorSchemeDark, &config, open);
        let _ = scheme_picker_control(ConfigKey::ColorSchemeLight, &config, open);
    }
    config.color_scheme_dark = "Dracula".to_string();
    config.color_scheme_light = "Not A Scheme".to_string();
    for open in [false, true] {
        let _ = scheme_picker_control(ConfigKey::ColorSchemeDark, &config, open);
        let _ = scheme_picker_control(ConfigKey::ColorSchemeLight, &config, open);
    }
}

#[test]
fn the_appearance_page_builds_with_a_picker_open_and_closed() {
    // The open flag is view state threaded through the page
    // builder; the page must construct in both states.
    let config = DropTermConfig::default();
    let buffer = SettingsBuffer::default();
    let _ = appearance_view(&config, &buffer, Some(ConfigKey::ColorSchemeDark));
    let _ = appearance_view(&config, &buffer, None);
}

#[test]
fn the_keybindings_tab_builds_with_and_without_matches() {
    // The third tab's view (keybinds-editor criterion 1):
    // construction smoke — every group renders with the empty
    // query, a query nothing matches still builds (the
    // empty-result state), and the armed-capture and conflict
    // states build too. Row inventory and filtering are pinned in
    // `keybinds` (the groups and search tests).
    let _unfiltered = keybindings_view(&BTreeMap::new(), "", None, None);
    let _no_matches = keybindings_view(&BTreeMap::new(), "xyzzy-no-action-has-this", None, None);
    let custom = BTreeMap::from([(
        "copy".to_string(),
        crate::keybinds::Binding {
            modifiers: vec![crate::keybinds::ModifierName::Alt],
            key: "C".to_string(),
        },
    )]);
    let _filtered = keybindings_view(&custom, "copy", None, None);
    let _armed = keybindings_view(&custom, "", Some(crate::terminal::Action::Copy), None);
    let _conflict = keybindings_view(
        &custom,
        "",
        None,
        Some((
            crate::terminal::Action::Copy,
            crate::terminal::Action::Paste,
        )),
    );
}

// --- the tab strip's model (settings-tabs ST-3) ---

#[test]
fn the_strip_model_carries_one_row_per_tab_in_order() {
    // Criterion 7's entity↔tab lockstep, construction side:
    // exactly the five tabs of `SettingsTab::ALL` in strip
    // order, each row labeled with its `tab_header` and carrying
    // its `SettingsTab` as data — the payload the activation
    // handler resolves.
    let mut model = tab_strip_model();
    assert_eq!(model.iter().count(), SettingsTab::ALL.len());
    for (position, tab) in SettingsTab::ALL.iter().enumerate() {
        let entity = model
            .entity_at(u16::try_from(position).expect("five tabs, position 0-4"))
            .expect("one row per tab, in ALL order");
        assert_eq!(model.data::<SettingsTab>(entity), Some(tab));
        assert_eq!(model.text(entity), Some(tab_header(*tab)));
    }
}

#[test]
fn activating_a_tab_row_selects_it_and_only_it() {
    // Criterion 7's lockstep, selection side: `activate_tab_row`
    // moves the single selection to the row carrying the tab —
    // the sync every page mutation applies after assigning
    // `settings_page` (pick, explicit close).
    let mut model = tab_strip_model();
    // The strip's selection only ever moves through
    // `activate_tab_row`: a fresh model selects nothing.
    assert_eq!(model.active_data::<SettingsTab>(), None);
    activate_tab_row(&mut model, SettingsTab::Advanced);
    assert_eq!(
        model.active_data::<SettingsTab>(),
        Some(&SettingsTab::Advanced)
    );
    activate_tab_row(&mut model, SettingsTab::Appearance);
    assert_eq!(
        model.active_data::<SettingsTab>(),
        Some(&SettingsTab::Appearance)
    );
}

// --- the drawer's lifecycle transitions (settings-drawer T3) ---

#[test]
fn an_explicit_drawer_close_clears_the_buffer_for_a_fresh_reopen() {
    // Criterion 8's close rule: Escape, the repeat toggle, and
    // the Close button are explicit closes — the next open
    // renders purely from config, the reopen-fresh rule the
    // settings window carried.
    let config = DropTermConfig::default();
    let mut buffer = SettingsBuffer::default();
    buffer.set_text(TextField::Shell, "/bin/tcsh".to_string());
    buffer.set_slider(ConfigKey::Opacity, 42.0);
    buffer.apply_drawer_transition(DrawerTransition::ExplicitClose);
    assert_eq!(
        buffer.text(TextField::Shell, &config),
        config.shell.clone().unwrap_or_default()
    );
    assert_eq!(
        buffer.slider_value(ConfigKey::Opacity, &config),
        f32::from(config.opacity)
    );
}

#[test]
fn a_focus_loss_hide_preserves_drafts_for_the_next_show() {
    // Criterion 8's hide rule: the surface hiding — focus loss
    // or an explicit hide, whatever re-shows later — never
    // touches the drawer's state; the next show renders the
    // drawer exactly as it was, drafts intact.
    let config = DropTermConfig::default();
    let mut buffer = SettingsBuffer::default();
    buffer.set_text(TextField::Shell, "/bin/tcsh".to_string());
    buffer.set_slider(ConfigKey::Opacity, 42.0);
    buffer.apply_drawer_transition(DrawerTransition::FocusLossHide);
    assert_eq!(buffer.text(TextField::Shell, &config), "/bin/tcsh");
    assert_eq!(buffer.slider_value(ConfigKey::Opacity, &config), 42.0);
}

#[test]
fn the_control_map_covers_every_exposed_key_exactly_once_and_never_output() {
    // One planned-write variant per config key — the writer side of
    // the control→key map — each landing in exactly one section.
    let writers = [
        PlannedWrite::HeightPercent(40),
        PlannedWrite::WidthPercent(100),
        PlannedWrite::HAlign(HAlign::Center),
        PlannedWrite::Edge(Edge::Top),
        PlannedWrite::TabPosition(TabPosition::Auto),
        PlannedWrite::FontName(String::new()),
        PlannedWrite::FontSize(14),
        PlannedWrite::Opacity(100),
        PlannedWrite::HideOnFocusLoss(true),
        PlannedWrite::SessionPersistenceEnabled(true),
        PlannedWrite::SlideAnimationEnabled(true),
        PlannedWrite::BellNotificationEnabled(true),
        PlannedWrite::TabTitleMode(TabTitleMode::Shell),
        PlannedWrite::NewTabAfterCurrent(true),
        PlannedWrite::NewTabCwd(NewTabCwd::Inherit),
        PlannedWrite::CloseConfirmationEnabled(true),
        PlannedWrite::DefaultCwd(None),
        PlannedWrite::Shell(String::new()),
        PlannedWrite::Args(Vec::new()),
        PlannedWrite::ScrollbackLines(10_000),
        PlannedWrite::Layer(Layer::Overlay),
        PlannedWrite::KeyboardMode(KeyboardMode::Exclusive),
        PlannedWrite::RespectPanel(true),
        PlannedWrite::ColorSchemeDark(String::new()),
        PlannedWrite::ColorSchemeLight(String::new()),
        PlannedWrite::ThemePreference(ThemePreference::System),
        PlannedWrite::StatusBarEnabled(true),
        PlannedWrite::StatusBarShowProcess(true),
        PlannedWrite::StatusBarShowPath(true),
        PlannedWrite::StatusBarShowBranch(true),
        PlannedWrite::StatusBarShowProfile(true),
        PlannedWrite::StatusBarShowGrid(true),
        PlannedWrite::StatusBarShowTab(true),
        PlannedWrite::StatusBarShowZoom(true),
        PlannedWrite::StatusBarShowClock(true),
    ];
    let keys = section_keys();
    assert_eq!(keys.len(), writers.len());
    for writer in &writers {
        let occurrences = keys.iter().filter(|key| **key == writer.key()).count();
        assert_eq!(
            occurrences,
            1,
            "{} missing from or duplicated in the section map",
            writer.key().key_name()
        );
    }
    // `output` is not writable from the UI.
    assert!(keys.iter().all(|key| key.key_name() != "output"));
}

#[test]
fn key_names_match_the_config_file_fields() {
    // The file-format contract: each key's name is the spelling
    // its `DropTermConfig` field serializes as, so a
    // `handler.set(key_name(), …)` merges with a hand-edited file.
    let expected = [
        (ConfigKey::HeightPercent, "height_percent"),
        (ConfigKey::WidthPercent, "width_percent"),
        (ConfigKey::HAlign, "h_align"),
        (ConfigKey::Edge, "edge"),
        (ConfigKey::TabPosition, "tab_position"),
        (ConfigKey::FontName, "font_name"),
        (ConfigKey::FontSize, "font_size"),
        (ConfigKey::Opacity, "opacity"),
        (ConfigKey::HideOnFocusLoss, "hide_on_focus_loss"),
        (
            ConfigKey::SessionPersistenceEnabled,
            "session_persistence_enabled",
        ),
        (ConfigKey::SlideAnimationEnabled, "slide_animation_enabled"),
        (
            ConfigKey::BellNotificationEnabled,
            "bell_notification_enabled",
        ),
        (ConfigKey::NewTabAfterCurrent, "new_tab_after_current"),
        (ConfigKey::NewTabCwd, "new_tab_cwd"),
        (ConfigKey::DefaultCwd, "default_cwd"),
        (
            ConfigKey::CloseConfirmationEnabled,
            "close_confirmation_enabled",
        ),
        (
            ConfigKey::SettingsDrawerClickClose,
            "settings_drawer_click_close",
        ),
        (ConfigKey::Shell, "shell"),
        (ConfigKey::Args, "args"),
        (ConfigKey::ScrollbackLines, "scrollback_lines"),
        (ConfigKey::Layer, "layer"),
        (ConfigKey::KeyboardMode, "keyboard_mode"),
        (ConfigKey::RespectPanel, "respect_panel"),
        (ConfigKey::ColorSchemeDark, "color_scheme_dark"),
        (ConfigKey::ColorSchemeLight, "color_scheme_light"),
        (ConfigKey::ThemePreference, "theme_preference"),
        (ConfigKey::SettingsDrawerWidth, "settings_drawer_width"),
    ];
    for (key, name) in expected {
        assert_eq!(key.key_name(), name);
    }
}

#[test]
fn height_writes_clamp_into_the_band() {
    assert_eq!(clamp_height_percent(0), MIN_HEIGHT_PERCENT);
    assert_eq!(clamp_height_percent(9), MIN_HEIGHT_PERCENT);
    assert_eq!(clamp_height_percent(10), MIN_HEIGHT_PERCENT);
    assert_eq!(clamp_height_percent(60), 60);
    assert_eq!(clamp_height_percent(100), MAX_HEIGHT_PERCENT);
    assert_eq!(clamp_height_percent(255), MAX_HEIGHT_PERCENT);
}

#[test]
fn width_writes_clamp_into_the_band() {
    assert_eq!(clamp_width_percent(0), MIN_WIDTH_PERCENT);
    assert_eq!(clamp_width_percent(29), MIN_WIDTH_PERCENT);
    assert_eq!(clamp_width_percent(30), MIN_WIDTH_PERCENT);
    assert_eq!(clamp_width_percent(60), 60);
    assert_eq!(clamp_width_percent(100), MAX_WIDTH_PERCENT);
    assert_eq!(clamp_width_percent(255), MAX_WIDTH_PERCENT);
}

#[test]
fn opacity_writes_clamp_to_at_most_100() {
    // Zero is valid (fully transparent), so only the upper bound
    // clamps — mirroring `DropTermConfig::clamped`.
    assert_eq!(clamp_opacity(0), 0);
    assert_eq!(clamp_opacity(50), 50);
    assert_eq!(clamp_opacity(100), 100);
    assert_eq!(clamp_opacity(101), 100);
    assert_eq!(clamp_opacity(255), 100);
}

#[test]
fn planned_writes_clamp_the_ranged_keys_and_pass_others_through() {
    assert_eq!(
        PlannedWrite::HeightPercent(255).clamped(),
        PlannedWrite::HeightPercent(MAX_HEIGHT_PERCENT)
    );
    assert_eq!(
        PlannedWrite::WidthPercent(0).clamped(),
        PlannedWrite::WidthPercent(MIN_WIDTH_PERCENT)
    );
    assert_eq!(
        PlannedWrite::Opacity(255).clamped(),
        PlannedWrite::Opacity(100)
    );
    // Keys without a documented range pass through byte-identical.
    let unchanged = PlannedWrite::FontSize(999);
    assert_eq!(unchanged.clamped(), unchanged);
    let enum_key = PlannedWrite::TabPosition(TabPosition::Bottom);
    assert_eq!(enum_key.clamped(), enum_key);
}

#[test]
fn a_bare_height_write_cancels_a_pending_drag_write_back() {
    assert!(bare_write_cancels_pending_drag(
        ConfigKey::HeightPercent,
        true
    ));
}

#[test]
fn a_bare_write_without_a_pending_drag_write_cancels_nothing() {
    assert!(!bare_write_cancels_pending_drag(
        ConfigKey::HeightPercent,
        false
    ));
}

#[test]
fn a_bare_write_of_a_non_geometry_key_never_touches_pending_drag_state() {
    for key in [
        ConfigKey::FontName,
        ConfigKey::Opacity,
        ConfigKey::SlideAnimationEnabled,
    ] {
        assert!(
            !bare_write_cancels_pending_drag(key, true),
            "{} must not cancel a pending height write",
            key.key_name()
        );
    }
}

#[test]
fn a_bare_width_write_leaves_a_pending_height_settle_alone() {
    // Width is a geometry key but not *the drag's* key: the race
    // rule cancels per key ("for the same key"), so moving the
    // width slider while a height settle is debouncing does not
    // drop it — the settle still lands.
    assert!(!bare_write_cancels_pending_drag(
        ConfigKey::WidthPercent,
        true
    ));
}

// --- parsing, timing copy, and buffer semantics ---

#[test]
fn empty_args_text_parses_to_no_arguments() {
    assert!(parse_args("").is_empty());
    assert!(parse_args("   ").is_empty());
    assert!(parse_args("\t \n").is_empty());
}

#[test]
fn args_text_splits_on_whitespace_runs() {
    assert_eq!(
        parse_args("  --login     -i\tverbose  "),
        vec![
            "--login".to_string(),
            "-i".to_string(),
            "verbose".to_string()
        ]
    );
}

#[test]
fn args_text_keeps_embedded_tokens_verbatim() {
    // No quoting (the documented limitation): every token lands
    // exactly as typed, spaces being the only separator.
    assert_eq!(
        parse_args("--command=echo hi"),
        vec!["--command=echo".to_string(), "hi".to_string()]
    );
}

#[test]
fn scrollback_text_parses_plain_numbers() {
    assert_eq!(parse_scrollback("10000"), Some(10_000));
    assert_eq!(parse_scrollback("  250 \n"), Some(250));
    assert_eq!(parse_scrollback("0"), Some(0));
}

#[test]
fn scrollback_text_clamps_negatives_to_zero() {
    // Scrollback is >= 0 — a negative entry never persists,
    // clamping instead of rejecting so the intent ("as small as
    // possible") still lands.
    assert_eq!(parse_scrollback("-1"), Some(0));
    assert_eq!(parse_scrollback("-5000"), Some(0));
}

#[test]
fn scrollback_text_rejects_garbage_and_emptiness() {
    // Nothing numeric to commit: no write at all (the draft stays
    // for the user to fix).
    assert_eq!(parse_scrollback(""), None);
    assert_eq!(parse_scrollback("   "), None);
    assert_eq!(parse_scrollback("10k"), None);
    assert_eq!(parse_scrollback("1e4"), None);
}

#[test]
fn scrollback_text_clamps_above_u32_range() {
    assert_eq!(parse_scrollback("99999999999999999999"), Some(u32::MAX));
}

#[test]
fn submitted_text_fields_commit_trimmed_values() {
    assert_eq!(
        submit_write(TextField::FontName, " Fira Code "),
        Some(PlannedWrite::FontName("Fira Code".to_string()))
    );
    assert_eq!(
        submit_write(TextField::Shell, " /usr/bin/fish "),
        Some(PlannedWrite::Shell("/usr/bin/fish".to_string()))
    );
}

#[test]
fn submitted_shell_text_commits_empty_as_unset() {
    // An emptied shell field writes "": `resolved_shell` filters
    // it back to the $SHELL fallback — same effect as a
    // hand-edited empty value in the file.
    assert_eq!(
        submit_write(TextField::Shell, "   "),
        Some(PlannedWrite::Shell(String::new()))
    );
}

#[test]
fn submitted_args_text_parses_through_the_args_rules() {
    assert_eq!(
        submit_write(TextField::Args, " --login  --noprofile "),
        Some(PlannedWrite::Args(vec![
            "--login".to_string(),
            "--noprofile".to_string()
        ]))
    );
    assert_eq!(
        submit_write(TextField::Args, ""),
        Some(PlannedWrite::Args(Vec::new()))
    );
}

#[test]
fn submitted_scrollback_follows_the_scrollback_rules() {
    assert_eq!(
        submit_write(TextField::ScrollbackLines, "5000"),
        Some(PlannedWrite::ScrollbackLines(5000))
    );
    assert_eq!(
        submit_write(TextField::ScrollbackLines, "-3"),
        Some(PlannedWrite::ScrollbackLines(0))
    );
    assert_eq!(submit_write(TextField::ScrollbackLines, "lots"), None);
}

#[test]
fn apply_timing_states_each_field_s_documented_semantics() {
    // The apply-timing copy, per key: geometry/font/colors/opacity
    // live; layer/keyboard_mode/respect_panel at next show;
    // shell/args/scrollback on new tabs; the toggles on their next
    // trigger.
    for key in [
        ConfigKey::HeightPercent,
        ConfigKey::WidthPercent,
        ConfigKey::HAlign,
        ConfigKey::Edge,
        ConfigKey::TabPosition,
        ConfigKey::FontName,
        ConfigKey::FontSize,
        ConfigKey::Opacity,
        ConfigKey::ColorSchemeDark,
        ConfigKey::ColorSchemeLight,
        ConfigKey::ThemePreference,
        ConfigKey::StatusBarEnabled,
        ConfigKey::StatusBarShowProcess,
        ConfigKey::StatusBarShowPath,
        ConfigKey::StatusBarShowBranch,
        ConfigKey::StatusBarShowProfile,
        ConfigKey::StatusBarShowGrid,
        ConfigKey::StatusBarShowTab,
        ConfigKey::StatusBarShowZoom,
        ConfigKey::StatusBarShowClock,
    ] {
        assert_eq!(apply_timing(key), ApplyTiming::Live, "{key:?}");
    }
    for key in [
        ConfigKey::Layer,
        ConfigKey::KeyboardMode,
        ConfigKey::RespectPanel,
    ] {
        assert_eq!(apply_timing(key), ApplyTiming::NextShow, "{key:?}");
    }
    for key in [
        ConfigKey::Shell,
        ConfigKey::Args,
        ConfigKey::ScrollbackLines,
        ConfigKey::NewTabCwd,
        ConfigKey::DefaultCwd,
    ] {
        assert_eq!(apply_timing(key), ApplyTiming::NewTab, "{key:?}");
    }
    for key in [
        ConfigKey::HideOnFocusLoss,
        ConfigKey::SessionPersistenceEnabled,
        ConfigKey::SlideAnimationEnabled,
        ConfigKey::NewTabAfterCurrent,
        ConfigKey::CloseConfirmationEnabled,
    ] {
        assert_eq!(apply_timing(key), ApplyTiming::NextTrigger, "{key:?}");
    }
}

#[test]
fn apply_timing_copy_names_every_timing() {
    // The UI copy itself: each timing has its human-readable
    // label, and they are all distinct.
    let labels = [
        ApplyTiming::Live.label(),
        ApplyTiming::NewTab.label(),
        ApplyTiming::NextShow.label(),
        ApplyTiming::NextTrigger.label(),
    ];
    for pair in labels.windows(2) {
        assert_ne!(pair[0], pair[1]);
    }
    assert_eq!(ApplyTiming::NewTab.label(), "new tabs only");
    assert_eq!(ApplyTiming::NextShow.label(), "next show");
    assert_eq!(ApplyTiming::NextTrigger.label(), "next trigger");
}

#[test]
fn dropdown_selections_round_trip_through_their_options() {
    let config = DropTermConfig::default();
    for key in [
        ConfigKey::HAlign,
        ConfigKey::Edge,
        ConfigKey::TabPosition,
        ConfigKey::Layer,
        ConfigKey::KeyboardMode,
        ConfigKey::ColorSchemeDark,
        ConfigKey::ColorSchemeLight,
        ConfigKey::ThemePreference,
        ConfigKey::NewTabCwd,
    ] {
        let selected =
            dropdown_selected(key, &config).unwrap_or_else(|| panic!("{key:?} has no selection"));
        let write =
            dropdown_write(key, selected).unwrap_or_else(|| panic!("{key:?} pick vanished"));
        // Re-selecting the current index reproduces the current
        // config value — the dropdown can never invent one.
        let back = dropdown_selected(key, &config).unwrap();
        assert_eq!(
            dropdown_write(key, back).unwrap(),
            write,
            "{key:?} round trip"
        );
    }
}

#[test]
fn dropdown_picks_out_of_range_reject_instead_of_panicking() {
    for key in [
        ConfigKey::HAlign,
        ConfigKey::Edge,
        ConfigKey::TabPosition,
        ConfigKey::Layer,
        ConfigKey::KeyboardMode,
        ConfigKey::ColorSchemeDark,
        ConfigKey::ColorSchemeLight,
        ConfigKey::ThemePreference,
    ] {
        assert!(dropdown_write(key, 99).is_none(), "{key:?}");
    }
}

#[test]
fn dropdown_writes_reject_keys_without_dropdowns() {
    // Shell's dropdown is dynamic — the `/etc/shells` picker with
    // its own planner — so it is no longer among the static-table
    // rejections; the other text and slider keys stay off the
    // dropdown paths entirely.
    assert!(dropdown_write(ConfigKey::FontName, 0).is_none());
    assert!(dropdown_write(ConfigKey::Opacity, 0).is_none());
    assert!(dropdown_selected(ConfigKey::FontName, &DropTermConfig::default()).is_none());
}

// --- the shell picker (default-spawn DS-2) ---

#[test]
fn etc_shells_parses_into_unique_existing_paths_in_file_order() {
    // Ruling 1's pipeline: comments and blanks skip, duplicates
    // collapse by basename (first occurrence wins — `/bin/sh`
    // keeps its file-order priority over `/usr/bin/sh`), and
    // entries that do not exist on this system drop quietly.
    // `/bin/sh`, `/bin/bash`, and `/usr/bin/env` are guaranteed on
    // this target; `/usr/bin/bash` collapses into `/bin/bash`.
    let content = "\
# /etc/shells: valid login shells
/bin/sh
/bin/bash
/usr/bin/bash
/nonsense/dropterm-stale-shell

/usr/bin/env
";
    assert_eq!(
        shell_picker_options(Some(content)),
        vec![
            "/bin/sh".to_string(),
            "/bin/bash".to_string(),
            "/usr/bin/env".to_string(),
        ]
    );
}

#[test]
fn unreadable_or_barren_etc_shells_falls_back_to_the_static_quartet() {
    // `None` = unreadable file; readable-but-empty content (and
    // nothing surviving the parse) degrades the same way — never
    // an empty dropdown.
    let fallback: Vec<String> = ["/bin/bash", "/bin/zsh", "/bin/fish", "/bin/sh"]
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    assert_eq!(shell_picker_options(None), fallback);
    assert_eq!(shell_picker_options(Some("")), fallback);
    assert_eq!(shell_picker_options(Some("# only a comment\n\n")), fallback);
}

#[test]
fn shell_dropdown_picks_write_full_paths_and_reject_stale_indices() {
    // A pick commits the option's full path under the existing
    // `shell` key — basename labels display, paths persist; an
    // index past the list (the file changed under the open
    // dropdown) rejects instead of panicking.
    let options = vec!["/bin/sh".to_string(), "/bin/bash".to_string()];
    assert_eq!(
        shell_dropdown_write(&options, 0),
        Some(PlannedWrite::Shell("/bin/sh".to_string()))
    );
    assert_eq!(
        shell_dropdown_write(&options, 1),
        Some(PlannedWrite::Shell("/bin/bash".to_string()))
    );
    assert_eq!(shell_dropdown_write(&options, 2), None);
    assert_eq!(shell_dropdown_write(&options, 99), None);
}

#[test]
fn shell_dropdown_selection_mirrors_the_stored_path_and_off_list_selects_none() {
    // The stored value selects its exact row; an unset key, a
    // custom path, or a stale value selects nothing — the
    // free-text row still shows it, and the control never invents
    // a selection.
    let options = vec!["/bin/sh".to_string(), "/bin/bash".to_string()];
    assert_eq!(
        shell_dropdown_selected(&options, Some("/bin/bash")),
        Some(1)
    );
    assert_eq!(shell_dropdown_selected(&options, Some("/bin/sh")), Some(0));
    assert_eq!(shell_dropdown_selected(&options, None), None);
    assert_eq!(shell_dropdown_selected(&options, Some("/opt/mysh")), None);
}

#[test]
fn shell_labels_are_bare_basenames() {
    // OQ-1's default ruling: basename labels, no suffixes — the
    // picker says `zsh`, never `zsh (current)`.
    assert_eq!(shell_label("/usr/bin/zsh"), "zsh");
    assert_eq!(shell_label("/bin/sh"), "sh");
    // A path with no final component falls back to itself rather
    // than emptying the row.
    assert_eq!(shell_label("/"), "/");
}

#[test]
fn the_default_profile_note_rides_the_three_plain_spawn_rows() {
    // Ruling 4: the picker, the mode dropdown, and the default
    // directory each say on their row that a set `default_profile`
    // still owns plain spawns outright.
    for key in [
        ConfigKey::Shell,
        ConfigKey::NewTabCwd,
        ConfigKey::DefaultCwd,
    ] {
        assert_eq!(
            drawer_description(key),
            "new tabs only — a default profile still owns plain spawns",
            "{key:?}"
        );
    }
    // Every other row keeps the plain timing label.
    assert_eq!(drawer_description(ConfigKey::Args), "new tabs only");
}

// --- registry-fed scheme dropdowns ---

#[test]
fn scheme_keys_sit_in_appearance_after_opacity_and_leave_advanced() {
    // The two scheme rows land in Appearance after Opacity;
    // Advanced keeps only the compositor-negotiation knobs.
    assert_eq!(
        Section::Appearance.keys(),
        &[
            ConfigKey::FontName,
            ConfigKey::FontSize,
            ConfigKey::Opacity,
            ConfigKey::ColorSchemeDark,
            ConfigKey::ColorSchemeLight,
            ConfigKey::ThemePreference,
        ]
    );
    assert_eq!(
        Section::Advanced.keys(),
        &[
            ConfigKey::RespectPanel,
            ConfigKey::Layer,
            ConfigKey::KeyboardMode,
        ]
    );
}

#[test]
fn the_terminal_card_joins_the_bell_after_scrollback() {
    // The Terminal card: the global shell defaults every profile
    // falls back to — shell, arguments, and the plain spawn's
    // default directory (default-spawn DS-1) — with the bell,
    // terminal behavior, last.
    assert_eq!(
        Section::Terminal.keys(),
        &[
            ConfigKey::Shell,
            ConfigKey::Args,
            ConfigKey::DefaultCwd,
            ConfigKey::ScrollbackLines,
            ConfigKey::BellNotificationEnabled,
        ]
    );
    assert_eq!(
        toggle_write(ConfigKey::BellNotificationEnabled, false),
        Some(PlannedWrite::BellNotificationEnabled(false))
    );
}

#[test]
fn the_dropdown_card_hosts_the_two_dropdown_mechanics() {
    // The Dropdown card: the drop-down's own mechanics — the
    // focus-loss hide and the slide — and nothing else.
    assert_eq!(
        Section::Dropdown.keys(),
        &[ConfigKey::HideOnFocusLoss, ConfigKey::SlideAnimationEnabled,]
    );
}

#[test]
fn the_status_bar_card_hosts_the_master_toggle_plus_eight_show_rows() {
    // status-segments Task 3: the Status bar card grows from one
    // row to nine, in band order — the master toggle, then one
    // "Show …" row per segment — each under its exact file
    // spelling, each flipping through the single planned-write
    // path, all Live.
    assert_eq!(
        SettingsTab::Behavior,
        SettingsTab::ALL[1], // the Behavior page exists where the strip puts it
    );
    assert_eq!(
        tab_sections(SettingsTab::Behavior)
            .iter()
            .position(|section| matches!(section, Section::StatusBar)),
        Some(1),
        "the Status bar card sits directly after Tabs on Behavior"
    );
    assert_eq!(
        Section::StatusBar.keys(),
        &[
            ConfigKey::StatusBarEnabled,
            ConfigKey::StatusBarShowProcess,
            ConfigKey::StatusBarShowPath,
            ConfigKey::StatusBarShowBranch,
            ConfigKey::StatusBarShowProfile,
            ConfigKey::StatusBarShowGrid,
            ConfigKey::StatusBarShowTab,
            ConfigKey::StatusBarShowZoom,
            ConfigKey::StatusBarShowClock,
        ]
    );
    assert_eq!(ConfigKey::StatusBarEnabled.key_name(), "status_bar_enabled");
    assert_eq!(
        ConfigKey::StatusBarShowProcess.key_name(),
        "status_bar_show_process"
    );
    assert_eq!(
        ConfigKey::StatusBarShowPath.key_name(),
        "status_bar_show_path"
    );
    assert_eq!(
        ConfigKey::StatusBarShowBranch.key_name(),
        "status_bar_show_branch"
    );
    assert_eq!(
        ConfigKey::StatusBarShowProfile.key_name(),
        "status_bar_show_profile"
    );
    assert_eq!(
        ConfigKey::StatusBarShowGrid.key_name(),
        "status_bar_show_grid"
    );
    assert_eq!(
        ConfigKey::StatusBarShowTab.key_name(),
        "status_bar_show_tab"
    );
    assert_eq!(
        ConfigKey::StatusBarShowZoom.key_name(),
        "status_bar_show_zoom"
    );
    assert_eq!(
        ConfigKey::StatusBarShowClock.key_name(),
        "status_bar_show_clock"
    );
    assert_eq!(
        toggle_write(ConfigKey::StatusBarShowBranch, false),
        Some(PlannedWrite::StatusBarShowBranch(false))
    );
    assert_eq!(
        toggle_write(ConfigKey::StatusBarShowBranch, true),
        Some(PlannedWrite::StatusBarShowBranch(true))
    );
}

#[test]
fn close_confirmation_joins_the_tabs_card() {
    // Close-confirm criterion 1's toggle home: the "Confirm
    // closing a tab with a running process" row sits in the Tabs
    // card (its full key list pinned by the Tabs-card test
    // above); its flip commits the config bool through the single
    // PlannedWrite path, under the field's exact file spelling.
    assert_eq!(
        ConfigKey::CloseConfirmationEnabled.label(),
        "Confirm closing a tab with a running process"
    );
    assert_eq!(
        ConfigKey::CloseConfirmationEnabled.key_name(),
        "close_confirmation_enabled"
    );
    assert_eq!(
        toggle_write(ConfigKey::CloseConfirmationEnabled, true),
        Some(PlannedWrite::CloseConfirmationEnabled(true))
    );
    assert_eq!(
        toggle_write(ConfigKey::CloseConfirmationEnabled, false),
        Some(PlannedWrite::CloseConfirmationEnabled(false))
    );
    assert_eq!(
        toggle_write(ConfigKey::CloseConfirmationEnabled, true)
            .expect("a toggle key always plans a write")
            .key(),
        ConfigKey::CloseConfirmationEnabled
    );
}

// --- the new-tab-directory dropdown (default-spawn DS-1) ---

#[test]
fn new_tab_cwd_lands_after_new_tab_after_current_in_the_tabs_card() {
    // The "New tab directory" row lands directly after "New tabs
    // after current tab" in the Tabs card (the full key list
    // pinned by the Tabs-card test), under the field's exact file
    // spelling.
    assert_eq!(ConfigKey::NewTabCwd.label(), "New tab directory");
    assert_eq!(ConfigKey::NewTabCwd.key_name(), "new_tab_cwd");
    assert_eq!(apply_timing(ConfigKey::NewTabCwd), ApplyTiming::NewTab);
}

#[test]
fn new_tab_cwd_options_are_exactly_inherit_default() {
    // Inherit first, so the default config's selection is index 0
    // (the Follow system pattern).
    assert_eq!(
        NEW_TAB_CWD_OPTIONS,
        &[
            (NewTabCwd::Inherit, "Inherit active tab"),
            (NewTabCwd::Default, "Default directory"),
        ]
    );
}

#[test]
fn new_tab_cwd_picks_round_trip_and_out_of_range_rejects() {
    // A pick commits exactly its variant — the control can never
    // invent a value; indices past the table (including exactly
    // len) reject instead of panicking.
    assert_eq!(
        dropdown_write(ConfigKey::NewTabCwd, 0),
        Some(PlannedWrite::NewTabCwd(NewTabCwd::Inherit))
    );
    assert_eq!(
        dropdown_write(ConfigKey::NewTabCwd, 1),
        Some(PlannedWrite::NewTabCwd(NewTabCwd::Default))
    );
    assert_eq!(dropdown_write(ConfigKey::NewTabCwd, 2), None);
    assert_eq!(dropdown_write(ConfigKey::NewTabCwd, 99), None);
    assert_eq!(
        PlannedWrite::NewTabCwd(NewTabCwd::Default).key(),
        ConfigKey::NewTabCwd
    );
}

#[test]
fn new_tab_cwd_selection_mirrors_the_config_value() {
    // Default config → Inherit → Some(0); the committed variant
    // re-selects its own position, so the dropdown opens on the
    // file's current value.
    let mut config = DropTermConfig::default();
    assert_eq!(dropdown_selected(ConfigKey::NewTabCwd, &config), Some(0));
    config.new_tab_cwd = NewTabCwd::Default;
    assert_eq!(dropdown_selected(ConfigKey::NewTabCwd, &config), Some(1));
}

// --- the Terminal default-directory row (default-spawn DS-1) ---

#[test]
fn default_directory_lands_after_arguments_in_the_terminal_card() {
    // The "Default directory" row lands after Arguments in the
    // Terminal card (the full key list pinned by the Terminal-card
    // test above), under the field's exact file spelling, with the
    // same new-tabs-only timing as the shell trio it joins.
    assert_eq!(ConfigKey::DefaultCwd.label(), "Default directory");
    assert_eq!(ConfigKey::DefaultCwd.key_name(), "default_cwd");
    assert_eq!(apply_timing(ConfigKey::DefaultCwd), ApplyTiming::NewTab);
}

#[test]
fn submitted_default_directory_text_parses_through_parse_cwd() {
    // The profile form's `parse_cwd` precedent: empty text clears
    // the key (`None` = the pty's default directory), any other
    // path lands verbatim — existence is never checked at write
    // time; spawn-time fallback is the live check.
    assert_eq!(
        submit_write(TextField::DefaultCwd, "/srv/work"),
        Some(PlannedWrite::DefaultCwd(Some(PathBuf::from("/srv/work"))))
    );
    assert_eq!(
        submit_write(TextField::DefaultCwd, "   "),
        Some(PlannedWrite::DefaultCwd(None))
    );
}

#[test]
fn buffer_default_directory_text_falls_back_to_the_config_path() {
    // The input shows the stored path (empty for unset — the
    // placeholder carries the hint), and a draft wins until
    // cleared.
    let mut config = DropTermConfig::default();
    assert_eq!(
        SettingsBuffer::default().text(TextField::DefaultCwd, &config),
        ""
    );
    config.default_cwd = Some(PathBuf::from("/srv/work"));
    assert_eq!(
        SettingsBuffer::default().text(TextField::DefaultCwd, &config),
        "/srv/work"
    );
    let mut buffer = SettingsBuffer::default();
    buffer.set_text(TextField::DefaultCwd, "/tmp".to_string());
    assert_eq!(buffer.text(TextField::DefaultCwd, &config), "/tmp");
}

// --- the Appearance theme dropdown ---

#[test]
fn theme_preference_sits_in_appearance_after_the_scheme_rows() {
    // The "Application theme" row lands after the scheme rows.
    assert_eq!(
        Section::Appearance.keys(),
        &[
            ConfigKey::FontName,
            ConfigKey::FontSize,
            ConfigKey::Opacity,
            ConfigKey::ColorSchemeDark,
            ConfigKey::ColorSchemeLight,
            ConfigKey::ThemePreference,
        ]
    );
    assert_eq!(ConfigKey::ThemePreference.label(), "Application theme");
}

#[test]
fn theme_preference_options_are_exactly_follow_system_dark_light() {
    // Follow system first, so the default config's selection is
    // index 0.
    assert_eq!(
        THEME_PREFERENCE_OPTIONS,
        &[
            (ThemePreference::System, "Follow system"),
            (ThemePreference::Dark, "Dark"),
            (ThemePreference::Light, "Light"),
        ]
    );
}

#[test]
fn theme_preference_picks_round_trip_and_out_of_range_rejects() {
    // A pick commits exactly its variant — the control can never
    // invent a value; indices past the table (including exactly
    // len) reject instead of panicking.
    assert_eq!(
        dropdown_write(ConfigKey::ThemePreference, 0),
        Some(PlannedWrite::ThemePreference(ThemePreference::System))
    );
    assert_eq!(
        dropdown_write(ConfigKey::ThemePreference, 1),
        Some(PlannedWrite::ThemePreference(ThemePreference::Dark))
    );
    assert_eq!(
        dropdown_write(ConfigKey::ThemePreference, 2),
        Some(PlannedWrite::ThemePreference(ThemePreference::Light))
    );
    assert_eq!(dropdown_write(ConfigKey::ThemePreference, 3), None);
    assert_eq!(dropdown_write(ConfigKey::ThemePreference, 99), None);
}

#[test]
fn theme_preference_selection_mirrors_the_config_value() {
    // Default config → Follow system → Some(0); every committed
    // variant re-selects its own position.
    let mut config = DropTermConfig::default();
    assert_eq!(
        dropdown_selected(ConfigKey::ThemePreference, &config),
        Some(0)
    );
    config.theme_preference = ThemePreference::Dark;
    assert_eq!(
        dropdown_selected(ConfigKey::ThemePreference, &config),
        Some(1)
    );
    config.theme_preference = ThemePreference::Light;
    assert_eq!(
        dropdown_selected(ConfigKey::ThemePreference, &config),
        Some(2)
    );
}

// --- the Geometry tab-position dropdown ---

#[test]
fn the_window_card_carries_the_geometry_four() {
    // The Window card: the four keys describing the window's
    // shape, formerly the Geometry card minus the tab-bar
    // position (which leads the Tabs card below).
    assert_eq!(
        Section::Window.keys(),
        &[
            ConfigKey::HeightPercent,
            ConfigKey::WidthPercent,
            ConfigKey::HAlign,
            ConfigKey::Edge,
        ]
    );
}

#[test]
fn tab_position_options_are_exactly_follow_edge_top_bottom() {
    // Follow edge first, so the default config's selection is
    // index 0 (the Follow system pattern).
    assert_eq!(
        TAB_POSITION_OPTIONS,
        &[
            (TabPosition::Auto, "Follow edge"),
            (TabPosition::Top, "Top"),
            (TabPosition::Bottom, "Bottom"),
        ]
    );
}

#[test]
fn tab_position_picks_round_trip_and_out_of_range_rejects() {
    // A pick commits exactly its variant — the control can never
    // invent a value; indices past the table (including exactly
    // len) reject instead of panicking.
    assert_eq!(
        dropdown_write(ConfigKey::TabPosition, 0),
        Some(PlannedWrite::TabPosition(TabPosition::Auto))
    );
    assert_eq!(
        dropdown_write(ConfigKey::TabPosition, 1),
        Some(PlannedWrite::TabPosition(TabPosition::Top))
    );
    assert_eq!(
        dropdown_write(ConfigKey::TabPosition, 2),
        Some(PlannedWrite::TabPosition(TabPosition::Bottom))
    );
    assert_eq!(dropdown_write(ConfigKey::TabPosition, 3), None);
    assert_eq!(dropdown_write(ConfigKey::TabPosition, 99), None);
}

#[test]
fn tab_position_selection_mirrors_the_config_value() {
    // Default config → Auto → Some(0); every committed variant
    // re-selects its own position.
    let mut config = DropTermConfig::default();
    assert_eq!(dropdown_selected(ConfigKey::TabPosition, &config), Some(0));
    config.tab_position = TabPosition::Top;
    assert_eq!(dropdown_selected(ConfigKey::TabPosition, &config), Some(1));
    config.tab_position = TabPosition::Bottom;
    assert_eq!(dropdown_selected(ConfigKey::TabPosition, &config), Some(2));
}

// --- the Behavior tab-titles dropdown ---

#[test]
fn the_tabs_card_groups_the_tab_strip_keys() {
    // The Tabs card: everything about the tab strip on one card —
    // position, titles, new-tab placement and its default
    // directory (default-spawn DS-1), close confirmation, and
    // session persistence (restoring *tabs*). The title mode's
    // flip applies live through the retitle loop.
    assert_eq!(
        Section::Tabs.keys(),
        &[
            ConfigKey::TabPosition,
            ConfigKey::TabTitleMode,
            ConfigKey::NewTabAfterCurrent,
            ConfigKey::NewTabCwd,
            ConfigKey::CloseConfirmationEnabled,
            ConfigKey::SessionPersistenceEnabled,
        ]
    );
    assert_eq!(ConfigKey::TabPosition.label(), "Tab bar position");
    assert_eq!(ConfigKey::TabTitleMode.label(), "Tab titles");
    assert_eq!(
        ConfigKey::NewTabAfterCurrent.label(),
        "New tabs after current tab"
    );
    assert_eq!(ConfigKey::TabTitleMode.key_name(), "tab_title_mode");
    assert_eq!(apply_timing(ConfigKey::TabTitleMode), ApplyTiming::Live);
    assert_eq!(
        toggle_write(ConfigKey::NewTabAfterCurrent, false),
        Some(PlannedWrite::NewTabAfterCurrent(false))
    );
}

#[test]
fn tab_title_mode_options_are_exactly_the_five_modes() {
    // Shell title first, so the default config's selection is
    // index 0 (the Follow system pattern).
    assert_eq!(
        TAB_TITLE_MODE_OPTIONS,
        &[
            (TabTitleMode::Shell, "Shell title"),
            (TabTitleMode::AbbreviatedPath, "Abbreviated path"),
            (TabTitleMode::LastSegment, "Last segment"),
            (TabTitleMode::Numbered, "Terminal number"),
            (TabTitleMode::Profile, "Profile name"),
        ]
    );
}

#[test]
fn tab_title_mode_picks_round_trip_and_out_of_range_rejects() {
    // A pick commits exactly its variant — the control can never
    // invent a value; indices past the table (including exactly
    // len) reject instead of panicking.
    assert_eq!(
        dropdown_write(ConfigKey::TabTitleMode, 0),
        Some(PlannedWrite::TabTitleMode(TabTitleMode::Shell))
    );
    assert_eq!(
        dropdown_write(ConfigKey::TabTitleMode, 1),
        Some(PlannedWrite::TabTitleMode(TabTitleMode::AbbreviatedPath))
    );
    assert_eq!(
        dropdown_write(ConfigKey::TabTitleMode, 2),
        Some(PlannedWrite::TabTitleMode(TabTitleMode::LastSegment))
    );
    assert_eq!(
        dropdown_write(ConfigKey::TabTitleMode, 3),
        Some(PlannedWrite::TabTitleMode(TabTitleMode::Numbered))
    );
    assert_eq!(
        dropdown_write(ConfigKey::TabTitleMode, 4),
        Some(PlannedWrite::TabTitleMode(TabTitleMode::Profile))
    );
    assert_eq!(dropdown_write(ConfigKey::TabTitleMode, 5), None);
    assert_eq!(dropdown_write(ConfigKey::TabTitleMode, 99), None);
    // The write targets exactly its own key, nothing else.
    assert_eq!(
        PlannedWrite::TabTitleMode(TabTitleMode::Numbered).key(),
        ConfigKey::TabTitleMode
    );
}

#[test]
fn tab_title_mode_selection_mirrors_the_config_value() {
    // Default config → Shell → Some(0); every committed variant
    // re-selects its own position, so the dropdown opens on the
    // file's current value.
    let mut config = DropTermConfig::default();
    assert_eq!(dropdown_selected(ConfigKey::TabTitleMode, &config), Some(0));
    config.tab_title_mode = TabTitleMode::AbbreviatedPath;
    assert_eq!(dropdown_selected(ConfigKey::TabTitleMode, &config), Some(1));
    config.tab_title_mode = TabTitleMode::LastSegment;
    assert_eq!(dropdown_selected(ConfigKey::TabTitleMode, &config), Some(2));
    config.tab_title_mode = TabTitleMode::Numbered;
    assert_eq!(dropdown_selected(ConfigKey::TabTitleMode, &config), Some(3));
}

#[test]
fn scheme_dropdown_options_are_the_registry_and_every_option_resolves() {
    let options = crate::terminal::theme_source::scheme_names();
    // Twelve shipped schemes, COSMIC pair first — the picker's
    // order is the registry's order.
    assert_eq!(options.len(), 12);
    assert_eq!(options.first(), Some(&crate::config::COSMIC_THEME_DARK));
    assert_eq!(options.get(1), Some(&crate::config::COSMIC_THEME_LIGHT));
    // What the picker offers is exactly what `by_name` serves, so
    // a pick can never name a scheme lookup would reject.
    for name in &options {
        assert!(
            crate::terminal::theme_source::by_name(name).is_some(),
            "{name:?} does not resolve"
        );
    }
}

#[test]
fn scheme_dropdown_picks_commit_registry_names_only() {
    // A pick commits exactly the option's name — the control can
    // never invent or misspell a value.
    assert_eq!(
        dropdown_write(ConfigKey::ColorSchemeDark, 0),
        Some(PlannedWrite::ColorSchemeDark("COSMIC Dark".to_string()))
    );
    assert_eq!(
        dropdown_write(ConfigKey::ColorSchemeLight, 4),
        Some(PlannedWrite::ColorSchemeLight("Dracula".to_string()))
    );
    // Out of range (including exactly len) rejects instead of
    // panicking — no index past the table commits anything.
    let len = crate::terminal::theme_source::scheme_names().len();
    assert_eq!(dropdown_write(ConfigKey::ColorSchemeDark, len), None);
}

#[test]
fn scheme_selection_opens_on_the_config_name_and_unknowns_select_nothing() {
    let mut config = DropTermConfig::default();
    // Defaults are the COSMIC pair — the registry's first two
    // entries — so both dropdowns open on a real selection.
    assert_eq!(
        dropdown_selected(ConfigKey::ColorSchemeDark, &config),
        Some(0)
    );
    assert_eq!(
        dropdown_selected(ConfigKey::ColorSchemeLight, &config),
        Some(1)
    );
    // A committed name selects its registry position.
    config.color_scheme_dark = "Dracula".to_string();
    assert_eq!(
        dropdown_selected(ConfigKey::ColorSchemeDark, &config),
        Some(4)
    );
    // An unknown name (stale, hand-typed, renamed upstream) shows
    // no selection — the control never invents one, while
    // `active_colors` keeps serving the COSMIC fallback.
    config.color_scheme_dark = "Tokyo Night".to_string();
    assert_eq!(dropdown_selected(ConfigKey::ColorSchemeDark, &config), None);
}

#[test]
fn slider_release_commits_the_ranged_keys_clamped() {
    // A stale or malformed buffered slider value can never
    // persist out of range.
    assert_eq!(
        slider_release_write(ConfigKey::HeightPercent, 250.0),
        Some(PlannedWrite::HeightPercent(MAX_HEIGHT_PERCENT))
    );
    assert_eq!(
        slider_release_write(ConfigKey::WidthPercent, 1.0),
        Some(PlannedWrite::WidthPercent(MIN_WIDTH_PERCENT))
    );
    assert_eq!(
        slider_release_write(ConfigKey::Opacity, 300.0),
        Some(PlannedWrite::Opacity(100))
    );
    assert_eq!(
        slider_release_write(ConfigKey::FontSize, 13.6),
        Some(PlannedWrite::FontSize(14))
    );
}

#[test]
fn slider_release_rejects_keys_without_sliders() {
    assert!(slider_release_write(ConfigKey::Shell, 1.0).is_none());
    assert!(slider_release_write(ConfigKey::Layer, 0.0).is_none());
}

#[test]
fn buffer_text_falls_back_to_the_config_value() {
    let config = DropTermConfig::default();
    let buffer = SettingsBuffer::default();
    assert_eq!(buffer.text(TextField::FontName, &config), "Noto Sans Mono");
    assert_eq!(buffer.text(TextField::ScrollbackLines, &config), "10000");
}

#[test]
fn buffer_text_draft_wins_until_it_is_cleared() {
    let config = DropTermConfig::default();
    let mut buffer = SettingsBuffer::default();
    buffer.set_text(TextField::FontName, "JetBrains Mono".to_string());
    assert_eq!(buffer.text(TextField::FontName, &config), "JetBrains Mono");
    buffer.clear_text(TextField::FontName);
    assert_eq!(buffer.text(TextField::FontName, &config), "Noto Sans Mono");
}

#[test]
fn buffer_slider_value_falls_back_to_the_config_value() {
    let config = DropTermConfig::default();
    let mut buffer = SettingsBuffer::default();
    assert_eq!(buffer.slider_value(ConfigKey::HeightPercent, &config), 40.0);
    buffer.set_slider(ConfigKey::HeightPercent, 77.0);
    assert_eq!(buffer.slider_value(ConfigKey::HeightPercent, &config), 77.0);
}

#[test]
fn clearing_the_buffer_restores_full_config_fallback() {
    // Reopen freshness: nothing a previous window session
    // buffered survives into a reopened window.
    let config = DropTermConfig::default();
    let mut buffer = SettingsBuffer::default();
    buffer.set_text(TextField::Shell, "/bin/tcsh".to_string());
    buffer.set_slider(ConfigKey::Opacity, 12.0);
    buffer.clear();
    assert_eq!(buffer.text(TextField::Shell, &config), "");
    assert_eq!(buffer.slider_value(ConfigKey::Opacity, &config), 100.0);
}

#[test]
fn buffer_shell_text_shows_empty_for_an_unconfigured_shell() {
    // The *placeholder* carries resolved_shell(); the input's own
    // content stays faithful to the file — empty means unset.
    let config = DropTermConfig::default();
    assert_eq!(
        SettingsBuffer::default().text(TextField::Shell, &config),
        ""
    );
}

// --- the Profiles page's pure seams (profile-settings T1) ---

fn profile(name: &str, command: &str) -> Profile {
    Profile {
        name: name.to_string(),
        command: command.to_string(),
        cwd: None,
        color_scheme: None,
        env: None,
        font_size: None,
        scrollback_lines: None,
    }
}

#[test]
fn name_unique_rejects_an_exact_duplicate_and_only_an_exact_one() {
    // Name matching is exact and case-sensitive (the model's id
    // rule): "SSH" does not collide with "ssh".
    let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
    assert!(!name_unique(&profiles, None, "ssh"));
    assert!(name_unique(&profiles, None, "SSH"));
    assert!(name_unique(&profiles, None, "new"));
}

#[test]
fn name_unique_excludes_the_edited_profile_itself() {
    // Re-committing a profile's own name is not a collision (the
    // self-exclusion on edit); any *other* holder still blocks.
    let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
    assert!(name_unique(&profiles, Some(0), "ssh"));
    assert!(!name_unique(&profiles, Some(0), "top"));
}

#[test]
fn default_after_rename_follows_a_renamed_default() {
    // The cascade: renaming the profile `default_profile` names
    // re-points the key in the same interaction — a paired write.
    assert_eq!(
        default_after_rename(Some("ssh"), "ssh", "server"),
        Some("server".to_string())
    );
}

#[test]
fn default_after_rename_leaves_every_other_rename_alone() {
    // Non-default renames and an unset default write nothing
    // extra — no paired write outside the cascade.
    assert_eq!(
        default_after_rename(Some("top"), "ssh", "server"),
        Some("top".to_string())
    );
    assert_eq!(default_after_rename(None, "ssh", "server"), None);
}

#[test]
fn default_after_delete_clears_when_it_named_the_deleted_profile() {
    // The owner's auto-clear ruling: the delete proceeds and the
    // key clears — a write must never dangle.
    assert_eq!(default_after_delete(Some("ssh"), "ssh"), None);
}

#[test]
fn default_after_delete_keeps_a_differently_named_default() {
    assert_eq!(
        default_after_delete(Some("top"), "ssh"),
        Some("top".to_string())
    );
    assert_eq!(default_after_delete(None, "ssh"), None);
}

#[test]
fn clone_name_seeds_the_copy_suffix() {
    let profiles = [profile("web-prod", "ssh web-prod")];
    assert_eq!(clone_name(&profiles, &profiles[0]), "web-prod (copy)");
}

#[test]
fn clone_name_escalates_the_suffix_on_collision() {
    // "(copy)" taken → "(copy 2)", then "(copy 3)" — the seed is
    // always unique against the list it will join.
    let profiles = [
        profile("web-prod", "ssh web-prod"),
        profile("web-prod (copy)", "ssh web-prod"),
    ];
    assert_eq!(
        clone_name(&profiles, &profiles[0]),
        "web-prod (copy 2)".to_string()
    );
    let deeper = [
        profile("web-prod", "ssh web-prod"),
        profile("web-prod (copy)", "ssh web-prod"),
        profile("web-prod (copy 2)", "ssh web-prod"),
    ];
    assert_eq!(clone_name(&deeper, &deeper[0]), "web-prod (copy 3)");
}

#[test]
fn add_profile_appends_at_the_end() {
    // Config order is the `+` dropdown / context-menu order, so
    // the list transform keeps append semantics explicit.
    let profiles = [profile("ssh", "ssh web-prod")];
    let added = add_profile(&profiles, profile("top", "htop"));
    assert_eq!(added.len(), 2);
    assert_eq!(added[1].name, "top");
}

#[test]
fn apply_edit_replaces_in_place_and_ignores_a_stale_index() {
    let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
    let edited = apply_edit(&profiles, 1, profile("monitor", "btop"));
    assert_eq!(edited[0].name, "ssh");
    assert_eq!(edited[1].name, "monitor");
    assert_eq!(edited.len(), 2);
    // An out-of-bounds index (config shrank under a stale click)
    // is a pure no-op, never a panic.
    assert_eq!(apply_edit(&profiles, 9, profile("ghost", "true")), profiles);
}

#[test]
fn remove_profile_drops_only_the_index_and_ignores_a_stale_one() {
    let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
    let removed = remove_profile(&profiles, 0);
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].name, "top");
    assert_eq!(remove_profile(&profiles, 9), profiles);
}

#[test]
fn move_profile_swaps_exactly_one_slot_per_press() {
    let profiles = [
        profile("a", "true"),
        profile("b", "true"),
        profile("c", "true"),
    ];
    let up = move_profile(&profiles, 1, ReorderDirection::Up);
    assert_eq!(
        up.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["b", "a", "c"]
    );
    let down = move_profile(&profiles, 1, ReorderDirection::Down);
    assert_eq!(
        down.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["a", "c", "b"]
    );
}

#[test]
fn move_profile_is_a_no_op_at_the_boundaries_and_beyond() {
    // Boundary rows have no out-of-bounds neighbor: the transform
    // is a pure no-op (the view disables the direction instead).
    let profiles = [profile("a", "true"), profile("b", "true")];
    assert_eq!(move_profile(&profiles, 0, ReorderDirection::Up), profiles);
    assert_eq!(move_profile(&profiles, 1, ReorderDirection::Down), profiles);
    assert_eq!(move_profile(&profiles, 9, ReorderDirection::Up), profiles);
}

#[test]
fn parse_cwd_clears_empty_text_to_none() {
    assert_eq!(parse_cwd(""), None);
    assert_eq!(parse_cwd("   "), None);
}

#[test]
fn parse_cwd_accepts_a_path_without_existence_checks() {
    // The model's define-time rule: a profile may be written
    // before its directory exists; spawn-time fallback is
    // `profile-launch`'s.
    assert_eq!(
        parse_cwd("/dropterm/profile-settings/not/yet/created"),
        Some(PathBuf::from("/dropterm/profile-settings/not/yet/created"))
    );
    assert_eq!(parse_cwd(" ~/src "), Some(PathBuf::from("~/src")));
}

#[test]
fn draft_valid_rejects_blank_and_duplicate_names() {
    let profiles = [profile("ssh", "ssh web-prod")];
    assert!(!draft_valid(&profiles, None, "   ", "htop"));
    assert!(!draft_valid(&profiles, None, "ssh", "htop"));
    // The duplicate check is self-excluded on edit.
    assert!(draft_valid(&profiles, Some(0), "ssh", "htop"));
}

#[test]
fn draft_valid_rejects_a_command_line_without_a_program() {
    let profiles = [profile("ssh", "ssh web-prod")];
    assert!(!draft_valid(&profiles, None, "top", ""));
    assert!(!draft_valid(&profiles, None, "top", "   "));
}

#[test]
fn draft_valid_accepts_a_complete_triple() {
    let profiles = [profile("ssh", "ssh web-prod")];
    assert!(draft_valid(&profiles, None, "top", "htop"));
}

// --- the Profiles page's view (construction, profile-settings T1) ---

#[test]
fn the_profiles_page_builds_in_every_read_state() {
    // Construction is the assertion: empty list (the empty state
    // with its add affordance), a populated list, a stale
    // selection index, and every draft shape — an erroring add
    // draft, a clone draft, an edit draft — must all build
    // without panicking.
    let mut config = DropTermConfig::default();
    let _ = profiles_view(&config, None, None);
    let _ = profiles_view(&config, Some(0), None);
    config.profiles = vec![profile("ssh", "ssh web-prod"), profile("top", "htop")];
    let _ = profiles_view(&config, None, None);
    let _ = profiles_view(&config, Some(1), None);
    let _ = profiles_view(&config, Some(9), None);
    let mut erroring = ProfileDraft::add();
    erroring.errors = vec![(ProfileField::Name, ProfileFieldError::DuplicateName)];
    let _ = profiles_view(&config, None, Some(&erroring));
    let cloning = ProfileDraft::clone_of(&config.profiles[0], &config.profiles);
    let _ = profiles_view(&config, Some(0), Some(&cloning));
    let editing = ProfileDraft::edit(0, &config.profiles[0]);
    let _ = profiles_view(&config, Some(0), Some(&editing));
}

// --- the default-profile picker (profile-settings T2) ---

#[test]
fn default_picker_options_are_plain_shell_then_config_order() {
    // Row 0 is the None row; the profiles follow in `profiles`
    // Vec order — the `+` dropdown / context-menu order.
    assert_eq!(
        default_picker_options(&[]),
        vec!["None (plain shell)".to_string()]
    );
    let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
    assert_eq!(
        default_picker_options(&profiles),
        vec![
            "None (plain shell)".to_string(),
            "ssh".to_string(),
            "top".to_string()
        ]
    );
}

#[test]
fn default_picker_selection_mirrors_effective_default_never_a_missing_name() {
    // Unset → row 0; a set, existing name → its row; a dangling
    // name (hand-edited) falls back to row 0 — the picker never
    // displays a name the list lacks.
    let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
    assert_eq!(default_picker_selected(&profiles, None), 0);
    assert_eq!(default_picker_selected(&profiles, Some("top")), 2);
    assert_eq!(default_picker_selected(&profiles, Some("ghost")), 0);
    assert_eq!(default_picker_selected(&[], Some("ghost")), 0);
}

#[test]
fn default_picker_picks_resolve_to_the_key_value_or_reject() {
    // Row 0 commits the cleared key (`None` = plain shell); a
    // profile row commits its name; anything else is stale and
    // rejects — a pick can never invent a value.
    let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
    assert_eq!(
        default_picker_write(&profiles, 0),
        Some(ProfileWrite::DefaultProfile(None))
    );
    assert_eq!(
        default_picker_write(&profiles, 2),
        Some(ProfileWrite::DefaultProfile(Some("top".to_string())))
    );
    assert_eq!(default_picker_write(&profiles, 3), None);
    // The None row exists even on an empty list; profile rows do
    // not.
    assert_eq!(
        default_picker_write(&[], 0),
        Some(ProfileWrite::DefaultProfile(None))
    );
    assert_eq!(default_picker_write(&[], 1), None);
}

// --- the profile form's scheme dropdown (profile-appearance) ---

fn registry_row(name: &str) -> usize {
    scheme_names()
        .iter()
        .position(|option| *option == name)
        .expect("a registry name")
}

#[test]
fn profile_scheme_options_are_the_none_row_then_the_registry() {
    // Row 0 is the None row; the built-in registry follows in its
    // own order — the same vocabulary the global pickers feed
    // from, so the form can never offer a name `by_name` would
    // reject.
    let options = profile_scheme_options();
    assert_eq!(options[0], "None — follow global setting");
    assert_eq!(&options[1..], scheme_names().as_slice());
}

#[test]
fn profile_scheme_selection_maps_unset_and_unknown_to_the_none_row() {
    // Unset → row 0; a set, known name → its option row; a name
    // the registry lacks (hand-edited) falls back to row 0 — the
    // control shows the fallback the resolution chain is already
    // serving, never a value it would invent.
    assert_eq!(profile_scheme_selected(None), 0);
    assert_eq!(
        profile_scheme_selected(Some("Dracula")),
        registry_row("Dracula") + 1
    );
    assert_eq!(profile_scheme_selected(Some("Catppuccin Frappe")), 0);
}

#[test]
fn profile_scheme_picks_resolve_to_the_field_value_or_reject() {
    // Row 0 commits the cleared field (follow the global pick); a
    // registry row commits its name; anything else is stale and
    // rejects — a pick can never invent a value (the
    // `dropdown_write` rule).
    assert_eq!(profile_scheme_write(0), Some(ProfileSchemePick::None));
    assert_eq!(
        profile_scheme_write(registry_row("Nord") + 1),
        Some(ProfileSchemePick::Scheme("Nord".to_string()))
    );
    assert_eq!(profile_scheme_write(profile_scheme_options().len()), None);
}

#[test]
fn a_scheme_pick_round_trips_through_apply_edit() {
    // The Edit-mode write the pick commits: the base profile with
    // only `color_scheme` replaced — pick a scheme, write, pick
    // None, write back, and the field returns to its start.
    let profiles = [sourced_profile()];
    let mut edited = profiles[0].clone();
    edited.color_scheme = profile_scheme_write(registry_row("Dracula") + 1)
        .expect("a registry row")
        .field_value();
    let written = apply_edit(&profiles, 0, edited);
    assert_eq!(written[0].color_scheme, Some("Dracula".to_string()));

    let mut cleared = written[0].clone();
    cleared.color_scheme = profile_scheme_write(0).expect("the None row").field_value();
    let restored = apply_edit(&written, 0, cleared);
    assert_eq!(restored[0].color_scheme, None);
}

#[test]
fn an_add_drafts_scheme_pick_rides_the_first_valid_commit() {
    // Gated identically to its siblings: the pick buffers in an
    // Add/Clone draft — no write until the triple commits — then
    // lands with the added profile.
    let profiles = [profile("ssh", "ssh web-prod")];
    let mut draft = ProfileDraft::add();
    draft.name = "monitor".to_string();
    draft.command = "btop".to_string();
    draft.scheme = profile_scheme_write(1)
        .expect("row 1 is the first registry name")
        .field_value();
    let ProfileCommitPlan::Write {
        profiles: written, ..
    } = plan_profile_commit(&draft, ProfileField::Name, "monitor", &profiles, None)
    else {
        panic!("expected a write plan");
    };
    assert_eq!(written[1].color_scheme, Some(scheme_names()[0].to_string()));
}

#[test]
fn draft_seeding_carries_the_scheme_into_clone_and_edit_forms() {
    // A Clone draft copies the source's scheme verbatim (the
    // name is what re-seeds unique); an Edit draft seeds from the
    // profile; an Add draft starts unset.
    let mut source = sourced_profile();
    source.color_scheme = Some("Dracula".to_string());
    assert_eq!(
        ProfileDraft::clone_of(&source, &[source.clone()]).scheme,
        Some("Dracula".to_string())
    );
    assert_eq!(
        ProfileDraft::edit(0, &source).scheme,
        Some("Dracula".to_string())
    );
    assert_eq!(ProfileDraft::add().scheme, None);
}

// --- the profile form's env rows (profile-env) ---

fn env_row(key: &str, value: &str) -> ProfileEnvRow {
    ProfileEnvRow {
        key: key.to_string(),
        value: value.to_string(),
    }
}

fn sourced_env_profile() -> Profile {
    Profile {
        env: Some(BTreeMap::from([
            ("DROPTERM_PROFILE".to_string(), "ssh-prod".to_string()),
            ("TERM_PROGRAM".to_string(), "dropterm-ssh".to_string()),
        ])),
        ..sourced_profile()
    }
}

#[test]
fn env_map_from_rows_drops_wholly_blank_rows() {
    // An abandoned row (nothing typed in either input) holds
    // nothing: it drops at the write without a word — never an
    // error, never an empty-string variable.
    assert_eq!(
        env_map_from_rows(&[
            env_row("", ""),
            env_row("DROPTERM_PROFILE", "prod"),
            env_row("   ", "   "),
        ]),
        Some(BTreeMap::from([(
            "DROPTERM_PROFILE".to_string(),
            "prod".to_string()
        )]))
    );
}

#[test]
fn env_map_from_rows_trims_keys_and_values() {
    // The form's uniform write rule (the name/command precedent):
    // surrounding whitespace is input debris, never part of the
    // variable. Interior characters — including `=` in a value —
    // are verbatim (ruling 2).
    assert_eq!(
        env_map_from_rows(&[env_row("  KEY  ", "  a=b  ")]),
        Some(BTreeMap::from([("KEY".to_string(), "a=b".to_string())]))
    );
}

#[test]
fn env_map_from_rows_is_none_when_nothing_survives() {
    // "No variables" is the field's unset shape (`None`), never
    // an empty map the store would carry as a set field.
    assert_eq!(env_map_from_rows(&[]), None);
    assert_eq!(env_map_from_rows(&[env_row("", "")]), None);
}

#[test]
fn env_map_from_rows_keeps_the_last_row_on_a_repeated_key() {
    // Ruling 1: discrete rows make duplicates a non-concern; if
    // the buffer holds one anyway, the last row wins (BTreeMap
    // insert order) — never an error surface.
    assert_eq!(
        env_map_from_rows(&[env_row("KEY", "first"), env_row("KEY", "second")]),
        Some(BTreeMap::from([("KEY".to_string(), "second".to_string())]))
    );
}

#[test]
fn env_map_from_rows_never_emits_an_invalid_key() {
    // The airtight floor under the gate: rows the editor rejects
    // (a value without its key, an `=`-bearing key — a name POSIX
    // environ entries cannot carry) also drop here, so no path —
    // however it reaches the transform — can land a broken entry
    // in the store.
    assert_eq!(
        env_map_from_rows(&[
            env_row("", "orphan"),
            env_row("A=B", "v"),
            env_row("KEY", "v")
        ]),
        Some(BTreeMap::from([("KEY".to_string(), "v".to_string())]))
    );
}

#[test]
fn env_row_invalid_flags_a_value_without_its_key() {
    // A value-only row is a variable half-defined: reject at its
    // row, so the inline error points where the key belongs.
    assert_eq!(
        env_row_invalid(&[env_row("KEY", "v"), env_row("", "orphan")]),
        Some((1, ProfileFieldError::BlankEnvKey))
    );
}

#[test]
fn env_row_invalid_flags_an_equals_bearing_key() {
    assert_eq!(
        env_row_invalid(&[env_row("A=B", "v"), env_row("KEY", "v")]),
        Some((0, ProfileFieldError::EnvKeyEquals))
    );
}

#[test]
fn env_row_invalid_passes_blank_and_wholesome_rows() {
    // A wholly blank row is an abandoned row, not a violation
    // (the transform drops it); an empty value is legitimate —
    // `FOO=` is a real variable.
    assert_eq!(
        env_row_invalid(&[env_row("", ""), env_row("KEY", "v"), env_row("EMPTY", "")]),
        None
    );
}

#[test]
fn env_rows_seed_from_the_stored_map_in_btree_order() {
    // Clone and Edit seed one row per entry in the map's BTree
    // order — the same profile opens the same form every time;
    // an Add draft starts with no rows.
    let source = sourced_env_profile();
    let seeded = vec![
        env_row("DROPTERM_PROFILE", "ssh-prod"),
        env_row("TERM_PROGRAM", "dropterm-ssh"),
    ];
    assert_eq!(
        ProfileDraft::clone_of(&source, std::slice::from_ref(&source)).env_rows,
        seeded
    );
    assert_eq!(ProfileDraft::edit(0, &source).env_rows, seeded);
    assert!(ProfileDraft::add().env_rows.is_empty());
}

#[test]
fn an_edit_env_commit_writes_the_map_from_the_rows() {
    // Enter in any env input commits the whole map from the row
    // list — the row inputs' Enter is the form's commit gesture,
    // exactly like the name input's. The committed row's raw
    // text lands in its row first (the `raw` rule), blank rows
    // drop, everything else lands trimmed.
    let profiles = [sourced_env_profile()];
    let mut draft = ProfileDraft::edit(0, &profiles[0]);
    draft.env_rows.push(env_row("  EXTRA  ", " 1 "));
    let ProfileCommitPlan::Write {
        profiles: written, ..
    } = plan_profile_commit(&draft, ProfileField::EnvKey(2), "EXTRA", &profiles, None)
    else {
        panic!("expected a write plan");
    };
    assert_eq!(
        written[0].env,
        Some(BTreeMap::from([
            ("DROPTERM_PROFILE".to_string(), "ssh-prod".to_string()),
            ("TERM_PROGRAM".to_string(), "dropterm-ssh".to_string()),
            ("EXTRA".to_string(), "1".to_string()),
        ]))
    );
}

#[test]
fn an_edit_env_commit_rejects_an_equals_key_at_its_row() {
    // The gate is the whole row list, not the committed row: the
    // first invalid row rejects the commit with its own inline
    // error (a value Enter can catch a bad key buffered rows
    // away).
    let profiles = [sourced_env_profile()];
    let mut draft = ProfileDraft::edit(0, &profiles[0]);
    draft.env_rows.push(env_row("A=B", "v"));
    assert_eq!(
        plan_profile_commit(
            &draft,
            ProfileField::EnvValue(0),
            "ssh-prod",
            &profiles,
            None
        ),
        ProfileCommitPlan::Invalid {
            field: ProfileField::EnvKey(2),
            error: ProfileFieldError::EnvKeyEquals,
        }
    );
}

#[test]
fn an_edit_env_commit_rejects_a_value_without_its_key() {
    let profiles = [sourced_env_profile()];
    let mut draft = ProfileDraft::edit(0, &profiles[0]);
    draft.env_rows.push(env_row("", "orphan"));
    assert_eq!(
        plan_profile_commit(
            &draft,
            ProfileField::EnvKey(0),
            "DROPTERM_PROFILE",
            &profiles,
            None
        ),
        ProfileCommitPlan::Invalid {
            field: ProfileField::EnvKey(2),
            error: ProfileFieldError::BlankEnvKey,
        }
    );
}

#[test]
fn an_edit_name_commit_never_touches_the_env_map() {
    // Per-field isolation (the form's standing rule): the rows
    // are the env inputs' buffer; another field's Enter writes
    // `base.env` untouched — buffered rows ride along nowhere.
    let profiles = [sourced_env_profile()];
    let mut draft = ProfileDraft::edit(0, &profiles[0]);
    draft.env_rows.clear();
    draft.env_rows.push(env_row("BUFFERED", "never-committed"));
    let ProfileCommitPlan::Write {
        profiles: written, ..
    } = plan_profile_commit(&draft, ProfileField::Name, "ssh-renamed", &profiles, None)
    else {
        panic!("expected a write plan");
    };
    assert_eq!(written[0].name, "ssh-renamed");
    assert_eq!(written[0].env, sourced_env_profile().env);
}

#[test]
fn an_add_drafts_env_rows_ride_the_first_valid_commit() {
    // The scheme pick's rule: the rows buffer in an Add/Clone
    // draft — no profile exists to write until the triple
    // commits — then the transformed map lands with it.
    let profiles = [profile("ssh", "ssh web-prod")];
    let mut draft = ProfileDraft::add();
    draft.name = "monitor".to_string();
    draft.command = "btop".to_string();
    draft.env_rows = vec![env_row("DROPTERM_PROFILE", "monitor"), env_row("", "")];
    let ProfileCommitPlan::Write {
        profiles: written, ..
    } = plan_profile_commit(&draft, ProfileField::Name, "monitor", &profiles, None)
    else {
        panic!("expected a write plan");
    };
    assert_eq!(
        written[1].env,
        Some(BTreeMap::from([(
            "DROPTERM_PROFILE".to_string(),
            "monitor".to_string()
        )]))
    );
}

#[test]
fn an_add_drafts_invalid_row_gates_the_first_commit() {
    // The first commit writes the env map too, so the row gate
    // rides the whole-triple gate: a bad row blocks the profile's
    // entry until fixed, never corrupts it.
    let profiles = [profile("ssh", "ssh web-prod")];
    let mut draft = ProfileDraft::add();
    draft.name = "monitor".to_string();
    draft.command = "btop".to_string();
    draft.env_rows = vec![env_row("A=B", "v")];
    assert_eq!(
        plan_profile_commit(&draft, ProfileField::Command, "btop", &profiles, None),
        ProfileCommitPlan::Invalid {
            field: ProfileField::EnvKey(0),
            error: ProfileFieldError::EnvKeyEquals,
        }
    );
}

#[test]
fn a_clone_commit_writes_the_rows_not_the_source_map() {
    // The rows — not the source's stored map — are what a clone
    // writes: buffer edits between seeding and the first commit
    // land with the clone.
    let source = sourced_env_profile();
    let mut draft = ProfileDraft::clone_of(&source, std::slice::from_ref(&source));
    draft.env_rows[1].value = "edited".to_string();
    let ProfileCommitPlan::Write {
        profiles: written, ..
    } = plan_profile_commit(
        &draft,
        ProfileField::Name,
        draft.name.as_str(),
        std::slice::from_ref(&source),
        None,
    )
    else {
        panic!("expected a write plan");
    };
    assert_eq!(
        written[1].env,
        Some(BTreeMap::from([
            ("DROPTERM_PROFILE".to_string(), "ssh-prod".to_string()),
            ("TERM_PROGRAM".to_string(), "edited".to_string()),
        ]))
    );
}

#[test]
fn an_env_commit_reseeds_the_rows_from_the_written_profile() {
    // `field_committed`'s echo rule for the row list: the written
    // map is the rows' new truth (rows the write dropped — the
    // blank ones — disappear from the form too).
    let written = sourced_env_profile();
    let mut draft = ProfileDraft::edit(0, &written);
    draft.env_rows.push(env_row("", ""));
    draft.field_committed(ProfileField::EnvKey(0), &written);
    assert_eq!(
        draft.env_rows,
        vec![
            env_row("DROPTERM_PROFILE", "ssh-prod"),
            env_row("TERM_PROGRAM", "dropterm-ssh"),
        ]
    );
}

#[test]
fn env_rows_round_trip_through_apply_edit_and_reseeding() {
    // The store's shape and the form's shape meet twice — the
    // write (plan → apply_edit) and the reseed (`ProfileDraft::
    // edit` off the written list) — and the rows come back
    // identical, in the map's `BTree` order.
    let profiles = [sourced_env_profile()];
    let mut draft = ProfileDraft::edit(0, &profiles[0]);
    draft.env_rows.push(env_row("EXTRA", "1"));
    let ProfileCommitPlan::Write {
        profiles: written, ..
    } = plan_profile_commit(
        &draft,
        ProfileField::EnvValue(1),
        "dropterm-ssh",
        &profiles,
        None,
    )
    else {
        panic!("expected a write plan");
    };
    assert_eq!(
        ProfileDraft::edit(0, &written[0]).env_rows,
        vec![
            env_row("DROPTERM_PROFILE", "ssh-prod"),
            env_row("EXTRA", "1"),
            env_row("TERM_PROGRAM", "dropterm-ssh"),
        ]
    );
}

#[test]
fn typing_into_an_env_row_buffers_and_clears_the_stale_error() {
    // The keystroke rule, row edition: the input buffers into its
    // row and that row's stale error clears — it described text
    // that no longer exists.
    let mut draft = ProfileDraft::edit(0, &sourced_env_profile());
    draft.errors = vec![(ProfileField::EnvKey(1), ProfileFieldError::EnvKeyEquals)];
    draft.set_field_text(ProfileField::EnvKey(1), "TERM".to_string());
    assert_eq!(draft.env_rows[1].key, "TERM");
    assert!(draft.errors.is_empty());
    draft.set_field_text(ProfileField::EnvValue(0), "prod".to_string());
    assert_eq!(draft.env_rows[0].value, "prod");
}

#[test]
fn an_env_enter_for_a_row_that_no_longer_exists_plans_a_drop() {
    // Unreachable in practice (row removal and the Enter are
    // ordered events on one channel), but a stale index must
    // never panic — it reports the same staleness a vanished
    // profile does.
    let profiles = [sourced_env_profile()];
    let draft = ProfileDraft::edit(0, &profiles[0]);
    assert_eq!(
        plan_profile_commit(&draft, ProfileField::EnvKey(9), "KEY", &profiles, None),
        ProfileCommitPlan::StaleEdit
    );
}

// --- the profile form: drafts, commits, lifecycle (T3) ---

fn sourced_profile() -> Profile {
    Profile {
        name: "web-prod".to_string(),
        command: "ssh web-prod".to_string(),
        cwd: Some(PathBuf::from("/srv/web")),
        color_scheme: None,
        env: None,
        font_size: None,
        scrollback_lines: None,
    }
}

#[test]
fn an_add_draft_opens_empty() {
    let draft = ProfileDraft::add();
    assert_eq!(draft.name, "");
    assert_eq!(draft.command, "");
    assert_eq!(draft.cwd, "");
    assert!(draft.errors.is_empty());
}

#[test]
fn commits_never_reset_the_overrides_the_form_does_not_expose() {
    // The form has no font/scrollback fields yet (profile-manager's
    // scope), so no commit gesture may reset them either: an Edit's
    // base clone carries them by construction, a Clone copies the
    // source's, and only an Add starts unset.
    let overridden = Profile {
        font_size: Some(11),
        scrollback_lines: Some(50_000),
        ..sourced_profile()
    };
    let profiles = [overridden.clone()];

    let edit = ProfileDraft::edit(0, &profiles[0]);
    let ProfileCommitPlan::Write {
        profiles: edited, ..
    } = plan_profile_commit(&edit, ProfileField::Name, "renamed", &profiles, None)
    else {
        panic!("a valid edit commits");
    };
    assert_eq!(edited[0].font_size, Some(11));
    assert_eq!(edited[0].scrollback_lines, Some(50_000));

    let clone = ProfileDraft::clone_of(&profiles[0], &profiles);
    let ProfileCommitPlan::Write {
        profiles: cloned, ..
    } = plan_profile_commit(
        &clone,
        ProfileField::Command,
        "ssh web-prod",
        &profiles,
        None,
    )
    else {
        panic!("a valid clone commits");
    };
    assert_eq!(cloned[1].font_size, Some(11));
    assert_eq!(cloned[1].scrollback_lines, Some(50_000));

    let add = ProfileDraft::add();
    let mut typed = add;
    typed.name = "fresh".to_string();
    typed.command = "htop".to_string();
    let ProfileCommitPlan::Write {
        profiles: added, ..
    } = plan_profile_commit(&typed, ProfileField::Name, "fresh", &[], None)
    else {
        panic!("a valid add commits");
    };
    assert_eq!(added[0].font_size, None);
    assert_eq!(added[0].scrollback_lines, None);
}

#[test]
fn a_clone_draft_seeds_from_the_source_with_a_unique_name() {
    // The seed is T1's clone_name output; command and cwd copy
    // verbatim; no write happens here — seeding only.
    let profiles = [sourced_profile()];
    let draft = ProfileDraft::clone_of(&profiles[0], &profiles);
    assert_eq!(draft.name, "web-prod (copy)");
    assert_eq!(draft.command, "ssh web-prod");
    assert_eq!(draft.cwd, "/srv/web");
    assert!(draft.errors.is_empty());
}

#[test]
fn an_edit_draft_seeds_from_the_profile() {
    let draft = ProfileDraft::edit(0, &sourced_profile());
    assert_eq!(draft.name, "web-prod");
    assert_eq!(draft.command, "ssh web-prod");
    assert_eq!(draft.cwd, "/srv/web");
    // An unset cwd seeds as empty text.
    let bare = ProfileDraft::edit(0, &profile("top", "htop"));
    assert_eq!(bare.cwd, "");
}

#[test]
fn typing_into_a_draft_field_clears_the_stale_error() {
    let mut draft = ProfileDraft::add();
    draft.errors = vec![(ProfileField::Name, ProfileFieldError::DuplicateName)];
    draft.set_field_text(ProfileField::Name, "server".to_string());
    assert!(draft.errors.is_empty());
    assert_eq!(draft.name, "server");
}

#[test]
fn an_add_commit_appends_only_on_a_valid_triple() {
    // Draft-gated: with a valid name but a blank command the Enter
    // is invalid on the command field — nothing is written.
    let profiles = [profile("ssh", "ssh web-prod")];
    let mut draft = ProfileDraft::add();
    draft.name = "monitor".to_string();
    assert_eq!(
        plan_profile_commit(&draft, ProfileField::Name, "monitor", &profiles, None,),
        ProfileCommitPlan::Invalid {
            field: ProfileField::Command,
            error: ProfileFieldError::BlankCommand,
        }
    );
    // The first valid commit appends — and writes no default.
    draft.command = "btop".to_string();
    let ProfileCommitPlan::Write {
        profiles: written,
        default_after_rename,
    } = plan_profile_commit(&draft, ProfileField::Command, "btop", &profiles, None)
    else {
        panic!("a valid triple must commit");
    };
    assert_eq!(written.len(), 2);
    assert_eq!(written[1].name, "monitor");
    assert_eq!(written[1].command, "btop");
    assert_eq!(default_after_rename, None);
}

#[test]
fn an_add_commit_rejects_blank_and_duplicate_names_at_the_name_field() {
    let profiles = [profile("ssh", "ssh web-prod")];
    let mut draft = ProfileDraft::add();
    draft.command = "htop".to_string();
    assert_eq!(
        plan_profile_commit(&draft, ProfileField::Name, "  ", &profiles, None),
        ProfileCommitPlan::Invalid {
            field: ProfileField::Name,
            error: ProfileFieldError::BlankName,
        }
    );
    assert_eq!(
        plan_profile_commit(&draft, ProfileField::Name, "ssh", &profiles, None),
        ProfileCommitPlan::Invalid {
            field: ProfileField::Name,
            error: ProfileFieldError::DuplicateName,
        }
    );
}

#[test]
fn an_add_commit_parses_the_cwd_field() {
    // The Enter'd raw text replaces the draft's cwd; empty clears
    // to None, a path lands verbatim (never existence-checked).
    let profiles = [profile("ssh", "ssh web-prod")];
    let mut draft = ProfileDraft::add();
    draft.name = "monitor".to_string();
    draft.command = "btop".to_string();
    let ProfileCommitPlan::Write {
        profiles: written, ..
    } = plan_profile_commit(&draft, ProfileField::Cwd, "/srv/mon", &profiles, None)
    else {
        panic!("expected a write plan")
    };
    assert_eq!(written[1].cwd, Some(PathBuf::from("/srv/mon")));
    let ProfileCommitPlan::Write {
        profiles: cleared, ..
    } = plan_profile_commit(&draft, ProfileField::Cwd, "   ", &profiles, None)
    else {
        panic!("expected a write plan")
    };
    assert_eq!(cleared[1].cwd, None);
}

#[test]
fn an_edit_commit_rewrites_only_the_entered_field() {
    // Per-field commits: Enter on the command field writes the
    // base profile with only the command replaced — a buffered
    // but un-Enter'd name edit rides along nowhere.
    let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
    let mut draft = ProfileDraft::edit(0, &profiles[0]);
    draft.name = "garbage-unentered".to_string();
    let ProfileCommitPlan::Write {
        profiles: written,
        default_after_rename,
    } = plan_profile_commit(&draft, ProfileField::Command, "btop", &profiles, None)
    else {
        panic!("expected a write plan")
    };
    assert_eq!(written[0].name, "ssh");
    assert_eq!(written[0].command, "btop");
    assert_eq!(written[1].command, "htop");
    assert_eq!(default_after_rename, None);
}

#[test]
fn a_rename_commit_cascades_only_when_it_hits_the_default() {
    // The paired write: renaming the default-named profile
    // re-points `default_profile` in the same interaction;
    // non-default renames, absent defaults, and a same-name
    // re-commit write nothing extra.
    let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
    let draft = ProfileDraft::edit(0, &profiles[0]);
    let ProfileCommitPlan::Write {
        profiles: renamed,
        default_after_rename,
    } = plan_profile_commit(&draft, ProfileField::Name, "server", &profiles, Some("ssh"))
    else {
        panic!("expected a write plan")
    };
    assert_eq!(renamed[0].name, "server");
    assert_eq!(default_after_rename, Some("server".to_string()));

    let draft = ProfileDraft::edit(0, &profiles[0]);
    let ProfileCommitPlan::Write {
        default_after_rename: non_default,
        ..
    } = plan_profile_commit(&draft, ProfileField::Name, "server", &profiles, Some("top"))
    else {
        panic!("expected a write plan")
    };
    assert_eq!(non_default, None);

    let draft = ProfileDraft::edit(0, &profiles[0]);
    let ProfileCommitPlan::Write {
        default_after_rename: absent,
        ..
    } = plan_profile_commit(&draft, ProfileField::Name, "server", &profiles, None)
    else {
        panic!("expected a write plan")
    };
    assert_eq!(absent, None);

    let draft = ProfileDraft::edit(0, &profiles[0]);
    let ProfileCommitPlan::Write {
        default_after_rename: same_name,
        ..
    } = plan_profile_commit(&draft, ProfileField::Name, "ssh", &profiles, Some("ssh"))
    else {
        panic!("expected a write plan")
    };
    assert_eq!(same_name, None);
}

#[test]
fn an_edit_commit_clears_the_cwd_on_empty_text() {
    let profiles = [sourced_profile()];
    let draft = ProfileDraft::edit(0, &profiles[0]);
    let ProfileCommitPlan::Write {
        profiles: written, ..
    } = plan_profile_commit(&draft, ProfileField::Cwd, "", &profiles, None)
    else {
        panic!("expected a write plan")
    };
    assert_eq!(written[0].cwd, None);
    let ProfileCommitPlan::Write {
        profiles: repathed, ..
    } = plan_profile_commit(&draft, ProfileField::Cwd, "~/src", &profiles, None)
    else {
        panic!("expected a write plan")
    };
    assert_eq!(repathed[0].cwd, Some(PathBuf::from("~/src")));
}

#[test]
fn an_edit_commit_rejects_a_duplicate_name_self_excluded() {
    let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
    let draft = ProfileDraft::edit(0, &profiles[0]);
    assert_eq!(
        plan_profile_commit(&draft, ProfileField::Name, "top", &profiles, None),
        ProfileCommitPlan::Invalid {
            field: ProfileField::Name,
            error: ProfileFieldError::DuplicateName,
        }
    );
    // The profile's own name re-commits cleanly.
    assert!(matches!(
        plan_profile_commit(&draft, ProfileField::Name, "ssh", &profiles, None),
        ProfileCommitPlan::Write { .. }
    ));
}

#[test]
fn a_stale_edit_index_plans_a_drop_not_a_write() {
    // The list shrank under an open edit draft (an external config
    // edit echoed in): the planner reports the staleness — never
    // a panic, never a write.
    let profiles = [profile("ssh", "ssh web-prod")];
    let draft = ProfileDraft {
        mode: ProfileDraftMode::Edit { index: 9 },
        ..ProfileDraft::add()
    };
    assert_eq!(
        plan_profile_commit(&draft, ProfileField::Name, "server", &profiles, None),
        ProfileCommitPlan::StaleEdit
    );
}

#[test]
fn a_focus_loss_hide_preserves_the_profile_draft_and_its_text() {
    // The DrawerTransition rule, page-local half: a hide keeps
    // the open draft and its buffered text for the next show.
    let mut draft = Some(ProfileDraft::add());
    draft.as_mut().unwrap().name = "half-typed".to_string();
    apply_profile_draft_transition(&mut draft, ProfileDraftTransition::Hide);
    let kept = draft.expect("the draft survives the hide");
    assert_eq!(kept.name, "half-typed");
}

#[test]
fn an_explicit_close_or_leaving_the_profiles_tab_clears_the_draft() {
    // One transition, two call sites (the drawer's explicit
    // close, a tab turn away from the Profiles page): both drop
    // the draft — a reopened Profiles page starts from config.
    let mut draft = Some(ProfileDraft::add());
    apply_profile_draft_transition(&mut draft, ProfileDraftTransition::Clear);
    assert!(draft.is_none());
    let mut draft = Some(ProfileDraft::add());
    apply_profile_draft_transition(&mut draft, ProfileDraftTransition::Clear);
    assert!(draft.is_none());
}

// --- row actions: delete + auto-clear, reorder (T4) ---

#[test]
fn a_delete_plan_removes_and_auto_clears_only_the_named_default() {
    // The owner's auto-clear ruling: deleting the profile the
    // key names clears `default_profile` in the same
    // interaction; any other delete writes nothing extra.
    let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
    let (list, paired) =
        plan_profile_delete(&profiles, 0, Some("ssh")).expect("a live index deletes");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "top");
    assert_eq!(paired, Some(ProfileWrite::DefaultProfile(None)));

    let (_, untouched) =
        plan_profile_delete(&profiles, 0, Some("top")).expect("a live index deletes");
    assert_eq!(untouched, None);

    let (_, absent) = plan_profile_delete(&profiles, 0, None).expect("a live index deletes");
    assert_eq!(absent, None);
}

#[test]
fn a_stale_delete_index_plans_nothing() {
    let profiles = [profile("ssh", "ssh web-prod")];
    assert_eq!(plan_profile_delete(&profiles, 9, None), None);
}

#[test]
fn a_reorder_plan_moves_one_slot_or_rejects_the_boundary() {
    // A move with a neighbor plans the swapped list; a boundary
    // press (the disabled direction) plans nothing — no
    // same-value write.
    let profiles = [
        profile("a", "true"),
        profile("b", "true"),
        profile("c", "true"),
    ];
    let moved =
        plan_profile_reorder(&profiles, 1, ReorderDirection::Up).expect("an interior row moves");
    assert_eq!(
        moved.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["b", "a", "c"]
    );
    assert_eq!(
        plan_profile_reorder(&profiles, 0, ReorderDirection::Up),
        None
    );
    assert_eq!(
        plan_profile_reorder(&profiles, 2, ReorderDirection::Down),
        None
    );
}

#[test]
fn the_selection_after_a_delete_drops_or_shifts() {
    // The deleted row's selection drops (its draft with it); a
    // later selection shifts down to keep naming the same
    // profile; earlier selections and none are untouched.
    assert_eq!(selection_after_delete(Some(1), 1), None);
    assert_eq!(selection_after_delete(Some(2), 1), Some(1));
    assert_eq!(selection_after_delete(Some(0), 1), Some(0));
    assert_eq!(selection_after_delete(None, 1), None);
}

#[test]
fn an_edit_draft_after_a_delete_drops_or_shifts() {
    // An edit of the deleted row drops; a later edit shifts;
    // non-edit drafts pass through untouched.
    let later = ProfileDraft::edit(2, &profile("c", "true"));
    let shifted = draft_after_delete(Some(later), 1).expect("a later edit shifts");
    assert_eq!(shifted.mode, ProfileDraftMode::Edit { index: 1 });

    let deleted = ProfileDraft::edit(1, &profile("b", "true"));
    assert_eq!(draft_after_delete(Some(deleted), 1), None);

    let adding = ProfileDraft::add();
    let kept = draft_after_delete(Some(adding.clone()), 1).expect("an add draft survives");
    assert_eq!(kept.mode, adding.mode);
}

#[test]
fn the_selection_after_a_reorder_follows_the_moved_profile() {
    // The swap trades the two involved slots' selections (moving
    // row 1 down swaps slots 1 and 2: each selection follows its
    // profile to the other slot); every other selection is
    // untouched.
    assert_eq!(
        selection_after_move(Some(1), 1, ReorderDirection::Up),
        Some(0)
    );
    assert_eq!(
        selection_after_move(Some(0), 1, ReorderDirection::Up),
        Some(1)
    );
    assert_eq!(
        selection_after_move(Some(2), 1, ReorderDirection::Down),
        Some(1)
    );
    assert_eq!(
        selection_after_move(Some(0), 1, ReorderDirection::Down),
        Some(0)
    );
    assert_eq!(selection_after_move(None, 1, ReorderDirection::Up), None);
}

// --- the transactional save (profile-form-commit PF-1) ---

#[test]
fn a_valid_add_save_plans_one_append_and_no_default_write() {
    // The whole draft lands as one profile on one whole-list write:
    // every field the form exposes — text, scheme, env rows — rides
    // the Save, trimmed through the same rules the per-field commits
    // used. An Add never touches `default_profile`.
    let profiles = [profile("ssh", "ssh web-prod")];
    let mut draft = ProfileDraft::add();
    draft.name = "  monitor ".to_string();
    draft.command = "btop".to_string();
    draft.cwd = "/srv/mon".to_string();
    draft.scheme = Some("Dracula".to_string());
    draft.env_rows = vec![env_row("DROPTERM_PROFILE", "monitor"), env_row("", "")];
    let ProfileSavePlan::Write {
        profiles: written,
        default_after_rename,
    } = plan_profile_save(&draft, &profiles, Some("ssh"))
    else {
        panic!("a valid add saves");
    };
    assert_eq!(written.len(), 2);
    assert_eq!(written[1].name, "monitor");
    assert_eq!(written[1].command, "btop");
    assert_eq!(written[1].cwd, Some(PathBuf::from("/srv/mon")));
    assert_eq!(written[1].color_scheme, Some("Dracula".to_string()));
    assert_eq!(
        written[1].env,
        Some(BTreeMap::from([(
            "DROPTERM_PROFILE".to_string(),
            "monitor".to_string()
        )]))
    );
    assert_eq!(default_after_rename, None);
}

#[test]
fn a_valid_edit_save_replaces_every_field_in_one_write() {
    // The Edit counterpart: the whole draft replaces the profile at
    // its index — no field rides along nowhere, no field needs its
    // own Enter — and every other slot stays byte-identical.
    let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
    let mut draft = ProfileDraft::edit(0, &profiles[0]);
    draft.name = "server".to_string();
    draft.command = "ssh backup".to_string();
    draft.cwd = "/srv/backup".to_string();
    draft.scheme = Some("Dracula".to_string());
    draft.env_rows = vec![env_row("KEY", "v")];
    let ProfileSavePlan::Write {
        profiles: written,
        default_after_rename,
    } = plan_profile_save(&draft, &profiles, None)
    else {
        panic!("a valid edit saves");
    };
    assert_eq!(written.len(), 2);
    assert_eq!(written[0].name, "server");
    assert_eq!(written[0].command, "ssh backup");
    assert_eq!(written[0].cwd, Some(PathBuf::from("/srv/backup")));
    assert_eq!(written[0].color_scheme, Some("Dracula".to_string()));
    assert_eq!(
        written[0].env,
        Some(BTreeMap::from([("KEY".to_string(), "v".to_string())]))
    );
    assert_eq!(written[1].command, "htop");
    assert_eq!(default_after_rename, None);
}

#[test]
fn an_edit_save_cascades_only_when_the_rename_hits_the_default() {
    // One Save, at most one paired default write: present iff the
    // draft renamed the default-named profile (the once-per-Save
    // rule the per-field cascade already followed); a non-default
    // rename or an absent default writes nothing extra.
    let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
    let mut draft = ProfileDraft::edit(0, &profiles[0]);
    draft.name = "server".to_string();
    let ProfileSavePlan::Write {
        default_after_rename: hit,
        ..
    } = plan_profile_save(&draft, &profiles, Some("ssh"))
    else {
        panic!("a valid rename saves");
    };
    assert_eq!(hit, Some("server".to_string()));

    let ProfileSavePlan::Write {
        default_after_rename: missed,
        ..
    } = plan_profile_save(&draft, &profiles, Some("top"))
    else {
        panic!("a valid rename saves");
    };
    assert_eq!(missed, None);

    let ProfileSavePlan::Write {
        default_after_rename: absent,
        ..
    } = plan_profile_save(&draft, &profiles, None)
    else {
        panic!("a valid rename saves");
    };
    assert_eq!(absent, None);
}

#[test]
fn an_invalid_save_names_every_offending_field_at_once() {
    // The transactional gate: every rule evaluates at Save, and
    // every offending field surfaces in one plan — the form shows
    // them all simultaneously, each under its own input. A wholly
    // blank env row never rejects (the write drops it).
    let profiles = [profile("ssh", "ssh web-prod")];
    let mut draft = ProfileDraft::add();
    draft.name = "  ".to_string();
    draft.command = "   ".to_string();
    draft.env_rows = vec![env_row("A=B", "v"), env_row("", ""), env_row("", "orphan")];
    assert_eq!(
        plan_profile_save(&draft, &profiles, None),
        ProfileSavePlan::Invalid {
            errors: vec![
                (ProfileField::Name, ProfileFieldError::BlankName),
                (ProfileField::Command, ProfileFieldError::BlankCommand),
                (ProfileField::EnvKey(0), ProfileFieldError::EnvKeyEquals),
                (ProfileField::EnvKey(2), ProfileFieldError::BlankEnvKey),
            ],
        }
    );
}

#[test]
fn a_duplicate_name_errors_alongside_its_fellow_offenders() {
    // The rule set is unchanged, only its evaluation point: a
    // duplicate name still rejects — now shoulder to shoulder with
    // every other bad field, not first-past-the-post.
    let profiles = [profile("ssh", "ssh web-prod")];
    let mut draft = ProfileDraft::add();
    draft.name = "ssh".to_string();
    draft.command = "btop".to_string();
    draft.env_rows = vec![env_row("", "orphan")];
    assert_eq!(
        plan_profile_save(&draft, &profiles, None),
        ProfileSavePlan::Invalid {
            errors: vec![
                (ProfileField::Name, ProfileFieldError::DuplicateName),
                (ProfileField::EnvKey(0), ProfileFieldError::BlankEnvKey),
            ],
        }
    );
}

#[test]
fn an_unchanged_edit_save_plans_no_write() {
    // The anti-churn idiom (`echo_retitles_tabs`'s): a seeded,
    // untouched Edit draft compares equal against the committed
    // profile — the Save is an empty transaction, not a same-value
    // write. Seeding round-trips every field, so equality is exact.
    let profiles = [sourced_profile()];
    let draft = ProfileDraft::edit(0, &profiles[0]);
    assert_eq!(
        plan_profile_save(&draft, &profiles, Some("web-prod")),
        ProfileSavePlan::Unchanged
    );
}

#[test]
fn a_stale_edit_save_plans_a_drop() {
    // The list shrank under the open draft: the Save reports the
    // staleness — never a panic, never a write.
    let profiles = [profile("ssh", "ssh web-prod")];
    let draft = ProfileDraft {
        mode: ProfileDraftMode::Edit { index: 9 },
        ..ProfileDraft::add()
    };
    assert_eq!(
        plan_profile_save(&draft, &profiles, None),
        ProfileSavePlan::StaleEdit
    );
}

#[test]
fn a_keystroke_clears_only_its_own_field_s_error() {
    // The stale-error rule, field-scoped: typing re-validates
    // nothing, it only retires the error that described the text
    // being replaced — the other fields' errors describe text that
    // still exists and stay.
    let mut draft = ProfileDraft::add();
    draft.errors = vec![
        (ProfileField::Name, ProfileFieldError::DuplicateName),
        (ProfileField::Command, ProfileFieldError::BlankCommand),
    ];
    draft.set_field_text(ProfileField::Name, "server".to_string());
    assert_eq!(
        draft.errors,
        vec![(ProfileField::Command, ProfileFieldError::BlankCommand)]
    );
    assert_eq!(draft.name, "server");
}

#[test]
fn selection_after_save_follows_the_written_row() {
    // Save closes the form on the row it wrote: an Edit keeps the
    // row it rewrote; an Add/Clone selects the appended row —
    // `profiles_len` is the pre-append length, the Save's own +1.
    assert_eq!(
        selection_after_save(&ProfileDraftMode::Edit { index: 1 }, 3),
        1
    );
    assert_eq!(selection_after_save(&ProfileDraftMode::Add, 3), 3);
    assert_eq!(
        selection_after_save(
            &ProfileDraftMode::Clone {
                source: sourced_profile()
            },
            3
        ),
        3
    );
}
