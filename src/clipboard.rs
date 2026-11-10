//! The terminal's clipboard I/O, owned end to end.
//!
//! Every copy and paste the app performs routes through this module
//! (clipboard-ownership spec). The chain it replaces —
//! `cosmic::iced::clipboard` → iced's winit runtime →
//! `window_clipboard` → smithay-clipboard — sets selections through
//! the legacy `wl_data_device`, which requires the client to hold
//! keyboard focus and a valid input serial at the instant of the
//! copy. A drop-down terminal lives outside that contract: surfaces
//! are destroyed and recreated on every hide/show, background shows
//! map without focus, and `hide_on_focus_loss` churns focus
//! constantly. The result was a copy that silently vanished — no
//! error, no log, at any layer.
//!
//! This module speaks the data-control protocol instead
//! (`wl-clipboard-rs`): focus-free and serial-free on this
//! compositor, verified against cosmic-comp in both directions.
//! A data-control failure is never silent — it warns once and falls
//! back to the iced clipboard for that single operation.

use std::collections::HashSet;
use std::io::Read as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::PathBuf;

use cosmic::iced::Task;
use cosmic::iced::clipboard as iced_clipboard;
use wl_clipboard_rs::copy::{ClipboardType, MimeType, Options, Source};
use wl_clipboard_rs::paste::{self, MimeType as PasteMime, Seat};

/// A data-control operation's failure, kept as one type so the warn
/// line and the fallback decision share a single classification.
enum DataControlError {
    /// The copy path failed.
    Copy(wl_clipboard_rs::copy::Error),
    /// The paste path failed.
    Paste(paste::Error),
    /// The blocking operation never completed (the join failed).
    Join(tokio::task::JoinError),
    /// Reading the offered content off the pipe failed.
    Io(std::io::Error),
    /// The offer carried more than the paste cap allows — a policy
    /// refusal, not a transport failure: the iced fallback would read
    /// the same oversized offer unbounded, so it is never retried.
    OfferTooLarge,
}

impl From<paste::Error> for DataControlError {
    fn from(e: paste::Error) -> Self {
        Self::Paste(e)
    }
}

impl From<std::io::Error> for DataControlError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl std::fmt::Display for DataControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Copy(e) => write!(f, "copy: {e}"),
            Self::Paste(e) => write!(f, "paste: {e}"),
            Self::Join(e) => write!(f, "join: {e}"),
            Self::Io(e) => write!(f, "io: {e}"),
            Self::OfferTooLarge => write!(f, "offer over the paste cap"),
        }
    }
}

/// How a failed read resolves. Writes have no `Empty` arm: a store
/// either lands or the operation falls back.
enum ReadFailure {
    /// An empty (or typeless) clipboard is a state, not a failure —
    /// the read answers `None`, silently.
    Empty,
    /// A transport or support failure — warn, then retry the
    /// operation through the iced clipboard once.
    Fallback,
    /// An offer over the paste cap — warn once and answer nothing:
    /// the payload is refused whole, never truncated (a truncated
    /// paste is a corrupt one) and never retried through the
    /// unbounded fallback.
    Refuse,
}

fn classify_read_failure(err: &DataControlError) -> ReadFailure {
    match err {
        DataControlError::Paste(paste::Error::ClipboardEmpty | paste::Error::NoMimeType) => {
            ReadFailure::Empty
        }
        DataControlError::OfferTooLarge => ReadFailure::Refuse,
        _ => ReadFailure::Fallback,
    }
}

/// The paste-image gate's engine-true floor (paste-image spec, PI-1):
/// on until `paste_image_enabled` becomes a config key (PI-3), on the
/// `autosuggestions_commands_enabled` engine-true precedent.
const PASTE_IMAGE_ENABLED: bool = true;

/// The one image interchange format v1 reads and writes
/// (paste-image OQ-3): PNG — every compositor converts clipboard
/// images to it. One constant for both directions, so what a
/// copy-image writes, a paste reads back.
const IMAGE_PNG_MIME: &str = "image/png";

/// The mime the first paste read prioritizes (paste-uri spec, PU-1):
/// a file-manager copy or a portal screenshot lands a `text/uri-list`
/// offer, and the read must ask for it before text — the plain text
/// read would serve the offer set's text/plain and paste a URI
/// verbatim.
const URI_LIST_MIME: &str = "text/uri-list";

/// The ceiling one clipboard offer may carry (the ship audit's
/// decompression-bomb guard for the paste path): reads stop one byte
/// past it — an offer over it is refused whole, and the paste cache
/// writes nothing over it. The engine's `IMAGE_PAYLOAD_CAP`
/// precedent: the caps protect the pipeline, never mangle the
/// payload.
pub(crate) const PASTE_PAYLOAD_CAP: usize = 32 * 1024 * 1024;

/// The four decodable image formats the paste pipeline accepts
/// (paste-uri spec, PU-3): the sniff's answer, carrying the cache
/// extension (PU-4) and the clipboard mime (PU-5) so every consumer
/// reads one classification instead of re-deriving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SniffedFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl SniffedFormat {
    /// The cache filename extension (PU-4): `jpg`, not `jpeg` — the
    /// conventional three-letter spelling.
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }

    /// The clipboard mime the format offers and accepts (PU-5).
    fn mime(self) -> &'static str {
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
fn sniff_image_format(bytes: &[u8]) -> Option<SniffedFormat> {
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

/// The string half of the file gate (paste-uri spec, PU-2): turn a
/// served `text/uri-list` into the one local file the paste may read.
/// RFC 2483 shape — lines, CRLF tolerated, `#` comment lines skipped
/// — and it qualifies only when the remaining URIs are exactly one,
/// a `file` scheme on an empty-or-`localhost` host, whose path
/// percent-decodes. `Some` fires the file read; `None` is the text
/// fallback the offer set owes the user (today's paste, verbatim).
/// Pure: no I/O, fully pinnable headless.
fn qualify_uri_list(list: &str) -> Option<PathBuf> {
    let mut uris = list
        .lines()
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let only = uris.next()?;
    if uris.next().is_some() {
        // Two or more URIs: a multi-select is a file list, not a
        // paste payload.
        return None;
    }
    qualify_file_uri(only)
}

/// One URI line's gate (PU-2): `file` scheme, local-or-empty host, a
/// path that percent-decodes to valid UTF-8. Scheme and host compare
/// case-insensitively per the URI grammar; anything else — `trash://`,
/// `smb://`, a foreign host, a missing path — never fires.
fn qualify_file_uri(uri: &str) -> Option<PathBuf> {
    let (scheme, rest) = uri.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("file") {
        return None;
    }
    // The authority runs to the first `/` of the path; that `/` stays
    // the path's own leading byte (`file:///x` has an empty host).
    let path_start = rest.find('/')?;
    let (authority, raw_path) = rest.split_at(path_start);
    if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
        return None;
    }
    let decoded = percent_decode(raw_path)?;
    String::from_utf8(decoded).ok().map(PathBuf::from)
}

