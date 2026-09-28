// SPDX-License-Identifier: GPL-3.0-only

//! The tab bar widget (tabs spec, T5): one button per tab — title plus a
//! close `×` — shown only while more than one tab exists.
//!
//! The button layout and hit-testing are pure functions over plain
//! geometry (kept free of any windowing state, like the list operations in
//! [`crate::tabs`]), so the behavior that decides which tab a click or
//! middle-click lands on is unit-testable headless; rendering itself needs
//! real COSMIC hardware and is covered by the README manual checklist.
//!
//! Inline rename (tabs spec, T6): a double-clicked tab body swaps its
//! button for a real text input, hosted as an overlay exactly over that
//! button — the delegation pattern of libcosmic's own popover widget
//! (pinned rev `08a09e3`, `widget/popover.rs`). The input is built by the
//! app (which knows its messages) and passed in via [`TabBar::renaming`];
//! the bar only positions it, classifies the double-click, and reports
//! presses outside the input so the app can end the session.
//!
//! Drag reorder (tabs spec, T7): a left press on a tab body that moves
//! past a small threshold drags the tab horizontally. iced has no
//! first-class drag API, so the bar tracks press/move/release itself —
//! the state machine of iced's own text editor (pinned rev `08a09e3`,
//! `widget/text_editor.rs`). [`drop_index`] and [`indicator_x`] decide
//! from plain geometry where a drag would land; the release reports the
//! drop, which the app applies via [`crate::tabs::reorder_tabs`].
//!
//! Colors mirror the `TabBar` preset of libcosmic's own
//! `theme/style/segmented_button.rs` (pinned rev `08a09e3`): the active
//! tab is `neutral_5` at 0.2 alpha with the accent text color and
//! `radius_s` corners, inactive tabs carry no background and use the
//! component's `on` text color.

use std::collections::HashMap;

use cosmic::iced::core::alignment;
use cosmic::iced::core::mouse::click;
use cosmic::iced::core::overlay;
use cosmic::iced::core::text::{Ellipsize, LineHeight, Shaping, Wrapping, paragraph};
use cosmic::iced::core::{
    Border, Shell, Text,
    clipboard::Clipboard,
    layout::{self, Layout},
    renderer::{self, Quad, Renderer as _},
    text::Renderer as _,
    widget::{Operation, Tree, Widget, tree},
};
use cosmic::iced::{
    Color, Length, Pixels, Point, Rectangle, Size, Vector,
    event::Event,
    mouse::{self, Button, Event as MouseEvent},
};
use cosmic::theme::Theme;
use cosmic::{Element, Renderer, cosmic_theme::palette::WithAlpha};

use crate::terminal::WINDOW_BG_COLOR;

use crate::tabs::{Tab, TabId};

/// A cached title paragraph, one per tab, keyed by [`TabId`].
///
/// The bar's background and a text color readable on it.
///
/// The strip is painted with the terminal pane's effective background
/// ([`WINDOW_BG_COLOR`]) instead of relying on the window clear color:
/// the two can disagree when the COSMIC system theme and the terminal
/// color scheme diverge, which left the bar as an unreadable foreign
/// strip that users read as window chrome rather than tabs. Text picks
/// near-black or near-white by the background's luminance so labels and
/// close glyphs stay readable in either scheme.
fn pane_colors() -> (Color, Color) {
    let packed = WINDOW_BG_COLOR.load(std::sync::atomic::Ordering::SeqCst);
    let background = Color::from_rgba8(
        ((packed >> 16) & 0xFF) as u8,
        ((packed >> 8) & 0xFF) as u8,
        (packed & 0xFF) as u8,
        1.0,
    );
    // Slightly darken the pane color for the strip so the bar reads as
    // its own band instead of blending into the terminal area.
    let strip = Color {
        r: background.r * 0.92,
        g: background.g * 0.92,
        b: background.b * 0.92,
        a: 1.0,
    };
    (strip, readable_foreground(background))
}

