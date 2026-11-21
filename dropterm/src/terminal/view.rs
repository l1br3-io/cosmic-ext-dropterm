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

use dropterm::core::tabs::TabId;
use dropterm::core::terminal::Terminal;
use dropterm::core::terminal::TerminalOptions;
use dropterm::core::terminal::mouse_reporter::{
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
    PasteRefused(dropterm::core::status::PasteRefusal),
    /// The pane pasted text onto the grid (the covering report's
    /// completion): a plain text paste's trace is as visible as the
    /// image path's, so the shell drops whatever veil stands over
    /// the band — no ledger row, the text itself leaves none.
    TextPasted,
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

/// The config `opacity` (0–100) as the paint alpha — the one math
/// the spawn's bake and the live retune share, so a mid-session
/// flip renders exactly what a fresh tab would.
pub(crate) fn opacity_alpha(opacity: u8) -> f32 {
    f32::from(opacity.min(100)) / 100.
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
    /// The owning shell, weak — the keyboard's shell verbs (tabs,
    /// settings, images, the band) update it from the key handler's
    /// own window, the menus' own callback shape. Weak by design:
    /// the shell's pane map owns this pane, so a strong handle would
    /// cycle; `None` until the shell adopts the pane (and forever in
    /// the standalone walk harness, where the verbs only log).
    /// Linux-only (mb-2): the shell is the layer-shell frontend's;
    /// no pane is adopted before a surface backend exists.
    #[cfg(target_os = "linux")]
    shell: Option<gpui::WeakEntity<crate::app_shell::Shell>>,
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

    /// The shell this pane's PTY was spawned as (the engine's
    /// recorded pid) — the RAM watch's children feed, polled by the
    /// shell's tick (ram-telemetry).
    pub(crate) fn shell_pid(&self) -> Option<u32> {
        self.terminal.lock().expect("engine lock").shell_pid()
    }

    /// The engine's visible screen rows — the toggle-dump trace's
    /// read (`DROPTERM_TOGGLE_DEBUG`): the shell dumps the active
    /// pane's grid at every accepted surface action, under the
    /// engine lock, through this.
    pub(crate) fn visible_rows(&self) -> Vec<String> {
        self.terminal.lock().expect("engine lock").visible_rows()
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
                opacity_alpha: opacity_alpha(opacity),
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
            #[cfg(target_os = "linux")]
            shell: None,
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
                // needs neither focus nor serial. The paste cap rides
                // every clipboard payload (ra-3): a remote pushing
                // past it is dropped, logged, never served.
                let primary = kind == alacritty_terminal::term::ClipboardType::Selection;
                if let Some(refusal) = clipboard::classify_read(text.len()) {
                    println!(
                        "dropterm: osc52 store refused — {} bytes over the paste cap ({refusal:?}, primary={primary})",
                        text.len()
                    );
                } else {
                    println!(
                        "dropterm: osc52 store {} bytes (primary={primary})",
                        text.len()
                    );
                    clipboard::write_data_control(text, primary);
                }
            }
            TermEvent::ClipboardLoad(kind, format) => {
                // OSC 52 load: a program asking for the clipboard —
                // the reply applies the engine's format closure to
                // the loaded text and lands in the asking PTY. An
                // empty clipboard still answers, formatted, never
                // silence (the clipboard-ownership spec's row).
                let primary = kind == alacritty_terminal::term::ClipboardType::Selection;
                println!("dropterm: osc52 load (primary={primary})");
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
                println!("dropterm: pane child exited ({code})");
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
                println!("dropterm: pane exit request (no shell in the walk harness)");
            }
        }
    }

    /// An OSC 52 load's answer: read the asked selection (the
    /// regular clipboard synchronously through GPUI, the primary
    /// selection through one background data-control read), apply
    /// the engine's format closure, and land the reply in the
    /// asking PTY — an empty clipboard still answers, formatted,
    /// never silence (the clipboard-ownership spec's row). An
    /// over-cap clipboard answers nothing: the reply would feed the
    /// whole payload into the PTY, and the paste cap rides every
    /// clipboard payload (ra-3).
    fn load_clipboard_reply(
        &mut self,
        primary: bool,
        format: std::sync::Arc<dyn Fn(&str) -> String + Sync + Send>,
        cx: &mut Context<Self>,
    ) {
        if !primary {
            let data = clipboard::read(cx);
            if clipboard::classify_read(data.as_ref().map_or(0, String::len)).is_some() {
                println!("dropterm: osc52 load reply refused — clipboard over the paste cap");
                return;
            }
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
                if clipboard::classify_read(data.as_ref().map_or(0, String::len)).is_some() {
                    println!("dropterm: osc52 load reply refused — primary over the paste cap");
                    return;
                }
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

    /// The kit-inspector seat's answer (`ctrl-shift-i` in debug
    /// builds — the render-time registration beside this method is
    /// the whole story): the chord runs whatever our table owns on
    /// it, exactly as the key listener would have run it; a table
    /// that no longer owns the chord propagates, and the kit's
    /// inspector keeps the seat.
    #[cfg(debug_assertions)]
    fn on_kit_inspector(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let chord = gpui::Keystroke {
            key: "i".into(),
            key_char: None,
            modifiers: gpui::Modifiers {
                control: true,
                shift: true,
                ..gpui::Modifiers::default()
            },
        };
        match input::matched_action(&self.binds, &chord) {
            Some(action) if action.is_shell_verb() => {
                self.dispatch_shell_verb(action, window, cx);
            }
            Some(action) => {
                self.perform_action(action, cx);
            }
            None => cx.propagate(),
        }
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

    /// Swap the pane's background opacity (the live-opacity fix,
    /// called by the tick's apply point): the alpha is paint-side
    /// only — the frame's whole use is `bg.a *= skin.opacity_alpha`
    /// — so the retune is the field swap and the notify, no engine
    /// call. The spawn's own bake and this pass share the one
    /// [`opacity_alpha`] math, so a live flip lands exactly what a
    /// fresh tab would render.
    pub(crate) fn set_opacity(&mut self, opacity: u8, cx: &mut Context<Self>) {
        self.skin.opacity_alpha = opacity_alpha(opacity);
        cx.notify();
    }

    /// Swap the pane's render family (the live-font fix, called by
    /// the tick's apply point): the skin's paint font and the
    /// engine's measurement host land on the same pre-resolved name —
    /// the engine's `set_font_family` re-measures the cell through
    /// the host when the family actually changed, so the grid
    /// arithmetic follows the face. The spawn's own resolution and
    /// this pass share [`resolve_family`](super::host::resolve_family),
    /// so a live flip renders exactly what a fresh tab would.
    pub(crate) fn set_font_family(&mut self, family: &str, cx: &mut Context<Self>) {
        self.skin.font.family = family.into();
        {
            let mut engine = self.terminal.lock().expect("engine lock");
            engine.set_font_family(family);
        }
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
            "dropterm: pane action Paste{} — no text offer; trying the image road",
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
                    println!("dropterm: paste image — no image offer either; nothing pasted");
                    return;
                }
                clipboard::ImageRead::Refused(refusal) => {
                    println!("dropterm: paste image refused — {refusal:?}");
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
                    dropterm::core::paste_cache::write_paste_image(&bytes).map(|path| (path, bytes))
                })
                .await;
            let Some((path, bytes)) = cached else {
                // The cache's own refusals (unresolvable cache dir, a
                // failed write) reach the same verdict the read-side
                // cap refusal does — the paste's one visible failure.
                println!("dropterm: paste image — the paste cache refused; nothing pasted");
                let _ = this.update(cx, |pane, _| {
                    if let Some(signals) = &pane.shell_signals {
                        let _ = signals.send(PaneSignal::PasteRefused(
                            dropterm::core::status::PasteRefusal::Failed,
                        ));
                    }
                });
                return;
            };
            let _ = this.update(cx, |pane, cx| {
                println!(
                    "dropterm: paste image — {} pastes at the prompt ({} bytes cached)",
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
                    "dropterm: pane action Copy{} — {} bytes",
                    if primary { "Primary" } else { "" },
                    text.len()
                );
                clipboard::write(text, primary, !primary, cx);
            }
            None => {
                println!(
                    "dropterm: pane action Copy{} — nothing selected",
                    if primary { "Primary" } else { "" }
                );
            }
        }
    }

    /// The text half of a paste action: the capped read — regular
    /// through GPUI, primary through one background data-control
    /// read — then the engine paste, the image road on silence, the
    /// band's verdict chip on refusal (ra-3: text offers carry the
    /// paste cap images already had).
    fn paste_text_action(&mut self, primary: bool, cx: &mut Context<Self>) {
        if !primary {
            match clipboard::read_text_capped(cx) {
                clipboard::TextRead::Text(text) => {
                    println!(
                        "dropterm: pane action Paste — {} bytes through the engine's paste",
                        text.len()
                    );
                    let mut engine = self.terminal.lock().expect("engine lock");
                    engine.paste(&text);
                    engine.needs_update = true;
                    drop(engine);
                    if let Some(signals) = &self.shell_signals {
                        let _ = signals.send(PaneSignal::TextPasted);
                    }
                    cx.notify();
                }
                clipboard::TextRead::None => self.paste_clipboard_image(false, cx),
                clipboard::TextRead::Refused(refusal) => {
                    println!("dropterm: paste text refused — {refusal:?}");
                    if let Some(signals) = &self.shell_signals {
                        let _ = signals.send(PaneSignal::PasteRefused(refusal));
                    }
                }
            }
            return;
        }
        // The primary selection has no GPUI API: one data-control
        // read off the UI thread, then the same engine paste.
        println!("dropterm: pane action PastePrimary — data-control read");
        let terminal = Arc::clone(&self.terminal);
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { clipboard::read_primary_text_capped() })
                .await;
            match read {
                clipboard::TextRead::Text(text) => {
                    let _ = this.update(cx, |pane, cx| {
                        println!(
                            "dropterm: paste primary — {} bytes through the engine's paste",
                            text.len()
                        );
                        let mut engine = terminal.lock().expect("engine lock");
                        engine.paste(&text);
                        engine.needs_update = true;
                        drop(engine);
                        if let Some(signals) = &pane.shell_signals {
                            let _ = signals.send(PaneSignal::TextPasted);
                        }
                        cx.notify();
                    });
                }
                clipboard::TextRead::None => {
                    let _ = this.update(cx, |pane, cx| pane.paste_clipboard_image(true, cx));
                }
                clipboard::TextRead::Refused(refusal) => {
                    println!("dropterm: paste primary text refused — {refusal:?}");
                    let _ = this.update(cx, |pane, _| {
                        if let Some(signals) = &pane.shell_signals {
                            let _ = signals.send(PaneSignal::PasteRefused(refusal));
                        }
                    });
                }
            }
        })
        .detach();
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
            TermAction::Paste => self.paste_text_action(false, cx),
            TermAction::PastePrimary => self.paste_text_action(true, cx),
            TermAction::SelectAll => {
                println!("dropterm: pane action SelectAll (every row selected)");
                let mut engine = self.terminal.lock().expect("engine lock");
                engine.select_all();
                engine.needs_update = true;
            }
            TermAction::ClearScrollback => {
                println!("dropterm: pane action ClearScrollback (history dropped)");
                let mut engine = self.terminal.lock().expect("engine lock");
                engine.term.lock().grid_mut().clear_history();
                engine.needs_update = true;
            }
            TermAction::ScrollToBottom => {
                println!("dropterm: pane action ScrollToBottom (viewport at the bottom)");
                let mut engine = self.terminal.lock().expect("engine lock");
                engine.scroll(dropterm::core::terminal::TerminalScroll::Bottom);
                engine.needs_update = true;
            }
            TermAction::ZoomIn | TermAction::ZoomOut | TermAction::ZoomReset => {
                let mut terminal = self.terminal.lock().expect("engine lock");
                let adj = match action {
                    TermAction::ZoomIn => terminal.zoom_adj().saturating_add(1),
                    TermAction::ZoomOut => terminal.zoom_adj().saturating_sub(1),
                    _ => 0,
                };
                println!("dropterm: pane action {action:?} (zoom_adj {adj})");
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
                        println!("dropterm: pane hide action -> the shell's funnel");
                        let _ = signals.send(PaneSignal::Hide);
                    }
                    None => {
                        println!("dropterm: pane hide action (no shell in the walk harness)");
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
                println!("dropterm: pane show-completions action (the routing owns it)");
            }
            // The shell family never reaches the pane's own dispatch
            // (on_key routes every shell verb to the handoff first);
            // a future caller that lands here gets a log, never a
            // panic.
            action if action.is_shell_verb() => {
                println!("dropterm: pane action {action:?} (a shell verb off its keyboard route)");
            }
            // Exhaustiveness: every pane-owned variant is matched
            // above; the shell family is the guard arm.
            _ => {}
        }
        cx.notify();
    }

    /// The shell's adoption (the keyboard's shell verbs): the weak
    /// handle lands here once the shell entity exists — the boot
    /// loop over the restored fleet, and every spawn path after it.
    #[cfg(target_os = "linux")]
    pub(crate) fn adopt_shell(&mut self, shell: gpui::WeakEntity<crate::app_shell::Shell>) {
        self.shell = Some(shell);
    }

    /// One shell verb off the keyboard (the menus' own funnels): the
    /// pane matched the chord, the shell executes it. The dispatch
    /// runs *outside* this key handler's own pane lease — the verb
    /// handlers read and update panes freely (the new tab's active-
    /// cwd inherit, the close gate's running check, the duplicate's
    /// source read), and every one of those reads would panic
    /// "cannot read `TerminalPane` while it is already being updated"
    /// under the synchronous nesting, because this pane is on the
    /// lease stack for the whole dispatch (the `NewTab` walk's panic,
    /// the Ctrl+Shift+I drawer's sibling). `Window::defer` parks the
    /// verb to the effect flush — App-level with the key handler's
    /// own window, exactly the standing the menu and gear callbacks
    /// already hold, so the modal (close), the rename input, and the
    /// settings focus hand-off all work identically. The walk
    /// harness (no shell) logs, the Hide signal's own precedent.
    #[cfg_attr(
        not(target_os = "linux"),
        expect(
            clippy::unused_self,
            reason = "the mac arm keeps the one signature the callers share; it has no shell to act on (macos-build mb-2)"
        )
    )]
    fn dispatch_shell_verb(
        &mut self,
        action: TermAction,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        // The shell half is Linux's (mb-2): everywhere else no shell
        // can have adopted the pane, so the verb takes the walk
        // harness's own honest arm — logged, never dispatched.
        #[cfg(target_os = "linux")]
        {
            let Some(shell) = self.shell.as_ref().and_then(gpui::WeakEntity::upgrade) else {
                println!("dropterm: pane shell verb {action:?} (no shell in the walk harness)");
                return;
            };
            let pane = self.id;
            window.defer(cx, move |window, cx| {
                shell.update(cx, |shell, cx| shell.shell_verb(action, pane, window, cx));
            });
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (window, cx);
            println!("dropterm: pane shell verb {action:?} (no shell on this platform)");
        }
    }

    /// One key press through the translation and its outcome.
    fn on_key(
        &mut self,
        keystroke: &gpui::Keystroke,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        // The walk affordance: the exact keystroke shape the platform
        // delivered (key, modifiers, key_char) — rig/VM input walks
        // set `DROPTERM_TRACE_KEYS=1` to see what actually arrives.
        if std::env::var_os("DROPTERM_TRACE_KEYS").is_some() {
            println!(
                "dropterm: key {:?} modifiers ctrl={} shift={} alt={} super={} key_char={:?}",
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
        // An owned chord never leaves the pane: the Kit's debug
        // bindings sit behind gpui's keymap phase — the inspector on
        // Ctrl+Shift+I in every debug build, colliding with the
        // Settings chord — so the app's own chords stop propagation
        // the moment they match (the shell verbs still route through
        // the handoff below, which runs before the return). Chords
        // the table does not own keep falling through; the inspector
        // stays reachable in dev on any unbound chord.
        let matched = input::matched_action(&self.binds, keystroke);
        if matched.is_some() {
            cx.stop_propagation();
        }
        // The completion menu's routing (suggest-menu spec), before
        // the keybind swallow and every mapping — a showing menu
        // outranks them all; the soft trigger chord tries the open
        // here, its decline falling through with the chord's own
        // shell bytes still to send.
        let trigger_chord = matches!(matched, Some(TermAction::ShowCompletions));
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
                    println!("dropterm: pane accept {kind:?} — the showing suffix lands");
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
            KeyInput::Action(action) => {
                if action.is_shell_verb() {
                    self.dispatch_shell_verb(action, window, cx);
                } else {
                    self.perform_action(action, cx);
                }
            }
            KeyInput::Scroll(scroll) => {
                println!("dropterm: pane scroll {scroll:?} (Shift+PageUp/Down/Home/End)");
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
                "dropterm: press {neutral:?} at ({:.0},{:.0}) shift={}",
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
                    "dropterm: selection finalized ({} bytes; Copy takes them)",
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
            println!("dropterm: wheel delta {kind} y={y}");
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
        let scroll = dropterm::core::terminal::TerminalScroll::Delta(-lines);
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
        let frame = div()
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
                move |event: &gpui::KeyDownEvent, window, cx| {
                    pane.update(cx, |pane, cx| {
                        pane.on_key(&event.keystroke, window, cx);
                    });
                }
            });
        // The Kit's inspector seat: the kit binds `ctrl-shift-i`
        // globally in every debug build, and gpui's dispatch runs
        // matched keymap actions ahead of every key listener — the
        // stop-propagation-at-the-listener guard below is a phase
        // too late for the one chord the keymap claims (the second
        // hardware walk's finding: the inspector still opened). The
        // pane answers the kit's own action at the element layer,
        // which runs before the kit's app-level handler; element
        // handlers stop propagation by default, and the not-ours arm
        // propagates the seat back.
        #[cfg(debug_assertions)]
        let frame = frame.on_action({
            let pane = pane.clone();
            move |_: &gpui_component::ToggleInspector,
                  window: &mut gpui::Window,
                  cx: &mut gpui::App| {
                pane.update(cx, |pane, cx| pane.on_kit_inspector(window, cx));
            }
        });
        frame.child(
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

/// One pane's paint: resize to the granted bounds, run the pin
/// funnel's settle close, drain the update flag, then walk the
/// visible grid through the projection and the shaper. One function
/// reads the whole frame together — the grid walk, the selection
/// overlay, and the evidence rows share the one lock and the one
/// geometry; splitting them re-derives the lock order per piece.
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
    let granted = resize_to_bounds(&mut terminal, bounds);
    // The pin funnel's settle close (the fresh-start trace's shape):
    // this frame is where a laid-out surface's granted size lands —
    // the draw resized before any granted-size echo could arrive, and
    // `resize` no-ops on unchanged sizes, so waiting for a changed
    // resize here waits forever. The close at the grid already held
    // freezes the record and arms the fresh-prompt latch; the update
    // it owes runs in this frame's own drain, so the shell's first
    // prompt adopts and the ghost and the Ctrl+Space menu answer on
    // a fresh tab. A zero-width frame makes no request and settles
    // nothing; a settled record never re-snapshots.
    if granted && terminal.settle_spawn_grid_now() {
        terminal.needs_update = true;
    }

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
                "dropterm: pane grid {cols}x{rows} at scroll offset {display_offset} \
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
                        "dropterm: pane image {} placed (first covered cell col {col} row {row}, {} layer)",
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
        // The caret's own quad: the block is the cell's rect in the
        // cell's resolved foreground, painted under the glyphs. The
        // projected run carries the same rect as its background
        // color, but that rect rides the shaper's glyph-boundary
        // flush — the one link in the caret chain no walk ever
        // banked (the hardware walks' "no cursor" against a paint
        // that traced green up to the run); the quad's geometry is
        // the cell's own, shaper-free, so the block no longer
        // depends on it. The run keeps the inverted glyph — the
        // cut-out when the cursor sits on a character.
        if let Some(col) = caret_col
            && let Some(cell) = cells.get(col)
        {
            let block = grid::hsla(grid::resolve_color(&skin.colors, cell.fg));
            #[expect(
                clippy::cast_precision_loss,
                reason = "a grid column is far below 2^24: exact in f32"
            )]
            let origin = point(
                bounds.origin.x + geometry.cell_width * col as f32,
                row_origin_y,
            );
            window.paint_quad(fill(
                gpui::Bounds {
                    origin,
                    size: gpui::Size::new(geometry.cell_width, geometry.cell_height),
                },
                block,
            ));
        }
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
                println!("dropterm: pane row{row} now {text:?}");
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
                "dropterm: caret at col {} row {} (block, cell quad)",
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
                "dropterm: pane ghost now {:?} ({} cells past the caret)",
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
            println!("dropterm: pane ghost cleared");
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
                    "dropterm: pane menu now {} rows (selected {}) — {:?}",
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
/// the transition logs the walk's resize evidence row. The decision
/// is the pure `resize_px` (the testable seam); this wrapper reads
/// the engine's own cell metrics and applies the request. `true` is
/// a real request — the frame the granted size landed on; `false` is
/// the zero-width pre-configure frame, which touches nothing.
fn resize_to_bounds(terminal: &mut Terminal, bounds: gpui::Bounds<Pixels>) -> bool {
    let size = terminal.size();
    let Some(px_size) = resize_px(bounds, size.cell_width, size.cell_height) else {
        return false;
    };
    if (size.width, size.height) != px_size {
        terminal.resize(px_size.0, px_size.1);
        #[expect(
            clippy::cast_precision_loss,
            reason = "surface px clamped above one cell and far below 2^24: exact in f32"
        )]
        let (w, h) = (px_size.0 as f32, px_size.1 as f32);
        let grid = grid::grid_of(w, h, size.cell_width, size.cell_height);
        println!("dropterm: pane resize {px_size:?} -> grid {grid:?} (Term::resize + SIGWINCH)");
    }
    true
}

