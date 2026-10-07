// SPDX-License-Identifier: GPL-3.0-only

use clap::Parser;
use cosmic::app::Task;

use super::DropTerm;
use super::Message;
use super::spawn::{SpawnCommand, default_spawn_command, live_preset_cwd};
use crate::Args;
use crate::profile;
use crate::tabs;

/// What the running instance should do on startup, mirrored by a second
/// launch's D-Bus activation (see `main.rs`).
// `pub`, not the spec's `pub(crate)`: rustc's E0446 forbids a
// crate-private type in the `type SubCommand = ToggleAction` binding
// the CosmicFlags impl requires. Reach is identical — this enum sits
// in a `pub(crate)` module, so it is crate-visible exactly as it was
// as a `pub` item of the crate-private `app` module.
#[derive(Debug, Clone, PartialEq, Eq, Default, clap::Subcommand)]
pub enum ToggleAction {
    #[default]
    Toggle,
    Show,
    Hide,
    /// Open a new tab — remotely (a second launch) or at first startup —
    /// optionally in an explicit working directory and/or running an
    /// explicit command line. Implies showing the overlay.
    NewTab {
        /// Working directory for the new tab; defaults to the active
        /// tab's cwd.
        #[arg(long)]
        cwd: Option<std::path::PathBuf>,
        /// Command line to run instead of the shell — whitespace-split,
        /// no quoting support.
        #[arg(long, value_parser = clap::builder::NonEmptyStringValueParser::new())]
        cmd: Option<String>,
        /// Name of the profile to launch (see the `profile` module):
        /// resolved against the running instance's config; a name that
        /// no longer exists warns and spawns the plain shell. An
        /// explicit `--cwd` composes with it and wins over the
        /// profile's cwd.
        #[arg(
            long,
            value_parser = clap::builder::NonEmptyStringValueParser::new(),
            conflicts_with = "cmd"
        )]
        profile: Option<String>,
    },
    /// Close a tab remotely — the active tab, or the 1-based `--tab-index`
    /// target. Out-of-range targets no-op with a warning; the overlay
    /// is not shown.
    CloseTab {
        /// 1-based index of the tab to close; defaults to the active tab.
        #[arg(long, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        tab_index: Option<usize>,
    },
    /// Activate a tab remotely — the strict 1-based `N`, no wrap and no
    /// `9 = last` keybind idiom. Out-of-range indices no-op with a
    /// warning; a resolved activation shows the overlay, exactly like
    /// a `new-tab`.
    SelectTab {
        /// 1-based index of the tab to activate.
        #[arg(value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        index: usize,
    },
    /// Rename a tab remotely — the active tab, or the 1-based
    /// `--tab-index` target. A non-empty TITLE becomes the sticky
    /// override shell OSC titles never clobber; the empty TITLE is the
    /// documented reset, clearing the override so shell titles resume.
    /// Quiet: the overlay is not shown.
    RenameTab {
        /// New title; the empty string clears the override.
        title: String,
        /// 1-based index of the tab to rename; defaults to the active tab.
        #[arg(long, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        tab_index: Option<usize>,
    },
    /// Duplicate a tab remotely — the active tab, or the 1-based
    /// `--tab-index` target: a fresh shell opens in that tab's cwd —
    /// the running command is never copied — lands after the active
    /// tab, and becomes active. Implies showing the overlay; an
    /// unresolvable target degrades to the bare new-tab inheritance
    /// with a warning.
    DuplicateTab {
        /// 1-based index of the tab to duplicate; defaults to the active tab.
        #[arg(long, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        tab_index: Option<usize>,
    },
    /// Switch a live tab's profile remotely — the active tab, or the
    /// 1-based `--tab-index` target: the tab's terminal is replaced
    /// in place by a fresh one running `--profile`'s resolved
    /// command/cwd (the running command and scrollback are lost, the
    /// same close+reopen cost), keeping the tab's slot, activation
    /// state, and sticky user rename; the tab is a persistent profile
    /// tab from then on. Implies showing the overlay; an unknown
    /// profile name or unresolvable target warns and edits nothing.
    SwitchProfileTab {
        /// Name of the profile to switch to (see the `profile`
        /// module): exact, case-sensitive, first-match against the
        /// running instance's config.
        #[arg(long, value_parser = clap::builder::NonEmptyStringValueParser::new())]
        profile: String,
        /// 1-based index of the tab to switch; defaults to the active tab.
        #[arg(long, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        tab_index: Option<usize>,
    },
}

