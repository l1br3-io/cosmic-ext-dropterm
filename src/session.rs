// SPDX-License-Identifier: GPL-3.0-only

//! Session persistence: the versioned state file that survives restarts.
//!
//! Pure file layer, free of iced/app types: the RON schema
//! ([`SessionState`]), capture from per-tab snapshots, restore planning
//! with cwd re-validation, load with corrupt-file quarantine, and atomic
//! save (temp + fsync + rename). The app layer wires these into the tab
//! model; everything here is unit-testable headless.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The schema version this build writes and understands. Files carrying
/// any other version are quarantined, never guessed at (see [`load`]).
pub const SESSION_VERSION: u32 = 1;

/// The saved form of a whole session: the tab list in tab-bar order plus
/// the focused tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    /// Schema version; must equal [`SESSION_VERSION`] to restore.
    pub version: u32,
    /// The focused tab's index, clamped when restored.
    pub active: Option<usize>,
    /// The tabs in tab-bar order; index order is restore order.
    pub tabs: Vec<SessionTab>,
}

/// The saved form of one tab: everything needed to respawn it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionTab {
    /// Last shell-reported title (the tab bar text at save time).
    pub title: String,
    /// A user rename, if any; survives restore un-clobbered.
    pub title_override: Option<String>,
    /// The shell this tab ran, recorded when it was spawned.
    pub shell: String,
    /// Working directory at save time; `None` spawns the default.
    pub cwd: Option<PathBuf>,
}

/// A borrowed view of one live tab at save time. `cwd` is the raw
/// `working_directory()` readlink — [`capture`] applies the staleness
/// filter (kept only while still a directory).
#[derive(Debug, Clone, Copy)]
pub struct TabSnapshot<'a> {
    pub title: &'a str,
    pub title_override: Option<&'a str>,
    pub shell: &'a str,
    pub cwd: Option<&'a Path>,
    /// Whether the tab is session-transient (a remote `--cmd` run):
    /// skipped by [`capture`] — the session never
    /// resumes a program.
    pub ephemeral: bool,
}

/// Builds the saved state from per-tab snapshots, keeping each `cwd`
/// only while it is still a directory (Guake's staleness filter — a
/// vanished directory must not be recorded as a spawn target).
/// Session-transient tabs (`ephemeral`) are skipped, and `active`
/// remaps into the surviving list — a session never resumes a
/// program.
pub fn capture(snapshots: &[TabSnapshot<'_>], active: usize) -> SessionState {
    let mut tabs = Vec::new();
    let mut saved_active = 0usize;
    for (index, snap) in snapshots.iter().enumerate() {
        if snap.ephemeral {
            continue;
        }
        if index == active {
            saved_active = tabs.len();
        }
        tabs.push(SessionTab {
            title: snap.title.to_owned(),
            title_override: snap.title_override.map(str::to_owned),
            shell: snap.shell.to_owned(),
            cwd: snap.cwd.filter(|cwd| cwd.is_dir()).map(Path::to_owned),
        });
    }
    SessionState {
        version: SESSION_VERSION,
        active: Some(saved_active),
        tabs,
    }
}

/// One tab's resolved spawn parameters, post-validation.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnSpec {
    pub title: String,
    pub title_override: Option<String>,
    pub shell: String,
    /// Re-validated at plan time: the saved cwd while still a directory,
    /// else `$HOME`, else `None` (spawn default).
    pub cwd: Option<PathBuf>,
}

/// A valid restore plan: one [`SpawnSpec`] per saved tab, in order, plus
/// the clamped active index.
#[derive(Debug, Clone, PartialEq)]
pub struct RestorePlan {
    pub tabs: Vec<SpawnSpec>,
    pub active: usize,
}

/// Turns a loaded state into per-tab [`SpawnSpec`]s: each saved cwd is
/// re-validated (dead → `home`, missing `home` → `None`) and `active`
/// is clamped into range. Returns `None` for an empty tab list — the
/// caller falls back to the fresh single-tab startup. Pure: `home` is
/// a parameter, not an environment read.
pub fn plan_restore(state: &SessionState, home: Option<&Path>) -> Option<RestorePlan> {
    if state.tabs.is_empty() {
        return None;
    }
    let tabs = state
        .tabs
        .iter()
        .map(|tab| SpawnSpec {
            title: tab.title.clone(),
            title_override: tab.title_override.clone(),
            shell: tab.shell.clone(),
            cwd: tab
                .cwd
                .as_deref()
                .filter(|cwd| cwd.is_dir())
                .or(home)
                .map(Path::to_owned),
        })
        .collect();
    Some(RestorePlan {
        tabs,
        active: state.active.unwrap_or(0).min(state.tabs.len() - 1),
    })
}

