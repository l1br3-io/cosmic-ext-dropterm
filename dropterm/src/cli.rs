// SPDX-License-Identifier: GPL-3.0-only

//! The CLI surface (py-5): the surface verbs and the tab verbs one
//! clap enum owns — the words both halves speak. The second launch
//! serializes its action as the Display name plus [`forward_words`]'s
//! payload; the running instance reconstructs the exact action
//! through [`parse_forwarded_action`] (the libcosmic encoder's wire,
//! kept verbatim so either receiver decodes either sender).

use clap::Parser;

/// The frontend's argument surface: `--is-visible` is a query;
/// pairing it with an action subcommand is a contradiction, rejected
/// at parse (the libcosmic frontend's rule verbatim).
#[derive(Debug, Clone, Parser)]
#[command(
    name = "dropterm",
    version,
    about,
    args_conflicts_with_subcommands = true
)]
pub(crate) struct Args {
    /// Print whether the running terminal's surface is visible
    /// (true/false) and exit with a matching code (0 visible, 1
    /// hidden or not running). Never summons the terminal.
    #[arg(long)]
    pub(crate) is_visible: bool,

    #[command(subcommand)]
    pub(crate) action: Option<Action>,
}

/// Every verb the CLI speaks — the surface verbs and the tab verbs
/// (the libcosmic `ToggleAction`'s definitions verbatim, payloads
/// and help included).
#[derive(Debug, Clone, PartialEq, Eq, Default, clap::Subcommand)]
pub(crate) enum Action {
    #[default]
    Toggle,
    Show,
    Hide,
    /// Open a new tab — remotely (a second launch) or at first
    /// startup — optionally in an explicit working directory and/or
    /// running an explicit command line. Implies showing the overlay.
    NewTab {
        /// Working directory for the new tab; defaults to the active
        /// tab's cwd.
        #[arg(long)]
        cwd: Option<std::path::PathBuf>,
        /// Command line to run instead of the shell —
        /// whitespace-split, no quoting support.
        #[arg(long, value_parser = clap::builder::NonEmptyStringValueParser::new())]
        cmd: Option<String>,
        /// Name of the profile to launch: resolved against the
        /// running instance's config; a name that no longer exists
        /// warns and spawns the plain shell. An explicit `--cwd`
        /// composes with it and wins over the profile's cwd.
        #[arg(
            long,
            value_parser = clap::builder::NonEmptyStringValueParser::new(),
            conflicts_with = "cmd"
        )]
        profile: Option<String>,
    },
    /// Close a tab remotely — the active tab, or the 1-based
    /// `--tab-index` target. Out-of-range targets no-op with a
    /// warning; the overlay is not shown.
    CloseTab {
        /// 1-based index of the tab to close; defaults to the active tab.
        #[arg(long, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        tab_index: Option<usize>,
    },
    /// Activate a tab remotely — the strict 1-based `N`, no wrap and
    /// no `9 = last` keybind idiom. Out-of-range indices no-op with a
    /// warning; a resolved activation shows the overlay, exactly like
    /// a `new-tab`.
    SelectTab {
        /// 1-based index of the tab to activate.
        #[arg(value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        index: usize,
    },
    /// Rename a tab remotely — the active tab, or the 1-based
    /// `--tab-index` target. A non-empty TITLE becomes the sticky
    /// override shell OSC titles never clobber; the empty TITLE is
    /// the documented reset, clearing the override so shell titles
    /// resume. Quiet: the overlay is not shown.
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
        /// Name of the profile to switch to: exact, case-sensitive,
        /// first-match against the running instance's config.
        #[arg(long, value_parser = clap::builder::NonEmptyStringValueParser::new())]
        profile: String,
        /// 1-based index of the tab to switch; defaults to the active tab.
        #[arg(long, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        tab_index: Option<usize>,
    },
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Toggle => "toggle",
            Self::Show => "show",
            Self::Hide => "hide",
            Self::NewTab { .. } => "new-tab",
            Self::CloseTab { .. } => "close-tab",
            Self::SelectTab { .. } => "select-tab",
            Self::RenameTab { .. } => "rename-tab",
            Self::DuplicateTab { .. } => "duplicate-tab",
            Self::SwitchProfileTab { .. } => "switch-profile-tab",
        };
        f.write_str(name)
    }
}

