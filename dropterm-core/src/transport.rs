// SPDX-License-Identifier: GPL-3.0-only

//! The startup transport's bus halves (shared-core spec, moved from
//! the frontend's `main.rs` beside the contract they serve): the
//! bus-name ownership decision, the second-launch forward through
//! the activation member, the forward's outcome decision, and the
//! platform-data dictionary — both frontends serve the same
//! activation shape, so the halves live where the contract does.

use crate::dbus_state;

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
        proxy.activate_action(action, args, data).await
    })
}

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
