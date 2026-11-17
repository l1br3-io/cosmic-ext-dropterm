# SPEC: resource-audit — nothing the drawer does can eat the machine

Status: not started · hardening pass · main · spec drafted 2026-11-16,
approval pending (builds after `toggle-ram`)

## Objective

A systematic sweep for every path that can crash the app or exhaust
the machine's resources, beyond the toggle path `toggle-ram` owns.
The owner's standing concern: identify anything that can make the
application crash or overflow the machine's resources. Every vector
found gets one of three dispositions: fixed with a stated bound and a
pin, filed in known-issues with evidence, or deferred by owner ruling.
The audit report is a research record under `docs/research/` and is
never edited after it is filed.

## Vector inventory (starting checklist, refined during the sweep)

- **Memory.** Engine scrollback retention per tab (is the bound real,
  what does a full buffer cost across many tabs); grid and damage
  copies under resize churn; the images ledger and decoded thumbnails
  in memory; GPUI atlas and per-window GPU/wgpu buffers surviving
  recreate; theme and font caches rebuilt per show.
- **File descriptors.** PTYs across tab lifecycle and profile respawn;
  inotify watchers (config notify); wayland/dbus sockets; /tmp debug
  logs; fd count across the spam regime.
- **Processes.** Child reaping (zombie shells after exits), the
  never-empty respawn and switch-profile respawn paths under rapid
  verbs, duplicate-tab storms.
- **Disk.** The paste cache's total growth (a per-image 32 MiB cap
  exists; does the directory itself grow unbounded?), session saves
  (shape-detected debounced: write frequency and file size), config
  writes, debug logs when enabled.
- **Threads.** Accumulation per toggle or per tab.
- **Concurrency and panics.** Reentrancy on the gc-2 precedent; the
  executor's retire-versus-map races on the sr-3 precedent; unwrap and
  expect on compositor-driven input; unbounded channels or timers (the
  slide clock, echo-apply quiet windows); zbus forwarding is bounded
  at 5 s (verify it holds on every path).
- **Input-carried.** Malformed or truncated config; huge paste
  payloads; image decode bombs (pixel dimensions vs bytes); OSC 52
  writes; select-all copy over a huge grid; dbus verb floods (the
  remote tab verbs); rapid resize churn (its resource cost; bug 1's
  reprint correctness lives with the ux findings, not here).

Each row records: priority (P1 reachable by ordinary use, P2 edge or
abuse), disposition, evidence.

## Scope

In: the sweep over the one GPUI binary (frontend and core); fixes for
P1 vectors and cheap P2s, each with a stated bound; filed rows for the
rest; the audit report (`docs/research/resource-audit.md`) with its
index row in `docs/research/README.md`; `docs/testing.md` and
known-issues updates.

Out: upstream fixes (cosmic-comp, GPUI, the Kit) — filed or carried
per the kit-root-border precedent, not forked beyond that; feature
changes; performance work that is not a resource bound.

## Commands

    just check
    just build-release

The probe and its extensions ride `toggle-ram`'s tooling.

## Files

Wherever the vectors land (frontend modules under `dropterm/src/`,
core under `dropterm/src/core/`), `docs/research/resource-audit.md`
(new), `docs/research/README.md` (index row), `docs/testing.md`,
`docs/known-issues.md`. Per-task ≤5-file lists at chunk time.

## Constraints

- Every stated bound is a number in a test or a docs row, never a
  vague "large enough".
- Fixes target the root cause; a cap added at the edge is legitimate
  only when the growth is by design (a cache) and the cap is the
  documented contract.
- Dispositions are honest: "filed, not fixed" rows stay visible in
  known-issues.

## Testing strategy

- Pure bounds (caps, reaping, retention) pinned headless inline per
  house rules.
- Measured vectors (fds, threads, disk, memory) evidenced through the
  extended probe, owner-run per the 2026-11-14 ruling.
- Panic-path hardening pinned by the failure it prevents (gc-2's
  reentrancy test pattern).
- `just check` green on every landing.

## Boundaries

- Always: the report is a dated research record; every fixed vector
  names its bound; docs rows land with the fix.
- Ask first: any bound that changes user-visible behavior by default
  (a scrollback cap's default, a cache eviction policy); dependencies.
- Never: swallow panics or weaken the loud-miss panics; mark runtime
  rows green that an owner did not report; edit the report after it is
  filed.

## Success criteria

- The inventory above is walked with a disposition for every vector:
  fixed and pinned, filed with evidence, or deferred by owner ruling.
- No P1 vector is left without a bound or a filed row.
- The report exists with its research-index row; `docs/testing.md` and
  known-issues reflect the landings; `just check` green.

## Open questions

`toggle-ram`'s findings are this module's first input; nothing else
blocking.

## Size

M to L (the checklist says M; findings can grow it).
