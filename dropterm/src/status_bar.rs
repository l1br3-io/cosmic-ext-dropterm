// SPDX-License-Identifier: GPL-3.0-only

//! The status band on GPUI (gpui-status spec): the 28 px strip at
//! the surface's free edge narrating the active terminal — the view
//! half `src/status_bar.rs` is for the libcosmic frontend. The
//! segment model, the fit math, the clock arithmetic, and the laid
//! view-model pass are core's own `dropterm::core::status` module,
//! consumed unchanged; what lives here is the `RenderOnce` view
//! that measures through GPUI's text system and paints over the
//! laid rows — the segments, the separators, the disc and halo,
//! the selection highlight, the hover tint — and the event surface
//! that funnels the band's presses and hovers to the shell's
//! interaction machine (the shell owns the state; the view is
//! stateless, the tab strip's own shape).
//!
//! Scope notes (the GPUI frontend's honest gaps, the tab strip's
//! own): no profile seats yet — every GPUI spawn is a plain shell,
//! so the profile segment resolves absent until a profile spawn
//! path exists — and the idle process name falls back to the
//! config's resolved shell (no per-tab recorded shell on this
//! frontend yet).

use dropterm::core::slots::SlotColor;
use dropterm::core::status::{
    BAND_HEIGHT, BandScriptRunVm, BandVm, DIM_ALPHA, DISC_RING_WIDTH, DISC_SIZE, HALO_GAP,
    HOVER_ALPHA, PasteRefusal, SEGMENT_FONT_SIZE, SELECTION_ALPHA, SEPARATOR_HEIGHT,
    SEPARATOR_WIDTH, SegmentId, StatusSegments, band_vm, hover_segment, segment_at,
    selection_rects,
};

use crate::tab_bar;
use gpui::prelude::*;

/// The styled runs' palette (styled-status-slot): the script-facing
/// color words the band resolves — the scheme's ANSI words, `accent`
/// riding the scheme's `BrightBlue` (`theme_gen`'s own accent mapping),
/// and `fg` the pane-derived foreground the band already paints
/// builtins with. One source, built once per render at the band's
/// composition site; resolution never invents a second theme path.
pub(crate) struct ScriptPalette {
    /// The pane-derived foreground — the unstyled run's color.
    pub fg: gpui::Hsla,
    black: gpui::Hsla,
    red: gpui::Hsla,
    green: gpui::Hsla,
    yellow: gpui::Hsla,
    blue: gpui::Hsla,
    magenta: gpui::Hsla,
    cyan: gpui::Hsla,
    white: gpui::Hsla,
    bright_black: gpui::Hsla,
    bright_red: gpui::Hsla,
    bright_green: gpui::Hsla,
    bright_yellow: gpui::Hsla,
    bright_blue: gpui::Hsla,
    bright_magenta: gpui::Hsla,
    bright_cyan: gpui::Hsla,
    bright_white: gpui::Hsla,
}

/// The palette's known words — the closed set the drain logs
/// unknowns against. Additive: a new word joins here and in the
/// docs row it rides.
pub(crate) const SLOT_TOKENS: [&str; 18] = [
    "accent",
    "fg",
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright_black",
    "bright_red",
    "bright_green",
    "bright_yellow",
    "bright_blue",
    "bright_magenta",
    "bright_cyan",
    "bright_white",
];

