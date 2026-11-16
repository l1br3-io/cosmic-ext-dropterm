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
- [x] Every listed vector carries a disposition with evidence (probe
      numbers or file:line citation) in this chunk
- [x] The debug-trace log bound lands (truncate-on-enable or size
      cap) with a pin
- [x] The paste-cache retention remedy is ask-first with the measured
      growth curve attached (no silent default change)
- [x] fd/thread counts under spam recorded; a leak gets its fix or
      its filed row

**Verification:**
- [x] Headless pins for every landed fix; `just check` green
- [x] Probe-derived numbers recorded (owner-run default; the standing
      exception if granted covers this module too — tr-1's owner data
      is the probe record; no exception was needed)

**Dispositions (2026-11-17, evidence: census citations + tr-1's
owner-run probe data):**

1. **Paste cache — FIXED (770ffe8, owner ruling: bytes cap +
   oldest-evict at 256 MiB).** Growth was monotonic with no
   eviction anywhere (one exclusive-create `0600` file per paste,
   `paste_cache.rs:132-185`; per-file 32 MiB cap only, `:26`);
   100 screenshot pastes ≈ 0.1–0.3 GB, abuse ceiling 32 MiB ×
   count. The ask-first carried the math; the ruling chose the cap.
   `PASTE_CACHE_CAP` + `enforce_paste_cache_cap_in` evict
   oldest-first (generated names are timestamp-stamped, so name
   order is creation order) after each write, never the fresh
   paste, never foreign names — five pins; known-issues row
   rewrites "never sweeps" to "capped, not swept".
2. **Debug-trace log — FIXED (4b46fd1).** The env-gated dump
   appended per call with no reset (`app_shell.rs`'s
   `dump_visible_grid`); the process's first dump now truncates —
   one session's evidence per file — pinned by
   `the_toggle_log_resets_on_the_first_dump_and_appends_after`.
   The owner's tr-1 class measurement (bounded-per-dump) stands as
   the within-run verdict.
3. **Session saves — CLEAN (code-cited).** Metadata-only per tab
   (title/shell/cwd/profile/pinned, `session.rs:37-60` — never the
   grid), shape-gated with a 500 ms debounce
   (`SESSION_WRITE_INTERVAL` `session.rs:262`, pumped from the
   100 ms tick `app_shell.rs:1023-1043`), atomic
   temp+fsync+rename (`session.rs:422-443`), overwrite-in-place
   with one `.bak` only on corrupt-load quarantine. Grows only
   with tab count at ~hundreds of bytes per tab (100 tabs ≈ tens
   of kB). Uncapped tab count itself is carried below.
4. **PTY + child reaping — CLEAN (code-cited).** Close → Terminal
   drop → notifier channel closes → pinned alacritty fork's reader
   thread exits → `Drop for Pty` does SIGHUP + blocking `wait()`
   (fork `tty/unix.rs:311-320`); SIGCHLD-driven `try_wait` while
   open (`unix.rs:381-393`); close path `app_shell.rs:1686-1713`,
   profile respawn replaces the pane (`app_shell.rs:2658`), the
   never-empty respawn runs after the old child is reaped; the
   suggest generator SIGKILLs + reaps its one-shot child
   (`engine.rs:1286-1310`). No zombie path found; the reap is
   asynchronous on the reader thread by design.
5. **inotify/config watcher — CLEAN.** Exactly one
   `recommended_watcher`, armed at boot on the config dir, its
   thread parked for the process lifetime
   (`app_shell.rs:4392-4407`); echo-apply drains through the same
   single watcher (`app_shell.rs:924-943`); never recreated. One
   fd, total.
6. **fd/thread counts under spam — CLEAN (measured, tr-1
   owner-run).** fds 53–61, threads 45–47 across all three regimes
   × 150, no upward trend (spam50 even ended 53/45). The census
   inventory reconciles the absolute count: std 2–3, one wayland
   display + gpui-pre's shm/poll fds, one zbus session connection
   + tokio internals (`dbus.rs:127-171`), 1 inotify, ~5–6 per PTY
   tab, transient bell/clipboard connections.
7. **dbus/wayland sockets — CLEAN.** Three persistent connections,
   none per tab/toggle/window (wayland display; the one served zbus
   connection held for the process lifetime; inotify). Transients:
   notify-rust opens one zbus connection per bell; wl-clipboard-rs
   opens its own per data-control copy/paste, the serve thread held
   only while an offer stands (`terminal/clipboard.rs:24-60,96-108`)
   — bounded by event rate, closed after. Boot probes' connections
   drop at startup (`startup.rs:43`, `transport.rs:38-45`).
8. **Other disk writes — CLEAN.** No logging-framework file sinks
   (the `log` facade with println/eprintln only); config writes
   atomic, fixed-schema (`config/store.rs:81-106`); no memfd, no
   other /tmp paths, no other cache dirs.
9. **Carried: uncapped tab count** (~5–6 fds + one engine per tab;
   the session file grows with it). Opening tabs is ordinary use,
   but the count is user-chosen — abuse-class P2, carried to ra-2's
   many-tabs cost math and the report; a cap would change
   user-visible defaults, ask-first only if that math makes it
   matter.

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
