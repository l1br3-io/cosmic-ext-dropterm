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
    App, Context, Entity, FontWeight, Pixels, TextAlign, UnderlineStyle, canvas, div, fill, point,
    prelude::*, px,
};
use tokio::sync::mpsc;

use dropterm_core::tabs::TabId;
use dropterm_core::terminal::Terminal;
use dropterm_core::terminal::TerminalOptions;
use dropterm_core::terminal::mouse_reporter::{
    self, MouseReporter, PointerButton, PointerEvent, PointerModifiers, PointerScrollDelta,
};

use alacritty_terminal::index::Column;

use super::clipboard;
use super::grid::{self, StyledRun};
use super::input::{self, Chord, KeyInput, KeyModes, TermAction};
use super::selection::{self, ClickKind};

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
    /// The effective chords (defaults + the config overlay).
    binds: std::collections::HashMap<Chord, TermAction>,
    /// The pane's engine options, retained for the zoom pass's
    /// `set_config` (the metrics reapply *is* the zoom).
    options: TerminalOptions,
    /// The pane's focus handle — the keyboard routes here.
    focus: gpui::FocusHandle,
    /// The click-kind state machine's last click.
    click: Option<(ClickKind, std::time::Instant)>,
    /// Whether a left drag is extending the selection.
    dragging: bool,
    /// The wheel accumulator (fractional notches carry over).
    scroll_lines: f32,
    /// The xterm mouse-tracking encoder (core's, consumed): holds the
    /// motion-dedup cell and the wheel accumulation between events.
    reporter: MouseReporter,
    /// Whether the in-flight gesture belongs to a mouse-reporting
    /// program (the widget's `drag_in_flight` rule turned inside
    /// out: a reported press owns its whole gesture — release
    /// included — so a bypass that starts as a selection never hands
    /// the child a release it never saw).
    reporting_gesture: bool,
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
        binds: std::collections::HashMap<Chord, TermAction>,
        options: TerminalOptions,
        focus: gpui::FocusHandle,
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
            binds,
            options,
            focus,
            click: None,
            dragging: false,
            scroll_lines: 0.,
            reporter: MouseReporter::default(),
            reporting_gesture: false,
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
            TermEvent::ClipboardStore(kind, text) => {
                // OSC 52 store: a program copying out (tmux's copy
                // mode, ssh forwarding) — the copy-while-unfocused
                // class by nature, so it rides data-control, which
                // needs neither focus nor serial.
                let primary = kind == alacritty_terminal::term::ClipboardType::Selection;
                println!(
                    "dropterm-gpui: osc52 store {} bytes (primary={primary})",
                    text.len()
                );
                clipboard::write_data_control(text, primary);
            }
            TermEvent::ClipboardLoad(kind, format) => {
                // OSC 52 load: a program asking for the clipboard —
                // the reply applies the engine's format closure to
                // the loaded text and lands in the asking PTY. An
                // empty clipboard still answers, formatted, never
                // silence (the clipboard-ownership spec's row).
                let primary = kind == alacritty_terminal::term::ClipboardType::Selection;
                println!("dropterm-gpui: osc52 load (primary={primary})");
                self.load_clipboard_reply(primary, format, cx);
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

    /// An OSC 52 load's answer: read the asked selection (the
    /// regular clipboard synchronously through GPUI, the primary
    /// selection through one background data-control read), apply
    /// the engine's format closure, and land the reply in the
    /// asking PTY — an empty clipboard still answers, formatted,
    /// never silence (the clipboard-ownership spec's row).
    fn load_clipboard_reply(
        &mut self,
        primary: bool,
        format: std::sync::Arc<dyn Fn(&str) -> String + Sync + Send>,
        cx: &mut Context<Self>,
    ) {
        if !primary {
            let data = clipboard::read(cx);
            let reply = format(&data.unwrap_or_default()).into_bytes();
            let mut terminal = self.terminal.lock().expect("engine lock");
            terminal.input_no_scroll(reply);
            terminal.needs_update = true;
            return;
        }
        let terminal = Arc::clone(&self.terminal);
        cx.spawn(async move |this, cx| {
            let data = cx
                .background_executor()
                .spawn(async move { clipboard::read_primary_blocking() })
                .await;
            let _ = this.update(cx, |pane, cx| {
                let reply = format(&data.unwrap_or_default()).into_bytes();
                let mut terminal = terminal.lock().expect("engine lock");
                terminal.input_no_scroll(reply);
                terminal.needs_update = true;
                drop(terminal);
                pane.needs_update = true;
                cx.notify();
            });
        })
        .detach();
    }
}

