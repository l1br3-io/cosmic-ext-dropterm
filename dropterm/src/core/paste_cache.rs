// SPDX-License-Identifier: GPL-3.0-only

//! The paste-image cache (paste-image spec, PI-1; moved whole from
//! the libcosmic clipboard module at gpui-parity Task 2 so both
//! frontends share one write path — the exclusive-create defense
//! and the cap exist once): pasted image bytes land in
//! `dropterm/paste` under the XDG cache dir, and the file's path is
//! the paste's one grid trace — the same text channel a text paste
//! uses, so bracketed paste rides unchanged. The name is generated
//! — timestamp and collision counter — never derived from the bytes.
//!
//! The four-format sniff ([`sniff_image_format`]) is the pipeline's
//! one content gate: the first bytes decide, never a name, an
//! extension, or an offer's say-so.

use std::collections::HashSet;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::PathBuf;

/// The ceiling one clipboard offer may carry (the ship audit's
/// decompression-bomb guard for the paste path): reads stop one byte
/// past it — an offer over it is refused whole, and the paste cache
/// writes nothing over it. The engine's `IMAGE_PAYLOAD_CAP`
/// precedent: the caps protect the pipeline, never mangle the
/// payload.
pub const PASTE_PAYLOAD_CAP: usize = 32 * 1024 * 1024;

/// The four decodable image formats the paste pipeline accepts
/// (paste-uri spec, PU-3): the sniff's answer, carrying the cache
/// extension (PU-4) and the clipboard mime (PU-5) so every consumer
/// reads one classification instead of re-deriving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SniffedFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl SniffedFormat {
    /// The cache filename extension (PU-4): `jpg`, not `jpeg` — the
    /// conventional three-letter spelling.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }

    /// The clipboard mime the format offers and accepts (PU-5).
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }
}

/// The paste pipeline's one content gate (paste-uri spec, PU-3): the
/// first bytes decide — PNG, JPEG, GIF, WebP magic — never a name, an
/// extension, or an offer's say-so. Hand-rolled prefix compares: the
/// format set is small and fixed, no dependency is owed it.
pub fn sniff_image_format(bytes: &[u8]) -> Option<SniffedFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(SniffedFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(SniffedFormat::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(SniffedFormat::Gif)
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(SniffedFormat::Webp)
    } else {
        None
    }
}

/// The paste cache's directory: `dropterm/paste` under the XDG cache
/// dir (paste-image spec). `None` when no cache dir resolves.
pub fn paste_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("dropterm").join("paste"))
}

/// The paste filename's timestamp: `YYYYMMDD-HHMMSS`, local zone (the
/// band clock's `strftime` precedent).
pub(crate) fn paste_stamp(now: &jiff::Zoned) -> String {
    now.strftime("%Y%m%d-%H%M%S").to_string()
}

/// The next paste filename for `stamp`, its counter stepping past
/// every same-stamp name the directory already holds — same-second
/// pastes never collide. The candidate carries `extension` whole
/// (PU-4): names under different extensions are separate slots, with
/// `create_new` still the arbiter on the open.
pub(crate) fn next_paste_name(existing: &HashSet<String>, stamp: &str, extension: &str) -> String {
    let mut counter = 1;
    loop {
        let name = format!("paste-{stamp}-{counter}.{extension}");
        if !existing.contains(&name) {
            return name;
        }
        counter += 1;
    }
}

/// Write pasted image bytes to the paste cache and answer the path
/// that pastes as text (paste-image spec, PI-1). The name is
/// generated — timestamp and collision counter — never derived from
/// the bytes. Best effort: one warn naming the operation and no paste
/// on any failure — including a payload over the paste cap, refused
/// before the cache is touched.
pub fn write_paste_image(bytes: &[u8]) -> Option<String> {
    let Some(dir) = paste_cache_dir() else {
        log::warn!("paste cache unavailable: no XDG cache directory resolves");
        return None;
    };
    write_paste_image_in(&dir, &paste_stamp(&jiff::Zoned::now()), bytes)
}

