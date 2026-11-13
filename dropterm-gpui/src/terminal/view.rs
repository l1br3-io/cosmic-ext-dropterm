// SPDX-License-Identifier: GPL-3.0-only

//! The terminal pane view (gpui-terminal Task 1): the render entity
//! that owns the engine's [`Terminal`], drives it on the same paths
//! the libcosmic widget drives (resize on view-size change,
//! `update()` on wakeup, `set_focused` for the caret) and paints
//! projected grid rows through GPUI's shaper — one shaped line per
//! row, its width forced to the cell grid, its runs carrying the
//! scheme's colors and the cell attributes.
//!
//! Damage discipline (the chunk's planning ruling): frames are
//! demanded by content, not by a clock — the event pump notifies on
//! the engine's wakeups (PTY output) and on the view's own state
//! changes, and an idle pane paints nothing new. Within a frame the
//! per-row shaping rides GPUI's layout cache, so unchanged rows
//! re-shape as cache hits.
//!
//! The paint closures cannot hold `&mut` pane state (GPUI elements
//! are `'static`), so the walk's evidence dedup lives in a shared
//! [`WalkLog`] the closures own a handle to.

use std::sync::{Arc, Mutex};

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::index::Line;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::term::color::Colors;
use gpui::{
    App, Context, Entity, FontWeight, Pixels, TextAlign, UnderlineStyle, canvas, fill, point,
    prelude::*, px,
};
use tokio::sync::mpsc;

use dropterm_core::tabs::TabId;
use dropterm_core::terminal::Terminal;

use super::grid::{self, StyledRun};

/// The walk evidence rows' dedup state (the last logged grid and
/// caret cells — one row per change, not per frame).
#[derive(Default)]
pub(crate) struct WalkLog {
    grid: Option<(usize, usize)>,
    caret: Option<(usize, usize)>,
    rows: Vec<String>,
}

/// The pane's paint skin: everything one frame's paint needs beside
/// the engine itself, bundled so the render closures capture one
/// value and the paint functions stay under the argument budget.
#[derive(Clone)]
pub(crate) struct PaneSkin {
    colors: Colors,
    /// The background alpha (config `opacity`, 0–100 → 0.0–1.0).
    opacity_alpha: f32,
    /// The base render font (runs refine weight/style per cell).
    font: gpui::Font,
}

/// The live grid geometry one frame paints with.
struct GridGeometry {
    font_size: Pixels,
    cell_width: Pixels,
    cell_height: Pixels,
    cols: usize,
}

/// The pane view: one engine terminal, its resolved scheme, and the
/// base font the host measured the cell through.
pub(crate) struct TerminalPane {
    pub(crate) terminal: Arc<Mutex<Terminal>>,
    /// The engine's wakeup flag — the pump sets it, the next frame's
    /// paint runs `update()` and clears it (the widget's own
    /// layout-time update, verbatim).
    needs_update: bool,
    skin: PaneSkin,
    /// The shell's latest title report (stored now, consumed when
    /// the tab chrome slices land).
    title: Option<String>,
    walk_log: Arc<Mutex<WalkLog>>,
}

