// SPDX-License-Identifier: GPL-3.0-only

//! The grid→run projection (gpui-terminal Task 1): one alacritty
//! grid row projected into styled text runs the GPUI view shapes and
//! paints — colors and attributes resolved off the scheme, the block
//! caret's cell inverted in place, wide-char spacers skipped (the
//! row's shaped width is forced to the cell grid, so a wide glyph
//! naturally claims its two cells). Pure over `alacritty_terminal`
//! data: every rule here is headless-tested; the paint that consumes
//! the runs is compositor-evidenced (the spec's testing strategy).

use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use gpui::{Hsla, rgb};

/// The engine's unset-background fallback, re-declared for the view's
/// own window-background derivation (the same opaque black
/// [`dropterm::core::terminal::engine::WINDOW_BG_COLOR`] serves).
const WINDOW_BG_FALLBACK: Rgb = Rgb {
    r: 0x00,
    g: 0x00,
    b: 0x00,
};

/// One run of same-styled text within a projected row: the unit the
/// paint feeds GPUI's shaper (one `TextRun` per run, one shaped line
/// per row).
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the four flags mirror alacritty's own cell attributes one-to-one; a bitfield would obscure the TextRun mapping"
)]
pub(crate) struct StyledRun {
    /// The run's text — cell chars in order, zero-width combining
    /// marks appended to the cell they decorate, wide-char spacers
    /// absent (the wide glyph claims their cells through the forced
    /// row width).
    pub text: String,
    /// The resolved foreground.
    pub fg: Hsla,
    /// A resolved background the paint fills behind exactly this
    /// run's cells; `None` is the row default (the window background
    /// already covers it).
    pub bg: Option<Hsla>,
    /// `Flags::BOLD` — the paint maps the run onto the bold weight.
    pub bold: bool,
    /// `Flags::ITALIC`.
    pub italic: bool,
    /// `Flags::UNDERLINE`.
    pub underline: bool,
    /// `Flags::STRIKEOUT`.
    pub strikethrough: bool,
}

/// Resolve a cell color through the scheme — the engine's
/// `convert_color` twin for the render side: named colors off the
/// scheme table (the unset background falls to the opaque fallback,
/// never a hard error), indexed colors off the 256-slot table, and
/// `Spec` colors verbatim.
pub(crate) fn resolve_color(colors: &Colors, color: Color) -> Rgb {
    match color {
        Color::Named(named) => {
            if let Some(rgb) = colors[named] {
                rgb
            } else if named == NamedColor::Background {
                // The engine's unset-background rule: the opaque
                // fallback serves, never an error.
                WINDOW_BG_FALLBACK
            } else {
                Rgb::default()
            }
        }
        Color::Spec(rgb) => rgb,
        Color::Indexed(index) => colors[usize::from(index)].unwrap_or_default(),
    }
}

/// The pane's window background: the scheme's background, or the
/// opaque fallback when the scheme leaves it unset (the engine's
/// own unset-background rule).
pub(crate) fn window_background(colors: &Colors) -> Hsla {
    hsla(resolve_color(colors, Color::Named(NamedColor::Background)))
}

