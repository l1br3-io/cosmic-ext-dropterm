// SPDX-License-Identifier: GPL-3.0-only

//! The status band on GPUI (gpui-status spec): the 28 px strip at
//! the surface's free edge narrating the active terminal — the view
//! half `src/status_bar.rs` is for the libcosmic frontend. The
//! segment model, the fit math, and the clock arithmetic are core's
//! own `dropterm_core::status` module (moved by this slice), consumed
//! unchanged; what lives here is the view-model pass (resolved
//! segments plus measured widths → the laid rows, separators, disc,
//! and halo — pure, headless-pinned) and the `RenderOnce` view that
//! measures through GPUI's text system and paints over it.
//!
//! Scope notes (the GPUI frontend's honest gaps, the tab strip's
//! own): no profile seats yet — every GPUI spawn is a plain shell,
//! so the profile segment resolves absent until a profile spawn
//! path exists — and the idle process name falls back to the
//! config's resolved shell (no per-tab recorded shell on this
//! frontend yet). The OSC-fed seats (reported cwd, the command
//! timer's verdicts, progress, program status) ride Task 2's wiring.

use dropterm_core::status::{
    BAND_HEIGHT, LEFT_CLUSTER, PathFit, PathMeasure, RIGHT_CLUSTER, SEGMENT_GAP, SegmentId,
    StatusSegments, band_fit, click_jumps, copy_text, duration_display, elide_path_left,
    exit_display, pasted_display, prefixed_display, process_display, program_needs_attention,
    progress_display, scrolled_display,
};
use dropterm_core::terminal::ProgressState;

use crate::tab_bar;
use gpui::prelude::*;

/// Segment text size in logical pixels — one notch below the tab
/// strip's 14 px titles: the band is the terminal speaking about
/// itself, in its own voice (the reference band's own constant).
const SEGMENT_FONT_SIZE: f32 = 12.0;

/// The state disc's diameter, and the hollow ring's stroke.
const DISC_SIZE: f32 = 8.0;
const DISC_RING_WIDTH: f32 = 1.5;

/// The attention halo's clearance outside the disc — the ring sits
/// this far beyond the disc's edge so it rings the disc, never
/// overdraws it.
const HALO_GAP: f32 = 1.5;

/// Side padding of the band's clusters.
const PAD_X: f32 = 12.0;

/// The inter-segment hairline's painted size: one pixel wide, half
/// the band tall, centered.
const SEPARATOR_WIDTH: f32 = 1.0;
const SEPARATOR_HEIGHT: f32 = 12.0;

/// The gap between the disc and the process name.
const DISC_GAP: f32 = 6.0;

/// The dimmed alpha the right cluster's segments — and the
/// duration's own narration — carry: hierarchy by alpha, never a
/// new hue.
const DIM_ALPHA: f32 = 0.6;

/// One laid row: a segment's hit rect (`x`/`width`, band-local),
/// where its text paints (`text_x`, the disc's gap ahead of the
/// process name), and its paint flags — the dim seat and the
/// attention-red seat.
pub(crate) struct BandRowVm {
    pub id: SegmentId,
    pub text: String,
    pub x: f32,
    pub width: f32,
    pub text_x: f32,
    pub dimmed: bool,
    /// The attention seat (the exit chip, error-state progress):
    /// painted in the scheme's red, the copy payload unaffected.
    pub attention: bool,
}

/// The process disc: its x (band-local), fill state, and the
/// program report's attention tint.
pub(crate) struct DiscVm {
    pub x: f32,
    pub filled: bool,
    pub attention: bool,
}

/// The band's laid view-model: rows in x-order (left cluster then
/// right), the separator hairlines' gap-center positions, the disc,
/// and the missed-activity halo's center — pure over the resolved
/// segments and measured widths, deterministic for a fixed input.
pub(crate) struct BandVm {
    pub rows: Vec<BandRowVm>,
    pub separators: Vec<f32>,
    pub disc: Option<DiscVm>,
    /// The halo's center x — `None` when no missed activity stands.
    pub halo_center_x: Option<f32>,
}

