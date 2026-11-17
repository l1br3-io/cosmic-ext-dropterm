# Which tools catch DropTerm's runaway memory? — heap profilers, in-process samplers, and the OOM forensics around them

Date: 2026-11-17 · Record: immutable — corrections land as new rows,
this text never changes. All external facts pinned to their primary
source at fetch date 2026-11-17.

## TL;DR

**heaptrack is the primary external instrument** for the capped
headless resize-churn repro (`systemd-run --user --scope` with
`MemoryHigh` below `MemoryMax`, `heaptrack --follow-children` on
`cargo test`, deadline-set so the dump finalizes), with **dhat's
testing-mode `HeapStats` polled mid-run** as the OOM-proof in-binary
fallback when the repro must actually reach the kill (stderr survives
SIGKILL via the journal), and **bytehound in server-streaming mode**
reserved for fragmentation questions because it is the only one of the
three whose data leaves the process live. The **always-on sampler needs
no new dependency**: `/proc/self/status` (`VmRSS`/`RssAnon`/`RssFile`/
`VmSwap`) at 1 Hz plus `smaps_rollup` every 30th tick — kernel-guaranteed
cheap aggregated reads — into a two-file bounded rotation under
`XDG_STATE_HOME`; `memory-stats` is rejected for that duty by its own
docs (it reads the expensive full `smaps`), `sysinfo` as built for
whole-system refreshes, `procfs` 0.18 as the optional typed reader for
`SmapsRollup`. For daily-use incidents the owner-run recipe is
psrecord/`smaps_rollup` for observation, then a heaptrack relaunch (or
`heaptrack --pid` attach, which needs gdb). The peer survey answers the
repro's mechanism class: every surveyed engine bounds scrollback with a
ring, and the documented GB-scale incidents are reflow/reflow-adjacent
(alacritty #5438 allocator retention; wez #1342's rewrap-cost warning)
or SIGWINCH redraw storms (Claude Code #39022: 21 GB in ~6 s), matching
DropTerm's 47.1 GB-in-12 s churn exactly in shape.

## Method

Primary sources only: upstream manuals and READMEs (valgrind, heaptrack,
bytehound book, dhat), the kernel's own proc/cgroup-v2 docs and
`mm/oom_kill.c` format strings, systemd man pages, the four terminal
engines' docs/changelogs/issue threads, and crates.io's API for
maintenance status. Every behavioral claim cited inline; maintenance
dates are the crates.io/GitHub release dates as fetched 2026-11-17.

## Findings

### 1. The three external heap profilers, compared

| | heaptrack 1.5.0 (KDE) | valgrind/massif 3.27.1 | bytehound 0.11.0 (koute) |
|---|---|---|---|
| Captures | every alloc/dealloc + full stack traces; **RSS sampled every 10 ms** by a timer thread | heap via `malloc`/`new` intercepts (useful + admin bytes); stacks opt-in; `--pages-as-heap=yes` covers **all** memory (mmap/brk) | every alloc/dealloc + full stack traces; **RSS and address-space graphs** (`graph().show_rss()`) |
| Overhead | "you only pay a price when you allocate memory"; threads "are not serialized" | the program "will execute (slowly)"; stack profiling "slows Massif down greatly" | custom unwinding "orders of magnitude faster in some cases"; shadow stack on by default |
| Output | `/tmp/heaptrack.APP.PID.gz` | `massif.out.<pid>` (plain text) | `memory-profiling_%e_%t_%p.dat` |
| Analysis | `heaptrack_gui` (flame graph, cost-over-time), `heaptrack_print` (top-10: calls/peak/temporaries; massif + flamegraph.pl export) | `ms_print`, massif-visualizer | web GUI (`bytehound server …*.dat` → :8080), Rhai scripting, export to heaptrack/JSON/flamegraph |
| OOM-killed child | **loses the dump** (written once at exit) | monitor commands can pull snapshots mid-run via gdbserver | **loses a file dump**, but `MEMORY_PROFILER_ENABLE_SERVER=1` streams data out-of-process live |
| Maintenance | 1.5.0 released 2023-09-23 (apps.kde.org); master active (1,644 commits) | stable suite, current 3.27.1 | release 0.11.0 = 2022-11-23 (GitHub API); nothing newer |

- **heaptrack** (github.com/KDE/heaptrack README): "traces all memory
  allocations and annotates these events with stack traces"; the
  background thread records "the timestamp and the RSS every 10ms";
  run as `heaptrack <app and parameters>`, `--follow-children` gives
  each forked/exec'd child its own `heaptrack.APP.PID.CHILD_PID.zst`
  (a `cargo test` binary that spawns bash is exactly this shape);
  `heaptrack --pid $(pidof app)` attaches to a running process "via
  GDB" injection; `--raw` records for offline symbolization, later
  `heaptrack --interpret <file> --sysroot/--debug-paths/--extra-paths`.
  Known issue: "Old gold linker + libunwind → broken backtraces"; long
  profiles load slowly in the GUI.
- **massif** (valgrind.org ms-manual): collates "the stack trace of
  every single allocation point" into per-snapshot trees; snapshots
  back off from one-per-allocation to a cap (`--max-snapshots`, 100
  default, half discarded at the cap); peak is recorded only after a
  deallocation and "within 1% of the size of the true peak"
  (`--peak-inaccuracy`); `--time-unit=B` makes short runs
  reproducible; `--peak-only` exists "for use in automated environments
  such as CI pipelines". Crucially for RSS-vs-heap: by default it
  "measures only heap memory … it does *not* directly measure memory
  allocated with lower-level system calls such as `mmap`" — the
  `--pages-as-heap=yes` mode treats "every page allocated via `mmap`
  and similar system calls" as a block. Forking programs: "the child
  will inherit all the profiling data", so `--massif-out-file` must
  contain `%p`. Monitor commands (`snapshot`, `detailed_snapshot`,
  `all_snapshots`, `xtmemory`) run through valgrind's gdbserver — a
  way to extract data *before* a kill lands.
- **bytehound** (koute.github.io/bytehound book): run is
  `LD_PRELOAD=./libbytehound.so ./your_application` with
  `MEMORY_PROFILER_LOG=info`, analyze with `./bytehound server
  memory-profiling_*.dat`; env knobs include
  `MEMORY_PROFILER_CULL_TEMPORARY_ALLOCATIONS=1` ("if you only care
  about memory leaks or you want to do long term profiling over several
  days"), `MEMORY_PROFILER_REGISTER_SIGUSR1/2` (signal handlers that
  toggle profiling), `MEMORY_PROFILER_ENABLE_SERVER` ("stream the
  profiling data through TCP using `bytehound gather`"),
  `MEMORY_PROFILER_WRITE_BINARIES_TO_OUTPUT=1` (default: embeds the
  profiled binaries in the dump so it decodes later without hunting
  originals), `MEMORY_PROFILER_GATHER_MMAP_CALLS=1` (mmap/munmap as a
  separate stream). It is "written in Rust nightly (only 1.62 or newer
  is supported)" and its jemalloc view "only works on AMD64 with the
  jemallocator crate". Huge dumps: `bytehound strip --threshold 60 -o
  stripped.dat original.dat` keeps only allocations that lived ≥60 s.
  Caveat: the GitHub API pins the last release at 0.11.0 / 2022-11-23
  — usable, but the maintenance clock stopped (see Unverified).

### 2. dhat inside the test binary — the OOM-proof instrument

dhat-rs (nnethercote/dhat-rs, docs.rs/dhat 0.3.3) is a global
allocator wrapper: `#[global_allocator] static ALLOC: dhat::Alloc =
dhat::Alloc;` plus `let _profiler = dhat::Profiler::new_heap();`; on
drop it prints a stderr summary and writes `dhat-heap.json` for
dh_view.html. The testing mode is what matters for the capped repro:
`dhat::Profiler::builder().testing().build()` with `dhat::HeapStats::get()`
exposing `total_blocks/total_bytes` (all allocations ever),
`max_blocks/max_bytes` (at peak) and `curr_blocks/curr_bytes`
(still-live), and the README's own example assertions — "The peak heap
usage of this code should be less than 10 MiB", "This code should free
all heap allocations before finishing". Constraints, verbatim from the
docs: profiling state is global, a second `Profiler` panics — "one
heap usage test per integration test file (own process, no
parallelism)" or `cargo test -- --test-threads=1`; the heap profile is
written only when the `Profiler` drops and "std::process::exit skips
destructors — explicitly drop the Profiler first"; it "deliberately
provides no way to reset the heap profiling state mid-run"; overhead
"is large" in debug builds ("Debug builds are too slow"), use release
with `debug = 1`. **The OOM-proof trick the crate's shape allows:**
because `HeapStats::get()` is callable anywhere, the churn test can
eprintln curr/max bytes every Nth resize iteration — that stderr is in
the journal after a SIGKILL, unlike any exit-time dump (heaptrack's
`.gz` and dhat's JSON are both written at exit; only mid-run logging
and bytehound's server streaming survive the kill).

### 3. In-process telemetry crates (maintenance as of late 2026)

| crate | latest (date) | reads | verdict for an always-on sampler |
|---|---|---|---|
| [sysinfo](https://crates.io/crates/sysinfo) | 0.40.4 (2026-11-16) | whole-system + per-process, diff-based refresh ("keep the same instance of `System`") | maintained at high tempo (five releases in Oct–Nov 2026), MSRV 1.95, multithreaded by default — built for system monitors, heavier than own-process `/proc` reads need |
| [memory-stats](https://crates.io/crates/memory-stats) | 1.2.0 (2024-06-26) | `physical_mem` (RSS) + `virtual_mem`; "always attempts to use the statistics from `/proc/self/smaps`", statm fallback, `always_use_statm` feature | its own docs price the accurate path as "fairly expensive"; dormant since 2024; wrong file per tick |
| [procfs](https://crates.io/crates/procfs) | 0.18.0 (2025-08-30) | typed `/proc` API: `Process::status()` (`VmRSS`, `VmSwap`…), **`Process::smaps_rollup()`** — "sum of all smaps data … much more performant to get it this way" (kernel ≥ 4.14, `CONFIG_PROC_PAGE_MONITOR`) | **the maintained crate that reads `smaps_rollup` directly**; yearly cadence; the answer to "any crates that read `/proc/self/smaps_rollup`" |
| [tikv-jemalloc-ctl](https://crates.io/crates/tikv-jemalloc-ctl) | 0.7.0 (2026-05-25) | `mallctl` typed API: `epoch::advance()` + `stats::allocated` / `stats::resident`, `stats_print` | only relevant if the app adopts jemallocator; the allocated-vs-resident gap is the fragmentation number |
| [mimalloc_rust](https://crates.io/crates/mimalloc_rust) | 0.1.52 (2026-05-22) | `MIMALLOC_SHOW_STATS=1` prints at exit; `mi_stats_print` deprecated for `mi_stats_print_out` | same "only if we adopt mimalloc" caveat |
| tracing / metrics | active 2026 | `gauge!("…")` fits "values that go up and down arbitrarily" (docs.rs/metrics) | emission channels, not samplers; neither reads `/proc` |

Kernel facts the sampler stands on (Documentation/filesystems/proc.rst,
torvalds/linux master): `/proc/pid/status` gives `VmRSS` ("VmRSS =
RssAnon + RssFile + RssShmem"), `RssAnon` ("size of resident anonymous
memory"), `RssFile`, `VmSize`, `VmSwap`; `smaps_rollup` carries "the
same fields as smaps" plus three extras "omitted in smaps since each
mapping identifies the type" — `Pss_Anon`, `Pss_File`, `Pss_Shmem` —
and all rollup info "can be derived from smaps, but at a significantly
higher cost" (i.e. reading full `smaps` is the expensive path
memory-stats takes). `clear_refs=5` resets the peak-RSS high-water
mark if the sampler ever wants per-window peaks.

### 4. Symbolization with split debuginfo / release frames

- **Rust side**: heaptrack's README: Rust "works out of the box" with
  debug symbols — set `debug = true` in `[profile.release]` — and needs
  its bundled rustc_demangle; dhat wants `debug = 1` for line info;
  massif: "compile with debugging info (the `-g` option)… it shouldn't
  matter much what optimisation level".
- **Split debuginfo**: valgrind follows `.gnu_debuglink`/`/usr/lib/debug`
  automatically and adds `--extra-debuginfo-path=/path` as "an extra,
  final place for Valgrind to search for debug objects", a
  `--debuginfo-server` (debuginfod) client, and
  `--allow-mismatched-debuginfo=yes` for build-id mismatches
  (valgrind(1), man7). heaptrack's answer is the `--raw` +
  `--interpret --debug-paths/--extra-paths/--sysroot` flow: record raw
  addresses under the cap, resolve symbols later against a tree that
  holds the `.so`/`.debug` files. bytehound sidesteps it: with
  `MEMORY_PROFILER_WRITE_BINARIES_TO_OUTPUT=1` (default) it "embed[s]
  the profiled application (and all of the libraries)" in the dump.
- **Unwinding**: heaptrack unwinds via libunwind (DWARF) — the gold
  linker caveat above; bytehound defaults to its own shadow-stack
  unwinder ("more intrusive, faster", `MEMORY_PROFILER_USE_SHADOW_STACK=1`);
  frame-pointer release builds unwind cheaper in both; DWARF works
  without `-fno-omit-frame-pointer` but slower.
- Split-debuginfo Rust specifics (`objcopy --only-keep-debug` +
  `.gnu_debuglink`, or cargo's `-Csplit-debuginfo`): both consumers
  resolve through the debug-link machinery valgrind documents; for
  heaptrack the `--debug-paths` argument is the switch. No
  Wayland/X11 interaction anywhere — all four tools are process-level.

### 5. Allocators: fragmentation plateau vs true leak

- **glibc**: free() returns memory only above the trim threshold —
  "The M_TRIM_THRESHOLD parameter specifies the minimum size (in
  bytes) that this block of memory must reach before sbrk(2) is used
  to trim the heap", default 128 KiB, and `M_TOP_PAD` likewise defaults
  128 KiB (mallopt(3)). The dynamic mmap threshold ratchets upward
  "when blocks larger than the current threshold … are freed", and
  "Dynamic adjustment of the mmap threshold is disabled if any of
  M_TRIM_THRESHOLD, M_TOP_PAD, M_MMAP_THRESHOLD, or M_MMAP_MAX is set"
  — i.e. explicitly pinning the trim threshold (as DropTerm already
  does) also freezes the ratchet. `malloc_trim()` "attempts to release
  free memory from the heap", and "since glibc 2.8 this function frees
  memory in all arenas and in all chunks with whole free pages"
  (malloc_trim(3)) — a diagnostic move, since it is "slow to run it
  automatically" (alacritty maintainer, issue #5438).
- **Peer precedent**: alacritty #5438 is the canonical GUI-allocator
  thread — RSS doubling after zoom/resize churn, "Zooming in and out
  leaking a little bit of memory is a known issue", `malloc_trim(0)
  does return everything back`, still open (milestone 0.18.0);
  wezterm #3815: wezterm "intentionally uses memory/caching to reduce
  CPU overhead, so high usage isn't necessarily a leak".
- **Reading the signature**: jemalloc's `stats.allocated` vs
  `stats.resident` split requested memory from held pages
  (tikv_jemalloc_ctl docs); massif's manual names the class — "space
  leaks that aren't detected by traditional leak-checkers … the memory
  isn't ever actually lost — a pointer remains to it — but it's not in
  use"; bytehound's leak-analysis chapter is the worked methodology:
  graph `only_leaked()`, then group by backtrace, then the bounded-leak
  case ("its memory usage is actually bounded… you might see this kind
  of allocation pattern" from an LRU cache), then
  `only_leaked_or_deallocated_after(runtime * 0.98)` for
  freed-only-at-shutdown. **For DropTerm**: growth that plateaus at the
  scrollback ring's pinned ~50 MiB/tab (resource-audit math) is
  allocator retention; growth past the caps that tracks grid content
  is either a real leak or an uncapped store — the churn repro's
  47.1 GB is neither plateau nor slow linear: it is transient
  re-allocation volume (see finding 6), which is why the allocation-
  count/temporaries views (heaptrack_print's "MOST TEMPORARY
  ALLOCATIONS", dhat's `total_bytes` vs `curr_bytes`) are the first
  readout, before any leak view.

### 6. Terminal-emulator memory hazards, with the documented incidents

- **Scrollback rings are universal**: alacritty's grid holds a
  `Storage` with a `max_scroll_limit`; growth is clamped —
  `increase_scroll_limit` adds `min(count, max_scroll_limit -
  history_size())` and resize ends with
  `truncate(max_scroll_limit + lines)` (alacritty_terminal
  grid/mod.rs, master). kitty: `scrollback_lines 2000` default,
  "Memory is allocated on demand", "using very large scrollback is not
  recommended as it can … use large amounts of RAM", pager history is
  a separate bounded buffer (`scrollback_pager_history_size`, MB, max
  4 GB, "approximately 10000 lines per megabyte") (kitty.conf).
  wezterm: `scrollback_lines` "serves as an upper bound on the number
  of lines… The larger this value, the more memory is required"
  (wezterm.org/scrollback.html). foot: ring + the 1.19.0
  `resize-keep-grid` option (CHANGELOG).
- **Resize/reflow is where engines blow up**: alacritty's
  `shrink_columns` wraps every overflowing cell into the next row —
  at 1 column "each original row expands to (row length − 1) rows"
  (grid/resize.rs, master): the mechanism class behind a
  1-col↔full-width churn storm (a 10 000-line × 200-col buffer
  becomes ~2 M rows per pass). Documented costs and incidents:
  - alacritty #5438 (open): resize/zoom churn doubling RSS; allocator
    retention; `malloc_trim` discussion above.
  - wez #1342: on resize "scrollback gets re-wrapped" and "several
    rewraps can occur within a fraction of a second" — the reason
    wezterm's unbounded-scrollback plan stalled since 2021.
  - foot 1.14.0 shipped reflow and immediately logged "Crash when
    interactively resizing the window with a very large scrollback";
    reflow regressions kept landing (1.21.0, 1.22.0) (CHANGELOG.md).
  - kitty deliberately does *not* reflow scrollback on resize — its
    resize defects are corruption reports (#7033, #7305), not
    memory blowups; its memory incident is #970 (scrollback "eats way
    too much memory", fixed by dynamic allocation per the changelog).
  - No engine publishes a "quadratic reflow" postmortem by that name;
    the closest primary evidence of the cost class is wez #1342 and
    the Contour `line-reflow-mode` VT extension, which exists precisely
    "since reflowing long lines is costly" (contour-terminal.org).
- **SIGWINCH / degenerate-size redraw storms**: alacritty #6794 —
  resize below the prompt width froze the app on X11 and the prompt
  "began repeating on screen"; the repetition itself is the shell
  redrawing its prompt per size change and "is working as intended (or
  expected I guess)" (chrisduerr); the freeze was an upstream glutin
  bug. The GB-scale postmortem of this class is Claude Code #39022
  (Mar 2026): per-SIGWINCH re-render with no debouncing took RSS
  "from under 1GB to roughly 21GB in about 6 seconds", kernel-killed
  with `anon-rss:21348556kB` — same shape as DropTerm's 47.1 GB /
  12 s churn, minus the terminal engine. bash/readline's own handling
  is the flag-on-SIGWINCH pattern (see Unverified for the two items I
  did not fetch directly).
- **Fast-output and PTY hazards**: foot 1.14.0 "Crash when application
  output scrolls very fast, e.g. `yes` (#1305)" is the documented
  scroll-storm incident; DropTerm's own PTY/child teardown chain is
  already censused (resource-audit).
- **Glyph/atlas retention**: VRAM "is mostly dictated by the number of
  unique glyphs", not scrollback (alacritty scrollback announcement);
  alacritty #5438's closing note: "200M Is not unusual at all for some
  graphics drivers". DropTerm allocates no GPU resources itself
  (resource-audit), so this hazard is inherited from the GPUI/wgpu
  stack, not ours to meter.

### 7. System-level capture: caps, guards, and journal forensics

- **Caps**: `systemd-run --user --scope -p MemoryMax=2G
  -p MemoryHigh=1536M -p MemorySwapMax=0 -- <cmd>` — a scope "will be
  executed by systemd-run itself as parent process and will thus
  inherit the execution environment of the caller… synchronous"
  (systemd-run(1)). Semantics: MemoryHigh is "the throttling limit…
  the processes are heavily slowed down… This is the main mechanism to
  control memory usage of a unit"; MemoryMax is the "absolute limit…
  If memory usage cannot be contained under the limit, out-of-memory
  killer is invoked inside the unit"; and the man page's own ordering
  rule: "It is recommended to use MemoryHigh= as the main control
  mechanism and use MemoryMax= as the last line of defense"
  (systemd.resource-control(7)). The kernel side
  (admin-guide/cgroup-v2): at `memory.max` "the OOM killer is invoked
  in the cgroup"; `memory.oom.group` kills "all tasks belonging to the
  cgroup … together or not at all"; `memory.events` exposes the
  `oom_kill` counter ("processes belonging to this cgroup killed by
  any kind of OOM killer") and `memory.peak` is readable and resettable.
- **Guards so a runaway can't freeze the machine**: earlyoom (v1.9.0,
  2025-09-16, active) "checks the amount of available memory and free
  swap up to 10 times a second… sending SIGTERM when mem avail <=
  10.00% and swap free <= 10.00%, SIGKILL when mem avail <= 5.00%",
  with `--prefer REGEX` to bias victims, self-protection ("Never
  terminate ourselves", "Skip processes with oom_score_adj=-1000",
  mlockall) — its premise is the kernel killer "acts too late" and the
  desktop dies in swap first. systemd-oomd is PSI-based and acts
  "before an OOM occurs in the kernel space", SIGKILLing "all
  processes" of a chosen cgroup; "It is highly recommended for the
  system to have swap enabled for systemd-oomd to function optimally…
  Without swap, the system enters a livelocked state much more
  quickly" (systemd-oomd.service(8)). Install: `sudo apt install
  earlyoom` / `sudo dnf install earlyoom` / `sudo pacman -S earlyoom`
  (README) — both guards are system services, i.e. root territory;
  everything else in this record runs as the user.
- **Observation tools**: psrecord "uses the psutil library to record
  the CPU and memory activity of a process… still under development
  and is therefore experimental" — `psrecord <pid|cmd> --log f.txt
  --plot p.png --interval 2 --include-children --duration N`
  (README; `pip install psrecord[plot]`). smem's reason to exist is
  RSS overcounting shared pages — it reports "proportional set size
  (PSS), which is a more meaningful representation", plus USS columns,
  from `/proc` or "data snapshots from directory mirrors or compressed
  tarballs" (`--source capture.tar.gz`) (selenic.com/smem).
- **OOM forensics from the journal** (`journalctl -k` after the kernel
  kills the repro; all format strings from mm/oom_kill.c, master): the
  invocation line `"%s invoked oom-killer: gfp_mask=%#x(%pGg),
  order=%d, oom_score_adj=%d"` (pr_warn, `dump_header()`); the kill
  line from `__oom_kill_process()`:
  `"%s: Killed process %d (%s) total-vm:%lukB, anon-rss:%lukB,
  file-rss:%lukB, shmem-rss:%lukB, UID:%u pgtables:%lukB
  oom_score_adj:%d"` where `%s` is `Out of memory` (system-wide) or
  `Memory cgroup out of memory` (our capped-scope case). Field
  readings: `total-vm` is VSZ, `anon-rss` is the anonymous resident
  figure the badness score weighed, `file-rss`/`shmem-rss` are
  file-backed/tmpfs residents, `pgtables` page-table bytes,
  `oom_score_adj` the victim's override (proc.rst: badness 0–1000,
  "-1000 means it will always report a badness score of 0"). The
  `dump_tasks()` table under it is
  `[  pid  ]   uid  tgid total_vm      rss rss_anon rss_file rss_shmem
  pgtables_bytes swapents oom_score_adj name` (values in pages). For
  the systemd scope itself, `memory.events`' `oom_kill` and
  `memory.peak` are the authoritative post-mortem counters.

## What DropTerm has today

- The churn repro is real and headless: `cargo lib-test` + real
  `/bin/bash` with the user's login rc + `Terminal::resize` churn
  between 1 column and full width reached **47.1 GB anon-rss in ~12 s**
  before the kernel OOM kill; future reproductions are specified to
  run under `systemd-run -p MemoryMax` caps (owner tasking,
  2026-11-17).
- glibc's ratchet is already pinned process-wide:
  `pin_malloc_trim_threshold()` sets `M_TRIM_THRESHOLD` to 128 KiB
  before any allocating thread starts, after the F12 toggle probe
  measured a sustained ~50 kB/toggle RSS ratchet that "ran flat" with
  the pin (dropterm/src/main.rs:81–104, measured 2026-11-16).
- Scrollback is capped in the engine fork: hard ring cap fixed at
  spawn, ~50.5 MiB/tab worst case at the 10 000-line default, resize
  headroom ≤1 000 rows, and the saturated-tab delta was live-confirmed
  at 71 396 kB against a 71 428 kB cell math (resource-audit record,
  2026-11-17).
- The GPUI frontend carries no RSS sampler today: a tree-wide search
  finds `/proc` reads only for `comm`, `cmdline`, and `cwd`
  (dropterm/src/core/terminal/engine.rs, suggest.rs). The "always-on
  RSS/smaps sampler ships in release builds" named in the tasking is
  **not locatable in this tree** — its design below is greenfield, not
  a refactor.
- The daily-use RAM growth and the release-build sampler requirement
  come from the owner tasking; the app's resource bounds otherwise
  stand as filed in resource-audit.

## Recommendation

Ranked for our three use sites:

**a. The capped headless cargo-test repro — heaptrack primary.**

```bash
systemd-run --user --scope -p MemoryMax=2G -p MemoryHigh=1536M -p MemorySwapMax=0 \
  -- heaptrack --follow-children \
  cargo test -p dropterm --lib -- resize_churn -- --test-threads=1 --nocapture
# → /tmp/heaptrack.dropterm-*.gz (and a per-child file for the spawned bash)
heaptrack_gui /tmp/heaptrack.dropterm-*.gz     # flame graph + cost over time
heaptrack_print /tmp/heaptrack.dropterm-*.gz   # top-10 text: peak / temporaries / calls
```

Install once: `apt install heaptrack heaptrack-gui` (or build per the
README). Build the test with `[profile.release] debug = true` (or run
the dev profile) so stacks symbolize. Give the churn loop a wall-clock
deadline or iteration bound so heaptrack *finalizes*: its `.gz` is
written at exit and **an OOM SIGKILL loses it** — MemoryHigh at 1536M
throttles before MemoryMax kills, which is exactly the
man-page-recommended ordering. When the point of the run *is* the
kill, switch instruments: dhat testing-mode in the test binary,
`HeapStats::get()` eprintln'd every Nth iteration (stderr → journal,
survives SIGKILL), or bytehound with `MEMORY_PROFILER_ENABLE_SERVER=1`
+ `bytehound gather` (data streams out-of-process, survives by
construction). massif is the fallback when heaptrack is unavailable
(`valgrind --tool=massif --time-unit=B --max-snapshots=200 …`, vgdb
`snapshot` before the kill) — accepted cost: the program "will execute
(slowly)", which changes the churn's timing.

