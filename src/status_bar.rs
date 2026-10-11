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
mod tests;