/// The cache write's testable core (the ship audit's exclusive-write
/// fix): the name comes from the same counter walk, but `create_new`
/// is the arbiter — the file is created exclusively, so a symlink
/// planted at a candidate name is stepped past rather than followed,
/// a same-instant writer in another instance collides on the open
/// instead of clobbering, and the `0600` mode keeps clipboard
/// screenshots the owner's alone under any umask. An unreadable
/// directory is not a stop: the write itself names the real failure
/// if it persists.
pub(crate) fn write_paste_image_in(
    dir: &std::path::Path,
    stamp: &str,
    bytes: &[u8],
) -> Option<String> {
    if bytes.len() > PASTE_PAYLOAD_CAP {
        log::warn!("paste image refused: payload over the {PASTE_PAYLOAD_CAP}-byte paste cap");
        return None;
    }
    if let Err(err) = std::fs::create_dir_all(dir) {
        log::warn!("failed to create the paste cache {}: {err}", dir.display());
        return None;
    }
    // PU-4: the cache name's extension follows the bytes' own sniffed
    // format; an unknown sniff keeps the clipboard road's
    // by-construction `.png`.
    let extension = sniff_image_format(bytes).map_or("png", SniffedFormat::extension);
    let mut existing: HashSet<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    loop {
        let name = next_paste_name(&existing, stamp, extension);
        let path = dir.join(&name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(mut file) => {
                if let Err(err) = std::io::Write::write_all(&mut file, bytes) {
                    log::warn!("failed to write {}: {err}", path.display());
                    return None;
                }
                return Some(path.display().to_string());
            }
            // The name was taken between the snapshot and the open
            // (or the snapshot never could read it): the set grows
            // with every observed name, so the walk always advances.
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                existing.insert(name);
            }
            Err(err) => {
                log::warn!("failed to write {}: {err}", path.display());
                return None;
            }
        }
    }
}

