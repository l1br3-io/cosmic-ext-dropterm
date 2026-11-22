use super::*;

/// The sanitized shape of a real COSMIC custom-shortcuts file:
/// two `DropTerm` summon entries, two foreign spawns, two disables
/// (one keyless), an escaped apostrophe in a foreign description.
const FIXTURE: &str = r#"{
(
    modifiers: [
        Super,
        Shift,
    ],
    key: "e",
    description: Some("dropterm"),
): Spawn("dropterm toggle"),
(
    modifiers: [],
    key: "F12",
    description: Some("dropterm-dev"),
): Spawn("~/.local/bin/dropterm-dev toggle"),
(
    modifiers: [
        Alt,
    ],
    key: "grave",
    description: Some("Assistant: every workspace\'s windows"),
): Spawn("assistant show-all-windows"),
(
    modifiers: [
        Alt,
        Shift,
    ],
    key: "grave",
    description: Some("Assistant: same app windows, backwards"),
): Spawn("assistant same-app-previous"),
(
    modifiers: [
        Super,
    ],
): Disable,
(
    modifiers: [
        Super,
        Ctrl,
    ],
    key: "k",
): Disable,
}"#;

#[test]
fn the_fixture_parses_and_the_summon_entries_surface() {
    let shortcuts = parse_custom(FIXTURE).expect("the fixture parses");
    assert_eq!(shortcuts.0.len(), 6, "every fixture entry parsed");

    let installed = summon_entries(&shortcuts, "dropterm");
    let installed: Vec<String> = installed.iter().map(binding_display).collect();
    assert_eq!(
        installed,
        vec!["Super + Shift + E".to_string()],
        "the installed summon chord surfaces for its own stem"
    );

    let dev = summon_entries(&shortcuts, "dropterm-dev");
    let dev: Vec<String> = dev.iter().map(binding_display).collect();
    assert_eq!(dev, vec!["F12".to_string()], "the dev launcher owns F12");

    let foreign = summon_entries(&shortcuts, "assistant");
    assert_eq!(
        foreign.len(),
        0,
        "the foreign spawns are not toggles: recognition needs the verb, not just the stem"
    );
}

#[test]
fn foreign_entries_survive_the_round_trip() {
    let parsed = parse_custom(FIXTURE).expect("the fixture parses");
    let text = serialize_custom(&parsed).expect("the map serializes");
    let reparsed = parse_custom(&text).expect("the round-trip parses");

    assert_eq!(reparsed.0.len(), parsed.0.len(), "no entry lost or gained");
    for (binding, value) in &parsed.0 {
        assert_eq!(
            reparsed.0.get(binding),
            Some(value),
            "entry {binding:?} survives the round-trip unchanged"
        );
    }
}

/// A store where built-in shortcuts were rebound beside the custom
/// entries — the shape cosmic-settings' shared `custom` key holds
/// on any machine with a rebound built-in (the defect GU4 fixes:
/// the typed enum rejected these, so the store read `Unparseable`
/// and a bound user was nagged at every cold boot).
const BUILT_IN_FIXTURE: &str = r#"{
(
    modifiers: [
        Super,
    ],
    key: "e",
    description: Some("dropterm"),
): Spawn("dropterm toggle"),
(
    modifiers: [
        Super,
    ],
    key: "d",
): Close,
(
    modifiers: [
        Super,
    ],
    key: "Left",
): Focus(Left),
(
    modifiers: [
        Super,
        Ctrl,
    ],
    key: "k",
): Disable,
(
    modifiers: [],
    key: "F12",
): System(Launcher),
}"#;

#[test]
fn built_in_variants_parse_beside_the_summon_entries() {
    let shortcuts = parse_custom(BUILT_IN_FIXTURE)
        .unwrap_or_else(|err| panic!("the rebound store parses as Loaded: {err}"));
    assert_eq!(
        shortcuts.0.len(),
        5,
        "every entry survives: spawn, two built-ins, disable, foreign"
    );
    let summon: Vec<String> = summon_entries(&shortcuts, "dropterm")
        .iter()
        .map(binding_display)
        .collect();
    assert_eq!(
        summon,
        vec!["Super + E".to_string()],
        "the summon entry is still recognized among the built-ins"
    );
}

