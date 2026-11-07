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

/// Poll the tagged-event channel until the next `CurrentDirectory`
/// event arrives — skipping the wakeups that share the stream — and
/// pin its payload.
fn poll_until_cwd(rx: &mut mpsc::UnboundedReceiver<(TabId, Event)>, want: &str, what: &str) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        while let Ok((_, event)) = rx.try_recv() {
            if let Event::CurrentDirectory(url) = event {
                assert_eq!(url, want, "{what}");
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for cwd {want:?}: {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn an_osc7_report_arrives_as_a_current_directory_event() {
    // The delivery half of the seat (osc7-cwd spec): a shell-emitted
    // OSC 7 reaches the app as `CurrentDirectory(url)` through the
    // same tagged stream the title rides — the fork arm's
    // end-to-end pin on the pinned revs.
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
    .expect("spawn a /bin/sh cwd test terminal");

    terminal.input_no_scroll(&b"printf '\\033]7;file://workstation/home\\007'\r"[..]);
    poll_until_cwd(
        &mut rx,
        "file://workstation/home",
        "the OSC 7 report arrives",
    );
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

// --- the generator runner's spawn contract (suggest-generators) ---

#[test]
fn generator_command_pins_the_spawn_contract() {
    // The contract in full: argv[0] is the absolutely-resolved
    // binary (never an ambient PATH lookup at exec time), the tab's
    // /proc cwd, env_clear with HOME (the spawn-time truth the pty
    // exec inherited) and LANG — pinned to C so git's noise markers
    // are the English bytes the post-filter pins, whatever the
    // desktop locale would have localized them into. Nothing else
    // of DropTerm's environment inherits.
    let command = generator_command(
        GIT_BRANCHES,
        Path::new("/opt/git/bin/git"),
        Path::new("/home/jpt/repo"),
        Some("/home/jpt"),
    );
    assert_eq!(command.get_program(), "/opt/git/bin/git");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        [
            "--no-optional-locks",
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)|%(objectname:short)|%(subject)",
            "refs/heads/",
        ]
    );
    assert_eq!(
        command.get_current_dir(),
        Some(Path::new("/home/jpt/repo")),
        "the tab's /proc cwd is the spawn's working directory"
    );
    let mut env: Vec<String> = command
        .get_envs()
        .map(|(key, value)| {
            format!(
                "{}={}",
                key.to_string_lossy(),
                value.map(|v| v.to_string_lossy()).unwrap_or_default()
            )
        })
        .collect();
    env.sort();
    assert_eq!(
        env,
        vec!["HOME=/home/jpt".to_string(), "LANG=C".to_string()]
    );
}

#[test]
fn generator_command_inherits_nothing_but_its_two_inputs() {
    // env(1) prints the environment it ran with — the scrub's own
    // behavioral proof, beyond the explicitly-set pair: whatever
    // DropTerm's process environment carries (a PATH, a USER, a
    // desktop locale), the spawned generator sees none of it.
    let mut command = generator_command(
        &["env"],
        Path::new("/usr/bin/env"),
        Path::new("/"),
        Some("/home/jpt"),
    );
    let output = command.output().expect("env(1) runs");
    let mut printed: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    printed.sort();
    assert_eq!(
        printed,
        vec!["HOME=/home/jpt".to_string(), "LANG=C".to_string()]
    );
}

#[test]
fn generator_command_without_home_sets_only_lang() {
    // A tab with no spawn-time HOME (the options default): the
    // scrubbed env carries LANG alone — git reads no user config it
    // cannot find, and the spawn stays reproducible.
    let command = generator_command(
        GIT_FILES,
        Path::new("/usr/bin/git"),
        Path::new("/repo"),
        None,
    );
    let env: Vec<String> = command
        .get_envs()
        .map(|(key, _)| key.to_string_lossy().into_owned())
        .collect();
    assert_eq!(env, vec!["LANG".to_string()]);
}

#[test]
fn the_generator_binary_resolves_against_the_spawn_path() {
    // First-hit resolution over the spawn-time snapshot — the
    // catalog walk's own executable check (regular file, exec bit,
    // symlinks followed), never the ambient PATH a mid-session
    // re-export could move. No snapshot or no git anywhere on it is
    // silence.
    let dir = catalog_fixture("gen-binary");
    std::fs::write(dir.join("git"), b"").expect("fixture file");
    mark_executable(&dir.join("git"));
    let path = dir.to_string_lossy().into_owned();
    assert_eq!(
        resolve_generator_binary(Some(&path), "git"),
        Some(dir.join("git")),
        "the first executable hit on the snapshot wins"
    );
    assert_eq!(resolve_generator_binary(None, "git"), None);
    assert_eq!(
        resolve_generator_binary(Some("/nonexistent-anywhere"), "git"),
        None
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// --- the generator runner: deadline + stale-while-revalidate (suggest-generators, Task 3) ---

/// Resolves the test environment's own git — `None` skips the
/// git-backed pins (the chunk's skip-if-no-git marker: the suite
/// stays green where git is absent).
fn test_git() -> Option<std::path::PathBuf> {
    resolve_generator_binary(std::env::var("PATH").ok().as_deref(), "git")
}

/// A git fixture repository: fresh directory, then each setup step
/// run through the test's own environment (setup, not the
/// generator contract).
fn git_fixture(name: &str, setup: &[&str]) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("dropterm-generator-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    let git = test_git().expect("git present — the skip guard ran");
    for step in setup {
        let status = std::process::Command::new(&git)
            .args(step.split_whitespace())
            .current_dir(&dir)
            .status()
            .expect("fixture git step runs");
        assert!(status.success(), "fixture step failed: {step}");
    }
    dir
}

/// An eligible runner over the test environment's own PATH, waking
/// a drained channel nobody reads.
fn test_runner() -> GeneratorRunner {
    GeneratorRunner::new(
        "/bin/sh",
        std::env::var("PATH").ok().as_deref(),
        None,
        true,
        TabId(0),
        mpsc::unbounded_channel().0,
    )
}

/// Polls the runner's cached sets for `dir` until they exist —
/// bounded at 10s (the fetch deadline's own cap plus slack).
fn poll_generator_sets(runner: &GeneratorRunner, dir: &Path) -> GenCached {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(sets) = runner.sets_for(Some(dir)) {
            return sets;
        }
        assert!(Instant::now() < deadline, "generator fetch never landed");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn a_two_branch_repo_yields_both_names_after_revalidation() {
    let Some(_git) = test_git() else {
        return;
    };
    let dir = git_fixture(
        "branches",
        &[
            "init -b main",
            "-c user.email=t@t -c user.name=t commit --allow-empty -m x",
            "branch feature/x",
        ],
    );
    let runner = test_runner();
    assert!(
        runner.sets_for(Some(&dir)).is_none(),
        "nothing cached before the first trigger"
    );
    runner.revalidate(Some(&dir));
    // The SWR bookkeeping: all three argv keys in flight the moment
    // the trigger returns, none left when the fetches land.
    {
        let state = runner.lock_generator_state();
        assert_eq!(state.in_flight.len(), 3, "one worker per argv");
    }
    let sets = poll_generator_sets(&runner, &dir);
    {
        let state = runner.lock_generator_state();
        assert!(state.in_flight.is_empty(), "landed workers retire");
        assert_eq!(state.cache.len(), 3, "one cache entry per argv");
    }
    assert!(sets.branches.iter().any(|name| name == "main"));
    assert!(sets.branches.iter().any(|name| name == "feature/x"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_repo_and_a_non_repo_are_silence() {
    let Some(_git) = test_git() else {
        return;
    };
    let empty = git_fixture("empty", &["init -b main"]);
    let runner = test_runner();
    runner.revalidate(Some(&empty));
    let sets = poll_generator_sets(&runner, &empty);
    assert!(
        sets.branches.is_empty() && sets.tags.is_empty() && sets.files.is_empty(),
        "an unborn repo carries no refs, no tags, no changes"
    );
    let plain = catalog_fixture("gen-nonrepo");
    runner.revalidate(Some(&plain));
    let sets = poll_generator_sets(&runner, &plain);
    assert!(
        sets.branches.is_empty() && sets.tags.is_empty() && sets.files.is_empty(),
        "fatal output degrades to the cached empty set — silence"
    );
    let _ = std::fs::remove_dir_all(&empty);
    let _ = std::fs::remove_dir_all(&plain);
}

#[test]
fn a_slow_generator_times_out_to_silence() {
    // The deadline path itself, through the test's own slow argv
    // (never a shipped constant) at a test-shortened 300ms: a 30s
    // sleep dies at the deadline, its output discarded.
    let mut command = std::process::Command::new("sleep");
    command.arg("30");
    assert!(run_under_deadline(command, Duration::from_millis(300)).is_none());
    // And the clean path for contrast: a fast child's stdout rides
    // home whole.
    let mut command = std::process::Command::new("echo");
    command.arg("main|abc|subject");
    assert_eq!(
        run_under_deadline(command, Duration::from_secs(5)).as_deref(),
        Some("main|abc|subject\n")
    );
}

#[test]
fn sets_for_is_the_spawn_free_hot_path() {
    // The update path's read serves stale-instantly and spawns
    // nothing: no in-flight worker, no cache write, no spawn count
    // — the fact the typing path's pin stands on.
    let runner = test_runner();
    let _ = runner.sets_for(Some(Path::new("/")));
    let state = runner.lock_generator_state();
    assert_eq!(state.spawns, 0);
    assert!(state.in_flight.is_empty());
    assert!(state.cache.is_empty());
}

#[test]
fn an_ineligible_tab_never_spawns() {
    // The standing ssh gate, engine-side: a tab spawned as ssh (or
    // any non-shell program) blocks the trigger before any spawn,
    // whatever the cwd would say.
    let runner = GeneratorRunner::new(
        "ssh",
        std::env::var("PATH").ok().as_deref(),
        None,
        true,
        TabId(0),
        mpsc::unbounded_channel().0,
    );
    runner.revalidate(Some(Path::new("/")));
    let state = runner.lock_generator_state();
    assert_eq!(state.spawns, 0);
    assert!(state.in_flight.is_empty());
}

#[test]
fn a_tab_without_git_on_its_spawn_path_never_spawns() {
    // No resolvable binary is the runner's own silence: the spawn
    // PATH carried no git, so no trigger ever forks.
    let runner = GeneratorRunner::new(
        "/bin/sh",
        Some("/nonexistent-generator-path"),
        None,
        true,
        TabId(0),
        mpsc::unbounded_channel().0,
    );
    runner.revalidate(Some(Path::new("/")));
    let state = runner.lock_generator_state();
    assert_eq!(state.spawns, 0);
    assert!(state.in_flight.is_empty());
}

/// One live terminal spawned inside `dir` with git resolvable on
/// its spawn PATH — the engine-level generator harness: construction
/// pre-warms, updates serve from cache, typing must spawn nothing.
fn generator_terminal(dir: &Path) -> Terminal {
    use alacritty_terminal::tty::Shell as PtyShell;

    let (tx, _rx) = mpsc::unbounded_channel();
    Terminal::new(
        TabId(0),
        tx,
        Config::default(),
        &Options {
            shell: Some(PtyShell::new("/bin/sh".to_owned(), Vec::new())),
            working_directory: Some(dir.to_path_buf()),
            drain_on_exit: true,
            ..Options::default()
        },
        &TerminalOptions {
            spawned_program: "/bin/sh".to_owned(),
            spawn_path: std::env::var("PATH").ok(),
            ..TerminalOptions::default()
        },
        &Colors::default(),
    )
    .expect("spawn a generator test terminal")
}

/// Waits until the construction pre-warm has fully landed — no
/// worker in flight, all three argv cached — bounded at 10s.
fn wait_for_generator_prewarm(terminal: &Terminal) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        // The guard drops before the sleep — held across it, the
        // poller starves the very workers it waits on (Linux's
        // mutex is not fair).
        let (done, spawns) = {
            let state = terminal.generator.lock_generator_state();
            (
                state.in_flight.is_empty() && state.cache.len() == 3,
                state.spawns,
            )
        };
        if done {
            return spawns;
        }
        assert!(Instant::now() < deadline, "pre-warm never landed");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn plain_typing_never_spawns_a_generator() {
    // The hot path's own pin (the standing rule: spawns never on
    // plain typing): the construction pre-warm lands its three, and
    // keystrokes driven through full updates spawn nothing more.
    let Some(_git) = test_git() else {
        return;
    };
    let dir = git_fixture(
        "typing",
        &[
            "init -b main",
            "-c user.email=t@t -c user.name=t commit --allow-empty -m x",
            "branch feature/x",
        ],
    );
    let mut terminal = generator_terminal(&dir);
    let prewarm_spawns = wait_for_generator_prewarm(&terminal);
    assert_eq!(prewarm_spawns, 3, "one spawn per argv at eligibility onset");
    for key in [b"g", b"i", b"t", b" ", b"c", b"h"] {
        terminal.input_no_scroll(key);
        terminal.update();
        std::thread::sleep(Duration::from_millis(10));
    }
    let state = terminal.generator.lock_generator_state();
    assert_eq!(
        state.spawns, prewarm_spawns,
        "plain typing drove updates and spawned nothing"
    );
    terminal.input_no_scroll(&b"\x03"[..]);
    terminal.input_no_scroll(&b"exit\r"[..]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn menu_open_revalidates_the_generator_cache() {
    // The menu's open moment is the ruled trigger: with the pre-warm
    // landed and no worker in flight, a Ctrl+Space open re-fires all
    // three argv — stale served in the rows it collects, the fresh
    // sets landing for the updates that follow.
    let Some(_git) = test_git() else {
        return;
    };
    let dir = git_fixture(
        "menu-open",
        &[
            "init -b main",
            "-c user.email=t@t -c user.name=t commit --allow-empty -m x",
            "branch feature/x",
        ],
    );
    let mut terminal = generator_terminal(&dir);
    let prewarm_spawns = wait_for_generator_prewarm(&terminal);
    terminal.update();
    let _ = terminal.menu_open();
    let state = terminal.generator.lock_generator_state();
    assert_eq!(
        state.spawns,
        prewarm_spawns + 3,
        "the menu's open revalidates every argv for the tab's cwd"
    );
    terminal.input_no_scroll(&b"exit\r"[..]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_disabled_tab_never_spawns() {
    // The kill switch's spawn half: `false` blocks the eligibility
    // onset's pre-warm and the menu's open trigger alike — no
    // worker, no cache, ever.
    let Some(_git) = test_git() else {
        return;
    };
    let dir = git_fixture(
        "disabled",
        &[
            "init -b main",
            "-c user.email=t@t -c user.name=t commit --allow-empty -m x",
            "branch feature/x",
        ],
    );
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut terminal = Terminal::new(
        TabId(0),
        tx,
        Config::default(),
        &Options {
            shell: Some(alacritty_terminal::tty::Shell::new(
                "/bin/sh".to_owned(),
                Vec::new(),
            )),
            working_directory: Some(dir.clone()),
            drain_on_exit: true,
            ..Options::default()
        },
        &TerminalOptions {
            spawned_program: "/bin/sh".to_owned(),
            spawn_path: std::env::var("PATH").ok(),
            autosuggestions_generators_enabled: false,
            ..TerminalOptions::default()
        },
        &Colors::default(),
    )
    .expect("spawn a disabled-generator test terminal");
    std::thread::sleep(Duration::from_millis(300));
    {
        let state = terminal.generator.lock_generator_state();
        assert_eq!(state.spawns, 0, "a disabled tab never pre-warms");
        assert!(state.cache.is_empty());
    }
    terminal.update();
    let _ = terminal.menu_open();
    let state = terminal.generator.lock_generator_state();
    assert_eq!(state.spawns, 0, "the menu's trigger stays blocked too");
    terminal.input_no_scroll(b"exit\r");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_off_flip_serves_nothing_from_cache() {
    // The kill switch's serving half: a warm cache under a live `false`
    // flip reads as nothing — the observation carries no sets, never
    // the stale rows — and the flip silences the triggers with it.
    let Some(_git) = test_git() else {
        return;
    };
    let dir = git_fixture(
        "flip-off",
        &[
            "init -b main",
            "-c user.email=t@t -c user.name=t commit --allow-empty -m x",
            "branch feature/x",
        ],
    );
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut terminal = Terminal::new(
        TabId(0),
        tx,
        Config::default(),
        &Options {
            shell: Some(alacritty_terminal::tty::Shell::new(
                "/bin/sh".to_owned(),
                Vec::new(),
            )),
            working_directory: Some(dir.clone()),
            drain_on_exit: true,
            ..Options::default()
        },
        &TerminalOptions {
            spawned_program: "/bin/sh".to_owned(),
            spawn_path: std::env::var("PATH").ok(),
            ..TerminalOptions::default()
        },
        &Colors::default(),
    )
    .expect("spawn a flip test terminal");
    let prewarm_spawns = wait_for_generator_prewarm(&terminal);
    assert!(
        terminal.generator.sets_for(Some(&dir)).is_some(),
        "the warm cache serves before the flip"
    );
    terminal.set_config(
        &TerminalOptions {
            spawned_program: "/bin/sh".to_owned(),
            spawn_path: std::env::var("PATH").ok(),
            autosuggestions_generators_enabled: false,
            ..TerminalOptions::default()
        },
        &Colors::default(),
    );
    assert!(
        terminal.generator.sets_for(Some(&dir)).is_none(),
        "an off flip blanks the cache read on the echo"
    );
    let _ = terminal.menu_open();
    let state = terminal.generator.lock_generator_state();
    assert_eq!(
        state.spawns, prewarm_spawns,
        "the off flip silences the menu's trigger too"
    );
    terminal.input_no_scroll(b"exit\r");
    let _ = std::fs::remove_dir_all(&dir);
}

// --- the missed-activity latch (status-activity) ---
//
// The latch's transition model, pinned over a real Terminal: set by
// the app's hidden-wakeup arm, held until one of the three user
// funnels clears it. The clear verbs are the funnels themselves, so
// these tests construct terminals exactly like their siblings above.

/// The module's own /bin/sh terminal shape, shared by the
/// latch-funnel tests below.
fn latch_terminal() -> Terminal {
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
    .expect("spawn a /bin/sh latch test terminal")
}

#[test]
fn a_fresh_terminal_carries_no_missed_activity() {
    // Respawned/restored tabs start clean by construction — the
    // latch dies with the terminal that owns it.
    let terminal = latch_terminal();
    assert!(!terminal.missed_activity());
}

#[test]
fn an_armed_latch_holds_across_a_read() {
    // The arm is sticky: reading the fact (every status sampling)
    // never consumes it — only a touch does.
    let mut terminal = latch_terminal();
    terminal.note_missed_activity();
    assert!(terminal.missed_activity());
    assert!(
        terminal.missed_activity(),
        "the read is not a drain; the mark persists"
    );
}

#[test]
fn a_typed_key_through_input_scroll_clears_the_latch() {
    let mut terminal = latch_terminal();
    terminal.note_missed_activity();
    terminal.input_scroll(&b"true\r"[..]);
    assert!(!terminal.missed_activity());
}

#[test]
fn a_paste_clears_the_latch() {
    let mut terminal = latch_terminal();
    terminal.note_missed_activity();
    terminal.paste("echo hi");
    assert!(!terminal.missed_activity());
}

#[test]
fn a_scroll_clears_the_latch() {
    let mut terminal = latch_terminal();
    terminal.note_missed_activity();
    terminal.scroll(TerminalScroll::Bottom);
    assert!(!terminal.missed_activity());
}

#[test]
fn an_internal_write_never_masquerades_as_a_touch() {
    // Engine-side writes (OSC replies, PtyWrite, suggest acceptance)
    // ride `input_no_scroll` directly — the arm must survive them,
    // or the mark would clear itself on the very output that set it.
    let mut terminal = latch_terminal();
    terminal.note_missed_activity();
    terminal.input_no_scroll(&b"\x1b[6n"[..]);
    assert!(terminal.missed_activity());
}

/// A `/bin/sh` terminal for the OSC 7 seat pins — the same shape as
/// [`latch_terminal`]: the report dies with the terminal that owns
/// it, so a respawn starts clean by construction.
fn cwd_report_terminal() -> Terminal {
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
    .expect("spawn a /bin/sh cwd-report test terminal")
}

#[test]
fn a_fresh_terminal_carries_no_reported_cwd() {
    // Respawned/restored tabs start clean by construction — the
    // report dies with the terminal that owns it.
    let terminal = cwd_report_terminal();
    assert_eq!(terminal.reported_cwd(), None);
}

#[test]
fn a_valid_report_lands_parsed_into_host_and_path() {
    let mut terminal = cwd_report_terminal();
    terminal.note_current_directory("file://workstation/home/u");
    let report = terminal.reported_cwd().expect("the report stands");
    assert_eq!(report.host.as_deref(), Some("workstation"));
    assert_eq!(report.path, "/home/u");
}

#[test]
fn an_empty_host_report_reads_as_local() {
    let mut terminal = cwd_report_terminal();
    terminal.note_current_directory("file:///home/u");
    let report = terminal.reported_cwd().expect("the report stands");
    assert_eq!(report.host, None);
    assert_eq!(report.path, "/home/u");
}

#[test]
fn percent_escapes_decode_in_the_path() {
    let mut terminal = cwd_report_terminal();
    terminal.note_current_directory("file://h/home/a%20b%2Fc");
    let report = terminal.reported_cwd().expect("the report stands");
    assert_eq!(report.path, "/home/a b/c");
}

#[test]
fn a_malformed_report_never_replaces_a_standing_one() {
    // Each malformed shape is ignored whole — a mangled report never
    // replaces a live truth.
    let mut terminal = cwd_report_terminal();
    terminal.note_current_directory("file://h/home");
    for url in ["gopher://h/x", "file://h/%zz", "file://h", ""] {
        terminal.note_current_directory(url);
        let report = terminal
            .reported_cwd()
            .expect("the standing report survives");
        assert_eq!(report.path, "/home", "url {url:?} must not disturb it");
    }
}

#[test]
fn a_newer_report_replaces_the_standing_one() {
    let mut terminal = cwd_report_terminal();
    terminal.note_current_directory("file://h/one");
    terminal.note_current_directory("file://h/two");
    let report = terminal.reported_cwd().expect("the newer report stands");
    assert_eq!(report.path, "/two");
}

#[test]
fn reported_cwd_parse_rejects_malformed_urls() {
    let overlong = format!("file://h/{}", "a".repeat(4096));
    for url in [
        "",
        "file://",
        "file://h",
        "https://h/p",
        "file://h/%2",
        "file://h/%zz",
        // A `+` sign is not a hex digit, whatever from_str_radix
        // says — escapes are exactly two hex digits.
        "file://h/%+f",
        // An escape re-admitting a control character is a payload
        // lying about being text — rejected whole.
        "file://h/a%1Bb",
        // A bidirectional override is not a path character.
        "file://h/\u{202E}evil",
        // Past the payload cap — rejected whole, never truncated.
        overlong.as_str(),
    ] {
        assert_eq!(
            ReportedCwd::parse(url),
            None,
            "url {url:?} must be rejected"
        );
    }
}

/// Spawn a `/bin/sh` terminal for the OSC 9;4 progress-report
/// pins, with its event receiver — the `cwd_report_terminal`
/// shape (a real PTY: the delivery and handoff facts need the
/// real event loop, so the channel must reach the caller).
fn progress_report_terminal() -> (Terminal, mpsc::UnboundedReceiver<(TabId, Event)>) {
    use alacritty_terminal::tty::Shell as PtyShell;

    let (tx, rx) = mpsc::unbounded_channel();
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
    .expect("spawn a /bin/sh progress-report test terminal");
    (terminal, rx)
}

/// Poll the tagged-event channel until the next `ProgressReport`
/// event arrives — skipping the wakeups that share the stream —
/// and pin its payload.
fn poll_until_progress(
    rx: &mut mpsc::UnboundedReceiver<(TabId, Event)>,
    want: (u8, Option<u8>),
    what: &str,
) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        while let Ok((_, event)) = rx.try_recv() {
            if let Event::ProgressReport(state, value) = event {
                assert_eq!((state, value), want, "{what}");
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for progress {want:?}: {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn a_fresh_terminal_carries_no_progress_report() {
    // Respawned/restored tabs start clean by construction — the
    // report dies with the terminal that owns it, deadline and all.
    let (terminal, _rx) = progress_report_terminal();
    assert_eq!(terminal.reported_progress(), None);
    assert_eq!(terminal.progress_decay_deadline(), None);
}

#[test]
fn progress_state_zero_clears_a_standing_report() {
    // The protocol's own invalidation (osc94-progress spec): state
    // 0 removes the report — a value riding it changes nothing —
    // and with it the decay deadline.
    let (mut terminal, _rx) = progress_report_terminal();
    terminal.note_progress_report(1, Some(42));
    assert_eq!(
        terminal.reported_progress().map(|r| (r.state, r.value)),
        Some((ProgressState::InProgress, Some(42)))
    );
    assert!(terminal.progress_decay_deadline().is_some());
    terminal.note_progress_report(0, None);
    assert_eq!(terminal.reported_progress(), None);
    assert_eq!(terminal.progress_decay_deadline(), None);
    // The clear is whole: a value on a state-0 report clears too.
    terminal.note_progress_report(2, Some(7));
    terminal.note_progress_report(0, Some(50));
    assert_eq!(terminal.reported_progress(), None);
}

#[test]
fn an_omitted_progress_value_keeps_the_standing_one() {
    // The protocol's leave-unchanged rule (osc94-progress spec): a
    // report may omit its value — states 2–4 do — and the standing
    // one survives the state change; with no value ever standing,
    // omitted stays absent.
    let (mut terminal, _rx) = progress_report_terminal();
    terminal.note_progress_report(1, Some(42));
    terminal.note_progress_report(2, None);
    assert_eq!(
        terminal.reported_progress().map(|r| (r.state, r.value)),
        Some((ProgressState::Error, Some(42)))
    );
    terminal.note_progress_report(4, None);
    assert_eq!(
        terminal.reported_progress().map(|r| (r.state, r.value)),
        Some((ProgressState::Paused, Some(42)))
    );
    terminal.note_progress_report(0, None);
    terminal.note_progress_report(3, None);
    assert_eq!(
        terminal.reported_progress().map(|r| (r.state, r.value)),
        Some((ProgressState::Indeterminate, None))
    );
}

#[test]
fn a_progress_report_expires_fifteen_seconds_after_its_last_refresh() {
    // The decay death (osc94-progress spec): Ghostty's hardcoded
    // ~15 s window, inherited by the record's rule — anchored at
    // the last refresh, so a refreshed report's clock restarts and
    // a dead reporter's last number cannot linger.
    let t0 = Instant::now();
    let standing = ReportedProgress::apply(None, 1, Some(42), t0).expect("the report stands");
    assert!(!standing.expired(t0 + Duration::from_secs(14)));
    assert!(standing.expired(t0 + Duration::from_secs(15)));
    // A refresh re-anchors the window.
    let refreshed = ReportedProgress::apply(Some(standing), 1, None, t0 + Duration::from_secs(10))
        .expect("the report still stands");
    assert!(!refreshed.expired(t0 + Duration::from_secs(24)));
    assert!(refreshed.expired(t0 + Duration::from_secs(25)));
}

#[test]
fn an_osc94_report_arrives_as_a_progress_report_event() {
    // The delivery half of the seat (osc94-progress spec): a
    // shell-emitted OSC 9;4 reaches the app as
    // `ProgressReport(state, value)` through the same tagged
    // stream the title rides — the fork arm's end-to-end pin on
    // the pinned revs, both the full and the omitted-value form.
    let (terminal, mut rx) = progress_report_terminal();

    terminal.input_no_scroll(&b"printf '\\033]9;4;1;42\\007\\033]9;4;3\\007'\r"[..]);
    poll_until_progress(&mut rx, (1, Some(42)), "the valued report arrives");
    poll_until_progress(&mut rx, (3, None), "the omitted-value report arrives");
}

#[test]
fn the_foreground_handoff_clears_a_standing_progress_report() {
    // The local edge (osc94-progress spec): the command that owned
    // the report finished — update()'s finish edge, the one the
    // timer already rides, clears the fact. The test stands in for
    // the app arm (event → engine note), then runs one real
    // foreground job that reports mid-run and exits: the handoff's
    // own update retires the number.
    let (mut terminal, mut rx) = progress_report_terminal();

    // One compound foreground job: report, then hold the
    // foreground long enough to witness it.
    terminal.input_no_scroll(&b"(printf '\\033]9;4;1;42\\007'; sleep 1)\r"[..]);
    poll_until_progress(&mut rx, (1, Some(42)), "the report arrives mid-command");
    terminal.note_progress_report(1, Some(42));

    // Witness the running state: the widget's draw is update()'s
    // driver in production (the duration tick wakes it for silent
    // runners — nothing else does), so the test stands in for the
    // render loop — one update while the job holds the foreground
    // records the state the finish edge needs, the timer's own
    // both-edges honesty rule.
    poll_until_foreground(&terminal, true, "the job takes the foreground");
    terminal.update();
    assert_eq!(
        terminal.reported_progress().map(|r| r.value),
        Some(Some(42)),
        "the report stands while its command owns the foreground"
    );

    // The job exits; one update after the handoff retires the
    // number — the prompt redraw's damage drives this update in
    // production.
    poll_until_foreground(&terminal, false, "the job hands the foreground back");
    terminal.update();
    assert_eq!(
        terminal.reported_progress(),
        None,
        "the handoff clears the standing report"
    );
}

/// Poll the tagged-event channel until the next `ProgramStatus`
/// event arrives — skipping the wakeups that share the stream — and
/// pin its payload.
fn poll_until_program_status(
    rx: &mut mpsc::UnboundedReceiver<(TabId, Event)>,
    want: &str,
    what: &str,
) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        while let Ok((_, event)) = rx.try_recv() {
            if let Event::ProgramStatus(body) = event {
                assert_eq!(body, want, "{what}");
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for program status {want:?}: {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn a_fresh_terminal_carries_no_program_status() {
    // Respawned/restored tabs start clean by construction — the
    // record dies with the terminal that owns it (osc7501-status
    // spec).
    let (terminal, _rx) = progress_report_terminal();
    assert_eq!(terminal.program_status(), None);
}

#[test]
fn program_status_pairs_parse_by_the_protocol_rules() {
    // The grammar (osc7501-status spec, the protocol's own rules):
    // pairs split on `:`, unknown keys ignored (the protocol's
    // extension path — `app`, `kind`, `title`, and `msg` ride it as
    // consumed-by-nothing), malformed pairs skipped, a repeated key's
    // last value winning.
    let standing = ProgramStatus {
        state: ProgramState::Done,
        progress: None,
    };
    // Unknown keys and ignored-by-design keys change nothing.
    assert_eq!(
        ProgramStatus::apply(
            Some(standing),
            "state=blocked:kind=permission:app=cargo:title=Rm9v:msg=YmFy:future=x"
        ),
        Some(ProgramStatus {
            state: ProgramState::Blocked,
            progress: None
        })
    );
    // Malformed pairs (no `=`) are skipped; the rest still parses.
    assert_eq!(
        ProgramStatus::apply(Some(standing), "state=working:progress=40:malformed"),
        Some(ProgramStatus {
            state: ProgramState::Working,
            progress: Some(40)
        })
    );
    // A repeated key's last value wins — state and progress alike.
    assert_eq!(
        ProgramStatus::apply(
            Some(standing),
            "state=working:state=error:progress=40:progress=70"
        ),
        Some(ProgramStatus {
            state: ProgramState::Error,
            progress: Some(70)
        })
    );
    // Each report replaces its record completely: an omitted progress
    // vanishes (no inheritance — the protocol's replace rule).
    assert_eq!(
        ProgramStatus::apply(
            Some(ProgramStatus {
                state: ProgramState::Working,
                progress: Some(40)
            }),
            "state=idle"
        ),
        Some(ProgramStatus {
            state: ProgramState::Idle,
            progress: None
        })
    );
    // A progress outside 0–100, or not an integer, is treated as
    // absent — the kind precedent for unrecognized values.
    assert_eq!(
        ProgramStatus::apply(Some(standing), "state=working:progress=101"),
        Some(ProgramStatus {
            state: ProgramState::Working,
            progress: None
        })
    );
    assert_eq!(
        ProgramStatus::apply(Some(standing), "state=working:progress=half"),
        Some(ProgramStatus {
            state: ProgramState::Working,
            progress: None
        })
    );
    assert_eq!(
        ProgramStatus::apply(Some(standing), "state=blocked:progress=100"),
        Some(ProgramStatus {
            state: ProgramState::Blocked,
            progress: Some(100)
        })
    );
}

#[test]
fn a_report_without_a_state_or_with_an_unrecognized_one_discards_whole() {
    // `state` is the protocol's one required key and an unrecognized
    // value discards the whole report — either way the standing
    // record is untouched (a mangled report never disturbs a live
    // truth).
    let standing = ProgramStatus {
        state: ProgramState::Done,
        progress: None,
    };
    assert_eq!(
        ProgramStatus::apply(Some(standing), "progress=40"),
        Some(standing)
    );
    assert_eq!(
        ProgramStatus::apply(Some(standing), "state=working:state=bogus"),
        Some(standing)
    );
    assert_eq!(
        ProgramStatus::apply(Some(standing), "state=bogus:state=working"),
        Some(standing)
    );
    // An over-long body rejects whole — the REPORTED_CWD_MAX
    // precedent, fail toward rejection.
    let long = format!("state=working:future={}", "x".repeat(5000));
    assert_eq!(ProgramStatus::apply(Some(standing), &long), Some(standing));
}

#[test]
fn id_records_are_not_the_band_s_narrator() {
    // Root-only (osc7501-status spec, OQ-2's locked default): one
    // disc, one name seat, the program's own summary — an id'd
    // report is a child record and is ignored whole, including an
    // id'd `clear` (it addresses the child, not the root).
    let standing = ProgramStatus {
        state: ProgramState::Working,
        progress: Some(40),
    };
    assert_eq!(
        ProgramStatus::apply(Some(standing), "id=us-east:state=blocked:progress=10"),
        Some(standing)
    );
    assert_eq!(
        ProgramStatus::apply(Some(standing), "state=clear:id=us-east"),
        Some(standing)
    );
    // The root `clear` removes the standing record — with anything
    // riding it.
    assert_eq!(ProgramStatus::apply(Some(standing), "state=clear"), None);
    assert_eq!(
        ProgramStatus::apply(Some(standing), "state=clear:progress=50"),
        None
    );
    // A `clear` for nothing standing stays nothing.
    assert_eq!(ProgramStatus::apply(None, "state=clear"), None);
}

#[test]
fn an_osc7501_report_arrives_as_a_program_status_event() {
    // The delivery half of the seat (osc7501-status spec): a
    // program-emitted OSC 7501 reaches the app as
    // `ProgramStatus(body)` through the same tagged stream — the
    // fork arm's end-to-end pin on the pinned revs, the body
    // verbatim.
    let (terminal, mut rx) = progress_report_terminal();

    terminal.input_no_scroll(&b"printf '\\033]7501;state=working:progress=40\\007'\r"[..]);
    poll_until_program_status(&mut rx, "state=working:progress=40", "the report arrives");
}

#[test]
fn the_foreground_handoff_drops_working_and_blocked_only() {
    // The local edge (osc7501-status spec): the program ended —
    // DropTerm watches it happen — so its `working`/`blocked`
    // reports die with it; `done` and `error` persist (their own
    // honest channels narrate finishes, and the protocol says they
    // survive). Three short jobs: a working report dies at the first
    // handoff, a blocked one at the second, a done one survives the
    // third.
    let (mut terminal, mut rx) = progress_report_terminal();

    // Job one: report `working` mid-run, then finish.
    terminal
        .input_no_scroll(&b"(printf '\\033]7501;state=working:progress=40\\007'; sleep 1)\r"[..]);
    poll_until_program_status(
        &mut rx,
        "state=working:progress=40",
        "the working report arrives",
    );
    terminal.note_program_status("state=working:progress=40");
    poll_until_foreground(&terminal, true, "job one takes the foreground");
    terminal.update();
    assert_eq!(
        terminal.program_status().map(|status| status.state),
        Some(ProgramState::Working),
        "the report stands while its program owns the foreground"
    );
    poll_until_foreground(&terminal, false, "job one hands the foreground back");
    terminal.update();
    assert_eq!(
        terminal.program_status(),
        None,
        "the handoff drops the working report"
    );

    // Job two: stand a blocked report (the program is waiting on the
    // user), run an unrelated job, and the handoff drops it too.
    terminal.note_program_status("state=blocked:kind=permission");
    assert_eq!(
        terminal.program_status().map(|status| status.state),
        Some(ProgramState::Blocked)
    );
    terminal.input_no_scroll(&b"sleep 1\r"[..]);
    poll_until_foreground(&terminal, true, "job two takes the foreground");
    terminal.update();
    poll_until_foreground(&terminal, false, "job two hands the foreground back");
    terminal.update();
    assert_eq!(
        terminal.program_status(),
        None,
        "the handoff drops the blocked report"
    );

    // Job three: a done report survives the handoff — finishes are
    // narrated by their own channels, and the record persists until
    // replaced, cleared, or respawned.
    terminal.note_program_status("state=done");
    terminal.input_no_scroll(&b"sleep 1\r"[..]);
    poll_until_foreground(&terminal, true, "job three takes the foreground");
    terminal.update();
    poll_until_foreground(&terminal, false, "job three hands the foreground back");
    terminal.update();
    assert_eq!(
        terminal.program_status().map(|status| status.state),
        Some(ProgramState::Done),
        "the done report persists through the handoff"
    );
}

#[test]
fn an_ris_arrives_as_a_full_reset_event_and_clears_the_record() {
    // RIS (osc7501-status spec, the protocol's MUST): a full reset
    // removes every record — the fork's FullReset event carries the
    // edge (sanctioned 2026-11-06), and the seat clears even the
    // persistent `done` on it. DECSTR and every other reset-shaped
    // sequence does not route through the fork's arm.
    let (mut terminal, mut rx) = progress_report_terminal();

    terminal.note_program_status("state=done");
    assert_eq!(
        terminal.program_status().map(|status| status.state),
        Some(ProgramState::Done)
    );

    terminal.input_no_scroll(&b"printf '\\033c'\r"[..]);
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        while let Ok((_, event)) = rx.try_recv() {
            if matches!(event, Event::FullReset) {
                terminal.note_terminal_reset();
                assert_eq!(
                    terminal.program_status(),
                    None,
                    "the full reset clears even the persistent record"
                );
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the RIS event"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn a_multibyte_escape_decodes_into_the_path() {
    let mut terminal = cwd_report_terminal();
    terminal.note_current_directory("file://h/home/%E4%BD%A0");
    let report = terminal
        .reported_cwd()
        .expect("the multibyte decode stands");
    assert_eq!(report.path, "/home/你");
}

#[test]
fn a_non_utf8_decode_rejects_without_disturbing_the_standing_report() {
    // `%FF` decodes to a byte sequence that is not UTF-8 — the
    // report rejects whole (the spec-named branch; fails toward
    // rejection).
    let mut terminal = cwd_report_terminal();
    terminal.note_current_directory("file://h/one");
    terminal.note_current_directory("file://h/%FF");
    let report = terminal
        .reported_cwd()
        .expect("the standing report survives");
    assert_eq!(report.path, "/one");
}

#[test]
fn a_root_path_report_stands() {
    let mut terminal = cwd_report_terminal();
    terminal.note_current_directory("file://h/");
    let report = terminal.reported_cwd().expect("the root is a path");
    assert_eq!(report.host.as_deref(), Some("h"));
    assert_eq!(report.path, "/");
}

#[test]
fn an_empty_host_report_never_names_a_remote_host() {
    let report = ReportedCwd::parse("file:///home/u").expect("parses");
    assert!(!report.names_remote_host(None));
    assert!(!report.names_remote_host(Some("laptop")));
}

#[test]
fn a_report_naming_our_host_is_local_case_insensitively() {
    let report = ReportedCwd::parse("file://Laptop/home/u").expect("parses");
    assert!(!report.names_remote_host(Some("laptop")));
}

#[test]
fn a_report_naming_another_host_is_remote() {
    let report = ReportedCwd::parse("file://workstation/home/u").expect("parses");
    assert!(report.names_remote_host(Some("laptop")));
    // No local name to compare against: a named host is taken at
    // its word.
    assert!(report.names_remote_host(None));
}

// ===== Inline images (iterm-images spec) =====

/// A test terminal whose pty never writes (the
/// `progress_report_terminal` precedent, but `/bin/cat` instead of
/// `/bin/sh`): the image pins assert exact cell coordinates, and an
/// interactive shell's prompt would have moved the cursor before the
/// placement — a silent pty keeps the grid pristine.
fn image_terminal() -> Terminal {
    use alacritty_terminal::tty::Shell as PtyShell;

    let (tx, _rx) = mpsc::unbounded_channel();
    Terminal::new(
        TabId(0),
        tx,
        Config::default(),
        &Options {
            shell: Some(PtyShell::new("/bin/cat".to_owned(), Vec::new())),
            drain_on_exit: true,
            ..Options::default()
        },
        &TerminalOptions::default(),
        &Colors::default(),
    )
    .expect("spawn a /bin/cat inline-image test terminal")
}

/// A small solid-color PNG of the given dimensions, as wire bytes.
fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(width, height, image::Rgba([200, 60, 60, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode the test png");
    bytes.into_inner()
}

/// Base64-encode wire bytes.
fn b64(data: &[u8]) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .encode(data)
        .into_bytes()
}

/// The image id standing behind the cell at the given grid point, if
/// one was placed.
fn cell_image(terminal: &Terminal, line: i32, col: usize) -> Option<u64> {
    let term = terminal.term.lock();
    term.grid()[alacritty_terminal::index::Line(line)][alacritty_terminal::index::Column(col)]
        .image()
        .map(|cell| cell.image)
}

/// A synthetic size for the pure resolver pins: 8×16 px cells in an
/// 800×480 session — every expected value below is exact arithmetic
/// on those numbers.
fn resolver_size() -> Size {
    Size {
        width: 800,
        height: 480,
        cell_width: 8.0,
        cell_height: 16.0,
    }
}

#[test]
fn size_unit_parses_every_documented_form() {
    use super::SizeUnit;

    assert_eq!(SizeUnit::parse(b"12"), Some(SizeUnit::Cells(12)));
    assert_eq!(SizeUnit::parse(b"12px"), Some(SizeUnit::Pixels(12)));
    assert_eq!(SizeUnit::parse(b"50%"), Some(SizeUnit::Percent(50)));
    assert_eq!(SizeUnit::parse(b"auto"), Some(SizeUnit::Auto));
    assert_eq!(SizeUnit::parse(b"AUTO"), Some(SizeUnit::Auto));
    // Malformed values read as absent, never a panic.
    assert_eq!(SizeUnit::parse(b"garbage"), None);
    assert_eq!(SizeUnit::parse(b"-3"), None);
    assert_eq!(SizeUnit::parse(b""), None);
}

#[test]
fn args_parse_the_placement_grammar_and_ignore_the_rest() {
    let (args, payload) = super::parse_image_args(&[
        b"File=inline=1".to_vec(),
        b"size=428".to_vec(),
        b"name=aGk=".to_vec(),
        b"width=2px".to_vec(),
        b"unknown=zzz".to_vec(),
    ]);

    assert!(args.inline);
    assert!(args.preserve_aspect);
    assert_eq!(args.width, Some(super::SizeUnit::Pixels(2)));
    assert_eq!(args.height, None);
    assert_eq!(payload, None, "no colon on the wire, no payload");
}

#[test]
fn legacy_payload_splits_at_the_first_colon() {
    let (args, payload) = super::parse_image_args(&[
        b"File=inline=1".to_vec(),
        b"name=aGk=:cGF5bG9hZA==".to_vec(),
    ]);

    assert!(args.inline);
    assert_eq!(payload, Some(b"cGF5bG9hZA==".to_vec()));
}

#[test]
fn preserve_aspect_zero_parses_as_off() {
    // The chunked header reaches the parser already split by vte —
    // each arg its own slice.
    let (args, _) = super::parse_image_args(&[
        b"MultipartFile=inline=1".to_vec(),
        b"preserveAspectRatio=0".to_vec(),
    ]);
    assert!(args.inline);
    assert!(!args.preserve_aspect);
}

#[test]
fn resolver_auto_auto_leaves_the_image_inherent() {
    let args = super::ImageArgs::default();
    assert_eq!(
        super::resolve_placement(&args, (64.0, 32.0), &resolver_size()),
        (8, 2)
    );
}

#[test]
fn resolver_cells_unit_scales_the_other_axis_along() {
    let args = super::ImageArgs {
        width: Some(super::SizeUnit::Cells(4)),
        ..Default::default()
    };
    // 4 cells = 32px; the 64px-wide image halves, carrying its 32px
    // height to 16px = exactly one row.
    assert_eq!(
        super::resolve_placement(&args, (64.0, 32.0), &resolver_size()),
        (4, 1)
    );
}

#[test]
fn resolver_pixels_unit_resolves_through_the_cell_size() {
    let args = super::ImageArgs {
        height: Some(super::SizeUnit::Pixels(64)),
        ..Default::default()
    };
    // 64px tall = 4 rows; the 32px-tall image doubles, its width to
    // 128px = 16 cols.
    assert_eq!(
        super::resolve_placement(&args, (64.0, 32.0), &resolver_size()),
        (16, 4)
    );
}

#[test]
fn resolver_percent_unit_resolves_against_the_session() {
    let args = super::ImageArgs {
        width: Some(super::SizeUnit::Percent(25)),
        ..Default::default()
    };
    // 25% of 800px = 200px wide; scale 200/64 = 3.125, height 100px =
    // 6.25 rows -> 6.
    assert_eq!(
        super::resolve_placement(&args, (64.0, 32.0), &resolver_size()),
        (25, 6)
    );
}

#[test]
fn resolver_both_given_fits_within_preserving_aspect() {
    let args = super::ImageArgs {
        width: Some(super::SizeUnit::Cells(10)),
        height: Some(super::SizeUnit::Cells(2)),
        ..Default::default()
    };
    // Box 80x32px; height binds (scale 1.0 vs width's 1.25).
    assert_eq!(
        super::resolve_placement(&args, (64.0, 32.0), &resolver_size()),
        (8, 2)
    );
}

#[test]
fn resolver_zero_aspect_honors_both_axes_exactly() {
    let args = super::ImageArgs {
        width: Some(super::SizeUnit::Cells(10)),
        height: Some(super::SizeUnit::Cells(2)),
        preserve_aspect: false,
        ..Default::default()
    };
    assert_eq!(
        super::resolve_placement(&args, (64.0, 32.0), &resolver_size()),
        (10, 2)
    );
}

#[test]
fn resolver_never_returns_zero_cells() {
    let args = super::ImageArgs::default();
    assert_eq!(
        super::resolve_placement(&args, (1.0, 1.0), &resolver_size()),
        (1, 1)
    );
}

#[test]
fn image_caps_are_the_documented_defaults() {
    assert_eq!(super::IMAGE_PAYLOAD_CAP, 32 * 1024 * 1024);
    assert_eq!(super::IMAGE_MAX_DIMENSION, 8192);
    assert_eq!(super::IMAGE_MAX_ALLOC, 64 * 1024 * 1024);
    assert_eq!(super::IMAGE_STORE_BUDGET, 320 * 1024 * 1024);
}

#[test]
fn legacy_single_shot_places_diced_cells_and_stores_pixels() {
    let mut terminal = image_terminal();
    let wire = b64(&png_bytes(8, 8));
    let mut name = b"name=aGk=".to_vec();
    name.push(b':');
    name.extend_from_slice(&wire);
    let params = vec![
        b"File=inline=1".to_vec(),
        b"width=4".to_vec(),
        b"height=4".to_vec(),
        // The live grid's cell shape is font-measured, so the pin
        // forces exact cells; aspect pins live on the pure resolver.
        b"preserveAspectRatio=0".to_vec(),
        name,
    ];

    terminal.note_inline_image(&params);

    // The placement is diced across exactly the requested cells.
    for line in 0..4 {
        for col in 0..4 {
            assert_eq!(
                cell_image(&terminal, line, col),
                Some(1),
                "cell ({line},{col})"
            );
        }
    }
    assert_eq!(cell_image(&terminal, 4, 0), None, "the line below is fresh");
    // The cursor landed below the placement.
    assert_eq!(
        terminal.term.lock().grid().cursor.point.line.0,
        4,
        "cursor below the placement"
    );
    // The store holds the decoded pixels.
    let store = terminal.images.lock().unwrap();
    assert_eq!(store.get(1).map(image::RgbaImage::dimensions), Some((8, 8)));
}

#[test]
fn chunked_session_accumulates_parts_until_the_end() {
    let mut terminal = image_terminal();
    let wire = b64(&png_bytes(8, 8));
    // Split at a 4-character base64 boundary so each part decodes
    // standalone, exactly like imgcat's chunks.
    let split = (wire.len() / 2) & !3;
    let (head, tail) = wire.split_at(split);

    let header = vec![
        b"MultipartFile=inline=1".to_vec(),
        b"width=2".to_vec(),
        b"height=2".to_vec(),
        b"preserveAspectRatio=0".to_vec(),
    ];
    terminal.note_inline_image(&header);
    terminal.note_inline_image(&[
        format!("FilePart={}", std::str::from_utf8(head).unwrap()).into_bytes()
    ]);
    terminal.note_inline_image(&[
        format!("FilePart={}", std::str::from_utf8(tail).unwrap()).into_bytes()
    ]);
    terminal.note_inline_image(&[b"FileEnd".to_vec()]);

    for line in 0..2 {
        for col in 0..2 {
            assert_eq!(
                cell_image(&terminal, line, col),
                Some(1),
                "cell ({line},{col})"
            );
        }
    }
    assert_eq!(
        terminal
            .images
            .lock()
            .unwrap()
            .get(1)
            .map(image::RgbaImage::dimensions),
        Some((8, 8))
    );
}

#[test]
fn a_second_header_aborts_the_first_session() {
    let mut terminal = image_terminal();
    let first = b64(&png_bytes(16, 16));
    let second = b64(&png_bytes(8, 8));
    let mut second_name = b"FilePart=".to_vec();
    second_name.extend_from_slice(&second);

    let header = || {
        vec![
            b"MultipartFile=inline=1".to_vec(),
            b"width=2".to_vec(),
            b"height=2".to_vec(),
            b"preserveAspectRatio=0".to_vec(),
        ]
    };
    terminal.note_inline_image(&header());
    let mut first_part = b"FilePart=".to_vec();
    first_part.extend_from_slice(&first);
    terminal.note_inline_image(&[first_part]);
    // The second header replaces the first session: its part is the
    // only payload left standing.
    terminal.note_inline_image(&header());
    terminal.note_inline_image(&[second_name]);
    terminal.note_inline_image(&[b"FileEnd".to_vec()]);

    assert_eq!(
        terminal
            .images
            .lock()
            .unwrap()
            .get(1)
            .map(image::RgbaImage::dimensions),
        Some((8, 8)),
        "the first session's bytes died with its header"
    );
}

#[test]
fn file_end_without_a_session_is_ignored() {
    let mut terminal = image_terminal();
    terminal.note_inline_image(&[b"FileEnd".to_vec()]);
    assert_eq!(cell_image(&terminal, 0, 0), None);
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

#[test]
fn file_part_without_a_session_is_ignored() {
    let mut terminal = image_terminal();
    terminal.note_inline_image(&[b"FilePart=aGVsbG8=".to_vec()]);
    terminal.note_inline_image(&[b"FileEnd".to_vec()]);
    assert_eq!(cell_image(&terminal, 0, 0), None);
}

#[test]
fn a_malformed_part_aborts_the_session_whole() {
    let mut terminal = image_terminal();
    terminal.note_inline_image(&[
        b"MultipartFile=inline=1".to_vec(),
        b"width=2".to_vec(),
        b"height=2".to_vec(),
    ]);
    terminal.note_inline_image(&[b"FilePart=@@@".to_vec()]);
    terminal.note_inline_image(&[b"FileEnd".to_vec()]);

    assert_eq!(cell_image(&terminal, 0, 0), None, "nothing half-places");
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

#[test]
fn a_malformed_payload_never_places() {
    let mut terminal = image_terminal();
    let wire = b64(b"not an image at all");
    let mut name = b"name=aGk=".to_vec();
    name.push(b':');
    name.extend_from_slice(&wire);
    terminal.note_inline_image(&[b"File=inline=1".to_vec(), b"width=4".to_vec(), name]);

    assert_eq!(cell_image(&terminal, 0, 0), None);
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

#[test]
fn inline_zero_renders_nothing() {
    let mut terminal = image_terminal();
    let wire = b64(&png_bytes(8, 8));
    let mut name = b"name=aGk=".to_vec();
    name.push(b':');
    name.extend_from_slice(&wire);
    terminal.note_inline_image(&[b"File=inline=0".to_vec(), b"width=4".to_vec(), name]);

    assert_eq!(cell_image(&terminal, 0, 0), None);
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

#[test]
fn images_enabled_false_makes_the_arm_inert() {
    let mut terminal = image_terminal();
    terminal.images_enabled = false;
    let wire = b64(&png_bytes(8, 8));
    let mut name = b"name=aGk=".to_vec();
    name.push(b':');
    name.extend_from_slice(&wire);
    terminal.note_inline_image(&[b"File=inline=1".to_vec(), b"width=4".to_vec(), name]);

    assert_eq!(cell_image(&terminal, 0, 0), None);
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

#[test]
fn an_over_cap_payload_drops_the_session_whole() {
    let mut terminal = image_terminal();
    terminal.note_inline_image(&[b"MultipartFile=inline=1".to_vec()]);
    let huge = vec![b'A'; super::IMAGE_PAYLOAD_CAP + 1];
    let part = format!("FilePart={}", {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(&huge)
    });
    terminal.note_inline_image(&[part.into_bytes()]);
    terminal.note_inline_image(&[b"FileEnd".to_vec()]);

    assert_eq!(
        cell_image(&terminal, 0, 0),
        None,
        "never half-buffered, never placed"
    );
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

#[test]
fn an_over_dimension_image_drops_whole() {
    let mut terminal = image_terminal();
    let wire = b64(&png_bytes(super::IMAGE_MAX_DIMENSION + 1, 1));
    let mut name = b"name=aGk=".to_vec();
    name.push(b':');
    name.extend_from_slice(&wire);
    terminal.note_inline_image(&[b"File=inline=1".to_vec(), name]);

    assert_eq!(cell_image(&terminal, 0, 0), None);
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

#[test]
fn an_over_allocation_image_drops_whole() {
    let mut terminal = image_terminal();
    // 5000x4000 RGBA decodes to ~80 MiB, past the 64 MiB allocation
    // ceiling; flat color keeps the PNG itself tiny.
    let wire = b64(&png_bytes(5000, 4000));
    let mut name = b"name=aGk=".to_vec();
    name.push(b':');
    name.extend_from_slice(&wire);
    terminal.note_inline_image(&[b"File=inline=1".to_vec(), name]);

    assert_eq!(cell_image(&terminal, 0, 0), None);
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

#[test]
fn store_evicts_oldest_under_its_budget() {
    let mut store = super::ImageStore::default();
    let image = || image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
    let budget = 2 * image().len();

    store.insert_with_budget(1, image(), budget);
    store.insert_with_budget(2, image(), budget);
    assert_eq!(store.total_bytes(), budget);

    store.insert_with_budget(3, image(), budget);
    // The oldest id left; the other two stand.
    assert!(store.get(1).is_none(), "evicted ids draw blank");
    assert!(store.get(2).is_some());
    assert!(store.get(3).is_some());
    assert_eq!(store.total_bytes(), budget);
}

#[test]
fn store_never_evicts_the_image_just_inserted() {
    let mut store = super::ImageStore::default();
    let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
    let len = image.len();

    store.insert_with_budget(1, image, len / 2);
    // Over budget alone, it still stands: the cap is a ceiling, not a
    // denial — decode's own limits keep any single image far under
    // the shipped budget.
    assert!(store.get(1).is_some());
    assert_eq!(store.total_bytes(), len);
}

#[test]
fn handle_for_builds_once_and_dies_with_eviction() {
    let mut store = super::ImageStore::default();
    let image = || image::RgbaImage::from_pixel(2, 2, image::Rgba([9, 9, 9, 255]));
    let budget = 2 * image().len();
    store.insert_with_budget(1, image(), budget);
    store.insert_with_budget(2, image(), budget);

    // One build, then clones: the ids match, so the renderer's cache
    // sees one handle — one upload — for the id's whole life.
    let first = store.handle_for(1).expect("the id stands behind pixels");
    assert_eq!(store.handle_for(1), Some(first));

    // Evicting id 1 drops its handle with its pixels.
    store.insert_with_budget(3, image(), budget);
    assert!(store.get(1).is_none());
    assert_eq!(store.handle_for(1), None);
}

// ===== Kitty graphics (kitty-images spec) =====

/// One APC `G` payload as the fork delivers it: the `G`, the control
/// keys, and the base64 payload behind the first `;` (omitted when
/// empty, the empty-final-chunk wire shape).
fn apc(keys: &str, payload: &[u8]) -> Vec<u8> {
    let mut bytes = format!("G{keys}").into_bytes();
    if !payload.is_empty() {
        bytes.push(b';');
        bytes.extend_from_slice(&b64(payload));
    }
    bytes
}

/// The wire form whose base64 slice is already encoded (for chunks
/// split at a base64 boundary, exactly like real clients).
fn apc_b64(keys: &str, encoded: &str) -> Vec<u8> {
    format!("G{keys};{encoded}").into_bytes()
}

/// Flat RGBA bytes for the `f=32` raw path.
fn raw_rgba(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for _ in 0..width * height {
        bytes.extend_from_slice(&[10, 20, 30, 255]);
    }
    bytes
}

/// zlib-compress bytes the `o=z` wire form arrives in.
fn zlib_bytes(data: &[u8]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("compress the test payload");
    encoder.finish().expect("finish the zlib stream")
}

#[test]
fn kitty_keys_parse_the_subset_and_ignore_unknown_keys() {
    let keys = super::parse_kitty_keys(
        b"a=T,f=100,t=d,s=8,v=8,c=4,r=4,x=1,y=2,o=z,m=1,q=1,i=7,p=3,z=-1,U=1,k=9",
    )
    .expect("every subset key parses");

    assert_eq!(keys.action, Some(b'T'));
    assert_eq!(keys.format, Some(100));
    assert_eq!(keys.transmission, Some(b'd'));
    assert_eq!(keys.width_px, Some(8));
    assert_eq!(keys.height_px, Some(8));
    assert_eq!(keys.cols, Some(4));
    assert_eq!(keys.rows, Some(4));
    assert_eq!(keys.offset_x, Some(1));
    assert_eq!(keys.offset_y, Some(2));
    assert!(keys.compressed);
    assert!(keys.more);
    assert_eq!(keys.quiet, 1);
    assert_eq!(keys.image_id, Some(7));
    assert_eq!(keys.placement_id, Some(3));
    assert_eq!(keys.z, Some(-1));
}

#[test]
fn kitty_keys_drop_malformed_control_whole() {
    // No '=' anywhere, an empty value, an empty key, a multi-char key
    // (kitty keys are single characters), and known keys with values
    // outside their grammar — each drops the sequence whole.
    for control in [
        &b"abcd"[..],
        b"a=",
        b"=3",
        b"ab=1",
        b"f=png",
        b"s=-4",
        b"a=Z",
        b"t=Q",
        b"o=q",
        b"m=2",
        b"q=3",
        b"z=abc",
    ] {
        assert_eq!(super::parse_kitty_keys(control), None, "{control:?}");
    }
}

#[test]
fn kitty_single_shot_png_transmits_and_displays() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("a=T,f=100,c=2,r=2", &png_bytes(8, 8)));

    for line in 0..2 {
        for col in 0..2 {
            assert_eq!(
                cell_image(&terminal, line, col),
                Some(1),
                "cell ({line},{col})"
            );
        }
    }
    assert_eq!(cell_image(&terminal, 2, 0), None, "the line below is fresh");
    let store = terminal.images.lock().unwrap();
    assert_eq!(store.get(1).map(image::RgbaImage::dimensions), Some((8, 8)));
}

#[test]
fn kitty_default_action_transmits_without_display() {
    // `a` absent defaults to transmit-only: the pixels stand in the
    // store, no cell carries them.
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("f=100", &png_bytes(8, 8)));

    assert_eq!(cell_image(&terminal, 0, 0), None);
    let store = terminal.images.lock().unwrap();
    assert!(store.get(1).is_some(), "transmit-only still stores");
}

#[test]
fn kitty_f32_raw_sizes_from_s_and_v() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("a=T,f=32,s=2,v=2,c=2,r=2", &raw_rgba(2, 2)));

    for line in 0..2 {
        for col in 0..2 {
            assert_eq!(
                cell_image(&terminal, line, col),
                Some(1),
                "cell ({line},{col})"
            );
        }
    }
    let store = terminal.images.lock().unwrap();
    assert_eq!(store.get(1).map(image::RgbaImage::dimensions), Some((2, 2)));
}

#[test]
fn kitty_format_defaults_to_f32() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("a=T,s=1,v=1,c=1,r=1", &raw_rgba(1, 1)));

    assert_eq!(cell_image(&terminal, 0, 0), Some(1));
    assert_eq!(cell_image(&terminal, 0, 1), None);
}

#[test]
fn kitty_f32_wrong_byte_count_drops_whole() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("f=32,s=2,v=2,c=1,r=1", &raw_rgba(2, 2)[..15]));

    assert_eq!(cell_image(&terminal, 0, 0), None);
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

#[test]
fn kitty_f32_missing_dims_drop_whole() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("f=32,c=1,r=1", &raw_rgba(1, 1)));

    assert_eq!(cell_image(&terminal, 0, 0), None);
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

#[test]
fn kitty_chunked_transmission_assembles_until_m0() {
    let mut terminal = image_terminal();
    let wire = b64(&png_bytes(8, 8));
    let split = (wire.len() / 2) & !3;
    let (head, tail) = wire.split_at(split);

    terminal.note_apc_payload(&apc_b64(
        "a=T,f=100,c=2,r=2,m=1",
        std::str::from_utf8(head).unwrap(),
    ));
    terminal.note_apc_payload(&apc_b64("m=1", std::str::from_utf8(tail).unwrap()));
    terminal.note_apc_payload(&apc("m=0", b""));

    for line in 0..2 {
        for col in 0..2 {
            assert_eq!(
                cell_image(&terminal, line, col),
                Some(1),
                "cell ({line},{col})"
            );
        }
    }
    let store = terminal.images.lock().unwrap();
    assert_eq!(store.get(1).map(image::RgbaImage::dimensions), Some((8, 8)));
}

#[test]
fn kitty_empty_final_chunk_commits() {
    // The observed chafa shape: a final `m=0` whose payload slice is
    // empty — the commit fires on the marker alone.
    let mut terminal = image_terminal();
    let wire = b64(&png_bytes(8, 8));

    terminal.note_apc_payload(&apc_b64(
        "a=T,f=100,c=1,r=1,m=1",
        std::str::from_utf8(&wire).unwrap(),
    ));
    terminal.note_apc_payload(&apc_b64("m=0", ""));

    assert_eq!(cell_image(&terminal, 0, 0), Some(1));
    let store = terminal.images.lock().unwrap();
    assert_eq!(store.get(1).map(image::RgbaImage::dimensions), Some((8, 8)));
}

#[test]
fn kitty_interleaved_transmissions_keyed_by_i() {
    let mut terminal = image_terminal();
    let first = b64(&png_bytes(8, 8));
    let second = b64(&png_bytes(16, 16));
    let first_split = (first.len() / 2) & !3;
    let second_split = (second.len() / 2) & !3;

    terminal.note_apc_payload(&apc_b64(
        "i=7,f=100,m=1",
        std::str::from_utf8(&first[..first_split]).unwrap(),
    ));
    terminal.note_apc_payload(&apc_b64(
        "i=9,f=100,m=1",
        std::str::from_utf8(&second[..second_split]).unwrap(),
    ));
    terminal.note_apc_payload(&apc_b64(
        "i=7,m=1",
        std::str::from_utf8(&first[first_split..]).unwrap(),
    ));
    terminal.note_apc_payload(&apc_b64(
        "i=9,m=1",
        std::str::from_utf8(&second[second_split..]).unwrap(),
    ));
    terminal.note_apc_payload(&apc("i=7,m=0", b""));
    terminal.note_apc_payload(&apc("i=9,m=0", b""));

    let store = terminal.images.lock().unwrap();
    assert_eq!(store.get(1).map(image::RgbaImage::dimensions), Some((8, 8)));
    assert_eq!(
        store.get(2).map(image::RgbaImage::dimensions),
        Some((16, 16))
    );
    drop(store);
    assert_eq!(
        terminal.kitty_ids.get(&7),
        Some(&1),
        "client i maps to its store id"
    );
    assert_eq!(terminal.kitty_ids.get(&9), Some(&2));
}

#[test]
fn kitty_continuation_without_a_session_drops() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc_b64("m=1", "QUJD"));
    terminal.note_apc_payload(&apc("m=0", b""));

    assert_eq!(cell_image(&terminal, 0, 0), None);
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

#[test]
fn kitty_second_header_replaces_the_session() {
    let mut terminal = image_terminal();
    let second = b64(&png_bytes(8, 8));

    terminal.note_apc_payload(&apc_b64(
        "i=7,f=100,m=1",
        std::str::from_utf8(&b64(&png_bytes(16, 16))).unwrap(),
    ));
    terminal.note_apc_payload(&apc_b64(
        "i=7,f=100,m=1",
        std::str::from_utf8(&second).unwrap(),
    ));
    terminal.note_apc_payload(&apc("i=7,m=0", b""));

    let store = terminal.images.lock().unwrap();
    assert_eq!(
        store.get(1).map(image::RgbaImage::dimensions),
        Some((8, 8)),
        "the first session's bytes died with its replaced header"
    );
}

#[test]
fn kitty_transmission_refusals_answer_the_uniform_error() {
    // Every forbidden medium answers the one identical error — no
    // filesystem, no temp file, no shared memory, no probing surface.
    assert_eq!(
        super::kitty_error_reply(Some(9)),
        b"\x1b_Gi=9;EBADF:Failed to read image file\x1b\\".to_vec()
    );
    assert_eq!(
        super::kitty_error_reply(None),
        b"\x1b_GEBADF:Failed to read image file\x1b\\".to_vec()
    );

    for refused in ["t=f", "t=t", "t=s"] {
        let mut terminal = image_terminal();
        terminal.note_apc_payload(&apc(&format!("{refused},i=9,f=100"), &png_bytes(8, 8)));

        assert_eq!(cell_image(&terminal, 0, 0), None, "{refused}");
        assert!(
            terminal.images.lock().unwrap().get(1).is_none(),
            "{refused} never crosses the seam"
        );
    }
}

#[test]
fn kitty_o_z_payloads_inflate() {
    let mut terminal = image_terminal();
    let compressed = zlib_bytes(&raw_rgba(2, 2));
    terminal.note_apc_payload(&apc("a=T,f=32,s=2,v=2,c=1,r=1,o=z", &compressed));

    assert_eq!(cell_image(&terminal, 0, 0), Some(1));
    let store = terminal.images.lock().unwrap();
    assert_eq!(store.get(1).map(image::RgbaImage::dimensions), Some((2, 2)));
}

#[test]
fn kitty_over_dimension_and_pixel_budget_drop_whole() {
    let mut terminal = image_terminal();
    // 8193 px wide is past the dimension ceiling; 5000x4000 is past
    // the 16 Mpx allocation budget at four bytes a pixel.
    terminal.note_apc_payload(&apc("f=32,s=8193,v=1,c=1,r=1", &raw_rgba(1, 1)));
    terminal.note_apc_payload(&apc("f=32,s=5000,v=4000,c=1,r=1", &raw_rgba(1, 1)));

    assert_eq!(cell_image(&terminal, 0, 0), None);
    let store = terminal.images.lock().unwrap();
    assert!(store.get(1).is_none() && store.get(2).is_none());
}

#[test]
fn kitty_over_cap_accumulation_drops_the_session_whole() {
    let mut terminal = image_terminal();
    // 3000x3000 RGBA is 36 MB — under the pixel budget, over the
    // 32 MiB payload cap, so the accumulation check is what fires.
    let huge = raw_rgba(3000, 3000);
    terminal.note_apc_payload(&apc("f=32,s=3000,v=3000,m=1", &huge));
    terminal.note_apc_payload(&apc("m=0", b""));

    assert_eq!(cell_image(&terminal, 0, 0), None, "never half-buffered");
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

#[test]
fn kitty_payload_without_leading_g_is_silence() {
    let mut terminal = image_terminal();
    // SOS/PM strings share the APC state; anything not carrying the
    // kitty introducer never reaches the machine.
    terminal.note_apc_payload(b"ga=q");
    terminal.note_apc_payload(b"");
    terminal.note_apc_payload(b"Xm=1;QUJD");

    assert_eq!(cell_image(&terminal, 0, 0), None);
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

#[test]
fn kitty_x_y_offsets_crop_the_displayed_region() {
    let mut terminal = image_terminal();
    // The 4x4 raw image with x=2,y=2 displays its bottom-right
    // quadrant: the stored pixels are the crop.
    terminal.note_apc_payload(&apc("a=T,f=32,s=4,v=4,c=1,r=1,x=2,y=2", &raw_rgba(4, 4)));

    assert_eq!(cell_image(&terminal, 0, 0), Some(1));
    let store = terminal.images.lock().unwrap();
    assert_eq!(store.get(1).map(image::RgbaImage::dimensions), Some((2, 2)));
}

#[test]
fn kitty_retransmit_remaps_the_client_id() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("i=7,f=100", &png_bytes(8, 8)));
    terminal.note_apc_payload(&apc("i=7,f=100", &png_bytes(16, 16)));

    assert_eq!(
        terminal.kitty_ids.get(&7),
        Some(&2),
        "the map follows the latest"
    );
    let store = terminal.images.lock().unwrap();
    assert!(store.get(1).is_some(), "the old pixels ride the budget");
    assert!(store.get(2).is_some());
}

#[test]
fn kitty_malformed_base64_drops_the_chunk_whole() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc_b64("f=100,m=1", "@@@"));
    terminal.note_apc_payload(&apc_b64("m=1", "QUJD"));
    terminal.note_apc_payload(&apc("m=0", b""));

    assert_eq!(
        cell_image(&terminal, 0, 0),
        None,
        "the session died with its bad chunk"
    );
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

#[test]
fn kitty_images_enabled_false_is_inert() {
    let mut terminal = image_terminal();
    terminal.images_enabled = false;
    terminal.note_apc_payload(&apc("a=T,f=100,c=1,r=1", &png_bytes(8, 8)));

    assert_eq!(cell_image(&terminal, 0, 0), None);
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

// ===== Kitty queries, placements, deletion (kitty-images spec) =====

#[test]
fn kitty_reply_builder_is_byte_exact() {
    assert_eq!(
        super::kitty_reply(Some(7), "OK"),
        b"\x1b_Gi=7;OK\x1b\\".to_vec()
    );
    assert_eq!(super::kitty_reply(None, "OK"), b"\x1b_GOK\x1b\\".to_vec());
    assert_eq!(
        super::kitty_error_reply(Some(9)),
        b"\x1b_Gi=9;EBADF:Failed to read image file\x1b\\".to_vec()
    );
}

#[test]
fn kitty_feature_lists_are_true_of_the_implementation() {
    // Exactly what each e= group's answer names — the keys this
    // implementation parses and honors, nothing more.
    assert_eq!(
        super::kitty_feature_list(b'T'),
        "t=d,f=24,f=32,f=100,o=z,s,v,m,i,I,q"
    );
    assert_eq!(super::kitty_feature_list(b't'), "a=T,a=t,c,r,x,y");
    assert_eq!(super::kitty_feature_list(b'p'), "a=p,p,i,I,z");
    assert_eq!(
        super::kitty_feature_list(b'd'),
        "d=a,d=A,d=i,d=I,d=n,d=N,d=c,d=C"
    );
}

#[test]
fn kitty_query_answers_ok_for_a_live_image() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("i=7,f=100", &png_bytes(8, 8)));

    let keys = super::parse_kitty_keys(b"i=7").expect("the query's keys");
    let images = terminal.images.lock().unwrap();
    assert_eq!(
        super::kitty_query_reply(&keys, &terminal.kitty_ids, &images),
        Some(b"\x1b_Gi=7;OK\x1b\\".to_vec())
    );

    // A number names the image too — the store id itself (OQ-1).
    let by_number = super::parse_kitty_keys(b"I=1").expect("the number query");
    assert_eq!(
        super::kitty_query_reply(&by_number, &terminal.kitty_ids, &images),
        Some(b"\x1b_GOK\x1b\\".to_vec()),
        "I= names by number; no i= rides the reply"
    );
}

#[test]
fn kitty_query_answers_the_error_for_unknown_or_freed_images() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("i=7,f=100", &png_bytes(8, 8)));
    // Free the pixels: the id maps but nothing stands behind it.
    terminal.images.lock().unwrap().remove(1);

    let unknown = super::parse_kitty_keys(b"i=7").expect("the query's keys");
    let images = terminal.images.lock().unwrap();
    assert_eq!(
        super::kitty_query_reply(&unknown, &terminal.kitty_ids, &images),
        Some(b"\x1b_Gi=7;EBADF:Failed to read image file\x1b\\".to_vec())
    );
}

#[test]
fn kitty_query_suppression_honors_q1_and_q2() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("i=7,f=100", &png_bytes(8, 8)));

    // q=1 silences the OK answer; the error answer still goes.
    let quiet_ok = super::parse_kitty_keys(b"i=7,q=1").expect("keys");
    let images = terminal.images.lock().unwrap();
    assert_eq!(
        super::kitty_query_reply(&quiet_ok, &terminal.kitty_ids, &images),
        None
    );
    let quiet_error = super::parse_kitty_keys(b"i=9,q=1").expect("keys");
    assert_eq!(
        super::kitty_query_reply(&quiet_error, &terminal.kitty_ids, &images),
        Some(b"\x1b_Gi=9;EBADF:Failed to read image file\x1b\\".to_vec())
    );

    // q=2 silences everything.
    let quiet_all = super::parse_kitty_keys(b"i=9,q=2").expect("keys");
    assert_eq!(
        super::kitty_query_reply(&quiet_all, &terminal.kitty_ids, &images),
        None
    );
    drop(images);

    // A query naming no image at all is silence, never a reply.
    let bare = super::parse_kitty_keys(b"").expect("keys");
    let images = terminal.images.lock().unwrap();
    assert_eq!(
        super::kitty_query_reply(&bare, &terminal.kitty_ids, &images),
        None
    );
}

#[test]
fn kitty_e_queries_answer_the_feature_lists() {
    let keys = super::parse_kitty_keys(b"i=1,e=T").expect("keys");
    let ids = std::collections::HashMap::new();
    let images = super::ImageStore::default();
    assert_eq!(
        super::kitty_query_reply(&keys, &ids, &images),
        Some(b"\x1b_Gi=1;OK t=d,f=24,f=32,f=100,o=z,s,v,m,i,I,q\x1b\\".to_vec())
    );

    let display = super::parse_kitty_keys(b"e=t").expect("keys");
    assert_eq!(
        super::kitty_query_reply(&display, &ids, &images),
        Some(b"\x1b_GOK a=T,a=t,c,r,x,y\x1b\\".to_vec())
    );
}

#[test]
fn kitty_place_seats_a_transmitted_image_at_the_cursor() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("i=7,f=100,c=2,r=2", &png_bytes(8, 8)));
    assert_eq!(
        cell_image(&terminal, 0, 0),
        None,
        "transmit-only placed nothing"
    );

    // Without a placement id the cells ride the image's own id.
    terminal.note_apc_payload(&apc("a=p,i=7,c=2,r=2", b""));
    for line in 0..2 {
        for col in 0..2 {
            assert_eq!(
                cell_image(&terminal, line, col),
                Some(1),
                "cell ({line},{col})"
            );
        }
    }
}

#[test]
fn kitty_re_place_moves_the_placements_cells_and_reuses_data() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("i=7,f=100", &png_bytes(8, 8)));

    // The placement seats under its own id (a fresh store entry), so
    // the move sweep takes exactly it.
    terminal.note_apc_payload(&apc("a=p,i=7,p=3,c=2,r=2", b""));
    for line in 0..2 {
        for col in 0..2 {
            assert_eq!(
                cell_image(&terminal, line, col),
                Some(2),
                "cell ({line},{col})"
            );
        }
    }

    // The re-place at the cursor (now below the first placement)
    // clears the old cells and seats the same id again — the data
    // reused, no fresh store entry.
    terminal.note_apc_payload(&apc("a=p,i=7,p=3,c=1,r=1", b""));
    assert_eq!(
        cell_image(&terminal, 0, 0),
        None,
        "the old cells moved away"
    );
    assert_eq!(
        cell_image(&terminal, 2, 0),
        Some(2),
        "the same id re-seated"
    );
    let store = terminal.images.lock().unwrap();
    assert!(store.get(2).is_some(), "the placement's pixels stand");
    assert!(
        store.get(3).is_none(),
        "no fresh entry — the data is reused"
    );
    assert_eq!(terminal.kitty_placements.get(&3).map(|p| p.cells), Some(2));
}

#[test]
fn kitty_delete_by_name_clears_placements_and_uppercase_frees() {
    // Lowercase: the cells go, the pixels stand.
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("i=7,a=T,f=100,c=1,r=1", &png_bytes(8, 8)));
    terminal.note_apc_payload(&apc("a=d,d=i7", b""));
    assert_eq!(cell_image(&terminal, 0, 0), None);
    assert!(
        terminal.images.lock().unwrap().get(1).is_some(),
        "lowercase keeps the data"
    );

    // Uppercase: the pixels free too, and the id map forgets.
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("i=7,a=T,f=100,c=1,r=1", &png_bytes(8, 8)));
    terminal.note_apc_payload(&apc("a=d,d=I7", b""));
    assert_eq!(cell_image(&terminal, 0, 0), None);
    assert!(
        terminal.images.lock().unwrap().get(1).is_none(),
        "uppercase frees"
    );
    assert!(!terminal.kitty_ids.contains_key(&7));
}

#[test]
fn kitty_delete_by_name_sweeps_the_placements_too() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("i=7,f=100", &png_bytes(8, 8)));
    terminal.note_apc_payload(&apc("a=p,i=7,p=3,c=1,r=1", b""));
    assert_eq!(cell_image(&terminal, 0, 0), Some(2));

    // Deleting the image takes its placements' cells and pixels.
    terminal.note_apc_payload(&apc("a=d,d=I7", b""));
    assert_eq!(cell_image(&terminal, 0, 0), None);
    let store = terminal.images.lock().unwrap();
    assert!(store.get(1).is_none() && store.get(2).is_none());
    assert!(terminal.kitty_placements.is_empty());
}

#[test]
fn kitty_delete_by_name_resolves_as_a_number_when_no_client_id_matches() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("a=T,f=100,c=1,r=1", &png_bytes(8, 8)));
    // No client id 1 was ever declared; the name resolves as the
    // number (the store id itself).
    terminal.note_apc_payload(&apc("a=d,d=i1", b""));
    assert_eq!(cell_image(&terminal, 0, 0), None);
}

#[test]
fn kitty_delete_newest_takes_the_last_transmitted_image() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("a=T,f=100,c=1,r=1", &png_bytes(8, 8)));
    terminal.note_apc_payload(&apc("a=T,f=100,c=1,r=1", &png_bytes(8, 8)));
    // The cursor moved below the first placement; both images stand.
    assert_eq!(cell_image(&terminal, 0, 0), Some(1));
    assert_eq!(cell_image(&terminal, 1, 0), Some(2));

    terminal.note_apc_payload(&apc("a=d,d=N", b""));
    assert_eq!(
        cell_image(&terminal, 1, 0),
        None,
        "the newest image's cells go"
    );
    let store = terminal.images.lock().unwrap();
    assert!(store.get(2).is_none(), "N frees the newest");
    assert!(store.get(1).is_some(), "the older image stands");
}

#[test]
fn kitty_delete_all_sweeps_everything() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("a=T,f=100,c=1,r=1", &png_bytes(8, 8)));
    terminal.note_apc_payload(&apc("a=T,f=100,c=1,r=1", &png_bytes(8, 8)));

    terminal.note_apc_payload(&apc("a=d,d=a", b""));
    assert_eq!(cell_image(&terminal, 0, 0), None);
    assert_eq!(cell_image(&terminal, 1, 0), None);
    {
        let store = terminal.images.lock().unwrap();
        assert!(
            store.get(1).is_some() && store.get(2).is_some(),
            "a keeps the data"
        );
    }

    // Re-place from the standing pixels, then A takes everything.
    terminal.note_apc_payload(&apc("a=p,i=1,c=1,r=1", b""));
    terminal.note_apc_payload(&apc("a=d,d=A", b""));
    assert_eq!(cell_image(&terminal, 0, 0), None);
    {
        let store = terminal.images.lock().unwrap();
        assert!(
            store.get(1).is_none() && store.get(2).is_none(),
            "A frees all"
        );
    }
    assert!(terminal.kitty_ids.is_empty());
}

