// SPDX-License-Identifier: GPL-3.0-only

//! The tab model: identifiers, the [`Tab`] record, and pure tab-list
//! operations (insert/close/reorder/cycle with wrap, title fallback).
//!
//! The operations are kept free of iced types — and generic over the tab
//! payload, since they only reorder entries — so they stay unit-testable
//! headless, without a live [`Terminal`] PTY. The app instantiates them
//! with [`Tab`].

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::config::{NewTabCwd, TabTitleMode};
use crate::terminal::Terminal;

/// Unique identifier for a tab, monotonically increasing and never reused
/// while the app runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u32);

/// A tab's attention state, surfaced as a marker prefixed to its
/// tab-bar title: a bell marks 🔔 (kitty's `bell_on_tab`),
/// output in an inactive tab marks activity (iTerm2's dots), and either
/// clears the moment the tab becomes active. Only inactive tabs accrue
/// markers — the active tab is the one the user is already looking at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Attention {
    #[default]
    None,
    /// The tab produced output while inactive.
    Activity,
    /// The tab's terminal rang the bell. A bell is also activity, so
    /// `Bell` outranks it in every transition.
    Bell,
}

impl Attention {
    /// Transition on a bell in this tab.
    #[expect(
        clippy::unused_self,
        reason = "transition-family API: on_bell/on_activity/on_activated stay uniform method calls at every site"
    )]
    pub fn on_bell(self) -> Self {
        Self::Bell
    }

    /// Transition on output in this tab while it is inactive: activity
    /// at least — never demoting an existing bell marker.
    pub fn on_activity(self) -> Self {
        match self {
            Self::Bell => Self::Bell,
            _ => Self::Activity,
        }
    }

    /// Transition on the tab becoming active: whatever marker it wore
    /// is cleared.
    #[expect(
        clippy::unused_self,
        reason = "transition-family API: on_bell/on_activity/on_activated stay uniform method calls at every site"
    )]
    pub fn on_activated(self) -> Self {
        Self::None
    }

    /// The title prefix this state renders, if any.
    pub fn marker(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Activity => Some("• "),
            Self::Bell => Some("\u{1f514} "),
        }
    }
}

/// The pin indicator a pinned tab's title prefixes: 📌 plus a leading
/// space, exactly the attention markers' shape.
pub const PIN_MARKER: &str = "\u{1f4cc} ";

/// The prefix a tab's title paragraph composes ahead of its title:
/// the pin indicator first, then any attention marker (ruling 3 — pin,
/// then marker, then title). Empty when the tab is neither pinned nor
/// marked, so the paragraph composes the bare title.
pub fn title_prefix(pinned: bool, attention: Attention) -> String {
    let mut prefix = String::new();
    if pinned {
        prefix.push_str(PIN_MARKER);
    }
    if let Some(marker) = attention.marker() {
        prefix.push_str(marker);
    }
    prefix
}

/// Which neighbors a bulk close sweep covers: every tab but the
/// anchor, or only those right of it (the per-tab menu's Close Other
/// Tabs and Close Tabs to the Right rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseSweep {
    Others,
    Right,
}

/// Resolves a close sweep's target indices — every tab but `anchor`
/// ([`CloseSweep::Others`]) or only the tabs right of it
/// ([`CloseSweep::Right`]) — with pinned tabs skipped: a pin refuses
/// the sweeps as surely as a direct close, so they never aim at one.
/// `anchor` is a valid index (the callers resolve it via `position`).
/// Pure over the tab payload — the app funnels every target through
/// the gated close front door.
pub fn sweep_targets<T>(
    tabs: &[T],
    anchor: usize,
    sweep: CloseSweep,
    is_pinned: impl Fn(&T) -> bool,
) -> Vec<usize> {
    let len = tabs.len();
    let mut targets: Vec<usize> = match sweep {
        CloseSweep::Others => (0..anchor).chain(anchor + 1..len).collect(),
        CloseSweep::Right => (anchor + 1..len).collect(),
    };
    targets.retain(|index| !is_pinned(&tabs[*index]));
    targets
}

