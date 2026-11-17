use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use super::*;

/// A per-test scratch directory under the system temp dir with a
/// process- and test-unique name, removed (best-effort) on drop even
/// when an assertion fails (session/tests.rs's std-only shape).
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let unique = format!(
            "dropterm-mem-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("create scratch dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// The env-mutating tests (`state_dir`) share one lock so they cannot
/// observe each other's overrides; the window is held only for the
/// duration of one call.
fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().expect("env lock poisoned")
}

/// A realistic `/proc/self/status` excerpt with the four sampled
/// fields amid neighbors (including the kB-less `Name:` line and
/// fields whose names prefix each other's, like VmRSS/VmPeak).
const STATUS_FIXTURE: &str = "\
Name:\tdropterm
Umask:\t0022
State:\tS (sleeping)
Tgid:\t4242
Pid:\t4242
VmPeak:\t  2000000 kB
VmSize:\t  1500000 kB
VmRSS:\t     51200 kB
RssAnon:\t   40960 kB
RssFile:\t   10240 kB
RssShmem:\t     0 kB
VmData:\t  1200000 kB
VmSwap:\t      128 kB
Threads:\t12
";

/// A realistic `smaps_rollup` excerpt: the four sampled fields amid
/// same-prefixed neighbors (`SwapPss` must not satisfy `Swap`).
const ROLLUP_FIXTURE: &str = "\
Rss\t\t   51200 kB
Pss\t\t   48896 kB
Pss_Anon\t\t   40960 kB
Pss_File\t\t    7936 kB
Pss_Shmem\t\t       0 kB
Swap\t\t     128 kB
SwapPss\t\t       0 kB
Locked\t\t       0 kB
";

fn status_sample() -> StatusSample {
    StatusSample {
        vm_rss_kb: 51200,
        rss_anon_kb: 40960,
        rss_file_kb: 10240,
        vm_swap_kb: 128,
    }
}

fn rollup_sample() -> RollupSample {
    RollupSample {
        pss_kb: 48896,
        pss_anon_kb: 40960,
        pss_file_kb: 7936,
        pss_swap_kb: 128,
    }
}

fn children(n: u64, rss_kb: u64) -> ChildrenStats {
    ChildrenStats { count: n, rss_kb }
}

fn data_rows(path: &Path) -> Vec<String> {
    let text = fs::read_to_string(path).expect("read log file");
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn log(dir: &Path) -> PathBuf {
    dir.join("memory.log")
}

fn log_alt(dir: &Path) -> PathBuf {
    dir.join("memory.log.1")
}

#[test]
fn status_fixture_parses_to_exact_numbers() {
    assert_eq!(parse_status(STATUS_FIXTURE), Some(status_sample()));
}

#[test]
fn status_missing_required_field_is_none() {
    let truncated = STATUS_FIXTURE.replace("VmSwap:\t      128 kB\n", "");
    assert_eq!(parse_status(&truncated), None);
}

#[test]
fn rollup_fixture_parses_to_exact_numbers() {
    assert_eq!(parse_smaps_rollup(ROLLUP_FIXTURE), Some(rollup_sample()));
}

#[test]
fn rollup_missing_required_field_is_none() {
    let truncated = ROLLUP_FIXTURE.replace("Pss_Anon\t\t   40960 kB\n", "");
    assert_eq!(parse_smaps_rollup(&truncated), None);
}

#[test]
fn rollup_swap_does_not_match_swappss() {
    // Swap removed, SwapPss kept: the exact-token rule must refuse the
    // prefix neighbor.
    let swapped = ROLLUP_FIXTURE.replace("Swap\t\t     128 kB\n", "");
    assert_eq!(parse_smaps_rollup(&swapped), None);
}

#[test]
fn header_is_the_pinned_schema() {
    assert_eq!(
        HEADER,
        "# ts_s vm_rss_kb rss_anon_kb rss_file_kb vm_swap_kb \
         pss_kb pss_anon_kb pss_file_kb pss_swap_kb children_n children_rss_kb"
    );
}

#[test]
fn row_with_rollup_has_eleven_numeric_columns() {
    let row = format_row(
        1_731_862_804,
        status_sample(),
        Some(rollup_sample()),
        children(2, 30720),
    );
    let fields: Vec<&str> = row.split_whitespace().collect();
    assert_eq!(fields.len(), 11);
    assert!(
        fields.iter().all(|f| f.parse::<u64>().is_ok()),
        "row: {row}"
    );
    assert_eq!(fields[0], "1731862804");
    assert_eq!(fields[9], "2");
    assert_eq!(fields[10], "30720");
}

#[test]
fn row_without_rollup_dashes_the_pss_columns() {
    let row = format_row(1, status_sample(), None, children(0, 0));
    let fields: Vec<&str> = row.split_whitespace().collect();
    assert_eq!(fields.len(), 11);
    for field in &fields[5..9] {
        assert_eq!(*field, "-", "rollup column carried a value: {row}");
    }
}

#[test]
fn record_writes_header_then_rows_in_order() {
    let dir = TempDir::new("record-order");
    let mut sampler = Sampler::new(dir.path());
    assert!(sampler.record(100, status_sample(), None, children(0, 0)));
    assert!(sampler.record(101, status_sample(), Some(rollup_sample()), children(1, 5)));
    assert!(sampler.record(102, status_sample(), None, children(0, 0)));

    let text = fs::read_to_string(log(dir.path())).expect("read log");
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some(HEADER));
    let rows: Vec<&str> = lines.collect();
    assert_eq!(rows.len(), 3);
    assert!(rows[0].starts_with("100 "));
    assert!(rows[1].starts_with("101 "));
    assert!(rows[2].starts_with("102 "));
}

