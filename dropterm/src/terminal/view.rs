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
use gpui_component::ActiveTheme as _;
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
use super::suggest;

/// The walk evidence rows' dedup state (the last logged grid and
/// caret cells — one row per change, not per frame).
#[derive(Default)]
pub(crate) struct WalkLog {
    grid: Option<(usize, usize)>,
    caret: Option<(usize, usize)>,
    rows: Vec<String>,
    /// The last logged ghost's visible string (the autosuggest
    /// rows' evidence: one line per change the suggestion makes).
    ghost: Option<String>,
    /// The last logged menu shape (its row count and selection —
    /// one line per change the menu makes).
    menu: Option<(usize, usize)>,
    /// The image ids whose placement has been logged (the images
    /// rows' evidence: one line per placed image, not per frame).
    placed_images: Vec<u64>,
}

/// A pane's handed-up signal to the shell that owns the tab fleet:
/// the Hide action (the shell's tick runs the machine's hide funnel)
/// and the engine's own exit request (the tabs slice's ungated close
/// — a shell exit never confirms). The standalone walk pane has no
/// shell and only logs.
pub(crate) enum PaneSignal {
    Hide,
    /// The pane's terminal exited; the tab closes through the
    /// ungated tail, never the confirm gate.
    Exited(TabId),
    /// The pane pasted clipboard image bytes (paste-image spec,
    /// PI-1): the cache file's path already pasted as text at the
    /// prompt; the shell records the ledger row on the pane's tab
    /// (the pasted-images chip's input — the panel itself rides the
    /// chrome slices).
    ImagePasted(TabId, std::path::PathBuf, Vec<u8>),
    /// The pane's image paste refused (the audit's F7): the cause
    /// rides to the shell, whose band narrates the verdict — the
    /// paste action's visible failure, never a silent one.
    PasteRefused(dropterm_core::status::PasteRefusal),
}

/// The pane's polled attention latch (the tab markers' input): bell
/// and output signals accumulate between the shell's polls, so a
/// storm of either reads as one row — the marker is a state, not a
/// counter — and a take hands the pair over and resets.
#[derive(Default)]
pub(crate) struct AttentionFlags {
    bell: bool,
    output: bool,
}

impl AttentionFlags {
    fn note_output(&mut self) {
        self.output = true;
    }

    fn note_bell(&mut self) {
        self.bell = true;
    }

    /// Hands the latched pair over — `(bell, output)` — and resets
    /// both for the next window.
    fn take(&mut self) -> (bool, bool) {
        let latched = (self.bell, self.output);
        self.bell = false;
        self.output = false;
        latched
    }
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
    skin: PaneSkin,
    /// The shell's latest title report (stored now, consumed when
    /// the tab chrome slices land).
    title: Option<String>,
    walk_log: Arc<Mutex<WalkLog>>,
    /// The inline-image pixels-to-handles cache (gpui-parity Task 2):
    /// one `RenderImage` per engine image id, shared into the paint
    /// closure like the walk log.
    image_cache: Arc<Mutex<super::images::ImageCache>>,
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
    /// The pane's signals handed up to a shell that owns the tab
    /// fleet (`None` in the standalone walk harness — the Hide
    /// action only logs there).
    shell_signals: Option<std::sync::mpsc::Sender<PaneSignal>>,
    /// The pane's tab id (the Exited signal's address).
    id: TabId,
    /// The latched attention signals since the shell's last poll —
    /// the tab markers' input.
    attention: AttentionFlags,
    /// Whether a suggest-reveal timer sleeps in the task queue
    /// (suggest-delay spec, the libcosmic `reveal_armed` shape): one
    /// at a time; the fire drives one update and re-syncs against
    /// then-current truth.
    reveal_armed: bool,
    /// The focus/activation subscriptions, wired on the first render
    /// (they need a window): the engine's `set_focused` rides real
    /// focus changes, so the ghost and the menu dismiss on focus
    /// loss and the caret state tracks the keyboard.
    focus_wiring: Option<[gpui::Subscription; 3]>,
}

impl TerminalPane {
    /// The shell's current OSC 0/2 report (`None` until the first,
    /// cleared by a reset) — the tab list's title input, polled by
    /// the shell's tick (the tracker the chrome slices consume).
    pub(crate) fn shell_title(&self) -> Option<String> {
        self.title.clone()
    }

    /// The program word this pane was spawned as (spawn truth, the
    /// session snapshot's `shell` input — read once at construction,
    /// never rewritten by `set_config`).
    pub(crate) fn spawn_program(&self) -> &str {
        &self.options.spawned_program
    }

    /// The engine's own working-directory read (the `/proc` cwd —
    /// the same input the libcosmic retitle path caches): the cwd
    /// title modes' input, polled by the shell's tick.
    pub(crate) fn working_directory(&self) -> Option<std::path::PathBuf> {
        self.terminal
            .lock()
            .expect("engine lock")
            .working_directory()
    }

    /// Whether the tab's foreground child is running (the engine's
    /// `/proc` foreground check) — the close gate's detection, read
    /// at request time so a mid-session toggle flip applies to the
    /// very next close.
    pub(crate) fn foreground_child_running(&self) -> bool {
        self.terminal
            .lock()
            .expect("engine lock")
            .foreground_child_running()
    }

    /// Hands the pane's latched attention signals over — `(bell,
    /// output)` since the last poll — and resets the latch (the
    /// shell's tick polls each pane).
    pub(crate) fn take_attention(&mut self) -> (bool, bool) {
        self.attention.take()
    }

    /// The pane's focus handle — the shell's modal hands the keyboard
    /// back through it when its confirmation resolves.
    pub(crate) fn focus_handle(&self) -> gpui::FocusHandle {
        self.focus.clone()
    }

