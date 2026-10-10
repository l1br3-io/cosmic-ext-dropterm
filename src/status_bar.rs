// SPDX-License-Identifier: GPL-3.0-only

//! The status band: a 28 px strip at the surface's free edge,
//! adjacent to the grab bar, narrating the **active terminal** —
//! what runs, where it is, and how it is dressed.
//!
//! Derived, not decorated: every color flows through the tab strip's
//! own `pane_colors` derivation, content reads existing in-app state
//! (the cwd cache, the recorded shell, the id-resolved profile, the
//! zoom adjustment), and the lone syscall — the foreground process
//! name — lands in an app-side snapshot refreshed only at event
//! piggyback points, never per-frame (status-bar criterion 5). The
//! widget half (Task 2) renders this module's pure model.
//!
//! Segments are values without labels — a path reads as a path —
//! separated by gaps, never punctuation. The one live element is the
//! process-state disc: filled while a foreground child runs, a hollow
//! ring over the idle shell.

use std::path::Path;

use cosmic::iced::core::alignment;
use cosmic::iced::core::text::{LineHeight, Shaping, paragraph};
use cosmic::iced::core::text::{Text, Wrapping};
use cosmic::iced::core::{
    Border, Layout, Rectangle, Size, layout,
    renderer::{self, Quad, Renderer as _},
    text::Renderer as _,
    widget::{Tree, Widget, tree},
};
use cosmic::iced::{Color, Length, Pixels, Point, mouse};
use cosmic::theme::Theme;
use cosmic::{Element, Renderer};

use crate::config::{Edge, TabPosition};
use crate::tab_bar::{fill_disc, pane_colors};

/// The band's height in logical pixels — one notch below the tab
/// strip's 32 px, for callers that reserve room for it outside the
/// widget tree (see `surface::terminal_view_height`).
pub const BAND_HEIGHT: f32 = 28.0;

/// One strip of the surface's vertical column, top to bottom. The
/// stacking helper ([`column_stacking`]) orders all four; the app's
/// column match delegates to it instead of re-deriving the orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strip {
    /// The tab strip (`tab_bar::BAR_HEIGHT`).
    Tabs,
    /// The status band ([`BAND_HEIGHT`]).
    StatusBand,
    /// The terminal pane.
    Terminal,
    /// The grab bar (`grab_bar::GRAB_BAR_HEIGHT`).
    GrabBar,
}

/// The surface column's stacking order: the status band always takes
/// the slot **adjacent to the grab bar** — the free-edge-most band,
/// whatever the tab strip's own pinning (status-bar criterion 1).
/// `Auto` keeps the tab strip on the surface edge opposite the screen
/// edge; a pin places it on its named side regardless of anchor.
pub const fn column_stacking(edge: Edge, tab_position: TabPosition) -> [Strip; 4] {
    match (edge, tab_position) {
        (Edge::Top, TabPosition::Top) => [
            Strip::Tabs,
            Strip::Terminal,
            Strip::StatusBand,
            Strip::GrabBar,
        ],
        (Edge::Top, TabPosition::Auto | TabPosition::Bottom) => [
            Strip::Terminal,
            Strip::Tabs,
            Strip::StatusBand,
            Strip::GrabBar,
        ],
        (Edge::Bottom, TabPosition::Top | TabPosition::Auto) => [
            Strip::GrabBar,
            Strip::StatusBand,
            Strip::Tabs,
            Strip::Terminal,
        ],
        (Edge::Bottom, TabPosition::Bottom) => [
            Strip::GrabBar,
            Strip::StatusBand,
            Strip::Terminal,
            Strip::Tabs,
        ],
    }
}

/// The resolved segments of the active tab's status band, left
/// cluster then right, values without labels — a path reads as a
/// path. The disc's fill state rides `running`; the process name is
/// the foreground child's comm, or the recorded shell's basename when
/// the shell itself owns the foreground (or the comm read failed —
/// the honest fallback, never a blank band).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusSegments {
    /// Whether a foreground child runs — the disc fills.
    pub running: bool,
    /// The foreground process's name, or the shell's basename when
    /// idle/unreadable.
    pub process: String,
    /// The `~`-collapsed cwd, absent when the tab has none.
    pub path: Option<String>,
    /// The tab's id-resolved profile name, absent on plain-shell tabs
    /// (and on dangling ids — never a stale name).
    pub profile: Option<String>,
    /// The signed zoom percent (`zoom_adj × step`), absent at zero.
    pub zoom_percent: Option<i16>,
}

