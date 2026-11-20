// SPDX-License-Identifier: GPL-3.0-only

//! The frontend's startup dispatch (gpui-scaffold Task 3; the paired
//! arms are macos-build mb-2): on Linux the layer-shell guard, run
//! before any GPUI initialization — the registry probe on this
//! binary's own wayland-client connection, core's verdict, and the
//! refuse-and-exit branch with the same message and exit code the
//! libcosmic binary keeps (check 11's shape). A session without
//! `zwlr_layer_shell_v1` never reaches the toolkit: no window, no bus
//! name, no surface that fails to appear while `--is-visible` lies
//! about it. On every other platform the verdict refuses outright —
//! macOS has no surface backend yet, and the refusal names it until
//! `macos-surface` replaces this arm.

use dropterm::core::transport::{self, Verdict};

#[cfg(target_os = "linux")]
use wayland_client::globals::{self, GlobalListContents};
#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_registry;
#[cfg(target_os = "linux")]
use wayland_client::{Connection, Dispatch, QueueHandle};

/// The registry probe's dispatch state. `registry_queue_init` records
/// the globals itself in the registry's object data; this state sees
/// only the dynamic add/remove events after that, which a one-shot
/// startup probe ignores — the libcosmic twin's shape exactly.
#[cfg(target_os = "linux")]
#[derive(Default)]
struct RegistryProbe;

#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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

/// The non-Linux arm (macos-build mb-2): no surface backend exists on
/// this platform yet, so the verdict refuses — never a probe, never a
/// false `Supported`. The message is the full startup error here (it
/// does not ride core's layer-shell wording, which would name the
/// wrong missing piece).
#[cfg(not(target_os = "linux"))]
pub(crate) fn startup_verdict() -> Result<Verdict, String> {
    Err(unsupported_platform_startup_error())
}

/// The macOS refusal — the marker `macos-surface` replaces (the
/// spec's success criterion: `toggle` exits non-zero with a message
/// naming macOS as not yet supported). Mirrors core's
/// unsupported-startup error's shape: the platform named, the reason,
/// the docs link.
#[cfg(not(target_os = "linux"))]
fn unsupported_platform_startup_error() -> String {
    "dropterm: macOS is not yet supported — no surface backend exists \
     here until the macos-surface module lands; see \
     https://l1br3-io.github.io/dropterm/known-issues/"
        .to_string()
}

/// The startup guard: where the session cannot host the drawer, print
/// the refusal and exit with the guard's code. Returns only on
/// `Supported` — which on non-Linux never happens (the arm above
/// refuses before any toolkit call).
pub(crate) fn guard_or_exit() {
    match startup_verdict() {
        Ok(Verdict::Supported) => {}
        Ok(Verdict::UnsupportedHere) => {
            eprintln!("{}", transport::unsupported_startup_error(None));
            std::process::exit(1);
        }
        Err(reason) => {
            // Linux: the probe's failure reason rides core's message.
            // Non-Linux: the Err IS the full message already.
            #[cfg(target_os = "linux")]
            eprintln!("{}", transport::unsupported_startup_error(Some(&reason)));
            #[cfg(not(target_os = "linux"))]
            eprintln!("{reason}");
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

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn the_macos_arm_refuses_naming_the_platform_and_the_successor() {
        // mb-2's pinned refusal path, the layer-shell-guard verdict
        // test's shape on the mac side: the verdict is a refusal (a
        // false Supported would let the boot run into a backend that
        // does not exist), the message names the platform and the
        // module that replaces it — the exact marker macos-surface
        // swaps for the real dispatch.
        let Err(reason) = startup_verdict() else {
            panic!("the non-Linux arm must refuse, never report support");
        };
        assert!(
            reason.contains("macOS"),
            "the refusal names the platform: {reason}"
        );
        assert!(
            reason.contains("macos-surface"),
            "the refusal names the module that replaces it: {reason}"
        );
        assert!(
            reason.contains("https://l1br3-io.github.io/dropterm/known-issues/"),
            "the refusal links the docs like core's does: {reason}"
        );
    }
}
