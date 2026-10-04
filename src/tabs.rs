// SPDX-License-Identifier: GPL-3.0-only

//! The tab model: identifiers, the [`Tab`] record, and pure tab-list
//! operations (insert/close/reorder/cycle with wrap, title fallback).
//!
//! The operations are kept free of iced types — and generic over the tab
//! payload, since they only reorder entries — so they stay unit-testable
//! headless, without a live [`Terminal`] PTY. The app instantiates them
//! with [`Tab`].

use std::path::PathBuf;
use std::sync::Mutex;

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

/// One terminal inside the overlay surface.
///
/// `title` is the last title the shell reported (the default comes from
/// [`next_default_title`]); `title_override` is a user rename, which
/// shell title changes never clobber (see [`apply_shell_title`]).
pub struct Tab {
    pub id: TabId,
    /// The tab's current title: the last one its shell reported,
    /// falling back to the default from [`next_default_title`].
    pub title: String,
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
            title_override: None,
            shell,
            attention: Attention::None,
            ephemeral: false,
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

/// Applies a shell-reported title to a tab's stored title: the report
/// lands only while no user rename (`title_override`) is set — a rename
/// is never clobbered by shell title changes. A title reset reports
/// the tab's default (see [`next_default_title`]).
pub fn apply_shell_title(title: &mut String, title_override: Option<&str>, reported: &str) {
    if title_override.is_none() {
        reported.clone_into(title);
    }
}

/// Applies a committed inline rename: a non-empty new
/// title becomes the tab's user rename — displayed immediately and safe
/// from later shell title changes — while committing an empty title is
/// the explicit "clear" that removes the rename, letting shell titles
/// land again (see [`apply_shell_title`]).
pub fn commit_rename(title: &mut String, title_override: &mut Option<String>, new_title: &str) {
    if new_title.is_empty() {
        *title_override = None;
    } else {
        *title_override = Some(new_title.to_owned());
        new_title.clone_into(title);
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
    fn shell_title_lands_when_no_rename_is_set() {
        let mut title = next_default_title(TabId(0));
        apply_shell_title(&mut title, None, "vim — src/main.rs");
        assert_eq!(title, "vim — src/main.rs");
    }

    #[test]
    fn shell_title_never_clobbers_a_rename() {
        let mut title = String::from("Terminal 1");
        apply_shell_title(&mut title, Some("work"), "vim — src/main.rs");
        assert_eq!(title, "Terminal 1");
    }

    #[test]
    fn title_reset_restores_default_when_no_rename_is_set() {
        let mut title = String::from("vim — src/main.rs");
        apply_shell_title(&mut title, None, &next_default_title(TabId(1)));
        assert_eq!(title, "Terminal 2");
    }

    #[test]
    fn committed_rename_sets_override_and_displayed_title() {
        let mut title = String::from("vim — src/main.rs");
        let mut title_override = None;
        commit_rename(&mut title, &mut title_override, "work");
        assert_eq!(title, "work");
        assert_eq!(title_override.as_deref(), Some("work"));
    }

    #[test]
    fn committed_rename_survives_shell_title_changes() {
        let mut title = String::from("Terminal 1");
        let mut title_override = None;
        commit_rename(&mut title, &mut title_override, "work");
        apply_shell_title(&mut title, title_override.as_deref(), "vim — src/main.rs");
        assert_eq!(title, "work");
    }

    #[test]
    fn committing_an_empty_rename_clears_the_override() {
        let mut title = String::from("work");
        let mut title_override = Some(String::from("work"));
        commit_rename(&mut title, &mut title_override, "");
        assert_eq!(title_override, None);
    }

    #[test]
    fn clearing_a_rename_resumes_shell_titles() {
        let mut title = String::from("work");
        let mut title_override = Some(String::from("work"));
        commit_rename(&mut title, &mut title_override, "");
        apply_shell_title(&mut title, title_override.as_deref(), "vim — src/main.rs");
        assert_eq!(title, "vim — src/main.rs");
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
    }
}
