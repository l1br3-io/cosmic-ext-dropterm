use super::*;
use std::path::Path;

use dropterm_core::config::{Edge, TabTitleMode};

#[test]
fn auto_places_the_strip_opposite_the_anchor_and_pins_override() {
    // Auto: opposite the anchored edge — a top-anchored drawer
    // carries the strip at its bottom, a bottom-anchored one at
    // its top.
    assert!(bar_at_bottom(Edge::Top, TabPosition::Auto));
    assert!(!bar_at_bottom(Edge::Bottom, TabPosition::Auto));
    // Top/Bottom pin regardless of anchor.
    assert!(!bar_at_bottom(Edge::Top, TabPosition::Top));
    assert!(!bar_at_bottom(Edge::Bottom, TabPosition::Top));
    assert!(bar_at_bottom(Edge::Top, TabPosition::Bottom));
    assert!(bar_at_bottom(Edge::Bottom, TabPosition::Bottom));
}

#[test]
fn tab_rows_render_from_two_tabs_on() {
    assert!(!shows_tab_buttons(1));
    assert!(shows_tab_buttons(2));
    assert!(shows_tab_buttons(7));
}

#[test]
fn strip_pair_darkens_the_pane_background_and_picks_readable_foreground() {
    use alacritty_terminal::vte::ansi::Rgb;
    // A dark pane background (Nord's #282A36-ish): the strip
    // darkens a step further and the foreground reads near-white.
    let (strip, fg) = strip_pair(Some(Rgb {
        r: 40,
        g: 42,
        b: 54,
    }));
    assert_eq!(strip, rgb_hsla(37, 39, 50));
    assert_eq!(fg, rgb_hsla(0xF5, 0xF5, 0xFF));
    // A light background flips the foreground near-black.
    let (_, fg) = strip_pair(Some(Rgb {
        r: 235,
        g: 235,
        b: 245,
    }));
    assert_eq!(fg, rgb_hsla(0x1B, 0x1B, 0x22));
    // No scheme background: the fallback pair.
    let (strip, fg) = strip_pair(None);
    assert_eq!((strip, fg), (rgb_hsla(0, 0, 0), rgb_hsla(0xF5, 0xF5, 0xFF)));
}

#[test]
fn row_labels_compose_cores_prefix_ahead_of_the_resolved_title() {
    // The no-duplication pin (the chunk's planning ruling): for
    // every pinned × attention combination the row's label is
    // exactly core's title_prefix output ahead of the stored
    // resolved title — the view composes, never re-derives.
    for pinned in [false, true] {
        for attention in [Attention::None, Attention::Activity, Attention::Bell] {
            let mut list = TabList::default();
            list.spawn(false);
            let tabs = list.tabs_mut();
            tabs[0].pinned = pinned;
            tabs[0].attention = attention;
            tabs[0].title = "My Title".to_string();
            let [row] = list.rows().try_into().expect("one row");
            assert_eq!(
                row.label,
                format!("{}My Title", title_prefix(pinned, attention))
            );
        }
    }
}

#[test]
fn rows_flag_the_active_tab_and_carry_state() {
    let mut list = TabList::default();
    let first = list.spawn(false);
    let second = list.spawn(false);
    let rows = list.rows();
    assert_eq!(rows.len(), 2);
    assert!(!rows[0].is_active);
    assert!(rows[1].is_active);
    assert_eq!(rows[0].id, first);
    assert_eq!(rows[1].id, second);

    assert!(list.select(first));
    let rows = list.rows();
    assert!(rows[0].is_active);
    assert!(!rows[1].is_active);
}