impl TerminalPane {
    /// One matched action through the pane — the terminal-view
    /// slice's execution (the libcosmic `perform_action` pared to
    /// the pane's own surface; clipboard lands with its task, the
    /// surface hide with the shell wiring).
    fn perform_action(&mut self, action: TermAction, cx: &mut Context<Self>) {
        match action {
            TermAction::Copy | TermAction::CopyPrimary => {
                // The chord just landed in the focused pane, so the
                // regular write rides GPUI's clipboard (focus plus
                // input serial, by construction — check 8's ruling);
                // the primary write has no GPUI API and rides
                // data-control.
                let text = {
                    let terminal = self.terminal.lock().expect("engine lock");
                    let term = terminal.term.lock();
                    term.selection_to_string()
                };
                match text {
                    Some(text) => {
                        println!(
                            "dropterm-gpui: pane action {action:?} — {} bytes",
                            text.len()
                        );
                        let primary = matches!(action, TermAction::CopyPrimary);
                        clipboard::write(text, primary, !primary, cx);
                    }
                    None => {
                        println!("dropterm-gpui: pane action {action:?} — nothing selected");
                    }
                }
            }
            TermAction::Paste => match clipboard::read(cx) {
                Some(text) => {
                    println!(
                        "dropterm-gpui: pane action Paste — {} bytes through the engine's paste",
                        text.len()
                    );
                    self.terminal.lock().expect("engine lock").paste(&text);
                    self.needs_update = true;
                }
                None => {
                    println!("dropterm-gpui: pane action Paste — the clipboard holds no text");
                }
            },
            TermAction::PastePrimary => {
                // The primary selection has no GPUI API: one
                // data-control read off the UI thread, then the same
                // engine paste.
                println!("dropterm-gpui: pane action PastePrimary — data-control read");
                let terminal = Arc::clone(&self.terminal);
                cx.spawn(async move |this, cx| {
                    let text = cx
                        .background_executor()
                        .spawn(async move { clipboard::read_primary_blocking() })
                        .await;
                    if let Some(text) = text {
                        let _ = this.update(cx, |pane, cx| {
                            println!(
                                "dropterm-gpui: paste primary — {} bytes through the engine's paste",
                                text.len()
                            );
                            terminal.lock().expect("engine lock").paste(&text);
                            pane.needs_update = true;
                            cx.notify();
                        });
                    }
                })
                .detach();
            }
            TermAction::SelectAll => {
                println!("dropterm-gpui: pane action SelectAll (every row selected)");
                self.terminal.lock().expect("engine lock").select_all();
                self.needs_update = true;
            }
            TermAction::ClearScrollback => {
                println!("dropterm-gpui: pane action ClearScrollback (history dropped)");
                self.terminal
                    .lock()
                    .expect("engine lock")
                    .term
                    .lock()
                    .grid_mut()
                    .clear_history();
                self.needs_update = true;
            }
            TermAction::ZoomIn | TermAction::ZoomOut | TermAction::ZoomReset => {
                let mut terminal = self.terminal.lock().expect("engine lock");
                let adj = match action {
                    TermAction::ZoomIn => terminal.zoom_adj().saturating_add(1),
                    TermAction::ZoomOut => terminal.zoom_adj().saturating_sub(1),
                    _ => 0,
                };
                println!("dropterm-gpui: pane action {action:?} (zoom_adj {adj})");
                // The metrics reapply *is* the zoom: `set_config`
                // recomputes metrics from the live `zoom_adj` and
                // re-fires the cell resize from there.
                terminal.set_zoom_adj(adj);
                let options = self.options.clone();
                let colors = self.skin.colors;
                terminal.set_config(&options, &colors);
                drop(terminal);
                self.needs_update = true;
            }
            TermAction::Hide => {
                println!(
                    "dropterm-gpui: pane hide action (the shell wiring routes it with the integration task)"
                );
            }
        }
        cx.notify();
    }