    /// Build the pane around a live engine terminal. The caller
    /// owns the terminal's construction (PTY spawn included) and
    /// hands over the receiver half of the engine's event channel —
    /// [`Self::spawn_pump`] turns it into frame demand.
    #[expect(
        clippy::large_types_passed_by_value,
        reason = "a Copy scheme table, moved once at the pane's construction; the pane owns it"
    )]
    #[expect(
        clippy::too_many_arguments,
        reason = "the pane's construction inputs beside the pump's channel: the id, the engine, the scheme trio, and the keybind overlay — a param struct would hide the spawn's one-shot shape"
    )]
    pub(crate) fn new(
        id: TabId,
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
            skin: PaneSkin {
                colors,
                opacity_alpha: f32::from(opacity.min(100)) / 100.,
                font,
            },
            title: None,
            walk_log: Arc::new(Mutex::new(WalkLog::default())),
            image_cache: Arc::new(Mutex::new(super::images::ImageCache::default())),
            binds,
            options,
            focus,
            click: None,
            dragging: false,
            scroll_lines: 0.,
            reporter: MouseReporter::default(),
            reporting_gesture: false,
            shell_signals: None,
            id,
            attention: AttentionFlags::default(),
            reveal_armed: false,
            focus_wiring: None,
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
                self.terminal.lock().expect("engine lock").needs_update = true;
                // Output latches an attention signal for the shell's
                // poll (the active tab's poll drops it — the markers
                // accrue only for inactive tabs).
                self.attention.note_output();
                self.arm_reveal_after_damage(cx);
                cx.notify();
            }
            TermEvent::Title(title) => {
                self.title = Some(title);
            }
            TermEvent::ResetTitle => {
                self.title = None;
            }
            TermEvent::Bell => {
                // The marker latches here (desktop notifications
                // arrive with the chrome slices).
                self.attention.note_bell();
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
            TermEvent::Exit => self.signal_exit(),
        }
    }

    /// A shell exit hands its close up to the shell that owns the
    /// fleet (the walk pane has none and only logs): the tab closes
    /// through the ungated tail — never the confirm gate.
    fn signal_exit(&self) {
        match &self.shell_signals {
            Some(signals) => {
                let _ = signals.send(PaneSignal::Exited(self.id));
            }
            None => {
                println!("dropterm-gpui: pane exit request (no shell in the walk harness)");
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
            let _ = this.update(cx, |_pane, cx| {
                let reply = format(&data.unwrap_or_default()).into_bytes();
                let mut terminal = terminal.lock().expect("engine lock");
                terminal.input_no_scroll(reply);
                terminal.needs_update = true;
                drop(terminal);
                cx.notify();
            });
        })
        .detach();
    }

    /// The reveal arm for the Wakeup point (suggest-delay spec, the
    /// libcosmic `arm_suggest_reveal_after_damage` port): damage just
    /// landed, but the update that re-steps the suggest model runs in
    /// the paint this notify schedules — the precise sync would read
    /// one state behind. So arm on the pane's own truth: a nonzero
    /// delay with suggestions on and no foreground child (output
    /// while a command runs can never withhold) arms the full-delay
    /// one-shot; the fire drives one update, and the next damage
    /// re-arms.
    fn arm_reveal_after_damage(&mut self, cx: &mut Context<Self>) {
        if self.reveal_armed
            || !self.options.autosuggestions_enabled
            || self.options.autosuggestions_delay_ms == 0
            || self.foreground_child_running()
        {
            return;
        }
        self.reveal_armed = true;
        let delay =
            std::time::Duration::from_millis(u64::from(self.options.autosuggestions_delay_ms));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |pane, cx| {
                // The fired sleep consumed itself: the engine update
                // it drives is the point — the idle fact resolves at
                // update()'s step site, so the ghost appears with no
                // new keystroke.
                pane.reveal_armed = false;
                pane.terminal.lock().expect("engine lock").needs_update = true;
                cx.notify();
            });
        })
        .detach();
    }

    /// The completion menu's routing for one press (the libcosmic
    /// `menu_consumes_press` port): `true` — the menu consumed it (a
    /// claimed key, or the soft chord that opened one); `false` —
    /// nothing claims it, and the press flows through the normal
    /// handling below, its bytes forwarded verbatim.
    fn route_menu_press(
        &mut self,
        keystroke: &gpui::Keystroke,
        trigger_chord: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let consumed;
        {
            let mut engine = self.terminal.lock().expect("engine lock");
            match suggest::menu_routing(
                &keystroke.key,
                keystroke.modifiers,
                engine.menu_is_open(),
                trigger_chord,
            ) {
                Some(suggest::MenuRouting::Consume(key)) => {
                    engine.menu_press(key);
                    engine.needs_update = true;
                    consumed = true;
                }
                Some(suggest::MenuRouting::CloseThenProcess) => {
                    // The press closes the menu, then still processes
                    // below (a claimed key's bytes, an action chord,
                    // an accept) — the repaint now, the press next.
                    engine.menu_close();
                    engine.needs_update = true;
                    consumed = false;
                }
                // The soft trigger: no menu, the chord pressed — try
                // the open; a decline forwards the chord's own shell
                // bytes through the normal handling below.
                None if trigger_chord => {
                    let opened = engine.menu_open();
                    engine.needs_update = opened;
                    consumed = opened;
                }
                None => return false,
            }
        }
        cx.notify();
        consumed
    }

    /// The engine's focus flag over real focus changes (the
    /// libcosmic app's focus-event wiring): the ghost's eligibility,
    /// the menu's open state, and the caret's swap all ride
    /// `set_focused`, so the pane's handle gaining or losing the
    /// keyboard (the drawer, a modal, the window itself) and the
    /// window's own activation land on the engine. Wired on the
    /// first render — the subscriptions need a window.
    fn wire_focus(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        if self.focus_wiring.is_some() {
            return;
        }
        let focus = self.focus.clone();
        let on_focus = cx.on_focus(&focus, window, |pane, _window, cx| {
            let mut engine = pane.terminal.lock().expect("engine lock");
            engine.set_focused(true);
            drop(engine);
            cx.notify();
        });
        let on_blur = cx.on_blur(&focus, window, |pane, _window, cx| {
            let mut engine = pane.terminal.lock().expect("engine lock");
            engine.set_focused(false);
            drop(engine);
            cx.notify();
        });
        let on_activation = cx.observe_window_activation(window, |pane, window, cx| {
            let mut engine = pane.terminal.lock().expect("engine lock");
            engine.set_focused(window.is_window_active());
            drop(engine);
            cx.notify();
        });
        self.focus_wiring = Some([on_focus, on_blur, on_activation]);
    }
}