/// Percent-decode a URI path per RFC 3986: `%XX` hex pairs become
/// their bytes, every other byte passes through. A dangling or
/// malformed escape answers `None` — the list does not qualify.
fn percent_decode(path: &str) -> Option<Vec<u8>> {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let high = hex_value(*bytes.get(i + 1)?)?;
            let low = hex_value(*bytes.get(i + 2)?)?;
            decoded.push(high << 4 | low);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    Some(decoded)
}

/// One hex digit's value, or `None` outside `[0-9a-fA-F]`.
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// What the file gate resolves a qualified URI's file to (paste-uri
/// spec, PU-3): the sniffed within-cap bytes the paste feeds the
/// pipeline, the silent text fallback, or the over-cap refusal.
#[derive(Debug, PartialEq, Eq)]
enum FileGateOutcome {
    /// Image bytes — decided by the file's own first bytes, read in
    /// the one bounded open.
    Image(Vec<u8>),
    /// Non-regular, missing, unreadable, or a failed sniff: today's
    /// text paste answers the offer set. Silent — a non-image copy
    /// is a normal state, no warn owed.
    TextFallback,
    /// An image over the paste cap: refused whole — one warn, no
    /// bytes, never the text fallback (it would paste a URI exactly
    /// where the user expected their image).
    Refused,
}

/// The file gate (paste-uri spec, PU-3): the one place the terminal
/// opens a file the clipboard named. `metadata()` (which follows
/// symlinks) must report a regular file before any open — a FIFO,
/// socket, device, or directory is never opened (a FIFO open would
/// block the paste). One open, one read: `PASTE_PAYLOAD_CAP + 1`
/// bytes through the same `take` pattern the clipboard reads use, so
/// the sniff sees exactly the bytes the pipeline would paste — no
/// second open, no TOCTOU re-read. The content decides: the
/// magic-byte sniff is the gate, never the name, the extension, or
/// the URI's say-so.
fn read_uri_image_file(path: &std::path::Path) -> FileGateOutcome {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => {}
        // Missing or non-regular: the silent text fallback.
        _ => return FileGateOutcome::TextFallback,
    }
    // Unopenable: the silent text fallback.
    let Ok(file) = std::fs::File::open(path) else {
        return FileGateOutcome::TextFallback;
    };
    let mut bytes = Vec::new();
    let mut bounded = file.take((PASTE_PAYLOAD_CAP + 1) as u64);
    if std::io::Read::read_to_end(&mut bounded, &mut bytes).is_err() {
        // Unreadable mid-read: the silent text fallback.
        return FileGateOutcome::TextFallback;
    }
    // The sniff runs before the cap: an over-cap NON-image is a
    // sniff fail (fallback), only an over-cap image refuses whole.
    if sniff_image_format(&bytes).is_none() {
        return FileGateOutcome::TextFallback;
    }
    if bytes.len() > PASTE_PAYLOAD_CAP {
        log::warn!(
            "paste image refused: {} is over the {PASTE_PAYLOAD_CAP}-byte paste cap",
            path.display()
        );
        return FileGateOutcome::Refused;
    }
    FileGateOutcome::Image(bytes)
}

/// Does the served mime name the URI list (PU-1)? Observed on
/// cosmic-comp (OQ-1, research record 2026-11-09): the priority read
/// answers `text/uri-list` verbatim in every offer shape. The
/// decorated arm is tolerance, not observation — the text fallback's
/// `text/plain;charset=utf-8` is the precedent for a compositor
/// decorating this one too — and it never over-matches: a longer
/// mime sharing the prefix is not the list mime.
fn is_uri_list_mime(served: &str) -> bool {
    served
        .strip_prefix(URI_LIST_MIME)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(';'))
}

/// Which road a successful first paste read takes (PU-1): the served
/// mime picks — the URI list drives the file road, any text type is
/// today's text paste — but only while the gate stands (PU-6): gate
/// off means the text road for every served mime, today restored.
enum PasteRoad {
    /// Parse, qualify, and read the file the list names (PU-2, PU-3).
    UriList,
    /// A text offer read verbatim — today's paste.
    Text,
}

fn paste_first_road(served_mime: &str, image_enabled: bool) -> PasteRoad {
    if image_enabled && is_uri_list_mime(served_mime) {
        PasteRoad::UriList
    } else {
        PasteRoad::Text
    }
}

/// What the URI road resolves a served list to (PU-1): image bytes
/// into today's pipeline, the text fallback (one plain-text read of
/// the same offer set), or the gate's refusal — which never falls
/// back.
#[derive(Debug, PartialEq, Eq)]
enum UriRoadOutcome {
    /// The one file's sniffed, within-cap bytes.
    Image(Vec<u8>),
    /// No qualification, or a fallback-shaped gate: today's text
    /// paste answers.
    TextFallback,
    /// The gate refused (an over-cap image): paste nothing.
    Refused,
}

/// The URI road's resolution: qualify the served list (PU-2), then
/// read the one file the gate allows (PU-3).
fn resolve_uri_road(list: &str) -> UriRoadOutcome {
    match qualify_uri_list(list) {
        Some(path) => match read_uri_image_file(&path) {
            FileGateOutcome::Image(bytes) => UriRoadOutcome::Image(bytes),
            FileGateOutcome::TextFallback => UriRoadOutcome::TextFallback,
            FileGateOutcome::Refused => UriRoadOutcome::Refused,
        },
        // Zero, two or more, or non-local URIs: today's text paste.
        None => UriRoadOutcome::TextFallback,
    }
}

/// What one finished paste read hands the async wrapper (PU-1).
#[derive(Debug, PartialEq, Eq)]
enum PasteReadOutcome {
    /// A paste payload — text read verbatim, or image bytes.
    Contents(PasteContents),
    /// Nothing pasted: empty-clipboard states and refusals (warns
    /// already emitted where owed).
    Nothing,
    /// Retry through the iced clipboard — a transport failure; iced
    /// reads text, so it is never the image read's fallback.
    IcedFallback,
}