    /// One key press through the translation and its outcome.
    fn on_key(&mut self, keystroke: &gpui::Keystroke, cx: &mut Context<Self>) {
        // The walk affordance: the exact keystroke shape the platform
        // delivered (key, modifiers, key_char) — rig/VM input walks
        // set `DROPTERM_TRACE_KEYS=1` to see what actually arrives.
        if std::env::var_os("DROPTERM_TRACE_KEYS").is_some() {
            println!(
                "dropterm-gpui: key {:?} modifiers ctrl={} shift={} alt={} super={} key_char={:?}",
                keystroke.key,
                keystroke.modifiers.control,
                keystroke.modifiers.shift,
                keystroke.modifiers.alt,
                keystroke.modifiers.platform,
                keystroke.key_char
            );
        }
        let modes = {
            let terminal = self.terminal.lock().expect("engine lock");
            let term = terminal.term.lock();
            KeyModes {
                app_cursor: term
                    .mode()
                    .contains(alacritty_terminal::term::TermMode::APP_CURSOR),
                has_selection: term.selection.is_some(),
            }
        };
        match input::translate(keystroke, &self.binds, modes) {
            KeyInput::Bytes(bytes) => {
                // Escape's selection-cancel is the one swallow with a
                // visible side effect: clear it, repaint.
                if bytes.is_empty() && modes.has_selection {
                    self.terminal
                        .lock()
                        .expect("engine lock")
                        .term
                        .lock()
                        .selection = None;
                    self.needs_update = true;
                    cx.notify();
                    return;
                }
                self.terminal
                    .lock()
                    .expect("engine lock")
                    .input_scroll(bytes);
            }
            KeyInput::Action(action) => self.perform_action(action, cx),
            KeyInput::Scroll(scroll) => {
                println!("dropterm-gpui: pane scroll {scroll:?} (Shift+PageUp/Down/Home/End)");
                self.terminal.lock().expect("engine lock").scroll(scroll);
                self.needs_update = true;
                cx.notify();
            }
            KeyInput::Swallowed => {
                if modes.has_selection {
                    // The swallow-with-selection rule: cancel it.
                    self.terminal
                        .lock()
                        .expect("engine lock")
                        .term
                        .lock()
                        .selection = None;
                    self.needs_update = true;
                    cx.notify();
                }
            }
        }
    }
}

impl TerminalPane {
    /// The child-owns-pointer gate (the widget's rule, verbatim): a
    /// mouse-reporting program owns the pointer unless Shift is held
    /// — the standard bypass so a selection can form under a
    /// mouse-grabbing editor — and a gesture that started bypassed
    /// stays bypassed (`reporting_gesture` false) so the child never
    /// sees a release for a press it never saw.
    fn pointer_child_owns(&self, shift: bool) -> bool {
        let mouse_reporting = {
            let terminal = self.terminal.lock().expect("engine lock");
            let term = terminal.term.lock();
            term.mode().intersects(TermMode::MOUSE_MODE)
        };
        mouse_reporting && !shift && !self.reporting_gesture
    }

