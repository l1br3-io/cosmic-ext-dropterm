// SPDX-License-Identifier: GPL-3.0-only

use std::path::{Path, PathBuf};

use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::tty::{Options as PtyOptions, Shell as PtyShell};

use super::theme_glue::{effective_colors, effective_is_dark};
use super::{DropTerm, SurfaceState, resolved_profile};
use crate::profile;
use crate::session;
use crate::spawn::{SpawnParts, spawn_home, spawn_parts, spawn_path, spawned_as_program};
use crate::tabs::{self, Tab, TabId};
use crate::terminal::suggest;
use crate::terminal::{CosmicHost, Terminal, TerminalOptions};

// The pure resolution half — `SpawnCommand`, the command-then-shell
// wrap, the degradations, the cwd compositions — lives in core
// (gpui-parity's py-5 moved it there so both frontends spawn through
// one implementation, tests and all); re-exported here so the sibling
// modules' `super::spawn` imports keep resolving unchanged.
pub(super) use crate::spawn::{
    SpawnCommand, default_spawn_command, live_preset_cwd, plain_spawn_cwd, profile_pick,
};

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
            self.surface.state,
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
            Box::new(CosmicHost::new()),
            tab_id,
            tx,
            term_config,
            &pty_options,
            &TerminalOptions {
                font_size,
                autosuggestions_enabled: self.config.autosuggestions_enabled,
                images_enabled: self.config.images_enabled,
                autosuggestions_paths_enabled: self.config.autosuggestions_paths_enabled,
                autosuggestions_commands_enabled: self.config.autosuggestions_commands_enabled,
                autosuggestions_menu_enabled: self.config.autosuggestions_menu_enabled,
                autosuggestions_menu_auto: self.config.autosuggestions_menu_auto,
                autosuggestions_aliases: self.config.autosuggestions_aliases.clone(),
                // The spec table is the app's shared load
                // (async-completions): a spawn before the background
                // load lands carries the empty placeholder and the
                // loading flag — the menu answers "loading" until the
                // load's `set_config` pass swaps the real table in.
                autosuggestions_specs_table: self
                    .specs_table
                    .clone()
                    .unwrap_or_else(suggest::empty_specs),
                autosuggestions_specs_loading: self.specs_table.is_none(),
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
