// SPDX-License-Identifier: GPL-3.0-only

// Pedantic lint table: `just clippy` runs with -D warnings, so every
// warn below is a build error. `standing:` entries are owner-approved
// exceptions (OQ rulings); a standing entry appears only in roots
// where it fires, for the same reason as the root crate's table.
#![warn(clippy::pedantic)]
#![cfg_attr(test, allow(clippy::float_cmp))]
// OQ-1: tests assert exact constructed values
// The is_empty assert allow rides clippy's rename — see lib.rs's
// table note.
#![cfg_attr(
    test,
    allow(unknown_lints, clippy::assert_is_empty, clippy::const_is_empty)
)]
// tests read better asserting is_empty

//! `dropterm` — `DropTerm`'s GPUI frontend (gpui-scaffold spec).
//!
//! Phase 3's second binary beside the libcosmic one: it owns the same
//! runtime identity ([`dropterm::core::APP_ID`]) and serves the same
//! contract, on the ruled gpui-pre pin. Ships nothing until cutover —
//! the release boundary holds.

mod app_shell;
mod context_menu;
mod dbus;
mod drawer;
mod executor;
mod fonts;
mod images_panel;
mod kit_check;
mod notify;
mod onboarding;
mod palette;
mod pickers;
mod startup;
mod status_bar;
mod tab_bar;
mod terminal;
mod terminal_walk;
// The carried C4 generator (gpui-theme planning ruling: verbatim-plus-
// compile — the file stays byte-identical to the spike's apart from
// the SPDX header). The allows below are the compile adaptation, not
// mapping changes: the doc prose and closure shapes predate the
// pedantic table, and `dropterm_dark` stays test-only by design (the
// C4 fixture scheme; Task 2 wired the generator itself into the apply
// path).
#[allow(
    clippy::doc_markdown,
    unused_mut,
    dead_code,
    reason = "carried verbatim from the spike; only the test fixture stays unused"
)]
mod theme_gen;
mod tray;

use std::sync::Arc;

use clap::Parser;

use dropterm::core::dbus_state;
use dropterm::core::transport::{self, BusName, ForwardOutcome};

// The CLI surface — surface verbs and tab verbs alike — lives in
// `cli` (py-5): one clap truth the launch, the forward encoder, and
// the dbus decode all share.
mod cli;

/// The env-selected scenario the scaffold can boot besides the shell —
/// the carried patch's verification harness (`DROPTERM_CHECK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckMode {
    /// kit-pass C2b: Root + Kit overlays through the reshape cadence.
    KitC2b,
    /// gpui-terminal Task 1: the live pane's render + resize walk.
    TerminalGrid,
}

/// `DROPTERM_CHECK`'s parse: unset or empty boots the real shell; only
/// the exact scenario name arms its harness (a typo falls back to the
/// shell, whose evidence lines name no scenario — visibly wrong in a
/// walk log, never silently half-armed).
fn check_mode_from_env(raw: Option<&str>) -> Option<CheckMode> {
    match raw?.trim() {
        "kit-c2b" => Some(CheckMode::KitC2b),
        "terminal-grid" => Some(CheckMode::TerminalGrid),
        _ => None,
    }
}

/// Pin glibc's trim threshold before any heavy allocation lands (the
/// F12 ratchet fix, measured 2026-11-16): every show/hide cycle tears
/// down and rebuilds two GPUI windows (the measuring pass plus the
/// swap on show, the slide-out destroy on hide), and the freed heap —
/// fonts, scenes, grids — piles onto the arena's top chunk. glibc's
/// *default* threshold is also 128 KiB, but it self-adjusts upward
/// after large frees, so the top never comes back down and RSS
/// ratchets by a sustained ~50 kB per toggle, unbounded (the VM
/// probe: every kB of the +3 MB growth over 100 toggles landed in
/// `[heap]`; the same probe with the threshold pinned ran flat).
/// Setting it explicitly disables that
/// dynamic adjustment, so each cycle's churn returns to the OS.
fn pin_malloc_trim_threshold() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: mallopt only adjusts allocator parameters; it touches
    // no memory the program owns. Called before any thread that
    // allocates has started, so the parameter applies process-wide.
    unsafe {
        libc::mallopt(libc::M_TRIM_THRESHOLD, 128 * 1024);
    }
}