/// The fallback text read's resolution: a text offer pastes verbatim,
/// an empty result pastes nothing silently, an over-cap offer refuses
/// whole, a transport failure retries through iced once.
fn resolve_fallback_read(op: &str, result: Result<String, DataControlError>) -> PasteReadOutcome {
    match result {
        Ok(text) => PasteReadOutcome::Contents(PasteContents::Text(text)),
        Err(e) => match classify_read_failure(&e) {
            ReadFailure::Empty => PasteReadOutcome::Nothing,
            ReadFailure::Refuse => {
                log::warn!("data-control {op} refused ({e}); pasting nothing");
                PasteReadOutcome::Nothing
            }
            ReadFailure::Fallback => {
                log::warn!("data-control {op} failed ({e}); falling back to the iced clipboard");
                PasteReadOutcome::IcedFallback
            }
        },
    }
}

/// The first read's failure follow-up (PU-1): the image attempt on an
/// empty-classified read under an open gate, today's nothing-pasted
/// under a closed one, the warn-and-refuse of an over-cap offer, the
/// iced retry of a transport failure.
fn paste_read_follow_up(
    primary: bool,
    image_enabled: bool,
    op: &str,
    failed: &DataControlError,
) -> PasteReadOutcome {
    match paste_follow_up(&classify_read_failure(failed), image_enabled) {
        PasteFollowUp::IcedFallback => {
            log::warn!("data-control {op} failed ({failed}); falling back to the iced clipboard");
            PasteReadOutcome::IcedFallback
        }
        PasteFollowUp::Refused => {
            log::warn!("data-control {op} refused ({failed}); pasting nothing");
            PasteReadOutcome::Nothing
        }
        PasteFollowUp::NothingPasted => PasteReadOutcome::Nothing,
        PasteFollowUp::TryImage => match load_image(primary) {
            Ok(bytes) => PasteReadOutcome::Contents(PasteContents::Image(bytes)),
            // No image offer either: the empty-clipboard state.
            Err(e) if matches!(classify_read_failure(&e), ReadFailure::Empty) => {
                PasteReadOutcome::Nothing
            }
            Err(e) => {
                log::warn!("data-control image read failed ({e})");
                PasteReadOutcome::Nothing
            }
        },
    }
}

/// The paste cascade's blocking core (PU-1): one thread hop for the
/// whole cascade — the priority read, the URI road it may open, the
/// fallback text read, the image attempt. The iced text retry is the
/// async wrapper's business (it needs no blocking).
fn load_paste_contents(primary: bool, image_enabled: bool) -> PasteReadOutcome {
    let op = if primary {
        "primary read"
    } else {
        "clipboard read"
    };
    let (text, served) = match load_for_paste(primary, image_enabled) {
        Ok(pair) => pair,
        Err(e) => return paste_read_follow_up(primary, image_enabled, op, &e),
    };
    match paste_first_road(&served, image_enabled) {
        // The URI road sits inside the text read: a clipboard
        // offering a URI list never reaches the image-bytes read.
        PasteRoad::UriList => match resolve_uri_road(&text) {
            UriRoadOutcome::Image(bytes) => PasteReadOutcome::Contents(PasteContents::Image(bytes)),
            // The gate already warned; paste nothing, never the text
            // fallback (it would paste a URI exactly where the user
            // expected their image).
            UriRoadOutcome::Refused => PasteReadOutcome::Nothing,
            UriRoadOutcome::TextFallback => resolve_fallback_read(op, load(primary)),
        },
        PasteRoad::Text => PasteReadOutcome::Contents(PasteContents::Text(text)),
    }
}

/// What a paste found on the clipboard (paste-image spec): a text
/// offer read verbatim, image bytes read on the text read's empty
/// fallthrough, or image bytes read from the one local file a
/// qualified URI list named (paste-uri spec, PU-1).
#[derive(Debug, PartialEq, Eq)]
pub enum PasteContents {
    /// A text offer — today's paste payload, byte-for-byte.
    Text(String),
    /// Image bytes — `image/png` from the clipboard road, any of the
    /// four sniffed formats from the file road.
    Image(Vec<u8>),
}

/// What follows a failed text read on the paste path: the image
/// attempt (an empty clipboard under an open gate), nothing pasted
/// (an empty clipboard under a closed gate — today's behavior
/// exactly), or the iced text fallback (a transport failure; iced
/// reads text, so it is never the image read's fallback).
enum PasteFollowUp {
    TryImage,
    NothingPasted,
    IcedFallback,
    /// An offer over the paste cap: warn once and paste nothing — the
    /// empty read's outcome with the warn the empty state owes no
    /// one (a refusal happened, not a state was found).
    Refused,
}

fn paste_follow_up(failure: &ReadFailure, image_enabled: bool) -> PasteFollowUp {
    match failure {
        ReadFailure::Empty if image_enabled => PasteFollowUp::TryImage,
        ReadFailure::Empty => PasteFollowUp::NothingPasted,
        ReadFailure::Refuse => PasteFollowUp::Refused,
        ReadFailure::Fallback => PasteFollowUp::IcedFallback,
    }
}

/// The blocking store core: one data-control copy. `copy()` returns
/// once the selection is set; its own detached thread serves the
/// offer until something else replaces it (no fork — the serving
/// loop is a spawned thread in wl-clipboard-rs).
fn store(text: String, primary: bool) -> Result<(), wl_clipboard_rs::copy::Error> {
    let mut options = Options::new();
    // Parity with the seam this module replaces: smithay-clipboard
    // stored text verbatim, and a copied shell line keeps its
    // trailing newline. Pinned explicitly so an upstream default
    // change cannot silently mangle copies.
    options.trim_newline(false).clipboard(if primary {
        ClipboardType::Primary
    } else {
        ClipboardType::Regular
    });
    wl_clipboard_rs::copy::copy(
        options,
        Source::Bytes(text.into_bytes().into_boxed_slice()),
        MimeType::Text,
    )
}

/// The offered mime for an image copy (paste-panel spec, PP-3;
/// paste-uri PU-5): `Specific` over the bytes' own sniffed format —
/// what the cache holds is what the copy claims — with `image/png`
/// as the unknown-sniff floor, keeping the write/read round-trip
/// honest for the clipboard road's by-construction PNG bytes.
fn image_mime(bytes: &[u8]) -> MimeType {
    MimeType::Specific(
        sniff_image_format(bytes)
            .map_or(IMAGE_PNG_MIME, SniffedFormat::mime)
            .to_owned(),
    )
}

