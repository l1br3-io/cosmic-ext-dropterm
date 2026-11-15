// SPDX-License-Identifier: GPL-3.0-only

//! The startup transport's halves, shared by both frontends: the
//! bus-name ownership decision, the second-launch forward through the
//! activation member, the forward's outcome decision, the
//! platform-data dictionary (moved from the frontend's `main.rs`
//! beside the contract they serve — both frontends serve the same
//! activation shape, so the halves live where the contract does), and
//! the layer-shell startup verdict (moved from the libcosmic
//! frontend's `layer_shell` module at gpui-scaffold Task 3: both
//! frontends refuse identically where the session offers no
//! `zwlr_layer_shell_v1`, so the verdict function lives where both
//! reach it; the registry probe itself stays per-frontend, a
//! wayland-client connection each toolkit wires its own way).

use crate::core::dbus_state;

/// Who owns the control bus's name, asked on a throwaway connection:
/// the ownership itself is taken later, on the long-lived connection
/// the app's runtime builds (a zbus connection's tasks ride the
/// runtime that built it — a name taken here would die with this
/// connection).
pub enum BusName {
    Unowned,
    OwnedByAnother,
    NoBus,
}

pub fn bus_name_status() -> BusName {
    // The query path's runtime pattern: current-thread is enough for
    // one synchronous-looking question.
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return BusName::NoBus;
    };
    runtime.block_on(async {
        let conn = match zbus::connection::Builder::session() {
            Ok(builder) => match builder.build().await {
                Ok(conn) => conn,
                Err(_) => return BusName::NoBus,
            },
            Err(_) => return BusName::NoBus,
        };
        let Ok(name) = dbus_state::BUS_NAME.try_into() else {
            return BusName::NoBus;
        };
        match zbus::fdo::DBusProxy::new(&conn).await {
            Ok(dbus) => match dbus.name_has_owner(name).await {
                Ok(true) => BusName::OwnedByAnother,
                Ok(false) => BusName::Unowned,
                Err(_) => BusName::NoBus,
            },
            Err(_) => BusName::NoBus,
        }
    })
}

/// The platform-data dictionary the forward carries, from the env a
/// launcher sets: the activation token (`XDG_ACTIVATION_TOKEN`) and the
/// startup id (`DESKTOP_STARTUP_ID`). Exactly libcosmic's keys — the
/// wire is the compatibility surface across an upgrade, and an
/// old-served instance still reads them.
pub fn platform_data(
    activation_token: Option<String>,
    desktop_startup_id: Option<String>,
) -> std::collections::HashMap<&'static str, zbus::zvariant::Value<'static>> {
    let mut data = std::collections::HashMap::new();
    if let Some(token) = activation_token {
        data.insert("activation-token", zbus::zvariant::Value::Str(token.into()));
    }
    if let Some(startup_id) = desktop_startup_id {
        data.insert(
            "desktop-startup-id",
            zbus::zvariant::Value::Str(startup_id.into()),
        );
    }
    data
}

/// The second launch's half of the control bus (control-transport):
/// hand this launch's verb to the running instance through the
/// activation member and exit. The member is `ActivateAction` with
/// the verb's Display name and its forward words — exactly what
/// libcosmic's client sent (`DropTerm`'s `CosmicFlags::action` is
/// always `Some`, so the plain `Activate` member never fired for it).
/// The caller serializes its own verb into `action` and `args`; the
/// words are opaque here, so every frontend forwards through the same
/// member with its own CLI's serialization. Fire-and-forget: the
/// call's reply carries no payload, its success is the signal.
pub fn forward_to_running(action: &str, args: Vec<&str>) -> Result<(), zbus::Error> {
    // The throwaway-connection pattern of the queries: zbus needs a
    // runtime with io and time; a current-thread runtime is enough
    // for one synchronous-looking call. Its feature set arrives
    // through zbus's own tokio dependency.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| zbus::Error::Handshake(format!("no runtime: {err}")))?;
    runtime.block_on(async {
        let conn = zbus::connection::Builder::session()
            .map_err(|err| zbus::Error::Handshake(err.to_string()))?
            .build()
            .await?;
        let proxy = dbus_state::ActivationInterfaceProxy::new(
            &conn,
            dbus_state::BUS_NAME,
            dbus_state::OBJECT_PATH,
        )
        .await?;
        let data = platform_data(
            std::env::var("XDG_ACTIVATION_TOKEN").ok(),
            std::env::var("DESKTOP_STARTUP_ID").ok(),
        );
        // zbus 5 carries no default method timeout, so a wedged owner
        // (alive, name held, never answering — the stale-binary window
        // the refusal exists for) would hang this launch forever:
        // every keybind press blocks. Bound the wait; silence past the
        // deadline is the same refusal as a failed send. Found on the
        // gpui-scaffold walk against a SIGSTOPped holder (sf-4,
        // 2026-11-13); the bound applies to both frontends.
        let deadline = std::time::Duration::from_secs(FORWARD_DEADLINE_SECS);
        match tokio::time::timeout(deadline, proxy.activate_action(action, args, data)).await {
            Ok(result) => result,
            Err(_) => Err(zbus::Error::Handshake(format!(
                "no answer from the bus name owner within {FORWARD_DEADLINE_SECS} s"
            ))),
        }
    })
}

