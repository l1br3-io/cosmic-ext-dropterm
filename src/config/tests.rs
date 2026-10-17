use super::*;

#[test]
fn defaults_are_sane() {
    let config = DropTermConfig::default();
    assert_eq!(config.height_percent, DEFAULT_HEIGHT_PERCENT);
    assert_eq!(config.width_percent, DEFAULT_WIDTH_PERCENT);
    assert_eq!(config.h_align, HAlign::Center);
    assert_eq!(config.edge, Edge::Top);
    assert_eq!(config.layer, Layer::Overlay);
    assert_eq!(config.keyboard_mode, KeyboardMode::Exclusive);
    assert!(config.output.is_none());
    assert!(config.shell.is_none());
    assert_eq!(config.opacity, 100);
    assert!(config.hide_on_focus_loss);
    assert!(!config.respect_panel);
    // Never dragged: the drawer renders its built-in default
    // width (drawer-resize spec criterion 1) until the first
    // drag release stores a px value.
    assert!(config.settings_drawer_width.is_none());
}

#[test]
fn session_and_slide_toggles_default_on() {
    // Both keys ship `true` — the default is the always-on
    // behavior, so existing configs keep it.
    let config = DropTermConfig::default();
    assert!(config.session_persistence_enabled);
    assert!(config.slide_animation_enabled);
}

#[test]
fn bell_notifications_default_to_on() {
    // The one DE feature a hidden drop-down uniquely needs ships
    // enabled; the Behavior toggle turns it off.
    assert!(DropTermConfig::default().bell_notification_enabled);
}

#[test]
fn new_tabs_after_current_default_on() {
    // The shipped behavior — insert after the active tab — is the
    // default, so existing users keep it; turning the toggle off
    // appends at the end instead (deliberately the inverse of
    // Guake `new-tab-after`, whose default is at-end).
    assert!(DropTermConfig::default().new_tab_after_current);
}

