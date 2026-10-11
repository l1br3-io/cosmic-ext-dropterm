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
//!
//! The set is eight — process, path, branch in the left cluster;
//! profile, grid, tab position, zoom, clock in the right — each
//! individually toggleable (a hidden segment is never computed) and
//! click-copyable through [`copy_text`]: the full value, never the
//! elided display.

use std::path::Path;

use cosmic::iced::core::Shell;
use cosmic::iced::core::alignment;
use cosmic::iced::core::clipboard::{Clipboard, Kind};
use cosmic::iced::core::overlay;
use cosmic::iced::core::text::{LineHeight, Shaping, paragraph};
use cosmic::iced::core::text::{Text, Wrapping};
use cosmic::iced::core::{
    Border, Layout, Rectangle, Size, Vector, layout,
    renderer::{self, Quad, Renderer as _},
    text::Renderer as _,
    widget::{Tree, Widget, tree},
};
use cosmic::iced::event::Event;
use cosmic::iced::mouse::{Button, Cursor, Event as MouseEvent};
use cosmic::iced::{Color, Length, Pixels, Point, mouse};
use cosmic::theme::Theme;
use cosmic::{Element, Renderer};

use crate::config::{Edge, TabPosition};
use crate::tab_bar::{
    DROPDOWN_GAP, DROPDOWN_PADDING_X, DROPDOWN_ROW_HEIGHT, TooltipSide, dropdown_row_at, fill_disc,
    pane_colors,
};
use crate::terminal::Action;

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

/// Which band region a segment owns — the shared id for cluster
/// order, visibility, hit-testing, and copy payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentId {
    /// The state disc + the process name (one click target).
    Process,
    /// The `~`-collapsed cwd.
    Path,
    /// The cwd's git branch (or detached sha).
    Branch,
    /// The id-resolved profile name.
    Profile,
    /// The terminal's cell dimensions.
    Grid,
    /// The active tab's position.
    Tab,
    /// The signed zoom percent.
    Zoom,
    /// The pre-formatted wall clock.
    Clock,
}

/// The left cluster's segments in draw order — full alpha.
pub const LEFT_CLUSTER: [SegmentId; 3] = [SegmentId::Process, SegmentId::Path, SegmentId::Branch];

/// The right cluster's segments in draw order — the dimmed alpha,
/// the clock rightmost (the tmux seat).
pub const RIGHT_CLUSTER: [SegmentId; 5] = [
    SegmentId::Profile,
    SegmentId::Grid,
    SegmentId::Tab,
    SegmentId::Zoom,
    SegmentId::Clock,
];

/// Drop precedence under width pressure — first-dropped first.
/// Process and path are absent by construction: the disc never
/// yields, and the path elides before anything else gives way.
/// Facts the shell itself knows (`$COLUMNS×$LINES`) go first,
/// then slow-moving identity (tab position, zoom, profile), the
/// branch stays beside its path, and the clock is last — tmux
/// keeps time to the bitter end.
pub const DROP_ORDER: [SegmentId; 6] = [
    SegmentId::Grid,
    SegmentId::Tab,
    SegmentId::Zoom,
    SegmentId::Profile,
    SegmentId::Branch,
    SegmentId::Clock,
];

/// The per-segment visibility switches, folded from the config's
/// eight `status_bar_show_*` bools. A hidden segment is absent
/// downstream — and its costly inputs are never gathered.
#[expect(
    clippy::struct_excessive_bools,
    reason = "eight independent visibility switches — one per config key, by design"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusToggles {
    /// The state disc + process name.
    pub process: bool,
    /// The `~`-collapsed cwd.
    pub path: bool,
    /// The git branch of the cwd.
    pub branch: bool,
    /// The profile name.
    pub profile: bool,
    /// The terminal's cell dimensions.
    pub grid: bool,
    /// The tab position.
    pub tab: bool,
    /// The zoom percent.
    pub zoom: bool,
    /// The wall clock.
    pub clock: bool,
}

impl StatusToggles {
    /// Every segment shown — the all-on default the config's eight
    /// `true` keys fold into.
    pub const fn all() -> Self {
        Self {
            process: true,
            path: true,
            branch: true,
            profile: true,
            grid: true,
            tab: true,
            zoom: true,
            clock: true,
        }
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
    /// idle/unreadable. Empty only when the toggle hides the segment.
    pub process: String,
    /// The `~`-collapsed cwd, absent when the tab has none.
    pub path: Option<String>,
    /// The tab's id-resolved profile name, absent on plain-shell tabs
    /// (and on dangling ids — never a stale name).
    pub profile: Option<String>,
    /// The signed zoom percent (`zoom_adj × step`), absent at zero.
    pub zoom_percent: Option<i16>,
    /// The cwd's git branch — a ref name, or a detached sha's first
    /// 7 characters — absent outside repos, when hidden, and when
    /// the reads fail (never a wrong name).
    pub branch: Option<String>,
    /// The terminal's cell dimensions (cols, rows).
    pub grid: Option<(u16, u16)>,
    /// The active tab's position (index, total).
    pub tab_position: Option<(u16, u16)>,
    /// The pre-formatted wall clock (`HH:MM`) — the widget never
    /// formats time; the app's tick stores the string.
    pub clock: Option<String>,
}

/// The app-side inputs to [`resolve_segments`] — plain data only, so
/// the resolver stays pure and headless-testable. The app feeds it
/// the `~`-collapse base, the cwd, the recorded shell, the
/// id-resolved profile name, the zoom adjustment and step, the
/// process-name and branch snapshots, the grid with its spawn record,
/// the tab position, and the stored clock string.
pub struct StatusInputs<'a> {
    /// The `~`-collapse base; `None` (an unset `HOME` — never in a
    /// real desktop session) keeps paths verbatim.
    pub home: Option<&'a Path>,
    /// The active tab's cwd.
    pub cwd: Option<&'a Path>,
    /// The shell the tab's spawn recorded.
    pub shell: &'a str,
    /// The id-resolved profile name.
    pub profile_name: Option<&'a str>,
    /// The terminal's live zoom adjustment.
    pub zoom_adj: i8,
    /// The zoom step the tab's terminal actually zooms by.
    pub zoom_step_mul_100: u16,
    /// The foreground-process snapshot (the app-side comm read).
    pub foreground: Option<&'a str>,
    /// The git-branch snapshot (the app-side `.git` read).
    pub branch: Option<&'a str>,
    /// The terminal's cell dimensions (cols, rows).
    pub grid: Option<(u16, u16)>,
    /// The grid recorded at the tab's spawn (status-glance
    /// criterion 1) — the resting grid the live one measures
    /// deviation against; `None` (no record) shows the live grid,
    /// the honest reading when default was never witnessed.
    pub spawn_grid: Option<(u16, u16)>,
    /// The active tab's position (index, total).
    pub tab_position: Option<(u16, u16)>,
    /// The stored wall-clock string.
    pub clock: Option<&'a str>,
}

