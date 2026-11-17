# Chunk: ram-telemetry — the app watches its own RAM

Spec: [SPEC-ram-telemetry.md](../SPEC-ram-telemetry.md) · Module
id: `ram-telemetry` · hardening pass, main · spec approved
2026-11-17 (owner `/planning` go-ahead; builds after `ram-leak`).

**Files touched:** `dropterm/src/core/mem.rs` (new),
`dropterm/src/app_shell.rs` (the tick hook),
`docs/memory-diagnostics.md` (new), `README.md` (Documentation
index), `docs/testing.md`. Per-task ≤5-file lists below.

## Task rt-1: The sampler module (`core/mem.rs`)

**Description:** Std-only, zero new crates: parse
`/proc/self/status` (VmRSS, RssAnon, RssFile, VmSwap) and
`/proc/self/smaps_rollup` (Rss, Pss, Pss_Anon, Pss_File, Swap) from
fixture strings; aggregate children (count + RSS sum) from pids the
caller hands in — no `/proc` scanning, one status read per tracked
child. The row: self-describing plain text, header line + fixed
columns (timestamp, the fields, `children_n`, `children_rss_kB`),
so an agent extracts the log with grep/std and no parsing preamble
(the owner's 2026-11-17 AI-extractability ruling). The sink: log
file pair under the resolved state dir — two-file A/B rotation,
hard 2 048-row cap per file, oldest dropped, never the fresh sample.
Path resolution rides the config store's XDG-override pattern.
Failure-safe throughout: a missing field or unwritable dir skips
the sample — telemetry never panics the app. Unit tests inline per
house rules (sibling `tests.rs` past ~500 lines).

**Acceptance criteria:**
- [x] Fixtures parse to exact numbers; unknown/missing fields skip
- [x] Rotation drops the oldest row at the cap and never the fresh
      one; both files' bounds hold as pinned numbers
- [x] The XDG override is honored (throwaway dirs in tests)
- [x] Zero new dependencies (Cargo.toml untouched)

**Verification:**
- [x] `just check` green — the module's tests ride it

**Dependencies:** none (build order after `ram-leak`, no code
dependency) · **Files:** `dropterm/src/core/mem.rs` ·
**Scope:** M

## Task rt-2: The hook — 1 Hz on the existing tick + children feed

**Description:** Sample every 10th tick of the shell's 100 ms loop
(`app_shell.rs` — no new timer, no new thread); `smaps_rollup`
every 30th sample; children fed from the panes' tracked PTY child
pids. Agent-run capped smoke, headless: a test drives N samples
through the module's loop on a fake cadence and asserts the on-disk
log — exists, header + N well-formed rows, rotation honored. The
real-session row is owner-run per the 2026-11-14 ruling: one
concise instruction issued at this task's close (install the build,
use a normal day, `tail` the log — the verdict reported back).

**Acceptance criteria:**
- [x] The hook fires exactly once per 10 ticks (cadence-counted,
      unit-pinned); `smaps_rollup` once per 30 samples
- [x] Children columns populated from tracked pids (unit-pinned
      with fixture children)
- [x] The smoke run is green and its log artifacts recorded
      (agent-run, capped); the owner's daily-session instruction
      issued

**Verification:**
- [x] Smoke output in the chunk:
      `rt-2 smoke: ticks=20600 writes=2060 A=2048 rows/86817B B=12 rows/626B rollup_rows=68`
      (the module's loop on a fake cadence — real own-process
      `/proc` reads, real files, 0.11 s: slot A at exactly its
      pinned cap, the spill in B, every 30th row carrying rollup,
      both files tens of KB)
- [x] `just check` green

**Owner-run row (the daily session, 2026-11-14 ruling — issued
2026-11-18, verdict pending):** install the dev build
(`just dev-image && just dev-image-install`), use `dropterm-dev`
for a normal day (tabs, shells, a couple of heavy commands), then
read the curve — `tail -n 3
~/.local/state/io.github.l1br3_io.dropterm.Dev/memory.log*` — and
report: the files exist, `vm_rss_kb` lands in a plausible band,
`children_n` tracks the open shells, and (the module's whole point)
whether the numbers hold flat over the day or climb. The verdict
closes the module.

**Dependencies:** rt-1 · **Files:** `dropterm/src/app_shell.rs`,
`dropterm/src/core/mem.rs` (integration point) · **Scope:** S to M

## Task rt-3: Docs + banking

**Description:** `docs/memory-diagnostics.md`: the log's schema with
agent-extraction one-liners (tail/grep/awk examples an AI runs
as-is against a real log), plus the owner capture ladder per the
tooling record — `psrecord --include-children`, the `smaps_rollup`
point read, heaptrack relaunch or `--pid` attach, and journalctl
OOM forensics. Linked from README's Documentation index.
`docs/testing.md` row; known-issues residue (or none, said);
SPEC.md status.

**Acceptance criteria:**
- [ ] The docs page exists with the schema, the one-liners, and the
      ladder; linked from the README Documentation index
- [ ] testing.md row landed; residue stated; the owner's
      daily-session verdict row stands open for the report
- [ ] `just check` green (docs-check: every link resolves)

**Verification:**
- [ ] `just check` green
- [ ] The owner's daily-session verdict closes the module when
      reported

**Dependencies:** rt-2 · **Files:** `docs/memory-diagnostics.md`
(new), `README.md`, `docs/testing.md`, `SPEC.md` · **Scope:** S

## Checkpoint — the watch is live

- [ ] Sampler in every build, the log verified on disk, docs
      landed; the owner's daily-session verdict is the module's
      close.