**b. The always-on in-app sampler — hand-rolled `/proc` reads, no new
dependency.**

- Tick: 1 Hz (every 10th of the existing 100 ms UI tick), std::fs
  reads, no crates.io additions required.
- Fields per row: unix timestamp, `VmRSS`, `RssAnon`, `RssFile`,
  `VmSwap` from `/proc/self/status` (cheap single-file read; kernel
  semantics pinned in proc.rst).
- Every 30th tick, one `smaps_rollup` read adding `Pss`,
  `Pss_Anon`, `Pss_File`, `Swap` — the rollup is the kernel's cheap
  aggregate ("much more performant", procfs docs; needs kernel ≥ 4.14,
  universally true on 2026 desktops). Take the typed reader from
  `procfs = "0.18"` only if we want it typed; a hand parser is ~20
  lines and keeps the dependency tree flat.
- Sink: `XDG_STATE_HOME/dropterm/memory-log` — two-file A/B rotation
  with a hard row cap (2 048 rows ≈ tens of KB), oldest dropped;
  plain CSV-ish text so the owner can `tail`/`grep` it without tooling.
- Explicitly rejected for this duty: `memory-stats` (reads full
  `smaps` per call by its own docs — the expensive path — and is
  dormant since 2024), `sysinfo` (whole-system refresh machinery,
  MSRV 1.95, multithreaded by default — wrong shape for one file per
  second), and tracing/`metrics` emission (an observability stack is
  not the deliverable; the bounded log is).

