// SPDX-License-Identifier: GPL-3.0-only

use cosmic::iced::core::alignment;
use cosmic::iced::core::overlay;
use cosmic::iced::core::text::{
    Ellipsize, EllipsizeHeightLimit, LineHeight, Shaping, Wrapping, paragraph,
};
use cosmic::iced::core::{
    Border, Shell, Text,
    clipboard::Clipboard,
    layout::{self, Layout},
    renderer::{self, Quad, Renderer as _},
    text::Renderer as _,
    widget::{Tree, Widget, tree},
};
use cosmic::iced::{
    Color, Pixels, Point, Rectangle, Size, Vector,
    event::Event,
    keyboard,
    mouse::{self, Button, Event as MouseEvent},
};
use cosmic::theme::Theme;
use cosmic::{Element, Renderer, widget};

use super::{ConfigKey, PickerId, SettingsEvent, font_picker_rows, font_picker_selected};
use crate::app::interned_font_family;
use crate::config::DropTermConfig;

// --- the font picker: the family list is the preview (font-picker) ---
//
// The Font control is `SchemePicker`'s shape adopted whole: a closed
// control previewing at rest (the configured family's name, set in
// that family), a host widget delegating layout/update/draw to it
// and hosting the popover while the app holds the picker open, and a
// dropdown-styled popover anchored below the control (above when the
// space runs out) tracking it through the drawer's scrollable. The
// font deltas on the scheme apparatus: every popover row's label is
// set in its own family — the row shows the real thing, not a
// name-as-proxy — and long lists wheel-scroll (OQ-2: wheel only, no
// scrollbar) with labels ellipsized end-ward at the clamped width,
// every row reachable through the clamp.

/// One popover row's height in px (OQ-3: 32 px, the scheme row's
/// family resemblance and comfortable own-font rendering).
const FONT_ROW_HEIGHT: f32 = 32.0;
/// The popover's inner horizontal padding in px — the scheme
/// popover's own value.
const FONT_POPOVER_PAD_X: f32 = 8.0;

/// The picker's cached label paragraph type — a plain paragraph in
/// the renderer's own backend.
type FontNameParagraph =
    paragraph::Plain<<Renderer as cosmic::iced::core::text::Renderer>::Paragraph>;

/// One cached label paragraph: the family's name set in that family
/// (the row's preview), laid out single-line at `width` —
/// [`f32::INFINITY`] measures the label's full width, a finite width
/// ellipsizes it end-ward (the long-Nerd-Font-name rule).
fn family_paragraph(family: &str, width: f32) -> FontNameParagraph {
    let mut paragraph = FontNameParagraph::default();
    paragraph.update(Text {
        content: family,
        bounds: Size::new(width, Size::INFINITE.height),
        size: Pixels(14.0),
        line_height: LineHeight::default(),
        font: cosmic::iced::Font {
            family: cosmic::iced::font::Family::Name(interned_font_family(family)),
            ..cosmic::iced::Font::default()
        },
        align_x: cosmic::iced::core::text::Alignment::Left,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
        wrapping: Wrapping::None,
        ellipsize: Ellipsize::End(EllipsizeHeightLimit::Lines(1)),
    });
    paragraph
}

/// Clamp a popover scroll offset into `[0, content − viewport]` px —
/// both ends honest, and a list shorter than its viewport pins at 0
/// (nothing to scroll into). Pure math, pinned.
pub(super) fn clamp_scroll(offset: f32, content: f32, viewport: f32) -> f32 {
    offset.clamp(0.0, (content - viewport).max(0.0))
}

/// A wheel delta's vertical px: pixels verbatim, lines scaled by the
/// row height — one notch ≈ one row, the terminal-scroll feel.
fn wheel_px(delta: mouse::ScrollDelta) -> f32 {
    match delta {
        mouse::ScrollDelta::Pixels { y, .. } => y,
        mouse::ScrollDelta::Lines { y, .. } => y * FONT_ROW_HEIGHT,
    }
}

