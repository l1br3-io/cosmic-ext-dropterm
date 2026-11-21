// SPDX-License-Identifier: GPL-3.0-only

//! The tray port (the tray module, `SPEC-tray.md`): a
//! `StatusNotifierItem` presence's toolkit-neutral front. The menu
//! description types and the [`TrayBackend`] seam sit in the
//! portability plan's vocabulary beside `SurfaceBackend` and
//! `ControlTransport`; no platform type crosses the trait's surface.
//! The ksni SNI service (Task T3) is the shipping implementation, the
//! Phase 4/5 native trays arrive as further implementations, and the
//! headless [`RecordingTray`] is the second implementation the tests
//! drive (design rule 5's testing clause, the surface-intents
//! precedent). The trait lives `pub(crate)` in the bin — "traits
//! inside the binary, contracts outside it" — and moves to
//! `dropterm-platform` with the other seams when that crate lands.

use std::sync::mpsc::Sender;

/// The tray menu's neutral vocabulary — no ksni types cross this
/// line. The render loop's verb queue is the one sink; the Phase 4/5
/// native trays implement the same enum against `tray-icon`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrayAction {
    /// Same path as `dropterm toggle` (the SNI `Activate` signal and
    /// the menu item both land here).
    Toggle,
    /// Open the About window (plain toplevel, never a layer surface).
    Help,
    /// The clean quit: session persisted, then the event loop ends.
    Quit,
}

/// The menu a backend presents: the item's title, its themed icon
/// name, and the actions in display order — an item IS its action, so
/// no label can drift from what activating it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrayMenu {
    /// The tray item's title and tooltip (the "(Dev)" suffix under
    /// `dev-identity`, so a dev and a release instance coexist in one
    /// tray).
    pub(crate) title: String,
    /// The themed icon name the installed hicolor set carries.
    pub(crate) icon: String,
    /// The menu items in display order.
    pub(crate) items: Vec<TrayAction>,
}

impl TrayMenu {
    /// The menu over `actions`, in display order: `title` names the
    /// tray item, `icon` the themed icon it shows.
    pub(crate) fn new(title: &str, icon: &str, actions: &[TrayAction]) -> Self {
        Self {
            title: title.to_string(),
            icon: icon.to_string(),
            items: actions.to_vec(),
        }
    }
}

/// What presenting the tray came to — the no-host degrade is data on
/// the same return path as registration, never a panic and never a
/// change to anything else (a desktop without a `StatusNotifierWatcher`
/// simply has no icon).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrayPresence {
    /// A host took the item; activations will flow into the sink.
    Registered,
    /// No `StatusNotifierWatcher` answered: no icon, no retry (the v1
    /// single attempt), one `dropterm:` log line at the spawn site.
    NoHost,
}

/// The tray port: present the menu for the process's lifetime.
/// Activations flow into `sink` — the channel is the only bridge, no
/// tray call ever rides the render thread — and the caller learns the
/// degrade outcome as data. Registration is process-lifetime: no
/// shutdown surface in v1 (the tray dies with the process).
pub(crate) trait TrayBackend {
    /// Presents `menu`, forwarding activations into `sink`.
    fn present(&self, menu: TrayMenu, sink: Sender<TrayAction>) -> TrayPresence;
}

/// The headless recording fake (design rule 5's testing clause, the
/// surface-intents recording-executor precedent): the second
/// [`TrayBackend`] implementation, standing in for a real tray host.
/// [`RecordingTray::with_host`] answers as a hosting desktop would;
/// [`RecordingTray::without_host`] degrades to
/// [`TrayPresence::NoHost`] as data. Everything presented is
/// recorded; [`RecordingTray::fire`] plays an activation into the
/// sink exactly as a host would deliver it.
#[cfg(test)]
pub(crate) struct RecordingTray {
    host: bool,
    presented: std::sync::Mutex<Option<TrayMenu>>,
    sink: std::sync::Mutex<Option<Sender<TrayAction>>>,
}