#[test]
fn action_values_carry_verbatim_through_a_round_trip() {
    let parsed = parse_custom(BUILT_IN_FIXTURE).expect("the rebound store parses");
    let text = serialize_custom(&parsed).expect("the map serializes");
    let reparsed = parse_custom(&text).expect("the round-trip parses");

    for (binding, value) in &parsed.0 {
        let carried = reparsed
            .0
            .get(binding)
            .unwrap_or_else(|| panic!("entry {binding:?} lost in the round-trip"));
        assert_eq!(
            carried.0.get_ron(),
            value.0.get_ron(),
            "entry {binding:?}'s action carried byte-for-byte"
        );
    }
}

#[test]
fn spawn_command_probes_the_opaque_value() {
    let value = |action: &str| {
        let map = format!(r"{{ (modifiers: []): {action} }}");
        let parsed = parse_custom(&map).expect("the one-entry map parses");
        parsed.0.into_iter().next().expect("one entry").1
    };
    assert_eq!(
        value(r#"Spawn("dropterm toggle")"#)
            .spawn_command()
            .as_deref(),
        Some("dropterm toggle"),
        "the Spawn shape probes out"
    );
    assert_eq!(value("Close").spawn_command(), None, "a built-in");
    assert_eq!(value("Disable").spawn_command(), None, "a disable");
    assert_eq!(value("Focus(Left)").spawn_command(), None, "a built-in arg");
    assert_eq!(
        value("System(Launcher)").spawn_command(),
        None,
        "a foreign variant"
    );
}

#[test]
fn an_unparseable_map_and_a_bad_keysym_never_parse() {
    assert!(parse_custom("not ron at all").is_err(), "corrupt text");
    let bad_key = r#"{ (modifiers: [], key: "NotARealKeyname"): Spawn("x") }"#;
    assert!(parse_custom(bad_key).is_err(), "an unresolvable keysym");
}

#[test]
fn the_read_states_degrade_explicitly() {
    let missing = std::env::temp_dir().join(format!(
        "dropterm-global-shortcut-missing-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&missing);
    assert_eq!(read_custom(&missing), ReadCustom::Absent);

    let present = std::env::temp_dir().join(format!(
        "dropterm-global-shortcut-present-{}",
        std::process::id()
    ));
    std::fs::write(&present, FIXTURE).expect("the temp fixture writes");
    let loaded = read_custom(&present);
    assert!(
        matches!(loaded, ReadCustom::Loaded(_)),
        "the fixture file loads"
    );
    std::fs::write(&present, "not ron at all").expect("the garbage writes");
    assert_eq!(read_custom(&present), ReadCustom::Unparseable);
    let _ = std::fs::remove_file(&present);

    let unreadable = std::env::temp_dir().join(format!(
        "dropterm-global-shortcut-dir-{}",
        std::process::id()
    ));
    std::fs::create_dir(&unreadable).expect("the temp dir creates");
    assert_eq!(read_custom(&unreadable), ReadCustom::Unreadable);
    std::fs::remove_dir(&unreadable).expect("the temp dir cleans");
}

#[test]
fn the_config_path_lands_under_the_settings_id() {
    let path = custom_shortcuts_path(Path::new("/home/tester"));
    let path = path.to_str().expect("a utf-8 path");
    assert!(
        path.ends_with("com.system76.CosmicSettings.Shortcuts/v1/custom"),
        "the watched file: {path}"
    );
}

#[test]
fn toggle_spawn_recognition_owns_its_own_stem() {
    assert!(is_toggle_spawn("dropterm toggle", "dropterm"));
    assert!(is_toggle_spawn("/usr/bin/dropterm toggle", "dropterm"));
    assert!(is_toggle_spawn(
        "~/.local/bin/dropterm-dev toggle",
        "dropterm-dev"
    ));
    assert!(!is_toggle_spawn("dropterm toggle", "dropterm-dev"));
    assert!(!is_toggle_spawn("other-app toggle", "dropterm"));
    assert!(!is_toggle_spawn("dropterm show", "dropterm"));
    assert!(!is_toggle_spawn("dropterm", "dropterm"));
}

#[test]
fn the_onboarding_trigger_matrix() {
    assert!(
        onboarding_should_show(false, false),
        "unbound and not retired: ask"
    );
    assert!(!onboarding_should_show(false, true), "retired: never again");
    assert!(!onboarding_should_show(true, false), "bound: never again");
    assert!(
        !onboarding_should_show(true, true),
        "bound and retired: never again"
    );
}

#[test]
fn setup_options_keep_the_configure_arm_off_in_every_cell() {
    // The shipped constant: the "Configure for me" provider is the
    // writer module (SPEC-global-toggle-cosmic-writer), which does
    // not exist yet — so every cell of the matrix degrades to the
    // manual path alone.
    let loaded = ReadCustom::Loaded(parse_custom(FIXTURE).expect("the fixture parses"));
    let cells: [(&str, Environment, &ReadCustom); 5] = [
        ("COSMIC with a loaded store", Environment::Cosmic, &loaded),
        (
            "COSMIC with no store",
            Environment::Cosmic,
            &ReadCustom::Absent,
        ),
        (
            "COSMIC with an unreadable store",
            Environment::Cosmic,
            &ReadCustom::Unreadable,
        ),
        (
            "COSMIC with an unparseable store",
            Environment::Cosmic,
            &ReadCustom::Unparseable,
        ),
        ("off-COSMIC", Environment::Other, &loaded),
    ];
    for (cell, environment, store) in cells {
        assert!(
            !setup_options(environment, store).configure_for_me,
            "{cell}: the configure arm stays off until the writer's W3 turns it on"
        );
    }
}

#[test]
fn every_walk_names_the_toggle_command() {
    for environment in [Environment::Cosmic, Environment::Other] {
        let walk = summon_walk(environment);
        assert!(
            walk.iter().any(|step| step.contains("`dropterm toggle`")),
            "{environment:?}'s walk names the command"
        );
    }
    assert!(
        summon_walk(Environment::Cosmic)
            .iter()
            .any(|step| step.contains("F12")),
        "the COSMIC walk carries the unshifted-key advice"
    );
}

// --- the writer: the captured chord's conversion (W1) ---

/// The gpui key name a conversion pins, resolved to its store
/// keysym name (what `serialize_custom` writes into the file).
fn chord_key(modifiers: Modifiers, key: &str) -> String {
    match captured_chord(modifiers, key) {
        CapturedChord::Binding(binding) => {
            let key = binding.key.expect("a converted chord carries a key");
            xkb::keysym_get_name(key)
        }
        CapturedChord::BareModifier => "BareModifier".to_string(),
        CapturedChord::Unresolvable => "Unresolvable".to_string(),
    }
}

#[test]
fn the_captured_chord_converts_the_pinned_vocabulary() {
    // Single ASCII letters resolve through the daemon's own
    // from_char route, lowercased like the entries cosmic-settings
    // itself writes (the GU1 fixture's `key: "e"` form).
    assert_eq!(chord_key(Modifiers::new(), "c"), "c");
    assert_eq!(
        chord_key(Modifiers::new().shift(), "C"),
        "c",
        "gpui's shifted form lands on the same keysym"
    );
    // Digits and the printable punctuation the zoom rows use.
    assert_eq!(chord_key(Modifiers::new(), "1"), "1");
    assert_eq!(chord_key(Modifiers::new(), "="), "equal");
    assert_eq!(chord_key(Modifiers::new(), "+"), "plus");
    assert_eq!(chord_key(Modifiers::new(), "-"), "minus");
    // The named keys, in gpui's lowercase platform vocabulary.
    // PageUp/PageDown carry their historical X11 names ("Prior",
    // "Next") — `keysym_get_name`'s canonical forms, the exact
    // strings the store holds and the daemon's own serializer
    // writes.
    assert_eq!(chord_key(Modifiers::new(), "space"), "space");
    assert_eq!(chord_key(Modifiers::new(), "pageup"), "Prior");
    assert_eq!(chord_key(Modifiers::new(), "pagedown"), "Next");
    assert_eq!(chord_key(Modifiers::new(), "end"), "End");
    assert_eq!(chord_key(Modifiers::new(), "tab"), "Tab");
    assert_eq!(chord_key(Modifiers::new(), "escape"), "Escape");
    assert_eq!(chord_key(Modifiers::new(), "f12"), "F12");
}

#[test]
fn a_modifier_press_keeps_the_capture_armed_and_a_bad_name_stays_out() {
    // The modifier names gpui delivers for a bare press: the
    // capture stays armed (the seat's contract).
    for name in ["shift", "control", "ctrl", "alt", "super", "meta", "logo"] {
        assert_eq!(
            captured_chord(Modifiers::new(), name),
            CapturedChord::BareModifier,
            "bare {name} keeps the capture armed"
        );
    }
    // Anything else that resolves to no keysym is refused — an
    // empty name and an unknown word both.
    assert_eq!(
        captured_chord(Modifiers::new(), ""),
        CapturedChord::Unresolvable
    );
    assert_eq!(
        captured_chord(Modifiers::new(), "not-a-real-key"),
        CapturedChord::Unresolvable
    );
}

// --- the writer: the verdict over the effective map (W1) ---

/// A stock system-defaults fixture: the F-keys' honest shape
/// (Alt+F4 and Super+F11 only — the stock COSMIC set), no
/// descriptions, built-in actions.
const STOCK_DEFAULTS: &str = r#"{
(
    modifiers: [
        Alt,
    ],
    key: "F4",
): Close,
(
    modifiers: [
        Super,
    ],
    key: "F11",
): Fullscreen,
}"#;

/// A custom entry holding the marker's exact shape at `key` — the
/// `Use F12` write's own product, for the Own-verdict pin.
fn marker_entry_fixture(key: &str) -> String {
    format!(
        r#"{{ (
    modifiers: [],
    key: "{key}",
    description: Some("DropTerm"),
): Spawn("dropterm toggle") }}"#
    )
}

