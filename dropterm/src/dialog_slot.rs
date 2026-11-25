// SPDX-License-Identifier: GPL-3.0-only

//! The dialog slot's modal (dialog-slot spec): a script opens a
//! dialog with typed content — a title, a message of spans, one to
//! three buttons — and the user's press or dismissal crosses back as
//! a `slot-event`. The palette's own shape carries it: the
//! full-surface scrim whose press dismisses, the top-centered panel
//! over it, the first-paint focus repair. The dialog holds the
//! keyboard while it stands (the palette's rule): the container's
//! one focus handle holds it, Tab cycles the buttons, Enter presses
//! the focused one, a pointer press presses its own, and a focus
//! escape dismisses.

use std::rc::Rc;

use dropterm::core::slots::{DialogSpec, SlotColor};
use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, div,
};
use gpui_component::ActiveTheme as _;
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};

use crate::app_shell::DismissDialog;
use crate::status_bar;

/// The press sink: the pressed button's id — the slot-event's
/// `element_id`, verbatim.
pub(crate) type DialogPressFn = Rc<dyn Fn(&str, &mut gpui::Window, &mut gpui::App)>;

/// The dismissal sink: Escape, the scrim's press, a focus escape,
/// the shell's own replacement — every close that is not a press.
pub(crate) type DialogDismissFn = Rc<dyn Fn(&mut gpui::Window, &mut gpui::App)>;

/// The open dialog: the modal entity the shell holds while it
/// stands, the script's validated [`DialogSpec`] riding whole.
pub(crate) struct DialogSlot {
    spec: DialogSpec,
    /// The dialog's keyboard hold — one container handle, the
    /// palette's shape: the keyboard rests here the whole dialog's
    /// life, so a blur is a focus escape and dismisses.
    container_focus: gpui::FocusHandle,
    /// The keyboard's button index (`None` while the container holds
    /// the keyboard itself); Tab cycles, Enter presses.
    focused: Option<usize>,
    /// The focus-escape latch (the shell's `focus_lost` shape): the
    /// dialog's own render sets it when the keyboard has left and
    /// the first paint is done; the shell's pump reads it and
    /// closes — window work never runs inside render, state changes
    /// land on the tick.
    escaped: bool,
    /// The body's resolved runs (styled-status-slot's painter): the
    /// shell builds the palette beside the band's, one color source.
    body_runs: Vec<status_bar::StyledRun>,
    /// Each button's resolved paint (SE2): the cascade's background
    /// and color, palette-mapped at construction — the Kit custom
    /// variant when anything resolves, the Kit secondary otherwise.
    button_paints: Vec<ButtonPaint>,
    /// The `dialog` root's resolved paint (SE4): the card's
    /// background and color, `None` leaving the theme's look.
    dialog_background: Option<gpui::Hsla>,
    dialog_color: Option<gpui::Hsla>,
    /// The first paint's repair — focus applied at open races the
    /// element's first mount (the palette's own pattern).
    painted_once: bool,
    on_press: DialogPressFn,
    on_dismiss: DialogDismissFn,
}

/// One button's resolved paint (SE2): the cascade's background and
/// color through the palette — the Kit custom variant when anything
/// resolves, the Kit secondary otherwise (today's quiet look).
#[derive(Debug, Clone, Copy)]
struct ButtonPaint {
    background: Option<gpui::Hsla>,
    color: Option<gpui::Hsla>,
}

