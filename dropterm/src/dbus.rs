// SPDX-License-Identifier: GPL-3.0-only

//! The control bus beside GPUI's render loop (gpui-scaffold Task 4,
//! grown by gpui-surface): the V1 state interface and the activation
//! interface served on a zbus connection held for the process
//! lifetime — the spike's check-10 coexistence shape, grown onto
//! core's real contract — while the forwarded verbs a second launch
//! sends now queue for the render loop's surface machine: the drainer
//! forwards, the machine decides, and the served state mirrors the
//! machine's published phase (the truth, never a local guess).
//!
//! Coexistence is the design: zbus's tasks ride the tokio runtime this
//! module builds on its own thread, GPUI's executor runs the render
//! loop beside it (the spike's proof, and the dropped-`_conn` lesson
//! is why the connection is bound across the serve loop and verified
//! with a `NameHasOwner` self-check row).

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dropterm_core::dbus_state::{self, Forwarded, SurfacePhase};

use crate::cli::{self, Action};

/// The action a forwarded activation carries: `Activate` is the bare
/// summon (the app's default, Toggle); `ActivateAction` carries the
/// Display name plus any words, reconstructed through the CLI's own
/// clap definitions (the libcosmic receiver's `parse_forwarded_action`
/// shape, so either frontend's sender decodes here) — a malformed
/// payload degrades to the plain Toggle, never an error surface.
/// `Open` is served for contract completeness and ignored.
fn decode(forwarded: &Forwarded) -> Option<Action> {
    match forwarded {
        Forwarded::Activate => Some(Action::Toggle),
        Forwarded::ActivateAction { action, args } => {
            let mut words = vec![action.clone()];
            words.extend(args.iter().cloned());
            cli::parse_forwarded_action(&words).map_or_else(
                || {
                    eprintln!(
                        "dropterm: malformed forwarded action {action:?} {args:?}; \
                         answering with the plain toggle"
                    );
                    Some(Action::Toggle)
                },
                Some,
            )
        }
        Forwarded::Open { .. } => None,
    }
}

/// What the served loop shares with the render loop: the machine
/// phase and the tab count the shell publishes (the served state's
/// one truth, mirrored under the same write) and the action queue the
/// shell drains. The shell boots hidden, so the served state does
/// too.
pub(crate) struct Served {
    pub(crate) phase: Arc<Mutex<SurfacePhase>>,
    /// The fleet's size, written by the shell at every change and
    /// mirrored into the served `TabCount`.
    pub(crate) tabs: Arc<Mutex<u32>>,
    pub(crate) verbs: Receiver<Action>,
}

impl Served {
    fn new() -> (Self, Arc<Mutex<dbus_state::State>>, Sender<Action>) {
        let phase = Arc::new(Mutex::new(SurfacePhase::Hidden));
        let state = Arc::new(Mutex::new(dbus_state::derive(
            SurfacePhase::Hidden,
            0, // no tabs until the fleet boots
        )));
        let (verbs_tx, verbs) = mpsc::channel();
        (
            Self {
                phase: Arc::clone(&phase),
                tabs: Arc::new(Mutex::new(0)),
                verbs,
            },
            state,
            verbs_tx,
        )
    }
}

/// Serve the contract on a thread of this process's own, and hold the
/// connection for the process lifetime. Evidence rows are
/// `dropterm:` prefixed like the shell's: the service-up line
/// carries the unique name and the `NameHasOwner` self-check, the
/// drainer logs every action it answers while the render loop renders.
pub(crate) fn spawn_service() -> Served {
    let (served, state, verbs_tx) = Served::new();
    let phase = Arc::clone(&served.phase);
    let tabs = Arc::clone(&served.tabs);
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(err) => return eprintln!("dropterm: tokio rt failed: {err:?}"),
        };
        runtime.block_on(serve(state, verbs_tx, phase, tabs));
    });
    served
}

/// Reads the published machine phase (the shell writes it; a poisoned
/// lock reads as the last resting state rather than taking the bus
/// down).
fn visible_phase(phase: &Mutex<SurfacePhase>) -> SurfacePhase {
    phase.lock().map_or(SurfacePhase::Hidden, |phase| *phase)
}

