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
/// [`next_default_title`]); `title_override` is a user rename, which shell
/// title changes do not clobber (see the tabs spec).
pub struct Tab {
    pub id: TabId,
    /// The tab's current title; surfaced by the tab bar task (T5).
    #[expect(dead_code, reason = "read by the tab bar widget (T5)")]
    pub title: String,
    /// A user rename, which shell title changes do not clobber; committed
    /// by the inline-rename task (T6).
    #[expect(dead_code, reason = "read by the tab bar widget (T5/T6)")]
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

/// Removes the tab at `index` and returns it together with the index to
/// activate afterward: the closed tab's successor, its predecessor when
/// the final tab closed, and `active` shifted down when a tab before it
/// closed. Returns `None` when `index` is out of bounds. Keeping the
/// list non-empty is the caller's job (the app respawns a fresh tab; see
/// the tabs spec).
pub fn close_tab<T>(tabs: &mut Vec<T>, index: usize, active: usize) -> Option<(T, usize)> {
    if index >= tabs.len() {
        return None;
    }
    let removed = tabs.remove(index);
    let new_active = if index < active {
        active - 1
    } else if index > active {
        active
    } else {
        active.min(tabs.len().saturating_sub(1))
    };
    Some((removed, new_active))
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
        let (removed, new_active) = close_tab(&mut list, 1, 1).expect("tab should close");
        assert_eq!(removed, 20);
        assert_eq!(list, vec![10, 30]);
        assert_eq!(new_active, 1);
    }

    #[test]
    fn close_tab_active_last_focuses_predecessor() {
        let mut list = vec![10, 20, 30];
        let (_, new_active) = close_tab(&mut list, 2, 2).expect("tab should close");
        assert_eq!(list, vec![10, 20]);
        assert_eq!(new_active, 1);
    }

    #[test]
    fn close_tab_before_active_shifts_active_down() {
        let mut list = vec![10, 20, 30];
        let (_, new_active) = close_tab(&mut list, 0, 2).expect("tab should close");
        assert_eq!(list, vec![20, 30]);
        assert_eq!(new_active, 1);
    }

    #[test]
    fn close_tab_after_active_keeps_active() {
        let mut list = vec![10, 20, 30];
        let (_, new_active) = close_tab(&mut list, 2, 0).expect("tab should close");
        assert_eq!(list, vec![10, 20]);
        assert_eq!(new_active, 0);
    }

    #[test]
    fn close_tab_only_tab_yields_empty_list() {
        let mut list = vec![10];
        let (removed, new_active) = close_tab(&mut list, 0, 0).expect("tab should close");
        assert_eq!(removed, 10);
        assert!(list.is_empty());
        assert_eq!(new_active, 0);
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
}