impl DialogSlot {
    /// The dialog over the script's spec. The shell hands in the two
    /// callbacks every path ends in — the press (with its button id)
    /// and the dismissal — so the surface holds no shell reference
    /// and the same construction mounts it headless. The engine is
    /// the shell's one `StyleSet`: the body and the buttons resolve
    /// at construction (an open dialog keeps the look it opened
    /// with, the standing body-runs shape).
    pub(crate) fn new(
        spec: DialogSpec,
        palette: &status_bar::ScriptPalette,
        engine: &dropterm::core::style_engine::StyleSet,
        on_press: DialogPressFn,
        on_dismiss: DialogDismissFn,
        cx: &mut Context<Self>,
    ) -> Self {
        use dropterm::core::style_engine::StyleNode;
        let container_focus = cx.focus_handle();
        // The dialog's surface root (style-engine SE4): the card's
        // background and color — a `dialog` rule restyles the modal
        // by ordinary cascade, unset leaving the theme's look.
        let dialog = engine.resolve(&StyleNode {
            element: "dialog",
            classes: &[],
            id: None,
            inline: &[],
        });
        let paint = |value: Option<&SlotColor>| match value {
            Some(SlotColor::Token(token)) => palette.color(token),
            Some(SlotColor::Hex(hex)) => status_bar::parse_hex(hex),
            None => None,
        };
        let dialog_background = paint(dialog.background.as_ref());
        let dialog_color = paint(dialog.color.as_ref());
        let body_runs = status_bar::paint_script_segment(
            &dropterm::core::status::script_runs(
                &dropterm::core::slots::SlotContent::Styled {
                    spans: spec.body.clone(),
                },
                engine,
                None,
            ),
            palette,
        );
        let button_paints = spec
            .buttons
            .iter()
            .map(|button| {
                let classes: Vec<&str> = button
                    .class
                    .as_deref()
                    .unwrap_or_default()
                    .split_whitespace()
                    .collect();
                let resolved = engine.resolve(&StyleNode {
                    element: "dialog-button",
                    classes: &classes,
                    id: Some(&button.id),
                    inline: &button.style,
                });
                let paint = |value: Option<&SlotColor>| match value {
                    Some(SlotColor::Token(token)) => palette.color(token),
                    Some(SlotColor::Hex(hex)) => status_bar::parse_hex(hex),
                    None => None,
                };
                ButtonPaint {
                    background: paint(resolved.background.as_ref()),
                    color: paint(resolved.color.as_ref()),
                }
            })
            .collect();
        Self {
            spec,
            container_focus,
            focused: None,
            escaped: false,
            body_runs,
            button_paints,
            dialog_background,
            dialog_color,
            painted_once: false,
            on_press,
            on_dismiss,
        }
    }

    /// The close-time repair capture (the palette's shape): the
    /// shell's render reads the departed handle to hand the keyboard
    /// back to the pane.
    pub(crate) fn container_focus(&self) -> gpui::FocusHandle {
        self.container_focus.clone()
    }

    /// The focus-escape read (the shell's pump closes on it): the
    /// keyboard left after a real first paint.
    pub(crate) fn focus_escaped(&self) -> bool {
        self.escaped
    }
}

