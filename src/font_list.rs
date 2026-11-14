// SPDX-License-Identifier: GPL-3.0-only

//! The font picker's enumeration seam (font-picker spec): the pure
//! collector and the one fontdb scan moved to
//! [`dropterm_core::font_list`] by the gpui-chrome spec — this front
//! re-exports it so the bin's `crate::font_list` paths keep
//! resolving unchanged (the move is a re-home, not a rewrite).

pub use dropterm_core::font_list::load_monospace_families;