/// One terminal inside the overlay surface.
///
/// `shell_title` is the last raw OSC 0/2 report and `title` the
/// *resolved* display value (the default comes from
/// [`next_default_title`]); `title_override` is a user rename, which
/// resolution never lets a shell report clobber (see
/// [`resolve_title`]).
pub struct Tab {
    pub id: TabId,
    /// The tab's current title: the value [`resolve_title`] produced
    /// for the live mode, falling back to the default from
    /// [`next_default_title`].
    pub title: String,
    /// The last raw OSC 0/2 report, stored in every mode so the
    /// retitle inputs stay fresh: a mode flip or a cleared rename
    /// re-resolves from current state instead of a stale snapshot.
    /// `None` until the first report; a shell reset (RIS) clears it.
    /// Deliberately not serialized — the session saves the resolved
    /// `title`, not its inputs.
    pub shell_title: Option<String>,
    /// A user rename, which shell title changes never clobber.
    pub title_override: Option<String>,
    /// The shell this tab runs, recorded at spawn: a mid-run config
    /// shell change never rewrites a live tab's shell, so a session
    /// save records what each tab actually runs.
    pub shell: String,
    /// The tab's attention marker state. Markers
    /// accrue only while the tab is inactive and clear when it becomes
    /// active; the session schema deliberately ignores this field.
    pub attention: Attention,
    /// Whether this tab is excluded from session saves: a `--cmd`
    /// tab runs an explicit program, and the session
    /// model restores "a fresh shell in a familiar place" — never a
    /// resumed program — so command tabs are transient by design.
    pub ephemeral: bool,
    /// The profile id this tab launched under, if any: recorded at
    /// spawn so the session saves it, and re-resolved against the
    /// live config at restore. `None` for plain-shell and `--cmd`
    /// tabs — profiles are the explicit opt-in that makes a command
    /// tab persistent.
    pub profile: Option<String>,
    /// Whether this tab is pinned: a pinned tab refuses
    /// user-initiated closes, is skipped by the close sweeps, and
    /// cannot be dragged or menu-moved — only its own shell exit
    /// closes it. Set by the per-tab menu's Pin/Unpin row and
    /// persisted through the session; a profile respawn keeps it,
    /// since the respawn plan resets terminal-life state only.
    pub pinned: bool,
    /// Whether this tab already posted a bell notification during the
    /// current hide period: armed when a
    /// notification is posted, cleared for every tab when the surface
    /// is shown — so a bell storm yields one notification per tab per
    /// hide, and the next bell after a show notifies again.
    pub bell_notified: bool,
    pub terminal: Mutex<Terminal>,
}

impl Tab {
    /// Creates a tab titled with the default `Terminal N` (numbered
    /// from `id` so numbers never repeat within a run), recording
    /// `shell` as the shell it was spawned with.
    pub fn new(id: TabId, shell: String, terminal: Terminal) -> Self {
        Self {
            id,
            title: next_default_title(id),
            shell_title: None,
            title_override: None,
            shell,
            attention: Attention::None,
            ephemeral: false,
            profile: None,
            pinned: false,
            bell_notified: false,
            terminal: Mutex::new(terminal),
        }
    }
}

/// Inserts `tab` at the configured position for a new tab and returns the
/// index to activate (the new tab's own index). `after_current` — the
/// `new_tab_after_current` config — picks the position: `true` inserts
/// directly after the active tab, clamped into the list so an
/// out-of-range active index — or an empty list — appends instead;
/// `false` appends at the end (Guake `new-tab-after`). Activation is
/// unaffected: the new tab becomes active either way.
pub fn insert_new_tab<T>(tabs: &mut Vec<T>, active: usize, after_current: bool, tab: T) -> usize {
    let at = if after_current {
        (active + 1).min(tabs.len())
    } else {
        tabs.len()
    };
    tabs.insert(at, tab);
    at
}

/// What should happen after a tab close, as decided by [`close_tab`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseDecision {
    /// The index to activate next: the closed tab's successor, its
    /// predecessor when the final tab closed, or `active` shifted down
    /// when a tab before it closed.
    pub next_active: usize,
    /// Whether the close took the last tab: the app must immediately
    /// respawn a fresh one, since the overlay is never left empty.
    pub respawn: bool,
}