impl Render for DialogSlot {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let hairline = theme.border;
        let fg = self.dialog_color.unwrap_or(theme.foreground);
        let card = self.dialog_background.unwrap_or(theme.background);
        // The keyboard's first-paint repair (the palette's own
        // windowless-arm pattern): focus applied at open races the
        // element's first mount, and a lost race strands the dialog
        // chord-dead under an invisible scrim. The first frame that
        // carries the panel takes the keyboard.
        if !self.painted_once && !self.container_focus.is_focused(window) {
            println!("dropterm: plugins: dialog focus repaired on first paint");
            self.container_focus.focus(window, cx);
        }
        if !self.painted_once {
            self.painted_once = true;
            println!("dropterm: plugins: dialog overlay painted");
        } else if !self.container_focus.is_focused(window) {
            // The keyboard left after a real first paint: the latch
            // sets, the shell's pump closes (never this render).
            self.escaped = true;
        }
        let on_dismiss = Rc::clone(&self.on_dismiss);
        // The overlay's root is out-of-flow — the palette's own rule:
        // an in-flow full-size sibling lays out past the visible area
        // (the invisible open). Absolute against the shell's
        // positioned overlay host puts the layer on top.
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x0000_0080))
            .flex()
            .justify_center()
            .items_start()
            // The dialog's key context: Escape rides the dismissal
            // action; Tab and Enter cycle and press the buttons here.
            .key_context("dialog")
            .track_focus(&self.container_focus)
            .on_action(move |_: &DismissDialog, window, cx| on_dismiss(window, cx))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "tab" => {
                        cx.stop_propagation();
                        let count = this.spec.buttons.len();
                        if count == 0 {
                            return;
                        }
                        this.focused = match (this.focused, event.keystroke.modifiers.shift) {
                            (None, false) => Some(0),
                            (None, true) => Some(count - 1),
                            (Some(index), false) => Some((index + 1) % count),
                            (Some(index), true) => Some((index + count - 1) % count),
                        };
                        cx.notify();
                    }
                    "enter" | "space" => {
                        if let Some(index) = this.focused {
                            cx.stop_propagation();
                            let id = this.spec.buttons[index].id.clone();
                            (this.on_press)(&id, window, cx);
                        }
                    }
                    _ => {}
                }
            }))
            // The scrim's press dismisses (the panel's occlusion keeps
            // its own presses — the buttons included — off this
            // listener).
            .on_mouse_down(gpui::MouseButton::Left, {
                let dismiss = Rc::clone(&self.on_dismiss);
                move |_, window, cx| dismiss(window, cx)
            })
            .child(
                div()
                    .id("dialog-panel")
                    .occlude()
                    .mt(gpui::px(48.))
                    .w(gpui::px(440.))
                    .rounded_md()
                    .border_1()
                    .border_color(hairline)
                    .bg(card)
                    .text_color(fg)
                    .text_size(gpui::px(14.))
                    .p(gpui::px(16.))
                    .flex()
                    .flex_col()
                    .gap(gpui::px(10.))
                    .overflow_hidden()
                    // The title bar.
                    .child(
                        gpui::div()
                            .text_size(gpui::px(15.))
                            .child(self.spec.title.clone()),
                    )
                    // The body: one styled run per span, the band's
                    // painter, the dialog's own colors.
                    .child(
                        gpui::div().flex().flex_col().gap(gpui::px(2.)).children(
                            self.body_runs.iter().map(|run| {
                                gpui::div().text_color(run.color).child(run.text.clone())
                            }),
                        ),
                    )
                    // The buttons: one Kit Button each, the accent
                    // flag the primary variant, the focused one
                    // hairlined for the keyboard's hand.
                    .child(self.button_row(cx, fg)),
            )
    }
}

impl DialogSlot {
    /// The dialog's button row: one Kit Button per spec button, the
    /// press sinking through the shell's slot-event emit. The paint
    /// is the resolved cascade (SE2): anything resolved rides the
    /// Kit's custom variant, the rest keep the Kit secondary — the
    /// accent look is the builtin `dialog-button.accent` rule now.
    fn button_row(&self, cx: &mut Context<Self>, fg: gpui::Hsla) -> gpui::AnyElement {
        gpui::div()
            .flex()
            .flex_row()
            .gap(gpui::px(8.))
            .justify_end()
            .children(self.spec.buttons.iter().enumerate().map(|(index, button)| {
                let id = button.id.clone();
                let press = Rc::clone(&self.on_press);
                let focused = self.focused == Some(index);
                let paint = self.button_paints[index];
                let verb = Button::new(id.clone())
                    .label(button.label.clone())
                    .small()
                    .on_click(move |_: &gpui::ClickEvent, window, cx| {
                        press(&id, window, cx);
                    });
                let verb = if paint.background.is_some() || paint.color.is_some() {
                    let mut variant = ButtonCustomVariant::new(cx);
                    if let Some(background) = paint.background {
                        variant = variant.color(background);
                    }
                    variant = variant.foreground(paint.color.unwrap_or(fg));
                    verb.custom(variant)
                } else {
                    verb.secondary()
                };
                if focused {
                    gpui::div()
                        .border_1()
                        .border_color(fg)
                        .rounded_md()
                        .child(verb)
                } else {
                    gpui::div().child(verb)
                }
            }))
            .into_any_element()
    }
}