/// The pane environment's one boot-time truth (the hidden-paste
/// fix): a terminal emulator advertises its own emulation — the pty
/// runs THIS VTE, never the launching terminal's. Without it the
/// panes inherit the launching env's `TERM`: a desktop-launched
/// drop-down (`autostart`, `DBus` activation) inherits a GUI session
/// with no `TERM` at all, and the shell's line editor degrades —
/// bracketed paste turns off, so a paste arrives as raw typing and
/// readline's scrolled redraw hides the pasted line's head until a
/// backspace forces a rewrite (the user's report). The vendored
/// alacritty-terminal's own answer, called exactly where alacritty
/// calls it: `TERM=alacritty` when the system carries that terminfo,
/// else `xterm-256color`, plus `COLORTERM=truecolor`.
fn setup_pane_env() {
    alacritty_terminal::tty::setup_env();
}

fn main() {
    pin_malloc_trim_threshold();
    setup_pane_env();
    println!("dropterm: booting on the gpui-pre 0.3.12 pin (wayland)");
    let args = cli::Args::parse();
    // The query path exits before any bus-name attempt and before the
    // guard's probe: it creates no surfaces, and its not-running answer
    // is truthful because the guard keeps an unsupported session from
    // ever hosting an instance (the frontend's ordering, kept).
    if args.is_visible {
        let (visible, code) = dbus_state::is_visible_decision(transport::query_visibility());
        println!("{visible}");
        std::process::exit(i32::from(code));
    }
    // The startup dispatch, before any GPUI initialization — the
    // probe takes its connection from the environment before the
    // platform reads it, and a session with no Wayland connection at
    // all never reaches the toolkit. The verdict it returns is the
    // surface strategy the executor runs under.
    let surface_mode = startup::surface_mode_or_exit();

    // The control bus's launch half: on a throwaway connection decide
    // whether this launch is the instance (the ownership itself is
    // taken by the served loop, on its own runtime).
    let action = args.action.unwrap_or_default();
    if !transport::single_instance_disabled() {
        match transport::bus_name_status() {
            BusName::Unowned | BusName::NoBus => {}
            BusName::OwnedByAnother => {
                // The payload words are owned here (a `--cwd` path, a
                // `--cmd` line, a profile name): borrowed for the one
                // forward, then dropped with the launch.
                let words = cli::forward_words(&action);
                let word_refs: Vec<&str> = words.iter().map(String::as_str).collect();
                match transport::forward_outcome(&transport::forward_to_running(
                    &action.to_string(),
                    word_refs,
                )) {
                    // The running instance applied the verb; a second
                    // surface must never appear. Today's contract: the
                    // forwarding launch exits 0.
                    ForwardOutcome::Delivered => std::process::exit(0),
                    // The name is owned but nobody answered — a stale
                    // binary mid-upgrade. Refuse, never a duplicate
                    // instance (the frontend's wording, kept).
                    ForwardOutcome::Refused => {
                        eprintln!(
                            "dropterm: another instance owns the bus name but did not \
                             answer (stale binary?); refusing to start a second instance"
                        );
                        std::process::exit(1);
                    }
                }
            }
        }
    }

    // A quit with nothing to forward into (tray Task T2, the tray
    // spec's ruling): no instance exists, so the quit is already
    // satisfied — one line, exit 0, never a boot and never a
    // fabricated start. The query path above is the early-exit
    // precedent.
    if cli::first_launch_exits_zero(&action) {
        println!("dropterm: no running instance to quit");
        return;
    }

    let served = dbus::spawn_service();
    // The tray (tray Task T3), spawned beside the control bus: the
    // channel is the only bridge — the backend hands activations to
    // the render loop's verb funnel, the render loop never calls into
    // the tray. Linux only (SNI is the Linux desktops' tray
    // protocol); the receiver drains empty everywhere else.
    #[cfg(target_os = "linux")]
    let tray_actions = {
        let (tx, rx) = std::sync::mpsc::channel();
        tray::spawn_linux(tx);
        rx
    };
    #[cfg(not(target_os = "linux"))]
    let tray_actions = std::sync::mpsc::channel::<tray::TrayAction>().1;
    let exit_after =
        app_shell::exit_after_from_env(std::env::var("DROPTERM_EXIT_AFTER_MS").ok().as_deref());
    let check = check_mode_from_env(std::env::var("DROPTERM_CHECK").ok().as_deref());
    // First launch has no running state to act on remotely: the tab
    // verbs degrade to a plain start through `startup_action` (the
    // degradation logged the libcosmic way); the surface verbs and a
    // `new-tab` keep startup application — the boot applies them.
    let startup = cli::startup_action(&action);
    if startup != action {
        println!("dropterm: {action} with no running instance: starting plain");
    }
    let phase = Arc::clone(&served.phase);
    let tabs = Arc::clone(&served.tabs);
    let verbs = served.verbs;
    // Explicit quit only: the app's lifetime is its own, never the
    // windows'. GPUI's default mode on Linux quits when the last
    // window closes — correct for an editor, wrong for a single-
    // instance daemon that boots hidden (zero windows) and hides by
    // destroying its surface (momentarily zero windows again while a
    // swap or a hide runs; the first walk caught the swap's
    // destroy-before-create frame quitting the whole instance).
    gpui_platform::application()
        // The Kit's icon bundle: every path-based icon the components
        // draw (the settings accordion's chevron, menu checkmarks)
        // resolves through the app's AssetSource, and without a
        // source those paths paint nothing. gpui-kit-assets is the
        // sanctioned carrier — the graph pin forbids only the
        // gpui-kit / gpui-shell aggregates.
        .with_assets(gpui_kit_assets::Assets)
        .with_quit_mode(gpui::QuitMode::Explicit)
        .run(move |cx: &mut gpui::App| {
            // The bundled default family registers before any pane
            // resolves (fonts.rs, gnome-window Task 5): every boot
            // mode passes through this one closure, so the resolver's
            // terminal fallback is held by construction — the bare-rig
            // walk's `fc-list` empty row closes here.
            fonts::register_bundled_fonts(cx);
            // The missing-glyph log rides the app's lifetime: a
            // dropped subscription disables the reporting, so the
            // boot forgets it deliberately (fonts.rs's hardening —
            // tofu lands in the log instead of a walk's blind spot).
            std::mem::forget(fonts::wire_missing_glyph_log(cx));
            match check {
                Some(CheckMode::KitC2b) => kit_check::boot(cx, exit_after),
                Some(CheckMode::TerminalGrid) => terminal_walk::boot(cx, exit_after),
                None => app_shell::boot(
                    cx,
                    exit_after,
                    &phase,
                    &tabs,
                    verbs,
                    startup,
                    surface_mode,
                    tray_actions,
                ),
            }
        });
    println!("dropterm: event loop exited cleanly");
}