/// The band's one layout pass: the reference band's fit + rect
/// rules over measured widths — the path elides first, segments drop
/// in [`DROP_ORDER` (the fit's accounting seats: the branch and the
/// right cluster, exactly the reference's set — the command
/// narration seats ride outside it, never dropped, the reference's
/// own construction), survivors keeping their cluster seats. The
/// right cluster anchors at the band's right padding, one
/// `SEGMENT_GAP` before each seat.
#[expect(
    clippy::too_many_lines,
    reason = "the reference band's three laid passes fused into one — displays, fit, and rects read as one rule"
)]
pub(crate) fn band_vm(
    segments: &StatusSegments,
    width: f32,
    measure: &dyn Fn(&str) -> f32,
    copied: Option<SegmentId>,
) -> BandVm {
    // The copy flash's swap (the reference's own rule): the ✓ takes
    // the copied segment's display seat alone — measured like any
    // text, so the row re-lays for its flash — while the path's own
    // fit measure below keeps the steady value (the flash never
    // re-fits the path).
    let display = |id: SegmentId, text: String| {
        if copied == Some(id) {
            "\u{2713}".to_string()
        } else {
            text
        }
    };
    // The display texts, glyphs riding (the copy payloads stay bare
    // values in core's copy_text — never these).
    let process_text = display(
        SegmentId::Process,
        process_display(&segments.process, segments.program_status),
    );
    let duration_text = display(
        SegmentId::Duration,
        prefixed_display(SegmentId::Duration, &duration_display(segments)),
    );
    let exit_text = segments
        .exit_code
        .map(|code| display(SegmentId::Exit, exit_display(code)));
    let progress_text = segments.progress.map(|progress| {
        display(
            SegmentId::Progress,
            prefixed_display(SegmentId::Progress, &progress_display(progress)),
        )
    });
    let path_value = prefixed_display(SegmentId::Path, segments.path.as_deref().unwrap_or(""));
    let branch = display(
        SegmentId::Branch,
        prefixed_display(SegmentId::Branch, segments.branch.as_deref().unwrap_or("")),
    );
    let profile = prefixed_display(
        SegmentId::Profile,
        segments.profile.as_deref().unwrap_or(""),
    );
    let grid = segments.grid.map(|(cols, rows)| {
        display(
            SegmentId::Grid,
            prefixed_display(SegmentId::Grid, &format!("{cols}×{rows}")),
        )
    });
    let tab = segments.tab_position.map(|(index, total)| {
        display(
            SegmentId::Tab,
            prefixed_display(SegmentId::Tab, &format!("{index}/{total}")),
        )
    });
    let zoom = segments.zoom_percent.map(|percent| {
        display(
            SegmentId::Zoom,
            prefixed_display(SegmentId::Zoom, &format!("{percent:+}%")),
        )
    });
    let scrolled = segments
        .scrolled
        .map(|lines| display(SegmentId::Scrolled, scrolled_display(lines)));
    let pasted = segments.pasted.map(|count| {
        display(
            SegmentId::Pasted,
            prefixed_display(SegmentId::Pasted, &pasted_display(count)),
        )
    });
    let clock = display(
        SegmentId::Clock,
        prefixed_display(SegmentId::Clock, segments.clock.as_deref().unwrap_or("")),
    );

    // The fit's accounting seats: the branch plus the right
    // cluster, measured — the process cluster and the command
    // narration ride `fixed`-side or unaccounted, exactly the
    // reference's construction.
    let mut widths: Vec<(SegmentId, f32)> = Vec::new();
    let mut right_cluster: Vec<(SegmentId, String)> = Vec::new();
    for (id, text) in [
        (SegmentId::Profile, profile),
        (SegmentId::Grid, grid.unwrap_or_default()),
        (SegmentId::Tab, tab.unwrap_or_default()),
        (SegmentId::Zoom, zoom.unwrap_or_default()),
        (SegmentId::Scrolled, scrolled.unwrap_or_default()),
        (SegmentId::Pasted, pasted.unwrap_or_default()),
        (SegmentId::Clock, clock),
    ] {
        if !text.is_empty() {
            widths.push((id, measure(&text)));
            right_cluster.push((id, text));
        }
    }
    if !branch.is_empty() {
        widths.push((SegmentId::Branch, measure(&branch)));
    }
    let process_width = measure(&process_text);
    let fixed = if process_width > 0.0 {
        PAD_X + DISC_SIZE + DISC_GAP + process_width + SEGMENT_GAP + PAD_X
    } else {
        PAD_X + SEGMENT_GAP + PAD_X
    };
    let fit = band_fit(
        width,
        fixed,
        &widths,
        PathMeasure {
            // The steady value measures the fit (the reference's
            // rule: the flash's swap never re-fits the path).
            chars: path_value.chars().count(),
            full_width: measure(&path_value),
        },
    );
    // The fit's verdicts: dropped seats compose no row.
    let dropped = |id: SegmentId| fit.dropped.contains(&id);

    // The laid rows — the reference's rect walk: the process
    // cluster from the left padding, the narration seats after it,
    // the right cluster anchored at the right padding with one gap
    // before each seat.
    let mut rows: Vec<BandRowVm> = Vec::new();
    let mut disc = None;
    let mut x = PAD_X;
    if process_width > 0.0 {
        disc = Some(DiscVm {
            x,
            filled: segments.running,
            attention: program_needs_attention(segments.program_status),
        });
        rows.push(BandRowVm {
            id: SegmentId::Process,
            text: process_text.clone(),
            x,
            width: DISC_SIZE + DISC_GAP + process_width,
            text_x: x + DISC_SIZE + DISC_GAP,
            dimmed: false,
            attention: false,
        });
        x += DISC_SIZE + DISC_GAP + process_width + SEGMENT_GAP;
    }
    let path_text = match fit.path {
        PathFit::Full => path_value.clone(),
        PathFit::Elided(max_chars) => elide_path_left(&path_value, max_chars),
        PathFit::Gone => String::new(),
    };
    let path_text = display(SegmentId::Path, path_text);
    let narration = [
        (SegmentId::Duration, duration_text),
        (SegmentId::Exit, exit_text.unwrap_or_default()),
        (SegmentId::Progress, progress_text.unwrap_or_default()),
        (SegmentId::Path, path_text),
        (SegmentId::Branch, branch),
    ];
    let progress_is_error = segments
        .progress
        .is_some_and(|progress| progress.state == ProgressState::Error);
    for (id, text) in narration {
        if text.is_empty() || dropped(id) {
            continue;
        }
        let w = measure(&text);
        rows.push(BandRowVm {
            id,
            text,
            x,
            width: w,
            text_x: x,
            dimmed: id == SegmentId::Duration,
            attention: id == SegmentId::Exit || (id == SegmentId::Progress && progress_is_error),
        });
        x += w + SEGMENT_GAP;
    }
    let mut right = width - PAD_X;
    for (id, text) in right_cluster.iter().rev() {
        if dropped(*id) {
            continue;
        }
        let w = measure(text);
        right -= w;
        rows.push(BandRowVm {
            id: *id,
            text: text.clone(),
            x: right,
            width: w,
            text_x: right,
            dimmed: true,
            attention: false,
        });
        right -= SEGMENT_GAP;
    }
    // x-order: the left cluster composed first, the right cluster's
    // seats accumulated right-to-left — reverse them back.
    let right_rows = right_cluster.iter().filter(|(id, _)| !dropped(*id)).count();
    let right_start = rows.len() - right_rows;
    rows[right_start..].reverse();

    // The separators: one between each pair of adjacent rendered
    // seats of the same cluster — never between the clusters, never
    // beside an absent one.
    let mut separators = Vec::new();
    for cluster in [LEFT_CLUSTER.as_slice(), RIGHT_CLUSTER.as_slice()] {
        let mut prev_end: Option<f32> = None;
        for id in cluster {
            let Some(rect) = rows.iter().find(|row| row.id == *id) else {
                continue;
            };
            if let Some(start) = prev_end {
                separators.push(f32::midpoint(start, rect.x));
            }
            prev_end = Some(rect.x + rect.width);
        }
    }

    let halo_center_x = segments.missed_activity.then(|| {
        disc.as_ref()
            .map_or(PAD_X / 2.0, |disc| disc.x + DISC_SIZE / 2.0)
    });
    BandVm {
        rows,
        separators,
        disc,
        halo_center_x,
    }
}

