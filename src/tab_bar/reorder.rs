// SPDX-License-Identifier: GPL-3.0-only

use std::time::{Duration, Instant};

use cosmic::iced::Point;

use super::ButtonGeometry;
use crate::tabs::TabId;

/// How long a displaced button takes to slide to its new slot,
/// cubic-eased. Matches libcosmic's reorderable row at the pinned rev.
const DRAG_ANIMATION: Duration = Duration::from_millis(180);

/// How close a slot's animated position must be to a new target before
/// retargeting treats them as the same spot.
const POSITION_EPSILON: f32 = 0.5;

/// Where a horizontal drag of the tab at `dragged` would drop if released
/// at bar-local `x`: the number of *other* buttons whose center the cursor
/// has passed. A tab must be dragged past a neighbor's center to move by
/// it, so the result is always a valid index of the reordered list.
pub(super) fn drop_index(buttons: &[ButtonGeometry], dragged: usize, x: f32) -> usize {
    buttons
        .iter()
        .enumerate()
        .filter(|(index, button)| *index != dragged && button.bounds.center_x() < x)
        .count()
}

/// The row as it looks while `dragged` is out of it and lands at
/// `drop`: the left-edge x every *other* button eases to — the slot it
/// occupies in the reordered row (`None` for the dragged one, which
/// follows the cursor instead) — plus the x the drop indicator marks,
/// half a gap ahead of the dragged tab's slot in that row.
pub(super) fn displaced_row(
    widths: &[f32],
    dragged: usize,
    drop: usize,
    gap: f32,
) -> (Vec<Option<f32>>, f32) {
    // The reordered row: the dragged button lifted out, then inserted
    // back at the drop slot.
    let mut order: Vec<usize> = (0..widths.len()).filter(|&i| i != dragged).collect();
    order.insert(drop.min(order.len()), dragged);

    let mut targets = vec![None; widths.len()];
    let mut indicator = 0.0;
    let mut x = 0.0;
    for &index in &order {
        if index == dragged {
            indicator = x - gap / 2.0;
        } else {
            targets[index] = Some(x);
        }
        x += widths[index] + gap;
    }
    (targets, indicator)
}

/// Left-edge x of the dragged button while it follows the cursor,
/// clamped to the row so it can never leave the bar.
pub(super) fn followed_x(row_width: f32, button_width: f32, cursor_x: f32, grab_x: f32) -> f32 {
    (cursor_x - grab_x).clamp(0.0, (row_width - button_width).max(0.0))
}

/// The drop slot for a drag of `dragged` whose cursor sits at bar-local
/// `cursor_x` holding the button `grab_x` from its left edge: the
/// *followed button's* center decides, not the cursor's — the button is
/// the thing on screen, so the release lands exactly where the row said
/// it would.
pub(super) fn drop_from_followed(
    buttons: &[ButtonGeometry],
    dragged: usize,
    cursor_x: f32,
    grab_x: f32,
) -> usize {
    let Some(button) = buttons.get(dragged) else {
        return dragged;
    };
    let row_width = buttons
        .last()
        .map_or(0.0, |last| last.bounds.x + last.bounds.width);
    let followed = followed_x(row_width, button.bounds.width, cursor_x, grab_x);
    drop_index(buttons, dragged, followed + button.bounds.width / 2.0)
}

/// An in-progress tab drag: the tab whose button the press landed
/// on, where that press started, how far into the button the grab
/// sits (so the button can follow the cursor at the point it was
/// caught), and — once the press moved past [`DRAG_THRESHOLD`](super::DRAG_THRESHOLD) — the
/// last bar-local x the cursor was seen at. The state machine
/// follows iced's own text editor (pinned rev `08a09e3`): record on
/// press, drag on move, finish on release.
pub(super) struct Drag {
    pub(super) tab: TabId,
    /// Bar-local press position.
    pub(super) origin: Point,
    /// Whether the press has crossed the drag threshold yet.
    pub(super) started: bool,
    /// Last bar-local cursor x, driving the followed button.
    pub(super) x: f32,
    /// Cursor x minus the followed button's left edge at press: the
    /// offset the button keeps while it follows the cursor.
    pub(super) grab_x: f32,
}

