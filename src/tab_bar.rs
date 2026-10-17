// SPDX-License-Identifier: GPL-3.0-only

//! The tab bar strip: one button per tab — title plus a close `×` —
//! with the buttons shown only while more than one tab exists. The
//! strip itself is always present, carrying the new-tab `+` and the
//! settings gear at its right corner in every tab state.
//!
//! The button layout and hit-testing are pure functions over plain
//! geometry (like the list operations in [`crate::tabs`]), so which
//! tab a click or middle-click lands on is unit-testable headless;
//! rendering needs real COSMIC hardware (README manual checklist).
//!
//! Inline rename: a double-clicked tab body swaps its button for a
//! real text input, hosted as an overlay exactly over that button
//! (the delegation pattern of libcosmic's popover, pinned rev
//! `08a09e3`, `widget/popover.rs`). The input is built by the app
//! and passed in via [`TabBar::renaming`]; the bar only positions
//! it, classifies the double-click, and reports presses outside the
//! input so the app can end the session.
//!
//! Drag reorder: a left press on a tab body that moves past a small
//! threshold drags the tab horizontally. iced has no first-class
//! drag API, so the bar tracks press/move/release itself — the state
//! machine of iced's own text editor (pinned rev `08a09e3`,
//! `widget/text_editor.rs`). While the drag lasts, the pressed
//! button follows the cursor (clamped to the row, lifted and drawn
//! on top) and every other button eases to the slot the drop would
//! leave it in — the feedback model of libcosmic's
//! `reorderable_flex_row` (same rev), driven by the same
//! redraw-request loop. [`displaced_row`], [`followed_x`] and
//! [`drop_index`](reorder::drop_index) decide all of it from plain geometry; the release
//! reports the drop, which the app applies via
//! [`crate::tabs::reorder_tabs`].
//!
//! Colors mirror the `TabBar` preset of libcosmic's own
//! `theme/style/segmented_button.rs` (pinned rev `08a09e3`): the
//! active tab is `neutral_5` at 0.2 alpha with the accent text color
//! and `radius_s` corners, inactive tabs carry no background and use
//! the component's `on` text color.

use std::collections::HashMap;

use cosmic::iced::core::alignment;
use cosmic::iced::core::mouse::click;
use cosmic::iced::core::overlay;
use cosmic::iced::core::text::{Ellipsize, LineHeight, Shaping, Wrapping, paragraph};
use cosmic::iced::core::{
    Border, Shadow, Shell, Text,
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
    window,
};
use cosmic::theme::Theme;
use cosmic::{Element, Renderer, cosmic_theme::palette::WithAlpha};

use crate::terminal::WINDOW_BG_COLOR;

use crate::profile::Profile;
use crate::tabs::{MoveDirection, Tab, TabId, title_prefix};

mod reorder;

use self::reorder::{Drag, SlotAnimation, displaced_row, drop_from_followed, followed_x};

/// The bar's background and a text color readable on it.
///
/// The strip is painted with the terminal pane's effective background
/// ([`WINDOW_BG_COLOR`]) instead of relying on the window clear color:
/// the two can disagree when the COSMIC system theme and the terminal
/// color scheme diverge, which left the bar as an unreadable foreign
/// strip that users read as window chrome rather than tabs. Text picks
/// near-black or near-white by the background's luminance so labels and
/// close glyphs stay readable in either scheme.
pub(crate) fn pane_colors() -> (Color, Color) {
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

/// The alpha of the bars' border hairlines — the pane foreground at
/// low strength, hierarchy by alpha, never a new hue (bar-borders
/// spec). The menu separators' own value; retunable at the hardware
/// pass's light/dark rows.
pub(crate) const BORDER_ALPHA: f32 = 0.3;

/// The border hairline's rect: 1 px tall, the strip's full width,
/// at the named vertical edge of `bounds` — the shared shape of the
/// tab strip's and the status band's border lines. Painted inside
/// each strip's own bounds, so no line ever moves layout. Degenerate
/// bounds floor to a zero-height (never negative) rect.
pub(crate) fn border_rect(bounds: Rectangle, side: TooltipSide) -> Rectangle {
    let y = match side {
        TooltipSide::Above => bounds.y,
        TooltipSide::Below => bounds.y + bounds.height - 1.0,
    };
    Rectangle::new(
        Point::new(bounds.x, y),
        Size::new(bounds.width.max(0.0), bounds.height.clamp(0.0, 1.0)),
    )
}

/// Fills `bounds` as a disc — a quad rounded by half its smaller side —
/// the shared shape of the gear's parts, the settings hover disc, and
/// the status band's filled process-state disc (status-bar spec).
pub(crate) fn fill_disc(renderer: &mut Renderer, bounds: Rectangle, color: Color) {
    renderer.fill_quad(
        Quad {
            bounds,
            border: Border {
                radius: (bounds.width.min(bounds.height) / 2.0).into(),
                width: 0.0,
                color: Color::TRANSPARENT,
            },
            snap: true,
            ..Default::default()
        },
        color,
    );
}

/// Draws a `×` inside `close` as squares marched along both diagonals.
///
/// The mark spans the middle of the target with a small inset, so it
/// clears the hover disc's edge; `thickness` and arm length scale with
/// the target size.
fn draw_close_mark(renderer: &mut Renderer, close: Rectangle, color: Color) {
    let thickness = (close.width / 9.0).max(2.0);
    let half = close.width / 2.0 - close.width / 4.5;
    let (cx, cy) = (close.x + close.width / 2.0, close.y + close.height / 2.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "half is a non-negative px radius derived from the target width; \
                  truncating to a step count is the draw contract"
    )]
    let steps = half as usize;
    for step in 0..=steps.max(1) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "step and steps are px-scale counts (< 2^24), so usize -> f32 is exact"
        )]
        let t = -half + 2.0 * half * (step as f32 / steps.max(1) as f32);
        for dx in [t, -t] {
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle::new(
                        Point::new(cx + dx - thickness / 2.0, cy + t - thickness / 2.0),
                        Size::new(thickness, thickness),
                    ),
                    border: Border {
                        radius: 0.0.into(),
                        width: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    snap: false,
                    ..Default::default()
                },
                color,
            );
        }
    }
}

/// Named through the iced core because libcosmic keeps its own `Plain`
/// alias private (pinned rev `08a09e3`).
type TitleParagraph = paragraph::Plain<<Renderer as cosmic::iced::core::text::Renderer>::Paragraph>;

/// Bar geometry in logical pixels. Constants rather than theme values so
/// the pure layout functions stay deterministic and testable.
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
/// much while the bar is visible — see `geometry::surface_height`).
pub const BAR_HEIGHT: f32 = METRICS.bar_height;

/// Title text size in logical pixels.
const TITLE_FONT_SIZE: f32 = 14.0;

/// One constant tooltip label's [`Text`] layout — the title
/// paragraphs' settings (see [`TabBar::refresh_paragraphs`]) for the
/// strip's two fixed tips.
fn label_text(content: &str, font: cosmic::iced::Font) -> Text<&str> {
    Text {
        content,
        bounds: Size::INFINITE,
        size: Pixels(TITLE_FONT_SIZE),
        line_height: LineHeight::default(),
        font,
        align_x: cosmic::iced::core::text::Alignment::Left,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
        wrapping: Wrapping::default(),
        ellipsize: Ellipsize::None,
    }
}

/// How far the cursor must travel horizontally from the press before a
/// held press on a tab body counts as a drag rather than a click.
/// Matches the movement threshold of iced's own click classification
/// (pinned rev `08a09e3`, `mouse/click.rs`: `distance < 6.0`).
const DRAG_THRESHOLD: f32 = 6.0;

/// How far the dragged button lifts out of the row while it follows
/// the cursor, in logical pixels.
const DRAG_LIFT: f32 = 2.0;

/// Width of the drop indicator line, in logical pixels.
const DROP_INDICATOR_WIDTH: f32 = 2.0;

/// Gear proportions as fractions of the settings target's side:
/// the ring's outer radius (also the radius the tooth centers ride),
/// the punched hole's radius, and the teeth's side length.
const GEAR_BODY_RADIUS: f32 = 0.29;
const GEAR_HOLE_RADIUS: f32 = 0.125;
const GEAR_TOOTH_SIZE: f32 = 0.167;
const GEAR_TEETH: usize = 8;

/// The settings tooltip's label: the same text as the context menu's
/// item, ellipsis included (U+2026).
const SETTINGS_LABEL: &str = "Settings\u{2026}";

/// The new-tab button's tooltip label — and the profile dropdown's
/// plain row 0 when it opens (the context-menu submenu's wording).
const NEW_TAB_LABEL: &str = "New Tab";

/// Tooltip geometry, logical pixels: tip height, label padding on each
/// side, and the gap left between the strip and the popped tip.
const TOOLTIP_HEIGHT: f32 = 24.0;
const TOOLTIP_PADDING_X: f32 = 8.0;
const TOOLTIP_GAP: f32 = 4.0;

/// Dropdown geometry, logical pixels: row height, label padding on
/// each side, and the gap left between the strip and the popped
/// dropdown (row metrics are OQ-2, ratified at the hardware STOP).
/// Shared by every hand-drawn menu bubble on the surface — the
/// per-tab menu and the status band's menu ride them too, so one
/// idiom keeps the bubbles pixel-identical.
pub const DROPDOWN_ROW_HEIGHT: f32 = 28.0;
pub const DROPDOWN_PADDING_X: f32 = 10.0;
pub const DROPDOWN_GAP: f32 = 4.0;

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

/// The settings button's target: a square the size of a close target,
/// anchored at the strip's right edge with the button inset, vertically
/// centered — the right corner in both edge configs. Takes no tab
/// count: the identical rect whether the strip carries a settings
/// button alone or a full tab row.
fn settings_target(bar_width: f32, metrics: &BarMetrics) -> Rectangle {
    Rectangle::new(
        Point::new(
            bar_width - metrics.button_padding_x - metrics.close_size,
            (metrics.bar_height - metrics.close_size) / 2.0,
        ),
        Size::new(metrics.close_size, metrics.close_size),
    )
}