/// Near-black or near-white text for `background`, by relative
/// luminance (WCAG definition).
fn readable_foreground(background: Color) -> Color {
    fn channel(c: f32) -> f32 {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let luminance = 0.2126 * channel(background.r)
        + 0.7152 * channel(background.g)
        + 0.0722 * channel(background.b);
    if luminance > 0.5 {
        Color::from_rgba8(0x1B, 0x1B, 0x22, 1.0)
    } else {
        Color::from_rgba8(0xF5, 0xF5, 0xFF, 1.0)
    }
}

/// Named through the iced core because libcosmic keeps its own `Plain`
/// alias private (pinned rev `08a09e3`).
type TitleParagraph = paragraph::Plain<<Renderer as cosmic::iced::core::text::Renderer>::Paragraph>;

/// Bar geometry in logical pixels. Constants rather than theme values so
/// the pure layout functions stay deterministic and testable; the bar is a
/// slim Yakuake-style strip rather than a document-window tab bar.
struct BarMetrics {
    /// Height of the whole bar (and of every tab button).
    bar_height: f32,
    /// Narrowest a button may get, even under squeeze or with an empty
    /// title.
    min_button_width: f32,
    /// Widest a button may get, no matter how long the title is (longer
    /// titles clip at the button edge).
    max_button_width: f32,
    /// Horizontal padding inside a button, on each side.
    button_padding_x: f32,
    /// Gap between consecutive buttons.
    button_gap: f32,
    /// Side length of the square close-button target.
    close_size: f32,
    /// Gap between the title text and the close button.
    text_gap: f32,
}

const METRICS: BarMetrics = BarMetrics {
    bar_height: 32.0,
    min_button_width: 48.0,
    max_button_width: 200.0,
    button_padding_x: 10.0,
    button_gap: 4.0,
    close_size: 24.0,
    text_gap: 6.0,
};

/// The bar's height in logical pixels, for callers that must reserve room
/// for it outside the widget tree (the app grows the layer surface by this
/// much while the bar is visible — see `surface::surface_height`).
pub const BAR_HEIGHT: f32 = METRICS.bar_height;

/// Title text size in logical pixels.
const TITLE_FONT_SIZE: f32 = 14.0;

/// How far the cursor must travel horizontally from the press before a
/// held press on a tab body counts as a drag (T7) rather than a click.
/// Matches the movement threshold of iced's own click classification
/// (pinned rev `08a09e3`, `mouse/click.rs`: `distance < 6.0`).
const DRAG_THRESHOLD: f32 = 6.0;

/// Width of the drop indicator line, in logical pixels.
const DROP_INDICATOR_WIDTH: f32 = 2.0;

/// Vertical inset of the drop indicator from the bar's top and bottom.
const DROP_INDICATOR_INSET: f32 = 4.0;

/// The rendered geometry of one tab button.
struct ButtonGeometry {
    /// The whole button: background, click target, middle-click target.
    bounds: Rectangle<f32>,
    /// The close (`×`) target, anchored inside `bounds`' right edge.
    close: Rectangle<f32>,
}

/// Which part of a tab button a point landed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HitTarget {
    /// The button body (left-click: select, middle-click: close).
    Tab,
    /// The close button (left-click: close).
    Close,
}

/// Lays out one rectangle per title, left to right, with the close target
/// anchored at each button's right edge.
///
/// Each button takes its natural width (padding + title + gap + close,
/// clamped to [`BarMetrics::min_button_width`] and
/// [`BarMetrics::max_button_width`]). When the row would overflow the
/// available width every button is squeezed proportionally, never below the
/// minimum width — a row that still overflows at the floor (far more tabs
/// than fit) simply clips at the surface edge.
fn layout_buttons(
    available_width: f32,
    text_widths: &[f32],
    metrics: &BarMetrics,
) -> Vec<ButtonGeometry> {
    if text_widths.is_empty() {
        return Vec::new();
    }

    let natural = |text_width: f32| {
        (metrics.button_padding_x * 2.0 + text_width + metrics.text_gap + metrics.close_size)
            .clamp(metrics.min_button_width, metrics.max_button_width)
    };
    let mut widths: Vec<f32> = text_widths.iter().map(|&w| natural(w)).collect();

    // Squeeze every button by the same factor when the row overflows,
    // flooring at the minimum width so buttons stay usable.
    let gaps = metrics.button_gap * text_widths.len().saturating_sub(1) as f32;
    let budget = (available_width - gaps).max(0.0);
    let total: f32 = widths.iter().sum();
    if total > budget && total > 0.0 {
        let scale = budget / total;
        for width in &mut widths {
            *width = (*width * scale).max(metrics.min_button_width);
        }
    }

    let mut x = 0.0;
    widths
        .drain(..)
        .map(|width| {
            let bounds = Rectangle::new(Point::new(x, 0.0), Size::new(width, metrics.bar_height));
            let close = Rectangle::new(
                Point::new(
                    bounds.x + width - metrics.button_padding_x - metrics.close_size,
                    bounds.y + (metrics.bar_height - metrics.close_size) / 2.0,
                ),
                Size::new(metrics.close_size, metrics.close_size),
            );
            x += width + metrics.button_gap;
            ButtonGeometry { bounds, close }
        })
        .collect()
}

/// Returns which tab button (and which part of it) a bar-local `point`
/// lands in, or `None` outside every button (in a gap, or beyond the row).
fn hit_test(buttons: &[ButtonGeometry], point: Point) -> Option<(usize, HitTarget)> {
    buttons.iter().enumerate().find_map(|(index, button)| {
        if !button.bounds.contains(point) {
            return None;
        }
        let target = if button.close.contains(point) {
            HitTarget::Close
        } else {
            HitTarget::Tab
        };
        Some((index, target))
    })
}

/// Where a horizontal drag of the tab at `dragged` would drop if released
/// at bar-local `x`: the number of *other* buttons whose center the cursor
/// has passed. A tab must be dragged past a neighbor's center to move by
/// it, so the result is always a valid index of the reordered list.
fn drop_index(buttons: &[ButtonGeometry], dragged: usize, x: f32) -> usize {
    buttons
        .iter()
        .enumerate()
        .filter(|(index, button)| *index != dragged && button.bounds.center_x() < x)
        .count()
}

