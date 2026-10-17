// SPDX-License-Identifier: GPL-3.0-only

//! The engine's unit pins — the foreground-check harness (live-PTY), the
//! grid flooring, the spawn-grid settle, and the autosuggest live
//! pins (the `foreground_check` precedent).

use super::*;

#[test]
fn tagged_event_proxy_stamps_events_with_tab_id() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let proxy = TaggedEventProxy {
        tab_id: TabId(7),
        tx,
    };

    proxy.send_event(Event::Wakeup);

    let (tab_id, event) = rx.try_recv().expect("tagged event should arrive");
    assert_eq!(tab_id, TabId(7));
    assert!(matches!(event, Event::Wakeup));
}

#[test]
fn foreground_check_reads_false_without_a_master_handle() {
    // Fail-open pin (criterion 7): no dup'd fd → the check can
    // never trap a tab behind its own confirmation.
    assert!(!foreground_child_running(None, Some(1234)));
}

#[test]
fn foreground_check_reads_false_without_a_shell_pid() {
    // Fail-open pin (criterion 7): nothing to compare the
    // foreground pgrp against reads as "no process".
    let master = fs::File::open("/dev/null").expect("open a non-pty fd");
    assert!(!foreground_child_running(Some(&master), None));
}

#[test]
fn foreground_check_reads_false_without_either_input() {
    assert!(!foreground_child_running(None, None));
}

#[test]
fn foreground_check_tracks_a_live_pty_from_idle_to_child_and_back() {
    // The heaviest test in this module — the second in the
    // codebase to construct a real Terminal (a real /bin/sh PTY) —
    // because the point under test is exactly the kernel pty
    // state the dup'd master fd observes: an idle shell holds the
    // foreground itself (criterion 2), a foreground child takes
    // it, and both SIGINT and the shell's own exit hand it back
    // or take it away (detection must agree with the never-gated
    // TermEvent::Exit close).
    use alacritty_terminal::tty::Shell as PtyShell;

    let (tx, _rx) = mpsc::unbounded_channel();
    let terminal = Terminal::new(
        TabId(0),
        tx,
        Config::default(),
        &Options {
            shell: Some(PtyShell::new("/bin/sh".to_owned(), Vec::new())),
            drain_on_exit: true,
            ..Options::default()
        },
        &TerminalOptions::default(),
        &Colors::default(),
    )
    .expect("spawn a /bin/sh test terminal");

    // Idle at the prompt: the shell's own pgrp holds the foreground.
    assert!(!terminal.foreground_child_running());

    // A foreground child (interactive dash job-controls it into
    // its own pgrp) takes the tty's foreground.
    terminal.input_no_scroll(&b"sleep 30\r"[..]);
    poll_until_foreground(&terminal, true, "foreground child takes the foreground");

    // SIGINT returns the foreground to the shell.
    terminal.input_no_scroll(&b"\x03"[..]);
    poll_until_foreground(
        &terminal,
        false,
        "SIGINT returns the foreground to the shell",
    );

    // The shell's own exit leaves nothing to confirm.
    terminal.input_no_scroll(&b"exit\r"[..]);
    poll_until_foreground(&terminal, false, "shell exit reads as no process");
}

#[test]
fn grid_floors_the_live_pixel_size_into_cells() {
    use alacritty_terminal::tty::Shell as PtyShell;

    let (tx, _rx) = mpsc::unbounded_channel();
    let mut terminal = Terminal::new(
        TabId(0),
        tx,
        Config::default(),
        &Options {
            shell: Some(PtyShell::new("/bin/sh".to_owned(), Vec::new())),
            drain_on_exit: true,
            ..Options::default()
        },
        &TerminalOptions::default(),
        &Colors::default(),
    )
    .expect("spawn a /bin/sh test terminal");

    // The constructor sizes the default grid at 80x24 cells.
    assert_eq!(terminal.grid(), (80, 24));

    // Non-whole cells floor: ten columns' worth of pixels plus
    // nine-tenths of a column is still ten columns.
    let cell = terminal.size().cell_width;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "px under a u32 ceiling, far below u32::MAX"
    )]
    let width = (cell * 10.9).ceil() as u32;
    terminal.resize(width, terminal.size().height);
    assert_eq!(terminal.grid().0, 10);

    // The one-cell minimum holds: a zero resize clamps before
    // the floor-division ever sees a sub-cell size.
    terminal.resize(0, 0);
    assert_eq!(terminal.grid(), (1, 1));
}