// The exclusive-create `mode` is unix-only; the write itself is std.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sniff_passes_each_formats_magic() {
        assert_eq!(
            sniff_image_format(b"\x89PNG\r\n\x1a\n\x00\x00"),
            Some(SniffedFormat::Png)
        );
        assert_eq!(
            sniff_image_format(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(SniffedFormat::Jpeg)
        );
        assert_eq!(sniff_image_format(b"GIF89a"), Some(SniffedFormat::Gif));
        assert_eq!(sniff_image_format(b"GIF87a"), Some(SniffedFormat::Gif));
        // WebP is a RIFF container: `RIFF` then the chunk `WEBP` at
        // offset 8 — a bare RIFF header (a WAV file) is not WebP.
        let webp = b"RIFF\x24\x00\x00\x00WEBPVP8 ";
        assert_eq!(sniff_image_format(webp), Some(SniffedFormat::Webp));
    }

    #[test]
    fn the_sniff_refuses_foreign_truncated_and_empty_bytes() {
        // TIFF (either byte order), text, a truncated PNG header, a
        // bare RIFF container, and an empty buffer never pass: the
        // content is the gate — a sniffed-format suffix on a
        // non-image body has no say.
        assert_eq!(sniff_image_format(b"II*\x00\x08\x00\x00\x00"), None);
        assert_eq!(sniff_image_format(b"MM\x00*\x00\x08"), None);
        assert_eq!(sniff_image_format(b"hello, I am a text file"), None);
        assert_eq!(sniff_image_format(&[0x89, b'P', b'N', b'G']), None);
        assert_eq!(sniff_image_format(b"RIFF\x24\x00\x00\x00WAVEfmt "), None);
        assert_eq!(sniff_image_format(b""), None);
    }

    #[test]
    fn the_sniffed_formats_carry_their_extensions_and_mimes() {
        assert_eq!(SniffedFormat::Png.extension(), "png");
        assert_eq!(SniffedFormat::Png.mime(), "image/png");
        assert_eq!(SniffedFormat::Jpeg.extension(), "jpg");
        assert_eq!(SniffedFormat::Jpeg.mime(), "image/jpeg");
        assert_eq!(SniffedFormat::Gif.extension(), "gif");
        assert_eq!(SniffedFormat::Gif.mime(), "image/gif");
        assert_eq!(SniffedFormat::Webp.extension(), "webp");
        assert_eq!(SniffedFormat::Webp.mime(), "image/webp");
    }

    #[test]
    fn paste_stamp_pins_the_filename_timestamp_format() {
        assert_eq!(
            paste_stamp(
                &"2026-11-08T14:03:09+00:00[UTC]"
                    .parse()
                    .expect("fixed zoned fixture")
            ),
            "20261108-140309"
        );
    }

    #[test]
    fn the_first_paste_of_a_second_takes_counter_one() {
        let existing = HashSet::new();
        assert_eq!(
            next_paste_name(&existing, "20261108-140309", "png"),
            "paste-20261108-140309-1.png"
        );
    }

    #[test]
    fn same_second_pastes_step_the_counter_past_existing_names() {
        // Two pastes inside one second must not collide: the counter
        // steps past every same-stamp name the directory already
        // holds.
        let existing: HashSet<String> =
            ["paste-20261108-140309-1.png", "paste-20261108-140309-2.png"]
                .into_iter()
                .map(str::to_owned)
                .collect();
        assert_eq!(
            next_paste_name(&existing, "20261108-140309", "png"),
            "paste-20261108-140309-3.png"
        );
    }

    #[test]
    fn the_counter_walk_is_unchanged_across_extensions() {
        // The walk checks the candidate name whole — extension
        // included — so a same-stamp name with a different extension
        // never blocks a counter this format's own name doesn't hold.
        let png_taken: HashSet<String> = ["paste-20261108-140309-1.png".to_owned()]
            .into_iter()
            .collect();
        assert_eq!(
            next_paste_name(&png_taken, "20261108-140309", "jpg"),
            "paste-20261108-140309-1.jpg"
        );
        let jpg_taken: HashSet<String> = ["paste-20261108-140309-1.jpg".to_owned()]
            .into_iter()
            .collect();
        assert_eq!(
            next_paste_name(&jpg_taken, "20261108-140309", "jpg"),
            "paste-20261108-140309-2.jpg"
        );
    }

    // --- the cache write (the ship audit's exclusive-write fix) ---

    /// A fresh temp dir per test, cleared first so re-runs start
    /// clean (the tabs suite's own fixture convention).
    fn cache_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dropterm-paste-cache-tests-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_first_cache_write_takes_counter_one_at_owner_only_perms() {
        use std::os::unix::fs::PermissionsExt;

        let dir = cache_dir("first");
        let path = write_paste_image_in(&dir, "20261108-140309", b"png").expect("the write lands");
        assert!(std::path::Path::new(&path).ends_with("paste-20261108-140309-1.png"));
        // Clipboard screenshots are the owner's alone: 0600 under any
        // umask, not the 0644/0666 a plain write inherits.
        let mode = std::fs::metadata(&path)
            .expect("the file stands")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_jpeg_payload_caches_as_jpg_at_owner_only_perms() {
        // PU-4: the cache name follows the bytes' own sniffed format —
        // a JPEG's magic names a .jpg, at the same 0600 the PNG road
        // writes.
        use std::os::unix::fs::PermissionsExt;

        let dir = cache_dir("jpeg-name");
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        let path = write_paste_image_in(&dir, "20261108-140309", &jpeg).expect("the write lands");
        assert!(std::path::Path::new(&path).ends_with("paste-20261108-140309-1.jpg"));
        let mode = std::fs::metadata(&path)
            .expect("the file stands")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_same_stamp_jpeg_takes_the_free_jpg_counter_one() {
        // A PNG already holding counter one does not block the JPEG's
        // own counter-one name: the extensions are separate namespaces.
        let dir = cache_dir("mixed-ext");
        std::fs::create_dir_all(&dir).expect("fixture dir");
        std::fs::write(dir.join("paste-20261108-140309-1.png"), b"png").expect("fixture file");
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        let path = write_paste_image_in(&dir, "20261108-140309", &jpeg).expect("the write lands");
        assert!(std::path::Path::new(&path).ends_with("paste-20261108-140309-1.jpg"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_taken_name_steps_and_the_taker_is_never_clobbered() {
        // A file already holding the counter-one name is left
        // byte-for-byte alone: the write takes counter two.
        let dir = cache_dir("collision");
        let taken = dir.join("paste-20261108-140309-1.png");
        std::fs::create_dir_all(&dir).expect("fixture dir");
        std::fs::write(&taken, b"sentinel").expect("fixture file");
        let path =
            write_paste_image_in(&dir, "20261108-140309", b"fresh").expect("the write lands");
        assert!(std::path::Path::new(&path).ends_with("paste-20261108-140309-2.png"));
        assert_eq!(std::fs::read(&taken).expect("sentinel stands"), b"sentinel");
        assert_eq!(std::fs::read(&path).expect("fresh stands"), b"fresh");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_planted_symlink_is_stepped_past_never_followed() {
        // A symlink at a candidate name is a taken name to the
        // exclusive open, not a door to the victim behind it.
        let dir = cache_dir("symlink");
        std::fs::create_dir_all(&dir).expect("fixture dir");
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, b"victim").expect("fixture file");
        std::os::unix::fs::symlink(&victim, dir.join("paste-20261108-140309-1.png"))
            .expect("fixture link");
        let path =
            write_paste_image_in(&dir, "20261108-140309", b"fresh").expect("the write lands");
        assert!(std::path::Path::new(&path).ends_with("paste-20261108-140309-2.png"));
        assert_eq!(std::fs::read(&victim).expect("victim stands"), b"victim");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_over_cap_payload_touches_nothing() {
        // The refusal happens before the cache is even created: no
        // directory, no file, one warn.
        let dir = cache_dir("cap");
        let oversize = vec![0u8; PASTE_PAYLOAD_CAP + 1];
        assert!(write_paste_image_in(&dir, "20261108-140309", &oversize).is_none());
        assert!(!dir.exists());
    }
}
