// SPDX-License-Identifier: GPL-3.0-only

use alacritty_terminal::vte::ansi::Rgb;
use cosmic::iced::core::alignment;
use cosmic::iced::core::overlay;
use cosmic::iced::core::text::{LineHeight, Shaping, paragraph};
use cosmic::iced::core::{
    Border, Shell, Text,
    clipboard::Clipboard,
    layout::{self, Layout},
    renderer::{self, Quad, Renderer as _},
    text::Renderer as _,
    widget::{Tree, Widget, tree},
};
use cosmic::iced::widget::Space;
use cosmic::iced::{
    Background, Color, Length, Pixels, Point, Rectangle, Size, Vector,
    event::Event,
    keyboard,
    mouse::{self, Button, Event as MouseEvent},
};
use cosmic::theme::Theme;
use cosmic::{Element, Renderer, theme, widget};

use super::{
    ConfigKey, PickerId, ProfileDraft, SchemePickerId, SettingsEvent, dropdown_selected,
    inherit_scheme_preview, profile_scheme_selected, scheme_preview_by_name,
};
use crate::config::DropTermConfig;
use crate::terminal::theme_source::{SchemePreview, scheme_names, scheme_previews, scheme_swatch};

/// The row's scheme chip: the profile's palette miniaturized — the
/// scheme's own background carrying its six ANSI accents in
/// `ls`-colors order ([`scheme_swatch`]). An unset scheme, or a name
/// the registry no longer knows, renders the hollow chip — the frame
/// alone, "follows the global look" — matching the resolution
/// chain's fallback instead of inventing colors.
pub(super) fn scheme_chip(scheme: Option<&str>) -> Element<'static, SettingsEvent> {
    let swatch = scheme.and_then(scheme_swatch);
    let cells: Element<'static, SettingsEvent> = match &swatch {
        Some(swatch) => {
            let mut row = widget::row(Vec::<Element<'static, SettingsEvent>>::new()).spacing(1);
            for accent in swatch.accents.iter().flatten() {
                row = row.push(swatch_cell(*accent));
            }
            row.into()
        }
        // The hollow chip keeps the filled chip's footprint so the
        // name column's left edge never shifts between rows.
        None => widget::space::Space::new()
            .width(Length::from(29u16))
            .height(Length::from(12u16))
            .into(),
    };
    let fill = swatch
        .as_ref()
        .and_then(|swatch| swatch.background)
        .map(|rgb| Color::from_rgb8(rgb.r, rgb.g, rgb.b));
    widget::container(cells)
        .padding(2)
        .class(theme::Container::custom(move |theme| {
            let cosmic = theme.cosmic();
            cosmic::iced::widget::container::Style {
                background: Some(Background::Color(
                    fill.unwrap_or(Color::from(cosmic.background(true).base)),
                )),
                border: Border {
                    color: Color::from(cosmic.palette.neutral_8).scale_alpha(0.4),
                    width: 1.0,
                    radius: cosmic.corner_radii.radius_xs.into(),
                },
                ..Default::default()
            }
        }))
        .into()
}

/// One chip cell: a fixed swatch of a scheme accent.
fn swatch_cell(rgb: Rgb) -> Element<'static, SettingsEvent> {
    let color = Color::from_rgb8(rgb.r, rgb.g, rgb.b);
    widget::container(
        widget::space::Space::new()
            .width(Length::from(4u16))
            .height(Length::from(12u16)),
    )
    .class(theme::Container::custom(move |_| {
        cosmic::iced::widget::container::Style {
            background: Some(Background::Color(color)),
            ..Default::default()
        }
    }))
    .into()
}