/// Assemble the active tab's segments from the inputs, hidden
/// segments dropped — hidden and absent are indistinguishable
/// downstream, which is what lets the app skip gathering a hidden
/// segment's inputs entirely.
pub fn resolve_segments(inputs: &StatusInputs, toggles: StatusToggles) -> StatusSegments {
    StatusSegments {
        running: inputs.foreground.is_some(),
        process: if toggles.process {
            inputs
                .foreground
                .map_or_else(|| shell_basename(inputs.shell), str::to_string)
        } else {
            String::new()
        },
        path: toggles
            .path
            .then(|| inputs.cwd.map(|cwd| collapse_home(cwd, inputs.home)))
            .flatten(),
        branch: toggles
            .branch
            .then_some(inputs.branch)
            .flatten()
            .map(str::to_string),
        profile: toggles
            .profile
            .then_some(inputs.profile_name)
            .flatten()
            .map(str::to_string),
        grid: toggles
            .grid
            .then_some(inputs.grid)
            .flatten()
            // The resized-from-spawn condition: equal means absent —
            // the same indistinguishable `None` a toggle-off segment
            // resolves to, so default and hidden cannot be told
            // apart downstream. An absent record shows the live grid.
            .filter(|&live| Some(live) != inputs.spawn_grid),
        tab_position: toggles.tab.then_some(inputs.tab_position).flatten(),
        zoom_percent: toggles
            .zoom
            .then(|| zoom_percent(inputs.zoom_adj, inputs.zoom_step_mul_100))
            .flatten(),
        clock: toggles
            .clock
            .then_some(inputs.clock)
            .flatten()
            .map(str::to_string),
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

/// How the path renders after the left cluster's squeeze decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathFit {
    /// The whole path fits.
    Full,
    /// Elide to `max_chars` (`…` plus the kept tail).
    Elided(usize),
    /// Even elision leaves nothing — the segment is gone.
    Gone,
}

/// The left cluster's squeeze decision under a pixel `budget`: the
/// path elides first (keeping its tail); the branch drops only when
/// even a fully elided path cannot make room beside it.
/// `path_chars` counts the path's characters; `path_full_width` and
/// `branch_width` are the measured pixel widths (`branch_width` of
/// zero when the branch is absent). Deterministic priority: path
/// over branch.
pub fn left_squeeze(
    budget: f32,
    path_chars: usize,
    path_full_width: f32,
    branch_width: f32,
) -> (PathFit, bool) {
    let branch = branch_width > 0.0;
    if path_chars == 0 {
        // Nothing to squeeze on the path's side — the branch either
        // fits or it does not.
        return (PathFit::Full, branch && branch_width <= budget);
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "char counts are px-scale strings (< 2^24), exact in f32"
    )]
    let per_char = path_full_width / path_chars as f32;
    if per_char <= 0.0 {
        return (PathFit::Full, branch && branch_width <= budget);
    }
    let branch_cost = if branch {
        SEGMENT_GAP + branch_width
    } else {
        0.0
    };
    if path_full_width + branch_cost <= budget {
        return (PathFit::Full, branch);
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a px-budget char count, far below usize::MAX by construction"
    )]
    let mut max_chars = ((budget - branch_cost).max(0.0) / per_char) as usize;
    let mut branch_shown = branch;
    if branch && max_chars == 0 {
        // Full elision still cannot fit the branch — the branch
        // gives way and the path elides against the whole budget.
        branch_shown = false;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a px-budget char count, far below usize::MAX by construction"
        )]
        let against_whole_budget = (budget.max(0.0) / per_char) as usize;
        max_chars = against_whole_budget;
    }
    if max_chars == 0 {
        return (PathFit::Gone, false);
    }
    (PathFit::Elided(max_chars), branch_shown)
}

/// The path's measured inputs to [`band_fit`]: its character count
/// and full un-elided pixel width (the per-character width is the
/// quotient — monospace cells).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathMeasure {
    /// The path's character count.
    pub chars: usize,
    /// The whole path's measured pixel width.
    pub full_width: f32,
}

/// The band's fit decision under a row width: which segments
/// render, and how the path elides into the room that remains.
/// Pure over measured widths — deterministic for a fixed input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BandFit {
    /// The path's render decision (the inner [`left_squeeze`]
    /// step).
    pub path: PathFit,
    /// The segments that give way, in drop order — a prefix of
    /// [`DROP_ORDER`] restricted to the measured (present) ones.
    pub dropped: Vec<SegmentId>,
}

