use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

/// A per-test scratch directory under the system temp dir with a
/// process- and test-unique name, removed (best-effort) on drop
/// even when an assertion fails. Std only — no dev-dependency.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let unique = format!(
            "dropterm-session-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("create scratch dir");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// A path that is reliably not a directory, for cwd-validation tests.
fn dead_path() -> PathBuf {
    PathBuf::from("/nonexistent/dropterm-session-test")
}

fn session_file(dir: &TempDir) -> PathBuf {
    dir.join("session.ron")
}

fn backup_of(path: &Path) -> PathBuf {
    sibling(path, ".bak")
}

fn write_session(path: &Path, text: &str) {
    fs::write(path, text).expect("write session file");
}

fn one_tab_state(version: u32) -> SessionState {
    SessionState {
        version,
        active: Some(0),
        tabs: vec![SessionTab {
            title: "vim".to_owned(),
            title_override: None,
            shell: "/bin/sh".to_owned(),
            cwd: Some(PathBuf::from("/tmp")),
            profile: None,
            pinned: false,
        }],
    }
}

#[test]
fn session_state_round_trips_through_ron() {
    let state = SessionState {
        version: SESSION_VERSION,
        active: Some(1),
        tabs: vec![SessionTab {
            title: "vim — src".to_owned(),
            title_override: Some("work".to_owned()),
            shell: "/bin/zsh".to_owned(),
            cwd: Some(PathBuf::from("/home/jpt/src")),
            profile: None,
            pinned: false,
        }],
    };
    let text = ron::to_string(&state).expect("serialize");
    let back: SessionState = ron::from_str(&text).expect("deserialize");
    assert_eq!(back, state);
}

#[test]
fn a_profile_tab_round_trips_through_ron() {
    let state = SessionState {
        version: SESSION_VERSION,
        active: Some(0),
        tabs: vec![SessionTab {
            title: "web-prod".to_owned(),
            title_override: None,
            shell: "ssh".to_owned(),
            cwd: Some(PathBuf::from("/srv/web")),
            profile: Some("web-prod".to_owned()),
            pinned: false,
        }],
    };
    let text = ron::to_string(&state).expect("serialize");
    let back: SessionState = ron::from_str(&text).expect("deserialize");
    assert_eq!(back, state);
}

#[test]
fn a_pinned_tab_round_trips_through_ron() {
    let state = SessionState {
        version: SESSION_VERSION,
        active: Some(0),
        tabs: vec![SessionTab {
            title: "vim".to_owned(),
            title_override: None,
            shell: "/bin/sh".to_owned(),
            cwd: None,
            profile: None,
            pinned: true,
        }],
    };
    let text = ron::to_string(&state).expect("serialize");
    let back: SessionState = ron::from_str(&text).expect("deserialize");
    assert_eq!(back, state);
}

#[test]
fn a_pre_pin_session_file_loads_with_pins_unset_and_no_quarantine() {
    // Files written before pins existed lack the `pinned` field
    // entirely; they must load as unpinned tabs — the
    // additive-field compatibility pin (the `profile` precedent),
    // with the version still 1 so nothing quarantines.
    let dir = TempDir::new("load-pre-pin");
    let path = session_file(&dir);
    write_session(
        &path,
        r#"(
                version: 1,
                active: Some(0),
                tabs: [
                    (title: "vim", title_override: None, shell: "/bin/sh", cwd: None, profile: None),
                ],
            )"#,
    );
    let state = load(&path).expect("a pre-pin session file still loads");
    assert_eq!(state.tabs.len(), 1);
    assert!(!state.tabs[0].pinned);
    assert!(!backup_of(&path).exists(), "version 1 never quarantines");
}

#[test]
fn capture_maps_the_pin_verbatim() {
    // The pin is saved state, not derived: whatever the live tab
    // carried at save time is what the session records.
    let snapshots = [
        TabSnapshot {
            title: "pinned",
            title_override: None,
            shell: "/bin/sh",
            cwd: None,
            ephemeral: false,
            profile: None,
            pinned: true,
        },
        TabSnapshot {
            title: "loose",
            title_override: None,
            shell: "/bin/sh",
            cwd: None,
            ephemeral: false,
            profile: None,
            pinned: false,
        },
    ];
    let state = capture(&snapshots, 0);
    assert!(state.tabs[0].pinned);
    assert!(!state.tabs[1].pinned);
}