/// Assemble the active tab's segments from the app's existing state
/// plus the process-name snapshot. Every input is plain data — the
/// resolver is pure and headless-testable; the app feeds it
/// `tab_cwds`, the recorded shell, the id-resolved profile name, the
/// zoom adjustment and step, and the comm snapshot. `home` is the
/// `~`-collapse base; `None` (an unset `HOME` — never in a real
/// desktop session) keeps paths verbatim.
pub fn resolve_segments(
    home: Option<&Path>,
    cwd: Option<&Path>,
    shell: &str,
    profile_name: Option<&str>,
    zoom_adj: i8,
    zoom_step_mul_100: u16,
    foreground: Option<&str>,
) -> StatusSegments {
    StatusSegments {
        running: foreground.is_some(),
        process: foreground.map_or_else(|| shell_basename(shell), str::to_string),
        path: cwd.map(|cwd| collapse_home(cwd, home)),
        profile: profile_name.map(str::to_string),
        zoom_percent: zoom_percent(zoom_adj, zoom_step_mul_100),
    }
}

/// Collapse a `home`-prefixed path to `~` plus its remainder — on
/// component boundaries, so a sibling directory sharing the home's
/// string prefix (`/home/jpt-x` under `/home/jpt`) never collapses.
/// `home` itself collapses to a bare `~`; a missing `home` keeps the
/// path verbatim.
pub fn collapse_home(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(remainder) if !remainder.as_os_str().is_empty() => {
            format!("~/{}", remainder.display())
        }
        Some(_) => "~".to_string(),
        None => path.display().to_string(),
    }
}

