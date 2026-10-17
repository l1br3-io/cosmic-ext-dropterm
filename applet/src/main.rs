// SPDX-License-Identifier: GPL-3.0-only

// Pedantic lint table: `just clippy` runs with -D warnings, so every
// warn below is a build error. `standing:` entries are owner-approved
// exceptions (OQ rulings); `transient:` entries are the T1 census
// worklist — lints firing in this crate — and are deleted as their
// class is cleared, because an expect whose lint stops firing errors
// out (the self-cleanup mechanism). A standing entry appears only in
// roots where it fires, for the same reason.
#![warn(clippy::pedantic)]
#![cfg_attr(test, allow(clippy::float_cmp))] // OQ-1: tests assert exact constructed values
#![cfg_attr(test, allow(clippy::assert_is_empty))] // tests read better asserting is_empty than assert_eq!(x, [] as [T; 0])
#![expect(
    clippy::too_many_lines,
    reason = "standing: OQ-4 — dissolved by module-decomposition"
)]

mod applet;
mod dbus;
mod subscriptions;

fn main() -> cosmic::iced::Result {
    env_logger::init();
    cosmic::applet::run::<applet::Applet>(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    /// Pins the applet desktop file to its exact contract: the
    /// panel stem-matches `Exec`, cosmic-settings lists
    /// `X-CosmicApplet=true`, the shrinkable key allows padding to
    /// give before overflow, and — deliberately absent —
    /// `X-HostWaylandDisplay`: the applet needs no privileged
    /// Wayland socket.
    #[test]
    fn desktop_file_matches_the_pinned_contract() {
        let path = repo_root()
            .join("res")
            .join("io.github.l1br3_io.CosmicExtDropterm.Applet.desktop");
        let content = std::fs::read_to_string(path).expect("desktop file");
        assert_eq!(
            content,
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=DropTerm\n\
             Exec=cosmic-ext-dropterm-applet\n\
             Icon=io.github.l1br3_io.CosmicExtDropterm-symbolic\n\
             NoDisplay=true\n\
             X-CosmicApplet=true\n\
             X-CosmicShrinkable=true\n"
        );
    }

    /// The shipped icon must be symbolic — `currentColor` strokes and
    /// no fixed palette — so the panel's theme recolors it.
    #[test]
    fn shipped_icon_is_symbolic() {
        let path = repo_root()
            .join("res")
            .join("icons")
            .join("hicolor")
            .join("scalable")
            .join("apps")
            .join("io.github.l1br3_io.CosmicExtDropterm-symbolic.svg");
        let svg = std::fs::read_to_string(path).expect("symbolic icon");
        assert!(svg.contains("currentColor"));
        assert!(!svg.contains('#'));
    }
}
