// SPDX-License-Identifier: GPL-3.0-only

//! The status band's framework-free model (gpui-status spec): the
//! segment vocabulary and its resolution rules, the display and
//! copy texts, the fit math (elision and drop precedence), the
//! surface column's stacking order, and the jiff clock arithmetic —
//! moved unchanged from the libcosmic frontend's `status_bar` when
//! the GPUI band needed the same one implementation (the mouse
//! encodings' move precedent). Both frontends' bands render from
//! this module; the iced-typed halves — hit-testing, selection, the
//! context menu, the widget itself — stayed with that frontend.
//! The engine's OSC-fed state ([`ReportedCwd`], the command timer's
//! verdicts, [`ReportedProgress`], [`ProgramStatus`]) is consumed
//! as plain events, never parsed here: OSC stays engine-side.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::core::config::{Edge, StatusToggles, TabPosition};
use crate::core::slots::SlotContent;

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

/// The surface column's stacking order: `Auto` keeps the historic
/// layout — the tab strip beside the status band, the band taking the
/// slot **adjacent to the grab bar** (the free-edge-most band, status
/// bar criterion 1) — while a pin claims the extreme slot on its named
/// side; when the pin claims the grab side the band yields one slot
/// rather than covering it (the pin ruling, owner 2026-11-21: every
/// dropdown entry reads differently on both edges).
pub const fn column_stacking(edge: Edge, tab_position: TabPosition) -> [Strip; 4] {
    match (edge, tab_position) {
        (Edge::Top, TabPosition::Top) => [
            Strip::Tabs,
            Strip::Terminal,
            Strip::StatusBand,
            Strip::GrabBar,
        ],
        (Edge::Top, TabPosition::Auto) => [
            Strip::Terminal,
            Strip::Tabs,
            Strip::StatusBand,
            Strip::GrabBar,
        ],
        (Edge::Top, TabPosition::Bottom) => [
            Strip::Terminal,
            Strip::StatusBand,
            Strip::Tabs,
            Strip::GrabBar,
        ],
        (Edge::Bottom, TabPosition::Top) => [
            Strip::GrabBar,
            Strip::Tabs,
            Strip::StatusBand,
            Strip::Terminal,
        ],
        (Edge::Bottom, TabPosition::Auto) => [
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

/// What the band copied, payload and all — the report the widget
/// publishes instead of writing a widget-layer clipboard
/// (clipboard-ownership spec): the app performs the write through
/// the focus-free path, so a band copy lands however unfocused the
/// terminal is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Copied {
    /// A segment's full value (never the elided display); the app
    /// also flashes the segment's `✓`.
    Segment(SegmentId, String),
    /// The drag-selected display text; the app writes the standard
    /// clipboard and the primary selection (middle-click serves it).
    Selection(String),
    /// Every present segment's full value (the menu's Copy All);
    /// standard clipboard only — no single segment owns the copy.
    All(String),
}

/// Which band region a segment owns — the shared id for cluster
/// order, visibility, hit-testing, and copy payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentId {
    /// The state disc + the process name (one click target).
    Process,
    /// The command timer's reading (command-lifecycle spec): live
    /// elapsed while a witnessed start runs, the frozen
    /// (name, duration) pair after it finishes.
    Duration,
    /// The last finished command's non-zero exit code — `✗1` in the
    /// attention red, beside its duration. Success renders nothing:
    /// code 0 is the unmarked default, and an unwitnessed verdict
    /// (no shell integration) renders nothing either — never a guess.
    Exit,
    /// The running command's own progress report (OSC 9;4) — `42%`,
    /// `…` when indeterminate, the value in attention red when the
    /// reporter says error. State 0, the foreground handoff, and ~15 s
    /// without a refresh each clear it: a dead reporter never leaves
    /// its last number lying.
    Progress,
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
    /// The viewport's lines scrolled back from the bottom
    /// (status-scrolled spec) — the off-bottom chip, `↑12`.
    Scrolled,
    /// The active tab's pasted-image count (paste-chip spec) —
    /// `N images`, absent at zero (the exit chip's rule: no pastes
    /// is the unmarked default). The click opens the images panel,
    /// never a copy.
    Pasted,
    /// The pre-formatted wall clock.
    Clock,
}

/// The left cluster's segments in draw order — full alpha, except
/// the duration's dimmed meta narration (OQ-2: the seat is the
/// process's side; the alpha stays a notch down). The exit chip sits
/// after the duration it verdicts (status-exit spec); the progress
/// chip after the exit (osc94-progress spec).
pub const LEFT_CLUSTER: [SegmentId; 6] = [
    SegmentId::Process,
    SegmentId::Duration,
    SegmentId::Exit,
    SegmentId::Progress,
    SegmentId::Path,
    SegmentId::Branch,
];

/// The right cluster's segments in draw order — the dimmed alpha,
/// the clock rightmost (the tmux seat). The scrolled chip sits
/// between Zoom and Clock (status-scrolled OQ-2): state of the
/// view, beside time. The pasted-count chip sits between Scrolled
/// and Clock (paste-chip spec OQ-2): state of the session beside
/// state of the view.
pub const RIGHT_CLUSTER: [SegmentId; 7] = [
    SegmentId::Profile,
    SegmentId::Grid,
    SegmentId::Tab,
    SegmentId::Zoom,
    SegmentId::Scrolled,
    SegmentId::Pasted,
    SegmentId::Clock,
];
/// Drop precedence under width pressure — first-dropped first.
/// Process and path are absent by construction: the disc never
/// yields, and the path elides before anything else gives way.
/// The pasted-count chip yields before everything (paste-chip
/// spec): the band's most redundant fact — the path already sits
/// in the grid, the files on disk. Facts the shell itself knows
/// (`$COLUMNS×$LINES`) follow,
/// then slow-moving identity (tab position, zoom, profile), the
/// branch stays beside its path, the duration follows its command,
/// the progress chip drops before the exit chip (the most
/// accessory fact — osc94-progress spec), the exit chip follows
/// the duration (the verdict rides its command's narration, never
/// outliving it), the scrolled chip follows both (it answers
/// "what's happening now" — it must survive while the command's
/// own narration stands), and the clock is last — tmux keeps time
/// to the bitter end.
pub const DROP_ORDER: [SegmentId; 11] = [
    SegmentId::Pasted,
    SegmentId::Grid,
    SegmentId::Tab,
    SegmentId::Zoom,
    SegmentId::Profile,
    SegmentId::Branch,
    SegmentId::Duration,
    SegmentId::Progress,
    SegmentId::Exit,
    SegmentId::Scrolled,
    SegmentId::Clock,
];
/// The side a script-provided segment chose for its seat
/// (status-segments spec) — which trailing group renders it, and the
/// group's cluster idiom: full alpha on the left, dimmed on the
/// right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptSide {
    /// After the builtin left cluster.
    Left,
    /// Before the builtin right cluster.
    Right,
}

/// One script segment's resolved slot (status-segments spec): the
/// registered id, its chosen side, and its resolved text — `None`
/// (never answered, or answered nil) renders nothing. Shell-fed,
/// never resolved: the tick's resolve starts it empty and the drain
/// applies the host's answers, so a slot keeps its prior text until
/// a fresh answer lands (the stale-degrade story).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptSegmentValue {
    /// The `dropterm.status_segment` registration name.
    pub id: String,
    /// The side the last shown answer chose.
    pub side: ScriptSide,
    /// The last shown typed answer; `None` renders the segment
    /// absent (styled-status-slot: the content rides whole, the
    /// painter decides its runs).
    pub content: Option<SlotContent>,
}

/// One script segment's answer on a query trip (status-segments
/// spec): the shown value (the chosen side and the typed answer — a
/// bare string for the v1 shape, styled spans the additive upgrade,
/// slot-contract spec) or a deliberate hide (nil). A raising or
/// timing-out handler answers not at all — its id is omitted from the
/// reply so the asking surface keeps the prior value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentAnswer {
    /// The handler answered: render this content on this side.
    Shown {
        id: String,
        side: ScriptSide,
        content: SlotContent,
    },
    /// The handler answered nil: render nothing (a fresh decision —
    /// distinct from an omission, which keeps the prior value).
    Hidden { id: String },
}

/// The interval fallback (status-segments spec): segments that poll
/// their own state (io.popen, files) refresh even with no input diff
/// — one interpreter trip per elapse, the soft bound covering the
/// trip whole. A constant, not a config key (assumption 4's ruling,
/// the lua-host timeout precedent).
pub const SCRIPT_QUERY_INTERVAL: Duration = Duration::from_secs(1);