/// Elide a `~`-collapsed path from its left, keeping the tail:
/// `…/l1br3/ced2` — what the user is inside of survives. Paths of
/// `max_chars` or fewer pass through untouched; `max_chars` of zero
/// elides to the empty string.
pub fn elide_path_left(path: &str, max_chars: usize) -> String {
    if path.chars().count() <= max_chars {
        return path.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    let mut kept: Vec<char> = path.chars().rev().take(max_chars - 1).collect();
    kept.reverse();
    kept.insert(0, '\u{2026}');
    kept.into_iter().collect()
}

/// The signed zoom percent a `zoom_adj` of `step_mul_100`-sized steps
/// adds: `adj × step_mul_100`, saturating into `i16`. Zero adjustment
/// is `None` — the segment hides until the font is not at its base
/// size.
pub fn zoom_percent(zoom_adj: i8, step_mul_100: u16) -> Option<i16> {
    (zoom_adj != 0).then(|| {
        let product = i32::from(zoom_adj) * i32::from(step_mul_100);
        i16::try_from(product.clamp(i32::from(i16::MIN), i32::from(i16::MAX)))
            .expect("clamped into i16 range before the conversion")
    })
}

/// The recorded shell's basename — the idle process segment's honest
/// name (`fish`, `zsh`), never the full path the spawn recorded.
pub fn shell_basename(shell: &str) -> String {
    Path::new(shell).file_name().map_or_else(
        || shell.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

// --- the widget: the band itself, per the tab_bar.rs pattern ---

/// Named through the iced core because libcosmic keeps its own `Plain`
/// alias private (pinned rev `08a09e3`) — the tab bar's own
/// `TitleParagraph` idiom.
type BandParagraph = paragraph::Plain<<Renderer as cosmic::iced::core::text::Renderer>::Paragraph>;

/// Segment text size in logical pixels — one notch below the tab
/// strip's 14 px titles: the band is the terminal speaking about
/// itself, in its own voice.
const SEGMENT_FONT_SIZE: f32 = 12.0;

/// The state disc's diameter, and the hollow ring's stroke.
const DISC_SIZE: f32 = 8.0;
const DISC_RING_WIDTH: f32 = 1.5;

/// Side padding of the band's clusters, and the gap between segments.
const PAD_X: f32 = 12.0;
const SEGMENT_GAP: f32 = 12.0;

/// The gap between the disc and the process name.
const DISC_GAP: f32 = 6.0;

/// The dimmed alpha the right cluster's segments carry — hierarchy by
/// alpha and mass, never a new hue.
const DIM_ALPHA: f32 = 0.6;

/// The band's cached paragraph state: one per segment, re-laid from
/// the resolved segments on every root layout (the tab bar's titles'
/// cost model).
#[derive(Default)]
struct State {
    process: BandParagraph,
    path: BandParagraph,
    profile: BandParagraph,
    zoom: BandParagraph,
}

/// The status band widget: a fixed-height fill-width strip rendering
/// [`StatusSegments`] — disc + process, then path on the left;
/// profile, then zoom on the right — in the terminal's own font at
/// 12 px, colors through the tab strip's shared `pane_colors`
/// derivation. Constructed from plain resolved data only: the
/// render path performs zero `/proc` reads by construction — the
/// app-side snapshot refresh is the lone caller of the engine's comm
/// read (status-bar criterion 5).
pub struct StatusBar<Message> {
    /// The active tab's resolved segments — owned, built fresh by the
    /// app on every view pass.
    segments: StatusSegments,
    /// The terminal's own font (OQ-2: `Family::Name(config.font_name)`
    /// built by the app; the sanctioned fallback if the family does
    /// not resolve at the hardware pass is the default UI font at
    /// 12 px with every other token standing).
    font: cosmic::iced::Font,
    _message: std::marker::PhantomData<fn() -> Message>,
}

impl<Message> StatusBar<Message> {
    /// Creates a band rendering `segments`.
    pub fn new(segments: StatusSegments, font: cosmic::iced::Font) -> Self {
        Self {
            segments,
            font,
            _message: std::marker::PhantomData,
        }
    }

    /// Re-lays the cached paragraphs from the resolved segments. The
    /// path paragraph elides from its left against the width the
    /// measured right cluster leaves — what the user is inside of
    /// survives (see [`elide_path_left`]).
    fn refresh_paragraphs(&self, state: &mut State, width: f32) {
        let font = self.font;
        state
            .process
            .update(segment_text(&self.segments.process, font));
        state.profile.update(segment_text(
            self.segments.profile.as_deref().unwrap_or(""),
            font,
        ));
        let zoom = self
            .segments
            .zoom_percent
            .map(|percent| format!("{percent:+}%"));
        state
            .zoom
            .update(segment_text(zoom.as_deref().unwrap_or(""), font));

        // The path's budget: the row width minus both side paddings,
        // the disc cluster, the process name, and the measured right
        // cluster with its gaps. An over-budget path elides by the
        // average glyph width of its full layout — a view heuristic
        // the hardware pass eyeballs at narrow widths.
        let mut right_width = SEGMENT_GAP + state.zoom.min_width();
        if self.segments.profile.is_some() {
            right_width += SEGMENT_GAP + state.profile.min_width();
        }
        let left_fixed = PAD_X + DISC_SIZE + DISC_GAP + state.process.min_width() + SEGMENT_GAP;
        let budget = (width - left_fixed - right_width - PAD_X).max(0.0);
        let path = self.segments.path.as_deref().unwrap_or("");
        let mut measure = BandParagraph::default();
        measure.update(segment_text(path, font));
        let full_width = measure.min_width();
        let elided = if full_width > budget && !path.is_empty() {
            #[expect(
                clippy::cast_precision_loss,
                reason = "char counts are px-scale strings (< 2^24), so usize -> f32 is exact"
            )]
            let per_char = full_width / path.chars().count() as f32;
            if per_char > 0.0 {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a px-budget char count, far below usize::MAX by construction"
                )]
                let max_chars = (budget / per_char) as usize;
                elide_path_left(path, max_chars)
            } else {
                path.to_string()
            }
        } else {
            path.to_string()
        };
        state.path.update(segment_text(&elided, font));
    }
}

/// One segment's [`Text`] layout — 12 px, advanced shaping, single
/// line.
fn segment_text(content: &str, font: cosmic::iced::Font) -> Text<&str> {
    Text {
        content,
        bounds: Size::INFINITE,
        size: Pixels(SEGMENT_FONT_SIZE),
        line_height: LineHeight::default(),
        font,
        align_x: cosmic::iced::core::text::Alignment::Left,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
        wrapping: Wrapping::default(),
        ellipsize: cosmic::iced::core::text::Ellipsize::None,
    }
}