#[test]
fn kitty_delete_visible_sweeps_the_shown_rows_and_c_frees_them() {
    let mut terminal = image_terminal();
    // The fixture's grid is 80x24: a one-row full-width image, then a
    // thirty-row placement whose scroll push rides the first image's
    // row six lines into scrollback — its cells survive only there,
    // at columns past the tall placement's three-column width.
    terminal.note_apc_payload(&apc("i=1,a=T,f=100,c=80,r=1", &png_bytes(8, 8)));
    terminal.note_apc_payload(&apc("i=2,a=T,f=100,c=3,r=30", &png_bytes(8, 8)));
    assert!(
        cell_image(&terminal, -7, 3).is_some(),
        "the first image rides in scrollback"
    );
    assert!(
        cell_image(&terminal, 0, 0).is_some(),
        "the tall placement covers the screen"
    );

    // c: the visible screen's placements go; the scrollback stands.
    terminal.note_apc_payload(&apc("a=d,d=c", b""));
    assert_eq!(cell_image(&terminal, 0, 0), None, "the visible cells swept");
    assert!(cell_image(&terminal, -7, 3).is_some(), "scrollback stands");
    {
        let store = terminal.images.lock().unwrap();
        assert!(
            store.get(1).is_some() && store.get(2).is_some(),
            "c keeps the data"
        );
    }

    // C on the re-placed tall image frees what was visible; the
    // scrollback image's pixels and cells stand.
    terminal.note_apc_payload(&apc("a=p,i=2,c=1,r=1", b""));
    terminal.note_apc_payload(&apc("a=d,d=C", b""));
    assert_eq!(
        cell_image(&terminal, 23, 0),
        None,
        "the re-placed cells swept"
    );
    {
        let store = terminal.images.lock().unwrap();
        assert!(
            store.get(2).is_none(),
            "the freed visible image's pixels go"
        );
        assert!(
            store.get(1).is_some(),
            "the scrollback image's pixels stand"
        );
    }
}