/// One fit pass over the whole band under a row `width`
/// (status-glance criterion 2): the path elides first,
/// progressively; when even a fully elided path cannot fit,
/// segments drop one at a time in [`DROP_ORDER`], skipping absent
/// ones, and the path re-expands into each freed width. `fixed` is
/// the never-yielding consumption — both side paddings and the
/// process's disc cluster with its gaps; `widths` are the measured
/// widths of every other present segment (path and process
/// excluded — the path elides, the disc never yields). Survivors
/// keep their cluster seats: positioning is stable through every
/// drop step, never uniformly compressed. Deterministic for a
/// fixed input — fewest drops, then least elision.
pub fn band_fit(width: f32, fixed: f32, widths: &[(SegmentId, f32)], path: PathMeasure) -> BandFit {
    let mut dropped: Vec<SegmentId> = Vec::new();
    let fit = loop {
        let consumed: f32 = widths
            .iter()
            .filter(|(id, _)| !dropped.contains(id))
            .map(|(_, w)| SEGMENT_GAP + w)
            .sum();
        let room = width - fixed - consumed;
        // The path's inner step: [`left_squeeze`] as pure elision —
        // the branch rides `widths` like any droppable segment, so
        // its eager internal yield is subsumed by the patient
        // DROP_ORDER loop above it.
        let (fit, _) = left_squeeze(room, path.chars, path.full_width, 0.0);
        if !matches!(fit, PathFit::Gone) {
            break fit;
        }
        // Even a fully elided path cannot fit — the next present
        // segment in precedence order gives way.
        match DROP_ORDER
            .iter()
            .find(|id| widths.iter().any(|(wid, _)| wid == *id) && !dropped.contains(id))
        {
            Some(next) => dropped.push(*next),
            None => break fit,
        }
    };
    BandFit { path: fit, dropped }
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

/// The full value a segment's click copies — the un-elided,
/// un-decorated string; a path the band would squeeze to `…/ced2`
/// still copies whole. An absent segment copies nothing (hit-testing
/// only yields rendered segments, so this is defensive).
pub fn copy_text(id: SegmentId, segments: &StatusSegments) -> String {
    match id {
        SegmentId::Process => segments.process.clone(),
        SegmentId::Path => segments.path.clone().unwrap_or_default(),
        SegmentId::Branch => segments.branch.clone().unwrap_or_default(),
        SegmentId::Profile => segments.profile.clone().unwrap_or_default(),
        SegmentId::Grid => segments
            .grid
            .map(|(cols, rows)| format!("{cols}×{rows}"))
            .unwrap_or_default(),
        SegmentId::Tab => segments
            .tab_position
            .map(|(index, total)| format!("{index}/{total}"))
            .unwrap_or_default(),
        SegmentId::Zoom => segments
            .zoom_percent
            .map(|percent| format!("{percent:+}%"))
            .unwrap_or_default(),
        SegmentId::Clock => segments.clock.clone().unwrap_or_default(),
    }
}

/// The segment whose rect contains `point`, if any — the pure half
/// of the band's hit-testing; the widget supplies the laid-out
/// rects. Gaps and everything outside the row belong to no segment.
pub fn segment_at(rects: &[(SegmentId, Rectangle)], point: Point) -> Option<SegmentId> {
    rects
        .iter()
        .find(|(_, rect)| rect.contains(point))
        .map(|(id, _)| *id)
}

/// Whether the inline reveal is armed (status-glance criterion 3,
/// the OQ-1 inline ruling): the path's display must be elided and
/// the cursor must rest on its **current** display rect — the
/// elided strip at rest, the expanded strip once armed (the
/// expanded rect contains the elided one, so arming is stable
/// through the swap). An un-elided path's display already is its
/// value, other segments arm nothing, and a Gone path has no rect.
/// Instant (OQ-3); disarming is absence.
pub fn reveal_armed(
    path_rect: Option<Rectangle>,
    cursor: Option<Point>,
    path_elided: bool,
) -> bool {
    path_elided && matches!((path_rect, cursor), (Some(rect), Some(point)) if rect.contains(point))
}

/// The armed frame's path display — the band's fit with the
/// branch's seat lent to the path (the branch stays beside its
/// path but yields it while the path is examined, v1's priority
/// inverted for one frame-set): usually the whole value, always at
/// least a longer elision than the resting one. `widths` are the
/// idle fit's survivors (already pruned of its drops). The press
/// payload never follows the display — it stays [`copy_text`]'s
/// full value.
pub fn reveal_display(
    path: &str,
    width: f32,
    fixed: f32,
    widths: &[(SegmentId, f32)],
    measure: PathMeasure,
) -> String {
    let lent: Vec<(SegmentId, f32)> = widths
        .iter()
        .copied()
        .filter(|(id, _)| *id != SegmentId::Branch)
        .collect();
    match band_fit(width, fixed, &lent, measure).path {
        PathFit::Full => path.to_string(),
        PathFit::Elided(max_chars) => elide_path_left(path, max_chars),
        PathFit::Gone => String::new(),
    }
}

// --- text selection (the user-ruled interaction, 2026-10-11) ---
//
// Press-and-drag across the band selects the covered characters of
// the row's display text and the release copies them (clipboard +
// primary, so the terminal's middle-click paste serves the
// selection). A click — a press that never crossed the threshold —
// keeps the shipped verb: the segment's full un-elided value and
// the `✓` flash. The highlight persists after release, like
// terminal selection, and clears on the next press.

/// One segment's selection inputs: its laid rect and its display
/// text — the monospace terminal font makes character offsets
/// px-derived, the per-character width being the rect's width over
/// the text's character count.
pub struct RowSegment {
    /// The segment's laid-out rect.
    pub rect: Rectangle,
    /// The segment's display text.
    pub text: String,
}

/// The selection-drag threshold: a press that stays within it is a
/// click, past it a selection — the tab row's reorder-drag idiom
/// at a text-selection scale.
const SELECTION_THRESHOLD: f32 = 4.0;

/// The alpha of the selection highlight — the pane foreground at a
/// quarter strength, hierarchy by alpha, never a new hue.
const SELECTION_ALPHA: f32 = 0.25;

/// Whether a press-release pair is a click (it rides the copy
/// verb) rather than a drag (it selects).
pub fn selection_is_click(anchor: Point, release: Point) -> bool {
    let dx = release.x - anchor.x;
    let dy = release.y - anchor.y;
    (dx * dx + dy * dy) < SELECTION_THRESHOLD * SELECTION_THRESHOLD
}

/// The normalized horizontal span of a selection drag.
fn selection_span(anchor: Point, focus: Point) -> (f32, f32) {
    if anchor.x <= focus.x {
        (anchor.x, focus.x)
    } else {
        (focus.x, anchor.x)
    }
}

/// The highlight quads of a selection: the span intersected with
/// each row segment's rect — the visual mark, painted under the
/// text on the armed frames.
pub fn selection_rects(anchor: Point, focus: Point, row: &[RowSegment]) -> Vec<Rectangle> {
    let (lo, hi) = selection_span(anchor, focus);
    row.iter()
        .map(|seg| {
            let left = lo.max(seg.rect.x);
            let right = hi.min(seg.rect.x + seg.rect.width);
            Rectangle::new(
                Point::new(left, seg.rect.y),
                Size::new((right - left).max(0.0), seg.rect.height),
            )
        })
        .filter(|rect| rect.width > 0.0)
        .collect()
}

/// The selected text: the characters whose cells the span covers,
/// segments joined by single spaces (the visual gaps) — WYSIWYG,
/// the elided display included; the click verb remains the full
/// value through [`copy_text`].
pub fn selection_text(anchor: Point, focus: Point, row: &[RowSegment]) -> String {
    let (lo, hi) = selection_span(anchor, focus);
    let mut parts: Vec<String> = Vec::new();
    for seg in row {
        let chars: Vec<char> = seg.text.chars().collect();
        if chars.is_empty() {
            continue;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "px-scale char math on strings < 2^24 chars, exact in f32"
        )]
        let len = chars.len() as f32;
        let per_char = seg.rect.width / len;
        if per_char <= 0.0 {
            continue;
        }
        // A character is covered when its cell intersects the span;
        // the cell indices clamp into range before the usize cast.
        let cells = |edge: f32, round: fn(f32) -> f32| {
            round((edge - seg.rect.x) / per_char).clamp(0.0, len)
        };
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a char index clamped into 0..=len from a px offset"
        )]
        let first = cells(lo, f32::floor) as usize;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a char index clamped into 0..=len from a px offset"
        )]
        let last = cells(hi, f32::ceil) as usize;
        if last > first {
            parts.push(chars[first..last].iter().collect());
        }
    }
    parts.join(" ")
}

// --- the context menu (the per-tab menu's pattern) ---

/// One pick a status-menu row carries. The copy rows resolve
/// against the live segments at pick time — the pressed segment
/// for [`MenuPick::CopySegment`], every present one for
/// [`MenuPick::CopyAll`] — while every action row routes to an
/// existing [`Action`] funnel, the keybinds' own: the menu
/// introduces no behavior of its own (the per-tab menu's
/// contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuPick {
    /// Copy the pressed segment's full [`copy_text`] value — the
    /// click verb's twin, ✓ flash included.
    CopySegment(SegmentId),
    /// Copy every present segment's full value, space-joined (the
    /// selection release's join rule).
    CopyAll,
    /// An existing terminal/app action — Paste, the zoom trio, the
    /// band's own toggle, Settings.
    Action(Action),
}

/// One rendered row of the band's context menu — the per-tab
/// menu's row shape: label, the pick a left press publishes
/// (`None` when the row has nothing to pick — the Copy row over a
/// gap press), whether it renders active (the GNOME disabled-row
/// rule), and whether a separator line renders above it.
struct MenuRow {
    /// Label drawn in the row.
    label: String,
    /// The pick a left press publishes.
    pick: Option<MenuPick>,
    /// Whether the row renders active and publishes its pick.
    enabled: bool,
    /// Whether a separator line renders above the row.
    separated: bool,
}

/// An interactive row.
fn pick_row(label: &str, pick: MenuPick, enabled: bool, separated: bool) -> MenuRow {
    MenuRow {
        label: label.to_owned(),
        pick: Some(pick),
        enabled,
        separated,
    }
}