impl TerminalPane {
    /// Swap the pane's scheme (gpui-theme Task 2, called by the
    /// shell's tick): the engine's config re-applies with the new
    /// colors — the zoom action's `set_config` seam — and the skin
    /// carries them to the next frame's paint, so a live scheme edit
    /// re-themes the terminal with the chrome.
    pub(crate) fn retheme(&mut self, colors: &Colors, cx: &mut Context<Self>) {
        let options = self.options.clone();
        {
            let mut engine = self.terminal.lock().expect("engine lock");
            engine.set_config(&options, colors);
            engine.needs_update = true;
        }
        self.skin.colors = *colors;
        cx.notify();
    }

    /// The live config pass (gpui-parity Task 1): fresh options from
    /// a config echo re-apply through the engine's `set_config`
    /// (spawn truth stays the pane's own — it is read once at
    /// construction and never rewritten), so a Behavior toggle or a
    /// delay edit applies at the next update with no respawn — the
    /// libcosmic config echo's own shape.
    pub(crate) fn apply_options(&mut self, options: TerminalOptions, cx: &mut Context<Self>) {
        let colors = self.skin.colors;
        {
            let mut engine = self.terminal.lock().expect("engine lock");
            engine.set_config(&options, &colors);
            engine.needs_update = true;
        }
        self.options = options;
        cx.notify();
    }

    /// The menu's entry into the pane's action funnel: the same
    /// [`perform_action`] a matched chord drives, exposed for the
    /// context menu's pane-routed picks (gpui-chrome) — the menu
    /// lands its clipboard/scrollback rows in the keybind's own
    /// path, never a parallel one.
    pub(crate) fn perform_term_action(&mut self, action: TermAction, cx: &mut Context<Self>) {
        self.perform_action(action, cx);
    }