/// Project one grid row into styled runs.
///
/// `cursor` names the column the block caret occupies in this row,
/// if it does: that cell always splits into its own run with its
/// colors inverted (fg becomes the cell's background and vice versa,
/// the unset background resolving to the window background first),
/// so the caret paints as an inverse cell in the very frame that
/// shows it — before any keystroke.
pub(crate) fn project_row(
    cells: &[Cell],
    colors: &Colors,
    cursor: Option<usize>,
) -> Vec<StyledRun> {
    let window_bg = window_background(colors);
    let mut runs: Vec<StyledRun> = Vec::new();
    for (col, cell) in cells.iter().enumerate() {
        // A wide char's spacer follower contributes no text — the
        // wide glyph claims both cells through the row's forced
        // width (a duplicated char would double-advance).
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let is_cursor = cursor == Some(col);
        let fg = hsla(resolve_color(colors, cell.fg));
        let bg = match cell.bg {
            // The scheme's background is the window background —
            // no per-cell rect.
            Color::Named(NamedColor::Background) => None,
            other => Some(hsla(resolve_color(colors, other))),
        };
        // INVERSE and the block caret swap the resolved colors: fg
        // takes the cell's background (the window background when
        // unset), the old fg becomes the run's rect.
        let (fg, bg) = if cell.flags.contains(Flags::INVERSE) || is_cursor {
            (bg.unwrap_or(window_bg), Some(fg))
        } else {
            (fg, bg)
        };
        let styled = (fg, bg);
        let attrs = (
            cell.flags.contains(Flags::BOLD),
            cell.flags.contains(Flags::ITALIC),
            cell.flags.contains(Flags::UNDERLINE),
            cell.flags.contains(Flags::STRIKEOUT),
        );
        // The caret cell never merges in either direction, so the
        // block caret is exactly its own run.
        let merges = !is_cursor
            && runs.last().is_some_and(|last| {
                (
                    last.fg,
                    last.bg,
                    last.bold,
                    last.italic,
                    last.underline,
                    last.strikethrough,
                ) == (styled.0, styled.1, attrs.0, attrs.1, attrs.2, attrs.3)
            });
        let mut text = String::new();
        // Tabs land as cells holding the raw character (the engine's
        // own projection maps them to spaces — a tab glyph is never
        // the intent).
        text.push(if cell.c == '\t' { ' ' } else { cell.c });
        if let Some(marks) = cell.zerowidth() {
            text.extend(marks.iter().copied());
        }
        if merges && let Some(last) = runs.last_mut() {
            last.text.push_str(&text);
        } else {
            runs.push(StyledRun {
                text,
                fg: styled.0,
                bg: styled.1,
                bold: attrs.0,
                italic: attrs.1,
                underline: attrs.2,
                strikethrough: attrs.3,
            });
        }
    }
    runs
}

/// View pixels onto the cell grid: floor division, clamped to at
/// least one column and one row (the engine's own resize clamps its
/// pixel floor, never a zero grid).
pub(crate) fn grid_of(
    width: f32,
    height: f32,
    cell_width: f32,
    cell_height: f32,
) -> (usize, usize) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [1, 65535] before the cast: a real surface's cell grid, far below usize::MAX"
    )]
    let cols = (width / cell_width).floor().clamp(1., 65_535.) as usize;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [1, 65535] before the cast: a real surface's cell grid, far below usize::MAX"
    )]
    let rows = (height / cell_height).floor().clamp(1., 65_535.) as usize;
    (cols, rows)
}

/// An `Rgb` onto GPUI's color type.
pub(crate) fn hsla(color: Rgb) -> Hsla {
    rgb(u32::from(color.r) << 16 | u32::from(color.g) << 8 | u32::from(color.b)).into()
}

/// The default cell's flags-relevant projection inputs, for tests:
/// a space with the scheme's default colors.
#[cfg(test)]
pub(crate) fn plain_cell(c: char) -> Cell {
    Cell {
        c,
        ..Cell::default()
    }
}

