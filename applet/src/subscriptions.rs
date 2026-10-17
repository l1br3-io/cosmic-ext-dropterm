// SPDX-License-Identifier: GPL-3.0-only

//! The applet's D-Bus watches, folded into the pure core's event
//! stream: ownership of the terminal's bus name plus its V1
//! properties. Signal-driven end to end — no polling, no reliance on
//! panel respawn.

use cosmic::iced::Subscription;
use cosmic::iced::futures::channel::mpsc;
use cosmic::iced::futures::future;
use cosmic::iced::futures::{SinkExt, StreamExt, stream};
use cosmic_ext_dropterm::dbus_state;
use std::any::TypeId;
use std::future::Future;

use crate::applet::Event;

/// Stays subscribed but inert when the bus is unreachable — a panel
/// slot that never blanks is worth more than a retry loop.
fn static_forever<T>() -> impl Future<Output = T> {
    future::pending()
}

/// What the watch reports besides core events: the shared session-bus
/// connection, delivered once before anything else so the click path
/// can issue its commands on the same connection.
#[derive(Clone, Debug)]
pub(crate) enum Update {
    Connected(zbus::Connection),
    Event(Event),
}

/// The merged internal feed: name-ownership transitions from the bus
/// daemon, property events from the currently-owned terminal.
enum Feed {
    OwnerAppeared,
    OwnerVanished,
    /// Initial property reads of a newly-owned terminal.
    Seeded {
        visible: bool,
        tab_count: u32,
    },
    Visible(bool),
    Tabs(u32),
}

/// Subscription identity; exists only to be unique.
struct Watch;

pub(crate) fn watch() -> Subscription<Update> {
    Subscription::run_with(TypeId::of::<Watch>(), |_| {
        cosmic::iced::stream::channel(16, |mut output: mpsc::Sender<Update>| async move {
            let conn = match zbus::connection::Builder::session() {
                Ok(builder) => match builder.build().await {
                    Ok(conn) => conn,
                    Err(err) => {
                        log::warn!("session bus unavailable, applet will stay static: {err}");
                        static_forever().await
                    }
                },
                Err(err) => {
                    log::warn!("session bus unavailable, applet will stay static: {err}");
                    static_forever().await
                }
            };
            if output.send(Update::Connected(conn.clone())).await.is_err() {
                return;
            }

            let dbus = match zbus::fdo::DBusProxy::new(&conn).await {
                Ok(dbus) => dbus,
                Err(err) => {
                    log::warn!("bus daemon proxy failed, applet will stay static: {err}");
                    static_forever().await
                }
            };
            // The match rule itself carries the name filter (argument
            // 0 of NameOwnerChanged) — only our bus name's transitions
            // ever reach us.
            let mut owners = match dbus
                .receive_name_owner_changed_with_args(&[(0, dbus_state::BUS_NAME)])
                .await
            {
                Ok(owners) => owners,
                Err(err) => {
                    log::warn!("name watch failed, applet will stay static: {err}");
                    static_forever().await
                }
            };

            let (feed_tx, feed_rx) = mpsc::channel(16);
            // The terminal may already be running when the applet
            // starts — no NameOwnerChanged will fire for it. Checked
            // after the watch is attached, so an ownership flip racing
            // the check is still delivered as a transition.
            let mut epoch = None;
            if dbus
                .name_has_owner(dbus_state::BUS_NAME.try_into().expect("valid bus name"))
                .await
                .unwrap_or(false)
            {
                epoch = Some(spawn_epoch(conn.clone(), feed_tx.clone()));
            }

            let name_feed = owners.by_ref().filter_map(|signal| async move {
                let Ok(args) = signal.args() else {
                    return None;
                };
                if args.new_owner().is_some() {
                    Some(Feed::OwnerAppeared)
                } else if args.old_owner().is_some() {
                    Some(Feed::OwnerVanished)
                } else {
                    None
                }
            });
            let feeds = stream::select(name_feed, feed_rx);
            cosmic::iced::futures::pin_mut!(feeds);

            while let Some(feed) = feeds.next().await {
                match feed {
                    Feed::OwnerAppeared => {
                        // A restart replaces any live epoch; aborting
                        // the old one drops its stale cache and signal
                        // subscriptions with it.
                        if let Some(old) = epoch.take() {
                            old.abort();
                        }
                        epoch = Some(spawn_epoch(conn.clone(), feed_tx.clone()));
                    }
                    Feed::OwnerVanished => {
                        if let Some(old) = epoch.take() {
                            old.abort();
                        }
                        if output.send(Update::Event(Event::Vanished)).await.is_err() {
                            return;
                        }
                    }
                    Feed::Seeded { visible, tab_count } => {
                        // Appeared first: a TabCount reaching the core
                        // before it is Running would be dropped as a
                        // zombie event.
                        if output
                            .send(Update::Event(Event::Appeared { visible }))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        if output
                            .send(Update::Event(Event::TabCount(tab_count)))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Feed::Visible(visible) => {
                        if output
                            .send(Update::Event(Event::Visible(visible)))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Feed::Tabs(tab_count) => {
                        if output
                            .send(Update::Event(Event::TabCount(tab_count)))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
        })
    })
}

/// Watches one incarnation of a running terminal: seeds state from
/// initial property reads, then forwards its property changes. The
/// change streams are attached *before* the initial reads, so a flip
/// racing the reads is either included in them or delivered right
/// after — the core's replay tolerance folds duplicates away. The
/// generated proxy caches `Lazily` (zbus 5.19 default), which is what
/// lets `receive_*_changed` deliver at all.
fn spawn_epoch(conn: zbus::Connection, mut tx: mpsc::Sender<Feed>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let v1 = match dbus_state::V1InterfaceProxy::new(
            &conn,
            dbus_state::BUS_NAME,
            dbus_state::OBJECT_PATH,
        )
        .await
        {
            Ok(v1) => v1,
            Err(err) => {
                log::warn!("V1 proxy failed: {err}");
                return;
            }
        };
        // Generated property streams hand back the stream directly —
        // no match-rule registration to fail.
        let (mut visible_changes, mut tab_changes) = (
            v1.receive_visible_changed().await,
            v1.receive_tab_count_changed().await,
        );
        let (visible, tab_count) = match (v1.visible().await, v1.tab_count().await) {
            (Ok(visible), Ok(tab_count)) => (visible, tab_count),
            errs => {
                log::warn!("V1 initial reads failed: {errs:?}");
                return;
            }
        };
        if tx.send(Feed::Seeded { visible, tab_count }).await.is_err() {
            return;
        }

        let changes = stream::select(
            visible_changes
                .by_ref()
                .filter_map(|change| async move { change.get().await.ok().map(Feed::Visible) }),
            tab_changes
                .by_ref()
                .filter_map(|change| async move { change.get().await.ok().map(Feed::Tabs) }),
        );
        cosmic::iced::futures::pin_mut!(changes);
        while let Some(feed) = changes.next().await {
            if tx.send(feed).await.is_err() {
                return;
            }
        }
    })
}
