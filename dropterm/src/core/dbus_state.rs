// SPDX-License-Identifier: GPL-3.0-only

//! Queryable `DropTerm` state over D-Bus, plus the activation
//! interface a second launch forwards through.
//!
//! Serves the read-only `Visible` and `TabCount` properties on the
//! `io.github.l1br3_io.dropterm.V1` interface, and the
//! `org.freedesktop.DbusActivation` members a second launch calls, at
//! the bus name and object path the app owns itself (the
//! control-transport package: `DropTerm` owns its bus name; libcosmic's
//! `single-instance` feature used to). The module deliberately keeps
//! the app's types out: it lives in `dropterm-core` so the panel applet
//! and every frontend consume the same contract implementation, and the
//! app converts its own state into the types defined here.
//!
//! The applet-facing contract is [`BUS_NAME`], [`OBJECT_PATH`],
//! [`INTERFACE`], [`State`], and the [`V1InterfaceProxy`] client (plus
//! the [`V1`] server, which the applet's wire tests use as a mock).
//! The remaining items are `pub` because the terminal bin and the
//! applet reach them through this crate as well.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use zbus::{interface, proxy};

/// Bus name the running instance owns (libcosmic single-instance) —
/// this build's identity; core's [`crate::core::APP_ID`] is the single
/// source of truth.
pub const BUS_NAME: &str = crate::core::APP_ID;

/// Object path libcosmic serves `org.freedesktop.DbusActivation` at;
/// the V1 interface rides the same path. The app id's dot-to-slash
/// form, exactly as libcosmic derives it (pinned by test).
#[cfg(not(feature = "dev-identity"))]
pub const OBJECT_PATH: &str = "/io/github/l1br3_io/dropterm";
/// The dev build's twin (see [`BUS_NAME`]).
#[cfg(feature = "dev-identity")]
pub const OBJECT_PATH: &str = "/io/github/l1br3_io/dropterm/Dev";

/// The V1 state interface name.
pub const INTERFACE: &str = "io.github.l1br3_io.dropterm.V1";

/// State snapshot served on the V1 interface. Derived, never
/// authoritative: `DropTerm`'s own fields remain the single source of
/// truth; the app updates the shared copy and signals the change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State {
    pub visible: bool,
    pub tab_count: u32,
}

/// The surface lifecycle phases `Visible` derives from — the app's
/// surface state machine without its `window::Id` payloads, so this
/// module needs no app types. The app's conversion match is
/// exhaustiveness-checked: a new surface state cannot silently map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfacePhase {
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

/// Derives the served snapshot from the surface phase and the
/// open-tab count — the app supplies its own fields, keeping this
/// module free of app types.
pub fn derive(phase: SurfacePhase, tab_count: u32) -> State {
    State {
        visible: surface_visible(phase),
        tab_count,
    }
}

/// Which V1 properties differ between two snapshots: only these earn
/// a `PropertiesChanged` emission, so a no-op message emits nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Changed {
    pub visible: bool,
    pub tab_count: bool,
}

pub fn changed(old: State, new: State) -> Changed {
    Changed {
        visible: old.visible != new.visible,
        tab_count: old.tab_count != new.tab_count,
    }
}

/// The decision table `--is-visible` runs, pure so it is testable
/// without a bus: `Some(visible)` when a running instance answered,
/// `None` when the name is unowned or owned without a V1 interface
/// (only the query layer's stderr note distinguishes those two).
/// Returns the stdout line to print and the exit code to exit with.
pub fn is_visible_decision(running: Option<bool>) -> (bool, u16) {
    match running {
        Some(true) => (true, 0),
        Some(false) | None => (false, 1),
    }
}

/// Server side of the V1 interface. Property getters read through the
/// shared [`State`]; the app mutates that state out-of-band and calls
/// the generated `*_changed` helpers to emit `PropertiesChanged`.
pub struct V1 {
    state: Arc<Mutex<State>>,
}