/// The frame's px request: `None` when the frame must not touch the
/// engine. The pixel request is clamped to the engine's own one-cell
/// floor *before* the comparison, so a sub-cell window converges
/// (the engine's internal clamp would otherwise store one-cell
/// dimensions against a sub-cell request every frame — the first
/// walk's oscillation). A zero-width grant is never a request: a
/// freshly opened window (the surface, the keyboard-hold swap) maps
/// its first frame before the compositor's configure lands — width
/// 0, the height carried over — and resizing then drives the shell
/// through a one-column grid (SIGWINCH); bash repaints its prompt
/// wrapped, the fragments survive the configure frame's resize-back,
/// and every hide/show toggle piles another layer onto the grid (the
/// toggle corruption walk). The configure frame carries the real
/// width and resizes once.
fn resize_px(
    bounds: gpui::Bounds<Pixels>,
    cell_width: f32,
    cell_height: f32,
) -> Option<(u32, u32)> {
    let width = bounds.size.width.as_f32().max(0.);
    if width <= 0. {
        return None;
    }
    let height = bounds.size.height.as_f32().max(0.);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "cell and surface px are positive and far below u32::MAX; ceil keeps a partial cell whole"
    )]
    let px_size = (
        width.max(cell_width.ceil()).round() as u32,
        height.max(cell_height.ceil()).round() as u32,
    );
    Some(px_size)
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
    // The background pass. `ShapedLine::paint` draws glyphs,
    // underlines and strikes only — a run's `background_color`
    // reaches pixels solely through the separate `paint_background`.
    // A row holding any explicit background (an INVERSE cell's
    // swapped pair, bash's whole bracketed-paste preview among them)
    // needs that pass under its glyphs, or its glyphs paint in the
    // swapped foreground — the pane background — over the pane
    // background: text and caret present, line reading as blank
    // until the program redraws the span plain. Rows without a
    // background skip the pass and pay nothing.
    if runs.iter().any(|run| run.bg.is_some()) {
        let _ = shaped.paint_background(
            origin,
            geometry.cell_height,
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }
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
/// preference's own dark/light; `System` follows the real OS mode
/// the system-mode cell holds (theme-defaults td-2).
pub(crate) fn boot_colors(config: &dropterm::core::config::DropTermConfig) -> Colors {
    let is_dark = dropterm::core::theme_glue::effective_is_dark(
        config.theme_preference,
        dropterm::core::system_mode::is_dark(),
    );
    dropterm::core::theme_glue::effective_colors(config, None, is_dark)
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
    custom_binds: &std::collections::BTreeMap<String, dropterm::core::keybinds::Binding>,
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
        command: &dropterm::core::spawn::SpawnCommand,
        cwd: Option<std::path::PathBuf>,
        config: &dropterm::core::config::DropTermConfig,
    ) -> Self {
        let parts = dropterm::core::spawn::spawn_parts(command, config);
        let profile = parts
            .profile_id
            .as_deref()
            .and_then(|id| dropterm::core::profile::find(&config.profiles, id));
        let colors = match profile {
            // The boot-time mode resolution mirrors `boot_colors`'s
            // (the one-chain pin): `System` follows the real OS mode
            // the system-mode cell holds.
            Some(profile) => {
                let is_dark = dropterm::core::theme_glue::effective_is_dark(
                    config.theme_preference,
                    dropterm::core::system_mode::is_dark(),
                );
                dropterm::core::theme_glue::effective_colors(config, Some(profile), is_dark)
            }
            None => boot_colors(config),
        };
        let appearance = PaneAppearance {
            colors,
            font_size: dropterm::core::profile::effective_font_size(profile, config.font_size),
            scrollback: dropterm::core::profile::effective_scrollback(
                profile,
                config.scrollback_lines,
            ),
        };
        Self {
            program: parts.program,
            args: parts.trailing,
            cwd,
            env: parts.env,
            spawned_as: dropterm::core::spawn::spawned_as_program(command, config),
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
    config: &dropterm::core::config::DropTermConfig,
    request: PaneRequest,
    specs_table: Option<&Arc<[dropterm::core::terminal::suggest::SpecCommand]>>,
    id: TabId,
    shell_signals: Option<std::sync::mpsc::Sender<PaneSignal>>,
) -> Option<(Arc<Mutex<Terminal>>, Entity<TerminalPane>)> {
    let colors = request.appearance.colors;
    // The configured family rides the boot: the host resolves
    // `font_name` against the held fonts (the fallback walk owns an
    // empty or unheld name), so a pane spawns in the picked face.
    let configured_family = config.font_name.trim();
    let host = crate::terminal::host::GpuiHost::new(
        cx,
        (!configured_family.is_empty()).then_some(configured_family),
    );
    let font = host.font();
    println!(
        "dropterm: pane pty spawning {} {:?} (spawned as {})",
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
    .map_err(|err| eprintln!("dropterm: pane pty spawn failed: {err:?}"))
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

    // --- the background alpha (the live-opacity fix) ---

    #[test]
    fn opacity_alpha_maps_the_band_and_clamps() {
        // The one math the spawn's bake and the tick's retune share:
        // 100 saturates at full alpha, 0 goes fully transparent, and
        // an over-band value clamps — never overshoots — so a
        // mid-session flip renders exactly what a fresh tab would.
        assert_eq!(opacity_alpha(100), 1.0);
        assert_eq!(opacity_alpha(0), 0.0);
        assert_eq!(opacity_alpha(50), 0.5);
        assert_eq!(opacity_alpha(255), 1.0);
    }

    // --- the pane request over core's spawn resolution (py-5) ---

    fn request_config() -> dropterm::core::config::DropTermConfig {
        dropterm::core::config::DropTermConfig {
            shell: Some("/bin/sh".to_string()),
            ..dropterm::core::config::DropTermConfig::default()
        }
    }

    fn profile_command(name: &str, command: &str) -> dropterm::core::spawn::SpawnCommand {
        dropterm::core::spawn::SpawnCommand::Profile {
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
        config.profiles = vec![dropterm::core::profile::Profile {
            name: "top".to_string(),
            command: "htop".to_string(),
            cwd: None,
            color_scheme: None,
            env: Some(env.clone()),
            font_size: Some(11),
            scrollback_lines: Some(500),
        }];
        let request = PaneRequest::for_command(
            &dropterm::core::spawn::SpawnCommand::Profile {
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
        config.profiles = vec![dropterm::core::profile::Profile {
            name: "top".to_string(),
            command: "htop".to_string(),
            cwd: None,
            color_scheme: None,
            env: None,
            font_size: Some(11),
            scrollback_lines: Some(500),
        }];
        let request = PaneRequest::for_command(
            &dropterm::core::spawn::SpawnCommand::Shell("/bin/sh".to_string()),
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
    fn boot_colors_and_command_requests_follow_the_system_mode_cell() {
        // theme-defaults td-2: both pane-side resolution sites read
        // the OS-mode cell the portal watch feeds — no literal dark
        // remains — so a tab opened under a light OS carries light
        // colors.
        use alacritty_terminal::vte::ansi::{NamedColor, Rgb};
        let config = request_config();
        let boot = dropterm::core::system_mode::with_system_mode(false, || boot_colors(&config));
        assert_eq!(
            boot[NamedColor::Background],
            Some(Rgb {
                r: 0xFF,
                g: 0xFF,
                b: 0xFF
            })
        );
        let request = dropterm::core::system_mode::with_system_mode(false, || {
            PaneRequest::for_command(
                &dropterm::core::spawn::SpawnCommand::Shell("/bin/sh".to_string()),
                None,
                &config,
            )
        });
        assert_eq!(
            request.appearance.colors[NamedColor::Background],
            Some(Rgb {
                r: 0xFF,
                g: 0xFF,
                b: 0xFF
            })
        );
    }

    #[test]
    fn the_blank_line_profile_degrades_with_no_identity() {
        // The one degradation left (a hand-edited blank command line):
        // the plain shell, no id, no env — never half a profile's
        // identity — and the global appearance.
        let mut config = request_config();
        config.profiles = vec![dropterm::core::profile::Profile {
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

    // --- the toggle corruption's guard: the zero-width frame ---

    fn px_bounds(width: f32, height: f32) -> gpui::Bounds<Pixels> {
        gpui::Bounds {
            origin: gpui::Point::new(px(0.), px(0.)),
            size: gpui::Size::new(px(width), px(height)),
        }
    }

    #[test]
    fn the_unconfigured_zero_width_frame_makes_no_resize_request() {
        // Every freshly opened window (the surface, the keyboard-hold
        // swap) maps its first frame before the compositor's
        // configure lands: a zero-width grant with the height carried
        // over. Resizing the engine there drives the shell through a
        // one-column grid (SIGWINCH) — bash repaints its prompt
        // wrapped, the fragments survive the configure frame's
        // resize-back, and every hide/show toggle piles another
        // layer onto the grid (the prompt smeared across rows). The
        // guard: a zero-width grant makes no request, so the
        // configure frame resizes once. The sub-cell floor and the
        // real request keep their old answers.
        assert_eq!(resize_px(px_bounds(0., 749.), 8.4, 20.), None);
        assert_eq!(
            resize_px(px_bounds(9., 679.), 8.4, 20.),
            Some((9, 679)),
            "the sub-cell width still floors to one cell (the first walk's oscillation)"
        );
        assert_eq!(
            resize_px(px_bounds(2558., 679.), 8.4, 20.),
            Some((2558, 679))
        );
    }

    // --- the live-font fix: the boot resolution and the tick's
    // per-pane apply ---

    // Linux-only (macos-build mb-2): a live PTY reader thread cannot
    // satisfy the gpui test scheduler's thread-affinity gate at
    // teardown — the same limitation the pasted_paths ignore below
    // documents, but deterministic on the mac target (every
    // assertion runs, then the harness aborts at drop). Runs green
    // on Linux CI.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    fn a_pane_boots_in_the_configured_family_and_a_live_edit_lands(cx: &mut gpui::TestAppContext) {
        // The font-name contract, end to end at the pane: an unheld
        // configured name falls back to the candidate walk (the
        // resolution rule host.rs pins — an unheld family silently
        // fails to rasterize), the fallback name itself lands
        // verbatim (a held name wins), and `set_font_family` — the
        // tick's fleet pass — swaps the skin's paint font to the
        // resolved family (the engine's own re-measure rides the
        // same call).
        cx.update(gpui_component::init);
        let unheld = "Dropterm Unheld Test Face";
        let fallback =
            cx.update(|cx| crate::terminal::host::resolve_family(cx.text_system(), Some(unheld)));
        assert_eq!(
            fallback,
            cx.update(|cx| crate::terminal::host::resolve_family(cx.text_system(), None)),
            "an unheld name falls back to the candidate walk"
        );
        assert_eq!(
            cx.update(|cx| crate::terminal::host::resolve_family(
                cx.text_system(),
                Some(&fallback)
            )),
            fallback,
            "a held name lands verbatim"
        );

        // Boot with the unheld name: the pane lands in the fallback
        // face, not the configured string.
        let config = dropterm::core::config::DropTermConfig {
            font_name: unheld.to_string(),
            ..request_config()
        };
        let (_terminal, pane) = cx
            .update(|cx| {
                let request = PaneRequest::for_command(
                    &dropterm::core::spawn::SpawnCommand::Shell("/bin/sh".to_string()),
                    None,
                    &config,
                );
                boot_pane(cx, &config, request, None, TabId(0), None)
            })
            .expect("boot the pane");
        let booted = cx.update(|cx| pane.read(cx).skin.font.family.to_string());
        assert_eq!(
            booted, fallback,
            "an unheld configured name boots the fallback walk's pick"
        );

        // The live edit to the (held) fallback family lands on the
        // skin verbatim.
        cx.update(|cx| pane.update(cx, |pane, cx| pane.set_font_family(&fallback, cx)));
        let edited = cx.update(|cx| pane.read(cx).skin.font.family.to_string());
        assert_eq!(edited, fallback, "the live edit lands the resolved family");
    }

    // --- the covering report's third telling: a pasted path hides
    // until a backspace redraws the line ---

    #[gpui::test]
    #[ignore = "a live PTY reader thread cannot satisfy the gpui test                 scheduler's thread-affinity gate at teardown: every                 assertion runs and prints its evidence, then the harness                 panics at drop — read the stdout, not the exit code                 (just test pasted_paths -- --ignored)"]
    #[expect(
        clippy::too_many_lines,
        reason = "one pane's whole lifecycle through its own paint clock — the boot, the prompt wait, both paste shapes, the backspace redraw, and the shell-exit teardown are one scene"
    )]
    fn pasted_paths_read_on_the_painted_prompt_line(cx: &mut gpui::TestAppContext) {
        // The hidden-paste report's headless pin, the text half. The
        // boot defect was real — the app never set `TERM` for its
        // ptys, so a desktop-launched drop-down handed its shells a
        // GUI session's empty `TERM` and readline degraded (bracketed
        // paste off, conservative redraws) — but it was only half the
        // veil: with a real TERM the pasted line still vanished,
        // because bash's bracketed-paste preview draws wholly in SGR-7
        // reverse video and the pane's row painter dropped run
        // backgrounds (the pixels half lives in
        // `reverse_video_rows_paint_their_highlight_band`). This test
        // pins the text half: under the environment the fixed boot
        // guarantees — a real TERM — bash's bracketed paste rides and
        // the preview's characters paint whole across wrapped rows.
        // The walk rows own the boot-side half (a pane's `$TERM`
        // answers `alacritty` or `xterm-256color`).
        cx.update(gpui_component::init);
        let config = request_config();
        let (terminal, pane) = cx
            .update(|cx| {
                // The PI test's own pty shape, hand-resolved past
                // `for_command` (whose wrap exists for the real app's
                // command-then-shell seats), carrying the TERM the
                // fixed boot guarantees: an interactive bash with a
                // known prompt, straight onto the pane.
                let request = PaneRequest {
                    program: "/bin/bash".to_string(),
                    args: vec!["--norc".to_string(), "-i".to_string()],
                    cwd: None,
                    env: std::collections::HashMap::from([
                        ("PS1".to_string(), "P$ ".to_string()),
                        ("TERM".to_string(), "xterm-256color".to_string()),
                    ]),
                    spawned_as: "bash".to_string(),
                    profile_id: None,
                    appearance: PaneAppearance {
                        colors: boot_colors(&config),
                        font_size: config.font_size,
                        scrollback: config.scrollback_lines,
                    },
                };
                boot_pane(cx, &config, request, None, TabId(0), None)
            })
            .expect("boot the pane");
        let pane_read = pane.clone();
        let handle = cx
            .update(|cx| {
                cx.open_window(
                    gpui::WindowOptions {
                        window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds {
                            origin: gpui::Point::default(),
                            size: gpui::size(px(900.), px(600.)),
                        })),
                        ..Default::default()
                    },
                    |_, _| pane,
                )
            })
            .expect("open the pane's window");
        let cx = &mut gpui::VisualTestContext::from_window(handle.into(), cx);

        // The paint clock: refresh-then-draw (a bare draw may skip
        // render/paint — the rename-slot pin's precedent), parked so
        // the pane's event pump lands between frames. The assertion
        // channel is the WALK LOG — what the paint actually shaped,
        // not the engine's rows (the PTY reader updates the grid
        // ahead of any paint, so engine truth can race ahead of
        // pixels; the walk log cannot).
        let frame = |cx: &mut gpui::VisualTestContext| {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        };
        let painted = |cx: &mut gpui::VisualTestContext, needle: &str| {
            cx.update(|_, cx| {
                pane_read
                    .read(cx)
                    .walk_log
                    .lock()
                    .expect("walk log")
                    .rows
                    .iter()
                    .any(|row| row.contains(needle))
            })
        };
        // The wrapped shape: a row that fills the grid's whole width
        // continues at the next row's first cell, so the trimmed rows
        // joined hold a wrapped line's full text.
        let painted_wrapped = |cx: &mut gpui::VisualTestContext, needle: &str| {
            cx.update(|_, cx| {
                let joined: String = pane_read
                    .read(cx)
                    .walk_log
                    .lock()
                    .expect("walk log")
                    .rows
                    .iter()
                    .map(|row| row.trim_end())
                    .collect::<Vec<_>>()
                    .join("");
                joined.contains(needle)
            })
        };
        let wait_painted = |cx: &mut gpui::VisualTestContext, needle: &str| -> Vec<String> {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                frame(cx);
                if painted(cx, needle) {
                    return Vec::new();
                }
                if std::time::Instant::now() >= deadline {
                    return cx.update(|_, cx| {
                        pane_read
                            .read(cx)
                            .walk_log
                            .lock()
                            .expect("walk log")
                            .rows
                            .clone()
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        };
        let missed = wait_painted(cx, "P$");
        assert!(
            missed.is_empty(),
            "the first prompt never painted: {missed:?}"
        );

        // The paste, both shapes. Fitting: the real paste-cache
        // path's own length. Over-width: past the grid's width — the
        // bracketed-paste preview paints the WHOLE text (wrapped over
        // rows), never a scrolled window that hides its head.
        let fits = "/home/jpt/.cache/dropterm/paste/paste-20261119-101010-1.png";
        terminal.lock().expect("engine lock").paste(fits);
        let missed = wait_painted(cx, fits);
        assert!(
            missed.is_empty(),
            "the fitting pasted path never painted: {missed:?}"
        );

        // A clean line for the over-width shape (^U), then the paste.
        let engine = terminal.lock().expect("engine lock");
        engine.input_scroll(&b"\x15"[..]);
        drop(engine);
        let missed = wait_painted(cx, "P$ ");
        assert!(
            missed.is_empty(),
            "the cleared prompt never painted: {missed:?}"
        );
        let long = format!("/tmp/paste-20261119-{}.png", "x".repeat(160));
        terminal.lock().expect("engine lock").paste(&long);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            frame(cx);
            if painted_wrapped(cx, &long) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the over-width pasted path must paint whole (the bracketed-paste \
                 preview), not hide behind a scrolled window: {:?}",
                cx.update(|_, cx| {
                    pane_read
                        .read(cx)
                        .walk_log
                        .lock()
                        .expect("walk log")
                        .rows
                        .clone()
                })
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            missed.is_empty(),
            "the over-width pasted path must paint whole (the bracketed-paste              preview), not hide behind a scrolled window: {missed:?}"
        );

        // The report's own reveal, still whole after it: one
        // backspace redraws the line (a char shorter at the tail)
        // and the path stays on the painted rows.
        let engine = terminal.lock().expect("engine lock");
        engine.input_scroll(&b"\x7f"[..]);
        drop(engine);
        let shortened = format!("/tmp/paste-20261119-{}.pn", "x".repeat(160));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            frame(cx);
            if painted_wrapped(cx, &shortened) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the backspace redraw must keep the pasted path painted: {:?}",
                cx.update(|_, cx| {
                    pane_read
                        .read(cx)
                        .walk_log
                        .lock()
                        .expect("walk log")
                        .rows
                        .clone()
                })
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Teardown: the gpui test scheduler demands determinism, and
        // the pane's PTY reader thread keeps scheduling wakeups while
        // its shell lives. Exit bash and let the reader drain to EOF
        // before the test ends.
        let engine = terminal.lock().expect("engine lock");
        engine.input_scroll(&b"\x15"[..]); // ^U: the line is the pasted path; exit needs it clear
        engine.input_scroll("exit\r".as_bytes());
        drop(engine);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while {
            cx.run_until_parked();
            terminal
                .lock()
                .expect("engine lock")
                .foreground_child_running()
        } {
            assert!(
                std::time::Instant::now() < deadline,
                "the pane's shell never exited for teardown"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        // The reader's own endgame (its EIO wakeup and final
        // bookkeeping) lands after the child's reap; settle until a
        // full quiet second passes so the scheduler never sees the
        // reader thread again.
        let quiet_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let quiet = std::time::Instant::now();
        while std::time::Instant::now() < quiet_deadline {
            cx.run_until_parked();
            if quiet.elapsed() >= std::time::Duration::from_secs(1) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    // --- the veil's pixels: a reverse-video span paints its block ---

    // Linux-only (macos-build mb-2): boots a live /bin/sh pane — the
    // pasted_paths ignore's thread-affinity limitation, deterministic
    // on the mac target (see the pane-boot twin above).
    #[cfg(target_os = "linux")]
    #[gpui::test]
    fn reverse_video_rows_paint_their_highlight_band(cx: &mut gpui::TestAppContext) {
        // The hidden-paste veil, pixels side. bash's bracketed-paste
        // preview draws wholly in SGR-7 reverse video (a pty probe
        // holds the bytes: `\x1b[7m`…text…`\x1b[27m`, and the
        // backspace redraw plain — the report's own reveal), so the
        // pane's row painter owes every run background a pass under
        // its glyphs: gpui's `ShapedLine::paint` paints glyphs,
        // underlines and strikes only, and without the pane's own
        // `paint_background` call an INVERSE cell's glyphs paint in
        // the swapped foreground — the pane background — over the
        // pane background. Text and caret present, line reading as
        // blank. This pins real rasterized pixels: the reversed span
        // must hold a solid highlight block as wide as its three
        // cells — the block caret is one cell and cannot counterfeit
        // three.
        cx.update(gpui_component::init);
        let config = request_config();
        let (terminal, pane) = cx
            .update(|cx| {
                let request = PaneRequest::for_command(
                    &dropterm::core::spawn::SpawnCommand::Shell("/bin/sh".to_string()),
                    None,
                    &config,
                );
                boot_pane(cx, &config, request, None, TabId(0), None)
            })
            .expect("boot the pane");
        let pane_read = pane.clone();
        let handle = cx
            .update(|cx| {
                cx.open_window(
                    gpui::WindowOptions {
                        window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds {
                            origin: gpui::Point::default(),
                            size: gpui::size(px(900.), px(600.)),
                        })),
                        ..Default::default()
                    },
                    |_, _| pane,
                )
            })
            .expect("open the pane's window");
        let cx = &mut gpui::VisualTestContext::from_window(handle.into(), cx);

        // The paint clock: refresh-then-draw (a bare draw may skip
        // render/paint), parked so the pane's event pump lands
        // between frames — the ignored walk test's own precedent.
        let frame = |cx: &mut gpui::VisualTestContext| {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        };
        // Feed the pane's shell a printf whose output is Q, a
        // three-cell reverse span, E — output, not echoed input: the
        // bytes pass back through the pty into the grid.
        terminal
            .lock()
            .expect("engine lock")
            .input_scroll("printf 'Q\u{1b}[7mWWW\u{1b}[27mE'\r".as_bytes());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            frame(cx);
            let painted = cx.update(|_, cx| {
                pane_read
                    .read(cx)
                    .walk_log
                    .lock()
                    .expect("walk log")
                    .rows
                    .iter()
                    .any(|row| row.contains("QWWWE"))
            });
            if painted {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the reverse span never painted: {:?}",
                cx.update(|_, cx| {
                    pane_read
                        .read(cx)
                        .walk_log
                        .lock()
                        .expect("walk log")
                        .rows
                        .clone()
                })
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // One more frame, then the scene itself: the rendered frame's
        // quads must hold the reversed span's background — a block
        // one cell tall and about three cells wide (the block caret
        // is a single cell and cannot counterfeit two more). The
        // raster path (`Window::render_to_image`) needs a headless
        // renderer this gpui-pre only configures on macOS, so the
        // scene walk — the tab-bar caret test's own channel — is the
        // pane's pixel truth on Linux. Quads ride scaled pixels; the
        // engine's cell metrics ride logical — one scale factor
        // reconciles them.
        frame(cx);
        let cell_w = terminal.lock().expect("engine lock").size().cell_width;
        let cell_h = terminal.lock().expect("engine lock").size().cell_height;
        let (block, quad_count) = cx.update(|window, _| {
            let scale = window.scale_factor();
            let quads = &window.rendered_scene().quads;
            (
                quads.iter().any(|quad| {
                    quad.bounds.size.width.as_f32() >= 1.8 * cell_w * scale
                        && (quad.bounds.size.height.as_f32() - cell_h * scale).abs() <= 2. * scale
                }),
                quads.len(),
            )
        });
        assert!(
            block,
            "the reverse span's background quad never painted ({quad_count} quads \
             in the frame, none one cell tall and ≥ two cells wide) — the pane \
             painted its glyphs but not the run's background, the hidden-paste \
             veil"
        );
    }
}