impl std::fmt::Display for ToggleAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Toggle => write!(f, "toggle"),
            Self::Show => write!(f, "show"),
            Self::Hide => write!(f, "hide"),
            Self::NewTab { .. } => write!(f, "new-tab"),
            Self::CloseTab { .. } => write!(f, "close-tab"),
            Self::SelectTab { .. } => write!(f, "select-tab"),
            Self::RenameTab { .. } => write!(f, "rename-tab"),
            Self::DuplicateTab { .. } => write!(f, "duplicate-tab"),
            Self::SwitchProfileTab { .. } => write!(f, "switch-profile-tab"),
        }
    }
}

/// What a first launch does with the startup action: pure so the
/// degradation is headless-testable (constructing `DropTerm` needs a
/// live `Core`). The tab verbs have no running state to act on — no
/// tabs exist yet — so they degrade to the plain default `Toggle`,
/// never a fabricated close, rename, duplicate, or profile switch on
/// a restored-or-fresh session; only `new-tab` carries startup
/// application, and the surface verbs apply as-is. The call site
/// info-logs the degradation.
pub(super) fn startup_action(action: &ToggleAction) -> ToggleAction {
    match action {
        ToggleAction::CloseTab { .. }
        | ToggleAction::SelectTab { .. }
        | ToggleAction::RenameTab { .. }
        | ToggleAction::DuplicateTab { .. }
        | ToggleAction::SwitchProfileTab { .. } => ToggleAction::Toggle,
        startup => startup.clone(),
    }
}

#[derive(Debug, Clone, Default)]
// `pub` for the same E0446 reason as `ToggleAction`: the
// `type Flags = Flags` binding on the pub `Application` impl. Reach
// is crate-only — the `pub(crate)` module caps it.
pub struct Flags {
    pub action: ToggleAction,
    /// The words the action forwards alongside its Display name,
    /// pre-encoded by [`Flags::new`]: `CosmicFlags::args` returns
    /// `&str`s borrowed from `self`, which cannot hand out a formatted
    /// `--tab-index 3` — so the encoding runs once per launch instead
    /// of leaking per call, with [`forward_words`] as the single
    /// encoder the D-Bus forward path and the round-trip test both
    /// ride.
    forwarded: Vec<String>,
}

/// Encodes the arguments a second launch forwards alongside the action's
/// Display name: everything the receiving instance needs to
/// reconstruct the exact action through `parse_forwarded_action`.
/// Actions without payloads forward nothing.
pub(super) fn forward_words(action: &ToggleAction) -> Vec<String> {
    let mut forwarded = Vec::new();
    match action {
        ToggleAction::NewTab { cwd, cmd, profile } => {
            // A non-UTF-8 path cannot cross this borrowed-str
            // boundary: the forwarded action then simply carries no
            // --cwd, and the receiving instance opens the tab in the
            // inherited directory.
            if let Some(dir) = cwd.as_deref().and_then(std::path::Path::to_str) {
                forwarded.push("--cwd".to_string());
                forwarded.push(dir.to_string());
            }
            if let Some(cmd) = cmd {
                forwarded.push("--cmd".to_string());
                forwarded.push(cmd.clone());
            }
            if let Some(name) = profile {
                forwarded.push("--profile".to_string());
                forwarded.push(name.clone());
            }
        }
        ToggleAction::CloseTab {
            tab_index: Some(index),
        }
        | ToggleAction::DuplicateTab {
            tab_index: Some(index),
        } => {
            forwarded.push("--tab-index".to_string());
            forwarded.push(index.to_string());
        }
        ToggleAction::SelectTab { index } => {
            forwarded.push(index.to_string());
        }
        ToggleAction::RenameTab { title, tab_index } => {
            forwarded.push(title.clone());
            if let Some(index) = tab_index {
                forwarded.push("--tab-index".to_string());
                forwarded.push(index.to_string());
            }
        }
        ToggleAction::SwitchProfileTab { profile, tab_index } => {
            forwarded.push("--profile".to_string());
            forwarded.push(profile.clone());
            if let Some(index) = tab_index {
                forwarded.push("--tab-index".to_string());
                forwarded.push(index.to_string());
            }
        }
        _ => {}
    }
    forwarded
}

