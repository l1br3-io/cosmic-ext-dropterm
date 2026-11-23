//! `DropTerm`'s toolkit-neutral core (the collapsed dropterm-core,
//! gpui-cutover cu-4) — the engine, models, and contract the bin's
//! views drive.
//!
//! The config store and model, session persistence, profiles, the
//! surface intent vocabulary, the theme data table, the pure geometry
//! and keybind models, and the served-state D-Bus contract live here
//! (shared-core spec). This module admits no toolkit dependency —
//! libcosmic, iced, or GPUI — pinned by test
//! (`tests/core_is_toolkit_free.rs` walking the module's sources):
//! the frontend-typed halves stay in the bin's own modules.

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
/// Enabled by this crate's same-named feature; never by a release
/// build.
#[cfg(feature = "dev-identity")]
pub const APP_ID: &str = "io.github.l1br3_io.dropterm.Dev";

pub mod config;
pub mod context_menu;
pub mod dbus_state;
pub mod font_list;
pub mod geometry;
pub mod global_shortcut;
pub mod intent;
pub mod keybinds;
pub mod lua_host;
pub mod mem;
pub mod palette;
pub mod paste_cache;
pub mod profile;
pub mod session;
pub mod settings;
pub mod slide;
pub mod slots;
pub mod spawn;
pub mod status;
pub mod surface_lifecycle;
pub mod system_mode;
pub mod tabs;
pub mod terminal;
pub mod theme_files;
pub mod theme_glue;
pub mod theme_source;
pub mod transport;