#[test]
fn spawns_insert_through_cores_positions_and_activate() {
    let mut list = TabList::default();
    let first = list.spawn(false);
    assert_eq!(list.active_id(), Some(first));

    // after_current: right of the active tab, and active.
    let second = list.spawn(true);
    assert_eq!(
        list.records().iter().map(|tab| tab.id).collect::<Vec<_>>(),
        vec![first, second]
    );
    assert_eq!(list.active_id(), Some(second));

    // append: at the end regardless of the active tab.
    let third = list.spawn(false);
    assert_eq!(
        list.records().iter().map(|tab| tab.id).collect::<Vec<_>>(),
        vec![first, second, third]
    );
    assert_eq!(list.active_id(), Some(third));

    // Fresh spawns title through the id-numbered default.
    assert_eq!(list.records()[0].title, "Terminal 1");
    assert_eq!(list.records()[2].title, "Terminal 3");

    // The empty list's first after_current spawn clamps to append.
    let mut fresh = TabList::default();
    let only = fresh.spawn(true);
    assert_eq!(
        fresh.records().iter().map(|tab| tab.id).collect::<Vec<_>>(),
        vec![only]
    );
}

#[test]
fn select_clears_attention_through_cores_transition_and_ignores_unknowns() {
    let mut list = TabList::default();
    let first = list.spawn(false);
    list.spawn(false);

    // An inactive tab wearing the bell marker.
    list.tabs_mut()[0].attention = Attention::Bell;
    assert!(list.select(first));
    assert_eq!(list.rows()[0].attention, Attention::None);

    // Re-selecting the active tab changes nothing (and reports
    // no change).
    assert!(!list.select(first));
    // An unknown id is a no-op.
    assert!(!list.select(TabId(99)));
    assert_eq!(list.active_id(), Some(first));
}

#[test]
fn titles_follow_the_configured_mode_through_cores_resolution() {
    let mut list = TabList::default();
    let first = list.spawn(false);

    // Shell mode: the report shows, a cleared report falls back
    // to the id-numbered default (core's own storage rule — an
    // empty report is a clear, stored as no-report).
    assert!(list.sync_title(
        first,
        Some("jpt@rig:~".to_string()),
        None,
        TabTitleMode::Shell
    ));
    assert_eq!(list.records()[0].title, "jpt@rig:~");
    assert!(list.sync_title(first, Some(String::new()), None, TabTitleMode::Shell));
    assert_eq!(list.records()[0].title, "Terminal 1");

    // Numbered: reports never land — the title parks at the
    // default whatever the report says.
    list.sync_title(
        first,
        Some("never shown".to_string()),
        None,
        TabTitleMode::Numbered,
    );
    assert_eq!(list.records()[0].title, "Terminal 1");

    // LastSegment: the cwd's final component, with the report as
    // core's documented fallback while no cwd is readable.
    list.sync_title(
        first,
        Some("fallback".to_string()),
        None,
        TabTitleMode::LastSegment,
    );
    assert_eq!(list.records()[0].title, "fallback");
    list.sync_title(
        first,
        Some("fallback".to_string()),
        Some(PathBuf::from("/home/jpt/src")),
        TabTitleMode::LastSegment,
    );
    assert_eq!(list.records()[0].title, "src");

    // A live mode flip re-titles from the stored inputs: the
    // stored report shows again under Shell.
    list.refresh_titles(TabTitleMode::Shell);
    assert_eq!(list.records()[0].title, "fallback");
}

#[test]
fn close_runs_through_cores_close_decision() {
    let mut list = TabList::default();
    let first = list.spawn(false);
    let second = list.spawn(false);
    let third = list.spawn(false);

    // Closing the active middle tab: its successor activates.
    let decision = list.close(second).expect("a decision");
    assert_eq!(decision.next_active, 1);
    assert!(!decision.respawn);
    assert_eq!(list.active_id(), Some(third));

    // Closing the last tab standing signals the never-empty
    // respawn.
    list.close(first);
    let decision = list.close(third).expect("a decision");
    assert!(decision.respawn);
    assert_eq!(list.active_id(), None);

    // An unknown id decides nothing.
    assert!(list.close(TabId(99)).is_none());
}