/// The new-tab button's target: a square the size of a close target,
/// anchored one gap left of the settings target — beside the gear at
/// the strip's right corner in both edge configs. Takes no tab count
/// (like [`settings_target`]): the identical rect whether the strip
/// carries the corner buttons alone or a full tab row.
fn newtab_target(bar_width: f32, metrics: &BarMetrics) -> Rectangle {
    let gear = settings_target(bar_width, metrics);
    Rectangle::new(
        Point::new(gear.x - metrics.button_gap - metrics.close_size, gear.y),
        Size::new(metrics.close_size, metrics.close_size),
    )
}

/// The strip's right-hand zone the tab row must keep clear of:
/// inset + `+` + gap + gear + gap — both corner targets and the gaps
/// around them, so the row ends a button-gap left of the `+`.
fn corner_zone(metrics: &BarMetrics) -> f32 {
    metrics.button_padding_x + metrics.close_size * 2.0 + metrics.button_gap * 2.0
}

/// The width the tab row may occupy: the bar minus the corner zone.
/// [`TabBar::button_geometry`] feeds this to [`layout_buttons`] — the
/// one seam the drag clamp, drop indicator, rename overlay, and hit
/// tests already share, so every consumer stays clear of the zone
/// with no math of their own to edit.
fn row_budget(bar_width: f32, metrics: &BarMetrics) -> f32 {
    (bar_width - corner_zone(metrics)).max(0.0)
}

/// Whether the tab row renders its buttons: the strip is always
/// present, but tab buttons — and their close marks — only from two
/// tabs on; the single-tab strip carries the settings button alone.
fn shows_tab_buttons(tab_count: usize) -> bool {
    tab_count >= 2
}

/// The vertical side of the strip the settings tooltip pops to:
/// the terminal's side, so the tip clears the strip into open surface
/// instead of off the layer surface's edge. The app derives it from
/// the configured edge — the bar itself stays edge-agnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TooltipSide {
    /// The terminal sits below the strip: the tip pops downward.
    #[default]
    Below,
    /// The terminal sits above the strip: the tip pops upward.
    Above,
}

/// The settings gear's quads (see [`gear_glyph`]).
#[derive(Debug)]
struct GearGlyph {
    /// The ring's outer disc.
    body: Rectangle,
    /// The hole punched through the body, refilled with the strip's
    /// current background (see [`composite`]).
    punch: Rectangle,
    /// The teeth, rotationally placed around the body.
    teeth: Vec<Rectangle>,
}

/// The gear glyph's quads: a ring — a disc punched by a strip-colored
/// disc — plus [`GEAR_TEETH`] teeth, all axis-aligned rounded quads,
/// font-free like the close `×`. Pure geometry: every quad is
/// contained in `target`, and the teeth are N-fold rotationally
/// symmetric about its center (equal angular spacing, identical tooth
/// geometry).
fn gear_glyph(target: Rectangle) -> GearGlyph {
    let side = target.width.min(target.height);
    let center = target.center();
    let disc = |radius: f32| {
        Rectangle::new(
            Point::new(center.x - radius, center.y - radius),
            Size::new(radius * 2.0, radius * 2.0),
        )
    };
    let ring = GEAR_BODY_RADIUS * side;
    let tooth_side = GEAR_TOOTH_SIZE * side;
    let teeth = (0..GEAR_TEETH)
        .map(|tooth| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "tooth < GEAR_TEETH = 8 and GEAR_TEETH = 8; both usize -> f32 conversions are exact"
            )]
            let angle = tooth as f32 * (std::f32::consts::TAU / GEAR_TEETH as f32);
            let (dx, dy) = (angle.cos() * ring, angle.sin() * ring);
            Rectangle::new(
                Point::new(
                    center.x + dx - tooth_side / 2.0,
                    center.y + dy - tooth_side / 2.0,
                ),
                Size::new(tooth_side, tooth_side),
            )
        })
        .collect();
    GearGlyph {
        body: disc(ring),
        punch: disc(GEAR_HOLE_RADIUS * side),
        teeth,
    }
}

/// The `+` glyph's two bars (see [`plus_glyph`]).
#[derive(Debug)]
struct PlusGlyph {
    /// The horizontal bar.
    horizontal: Rectangle,
    /// The vertical bar.
    vertical: Rectangle,
}

/// The `+` glyph: two axis-aligned crossing bars sharing the target's
/// center, with thickness and arm length scaled from the target
/// exactly like the close mark's `×` (see [`draw_close_mark`]).
/// Pure geometry: both bars are contained in `target` and cross at
/// its center.
fn plus_glyph(target: Rectangle) -> PlusGlyph {
    let side = target.width.min(target.height);
    let thickness = (side / 9.0).max(2.0);
    let arm = side / 2.0 - side / 4.5;
    let center = target.center();
    let horizontal = Rectangle::new(
        Point::new(center.x - arm, center.y - thickness / 2.0),
        Size::new(arm * 2.0, thickness),
    );
    let vertical = Rectangle::new(
        Point::new(center.x - thickness / 2.0, center.y - arm),
        Size::new(thickness, arm * 2.0),
    );
    PlusGlyph {
        horizontal,
        vertical,
    }
}

/// One profile-dropdown row: the label to draw and which launch a
/// pick runs — the plain default spawn (row 0) or the profile at
/// this config index. Plain data, the `context_menu::rows`
/// precedent, so the composition is unit-testable without a
/// renderer.
struct DropdownRow {
    /// Label drawn in the row.
    label: String,
    /// The launch a pick publishes: `None` = the default spawn
    /// (`Action::NewTab` semantics), `Some(index)` =
    /// `Action::NewProfileTab(index)`, resolved at apply time.
    profile: Option<usize>,
}

/// The dropdown's rows: row 0 is the plain `New Tab` (the default
/// spawn — same semantics as the `+`'s left click), then one row per
/// profile in config order — the context-menu submenu's composition.
/// An empty profile list yields no rows: zero profiles, no dropdown
/// (the flat-menu ruling).
fn dropdown_rows(profiles: &[Profile]) -> Vec<DropdownRow> {
    if profiles.is_empty() {
        return Vec::new();
    }
    let mut rows = vec![DropdownRow {
        label: NEW_TAB_LABEL.to_owned(),
        profile: None,
    }];
    rows.extend(
        profiles
            .iter()
            .enumerate()
            .map(|(index, profile)| DropdownRow {
                label: profile.name.clone(),
                profile: Some(index),
            }),
    );
    rows
}

/// The dropdown's rect, bar-local: right edge pinned at the strip's
/// right corner with growth entirely leftward (the tooltip rule, so
/// it can never overflow the surface's right edge), width from the
/// widest row label plus padding, rows stacked on
/// [`DROPDOWN_ROW_HEIGHT`], popping to the terminal's side of the
/// strip (`side`, the tooltip derivation reused).
fn dropdown_bounds(
    bar_width: f32,
    widest_label: f32,
    row_count: usize,
    metrics: &BarMetrics,
    side: TooltipSide,
) -> Rectangle {
    let width = widest_label + DROPDOWN_PADDING_X * 2.0;
    #[expect(
        clippy::cast_precision_loss,
        reason = "row count is far below 2^24, so usize -> f32 is exact"
    )]
    let height = row_count as f32 * DROPDOWN_ROW_HEIGHT;
    let y = match side {
        TooltipSide::Below => metrics.bar_height + DROPDOWN_GAP,
        TooltipSide::Above => -height - DROPDOWN_GAP,
    };
    Rectangle::new(Point::new(bar_width - width, y), Size::new(width, height))
}

/// Which menu-bubble row a point lands on, `None` outside the
/// bubble's rect: the pure press classification behind dismissal —
/// a row pick and an away-press close both reduce to this. The
/// per-tab menu and the status band's menu ride it unchanged.
pub fn dropdown_row_at(dropdown: Rectangle, point: Point) -> Option<usize> {
    if !dropdown.contains(point) {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a contained point is inside the rect, so the offset is a \
                  non-negative row multiple; truncation is the floor the row index is"
    )]
    Some(((point.y - dropdown.y) / DROPDOWN_ROW_HEIGHT) as usize)
}

/// One pick the per-tab context menu can publish: the verb a row
/// carries, published with the right-pressed tab's id. Every variant
/// routes to an existing funnel (see [`tab_menu_rows`]) — the menu
/// introduces no behavior of its own, the `context_menu::Item`
/// contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabMenuPick {
    /// The default spawn — the `+`'s left click and `Ctrl+Shift+T`
    /// funnel: default profile command, cwd inheritance,
    /// `new_tab_after_current` placement.
    NewTab,
    /// Inline rename of the clicked tab — the double-click and
    /// `Ctrl+Shift+R` path, id-addressed; no activation side effect
    /// (the remote `rename-tab` precedent).
    Rename,
    /// Duplicate the clicked tab — the remote `duplicate-tab
    /// --tab-index` semantics: cwd and rename pin read from it,
    /// after-active landing, the duplicate activated.
    Duplicate,
    /// Flip the clicked tab's pin — the row's label names the action
    /// in store ("Pin Tab" loose, "Unpin Tab" pinned). Menu-only by
    /// ruling: no keybind, no terminal-menu row.
    Pin,
    /// Respawn the clicked tab under the profile at this config index
    /// — the Switch Profile folder's row, apply-time resolved (the
    /// `NewProfileTab` stale-index contract).
    SwitchProfile(usize),
    /// Nudge the clicked tab one slot toward the direction, clamping
    /// at the edge — the move binds' move with the clicked tab in
    /// place of the active one.
    Move(MoveDirection),
    /// Close the clicked tab through the gated front door — the
    /// confirmation holds like every user-initiated close.
    Close,
    /// Close every tab but the clicked one, through the same gate.
    CloseOthers,
    /// Close every tab right of the clicked one, through the same
    /// gate.
    CloseRight,
}

