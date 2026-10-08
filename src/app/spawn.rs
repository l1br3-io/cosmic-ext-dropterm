// SPDX-License-Identifier: GPL-3.0-only

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::tty::{Options as PtyOptions, Shell as PtyShell};

use super::{DropTerm, colors_for};
use crate::config::DropTermConfig;
use crate::profile;
use crate::session;
use crate::tabs::{self, Tab, TabId};
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
    /// session) plus the command line, cwd, and env map the id
    /// resolves to at spawn time. Handed to [`DropTerm::spawn_shell`]
    /// already resolved — spawn applies a profile, it never looks one
    /// up.
    Profile {
        name: String,
        command: String,
        cwd: Option<PathBuf>,
        env: Option<BTreeMap<String, String>>,
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
            env: profile.env.clone(),
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
            env: profile.env.clone(),
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

/// The pty env a profile spawn applies: the preset's map in
/// `PtyOptions::env`'s shape, or empty when the preset carries none.
/// Called only from the runnable-preset branch of the Profile arm
/// below — the degraded blank-command-line branch records no
/// `profile_id` and applies no profile env either: a degraded tab
/// must not carry half a profile's identity. The merge over the
/// inherited environment is alacritty's own (the pty builder applies
/// each entry with `Command::env` at exec).
fn preset_env(map: Option<&BTreeMap<String, String>>) -> HashMap<String, String> {
    map.map(|entries| {
        entries
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    })
    .unwrap_or_default()
}

impl DropTerm {
    /// Spawns one terminal tab from a session restore's resolved
    /// spawn parameters ([`session::SpawnSpec`], with its recorded
    /// shell or re-resolved profile and re-validated cwd) — or, with
    /// `None`, from the config's resolved shell with the pty's
    /// default working directory.
    fn spawn_tab(&mut self, spec: Option<&session::SpawnSpec>) -> Option<Tab> {
        let Some(spec) = spec else {
            // The plain startup/respawn spawns ride the configured
            // default (ruling 1 — the never-empty respawn is the same
            // no-user-context plain spawn). Nothing to inherit here:
            // a default preset's dead cwd warns and degrades to the
            // pty's default directory.
            let command = default_spawn_command(&self.config);
            let cwd = match &command {
                SpawnCommand::Profile {
                    name, cwd: preset, ..
                } => live_preset_cwd(name, preset.as_ref()),
                _ => None,
            };
            return self.spawn_shell(&command, cwd);
        };
        if let Some(profile) = &spec.profile {
            // The id re-resolved at plan time: the current definition
            // runs — command and env included — and `spec.cwd` — the
            // ruled chain result — already is the spawn directory, so
            // the arm mirrors it and spawn re-derives nothing.
            self.spawn_shell(
                &SpawnCommand::Profile {
                    name: profile.name.clone(),
                    command: profile.command.clone(),
                    cwd: spec.cwd.clone(),
                    env: profile.env.clone(),
                },
                spec.cwd.clone(),
            )
        } else {
            let shell = spec.shell.clone();
            let cwd = spec.cwd.clone();
            self.spawn_shell(&SpawnCommand::Shell(shell), cwd)
        }
    }

    /// Spawns one terminal tab running `command` in `cwd` — the common
    /// path behind session restore ([`Self::spawn_tab`]), the new-tab
    /// keybind (which inherits the active tab's cwd), and the CLI's
    /// `new-tab` action. `cwd: None` means the pty's default directory.
    /// The shell is recorded per tab at spawn, so a mid-run config
    /// change never rewrites a live tab's shell. The spawn itself is
    /// [`Self::spawn_terminal`]'s; this wrapper only wraps the result
    /// into a [`Tab`] and seeds the cwd cache.
    pub(super) fn spawn_shell(
        &mut self,
        command: &SpawnCommand,
        cwd: Option<PathBuf>,
    ) -> Option<Tab> {
        let SpawnedTerminal {
            tab_id,
            terminal,
            shell,
            profile_id,
            ephemeral,
        } = self.spawn_terminal(command, cwd.as_deref())?;
        let mut tab = Tab::new(tab_id, shell, terminal);
        tab.ephemeral = ephemeral;
        // The profile id a tab launched under (if any), recorded beside
        // the ephemeral flag it is the opposite of: the session saves
        // one and never the other.
        tab.profile = profile_id;
        // Seed the cwd cache with the spawn cwd: a restored tab seeds
        // its re-validated saved cwd — so the fresh shell's first title
        // report reads as unchanged — and a default spawn seeds `None`;
        // an inherited cwd seeds the directory the tab opened in, for
        // the same reason.
        self.tab_cwds.insert(tab.id, cwd);
        Some(tab)
    }

    /// Spawns one terminal running `command` in `cwd` — the bottom
    /// half of every spawn path, below the [`Tab`] wrapping
    /// [`Self::spawn_shell`] adds: resolve the shell/args pair, build
    /// the pty options, allocate the tab's id, and construct the
    /// [`Terminal`] tagged with it. `cwd: None` means the pty's
    /// default directory. The id is allocated only once the spawn is
    /// actually attempted — after the channel guard — so a deferred
    /// spawn never burns one and `Terminal N` numbering cannot drift.
    /// Returns `None` the two ways the old single fn did: no terminal
    /// event channel yet (the caller defers), or `Terminal::new`
    /// erroring (logged).
    pub(super) fn spawn_terminal(
        &mut self,
        command: &SpawnCommand,
        cwd: Option<&Path>,
    ) -> Option<SpawnedTerminal> {
        let Some(tx) = self.term_event_tx.clone() else {
            log::warn!("no terminal event channel yet; deferring terminal spawn");
            return None;
        };

        // A shell path pairs with the config's shell arguments; an
        // explicit command line — like a profile's stored one —
        // carries its own trailing arguments and never inherits them:
        // `--cmd htop` must not run `htop --login`. A command tab is
        // also session-ephemeral (never saved, never replayed); a
        // profile tab is the explicit opt-in for persistence, so it
        // never is.
        let ephemeral = matches!(command, SpawnCommand::CommandLine(_));
        let mut profile_id: Option<String> = None;
        // The preset's extra environment, applied exactly where
        // `profile_id` is: only a spawn that actually runs the
        // preset's command line carries it — the degraded branch
        // leaves this empty, so a degraded tab is a plain-shell tab
        // in full (no id, no env — never half a profile's identity).
        let mut pty_env: HashMap<String, String> = HashMap::new();
        let (shell, args) = match command {
            SpawnCommand::Shell(shell) => (shell.clone(), self.config.args.clone()),
            SpawnCommand::CommandLine(line) => {
                match tabs::command_line_parts(line) {
                    Some((program, trailing)) => (program, trailing),
                    // Unreachable in practice — the CLI rejects an
                    // empty --cmd — but a degraded empty line still
                    // spawns the default shell rather than a dying
                    // exec of "".
                    None => (self.config.resolved_shell(), self.config.args.clone()),
                }
            }
            SpawnCommand::Profile {
                name, command, env, ..
            } => {
                if let Some((program, trailing)) = tabs::command_line_parts(command) {
                    // The id is recorded only when the spawn actually
                    // runs the preset: the degraded line below spawns
                    // a plain-shell tab, which must not save an id
                    // restore would keep re-degrading.
                    profile_id = Some(name.clone());
                    pty_env = preset_env(env.as_ref());
                    (program, trailing)
                } else {
                    // A hand-edited config can store a blank command
                    // line; warn and spawn the plain shell —
                    // degradation, never an error surface.
                    log::warn!(
                        "profile {name} stores no runnable command line; \
                         spawning the shell instead"
                    );
                    (self.config.resolved_shell(), self.config.args.clone())
                }
            }
        };

        let term_config = TermConfig {
            scrolling_history: self.config.scrollback_lines as usize,
            ..TermConfig::default()
        };
        let pty_options = PtyOptions {
            shell: Some(PtyShell::new(shell.clone(), args)),
            // Alacritty chdirs before exec and silently keeps this
            // process's cwd on failure, so only a re-validated
            // directory ever reaches here (dead cwd → `$HOME` → unset
            // — see `plan_restore`; a new tab passes either an
            // inherited- or explicitly-requested live cwd). The env
            // map rides the same builder: each entry is applied with
            // `Command::env` at exec, merging over the inherited
            // environment (empty = inherit untouched).
            working_directory: cwd.map(Path::to_owned),
            drain_on_exit: true,
            env: pty_env,
        };

        let tab_id = TabId(self.next_tab_id);
        self.next_tab_id += 1;

        // The spawn's own color resolution — the same [`colors_for`]
        // chain the config-apply pass runs per tab: the Profile arm's
        // recorded id, the global pick for every other arm (including
        // the degraded blank-line branch above, whose
        // `profile_id = None` rule this shares).
        let colors = colors_for(&self.config, &self.active_colors(), profile_id.as_deref());

        match Terminal::new(
            tab_id,
            tx,
            term_config,
            &pty_options,
            &self.terminal_options(),
            &colors,
        ) {
            Ok(terminal) => Some(SpawnedTerminal {
                tab_id,
                terminal,
                shell,
                profile_id,
                ephemeral,
            }),
            Err(err) => {
                log::error!("failed to spawn shell: {err}");
                None
            }
        }
    }

    /// Guarantees at least one tab exists — restoring the saved
    /// session on the first productive run, spawning a fresh default
    /// tab every other time. Every path that needs a tab (the
    /// terminal channel arriving, showing the surface, the last tab's
    /// respawn) lands here, so the session is restored exactly once
    /// and nothing is spawned-then-deleted; runs before the channel
    /// exists defer, leaving the stash armed.
    pub(super) fn ensure_tabs(&mut self) {
        if !self.tabs.is_empty() {
            return;
        }
        if let Some(plan) = self.restore.take_for_run(self.term_event_tx.is_some()) {
            let mut spawned = 0usize;
            for spec in &plan.tabs {
                if let Some(mut tab) = self.spawn_tab(Some(spec)) {
                    // Saved chrome reapplied onto the fresh tab: plain
                    // append keeps the saved order, and the override
                    // keeps the fresh shell's first OSC title from
                    // clobbering the saved one.
                    spec.title.clone_into(&mut tab.title);
                    spec.title_override.clone_into(&mut tab.title_override);
                    self.tabs.push(tab);
                    spawned += 1;
                }
            }
            if let session::RestoreOutcome::Restored { active } =
                session::restore_outcome(plan.active, spawned)
            {
                self.active_tab = active;
                // The restored active tab's scheme owns the window
                // background from the first paint (the every-assignment
                // rule; spawn resolved each tab's own cells already).
                self.refresh_active_window_bg();
                return;
            }
            // Every spawn failed: fall through to the
            // single-default-tab path below.
        }
        if let Some(tab) = self.spawn_tab(None) {
            // Placement is moot for the never-empty respawn's first tab:
            // the list is empty by construction, so both positions
            // coincide — `true` just names the shipped default.
            self.active_tab = tabs::insert_new_tab(&mut self.tabs, self.active_tab, true, tab);
            self.refresh_active_window_bg();
            // The spawn title through the live mode (the
            // `open_new_tab` precedent): the cwd modes read the
            // preset cwd a default profile seeded, if any.
            self.retitle_tab(self.active_tab);
        }
    }
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
            color_scheme: None,
            env: None,
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
                env: None,
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
                env: None,
            })
        );
        assert_eq!(
            profile_pick(&config, 0),
            Some(SpawnCommand::Profile {
                name: "ssh".to_string(),
                command: "ssh web".to_string(),
                cwd: Some(PathBuf::from("/srv")),
                env: None,
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

    // --- the preset-env application policy ---

    fn env_map() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("DROPTERM_PROFILE".to_string(), "ssh-prod".to_string()),
            ("TERM_PROGRAM".to_string(), "dropterm-ssh".to_string()),
        ])
    }

    #[test]
    fn preset_env_is_empty_when_the_preset_carries_no_map() {
        // `None` — every pre-initiative profile, every profile without
        // extra env — is a no-op: the pty inherits this process's
        // environment untouched, exactly the pre-module spawn.
        assert!(preset_env(None).is_empty());
    }

    #[test]
    fn preset_env_clones_a_set_map_into_the_pty_shape() {
        // A set map lands in `PtyOptions::env` verbatim, key for key
        // and value for value; alacritty's pty builder merges it over
        // the inherited environment at exec.
        assert_eq!(
            preset_env(Some(&env_map())),
            HashMap::from([
                ("DROPTERM_PROFILE".to_string(), "ssh-prod".to_string()),
                ("TERM_PROGRAM".to_string(), "dropterm-ssh".to_string()),
            ])
        );
    }

    #[test]
    fn default_spawn_command_carries_the_preset_env() {
        // The resolved default hands spawn the preset's env map with
        // the rest of the preset — spawn applies a profile, it never
        // looks one up. The runnable-vs-degraded split stays
        // `spawn_terminal`'s (below), exactly as for `profile_id`.
        let config = test_config(
            &[profile::Profile {
                env: Some(env_map()),
                ..named_profile("ssh", "ssh web", None)
            }],
            Some("ssh"),
        );
        assert_eq!(
            default_spawn_command(&config),
            SpawnCommand::Profile {
                name: "ssh".to_string(),
                command: "ssh web".to_string(),
                cwd: None,
                env: Some(env_map()),
            }
        );
    }

    #[test]
    fn profile_pick_carries_the_preset_env() {
        // The menu pick resolves the whole preset, env included; the
        // call site composes the live-cwd filter on top as always.
        let config = test_config(
            &[
                profile::Profile {
                    env: Some(env_map()),
                    ..named_profile("ssh", "ssh web", None)
                },
                named_profile("top", "htop", None),
            ],
            None,
        );
        assert_eq!(
            profile_pick(&config, 0),
            Some(SpawnCommand::Profile {
                name: "ssh".to_string(),
                command: "ssh web".to_string(),
                cwd: None,
                env: Some(env_map()),
            })
        );
        // A preset without a map rides along as None — never an empty
        // map the pty arm would treat differently.
        assert_eq!(
            profile_pick(&config, 1),
            Some(SpawnCommand::Profile {
                name: "top".to_string(),
                command: "htop".to_string(),
                cwd: None,
                env: None,
            })
        );
    }
}