/// The blocking image-store core: one data-control copy offering the
/// bytes' sniffed mime (PU-5) — the same shape [`store`] makes for
/// text, minus the newline trim (these are binary bytes, not a shell
/// line; a trim would be a corrupter). Serves the regular clipboard
/// only: the primary selection has no image consumer worth a twin.
fn store_image(bytes: Vec<u8>) -> Result<(), wl_clipboard_rs::copy::Error> {
    let mime = image_mime(&bytes);
    let mut options = Options::new();
    options.clipboard(ClipboardType::Regular);
    wl_clipboard_rs::copy::copy(options, Source::Bytes(bytes.into_boxed_slice()), mime)
}

/// The data-control selection a paste reads: the primary selection or
/// the regular clipboard, per the flag every read seam already
/// carries.
fn paste_clipboard_type(primary: bool) -> paste::ClipboardType {
    if primary {
        paste::ClipboardType::Primary
    } else {
        paste::ClipboardType::Regular
    }
}

/// The pipe half of every text read: one byte past the paste cap, so
/// an offer landing over it refuses whole and never truncates (a
/// truncated paste is a corrupt one). Generic over the reader — the
/// crate returns `os_pipe`'s pipe without re-exporting its type.
fn read_offer_capped(pipe: impl std::io::Read) -> Result<String, DataControlError> {
    let mut text = String::new();
    let mut offer = pipe.take((PASTE_PAYLOAD_CAP + 1) as u64);
    std::io::Read::read_to_string(&mut offer, &mut text)?;
    if text.len() > PASTE_PAYLOAD_CAP {
        return Err(DataControlError::OfferTooLarge);
    }
    Ok(text)
}

/// The blocking load core: one data-control read of any text type. An
/// empty clipboard surfaces as `Err(ClipboardEmpty)` — the pure
/// classifier decides how that resolves.
fn load(primary: bool) -> Result<String, DataControlError> {
    let (pipe, _mime) = paste::get_contents(
        paste_clipboard_type(primary),
        Seat::Unspecified,
        PasteMime::Text,
    )?;
    read_offer_capped(pipe)
}

/// The first paste read's blocking core (paste-uri spec, PU-1): while
/// the gate stands it prioritizes [`URI_LIST_MIME`] — ask for the
/// list, else any text, order over the offer list irrelevant — and
/// answers the served mime that picks the road. Gate off requests
/// exactly today's plain text read, restoring today byte-for-byte
/// (PU-6). An empty clipboard surfaces as `Err(ClipboardEmpty)` like
/// [`load`].
fn load_for_paste(
    primary: bool,
    image_enabled: bool,
) -> Result<(String, String), DataControlError> {
    let request = if image_enabled {
        PasteMime::TextWithPriority(URI_LIST_MIME)
    } else {
        PasteMime::Text
    };
    let (pipe, served) =
        paste::get_contents(paste_clipboard_type(primary), Seat::Unspecified, request)?;
    Ok((read_offer_capped(pipe)?, served))
}

/// The blocking image-read core (paste-image spec, PI-1): one
/// data-control read pinned to `image/png` — `wl-clipboard-rs` has no
/// image mime variant, `Specific` is the request. A clipboard without
/// an image offer surfaces as the same empty classification an empty
/// text read gives. The same paste cap and the same refuse-whole
/// rule as [`load`] bound the pipe.
fn load_image(primary: bool) -> Result<Vec<u8>, DataControlError> {
    let (pipe, _mime) = paste::get_contents(
        paste_clipboard_type(primary),
        Seat::Unspecified,
        PasteMime::Specific(IMAGE_PNG_MIME),
    )?;
    let mut bytes = Vec::new();
    let mut offer = pipe.take((PASTE_PAYLOAD_CAP + 1) as u64);
    std::io::Read::read_to_end(&mut offer, &mut bytes)?;
    if bytes.len() > PASTE_PAYLOAD_CAP {
        return Err(DataControlError::OfferTooLarge);
    }
    Ok(bytes)
}

/// Write `text` to the system clipboard without requiring keyboard
/// focus — the whole point of this module. A data-control failure
/// warns once (naming the operation) and falls back to the iced
/// clipboard for this single write.
pub fn write<T>(text: String) -> Task<T>
where
    T: Send + 'static,
{
    let for_fallback = text.clone();
    let join = tokio::task::spawn_blocking(move || store(text, false));
    Task::future(async move {
        if let Err(err) = join
            .await
            .map_err(DataControlError::Join)
            .and_then(|res| res.map_err(DataControlError::Copy))
        {
            log::warn!(
                "data-control clipboard write failed ({err}); falling back to the iced clipboard"
            );
            return Some(for_fallback);
        }
        None
    })
    .then(|fallback| match fallback {
        Some(text) => iced_clipboard::write(text),
        None => Task::none(),
    })
}

/// Write image `bytes` to the system clipboard — the images panel's
/// copy-image verb (paste-panel spec): the focus-free write path
/// offering the bytes' sniffed format as the mime (paste-uri PU-5;
/// the name keeps its PNG birth, the cache it serves no longer holds
/// only PNG), the remote-friendly complement to the SSH caveat
/// (bytes travel where a local path cannot). No iced fallback exists
/// for bytes (iced writes text), so a data-control failure warns
/// once (naming the operation) and copies nothing — the image has no
/// text form (spec OQ-4).
pub fn write_image_png<T>(bytes: Vec<u8>) -> Task<T>
where
    T: Send + 'static,
{
    let join = tokio::task::spawn_blocking(move || store_image(bytes));
    Task::future(async move {
        if let Err(err) = join
            .await
            .map_err(DataControlError::Join)
            .and_then(|res| res.map_err(DataControlError::Copy))
        {
            log::warn!("data-control image clipboard write failed ({err}); nothing copied");
        }
    })
    .then(|()| Task::none())
}

