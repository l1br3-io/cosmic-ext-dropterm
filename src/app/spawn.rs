// SPDX-License-Identifier: GPL-3.0-only

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::tty::{Options as PtyOptions, Shell as PtyShell};

use super::theme_glue::{effective_colors, effective_is_dark};
use super::{DropTerm, SurfaceState, resolved_profile};
use crate::config::{DropTermConfig, NewTabCwd, program_is_runnable};
use crate::profile;
use crate::session;
use crate::tabs::{self, Tab, TabId};
use crate::terminal::{Terminal, TerminalOptions};

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

/// The live-directory filter a configured cwd passes at spawn: dead →
/// warn and `None` — the `--cwd` degradation, never an error surface —
/// so the next leg of the composition applies instead. `what` names
/// the owning key in the warning (`"profile ssh"`, `default_cwd`).
pub(super) fn live_dir(what: &str, cwd: Option<&PathBuf>) -> Option<PathBuf> {
    cwd.cloned().filter(|dir| {
        let live = dir.is_dir();
        if !live {
            log::warn!("ignoring {what} cwd {}: not a directory", dir.display());
        }
        live
    })
}

/// The preset-cwd case of [`live_dir`] — the profile spawn sites'
/// spelling of the shared warn-and-degrade rule.
pub(super) fn live_preset_cwd(name: &str, cwd: Option<&PathBuf>) -> Option<PathBuf> {
    live_dir(&format!("profile {name}"), cwd)
}

