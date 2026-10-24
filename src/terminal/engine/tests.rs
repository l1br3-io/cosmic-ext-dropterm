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

/// Poll the tagged-event channel until the next `Title` event
/// arrives — skipping the wakeups and redraw requests that
/// share the stream — and pin its payload.
fn poll_until_title(rx: &mut mpsc::UnboundedReceiver<(TabId, Event)>, want: &str, what: &str) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        while let Ok((_, event)) = rx.try_recv() {
            if let Event::Title(title) = event {
                assert_eq!(title, want, "{what}");
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for title {want:?}: {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn an_empty_osc_title_report_arrives_as_a_bare_title_event() {
    // The delivery half of the empty-report clear: OSC 0/2 with
    // an empty payload — a TUI's only way to retract a title it
    // set (opencode's Ctrl+C) — reaches the app as
    // `Title("")`, neither swallowed nor turned into
    // `ResetTitle`. The storage policy
    // (`tabs::shell_title_from_report`) is what maps that bare
    // event to the no-report state, so this delivery shape is
    // the contract it stands on.
    use alacritty_terminal::tty::Shell as PtyShell;

    let (tx, mut rx) = mpsc::unbounded_channel();
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

    // One printf, two reports: a set, then the empty clear.
    terminal.input_no_scroll(&b"printf '\\033]0;titled\\007\\033]0;\\007'\r"[..]);
    poll_until_title(&mut rx, "titled", "the set report arrives");
    poll_until_title(&mut rx, "", "the empty clear arrives as a bare Title");
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
fn settling_after_the_grant_applied_records_the_grid_already_held() {
    // The fresh-start hardware shape (2026-10-24 trace): the
    // granted size applies first (the draw resized), and the pin
    // funnel's adoption lands after — but `resize` no-ops on
    // unchanged sizes, so an arm at that point waits for a changed
    // resize that never comes: the settle (and its fresh-prompt
    // latch) never closed, the anchor never adopted, and the tab
    // never suggested. `settle_spawn_grid_now` closes at the grid
    // already held; a settled record still never re-snapshots.
    let mut terminal = spawn_grid_terminal();
    resize_cols(&mut terminal, 45); // the grant applied first
    terminal.settle_spawn_grid_now(); // the funnel's close, after
    assert_eq!(terminal.spawn_grid().0, 45);
    resize_cols(&mut terminal, 70);
    assert_eq!(
        terminal.spawn_grid().0,
        45,
        "settled: later resizes deviate"
    );
    terminal.settle_spawn_grid_now(); // every re-show adoption: no-op
    resize_cols(&mut terminal, 80);
    assert_eq!(
        terminal.spawn_grid().0,
        45,
        "a settled record never re-snapshots"
    );
}

#[test]
fn cell_metric_changes_after_settle_deviates_without_rebaselining() {
    // A zoom press funnels through `set_config`: it recomputes
    // metrics from the live `zoom_adj` and re-fires the cell
    // resize. `update_cell_size` alone measures with the
    // buffer's *current* metrics — the ported zoom arm once
    // called only it, so the band's percent moved while the
    // font stood (the scale-only bug). A settled record must
    // read neither a zoom nor an options change as a
    // re-baseline: the grid segment narrates the deviation, and
    // the metrics' return restores the rest.
    let mut terminal = spawn_grid_terminal();
    terminal.arm_spawn_grid_next_resize();
    resize_cols(&mut terminal, 40);
    let resting = terminal.spawn_grid();

    // The app's zoom sequence: adjust, then reapply *unchanged*
    // options — `set_config` reads the live `zoom_adj`, so the
    // zoom lands with no options edit at all.
    terminal.set_zoom_adj(2);
    terminal.set_config(&TerminalOptions::default(), &Colors::default());
    assert_eq!(terminal.spawn_grid(), resting);
    assert_ne!(terminal.grid().0, 40, "zoomed cells deviate from rest");

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

// --- the ssh stand-in's getter (status-remote spec) ---
//
// The live pins ride the same foreground harness as the comm read:
// a real /bin/sh PTY whose foreground the dup'd master observes.
// The getter's own gate is the pin — only the `ssh` comm reaches a
// cmdline read — so the positive path renames a copy of sleep into
// an `ssh`-comm'd foreground without any network.

#[test]
fn ssh_destination_reads_none_without_inputs() {
    // The fail-open shape, mirrored from the name read: missing
    // inputs read as "nothing to stand in" — the local path stands.
    assert_eq!(foreground_ssh_destination(None, Some(1234)), None);
    let master = fs::File::open("/dev/null").expect("open a non-pty fd");
    assert_eq!(foreground_ssh_destination(Some(&master), None), None);
}

#[test]
fn a_non_ssh_foreground_comm_yields_no_destination() {
    // The getter's gate (status-remote criterion 3): a live
    // non-ssh foreground — here `sleep`, the harness's own child —
    // never reaches the cmdline read, and an idle shell has no
    // foreground to read at all.
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

    assert_eq!(terminal.ssh_destination(), None);

    terminal.input_no_scroll(&b"sleep 30\r"[..]);
    poll_until_process_name(&terminal, Some("sleep"), "the child's comm names it");
    assert_eq!(
        terminal.ssh_destination(),
        None,
        "a non-ssh comm never reads cmdline"
    );

    terminal.input_no_scroll(&b"\x03"[..]);
    poll_until_process_name(
        &terminal,
        None,
        "SIGINT returns the foreground to the shell",
    );
    assert_eq!(terminal.ssh_destination(), None);
}

#[test]
fn a_live_ssh_comm_reads_its_cmdline_destination() {
    // The positive path: a foreground process whose kernel comm is
    // `ssh` — a copy of sleep renamed, so the name is the kernel's
    // own, no network — reaches the cmdline read, and the pure
    // parser's token stands in. argv ["<path>/ssh", "30"] yields
    // the operand "30".
    use alacritty_terminal::tty::Shell as PtyShell;

    let dir = std::env::temp_dir().join(format!("dropterm-ssh-pin-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("make the pin's temp dir");
    let ssh = dir.join("ssh");
    fs::copy("/bin/sleep", &ssh).expect("copy sleep as an ssh-named binary");

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

    let launch = format!("{} 30\r", ssh.display());
    terminal.input_no_scroll(launch.into_bytes());
    poll_until_ssh_destination(&terminal, Some("30"), "the ssh comm's argv token stands in");

    terminal.input_no_scroll(&b"\x03"[..]);
    poll_until_process_name(
        &terminal,
        None,
        "SIGINT returns the foreground to the shell",
    );
    terminal.input_no_scroll(&b"exit\r"[..]);
    poll_until_process_name(&terminal, None, "shell exit reads as no process");
    fs::remove_dir_all(&dir).expect("clean the pin's temp dir");
}

/// Poll a live terminal's ssh stand-in until it reads `want`,
/// bounded at 10s (the foreground harness's own flake guard).
fn poll_until_ssh_destination(terminal: &Terminal, want: Option<&str>, what: &str) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if terminal.ssh_destination().as_deref() == want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the destination to read {want:?}: {what}"
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
fn a_fresh_terminal_suggests_without_pushing_the_prompt_off_the_top() {
    // The hardware report (2026-10-24): a fresh app start's tabs
    // never suggested at all. Two stacked causes, both exercised
    // here: the pin funnel's settle close lands AFTER the granted
    // size is applied (resize no-ops on unchanged sizes, so waiting
    // for the next changed resize waits forever — settle_spawn_grid_
    // now closes at the grid already held), and a fresh prompt sits
    // near the top of an empty grid, where the old bottom-row guard
    // silenced it. No `seq` push, no scrolled output: the prompt
    // stays exactly where the shell drew it, the ring is seeded the
    // documented spawn-time way, and a typed prefix suggests.
    let mut terminal = autosuggest_terminal();
    resize_cols(&mut terminal, 100); // the grant applied first
    terminal.settle_spawn_grid_now(); // the funnel's close, after
    poll_update_until(
        &mut terminal,
        |t| t.suggest.anchored(),
        "the settle prompt anchors at the top of the empty grid",
    );
    terminal.suggest.seed_history(["sleep 30".to_string()]);
    terminal.input_no_scroll(&b"slee"[..]);
    poll_update_until(
        &mut terminal,
        |t| t.suggestion() == Some("p 30"),
        "a prefix at a fresh top-of-grid prompt suggests",
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

// --- the command timer (command-lifecycle spec) ---
//
// The live pins ride the foreground_check harness: a real /bin/sh
// whose foreground edges update() itself observes at its diff site.
// The engine owns the timing beside the diff that feeds it (the
// status-glance lesson), a fresh Terminal carries a fresh timer (the
// respawn reset by construction), and the timestamps always run —
// the autosuggest toggle gates nothing here.

#[test]
fn command_timer_freezes_a_bounded_duration_and_drains_the_latch_once() {
    let mut terminal = autosuggest_terminal();
    // The fresh-terminal shape (the respawn note): nothing running
    // the timer witnessed, nothing frozen, nothing latched.
    let (running, elapsed) = terminal.command_live();
    assert!(!running);
    assert_eq!(elapsed, None);
    assert_eq!(terminal.command_frozen(), None);
    assert_eq!(terminal.take_command_finished(), None);

    // Witness idle first — the honesty input the start edge diffs
    // against.
    poll_update_until(
        &mut terminal,
        |t| !t.command_live().0,
        "the idle shell holds the foreground",
    );

    let started = Instant::now();
    terminal.input_no_scroll(&b"sleep 0.3\r"[..]);
    poll_update_until(
        &mut terminal,
        |t| t.command_live().1.is_some(),
        "the witnessed start runs a live elapsed",
    );
    poll_update_until(
        &mut terminal,
        |t| !t.command_live().0 && t.command_frozen().is_some(),
        "the finish freezes the duration",
    );

    // Bounded window (in-run ruling, recorded in the chunk): the
    // witnessed start lands after the fork and the finish after the
    // exit, so the measured window is the sleep's own 0.3 s plus
    // the finish's observation latency minus the start's — the 50 ms
    // poll cadence can shave ~50 ms per edge. 200 ms floors the
    // run's signal (never 0, never garbage); 5 s caps it; the wall
    // time since Enter bounds it above.
    let duration = terminal.command_frozen().expect("the frozen duration");
    assert!(duration >= std::time::Duration::from_millis(200));
    assert!(duration < std::time::Duration::from_secs(5));
    assert!(duration <= started.elapsed() + std::time::Duration::from_secs(1));

    // The latch drains exactly once; the frozen display keeps
    // serving until the next start.
    assert_eq!(terminal.take_command_finished(), Some(duration));
    assert_eq!(terminal.take_command_finished(), None);
    assert_eq!(terminal.command_frozen(), Some(duration));
}

#[test]
fn command_timer_re_freezes_a_second_command_fresh() {
    let mut terminal = autosuggest_terminal();
    poll_update_until(
        &mut terminal,
        |t| !t.command_live().0,
        "the idle shell holds the foreground",
    );

    terminal.input_no_scroll(&b"sleep 0.3\r"[..]);
    poll_update_until(
        &mut terminal,
        |t| !t.command_live().0 && t.command_frozen().is_some(),
        "the first finish freezes",
    );
    let first = terminal
        .command_frozen()
        .expect("the first frozen duration");
    assert_eq!(terminal.take_command_finished(), Some(first));

    // The next witnessed start clears the freeze and re-arms — the
    // window between proves the second freeze is the second
    // command's own, never the first's survivor.
    terminal.input_no_scroll(&b"sleep 0.3\r"[..]);
    poll_update_until(
        &mut terminal,
        |t| t.command_frozen().is_none() && t.command_live().1.is_some(),
        "the next start clears the freeze and re-arms",
    );
    poll_update_until(
        &mut terminal,
        |t| !t.command_live().0 && t.command_frozen().is_some(),
        "the second finish freezes",
    );
    let second = terminal
        .command_frozen()
        .expect("the second frozen duration");
    assert!(second >= std::time::Duration::from_millis(200));
    assert!(second < std::time::Duration::from_secs(5));
    assert_eq!(terminal.take_command_finished(), Some(second));
    assert_eq!(terminal.take_command_finished(), None);
}

#[test]
fn command_timer_steps_with_autosuggestions_disabled() {
    // Toggle independence: the timer rides the engine's own
    // foreground diff, never suggest's — a disabled kill switch
    // changes nothing about the edges or the freeze.
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
        &TerminalOptions {
            autosuggestions_enabled: false,
            ..TerminalOptions::default()
        },
        &Colors::default(),
    )
    .expect("spawn a /bin/sh test terminal");

    poll_update_until(
        &mut terminal,
        |t| !t.command_live().0,
        "the idle shell holds the foreground",
    );
    terminal.input_no_scroll(&b"sleep 0.3\r"[..]);
    poll_update_until(
        &mut terminal,
        |t| !t.command_live().0 && t.command_frozen().is_some(),
        "the finish freezes with suggestions off",
    );
    let duration = terminal.command_frozen().expect("the frozen duration");
    assert_eq!(terminal.take_command_finished(), Some(duration));
}

#[test]
fn the_path_spawn_gate_reads_spawn_resolved_truth_from_the_options() {
    // Options → terminal → the observation's `paths_eligible` gate
    // (path-suggest spec): an allowlisted shell opens it, a
    // spawned-as-ssh program keeps it shut — whatever shell the pty
    // wraps the command in. The pty here runs /bin/sh for both; only
    // the spawn truth differs.
    use alacritty_terminal::tty::Shell as PtyShell;

    let gate = |spawned_program: &str| {
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
            &TerminalOptions {
                spawned_program: spawned_program.to_owned(),
                ..TerminalOptions::default()
            },
            &Colors::default(),
        )
        .expect("spawn a /bin/sh test terminal");
        terminal.paths_eligible()
    };
    assert!(gate("/bin/bash"), "a spawned bash tab may suggest paths");
    assert!(!gate("ssh"), "a spawned-as-ssh tab never does");
}

// --- the path provider's listing cache (path-suggest) ---

/// A fresh fixture directory under the temp root, unique per name —
/// the spawn.rs fixture convention, std-only.
fn path_fixture(name: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("dropterm-path-cache-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

#[test]
fn the_listing_cache_reads_once_per_parent_not_per_keystroke() {
    let dir = path_fixture("lazy");
    std::fs::write(dir.join("a.txt"), b"").expect("fixture file");
    std::fs::create_dir(dir.join("sub")).expect("fixture directory");
    let prefix = dir.to_string_lossy().into_owned();
    let mut cache = PathListingCache::default();
    let first = cache
        .listing_for(true, Some(&format!("{prefix}/a")), None, None)
        .expect("the first token lists its parent")
        .to_vec();
    assert_eq!(
        first,
        &[("a.txt".to_string(), false), ("sub".to_string(), true)],
        "entries sorted by name, directories flagged"
    );
    // A file created between two tokens in the SAME parent stays
    // invisible: the second call is not a re-read.
    std::fs::write(dir.join("c.new"), b"").expect("fixture file");
    let second = cache
        .listing_for(true, Some(&format!("{prefix}/b")), None, None)
        .expect("the same parent still lists");
    assert_eq!(
        second, first,
        "same parent, cached listing — never per keystroke"
    );
    // A different parent refreshes; coming back re-reads, and c.new
    // is there.
    cache.listing_for(true, Some(&format!("{prefix}/sub/x")), None, None);
    let back = cache
        .listing_for(true, Some(&format!("{prefix}/a")), None, None)
        .expect("the returned parent lists again");
    assert!(back.contains(&("c.new".to_string(), false)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unreadable_directory_yields_an_empty_listing_never_a_stale_one() {
    use std::os::unix::fs::PermissionsExt;

    let dir = path_fixture("unreadable");
    std::fs::write(dir.join("here.txt"), b"").expect("fixture file");
    let other = path_fixture("unreadable-other");
    let prefix = dir.to_string_lossy().into_owned();
    let other_prefix = other.to_string_lossy().into_owned();
    let mut cache = PathListingCache::default();
    let good = cache
        .listing_for(true, Some(&format!("{prefix}/h")), None, None)
        .expect("the readable parent lists")
        .to_vec();
    assert_eq!(good, &[("here.txt".to_string(), false)]);
    // Unreadable now: a later visit — the key changed away and back —
    // reads silence, never the stale list.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000))
        .expect("drop read permission");
    cache.listing_for(true, Some(&format!("{other_prefix}/x")), None, None);
    let reread = cache
        .listing_for(true, Some(&format!("{prefix}/h")), None, None)
        .expect("the now-unreadable parent still yields a listing shape");
    assert!(reread.is_empty(), "unreadable → silence, never stale");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))
        .expect("restore permission for cleanup");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn tilde_rooted_tokens_resolve_against_the_spawn_time_home() {
    let home = path_fixture("home");
    std::fs::write(home.join("notes.md"), b"").expect("fixture file");
    let home_str = home.to_string_lossy().into_owned();
    let mut cache = PathListingCache::default();
    let listing = cache
        .listing_for(true, Some("~/no"), Some(&home_str), None)
        .expect("~ expands through the spawn HOME")
        .to_vec();
    assert_eq!(listing, &[("notes.md".to_string(), false)]);
    // The bare HOME root is the same parent — the cached listing.
    let root = cache
        .listing_for(true, Some("~"), Some(&home_str), None)
        .expect("the bare ~ is the HOME root");
    assert_eq!(root, listing);
    // No HOME: unresolvable — no listing context at all.
    let mut no_home = PathListingCache::default();
    assert_eq!(no_home.listing_for(true, Some("~/no"), None, None), None);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn relative_parents_resolve_against_the_tab_cwd() {
    let base = path_fixture("cwd");
    std::fs::create_dir_all(base.join("src")).expect("fixture directory");
    std::fs::write(base.join("src/main.rs"), b"").expect("fixture file");
    let mut cache = PathListingCache::default();
    let listing = cache
        .listing_for(true, Some("src/ma"), None, Some(&base))
        .expect("a relative parent joins the cwd");
    assert_eq!(listing, &[("main.rs".to_string(), false)]);
    // No cwd: a relative parent is unresolvable — silence.
    let mut no_cwd = PathListingCache::default();
    assert_eq!(no_cwd.listing_for(true, Some("src/ma"), None, None), None);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn closed_gates_and_plain_tokens_never_produce_a_listing() {
    // The spawn gate closed: a perfectly valid token returns None
    // before any resolution — the filesystem is never touched.
    let mut cache = PathListingCache::default();
    let real = format!("{}/a", std::env::temp_dir().to_string_lossy());
    assert_eq!(cache.listing_for(false, Some(&real), None, None), None);
    // No path-shaped word being entered: equally None.
    assert_eq!(cache.listing_for(true, None, None, None), None);
    // Nothing cached behind either.
    assert!(cache.entries.is_empty());
    assert_eq!(cache.key, None);
}

// --- the command catalog's spawn-time snapshot (command-catalog) ---

/// A fresh fixture directory under the temp root, unique per name —
/// the listing-cache fixture convention, std-only.
fn catalog_fixture(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("dropterm-catalog-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

/// Marks `path` executable (0755) — `fs::write` creates 0644, and
/// the catalog walks only executables.
fn mark_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("fixture exec bit");
}

#[test]
fn the_catalog_walks_regular_executables_sorted_and_deduped() {
    use std::os::unix::ffi::OsStrExt;

    let dir = catalog_fixture("walk");
    std::fs::write(dir.join("git"), b"").expect("fixture file");
    mark_executable(&dir.join("git"));
    std::fs::write(dir.join("cargo"), b"").expect("fixture file");
    mark_executable(&dir.join("cargo"));
    // A file without the execute bit never lands in the catalog...
    std::fs::write(dir.join("plain"), b"").expect("fixture file");
    // ...nor does a directory (its 0755 carries the bit; regular-file
    // is the filter), a broken symlink, or a non-UTF-8 name that
    // could never be safely suggested.
    std::fs::create_dir(dir.join("sub")).expect("fixture directory");
    std::os::unix::fs::symlink("nowhere", dir.join("dead")).expect("broken symlink");
    std::fs::write(dir.join(std::ffi::OsStr::from_bytes(b"bad\xffname")), b"")
        .expect("non-UTF-8 fixture name");
    // A symlink to an executable is an executable (the
    // `python3` → `python3.11` shape PATH abuts in).
    std::os::unix::fs::symlink("git", dir.join("gitlink")).expect("symlink");

    let path = dir.to_string_lossy().into_owned();
    assert_eq!(
        command_catalog_from_path(Some(&path)),
        [
            "cargo".to_string(),
            "git".to_string(),
            "gitlink".to_string()
        ],
        "sorted, regular executables only, symlinks followed"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_catalog_dedupes_across_path_directories() {
    // A name in two PATH directories appears once: the set is keyed
    // by basename, case-sensitively, per tab.
    let first = catalog_fixture("dup-one");
    let second = catalog_fixture("dup-two");
    std::fs::write(first.join("dup"), b"").expect("fixture file");
    mark_executable(&first.join("dup"));
    std::fs::write(second.join("dup"), b"").expect("fixture file");
    mark_executable(&second.join("dup"));
    let path = format!("{}:{}", first.to_string_lossy(), second.to_string_lossy());
    assert_eq!(
        command_catalog_from_path(Some(&path)),
        ["dup".to_string()],
        "the duplicate name contributes one catalog entry"
    );
    let _ = std::fs::remove_dir_all(&first);
    let _ = std::fs::remove_dir_all(&second);
}

#[test]
fn a_missing_path_directory_is_skipped_silently() {
    // The walk never guesses and never fails: a PATH entry that does
    // not exist contributes nothing, and the directories around it
    // still walk.
    let dir = catalog_fixture("missing");
    std::fs::write(dir.join("only"), b"").expect("fixture file");
    mark_executable(&dir.join("only"));
    let missing = dir.join("does-not-exist");
    let path = format!("{}:{}", missing.to_string_lossy(), dir.to_string_lossy());
    assert_eq!(
        command_catalog_from_path(Some(&path)),
        ["only".to_string()],
        "the missing directory contributes nothing"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_path_at_all_is_the_empty_catalog() {
    // `None` is no PATH anywhere (the options default); `""` is an
    // empty PATH value. Both are silence — never a guess.
    assert!(command_catalog_from_path(None).is_empty());
    assert!(command_catalog_from_path(Some("")).is_empty());
}

// --- the history seed's one-time read (history-seed) ---

#[test]
fn the_history_seed_reads_the_shells_default_file_under_home() {
    let home = catalog_fixture("histfile");
    std::fs::write(home.join(".bash_history"), "old\n#1234\ngit status\n")
        .expect("fixture history");
    assert_eq!(
        shell_history_seed("bash", Some(home.to_string_lossy().as_ref())),
        ["old".to_string(), "git status".to_string()],
        "the default location under HOME, parsed by the shell's format"
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn the_history_seed_silences_on_unknown_shell_no_home_or_missing_file() {
    // Never a guess: a shell without a standard history file, no
    // HOME to join onto, or a file that does not read — all empty.
    let home = catalog_fixture("histfile-silent");
    assert!(shell_history_seed("ssh", Some(home.to_string_lossy().as_ref())).is_empty());
    assert!(shell_history_seed("bash", None).is_empty());
    assert!(
        shell_history_seed("bash", Some(home.join("absent").to_string_lossy().as_ref())).is_empty()
    );
    let _ = std::fs::remove_dir_all(&home);
}

// --- the scrolled-back getter (status-scrolled spec) ---
//
// The band's chip reads `display_offset` — state only the grid
// owns — saturated into `u16` under the same term lock the grid
// and scrollbar reads take. Pinned live: a fresh terminal sits at
// the bottom, a scroll up counts its lines, Bottom returns to
// zero (the click funnel the chip routes through).

#[test]
fn scrolled_back_lines_reads_the_display_offset() {
    let mut terminal = autosuggest_terminal();
    // Construction: no history, the viewport at the bottom.
    assert_eq!(terminal.scrolled_back_lines(), 0);

    // Build scrollback (40 numbered lines on the 24-row grid),
    // then scroll up five — the same scroll_display call the
    // wheel's Delta(lines) funnels through.
    terminal.input_no_scroll(&b"seq 1 40\r"[..]);
    poll_update_until(
        &mut terminal,
        |t| {
            let term = t.term.lock();
            term.grid().history_size() > 5
        },
        "the seq output builds scrollback",
    );
    terminal.scroll(TerminalScroll::Delta(5));
    assert_eq!(terminal.scrolled_back_lines(), 5);

    // The chip's own click funnel: Bottom clears the offset.
    terminal.scroll(TerminalScroll::Bottom);
    assert_eq!(terminal.scrolled_back_lines(), 0);
}

// The caret is painted by the buffer build — the cursor cell's
// fg/bg swap keys on the engine's focus flag — so the flag
// flipping must arm a rebuild (the fix for the caret-less show:
// a re-shown surface painted typed text but no cursor until
// unrelated pty output happened to rebuild the lines, and a
// non-focus-reporting shell like bash never emits any).

#[test]
fn a_real_focus_change_arms_the_buffer_rebuild_and_a_repeat_does_not() {
    let mut terminal = spawn_grid_terminal();
    // Construction leaves the engine focused; drain the spawn
    // churn's arming so the assertions read only set_focused.
    terminal.needs_update = false;

    terminal.set_focused(false);
    assert!(
        terminal.needs_update,
        "a real focus change must arm the rebuild that repaints the caret"
    );

    terminal.needs_update = false;
    terminal.set_focused(false);
    assert!(
        !terminal.needs_update,
        "a repeated focus report changes nothing and arms nothing"
    );

    terminal.set_focused(true);
    assert!(terminal.needs_update, "flipping back is a real change");
}
