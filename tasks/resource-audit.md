# Chunk: resource-audit — nothing the drawer does can eat the machine

Spec: [SPEC-resource-audit.md](../SPEC-resource-audit.md) · Module id:
`resource-audit` · hardening pass, main · spec approved 2026-11-16
(owner `/planning` go-ahead) · builds after `toggle-ram` (consumes its
extended probe).

**Files touched:** wherever the vectors land — frontend under
`dropterm/src/`, core under `dropterm/src/core/` — plus
`docs/research/resource-audit.md` (new, the report),
`docs/research/README.md` (index row), `docs/testing.md`,
`docs/known-issues.md`. Per-task ≤5-file lists below; findings may
split a batch rather than grow a task.

## Task ra-1: Descriptors, processes, threads, disk — the measurable sweep

**Description:** The system-resource tranche, each vector measured or
code-cited, then dispositioned (fixed + bound + pin, filed, or
deferred by ruling). Anchors from the planning read: the paste cache
has a per-image 32 MiB cap but no runtime eviction found
(`core/paste_cache.rs` — directory growth is the vector; the remedy
ask-first: a retention policy, count/bytes cap + oldest-evict, changes
user-visible defaults); session saves are atomic temp+fsync+rename and
shape-detected debounced (`core/session.rs` — verify write frequency
and file size under churn); PTY and child reaping across tab close,
profile respawn, and the never-empty respawn (`core/tabs.rs`,
`core/spawn.rs` — zombie check by `ps` state); inotify/config-watcher
lifetime in the shell's echo-apply window; fd and thread counts under
the spam regime (probe data from tr-1); the debug trace's append-only
log (`app_shell.rs:1276`, bounded or gated — cheap fix, land it);
dbus/wayland socket counts.

**Acceptance criteria:**
- [ ] Every listed vector carries a disposition with evidence (probe
      numbers or file:line citation) in this chunk
- [ ] The debug-trace log bound lands (truncate-on-enable or size
      cap) with a pin
- [ ] The paste-cache retention remedy is ask-first with the measured
      growth curve attached (no silent default change)
- [ ] fd/thread counts under spam recorded; a leak gets its fix or
      its filed row

**Verification:**
- [ ] Headless pins for every landed fix; `just check` green
- [ ] Probe-derived numbers recorded (owner-run default; the standing
      exception if granted covers this module too)

**Dependencies:** tr-1 (probe tooling) · **Files:**
`core/paste_cache.rs`, `core/tabs.rs`, `dropterm/src/app_shell.rs`,
docs files · **Scope:** M

## Task ra-2: Memory and GPU — the retention sweep

**Description:** The memory tranche. Anchors: `scrollback_lines`
defaults 10 000 (`core/config.rs:603`) — the bound exists; the audit
verifies the engine honors it as a hard cap and documents the
worst-case cost (full buffer × grid width × tabs, measured on one
saturated tab, then the many-tabs math); grid and damage copies under
resize churn (the engine's resize path — the toggle-reprint repro pin
at 7750755 is the harness pattern); the images ledger and decoded
thumbnails in memory (`images_panel.rs`, core decode); GPUI atlas and
per-window wgpu buffers surviving recreate (direct continuation of
tr-1/tr-2's findings — same attribution discipline); theme and font
caches rebuilt per show (the theme-applied count behavior from gm-2 is
the pin pattern).

**Acceptance criteria:**
- [ ] Scrollback: cap verified honored, worst-case per-tab and
      10-tab cost measured and documented (docs row states the number)
- [ ] Atlas/GPU retention dispositioned — a bound, a filed upstream
      row, or a carried remedy (ask-first)
- [ ] Every remaining tranche vector dispositioned with evidence

**Verification:**
- [ ] Saturated-tab measurement recorded; headless pins where pure;
      `just check` green

**Dependencies:** tr-2 · **Files:** `core/config.rs` (docs of the
bound), engine/terminal view files as named by findings ·
**Scope:** M

## Task ra-3: Concurrency, panics, input-carried bombs

**Description:** The correctness-under-hostility tranche. Census: the
15 unwrap/expect sites in `app_shell.rs` and 6 in `executor.rs`,
classified startup-only vs reachable from compositor/socket input
(the gc-2 reentrancy pattern and the sr-3 apply-time retirement are
the precedents; any update-in-borrow risk gets the gc-2 test shape);
the 100 ms tick's pending queues verified bounded-drain (no queue
grows faster than the tick drains under verb flood); zbus forwarding's
5 s bound verified on every path that talks to a possibly-wedged
owner; input-carried bombs tested headless where constructible —
malformed/truncated config (the store's quarantine is the existing
guard; verify), huge paste (cap exists — verify the refusal path),
image decode bombs (pixel dimensions vs payload bytes on the decode
path), OSC 52 write size, select-all copy over a huge grid, remote
dbus verb flood (tab verbs at tick rate), rapid resize churn's
resource cost (bug 1's reprint correctness stays with the ux
findings; this row is its cost).

**Acceptance criteria:**
- [ ] The unwrap/expect census lands in the report with
      classifications; every input-reachable site is fixed, guarded,
      or filed with reasoning
- [ ] Each constructible bomb has a headless test proving the bound
      or the graceful refusal
- [ ] Verb-flood and tick-drain verdicts recorded (headless where
      possible; the live flood row is owner-run)

**Verification:**
- [ ] `just check` green with the new pins; the report cites each

**Dependencies:** tr-1 · **Files:** `app_shell.rs`, `executor.rs`,
`core/transport.rs`, clipboard/images files as named · **Scope:** M

## Task ra-4: The P1 fix batch (gate: ra-1..ra-3's findings)

**Description:** Land the fixes the tranches marked P1 but did not
take inline (inline is preferred for cheap P2s; this batch is for
what needed a decision or crossed files). Each fix its own commit,
bound stated as a number in a test or docs row, pinned headless.
Anything whose default changes user-visible behavior is ask-first
with the alternatives named.

**Acceptance criteria:**
- [ ] No P1 vector remains open without a bound or a filed row
- [ ] Each landing carries its bound as a number and its pin

**Verification:**
- [ ] `just check` green; probe rerun where the fix is measurable

**Dependencies:** ra-1, ra-2, ra-3 · **Files:** per finding, ≤5 per
landing (split rather than grow) · **Scope:** M

## Task ra-5: The report filed — and the pass closes

**Description:** `docs/research/resource-audit.md` is written and
filed: the inventory walked, every vector's disposition (fixed +
pinned, filed with evidence, deferred by owner ruling), the numbers
behind each, dated. It is a research record — never edited after
filing; corrections land as new rows. `docs/research/README.md` gains
the index row; testing.md and known-issues reflect every landing;
`just check` closes the pass.

**Acceptance criteria:**
- [ ] The report exists with a disposition for every inventory
      vector and the evidence cited
- [ ] The research index row links it; docs rows true; `just check`
      green

**Verification:**
- [ ] `just check` (docs-check rides it); owner review of the report

**Dependencies:** ra-4 · **Files:** `docs/research/resource-audit.md`,
`docs/research/README.md`, `docs/testing.md`,
`docs/known-issues.md` · **Scope:** S

## Checkpoint — the sweep is dispositioned

- [ ] Every vector: fixed + pinned, filed, or deferred by ruling. No
      P1 open. The report is filed and the owner has seen it.