#[test]
fn rotation_bounds_are_pinned_numbers() {
    assert_eq!(ROW_CAP, 2048);
    assert_eq!(ROLLUP_EVERY, 30);
}

#[test]
fn rotation_drops_oldest_and_never_the_fresh_row() {
    let dir = TempDir::new("rotation");
    let mut sampler = Sampler::new(dir.path());
    sampler.cap = 4;
    for ts in 1..=9u64 {
        assert!(
            sampler.record(ts, status_sample(), None, children(0, 0)),
            "the fresh row (ts {ts}) was dropped"
        );
    }
    // File A carried rows 1-4, B took 5-8 at the first flip, and the
    // second flip truncated A to the fresh row 9: the oldest quarter
    // is gone, both files hold at most the cap, the fresh one lives.
    let a_rows = data_rows(&log(dir.path()));
    assert_eq!(a_rows.len(), 1);
    assert!(a_rows[0].starts_with("9 "));
    let b_rows = data_rows(&log_alt(dir.path()));
    assert_eq!(b_rows.len(), 4);
    assert!(b_rows[0].starts_with("5 "));
    assert!(b_rows[3].starts_with("8 "));
}

#[test]
fn startup_preserves_the_only_existing_log() {
    let dir = TempDir::new("startup-one");
    fs::write(log(dir.path()), format!("{HEADER}\nseeded\n")).expect("seed A");
    let before = fs::read_to_string(log(dir.path())).expect("read seeded A");

    let mut sampler = Sampler::new(dir.path());
    assert!(sampler.record(7, status_sample(), None, children(0, 0)));

    // The seeded file survives byte-for-byte; the sample opened the
    // other slot.
    assert_eq!(
        fs::read_to_string(log(dir.path())).expect("re-read A"),
        before
    );
    let alt = data_rows(&log_alt(dir.path()));
    assert_eq!(alt.len(), 1);
    assert!(alt[0].starts_with("7 "));
}

#[test]
fn startup_rotates_to_the_older_file_when_both_exist() {
    let dir = TempDir::new("startup-both");
    fs::write(log(dir.path()), format!("{HEADER}\nolder\n")).expect("seed A");
    fs::write(log_alt(dir.path()), format!("{HEADER}\nnewer\n")).expect("seed B");
    let b_before = fs::read_to_string(log_alt(dir.path())).expect("read seeded B");

    let mut sampler = Sampler::new(dir.path());
    assert!(sampler.record(9, status_sample(), None, children(0, 0)));

    // B was written last (mtime >= A), so it is the survivor and the
    // sample opened A — the older file — fresh.
    assert_eq!(
        fs::read_to_string(log_alt(dir.path())).expect("re-read B"),
        b_before
    );
    let a_rows = data_rows(&log(dir.path()));
    assert_eq!(a_rows.len(), 1);
    assert!(a_rows[0].starts_with("9 "));
}

#[test]
fn unwritable_dir_skips_the_sample_without_panicking() {
    let dir = TempDir::new("unwritable");
    let blocked = dir.path().join("blocked");
    fs::write(&blocked, "a regular file where the dir should go").expect("block");

    let mut sampler = Sampler::new(&blocked);
    assert!(!sampler.record(1, status_sample(), None, children(0, 0)));
    assert!(!sampler.record(2, status_sample(), None, children(0, 0)));
}

