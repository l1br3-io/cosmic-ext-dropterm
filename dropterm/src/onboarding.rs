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
    /// seat renders only while this is true (false everywhere today —
    /// the writer's W3 turns the COSMIC arm on).
    configure_for_me: bool,
    step: Step,
    dismiss_checked: bool,
    /// A "Check again" that found nothing leaves the line standing
    /// under the walk.
    not_found: bool,
}

impl OnboardingView {
    fn new(
        shell: gpui::Entity<Shell>,
        origin: Origin,
        snapshot: &SummonSnapshot,
        store: &gs::ReadCustom,
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
        // The "Configure for me" seat (the writer module's contents,
        // its own slice): rendered only while the arm is on, never a
        // dead control. The seat stands empty until the writer's W3
        // replaces it with the writer's buttons.
        if self.configure_for_me {
            // (The writer's slice lands here.)
        }
        ask.child(self.render_footer(cx))
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
    let view = cx.new(|_| OnboardingView::new(shell, origin, &snapshot, &store));
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.step.clone() {
            Step::SummonKey => self.render_ask(cx).into_any_element(),
            Step::Confirmed(chord) => Self::render_confirmed(&chord, cx).into_any_element(),
        }
    }
}