/// How long a second launch waits for the running instance to take its
/// verb before refusing (see [`forward_to_running`]): generous against
/// a busy instance answering in milliseconds, short against a wedged
/// one blocking every summon.
pub const FORWARD_DEADLINE_SECS: u64 = 5;

/// What a forward's result means for this launch, pure so the one
/// deliberate delta from libcosmic's second-launch behavior is
/// pinned headless: a delivered verb exits 0 (the contract's code),
/// and a name owned but unanswered — a stale binary mid-upgrade —
/// refuses with exit 1, where libcosmic fell through to a full
/// duplicate instance (two terminals, the applet talking to the
/// corpse).
pub enum ForwardOutcome {
    /// The running instance took the verb; this launch exits 0.
    Delivered,
    /// Nobody answered behind the owned name; this launch exits 1
    /// with the stale-binary note.
    Refused,
}

pub fn forward_outcome(result: &Result<(), zbus::Error>) -> ForwardOutcome {
    match result {
        Ok(()) => ForwardOutcome::Delivered,
        Err(_) => ForwardOutcome::Refused,
    }
}

/// The global a drop-down surface needs: the wlr layer-shell
/// protocol's single registry name (every published version 1–4 binds
/// through it; the version is negotiated at bind, not a separate
/// global).
const LAYER_SHELL_GLOBAL: &str = "zwlr_layer_shell_v1";

/// The docs page the unsupported error links: the known-issues row
/// explaining the verdict.
const UNSUPPORTED_DOCS_URL: &str = "https://l1br3-io.github.io/dropterm/known-issues/";

/// Whether this compositor can host the drop-down surface (moved from
/// the libcosmic frontend's `layer_shell` module: both frontends
/// refuse identically, so the decision lives once).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The registry advertises [`LAYER_SHELL_GLOBAL`] (COSMIC, sway):
    /// nothing changes for these compositors.
    Supported,
    /// It does not (GNOME, a sandbox, a session without Wayland): no
    /// layer surface can exist here, so none is ever created.
    UnsupportedHere,
}

/// Decide layer-shell support from the registry's advertised global
/// names. Pure — no I/O — so the decision is unit-testable headless;
/// each frontend's probe feeds it the real registry's names.
pub fn verdict(globals: &[&str]) -> Verdict {
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
pub fn unsupported_startup_error(reason: Option<&str>) -> String {
    let mut msg = format!(
        "dropterm: this session offers no {LAYER_SHELL_GLOBAL} \
         (the Wayland layer-shell protocol a drop-down terminal's surface \
         needs), so DropTerm cannot start here; see {UNSUPPORTED_DOCS_URL}"
    );
    if let Some(reason) = reason {
        // write! into a String cannot fail; clippy::format_push_string
        // prefers it over push_str(&format!(..)).
        let _ = std::fmt::Write::write_fmt(&mut msg, format_args!(" (probe: {reason})"));
    }
    msg
}

/// libcosmic's debug escape hatch, kept (control-transport) and moved
/// here at gpui-scaffold Task 4 — both frontends' launch decisions
/// need it: `COSMIC_SINGLE_INSTANCE=false` (or `0`) runs this launch
/// as a plain instance — no forwarding, no name ownership, no applet
/// state — so a broken primary never blocks debugging a fresh one.
/// Read once per process: the launch decision and the served loop must
/// agree for the whole run.
pub fn single_instance_disabled() -> bool {
    static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("COSMIC_SINGLE_INSTANCE")
            .is_ok_and(|value| value.to_lowercase() == "false" || value == "0")
    })
}

