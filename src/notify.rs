// SPDX-License-Identifier: GPL-3.0-only

//! Bell → desktop notification.
//!
//! The one DE feature a *hidden* terminal uniquely needs: a bell that
//! rings while the overlay is hidden posts a freedesktop desktop
//! notification. Delivery is direct D-Bus `org.freedesktop.Notifications`
//! (notify-rust) — COSMIC implements no Notification portal, and
//! cosmic-notifications serves the interface directly
//! (docs/research/terminal-de-integration.md §7).

use std::time::Duration;

/// The bell notification's app name — what the notification daemon
/// groups and displays the notification under. Identity-split: with a
/// dev instance and an installed release both running, `DropTerm`
/// alone cannot say which one rang.
#[cfg(not(feature = "dev-identity"))]
const NOTIFY_APP_NAME: &str = "DropTerm";
/// The dev build's twin — matches the `DropTerm (Dev)` names of the
/// dev launcher's desktop file and the dev applet's panel entry.
#[cfg(feature = "dev-identity")]
const NOTIFY_APP_NAME: &str = "DropTerm (Dev)";

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
            .appname(NOTIFY_APP_NAME)
            .summary(&tab_title)
            .body("A bell rang while the terminal was hidden.")
            .show();
        if let Err(err) = result {
            log::warn!("failed to post bell notification: {err}");
        }
    });
}

/// Whether a finished command posts a desktop notification
/// (command-notify spec): only while the overlay is hidden (bell
/// parity — visible tabs carry the frozen-duration segment or the
/// activity marker), only past the threshold (a hidden `ls` never
/// pings), and only once per tab per hide period (the bell latch).
// Dead until the Wakeup wiring lands (this chunk's Task 2); the
// attribute goes with it.
#[cfg_attr(not(test), expect(dead_code))]
pub fn should_notify_command(
    surface_hidden: bool,
    enabled: bool,
    duration: Duration,
    threshold: Duration,
    already_notified_this_hide: bool,
) -> bool {
    surface_hidden && enabled && duration >= threshold && !already_notified_this_hide
}

/// The finished-command notification's body (command-notify spec,
/// OQ-2): the sampled command name when one is in hand, falling back
/// to the generic "Command" — the same honesty as the frozen pairing —
/// with the duration in the band's own `m:ss`/`h:mm:ss` shape
/// (command-lifecycle spec, OQ-3), so the notification and the
/// frozen segment never disagree.
// Dead until the Wakeup wiring lands (this chunk's Task 2); the
// attribute goes with it.
#[cfg_attr(not(test), expect(dead_code))]
fn command_body(name: Option<&str>, duration: Duration) -> String {
    let name = name.unwrap_or("Command");
    format!(
        "{name} finished in {}",
        crate::status_bar::format_duration(duration)
    )
}

/// Posts the finished-command notification, off the main thread
/// (the bell post's reasoning: a D-Bus round-trip must never stall
/// the UI thread). A failed post (no notification daemon) logs a
/// warning and vanishes — the terminal is unaffected.
// Dead until the Wakeup wiring lands (this chunk's Task 2); the
// attribute goes with it.
#[expect(dead_code)]
pub fn post_command_notification(tab_title: String, name: Option<&str>, duration: Duration) {
    let body = command_body(name, duration);
    std::thread::spawn(move || {
        let result = notify_rust::Notification::new()
            .appname(NOTIFY_APP_NAME)
            .summary(&tab_title)
            .body(&body)
            .show();
        if let Err(err) = result {
            log::warn!("failed to post command-finished notification: {err}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{NOTIFY_APP_NAME, command_body, should_notify, should_notify_command};
    use std::time::Duration;

    const THRESHOLD: Duration = Duration::from_secs(5);

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

    #[test]
    fn a_finished_command_notifies_only_while_the_terminal_is_hidden() {
        assert!(should_notify_command(
            true,
            true,
            Duration::from_secs(6),
            THRESHOLD,
            false
        ));
        // Visible: the frozen-duration segment or the activity marker
        // carries the signal instead.
        assert!(!should_notify_command(
            false,
            true,
            Duration::from_secs(6),
            THRESHOLD,
            false
        ));
    }

    #[test]
    fn a_disabled_command_setting_never_notifies() {
        assert!(!should_notify_command(
            true,
            false,
            Duration::from_secs(6),
            THRESHOLD,
            false
        ));
    }

    #[test]
    fn an_under_threshold_finish_never_notifies() {
        assert!(!should_notify_command(
            true,
            true,
            Duration::from_secs(4),
            THRESHOLD,
            false
        ));
    }

    #[test]
    fn a_finish_exactly_at_the_threshold_notifies() {
        // OQ-3's `>=` ruling: 5 s sharp counts.
        assert!(should_notify_command(
            true, true, THRESHOLD, THRESHOLD, false
        ));
    }

    #[test]
    fn a_command_latched_tab_does_not_notify_again_until_the_next_show() {
        assert!(!should_notify_command(
            true,
            true,
            Duration::from_secs(6),
            THRESHOLD,
            true
        ));
    }

    #[test]
    fn the_body_names_the_command_when_one_was_sampled() {
        // OQ-2's ruling: the sampled name, the band's own duration
        // shape (command-lifecycle OQ-3).
        assert_eq!(
            command_body(Some("cargo"), Duration::from_secs(161)),
            "cargo finished in 2:41"
        );
    }

    #[test]
    fn the_body_falls_back_when_no_name_was_sampled() {
        assert_eq!(
            command_body(None, Duration::from_secs(161)),
            "Command finished in 2:41"
        );
    }

    #[test]
    #[cfg(not(feature = "dev-identity"))]
    fn the_release_build_notifies_as_dropterm() {
        assert_eq!(NOTIFY_APP_NAME, "DropTerm");
    }

    #[test]
    #[cfg(feature = "dev-identity")]
    fn the_dev_build_notifies_as_dropterm_dev() {
        assert_eq!(NOTIFY_APP_NAME, "DropTerm (Dev)");
    }
}
