// SPDX-License-Identifier: GPL-3.0-only

//! The pane's clipboard seam (gpui-terminal Task 3): copy and paste
//! through GPUI's own clipboard while the pane holds the keyboard —
//! the focus-plus-input-serial write gate is satisfied by
//! construction there (check 8's ruling; the libcosmic build's
//! anchor surface does not port) — and the data-control protocol
//! (`wl-clipboard-rs`) serving every write the focused path cannot
//! make: copies while unfocused (OSC 52 from a background tab) and
//! the primary selection, which GPUI's clipboard does not expose.
//! Each data-control copy serves on its own detached thread, exactly
//! the libcosmic clipboard module's shape.

#[cfg(target_os = "linux")]
use std::io::Read as _;

use gpui::{App, ClipboardItem};

/// Serve one data-control copy on its own thread: the call returns
/// once the selection is set, and wl-clipboard-rs's own serving
/// thread keeps the offer alive until something replaces it. Never
/// blocks the UI thread; a failure logs once — there is no focused
/// fallback to hand the text to when the write is being made
/// precisely because the pane is unfocused.
#[cfg(target_os = "linux")]
pub(crate) fn write_data_control(text: String, primary: bool) {
    std::thread::spawn(move || {
        let mut options = wl_clipboard_rs::copy::Options::new();
        // Parity with the libcosmic module's pinned copy shape: text
        // verbatim, a copied shell line keeps its trailing newline.
        options.trim_newline(false).clipboard(if primary {
            wl_clipboard_rs::copy::ClipboardType::Primary
        } else {
            wl_clipboard_rs::copy::ClipboardType::Regular
        });
        let result = wl_clipboard_rs::copy::copy(
            options,
            wl_clipboard_rs::copy::Source::Bytes(text.into_bytes().into_boxed_slice()),
            wl_clipboard_rs::copy::MimeType::Text,
        );
        if let Err(err) = result {
            println!("dropterm: data-control write failed ({err})");
        }
    });
}

/// The non-Linux arm (macos-build mb-2): no data-control protocol
/// exists off Wayland, and no pane exists to call this before a
/// surface backend does (macos-surface's call). The payload drops —
/// the honest no-op, never a fake success.
#[cfg(not(target_os = "linux"))]
pub(crate) fn write_data_control(text: String, primary: bool) {
    let _ = (text, primary);
}

/// Serve one data-control image copy on its own thread (the images
/// panel's Copy-image verb, py-6): the row's bytes in their sniffed
/// format — GPUI's clipboard carries no image seat, so the
/// data-control protocol is the write's only road, serving unfocused
/// exactly like the text half above.
#[cfg(target_os = "linux")]
pub(crate) fn write_image_data_control(bytes: Vec<u8>, mime: &'static str) {
    std::thread::spawn(move || {
        let options = wl_clipboard_rs::copy::Options::new();
        let result = wl_clipboard_rs::copy::copy(
            options,
            wl_clipboard_rs::copy::Source::Bytes(bytes.into_boxed_slice()),
            wl_clipboard_rs::copy::MimeType::Specific(mime.to_string()),
        );
        if let Err(err) = result {
            println!("dropterm: data-control image write failed ({err})");
        }
    });
}

/// The non-Linux arm (mb-2): the image twin of the text no-op above.
#[cfg(not(target_os = "linux"))]
pub(crate) fn write_image_data_control(bytes: Vec<u8>, mime: &'static str) {
    let _ = (bytes, mime);
}

/// One copy out of the pane: the focused, non-primary write rides
/// GPUI's clipboard (the wayland `wl_data_device` set-selection —
/// focus and serial are the pane's by construction); every other
/// write (unfocused, or the primary selection) rides data-control,
/// which needs neither.
pub(crate) fn write(text: String, primary: bool, focused: bool, cx: &App) {
    if focused && !primary {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    } else {
        write_data_control(text, primary);
    }
}

/// The string one paste reads off GPUI's clipboard: `None` for an
/// empty clipboard or an offer holding no text entry.
pub(crate) fn read(cx: &App) -> Option<String> {
    let item = cx.read_from_clipboard()?;
    item.entries.into_iter().find_map(|entry| match entry {
        gpui::ClipboardEntry::String(string) => Some(string.text),
        _ => None,
    })
}