impl ScriptPalette {
    /// Resolve from the pane's effective scheme (the same `Colors`
    /// the strip pair derives from) and the band's foreground. A
    /// scheme slot standing empty resolves to the foreground — the
    /// word is known, the scheme just has nothing for it.
    pub fn resolve(colors: &alacritty_terminal::term::color::Colors, fg: gpui::Hsla) -> Self {
        use alacritty_terminal::vte::ansi::NamedColor::{
            Black, Blue, BrightBlack, BrightBlue, BrightCyan, BrightGreen, BrightMagenta,
            BrightRed, BrightWhite, BrightYellow, Cyan, Green, Magenta, Red, White, Yellow,
        };
        let pick = |named: alacritty_terminal::vte::ansi::NamedColor| {
            colors[named].map_or(fg, |rgb| tab_bar::hsla(rgb.r, rgb.g, rgb.b))
        };
        Self {
            fg,
            black: pick(Black),
            red: pick(Red),
            green: pick(Green),
            yellow: pick(Yellow),
            blue: pick(Blue),
            magenta: pick(Magenta),
            cyan: pick(Cyan),
            white: pick(White),
            bright_black: pick(BrightBlack),
            bright_red: pick(BrightRed),
            bright_green: pick(BrightGreen),
            bright_yellow: pick(BrightYellow),
            bright_blue: pick(BrightBlue),
            bright_magenta: pick(BrightMagenta),
            bright_cyan: pick(BrightCyan),
            bright_white: pick(BrightWhite),
        }
    }

    /// The palette's word → its color; an unknown word is `None`
    /// (the caller renders the run unstyled).
    pub fn color(&self, token: &str) -> Option<gpui::Hsla> {
        let color = match token {
            "accent" | "bright_blue" => self.bright_blue,
            "fg" => self.fg,
            "black" => self.black,
            "red" => self.red,
            "green" => self.green,
            "yellow" => self.yellow,
            "blue" => self.blue,
            "magenta" => self.magenta,
            "cyan" => self.cyan,
            "white" => self.white,
            "bright_black" => self.bright_black,
            "bright_red" => self.bright_red,
            "bright_green" => self.bright_green,
            "bright_yellow" => self.bright_yellow,
            "bright_magenta" => self.bright_magenta,
            "bright_cyan" => self.bright_cyan,
            "bright_white" => self.bright_white,
            _ => return None,
        };
        Some(color)
    }
}

/// One styled run the painter mounts: the run's composed text and
/// its resolved color (styled-status-slot).
pub(crate) struct StyledRun {
    pub text: String,
    pub color: gpui::Hsla,
}

/// Paint one script row's runs (styled-status-slot): the resolved
/// color per run — an uncolored run is the row's fg, a token
/// resolves through the palette, a hex parses direct, an unknown
/// token renders unstyled (fg; the drain named it) — and the side's
/// dim alpha applied to every run (the band's own rule, never the
/// script's). No run ever drops for a color failure.
pub(crate) fn paint_script_segment(
    runs: &[BandScriptRunVm],
    side: dropterm::core::status::ScriptSide,
    palette: &ScriptPalette,
) -> Vec<StyledRun> {
    let dim = side == dropterm::core::status::ScriptSide::Right;
    let shade = |color: gpui::Hsla| {
        if dim {
            gpui::Hsla {
                a: DIM_ALPHA,
                ..color
            }
        } else {
            color
        }
    };
    runs.iter()
        .map(|run| {
            let color = match run.color.as_ref() {
                None => palette.fg,
                Some(SlotColor::Token(token)) => palette.color(token).unwrap_or(palette.fg),
                Some(SlotColor::Hex(hex)) => parse_hex(hex).unwrap_or(palette.fg),
            };
            StyledRun {
                text: run.text.clone(),
                color: shade(color),
            }
        })
        .collect()
}

/// Parse the contract's `#rrggbb` form to the band's color. The
/// port validated it; a hand-built bad literal falls back to
/// unstyled at the paint (never a raise).
fn parse_hex(hex: &str) -> Option<gpui::Hsla> {
    let raw = hex.strip_prefix('#')?;
    if raw.len() != 6 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channels = |range: std::ops::Range<usize>| u8::from_str_radix(&raw[range], 16).ok();
    Some(tab_bar::hsla(
        channels(0..2)?,
        channels(2..4)?,
        channels(4..6)?,
    ))
}