/// Removes the tab at `index` — its terminal's PTY closes with it —
/// and decides what follows for the tab list: the index to activate
/// next (see [`CloseDecision::next_active`]) and whether the never-
/// empty rule now demands a fresh spawn. Returns `None` when `index`
/// is out of bounds.
pub fn close_tab<T>(tabs: &mut Vec<T>, index: usize, active: usize) -> Option<CloseDecision> {
    if index >= tabs.len() {
        return None;
    }
    tabs.remove(index);
    let next_active = match index.cmp(&active) {
        std::cmp::Ordering::Less => active - 1,
        std::cmp::Ordering::Greater => active,
        std::cmp::Ordering::Equal => active.min(tabs.len().saturating_sub(1)),
    };
    Some(CloseDecision {
        next_active,
        respawn: tabs.is_empty(),
    })
}

/// Returns the index of the tab after `active`, wrapping from the last
/// tab back to the first. An empty list yields `0`.
pub fn cycle_next<T>(tabs: &[T], active: usize) -> usize {
    if tabs.is_empty() {
        return 0;
    }
    (active + 1) % tabs.len()
}

/// Returns the index of the tab before `active`, wrapping from the first
/// tab back to the last. An empty list yields `0`.
pub fn cycle_prev<T>(tabs: &[T], active: usize) -> usize {
    if tabs.is_empty() {
        return 0;
    }
    if active == 0 {
        tabs.len() - 1
    } else {
        active - 1
    }
}

/// Moves the tab at `from` to `to` and returns the new index of the moved
/// tab. Indices are bounds-checked by the caller.
pub fn move_tab<T>(tabs: &mut Vec<T>, from: usize, to: usize) -> usize {
    let tab = tabs.remove(from);
    tabs.insert(to, tab);
    to
}

/// Resolves the working directory a new tab spawns in: an explicit request
/// (the CLI's `--cwd`) wins, otherwise the active tab's cwd is inherited
/// (Guake `open-tab-cwd`, cosmic-term `tab_new_inherit_working_directory`),
/// otherwise `None` — the pty's default directory.
pub fn resolve_new_tab_cwd(
    explicit: Option<PathBuf>,
    inherited: Option<PathBuf>,
) -> Option<PathBuf> {
    explicit.or(inherited)
}

/// Resolves the working directory a *plain* new tab spawns in, the
/// `new_tab_cwd` mode threaded through (the default-spawn module):
/// `Inherit` — the default — is exactly [`resolve_new_tab_cwd`], the
/// default leg never consulted, so the mode's chain is byte-identical
/// to today's. `Default` *replaces* the inherited leg with
/// `default_cwd` — the live-checked value the app passes, an unset or
/// degraded-dead key already collapsed to `None` upstream — so a
/// missing default leg means the pty's default directory, never the
/// active tab's cwd: inheritance is replaced, not supplemented. An
/// explicit request (the CLI's `--cwd`) wins in both modes.
pub fn resolve_plain_spawn_cwd(
    mode: NewTabCwd,
    explicit: Option<PathBuf>,
    inherited: Option<PathBuf>,
    default_cwd: Option<PathBuf>,
) -> Option<PathBuf> {
    match mode {
        NewTabCwd::Inherit => resolve_new_tab_cwd(explicit, inherited),
        NewTabCwd::Default => explicit.or(default_cwd),
    }
}

/// Resolves a strict numbered activation (`Ctrl+Shift+1..8`) to the index
/// to activate: the requested index when it exists, `None` — no-op —
/// otherwise (cosmic-term `TabActivate0..7` semantics; out of range never
/// wraps).
pub fn activate_index_target(len: usize, requested: usize) -> Option<usize> {
    (requested < len).then_some(requested)
}

/// Resolves the `Ctrl+Shift+9` activation to the last tab's index
/// (cosmic-term `TabActivate8`): the final tab however many exist, `None`
/// when there are none.
pub fn activate_last_target(len: usize) -> Option<usize> {
    len.checked_sub(1)
}

/// Resolves a remote verb's `--tab-index N` target: wire indices are
/// 1-based, so `N` resolves to `N - 1` while it sits within `1..=len`,
/// and an absent index resolves to the active tab. Anything else —
/// zero, past the end, or an active index into an empty list — is
/// `None`: the caller warns and no-ops, since the fire-and-forget CLI
/// path cannot reply.
pub fn remote_target_index(len: usize, one_based: Option<usize>, active: usize) -> Option<usize> {
    match one_based {
        None => (active < len).then_some(active),
        Some(n) => n.checked_sub(1).filter(|index| *index < len),
    }
}

