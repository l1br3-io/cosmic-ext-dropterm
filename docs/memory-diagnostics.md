# Memory diagnostics

DropTerm watches its own RAM. Every second, always on, in every
build — release included — the process reads its own `/proc` numbers
and appends one plain-text row to a bounded rotating log under XDG
state. The 2026-11-17 incident ([a 47.1 GB OOM in ~12 s of resize
churn](research/ram-leak.md)) was found only because a probe
happened to be running; this watch exists so the next one captures
itself. Post-fix, a **flat curve is the regression signal** — a
climbing one is a captured incident with timestamps.

The sampler is std-only (no new dependencies — the [tooling
record](research/memory-tracking-tooling.md) priced the crates and
rejected them), rides the UI loop's own 100 ms tick (every 10th
tick), and is failure-safe by construction: a missing `/proc` field
or an unwritable state dir skips that one row — telemetry never
panics the app. Nothing but timestamps, counters, and sizes ever
enters the log.

## Where the log lives

    $XDG_STATE_HOME/io.github.l1br3_io.dropterm/memory.log      # active file
    $XDG_STATE_HOME/io.github.l1br3_io.dropterm/memory.log.1    # the other slot

`XDG_STATE_HOME` defaults to `~/.local/state`; only an absolute
override counts. A dev build (the `dev-identity` feature) carries
its own id — `…dropterm.Dev/memory.log` — and never shares the
release's files.

**Rotation:** two-file A/B with a hard 2 048-row cap per file (tens
of KB — 86 KB for a full slot as measured). When the active file
reaches its cap, the next row opens the *other* file fresh (truncating
it), so the oldest quarter of the window is what gets dropped —
never the fresh sample. On startup the sampler keeps the newer file
and opens the older one, so the previous session's tail survives
restarts. The window is therefore 2 048–4 096 rows ≈ 34–68 minutes
of wall clock at 1 Hz.

## The row

Each file starts with one `#` header line (the schema below), then
fixed whitespace-separated columns — one row per second:

    # ts_s vm_rss_kb rss_anon_kb rss_file_kb vm_swap_kb pss_kb pss_anon_kb pss_file_kb pss_swap_kb children_n children_rss_kb
    1765852804 51200 40960 10240 128 - - - - 2 30720
    1765852834 51904 41472 10432 128 48896 40960 7936 128 2 30720

| # | Column | Meaning |
|---|--------|---------|
| 1 | `ts_s` | unix time, seconds |
| 2 | `vm_rss_kb` | resident set, kB (`VmRSS` from `/proc/self/status`) |
| 3 | `rss_anon_kb` | anonymous resident (`RssAnon`) — the heap-ish share |
| 4 | `rss_file_kb` | file-backed resident (`RssFile`) — mappings, fonts |
| 5 | `vm_swap_kb` | swapped-out (`VmSwap`) |
| 6 | `pss_kb` | proportional set (`smaps_rollup` `Pss`); `-` on 29 of 30 rows |
| 7 | `pss_anon_kb` | `Pss_Anon`, same cadence |
| 8 | `pss_file_kb` | `Pss_File`, same cadence |
| 9 | `pss_swap_kb` | the rollup's `Swap` (not `SwapPss`), same cadence |
| 10 | `children_n` | live tracked PTY child shells (dead pids don't count) |
| 11 | `children_rss_kb` | summed child `VmRSS`, kB |

The four `pss_*` columns ride every 30th sample (~30 s): the
rollup read is the kernel's cheap aggregate, and RSS overcounts
shared pages — when `vm_rss_kb` and `pss_kb` disagree, PSS is the
fairer number. A missing row means the sample was skipped
(failure-safe), not that time stopped; read gaps as "unreadable
that second", nothing more.

## Reading it — one-liners

Run as-is against a real log; `LOGDIR` first, everything after is
copy-paste:

    LOGDIR=~/.local/state/io.github.l1br3_io.dropterm

The last readings, both files, newest last:

    tail -n 5 $LOGDIR/memory.log $LOGDIR/memory.log.1

The whole window's `vm_rss_kb` curve, chronological (the two files'
order on disk is not time order):

    grep -hv '^#' $LOGDIR/memory.log $LOGDIR/memory.log.1 | sort -k1n | awk '{print $1, $2}'

The verdict in one line — min / max / last resident (kB):

    grep -hv '^#' $LOGDIR/memory.log $LOGDIR/memory.log.1 | awk 'NR==1{min=$2} $2<min{min=$2} $2>max{max=$2} {last=$2} END{print "min_kB",min,"max_kB",max,"last_kB",last}'

Only the rollup rows (the PSS truth, every ~30 s):

    grep -hv '^#' $LOGDIR/memory.log $LOGDIR/memory.log.1 | awk '$6!="-"'

Children: is it the app or what it's running? (`children_n` then
`children_rss_kb`):

    grep -hv '^#' $LOGDIR/memory.log $LOGDIR/memory.log.1 | awk '{print $1, $10, $11}'

Reading the shape: a one-time climb that plateaus is allocator
bedding-in (glibc holds freed memory under its trim threshold — the
[resource audit](research/resource-audit.md) states the app's
bounds); a steady climb across the window with `children_n` flat is
the app, and matches the [known-issues daily-growth
thread](known-issues.md) — a climb with `children_rss_kb` climbing
is a child process, and the app is innocent.

## When the curve climbs — the capture ladder

Escalating observation, owner-run (each step needs nothing but the
tools named; all are process-level, no compositor interaction):

1. **Observe the shape live** — psrecord plots RSS+CPU of the
   process *and* its children while the growth reproduces:
   `psrecord $(pidof dropterm) --interval 2 --include-children --log /tmp/dt-ram.log --plot /tmp/dt-ram.png`
   (one-time `pip install psrecord[plot]`; the tool's own README
   calls it experimental).
2. **One point read, split anon/file** — the same rollup the
   sampler reads, now: `grep -E '^(Rss|Pss|Pss_Anon|Pss_File|Swap):' /proc/$(pidof dropterm)/smaps_rollup`
3. **Capture allocations** — relaunch under the profiler, full run:
   `heaptrack --follow-children dropterm` (from a terminal); or
   attach to the live instance: `heaptrack --pid $(pidof dropterm)`
   (injection goes through GDB — gdb must be installed; a fresh
   launch has no such requirement). Then `heaptrack_gui
   /tmp/heaptrack.dropterm.*.gz` for the flame graph and
   cost-over-time, or `heaptrack_print` for a text top-10. Install:
   `apt install heaptrack heaptrack-gui`. Note: heaptrack's dump is
   written at exit — an OOM SIGKILL loses it, which is exactly why
   the sampler's log (which survives everything) runs first.
4. **If it died by OOM** — the kernel's own account:
   `journalctl -k --since today` and read the
   `Killed process … anon-rss:…kB` line (the tooling record's
   finding 7 has the full field table).
5. **Fragmentation specifically** (RSS high, heaptrack shows the
   heap small and freed) — bytehound with
   `MEMORY_PROFILER_CULL_TEMPORARY_ALLOCATIONS=1` over a long
   window, then its web GUI's RSS views.

The full reasoning — why heaptrack over massif and bytehound, why
no sampler crate, the glibc trim-threshold ratchet — is the
[tooling record](research/memory-tracking-tooling.md); the incident
this watch exists for is [the ram-leak
record](research/ram-leak.md).
