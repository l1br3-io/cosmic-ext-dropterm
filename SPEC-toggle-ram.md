# SPEC: toggle-ram — repeated toggling must not eat the machine

Status: not started · hardening pass · main · spec drafted 2026-11-16,
approval pending

## Objective

The owner reports that repeated toggling eats RAM until the app
crashes. The prior incident is 74e6d85 (2026-11-16): every show/hide
cycle tears down and rebuilds two GPUI windows, freed heap piles onto
glibc's top chunk, and resident RAM ratcheted ~53 kB/toggle until a
pinned `M_TRIM_THRESHOLD` flattened the tail under paced toggles in
the validation VM. The approach there is the approach here, extended
past `[heap]`: measure first, attribute the growth to a mapping and a
code path, fix the cause, bank the numbers.

Three questions get answered with evidence before anything is fixed:

1. Which build crashes — with or without 74e6d85's pin.
2. What the crash's signature is — OOM kill, panic/abort, hang, or
   session-level trouble.
3. Which resource grows under both toggle regimes: paced (the probe's
   1 s settle, the only regime ever measured) and rapid spam
   (overlapping slides and recreate churn inside `TOGGLE_DEBOUNCE`'s
   window, never probed).

## Scope

In: establishing the crashing build and retesting current main; a
probe extension attributing growth per mapping (`[heap]` vs anon vs
`/dev/dri` vs memfd) plus fd and thread counts, in both regimes;
crash-signature capture (journalctl OOM rows, coredumpctl, stderr);
the fix wherever it lands (frontend shell/executor, core, allocator
setup); regression evidence (a flat probe tail over a long run in the
triggering regime); docs rows (`docs/testing.md`, known-issues where
user-visible).

Out: cosmic-comp's own per-toggle creep (upstream, out of scope by
74e6d85's record); the sweep across other resource vectors (that is
`resource-audit`, this module's successor); upstream GPUI/Kit fixes
beyond a carried patch.

## Commands

    just check
    just build-release
    # probe (owner-run by default; agent-run only under an explicit
    # owner instruction, the 74e6d85 precedent):
    just vm-exec-user 'sh /tmp/vm-leak-probe.sh 150'

## Files

`scripts/vm-leak-probe.sh` (per-mapping extension, both regimes),
`dropterm/src/main.rs` (allocator setup, startup), the window
lifecycle (`dropterm/src/app_shell.rs`, `dropterm/src/executor.rs`),
core's slide/toggle machine if the cause is core-side
(`dropterm/src/core/…`), `docs/testing.md`, `docs/known-issues.md`.
Per-task ≤5-file lists come at chunk time.

## Constraints

- Measure before fixing: no fix lands without an attributed vector
  (mapping + code path) and before/after numbers in its commit.
- Both regimes are measured, paced and rapid spam; the spam regime has
  never been probed.
- The mallopt pin stays; changing it is its own commit with
  re-measured numbers.
- Debug instrumentation must not be its own leak: anything like
  `DROPTERM_TOGGLE_DEBUG`'s /tmp log is checked for unbounded growth.
- COSMIC is primary (the crash report is COSMIC hardware); sway is the
  cross-check where the vector could be compositor-coupled.

## Testing strategy

- Reproduction and attribution run through the probe in the validation
  VM (PopOs24), owner-run per the 2026-11-14 manual ruling unless the
  owner grants a standing exception (open question below).
- The crash signature is captured for the triggering regime
  (journalctl, coredumpctl, stderr) and recorded in the chunk.
- Whatever pure logic the root cause turns out to be (a latch, a
  retention bug, a rebuild storm) is pinned headless; the leak itself
  is proven by the probe tail, which is evidence, not a unit test.
- `just check` green on the result.

## Boundaries

- Always: before/after numbers in the fixing commit; docs rows land in
  the same change; the probe script stays in the tree afterward.
- Ask first: new dependencies (profilers count); touching the engine's
  grid semantics; changing the mallopt pin.
- Never: paper over with periodic-restart or realloc hacks, silence
  the loud-miss panics, or mark a runtime row green that an owner did
  not report.

## Success criteria

- The crashing build is identified and current main retested; the
  regime and signature of any surviving crash are recorded.
- The growth vector is attributed (mapping + path) and fixed; the
  probe tail is flat over ≥150 toggles in each measured regime
  (numbers in the commit).
- The triggering crash no longer reproduces at ≥2× the triggering
  duration on the build with the fix.
- testing.md carries the row; known-issues carries any user-visible
  residue; `just check` green.

## Open questions

1. Which build crashed, with or without 74e6d85 — task one answers
   this; if current main is clean under both regimes, the finding is
   filed and this module narrows to the surviving signature's cause.
2. The crash's shape (OOM, panic, hang, session-level) — captured by
   the early tasks; each shape points at a different suspect list.
3. VM probe authorization: owner-run by the 2026-11-14 ruling; whether
   74e6d85's one-off becomes a standing exception for this module is
   the owner's word, requested at chunk review.

## Size

M.