/// The band's context-menu rows for the live segments and the
/// opening press's segment — the clipboard cluster (the pressed
/// segment's value, every present segment's value, paste into the
/// narrated terminal), the zoom cluster the Zoom segment narrates
/// (Reset muted at base size — `zoom_percent`'s own condition),
/// and the band's lifecycle rows. Labels are static; enablement is
/// live, re-derived each frame, so a keybind passing through the
/// open menu cannot leave a stale enabled row. Muted rows — a gap
/// press's Copy, an empty payload — render disabled and never
/// publish, the tab menu's edge-move rule.
fn menu_rows(segments: &StatusSegments, pressed: Option<SegmentId>) -> Vec<MenuRow> {
    // Copy resolves its payload against the same live segments at
    // pick time; the row mutes when the press scoped no segment or
    // the segment's live value is empty.
    let copy_enabled = pressed.is_some_and(|id| !copy_text(id, segments).is_empty());
    vec![
        MenuRow {
            label: "Copy".to_owned(),
            pick: pressed.map(MenuPick::CopySegment),
            enabled: copy_enabled,
            separated: false,
        },
        pick_row(
            "Copy All",
            MenuPick::CopyAll,
            !copy_all_text(segments).is_empty(),
            false,
        ),
        pick_row("Paste", MenuPick::Action(Action::Paste), true, false),
        pick_row("Zoom In", MenuPick::Action(Action::ZoomIn), true, true),
        pick_row("Zoom Out", MenuPick::Action(Action::ZoomOut), true, false),
        pick_row(
            "Reset Zoom",
            MenuPick::Action(Action::ZoomReset),
            segments.zoom_percent.is_some(),
            false,
        ),
        pick_row(
            "Hide Status Bar",
            MenuPick::Action(Action::ToggleStatusBar),
            true,
            true,
        ),
        // The ellipsis (U+2026) marks an item that opens a window —
        // the terminal menu's Settings… rule.
        pick_row(
            "Settings\u{2026}",
            MenuPick::Action(Action::Settings),
            true,
            false,
        ),
    ]
}

/// The Copy All payload: every present segment's full [`copy_text`]
/// value in row order (left cluster then right), joined by single
/// spaces — the selection release's join rule; full values, never
/// displays. Empty when nothing is present.
fn copy_all_text(segments: &StatusSegments) -> String {
    LEFT_CLUSTER
        .iter()
        .chain(RIGHT_CLUSTER.iter())
        .map(|id| copy_text(*id, segments))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The menu's rect, band-local: anchored at the opening
/// right-press's x, clamped so the bubble never overflows either
/// end of the band (the per-tab menu's both-edges rule), popping
/// to `side` — the terminal's side, the only side with surface
/// under it. Width from the widest row label plus the shared menu
/// padding, rows stacked on the shared menu row height (the tab
/// strip's menus' metrics — one idiom).
fn menu_bounds(
    band_width: f32,
    press_x: f32,
    widest_label: f32,
    row_count: usize,
    side: TooltipSide,
) -> Rectangle {
    let width = widest_label + DROPDOWN_PADDING_X * 2.0;
    #[expect(
        clippy::cast_precision_loss,
        reason = "row count is far below 2^24, so usize -> f32 is exact"
    )]
    let height = row_count as f32 * DROPDOWN_ROW_HEIGHT;
    let x = press_x.clamp(0.0, (band_width - width).max(0.0));
    let y = match side {
        TooltipSide::Below => BAND_HEIGHT + DROPDOWN_GAP,
        TooltipSide::Above => -height - DROPDOWN_GAP,
    };
    Rectangle::new(Point::new(x, y), Size::new(width, height))
}

/// The band's wall-clock text: 24-hour `HH:MM`, local zone.
pub fn clock_text(now: &jiff::Zoned) -> String {
    now.strftime("%H:%M").to_string()
}

/// Whether the clock's tick loop should exist — the band shown and
/// the clock segment shown. Evaluated at init, on every config echo,
/// and at every fired tick: a mid-flight toggle stands the loop down
/// within one minute, and the resolver's toggle hides the segment
/// itself immediately.
pub const fn clock_due(band_enabled: bool, show_clock: bool) -> bool {
    band_enabled && show_clock
}

/// The duration from `now` to the next minute boundary — the tick's
/// alignment; an on-boundary `now` waits a full minute.
pub fn next_tick_delay(now: &jiff::Zoned) -> std::time::Duration {
    // Civil seconds are 0..=59 and the subsecond part is strictly
    // below a second, so the difference is always positive and at
    // most a minute.
    let remaining =
        (60 - i64::from(now.second())) * 1_000_000_000 - i64::from(now.subsec_nanosecond());
    std::time::Duration::from_nanos(
        remaining
            .try_into()
            .expect("a minute-boundary delay is 0..=60 s of nanoseconds"),
    )
}

/// The cwd's git branch — bounded file reads, never a subprocess.
/// `<cwd>/.git` a directory reads its `HEAD`; a file (worktrees,
/// submodules) follows its one `gitdir:` indirection — absolute, or
/// relative to the cwd — and reads that `HEAD`. A `ref:
/// refs/heads/<name>` names the branch; anything else is a detached
/// sha whose first 7 hex characters stand in. Unreadable or
/// malformed input resolves `None` — never a wrong name.
pub fn git_branch(cwd: Option<&Path>) -> Option<String> {
    let cwd = cwd?;
    let dot = cwd.join(".git");
    let head_path = if dot.is_dir() {
        dot.join("HEAD")
    } else {
        let gitdir = std::fs::read_to_string(&dot).ok()?;
        let location = gitdir.trim().strip_prefix("gitdir: ")?;
        let dir = Path::new(location);
        let dir = if dir.is_absolute() {
            dir
        } else {
            &*cwd.join(dir)
        };
        dir.join("HEAD")
    };
    parse_head(&std::fs::read_to_string(head_path).ok()?)
}