/// One rendered row of the per-tab context menu: its label, the pick a
/// left press publishes (`None` for the Switch Profile section
/// header), whether it renders active — edge-clamped moves and the
/// header render muted, the GNOME disabled-row rule — and whether a
/// separator line renders above it. Plain data beside
/// [`dropdown_rows`], so the composition is unit-testable without a
/// renderer.
struct TabMenuRow {
    /// Label drawn in the row.
    label: String,
    /// The pick a left press publishes, `None` for the section header
    /// (dismissed, never published).
    pick: Option<TabMenuPick>,
    /// Whether the row renders active and publishes its pick.
    enabled: bool,
    /// Whether a separator line renders above the row.
    separated: bool,
}

/// An interactive row.
fn pick_row(label: &str, pick: TabMenuPick, enabled: bool, separated: bool) -> TabMenuRow {
    TabMenuRow {
        label: label.to_owned(),
        pick: Some(pick),
        enabled,
        separated,
    }
}

/// The Switch Profile section header: muted, non-clickable, separated
/// from the rows above it.
fn header_row(label: &str) -> TabMenuRow {
    TabMenuRow {
        label: label.to_owned(),
        pick: None,
        enabled: false,
        separated: true,
    }
}

/// The per-tab context menu's rows for the tab at `tab_index` of
/// `tab_count`, `pinned` naming that tab's pin state — the
/// consolidation surface: every row rides an existing funnel, scoped
/// to the clicked tab rather than the active one. New Tab leads
/// (every peer's tab menu opens with it); the clicked-tab cluster
/// follows, Pin/Unpin in it after Duplicate Tab with the label
/// naming the flip in store; the profile section renders exactly
/// while ≥1 profile exists (the context menu's zero-profiles flat
/// ruling), profiles surfacing here exactly once; moves mute at the
/// edges they clamp at and — a pinned tab staying put (ruling 4) —
/// for a pinned target, whose own Close Tab row mutes with them (the
/// close front door's refusal, rendered); the close cluster closes,
/// with Close Tabs to the Right omitted at the rightmost tab
/// (Firefox's hide rule — the row would always no-op there).
fn tab_menu_rows(
    profiles: &[Profile],
    tab_count: usize,
    tab_index: usize,
    pinned: bool,
) -> Vec<TabMenuRow> {
    let mut rows = vec![pick_row(NEW_TAB_LABEL, TabMenuPick::NewTab, true, false)];
    rows.push(pick_row("Rename Tab", TabMenuPick::Rename, true, true));
    rows.push(pick_row(
        "Duplicate Tab",
        TabMenuPick::Duplicate,
        true,
        false,
    ));
    rows.push(pick_row(
        if pinned { "Unpin Tab" } else { "Pin Tab" },
        TabMenuPick::Pin,
        true,
        false,
    ));
    if !profiles.is_empty() {
        rows.push(header_row("Switch Profile"));
        rows.extend(profiles.iter().enumerate().map(|(index, profile)| {
            pick_row(
                &profile.name,
                TabMenuPick::SwitchProfile(index),
                true,
                false,
            )
        }));
    }
    // Moves clamp at the edges (the binds' `move_target` rule); the
    // out-of-room direction renders muted. A pinned tab refuses moves
    // outright (ruling 4), so both directions mute for it — a pick
    // `move_tab_by` would refuse is never a clickable row.
    rows.push(pick_row(
        "Move Tab Left",
        TabMenuPick::Move(MoveDirection::Left),
        !pinned && tab_index > 0,
        true,
    ));
    rows.push(pick_row(
        "Move Tab Right",
        TabMenuPick::Move(MoveDirection::Right),
        !pinned && tab_index + 1 < tab_count,
        false,
    ));
    // The anchor's own close rides the gated front door, which refuses
    // a pinned tab (ruling 2): muted here rather than a clickable
    // no-op, with Unpin Tab in arm's reach above it. The sweeps aim at
    // the *other* tabs and stay live — pinned neighbors drop out of
    // their target lists silently (the OQ-2 default).
    rows.push(pick_row("Close Tab", TabMenuPick::Close, !pinned, true));
    rows.push(pick_row(
        "Close Other Tabs",
        TabMenuPick::CloseOthers,
        true,
        false,
    ));
    if tab_index + 1 < tab_count {
        rows.push(pick_row(
            "Close Tabs to the Right",
            TabMenuPick::CloseRight,
            true,
            false,
        ));
    }
    rows
}

/// The per-tab menu's rect, bar-local: popping to the terminal's side
/// of the strip at the clicked button — left edge at the button's,
/// clamped so the bubble never overflows either end of the strip (the
/// dropdown's right-edge rule, both edges) — width from the widest row
/// label plus padding, rows stacked on [`DROPDOWN_ROW_HEIGHT`] (the
/// dropdown's metrics, OQ-2).
fn tab_menu_bounds(
    bar_width: f32,
    anchor_x: f32,
    widest_label: f32,
    row_count: usize,
    metrics: &BarMetrics,
    side: TooltipSide,
) -> Rectangle {
    let width = widest_label + DROPDOWN_PADDING_X * 2.0;
    #[expect(
        clippy::cast_precision_loss,
        reason = "row count is far below 2^24, so usize -> f32 is exact"
    )]
    let height = row_count as f32 * DROPDOWN_ROW_HEIGHT;
    let x = anchor_x.clamp(0.0, (bar_width - width).max(0.0));
    let y = match side {
        TooltipSide::Below => metrics.bar_height + DROPDOWN_GAP,
        TooltipSide::Above => -height - DROPDOWN_GAP,
    };
    Rectangle::new(Point::new(x, y), Size::new(width, height))
}

/// The settings tooltip's rect, bar-local: the right edge pinned at
/// `right_edge` whatever the label's width — growth is entirely
/// leftward, so the tip can never overflow the edge it anchors to —
/// and vertically clearing the strip toward `side` (the terminal's
/// side). The gear passes the strip's width as `right_edge`; the `+`
/// passes its own target's right edge, inboard of the corner.
fn tooltip_bounds(
    right_edge: f32,
    label_width: f32,
    metrics: &BarMetrics,
    side: TooltipSide,
) -> Rectangle {
    let width = label_width + TOOLTIP_PADDING_X * 2.0;
    let y = match side {
        TooltipSide::Below => metrics.bar_height + TOOLTIP_GAP,
        TooltipSide::Above => -TOOLTIP_HEIGHT - TOOLTIP_GAP,
    };
    Rectangle::new(
        Point::new(right_edge - width, y),
        Size::new(width, TOOLTIP_HEIGHT),
    )
}

/// `over` blended onto an opaque `base`: the exact pixel a translucent
/// quad leaves on the strip, used to punch the gear's hole over the
/// hover disc so the ring reads identically hovered or not.
fn composite(over: Color, base: Color) -> Color {
    Color {
        r: over.r * over.a + base.r * (1.0 - over.a),
        g: over.g * over.a + base.g * (1.0 - over.a),
        b: over.b * over.a + base.b * (1.0 - over.a),
        a: 1.0,
    }
}

/// Lays out one rectangle per title, left to right, with the close target
/// anchored at each button's right edge.
///
/// Each button takes its natural width (padding + title + gap + close,
/// clamped to [`BarMetrics::min_button_width`] and
/// [`BarMetrics::max_button_width`]). When the row would overflow the
/// available width every button is squeezed proportionally, never below the
/// minimum width — a row that still overflows at the floor (far more tabs
/// than fit) is clipped at the settings zone by the bar's drawing (see
/// [`TabBar::draw`]).
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
    #[expect(
        clippy::cast_precision_loss,
        reason = "tab count is far below 2^24, so usize -> f32 is exact"
    )]
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

/// What a press on the strip landed on: one of the corner targets,
/// or a part of a tab button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PressTarget {
    /// The settings button's target.
    Settings,
    /// The new-tab button's target.
    NewTab,
    /// A tab button's body or close mark.
    Tab(usize, HitTarget),
}

/// Classifies a bar-local `point` for pressing: the gear, then the
/// `+`, then the tab row — a fixed order for determinism, so either
/// corner target wins over any floor-overflow button fragment that
/// geometrically reaches under it (the three rects cannot overlap).
fn press_target(
    buttons: &[ButtonGeometry],
    settings: Rectangle,
    newtab: Rectangle,
    point: Point,
) -> Option<PressTarget> {
    if settings.contains(point) {
        return Some(PressTarget::Settings);
    }
    if newtab.contains(point) {
        return Some(PressTarget::NewTab);
    }
    hit_test(buttons, point).map(|(index, target)| PressTarget::Tab(index, target))
}

/// Per-widget state: the title paragraphs, cached across frames and
/// rebuilt whenever a tab's title changes (measured once, then reused by
/// layout, hit-testing and drawing), the last tab-body click for
/// double-click detection, and one slot slide per tab button for drag
/// displacement and the settle after a release.
#[derive(Default)]
struct State {
    paragraphs: HashMap<TabId, TitleParagraph>,
    /// The settings tooltip's label paragraph: one constant label,
    /// cached and measured like the titles.
    tooltip: TitleParagraph,
    /// The new-tab button's tooltip label paragraph, cached the same
    /// way (one constant label).
    newtab_tooltip: TitleParagraph,
    /// The dropdown's row label paragraphs (label + paragraph pairs),
    /// cached like the titles and rebuilt whenever the profile list
    /// changes — a config edit while the dropdown is open re-lays
    /// the rows on the next frame.
    dropdown_paragraphs: Vec<(String, TitleParagraph)>,
    /// Whether the profile dropdown is open. Bar-local by design: the
    /// flag lives in the bar's widget tree, and hiding the surface
    /// destroys that tree with it — an open dropdown can never
    /// survive a hide/show cycle (STOP-confirmed).
    dropdown_open: bool,
    /// The open per-tab context menu: the right-pressed tab's id.
    /// Bar-local by the dropdown's rule — the open state dies with
    /// the strip's tree on hide/show.
    tab_menu: Option<TabId>,
    /// The tab menu's row label paragraphs, cached and synced like the
    /// dropdown's — rebuilt whenever the open composition's labels
    /// change (a different tab right-clicked, a profile edited, a
    /// right neighbor appearing or leaving).
    tab_menu_paragraphs: Vec<(String, TitleParagraph)>,
    click: Option<click::Click>,
    drag: Option<Drag>,
    slots: HashMap<TabId, SlotAnimation>,
}

