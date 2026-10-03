// SPDX-License-Identifier: GPL-3.0-only

//! Stub entry for the panel-applet crate.
//!
//! The real `cosmic::applet::run` entry lands with the applet shell;
//! for now this only proves the crate builds against the locked
//! wgpu-free libcosmic feature set and reaches the root lib's V1
//! contract from outside the terminal's crate.

use cosmic_ext_dropterm::dbus_state;

/// Compile-time witness that every applet-facing item of the V1
/// contract is reachable from this crate: the workspace split is only
/// complete if the names resolve here. `V1InterfaceProxy` is the
/// generated client — the `#[proxy]` macro consumes its input trait,
/// so the proxy struct is the contract item downstream crates see.
const _V1_CONTRACT: (
    &str,
    &str,
    &str,
    Option<dbus_state::State>,
    Option<dbus_state::V1InterfaceProxy<'static>>,
) = (
    dbus_state::BUS_NAME,
    dbus_state::OBJECT_PATH,
    dbus_state::INTERFACE,
    None,
    None,
);

fn main() {}
