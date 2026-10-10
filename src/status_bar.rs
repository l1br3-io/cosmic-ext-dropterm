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
use cosmic::iced::core::text::{LineHeight, Shaping, paragraph};
use cosmic::iced::core::text::{Text, Wrapping};
use cosmic::iced::core::{
    Border, Layout, Rectangle, Size, layout,
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
/// process-name and branch snapshots, the grid and tab position, and
/// the stored clock string.
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
        grid: toggles.grid.then_some(inputs.grid).flatten(),
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
/// cost model).
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
        }
    }

    /// Re-lays the cached paragraphs from the resolved segments, the
    /// flashed segment rendering `✓` in place of its value (the copy
    /// payload never changes — it comes from [`copy_text`], not the
    /// display). The path elides from its left against the width the
    /// measured right cluster leaves, and the branch yields only
    /// after full elision — the pinned [`left_squeeze`] decision.
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

        // The path's budget: the row width minus both side paddings,
        // the disc cluster, the process name, and the measured right
        // cluster with its gaps — a view heuristic the hardware pass
        // eyeballs at narrow widths.
        let mut right_width = 0.0;
        for id in RIGHT_CLUSTER {
            let paragraph = state.paragraph_of(id);
            if paragraph.min_width() > 0.0 {
                right_width += SEGMENT_GAP + paragraph.min_width();
            }
        }
        let left_fixed = if state.process.min_width() > 0.0 {
            PAD_X + DISC_SIZE + DISC_GAP + state.process.min_width() + SEGMENT_GAP
        } else {
            PAD_X + SEGMENT_GAP
        };
        let budget = (width - left_fixed - right_width - PAD_X).max(0.0);
        let path = self.segments.path.as_deref().unwrap_or("");
        let mut measure = BandParagraph::default();
        measure.update(segment_text(
            display_text(copied, SegmentId::Path, path),
            font,
        ));
        let (fit, branch_shown) = left_squeeze(
            budget,
            path.chars().count(),
            measure.min_width(),
            state.branch.min_width(),
        );
        let elided = match fit {
            PathFit::Full => display_text(copied, SegmentId::Path, path).to_string(),
            PathFit::Elided(max_chars) => elide_path_left(path, max_chars),
            PathFit::Gone => String::new(),
        };
        state.path.update(segment_text(&elided, font));
        if !branch_shown {
            state.branch.update(segment_text("", font));
        }
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
        // the gaps between them stay idle.
        let Some(position) = cursor.position_in(layout.bounds()) else {
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
        let Event::Mouse(MouseEvent::ButtonPressed(Button::Left)) = event else {
            return;
        };
        let Some(position) = cursor.position_in(layout.bounds()) else {
            return;
        };
        let state = tree.state.downcast_ref::<State>();
        let rects = segment_rects(state, layout.bounds());
        if let Some(id) = segment_at(&rects, position) {
            // The sole clipboard payload source: the segment's full
            // value through `copy_text` — never the elided display.
            clipboard.write(Kind::Standard, copy_text(id, &self.segments));
            shell.publish((self.on_copy)(id));
            shell.capture_event();
        }
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

    /// The v1 positional shape over the grown resolver — the grown
    /// inputs stay absent, exactly how they arrive until the app
    /// wires them.
    fn resolve(
        home: Option<&Path>,
        cwd: Option<&Path>,
        shell: &str,
        profile_name: Option<&str>,
        zoom_adj: i8,
        zoom_step_mul_100: u16,
        foreground: Option<&str>,
    ) -> StatusSegments {
        let inputs = StatusInputs {
            home,
            cwd,
            shell,
            profile_name,
            zoom_adj,
            zoom_step_mul_100,
            foreground,
            branch: None,
            grid: None,
            tab_position: None,
            clock: None,
        };
        resolve_segments(&inputs, StatusToggles::all())
    }

    // --- segment assembly (running/idle × profile × zoom × cwd) ---

    #[test]
    fn a_running_child_names_the_process_and_fills_the_disc() {
        let segments = resolve(
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
        let segments = resolve(
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
        let segments = resolve(
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
        let segments = resolve(
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
        let segments = resolve(Some(&home()), None, "/usr/bin/zsh", None, 0, 100, None);
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

    // --- the left cluster's squeeze (path elides before the branch
    // yields) ---

    #[test]
    fn a_roomy_cluster_shows_the_path_and_branch_whole() {
        // 120 px of path + 12 px gap + 30 px of branch under 200 px.
        assert_eq!(left_squeeze(200.0, 12, 120.0, 30.0), (PathFit::Full, true));
    }

    #[test]
    fn the_path_elides_before_the_branch_yields() {
        // 100 px budget: the branch keeps its 42 px (gap included),
        // the path elides into the remaining 58 px (5 chars at
        // 10 px each).
        assert_eq!(
            left_squeeze(100.0, 12, 120.0, 30.0),
            (PathFit::Elided(5), true)
        );
    }

    #[test]
    fn the_branch_yields_only_after_full_elision_of_the_path() {
        // 40 px cannot host the branch beside even an empty path —
        // the branch drops and the path elides into the whole 40 px.
        assert_eq!(
            left_squeeze(40.0, 12, 120.0, 30.0),
            (PathFit::Elided(4), false)
        );
    }

    #[test]
    fn a_starved_cluster_loses_the_path_entirely() {
        assert_eq!(left_squeeze(5.0, 12, 120.0, 30.0), (PathFit::Gone, false));
    }

    #[test]
    fn an_absent_path_never_elides() {
        // No path characters: nothing to squeeze — the branch either
        // fits or it does not.
        assert_eq!(left_squeeze(50.0, 0, 0.0, 30.0), (PathFit::Full, true));
        assert_eq!(left_squeeze(20.0, 0, 0.0, 30.0), (PathFit::Full, false));
        assert_eq!(left_squeeze(20.0, 0, 0.0, 0.0), (PathFit::Full, false));
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

    // --- the grown contract (status-segments) ---

    fn resolve_full(toggles: StatusToggles) -> StatusSegments {
        let home = home();
        let cwd = std::path::PathBuf::from("/home/jpt/src/oss/l1br3/ced2");
        let inputs = StatusInputs {
            home: Some(&home),
            cwd: Some(&cwd),
            shell: "/usr/bin/fish",
            profile_name: Some("dev"),
            zoom_adj: 2,
            zoom_step_mul_100: 100,
            foreground: Some("cargo"),
            branch: Some("ced2"),
            grid: Some((80, 24)),
            tab_position: Some((2, 5)),
            clock: Some("14:32"),
        };
        resolve_segments(&inputs, toggles)
    }

    fn segment_present(segments: &StatusSegments, id: SegmentId) -> bool {
        match id {
            SegmentId::Process => !segments.process.is_empty(),
            SegmentId::Path => segments.path.is_some(),
            SegmentId::Branch => segments.branch.is_some(),
            SegmentId::Profile => segments.profile.is_some(),
            SegmentId::Grid => segments.grid.is_some(),
            SegmentId::Tab => segments.tab_position.is_some(),
            SegmentId::Zoom => segments.zoom_percent.is_some(),
            SegmentId::Clock => segments.clock.is_some(),
        }
    }

    fn hide(toggles: &mut StatusToggles, id: SegmentId) {
        let field = match id {
            SegmentId::Process => &mut toggles.process,
            SegmentId::Path => &mut toggles.path,
            SegmentId::Branch => &mut toggles.branch,
            SegmentId::Profile => &mut toggles.profile,
            SegmentId::Grid => &mut toggles.grid,
            SegmentId::Tab => &mut toggles.tab,
            SegmentId::Zoom => &mut toggles.zoom,
            SegmentId::Clock => &mut toggles.clock,
        };
        *field = false;
    }

    #[test]
    fn the_clusters_pin_the_eight_segments_order() {
        assert_eq!(
            LEFT_CLUSTER,
            [SegmentId::Process, SegmentId::Path, SegmentId::Branch]
        );
        assert_eq!(
            RIGHT_CLUSTER,
            [
                SegmentId::Profile,
                SegmentId::Grid,
                SegmentId::Tab,
                SegmentId::Zoom,
                SegmentId::Clock
            ]
        );
    }

    #[test]
    fn each_toggle_hides_exactly_its_segment() {
        let every: Vec<SegmentId> = LEFT_CLUSTER
            .iter()
            .chain(RIGHT_CLUSTER.iter())
            .copied()
            .collect();
        for hidden in &every {
            let mut toggles = StatusToggles::all();
            hide(&mut toggles, *hidden);
            let segments = resolve_full(toggles);
            assert!(
                !segment_present(&segments, *hidden),
                "{hidden:?} must hide behind its toggle"
            );
            for shown in every.iter().filter(|id| **id != *hidden) {
                assert!(
                    segment_present(&segments, *shown),
                    "{shown:?} must survive {hidden:?}'s toggle"
                );
            }
        }
    }

    #[test]
    fn toggles_compose() {
        let mut toggles = StatusToggles::all();
        hide(&mut toggles, SegmentId::Branch);
        hide(&mut toggles, SegmentId::Clock);
        let segments = resolve_full(toggles);
        assert_eq!(segments.branch, None);
        assert_eq!(segments.clock, None);
        assert!(segment_present(&segments, SegmentId::Path));
        assert!(segment_present(&segments, SegmentId::Grid));
    }

    #[test]
    fn the_new_segments_resolve_from_their_inputs() {
        let segments = resolve_full(StatusToggles::all());
        assert_eq!(segments.branch.as_deref(), Some("ced2"));
        assert_eq!(segments.grid, Some((80, 24)));
        assert_eq!(segments.tab_position, Some((2, 5)));
        assert_eq!(segments.clock.as_deref(), Some("14:32"));
    }

    #[test]
    fn copy_text_carries_each_segments_full_value() {
        // Copy pays no attention to display: the payload is the
        // segment's whole value — the path whole even where the band
        // would elide it (hit-testing feeds ids; elision never
        // reaches the payload).
        let segments = resolve_full(StatusToggles::all());
        assert_eq!(copy_text(SegmentId::Process, &segments), "cargo");
        assert_eq!(
            copy_text(SegmentId::Path, &segments),
            "~/src/oss/l1br3/ced2"
        );
        assert_eq!(copy_text(SegmentId::Branch, &segments), "ced2");
        assert_eq!(copy_text(SegmentId::Profile, &segments), "dev");
        assert_eq!(copy_text(SegmentId::Grid, &segments), "80×24");
        assert_eq!(copy_text(SegmentId::Tab, &segments), "2/5");
        assert_eq!(copy_text(SegmentId::Zoom, &segments), "+200%");
        assert_eq!(copy_text(SegmentId::Clock, &segments), "14:32");
    }

    #[test]
    fn copy_text_of_an_absent_segment_is_empty() {
        let empty = StatusSegments::default();
        for id in LEFT_CLUSTER.iter().chain(RIGHT_CLUSTER.iter()) {
            assert_eq!(copy_text(*id, &empty), String::new());
        }
    }

    #[test]
    fn segment_at_hits_only_the_rect_containing_the_point() {
        let rects = [
            (
                SegmentId::Process,
                Rectangle::new(Point::new(0.0, 0.0), Size::new(60.0, BAND_HEIGHT)),
            ),
            (
                SegmentId::Path,
                Rectangle::new(Point::new(72.0, 0.0), Size::new(90.0, BAND_HEIGHT)),
            ),
        ];
        assert_eq!(
            segment_at(&rects, Point::new(10.0, 14.0)),
            Some(SegmentId::Process)
        );
        assert_eq!(
            segment_at(&rects, Point::new(100.0, 27.9)),
            Some(SegmentId::Path)
        );
        // The gap between segments — and everything outside the row
        // (the bottom edge included; `contains` is half-open) —
        // belongs to no segment.
        assert_eq!(segment_at(&rects, Point::new(65.0, 14.0)), None);
        assert_eq!(segment_at(&rects, Point::new(-1.0, 14.0)), None);
        assert_eq!(segment_at(&rects, Point::new(10.0, BAND_HEIGHT)), None);
    }

    // --- the clock helpers ---

    fn zoned(at: &str) -> jiff::Zoned {
        at.parse().expect("fixed zoned fixture")
    }

    #[test]
    fn clock_text_formats_twenty_four_hour_minutes() {
        assert_eq!(
            clock_text(&zoned("2026-10-10T14:32:00+00:00[UTC]")),
            "14:32"
        );
        assert_eq!(
            clock_text(&zoned("2026-10-10T09:05:59+00:00[UTC]")),
            "09:05"
        );
    }

    #[test]
    fn next_tick_delay_lands_on_the_minute_boundary() {
        assert_eq!(
            next_tick_delay(&zoned("2026-10-10T14:32:00+00:00[UTC]")),
            std::time::Duration::from_mins(1)
        );
        assert_eq!(
            next_tick_delay(&zoned("2026-10-10T14:32:30.500+00:00[UTC]")),
            std::time::Duration::from_millis(29_500)
        );
    }

    #[test]
    fn the_clock_ticks_only_while_both_gates_hold() {
        assert!(clock_due(true, true));
        assert!(!clock_due(true, false));
        assert!(!clock_due(false, true));
        assert!(!clock_due(false, false));
    }

    // --- the git branch (real fixture trees under temp) ---

    fn temp_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "dropterm-status-git-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp repo root");
        root
    }

    fn write_fixture(path: &std::path::Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("fixture parent");
        }
        std::fs::write(path, content).expect("fixture write");
    }

    #[test]
    fn a_ref_head_names_the_branch() {
        let root = temp_root("ref");
        write_fixture(&root.join(".git/HEAD"), "ref: refs/heads/main\n");
        assert_eq!(git_branch(Some(&root)), Some("main".to_string()));
    }

    #[test]
    fn a_detached_head_shortens_to_seven_hex_characters() {
        let root = temp_root("detached");
        write_fixture(
            &root.join(".git/HEAD"),
            "0123456789abcdef0123456789abcdef01234567\n",
        );
        assert_eq!(git_branch(Some(&root)), Some("0123456".to_string()));
    }

    #[test]
    fn a_gitdir_file_indirection_resolves_the_worktree_head() {
        let root = temp_root("indirect");
        write_fixture(
            &root.join("wt/.git"),
            &format!("gitdir: {}\n", root.join("real.git").display()),
        );
        write_fixture(&root.join("real.git/HEAD"), "ref: refs/heads/ced2\n");
        assert_eq!(git_branch(Some(&root.join("wt"))), Some("ced2".to_string()));
    }

    #[test]
    fn a_relative_gitdir_resolves_against_the_cwd() {
        // Submodule-style gitdir files are relative to the working
        // copy's directory, not the process cwd.
        let root = temp_root("relative");
        write_fixture(&root.join("work/.git"), "gitdir: ../real.git\n");
        write_fixture(&root.join("real.git/HEAD"), "ref: refs/heads/main\n");
        assert_eq!(
            git_branch(Some(&root.join("work"))),
            Some("main".to_string())
        );
    }

    #[test]
    fn outside_a_repo_or_without_a_cwd_there_is_no_branch() {
        let root = temp_root("bare");
        assert_eq!(git_branch(Some(&root)), None);
        assert_eq!(git_branch(None), None);
    }

    #[test]
    fn malformed_heads_resolve_to_none() {
        let root = temp_root("malformed");
        for (label, head) in [
            ("empty", ""),
            ("ref-without-a-name", "ref: refs/heads/\n"),
            ("short-garbage", "abc\n"),
            ("non-hex-garbage", "not-a-sha!!\n"),
        ] {
            write_fixture(&root.join(".git/HEAD"), head);
            assert_eq!(git_branch(Some(&root)), None, "{label} must not parse");
        }
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
            branch: None,
            grid: None,
            tab_position: None,
            clock: None,
        };
        let band: StatusBar<()> = StatusBar::new(
            segments.clone(),
            cosmic::iced::Font::default(),
            None,
            |_| (),
        );
        assert_eq!(band.segments, segments);
    }
}