impl TerminalPane {
    /// Build the pane around a live engine terminal. The caller
    /// owns the terminal's construction (PTY spawn included) and
    /// hands over the receiver half of the engine's event channel —
    /// [`Self::spawn_pump`] turns it into frame demand.
    #[expect(
        clippy::large_types_passed_by_value,
        reason = "a Copy scheme table, moved once at the pane's construction; the pane owns it"
    )]
    pub(crate) fn new(
        terminal: Arc<Mutex<Terminal>>,
        colors: Colors,
        opacity: u8,
        font: gpui::Font,
    ) -> Self {
        {
            // The pane is the focused surface by construction (the
            // drawer holds the keyboard) — armed once so the very
            // first frame paints the caret.
            terminal.lock().expect("engine lock").set_focused(true);
        }
        Self {
            terminal,
            needs_update: true,
            skin: PaneSkin {
                colors,
                opacity_alpha: f32::from(opacity.min(100)) / 100.,
                font,
            },
            title: None,
            walk_log: Arc::new(Mutex::new(WalkLog::default())),
        }
    }

    /// The engine event pump: every wakeup is frame demand (PTY
    /// output arrived), and the rest of the events land their
    /// engine-side handling here — the `tab_actions` mirror for the
    /// events a single pane owns. Runs on GPUI's executor; tokio's
    /// unbounded channel needs no runtime of its own.
    #[expect(
        clippy::unused_self,
        reason = "the pump is the pane's own lifetime concern: a method on the pane it will outlive reads correctly at the construction site"
    )]
    pub(crate) fn spawn_pump(
        &mut self,
        mut events: mpsc::UnboundedReceiver<(TabId, TermEvent)>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            while let Some((_, event)) = events.recv().await {
                let _ = this.update(cx, |pane, cx| pane.on_event(event, cx));
            }
        })
        .detach();
    }

    /// One engine event through the pane (the libcosmic app's
    /// `handle_term_event`, pared to the single-pane surface).
    fn on_event(&mut self, event: TermEvent, cx: &mut Context<Self>) {
        match event {
            TermEvent::Wakeup => {
                self.needs_update = true;
                cx.notify();
            }
            TermEvent::Title(title) => {
                self.title = Some(title);
            }
            TermEvent::ResetTitle => {
                self.title = None;
            }
            TermEvent::Bell => {
                println!("dropterm-gpui: bell (notifications arrive with the chrome slices)");
            }
            TermEvent::ClipboardStore(_, _) | TermEvent::ClipboardLoad(_, _) => {
                // OSC 52 store/load: wired with the clipboard task —
                // logged now so a walk sees the arrival.
                println!("dropterm-gpui: osc52 clipboard event (wiring arrives with the task)");
            }
            TermEvent::ColorRequest(index, format) => {
                let terminal = self.terminal.lock().expect("engine lock");
                let rgb = terminal.effective_color(index);
                terminal.input_no_scroll(format(rgb).into_bytes());
            }
            TermEvent::PtyWrite(text) => {
                self.terminal
                    .lock()
                    .expect("engine lock")
                    .input_no_scroll(text.into_bytes());
            }
            TermEvent::TextAreaSizeRequest(format) => {
                let terminal = self.terminal.lock().expect("engine lock");
                let answer = format(terminal.size().into());
                terminal.input_no_scroll(answer.into_bytes());
            }
            TermEvent::InlineImage(params) => {
                self.terminal
                    .lock()
                    .expect("engine lock")
                    .note_inline_image(&params);
            }
            TermEvent::ApcPayload(payload) => {
                self.terminal
                    .lock()
                    .expect("engine lock")
                    .note_apc_payload(&payload);
            }
            TermEvent::PromptExit(code) => {
                self.terminal
                    .lock()
                    .expect("engine lock")
                    .note_prompt_exit(code);
            }
            TermEvent::CurrentDirectory(url) => {
                self.terminal
                    .lock()
                    .expect("engine lock")
                    .note_current_directory(&url);
            }
            TermEvent::ProgressReport(state, value) => {
                self.terminal
                    .lock()
                    .expect("engine lock")
                    .note_progress_report(state, value);
            }
            TermEvent::ProgramStatus(body) => {
                self.terminal
                    .lock()
                    .expect("engine lock")
                    .note_program_status(&body);
            }
            TermEvent::FullReset => {
                self.terminal
                    .lock()
                    .expect("engine lock")
                    .note_terminal_reset();
            }
            TermEvent::ChildExit(code) => {
                // Tab lifecycle (respawn-on-close) is the tabs
                // slice's; the exit is an evidence row until then.
                println!("dropterm-gpui: pane child exited ({code})");
            }
            // The pane owns no window-icon or mouse-cursor surface
            // yet, and blinking is off (the libcosmic build's rule —
            // the caret is a steady block).
            TermEvent::MouseCursorDirty | TermEvent::CursorBlinkingChange => {}
            // The engine's own shutdown request: the pane has no
            // app-level surface to route it through yet; the tabs
            // slice owns closes.
            TermEvent::Exit => {
                println!("dropterm-gpui: pane exit request (tab lifecycle arrives with its slice)");
            }
        }
    }
}

impl gpui::Render for TerminalPane {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut Context<Self>,
    ) -> impl gpui::IntoElement {
        let terminal = Arc::clone(&self.terminal);
        let skin = self.skin.clone();
        let walk_log = Arc::clone(&self.walk_log);
        canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, cx| {
                paint_pane(&terminal, &skin, &walk_log, bounds, window, cx);
            },
        )
        .size_full()
    }
}