impl State {
    /// Snaps every finished slide onto its target.
    fn settle_finished_slots(&mut self) {
        for slot in self.slots.values_mut() {
            slot.settle();
        }
    }

    fn any_slot_animating(&self) -> bool {
        self.slots.values().any(SlotAnimation::is_animating)
    }

    /// Drops the slides of tabs that no longer exist.
    fn prune_slots(&mut self, tabs: &[Tab]) {
        self.slots
            .retain(|id, _| tabs.iter().any(|tab| tab.id == *id));
    }
}

/// The tab row's button geometry for `tabs` inside a strip `bounds`
/// wide — [`TabBar::button_geometry`] delegates here so the per-tab
/// menu's overlay can resolve its anchor per frame without the bar:
/// the anchor must follow its button through any tab-list change
/// while the menu is open.
fn row_button_geometry(tabs: &[Tab], state: &State, bounds: Rectangle) -> Vec<ButtonGeometry> {
    if !shows_tab_buttons(tabs.len()) {
        return Vec::new();
    }
    let widths: Vec<f32> = tabs
        .iter()
        .map(|tab| {
            state
                .paragraphs
                .get(&tab.id)
                .map_or(0.0, TitleParagraph::min_width)
        })
        .collect();
    layout_buttons(row_budget(bounds.width, &METRICS), &widths, &METRICS)
}