/// One button's horizontal slide toward the slot a drag opens or
/// closes for it, cubic-eased over [`DRAG_ANIMATION`] — the same
/// spring as libcosmic's reorderable row (pinned rev,
/// `reorderable_flex_row/widget.rs`).
pub(super) struct SlotAnimation {
    from: f32,
    to: f32,
    started_at: Option<Instant>,
}

impl SlotAnimation {
    pub(super) fn new(x: f32) -> Self {
        Self {
            from: x,
            to: x,
            started_at: None,
        }
    }

    /// The eased x to paint this frame.
    pub(super) fn current(&self) -> f32 {
        let Some(started_at) = self.started_at else {
            return self.to;
        };
        let progress =
            (started_at.elapsed().as_secs_f32() / DRAG_ANIMATION.as_secs_f32()).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - progress).powi(3);
        self.from + (self.to - self.from) * eased
    }

    pub(super) fn is_animating(&self) -> bool {
        self.started_at
            .is_some_and(|started_at| started_at.elapsed() < DRAG_ANIMATION)
    }

    /// Snaps a finished slide onto its target.
    pub(super) fn settle(&mut self) {
        if !self.is_animating() {
            self.from = self.to;
            self.started_at = None;
        }
    }

    /// Starts (or restarts, from wherever it currently paints) a slide
    /// toward `target`; a settled slot already there just snaps.
    pub(super) fn retarget(&mut self, target: f32) {
        if (self.to - target).abs() < POSITION_EPSILON {
            if !self.is_animating() {
                self.from = target;
                self.to = target;
                self.started_at = None;
            }
            return;
        }
        self.from = self.current();
        self.to = target;
        self.started_at = Some(Instant::now());
    }
}
#[cfg(test)]
mod tests {
    use super::super::tests::geometry;
    use super::*;

    #[test]
    fn drop_index_left_of_the_row_drops_first() {
        let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
        assert_eq!(drop_index(&buttons, 0, -10.0), 0);
    }

    #[test]
    fn drop_index_holding_a_neighbors_left_half_keeps_the_slot() {
        let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
        // x = 100 sits inside button 1, left of its center (149): the
        // cursor has not passed any other button's center yet.
        assert_eq!(drop_index(&buttons, 0, 100.0), 0);
    }

    #[test]
    fn drop_index_counts_only_the_neighbors_passed() {
        let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
        // Past button 1's center (149) but not button 2's (253).
        assert_eq!(drop_index(&buttons, 0, 180.0), 1);
        // Past every other button's center.
        assert_eq!(drop_index(&buttons, 0, 260.0), 2);
    }

    #[test]
    fn drop_index_skips_the_dragged_button_itself() {
        let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
        // Dragging the middle tab from within its own span: only button 0
        // was passed, so the drop slot still holds its own position.
        assert_eq!(drop_index(&buttons, 1, 180.0), 1);
        // Past button 2's center, the middle tab drops last.
        assert_eq!(drop_index(&buttons, 1, 260.0), 2);
    }

    #[test]
    fn drop_index_beyond_the_row_drops_last() {
        let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
        assert_eq!(drop_index(&buttons, 0, 10_000.0), 2);
    }

    #[test]
    fn displaced_row_holds_every_button_home_when_the_drop_slot_is_its_own() {
        // Dragging the middle tab without passing a neighbor: the row
        // it leaves behind is the row itself.
        let widths = [90.0, 110.0, 90.0];
        let (targets, indicator) = displaced_row(&widths, 1, 1, 4.0);
        assert_eq!(targets, vec![Some(0.0), None, Some(208.0)]);
        // The dragged tab's own slot starts at 94; the indicator sits
        // in the gap before its left edge.
        assert_eq!(indicator, 94.0 - 2.0);
    }