/// The shell's live script-segment state (status-segments spec, task
/// SG2): the registry (the host's ordered sync reports), each id's
/// last resolved slot, the outbound token, and the requery decision
/// — pure state the shell owns, so the trigger rules are
/// headless-testable beside the merge they feed. The stale-degrade
/// story lives here: a slot keeps its prior text until a fresh
/// answer lands, a raise omits its id (nothing applied), a nil
/// clears the text, and a stale or superseded reply drops whole.
#[derive(Debug, Default)]
pub struct ScriptSegmentsState {
    /// The live registry in registration order — the render order and
    /// the requery gate (empty = no ctx build, no dispatch, today's
    /// band).
    ids: Vec<String>,
    /// Each registered id's last resolved slot — the typed answer
    /// whole (slot-contract spec); the render surface flattens.
    slots: std::collections::HashMap<String, (ScriptSide, Option<SlotContent>)>,
    /// The ctx the last dispatch carried — the diff trigger's
    /// comparison. `None` before the first dispatch and after an
    /// emptied registry: the next query fires on the diff alone.
    last_ctx: Option<serde_json::Value>,
    /// The last dispatch's instant — the interval trigger's clock.
    queried_at: Option<Instant>,
    /// The outbound match key; replies carry it back, bumped per
    /// dispatch so a superseded reply cannot apply.
    token: u64,
    /// A drained dirty request (the reserved emit's translation) —
    /// spent by the next requery decision.
    dirty: bool,
    /// Whether the boot query is still armed — the shell arms it at
    /// construction exactly when the plugins dir stood (the watcher
    /// only wakes on dir *events*, so without this one query a
    /// standing dir's segments would never render).
    boot_armed: bool,
    /// The config's ordered-id key: `None` renders the registry's
    /// own order; `Some` renders exactly the listed ids, in list
    /// order — the render-side filter (the registry itself stays
    /// truthful to what scripts registered).
    order: Option<Vec<String>>,
}

impl ScriptSegmentsState {
    /// The live registry, in registration order.
    pub fn registry(&self) -> &[String] {
        &self.ids
    }

    /// The ctx the last dispatch carried — `None` when none ever did
    /// (the no-bookkeeping pin's observation point).
    pub fn last_query_ctx(&self) -> Option<&serde_json::Value> {
        self.last_ctx.as_ref()
    }

    /// Apply a registry-sync report: the ordered live id set. Slots
    /// for vanished ids sweep; survivors keep their text across the
    /// reload gap (spec assumption 6). An emptied registry resets
    /// whole — nothing renders, bookkeeping cleared — so the next
    /// sync starts fresh.
    pub fn note_sync(&mut self, ids: Vec<String>) {
        if ids == self.ids {
            return; // an idempotent re-report changes nothing
        }
        if ids.is_empty() {
            *self = Self::default();
            return;
        }
        self.slots.retain(|id, _| ids.contains(id));
        self.ids = ids;
    }

    /// Apply a drained dirty request: the next requery decision fires
    /// regardless of diff or interval.
    pub fn note_dirty(&mut self) {
        self.dirty = true;
    }

    /// Arm the boot query — the shell does this at construction
    /// exactly when the plugins dir stood at launch.
    pub fn arm_boot_query(&mut self) {
        self.boot_armed = true;
    }

    /// The boot query's token: the one dispatch the shell sends when
    /// the plugins dir stood at launch but the registry has not
    /// reported — the lazy host's first work item (spec assumption
    /// 4's "first query" wake), so segments render without a dir
    /// touch. The trip's sync report is the point; its reply applies
    /// like any other (the token matches, the answers land). One
    /// shot: the flag spends with the send, and a registry that
    /// later reports needs no second bootstrap.
    pub fn boot_query_due(&mut self) -> Option<u64> {
        if self.boot_armed && self.ids.is_empty() {
            self.boot_armed = false;
            self.token += 1;
            return Some(self.token);
        }
        self.boot_armed = false;
        None
    }

    /// Apply a query reply: a stale flag or a superseded token drops
    /// it whole; otherwise shown values fill their slots, a hidden
    /// (nil) clears its text keeping its side, and omitted ids keep
    /// their priors.
    pub fn apply_reply(&mut self, token: u64, stale: bool, answers: Vec<SegmentAnswer>) {
        if stale || token != self.token {
            return;
        }
        for answer in answers {
            match answer {
                SegmentAnswer::Shown { id, side, content } => {
                    self.slots.insert(id, (side, Some(content)));
                }
                SegmentAnswer::Hidden { id } => {
                    self.slots
                        .entry(id)
                        .and_modify(|slot| slot.1 = None)
                        .or_insert((ScriptSide::Left, None));
                }
            }
        }
    }

    /// Apply the config's ordered-id key: `None` renders the
    /// registry's own order; `Some` renders exactly the listed ids,
    /// in list order. Cheap when unchanged (one equality) — the
    /// shell may call it every refresh.
    pub fn configure(&mut self, order: Option<&[String]>) {
        let order = order.map(<[String]>::to_vec);
        if order != self.order {
            self.order = order;
        }
    }

    /// The render overlay: one slot per rendered id — the registry's
    /// order, or the configured list filtered to it — an unanswered
    /// id laying absent (text `None`); an empty registry (or one the
    /// config empties) lays the empty vec (today's band,
    /// byte-identical).
    pub fn render(&self) -> Vec<ScriptSegmentValue> {
        let order: Vec<&str> = match &self.order {
            Some(list) => list
                .iter()
                .filter(|id| self.ids.contains(id))
                .map(String::as_str)
                .collect(),
            None => self.ids.iter().map(String::as_str).collect(),
        };
        order
            .into_iter()
            .map(|id| {
                let slot = self.slots.get(id);
                ScriptSegmentValue {
                    id: id.to_owned(),
                    side: slot.map_or(ScriptSide::Left, |(side, _)| *side),
                    // The content rides whole (styled-status-slot):
                    // the painter lays its painted runs — the
                    // byte-compat pin holds because a string answer
                    // composes its own single run.
                    content: slot.and_then(|(_, content)| content.clone()),
                }
            })
            .collect()
    }

    /// The requery decision (the spec's three triggers): an empty
    /// registry never fires and never touches bookkeeping; otherwise
    /// a drained dirty request, an input diff, or the interval
    /// ([`SCRIPT_QUERY_INTERVAL`] past the last dispatch) fires one
    /// dispatch — the token bumps and the ctx becomes the comparison
    /// baseline. Returns the dispatch's token for the send; the send
    /// itself is the shell's (never on render, never blocking the
    /// tick).
    pub fn requery_due(&mut self, ctx: &serde_json::Value, now: Instant) -> Option<u64> {
        if self.ids.is_empty() {
            return None;
        }
        let interval_due = self
            .queried_at
            .is_none_or(|at| now >= at + SCRIPT_QUERY_INTERVAL);
        let diff = self.last_ctx.as_ref() != Some(ctx);
        if !self.dirty && !interval_due && !diff {
            return None;
        }
        self.dirty = false;
        self.queried_at = Some(now);
        self.last_ctx = Some(ctx.clone());
        self.token += 1;
        Some(self.token)
    }
}

