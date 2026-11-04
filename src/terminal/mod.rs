// SPDX-License-Identifier: GPL-3.0-only

//! The terminal engine and widget: a `Terminal` (PTY + `alacritty_terminal`
//! grid + cosmic-text buffer) rendered by a `TerminalBox` iced widget.
//!
//! Adapted from cosmic-term (<https://github.com/pop-os/cosmic-term>);
//! see the module-level comments in `engine.rs`, `widget.rs` and
//! `theme_source.rs` for what changed and why.

mod engine;
pub mod mouse_reporter;
// pub(crate) for one consumer: the app layer loads the spec table's
// data file (suggest-specs-data spec) and hands it down through
// `TerminalOptions` — the model itself never reads files.
pub(crate) mod suggest;
pub mod theme_source;
mod widget;

pub use engine::{
    ProgressState, ReportedCwd, ReportedProgress, Terminal, TerminalOptions, TerminalScroll,
    WINDOW_BG_COLOR,
};
pub use widget::{Action, TerminalBox};
