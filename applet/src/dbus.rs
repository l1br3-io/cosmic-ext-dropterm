// SPDX-License-Identifier: GPL-3.0-only

//! The applet's thin command path to the terminal:
//! `org.freedesktop.DbusActivation.activate_action`, the same
//! interface and argument shape libcosmic's single-instance
//! forwarding already rides. V1 state comes through the root lib's
//! proxy instead (`cosmic_ext_dropterm::dbus_state`).

use std::collections::HashMap;

use zbus::proxy;

/// Client of the activation interface the terminal serves on its
/// owned bus name. The attribute literals duplicate the root lib's
/// `BUS_NAME`/`OBJECT_PATH` because proxy attributes take literals
/// only; a test pins the two together so they cannot drift.
#[proxy(
    interface = "org.freedesktop.DbusActivation",
    default_service = "io.github.l1br3_io.CosmicExtDropterm",
    default_path = "/io/github/l1br3_io/CosmicExtDropterm"
)]
pub(crate) trait DbusActivation {
    /// The argument split libcosmic itself uses: action name, its
    /// argument words, platform data. Signature-identical to the
    /// proxy trait at the pinned libcosmic rev.
    fn activate_action(
        &mut self,
        action_name: &str,
        parameter: Vec<&str>,
        platform_data: HashMap<&str, zbus::zvariant::Value<'_>>,
    ) -> zbus::Result<()>;
}

