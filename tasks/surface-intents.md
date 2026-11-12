# Chunk: surface-intents — a toolkit-neutral surface vocabulary

Spec: [SPEC-surface-intents.md](../SPEC-surface-intents.md) · Module id:
`surface-intents` · Phase 1, lane A · branch `port/surface-intents`
(worktree `../ced`, after `app-identity` merges).

**Files touched:** `src/app/surface_lifecycle.rs` (with its tests at
`:793-1122` today), `src/app.rs` (the surface handlers), `src/geometry.rs`,
`src/surface.rs`, plus the new executor module and tests. Shares
`src/app.rs` with `config-store` — different regions, second merger
rebases.

## Planning rulings (made at /planning; veto at plan review)

- **The intent vocabulary is one enum**, each variant declarative
  desired-state (the desired surface: output choice, geometry, layer,
  keyboard mode, exclusive zone, visibility), carrying a DropTerm
  surface handle the executor maps to the toolkit's window id. This
  follows constraint 1 verbatim: no `set_anchor`/`set_margin`/
  `set_layer`-shaped requests — they are unimplementable on GPUI, where
  those are create-only. The executor reconciles desired against
  current and chooses to mutate or recreate. Exact variant set is
  Task 1's, discovered from the code, inside those constraints.
- **The trait and the recording executor land together** (design rule
  5: the recording executor is the second implementation that justifies
  the trait). The trait lives inside the binary — no `dropterm-platform`
  crate, no GPUI-shaped abstraction (Phase 3's ids).

## Task 1: The vocabulary, trait, and recording executor

**Description:** Define the intent enum, the executor trait, and the
recording executor; drive a first headless show/hide through the intent
layer and assert the recorded sequence. Types are `window::Id`-free
from the start — a DropTerm surface handle instead.

**Acceptance criteria:**
- [x] A headless show/hide test drives the intent layer with the
      recording executor and asserts the intent sequence
- [x] No iced/libcosmic type appears in the intent vocabulary

**Verification:**
- [x] Tests pass: `just check`

**Dependencies:** `app-identity` merged (rebase onto it) · **Files:**
`src/app/surface_lifecycle.rs` (or its new sibling module) · **Scope:**
M

## Task 2: The libcosmic executor, pinned

**Description:** The real executor turns each intent into today's exact
layer-shell command sequence — including the anchor heal chained ahead
of the measuring probe at show and the stale-echo gate before a reshape
(`request_geometry_height`). A translation pin test fixes the mapping
intent-by-intent, request order included; request-order changes stay
out unless a test deliberately pins a new order.

**Acceptance criteria:**
- [x] A test pins the executor's translation of each intent into
      today's command sequence, order included

**Verification:**
- [x] Tests pass: `just check`

**Dependencies:** Task 1 · **Files:** the executor module,
`src/surface.rs` as read-only reference · **Scope:** M

## Task 3: The decision layer migrates

**Description:** The state machine's executors (`show`, `hide`,
`reverse_show`, `focus_lost`, `debounced_toggle`) return intents instead
of `cosmic::app::Task`; the ~25 decision-layer tests
(`src/app/surface_lifecycle.rs:793-1122`) move onto the intent types,
`window::Id`-free. The clipboard anchor stays private to the libcosmic
executor (out of scope).

**Acceptance criteria:**
- [x] The decision layer's tests all run against intent types; no
      `window::Id` in them
- [x] `just check` green

**Verification:**
- [x] Tests pass: `just check`

**Dependencies:** Task 2 · **Files:** `src/app/surface_lifecycle.rs`,
its tests (or the sibling `tests.rs` per the repo's size rule) ·
**Scope:** L

## Task 4: The app handlers emit intents; the full-lifecycle test

**Description:** The `SurfaceOpened`, `ConfigChanged`-reshape, drag, and
slide-end handlers in `src/app.rs` route through the intent layer. One
headless test drives the whole contract path: show, first configure,
drag, live edit, hide, a mid-slide reversal, and a focus-loss hide,
asserting the intent sequence end to end.

**Acceptance criteria:**
- [x] The full-lifecycle headless test passes through the intent layer
      with the recording executor
- [x] `just check` green

**Verification:**
- [x] Tests pass: `just check`

**Dependencies:** Task 3 · **Files:** `src/app.rs` (surface handlers),
`src/geometry.rs` as needed · **Scope:** M

## Task 5: No-behavior-change verification and merge

**Description:** The contract is no behavior change: the surface rows of
the manual checklist in [testing](../docs/testing.md) pass on COSMIC
against an installed build (`just build-release && sudo just install`).
Then merge (after `app-identity`; rebase over `config-store`'s regions
if it merged first).

**Acceptance criteria:**
- [x] The manual surface rows pass on COSMIC — walked on the
      validation VM (the sanctioned evidence route,
      development.md#the-cosmic-validation-vm; snapshot
      `pre-surface-intents-walk` taken first, VM rewound after;
      evidence in this lane's `target/vm-evidence-si/`, host-side):
      show at 40% pixel-exact (y=320 of 800) with the block caret at
      the first frame, hide/release/destroy traced on the wire
      (keyboard release at slide-out start, destroy 133 ms later =
      the 130 ms slide + parking tick), background job survives a
      hide/show cycle, drag live-applies in real time with the
      release settle + write-back persisted (height key file 53,
      every later show exactly 53%), live edit reshapes the shown
      surface in place (560→240, same pid and surface), focus-loss
      hides on a desktop click with the instance surviving and the
      caret back on re-show. The mid-slide reversal is unreachable
      via the CLI by construction (150 ms debounce > 130 ms slide —
      a second activation always lands post-completion; wire-verified
      as a fresh measuring probe) and is headless-verified
      (`decide_reverse_show` + the lifecycle test); its keybind press
      belongs to a human at the console per the runbook.
- [ ] `just check` green on the merge result — **merge is
      owner-gated**: main has moved past `ffb612d` (layer-shell-guard
      and the Phase 0 sessions; both sides touch `src/main.rs` —
      rebase first); config-store has not merged yet

**Verification:**
- [x] Manual: the checklist's surface rows (show, drag, live edit,
      reversal, focus-loss, hide) — VM walk above; two VM-harness
      artifacts worth knowing: a stale `cosmic-ext-dropterm` binary
      from a previous walk's install survives `vm-push`'s anchored
      pkill and confused the first instance's D-Bus activation
      (removed; the walk then ran clean), and the app's config store
      is per-key files under `v1/` (`height_percent`), so hand edits
      to `config.ron` are inert — the live-edit row must edit the key
      file

**Dependencies:** Task 4 · **Files:** the branch's diff · **Scope:** S

## Checkpoint: the lifecycle is headless

- [ ] The whole surface lifecycle is tested headless through intents —
      the property that makes a spike REDESIGN survivable (the state
      machine survives; only executors change)
