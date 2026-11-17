# SPEC: ram-telemetry — the app watches its own RAM so the next incident captures itself

Status: not started · hardening pass · main · spec drafted 2026-11-17,
approved 2026-11-17 (the owner's proceed-to-`/planning`; builds after
`ram-leak`)

## Objective

An always-on, release-included memory sampler so the owner's
confirmed daily-use RAM growth is visible as a curve instead of a
crash: the process samples its own memory on a slow cadence and
writes a bounded rotating log under XDG state. Post-`ram-leak`, a
flat curve is the regression signal; a climbing one is a captured
incident with timestamps. Ships with an owner-run capture recipe —
the external-tool steps for the day the curve climbs.

Owner ruling this spec rides (2026-11-17): always-on in release.

## Scope

In: a core sampler module (own-process reads only, per the tooling
record: `/proc/self/status` VmRSS/RssAnon/RssFile/VmSwap at 1 Hz,
plus `smaps_rollup` Pss/Pss_Anon/Pss_File/Swap every 30th sample —
std-only, no new dependencies); a children column (count + RSS sum
from the app's own tracked PTY child pids — one bounded status read
per tracked child, no /proc scanning) so child-driven growth shows
in the log itself; the hook on the existing 100 ms
tick's cadence (every 10th tick, not a new timer or thread); a
rotating log under `XDG_STATE_HOME/dropterm/` (two-file A/B rotation
at a hard row cap, oldest dropped; rows self-describing plain text —
header line + fixed columns — so an agent extracts them with std
tooling, no binary formats); the XDG-override pattern the config store
uses (testability, throwaway dirs); a docs page with the log's schema
plus the owner capture recipe (psrecord/heaptrack per the tooling
report); a testing.md row.

Out: anything that leaves the machine (no exporters, no metrics
endpoints); CPU/GPU telemetry; new crates (std-only by design —
`memory-stats` and `sysinfo` are rejected by the tooling record's
verdicts; `procfs = "0.18"` is the one sanctioned typed alternative
if a hand parser ever proves insufficient, ask-first); the leak
hunt itself (`ram-leak` owns it).

## Commands

    just check

## Files

`dropterm/src/core/mem.rs` (new — one concept, one name), the tick
hook site (`dropterm/src/app_shell.rs`), `docs/memory-diagnostics.md`
(new) linked from README's Documentation index, `docs/testing.md`.
Per-task ≤5-file lists at chunk time.

## Constraints

- Zero new dependencies by default: a hand parser (~20 lines per the
  record) keeps the tree flat; `procfs` is the sanctioned typed
  alternative, ask-first.
- Overhead bounded by construction: one cheap file read per sample
  period, one `smaps_rollup` every 30th; no in-memory history (write
  and forget — the log file is the only history).
- The log's bound is the record's: two-file A/B rotation at a hard
  2 048-row cap (tens of KB), oldest dropped, never the fresh sample.
- Sampler failure is silent-safe: missing `/proc` fields or an
  unwritable state dir skip the sample — telemetry never panics the
  app.
- No secrets in the log: timestamps, counters, and sizes only.

## Testing strategy

- Headless unit pins: procfs-fixture parsing, rotation eviction under
  the cap, the XDG override honored, failure-safe paths (missing
  fields, unwritable dir).
- One agent-run smoke (the sanctioned pattern, capped): a headless
  run produces a non-empty log with plausible numbers; the
  daily-session row rides the recipe doc, owner-reported per the
  2026-11-14 ruling.
- `just check` green on every landing.

## Boundaries

- Always: the rotation bound holds as a pinned number; docs land
  with the feature.
- Ask first: any exporter/remote path; any new dependency; a default
  sample period below 1 s.
- Never: unbounded growth anywhere (the sampler policing memory may
  not itself leak); user content in the log.

## Success criteria

- A release-build session produces the log with the documented
  schema; the rotation bound is pinned by a test as a number.
- The docs page exists with the capture recipe, linked from README's
  Documentation index; the testing.md row is landed.
- `just check` green.

## Open questions

Resolved by the tooling record (2026-11-17): the field set (1 Hz
status fields + 30th-tick `smaps_rollup`), the two-file A/B
rotation with its row cap, and the no-new-crate verdict are all
stated above from its findings. Nothing else open.

## Size

S to M (one module, one hook, one docs page).