impl Flags {
    /// Wraps `action` for launch, pre-encoding the words it forwards
    /// to the running instance (see [`Flags::forwarded`]).
    pub(crate) fn new(action: ToggleAction) -> Self {
        let forwarded = forward_words(&action);
        Self { action, forwarded }
    }
}

impl cosmic::app::CosmicFlags for Flags {
    type SubCommand = ToggleAction;
    type Args = Vec<String>;

    fn action(&self) -> Option<&ToggleAction> {
        Some(&self.action)
    }

    /// The arguments forwarded alongside the action name: a
    /// `new-tab` request carries its `--cwd`/`--cmd`, a `close-tab`
    /// or `rename-tab` its 1-based `--tab-index`, a `select-tab` its
    /// 1-based index, a `rename-tab` its TITLE word — everything else
    /// forwards no args.
    fn args(&self) -> Vec<&str> {
        self.forwarded.iter().map(String::as_str).collect()
    }
}

impl DropTerm {
    /// Opens the tab a remote `new-tab` asked for, placing it per
    /// `after_current` (the `new_tab_after_current` config). At first
    /// launch the terminal event channel may not exist yet — the
    /// spawn defers, the request stashes, and the channel's arrival
    /// applies it right after the session restore (re-reading the
    /// config then, so a flipped toggle still applies to the stash).
    pub(super) fn open_remote_tab(
        &mut self,
        action: ToggleAction,
        after_current: bool,
    ) -> Task<Message> {
        let ToggleAction::NewTab { .. } = action else {
            return Task::none();
        };
        if self.term_event_tx.is_none() {
            self.pending_remote_tab = Some(action);
            return Task::none();
        }
        self.spawn_remote_tab(action, after_current)
    }

    /// Spawns the requested remote tab: an explicit live `--cwd` wins,
    /// then the active tab's cwd, then the pty default; a `--cmd` runs
    /// as the tab's program instead of the shell; a `--profile`
    /// resolves against the config and runs under its id. The tab
    /// lands at the position `after_current` picks — after the active
    /// one, or at the end — becomes active, and — being a layout
    /// change — marks the session dirty.
    pub(super) fn spawn_remote_tab(
        &mut self,
        action: ToggleAction,
        after_current: bool,
    ) -> Task<Message> {
        let ToggleAction::NewTab { cwd, cmd, profile } = action else {
            return Task::none();
        };
        self.ensure_tabs();
        // Flag presence, not live-ness: an explicit --cwd guards the
        // plain-shell branch below even when the directory turns out
        // dead (the user asked for an override; a dead one warns and
        // degrades within that override, it does not fall back to the
        // default profile).
        let explicit_cwd = cwd.is_some();
        // A nonexistent --cwd degrades to the inherited/default cwd
        // with a warning, never an error surface.
        let explicit = cwd.filter(|dir| {
            let live = dir.is_dir();
            if !live {
                log::warn!("ignoring --cwd {}: not a directory", dir.display());
            }
            live
        });
        let inherited = self
            .active()
            .and_then(|tab| tab.terminal.lock().unwrap().working_directory());
        // `--cmd` wins its arm outright (the clap conflict keeps a
        // --cmd/--profile pairing off the wire); a profile resolves
        // against the live config before spawn — a name that no
        // longer exists warns and spawns the plain shell (ruling 3:
        // the fire-and-forget forward has no reply surface). An
        // explicit --cwd keeps the spawn a plain shell even with a
        // default profile set: the user overrode the preset
        // wholesale, and duplicate-tab rides exactly here (a fresh
        // shell in the target's cwd, never the default — ruling 1).
        // The bare remainder is the one branch the configured default
        // owns.
        let command = match (cmd, profile) {
            (Some(line), _) => SpawnCommand::CommandLine(line),
            (None, Some(name)) => {
                if let Some(profile) = profile::find(&self.config.profiles, &name) {
                    SpawnCommand::Profile {
                        name: profile.name.clone(),
                        command: profile.command.clone(),
                        cwd: profile.cwd.clone(),
                    }
                } else {
                    log::warn!("no profile named {name}; spawning the shell instead");
                    SpawnCommand::Shell(self.config.resolved_shell())
                }
            }
            (None, None) if explicit_cwd => SpawnCommand::Shell(self.config.resolved_shell()),
            (None, None) => default_spawn_command(&self.config),
        };
        // A profile tab's directory — by name or by default: the
        // explicit --cwd over the preset's live cwd over inheritance,
        // the preset degrading the way --cwd does (warn when dead,
        // then the inherited/default directory applies). Every other
        // arm resolves exactly as before.
        let spawn_cwd = match &command {
            SpawnCommand::Profile {
                name, cwd: preset, ..
            } => explicit
                .clone()
                .or_else(|| live_preset_cwd(name, preset.as_ref()))
                .or(inherited.clone()),
            _ => tabs::resolve_new_tab_cwd(explicit, inherited),
        };
        if let Some(tab) = self.spawn_shell(&command, spawn_cwd) {
            let at = tabs::insert_new_tab(&mut self.tabs, self.active_tab, after_current, tab);
            // The spawn title through the live mode, exactly like the
            // keybind funnel's shared tail (see `open_new_tab`).
            self.retitle_tab(at);
            let switch = self.set_active_tab(at);
            return Task::batch([switch, self.mark_session_dirty()]);
        }
        Task::none()
    }

