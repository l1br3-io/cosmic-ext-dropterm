// SPDX-License-Identifier: GPL-3.0-only

//! The GPUI frontend's startup dispatch: the surface-strategy
//! decision, run before any GPUI initialization — the registry probe
//! on this binary's own wayland-client connection, core's verdict,
//! and the refuse-and-exit branch for the one unbootable session (no
//! Wayland connection at all), with the same message and exit code
//! the guard era kept. A Wayland session without
//! `zwlr_layer_shell_v1` returns [`Verdict::PlainWindow`] and boots
//! the plain-window strategy; only the probe's error arm refuses.

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

/// The startup dispatch: probe the session and return the surface
/// strategy it boots under. Refuses — core's unsupported-startup
/// error naming the missing protocol, linking the docs, carrying the
/// probe's reason, then exit 1 — only where the probe could not even
/// ask: a session with no Wayland connection at all. Both verdicts
/// are bootable; the caller threads the choice into the executor.
pub(crate) fn surface_mode_or_exit() -> Verdict {
    match startup_verdict() {
        Ok(mode) => mode,
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
    fn the_dispatch_refusal_is_core_message_plus_exit_one_shape() {
        // The refusal (now the probe's error arm alone) keeps the
        // guard-era contract: the message is exactly core's —
        // protocol named, docs linked, the probe reason riding only
        // when the probe could not even ask — and the exit is 1. The
        // refuse arm itself calls exit(1), so what is testable
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
