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
- [x] The probe samples per-mapping RSS + fds + threads every 10
      toggles, paced and both spam profiles, ≥150 toggles each, on
      current main (74e6d85's pin in)
- [x] The growth verdict is recorded: which mapping grows per regime,
      or main is clean under both (then the finding is the owner's
      build being older, and the row narrows to that)
- [ ] The debug-trace growth class measured and noted (bounded
      file-open per dump vs runaway append)
- [ ] The crash signature captured owner-run (OOM row, coredump, or
      clean) and recorded in this chunk

**Verification:**
- [x] Probe outputs (all three regimes) recorded here with the build
      identity — owner-run by default per the 2026-11-14 ruling;
      agent-run only if the owner grants the standing exception at
      this task's review (the 74e6d85 one-off is the precedent)
- [x] `just check` green (probe changes are shell + docs only)

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

**Evidence (owner-run 2026-11-17, PopOs24):** three regimes × 150
toggles, fresh hidden instance each, one build — `sha256
1b68d22c1669c27d4d6f25af8f54609cc27dc1c604d6032f77cca58d06f4c9dd`
(current main @ 678caee, 74e6d85's pin in, installed 00:27).
Verbatim paste also under `target/vm-evidence-tr1/`.

```
# vm-leak-probe regime=paced settle=1.0s toggles=150
# build exe=/usr/bin/dropterm sha256=1b68d22c1669c27d4d6f25af8f54609cc27dc1c604d6032f77cca58d06f4c9dd size=70699744 mtime=2026-11-17 00:27:49.000000000 +0800
t0-baseline rss=244320kB hwm=244320kB heap=54332kB anon=41328kB dri=0kB memfd=3272kB file=145388kB fds=57 threads=47 dbglog=0B comp_rss=288600kB procs=1
after-10 rss=257844kB hwm=263404kB heap=67764kB anon=41400kB dri=0kB memfd=3292kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=288600kB procs=1
after-20 rss=258740kB hwm=264196kB heap=68624kB anon=41428kB dri=0kB memfd=3300kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=288600kB procs=1
after-30 rss=258756kB hwm=268200kB heap=68720kB anon=41364kB dri=0kB memfd=3284kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=288600kB procs=1
after-40 rss=259388kB hwm=270492kB heap=69212kB anon=41496kB dri=0kB memfd=3292kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=288600kB procs=1
after-50 rss=260004kB hwm=270652kB heap=69800kB anon=41524kB dri=0kB memfd=3292kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=288600kB procs=1
after-60 rss=263056kB hwm=273716kB heap=72804kB anon=41564kB dri=0kB memfd=3288kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=288600kB procs=1
after-70 rss=265204kB hwm=276436kB heap=74920kB anon=41596kB dri=0kB memfd=3300kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=288600kB procs=1
after-80 rss=273320kB hwm=279004kB heap=83012kB anon=41628kB dri=0kB memfd=3292kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=289176kB procs=1
after-90 rss=275408kB hwm=281176kB heap=85196kB anon=41536kB dri=0kB memfd=3288kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=289176kB procs=1
after-100 rss=275684kB hwm=281596kB heap=85464kB anon=41540kB dri=0kB memfd=3292kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=289176kB procs=1
after-110 rss=275608kB hwm=281596kB heap=85388kB anon=41540kB dri=0kB memfd=3292kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=289176kB procs=1
after-120 rss=275672kB hwm=281596kB heap=85444kB anon=41540kB dri=0kB memfd=3300kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=289176kB procs=1
after-130 rss=275884kB hwm=281676kB heap=85664kB anon=41540kB dri=0kB memfd=3292kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=289176kB procs=1
after-140 rss=275720kB hwm=281676kB heap=85500kB anon=41540kB dri=0kB memfd=3292kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=289176kB procs=1
after-150 rss=275808kB hwm=281676kB heap=85588kB anon=41540kB dri=0kB memfd=3292kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=290008kB procs=1
after-150-settled rss=275808kB hwm=281676kB heap=85588kB anon=41540kB dri=0kB memfd=3292kB file=145388kB fds=57 threads=46 dbglog=0B comp_rss=290008kB procs=1
# tail summary (settled minus after-70 — the sustained rate; positive heap/anon/dri/memfd names the growing mapping)
# rss=+10604kB heap=+10668kB anon=+-56kB dri=+0kB memfd=+-8kB file=+0kB fds=+0 threads=+0 dbglog=+0B comp_rss=+1408kB

# vm-leak-probe regime=spam100 settle=0.1s toggles=150
# build exe=/usr/bin/dropterm sha256=1b68d22c1669c27d4d6f25af8f54609cc27dc1c604d6032f77cca58d06f4c9dd size=70699744 mtime=2026-11-17 00:27:49.000000000 +0800
t0-baseline rss=244576kB hwm=244576kB heap=54852kB anon=41512kB dri=0kB memfd=3272kB file=144940kB fds=57 threads=47 dbglog=0B comp_rss=290008kB procs=1
after-10 rss=253176kB hwm=259524kB heap=65684kB anon=41368kB dri=0kB memfd=1660kB file=144940kB fds=57 threads=47 dbglog=0B comp_rss=288408kB procs=1
after-20 rss=256860kB hwm=261512kB heap=67908kB anon=41548kB dri=0kB memfd=2464kB file=144940kB fds=57 threads=47 dbglog=0B comp_rss=289208kB procs=1
after-30 rss=257052kB hwm=264076kB heap=68876kB anon=41572kB dri=0kB memfd=1664kB file=144940kB fds=57 threads=47 dbglog=0B comp_rss=288408kB procs=1
after-40 rss=256976kB hwm=264740kB heap=69004kB anon=41408kB dri=0kB memfd=1624kB file=144940kB fds=57 threads=47 dbglog=0B comp_rss=288408kB procs=1
after-50 rss=261312kB hwm=264740kB heap=69312kB anon=41424kB dri=0kB memfd=5636kB file=144940kB fds=57 threads=47 dbglog=0B comp_rss=302152kB procs=1
after-60 rss=257816kB hwm=267768kB heap=69356kB anon=41616kB dri=0kB memfd=2056kB file=144940kB fds=57 threads=47 dbglog=0B comp_rss=298152kB procs=1
after-70 rss=258888kB hwm=269048kB heap=69364kB anon=41692kB dri=0kB memfd=2892kB file=144940kB fds=57 threads=47 dbglog=0B comp_rss=292848kB procs=1
after-80 rss=257668kB hwm=269676kB heap=74352kB anon=41668kB dri=0kB memfd=1648kB file=144940kB fds=61 threads=46 dbglog=0B comp_rss=288408kB procs=1
after-90 rss=264196kB hwm=270468kB heap=75132kB anon=41660kB dri=0kB memfd=2464kB file=144940kB fds=57 threads=46 dbglog=0B comp_rss=288408kB procs=1
after-100 rss=264008kB hwm=271220kB heap=75864kB anon=41540kB dri=0kB memfd=1664kB file=144940kB fds=57 threads=46 dbglog=0B comp_rss=288408kB procs=1
after-110 rss=264924kB hwm=271220kB heap=76616kB anon=41696kB dri=0kB memfd=1672kB file=144940kB fds=57 threads=46 dbglog=0B comp_rss=288408kB procs=1
after-120 rss=265288kB hwm=271220kB heap=77168kB anon=41528kB dri=0kB memfd=1652kB file=144940kB fds=57 threads=46 dbglog=0B comp_rss=288408kB procs=1
after-130 rss=269420kB hwm=272756kB heap=77180kB anon=41664kB dri=0kB memfd=5636kB file=144940kB fds=57 threads=46 dbglog=0B comp_rss=294148kB procs=1
after-140 rss=269844kB hwm=275196kB heap=73712kB anon=41556kB dri=0kB memfd=9636kB file=144940kB fds=57 threads=46 dbglog=0B comp_rss=306152kB procs=1
after-150 rss=261988kB hwm=276644kB heap=75488kB anon=41692kB dri=0kB memfd=1648kB file=144940kB fds=57 threads=46 dbglog=0B comp_rss=298152kB procs=1
after-150-settled rss=265408kB hwm=276644kB heap=75488kB anon=41712kB dri=0kB memfd=3268kB file=144940kB fds=57 threads=46 dbglog=0B comp_rss=290008kB procs=1
# tail summary (settled minus after-70 — the sustained rate; positive heap/anon/dri/memfd names the growing mapping)
# rss=+6520kB heap=+6124kB anon=+20kB dri=+0kB memfd=+376kB file=+0kB fds=+0 threads=+-1 dbglog=+0B comp_rss=+-2840kB

# vm-leak-probe regime=spam50 settle=0.05s toggles=150
# build exe=/usr/bin/dropterm sha256=1b68d22c1669c27d4d6f25af8f54609cc27dc1c604d6032f77cca58d06f4c9dd size=70699744 mtime=2026-11-17 00:27:49.000000000 +0800
t0-baseline rss=243436kB hwm=243436kB heap=54788kB anon=40208kB dri=0kB memfd=3272kB file=145168kB fds=57 threads=46 dbglog=0B comp_rss=290208kB procs=1
after-10 rss=247996kB hwm=251256kB heap=62964kB anon=40276kB dri=0kB memfd=2048kB file=145168kB fds=61 threads=46 dbglog=0B comp_rss=294348kB procs=1
after-20 rss=252040kB hwm=258104kB heap=64104kB anon=40300kB dri=0kB memfd=2468kB file=145168kB fds=57 threads=46 dbglog=0B comp_rss=289408kB procs=1
after-30 rss=253612kB hwm=259632kB heap=66484kB anon=40304kB dri=0kB memfd=1656kB file=145168kB fds=57 threads=46 dbglog=0B comp_rss=288608kB procs=1
after-40 rss=258880kB hwm=259632kB heap=67784kB anon=40292kB dri=0kB memfd=5636kB file=145168kB fds=57 threads=46 dbglog=0B comp_rss=288608kB procs=1
after-50 rss=263392kB hwm=263392kB heap=68288kB anon=40300kB dri=0kB memfd=9636kB file=145168kB fds=57 threads=46 dbglog=0B comp_rss=296608kB procs=1
after-60 rss=256596kB hwm=262984kB heap=68192kB anon=40344kB dri=0kB memfd=2892kB file=145168kB fds=57 threads=46 dbglog=0B comp_rss=289408kB procs=1
after-70 rss=255588kB hwm=263228kB heap=69800kB anon=40356kB dri=0kB memfd=1648kB file=145168kB fds=61 threads=46 dbglog=0B comp_rss=288608kB procs=1
after-80 rss=258424kB hwm=263672kB heap=70420kB anon=40360kB dri=0kB memfd=2868kB file=145168kB fds=57 threads=46 dbglog=0B comp_rss=289808kB procs=1
after-90 rss=255852kB hwm=263672kB heap=68652kB anon=40380kB dri=0kB memfd=1652kB file=145168kB fds=57 threads=46 dbglog=0B comp_rss=288608kB procs=1
after-100 rss=255852kB hwm=263672kB heap=68676kB anon=40348kB dri=0kB memfd=5636kB file=145168kB fds=61 threads=46 dbglog=0B comp_rss=288608kB procs=1
after-110 rss=262756kB hwm=263672kB heap=71600kB anon=40352kB dri=0kB memfd=5636kB file=145168kB fds=57 threads=46 dbglog=0B comp_rss=292608kB procs=1
after-120 rss=256676kB hwm=267028kB heap=69020kB anon=40392kB dri=0kB memfd=2468kB file=145168kB fds=57 threads=46 dbglog=0B comp_rss=288608kB procs=1
after-130 rss=257584kB hwm=267028kB heap=68732kB anon=40396kB dri=0kB memfd=3288kB file=145168kB fds=57 threads=46 dbglog=0B comp_rss=290208kB procs=1
after-140 rss=261172kB hwm=267888kB heap=73480kB anon=40464kB dri=0kB memfd=2060kB file=145168kB fds=57 threads=45 dbglog=0B comp_rss=289008kB procs=1
after-150 rss=262500kB hwm=267888kB heap=74036kB anon=40428kB dri=0kB memfd=1664kB file=145168kB fds=53 threads=45 dbglog=0B comp_rss=288608kB procs=1
after-150-settled rss=261296kB hwm=267888kB heap=74036kB anon=40428kB dri=0kB memfd=1664kB file=145168kB fds=53 threads=45 dbglog=0B comp_rss=288608kB procs=1
# tail summary (settled minus after-70 — the sustained rate; positive heap/anon/dri/memfd names the growing mapping)
# rss=+5708kB heap=+4236kB anon=+72kB dri=+0kB memfd=+16kB file=+0kB fds=+-8 threads=+-1 dbglog=+0B comp_rss=+0kB
```

**Verdict (from the series' shapes, not the after-70 anchors — those
sit mid-climb):** current main is **clean in all three regimes at
150 toggles**. No crash, no pid change, `procs=1` throughout — the
crash signature on this build in the VM is *none*. `[heap]` climbs
from ~54 MB to a bounded steady state (~85 MB paced by toggle ~90,
~75 MB spam) — bedding-in converging, not a ratchet: paced
after-100→settled +392 kB/50 toggles (~8 kB/toggle), spam100
−396 kB/50, spam50 noise-dominated (68–74 MB oscillation), flat.
anon/dri/file flat; memfd oscillates 1.6–9.6 MB transiently and
returns down; fds 53–61 and threads 45–47 bounded. This is the
spec's narrowing branch: main clean ⇒ the finding is the owner's
crashing build being older (74e6d85's pin explains a pre-pin
ratchet), pending the owner's answer on which build their hardware
crashed on.

**Still open for tr-1's boxes:** the debug-trace growth class (the
`DROPTERM_TOGGLE_DEBUG` pass — dbglog=0 in all three runs because
the env wasn't set) and the crashing build's identity (owner report)
— the 2×-duration confirmation (300 toggles) rides whichever regime
the owner's crash resembles.

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