/// A binding parsed out of a one-entry map — the verdict tests'
/// chord builder. The dummy action only satisfies the map's
/// shape; the tests read the binding back.
fn binding_of(key: &str, modifiers: Modifiers) -> Binding {
    let text = format!(
        r#"{{ (modifiers: [{mods}], key: "{key}"): Disable }}"#,
        mods = ModifiersDef::from(modifiers)
            .0
            .iter()
            .map(|modifier| match modifier {
                Modifier::Ctrl => "Ctrl",
                Modifier::Alt => "Alt",
                Modifier::Shift => "Shift",
                Modifier::Super => "Super",
            })
            .collect::<Vec<_>>()
            .join(", "),
    );
    parse_custom(&text)
        .expect("the one-entry map parses")
        .0
        .into_iter()
        .next()
        .expect("one entry")
        .0
}

#[test]
fn the_merge_overrides_defaults_with_custom_at_equal_bindings() {
    let defaults = parse_custom(STOCK_DEFAULTS).expect("the stock fixture parses");
    let custom = parse_custom(
        r#"{ (modifiers: [Alt], key: "F4", description: Some("DropTerm")): Spawn("dropterm toggle") }"#,
    )
    .expect("the marker parses");
    let effective = effective_shortcuts(&defaults, &custom);

    // The marker's chord wins over the stock default.
    let f4 = binding_of("F4", Modifiers::new().alt());
    match effective.0.get(&f4) {
        Some(value) => assert!(
            value.spawn_command().is_some(),
            "custom's spawn overrides the stock Close at Alt+F4"
        ),
        None => panic!("the overridden entry vanished"),
    }
    // The untouched default stands.
    let f11 = binding_of("F11", Modifiers::new().logo());
    assert!(
        effective.0.contains_key(&f11),
        "the stock entry without a custom override stands"
    );
    assert_eq!(effective.0.len(), 2, "equal bindings collide as one");
}

