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

use crate::profile::{Profile, find};

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
    /// The shell this tab ran, recorded when it was spawned. For a
    /// profile tab this is the program path (uniform capture);
    /// restore ignores it when the profile id resolves.
    pub shell: String,
    /// Working directory at save time; `None` spawns the default.
    pub cwd: Option<PathBuf>,
    /// The profile id this tab launched under, if any: re-resolved
    /// against the current config at restore, so a profile edited
    /// between save and restart launches the edited command. Absent
    /// in files written before profiles existed; plain-shell and
    /// `--cmd` tabs save `None`.
    pub profile: Option<String>,
    /// Whether this tab was pinned at save time. Absent in files
    /// written before pins existed — `serde(default)` loads them
    /// unpinned — and an old build ignores the field in a new file
    /// (the `profile` field's additive precedent).
    #[serde(default)]
    pub pinned: bool,
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
    /// The profile id the tab launched under, if any — saved verbatim
    /// by [`capture`]; restore re-resolves it against the live
    /// profile list.
    pub profile: Option<&'a str>,
    /// Whether the tab is pinned — saved verbatim by [`capture`] and
    /// replayed onto the restored tab by the plan.
    pub pinned: bool,
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
            profile: snap.profile.map(str::to_owned),
            pinned: snap.pinned,
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
    /// The profile the tab's saved id re-resolved to, if it still
    /// exists: the *current* definition (command and cwd) governs the
    /// respawn. `None` for plain-shell tabs — and for a saved id that
    /// no longer resolves, which spawns the plain shell with no
    /// chaining through any default profile.
    pub profile: Option<Profile>,
    /// The saved pin, replayed onto the restored tab: a pinned tab
    /// comes back pinned.
    pub pinned: bool,
}

/// A valid restore plan: one [`SpawnSpec`] per saved tab, in order, plus
/// the clamped active index.
#[derive(Debug, Clone, PartialEq)]
pub struct RestorePlan {
    pub tabs: Vec<SpawnSpec>,
    pub active: usize,
}

/// Turns a loaded state into per-tab [`SpawnSpec`]s: profile ids
/// re-resolve against `profiles` — the *current* definition wins, so
/// an edit between save and restore launches the edited command —
/// each saved cwd is re-validated (dead → the profile's live cwd →
/// `home`, missing `home` → `None`) and `active` is clamped into
/// range. Returns `None` for an empty tab list — the caller falls
/// back to the fresh single-tab startup. Pure: `home` and the
/// profile list are parameters, not environment reads.
pub fn plan_restore(
    state: &SessionState,
    home: Option<&Path>,
    profiles: &[Profile],
) -> Option<RestorePlan> {
    if state.tabs.is_empty() {
        return None;
    }
    let tabs = state
        .tabs
        .iter()
        .map(|tab| {
            // Re-resolve by id: the *current* definition wins, so a
            // profile edited between save and restore launches the
            // edited command; a missing id is the plain shell — no
            // chaining through any default (the session records the
            // id it launched under, and that id alone).
            let profile = tab.profile.as_deref().and_then(|id| find(profiles, id));
            SpawnSpec {
                title: tab.title.clone(),
                title_override: tab.title_override.clone(),
                shell: tab.shell.clone(),
                pinned: tab.pinned,
                // The familiar place: the saved cwd while still a
                // directory, else the (current) profile's cwd while
                // still a directory, else `$HOME`, else `None`.
                cwd: tab
                    .cwd
                    .as_deref()
                    .filter(|cwd| cwd.is_dir())
                    .or_else(|| {
                        profile
                            .and_then(|profile| profile.cwd.as_deref())
                            .filter(|cwd| cwd.is_dir())
                    })
                    .or(home)
                    .map(Path::to_owned),
                profile: profile.cloned(),
            }
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
    /// Stashes the restore plan for a loaded state — re-resolving
    /// saved profile ids against `profiles` (see [`plan_restore`]) —
    /// for the first spawn run that can use it. An absent, invalid,
    /// or tab-less session stashes nothing, leaving startup on the
    /// fresh single-tab path.
    pub fn new(state: Option<&SessionState>, home: Option<&Path>, profiles: &[Profile]) -> Self {
        Self {
            plan: state.and_then(|state| plan_restore(state, home, profiles)),
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
/// decided here, before the file is ever touched, and saved profile
/// ids re-resolve against `profiles` — the loaded config, so a
/// profile edited between sessions wins. Disabled, the read is
/// skipped entirely — not merely the restore — because `load`
/// quarantines invalid files, and a disabled run must leave even a
/// corrupt state file exactly where it waits (no `.bak`, no fresh
/// start) for a future launch with the key back on.
pub fn restore_entry(
    path: &Path,
    home: Option<&Path>,
    enabled: bool,
    profiles: &[Profile],
) -> RestoreStash {
    if !enabled {
        return RestoreStash::default();
    }
    RestoreStash::new(load(path).as_ref(), home, profiles)
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

/// Atomically writes `state` to `path` (`KConfig` `QSaveFile` semantics):
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
mod tests;
