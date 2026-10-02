// SPDX-License-Identifier: GPL-3.0-only
// Wired into the app by the dbus-state P2/P3 tasks; until then the
// module compiles inertly (spec: foundation ships before wiring) and
// its items are test-only from the bin target's point of view. Allow
// rather than expect: `expect` goes unfulfilled in the cfg(test)
// compilation, where the tests do use them. Drop this when P2/P3 wire
// the module in.
#![allow(dead_code)]

//! Queryable DropTerm state over D-Bus (dbus-state spec).
//!
//! Serves the read-only [`Visible`] and [`TabCount`] properties on the
//! `io.github.l1br3_io.CosmicExtDropterm.V1` interface, at the bus name
//! and object path the app already owns through libcosmic's
//! single-instance activation. The module deliberately keeps the app's
//! types out: it must stay liftable into the lib target the panel
//! applet later consumes, so the app converts its own state into the
//! types defined here.

use std::sync::{Arc, Mutex};

use zbus::{interface, proxy};

/// Bus name the running instance owns (libcosmic single-instance).
pub(crate) const BUS_NAME: &str = "io.github.l1br3_io.CosmicExtDropterm";

/// Object path libcosmic serves `org.freedesktop.DbusActivation` at; the
/// V1 interface rides the same path.
pub(crate) const OBJECT_PATH: &str = "/io/github/l1br3_io/CosmicExtDropterm";

/// The V1 state interface name.
pub(crate) const INTERFACE: &str = "io.github.l1br3_io.CosmicExtDropterm.V1";

/// State snapshot served on the V1 interface. Derived, never
/// authoritative: `DropTerm`'s own fields remain the single source of
/// truth; the app updates the shared copy and signals the change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) visible: bool,
    pub(crate) tab_count: u32,
}

/// The surface lifecycle phases `Visible` derives from — the app's
/// surface state machine without its `window::Id` payloads, so this
/// module needs no app types. The app's conversion match is
/// exhaustiveness-checked: a new surface state cannot silently map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfacePhase {
    Hidden,
    Measuring,
    Shown,
    Hiding,
}

/// `Visible` is `true` while the surface is on screen or animating in,
/// `false` once it is gone or animating out — exactly one flip per show
/// and per hide.
pub(crate) fn surface_visible(phase: SurfacePhase) -> bool {
    matches!(phase, SurfacePhase::Measuring | SurfacePhase::Shown)
}

/// Server side of the V1 interface. Property getters read through the
/// shared [`State`]; the app mutates that state out-of-band and calls
/// the generated `*_changed` helpers to emit `PropertiesChanged`.
pub(crate) struct V1 {
    state: Arc<Mutex<State>>,
}

impl V1 {
    pub(crate) fn new(state: Arc<Mutex<State>>) -> Self {
        Self { state }
    }

    /// Current snapshot. A poisoned lock means some other thread
    /// panicked mid-update; the guarded state is still structurally
    /// sound to read, so recover it rather than take the interface
    /// down.
    fn snapshot(&self) -> State {
        *self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[interface(name = "io.github.l1br3_io.CosmicExtDropterm.V1")]
impl V1 {
    /// Read-only: `true` while the drop-down surface is shown or
    /// animating in.
    #[zbus(property)]
    fn visible(&self) -> bool {
        self.snapshot().visible
    }

    /// Read-only: number of open tabs.
    #[zbus(property)]
    fn tab_count(&self) -> u32 {
        self.snapshot().tab_count
    }
}

/// Client side of the V1 interface, generated from the same contract.
/// The CLI's `--is-visible` query and the panel applet both go through
/// this proxy.
#[proxy(
    interface = "io.github.l1br3_io.CosmicExtDropterm.V1",
    default_service = "io.github.l1br3_io.CosmicExtDropterm",
    default_path = "/io/github/l1br3_io/CosmicExtDropterm"
)]
pub(crate) trait V1Interface {
    #[zbus(property)]
    fn visible(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn tab_count(&self) -> zbus::Result<u32>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::iced::futures::{StreamExt, join};
    use zbus::connection;
    use zbus::proxy::Defaults;

    #[test]
    fn surface_visible_is_true_while_measuring_or_shown() {
        assert!(surface_visible(SurfacePhase::Measuring));
        assert!(surface_visible(SurfacePhase::Shown));
    }

    #[test]
    fn surface_visible_is_false_while_hidden_or_hiding() {
        assert!(!surface_visible(SurfacePhase::Hidden));
        assert!(!surface_visible(SurfacePhase::Hiding));
    }

    #[test]
    fn proxy_defaults_match_the_served_names() {
        assert_eq!(V1InterfaceProxy::INTERFACE.as_deref(), Some(INTERFACE));
        assert_eq!(V1InterfaceProxy::PATH.as_deref(), Some(OBJECT_PATH));
        assert_eq!(V1InterfaceProxy::DESTINATION.as_deref(), Some(BUS_NAME));
    }

    /// Headless test harness: serves [`V1`] over a peer-to-peer unix
    /// socket so the wire tests need no session bus and no display.
    /// Mirrors the p2p setup of zbus's own e2e suite.
    /// Headless test harness: serves [`V1`] over a peer-to-peer unix
    /// socket so the wire tests need no session bus and no display.
    /// Mirrors the p2p setup of zbus's own e2e suite: both ends of the
    /// handshake must be driven concurrently or the SASL exchange
    /// deadlocks.
    async fn p2p_pair(state: Arc<Mutex<State>>) -> (zbus::Connection, zbus::Connection) {
        let guid: zbus::Guid = "664a0d6b8db84a7e9e41e46a2d2817ba"
            .parse()
            .expect("valid guid");
        let (server_stream, client_stream) =
            tokio::net::UnixStream::pair().expect("unix socket pair");

        let (server, client) = join!(
            connection::Builder::unix_stream(server_stream)
                .server(guid)
                .expect("server guid")
                .p2p()
                .serve_at(OBJECT_PATH, V1::new(state))
                .expect("serve V1")
                .build(),
            connection::Builder::unix_stream(client_stream)
                .p2p()
                .build()
        );

        (
            server.expect("server connection"),
            client.expect("client connection"),
        )
    }

    /// zbus's p2p support needs the tokio feature set (rt, net, time)
    /// that arrives here through zbus itself, not through our own tokio
    /// declaration (sync, time) — build the runtime explicitly instead
    /// of `#[tokio::test]`, which would need the macros feature.
    fn current_thread_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
    }

