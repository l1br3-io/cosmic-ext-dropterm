// SPDX-License-Identifier: GPL-3.0-only

// Pedantic lint table (same convention as the bin's): `just clippy`
// runs with -D warnings, so every warn below is a build error.
// `standing:` entries are owner-approved exceptions (OQ rulings); a
// standing entry appears only in roots where it fires.
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
// Another exported-API standing the bin never needed (the settings
// model's move made it fire): `PlannedWrite::clamped` returns Self by
// contract — the write path's own one call site — and rides OQ-3's
// no-sweep ruling with its sibling above.
#![expect(
    clippy::return_self_not_must_use,
    reason = "standing: OQ-3 — no #[must_use] attribute sweep"
)]

//! `dropterm`'s lib half: the `core` module, toolkit-free.
//!
//! The bin's GPUI view half and this library are one package — one
//! CLI, one crate (gpui-cutover cu-4). The lib target exists so the
//! core's API surface keeps its crate-boundary reachability (its
//! tests, the wire tests, and the bin all consume it as a library).

pub mod core;
