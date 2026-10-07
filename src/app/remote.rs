// SPDX-License-Identifier: GPL-3.0-only

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

#[cfg(test)]
mod tests {
    use super::*;
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
}