#[test]
fn plan_restore_replays_the_saved_pin() {
    // The pin chain's plan half: a saved pin reaches the restore
    // plan's spawn spec, an unset one stays unset.
    let state = SessionState {
        version: SESSION_VERSION,
        active: Some(0),
        tabs: vec![
            SessionTab {
                title: "pinned".to_owned(),
                title_override: None,
                shell: "/bin/sh".to_owned(),
                cwd: None,
                profile: None,
                pinned: true,
            },
            SessionTab {
                title: "loose".to_owned(),
                title_override: None,
                shell: "/bin/sh".to_owned(),
                cwd: None,
                profile: None,
                pinned: false,
            },
        ],
    };
    let plan = plan_restore(&state, None, &[]).expect("valid plan");
    assert!(plan.tabs[0].pinned);
    assert!(!plan.tabs[1].pinned);
}

#[test]
fn a_session_file_without_the_profile_key_deserializes() {
    // Files written before profiles existed lack the `profile`
    // field entirely; they must load as plain-shell tabs — the
    // additive-optional-field compatibility pin.
    let text = r#"(
            version: 1,
            active: Some(0),
            tabs: [
                (title: "vim", title_override: None, shell: "/bin/sh", cwd: None),
            ],
        )"#;
    let state: SessionState = ron::from_str(text).expect("pre-profile session parses");
    assert_eq!(state.tabs.len(), 1);
    assert_eq!(state.tabs[0].profile, None);
}

#[test]
fn capture_skips_ephemeral_tabs_and_remaps_active() {
    // A remote --cmd tab is session-transient: never saved, never
    // replayed — and a saved active index must point at the same
    // surviving tab after the skip.
    let snapshots = [
        TabSnapshot {
            title: "work",
            title_override: None,
            shell: "/bin/zsh",
            cwd: None,
            ephemeral: false,
            profile: None,
            pinned: false,
        },
        TabSnapshot {
            title: "htop",
            title_override: None,
            shell: "htop",
            cwd: None,
            ephemeral: true,
            profile: None,
            pinned: false,
        },
        TabSnapshot {
            title: "src",
            title_override: None,
            shell: "/bin/zsh",
            cwd: None,
            ephemeral: false,
            profile: None,
            pinned: false,
        },
    ];
    let state = capture(&snapshots, 2);
    let titles: Vec<&str> = state.tabs.iter().map(|tab| tab.title.as_str()).collect();
    assert_eq!(titles, vec!["work", "src"]);
    assert_eq!(state.active, Some(1));
}

#[test]
fn an_all_ephemeral_layout_saves_an_empty_session() {
    // Restore then spawns a single fresh tab — the never-empty
    // rule — rather than replaying any program.
    let snapshots = [TabSnapshot {
        title: "htop",
        title_override: None,
        shell: "htop",
        cwd: None,
        ephemeral: true,
        profile: None,
        pinned: false,
    }];
    let state = capture(&snapshots, 0);
    assert!(state.tabs.is_empty(), "not empty: {:?}", state.tabs);
}

#[test]
fn capture_keeps_only_cwds_that_are_directories() {
    let dir = TempDir::new("capture-stale");
    let live = dir.join("live");
    fs::create_dir(&live).expect("create live dir");
    let items = [
        TabSnapshot {
            title: "live",
            title_override: None,
            shell: "/bin/sh",
            cwd: Some(&live),
            ephemeral: false,
            profile: None,
            pinned: false,
        },
        TabSnapshot {
            title: "dead",
            title_override: None,
            shell: "/bin/sh",
            cwd: Some(&dead_path()),
            ephemeral: false,
            profile: None,
            pinned: false,
        },
        TabSnapshot {
            title: "unset",
            title_override: None,
            shell: "/bin/sh",
            cwd: None,
            ephemeral: false,
            profile: None,
            pinned: false,
        },
    ];
    let state = capture(&items, 2);
    assert_eq!(state.version, SESSION_VERSION);
    assert_eq!(state.active, Some(2));
    assert_eq!(state.tabs[0].cwd.as_deref(), Some(live.as_path()));
    assert_eq!(state.tabs[1].cwd, None);
    assert_eq!(state.tabs[2].cwd, None);
}