/// The serve loop: both interfaces at the one object path, the name
/// taken without queueing, then the forwarded-verb drain — every step
/// the libcosmic app's control-bus subscription runs, on this
/// frontend's own runtime.
async fn serve(
    state: Arc<Mutex<dbus_state::State>>,
    verbs: Sender<Action>,
    phase: Arc<Mutex<SurfacePhase>>,
    tabs: Arc<Mutex<u32>>,
) {
    let (tx, mut rx) = tokio::sync::mpsc::channel(dbus_state::FORWARD_CAPACITY);
    let builder = match zbus::connection::Builder::session()
        .and_then(|builder| {
            builder.serve_at(dbus_state::OBJECT_PATH, dbus_state::Activation::new(tx))
        })
        .and_then(|builder| {
            builder.serve_at(
                dbus_state::OBJECT_PATH,
                dbus_state::V1::new(Arc::clone(&state)),
            )
        }) {
        Ok(builder) => builder,
        Err(err) => return eprintln!("dropterm: dbus builder failed: {err}"),
    };
    // The connection must be bound to a variable that outlives this
    // match — binding it inside an arm (the spike's dropped-`_conn`
    // bug) releases the bus name microseconds after "service up"
    // prints.
    let conn = match builder.build().await {
        Ok(conn) => conn,
        Err(err) => return eprintln!("dropterm: dbus connection failed: {err}"),
    };
    match conn
        .request_name_with_flags(
            dbus_state::BUS_NAME,
            zbus::fdo::RequestNameFlags::DoNotQueue.into(),
        )
        .await
    {
        Ok(zbus::fdo::RequestNameReply::PrimaryOwner) => {}
        // Exists, in any wording: a concurrent first launch took the
        // name; this instance's serving is unreachable, and the loop
        // parks quietly (the loser-stays-quiet rule; the launch
        // decision in main should have forwarded instead of getting
        // here — this is the race backstop).
        Ok(reply) => {
            eprintln!(
                "dropterm: another instance owns the bus name ({reply}); control bus off this run"
            );
            std::future::pending::<()>().await;
        }
        Err(err) => {
            eprintln!("dropterm: failed to own the bus name: {err}");
            std::process::exit(1);
        }
    }
    // Self-verification: ask the bus whether the well-known name is
    // really owned, so a dropped-connection regression shows up as a
    // missing/false row instead of a silent no-name.
    let owned = match zbus::fdo::DBusProxy::new(&conn).await {
        Ok(dbus) => match zbus::names::BusName::try_from(dbus_state::BUS_NAME) {
            Ok(name) => dbus.name_has_owner(name).await.unwrap_or(false),
            Err(_) => false,
        },
        Err(_) => false,
    };
    let unique = conn
        .unique_name()
        .map(ToString::to_string)
        .unwrap_or_default();
    println!(
        "dropterm: dbus service up: {} (unique {unique}, name_has_owner={owned})",
        dbus_state::BUS_NAME
    );
    // The served state mirrors the machine's published phase and the
    // fleet's published size: the action queue carries the decisions
    // to the render loop, and this poller carries the answers back —
    // `Visible` flips when the machine's phase actually flipped,
    // `TabCount` when the fleet's size actually changed, never on a
    // local guess. It rides its own task on this runtime, beside the
    // drainer below.
    tokio::spawn(mirror(conn, state, phase, tabs));
    while let Some(forwarded) = rx.recv().await {
        let Some(action) = decode(&forwarded) else {
            eprintln!("dropterm: forwarded Open ignored: {forwarded:?}");
            continue;
        };
        println!("dropterm: dbus action {action} forwarded to the surface loop");
        // The queue is unbounded and this send never blocks; a closed
        // receiver means the render loop is gone and the forward is
        // moot either way.
        let _ = verbs.send(action);
    }
}

