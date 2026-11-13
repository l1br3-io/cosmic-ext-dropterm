// SPDX-License-Identifier: GPL-3.0-only

//! The GPUI text host (gpui-terminal Task 1): the engine's
//! [`TextHost`] seam implemented against **GPUI's own font system** —
//! the same system the view renders through — so the cell grid the
//! engine's arithmetic builds on and the glyphs the view paints are
//! measured by one shaper, consistent by construction.
//!
//! The host still owns a real cosmic-text `Buffer` for the engine's
//! projection (the seam's data type, and the projection is engine
//! state advancement, not render output), but nothing on this
//! frontend reads it back: check 9's ruling holds, the view renders
//! from the grid ([`super::grid`]), and [`TextHost::shape`] is
//! therefore a documented no-op here — the trait's contract serves
//! renderers that read the buffer, and this frontend has none.

use std::sync::Arc;

use cosmic_text::{Attrs, Buffer, Metrics};
use gpui::{App, FontStyle, FontWeight, Pixels, px};

use dropterm_core::terminal::TextHost;

/// The monospace families the pane tries, in order: cosmic-text's
/// own hard-wired `Family::Monospace` default first (the family the
/// libcosmic build's cells resolve to where it is installed), then
/// the common distribution monospaces. The first family GPUI's font
/// system actually holds wins — an unheld family does not fail
/// resolution loudly, it silently fails to rasterize glyphs (the
/// first walk's lesson on the validation VM: quads painted, no
/// glyph ever landed).
pub(crate) const MONOSPACE_CANDIDATES: [&str; 5] = [
    "Noto Sans Mono",
    "DejaVu Sans Mono",
    "JetBrains Mono",
    "Fira Mono",
    "Liberation Mono",
];

/// The first candidate family GPUI's font system holds, or the
/// cosmic-text default verbatim when none is listed (the resolver's
/// own fallback then owns the pick — visible as the logged row).
pub(crate) fn resolve_monospace_family(text_system: &gpui::TextSystem) -> String {
    let names = text_system.all_font_names();
    for candidate in MONOSPACE_CANDIDATES {
        if names.iter().any(|name| name == candidate) {
            return candidate.to_string();
        }
    }
    MONOSPACE_CANDIDATES[0].to_string()
}

/// The GPUI-side host: a real cosmic-text buffer for the engine's
/// projection, measured through GPUI's text system.
pub(crate) struct GpuiHost {
    buffer: Arc<Buffer>,
    text_system: Arc<gpui::TextSystem>,
    /// The render font the measurement resolves through — the base
    /// font the view also paints with (weight/style refined per run).
    font: gpui::Font,
}

impl GpuiHost {
    /// Built at boot with the app-level text system; the engine's
    /// `Terminal::new` initializes and measures through the trait
    /// before anything renders.
    pub(crate) fn new(cx: &App) -> Self {
        let text_system = cx.text_system().clone();
        let family = resolve_monospace_family(&text_system);
        println!("dropterm-gpui: pane font family resolved to {family:?}");
        Self {
            buffer: Arc::new(Buffer::new_empty(Metrics::new(16.0, 8.0))),
            font: gpui::Font {
                family: family.into(),
                ..gpui::Font::default()
            },
            text_system,
        }
    }

    /// The base render font (the view paints runs as weight/style
    /// refinements of this).
    pub(crate) fn font(&self) -> gpui::Font {
        self.font.clone()
    }

    /// Measure the space-glyph cell under the buffer's current
    /// metrics through GPUI's text system: the advance of the space
    /// glyph at the live size, with core's own line-height formula
    /// (the metrics the engine passed in) as the cell height.
    fn measure(&mut self, attrs: &Attrs) -> (f32, f32) {
        // The engine's attrs carry the configured weight/style (both
        // plain cosmic-text fields); the family is the monospace
        // constant above (the engine's own default attrs are
        // `Family::Monospace`).
        self.font.weight = FontWeight(f32::from(attrs.weight.0));
        self.font.style = if attrs.style == cosmic_text::Style::Italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };
        let metrics = self.buffer.metrics();
        let font_id = self.text_system.resolve_font(&self.font);
        let size = Pixels::from(metrics.font_size);
        let advance = self
            .text_system
            .advance(font_id, size, ' ')
            .map_or(px(8.), |advance| advance.width);
        (advance.as_f32(), metrics.line_height)
    }
}

impl TextHost for GpuiHost {
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
        // Deliberately nothing: the engine's projection still writes
        // the buffer (its own state advancement), but this frontend
        // never renders from it — check 9's ruling — so shaping the
        // lines would be work with no reader. The libcosmic host
        // shapes because iced's renderer reads the buffer back.
    }
}

#[cfg(test)]
mod tests {
    use super::MONOSPACE_CANDIDATES;

    /// The headless selection over a plain name list — the family
    /// choice without GPUI's text system (the candidate order is the
    /// policy; the test pins it).
    fn select(held: &[&str]) -> String {
        for candidate in MONOSPACE_CANDIDATES {
            if held.contains(&candidate) {
                return candidate.to_string();
            }
        }
        MONOSPACE_CANDIDATES[0].to_string()
    }

    #[test]
    fn the_cosmic_text_default_wins_where_held() {
        // Noto Sans Mono is cosmic-text's own `Family::Monospace`
        // default — where the system holds it, parity with the
        // libcosmic cells is exact.
        assert_eq!(
            select(&["DejaVu Sans Mono", "Noto Sans Mono"]),
            "Noto Sans Mono"
        );
    }

    #[test]
    fn an_unheld_default_falls_through_to_a_held_monospace() {
        // The validation VM holds none of Noto: the first held
        // candidate takes the pane (the walk's silent-glyph lesson).
        assert_eq!(
            select(&["Some Prop Font", "DejaVu Sans Mono", "Fira Mono"]),
            "DejaVu Sans Mono"
        );
        assert_eq!(select(&["Fira Mono"]), "Fira Mono");
    }

    #[test]
    fn a_held_non_candidate_never_matches() {
        // Only the candidate list is eligible; a system holding none
        // of them keeps the cosmic-text default and lets the
        // resolver's own fallback own the pick (the logged row makes
        // that state visible in a walk).
        assert_eq!(select(&["Comic Sans"]), "Noto Sans Mono");
    }

    #[test]
    fn the_candidates_are_all_monospace_and_ordered() {
        // The order is the policy: cosmic-text's default, then the
        // common distribution monospaces.
        assert_eq!(
            MONOSPACE_CANDIDATES,
            [
                "Noto Sans Mono",
                "DejaVu Sans Mono",
                "JetBrains Mono",
                "Fira Mono",
                "Liberation Mono",
            ]
        );
    }
}