    /// The paste-image road (paste-image spec, PI-1; the libcosmic
    /// handler's port): with no text offer standing, one data-control
    /// image read off the UI thread; the bytes land in the paste
    /// cache (core's exclusive-create write) and the FILE'S PATH
    /// pastes as text — the same channel a text paste uses, so
    /// bracketed paste rides unchanged. The path is the paste's one
    /// grid trace; the ledger row records it shell-side through the
    /// [`PaneSignal::ImagePasted`] hand-up (the panel that lists
    /// rows rides the chrome slices).
    #[expect(
        clippy::unused_self,
        reason = "the pane's own funnel shape: called on the pane beside every other action, its body rides the spawned `this` the closure needs"
    )]
    fn paste_clipboard_image(&mut self, primary: bool, cx: &mut Context<Self>) {
        println!(
            "dropterm-gpui: pane action Paste{} — no text offer; trying the image road",
            if primary { "Primary" } else { "" }
        );
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { clipboard::read_image_blocking(primary) })
                .await;
            let bytes = match read {
                clipboard::ImageRead::Bytes(bytes) => bytes,
                clipboard::ImageRead::None => {
                    println!("dropterm-gpui: paste image — no image offer either; nothing pasted");
                    return;
                }
                clipboard::ImageRead::Refused(refusal) => {
                    println!("dropterm-gpui: paste image refused — {refusal:?}");
                    let _ = this.update(cx, |pane, _| {
                        if let Some(signals) = &pane.shell_signals {
                            let _ = signals.send(PaneSignal::PasteRefused(refusal));
                        }
                    });
                    return;
                }
            };
            let cached = cx
                .background_executor()
                .spawn(async move {
                    dropterm_core::paste_cache::write_paste_image(&bytes).map(|path| (path, bytes))
                })
                .await;
            let Some((path, bytes)) = cached else {
                // The cache's own refusals (unresolvable cache dir, a
                // failed write) reach the same verdict the read-side
                // cap refusal does — the paste's one visible failure.
                println!("dropterm-gpui: paste image — the paste cache refused; nothing pasted");
                let _ = this.update(cx, |pane, _| {
                    if let Some(signals) = &pane.shell_signals {
                        let _ = signals.send(PaneSignal::PasteRefused(
                            dropterm_core::status::PasteRefusal::Failed,
                        ));
                    }
                });
                return;
            };
            let _ = this.update(cx, |pane, cx| {
                println!(
                    "dropterm-gpui: paste image — {} pastes at the prompt ({} bytes cached)",
                    path,
                    bytes.len()
                );
                let mut engine = pane.terminal.lock().expect("engine lock");
                engine.paste(&path);
                engine.needs_update = true;
                drop(engine);
                if let Some(signals) = &pane.shell_signals {
                    let _ = signals.send(PaneSignal::ImagePasted(
                        pane.id,
                        std::path::PathBuf::from(&path),
                        bytes,
                    ));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The copy actions' shared body: the selection out of the
    /// engine, the regular write through GPUI's clipboard (the chord
    /// just landed in the focused pane, so focus plus input serial
    /// hold by construction — check 8's ruling), the primary write
    /// through data-control (no GPUI API).
    fn copy_selection(&mut self, primary: bool, cx: &mut Context<Self>) {
        let text = {
            let terminal = self.terminal.lock().expect("engine lock");
            let term = terminal.term.lock();
            term.selection_to_string()
        };
        match text {
            Some(text) => {
                println!(
                    "dropterm-gpui: pane action Copy{} — {} bytes",
                    if primary { "Primary" } else { "" },
                    text.len()
                );
                clipboard::write(text, primary, !primary, cx);
            }
            None => {
                println!(
                    "dropterm-gpui: pane action Copy{} — nothing selected",
                    if primary { "Primary" } else { "" }
                );
            }
        }
    }

    /// One matched action through the pane — the terminal-view
    /// slice's execution (the libcosmic `perform_action` pared to
    /// the pane's own surface; clipboard lands with its task, the
    /// surface hide with the shell wiring).
    fn perform_action(&mut self, action: TermAction, cx: &mut Context<Self>) {
        match action {
            TermAction::Copy | TermAction::CopyPrimary => {
                self.copy_selection(matches!(action, TermAction::CopyPrimary), cx);
            }
            TermAction::Paste => match clipboard::read(cx) {
                Some(text) => {
                    println!(
                        "dropterm-gpui: pane action Paste — {} bytes through the engine's paste",
                        text.len()
                    );
                    let mut engine = self.terminal.lock().expect("engine lock");
                    engine.paste(&text);
                    engine.needs_update = true;
                }
                None => self.paste_clipboard_image(false, cx),
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
                    match text {
                        Some(text) => {
                            let _ = this.update(cx, |_pane, cx| {
                                println!(
                                    "dropterm-gpui: paste primary — {} bytes through the engine's paste",
                                    text.len()
                                );
                                let mut engine = terminal.lock().expect("engine lock");
                                engine.paste(&text);
                                engine.needs_update = true;
                                cx.notify();
                            });
                        }
                        None => {
                            let _ = this.update(cx, |pane, cx| pane.paste_clipboard_image(true, cx));
                        }
                    }
                })
                .detach();
            }
            TermAction::SelectAll => {
                println!("dropterm-gpui: pane action SelectAll (every row selected)");
                let mut engine = self.terminal.lock().expect("engine lock");
                engine.select_all();
                engine.needs_update = true;
            }
            TermAction::ClearScrollback => {
                println!("dropterm-gpui: pane action ClearScrollback (history dropped)");
                let mut engine = self.terminal.lock().expect("engine lock");
                engine.term.lock().grid_mut().clear_history();
                engine.needs_update = true;
            }
            TermAction::ScrollToBottom => {
                println!("dropterm-gpui: pane action ScrollToBottom (viewport at the bottom)");
                let mut engine = self.terminal.lock().expect("engine lock");
                engine.scroll(dropterm_core::terminal::TerminalScroll::Bottom);
                engine.needs_update = true;
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
                terminal.needs_update = true;
                drop(terminal);
            }
            TermAction::Hide => {
                // Handed to the shell that owns the surface lifecycle
                // (its tick runs the machine's hide funnel); the
                // standalone walk pane has no shell and only logs.
                match &self.shell_signals {
                    Some(signals) => {
                        println!("dropterm-gpui: pane hide action -> the shell's funnel");
                        let _ = signals.send(PaneSignal::Hide);
                    }
                    None => {
                        println!("dropterm-gpui: pane hide action (no shell in the walk harness)");
                    }
                }
            }
            // The trigger's own arm is a documented no-op (the
            // libcosmic precedent): the routing in [`Self::on_key`]
            // owns the open and the toggle where a declined chord's
            // shell bytes live. This arm fires only when the map
            // swallow sees the chord unmolested — a declined open
            // that fell through translate, nothing to do.
            TermAction::ShowCompletions => {
                println!("dropterm-gpui: pane show-completions action (the routing owns it)");
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
        // The completion menu's routing (suggest-menu spec), before
        // the keybind swallow and every mapping — a showing menu
        // outranks them all; the soft trigger chord tries the open
        // here, its decline falling through with the chord's own
        // shell bytes still to send.
        let trigger_chord = matches!(
            input::matched_action(&self.binds, keystroke),
            Some(TermAction::ShowCompletions)
        );
        if self.route_menu_press(keystroke, trigger_chord, cx) {
            return;
        }
        // The Space special case (the libcosmic widget's own, verbatim):
        // Ctrl+Space is the NUL press wherever it is not consumed
        // above — the trigger's decline included, never a swallow.
        if keystroke.key == "space" && keystroke.modifiers.control {
            let mut engine = self.terminal.lock().expect("engine lock");
            engine.input_scroll(b"\x00".to_vec());
            engine.needs_update = true;
            drop(engine);
            cx.notify();
            return;
        }
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
                    self.terminal.lock().expect("engine lock").needs_update = true;
                    cx.notify();
                    return;
                }
                // The autosuggest soft keys (autosuggest spec): accept
                // only while the engine shows a suggestion — with
                // nothing showing the arm falls through, and Right
                // maps to its escape code exactly as before.
                if let Some(kind) = suggest::accept_kind(&keystroke.key, keystroke.modifiers)
                    && self
                        .terminal
                        .lock()
                        .expect("engine lock")
                        .suggestion()
                        .is_some()
                {
                    println!("dropterm-gpui: pane accept {kind:?} — the showing suffix lands");
                    let mut engine = self.terminal.lock().expect("engine lock");
                    engine.accept_suggestion(kind);
                    engine.needs_update = true;
                    drop(engine);
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
                let mut engine = self.terminal.lock().expect("engine lock");
                engine.scroll(scroll);
                engine.needs_update = true;
                drop(engine);
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
                    self.terminal.lock().expect("engine lock").needs_update = true;
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
            // The child process owns a right press while reporting is
            // active — the menu's default-mode rule read from the
            // child's side: stop the press here so the pane-hosting
            // context-menu wrap never opens over the child's bytes.
            if neutral == PointerButton::Right {
                cx.stop_propagation();
            }
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
        // focused handle is a no-op. A shell-owned modal outranks the
        // claim: while another handle holds the keyboard (the
        // close-confirm gate), the pane waits — its keys stay the
        // modal's to swallow.
        let another_holds = window
            .focused(cx)
            .is_some_and(|focused| focused != self.focus);
        if !another_holds {
            self.focus.focus(window, cx);
        }
        self.wire_focus(window, cx);
        let terminal = Arc::clone(&self.terminal);
        let skin = self.skin.clone();
        let walk_log = Arc::clone(&self.walk_log);
        let image_cache = Arc::clone(&self.image_cache);
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
                        paint_pane(
                            &terminal,
                            &skin,
                            &walk_log,
                            &image_cache,
                            bounds,
                            window,
                            cx,
                        );
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
    image_cache: &Arc<Mutex<super::images::ImageCache>>,
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

    // Inline images, seat collection (gpui-parity Task 2; the
    // libcosmic widget's two-pass rule): every covered cell carries
    // its normalized sub-rect of the shared image — the draw clips
    // to the cell, the clip the sub-rect. A beneath-layer id (an
    // unrecorded one — every iterm image — or kitty `z<0`) draws
    // under the glyph pass; an above-layer one (kitty `z>=0`)
    // collects for the pass after the glyphs, still below the
    // cursor and the ghost overlays. An evicted id draws blank
    // (OQ-3): the cell stands, the handle dying with the pixels
    // through the store's removal report.
    let mut beneath_images: Vec<(
        gpui::Bounds<Pixels>,
        gpui::Bounds<Pixels>,
        Arc<gpui::RenderImage>,
    )> = Vec::new();
    let mut above_images: Vec<(
        gpui::Bounds<Pixels>,
        gpui::Bounds<Pixels>,
        Arc<gpui::RenderImage>,
    )> = Vec::new();
    {
        let store = Arc::clone(&terminal.images);
        let mut store = store.lock().expect("image store");
        let mut cache = image_cache.lock().expect("image cache");
        cache.sync_removals(store.take_removed());
        for (row, cells) in display_rows.iter().enumerate() {
            for (col, cell) in cells.iter().enumerate() {
                let Some(sub) = cell.image() else {
                    continue;
                };
                let Some(handle) = cache.handle_for(sub.image, || store.get(sub.image).cloned())
                else {
                    continue;
                };
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "cell indices < 2^24, exact in f32"
                )]
                let (x, y) = (
                    bounds.origin.x + geometry.cell_width * col as f32,
                    bounds.origin.y + geometry.cell_height * row as f32,
                );
                let cell_bounds = gpui::Bounds {
                    origin: point(x, y),
                    size: gpui::size(geometry.cell_width, geometry.cell_height),
                };
                let seat = (
                    super::images::image_draw_bounds(cell_bounds, sub),
                    cell_bounds,
                    handle,
                );
                if store.layer_of(sub.image) < 0 {
                    beneath_images.push(seat);
                } else {
                    above_images.push(seat);
                }
                let mut log = walk_log.lock().expect("walk log");
                if !log.placed_images.contains(&sub.image) {
                    log.placed_images.push(sub.image);
                    println!(
                        "dropterm-gpui: pane image {} placed (first covered cell col {col} row {row}, {} layer)",
                        sub.image,
                        if store.layer_of(sub.image) < 0 {
                            "beneath"
                        } else {
                            "above"
                        }
                    );
                }
            }
        }
    }
    // The beneath pass: under the glyphs.
    for (image_bounds, cell_bounds, handle) in beneath_images {
        let _ = window.paint_image(
            cell_bounds,
            image_bounds,
            gpui::Corners::default(),
            handle,
            0,
            false,
        );
    }

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

    // The above-layer pass (kitty z>=0): over the glyphs, under the
    // cursor, the ghost, and the menu that follow.
    for (image_bounds, cell_bounds, handle) in above_images {
        let _ = window.paint_image(
            cell_bounds,
            image_bounds,
            gpui::Corners::default(),
            handle,
            0,
            false,
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

    // The ghost-text overlay (gpui-parity Task 1; the libcosmic
    // widget's rule, same seat): the showing's visible string as dim
    // glyphs starting one cell past the cursor — an overlay, never
    // the grid and never the engine's own buffers, so selection,
    // scrollback, and copy see nothing. The clip already guaranteed
    // the fit, and an empty visible (below the three-column draw
    // threshold) fills nothing while the showing still accepts.
    // Scrolled back renders nothing (the offset gate).
    let ghost_row = usize::try_from(cursor.line.0).unwrap_or(0) + display_offset;
    if display_offset == 0
        && let Some(visible) = terminal.suggestion().map(str::to_string)
        && !visible.is_empty()
    {
        let tint = color_hsla(terminal.ghost_color());
        let shaped = shape_overlay_line(&visible, &skin.font, tint, geometry.font_size, window);
        #[expect(
            clippy::cast_precision_loss,
            reason = "cell indices < 2^24, exact in f32"
        )]
        let origin = point(
            bounds.origin.x + geometry.cell_width * (cursor.column.0 + 1) as f32,
            bounds.origin.y + geometry.cell_height * ghost_row as f32,
        );
        let _ = shaped.paint(
            origin,
            geometry.cell_height,
            TextAlign::Left,
            None,
            window,
            cx,
        );
        let mut log = walk_log.lock().expect("walk log");
        if log.ghost.as_deref() != Some(visible.as_str()) {
            println!(
                "dropterm-gpui: pane ghost now {:?} ({} cells past the caret)",
                visible,
                visible.chars().count()
            );
            log.ghost = Some(visible);
        }
    } else {
        // The ghost's dismissal is evidence too — the dismissal rows
        // (another keystroke, a resize, a focus loss) name it.
        let mut log = walk_log.lock().expect("walk log");
        if log.ghost.take().is_some() {
            println!("dropterm-gpui: pane ghost cleared");
        }
    }

    // The completion-menu overlay (gpui-parity Task 1; the libcosmic
    // panel, same rules): a pure overlay anchored at the cursor's
    // cell — dropping below it, flipping above at the floor, clamped
    // into the view — carrying the visible window from the scroll
    // offset, the selected row's highlight, and right-aligned
    // descriptions in the ghost's own dim tint. The panel surface is
    // the Kit's popover token (theme_gen keeps it on the scheme), an
    // opaque elevated surface over the grid's window background.
    // GPUI's scene interleaves quads and glyphs by paint order, so
    // the panel quad and its rows issued after the grid float above
    // it with no layering ceremony (the libcosmic build needed
    // `with_layer` for the same effect — wgpu sorts by kind there).
    if display_offset == 0 && terminal.menu_is_open() {
        let all_rows = terminal.menu_rows();
        let range = suggest::menu_visible_window(terminal.menu_scroll(), all_rows.len());
        if !range.is_empty() {
            let visible = &all_rows[range.clone()];
            let selected = terminal.menu_selected();
            let label_color = color_hsla(terminal.foreground_color());
            let dim = color_hsla(terminal.ghost_color());
            let mut shaped: Vec<(gpui::ShapedLine, Option<(gpui::ShapedLine, Pixels)>)> =
                Vec::with_capacity(visible.len());
            let mut rows_width = px(0.);
            for row in visible {
                let label = shape_overlay_line(
                    &row.label,
                    &skin.font,
                    label_color,
                    geometry.font_size,
                    window,
                );
                let description = row.description.as_deref().map(|text| {
                    shape_overlay_line(text, &skin.font, dim, geometry.font_size, window)
                });
                // The description right-aligns at the trailing pad
                // with the ghost's own two-cell clip rhythm between.
                let gap = geometry.cell_width * 2.;
                let desc_width = description.as_ref().map_or(px(0.), |d| d.width);
                rows_width = rows_width.max(label.width + gap + desc_width);
                shaped.push((label, description.map(|d| (d, desc_width))));
            }
            let pad = px(4.);
            #[expect(
                clippy::cast_precision_loss,
                reason = "cell indices < 2^24, exact in f32"
            )]
            let anchor = point(
                bounds.origin.x + geometry.cell_width * cursor.column.0 as f32,
                bounds.origin.y + geometry.cell_height * ghost_row as f32,
            );
            let panel = suggest::menu_panel_bounds(
                anchor,
                geometry.cell_height,
                visible.len(),
                rows_width + pad * 2.,
                bounds,
            );
            window.paint_quad(gpui::PaintQuad {
                bounds: panel,
                background: cx.theme().popover.into(),
                border_color: cx.theme().border,
                corner_radii: gpui::Corners::all(cx.theme().radius),
                border_widths: gpui::Edges::all(px(1.)),
                border_style: gpui::BorderStyle::Solid,
            });
            if range.contains(&selected) {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a visible-row index < the eight-row budget: exact in f32"
                )]
                let row = (selected - range.start) as f32;
                window.paint_quad(fill(
                    gpui::Bounds {
                        origin: point(panel.origin.x, panel.origin.y + geometry.cell_height * row),
                        size: gpui::size(panel.size.width, geometry.cell_height),
                    },
                    cx.theme().secondary_hover,
                ));
            }
            for (index, (label, description)) in shaped.into_iter().enumerate() {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a visible-row index < the eight-row budget: exact in f32"
                )]
                let row = index as f32;
                let origin = point(
                    panel.origin.x + pad,
                    panel.origin.y + geometry.cell_height * row,
                );
                let _ = label.paint(
                    origin,
                    geometry.cell_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
                if let Some((description, width)) = description {
                    let origin = point(panel.origin.x + panel.size.width - pad - width, origin.y);
                    let _ = description.paint(
                        origin,
                        geometry.cell_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }
            }
            let mut log = walk_log.lock().expect("walk log");
            let shape = (all_rows.len(), selected);
            if log.menu != Some(shape) {
                println!(
                    "dropterm-gpui: pane menu now {} rows (selected {}) — {:?}",
                    all_rows.len(),
                    selected,
                    visible.iter().map(|r| r.label.clone()).collect::<Vec<_>>()
                );
                log.menu = Some(shape);
            }
        }
    }
}