#[cfg(test)]
mod tests {
    // The manifest/graph pins (gpui-scaffold Task 1): the ruled pin is
    // exact, wayland-only, and the dependency graph never grows an
    // edge to the Kit's aggregates — so a careless bump fails `just
    // check` instead of shipping.

    use super::{check_mode_from_env, setup_pane_env};
    use std::process::Command;

    #[test]
    fn boot_advertises_the_terms_the_ptys_inherit() {
        // The hidden-paste fix's own pin (the covering report): a
        // pane's shell must see a real TERM — with none, readline
        // degrades (bracketed paste off, conservative redraws) and a
        // paste arrives as raw typing that hides until a backspace
        // redraws. The boot's one call carries the vendored
        // alacritty-terminal's own answer: TERM=alacritty when the
        // system carries that terminfo, else xterm-256color, plus
        // COLORTERM=truecolor. The pane-level render half of the
        // guard is the ignored live-PTY test in terminal::view.
        let restore_term = std::env::var_os("TERM");
        let restore_colorterm = std::env::var_os("COLORTERM");
        // SAFETY: single-threaded within this test's critical
        // section; the process-global writes are restored below and
        // no parallel test of this crate reads TERM.
        unsafe {
            std::env::remove_var("TERM");
            std::env::remove_var("COLORTERM");
        }
        setup_pane_env();
        let term = std::env::var("TERM").unwrap_or_default();
        assert!(
            term == "alacritty" || term == "xterm-256color",
            "TERM must be the emulation this VTE owns, got {term:?}"
        );
        assert_eq!(
            std::env::var("COLORTERM").unwrap_or_default(),
            "truecolor",
            "COLORTERM advertises the 24-bit pipeline"
        );
        // SAFETY: as above — restoring what the process carried.
        unsafe {
            match restore_term {
                Some(value) => std::env::set_var("TERM", value),
                None => std::env::remove_var("TERM"),
            }
            match restore_colorterm {
                Some(value) => std::env::set_var("COLORTERM", value),
                None => std::env::remove_var("COLORTERM"),
            }
        }
    }

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
        assert_eq!(
            check_mode_from_env(Some("terminal-grid")),
            Some(CheckMode::TerminalGrid)
        );
        // Unset, empty, and every misspelling boot the real shell;
        // surrounding whitespace is not a misspelling.
        assert_eq!(check_mode_from_env(None), None);
        assert_eq!(check_mode_from_env(Some("")), None);
        assert_eq!(check_mode_from_env(Some("KIT-C2B")), None);
        assert_eq!(check_mode_from_env(Some("kitc2b")), None);
        assert_eq!(check_mode_from_env(Some("terminal_grid")), None);
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