    /// One pointer event the child owns through the core encoder, at
    /// the cell the position lands in.
    fn report_pointer(
        &mut self,
        event: PointerEvent,
        position: gpui::Point<gpui::Pixels>,
        modifiers: gpui::Modifiers,
    ) {
        let terminal = self.terminal.lock().expect("engine lock");
        let size = terminal.size();
        let (col, row) = selection::cell_at(
            position.x.as_f32(),
            position.y.as_f32(),
            size.cell_width,
            size.cell_height,
        );
        #[expect(
            clippy::cast_possible_truncation,
            reason = "clamped cell coordinates over a grid far below the u32 row"
        )]
        let (col, row) = (col as u32, row as u32);
        mouse_reporter::report_mouse(
            &terminal,
            &mut self.reporter,
            &event,
            PointerModifiers {
                shift: modifiers.shift,
                alt: modifiers.alt,
                control: modifiers.control,
            },
            col,
            row,
        );
    }

    /// A press into the grid: a mouse-reporting child takes it
    /// (encoding through the core reporter) unless Shift is held;
    /// otherwise the left button runs the click-kind cycle, the
    /// selection anchor (or a Shift-press extending the standing
    /// one), and the drag arm.
    fn on_press(
        &mut self,
        button: gpui::MouseButton,
        position: gpui::Point<gpui::Pixels>,
        modifiers: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        let neutral = match button {
            gpui::MouseButton::Left => PointerButton::Left,
            gpui::MouseButton::Middle => PointerButton::Middle,
            gpui::MouseButton::Right => PointerButton::Right,
            gpui::MouseButton::Navigate(_) => return,
        };
        if std::env::var_os("DROPTERM_TRACE_KEYS").is_some() {
            println!(
                "dropterm-gpui: press {neutral:?} at ({:.0},{:.0}) shift={}",
                position.x.as_f32(),
                position.y.as_f32(),
                modifiers.shift
            );
        }
        if self.pointer_child_owns(modifiers.shift) {
            self.reporting_gesture = true;
            self.report_pointer(PointerEvent::Pressed(neutral), position, modifiers);
            return;
        }
        if neutral != PointerButton::Left {
            return;
        }
        let (cell, geometry) = {
            let terminal = self.terminal.lock().expect("engine lock");
            let size = terminal.size();
            let (col, row) = selection::cell_at(
                position.x.as_f32(),
                position.y.as_f32(),
                size.cell_width,
                size.cell_height,
            );
            (col, (size.cell_width, size.cell_height, row))
        };
        let (_, _, row) = geometry;
        let within_window = self
            .click
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() < std::time::Duration::from_millis(500));
        let kind = selection::next_click_kind(self.click.map(|(kind, _)| kind), within_window);
        self.click = Some((kind, std::time::Instant::now()));
        self.dragging = true;
        {
            let mut terminal = self.terminal.lock().expect("engine lock");
            let location = terminal
                .viewport_to_point(alacritty_terminal::index::Point::new(row, Column(cell)));
            let side = selection::side_of(position.x.as_f32(), geometry.0);
            {
                let mut term = terminal.term.lock();
                if modifiers.shift
                    && let Some(ref mut standing) = term.selection
                {
                    standing.update(location, side);
                } else {
                    term.selection = Some(alacritty_terminal::selection::Selection::new(
                        selection::selection_kind_of(kind),
                        location,
                        side,
                    ));
                }
            }
            terminal.needs_update = true;
        }
        cx.notify();
    }

    /// A motion: a reported gesture extends through the encoder
    /// (motion only encodes while a button holds — the reporter's
    /// own rule), a selection drag extends the selection.
    fn on_motion(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        modifiers: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        if self.reporting_gesture {
            self.report_pointer(PointerEvent::Moved, position, modifiers);
            return;
        }
        self.on_left_drag(position, cx);
    }

    /// A drag motion while the left button holds: extend the
    /// selection to the pointer's cell.
    fn on_left_drag(&mut self, position: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        {
            let mut terminal = self.terminal.lock().expect("engine lock");
            let size = terminal.size();
            let (col, row) = selection::cell_at(
                position.x.as_f32(),
                position.y.as_f32(),
                size.cell_width,
                size.cell_height,
            );
            let location =
                terminal.viewport_to_point(alacritty_terminal::index::Point::new(row, Column(col)));
            let edge = selection::side_of(position.x.as_f32(), size.cell_width);
            {
                let mut term = terminal.term.lock();
                if let Some(ref mut standing) = term.selection {
                    standing.update(location, edge);
                }
            }
            terminal.needs_update = true;
        }
        cx.notify();
    }

    /// A release: a reported gesture ends through the encoder
    /// (whatever button it pressed); the left button finalizes the
    /// selection. The libcosmic rows carry no copy-on-select — the
    /// selection stands until Copy grabs it — so this release only
    /// logs the byte count a Copy would take.
    fn on_release(
        &mut self,
        button: gpui::MouseButton,
        position: gpui::Point<gpui::Pixels>,
        modifiers: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        let neutral = match button {
            gpui::MouseButton::Left => PointerButton::Left,
            gpui::MouseButton::Middle => PointerButton::Middle,
            gpui::MouseButton::Right => PointerButton::Right,
            gpui::MouseButton::Navigate(_) => return,
        };
        if self.reporting_gesture {
            self.report_pointer(PointerEvent::Released(neutral), position, modifiers);
            self.reporting_gesture = false;
            return;
        }
        if neutral != PointerButton::Left {
            return;
        }
        self.dragging = false;
        let selected = {
            let mut terminal = self.terminal.lock().expect("engine lock");
            let text = {
                let term = terminal.term.lock();
                term.selection_to_string()
            };
            if let Some(text) = &text {
                println!(
                    "dropterm-gpui: selection finalized ({} bytes; Copy takes them)",
                    text.len()
                );
            }
            terminal.needs_update = text.is_some();
            text.is_some()
        };
        if selected {
            cx.notify();
        }
    }

    /// One wheel scroll: a mouse-reporting program receives its
    /// bytes (SGR wheel codes, or the arrow fallback when it did not
    /// ask for SGR); the alt screen has no scrollback so it gets the
    /// arrow keys; otherwise the wheel drives the scrollback by
    /// whole lines.
    fn on_wheel(
        &mut self,
        delta: gpui::ScrollDelta,
        position: gpui::Point<gpui::Pixels>,
        modifiers: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        if std::env::var_os("DROPTERM_TRACE_KEYS").is_some() {
            let (kind, y) = match delta {
                gpui::ScrollDelta::Lines(lines) => ("lines", lines.y),
                gpui::ScrollDelta::Pixels(pixels) => ("pixels", pixels.y.as_f32()),
            };
            println!("dropterm-gpui: wheel delta {kind} y={y}");
        }
        let neutral_delta = match delta {
            gpui::ScrollDelta::Lines(lines) => PointerScrollDelta::Lines {
                x: lines.x,
                y: lines.y,
            },
            gpui::ScrollDelta::Pixels(pixels) => PointerScrollDelta::Pixels {
                x: pixels.x.as_f32(),
                y: pixels.y.as_f32(),
            },
        };
        let pointer_modifiers = PointerModifiers {
            shift: modifiers.shift,
            alt: modifiers.alt,
            control: modifiers.control,
        };
        let mode = {
            let terminal = self.terminal.lock().expect("engine lock");
            let term = terminal.term.lock();
            *term.mode()
        };
        if mode.intersects(TermMode::MOUSE_MODE) {
            let terminal = self.terminal.lock().expect("engine lock");
            let size = terminal.size();
            let (col, row) = selection::cell_at(
                position.x.as_f32(),
                position.y.as_f32(),
                size.cell_width,
                size.cell_height,
            );
            #[expect(
                clippy::cast_possible_truncation,
                reason = "clamped cell coordinates over a grid far below the u32 row"
            )]
            let (col, row) = (col as u32, row as u32);
            mouse_reporter::scroll_mouse(
                &terminal,
                &mut self.reporter,
                neutral_delta,
                pointer_modifiers,
                col,
                row,
            );
            return;
        }
        if mode.contains(TermMode::ALT_SCREEN) {
            // The alt screen has no scrollback: core's arrow
            // fallback, three per line, app-cursor forms respected.
            let terminal = self.terminal.lock().expect("engine lock");
            mouse_reporter::scroll_as_arrows(&terminal, &mut self.reporter, neutral_delta);
            return;
        }
        let y = match delta {
            gpui::ScrollDelta::Lines(lines) => lines.y,
            gpui::ScrollDelta::Pixels(pixels) => {
                let terminal = self.terminal.lock().expect("engine lock");
                pixels.y.as_f32() / terminal.size().cell_height
            }
        };
        self.scroll_lines += y;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "whole accumulated wheel lines: a notch count far below i32::MAX"
        )]
        let lines = self.scroll_lines as i32;
        if lines == 0 {
            return;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "whole wheel lines: exact in f32"
        )]
        let consumed = lines as f32;
        self.scroll_lines -= consumed;
        let scroll = dropterm_core::terminal::TerminalScroll::Delta(-lines);
        let terminal = self.terminal.lock().expect("engine lock");
        terminal.scroll(scroll);
        cx.notify();
    }
}