#[test]
fn missing_new_tab_after_current_key_loads_as_true() {
    // A config directory from a version that did not yet write
    // `new_tab_after_current` loads as `true` through the same
    // default-entry leniency pinned for `respect_panel` — the
    // key list is additive, `CONFIG_VERSION` stays 1.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-newtabafter-key-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    // Simulate the pre-`new_tab_after_current` version by deleting
    // the one key it never wrote.
    std::fs::remove_file(
        root.join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"))
            .join("new_tab_after_current"),
    )
    .expect("remove new_tab_after_current key file");

    let loaded =
        DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
    assert!(loaded.new_tab_after_current);
    // The directory is live, not ignored: the written key comes back.
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn new_tab_after_current_key_on_disk_is_honored() {
    // Control for `missing_new_tab_after_current_key_loads_as_true`:
    // the same read path honors a key that does exist, so the
    // leniency result above cannot be a vacuous pass.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-present-newtabafter-key-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        new_tab_after_current: false,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert!(!loaded.new_tab_after_current);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn close_confirmation_defaults_to_on() {
    // Close-confirm criterion 1: the guardrail ships enabled — a
    // fresh config never lets a close misfire kill a foreground
    // vim/ssh/pipeline unasked. The Behavior toggle turns it off.
    assert!(DropTermConfig::default().close_confirmation_enabled);
}

#[test]
fn missing_close_confirmation_key_loads_as_true() {
    // A config directory from a version that did not yet write
    // `close_confirmation_enabled` loads as `true` through the
    // same default-entry leniency pinned for
    // `new_tab_after_current` — existing users get the guardrail
    // on (criterion 1); the key list is additive,
    // `CONFIG_VERSION` stays 1.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-closeconfirm-key-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    // Simulate the pre-`close_confirmation_enabled` version by
    // deleting the one key it never wrote.
    std::fs::remove_file(
        root.join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"))
            .join("close_confirmation_enabled"),
    )
    .expect("remove close_confirmation_enabled key file");

    let loaded =
        DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
    assert!(loaded.close_confirmation_enabled);
    // The directory is live, not ignored: the written key comes back.
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn close_confirmation_key_on_disk_is_honored() {
    // Control for `missing_close_confirmation_key_loads_as_true`:
    // the same read path honors a key that does exist (the toggle
    // off persists), so the leniency result above cannot be a
    // vacuous pass.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-present-closeconfirm-key-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        close_confirmation_enabled: false,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert!(!loaded.close_confirmation_enabled);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn drawer_click_close_defaults_to_on() {
    // Drawer-click-close criterion 5: the gesture ships enabled —
    // a fresh config dismisses the drawer on a terminal click
    // without any opt-in. The Behavior toggle turns it off.
    assert!(DropTermConfig::default().settings_drawer_click_close);
}

#[test]
fn missing_settings_drawer_click_close_key_loads_as_true() {
    // A config directory from a version that did not yet write
    // `settings_drawer_click_close` loads as `true` through the
    // same default-entry leniency pinned for
    // `close_confirmation_enabled` — existing users get the
    // gesture on (criterion 5); the key list is additive,
    // `CONFIG_VERSION` stays 1.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-drawerclick-key-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    // Simulate the pre-`settings_drawer_click_close` version by
    // deleting the one key it never wrote.
    std::fs::remove_file(
        root.join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"))
            .join("settings_drawer_click_close"),
    )
    .expect("remove settings_drawer_click_close key file");

    let loaded =
        DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
    assert!(loaded.settings_drawer_click_close);
    // The directory is live, not ignored: the written key comes back.
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn settings_drawer_click_close_key_on_disk_is_honored() {
    // Control for
    // `missing_settings_drawer_click_close_key_loads_as_true`: the
    // same read path honors a key that does exist (the toggle off
    // persists), so the leniency result above cannot be a vacuous
    // pass.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-present-drawerclick-key-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        settings_drawer_click_close: false,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert!(!loaded.settings_drawer_click_close);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn missing_profile_keys_load_as_empty_and_unset() {
    // A config directory from a version that did not yet write
    // `profiles` or `default_profile` loads as "no profiles, no
    // default" — byte-identical to today's behavior — through the
    // same default-entry leniency pinned for the other additive
    // keys; the key list grows, `CONFIG_VERSION` stays 1.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-profile-keys-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    // Simulate the pre-profile version by deleting the two keys
    // it never wrote.
    let version_dir = root
        .join("cosmic")
        .join("dropterm-config-test")
        .join(format!("v{CONFIG_VERSION}"));
    std::fs::remove_file(version_dir.join("profiles")).expect("remove profiles key file");
    std::fs::remove_file(version_dir.join("default_profile"))
        .expect("remove default_profile key file");

    let loaded =
        DropTermConfig::get_entry(&config).expect("profile-less v1 config loads without error");
    assert!(
        loaded.profiles.is_empty(),
        "not empty: {:?}",
        loaded.profiles
    );
    assert_eq!(loaded.default_profile, None);
    // The directory is live, not ignored: the written key comes back.
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn profile_keys_round_trip_through_the_config_store() {
    // Two distinct profiles — one with a cwd, one without — plus
    // a set default survive a write→load cycle with equality.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-profile-roundtrip-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    let stored = DropTermConfig {
        profiles: vec![
            Profile {
                name: "web-prod".to_string(),
                command: "ssh web-prod".to_string(),
                cwd: Some(std::path::PathBuf::from("/srv/web")),
                color_scheme: None,
                env: None,
                font_size: None,
                scrollback_lines: None,
            },
            Profile {
                name: "top".to_string(),
                command: "htop".to_string(),
                cwd: None,
                color_scheme: None,
                env: None,
                font_size: None,
                scrollback_lines: None,
            },
        ],
        default_profile: Some("web-prod".to_string()),
        ..DropTermConfig::default()
    };
    stored.write_entry(&config).expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert_eq!(loaded, stored);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn profile_color_scheme_round_trips_through_the_config_store() {
    // A set scheme and an unset one — the two shapes a mixed
    // profile list writes — survive a write→load cycle with
    // equality, riding the existing `profiles` key unchanged.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-profile-scheme-roundtrip-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    let stored = DropTermConfig {
        profiles: vec![
            Profile {
                name: "web-prod".to_string(),
                command: "ssh web-prod".to_string(),
                cwd: None,
                color_scheme: Some("Dracula".to_string()),
                env: None,
                font_size: None,
                scrollback_lines: None,
            },
            Profile {
                name: "top".to_string(),
                command: "htop".to_string(),
                cwd: None,
                color_scheme: None,
                env: None,
                font_size: None,
                scrollback_lines: None,
            },
        ],
        ..DropTermConfig::default()
    };
    stored.write_entry(&config).expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert_eq!(loaded, stored);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn profile_env_round_trips_through_the_config_store() {
    // A set env map and an unset one — the two shapes a mixed
    // profile list writes — survive a write→load cycle with
    // equality, riding the existing `profiles` key unchanged.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-profile-env-roundtrip-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    let stored = DropTermConfig {
        profiles: vec![
            Profile {
                name: "ssh-prod".to_string(),
                command: "ssh web-prod".to_string(),
                cwd: None,
                color_scheme: None,
                env: Some(BTreeMap::from([
                    ("DROPTERM_PROFILE".to_string(), "prod".to_string()),
                    ("TERM_PROGRAM".to_string(), "dropterm-ssh".to_string()),
                ])),
                font_size: None,
                scrollback_lines: None,
            },
            Profile {
                name: "top".to_string(),
                command: "htop".to_string(),
                cwd: None,
                color_scheme: None,
                env: None,
                font_size: None,
                scrollback_lines: None,
            },
        ],
        ..DropTermConfig::default()
    };
    stored.write_entry(&config).expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert_eq!(loaded, stored);

    // The map serializes in BTree order — the deterministic-
    // round-trip property that made the field a BTreeMap (the
    // `custom_key_binds` precedent): the same profile always
    // writes the same bytes, so the store never churns.
    let version_dir = root
        .join("cosmic")
        .join("dropterm-config-test")
        .join(format!("v{CONFIG_VERSION}"));
    let text =
        std::fs::read_to_string(version_dir.join("profiles")).expect("profiles key still on disk");
    let first = text.find("DROPTERM_PROFILE").expect("first key present");
    let second = text.find("TERM_PROGRAM").expect("second key present");
    assert!(first < second, "env keys serialize in BTree order: {text}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn new_tab_cwd_defaults_to_inherit_with_no_default_directory() {
    // Default-spawn ruling 2: `Inherit` is today's chain
    // byte-identical, and no default directory is configured — a
    // fresh config never moves a new tab's cwd.
    assert_eq!(DropTermConfig::default().new_tab_cwd, NewTabCwd::Inherit);
    assert_eq!(DropTermConfig::default().default_cwd, None);
}

#[test]
fn new_tab_cwd_serde_round_trips_all_variants() {
    // The enum must survive the serde path cosmic-config uses for
    // key values; every variant, not just the default.
    for variant in [NewTabCwd::Inherit, NewTabCwd::Default] {
        let serialized = ron::to_string(&variant).expect("serialize NewTabCwd");
        let deserialized: NewTabCwd = ron::from_str(&serialized).expect("deserialize NewTabCwd");
        assert_eq!(deserialized, variant);
    }
}

#[test]
fn missing_default_spawn_keys_load_as_inherit_and_unset() {
    // A config directory from a version that did not yet write
    // `new_tab_cwd` or `default_cwd` loads as "inherit, unset" —
    // byte-identical to today's plain-spawn chain — through the
    // same default-entry leniency pinned for the profile keys;
    // the key list is additive, `CONFIG_VERSION` stays 1.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-default-spawn-keys-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    // Simulate the pre-`default-spawn` version by deleting the
    // two keys it never wrote.
    let version_dir = root
        .join("cosmic")
        .join("dropterm-config-test")
        .join(format!("v{CONFIG_VERSION}"));
    std::fs::remove_file(version_dir.join("new_tab_cwd")).expect("remove new_tab_cwd key file");
    std::fs::remove_file(version_dir.join("default_cwd")).expect("remove default_cwd key file");

    let loaded = DropTermConfig::get_entry(&config)
        .expect("default-spawn-less v1 config loads without error");
    assert_eq!(loaded.new_tab_cwd, NewTabCwd::Inherit);
    assert_eq!(loaded.default_cwd, None);
    // The directory is live, not ignored: the written key comes back.
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn default_spawn_keys_round_trip_through_the_config_store() {
    // Default mode plus a set directory survive a write→load cycle
    // with equality — the `default_profile` precedent.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-default-spawn-roundtrip-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    let stored = DropTermConfig {
        new_tab_cwd: NewTabCwd::Default,
        default_cwd: Some(std::path::PathBuf::from("/srv/work")),
        ..DropTermConfig::default()
    };
    stored.write_entry(&config).expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert_eq!(loaded, stored);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_pre_initiative_profiles_file_loads_with_the_scheme_unset() {
    // The version that shipped before `color_scheme` existed
    // wrote profile entries without the field. Seeding exactly
    // that shape and loading must succeed with the scheme unset
    // — and must leave the file byte-identical: the store is
    // never rewritten to "fix" an older format.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-pre-initiative-profiles-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig::default()
        .write_entry(&config)
        .expect("write config keys");

    let version_dir = root
        .join("cosmic")
        .join("dropterm-config-test")
        .join(format!("v{CONFIG_VERSION}"));
    let profiles_key = version_dir.join("profiles");
    // The pre-initiative shape: a plain Vec<Profile> with no
    // `color_scheme` field anywhere (cwd spelled out so nothing
    // rides serde defaults but the new field itself).
    let pre_initiative = "[\n    (\n        name: \"ssh\",\n        command: \"ssh web\",\n        cwd: None,\n    ),\n]";
    std::fs::write(&profiles_key, pre_initiative).expect("seed pre-initiative profiles key file");

    let loaded = DropTermConfig::get_entry(&config).expect("pre-initiative profiles file loads");
    assert_eq!(loaded.profiles.len(), 1);
    assert_eq!(loaded.profiles[0].name, "ssh");
    assert_eq!(loaded.profiles[0].color_scheme, None);
    // The same leniency covers every later additive field: `env`
    // loads unset when the key never existed.
    assert_eq!(loaded.profiles[0].env, None);

    // Byte-stable: the load rewrote nothing.
    assert_eq!(
        std::fs::read_to_string(&profiles_key).expect("profiles key still on disk"),
        pre_initiative
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn custom_key_binds_round_trip_through_the_config_store() {
    // Two entries — an ordinary rebind with modifiers and a
    // numbered activate row without — survive a write→load cycle
    // with equality. The key file's name is the field's
    // file-format spelling; the Binding shape is the rest of the
    // contract.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-keybinds-roundtrip-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    let stored = DropTermConfig {
        custom_key_binds: BTreeMap::from([
            (
                "copy".to_string(),
                crate::keybinds::Binding {
                    modifiers: vec![
                        crate::keybinds::ModifierName::Ctrl,
                        crate::keybinds::ModifierName::Shift,
                    ],
                    key: "C".to_string(),
                },
            ),
            (
                "activate_tab_3".to_string(),
                crate::keybinds::Binding {
                    modifiers: Vec::new(),
                    key: "F2".to_string(),
                },
            ),
        ]),
        ..DropTermConfig::default()
    };
    stored.write_entry(&config).expect("write config keys");
    let version_dir = root
        .join("cosmic")
        .join("dropterm-config-test")
        .join(format!("v{CONFIG_VERSION}"));
    assert!(
        version_dir.join("custom_key_binds").is_file(),
        "the key file's name is the field's on-disk spelling"
    );

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert_eq!(loaded, stored);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn missing_custom_key_binds_loads_as_the_empty_map() {
    // A config directory from a version that did not yet write
    // `custom_key_binds` loads as "all defaults" — the same
    // default-entry leniency as every additive key before it.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-keybinds-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    let version_dir = root
        .join("cosmic")
        .join("dropterm-config-test")
        .join(format!("v{CONFIG_VERSION}"));
    std::fs::remove_file(version_dir.join("custom_key_binds"))
        .expect("remove custom_key_binds key file");

    let loaded = DropTermConfig::get_entry(&config).expect("keybinds-less v1 config loads clean");
    assert!(loaded.custom_key_binds.is_empty());
    // The directory is live, not ignored: the written key comes back.
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn keybinds_write_persists_the_whole_map_through_the_store() {
    // The settings module's whole-map write lands as one `set` of
    // the `custom_key_binds` key — the ProfileWrite precedent —
    // and the next load reads exactly the written map back.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-keybinds-write-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    let write = crate::settings::KeybindsWrite(BTreeMap::from([(
        "copy".to_string(),
        crate::keybinds::Binding {
            modifiers: vec![crate::keybinds::ModifierName::Alt],
            key: "C".to_string(),
        },
    )]));
    write.persist(&config).expect("persist the whole map");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert_eq!(loaded.custom_key_binds, write.0);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn config_version_stays_at_one() {
    // New boolean keys merge into pre-existing files through the
    // CosmicConfigEntry default-entry fallback — no version bump,
    // no per-field serde attributes.
    assert_eq!(CONFIG_VERSION, 1);
}

#[test]
fn height_percent_is_clamped_into_range() {
    let mut config = DropTermConfig {
        height_percent: 0,
        ..DropTermConfig::default()
    };
    config = config.clamped();
    assert_eq!(config.height_percent, MIN_HEIGHT_PERCENT);

    let mut config = DropTermConfig {
        height_percent: 255,
        ..DropTermConfig::default()
    };
    config = config.clamped();
    assert_eq!(config.height_percent, MAX_HEIGHT_PERCENT);

    let mut config = DropTermConfig {
        height_percent: 60,
        ..DropTermConfig::default()
    };
    config = config.clamped();
    assert_eq!(config.height_percent, 60);
}

#[test]
fn width_percent_is_clamped_into_range() {
    let mut config = DropTermConfig {
        width_percent: 0,
        ..DropTermConfig::default()
    };
    config = config.clamped();
    assert_eq!(config.width_percent, MIN_WIDTH_PERCENT);

    let mut config = DropTermConfig {
        width_percent: 255,
        ..DropTermConfig::default()
    };
    config = config.clamped();
    assert_eq!(config.width_percent, MAX_WIDTH_PERCENT);

    let mut config = DropTermConfig {
        width_percent: 60,
        ..DropTermConfig::default()
    };
    config = config.clamped();
    assert_eq!(config.width_percent, 60);
}

#[test]
fn opacity_is_clamped_to_100() {
    let config = DropTermConfig {
        opacity: 255,
        ..DropTermConfig::default()
    }
    .clamped();
    assert_eq!(config.opacity, 100);
}

#[test]
fn shell_falls_back_to_env_then_bin_sh() {
    // The chain keeps only values whose program can exec today — a
    // dead configured or env shell skips to the next leg instead of
    // arming every spawn to fail. Every SHELL mutation lives in this
    // one test so the asserts stay sequenced (tests run in parallel
    // threads).
    let configured = DropTermConfig {
        shell: Some("/bin/sh".to_string()),
        ..DropTermConfig::default()
    };
    assert_eq!(configured.resolved_shell(), "/bin/sh");

    let dead_configured = DropTermConfig {
        shell: Some("/dropterm-not-a-real-shell".to_string()),
        ..DropTermConfig::default()
    };

    // SAFETY: single-threaded test, no other test reads/writes SHELL.
    unsafe {
        std::env::set_var("SHELL", "/bin/sh");
    }
    assert_eq!(DropTermConfig::default().resolved_shell(), "/bin/sh");
    assert_eq!(dead_configured.resolved_shell(), "/bin/sh");

    // SAFETY: single-threaded test, no other test reads/writes SHELL.
    unsafe {
        std::env::set_var("SHELL", "/dropterm-not-a-real-shell");
    }
    assert_eq!(dead_configured.resolved_shell(), "/bin/sh");

    // SAFETY: single-threaded test, no other test reads/writes SHELL.
    unsafe {
        std::env::remove_var("SHELL");
    }
    let no_env = DropTermConfig::default();
    assert_eq!(no_env.resolved_shell(), "/bin/sh");
}

#[test]
fn program_is_runnable_checks_a_path_directly() {
    // `/bin/sh` is POSIX-guaranteed present and executable.
    assert!(program_is_runnable("/bin/sh"));
    assert!(!program_is_runnable("/dropterm-not-a-real-program"));
    // No program at all is never runnable — an exec of "" dies.
    assert!(!program_is_runnable(""));
}

#[test]
fn program_is_runnable_requires_an_execute_bit() {
    use std::os::unix::fs::PermissionsExt;

    let path = std::env::temp_dir().join(format!(
        "dropterm-runnable-execute-bit-{}",
        std::process::id()
    ));
    std::fs::write(&path, b"#!/bin/sh\n").expect("write the test program");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
        .expect("drop the exec bit");
    assert!(!program_is_runnable(
        path.to_str().expect("utf-8 temp path")
    ));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("set the exec bit");
    assert!(program_is_runnable(path.to_str().expect("utf-8 temp path")));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn program_is_runnable_scans_path_for_bare_names() {
    assert!(program_is_runnable("sh"));
    assert!(!program_is_runnable("dropterm-not-a-real-program"));
}

#[test]
fn missing_respect_panel_key_loads_as_false() {
    // A config directory from a version that did not yet write
    // `respect_panel`: every legacy key on disk, no `respect_panel`
    // file. cosmic-config reads
    // each key independently and leaves missing keys at their default
    // (`Error::is_err` treats NotFound/NoConfigDirectory as lenient).
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-key-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    // Simulate the pre-`respect_panel` version by deleting the one key
    // it never wrote. On-disk layout is <root>/cosmic/<name>/v<version>
    // (cosmic-config rev 08a09e3, `Config::with_custom_path`).
    std::fs::remove_file(
        root.join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"))
            .join("respect_panel"),
    )
    .expect("remove respect_panel key file");

    let loaded =
        DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
    assert!(!loaded.respect_panel);
    // The directory is live, not ignored: the written key comes back.
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn respect_panel_key_on_disk_is_honored() {
    // Control for `missing_respect_panel_key_loads_as_false`: the same
    // read path honors a key that does exist, so the leniency result
    // above cannot be a vacuous pass.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-present-key-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        respect_panel: true,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert!(loaded.respect_panel);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn missing_status_bar_key_loads_as_true() {
    // A config directory from a version before the status band: no
    // `status_bar_enabled` key file. The entry derive backfills
    // from the struct Default — which is `true`, the
    // visible-by-default ruling (OQ-1) — so pre-band configs see
    // the band, not a silently-off key.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-status-bar-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    std::fs::remove_file(
        root.join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"))
            .join("status_bar_enabled"),
    )
    .expect("remove status_bar_enabled key file");

    let loaded =
        DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
    assert!(loaded.status_bar_enabled);
    // The directory is live, not ignored: the written key comes back.
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_disabled_status_bar_round_trips_through_the_config_store() {
    // The off state persists — a hidden band stays hidden across
    // a write→load cycle (the control for the missing-key pin: a
    // present `false` key is honored, not defaulted over).
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-status-bar-off-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    let stored = DropTermConfig {
        status_bar_enabled: false,
        ..DropTermConfig::default()
    };
    stored.write_entry(&config).expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert_eq!(loaded, stored);
    assert!(!loaded.status_bar_enabled);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn missing_autosuggestions_key_loads_as_true() {
    // A config directory from a version before autosuggestions: no
    // `autosuggestions_enabled` key file. The entry derive backfills
    // from the struct Default — `true`, the default-on ruling — so
    // pre-existing configs see ghost text, not a silently-off key.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-autosuggest-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    std::fs::remove_file(
        root.join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"))
            .join("autosuggestions_enabled"),
    )
    .expect("remove autosuggestions_enabled key file");

    let loaded =
        DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
    assert!(loaded.autosuggestions_enabled);
    // The directory is live, not ignored: the written key comes back.
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_disabled_autosuggestions_round_trips_through_the_config_store() {
    // The off state persists — the kill switch stays off across a
    // write→load cycle (the control for the missing-key pin: a
    // present `false` key is honored, not defaulted over).
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-autosuggest-off-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    let stored = DropTermConfig {
        autosuggestions_enabled: false,
        ..DropTermConfig::default()
    };
    stored.write_entry(&config).expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert_eq!(loaded, stored);
    assert!(!loaded.autosuggestions_enabled);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn missing_show_keys_load_as_shown() {
    // A config directory from before the per-segment toggles
    // (status-segments spec): no `status_bar_show_*` key files.
    // The entry derive backfills from the struct Default — all
    // eight `true` — so an old config sees the full band, not a
    // silently hollowed one.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-show-keys-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig::default()
        .write_entry(&config)
        .expect("write config keys");
    let keys = root
        .join("cosmic")
        .join("dropterm-config-test")
        .join(format!("v{CONFIG_VERSION}"));
    for key in [
        "status_bar_show_process",
        "status_bar_show_path",
        "status_bar_show_branch",
        "status_bar_show_profile",
        "status_bar_show_grid",
        "status_bar_show_tab",
        "status_bar_show_zoom",
        "status_bar_show_clock",
    ] {
        std::fs::remove_file(keys.join(key))
            .unwrap_or_else(|error| panic!("remove {key} key file: {error}"));
    }

    let loaded = DropTermConfig::get_entry(&config).expect("key-less config loads");
    assert!(loaded.status_bar_show_process);
    assert!(loaded.status_bar_show_branch);
    assert!(loaded.status_bar_show_clock);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tab_bar_border_defaults_on() {
    // The border ships visible — the default-on ruling (the
    // `status_bar_enabled` precedent: the feature is the point, so
    // the default shows it).
    assert!(DropTermConfig::default().tab_bar_border);
}

#[test]
fn status_bar_border_defaults_on() {
    // The band's twin of the strip's key: both borders ship
    // visible, one ruling for both bars.
    assert!(DropTermConfig::default().status_bar_border);
}

#[test]
fn missing_tab_bar_border_key_loads_as_true() {
    // A config directory from a version before the border keys: no
    // `tab_bar_border` key file. The entry derive backfills from the
    // struct Default — `true` — so pre-border configs gain the
    // hairline, never a silently-off key (the additive-load
    // contract, `missing_status_bar_key_loads_as_true`'s twin).
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-tab-border-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    std::fs::remove_file(
        root.join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"))
            .join("tab_bar_border"),
    )
    .expect("remove tab_bar_border key file");

    let loaded =
        DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
    assert!(loaded.tab_bar_border);
    // The directory is live, not ignored: the written key comes back.
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn missing_status_bar_border_key_loads_as_true() {
    // The band's twin of the strip's missing-key pin: a pre-border
    // config directory gains the band's hairline through the same
    // additive backfill.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-band-border-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    std::fs::remove_file(
        root.join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"))
            .join("status_bar_border"),
    )
    .expect("remove status_bar_border key file");

    let loaded =
        DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
    assert!(loaded.status_bar_border);
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn hidden_segments_round_trip_through_the_config_store() {
    // Every off state persists across a write→load cycle (the
    // control for the missing-key pin: present `false` keys are
    // honored, not defaulted over) — and the fold reads them
    // back as the resolver's all-off toggle set.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-show-keys-off-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    let stored = DropTermConfig {
        status_bar_show_process: false,
        status_bar_show_path: false,
        status_bar_show_branch: false,
        status_bar_show_profile: false,
        status_bar_show_grid: false,
        status_bar_show_tab: false,
        status_bar_show_zoom: false,
        status_bar_show_clock: false,
        ..DropTermConfig::default()
    };
    stored.write_entry(&config).expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert_eq!(loaded, stored);
    let toggles = loaded.status_toggles();
    assert!(
        !toggles.process
            && !toggles.path
            && !toggles.branch
            && !toggles.profile
            && !toggles.grid
            && !toggles.tab
            && !toggles.zoom
            && !toggles.clock
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn theme_preference_defaults_to_system() {
    // The default is Follow system — a fresh config never issues
    // a theme override.
    assert_eq!(
        DropTermConfig::default().theme_preference,
        ThemePreference::System
    );
}

#[test]
fn theme_preference_serde_round_trips_all_variants() {
    // The enum must survive the serde path cosmic-config uses for
    // key values; every variant, not just the default.
    for variant in [
        ThemePreference::System,
        ThemePreference::Dark,
        ThemePreference::Light,
    ] {
        let serialized = ron::to_string(&variant).expect("serialize ThemePreference");
        let deserialized: ThemePreference =
            ron::from_str(&serialized).expect("deserialize ThemePreference");
        assert_eq!(deserialized, variant);
    }
}

#[test]
fn missing_theme_preference_key_loads_as_system() {
    // A config directory from a version that did not yet write
    // `theme_preference` loads as `System` through the same
    // default-entry leniency pinned for `respect_panel` — the
    // key list is additive, `CONFIG_VERSION` stays 1.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-theme-key-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    // Simulate the pre-`theme_preference` version by deleting the
    // one key it never wrote.
    std::fs::remove_file(
        root.join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"))
            .join("theme_preference"),
    )
    .expect("remove theme_preference key file");

    let loaded =
        DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
    assert_eq!(loaded.theme_preference, ThemePreference::System);
    // The directory is live, not ignored: the written key comes back.
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn theme_preference_key_on_disk_is_honored() {
    // Control for `missing_theme_preference_key_loads_as_system`:
    // the same read path honors a key that does exist, so the
    // leniency result above cannot be a vacuous pass.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-present-theme-key-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        theme_preference: ThemePreference::Dark,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert_eq!(loaded.theme_preference, ThemePreference::Dark);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tab_position_defaults_to_auto() {
    // `Auto` keeps the strip at the leading edge beside the grab
    // bar on both anchors.
    assert_eq!(DropTermConfig::default().tab_position, TabPosition::Auto);
}

#[test]
fn tab_position_serde_round_trips_all_variants() {
    // The enum must survive the serde path cosmic-config uses for
    // key values; every variant, not just the default.
    for variant in [TabPosition::Auto, TabPosition::Top, TabPosition::Bottom] {
        let serialized = ron::to_string(&variant).expect("serialize TabPosition");
        let deserialized: TabPosition =
            ron::from_str(&serialized).expect("deserialize TabPosition");
        assert_eq!(deserialized, variant);
    }
}

#[test]
fn missing_tab_position_key_loads_as_auto() {
    // A config directory from a version that did not yet write
    // `tab_position` loads as `Auto` through the same
    // default-entry leniency pinned for `respect_panel` — the
    // key list is additive, `CONFIG_VERSION` stays 1.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-tabpos-key-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    // Simulate the pre-`tab_position` version by deleting the one
    // key it never wrote.
    std::fs::remove_file(
        root.join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"))
            .join("tab_position"),
    )
    .expect("remove tab_position key file");

    let loaded =
        DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
    assert_eq!(loaded.tab_position, TabPosition::Auto);
    // The directory is live, not ignored: the written key comes back.
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tab_title_mode_defaults_to_shell() {
    // `Shell` is today's behavior: OSC 0/2 reports land verbatim,
    // so a config without the key never changes a title.
    assert_eq!(
        DropTermConfig::default().tab_title_mode,
        TabTitleMode::Shell
    );
}

#[test]
fn tab_title_mode_serde_round_trips_all_variants() {
    // The enum must survive the serde path cosmic-config uses for
    // key values; every variant, not just the default.
    for variant in [
        TabTitleMode::Shell,
        TabTitleMode::AbbreviatedPath,
        TabTitleMode::LastSegment,
        TabTitleMode::Numbered,
        TabTitleMode::Profile,
    ] {
        let serialized = ron::to_string(&variant).expect("serialize TabTitleMode");
        let deserialized: TabTitleMode =
            ron::from_str(&serialized).expect("deserialize TabTitleMode");
        assert_eq!(deserialized, variant);
    }
}

#[test]
fn missing_tab_title_mode_key_loads_as_shell() {
    // A config directory from a version that did not yet write
    // `tab_title_mode` loads as `Shell` through the same
    // default-entry leniency pinned for `respect_panel` — the
    // key list is additive, `CONFIG_VERSION` stays 1.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-titlemode-key-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    // Simulate the pre-`tab_title_mode` version by deleting the
    // one key it never wrote.
    std::fs::remove_file(
        root.join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"))
            .join("tab_title_mode"),
    )
    .expect("remove tab_title_mode key file");

    let loaded =
        DropTermConfig::get_entry(&config).expect("key-less v1 config loads without error");
    assert_eq!(loaded.tab_title_mode, TabTitleMode::Shell);
    // The directory is live, not ignored: the written key comes back.
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tab_title_mode_key_on_disk_is_honored() {
    // Control for `missing_tab_title_mode_key_loads_as_shell`: the
    // same read path honors a key that does exist, so the leniency
    // result above cannot be a vacuous pass.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-present-titlemode-key-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        tab_title_mode: TabTitleMode::LastSegment,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert_eq!(loaded.tab_title_mode, TabTitleMode::LastSegment);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tab_position_key_on_disk_is_honored() {
    // Control for `missing_tab_position_key_loads_as_auto`: the
    // same read path honors a key that does exist, so the leniency
    // result above cannot be a vacuous pass.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-present-tabpos-key-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        tab_position: TabPosition::Top,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert_eq!(loaded.tab_position, TabPosition::Top);

    let _ = std::fs::remove_dir_all(&root);
}

/// Every serialized config key must appear in `docs/configuration.md`
/// — the docs-as-code drift gate: an undocumented key fails `just
/// check`. The key set is enumerated through serde itself (no
/// hand-kept list to drift), and the backticked form is required so a
/// bare word elsewhere in the prose cannot satisfy the check.
#[test]
fn every_config_key_is_documented() {
    let keys = serde_json::to_value(DropTermConfig::default())
        .expect("config serializes")
        .as_object()
        .expect("config serializes to a map")
        .keys()
        .cloned()
        .collect::<Vec<String>>();
    assert!(!keys.is_empty(), "key enumeration came back empty");

    let docs = include_str!("../../docs/configuration.md");
    for key in keys {
        assert!(
            docs.contains(&format!("`{key}`")),
            "config key `{key}` is not documented in docs/configuration.md"
        );
    }
}

#[test]
fn missing_autosuggestions_paths_key_loads_as_true() {
    // A config from a version before path completion: no
    // `autosuggestions_paths_enabled` key file. The entry derive
    // backfills from the struct Default — `true` — so pre-existing
    // configs see path completion, not a silently-off key.
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-missing-paths-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    DropTermConfig {
        height_percent: 70,
        ..DropTermConfig::default()
    }
    .write_entry(&config)
    .expect("write config keys");
    std::fs::remove_file(
        root.join("cosmic")
            .join("dropterm-config-test")
            .join(format!("v{CONFIG_VERSION}"))
            .join("autosuggestions_paths_enabled"),
    )
    .expect("remove autosuggestions_paths_enabled key file");

    let loaded = DropTermConfig::get_entry(&config).expect("key-less config loads without error");
    assert!(loaded.autosuggestions_paths_enabled);
    // The directory is live, not ignored: the written key comes back.
    assert_eq!(loaded.height_percent, 70);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_disabled_paths_toggle_round_trips_through_the_config_store() {
    // The off state persists — history-only behavior stays across a
    // write→load cycle (a present `false` key is honored, not
    // defaulted over).
    let root = std::env::temp_dir().join(format!(
        "dropterm-config-test-paths-off-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = cosmic_config::Config::with_custom_path(
        "dropterm-config-test",
        CONFIG_VERSION,
        root.clone(),
    )
    .expect("temp config context");
    let stored = DropTermConfig {
        autosuggestions_paths_enabled: false,
        ..DropTermConfig::default()
    };
    stored.write_entry(&config).expect("write config keys");

    let loaded = DropTermConfig::get_entry(&config).expect("config loads without error");
    assert_eq!(loaded, stored);
    assert!(!loaded.autosuggestions_paths_enabled);

    let _ = std::fs::remove_dir_all(&root);
}