#[test]
fn state_dir_honors_absolute_xdg_override() {
    let _env = env_lock();
    let tmp = TempDir::new("xdg-override");
    let saved_xdg = std::env::var_os("XDG_STATE_HOME");
    let saved_home = std::env::var_os("HOME");
    unsafe {
        std::env::set_var("XDG_STATE_HOME", tmp.path());
        std::env::set_var("HOME", "/nonexistent-dropterm-home");
    }
    assert_eq!(state_dir(), tmp.path().join(crate::core::APP_ID));
    unsafe {
        match saved_xdg {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
        match saved_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}

#[test]
fn state_dir_falls_back_to_home_local_state() {
    let _env = env_lock();
    let tmp = TempDir::new("xdg-fallback");
    let saved_xdg = std::env::var_os("XDG_STATE_HOME");
    let saved_home = std::env::var_os("HOME");
    unsafe {
        std::env::remove_var("XDG_STATE_HOME");
        std::env::set_var("HOME", tmp.path());
    }
    assert_eq!(
        state_dir(),
        tmp.path().join(".local/state").join(crate::core::APP_ID)
    );
    unsafe {
        match saved_xdg {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
        match saved_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}

#[test]
fn relative_xdg_override_is_ignored() {
    let _env = env_lock();
    let tmp = TempDir::new("xdg-relative");
    let saved_xdg = std::env::var_os("XDG_STATE_HOME");
    let saved_home = std::env::var_os("HOME");
    unsafe {
        std::env::set_var("XDG_STATE_HOME", "relative/state");
        std::env::set_var("HOME", tmp.path());
    }
    assert_eq!(
        state_dir(),
        tmp.path().join(".local/state").join(crate::core::APP_ID)
    );
    unsafe {
        match saved_xdg {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
        match saved_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn children_stats_counts_live_pids_and_sums_rss() {
    let own = std::process::id();
    let stats = children_stats(&[own, u32::MAX]);
    assert_eq!(stats.count, 1, "the dead pid must not count");
    assert!(stats.rss_kb > 0, "own RSS must read as a live number");
}

#[cfg(target_os = "linux")]
#[test]
fn sample_takes_a_real_own_process_reading() {
    let dir = TempDir::new("sample-live");
    let mut sampler = Sampler::new(dir.path());
    assert!(sampler.sample(&[]));
    let rows = data_rows(&log(dir.path()));
    assert_eq!(rows.len(), 1);
    // Own-process numbers: RSS in kB must be plausible (the test
    // binary holds megabytes) and the rollup columns dashed — the
    // first sample is not the 30th.
    let fields: Vec<&str> = rows[0].split_whitespace().collect();
    let vm_rss: u64 = fields[1].parse().expect("vm_rss_kb numeric");
    assert!(vm_rss > 1024, "implausible own VmRSS: {vm_rss} kB");
    assert_eq!(fields[5], "-");
}

#[test]
fn tick_cadence_fires_exactly_once_per_ten_ticks() {
    assert_eq!(SAMPLE_EVERY_TICKS, 10);
    // One firing per 10-tick window, on the window's last tick —
    // never two inside one window, never a silent window.
    for tick in 1..=30u64 {
        assert_eq!(tick_samples(tick), tick.is_multiple_of(10), "tick {tick}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn rollup_rides_every_thirtieth_sample() {
    let dir = TempDir::new("rollup-cadence");
    let mut sampler = Sampler::new(dir.path());
    for _ in 0..31 {
        assert!(sampler.sample(&[]));
    }
    let rows = data_rows(&log(dir.path()));
    assert_eq!(rows.len(), 31);
    let is_rollup = |row: &String| row.split_whitespace().nth(5) != Some("-");
    let rollups: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| is_rollup(row))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(rollups, vec![29], "exactly the 30th sample carries rollup");
}

/// The rt-2 agent-run capped smoke (headless, the sanctioned
/// pattern): the shell's 100 ms loop faked by its counter — real
/// own-process `/proc` reads, real log files, no sleeps — driven
/// past the rotation cap, asserting the on-disk log whole.
#[cfg(target_os = "linux")]
#[test]
fn smoke_the_tick_loop_produces_the_bounded_log() {
    let dir = TempDir::new("rt2-smoke");
    let mut sampler = Sampler::new(dir.path());
    // 20 600 ticks of the 100 ms cadence (~34 min) -> 2 060 samples:
    // the first slot fills to its cap and the tail spills into the
    // second.
    let ticks = 20_600u64;
    let mut writes = 0u64;
    for tick in 1..=ticks {
        if tick_samples(tick) {
            assert!(sampler.sample(&[]), "the fresh sample was dropped");
            writes += 1;
        }
    }
    assert_eq!(writes, 2_060);

    let a_rows = data_rows(&log(dir.path()));
    let b_rows = data_rows(&log_alt(dir.path()));
    assert_eq!(a_rows.len(), ROW_CAP, "slot A holds exactly its cap");
    assert_eq!(b_rows.len(), 12, "the spill lands in slot B");
    let is_rollup = |row: &String| row.split_whitespace().nth(5) != Some("-");
    let rollups = a_rows.iter().filter(|row| is_rollup(row)).count()
        + b_rows.iter().filter(|row| is_rollup(row)).count();
    assert_eq!(rollups, 68, "every 30th of 2 060 samples carries rollup");
    for row in a_rows.iter().chain(&b_rows) {
        assert_eq!(row.split_whitespace().count(), 11, "malformed row: {row}");
    }
    let a_bytes = fs::metadata(log(dir.path())).expect("stat A").len();
    let b_bytes = fs::metadata(log_alt(dir.path())).expect("stat B").len();
    assert!(
        a_bytes < 1024 * 1024 && b_bytes < 1024 * 1024,
        "log out of bound"
    );
    println!(
        "rt-2 smoke: ticks={ticks} writes={writes} A={} rows/{a_bytes}B B={} rows/{b_bytes}B rollup_rows={rollups}",
        a_rows.len(),
        b_rows.len(),
    );
}
