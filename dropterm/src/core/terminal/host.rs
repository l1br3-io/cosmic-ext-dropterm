// SPDX-License-Identifier: GPL-3.0-only

//! The font-and-buffer seam (shared-core spec): everything the engine
//! does that needs a **live font system** — owning the cosmic-text
//! buffer, measuring the cell out of the space glyph, shaping
//! projected lines — lives behind [`TextHost`], implemented by the
//! frontend against its toolkit's font system (iced's global for the
//! libcosmic widget). The engine stays toolkit-free: it projects grid
//! state into whatever buffer the host holds and asks the host to
//! shape. cosmic-text's plain-data types (`Buffer`, `Attrs`,
//! `Metrics`, `Color`) cross this seam freely — the crate is a text
//! shaper, not a toolkit, and GPUI's own stack is cosmic-text-based
//! at this pin (spike record, check 9).

use std::sync::Weak;

use cosmic_text::{Attrs, Buffer, Metrics};

/// The frontend-injected text host: owns the engine's cosmic-text
/// buffer and answers every font-system question the engine has.
/// Production builds inject the libcosmic frontend's host (iced's
/// global font system); the engine's tests inject [`PlainHost`]
/// (cosmic-text's own font system — the shaper without the toolkit).
pub trait TextHost: Send {
    /// The buffer the engine projects into and the renderer reads.
    fn buffer(&self) -> &Buffer;

    /// Mutable access for the projection and the metrics applies.
    fn buffer_mut(&mut self) -> &mut Buffer;

    /// A weak handle for the renderer.
    fn buffer_weak(&self) -> Weak<Buffer>;

    /// The buffer's redraw flag — the engine's damage answer.
    fn redraw(&self) -> bool;

    /// Create the buffer under `attrs`/`metrics` and measure the
    /// space-glyph cell: `(cell_width, line_height)`. The font system
    /// decides the cell size; the engine builds its grid arithmetic
    /// on the answer.
    fn init_and_measure(&mut self, attrs: &Attrs, metrics: &Metrics) -> (f32, f32);

    /// Measure the space-glyph cell under the buffer's current
    /// metrics (a font change's re-measure).
    fn remeasure(&mut self, attrs: &Attrs) -> (f32, f32);

    /// Whether `metrics` differs from the buffer's applied metrics —
    /// the `set_config` change gate.
    fn metrics_differ(&self, metrics: &Metrics) -> bool;

    /// Shape recently-projected lines through the toolkit's font
    /// system, trimming its run cache. Called once per update, after
    /// the projection.
    fn shape(&mut self);
}

/// The test host: a persistent real buffer shaped by cosmic-text's
/// own font system — the shaper without the toolkit. The engine's
/// projection tests assert on real buffer lines, so the buffer must
/// persist across borrows exactly as the frontend's does; one shared
/// font system serves every test's terminal (the same sharing iced's
/// global provides in production).
#[cfg(test)]
pub struct PlainHost {
    buffer: std::sync::Arc<Buffer>,
}

#[cfg(test)]
impl Default for PlainHost {
    fn default() -> Self {
        Self {
            buffer: std::sync::Arc::new(Buffer::new_empty(Metrics::new(16.0, 8.0))),
        }
    }
}

/// The tests' shared font system — one font scan for the whole suite.
#[cfg(test)]
use std::sync::{LazyLock, Mutex};

#[cfg(test)]
use cosmic_text::Shaping;

#[cfg(test)]
use cosmic_text::Wrap;

#[cfg(test)]
static FONT_SYSTEM: LazyLock<Mutex<cosmic_text::FontSystem>> =
    LazyLock::new(|| Mutex::new(cosmic_text::FontSystem::new()));

#[cfg(test)]
impl PlainHost {
    fn measure(&mut self, attrs: &Attrs) -> (f32, f32) {
        let buffer = std::sync::Arc::make_mut(&mut self.buffer);
        buffer.set_wrap(Wrap::None);
        buffer.set_text(" ", attrs, Shaping::Advanced, None);
        let mut font_system = FONT_SYSTEM.lock().expect("test font system");
        let layout = buffer
            .line_layout(&mut font_system, 0)
            .expect("space glyph layout");
        let w = layout[0].w;
        buffer.set_monospace_width(Some(w));
        (w, buffer.metrics().line_height)
    }
}

#[cfg(test)]
impl TextHost for PlainHost {
    fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    fn buffer_mut(&mut self) -> &mut Buffer {
        std::sync::Arc::make_mut(&mut self.buffer)
    }

    fn buffer_weak(&self) -> Weak<Buffer> {
        std::sync::Arc::downgrade(&self.buffer)
    }

    fn redraw(&self) -> bool {
        self.buffer.redraw()
    }

    fn init_and_measure(&mut self, attrs: &Attrs, metrics: &Metrics) -> (f32, f32) {
        self.buffer = std::sync::Arc::new(Buffer::new_empty(*metrics));
        self.measure(attrs)
    }

    fn remeasure(&mut self, attrs: &Attrs) -> (f32, f32) {
        self.measure(attrs)
    }

    fn metrics_differ(&self, metrics: &Metrics) -> bool {
        *metrics != self.buffer.metrics()
    }

    fn shape(&mut self) {
        let buffer = std::sync::Arc::make_mut(&mut self.buffer);
        let mut font_system = FONT_SYSTEM.lock().expect("test font system");
        buffer.shape_until_scroll(&mut font_system, true);
    }
}