/// The resolved segments of the active tab's status band, left
/// cluster then right, values without labels — a path reads as a
/// path. The disc's fill state rides `running`; the process name is
/// the foreground child's comm, or the recorded shell's basename when
/// the shell itself owns the foreground (or the comm read failed —
/// the honest fallback, never a blank band).
//
// No `Eq`: the attention reds are f32 tuples — color values compared
// by field, never hashed or totally ordered.
#[derive(Debug, Clone, PartialEq, Default)]
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
    /// The running command's live elapsed (command-lifecycle spec),
    /// from the engine's timer — absent when hidden, unwitnessed,
    /// or idle. The display formats it ([`duration_display`]); no
    /// wall clock is ever read here.
    pub running_elapsed: Option<Duration>,
    /// The last finished command's (name, duration) pair
    /// (command-lifecycle spec, OQ-1): the app's last sampled
    /// running comm beside its frozen duration, until the next
    /// command starts. An empty name is the honest floor — the
    /// duration without a name to lie about.
    pub frozen: Option<(String, Duration)>,
    /// The last finished command's non-zero exit code (status-exit
    /// spec): the shell's own 133;D report, until the next command
    /// starts. `None` is both success and no report — the band never
    /// guesses a verdict.
    pub exit_code: Option<u32>,
    /// The viewport's lines scrolled back from the bottom
    /// (status-scrolled spec) — absent at the bottom (and when
    /// hidden): the same indistinguishable `None`, so the chip
    /// exists only when the viewport is somewhere it shouldn't
    /// rest. The display is [`scrolled_display`]; the click is the
    /// jump, never a copy.
    pub scrolled: Option<u16>,
    /// The active tab's pasted-image count (paste-chip spec) —
    /// absent at zero (and when there is no active tab): the exit
    /// chip's indistinguishable `None`, so the chip exists only
    /// when there are pastes to point at. The display is
    /// [`pasted_display`]; the click opens the images panel, never
    /// a copy.
    pub pasted: Option<usize>,
    /// The running command's own progress report (osc94-progress
    /// spec), verbatim from the engine — the display
    /// ([`progress_display`]) and the error-red decision derive at
    /// paint; the copy payload is the bare number. Absent when no
    /// report stands (and when hidden): the same
    /// indistinguishable `None`.
    pub progress: Option<crate::core::terminal::ReportedProgress>,
    /// The program's own status report (osc7501-status spec),
    /// verbatim from the engine — the disc's tint decision
    /// ([`program_needs_attention`]) and the name-seat percent
    /// ([`process_display`]) derive at paint. Absent when no record
    /// stands (and when hidden): the same indistinguishable
    /// `None`.
    pub program_status: Option<crate::core::terminal::ProgramStatus>,
    /// Output landed on the active tab while the surface was hidden
    /// and no interaction has acknowledged it since
    /// (status-activity spec) — the halo's fact. A pass-through:
    /// the app samples it under [`activity_mark_due`]'s gate, and
    /// no segment toggle hides it — the mark is the surface's
    /// news, not the process segment's decoration.
    pub missed_activity: bool,
    /// The live scheme's plain red, as `(r, g, b)` linear f32 —
    /// the attention token's source (status-activity spec). Flows
    /// ungated by the mark key: status-exit's chip reuses the
    /// token under its own key.
    pub attention_red: Option<(f32, f32, f32)>,
    /// The live scheme's bright red — [`Self::attention_red`]'s
    /// sibling for dark strips.
    pub attention_bright_red: Option<(f32, f32, f32)>,
    /// The script-provided segments' resolved slots (status-segments
    /// spec), the shell's render order — an empty vec is today's band
    /// exactly (the pin): no group lays, nothing consumes room.
    pub script: Vec<ScriptSegmentValue>,
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
    /// The shell's own cwd report (OSC 7, osc7-cwd spec) —
    /// `file://host/path`, decoded. A standing report outranks both
    /// layers it replaces: the ssh stand-in and the `/proc` cwd. The
    /// host renders only when it differs from ours — the remote case
    /// is the one that matters; remote paths never collapse (no
    /// guessed `$HOME`). Gathered ungated: the branch rule reads the
    /// fact even when the path key hides the seat, which the
    /// resolver gates itself.
    pub reported_cwd: Option<crate::core::terminal::ReportedCwd>,
    /// Our own hostname (osc7-cwd spec) — the host-qualification
    /// base: a reported host renders only when it differs from
    /// this. Gathered once at init, passed every snapshot.
    pub hostname: Option<&'a str>,
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
    /// The running command's live elapsed (the engine's timer).
    pub running_elapsed: Option<Duration>,
    /// The last finished command's (name, duration) pair — the
    /// name the app last sampled running on this tab.
    pub frozen: Option<(&'a str, Duration)>,
    /// The last finished command's exit code — the shell's own
    /// 133;D report sampled from the engine's timer under the exit
    /// toggle's gate (key off means never gathered and never
    /// rendered).
    pub exit_status: Option<u32>,
    /// The viewport's lines scrolled back from the bottom — the
    /// engine's `scrolled_back_lines()` read at the same lock the
    /// grid and zoom take.
    pub scrolled_back: Option<u16>,
    /// The active tab's pasted-image count (paste-chip spec) — the
    /// ledger's length, pruned at the panel's own moments (open and
    /// remove; a file deleted out from under a row counts until the
    /// next one); ungathered past zero by construction. No toggle
    /// gates it: the chip's visibility is its conditionality
    /// (paste-chip OQ-4).
    pub pasted: Option<usize>,
    /// The running command's own progress report (OSC 9;4,
    /// osc94-progress spec) — the engine's standing fact, gathered
    /// under the progress toggle's gate. `None` (no integration, a
    /// clear, the handoff, or the decay window) shows nothing:
    /// never a guess.
    pub progress: Option<crate::core::terminal::ReportedProgress>,
    /// The program's own status report (OSC 7501, osc7501-status
    /// spec) — the engine's standing root record, gathered under
    /// the program-status toggle's gate. `None` (no integration, a
    /// clear, the handoff for its live states, a full reset) shows
    /// nothing: never a guess.
    pub program_status: Option<crate::core::terminal::ProgramStatus>,
    /// The foreground ssh command's destination token (status-remote
    /// spec) — the engine's cmdline read, gathered only at the
    /// snapshot points and only under [`ssh_read_due`]'s gate;
    /// `None` (not ssh, unreadable, unparseable) keeps today's
    /// local path and branch.
    pub ssh_destination: Option<&'a str>,
    /// The engine's missed-activity latch, sampled only under
    /// [`activity_mark_due`]'s gate (status-activity spec) — key
    /// off means never gathered and never rendered.
    pub missed_activity: bool,
    /// The live scheme's plain red `(r, g, b)` — the attention
    /// token's source, sampled ungated (status-exit's chip reuses
    /// the token under its own key).
    pub attention_red: Option<(f32, f32, f32)>,
    /// The scheme's bright red — [`Self::attention_red`]'s sibling
    /// for dark strips.
    pub attention_bright_red: Option<(f32, f32, f32)>,
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
            .then(|| {
                // A standing report outranks both layers it
                // replaces (osc7-cwd spec): the shell said its own
                // cwd, so the ssh stand-in and the `/proc` guess
                // both stand down — the report renders
                // host-qualified only when it names another machine,
                // `~`-collapsed only against our own `$HOME` (a
                // remote path never borrows a local abbreviation).
                // The copy payload rides the same field, as the
                // stand-in's does.
                inputs
                    .reported_cwd
                    .as_ref()
                    .map(|report| reported_path_value(report, inputs.hostname, inputs.home))
                    // The ssh stand-in (status-remote spec): while
                    // ssh owns the foreground and its argv certifies
                    // a destination, the path's own seat shows that
                    // token verbatim — the local cwd froze when ssh
                    // started, and the band never narrates a stale
                    // seat. The copy payload rides the same field:
                    // the token is the full value.
                    .or_else(|| {
                        inputs
                            .ssh_destination
                            .map(str::to_string)
                            .or_else(|| inputs.cwd.map(|cwd| collapse_home(cwd, inputs.home)))
                    })
            })
            .flatten(),
        // The branch hides while stood-in (status-remote OQ-1): the
        // token's presence is the ssh fact — the local repo's branch
        // is as stale as the path was. Keyed on the fact, not the
        // seat's visibility; the app's read gate keeps the two
        // reachable states one and the same. A remote report hides
        // it too (osc7-cwd spec) — the same staleness, extended; a
        // local report leaves the branch to its own cwd read.
        branch: (toggles.branch
            && inputs.ssh_destination.is_none()
            && !inputs
                .reported_cwd
                .as_ref()
                .is_some_and(|report| report.names_remote_host(inputs.hostname)))
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
        running_elapsed: toggles.duration.then_some(inputs.running_elapsed).flatten(),
        frozen: toggles
            .duration
            .then(|| {
                inputs
                    .frozen
                    .map(|(name, duration)| (name.to_string(), duration))
            })
            .flatten(),
        // The verdict gate: hidden, absent, and success are all the
        // same indistinguishable `None` (status-exit spec) — the
        // filter repeats the model's own normalization so the
        // resolver's contract holds regardless of its feeder.
        exit_code: toggles
            .exit
            .then_some(inputs.exit_status)
            .flatten()
            .filter(|&code| code != 0),
        // Absent at bottom — the same indistinguishable None a
        // hidden or zoomed-at-base segment resolves to
        // (status-scrolled spec); the chip exists only when the
        // viewport is somewhere it shouldn't rest.
        scrolled: toggles
            .scrolled
            .then_some(inputs.scrolled_back)
            .flatten()
            .filter(|&lines| lines > 0),
        // The pasted count, ungated by a toggle (paste-chip spec):
        // the chip's visibility is its conditionality — zero is the
        // same indistinguishable None a hidden segment resolves to.
        pasted: inputs.pasted.filter(|&count| count > 0),
        // The engine's standing report, verbatim — hidden and
        // absent the same indistinguishable None (osc94-progress
        // spec); the display and the error-red decision derive at
        // paint, the copy payload from the value.
        progress: toggles.progress.then_some(inputs.progress).flatten(),
        // The engine's standing record, verbatim — the same
        // indistinguishable None (osc7501-status spec); the disc's
        // tint and the name-seat percent derive at paint.
        program_status: toggles
            .program_status
            .then_some(inputs.program_status)
            .flatten(),
        // Pass-throughs (status-activity spec): the mark is the
        // surface's news, so no segment toggle gates it (the
        // config-key gate lives upstream at the sampling), and the
        // red pair rides ungated — status-exit's chip reuses the
        // token under its own key.
        missed_activity: inputs.missed_activity,
        attention_red: inputs.attention_red,
        attention_bright_red: inputs.attention_bright_red,
        // Script slots are the drain's to apply, never the
        // resolver's — resolve starts them empty and the shell's
        // stale-keep overlays its live state after (status-segments
        // spec).
        script: Vec::new(),
    }
}

/// The duration segment's display (command-lifecycle spec): the
/// bare elapsed while running — the live name sits beside it in the
/// process cluster, no duplication — and the frozen pair after the
/// finish (OQ-1: `cargo 2:41`, never a bare duration the shell
/// would own). Nothing when the timer has nothing honest.
pub fn duration_display(segments: &StatusSegments) -> String {
    match (&segments.running_elapsed, &segments.frozen) {
        (Some(elapsed), _) => format_duration(*elapsed),
        (None, Some((name, duration))) if name.is_empty() => format_duration(*duration),
        (None, Some((name, duration))) => format!("{name} {}", format_duration(*duration)),
        (None, None) => String::new(),
    }
}

/// The scrolled chip's display (status-scrolled spec, OQ-1):
/// `↑12` — the arrow, then the count, reading "12 up". The chip's
/// click is the jump, not a copy; this display is the whole value.
pub fn scrolled_display(lines: u16) -> String {
    format!("↑{lines}")
}

/// The pasted chip's display (paste-chip spec): `N images` — plain
/// words over the old `img N` shorthand (paste-chip OQ-5: a count a
/// first-time reader parses without decoding), pluralized honestly,
/// no emoji: the band font is the terminal's own and owes no glyph
/// coverage. The chip's click opens the images panel, not a copy;
/// this display is the whole value.
pub fn pasted_display(count: usize) -> String {
    format!("{count} image{}", if count == 1 { "" } else { "s" })
}