/// Sends the toggle: byte-identical to what a second CLI launch with
/// the default action produces — action name `toggle`, no argument
/// words, no platform data (the running app's toggle needs no
/// activation token; its layer surface takes keyboard focus itself).
/// Fire-and-forget by design: debouncing is the terminal's job.
pub(crate) async fn send_toggle(conn: &zbus::Connection) -> zbus::Result<()> {
    let mut proxy = DbusActivationProxy::new(conn).await?;
    proxy
        .activate_action("toggle", Vec::new(), HashMap::new())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::applet::{Event, TargetState, tooltip, transition};
    use cosmic::iced::futures::{StreamExt, join};
    use cosmic_ext_dropterm::dbus_state;
    use cosmic_ext_dropterm::dbus_state::V1;
    use std::sync::{Arc, Mutex};
    use zbus::connection;
    use zbus::interface;
    use zbus::proxy::Defaults;

    /// zbus needs the tokio feature set (rt, net, time) that arrives
    /// here through zbus itself, not through our `sync` + `time`
    /// declaration — build the runtime explicitly instead of
    /// `#[tokio::test]`, which would need the macros feature.
    fn current_thread_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
    }

    /// Bounded so a broken wire path fails the test instead of
    /// hanging it.
    const WIRE_TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

    /// One recorded `activate_action` delivery, in the server's own
    /// types (platform data kept as owned values to outlive the
    /// call).
    #[derive(Debug)]
    struct ActivationCall {
        action: String,
        words: Vec<String>,
        platform_data: HashMap<String, zbus::zvariant::OwnedValue>,
    }

    /// Mock of the terminal's activation server: records calls
    /// instead of forwarding them to a real app.
    struct MockActivation {
        calls: Arc<Mutex<Vec<ActivationCall>>>,
    }

    #[interface(name = "org.freedesktop.DbusActivation")]
    impl MockActivation {
        async fn activate_action(
            &mut self,
            action_name: &str,
            parameter: Vec<&str>,
            platform_data: HashMap<&str, zbus::zvariant::Value<'_>>,
        ) {
            let owned = platform_data
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.try_into().expect("owned value")))
                .collect();
            self.calls.lock().expect("calls lock").push(ActivationCall {
                action: action_name.to_string(),
                words: parameter.into_iter().map(str::to_string).collect(),
                platform_data: owned,
            });
        }
    }

    /// Headless p2p harness (mirrors the root lib's dbus_state tests):
    /// both handshake ends must be driven concurrently or the SASL
    /// exchange deadlocks.
    async fn p2p_pair<T>(serve: T) -> (zbus::Connection, zbus::Connection)
    where
        T: zbus::object_server::Interface,
    {
        let guid: zbus::Guid = "7c1b2f04a9d64e0f8b3c5d6e9f0a1b2c"
            .parse()
            .expect("valid guid");
        let (server_stream, client_stream) =
            tokio::net::UnixStream::pair().expect("unix socket pair");

        let (server, client) = join!(
            connection::Builder::unix_stream(server_stream)
                .server(guid)
                .expect("server guid")
                .p2p()
                .serve_at(dbus_state::OBJECT_PATH, serve)
                .expect("serve mock")
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

    /// The generated proxy's defaults must point at exactly the names
    /// the terminal owns — the literals above cannot be consts, so
    /// this pins them to the root lib's single source of truth.
    #[test]
    fn activation_defaults_pin_to_the_served_names() {
        assert_eq!(
            DbusActivationProxy::DESTINATION.as_deref(),
            Some(dbus_state::BUS_NAME)
        );
        assert_eq!(
            DbusActivationProxy::PATH.as_deref(),
            Some(dbus_state::OBJECT_PATH)
        );
    }

    /// The toggle the applet sends must be the message a second CLI
    /// launch produces: interface and member are proven by the mock's
    /// dispatch (the object server only delivers on an exact
    /// path/interface/member match), and the body by the recording.
    #[test]
    fn toggle_send_matches_the_cli_activation_message() {
        current_thread_runtime().block_on(async {
            tokio::time::timeout(WIRE_TEST_TIMEOUT, async {
                let calls = Arc::new(Mutex::new(Vec::new()));
                let (_server, client) = p2p_pair(MockActivation {
                    calls: calls.clone(),
                })
                .await;

                send_toggle(&client).await.expect("toggle sent");

                let calls = calls.lock().expect("calls lock");
                assert_eq!(calls.len(), 1, "exactly one activation");
                assert_eq!(calls[0].action, "toggle");
                assert_eq!(calls[0].words, Vec::<String>::new());
                assert!(calls[0].platform_data.is_empty());
            })
            .await
            .expect("wire test timed out")
        })
    }

    /// Forwards V1 `PropertiesChanged` signals to the test as core
    /// [`Event`]s. The stream is created synchronously here — before
    /// the caller emits anything — so no signal can race the attach;
    /// consumption itself runs from a spawned task because a p2p
    /// peer stamps no sender and a proxy's destination-derived sender
    /// filter would never match. On the real session bus the
    /// generated proxy's cached `receive_*_changed` stream does this
    /// for us (zbus 5.19: generated proxies cache `Lazily`, and
    /// `receive_property_changed` rides that cache's subscription).
    fn spawn_v1_changes(conn: &zbus::Connection, tx: tokio::sync::mpsc::Sender<Event>) {
        let mut stream = zbus::MessageStream::from(conn.clone());
        tokio::spawn(async move {
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
                let Ok((iface_name, changed, _invalidated)) = msg.body().deserialize::<(
                    String,
                    std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
                    Vec<String>,
                )>() else {
                    continue;
                };
                if iface_name != dbus_state::INTERFACE {
                    continue;
                }
                for (name, value) in changed {
                    let event = match name.as_str() {
                        "Visible" => Event::Visible(bool::try_from(value).expect("bool value")),
                        "TabCount" => Event::TabCount(u32::try_from(value).expect("u32 value")),
                        _ => continue,
                    };
                    if tx.send(event).await.is_err() {
                        return;
                    }
                }
            }
        });
    }

    /// Emits changed signals for exactly the properties that differ —
    /// the same dedupe the terminal applies before touching the
    /// wire.
    async fn emit_diff(server: &zbus::Connection, old: dbus_state::State, new: dbus_state::State) {
        let diff = dbus_state::changed(old, new);
        let iface_ref = server
            .object_server()
            .interface::<_, V1>(dbus_state::OBJECT_PATH)
            .await
            .expect("served V1");
        let iface = iface_ref.get().await;
        let emitter = iface_ref.signal_emitter();
        if diff.visible {
            iface.visible_changed(emitter).await.expect("emit Visible");
        }
        if diff.tab_count {
            iface
                .tab_count_changed(emitter)
                .await
                .expect("emit TabCount");
        }
    }

    /// The consumption path, end to end over p2p: initial property
    /// reads through the root lib's proxy seed the core, and the
    /// terminal's `PropertiesChanged` emissions drive it.
    #[test]
    fn v1_reads_seed_state_and_properties_changed_drives_it() {
        current_thread_runtime().block_on(async {
            tokio::time::timeout(WIRE_TEST_TIMEOUT, async {
                let shared = Arc::new(Mutex::new(dbus_state::State {
                    visible: false,
                    tab_count: 3,
                }));
                let (server, client) = p2p_pair(V1::new(shared.clone())).await;

                let proxy = dbus_state::V1InterfaceProxy::new(&client)
                    .await
                    .expect("proxy");
                let visible = proxy.visible().await.expect("Visible read");
                let tab_count = proxy.tab_count().await.expect("TabCount read");
                assert!(!visible);
                assert_eq!(tab_count, 3);

                let mut core = transition(TargetState::NotRunning, Event::Appeared { visible });
                assert_eq!(core, TargetState::Running { visible: false });

                let (tx, mut rx) = tokio::sync::mpsc::channel(4);
                spawn_v1_changes(&client, tx);

                shared.lock().expect("state lock").visible = true;
                emit_diff(
                    &server,
                    dbus_state::State {
                        visible: false,
                        tab_count: 3,
                    },
                    dbus_state::State {
                        visible: true,
                        tab_count: 3,
                    },
                )
                .await;

                let event = rx.recv().await.expect("PropertiesChanged");
                core = transition(core, event);
                assert_eq!(core, TargetState::Running { visible: true });
                assert_eq!(tooltip(&core, 3), "DropTerm — visible, 3 tabs");
            })
            .await
            .expect("wire test timed out")
        })
    }

    /// Restore-style bursts — several rapid emissions, values only
    /// ever moving forward — must leave the core at the final values.
    #[test]
    fn restore_style_bursts_replay_without_corrupting_state() {
        current_thread_runtime().block_on(async {
            tokio::time::timeout(WIRE_TEST_TIMEOUT, async {
                let shared = Arc::new(Mutex::new(dbus_state::State {
                    visible: false,
                    tab_count: 1,
                }));
                let (server, client) = p2p_pair(V1::new(shared.clone())).await;

                let mut core = TargetState::Running { visible: false };
                let mut tab_count = 1;

                let (tx, mut rx) = tokio::sync::mpsc::channel(8);
                spawn_v1_changes(&client, tx);

                // A session restore: the surface shows, tabs stream in
                // one after another, then the surface hides again.
                let mut old = dbus_state::State {
                    visible: false,
                    tab_count: 1,
                };
                let steps = [
                    dbus_state::State {
                        visible: true,
                        tab_count: 1,
                    },
                    dbus_state::State {
                        visible: true,
                        tab_count: 2,
                    },
                    dbus_state::State {
                        visible: true,
                        tab_count: 3,
                    },
                    dbus_state::State {
                        visible: false,
                        tab_count: 3,
                    },
                ];
                for new in steps {
                    *shared.lock().expect("state lock") = new;
                    emit_diff(&server, old, new).await;
                    old = new;
                }

                for _ in 0..4 {
                    match rx.recv().await.expect("PropertiesChanged") {
                        event @ (Event::Visible(_) | Event::TabCount(_)) => {
                            if let Event::TabCount(n) = event {
                                tab_count = n;
                            }
                            core = transition(core, event);
                        }
                        unexpected => panic!("unexpected event: {unexpected:?}"),
                    }
                }
                assert_eq!(core, TargetState::Running { visible: false });
                assert_eq!(tooltip(&core, tab_count), "DropTerm — hidden, 3 tabs");
            })
            .await
            .expect("wire test timed out")
        })
    }
}
