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
- [ ] The stack and mechanism are recorded in this chunk with the
      numbers, and the GB/s arithmetic shown
- [ ] The fix-site verdict is stated: ours, or the fork's (with the
      carried-patch implication flagged for rl-3)
- [ ] The instrument choice is recorded (heaptrack, or dhat fallback
      with why)

**Verification:**
- [ ] heaptrack output (or the dhat stderr series) summarized in the
      chunk, agent-run, capped
- [ ] `just check` green (a dhat dev-dep, if added, is dev-only and
      noted)

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
- [ ] The root cause is named in the fixing commit with before/after
      numbers
- [ ] The synthetic pin is green inside `just check`; the real-rc
      test stays `#[ignore]`-gated
- [ ] The capped real-rc rerun is flat, its bound stated as a number
- [ ] Fork path, if taken: upstream ask filed, the owner's re-pin
      ruling recorded

**Verification:**
- [ ] Capped repro rerun output in the chunk (agent-run)
- [ ] `just check` green
- [ ] Ask-first answered before any re-pin

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
- [ ] The record is filed with its index row (never edited after)
- [ ] testing.md row landed; residue stated (or none, said)
- [ ] SPEC.md's `ram-leak` status updated; the daily-growth
      expectation stated honestly

**Verification:**
- [ ] `just check` green (docs-check rides it)
- [ ] Owner review at the checkpoint

**Dependencies:** rl-3 · **Files:** `docs/research/ram-leak.md`
(new), `docs/research/README.md`, `docs/testing.md`,
`docs/known-issues.md`, `SPEC.md` · **Scope:** S

## Checkpoint — the bomb is fixed and evidenced

- [ ] Capped repro flat with a stated bound, pin green in
      `just check`, owner has the numbers. `ram-telemetry` opens on
      this checkpoint.