/// The pasted chip's hover tip (paste-chip OQ-5): names what the
/// count counts and what the click does — the two facts the resting
/// chip cannot carry. The scrolled chip's tip sibling; the copy
/// segments' own tips come from [`segment_tip`].
pub fn pasted_tip(count: usize) -> String {
    format!(
        "{} pasted {}. Click to open the images panel.",
        count,
        if count == 1 { "image" } else { "images" }
    )
}

/// The scrolled chip's hover tip: the jump verb the resting `↑N`
/// cannot say — the pasted tip's sibling, same rules.
pub fn scrolled_tip(lines: u16) -> String {
    format!(
        "Scrolled back {lines} {}. Click to jump to bottom.",
        if lines == 1 { "line" } else { "lines" }
    )
}

/// The leading glyph a segment's display carries (user-ruled
/// 2026-11-09, then re-ruled the same day): every segment carries a
/// mark. Ten ride the display as monochrome text shapes — the branch,
/// grid, tab, and profile glyphs the first ruling gave the ambiguous
/// values, plus the duration, progress, path, zoom, pasted, and clock
/// glyphs the follow-up gave the rest (a row where every item wears
/// its mark parses faster than one where only some do). The other
/// three segments own their marks already — the process segment's
/// disc, the exit chip's `✗`, the scrolled chip's `↑` — and a text
/// glyph would double them. The glyphs are monochrome text shapes,
/// not emoji — the paste chip's own no-emoji ruling stands — and they
/// ride the display alone: [`copy_text`] payloads and
/// [`resolve_segments`] fields stay bare values. Retunable constants;
/// the hardware pass is the only judge of how a given terminal font
/// renders them (the text system's fallback answers a family that
/// lacks the shape).
pub fn segment_icon(id: SegmentId) -> Option<&'static str> {
    match id {
        // ⎇ — the branch sigil terminals and editors converged on.
        SegmentId::Branch => Some("\u{2387}"),
        // ⊞ — a grid of cells.
        SegmentId::Grid => Some("\u{229E}"),
        // ▤ — stacked pages, a position among them.
        SegmentId::Tab => Some("\u{25A4}"),
        // ◉ — the active persona.
        SegmentId::Profile => Some("\u{25C9}"),
        // ⧗ — the command's cost, run out.
        SegmentId::Duration => Some("\u{29D7}"),
        // ◐ — a dial half-spent, the spinner's own quarter.
        SegmentId::Progress => Some("\u{25D0}"),
        // ⌂ — where you stand, the house of the cwd.
        SegmentId::Path => Some("\u{2302}"),
        // ⇕ — the size verbs, up and down.
        SegmentId::Zoom => Some("\u{21D5}"),
        // ▦ — an image's crosshatch texture.
        SegmentId::Pasted => Some("\u{25A6}"),
        // ◷ — a clock's face, its quarter gone.
        SegmentId::Clock => Some("\u{25F7}"),
        // The disc, the ✗, the ↑ — the marks these three already
        // own. A text glyph would double them.
        SegmentId::Process | SegmentId::Exit | SegmentId::Scrolled => None,
    }
}

/// A segment's display with its leading glyph, when it carries one:
/// `⎇ main`. An empty value stays empty — a glyph alone would
/// narrate a segment that has nothing to say.
pub fn prefixed_display(id: SegmentId, value: &str) -> String {
    match segment_icon(id) {
        Some(icon) if !value.is_empty() => format!("{icon} {value}"),
        _ => value.to_string(),
    }
}

/// The tip a segment pops on hover (user-ruled 2026-11-09), naming
/// what the segment is and its verb: the copy segments state their
/// fact and "click to copy"; the two action chips state their own
/// verbs ([`scrolled_tip`], [`pasted_tip`]). An absent segment tips
/// nothing — the empty string lays no paragraph, and the tip overlay
/// paints only over a rendered segment's rect, which an absent
/// segment never has.
pub fn segment_tip(id: SegmentId, segments: &StatusSegments) -> String {
    match id {
        SegmentId::Process => {
            if segments.process.is_empty() {
                String::new()
            } else {
                format!("Foreground process: {}. Click to copy.", segments.process)
            }
        }
        SegmentId::Duration => {
            let display = duration_display(segments);
            if display.is_empty() {
                String::new()
            } else {
                format!("Command time: {display}. Click to copy.")
            }
        }
        SegmentId::Exit => segments.exit_code.map_or_else(String::new, |code| {
            format!("Last command failed with code {code}. Click to copy the code.")
        }),
        SegmentId::Progress => segments.progress.map_or_else(String::new, |progress| {
            format!(
                "Command progress: {}. Click to copy.",
                progress_display(progress)
            )
        }),
        SegmentId::Path => segments.path.as_ref().map_or_else(String::new, |_| {
            "Working directory. Click to copy the full path.".to_owned()
        }),
        SegmentId::Branch => segments.branch.as_ref().map_or_else(String::new, |name| {
            format!("Git branch: {name}. Click to copy.")
        }),
        SegmentId::Profile => segments.profile.as_ref().map_or_else(String::new, |name| {
            format!("Profile: {name}. Click to copy.")
        }),
        SegmentId::Grid => segments.grid.map_or_else(String::new, |(cols, rows)| {
            format!("Terminal size: {cols}×{rows} cells. Click to copy.")
        }),
        SegmentId::Tab => segments
            .tab_position
            .map_or_else(String::new, |(index, total)| {
                format!("Tab {index} of {total}. Click to copy.")
            }),
        SegmentId::Zoom => segments.zoom_percent.map_or_else(String::new, |percent| {
            format!("Font zoom: {percent:+}%. Click to copy.")
        }),
        // The action chips' own verbs, verbatim.
        SegmentId::Scrolled => segments.scrolled.map_or_else(String::new, scrolled_tip),
        SegmentId::Pasted => segments.pasted.map_or_else(String::new, pasted_tip),
        SegmentId::Clock => segments
            .clock
            .as_ref()
            .map_or_else(String::new, |_| "Local time. Click to copy.".to_owned()),
    }
}

/// The exit chip's display (status-exit spec): `✗{code}` — the
/// cross, then the shell's own verdict, verbatim (OQ-2: no clamp).
/// The click copies the bare code ([`copy_text`]'s payload), never
/// the glyph.
pub fn exit_display(code: u32) -> String {
    format!("✗{code}")
}

/// Why an image paste refused (the audit's F7): the paste road's
/// visible verdict, riding the band's flash channel the way the ✓
/// copy flash does — a refusal is a paste *action's* verdict, so it
/// is no segment's fact: no toggle owns it, no copy payload exists,
/// and its click does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteRefusal {
    /// The image's bytes passed the paste cap; nothing cached,
    /// nothing pasted — never a fallback that pastes a URI where
    /// the user expected their image.
    OverCap,
    /// The image road failed before any bytes could land — an
    /// unreadable offer, a failed read, a paste-cache write
    /// failure. The honest umbrella: the band names the failure,
    /// never guesses its cause.
    Failed,
}

/// The refusal verdict's display — the seat's swap text (the ✓
/// flash's rule generalized: the verdict takes a seat, measured
/// like any text, the row re-lays). The cap renders as MiB —
/// [`crate::core::paste_cache::PASTE_PAYLOAD_CAP`] is whole MiB by
/// construction.
pub fn refusal_display(refusal: PasteRefusal) -> String {
    match refusal {
        PasteRefusal::OverCap => format!(
            "✗ image over {} MiB cap",
            crate::core::paste_cache::PASTE_PAYLOAD_CAP / (1024 * 1024)
        ),
        PasteRefusal::Failed => "✗ paste failed".to_owned(),
    }
}

/// The seat the verdict chip takes while fresh — the pasted chip
/// when one stands (its position is where the user's paste news
/// lives), the clock otherwise, the right cluster's tail that a
/// rendered band always carries.
pub fn refusal_seat(segments: &StatusSegments) -> SegmentId {
    if segments.pasted.is_some() {
        SegmentId::Pasted
    } else {
        SegmentId::Clock
    }
}

/// The progress chip's display (osc94-progress spec): the percent
/// the reporter gave — `42%`; `…` when indeterminate owns the chip
/// or no value ever stood; the value when the reporter says error
/// (painted in the attention red beside the exit chip's rule — `!`
/// alone when none ever stood). Paused renders its percent plain:
/// the percent is the truth, paused is not a hue (OQ-1).
pub fn progress_display(progress: crate::core::terminal::ReportedProgress) -> String {
    use crate::core::terminal::ProgressState;
    match progress.state {
        ProgressState::InProgress | ProgressState::Paused => progress
            .value
            .map_or_else(|| "…".to_owned(), |value| format!("{value}%")),
        // Indeterminate owns the chip — a standing value is not a
        // number worth narrating while the reporter says it cannot
        // measure.
        ProgressState::Indeterminate => "…".to_owned(),
        ProgressState::Error => progress
            .value
            .map_or_else(|| "!".to_owned(), |value| format!("{value}%")),
    }
}

