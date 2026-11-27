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
use super::editor;
use super::grid::{self, StyledRun};
use super::input::{self, Chord, KeyInput, KeyModes, TermAction};
use super::selection::{self, ClickKind};
use super::suggest;
use dropterm::core::terminal::editor::{Key, Signal, Verdict};

gpui::actions!(dropterm_gpui, [TerminalTab, TerminalBackTab]);

/// The pane's Tab claim (key-delivery B3): the Kit Root registers a
/// global-path `"tab"`/`"shift-tab"` focus-traversal binding in its
/// own Root context (gpui-base `root.rs:44`), and a consumed binding
/// returns before any key listener runs — so a pane that claims
/// nothing never sees bare Tab, whatever the compositor delivered
/// (A1's ten silent presses). The pane is a text surface over a
/// shell: it answers by binding the same keystrokes in its own
/// deeper "Terminal" context and forwarding them into
/// [`TerminalPane::on_key`] — the exact path letters take, so the
/// suggest menu's Accept-first routing, the chord table, and every
/// other rule apply unchanged, while non-terminal surfaces (settings
/// inputs, dialogs) keep the Root's traversal.
pub(crate) fn bind_pane_keys(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("tab", TerminalTab, Some("Terminal")),
        gpui::KeyBinding::new("shift-tab", TerminalBackTab, Some("Terminal")),
    ]);
}

/// The keystroke a tab press arrives as when the claim forwards it:
/// the platform's own construction shape (key "tab", shift carried on
/// the modifiers, no `key_char` — a control character).
fn tab_keystroke(shift: bool) -> gpui::Keystroke {
    gpui::Keystroke {
        key: "tab".to_string(),
        modifiers: gpui::Modifiers {
            shift,
            ..Default::default()
        },
        key_char: None,
    }
}

/// The walk evidence rows' dedup state (the last logged grid and
/// caret cells — one row per change, not per frame).
#[derive(Default)]
pub(crate) struct WalkLog {
    grid: Option<(usize, usize)>,
    caret: Option<(usize, usize)>,
    /// The input holding the caret at the last logged frame (the
    /// seat evidence row's dedup).
    caret_seat: Option<editor::CaretSeat>,
    rows: Vec<String>,
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
    /// The Settings window's return request (settings-window SW2):
    /// the drop-down's show funnel runs at the tick — never through
    /// `dispatch`, so the toggle debounce cannot drop a return that
    /// follows a summon quickly. No production sender yet: SW3's
    /// close path queues it; the bin tests exercise the tick arm
    /// meanwhile.
    Show,
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
}