/// The drain-side unknown-token lines (styled-status-slot): one per
/// distinct unknown token per segment in one reply — the shape's
/// one-log-line culture applied to color words. The drain prints
/// them; the painter renders unstyled.
pub(crate) fn unknown_token_lines(
    answers: &[dropterm::core::status::SegmentAnswer],
) -> Vec<String> {
    use dropterm::core::slots::SlotContent;
    use dropterm::core::status::SegmentAnswer;
    let mut lines = Vec::new();
    for answer in answers {
        let SegmentAnswer::Shown {
            id,
            side: _,
            content,
        } = answer
        else {
            continue;
        };
        let SlotContent::Styled { spans } = content else {
            continue;
        };
        let mut named = std::collections::HashSet::new();
        for span in spans {
            let Some(SlotColor::Token(token)) = span.color.as_ref() else {
                continue;
            };
            if SLOT_TOKENS.contains(&token.as_str()) || !named.insert(token.clone()) {
                continue;
            }
            lines.push(format!(
                "dropterm: plugins: segment '{id}' colors with the unknown token \
                 '{token}'; it renders unstyled"
            ));
        }
    }
    lines
}

/// One event the band's root surface reports to the shell's
/// interaction machine, band-local coordinates and the press's
/// hit already resolved against the laid rows (the snapshot rides
/// along so the machine's pure calls — selection assembly,
/// hit-testing — walk the very rows the frame painted).
pub(crate) enum BandEvent {
    /// A button press at `(x, y)` band-local (and `(wx, wy)` in
    /// window coordinates — the menu's anchor): the hit segment (a
    /// gap press carries `None`), GPUI's own `click_count` (the
    /// double-click's `>= 2`), and the button.
    Press {
        button: gpui::MouseButton,
        x: f32,
        y: f32,
        wx: f32,
        wy: f32,
        click_count: usize,
        hit: Option<SegmentId>,
    },
    /// The cursor moved over the band (`Some`, band-local) or left
    /// it (`None`) — the hover tint's, the tips', and the reveal's
    /// one input.
    Hover { at: Option<(f32, f32)> },
}

/// The band's event funnel: the machine callback, re-entering the
/// shell with the event and the laid snapshot.
type BandEventFn =
    std::rc::Rc<dyn Fn(BandEvent, std::rc::Rc<BandVm>, &mut gpui::Window, &mut gpui::App)>;

/// The band view: resolved segments plus the strip pair, rendered
/// through one `band_vm` pass over GPUI-shaped widths.
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
    /// The styled runs' palette (styled-status-slot) — the shell
    /// resolves it beside the strip pair, same scheme, same render.
    palette: ScriptPalette,
    /// The band's font — the terminal's own resolved family.
    font: gpui::Font,
    /// Whether the band renders its border hairline on its side
    /// away from the grab bar (the terminal's side).
    border: bool,
    /// Whether the band sits at the column's bottom (the terminal
    /// above it) — the border hairline's side, and the band-local
    /// y origin's (the press conversion's other half).
    at_bottom: bool,
    /// The laid row width the fit decides against — the window's
    /// current width (the band spans it).
    width: f32,
    /// The segment flashing its `✓` (the copy verb's confirm); the
    /// shell owns the timing and hands the fresh value per frame.
    copied: Option<SegmentId>,
    /// The standing paste-refusal verdict (the audit's F7); the
    /// shell owns the timing and hands the fresh value per frame.
    paste_refusal: Option<PasteRefusal>,
    /// The cursor's band-local rest (`None` = away) — the hover
    /// tint's and the inline reveal's input; `None` while a
    /// selection stands or the menu is open (the shell decides,
    /// the one-narrator rule).
    hover: Option<(f32, f32)>,
    /// The standing selection span (`anchor`, `focus`), band-local —
    /// live while the drag runs, persisting after the release until
    /// the next press clears it.
    selection: Option<((f32, f32), (f32, f32))>,
    /// The machine callback: every press and hover, with the laid
    /// snapshot the frame painted.
    on_event: BandEventFn,
}

impl StatusBandView {
    pub(crate) fn new(segments: StatusSegments, strip: gpui::Hsla, fg: gpui::Hsla) -> Self {
        Self {
            segments,
            strip,
            fg,
            palette: ScriptPalette::resolve(
                &alacritty_terminal::term::color::Colors::default(),
                fg,
            ),
            attention: None,
            font: gpui::Font::default(),
            border: false,
            at_bottom: true,
            width: 0.0,
            copied: None,
            paste_refusal: None,
            hover: None,
            selection: None,
            on_event: std::rc::Rc::new(|_, _, _, _: &mut gpui::App| {}),
        }
    }