    /// Applies a remote `close-tab`: the 1-based `--tab-index`
    /// resolves through [`tabs::remote_target_index`] — absent means
    /// the active tab — and the close rides the shared
    /// [`Self::close_tab_at`], inheriting successor activation,
    /// respawn-when-last, attention retirement, the session-dirty
    /// mark, and the rename-session prune. An unresolvable target —
    /// no tabs, out of range — warns and no-ops: the fire-and-forget
    /// D-Bus call has no reply to carry the failure, so degradation
    /// is quiet.
    pub(super) fn close_remote_tab(&mut self, tab_index: Option<usize>) -> Task<Message> {
        if let Some(index) = tabs::remote_target_index(self.tabs.len(), tab_index, self.active_tab)
        {
            self.close_tab_at(index)
        } else {
            log::warn!(
                "close-tab: no tab at --tab-index {tab_index:?} ({} open)",
                self.tabs.len()
            );
            Task::none()
        }
    }

    /// Applies a remote `select-tab`: the strict 1-based `N` resolves
    /// through [`tabs::select_tab_target`] — the wire's 1-based
    /// indices converted once, at this boundary — and the activation
    /// rides the shared [`Self::set_active_tab`], inheriting attention
    /// retirement, the session-dirty mark, and the already-active
    /// no-op. An unresolvable target — no tabs, out of range — warns
    /// and no-ops: the fire-and-forget D-Bus call has no reply to
    /// carry the failure, so degradation is quiet. The `None` return
    /// tells the caller to skip the surface half too.
    pub(super) fn select_remote_tab(&mut self, one_based: usize) -> Option<Task<Message>> {
        if let Some(index) = tabs::select_tab_target(self.tabs.len(), one_based) {
            Some(self.set_active_tab(index))
        } else {
            log::warn!(
                "select-tab: no tab at index {one_based} ({} open)",
                self.tabs.len()
            );
            None
        }
    }

