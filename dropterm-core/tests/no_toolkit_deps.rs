// SPDX-License-Identifier: GPL-3.0-only

//! The toolkit-freedom pin (shared-core spec): `dropterm-core` must
//! admit no libcosmic/iced (or GPUI) dependency. Both halves are
//! asserted — the manifest is grepped directly, and the resolved
//! graph (`cargo tree` from the committed lockfile, normal edges,
//! scoped to this package) is asserted edge-clean — so a leaked
//! toolkit import fails `just check`.

use std::path::Path;
use std::process::Command;

/// Crate names whose presence in the graph means a toolkit leaked in.
const FORBIDDEN: [&str; 3] = ["libcosmic", "iced", "gpui"];

#[test]
fn manifest_declares_no_toolkit_dependency() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).expect("read dropterm-core manifest");
    // Comments don't declare dependencies — only keys and values do.
    // Naive cut at the first `#` per line: no manifest string in this
    // crate legitimately carries one (an app id or URL never does).
    let stripped: String = text
        .lines()
        .map(|line| match line.find('#') {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in FORBIDDEN {
        assert!(
            !stripped.contains(forbidden),
            "dropterm-core's manifest names `{forbidden}` — the core crate must stay toolkit-free"
        );
    }
}

#[test]
fn resolved_graph_has_no_toolkit_edges() {
    // `--locked` keeps resolution on the committed workspace
    // lockfile — no network, no drift; the same graph `just check`
    // builds. `--prefix none` puts one bare `name vX.Y.Z` per line,
    // so the package name is everything before the first space.
    // Scoped to this package: the workspace's other members do pull
    // toolkits, by design.
    let output = Command::new(env!("CARGO"))
        .args([
            "tree",
            "--locked",
            "--edges",
            "normal",
            "--prefix",
            "none",
            "-p",
            "dropterm-core",
        ])
        .current_dir(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("workspace root beside dropterm-core"),
        )
        .output()
        .expect("run cargo tree");
    assert!(
        output.status.success(),
        "cargo tree failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8_lossy(&output.stdout);
    for line in tree.lines() {
        let name = line.split(' ').next().unwrap_or_default();
        for forbidden in FORBIDDEN {
            assert_ne!(
                name, forbidden,
                "dropterm-core's resolved graph contains `{forbidden}`"
            );
        }
    }
}
