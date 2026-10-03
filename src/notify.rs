// SPDX-License-Identifier: GPL-3.0-only

//! Bell → desktop notification.
//!
//! The one DE feature a *hidden* terminal uniquely needs: a bell that
//! rings while the overlay is hidden posts a freedesktop desktop
//! notification. Delivery is direct D-Bus `org.freedesktop.Notifications`
//! (notify-rust) — COSMIC implements no Notification portal, and
//! cosmic-notifications serves the interface directly
//! (docs/research/terminal-de-integration.md §7).

/// Whether a bell posts a desktop notification: only while the overlay
/// is hidden (kitty/iTerm2 semantics — when visible, the tab-bar 🔔
/// marker alone carries the signal), only when the setting is enabled,
/// and only once per tab per hide period (the anti-spam latch, cleared
/// on every show).
pub fn should_notify(
    surface_hidden: bool,
    bell_notifications_enabled: bool,
    already_notified_this_hide: bool,
) -> bool {
    surface_hidden && bell_notifications_enabled && !already_notified_this_hide
}

/// Posts the bell notification, off the main thread: D-Bus round-trips
/// must never stall the UI thread, the same reasoning that sends URL
/// opens through `open::that_detached`. A failed post (no notification
/// daemon) logs a warning and vanishes — the terminal is unaffected.
pub fn post_bell_notification(tab_title: String) {
    std::thread::spawn(move || {
        let result = notify_rust::Notification::new()
            .appname("DropTerm")
            .summary(&tab_title)
            .body("A bell rang while the terminal was hidden.")
            .show();
        if let Err(err) = result {
            log::warn!("failed to post bell notification: {err}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::should_notify;

    #[test]
    fn a_bell_notifies_only_while_the_terminal_is_hidden() {
        assert!(should_notify(true, true, false));
        // Visible: the tab-bar marker carries the signal instead.
        assert!(!should_notify(false, true, false));
    }

    #[test]
    fn a_disabled_setting_never_notifies() {
        assert!(!should_notify(true, false, false));
    }

    #[test]
    fn a_latched_tab_does_not_notify_again_until_the_next_show() {
        // One notification per tab per hide period: the latch arms on
        // the first notification and clears only when the surface is
        // shown again.
        assert!(!should_notify(true, true, true));
    }
}