    pub(crate) fn font(mut self, font: gpui::Font) -> Self {
        self.font = font;
        self
    }

    pub(crate) fn palette(mut self, palette: ScriptPalette) -> Self {
        self.palette = palette;
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

    pub(crate) fn paste_refusal(mut self, paste_refusal: Option<PasteRefusal>) -> Self {
        self.paste_refusal = paste_refusal;
        self
    }

    pub(crate) fn attention(mut self, red: Option<gpui::Hsla>) -> Self {
        self.attention = red;
        self
    }

    pub(crate) fn hover(mut self, hover: Option<(f32, f32)>) -> Self {
        self.hover = hover;
        self
    }

    pub(crate) fn selection(mut self, selection: Option<((f32, f32), (f32, f32))>) -> Self {
        self.selection = selection;
        self
    }

    pub(crate) fn on_event(
        mut self,
        on_event: impl Fn(BandEvent, std::rc::Rc<BandVm>, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_event = std::rc::Rc::new(on_event);
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
        let vm = std::rc::Rc::new(band_vm(
            &self.segments,
            self.width,
            &measure,
            self.copied,
            self.paste_refusal,
            self.hover,
        ));

        let mut band = gpui::div()
            .id("status-band")
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

        // The selection highlight: the span's intersection with the
        // laid rows — pane foreground at a quarter alpha, under the
        // text, no radius (the reference's own mark).
        if let Some((anchor, focus)) = self.selection {
            for rect in selection_rects(anchor, focus, &vm.rows) {
                band = band.child(
                    gpui::div()
                        .absolute()
                        .left(gpui::px(rect.x))
                        .top(gpui::px(rect.y))
                        .w(gpui::px(rect.width))
                        .h(gpui::px(rect.height))
                        .bg(gpui::Hsla {
                            a: SELECTION_ALPHA,
                            ..fg
                        }),
                );
            }
        }

        // The hover tint: the hovered segment's own strip, the menu
        // rows' hover weight — suppressed while a selection stands
        // (one narrator at a time; the shell also clears the hover
        // while the menu is open).
        if let Some(hovered) = hover_segment(&vm.rows, self.hover, self.selection.is_some())
            && let Some(row) = vm.rows.iter().find(|row| row.id == hovered)
        {
            band = band.child(
                gpui::div()
                    .absolute()
                    .left(gpui::px(row.x))
                    .top(gpui::px(0.))
                    .w(gpui::px(row.width))
                    .h(gpui::px(BAND_HEIGHT))
                    .rounded(gpui::px(4.))
                    .bg(gpui::Hsla {
                        a: HOVER_ALPHA,
                        ..fg
                    }),
            );
        }

        // The rows: absolutely placed at their laid rects — the
        // VM's own x math, one element per seat, each its own
        // cursor (the pointer over rendered seats, the default in
        // the gaps — the root carries it).
        for row in &vm.rows {
            let mut text = gpui::div()
                .absolute()
                .left(gpui::px(row.text_x))
                .top(gpui::px(0.))
                .h(gpui::px(BAND_HEIGHT))
                .w(gpui::px(row.width))
                .flex()
                .items_center()
                .font(self.font.clone())
                // The laid rects come from the measure pass at
                // SEGMENT_FONT_SIZE; the divs must paint at the same
                // size or gpui's 20 px default text style overflows
                // every row into its neighbor (the first `just dev`
                // walk's overlap — the tab strip sets its own the
                // same way).
                .text_size(gpui::px(SEGMENT_FONT_SIZE))
                .cursor_pointer();
            text = text.child(row.text.clone());
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
        // The script segments' rows (status-segments spec): painted
        // like any row over their laid rects, but non-interactive in
        // v1 — the default cursor, no hover tint, no copy (the
        // interactions key on SegmentId and never see these). The
        // painter resolves each run's color (styled-status-slot): a
        // string answer's single run paints exactly as before — the
        // row's fg — and the side's dim alpha rides every run.
        for row in &vm.script_rows {
            let mut text = gpui::div()
                .absolute()
                .left(gpui::px(row.text_x))
                .top(gpui::px(0.))
                .h(gpui::px(BAND_HEIGHT))
                .w(gpui::px(row.width))
                .flex()
                .items_center()
                .font(self.font.clone())
                .text_size(gpui::px(SEGMENT_FONT_SIZE));
            let side = if row.dimmed {
                dropterm::core::status::ScriptSide::Right
            } else {
                dropterm::core::status::ScriptSide::Left
            };
            for run in paint_script_segment(&row.runs, side, &self.palette) {
                text = text.child(gpui::div().child(run.text).text_color(run.color));
            }
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
        if let Some(disc) = &vm.disc {
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

        // The event surface (root-level: gaps press too — the Copy
        // row's mute and a gap-anchored drag are the checklist's own
        // cases): every button's press and the hover trail, each
        // converted band-local and hit-tested against the snapshot
        // the frame painted. The y origin is re-derived at event
        // time (the window may have resized since the frame).
        let at_bottom = self.at_bottom;
        let band_y0 = move |window: &gpui::Window| -> f32 {
            let height = window.bounds().size.height.as_f32();
            if at_bottom { height - BAND_HEIGHT } else { 0.0 }
        };
        let on_event = std::rc::Rc::clone(&self.on_event);
        let hover_vm = std::rc::Rc::clone(&vm);
        let hover = {
            move |event: &gpui::MouseMoveEvent, window: &mut gpui::Window, cx: &mut gpui::App| {
                let y0 = band_y0(window);
                let at = (event.position.x.as_f32(), event.position.y.as_f32() - y0);
                on_event(
                    BandEvent::Hover { at: Some(at) },
                    std::rc::Rc::clone(&hover_vm),
                    window,
                    cx,
                );
            }
        };
        let on_event = std::rc::Rc::clone(&self.on_event);
        let leave_vm = std::rc::Rc::clone(&vm);
        let leave = move |window: &mut gpui::Window, cx: &mut gpui::App| {
            on_event(
                BandEvent::Hover { at: None },
                std::rc::Rc::clone(&leave_vm),
                window,
                cx,
            );
        };
        let on_event = std::rc::Rc::clone(&self.on_event);
        let press_vm = std::rc::Rc::clone(&vm);
        let press =
            move |event: &gpui::MouseDownEvent, window: &mut gpui::Window, cx: &mut gpui::App| {
                let y0 = band_y0(window);
                let (x, y, wy) = (
                    event.position.x.as_f32(),
                    event.position.y.as_f32() - y0,
                    event.position.y.as_f32(),
                );
                let hit = segment_at(&press_vm.rows, x, y);
                cx.stop_propagation();
                on_event(
                    BandEvent::Press {
                        button: event.button,
                        x,
                        y,
                        wx: event.position.x.as_f32(),
                        wy,
                        click_count: event.click_count,
                        hit,
                    },
                    std::rc::Rc::clone(&press_vm),
                    window,
                    cx,
                );
            };
        band = band
            .on_mouse_move(hover)
            .on_hover(
                move |hovered: &bool, window: &mut gpui::Window, cx: &mut gpui::App| {
                    // The leave report: the hover tint, the tips, and
                    // the reveal all disarm on absence — `on_hover`'s
                    // false edge is the band's own leave event.
                    if !*hovered {
                        leave(window, cx);
                    }
                },
            )
            .on_mouse_down(gpui::MouseButton::Left, {
                let press = press.clone();
                move |event, window, cx| press(event, window, cx)
            })
            .on_mouse_down(gpui::MouseButton::Middle, {
                let press = press.clone();
                move |event, window, cx| press(event, window, cx)
            })
            .on_mouse_down(gpui::MouseButton::Right, {
                let press = press.clone();
                move |event, window, cx| press(event, window, cx)
            });

        band
    }
}

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

#[cfg(test)]
mod tests;
