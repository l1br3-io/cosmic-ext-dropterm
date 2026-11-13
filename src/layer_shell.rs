// SPDX-License-Identifier: GPL-3.0-only

//! The layer-shell registry probe — the connection half of backend
//! detection (portability plan, design rule 3). The verdict function,
//! the `Verdict` type, and the unsupported-session error message live
//! in `dropterm_core::transport` (gpui-scaffold Task 3): both
//! frontends refuse identically where the session offers no
//! `zwlr_layer_shell_v1`, so the decision is made once, beside the
//! startup transport; this module stays the libcosmic binary's probe —
//! the wayland-client roundtrip feeding core the registry's names.

use wayland_client::globals::{self, GlobalListContents};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle};

/// The registry probe's dispatch state. `registry_queue_init` records
/// the globals itself in the registry's object data; this state sees
/// only the dynamic add/remove events after that, which a one-shot
/// startup probe ignores.
#[derive(Default)]
struct RegistryProbe;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for RegistryProbe {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

/// Probe the session before any surface is created: connect to
/// Wayland, enumerate the registry once, and hand the interface names
/// to core's verdict. A failure to connect or roundtrip is an error — a
/// session with no Wayland at all is one of the unsupported cases, so
/// the reason travels back for the startup error rather than crashing.
pub(crate) fn startup_verdict() -> Result<dropterm_core::transport::Verdict, String> {
    let conn = Connection::connect_to_env().map_err(|err| err.to_string())?;
    let (globals, _queue) =
        globals::registry_queue_init::<RegistryProbe>(&conn).map_err(|err| err.to_string())?;
    let global_list = globals.contents().clone_list();
    let names: Vec<&str> = global_list
        .iter()
        .map(|global| global.interface.as_str())
        .collect();
    Ok(dropterm_core::transport::verdict(&names))
}