// --- the scheme pickers: the palette is the preview (scheme-picker) ---
//
// The scheme controls are not stock dropdowns: every scheme row
// shows the scheme itself — a background chip, a foreground chip,
// then the sixteen ANSI colors the terminal actually renders with,
// resolved through [`scheme_previews`] (the same conversion the
// terminal serves, pinned). The closed control shows the configured
// scheme's name plus a mini chip strip; a press opens a popover that
// lists the whole registry, anchored like the stock dropdown's menu
// (the overlay resolves its position per frame against the anchor's
// translated bounds, so it tracks the control through the drawer's
// scrollable). A global picker's row pick emits the *same*
// [`SettingsEvent::Picked`] with the *same* registry-order index the
// stock dropdown emitted — planners and writes untouched. The
// profile form's picker reuses the whole apparatus with one extra
// row: a leading **Inherit** row (row 0) previewing the global
// scheme resolved for the current system mode, whose pick emits
// [`SettingsEvent::PickProfileScheme`] in the same row space
// [`profile_scheme_write`] maps.

/// One popover row's height in px.
const SCHEME_ROW_HEIGHT: f32 = 32.0;
/// One popover chip's edge in px — 2px-radius squares, a nod to
/// terminal cells (the OQ-2 recommendation, hardware-confirmed
/// pending).
const SCHEME_CHIP: f32 = 10.0;
/// The gap between chips in px.
const SCHEME_CHIP_GAP: f32 = 2.0;
/// The closed control's mini-strip chip edge in px — small enough
/// that all eighteen fit the item's control column.
const SCHEME_CHIP_MINI: f32 = 5.0;
/// The popover's inner horizontal padding in px.
const SCHEME_POPOVER_PAD_X: f32 = 8.0;
/// The profile picker's leading Inherit row label.
const SCHEME_INHERIT_ROW: &str = "Inherit";

/// One chip strip's total width: eighteen chips and their gaps —
/// the popover's rows and layout share the arithmetic.
const fn scheme_strip_width() -> f32 {
    18.0 * SCHEME_CHIP + 17.0 * SCHEME_CHIP_GAP
}

/// The picker's cached name paragraph — registry names are
/// compile-time, so the cache is built once and only the count is
/// guarded.
type SchemeNameParagraph =
    paragraph::Plain<<Renderer as cosmic::iced::core::text::Renderer>::Paragraph>;

/// One cached label paragraph: the fixed styling every popover row's
/// name shares.
fn name_paragraph(content: &str) -> SchemeNameParagraph {
    let mut paragraph = SchemeNameParagraph::default();
    paragraph.update(Text {
        content,
        bounds: Size::INFINITE,
        size: Pixels(14.0),
        line_height: LineHeight::default(),
        font: cosmic::font::default(),
        align_x: cosmic::iced::core::text::Alignment::Left,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
        wrapping: cosmic::iced::core::text::Wrapping::default(),
        ellipsize: cosmic::iced::core::text::Ellipsize::None,
    });
    paragraph
}

/// The scheme picker host's tree state: one name paragraph per
/// registry scheme in registry order, plus the profile picker's
/// static Inherit label cached beside them.
struct SchemePickerState {
    names: Vec<SchemeNameParagraph>,
    inherit: SchemeNameParagraph,
}

impl SchemePickerState {
    fn new() -> Self {
        let names = scheme_names()
            .iter()
            .map(|name| name_paragraph(name))
            .collect();
        Self {
            names,
            inherit: name_paragraph(SCHEME_INHERIT_ROW),
        }
    }
}

/// One chip of the closed control's mini strip: a solid rounded
/// square in the previewed color. A `None` color (the COSMIC pair's
/// background) previews the honest thing — the theme surface the
/// terminal falls back to, read live so the chip follows the theme.
fn mini_chip(size: f32, color: Option<cosmic::iced::Color>) -> Element<'static, SettingsEvent> {
    widget::container(Space::new().width(size).height(size))
        .width(size)
        .height(size)
        .class(theme::Container::custom(move |theme| {
            let color = color.unwrap_or_else(|| {
                cosmic::iced::Color::from(theme.cosmic().background(theme.transparent).base)
            });
            cosmic::iced::widget::container::Style {
                background: Some(color.into()),
                border: Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }))
        .into()
}