/// The copy verb's callback shape: the segment, its full value,
/// and the App context the write needs.
type CopyFn = std::rc::Rc<dyn Fn(SegmentId, String, &mut gpui::App)>;

/// The scrolled chip's jump-verb shape.
type JumpFn = std::rc::Rc<dyn Fn(&mut gpui::App)>;

/// The attention token's GPUI resolution (the reference band's own
/// rule): the live terminal scheme's red — the bright variant on
/// dark strips, the plain on light, the same luminance flip the
/// band's foreground performs over its strip. A chosen variant the
/// scheme leaves unset falls back to its sibling, and a scheme with
/// no red at all degrades to the pane foreground — monochrome,
/// never an invented hue.
pub(crate) fn attention_color(
    scheme_red: Option<gpui::Hsla>,
    scheme_bright_red: Option<gpui::Hsla>,
    pane_fg: gpui::Hsla,
    strip: gpui::Hsla,
) -> gpui::Hsla {
    let rgba = gpui::Rgba::from(strip);
    let (chosen, sibling) = if tab_bar::relative_luminance(rgba.r, rgba.g, rgba.b) > 0.5 {
        (scheme_red, scheme_bright_red)
    } else {
        (scheme_bright_red, scheme_red)
    };
    chosen.or(sibling).unwrap_or(pane_fg)
}