/// The process segment's display with the program's own report
/// standing (osc7501-status spec): any state carrying a progress
/// percent shows it beside the name — `cargo 40%`, the name seat's
/// own idiom — and a record without one leaves the name bare. The
/// copy payload is unaffected: the name stays the name.
pub fn process_display(
    name: &str,
    program_status: Option<crate::core::terminal::ProgramStatus>,
) -> String {
    match program_status.and_then(|status| status.progress) {
        Some(percent) => format!("{name} {percent}%"),
        None => name.to_owned(),
    }
}

/// Whether the standing program report tints the disc's fill
/// `attention` (osc7501-status spec): `blocked` — the program is
/// waiting on *you* — and `error` — it stopped on failure — carry
/// the token's own pair, both reds; `working`, `done`, and `idle`
/// leave the disc to its `/proc` truth, and `done` deliberately so:
/// finishes are already narrated by their own honest channels, and a
/// second finish signal would be a second narrator. No record tints
/// nothing.
pub fn program_needs_attention(
    program_status: Option<crate::core::terminal::ProgramStatus>,
) -> bool {
    use crate::core::terminal::ProgramState;
    program_status
        .is_some_and(|status| matches!(status.state, ProgramState::Blocked | ProgramState::Error))
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

/// The path seat's value from a standing OSC 7 report (osc7-cwd
/// spec): the report's path, host-qualified only when it names
/// another machine — the remote case is the one that matters — and
/// `~`-collapsed only against our own `$HOME`; a remote path renders
/// verbatim, because no remote `$HOME` is known and a guessed
/// collapse is a lie.
fn reported_path_value(
    report: &crate::core::terminal::ReportedCwd,
    hostname: Option<&str>,
    home: Option<&Path>,
) -> String {
    let path = Path::new(&report.path);
    if report.names_remote_host(hostname) {
        format!(
            "{}:{}",
            report.host.as_deref().unwrap_or_default(),
            path.display()
        )
    } else {
        collapse_home(path, home)
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
/// drop step, never uniformly compressed. Deterministic for a fixed
/// input — fewest drops, then least elision.
pub fn band_fit(width: f32, fixed: f32, widths: &[(SegmentId, f32)], path: PathMeasure) -> BandFit {
    band_fit_with_scripts(width, fixed, widths, path, &[]).0
}

/// [`band_fit`]'s pass with script seats in the accounting
/// (status-segments spec): each seat consumes room like any present
/// segment, and the group gives way first — the last seat in order
/// drops, then the next, before any builtin yields; once the group
/// is exhausted the builtin [`DROP_ORDER`] loop takes over
/// unchanged. An empty `script_widths` is exactly [`band_fit`].
/// Returns the builtin fit plus how many leading script seats
/// survive.
pub fn band_fit_with_scripts(
    width: f32,
    fixed: f32,
    widths: &[(SegmentId, f32)],
    path: PathMeasure,
    script_widths: &[f32],
) -> (BandFit, usize) {
    let mut dropped: Vec<SegmentId> = Vec::new();
    let mut kept_scripts = script_widths.len();
    let fit = loop {
        let script_consumed: f32 = script_widths[..kept_scripts]
            .iter()
            .map(|w| SEGMENT_GAP + w)
            .sum();
        let consumed: f32 = widths
            .iter()
            .filter(|(id, _)| !dropped.contains(id))
            .map(|(_, w)| SEGMENT_GAP + w)
            .sum();
        let room = width - fixed - consumed - script_consumed;
        // The path's inner step: [`left_squeeze`] as pure elision —
        // the branch rides `widths` like any droppable segment, so
        // its eager internal yield is subsumed by the patient
        // DROP_ORDER loop above it.
        let (fit, _) = left_squeeze(room, path.chars, path.full_width, 0.0);
        if !matches!(fit, PathFit::Gone) {
            break fit;
        }
        // Even a fully elided path cannot fit — a script seat gives
        // way first (the last in order), then the next present
        // builtin segment in precedence order.
        if kept_scripts > 0 {
            kept_scripts -= 1;
            continue;
        }
        match DROP_ORDER
            .iter()
            .find(|id| widths.iter().any(|(wid, _)| wid == *id) && !dropped.contains(id))
        {
            Some(next) => dropped.push(*next),
            None => break fit,
        }
    };
    (BandFit { path: fit, dropped }, kept_scripts)
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
        SegmentId::Duration => duration_display(segments),
        SegmentId::Exit => segments
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_default(),
        // The bare number, never the glyph or the `%` — the exit
        // chip's own convention; the valueless shapes (`…`, `!`)
        // have no number to copy.
        SegmentId::Progress => segments
            .progress
            .and_then(|progress| progress.value)
            .map(|value| value.to_string())
            .unwrap_or_default(),
        // The action chip has no copyable value — the jump is the
        // verb (status-scrolled spec). The widget's press routing
        // never reaches this arm; empty is defensive.
        // The band's two action chips share the empty payload: the
        // scrolled chip's jump and the pasted chip's panel-open both
        // route ahead of the copy path (their click verbs), so this
        // arm is defensive only — never a copy target.
        SegmentId::Scrolled | SegmentId::Pasted => String::new(),
    }
}
/// Whether a segment's click runs an action verb rather than the
/// copy verb — the band's two deliberate action chips: the
/// scrolled chip's click jumps the viewport to the bottom
/// (status-scrolled spec) and the pasted chip's opens the images
/// panel (paste-chip spec), both through their app callbacks,
/// ahead of the copy path and never reaching [`copy_text`] (whose
/// payload for them is empty besides, defensive).
pub fn click_jumps(id: SegmentId) -> bool {
    matches!(id, SegmentId::Scrolled | SegmentId::Pasted)
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
// --- the laid band (the view-model pass the renderers share) ---
//
// gpui-status Task 4 moved the GPUI band's pure layout pass here
// framework-free (the model move's precedent): the reference band's
// fit + rect rules over measured widths, one deterministic pass the
// view paints and the shell's interaction machine reads. The
// interaction families below (selection, hover, reveal, menu) all
// walk these laid rows, so the tint, the hit-testing, and the copy
// verbs can never disagree about a strip.

/// Segment text size in logical pixels — one notch below the tab
/// strip's 14 px titles: the band is the terminal speaking about
/// itself, in its own voice (the reference band's own constant).
pub const SEGMENT_FONT_SIZE: f32 = 12.0;

/// The state disc's diameter, and the hollow ring's stroke.
pub const DISC_SIZE: f32 = 8.0;
pub const DISC_RING_WIDTH: f32 = 1.5;

/// The attention halo's clearance outside the disc — the ring sits
/// this far beyond the disc's edge so it rings the disc, never
/// overdraws it.
pub const HALO_GAP: f32 = 1.5;

/// Side padding of the band's clusters.
pub const PAD_X: f32 = 12.0;

/// The inter-segment hairline's painted size: one pixel wide, half
/// the band tall, centered.
pub const SEPARATOR_WIDTH: f32 = 1.0;
pub const SEPARATOR_HEIGHT: f32 = 12.0;

/// The gap between the disc and the process name.
pub const DISC_GAP: f32 = 6.0;

/// The dimmed alpha the right cluster's segments — and the
/// duration's own narration — carry: hierarchy by alpha, never a
/// new hue.
pub const DIM_ALPHA: f32 = 0.6;

/// One laid row: a segment's hit rect (`x`/`width`, band-local),
/// where its text paints (`text_x`, the disc's gap ahead of the
/// process name), and its paint flags — the dim seat and the
/// attention-red seat.
#[derive(Debug, Clone, PartialEq)]
pub struct BandRowVm {
    pub id: SegmentId,
    pub text: String,
    pub x: f32,
    pub width: f32,
    pub text_x: f32,
    pub dimmed: bool,
    /// The attention seat (the exit chip, error-state progress):
    /// painted in the scheme's red, the copy payload unaffected.
    pub attention: bool,
}

/// The process disc: its x (band-local), fill state, and the
/// program report's attention tint.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscVm {
    pub x: f32,
    pub filled: bool,
    pub attention: bool,
}

/// One painted run of a laid script-segment row
/// (styled-status-slot): the span's text with its glyph prefix
/// already composed (the builtin `"{icon} {value}"` idiom — the
/// icon string verbatim, the band's sigil shape). The painter
/// mounts one child per run; ST2 adds the resolved colors.
#[derive(Debug, Clone, PartialEq)]
pub struct BandScriptRunVm {
    pub text: String,
}

/// One laid script-segment row (status-segments spec) — the script
/// groups' own seat type: painted like any row over its laid rect,
/// but non-interactive in v1 (no hover tint, no selection, no copy —
/// the interactions key on [`SegmentId`] and never see these).
#[derive(Debug, Clone, PartialEq)]
pub struct BandScriptRowVm {
    /// The `dropterm.status_segment` registration name.
    pub id: String,
    /// The row's painted runs, in span order — a string answer is
    /// its own single run (styled-status-slot).
    pub runs: Vec<BandScriptRunVm>,
    pub x: f32,
    pub width: f32,
    pub text_x: f32,
    /// The side's cluster idiom: full alpha on the left, dimmed on
    /// the right.
    pub dimmed: bool,
}

/// The band's laid view-model: rows in x-order (left cluster then
/// right), the separator hairlines' gap-center positions, the disc,
/// and the missed-activity halo's center — pure over the resolved
/// segments and measured widths, deterministic for a fixed input.
/// The script segments' rows ride their own vec
/// ([`BandVm::script_rows`]), x-ordered after the builtin rows'
/// sides.
#[derive(Debug, Clone, PartialEq)]
pub struct BandVm {
    pub rows: Vec<BandRowVm>,
    pub separators: Vec<f32>,
    pub disc: Option<DiscVm>,
    /// The halo's center x — `None` when no missed activity stands.
    pub halo_center_x: Option<f32>,
    /// The path seat's fit decision behind the laid row — the
    /// reveal's arming reads its elided flag; a laid path row is
    /// always `Full` or `Elided` (a `Gone` path composes no row).
    pub path_fit: PathFit,
    /// The script segments' laid rows (status-segments spec), in
    /// x-order — the left group's seats, then the right group's.
    pub script_rows: Vec<BandScriptRowVm>,
}

/// One script answer's painted runs (styled-status-slot): a string
/// answer is its own single run; a styled answer composes one run
/// per span, its glyph prefix riding the builtin
/// [`prefixed_display`]-shaped idiom — `{icon} {value}` with text,
/// nothing without (a glyph alone narrates a span with nothing to
/// say). The icon string rides verbatim: it is a glyph string, not
/// a name.
fn script_runs(content: &SlotContent) -> Vec<BandScriptRunVm> {
    match content {
        SlotContent::Text(text) => vec![BandScriptRunVm { text: text.clone() }],
        SlotContent::Styled { spans } => spans
            .iter()
            .map(|span| BandScriptRunVm {
                text: match span.icon.as_deref() {
                    Some(icon) if !span.text.is_empty() => format!("{icon} {}", span.text),
                    _ => span.text.clone(),
                },
            })
            .collect(),
    }
}

/// The band's one layout pass: the reference band's fit + rect
/// rules over measured widths — the path elides first, segments
/// drop in [`DROP_ORDER`] (the fit's accounting seats: the branch
/// and the right cluster, exactly the reference's set — the command
/// narration seats ride outside it, never dropped, the reference's
/// own construction), survivors keeping their cluster seats. The
/// right cluster anchors at the band's right padding, one
/// [`SEGMENT_GAP`] before each seat.
#[expect(
    clippy::too_many_lines,
    reason = "the reference band's three laid passes fused into one — displays, fit, and rects read as one rule"
)]
pub fn band_vm(
    segments: &StatusSegments,
    width: f32,
    measure: &dyn Fn(&str) -> f32,
    copied: Option<SegmentId>,
    paste_refusal: Option<PasteRefusal>,
    hover: Option<(f32, f32)>,
) -> BandVm {
    use crate::core::terminal::ProgressState;

    // The copy flash's swap (the reference's own rule): the ✓ takes
    // the copied segment's display seat alone — measured like any
    // text, so the row re-lays for its flash — while the path's own
    // fit measure below keeps the steady value (the flash never
    // re-fits the path). The refusal verdict (the audit's F7) is
    // the same rule's second tenant: while fresh it takes the
    // pasted chip's seat, or the clock's when no pastes stand —
    // measured like text, the row re-lays, the fit follows.
    let refusal_swap = paste_refusal.map(|r| (refusal_seat(segments), refusal_display(r)));
    let display = |id: SegmentId, text: String| {
        if copied == Some(id) {
            "\u{2713}".to_string()
        } else if let Some((_, verdict)) = refusal_swap.as_ref().filter(|(seat, _)| *seat == id) {
            verdict.clone()
        } else {
            text
        }
    };
    // The display texts, glyphs riding (the copy payloads stay bare
    // values in copy_text — never these).
    let process_text = display(
        SegmentId::Process,
        process_display(&segments.process, segments.program_status),
    );
    // The narration seats' steady texts: the fit accounts their
    // widths (band_fit's contract — every present segment but the
    // path and the process consumes room), and like the path it
    // measures the steady value so the ✓/refusal swap re-lays the
    // row without re-fitting the band.
    let duration_steady = prefixed_display(SegmentId::Duration, &duration_display(segments));
    let duration_text = display(SegmentId::Duration, duration_steady.clone());
    let exit_steady = segments.exit_code.map(exit_display).unwrap_or_default();
    let exit_text = segments
        .exit_code
        .map(|_| display(SegmentId::Exit, exit_steady.clone()));
    let progress_steady = segments
        .progress
        .map(|progress| prefixed_display(SegmentId::Progress, &progress_display(progress)))
        .unwrap_or_default();
    let progress_text = segments
        .progress
        .map(|_| display(SegmentId::Progress, progress_steady.clone()));
    let path_value = prefixed_display(SegmentId::Path, segments.path.as_deref().unwrap_or(""));
    let branch = display(
        SegmentId::Branch,
        prefixed_display(SegmentId::Branch, segments.branch.as_deref().unwrap_or("")),
    );
    let profile = prefixed_display(
        SegmentId::Profile,
        segments.profile.as_deref().unwrap_or(""),
    );
    let grid = segments.grid.map(|(cols, rows)| {
        display(
            SegmentId::Grid,
            prefixed_display(SegmentId::Grid, &format!("{cols}×{rows}")),
        )
    });
    let tab = segments.tab_position.map(|(index, total)| {
        display(
            SegmentId::Tab,
            prefixed_display(SegmentId::Tab, &format!("{index}/{total}")),
        )
    });
    let zoom = segments.zoom_percent.map(|percent| {
        display(
            SegmentId::Zoom,
            prefixed_display(SegmentId::Zoom, &format!("{percent:+}%")),
        )
    });
    let scrolled = segments
        .scrolled
        .map(|lines| display(SegmentId::Scrolled, scrolled_display(lines)));
    let pasted = segments.pasted.map(|count| {
        display(
            SegmentId::Pasted,
            prefixed_display(SegmentId::Pasted, &pasted_display(count)),
        )
    });
    let clock = display(
        SegmentId::Clock,
        prefixed_display(SegmentId::Clock, segments.clock.as_deref().unwrap_or("")),
    );

    // The fit's accounting seats: the branch plus the right
    // cluster, measured — the process cluster and the command
    // narration ride `fixed`-side or unaccounted, exactly the
    // reference's construction.
    let mut widths: Vec<(SegmentId, f32)> = Vec::new();
    let mut right_cluster: Vec<(SegmentId, String)> = Vec::new();
    for (id, text) in [
        (SegmentId::Profile, profile),
        (SegmentId::Grid, grid.unwrap_or_default()),
        (SegmentId::Tab, tab.unwrap_or_default()),
        (SegmentId::Zoom, zoom.unwrap_or_default()),
        (SegmentId::Scrolled, scrolled.unwrap_or_default()),
        (SegmentId::Pasted, pasted.unwrap_or_default()),
        (SegmentId::Clock, clock),
    ] {
        if !text.is_empty() {
            widths.push((id, measure(&text)));
            right_cluster.push((id, text));
        }
    }
    if !branch.is_empty() {
        widths.push((SegmentId::Branch, measure(&branch)));
    }
    // The left cluster's narration seats are fit-accounted like
    // every other present segment (band_fit's contract): the rect
    // walk lays them beside the process, so leaving them out of the
    // sum painted the path and branch straight through the right
    // cluster under width pressure — the overlap the narrow walk
    // exposed.
    for (id, text) in [
        (SegmentId::Duration, &duration_steady),
        (SegmentId::Exit, &exit_steady),
        (SegmentId::Progress, &progress_steady),
    ] {
        if !text.is_empty() {
            widths.push((id, measure(text)));
        }
    }
    let process_width = measure(&process_text);
    let fixed = if process_width > 0.0 {
        PAD_X + DISC_SIZE + DISC_GAP + process_width + SEGMENT_GAP + PAD_X
    } else {
        PAD_X + SEGMENT_GAP + PAD_X
    };
    // The script seats (status-segments spec): every slot whose
    // painted runs compose non-empty text, in the shell's render
    // order — the seats consume room like any present segment and
    // give it back first (the group's tail drops before any builtin
    // yields), so an empty script vec is today's fit exactly (the
    // byte-identical pin). Styled answers lay their painted runs
    // (styled-status-slot): a string answer composes its own single
    // run, so its seat measures today's width exactly.
    let script_seats: Vec<(&ScriptSegmentValue, Vec<BandScriptRunVm>, f32)> = segments
        .script
        .iter()
        .filter_map(|slot| {
            let runs = slot.content.as_ref().map(script_runs)?;
            let painted = runs.iter().map(|run| run.text.as_str()).collect::<String>();
            if painted.is_empty() {
                return None;
            }
            let width: f32 = runs.iter().map(|run| measure(&run.text)).sum();
            Some((slot, runs, width))
        })
        .collect();
    let script_widths: Vec<f32> = script_seats.iter().map(|(_, _, w)| *w).collect();
    let (fit, kept_scripts) = band_fit_with_scripts(
        width,
        fixed,
        &widths,
        PathMeasure {
            // The steady value measures the fit (the reference's
            // rule: the flash's swap never re-fits the path).
            chars: path_value.chars().count(),
            full_width: measure(&path_value),
        },
        &script_widths,
    );
    // The fit's verdicts: dropped seats compose no row.
    let dropped = |id: SegmentId| fit.dropped.contains(&id);

    // The laid rows — the reference's rect walk: the process
    // cluster from the left padding, the narration seats after it,
    // the right cluster anchored at the right padding with one gap
    // before each seat.
    let mut rows: Vec<BandRowVm> = Vec::new();
    let mut disc = None;
    let mut x = PAD_X;
    if process_width > 0.0 {
        disc = Some(DiscVm {
            x,
            filled: segments.running,
            attention: program_needs_attention(segments.program_status),
        });
        rows.push(BandRowVm {
            id: SegmentId::Process,
            text: process_text.clone(),
            x,
            width: DISC_SIZE + DISC_GAP + process_width,
            text_x: x + DISC_SIZE + DISC_GAP,
            dimmed: false,
            attention: false,
        });
        x += DISC_SIZE + DISC_GAP + process_width + SEGMENT_GAP;
    }
    let path_text = match fit.path {
        PathFit::Full => path_value.clone(),
        PathFit::Elided(max_chars) => elide_path_left(&path_value, max_chars),
        PathFit::Gone => String::new(),
    };
    // The inline reveal's seat loan (status-glance criterion 3): an
    // elided path whose cursor rests on its rect re-lays with the
    // branch's seat lent to it — the longest fitting expansion (the
    // whole value when it fits), the branch row vanishing for the
    // loan. Arming reads the path's resting rect, computable ahead
    // of the rect walk (the narration seats ahead of it are already
    // measured); the armed text fits by `reveal_display`'s own
    // construction, and the expanded rect contains the resting one,
    // so arming is stable through the swap.
    let seats_ahead = [
        (SegmentId::Duration, &duration_steady),
        (SegmentId::Exit, &exit_steady),
        (SegmentId::Progress, &progress_steady),
    ]
    .iter()
    .filter(|(id, text)| !text.is_empty() && !dropped(*id))
    .map(|(_, text)| measure(text) + SEGMENT_GAP)
    .sum::<f32>();
    let path_resting_x = x + seats_ahead;
    let armed = reveal_armed(
        (!path_text.is_empty()).then(|| BandRect {
            x: path_resting_x,
            y: 0.0,
            width: measure(&path_text),
            height: BAND_HEIGHT,
        }),
        hover,
        matches!(fit.path, PathFit::Elided(_)),
    );
    let (path_text, branch) = if armed {
        let survivors: Vec<(SegmentId, f32)> = widths
            .iter()
            .copied()
            .filter(|(id, _)| !dropped(*id))
            .collect();
        // The kept script seats keep consuming their room through the
        // loan — the expansion fits beside them, never through them.
        let script_room: f32 = script_seats[..kept_scripts]
            .iter()
            .map(|(_, _, w)| SEGMENT_GAP + w)
            .sum();
        (
            reveal_display(
                &path_value,
                width - script_room,
                fixed,
                &survivors,
                PathMeasure {
                    chars: path_value.chars().count(),
                    full_width: measure(&path_value),
                },
            ),
            String::new(),
        )
    } else {
        (path_text, branch)
    };
    let path_text = display(SegmentId::Path, path_text);
    let narration = [
        (SegmentId::Duration, duration_text),
        (SegmentId::Exit, exit_text.unwrap_or_default()),
        (SegmentId::Progress, progress_text.unwrap_or_default()),
        (SegmentId::Path, path_text),
        (SegmentId::Branch, branch),
    ];
    let progress_is_error = segments
        .progress
        .is_some_and(|progress| progress.state == ProgressState::Error);
    for (id, text) in narration {
        if text.is_empty() || dropped(id) {
            continue;
        }
        let w = measure(&text);
        rows.push(BandRowVm {
            id,
            text,
            x,
            width: w,
            text_x: x,
            dimmed: id == SegmentId::Duration,
            attention: id == SegmentId::Exit
                || (id == SegmentId::Progress && progress_is_error)
                || refusal_swap.as_ref().is_some_and(|(seat, _)| *seat == id),
        });
        x += w + SEGMENT_GAP;
    }
    // The separators and the script groups' own rows collect here,
    // ahead of the cluster walk below, so every walk writes one vec.
    let mut separators = Vec::new();
    let mut script_rows: Vec<BandScriptRowVm> = Vec::new();
    // The left script group (status-segments spec): the trailing
    // cluster after the whole builtin left cluster, full alpha —
    // seats in slot order, hairlines between adjacent seats like any
    // cluster, none across to the builtins (never between clusters).
    let mut prev_end: Option<f32> = None;
    for (slot, runs, w) in script_seats[..kept_scripts]
        .iter()
        .filter(|(slot, _, _)| slot.side == ScriptSide::Left)
    {
        if let Some(start) = prev_end {
            separators.push(f32::midpoint(start, x));
        }
        script_rows.push(BandScriptRowVm {
            id: slot.id.clone(),
            runs: runs.clone(),
            x,
            width: *w,
            text_x: x,
            dimmed: false,
        });
        prev_end = Some(x + w);
        x += w + SEGMENT_GAP;
    }
    let mut right = width - PAD_X;
    for (id, text) in right_cluster.iter().rev() {
        if dropped(*id) {
            continue;
        }
        let w = measure(text);
        right -= w;
        rows.push(BandRowVm {
            id: *id,
            text: text.clone(),
            x: right,
            width: w,
            text_x: right,
            dimmed: true,
            // The refusal verdict's seat wears the attention red
            // (the exit chip's rule) even though the cluster dims —
            // the verdict outranks the cluster's quiet styling.
            attention: refusal_swap.as_ref().is_some_and(|(seat, _)| *seat == *id),
        });
        right -= SEGMENT_GAP;
    }
    // The right script group: hugging the builtin right cluster's own
    // left edge — seats in slot order laid right-to-left with the
    // cluster, dimmed like the cluster they join.
    let mut next_start: Option<f32> = None;
    for (slot, runs, w) in script_seats[..kept_scripts]
        .iter()
        .filter(|(slot, _, _)| slot.side == ScriptSide::Right)
    {
        right -= w;
        if let Some(start) = next_start {
            separators.push(f32::midpoint(right + w, start));
        }
        script_rows.push(BandScriptRowVm {
            id: slot.id.clone(),
            runs: runs.clone(),
            x: right,
            width: *w,
            text_x: right,
            dimmed: true,
        });
        next_start = Some(right);
        right -= SEGMENT_GAP;
    }
    // x-order: the left cluster composed first, the right cluster's
    // seats accumulated right-to-left — reverse them back. (The
    // script groups keep their own vec, already x-ordered.)
    let right_rows = right_cluster.iter().filter(|(id, _)| !dropped(*id)).count();
    let right_start = rows.len() - right_rows;
    rows[right_start..].reverse();

    // The separators: one between each pair of adjacent rendered
    // seats of the same cluster — never between the clusters, never
    // beside an absent one.
    for cluster in [LEFT_CLUSTER.as_slice(), RIGHT_CLUSTER.as_slice()] {
        let mut prev_end: Option<f32> = None;
        for id in cluster {
            let Some(rect) = rows.iter().find(|row| row.id == *id) else {
                continue;
            };
            if let Some(start) = prev_end {
                separators.push(f32::midpoint(start, rect.x));
            }
            prev_end = Some(rect.x + rect.width);
        }
    }

    let halo_center_x = segments.missed_activity.then(|| {
        disc.as_ref()
            .map_or(PAD_X / 2.0, |disc| disc.x + DISC_SIZE / 2.0)
    });
    BandVm {
        rows,
        separators,
        disc,
        halo_center_x,
        path_fit: fit.path,
        script_rows,
    }
}