#[test]
fn kitty_region_and_unknown_deletes_are_ignored() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("a=T,f=100,c=1,r=1", &png_bytes(8, 8)));

    for ignored in ["d=x0,0,1,1", "d=y", "d=z5", "d=q"] {
        terminal.note_apc_payload(&apc(&format!("a=d,{ignored}"), b""));
    }

    assert_eq!(cell_image(&terminal, 0, 0), Some(1), "nothing was deleted");
    assert!(terminal.images.lock().unwrap().get(1).is_some());
}

#[test]
fn kitty_command_sequences_never_open_a_session() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("a=q,i=1,m=1", b""));
    terminal.note_apc_payload(&apc("m=0", b""));
    assert!(terminal.kitty_chunks.is_empty());
    assert!(terminal.images.lock().unwrap().get(1).is_none());
}

// ===== Kitty z-layers (kitty-images spec) =====

#[test]
fn store_layers_default_beneath_and_record_on_ask() {
    let mut store = super::ImageStore::default();
    store.insert(
        1,
        image::RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 255])),
    );
    assert_eq!(store.layer_of(1), -1, "an unrecorded id draws beneath");

    store.set_layer(1, 2);
    assert_eq!(store.layer_of(1), 2, "above the glyphs at z>=0");
    store.set_layer(1, -3);
    assert_eq!(store.layer_of(1), -3, "beneath at z<0");

    // A freed or evicted id's layer dies with its pixels.
    store.remove(1);
    assert_eq!(store.layer_of(1), -1);

    store.insert(
        2,
        image::RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 255])),
    );
    store.set_layer(2, 1);
    store.clear();
    assert_eq!(store.layer_of(2), -1, "the free-all takes the layers too");
}

