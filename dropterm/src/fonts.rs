// SPDX-License-Identifier: GPL-3.0-only

//! The bundled default family (`gnome-window` Task 5): `DropTerm` carries
//! its own Noto Sans Mono faces and registers them into GPUI's text
//! system at boot, so the font resolver's terminal fallback (the
//! `terminal::host` candidate walk's last resort) is held by
//! construction. Two recorded lessons close together: the plan
//! record's bare-rig row — a base container ships no fonts at all
//! (`fc-list` empty), so no system candidate is guaranteed — and the
//! silent-failure pin in `terminal/host.rs`: an unheld family paints
//! quads but never glyphs. A fallback that names no system family is
//! a default that works only by luck; the bundle removes the luck.
//!
//! Provenance: the Google Fonts-served hinted instances of the Noto
//! Sans Mono v37 monthly, under the OFL-1.1 license carried alongside
//! (`res/fonts/OFL.txt`; embedding is permitted, GPL-3.0 app + OFL
//! font). The family ships Regular and Bold — upstream builds no
//! italics, so italic runs resolve through GPUI's fallback chain like
//! any other missing face. Measured cmap coverage of the
//! terminal-critical sets (`fc-query` over the carried bytes, recorded
//! at acquisition): box drawing U+2500–257F 128/128, block elements
//! U+2580–259F 32/32, geometric shapes U+25A0–25FF 96/96, arrows
//! 2190–21FF 23/112; powerline, braille, emoji, and CJK are absent —
//! those still ride the system's fallback fonts where held, and tofu
//! there is a system-fonts concern, not a boot concern.

use std::borrow::Cow;

/// The bundled family's exact name: the config's `font_name` default
/// and the resolver's terminal fallback both resolve against this, so
/// after registration runs the default is held on every platform —
/// pinned by [`tests::the_bundled_family_is_the_configured_default`].
pub(crate) const BUNDLED_FAMILY: &str = "Noto Sans Mono";

/// The bundled sfnt streams under `res/fonts/`, embedded at compile
/// time — nothing reads the filesystem at boot, so the binary is
/// self-contained. Order is registration order only; face selection
/// within the family stays GPUI's (weight resolution against the
/// registered set).
pub(crate) fn bundled_face_bytes() -> Vec<Cow<'static, [u8]>> {
    named_face_bytes()
        .into_iter()
        .map(|(_, bytes)| bytes)
        .collect()
}

/// The same streams with their file names: the digest record
/// (`res/fonts/FACES.sha256`) keys on the names, and the integrity
/// test pairs each embedded stream with its recorded line.
fn named_face_bytes() -> [(&'static str, Cow<'static, [u8]>); 2] {
    [
        (
            "NotoSansMono-Regular.ttf",
            Cow::Borrowed(include_bytes!("../../res/fonts/NotoSansMono-Regular.ttf").as_slice()),
        ),
        (
            "NotoSansMono-Bold.ttf",
            Cow::Borrowed(include_bytes!("../../res/fonts/NotoSansMono-Bold.ttf").as_slice()),
        ),
    ]
}

/// Register the bundled faces into GPUI's text system. Called once at
/// the run closure's top — before any pane resolves — so every boot
/// mode (the app, the kit check, the terminal walk) carries the
/// family. A failed registration is logged, not fatal: the resolver
/// then behaves exactly as before the bundle (system fonts only, the
/// pre-task walk), which is degradation to today's behavior, not a
/// new failure mode.
pub(crate) fn register_bundled_fonts(cx: &mut gpui::App) {
    let faces = bundled_face_bytes();
    if let Err(err) = cx.text_system().add_fonts(faces) {
        println!("dropterm: bundled font registration failed ({err}); system fonts only");
    } else {
        println!("dropterm: bundled default family {BUNDLED_FAMILY:?} registered");
    }
}

/// The visible row for glyphs no registered font provides: code
/// points spelled out (`U+E0B0`), stable and identifiable regardless
/// of the logging terminal's own font — a raw cluster prints as tofu
/// or blank space exactly where the terminal is missing the face,
/// which is the state being reported (the bundle's measured coverage
/// pins what the family carries — powerline, braille, emoji, and CJK
/// do not ride it, and this log is where those misses surface).
pub(crate) fn missing_glyph_log_line(missing: &[gpui::MissingGlyph]) -> String {
    use std::fmt::Write as _;
    let mut clusters = Vec::with_capacity(missing.len());
    for g in missing {
        let mut cluster = String::new();
        for c in g.grapheme().chars() {
            if !cluster.is_empty() {
                cluster.push('+');
            }
            let _ = write!(cluster, "U+{:04X}", c as u32);
        }
        clusters.push(cluster);
    }
    format!(
        "dropterm: missing glyphs, no registered font provides them: {}",
        clusters.join(" ")
    )
}