/// Bar-local x of the drop indicator for a drag of `dragged` landing at
/// slot `drop`: the left edge (minus half the gap) of the button that will
/// follow the drop slot once the dragged button leaves the row, or half a
/// gap past the row's right edge when the drag appends at the end.
fn indicator_x(buttons: &[ButtonGeometry], dragged: usize, drop: usize, gap: f32) -> f32 {
    // The row as it will look while the dragged button is gone: the
    // button at `drop` in that row follows the slot the drop creates.
    let following = buttons
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != dragged)
        .nth(drop);
    match following {
        Some((_, button)) => button.bounds.x - gap / 2.0,
        None => buttons
            .last()
            .map(|button| button.bounds.x + button.bounds.width + gap / 2.0)
            .unwrap_or(0.0),
    }
}

/// An in-progress tab drag (tabs spec, T7): the tab whose button the
/// press landed on, where that press started, and — once the press moved
/// past [`DRAG_THRESHOLD`] — the last bar-local x the cursor was seen at.
/// The state machine follows iced's own text editor (pinned rev
/// `08a09e3`): record on press, drag on move, finish on release.
struct Drag {
    tab: TabId,
    /// Bar-local press position.
    origin: Point,
    /// Whether the press has crossed the drag threshold yet.
    started: bool,
    /// Last bar-local cursor x, driving the drop indicator.
    x: f32,
}

/// Per-widget state: the title paragraphs, cached across frames and
/// rebuilt whenever a tab's title changes (measured once, then reused by
/// layout, hit-testing and drawing), plus the last tab-body click for
/// double-click detection (T6).
#[derive(Default)]
struct State {
    paragraphs: HashMap<TabId, TitleParagraph>,
    click: Option<click::Click>,
    drag: Option<Drag>,
}

/// The tab bar: one button per tab — title plus a close `×` — rendered by
/// the app only while more than one tab exists (see the tabs spec).
///
/// Interactions report the stable [`TabId`] of the tab they landed on, so
/// a tab list that changes between redraw and click cannot act on the
/// wrong tab: the app resolves the id against its current list.
pub struct TabBar<'a, Message> {
    tabs: &'a [Tab],
    /// Index into `tabs` of the active tab.
    active: usize,
    on_select: Option<Box<dyn Fn(TabId) -> Message + 'a>>,
    on_close: Option<Box<dyn Fn(TabId) -> Message + 'a>>,
    /// Reports a completed drag (T7): the dragged tab's id and the index
    /// it was dropped at in the current list.
    on_move: Option<Box<dyn Fn(TabId, usize) -> Message + 'a>>,
    on_rename: Option<Box<dyn Fn(TabId) -> Message + 'a>>,
    /// Reports a press outside the rename input while a rename is active
    /// (T6): iced inputs keep keyboard focus on outside presses, so the
    /// bar reports the click-away itself. The app maps it to a commit.
    on_rename_end: Option<Box<dyn Fn() -> Message + 'a>>,
    /// The active inline rename (T6): which tab is being renamed and the
    /// text input overlaying its button. Built by the app, positioned by
    /// the bar.
    rename: Option<(TabId, Element<'a, Message>)>,
}

