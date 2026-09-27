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
//! Colors mirror the `TabBar` preset of libcosmic's own
//! `theme/style/segmented_button.rs` (pinned rev `08a09e3`): the active
//! tab is `neutral_5` at 0.2 alpha with the accent text color and
//! `radius_s` corners, inactive tabs carry no background and use the
//! component's `on` text color.

use std::collections::HashMap;

use cosmic::iced::core::alignment;
use cosmic::iced::core::text::{Ellipsize, LineHeight, Shaping, Wrapping, paragraph};
use cosmic::iced::core::{
    Border, Shell, Text,
    clipboard::Clipboard,
    layout::{self, Layout},
    renderer::{self, Quad, Renderer as _},
    text::Renderer as _,
    widget::{Tree, Widget, tree},
};
use cosmic::iced::{
    Color, Length, Pixels, Point, Rectangle, Size, Vector,
    event::Event,
    mouse::{self, Button, Event as MouseEvent},
};
use cosmic::theme::Theme;
use cosmic::{Element, Renderer, cosmic_theme::palette::WithAlpha};

use crate::tabs::{Tab, TabId};

/// A cached title paragraph, one per tab, keyed by [`TabId`].
///
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

/// Title text size in logical pixels.
const TITLE_FONT_SIZE: f32 = 14.0;

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

/// Per-widget state: the title paragraphs, cached across frames and
/// rebuilt whenever a tab's title changes (measured once, then reused by
/// layout, hit-testing and drawing).
#[derive(Default)]
struct State {
    paragraphs: HashMap<TabId, TitleParagraph>,
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
        let button = match event {
            Event::Mouse(MouseEvent::ButtonPressed(Button::Left)) => Button::Left,
            Event::Mouse(MouseEvent::ButtonPressed(Button::Middle)) => Button::Middle,
            _ => return,
        };

        let state = tree.state.downcast_ref::<State>();
        let Some((index, target)) = cursor_position
            .position_in(layout.bounds())
            .and_then(|point| hit_test(&self.button_geometry(state, layout.bounds()), point))
        else {
            return;
        };
        let Some(tab) = self.tabs.get(index) else {
            return;
        };

        let on_close = target == HitTarget::Close || button == Button::Middle;
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
                cosmic_theme
                    .background(theme.transparent)
                    .component
                    .on
                    .into()
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
}