/// The band view: resolved segments plus the strip pair, rendered
/// through one [`band_vm`] pass over GPUI-shaped widths.
#[derive(IntoElement)]
pub(crate) struct StatusBandView {
    segments: StatusSegments,
    /// The strip background and foreground — the pane-derived pair
    /// the tab strip already uses.
    strip: gpui::Hsla,
    fg: gpui::Hsla,
    /// The scheme's attention red pair, resolved for the strip's
    /// luminance — `None` degrades the red seats to the foreground,
    /// the reference's monochrome floor.
    attention: Option<gpui::Hsla>,
    /// The band's font — the terminal's own resolved family.
    font: gpui::Font,
    /// Whether the band renders its border hairline on its side
    /// away from the grab bar (the terminal's side).
    border: bool,
    /// Whether the band sits at the column's bottom (the terminal
    /// above it) — the border hairline's side.
    at_bottom: bool,
    /// The laid row width the fit decides against — the window's
    /// current width (the band spans it).
    width: f32,
    /// The segment flashing its `✓` (the copy verb's confirm); the
    /// shell owns the timing and hands the fresh value per frame.
    copied: Option<SegmentId>,
    /// The copy verb: `(segment, full value)` — the shell writes the
    /// clipboard and arms the flash (the App context rides the
    /// callback, the strip's own pattern).
    on_copy: CopyFn,
    /// The scrolled chip's jump verb — the shell routes it to the
    /// engine's scroll-to-bottom, the one deliberate action chip
    /// this frontend carries (the pasted chip's panel arrives with
    /// the chrome slices; its seat resolves absent until then).
    on_scroll_bottom: JumpFn,
}

impl StatusBandView {
    pub(crate) fn new(segments: StatusSegments, strip: gpui::Hsla, fg: gpui::Hsla) -> Self {
        Self {
            segments,
            strip,
            fg,
            attention: None,
            font: gpui::Font::default(),
            border: false,
            at_bottom: true,
            width: 0.0,
            copied: None,
            on_copy: std::rc::Rc::new(|_, _, _: &mut gpui::App| {}),
            on_scroll_bottom: std::rc::Rc::new(|_: &mut gpui::App| {}),
        }
    }

    pub(crate) fn font(mut self, font: gpui::Font) -> Self {
        self.font = font;
        self
    }

    pub(crate) fn show_border(mut self, show: bool) -> Self {
        self.border = show;
        self
    }

    pub(crate) fn at_bottom(mut self, at_bottom: bool) -> Self {
        self.at_bottom = at_bottom;
        self
    }

    pub(crate) fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub(crate) fn copied(mut self, copied: Option<SegmentId>) -> Self {
        self.copied = copied;
        self
    }

    pub(crate) fn attention(mut self, red: Option<gpui::Hsla>) -> Self {
        self.attention = red;
        self
    }

