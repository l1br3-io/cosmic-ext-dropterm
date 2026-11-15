// SPDX-License-Identifier: GPL-3.0-only

//! The selection model's pure halves (gpui-terminal Task 2): the
//! range→row-spans projection the paint overlays, the click-kind
//! state machine (single/double/triple cycling inside the click
//! window), and the pixel→cell arithmetic the pointer handlers feed.
//! The pointer gesture itself lives in the view; the alacritty
//! `Selection` object it drives is the engine's own.

#[cfg(test)]
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::selection::SelectionRange;

/// One row's selected column span `[start, end)` — the overlay
/// quad's cell arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RowSpan {
    /// The absolute grid line (top-down; negative in scrollback).
    pub line: i32,
    pub start: usize,
    pub end: usize,
}

/// Project a selection range onto per-row column spans: the first
/// and last lines are partial, the lines between span the full
/// width. A block selection spans each line's own columns. A range
/// only exists once `to_range` accepted the gesture (a click that
/// never dragged produces no range at all), so the same-point case
/// is the one-cell selection, not an empty one.
pub(crate) fn spans(range: &SelectionRange, cols: usize) -> Vec<RowSpan> {
    let mut out = Vec::new();
    for line in range.start.line.0..=range.end.line.0 {
        let start = if line == range.start.line.0 {
            range.start.column.0
        } else {
            0
        };
        let end = if range.end.line.0 == line {
            (range.end.column.0 + 1).min(cols)
        } else {
            cols
        };
        if range.is_block {
            // A block selection clips every line to its own columns.
            let block_start = range.start.column.0.min(end);
            let block_end = (range.end.column.0 + 1).min(cols).max(block_start);
            out.push(RowSpan {
                line,
                start: block_start,
                end: block_end.max(block_start),
            });
        } else if start < end {
            out.push(RowSpan {
                line,
                start,
                end: end.min(cols),
            });
        }
    }
    out
}

/// The click-kind cycle: a press inside the click window after a
/// single is a double, after a double a triple, and a triple folds
/// back to a single (the libcosmic state machine, verbatim).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClickKind {
    Single,
    Double,
    Triple,
}

pub(crate) fn next_click_kind(prev: Option<ClickKind>, within_window: bool) -> ClickKind {
    // The catch-all folds the triple back to a single and resets any
    // press that missed the window — both land on the same kind.
    match (prev, within_window) {
        (Some(ClickKind::Single), true) => ClickKind::Double,
        (Some(ClickKind::Double), true) => ClickKind::Triple,
        _ => ClickKind::Single,
    }
}

/// The cell a pointer position lands in: the column as a floor of
/// the pixel ratio (the release's side rule reads the fractional
/// half) and the display row.
pub(crate) fn cell_at(x: f32, y: f32, cell_width: f32, cell_height: f32) -> (usize, usize) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped non-negative px over a cell: a grid coordinate far below 2^24"
    )]
    let cell = (
        (x / cell_width).floor().max(0.) as usize,
        (y / cell_height).floor().max(0.) as usize,
    );
    cell
}

/// The selection side a press lands on: left of the cell's midpoint
/// anchors before the character, right after it (the widget's rule).
pub(crate) fn side_of(x: f32, cell_width: f32) -> alacritty_terminal::index::Side {
    use alacritty_terminal::index::Side;
    if (x / cell_width).fract() < 0.5 {
        Side::Left
    } else {
        Side::Right
    }
}

/// The alacritty selection type a click kind starts.
pub(crate) fn selection_kind_of(click: ClickKind) -> alacritty_terminal::selection::SelectionType {
    use alacritty_terminal::selection::SelectionType;
    match click {
        ClickKind::Single => SelectionType::Simple,
        ClickKind::Double => SelectionType::Semantic,
        ClickKind::Triple => SelectionType::Lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(start: (i32, usize), end: (i32, usize)) -> SelectionRange {
        SelectionRange {
            start: Point::new(Line(start.0), Column(start.1)),
            end: Point::new(Line(end.0), Column(end.1)),
            is_block: false,
        }
    }

    #[test]
    fn a_multi_line_range_spans_partial_first_and_last_full_between() {
        let r = range((0, 3), (2, 5));
        let s = spans(&r, 80);
        assert_eq!(
            s,
            vec![
                RowSpan {
                    line: 0,
                    start: 3,
                    end: 80
                },
                RowSpan {
                    line: 1,
                    start: 0,
                    end: 80
                },
                RowSpan {
                    line: 2,
                    start: 0,
                    end: 6
                },
            ]
        );
    }

    #[test]
    fn a_single_cell_range_spans_one_cell() {
        let r = range((1, 2), (1, 2));
        let s = spans(&r, 80);
        assert_eq!(
            s,
            vec![RowSpan {
                line: 1,
                start: 2,
                end: 3
            }]
        );
    }

    #[test]
    fn the_end_column_clamps_to_the_grid_width() {
        let r = range((0, 78), (0, 79));
        let s = spans(&r, 80);
        assert_eq!(s.last().map(|span| span.end), Some(80));
    }

    #[test]
    fn the_click_cycle_advances_inside_the_window_and_resets_outside() {
        use ClickKind::*;
        assert_eq!(next_click_kind(None, false), Single);
        assert_eq!(next_click_kind(Some(Single), true), Double);
        assert_eq!(next_click_kind(Some(Double), true), Triple);
        assert_eq!(next_click_kind(Some(Triple), true), Single);
        assert_eq!(next_click_kind(Some(Double), false), Single);
    }

    #[test]
    fn pixels_floor_onto_cells_with_side_from_the_half() {
        assert_eq!(cell_at(19.9, 41.0, 10., 20.), (1, 2));
        assert_eq!(cell_at(-5., -5., 10., 20.), (0, 0), "clamped at the origin");
        assert_eq!(side_of(14.9, 10.), alacritty_terminal::index::Side::Left);
        assert_eq!(side_of(15.1, 10.), alacritty_terminal::index::Side::Right);
    }
}