/// The scheme's mini chip strip: background, foreground, then the
/// sixteen ANSI colors — the shared preview seam. The closed scheme
/// controls render it; the profile form's scheme feedback
/// (`profile-manager`) reuses it with zero new chip code.
fn scheme_chip_strip(preview: SchemePreview) -> Element<'static, SettingsEvent> {
    let rgb_color = |rgb: crate::terminal::theme_source::Rgb| Color::from_rgb8(rgb.r, rgb.g, rgb.b);
    let chips = std::iter::once(preview.background.map(rgb_color))
        .chain(std::iter::once(preview.foreground.map(rgb_color)))
        .chain(preview.ansi.iter().map(|slot| slot.map(rgb_color)))
        .map(|color| mini_chip(SCHEME_CHIP_MINI, color));
    widget::row(chips.collect::<Vec<_>>()).spacing(1.5).into()
}

/// The scheme picker's closed control: the configured scheme's name
/// over its mini chip strip, the whole button opening the popover.
/// The name is the stored value verbatim — a resolvable name is the
/// registry's own spelling, an unknown one stays honest about what
/// the file says (and marks no popover row: `dropdown_selected` is
/// the single index source for both the strip and the marker).
#[must_use]
pub(super) fn scheme_picker_control(
    key: ConfigKey,
    config: &DropTermConfig,
    open: bool,
) -> Element<'static, SettingsEvent> {
    let selected = dropdown_selected(key, config);
    let stored = match key {
        ConfigKey::ColorSchemeDark => config.color_scheme_dark.clone(),
        ConfigKey::ColorSchemeLight => config.color_scheme_light.clone(),
        _ => String::new(),
    };
    let preview =
        selected.and_then(|index| scheme_previews().get(index).map(|(_, preview)| *preview));
    let mut content = widget::column![widget::text(stored)].spacing(4);
    if let Some(preview) = preview {
        content = content.push(scheme_chip_strip(preview));
    }
    let control: Element<'static, SettingsEvent> = widget::button::custom(content)
        .on_press(SettingsEvent::TogglePicker(PickerId::Scheme(
            SchemePickerId::Global(key),
        )))
        .into();
    Element::new(SchemePicker {
        id: SchemePickerId::Global(key),
        selected,
        inherit: None,
        open,
        control,
    })
}

/// The profile form's scheme picker control — the shared picker with
/// the override's vocabulary: the closed control names the picked
/// scheme (or "Inherit") over its strip, and an inherit preview is
/// the global scheme resolved for the current system mode — the
/// colors a save-less inherit actually renders, the closed control's
/// honest-preview rule. An unknown stored name shows verbatim with
/// no strip and marks no row (the global control's unknown rule).
#[must_use]
pub(super) fn profile_scheme_control(
    config: &DropTermConfig,
    draft: &ProfileDraft,
    open: bool,
    is_dark: bool,
) -> Element<'static, SettingsEvent> {
    let inherit = inherit_scheme_preview(config, is_dark);
    let label = match draft.scheme.as_deref() {
        Some(name) => name.to_string(),
        None => SCHEME_INHERIT_ROW.to_string(),
    };
    let preview = match draft.scheme.as_deref() {
        Some(name) => scheme_preview_by_name(name),
        None => inherit,
    };
    let mut content = widget::column![widget::text(label)].spacing(4);
    if let Some(preview) = preview {
        content = content.push(scheme_chip_strip(preview));
    }
    let control: Element<'static, SettingsEvent> = widget::button::custom(content)
        .on_press(SettingsEvent::TogglePicker(PickerId::Scheme(
            SchemePickerId::ProfileForm,
        )))
        .into();
    Element::new(SchemePicker {
        id: SchemePickerId::ProfileForm,
        selected: Some(profile_scheme_selected(draft.scheme.as_deref())),
        inherit,
        open,
        control,
    })
}

