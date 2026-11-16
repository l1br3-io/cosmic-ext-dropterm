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
- [x] Scrollback: cap verified honored, worst-case per-tab and
      10-tab cost measured and documented (docs row states the number)
- [x] Atlas/GPU retention dispositioned — a bound, a filed upstream
      row, or a carried remedy (ask-first)
- [x] Every remaining tranche vector dispositioned with evidence

**Dispositions (2026-11-17, census citations; tr-1/tr-2 probe data
for the upstream-owned halves):**

1. **Scrollback — VERIFIED + DOCUMENTED (515e5a0).** The cap is a
   hard ring: the fork clamps growth at
   `min(count, max_scroll_limit - history)` and shrinks through
   `update_history` (fork `grid/mod.rs:154-180`), reached only via
   the spawn request (`scrolling_history` from
   `effective_scrollback`, `view.rs:2168-2171` → `engine.rs` Term
   config) — spawn-fixed per tab, the documented "applies to the
   next spawn" semantics. Worst case: 24 B/cell (fork
   `term/cell.rs:162-170`, char + two Colors + u16 flags + Option
   Arc niche) × columns × (history + rows + the storage's ≤1 000-row
   headroom, `grid/storage.rs:13,107-138`) = **~53 MB (50.5 MiB) per
   saturated tab** at the default and 200 columns; **10 saturated
   tabs ≈ 0.5 GB**. configuration.md's row states the numbers;
   `the_scrollback_worst_case_math_is_pinned_to_the_cells_size`
   pins the basis (Cell size, default, arithmetic). Owner-run live
   confirmation instructed 2026-11-17 (below).
2. **Resize copies — CLEAN.** dropterm-side resize is the PTY ioctl
   + `Term::resize` (`engine.rs:2648-2706`); the per-paint
   `display_rows` Vec is rebuilt and freed per frame
   (`view.rs:1553-1559`). Fork-side reflow allocates transient Vecs
   per wrapped row; the retained effects are the damage Vec replaced
   in place and the bounded ≤1 000-row headroom — nothing grows with
   resize count.
3. **Images in memory — one FIXED, rest CLEAN/BOUNDED.** Ledger
   rows are metadata (header-only dims read, `core/tabs.rs:139-155`)
   and die with the tab. The pane's `ImageCache` handles die with
   the pane (`terminal/images.rs:25-58`). The engine's decoded-store
   is budgeted **320 MiB per tab with oldest-evict**
   (`IMAGE_STORE_BUDGET`, `engine.rs:4141`, eviction pinned by
   `removals_feed_the_frontend_handle_cache`) — a stated bound, but
   per-tab not global: N tabs of image traffic worst-case N ×
   320 MiB. Lowering or globalizing changes user-visible defaults →
   **filed for the report/owner** (P2: needs N tabs each carrying
   image output), not silently changed. The shell's
   `images_thumbs` map was the leak-shaped one — full-size decodes
   for every path ever panel-shown, surviving tab close — **FIXED
   (5c4a159)**: orphans drop at close, the union of live ledgers the
   rule (duplicates share paths), pinned by
   `the_thumbs_cache_sheds_orphans_when_a_tab_closes`.
4. **Theme and font caches — CLEAN (one cost note).** Theme schemes
   are compile-time consts (`theme_source.rs`), no runtime cache;
   the apply path is identity-gated (gm-2's pinned behavior). The
   terminal font rides GPUI's app-level text system (one clone per
   pane, `host.rs:43-79`); status/glyph caches are GPUI's. Cost
   note, no fix: fontdb does a full system-font scan per
   settings-drawer open (`font_list.rs:35-38` ← `drawer.rs:298`,
   drawer dropped on close) — transient, user-triggered, bounded;
   caching it would change nothing user-visible for real cost.
5. **Atlas/GPU — UPSTREAM-OWNED, MEASURED CLEAN.** dropterm
   allocates no GPU/wgpu resources itself (two paint-hook
   `gpui::canvas` uses only; images ride upstream
   `paint_image`/`RenderImage`). tr-1/tr-2's owner-run probe shows
   no `/dev/dri` or memfd growth across toggle churn — nothing to
   file; window/compositor retention is GPUI's and measured flat.