#[test]
fn capture_keeps_titles_overrides_and_shells() {
    let items = [TabSnapshot {
        title: "vim",
        title_override: Some("work"),
        shell: "/bin/zsh",
        cwd: None,
        ephemeral: false,
        profile: None,
        pinned: false,
    }];
    let state = capture(&items, 0);
    assert_eq!(state.tabs[0].title, "vim");
    assert_eq!(state.tabs[0].title_override.as_deref(), Some("work"));
    assert_eq!(state.tabs[0].shell, "/bin/zsh");
}

#[test]
fn capture_maps_the_profile_id_and_keeps_skipping_ephemerals() {
    // The id is saved verbatim from the live tab; the ephemeral
    // skip rule stays capture's own — a transient tab never enters
    // the session, whatever it carries.
    let snapshots = [
        TabSnapshot {
            title: "shell",
            title_override: None,
            shell: "/bin/zsh",
            cwd: None,
            ephemeral: false,
            profile: None,
            pinned: false,
        },
        TabSnapshot {
            title: "web",
            title_override: None,
            shell: "ssh",
            cwd: None,
            ephemeral: false,
            profile: Some("web-prod"),
            pinned: false,
        },
        TabSnapshot {
            title: "gone",
            title_override: None,
            shell: "htop",
            cwd: None,
            ephemeral: true,
            profile: Some("web-prod"),
            pinned: false,
        },
    ];
    let state = capture(&snapshots, 1);
    let ids: Vec<Option<&str>> = state
        .tabs
        .iter()
        .map(|tab| tab.profile.as_deref())
        .collect();
    assert_eq!(ids, vec![None, Some("web-prod")]);
}

#[test]
fn plan_restore_keeps_live_cwds_and_falls_back_to_home() {
    let dir = TempDir::new("plan-fallback");
    let home = dir.join("home");
    let live = dir.join("live");
    fs::create_dir(&home).expect("create home dir");
    fs::create_dir(&live).expect("create live dir");
    let state = SessionState {
        version: SESSION_VERSION,
        active: Some(0),
        tabs: vec![
            SessionTab {
                title: "live".to_owned(),
                title_override: None,
                shell: "/bin/sh".to_owned(),
                cwd: Some(live.clone()),
                profile: None,
                pinned: false,
            },
            SessionTab {
                title: "dead".to_owned(),
                title_override: Some("work".to_owned()),
                shell: "/bin/zsh".to_owned(),
                cwd: Some(dead_path()),
                profile: None,
                pinned: false,
            },
        ],
    };
    let plan = plan_restore(&state, Some(&home), &[]).expect("valid plan");
    assert_eq!(plan.tabs[0].cwd.as_deref(), Some(live.as_path()));
    assert_eq!(plan.tabs[1].cwd.as_deref(), Some(home.as_path()));
    assert_eq!(plan.tabs[1].title, "dead");
    assert_eq!(plan.tabs[1].title_override.as_deref(), Some("work"));
    assert_eq!(plan.tabs[1].shell, "/bin/zsh");
}

#[test]
fn plan_restore_without_home_spawns_without_a_cwd() {
    let state = SessionState {
        version: SESSION_VERSION,
        active: Some(0),
        tabs: vec![SessionTab {
            title: "dead".to_owned(),
            title_override: None,
            shell: "/bin/sh".to_owned(),
            cwd: Some(dead_path()),
            profile: None,
            pinned: false,
        }],
    };
    let plan = plan_restore(&state, None, &[]).expect("valid plan");
    assert_eq!(plan.tabs[0].cwd, None);
}

#[test]
fn plan_restore_clamps_active_into_range() {
    let state = SessionState {
        version: SESSION_VERSION,
        active: Some(7),
        tabs: vec![
            SessionTab {
                title: "a".to_owned(),
                title_override: None,
                shell: "/bin/sh".to_owned(),
                cwd: None,
                profile: None,
                pinned: false,
            },
            SessionTab {
                title: "b".to_owned(),
                title_override: None,
                shell: "/bin/sh".to_owned(),
                cwd: None,
                profile: None,
                pinned: false,
            },
        ],
    };
    let plan = plan_restore(&state, None, &[]).expect("valid plan");
    assert_eq!(plan.active, 1);
}

