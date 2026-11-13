// SPDX-License-Identifier: GPL-3.0-only

// Pedantic lint table: `just clippy` runs with -D warnings, so every
// warn below is a build error. `standing:` entries are owner-approved
// exceptions (OQ rulings); `transient:` entries are the T1 census
// worklist — lints firing in this crate — and are deleted as their
// class is cleared, because an expect whose lint stops firing errors
// out (the self-cleanup mechanism). A standing entry appears only in
// roots where it fires, for the same reason. The float_cmp transient
// sits after the test blanket so the blanket owns test builds.
#![warn(clippy::pedantic)]
#![cfg_attr(test, allow(clippy::float_cmp))] // OQ-1: tests assert exact constructed values
#![cfg_attr(test, allow(clippy::assert_is_empty))] // tests read better asserting is_empty than assert_eq!(x, [] as [T; 0])
#![expect(
    clippy::too_many_lines,
    reason = "standing: OQ-4 — dissolved by module-decomposition"
)]

mod app;
mod clipboard;
mod context_menu;
mod drawer_handle;
mod executor;
mod font_list;
mod geometry;
mod grab_bar;
mod keybinds;
mod layer_shell;
mod notify;
mod settings;
mod slide;
mod status_bar;
mod surface;
mod tab_bar;
mod terminal;

// The toolkit-neutral movers (shared-core spec) live in the
// `dropterm-core` workspace member: config (store and model),
// session, profile, intent, the served-state D-Bus contract, and
// the startup transport. Imported at the crate root so every
// `crate::config`/`crate::session`/`crate::dbus_state`/… path in
// the bin's modules keeps resolving unchanged — the move is a
// re-home, not a rewrite.
use dropterm_core::{
    config, dbus_state, intent, profile, session, tabs, theme_glue, theme_source, transport,
};

use clap::Parser;

use app::DropTerm;
use app::remote::{Flags, ToggleAction};

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
    name = "dropterm",
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

/// The bus half of `--is-visible`: `Some(visible)` when a running
/// instance answered, `None` when the name is unowned (no note owed)
/// or owned without the V1 interface (stale binary mid-upgrade —
/// noted on stderr). Query-only: nothing here activates or spawns the
/// app, so the flag can never summon the terminal.
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
            dbus_state::V1InterfaceProxy::new(&conn, dbus_state::BUS_NAME, dbus_state::OBJECT_PATH)
                .await?
                .visible()
                .await
        };
        if let Ok(visible) = read.await {
            Some(visible)
        } else {
            eprintln!(
                "dropterm: bus name owned but the V1 state \
                 interface is missing (stale binary?); treating as not \
                 visible"
            );
            None
        }
    })
}

/// libcosmic's debug escape hatch, kept (control-transport):
/// `COSMIC_SINGLE_INSTANCE=false` (or `0`) runs this launch as a
/// plain instance — no forwarding, no name ownership, no applet
/// state — so a broken primary never blocks debugging a fresh one.
/// Read once per process: main's launch decision and the app's
/// control-bus subscription must agree for the whole run.
pub(crate) fn single_instance_disabled() -> bool {
    static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("COSMIC_SINGLE_INSTANCE")
            .is_ok_and(|value| value.to_lowercase() == "false" || value == "0")
    })
}

