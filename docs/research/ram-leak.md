# What ate 47 GB in twelve seconds? — the ram-leak incident

Date: 2026-11-17 · Module: `ram-leak` (the RAM initiative, opened on
the incident) · Record: immutable — corrections land as new rows,
this text never changes. Attribution and fix evidence are agent-run
under the owner's 2026-11-17 sanction, every growth run capped.

## TL;DR

The kernel OOM kill of 2026-11-17 20:54 was **an immortal row in the
pinned alacritty fork's `shrink_columns` reflow**: at one column, a
row led by a wide (East-Asian) glyph is rebuilt at its own length on
every loop iteration — one row allocation per iteration, forever —
and the owner's emoji prompt supplies the wide cells on the exact
churn every real toggle drives (the degenerate 1-col first paint ↔
full width). The same unguarded loop is in alacritty master today.
Fixed by two one-column guards carried at the fork pin (owner
ruling: carried path, kit-root-border shape) plus one dropterm-side
victim fix; the capped reproduction that climbed 49 MB → the cap
inside one resize now runs flat at +1.1 MiB over 22 cycles.

## The incident

Verbatim, `journalctl -k` (this machine, 2026-11-17):

```text
Nov 17 20:54:16 pop-os kernel: [  67426]  1000 67426 14867501 11782360 11781754      606         0 118751232  3013920           200 dropterm-59e801
Nov 17 20:54:16 pop-os kernel: oom-kill:constraint=CONSTRAINT_NONE,...,global_oom,task_memcg=/user.slice/user-1000.slice/user@1000.service/app.slice/app-zcode-9017.scope,task=dropterm-59e801,pid=67426,uid=1000
Nov 17 20:54:16 pop-os kernel: Out of memory: Killed process 67426 (dropterm-59e801) total-vm:59470004kB, anon-rss:47127016kB, file-rss:2424kB, shmem-rss:0kB, UID:1000 pgtables:115968kB oom_score_adj:200
```

- **47.1 GB anon-rss, 59.4 GB total-vm, oom_score_adj 200, inside the
  zcode scope** — an agent-run lib test, not the installed app: the
  probe binary `target/debug/deps/dropterm-59e80177bea39695`, a
  ~12 s run of the real-rc resize-churn probe (`probe_real_rc_toggle_churn`)
  written by a sibling session that same evening and left uncommitted
  in `engine/tests.rs` — the owner ruled that probe this module's
  seed (2026-11-17). Three churn cycles sufficed: GB/s-scale
  allocation, a computed-size bug class.
- What the resource-audit could not see: its resize rows ran
  PlainHost/default shells — no real rc, no readline redraw during
  reflow.

## The reproduction (rl-1, capped)

The probe formalized as the `#[ignore]`-gated
`real_rc_toggle_churn_rss_curve` (the login rc is machine-dependent;
the growth path only ever runs under a cap — the recipe is pinned in
the test's doc comment):

```text
systemd-run --user --scope -p MemoryMax=2G -p MemoryHigh=1536M \
  -p MemorySwapMax=0 -- cargo test -p dropterm --lib \
  real_rc_toggle_churn_rss_curve -- --ignored --test-threads=1 --nocapture
```

Pre-fix, capped: the first prompt found (curve start 49 MB), then
the climb lands inside the first cycle's resize pair and the run
**wedged in a swap-less MemoryHigh livelock** — `memory.current`
1,789,595,648 B between High (17,100 throttle events) and Max (0
oom kills); live anon at the watermark with `MemorySwapMax=0` gives
reclaim nothing, so the run neither finishes nor reaches the kill.
Ended by scope stop. Lesson banked: stdout to a pipe is
block-buffered, so the per-cycle prints died unflushed (the repro's
curve now prints to stderr), and an exit-time dump can never
finalize against a run that never exits.

## The attribution (rl-2)

- **Instrument:** heaptrack is not installed on this rig and its
  exit-time dump could not fire against the livelock anyway; dhat's
  aggregates were unneeded once the stack could be sampled. A
  `PR_SET_PTRACER_ANY` aid in the test opened the wedged process to
  `eu-stack` (yama scope 1, no root), and **12/12 mid-wedge samples
  landed on one stack**, PCs resolved with addr2line:
  `Terminal::resize → Term::resize → Grid::shrink_columns →
  Vec<Cell>::insert` at `grid/resize.rs:297`
  (`wrapped.insert(0, wide_char)`), the allocation stalled in
  `mem_cgroup_handle_over_high`.
- **The mechanism — the immortal wide-led row:** at `columns == 1`,
  the shrink loop peels one cell per iteration, but when the
  surviving cell `row[0]` is `WIDE_CHAR`-flagged, lines 293-297
  replace it with a spacer, push the 1-cell row, and re-prepend the
  wide cell to `wrapped` — which becomes the next row **at the same
  length**. The row never shrinks; one fresh k-cell `Vec` + one
  pushed `Row` per iteration, unbounded.
- **The wide cells' source (captured):** feeding a live `/bin/bash -i`
  a cols-1↔cols-264 churn under `script` shows readline's redraw
  emitting U+1F918 🤘, U+1F387 🎇, U+1F4BB 💻 — East-Asian wide
  glyphs from the owner's real prompt (set in the interactive rc
  chain; the `~/.bashrc` `PS1` lines are ASCII). At one column the
  redraw wraps one glyph per row, so any emoji with a cell behind it
  is an immortal-row lead.
