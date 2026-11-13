// SPDX-License-Identifier: GPL-3.0-only

// Pedantic lint table: `just clippy` runs with -D warnings, so every
// warn below is a build error. `standing:` entries are owner-approved
// exceptions (OQ rulings); a standing entry appears only in roots
// where it fires, for the same reason as the root crate's table.
#![warn(clippy::pedantic)]
#![cfg_attr(test, allow(clippy::float_cmp))] // OQ-1: tests assert exact constructed values

//! `dropterm-gpui` — `DropTerm`'s GPUI frontend (gpui-scaffold spec).
//!
//! Phase 3's second binary beside the libcosmic one: it owns the same
//! runtime identity ([`dropterm_core::APP_ID`]) and serves the same
//! contract, on the ruled gpui-pre pin. Ships nothing until cutover —
//! the release boundary holds.

mod app_shell;

fn main() {
    println!("dropterm-gpui: booting on the gpui-pre 0.3.12 pin (wayland)");
    let exit_after =
        app_shell::exit_after_from_env(std::env::var("DROPTERM_EXIT_AFTER_MS").ok().as_deref());
    gpui_platform::application().run(move |cx: &mut gpui::App| {
        app_shell::boot(cx, exit_after);
    });
    println!("dropterm-gpui: event loop exited cleanly");
}

#[cfg(test)]
mod tests {
    // The manifest/graph pins (gpui-scaffold Task 1): the ruled pin is
    // exact, wayland-only, and the dependency graph never grows an
    // edge to the Kit's aggregates — so a careless bump fails
    // `just check` instead of shipping.

    use std::process::Command;

    /// This crate's own manifest text.
    fn manifest() -> String {
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
            .expect("crate manifest beside the crate root")
    }

    #[test]
    fn the_ruled_gpui_pins_are_exact_and_wayland_only() {
        let text = manifest();
        // The pin (plan open question item 8, ruled 2026-11-13):
        // published crates, never a zed git pin — gpui-pre exact at
        // 0.3.12, default features off, wayland + font-kit only, and
        // the platform crate exact at the same version with wayland
        // (the consumer-side switch into the Linux crate's wayland
        // backend).
        let gpui = text
            .lines()
            .find(|l| l.contains("package = \"gpui-pre\""))
            .expect("a gpui-pre dependency line in the manifest");
        assert!(
            gpui.contains("version = \"=0.3.12\""),
            "pin is exact: {gpui}"
        );
        assert!(
            gpui.contains("default-features = false"),
            "defaults off (x11 & friends never enter): {gpui}"
        );
        assert!(
            gpui.contains("features = [\"wayland\", \"font-kit\"]"),
            "wayland + font-kit, the Kit's own set: {gpui}"
        );
        let platform = text
            .lines()
            .find(|l| l.contains("package = \"gpui-pre-platform\""))
            .expect("a gpui-pre-platform dependency line in the manifest");
        assert!(
            platform.contains("version = \"=0.3.12\""),
            "platform pin rides the same exact version: {platform}"
        );
        assert!(
            platform.contains("\"wayland\""),
            "the wayland backend is selected: {platform}"
        );
        // The spec's boundary: never GPUI's x11/macOS features —
        // Phase 4–5 own those.
        for line in text.lines().filter(|l| l.contains("gpui-pre")) {
            assert!(!line.contains("x11"), "no x11 feature: {line}");
        }
    }

    /// Every `(package, version)` pair in this crate's normal
    /// dependency graph, deduplicated — `cargo tree --prefix none`
    /// emits one `name vX.Y.Z (source)` per node.
    fn graph_packages() -> Vec<(String, String)> {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let out = Command::new(cargo)
            .args([
                "tree",
                "-p",
                env!("CARGO_PKG_NAME"),
                "-e",
                "normal",
                "--locked",
                "--prefix",
                "none",
            ])
            .output()
            .expect("cargo tree runs");
        assert!(
            out.status.success(),
            "cargo tree failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut pkgs: Vec<(String, String)> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let name = fields.next()?;
                let version = fields.next()?.strip_prefix('v')?;
                Some((name.to_string(), version.to_string()))
            })
            .collect();
        pkgs.sort_unstable();
        pkgs.dedup();
        pkgs
    }

    #[test]
    fn the_graph_resolves_the_gpui_family_at_the_pin() {
        let pkgs = graph_packages();
        // Non-vacuous: the family must be present at all — an emptied
        // manifest would otherwise pass every version check below.
        let family: Vec<&(String, String)> = pkgs
            .iter()
            .filter(|(name, _)| name.starts_with("gpui"))
            .collect();
        assert!(!family.is_empty(), "the gpui family is in the graph at all");
        for (name, version) in &family {
            // gpui-pre-reqwest rides its own 0.12 line (its semver
            // universe); every other family crate is a 0.3.12 snapshot
            // cut. The renamed top-level `gpui` alias resolves to
            // gpui-pre and shows under the family prefix either way.
            assert!(
                version == "0.3.12" || name == "gpui-pre-reqwest",
                "{name} resolved at {version}, not the ruled 0.3.12 snapshot"
            );
        }
    }

    #[test]
    fn the_graph_has_no_kit_aggregate_edges() {
        // C3 (kit-pass): the Kit enters component-only — gpui-component
        // brings gpui-base, never the gpui-kit aggregate, never
        // gpui-shell. Exact names: gpui-kit-assets is a different
        // crate and gpui-base is expected.
        let pkgs = graph_packages();
        for forbidden in ["gpui-shell", "gpui-kit"] {
            assert!(
                !pkgs.iter().any(|(name, _)| name == forbidden),
                "{forbidden} entered the graph — the Kit must stay component-only"
            );
        }
    }

    #[test]
    fn the_lock_reconciles_unicode_properties_at_the_spike_pin() {
        // Check 14's one reconciliation (gs-8): gpui_web exact-pins
        // unicode-properties =0.1.3 while libcosmic's chain accepts any
        // 0.1.x, so the one shared lockfile settles at 0.1.3 — and
        // both trees built green there. Pinned here so a future
        // resolution drift fails the gate instead of silently forking
        // or moving the shared version.
        let pkgs = graph_packages();
        let seen: Vec<&(String, String)> = pkgs
            .iter()
            .filter(|(name, _)| name == "unicode-properties")
            .collect();
        assert_eq!(
            seen,
            vec![&("unicode-properties".to_string(), "0.1.3".to_string())],
            "unicode-properties must resolve once, at 0.1.3"
        );
    }
}