// --- the spawn-grid settle (status-glance criterion 1) ---
//
// The band's grid segment reports only a resize away from the
// grid the tab started at, and a fresh terminal's first resizes
// are not that: the construction default, an opening surface's
// measuring stretch, and the granted-size stream all land before
// the resting grid exists. The settle state machine below
// records the resting grid and freezes it — engine-owned, per
// the 2026-10-11 ruling.

fn spawn_grid_terminal() -> Terminal {
    use alacritty_terminal::tty::Shell as PtyShell;
    let (tx, _rx) = mpsc::unbounded_channel();
    Terminal::new(
        TabId(0),
        tx,
        Config::default(),
        &Options {
            shell: Some(PtyShell::new("/bin/sh".to_owned(), Vec::new())),
            drain_on_exit: true,
            ..Options::default()
        },
        &TerminalOptions::default(),
        &Colors::default(),
    )
    .expect("spawn a /bin/sh test terminal")
}

/// Resize to exactly `cols` cells wide, keeping the current
/// height — the whole-cell multiple the floor division lands on.
fn resize_cols(terminal: &mut Terminal, cols: u32) {
    let cell = terminal.size().cell_width;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "test col counts and their px products are far below both u32::MAX and f32's mantissa"
    )]
    let width = (cell * cols as f32).ceil() as u32;
    let height = terminal.size().height;
    terminal.resize(width, height);
}

#[test]
fn a_fresh_terminal_carries_the_default_grid_as_its_spawn_grid() {
    let terminal = spawn_grid_terminal();
    assert_eq!(terminal.grid(), (80, 24));
    assert_eq!(terminal.spawn_grid(), (80, 24));
}

#[test]
fn arming_makes_the_next_resize_record_the_resting_grid_and_freeze() {
    // The shown-surface spawn: one settling resize to the
    // resting grid, then the record is frozen — later resizes
    // are the deviations the band reports, and resizing back
    // to the resting grid reads as returned (live == record).
    let mut terminal = spawn_grid_terminal();
    terminal.arm_spawn_grid_next_resize();
    resize_cols(&mut terminal, 40);
    assert_eq!(terminal.spawn_grid().0, 40);
    resize_cols(&mut terminal, 60);
    assert_eq!(
        terminal.spawn_grid().0,
        40,
        "a settled record never follows a later resize"
    );
    resize_cols(&mut terminal, 40);
    assert_eq!(terminal.spawn_grid().0, 40);
    assert_eq!(
        terminal.grid().0,
        40,
        "returning to rest reads as un-resized"
    );
}

#[test]
fn opening_churn_rebaselines_until_the_adoption_arms() {
    // The startup spawn: the measuring stretch and the granted
    // stream both land before any adoption — each re-baselines
    // (churn, not deviation) — and only the resize after the
    // adoption's arm freezes the record.
    let mut terminal = spawn_grid_terminal();
    resize_cols(&mut terminal, 120);
    assert_eq!(terminal.spawn_grid().0, 120, "the stretch re-baselines");
    resize_cols(&mut terminal, 90);
    assert_eq!(terminal.spawn_grid().0, 90, "the grant re-baselines");
    terminal.arm_spawn_grid_next_resize();
    resize_cols(&mut terminal, 45);
    assert_eq!(terminal.spawn_grid().0, 45);
    resize_cols(&mut terminal, 70);
    assert_eq!(
        terminal.spawn_grid().0,
        45,
        "the record froze at the resting grid"
    );
}

#[test]
fn arming_a_settled_spawn_grid_is_a_no_op() {
    // Every re-show runs an adoption; one must never resurrect
    // or re-snapshot a record that already settled — a geometry
    // change made while hidden stays a visible deviation.
    let mut terminal = spawn_grid_terminal();
    terminal.arm_spawn_grid_next_resize();
    resize_cols(&mut terminal, 40);
    terminal.arm_spawn_grid_next_resize();
    resize_cols(&mut terminal, 55);
    assert_eq!(
        terminal.spawn_grid().0,
        40,
        "the second arm changes nothing"
    );
}