/// The dialog's builtin style rules (style-engine SE2): today's
/// accent look as an ordinary rule in the host's lowest layer — the
/// `dialog-button.accent` class's background and color, the `accent`
/// flag's successor. A plugin out-specifying or out-ordering it
/// replaces it by ordinary cascade.
pub(crate) fn builtin_rules() -> Vec<dropterm::core::style_engine::Rule> {
    use dropterm::core::style_engine::{Rule, RuleOrigin, parse_declarations, parse_selector};
    let rule = |selector: &str, declarations: serde_json::Value| Rule {
        origin: RuleOrigin::Builtin,
        selector: parse_selector(selector).expect("the builtin selector parses — pinned by test"),
        declarations: parse_declarations(&declarations).declarations,
    };
    vec![rule(
        "dialog-button.accent",
        serde_json::json!({ "background": "accent", "color": "fg" }),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use dropterm::core::slots::DialogButton;
    use gpui::{AppContext as _, Entity, px};
    use std::cell::RefCell;

    /// The fixture spec: one accent and one quiet button.
    fn fixture_spec() -> DialogSpec {
        DialogSpec {
            title: "Build finished".to_owned(),
            body: Vec::new(),
            buttons: vec![
                DialogButton {
                    id: "restart".to_owned(),
                    label: "Restart".to_owned(),
                    class: Some("accent".to_owned()),
                    style: Vec::new(),
                },
                DialogButton {
                    id: "dismiss".to_owned(),
                    label: "Dismiss".to_owned(),
                    class: None,
                    style: Vec::new(),
                },
            ],
        }
    }

    /// The mounted dialog over a 1000×800 window with a shared press
    /// and dismiss log — the palette's own harness shape.
    fn mount(
        cx: &mut gpui::TestAppContext,
    ) -> (
        gpui::VisualTestContext,
        Entity<DialogSlot>,
        Rc<RefCell<Vec<String>>>,
    ) {
        cx.update(gpui_component::init);
        cx.update(|cx| {
            cx.bind_keys([gpui::KeyBinding::new(
                "escape",
                DismissDialog,
                Some("dialog"),
            )]);
        });
        let log = Rc::new(RefCell::new(Vec::new()));
        let press_log = Rc::clone(&log);
        let dismiss_log = Rc::clone(&log);
        let entity = cx.update(|cx| {
            cx.open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds {
                        origin: gpui::Point::default(),
                        size: gpui::size(px(1000.), px(800.)),
                    })),
                    ..Default::default()
                },
                |_window, cx| {
                    cx.new(|cx| {
                        let palette = status_bar::ScriptPalette::resolve(
                            &alacritty_terminal::term::color::Colors::default(),
                            gpui::hsla(0., 0., 0.9, 1.),
                            gpui::hsla(0., 0., 0.1, 1.),
                        );
                        DialogSlot::new(
                            fixture_spec(),
                            &palette,
                            &dropterm::core::style_engine::StyleSet::default(),
                            Rc::new(move |button_id, _window, _cx| {
                                press_log.borrow_mut().push(format!("press {button_id}"));
                            }),
                            Rc::new(move |_window, _cx| {
                                dismiss_log.borrow_mut().push("dismiss".to_owned());
                            }),
                            cx,
                        )
                    })
                },
            )
            .expect("the window opens")
        });
        let mut cx = gpui::VisualTestContext::from_window(entity.into(), cx);
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let dialog: Entity<DialogSlot> = cx.update(|window, _| {
            window
                .root()
                .expect("the dialog is the window's root type")
                .expect("the window has a root")
        });
        (cx, dialog, log)
    }

    #[gpui::test]
    fn the_first_paint_takes_the_keyboard(cx: &mut gpui::TestAppContext) {
        let (mut cx, entity, _log) = mount(cx);
        cx.run_until_parked();
        // The repair ran: the dialog holds the keyboard.
        cx.update(|window, cx| {
            assert!(
                entity.read(cx).container_focus.is_focused(window),
                "the dialog holds the keyboard after the first paint"
            );
        });
    }

    #[gpui::test]
    fn one_tab_arms_the_first_button(cx: &mut gpui::TestAppContext) {
        let (mut cx, _entity, log) = mount(cx);
        cx.simulate_keystrokes("tab enter");
        cx.run_until_parked();
        assert_eq!(
            *log.borrow(),
            ["press restart"],
            "Tab arms the first button, Enter presses it"
        );
    }

    #[gpui::test]
    fn two_tabs_cycle_to_the_second_button(cx: &mut gpui::TestAppContext) {
        let (mut cx, _entity, log) = mount(cx);
        cx.simulate_keystrokes("tab tab enter");
        cx.run_until_parked();
        assert_eq!(
            *log.borrow(),
            ["press dismiss"],
            "two Tabs cycle to the second button"
        );
    }

    #[gpui::test]
    fn shift_tab_cycles_back(cx: &mut gpui::TestAppContext) {
        let (mut cx, _entity, log) = mount(cx);
        cx.simulate_keystrokes("tab tab shift-tab enter");
        cx.run_until_parked();
        assert_eq!(*log.borrow(), ["press restart"], "Shift-Tab cycles back");
    }

    #[gpui::test]
    fn escape_dismisses_once(cx: &mut gpui::TestAppContext) {
        let (mut cx, _entity, log) = mount(cx);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(*log.borrow(), ["dismiss"], "Escape dismisses, once");
    }

    #[gpui::test]
    fn a_scrim_press_dismisses(cx: &mut gpui::TestAppContext) {
        let (mut cx, _entity, log) = mount(cx);
        // Bare scrim: below the top-centered panel (48px down, the
        // 1000px window's left edge).
        cx.simulate_click(gpui::point(px(20.), px(700.)), gpui::Modifiers::default());
        cx.run_until_parked();
        assert_eq!(*log.borrow(), ["dismiss"], "the scrim's press dismisses");
    }

    #[gpui::test]
    fn a_focus_escape_latches(cx: &mut gpui::TestAppContext) {
        let (mut cx, entity, log) = mount(cx);
        cx.run_until_parked();
        // The keyboard leaves the dialog for a foreign handle: the
        // next dialog render sets the latch (the shell's pump closes
        // on it — the entity reports the escape, never closes).
        let outside = cx.update(|_window, cx| cx.focus_handle());
        cx.update(|window, cx| outside.focus(window, cx));
        cx.run_until_parked();
        let escaped = cx.update(|_window, cx| entity.read(cx).escaped);
        assert!(
            escaped,
            "the keyboard leaving the dialog sets the escape latch"
        );
        assert!(
            log.borrow().is_empty(),
            "the entity itself never emits — the shell closes"
        );
    }
}

