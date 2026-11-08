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

/// The ceiling one clipboard offer may carry (the ship audit's
/// decompression-bomb guard for the paste path): reads stop one byte
/// past it — an offer over it is refused whole, and the paste cache
/// writes nothing over it. The engine's `IMAGE_PAYLOAD_CAP`
/// precedent: the caps protect the pipeline, never mangle the
/// payload.
pub(crate) const PASTE_PAYLOAD_CAP: usize = 32 * 1024 * 1024;

/// What a paste found on the clipboard (paste-image spec): a text
/// offer read verbatim, or image bytes read on the text read's empty
/// fallthrough.
pub enum PasteContents {
    /// A text offer — today's paste payload, byte-for-byte.
    Text(String),
    /// `image/png` bytes, the only image interchange format v1 reads.
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

/// The offered mime for an image copy (paste-panel spec, PP-3):
/// `Specific` over [`IMAGE_PNG_MIME`] — the copy side's twin of
/// `load_image`'s read pin.
fn image_png_mime() -> MimeType {
    MimeType::Specific(IMAGE_PNG_MIME.to_owned())
}

/// The blocking image-store core: one data-control copy pinned to
/// `image/png` — the same shape [`store`] makes for text, minus the
/// newline trim (these are binary bytes, not a shell line; a trim
/// would be a corrupter). Serves the regular clipboard only: the
/// primary selection has no image consumer worth a twin.
fn store_image_png(bytes: Vec<u8>) -> Result<(), wl_clipboard_rs::copy::Error> {
    let mut options = Options::new();
    options.clipboard(ClipboardType::Regular);
    wl_clipboard_rs::copy::copy(
        options,
        Source::Bytes(bytes.into_boxed_slice()),
        image_png_mime(),
    )
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

/// The blocking load core: one data-control read. An empty clipboard
/// surfaces as `Err(ClipboardEmpty)` — the pure classifier decides
/// how that resolves. The pipe is read one byte past the paste cap:
/// landing over it refuses the offer whole, never truncates it (a
/// truncated paste is a corrupt one).
fn load(primary: bool) -> Result<String, DataControlError> {
    let (pipe, _mime) = paste::get_contents(
        paste_clipboard_type(primary),
        Seat::Unspecified,
        PasteMime::Text,
    )?;
    let mut text = String::new();
    let mut offer = pipe.take((PASTE_PAYLOAD_CAP + 1) as u64);
    std::io::Read::read_to_string(&mut offer, &mut text)?;
    if text.len() > PASTE_PAYLOAD_CAP {
        return Err(DataControlError::OfferTooLarge);
    }
    Ok(text)
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

/// Write PNG `bytes` to the system clipboard — the images panel's
/// copy-image verb (paste-panel spec): the focus-free write path
/// with `image/png` as the offered mime, the remote-friendly
/// complement to the SSH caveat (bytes travel where a local path
/// cannot). No iced fallback exists for bytes (iced writes text),
/// so a data-control failure warns once (naming the operation) and
/// copies nothing — the image has no text form (spec OQ-4).
pub fn write_image_png<T>(bytes: Vec<u8>) -> Task<T>
where
    T: Send + 'static,
{
    let join = tokio::task::spawn_blocking(move || store_image_png(bytes));
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

/// Read the clipboard for a paste, text first (paste-image spec,
/// PI-1). A text offer resolves as [`PasteContents::Text`] —
/// byte-for-byte today's read. Only an empty-classified text read
/// falls through to an `image/png` attempt; a clipboard offering
/// neither resolves `None`, the same silent nothing-pasted state an
/// empty clipboard is today. The image attempt has no iced fallback
/// (iced reads text): a transport failure there warns once and
/// pastes nothing — as does an offer over the paste cap, refused
/// whole on either read.
pub fn read_for_paste(primary: bool) -> Task<Option<PasteContents>> {
    let op = if primary {
        "primary read"
    } else {
        "clipboard read"
    };
    let join = tokio::task::spawn_blocking(move || load(primary));
    Task::future(async move {
        let failed = match join.await {
            Ok(Ok(text)) => return Some(Some(PasteContents::Text(text))),
            Ok(Err(e)) => e,
            Err(e) => DataControlError::Join(e),
        };
        match paste_follow_up(&classify_read_failure(&failed), PASTE_IMAGE_ENABLED) {
            PasteFollowUp::IcedFallback => {
                log::warn!(
                    "data-control {op} failed ({failed}); falling back to the iced clipboard"
                );
                None
            }
            PasteFollowUp::Refused => {
                log::warn!("data-control {op} refused ({failed}); pasting nothing");
                Some(None)
            }
            PasteFollowUp::NothingPasted => Some(None),
            PasteFollowUp::TryImage => {
                match tokio::task::spawn_blocking(move || load_image(primary)).await {
                    Ok(Ok(bytes)) => Some(Some(PasteContents::Image(bytes))),
                    // No image offer either: the empty-clipboard state.
                    Ok(Err(e)) if matches!(classify_read_failure(&e), ReadFailure::Empty) => {
                        Some(None)
                    }
                    Ok(Err(e)) => {
                        log::warn!("data-control image read failed ({e})");
                        Some(None)
                    }
                    Err(e) => {
                        let err = DataControlError::Join(e);
                        log::warn!("data-control image read failed ({err})");
                        Some(None)
                    }
                }
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
/// dir (paste-image spec). `None` when no cache dir resolves.
fn paste_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("dropterm").join("paste"))
}

/// The paste filename's timestamp: `YYYYMMDD-HHMMSS`, local zone (the
/// band clock's `strftime` precedent).
fn paste_stamp(now: &jiff::Zoned) -> String {
    now.strftime("%Y%m%d-%H%M%S").to_string()
}

/// The next paste filename for `stamp`, its counter stepping past
/// every same-stamp name the directory already holds — same-second
/// pastes never collide.
fn next_paste_name(existing: &HashSet<String>, stamp: &str) -> String {
    let mut counter = 1;
    loop {
        let name = format!("paste-{stamp}-{counter}.png");
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
    let mut existing: HashSet<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    loop {
        let name = next_paste_name(&existing, stamp);
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
        DataControlError, PASTE_PAYLOAD_CAP, PasteFollowUp, ReadFailure, classify_read_failure,
        image_png_mime, next_paste_name, paste_follow_up, paste_stamp, write_paste_image_in,
    };

    #[test]
    fn the_image_write_offers_the_mime_the_read_accepts() {
        // The panel's copy-image verb round-trips with the paste
        // read (paste-panel spec, PP-3): the write offers the same
        // interchange mime `load_image` pins — what copy-image
        // writes, a paste reads back.
        assert!(matches!(
            image_png_mime(),
            wl_clipboard_rs::copy::MimeType::Specific(mime) if mime == "image/png"
        ));
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
            next_paste_name(&existing, "20261108-140309"),
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
            next_paste_name(&existing, "20261108-140309"),
            "paste-20261108-140309-3.png"
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
        // umask, not the 0644/0664 a plain write inherits.
        let mode = std::fs::metadata(&path)
            .expect("the file stands")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
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
