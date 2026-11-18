// SPDX-License-Identifier: GPL-3.0-only

//! The system dark/light mode (theme-defaults td-2): a process-global
//! cell the theme sites read instead of assuming dark, fed by the
//! xdg-desktop-portal appearance watch. The value mapping is pure and
//! pinned below; the watch is this module's one impure shell — its
//! own thread and current-thread tokio runtime (the `transport.rs`
//! shape), no coupling to the control bus's server lifecycle. The
//! fallback is also the pre-read state: dark until a read lands, and
//! dark again on every error path (no session bus, no portal, unknown
//! key, wrong type) — byte-identical to the pre-watch behavior on
//! bare sway.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use futures::StreamExt;
use zbus::proxy;

/// The portal's appearance namespace and its color-scheme key
/// (`org.freedesktop.portal.Settings`, the freedesktop color-scheme
/// spec: 0 no-preference, 1 prefer-dark, 2 prefer-light).
const APPEARANCE_NAMESPACE: &str = "org.freedesktop.appearance";
const COLOR_SCHEME_KEY: &str = "color-scheme";

/// Bounds the boot `ReadOne` — zbus 5 carries no default method
/// timeout (the gpui-scaffold finding), and a wedged portal would
/// leave the watch thread hung before the change stream ever arms.
/// Silence past the deadline is the same fallback as a failed read.
const READ_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

/// The current system mode. Dark at rest: the spec's fallback
/// (no-preference, portal absent, read error all resolve dark) is
/// also the state before any read lands.
static SYSTEM_DARK: AtomicBool = AtomicBool::new(true);

/// Bumped exactly when the applied mode changes — the shell's tick
/// compares it to notice an OS flip without re-resolving per frame.
static REVISION: AtomicU64 = AtomicU64::new(0);

/// The current system dark/light mode — dark until the watch's read
/// lands.
pub fn is_dark() -> bool {
    SYSTEM_DARK.load(Ordering::Relaxed)
}

/// The mode revision: changes when (and only when) [`is_dark`]
/// flipped since the caller's last read.
pub fn revision() -> u64 {
    REVISION.load(Ordering::Relaxed)
}

/// Apply a mode; bumps the revision exactly on a change, so a
/// portal answer agreeing with the current state stays silent.
pub fn set_dark(dark: bool) -> bool {
    if SYSTEM_DARK.swap(dark, Ordering::Relaxed) == dark {
        false
    } else {
        REVISION.fetch_add(1, Ordering::Relaxed);
        true
    }
}

/// The portal value → mode mapping, pure:
/// `org.freedesktop.appearance color-scheme` — 0 no-preference and
/// 1 prefer-dark resolve dark (no-preference's dark is the spec's
/// ruling), 2 prefer-light resolves light. A value the enum does not
/// define resolves dark too — the same fallback semantics, never a
/// panic on a newer portal.
pub const fn portal_color_scheme_is_dark(value: u32) -> bool {
    !matches!(value, 2)
}

/// Restores the seam's saved prior state on scope exit, panic or not.
struct Restore(bool, u64);
impl Drop for Restore {
    fn drop(&mut self) {
        SYSTEM_DARK.store(self.0, Ordering::Relaxed);
        REVISION.store(self.1, Ordering::Relaxed);
    }
}

/// The test seam: run `body` with the cell forced to `dark`,
/// restoring the prior state after — lock-guarded so parallel tests
/// can't race the global, and the restore runs on unwind too, so an
/// asserting body cannot leak its forced state into other tests.
/// The guard is held across `body`, so a nested seam on the same
/// thread deadlocks: tests must never nest it (none do — the
/// restore pin exercises exit, not recursion). Tests never
/// `set_dark` directly.
pub fn with_system_mode<T>(dark: bool, body: impl FnOnce() -> T) -> T {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let prior_dark = SYSTEM_DARK.swap(dark, Ordering::Relaxed);
    let prior_revision = REVISION.load(Ordering::Relaxed);
    let _restore = Restore(prior_dark, prior_revision);
    body()
}

// --- the portal watch (the module's impure shell) ---

/// The xdg-desktop-portal Settings interface, as consumed:
/// `ReadOne` for the boot read and the `SettingChanged` signal for
/// runtime flips.
#[proxy(
    interface = "org.freedesktop.portal.Settings",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
trait PortalSettings {
    /// `ReadOne(namespace, key) → value`.
    fn read_one(&self, namespace: &str, key: &str) -> zbus::Result<zbus::zvariant::OwnedValue>;

    /// `SettingChanged(namespace, key, value)`.
    #[zbus(signal)]
    fn setting_changed(&self, namespace: String, key: String, value: zbus::zvariant::OwnedValue);
}

/// Spawn the portal watch on its own thread: one `ReadOne` at boot,
/// then the `SettingChanged` stream — fallback-then-correct, so boot
/// never blocks on the portal (the `OPEN→FIRST_FRAME` budget stands;
/// the app boots hidden, and a correction lands before any human
/// sees a frame). Fire-and-forget: every failure logs once and the
/// cell stays where it was.
pub fn spawn_watch() {
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(err) => {
                return eprintln!("dropterm: system mode: no runtime: {err:?}");
            }
        };
        runtime.block_on(watch());
    });
}

