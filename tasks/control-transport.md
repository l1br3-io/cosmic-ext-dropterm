# Chunk: control-transport — DropTerm owns its single instance

Spec: [SPEC-control-transport.md](../SPEC-control-transport.md) · Module
id: `control-transport` · Phase 1, lane B, after `config-store` · branch
`port/control-transport` (worktree `../ced2`).

**Files touched:** `src/main.rs` (`:88` the `--is-visible` query,
`:149` `run_single_instance`, startup order), `src/app.rs:2785-2791`
(`dbus_activation`), `src/app/remote.rs` (the decode path), `Cargo.toml`
(the `terminal` feature list). Shares `src/main.rs` with
`layer-shell-guard` — plan guidance: land the small guard first, or
rebase it after.

## Planning ruling (verified at /planning; the spec asked for it)

The spec's open question proposed forwarding "through the V1 contract's
existing members." Verified against `src/dbus_state.rs`: **the V1
interface has no methods** — it serves exactly two read-only properties,
`Visible` and `TabCount`. There is no member to forward through, and
adding one is a contract-surface change the spec rules out (the V1
contract is *the* API; observable behavior unchanged apart from
`app-identity`'s renames). **Ruling: keep the
`org.freedesktop.DbusActivation` shape** — DropTerm serves it at the
object path over zbus itself, replacing libcosmic's serving, one code
path, no activation-shaped special case beyond what exists today. Read
libcosmic's exact `ActivateAction` member signature from the vendored
source during Task 2 and pin it by test.

## Task 1: Bus-name ownership at startup

**Description:** Replace `cosmic::app::run_single_instance`
(`src/main.rs:149`): the app connects to the session bus over zbus,
attempts ownership of the bus name, and on ownership serves the V1
interface and the activation interface at the object path beside the
normal iced run. Startup order in `src/main.rs` is this package's to
settle: `--is-visible` still exits before any ownership attempt, TERM
setup and env_logger stay put.

**Acceptance criteria:**
- [x] First launch owns the bus name, serves V1 + activation at the
      object path, and runs the app *(e16a976: ownership taken by the
      app's control-bus subscription, DoNotQueue; interim note+exit-1
      on an owned name until T2's forward lands)*
- [x] `--is-visible` exits before any ownership attempt, as today
      *(structural: the query path returns before the bus-name check;
      pinned by review, the manual row carries it)*

**Verification:**
- [x] Tests pass: `just check`
- [ ] Manual: a first launch answers `--is-visible` over the served
      name

**Dependencies:** `config-store` merged (lane B sequence);
`app-identity` merged · **Files:** `src/main.rs`, `src/dbus_state.rs`
(serving wiring) · **Scope:** M

## Task 2: Second-launch forwarding

**Description:** When the bus name is already owned, the second launch
forwards its verb and arguments through the served activation member
(today `ActivateAction` via `Application::dbus_activation`,
`src/app.rs:2785-2791`) and exits; the receiving instance decodes
through `parse_forwarded_action` (`src/app/remote.rs`) exactly as
today. Exit codes and the fire-and-forget semantics are unchanged.

**Acceptance criteria:**
- [x] A second launch forwards every verb and exits; the running
      instance applies it *(c7e2a4f: forward via activate_action with
      the same member/args/platform-data libcosmic sent; Ok exits 0,
      failure against an owned name notes + exits 1 — tighter than
      libcosmic's duplicate-instance fall-through; the applies-it half
      rides the Phase 0 manual row)*
- [x] The forwarded-actions round-trip test still passes unchanged
      (the decode path is untouched)

**Verification:**
- [x] Tests pass: `just check`
- [ ] Manual: Phase 0 checks 12–13 on COSMIC (second-launch handoff
      holds)

**Dependencies:** Task 1 · **Files:** `src/main.rs`,
`src/app.rs:2785-2791`, `src/app/remote.rs` as needed · **Scope:** M

## Task 3: The contract-surface pin test

**Description:** A test pins the served contract surface: interface
name, member set, and object path — the V1 interface and the activation
member signature read from libcosmic's vendored source. This is the
guard that the ownership change changed nothing observable.

**Acceptance criteria:**
- [x] The pin test enumerates the served members and paths and passes
      *(616cba1; mutation-checked — a wrong signature fails it)*

**Verification:**
- [x] Tests pass: `just check`

**Dependencies:** Task 2 · **Files:** `src/dbus_state.rs` (tests) ·
**Scope:** S

## Task 4: Drop the feature; applet canary; merge

**Description:** Remove `libcosmic/single-instance` from `Cargo.toml`'s
`terminal` feature list. The applet stays unmodified — it is the
contract's canary: it toggles and reports state over the owned name.
Manual verification, then merge (rebase over `layer-shell-guard` if the
guard landed first on `src/main.rs`).

**Acceptance criteria:**
- [ ] With `single-instance` removed, every verb in [the CLI
      reference](../docs/cli.md) and `--is-visible` behave as before,
      and the applet toggles and reports state
- [ ] `just check` green on the merge result

**Verification:**
- [ ] Manual on COSMIC: the CLI verbs + the applet against an installed
      build

**Dependencies:** Task 3 · **Files:** `Cargo.toml`, `Cargo.lock`
(regenerates) · **Scope:** S