**c. Daily-use incidents — the owner-run capture ladder** (owner-run
per the 2026-11-14 runtime ruling; each step is one concise
instruction):

1. Observe: `psrecord $(pidof dropterm) --interval 2 --include-children
   --log /tmp/dt-ram.log --plot /tmp/dt-ram.png` while the growth
   reproduces (one-time `pip install psrecord[plot]`).
2. Point-in-time: `grep -E '^(Rss|Pss|Pss_Anon|Pss_File|Swap):'
   /proc/$(pidof dropterm)/smaps_rollup` — anon-vs-file split in one
   cheap read.
3. Capture: relaunch under the profiler — `heaptrack --follow-children
   dropterm` from a terminal (full run); or attach to the live
   instance `heaptrack --pid $(pidof dropterm)` (README: injection
   "via GDB", so gdb must be installed; kernel.yama ptrace restrictions
   apply to attaching, none to a fresh launch). Nothing here touches
   X11/Wayland — all tools are process-level.
4. If it died by OOM: `journalctl -k --since today` and read the
   `Killed process … anon-rss … oom_score_adj` line per the field
   table in finding 7; the scope's own `memory.events`/`memory.peak`
   tell the cap-side story.
5. Fragmentation question specifically (RSS plateaus high, heaptrack
   shows freed): bytehound run with `MEMORY_PROFILER_CULL_TEMPORARY_
   ALLOCATIONS=1` for a long window, then the web GUI's
   fragmentation/RSS views.

