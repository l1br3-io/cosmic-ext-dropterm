# SPEC: ram-watchdog — a soft in-app line the whole process tree is watched against

Status: not started · hardening pass · main · spec drafted 2026-11-18,
awaiting the owner's approval; builds after `ram-telemetry`

## Objective

An in-app soft limit on the app's whole-process RAM: a configurable
threshold checked against a 1 Hz tree reading (own RSS plus every
descendant), crossing it visibly. Post-`ram-leak` the growth risk was
named and fixed, and `ram-telemetry` made it visible as a curve — the
watchdog is the response half: the day the curve climbs anyway, the
app says so before the kernel does. The hard wall stays outside the
app (a systemd user scope, recipe pointed to from the docs); this
module owns only what an app can honestly do about its own memory:
notice, record, and — per the owner's rung ruling — decline to grow
further.

Owner rulings this spec rides: the 2026-11-18 session scoped the
watchdog to the whole tree, not per-tab (the app process itself is
the historical eater; "overall, not just one tab").

## Scope

In: a pure watchdog state machine (threshold, hysteresis, one firing
per upward crossing, re-arm below the line — no toolkit, fully
pinnable); the tree reading — `/proc/self/task/<tid>/children` walked
recursively plus one bounded status read per live pid, std-only, the
kernel-maintained children map so no `/proc` scan (direct PTY
children today's sampler reads are a subset; grandchildren — a
compile job's `cc1`, a shell's server — are the coverage this adds);
a config key `memory_watchdog_mb` (`Option<u32>`, absent = off)
ridden by the `every_config_key_is_documented` gate; the notify
surface — a status-bar warning segment while over the line, plus a
`# wd` marker row in the telemetry log at each crossing (comment-
prefixed, so the documented column-extraction one-liners skip it and
`grep '^# wd'` finds it); the tick hook beside `sample_memory`; a
docs section on the diagnostics page (the threshold, the summed-RSS
double-count caveat, the systemd hard-wall recipe pointer).

Rung 2 (separate chunk, builds only on the owner's ruling at review):
refuse new tabs while over the line, and trim every pane's scrollback
through one new engine method wrapping carried
`Grid::clear_history` (upstream API — no carry change).

Out: killing children, closing tabs, or self-exit (never in-app —
the OS's job via cgroup); CPU/GPU limits; cgroup creation or
systemd integration in code; changing the closed telemetry schema
(the watchdog's walk is its own, beside the sampler's tracked-children
reads — unifying them is a possible later simplification, not this
module's); any new dependency.

## Commands

    just check

## Files

`dropterm/src/core/watchdog.rs` (new — one concept, one name), the
tick hook site and config (`dropterm/src/app_shell.rs`,
`dropterm/src/core/config.rs`), a status-bar segment
(`dropterm/src/status_bar.rs`), `docs/memory-diagnostics.md`,
`docs/configuration.md`, `docs/testing.md`. Rung 2 adds the engine
method (`dropterm/src/core/terminal/engine.rs`). Per-task ≤5-file
lists at chunk time.

## Constraints

- Zero new dependencies (the tooling record's no-new-crate verdict
  carries; the tree walk is the same std procfs reads as `core/mem.rs`).
- Overhead bounded: one children-map walk plus one status read per
  live pid per sample period, only when a threshold is configured —
  the watchdog is free when off.
- Failure-safe like the sampler: a missing `/proc` path or field
  skips that pid (or the sample); the watchdog never panics the app,
  and never blocks the tick.
- Hysteresis is pinned as a fraction (re-arm at threshold minus 10%),
  a number in a test.
- Marker rows count toward the log's existing `ROW_CAP` — the bound
  holds without new accounting.
- No secrets, no user content: the marker row is timestamps and
  numbers, like every other row.

## Testing strategy

- Headless unit pins: the state machine (crossing fires once,
  hysteresis re-arms, off-when-unconfigured), the tree walk against
  a fixture `/proc` (the `core/mem/tests.rs` pattern: grandchildren
  counted, dead pids dropped, unreadable entries skipped), the
  marker-row format skipped by the documented extraction one-liners.
- The config-gate test enforces the `docs/configuration.md` row.
- One agent-run capped smoke (the sanctioned pattern): a configured
  threshold fires exactly one marker row per crossing in a real log.
- `just check` green on every landing.

## Boundaries

- Always: docs land with the feature; the state machine stays pure
  and pinned.
- Ask first: any new dependency; any behavior beyond the ruled rungs;
  changing the telemetry schema; a default threshold below 1 GB.
- Never: killing anything from in-app; blocking the tick; unbounded
  growth in the watchdog itself.

## Success criteria

- With the key unset, behavior and logs are byte-identical to today.
- With a threshold set, an over-line tree produces exactly one status
  -bar warning and one `# wd` marker row per crossing, re-armed under
  the line; the pins and the smoke prove it; the docs section and the
  config row are landed; `just check` green.

## Open questions

1. Default off (this spec's proposal — the number should come from
   the daily curve once a post-rt-2 build has run) or on at a
   generous line (2 GB) from day one?
2. Does rung 2 (refuse tabs + scrollback trim) ship in this module's
   chunk set, or as a follow-up after the notify cut has a daily
   verdict? This spec assumes follow-up.
3. Notify surface: status-bar segment only (assumed), or also a
   desktop notification per crossing?

## Size

S (rung 1: one pure module, one walk, one segment, one config key,
docs). Rung 2 adds S.
