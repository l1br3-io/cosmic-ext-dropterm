// SPDX-License-Identifier: GPL-3.0-only

//! The onboarding flow for the summon key (summon-onboarding spec):
//! the first-run dialog's replacement — one step today, built as a
//! step list so later steps join without a rewrite. It offers two
//! paths, **Set up manually** (the environment's walk, the COSMIC
//! deep link, a "Check again" re-read into the confirmation state)
//! and **Configure for me** (the writer module's seat, unrendered
//! while [`dropterm::core::global_shortcut::setup_options`] keeps
//! the arm off — never a dead control). The skip contract is the
//! shipped dialog's: first run, "Don't ask again" unticked and no
//! key set re-opens at the next cold start; ticked retires the flow
//! through the settings write path; a bound key retires it through
//! any path. The same step re-opens from the settings panel's Global
//! group, where closing persists nothing.
//!
//! A sibling of `tray.rs`, which owns the About window the same way:
//! the flow is one nameable thing whose contents change together and
//! need nothing from the drawer but its `SummonSnapshot`.

use gpui::prelude::*;
use gpui::{
    App, Context, IntoElement, ParentElement, Render, Styled, Window, WindowBounds, WindowKind,
    WindowOptions, point, px, size,
};
use gpui_component::ActiveTheme as _;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::switch::Switch;
use gpui_component::{Sizable, h_flex, v_flex};

use dropterm::core::APP_ID;
use dropterm::core::global_shortcut as gs;
use dropterm::core::settings::PlannedWrite;

use crate::app_shell::{Shell, load_summon_snapshot};
use crate::drawer::SummonSnapshot;

/// Where the flow was opened from — the footer contract switches on
/// it: first run shows the "Don't ask again" checkbox and Skip;
/// settings shows Close only (nothing persisted on close).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The cold-boot open: the skip contract applies.
    FirstRun,
    /// The Global group's "Set up the summon key…" button: closing
    /// persists nothing.
    Settings,
}

/// The flow's steps, as a list: the summon-key ask today, its
/// confirmation landing when a check finds a chord. Window-local
/// state — no step engine, no persistence; a later step joins as a
/// new variant wired into [`Step::next`].
#[derive(Clone, Debug, PartialEq, Eq)]
enum Step {
    /// The summon-key ask: the walk, the deep link, the check.
    SummonKey,
    /// The landing after a found chord, carrying its display form.
    Confirmed(String),
}

impl Step {
    /// The step that follows this one — the ask advances to its
    /// confirmation when a check found a chord (the chord decides
    /// the landing, so the successor is data-driven); the
    /// confirmation is the list's tail. `None` at the tail.
    fn next(&self, found: Option<String>) -> Option<Self> {
        match self {
            Self::SummonKey => found.map(Self::Confirmed),
            Self::Confirmed(_) => None,
        }
    }
}

/// The onboarding window's view: one render, no stateful Kit halves —
/// the flow's logic lives in core ([`gs::setup_options`],
/// [`gs::onboarding_should_show`]), so this module is render plus
/// wiring and the window is covered by the owner's runtime walk.
#[expect(
    clippy::struct_excessive_bools,
    reason = "view state: each bool is its own render concern (the session family, the writer's seat, the retire box, the not-found line)"
)]
struct OnboardingView {
    shell: gpui::Entity<Shell>,
    origin: Origin,
    walk: &'static [&'static str],
    cosmic: bool,
    /// The session's answer from `setup_options`: the configure-for-me
    /// seat renders only while this is true (the writer's COSMIC arm
    /// over a writable store).
    configure_for_me: bool,
    step: Step,
    dismiss_checked: bool,
    /// A "Check again" that found nothing leaves the line standing
    /// under the walk.
    not_found: bool,
    /// The configure-for-me capture's armed flag: the next key press
    /// takes the verdict path (Escape cancels; a bare modifier keeps
    /// the capture armed, the keyboard page's own behavior).
    capture_armed: bool,
    /// The seat's inline error — a conflict naming its holder, an
    /// unsupported key, or a refused write. Cleared on the next
    /// attempt.
    seat_error: Option<String>,
    /// The window's keyboard anchor: the capture reads keys through
    /// the root's `on_key_down`, which needs a focused node.
    focus: gpui::FocusHandle,
}