#[test]
fn cell_metric_changes_after_settle_deviates_without_rebaselining() {
    // The zoom path re-fires `resize` through `update_cell_size`
    // — scale-only today: `zoom_adj` moves the band's percent,
    // not the cells — and a real metrics change (a profile's
    // font size through `set_config`) re-fires it with new
    // cells. A settled record must read neither as a
    // re-baseline: the grid segment narrates the deviation, and
    // the metrics' return restores the rest.
    let mut terminal = spawn_grid_terminal();
    terminal.arm_spawn_grid_next_resize();
    resize_cols(&mut terminal, 40);
    let resting = terminal.spawn_grid();

    terminal.set_zoom_adj(2);
    terminal.update_cell_size();
    assert_eq!(terminal.spawn_grid(), resting);
    assert_eq!(terminal.grid().0, 40, "zoom is scale-only: cells stand");

    let enlarged = TerminalOptions {
        font_size: 24,
        ..TerminalOptions::default()
    };
    terminal.set_config(&enlarged, &Colors::default());
    assert_eq!(terminal.spawn_grid(), resting);
    assert_ne!(terminal.grid().0, 40, "bigger cells deviate from rest");

    terminal.set_zoom_adj(0);
    terminal.set_config(&TerminalOptions::default(), &Colors::default());
    assert_eq!(
        terminal.grid().0,
        40,
        "the metrics' return restores the rest"
    );
}