#[test]
fn the_verdict_matrix_over_the_effective_map() {
    let stock = parse_custom(STOCK_DEFAULTS).expect("the stock fixture parses");
    let custom = parse_custom(FIXTURE).expect("the fixture parses");
    let stock_effective = effective_shortcuts(&stock, &CustomShortcuts::default());
    let custom_effective = effective_shortcuts(&CustomShortcuts::default(), &custom);

    // Free: commit.
    assert_eq!(
        capture_verdict(
            &binding_of("q", Modifiers::new().logo()),
            &stock_effective,
            "dropterm"
        ),
        CaptureVerdict::Commit
    );
    // F12 is commit against the stock defaults — the "Use F12"
    // button's verdict (research: stock COSMIC leaves F12 free).
    assert_eq!(
        capture_verdict(
            &binding_of("F12", Modifiers::new()),
            &stock_effective,
            "dropterm"
        ),
        CaptureVerdict::Commit
    );
    // A stock default holds Alt+F4; no description, so the holder
    // names itself by chord.
    assert_eq!(
        capture_verdict(
            &binding_of("F4", Modifiers::new().alt()),
            &stock_effective,
            "dropterm"
        ),
        CaptureVerdict::Conflict("Alt + F4".to_string())
    );
    // A custom entry holds Super+Shift+E; its description names
    // the holder.
    assert_eq!(
        capture_verdict(
            &binding_of("e", Modifiers::new().logo().shift()),
            &custom_effective,
            "dropterm"
        ),
        CaptureVerdict::Conflict("dropterm".to_string())
    );
    // The writer's own marker is Own — recapturing its chord
    // writes nothing. F12 keeps the chord settable unmodified
    // (a bare letter would be Unsupported before the lookup).
    let own = parse_custom(&marker_entry_fixture("F12")).expect("the marker parses");
    let own_effective = effective_shortcuts(&CustomShortcuts::default(), &own);
    assert_eq!(
        capture_verdict(
            &binding_of("F12", Modifiers::new()),
            &own_effective,
            "dropterm"
        ),
        CaptureVerdict::Own
    );
    // The same chord under another stem is foreign: conflict.
    assert_eq!(
        capture_verdict(
            &binding_of("F12", Modifiers::new()),
            &own_effective,
            "dropterm-dev"
        ),
        CaptureVerdict::Conflict("DropTerm".to_string())
    );
}