#[test]
fn rename_commits_through_core_and_sticks_against_reports() {
    let mut list = TabList::default();
    let first = list.spawn(false);

    // A non-empty commit becomes the user rename, shown at once.
    assert!(list.commit_rename(first, "work", TabTitleMode::Shell));
    assert_eq!(list.records()[0].title, "work");
    assert_eq!(list.records()[0].title_override.as_deref(), Some("work"));

    // The rename wins in every mode: a shell report never clobbers
    // it (core's override rule), and a mode flip re-resolves onto
    // the override.
    list.sync_title(
        first,
        Some("jpt@rig:~".to_string()),
        None,
        TabTitleMode::Shell,
    );
    assert_eq!(list.records()[0].title, "work");
    list.refresh_titles(TabTitleMode::Numbered);
    assert_eq!(list.records()[0].title, "work");

    // An empty commit is the explicit clear: the live mode's title
    // returns right away — the *stored* report, which the override
    // had been masking, not the numbered default — and later
    // reports resume landing.
    assert!(list.commit_rename(first, "", TabTitleMode::Shell));
    assert_eq!(list.records()[0].title, "jpt@rig:~");
    assert_eq!(list.records()[0].title_override, None);
    list.sync_title(first, Some("next".to_string()), None, TabTitleMode::Shell);
    assert_eq!(list.records()[0].title, "next");

    // An unknown id renames nothing.
    assert!(!list.commit_rename(TabId(99), "x", TabTitleMode::Shell));
}

#[test]
fn move_to_reorders_through_core_keeping_the_active_tab() {
    let mut list = TabList::default();
    let first = list.spawn(false);
    let second = list.spawn(false);
    let third = list.spawn(false);
    list.select(first);

    // Dragging an inactive tab across the active one keeps the
    // active tab active (core's reorder identity rule).
    assert!(list.move_to(third, 0));
    assert_eq!(
        list.records().iter().map(|tab| tab.id).collect::<Vec<_>>(),
        vec![third, first, second]
    );
    assert_eq!(list.active_id(), Some(first));

    // Dragging the active tab itself carries the activation with
    // it (the moved tab lands at `to` and stays active).
    assert!(list.move_to(first, 2));
    assert_eq!(
        list.records().iter().map(|tab| tab.id).collect::<Vec<_>>(),
        vec![third, second, first]
    );
    assert_eq!(list.active_id(), Some(first));
}

#[test]
fn move_to_refuses_pinned_unknown_and_no_op_drops() {
    let mut list = TabList::default();
    let first = list.spawn(false);
    list.spawn(false);

    // A pinned tab stays put (the reference bar's ruling 4).
    list.tabs_mut()[0].pinned = true;
    assert!(!list.move_to(first, 1));
    assert_eq!(
        list.records().iter().map(|tab| tab.id).collect::<Vec<_>>(),
        vec![first, list.records()[1].id]
    );

    // An unknown id, a drop onto the tab's own slot, and an
    // out-of-range target are all quiet no-ops.
    list.tabs_mut()[0].pinned = false;
    assert!(!list.move_to(TabId(99), 0));
    assert!(!list.move_to(first, 0));
    assert!(!list.move_to(first, 9));
}

#[test]
fn move_by_nudges_one_slot_and_clamps_at_the_edges() {
    let mut list = TabList::default();
    let first = list.spawn(false);
    let second = list.spawn(false);
    let third = list.spawn(false);
    list.select(second);

    // One slot toward the direction, the moved tab not necessarily
    // active — the active tab's identity survives (core's rule).
    // The right edge clamps.
    assert!(!list.move_by(third, MoveDirection::Right));
    assert!(list.move_by(third, MoveDirection::Left));
    assert_eq!(
        list.records().iter().map(|tab| tab.id).collect::<Vec<_>>(),
        vec![first, third, second]
    );
    assert_eq!(list.active_id(), Some(second));

    // The left edge clamps too.
    assert!(!list.move_by(first, MoveDirection::Left));
}