/// A cell with explicit colors and flags, for tests.
#[cfg(test)]
pub(crate) fn styled_cell(c: char, fg: Color, bg: Color, flags: Flags) -> Cell {
    Cell {
        c,
        fg,
        bg,
        flags,
        ..Cell::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::ansi::Color::*;

    /// The probe listener: events go nowhere (pure grid semantics).
    #[derive(Clone)]
    struct Nop;
    impl alacritty_terminal::event::EventListener for Nop {
        fn send_event(&self, _event: alacritty_terminal::event::Event) {}
    }

    /// The probe dimensions: a bare fixed grid.
    struct Dims {
        cols: usize,
        lines: usize,
    }
    impl alacritty_terminal::grid::Dimensions for Dims {
        fn columns(&self) -> usize {
            self.cols
        }
        fn screen_lines(&self) -> usize {
            self.lines
        }
        fn total_lines(&self) -> usize {
            self.lines
        }
    }

    /// A probe `Term` fed through alacritty's own byte processor —
    /// no PTY, pure grid semantics.
    fn probe_term(cols: usize, lines: usize) -> alacritty_terminal::Term<Nop> {
        alacritty_terminal::Term::new(
            alacritty_terminal::term::Config::default(),
            &Dims { cols, lines },
            Nop,
        )
    }

    #[test]
    fn the_grids_lines_are_top_down_and_the_display_iter_walks_them_in_order() {
        // The view's row mapping leans on alacritty 0.25's grid
        // semantics: `Line(0)` is the screen's TOP row, lines grow
        // downward, `display_iter` yields top-to-bottom, and the
        // cursor's line is itself a top-down screen row. Pinned here
        // against upstream drift — the first rig walk chased a
        // bottom-up model that does not exist.
        use alacritty_terminal::grid::Dimensions as _;
        use alacritty_terminal::index::Column;
        use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
        let mut term = probe_term(8, 4);
        let mut processor: Processor<StdSyncHandler> = Processor::new();
        processor.advance(&mut term, b"TOPBOT\r\nSECOND");
        let text = |line: i32| {
            let row = &term.grid()[alacritty_terminal::index::Line(line)];
            (0..term.grid().columns())
                .map(|c| row[Column(c)].c)
                .collect::<String>()
        };
        assert_eq!(text(0), "TOPBOT  ", "Line(0) is the top row");
        assert_eq!(text(1), "SECOND  ", "lines grow downward");
        let order: Vec<i32> = {
            let mut lines = Vec::new();
            for indexed in term.grid().display_iter() {
                if lines.last() != Some(&indexed.point.line.0) {
                    lines.push(indexed.point.line.0);
                }
            }
            lines
        };
        assert_eq!(order, vec![0, 1, 2, 3], "display_iter walks top to bottom");
        assert_eq!(
            term.grid().cursor.point.line.0,
            1,
            "the cursor's line is top-down"
        );
        assert_eq!(term.grid().screen_lines(), 4);
    }

    #[test]
    fn a_narrowing_resize_is_the_engines_own_reflow_not_the_views() {
        // The write-then-narrow sequence the rig walk hits: content
        // written at the spawn width reflows under alacritty's own
        // rules when the grid narrows (the wrapped tail moves into
        // history; the cursor's line stays). Recorded so a future
        // "the view lost content on resize" report resolves against
        // the engine's behavior, not the projection.
        use alacritty_terminal::grid::Dimensions as _;
        use alacritty_terminal::index::Column;
        use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
        let mut term = probe_term(80, 4);
        let mut processor: Processor<StdSyncHandler> = Processor::new();
        processor.advance(&mut term, b"RED GREEN BLUE plain\r");
        term.resize(Dims { cols: 8, lines: 4 });
        processor.advance(&mut term, b"after");
        let row = &term.grid()[alacritty_terminal::index::Line(0)];
        let text: String = (0..term.grid().columns())
            .map(|c| row[Column(c)].c)
            .collect();
        assert_eq!(text, "after   ", "the cursor's line survives the reflow");
        assert_eq!(term.grid().cursor.point.line.0, 0);
    }

    /// The default cell text (`" "` with default colors) of a run —
    /// the exacting form several asserts below read.
    fn run_text(runs: &[StyledRun]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn named_indexed_and_spec_colors_resolve_through_the_scheme() {
        let mut colors = Colors::default();
        colors[NamedColor::Red] = Some(Rgb {
            r: 0xff,
            g: 0x55,
            b: 0x55,
        });
        colors[NamedColor::Background] = Some(Rgb {
            r: 0x1a,
            g: 0x1b,
            b: 0x26,
        });
        // Named: the scheme's slot, verbatim.
        assert_eq!(
            resolve_color(&colors, Named(NamedColor::Red)),
            Rgb {
                r: 0xff,
                g: 0x55,
                b: 0x55
            }
        );
        // The unset background falls to the opaque fallback — the
        // engine's unset-background rule, not an error.
        assert_eq!(
            resolve_color(&colors, Named(NamedColor::Foreground)),
            Rgb::default(),
            "an unset named slot resolves to the zero fill like the engine's"
        );
        // Indexed: the 256-slot table (unset → the zero fill).
        colors[250] = Some(Rgb {
            r: 0x12,
            g: 0x34,
            b: 0x56,
        });
        assert_eq!(
            resolve_color(&colors, Indexed(250)),
            Rgb {
                r: 0x12,
                g: 0x34,
                b: 0x56
            }
        );
        // Spec: verbatim.
        assert_eq!(
            resolve_color(&colors, Spec(Rgb { r: 1, g: 2, b: 3 })),
            Rgb { r: 1, g: 2, b: 3 }
        );
    }

    #[test]
    fn the_window_background_is_the_scheme_background_or_the_fallback() {
        let mut colors = Colors::default();
        assert_eq!(
            window_background(&colors),
            hsla(WINDOW_BG_FALLBACK),
            "an unset background paints the opaque fallback"
        );
        colors[NamedColor::Background] = Some(Rgb {
            r: 0x1a,
            g: 0x1b,
            b: 0x26,
        });
        assert_eq!(
            window_background(&colors),
            hsla(Rgb {
                r: 0x1a,
                g: 0x1b,
                b: 0x26
            })
        );
    }

    #[test]
    fn plain_cells_merge_into_one_default_run() {
        // A row of plain cells is one run: default fg, no per-cell
        // background rect (the window background already covers it),
        // every cell's char in the text — spaces included, because
        // the shaper advances on them.
        let colors = Colors::default();
        let cells: Vec<Cell> = "hi  you".chars().map(plain_cell).collect();
        let runs = project_row(&cells, &colors, None);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "hi  you");
        assert_eq!(runs[0].bg, None);
        assert!(!runs[0].bold && !runs[0].italic && !runs[0].underline);
    }

    #[test]
    fn a_style_change_splits_the_row_and_attributes_ride_their_runs() {
        // RED plain, GREEN fg swap, then a bold+underline cell: four
        // runs, the attributes on exactly their own runs.
        let mut colors = Colors::default();
        colors[NamedColor::Green] = Some(Rgb {
            r: 0x55,
            g: 0xff,
            b: 0x55,
        });
        let cells = vec![
            plain_cell('a'),
            styled_cell(
                'B',
                Named(NamedColor::Green),
                Named(NamedColor::Background),
                Flags::BOLD | Flags::UNDERLINE,
            ),
            plain_cell('c'),
        ];
        let runs = project_row(&cells, &colors, None);
        assert_eq!(runs.len(), 3, "{runs:?}");
        assert_eq!(runs[0].text, "a");
        assert!(!runs[0].bold && !runs[0].underline);
        assert_eq!(runs[1].text, "B");
        assert!(runs[1].bold && runs[1].underline);
        assert_eq!(
            runs[1].fg,
            hsla(Rgb {
                r: 0x55,
                g: 0xff,
                b: 0x55
            })
        );
        assert_eq!(runs[2].text, "c");
    }

    #[test]
    fn a_cell_background_becomes_the_run_rect() {
        // A cell with a non-default bg carries it as the run's bg;
        // the default-bg neighbors stay rect-free.
        let mut colors = Colors::default();
        colors[NamedColor::Blue] = Some(Rgb {
            r: 0x55,
            g: 0x55,
            b: 0xff,
        });
        let cells = vec![
            plain_cell('a'),
            styled_cell(
                'b',
                Named(NamedColor::Foreground),
                Named(NamedColor::Blue),
                Flags::empty(),
            ),
            plain_cell('c'),
        ];
        let runs = project_row(&cells, &colors, None);
        assert_eq!(runs.len(), 3);
        assert_eq!(
            runs[1].bg,
            Some(hsla(Rgb {
                r: 0x55,
                g: 0x55,
                b: 0xff
            }))
        );
        assert_eq!(runs[0].bg, None);
        assert_eq!(runs[2].bg, None);
    }

    #[test]
    fn inverse_swaps_the_resolved_colors() {
        // INVERSE swaps fg and bg, an unset background resolving to
        // the window background first (xterm's rule: the swap is
        // between resolved colors, never with `None`).
        let mut colors = Colors::default();
        colors[NamedColor::Background] = Some(Rgb {
            r: 0x10,
            g: 0x20,
            b: 0x30,
        });
        colors[NamedColor::Red] = Some(Rgb {
            r: 0xff,
            g: 0x00,
            b: 0x00,
        });
        let cells = vec![styled_cell(
            'X',
            Named(NamedColor::Red),
            Named(NamedColor::Background),
            Flags::INVERSE,
        )];
        let runs = project_row(&cells, &colors, None);
        assert_eq!(
            runs[0].fg,
            hsla(Rgb {
                r: 0x10,
                g: 0x20,
                b: 0x30
            }),
            "inverse fg is the cell's resolved background"
        );
        assert_eq!(
            runs[0].bg,
            Some(hsla(Rgb {
                r: 0xff,
                g: 0x00,
                b: 0x00
            })),
            "inverse bg is the cell's resolved foreground"
        );
    }

    #[test]
    fn the_cursor_cell_is_the_block_caret_run() {
        // The caret column splits its own inverse run even over
        // otherwise-plain neighbors: the very frame that shows the
        // pane paints a block caret, before any keystroke.
        let mut colors = Colors::default();
        colors[NamedColor::Background] = Some(Rgb {
            r: 0x10,
            g: 0x20,
            b: 0x30,
        });
        colors[NamedColor::Foreground] = Some(Rgb {
            r: 0xdd,
            g: 0xdd,
            b: 0xdd,
        });
        let cells: Vec<Cell> = "ab".chars().map(plain_cell).collect();
        let runs = project_row(&cells, &colors, Some(1));
        // Two runs: `a` plain, `b` the inverted caret cell.
        assert_eq!(runs.len(), 2, "{runs:?}");
        assert_eq!(runs[1].text, "b");
        assert_eq!(
            runs[1].fg,
            hsla(Rgb {
                r: 0x10,
                g: 0x20,
                b: 0x30
            })
        );
        assert_eq!(
            runs[1].bg,
            Some(hsla(Rgb {
                r: 0xdd,
                g: 0xdd,
                b: 0xdd
            }))
        );
        // Without the caret, the same row is one plain run.
        assert_eq!(project_row(&cells, &colors, None).len(), 1);
    }

    #[test]
    fn wide_char_spacers_contribute_no_text() {
        // A wide char claims two cells; its spacer follower is absent
        // from the run text (the row's forced width gives the glyph
        // both cells; a duplicated char would double-advance).
        let colors = Colors::default();
        let mut wide = plain_cell('漢');
        wide.flags.insert(Flags::WIDE_CHAR);
        let mut spacer = plain_cell(' ');
        spacer.flags.insert(Flags::WIDE_CHAR_SPACER);
        let cells = vec![plain_cell('a'), wide, spacer, plain_cell('b')];
        let runs = project_row(&cells, &colors, None);
        assert_eq!(
            run_text(&runs),
            "a漢b",
            "the wide glyph once; the spacer never"
        );
        assert_eq!(runs.len(), 1, "the spacer carries no style of its own");
    }

    #[test]
    fn zero_width_marks_join_their_cell() {
        // Combining marks live in the extra of the cell they
        // decorate: they join the run's text after the base char.
        let colors = Colors::default();
        let mut base = plain_cell('e');
        base.push_zerowidth('\u{0301}'); // combining acute
        let cells = vec![base, plain_cell('!')];
        let runs = project_row(&cells, &colors, None);
        assert_eq!(run_text(&runs), "e\u{301}!");
    }

    #[test]
    fn view_pixels_floor_onto_the_cell_grid_clamped_to_one() {
        // The resize math: floor division on the live cell metrics,
        // never fewer than one column or one row.
        assert_eq!(grid_of(800., 400., 9.6, 19.6), (83, 20));
        assert_eq!(grid_of(9.6, 19.6, 9.6, 19.6), (1, 1));
        assert_eq!(
            grid_of(0., 0., 9.6, 19.6),
            (1, 1),
            "the clamp holds at zero"
        );
        assert_eq!(
            grid_of(9.5, 19.5, 9.6, 19.6),
            (1, 1),
            "a partial cell is not a column"
        );
    }
}