/// The scheme picker host: lays out, updates, and draws exactly its
/// closed control (the delegation pattern of libcosmic's `Tooltip`),
/// and — while the app holds the picker open — hosts the popover as
/// its overlay, anchored to the control's translated bounds so it
/// tracks the control inside the drawer's scrollable. `id` picks the
/// popover's row model (registry-only, or registry behind a leading
/// Inherit row); `inherit` is the Inherit row's resolved preview,
/// `None` for the global pickers and for an unresolvable global.
struct SchemePicker<'a> {
    id: SchemePickerId,
    selected: Option<usize>,
    inherit: Option<SchemePreview>,
    open: bool,
    control: Element<'a, SettingsEvent>,
}

impl Widget<SettingsEvent, Theme, Renderer> for SchemePicker<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<SchemePickerState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(SchemePickerState::new())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.control)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut [self.control.as_widget_mut()]);
        debug_assert_eq!(
            tree.state.downcast_ref::<SchemePickerState>().names.len(),
            scheme_names().len(),
            "the registry is compile-time; the paragraph cache cannot drift"
        );
    }

    fn size(&self) -> Size<Length> {
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
        Some(overlay::Element::new(Box::new(SchemePopover {
            state: tree.state.downcast_mut::<SchemePickerState>(),
            id: self.id,
            selected: self.selected,
            inherit: self.inherit,
            anchor: Rectangle::new(bounds.position() + translation, bounds.size()),
        })))
    }
}

/// The open scheme picker's popover: one row per registry scheme —
/// name plus its resolved eighteen colors — in a dropdown-styled
/// bubble anchored below the control (above it when the space below
/// runs out, the stock menu's rule); the profile form's variant
/// carries one leading Inherit row in front of the registry,
/// previewing the global scheme resolved for the current system
/// mode. Dismissal is the dropdown's own contract: a left press on a
/// row picks it and closes, any other press closes without picking,
/// every press is captured, and Escape closes. The marker is the
/// stock menu's selected treatment: the picked row fills and its
/// name carries the accent.
struct SchemePopover<'a> {
    state: &'a mut SchemePickerState,
    id: SchemePickerId,
    selected: Option<usize>,
    inherit: Option<SchemePreview>,
    /// The control's absolute bounds (translation applied).
    anchor: Rectangle,
}

impl SchemePopover<'_> {
    /// The row under the point, if any.
    fn row_at(bounds: Rectangle, point: Point) -> Option<usize> {
        if !bounds.contains(point) {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "row indices are far below 2^24, so the f32 floor is exact and non-negative inside the bounds"
        )]
        Some(((point.y - bounds.y) / SCHEME_ROW_HEIGHT) as usize)
    }

    /// The popover's row count: the registry, plus the profile
    /// form's leading Inherit row.
    fn rows(&self) -> usize {
        scheme_previews().len() + self.registry_offset()
    }

    /// The registry's first row index — 0 for the global pickers,
    /// one past the Inherit row for the profile form's.
    const fn registry_offset(&self) -> usize {
        match self.id {
            SchemePickerId::Global(_) => 0,
            SchemePickerId::ProfileForm => 1,
        }
    }

    /// The pick event a left press on `row` publishes — the global
    /// pickers' `Picked` in registry space (row space, offset 0) and
    /// the profile form's `PickProfileScheme` in its own row space
    /// (row 0 = Inherit), the spaces their planners already map.
    const fn pick_event(&self, row: usize) -> SettingsEvent {
        match self.id {
            SchemePickerId::Global(key) => SettingsEvent::Picked(key, row),
            SchemePickerId::ProfileForm => SettingsEvent::PickProfileScheme(row),
        }
    }

    /// One row's paint inputs: the marker against `hovered`, the
    /// cached label, and the strip.
    fn row_view<'label>(
        &self,
        row: usize,
        hovered: Option<usize>,
        label: &'label SchemeNameParagraph,
        strip: Option<SchemePreview>,
    ) -> SchemeRowView<'label> {
        let mark = if self.selected == Some(row) {
            RowMark::Selected
        } else if hovered == Some(row) {
            RowMark::Hovered
        } else {
            RowMark::Plain
        };
        SchemeRowView { mark, label, strip }
    }
}

