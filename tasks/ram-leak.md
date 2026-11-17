# Chunk: ram-leak — the growth the kernel kill evidenced, named and fixed

Spec: [SPEC-ram-leak.md](../SPEC-ram-leak.md) · Module id:
`ram-leak` · hardening pass, main · spec approved 2026-11-17
(owner `/planning` go-ahead).

**Files touched:** `dropterm/src/core/terminal/engine/tests.rs`
(rl-1's test, replacing the uncommitted probe — the probe is never
committed in its panic shape), wherever rl-2's attribution points
(candidates: the engine wrapper `engine.rs`, the pinned fork's
reflow via the git pin, the pane's resize funnel), a dev-dependency
line only if dhat is needed,
`docs/research/ram-leak.md` (new), `docs/research/README.md`,
`docs/testing.md`, `docs/known-issues.md`. The fix task's exact
files are named by the attribution, capped per landing.

## Task rl-1: The capped reproduction, formalized

**Description:** Turn the uncommitted probe ("bug 1",
`probe_real_rc_toggle_churn`) into a real `#[ignore]`-gated test —
the real login rc is environment-dependent, so `just check` never
runs it. Shape: spawn `/bin/bash -i` through the engine exactly as
the app does, wait for the first prompt, then the app's exact show
churn (the degenerate 1-col first paint ↔ full width), cycles
bounded by a deadline; per-cycle `VmRSS` sampled from
`/proc/self/status` into the test's output. The run recipe is the
record's, every time:

    systemd-run --user --scope -p MemoryMax=2G -p MemoryHigh=1536M \
      -p MemorySwapMax=0 -- cargo test -p dropterm --lib <name> -- \
      --ignored --test-threads=1 --nocapture

Pre-fix expectation under the cap: the curve the kernel saw
uncapped (47.1 GB) climbs until MemoryHigh throttles or MemoryMax
kills — the run records the climb to that point. The probe file's
panic-shape is replaced wholesale in this task's commit.

**Acceptance criteria:**
- [x] The test exists, `#[ignore]`-gated; `just check` runs nothing
      of it and stays green
- [x] The capped run's curve is recorded in this chunk (agent-run,
      per the owner's 2026-11-17 sanction, cap values in the header)
- [x] The old probe text is gone from the tree — no panic-shape
      test is ever committed
- [x] No uncapped run of the growth path is made, ever

**Verification:**
- [x] Capped run output in the chunk with the cap values and build
      identity
- [x] `just check` green

**rl-1 capped run record (agent-run, 2026-11-17, scope
`run-r2ea836d9d18a459e979b1b7b1e5fd8fc`):** the record's recipe
verbatim — MemoryMax=2G, MemoryHigh=1536M, MemorySwapMax=0,
`--ignored --test-threads=1 --nocapture` — on
`target/debug/deps/dropterm-59e80177bea39695` (sha256 986e6f97…).
First prompt found; `curve start: VmRSS 50052 KiB, 1 prompt markers`;
cycle 0 never printed — the climb (49 MB → ~1.67 GiB) lands inside
the first cycle's resize pair, and the run then wedges in a
**swap-less MemoryHigh livelock**: `memory.current` 1,789,595,648 B
between High (17,100 `high` throttle events) and Max (0 oom, 0
kills), RSS 1.6 GiB / VSZ 1.71 GiB; threads — main futex-waiting on
the term mutex, one in D-state `mem_cgroup_handle_over_high` (the
allocator), one running. Live anon at the watermark with swap off
gives reclaim nothing, so the run neither finishes nor reaches the
kill; it was ended by `systemctl --user stop` (SIGTERM, cargo exit
143). Read: **one churn cycle's resize allocates at GB/s** — the
incident's 47.1 GB / ~12 s uncapped, the same climb throttled at
1.5 GiB inside cycle 0 here. Consequence for rl-2: an exit-time
heaptrack dump cannot finalize against a livelock that never exits —
dhat's mid-run `HeapStats` polling (the sanctioned fallback) is the
expected instrument; heaptrack only if a run shape allows an exit.

**Dependencies:** none · **Files:** engine/tests.rs · **Scope:** S

## Task rl-2: Attribution — name the allocating site

**Description:** heaptrack under the same cap and ordering —
MemoryHigh throttles before MemoryMax kills and the churn is
deadline-bounded, so the exit-time dump finalizes. If the run still
dies dumpless: dhat as a dev-dependency in the test binary,
`HeapStats::get()` eprintln'd per cycle (stderr survives SIGKILL).
First suspect per the tooling record's terminal-hazards finding:
the reflow's 1-col row expansion (alacritty's own `resize.rs`
mechanism — at 1 column a full-width row expands to ~cols−1 rows)
multiplied by the real rc's readline reprints. Deliver the name:
the allocating stack, the arithmetic that reaches GB/s, and where
the fix lands — ours (engine wrapper, pane funnel) or the pinned
fork's reflow.

