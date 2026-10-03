// SPDX-License-Identifier: GPL-3.0-only

mod app;
mod config;
mod context_menu;
mod geometry;
mod grab_bar;
mod keybinds;
mod notify;
mod session;
mod settings;
mod slide;
mod surface;
mod tab_bar;
mod tabs;
mod terminal;

// The served-state contract lives in the lib target (src/lib.rs) so
// the panel applet crate consumes the same implementation.
use cosmic_ext_dropterm::dbus_state;

use clap::Parser;

use app::{DropTerm, Flags, ToggleAction};

/// A native, Yakuake/Guake-style drop-down terminal for the COSMIC desktop.
///
/// A second launch toggles the running instance instead of opening a new
/// one; see the project README for how to bind this to a key in COSMIC
/// Settings.
#[derive(Parser, Debug, Clone)]
// `--is-visible` is a query; pairing it with an action subcommand is
// a contradiction, rejected at parse. (`conflicts_with` on the field
// can't express this: a subcommand is not an arg id.)
#[command(
    name = "cosmic-ext-dropterm",
    version,
    about,
    args_conflicts_with_subcommands = true
)]
struct Args {
    /// Print whether the running terminal's surface is visible
    /// (true/false) and exit with a matching code (0 visible, 1
    /// hidden or not running). Never summons the terminal.
    #[arg(long)]
    is_visible: bool,

    #[command(subcommand)]
    action: Option<ToggleAction>,
}

/// Parses a forwarded action — the action name plus any arguments the
/// second launch sent along — back into a [`ToggleAction`], reusing the
/// clap definitions above as the single source of truth for the CLI
/// surface (remote-newtab spec: the D-Bus round-trip). `None` when the
/// words do not form a valid action.
pub(crate) fn parse_forwarded_action(words: &[String]) -> Option<ToggleAction> {
    let argv: Vec<String> = std::iter::once("cosmic-ext-dropterm".to_string())
        .chain(words.iter().cloned())
        .collect();
    Args::try_parse_from(argv).ok()?.action
}

/// The bus half of `--is-visible`: `Some(visible)` when a running
/// instance answered, `None` when the name is unowned (no note owed)
/// or owned without the V1 interface (stale binary mid-upgrade — a
/// stderr note, per the spec's decision table). Query-only: nothing
/// here activates or spawns the app, so the flag can never summon the
/// terminal.
fn query_visibility() -> Option<bool> {
    // zbus needs a runtime with io and time; a current-thread runtime
    // is enough for one synchronous-looking query. Its feature set
    // arrives through zbus's own tokio dependency, not our `sync` +
    // `time` declaration.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    runtime.block_on(async {
        let conn = zbus::connection::Builder::session()
            .ok()?
            .build()
            .await
            .ok()?;
        let owned = zbus::fdo::DBusProxy::new(&conn)
            .await
            .ok()?
            .name_has_owner(dbus_state::BUS_NAME.try_into().ok()?)
            .await
            .ok()?;
        if !owned {
            return None;
        }
        let read = async {
            dbus_state::V1InterfaceProxy::new(&conn)
                .await?
                .visible()
                .await
        };
        match read.await {
            Ok(visible) => Some(visible),
            Err(_) => {
                eprintln!(
                    "cosmic-ext-dropterm: bus name owned but the V1 state \
                     interface is missing (stale binary?); treating as not \
                     visible"
                );
                None
            }
        }
    })
}