/// Resolves a remote `select-tab N` activation: the wire index is
/// strict 1-based, so `N` resolves to `N - 1` while it sits within
/// `1..=len` — no wrap, no `9 = last` idiom. Anything else — zero,
/// past the end, no tabs — is `None`: the caller warns and no-ops,
/// since the fire-and-forget CLI path cannot reply. Distinct from the
/// 0-based keybind helper [`activate_index_target`]; the 1-based
/// conversion happens once, here at this boundary.
pub fn select_tab_target(len: usize, one_based: usize) -> Option<usize> {
    one_based.checked_sub(1).filter(|index| *index < len)
}

/// Splits an explicit command line (the CLI's `--cmd`) into its
/// program and trailing arguments: whitespace-split
/// exactly like the `args` config — quoting is not supported. `None`
/// when the line holds no program at all.
pub fn command_line_parts(line: &str) -> Option<(String, Vec<String>)> {
    let mut parts = line.split_whitespace();
    let program = parts.next()?.to_string();
    Some((program, parts.map(str::to_string).collect()))
}

/// Which way a move-bind shifts the active tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDirection {
    Left,
    Right,
}

/// Resolves a move bind to the index the active tab should land at:
/// one slot toward the direction, `None` — no-op — when already at that
/// edge (moves clamp, they never wrap).
pub fn move_target(active: usize, len: usize, direction: MoveDirection) -> Option<usize> {
    match direction {
        MoveDirection::Left => active.checked_sub(1),
        MoveDirection::Right => {
            let to = active + 1;
            (to < len).then_some(to)
        }
    }
}

/// Applies a completed tab drag: moves the tab at `from` to `to` and
/// returns the index the previously active tab landed at, so the same
/// tab stays active whichever tab was dragged. The resulting order is
/// the one the session save persists. Out-of-range indices leave the
/// list untouched.
pub fn reorder_tabs<T>(tabs: &mut Vec<T>, active: usize, from: usize, to: usize) -> usize {
    if tabs.is_empty() || from >= tabs.len() || to >= tabs.len() {
        return active.min(tabs.len().saturating_sub(1));
    }
    move_tab(tabs, from, to);
    // The active tab itself was dragged, or a tab moved from one side of
    // it to the other, shifting it by one; otherwise it keeps its index.
    if active == from {
        to
    } else if from < active && active <= to {
        active - 1
    } else if to <= active && active < from {
        active + 1
    } else {
        active
    }
}

/// The default title for a newly created tab: `Terminal N`, numbered from
/// the tab's id so numbers never repeat within a run.
pub fn next_default_title(id: TabId) -> String {
    format!("Terminal {}", id.0 + 1)
}

/// Resolves the title a tab's button displays, from the inputs every
/// apply point shares: the configured mode, the last raw OSC 0/2
/// report, the tab's cwd, the tab's id-resolved profile name, and its
/// id (the `Terminal N` fallback). A user rename wins in every mode —
/// the stickiness Guake's `custom_label_set`, kitty's `name or
/// title`, and Yakuake's interactive-rename flag all implement. With
/// no rename:
///
/// - `Shell` shows the raw report — today's behavior — `Terminal N`
///   until the first report;
/// - `AbbreviatedPath` shortens a trailing-cwd suffix Guake-style
///   (`compute_tab_title`, `guake_app.py`: every parent component of
///   the matched suffix cut to its first character, the last kept
///   whole) and leaves any other report verbatim. The suffix matches
///   the cwd in both absolute and `$HOME`→`~` spelling — a deliberate
///   superset of Guake, since shells emit `~` in titles while
///   `DropTerm`'s cwd cache is absolute. `$HOME` is the one environment
///   input, the same read `DropTermConfig::resolved_shell` makes of
///   `$SHELL`.
/// - `LastSegment` shows the cwd's final component — `"(root)"` for
///   `/`, Guake verbatim — falling back to the raw report when no
///   cwd is readable (Guake's `OSError` fallback);
/// - `Numbered` parks at `Terminal N`: reports never land (Guake
///   `use-vte-titles=false`);
/// - `Profile` shows the tab's profile name — the id resolved by the
///   call site at retitle time (ruling 2: a dangling id passes
///   `None`, never a stale name), so profile renames and deletes
///   reflect on the next retitle. No name — a plain tab or a dangling
///   id — falls back to exactly `Shell`'s behavior.
pub fn resolve_title(
    mode: TabTitleMode,
    shell_title: Option<&str>,
    cwd: Option<&Path>,
    profile_name: Option<&str>,
    id: TabId,
    title_override: Option<&str>,
) -> String {
    if let Some(rename) = title_override {
        return rename.to_owned();
    }
    match mode {
        TabTitleMode::Shell => raw_report_or_default(shell_title, id),
        TabTitleMode::Numbered => next_default_title(id),
        TabTitleMode::LastSegment => cwd
            .and_then(last_cwd_segment)
            .unwrap_or_else(|| raw_report_or_default(shell_title, id)),
        TabTitleMode::AbbreviatedPath => abbreviated_report(shell_title, cwd, id),
        TabTitleMode::Profile => {
            profile_name.map_or_else(|| raw_report_or_default(shell_title, id), str::to_owned)
        }
    }
}

