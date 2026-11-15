// SPDX-License-Identifier: GPL-3.0-only

//! The toolkit-freedom pin (shared-core spec, re-homed by the cu-4
//! collapse): the `core` module must admit no libcosmic/iced/GPUI
//! dependency. The old two-halved pin (manifest grep + resolved
//! graph) split per crate and died with the separate `dropterm-core`
//! package — one package manifest now also names gpui for the bin's
//! view half, by design. The guarantee it pinned is structural now:
//! no toolkit import may appear anywhere under `src/core/`, so this
//! test walks the module's sources and asserts the imports clean. A
//! leaked toolkit use fails `just check`.

use std::path::{Path, PathBuf};

/// Import paths whose appearance under `src/core/` means a toolkit
/// leaked into the core module. Matched against every `use`/`extern
/// crate` line's leading path segment.
const FORBIDDEN: [&str; 4] = ["gpui", "gpui_platform", "libcosmic", "iced"];

/// Every `.rs` file under `src/core/` (the module and its tree).
fn core_sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read src/core") {
            let path = entry.expect("read src/core entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("core");
    let mut out = vec![
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("core.rs"),
    ];
    walk(&root, &mut out);
    out
}

#[test]
fn core_module_admits_no_toolkit_import() {
    for path in core_sources() {
        let text = std::fs::read_to_string(&path).expect("read core source");
        for line in text.lines() {
            // Only import- and extern-crate statements can pull a
            // toolkit's name into the module's scope. Doc comments and
            // prose naming a crate never compile into one.
            let trimmed = line.trim_start();
            let is_import = trimmed.starts_with("use ") || trimmed.starts_with("extern crate ");
            if !is_import {
                continue;
            }
            for forbidden in FORBIDDEN {
                let path_head = format!("{forbidden}::");
                let extern_head = format!("{forbidden} as");
                let bare = trimmed.starts_with(&format!("use {forbidden};"))
                    || trimmed.starts_with(&format!("extern crate {forbidden};"));
                let pathed = trimmed.contains(&path_head)
                    || trimmed.starts_with(&format!("use {extern_head}"))
                    || trimmed.starts_with(&format!("extern crate {extern_head}"));
                assert!(
                    !(bare || pathed),
                    "{} names `{forbidden}` in an import — the core module must stay toolkit-free: {line}",
                    path.display()
                );
            }
        }
    }
}