impl overlay::Overlay<SettingsEvent, Theme, Renderer> for SchemePopover<'_> {
    fn layout(&mut self, _renderer: &Renderer, bounds: Size) -> layout::Node {
        let mut widest = self
            .state
            .names
            .iter()
            .map(SchemeNameParagraph::min_width)
            .fold(0.0, f32::max);
        if self.registry_offset() > 0 {
            widest = widest.max(self.state.inherit.min_width());
        }
        let content = SCHEME_POPOVER_PAD_X * 2.0 + 24.0 + widest + 8.0 + scheme_strip_width();
        let width = content
            .max(self.anchor.width)
            .min((bounds.width - self.anchor.x).max(0.0));
        let space_below = bounds.height - (self.anchor.y + self.anchor.height);
        let space_above = self.anchor.y;
        let below = space_below >= space_above;
        #[expect(
            clippy::cast_precision_loss,
            reason = "row count is far below 2^24, so usize -> f32 is exact"
        )]
        let height = (self.rows() as f32 * SCHEME_ROW_HEIGHT).min(if below {
            space_below
        } else {
            space_above
        });
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
            // The dropdown's dismissal contract, unchanged from the
            // tab-bar menus: a left press on a row first publishes
            // its pick — the identical event, the identical
            // row-space index the stock dropdown emitted for the
            // global pickers, the profile form's own space otherwise
            // — then everything closes, and the press is captured so
            // nothing under the bubble acts on it.
            Event::Mouse(MouseEvent::ButtonPressed(button)) => {
                if *button == Button::Left
                    && let Some(index) = cursor
                        .position()
                        .and_then(|point| Self::row_at(layout.bounds(), point))
                    && index < self.rows()
                {
                    shell.publish(self.pick_event(index));
                }
                shell.publish(SettingsEvent::ClosePicker);
                shell.capture_event();
                shell.request_redraw();
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
        // The stock dropdown container's own surface: component
        // background, hairline divider, small radius — cohesion with
        // every other dropdown in the drawer.
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
            .and_then(|point| Self::row_at(menu, point));
        let offset = self.registry_offset();
        // The profile form's leading row: the Inherit label over the
        // global scheme resolved for the current system mode — the
        // palette an unset override actually renders.
        if offset > 0 {
            draw_scheme_row(
                renderer,
                theme,
                style,
                menu,
                0,
                self.row_view(0, hovered, &self.state.inherit, self.inherit),
            );
        }
        for (index, (_, preview)) in scheme_previews().iter().enumerate() {
            let row = offset + index;
            draw_scheme_row(
                renderer,
                theme,
                style,
                menu,
                row,
                self.row_view(row, hovered, &self.state.names[index], Some(*preview)),
            );
        }
    }
}

/// One popover row's marker state: the stock menu's picked-row
/// treatment, the same fill at whisper alpha on hover, or plain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowMark {
    Selected,
    Hovered,
    Plain,
}

/// One popover row's paint inputs: the marker, the cached label,
/// and the previewed strip (`None` renders the label alone — the
/// Inherit row whose global name does not resolve).
#[derive(Debug, Clone, Copy)]
struct SchemeRowView<'a> {
    mark: RowMark,
    label: &'a SchemeNameParagraph,
    strip: Option<SchemePreview>,
}

