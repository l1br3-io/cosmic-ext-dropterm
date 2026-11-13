# Chunk: shared-core — one libcosmic-free crate under both frontends

Spec: [SPEC-shared-core.md](../SPEC-shared-core.md) · Module id:
`shared-core` · Phase 3 · branch `port/shared-core` (main checkout;
merges first in Phase 3).

**Files touched:** a new `dropterm-core` crate; `src/config.rs` + `src/config/`,
`src/session.rs` + `src/session/`, `src/dbus_state.rs`, `src/lib.rs`,
`src/intent.rs`, `src/tabs.rs` + `src/tabs/`, `src/profile.rs`,
`src/terminal/engine*` + `src/terminal/suggest*` (moves); `src/app/surface_lifecycle*`,
`src/geometry.rs`, `src/keybinds.rs`, `src/app/theme_glue.rs`, `src/main.rs`
(splits — their libcosmic halves stay in `dropterm`); `applet/Cargo.toml`
(dependency re-point); root `Cargo.toml` (workspace member). Shares every
file Phase 1 touched, by nature of being a move — it rebases onto main
fresh and nothing else should be in flight.

## Planning rulings (made at /planning; veto at plan review)

- **The crate is `dropterm-core`, a path member of the root workspace** —
  one lockfile stays (the check-14 absorption argument), and the applet's
  `default-features = false` consumption pattern carries over verbatim.
- **The decision layer moves** (beyond the plan's four-item list): the
  spike ruled "the state machine survives; only its executors change" — a
  state machine left libcosmic-side would be duplicated, not shared. Its
  `cosmic::app::Task` continuation becomes an executor-neutral type (the
  trait's `Effect` pattern, lifted to the machine's outputs).
- **The transport halves move with it**: both frontends serve the same
  activation shape, so the bus-name decision, forwarding, and
  `forward_outcome` live in core next to `dbus_state`.
- **Engine-side cosmic-text buffers stay frontend-side** with the widget —
  check 9's boundary ruling; they are widget-serving caches.

## Task 1: The crate; the pure movers

**Description:** `dropterm-core` exists as a workspace member; the already
libcosmic-free modules move first — config store and model (`config.rs`,
`config/`), session (`session.rs`, `session/`), tabs model (`tabs.rs`),
profiles (`profile.rs`), the intent vocabulary (`intent.rs`). `dropterm`
re-exports or re-imports them so the binary compiles unchanged in
behavior. A manifest/`cargo tree` pin test lands with the crate: zero
libcosmic/iced edges, or `just check` fails.

**Acceptance criteria:**
- [x] `dropterm-core` builds with zero libcosmic/iced dependencies, pinned
      by a test
- [x] `just check` green across the workspace; the moved modules' tests
      run from core

**Verification:**
- [x] Tests pass: `just check`

**Done (2026-11-13, port/shared-core):** moved — config (+store, +tests),
session (+tests), profile (+tests), intent (+tests), theme_source (the
theme data table; pure, engine-colors only), the keybinds model
(`Binding`/`ModifierName`; `to_key_bind` stays frontend — it produces a
toolkit `KeyBind`), the geometry model (pure math incl. the drag and
write-back machines; `anchor_margins`/`apply_drag_geometry` stay
frontend), `StatusToggles` (config's fold target; status_bar re-exports),
`command_line_parts` (profile validates through it), and `APP_ID`
(store/session read it; the dropterm lib re-exports, `dev-identity`
forwards — Task 3 shrinks to dbus_state + the applet re-point).
`tabs.rs` deferred to Task 2: it holds `Mutex<Terminal>`, the spec's own
engine-state grouping. The pin test was mutation-verified (a manifest
mention fails it; the tree half proven against the applet's real
libcosmic graph). One test moved homes: the `KeybindsWrite` store pin
now runs in settings' tests (it pins a frontend type). The
`RecordingExecutor` harness got a frontend twin in
surface_lifecycle/tests.rs (a core `cfg(test)` item is invisible across
crates); it retires with that file in Task 4.

**Dependencies:** Phase 3 entry (met) · **Files:** new crate,
the moved modules, both `Cargo.toml`s · **Scope:** M

## Task 2: The split movers — engine, geometry, keybinds, theme glue

**Description:** The mixed modules split along their pure/wiring seam:
engine state (`terminal/engine*`, suggest model and data) moves minus the
cosmic-text buffers and `widget.rs`; `geometry.rs`'s model moves, its
libcosmic conversion stays; `keybinds.rs`'s chord map moves, the
libcosmic action mapping stays; `theme_glue.rs`'s pure decisions move with
the system-palette reader left behind, passed in packed.

**Acceptance criteria:**
- [ ] Core's engine tests (grid, suggest fixtures) run green from core
- [ ] The libcosmic halves compile against core's types; `just check`
      green

**Verification:**
- [ ] Tests pass: `just check`

**Dependencies:** Task 1 · **Files:** `src/terminal/`, `src/geometry.rs`,
`src/keybinds.rs`, `src/app/theme_glue.rs` + their frontend remainders ·
**Scope:** L

## Task 3: Identity, contract, and the applet re-point

**Description:** `APP_ID` and `dbus_state` move from the `dropterm` lib
target into core (the lib target shrinks to what the applet still needs,
or disappears if nothing remains); `applet/Cargo.toml` consumes
`dropterm-core` with `default-features = false`. The dbus↔docs pin test
moves with the module.

**Acceptance criteria:**
- [ ] The applet builds and its tests pass against `dropterm-core`
- [ ] The `every_dbus_member_is_documented` pin green from its new home

**Verification:**
- [ ] Tests pass: `just check` (all members)

**Dependencies:** Task 1 · **Files:** `src/lib.rs`, `src/dbus_state.rs`,
`applet/Cargo.toml`, `applet/` imports · **Scope:** S

## Task 4: The decision layer and transport move

**Description:** `surface_lifecycle*` moves onto core (the `Task`
continuation replaced by the neutral effect type; the libcosmic executor
maps it back), and `main.rs`'s transport halves (bus-name decision,
`forward_to_running`, `forward_outcome`, platform-data) move beside
`dbus_state`. The ~25 decision-layer tests and the forward-outcome tests
move with their code, unchanged.

**Acceptance criteria:**
- [ ] The decision-layer tests green in core, unchanged in content
- [ ] `forward_outcome` tests green in core

**Verification:**
- [ ] Tests pass: `just check`

**Dependencies:** Tasks 1–2 · **Files:** `src/app/surface_lifecycle*`,
`src/executor.rs` (effect mapping), `src/main.rs` · **Scope:** M

## Task 5: No-behavior-change walk and merge

**Description:** The contract is zero behavior change. Surface rows of the
manual checklist walk on the validation VM against an installed build
(snapshot discipline per development.md); then merge — first Phase 3
merge, so the branch is cut fresh from main and has no rebase partner.

**Acceptance criteria:**
- [ ] The manual surface rows pass on COSMIC (show, drag, live edit,
      focus-loss, hide — the si-5 set)
- [ ] `just check` green on the merge result

**Verification:**
- [ ] Manual: the checklist's surface rows (VM walk)
- [ ] Tests pass: `just check`

**Dependencies:** Tasks 1–4 · **Files:** the branch's diff · **Scope:** S

## Checkpoint: the core stands alone

- [ ] `dropterm-core` builds with zero toolkit deps, pinned by test; the
      libcosmic binary and applet behave identically — the base every
      GPUI slice builds on