#[test]
fn plan_restore_defaults_active_to_zero_when_absent() {
    let state = SessionState {
        version: SESSION_VERSION,
        active: None,
        tabs: vec![SessionTab {
            title: "a".to_owned(),
            title_override: None,
            shell: "/bin/sh".to_owned(),
            cwd: None,
            profile: None,
            pinned: false,
        }],
    };
    let plan = plan_restore(&state, None, &[]).expect("valid plan");
    assert_eq!(plan.active, 0);
}

#[test]
fn plan_restore_rejects_an_empty_tab_list() {
    let state = SessionState {
        version: SESSION_VERSION,
        active: None,
        tabs: vec![],
    };
    assert!(plan_restore(&state, None, &[]).is_none());
}

fn test_profile(name: &str, command: &str, cwd: Option<PathBuf>) -> Profile {
    Profile {
        name: name.to_owned(),
        command: command.to_owned(),
        cwd,
        color_scheme: None,
        env: None,
        font_size: None,
        scrollback_lines: None,
    }
}

#[test]
fn plan_restore_re_resolves_a_saved_profile_id_to_the_current_definition() {
    // Re-resolve by id: the profile's *current* definition wins, so
    // an edit between save and restore launches the edited command
    // — the session stores the id, never the command.
    let state = SessionState {
        version: SESSION_VERSION,
        active: Some(0),
        tabs: vec![SessionTab {
            title: "web-prod".to_owned(),
            title_override: None,
            shell: "ssh".to_owned(),
            cwd: None,
            profile: Some("web-prod".to_owned()),
            pinned: false,
        }],
    };
    let profiles = [test_profile("web-prod", "ssh edited-host", None)];
    let plan = plan_restore(&state, None, &profiles).expect("valid plan");
    assert_eq!(
        plan.tabs[0].profile,
        Some(test_profile("web-prod", "ssh edited-host", None))
    );
}

#[test]
fn plan_restore_missing_profile_id_spawns_the_plain_shell_without_chaining() {
    // A saved id that no longer resolves is the plain shell in the
    // re-validated saved cwd — never a chain through another
    // profile (ruling 4: the session records the id it launched
    // under, and that id alone). The decoy profile's live cwd must
    // not be consulted.
    let dir = TempDir::new("plan-missing-id");
    let live = dir.join("live");
    let decoy = dir.join("decoy");
    fs::create_dir(&live).expect("create live dir");
    fs::create_dir(&decoy).expect("create decoy dir");
    let state = SessionState {
        version: SESSION_VERSION,
        active: Some(0),
        tabs: vec![SessionTab {
            title: "gone".to_owned(),
            title_override: None,
            shell: "/bin/sh".to_owned(),
            cwd: Some(live.clone()),
            profile: Some("ghost".to_owned()),
            pinned: false,
        }],
    };
    let profiles = [test_profile("other", "ssh other-host", Some(decoy))];
    let plan = plan_restore(&state, None, &profiles).expect("valid plan");
    assert_eq!(plan.tabs[0].profile, None);
    assert_eq!(plan.tabs[0].cwd.as_deref(), Some(live.as_path()));
}

#[test]
fn plan_restore_keeps_a_live_saved_cwd_over_the_profile_cwd() {
    // The saved cwd keeps the familiar place: it wins while it is
    // still a directory, even when the profile's current
    // definition carries a different one.
    let dir = TempDir::new("plan-saved-wins");
    let saved = dir.join("saved");
    let preset = dir.join("preset");
    fs::create_dir(&saved).expect("create saved dir");
    fs::create_dir(&preset).expect("create preset dir");
    let state = SessionState {
        version: SESSION_VERSION,
        active: Some(0),
        tabs: vec![SessionTab {
            title: "web".to_owned(),
            title_override: None,
            shell: "ssh".to_owned(),
            cwd: Some(saved.clone()),
            profile: Some("web-prod".to_owned()),
            pinned: false,
        }],
    };
    let profiles = [test_profile("web-prod", "ssh web-prod", Some(preset))];
    let plan = plan_restore(&state, None, &profiles).expect("valid plan");
    assert_eq!(plan.tabs[0].cwd.as_deref(), Some(saved.as_path()));
}

