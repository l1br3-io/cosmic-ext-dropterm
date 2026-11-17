//! The app watches its own RAM (ram-telemetry): an always-on
//! own-process sampler writing a bounded rotating log under XDG
//! state, so daily-use growth shows as a curve instead of a crash —
//! post-`ram-leak`, a flat curve is the regression signal. Std-only
//! by design (the tooling record's no-new-crate verdict) and
//! failure-safe throughout: a missing `/proc` field or an unwritable
//! state dir skips the sample, telemetry never panics the app.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The hard per-file row cap of the rotating pair (ram-telemetry's
/// pinned bound: two files, oldest dropped, never the fresh sample).
pub const ROW_CAP: usize = 2048;

/// `smaps_rollup` rides every Nth sample (the tooling record's
/// cadence: the expensive read once per 30 s of 1 Hz sampling).
pub const ROLLUP_EVERY: u64 = 30;

/// The log's self-describing schema — fixed whitespace-separated
/// columns, the `#` line skipped by `grep -v '^#'` — so an agent
/// extracts the log with std tooling and no parsing preamble.
pub const HEADER: &str = "# ts_s vm_rss_kb rss_anon_kb rss_file_kb vm_swap_kb \
                          pss_kb pss_anon_kb pss_file_kb pss_swap_kb children_n children_rss_kb";

const LOG_A: &str = "memory.log";
const LOG_B: &str = "memory.log.1";

/// The own-process `/proc/self/status` reading (kB, the four fields
/// the tooling record sampled at 1 Hz).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusSample {
    pub vm_rss_kb: u64,
    pub rss_anon_kb: u64,
    pub rss_file_kb: u64,
    pub vm_swap_kb: u64,
}

/// The every-30th `smaps_rollup` reading (kB): the proportionally
/// counted truth under RSS, where shared pages stop hiding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollupSample {
    pub pss_kb: u64,
    pub pss_anon_kb: u64,
    pub pss_file_kb: u64,
    pub pss_swap_kb: u64,
}

/// The children aggregate from the caller's tracked PTY child pids:
/// live count and summed RSS (kB). A dead pid reads nothing and does
/// not count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChildrenStats {
    pub count: u64,
    pub rss_kb: u64,
}

/// One `Name: 123 kB` field, matched by the exact first token —
/// `Swap` must not satisfy `SwapPss`. The colon is optional because
/// the two sources differ: `status` writes `VmRSS:`, `smaps_rollup`
/// writes `Rss` bare.
fn kb_field(text: &str, field: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let name = parts.next()?;
        (name.strip_suffix(':').unwrap_or(name) == field)
            .then(|| parts.next())
            .flatten()?
            .parse::<u64>()
            .ok()
    })
}

pub fn parse_status(text: &str) -> Option<StatusSample> {
    Some(StatusSample {
        vm_rss_kb: kb_field(text, "VmRSS")?,
        rss_anon_kb: kb_field(text, "RssAnon")?,
        rss_file_kb: kb_field(text, "RssFile")?,
        vm_swap_kb: kb_field(text, "VmSwap")?,
    })
}

pub fn parse_smaps_rollup(text: &str) -> Option<RollupSample> {
    Some(RollupSample {
        pss_kb: kb_field(text, "Pss")?,
        pss_anon_kb: kb_field(text, "Pss_Anon")?,
        pss_file_kb: kb_field(text, "Pss_File")?,
        pss_swap_kb: kb_field(text, "Swap")?,
    })
}

fn read_status() -> Option<StatusSample> {
    parse_status(&fs::read_to_string("/proc/self/status").ok()?)
}

fn read_smaps_rollup() -> Option<RollupSample> {
    parse_smaps_rollup(&fs::read_to_string("/proc/self/smaps_rollup").ok()?)
}

/// One bounded status read per tracked child, no `/proc` scanning: a
/// dead or unreadable pid contributes nothing.
fn children_stats(pids: &[u32]) -> ChildrenStats {
    let mut stats = ChildrenStats::default();
    for &pid in pids {
        let path = Path::new("/proc").join(pid.to_string()).join("status");
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        if let Some(kb) = kb_field(&text, "VmRSS") {
            stats.count += 1;
            stats.rss_kb = stats.rss_kb.saturating_add(kb);
        }
    }
    stats
}

