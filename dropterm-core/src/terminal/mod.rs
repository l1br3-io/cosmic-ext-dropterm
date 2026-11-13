// SPDX-License-Identifier: GPL-3.0-only

//! The terminal engine (shared-core spec): PTY + `alacritty_terminal`
//! grid + suggest model, toolkit-free. The cosmic-text **buffers**
//! and everything iced-typed — input translation, the render widget —
//! stay with the frontend (spike check 9's boundary: the buffers are
//! widget-serving caches with no role on GPUI's path; the font
//! system that shapes them belongs to the toolkit). The
//! [`TextHost`] trait is the seam: the frontend injects a host that
//! owns the buffer and measures through its font system.
//!
//! Adapted from cosmic-term (<https://github.com/pop-os/cosmic-term>);
//! see the module-level comments in `engine.rs` and `theme_source.rs`
//! for what changed and why.

mod engine;

// The app layer loads the spec table's data file (suggest-specs-data
// spec) and hands it down through `TerminalOptions` — the model
// itself never reads files.
pub mod suggest;

mod host;

pub use engine::{
    CommandTimer, ImageStore, Metadata, ProgramState, ProgramStatus, ProgressState, ReportedCwd,
    ReportedProgress, Terminal, TerminalOptions, TerminalScroll, WINDOW_BG_COLOR,
};

// The images panel's thumbnail decode (paste-panel spec) rides the
// engine's own decompression-bomb guard.
pub use engine::{decode_image, ssh_destination};

#[cfg(test)]
pub use host::PlainHost;
pub use host::TextHost;