#[test]
fn plan_restore_falls_back_to_a_live_profile_cwd_then_home() {
    // The OQ-1 chain below the saved cwd: the profile's cwd while
    // still a directory (replacing `home` as the first fallback),
    // then `$HOME` — program from re-resolve, place from the chain.
    let dir = TempDir::new("plan-chain");
    let home = dir.join("home");
    let preset = dir.join("preset");
    fs::create_dir(&home).expect("create home dir");
    fs::create_dir(&preset).expect("create preset dir");
    let state = SessionState {
        version: SESSION_VERSION,
        active: Some(0),
        tabs: vec![
            SessionTab {
                title: "preset".to_owned(),
                title_override: None,
                shell: "ssh".to_owned(),
                cwd: Some(dead_path()),
                profile: Some("live-preset".to_owned()),
                pinned: false,
            },
            SessionTab {
                title: "home".to_owned(),
                title_override: None,
                shell: "ssh".to_owned(),
                cwd: Some(dead_path()),
                profile: Some("dead-preset".to_owned()),
                pinned: false,
            },
        ],
    };
    let profiles = [
        test_profile("live-preset", "ssh one", Some(preset.clone())),
        test_profile("dead-preset", "ssh two", Some(dead_path())),
    ];
    let plan = plan_restore(&state, Some(&home), &profiles).expect("valid plan");
    assert_eq!(plan.tabs[0].cwd.as_deref(), Some(preset.as_path()));
    assert_eq!(plan.tabs[1].cwd.as_deref(), Some(home.as_path()));
}

#[test]
fn load_returns_none_for_a_missing_file() {
    let dir = TempDir::new("load-missing");
    assert!(load(&session_file(&dir)).is_none());
    // First run is silent: nothing to quarantine, nothing created.
    assert!(!backup_of(&session_file(&dir)).exists());
}

#[test]
fn load_reads_a_valid_session() {
    let dir = TempDir::new("load-valid");
    let path = session_file(&dir);
    let state = one_tab_state(SESSION_VERSION);
    write_session(&path, &ron::to_string(&state).expect("serialize"));
    assert_eq!(load(&path), Some(state));
}

#[test]
fn load_rejects_versions_other_than_one() {
    for version in [0, 2] {
        let dir = TempDir::new("load-version");
        let path = session_file(&dir);
        write_session(
            &path,
            &ron::to_string(&one_tab_state(version)).expect("serialize"),
        );
        assert!(load(&path).is_none(), "version {version} must not restore");
        assert!(
            backup_of(&path).exists(),
            "version {version} must be quarantined"
        );
    }
}

#[test]
fn load_rejects_a_file_without_a_version() {
    let dir = TempDir::new("load-absent-version");
    let path = session_file(&dir);
    write_session(&path, "(active: None, tabs: [])");
    assert!(load(&path).is_none());
    assert!(backup_of(&path).exists());
}

#[test]
fn load_quarantines_corrupt_input() {
    let dir = TempDir::new("load-corrupt");
    let path = session_file(&dir);
    write_session(&path, "not ron at all((");
    assert!(load(&path).is_none());
    let backup = fs::read_to_string(backup_of(&path)).expect("backup exists");
    assert_eq!(backup, "not ron at all((");
    assert!(!path.exists());
}

#[test]
fn load_quarantines_non_struct_input() {
    let dir = TempDir::new("load-non-struct");
    let path = session_file(&dir);
    write_session(&path, "42");
    assert!(load(&path).is_none());
    assert!(backup_of(&path).exists());
}

#[test]
fn load_replaces_an_older_backup() {
    let dir = TempDir::new("load-stale-backup");
    let path = session_file(&dir);
    write_session(&backup_of(&path), "old backup");
    write_session(&path, "garbage((");
    assert!(load(&path).is_none());
    let backup = fs::read_to_string(backup_of(&path)).expect("backup exists");
    assert_eq!(backup, "garbage((");
}

#[test]
fn save_atomic_writes_a_loadable_file_and_leaves_no_temp() {
    let dir = TempDir::new("save-round-trip");
    let path = session_file(&dir);
    let state = one_tab_state(SESSION_VERSION);
    save_atomic(&path, &state).expect("save");
    assert_eq!(load(&path), Some(state));
    assert!(!sibling(&path, ".tmp").exists());
}