#[cfg(test)]
impl RecordingTray {
    /// A fake whose session carries a tray host: presenting
    /// registers.
    fn with_host() -> Self {
        Self::over(true)
    }

    /// A fake whose session carries no `StatusNotifierWatcher`:
    /// presenting degrades, as data.
    fn without_host() -> Self {
        Self::over(false)
    }

    /// The one constructor both scenarios share.
    fn over(host: bool) -> Self {
        Self {
            host,
            presented: std::sync::Mutex::new(None),
            sink: std::sync::Mutex::new(None),
        }
    }

    /// The menu last presented through the trait, if any.
    fn menu(&self) -> Option<TrayMenu> {
        self.presented
            .lock()
            .map_or(None, |presented| presented.clone())
    }

    /// Plays one activation into the sink, as a host delivers it. A
    /// closed receiver means the render loop is gone and the
    /// activation is moot either way.
    fn fire(&self, action: TrayAction) {
        if let Ok(sink) = self.sink.lock()
            && let Some(sink) = sink.as_ref()
        {
            let _ = sink.send(action);
        }
    }
}

#[cfg(test)]
impl TrayBackend for RecordingTray {
    fn present(&self, menu: TrayMenu, sink: Sender<TrayAction>) -> TrayPresence {
        *self
            .presented
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(menu);
        *self
            .sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sink);
        if self.host {
            TrayPresence::Registered
        } else {
            TrayPresence::NoHost
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn the_menu_builds_the_toggle_help_quit_shape() {
        // The intended menu (the spec's objective): toggle first, help
        // between, quit last — the display order the About window's
        // arrival (T4) completes; T3 ships the same model minus help
        // (no dead menu item).
        let menu = TrayMenu::new(
            "DropTerm",
            "io.github.l1br3_io.dropterm",
            &[TrayAction::Toggle, TrayAction::Help, TrayAction::Quit],
        );
        assert_eq!(menu.title, "DropTerm");
        assert_eq!(menu.icon, "io.github.l1br3_io.dropterm");
        assert_eq!(
            menu.items,
            vec![TrayAction::Toggle, TrayAction::Help, TrayAction::Quit]
        );
    }

    #[test]
    fn activations_reach_the_sink_in_order_through_the_trait() {
        // The trait is the only path: the menu is presented through
        // `present`, the host's activations land in the sink in fire
        // order, and the menu the backend saw is the menu that was
        // presented — no side channel.
        let (tx, rx) = mpsc::channel();
        let tray = RecordingTray::with_host();
        let menu = TrayMenu::new("DropTerm", "icon", &[TrayAction::Toggle]);
        assert_eq!(tray.present(menu.clone(), tx), TrayPresence::Registered);
        assert_eq!(tray.menu(), Some(menu));
        for action in [TrayAction::Toggle, TrayAction::Help, TrayAction::Quit] {
            tray.fire(action);
        }
        let drained: Vec<TrayAction> = [rx.try_recv(), rx.try_recv(), rx.try_recv()]
            .into_iter()
            .map(|received| received.expect("three activations queued"))
            .collect();
        assert_eq!(
            drained,
            vec![TrayAction::Toggle, TrayAction::Help, TrayAction::Quit]
        );
    }

    #[test]
    fn the_no_host_degrade_arrives_as_data_never_a_panic() {
        // A desktop without a `StatusNotifierWatcher`: presenting
        // reports NoHost on the same return path — the call completes
        // (no panic by construction) and everything else about the
        // app is unchanged, the degrade riding as data.
        let (tx, _rx) = mpsc::channel();
        let tray = RecordingTray::without_host();
        assert_eq!(
            tray.present(TrayMenu::new("DropTerm", "icon", &[TrayAction::Quit]), tx),
            TrayPresence::NoHost
        );
    }
}