impl<Message> Widget<Message, Theme, Renderer> for StatusBar<Message>
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
        Size::new(Length::Fill, Length::Fixed(BAND_HEIGHT))
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        let limits = limits
            .width(Length::Fill)
            .height(Length::Fixed(BAND_HEIGHT));
        let size = limits.resolve(
            Length::Fill,
            Length::Fixed(BAND_HEIGHT),
            Size::new(limits.max().width, BAND_HEIGHT),
        );
        self.refresh_paragraphs(state, size.width);
        layout::Node::new(size)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor_position: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let (band_bg, pane_fg) = pane_colors();

        // The band's own opaque strip, the tab bar's derivation:
        // window background × 0.92, so the band reads as its own band
        // whatever scheme is live — a scheme flip recolors on the
        // next render (criterion 3).
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
            band_bg,
        );

        // The process-state disc: filled while a foreground child
        // runs, a hollow ring over the idle shell — the gear's own
        // disc primitive.
        let center_y = bounds.y + BAND_HEIGHT / 2.0;
        let disc = Rectangle::new(
            Point::new(bounds.x + PAD_X, center_y - DISC_SIZE / 2.0),
            Size::new(DISC_SIZE, DISC_SIZE),
        );
        if self.segments.running {
            fill_disc(renderer, disc, pane_fg);
        } else {
            renderer.fill_quad(
                Quad {
                    bounds: disc,
                    border: Border {
                        radius: (DISC_SIZE / 2.0).into(),
                        width: DISC_RING_WIDTH,
                        color: pane_fg,
                    },
                    snap: true,
                    ..Default::default()
                },
                Color::TRANSPARENT,
            );
        }

        // Left cluster: the process name after the disc, then the
        // path — full-alpha foreground, each clipped to the band.
        let text_y =
            |paragraph: &BandParagraph| bounds.y + (BAND_HEIGHT - paragraph.min_height()) / 2.0;
        let mut x = bounds.x + PAD_X + DISC_SIZE + DISC_GAP;
        renderer.fill_paragraph(
            state.process.raw(),
            Point::new(x, text_y(&state.process)),
            pane_fg,
            Rectangle::new(
                Point::new(x, bounds.y),
                Size::new(state.process.min_width().max(0.0), BAND_HEIGHT),
            ),
        );
        x += state.process.min_width() + SEGMENT_GAP;
        renderer.fill_paragraph(
            state.path.raw(),
            Point::new(x, text_y(&state.path)),
            pane_fg,
            Rectangle::new(
                Point::new(x, bounds.y),
                Size::new(
                    (bounds.width - PAD_X - (x - bounds.x)).max(0.0),
                    BAND_HEIGHT,
                ),
            ),
        );

        // Right cluster: profile, then zoom at the right edge —
        // dimmed, hierarchy by alpha.
        let dimmed = Color {
            a: DIM_ALPHA,
            ..pane_fg
        };
        let mut right = bounds.x + bounds.width - PAD_X;
        if self.segments.zoom_percent.is_some() {
            right -= state.zoom.min_width();
            renderer.fill_paragraph(
                state.zoom.raw(),
                Point::new(right, text_y(&state.zoom)),
                dimmed,
                Rectangle::new(
                    Point::new(right, bounds.y),
                    Size::new(state.zoom.min_width().max(0.0), BAND_HEIGHT),
                ),
            );
            right -= SEGMENT_GAP;
        }
        if self.segments.profile.is_some() {
            right -= state.profile.min_width();
            renderer.fill_paragraph(
                state.profile.raw(),
                Point::new(right, text_y(&state.profile)),
                dimmed,
                Rectangle::new(
                    Point::new(right, bounds.y),
                    Size::new(state.profile.min_width().max(0.0), BAND_HEIGHT),
                ),
            );
        }
    }
}