    /// Applies a remote `rename-tab`: the target resolves like
    /// close-tab's — absent means the active tab — through
    /// [`tabs::remote_target_index`]. Any inline rename session on
    /// that tab ends first with its draft discarded — a pending draft
    /// must not commit over the remote title — then
    /// [`tabs::commit_rename`] applies the payload: a non-empty TITLE
    /// sets the sticky override shell OSC titles never clobber, the
    /// empty TITLE clears it so shell titles resume. Quiet like
    /// close-tab, and the session-dirty mark persists titles. An
    /// unresolvable target warns and no-ops.
    pub(super) fn rename_remote_tab(
        &mut self,
        title: &str,
        tab_index: Option<usize>,
    ) -> Task<Message> {
        if let Some(index) = tabs::remote_target_index(self.tabs.len(), tab_index, self.active_tab)
        {
            let refocus = self.end_rename_on(index);
            self.commit_rename_on(index, title);
            Task::batch([refocus, self.mark_session_dirty()])
        } else {
            log::warn!(
                "rename-tab: no tab at --tab-index {tab_index:?} ({} open)",
                self.tabs.len()
            );
            Task::none()
        }
    }

    /// Applies a remote `duplicate-tab`: the target — absent means the
    /// active tab — resolves through [`tabs::remote_target_index`],
    /// and a fresh shell tab opens in that tab's cwd; the running
    /// command is never copied, and the source's rename pin carries
    /// (ruling 2 — the keybind, menu, and CLI cannot fork). An
    /// explicit but unresolvable `--tab-index` warns, and the source
    /// degrades to the active tab — the same tab the warning names —
    /// so its cwd and pin are read, not the pty default's. The spawn
    /// rides [`Self::open_duplicate_tab`] end to end: the
    /// channel-bootstrap deferral, after-active landing, activation,
    /// and the session-dirty mark are all inherited. The overlay half
    /// is the caller's.
    pub(super) fn duplicate_remote_tab(&mut self, tab_index: Option<usize>) -> Task<Message> {
        let target = tabs::remote_target_index(self.tabs.len(), tab_index, self.active_tab);
        if target.is_none() && tab_index.is_some() {
            log::warn!(
                "duplicate-tab: no tab at --tab-index {tab_index:?} ({} open); duplicating the active tab instead",
                self.tabs.len()
            );
        }
        // The degraded path keeps the warning's promise — "duplicating
        // the active tab instead" — by reading cwd and pin from the
        // active tab, the same source the bare verb resolves to.
        // Capture both before the helper's `&mut self` call.
        let source = target
            .or(Some(self.active_tab))
            .filter(|&index| index < self.tabs.len());
        let (cwd, title_override) =
            source
                .and_then(|index| self.tabs.get(index))
                .map_or((None, None), |tab| {
                    (
                        tab.terminal.lock().unwrap().working_directory(),
                        tab.title_override.clone(),
                    )
                });
        self.open_duplicate_tab(cwd, title_override)
    }
}