#[test]
fn kitty_transmissions_record_the_asked_layer() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("i=1,a=T,f=100,c=1,r=1,z=3", &png_bytes(8, 8)));
    terminal.note_apc_payload(&apc("i=2,a=T,f=100,c=1,r=1,z=-1", &png_bytes(8, 8)));
    terminal.note_apc_payload(&apc("i=3,a=T,f=100,c=1,r=1", &png_bytes(8, 8)));

    let store = terminal.images.lock().unwrap();
    assert_eq!(store.layer_of(1), 3, "an explicit z>=0 asks above");
    assert_eq!(store.layer_of(2), -1, "an explicit z<0 asks beneath");
    assert_eq!(store.layer_of(3), -1, "an absent z reads as beneath");
}

#[test]
fn kitty_placements_carry_and_inherit_their_layer() {
    let mut terminal = image_terminal();
    terminal.note_apc_payload(&apc("i=7,f=100,z=4", &png_bytes(8, 8)));
    terminal.note_apc_payload(&apc("a=p,i=7,p=3,c=1,r=1", b""));
    terminal.note_apc_payload(&apc("a=p,i=7,p=4,c=1,r=1,z=-2", b""));

    let store = terminal.images.lock().unwrap();
    let cells_three = terminal.kitty_placements.get(&3).map(|p| p.cells);
    let cells_four = terminal.kitty_placements.get(&4).map(|p| p.cells);
    assert_eq!(
        store.layer_of(cells_three.expect("placement 3 seated")),
        4,
        "an absent z inherits its image's layer"
    );
    assert_eq!(
        store.layer_of(cells_four.expect("placement 4 seated")),
        -2,
        "the placement's own z overrides"
    );
}

#[test]
fn iterm_images_stay_beneath_by_default() {
    let mut terminal = image_terminal();
    let wire = b64(&png_bytes(8, 8));
    let mut name = b"name=aGk=".to_vec();
    name.push(b':');
    name.extend_from_slice(&wire);
    terminal.note_inline_image(&[b"File=inline=1".to_vec(), b"width=4".to_vec(), name]);

    assert_eq!(
        terminal.images.lock().unwrap().layer_of(1),
        -1,
        "iterm images never enter the layer map"
    );
}