impl OnboardingView {
    fn new(
        shell: gpui::Entity<Shell>,
        origin: Origin,
        snapshot: &SummonSnapshot,
        store: &gs::ReadCustom,
        cx: &mut Context<Self>,
    ) -> Self {
        let options = gs::setup_options(snapshot.environment, store);
        Self {
            shell,
            origin,
            walk: gs::summon_walk(snapshot.environment),
            cosmic: snapshot.environment == gs::Environment::Cosmic,
            configure_for_me: options.configure_for_me,
            step: Step::SummonKey,
            dismiss_checked: false,
            not_found: false,
            capture_armed: false,
            seat_error: None,
            focus: cx.focus_handle(),
        }
    }

    /// The summon-key ask: the intro, the walk, the deep link, the
    /// check, and the origin's footer.
    fn render_ask(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let check_shell = self.shell.clone();
        let check = cx.listener(
            move |view: &mut Self,
                  _: &gpui::ClickEvent,
                  _window: &mut Window,
                  cx: &mut Context<Self>| {
                // The same re-read the boot decision and the Global
                // row read: found chords advance to the confirmation
                // and refresh the Global row's display in the same
                // pass; nothing found leaves the ask standing with
                // the not-found line.
                let snapshot = load_summon_snapshot();
                let found = (!snapshot.display.is_empty()).then(|| snapshot.display.join(" / "));
                check_shell.update(cx, |shell, _| shell.settings.summon = snapshot);
                view.not_found = found.is_none();
                if let Some(step) = view.step.next(found) {
                    view.step = step;
                }
            },
        );
        let mut ask = v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .p_4()
            .gap_2()
            .child(gpui::div().text_xl().child("Set a summon key"))
            .child(
                gpui::div()
                    .text_color(cx.theme().muted_foreground)
                    .child("DropTerm is summoned with a global key — a chord that works while the terminal is hidden."),
            );
        for step in self.walk {
            ask = ask.child(
                gpui::div()
                    .text_size(px(13.))
                    .text_color(cx.theme().muted_foreground)
                    .child(*step),
            );
        }
        if self.cosmic {
            ask = ask.child(small_button(
                "onboarding-open-keyboard",
                "Open keyboard settings",
                |_, _| {
                    if let Err(err) = std::process::Command::new("cosmic-settings")
                        .arg("keyboard")
                        .spawn()
                    {
                        eprintln!("dropterm: cosmic-settings spawn failed: {err}");
                    }
                },
            ));
            // The store is COSMIC's live seam; off-COSMIC the check
            // could never find anything, so the button stays a real
            // control.
            ask = ask.child(
                Button::new("onboarding-check-again")
                    .label("Check again")
                    .small()
                    .secondary()
                    .on_click(check),
            );
        }
        if self.not_found {
            ask = ask.child(
                gpui::div()
                    .text_size(px(13.))
                    .text_color(cx.theme().muted_foreground)
                    .child("No summon key was found yet."),
            );
        }
        if let Some(seat) = self.render_seat(cx) {
            ask = ask.child(seat);
        }
        ask.child(self.render_footer(cx))
    }