    #[test]
    fn displaced_row_shifts_followers_left_when_the_drag_moves_right() {
        let widths = [90.0, 110.0, 90.0];
        // Dragging tab 0 past tab 1: tab 1 takes the row's head, and
        // tab 2 — already past the swap — keeps its slot.
        let (targets, _) = displaced_row(&widths, 0, 1, 4.0);
        assert_eq!(targets, vec![None, Some(0.0), Some(208.0)]);
    }

    #[test]
    fn displaced_row_appends_the_dragged_tab_at_the_rows_new_end() {
        let widths = [90.0, 110.0, 90.0];
        // Dragging tab 0 last: the displaced row ends at 204, so the
        // indicator lands half a gap past it.
        let (targets, indicator) = displaced_row(&widths, 0, 2, 4.0);
        assert_eq!(targets, vec![None, Some(0.0), Some(114.0)]);
        assert_eq!(indicator, 114.0 + 90.0 + 2.0);
    }

    #[test]
    fn displaced_row_precedes_the_row_when_dropping_first() {
        let widths = [90.0, 110.0, 90.0];
        let (_, indicator) = displaced_row(&widths, 1, 0, 4.0);
        assert_eq!(indicator, 0.0 - 2.0);
    }

    #[test]
    fn followed_x_tracks_the_cursor_at_the_grab_offset() {
        assert_eq!(followed_x(1000.0, 100.0, 300.0, 30.0), 270.0);
    }

    #[test]
    fn followed_x_clamps_to_the_row() {
        // Neither edge of the followed button may leave the row.
        assert_eq!(followed_x(300.0, 100.0, 10_000.0, 30.0), 200.0);
        assert_eq!(followed_x(300.0, 100.0, -50.0, 30.0), 0.0);
    }

    #[test]
    fn drop_from_followed_decides_by_the_button_not_the_cursor() {
        let buttons = geometry(&[40.0, 60.0, 40.0], 1000.0);
        // Button 0 spans 0..90, button 1's center is at 149. Grabbed
        // 80 into button 0, the cursor at 155 has crossed button 1's
        // center — but the followed button's center (120) has not, so
        // the slot holds.
        assert_eq!(drop_from_followed(&buttons, 0, 155.0, 80.0), 0);
        assert_eq!(drop_index(&buttons, 0, 155.0), 1);
        // Deeper in, the button's own center passes 149 and the drop
        // advances.
        assert_eq!(drop_from_followed(&buttons, 0, 240.0, 80.0), 1);
    }

    #[test]
    fn slot_slides_settle_on_their_target_and_stay_put() {
        let mut slot = SlotAnimation::new(50.0);
        slot.retarget(120.0);
        assert!(slot.is_animating());
        // Let the slide run out; it settles exactly on its target.
        let deadline = Instant::now() + DRAG_ANIMATION + Duration::from_millis(50);
        while slot.is_animating() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            slot.settle();
        }
        assert!(!slot.is_animating());
        assert_eq!(slot.current(), 120.0);
        // Retargeting a settled slot to the same spot restarts nothing.
        slot.retarget(120.0);
        assert!(!slot.is_animating());
        assert_eq!(slot.current(), 120.0);
    }

    #[test]
    fn slot_retarget_mid_slide_starts_from_where_it_paints() {
        let mut slot = SlotAnimation::new(0.0);
        slot.retarget(100.0);
        assert!(slot.is_animating());
        // Mid-slide, retarget back toward the start: the new slide
        // begins from wherever the first one currently paints, not from
        // either endpoint.
        slot.retarget(0.0);
        assert!(slot.is_animating());
        let current = slot.current();
        assert!(
            current > 0.0 && current < 100.0,
            "retarget should start mid-slide, got {current}"
        );
    }
}