#[test]
fn save_atomic_overwrites_an_existing_file() {
    let dir = TempDir::new("save-overwrite");
    let path = session_file(&dir);
    save_atomic(&path, &one_tab_state(SESSION_VERSION)).expect("first save");
    let second = SessionState {
        version: SESSION_VERSION,
        active: None,
        tabs: vec![],
    };
    save_atomic(&path, &second).expect("second save");
    assert_eq!(load(&path), Some(second));
    assert!(!sibling(&path, ".tmp").exists());
}

#[test]
fn save_atomic_creates_missing_parent_directories() {
    let dir = TempDir::new("save-parents");
    let path = dir.join("nested/deeper/session.ron");
    save_atomic(&path, &one_tab_state(SESSION_VERSION)).expect("save");
    assert!(path.is_file());
}

/// A one-tab session with no filesystem dependencies (no cwd to
/// validate), so the stash tests stay purely in-memory.
fn restorable_state() -> SessionState {
    SessionState {
        version: SESSION_VERSION,
        active: Some(0),
        tabs: vec![SessionTab {
            title: "vim".to_owned(),
            title_override: None,
            shell: "/bin/sh".to_owned(),
            cwd: None,
            profile: None,
            pinned: false,
        }],
    }
}

#[test]
fn restore_stash_hands_its_plan_to_the_first_run_that_can_spawn() {
    let mut stash = RestoreStash::new(Some(&restorable_state()), None, &[]);
    assert_eq!(
        stash.take_for_run(true),
        Some(RestorePlan {
            tabs: vec![SpawnSpec {
                title: "vim".to_owned(),
                title_override: None,
                shell: "/bin/sh".to_owned(),
                cwd: None,
                profile: None,
                pinned: false,
            }],
            active: 0,
        })
    );
}

#[test]
fn restore_stash_hands_nothing_to_later_runs() {
    // The plan is consumed exactly once: every later spawn run
    // spawns plain default tabs.
    let mut stash = RestoreStash::new(Some(&restorable_state()), None, &[]);
    assert!(stash.take_for_run(true).is_some());
    assert_eq!(stash.take_for_run(true), None);
}

#[test]
fn restore_stash_stays_armed_through_runs_that_cannot_spawn() {
    // The first spawn run can arrive before the terminal event
    // channel exists (show() outracing TermEventTx): it must not
    // eat the session, or the startup race would be lost.
    let mut stash = RestoreStash::new(Some(&restorable_state()), None, &[]);
    assert_eq!(stash.take_for_run(false), None);
    assert!(stash.take_for_run(true).is_some());
}

#[test]
fn restore_stash_starts_empty_without_a_restorable_session() {
    // A missing file, and a parsed-but-tab-less session, both
    // stash nothing: startup stays on the fresh single-tab path.
    let mut missing = RestoreStash::new(None, None, &[]);
    assert_eq!(missing.take_for_run(true), None);

    let empty = SessionState {
        version: SESSION_VERSION,
        active: None,
        tabs: vec![],
    };
    let mut empty_stash = RestoreStash::new(Some(&empty), None, &[]);
    assert_eq!(empty_stash.take_for_run(true), None);
}

#[test]
fn restore_outcome_clamps_the_saved_active_onto_the_survivors() {
    // Some spawns failed: the saved focus index must not point
    // past the tabs that exist. A full spawn keeps it as planned.
    assert_eq!(
        restore_outcome(3, 2),
        RestoreOutcome::Restored { active: 1 }
    );
    assert_eq!(
        restore_outcome(1, 3),
        RestoreOutcome::Restored { active: 1 }
    );
}

#[test]
fn restore_outcome_falls_back_when_no_tab_spawned() {
    assert_eq!(restore_outcome(0, 0), RestoreOutcome::Fallback);
}

#[test]
fn session_save_arms_one_window_and_owes_the_write_when_due() {
    let mut save = SessionSave::default();
    let generation = save.mark().expect("the first mark arms a window");
    assert!(save.due(generation), "the armed window owes one write");
}

#[test]
fn session_save_marks_inside_an_armed_window_coalesce_into_one_write() {
    let mut save = SessionSave::default();
    let generation = save.mark().expect("the first mark arms a window");
    assert_eq!(save.mark(), None, "no second timer while one is armed");
    assert!(
        save.due(generation),
        "one write covers every coalesced mark"
    );
}

