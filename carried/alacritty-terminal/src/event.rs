use std::borrow::Cow;
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;

use crate::term::ClipboardType;
use crate::vte::ansi::Rgb;

/// Terminal event.
///
/// These events instruct the UI over changes that can't be handled by the terminal emulation layer
/// itself.
#[derive(Clone)]
pub enum Event {
    /// Grid has changed possibly requiring a mouse cursor shape change.
    MouseCursorDirty,

    /// Window title change.
    Title(String),

    /// Reset to the default window title.
    ResetTitle,

    /// Request to store a text string in the clipboard.
    ClipboardStore(ClipboardType, String),

    /// Request to write the contents of the clipboard to the PTY.
    ///
    /// The attached function is a formatter which will correctly transform the clipboard content
    /// into the expected escape sequence format.
    ClipboardLoad(ClipboardType, Arc<dyn Fn(&str) -> String + Sync + Send + 'static>),

    /// Request to write the RGB value of a color to the PTY.
    ///
    /// The attached function is a formatter which will correctly transform the RGB color into the
    /// expected escape sequence format.
    ColorRequest(usize, Arc<dyn Fn(Rgb) -> String + Sync + Send + 'static>),

    /// Write some text to the PTY.
    PtyWrite(String),

    /// Request to write the text area size.
    TextAreaSizeRequest(Arc<dyn Fn(WindowSize) -> String + Sync + Send + 'static>),

    /// Cursor blinking state has changed.
    CursorBlinkingChange,

    /// New terminal content available.
    Wakeup,

    /// Terminal bell ring.
    Bell,

    /// Shutdown request.
    Exit,

    /// Child process exited with an error code.
    ChildExit(i32),

    /// Semantic prompt zone finished; the final command's exit code
    /// (OSC 133;D shell integration).
    PromptExit(u32),

    /// Current working directory reported by the shell (OSC 7); the
    /// `file://` URL verbatim.
    CurrentDirectory(String),

    /// Progress report from the running command (OSC 9;4); the state
    /// (0 remove, 1 in progress, 2 error, 3 indeterminate, 4 paused)
    /// and the optional percent value 0–100.
    ProgressReport(u8, Option<u8>),

    /// Program status report (OSC 7501, Hashimoto's Program Status
    /// Protocol); the body's `key=value[:key=value…]` pairs verbatim.
    ProgramStatus(String),

    /// The terminal was fully reset (RIS, `ESC c`); every
    /// shell-integration report dies with the screen.
    FullReset,

    /// Inline image (OSC 1337, iTerm2); the raw semicolon-split
    /// parameter slices of the sequence body verbatim — the argument
    /// grammar and every policy stay consumer-side.
    InlineImage(Vec<Vec<u8>>),

    /// APC string payload (kitty graphics protocol); the accumulated
    /// bytes verbatim, introducer included — the protocol check and
    /// every policy stay consumer-side.
    ApcPayload(Vec<u8>),
}

impl Debug for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::ClipboardStore(ty, text) => write!(f, "ClipboardStore({ty:?}, {text})"),
            Event::ClipboardLoad(ty, _) => write!(f, "ClipboardLoad({ty:?})"),
            Event::TextAreaSizeRequest(_) => write!(f, "TextAreaSizeRequest"),
            Event::ColorRequest(index, _) => write!(f, "ColorRequest({index})"),
            Event::PtyWrite(text) => write!(f, "PtyWrite({text})"),
            Event::Title(title) => write!(f, "Title({title})"),
            Event::CursorBlinkingChange => write!(f, "CursorBlinkingChange"),
            Event::MouseCursorDirty => write!(f, "MouseCursorDirty"),
            Event::ResetTitle => write!(f, "ResetTitle"),
            Event::Wakeup => write!(f, "Wakeup"),
            Event::Bell => write!(f, "Bell"),
            Event::Exit => write!(f, "Exit"),
            Event::ChildExit(code) => write!(f, "ChildExit({code})"),
            Event::PromptExit(code) => write!(f, "PromptExit({code})"),
            Event::CurrentDirectory(url) => write!(f, "CurrentDirectory({url})"),
            Event::ProgressReport(state, value) => write!(f, "ProgressReport({state}, {value:?})"),
            Event::ProgramStatus(body) => write!(f, "ProgramStatus({body})"),
            Event::FullReset => write!(f, "FullReset"),
            // Only the parameter count: a legacy single-shot sequence
            // carries the whole image payload in its parameters.
            Event::InlineImage(params) => write!(f, "InlineImage({} params)", params.len()),
            // Only the byte count: a single payload carries the whole
            // image.
            Event::ApcPayload(payload) => write!(f, "ApcPayload({} bytes)", payload.len()),
        }
    }
}

/// Byte sequences are sent to a `Notify` in response to some events.
pub trait Notify {
    /// Notify that an escape sequence should be written to the PTY.
    ///
    /// TODO this needs to be able to error somehow.
    fn notify<B: Into<Cow<'static, [u8]>>>(&self, _: B);
}

#[derive(Copy, Clone, Debug)]
pub struct WindowSize {
    pub num_lines: u16,
    pub num_cols: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

/// Types that are interested in when the display is resized.
pub trait OnResize {
    fn on_resize(&mut self, window_size: WindowSize);
}

/// Event Loop for notifying the renderer about terminal events.
pub trait EventListener {
    fn send_event(&self, _event: Event) {}
}

/// Null sink for events.
pub struct VoidListener;

impl EventListener for VoidListener {}