/// The pane view: one engine terminal, its resolved scheme, and the
/// base font the host measured the cell through.
pub(crate) struct TerminalPane {
    pub(crate) terminal: Arc<Mutex<Terminal>>,
    skin: PaneSkin,
    /// The rich-input editor's pane-side state (rich-input RI-4):
    /// the owned line's pure model, shared into the strip's paint
    /// closure. The history slice feeds from the engine's store with
    /// the parity replay (RI-5); until then Up/Down walk an empty
    /// lane and hold.
    editor: Arc<Mutex<editor::Editor>>,
    /// The shell's latest title report (stored now, consumed when
    /// the tab chrome slices land).
    title: Option<String>,
    walk_log: Arc<Mutex<WalkLog>>,
    /// The inline-image pixels-to-handles cache (gpui-parity Task 2):
    /// one `RenderImage` per engine image id, shared into the paint
    /// closure like the walk log.
    image_cache: Arc<Mutex<super::images::ImageCache>>,
    /// The pane's window-space origin the last frame painted — the
    /// pointer handlers' window→pane conversion input. GPUI's mouse
    /// events carry window coordinates (the status band's own
    /// conversion is the precedent), and the pane sits below the strip
    /// and the bands; zero until the first frame paints.
    viewport_origin: Arc<Mutex<gpui::Point<Pixels>>>,
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
    /// The Lua bus inlet (event-hooks spec), handed by the shell only
    /// when the plugins dir stood at boot. `None` — the no-plugins
    /// norm — means the pane's engine events never reach the channel
    /// at all: engine events arrive constantly, and nobody drains a
    /// channel whose host never spawned.
    lua_bus: Option<dropterm::core::lua_host::LuaHostHandle>,
    /// The owning shell, weak — the keyboard's shell verbs (tabs,
    /// settings, images, the band) update it from the key handler's
    /// own window, the menus' own callback shape. Weak by design:
    /// the shell's pane map owns this pane, so a strong handle would
    /// cycle; `None` until the shell adopts the pane (and forever in
    /// the standalone walk harness, where the verbs only log).
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
            editor: Arc::new(Mutex::new(editor::Editor::new(Vec::new()))),
            skin: PaneSkin {
                colors,
                opacity_alpha: opacity_alpha(opacity),
                font,
            },
            title: None,
            walk_log: Arc::new(Mutex::new(WalkLog::default())),
            image_cache: Arc::new(Mutex::new(super::images::ImageCache::default())),
            viewport_origin: Arc::new(Mutex::new(point(px(0.), px(0.)))),
            binds,
            options,
            focus,
            click: None,
            dragging: false,
            scroll_lines: 0.,
            reporter: MouseReporter::default(),
            reporting_gesture: false,
            shell_signals: None,
            lua_bus: None,
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
    #[expect(
        clippy::too_many_lines,
        reason = "the funnel: every engine arm beside its own gate, one match body — splitting it would scatter the funnel's one shape"
    )]
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
                // Change-gated onto the bus (event-hooks spec): title
                // reports fire on every prompt, and the tap speaks
                // only when the latched title actually moves — a
                // repeat of the standing report forwards nothing.
                if self.title.as_deref() != Some(title.as_str()) {
                    forward_to_lua(
                        self.lua_bus.as_ref(),
                        self.id,
                        "title",
                        serde_json::json!({ "title": title.as_str() }),
                    );
                }
                self.title = Some(title);
            }
            TermEvent::ResetTitle => {
                // A reset forwards only when one was set (the same
                // change gate's clear leg).
                if self.title.is_some() {
                    forward_to_lua(
                        self.lua_bus.as_ref(),
                        self.id,
                        "title-reset",
                        serde_json::json!({}),
                    );
                }
                self.title = None;
            }
            TermEvent::Bell => {
                // The marker latches here (desktop notifications
                // arrive with the chrome slices).
                self.attention.note_bell();
                forward_to_lua(
                    self.lua_bus.as_ref(),
                    self.id,
                    "bell",
                    serde_json::json!({}),
                );
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
            // The zone marks' engine notes (shell-integration spec,
            // SI-4): engine only, no lua-bus forward — event-hooks'
            // vocabulary is not this module's.
            TermEvent::PromptStart => {
                self.terminal
                    .lock()
                    .expect("engine lock")
                    .note_prompt_start();
            }
            TermEvent::CommandStart => {
                self.terminal
                    .lock()
                    .expect("engine lock")
                    .note_command_start();
            }
            TermEvent::PromptExit(code) => {
                self.terminal
                    .lock()
                    .expect("engine lock")
                    .note_prompt_exit(code);
                // OSC 133;D onto the bus (event-hooks spec): the
                // command-finished report — the fork's arm carries
                // its code unconditionally (`PromptExit(u32)`,
                // verified against the carried fork at plan).
                forward_to_lua(
                    self.lua_bus.as_ref(),
                    self.id,
                    "child-exit",
                    serde_json::json!({ "code": code }),
                );
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
            TermEvent::Exit => {
                forward_to_lua(
                    self.lua_bus.as_ref(),
                    self.id,
                    "exit",
                    serde_json::json!({}),
                );
                self.signal_exit();
            }
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

/// Forward one engine event to the Lua bus, pane side (event-hooks
/// spec). Fire-and-forget into the host's command channel — the send
/// is the work item, the lazy contract — gated on plugins-dir
/// presence at the shell's handoff, so a no-plugins session forwards
/// nothing and the channel stays empty: engine events arrive
/// constantly, and nobody drains a channel whose host never spawned.
/// The payload gains the pane's `tab` id, the minimal correlation key
/// every event carries.
fn forward_to_lua(
    bus: Option<&dropterm::core::lua_host::LuaHostHandle>,
    tab: TabId,
    event: &str,
    payload: serde_json::Value,
) {
    let Some(bus) = bus else { return };
    let mut payload = payload;
    if let Some(fields) = payload.as_object_mut() {
        fields.insert("tab".to_owned(), serde_json::json!(tab.0));
    }
    bus.emit(event, payload);
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

    /// The overlay lanes and the editor claim, before the byte
    /// translation (rich-input RI-4's gate beside the standing
    /// menu routing): the completion menu's own key lane while it
    /// shows and the Ctrl+Space NUL special case — both standing
    /// down in rich mode, where the grid menu is dark and the
    /// editor's surfaces arrive with the parity replay (RI-5) —
    /// then the rich-input claim itself. True when the press is
    /// consumed.
    fn route_overlay_and_claim(
        &mut self,
        keystroke: &gpui::Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        // The rich-input claim (RI-4): the v1 chord table takes the
        // press ahead of the byte translation when the editor owns
        // the line — an unmapped chord falls through and forwards
        // verbatim, and native mode never claims (the claim is gated
        // inside the engine's own routing answer).
        self.route_editor_claim(keystroke, cx)
    }

    /// The rich-input claim (rich-input spec, RI-4): with the mode
    /// rich and the editor owning the line, the v1 chord table takes
    /// the press ahead of the byte translation — an unmapped chord
    /// falls through (false) and forwards verbatim, and native mode
    /// never claims at all (the claim is gated inside the engine's
    /// own routing answer).
    fn route_editor_claim(&mut self, keystroke: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        if !self
            .terminal
            .lock()
            .expect("engine lock")
            .editor_owns_line()
        {
            return false;
        }
        // The menu layer (rich-input RI-5): while the menu shows
        // inside the editor, the parity table owns the press.
        if self.terminal.lock().expect("engine lock").menu_is_open()
            && self.route_editor_menu_press(keystroke, cx)
        {
            return true;
        }
        // Tab while closed (the lift's conditioned opener): a line
        // with a qualifying row opens the menu; the decline falls
        // through and Tab forwards to the shell verbatim — SP-D10's
        // Tab stays the native lane's rule, the lift is the
        // editor's own. Bare Tab alone claims (tab-surface OQ-1):
        // Shift+Tab forwards as the shell's back-tab while the menu
        // is closed.
        if editor::opener_claims_tab(keystroke) {
            let (to_caret, _) = self.editor_to_caret();
            if self
                .terminal
                .lock()
                .expect("engine lock")
                .editor_menu_open(&to_caret)
            {
                cx.notify();
                return true;
            }
            return false;
        }
        // The ghost's accepts (the settled set): bare Right takes
        // the whole showing, Alt+Right one word — only while showing
        // and only with the caret at the line's end (the ghost
        // paints after the buffer; a mid-line Right moves).
        if let Some(kind) = suggest::accept_kind(&keystroke.key, keystroke.modifiers) {
            let showing = self
                .terminal
                .lock()
                .expect("engine lock")
                .suggestion()
                .is_some();
            let at_end = self.editor.lock().expect("editor lock").caret_at_end();
            if showing && at_end {
                let gained = {
                    let mut engine = self.terminal.lock().expect("engine lock");
                    engine.editor_accept_suggestion(kind)
                };
                if let Some(suffix) = gained {
                    self.editor.lock().expect("editor lock").paste(&suffix);
                    self.step_editor_lane();
                    cx.notify();
                    return true;
                }
            }
        }
        // The buffer keys: the v1 chord table, the history slice fed
        // fresh at each navigation entry (the first Up of a line).
        if let Some(key) = editor::editor_key(keystroke) {
            if key == Key::Up && self.editor.lock().expect("editor lock").at_live_line() {
                let history = self.terminal.lock().expect("engine lock").editor_history();
                self.editor
                    .lock()
                    .expect("editor lock")
                    .set_history(history);
            }
            self.apply_editor_key(key, cx);
            true
        } else {
            false
        }
    }

    /// One press against the editor's showing menu (rich-input
    /// RI-5): the parity table's decision applied over the lane —
    /// navigation, the two-stage Enter, Tab's fill-or-cycle, the
    /// close paths — with `KeepOpen` and `CloseThenProcess` falling
    /// through to the buffer keys after their own side effects.
    /// True when the press is consumed here.
    fn route_editor_menu_press(
        &mut self,
        keystroke: &gpui::Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        // The grid menu's key lane retired with its paint
        // (rich-input RI-10): native mode's Ctrl+Space is the bare
        // NUL byte through the translation, the editor's own menu
        // rides the claim below.
        use dropterm::core::terminal::suggest::MenuKey;
        let (to_caret, _) = self.editor_to_caret();
        let equal_line = {
            let engine = self.terminal.lock().expect("engine lock");
            engine.editor_menu_selected_equals(&to_caret)
        };
        match editor::editor_menu_routing(keystroke, equal_line) {
            editor::EditorMenuRouting::Navigate(menu_key) => {
                self.terminal
                    .lock()
                    .expect("engine lock")
                    .editor_menu_press(menu_key, &to_caret);
                cx.notify();
                true
            }
            editor::EditorMenuRouting::Accept => {
                let outcome = {
                    let mut engine = self.terminal.lock().expect("engine lock");
                    engine.editor_menu_press(MenuKey::Accept, &to_caret)
                };
                self.apply_editor_menu_outcome(outcome, cx);
                true
            }
            editor::EditorMenuRouting::CloseThenRun => {
                self.terminal.lock().expect("engine lock").menu_close();
                self.apply_editor_key(Key::Enter, cx);
                true
            }
            editor::EditorMenuRouting::Close => {
                self.terminal.lock().expect("engine lock").menu_close();
                cx.notify();
                true
            }
            editor::EditorMenuRouting::FillOrCycle => {
                // The fill compares the rows' own continuations
                // against the typed segment: the text to the caret.
                let fill = {
                    let engine = self.terminal.lock().expect("engine lock");
                    editor::tab_fill(engine.menu_rows(), &to_caret)
                };
                match fill {
                    editor::TabFill::Insert { beyond, close } => {
                        self.editor.lock().expect("editor lock").paste(&beyond);
                        if close {
                            self.terminal.lock().expect("engine lock").menu_close();
                        }
                        self.step_editor_lane();
                        cx.notify();
                    }
                    editor::TabFill::Cycle => {
                        self.terminal
                            .lock()
                            .expect("engine lock")
                            .editor_menu_press(MenuKey::Down, &to_caret);
                        cx.notify();
                    }
                }
                true
            }
            // The printable edit: the buffer keys insert, the lane's
            // step re-collects, the menu stays open.
            editor::EditorMenuRouting::KeepOpen => false,
            // The press's own meaning runs after the close — the
            // buffer keys, the ghost accepts, or the fall-through.
            editor::EditorMenuRouting::CloseThenProcess => {
                self.terminal.lock().expect("engine lock").menu_close();
                false
            }
        }
    }

    /// Apply a lane menu outcome to the buffer (rich-input RI-5):
    /// the insert lands at the caret (an undoable edit, the accept
    /// funnel's own shape) and the lane steps; Execute runs the line
    /// through the editor's own Enter.
    fn apply_editor_menu_outcome(&mut self, outcome: editor::MenuPress, cx: &mut Context<Self>) {
        match outcome {
            editor::MenuPress::Insert(suffix) => {
                self.editor.lock().expect("editor lock").paste(&suffix);
                self.step_editor_lane();
                cx.notify();
            }
            editor::MenuPress::Execute => self.apply_editor_key(Key::Enter, cx),
            editor::MenuPress::Noop => cx.notify(),
        }
    }

    /// The owned buffer's step inputs: the text to the caret and the
    /// caret-at-end fact.
    fn editor_to_caret(&self) -> (String, bool) {
        let state = self.editor.lock().expect("editor lock");
        let (to_caret, after_blank) = state.step_inputs();
        (to_caret.to_string(), after_blank)
    }

    /// Step the suggest lane over the owned buffer (rich-input
    /// RI-5): the ghost, the anchor bookkeeping, and the auto-pop
    /// re-collect answer the editor's line after every mutation.
    fn step_editor_lane(&mut self) {
        let (to_caret, after_blank) = self.editor_to_caret();
        self.terminal
            .lock()
            .expect("engine lock")
            .editor_step(&to_caret, after_blank);
    }

    /// One claimed keystroke through the pure editor model
    /// (rich-input RI-4): the verdict routes — Handled repaints the
    /// strip and steps the lane, Execute PTY-writes through the
    /// engine's own execution (the echo handoff riding it), and the
    /// signals forward their byte or clear the screen (Warp's own
    /// Ctrl+L reading, the record's finding 3: the history goes,
    /// the owned line stands).
    fn apply_editor_key(&mut self, key: Key, cx: &mut Context<Self>) {
        let verdict = self.editor.lock().expect("editor lock").key(key);
        // Every buffer mutation steps the lane (RI-5): the ghost and
        // the auto-pop answer the editor's line, never the grid's.
        let mutated = matches!(verdict, Verdict::Handled | Verdict::Passthrough);
        {
            let mut engine = self.terminal.lock().expect("engine lock");
            match verdict {
                // Handled repaints the strip alone; Passthrough is
                // unreachable in v1 (every mapped key handles) — the
                // arm keeps the match honest for the model's future
                // keys.
                Verdict::Handled | Verdict::Passthrough => {}
                Verdict::Execute { line } => {
                    println!(
                        "dropterm: rich-input execute — {} bytes PTY-written",
                        line.len() + 1
                    );
                    engine.editor_execute(&line);
                    engine.needs_update = true;
                }
                Verdict::Signal(Signal::Interrupt) => {
                    engine.input_scroll(b"\x03".to_vec());
                    engine.needs_update = true;
                }
                Verdict::Signal(Signal::EndOfFile) => {
                    engine.input_scroll(b"\x04".to_vec());
                    engine.needs_update = true;
                }
                Verdict::Signal(Signal::ClearScreen) => {
                    engine.term.lock().grid_mut().clear_history();
                    engine.needs_update = true;
                }
            }
        }
        if mutated {
            self.step_editor_lane();
        }
        cx.notify();
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
                    // The rich-input lane (rich-input RI-4): while the
                    // editor owns the line, paste feeds the buffer
                    // verbatim — the PTY never sees it until Enter
                    // runs the whole line.
                    let editor_owns = {
                        let mut engine = self.terminal.lock().expect("engine lock");
                        if engine.editor_owns_line() {
                            true
                        } else {
                            engine.paste(&text);
                            engine.needs_update = true;
                            false
                        }
                    };
                    if editor_owns {
                        self.editor.lock().expect("editor lock").paste(&text);
                        self.step_editor_lane();
                    }
                    println!(
                        "dropterm: pane action Paste — {} bytes {}",
                        text.len(),
                        if editor_owns {
                            "into the owned line"
                        } else {
                            "through the engine's paste"
                        }
                    );
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
    pub(crate) fn adopt_shell(&mut self, shell: gpui::WeakEntity<crate::app_shell::Shell>) {
        self.shell = Some(shell);
    }

    /// Take the Lua bus inlet from the shell's handoff (event-hooks
    /// spec): the shell calls this only when the plugins dir stood at
    /// boot — the pane never re-checks the dir, so the gate stays a
    /// boot-time fact.
    pub(crate) fn set_lua_bus(&mut self, bus: dropterm::core::lua_host::LuaHostHandle) {
        self.lua_bus = Some(bus);
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
    fn dispatch_shell_verb(
        &mut self,
        action: TermAction,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let Some(shell) = self.shell.as_ref().and_then(gpui::WeakEntity::upgrade) else {
            println!("dropterm: pane shell verb {action:?} (no shell in the walk harness)");
            return;
        };
        let pane = self.id;
        window.defer(cx, move |window, cx| {
            shell.update(cx, |shell, cx| shell.shell_verb(&action, pane, window, cx));
        });
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
        // The overlay lanes' routing and the rich-input claim, in
        // order: a showing menu (native mode) outranks everything;
        // the Ctrl+Space special case rides behind it; then the
        // editor's v1 chord table takes its press when the line is
        // owned — each a fall-through, never a swallow.
        if self.route_overlay_and_claim(keystroke, cx) {
            return;
        }
        // The Space special case (the libcosmic widget's own,
        // verbatim): Ctrl+Space is the NUL press wherever nothing
        // above consumed it — the translation's own arm (the
        // control-character rule cannot see a space), never a
        // swallow.
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
    /// A pointer event's window-space position onto the pane's own
    /// coordinates (the conversion the cell arithmetic must run on):
    /// GPUI's mouse events carry window coordinates, and the pane
    /// sits below the strip and the bands — the painted origin is the
    /// frame's truth, captured fresh every frame by the paint.
    fn pane_local(&self, position: gpui::Point<Pixels>) -> gpui::Point<Pixels> {
        let origin = *self.viewport_origin.lock().expect("pane origin");
        let (x, y) = selection::pane_local(
            (position.x.as_f32(), position.y.as_f32()),
            (origin.x.as_f32(), origin.y.as_f32()),
        );
        point(px(x), px(y))
    }

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
    /// the cell the position lands in (the window→pane conversion
    /// first — the position arrives in window coordinates).
    fn report_pointer(
        &mut self,
        event: PointerEvent,
        position: gpui::Point<gpui::Pixels>,
        modifiers: gpui::Modifiers,
    ) {
        let position = self.pane_local(position);
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
        // The window→pane conversion (the status band's own shape):
        // the event arrives in window coordinates and the cell
        // arithmetic runs on the pane's own space.
        let position = self.pane_local(position);
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
    /// selection to the pointer's cell (the window→pane conversion
    /// first — the position arrives in window coordinates).
    fn on_left_drag(&mut self, position: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        let position = self.pane_local(position);
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
    /// selection — expanding its sides first, so every cell the drag
    /// touched is included (the boundary-cell fix), then logging the
    /// byte count a Copy would take. The libcosmic rows carry no
    /// copy-on-select — the selection stands until Copy grabs it.
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
                let mut term = terminal.term.lock();
                // The release expands the standing selection's sides
                // (`Selection::include_all`): `range_simple` drops a
                // Right-anchored start's own cell and a Left-ended
                // release's own cell, so without this every drag's
                // first and last character hinged on which sub-cell
                // half the pointer landed in. The empty case is
                // skipped — a plain click's same-anchor selection must
                // stay nothing, and include_all would split its
                // anchors' sides into a one-cell "selection"
                // (selection.rs's trap pin).
                if let Some(ref mut standing) = term.selection
                    && !standing.is_empty()
                {
                    standing.include_all();
                }
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
            let position = self.pane_local(position);
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
        let viewport_origin = Arc::clone(&self.viewport_origin);
        let pane = cx.entity();
        // The focusable frame: keys and the pointer route to the
        // pane, the canvas
        // paints inside it (carrying its own `size_full` — a
        // default-styled child collapses; the first walk's lesson).
        let frame = div()
            .size_full()
            .key_context("Terminal")
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
        // The Tab claim's landing (key-delivery B3): the Root's
        // traversal binding never yields once matched, so the pane
        // answers the keystrokes in its own deeper "Terminal" context
        // (depth precedence) and forwards them into on_key as the
        // keystrokes the platform would have delivered — every
        // downstream rule then applies exactly as it does for letters.
        let frame = frame
            .on_action({
                let pane = pane.clone();
                move |_: &TerminalTab, window: &mut gpui::Window, cx: &mut gpui::App| {
                    pane.update(cx, |pane, cx| {
                        pane.on_key(&tab_keystroke(false), window, cx);
                    });
                }
            })
            .on_action({
                let pane = pane.clone();
                move |_: &TerminalBackTab, window: &mut gpui::Window, cx: &mut gpui::App| {
                    pane.update(cx, |pane, cx| pane.on_key(&tab_keystroke(true), window, cx));
                }
            });
        // The editor menu's paint bundle (rich-input RI-7): the
        // chrome capture under one engine read — None when the menu
        // stands closed, the paint's whole gate.
        let strip_top_dock = editor::strip_docked_top(self.options.input_position);
        // The editor's fact for this frame, under one engine read
        // beside the menu's bundle: whether it owns the line — asked
        // once for both canvases, so no frame paints both carets or
        // neither. The terminal's own cursor mode stays the grid's
        // post-update read, where it always was.
        let (editor_menu, editor_owns_line) = {
            let engine = self.terminal.lock().expect("engine lock");
            let editor_owns_line = engine.editor_owns_line();
            let state = self.editor.lock().expect("editor lock");
            let menu = editor::EditorMenuPaint::capture(
                &engine,
                &state,
                editor::MenuChrome {
                    descriptions: self.options.input_descriptions,
                    badges: self.options.input_badges,
                    icons: self.options.input_icons,
                    argument_hint: self.options.input_argument_hint,
                    compact_rows: self.options.input_compact_rows,
                },
            );
            (menu, editor_owns_line)
        };
        // The rich-input strip (rich-input spec, RI-4): in rich mode
        // the owned line paints in its own docked row under the grid
        // and the grid's prompt-row overlays stand down — the column
        // replaces the canvas's size-full so the rows share the
        // frame. The mode flip rides `apply_options`' live pass, so
        // the strip appears and disappears with the config echo.
        // The row belongs to the mode, not to the owned line: it
        // stays in the layout while the editor stands aside and
        // paints nothing, because a row that collapsed at every
        // prompt-to-program edge would resize the PTY each time, and
        // a resize repaints the shell's prompt.
        let grid = canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, cx| {
                let seat_moved = paint_pane(
                    &terminal,
                    &skin,
                    &walk_log,
                    &image_cache,
                    &viewport_origin,
                    editor_menu.as_ref(),
                    editor_owns_line,
                    strip_top_dock,
                    bounds,
                    window,
                    cx,
                );
                // The ownership was read for this frame before the
                // frame's own update ran, and an update can move it
                // (the grid read adopting a shell's first prompt). A
                // plain notify is swallowed mid-draw; this asks for
                // the frame that paints the settled seat, and nothing
                // further once it has.
                if seat_moved {
                    window.request_animation_frame();
                }
            },
        );
        if self.options.input_mode == dropterm::core::config::InputMode::Rich {
            // The dock (rich-input RI-9): `top` pins the strip at the
            // head of the grid region — the column's order flips, the
            // flow a normal terminal either way.
            let strip_first = editor::strip_docked_top(self.options.input_position);
            let strip_editor = Arc::clone(&self.editor);
            let strip_terminal = Arc::clone(&self.terminal);
            let strip_skin = self.skin.clone();
            // Points onto pixels (96/72): the strip shapes in the
            // terminal's own font, sized from the pane's option.
            let font_px = px(f32::from(self.options.font_size) * 4. / 3.);
            let highlight = self.options.input_highlight;
            let strip = canvas(
                move |bounds, _, _| bounds,
                move |bounds, _, window, cx| {
                    paint_strip(
                        &strip_editor,
                        &strip_terminal,
                        &strip_skin,
                        editor_owns_line,
                        bounds,
                        font_px,
                        highlight,
                        window,
                        cx,
                    );
                },
            )
            .w_full()
            .h(px(30.));
            // One column, the strip at the end the dock names. The
            // strip is a fixed row and the grid takes the rest on a
            // zero basis: nothing competes with the strip for the
            // height, so the row is exactly its own at both docks.
            let column = div().flex().flex_col().size_full();
            let grid = grid.flex_1().min_h_0();
            frame.child(if strip_first {
                column.child(strip).child(grid)
            } else {
                column.child(grid).child(strip)
            })
        } else {
            frame.child(grid.size_full())
        }
    }
}

/// One pane's paint: resize to the granted bounds, run the pin
/// funnel's settle close, drain the update flag, then walk the
/// visible grid through the projection and the shaper. One function
/// reads the whole frame together — the grid walk, the selection
/// overlay, and the evidence rows share the one lock and the one
/// geometry; splitting them re-derives the lock order per piece.
/// Returns whether the frame's own update moved the caret seat it
/// was painted on — the caller's cue for one more frame.
#[expect(
    clippy::too_many_lines,
    reason = "the frame reads one lock and one geometry end to end; the walk, the overlay, and the evidence rows are one pass"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "the frame's shared handles each stand alone at the call site; a param struct would bundle five unrelated Arcs"
)]
fn paint_pane(
    terminal: &Arc<Mutex<Terminal>>,
    skin: &PaneSkin,
    walk_log: &Arc<Mutex<WalkLog>>,
    image_cache: &Arc<Mutex<super::images::ImageCache>>,
    viewport_origin: &Arc<Mutex<gpui::Point<Pixels>>>,
    editor_menu: Option<&editor::EditorMenuPaint>,
    editor_owns_line: bool,
    strip_top_dock: bool,
    bounds: gpui::Bounds<Pixels>,
    window: &mut gpui::Window,
    cx: &mut App,
) -> bool {
    // The pane's window-space origin, this frame's truth: the pointer
    // handlers convert window coordinates through it (GPUI's mouse
    // events carry window coordinates; the painted bounds are the
    // slide-aware layout, the slide's `top` included).
    *viewport_origin.lock().expect("pane origin") = bounds.origin;
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

    // The ownership this frame was handed was read before the update
    // above ran; this is the engine's answer after it. The two agree
    // on every frame but the one whose update moved them. Read before
    // the term guard below: the gates take that lock themselves.
    let owns_line_now = terminal.editor_owns_line();

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

    // The caret: shown while the frame's caret seat is the grid's —
    // the terminal's cursor mode, read after this frame's update as
    // ever, and the editor's ownership, folded by `caret_seat` — and
    // always a block (the libcosmic build's rule). Grid semantics
    // (pinned by the probe test in this module): `Line(0)` is the
    // screen's top, lines grow downward, and the cursor's line is a
    // screen row — so scrolled state `d` puts it at display row
    // `cursor.line + d`, on the display only while that lands inside
    // the rows.
    let cursor = grid.cursor.point;
    let shows_cursor = term.mode().contains(TermMode::SHOW_CURSOR);
    let seat = editor::caret_seat(editor_owns_line, shows_cursor);
    let seat_moved = editor::caret_seat(owns_line_now, shows_cursor) != seat;
    let caret_row = (seat == Some(editor::CaretSeat::Grid))
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

    // The window background under every row.
    pane_background(skin, bounds, window);

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

    // The walk's seat evidence row: which input holds the caret,
    // once per change — the stand-aside walk reads it beside the
    // pixels.
    {
        let mut log = walk_log.lock().expect("walk log");
        if log.caret_seat != seat {
            let row = match seat {
                Some(editor::CaretSeat::Strip) => {
                    "caret in the input strip — the editor owns the line, the grid's block caret stands down"
                }
                Some(editor::CaretSeat::Grid) => {
                    "caret in the grid — the shell's line is the input"
                }
                None => "no caret — the terminal hides its cursor",
            };
            println!("dropterm: {row}");
            log.caret_seat = seat;
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

    // The editor's own menu panel (rich-input RI-7): the chrome
    // above the strip — the grid's overlay paint retired (RI-10),
    // this is the menu's one surface.
    if let Some(menu) = editor_menu {
        paint_editor_menu(
            menu,
            strip_top_dock,
            skin,
            bounds,
            geometry.font_size,
            window,
            cx,
        );
    }
    seat_moved
}

/// A cosmic-text color as GPUI color (the ghost tint and the menu's
/// text seats — the engine hands scheme colors over in cosmic-text's
/// shape).
fn color_hsla(color: cosmic_text::Color) -> gpui::Hsla {
    gpui::rgb(u32::from(color.r()) << 16 | u32::from(color.g()) << 8 | u32::from(color.b())).into()
}

/// The editor menu's panel paint (rich-input RI-7): the mock's
/// chrome above the strip — icon, the marked label (the typed
/// prefix semibold), the badge chip in its class color, the
/// description dim (its own line comfortable, inline compact), the
/// selection's tint, the visible window from the scroll offset, the
/// argument hint line above the panel, and the popover surface the
/// grid menu itself rides — every element behind its toggle at
/// capture time (the bundle carries None).
#[expect(
    clippy::too_many_lines,
    reason = "one panel painted in one pass — rows, chrome, and hint share the one geometry"
)]
fn paint_editor_menu(
    menu: &editor::EditorMenuPaint,
    top_dock: bool,
    skin: &PaneSkin,
    bounds: gpui::Bounds<Pixels>,
    font_px: Pixels,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) {
    let theme = cx.theme();
    let panel_bg = theme.popover;
    let panel_border = theme.border;
    let fg = theme.popover_foreground;
    let dim = gpui::hsla(fg.h, fg.s, fg.l, 0.55);
    let palette = TokenPalette::for_scheme(fg.l > 0.5);

    let row_h = px(editor::menu_row_height_px(menu.chrome.compact_rows));
    let range = suggest::menu_visible_window(menu.scroll, menu.rows.len());
    if range.is_empty() {
        return;
    }
    let visible = &menu.rows[range.clone()];
    #[expect(
        clippy::cast_precision_loss,
        reason = "the visible row count is the menu's budget (<=8), exact in f32"
    )]
    let rows_height = row_h * visible.len() as f32;
    let hint_h = menu.hint.as_ref().map_or(px(0.), |_| px(26.));
    let width = (px(460.)).min(bounds.size.width - px(16.));
    let panel = gpui::Bounds {
        origin: point(
            bounds.origin.x + px(8.),
            editor::menu_anchor_y(top_dock, bounds, rows_height + hint_h),
        ),
        size: gpui::Size::new(width, rows_height + hint_h),
    };
    let mut panel_quad = fill(panel, panel_bg);
    panel_quad.corner_radii = gpui::Corners::all(px(6.));
    panel_quad.border_widths = gpui::Edges::all(px(1.));
    panel_quad.border_color = panel_border;
    window.paint_quad(panel_quad);

    // The argument hint line (the mock's own seat: the panel's top
    // strip, the command semibold, its description dim).
    let mut y = panel.origin.y;
    if let Some((name, description)) = &menu.hint {
        let mut hint_font = skin.font.clone();
        hint_font.weight = FontWeight::SEMIBOLD;
        let shaped = shape_overlay_line(name, &hint_font, fg, font_px, window);
        let _ = shaped.paint(
            point(panel.origin.x + px(10.), y + px(4.)),
            font_px,
            TextAlign::Left,
            None,
            window,
            cx,
        );
        let x_after = panel.origin.x + px(10.) + shaped.width + px(8.);
        if let Some(description) = description {
            let shaped = shape_overlay_line(description, &skin.font, dim, font_px, window);
            let _ = shaped.paint(
                point(x_after, y + px(4.)),
                font_px,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
        y += hint_h;
    }

    // The rows: icon, the marked label, the badge, the description —
    // the selected one tinted.
    let badge_color = |badge: &str| -> gpui::Hsla {
        match badge {
            "cmd" => palette.cmd,
            "flag" => palette.flag,
            "path" => palette.string,
            "env" => palette.err,
            "hist" => dim,
            _ => fg,
        }
    };
    for (index, row) in visible.iter().enumerate() {
        let absolute = range.start + index;
        let row_bounds = gpui::Bounds {
            origin: point(panel.origin.x, y),
            size: gpui::Size::new(panel.size.width, row_h),
        };
        if absolute == menu.selected {
            window.paint_quad(fill(
                gpui::Bounds {
                    origin: point(row_bounds.origin.x + px(2.), row_bounds.origin.y + px(1.)),
                    size: gpui::Size::new(row_bounds.size.width - px(4.), row_h - px(2.)),
                },
                gpui::hsla(fg.h, fg.s, fg.l, 0.12),
            ));
        }
        let mut x = row_bounds.origin.x + px(10.);
        if let Some(icon) = row.icon {
            let shaped = shape_overlay_line(&icon.to_string(), &skin.font, dim, font_px, window);
            let _ = shaped.paint(
                point(x, y + (row_h - font_px) / 2.),
                font_px,
                TextAlign::Left,
                None,
                window,
                cx,
            );
            x += shaped.width + px(8.);
        }
        // The marked label: the typed prefix semibold, the rest
        // normal — two shapes split at the char mark.
        let marked_byte = row
            .label
            .char_indices()
            .nth(row.marked)
            .map_or(row.label.len(), |(byte, _)| byte);
        let (before, after) = row.label.split_at(marked_byte);
        let mut mark_font = skin.font.clone();
        mark_font.weight = FontWeight::SEMIBOLD;
        let shaped = shape_overlay_line(before, &mark_font, fg, font_px, window);
        let _ = shaped.paint(
            point(x, y + (row_h - font_px) / 2.),
            font_px,
            TextAlign::Left,
            None,
            window,
            cx,
        );
        x += shaped.width;
        if !after.is_empty() {
            let shaped = shape_overlay_line(after, &skin.font, fg, font_px, window);
            let _ = shaped.paint(
                point(x, y + (row_h - font_px) / 2.),
                font_px,
                TextAlign::Left,
                None,
                window,
                cx,
            );
            x += shaped.width;
        }
        if let Some(badge) = row.badge {
            x += px(8.);
            let shaped = shape_overlay_line(badge, &skin.font, badge_color(badge), font_px, window);
            let _ = shaped.paint(
                point(x, y + (row_h - font_px) / 2.),
                font_px,
                TextAlign::Left,
                None,
                window,
                cx,
            );
            x += shaped.width;
        }
        if let Some(description) = &row.description {
            let desc_x = if menu.chrome.compact_rows {
                x + px(8.)
            } else {
                row_bounds.origin.x + px(28.)
            };
            let desc_y = if menu.chrome.compact_rows {
                y + (row_h - font_px) / 2.
            } else {
                y + row_h - font_px - px(4.)
            };
            let shaped = shape_overlay_line(description, &skin.font, dim, font_px, window);
            let _ = shaped.paint(
                point(desc_x, desc_y),
                font_px,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
        y += row_h;
    }
}

/// The mock's token palette (rich-input RI-6): the light/dark pairs
/// keyed on the scheme's foreground luminance — command, flag,
/// string, and the unknown-command error red (the variable rides
/// the string green; the mock carries no variable color of its own).
struct TokenPalette {
    cmd: gpui::Hsla,
    flag: gpui::Hsla,
    string: gpui::Hsla,
    err: gpui::Hsla,
}

impl TokenPalette {
    fn for_scheme(light: bool) -> Self {
        Self {
            cmd: hex_hsla(if light { 0x174_7c2 } else { 0x82b_7ff }),
            flag: hex_hsla(if light { 0x966_60a } else { 0xffd_37e }),
            string: hex_hsla(if light { 0x177_245 } else { 0x93e_6a5 }),
            err: hex_hsla(if light { 0xc03_434 } else { 0xff8_080 }),
        }
    }

    fn color(&self, class: editor::TokenClass, plain: gpui::Hsla) -> gpui::Hsla {
        match class {
            editor::TokenClass::Command { known: true } => self.cmd,
            editor::TokenClass::Command { known: false } => self.err,
            editor::TokenClass::Flag => self.flag,
            editor::TokenClass::String | editor::TokenClass::Variable => self.string,
            editor::TokenClass::Word => plain,
        }
    }
}

fn hex_hsla(hex: u32) -> gpui::Hsla {
    gpui::rgb(hex).into()
}

/// The strip's own shaper (rich-input RI-6): [`shape_overlay_line`]'s
/// shape with an optional underline — the unknown command's error
/// mark (gpui's underline is solid or wavy; the record's dashed is
/// approximated solid, a walk finding to refine).
fn shape_underlined_line(
    text: &str,
    font: &gpui::Font,
    color: gpui::Hsla,
    font_size: Pixels,
    underline: Option<UnderlineStyle>,
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
            underline,
            strikethrough: None,
        }],
        None,
    )
}

/// The pane's own background under a paint: the scheme background
/// at the configured opacity — the grid's rows and the strip's row
/// share it, so the strip reads as the terminal's row and not a bar
/// of its own (the mock's input zone carries no background).
fn pane_background(skin: &PaneSkin, bounds: gpui::Bounds<Pixels>, window: &mut gpui::Window) {
    let mut bg = grid::window_background(&skin.colors);
    bg.a *= skin.opacity_alpha;
    window.paint_quad(fill(bounds, bg));
}

/// The rich-input strip's paint (rich-input RI-4 + RI-6): the owned
/// line shaped in the terminal's own font, segment by colored
/// segment — the highlight tokenizer's classes when the toggle is
/// on, one plain tint when it is off — with the caret quad flush at
/// its byte offset and the ghost's dim suffix after the whole line.
/// The chrome (RI-7) restyles the row against the mock around this
/// token floor. Only a frame whose editor owns the line paints one;
/// every other frame the row carries the background alone — the
/// editor stands aside, and the grid's caret is the one on screen.
#[expect(
    clippy::too_many_arguments,
    reason = "the strip's paint frame mirrors paint_pane's own handle list; a param struct would bundle six unrelated facts"
)]
#[expect(
    clippy::too_many_lines,
    reason = "the strip paints one pass — the segment walk, the caret split, and the ghost share the one lock-derived snapshot"
)]
fn paint_strip(
    editor_state: &Arc<Mutex<editor::Editor>>,
    terminal: &Arc<Mutex<Terminal>>,
    skin: &PaneSkin,
    editor_owns_line: bool,
    bounds: gpui::Bounds<Pixels>,
    font_px: Pixels,
    highlight: bool,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) {
    pane_background(skin, bounds, window);
    if !editor_owns_line {
        return;
    }
    let (segments, ghost, caret_byte, fg, dim, palette) = {
        let engine = terminal.lock().expect("engine lock");
        let ghost = engine.suggestion().map(str::to_string);
        let fg = color_hsla(engine.foreground_color());
        let dim = color_hsla(engine.ghost_color());
        let palette = TokenPalette::for_scheme(fg.l > 0.5);
        let (segments, caret_byte) = {
            let state = editor_state.lock().expect("editor lock");
            let caret_byte = state.step_inputs().0.len();
            let segments =
                editor::segments_for(state.line(), highlight, |name| engine.command_known(name));
            (segments, caret_byte)
        };
        (segments, ghost, caret_byte, fg, dim, palette)
    };
    let pad = px(8.);
    let origin = point(
        bounds.origin.x + pad,
        bounds.origin.y + (bounds.size.height - font_px) / 2.,
    );
    let caret_quad = |x: Pixels, window: &mut gpui::Window| {
        window.paint_quad(fill(
            gpui::Bounds {
                origin: point(x, origin.y),
                size: gpui::Size::new(px(2.), font_px),
            },
            fg,
        ));
    };
    // Walk the segments, splitting the one carrying the caret so the
    // quad lands flush between its halves.
    let mut x = origin.x;
    let mut painted_bytes = 0usize;
    let mut caret_placed = false;
    for (class, text) in &segments {
        let start = painted_bytes;
        painted_bytes += text.len();
        let color = palette.color(*class, fg);
        let underline =
            matches!(class, editor::TokenClass::Command { known: false }).then(|| UnderlineStyle {
                color: Some(palette.err),
                thickness: px(1.),
                wavy: false,
            });
        if !caret_placed && start <= caret_byte && caret_byte <= painted_bytes {
            let (before, after) = text.split_at(caret_byte - start);
            if !before.is_empty() {
                let shaped =
                    shape_underlined_line(before, &skin.font, color, font_px, underline, window);
                let _ = shaped.paint(
                    point(x, origin.y),
                    font_px,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
                x += shaped.width;
            }
            caret_quad(x, window);
            x += px(2.);
            if !after.is_empty() {
                let shaped =
                    shape_underlined_line(after, &skin.font, color, font_px, underline, window);
                let _ = shaped.paint(
                    point(x, origin.y),
                    font_px,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
                x += shaped.width;
            }
            caret_placed = true;
        } else if !text.is_empty() {
            let shaped = shape_underlined_line(text, &skin.font, color, font_px, underline, window);
            let _ = shaped.paint(
                point(x, origin.y),
                font_px,
                TextAlign::Left,
                None,
                window,
                cx,
            );
            x += shaped.width;
        }
    }
    if !caret_placed {
        caret_quad(x, window);
        x += px(2.);
    }
    if let Some(ghost) = ghost.filter(|ghost| !ghost.is_empty()) {
        let shaped = shape_overlay_line(&ghost, &skin.font, dim, font_px, window);
        let _ = shaped.paint(
            point(x, origin.y),
            font_px,
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }
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

/// Shape one projected run onto its own cells. The snap GPUI applies
/// to a forced line steps glyphs by the FORCE WIDTH itself —
/// `forced_x = glyph_index * force_width`, the snap's own "cell
/// counter" — so the force passed here is the CELL width, never the
/// run's total span: a span-force displaces every glyph past the
/// run's half to multiples of the whole span (the overlapping-text
/// regression's mechanism). Under the per-cell force a run's glyph
/// *i* lands at `i * cell_width` from the run's origin, so the run's
/// cells and its glyphs agree.
fn shape_run(
    run: &StyledRun,
    font: &gpui::Font,
    geometry: &GridGeometry,
    text_system: &gpui::WindowTextSystem,
) -> gpui::ShapedLine {
    let mut run_font = font.clone();
    if run.bold {
        run_font.weight = FontWeight::BOLD;
    }
    if run.italic {
        run_font.style = gpui::FontStyle::Italic;
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
    let text_runs = [gpui::TextRun {
        len: run.text.len(),
        font: run_font,
        color: run.fg,
        background_color: run.bg,
        underline,
        strikethrough,
    }];
    text_system.shape_line(
        run.text.as_str().into(),
        geometry.font_size,
        &text_runs,
        Some(geometry.cell_width),
    )
}

/// Paint one projected row: each run shaped alone onto its own cells
/// and placed at its start column — every glyph sits on its grid
/// columns whatever the wide chars before it did (the old whole-row
/// shape stacked the shaper's natural advances, so a fallback-font
/// emoji that does not advance exactly two cells pushed every glyph
/// after it left — select-accuracy S2's defect). All run backgrounds
/// land before any glyphs, so a spacer run's cell rect — the wide
/// glyph's tail cell, at its own column — never covers the glyph it
/// overlaps.
fn paint_row(
    runs: &[StyledRun],
    skin: &PaneSkin,
    geometry: &GridGeometry,
    origin: gpui::Point<Pixels>,
    window: &mut gpui::Window,
    cx: &mut App,
) {
    let mut painted: Vec<(gpui::Point<Pixels>, gpui::ShapedLine)> = Vec::with_capacity(runs.len());
    for run in runs {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a grid's column count is far below 2^24: exact in f32"
        )]
        let run_origin = point(
            origin.x + geometry.cell_width * run.start_col as f32,
            origin.y,
        );
        // A spacer run owns its cell but no glyphs: its rect paints
        // straight (the shaper has nothing to place), and the run's
        // empty text keeps the joined row text — the walk-log's
        // evidence — byte-identical.
        if run.text.is_empty() {
            if let Some(bg) = run.bg {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a run's cell count is far below 2^24: exact in f32"
                )]
                let width = geometry.cell_width * run.cell_len as f32;
                window.paint_quad(fill(
                    gpui::Bounds {
                        origin: run_origin,
                        size: gpui::Size::new(width, geometry.cell_height),
                    },
                    bg,
                ));
            }
            continue;
        }
        let shaped = shape_run(run, &skin.font, geometry, window.text_system());
        // The background pass under this run's own glyphs.
        // `ShapedLine::paint` draws glyphs, underlines and strikes
        // only — a run's `background_color` reaches pixels solely
        // through this pass, without which an INVERSE span (bash's
        // bracketed-paste preview among them) paints its glyphs in
        // the swapped foreground over the pane background: text
        // present, the line reading blank until the program redraws
        // it plain. Runs without a background skip the pass and pay
        // nothing.
        if run.bg.is_some() {
            let _ = shaped.paint_background(
                run_origin,
                geometry.cell_height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
        painted.push((run_origin, shaped));
    }
    for (run_origin, shaped) in &painted {
        let _ = shaped.paint(
            *run_origin,
            geometry.cell_height,
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }
}

/// The scheme the pane resolves for a config at boot — the effective
/// colors chain with no profile (single pane, no tabs yet) and the
/// preference's own dark/light; `System` follows the real OS mode
/// the system-mode cell holds (theme-defaults td-2). The user
/// schemes ride in from the shell's holder (theme-files F2: a
/// stored user-scheme name resolves through them).
pub(crate) fn boot_colors(
    config: &dropterm::core::config::DropTermConfig,
    user_schemes: &[dropterm::core::theme_files::UserScheme],
) -> Colors {
    let is_dark = dropterm::core::theme_glue::effective_is_dark(
        config.theme_preference,
        dropterm::core::system_mode::is_dark(),
    );
    dropterm::core::theme_glue::effective_colors(config, None, is_dark, user_schemes)
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
        user_schemes: &[dropterm::core::theme_files::UserScheme],
    ) -> Self {
        let parts = dropterm::core::spawn::spawn_parts(command, config);
        // The injection decision, applied once here (shell-integration
        // spec, SI-6): this is the one construction path every spawn
        // shares — the app shell and the walk harness alike — so the
        // emitters ride whatever argv this request builds. A skip
        // keeps the parts byte-for-byte; the unsupported shells'
        // silence is the suite's default (`/bin/sh` never integrates,
        // never logs).
        let parts = match dropterm::core::shell_integration::decide(command, &parts, config) {
            dropterm::core::shell_integration::Injection::Injected(parts) => parts,
            dropterm::core::shell_integration::Injection::Skipped(_) => parts,
        };
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
                dropterm::core::theme_glue::effective_colors(
                    config,
                    Some(profile),
                    is_dark,
                    user_schemes,
                )
            }
            None => boot_colors(config, user_schemes),
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
mod tests;
