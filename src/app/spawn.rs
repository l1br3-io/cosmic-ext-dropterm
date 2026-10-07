// SPDX-License-Identifier: GPL-3.0-only

use std::path::PathBuf;

use crate::config::DropTermConfig;
use crate::profile;
use crate::tabs::TabId;
use crate::terminal::Terminal;

/// What a new tab runs: the user's shell — config-resolved or per-tab
/// recorded — paired with the config's shell arguments, an explicit
/// whitespace-split command line whose exit closes the tab and whose
/// tabs are session-transient, or a profile: a named preset resolved
/// before spawn so the tab can record the id it launched under.
/// Profile tabs persist in the session and re-resolve by id at
/// restore; that is the explicit opt-in a raw `--cmd` tab
/// deliberately is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SpawnCommand {
    Shell(String),
    CommandLine(String),
    /// The resolved preset: the id (recorded on the tab, saved by the
    /// session) plus the command line and cwd the id resolves to at
    /// spawn time. Handed to [`DropTerm::spawn_shell`] already
    /// resolved — spawn applies a profile, it never looks one up.
    Profile {
        name: String,
        command: String,
        cwd: Option<PathBuf>,
    },
}

/// One freshly spawned terminal plus the tab-level facts recorded
/// beside it at spawn: the id the terminal is tagged with (allocated
/// only once the spawn is actually attempted — a deferred spawn never
/// burns one, so `Terminal N` numbering cannot drift), the shell
/// actually running (a command line's program, not the config shell),
/// the profile id the spawn ran under — set only when the preset's
/// command line was runnable — and the session-ephemeral flag a
/// command-line spawn carries. [`DropTerm::spawn_shell`] wraps this
/// into a [`Tab`] and records the facts on it.
pub(super) struct SpawnedTerminal {
    pub(super) tab_id: TabId,
    pub(super) terminal: Terminal,
    pub(super) shell: String,
    pub(super) profile_id: Option<String>,
    pub(super) ephemeral: bool,
}

/// Resolves the effective default for a plain new tab (ruling 1): a
/// set, existing `default_profile` becomes the full profile spawn —
/// id recorded, session-persisted — while an unset or dangling one
/// keeps today's plain shell (the module-1 seam's contract: the load
/// never rewrites the config, and nothing chains onto another
/// profile). The plain-spawn sites — the new-tab keybind, a bare
/// remote `new-tab`, and the never-empty startup/respawn — all route
/// through here; `duplicate-tab` and every explicit CLI flag do not.
pub(super) fn default_spawn_command(config: &DropTermConfig) -> SpawnCommand {
    match profile::effective_default(&config.profiles, config.default_profile.as_deref()) {
        Some(profile) => SpawnCommand::Profile {
            name: profile.name.clone(),
            command: profile.command.clone(),
            cwd: profile.cwd.clone(),
        },
        None => SpawnCommand::Shell(config.resolved_shell()),
    }
}

/// Resolves a context-menu profile pick — the submenu's index into
/// the config's profile list — into the spawn it runs: the Profile
/// arm with the preset's stored cwd (the call site composes the live
/// filter and inheritance on top). `None` when the index is stale, a
/// config edited while the menu was open: the caller warns and no-ops.
pub(super) fn profile_pick(config: &DropTermConfig, index: usize) -> Option<SpawnCommand> {
    config
        .profiles
        .get(index)
        .map(|profile| SpawnCommand::Profile {
            name: profile.name.clone(),
            command: profile.command.clone(),
            cwd: profile.cwd.clone(),
        })
}

/// The live-directory filter a profile preset's cwd passes at spawn:
/// dead → warn and `None` — the `--cwd` degradation, never an error
/// surface — so the inherited/default directory applies instead.
pub(super) fn live_preset_cwd(name: &str, cwd: Option<&PathBuf>) -> Option<PathBuf> {
    cwd.cloned().filter(|dir| {
        let live = dir.is_dir();
        if !live {
            log::warn!(
                "ignoring profile {name} cwd {}: not a directory",
                dir.display()
            );
        }
        live
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- the default-profile application policy ---

    fn test_config(profiles: &[profile::Profile], default_profile: Option<&str>) -> DropTermConfig {
        DropTermConfig {
            shell: Some("/bin/sh".to_string()),
            profiles: profiles.to_vec(),
            default_profile: default_profile.map(str::to_owned),
            ..DropTermConfig::default()
        }
    }

    fn named_profile(name: &str, command: &str, cwd: Option<PathBuf>) -> profile::Profile {
        profile::Profile {
            name: name.to_owned(),
            command: command.to_owned(),
            cwd,
        }
    }

    #[test]
    fn default_spawn_command_is_the_shell_without_a_configured_default() {
        // Unset default (every default config): plain shell, exactly
        // today's plain-spawn behavior.
        let config = test_config(&[named_profile("ssh", "ssh web", None)], None);
        assert_eq!(
            default_spawn_command(&config),
            SpawnCommand::Shell("/bin/sh".to_string())
        );
    }

    #[test]
    fn default_spawn_command_resolves_a_set_and_existing_default_into_the_profile() {
        // A set, existing default turns every plain spawn into a full
        // profile tab (ruling 1): id, command, and cwd cloned from the
        // resolved profile — the spawn sites compose the live cwd
        // filter and inheritance on top.
        let config = test_config(
            &[
                named_profile("ssh", "ssh web", None),
                named_profile("top", "htop", None),
            ],
            Some("top"),
        );
        assert_eq!(
            default_spawn_command(&config),
            SpawnCommand::Profile {
                name: "top".to_string(),
                command: "htop".to_string(),
                cwd: None,
            }
        );
    }

    #[test]
    fn default_spawn_command_is_the_shell_on_a_dangling_default() {
        // The default names a profile the list no longer carries:
        // plain shell, never a config rewrite (the module-1 seam's
        // contract) — and never a chain onto another profile.
        let config = test_config(&[named_profile("ssh", "ssh web", None)], Some("ghost"));
        assert_eq!(
            default_spawn_command(&config),
            SpawnCommand::Shell("/bin/sh".to_string())
        );
    }

    #[test]
    fn profile_pick_resolves_the_submenu_index_into_the_profile_arm() {
        // The pick carries the preset verbatim; the call site composes
        // the live-cwd filter and inheritance on top.
        let config = test_config(
            &[
                named_profile("ssh", "ssh web", Some(PathBuf::from("/srv"))),
                named_profile("top", "htop", None),
            ],
            None,
        );
        assert_eq!(
            profile_pick(&config, 1),
            Some(SpawnCommand::Profile {
                name: "top".to_string(),
                command: "htop".to_string(),
                cwd: None,
            })
        );
        assert_eq!(
            profile_pick(&config, 0),
            Some(SpawnCommand::Profile {
                name: "ssh".to_string(),
                command: "ssh web".to_string(),
                cwd: Some(PathBuf::from("/srv")),
            })
        );
    }

    #[test]
    fn profile_pick_is_none_on_a_stale_submenu_index() {
        // The index came from the menu-open snapshot; a config edited
        // while the menu was open can leave it out of range — the
        // caller's warn + no-op, never a panic.
        let config = test_config(&[named_profile("ssh", "ssh web", None)], None);
        assert_eq!(profile_pick(&config, 1), None);
        assert_eq!(profile_pick(&config, usize::MAX), None);
    }
}
