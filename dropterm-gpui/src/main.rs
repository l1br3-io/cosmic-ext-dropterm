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
mod kit_check;
mod startup;

/// The env-selected scenario the scaffold can boot besides the shell —
/// the carried patch's verification harness (`DROPTERM_CHECK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckMode {
    /// kit-pass C2b: Root + Kit overlays through the reshape cadence.
    KitC2b,
}

/// `DROPTERM_CHECK`'s parse: unset or empty boots the real shell; only
/// the exact scenario name arms its harness (a typo falls back to the
/// shell, whose evidence lines name no scenario — visibly wrong in a
/// walk log, never silently half-armed).
fn check_mode_from_env(raw: Option<&str>) -> Option<CheckMode> {
    match raw?.trim() {
        "kit-c2b" => Some(CheckMode::KitC2b),
        _ => None,
    }
}

fn main() {
    println!("dropterm-gpui: booting on the gpui-pre 0.3.12 pin (wayland)");
    // The layer-shell guard, before any GPUI initialization — the
    // probe takes its connection from the environment before the
    // platform reads it, and an unsupported session never reaches the
    // toolkit at all.
    startup::guard_or_exit();
    let exit_after =
        app_shell::exit_after_from_env(std::env::var("DROPTERM_EXIT_AFTER_MS").ok().as_deref());
    let check = check_mode_from_env(std::env::var("DROPTERM_CHECK").ok().as_deref());
    gpui_platform::application().run(move |cx: &mut gpui::App| match check {
        Some(CheckMode::KitC2b) => kit_check::boot(cx, exit_after),
        None => app_shell::boot(cx, exit_after),
    });
    println!("dropterm-gpui: event loop exited cleanly");
}

#[cfg(test)]
mod tests {
    // The manifest/graph pins (gpui-scaffold Task 1): the ruled pin is
    // exact, wayland-only, and the dependency graph never grows an
    // edge to the Kit's aggregates — so a careless bump fails
    // `just check` instead of shipping.

    use super::check_mode_from_env;
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

    #[test]
    fn check_mode_arms_only_the_exact_scenario_name() {
        use super::CheckMode;
        assert_eq!(
            check_mode_from_env(Some("kit-c2b")),
            Some(CheckMode::KitC2b)
        );
        assert_eq!(
            check_mode_from_env(Some(" kit-c2b ")),
            Some(CheckMode::KitC2b)
        );
        // Unset, empty, and every misspelling boot the real shell;
        // surrounding whitespace is not a misspelling.
        assert_eq!(check_mode_from_env(None), None);
        assert_eq!(check_mode_from_env(Some("")), None);
        assert_eq!(check_mode_from_env(Some("KIT-C2B")), None);
        assert_eq!(check_mode_from_env(Some("kitc2b")), None);
    }

    #[test]
    fn the_kit_enters_component_only_at_the_ruled_version() {
        // kit-pass C3 + plan item 7's ruling: gpui-component exact at
        // 0.9.1, component-only (default-features = false) — the
        // gpui-kit aggregate never enters by our hand, and the graph
        // side of that fence is the no-aggregate-edges test below.
        let text = manifest();
        let kit = text
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with("gpui-component"))
            .expect("a gpui-component dependency line in the manifest");
        assert!(
            kit.contains("version = \"=0.9.1\""),
            "the Kit is exact-pinned: {kit}"
        );
        assert!(
            kit.contains("default-features = false"),
            "component-only, no aggregate features: {kit}"
        );
    }

    #[test]
    fn the_carried_copy_is_the_patched_kit() {
        // The ruling (plan item 7): carried/gpui-component is the 0.9.1
        // source under [patch.crates-io] with exactly one const change
        // — the Linux SHADOW_SIZE zeroed, so a Client-decorated layer
        // surface negotiates its configured size instead of 2×20 px
        // larger. This drift-gates the copy: a bumped copy that
        // forgets the const fails `just check`.
        let toml = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../carried/gpui-component/Cargo.toml"
        ))
        .expect("the carried copy's manifest");
        assert!(
            toml.contains("name = \"gpui-component\""),
            "the copy keeps the patch's identity"
        );
        assert!(
            toml.contains("version = \"0.9.1\""),
            "the copy is the ruled 0.9.1 source"
        );
        let border = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../carried/gpui-component/src/window_border.rs"
        ))
        .expect("the carried copy's window_border.rs");
        assert!(
            !border.contains("px(20.0)"),
            "the 20 px shadow is gone from the carried source"
        );
        let linux_arm = border
            .lines()
            .zip(border.lines().skip(1))
            .find(|(cfg, _)| cfg.contains("cfg(target_os = \"linux\")"))
            .map(|(_, const_line)| const_line.to_string())
            .expect("the Linux const arm");
        assert!(
            linux_arm.contains("SHADOW_SIZE: Pixels = px(0.0)"),
            "the one-const change stands: {linux_arm}"
        );
        let root = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.toml"))
            .expect("the workspace manifest");
        assert!(
            root.contains("gpui-component = { path = \"carried/gpui-component\" }"),
            "the [patch.crates-io] entry points at the copy"
        );
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
        // manifest would otherwise pass every version check below. The
        // prefix is gpui-pre (the snapshot family); the Kit's own
        // crates (gpui-component, gpui-base, 0.9.x) ride their own
        // line and are not held to this pin.
        let family: Vec<&(String, String)> = pkgs
            .iter()
            .filter(|(name, _)| name.starts_with("gpui-pre"))
            .collect();
        assert!(
            !family.is_empty(),
            "the gpui-pre snapshot family is in the graph at all"
        );
        for (name, version) in &family {
            // gpui-pre-reqwest rides its own 0.12 line (its semver
            // universe); every other family crate is a 0.3.12 snapshot
            // cut.
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