/// The watch loop: connect, read the current preference, then follow
/// the change signal. Every error arm returns after one log line —
/// the mode stays where the last good answer left it.
async fn watch() {
    let conn = match zbus::Connection::session().await {
        Ok(conn) => conn,
        Err(err) => {
            return eprintln!("dropterm: system mode: no session bus ({err}); following dark");
        }
    };
    let portal = match PortalSettingsProxy::new(&conn).await {
        Ok(portal) => portal,
        Err(err) => {
            return eprintln!("dropterm: system mode: no portal ({err}); following dark");
        }
    };
    match tokio::time::timeout(
        READ_DEADLINE,
        portal.read_one(APPEARANCE_NAMESPACE, COLOR_SCHEME_KEY),
    )
    .await
    {
        Ok(Ok(value)) => apply_portal_value(&value),
        Ok(Err(err)) => {
            eprintln!("dropterm: system mode: read failed ({err}); following dark");
        }
        Err(_) => {
            eprintln!("dropterm: system mode: read timed out; following dark");
        }
    }
    let mut changes = match portal.receive_setting_changed().await {
        Ok(changes) => changes,
        Err(err) => {
            return eprintln!(
                "dropterm: system mode: no change stream ({err}); \
                 following the last resolved mode"
            );
        }
    };
    while let Some(change) = changes.next().await {
        let Ok(args) = change.args() else {
            continue;
        };
        if args.namespace != APPEARANCE_NAMESPACE || args.key != COLOR_SCHEME_KEY {
            continue;
        }
        apply_portal_value(&args.value);
    }
}

/// One portal value applied: a `u32` through the pinned mapping, a
/// wrong type logged once with the mode left where it was.
fn apply_portal_value(value: &zbus::zvariant::OwnedValue) {
    let code = match &**value {
        zbus::zvariant::Value::U32(code) => *code,
        other => {
            return eprintln!(
                "dropterm: system mode: unexpected color-scheme value {other:?}; \
                 following the current mode"
            );
        }
    };
    if set_dark(portal_color_scheme_is_dark(code)) {
        println!(
            "dropterm: system mode -> {}",
            if is_dark() { "dark" } else { "light" }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_values_map_no_preference_and_dark_to_dark_light_to_light() {
        assert!(portal_color_scheme_is_dark(0));
        assert!(portal_color_scheme_is_dark(1));
        assert!(!portal_color_scheme_is_dark(2));
    }

    #[test]
    fn an_undefined_portal_value_resolves_dark() {
        // A newer portal's unknown enum value gets the fallback, not
        // a panic or a silent light.
        assert!(portal_color_scheme_is_dark(3));
        assert!(portal_color_scheme_is_dark(u32::MAX));
    }

    #[test]
    fn the_cell_reports_the_set_mode_and_starts_dark() {
        // The fallback is the pre-read state: untouched, the process
        // resolves dark (pinned through the seam's forced true, which
        // is also the static's own rest value).
        with_system_mode(true, || assert!(is_dark()));
        with_system_mode(false, || assert!(!is_dark()));
    }

    #[test]
    fn set_dark_bumps_the_revision_exactly_on_a_change() {
        with_system_mode(true, || {
            assert!(!set_dark(true));
            let before = revision();
            assert!(set_dark(false));
            assert_eq!(revision(), before + 1);
            assert!(!set_dark(false));
            assert_eq!(revision(), before + 1);
        });
    }

    #[test]
    fn the_seam_restores_the_prior_state_on_exit() {
        // The outermost seam saves and restores: inside, the cell
        // reads forced; after the seam exits, the cell is back where
        // it started — the process default (dark), which every seam
        // in this suite restores its own prior onto, so no test can
        // leak a mode into another's. (The guard is held across the
        // body, so nesting a seam inside a seam would deadlock —
        // exit semantics are what this pin needs, not recursion.)
        with_system_mode(false, || assert!(!is_dark()));
        assert!(is_dark());
        with_system_mode(true, || assert!(is_dark()));
        assert!(is_dark());
    }

    #[test]
    fn a_portal_value_of_the_right_type_applies_the_mapping() {
        let light = zbus::zvariant::OwnedValue::try_from(zbus::zvariant::Value::U32(2))
            .expect("valid value");
        let dark = zbus::zvariant::OwnedValue::try_from(zbus::zvariant::Value::U32(1))
            .expect("valid value");
        with_system_mode(true, || {
            apply_portal_value(&light);
            assert!(!is_dark());
            apply_portal_value(&dark);
            assert!(is_dark());
        });
    }

    #[test]
    fn a_wrong_typed_portal_value_leaves_the_mode_alone() {
        let wrong = zbus::zvariant::OwnedValue::try_from(zbus::zvariant::Value::U16(1))
            .expect("valid value");
        with_system_mode(false, || {
            apply_portal_value(&wrong);
            assert!(!is_dark());
        });
    }
}