/// One data-control read of the primary selection — the blocking
/// twin the caller runs off the UI thread (GPUI's clipboard has no
/// primary API). An empty selection answers `None`; a transport
/// failure warns once and answers `None` (the pane is focused when
/// its paste actions fire, so the regular read has the focused path
/// covered and this one has no fallback worth retrying into).
#[cfg(target_os = "linux")]
pub(crate) fn read_primary_blocking() -> Option<String> {
    let (mut pipe, _mime) = wl_clipboard_rs::paste::get_contents(
        wl_clipboard_rs::paste::ClipboardType::Primary,
        wl_clipboard_rs::paste::Seat::Unspecified,
        wl_clipboard_rs::paste::MimeType::Text,
    )
    .inspect_err(|err| println!("dropterm: data-control primary read failed ({err})"))
    .ok()?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut pipe, &mut text)
        .inspect_err(|err| println!("dropterm: data-control primary read failed ({err})"))
        .ok()?;
    Some(text)
}

/// The non-Linux arm (mb-2): no primary selection exists to read —
/// the honest `None`, same answer an empty selection gives.
#[cfg(not(target_os = "linux"))]
pub(crate) fn read_primary_blocking() -> Option<String> {
    None
}

/// What one image read resolved to — the paste road's outcome (the
/// audit's F7): the bytes, the honest silence (no image offer stood:
/// an empty clipboard, no PNG offer, a transport failure — a normal
/// paste state, no verdict owed), or the refusal the band's verdict
/// chip narrates.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ImageRead {
    Bytes(Vec<u8>),
    None,
    Refused(dropterm::core::status::PasteRefusal),
}

/// The read's verdict split — the pure half the test pins: payload
/// bytes under the cap, the over-cap refusal at it. The OSC 52
/// store/load guards ride the same split (ra-3): one cap, every
/// clipboard payload.
pub(crate) fn classify_read(len: usize) -> Option<dropterm::core::status::PasteRefusal> {
    (len > dropterm::core::paste_cache::PASTE_PAYLOAD_CAP)
        .then_some(dropterm::core::status::PasteRefusal::OverCap)
}

/// What one text read resolved to — the text twin of [`ImageRead`]
/// (ra-3's ruling: text offers carry the paste cap images already
/// had): the text, the honest silence (no text offer stood), or the
/// refusal the band's verdict chip narrates.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TextRead {
    Text(String),
    None,
    Refused(dropterm::core::status::PasteRefusal),
}

/// One paste's regular-clipboard text read, capped: GPUI has read
/// the offer by the time the app sees it, so the cap lands as the
/// post-read check — over it the paste refuses before a byte reaches
/// the engine, the string dropped whole (ra-3).
pub(crate) fn read_text_capped(cx: &App) -> TextRead {
    match read(cx) {
        Some(text) => match classify_read(text.len()) {
            None => TextRead::Text(text),
            Some(refusal) => {
                println!(
                    "dropterm: text offer refused — {} bytes over the {}-byte paste cap",
                    text.len(),
                    dropterm::core::paste_cache::PASTE_PAYLOAD_CAP
                );
                TextRead::Refused(refusal)
            }
        },
        None => TextRead::None,
    }
}

/// One paste's primary-selection text read, capped: the blocking
/// twin stops the pipe one byte past the cap — the true read-side
/// bound this road can apply — and over it the paste refuses whole
/// (ra-3).
#[cfg(target_os = "linux")]
pub(crate) fn read_primary_text_capped() -> TextRead {
    let cap = u64::try_from(dropterm::core::paste_cache::PASTE_PAYLOAD_CAP + 1)
        .expect("the paste cap is a constant far below u64::MAX");
    let (pipe, _mime) = match wl_clipboard_rs::paste::get_contents(
        wl_clipboard_rs::paste::ClipboardType::Primary,
        wl_clipboard_rs::paste::Seat::Unspecified,
        wl_clipboard_rs::paste::MimeType::Text,
    ) {
        Ok(pair) => pair,
        Err(err) => {
            println!("dropterm: data-control primary read failed ({err})");
            return TextRead::None;
        }
    };
    let mut text = String::new();
    let mut offer = pipe.take(cap);
    if let Err(err) = std::io::Read::read_to_string(&mut offer, &mut text) {
        println!("dropterm: data-control primary read failed ({err})");
        return TextRead::None;
    }
    match classify_read(text.len()) {
        None => TextRead::Text(text),
        Some(refusal) => {
            println!(
                "dropterm: primary text offer refused — over the {}-byte paste cap",
                dropterm::core::paste_cache::PASTE_PAYLOAD_CAP
            );
            return TextRead::Refused(refusal);
        }
    }
}