/// The tab bar strip: always mounted by the app, carrying the settings
/// button at its right corner; the tab buttons inside it render only
/// while more than one tab exists (see [`shows_tab_buttons`]).
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
    /// Reports a completed drag: the dragged tab's id and the index it
    /// was dropped at in the current list.
    on_move: Option<Box<dyn Fn(TabId, usize) -> Message + 'a>>,
    on_rename: Option<Box<dyn Fn(TabId) -> Message + 'a>>,
    /// Reports a press outside the rename input while a rename is
    /// active: iced inputs keep keyboard focus on outside presses, so
    /// the bar reports the click-away itself. The app maps it to a
    /// commit.
    on_rename_end: Option<Box<dyn Fn() -> Message + 'a>>,
    /// Reports a left press on the settings target: the app maps it
    /// onto the same `Action::Settings` path the context menu's
    /// `Settings…` row uses, so the button opens the identical window.
    on_settings: Option<Box<dyn Fn() -> Message + 'a>>,
    /// Reports a left press on the new-tab target: the app maps it
    /// onto the same `Action::NewTab` path as the `Ctrl+Shift+T`
    /// keybind, so the click is byte-identical to the keypress.
    on_new_tab: Option<Box<dyn Fn() -> Message + 'a>>,
    /// Reports a profile-row pick from the dropdown: the app maps it
    /// onto the same `Action::NewProfileTab(index)` path the
    /// context-menu submenu uses (index into the same profile list,
    /// resolved — and stale-warned — at apply time).
    on_profile_pick: Option<Box<dyn Fn(usize) -> Message + 'a>>,
    /// Reports a row pick from the per-tab context menu: the
    /// right-pressed tab's id and the row's [`TabMenuPick`], which the
    /// app maps onto the existing funnels with the clicked tab in
    /// place of "active" (id resolved against the live list at apply
    /// time, the `TabSelected` rule).
    on_tab_menu_pick: Option<Box<dyn Fn(TabId, TabMenuPick) -> Message + 'a>>,
    /// The configured profiles the dropdown lists. The app passes
    /// the same `config.profiles` slice the context menu reads, so
    /// both surfaces agree on order and membership.
    profiles: &'a [Profile],
    /// The active inline rename: which tab is being renamed and the
    /// text input overlaying its button. Built by the app, positioned
    /// by the bar.
    rename: Option<(TabId, Element<'a, Message>)>,
    /// Which vertical side of the strip the settings tooltip pops to:
    /// the terminal's side, set by the app from the configured edge.
    tooltip_side: TooltipSide,
    /// Whether the strip renders its border hairline (bar-borders
    /// spec) on its terminal-facing edge — the side `tooltip_side`
    /// already names. Off by default at construction; the app passes
    /// the `tab_bar_border` key.
    border: bool,
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
            on_settings: None,
            on_new_tab: None,
            on_profile_pick: None,
            on_tab_menu_pick: None,
            profiles: &[],
            rename: None,
            tooltip_side: TooltipSide::default(),
            border: false,
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

    /// Sets the message published when a tab drag completes: the
    /// dragged tab is dropped at the given index of the current list.
    pub fn on_move(mut self, on_move: impl Fn(TabId, usize) -> Message + 'a) -> Self {
        self.on_move = Some(Box::new(on_move));
        self
    }

    /// Sets the message published when a tab body is double-clicked: the
    /// start of an inline rename.
    pub fn on_rename(mut self, on_rename: impl Fn(TabId) -> Message + 'a) -> Self {
        self.on_rename = Some(Box::new(on_rename));
        self
    }

    /// Sets the message published when a press lands outside the rename
    /// input while a rename is active. The press still reaches whatever
    /// it landed on.
    pub fn on_rename_end(mut self, on_rename_end: impl Fn() -> Message + 'a) -> Self {
        self.on_rename_end = Some(Box::new(on_rename_end));
        self
    }

    /// Sets the message published when the settings target is
    /// left-pressed.
    pub fn on_settings(mut self, on_settings: impl Fn() -> Message + 'a) -> Self {
        self.on_settings = Some(Box::new(on_settings));
        self
    }

    /// Sets the message published when the new-tab target is
    /// left-pressed.
    pub fn on_new_tab(mut self, on_new_tab: impl Fn() -> Message + 'a) -> Self {
        self.on_new_tab = Some(Box::new(on_new_tab));
        self
    }

    /// Sets the message published when a profile row is picked from
    /// the dropdown: the row's index into [`TabBar::profiles`].
    pub fn on_profile_pick(mut self, on_profile_pick: impl Fn(usize) -> Message + 'a) -> Self {
        self.on_profile_pick = Some(Box::new(on_profile_pick));
        self
    }

    /// Sets the message published when an enabled row of the per-tab
    /// context menu is picked: the right-pressed tab's id and the
    /// row's pick.
    pub fn on_tab_menu_pick(
        mut self,
        on_tab_menu_pick: impl Fn(TabId, TabMenuPick) -> Message + 'a,
    ) -> Self {
        self.on_tab_menu_pick = Some(Box::new(on_tab_menu_pick));
        self
    }

    /// Sets the profiles the `+`'s right-click dropdown lists.
    pub fn profiles(mut self, profiles: &'a [Profile]) -> Self {
        self.profiles = profiles;
        self
    }

    /// Sets which vertical side of the strip the settings tooltip pops
    /// to: the terminal's side.
    pub fn tooltip_side(mut self, side: TooltipSide) -> Self {
        self.tooltip_side = side;
        self
    }

    /// Sets whether the strip renders its border hairline: a 1 px
    /// pane-derived line on its terminal-facing edge — the side
    /// `tooltip_side` names — painted inside the strip's own bounds
    /// (bar-borders spec; pure paint, no layout).
    pub fn show_border(mut self, show: bool) -> Self {
        self.border = show;
        self
    }

    /// Hosts `input` as an overlay over the renamed tab's button,
    /// swapping the title for a text input in place.
    pub fn renaming(mut self, tab: TabId, input: impl Into<Element<'a, Message>>) -> Self {
        self.rename = Some((tab, input.into()));
        self
    }

    /// Rebuilds the cached title paragraphs: entries for closed tabs are
    /// pruned, new or retitled tabs are (re)laid out. A tab wearing an
    /// attention marker renders it as a prefix to its title, so a
    /// marker change re-lays the paragraph exactly like a title change.
    /// Returns nothing; measurement then goes through
    /// [`TitleParagraph::min_width`].
    fn refresh_paragraphs(&self, state: &mut State, renderer: &Renderer) {
        state
            .paragraphs
            .retain(|id, _| self.tabs.iter().any(|tab| tab.id == *id));

        let font = renderer.default_font();
        for tab in self.tabs {
            // The pin indicator prefixes everything — pin, then any
            // attention marker, then the title (ruling 3) — so a pin
            // re-lays the paragraph exactly like a marker change.
            let prefix = title_prefix(tab.pinned, tab.attention);
            let combined;
            let content = if prefix.is_empty() {
                tab.title.as_str()
            } else {
                combined = format!("{prefix}{}", tab.title);
                combined.as_str()
            };
            let text = Text {
                content,
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
        state.tooltip.update(label_text(SETTINGS_LABEL, font));
        state.newtab_tooltip.update(label_text(NEW_TAB_LABEL, font));

        // The dropdown's row labels, cached like the titles: the vec
        // is rebuilt only when the labels change, the paragraphs
        // re-laid every pass (the titles' cost model).
        let rows = dropdown_rows(self.profiles);
        let labels_match = state
            .dropdown_paragraphs
            .iter()
            .map(|(label, _)| label.as_str())
            .eq(rows.iter().map(|row| row.label.as_str()));
        if !labels_match {
            state.dropdown_paragraphs = rows
                .iter()
                .map(|row| (row.label.clone(), TitleParagraph::default()))
                .collect();
        }
        for (label, paragraph) in &mut state.dropdown_paragraphs {
            paragraph.update(label_text(label, font));
        }

        // The tab menu's rows, synced the same way — but only the
        // composition currently open: the clicked tab's position and
        // the live tab count decide the shape (the Close-Right row,
        // muting), so the labels are recomputed for that tab alone
        // while its menu is open.
        if let Some(menu_tab) = state.tab_menu
            && let Some(index) = self.tabs.iter().position(|tab| tab.id == menu_tab)
        {
            let rows = tab_menu_rows(
                self.profiles,
                self.tabs.len(),
                index,
                self.tabs[index].pinned,
            );
            let labels_match = state
                .tab_menu_paragraphs
                .iter()
                .map(|(label, _)| label.as_str())
                .eq(rows.iter().map(|row| row.label.as_str()));
            if !labels_match {
                state.tab_menu_paragraphs = rows
                    .iter()
                    .map(|row| (row.label.clone(), TitleParagraph::default()))
                    .collect();
            }
            for (label, paragraph) in &mut state.tab_menu_paragraphs {
                paragraph.update(label_text(label, font));
            }
        }
    }

    /// The current button geometry, bar-local (origin at the bar's
    /// top-left corner). Empty while the tab-count rule hides the row;
    /// otherwise laid out inside the row budget the settings zone
    /// leaves.
    fn button_geometry(&self, state: &State, bounds: Rectangle) -> Vec<ButtonGeometry> {
        row_button_geometry(self.tabs, state, bounds)
    }

    /// Retargets every non-dragged button's slot animation to the slot
    /// the in-progress drag leaves it: its position in the reordered
    /// row. Called on every cursor move once the drag has crossed the
    /// threshold, so neighbors ease aside live.
    fn retarget_displaced(&self, tree: &mut Tree, bounds: Rectangle) {
        let Some((drag_id, cursor_x, grab_x)) = tree
            .state
            .downcast_ref::<State>()
            .drag
            .as_ref()
            .filter(|drag| drag.started)
            .map(|drag| (drag.tab, drag.x, drag.grab_x))
        else {
            return;
        };
        let state = tree.state.downcast_mut::<State>();
        let Some(dragged) = self.tabs.iter().position(|tab| tab.id == drag_id) else {
            return;
        };
        let buttons = self.button_geometry(state, bounds);
        if buttons.get(dragged).is_none() {
            return;
        }
        let widths: Vec<f32> = buttons.iter().map(|b| b.bounds.width).collect();
        let drop = drop_from_followed(&buttons, dragged, cursor_x, grab_x);
        let (targets, _) = displaced_row(&widths, dragged, drop, METRICS.button_gap);
        for (index, tab) in self.tabs.iter().enumerate() {
            let Some(target) = targets[index] else {
                continue;
            };
            let home = buttons[index].bounds.x;
            state
                .slots
                .entry(tab.id)
                .or_insert_with(|| SlotAnimation::new(home))
                .retarget(target);
        }
    }
}

/// Everything [`TabBar::paint_button`] needs for one frame besides the
/// button itself.
struct PaintContext<'a> {
    state: &'a State,
    cosmic_theme: &'a cosmic::cosmic_theme::Theme,
    /// Foreground for titles and close marks, derived from the pane
    /// background (see `pane_colors`).
    pane_fg: Color,
    /// Bar-global cursor position, if it is over the bar.
    hover_point: Option<Point>,
}

impl<Message> TabBar<'_, Message>
where
    Message: Clone,
{
    /// Paints one tab button at bar-global `bounds`: pill background,
    /// title, and close mark. `elevated` renders the dragged button —
    /// a stronger pill with a drop shadow, drawn by the caller after
    /// the rest of the row so it rides on top of its neighbors.
    fn paint_button(
        renderer: &mut Renderer,
        ctx: &PaintContext<'_>,
        tab: &Tab,
        bounds: Rectangle,
        is_active: bool,
        elevated: bool,
    ) {
        let PaintContext {
            state,
            cosmic_theme,
            pane_fg,
            hover_point,
        } = ctx;

        let is_hovered = hover_point.is_some_and(|point| bounds.contains(point));

        // Backgrounds and text colors mirror the native `TabBar`
        // preset of libcosmic's segmented-button theme (pinned rev
        // `08a09e3`): the active tab is neutral_5 at 0.2 alpha with
        // the accent text color, hovered tabs get the subtler 0.1,
        // inactive tabs stay flat. The dragged button carries the
        // strongest pill and a shadow, reading as lifted.
        let background: Option<Color> = if elevated {
            Some(cosmic_theme.palette.neutral_5.with_alpha(0.3).into())
        } else if is_active {
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
                    shadow: if elevated {
                        Shadow {
                            color: Color {
                                a: 0.35,
                                ..Color::BLACK
                            },
                            blur_radius: 10.0,
                            offset: Vector::new(0.0, 4.0),
                        }
                    } else {
                        Shadow::default()
                    },
                    snap: true,
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
            *pane_fg
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

        // Close button: a font-free `×` — squares marched along the
        // two diagonals of the target — with a hover disc so the
        // target reads as clickable at a glance. Rendering the mark
        // directly (rather than a text glyph) keeps it centered and
        // legible regardless of font metrics or fallback fonts.
        let close = Rectangle::new(
            Point::new(
                bounds.x + bounds.width - METRICS.button_padding_x - METRICS.close_size,
                bounds.y + (bounds.height - METRICS.close_size) / 2.0,
            ),
            Size::new(METRICS.close_size, METRICS.close_size),
        );
        let close_hovered = hover_point.is_some_and(|point| close.contains(point));
        if close_hovered {
            let hover_bg: Color = cosmic_theme.palette.neutral_5.with_alpha(0.35).into();
            renderer.fill_quad(
                Quad {
                    bounds: close,
                    border: Border {
                        radius: (close.height / 2.0).into(),
                        width: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    snap: true,
                    ..Default::default()
                },
                hover_bg,
            );
        }
        draw_close_mark(renderer, close, *pane_fg);
    }
}

impl<Message> Widget<Message, Theme, Renderer> for TabBar<'_, Message>
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
        // The rename input's tree, when a rename is active.
        self.rename
            .as_ref()
            .map(|(_, input)| Tree::new(input.as_widget()))
            .into_iter()
            .collect()
    }

    fn diff(&mut self, tree: &mut Tree) {
        // Reconcile the input's tree with the current session state. The
        // default implementation clears all children, which would discard
        // the input's cursor and focus state on every re-render.
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
        // An active drag grabs the cursor wherever it roams.
        if state.drag.as_ref().is_some_and(|drag| drag.started) {
            return mouse::Interaction::Grabbing;
        }
        let bounds = layout.bounds();
        let hit = cursor_position.position_in(bounds).is_some_and(|point| {
            press_target(
                &self.button_geometry(state, bounds),
                settings_target(bounds.width, &METRICS),
                newtab_target(bounds.width, &METRICS),
                point,
            )
            .is_some()
        });
        if hit {
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
        // closes whichever part of the button it lands on. A
        // double-click on a tab body starts an inline rename, any press
        // outside the rename input ends one, and a left press on a tab
        // body that moves beyond a small threshold drags it.
        //
        // Frame tick first: settle finished slot slides and keep frames
        // coming while a drag is live or a slide still moves — the
        // redraw loop libcosmic's reorderable row runs. With no drag in
        // flight the slides retarget to the row's static layout, which
        // is what animates everything home after the app applies a
        // completed reorder.
        if matches!(event, Event::Window(window::Event::RedrawRequested(_))) {
            let dragging = tree
                .state
                .downcast_ref::<State>()
                .drag
                .as_ref()
                .is_some_and(|drag| drag.started);
            {
                let state = tree.state.downcast_mut::<State>();
                state.settle_finished_slots();
                if !dragging {
                    state.prune_slots(self.tabs);
                    let buttons = self.button_geometry(state, layout.bounds());
                    for (index, tab) in self.tabs.iter().enumerate() {
                        let Some(button) = buttons.get(index) else {
                            continue;
                        };
                        state
                            .slots
                            .entry(tab.id)
                            .or_insert_with(|| SlotAnimation::new(button.bounds.x))
                            .retarget(button.bounds.x);
                    }
                }
            }
            let animating = tree.state.downcast_ref::<State>().any_slot_animating();
            if dragging || animating {
                shell.request_redraw();
            }
            return;
        }

        let button = match event {
            Event::Mouse(MouseEvent::ButtonPressed(button)) => *button,
            Event::Mouse(MouseEvent::CursorMoved { position }) => {
                // Drag progress: once the press crossed the
                // threshold, the pressed button follows the cursor's
                // bar-local x (clamped to the row) and its neighbors
                // retarget to the slots the drop would leave them in.
                // The release may land outside the bar — iced's column
                // routes events to every child, whatever the cursor is
                // over — so this is the position the drop is decided
                // from.
                let origin = layout.bounds().position();
                let local = Point::new(position.x - origin.x, position.y - origin.y);
                {
                    let Some(drag) = tree.state.downcast_mut::<State>().drag.as_mut() else {
                        return;
                    };
                    if !drag.started && (local.x - drag.origin.x).abs() < DRAG_THRESHOLD {
                        return;
                    }
                    drag.started = true;
                    drag.x = local.x;
                }
                shell.capture_event();
                shell.request_redraw();
                self.retarget_displaced(tree, layout.bounds());
                return;
            }
            Event::Mouse(MouseEvent::ButtonReleased(Button::Left)) => {
                // Drag finish: a plain click already selected on
                // press, so only a drag that crossed the threshold
                // reports a move. The drop is decided by the followed
                // button's center — the same rule the row painted — so
                // the tab lands exactly where it looked like it would.
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
                let to = drop_from_followed(&buttons, index, drag.x, drag.grab_x);
                if let Some(message) = self.on_move.as_ref().map(|on_move| on_move(drag.tab, to)) {
                    shell.publish(message);
                    shell.capture_event();
                }
                shell.request_redraw();
                return;
            }
            _ => return,
        };

        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let settings_rect = settings_target(bounds.width, &METRICS);
        let newtab_rect = newtab_target(bounds.width, &METRICS);
        let point = cursor_position.position_in(bounds);
        let buttons = self.button_geometry(state, bounds);

        // A press outside the rename input ends the rename: iced
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

        let Some(point) = point else {
            return;
        };

        // The `+`'s right-press opens the profile dropdown exactly
        // when ≥1 profile exists — zero profiles keeps the strip's
        // right-click today's no-op (the context menu's zero-profiles
        // flat-menu ruling). Toggle-close never runs here: while the
        // dropdown is open, its overlay sees and captures every press
        // first (see `DropdownOverlay::update`), so this branch only
        // ever handles the closed-side press. A right-press on a tab
        // button — body or close mark, both scope to the tab — opens
        // the per-tab context menu scoped to that tab, except over
        // the rename input's host: that button belongs to the input
        // while it overlays it. The menu's open also invalidates
        // layout: its bubble sizes from the row-paragraph cache,
        // which `refresh_paragraphs` fills only during a root layout
        // and only while the menu is open — without the invalidation
        // that layout never re-runs after this state-only flip (no
        // message, no view rebuild), and the overlay lays out against
        // an empty cache as a padding-only sliver (the combo-box
        // overlay's own open invalidates for the same reason; the
        // dropdown needs no such nudge, its cache syncing on every
        // root layout).
        if button == Button::Right {
            if newtab_rect.contains(point) && !self.profiles.is_empty() {
                state.dropdown_open = true;
                shell.capture_event();
                shell.request_redraw();
            } else if let Some((index, _)) = hit_test(&buttons, point)
                && let Some(tab) = self.tabs.get(index)
                && self.rename.as_ref().is_none_or(|edit| edit.0 != tab.id)
            {
                state.tab_menu = Some(tab.id);
                shell.invalidate_layout();
                shell.capture_event();
                shell.request_redraw();
            }
            return;
        }
        if !matches!(button, Button::Left | Button::Middle) {
            return;
        }
        // The settings target routes ahead of the tab row: a left
        // press inside it opens settings through the app's handler, a
        // middle press does nothing, and the press never arms a tab
        // drag. A press here during a rename already ended it above.
        if settings_rect.contains(point) {
            if button == Button::Left
                && let Some(message) = self.on_settings.as_ref().map(|on_settings| on_settings())
            {
                shell.publish(message);
                shell.capture_event();
            }
            return;
        }
        // The `+` target, next in the fixed routing order: a left
        // press spawns through the app's handler — the existing
        // `Ctrl+Shift+T` funnel, byte-identical semantics — while a
        // middle press does nothing and no drag is armed (the drag
        // arming lives in the tab-row branch below, never reached from
        // a corner target). A press here during a rename already ended
        // it above.
        if newtab_rect.contains(point) {
            if button == Button::Left
                && let Some(message) = self.on_new_tab.as_ref().map(|on_new_tab| on_new_tab())
            {
                shell.publish(message);
                shell.capture_event();
            }
            return;
        }
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

        // A single left press on a tab body arms a potential drag:
        // the press selects now, and crossing [`DRAG_THRESHOLD`] while
        // held turns it into a reorder — the native tab bar split between
        // click and drag. The grab offset is kept so the button can
        // follow the cursor at the point it was caught. A pinned tab
        // never arms — it stays put (ruling 4) — while the press still
        // selects below, so the tab behaves like any other until moved.
        if !on_close && !tab.pinned {
            let Some(pressed) = buttons.get(index) else {
                return;
            };
            state.drag = Some(Drag {
                tab: tab.id,
                origin: point,
                started: false,
                x: point.x,
                grab_x: point.x - pressed.bounds.x,
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
        // While a rename is active, host its input as an overlay
        // exactly over the renamed tab's button. The rename overlay
        // outranks everything below: the input is interactive, and a
        // press on either corner target commits the rename anyway.
        if self.rename.is_some() {
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
            return Some(overlay::Element::new(Box::new(RenameOverlay {
                tree: tree.children.first_mut()?,
                input,
                position,
                size: button.size(),
            })));
        }

        let bounds = layout.bounds();
        let absolute = Rectangle::new(bounds.position() + translation, bounds.size());

        // The per-tab context menu, when open: like the dropdown it
        // must be an overlay (its rect hangs over the terminal), and
        // it outranks the dropdown and the tooltips — only one strip
        // surface can be open at once anyway, since whichever is open
        // captures the press that would open the other. The anchor
        // resolves live per frame from the clicked tab's button, so
        // the bubble follows its button through any tab-list change —
        // and a target that left the row (its tab exited) closes the
        // menu with it.
        if let Some(menu_tab) = tree.state.downcast_ref::<State>().tab_menu {
            let state = tree.state.downcast_ref::<State>();
            let anchored = self
                .tabs
                .iter()
                .position(|tab| tab.id == menu_tab)
                .and_then(|index| {
                    row_button_geometry(self.tabs, state, bounds)
                        .get(index)
                        .map(|button| button.bounds)
                });
            match anchored {
                Some(anchor) => {
                    return Some(overlay::Element::new(Box::new(TabMenuOverlay {
                        state: tree.state.downcast_mut::<State>(),
                        tabs: self.tabs,
                        profiles: self.profiles,
                        on_tab_menu_pick: &self.on_tab_menu_pick,
                        side: self.tooltip_side,
                        bar: absolute,
                        anchor,
                    })));
                }
                None => tree.state.downcast_mut::<State>().tab_menu = None,
            }
        }

        // While the dropdown is open it outranks the tooltip — the
        // dropdown is the stronger affordance at that corner, so the
        // `+`'s tip is suppressed — and it must be an overlay at all
        // because its rect hangs over the terminal: only the overlay
        // pass sees presses there, ahead of the tree, and a captured
        // overlay press reaches nothing else (iced's user_interface).
        if !self.profiles.is_empty() && tree.state.downcast_ref::<State>().dropdown_open {
            return Some(overlay::Element::new(Box::new(DropdownOverlay {
                state: tree.state.downcast_mut::<State>(),
                profiles: self.profiles,
                on_new_tab: &self.on_new_tab,
                on_profile_pick: &self.on_profile_pick,
                side: self.tooltip_side,
                bounds: absolute,
            })));
        }

        // Otherwise the corner buttons' tooltips ride the same
        // overlay pass — see [`TooltipOverlay`].
        Some(overlay::Element::new(Box::new(TooltipOverlay {
            state: tree.state.downcast_ref::<State>(),
            side: self.tooltip_side,
            bounds: absolute,
        })))
    }

    // The pane_bg/pane_fg names match the PaintContext field
    // vocabulary; `similar_names` resolves its level at the fn, so the
    // expect rides here rather than on the binding.
    #[expect(
        clippy::similar_names,
        reason = "the pane_bg/pane_fg pair matches the PaintContext field vocabulary"
    )]
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
        let bounds = layout.bounds();
        let origin = bounds.position();
        let buttons = self.button_geometry(state, bounds);
        let hover_point = cursor_position.position_in(bounds);

        // The strip's own opaque background: matching the terminal pane
        // makes the bar part of the terminal surface instead of a strip
        // of window clear color (which follows the system theme and can
        // disagree with the terminal scheme).
        let (pane_bg, pane_fg) = pane_colors();
        renderer.fill_quad(
            Quad {
                bounds,
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

        // The drag's view of the row: where the dragged button
        // follows the cursor, where every other button's slot is
        // headed, and where the drop indicator marks the landing.
        let drag_view = state
            .drag
            .as_ref()
            .filter(|drag| drag.started)
            .and_then(|drag| {
                let index = self.tabs.iter().position(|tab| tab.id == drag.tab)?;
                let button = buttons.get(index)?;
                let row_width = buttons
                    .last()
                    .map_or(0.0, |last| last.bounds.x + last.bounds.width);
                let followed = followed_x(row_width, button.bounds.width, drag.x, drag.grab_x);
                let drop = drop_from_followed(&buttons, index, drag.x, drag.grab_x);
                let widths: Vec<f32> = buttons.iter().map(|b| b.bounds.width).collect();
                let (_, indicator) = displaced_row(&widths, index, drop, METRICS.button_gap);
                Some((index, followed, indicator))
            });

        // Slot animations only paint while they mean something: during
        // a drag, or while a slide settles. Otherwise a stale slide
        // (e.g. after a title change re-widens the row) could paint a
        // button where the layout no longer puts it.
        let animate = drag_view.is_some() || state.any_slot_animating();

        // The tab row paints clipped to the strip minus the settings
        // zone: a row at the minimum-width floor still geometrically overflows its
        // budget, and without the clip its tail would run under the
        // settings button at the right corner.
        renderer.with_layer(
            Rectangle::new(
                origin,
                Size::new(row_budget(bounds.width, &METRICS), METRICS.bar_height),
            ),
            |renderer| {
                let ctx = PaintContext {
                    state,
                    cosmic_theme,
                    pane_fg,
                    hover_point,
                };
                for (index, (tab, button)) in self.tabs.iter().zip(&buttons).enumerate() {
                    if drag_view.is_some_and(|(dragged, ..)| dragged == index) {
                        continue; // the dragged button paints last, on top
                    }
                    let slot_x = state
                        .slots
                        .get(&tab.id)
                        .map_or(button.bounds.x, SlotAnimation::current);
                    let x = if animate { slot_x } else { button.bounds.x };
                    let bounds = Rectangle::new(
                        Point::new(origin.x + x, origin.y),
                        Size::new(button.bounds.width, METRICS.bar_height),
                    );
                    Self::paint_button(renderer, &ctx, tab, bounds, index == self.active, false);
                }

                // The dragged button: follows the cursor, lifted
                // clear of the row and painted after its neighbors so
                // it rides on top.
                if let Some((index, followed, _)) = drag_view
                    && let (Some(tab), Some(button)) = (self.tabs.get(index), buttons.get(index))
                {
                    let bounds = Rectangle::new(
                        Point::new(origin.x + followed, origin.y - DRAG_LIFT),
                        Size::new(button.bounds.width, METRICS.bar_height),
                    );
                    Self::paint_button(renderer, &ctx, tab, bounds, index == self.active, true);
                }

                // Drop indicator: a vertical accent line in the
                // slot the drag currently lands in, between the buttons
                // that would surround the dragged tab in the displaced
                // row.
                if let Some((_, _, indicator)) = drag_view {
                    let x = origin.x + indicator;
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
            },
        );

        // The border hairline (bar-borders spec): one 1 px line on
        // the strip's terminal-facing edge — the side `tooltip_side`
        // names — in the pane foreground at low alpha, painted over
        // the row layer so buttons that reach the strip's edge do
        // not swallow it. Inside the strip's own bounds: no layout
        // ever moves.
        if self.border {
            renderer.fill_quad(
                Quad {
                    bounds: border_rect(bounds, self.tooltip_side),
                    border: Border {
                        radius: 0.0.into(),
                        width: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    snap: true,
                    ..Default::default()
                },
                Color {
                    a: BORDER_ALPHA,
                    ..pane_fg
                },
            );
        }

        // The corner buttons — the new-tab `+` and the settings gear —
        // are quads only, font-free like the close `×`, over the
        // shared hover treatment, shown the moment the cursor enters
        // either target (no delay). Their tooltips ride the overlay
        // pass (see `TooltipOverlay`).
        let hover_bg: Color = cosmic_theme.palette.neutral_5.with_alpha(0.35).into();

        // The new-tab `+`: two crossing bars in the pane foreground,
        // one gap left of the gear.
        let newtab = newtab_target(bounds.width, &METRICS);
        let newtab = Rectangle::new(
            Point::new(origin.x + newtab.x, origin.y + newtab.y),
            newtab.size(),
        );
        if hover_point.is_some_and(|point| newtab.contains(point)) {
            fill_disc(renderer, newtab, hover_bg);
        }
        let plus = plus_glyph(newtab);
        for bar in [plus.horizontal, plus.vertical] {
            renderer.fill_quad(
                Quad {
                    bounds: bar,
                    border: Border {
                        radius: 0.0.into(),
                        width: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    snap: true,
                    ..Default::default()
                },
                pane_fg,
            );
        }

        // The settings gear: a ring punched by a strip-colored disc,
        // plus teeth, over the same hover treatment.
        let target = settings_target(bounds.width, &METRICS);
        let target = Rectangle::new(
            Point::new(origin.x + target.x, origin.y + target.y),
            target.size(),
        );
        let settings_hovered = hover_point.is_some_and(|point| target.contains(point));
        // The punched hole must show whatever the strip shows under
        // the glyph — the hover tint when the disc is up, the plain
        // strip otherwise — composited exactly, so the ring reads
        // identically either way.
        let under_glyph = if settings_hovered {
            fill_disc(renderer, target, hover_bg);
            composite(hover_bg, pane_bg)
        } else {
            pane_bg
        };
        let gear = gear_glyph(target);
        fill_disc(renderer, gear.body, pane_fg);
        fill_disc(renderer, gear.punch, under_glyph);
        for tooth in gear.teeth {
            fill_disc(renderer, tooth, pane_fg);
        }
    }
}

/// The corner buttons' tooltips, hosted on the overlay pass (the
/// rename-input precedent). A tip painted in the bar's own `draw`
/// would be overpainted in the Bottom-edge stacking, where the
/// column draws the terminal after the bar; overlays draw after the
/// whole tree, so the tip stays visible clearing the strip on both
/// edge placements. Visibility is decided per frame from the live
/// cursor — no hover delay — with the gear's tip anchored at the
/// strip's right corner and the `+`'s at its own target's right
/// edge.
struct TooltipOverlay<'a> {
    /// The bar's state, carrying the tips' cached label paragraphs.
    state: &'a State,
    /// Which vertical side of the strip the tips pop to.
    side: TooltipSide,
    /// The bar's absolute bounds (translation applied).
    bounds: Rectangle,
}

impl<Message> overlay::Overlay<Message, Theme, Renderer> for TooltipOverlay<'_> {
    fn layout(&mut self, _renderer: &Renderer, _bounds: Size) -> layout::Node {
        // Laid out for the gear tip (see `draw` for how the `+` tip
        // gets its rect): pinned at the strip's right corner.
        let tip = tooltip_bounds(
            self.bounds.width,
            self.state.tooltip.min_width(),
            &METRICS,
            self.side,
        );
        let position = self.bounds.position() + Vector::new(tip.x, tip.y);
        layout::Node::new(tip.size()).move_to(position)
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor_position: mouse::Cursor,
    ) {
        let origin = self.bounds.position();
        let gear = settings_target(self.bounds.width, &METRICS);
        let gear = Rectangle::new(origin + Vector::new(gear.x, gear.y), gear.size());
        if cursor_position.position_in(gear).is_some() {
            paint_tip(renderer, theme, layout.bounds(), &self.state.tooltip);
            return;
        }
        // The `+` tip. Overlay layout carries no cursor, so the node
        // was laid out for the gear tip; this tip's rect is recomputed
        // here from the live cursor — the tips are dressing with no
        // hit-testing, so the node only ever matters as the paint
        // rect.
        let newtab = newtab_target(self.bounds.width, &METRICS);
        let newtab = Rectangle::new(origin + Vector::new(newtab.x, newtab.y), newtab.size());
        if cursor_position.position_in(newtab).is_some() {
            let right_edge = newtab.x + newtab.width;
            let bar_local = tooltip_bounds(
                right_edge,
                self.state.newtab_tooltip.min_width(),
                &METRICS,
                self.side,
            );
            let tip = Rectangle::new(
                origin + Vector::new(bar_local.x, bar_local.y),
                bar_local.size(),
            );
            paint_tip(renderer, theme, tip, &self.state.newtab_tooltip);
        }
    }
}

/// Paints one tooltip bubble at `tip`: the pane color pair swapped —
/// a foreground-colored bubble carrying the strip's color as its text
/// — so the label stays readable on light and dark panes alike.
// The pane_bg/pane_fg names match the PaintContext field vocabulary;
// `similar_names` resolves its level at the fn, so the expect rides
// here rather than on the binding.
#[expect(
    clippy::similar_names,
    reason = "the pane_bg/pane_fg pair matches the PaintContext field vocabulary"
)]
fn paint_tip(renderer: &mut Renderer, theme: &Theme, tip: Rectangle, paragraph: &TitleParagraph) {
    let (pane_bg, pane_fg) = pane_colors();
    renderer.fill_quad(
        Quad {
            bounds: tip,
            border: Border {
                radius: theme.cosmic().radius_s().into(),
                width: 0.0,
                color: Color::TRANSPARENT,
            },
            snap: true,
            ..Default::default()
        },
        pane_fg,
    );
    let text_y = tip.y + (tip.height - paragraph.min_height()) / 2.0;
    renderer.fill_paragraph(
        paragraph.raw(),
        Point::new(tip.x + TOOLTIP_PADDING_X, text_y),
        pane_bg,
        tip,
    );
}

/// The profile dropdown, hand-drawn on the overlay pass (the tooltip
/// precedent — no hosted menu machinery): a pane-inverted bubble of
/// rows beside the strip's right corner, one row per
/// [`dropdown_rows`] entry. It exists only while open, and while it
/// exists it owns every press: the overlay pass runs ahead of the
/// tree, so a captured press reaches nothing else — no spawn from the
/// `+` under a toggle press, no tab select, no drag armed, no
/// fall-through to the terminal below (iced's `user_interface`
/// forwards a captured overlay event to nothing).
struct DropdownOverlay<'a, 'b, Message> {
    /// The bar's state: the open flag (cleared on any press) and the
    /// cached row label paragraphs.
    state: &'a mut State,
    /// The profile list the rows are built from — the bar's slice for
    /// this frame.
    profiles: &'a [Profile],
    /// The plain row's launch — the `+`'s left-click handler.
    on_new_tab: &'a Option<Box<dyn Fn() -> Message + 'b>>,
    /// A profile row's launch — the context-menu submenu's handler.
    on_profile_pick: &'a Option<Box<dyn Fn(usize) -> Message + 'b>>,
    /// Which vertical side of the strip the dropdown pops to.
    side: TooltipSide,
    /// The bar's absolute bounds (translation applied).
    bounds: Rectangle,
}

impl<Message> overlay::Overlay<Message, Theme, Renderer> for DropdownOverlay<'_, '_, Message> {
    fn layout(&mut self, _renderer: &Renderer, _bounds: Size) -> layout::Node {
        let rows = dropdown_rows(self.profiles);
        let widest = self
            .state
            .dropdown_paragraphs
            .iter()
            .map(|(_, paragraph)| paragraph.min_width())
            .fold(0.0, f32::max);
        let bar_local = dropdown_bounds(self.bounds.width, widest, rows.len(), &METRICS, self.side);
        let position = self.bounds.position() + Vector::new(bar_local.x, bar_local.y);
        layout::Node::new(bar_local.size()).move_to(position)
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        // Dismissal is press-driven and captured: every press closes
        // the dropdown, a left press on a row first publishes its
        // launch, and every press is captured so nothing under the
        // dropdown — the `+` itself (toggle), the strip, the terminal
        // — acts on it. Motion and keyboard pass through: the bar
        // handles no keyboard. A motion event also asks for a frame:
        // the runtime draws only on request, and nothing under an
        // open dropdown churns on its own, so without this the row
        // hover would stick on whatever row a chance frame caught.
        if matches!(event, Event::Mouse(MouseEvent::CursorMoved { .. })) {
            shell.request_redraw();
            return;
        }
        let Event::Mouse(MouseEvent::ButtonPressed(button)) = event else {
            return;
        };
        let dropdown = layout.bounds();
        let row = cursor
            .position()
            .and_then(|point| dropdown_row_at(dropdown, point))
            .and_then(|index| {
                dropdown_rows(self.profiles)
                    .get(index)
                    .map(|row| row.profile)
            });
        if *button == Button::Left
            && let Some(profile) = row
        {
            let message = match profile {
                None => self.on_new_tab.as_ref().map(|on_new_tab| on_new_tab()),
                Some(index) => self
                    .on_profile_pick
                    .as_ref()
                    .map(|on_profile_pick| on_profile_pick(index)),
            };
            if let Some(message) = message {
                shell.publish(message);
            }
        }
        self.state.dropdown_open = false;
        shell.capture_event();
        shell.request_redraw();
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        // Pointer over the rows only: elsewhere the interaction falls
        // back to whatever the strip itself reports.
        let over_rows = cursor
            .position()
            .is_some_and(|point| dropdown_row_at(layout.bounds(), point).is_some());
        if over_rows {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }

    // The pane_bg/pane_fg names match the PaintContext field
    // vocabulary; `similar_names` resolves its level at the fn, so
    // the expect rides here rather than on the binding.
    #[expect(
        clippy::similar_names,
        reason = "the pane_bg/pane_fg pair matches the PaintContext field vocabulary"
    )]
    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor_position: mouse::Cursor,
    ) {
        let dropdown = layout.bounds();
        let (pane_bg, pane_fg) = pane_colors();
        // The pane pair swapped, the tooltip rule: a
        // foreground-colored bubble carrying the strip's color as its
        // text stays readable on light and dark panes alike.
        renderer.fill_quad(
            Quad {
                bounds: dropdown,
                border: Border {
                    radius: theme.cosmic().radius_s().into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                snap: true,
                ..Default::default()
            },
            pane_fg,
        );

        let hovered = cursor_position
            .position()
            .and_then(|point| dropdown_row_at(dropdown, point));
        for (index, (_, paragraph)) in self.state.dropdown_paragraphs.iter().enumerate() {
            #[expect(
                clippy::cast_precision_loss,
                reason = "row count is far below 2^24, so usize -> f32 is exact"
            )]
            let row_bounds = Rectangle::new(
                Point::new(dropdown.x, dropdown.y + index as f32 * DROPDOWN_ROW_HEIGHT),
                Size::new(dropdown.width, DROPDOWN_ROW_HEIGHT),
            );
            // Rows highlight on hover like tab bodies: a subtle tint
            // over the bubble (row metrics and this treatment are
            // OQ-2, ratified at the hardware STOP).
            if hovered == Some(index) {
                renderer.fill_quad(
                    Quad {
                        bounds: row_bounds,
                        border: Border {
                            radius: theme.cosmic().radius_s().into(),
                            width: 0.0,
                            color: Color::TRANSPARENT,
                        },
                        snap: true,
                        ..Default::default()
                    },
                    Color { a: 0.15, ..pane_bg },
                );
            }
            let text_y = row_bounds.y + (row_bounds.height - paragraph.min_height()) / 2.0;
            renderer.fill_paragraph(
                paragraph.raw(),
                Point::new(row_bounds.x + DROPDOWN_PADDING_X, text_y),
                pane_bg,
                dropdown,
            );
        }
    }
}