impl<'a, Message> TabBar<'a, Message>
where
    Message: Clone,
{
    /// Creates a bar rendering `tabs`, highlighting the one at `active`.
    pub fn new(tabs: &'a [Tab], active: usize) -> Self {
        Self {
            tabs,
            active,
            on_select: None,
            on_close: None,
            on_move: None,
            on_rename: None,
            on_rename_end: None,
            rename: None,
        }
    }

    /// Sets the message published when a tab body is left-clicked.
    pub fn on_select(mut self, on_select: impl Fn(TabId) -> Message + 'a) -> Self {
        self.on_select = Some(Box::new(on_select));
        self
    }

    /// Sets the message published when a tab is closed: its `×` button,
    /// or a middle-click anywhere on the tab.
    pub fn on_close(mut self, on_close: impl Fn(TabId) -> Message + 'a) -> Self {
        self.on_close = Some(Box::new(on_close));
        self
    }

    /// Sets the message published when a tab drag completes (T7): the
    /// dragged tab is dropped at the given index of the current list.
    pub fn on_move(mut self, on_move: impl Fn(TabId, usize) -> Message + 'a) -> Self {
        self.on_move = Some(Box::new(on_move));
        self
    }

    /// Sets the message published when a tab body is double-clicked: the
    /// start of an inline rename (T6).
    pub fn on_rename(mut self, on_rename: impl Fn(TabId) -> Message + 'a) -> Self {
        self.on_rename = Some(Box::new(on_rename));
        self
    }

    /// Sets the message published when a press lands outside the rename
    /// input while a rename is active (T6). The press still reaches
    /// whatever it landed on.
    pub fn on_rename_end(mut self, on_rename_end: impl Fn() -> Message + 'a) -> Self {
        self.on_rename_end = Some(Box::new(on_rename_end));
        self
    }

    /// Hosts `input` as an overlay over the renamed tab's button (T6),
    /// swapping the title for a text input in place.
    pub fn renaming(mut self, tab: TabId, input: impl Into<Element<'a, Message>>) -> Self {
        self.rename = Some((tab, input.into()));
        self
    }

    /// Rebuilds the cached title paragraphs: entries for closed tabs are
    /// pruned, new or retitled tabs are (re)laid out. Returns nothing;
    /// measurement then goes through [`TitleParagraph::min_width`].
    fn refresh_paragraphs(&self, state: &mut State, renderer: &Renderer) {
        state
            .paragraphs
            .retain(|id, _| self.tabs.iter().any(|tab| tab.id == *id));

        let font = renderer.default_font();
        for tab in self.tabs {
            let text = Text {
                content: tab.title.as_str(),
                bounds: Size::INFINITE,
                size: Pixels(TITLE_FONT_SIZE),
                line_height: LineHeight::default(),
                font,
                align_x: cosmic::iced::core::text::Alignment::Left,
                align_y: alignment::Vertical::Center,
                shaping: Shaping::Advanced,
                wrapping: Wrapping::default(),
                ellipsize: Ellipsize::None,
            };
            state.paragraphs.entry(tab.id).or_default().update(text);
        }
    }

    /// The current button geometry, bar-local (origin at the bar's
    /// top-left corner).
    fn button_geometry(&self, state: &State, bounds: Rectangle) -> Vec<ButtonGeometry> {
        let widths: Vec<f32> = self
            .tabs
            .iter()
            .map(|tab| {
                state
                    .paragraphs
                    .get(&tab.id)
                    .map(|paragraph| paragraph.min_width())
                    .unwrap_or(0.0)
            })
            .collect();
        layout_buttons(bounds.width, &widths, &METRICS)
    }
}