**Acceptance criteria:**
- [x] The stack and mechanism are recorded in this chunk with the
      numbers, and the GB/s arithmetic shown
- [x] The fix-site verdict is stated: ours, or the fork's (with the
      carried-patch implication flagged for rl-3)
- [x] The instrument choice is recorded (heaptrack, or dhat fallback
      with why)

**Verification:**
- [x] heaptrack output (or the dhat stderr series) summarized in the
      chunk, agent-run, capped
- [x] `just check` green (a dhat dev-dep, if added, is dev-only and
      noted)

**rl-2 attribution record (agent-run, 2026-11-17, every run capped
under rl-1's recipe):**

- **Instrument:** heaptrack is not installed on this rig, and an
  exit-time dump could not fire against rl-1's swap-less MemoryHigh
  livelock anyway (the run never exits); dhat's aggregates were not
  needed once the stack could be sampled directly. The instrument that
  worked: the capped repro plus a `PR_SET_PTRACER_ANY` aid in the
  test, `eu-stack` sampled mid-wedge (yama scope 1 otherwise blocks
  attach; no root on this rig), PCs resolved with addr2line. The
  spec's "mid-run survives" principle, realized as mid-run stack
  sampling. The repro's per-cycle prints also moved to stderr —
  stdout to a pipe is block-buffered, so the wedged run's cycle lines
  died unflushed (why rl-1 could not say which cycle wedged).
- **The stack (12/12 samples identical):**
  `Terminal::resize → Term::resize → Grid::resize →
  Grid::shrink_columns → Vec<Cell>::insert → insert_mut → grow_one →
  grow_amortized → finish_grow → realloc`, stalled in
  `mem_cgroup_handle_over_high`. Line:
  `alacritty_terminal/src/grid/resize.rs:297` —
  `wrapped.insert(0, wide_char)` (the fork @ 1185ffd).
- **The mechanism — the immortal wide-led row:** at `columns == 1`,
  `shrink_columns` peels one cell per loop iteration; but when the
  surviving cell (`row[0]`) is `WIDE_CHAR`-flagged, lines 293-297
  replace it with a spacer, push the 1-cell row, and **re-prepend the
  wide cell to `wrapped` — which becomes the next row at the same
  length**. The row never shrinks, the loop never exits, and every
  iteration allocates a fresh k-cell `Vec` plus one pushed `Row`.
  Unbounded, at GB/s.
- **The wide-glyph source (captured):** the owner's real prompt
  carries wide emoji. Feeding a live `/bin/bash -i` a cols-1↔cols-264
  churn under `script` emitted U+1F918 🤘, U+1F387 🎇, U+1F4BB 💻 in
  readline's redraw — all East-Asian Wide (the `┌` U+250C is narrow).
  At 1 column the redraw wraps one glyph per row, so an emoji with
  any cell behind it is an immortal-row lead. (The `~/.bashrc` `PS1`
  lines are ASCII; the emoji prompt is set elsewhere in the
  interactive rc chain — the capture is the evidence, the rc line is
  not pinned.)
- **The arithmetic:** one iteration ≈ rebuild a k-cell `Vec`
  (k ≈ 300 cells × ~16 B ≈ 4.8 KB) + one 1-cell `Row` push + an O(k)
  insert shift. Observed ~1 GB/s throttled (49 MB → 1.67 GB inside
  the first churn pair); the uncapped incident 47.1 GB / ~12 s ≈
  4 GB/s ≈ 800 K iterations/s — a µs-scale tight loop. Consistent.
- **Fix-site verdict: the pinned fork's `shrink_columns`** — and the
  code is upstream-identical (the fork's only delta is the OSC 133;D
  arm; alacritty/alacritty **master carries the same unguarded loop
  today**, checked 2026-11-17). rl-3 takes the kit-root-border
  shape: a carried patch at the pin plus an upstream ask
  (alacritty/alacritty), re-pin **ask-first** before any pin change.
- **Honest residue:** this explains the incident (emoji prompt + the
  real toggle churn is the exact trigger) and the capped repro. It
  does not by itself explain the owner's gradual daily-use growth —
  whether each real show toggle catches an emoji-led row is
  timing-dependent; `ram-telemetry`'s curve is the watch.

**Dependencies:** rl-1 · **Files:** this chunk, `dropterm/Cargo.toml`
(dev-dep only if dhat) · **Scope:** S to M

## Task rl-3: The fix + pins *(gate: rl-2's attribution)*

**Description:** Fix the root cause where rl-2 names it. If ours:
the computation is fixed, and the pin lands as a headless test
constructible without the real rc once the mechanism is known —
synthetic feed + degenerate resize asserting the bound — running in
`just check`. If the fork's: the kit-root-border shape — the patch
carried at the pin plus an upstream ask (alacritty/alacritty PR),
re-pin ask-first, never silent. The capped real-rc reproduction
from rl-1 reruns post-fix: flat, the bound a stated number.

**Acceptance criteria:**
- [x] The root cause is named in the fixing commit with before/after
      numbers
- [x] The synthetic pin is green inside `just check`; the real-rc
      test stays `#[ignore]`-gated
- [x] The capped real-rc rerun is flat, its bound stated as a number
- [x] Fork path, if taken: upstream ask filed, the owner's re-pin
      ruling recorded

**Verification:**
- [x] Capped repro rerun output in the chunk (agent-run)
- [x] `just check` green
- [x] Ask-first answered before any re-pin

**rl-3 fix record (agent-run, 2026-11-17, every growth run capped;
the owner's re-pin ruling — carried path — landed the same day):**

- **The patch (two guards, both upstream-bug class):** (1)
  `grid/resize.rs` `shrink_columns` — skip the wide-char carry at
  `columns < 2` (rl-2's immortal row); (2) `term/mod.rs` `input` —
  at `columns < 2` a wide glyph's spacer has no cell to occupy: skip
  the fit-dance and the spacer write, leave the cursor on the glyph
  with `input_needs_wrap`. Bug (2) is a second latent upstream
  OOB the fix exposed (the pty write path indexed `column+1` on a
  1-wide row; unreachable pre-fix only because the resize wedged
  first — PTY-reader panic, backtrace captured capped).
- **A third finding, ours:** `engine.rs` `visible_rows` indexed
  `grid[Line(history_size..)]` — this fork's positive-line domain is
  `[0, screen_lines)` (history is negative lines), so any scrolled
  content tripped a debug assert (release: wrapped index). Exposed by
  the real-rc rerun (storage.rs:221 backtrace, captured); fixed to
  read `0..screen_lines`. The app's only caller is the debug-only
  toggle-dump trace.
- **The carry:** `carried/alacritty-terminal` — the exact fork @
  1185ffd sources plus the two guards (diff-audited: only
  `resize.rs` and `term/mod.rs` differ), manifest carrying the
  kit-root-border provenance comment; the working tree's
  `[patch.crates-io]` points at it (the git-rev entry preserved in a
  comment for verbatim restore).
- **The pin:** `a_one_col_churn_over_wide_glyphs_stays_bounded` —
  `--norc` bash, 🤘 + trailing content through the pty, three
  1-col↔full cycles, asserting the grid stays ≤ 1024 total rows
  (history + screen; the churn's content is a few dozen). Hermetic
  where process RSS is not — in-suite parallel tests rode the same
  VmRSS (166 MB observed noise), so the row bound is the in-`just
  check` pin and the RSS numbers live here: **+488 KiB** isolated
  capped for the pin's churn.
- **The capped real-rc rerun (the headline):** flat — 50,116 →
  51,184 KiB over 22 cycles (**+1.1 MiB total drift**, plateauing;
  ≈49 KiB/cycle bedding-in). Before: 49 MB → the 1.5 GiB throttle
  inside the first resize, unbounded beyond; uncapped, 47.1 GB /
  ~12 s. No panics; `just check` green on the fixed tree.
- **The ruling (owner, 2026-11-17): carried path copy** — the
  kit-root-border shape exactly, as staged: `[patch.crates-io]`
  points at `carried/alacritty-terminal`, the pre-carry git-rev pin
  preserved in a comment as the retirement's re-pin target. The
  upstream ask is the owner's outward action; the PR draft (both
  guards — the immortal shrink loop and the wide-spacer OOB at one
  column, both still in alacritty master) rides the incident record
  (`docs/research/ram-leak.md`, rl-4).

**Dependencies:** rl-2 · **Files:** named by the attribution
(candidates: `engine.rs`, the fork's grid/reflow through the git
pin, the pane funnel) · **Scope:** M

## Task rl-4: Bank it — the incident record

**Description:** `docs/research/ram-leak.md`, the immutable record:
the kernel OOM line verbatim, the binary identity
(`dropterm-59e80177bea39695`), the sibling-session provenance, the
attribution, the before/after. Index row in
`docs/research/README.md`; a `docs/testing.md` row; known-issues
residue — or none, said — including what the fix does *not* explain
of the owner's daily-use growth, stated as the honest open thread
`ram-telemetry` watches.

**Acceptance criteria:**
- [x] The record is filed with its index row (never edited after)
- [x] testing.md row landed; residue stated (or none, said)
- [x] SPEC.md's `ram-leak` status updated; the daily-growth
      expectation stated honestly

**Verification:**
- [x] `just check` green (docs-check rides it)
- [ ] Owner review at the checkpoint

**Dependencies:** rl-3 · **Files:** `docs/research/ram-leak.md`
(new), `docs/research/README.md`, `docs/testing.md`,
`docs/known-issues.md`, `SPEC.md` · **Scope:** S

## Checkpoint — the bomb is fixed and evidenced

- [x] Capped repro flat with a stated bound, pin green in
      `just check`, owner has the numbers. `ram-telemetry` opens on
      this checkpoint.