/// Attach the missing-glyph log at boot. The returned subscription
/// must live as long as the app — dropping it disables the reporting
/// (gpui's own contract) — so the boot forgets it deliberately: the
/// log is a boot property, never unloadable.
pub(crate) fn wire_missing_glyph_log(cx: &mut gpui::App) -> gpui::Subscription {
    cx.on_missing_glyphs(|missing, _cx| {
        println!("{}", missing_glyph_log_line(missing));
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dropterm::core::config::DropTermConfig;

    // The registration call itself is not headless-testable: GPUI's
    // test platform ships a `NoopTextSystem` (platform.rs) whose
    // `add_fonts` no-ops and whose `all_font_names` returns empty, so
    // a family-presence assertion there would test the fixture, not
    // the seam. The registration's effect is owner-walk territory —
    // the bare-rig row of this task — and the walk-level guarantee it
    // enables is pinned in terminal::host's resolver tests (the
    // bundled family as the terminal fallback of the candidate walk).

    #[test]
    fn the_bundled_faces_are_complete_sfnt_streams() {
        // Every face is a full TrueType sfnt — the magic word at
        // offset zero — catching a truncated or mis-provenanced asset
        // at test time rather than as silent glyph loss on the bare
        // rig, the failure mode this module exists to prevent.
        let faces = bundled_face_bytes();
        assert_eq!(faces.len(), 2, "Regular and Bold carry the default");
        for bytes in faces {
            assert!(bytes.len() > 64, "face implausibly short");
            assert_eq!(
                &bytes[..4],
                b"\x00\x01\x00\x00",
                "not a TrueType sfnt stream"
            );
        }
    }

    #[test]
    fn the_bundled_family_is_the_configured_default() {
        // The pin that makes the task cohere: config default,
        // resolver fallback, and registered family are one name, so
        // "default font" cannot drift away from "bundled font".
        assert_eq!(DropTermConfig::default().font_name, BUNDLED_FAMILY);
    }

    #[test]
    fn the_embedded_faces_match_the_recorded_digests() {
        // The integrity pin: every embedded stream re-hashes to its
        // recorded line, so a substituted or re-encoded asset — a
        // re-download of a different monthly, a tooling mangle —
        // fails the suite instead of shipping as silent glyph loss.
        // The record lives with the faces it pins; the recorded
        // acquisition digests match the Google Fonts-served v37
        // monthly (the chunk's task note carries the provenance).
        use sha2::{Digest, Sha256};
        fn hex(bytes: &[u8]) -> String {
            use std::fmt::Write as _;
            let mut out = String::with_capacity(bytes.len() * 2);
            for b in bytes {
                let _ = write!(out, "{b:02x}");
            }
            out
        }
        let recorded = include_str!("../../res/fonts/FACES.sha256");
        let bundled = named_face_bytes();
        for (file, bytes) in &bundled {
            let digest = hex(&Sha256::digest(bytes));
            let line = format!("{digest}  res/fonts/{file}");
            assert!(
                recorded.lines().any(|l| l == line),
                "the digest record must carry {file} at {digest}"
            );
        }
        // Every recorded line pins a bundled face: a face removed
        // without retiring its record line fails here too, and the
        // record cannot grow stale rows nobody verifies.
        assert_eq!(recorded.lines().count(), bundled.len());
    }

    #[gpui::test]
    fn the_missing_glyph_log_wires_headless(cx: &mut gpui::TestAppContext) {
        // The test platform's text system does no shaping
        // (NoopTextSystem), so no report ever fires here — this pins
        // the wiring only: the log attaches, the subscription is
        // handed back, and the boot's ownership decision (forget it —
        // the log is a boot property, never unloadable) is the one
        // thing a walk must confirm on real hardware.
        cx.update(|cx| {
            let subscription = wire_missing_glyph_log(cx);
            std::mem::forget(subscription);
        });
    }

    #[test]
    fn the_missing_glyph_log_names_the_unresolved_graphemes() {
        // The visible row: the log names the exact grapheme clusters
        // that exhausted every registered font. The Debug escaping is
        // deliberate — an invisible grapheme (a braille blank, say)
        // logs as its escape, still identifiable in a walk, where the
        // raw character would print as blank space.
        let line = missing_glyph_log_line(&[
            gpui::MissingGlyph::new("\u{e0b0}".into(), gpui::FallbackFontClass::Monospace),
            gpui::MissingGlyph::new("\u{2800}".into(), gpui::FallbackFontClass::Monospace),
        ]);
        assert!(
            line.contains("missing glyphs"),
            "the row says what happened"
        );
        assert!(
            line.contains("U+E0B0") && line.contains("U+2800"),
            "the row names the code points, stable and identifiable regardless of the logging terminal's own font"
        );
    }

    #[test]
    fn the_ofl_license_is_carried_with_the_faces() {
        // The OFL's obligations the bundle inherits: the license
        // rides the faces, and the reserved-name clause stands —
        // these are unmodified bytes, and a modified build must
        // rename the family (the pin keeps that rule visible where
        // the faces live).
        let license = include_str!("../../res/fonts/OFL.txt");
        assert!(
            license.contains("SIL OPEN FONT LICENSE Version 1.1"),
            "the carried license is the SIL OFL"
        );
        assert!(
            license.contains("Reserved Font Name"),
            "the reserved-name clause rides the carried license"
        );
    }
}