/// One popover row's paint: the marker fills, the eighteen-chip
/// strip, and the cached label in the row's accent-or-body color —
/// every popover row (registry and Inherit alike) renders through
/// here.
fn draw_scheme_row(
    renderer: &mut Renderer,
    theme: &Theme,
    style: &renderer::Style,
    menu: Rectangle,
    row: usize,
    view: SchemeRowView<'_>,
) {
    let SchemeRowView { mark, label, strip } = view;
    let selected = matches!(mark, RowMark::Selected);
    let cosmic = theme.cosmic();
    let accent: Color = cosmic.accent.base.into();
    #[expect(
        clippy::cast_precision_loss,
        reason = "row count is far below 2^24, so usize -> f32 is exact"
    )]
    let row_y = menu.y + row as f32 * SCHEME_ROW_HEIGHT;
    let bounds = Rectangle::new(
        Point::new(menu.x, row_y),
        Size::new(menu.width, SCHEME_ROW_HEIGHT),
    );
    if selected {
        renderer.fill_quad(
            Quad {
                bounds,
                snap: true,
                ..Default::default()
            },
            Color { a: 0.25, ..accent },
        );
    } else if matches!(mark, RowMark::Hovered) {
        renderer.fill_quad(
            Quad {
                bounds,
                snap: true,
                ..Default::default()
            },
            Color { a: 0.12, ..accent },
        );
    }
    let text_color = if selected { accent } else { style.text_color };
    if let Some(preview) = strip {
        // The chip strip: background, foreground, then the ANSI
        // sixteen — the palette is the preview. The background chip
        // carries a hairline divider border, the one chip that can
        // otherwise vanish against the bubble's own surface (OQ-2's
        // rule).
        let chip_y = menu.y + (SCHEME_ROW_HEIGHT - SCHEME_CHIP) / 2.0;
        let strip_x = menu.x + menu.width - SCHEME_POPOVER_PAD_X - scheme_strip_width();
        for (chip_index, color) in std::iter::once(preview.background)
            .chain(std::iter::once(preview.foreground))
            .chain(preview.ansi.iter().copied())
            .enumerate()
        {
            #[expect(
                clippy::cast_precision_loss,
                reason = "chip count is far below 2^24, so usize -> f32 is exact"
            )]
            let chip_x = strip_x + chip_index as f32 * (SCHEME_CHIP + SCHEME_CHIP_GAP);
            let chip = Rectangle::new(
                Point::new(chip_x, chip_y),
                Size::new(SCHEME_CHIP, SCHEME_CHIP),
            );
            let color = color.map_or(Color::from(cosmic.bg_component_color()), |rgb| {
                Color::from_rgb8(rgb.r, rgb.g, rgb.b)
            });
            renderer.fill_quad(
                Quad {
                    bounds: chip,
                    border: if chip_index == 0 {
                        Border {
                            width: 1.0,
                            color: cosmic.bg_component_divider().into(),
                            radius: 2.0.into(),
                        }
                    } else {
                        Border {
                            radius: 2.0.into(),
                            ..Default::default()
                        }
                    },
                    snap: true,
                    ..Default::default()
                },
                color,
            );
        }
    }
    let text_y = bounds.y + (bounds.height - label.min_height()) / 2.0;
    renderer.fill_paragraph(
        label.raw(),
        Point::new(bounds.x + SCHEME_POPOVER_PAD_X, text_y),
        text_color,
        menu,
    );
}
#[cfg(test)]
mod tests {
    use super::*;

    // --- the scheme pickers (scheme-picker T2) ---

    #[test]
    fn the_scheme_picker_control_builds_in_every_config_state() {
        // Construction is the assertion for both halves: the closed
        // control (name + mini chip strip) and the popover host it
        // becomes while open — both keys, open and closed, over a
        // default, set, and unknown stored scheme name.
        let mut config = DropTermConfig::default();
        for open in [false, true] {
            let _ = scheme_picker_control(ConfigKey::ColorSchemeDark, &config, open);
            let _ = scheme_picker_control(ConfigKey::ColorSchemeLight, &config, open);
        }
        config.color_scheme_dark = "Dracula".to_string();
        config.color_scheme_light = "Not A Scheme".to_string();
        for open in [false, true] {
            let _ = scheme_picker_control(ConfigKey::ColorSchemeDark, &config, open);
            let _ = scheme_picker_control(ConfigKey::ColorSchemeLight, &config, open);
        }
    }
}