fn main() -> cosmic::iced::Result {
    env_logger::init();

    // Matches cosmic-term's own startup: alacritty_terminal doesn't set
    // these itself, and a correct TERM is needed for full-color, mouse and
    // bracketed-paste support in most shells.
    alacritty_terminal::tty::setup_env();
    // SAFETY: called once, before any thread that reads the environment is
    // spawned (the PTY event loop threads are spawned per-terminal, later).
    unsafe {
        std::env::set_var("TERM", "xterm-256color");
    }

    let args = Args::parse();
    // The query path exits before `run_single_instance` so a second
    // launch asking a question never forwards an action to the
    // running instance.
    if args.is_visible {
        let (visible, code) = dbus_state::is_visible_decision(query_visibility());
        println!("{visible}");
        std::process::exit(i32::from(code));
    }
    let flags = Flags::new(args.action.unwrap_or_default());

    let settings = cosmic::app::Settings::default()
        .no_main_window(true)
        .exit_on_close(false);

    cosmic::app::run_single_instance::<DropTerm>(settings, flags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::app::CosmicFlags;

    #[test]
    fn new_tab_parses_with_either_or_both_flags() {
        let plain = Args::parse_from(["dropterm", "new-tab"]).action;
        assert_eq!(
            plain,
            Some(ToggleAction::NewTab {
                cwd: None,
                cmd: None
            })
        );

        let with_cwd = Args::parse_from(["dropterm", "new-tab", "--cwd", "/tmp"]).action;
        assert_eq!(
            with_cwd,
            Some(ToggleAction::NewTab {
                cwd: Some("/tmp".into()),
                cmd: None
            })
        );

        let with_cmd = Args::parse_from(["dropterm", "new-tab", "--cmd", "htop -u root"]).action;
        assert_eq!(
            with_cmd,
            Some(ToggleAction::NewTab {
                cwd: None,
                cmd: Some("htop -u root".to_string())
            })
        );

        let with_both =
            Args::parse_from(["dropterm", "new-tab", "--cwd", "/tmp", "--cmd", "htop"]).action;
        assert_eq!(
            with_both,
            Some(ToggleAction::NewTab {
                cwd: Some("/tmp".into()),
                cmd: Some("htop".to_string())
            })
        );
    }

    #[test]
    fn empty_cmd_is_rejected_at_parse() {
        // A --cmd with no program would spawn a dying tab; reject it at
        // the CLI edge instead.
        let result = Args::try_parse_from(["dropterm", "new-tab", "--cmd", ""]);
        assert!(result.is_err());
    }

    #[test]
    fn is_visible_flag_parses_alone() {
        let args = Args::try_parse_from(["dropterm", "--is-visible"]).expect("parses alone");
        assert!(args.is_visible);
        assert_eq!(args.action, None);
    }

    #[test]
    fn is_visible_flag_conflicts_with_subcommands() {
        assert!(Args::try_parse_from(["dropterm", "--is-visible", "show"]).is_err());
        assert!(Args::try_parse_from(["dropterm", "show", "--is-visible"]).is_err());
        assert!(Args::try_parse_from(["dropterm", "--is-visible", "new-tab"]).is_err());
    }

    #[test]
    fn close_tab_parses_bare_and_indexed() {
        let bare = Args::parse_from(["dropterm", "close-tab"]).action;
        assert_eq!(bare, Some(ToggleAction::CloseTab { tab_index: None }));

        let indexed = Args::parse_from(["dropterm", "close-tab", "--tab-index", "2"]).action;
        assert_eq!(indexed, Some(ToggleAction::CloseTab { tab_index: Some(2) }));
    }

    #[test]
    fn close_tab_rejects_a_zero_tab_index() {
        // Tab indices are 1-based (remote-tab-actions spec); zero dies
        // at the CLI edge, exactly like new-tab's empty --cmd.
        let result = Args::try_parse_from(["dropterm", "close-tab", "--tab-index", "0"]);
        assert!(result.is_err());
    }

    #[test]
    fn select_tab_parses_a_one_based_index() {
        let selected = Args::parse_from(["dropterm", "select-tab", "2"]).action;
        assert_eq!(selected, Some(ToggleAction::SelectTab { index: 2 }));
    }

    #[test]
    fn select_tab_rejects_a_zero_index() {
        // Indices are strict 1-based (remote-tab-actions spec); zero
        // dies at the CLI edge like close-tab's --tab-index 0.
        let result = Args::try_parse_from(["dropterm", "select-tab", "0"]);
        assert!(result.is_err());
    }

    #[test]
    fn rename_tab_parses_title_bare_and_indexed() {
        let bare = Args::parse_from(["dropterm", "rename-tab", "work"]).action;
        assert_eq!(
            bare,
            Some(ToggleAction::RenameTab {
                title: "work".to_string(),
                tab_index: None
            })
        );

        let indexed =
            Args::parse_from(["dropterm", "rename-tab", "work", "--tab-index", "2"]).action;
        assert_eq!(
            indexed,
            Some(ToggleAction::RenameTab {
                title: "work".to_string(),
                tab_index: Some(2)
            })
        );
    }

    #[test]
    fn rename_tab_empty_title_parses_as_the_documented_reset() {
        // `rename-tab ""` clears the override (commit_rename
        // semantics); the empty string must survive the parse —
        // deliberately no non-empty parser on TITLE.
        let reset = Args::parse_from(["dropterm", "rename-tab", ""]).action;
        assert_eq!(
            reset,
            Some(ToggleAction::RenameTab {
                title: String::new(),
                tab_index: None
            })
        );
    }

    #[test]
    fn rename_tab_rejects_a_zero_tab_index() {
        let result = Args::try_parse_from(["dropterm", "rename-tab", "work", "--tab-index", "0"]);
        assert!(result.is_err());
    }

    #[test]
    fn duplicate_tab_parses_bare_and_indexed() {
        let bare = Args::parse_from(["dropterm", "duplicate-tab"]).action;
        assert_eq!(bare, Some(ToggleAction::DuplicateTab { tab_index: None }));

        let indexed = Args::parse_from(["dropterm", "duplicate-tab", "--tab-index", "2"]).action;
        assert_eq!(
            indexed,
            Some(ToggleAction::DuplicateTab { tab_index: Some(2) })
        );
    }

    #[test]
    fn duplicate_tab_rejects_a_zero_tab_index() {
        let result = Args::try_parse_from(["dropterm", "duplicate-tab", "--tab-index", "0"]);
        assert!(result.is_err());
    }

    #[test]
    fn a_malformed_forward_degrades_to_none() {
        // Words clap rejects yield None, which dbus_activation turns
        // into a plain toggle — never an error surface (the
        // remote-newtab rule, now carrying a bad --tab-index).
        let words: Vec<String> = ["close-tab", "--tab-index", "0"]
            .iter()
            .map(|word| word.to_string())
            .collect();
        assert_eq!(parse_forwarded_action(&words), None);
    }

    #[test]
    fn forwarded_actions_survive_the_dbus_round_trip() {
        // The second launch serializes the action as its Display name
        // plus flags.args(); the primary reconstructs it through
        // parse_forwarded_action. Every action must survive exactly,
        // serialized by the real encoder — no hand-rolled words
        // (remote-tab-actions spec).
        let actions = [
            ToggleAction::Toggle,
            ToggleAction::Show,
            ToggleAction::Hide,
            ToggleAction::NewTab {
                cwd: None,
                cmd: None,
            },
            ToggleAction::NewTab {
                cwd: Some("/tmp".into()),
                cmd: None,
            },
            ToggleAction::NewTab {
                cwd: None,
                cmd: Some("htop".to_string()),
            },
            ToggleAction::NewTab {
                cwd: Some("/tmp".into()),
                cmd: Some("htop -u root".to_string()),
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
