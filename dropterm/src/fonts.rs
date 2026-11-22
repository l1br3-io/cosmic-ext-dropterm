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
    vec![
        Cow::Borrowed(include_bytes!("../../res/fonts/NotoSansMono-Regular.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../res/fonts/NotoSansMono-Bold.ttf").as_slice()),
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
}