impl gpui::Render for TerminalPane {
    #[expect(
        clippy::too_many_lines,
        reason = "the frame registers its three buttons' press/release pairs, motion, wheel, and keys — one wiring block, uniform per button"
    )]
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> impl gpui::IntoElement {
        // The pane takes the keyboard when it renders into a window
        // (the drawer holds it by construction); focusing an already
        // focused handle is a no-op.
        self.focus.focus(window, cx);
        let terminal = Arc::clone(&self.terminal);
        let skin = self.skin.clone();
        let walk_log = Arc::clone(&self.walk_log);
        let pane = cx.entity();
        // The focusable frame: keys and the pointer route to the
        // pane, the canvas
        // paints inside it (carrying its own `size_full` — a
        // default-styled child collapses; the first walk's lesson).
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_mouse_down(gpui::MouseButton::Left, {
                let pane = pane.clone();
                move |event: &gpui::MouseDownEvent, _window, cx| {
                    pane.update(cx, |pane, cx| {
                        pane.on_press(gpui::MouseButton::Left, event.position, event.modifiers, cx);
                    });
                }
            })
            .on_mouse_down(gpui::MouseButton::Middle, {
                let pane = pane.clone();
                move |event: &gpui::MouseDownEvent, _window, cx| {
                    pane.update(cx, |pane, cx| {
                        pane.on_press(
                            gpui::MouseButton::Middle,
                            event.position,
                            event.modifiers,
                            cx,
                        );
                    });
                }
            })
            .on_mouse_down(gpui::MouseButton::Right, {
                let pane = pane.clone();
                move |event: &gpui::MouseDownEvent, _window, cx| {
                    pane.update(cx, |pane, cx| {
                        pane.on_press(
                            gpui::MouseButton::Right,
                            event.position,
                            event.modifiers,
                            cx,
                        );
                    });
                }
            })
            .on_mouse_move({
                let pane = pane.clone();
                move |event: &gpui::MouseMoveEvent, _window, cx| {
                    pane.update(cx, |pane, cx| {
                        pane.on_motion(event.position, event.modifiers, cx);
                    });
                }
            })
            .on_mouse_up(gpui::MouseButton::Left, {
                let pane = pane.clone();
                move |event: &gpui::MouseUpEvent, _window, cx| {
                    pane.update(cx, |pane, cx| {
                        pane.on_release(
                            gpui::MouseButton::Left,
                            event.position,
                            event.modifiers,
                            cx,
                        );
                    });
                }
            })
            .on_mouse_up(gpui::MouseButton::Middle, {
                let pane = pane.clone();
                move |event: &gpui::MouseUpEvent, _window, cx| {
                    pane.update(cx, |pane, cx| {
                        pane.on_release(
                            gpui::MouseButton::Middle,
                            event.position,
                            event.modifiers,
                            cx,
                        );
                    });
                }
            })
            .on_mouse_up(gpui::MouseButton::Right, {
                let pane = pane.clone();
                move |event: &gpui::MouseUpEvent, _window, cx| {
                    pane.update(cx, |pane, cx| {
                        pane.on_release(
                            gpui::MouseButton::Right,
                            event.position,
                            event.modifiers,
                            cx,
                        );
                    });
                }
            })
            .on_scroll({
                let pane = pane.clone();
                move |event: &gpui::ScrollEvent, _window, cx| {
                    pane.update(cx, |pane, cx| {
                        pane.on_wheel(event.delta, event.position, event.modifiers, cx);
                    });
                }
            })
            .on_key_down({
                let pane = pane.clone();
                move |event: &gpui::KeyDownEvent, _window, cx| {
                    pane.update(cx, |pane, cx| pane.on_key(&event.keystroke, cx));
                }
            })
            .child(
                canvas(
                    move |bounds, _, _| bounds,
                    move |bounds, _, window, cx| {
                        paint_pane(&terminal, &skin, &walk_log, bounds, window, cx);
                    },
                )
                .size_full(),
            )
    }
}