impl<'a, Message> Widget<Message, Theme, Renderer> for TabBar<'a, Message>
where
    Message: Clone,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        // The rename input's tree, when a rename is active (T6).
        self.rename
            .as_ref()
            .map(|(_, input)| Tree::new(input.as_widget()))
            .into_iter()
            .collect()
    }

    fn diff(&mut self, tree: &mut Tree) {
        // Reconcile the input's tree with the current session state. The
        // default implementation clears all children, which would discard
        // the input's cursor and focus state on every re-render (T6).
        if let Some((_, input)) = &mut self.rename {
            let input: &mut dyn Widget<Message, Theme, Renderer> = input.as_widget_mut();
            tree.diff_children(&mut [input]);
        } else {
            tree.children.clear();
        }
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(METRICS.bar_height))
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        self.refresh_paragraphs(state, renderer);

        let limits = limits
            .width(Length::Fill)
            .height(Length::Fixed(METRICS.bar_height));
        let size = limits.resolve(
            Length::Fill,
            Length::Fixed(METRICS.bar_height),
            Size::new(limits.max().width, METRICS.bar_height),
        );
        layout::Node::new(size)
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor_position: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        // An active drag grabs the cursor wherever it roams (T7).
        if state.drag.as_ref().is_some_and(|drag| drag.started) {
            return mouse::Interaction::Grabbing;
        }
        let hit = cursor_position
            .position_in(layout.bounds())
            .and_then(|point| hit_test(&self.button_geometry(state, layout.bounds()), point));
        if hit.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::Idle
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor_position: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle<f32>,
    ) {
        // Left-click: tab body selects, close button closes. Middle-click
        // closes whichever part of the button it lands on (Yakuake-style).
        // A double-click on a tab body starts an inline rename (T6), any
        // press outside the rename input ends one, and a left press on a
        // tab body that moves beyond a small threshold drags it (T7).
        let button = match event {
            Event::Mouse(MouseEvent::ButtonPressed(button)) => *button,
            Event::Mouse(MouseEvent::CursorMoved { position }) => {
                // Drag progress (T7): once the press crossed the
                // threshold, follow the cursor's bar-local x. The release
                // may land outside the bar — iced's column routes events
                // to every child, whatever the cursor is over — so this
                // is the position the drop is decided from.
                let origin = layout.bounds().position();
                let local = Point::new(position.x - origin.x, position.y - origin.y);
                let Some(drag) = tree.state.downcast_mut::<State>().drag.as_mut() else {
                    return;
                };
                if !drag.started && (local.x - drag.origin.x).abs() < DRAG_THRESHOLD {
                    return;
                }
                drag.started = true;
                drag.x = local.x;
                return;
            }
            Event::Mouse(MouseEvent::ButtonReleased(Button::Left)) => {
                // Drag finish (T7): a plain click already selected on
                // press, so only a drag that crossed the threshold
                // reports a move, dropped wherever the indicator last was.
                let state = tree.state.downcast_mut::<State>();
                let Some(drag) = state.drag.take() else {
                    return;
                };
                if !drag.started {
                    return;
                }
                let Some(index) = self.tabs.iter().position(|tab| tab.id == drag.tab) else {
                    return;
                };
                let buttons = self.button_geometry(state, layout.bounds());
                let to = drop_index(&buttons, index, drag.x);
                if let Some(message) = self.on_move.as_ref().map(|on_move| on_move(drag.tab, to)) {
                    shell.publish(message);
                    shell.capture_event();
                }
                return;
            }
            _ => return,
        };

        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let point = cursor_position.position_in(bounds);
        let buttons = self.button_geometry(state, bounds);

        // A press outside the rename input ends the rename (T6): iced
        // inputs keep keyboard focus on outside presses, so the bar
        // reports the click-away itself and the press then lands on
        // whatever it hit — another tab, a close button, or the terminal
        // below the bar.
        if let Some((rename_tab, _)) = &self.rename {
            let over_input = point.is_some_and(|point| {
                self.tabs
                    .iter()
                    .position(|tab| tab.id == *rename_tab)
                    .and_then(|index| buttons.get(index))
                    .is_some_and(|button| button.bounds.contains(point))
            });
            if !over_input && let Some(end) = self.on_rename_end.as_ref().map(|end| end()) {
                shell.publish(end);
            }
        }

        if !matches!(button, Button::Left | Button::Middle) {
            return;
        }
        let Some(point) = point else {
            return;
        };
        let Some((index, target)) = hit_test(&buttons, point) else {
            return;
        };
        let Some(tab) = self.tabs.get(index) else {
            return;
        };

        let on_close = target == HitTarget::Close || button == Button::Middle;
        if !on_close {
            // Classify this press against the previous tab-body press
            // using iced's own double-click thresholds (within 300 ms
            // and 6 px of the last click; see `mouse::click`).
            let click = click::Click::new(point, Button::Left, state.click.take());
            let is_double = click.kind() == click::Kind::Double;
            state.click = Some(click);
            if is_double && let Some(rename) = self.on_rename.as_ref().map(|rename| rename(tab.id))
            {
                shell.publish(rename);
                shell.capture_event();
                return;
            }
        }

        // A single left press on a tab body arms a potential drag (T7):
        // the press selects now, and crossing [`DRAG_THRESHOLD`] while
        // held turns it into a reorder — the native tab bar split between
        // click and drag.
        if !on_close {
            state.drag = Some(Drag {
                tab: tab.id,
                origin: point,
                started: false,
                x: point.x,
            });
        }

        let handler = if on_close {
            self.on_close.as_ref().map(|close| close(tab.id))
        } else {
            self.on_select.as_ref().map(|select| select(tab.id))
        };
        if let Some(message) = handler {
            shell.publish(message);
            shell.capture_event();
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        // While a rename is active (T6), host its input as an overlay
        // exactly over the renamed tab's button — the spec's "title swaps
        // for an inline text input".
        let rename_tab = self.rename.as_ref().map(|(tab, _)| *tab)?;
        let index = self.tabs.iter().position(|tab| tab.id == rename_tab)?;
        let state = tree.state.downcast_ref::<State>();
        let button = self
            .button_geometry(state, layout.bounds())
            .get(index)?
            .bounds;
        let origin = layout.bounds().position();
        let mut position = button.position() + Vector::new(origin.x, origin.y) + translation;
        // Rounded like libcosmic's popover, to keep rendering crisp.
        position.x = position.x.round();
        position.y = position.y.round();

        let input = &mut self.rename.as_mut()?.1;
        Some(overlay::Element::new(Box::new(RenameOverlay {
            tree: tree.children.first_mut()?,
            input,
            position,
            size: button.size(),
        })))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor_position: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let cosmic_theme = theme.cosmic();
        let origin = layout.bounds().position();
        let buttons = self.button_geometry(state, layout.bounds());
        let hover_point = cursor_position.position_in(layout.bounds());

        // The strip's own opaque background: matching the terminal pane
        // makes the bar part of the terminal surface instead of a strip
        // of window clear color (which follows the system theme and can
        // disagree with the terminal scheme).
        let (pane_bg, pane_fg) = pane_colors();
        renderer.fill_quad(
            Quad {
                bounds: layout.bounds(),
                border: Border {
                    radius: 0.0.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                snap: true,
                ..Default::default()
            },
            pane_bg,
        );

        for (index, (tab, button)) in self.tabs.iter().zip(&buttons).enumerate() {
            let is_active = index == self.active;
            let is_hovered = hover_point.is_some_and(|point| button.bounds.contains(point));

            let bounds = button.bounds + Vector::new(origin.x, origin.y);

            // Backgrounds and text colors mirror the native `TabBar`
            // preset of libcosmic's segmented-button theme (pinned rev
            // `08a09e3`): the active tab is neutral_5 at 0.2 alpha with
            // the accent text color, hovered tabs get the subtler 0.1,
            // inactive tabs stay flat.
            let background: Option<Color> = if is_active {
                Some(cosmic_theme.palette.neutral_5.with_alpha(0.2).into())
            } else if is_hovered {
                Some(cosmic_theme.palette.neutral_5.with_alpha(0.1).into())
            } else {
                None
            };
            if let Some(background) = background {
                renderer.fill_quad(
                    Quad {
                        bounds,
                        border: Border {
                            radius: cosmic_theme.radius_s().into(),
                            width: 0.0,
                            color: Color::TRANSPARENT,
                        },
                        snap: true,
                        ..Default::default()
                    },
                    background,
                );
            }

            let text_color: Color = if is_active {
                cosmic_theme.accent_text_color().into()
            } else {
                // Luminance-derived against the pane background, so the
                // label stays readable whatever theme/scheme combo is
                // active (see `pane_colors`).
                pane_fg
            };

            // Title, clipped to the region between the button's left
            // padding and its close target (long titles cut at the edge).
            if let Some(paragraph) = state.paragraphs.get(&tab.id) {
                let text_x = bounds.x + METRICS.button_padding_x;
                let text_width = (bounds.width
                    - METRICS.button_padding_x * 2.0
                    - METRICS.close_size
                    - METRICS.text_gap)
                    .max(0.0);
                let text_y = bounds.y + (bounds.height - paragraph.min_height()) / 2.0;
                renderer.fill_paragraph(
                    paragraph.raw(),
                    Point::new(text_x, text_y),
                    text_color,
                    Rectangle::new(
                        Point::new(text_x, bounds.y),
                        Size::new(text_width, bounds.height),
                    ),
                );
            }

            // Close button: a `×` glyph centered in its target rect.
            let close = button.close + Vector::new(origin.x, origin.y);
            renderer.fill_text(
                Text {
                    content: "×".to_owned(),
                    bounds: Size::new(close.width, close.height),
                    size: Pixels(TITLE_FONT_SIZE),
                    line_height: LineHeight::default(),
                    font: renderer.default_font(),
                    align_x: cosmic::iced::core::text::Alignment::Center,
                    align_y: alignment::Vertical::Center,
                    shaping: Shaping::Basic,
                    wrapping: Wrapping::default(),
                    ellipsize: Ellipsize::None,
                },
                close.position(),
                text_color,
                close,
            );
        }

        // Drop indicator (T7): a vertical accent line in the slot the
        // drag currently lands in, between the buttons that would
        // surround the dragged tab once it leaves the row.
        if let Some(drag) = state.drag.as_ref().filter(|drag| drag.started)
            && let Some(index) = self.tabs.iter().position(|tab| tab.id == drag.tab)
        {
            let drop = drop_index(&buttons, index, drag.x);
            let x = origin.x + indicator_x(&buttons, index, drop, METRICS.button_gap);
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle::new(
                        Point::new(
                            x - DROP_INDICATOR_WIDTH / 2.0,
                            origin.y + DROP_INDICATOR_INSET,
                        ),
                        Size::new(
                            DROP_INDICATOR_WIDTH,
                            METRICS.bar_height - DROP_INDICATOR_INSET * 2.0,
                        ),
                    ),
                    border: Border {
                        radius: (DROP_INDICATOR_WIDTH / 2.0).into(),
                        width: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    snap: true,
                    ..Default::default()
                },
                Color::from(cosmic_theme.accent_color()),
            );
        }
    }
}