/// The per-tab context menu, hand-drawn on the overlay pass (the
/// dropdown's pattern — no hosted menu machinery): a pane-inverted
/// bubble of [`tab_menu_rows`] rows popping at the clicked tab's
/// button on the terminal's side of the strip. While open it owns
/// every press, the dropdown's contract: any press closes it, a left
/// press on an enabled row first publishing its pick — carried with
/// the clicked tab's id — and every press captured so nothing under
/// the menu acts on it.
struct TabMenuOverlay<'a, 'b, Message> {
    /// The bar's state: the clicked tab (cleared on any press) and the
    /// cached row label paragraphs.
    state: &'a mut State,
    /// The tabs the rows compose for — the bar's slice for this frame.
    tabs: &'a [Tab],
    /// The profile list the Switch Profile section lists — the bar's
    /// slice, the dropdown's rule.
    profiles: &'a [Profile],
    /// The pick publisher — the app maps (tab, pick) onto the
    /// existing funnels.
    on_tab_menu_pick: &'a Option<Box<dyn Fn(TabId, TabMenuPick) -> Message + 'b>>,
    /// Which vertical side of the strip the menu pops to.
    side: TooltipSide,
    /// The bar's absolute bounds (translation applied).
    bar: Rectangle,
    /// The clicked button's bar-local rect — the anchor the bubble
    /// pops at.
    anchor: Rectangle,
}