/// The bus half of `--is-visible` (moved from the libcosmic frontend's
/// `main.rs` at gpui-scaffold Task 4): `Some(visible)` when a running
/// instance answered, `None` when the name is unowned (no note owed)
/// or owned without the V1 interface (stale binary mid-upgrade — noted
/// on stderr). Query-only: nothing here activates or spawns the app,
/// so the flag can never summon the terminal.
pub fn query_visibility() -> Option<bool> {
    // zbus needs a runtime with io and time; a current-thread runtime
    // is enough for one synchronous-looking query. Its feature set
    // arrives through zbus's own tokio dependency.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    runtime.block_on(async {
        let conn = zbus::connection::Builder::session().ok()?.build().await.ok()?;
        let owned = zbus::fdo::DBusProxy::new(&conn)
            .await
            .ok()?
            .name_has_owner(crate::core::dbus_state::BUS_NAME.try_into().ok()?)
            .await
            .ok()?;
        if !owned {
            return None;
        }
        let read = async {
            crate::core::dbus_state::V1InterfaceProxy::new(
                &conn,
                crate::core::dbus_state::BUS_NAME,
                crate::core::dbus_state::OBJECT_PATH,
            )
            .await?
            .visible()
            .await
        };
        if let Ok(visible) = read.await {
            Some(visible)
        } else {
            eprintln!(
                "dropterm: bus name owned but the V1 state                  interface is missing (stale binary?); treating as not                  visible"
            );
            None
        }
    })
}

// --- the layer-shell startup verdict (gpui-scaffold Task 3) -----------

#[cfg(test)]
mod layer_shell_tests {
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
    fn duplicate_layer_shell_globals_still_verdict_supported() {
        // A registry that lists the global twice still counts as
        // advertised — `contains` semantics, never a count, so a
        // count-based refactor fails here.
        let globals = [
            "wl_compositor",
            "zwlr_layer_shell_v1",
            "zwlr_layer_shell_v1",
        ];
        assert_eq!(verdict(&globals), Verdict::Supported);
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- the second-launch forward's platform data (control-transport T2) ---

    #[test]
    fn platform_data_carries_both_env_tokens_when_present() {
        // libcosmic's client inserted exactly these two keys; the new
        // forward keeps them so an old-served instance across an
        // upgrade still sees the wire it knows.
        fn str_of<'a>(value: &'a zbus::zvariant::Value<'_>) -> Option<&'a str> {
            match value {
                zbus::zvariant::Value::Str(text) => Some(text.as_str()),
                _ => None,
            }
        }
        let data = platform_data(Some("token-42".to_string()), Some("startup-7".to_string()));
        assert_eq!(
            data.get("activation-token").and_then(str_of),
            Some("token-42")
        );
        assert_eq!(
            data.get("desktop-startup-id").and_then(str_of),
            Some("startup-7")
        );
        assert_eq!(data.len(), 2);
    }

    #[test]
    fn platform_data_is_empty_without_the_env_tokens() {
        assert!(platform_data(None, None).is_empty());
    }

    #[test]
    fn platform_data_carries_each_token_independently() {
        // Two independent arms: one token present must not leak the
        // other's key.
        fn str_of<'a>(value: &'a zbus::zvariant::Value<'_>) -> Option<&'a str> {
            match value {
                zbus::zvariant::Value::Str(text) => Some(text.as_str()),
                _ => None,
            }
        }
        let token_only = platform_data(Some("token-42".to_string()), None);
        assert_eq!(
            token_only.get("activation-token").and_then(str_of),
            Some("token-42")
        );
        assert!(!token_only.contains_key("desktop-startup-id"));

        let startup_only = platform_data(None, Some("startup-7".to_string()));
        assert!(!startup_only.contains_key("activation-token"));
        assert_eq!(
            startup_only.get("desktop-startup-id").and_then(str_of),
            Some("startup-7")
        );
    }

    #[test]
    fn a_delivered_forward_exits_zero_and_a_dead_owner_refuses() {
        // The pinned delta from libcosmic: Ok is the 0-exit delivery;
        // any error against the owned name refuses (exit 1), never a
        // duplicate instance.
        assert!(matches!(
            forward_outcome(&Ok(())),
            ForwardOutcome::Delivered
        ));
        assert!(matches!(
            forward_outcome(&Err(zbus::Error::Handshake("stale".into()))),
            ForwardOutcome::Refused
        ));
    }
}
