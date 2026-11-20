// SPDX-License-Identifier: GPL-3.0-only

//! The GPUI frontend's startup dispatch (gpui-scaffold Task 3): the
//! layer-shell guard, run before any GPUI initialization — the
//! registry probe on this binary's own wayland-client connection,
//! core's verdict, and the refuse-and-exit branch with the same
//! message and exit code the libcosmic binary keeps (check 11's
//! shape). A session without `zwlr_layer_shell_v1` never reaches the
//! toolkit: no window, no bus name, no surface that fails to appear
//! while `--is-visible` lies about it.

use wayland_client::globals::{self, GlobalListContents};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle};

use dropterm::core::transport::{self, Verdict};

/// The registry probe's dispatch state. `registry_queue_init` records
/// the globals itself in the registry's object data; this state sees
/// only the dynamic add/remove events after that, which a one-shot
/// startup probe ignores — the libcosmic twin's shape exactly.
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

/// Probe the session before any surface or toolkit exists: connect to
/// Wayland, enumerate the registry once, and hand the interface names
/// to core's verdict. A failure to connect or roundtrip is an error —
/// a session with no Wayland at all is one of the unsupported cases,
/// so the reason travels back with the verdict for the startup error.
pub(crate) fn startup_verdict() -> Result<Verdict, String> {
    let conn = Connection::connect_to_env().map_err(|err| err.to_string())?;
    let (globals, _queue) =
        globals::registry_queue_init::<RegistryProbe>(&conn).map_err(|err| err.to_string())?;
    let global_list = globals.contents().clone_list();
    let names: Vec<&str> = global_list
        .iter()
        .map(|global| global.interface.as_str())
        .collect();
    Ok(transport::verdict(&names))
}

/// The startup guard: where the session cannot host the drawer, print
/// core's unsupported-startup error (naming the missing protocol,
/// linking the docs, carrying the probe reason only when the probe
/// could not even ask) and refuse with the guard's exit code. Returns
/// only on `Supported`.
pub(crate) fn guard_or_exit() {
    match startup_verdict() {
        Ok(Verdict::LayerShell) => {}
        // A plain-window session must not fall through to the
        // layer-shell executor: a layer surface cannot exist there,
        // and booting one anyway is the failure the guard exists for
        // (a window that never appears while `--is-visible` could lie
        // about it). Until the executor grows its plain surface, the
        // guard keeps refusing — core's message, the guard's exit 1,
        // both unchanged from the refusal era.
        Ok(Verdict::PlainWindow) => {
            eprintln!("{}", transport::unsupported_startup_error(None));
            std::process::exit(1);
        }
        Err(reason) => {
            eprintln!("{}", transport::unsupported_startup_error(Some(&reason)));
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guard_refusal_is_core_message_plus_exit_one_shape() {
        // The no-layer-shell branch keeps the guard's contract: the
        // message is exactly core's (protocol named, docs linked, no
        // probe reason when the roundtrip succeeded and the registry
        // merely lacked the global), and the exit is the guard's 1 —
        // the refuse arms both call exit(1), so what is testable
        // headless is that each refusal message is core's function of
        // its reason arm.
        let asked_and_no = transport::unsupported_startup_error(None);
        assert!(asked_and_no.contains("zwlr_layer_shell_v1"));
        assert!(
            !asked_and_no.contains("(probe:"),
            "no probe reason: {asked_and_no}"
        );
        let cannot_ask = transport::unsupported_startup_error(Some("no wayland socket"));
        assert!(cannot_ask.contains("(probe: no wayland socket)"));
    }
}
