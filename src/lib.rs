// SPDX-License-Identifier: GPL-3.0-only

// Pedantic lint table: `just clippy` runs with -D warnings, so every
// warn below is a build error. `standing:` entries are owner-approved
// exceptions (OQ rulings); `transient:` entries are the T1 census
// worklist — lints firing in this crate — and are deleted as their
// class is cleared, because an expect whose lint stops firing errors
// out (the self-cleanup mechanism). A standing entry appears only in
// roots where it fires, for the same reason.
#![warn(clippy::pedantic)]
#![cfg_attr(test, allow(clippy::float_cmp))] // OQ-1: tests assert exact constructed values
#![expect(
    clippy::must_use_candidate,
    reason = "standing: OQ-3 — no #[must_use] attribute sweep"
)]

//! Library target for the workspace.
//!
//! The terminal binary keeps its implementation bin-internal; this lib
//! exists so the `cosmic-ext-dropterm-applet` member consumes the V1
//! D-Bus contract from the same code that serves it — one contract,
//! one implementation.

pub mod dbus_state;

/// The terminal's desktop identity — the single string behind the
/// single-instance key, the D-Bus bus name and object path, the
/// `cosmic_config` namespace, the XDG state directory, the
/// layer-shell namespace, and the wayland app id. One const, owned
/// here, so no consumer can drift from another.
///
/// A cargo feature, not an environment variable: cargo does not
/// fingerprint arbitrary env vars, so an `option_env!` identity would
/// silently reuse stale binaries between identity flips.
#[cfg(not(feature = "dev-identity"))]
pub const APP_ID: &str = "io.github.l1br3_io.CosmicExtDropterm";

/// The dev build's identity (see the release const above): every
/// runtime name carries the `.Dev` suffix, so a dev build owns its own
/// single-instance slot, bus name, config file, and state directory —
/// it never forwards to, or shares files with, an installed release.
/// Enabled by the justfile's dev recipes and the applet's same-named
/// feature; never by a release build.
#[cfg(feature = "dev-identity")]
pub const APP_ID: &str = "io.github.l1br3_io.CosmicExtDropterm.Dev";