/// The startup session stash: the restore plan for the state `init()`
/// loaded from the session file, handed to the first spawn run that
/// can use it. Pure bookkeeping — the app performs the real spawns —
/// so the consumption decision stays unit-testable headless.
#[derive(Debug, Clone, Default)]
pub struct RestoreStash {
    plan: Option<RestorePlan>,
}

impl RestoreStash {
    /// Stashes the restore plan for a loaded state: an absent,
    /// invalid, or tab-less session stashes nothing, leaving startup
    /// on the fresh single-tab path.
    pub fn new(state: Option<SessionState>, home: Option<&Path>) -> Self {
        Self {
            plan: state.as_ref().and_then(|state| plan_restore(state, home)),
        }
    }

    /// Takes the plan for one spawn-funnel run: the first run that
    /// can spawn (its terminal event channel exists) consumes the
    /// plan, exactly once; every later run — and every run before the
    /// channel arrives — gets `None` and spawns plain default tabs.
    /// A cannot-spawn run leaves the stash armed, so whichever
    /// startup message reaches the funnel first, the session wins
    /// exactly once and nothing is spawned-then-deleted.
    pub fn take_for_run(&mut self, can_spawn: bool) -> Option<RestorePlan> {
        if can_spawn { self.plan.take() } else { None }
    }
}

/// What a spawn run that consumed a plan does once its spawns
/// settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// Keep the spawned tabs and focus the saved tab, clamped onto
    /// the survivors when some spawns failed.
    Restored { active: usize },
    /// Every spawn failed: fall back to the single-default-tab
    /// startup.
    Fallback,
}

/// Decides a consumed plan's outcome from how many of its tabs
/// actually spawned: zero survivors fall back, otherwise the saved
/// active index clamps onto the spawned count.
pub fn restore_outcome(planned_active: usize, spawned: usize) -> RestoreOutcome {
    if spawned == 0 {
        RestoreOutcome::Fallback
    } else {
        RestoreOutcome::Restored {
            active: planned_active.min(spawned - 1),
        }
    }
}

/// How long after a change settles before the session file is
/// rewritten: long enough that rapid
/// tab churn (open/close/reorder bursts, a `cd` loop) coalesces into
/// one write, short enough that a quit right after a change reopens
/// the change.
pub const SESSION_WRITE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// State of the debounced session save: pure bookkeeping over
/// generations — the same shape as `geometry::WriteBack`, adapted to
/// dirty-flag semantics. The wait itself is a
/// `tokio::time::sleep` the app tasks in, so this machine needs no
/// clock and stays headless-testable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionSave {
    /// Source of monotonically increasing window generations, never
    /// reused within a run.
    next_generation: u32,
    /// The generation whose debounce window is armed, if any. At most
    /// one window is ever armed — marks inside it coalesce — so no
    /// sleeps stack and no window is starved.
    armed: Option<u32>,
    /// Whether a change is still unwritten; taken by the armed
    /// window's `due`, which writes everything changed up to that
    /// moment (capture happens at write time, not schedule time).
    dirty: bool,
}

impl SessionSave {
    /// Marks the session dirty and arms a debounce window if none is
    /// armed, returning the generation its `SessionSaveDue` message
    /// must carry. `None` when a window is already armed: the pending
    /// write covers this change too (trailing semantics), so the
    /// caller must not spawn another sleep.
    pub fn mark(&mut self) -> Option<u32> {
        self.dirty = true;
        if self.armed.is_none() {
            let generation = self.next_generation;
            self.next_generation += 1;
            self.armed = Some(generation);
            return Some(generation);
        }
        None
    }

    /// Disarms the window when `generation` still owns it and returns
    /// whether a write is owed. `false` for a stale generation — the
    /// fire of an older window, or a duplicate delivery after the
    /// write — which must not write or disarm.
    pub fn due(&mut self, generation: u32) -> bool {
        if self.armed == Some(generation) {
            self.armed = None;
            std::mem::take(&mut self.dirty)
        } else {
            false
        }
    }

    /// The toggle-gated save trigger: a disabled trigger is a total
    /// no-op — nothing marks dirty, no debounce window arms, no timer
    /// task spawns — so a mid-run disable stops further saves
    /// immediately. Enabled, this is exactly [`SessionSave::mark`].
    pub fn mark_gated(&mut self, enabled: bool) -> Option<u32> {
        if enabled { self.mark() } else { None }
    }