/// The served-state mirror's poll loop (py-5 grew it the `TabCount`
/// half): the shell's published phase and fleet size derive the
/// served snapshot, a real flip writes it and emits the flipped
/// property's changed signal — never a local guess.
async fn mirror(
    conn: zbus::Connection,
    state: Arc<Mutex<dbus_state::State>>,
    phase: Arc<Mutex<SurfacePhase>>,
    tabs: Arc<Mutex<u32>>,
) {
    // The first snapshot mirrors whatever phase the shell has
    // published — hidden at boot — before any comparison runs.
    let mut mirrored = dbus_state::derive(visible_phase(&phase), 0);
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        // Scoped: the guard never crosses an await (the future must
        // stay Send).
        let tab_count = {
            let guard = tabs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *guard
        };
        let snapshot = dbus_state::derive(visible_phase(&phase), tab_count);
        let diff = dbus_state::changed(mirrored, snapshot);
        if !diff.visible && !diff.tab_count {
            continue;
        }
        *state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot;
        mirrored = snapshot;
        if diff.visible {
            println!(
                "dropterm: surface phase mirrored -> visible={}",
                snapshot.visible
            );
        }
        // The changed-helper emissions mirror the app's
        // sync_dbus_state: only a real flip emits.
        if let Ok(iface_ref) = conn
            .object_server()
            .interface::<_, dbus_state::V1>(dbus_state::OBJECT_PATH)
            .await
        {
            let iface = iface_ref.get().await;
            if diff.visible {
                let _ = iface
                    .visible_changed(iface_ref.signal_emitter())
                    .await
                    .inspect_err(|err| {
                        eprintln!("dropterm: Visible change emit failed: {err}");
                    });
            }
            if diff.tab_count {
                let _ = iface
                    .tab_count_changed(iface_ref.signal_emitter())
                    .await
                    .inspect_err(|err| {
                        eprintln!("dropterm: TabCount change emit failed: {err}");
                    });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dropterm_core::dbus_state::Forwarded;

    #[test]
    fn the_bare_summon_forwards_as_toggle() {
        // `Activate` is a summon with no verb; the app's default is
        // Toggle (the libcosmic receiver's rule, kept).
        assert_eq!(decode(&Forwarded::Activate), Some(Action::Toggle));
    }

    #[test]
    fn the_surface_verbs_decode_by_their_display_names() {
        // Exactly the words both CLIs forward — the cross-frontend
        // wire.
        assert_eq!(
            decode(&Forwarded::ActivateAction {
                action: "toggle".into(),
                args: vec![]
            }),
            Some(Action::Toggle)
        );
        assert_eq!(
            decode(&Forwarded::ActivateAction {
                action: "show".into(),
                args: vec![]
            }),
            Some(Action::Show)
        );
        assert_eq!(
            decode(&Forwarded::ActivateAction {
                action: "hide".into(),
                args: vec![]
            }),
            Some(Action::Hide)
        );
    }

    #[test]
    fn a_tab_verb_decodes_with_its_payload_words() {
        // The tab verbs ride the same reconstruction the CLI's own
        // round-trip pins — Display name plus the payload words the
        // forwarding launch sent.
        assert_eq!(
            decode(&Forwarded::ActivateAction {
                action: "new-tab".into(),
                args: vec!["--cwd".into(), "/tmp".into()]
            }),
            Some(Action::NewTab {
                cwd: Some("/tmp".into()),
                cmd: None,
                profile: None
            })
        );
        assert_eq!(
            decode(&Forwarded::ActivateAction {
                action: "switch-profile-tab".into(),
                args: vec![
                    "--profile".into(),
                    "web-prod".into(),
                    "--tab-index".into(),
                    "2".into()
                ]
            }),
            Some(Action::SwitchProfileTab {
                profile: "web-prod".to_string(),
                tab_index: Some(2)
            })
        );
    }

    #[test]
    fn a_malformed_payload_degrades_to_the_plain_toggle() {
        // The libcosmic receiver's rule (a bad --tab-index): never an
        // error surface — the plain Toggle answers.
        assert_eq!(
            decode(&Forwarded::ActivateAction {
                action: "close-tab".into(),
                args: vec!["--tab-index".into(), "0".into()]
            }),
            Some(Action::Toggle)
        );
    }

    #[test]
    fn open_stays_ignored() {
        // `Open` is served for contract completeness and ignored.
        assert_eq!(decode(&Forwarded::Open { uris: vec![] }), None);
    }
}