/// Hosts the inline-rename input over its tab button (T6), delegating
/// every [`overlay::Overlay`] method to the widget inside — the same
/// hosting pattern as libcosmic's popover at the pinned rev
/// (`widget/popover.rs`). Delegating [`overlay::Overlay::operate`] is
/// what lets `text_input::focus` and `select_all` reach the input through
/// the overlay.
struct RenameOverlay<'a, 'b, Message> {
    tree: &'a mut Tree,
    input: &'a mut Element<'b, Message>,
    /// Surface-space position of the renamed button.
    position: Point,
    size: Size,
}

impl<Message> overlay::Overlay<Message, Theme, Renderer> for RenameOverlay<'_, '_, Message>
where
    Message: Clone,
{
    fn layout(&mut self, renderer: &Renderer, _bounds: Size) -> layout::Node {
        // The input fills its tab button's rectangle.
        let limits = layout::Limits::new(Size::ZERO, self.size)
            .width(Length::Fill)
            .height(Length::Fill);
        self.input
            .as_widget_mut()
            .layout(self.tree, renderer, &limits)
            .move_to(self.position)
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor_position: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let viewport = layout.bounds();
        self.input.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor_position,
            renderer,
            clipboard,
            shell,
            &viewport,
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor_position: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let viewport = layout.bounds();
        self.input.as_widget().mouse_interaction(
            self.tree,
            layout,
            cursor_position,
            &viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor_position: mouse::Cursor,
    ) {
        let viewport = layout.bounds();
        self.input.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout,
            cursor_position,
            &viewport,
        )
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.input
            .as_widget_mut()
            .operate(self.tree, layout, renderer, operation)
    }
}

