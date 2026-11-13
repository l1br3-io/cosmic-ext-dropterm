// SPDX-License-Identifier: GPL-3.0-only

//! The plain-RON config store (config-store spec): one file per
//! writer-section under `$XDG_CONFIG_HOME/<APP_ID>/`, replacing the
//! cosmic-config namespace. Pure file layer on the session-file
//! pattern ([`crate::session`]): load with corrupt-file quarantine,
//! atomic save (tmp + fsync + rename), parent directories created on
//! demand. Generic over the section type; the sections own their
//! shape.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

/// The settings section's file name inside the store directory.
pub const SETTINGS_FILE: &str = "config.ron";
/// The profiles section's file name inside the store directory.
pub const PROFILES_FILE: &str = "profiles.ron";
/// The keybinds section's file name inside the store directory.
pub const KEYBINDS_FILE: &str = "keybinds.ron";

/// Loads one section file. A missing file is a silent `None` (first
/// run); anything unreadable or unparseable is quarantined by
/// renaming the file to a `.bak` sibling (replacing an older backup)
/// with a `warn` log, and yields `None`. Never panics on bad input.
pub fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        // A missing file is the silent first run, not an error.
        Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
        Err(err) => {
            log::warn!("failed to read config section {}: {err}", path.display());
            return quarantine(path);
        }
    };
    match ron::from_str::<T>(&text) {
        Ok(section) => Some(section),
        Err(err) => {
            log::warn!("failed to parse config section {}: {err}", path.display());
            quarantine(path)
        }
    }
}

/// Reads one section file without ever touching it — the live
/// path's load, for a store being edited while the app runs: `None`
/// when the file is missing, unreadable, or does not parse *right
/// now*, which is all one answer to a watcher (an editor mid-write
/// passes through each of those states). Unlike [`load`] nothing is
/// quarantined: a half-written file is not a corrupt one, and moving
/// it aside would take it from the editor writing it.
pub fn peek<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    match ron::from_str::<T>(&text) {
        Ok(section) => Some(section),
        Err(err) => {
            // `debug`, not `warn`: transient by design, and a
            // non-atomic save would otherwise log on every write.
            log::debug!(
                "config section {} does not parse yet: {err}",
                path.display()
            );
            None
        }
    }
}

/// Atomically writes one section (`QSaveFile` semantics, the
/// session-file pattern): serialize to a `.tmp` sibling in the same
/// directory, fsync it, rename over the target, then best-effort
/// fsync the parent directory so the rename itself survives a crash.
/// Parent directories are created on demand.
///
/// # Errors
///
/// Returns the underlying [`io::Error`] when serialization, directory
/// creation, writing, fsync, or the rename fails; a failure at any
/// point leaves any previous file intact.
pub fn save_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    // Pretty RON, one key per line: these files are hand-edited, and
    // a save must keep them readable rather than rewrite a section
    // as one unbroken line. The trailing newline closes the file the
    // way a text editor expects.
    let mut text = ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    text.push('\n');
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
        log::warn!("failed to fsync config dir {}: {err}", parent.display());
    }
    Ok(())
}

/// Moves an invalid section file aside so the next save cannot be
/// blocked by (or silently merge with) the bad file.
fn quarantine<T>(path: &Path) -> Option<T> {
    let backup = sibling(path, ".bak");
    match fs::rename(path, &backup) {
        // A failed rename is logged and survived: the bad file stays,
        // the section still starts fresh, and the next save overwrites it.
        Ok(()) => {}
        Err(err) => log::warn!(
            "failed to move config section {} aside: {err}",
            path.display()
        ),
    }
    None
}