/// The plain-arm cwd composition the plain spawn sites share — the
/// new-tab keybind, the bare remote `new-tab`, and the
/// startup/respawn fresh tab: [`tabs::resolve_plain_spawn_cwd`] over
/// the live-checked `default_cwd`. The check runs only when the leg
/// is actually consulted — `Inherit` never reads the key (a dead
/// default cannot warn from a mode that ignores it), and an explicit
/// request wins in both modes before the default leg matters (an
/// overridden spawn never warns about a key it did not use). Profile
/// spawns never route here: a set, existing `default_profile` still
/// owns the cwd outright (ruling 4).
pub(super) fn plain_spawn_cwd(
    config: &DropTermConfig,
    explicit: Option<PathBuf>,
    inherited: Option<PathBuf>,
) -> Option<PathBuf> {
    match (config.new_tab_cwd, explicit.as_ref()) {
        (NewTabCwd::Inherit, _) | (NewTabCwd::Default, Some(_)) => {
            tabs::resolve_plain_spawn_cwd(config.new_tab_cwd, explicit, inherited, None)
        }
        (NewTabCwd::Default, None) => tabs::resolve_plain_spawn_cwd(
            NewTabCwd::Default,
            None,
            inherited,
            live_dir("default_cwd", config.default_cwd.as_ref()),
        ),
    }
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

/// What a spawn actually runs once its command resolves: the program
/// the pty execs, the trailing arguments handed to it, and the
/// profile identity the spawn carries — id and env only ever
/// together, and only when the preset's command line actually runs.
#[derive(Debug, PartialEq, Eq)]
struct SpawnParts {
    program: String,
    trailing: Vec<String>,
    profile_id: Option<String>,
    env: HashMap<String, String>,
}

/// Resolves a [`SpawnCommand`] against the config's shell settings —
/// the pure resolution half of [`DropTerm::spawn_terminal`], extracted
/// so every rule is testable headless. A shell path pairs with the
/// config's shell arguments; an explicit `--cmd` line carries its own
/// trailing arguments and never inherits them: `--cmd htop` must not
/// run `htop --login`. A profile spawn runs its command as a preamble
/// of the resolved shell with a guaranteed interactive tail (the
/// command-then-shell ruling, 2026-10-12): `sh -c "<line>; exec sh"`,
/// so a command that finishes — `ls`, an unrunnable `a` — leaves the
/// tab alive with its output above the prompt and the profile's whole
/// identity intact (id, env, appearance); an unrunnable program is
/// reported by the shell in-band, never degraded. Only a blank stored
/// line — a hand-edited config — degrades to the plain shell (no id,
/// no env: never half a profile's identity). And a program that
/// cannot exec today — a typo, an uninstall, a hand-edited path, a
/// session-recorded shell the system no longer carries — degrades to
/// the config's plain shell exactly like a blank command line: warn
/// once, spawn a shell tab, never an error surface, never a spawn
/// that fails into a silent no-op.
fn spawn_parts(command: &SpawnCommand, config: &DropTermConfig) -> SpawnParts {
    let plain = || SpawnParts {
        program: config.resolved_shell(),
        trailing: config.args.clone(),
        profile_id: None,
        env: HashMap::new(),
    };
    match command {
        // The recorded-shell leg: `resolved_shell`'s own chain
        // filters configured and env values, so this guard covers a
        // shell recorded on a tab — session-restored — that the
        // system no longer carries.
        SpawnCommand::Shell(shell) if program_is_runnable(shell) => SpawnParts {
            program: shell.clone(),
            ..plain()
        },
        SpawnCommand::Shell(shell) => {
            log::warn!("shell {shell:?} cannot run; spawning the configured shell instead");
            plain()
        }
        SpawnCommand::CommandLine(line) => match tabs::command_line_parts(line) {
            Some((program, trailing)) if program_is_runnable(&program) => SpawnParts {
                program,
                trailing,
                ..plain()
            },
            // A `--cmd` whose program cannot exec: degrade to the
            // plain shell — something runnable always appears.
            Some((program, ..)) => {
                log::warn!(
                    "command program {program:?} cannot run; \
                     spawning the shell instead"
                );
                plain()
            }
            // Unreachable in practice — the CLI rejects an empty
            // --cmd — but a degraded empty line still spawns the
            // default shell rather than a dying exec of "".
            None => plain(),
        },
        SpawnCommand::Profile {
            name, command, env, ..
        } => {
            // A blank stored line has no preamble to run — a hand-edited
            // config — so it keeps the old degradation: the plain shell,
            // and no id restore would keep re-wrapping a line that isn't
            // there (never half a profile's identity).
            if command.trim().is_empty() {
                log::warn!(
                    "profile {name} stores no runnable command line; \
                     spawning the shell instead"
                );
                return plain();
            }
            // The command-then-shell ruling: the stored line becomes a
            // preamble of the resolved shell, closed by `exec` of the
            // same shell — the terminal the command leaves behind is an
            // interactive prompt in the profile's own colors and env,
            // with the command's output still on screen. The line rides
            // as one `-c` argument verbatim (no re-quoting), and an
            // unrunnable program surfaces as the shell's own "command
            // not found" above that prompt — visible, honest, identity
            // kept. The wrap also makes one-shot commands long-lived,
            // so their tabs never flash-die (the exit path keeps
            // today's close-on-exit for everything).
            let shell = config.resolved_shell();
            SpawnParts {
                trailing: vec!["-c".to_owned(), format!("{command}; exec {shell}")],
                program: shell,
                profile_id: Some(name.clone()),
                env: preset_env(env.as_ref()),
            }
        }
    }
}

/// The spawn-time HOME (path-suggest spec): a profile env override
/// when the preset carries one, else this process's — the environment
/// the pty exec inherits, resolved app-side so `~` reduces against
/// the same HOME the shell sees. `None` (no HOME anywhere) leaves
/// `~`-rooted tokens unresolvable: silence.
fn spawn_home(profile_env: &HashMap<String, String>) -> Option<String> {
    profile_env
        .get("HOME")
        .cloned()
        .or_else(|| std::env::var("HOME").ok())
}

/// The spawn-time PATH (command-catalog spec): a profile env
/// override when the preset carries one, else this process's — the
/// environment the pty exec inherits, the one the command catalog
/// snapshots at spawn (resolved app-side so the catalog describes
/// the same machine the shell sees). `None` (no PATH anywhere) is
/// the empty catalog.
fn spawn_path(profile_env: &HashMap<String, String>) -> Option<String> {
    profile_env
        .get("PATH")
        .cloned()
        .or_else(|| std::env::var("PATH").ok())
}

/// The program word the tab is *spawned as* (path-suggest spec): the
/// shell for a plain spawn and every degradation that lands on one,
/// the leading program of a `--cmd` line, and a profile command's
/// leading program — spawn truth resolved app-side, never a
/// process-name heuristic. The one deliberate divergence from
/// [`SpawnParts::program`]: a profile's pty wraps its command in a
/// shell preamble, but the tab is still *spawned as* that command
/// (`ssh web` is an ssh tab), so the path provider's allowlist gates
/// on the command's program, never the wrapper.
fn spawned_as_program(command: &SpawnCommand, config: &DropTermConfig) -> String {
    let plain = || config.resolved_shell();
    match command {
        SpawnCommand::Shell(shell) if program_is_runnable(shell) => shell.clone(),
        SpawnCommand::Shell(_) => plain(),
        SpawnCommand::CommandLine(line) => match tabs::command_line_parts(line) {
            Some((program, ..)) if program_is_runnable(&program) => program,
            _ => plain(),
        },
        // Every non-blank stored line keeps its profile identity —
        // runnable or not — so its leading program is the spawn truth
        // regardless of runnability; only the blank line (a
        // hand-edited config) degrades to the plain shell.
        SpawnCommand::Profile { command, .. } => match tabs::command_line_parts(command) {
            Some((program, ..)) => program,
            None => plain(),
        },
    }
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
            // no-user-context plain spawn) and the mode-threaded cwd
            // composition: `Default` opens the fresh tab in the
            // live-checked `default_cwd`, `Inherit` keeps the pty's
            // default directory exactly as before. A default preset's
            // dead cwd warns and degrades the same way.
            let command = default_spawn_command(&self.config);
            let cwd = match &command {
                SpawnCommand::Profile {
                    name, cwd: preset, ..
                } => live_preset_cwd(name, preset.as_ref()),
                _ => plain_spawn_cwd(&self.config, None, None),
            };
            return self.spawn_shell(&command, cwd);
        };
        let mut tab = if let Some(profile) = &spec.profile {
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
        }?;
        // The saved pin replays onto the restored tab: `Tab::new`
        // seeds unpinned, and only a restore carries a pin — a new
        // tab never spawns pinned.
        tab.pinned = spec.pinned;
        Some(tab)
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
        // The spawn-grid settle's arming (status-glance criterion 1):
        // with a live surface already laid out, the fresh terminal's
        // first resize is the settling one — arm the close so it
        // records the resting grid and every later resize reads as
        // the user's deviation. With no live surface (startup, or a
        // hidden one), the settle waits for the opening's granted-
        // size adoption to arm it instead.
        if matches!(
            self.surface,
            SurfaceState::Shown(_) | SurfaceState::Hiding(_)
        ) {
            tab.terminal.lock().unwrap().arm_spawn_grid_next_resize();
        }
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

        // A command tab is session-ephemeral (never saved, never
        // replayed); a profile tab is the explicit opt-in for
        // persistence, so it never is.
        let ephemeral = matches!(command, SpawnCommand::CommandLine(_));
        let SpawnParts {
            program: shell,
            trailing: args,
            profile_id,
            env: pty_env,
        } = spawn_parts(command, &self.config);

        // The spawn's own appearance resolution — the same
        // [`resolved_profile`]/seam chain the config-apply pass runs
        // per tab: the spawning profile's effective values (global
        // for every non-Profile arm and the degraded blank-line
        // branch, whose `profile_id = None` rule this shares).
        // Resolved into primitives here — the profile reference
        // borrows `self.config`, and the id allocation below needs
        // `&mut self`.
        let is_dark = effective_is_dark(self.config.theme_preference, cosmic::theme::is_dark());
        let (font_size, scrollback, colors) = {
            let profile = resolved_profile(&self.config, profile_id.as_deref());
            (
                profile::effective_font_size(profile, self.config.font_size),
                profile::effective_scrollback(profile, self.config.scrollback_lines),
                effective_colors(&self.config, profile, is_dark),
            )
        };
        let term_config = TermConfig {
            scrolling_history: scrollback as usize,
            ..TermConfig::default()
        };
        // The path provider's `~` root, resolved from the same env map
        // the pty is about to own.
        let spawn_home = spawn_home(&pty_env);
        // The command catalog's PATH, from the same env map.
        let spawn_path = spawn_path(&pty_env);
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

        match Terminal::new(
            tab_id,
            tx,
            term_config,
            &pty_options,
            &TerminalOptions {
                font_size,
                autosuggestions_enabled: self.config.autosuggestions_enabled,
                autosuggestions_paths_enabled: self.config.autosuggestions_paths_enabled,
                spawned_program: spawned_as_program(command, &self.config),
                spawn_home,
                spawn_path,
                ..TerminalOptions::default()
            },
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
                // rule; spawn resolved each tab's own cells already),
                // and its fresh shell seeds the status band's snapshot.
                self.refresh_active_window_bg();
                self.refresh_status_snapshot();
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
            self.refresh_status_snapshot();
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
            font_size: None,
            scrollback_lines: None,
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
        // call site composes the live cwd filter on top as always.
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

    // --- the unrunnable-program degradation policy ---

    /// A program no test installs — deterministically not runnable,
    /// as a bare name (PATH-scan miss) or as a path.
    fn dead_program() -> &'static str {
        "dropterm-not-a-real-program"
    }

    /// The plain-shell shape every degradation lands on: the test
    /// config's live `/bin/sh`, its args, and no profile identity.
    fn plain_parts() -> SpawnParts {
        SpawnParts {
            program: "/bin/sh".to_string(),
            trailing: Vec::new(),
            profile_id: None,
            env: HashMap::new(),
        }
    }

    #[test]
    fn spawn_parts_runs_a_profile_command_as_a_shell_preamble_with_an_interactive_tail() {
        // The command-then-shell ruling (2026-10-12): the stored line
        // rides as one `-c` argument of the resolved shell, closed by
        // `exec` of the same shell — the command's output stays above
        // an interactive prompt and the profile keeps its whole
        // identity (id + env together).
        let config = test_config(&[], None);
        assert_eq!(
            spawn_parts(
                &SpawnCommand::Profile {
                    name: "top".to_string(),
                    command: "/bin/sh -c top".to_string(),
                    cwd: None,
                    env: Some(env_map()),
                },
                &config,
            ),
            SpawnParts {
                program: "/bin/sh".to_string(),
                trailing: vec!["-c".to_string(), "/bin/sh -c top; exec /bin/sh".to_string(),],
                profile_id: Some("top".to_string()),
                env: HashMap::from([
                    ("DROPTERM_PROFILE".to_string(), "ssh-prod".to_string()),
                    ("TERM_PROGRAM".to_string(), "dropterm-ssh".to_string()),
                ]),
            }
        );
    }

    #[test]
    fn spawn_parts_wraps_an_unrunnable_profile_program_with_its_identity_kept() {
        // A program that cannot exec — a typo, an uninstall — is no
        // longer a degrade-to-plain-shell case: the shell reports it
        // in-band ("command not found") above the prompt, and the tab
        // keeps the id and env it launched under.
        let config = test_config(&[], None);
        assert_eq!(
            spawn_parts(
                &SpawnCommand::Profile {
                    name: "ttttt".to_string(),
                    command: dead_program().to_string(),
                    cwd: Some(PathBuf::from("/srv")),
                    env: Some(env_map()),
                },
                &config,
            ),
            SpawnParts {
                program: "/bin/sh".to_string(),
                trailing: vec![
                    "-c".to_string(),
                    format!("{}; exec /bin/sh", dead_program()),
                ],
                profile_id: Some("ttttt".to_string()),
                env: HashMap::from([
                    ("DROPTERM_PROFILE".to_string(), "ssh-prod".to_string()),
                    ("TERM_PROGRAM".to_string(), "dropterm-ssh".to_string()),
                ]),
            }
        );
    }

    #[test]
    fn spawn_parts_degrades_a_blank_profile_command_line() {
        // The hand-edited blank line has no preamble to wrap — the
        // one degradation left, landing on the plain shell with no
        // half identity.
        let config = test_config(&[], None);
        assert_eq!(
            spawn_parts(
                &SpawnCommand::Profile {
                    name: "ssh".to_string(),
                    command: "   ".to_string(),
                    cwd: None,
                    env: Some(env_map()),
                },
                &config,
            ),
            plain_parts()
        );
    }

    #[test]
    fn spawn_parts_runs_a_cmd_line_with_its_trailing_arguments() {
        // The runnable `--cmd` line: program and args from the line,
        // never the config's shell arguments.
        let config = test_config(&[], None);
        assert_eq!(
            spawn_parts(
                &SpawnCommand::CommandLine("/bin/sh -c htop".to_string()),
                &config,
            ),
            SpawnParts {
                program: "/bin/sh".to_string(),
                trailing: vec!["-c".to_string(), "htop".to_string()],
                profile_id: None,
                env: HashMap::new(),
            }
        );
    }

    #[test]
    fn spawn_parts_degrades_a_cmd_program_that_cannot_run() {
        // A `--cmd` whose program is gone degrades to the plain shell
        // rather than a silent no-op: something runnable always
        // appears.
        let config = test_config(&[], None);
        assert_eq!(
            spawn_parts(
                &SpawnCommand::CommandLine(dead_program().to_string()),
                &config,
            ),
            plain_parts()
        );
    }

    #[test]
    fn spawn_parts_pairs_the_config_args_with_the_shell() {
        // A shell path rides the config's shell arguments (`--login`
        // and kin); a live recorded shell keeps them verbatim.
        let mut config = test_config(&[], None);
        config.args = vec!["--login".to_string()];
        assert_eq!(
            spawn_parts(&SpawnCommand::Shell("/bin/sh".to_string()), &config),
            SpawnParts {
                trailing: vec!["--login".to_string()],
                ..plain_parts()
            }
        );
    }

    #[test]
    fn spawn_parts_degrades_a_recorded_shell_that_cannot_run() {
        // A session-recorded shell can outlive its system: the
        // respawn degrades through the config's live-checked chain —
        // the tab opens on a working shell, never drops silently.
        let config = test_config(&[], None);
        assert_eq!(
            spawn_parts(
                &SpawnCommand::Shell("/dropterm-not-a-real-shell".to_string()),
                &config,
            ),
            plain_parts()
        );
    }

    // --- the path provider's spawn truth (path-suggest) ---

    #[test]
    fn spawn_home_prefers_the_profile_env_override_over_the_process_home() {
        // The preset's HOME wins; without one the pty inherits this
        // process's HOME — the same merge the exec applies.
        assert_eq!(
            spawn_home(&HashMap::from([(
                "HOME".to_string(),
                "/srv/remote-home".to_string()
            )])),
            Some("/srv/remote-home".to_string())
        );
        assert_eq!(spawn_home(&HashMap::new()), std::env::var("HOME").ok());
    }

    #[test]
    fn spawn_path_prefers_the_profile_env_override_over_the_process_path() {
        // The preset's PATH wins; without one the pty inherits this
        // process's PATH — the environment the command catalog
        // snapshots at spawn (command-catalog spec).
        assert_eq!(
            spawn_path(&HashMap::from([(
                "PATH".to_string(),
                "/srv/remote-bin".to_string()
            )])),
            Some("/srv/remote-bin".to_string())
        );
        assert_eq!(spawn_path(&HashMap::new()), std::env::var("PATH").ok());
    }

    #[test]
    fn spawned_as_program_resolves_shell_and_degraded_spawns_to_the_shell() {
        // Plain shell, the unrunnable recorded shell's degradation,
        // and the unrunnable --cmd program's degradation all land on
        // a shell — exactly what the pty execs, so the spawn truth
        // matches.
        let config = test_config(&[], None);
        assert_eq!(
            spawned_as_program(&SpawnCommand::Shell("/bin/bash".to_string()), &config),
            "/bin/bash"
        );
        assert_eq!(
            spawned_as_program(
                &SpawnCommand::Shell("/dropterm-not-a-real-shell".to_string()),
                &config
            ),
            "/bin/sh"
        );
        assert_eq!(
            spawned_as_program(
                &SpawnCommand::CommandLine(dead_program().to_string()),
                &config
            ),
            "/bin/sh"
        );
    }

    #[test]
    fn spawned_as_program_resolves_a_cmd_line_to_its_leading_program() {
        // A bare PATH name resolves like the pty leg's own check —
        // `sh` is on every test PATH, `htop` is not (its degradation
        // is pinned above).
        let config = test_config(&[], None);
        assert_eq!(
            spawned_as_program(
                &SpawnCommand::CommandLine("sh -c htop".to_string()),
                &config
            ),
            "sh"
        );
    }

    #[test]
    fn spawned_as_program_resolves_a_profile_to_its_commands_leading_program() {
        // The wrapping shell never counts: an ssh profile is
        // spawned-as ssh, a python-REPL profile spawned-as python3 —
        // the path provider's allowlist shuts both out by
        // construction. An unrunnable program keeps that identity
        // (`spawn_parts` keeps it too); only the blank stored line
        // degrades to the shell.
        let config = test_config(&[], None);
        let profile = |command: &str| SpawnCommand::Profile {
            name: "p".to_string(),
            command: command.to_string(),
            cwd: None,
            env: None,
        };
        assert_eq!(spawned_as_program(&profile("ssh web"), &config), "ssh");
        assert_eq!(spawned_as_program(&profile("python3"), &config), "python3");
        assert_eq!(
            spawned_as_program(&profile(dead_program()), &config),
            dead_program()
        );
        assert_eq!(spawned_as_program(&profile("   "), &config), "/bin/sh");
    }

    // --- the plain-arm cwd composition (default-spawn DS-1) ---

    /// A path no test creates — deterministically not a directory.
    fn dead_dir() -> PathBuf {
        PathBuf::from("/dropterm-default-spawn-not-a-real-directory")
    }

    #[test]
    fn plain_spawn_cwd_inherit_mode_never_consults_the_key() {
        // Ruling 2: `Inherit` is byte-identical to today — the chain
        // is explicit → inherited → pty default, and a set (even dead)
        // `default_cwd` can neither move the spawn nor warn about
        // itself from a mode that ignores it.
        let mut config = test_config(&[], None);
        config.new_tab_cwd = NewTabCwd::Inherit;
        config.default_cwd = Some(dead_dir());
        assert_eq!(
            plain_spawn_cwd(&config, None, Some(PathBuf::from("/home/jpt/src"))),
            Some(PathBuf::from("/home/jpt/src"))
        );
        assert_eq!(plain_spawn_cwd(&config, None, None), None);
    }

    #[test]
    fn plain_spawn_cwd_default_mode_opens_in_the_live_directory() {
        let dir = std::env::temp_dir().join(format!(
            "dropterm-default-spawn-live-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("make a live test directory");
        let mut config = test_config(&[], None);
        config.new_tab_cwd = NewTabCwd::Default;
        config.default_cwd = Some(dir.clone());
        // The default leg replaces inheritance — ruling 2 — and lands
        // set-and-live exactly like a preset cwd.
        assert_eq!(
            plain_spawn_cwd(&config, None, Some(PathBuf::from("/home/jpt/src"))),
            Some(dir.clone())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plain_spawn_cwd_default_mode_degrades_a_dead_directory_to_the_pty_default() {
        // The `live_preset_cwd` rule generalized to the key's own
        // name: warn once, then the pty's default directory — the
        // inherited cwd is replaced, never kept as the fallback.
        let mut config = test_config(&[], None);
        config.new_tab_cwd = NewTabCwd::Default;
        config.default_cwd = Some(dead_dir());
        assert_eq!(
            plain_spawn_cwd(&config, None, Some(PathBuf::from("/home/jpt/src"))),
            None
        );
    }

    #[test]
    fn plain_spawn_cwd_keeps_an_explicit_request_winning_in_default_mode() {
        // An explicit `--cwd` wins in both modes — and never warns
        // about the default key it overrides.
        let mut config = test_config(&[], None);
        config.new_tab_cwd = NewTabCwd::Default;
        config.default_cwd = Some(dead_dir());
        assert_eq!(
            plain_spawn_cwd(&config, Some(PathBuf::from("/tmp")), None),
            Some(PathBuf::from("/tmp"))
        );
    }
}