/// The primary-selection twin of [`write`].
pub fn write_primary<T>(text: String) -> Task<T>
where
    T: Send + 'static,
{
    let for_fallback = text.clone();
    let join = tokio::task::spawn_blocking(move || store(text, true));
    Task::future(async move {
        if let Err(err) = join
            .await
            .map_err(DataControlError::Join)
            .and_then(|res| res.map_err(DataControlError::Copy))
        {
            log::warn!(
                "data-control primary write failed ({err}); falling back to the iced clipboard"
            );
            return Some(for_fallback);
        }
        None
    })
    .then(|fallback| match fallback {
        Some(text) => iced_clipboard::write_primary(text),
        None => Task::none(),
    })
}

/// Read the system clipboard without requiring keyboard focus. An
/// empty clipboard answers `None` silently; a transport failure warns
/// and falls back to the iced clipboard for this single read; an
/// offer over the paste cap warns and answers `None` — refused
/// whole, never truncated, never the unbounded fallback.
pub fn read() -> Task<Option<String>> {
    let join = tokio::task::spawn_blocking(move || load(false));
    Task::future(async move {
        let failed = match join.await {
            Ok(Ok(text)) => return Some(Some(text)),
            Ok(Err(e)) => e,
            Err(e) => DataControlError::Join(e),
        };
        match classify_read_failure(&failed) {
            ReadFailure::Empty => Some(None),
            ReadFailure::Refuse => {
                log::warn!("clipboard read refused ({failed}); the offer is over the paste cap");
                Some(None)
            }
            ReadFailure::Fallback => {
                log::warn!("data-control clipboard read failed ({failed}); falling back to the iced clipboard");
                None
            }
        }
    })
    .then(|outcome| match outcome {
        Some(value) => Task::done(value),
        None => iced_clipboard::read(),
    })
}

/// The primary-selection twin of [`read`].
pub fn read_primary() -> Task<Option<String>> {
    let join = tokio::task::spawn_blocking(move || load(true));
    Task::future(async move {
        let failed = match join.await {
            Ok(Ok(text)) => return Some(Some(text)),
            Ok(Err(e)) => e,
            Err(e) => DataControlError::Join(e),
        };
        match classify_read_failure(&failed) {
            ReadFailure::Empty => Some(None),
            ReadFailure::Refuse => {
                log::warn!("primary read refused ({failed}); the offer is over the paste cap");
                Some(None)
            }
            ReadFailure::Fallback => {
                log::warn!("data-control primary read failed ({failed}); falling back to the iced clipboard");
                None
            }
        }
    })
    .then(|outcome| match outcome {
        Some(value) => Task::done(value),
        None => iced_clipboard::read_primary(),
    })
}

/// Read the clipboard for a paste, URI-list first (paste-uri spec,
/// PU-1; paste-image PI-1). The first read prioritizes
/// [`URI_LIST_MIME`]: a served URI list drives the file road —
/// qualify the list (PU-2), read the one file the gate allows
/// (PU-3), its sniffed bytes into today's pipeline — and a
/// non-qualifying list or a fallback-shaped gate falls back to one
/// plain-text read of the same offer set, pasted verbatim. A text
/// type served resolves [`PasteContents::Text`], byte-for-byte
/// today's read. Only an empty-classified first read falls through
/// to the `image/png` attempt; a clipboard offering neither resolves
/// `None`, the same silent nothing-pasted state an empty clipboard
/// is today. The whole cascade rides [`PASTE_IMAGE_ENABLED`] (PU-6):
/// gate off requests today's plain text read and restores today
/// exactly. The image attempt has no iced fallback (iced reads
/// text): a transport failure there warns once and pastes nothing —
/// as does an offer over the paste cap, refused whole on any read.
pub fn read_for_paste(primary: bool) -> Task<Option<PasteContents>> {
    let join =
        tokio::task::spawn_blocking(move || load_paste_contents(primary, PASTE_IMAGE_ENABLED));
    Task::future(async move {
        match join.await {
            Ok(PasteReadOutcome::Contents(contents)) => Some(Some(contents)),
            Ok(PasteReadOutcome::Nothing) => Some(None),
            Ok(PasteReadOutcome::IcedFallback) => None,
            Err(e) => {
                let err = DataControlError::Join(e);
                log::warn!(
                    "data-control paste read failed ({err}); falling back to the iced clipboard"
                );
                None
            }
        }
    })
    .then(move |outcome| match outcome {
        Some(value) => Task::done(value),
        None => if primary {
            iced_clipboard::read_primary()
        } else {
            iced_clipboard::read()
        }
        .map(|text| text.map(PasteContents::Text)),
    })
}

/// The paste cache's directory: `dropterm/paste` under the XDG cache
/// dir (paste-image spec). `None` when no cache dir resolves. Shared
/// with the images panel's Open Folder action — one resolution of
/// the directory the panel's rows live in.
pub(crate) fn paste_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("dropterm").join("paste"))
}

/// The paste filename's timestamp: `YYYYMMDD-HHMMSS`, local zone (the
/// band clock's `strftime` precedent).
fn paste_stamp(now: &jiff::Zoned) -> String {
    now.strftime("%Y%m%d-%H%M%S").to_string()
}