/// One pane's paint: resize to the granted bounds, drain the update
/// flag, then walk the visible grid through the projection and the
/// shaper.
fn paint_pane(
    terminal: &Arc<Mutex<Terminal>>,
    skin: &PaneSkin,
    walk_log: &Arc<Mutex<WalkLog>>,
    bounds: gpui::Bounds<Pixels>,
    window: &mut gpui::Window,
    cx: &mut App,
) {
    let mut terminal = terminal.lock().expect("engine lock");
    resize_to_bounds(&mut terminal, bounds);

    // The wakeup's update, on the render path (the widget's rule).
    if terminal.needs_update {
        terminal.needs_update = false;
        let _ = terminal.update();
    }

    let term = terminal.term.lock();
    let grid = term.grid();
    let display_offset = grid.display_offset();
    let cols = grid.columns();
    let rows = grid.screen_lines();
    let cell = terminal.size();
    let font_size = px(terminal.with_buffer(|buffer| buffer.metrics().font_size));
    let geometry = GridGeometry {
        font_size,
        cell_width: px(cell.cell_width),
        cell_height: px(cell.cell_height),
        cols,
    };

    // The walk's grid evidence row: the resting grid, once per change.
    {
        let mut log = walk_log.lock().expect("walk log");
        if log.grid != Some((cols, rows)) {
            println!(
                "dropterm-gpui: pane grid {cols}x{rows} at scroll offset {display_offset} \
                 (cell {:.2}x{:.2}, font {:.1}px)",
                cell.cell_width,
                cell.cell_height,
                geometry.font_size.as_f32()
            );
            log.grid = Some((cols, rows));
        }
    }

    // The caret: shown when the mode says so, always a block (the
    // libcosmic build's rule). Grid semantics (pinned by the probe
    // test in this module): `Line(0)` is the screen's top, lines
    // grow downward, and the cursor's line is a screen row — so
    // scrolled state `d` puts it at display row `cursor.line + d`,
    // on the display only while that lands inside the rows.
    let cursor = grid.cursor.point;
    let caret_row = term
        .mode()
        .contains(TermMode::SHOW_CURSOR)
        .then(|| usize::try_from(cursor.line.0).unwrap_or(0) + display_offset);

    // The visible rows, grouped from `display_iter` — the same walk
    // the engine's own projection rides, top row first (hand-rolled
    // `Line` math into unheld history panics the grid's storage;
    // the iterator is the supported traversal).
    let mut display_rows: Vec<Vec<Cell>> = Vec::with_capacity(rows);
    let mut current_line: Option<Line> = None;
    for indexed in grid.display_iter() {
        if current_line != Some(indexed.point.line) {
            current_line = Some(indexed.point.line);
            display_rows.push(Vec::with_capacity(cols));
        }
        if let Some(row) = display_rows.last_mut() {
            row.push(indexed.cell.clone());
        }
    }

    // The window background under every row (the scheme background
    // at the configured opacity, over the pane's whole bounds).
    let mut bg = grid::window_background(&skin.colors);
    bg.a *= skin.opacity_alpha;
    window.paint_quad(fill(bounds, bg));

    for (row, cells) in display_rows.iter().enumerate() {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a viewport row index: far below 2^24, exact in f32"
        )]
        let row_origin_y = bounds.origin.y + geometry.cell_height * row as f32;
        let caret_col = (caret_row == Some(row)).then_some(cursor.column.0);
        let runs = grid::project_row(cells, &skin.colors, caret_col);
        // The walk's row-0 content evidence (the spike's own check-9
        // row): what the paint actually shaped, logged once per
        // change — the compositor's pixels and this row together
        // pin which side a defect lives on.
        if row < 4 {
            let text: String = runs.iter().map(|r| r.text.as_str()).collect();
            let mut log = walk_log.lock().expect("walk log");
            let changed = log.rows.get(row).is_none_or(|seen| seen != &text);
            if changed {
                log.rows.resize(row + 1, String::new());
                log.rows[row].clone_from(&text);
                println!("dropterm-gpui: pane row{row} now {text:?}");
            }
        }
        paint_row(
            &runs,
            skin,
            &geometry,
            point(bounds.origin.x, row_origin_y),
            window,
            cx,
        );
    }

    // The walk's caret evidence row: the caret cell, once per move.
    if let Some(row) = caret_row {
        let cell_at = (cursor.column.0, row);
        let mut log = walk_log.lock().expect("walk log");
        if log.caret != Some(cell_at) {
            println!(
                "dropterm-gpui: caret at col {} row {} (block, first-frame rule)",
                cell_at.0, cell_at.1
            );
            log.caret = Some(cell_at);
        }
    }
}