/// Encodes the arguments a second launch forwards alongside the
/// action's Display name — the libcosmic `forward_words` encoder
/// verbatim: everything the receiving instance needs to reconstruct
/// the exact action through [`parse_forwarded_action`]. Actions
/// without payloads forward nothing.
pub(crate) fn forward_words(action: &Action) -> Vec<String> {
    let mut forwarded = Vec::new();
    match action {
        Action::NewTab { cwd, cmd, profile } => {
            // A non-UTF-8 path cannot cross the wire: the forwarded
            // action then simply carries no `--cwd`, and the
            // receiving instance opens the tab in the inherited
            // directory.
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
        Action::CloseTab {
            tab_index: Some(index),
        }
        | Action::DuplicateTab {
            tab_index: Some(index),
        } => {
            forwarded.push("--tab-index".to_string());
            forwarded.push(index.to_string());
        }
        Action::SelectTab { index } => {
            forwarded.push(index.to_string());
        }
        Action::RenameTab { title, tab_index } => {
            forwarded.push(title.clone());
            if let Some(index) = tab_index {
                forwarded.push("--tab-index".to_string());
                forwarded.push(index.to_string());
            }
        }
        Action::SwitchProfileTab { profile, tab_index } => {
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

/// What a first launch does with the startup action — the libcosmic
/// `startup_action` degradation verbatim: the tab verbs have no
/// running state to act on (no tabs exist yet), so they degrade to
/// the plain default `Toggle`, never a fabricated close, rename,
/// duplicate, or profile switch on a restored-or-fresh session; only
/// `new-tab` carries startup application (the boot opens that tab
/// alongside any restored session tabs), and the surface verbs apply
/// as-is.
pub(crate) fn startup_action(action: &Action) -> Action {
    match action {
        Action::CloseTab { .. }
        | Action::SelectTab { .. }
        | Action::RenameTab { .. }
        | Action::DuplicateTab { .. }
        | Action::SwitchProfileTab { .. } => Action::Toggle,
        startup => startup.clone(),
    }
}

/// Parses a forwarded action — the action name plus any arguments
/// the second launch sent along — back into an [`Action`], reusing
/// the clap definitions above as the single source of truth for the
/// surface. `None` when the words do not form a valid action.
pub(crate) fn parse_forwarded_action(words: &[String]) -> Option<Action> {
    let argv: Vec<String> = std::iter::once("dropterm".to_string())
        .chain(words.iter().cloned())
        .collect();
    Args::try_parse_from(argv).ok()?.action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_surface_verbs_parse_and_default_to_toggle() {
        let bare = Args::parse_from(["dropterm"]);
        assert_eq!(bare.action, None);
        assert_eq!(bare.action.unwrap_or_default(), Action::Toggle);
        assert_eq!(
            Args::parse_from(["dropterm", "show"]).action,
            Some(Action::Show)
        );
        assert_eq!(
            Args::parse_from(["dropterm", "hide"]).action,
            Some(Action::Hide)
        );
    }

    #[test]
    fn is_visible_parses_alone_and_conflicts_with_subcommands() {
        let alone = Args::try_parse_from(["dropterm", "--is-visible"]).expect("parses alone");
        assert!(alone.is_visible);
        assert_eq!(alone.action, None);
        assert!(Args::try_parse_from(["dropterm", "--is-visible", "show"]).is_err());
        assert!(Args::try_parse_from(["dropterm", "show", "--is-visible"]).is_err());
    }

    #[test]
    fn first_launch_tab_verbs_start_plain() {
        // With no running instance to act on, the tab verbs degrade
        // to the default Toggle — never a fabricated close, rename,
        // duplicate, or profile switch on a restored-or-fresh
        // session.
        for action in [
            Action::CloseTab { tab_index: None },
            Action::SelectTab { index: 2 },
            Action::RenameTab {
                title: "work".to_string(),
                tab_index: None,
            },
            Action::DuplicateTab { tab_index: None },
            Action::SwitchProfileTab {
                profile: "htop".to_string(),
                tab_index: None,
            },
        ] {
            assert_eq!(startup_action(&action), Action::Toggle);
        }
    }

    #[test]
    fn first_launch_keeps_new_tab_and_surface_verbs() {
        // Only the tab verbs degrade: new-tab carries startup
        // meaning, and the surface verbs apply as-is.
        let new_tab = Action::NewTab {
            cwd: None,
            cmd: None,
            profile: None,
        };
        assert_eq!(startup_action(&new_tab), new_tab);
        assert_eq!(startup_action(&Action::Show), Action::Show);
    }

    #[test]
    fn a_malformed_forward_degrades_to_none() {
        // Words clap rejects yield None, which the dbus decode turns
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
        // plus forward_words' payload; the running instance
        // reconstructs it through parse_forwarded_action. Every
        // action must survive exactly, serialized by the real encoder
        // — no hand-rolled words.
        let actions = [
            Action::Toggle,
            Action::Show,
            Action::Hide,
            Action::NewTab {
                cwd: None,
                cmd: None,
                profile: None,
            },
            Action::NewTab {
                cwd: Some("/tmp".into()),
                cmd: None,
                profile: None,
            },
            Action::NewTab {
                cwd: None,
                cmd: Some("htop".to_string()),
                profile: None,
            },
            Action::NewTab {
                cwd: Some("/tmp".into()),
                cmd: Some("htop -u root".to_string()),
                profile: None,
            },
            Action::NewTab {
                cwd: None,
                cmd: None,
                profile: Some("ssh-web".to_string()),
            },
            Action::NewTab {
                cwd: Some("/tmp".into()),
                cmd: None,
                profile: Some("ssh-web".to_string()),
            },
            Action::CloseTab { tab_index: None },
            Action::CloseTab { tab_index: Some(2) },
            Action::SelectTab { index: 1 },
            Action::SelectTab { index: 3 },
            Action::RenameTab {
                title: "work".to_string(),
                tab_index: None,
            },
            Action::RenameTab {
                title: String::new(),
                tab_index: None,
            },
            Action::RenameTab {
                title: "logs".to_string(),
                tab_index: Some(3),
            },
            Action::DuplicateTab { tab_index: None },
            Action::DuplicateTab { tab_index: Some(2) },
            Action::SwitchProfileTab {
                profile: "htop".to_string(),
                tab_index: None,
            },
            Action::SwitchProfileTab {
                profile: "ssh-web".to_string(),
                tab_index: Some(2),
            },
        ];
        for action in actions {
            let words = std::iter::once(action.to_string())
                .chain(forward_words(&action))
                .collect::<Vec<String>>();
            assert_eq!(parse_forwarded_action(&words), Some(action));
        }
    }
}