/// The font picker's closed control: the stored `font_name` — the
/// family's name, set in that family (OQ-1: name only; the popover
/// one press away is the glyph sample). A family the renderer cannot
/// resolve falls back to the UI font while the name stays what the
/// file says — honest about the stored value, never a corrected one.
#[must_use]
pub(super) fn font_picker_control(
    config: &DropTermConfig,
    families: &[String],
    open: bool,
) -> Element<'static, SettingsEvent> {
    let control: Element<'static, SettingsEvent> = widget::button::custom(
        widget::text(config.font_name.clone()).font(cosmic::iced::Font {
            family: cosmic::iced::font::Family::Name(interned_font_family(&config.font_name)),
            ..cosmic::iced::Font::default()
        }),
    )
    .on_press(SettingsEvent::TogglePicker(PickerId::Font))
    .into();
    Element::new(FontPicker {
        rows: font_picker_rows(families, &config.font_name),
        selected: font_picker_selected(families, &config.font_name),
        open,
        control,
    })
}

/// The font picker host: the `SchemePicker` delegation pattern —
/// lays out, updates, and draws exactly its closed control, and,
/// while the app holds the picker open, hosts the popover as its
/// overlay, anchored to the control's translated bounds so it tracks
/// the control inside the drawer's scrollable.
struct FontPicker<'a> {
    /// The popover's rows: the families in scan order, the stored
    /// name leading when no family matches it — each row's own name
    /// is both its label and its font.
    rows: Vec<String>,
    /// The selected row ([`font_picker_selected`]'s resolution).
    selected: usize,
    /// Whether the app holds this picker's popover open.
    open: bool,
    control: Element<'a, SettingsEvent>,
}

/// The picker host's tree state: the label paragraphs (measure and
/// draw sets) and the popover's scroll offset. Families are
/// process-stable, so the caches rebuild only when the row list
/// changes (the synthetic leader's presence) or the clamped width
/// does (the window resized under the open popover) — never per
/// frame.
struct FontPickerState {
    /// The rows' unbounded label paragraphs — each label's full width
    /// in its own family, the popover's content-sizing input.
    measure: Vec<FontNameParagraph>,
    /// The rows' draw paragraphs — the same labels, ellipsized at
    /// `width` when the clamped popover cannot fit them.
    draw: Vec<FontNameParagraph>,
    /// The text width the draw set was ellipsized at; `None` while it
    /// mirrors the unbounded measure (the content fits).
    width: Option<f32>,
    /// The popover's wheel-scroll offset in px, clamped by
    /// [`clamp_scroll`].
    scroll: f32,
}

impl FontPickerState {
    fn new() -> Self {
        Self {
            measure: Vec::new(),
            draw: Vec::new(),
            width: None,
            scroll: 0.0,
        }
    }

    /// Rebuild both paragraph sets for `rows` (a row-list change):
    /// unbounded measure, draw mirroring it, scroll back to the top —
    /// a fresh list starts fresh.
    fn rebuild(&mut self, rows: &[String]) {
        self.measure = rows
            .iter()
            .map(|family| family_paragraph(family, f32::INFINITY))
            .collect();
        self.draw.clone_from(&self.measure);
        self.width = None;
        self.scroll = 0.0;
    }

    /// Re-ellipsize the draw set at `budget` (the popover's inner
    /// text width); `None` restores the unbounded measure verbatim
    /// (the content fits — nothing to ellipsize). A no-op when the
    /// budget is unchanged, so the steady frame costs two compares.
    fn ellipsize_at(&mut self, rows: &[String], budget: Option<f32>) {
        if self.width == budget {
            return;
        }
        match budget {
            Some(width) => {
                let rebuilt: Vec<_> = rows
                    .iter()
                    .map(|family| family_paragraph(family, width))
                    .collect();
                self.draw = rebuilt;
            }
            None => self.draw.clone_from(&self.measure),
        }
        self.width = budget;
    }
}