    /// The "Configure for me" seat (the writer's contents): the two
    /// buttons, the armed capture's hint, and the inline error —
    /// rendered only while the arm is on, never a dead control.
    fn render_seat(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if !self.configure_for_me {
            return None;
        }
        let use_f12 = cx.listener(
            move |view: &mut Self,
                  _: &gpui::ClickEvent,
                  window: &mut Window,
                  cx: &mut Context<Self>| {
                // Yakuake's assign-F12 consent model: the default
                // chord, one press away.
                match gs::captured_chord(gs::Modifiers::new(), "f12") {
                    gs::CapturedChord::Binding(binding) => {
                        view.commit_chord(&binding, window, cx);
                    }
                    _ => {
                        view.seat_error =
                            Some("F12 could not be read; choose another key.".to_string());
                    }
                }
            },
        );
        let arm = cx.listener(
            move |view: &mut Self,
                  _: &gpui::ClickEvent,
                  window: &mut Window,
                  cx: &mut Context<Self>| {
                view.capture_armed = true;
                view.seat_error = None;
                view.focus.focus(window, cx);
            },
        );
        let mut seat = v_flex().gap_2().child(
            h_flex()
                .gap_2()
                .flex_wrap()
                .child(
                    Button::new("onboarding-use-f12")
                        .label("Use F12")
                        .small()
                        .primary()
                        .on_click(use_f12),
                )
                .child(
                    Button::new("onboarding-choose-key")
                        .label(if self.capture_armed {
                            "Press a key… (Esc cancels)"
                        } else {
                            "Choose another key…"
                        })
                        .small()
                        .secondary()
                        .on_click(arm),
                ),
        );
        if self.capture_armed {
            seat = seat.child(
                gpui::div()
                    .text_size(px(13.))
                    .text_color(cx.theme().muted_foreground)
                    .child("Press the chord to summon DropTerm with."),
            );
        }
        if let Some(error) = &self.seat_error {
            seat = seat.child(
                gpui::div()
                    .text_size(px(13.))
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        Some(seat.into_any_element())
    }

    /// The seat's whole commit path, from a captured chord to the
    /// confirmation: the verdict over the effective map, the write
    /// through the seam, the snapshot refresh, the step. Nothing
    /// writes except on this path's button presses.
    fn commit_chord(
        &mut self,
        binding: &gs::Binding,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.capture_armed = false;
        self.seat_error = None;
        // The session's own stem and binary: recognition keys off the
        // stem, the marker command off the path.
        let Some(exe_stem) = gs::own_exe_stem() else {
            self.seat_error =
                Some("DropTerm could not locate its own binary. Use Set up manually.".to_string());
            cx.notify();
            return;
        };
        let Ok(exe) = std::env::current_exe() else {
            self.seat_error =
                Some("DropTerm could not locate its own binary. Use Set up manually.".to_string());
            cx.notify();
            return;
        };
        let command = gs::marker_spawn_command(&exe);
        let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
            self.seat_error = Some("The home directory could not be read.".to_string());
            cx.notify();
            return;
        };
        let store_path = gs::custom_shortcuts_path(&home);

        // The verdict reads the effective map: the system defaults
        // overridden by the store as it stands (a broken store here
        // only degrades the answer's inputs — the seam's own fresh
        // read gates the write).
        let store = gs::read_custom(&store_path);
        let empty = gs::CustomShortcuts::default();
        let custom = match &store {
            gs::ReadCustom::Loaded(custom) => custom,
            _ => &empty,
        };
        let effective = gs::effective_shortcuts(&gs::load_defaults(&gs::defaults_paths()), custom);
        match gs::capture_verdict(binding, &effective, &exe_stem) {
            gs::CaptureVerdict::Commit => {}
            gs::CaptureVerdict::Own => {
                // Already the writer's own marker: straight to the
                // confirmation, nothing writes.
                self.finish(binding, cx);
                return;
            }
            gs::CaptureVerdict::Conflict(holder) => {
                self.seat_error = Some(format!("{holder} already uses this key."));
                cx.notify();
                return;
            }
            gs::CaptureVerdict::Unsupported => {
                self.seat_error = Some(
                    "That key cannot summon DropTerm on its own. Add a modifier, or pick a key like F12."
                        .to_string(),
                );
                cx.notify();
                return;
            }
        }
        match gs::write_custom(&store_path, |custom| {
            gs::commit_plan(custom, binding, &exe_stem, &command)
        }) {
            gs::WriteOutcome::Written(_) => self.finish(binding, cx),
            gs::WriteOutcome::Refused(reason) => {
                eprintln!("dropterm: summon write refused: {reason:?}");
                self.seat_error = Some(
                    "The shortcuts store refused the change, so nothing was written.".to_string(),
                );
            }
            gs::WriteOutcome::Io(err) => {
                self.seat_error = Some(format!("Writing the shortcut failed: {err}"));
            }
        }
        cx.notify();
    }

    /// The commit's landing: the Global row's snapshot refresh and
    /// the confirmation step with the chord's display form.
    fn finish(&mut self, binding: &gs::Binding, cx: &mut Context<Self>) {
        let snapshot = load_summon_snapshot();
        let chord = gs::binding_display(binding);
        self.shell
            .update(cx, |shell, _| shell.settings.summon = snapshot);
        self.step = Step::Confirmed(chord);
        cx.notify();
    }

    /// The origin's footer: first run carries the retire contract —
    /// the box mirrors itself into the config on every flip through
    /// the settings write path (a tick survives even a crash, and an
    /// un-tick before closing really does re-ask) — plus Skip;
    /// settings origin closes, persisting nothing.
    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let retire_shell = self.shell.clone();
        let retire = cx.listener(
            move |view: &mut Self, checked: &bool, _window: &mut Window, cx: &mut Context<Self>| {
                view.dismiss_checked = *checked;
                retire_shell.update(cx, |shell, _| {
                    shell.write_settings(&PlannedWrite::OnboardingDismissed(*checked));
                });
            },
        );
        match self.origin {
            Origin::FirstRun => h_flex()
                .gap_2()
                .justify_between()
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Switch::new("onboarding-dismiss")
                                .checked(self.dismiss_checked)
                                .on_click(retire),
                        )
                        .child(gpui::div().text_size(px(13.)).child("Don't ask again")),
                )
                .child(small_button("onboarding-skip", "Skip", |window, _| {
                    window.remove_window();
                })),
            Origin::Settings => h_flex().justify_end().child(small_button(
                "onboarding-close",
                "Close",
                |window, _| {
                    window.remove_window();
                },
            )),
        }
    }

    /// The confirmation landing: the chord read back, Finish closing.
    fn render_confirmed(chord: &str, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .p_4()
            .gap_2()
            .child(gpui::div().text_xl().child("You're set"))
            .child(
                gpui::div()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("DropTerm is summoned with {chord}.")),
            )
            .child(
                Button::new("onboarding-finish")
                    .label("Finish")
                    .small()
                    .primary()
                    .on_click(|_, window, _| window.remove_window()),
            )
    }
}

