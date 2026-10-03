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
#![expect(clippy::doc_markdown, reason = "transient: T9")]
#![expect(clippy::redundant_closure_for_method_calls, reason = "transient: T9")]

//! Library target for the workspace.
//!
//! The terminal binary keeps its implementation bin-internal; this lib
//! exists so the `cosmic-ext-dropterm-applet` member consumes the V1
//! D-Bus contract from the same code that serves it — one contract,
//! one implementation.

pub mod dbus_state;