/// Parses a forwarded action — the action name plus any arguments the
/// second launch sent along — back into a [`ToggleAction`], reusing the
/// clap definitions above as the single source of truth for the CLI
/// surface. `None` when the words do not form a valid action.
pub(crate) fn parse_forwarded_action(words: &[String]) -> Option<ToggleAction> {
    let argv: Vec<String> = std::iter::once("cosmic-ext-dropterm".to_string())
        .chain(words.iter().cloned())
        .collect();
    Args::try_parse_from(argv).ok()?.action
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::app::CosmicFlags;
    use std::path::PathBuf;

    // --- the first-launch degradation ---

    #[test]
    fn first_launch_tab_verbs_start_plain() {
        // With no running instance to act on, the tab verbs degrade
        // to the default Toggle — never a fabricated close, rename,
        // duplicate, or profile switch on a restored-or-fresh
        // session.
        for action in [
            ToggleAction::CloseTab { tab_index: None },
            ToggleAction::SelectTab { index: 2 },
            ToggleAction::RenameTab {
                title: "work".to_string(),
                tab_index: None,
            },
            ToggleAction::DuplicateTab { tab_index: None },
            ToggleAction::SwitchProfileTab {
                profile: "htop".to_string(),
                tab_index: None,
            },
        ] {
            assert_eq!(startup_action(&action), ToggleAction::Toggle);
        }
    }

    #[test]
    fn first_launch_keeps_new_tab_and_surface_verbs() {
        // Only the tab verbs degrade: new-tab carries startup
        // meaning, and the surface verbs apply as-is.
        let new_tab = ToggleAction::NewTab {
            cwd: None,
            cmd: None,
            profile: None,
        };
        assert_eq!(startup_action(&new_tab), new_tab);
        assert_eq!(startup_action(&ToggleAction::Show), ToggleAction::Show);
    }

    // --- the profile flag's forward encoding ---

    #[test]
    fn forward_words_encodes_the_profile_flag() {
        // The forwarded words must carry `--profile NAME`, or the
        // receiving instance would re-parse a plain new-tab.
        let action = ToggleAction::NewTab {
            cwd: None,
            cmd: None,
            profile: Some("ssh-web".to_string()),
        };
        assert_eq!(
            forward_words(&action),
            vec!["--profile".to_string(), "ssh-web".to_string()]
        );
    }

    #[test]
    fn forward_words_composes_cwd_and_profile() {
        // --cwd + --profile compose on the wire (OQ-2): both flags
        // survive the forward, the explicit --cwd winning at spawn.
        let action = ToggleAction::NewTab {
            cwd: Some(PathBuf::from("/tmp")),
            cmd: None,
            profile: Some("ssh-web".to_string()),
        };
        assert_eq!(
            forward_words(&action),
            vec![
                "--cwd".to_string(),
                "/tmp".to_string(),
                "--profile".to_string(),
                "ssh-web".to_string(),
            ]
        );
    }

    #[test]
    fn a_malformed_forward_degrades_to_none() {
        // Words clap rejects yield None, which dbus_activation turns
        // into a plain toggle — never an error surface (here, a bad
        // --tab-index).
        let words: Vec<String> = ["close-tab", "--tab-index", "0"]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(parse_forwarded_action(&words), None);
    }

    #[test]
    fn forwarded_actions_survive_the_dbus_round_trip() {
        // The second launch serializes the action as its Display name
        // plus flags.args(); the primary reconstructs it through
        // parse_forwarded_action. Every action must survive exactly,
        // serialized by the real encoder — no hand-rolled words.
        let actions = [
            ToggleAction::Toggle,
            ToggleAction::Show,
            ToggleAction::Hide,
            ToggleAction::NewTab {
                cwd: None,
                cmd: None,
                profile: None,
            },
            ToggleAction::NewTab {
                cwd: Some("/tmp".into()),
                cmd: None,
                profile: None,
            },
            ToggleAction::NewTab {
                cwd: None,
                cmd: Some("htop".to_string()),
                profile: None,
            },
            ToggleAction::NewTab {
                cwd: Some("/tmp".into()),
                cmd: Some("htop -u root".to_string()),
                profile: None,
            },
            ToggleAction::NewTab {
                cwd: None,
                cmd: None,
                profile: Some("ssh-web".to_string()),
            },
            ToggleAction::NewTab {
                cwd: Some("/tmp".into()),
                cmd: None,
                profile: Some("ssh-web".to_string()),
            },
            ToggleAction::CloseTab { tab_index: None },
            ToggleAction::CloseTab { tab_index: Some(2) },
            ToggleAction::SelectTab { index: 1 },
            ToggleAction::SelectTab { index: 3 },
            ToggleAction::RenameTab {
                title: "work".to_string(),
                tab_index: None,
            },
            ToggleAction::RenameTab {
                title: String::new(),
                tab_index: None,
            },
            ToggleAction::RenameTab {
                title: "logs".to_string(),
                tab_index: Some(3),
            },
            ToggleAction::DuplicateTab { tab_index: None },
            ToggleAction::DuplicateTab { tab_index: Some(2) },
            ToggleAction::SwitchProfileTab {
                profile: "htop".to_string(),
                tab_index: None,
            },
            ToggleAction::SwitchProfileTab {
                profile: "ssh-web".to_string(),
                tab_index: Some(2),
            },
        ];
        for action in actions {
            let flags = Flags::new(action.clone());
            let words = std::iter::once(action.to_string())
                .chain(flags.args().into_iter().map(str::to_string))
                .collect::<Vec<String>>();
            assert_eq!(parse_forwarded_action(&words), Some(action));
        }
    }
}