// --- the band's interactions (gpui-status Task 4) ---
//
// The reference band's walked semantics ported whole (`git show
// 55ba623:src/status_bar.rs`, the cutover's parent): the drag
// selection with its 4 px click discrimination and its per-char
// WYSIWYG assembly, the hover tint's one-narrator rule, the inline
// reveal's arming, and the context menu's live-enablement rows. All
// pure over the laid rows — the renderers supply rects and points.

/// A band-local rectangle — the pure half of the laid rows' hit
/// geometry; the laid rows' rects span the band's full height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl BandRect {
    /// The laid row's hit rect: its x/width strip, full band height.
    pub fn from_row(row: &BandRowVm) -> Self {
        Self {
            x: row.x,
            y: 0.0,
            width: row.width,
            height: BAND_HEIGHT,
        }
    }

    /// Whether the point rests inside — a `width`/`height` of zero
    /// contains nothing.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// The selection-drag threshold: a press that stays within it is a
/// click, past it a selection — the tab row's reorder-drag idiom
/// at a text-selection scale. The double-click's px tolerance rides
/// the same value, one constant fewer.
pub const SELECTION_THRESHOLD: f32 = 4.0;

/// The alpha of the selection highlight — the pane foreground at a
/// quarter strength, hierarchy by alpha, never a new hue.
pub const SELECTION_ALPHA: f32 = 0.25;