    /// Bounded so a broken wire path fails the test instead of hanging
    /// it: a property getter that panics, for example, never answers
    /// the client's method call.
    const WIRE_TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

    #[test]
    fn proxy_reads_served_state_over_p2p() {
        current_thread_runtime().block_on(async {
            tokio::time::timeout(WIRE_TEST_TIMEOUT, async {
                let state = Arc::new(Mutex::new(State {
                    visible: false,
                    tab_count: 3,
                }));
                let (_server, client) = p2p_pair(state).await;

                let proxy = V1InterfaceProxy::new(&client).await.expect("proxy");
                assert_eq!(proxy.visible().await, Ok(false));
                assert_eq!(proxy.tab_count().await, Ok(3));
            })
            .await
            .expect("wire test timed out")
        })
    }

    #[test]
    fn changed_helpers_emit_properties_changed_with_new_values() {
        current_thread_runtime().block_on(async {
            tokio::time::timeout(WIRE_TEST_TIMEOUT, async {
                let state = Arc::new(Mutex::new(State {
                    visible: false,
                    tab_count: 1,
                }));
                let (server, client) = p2p_pair(state.clone()).await;

                // Uncached: every read below is a fresh Get on the wire,
                // so assertions reflect the served state, not a cache.
                let proxy = V1InterfaceProxy::builder(&client)
                    .cache_properties(zbus::proxy::CacheProperties::No)
                    .build()
                    .await
                    .expect("proxy");
                assert_eq!(proxy.visible().await, Ok(false));
                assert_eq!(proxy.tab_count().await, Ok(1));

                // Signals are taken from the connection's raw message
                // stream, consumed from a spawned task: a p2p peer
                // stamps no sender, so a proxy's destination-derived
                // sender filter would never match here, and zbus's own
                // e2e suite likewise drives its client from a spawned
                // task. The V1 identity is asserted from each signal's
                // interface-name body argument, which is what a real
                // bus consumer matches on.
                let (signal_tx, mut signals) = tokio::sync::mpsc::channel(2);
                let wire = client.clone();
                tokio::spawn(async move {
                    let mut stream = zbus::MessageStream::from(wire);
                    while let Some(msg) = stream.next().await {
                        let Ok(msg) = msg else {
                            return;
                        };
                        let header = msg.header();
                        if header.message_type() != zbus::message::Type::Signal
                            || header.interface().map(|i| i.to_string())
                                != Some("org.freedesktop.DBus.Properties".to_string())
                            || header.member().map(|m| m.to_string())
                                != Some("PropertiesChanged".to_string())
                        {
                            continue;
                        }
                        let parsed = msg.body().deserialize::<(
                            String,
                            std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
                            Vec<String>,
                        )>();
                        let forwarded = match parsed {
                            Ok((iface_name, changed, _invalidated)) => Ok((iface_name, changed)),
                            Err(err) => Err(err.to_string()),
                        };
                        if signal_tx.send(forwarded).await.is_err() {
                            return;
                        }
                    }
                });

                {
                    let mut state = state.lock().expect("state lock");
                    state.visible = true;
                    state.tab_count = 2;
                }

                let iface_ref = server
                    .object_server()
                    .interface::<_, V1>(OBJECT_PATH)
                    .await
                    .expect("served interface");
                // The changed helpers only need &self, and the guard is
                // dropped before the final reads below: the object server
                // must be free to answer them.
                {
                    let iface = iface_ref.get().await;
                    iface
                        .visible_changed(iface_ref.signal_emitter())
                        .await
                        .expect("emit visible changed");
                    iface
                        .tab_count_changed(iface_ref.signal_emitter())
                        .await
                        .expect("emit tab count changed");
                }

                // Both emits arrive as PropertiesChanged on the V1
                // interface, each carrying the getter's new value.
                let mut saw_visible = false;
                let mut saw_tab_count = false;
                while !(saw_visible && saw_tab_count) {
                    let (iface_name, changed) = signals
                        .recv()
                        .await
                        .expect("PropertiesChanged")
                        .expect("deserialized PropertiesChanged body");
                    assert_eq!(iface_name, INTERFACE);
                    for (name, value) in changed {
                        match name.as_str() {
                            "Visible" => {
                                saw_visible = true;
                                assert!(bool::try_from(value).expect("bool value"));
                            }
                            "TabCount" => {
                                saw_tab_count = true;
                                assert_eq!(u32::try_from(value).expect("u32 value"), 2);
                            }
                            other => panic!("unexpected property {other} changed"),
                        }
                    }
                }

                // The served state itself answers fresh reads with the
                // new values.
                assert_eq!(proxy.visible().await, Ok(true));
                assert_eq!(proxy.tab_count().await, Ok(2));
            })
            .await
            .expect("wire test timed out")
        })
    }
}
