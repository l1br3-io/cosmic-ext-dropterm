// SPDX-License-Identifier: GPL-3.0-only

//! The layer-shell support verdict — the first slice of backend
//! detection (portability plan, design rule 3): where the compositor
//! offers no `zwlr_layer_shell_v1` global, `DropTerm` says so instead
//! of silently never surfacing.

use std::fmt::Write as _;

use wayland_client::globals::{self, GlobalListContents};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle};

/// The global a drop-down surface needs: the wlr layer-shell
/// protocol's single registry name (every published version 1–4 binds
/// through it; the version is negotiated at bind, not a separate
/// global).
pub(crate) const LAYER_SHELL_GLOBAL: &str = "zwlr_layer_shell_v1";

/// The docs page the unsupported error links: the known-issues row
/// this package adds explains the verdict.
pub(crate) const UNSUPPORTED_DOCS_URL: &str = "https://l1br3-io.github.io/dropterm/known-issues/";

/// Whether this compositor can host the drop-down surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The registry advertises [`LAYER_SHELL_GLOBAL`] (COSMIC, sway):
    /// nothing changes for these compositors.
    Supported,
    /// It does not (GNOME, a sandbox, a session without Wayland): no
    /// layer surface can exist here, so none is ever created.
    UnsupportedHere,
}

/// Decide layer-shell support from the registry's advertised global
/// names. Pure — no I/O — so the decision is unit-testable headless;
/// [`startup_verdict`] feeds it the real registry's names.
pub(crate) fn verdict(globals: &[&str]) -> Verdict {
    if globals.contains(&LAYER_SHELL_GLOBAL) {
        Verdict::Supported
    } else {
        Verdict::UnsupportedHere
    }
}

/// The startup error for the unsupported verdict: names the missing
/// protocol and links the docs (the spec's always-boundary).
/// `reason` — present when the probe could not even ask (no Wayland
/// session, a broken connection) — rides along after that text.
pub(crate) fn unsupported_startup_error(reason: Option<&str>) -> String {
    let mut msg = format!(
        "dropterm: this session offers no {LAYER_SHELL_GLOBAL} \
         (the Wayland layer-shell protocol a drop-down terminal's surface \
         needs), so DropTerm cannot start here; see {UNSUPPORTED_DOCS_URL}"
    );
    if let Some(reason) = reason {
        // write! into a String cannot fail; clippy::format_push_string
        // prefers it over push_str(&format!(..)).
        let _ = write!(msg, " (probe: {reason})");
    }
    msg
}

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
/// to [`verdict`]. A failure to connect or roundtrip is an error — a
/// session with no Wayland at all is one of the unsupported cases, so
/// the reason travels back for [`unsupported_startup_error`] rather
/// than crashing.
pub(crate) fn startup_verdict() -> Result<Verdict, String> {
    let conn = Connection::connect_to_env().map_err(|err| err.to_string())?;
    let (globals, _queue) =
        globals::registry_queue_init::<RegistryProbe>(&conn).map_err(|err| err.to_string())?;
    let global_list = globals.contents().clone_list();
    let names: Vec<&str> = global_list
        .iter()
        .map(|global| global.interface.as_str())
        .collect();
    Ok(verdict(&names))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_when_the_registry_advertises_layer_shell() {
        // A COSMIC-like registry: layer-shell among the usual globals.
        let globals = [
            "wl_compositor",
            "wl_shm",
            "wl_seat",
            "xdg_wm_base",
            "zwlr_layer_shell_v1",
            "zcosmic_toplevel_info_v1",
        ];
        assert_eq!(verdict(&globals), Verdict::Supported);
    }

    #[test]
    fn unsupported_when_the_registry_lacks_layer_shell() {
        // A GNOME-like registry: everything a toplevel app needs, but
        // no layer-shell — a drop-down surface cannot exist there.
        let globals = [
            "wl_compositor",
            "wl_shm",
            "wl_seat",
            "xdg_wm_base",
            "xdg_activation_v1",
            "zwp_idle_inhibit_manager_v1",
        ];
        assert_eq!(verdict(&globals), Verdict::UnsupportedHere);
    }

    #[test]
    fn unsupported_on_an_empty_registry() {
        // Degenerate input class: the roundtrip yielded no globals at
        // all (a proxy registry, or a compositor mid-shutdown).
        assert_eq!(verdict(&[]), Verdict::UnsupportedHere);
    }

    #[test]
    fn the_match_is_exact_not_substring() {
        // No real global's name contains the protocol's today; this
        // pins the equality contract against a substring refactor.
        let lookalikes = ["zwlr_layer_shell_v1_debug", "zwlr_layer_shell_v2"];
        for globals in lookalikes.map(|name| [name]) {
            assert_eq!(verdict(&globals), Verdict::UnsupportedHere);
        }
    }

    #[test]
    fn the_unsupported_startup_error_names_the_protocol_and_links_the_docs() {
        // The spec's always-boundary: whatever the session, the error
        // names the missing protocol and points at the docs.
        let msg = unsupported_startup_error(None);
        assert!(msg.contains(LAYER_SHELL_GLOBAL));
        assert!(msg.contains(UNSUPPORTED_DOCS_URL));
    }

    #[test]
    fn the_unsupported_startup_error_carries_the_probe_reason_when_there_is_one() {
        // A session with no Wayland at all: the reason rides along,
        // and the protocol + docs boundary still holds.
        let msg = unsupported_startup_error(Some("no wayland socket"));
        assert!(msg.contains("no wayland socket"));
        assert!(msg.contains(LAYER_SHELL_GLOBAL));
        assert!(msg.contains(UNSUPPORTED_DOCS_URL));
    }
}