/// One pane's paint: resize to the granted bounds, drain the update
/// flag, then walk the visible grid through the projection and the
/// shaper. One function reads the whole frame together — the grid
/// walk, the selection overlay, and the evidence rows share the one
/// lock and the one geometry; splitting them re-derives the lock
/// order per piece.
#[expect(
    clippy::too_many_lines,
    reason = "the frame reads one lock and one geometry end to end; the walk, the overlay, and the evidence rows are one pass"
)]
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

    // The selection overlay: the range projected onto row spans,
    // the pane foreground at quarter alpha (the repo's text-selection
    // convention). Absolute line L sits at display row L + offset.
    if let Some(range) = term.selection.as_ref().and_then(|s| s.to_range(&term)) {
        let fg = hsla_fg(&skin.colors);
        let mut overlay = gpui::hsla(fg.h, fg.s, fg.l, 0.25);
        for span in selection::spans(&range, cols) {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                clippy::cast_sign_loss,
                reason = "a selected line's absolute index plus the offset is a display row: bounded by the grid's own sizes"
            )]
            let row = (span.line + display_offset as i32) as usize;
            if row >= display_rows.len() {
                continue;
            }
            overlay.a = 0.25;
            #[expect(
                clippy::cast_precision_loss,
                reason = "grid columns and rows are far below 2^24: exact in f32"
            )]
            let (x, y, w) = (
                geometry.cell_width * span.start as f32,
                geometry.cell_height * row as f32,
                geometry.cell_width * (span.end - span.start) as f32,
            );
            let origin = point(bounds.origin.x + x, bounds.origin.y + y);
            let size = gpui::Size::new(w, geometry.cell_height);
            window.paint_quad(fill(gpui::Bounds { origin, size }, overlay));
        }
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

