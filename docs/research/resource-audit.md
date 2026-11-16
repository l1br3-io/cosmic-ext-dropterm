# What can eat the machine? — the resource-audit sweep

Date: 2026-11-17 · Module: `resource-audit` (hardening pass, opened on
`toggle-ram`'s checkpoint) · Record: immutable — corrections land as
new rows, this text never changes.

## TL;DR

The sweep walked every vector in
[SPEC-resource-audit.md](../../SPEC-resource-audit.md)'s inventory
across the one GPUI binary. **Eight bounds landed** (two by owner
ruling), **ten vectors verified bounded with code citations**, and
**three filed as P2** with their evidence here. No P1 vector remains
open. Every stated bound is a number pinned by a test or stated in a
docs row — the card at the end lists them all.

## Method

Read-only censes with file:line citations (the panic census, the
queue/reentrancy census, the bomb-guard census, and the two memory
censes), cross-checked against `toggle-ram`'s owner-run probe data
(three regimes × 150 toggles, build `1b68d22c…`). Every fix landed
its own commit with a headless pin; the chunks in
[tasks/resource-audit.md](../../tasks/resource-audit.md) carry the
per-task dispositions verbatim.

## Bounds that landed

| Bound | Number | Landed |
|---|---|---|
| The toggle-dump trace never crosses runs | first dump truncates, later append | 4b46fd1 |
| The paste cache directory | 256 MiB, oldest-evicted after each write, never the fresh paste (owner ruling 2026-11-17) | 770ffe8 |
| Panel thumbnails follow the data | orphaned decodes drop with their tab | 5c4a159 |
| A stale env-row index | removes nothing, never panics | d4fa558, 10b2235 |
| `--is-visible`'s owner wait | 5 s (`FORWARD_DEADLINE_SECS`, the sf-4 rule applied to the query path) | dea31aa |
| Text paste reads (regular + primary) | 32 MiB refuse-over, the band's verdict chip narrating (owner ruling 2026-11-17) | d4165da |
| OSC 52 store + load reply | 32 MiB refuse-over, dropped logged | d4165da |
| The kitty inflate's over-cap branch | detected one byte past, pinned | e556a91 |

## Verified bounded (numbers and citations in the chunks)

- **Scrollback**: a hard ring cap in the pinned engine fork, fixed at
  spawn; worst case **~53 MB (50.5 MiB) per saturated tab** at the
  default 10 000 lines and 200 columns (24 B/cell, ≤1 000-row storage
  headroom) — stated in [configuration.md](../configuration.md),
  basis pinned in the engine tests.
- **Inline-image decode**: 32 MiB payload, 8192 px dimensions,
  64 MiB allocation (with the explicit `w*h*4` pre-check covering the
  image crate's PNG gap), capped inflate, first-frame-only GIF — the
  existing pin battery plus the inflate branch.
- **The engine's decoded-image store**: 320 MiB per tab,
  oldest-evicting, dying with the pane (see filings for the
  per-tab-not-global note).
- **Session saves**: metadata-only, atomic temp+fsync+rename,
  shape-gated with a 500 ms debounce; grows only with tab count at
  ~hundreds of bytes a tab.
- **PTYs and children**: close → drop → SIGHUP + blocking `wait()` on
  the reader thread; SIGCHLD-driven `try_wait` while open; every
  respawn path rides the same chain; the suggest generator reaps its
  one-shot child.
- **The config watcher**: exactly one inotify fd, armed at boot, held
  for the process lifetime, never recreated.
- **dbus/wayland sockets**: three persistent connections, none per
  tab/toggle/window; bell and clipboard transients bounded by event
  rate and closed after.
- **The 100 ms tick's queues**: every accumulator drains to empty per
  tick or is a single-slot `Option`; the verb hop off dbus is bounded
  at 64 with drop+warn; toggle *effects* debounce-capped at ~6.7/s.
- **Reentrancy**: the gc-2 fix stands; every spawn/update site
  audited against it.
- **Resize churn, select-all copy, theme/font state, GPUI
  atlas/GPU**: transient or bounded by construction (per-resize work
  is transient with a bounded ≤1 000-row headroom; select-all is
  grid-capacity-bounded — the scrollback math is its ceiling; theme
  schemes are compile-time consts; dropterm allocates no GPU
  resources itself and `toggle-ram`'s probe measured no `/dev/dri` or
  memfd growth).

## Filed, not fixed (P2, rows in [known-issues](../known-issues.md))

1. **The decoded-image store is per-tab, not global** — N tabs of
   image traffic worst-case N × 320 MiB. Lowering or globalizing
   changes user-visible defaults; ask-first, not silently changed.
2. **A dbus verb flood / uncapped tab count** — the wire hop drops at
   64, but a sustained `new-tab` flood spawns N ptys inside one tick
   and nothing caps the fleet. Same-session caller class.
3. **A huge config file is read whole** — no size ceiling on the
   owner's own section files; self-inflicted by definition.

Cost observation, no row: fontdb does a full system-font scan per
settings-drawer open (transient, user-triggered, dropped on close).

## Carried

- The ra-4 P1 batch is **empty by construction**: every P1-class
  finding was taken inline by its tranche.
- Owner-run confirmations instructed 2026-11-17: the saturated-tab
  RSS delta (against the pinned ~50 MB math) and the live verb-flood
  row; verdicts pending, recorded in the chunk when reported.

## Sources

The five censes (2026-11-17, this repository at 17767c1 + the
module's commits through 1299312), `toggle-ram`'s owner-run probe
series ([tasks/toggle-ram.md](../../tasks/toggle-ram.md)), and the
pinned alacritty fork at rev 1185ffd.