**Rejected, spelled out**: valgrind as the primary repro instrument
(20×-ish slowdown distorts a timing-sensitive bash/redraw storm);
bytehound as primary (nightly-rustc build, last release 2022 — a
frozen tool at the center of the workflow, and its dumps get huge
without culling); `perf`/eBPF routes (no allocation-stack attribution
without heavy custom setup; nothing off-the-shelf beats heaptrack
here); swapping glibc for jemalloc/mimalloc app-wide to "fix" the
symptom (changes the allocator under diagnosis, and the repro is
glibc-specific — the trim-threshold pin is already the measured
mitigation); `memory-stats`/`sysinfo` for the sampler (finding 3's
verdicts); log-based leak heuristics without an allocation profiler
(RSS-only lines cannot separate leak from retention from churn — the
whole point of finding 5).

## Sources

Fetched 2026-11-17 unless noted. Pinned by repo/version; upstream
master files cited as of that date.

- github.com/KDE/heaptrack (README, master; v1.5.0 per
  apps.kde.org/heaptrack 2023-09-23, KDAB announcement 2023-10-04) —
  capture model, 10 ms RSS thread, invocations, `--pid` attach,
  `--raw`/`--interpret`, Rust symbolization notes, gold-linker caveat.
- valgrind.org/docs/manual/ms-manual.html (valgrind 3.27.1) — massif
  scope, snapshot/peak model, `--pages-as-heap`, time units, monitor
  commands, output format; manual footer for the 3.27.1 version pin.