/// A cosmic-text color as GPUI color (the ghost tint and the menu's
/// text seats — the engine hands scheme colors over in cosmic-text's
/// shape).
fn color_hsla(color: cosmic_text::Color) -> gpui::Hsla {
    gpui::rgb(u32::from(color.r()) << 16 | u32::from(color.g()) << 8 | u32::from(color.b())).into()
}

/// One overlay text line — the ghost's visible string or a menu row
/// — shaped once in the pane's font at its color, natural width (no
/// cell-grid forcing: the overlays are not the grid).
fn shape_overlay_line(
    text: &str,
    font: &gpui::Font,
    color: gpui::Hsla,
    font_size: Pixels,
    window: &mut gpui::Window,
) -> gpui::ShapedLine {
    window.text_system().shape_line(
        text.into(),
        font_size,
        &[gpui::TextRun {
            len: text.len(),
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    )
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
/// preference's own dark/light; `System` still resolves dark (the
/// drawer default): gpui-theme shipped the config-pass re-theme, and
/// a live system-mode watch rides a later slice.
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
    id: TabId,
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
        let mut pane =
            TerminalPane::new(id, terminal, *colors, opacity, font, binds, options, focus);
        pane.spawn_pump(events, cx);
        pane
    })
}

/// The per-tab appearance a spawn resolves — the profile seams'
/// `profile.override ?? global` per field (scheme, font, scrollback),
/// resolved once here so the pane's boot and the suggest options read
/// one shape.
#[derive(Clone)]
pub(crate) struct PaneAppearance {
    pub colors: Colors,
    pub font_size: u16,
    pub scrollback: u32,
}