    /// The toggle-gated write decision: the machine state always
    /// clears exactly as [`SessionSave::due`]
    /// would — a stale window must never block a later one — but a
    /// disabled run stops the pending write itself, so the file last
    /// written before the flip stays put.
    pub fn due_writes(&mut self, generation: u32, enabled: bool) -> bool {
        self.due(generation) && enabled
    }
}

/// Whether a terminal's working directory moved since it was last
/// seen: the change gate on title events — shells report titles far
/// more often than cwds change, so
/// only a moved cwd turns a title event into a save trigger (Guake's
/// change-gated cadence). `None` on either side is just another
/// value: a failed readlink reads as a change, and two failures in a
/// row read as quiet.
pub fn cwd_changed(last: Option<&Path>, current: Option<&Path>) -> bool {
    last != current
}

/// The gated restore entry the app's `init` calls: the toggle is
/// decided here, before the file is ever touched. Disabled, the read
/// is skipped
/// entirely — not merely the restore — because `load` quarantines
/// invalid files, and a disabled run must leave even a corrupt state
/// file exactly where it waits (no `.bak`, no fresh start) for a
/// future launch with the key back on.
pub fn restore_entry(path: &Path, home: Option<&Path>, enabled: bool) -> RestoreStash {
    if !enabled {
        return RestoreStash::default();
    }
    RestoreStash::new(load(path), home)
}

/// Loads the session file. A missing file is a silent `None` (first
/// run); anything invalid — unreadable, unparseable, non-struct, or a
/// version other than [`SESSION_VERSION`] — is quarantined by renaming
/// the file to `session.ron.bak` (replacing an older backup) with a
/// `warn` log, and yields `None`. Never panics on bad input.
pub fn load(path: &Path) -> Option<SessionState> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        // A missing file is the silent first run, not an error.
        Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
        Err(err) => {
            log::warn!("failed to read session file {}: {err}", path.display());
            return quarantine(path);
        }
    };
    match ron::from_str::<SessionState>(&text) {
        Ok(state) if state.version == SESSION_VERSION => Some(state),
        Ok(state) => {
            log::warn!(
                "session file {} has unsupported version {}; starting fresh",
                path.display(),
                state.version
            );
            quarantine(path)
        }
        Err(err) => {
            log::warn!("failed to parse session file {}: {err}", path.display());
            quarantine(path)
        }
    }
}

/// Moves an invalid session file aside so the next save cannot be
/// blocked by (or silently merge with) the bad file.
fn quarantine(path: &Path) -> Option<SessionState> {
    let backup = sibling(path, ".bak");
    match fs::rename(path, &backup) {
        // A failed rename is logged and survived: the bad file stays,
        // the app still starts fresh, and the next save overwrites it.
        Ok(()) => {}
        Err(err) => log::warn!(
            "failed to move session file {} aside: {err}",
            path.display()
        ),
    }
    None
}