- man7.org/linux/man-pages/man1/valgrind.1.html —
  `--extra-debuginfo-path`; valgrind.org manual index +
  superuser-cited `--allow-mismatched-debuginfo` (option documented in
  the core manual).
- github.com/koute/bytehound (README + book:
  getting_started/configuration/memory_leak_analysis/common_issues) —
  LD_PRELOAD flow, all `MEMORY_PROFILER_*` knobs, web GUI, leak
  methodology chapter, `strip`; GitHub API releases/latest for 0.11.0
  / 2022-11-23 (tag acce3bc).
- github.com/nnethercote/dhat-rs (README) + docs.rs/dhat 0.3.3 —
  `dhat::Alloc`/`Profiler`, testing mode, `HeapStats` fields,
  single-profiler constraint, exit/drop caveats.
- crates.io API (sysinfo 0.40.4, memory-stats 1.2.0, procfs 0.18.0,
  tikv-jemalloc-ctl 0.7.0, mimalloc_rust 0.1.52) + docs.rs pages for
  sysinfo, memory-stats, tikv-jemalloc-ctl, procfs
  (`Process::smaps_rollup`, `SmapsRollup`) — maintenance dates and
  APIs.
- torvalds/linux Documentation/filesystems/proc.rst — status fields,
  smaps_rollup fields and cost note, oom_score/oom_score_adj,
  clear_refs=5.
