# Chunk: toggle-ram — repeated toggling must not eat the machine

Spec: [SPEC-toggle-ram.md](../SPEC-toggle-ram.md) · Module id:
`toggle-ram` · hardening pass, main · spec approved 2026-11-16
(owner `/planning` go-ahead).

**Files touched:** `scripts/vm-leak-probe.sh` (the attribution
extension, both regimes), `dropterm/src/main.rs` (allocator setup),
the window lifecycle the toggles drive
(`dropterm/src/app_shell.rs`, `dropterm/src/executor.rs`), core's
machine (`dropterm/src/core/surface_lifecycle.rs`) if the evidence
points there, `docs/testing.md`, `docs/known-issues.md`. The fix
task's exact files are named by the evidence, capped per landing.

## Task tr-1: The attribution probe — both regimes, per mapping

**Description:** Extend `scripts/vm-leak-probe.sh` past total RSS
into attribution, and past the paced regime into the never-probed
spam territory. Per sample: per-mapping RSS from
`/proc/<pid>/smaps` (`[heap]` vs anonymous vs `/dev/dri` vs memfd
summed separately), fd count (`/proc/<pid>/fd`), thread count
(`Status`), plus the binary's build identity (mtime or sha) so every
run answers "which build" by construction. Regimes: the existing
paced 1 s settle, a 100 ms profile, and a 50 ms burst profile — the
last two drive toggles under `TOGGLE_DEBOUNCE` (150 ms,
`core/surface_lifecycle.rs:21`) and inside the ~130 ms slide, the
overlap territory no probe has touched. While in there, measure the
debug trace's own footprint: `DROPTERM_TOGGLE_DEBUG`'s dump appends
to `/tmp/dropterm-toggle.log` unboundedly (`app_shell.rs:1276`) —
record its growth class. The owner's crash signature is captured
alongside: one documented owner-run pass for journalctl OOM/kill rows,
`coredumpctl`, and stderr over the regime that crashes.

**Acceptance criteria:**
- [ ] The probe samples per-mapping RSS + fds + threads every 10
      toggles, paced and both spam profiles, ≥150 toggles each, on
      current main (74e6d85's pin in)
- [ ] The growth verdict is recorded: which mapping grows per regime,
      or main is clean under both (then the finding is the owner's
      build being older, and the row narrows to that)
- [ ] The debug-trace growth class measured and noted (bounded
      file-open per dump vs runaway append)
- [ ] The crash signature captured owner-run (OOM row, coredump, or
      clean) and recorded in this chunk

**Verification:**
- [ ] Probe outputs (all three regimes) recorded here with the build
      identity — owner-run by default per the 2026-11-14 ruling;
      agent-run only if the owner grants the standing exception at
      this task's review (the 74e6d85 one-off is the precedent)
- [ ] `just check` green (probe changes are shell + docs only)

**Dependencies:** none · **Files:** `scripts/vm-leak-probe.sh`,
this chunk, a testing.md row if the harness changes shape ·
**Scope:** S to M

**State (2026-11-16, agent):** the probe is built and smoke-verified
headless — the smaps bucket walk reconciles exactly with the kernel's
own `smaps_rollup` (busy cosmic-comp: rollup = walk, both 335,752 kB,
the stable gap to status `VmRSS` being shmem-class accounting, noted
in the script); the full loop (build-identity header from the running
exe, samples every 10, halfway anchor, settled sample, tail-summary
deltas, pid-vanish/respawn notices, regime guard) exercised against
a fake pid on the host. The boxes above stay open: the three ≥150-
toggle runs, the growth verdict, the debug-trace class, and the
crash signature are the owner-run evidence (2026-11-14 ruling; the
standing-exception question below is asked at this review).

## Task tr-2: The fix (gate: tr-1's attribution)

**Description:** Fix what the evidence names, at the root. Suspects to
have ready, none assumed: the executor's `live` window map retention
across recreate storms (`executor.rs:367`); per-window wgpu/GPU
buffers surviving the two-window show's teardown; pending queues and
slide-clock state accumulating in the shell's 100 ms tick under
sub-debounce presses; allocator behavior beyond the pinned trim
threshold (per-thread caches under rapid window churn); the measuring
surface's lifetime in the show path. If the cause lands upstream
(GPUI proper, cosmic-comp), file it with the evidence and take the
smallest carried remedy — ask first, the kit-root-border precedent is
the shape. The commit carries before/after numbers for the named
mapping; no fix lands without an attributed vector.

**Acceptance criteria:**
- [ ] The root cause is named (mapping + code path) in the fixing
      commit, with before/after per-regime numbers
- [ ] The probe tail is flat over ≥150 toggles in each regime on the
      fixed build (the one-time warm-up allowance stands, bounded as
      74e6d85 measured it)
- [ ] The triggering crash no longer reproduces at ≥2× the
      triggering duration/count
- [ ] If the cause is upstream: filed with evidence, remedy
      ask-first, no silent fork

**Verification:**
- [ ] Probe rerun on the fixed build, both regimes (same
      authorization regime as tr-1)
- [ ] The crash-regime walkthrough on owner hardware
- [ ] `just check` green

**Dependencies:** tr-1 · **Files:** named by the evidence (candidates
above) · **Scope:** M

## Task tr-3: Bank it — long-run evidence and docs rows

**Description:** The fix's evidence becomes the record: testing.md's
RAM row extends 74e6d85's with the both-regime tail (its current row
is paced-only, unchecked); known-issues gains a row for any residue
(cosmic-comp's own creep is already out of scope there; anything new
the fix leaves behind gets its own row); this chunk records the probe
outputs verbatim. If the standing exception was granted, agent-run
evidence is marked agent-run per the ruling's recording rule.

**Acceptance criteria:**
- [ ] testing.md's row extended with both-regime evidence; the
      owner's walk instruction is one concise block per the manual
      ruling
- [ ] known-issues row for any user-visible residue (or none, said)
- [ ] Probe outputs recorded in this chunk with build identities

**Verification:**
- [ ] `just check` green (docs-check rides it)
- [ ] Owner review of the evidence before `resource-audit` opens

**Dependencies:** tr-2 · **Files:** `docs/testing.md`,
`docs/known-issues.md`, this chunk · **Scope:** S

## Checkpoint — the crash is fixed and evidenced

- [ ] Flat tail both regimes, crash gone at 2× duration, rows green,
      owner has seen the numbers. `resource-audit` opens on this.