#[test]
fn unsupported_chords_write_nothing() {
    let empty = effective_shortcuts(&CustomShortcuts::default(), &CustomShortcuts::default());
    // COSMIC's forbidden unmodified keysyms: a bare letter, digit,
    // space, tab, return, caps lock.
    for key in ["c", "1", "space", "tab", "return", "Caps_Lock"] {
        assert_eq!(
            capture_verdict(&binding_of(key, Modifiers::new()), &empty, "dropterm"),
            CaptureVerdict::Unsupported,
            "unmodified {key} is forbidden"
        );
    }
    // A bare modifier with no key (not bare Super) is unsettable.
    assert_eq!(
        capture_verdict(
            &binding_of_no_key(Modifiers::new().shift()),
            &empty,
            "dropterm"
        ),
        CaptureVerdict::Unsupported
    );
    // Bare Super is a legal binding: evaluated normally, commit
    // over an empty map.
    assert_eq!(
        capture_verdict(
            &binding_of_no_key(Modifiers::new().logo()),
            &empty,
            "dropterm"
        ),
        CaptureVerdict::Commit
    );
    // Printable punctuation outside the forbidden set is settable
    // unmodified.
    assert_eq!(
        capture_verdict(&binding_of("equal", Modifiers::new()), &empty, "dropterm"),
        CaptureVerdict::Commit
    );
}

