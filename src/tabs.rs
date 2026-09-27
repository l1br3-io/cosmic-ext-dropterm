// SPDX-License-Identifier: GPL-3.0-only

//! The tab model: identifiers, the [`Tab`] record, and pure tab-list
//! operations (insert/close/reorder/cycle with wrap, title fallback).
//!
//! The operations are kept free of iced types — and generic over the tab
//! payload, since they only reorder entries — so they stay unit-testable
//! headless, without a live [`Terminal`] PTY. The app instantiates them
//! with [`Tab`].

use std::sync::Mutex;

use crate::terminal::Terminal;

/// Unique identifier for a tab, monotonically increasing and never reused
/// while the app runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u32);

/// One terminal inside the overlay surface.
///
/// `title` is the last title the shell reported (the default comes from
/// [`next_default_title`]); `title_override` is a user rename, which
/// shell title changes never clobber (see [`apply_shell_title`]).
pub struct Tab {
    pub id: TabId,
    /// The tab's current title: the last one its shell reported,
    /// falling back to the default from [`next_default_title`].
    /// Surfaced by the tab bar widget (T5).
    pub title: String,
    /// A user rename, which shell title changes never clobber;
    /// committed by the inline-rename task (T6).
    pub title_override: Option<String>,
    pub terminal: Mutex<Terminal>,
}

impl Tab {
    /// Creates a tab whose title is the default `Terminal N`, numbered
    /// from `id` so numbers never repeat within a run.
    pub fn new(id: TabId, terminal: Terminal) -> Self {
        Self {
            id,
            title: next_default_title(id),
            title_override: None,
            terminal: Mutex::new(terminal),
        }
    }
}

/// Inserts `tab` directly after the active tab and returns the index to
/// activate (the new tab's own index). The insertion point is clamped
/// into the list, so an out-of-range active index — or an empty list —
/// appends instead.
pub fn insert_after_active<T>(tabs: &mut Vec<T>, active: usize, tab: T) -> usize {
    let at = (active + 1).min(tabs.len());
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
    /// respawn a fresh one, since the overlay is never left empty (see
    /// the tabs spec).
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
    let next_active = if index < active {
        active - 1
    } else if index > active {
        active
    } else {
        active.min(tabs.len().saturating_sub(1))
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
// Consumed by the drag-reorder task (T7); tests keep it exercised until
// then.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "wired up by the drag-reorder task (T7)")
)]
pub fn move_tab<T>(tabs: &mut Vec<T>, from: usize, to: usize) -> usize {
    let tab = tabs.remove(from);
    tabs.insert(to, tab);
    to
}

/// The default title for a newly created tab: `Terminal N`, numbered from
/// the tab's id so numbers never repeat within a run.
pub fn next_default_title(id: TabId) -> String {
    format!("Terminal {}", id.0 + 1)
}

/// Applies a shell-reported title to a tab's stored title: the report
/// lands only while no user rename (`title_override`) is set — a rename
/// is never clobbered by shell title changes (see the tabs spec). A
/// title reset reports the tab's default (see [`next_default_title`]).
pub fn apply_shell_title(title: &mut String, title_override: Option<&str>, reported: &str) {
    if title_override.is_none() {
        *title = reported.to_owned();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_after_active_places_new_tab_after_active() {
        let mut list = vec![10, 20, 30];
        let new_active = insert_after_active(&mut list, 1, 99);
        assert_eq!(list, vec![10, 20, 99, 30]);
        assert_eq!(new_active, 2);
    }

    #[test]
    fn insert_after_active_appends_when_active_is_last() {
        let mut list = vec![10, 20];
        let new_active = insert_after_active(&mut list, 1, 99);
        assert_eq!(list, vec![10, 20, 99]);
        assert_eq!(new_active, 2);
    }

    #[test]
    fn insert_after_active_fills_an_empty_list() {
        let mut list = Vec::new();
        let new_active = insert_after_active(&mut list, 0, 7);
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
}