/// The shared fallback of the report-displaying modes: the raw
/// report, else the `Terminal N` default.
fn raw_report_or_default(shell_title: Option<&str>, id: TabId) -> String {
    shell_title.map_or_else(|| next_default_title(id), str::to_owned)
}

/// Guake's Last Segment value for a cwd: its final path component,
/// `"(root)"` for the root — verbatim — `None` when the path has no
/// meaningful final component.
fn last_cwd_segment(cwd: &Path) -> Option<String> {
    if cwd == Path::new("/") {
        return Some("(root)".to_string());
    }
    cwd.file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

/// Guake's Abbreviated Path reshape: the first cwd spelling the
/// report ends with gets abbreviated; a report with no directory
/// suffix — or no readable cwd — stays verbatim. With no report at
/// all — a fresh spawn before the shell's first title, or a shell
/// that never titles — the mode abbreviates the cwd itself
/// (preferring its `~` spelling), so it still shows a path; only a
/// missing cwd too falls back to the `Terminal N` default.
fn abbreviated_report(shell_title: Option<&str>, cwd: Option<&Path>, id: TabId) -> String {
    let Some(report) = shell_title else {
        return match cwd.map(abbreviated_cwd_display) {
            Some(display) => display,
            None => next_default_title(id),
        };
    };
    let Some(cwd) = cwd else {
        return report.to_owned();
    };
    for suffix in cwd_spellings(cwd) {
        if !suffix.is_empty() && report.ends_with(&suffix) {
            return abbreviate_suffix(report, &suffix);
        }
    }
    report.to_owned()
}

/// The no-report Abbreviated Path title: the cwd's own spelling,
/// cut Guake-style — the `~` form when the cwd lies under `$HOME`
/// (the shorter, conventional display), the absolute form otherwise.
fn abbreviated_cwd_display(cwd: &Path) -> String {
    // `cwd_spellings` always yields the absolute spelling first and
    // appends the `~` form only when it exists, so the last entry is
    // the preferred one.
    let spellings = cwd_spellings(cwd);
    let preferred = spellings
        .last()
        .expect("the absolute spelling always exists");
    abbreviate_suffix(preferred, preferred)
}

/// The spellings of a cwd a shell title may end with: the absolute
/// path, then its `$HOME`→`~` form when the cwd lies under `$HOME`.
/// Absolute first, so results never depend on whether `$HOME` is set.
fn cwd_spellings(cwd: &Path) -> Vec<String> {
    let mut spellings = vec![cwd.to_string_lossy().into_owned()];
    if let Some(home) = std::env::var("HOME").ok().filter(|home| !home.is_empty())
        && let Ok(rest) = cwd.strip_prefix(home.as_str())
    {
        let tilde = if rest.as_os_str().is_empty() {
            "~".to_string()
        } else {
            format!("~/{}", rest.display())
        };
        spellings.push(tilde);
    }
    spellings
}

/// Cuts every `/`-separated component of the matched suffix except
/// the last to its first character, keeping the report's leading text
/// (the `user@host:` part) untouched: `~/src/oss/l1br3/ced` →
/// `~/s/o/l/ced`, `/home/jpt/src/ced` → `/h/j/s/ced`.
fn abbreviate_suffix(report: &str, suffix: &str) -> String {
    // Splitting a non-empty string yields at least one component, so
    // the kept-whole last one always exists; the empty suffix is
    // filtered by the caller before matching.
    let mut components = suffix.split('/');
    let last = components
        .next_back()
        .expect("a non-empty suffix has a last component");
    let mut abbreviated = String::with_capacity(report.len());
    abbreviated.push_str(&report[..report.len() - suffix.len()]);
    for component in components {
        abbreviated.extend(component.chars().take(1));
        abbreviated.push('/');
    }
    abbreviated.push_str(last);
    abbreviated
}

/// Applies a committed inline rename: a non-empty new title becomes
/// the tab's user rename — displayed immediately and safe from later
/// shell title changes — while committing an empty title is the
/// explicit "clear" that removes the rename. Either way the displayed
/// title re-resolves through [`resolve_title`]: a clear shows what
/// the live mode dictates right away instead of parking on the
/// cleared text until the next shell report.
// The argument list mirrors [`resolve_title`]'s inputs plus the
// two mutable title fields and the commit — the policy stays one
// pure fn over the tab's whole title state, so regrouping into a
// params struct would just rename eight locals.
#[expect(
    clippy::too_many_arguments,
    reason = "the fn is the rename policy over the tab's full title state; a params struct would only rename its inputs"
)]
pub fn commit_rename(
    mode: TabTitleMode,
    shell_title: Option<&str>,
    cwd: Option<&Path>,
    profile_name: Option<&str>,
    id: TabId,
    title: &mut String,
    title_override: &mut Option<String>,
    new_title: &str,
) {
    if new_title.is_empty() {
        *title_override = None;
    } else {
        *title_override = Some(new_title.to_owned());
    }
    resolve_title(
        mode,
        shell_title,
        cwd,
        profile_name,
        id,
        title_override.as_deref(),
    )
    .clone_into(title);
}

