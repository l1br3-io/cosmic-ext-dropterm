// SPDX-License-Identifier: GPL-3.0-only

//! The terminal's libcosmic frontend: the render widget
//! (`TerminalBox`), the iced input translation, and the toolkit's
//! text host. The engine itself (PTY + `alacritty_terminal` grid +
//! suggest model) lives in `dropterm-core` (shared-core spec) and is
//! re-exported whole, so every `crate::terminal::…` path keeps
//! resolving; the cosmic-text buffers stay here with the widget
//! (spike check 9's boundary — they have no role on GPUI's path).
//!
//! Adapted from cosmic-term (<https://github.com/pop-os/cosmic-term>);
//! see the module-level comments in the engine (core) and `widget.rs`
//! for what changed and why.

mod host;
mod input;
mod widget;

pub use dropterm_core::terminal::{
    Metadata, ProgressState, Terminal, TerminalOptions, TerminalScroll, TextHost, WINDOW_BG_COLOR,
};
// The status model's engine types: core's status module owns the
// paths now, and only the tests still name them through
// `crate::terminal` (their historical import path).
#[cfg(test)]
pub use dropterm_core::terminal::{ProgramState, ProgramStatus, ReportedCwd, ReportedProgress};

// The app layer loads the spec table's data file (suggest-specs-data
// spec) and hands it down through `TerminalOptions` — the model
// itself never reads files.
pub use dropterm_core::terminal::suggest;

pub use host::CosmicHost;
pub use widget::{Action, TerminalBox};

// The images panel's thumbnail decode (paste-panel spec) rides the
// engine's own decompression-bomb guard — clipboard-originated cache
// files meet the same ceilings every OSC decode already had.
pub(crate) use dropterm_core::terminal::decode_image;
