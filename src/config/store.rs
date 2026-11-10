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
    let text =
        ron::to_string(value).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
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
/// twin of [`crate::session::state_file_path`]: only an absolute
/// `XDG_CONFIG_HOME` is honored (defaulting to `~/.config`), and the
/// directory is this build's app id ([`crate::app::APP_ID`]), so the
/// dev identity gets its own config files.
pub fn store_dir() -> PathBuf {
    // XDG base-directory specification: only an absolute
    // XDG_CONFIG_HOME counts; anything else (unset, empty, relative)
    // falls back to ~/.config.
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|base| base.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    match base {
        Some(base) => base.join(crate::app::APP_ID),
        None => PathBuf::from(".config").join(crate::app::APP_ID),
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
}