#[test]
fn session_save_re_arms_after_a_write() {
    let mut save = SessionSave::default();
    let first = save.mark().expect("the first mark arms a window");
    assert!(save.due(first), "the write happened");
    let second = save.mark().expect("a mark after a write re-arms");
    assert_ne!(first, second, "the new window is a new generation");
    assert!(save.due(second), "the re-armed window owes its write");
}

#[test]
fn session_save_treats_a_stale_generation_as_a_no_op() {
    let mut save = SessionSave::default();
    let armed = save.mark().expect("the first mark arms a window");
    assert!(
        !save.due(armed.wrapping_add(1)),
        "a stale generation owes nothing"
    );
    assert!(save.due(armed), "the still-armed window writes");
}

#[test]
fn cwd_changed_is_quiet_while_the_directory_holds() {
    assert!(!cwd_changed(
        Some(Path::new("/tmp")),
        Some(Path::new("/tmp"))
    ));
    assert!(!cwd_changed(None, None));
}

#[test]
fn cwd_changed_marks_on_any_difference() {
    assert!(cwd_changed(
        Some(Path::new("/tmp")),
        Some(Path::new("/home"))
    ));
    assert!(cwd_changed(None, Some(Path::new("/tmp"))));
    assert!(cwd_changed(Some(Path::new("/tmp")), None));
}

// --- the symmetric toggle gate ---

#[test]
fn restore_entry_enabled_stashes_a_valid_sessions_plan() {
    let dir = TempDir::new("restore-entry-on");
    let path = session_file(&dir);
    write_session(
        &path,
        &ron::to_string(&restorable_state()).expect("serialize"),
    );
    let mut stash = restore_entry(&path, None, true, &[]);
    assert!(stash.take_for_run(true).is_some());
}

#[test]
fn restore_entry_disabled_skips_even_loading_the_file() {
    // The file policy's hidden edge: `load` quarantines invalid
    // files, so gating only the restore would still let a
    // disabled run rename the waiting file away. Skipping the
    // read keeps disabling totally inert: the corrupt file stays
    // put, no backup appears, nothing restores — it all waits
    // for a future re-enable.
    let dir = TempDir::new("restore-entry-off");
    let path = session_file(&dir);
    write_session(&path, "corrupt((");
    let mut stash = restore_entry(&path, None, false, &[]);
    assert_eq!(stash.take_for_run(true), None, "disabled restores nothing");
    assert!(path.exists(), "the state file is left untouched");
    assert!(!backup_of(&path).exists(), "no quarantine while disabled");
}

#[test]
fn disabled_save_triggers_arm_no_window() {
    // The save-trigger half of the symmetric gate: a disabled
    // trigger no-ops — and leaves no zombie window behind, so
    // the next enabled trigger arms fresh.
    let mut save = SessionSave::default();
    assert_eq!(save.mark_gated(false), None, "a disabled trigger no-ops");
    assert!(save.mark_gated(true).is_some(), "no zombie window armed");
}

#[test]
fn enabled_save_triggers_match_the_plain_mark() {
    // Enabled, the gated path is the machine the existing tests
    // pin.
    let mut gated = SessionSave::default();
    let mut plain = SessionSave::default();
    assert_eq!(gated.mark_gated(true), plain.mark());
}

#[test]
fn a_window_armed_before_the_flip_is_stopped_at_its_write() {
    // Mid-run disable stops further saves immediately: the
    // machine state still clears (a stale window must never
    // block a later one), but the pending write itself is
    // stopped.
    let mut save = SessionSave::default();
    let generation = save.mark_gated(true).expect("armed while enabled");
    assert!(
        !save.due_writes(generation, false),
        "the flip stops the pending write"
    );
    assert!(
        save.mark_gated(true).is_some(),
        "the machine is clear for a new window"
    );
}

#[test]
fn due_writes_enabled_matches_the_plain_due() {
    let mut gated = SessionSave::default();
    let mut plain = SessionSave::default();
    let g = gated.mark_gated(true).expect("armed");
    let p = plain.mark().expect("armed");
    assert_eq!(gated.due_writes(g, true), plain.due(p));
}