/// Poll a live terminal's foreground check until it reads `want`,
/// bounded at 10s (the chunk's flake guard).
fn poll_until_foreground(terminal: &Terminal, want: bool, what: &str) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if terminal.foreground_child_running() == want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for detection to read {want}: {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn process_name_reads_none_without_inputs() {
    // The fail-open shape, mirrored from the bool check: missing
    // inputs read as "nothing to name" — never a fake process.
    assert_eq!(foreground_process_name(None, Some(1234)), None);
    let master = fs::File::open("/dev/null").expect("open a non-pty fd");
    assert_eq!(foreground_process_name(Some(&master), None), None);
}

#[test]
fn process_name_tracks_a_live_pty_from_idle_to_child_and_back() {
    // The name variant of the bool check's live-pty pin: an idle
    // shell has nothing to name, a foreground child is named by
    // its own comm (the kernel's name for `sleep` is "sleep"),
    // and SIGINT hands the foreground back.
    use alacritty_terminal::tty::Shell as PtyShell;

    let (tx, _rx) = mpsc::unbounded_channel();
    let terminal = Terminal::new(
        TabId(0),
        tx,
        Config::default(),
        &Options {
            shell: Some(PtyShell::new("/bin/sh".to_owned(), Vec::new())),
            drain_on_exit: true,
            ..Options::default()
        },
        &TerminalOptions::default(),
        &Colors::default(),
    )
    .expect("spawn a /bin/sh test terminal");

    assert_eq!(terminal.foreground_process_name(), None);

    terminal.input_no_scroll(&b"sleep 30\r"[..]);
    poll_until_process_name(&terminal, Some("sleep"), "the child's comm names it");

    terminal.input_no_scroll(&b"\x03"[..]);
    poll_until_process_name(
        &terminal,
        None,
        "SIGINT returns the foreground to the shell",
    );

    terminal.input_no_scroll(&b"exit\r"[..]);
    poll_until_process_name(&terminal, None, "shell exit reads as no process");
}

/// Poll a live terminal's process name until it reads `want`,
/// bounded at 10s (the bool check's own flake guard).
fn poll_until_process_name(terminal: &Terminal, want: Option<&str>, what: &str) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if terminal.foreground_process_name().as_deref() == want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the name to read {want:?}: {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

// --- autosuggest (autosuggest spec) ---
//
// The live pins ride the foreground_check harness: a real
// /bin/sh whose prompt state the dup'd master observes. The
// engine owns the state (the status-glance lesson), and a fresh
// Terminal carries a fresh Suggest — the respawn reset by
// construction: nothing recorded, nothing anchored, nothing
// showing.

fn autosuggest_terminal() -> Terminal {
    use alacritty_terminal::tty::Shell as PtyShell;
    let (tx, _rx) = mpsc::unbounded_channel();
    Terminal::new(
        TabId(0),
        tx,
        Config::default(),
        &Options {
            shell: Some(PtyShell::new("/bin/sh".to_owned(), Vec::new())),
            drain_on_exit: true,
            ..Options::default()
        },
        &TerminalOptions::default(),
        &Colors::default(),
    )
    .expect("spawn a /bin/sh test terminal")
}

/// Drive `update()` until `want` holds — the echo and settle
/// races bounded at 10s (the `foreground_check` flake guard). Every
/// look is a full engine update, exactly the cadence the app's
/// Wakeup loop produces.
fn poll_update_until<F>(terminal: &mut Terminal, want: F, what: &str)
where
    F: Fn(&Terminal) -> bool,
{
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        terminal.update();
        if want(terminal) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the autosuggest state: {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// The buffer's last line text — where the echoed prompt line
/// lands.
fn bottom_line_text(terminal: &Terminal) -> String {
    terminal.with_buffer(|buffer| {
        buffer
            .lines
            .last()
            .map(|line| line.text().to_string())
            .unwrap_or_default()
    })
}

#[test]
fn autosuggest_records_a_run_command_and_suggests_its_prefix() {
    let mut terminal = autosuggest_terminal();
    // The fresh-terminal shape (the respawn note): nothing
    // showing, so nothing to accept.
    assert_eq!(terminal.suggestion(), None);

    // The settle path: arm the close, land the settling resize,
    // and let the latch hold the fresh-prompt mark until the
    // shell's own prompt appears.
    terminal.arm_spawn_grid_next_resize();
    resize_cols(&mut terminal, 100);
    poll_update_until(
        &mut terminal,
        |t| t.suggest.anchored(),
        "the settle prompt anchors",
    );

    // Push the prompt to the bottom row (the showing guards'
    // geometry), then record a command whose foreground state
    // observably lasts.
    terminal.input_no_scroll(&b"seq 1 40\r"[..]);
    // Wait for the prompt to reach the bottom row before typing:
    // type-ahead sent while a child runs is swallowed at its exit
    // (the shell flushes its queue), so the echo poll below needs
    // the prompt already waiting for input.
    poll_update_until(
        &mut terminal,
        |t| {
            bottom_line_text(t)
                .trim_start_matches('\u{2066}')
                .starts_with('$')
        },
        "the prompt lands on the bottom row",
    );
    // Type before Enter, the way real keystrokes arrive: the
    // capture refreshes on an idle update that observes the
    // echoed line, and only then does the run transition commit
    // it (one burst would echo+exec inside a single update, and
    // the commit is capture-based by design).
    terminal.input_no_scroll(&b"sleep 30"[..]);
    poll_update_until(
        &mut terminal,
        |t| bottom_line_text(t).contains("sleep 30"),
        "the typed line echoes",
    );
    terminal.input_no_scroll(&b"\r"[..]);
    poll_update_until(
        &mut terminal,
        Terminal::foreground_child_running,
        "the child takes the foreground",
    );
    terminal.input_no_scroll(&b"\x03"[..]);
    poll_update_until(
        &mut terminal,
        |t| !t.foreground_child_running(),
        "SIGINT returns the prompt",
    );

    // Typing a prefix suggests the recorded command's remainder —
    // the state-level pin (Task 3's overlay renders this).
    terminal.input_no_scroll(&b"slee"[..]);
    poll_update_until(
        &mut terminal,
        |t| t.suggestion() == Some("p 30"),
        "the prefix suggests the remainder",
    );
}

#[test]
fn autosuggest_accept_full_lands_the_suffix_in_the_grid() {
    let mut terminal = autosuggest_terminal();
    terminal.arm_spawn_grid_next_resize();
    resize_cols(&mut terminal, 100);
    poll_update_until(
        &mut terminal,
        |t| t.suggest.anchored(),
        "the settle prompt anchors",
    );
    terminal.input_no_scroll(&b"seq 1 40\r"[..]);
    // Wait for the prompt to reach the bottom row before typing:
    // type-ahead sent while a child runs is swallowed at its exit
    // (the shell flushes its queue), so the echo poll below needs
    // the prompt already waiting for input.
    poll_update_until(
        &mut terminal,
        |t| {
            bottom_line_text(t)
                .trim_start_matches('\u{2066}')
                .starts_with('$')
        },
        "the prompt lands on the bottom row",
    );
    // Type before Enter, the way real keystrokes arrive: the
    // capture refreshes on an idle update that observes the
    // echoed line, and only then does the run transition commit
    // it (one burst would echo+exec inside a single update, and
    // the commit is capture-based by design).
    terminal.input_no_scroll(&b"sleep 30"[..]);
    poll_update_until(
        &mut terminal,
        |t| bottom_line_text(t).contains("sleep 30"),
        "the typed line echoes",
    );
    terminal.input_no_scroll(&b"\r"[..]);
    poll_update_until(
        &mut terminal,
        Terminal::foreground_child_running,
        "the child takes the foreground",
    );
    terminal.input_no_scroll(&b"\x03"[..]);
    poll_update_until(
        &mut terminal,
        |t| !t.foreground_child_running(),
        "SIGINT returns the prompt",
    );
    terminal.input_no_scroll(&b"slee"[..]);
    poll_update_until(
        &mut terminal,
        |t| t.suggestion() == Some("p 30"),
        "the prefix suggests the remainder",
    );

    terminal.accept_suggestion(AcceptKind::Full);
    poll_update_until(
        &mut terminal,
        |t| bottom_line_text(t).contains("sleep 30"),
        "the accepted suffix echoes into the line",
    );
    // Accepted, never executed: no child took the foreground, and
    // the now-complete line suggests nothing further.
    assert!(!terminal.foreground_child_running());
    assert_eq!(terminal.suggestion(), None);
}

#[test]
fn autosuggest_accept_word_lands_one_word_and_resuggests() {
    let mut terminal = autosuggest_terminal();
    terminal.arm_spawn_grid_next_resize();
    resize_cols(&mut terminal, 100);
    poll_update_until(
        &mut terminal,
        |t| t.suggest.anchored(),
        "the settle prompt anchors",
    );
    terminal.input_no_scroll(&b"seq 1 40\r"[..]);
    // Wait for the prompt to reach the bottom row before typing:
    // type-ahead sent while a child runs is swallowed at its exit
    // (the shell flushes its queue), so the echo poll below needs
    // the prompt already waiting for input.
    poll_update_until(
        &mut terminal,
        |t| {
            bottom_line_text(t)
                .trim_start_matches('\u{2066}')
                .starts_with('$')
        },
        "the prompt lands on the bottom row",
    );
    // Type before Enter, the way real keystrokes arrive: the
    // capture refreshes on an idle update that observes the
    // echoed line, and only then does the run transition commit
    // it (one burst would echo+exec inside a single update, and
    // the commit is capture-based by design).
    terminal.input_no_scroll(&b"sleep 30"[..]);
    poll_update_until(
        &mut terminal,
        |t| bottom_line_text(t).contains("sleep 30"),
        "the typed line echoes",
    );
    terminal.input_no_scroll(&b"\r"[..]);
    poll_update_until(
        &mut terminal,
        Terminal::foreground_child_running,
        "the child takes the foreground",
    );
    terminal.input_no_scroll(&b"\x03"[..]);
    poll_update_until(
        &mut terminal,
        |t| !t.foreground_child_running(),
        "SIGINT returns the prompt",
    );
    terminal.input_no_scroll(&b"sl"[..]);
    poll_update_until(
        &mut terminal,
        |t| t.suggestion() == Some("eep 30"),
        "the prefix suggests the remainder",
    );

    // One fish-shaped word lands; the remainder stays suggested —
    // ghost text only, never the grid.
    terminal.accept_suggestion(AcceptKind::Word);
    poll_update_until(
        &mut terminal,
        |t| t.suggestion() == Some(" 30"),
        "the remainder re-suggests after the word accept",
    );
    let line = bottom_line_text(&terminal);
    assert!(line.contains("sleep"), "the word echoes: {line:?}");
    assert!(
        !line.contains("sleep 30"),
        "the remainder never lands: {line:?}"
    );
    assert!(!terminal.foreground_child_running());
}

#[test]
fn autosuggest_accept_with_nothing_showing_writes_nothing() {
    // The inert guard: with nothing showing, an accept writes no
    // bytes — the line the user sees never moves.
    let mut terminal = autosuggest_terminal();
    terminal.arm_spawn_grid_next_resize();
    resize_cols(&mut terminal, 100);
    poll_update_until(
        &mut terminal,
        |t| t.suggest.anchored(),
        "the settle prompt anchors",
    );

    assert_eq!(terminal.suggestion(), None);
    let before = bottom_line_text(&terminal);
    terminal.accept_suggestion(AcceptKind::Full);
    terminal.update();
    assert_eq!(
        bottom_line_text(&terminal),
        before,
        "no bytes reached the shell"
    );
    assert_eq!(terminal.suggestion(), None);
}