/// The next paste filename for `stamp`, its counter stepping past
/// every same-stamp name the directory already holds — same-second
/// pastes never collide. The candidate carries `extension` whole
/// (PU-4): names under different extensions are separate slots, with
/// `create_new` still the arbiter on the open.
fn next_paste_name(existing: &HashSet<String>, stamp: &str, extension: &str) -> String {
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
fn write_paste_image_in(dir: &std::path::Path, stamp: &str, bytes: &[u8]) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::{
        DataControlError, FileGateOutcome, PASTE_PAYLOAD_CAP, PasteContents, PasteFollowUp,
        PasteReadOutcome, PasteRoad, ReadFailure, SniffedFormat, UriRoadOutcome,
        classify_read_failure, image_mime, is_uri_list_mime, load_paste_contents, next_paste_name,
        paste_first_road, paste_follow_up, paste_stamp, qualify_uri_list, read_uri_image_file,
        resolve_fallback_read, resolve_uri_road, sniff_image_format, write_paste_image_in,
    };

    #[test]
    fn the_image_write_offers_the_sniffed_mime() {
        // The panel's copy-image verb round-trips with the paste read
        // (paste-panel spec, PP-3; paste-uri PU-5): with mixed formats
        // in the cache, the write offers the file's own sniffed mime —
        // an unknown sniff keeps the image/png floor `load_image` pins.
        assert!(matches!(
            image_mime(b"\x89PNG\r\n\x1a\n"),
            wl_clipboard_rs::copy::MimeType::Specific(mime) if mime == "image/png"
        ));
        assert!(matches!(
            image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]),
            wl_clipboard_rs::copy::MimeType::Specific(mime) if mime == "image/jpeg"
        ));
        assert!(matches!(
            image_mime(b"not an image at all"),
            wl_clipboard_rs::copy::MimeType::Specific(mime) if mime == "image/png"
        ));
    }

    // --- the sniff core (paste-uri spec, PU-3/PU-4) ---

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

    // --- the URI-list qualification (paste-uri spec, PU-2) ---

    #[test]
    fn a_single_local_file_uri_qualifies_with_its_decoded_path() {
        assert_eq!(
            qualify_uri_list("file:///home/you/pic%20nic.jpg\r\n"),
            Some(std::path::PathBuf::from("/home/you/pic nic.jpg"))
        );
        assert_eq!(
            qualify_uri_list("file:///home/you/pic.jpg\n"),
            Some(std::path::PathBuf::from("/home/you/pic.jpg"))
        );
        // Schemes and hosts compare case-insensitively (RFC 3986);
        // file managers emit lowercase, the URI grammar says do not
        // care.
        assert_eq!(
            qualify_uri_list("FILE://LocalHost/home/you/pic.jpg"),
            Some(std::path::PathBuf::from("/home/you/pic.jpg"))
        );
    }

    #[test]
    fn a_localhost_host_qualifies_a_foreign_host_does_not() {
        assert_eq!(
            qualify_uri_list("file://localhost/home/you/pic.jpg"),
            Some(std::path::PathBuf::from("/home/you/pic.jpg"))
        );
        assert_eq!(qualify_uri_list("file://nas/photos/pic.jpg"), None);
        // An authority with no path names no file.
        assert_eq!(qualify_uri_list("file://localhost"), None);
    }

    #[test]
    fn non_qualifying_lists_never_fire() {
        // Zero URIs (comments only, or nothing), two or more, other
        // schemes, and schemeless lines all mean the text fallback.
        assert_eq!(qualify_uri_list("# a comment\r\n# another\r\n"), None);
        assert_eq!(qualify_uri_list(""), None);
        assert_eq!(qualify_uri_list("file:///a.jpg\r\nfile:///b.jpg\r\n"), None);
        assert_eq!(
            qualify_uri_list("file:///a.jpg\r\nfile:///b.jpg\r\nfile:///c.jpg\r\n"),
            None
        );
        assert_eq!(qualify_uri_list("trash:///home/you/pic.jpg"), None);
        assert_eq!(qualify_uri_list("smb://server/pic.jpg"), None);
        assert_eq!(qualify_uri_list("/home/you/pic.jpg"), None);
    }

    #[test]
    fn comments_and_crlf_shapes_parse_per_rfc_2483() {
        // cosmic-files emits LF, Nautilus CRLF; comments and blank
        // lines around the URI never disqualify it.
        assert_eq!(
            qualify_uri_list("# comment\n\nfile:///home/you/pic.jpg\n"),
            Some(std::path::PathBuf::from("/home/you/pic.jpg"))
        );
        assert_eq!(
            qualify_uri_list("# Nautilus shape\r\n\r\nfile:///home/you/pic.jpg\r\n"),
            Some(std::path::PathBuf::from("/home/you/pic.jpg"))
        );
    }

    #[test]
    fn malformed_escapes_and_non_utf8_paths_do_not_qualify() {
        // A dangling or malformed % escape, and a path decoding
        // outside UTF-8, answer nothing: the list does not qualify.
        assert_eq!(qualify_uri_list("file:///home/pic%2.jpg"), None);
        assert_eq!(qualify_uri_list("file:///home/pic%zz.jpg"), None);
        assert_eq!(qualify_uri_list("file:///home/pic%.jpg"), None);
        assert_eq!(qualify_uri_list("file:///home/pic%ff.jpg"), None);
    }

    // --- the file gate (paste-uri spec, PU-3) ---

    /// A fresh temp dir per test, cleared first so re-runs start
    /// clean (the cache suite's own fixture convention).
    fn gate_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dropterm-uri-gate-tests-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("fixture dir");
        dir
    }

    /// A minimal PNG: the 8-byte signature plus a body — the gate
    /// never looks past magic, so this is a stand-in image.
    const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

    #[test]
    fn a_fixture_image_file_yields_its_bytes() {
        let dir = gate_dir("image");
        let file = dir.join("pic.png");
        let mut image = PNG_MAGIC.to_vec();
        image.extend_from_slice(b"body bytes");
        std::fs::write(&file, &image).expect("fixture image");
        assert_eq!(
            read_uri_image_file(&file),
            FileGateOutcome::Image(image.clone())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_symlink_to_an_image_qualifies_identically() {
        // metadata() follows the link: a symlink resolving to a
        // regular image file is that file.
        let dir = gate_dir("symlink-image");
        let file = dir.join("pic.png");
        std::fs::write(&file, PNG_MAGIC).expect("fixture image");
        let link = dir.join("link.png");
        std::os::unix::fs::symlink(&file, &link).expect("fixture link");
        assert_eq!(
            read_uri_image_file(&link),
            FileGateOutcome::Image(PNG_MAGIC.to_vec())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_and_a_device_are_never_opened() {
        // Non-regular targets resolve to the text fallback without an
        // open: a FIFO open would block the paste, a directory or
        // device read is meaningless.
        let dir = gate_dir("non-regular");
        assert_eq!(read_uri_image_file(&dir), FileGateOutcome::TextFallback);
        assert_eq!(
            read_uri_image_file(std::path::Path::new("/dev/null")),
            FileGateOutcome::TextFallback
        );
        let fifo = dir.join("fifo");
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs")
            .success()
            .then_some(())
            .expect("fifo stands");
        // Would hang on an open; resolving without one is the pin.
        assert_eq!(read_uri_image_file(&fifo), FileGateOutcome::TextFallback);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_or_non_image_file_resolves_to_the_silent_fallback() {
        // A non-image copy is a normal state: no warn, the offer set's
        // text answers the paste.
        let dir = gate_dir("fallbacks");
        assert_eq!(
            read_uri_image_file(&dir.join("missing.png")),
            FileGateOutcome::TextFallback
        );
        let text = dir.join("notes.txt");
        std::fs::write(&text, b"just words, an extension that lies").expect("fixture text");
        assert_eq!(read_uri_image_file(&text), FileGateOutcome::TextFallback);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_over_cap_image_is_refused_whole_never_fallen_back() {
        // One byte past the cap: the refusal classification — no
        // bytes, no partial read, and never the text fallback (it
        // would paste a URI exactly where the user expected their
        // image).
        let dir = gate_dir("over-cap");
        let file = dir.join("huge.png");
        let mut image = PNG_MAGIC.to_vec();
        image.resize(PASTE_PAYLOAD_CAP + 1, 0);
        std::fs::write(&file, &image).expect("fixture image");
        assert_eq!(read_uri_image_file(&file), FileGateOutcome::Refused);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- the cascade (paste-uri spec, PU-1/PU-6) ---

    #[test]
    fn the_served_mime_picks_the_road_only_while_gated() {
        // Observed on cosmic-comp (OQ-1, research record 2026-11-09):
        // the priority read answers "text/uri-list" verbatim in every
        // offer shape; the text fallback decorates
        // ("text/plain;charset=utf-8"). The decorated uri-list arm is
        // tolerance, not observation — the text fallback's
        // decoration is the precedent for a compositor decorating
        // this one too.
        assert!(matches!(
            paste_first_road("text/uri-list", true),
            PasteRoad::UriList
        ));
        assert!(matches!(
            paste_first_road("text/uri-list;charset=utf-8", true),
            PasteRoad::UriList
        ));
        assert!(matches!(
            paste_first_road("text/plain;charset=utf-8", true),
            PasteRoad::Text
        ));
        assert!(matches!(
            paste_first_road("text/plain", true),
            PasteRoad::Text
        ));
        // Gate off (PU-6): today's text road for every served mime.
        assert!(matches!(
            paste_first_road("text/uri-list", false),
            PasteRoad::Text
        ));
    }

    #[test]
    fn uri_list_mime_matching_never_over_matches() {
        assert!(is_uri_list_mime("text/uri-list"));
        assert!(is_uri_list_mime("text/uri-list;charset=utf-8"));
        // A longer mime sharing the prefix is not the list mime.
        assert!(!is_uri_list_mime("text/uri-listish"));
        assert!(!is_uri_list_mime("text/plain;charset=utf-8"));
        assert!(!is_uri_list_mime(""));
    }

    #[test]
    fn the_uri_road_resolves_its_three_outcomes() {
        let dir = gate_dir("uri-road");
        let image = dir.join("pic.png");
        std::fs::write(&image, PNG_MAGIC).expect("fixture image");
        // A qualifying list over the fixture: the image bytes.
        assert_eq!(
            resolve_uri_road(&format!("file://{}\n", image.display())),
            UriRoadOutcome::Image(PNG_MAGIC.to_vec())
        );
        // A non-image local file: today's text paste answers.
        let text = dir.join("notes.txt");
        std::fs::write(&text, b"words").expect("fixture text");
        assert_eq!(
            resolve_uri_road(&format!("file://{}\n", text.display())),
            UriRoadOutcome::TextFallback
        );
        // A multi-select never qualifies: the text fallback.
        assert_eq!(
            resolve_uri_road("file:///a.png\r\nfile:///b.png\r\n"),
            UriRoadOutcome::TextFallback
        );
        // An over-cap image: refused whole, never the fallback.
        let huge = dir.join("huge.png");
        let mut body = PNG_MAGIC.to_vec();
        body.resize(PASTE_PAYLOAD_CAP + 1, 0);
        std::fs::write(&huge, &body).expect("fixture image");
        assert_eq!(
            resolve_uri_road(&format!("file://{}\n", huge.display())),
            UriRoadOutcome::Refused
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_fallback_text_read_pastes_verbatim_and_empty_pastes_nothing() {
        // The second read's resolution: a text offer pastes verbatim;
        // an empty one pastes nothing, silently.
        assert_eq!(
            resolve_fallback_read("clipboard read", Ok("the plain offer, verbatim".to_owned())),
            PasteReadOutcome::Contents(PasteContents::Text("the plain offer, verbatim".to_owned()))
        );
        assert_eq!(
            resolve_fallback_read(
                "clipboard read",
                Err(DataControlError::Paste(
                    wl_clipboard_rs::paste::Error::ClipboardEmpty
                ))
            ),
            PasteReadOutcome::Nothing
        );
    }

    // The COSMIC-hardware manual check (paste-uri spec's testing
    // strategy; the docs/testing.md row points here). Serves real
    // data-control offers on the running compositor and runs the
    // shipped cascade against them — it takes over the system
    // clipboard while it runs, so it stays #[ignore]d out of the
    // headless suite: `cargo test --bin dropterm
    // live_uri_paste_smoke -- --ignored`. The expectations encode
    // observed cosmic-comp behavior (research record, 2026-11-09):
    // the priority read serves "text/uri-list" verbatim, and the
    // plain Text read prefers the compositor's
    // "text/plain;charset=utf-8" conversion of the list — so the
    // multi-select fallback pastes the list text, byte-for-byte
    // today's read.
    #[test]
    #[ignore = "takes over the system clipboard; run on a COSMIC session with -- --ignored"]
    fn live_uri_paste_smoke() {
        use wl_clipboard_rs::copy::{ClipboardType, MimeSource, Options, Source};

        let dir = std::env::temp_dir().join("dropterm-live-paste-smoke");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("fixture dir");
        let jpeg = dir.join("smoke.jpg");
        std::fs::write(&jpeg, [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3]).expect("fixture jpeg");
        let text = dir.join("notes.txt");
        std::fs::write(&text, b"just words").expect("fixture text");

        let serve = |sources: Vec<MimeSource>| {
            let mut options = Options::new();
            options.clipboard(ClipboardType::Regular);
            wl_clipboard_rs::copy::copy_multi(options, sources).expect("serve");
        };
        let uri_offer = |paths: &[&std::path::Path]| MimeSource {
            source: Source::Bytes(
                paths
                    .iter()
                    .fold(String::new(), |list, path| {
                        list + &format!("file://{}\r\n", path.display())
                    })
                    .into_bytes()
                    .into_boxed_slice(),
            ),
            mime_type: wl_clipboard_rs::copy::MimeType::Specific("text/uri-list".to_owned()),
        };
        let text_offer = |content: &str| MimeSource {
            source: Source::Bytes(content.as_bytes().to_vec().into_boxed_slice()),
            mime_type: wl_clipboard_rs::copy::MimeType::Specific("text/plain".to_owned()),
        };

        // cosmic-files shape: uri-list + text/plain naming one JPEG.
        serve(vec![uri_offer(&[&jpeg]), text_offer("/the/plain/offer")]);
        assert_eq!(
            load_paste_contents(false, true),
            PasteReadOutcome::Contents(PasteContents::Image(vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3]))
        );

        // Multi-select: one list naming two files — today's text
        // paste. Observed on cosmic-comp: the plain Text read prefers
        // the compositor's "text/plain;charset=utf-8" conversion of
        // the uri-list, so today's paste IS the list text — which is
        // exactly what the fallback must reproduce.
        let list_two = format!("file://{}\r\nfile://{}\r\n", jpeg.display(), text.display());
        serve(vec![
            uri_offer(&[&jpeg, &text]),
            text_offer("/the/plain/offer"),
        ]);
        assert_eq!(
            load_paste_contents(false, true),
            PasteReadOutcome::Contents(PasteContents::Text(list_two))
        );

        // A URI naming a non-image local file: no file bytes, the
        // plain read's answer pastes.
        serve(vec![uri_offer(&[&text]), text_offer("/the/plain/offer")]);
        assert_eq!(
            load_paste_contents(false, true),
            PasteReadOutcome::Contents(PasteContents::Text(format!(
                "file://{}\r\n",
                text.display()
            )))
        );

        // Gate off (PU-6): today restored — the same plain read the
        // old code made, list text and all.
        serve(vec![uri_offer(&[&jpeg]), text_offer("/the/plain/offer")]);
        assert_eq!(
            load_paste_contents(false, false),
            PasteReadOutcome::Contents(PasteContents::Text(format!(
                "file://{}\r\n",
                jpeg.display()
            )))
        );

        // Plain text only: today's paste.
        serve(vec![text_offer("plain words")]);
        assert_eq!(
            load_paste_contents(false, true),
            PasteReadOutcome::Contents(PasteContents::Text("plain words".to_owned()))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_clipboard_reads_resolve_to_silent_none() {
        // An empty (or typeless) clipboard is a state, not a failure:
        // the read answers None and no warn line is owed.
        assert!(matches!(
            classify_read_failure(&DataControlError::Paste(
                wl_clipboard_rs::paste::Error::ClipboardEmpty
            )),
            ReadFailure::Empty
        ));
        assert!(matches!(
            classify_read_failure(&DataControlError::Paste(
                wl_clipboard_rs::paste::Error::NoMimeType
            )),
            ReadFailure::Empty
        ));
    }

    #[test]
    fn transport_and_support_errors_fall_back() {
        // Everything else — no seats, a missing protocol, primary
        // unsupported, a dead serving join — is a real failure: warn
        // and retry the operation through the iced clipboard once.
        assert!(matches!(
            classify_read_failure(&DataControlError::Paste(
                wl_clipboard_rs::paste::Error::NoSeats
            )),
            ReadFailure::Fallback
        ));
        assert!(matches!(
            classify_read_failure(&DataControlError::Paste(
                wl_clipboard_rs::paste::Error::SeatNotFound
            )),
            ReadFailure::Fallback
        ));
        assert!(matches!(
            classify_read_failure(&DataControlError::Paste(
                wl_clipboard_rs::paste::Error::PrimarySelectionUnsupported
            )),
            ReadFailure::Fallback
        ));
        assert!(matches!(
            classify_read_failure(&DataControlError::Paste(
                wl_clipboard_rs::paste::Error::MissingProtocol {
                    name: "zwlr_data_control_manager_v1",
                    version: 2
                }
            )),
            ReadFailure::Fallback
        ));
        assert!(matches!(
            classify_read_failure(&DataControlError::Copy(
                wl_clipboard_rs::copy::Error::NoSeats
            )),
            ReadFailure::Fallback
        ));
    }

    // --- the paste-image read seam (paste-image spec, PI-1) ---

    #[test]
    fn an_empty_text_read_attempts_the_image_read_only_while_gated() {
        // The fallthrough: only the empty classification (an empty or
        // typeless clipboard) reaches the image attempt, and only
        // while the paste-image gate stands — gate off restores
        // today's behavior exactly (nothing pasted, no image read).
        assert!(matches!(
            paste_follow_up(&ReadFailure::Empty, true),
            PasteFollowUp::TryImage
        ));
        assert!(matches!(
            paste_follow_up(&ReadFailure::Empty, false),
            PasteFollowUp::NothingPasted
        ));
    }

    #[test]
    fn a_transport_failure_never_attempts_the_image_read() {
        // A transport failure keeps its warn + iced fallback (iced
        // reads text); the image read is not a fallback for it.
        assert!(matches!(
            paste_follow_up(&ReadFailure::Fallback, true),
            PasteFollowUp::IcedFallback
        ));
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
        let existing = std::collections::HashSet::new();
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
        let existing: std::collections::HashSet<String> =
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
        let png_taken: std::collections::HashSet<String> =
            ["paste-20261108-140309-1.png".to_owned()]
                .into_iter()
                .collect();
        assert_eq!(
            next_paste_name(&png_taken, "20261108-140309", "jpg"),
            "paste-20261108-140309-1.jpg"
        );
        let jpg_taken: std::collections::HashSet<String> =
            ["paste-20261108-140309-1.jpg".to_owned()]
                .into_iter()
                .collect();
        assert_eq!(
            next_paste_name(&jpg_taken, "20261108-140309", "jpg"),
            "paste-20261108-140309-2.jpg"
        );
    }

    #[test]
    fn an_over_cap_offer_is_refused_never_fallen_back() {
        // The cap is a policy refusal, not a transport failure: warn
        // and answer nothing on either read — iced would read the
        // same oversized offer unbounded, so it is never the
        // fallback, and the image gate has no say in a refusal.
        assert!(matches!(
            classify_read_failure(&DataControlError::OfferTooLarge),
            ReadFailure::Refuse
        ));
        assert!(matches!(
            paste_follow_up(&ReadFailure::Refuse, true),
            PasteFollowUp::Refused
        ));
        assert!(matches!(
            paste_follow_up(&ReadFailure::Refuse, false),
            PasteFollowUp::Refused
        ));
    }

    // --- the cache write (the ship audit's exclusive-write fix) ---

    /// A fresh temp dir per test, cleared first so re-runs start
    /// clean (the tabs suite's own fixture convention).
    fn cache_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dropterm-clipboard-tests-{tag}"));
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