    #[test]
    fn the_shipped_desktop_entry_lists_the_icon_and_pins_identity() {
        // The icon-activation contract (Task I1): the shipped entry is
        // what a launcher clicks. It may never silently regress to
        // hidden — NoDisplay is the "icon does not work" bug in one
        // line — nor to an identity the surface never claims: without
        // StartupWMClass matching the surface's app id, a launcher
        // cannot associate the booted windowless process back to its
        // icon. Exec stays the bare toggle verb: a cold click
        // boots-and-shows through the default startup action, a warm
        // click forwards `toggle` and exits at the single-instance
        // seam.
        let entry = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../res/io.github.l1br3_io.dropterm.desktop"
        ))
        .expect("the shipped desktop entry beside the workspace root");
        assert!(
            !entry.lines().any(|l| l.starts_with("NoDisplay")),
            "the entry must list in launchers — NoDisplay has no business here:\n{entry}"
        );
        for field in [
            "Exec=dropterm",
            "Type=Application",
            "Terminal=false",
            "StartupWMClass=io.github.l1br3_io.dropterm",
        ] {
            assert!(
                entry.lines().any(|l| l == field),
                "the entry must carry {field}:\n{entry}"
            );
        }
        // The identity half of the pin runs in the release feature set
        // only: under dev-identity APP_ID flips to the `.Dev` slot
        // while this shipped entry keeps describing the release (the
        // justfile's dev generator rewrites its own copy's WMClass).
        #[cfg(not(feature = "dev-identity"))]
        assert_eq!(
            dropterm::core::APP_ID,
            "io.github.l1br3_io.dropterm",
            "the entry's StartupWMClass and the surface app id have drifted apart"
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