/// `path` with `suffix` appended to its whole file name
/// (`config.ron` + `".bak"` → `config.ron.bak`).
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// `$XDG_CONFIG_HOME/<APP_ID>` — the store directory, the config
/// twin of [`crate::session::state_file_path`], with cosmic-config's
/// Flatpak semantics preserved: inside a sandbox the HOST's config
/// is the truth (`HOST_XDG_CONFIG_HOME` when the runtime grants it,
/// the flatpak `HOME` otherwise), outside it the XDG base-directory
/// rules — only an absolute `XDG_CONFIG_HOME` counts, `~/.config`
/// otherwise. The directory is this build's app id
/// ([`crate::APP_ID`]), so the dev identity gets its own config
/// files.
pub fn store_dir() -> PathBuf {
    store_dir_with(
        std::env::var_os("FLATPAK_ID").is_some(),
        std::env::var_os("HOST_XDG_CONFIG_HOME"),
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

/// The resolution core of [`store_dir`], every input explicit so
/// the rules are table-testable: a Flatpak reads the host's config
/// (absolute `HOST_XDG_CONFIG_HOME` only — a relative value never
/// steers the store cwd-relative — falling to the flatpak HOME's
/// `.config`); otherwise absolute `XDG_CONFIG_HOME` only, falling
/// to HOME's `.config`. With neither variable set the relative
/// `.config/<APP_ID>` twin of `session::state_file_path` answers —
/// a session with no HOME has no absolute anything, and every
/// consumer treats a relative store the way the state file always
/// has.
fn store_dir_with(
    flatpak: bool,
    host_xdg: Option<std::ffi::OsString>,
    xdg: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> PathBuf {
    let home_config =
        |home: Option<std::ffi::OsString>| home.map(|home| PathBuf::from(home).join(".config"));
    let base = if flatpak {
        host_xdg
            .map(PathBuf::from)
            .filter(|base| base.is_absolute())
            .or_else(|| home_config(home))
    } else {
        xdg.map(PathBuf::from)
            .filter(|base| base.is_absolute())
            .or_else(|| home_config(home))
    };
    match base {
        Some(base) => base.join(crate::APP_ID),
        None => PathBuf::from(".config").join(crate::APP_ID),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use serde::Deserialize;

    use super::*;

    /// A per-test scratch directory under the system temp dir with a
    /// process- and test-unique name, removed (best-effort) on drop
    /// even when an assertion fails. Std only — no dev-dependency.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let unique = format!(
                "dropterm-config-store-{tag}-{}-{}",
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

    /// The stand-in section: everything the store asks of a type
    /// (serde plus equality) and nothing more.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
    struct Section {
        font_size: u16,
        note: Option<String>,
    }

    fn section_file(dir: &TempDir) -> PathBuf {
        dir.join(SETTINGS_FILE)
    }

    fn backup_of(path: &std::path::Path) -> PathBuf {
        sibling(path, ".bak")
    }

    #[test]
    fn the_store_dir_resolves_by_the_xdg_rules() {
        // The resolution table: absolute XDG wins, a relative XDG
        // never steers the store cwd-relative, HOME's `.config` is
        // the fallback, and a Flatpak reads the host's config under
        // the same absolute-only rule — a relative
        // `HOST_XDG_CONFIG_HOME` falls to the flatpak HOME like an
        // unset one.
        let app = PathBuf::from(crate::APP_ID);
        let os = |value: &str| Some(std::ffi::OsString::from(value));
        assert_eq!(
            store_dir_with(false, None, os("/custom/config"), os("/home/u")),
            PathBuf::from("/custom/config").join(&app)
        );
        assert_eq!(
            store_dir_with(false, None, os("relative/x"), os("/home/u")),
            PathBuf::from("/home/u/.config").join(&app),
            "a relative XDG_CONFIG_HOME is ignored"
        );
        assert_eq!(
            store_dir_with(false, None, None, os("/home/u")),
            PathBuf::from("/home/u/.config").join(&app)
        );
        assert_eq!(
            store_dir_with(false, None, None, None),
            PathBuf::from(".config").join(&app),
            "no HOME: the state file's relative twin"
        );
        assert_eq!(
            store_dir_with(
                true,
                os("/host/config"),
                os("/sandbox/config"),
                os("/sandbox/home")
            ),
            PathBuf::from("/host/config").join(&app),
            "inside a Flatpak the host's config wins over the sandbox's XDG"
        );
        assert_eq!(
            store_dir_with(true, os("relative/h"), None, os("/sandbox/home")),
            PathBuf::from("/sandbox/home/.config").join(&app),
            "a relative HOST_XDG_CONFIG_HOME is ignored"
        );
    }

    #[test]
    fn a_failed_save_leaves_the_previous_file_intact() {
        // The store's anti-data-loss half: when a save cannot
        // complete (here the tmp sibling blocked by a directory), it
        // errors and the previous file survives byte-identical — the
        // rename onto the target is the only step that ever replaces
        // it.
        let dir = TempDir::new("failed-save");
        let path = section_file(&dir);
        save_atomic(
            &path,
            &Section {
                font_size: 15,
                note: None,
            },
        )
        .expect("first save");
        let before = fs::read_to_string(&path).expect("read the standing file");
        fs::create_dir(sibling(&path, ".tmp")).expect("block the tmp sibling");
        assert!(
            save_atomic(
                &path,
                &Section {
                    font_size: 16,
                    note: None
                }
            )
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read after the failed save"),
            before,
            "the standing file is untouched"
        );
    }

    #[test]
    fn missing_file_loads_none() {
        let dir = TempDir::new("missing");
        assert_eq!(load::<Section>(&section_file(&dir)), None);
    }

    #[test]
    fn round_trip_preserves_the_section_and_leaves_no_temp() {
        let dir = TempDir::new("round-trip");
        let path = section_file(&dir);
        let section = Section {
            font_size: 15,
            note: Some("hi".to_owned()),
        };
        save_atomic(&path, &section).expect("save");
        assert_eq!(load(&path), Some(section));
        assert!(!sibling(&path, ".tmp").exists());
    }

    #[test]
    fn save_overwrites_an_existing_file() {
        let dir = TempDir::new("overwrite");
        let path = section_file(&dir);
        save_atomic(&path, &Section::default()).expect("first save");
        let second = Section {
            font_size: 99,
            note: None,
        };
        save_atomic(&path, &second).expect("second save");
        assert_eq!(load(&path), Some(second));
        assert!(!sibling(&path, ".tmp").exists());
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let dir = TempDir::new("parents");
        let path = dir.join("nested/deeper").join(SETTINGS_FILE);
        save_atomic(&path, &Section::default()).expect("save");
        assert!(path.is_file());
    }

    #[test]
    fn corrupt_file_is_quarantined_and_loads_none() {
        let dir = TempDir::new("corrupt");
        let path = section_file(&dir);
        fs::write(&path, "garbage((").expect("write corrupt section");
        assert_eq!(load::<Section>(&path), None);
        let backup = fs::read_to_string(backup_of(&path)).expect("backup exists");
        assert_eq!(backup, "garbage((");
    }

    #[test]
    fn quarantine_replaces_an_older_backup() {
        let dir = TempDir::new("rebackup");
        let path = section_file(&dir);
        fs::write(backup_of(&path), "older garbage").expect("write older backup");
        fs::write(&path, "newer garbage((").expect("write corrupt section");
        assert_eq!(load::<Section>(&path), None);
        let backup = fs::read_to_string(backup_of(&path)).expect("backup exists");
        assert_eq!(backup, "newer garbage((");
    }

    #[test]
    fn failed_quarantine_rename_still_loads_none() {
        let dir = TempDir::new("quarantine-fail");
        let path = section_file(&dir);
        fs::write(&path, "garbage((").expect("write corrupt section");
        // A directory where the backup would land makes the quarantine
        // rename fail (renaming a file over a directory is EISDIR):
        // the survival contract — start fresh anyway, bad file left.
        fs::create_dir(backup_of(&path)).expect("block the backup name");
        assert_eq!(load::<Section>(&path), None);
        assert_eq!(
            fs::read_to_string(&path).expect("bad file stays"),
            "garbage(("
        );
    }

    #[test]
    fn peek_reads_a_parseable_file() {
        let dir = TempDir::new("peek-ok");
        let path = section_file(&dir);
        let section = Section {
            font_size: 15,
            note: None,
        };
        save_atomic(&path, &section).expect("save");
        assert_eq!(peek(&path), Some(section));
    }

    #[test]
    fn peek_answers_none_for_a_missing_file() {
        let dir = TempDir::new("peek-missing");
        assert_eq!(peek::<Section>(&section_file(&dir)), None);
    }

    #[test]
    fn a_saved_section_is_one_key_per_line() {
        // The files are hand-edited: a save keeps them that way —
        // one key per line, a newline at the end — instead of one
        // unbroken line.
        let dir = TempDir::new("pretty");
        let path = section_file(&dir);
        save_atomic(
            &path,
            &Section {
                font_size: 15,
                note: None,
            },
        )
        .expect("save");
        let text = fs::read_to_string(&path).expect("read back");
        assert!(
            text.lines().any(|line| line.trim() == "font_size: 15,"),
            "no per-key line in: {text}"
        );
        assert!(text.ends_with('\n'), "no trailing newline: {text:?}");
    }

    #[test]
    fn peek_leaves_an_unreadable_file_in_place() {
        // The live path's whole difference from `load`: a file that
        // does not parse right now may be an editor mid-write, so it
        // is neither moved nor touched — no quarantine, no backup.
        let dir = TempDir::new("peek-unreadable");
        let path = section_file(&dir);
        fs::write(&path, "garbage((").expect("write half a section");
        assert_eq!(peek::<Section>(&path), None);
        assert_eq!(fs::read_to_string(&path).expect("file stays"), "garbage((");
        assert!(!backup_of(&path).exists(), "nothing was quarantined");
    }
}
