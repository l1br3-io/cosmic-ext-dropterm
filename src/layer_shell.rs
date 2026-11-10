// SPDX-License-Identifier: GPL-3.0-only

//! The layer-shell support verdict — the first slice of backend
//! detection (portability plan, design rule 3): where the compositor
//! offers no `zwlr_layer_shell_v1` global, `DropTerm` says so instead
//! of silently never surfacing.

/// The global a drop-down surface needs: the wlr layer-shell
/// protocol's single registry name (every published version 1–4 binds
/// through it; the version is negotiated at bind, not a separate
/// global).
// not(test): the tests exercise every item, so the expectation lives
// only in the build where the startup probe (lg-2) hasn't wired them
// yet — and it errors out the moment that wiring lands.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "transient: wired by the startup probe (lg-2)")
)]
pub(crate) const LAYER_SHELL_GLOBAL: &str = "zwlr_layer_shell_v1";

/// Whether this compositor can host the drop-down surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "transient: wired by the startup probe (lg-2)")
)]
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
/// the startup probe (src/main.rs) feeds it the real registry's names.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "transient: wired by the startup probe (lg-2)")
)]
pub(crate) fn verdict(globals: &[&str]) -> Verdict {
    if globals.contains(&LAYER_SHELL_GLOBAL) {
        Verdict::Supported
    } else {
        Verdict::UnsupportedHere
    }
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
}