/// The scheme's foreground as GPUI color (the selection overlay's
/// tint).
fn hsla_fg(colors: &Colors) -> gpui::Hsla {
    let fg = grid::resolve_color(
        colors,
        alacritty_terminal::vte::ansi::Color::Named(
            alacritty_terminal::vte::ansi::NamedColor::Foreground,
        ),
    );
    gpui::rgb(u32::from(fg.r) << 16 | u32::from(fg.g) << 8 | u32::from(fg.b)).into()
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
#[expect(
    clippy::too_many_arguments,
    reason = "the pane's construction inputs: the engine, the scheme trio, the keybind overlay, and the event channel — a param struct would hide the spawn's one-shot shape"
)]
pub(crate) fn pane_with_pump(
    cx: &mut App,
    terminal: Arc<Mutex<Terminal>>,
    colors: &Colors,
    opacity: u8,
    font: gpui::Font,
    options: TerminalOptions,
    custom_binds: &std::collections::BTreeMap<String, dropterm_core::keybinds::Binding>,
    events: mpsc::UnboundedReceiver<(TabId, TermEvent)>,
) -> Entity<TerminalPane> {
    let binds = input::effective_key_binds(custom_binds);
    let focus = cx.focus_handle();
    cx.new(|cx| {
        let mut pane = TerminalPane::new(terminal, *colors, opacity, font, binds, options, focus);
        pane.spawn_pump(events, cx);
        pane
    })
}
