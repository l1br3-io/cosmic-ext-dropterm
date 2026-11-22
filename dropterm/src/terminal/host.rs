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

use dropterm::core::terminal::TextHost;

/// The monospace families the pane tries, in order: the platform
/// natives lead (each absent where it is not native, so the entry
/// never fires there — an unconfigured pick matches the OS's own
/// expectations: Menlo on macOS, Consolas on Windows), then the
/// common distribution monospaces of the Linux platforms (Noto Sans
/// Mono is cosmic-text's hard-wired `Family::Monospace` default, the
/// family the libcosmic build's cells resolve to where it is
/// installed). The first family GPUI's font system actually holds
/// wins — an unheld family does not fail resolution loudly, it
/// silently fails to rasterize glyphs (the first walk's lesson on
/// the validation VM: quads painted, no glyph ever landed). The
/// walk's terminal fallback is the bundled family
/// ([`crate::fonts::BUNDLED_FAMILY`]), held by construction once the
/// boot registration ran — the one candidate no platform needs
/// installed.
pub(crate) const MONOSPACE_CANDIDATES: [&str; 10] = [
    "Menlo",
    "SF Mono",
    "Monaco",
    "Consolas",
    "Cascadia Mono",
    "Noto Sans Mono",
    "DejaVu Sans Mono",
    "JetBrains Mono",
    "Fira Mono",
    "Liberation Mono",
];

/// The walk, pure: the first listed family `held` carries, else the
/// bundled family — the boot registration's guarantee, so no path can
/// return a name nothing holds (the silent-failure state the first
/// walk's lesson pins is unreachable by construction). The tests pin
/// the ordering policy through this function directly.
fn select_monospace_family(held: &impl Fn(&str) -> bool) -> String {
    for candidate in MONOSPACE_CANDIDATES {
        if held(candidate) {
            return candidate.to_string();
        }
    }
    crate::fonts::BUNDLED_FAMILY.to_string()
}

/// The first candidate family GPUI's font system holds, else the
/// bundled family verbatim (held once the boot registration ran; if
/// registration itself failed, the boot's logged row makes that state
/// visible and the pick rides the system's fonts as before the
/// bundle).
pub(crate) fn resolve_monospace_family(text_system: &gpui::TextSystem) -> String {
    let names = text_system.all_font_names();
    select_monospace_family(&|name| names.iter().any(|n| n == name))
}

/// The configured `font_name` resolved against GPUI's held fonts: a
/// non-empty name the font system actually holds wins (the picker's
/// rows are fontdb families, the same names GPUI resolves through);
/// anything else — empty, unheld — falls back to the candidate walk
/// above, because an unheld family silently fails to rasterize (the
/// resolution rule `resolve_monospace_family`'s doc pins). The one
/// resolution both a pane boot and a live font edit run, so every
/// render surface lands on the same family.
pub(crate) fn resolve_family(text_system: &gpui::TextSystem, configured: Option<&str>) -> String {
    if let Some(wanted) = configured {
        if text_system
            .all_font_names()
            .iter()
            .any(|name| name == wanted)
        {
            return wanted.to_string();
        }
        println!("dropterm: configured font {wanted:?} not held — falling back");
    }
    resolve_monospace_family(text_system)
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
    /// before anything renders. `configured` is the config's
    /// `font_name` — resolved against the held fonts
    /// ([`resolve_family`]), so a pane boots in the family the
    /// settings panel picked whenever GPUI holds it.
    pub(crate) fn new(cx: &App, configured: Option<&str>) -> Self {
        let text_system = cx.text_system().clone();
        let family = resolve_family(&text_system, configured);
        println!("dropterm: pane font family resolved to {family:?}");
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

    fn set_family(&mut self, family: &str) -> bool {
        // The input is pre-resolved (the caller ran
        // [`resolve_family`]), so it lands verbatim — `true` only on
        // a real change, so the engine's re-measure fires once.
        if self.font.family.as_ref() == family {
            return false;
        }
        self.font.family = family.into();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::MONOSPACE_CANDIDATES;
    use crate::fonts::BUNDLED_FAMILY;

    /// The headless selection over a plain name list — the real walk
    /// function against a synthetic held set, so the tests pin the
    /// resolver's policy, not a helper's copy of it.
    fn select(held: &[&str]) -> String {
        super::select_monospace_family(&|name| held.contains(&name))
    }

    #[test]
    fn the_cosmic_text_default_wins_where_held() {
        // On Linux none of the platform natives is held, so the walk
        // lands on Noto Sans Mono — cosmic-text's own `Family::
        // Monospace` default and the bundled family, parity with the
        // libcosmic cells exact.
        assert_eq!(
            select(&["DejaVu Sans Mono", "Noto Sans Mono"]),
            "Noto Sans Mono"
        );
    }

    #[test]
    fn the_platform_natives_win_where_held() {
        // An unconfigured pick matches the OS's expectations: the
        // macOS natives lead the walk, then Windows'. The Linux list
        // follows — absent everywhere but Linux, so the other
        // platforms' natives resolve first without disturbing this
        // platform's order.
        assert_eq!(
            select(&[
                "DejaVu Sans Mono",
                "Noto Sans Mono",
                "Menlo",
                "SF Mono",
                "Monaco",
                "Consolas",
                "Cascadia Mono",
            ]),
            "Menlo"
        );
        assert_eq!(select(&["Consolas", "Noto Sans Mono"]), "Consolas");
        assert_eq!(select(&["Cascadia Mono"]), "Cascadia Mono");
        assert_eq!(select(&["SF Mono", "Monaco"]), "SF Mono");
        assert_eq!(select(&["Monaco"]), "Monaco");
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
    fn a_font_less_system_lands_on_the_bundled_family() {
        // The bare rig holds nothing (the plan record's container row:
        // `fc-list` empty). The walk's terminal fallback is the
        // registered bundled family — held by construction once the
        // boot ran, so the pane renders glyphs instead of the
        // silent-failure state (quads painted, none rasterized).
        assert_eq!(select(&[]), BUNDLED_FAMILY);
    }

    #[test]
    fn a_held_non_candidate_never_matches() {
        // Only the candidate list is eligible; a system holding none
        // of them keeps the bundled default (the logged row at boot
        // and the resolver's println make that state visible in a
        // walk).
        assert_eq!(select(&["Comic Sans"]), BUNDLED_FAMILY);
    }

    #[test]
    fn the_candidates_are_all_monospace_and_ordered() {
        // The order is the policy: the platform natives lead (each
        // absent where it is not native, so it never fires there),
        // then the Linux distribution monospaces, the bundled family
        // behind them all as the guaranteed last resort.
        assert_eq!(
            MONOSPACE_CANDIDATES,
            [
                "Menlo",
                "SF Mono",
                "Monaco",
                "Consolas",
                "Cascadia Mono",
                "Noto Sans Mono",
                "DejaVu Sans Mono",
                "JetBrains Mono",
                "Fira Mono",
                "Liberation Mono",
            ]
        );
        // The terminal fallback and the configured default are the
        // bundled family — the registration's guarantee is the walk's.
        assert_eq!(BUNDLED_FAMILY, "Noto Sans Mono");
    }
}