impl<Message> From<StatusBar<Message>> for Element<'_, Message>
where
    Message: Clone + 'static,
{
    fn from(status_bar: StatusBar<Message>) -> Self {
        Self::new(status_bar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> std::path::PathBuf {
        std::path::PathBuf::from("/home/jpt")
    }

    // --- segment assembly (running/idle × profile × zoom × cwd) ---

    #[test]
    fn a_running_child_names_the_process_and_fills_the_disc() {
        let segments = resolve_segments(
            Some(&home()),
            Some(Path::new("/home/jpt/src/oss/l1br3/ced2")),
            "/usr/bin/fish",
            Some("dev"),
            2,
            100,
            Some("cargo"),
        );
        assert!(segments.running);
        assert_eq!(segments.process, "cargo");
        assert_eq!(segments.path.as_deref(), Some("~/src/oss/l1br3/ced2"));
        assert_eq!(segments.profile.as_deref(), Some("dev"));
        assert_eq!(segments.zoom_percent, Some(200));
    }

    #[test]
    fn an_idle_tab_falls_back_to_the_shell_basename_and_a_hollow_disc() {
        let segments = resolve_segments(
            Some(&home()),
            Some(Path::new("/home/jpt")),
            "/usr/bin/fish",
            None,
            0,
            100,
            None,
        );
        assert!(!segments.running);
        assert_eq!(segments.process, "fish");
        assert_eq!(segments.path.as_deref(), Some("~"));
        assert_eq!(segments.profile, None);
        assert_eq!(segments.zoom_percent, None);
    }

    #[test]
    fn an_unreadable_comm_falls_back_like_idle() {
        // A failed /proc read is indistinguishable from idle in its
        // inputs — the band stays honest either way.
        let segments = resolve_segments(
            Some(&home()),
            Some(Path::new("/srv")),
            "/bin/bash",
            Some("ops"),
            1,
            100,
            None,
        );
        assert!(!segments.running);
        assert_eq!(segments.process, "bash");
        assert_eq!(segments.profile.as_deref(), Some("ops"));
    }

    #[test]
    fn a_missing_cwd_drops_only_the_path_segment() {
        let segments = resolve_segments(
            Some(&home()),
            None,
            "/usr/bin/zsh",
            Some("dev"),
            1,
            100,
            Some("vim"),
        );
        assert_eq!(segments.path, None);
        assert!(segments.running);
        assert_eq!(segments.process, "vim");
        assert_eq!(segments.profile.as_deref(), Some("dev"));
        assert_eq!(segments.zoom_percent, Some(100));
    }

    #[test]
    fn a_dangling_profile_id_hides_the_segment() {
        // The app resolves the id before feeding the resolver; a
        // missing profile arrives as `None`, never a stale name.
        let segments = resolve_segments(Some(&home()), None, "/usr/bin/zsh", None, 0, 100, None);
        assert_eq!(segments.profile, None);
        assert_eq!(segments.zoom_percent, None);
    }

    // --- the ~ collapse ---

    #[test]
    fn a_home_prefixed_path_collapses_to_the_tilde() {
        assert_eq!(
            collapse_home(Path::new("/home/jpt/src/oss"), Some(&home())),
            "~/src/oss"
        );
    }

    #[test]
    fn home_itself_collapses_to_a_bare_tilde() {
        assert_eq!(collapse_home(&home(), Some(&home())), "~");
    }

    #[test]
    fn a_foreign_path_stays_verbatim() {
        assert_eq!(
            collapse_home(Path::new("/srv/web"), Some(&home())),
            "/srv/web"
        );
    }

    #[test]
    fn a_missing_home_keeps_paths_verbatim() {
        // An unset `HOME` — never in a real desktop session — must
        // not mangle paths into a bogus tilde form.
        assert_eq!(
            collapse_home(Path::new("/home/jpt/src"), None),
            "/home/jpt/src"
        );
    }

    #[test]
    fn a_sibling_sharing_the_home_prefix_does_not_collapse() {
        // Component boundaries, not string prefixes: `/home/jpt-x`
        // does not live inside `/home/jpt`.
        assert_eq!(
            collapse_home(Path::new("/home/jpt-x/thing"), Some(&home())),
            "/home/jpt-x/thing"
        );
    }

    // --- left elision ---

    #[test]
    fn short_paths_pass_through_untouched() {
        assert_eq!(elide_path_left("~/src", 100), "~/src");
        assert_eq!(elide_path_left("~/src", 5), "~/src");
    }

    #[test]
    fn long_paths_keep_their_tail_under_a_leading_ellipsis() {
        assert_eq!(elide_path_left("~/src/oss/l1br3/ced2", 12), "…/l1br3/ced2");
        assert_eq!(
            elide_path_left("/srv/web/prod/current", 13),
            "…prod/current"
        );
    }

    #[test]
    fn degenerate_widths_elide_to_the_floor() {
        assert_eq!(elide_path_left("~/src/oss", 0), "");
        assert_eq!(elide_path_left("~/src/oss", 1), "…");
    }

    // --- zoom arithmetic ---

    #[test]
    fn zoom_percent_multiplies_the_step_signed() {
        assert_eq!(zoom_percent(2, 100), Some(200));
        assert_eq!(zoom_percent(-1, 100), Some(-100));
        assert_eq!(zoom_percent(3, 50), Some(150));
    }

    #[test]
    fn zoom_percent_hides_at_zero_adjustment() {
        assert_eq!(zoom_percent(0, 100), None);
        assert_eq!(zoom_percent(0, 0), None);
    }

    #[test]
    fn zoom_percent_saturates_rather_than_wrapping() {
        assert_eq!(zoom_percent(i8::MAX, u16::MAX), Some(i16::MAX));
        assert_eq!(zoom_percent(i8::MIN, u16::MAX), Some(i16::MIN));
    }

    // --- the stacking rule ---

    #[test]
    fn top_anchor_with_a_top_pinned_strip_keeps_the_band_beside_the_grab_bar() {
        assert_eq!(
            column_stacking(Edge::Top, TabPosition::Top),
            [
                Strip::Tabs,
                Strip::Terminal,
                Strip::StatusBand,
                Strip::GrabBar
            ]
        );
    }

    #[test]
    fn top_anchor_with_an_auto_or_bottom_strip_stacks_the_strip_beside_the_grab_bar() {
        // Auto keeps the strip opposite the screen edge — surface
        // bottom under a Top anchor, beside the grab bar; an explicit
        // Bottom pin lands the same way.
        let expected = [
            Strip::Terminal,
            Strip::Tabs,
            Strip::StatusBand,
            Strip::GrabBar,
        ];
        assert_eq!(column_stacking(Edge::Top, TabPosition::Auto), expected);
        assert_eq!(column_stacking(Edge::Top, TabPosition::Bottom), expected);
    }

    #[test]
    fn bottom_anchor_with_an_auto_or_top_strip_stacks_the_band_under_the_grab_bar() {
        // The grab bar leads the column at the Bottom anchor's free
        // edge (the surface top); the band takes the slot right under
        // it.
        let expected = [
            Strip::GrabBar,
            Strip::StatusBand,
            Strip::Tabs,
            Strip::Terminal,
        ];
        assert_eq!(column_stacking(Edge::Bottom, TabPosition::Auto), expected);
        assert_eq!(column_stacking(Edge::Bottom, TabPosition::Top), expected);
    }

    #[test]
    fn bottom_anchor_with_a_bottom_pinned_strip_keeps_the_band_beside_the_grab_bar() {
        assert_eq!(
            column_stacking(Edge::Bottom, TabPosition::Bottom),
            [
                Strip::GrabBar,
                Strip::StatusBand,
                Strip::Terminal,
                Strip::Tabs,
            ]
        );
    }

    #[test]
    fn the_band_is_adjacent_to_the_grab_bar_in_every_stacking() {
        for (edge, position) in [
            (Edge::Top, TabPosition::Auto),
            (Edge::Top, TabPosition::Top),
            (Edge::Top, TabPosition::Bottom),
            (Edge::Bottom, TabPosition::Auto),
            (Edge::Bottom, TabPosition::Top),
            (Edge::Bottom, TabPosition::Bottom),
        ] {
            let stacking = column_stacking(edge, position);
            let band = stacking
                .iter()
                .position(|strip| *strip == Strip::StatusBand)
                .expect("the band stacks in every order");
            let grab = stacking
                .iter()
                .position(|strip| *strip == Strip::GrabBar)
                .expect("the grab bar stacks in every order");
            assert_eq!(
                band.abs_diff(grab),
                1,
                "band must sit beside the grab bar: {edge:?}/{position:?} -> {stacking:?}"
            );
            // The grab bar stays at the free edge — the surface edge
            // opposite the screen anchor — wherever the strips pin.
            let expected_grab = if matches!(edge, Edge::Bottom) {
                0
            } else {
                stacking.len() - 1
            };
            assert_eq!(
                grab, expected_grab,
                "grab bar pins to the free edge: {edge:?}/{position:?} -> {stacking:?}"
            );
        }
    }

    // --- the idle name ---

    #[test]
    fn the_shell_basename_strips_the_directory() {
        assert_eq!(shell_basename("/usr/bin/fish"), "fish");
        assert_eq!(shell_basename("zsh"), "zsh");
    }

    // --- the widget's construction pin (Task 2) ---

    #[test]
    fn the_band_constructs_from_plain_resolved_data_only() {
        // The construction pin: the widget takes the resolved
        // segments and a font — no terminal handle, no engine import
        // — so the render path cannot read `/proc` by construction;
        // the app-side snapshot refresh stays the lone caller of the
        // engine's comm read (criterion 5).
        let segments = StatusSegments {
            running: true,
            process: "cargo".to_string(),
            path: Some("~/src".to_string()),
            profile: Some("dev".to_string()),
            zoom_percent: Some(20),
        };
        let band: StatusBar<()> = StatusBar::new(segments.clone(), cosmic::iced::Font::default());
        assert_eq!(band.segments, segments);
    }
}