impl<'a, Message> From<TabBar<'a, Message>> for Element<'a, Message>
where
    Message: Clone + 'a,
{
    fn from(tab_bar: TabBar<'a, Message>) -> Self {
        Self::new(tab_bar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_foreground_is_dark_on_light_panes() {
        // The light COSMIC scheme's lavender background.
        let fg = readable_foreground(Color::from_rgba8(0xCC, 0xD0, 0xDE, 1.0));
        assert!(
            (fg.r + fg.g + fg.b) / 3.0 < 0.2,
            "expected near-black, got {fg:?}"
        );
    }

    #[test]
    fn readable_foreground_is_light_on_dark_panes() {
        // A dark scheme background.
        let fg = readable_foreground(Color::from_rgba8(0x1B, 0x1B, 0x22, 1.0));
        assert!(
            (fg.r + fg.g + fg.b) / 3.0 > 0.8,
            "expected near-white, got {fg:?}"
        );
    }

    #[test]
    fn pane_colors_derives_text_from_the_stored_pane_background() {
        // Drive the process-global with a known light pane (the COSMIC
        // light scheme's lavender) so the assertions are deterministic.
        let saved = WINDOW_BG_COLOR.swap(0xAACCD0DE, std::sync::atomic::Ordering::SeqCst);
        let (strip, fg) = pane_colors();
        WINDOW_BG_COLOR.store(saved, std::sync::atomic::Ordering::SeqCst);

        let pane = Color::from_rgba8(0xCC, 0xD0, 0xDE, 1.0);
        let mean = |c: Color| (c.r + c.g + c.b) / 3.0;
        assert!(
            mean(strip) < mean(pane),
            "strip {strip:?} should be darker than pane {pane:?}"
        );
        assert!(
            mean(fg) < 0.2,
            "expected near-black text on a light pane, got {fg:?}"
        );
    }

    /// Tests run against a small, easy-to-reason-about metrics set.
    const M: BarMetrics = BarMetrics {
        bar_height: 32.0,
        min_button_width: 48.0,
        max_button_width: 200.0,
        button_padding_x: 10.0,
        button_gap: 4.0,
        close_size: 24.0,
        text_gap: 6.0,
    };

    fn geometry(text_widths: &[f32], available_width: f32) -> Vec<ButtonGeometry> {
        layout_buttons(available_width, text_widths, &M)
    }

    fn center_of(rect: &Rectangle<f32>) -> Point {
        Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }

    #[test]
    fn layout_places_buttons_left_to_right_with_gaps() {
        let buttons = geometry(&[40.0, 60.0], 1000.0);
        assert_eq!(buttons.len(), 2);

        let w0 = 10.0 + 40.0 + 6.0 + 24.0 + 10.0;
        let w1 = 10.0 + 60.0 + 6.0 + 24.0 + 10.0;
        assert_eq!(buttons[0].bounds.x, 0.0);
        assert_eq!(buttons[0].bounds.width, w0);
        assert_eq!(buttons[1].bounds.x, w0 + 4.0);
        assert_eq!(buttons[1].bounds.width, w1);
    }

    #[test]
    fn layout_buttons_fill_the_bar_height() {
        let buttons = geometry(&[40.0], 1000.0);
        assert_eq!(buttons[0].bounds.y, 0.0);
        assert_eq!(buttons[0].bounds.height, M.bar_height);
    }

    #[test]
    fn layout_anchors_close_button_at_each_buttons_right_edge() {
        let buttons = geometry(&[40.0], 1000.0);
        let button = &buttons[0];
        assert_eq!(button.close.width, M.close_size);
        assert_eq!(button.close.height, M.close_size);
        assert_eq!(
            button.close.x + button.close.width + M.button_padding_x,
            button.bounds.x + button.bounds.width
        );
        assert_eq!(
            button.close.y,
            button.bounds.y + (M.bar_height - M.close_size) / 2.0
        );
    }

    #[test]
    fn layout_clamps_long_titles_to_the_max_button_width() {
        let buttons = geometry(&[500.0], 1000.0);
        assert_eq!(buttons[0].bounds.width, M.max_button_width);
    }

    #[test]
    fn layout_raises_buttons_to_the_min_button_width() {
        // A minimum above the natural padding+close width (50) clamps the
        // narrowest possible button up, so it stays a usable target.
        let metrics = BarMetrics {
            min_button_width: 60.0,
            ..M
        };
        let buttons = layout_buttons(1000.0, &[0.0], &metrics);
        assert_eq!(buttons[0].bounds.width, 60.0);
    }

    #[test]
    fn layout_squeezes_buttons_proportionally_when_the_row_overflows() {
        // Three titles whose natural buttons are 100 wide (300 total, plus
        // 8 of gaps) squeezed into 158: buttons budget 150, scale 0.5, so
        // every button lands at 50 and the row exactly fits.
        let buttons = geometry(&[50.0, 50.0, 50.0], 158.0);
        assert_eq!(buttons.len(), 3);
        assert_eq!(buttons[0].bounds.width, 50.0);
        assert_eq!(buttons[1].bounds.width, 50.0);
        assert_eq!(buttons[2].bounds.width, 50.0);
        assert_eq!(buttons[2].bounds.x + buttons[2].bounds.width, 158.0);
    }

    #[test]
    fn layout_never_squeezes_below_the_min_button_width() {
        // Five tabs that cannot fit: the proportional squeeze hits the
        // floor, so every button keeps its minimum width.
        let buttons = geometry(&[50.0; 5], 100.0);
        for (i, button) in buttons.iter().enumerate() {
            assert_eq!(button.bounds.width, M.min_button_width, "button {i}");
        }
    }

    #[test]
    fn layout_of_zero_tabs_is_empty() {
        assert!(geometry(&[], 500.0).is_empty());
    }

    #[test]
    fn hit_test_lands_on_the_tab_body() {
        let buttons = geometry(&[40.0, 60.0], 1000.0);
        // Inside the first button, left of its close target.
        let point = Point::new(buttons[0].bounds.x + 5.0, 10.0);
        assert_eq!(hit_test(&buttons, point), Some((0, HitTarget::Tab)));
    }

    #[test]
    fn hit_test_lands_on_the_close_button() {
        let buttons = geometry(&[40.0, 60.0], 1000.0);
        assert_eq!(
            hit_test(&buttons, center_of(&buttons[1].close)),
            Some((1, HitTarget::Close))
        );
    }

    #[test]
    fn hit_test_misses_gaps_and_outside_the_row() {
        let buttons = geometry(&[40.0, 60.0], 1000.0);
        // In the gap between the two buttons.
        let gap_x = buttons[0].bounds.x + buttons[0].bounds.width + 2.0;
        assert_eq!(hit_test(&buttons, Point::new(gap_x, 5.0)), None);
        // Left of the first and beyond the last button.
        assert_eq!(hit_test(&buttons, Point::new(-5.0, 5.0)), None);
        assert_eq!(
            hit_test(
                &buttons,
                Point::new(buttons[1].bounds.x + buttons[1].bounds.width + 5.0, 5.0)
            ),
            None
        );
        // Vertically outside the bar.
        assert_eq!(hit_test(&buttons, Point::new(5.0, 100.0)), None);
    }

    #[test]
    fn hit_test_on_an_empty_bar_misses() {
        assert_eq!(hit_test(&[], Point::new(5.0, 5.0)), None);
    }

    #[test]
    fn drop_index_left_of_the_row_drops_first() {
        let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
        assert_eq!(drop_index(&buttons, 0, -10.0), 0);
    }

    #[test]
    fn drop_index_holding_a_neighbors_left_half_keeps_the_slot() {
        let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
        // x = 100 sits inside button 1, left of its center (149): the
        // cursor has not passed any other button's center yet.
        assert_eq!(drop_index(&buttons, 0, 100.0), 0);
    }

    #[test]
    fn drop_index_counts_only_the_neighbors_passed() {
        let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
        // Past button 1's center (149) but not button 2's (253).
        assert_eq!(drop_index(&buttons, 0, 180.0), 1);
        // Past every other button's center.
        assert_eq!(drop_index(&buttons, 0, 260.0), 2);
    }

    #[test]
    fn drop_index_skips_the_dragged_button_itself() {
        let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
        // Dragging the middle tab from within its own span: only button 0
        // was passed, so the drop slot still holds its own position.
        assert_eq!(drop_index(&buttons, 1, 180.0), 1);
        // Past button 2's center, the middle tab drops last.
        assert_eq!(drop_index(&buttons, 1, 260.0), 2);
    }

    #[test]
    fn drop_index_beyond_the_row_drops_last() {
        let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
        assert_eq!(drop_index(&buttons, 0, 10_000.0), 2);
    }

    #[test]
    fn indicator_x_sits_before_the_button_following_the_drop_slot() {
        let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
        // Dropping tab 0 at slot 1: the row minus tab 0 is [b1, b2], and
        // the slot's follower is b2, so the indicator precedes b2.
        assert_eq!(indicator_x(&buttons, 0, 1, M.button_gap), 208.0 - 2.0);
    }

    #[test]
    fn indicator_x_dropping_first_precedes_the_row() {
        let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
        assert_eq!(indicator_x(&buttons, 1, 0, M.button_gap), 0.0 - 2.0);
    }

    #[test]
    fn indicator_x_appending_at_the_end_follows_the_row() {
        let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
        // Dropping tab 0 last: with tab 0 gone the row ends at b2's right
        // edge (298), so the indicator lands half a gap past it.
        assert_eq!(indicator_x(&buttons, 0, 2, M.button_gap), 298.0 + 2.0);
    }

    #[test]
    fn indicator_x_on_an_empty_row_is_the_origin() {
        assert_eq!(indicator_x(&[], 0, 0, M.button_gap), 0.0);
    }
}