#[test]
fn move_by_refuses_a_pinned_tab() {
    let mut list = TabList::default();
    let first = list.spawn(false);
    list.spawn(false);
    list.tabs_mut()[0].pinned = true;
    assert!(!list.move_by(first, MoveDirection::Right));
}

#[test]
fn toggle_pin_flips_the_clicked_tab_and_ignores_unknowns() {
    let mut list = TabList::default();
    let first = list.spawn(false);
    assert!(!list.records()[0].pinned);
    assert!(list.toggle_pin(first));
    assert!(list.records()[0].pinned);
    assert!(list.toggle_pin(first));
    assert!(!list.records()[0].pinned);
    assert!(!list.toggle_pin(TabId(99)));
}

#[test]
fn spawn_duplicate_seeds_from_the_source_and_lands_after_active() {
    let mut list = TabList::default();
    let first = list.spawn(false);
    let second = list.spawn(false);
    list.sync_title(
        first,
        Some("jpt@rig:~".to_string()),
        Some(PathBuf::from("/home/jpt/src")),
        TabTitleMode::Shell,
    );
    list.commit_rename(first, "work", TabTitleMode::Shell);
    list.select(second);

    // A duplicate is a fresh record in the source's cwd carrying
    // its rename pin, landing after the active tab and activated
    // (the reference `duplicate-tab --tab-index` semantics).
    let dup = list.spawn_duplicate(first, TabTitleMode::Shell);
    assert_eq!(dup, TabId(2));
    assert_eq!(
        list.records().iter().map(|tab| tab.id).collect::<Vec<_>>(),
        vec![first, second, dup]
    );
    assert_eq!(list.active_id(), Some(dup));
    let record = &list.records()[2];
    assert_eq!(record.title_override.as_deref(), Some("work"));
    assert_eq!(record.title, "work");
    assert_eq!(record.cwd.as_deref(), Some(Path::new("/home/jpt/src")));

    // A missing source spawns the plain default record (the
    // keybind's empty-list behavior).
    let plain = list.spawn_duplicate(TabId(99), TabTitleMode::Shell);
    assert_eq!(
        list.records().last().map(|tab| tab.title.clone()),
        Some(next_default_title(plain))
    );
    assert_eq!(
        list.records()
            .last()
            .and_then(|tab| tab.title_override.clone()),
        None
    );
}

#[test]
fn close_sweeps_skip_pinned_neighbors_and_descend() {
    let mut list = TabList::default();
    let ids = [(); 5].map(|()| list.spawn(false));
    list.tabs_mut()[3].pinned = true;

    // Others: every tab but the anchor, the pinned one skipped,
    // returned descending so closes never shift later targets.
    assert_eq!(
        list.close_sweep(ids[1], CloseSweep::Others),
        vec![ids[4], ids[2], ids[0]]
    );

    // Right: only right of the anchor (indices 2 and 4), the
    // pinned neighbor 3 gone the same way.
    assert_eq!(
        list.close_sweep(ids[1], CloseSweep::Right),
        vec![ids[4], ids[2]]
    );

    // An unknown anchor sweeps nothing.
    assert_eq!(
        list.close_sweep(TabId(99), CloseSweep::Others),
        Vec::<TabId>::new()
    );
}

#[test]
fn note_activity_marks_inactive_tabs_only_through_core() {
    let mut list = TabList::default();
    let first = list.spawn(false);
    let second = list.spawn(false);
    // `second` is active (the last spawn); `first` is not.

    // Output in an inactive tab accrues the activity marker…
    list.note_attention(first, false, true);
    assert_eq!(list.rows()[0].attention, Attention::Activity);
    // …a bell outranks it, and later output never demotes it.
    list.note_attention(first, true, true);
    assert_eq!(list.rows()[0].attention, Attention::Bell);
    list.note_attention(first, false, true);
    assert_eq!(list.rows()[0].attention, Attention::Bell);

    // The active tab never accrues a marker — the user is already
    // looking at it.
    list.note_attention(second, true, true);
    assert_eq!(list.rows()[1].attention, Attention::None);

    // An unknown id is a quiet no-op.
    list.note_attention(TabId(99), true, true);
}