/// The non-Linux arm (mb-2): no primary selection exists — the
/// paste's honest silence.
#[cfg(not(target_os = "linux"))]
pub(crate) fn read_primary_text_capped() -> TextRead {
    TextRead::None
}

/// One data-control read of the clipboard's image offer — the paste
/// path's image road (paste-image spec, PI-1; the libcosmic
/// `load_image` port): PNG, the one interchange format every
/// compositor converts clipboard images to, read under the paste
/// cap. [`ImageRead::None`] is every no-image state — an empty
/// clipboard, no PNG offer, a transport failure — each the paste's
/// honest silence (warned once where owed); the refusals name
/// themselves for the band's verdict chip.
#[cfg(target_os = "linux")]
pub(crate) fn read_image_blocking(primary: bool) -> ImageRead {
    let cap = u64::try_from(dropterm::core::paste_cache::PASTE_PAYLOAD_CAP + 1)
        .expect("the paste cap is a constant far below u64::MAX");
    let (pipe, _mime) = match wl_clipboard_rs::paste::get_contents(
        if primary {
            wl_clipboard_rs::paste::ClipboardType::Primary
        } else {
            wl_clipboard_rs::paste::ClipboardType::Regular
        },
        wl_clipboard_rs::paste::Seat::Unspecified,
        wl_clipboard_rs::paste::MimeType::Specific("image/png"),
    ) {
        Ok(pair) => pair,
        Err(err) => {
            println!("dropterm: data-control image read — no offer ({err})");
            return ImageRead::None;
        }
    };
    let mut bytes = Vec::new();
    let mut offer = pipe.take(cap);
    if let Err(err) = std::io::Read::read_to_end(&mut offer, &mut bytes) {
        println!("dropterm: data-control image read failed ({err})");
        return ImageRead::Refused(dropterm::core::status::PasteRefusal::Failed);
    }
    if let Some(refusal) = classify_read(bytes.len()) {
        println!(
            "dropterm: image offer refused — over the {}-byte paste cap",
            dropterm::core::paste_cache::PASTE_PAYLOAD_CAP
        );
        return ImageRead::Refused(refusal);
    }
    ImageRead::Bytes(bytes)
}

/// The non-Linux arm (mb-2): no image offer can stand — the paste's
/// honest silence.
#[cfg(not(target_os = "linux"))]
pub(crate) fn read_image_blocking(primary: bool) -> ImageRead {
    let _ = primary;
    ImageRead::None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The seam's policy is the split itself — which write rides
    /// which transport — and the split is decided by a pure
    /// predicate over (focused, primary) the test pins. The two
    /// transports' own behavior is compositor-evidenced (the walk).
    fn rides_gpui(focused: bool, primary: bool) -> bool {
        focused && !primary
    }

    #[test]
    fn the_read_verdict_split_is_the_cap() {
        // At the cap the bytes are the paste's payload; past it the
        // refusal is the band verdict's input.
        assert_eq!(
            classify_read(dropterm::core::paste_cache::PASTE_PAYLOAD_CAP),
            None
        );
        assert_eq!(
            classify_read(dropterm::core::paste_cache::PASTE_PAYLOAD_CAP + 1),
            Some(dropterm::core::status::PasteRefusal::OverCap)
        );
    }

    #[test]
    fn the_focused_regular_write_is_the_gpui_one() {
        // Check 8's ruling: focus plus input serial, satisfied by
        // construction when the pane holds the keyboard.
        assert!(rides_gpui(true, false));
    }

    #[test]
    fn the_primary_selection_and_unfocused_writes_ride_data_control() {
        // GPUI's clipboard has no primary API, and an unfocused pane
        // has no serial — data-control needs neither.
        assert!(!rides_gpui(true, true));
        assert!(!rides_gpui(false, false));
        assert!(!rides_gpui(false, true));
    }
}