/// Resize the engine to the pane's granted bounds — this frame
/// already renders the new grid (SIGWINCH rides the notifier), and
/// the transition logs the walk's resize evidence row. The pixel
/// request is clamped to the engine's own one-cell floor *before*
/// the comparison, so a sub-cell window converges (the engine's
/// internal clamp would otherwise store one-cell dimensions against
/// a sub-cell request every frame — the first walk's oscillation).
fn resize_to_bounds(terminal: &mut Terminal, bounds: gpui::Bounds<Pixels>) {
    let (width, height) = (
        bounds.size.width.as_f32().max(0.),
        bounds.size.height.as_f32().max(0.),
    );
    let size = terminal.size();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "cell and surface px are positive and far below u32::MAX; ceil keeps a partial cell whole"
    )]
    let px_size = (
        width.max(size.cell_width.ceil()).round() as u32,
        height.max(size.cell_height.ceil()).round() as u32,
    );
    if (size.width, size.height) != px_size {
        terminal.resize(px_size.0, px_size.1);
        #[expect(
            clippy::cast_precision_loss,
            reason = "surface px clamped above one cell and far below 2^24: exact in f32"
        )]
        let (w, h) = (px_size.0 as f32, px_size.1 as f32);
        let grid = grid::grid_of(w, h, size.cell_width, size.cell_height);
        println!(
            "dropterm-gpui: pane resize {px_size:?} -> grid {grid:?} (Term::resize + SIGWINCH)"
        );
    }
}

/// Paint one projected row: one shaped line, forced to the cell
/// grid's exact width, one text run per styled run.
fn paint_row(
    runs: &[StyledRun],
    skin: &PaneSkin,
    geometry: &GridGeometry,
    origin: gpui::Point<Pixels>,
    window: &mut gpui::Window,
    cx: &mut App,
) {
    let mut text = String::new();
    let mut text_runs = Vec::with_capacity(runs.len());
    for run in runs {
        let mut font = skin.font.clone();
        if run.bold {
            font.weight = FontWeight::BOLD;
        }
        if run.italic {
            font.style = gpui::FontStyle::Italic;
        }
        let underline = run.underline.then(|| UnderlineStyle {
            color: Some(run.fg),
            thickness: px(1.),
            wavy: false,
        });
        let strikethrough = run.strikethrough.then(|| gpui::StrikethroughStyle {
            color: Some(run.fg),
            thickness: px(1.),
        });
        text_runs.push(gpui::TextRun {
            len: run.text.len(),
            font,
            color: run.fg,
            background_color: run.bg,
            underline,
            strikethrough,
        });
        text.push_str(&run.text);
    }
    if text.is_empty() {
        return;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a grid's column count is far below 2^24: exact in f32"
    )]
    let forced = geometry.cell_width * geometry.cols as f32;
    let shaped =
        window
            .text_system()
            .shape_line(text.into(), geometry.font_size, &text_runs, Some(forced));
    let _ = shaped.paint(
        origin,
        geometry.cell_height,
        TextAlign::Left,
        None,
        window,
        cx,
    );
}

/// The scheme the pane resolves for a config at boot — the effective
/// colors chain with no profile (single pane, no tabs yet) and the
/// preference's own dark/light; `System` resolves dark until the
/// theme slice lands the live system-mode read (the drawer default).
pub(crate) fn boot_colors(config: &dropterm_core::config::DropTermConfig) -> Colors {
    let is_dark = dropterm_core::theme_glue::effective_is_dark(
        config.theme_preference,
        // System → dark: the live system-mode read arrives with the
        // gpui-theme slice.
        true,
    );
    dropterm_core::theme_glue::effective_colors(config, None, is_dark)
}

/// The pane factory the walk harness and the shell wiring share:
/// pane and pump in one entity.
pub(crate) fn pane_with_pump(
    cx: &mut App,
    terminal: Arc<Mutex<Terminal>>,
    colors: &Colors,
    opacity: u8,
    font: gpui::Font,
    events: mpsc::UnboundedReceiver<(TabId, TermEvent)>,
) -> Entity<TerminalPane> {
    cx.new(|cx| {
        let mut pane = TerminalPane::new(terminal, *colors, opacity, font);
        pane.spawn_pump(events, cx);
        pane
    })
}