/// A keyless binding — bare Super and bare modifiers. The dummy
/// action only satisfies the map's shape.
fn binding_of_no_key(modifiers: Modifiers) -> Binding {
    let mods = ModifiersDef::from(modifiers)
        .0
        .iter()
        .map(|modifier| match modifier {
            Modifier::Ctrl => "Ctrl",
            Modifier::Alt => "Alt",
            Modifier::Shift => "Shift",
            Modifier::Super => "Super",
        })
        .collect::<Vec<_>>()
        .join(", ");
    let text = format!(r"{{ (modifiers: [{mods}]): Disable }}");
    parse_custom(&text)
        .expect("the keyless map parses")
        .0
        .into_iter()
        .next()
        .expect("one entry")
        .0
}

#[test]
fn a_disable_held_chord_is_committable_over() {
    // The keyboard page's conflict rule filters Disable: a chord
    // the user disabled is free for the marker to take (the write
    // replaces the disable, exactly as COSMIC's replace would).
    // F12 keeps the chord settable unmodified.
    let disabled = parse_custom(r#"{ (modifiers: [], key: "F12"): Disable }"#)
        .expect("the disable map parses");
    let effective = effective_shortcuts(&CustomShortcuts::default(), &disabled);
    assert_eq!(
        capture_verdict(&binding_of("F12", Modifiers::new()), &effective, "dropterm"),
        CaptureVerdict::Commit
    );
}

// --- the writer: the commit and revert planners (W1) ---

/// The marker's action RON as the store holds it.
const MARKER_ACTION_RON: &str = r#"Spawn("/usr/bin/dropterm toggle")"#;

/// Serialize, and re-parse what will be written — every planner
/// test's output gate: the write seam refuses to write anything
/// that does not parse, so the planners' product must.
fn round_trip(planned: &CustomShortcuts) -> CustomShortcuts {
    let text = serialize_custom(planned).expect("the planned map serializes");
    parse_custom(&text).expect("the planned map's output re-parses")
}

#[test]
fn the_commit_planner_adds_exactly_one_marker_and_foreign_entries_carry_verbatim() {
    // FIXTURE already carries a hand-made own-stem summon
    // (`description: Some("dropterm")`) — Open Question 2's
    // standing user: it survives untouched beside the marker.
    let custom = parse_custom(FIXTURE).expect("the fixture parses");
    // F9 is free in the fixture (F12 is the dev launcher's chord
    // — a non-Disable holder the verdict would refuse).
    let binding = binding_of("F9", Modifiers::new());
    let reparsed = round_trip(&commit_plan(
        &custom,
        &binding,
        "dropterm",
        "/usr/bin/dropterm toggle",
    ));

    assert_eq!(reparsed.0.len(), custom.0.len() + 1, "one entry added");
    let value = reparsed
        .0
        .get(&binding)
        .expect("the marker entry is at the committed chord");
    assert_eq!(value.0.get_ron(), MARKER_ACTION_RON, "the marker's action");
    let (stored_key, stored_value) = reparsed.0.get_key_value(&binding).expect("the marker key");
    assert!(
        is_marker(stored_key, stored_value, "dropterm"),
        "the written entry is the marker: description, chord, own toggle"
    );
    // Every foreign entry — the hand-made summon included — is
    // byte-identical before and after.
    for (foreign_binding, foreign_value) in &custom.0 {
        let carried = reparsed
            .0
            .get(foreign_binding)
            .unwrap_or_else(|| panic!("entry {foreign_binding:?} lost in the commit"));
        assert_eq!(
            carried.0.get_ron(),
            foreign_value.0.get_ron(),
            "entry {foreign_binding:?}'s action carried verbatim"
        );
    }
}

#[test]
fn the_commit_planner_moves_an_existing_marker_and_reparses_built_in_stores() {
    // A marker already at F12; the commit moves it to Super+Q —
    // the update path, never a second marker.
    let custom = parse_custom(&marker_entry_fixture("F12")).expect("the marker parses");
    let binding = binding_of("Q", Modifiers::new().logo());
    let reparsed = round_trip(&commit_plan(
        &custom,
        &binding,
        "dropterm",
        "/usr/bin/dropterm toggle",
    ));
    assert_eq!(reparsed.0.len(), 1, "the marker moved, not added");
    let old_chord = binding_of("F12", Modifiers::new());
    assert!(
        !reparsed.0.contains_key(&old_chord),
        "the old marker chord is gone"
    );
    assert_eq!(
        reparsed
            .0
            .get(&binding)
            .expect("the new marker")
            .spawn_command()
            .as_deref(),
        Some("/usr/bin/dropterm toggle")
    );

    // A store carrying rebound built-ins (`Close`, `Focus(Left)`)
    // and a `Disable` plans and re-parses with every action
    // verbatim — the wipe hazard's own fixture.
    let built_ins = parse_custom(BUILT_IN_FIXTURE).expect("the rebound store parses");
    let reparsed = round_trip(&commit_plan(
        &built_ins,
        &binding,
        "dropterm",
        "/usr/bin/dropterm toggle",
    ));
    for (foreign_binding, foreign_value) in &built_ins.0 {
        let carried = reparsed
            .0
            .get(foreign_binding)
            .unwrap_or_else(|| panic!("entry {foreign_binding:?} lost in the commit"));
        assert_eq!(
            carried.0.get_ron(),
            foreign_value.0.get_ron(),
            "entry {foreign_binding:?}'s built-in action carried verbatim"
        );
    }
}

#[test]
fn the_revert_planner_removes_exactly_the_marker() {
    let custom = parse_custom(FIXTURE).expect("the fixture parses");
    let with_marker = round_trip(&commit_plan(
        &custom,
        &binding_of("F9", Modifiers::new()),
        "dropterm",
        "/usr/bin/dropterm toggle",
    ));
    let reparsed = round_trip(&revert_plan(&with_marker, "dropterm"));

    let marker_chord = binding_of("F9", Modifiers::new());
    assert!(
        !reparsed.0.contains_key(&marker_chord),
        "the marker is gone"
    );
    assert_eq!(reparsed.0.len(), custom.0.len(), "only the marker left");
    for (foreign_binding, foreign_value) in &custom.0 {
        let carried = reparsed
            .0
            .get(foreign_binding)
            .unwrap_or_else(|| panic!("entry {foreign_binding:?} lost in the revert"));
        assert_eq!(
            carried.0.get_ron(),
            foreign_value.0.get_ron(),
            "entry {foreign_binding:?}'s action carried verbatim"
        );
    }

    // A "DropTerm"-described entry that is not this binary's
    // toggle is not the marker: another stem's marker survives
    // this stem's revert.
    let other_stem = parse_custom(
        r#"{
(
    modifiers: [],
    key: "F9",
    description: Some("DropTerm"),
): Spawn("dropterm-dev toggle") }"#,
    )
    .expect("the other-stem marker parses");
    let reparsed = round_trip(&revert_plan(&other_stem, "dropterm"));
    assert_eq!(reparsed.0.len(), 1, "the dev build's marker survives");
}

#[test]
fn the_marker_command_is_the_absolute_path_and_the_toggle_verb() {
    assert_eq!(
        marker_spawn_command(Path::new("/usr/bin/dropterm")),
        "/usr/bin/dropterm toggle"
    );
}

#[test]
fn environment_detection_reads_the_session_variable() {
    assert_eq!(detect_environment(Some("COSMIC")), Environment::Cosmic);
    assert_eq!(detect_environment(Some("cosmic")), Environment::Cosmic);
    assert_eq!(detect_environment(Some("COSMIC:foo")), Environment::Cosmic);
    assert_eq!(detect_environment(Some("GNOME")), Environment::Other);
    assert_eq!(detect_environment(None), Environment::Other);
}
