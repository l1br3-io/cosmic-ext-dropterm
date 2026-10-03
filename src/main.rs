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

use clap::Parser;

use app::{DropTerm, Flags, ToggleAction};

/// A native, Yakuake/Guake-style drop-down terminal for the COSMIC desktop.
///
/// A second launch toggles the running instance instead of opening a new
/// one; see the project README for how to bind this to a key in COSMIC
/// Settings.
#[derive(Parser, Debug, Clone)]
#[command(name = "cosmic-ext-dropterm", version, about)]
struct Args {
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