/// The pane's spawn request (py-5): everything `boot_pane` needs
/// beyond the config, resolved from a [`SpawnCommand`] through core's
/// one spawn resolution — the PTY shape (the command-then-shell wrap,
/// the degradations, the env merge), the suggest gate's spawn truth
/// (the command's program, never the wrapper), the profile id the
/// spawn ran under (recorded on the tab, saved by the session), and
/// the per-tab appearance the id resolves.
#[derive(Clone)]
pub(crate) struct PaneRequest {
    pub program: String,
    pub args: Vec<String>,
    /// The directory the PTY execs in (`None` inherits this
    /// process's — the session restore's validated cwd rides here).
    pub cwd: Option<std::path::PathBuf>,
    /// The env map merged over the inherited environment at exec
    /// (empty = inherit untouched).
    pub env: std::collections::HashMap<String, String>,
    /// The program word the tab is *spawned as* — the path provider's
    /// allowlist gates on the command's program, never the wrap.
    pub spawned_as: String,
    /// The profile id this spawn runs under, `None` on every plain
    /// and degraded arm.
    pub profile_id: Option<String>,
    pub appearance: PaneAppearance,
}

impl PaneRequest {
    /// Resolves a [`SpawnCommand`] against the config into the pane's
    /// boot shape: core's `spawn_parts` (the one resolution both
    /// frontends share) plus the appearance seams re-resolving the
    /// profile id (the libcosmic `spawn_terminal`'s own chain — the
    /// preset's overrides, else the global picks; the plain arm is
    /// literally [`boot_colors`], the gm-2 one-chain pin).
    pub(crate) fn for_command(
        command: &dropterm_core::spawn::SpawnCommand,
        cwd: Option<std::path::PathBuf>,
        config: &dropterm_core::config::DropTermConfig,
    ) -> Self {
        let parts = dropterm_core::spawn::spawn_parts(command, config);
        let profile = parts
            .profile_id
            .as_deref()
            .and_then(|id| dropterm_core::profile::find(&config.profiles, id));
        let colors = match profile {
            // The boot-time mode resolution mirrors `boot_colors`'s
            // (the one-chain pin): `System` resolves dark until the
            // live system-mode watch ships.
            Some(profile) => {
                let is_dark =
                    dropterm_core::theme_glue::effective_is_dark(config.theme_preference, true);
                dropterm_core::theme_glue::effective_colors(config, Some(profile), is_dark)
            }
            None => boot_colors(config),
        };
        let appearance = PaneAppearance {
            colors,
            font_size: dropterm_core::profile::effective_font_size(profile, config.font_size),
            scrollback: dropterm_core::profile::effective_scrollback(
                profile,
                config.scrollback_lines,
            ),
        };
        Self {
            program: parts.program,
            args: parts.trailing,
            cwd,
            env: parts.env,
            spawned_as: dropterm_core::spawn::spawned_as_program(command, config),
            profile_id: parts.profile_id,
            appearance,
        }
    }
}

