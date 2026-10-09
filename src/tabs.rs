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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_current_placement_puts_the_new_tab_after_active() {
        let mut list = vec![10, 20, 30];
        let new_active = insert_new_tab(&mut list, 1, true, 99);
        assert_eq!(list, vec![10, 20, 99, 30]);
        assert_eq!(new_active, 2);
    }

    #[test]
    fn after_current_placement_appends_when_active_is_last() {
        let mut list = vec![10, 20];
        let new_active = insert_new_tab(&mut list, 1, true, 99);
        assert_eq!(list, vec![10, 20, 99]);
        assert_eq!(new_active, 2);
    }

    #[test]
    fn after_current_placement_fills_an_empty_list() {
        let mut list = Vec::new();
        let new_active = insert_new_tab(&mut list, 0, true, 7);
        assert_eq!(list, vec![7]);
        assert_eq!(new_active, 0);
    }

    #[test]
    fn at_end_placement_appends_past_a_mid_list_active() {
        let mut list = vec![10, 20, 30];
        let new_active = insert_new_tab(&mut list, 1, false, 99);
        assert_eq!(list, vec![10, 20, 30, 99]);
        assert_eq!(new_active, 3);
    }

    #[test]
    fn at_end_placement_appends_when_active_is_last() {
        let mut list = vec![10, 20];
        let new_active = insert_new_tab(&mut list, 1, false, 99);
        assert_eq!(list, vec![10, 20, 99]);
        assert_eq!(new_active, 2);
    }

    #[test]
    fn at_end_placement_fills_an_empty_list() {
        let mut list = Vec::new();
        let new_active = insert_new_tab(&mut list, 0, false, 7);
        assert_eq!(list, vec![7]);
        assert_eq!(new_active, 0);
    }

    #[test]
    fn close_tab_active_middle_focuses_successor() {
        let mut list = vec![10, 20, 30];
        let decision = close_tab(&mut list, 1, 1).expect("tab should close");
        assert_eq!(list, vec![10, 30]);
        assert_eq!(decision.next_active, 1);
        assert!(!decision.respawn);
    }

    #[test]
    fn close_tab_active_last_focuses_predecessor() {
        let mut list = vec![10, 20, 30];
        let decision = close_tab(&mut list, 2, 2).expect("tab should close");
        assert_eq!(list, vec![10, 20]);
        assert_eq!(decision.next_active, 1);
        assert!(!decision.respawn);
    }

    #[test]
    fn close_tab_before_active_shifts_active_down() {
        let mut list = vec![10, 20, 30];
        let decision = close_tab(&mut list, 0, 2).expect("tab should close");
        assert_eq!(list, vec![20, 30]);
        assert_eq!(decision.next_active, 1);
        assert!(!decision.respawn);
    }

    #[test]
    fn close_tab_after_active_keeps_active() {
        let mut list = vec![10, 20, 30];
        let decision = close_tab(&mut list, 2, 0).expect("tab should close");
        assert_eq!(list, vec![10, 20]);
        assert_eq!(decision.next_active, 0);
        assert!(!decision.respawn);
    }

    #[test]
    fn close_tab_last_tab_decides_to_respawn() {
        let mut list = vec![10];
        let decision = close_tab(&mut list, 0, 0).expect("tab should close");
        assert!(list.is_empty());
        assert_eq!(decision.next_active, 0);
        assert!(decision.respawn);
    }

    #[test]
    fn close_tab_out_of_bounds_is_none_and_keeps_list() {
        let mut list = vec![10];
        assert!(close_tab(&mut list, 5, 0).is_none());
        assert_eq!(list, vec![10]);
    }

    #[test]
    fn cycle_next_advances_one() {
        assert_eq!(cycle_next(&[10, 20, 30], 0), 1);
    }

    #[test]
    fn cycle_next_wraps_around_to_first() {
        assert_eq!(cycle_next(&[10, 20, 30], 2), 0);
    }

    #[test]
    fn cycle_prev_steps_back() {
        assert_eq!(cycle_prev(&[10, 20, 30], 2), 1);
    }

    #[test]
    fn cycle_prev_wraps_around_to_last() {
        assert_eq!(cycle_prev(&[10, 20, 30], 0), 2);
    }

    #[test]
    fn new_tab_cwd_falls_back_to_the_active_tabs_cwd() {
        let inherited = PathBuf::from("/home/jpt/src");
        assert_eq!(
            resolve_new_tab_cwd(None, Some(inherited.clone())),
            Some(inherited)
        );
    }

    #[test]
    fn new_tab_cwd_without_inheritance_spawns_in_the_pty_default() {
        assert_eq!(resolve_new_tab_cwd(None, None), None);
    }

    #[test]
    fn new_tab_cwd_an_explicit_request_wins_over_inheritance() {
        let explicit = PathBuf::from("/tmp");
        let inherited = PathBuf::from("/home/jpt/src");
        assert_eq!(
            resolve_new_tab_cwd(Some(explicit.clone()), Some(inherited)),
            Some(explicit)
        );
    }

    // --- the mode-threaded plain-spawn cwd (default-spawn DS-1) ---

    #[test]
    fn inherit_mode_is_today_exact_chain_and_never_consults_the_default() {
        // Ruling 2: `Inherit` byte-identical — the default leg drops
        // out entirely, set or not, so a dead `default_cwd` can never
        // move (or warn about) an inherit-mode spawn.
        let inherited = PathBuf::from("/home/jpt/src");
        let default = PathBuf::from("/srv");
        assert_eq!(
            resolve_plain_spawn_cwd(
                NewTabCwd::Inherit,
                None,
                Some(inherited.clone()),
                Some(default.clone())
            ),
            Some(inherited)
        );
        assert_eq!(
            resolve_plain_spawn_cwd(NewTabCwd::Inherit, None, None, Some(default)),
            None
        );
    }

    #[test]
    fn default_mode_replaces_the_inherited_leg() {
        // Ruling 2: `Default` *replaces* inheritance — the configured
        // directory wins over the active tab's cwd, not under it.
        let inherited = PathBuf::from("/home/jpt/src");
        let default = PathBuf::from("/srv");
        assert_eq!(
            resolve_plain_spawn_cwd(
                NewTabCwd::Default,
                None,
                Some(inherited),
                Some(default.clone())
            ),
            Some(default)
        );
    }

    #[test]
    fn default_mode_without_a_default_leg_spawns_in_the_pty_default() {
        // The inherited leg is replaced, never kept as a fallback: an
        // unset — or upstream-degraded dead — `default_cwd` means the
        // pty's default directory, not the active tab's cwd.
        let inherited = PathBuf::from("/home/jpt/src");
        assert_eq!(
            resolve_plain_spawn_cwd(NewTabCwd::Default, None, Some(inherited), None),
            None
        );
    }

    #[test]
    fn an_explicit_request_wins_in_default_mode_too() {
        // Explicit `--cwd` wins in both modes — ruling 2's constant.
        let explicit = PathBuf::from("/tmp");
        assert_eq!(
            resolve_plain_spawn_cwd(
                NewTabCwd::Default,
                Some(explicit.clone()),
                Some(PathBuf::from("/home/jpt/src")),
                Some(PathBuf::from("/srv"))
            ),
            Some(explicit)
        );
    }

    #[test]
    fn numbered_activation_targets_the_requested_index() {
        assert_eq!(activate_index_target(3, 0), Some(0));
        assert_eq!(activate_index_target(3, 2), Some(2));
    }

    #[test]
    fn numbered_activation_out_of_range_is_a_no_op() {
        assert_eq!(activate_index_target(3, 3), None);
        assert_eq!(activate_index_target(0, 0), None);
    }

    #[test]
    fn last_tab_activation_targets_the_final_index() {
        assert_eq!(activate_last_target(3), Some(2));
        assert_eq!(activate_last_target(1), Some(0));
    }

    #[test]
    fn last_tab_activation_with_no_tabs_is_a_no_op() {
        assert_eq!(activate_last_target(0), None);
    }

    #[test]
    fn remote_target_absent_resolves_to_the_active_tab() {
        assert_eq!(remote_target_index(3, None, 1), Some(1));
    }

    #[test]
    fn remote_target_index_is_one_based() {
        // The wire's N=1 is the list's index 0; N=len is the last tab.
        assert_eq!(remote_target_index(3, Some(1), 0), Some(0));
        assert_eq!(remote_target_index(3, Some(3), 1), Some(2));
    }

    #[test]
    fn remote_target_beyond_the_list_is_none() {
        assert_eq!(remote_target_index(3, Some(4), 1), None);
    }

    #[test]
    fn remote_target_zero_is_none_even_though_the_parser_rejects_it() {
        // Defense in depth: clap already rejects --tab-index 0, but the
        // helper is the boundary any future wire path goes through.
        assert_eq!(remote_target_index(3, Some(0), 1), None);
    }

    #[test]
    fn remote_target_absent_with_no_tabs_is_none() {
        assert_eq!(remote_target_index(0, None, 0), None);
    }

    #[test]
    fn select_tab_target_is_strictly_one_based() {
        // The wire's N=1 is the list's index 0; N=len is the last tab —
        // there is no `9 = last` idiom and no wrap.
        assert_eq!(select_tab_target(3, 1), Some(0));
        assert_eq!(select_tab_target(3, 3), Some(2));
    }

    #[test]
    fn select_tab_target_beyond_the_list_is_none() {
        assert_eq!(select_tab_target(3, 4), None);
    }

    #[test]
    fn select_tab_target_zero_is_none_even_though_the_parser_rejects_it() {
        // Defense in depth: clap already rejects `select-tab 0`, but the
        // helper is the boundary any future wire path goes through.
        assert_eq!(select_tab_target(3, 0), None);
    }

    #[test]
    fn select_tab_target_with_no_tabs_is_none() {
        assert_eq!(select_tab_target(0, 1), None);
    }

    #[test]
    fn move_target_clamps_at_the_list_edges() {
        // Moving left from index 0 or right from the last index stays
        // put — move binds clamp, they do not wrap.
        assert_eq!(move_target(0, 3, MoveDirection::Left), None);
        assert_eq!(move_target(2, 3, MoveDirection::Right), None);
        assert_eq!(move_target(1, 3, MoveDirection::Left), Some(0));
        assert_eq!(move_target(1, 3, MoveDirection::Right), Some(2));
    }

    #[test]
    fn a_bell_raises_attention_to_the_bell_marker() {
        assert_eq!(Attention::None.on_bell(), Attention::Bell);
        assert_eq!(Attention::Activity.on_bell(), Attention::Bell);
    }

    #[test]
    fn output_in_an_inactive_tab_raises_attention_to_activity_at_least() {
        assert_eq!(Attention::None.on_activity(), Attention::Activity);
    }

    #[test]
    fn output_does_not_demote_an_existing_bell_marker() {
        assert_eq!(Attention::Bell.on_activity(), Attention::Bell);
    }

    #[test]
    fn activating_a_tab_clears_its_attention() {
        assert_eq!(Attention::Bell.on_activated(), Attention::None);
        assert_eq!(Attention::Activity.on_activated(), Attention::None);
    }

    #[test]
    fn attention_markers_prefix_the_tab_title() {
        assert_eq!(Attention::None.marker(), None);
        assert_eq!(Attention::Activity.marker(), Some("• "));
        assert_eq!(Attention::Bell.marker(), Some("\u{1f514} "));
    }

    #[test]
    fn a_pin_prefixes_the_title_ahead_of_any_attention_marker() {
        // Ruling 3: pin, then marker, then title — the pin indicator
        // composes ahead of everything, exactly the attention markers'
        // shape (glyph plus a leading space).
        assert_eq!(title_prefix(true, Attention::None), "\u{1f4cc} ");
        assert_eq!(title_prefix(true, Attention::Activity), "\u{1f4cc} • ");
        assert_eq!(title_prefix(true, Attention::Bell), "\u{1f4cc} \u{1f514} ");
    }

    #[test]
    fn an_unpinned_tab_prefixes_the_attention_marker_alone() {
        assert_eq!(title_prefix(false, Attention::None), "");
        assert_eq!(title_prefix(false, Attention::Activity), "• ");
        assert_eq!(title_prefix(false, Attention::Bell), "\u{1f514} ");
    }

    #[test]
    fn sweep_targets_cover_the_sweep_neighborhood() {
        // Today's shape with every tab loose: Others hits every tab
        // but the anchor on both sides, Right only the tabs after it.
        let loose = [false; 5];
        let is_pinned = |pinned: &bool| *pinned;
        assert_eq!(
            sweep_targets(&loose, 1, CloseSweep::Others, is_pinned),
            vec![0, 2, 3, 4]
        );
        assert_eq!(
            sweep_targets(&loose, 1, CloseSweep::Right, is_pinned),
            vec![2, 3, 4]
        );
    }

    #[test]
    fn sweep_targets_skip_pinned_tabs() {
        // A pin refuses the sweeps as surely as a direct close: pinned
        // tabs drop out of the target list while loose neighbors close.
        let flags = [false, false, true, false, true];
        let is_pinned = |pinned: &bool| *pinned;
        assert_eq!(
            sweep_targets(&flags, 1, CloseSweep::Others, is_pinned),
            vec![0, 3]
        );
        assert_eq!(
            sweep_targets(&flags, 1, CloseSweep::Right, is_pinned),
            vec![3]
        );
    }

    #[test]
    fn a_command_line_splits_into_program_and_args() {
        assert_eq!(
            command_line_parts("htop -u root"),
            Some((
                "htop".to_string(),
                vec!["-u".to_string(), "root".to_string()]
            ))
        );
    }

    #[test]
    fn a_bare_command_line_has_no_trailing_args() {
        assert_eq!(
            command_line_parts("htop"),
            Some(("htop".to_string(), Vec::new()))
        );
    }

    #[test]
    fn an_empty_command_line_is_rejected() {
        assert_eq!(command_line_parts(""), None);
        assert_eq!(command_line_parts("   "), None);
    }

    #[test]
    fn cycling_an_empty_list_stays_at_zero() {
        let empty: [u32; 0] = [];
        assert_eq!(cycle_next(&empty, 0), 0);
        assert_eq!(cycle_prev(&empty, 0), 0);
    }

    #[test]
    fn move_tab_forward_returns_target_index() {
        let mut list = vec![10, 20, 30];
        let new_index = move_tab(&mut list, 0, 2);
        assert_eq!(list, vec![20, 30, 10]);
        assert_eq!(new_index, 2);
    }

    #[test]
    fn move_tab_backward_returns_target_index() {
        let mut list = vec![10, 20, 30];
        let new_index = move_tab(&mut list, 2, 0);
        assert_eq!(list, vec![30, 10, 20]);
        assert_eq!(new_index, 0);
    }

    #[test]
    fn reorder_tabs_moves_the_dragged_tab_and_keeps_it_active() {
        let mut list = vec![10, 20, 30];
        let active = reorder_tabs(&mut list, 0, 0, 2);
        assert_eq!(list, vec![20, 30, 10]);
        assert_eq!(active, 2);
    }

    #[test]
    fn reorder_tabs_keeps_a_later_active_tab_its_index() {
        // Dragging tab 0 between tabs 1 and 2 does not disturb tab 30.
        let mut list = vec![10, 20, 30];
        let active = reorder_tabs(&mut list, 2, 0, 1);
        assert_eq!(list, vec![20, 10, 30]);
        assert_eq!(active, 2);
    }

    #[test]
    fn reorder_tabs_shifts_active_when_a_later_tab_lands_before_it() {
        // Dragging tab 2 to the front pushes the active tab 10 to index 1.
        let mut list = vec![10, 20, 30];
        let active = reorder_tabs(&mut list, 0, 2, 0);
        assert_eq!(list, vec![30, 10, 20]);
        assert_eq!(active, 1);
    }

    #[test]
    fn reorder_tabs_shifts_active_when_an_earlier_tab_lands_after_it() {
        // Dragging tab 0 to the end pulls the active tab 20 up to index 0.
        let mut list = vec![10, 20, 30];
        let active = reorder_tabs(&mut list, 1, 0, 2);
        assert_eq!(list, vec![20, 30, 10]);
        assert_eq!(active, 0);
    }

    #[test]
    fn reorder_tabs_to_the_same_index_is_a_no_op() {
        let mut list = vec![10, 20, 30];
        let active = reorder_tabs(&mut list, 1, 1, 1);
        assert_eq!(list, vec![10, 20, 30]);
        assert_eq!(active, 1);
    }

    #[test]
    fn reorder_tabs_ignores_out_of_range_indices() {
        let mut list = vec![10, 20];
        assert_eq!(reorder_tabs(&mut list, 0, 5, 0), 0);
        assert_eq!(list, vec![10, 20]);
    }

    #[test]
    fn next_default_title_numbers_from_tab_id() {
        assert_eq!(next_default_title(TabId(0)), "Terminal 1");
        assert_eq!(next_default_title(TabId(4)), "Terminal 5");
    }

    #[test]
    fn shell_mode_lands_the_raw_report() {
        let title = resolve_title(
            TabTitleMode::Shell,
            Some("vim — src/main.rs"),
            None,
            None,
            TabId(0),
            None,
        );
        assert_eq!(title, "vim — src/main.rs");
    }

    #[test]
    fn a_missing_report_shows_the_default_number() {
        // Also the RIS story: a reset clears the stored report and
        // the default comes back.
        let title = resolve_title(TabTitleMode::Shell, None, None, None, TabId(4), None);
        assert_eq!(title, "Terminal 5");
    }

    #[test]
    fn a_rename_wins_in_every_mode() {
        for mode in [
            TabTitleMode::Shell,
            TabTitleMode::AbbreviatedPath,
            TabTitleMode::LastSegment,
            TabTitleMode::Numbered,
            TabTitleMode::Profile,
        ] {
            assert_eq!(
                resolve_title(
                    mode,
                    Some("jpt@box:~/src"),
                    Some(Path::new("/home/jpt/src")),
                    None,
                    TabId(0),
                    Some("work")
                ),
                "work"
            );
        }
    }

    // --- the Profile arm (profile-title) ---

    #[test]
    fn profile_mode_shows_the_tab_recorded_profile_name() {
        // The mode's whole point: the tab says `ssh-prod` no matter
        // what the shell's OSC title reports underneath.
        assert_eq!(
            resolve_title(
                TabTitleMode::Profile,
                Some("user@host: ~"),
                None,
                Some("ssh-prod"),
                TabId(0),
                None
            ),
            "ssh-prod"
        );
    }

    #[test]
    fn profile_mode_shows_the_name_before_any_report() {
        // The name needs nothing from the shell: a fresh profile tab
        // is labeled from its first paint.
        assert_eq!(
            resolve_title(
                TabTitleMode::Profile,
                None,
                None,
                Some("top"),
                TabId(0),
                None
            ),
            "top"
        );
    }

    #[test]
    fn profile_mode_rename_pin_wins_over_the_name() {
        // The rename pin is the last word in every mode — Profile is
        // no exception.
        assert_eq!(
            resolve_title(
                TabTitleMode::Profile,
                Some("user@host"),
                None,
                Some("ssh-prod"),
                TabId(0),
                Some("prod box")
            ),
            "prod box"
        );
    }

    #[test]
    fn profile_mode_falls_back_to_shell_behavior_without_a_profile() {
        // A plain tab looks exactly as it does in Shell mode: the raw
        // report when one exists...
        assert_eq!(
            resolve_title(
                TabTitleMode::Profile,
                Some("zsh"),
                None,
                None,
                TabId(0),
                None
            ),
            "zsh"
        );
        // ...and the `Terminal N` default when none does.
        assert_eq!(
            resolve_title(TabTitleMode::Profile, None, None, None, TabId(2), None),
            "Terminal 3"
        );
    }

    #[test]
    fn profile_mode_dangling_id_renders_the_shell_fallback() {
        // Ruling 2: the call site resolves the id at retitle time; a
        // profile deleted under a running tab yields no name, and the
        // mode degrades to Shell behavior — never a stale name.
        assert_eq!(
            resolve_title(
                TabTitleMode::Profile,
                Some("zsh"),
                None,
                None,
                TabId(0),
                None
            ),
            "zsh"
        );
        assert_eq!(
            resolve_title(TabTitleMode::Profile, None, None, None, TabId(4), None),
            "Terminal 5"
        );
    }

    #[test]
    fn numbered_mode_ignores_reports_and_parks_at_the_number() {
        // Guake `use-vte-titles=false`: the title never moves after
        // creation, whatever the shell reports.
        let title = resolve_title(
            TabTitleMode::Numbered,
            Some("jpt@box:~/src"),
            Some(Path::new("/home/jpt/src")),
            None,
            TabId(0),
            None,
        );
        assert_eq!(title, "Terminal 1");
    }

    #[test]
    fn abbreviated_path_shortens_a_tilde_suffix() {
        // SAFETY: the only test that writes HOME; every other
        // resolve_title test uses cwds and reports whose outcome is
        // HOME-independent, so the swap cannot change their results.
        let saved_home = std::env::var("HOME").ok();
        unsafe { std::env::set_var("HOME", "/home/jpt") };
        let title = resolve_title(
            TabTitleMode::AbbreviatedPath,
            Some("jpt@box:~/src/oss/l1br3/ced"),
            Some(Path::new("/home/jpt/src/oss/l1br3/ced")),
            None,
            TabId(0),
            None,
        );
        // The no-report spawn form prefers the same `~` spelling: a
        // fresh tab — or a shell that never titles — shows the
        // abbreviated cwd itself, never the default number, while
        // the cwd is known.
        let spawned = resolve_title(
            TabTitleMode::AbbreviatedPath,
            None,
            Some(Path::new("/home/jpt/src/oss/l1br3/ced")),
            None,
            TabId(0),
            None,
        );
        let spawned_home = resolve_title(
            TabTitleMode::AbbreviatedPath,
            None,
            Some(Path::new("/home/jpt")),
            None,
            TabId(0),
            None,
        );
        match &saved_home {
            Some(home) => unsafe { std::env::set_var("HOME", home) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        assert_eq!(title, "jpt@box:~/s/o/l/ced");
        assert_eq!(spawned, "~/s/o/l/ced");
        assert_eq!(spawned_home, "~");
    }

    #[test]
    fn abbreviated_path_shortens_an_absolute_suffix_identically() {
        // HOME-independent: the absolute spelling is always the first
        // candidate, tilde or not.
        let title = resolve_title(
            TabTitleMode::AbbreviatedPath,
            Some("jpt@box:/home/jpt/src/oss/l1br3/ced"),
            Some(Path::new("/home/jpt/src/oss/l1br3/ced")),
            None,
            TabId(0),
            None,
        );
        assert_eq!(title, "jpt@box:/h/j/s/o/l/ced");
    }

    #[test]
    fn abbreviated_path_leaves_a_non_directory_title_verbatim() {
        let title = resolve_title(
            TabTitleMode::AbbreviatedPath,
            Some("vim — src/main.rs"),
            Some(Path::new("/home/jpt/src")),
            None,
            TabId(0),
            None,
        );
        assert_eq!(title, "vim — src/main.rs");
    }

    #[test]
    fn abbreviated_path_without_a_cwd_shows_the_report_verbatim() {
        let title = resolve_title(
            TabTitleMode::AbbreviatedPath,
            Some("jpt@box:~/src"),
            None,
            None,
            TabId(0),
            None,
        );
        assert_eq!(title, "jpt@box:~/src");
    }

    #[test]
    fn abbreviated_path_without_a_report_abbreviates_the_cwd_itself() {
        // HOME-independent: `/tmp` is no test's HOME, so only the
        // absolute spelling exists and the outcome cannot drift with
        // the environment.
        let title = resolve_title(
            TabTitleMode::AbbreviatedPath,
            None,
            Some(Path::new("/tmp/a/bc/de")),
            None,
            TabId(0),
            None,
        );
        assert_eq!(title, "/t/a/b/de");
    }

    #[test]
    fn abbreviated_path_without_a_report_or_cwd_shows_the_default_number() {
        let title = resolve_title(
            TabTitleMode::AbbreviatedPath,
            None,
            None,
            None,
            TabId(2),
            None,
        );
        assert_eq!(title, "Terminal 3");
    }

    #[test]
    fn last_segment_shows_the_cwds_final_component() {
        let title = resolve_title(
            TabTitleMode::LastSegment,
            Some("jpt@box:~/src/oss/l1br3/ced"),
            Some(Path::new("/home/jpt/src/oss/l1br3/ced")),
            None,
            TabId(0),
            None,
        );
        assert_eq!(title, "ced");
    }

    #[test]
    fn last_segment_renders_root_for_the_root_directory() {
        // Guake verbatim: `/` is "(root)", not the empty string.
        let title = resolve_title(
            TabTitleMode::LastSegment,
            Some("jpt@box:/"),
            Some(Path::new("/")),
            None,
            TabId(0),
            None,
        );
        assert_eq!(title, "(root)");
    }

    #[test]
    fn last_segment_without_a_cwd_shows_the_report_verbatim() {
        // Guake's OSError fallback: an unreadable cwd degrades to the
        // raw report, never an error surface.
        let title = resolve_title(
            TabTitleMode::LastSegment,
            Some("jpt@box:~/src"),
            None,
            None,
            TabId(0),
            None,
        );
        assert_eq!(title, "jpt@box:~/src");
    }

    #[test]
    fn last_segment_at_spawn_shows_the_spawn_cwd_before_any_report() {
        // The form a fresh tab opens with: no shell report exists
        // yet, and the mode must not wait for one — the spawn cwd
        // (seeded in the app's cache at spawn) is already known.
        let title = resolve_title(
            TabTitleMode::LastSegment,
            None,
            Some(Path::new("/home/jpt")),
            None,
            TabId(4),
            None,
        );
        assert_eq!(title, "jpt");
    }

    #[test]
    fn last_segment_with_neither_report_nor_cwd_shows_the_default_number() {
        let title = resolve_title(TabTitleMode::LastSegment, None, None, None, TabId(0), None);
        assert_eq!(title, "Terminal 1");
    }

    #[test]
    fn committed_rename_sets_override_and_displayed_title() {
        let mut title = String::from("vim — src/main.rs");
        let mut title_override = None;
        commit_rename(
            TabTitleMode::Shell,
            Some("vim — src/main.rs"),
            None,
            None,
            TabId(1),
            &mut title,
            &mut title_override,
            "work",
        );
        assert_eq!(title, "work");
        assert_eq!(title_override.as_deref(), Some("work"));
    }

    #[test]
    fn committed_rename_survives_shell_title_changes() {
        let mut title = String::from("Terminal 1");
        let mut title_override = None;
        commit_rename(
            TabTitleMode::Shell,
            None,
            None,
            None,
            TabId(0),
            &mut title,
            &mut title_override,
            "work",
        );
        // A later report resolves under the override and cannot clobber it.
        let resolved = resolve_title(
            TabTitleMode::Shell,
            Some("vim — src/main.rs"),
            None,
            None,
            TabId(0),
            title_override.as_deref(),
        );
        assert_eq!(resolved, "work");
    }

    #[test]
    fn committing_an_empty_rename_clears_the_override_and_re_resolves() {
        let mut title = String::from("work");
        let mut title_override = Some(String::from("work"));
        // The cleared rename falls back to the stored report —
        // immediately, not on the next shell report.
        commit_rename(
            TabTitleMode::Shell,
            Some("vim — src/main.rs"),
            None,
            None,
            TabId(1),
            &mut title,
            &mut title_override,
            "",
        );
        assert_eq!(title_override, None);
        assert_eq!(title, "vim — src/main.rs");
    }

    #[test]
    fn clearing_a_rename_in_numbered_mode_parks_at_the_terminal_number() {
        let mut title = String::from("work");
        let mut title_override = Some(String::from("work"));
        commit_rename(
            TabTitleMode::Numbered,
            Some("jpt@box:~"),
            None,
            None,
            TabId(2),
            &mut title,
            &mut title_override,
            "",
        );
        assert_eq!(title_override, None);
        assert_eq!(title, "Terminal 3");
    }

    #[test]
    fn plan_respawn_keeps_a_user_rename_verbatim_in_title_and_override() {
        // The stickiness rule: a respawn is not a title action, so the
        // rename survives in every mode — displayed title and stored
        // override both.
        for mode in [
            TabTitleMode::Shell,
            TabTitleMode::AbbreviatedPath,
            TabTitleMode::LastSegment,
            TabTitleMode::Numbered,
        ] {
            let plan = plan_respawn(mode, TabId(6), None, Some("work"), "htop");
            assert_eq!(plan.title, "work");
            assert_eq!(plan.title_override.as_deref(), Some("work"));
        }
    }

    #[test]
    fn plan_respawn_without_a_rename_titles_from_the_fresh_terminal_number() {
        let plan = plan_respawn(TabTitleMode::Shell, TabId(6), None, None, "htop");
        assert_eq!(plan.title, "Terminal 7");
    }

    #[test]
    fn plan_respawn_in_last_segment_mode_titles_from_the_seeded_cwd() {
        // The cwd the app hands over is the spawn directory, so the
        // cwd modes resolve from it before any shell report exists.
        let plan = plan_respawn(
            TabTitleMode::LastSegment,
            TabId(6),
            Some(Path::new("/home/jpt/src/ced")),
            None,
            "htop",
        );
        assert_eq!(plan.title, "ced");
    }

    #[test]
    fn plan_respawn_in_abbreviated_mode_titles_from_the_seeded_cwd() {
        // The OSC tracker resets with the new PTY, so AbbreviatedPath
        // has no report to shorten until the new shell emits one; the
        // seeded cwd itself is abbreviated meanwhile — HOME-
        // independent: `/tmp` is no test's HOME, so only the absolute
        // spelling exists.
        let plan = plan_respawn(
            TabTitleMode::AbbreviatedPath,
            TabId(6),
            Some(Path::new("/tmp/src/ced")),
            None,
            "htop",
        );
        assert_eq!(plan.title, "/t/s/ced");
    }

    #[test]
    fn plan_respawn_resets_attention_and_bell_notification_even_from_a_bell() {
        // The plan clears both unconditionally — a Bell the old
        // terminal accrued dies with it, and the new program starts
        // with a clean attention slate.
        let plan = plan_respawn(TabTitleMode::Shell, TabId(6), None, None, "htop");
        assert_eq!(plan.attention, Attention::None);
        assert!(!plan.bell_notified);
    }

    #[test]
    fn plan_respawn_clears_the_ephemeral_flag_a_cmd_tab_carried() {
        // Whatever the tab was before — a `--cmd` tab is ephemeral —
        // a profile respawn makes it a persistent profile tab.
        let plan = plan_respawn(TabTitleMode::Shell, TabId(6), None, None, "htop");
        assert!(!plan.ephemeral);
    }

    #[test]
    fn plan_respawn_records_the_profile_name() {
        let plan = plan_respawn(TabTitleMode::Shell, TabId(6), None, None, "htop");
        assert_eq!(plan.profile.as_deref(), Some("htop"));
    }

    #[test]
    fn plan_respawn_resets_the_shell_title_tracker() {
        // The stored OSC 0/2 report belongs to the dead PTY; the new
        // one has reported nothing yet.
        let plan = plan_respawn(TabTitleMode::Shell, TabId(6), None, None, "htop");
        assert_eq!(plan.shell_title, None);
    }

    #[test]
    fn tab_records_its_spawn_shell() {
        // The heaviest test in the module — the only one constructing
        // a real Terminal, hence a real /bin/sh PTY — because the
        // point under test is exactly what `Tab::new` captures off a
        // live spawn: the shell it was spawned with, recorded so a
        // later session save replays the right one.
        use alacritty_terminal::term::{self as aterm, Config as TermConfig};
        use alacritty_terminal::tty::{Options as PtyOptions, Shell as PtyShell};
        use tokio::sync::mpsc;

        use crate::terminal::TerminalOptions;

        let (tx, _rx) = mpsc::unbounded_channel();
        let terminal = Terminal::new(
            TabId(0),
            tx,
            TermConfig::default(),
            &PtyOptions {
                shell: Some(PtyShell::new("/bin/sh".to_owned(), Vec::new())),
                drain_on_exit: true,
                ..PtyOptions::default()
            },
            &TerminalOptions::default(),
            &aterm::color::Colors::default(),
        )
        .expect("spawn a /bin/sh test terminal");
        let tab = Tab::new(TabId(3), "/bin/bash".to_owned(), terminal);
        assert_eq!(tab.shell, "/bin/bash");
        // A fresh tab is never pinned: only the menu verb — or a
        // restore replaying a saved pin — sets the flag.
        assert!(!tab.pinned);
    }
}