impl Widget<SettingsEvent, Theme, Renderer> for FontPicker<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<FontPickerState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(FontPickerState::new())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.control)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut [self.control.as_widget_mut()]);
        let state = tree.state.downcast_mut::<FontPickerState>();
        // The row list changes only when the synthetic leader's
        // presence flips or the stored leader renames — anything else
        // (a family pick) leaves the list byte-identical.
        let rows_changed = state.measure.len() != self.rows.len()
            || state
                .measure
                .iter()
                .zip(&self.rows)
                .any(|(paragraph, name)| paragraph.content() != name);
        if rows_changed {
            state.rebuild(&self.rows);
        }
    }

    fn size(&self) -> Size<cosmic::iced::Length> {
        self.control.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.control
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, SettingsEvent>,
        viewport: &Rectangle,
    ) {
        self.control.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.control.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.control.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, SettingsEvent, Theme, Renderer>> {
        if let Some(child) = self.control.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        ) {
            return Some(child);
        }
        if !self.open {
            return None;
        }
        let bounds = layout.bounds();
        Some(overlay::Element::new(Box::new(FontPopover {
            state: tree.state.downcast_mut::<FontPickerState>(),
            rows: &self.rows,
            selected: self.selected,
            anchor: Rectangle::new(bounds.position() + translation, bounds.size()),
        })))
    }
}

/// The open font picker's popover: one row per picker row — the
/// family's name set in that family, ellipsized end-ward at the
/// clamped width — in a dropdown-styled bubble anchored below the
/// control (above when the space below runs out, the stock menu's
/// rule), width content-sized and clamped to the window's edge.
/// Dismissal is the scheme popover's contract verbatim: a left press
/// on a row picks it and closes, any other press closes, every press
/// is captured, and Escape closes. Long lists wheel-scroll (OQ-2)
/// with the offset clamped so every listed family is reachable; the
/// persisted pick carries the stock menu's selected treatment (fill
/// + accent name).
struct FontPopover<'a> {
    state: &'a mut FontPickerState,
    rows: &'a [String],
    selected: usize,
    /// The control's absolute bounds (translation applied).
    anchor: Rectangle,
}

impl FontPopover<'_> {
    /// The popover's full content height — every row, scrolled or
    /// not.
    #[expect(
        clippy::cast_precision_loss,
        reason = "row count is far below 2^24, so usize -> f32 is exact"
    )]
    fn full_height(&self) -> f32 {
        self.rows.len() as f32 * FONT_ROW_HEIGHT
    }

    /// The row under the point, scroll-offset aware: the point's
    /// place in the scrolled content, not the viewport.
    fn row_at(bounds: Rectangle, point: Point, scroll: f32) -> Option<usize> {
        if !bounds.contains(point) {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "row indices are far below 2^24, so the f32 floor is exact and non-negative inside the bounds"
        )]
        Some(((point.y - bounds.y + scroll) / FONT_ROW_HEIGHT) as usize)
    }
}

