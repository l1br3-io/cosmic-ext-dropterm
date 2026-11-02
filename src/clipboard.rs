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
}

fn classify_read_failure(err: &DataControlError) -> ReadFailure {
    match err {
        DataControlError::Paste(paste::Error::ClipboardEmpty | paste::Error::NoMimeType) => {
            ReadFailure::Empty
        }
        _ => ReadFailure::Fallback,
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

/// The blocking load core: one data-control read. An empty clipboard
/// surfaces as `Err(ClipboardEmpty)` — the pure classifier decides
/// how that resolves.
fn load(primary: bool) -> Result<String, DataControlError> {
    let (mut pipe, _mime) = paste::get_contents(
        if primary {
            wl_clipboard_rs::paste::ClipboardType::Primary
        } else {
            wl_clipboard_rs::paste::ClipboardType::Regular
        },
        Seat::Unspecified,
        PasteMime::Text,
    )?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut pipe, &mut text)?;
    Ok(text)
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
/// and falls back to the iced clipboard for this single read.
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

#[cfg(test)]
mod tests {
    use super::{DataControlError, ReadFailure, classify_read_failure};

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
}