impl<Message> TabMenuOverlay<'_, '_, Message> {
    /// The rows for the menu's current target — the composition the
    /// clicked tab's position dictates (muted moves, the conditional
    /// Close-Right row). Empty once the target left the list; the
    /// overlay's caller then drops the menu, so this is the draw-side
    /// guard only.
    fn rows(&self, menu_tab: Option<TabId>) -> Vec<TabMenuRow> {
        menu_tab
            .and_then(|tab| self.tabs.iter().position(|candidate| candidate.id == tab))
            .map(|index| {
                tab_menu_rows(
                    self.profiles,
                    self.tabs.len(),
                    index,
                    self.tabs[index].pinned,
                )
            })
            .unwrap_or_default()
    }
}

impl<Message> overlay::Overlay<Message, Theme, Renderer> for TabMenuOverlay<'_, '_, Message> {
    fn layout(&mut self, _renderer: &Renderer, _bounds: Size) -> layout::Node {
        let Some(menu_tab) = self.state.tab_menu else {
            return layout::Node::new(Size::ZERO);
        };
        let rows = self.rows(Some(menu_tab));
        let widest = self
            .state
            .tab_menu_paragraphs
            .iter()
            .map(|(_, paragraph)| paragraph.min_width())
            .fold(0.0, f32::max);
        let bar_local = tab_menu_bounds(
            self.bar.width,
            self.anchor.x,
            widest,
            rows.len(),
            &METRICS,
            self.side,
        );
        let position = self.bar.position() + Vector::new(bar_local.x, bar_local.y);
        layout::Node::new(bar_local.size()).move_to(position)
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        // The dropdown's dismissal contract, unchanged: every press
        // closes, a left press on an enabled row first publishes its
        // pick (the clicked tab's id captured before the clear), and
        // every press is captured so nothing under the menu — the
        // strip, a tab button, the terminal below — acts on it. Motion
        // and keyboard pass through. A motion event also asks for a
        // frame: the runtime draws only on request, and nothing under
        // an open menu churns on its own, so without this the row
        // hover would stick on whatever row a chance frame caught
        // (the first hover, then nothing until an unrelated repaint).
        if matches!(event, Event::Mouse(MouseEvent::CursorMoved { .. })) {
            shell.request_redraw();
            return;
        }
        let Event::Mouse(MouseEvent::ButtonPressed(button)) = event else {
            return;
        };
        let menu_tab = self.state.tab_menu;
        let rows = self.rows(menu_tab);
        let pick = cursor
            .position()
            .and_then(|point| dropdown_row_at(layout.bounds(), point))
            .and_then(|index| rows.get(index))
            .filter(|row| row.enabled)
            .and_then(|row| row.pick);
        if *button == Button::Left
            && let Some((tab, pick)) = menu_tab.zip(pick)
            && let Some(message) = self
                .on_tab_menu_pick
                .as_ref()
                .map(|on_tab_menu_pick| on_tab_menu_pick(tab, pick))
        {
            shell.publish(message);
        }
        self.state.tab_menu = None;
        shell.capture_event();
        shell.request_redraw();
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        // Pointer over the bubble only: elsewhere the interaction
        // falls back to whatever the strip itself reports (the
        // dropdown's rule — the header and muted rows keep the
        // pointer shape, honest at a glance).
        let over_menu = cursor
            .position()
            .is_some_and(|point| layout.bounds().contains(point));
        if over_menu {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }

    // The pane_bg/pane_fg names match the PaintContext field
    // vocabulary; `similar_names` resolves its level at the fn, so the
    // expect rides here rather than on the binding.
    #[expect(
        clippy::similar_names,
        reason = "the pane_bg/pane_fg pair matches the PaintContext field vocabulary"
    )]
    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor_position: mouse::Cursor,
    ) {
        let menu = layout.bounds();
        let (pane_bg, pane_fg) = pane_colors();
        // The pane pair swapped, the dropdown's rule: a
        // foreground-colored bubble carrying the strip's color as its
        // text stays readable on light and dark panes alike.
        renderer.fill_quad(
            Quad {
                bounds: menu,
                border: Border {
                    radius: theme.cosmic().radius_s().into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                snap: true,
                ..Default::default()
            },
            pane_fg,
        );

        let rows = self.rows(self.state.tab_menu);
        let hovered = cursor_position
            .position()
            .and_then(|point| dropdown_row_at(menu, point));
        for (index, (_, paragraph)) in self.state.tab_menu_paragraphs.iter().enumerate() {
            let Some(row) = rows.get(index) else {
                break;
            };
            #[expect(
                clippy::cast_precision_loss,
                reason = "row count is far below 2^24, so usize -> f32 is exact"
            )]
            let row_bounds = Rectangle::new(
                Point::new(menu.x, menu.y + index as f32 * DROPDOWN_ROW_HEIGHT),
                Size::new(menu.width, DROPDOWN_ROW_HEIGHT),
            );
            // A hairline separator above the section-leading rows,
            // drawn in the text color at low alpha.
            if row.separated {
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle::new(
                            row_bounds.position(),
                            Size::new(row_bounds.width, 1.0),
                        ),
                        border: Border {
                            radius: 0.0.into(),
                            width: 0.0,
                            color: Color::TRANSPARENT,
                        },
                        snap: true,
                        ..Default::default()
                    },
                    Color { a: 0.3, ..pane_bg },
                );
            }
            // Rows highlight on hover, clickable ones only — the
            // header and muted rows are not targets. The tint is
            // deliberately stronger than the dropdown's 0.15: the
            // menu is the keyboardless verb surface, so the hovered
            // row must read unmistakably as the press's target on
            // both light and dark panes (pane_bg over the inverted
            // bubble at low contrast reads as no hover at all).
            if hovered == Some(index) && row.enabled && row.pick.is_some() {
                renderer.fill_quad(
                    Quad {
                        bounds: row_bounds,
                        border: Border {
                            radius: theme.cosmic().radius_s().into(),
                            width: 0.0,
                            color: Color::TRANSPARENT,
                        },
                        snap: true,
                        ..Default::default()
                    },
                    Color { a: 0.3, ..pane_bg },
                );
            }
            // Muted rows — the section header, an edge-clamped move —
            // draw their label at half alpha; enabled rows full.
            let active = row.enabled && row.pick.is_some();
            let text_color = if active {
                pane_bg
            } else {
                Color { a: 0.5, ..pane_bg }
            };
            let text_y = row_bounds.y + (row_bounds.height - paragraph.min_height()) / 2.0;
            renderer.fill_paragraph(
                paragraph.raw(),
                Point::new(row_bounds.x + DROPDOWN_PADDING_X, text_y),
                text_color,
                menu,
            );
        }
    }
}

/// Hosts the inline-rename input over its tab button, delegating
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
        );
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.input
            .as_widget_mut()
            .operate(self.tree, layout, renderer, operation);
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
mod tests;