impl overlay::Overlay<SettingsEvent, Theme, Renderer> for FontPopover<'_> {
    fn layout(&mut self, _renderer: &Renderer, bounds: Size) -> layout::Node {
        let widest = self
            .state
            .measure
            .iter()
            .map(FontNameParagraph::min_width)
            .fold(0.0, f32::max);
        let content = widest + FONT_POPOVER_PAD_X * 2.0;
        let width = content
            .max(self.anchor.width)
            .min((bounds.width - self.anchor.x).max(0.0));
        // Labels ellipsize only where the clamped width cannot fit
        // them; a fitting list draws its measure set verbatim.
        let budget = width - FONT_POPOVER_PAD_X * 2.0;
        self.state
            .ellipsize_at(self.rows, (budget < widest).then_some(budget.max(0.0)));
        let space_below = bounds.height - (self.anchor.y + self.anchor.height);
        let space_above = self.anchor.y;
        let below = space_below >= space_above;
        let height = self
            .full_height()
            .min(if below { space_below } else { space_above });
        // The settled viewport re-clamps the offset: a resize or a
        // row-list change can never leave it past either end.
        self.state.scroll = clamp_scroll(self.state.scroll, self.full_height(), height);
        let position = if below {
            Point::new(self.anchor.x, self.anchor.y + self.anchor.height)
        } else {
            Point::new(self.anchor.x, self.anchor.y - height)
        };
        layout::Node::new(Size::new(width, height)).move_to(position)
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, SettingsEvent>,
    ) {
        match event {
            // The scheme popover's dismissal contract, unchanged: a
            // left press on a row first publishes its pick — the
            // same `Picked` event and row-space index the shell
            // dropdown established — then everything closes, and the
            // press is captured so nothing under the bubble acts on
            // it.
            Event::Mouse(MouseEvent::ButtonPressed(button)) => {
                if *button == Button::Left
                    && let Some(index) = cursor
                        .position()
                        .and_then(|point| Self::row_at(layout.bounds(), point, self.state.scroll))
                    && index < self.rows.len()
                {
                    shell.publish(SettingsEvent::Picked(ConfigKey::FontName, index));
                }
                shell.publish(SettingsEvent::ClosePicker);
                shell.capture_event();
                shell.request_redraw();
            }
            // Wheel-only scrolling (OQ-2), inside the bubble: the
            // offset moves through the clamp and the wheel is
            // consumed there — a wheel outside the bubble falls
            // through to the drawer's own scrollable.
            Event::Mouse(MouseEvent::WheelScrolled { delta }) => {
                if cursor
                    .position()
                    .is_some_and(|point| layout.bounds().contains(point))
                {
                    self.state.scroll = clamp_scroll(
                        self.state.scroll + wheel_px(*delta),
                        self.full_height(),
                        layout.bounds().height,
                    );
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) => {
                shell.publish(SettingsEvent::ClosePicker);
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor
            .position()
            .is_some_and(|point| layout.bounds().contains(point))
        {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let cosmic = theme.cosmic();
        let menu = layout.bounds();
        // The stock dropdown container's own surface (the scheme
        // popover's paint): component background, hairline divider,
        // small radius — cohesion with every other dropdown in the
        // drawer.
        renderer.fill_quad(
            Quad {
                bounds: menu,
                border: Border {
                    width: 1.0,
                    color: cosmic.bg_component_divider().into(),
                    radius: cosmic.radius_s().into(),
                },
                snap: true,
                ..Default::default()
            },
            cosmic.bg_component_color(),
        );

        let hovered = cursor
            .position()
            .and_then(|point| Self::row_at(menu, point, self.state.scroll));
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "row indices are far below 2^24, so the f32 floor is exact and non-negative"
        )]
        let first = (self.state.scroll / FONT_ROW_HEIGHT) as usize;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the viewport's row count is far below 2^24; ceil rounds partial rows in"
        )]
        let visible = (menu.height / FONT_ROW_HEIGHT).ceil() as usize + 1;
        let accent: Color = cosmic.accent.base.into();
        // The scroll viewport clips the drawn slice: rows past either
        // end of the window never paint over the drawer beneath.
        renderer.with_layer(menu, |renderer| {
            for row in first..(first + visible).min(self.rows.len()) {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "row count is far below 2^24, so usize -> f32 is exact"
                )]
                let row_y = menu.y + row as f32 * FONT_ROW_HEIGHT - self.state.scroll;
                let bounds = Rectangle::new(
                    Point::new(menu.x, row_y),
                    Size::new(menu.width, FONT_ROW_HEIGHT),
                );
                let selected = self.selected == row;
                if selected {
                    renderer.fill_quad(
                        Quad {
                            bounds,
                            snap: true,
                            ..Default::default()
                        },
                        Color { a: 0.25, ..accent },
                    );
                } else if hovered == Some(row) {
                    renderer.fill_quad(
                        Quad {
                            bounds,
                            snap: true,
                            ..Default::default()
                        },
                        Color { a: 0.12, ..accent },
                    );
                }
                let label = &self.state.draw[row];
                let text_color = if selected { accent } else { style.text_color };
                let text_y = row_y + (FONT_ROW_HEIGHT - label.min_height()) / 2.0;
                renderer.fill_paragraph(
                    label.raw(),
                    Point::new(bounds.x + FONT_POPOVER_PAD_X, text_y),
                    text_color,
                    menu,
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- the font picker's widget (font-picker T2) ---

    #[test]
    fn the_font_picker_control_builds_in_every_config_state() {
        // Construction is the assertion (the scheme control's own
        // pin): closed and open, over a default config, a
        // stored-but-uninstalled family (the synthetic leader), and a
        // barren scan (the stored-value row alone).
        let mut config = DropTermConfig::default();
        let families = vec!["DejaVu Sans Mono".to_string(), "Noto Sans Mono".to_string()];
        for open in [false, true] {
            let _ = font_picker_control(&config, &families, open);
        }
        config.font_name = "Gone Font".to_string();
        for open in [false, true] {
            let _ = font_picker_control(&config, &families, open);
        }
        for open in [false, true] {
            let _ = font_picker_control(&config, &[], open);
        }
    }
}