impl V1 {
    pub fn new(state: Arc<Mutex<State>>) -> Self {
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
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[interface(name = "io.github.l1br3_io.dropterm.V1")]
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
/// this proxy. No `default_service`/`default_path`: proxy attributes
/// take literals, which would pin the release identity into dev
/// builds — the generated constructor instead demands the names, and
/// every caller passes [`BUS_NAME`]/[`OBJECT_PATH`].
#[proxy(interface = "io.github.l1br3_io.dropterm.V1")]
pub trait V1Interface {
    #[zbus(property)]
    fn visible(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn tab_count(&self) -> zbus::Result<u32>;
}

/// What a second launch forwarded to the running instance, decoded
/// from an `org.freedesktop.DbusActivation` member call. The member
/// names and signatures are the ones libcosmic's `single-instance`
/// feature served (the contract is *the* API; a new-protocol second
/// launch must reach an old-protocol running instance across an
/// upgrade, and vice versa), pinned by the wire tests below.
///
/// The platform-data dictionary (activation token, startup id) is
/// accepted on the wire and dropped here: `DropTerm`'s surface is a
/// layer surface, not the main window libcosmic's token handling
/// unminimizes, so the token had no effect on this app even when
/// libcosmic carried it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Forwarded {
    /// `Activate` — summon with no verb. The app applies its default
    /// (`Toggle`).
    Activate,
    /// `Open` — open URIs. `DropTerm` defines no openable target, so
    /// the payload is carried raw and the app ignores the message;
    /// the member stays served for contract completeness.
    Open { uris: Vec<String> },
    /// `ActivateAction` — the verb's `Display` name plus its argument
    /// words, re-parsed by the receiving instance through the same
    /// clap definitions the CLI uses.
    ActivateAction { action: String, args: Vec<String> },
}

/// How many forwarded verbs may sit queued between the wire and the
/// app's message loop before the next call is dropped: the forward is
/// fire-and-forget, so a full queue (the app stalled, or a same-user
/// caller flooding) drops with a warn instead of growing unbounded —
/// libcosmic's equivalent channel was bounded at ten and applied
/// backpressure inside the D-Bus handler instead.
pub const FORWARD_CAPACITY: usize = 64;

/// Server side of the activation interface. Every member converts its
/// wire call into a [`Forwarded`] on the channel the launcher handed
/// in; the app's subscription turns each into an application message
/// through the same decode path a CLI verb takes.
pub struct Activation {
    tx: tokio::sync::mpsc::Sender<Forwarded>,
}

impl Activation {
    pub fn new(tx: tokio::sync::mpsc::Sender<Forwarded>) -> Self {
        Self { tx }
    }

    /// The fire-and-forget send every member shares: a full queue
    /// drops the verb with a warn, a dead receiver (the app gone) is
    /// silence — the wire call itself always succeeds, exactly as
    /// libcosmic's members replied.
    fn forward(&self, forwarded: Forwarded) {
        if let Err(tokio::sync::mpsc::error::TrySendError::Full(forwarded)) =
            self.tx.try_send(forwarded)
        {
            log::warn!(
                "dropping a forwarded verb (queue full): {:?}",
                forwarded.action_name()
            );
        }
    }
}

impl Forwarded {
    /// The verb's own name, for the queue-full warning — `Activate`
    /// for the bare summon, the action name for `ActivateAction`,
    /// nothing worth reporting for the ignored `Open`.
    fn action_name(&self) -> &str {
        match self {
            Self::Activate => "Activate",
            Self::ActivateAction { action, .. } => action,
            Self::Open { .. } => "Open",
        }
    }
}

#[interface(name = "org.freedesktop.DbusActivation")]
impl Activation {
    /// Activate the application. The platform-data dictionary is
    /// accepted and dropped (see [`Forwarded`]). Owned arg types —
    /// the wire signature (`a{sv}`) is what the pin test holds, not
    /// the borrow shape.
    fn activate(&mut self, platform_data: HashMap<String, zbus::zvariant::OwnedValue>) {
        // Accepted for the contract's signature, dropped for its
        // semantics (see [`Forwarded`]).
        drop(platform_data);
        self.forward(Forwarded::Activate);
    }

    /// Open the given URIs.
    fn open(
        &mut self,
        uris: Vec<String>,
        platform_data: HashMap<String, zbus::zvariant::OwnedValue>,
    ) {
        drop(platform_data);
        self.forward(Forwarded::Open { uris });
    }

    /// Activate the given action.
    fn activate_action(
        &mut self,
        action_name: String,
        parameter: Vec<String>,
        platform_data: HashMap<String, zbus::zvariant::OwnedValue>,
    ) {
        drop(platform_data);
        self.forward(Forwarded::ActivateAction {
            action: action_name,
            args: parameter,
        });
    }
}

/// Client side of the activation interface, generated from the same
/// contract — the second launch's forward path. Like
/// [`V1InterfaceProxy`], no `default_service`/`default_path`: every
/// caller passes [`BUS_NAME`]/[`OBJECT_PATH`], keeping the release
/// identity out of dev builds.
#[proxy(interface = "org.freedesktop.DbusActivation")]
pub trait ActivationInterface {
    /// Activate the application.
    fn activate(&self, platform_data: HashMap<&str, zbus::zvariant::Value<'_>>)
    -> zbus::Result<()>;

    /// Open the given URIs.
    fn open(
        &self,
        uris: Vec<&str>,
        platform_data: HashMap<&str, zbus::zvariant::Value<'_>>,
    ) -> zbus::Result<()>;

    /// Activate the given action.
    fn activate_action(
        &self,
        action_name: &str,
        parameter: Vec<&str>,
        platform_data: HashMap<&str, zbus::zvariant::Value<'_>>,
    ) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    // The wire tests' stream combinators and `join!` — the toolkit-free
    // twin of the frontend's `cosmic::iced::futures` re-export
    // (dev-dependency, tests only).
    use futures::{StreamExt, join};
    use zbus::connection;

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
    fn derive_maps_phase_and_tab_count_into_a_snapshot() {
        assert_eq!(
            derive(SurfacePhase::Shown, 3),
            State {
                visible: true,
                tab_count: 3
            }
        );
        assert_eq!(
            derive(SurfacePhase::Hiding, 0),
            State {
                visible: false,
                tab_count: 0
            }
        );
    }

    #[test]
    fn changed_reports_nothing_for_identical_snapshots() {
        let state = State {
            visible: true,
            tab_count: 2,
        };
        let diff = changed(state, state);
        assert!(!diff.visible);
        assert!(!diff.tab_count);
    }

    #[test]
    fn changed_reports_each_property_independently() {
        let old = State {
            visible: false,
            tab_count: 1,
        };
        let flip = changed(
            old,
            State {
                visible: true,
                tab_count: 1,
            },
        );
        assert!(flip.visible);
        assert!(!flip.tab_count);
        let tabs = changed(
            old,
            State {
                visible: false,
                tab_count: 4,
            },
        );
        assert!(!tabs.visible);
        assert!(tabs.tab_count);
        let both = changed(
            old,
            State {
                visible: true,
                tab_count: 4,
            },
        );
        assert!(both.visible);
        assert!(both.tab_count);
    }

    #[test]
    fn is_visible_decision_prints_true_and_exits_zero_when_shown() {
        assert_eq!(is_visible_decision(Some(true)), (true, 0));
    }

    #[test]
    fn is_visible_decision_prints_false_and_exits_one_when_hidden() {
        assert_eq!(is_visible_decision(Some(false)), (false, 1));
    }

    #[test]
    fn is_visible_decision_exits_one_when_not_running_or_stale() {
        // Both the unowned-name and owned-but-no-V1 cases resolve to
        // `None` here; only the query layer's stderr note tells them
        // apart.
        assert_eq!(is_visible_decision(None), (false, 1));
    }

    #[test]
    fn bus_name_is_the_app_id() {
        // One identity string behind everything runtime-visible; the
        // bus name is not allowed to drift from it.
        assert_eq!(BUS_NAME, crate::core::APP_ID);
    }

    #[test]
    fn object_path_is_the_app_id_slash_form() {
        // libcosmic derives the activation path from the app id
        // (dots to slashes); the V1 path must stay that exact form in
        // both identity states.
        assert_eq!(
            OBJECT_PATH,
            format!("/{}", crate::core::APP_ID.replace('.', "/"))
        );
    }

    #[cfg(not(feature = "dev-identity"))]
    #[test]
    fn release_state_carries_the_plain_identity() {
        assert_eq!(crate::core::APP_ID, "io.github.l1br3_io.dropterm");
    }

    #[cfg(feature = "dev-identity")]
    #[test]
    fn dev_state_carries_the_dev_identity() {
        assert_eq!(crate::core::APP_ID, "io.github.l1br3_io.dropterm.Dev");
    }

    /// Headless test harness: serves [`V1`] and [`Activation`] over a
    /// peer-to-peer unix socket — the control bus's real serving
    /// shape, both interfaces at the one object path — so the wire
    /// tests need no session bus and no display. Mirrors the p2p
    /// setup of zbus's own e2e suite: both ends of the handshake must
    /// be driven concurrently or the SASL exchange deadlocks.
    async fn serve_pair(
        state: Arc<Mutex<State>>,
        activation: Activation,
    ) -> (zbus::Connection, zbus::Connection) {
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
                .serve_at(OBJECT_PATH, activation)
                .expect("serve activation")
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

    /// [`serve_pair`] with a discard-only [`Activation`] — the V1 wire
    /// tests below have no forwarded actions to observe.
    async fn p2p_pair(state: Arc<Mutex<State>>) -> (zbus::Connection, zbus::Connection) {
        let (tx, _rx) = tokio::sync::mpsc::channel(FORWARD_CAPACITY);
        serve_pair(state, Activation::new(tx)).await
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

                let proxy = V1InterfaceProxy::new(&client, BUS_NAME, OBJECT_PATH)
                    .await
                    .expect("proxy");
                assert_eq!(proxy.visible().await, Ok(false));
                assert_eq!(proxy.tab_count().await, Ok(3));
            })
            .await
            .expect("wire test timed out");
        });
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
                    .destination(BUS_NAME)
                    .and_then(|builder| builder.path(OBJECT_PATH))
                    .expect("identity names")
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
                            || header.interface().map(ToString::to_string)
                                != Some("org.freedesktop.DBus.Properties".to_string())
                            || header.member().map(ToString::to_string)
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
            .expect("wire test timed out");
        });
    }

    // --- the activation interface (control-transport) ---

    /// The empty platform-data dictionary every activation call in
    /// these tests carries (the payload-bearing half is exercised by
    /// the pin test's signature assertions, not the forwarding ones).
    fn no_platform_data() -> HashMap<&'static str, zbus::zvariant::Value<'static>> {
        HashMap::new()
    }

    /// Serves [`Activation`] with an observed channel, hands back
    /// both connections (the server one must outlive the calls — a
    /// dropped server closes the socket) and the receiver — the shape
    /// the activation wire tests share.
    async fn activation_pair() -> (
        zbus::Connection,
        zbus::Connection,
        tokio::sync::mpsc::Receiver<Forwarded>,
    ) {
        let state = Arc::new(Mutex::new(State {
            visible: false,
            tab_count: 0,
        }));
        let (tx, rx) = tokio::sync::mpsc::channel(FORWARD_CAPACITY);
        let (server, client) = serve_pair(state, Activation::new(tx)).await;
        (server, client, rx)
    }

    #[test]
    fn activation_forwards_every_member_as_its_message() {
        current_thread_runtime().block_on(async {
            tokio::time::timeout(WIRE_TEST_TIMEOUT, async {
                let (_server, client, mut rx) = activation_pair().await;
                let proxy = ActivationInterfaceProxy::new(&client, BUS_NAME, OBJECT_PATH)
                    .await
                    .expect("proxy");

                // Call order is reply order on one connection, so the
                // messages arrive in the same sequence.
                proxy.activate(no_platform_data()).await.expect("activate");
                assert_eq!(rx.recv().await, Some(Forwarded::Activate));

                proxy
                    .open(
                        vec!["file:///tmp/one", "file:///tmp/two"],
                        no_platform_data(),
                    )
                    .await
                    .expect("open");
                assert_eq!(
                    rx.recv().await,
                    Some(Forwarded::Open {
                        uris: vec!["file:///tmp/one".to_string(), "file:///tmp/two".to_string()]
                    })
                );

                proxy
                    .activate_action("new-tab", vec!["--cwd", "/tmp"], no_platform_data())
                    .await
                    .expect("activate_action");
                assert_eq!(
                    rx.recv().await,
                    Some(Forwarded::ActivateAction {
                        action: "new-tab".to_string(),
                        args: vec!["--cwd".to_string(), "/tmp".to_string()]
                    })
                );
            })
            .await
            .expect("wire test timed out");
        });
    }

    #[test]
    fn a_dead_receiver_never_wedges_the_served_object() {
        // Fire-and-forget's error contract: the receiver can die (the
        // app gone) and every member still replies Ok on the wire —
        // the send's failure is silence, never a broken dispatch.
        current_thread_runtime().block_on(async {
            tokio::time::timeout(WIRE_TEST_TIMEOUT, async {
                let state = Arc::new(Mutex::new(State {
                    visible: false,
                    tab_count: 0,
                }));
                let (tx, rx) = tokio::sync::mpsc::channel(FORWARD_CAPACITY);
                let (_server, client) = serve_pair(state, Activation::new(tx)).await;
                drop(rx);
                let proxy = ActivationInterfaceProxy::new(&client, BUS_NAME, OBJECT_PATH)
                    .await
                    .expect("proxy");
                assert!(proxy.activate(no_platform_data()).await.is_ok());
                assert!(
                    proxy
                        .open(vec!["file:///tmp/x"], no_platform_data())
                        .await
                        .is_ok()
                );
                assert!(
                    proxy
                        .activate_action("toggle", vec![], no_platform_data())
                        .await
                        .is_ok()
                );
            })
            .await
            .expect("wire test timed out");
        });
    }

    #[test]
    fn a_full_queue_drops_the_verb_and_still_replies_ok() {
        // The bounded channel's other half: FORWARD_CAPACITY verbs
        // queued with no drain, then one more — the wire call still
        // succeeds (the drop is the app-side policy, the caller never
        // learns), and the queue holds exactly the capacity.
        current_thread_runtime().block_on(async {
            tokio::time::timeout(WIRE_TEST_TIMEOUT, async {
                let state = Arc::new(Mutex::new(State {
                    visible: false,
                    tab_count: 0,
                }));
                let (tx, mut rx) = tokio::sync::mpsc::channel(FORWARD_CAPACITY);
                let (_server, client) = serve_pair(state, Activation::new(tx)).await;
                let proxy = ActivationInterfaceProxy::new(&client, BUS_NAME, OBJECT_PATH)
                    .await
                    .expect("proxy");
                for _ in 0..FORWARD_CAPACITY {
                    assert!(
                        proxy
                            .activate_action("toggle", vec![], no_platform_data())
                            .await
                            .is_ok()
                    );
                }
                // The queue is full but not yet observed drained; give
                // the object server a beat to land every send, then
                // push one past the capacity — still Ok on the wire.
                tokio::task::yield_now().await;
                assert!(
                    proxy
                        .activate_action("toggle", vec![], no_platform_data())
                        .await
                        .is_ok()
                );
                let mut queued = 0;
                while rx.try_recv().is_ok() {
                    queued += 1;
                }
                assert_eq!(queued, FORWARD_CAPACITY);
            })
            .await
            .expect("wire test timed out");
        });
    }

    #[test]
    fn activation_payloads_carry_the_verbs_a_second_launch_sends() {
        // The exact words the CLI forwards (forward_words' encoding)
        // survive the wire untouched — the receiving instance
        // re-parses exactly what the second launch serialized.
        current_thread_runtime().block_on(async {
            tokio::time::timeout(WIRE_TEST_TIMEOUT, async {
                let (_server, client, mut rx) = activation_pair().await;
                let proxy = ActivationInterfaceProxy::new(&client, BUS_NAME, OBJECT_PATH)
                    .await
                    .expect("proxy");

                proxy
                    .activate_action(
                        "rename-tab",
                        vec!["work in progress", "--tab-index", "2"],
                        no_platform_data(),
                    )
                    .await
                    .expect("activate_action");
                assert_eq!(
                    rx.recv().await,
                    Some(Forwarded::ActivateAction {
                        action: "rename-tab".to_string(),
                        args: vec![
                            "work in progress".to_string(),
                            "--tab-index".to_string(),
                            "2".to_string()
                        ]
                    })
                );
            })
            .await
            .expect("wire test timed out");
        });
    }

    // --- the contract-surface pin (control-transport T3) ---

    /// The `<method>` entries of one interface block: name → the
    /// ordered `(type, direction)` list of its `<arg>`s, enumerated
    /// from the introspection XML rather than string-matched — the
    /// pin holds the served surface, not the XML's spelling of it.
    fn methods(block: &str) -> Vec<(&str, Vec<(&str, &str)>)> {
        block
            .split("<method")
            .skip(1)
            .map(|method| {
                let body = &method[..method.find("</method>").unwrap_or(method.len())];
                let name = attr_of(method, "name");
                let args = body
                    .split("<arg")
                    .skip(1)
                    .map(|arg| (attr_of(arg, "type"), attr_of(arg, "direction")))
                    .collect();
                (name, args)
            })
            .collect()
    }

    /// The `<property>` entries of one interface block:
    /// `(name, type, access)`, enumerated like [`methods`].
    fn properties(block: &str) -> Vec<(&str, &str, &str)> {
        block
            .split("<property")
            .skip(1)
            .map(|property| {
                (
                    attr_of(property, "name"),
                    attr_of(property, "type"),
                    attr_of(property, "access"),
                )
            })
            .collect()
    }

    /// One `key="value"` attribute of an XML fragment (the first).
    fn attr_of<'a>(fragment: &'a str, key: &str) -> &'a str {
        fragment
            .split(&format!("{key}=\""))
            .nth(1)
            .map_or("", |rest| &rest[..rest.find('"').unwrap_or(0)])
    }

    /// The `<interface name="...">...</interface>` blocks of the
    /// introspection XML.
    fn interfaces(xml: &str) -> Vec<(&str, &str)> {
        xml.split("<interface")
            .skip(1)
            .map(|iface| {
                let body = &iface[..iface.find("</interface>").unwrap_or(iface.len())];
                (attr_of(iface, "name"), body)
            })
            .collect()
    }

    #[test]
    fn served_contract_surface_is_pinned() {
        // The contract is *the* API: this pins what the control bus
        // serves at the object path — the V1 interface and the
        // activation member set with exactly the signatures
        // libcosmic's `single-instance` feature served (read from its
        // vendored source at /planning), so the ownership change
        // changed nothing observable, and a new-served instance still
        // answers an old second launch across an upgrade.
        current_thread_runtime().block_on(async {
            tokio::time::timeout(WIRE_TEST_TIMEOUT, async {
                let state = Arc::new(Mutex::new(State {
                    visible: true,
                    tab_count: 2,
                }));
                let (tx, _rx) = tokio::sync::mpsc::channel(FORWARD_CAPACITY);
                let (_server, client) = serve_pair(state, Activation::new(tx)).await;
                let introspectable = zbus::fdo::IntrospectableProxy::builder(&client)
                    .destination(BUS_NAME)
                    .and_then(|builder| builder.path(OBJECT_PATH))
                    .expect("identity names")
                    .build()
                    .await
                    .expect("introspectable proxy");
                let xml = introspectable
                    .introspect()
                    .await
                    .expect("introspect the object path");
                // The served node's own interfaces only: the XML
                // also carries the child-node tree zbus walks below
                // the path, where the D-Bus standards repeat per
                // level.
                let own = &xml[..xml.find("<node name=").unwrap_or(xml.len())];
                let served = interfaces(own);

                // The one object path carries exactly the two
                // DropTerm interfaces beside the three D-Bus
                // standards zbus adds.
                let mut names: Vec<&str> = served.iter().map(|(name, _)| *name).collect();
                names.sort_unstable();
                assert_eq!(
                    names,
                    vec![
                        "io.github.l1br3_io.dropterm.V1",
                        "org.freedesktop.DBus.Introspectable",
                        "org.freedesktop.DBus.Peer",
                        "org.freedesktop.DBus.Properties",
                        "org.freedesktop.DbusActivation",
                    ]
                );

                let v1 = served
                    .iter()
                    .find(|(name, _)| *name == "io.github.l1br3_io.dropterm.V1")
                    .expect("V1 served")
                    .1;
                // V1 is exactly the two read-only properties the
                // applet and --is-visible speak; it has no methods.
                assert!(
                    methods(v1).is_empty(),
                    "V1 serves no methods: {:?}",
                    methods(v1)
                );
                let mut props = properties(v1);
                props.sort_unstable();
                assert_eq!(
                    props,
                    vec![("TabCount", "u", "read"), ("Visible", "b", "read")]
                );

                let activation = served
                    .iter()
                    .find(|(name, _)| *name == "org.freedesktop.DbusActivation")
                    .expect("activation served")
                    .1;
                // libcosmic's member set, signature for signature:
                // Activate(a{sv}), Open(as, a{sv}),
                // ActivateAction(s, as, a{sv}) — all in, no reply
                // payload, no properties.
                let mut served_methods = methods(activation);
                served_methods.sort_unstable();
                assert_eq!(
                    served_methods,
                    vec![
                        ("Activate", vec![("a{sv}", "in")]),
                        (
                            "ActivateAction",
                            vec![("s", "in"), ("as", "in"), ("a{sv}", "in")]
                        ),
                        ("Open", vec![("as", "in"), ("a{sv}", "in")]),
                    ]
                );
                assert!(
                    properties(activation).is_empty(),
                    "activation serves no properties: {:?}",
                    properties(activation)
                );
            })
            .await
            .expect("wire test timed out");
        });
    }

    // --- the docs pin (contract-docs) ---

    #[test]
    fn every_dbus_member_is_documented() {
        // The docs-as-code drift gate for the contract page, the
        // dbus twin of `every_config_key_is_documented`: every member
        // (methods and properties — neither interface serves a
        // signal, and the helpers below would not see one) the
        // served surface exposes on the two DropTerm interfaces must
        // appear backticked in docs/dbus.md. The expectation is
        // enumerated from a live introspection of the served pair —
        // no hand-kept member list to drift — so a member added to
        // either interface fails `just check` until the page carries
        // it. The three D-Bus standards zbus adds are not this
        // contract's members; the page cites their Get and
        // PropertiesChanged calls in prose, which is their place in
        // the docs.
        current_thread_runtime().block_on(async {
            tokio::time::timeout(WIRE_TEST_TIMEOUT, async {
                let state = Arc::new(Mutex::new(State {
                    visible: true,
                    tab_count: 1,
                }));
                let (_server, client) = p2p_pair(state).await;
                let introspectable = zbus::fdo::IntrospectableProxy::builder(&client)
                    .destination(BUS_NAME)
                    .and_then(|builder| builder.path(OBJECT_PATH))
                    .expect("identity names")
                    .build()
                    .await
                    .expect("introspectable proxy");
                let xml = introspectable
                    .introspect()
                    .await
                    .expect("introspect the object path");
                // The served node's own interfaces only — the XML
                // also carries the child-node tree zbus walks below
                // the path (`served_contract_surface_is_pinned` cuts
                // at the same seam).
                let own = &xml[..xml.find("<node name=").unwrap_or(xml.len())];
                let served = interfaces(own);

                // Guard against a vacuous pass: if the filter below
                // matched nothing (a changed XML shape, a typo'd
                // interface literal), the loops would assert over an
                // empty set and stay green. Both contract interfaces
                // must be in the enumeration for the pin to mean
                // anything.
                let names: Vec<&str> = served.iter().map(|(name, _)| *name).collect();
                for expected in [INTERFACE, "org.freedesktop.DbusActivation"] {
                    assert!(
                        names.contains(&expected),
                        "introspection did not serve `{expected}`: the docs pin would pass vacuously"
                    );
                }

                let docs = include_str!("../../../docs/dbus.md");
                for (interface, block) in served {
                    if interface != INTERFACE && interface != "org.freedesktop.DbusActivation" {
                        continue;
                    }
                    for (method, _) in methods(block) {
                        assert!(
                            docs.contains(&format!("`{method}`")),
                            "method `{method}` of `{interface}` is not documented in docs/dbus.md"
                        );
                    }
                    for (property, _, _) in properties(block) {
                        assert!(
                            docs.contains(&format!("`{property}`")),
                            "property `{property}` of `{interface}` is not documented in docs/dbus.md"
                        );
                    }
                }
            })
            .await
            .expect("wire test timed out");
        });
    }
}