/// `$XDG_STATE_HOME/<app id>` — the config store's XDG-override
/// pattern (only an absolute override counts; the fallback is
/// `~/.local/state`).
pub fn state_dir() -> PathBuf {
    // XDG base-directory specification: only an absolute
    // XDG_STATE_HOME counts; anything else (unset, empty, relative)
    // falls back to ~/.local/state.
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|base| base.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")));
    match base {
        Some(base) => base.join(crate::core::APP_ID),
        None => PathBuf::from(".local/state").join(crate::core::APP_ID),
    }
}

/// The A/B slot of the rotating pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    A,
    B,
}

impl Slot {
    fn name(self) -> &'static str {
        match self {
            Slot::A => LOG_A,
            Slot::B => LOG_B,
        }
    }

    fn other(self) -> Self {
        match self {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }
}

/// The sampler: no in-memory history (write and forget — the log
/// file is the only history), only the active slot, its data-row
/// count, and the sample counter driving the rollup cadence.
pub struct Sampler {
    dir: PathBuf,
    active: Slot,
    rows: usize,
    cap: usize,
    periods: u64,
}

impl Sampler {
    pub fn new(dir: &Path) -> Self {
        fs::create_dir_all(dir).ok();
        // Preserve the newest slot across restarts: the active file
        // is the most recently written one, already treated as full
        // so the first sample opens the other slot fresh. A missing
        // file counts as the oldest; a timestamp tie favors B (the
        // conventional second slot), keeping the choice total.
        let a = dir.join(LOG_A);
        let b = dir.join(LOG_B);
        let newer = match (
            fs::metadata(&a).and_then(|m| m.modified()),
            fs::metadata(&b).and_then(|m| m.modified()),
        ) {
            (Ok(a_time), Ok(b_time)) if a_time > b_time => Slot::A,
            (Ok(_) | Err(_), Ok(_)) => Slot::B,
            (Ok(_), Err(_)) => Slot::A,
            (Err(_), Err(_)) => {
                return Self {
                    dir: dir.to_owned(),
                    active: Slot::A,
                    rows: 0,
                    cap: ROW_CAP,
                    periods: 0,
                };
            }
        };
        Self {
            dir: dir.to_owned(),
            active: newer,
            rows: ROW_CAP,
            cap: ROW_CAP,
            periods: 0,
        }
    }

    /// One sample period: read the own-process status (plus rollup
    /// every [`ROLLUP_EVERY`]th), aggregate the handed-in children,
    /// and write the row. `false` means the sample was skipped.
    pub fn sample(&mut self, children: &[u32]) -> bool {
        let Some(status) = read_status() else {
            return false;
        };
        self.periods = self.periods.saturating_add(1);
        let rollup = self
            .periods
            .is_multiple_of(ROLLUP_EVERY)
            .then(read_smaps_rollup)
            .flatten();
        self.record(unix_now_s(), status, rollup, children_stats(children))
    }

    fn record(
        &mut self,
        ts_s: u64,
        status: StatusSample,
        rollup: Option<RollupSample>,
        children: ChildrenStats,
    ) -> bool {
        if self.rows >= self.cap {
            self.active = self.active.other();
            self.rows = 0;
        }
        let path = self.dir.join(self.active.name());
        // A fresh slot starts with its header; a header-write failure
        // leaves `rows` at 0 so the next attempt reinitializes.
        if self.rows == 0 && fs::write(&path, format!("{HEADER}\n")).is_err() {
            return false;
        }
        let Ok(mut file) = OpenOptions::new().append(true).open(&path) else {
            return false;
        };
        let row = format_row(ts_s, status, rollup, children);
        if file.write_all(row.as_bytes()).is_err() {
            return false;
        }
        self.rows += 1;
        true
    }
}

fn format_row(
    ts_s: u64,
    status: StatusSample,
    rollup: Option<RollupSample>,
    children: ChildrenStats,
) -> String {
    let [pss, pss_anon, pss_file, pss_swap] = match rollup {
        Some(rollup) => [
            rollup.pss_kb,
            rollup.pss_anon_kb,
            rollup.pss_file_kb,
            rollup.pss_swap_kb,
        ]
        .map(|kb| kb.to_string()),
        None => ["-", "-", "-", "-"].map(str::to_owned),
    };
    format!(
        "{} {} {} {} {} {pss} {pss_anon} {pss_file} {pss_swap} {} {}\n",
        ts_s,
        status.vm_rss_kb,
        status.rss_anon_kb,
        status.rss_file_kb,
        status.vm_swap_kb,
        children.count,
        children.rss_kb,
    )
}

fn unix_now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests;