- **The arithmetic:** one iteration ≈ rebuild a ~300-cell `Vec`
  (~4.8 KB) + one `Row` push + an O(k) insert shift. Observed ~1 GB/s
  throttled; the incident 47.1 GB / ~12 s ≈ 4 GB/s ≈ 800 K
  iterations/s — a µs-scale tight loop. Consistent.
- **Upstream:** the fork @ 1185ffd is upstream-identical here (its
  only delta is the OSC 133;D arm); alacritty/alacritty **master
  carried the same unguarded loop** when checked 2026-11-17.

## The fix (rl-3)

Three findings, all fixed; the first two are upstream bugs carried
at the pin (owner ruling 2026-11-17: carried path,
[the kit-root-border precedent](kit-root-border.md) — the exact fork
sources at `carried/alacritty-terminal`, diff-audited to the two
files, retirement = the upstream fix):

1. **`grid/resize.rs` `shrink_columns`** — skip the wide-char carry
   at `columns < 2`: a wide glyph needs two columns, so carrying it
   into `wrapped` rebuilds the same row forever. At one column the
   wide cell stays in place as its own row.
2. **`term/mod.rs` `input`** (exposed by fix 1 — a second latent
   upstream OOB): at `columns < 2` the wide-glyph write path indexed
   `column+1` for the spacer on a 1-wide row (PTY-reader panic,
   backtrace captured capped). Now the fit-dance and the spacer
   write are skipped at one column; the cursor stays on the glyph
   with `input_needs_wrap`.
3. **dropterm's `visible_rows`** (ours; tripped by the fixed rerun):
   it indexed `grid[Line(history_size..)]`, but this fork's
   positive-line domain is the viewport only — any scrolled content
   tripped a debug assert in `compute_index` (a wrapped index in
   release builds). Now reads `0..screen_lines`. The app's only
   caller is the debug-only toggle-dump trace.

**Numbers (all agent-run, capped):** the synthetic pin
(`a_one_col_churn_over_wide_glyphs_stays_bounded`, in `just check`)
holds the grid ≤ 1024 rows over three 1-col↔full cycles, +488 KiB
RSS isolated; the real-rc reproduction runs **flat: 50,116 → 51,184
KiB over 22 cycles (+1.1 MiB total drift, plateauing)** — pre-fix
the same run climbed 49 MB → the 1.5 GiB throttle inside the first
resize and was unbounded beyond.

## What this does not explain

The owner's gradual daily-use RAM growth. Every real show toggle
drives the same 1-col↔full churn against the same emoji prompt, so
the immortal loop plausibly fired with timing-dependent frequency
(some toggles catch an emoji-led row, others grow out first) — but
this record cannot prove that was *all* of it. The always-on sampler
([ram-telemetry](../../SPEC-ram-telemetry.md)) owns that question:
if daily curves now sit flat, this fix was the whole story; if a
second vector remains, its curve names it.

## The upstream ask (the owner's outward action)

Draft PR to alacritty/alacritty, both guards, both still in master:

> **Title:** Fix infinite loop and out-of-bounds with wide chars at
> one column
>
> `Grid::shrink_columns` re-prepends a wide char into `wrapped` when
> it would be split at the shrink boundary — but at `columns == 1`
> the rebuilt next row has the same length as the current one, so
> the reflow loop never terminates, allocating one row per iteration
> (observed 47 GB anon-rss in ~12 s under an emoji prompt). Guard
> the carry with `columns > 1`. Separately, `Term::input`'s
> wide-glyph path writes its spacer at `point.column + 1`
> unconditionally, which indexes past the last column on a
> 1-column grid once the first fix lets writes resume — skip the
> fit-dance and spacer write at `columns < 2` and set
> `input_needs_wrap`. A wide glyph simply cannot be represented at
> one column; leaving it in place is finite and degrades only that
> glyph's rendering.

## Sources

- The kernel's own account: `journalctl -k`, 2026-11-17 20:54:16
  (quoted verbatim above).
- The fork: l1br3-io/alacritty @ `1185ffd98e1c…` (based on the
  0.25.1 publish + the OSC 133;D arm); upstream master's
  `alacritty_terminal/src/grid/resize.rs` checked 2026-11-17 —
  identical loop.
- The carried fix: `carried/alacritty-terminal` (this repo, ruled
  2026-11-17); the pins and repro in
  [dropterm's engine tests](../../dropterm/src/core/terminal/engine/tests.rs);
  the chunk ([tasks/ram-leak.md](../../tasks/ram-leak.md)) carries
  every run's numbers and cap values.
