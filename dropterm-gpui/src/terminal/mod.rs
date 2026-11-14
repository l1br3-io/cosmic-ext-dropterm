// SPDX-License-Identifier: GPL-3.0-only

//! The terminal view (gpui-terminal spec): the alacritty grid
//! rendered through GPUI's text stack — the view and input halves
//! `src/terminal/widget.rs` is for the libcosmic frontend. The
//! engine is [`dropterm_core::terminal::Terminal`], consumed
//! unchanged: the view reads the grid under its lock, drives
//! `resize`/`update`/`input` on the engine's own paths, and renders
//! rows shaped through GPUI's shaper with the row's width forced to
//! the cell grid (the spike's check-9 route — the engine-side
//! cosmic-text buffers are widget-serving caches with no role on
//! this path).
//!
//! Module map: [`grid`] is the pure row projection (headless-tested),
//! [`host`] is the engine's `TextHost` seam implemented against
//! GPUI's font system, and [`view`] is the render entity.

pub(crate) mod grid;
pub(crate) mod host;
pub(crate) mod input;
pub(crate) mod selection;
pub(crate) mod view;