**Saturated-tab confirmation (owner-run, instructed 2026-11-17):**
with dropterm running, read `grep VmRSS /proc/$(pidof
dropterm)/status`, fill one tab's scrollback at full width — `awk
'BEGIN{line="";for(j=0;j<200;j++)line=line "x";for(i=0;i<12000;i++)
print line}'` — and read VmRSS again: the delta should sit near the
pinned ~50 MB (the storage only touches what the fill wrote).

**Verification:**
- [ ] Saturated-tab measurement recorded; headless pins where pure;
      `just check` green — pins landed and `just check` green on
      both commits; the saturated-tab confirmation is the owner-run
      half, reported verdict pending

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
- [x] The unwrap/expect census lands in the report with
      classifications; every input-reachable site is fixed, guarded,
      or filed with reasoning
- [x] Each constructible bomb has a headless test proving the bound
      or the graceful refusal
- [x] Verb-flood and tick-drain verdicts recorded (headless where
      possible; the live flood row is owner-run)

**Census + dispositions (2026-11-17):**

*The panic census:* zero bare `unwrap`/`panic!`/`unreachable!` in
non-test code across `app_shell.rs`, `executor.rs`, `dbus.rs`,
`core/transport.rs` (the planning-read "15+6" were `expect`s).
Classification: one **input-reachable** site — the drawer env-row
`Vec::remove` on a stale captured index — **FIXED** (d4fa558 +
10b2235, `remove_env_row` in core guarded like its own `get_mut`
edit paths, pinned by `a_stale_env_row_index_removes_nothing`).
The rest: startup/generator-regression tripwires (the theme-parse
expect, pinned by theme_gen's tests), liveness expects by design
(executor's "an intent names a live surface", `#[should_panic]`-
pinned; the `record` None arm is invariant-guarded dead), poison
amplifiers not origins (the engine-lock expects — a PTY-thread
panic is loud at the next tick by design, the loud-miss rule),
saturating casts, fixed-table indexing. Recorded, no change
warranted; weakening any would break the loud-miss Never.

*The tick's queues:* every accumulator drains to empty per tick or
is a single-slot Option (census table in the research record);
`verbs` is unbounded-between-ticks but drained wholesale, with the
150 ms debounce capping *effects* at ~6.7/s and hop 1 (dbus →
serve loop) bounded at 64 with drop+warn — pinned
(`a_full_queue_drops_the_verb_and_still_replies_ok`). Bounded-drain
holds. *zbus:* every owner-directed wait is now bounded — dea31aa
closed the one gap (`--is-visible`'s property read, transport.rs),
applying `FORWARD_DEADLINE_SECS` exactly as the forward does;
recorded un-pinned with reasoning: BUS_NAME/session-bus hardcoding
makes a headless wedged-owner fixture impractical without a
testability refactor, and the wrap is the sf-4-walked pattern
verbatim. *Reentrancy:* the gc-2 fix stands; every spawn/update
site audited against it, none re-enters a live borrow.

*The bombs:* config malformed/truncated — quarantine + defaults
verified with the store's existing pins; huge config file
uncapped — **FILED** (P2: the owner's own config file,
self-inflicted). Text paste — was uncapped whole into RAM and the
PTY; **FIXED** (d4165da, owner ruling 32 MiB refuse-over): the
primary road takes the pipe at cap+1 (true read-side bound), the
GPUI road post-checks (GPUI has already read by the time the app
sees it — noted), refusals ride the existing band verdict
(`PasteRefusal::OverCap`); the shared split is the already-pinned
`classify_read`. Image decode bombs — verified fully guarded
(32 MiB payload, 8192 px dimension, 64 MiB alloc incl. the
explicit `w*h*4` pre-check, capped inflate, first-frame-only GIF)
with the existing pin battery; the one unpinned branch (inflate
over-cap) now pinned (e556a91). OSC 52 store + load reply — were
uncapped verbatim; **FIXED** (d4165da, same ruling): over-cap
stores drop logged, over-cap load replies never feed the PTY.
Select-all copy — bounded by grid capacity by construction (the
ra-2 scrollback math documents the ceiling); clean. Dbus verb
flood — hop 1 bounded/pinned, drain exhaustive, but the unbounded
between-tick queue plus no tab cap means a sustained `new-tab`
flood spawns N ptys in one tick — **FILED** (P2: same-session
caller class; the wire hop already drops at 64; the live flood
row rides the owner's pass). Resize churn — equality-gated, one
reflow per changed frame, per-resize work transient (ra-2); clean.

**Verification:**
- [x] `just check` green with the new pins; the report cites each

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