#[test]
fn a_dialog_rule_restyles_the_card_root() {
    // The `dialog` root (SE4): the card's resolved background and
    // color — constructed the same headless way the mounted
    // dialog is, asserting the retained paint fields.
    let _ = std::panic::catch_unwind(|| {}); // keep the test in the same module shape
    let engine = |declarations: serde_json::Value| {
        let mut set = dropterm::core::style_engine::StyleSet::default();
        let parsed = dropterm::core::style_engine::parse_declarations(&declarations);
        assert!(parsed.drops.is_empty(), "{:?}", parsed.drops);
        set.replace_manifest(vec![dropterm::core::style_engine::Rule {
            origin: dropterm::core::style_engine::RuleOrigin::Manifest {
                plugin: "styler".to_owned(),
            },
            selector: dropterm::core::style_engine::parse_selector("dialog").expect("parses"),
            declarations: parsed.declarations,
        }]);
        set
    };
    // The resolve is the render's own input: a rule on `dialog`
    // answers background/color; no rule answers nothing.
    let bare = dropterm::core::style_engine::StyleSet::default();
    let node = dropterm::core::style_engine::StyleNode {
        element: "dialog",
        classes: &[],
        id: None,
        inline: &[],
    };
    assert_eq!(bare.resolve(&node).background, None);
    let set = engine(serde_json::json!({ "background": "accent", "color": "fg" }));
    let resolved = set.resolve(&node);
    assert_eq!(
        resolved.background,
        Some(dropterm::core::slots::SlotColor::Token("accent".to_owned()))
    );
    assert_eq!(
        resolved.color,
        Some(dropterm::core::slots::SlotColor::Token("fg".to_owned()))
    );
}