/// The hover tint's alpha — the context menu's hover strength, one
/// idiom across the band and its bubble, hierarchy by alpha.
pub const HOVER_ALPHA: f32 = 0.1;

/// The segment whose laid rect contains `point`, if any. Gaps and
/// everything outside the row belong to no segment.
pub fn segment_at(rows: &[BandRowVm], x: f32, y: f32) -> Option<SegmentId> {
    rows.iter()
        .map(BandRect::from_row)
        .position(|rect| rect.contains(x, y))
        .and_then(|index| rows.get(index).map(|row| row.id))
}

/// The segment the hover tint paints — the hovered segment's own
/// rect, the same rects hit-testing walks, so the tint and the copy
/// verb can never disagree about a strip. Suppressed while a live
/// drag or the persisting selection owns the visual (one narrator
/// at a time); an absent cursor, a gap, and everything outside the
/// row tint nothing.
pub fn hover_segment(
    rows: &[BandRowVm],
    cursor: Option<(f32, f32)>,
    selection_live: bool,
) -> Option<SegmentId> {
    if selection_live {
        return None;
    }
    cursor.and_then(|(x, y)| segment_at(rows, x, y))
}

/// Whether the inline reveal is armed (status-glance criterion 3,
/// the OQ-1 inline ruling): the path's display must be elided and
/// the cursor must rest on its **current** display rect — the
/// elided strip at rest, the expanded strip once armed (the
/// expanded rect contains the elided one, so arming is stable
/// through the swap). An un-elided path's display already is its
/// value, other segments arm nothing, and a gone path has no rect.
/// Instant (OQ-3); disarming is absence.
pub fn reveal_armed(
    path_rect: Option<BandRect>,
    cursor: Option<(f32, f32)>,
    path_elided: bool,
) -> bool {
    path_elided
        && matches!(
            (path_rect, cursor),
            (Some(rect), Some((x, y))) if rect.contains(x, y)
        )
}

