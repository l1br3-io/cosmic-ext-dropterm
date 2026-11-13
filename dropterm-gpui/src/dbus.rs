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

/// A decoded surface verb — the vocabulary the scaffold serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verb {
    Toggle,
    Show,
    Hide,
}

/// The verb a forwarded action carries: `Activate` is the bare summon
/// (the app's default, Toggle); `ActivateAction` decodes by the
/// action's name — exactly the Display names both CLIs forward, with
/// no words for the surface verbs. Everything else (the tab verbs the
/// terminal slices will own, `Open`) decodes to `None` so the drainer
/// warns instead of guessing.
pub(crate) fn verb_of(forwarded: &Forwarded) -> Option<Verb> {
    match forwarded {
        Forwarded::Activate => Some(Verb::Toggle),
        Forwarded::ActivateAction { action, .. } => match action.as_str() {
            "toggle" => Some(Verb::Toggle),
            "show" => Some(Verb::Show),
            "hide" => Some(Verb::Hide),
            _ => None,
        },
        Forwarded::Open { .. } => None,
    }
}

/// What the served loop shares with the render loop: the machine
/// phase the shell publishes (the served state's one truth, mirrored
/// under the same write) and the verb queue the shell drains. The
/// shell boots hidden, so the served state does too.
pub(crate) struct Served {
    pub(crate) phase: Arc<Mutex<SurfacePhase>>,
    pub(crate) verbs: Receiver<Verb>,
}

impl Served {
    fn new() -> (Self, Arc<Mutex<dbus_state::State>>, Sender<Verb>) {
        let phase = Arc::new(Mutex::new(SurfacePhase::Hidden));
        let state = Arc::new(Mutex::new(dbus_state::derive(
            SurfacePhase::Hidden,
            0, // no tabs until the terminal slices exist
        )));
        let (verbs_tx, verbs) = mpsc::channel();
        (
            Self {
                phase: Arc::clone(&phase),
                verbs,
            },
            state,
            verbs_tx,
        )
    }
}

/// Serve the contract on a thread of this process's own, and hold the
/// connection for the process lifetime. Evidence rows are
/// `dropterm-gpui:` prefixed like the shell's: the service-up line
/// carries the unique name and the `NameHasOwner` self-check, the
/// drainer logs every verb it answers while the render loop renders.
pub(crate) fn spawn_service() -> Served {
    let (served, state, verbs_tx) = Served::new();
    let phase = Arc::clone(&served.phase);
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(err) => return eprintln!("dropterm-gpui: tokio rt failed: {err:?}"),
        };
        runtime.block_on(serve(state, verbs_tx, phase));
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
    verbs: Sender<Verb>,
    phase: Arc<Mutex<SurfacePhase>>,
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
        Err(err) => return eprintln!("dropterm-gpui: dbus builder failed: {err}"),
    };
    // The connection must be bound to a variable that outlives this
    // match — binding it inside an arm (the spike's dropped-`_conn`
    // bug) releases the bus name microseconds after "service up"
    // prints.
    let conn = match builder.build().await {
        Ok(conn) => conn,
        Err(err) => return eprintln!("dropterm-gpui: dbus connection failed: {err}"),
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
                "dropterm-gpui: another instance owns the bus name ({reply}); control bus off this run"
            );
            std::future::pending::<()>().await;
        }
        Err(err) => {
            eprintln!("dropterm-gpui: failed to own the bus name: {err}");
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
        "dropterm-gpui: dbus service up: {} (unique {unique}, name_has_owner={owned})",
        dbus_state::BUS_NAME
    );
    // The served state mirrors the machine's published phase: the
    // verb queue carries the decisions to the render loop, and this
    // poller carries the answers back — `Visible` flips when the
    // machine's phase actually flipped, never on a local guess. It
    // rides its own task on this runtime, beside the drainer below.
    let mirror_conn = conn.clone();
    let mirror_state = Arc::clone(&state);
    let mirror_phase = Arc::clone(&phase);
    tokio::spawn(async move {
        // The first snapshot mirrors whatever phase the shell has
        // published — hidden at boot — before any comparison runs.
        let mut mirrored = dbus_state::derive(visible_phase(&mirror_phase), 0);
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let snapshot = dbus_state::derive(visible_phase(&mirror_phase), mirrored.tab_count);
            if !dbus_state::changed(mirrored, snapshot).visible {
                continue;
            }
            *mirror_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot;
            mirrored = snapshot;
            println!(
                "dropterm-gpui: surface phase mirrored -> visible={}",
                snapshot.visible
            );
            // The changed-helper emission mirrors the app's
            // sync_dbus_state: only a real flip emits.
            if let Ok(iface_ref) = mirror_conn
                .object_server()
                .interface::<_, dbus_state::V1>(dbus_state::OBJECT_PATH)
                .await
            {
                let iface = iface_ref.get().await;
                let _ = iface
                    .visible_changed(iface_ref.signal_emitter())
                    .await
                    .inspect_err(|err| {
                        eprintln!("dropterm-gpui: Visible change emit failed: {err}");
                    });
            }
        }
    });
    while let Some(forwarded) = rx.recv().await {
        let Some(verb) = verb_of(&forwarded) else {
            eprintln!(
                "dropterm-gpui: forwarded action not served by this slice yet: {forwarded:?}"
            );
            continue;
        };
        println!("dropterm-gpui: dbus verb {verb:?} forwarded to the surface loop");
        // The queue is unbounded and this send never blocks; a closed
        // receiver means the render loop is gone and the forward is
        // moot either way.
        let _ = verbs.send(verb);
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
        assert_eq!(verb_of(&Forwarded::Activate), Some(Verb::Toggle));
    }

    #[test]
    fn the_surface_verbs_decode_by_their_display_names() {
        // Exactly the words the libcosmic CLI forwards (CosmicFlags
        // serializes the verb's Display name with no words for the
        // surface verbs) and this CLI forwards the same way — the
        // cross-frontend wire.
        assert_eq!(
            verb_of(&Forwarded::ActivateAction {
                action: "toggle".into(),
                args: vec![]
            }),
            Some(Verb::Toggle)
        );
        assert_eq!(
            verb_of(&Forwarded::ActivateAction {
                action: "show".into(),
                args: vec![]
            }),
            Some(Verb::Show)
        );
        assert_eq!(
            verb_of(&Forwarded::ActivateAction {
                action: "hide".into(),
                args: vec![]
            }),
            Some(Verb::Hide)
        );
    }

    #[test]
    fn terminal_verbs_and_open_do_not_decode_yet() {
        // The tab verbs belong to the terminal slices; `Open` is
        // served for contract completeness and ignored. Both decode to
        // None so the drainer can warn instead of guessing.
        assert_eq!(
            verb_of(&Forwarded::ActivateAction {
                action: "new-tab".into(),
                args: vec!["--cwd".into(), "/tmp".into()]
            }),
            None
        );
        assert_eq!(verb_of(&Forwarded::Open { uris: vec![] }), None);
    }
}