#[test]
fn close_retires_the_successors_attention_marker() {
    let mut list = TabList::default();
    let first = list.spawn(false);
    let second = list.spawn(false);
    let third = list.spawn(false);
    // `third` is active; closing it hands activation to its
    // successor-shaped neighbor `second` — wearing a bell, it
    // clears through core's activation transition.
    list.tabs_mut()[1].attention = Attention::Bell;
    let decision = list.close(third).expect("a decision");
    assert_eq!(decision.next_active, 1);
    assert_eq!(list.active_id(), Some(second));
    assert_eq!(list.rows()[1].attention, Attention::None);
    assert_eq!(first, list.records()[0].id);
}

#[test]
fn confirm_needs_both_the_toggle_and_a_running_child() {
    // The close gate's truth table: either input false closes
    // immediately, exactly the reference bar's criterion 3.
    assert!(!confirm_needed(false, false));
    assert!(!confirm_needed(false, true));
    assert!(!confirm_needed(true, false));
    assert!(confirm_needed(true, true));
}

#[test]
fn menu_rows_compose_the_per_tab_surface() {
    // A middle loose tab of five: every row present, both moves
    // live, the close cluster complete.
    let rows = tab_menu_rows(5, 2, false);
    let labels = rows
        .iter()
        .map(|row| row.label.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        vec![
            "New Tab",
            "Rename Tab",
            "Duplicate Tab",
            "Pin Tab",
            "Move Tab Left",
            "Move Tab Right",
            "Close Tab",
            "Close Other Tabs",
            "Close Tabs to the Right",
        ]
    );
    // Every row is clickable (no profiles means no muted header on
    // this surface), each carrying its pick, with the separators
    // ahead of the rename cluster, the moves, and the close cluster.
    assert!(rows.iter().all(|row| row.enabled));
    assert_eq!(
        rows.iter().map(|row| row.pick).collect::<Vec<_>>(),
        vec![
            TabMenuPick::NewTab,
            TabMenuPick::Rename,
            TabMenuPick::Duplicate,
            TabMenuPick::Pin,
            TabMenuPick::Move(MoveDirection::Left),
            TabMenuPick::Move(MoveDirection::Right),
            TabMenuPick::Close,
            TabMenuPick::CloseOthers,
            TabMenuPick::CloseRight,
        ]
    );
    assert_eq!(
        rows.iter().map(|row| row.separated).collect::<Vec<_>>(),
        vec![false, true, false, false, true, false, true, false, false]
    );

    // A pinned tab: the pin row's label flips, both moves and the
    // close row mute (they would refuse), the sweeps stay live.
    let pinned = tab_menu_rows(4, 1, true);
    assert_eq!(pinned[3].label, "Unpin Tab");
    assert!(pinned[3].enabled);
    assert!(!pinned[4].enabled, "Move Tab Left mutes for a pinned tab");
    assert!(!pinned[5].enabled, "Move Tab Right mutes for a pinned tab");
    assert!(!pinned[6].enabled, "Close Tab mutes for a pinned tab");
    assert!(pinned[7].enabled && pinned[8].enabled);

    // Edge clamps: the leftmost tab's Move Left mutes; the
    // rightmost tab's Move Right mutes and its Close-Right row is
    // absent entirely (it would always no-op).
    let leftmost = tab_menu_rows(3, 0, false);
    assert!(!leftmost[4].enabled);
    assert!(leftmost[5].enabled);
    let rightmost = tab_menu_rows(3, 2, false);
    assert!(rightmost[4].enabled);
    assert!(!rightmost[5].enabled);
    assert_eq!(rightmost.len(), 8, "Close Tabs to the Right is hidden");
    assert_eq!(
        rightmost.last().map(|row| row.label.as_str()),
        Some("Close Other Tabs")
    );
}