/// Whether a press-release pair is a click (it rides the copy
/// verb) rather than a drag (it selects).
pub fn selection_is_click(anchor: (f32, f32), release: (f32, f32)) -> bool {
    let dx = release.0 - anchor.0;
    let dy = release.1 - anchor.1;
    (dx * dx + dy * dy) < SELECTION_THRESHOLD * SELECTION_THRESHOLD
}

/// The normalized horizontal span of a selection drag.
fn selection_span(anchor: (f32, f32), focus: (f32, f32)) -> (f32, f32) {
    if anchor.0 <= focus.0 {
        (anchor.0, focus.0)
    } else {
        (focus.0, anchor.0)
    }
}

/// The highlight quads of a selection: the span intersected with
/// each laid row's rect — the visual mark, painted under the text
/// on the armed frames.
pub fn selection_rects(anchor: (f32, f32), focus: (f32, f32), rows: &[BandRowVm]) -> Vec<BandRect> {
    let (lo, hi) = selection_span(anchor, focus);
    rows.iter()
        .map(BandRect::from_row)
        .map(|rect| {
            let left = lo.max(rect.x);
            let right = hi.min(rect.x + rect.width);
            BandRect {
                x: left,
                y: rect.y,
                width: (right - left).max(0.0),
                height: rect.height,
            }
        })
        .filter(|rect| rect.width > 0.0)
        .collect()
}

/// The selected text: the characters whose cells the span covers,
/// segments joined by single spaces (the visual gaps) — WYSIWYG,
/// the elided display included; the click verb remains the full
/// value through [`copy_text`].
pub fn selection_text(anchor: (f32, f32), focus: (f32, f32), rows: &[BandRowVm]) -> String {
    let (lo, hi) = selection_span(anchor, focus);
    let mut parts: Vec<String> = Vec::new();
    for row in rows {
        let chars: Vec<char> = row.text.chars().collect();
        if chars.is_empty() {
            continue;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "px-scale char math on strings < 2^24 chars, exact in f32"
        )]
        let len = chars.len() as f32;
        let per_char = row.width / len;
        if per_char <= 0.0 {
            continue;
        }
        // A character is covered when its cell intersects the span;
        // the cell indices clamp into range before the usize cast.
        let cells =
            |edge: f32, round: fn(f32) -> f32| round((edge - row.x) / per_char).clamp(0.0, len);
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

/// One pick a status-menu row carries. The copy rows resolve
/// against the live segments at pick time — the pressed segment for
/// [`MenuPick::CopySegment`], every present one for
/// [`MenuPick::CopyAll`] — while every action row routes to an
/// existing funnel, the keybinds' own: the menu introduces no
/// behavior of its own (the terminal menu's contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuPick {
    /// Copy the pressed segment's full [`copy_text`] value — the
    /// click verb's twin, ✓ flash included.
    CopySegment(SegmentId),
    /// Copy every present segment's full value, space-joined (the
    /// selection release's join rule).
    CopyAll,
    /// Paste into the narrated terminal — the keybind's own verb.
    Paste,
    ZoomIn,
    ZoomOut,
    /// Back to the base font size — muted while already there.
    ZoomReset,
    /// Hide the band — the `Ctrl+Shift+B` toggle's own write path.
    ToggleStatusBar,
    /// Open the settings drawer.
    Settings,
}

/// One rendered row of the band's context menu — the per-tab
/// menu's row shape: label, the pick a left press publishes
/// (`None` when the row has nothing to pick — the Copy row over a
/// gap press), whether it renders active (the GNOME disabled-row
/// rule), and whether a separator line renders above it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuRow {
    /// Label drawn in the row.
    pub label: String,
    /// The pick a left press publishes.
    pub pick: Option<MenuPick>,
    /// Whether the row renders active and publishes its pick.
    pub enabled: bool,
    /// Whether a separator line renders above the row.
    pub separated: bool,
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
/// opening press's segment — the checklist's composition verbatim:
/// Copy · Copy All · Paste │ Zoom In · Zoom Out · Reset Zoom │
/// Hide Status Bar · Settings… . Copy resolves the pressed
/// segment's value against the same live segments at pick time and
/// mutes on a gap press or an empty payload; Reset Zoom mutes at
/// base size (`zoom_percent`'s own condition); Paste never mutes —
/// the band is chrome, and its menu's Paste serves the terminal
/// under any state. Labels are static; enablement is live,
/// re-derived each open, so a keybind passing through the open
/// menu cannot leave a stale enabled row. Muted rows render
/// disabled and never publish, the tab menu's edge-move rule.
pub fn menu_rows(segments: &StatusSegments, pressed: Option<SegmentId>) -> Vec<MenuRow> {
    vec![
        MenuRow {
            label: "Copy".to_owned(),
            pick: pressed.map(MenuPick::CopySegment),
            enabled: pressed.is_some_and(|id| !copy_text(id, segments).is_empty()),
            separated: false,
        },
        pick_row(
            "Copy All",
            MenuPick::CopyAll,
            !copy_all_text(segments).is_empty(),
            false,
        ),
        pick_row("Paste", MenuPick::Paste, true, false),
        pick_row("Zoom In", MenuPick::ZoomIn, true, true),
        pick_row("Zoom Out", MenuPick::ZoomOut, true, false),
        pick_row(
            "Reset Zoom",
            MenuPick::ZoomReset,
            segments.zoom_percent.is_some(),
            false,
        ),
        pick_row("Hide Status Bar", MenuPick::ToggleStatusBar, true, true),
        pick_row("Settings\u{2026}", MenuPick::Settings, true, false),
    ]
}

/// The Copy All payload: every present segment's full [`copy_text`]
/// value in row order (left cluster then right), joined by single
/// spaces — the selection release's join rule; full values, never
/// displays. Empty when nothing is present.
pub fn copy_all_text(segments: &StatusSegments) -> String {
    LEFT_CLUSTER
        .iter()
        .chain(RIGHT_CLUSTER.iter())
        .map(|id| copy_text(*id, segments))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
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

/// Whether the duration's 1 s tick should exist (command-lifecycle
/// spec): the band shown, the segment shown, the active tab running
/// a foreground child, and the surface visible. `clock_due`'s shape
/// with two more gates — the tick is the prior record's no-standing-
/// timer verdict, so it sleeps only while a command actually runs on
/// a shown surface. Evaluated at every wakeup sync, every config
/// echo, and every fired tick: any gate dropping stands it down
/// within a second.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "the spec's truth-table signature: four independent gates, one bool each"
)]
pub const fn duration_due(
    band_enabled: bool,
    show_duration: bool,
    running: bool,
    surface_visible: bool,
) -> bool {
    band_enabled && show_duration && running && surface_visible
}

/// Whether the ssh stand-in's cmdline read should happen
/// (status-remote spec): the band shown, the path seat shown, and
/// the foreground comm `ssh` — `clock_due`'s shape with the comm
/// gate. Evaluated at every snapshot refresh: hidden means never
/// gathered (the read stops before `/proc`), and a non-ssh
/// foreground never pays it.
pub const fn ssh_read_due(band_enabled: bool, show_path: bool, foreground_is_ssh: bool) -> bool {
    band_enabled && show_path && foreground_is_ssh
}

/// Whether the missed-activity fact should be sampled
/// (status-activity spec): the band shown and the mark enabled —
/// [`clock_due`]'s shape. Gates only the fact's gathering (key off
/// means never gathered and never rendered), never the halo's
/// rules; the scheme's red pair rides ungated beside it, because
/// status-exit's chip reuses the attention token under its own
/// key. Evaluated at every status sampling.
pub const fn activity_mark_due(band_enabled: bool, mark_enabled: bool) -> bool {
    band_enabled && mark_enabled
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

/// `m:ss` under an hour, `h:mm:ss` over — one shape live and frozen
/// (command-lifecycle spec, OQ-3).
pub fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (hours, minutes, secs) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
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

/// The gap between segments — the fit math's accounting unit beside
/// every measured width ([`band_fit`] reserves one before each
/// segment); the renderers' own layout gaps match it.
pub const SEGMENT_GAP: f32 = 12.0;

#[cfg(test)]
mod tests;
