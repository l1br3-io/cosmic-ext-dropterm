// SPDX-License-Identifier: GPL-3.0-only

mod app;
mod config;
mod keybinds;
mod surface;
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
    let flags = Flags {
        action: args.action.unwrap_or_default(),
    };

    let settings = cosmic::app::Settings::default()
        .no_main_window(true)
        .exit_on_close(false);

    cosmic::app::run_single_instance::<DropTerm>(settings, flags)
}