/// One `HEAD` file's branch: a `ref:` line names the branch after
/// `refs/heads/`; anything else must be a detached sha (hex, at
/// least 7 characters) and shortens to its first 7.
fn parse_head(head: &str) -> Option<String> {
    let head = head.trim_end();
    if let Some(name) = head.strip_prefix("ref: refs/heads/") {
        (!name.is_empty()).then(|| name.to_string())
    } else {
        let sha: String = head.chars().take(7).collect();
        (sha.len() == 7 && sha.chars().all(|c| c.is_ascii_hexdigit())).then_some(sha)
    }
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

/// How long a copied segment flashes its `✓` before the value
/// returns (OQ-3: 800 ms, retunable at the hardware pass).
pub const COPY_FLASH: std::time::Duration = std::time::Duration::from_millis(800);

/// The band's cached paragraph state: one per segment, re-laid from
/// the resolved segments on every root layout (the tab bar's titles'
/// cost model). The inline reveal swaps the path's and branch's
/// paragraphs' text on arming transitions only — between
/// transitions, every read (draw, hit-testing, pressing) sees the
/// same laid display.
#[derive(Default)]
struct State {
    process: BandParagraph,
    path: BandParagraph,
    branch: BandParagraph,
    profile: BandParagraph,
    grid: BandParagraph,
    tab: BandParagraph,
    zoom: BandParagraph,
    clock: BandParagraph,
    /// Whether the inline reveal is armed — the cursor rests on the
    /// (elided) path's display rect.
    armed: bool,
    /// Whether the path's resting display is elided this layout —
    /// the reveal's arming condition.
    path_elided: bool,
    /// The path's resting display (the fit's elision verdict).
    path_resting_text: String,
    /// The path's armed display (the branch's seat lent).
    path_armed_text: String,
    /// The branch's display, cached for its restore on disarm (its
    /// seat is lent while armed).
    branch_text: String,
    /// The row's display texts in x-order (left cluster, then
    /// right), cached every layout — the selection's WYSIWYG
    /// source, paired with the laid rects at event and draw time.
    row_texts: Vec<(SegmentId, String)>,
    /// The press that may become a selection drag — its anchor,
    /// held from press to release (window-absolute, the space
    /// moves and releases speak).
    press: Option<Point>,
    /// The live-or-persisting selection: anchor and focus. Persists
    /// highlighted after release until the next press.
    selection: Option<(Point, Point)>,
    /// Whether the context menu is open. Band-local by the
    /// dropdown's rule — the open state dies with the band's tree
    /// on hide/show.
    menu_open: bool,
    /// The opening right-press's segment (`None` over a gap) — the
    /// Copy row's target, re-resolved against the live segments at
    /// pick time.
    menu_segment: Option<SegmentId>,
    /// The opening right-press's band-local point — the bubble's
    /// anchor.
    menu_point: Point,
    /// The menu's row label paragraphs, cached and synced like the
    /// tab menu's — static labels, so the cache fills once per
    /// open; enablement re-derives live at update and draw.
    menu_paragraphs: Vec<(String, BandParagraph)>,
}

impl State {
    fn paragraph_of(&self, id: SegmentId) -> &BandParagraph {
        match id {
            SegmentId::Process => &self.process,
            SegmentId::Path => &self.path,
            SegmentId::Branch => &self.branch,
            SegmentId::Profile => &self.profile,
            SegmentId::Grid => &self.grid,
            SegmentId::Tab => &self.tab,
            SegmentId::Zoom => &self.zoom,
            SegmentId::Clock => &self.clock,
        }
    }

    fn paragraph_of_mut(&mut self, id: SegmentId) -> &mut BandParagraph {
        match id {
            SegmentId::Process => &mut self.process,
            SegmentId::Path => &mut self.path,
            SegmentId::Branch => &mut self.branch,
            SegmentId::Profile => &mut self.profile,
            SegmentId::Grid => &mut self.grid,
            SegmentId::Tab => &mut self.tab,
            SegmentId::Zoom => &mut self.zoom,
            SegmentId::Clock => &mut self.clock,
        }
    }
}

/// The status band widget: a fixed-height fill-width strip rendering
/// [`StatusSegments`] — disc + process + path + branch in the left
/// cluster; profile, grid, tab position, zoom, and clock in the
/// right — in the terminal's own font at 12 px, colors through the
/// tab strip's shared `pane_colors` derivation. Constructed from
/// plain resolved data only: the render path performs zero `/proc`
/// or `.git` reads by construction — the app-side snapshot refresh
/// is the lone caller of both (status-bar criterion 5, extended).
/// Every rendered segment is one click target: the press writes the
/// segment's full value through [`copy_text`] and reports through
/// `on_copy` for the app's `✓` flash.
pub struct StatusBar<Message> {
    /// The active tab's resolved segments — owned, built fresh by the
    /// app on every view pass.
    segments: StatusSegments,
    /// The terminal's own font (OQ-2: `Family::Name(config.font_name)`
    /// built by the app; the sanctioned fallback if the family does
    /// not resolve at the hardware pass is the default UI font at
    /// 12 px with every other token standing).
    font: cosmic::iced::Font,
    /// The segment currently flashing its `✓`; the app owns the
    /// timing (see [`COPY_FLASH`]).
    copied: Option<SegmentId>,
    /// Maps a clicked segment to the app's message.
    on_copy: Box<dyn Fn(SegmentId) -> Message>,
    /// Maps a context-menu action pick to the app's message — the
    /// app routes it through the same `perform_action` funnel the
    /// keybind map uses (the menu introduces no behavior).
    on_menu_action: Option<Box<dyn Fn(Action) -> Message>>,
    /// Which vertical side of the band the context menu pops to:
    /// the terminal's side (the band hugs the free edge; the other
    /// side is off the surface). The app knows the edge; the
    /// widget does not.
    menu_side: TooltipSide,
}

impl<Message> StatusBar<Message> {
    /// Creates a band rendering `segments`, flashing `copied` as a
    /// `✓` and reporting presses through `on_copy`.
    pub fn new(
        segments: StatusSegments,
        font: cosmic::iced::Font,
        copied: Option<SegmentId>,
        on_copy: impl Fn(SegmentId) -> Message + 'static,
    ) -> Self {
        Self {
            segments,
            font,
            copied,
            on_copy: Box::new(on_copy),
            on_menu_action: None,
            menu_side: TooltipSide::default(),
        }
    }

    /// Sets the message published when an action row of the
    /// context menu is picked — the app maps it onto the existing
    /// `Action` funnel, the keybinds' own path.
    pub fn on_menu_action(mut self, on_menu_action: impl Fn(Action) -> Message + 'static) -> Self {
        self.on_menu_action = Some(Box::new(on_menu_action));
        self
    }

    /// Sets which vertical side of the band the context menu pops
    /// to: the terminal's side.
    pub fn menu_side(mut self, side: TooltipSide) -> Self {
        self.menu_side = side;
        self
    }

    /// Re-lays the cached paragraphs from the resolved segments, the
    /// flashed segment rendering `✓` in place of its value (the copy
    /// payload never changes — it comes from [`copy_text`], not the
    /// display). The layout is the pinned [`band_fit`] decision over
    /// the row's whole width — [`left_squeeze`] as the path's inner
    /// step.
    fn refresh_paragraphs(&self, state: &mut State, width: f32) {
        let font = self.font;
        let copied = self.copied;
        state.process.update(segment_text(
            display_text(copied, SegmentId::Process, &self.segments.process),
            font,
        ));
        state.branch.update(segment_text(
            display_text(
                copied,
                SegmentId::Branch,
                self.segments.branch.as_deref().unwrap_or(""),
            ),
            font,
        ));
        state.profile.update(segment_text(
            display_text(
                copied,
                SegmentId::Profile,
                self.segments.profile.as_deref().unwrap_or(""),
            ),
            font,
        ));
        let grid = self
            .segments
            .grid
            .map(|(cols, rows)| format!("{cols}×{rows}"));
        state.grid.update(segment_text(
            display_text(copied, SegmentId::Grid, grid.as_deref().unwrap_or("")),
            font,
        ));
        let tab = self
            .segments
            .tab_position
            .map(|(index, total)| format!("{index}/{total}"));
        state.tab.update(segment_text(
            display_text(copied, SegmentId::Tab, tab.as_deref().unwrap_or("")),
            font,
        ));
        let zoom = self
            .segments
            .zoom_percent
            .map(|percent| format!("{percent:+}%"));
        state.zoom.update(segment_text(
            display_text(copied, SegmentId::Zoom, zoom.as_deref().unwrap_or("")),
            font,
        ));
        state.clock.update(segment_text(
            display_text(
                copied,
                SegmentId::Clock,
                self.segments.clock.as_deref().unwrap_or(""),
            ),
            font,
        ));

        // The band-wide fit (status-glance criterion 2): one pure
        // pass over every measured width — the path elides first,
        // then segments drop one at a time in DROP_ORDER, the path
        // re-expanding into each freed width, survivors keeping
        // their seats. The old left-budget math subtracted the
        // right cluster's width unconditionally and overflowed on
        // narrow surfaces; the drop order fits them instead.
        let mut widths = Vec::new();
        for id in std::iter::once(SegmentId::Branch).chain(RIGHT_CLUSTER.iter().copied()) {
            let w = state.paragraph_of(id).min_width();
            if w > 0.0 {
                widths.push((id, w));
            }
        }
        let fixed = if state.process.min_width() > 0.0 {
            PAD_X + DISC_SIZE + DISC_GAP + state.process.min_width() + SEGMENT_GAP + PAD_X
        } else {
            PAD_X + SEGMENT_GAP + PAD_X
        };
        let path = self.segments.path.as_deref().unwrap_or("");
        let mut measure = BandParagraph::default();
        // The steady-state measurement: the flash's `✓` swap is
        // textual and never re-fits the band (the reveal keeps the
        // full value while the display flashes).
        measure.update(segment_text(path, font));
        let fit = band_fit(
            width,
            fixed,
            &widths,
            PathMeasure {
                chars: path.chars().count(),
                full_width: measure.min_width(),
            },
        );
        for id in &fit.dropped {
            state.paragraph_of_mut(*id).update(segment_text("", font));
        }

        // The inline reveal's two displays, cached as text and
        // applied through [`Self::apply_reveal`] — the resting
        // elision the fit just produced, and the armed expansion
        // (the branch's seat lent to the path; the idle fit's
        // drops already freed, the branch filtered besides). A
        // rebuild while hovered re-lays the same armed selection.
        state.path_elided = matches!(fit.path, PathFit::Elided(_));
        state.path_resting_text = match fit.path {
            PathFit::Full => path.to_string(),
            PathFit::Elided(max_chars) => elide_path_left(path, max_chars),
            PathFit::Gone => String::new(),
        };
        let survivors: Vec<(SegmentId, f32)> = widths
            .iter()
            .copied()
            .filter(|(id, _)| !fit.dropped.contains(id))
            .collect();
        state.path_armed_text = reveal_display(
            path,
            width,
            fixed,
            &survivors,
            PathMeasure {
                chars: path.chars().count(),
                full_width: measure.min_width(),
            },
        );
        state.branch_text = self.segments.branch.clone().unwrap_or_default();
        self.apply_reveal(state);

        // The selection's WYSIWYG row: every segment's display text
        // in x-order, exactly as laid — post-drop, post-reveal, the
        // `✓` flash included while it holds. Dropped segments keep
        // their text here; the row builder pairs texts with rects,
        // and a dropped segment has no rect.
        let armed = state.armed;
        let displayed = |id: SegmentId| -> String {
            let value = match id {
                SegmentId::Process => self.segments.process.clone(),
                SegmentId::Path => {
                    if armed {
                        state.path_armed_text.clone()
                    } else {
                        state.path_resting_text.clone()
                    }
                }
                SegmentId::Branch => {
                    if armed {
                        String::new()
                    } else {
                        state.branch_text.clone()
                    }
                }
                SegmentId::Profile => self.segments.profile.clone().unwrap_or_default(),
                SegmentId::Grid => grid.clone().unwrap_or_default(),
                SegmentId::Tab => tab.clone().unwrap_or_default(),
                SegmentId::Zoom => zoom.clone().unwrap_or_default(),
                SegmentId::Clock => self.segments.clock.clone().unwrap_or_default(),
            };
            display_text(copied, id, &value).to_string()
        };
        state.row_texts = LEFT_CLUSTER
            .iter()
            .chain(RIGHT_CLUSTER.iter())
            .map(|id| (*id, displayed(*id)))
            .collect();

        // The menu's rows, synced like the tab menu's — but with
        // static labels, so the cache fills once per open and only
        // re-lays afterwards; enablement never lives here (it
        // re-derives from the live segments at update and draw).
        if state.menu_open {
            let rows = menu_rows(&self.segments, state.menu_segment);
            let labels_match = state
                .menu_paragraphs
                .iter()
                .map(|(label, _)| label.as_str())
                .eq(rows.iter().map(|row| row.label.as_str()));
            if !labels_match {
                state.menu_paragraphs = rows
                    .iter()
                    .map(|row| (row.label.clone(), BandParagraph::default()))
                    .collect();
            }
            for (label, paragraph) in &mut state.menu_paragraphs {
                paragraph.update(segment_text(label, font));
            }
        }
    }

    /// Applies the reveal's armed selection to the laid paragraphs
    /// — the text-swap mechanism the `✓` flash uses, driven by the
    /// arming transition instead of the app: while armed the path
    /// renders its expanded display and the branch lends its seat
    /// (its paragraph blanks, so its rect — and click target —
    /// vanish for the loan); on disarm both restore. The copy flash
    /// wins over either display for its 800 ms.
    fn apply_reveal(&self, state: &mut State) {
        let font = self.font;
        let path_text = if state.armed {
            state.path_armed_text.clone()
        } else {
            state.path_resting_text.clone()
        };
        state.path.update(segment_text(
            display_text(self.copied, SegmentId::Path, &path_text),
            font,
        ));
        let branch_text = if state.armed {
            String::new()
        } else {
            state.branch_text.clone()
        };
        state.branch.update(segment_text(
            display_text(self.copied, SegmentId::Branch, &branch_text),
            font,
        ));
    }
}

/// The laid-out rects of every rendered segment, in cluster order —
/// `draw` paints from them, `update` and `mouse_interaction`
/// hit-test through them. The process rect owns its disc: one state,
/// one click target. An empty paragraph (absent, hidden, or squeezed
/// out) yields no rect.
fn segment_rects(state: &State, bounds: Rectangle) -> Vec<(SegmentId, Rectangle)> {
    let mut rects = Vec::new();
    let row =
        |x: f32, width: f32| Rectangle::new(Point::new(x, bounds.y), Size::new(width, BAND_HEIGHT));
    let mut x = bounds.x + PAD_X;
    if state.process.min_width() > 0.0 {
        let width = DISC_SIZE + DISC_GAP + state.process.min_width();
        rects.push((SegmentId::Process, row(x, width)));
        x += width + SEGMENT_GAP;
    }
    for id in [SegmentId::Path, SegmentId::Branch] {
        let width = state.paragraph_of(id).min_width();
        if width > 0.0 {
            rects.push((id, row(x, width)));
            x += width + SEGMENT_GAP;
        }
    }
    let mut right = bounds.x + bounds.width - PAD_X;
    for id in RIGHT_CLUSTER.iter().rev() {
        let width = state.paragraph_of(*id).min_width();
        if width > 0.0 {
            right -= width;
            rects.push((*id, row(right, width)));
        }
    }
    rects
}

/// The selection's row: every rendered segment's rect paired with
/// its cached display text, in x-order — the WYSIWYG source for
/// both the highlight quads and the copied text.
fn row_segments(state: &State, bounds: Rectangle) -> Vec<RowSegment> {
    let rects = segment_rects(state, bounds);
    LEFT_CLUSTER
        .iter()
        .chain(RIGHT_CLUSTER.iter())
        .filter_map(|id| {
            let rect = rects
                .iter()
                .find(|(rid, _)| rid == id)
                .map(|(_, rect)| *rect)?;
            let text = state
                .row_texts
                .iter()
                .find(|(rid, _)| rid == id)
                .map(|(_, text)| text.clone())
                .unwrap_or_default();
            Some(RowSegment { rect, text })
        })
        .collect()
}

/// The text a segment renders: its value, or — while the copy flash
/// holds — the `✓` in its place (the clipboard payload never
/// changes; it comes from [`copy_text`], not the display).
fn display_text(copied: Option<SegmentId>, id: SegmentId, value: &str) -> &str {
    if copied == Some(id) { "✓" } else { value }
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

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        // A rendered segment is a copy target — the cursor says so;
        // the gaps between them stay idle. Window-absolute, the
        // laid rects' own space (the update paths' rule).
        let Some(position) = cursor
            .position()
            .filter(|point| layout.bounds().contains(*point))
        else {
            return mouse::Interaction::Idle;
        };
        let state = tree.state.downcast_ref::<State>();
        let rects = segment_rects(state, layout.bounds());
        if segment_at(&rects, position).is_some() {
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
        cursor: Cursor,
        _renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        // The inline reveal's arming (status-glance criterion 3, the
        // OQ-1 inline ruling): recomputed against the path's
        // *current* display rect whenever the cursor is placed —
        // the elided strip arms, the expanded strip (a superset)
        // keeps it armed, and absence or an un-elided path disarms.
        // Instant (OQ-3); the swap happens on transitions only.
        let path_rect = cursor.position().map(|_| {
            let state = tree.state.downcast_ref::<State>();
            segment_rects(state, layout.bounds())
                .into_iter()
                .find(|(id, _)| *id == SegmentId::Path)
                .map(|(_, rect)| rect)
        });
        let want = {
            let state = tree.state.downcast_ref::<State>();
            reveal_armed(path_rect.flatten(), cursor.position(), state.path_elided)
        };
        let state = tree.state.downcast_mut::<State>();
        if want != state.armed {
            state.armed = want;
            self.apply_reveal(state);
        }

        // The selection machine (the user-ruled interaction): a
        // press arms, a crossing drag selects — the highlight
        // following the cursor — and the release decides: a click
        // rides the shipped copy verb (the segment's full value
        // plus the ✓ flash), a drag writes the selected display
        // text to the clipboard and the primary selection (the
        // terminal's middle-click paste serves it). The highlight
        // persists after release until the next press.
        match event {
            Event::Mouse(MouseEvent::ButtonPressed(Button::Right)) => {
                // The context menu opens on any right-press inside
                // the band — segment or gap alike, the pressed
                // segment scoping the Copy row (a gap mutes it).
                // The anchor is band-local (the bubble's frame);
                // the segment resolves in the window-absolute space
                // the laid rects live in. A press is a press, so
                // any live selection clears — the next-press rule —
                // and no left drag can be in flight. The open
                // invalidates layout: the bubble sizes from the row
                // cache `refresh_paragraphs` fills only during a
                // root layout and only while open (the tab menu's
                // nudge).
                let Some(local) = cursor.position_in(layout.bounds()) else {
                    return;
                };
                let rects = segment_rects(state, layout.bounds());
                state.menu_segment = cursor
                    .position()
                    .and_then(|point| segment_at(&rects, point));
                state.menu_point = local;
                state.menu_open = true;
                state.press = None;
                state.selection = None;
                shell.invalidate_layout();
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(MouseEvent::ButtonPressed(Button::Left)) => {
                // The anchor is window-absolute — the space
                // CursorMoved and the release already speak — so
                // the 4 px click/drag discrimination is honest on a
                // band laid out mid-column (`position_in`'s local
                // point would skew it by the band's own y).
                let Some(position) = cursor
                    .position()
                    .filter(|point| layout.bounds().contains(*point))
                else {
                    return;
                };
                state.press = Some(position);
                state.selection = None;
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(MouseEvent::CursorMoved { position }) => {
                let state = tree.state.downcast_ref::<State>();
                let Some(anchor) = state.press else {
                    return;
                };
                let crossed = (position.x - anchor.x).abs() >= SELECTION_THRESHOLD
                    || (position.y - anchor.y).abs() >= SELECTION_THRESHOLD;
                if !crossed {
                    return;
                }
                let state = tree.state.downcast_mut::<State>();
                state.selection = Some((anchor, *position));
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(MouseEvent::ButtonReleased(Button::Left)) => {
                let state = tree.state.downcast_mut::<State>();
                let Some(anchor) = state.press.take() else {
                    return;
                };
                shell.capture_event();
                shell.request_redraw();
                if selection_is_click(anchor, cursor.position().unwrap_or(anchor)) {
                    // The click verb: the segment's full value
                    // through `copy_text` — never the elided
                    // display.
                    let rects = segment_rects(state, layout.bounds());
                    if let Some(id) = cursor
                        .position()
                        .and_then(|point| segment_at(&rects, point))
                    {
                        clipboard.write(Kind::Standard, copy_text(id, &self.segments));
                        shell.publish((self.on_copy)(id));
                    }
                } else if let Some((from, to)) = state.selection {
                    let text = selection_text(from, to, &row_segments(state, layout.bounds()));
                    if !text.is_empty() {
                        clipboard.write(Kind::Standard, text.clone());
                        clipboard.write(Kind::Primary, text);
                    }
                }
            }
            _ => {}
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
        // The context menu, when open: the per-tab menu's pattern.
        // The bubble hangs over the terminal (and, popping to the
        // terminal's side, never over the grab bar), so it must be
        // an overlay at all — only the overlay pass sees presses
        // there, ahead of the tree, and a captured overlay press
        // reaches nothing else.
        if !tree.state.downcast_ref::<State>().menu_open {
            return None;
        }
        let bounds = layout.bounds();
        let absolute = Rectangle::new(bounds.position() + translation, bounds.size());
        Some(overlay::Element::new(Box::new(StatusMenuOverlay {
            state: tree.state.downcast_mut::<State>(),
            segments: &self.segments,
            on_copy: &self.on_copy,
            on_menu_action: &self.on_menu_action,
            side: self.menu_side,
            band: absolute,
        })))
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

        // The selection highlight, under the text and above the
        // strip: the pane foreground at a quarter strength —
        // hierarchy by alpha, never a new hue. Painted while a
        // selection lives, drag or persisting.
        if let Some((from, to)) = state.selection {
            for rect in selection_rects(from, to, &row_segments(state, bounds)) {
                renderer.fill_quad(
                    Quad {
                        bounds: rect,
                        border: Border {
                            radius: 0.0.into(),
                            width: 0.0,
                            color: Color::TRANSPARENT,
                        },
                        snap: true,
                        ..Default::default()
                    },
                    Color {
                        a: SELECTION_ALPHA,
                        ..pane_fg
                    },
                );
            }
        }

        // The process-state disc: filled while a foreground child
        // runs, a hollow ring over the idle shell — the gear's own
        // disc primitive. It renders with the process segment and
        // belongs to its click target.
        let center_y = bounds.y + BAND_HEIGHT / 2.0;
        let disc = Rectangle::new(
            Point::new(bounds.x + PAD_X, center_y - DISC_SIZE / 2.0),
            Size::new(DISC_SIZE, DISC_SIZE),
        );
        if state.process.min_width() > 0.0 {
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
        }

        // Both clusters paint from the same rects hit-testing walks:
        // full alpha on the left, dimmed on the right — hierarchy by
        // alpha and mass, never a new hue.
        let text_y =
            |paragraph: &BandParagraph| bounds.y + (BAND_HEIGHT - paragraph.min_height()) / 2.0;
        let dimmed = Color {
            a: DIM_ALPHA,
            ..pane_fg
        };
        for (id, rect) in segment_rects(state, bounds) {
            let paragraph = state.paragraph_of(id);
            let color = if LEFT_CLUSTER.contains(&id) {
                pane_fg
            } else {
                dimmed
            };
            let text_x = if id == SegmentId::Process {
                rect.x + DISC_SIZE + DISC_GAP
            } else {
                rect.x
            };
            renderer.fill_paragraph(
                paragraph.raw(),
                Point::new(text_x, text_y(paragraph)),
                color,
                Rectangle::new(
                    Point::new(text_x, bounds.y),
                    Size::new(paragraph.min_width().max(0.0), BAND_HEIGHT),
                ),
            );
        }
    }
}

/// The band's context menu, hand-drawn on the overlay pass (the
/// per-tab menu's pattern): a pane-inverted bubble of
/// [`menu_rows`] rows anchored at the opening right-press, popping
/// to the terminal's side of the band. While open it owns every
/// press, the dropdown's contract: any press closes it, a left
/// press on an enabled row first resolves its pick — the copy rows
/// write the clipboard here, where it already lives for the click
/// verb, and report through the band's copy callback for the ✓
/// flash; the action rows publish through `on_menu_action` — and
/// every press is captured so nothing under the menu acts on it.
struct StatusMenuOverlay<'a, Message> {
    /// The band's state: the open flag (cleared on any press), the
    /// pressed segment, the anchor, and the cached row label
    /// paragraphs.
    state: &'a mut State,
    /// The live segments — the rows' enablement and the copy
    /// payloads resolve against them at pick time.
    segments: &'a StatusSegments,
    /// The copy report — the click verb's ✓ flash path.
    on_copy: &'a dyn Fn(SegmentId) -> Message,
    /// The action rows' publisher — the app's `Action` funnel.
    on_menu_action: &'a Option<Box<dyn Fn(Action) -> Message>>,
    /// Which vertical side of the band the bubble pops to.
    side: TooltipSide,
    /// The band's absolute bounds (translation applied).
    band: Rectangle,
}

impl<Message> StatusMenuOverlay<'_, Message> {
    /// The live rows for the open menu's pressed segment.
    fn rows(&self) -> Vec<MenuRow> {
        menu_rows(self.segments, self.state.menu_segment)
    }
}

impl<Message> overlay::Overlay<Message, Theme, Renderer> for StatusMenuOverlay<'_, Message>
where
    Message: Clone,
{
    fn layout(&mut self, _renderer: &Renderer, _bounds: Size) -> layout::Node {
        let rows = self.rows();
        let widest = self
            .state
            .menu_paragraphs
            .iter()
            .map(|(_, paragraph)| paragraph.min_width())
            .fold(0.0, f32::max);
        let band_local = menu_bounds(
            self.band.width,
            self.state.menu_point.x,
            widest,
            rows.len(),
            self.side,
        );
        let position = self.band.position() + Vector::new(band_local.x, band_local.y);
        layout::Node::new(band_local.size()).move_to(position)
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: Cursor,
        _renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        // The dropdown's dismissal contract, unchanged: every press
        // closes, a left press on an enabled row first resolves its
        // pick against the live segments, and every press is
        // captured so nothing under the menu — the band, the
        // terminal below — acts on it. Motion and keyboard pass
        // through; a motion event asks for the frame the hover tint
        // needs (the runtime draws only on request).
        if matches!(event, Event::Mouse(MouseEvent::CursorMoved { .. })) {
            shell.request_redraw();
            return;
        }
        let Event::Mouse(MouseEvent::ButtonPressed(button)) = event else {
            return;
        };
        let rows = self.rows();
        let pick = cursor
            .position()
            .and_then(|point| dropdown_row_at(layout.bounds(), point))
            .and_then(|index| rows.get(index))
            .filter(|row| row.enabled)
            .and_then(|row| row.pick);
        if *button == Button::Left
            && let Some(pick) = pick
        {
            match pick {
                MenuPick::CopySegment(id) => {
                    // The click verb's twin: the full value through
                    // `copy_text` (never the display), the same ✓
                    // flash through the band's copy report. An
                    // emptied live value writes nothing.
                    let text = copy_text(id, self.segments);
                    if !text.is_empty() {
                        clipboard.write(Kind::Standard, text);
                        shell.publish((self.on_copy)(id));
                    }
                }
                MenuPick::CopyAll => {
                    // Every present segment's full value — no
                    // flash, no single segment owns the copy (the
                    // menu's own dismissal is the acknowledgment).
                    let text = copy_all_text(self.segments);
                    if !text.is_empty() {
                        clipboard.write(Kind::Standard, text);
                    }
                }
                MenuPick::Action(action) => {
                    if let Some(message) = self
                        .on_menu_action
                        .as_ref()
                        .map(|on_menu_action| on_menu_action(action))
                    {
                        shell.publish(message);
                    }
                }
            }
        }
        self.state.menu_open = false;
        shell.capture_event();
        shell.request_redraw();
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: Cursor,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        // Pointer over the bubble only: elsewhere the interaction
        // falls back to whatever the band itself reports (the
        // dropdown's rule — muted rows keep the pointer shape,
        // honest at a glance).
        let over_menu = cursor
            .position()
            .is_some_and(|point| layout.bounds().contains(point));
        if over_menu {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }

    // The pane_bg/pane_fg names match the band draw's vocabulary;
    // `similar_names` resolves its level at the fn, so the expect
    // rides here rather than on the binding.
    #[expect(
        clippy::similar_names,
        reason = "the pane_bg/pane_fg pair matches the band draw's vocabulary"
    )]
    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor_position: Cursor,
    ) {
        let menu = layout.bounds();
        let (pane_bg, pane_fg) = pane_colors();
        // The pane pair swapped, the dropdown's rule: a
        // foreground-colored bubble carrying the band's color as
        // its text stays readable on light and dark panes alike.
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

        let rows = self.rows();
        let hovered = cursor_position
            .position()
            .and_then(|point| dropdown_row_at(menu, point));
        for (index, (_, paragraph)) in self.state.menu_paragraphs.iter().enumerate() {
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
            // Rows highlight on hover, clickable ones only — a
            // muted row is not a target (the per-tab menu's rule).
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
            // Muted rows — a gap press's Copy, base-size Reset —
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

impl<Message> From<StatusBar<Message>> for Element<'_, Message>
where
    Message: Clone + 'static,
{
    fn from(status_bar: StatusBar<Message>) -> Self {
        Self::new(status_bar)
    }
}

#[cfg(test)]
mod tests;
