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
- [x] Core's engine tests (grid, suggest fixtures) run green from core
- [x] The libcosmic halves compile against core's types; `just check`
      green

**Verification:**
- [x] Tests pass: `just check`

**Done (2026-11-13, port/shared-core):** the engine (`engine.rs` +
tests, suggest + tests + walker) and `tabs.rs` moved, minus exactly
the widget-serving halves the spec names: the cosmic-text buffers now
live behind a `TextHost` trait the frontend implements (`CosmicHost`,
iced's global font system) and core tests drive with `PlainHost`
(cosmic-text's own FontSystem — the shaper without the toolkit; GPUI's
stack is cosmic-text-based at this pin, spike check 9, so the crate in
core's graph costs that frontend nothing); the iced input translation
(`report_mouse`/`scroll_mouse`/`scroll_as_arrows` + `MouseReporter`)
moved onto the widget's state; `ImageStore`'s iced handle cache split
frontend-side with a core `take_removed` eviction feed (pinned by
test); `CommandTimer` and `ssh_destination` moved with their only
consumer (the engine); theme glue's pure decisions (`window_bg_color`,
`effective_colors`, `effective_is_dark`, `theme_override`) are
`dropterm_core::theme_glue` with their tests, the palette reader and
`set_theme` task stay frontend. The widget kept every
`with_buffer`/`buffer_weak` path — Terminal delegates to the host.

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
- [x] The applet builds and its tests pass against `dropterm-core`
- [x] The `every_dbus_member_is_documented` pin green from its new home

**Verification:**
- [x] Tests pass: `just check` (all members)

**Done (2026-11-13, port/shared-core):** `APP_ID` already stood in
core (Task 1's note), so this landed as the dbus_state move plus the
retirement: `dbus_state.rs` moved with all 19 tests (the wire tests'
`cosmic::iced::futures` import became a plain `futures` dev-dependency
— version-matched to the lock's 0.3.34 line, zero new packages — and
the dbus↔docs pin re-pathed to `../../docs/dbus.md`, the config
precedent); the `dropterm` lib target disappeared because nothing it
served remained — `font_list` (bin-only, its only caller the settings
drawer) became a bin module, `APP_ID` reads `dropterm_core::APP_ID`
directly, and the `[lib]` section went; the applet's dependency
re-pointed to `dropterm-core` with `default-features = false` carried
over verbatim (the planning ruling) and `dev-identity` forwarding
straight to core — both ends of the contract still move together, now
one hop shorter. The root's zbus declaration kept its `tokio` feature
(transport and control bus still use it directly) and dropped the
test-only `p2p`. docs/architecture.md's crate list and lint-table line
re-truthed; its module inventory still describes the pre-split tree
(Task 1–2 debt, one coherent pass owed before merge).

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
- [x] The decision-layer tests green in core, unchanged in content
- [x] `forward_outcome` tests green in core

**Verification:**
- [x] Tests pass: `just check`

**Done (2026-11-13, port/shared-core):** the decision core
(`SurfaceMachine`, `SurfaceInputs`, the guards, `toggle_step`,
`TOGGLE_DEBOUNCE`, and `shown_keyboard_mode` from surface.rs) moved
to `dropterm_core::surface_lifecycle` with its 44 tests — assertions
byte-identical; the one vocabulary change is the funnel's action
enum: the CLI's clap-derived `ToggleAction` stays frontend (Task 4's
file list never named remote.rs, and core owning a CLI enum would
hand the applet clap), so core owns `ToggleIntent` — the three
surface verbs the funnel actually acts on — and the frontend maps
onto it with the old unreachable arm (remote verbs apply outside the
funnel) as the total match that proves it. The slide *model*
(Direction/Active/Playing/offset/reverse/finished/show_slide +
constants and tests) moved with the machine — it is machine state —
as `dropterm_core::slide`; the SlideBox widget and translation_y
stay frontend, re-exporting the model so every `crate::slide::`
path kept resolving. The `RecordingExecutor` frontend twin retired
as Task 1 predicted (core's own `cfg(test)` twin is same-crate
visible now). main.rs's transport halves (`bus_name_status`/
`BusName`, `platform_data`, `forward_to_running`,
`forward_outcome`) moved as `dropterm_core::transport` with the four
forward-outcome/platform-data tests; `forward_to_running` grew its
one seam — the verb name and forward words are parameters now (the
frontend serializes through `CosmicFlags::args`, libcosmic's own
client encoding), so every frontend forwards through the same member
with its own CLI's serialization. `query_visibility` and
`single_instance_disabled` stayed frontend-side: the chunk names
neither, and both are this launch's own client behavior.

**Dependencies:** Tasks 1–2 · **Files:** `src/app/surface_lifecycle*`,
`src/executor.rs` (effect mapping), `src/main.rs` · **Scope:** M

## Task 5: No-behavior-change walk and merge

**Description:** The contract is zero behavior change. Surface rows of the
manual checklist walk on the validation VM against an installed build
(snapshot discipline per development.md); then merge — first Phase 3
merge, so the branch is cut fresh from main and has no rebase partner.

**Acceptance criteria:**
- [x] The manual surface rows pass on COSMIC (show, drag, live edit,
      focus-loss, hide — the si-5 set)
- [ ] `just check` green on the merge result

**Verification:**
- [x] Manual: the checklist's surface rows (VM walk) — green 2026-11-13
      on the validation VM (the sanctioned evidence route; stock snapshot
      `pre-airlock` restored first, checkpoint `pre-shared-core-walk`
      taken, VM rewound to stock after; evidence host-side in
      `target/vm-evidence-sc5/`): show at 40 % pixel-exact (320 px of
      800, full width, top edge) with the block caret painted at the
      prompt in the first pre-keystroke frame (a solid 9×20 block,
      machine-measured); `sleep 97 &` survives a full CLI hide/show
      cycle with dropterm/bash/sleep pids unchanged; the grab-bar drag
      (QEMU monitor pointer events) live-applies mid-drag, settles at
      52 %, and the debounced write-back lands once in `config.ron`
      (`height_percent: 52`); a hand edit of `height_percent` while
      shown reshapes the surface in place the same second at the same
      pid (70→35 measured 280 px exactly), and while hidden applies on
      the next show (opens at 560 px = 70 %); focus-loss — a desktop
      click hides the shown surface with the instance surviving; CLI
      `toggle` round-trips exit 0 with symmetric shown/hidden diffs.
      Two harness facts for the next walk: `virsh send-key` sends its
      key list as one chord (a long chord drops letters at PS/2
      rollover — type one key per call; strace on the PTY master proved
      the app delivered every byte it received, so the one-key-per-call
      rule is an injection constraint, not an app fault), and pointer
      input rides QMP `input-send-event` with per-axis abs events,
      move/press/release as separate commands.
- [ ] Tests pass: `just check`

**Dependencies:** Tasks 1–4 · **Files:** the branch's diff · **Scope:** S

## Checkpoint: the core stands alone

- [x] `dropterm-core` builds with zero toolkit deps, pinned by test; the
      libcosmic binary and applet behave identically — the base every
      GPUI slice builds on (the VM walk's no-behavior-change row, plus
      the sc-1–4 moved-test suites green throughout)