- docs.kernel.org/admin-guide/cgroup-v2.html — memory.max OOM
  behavior, memory.oom.group, memory.events, memory.peak.
- torvalds/linux mm/oom_kill.c — `__oom_kill_process()` kill-line
  format string, `dump_header()` invocation line, `dump_tasks()` table
  header.
- freedesktop.org systemd man pages (latest):
  systemd.resource-control.html (MemoryHigh/MemoryMax semantics +
  ordering recommendation), systemd-run.html (`--scope` syntax and
  semantics), systemd-oomd.service.html (PSI model, swap
  recommendation).
- github.com/rfjakob/earlyoom (README; v1.9.0 2025-09-16) —
  thresholds, flags, self-protection, install commands.
- github.com/astrofrog/psrecord (README) — flags, "experimental"
  label.
- selenic.com/smem — PSS/USS framing, sources, kernel floor.
- man7.org man pages mallopt.3 / malloc_trim.3 — tunables, dynamic
  mmap threshold and its disabling, trim semantics.
- alacritty: alacritty_terminal/src/grid/{mod,resize}.rs (master) —
  ring clamp, truncate-on-resize, shrink-to-1-column row expansion;
  issues #5438 (malloc_trim/retention thread), #6794 (sub-prompt
  resize freeze + prompt-repetition ruling), #4419 (reflow
  correctness).
