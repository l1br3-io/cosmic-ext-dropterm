// SPDX-License-Identifier: GPL-3.0-only

//! The production text host: iced's global font system owns shaping
//! and measurement, and the buffer lives here with the widget
//! (shared-core spec — the engine stays toolkit-free and asks its
//! host through [`TextHost`]).

use std::sync::Arc;

use cosmic::iced::advanced::graphics::text::font_system;
use cosmic_text::{Attrs, Buffer, Metrics, Shaping, Wrap};

use super::TextHost;

/// The libcosmic frontend's host: one cosmic-text buffer per
/// terminal, shaped by iced's shared global font system.
pub struct CosmicHost {
    buffer: Arc<Buffer>,
}

impl CosmicHost {
    /// An empty host — the engine's `Terminal::new` initializes the
    /// buffer (metrics, the space-glyph cell measure) through
    /// [`TextHost::init_and_measure`] before anything renders, so the
    /// placeholder metrics here never render.
    pub fn new() -> Self {
        Self {
            buffer: Arc::new(Buffer::new_empty(Metrics::new(16.0, 8.0))),
        }
    }

    fn measure(&mut self, attrs: &Attrs) -> (f32, f32) {
        let buffer = Arc::make_mut(&mut self.buffer);
        buffer.set_wrap(Wrap::None);

        // Use size of space to determine cell size
        buffer.set_text(" ", attrs, Shaping::Advanced, None);
        let mut font_system = font_system().write().expect("iced font system");
        let layout = buffer
            .line_layout(font_system.raw(), 0)
            .expect("space glyph layout");
        let w = layout[0].w;
        buffer.set_monospace_width(Some(w));
        (w, buffer.metrics().line_height)
    }
}

impl TextHost for CosmicHost {
    fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    fn buffer_mut(&mut self) -> &mut Buffer {
        Arc::make_mut(&mut self.buffer)
    }

    fn buffer_weak(&self) -> std::sync::Weak<Buffer> {
        Arc::downgrade(&self.buffer)
    }

    fn redraw(&self) -> bool {
        self.buffer.redraw()
    }

    fn init_and_measure(&mut self, attrs: &Attrs, metrics: &Metrics) -> (f32, f32) {
        self.buffer = Arc::new(Buffer::new_empty(*metrics));
        self.measure(attrs)
    }

    fn remeasure(&mut self, attrs: &Attrs) -> (f32, f32) {
        self.measure(attrs)
    }

    fn metrics_differ(&self, metrics: &Metrics) -> bool {
        *metrics != self.buffer.metrics()
    }

    fn shape(&mut self) {
        let buffer = Arc::make_mut(&mut self.buffer);
        let mut font_system = font_system().write().expect("iced font system");
        buffer.shape_until_scroll(font_system.raw(), true);
        font_system.raw().shape_run_cache.trim(1024);
    }
}