/// Atomically writes `state` to `path` (KConfig `QSaveFile` semantics):
/// serialize to a `.tmp` sibling in the same directory, fsync it, rename
/// over the target, then best-effort fsync the parent directory so the
/// rename itself survives a crash. Parent directories are created on
/// demand.
///
/// # Errors
///
/// Returns the underlying [`io::Error`] when serialization, directory
/// creation, writing, fsync, or the rename fails; a failure at any
/// point leaves any previous file intact.
pub fn save_atomic(path: &Path, state: &SessionState) -> std::io::Result<()> {
    let text =
        ron::to_string(state).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = sibling(path, ".tmp");
    {
        let mut file = File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    // Best-effort fsync of the directory so the rename itself is
    // durable; filesystems that reject directory fsync just log.
    if let Some(parent) = path.parent()
        && let Err(err) = File::open(parent).and_then(|dir| dir.sync_all())
    {
        log::warn!("failed to fsync session dir {}: {err}", parent.display());
    }
    Ok(())
}

/// `path` with `suffix` appended to its whole file name
/// (`session.ron` + `".bak"` → `session.ron.bak`).
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// The XDG state directory this app's session file lives in, named
/// after the single-instance app id (must stay in sync with
/// [`crate::app::APP_ID`], which owns the canonical value).
const APP_ID: &str = "io.github.l1br3_io.CosmicExtDropterm";

/// `$XDG_STATE_HOME/io.github.l1br3_io.CosmicExtDropterm/session.ron`,
/// defaulting to `~/.local/state/…` per the XDG base-directory
/// specification (only an absolute `XDG_STATE_HOME` is honored).
pub fn state_file_path() -> PathBuf {
    // XDG base-directory specification: only an absolute XDG_STATE_HOME
    // counts; anything else (unset, empty, relative) falls back to
    // ~/.local/state.
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|base| base.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")));
    match base {
        Some(base) => base.join(APP_ID).join("session.ron"),
        None => PathBuf::from(".local/state")
            .join(APP_ID)
            .join("session.ron"),
    }
}

#[cfg(test)]
mod tests {
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
            }],
        };
        let text = ron::to_string(&state).expect("serialize");
        let back: SessionState = ron::from_str(&text).expect("deserialize");
        assert_eq!(back, state);
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
            },
            TabSnapshot {
                title: "htop",
                title_override: None,
                shell: "htop",
                cwd: None,
                ephemeral: true,
            },
            TabSnapshot {
                title: "src",
                title_override: None,
                shell: "/bin/zsh",
                cwd: None,
                ephemeral: false,
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
        }];
        let state = capture(&snapshots, 0);
        assert!(state.tabs.is_empty());
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
            },
            TabSnapshot {
                title: "dead",
                title_override: None,
                shell: "/bin/sh",
                cwd: Some(&dead_path()),
                ephemeral: false,
            },
            TabSnapshot {
                title: "unset",
                title_override: None,
                shell: "/bin/sh",
                cwd: None,
                ephemeral: false,
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
        }];
        let state = capture(&items, 0);
        assert_eq!(state.tabs[0].title, "vim");
        assert_eq!(state.tabs[0].title_override.as_deref(), Some("work"));
        assert_eq!(state.tabs[0].shell, "/bin/zsh");
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
                },
                SessionTab {
                    title: "dead".to_owned(),
                    title_override: Some("work".to_owned()),
                    shell: "/bin/zsh".to_owned(),
                    cwd: Some(dead_path()),
                },
            ],
        };
        let plan = plan_restore(&state, Some(&home)).expect("valid plan");
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
            }],
        };
        let plan = plan_restore(&state, None).expect("valid plan");
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
                },
                SessionTab {
                    title: "b".to_owned(),
                    title_override: None,
                    shell: "/bin/sh".to_owned(),
                    cwd: None,
                },
            ],
        };
        let plan = plan_restore(&state, None).expect("valid plan");
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
            }],
        };
        let plan = plan_restore(&state, None).expect("valid plan");
        assert_eq!(plan.active, 0);
    }

    #[test]
    fn plan_restore_rejects_an_empty_tab_list() {
        let state = SessionState {
            version: SESSION_VERSION,
            active: None,
            tabs: vec![],
        };
        assert!(plan_restore(&state, None).is_none());
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
            }],
        }
    }

    #[test]
    fn restore_stash_hands_its_plan_to_the_first_run_that_can_spawn() {
        let mut stash = RestoreStash::new(Some(restorable_state()), None);
        assert_eq!(
            stash.take_for_run(true),
            Some(RestorePlan {
                tabs: vec![SpawnSpec {
                    title: "vim".to_owned(),
                    title_override: None,
                    shell: "/bin/sh".to_owned(),
                    cwd: None,
                }],
                active: 0,
            })
        );
    }

    #[test]
    fn restore_stash_hands_nothing_to_later_runs() {
        // The plan is consumed exactly once: every later spawn run
        // spawns plain default tabs.
        let mut stash = RestoreStash::new(Some(restorable_state()), None);
        assert!(stash.take_for_run(true).is_some());
        assert_eq!(stash.take_for_run(true), None);
    }

    #[test]
    fn restore_stash_stays_armed_through_runs_that_cannot_spawn() {
        // The first spawn run can arrive before the terminal event
        // channel exists (show() outracing TermEventTx): it must not
        // eat the session, or the startup race would be lost.
        let mut stash = RestoreStash::new(Some(restorable_state()), None);
        assert_eq!(stash.take_for_run(false), None);
        assert!(stash.take_for_run(true).is_some());
    }

    #[test]
    fn restore_stash_starts_empty_without_a_restorable_session() {
        // A missing file, and a parsed-but-tab-less session, both
        // stash nothing: startup stays on the fresh single-tab path.
        let mut missing = RestoreStash::new(None, None);
        assert_eq!(missing.take_for_run(true), None);

        let empty = SessionState {
            version: SESSION_VERSION,
            active: None,
            tabs: vec![],
        };
        let mut empty_stash = RestoreStash::new(Some(empty), None);
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
        let mut stash = restore_entry(&path, None, true);
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
        let mut stash = restore_entry(&path, None, false);
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
}