- kitty: sw.kovidgoyal.net/kitty/conf — scrollback_lines,
  scrollback_pager_history_size; issues #970 (memory incident),
  #7033/#7305 (resize corruption, via search summaries).
- wezterm: wezterm.org/scrollback.html — bound + memory warning;
  issues #1342 (rewrap cost, wez quotes), #3815 (cache-by-design).
- foot: codeberg.org/dnkl/foot CHANGELOG.md (master raw) — 1.14.0
  reflow-era crash bullets, 1.19.0 `resize-keep-grid`, 1.21/1.22
  reflow regressions.
- github.com/anthropics/claude-code issues #39022 — the SIGWINCH
  21 GB/6 s OOM postmortem.
- docs.rs/metrics — gauge semantics. microsoft.github.io/mimalloc/
  environment.html — `MIMALLOC_SHOW_STATS` (via search).

## Unverified

- **bytehound maintenance**: the release trail stops at 0.11.0
  (2022-11-23, GitHub API); search found no 2026 commit activity, but
  I could not pin a last-commit date from the repo page — "frozen
  since 2022" is an inference from the release record, not a verified
  commit date.
- **The tasking's "always-on RSS/smaps sampler ships in release
  builds"**: not found in this tree (grep across `dropterm/src` shows
  `/proc` reads only for comm/cmdline/cwd). Either it lives on another
  branch or the tasking anticipates it; this record treats the design
  as greenfield. If a sampler ships elsewhere, the tick/field/rotation
  spec in the recommendation still applies.
- **psrecord's last commit date** was not visible on the fetched page
  (no releases listed, 28 open issues); "experimental" is the
  author's own label, aging-unclear is my inference.
- **heaptrack release cadence**: 1.5.0 (2023-09-23) is the newest
  tagged release per apps.kde.org; master's activity level (1,644
  commits) came from the repo page without a last-commit date.
- **bash/readline SIGWINCH specifics**: the flag-on-SIGWINCH redisplay
  pattern (CPython #67923) and fish 4.7.1's "infinite prompt redraw
  loop" fix came from search summaries of those trackers, not direct
  fetches — directionally consistent with #6794's maintainer ruling,
  but not pinned here.
- **memory-stats's full-`smaps` cost at 1 Hz** is unmeasured here;
  "expensive" is the crate's own wording, cross-checked against the
  kernel doc's cost framing for smaps vs rollup.