#[test]
fn sync_title_reports_no_change_for_standing_inputs() {
    let mut list = TabList::default();
    let first = list.spawn(false);
    assert!(list.sync_title(
        first,
        Some("standing".to_string()),
        Some(PathBuf::from("/tmp")),
        TabTitleMode::Shell
    ));
    // The same inputs again: nothing moves.
    assert!(!list.sync_title(
        first,
        Some("standing".to_string()),
        Some(PathBuf::from("/tmp")),
        TabTitleMode::Shell
    ));
    // An unknown tab is a no-op.
    assert!(!list.sync_title(TabId(99), Some("x".to_string()), None, TabTitleMode::Shell));
}

// -- helpers ------------------------------------------------------

fn rgb_hsla(r: u8, g: u8, b: u8) -> gpui::Hsla {
    gpui::Rgba {
        r: f32::from(r) / 255.,
        g: f32::from(g) / 255.,
        b: f32::from(b) / 255.,
        a: 1.,
    }
    .into()
}

#[test]
fn running_comm_records_per_tab_and_reads_back_the_last() {
    // The frozen duration's pairing name (command-lifecycle OQ-1):
    // the comm sampled running lands on its own tab alone, so a
    // switch can never pair one tab's name with another's duration.
    let mut tabs = TabList::default();
    let a = tabs.spawn(true);
    let b = tabs.spawn(true);
    assert_eq!(tabs.running_comm(a), None);
    tabs.note_running_comm(a, "cargo".to_string());
    tabs.note_running_comm(b, "vim".to_string());
    assert_eq!(tabs.running_comm(a).as_deref(), Some("cargo"));
    assert_eq!(tabs.running_comm(b).as_deref(), Some("vim"));
    // A re-sample replaces; an unknown id records nothing.
    tabs.note_running_comm(a, "make".to_string());
    assert_eq!(tabs.running_comm(a).as_deref(), Some("make"));
    tabs.note_running_comm(TabId(9999), "ghost".to_string());
    assert_eq!(tabs.running_comm(TabId(9999)), None);
    // A fresh spawn carries no name (a fresh shell runs no command
    // yet) — the pairing never inherits.
    let fresh = tabs.spawn(true);
    assert_eq!(tabs.running_comm(fresh), None);
}

#[test]
fn a_pasted_image_records_its_ledger_row_on_the_pasting_tab() {
    // The paste-ledger funnel (gpui-parity Task 2): the pane's
    // ImagePasted hand-up lands one core PastedImage row on the
    // pasting tab alone — the stamp from the pasted filename, the
    // dimensions from a header-only read of the bytes (None for
    // non-image bytes, the honest floor). An unknown id records
    // nothing; a fresh spawn inherits no rows.
    let mut tabs = TabList::default();
    let a = tabs.spawn(true);
    let b = tabs.spawn(true);
    let path = std::path::PathBuf::from("/tmp/paste-20261115-101010-1.png");
    tabs.note_pasted_image(b, path.clone(), b"not an image");
    let rows = &tabs.records()[1].pasted_images;
    assert_eq!(rows.len(), 1, "the pasting tab alone carries the row");
    assert_eq!(rows[0].path, path);
    assert_eq!(rows[0].stamp, "paste-20261115-101010-1");
    assert_eq!(rows[0].dims, None, "plain bytes carry no dimensions");
    assert!(
        tabs.records()
            .iter()
            .find(|tab| tab.id == a)
            .expect("a stands")
            .pasted_images
            .is_empty(),
        "the neighbor carries nothing"
    );
    tabs.note_pasted_image(TabId(9999), path, b"x");
    assert_eq!(tabs.records()[1].pasted_images.len(), 1);
    let fresh = tabs.spawn(true);
    assert!(
        tabs.records()
            .iter()
            .find(|tab| tab.id == fresh)
            .expect("the fresh tab stands")
            .pasted_images
            .is_empty()
    );
}
