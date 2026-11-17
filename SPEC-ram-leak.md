# SPEC: ram-leak — the growth the kernel kill evidenced, named and fixed

Status: not started · hardening pass · main · spec drafted 2026-11-17,
approved 2026-11-17 (the owner's proceed-to-`/planning`)

## Objective

Find and fix the allocation bomb behind the 2026-11-17 incident: the
kernel OOM-killed this repo's own lib-test binary at 20:54:16 holding
47.1 GB anon-rss (59.4 GB total-vm) — a ~12 s run of the real-rc
resize-churn probe (interactive bash with the owner's login rc, then
the app's exact show churn: the degenerate 1-col first paint ↔ full
width). That churn is the path every real toggle takes, and the owner
confirms the daily-use app also eats RAM over a session. This module
reproduces the growth headless under a hard memory cap, attributes it
to the allocating site, fixes the root cause, and pins the bound.

Owner rulings this spec rides (2026-11-17): agent-run headless
reproductions are sanctioned, always wrapped in a memory cap so a
reproduction can never OOM the machine; the sibling session's probe
("bug 1", uncommitted in engine/tests.rs) is this module's seed.

## Evidence baseline (the incident)

- `journalctl -k` 2026-11-17 20:54:16: `Killed process 67426
  (dropterm-59e801) total-vm:59470004kB, anon-rss:47127016kB`,
  oom_score_adj 200, inside the zcode scope — an agent-run test, not
  the installed app.
- `target/debug/deps/dropterm-59e80177bea39695` — the lib test
  binary carrying the probe; built tonight (sibling artifacts
  20:34–21:03).
- The probe: `probe_real_rc_toggle_churn` in engine/tests.rs — real
  login rc + 1-col↔full resize cycles; three cycles sufficed for
  47 GB, i.e. GB/s-scale allocation: a computed-size or
  multiplicative-growth bug class, not slow accumulation.
- What the resource-audit could not see: its resize-churn rows ran
  against PlainHost/default shells — no real rc, no shell redraw
  during reflow (ra-3's rapid-resize row; the audit's live
  confirmations never churned resize).
- First suspect (the tooling record's terminal-hazards finding):
  alacritty's own `resize.rs` documents the mechanism class — at 1
  column a full-width row expands to ~cols−1 rows; the attribution
  pass starts there.

## Scope

In: a formalized capped reproduction (the probe's shape, as a real
test, never its panic-shape); heaptrack attribution of the capped
run; the root-cause fix wherever it lands — dropterm's engine wrapper
(engine.rs resize, grid copies, spawn-settle), the pinned alacritty
fork's reflow (git fork `l1br3-io/alacritty` @ `1185ffd`), or the
pane's resize funnel; a headless pin of the growth bound; the
incident record under `docs/research/` with its index row;
known-issues/docs residue.

Out: memory telemetry (`ram-telemetry` owns it); non-memory bugs;
allocator swaps (glibc → jemalloc/mimalloc) — ask-first, never this
module's fix; performance work beyond the leak.

## Commands

    just check
    just build-release
    # the reproduction's shape (the record's recipe; the chunk pins final flags):
    systemd-run --user --scope -p MemoryMax=2G -p MemoryHigh=1536M -p MemorySwapMax=0 \
      cargo test -p dropterm --lib <repro> -- --test-threads=1 --nocapture
    # attribution under the same ordering — MemoryHigh throttles before
    # MemoryMax kills, so heaptrack's exit-time dump finalizes:
    systemd-run --user --scope -p MemoryMax=2G -p MemoryHigh=1536M -p MemorySwapMax=0 \
      -- heaptrack --follow-children \
      cargo test -p dropterm --lib <repro> -- --test-threads=1 --nocapture
    # OOM-proof fallback (dhat as a dev-dependency inside the test binary):
    #   HeapStats::get() eprintln'd every Nth iteration — stderr survives SIGKILL

## Files

`dropterm/src/core/terminal/engine/tests.rs` (the probe formalized),
wherever the root cause lands (engine.rs / the fork's reflow / the
pane funnel), `docs/research/ram-leak.md` (new — the incident +
attribution record), `docs/research/README.md` (index row),
`docs/testing.md`, `docs/known-issues.md` (residue). Per-task
≤5-file lists at chunk time.

## Constraints

- Every reproduction runs under a hard cap (systemd-run MemoryMax or
  an equivalent prlimit), the cap value recorded in the chunk; an
  uncapped run of the growth path never happens.
- The churn loop is deadline- or iteration-bounded so heaptrack
  finalizes; when the point of the run is the kill, the instrument
  switches to dhat's mid-run `HeapStats` (the record's rule:
  exit-time dumps die with SIGKILL; stderr polling survives).
- Root cause over edge cap: a cap added at the boundary is
  legitimate only when the growth is by design (the audit's
  constraint, restated); a computed-size bug gets the computation
  fixed.
- A fix in the pinned alacritty fork follows the kit-root-border
  precedent: the patch carried at the pin plus an upstream ask
  (alacritty/alacritty PR), ask-first before re-pinning.
- Evidence is recorded as agent-run (the sanctioned pattern; the
  2026-11-14 ruling's owner-run class covers GUI walks, of which
  this module needs none).

## Testing strategy

- The reproduction itself becomes a headless test asserting a stated
  growth bound over N churn cycles — a number, not "small".
- The root-cause fix lands with its own pin (the bound's shape
  depends on the site: allocation size, grid growth, or cycle cost).
- `just check` green on every landing.

## Boundaries

- Always: capped reproductions; evidence recorded; the incident
  record is a dated research record, never edited after filing.
- Ask first: re-pinning the alacritty fork; any allocator change;
  any fix that changes user-visible behavior.
- Never: commit the probe's panic-shape; run a growth reproduction
  uncapped; mark a row green on evidence not actually captured.

## Success criteria

- The capped reproduction runs the churn to a stated flat bound
  (numbered) where it grew to 47 GB before the fix.
- The root cause is named with the allocating stack (heaptrack
  evidence in the record), fixed, and pinned.
- The daily-use connection is stated honestly: what the fix explains
  of the owner's observed growth, and what `ram-telemetry`'s curve
  should now show — or what it may still expose (a second vector,
  filed).
- `just check` green; records and index rows landed.

## Open questions

Resolved by the tooling record (2026-11-17): heaptrack primary with
the MemoryHigh-before-Max ordering; dhat mid-run polling when the
kill is the point; massif the fallback when heaptrack is unavailable
(timing distortion noted); bytehound reserved for fragmentation
questions. Nothing else blocking.

## Size

M (one reproduction, one attribution pass, one fix with pins; grows
to L only if the root cause splits across fork and wrapper).
