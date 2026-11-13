// SPDX-License-Identifier: GPL-3.0-only

// Pedantic lint table (same convention as the root crate's):
// `just clippy` runs with -D warnings, so every warn below is a
// build error. `standing:` entries are owner-approved exceptions
// (OQ rulings); a standing entry appears only in roots where it
// fires, for the same reason as the root crate's table.
#![warn(clippy::pedantic)]
#![cfg_attr(test, allow(clippy::float_cmp))] // OQ-1: tests assert exact constructed values
#![cfg_attr(test, allow(clippy::assert_is_empty))] // tests read better asserting is_empty
#![expect(
    clippy::too_many_lines,
    reason = "standing: OQ-4 — dissolved by module-decomposition"
)]
// Exported-API standings the bin never needed (a bin exports nothing):
// the engine's constructor/OSC handlers predate the lib split; docs
// land with the GPUI slices that make them consumer-facing.
#![expect(
    clippy::missing_errors_doc,
    reason = "standing: engine ctor docs predate the core split"
)]
#![expect(
    clippy::missing_panics_doc,
    reason = "standing: engine OSC handlers lock a Mutex the engine owns"
)]
#![expect(
    clippy::must_use_candidate,
    reason = "standing: OQ-3 — no #[must_use] attribute sweep"
)]

//! `DropTerm`'s toolkit-neutral core — the state both frontends share.
//!
//! The config store and model, session persistence, profiles, the
//! surface intent vocabulary, the theme data table, the pure geometry
//! and keybind models, and the served-state D-Bus contract live here
//! (shared-core spec), so the GPUI frontend builds on exactly the
//! engine state the libcosmic binary runs today, and the panel applet
//! consumes the same contract implementation the terminal serves. This
//! crate admits no toolkit dependency — libcosmic, iced, or GPUI —
//! pinned by test (`tests/no_toolkit_deps.rs`): the frontend-typed
//! halves stay with their frontends.

/// The terminal's desktop identity — the single string behind the
/// single-instance key, the D-Bus bus name and object path, the
/// config store directory, the XDG state directory, the
/// layer-shell namespace, and the wayland app id. One const, owned
/// here, so no consumer can drift from another.
///
/// A cargo feature, not an environment variable: cargo does not
/// fingerprint arbitrary env vars, so an `option_env!` identity would
/// silently reuse stale binaries between identity flips.
#[cfg(not(feature = "dev-identity"))]
pub const APP_ID: &str = "io.github.l1br3_io.dropterm";

/// The dev build's identity (see the release const above): every
/// runtime name carries the `.Dev` suffix, so a dev build owns its own
/// single-instance slot, bus name, config file, and state directory —
/// it never forwards to, or shares files with, an installed release.
/// Enabled by the root crate's same-named feature (which the applet
/// forwards to); never by a release build.
#[cfg(feature = "dev-identity")]
pub const APP_ID: &str = "io.github.l1br3_io.dropterm.Dev";

pub mod config;
pub mod dbus_state;
pub mod geometry;
pub mod intent;
pub mod keybinds;
pub mod profile;
pub mod session;
pub mod tabs;
pub mod terminal;
pub mod theme_glue;
pub mod theme_source;
