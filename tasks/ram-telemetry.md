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
- [ ] The hook fires exactly once per 10 ticks (cadence-counted,
      unit-pinned); `smaps_rollup` once per 30 samples
- [ ] Children columns populated from tracked pids (unit-pinned
      with fixture children)
- [ ] The smoke run is green and its log artifacts recorded
      (agent-run, capped); the owner's daily-session instruction
      issued

**Verification:**
- [ ] Smoke output in the chunk
- [ ] `just check` green

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