    pub(crate) fn on_copy(
        mut self,
        on_copy: impl Fn(SegmentId, String, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_copy = std::rc::Rc::new(on_copy);
        self
    }

    pub(crate) fn on_scroll_bottom(mut self, on_jump: impl Fn(&mut gpui::App) + 'static) -> Self {
        self.on_scroll_bottom = std::rc::Rc::new(on_jump);
        self
    }
}

impl gpui::RenderOnce for StatusBandView {
    #[expect(
        clippy::too_many_lines,
        reason = "one frame's whole paint over the VM: rows, separators, disc, halo — the tab strip's render shape"
    )]
    fn render(self, window: &mut gpui::Window, _cx: &mut gpui::App) -> impl gpui::IntoElement {
        let fg = self.fg;
        let attention = self.attention.unwrap_or(fg);
        let font = self.font.clone();
        let text_system = window.text_system().clone();
        let measure = move |text: &str| -> f32 {
            if text.is_empty() {
                return 0.0;
            }
            let run = gpui::TextRun {
                len: text.len(),
                font: font.clone(),
                color: fg,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            text_system
                .shape_line(text.into(), gpui::px(SEGMENT_FONT_SIZE), &[run], None)
                .width
                .as_f32()
        };
        let vm = band_vm(&self.segments, self.width, &measure, self.copied);

        let mut band = gpui::div()
            .relative()
            .w_full()
            .h(gpui::px(BAND_HEIGHT))
            .flex_none()
            .overflow_hidden()
            .bg(self.strip);
        if self.border {
            // The hairline faces the terminal: the band's top edge
            // when it sits at the column's bottom, its bottom edge
            // otherwise — the tab strip's own rule.
            let hairline = gpui::Hsla {
                a: tab_bar::BORDER_ALPHA,
                ..fg
            };
            band = if self.at_bottom {
                band.border_t(gpui::px(1.)).border_color(hairline)
            } else {
                band.border_b(gpui::px(1.)).border_color(hairline)
            };
        }
        // The rows: absolutely placed at their laid rects — the
        // VM's own x math, one element per seat (each its own hit
        // target for the coming click verbs).
        for row in &vm.rows {
            // The click verbs (Task 2): the two action chips jump,
            // every other rendered seat copies its full un-elided
            // value — resolved against the live segments here, at
            // frame-build time (the reference's own pick-time rule).
            let (id, payload) = (row.id, copy_text(row.id, &self.segments));
            let on_copy = std::rc::Rc::clone(&self.on_copy);
            let on_jump = std::rc::Rc::clone(&self.on_scroll_bottom);
            let mut text = gpui::div()
                .absolute()
                .left(gpui::px(row.text_x))
                .top(gpui::px(0.))
                .h(gpui::px(BAND_HEIGHT))
                .w(gpui::px(row.width))
                .flex()
                .items_center()
                .font(self.font.clone());
            text = if click_jumps(id) {
                text.on_mouse_down(
                    gpui::MouseButton::Left,
                    move |_, _: &mut gpui::Window, cx: &mut gpui::App| on_jump(cx),
                )
            } else {
                text.on_mouse_down(
                    gpui::MouseButton::Left,
                    move |_, _: &mut gpui::Window, cx: &mut gpui::App| {
                        on_copy(id, payload.clone(), cx);
                    },
                )
            };
            let mut text = text.child(row.text.clone());
            let color = if row.attention {
                attention
            } else if row.dimmed {
                gpui::Hsla { a: DIM_ALPHA, ..fg }
            } else {
                fg
            };
            text = text.text_color(color);
            band = band.child(text);
        }
        // The separators: thin hairlines centered in the gaps.
        for x in &vm.separators {
            band = band.child(
                gpui::div()
                    .absolute()
                    .left(gpui::px(x - SEPARATOR_WIDTH / 2.0))
                    .top(gpui::px((BAND_HEIGHT - SEPARATOR_HEIGHT) / 2.0))
                    .w(gpui::px(SEPARATOR_WIDTH))
                    .h(gpui::px(SEPARATOR_HEIGHT))
                    .bg(gpui::Hsla {
                        a: tab_bar::BORDER_ALPHA,
                        ..fg
                    }),
            );
        }
        // The disc: filled while a foreground child runs, a hollow
        // ring over the idle shell — tinted when the program's own
        // report asks for eyes.
        if let Some(disc) = vm.disc {
            let color = if disc.attention { attention } else { fg };
            let disc_element = gpui::div()
                .absolute()
                .left(gpui::px(disc.x))
                .top(gpui::px((BAND_HEIGHT - DISC_SIZE) / 2.0))
                .size(gpui::px(DISC_SIZE))
                .rounded_full();
            band = band.child(if disc.filled {
                disc_element.bg(color)
            } else {
                disc_element
                    .border(gpui::px(DISC_RING_WIDTH))
                    .border_color(color)
            });
        }
        // The missed-activity halo: a thin ring `HALO_GAP` beyond
        // the disc's edge — or standing alone at the band's leading
        // edge when the process seat is hidden.
        if let Some(center) = vm.halo_center_x {
            let size = DISC_SIZE + HALO_GAP * 2.0;
            band = band.child(
                gpui::div()
                    .absolute()
                    .left(gpui::px(center - size / 2.0))
                    .top(gpui::px(BAND_HEIGHT / 2.0 - size / 2.0))
                    .size(gpui::px(size))
                    .rounded_full()
                    .border(gpui::px(DISC_RING_WIDTH))
                    .border_color(attention),
            );
        }
        band
    }
}

#[cfg(test)]
mod tests;