/// The one construction path for a pane and its engine terminal —
/// the walk harness and the app shell share it (gpui-terminal Task
/// 4's integration): request → host → PTY → engine → the pane entity
/// with its pump and the config's keybind overlay. The request is
/// core's spawn resolution applied ([`PaneRequest::for_command`] —
/// the wrap, the degradations, the env merge, the per-profile
/// appearance); the plain default spawns ride `default_spawn_command`
/// so a configured `default_profile` owns them exactly as on
/// libcosmic. The completions table is the app's shared load
/// (async-completions): `None` means the background load has not
/// landed yet, and the pane carries the empty placeholder plus the
/// loading flag until the load's `set_config` pass swaps the real
/// table in. The optional channel carries the pane's handed-up
/// signals (its Hide action, its terminal's exit request) to a shell
/// that owns the tab fleet (the walk's standalone pane passes `None`
/// and only logs).
pub(crate) fn boot_pane(
    cx: &mut App,
    config: &dropterm_core::config::DropTermConfig,
    request: PaneRequest,
    specs_table: Option<&Arc<[dropterm_core::terminal::suggest::SpecCommand]>>,
    id: TabId,
    shell_signals: Option<std::sync::mpsc::Sender<PaneSignal>>,
) -> Option<(Arc<Mutex<Terminal>>, Entity<TerminalPane>)> {
    let colors = request.appearance.colors;
    let host = crate::terminal::host::GpuiHost::new(cx);
    let font = host.font();
    println!(
        "dropterm-gpui: pane pty spawning {} {:?} (spawned as {})",
        request.program, request.args, request.spawned_as
    );
    let pty_options = alacritty_terminal::tty::Options {
        shell: Some(alacritty_terminal::tty::Shell::new(
            request.program,
            request.args,
        )),
        working_directory: request.cwd,
        drain_on_exit: true,
        env: request.env,
    };
    let mut options = suggest::pane_options(config, specs_table);
    options.spawned_program = request.spawned_as;
    options.font_size = request.appearance.font_size;
    let term_config = alacritty_terminal::term::Config {
        scrolling_history: request.appearance.scrollback as usize,
        ..alacritty_terminal::term::Config::default()
    };
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let terminal = Terminal::new(
        Box::new(host),
        id,
        event_tx,
        term_config,
        &pty_options,
        &options,
        &colors,
    )
    .map_err(|err| eprintln!("dropterm-gpui: pane pty spawn failed: {err:?}"))
    .ok()?;
    let terminal = Arc::new(Mutex::new(terminal));
    let pane = pane_with_pump(
        cx,
        id,
        Arc::clone(&terminal),
        &colors,
        config.opacity,
        font,
        options,
        &config.custom_key_binds,
        event_rx,
    );
    pane.update(cx, |pane, _| pane.shell_signals = shell_signals);
    Some((terminal, pane))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- the pane request over core's spawn resolution (py-5) ---

    fn request_config() -> dropterm_core::config::DropTermConfig {
        dropterm_core::config::DropTermConfig {
            shell: Some("/bin/sh".to_string()),
            ..dropterm_core::config::DropTermConfig::default()
        }
    }

    fn profile_command(name: &str, command: &str) -> dropterm_core::spawn::SpawnCommand {
        dropterm_core::spawn::SpawnCommand::Profile {
            name: name.to_string(),
            command: command.to_string(),
            cwd: None,
            env: None,
        }
    }

    #[test]
    fn a_profile_request_carries_the_wrap_the_identity_and_the_overrides() {
        // The one GPUI-side composition pin: core's parts (the
        // command-then-shell wrap, the env merge), the spawn truth
        // (the command's program, never the wrapper), the recorded
        // id, and the appearance seams' `profile.override ?? global`
        // per field. The env map rides the command (spawn applies a
        // profile, never looks one up) — the way every resolver
        // builds the arm.
        let mut config = request_config();
        config.font_size = 14;
        config.scrollback_lines = 10_000;
        let env =
            std::collections::BTreeMap::from([("DROPTERM_PROFILE".to_string(), "top".to_string())]);
        config.profiles = vec![dropterm_core::profile::Profile {
            name: "top".to_string(),
            command: "htop".to_string(),
            cwd: None,
            color_scheme: None,
            env: Some(env.clone()),
            font_size: Some(11),
            scrollback_lines: Some(500),
        }];
        let request = PaneRequest::for_command(
            &dropterm_core::spawn::SpawnCommand::Profile {
                name: "top".to_string(),
                command: "htop".to_string(),
                cwd: None,
                env: Some(env),
            },
            None,
            &config,
        );
        assert_eq!(request.program, "/bin/sh");
        assert_eq!(request.args, vec!["-c", "htop; exec /bin/sh"]);
        assert_eq!(
            request.env.get("DROPTERM_PROFILE").map(String::as_str),
            Some("top")
        );
        assert_eq!(request.spawned_as, "htop");
        assert_eq!(request.profile_id.as_deref(), Some("top"));
        assert_eq!(request.appearance.font_size, 11);
        assert_eq!(request.appearance.scrollback, 500);
    }

    #[test]
    fn a_plain_shell_request_resolves_global_appearance_and_no_identity() {
        // The plain arm: no env merge (the pty inherits untouched),
        // no recorded id, and every appearance field global.
        let mut config = request_config();
        config.font_size = 15;
        config.scrollback_lines = 4_000;
        config.profiles = vec![dropterm_core::profile::Profile {
            name: "top".to_string(),
            command: "htop".to_string(),
            cwd: None,
            color_scheme: None,
            env: None,
            font_size: Some(11),
            scrollback_lines: Some(500),
        }];
        let request = PaneRequest::for_command(
            &dropterm_core::spawn::SpawnCommand::Shell("/bin/sh".to_string()),
            None,
            &config,
        );
        assert!(request.env.is_empty());
        assert_eq!(request.profile_id, None);
        assert_eq!(request.spawned_as, "/bin/sh");
        assert_eq!(request.appearance.font_size, 15);
        assert_eq!(request.appearance.scrollback, 4_000);
    }

    #[test]
    fn the_blank_line_profile_degrades_with_no_identity() {
        // The one degradation left (a hand-edited blank command line):
        // the plain shell, no id, no env — never half a profile's
        // identity — and the global appearance.
        let mut config = request_config();
        config.profiles = vec![dropterm_core::profile::Profile {
            name: "broken".to_string(),
            command: "   ".to_string(),
            cwd: None,
            color_scheme: None,
            env: None,
            font_size: Some(9),
            scrollback_lines: Some(100),
        }];
        let request = PaneRequest::for_command(&profile_command("broken", "   "), None, &config);
        assert_eq!(request.program, "/bin/sh");
        assert!(request.env.is_empty());
        assert_eq!(request.profile_id, None);
        assert_eq!(request.appearance.font_size, config.font_size);
    }

    #[test]
    fn attention_flags_latch_signals_and_clear_on_take() {
        // The pane's polled attention seam: signals latch between
        // polls (a bell storm stays one row, an output batch one
        // marker), the bell outranks output, and a take hands the
        // pair over and resets — the next poll starts clean.
        let mut flags = AttentionFlags::default();
        assert_eq!(
            flags.take(),
            (false, false),
            "a fresh pane has signaled nothing"
        );

        flags.note_output();
        assert_eq!(flags.take(), (false, true));

        flags.note_bell();
        flags.note_output();
        assert_eq!(flags.take(), (true, true));
        assert_eq!(flags.take(), (false, false), "the take cleared both");

        flags.note_bell();
        assert_eq!(flags.take(), (true, false));
    }
}