/// The plain-data half of an in-place respawn: what every
/// non-terminal field of a tab becomes when its terminal is replaced
/// to run a profile. The app swaps the `Terminal` and applies this
/// plan; keeping the policy here — like `resolve_title` and
/// `commit_rename` — makes the respawn rules unit-testable headless.
pub struct RespawnFields {
    pub title: String,
    pub shell_title: Option<String>,
    pub title_override: Option<String>,
    pub attention: Attention,
    pub bell_notified: bool,
    pub ephemeral: bool,
    pub profile: Option<String>,
}

/// Plans the field half of respawning a tab under `profile_name`: the
/// sticky user rename survives (a respawn is not a title action), the
/// OSC 0/2 tracker resets with the new PTY, and the title re-resolves
/// from the live mode with no report yet — a rename wins, the cwd
/// modes read `cwd` (the spawn directory the app seeds), `Shell` and
/// `Numbered` fall back to the fresh `Terminal N` default of the new
/// id, and `Profile` reads the respawn's own profile name. The tab
/// becomes a persistent profile tab (`ephemeral = false`,
/// id recorded) whatever it was before — the pair `spawn_shell` sets
/// for a profile launch.
pub fn plan_respawn(
    mode: TabTitleMode,
    id: TabId,
    cwd: Option<&Path>,
    old_override: Option<&str>,
    profile_name: &str,
) -> RespawnFields {
    RespawnFields {
        title: resolve_title(mode, None, cwd, Some(profile_name), id, old_override),
        shell_title: None,
        title_override: old_override.map(str::to_owned),
        attention: Attention::None,
        bell_notified: false,
        ephemeral: false,
        profile: Some(profile_name.to_owned()),
    }
}

/// Plans the field half of the profile-exit fallback: a profile tab
/// whose child exited respawns the configured plain shell in its
/// place — the never-empty rule generalized from "the last tab" to
/// "every profile tab", because a one-shot profile command (`ls`)
/// must not take its tab with it (the New Tab pick would flash and
/// vanish; the Switch Profile pick would destroy the current slot).
/// [`plan_respawn`]'s resets apply verbatim — rename kept, OSC
/// tracker cleared, markers cleared, `ephemeral = false` — with the
/// profile id dropped: the preset's process is gone, and the tab is
/// a plain shell tab from here on (a later restore must not
/// re-resolve a profile whose exit already played out).
pub fn plan_shell_respawn(
    mode: TabTitleMode,
    id: TabId,
    cwd: Option<&Path>,
    old_override: Option<&str>,
) -> RespawnFields {
    RespawnFields {
        title: resolve_title(mode, None, cwd, None, id, old_override),
        shell_title: None,
        title_override: old_override.map(str::to_owned),
        attention: Attention::None,
        bell_notified: false,
        ephemeral: false,
        profile: None,
    }
}

#[cfg(test)]
mod tests;