// Who owns the control bus's name and the second launch's forward
// through the activation member live in the core crate's `transport`
// module (shared-core spec), beside the contract they serve; the
// launch decision below drives them.
fn main() -> cosmic::iced::Result {
    env_logger::init();

    // Matches cosmic-term's own startup: alacritty_terminal doesn't set
    // these itself, and a correct TERM is needed for full-color, mouse and
    // bracketed-paste support in most shells.
    alacritty_terminal::tty::setup_env();
    // SAFETY: called once, before any thread that reads the environment
    // is spawned (per-terminal PTY threads start later).
    unsafe {
        std::env::set_var("TERM", "xterm-256color");
    }

    let args = Args::parse();
    // The query path exits before any bus-name attempt so a second
    // launch asking a question never forwards an action to the
    // running instance.
    if args.is_visible {
        let (visible, code) = dbus_state::is_visible_decision(query_visibility());
        println!("{visible}");
        std::process::exit(i32::from(code));
    }
    let flags = Flags::new(args.action.unwrap_or_default());

    // The layer-shell guard (spec: fail loudly without layer-shell):
    // before any surface is created, ask the session's registry for
    // the protocol a drop-down surface needs. Where it is missing
    // (GNOME, a sandbox, no Wayland session), say so and exit — the
    // old failure mode was starting, never surfacing, and
    // `--is-visible` answering true. The query path above stays ahead
    // of the probe: it creates no surfaces, and its not-running
    // answer is truthful because the guard keeps an unsupported
    // session from ever hosting an instance.
    let session = layer_shell::startup_verdict();
    if !matches!(session, Ok(transport::Verdict::Supported)) {
        eprintln!(
            "{}",
            transport::unsupported_startup_error(session.err().as_deref())
        );
        std::process::exit(1);
    }

    // The control bus's launch half (control-transport): decide on a
    // throwaway connection whether this launch is the instance. The
    // ownership itself is taken inside the app's runtime, by the
    // control-bus subscription; `run_single_instance` used to make
    // this decision by attempting the forward itself.
    if !single_instance_disabled() {
        match transport::bus_name_status() {
            transport::BusName::Unowned | transport::BusName::NoBus => {}
            transport::BusName::OwnedByAnother => {
                // This launch's verb serialized the way libcosmic's
                // own client did: the verb's Display name plus its
                // forward words (`CosmicFlags::args`) — the frontend
                // half the core forward leaves to its caller.
                let action = flags.action.to_string();
                let args = cosmic::app::CosmicFlags::args(&flags);
                match transport::forward_outcome(&transport::forward_to_running(&action, args)) {
                    // The running instance applied the verb; a second
                    // surface must never appear. Today's contract: the
                    // forwarding launch exits 0.
                    transport::ForwardOutcome::Delivered => std::process::exit(0),
                    // The name is owned but nobody answered — a stale
                    // binary mid-upgrade. libcosmic's second launch fell
                    // through to a full duplicate instance here; this
                    // refuses instead, in the query path's own wording.
                    transport::ForwardOutcome::Refused => {
                        eprintln!(
                            "dropterm: another instance owns the bus name but did not \
                             answer (stale binary?); refusing to start a second instance"
                        );
                        std::process::exit(1);
                    }
                }
            }
        }
    }

    let settings = cosmic::app::Settings::default()
        .no_main_window(true)
        .exit_on_close(false);

    cosmic::app::run::<DropTerm>(settings, flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(
        clippy::similar_names,
        reason = "with_cwd/with_cmd/with_both name the tested flag combos"
    )]
    fn new_tab_parses_with_either_or_both_flags() {
        let plain = Args::parse_from(["dropterm", "new-tab"]).action;
        assert_eq!(
            plain,
            Some(ToggleAction::NewTab {
                cwd: None,
                cmd: None,
                profile: None
            })
        );

        let with_cwd = Args::parse_from(["dropterm", "new-tab", "--cwd", "/tmp"]).action;
        assert_eq!(
            with_cwd,
            Some(ToggleAction::NewTab {
                cwd: Some("/tmp".into()),
                cmd: None,
                profile: None
            })
        );

        let with_cmd = Args::parse_from(["dropterm", "new-tab", "--cmd", "htop -u root"]).action;
        assert_eq!(
            with_cmd,
            Some(ToggleAction::NewTab {
                cwd: None,
                cmd: Some("htop -u root".to_string()),
                profile: None
            })
        );

        let with_both =
            Args::parse_from(["dropterm", "new-tab", "--cwd", "/tmp", "--cmd", "htop"]).action;
        assert_eq!(
            with_both,
            Some(ToggleAction::NewTab {
                cwd: Some("/tmp".into()),
                cmd: Some("htop".to_string()),
                profile: None
            })
        );
    }

    #[test]
    fn new_tab_profile_parses_alone_and_composed_with_cwd() {
        let alone = Args::parse_from(["dropterm", "new-tab", "--profile", "ssh-web"]).action;
        assert_eq!(
            alone,
            Some(ToggleAction::NewTab {
                cwd: None,
                cmd: None,
                profile: Some("ssh-web".to_string())
            })
        );

        // The explicit --cwd composes with --profile (OQ-2): the tab
        // runs the profile in the requested directory.
        let composed = Args::parse_from([
            "dropterm",
            "new-tab",
            "--cwd",
            "/tmp",
            "--profile",
            "ssh-web",
        ])
        .action;
        assert_eq!(
            composed,
            Some(ToggleAction::NewTab {
                cwd: Some("/tmp".into()),
                cmd: None,
                profile: Some("ssh-web".to_string())
            })
        );
    }

    #[test]
    fn profile_conflicts_with_cmd_at_parse() {
        // A --cmd run is not a profile run; clap rejects the pairing
        // instead of guessing which one wins.
        let result =
            Args::try_parse_from(["dropterm", "new-tab", "--profile", "x", "--cmd", "htop"]);
        assert!(result.is_err());
    }

    #[test]
    fn empty_profile_is_rejected_at_parse() {
        // An empty name cannot resolve to anything; it dies at the CLI
        // edge exactly like an empty --cmd.
        let result = Args::try_parse_from(["dropterm", "new-tab", "--profile", ""]);
        assert!(result.is_err());
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
        // Tab indices are 1-based; zero dies at the CLI edge, exactly
        // like new-tab's empty --cmd.
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
        // Indices are strict 1-based; zero dies at the CLI edge like
        // close-tab's --tab-index 0.
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
    fn switch_profile_tab_parses_bare_and_indexed() {
        let bare = Args::parse_from(["dropterm", "switch-profile-tab", "--profile", "htop"]).action;
        assert_eq!(
            bare,
            Some(ToggleAction::SwitchProfileTab {
                profile: "htop".to_string(),
                tab_index: None
            })
        );

        let indexed = Args::parse_from([
            "dropterm",
            "switch-profile-tab",
            "--profile",
            "htop",
            "--tab-index",
            "2",
        ])
        .action;
        assert_eq!(
            indexed,
            Some(ToggleAction::SwitchProfileTab {
                profile: "htop".to_string(),
                tab_index: Some(2)
            })
        );
    }

    #[test]
    fn switch_profile_tab_requires_a_profile_name() {
        // --profile is the verb's whole payload: a bare
        // switch-profile-tab is a clap error, never a runtime guess.
        let result = Args::try_parse_from(["dropterm", "switch-profile-tab"]);
        assert!(result.is_err());
    }

    #[test]
    fn switch_profile_tab_rejects_a_zero_tab_index() {
        let result = Args::try_parse_from([
            "dropterm",
            "switch-profile-tab",
            "--profile",
            "htop",
            "--tab-index",
            "0",
        ]);
        assert!(result.is_err());
    }
}
