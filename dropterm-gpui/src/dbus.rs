// SPDX-License-Identifier: GPL-3.0-only

//! The control bus beside GPUI's render loop (gpui-scaffold Task 4):
//! the V1 state interface and the activation interface served on a
//! zbus connection held for the process lifetime — the spike's
//! check-10 coexistence shape, grown onto core's real contract — while
//! the forwarded verbs a second launch sends apply to the served
//! visibility live.
//!
//! Coexistence is the design: zbus's tasks ride the tokio runtime this
//! module builds on its own thread, GPUI's executor runs the render
//! loop beside it (the spike's proof, and the dropped-`_conn` lesson
//! is why the connection is bound across the serve loop and verified
//! with a `NameHasOwner` self-check row).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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

/// The visibility a verb applied to the current one lands on.
pub(crate) fn applied(current: bool, verb: Verb) -> bool {
    match verb {
        Verb::Toggle => !current,
        Verb::Show => true,
        Verb::Hide => false,
    }
}

/// What the served loop shares with the render loop: the served
/// visibility, mirrored in the V1 state under the same write.
pub(crate) struct Served {
    pub(crate) visible: Arc<AtomicBool>,
    state: Arc<Mutex<dbus_state::State>>,
}

impl Served {
    fn new() -> Self {
        let state = Arc::new(Mutex::new(dbus_state::derive(
            SurfacePhase::Shown,
            0, // no tabs until the terminal slices exist
        )));
        Self {
            visible: Arc::new(AtomicBool::new(true)),
            state,
        }
    }
}

/// Serve the contract on a thread of this process's own, and hold the
/// connection for the process lifetime. Evidence rows are
/// `dropterm-gpui:` prefixed like the shell's: the service-up line
/// carries the unique name and the `NameHasOwner` self-check, the
/// drainer logs every verb it answers while the render loop renders.
pub(crate) fn spawn_service() -> Served {
    let served = Served::new();
    let visible = Arc::clone(&served.visible);
    let state = Arc::clone(&served.state);
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(err) => return eprintln!("dropterm-gpui: tokio rt failed: {err:?}"),
        };
        runtime.block_on(serve(visible, state));
    });
    served
}

/// The serve loop: both interfaces at the one object path, the name
/// taken without queueing, then the forwarded-verb drain — every step
/// the libcosmic app's control-bus subscription runs, on this
/// frontend's own runtime.
async fn serve(visible: Arc<AtomicBool>, state: Arc<Mutex<dbus_state::State>>) {
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
    while let Some(forwarded) = rx.recv().await {
        apply(forwarded, &visible, &state, &conn).await;
    }
}

/// Apply one forwarded verb to the served visibility and the V1 state,
/// then emit `PropertiesChanged` for `Visible` — the same sync the
/// libcosmic app runs after every message.
async fn apply(
    forwarded: Forwarded,
    visible: &AtomicBool,
    state: &Mutex<dbus_state::State>,
    conn: &zbus::Connection,
) {
    let Some(verb) = verb_of(&forwarded) else {
        eprintln!("dropterm-gpui: forwarded action not served by the scaffold yet: {forwarded:?}");
        return;
    };
    let now_visible = applied(visible.load(Ordering::Relaxed), verb);
    visible.store(now_visible, Ordering::Relaxed);
    let phase = if now_visible {
        SurfacePhase::Shown
    } else {
        SurfacePhase::Hidden
    };
    if let Ok(mut served) = state.lock() {
        *served = dbus_state::derive(phase, served.tab_count);
    }
    println!("dropterm-gpui: dbus verb {verb:?} answered while rendering -> visible={now_visible}");
    // The changed-helper emission mirrors the app's sync_dbus_state: a
    // no-op flip emits nothing, a real one carries the new value.
    if let Ok(iface_ref) = conn
        .object_server()
        .interface::<_, dbus_state::V1>(dbus_state::OBJECT_PATH)
        .await
    {
        let iface = iface_ref.get().await;
        let _ = iface
            .visible_changed(iface_ref.signal_emitter())
            .await
            .inspect_err(|err| eprintln!("dropterm-gpui: Visible change emit failed: {err}"));
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

    #[test]
    fn toggle_flips_and_show_hide_set() {
        assert!(!applied(true, Verb::Toggle));
        assert!(applied(false, Verb::Toggle));
        assert!(applied(false, Verb::Show));
        assert!(applied(true, Verb::Show));
        assert!(!applied(true, Verb::Hide));
        assert!(!applied(false, Verb::Hide));
    }
}