/// The window shape the shipped dialog and the About window share:
/// a plain toplevel (`WindowKind::Normal`, app id set) at a modest
/// size. A failed open logs and drops — the flow is guidance, never
/// fatal, and the Global row stays available in the drawer either
/// way; closing it can never quit the daemon (`QuitMode::Explicit`
/// holds).
pub(crate) fn open_onboarding(cx: &mut App, shell: gpui::Entity<Shell>, origin: Origin) {
    let snapshot = shell.read(cx).settings.summon.clone();
    let store = read_session_store();
    let options = WindowOptions {
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(gpui::Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(440.), px(340.)),
        })),
        app_id: Some(APP_ID.to_string()),
        kind: WindowKind::Normal,
        ..Default::default()
    };
    let view = cx.new(|cx| OnboardingView::new(shell, origin, &snapshot, &store, cx));
    if let Err(err) = cx.open_window(options, |_, _| view.clone()) {
        eprintln!("dropterm: onboarding open failed: {err:?}");
    }
}

/// The session's custom-shortcuts store as it stands right now — the
/// `setup_options` input. Absent off-COSMIC (the file is COSMIC's);
/// every degrade answers guidance, never an error surface.
fn read_session_store() -> gs::ReadCustom {
    match std::env::var_os("HOME").map(std::path::PathBuf::from) {
        Some(home) => gs::read_custom(&gs::custom_shortcuts_path(&home)),
        None => gs::ReadCustom::Absent,
    }
}

/// `drawer.rs`'s secondary small button, local to this module: the
/// flow's guidance verbs (Skip, Close, the deep link).
fn small_button(
    id: impl Into<gpui::ElementId>,
    label: &str,
    on_press: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .label(label)
        .small()
        .secondary()
        .on_click(move |_: &gpui::ClickEvent, window, cx| on_press(window, cx))
}

impl Render for OnboardingView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The keyboard anchor: the window's keys route through the
        // root's handler, which only acts while the seat's capture is
        // armed. The focus lands once here — the render is the one
        // place that already reseeds this window's state.
        if !self.focus.is_focused(window) {
            self.focus.focus(window, cx);
        }
        let step = match self.step.clone() {
            Step::SummonKey => self.render_ask(cx).into_any_element(),
            Step::Confirmed(chord) => Self::render_confirmed(&chord, cx).into_any_element(),
        };
        v_flex()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .child(step)
    }
}

impl OnboardingView {
    /// The armed capture's key press: Escape cancels, a bare
    /// modifier keeps the capture armed (the keyboard page's own
    /// listener behavior), a chord that will not resolve errors, and
    /// anything else takes the verdict path.
    fn on_key(&mut self, event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.capture_armed {
            return;
        }
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" {
            self.capture_armed = false;
            cx.notify();
            return;
        }
        let modifiers = gs::Modifiers {
            ctrl: keystroke.modifiers.control,
            alt: keystroke.modifiers.alt,
            shift: keystroke.modifiers.shift,
            logo: keystroke.modifiers.platform,
        };
        match gs::captured_chord(modifiers, &keystroke.key) {
            gs::CapturedChord::Binding(binding) => self.commit_chord(&binding, window, cx),
            // Still arming: the press was only a modifier.
            gs::CapturedChord::BareModifier => {}
            gs::CapturedChord::Unresolvable => {
                self.capture_armed = false;
                self.seat_error = Some(format!(
                    "'{}' cannot be used as a summon key.",
                    keystroke.key
                ));
                cx.notify();
            }
        }
    }
}
