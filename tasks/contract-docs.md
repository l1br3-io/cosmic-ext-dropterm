# Chunk: contract-docs — the V1 contract for integrators

Spec: [SPEC-contract-docs.md](../SPEC-contract-docs.md) · Module id:
`contract-docs` · Phase 1, lane C · branch `port/contract-docs` (main
checkout).

**Files touched:** `docs/dbus.md` (new), `README.md` (Documentation
index), the pinning test in `src/dbus_state.rs`'s test module.

## Planning rulings (made at /planning; veto at plan review)

- **Page: `docs/dbus.md`** — the integrator-facing V1 contract page.
- **The pinning test is cheap: write it.** The existing `p2p_pair`
  harness (`src/dbus_state.rs` tests) already serves V1 headless; the
  test serves the interface, calls `Introspect`, parses the member
  list, and asserts every member appears on the page — same shape as
  `every_config_key_is_documented`, no session bus needed.

## Task 1: The contract page, derived from the source

**Description:** Write `docs/dbus.md`: bus name, object path, interface,
methods and properties (including the activation member
`control-transport` now serves, if it has merged — derive from the
source, never from memory or from this chunk), the CLI equivalents
([the CLI reference](../docs/cli.md) links both ways), and the
stability promise exactly as the plan ruled it: the V1 D-Bus contract
becomes *the* API, D-Bus its Linux binding. The applet is named the
reference consumer ([applet](../docs/applet.md) links in).

**Acceptance criteria:**
- [x] Every member `src/dbus_state.rs` serves appears on the page
- [x] No member is documented that does not exist; the stability
      promise goes no further than the plan's ruling

**Verification:**
- [x] Manual: the page reviewed against `src/dbus_state.rs` line by
      line *(agent-run on port/contract-docs: every claim traced to
      the source — the address and dev twin, the V1 property pair
      with the change-signal rule, the three activation members with
      signatures and the fire-and-forget/queue-64 semantics, the
      CLI-verb encoding against `forward_words` and the Display
      names; two draft errors caught by the review itself — a bare
      `dropterm` sends `ActivateAction("toggle", [])` not `Activate`
      (`src/main.rs`: the CLI never fires the plain member), and the
      applet's click likewise sends `ActivateAction("toggle", [])`
      (`applet/src/dbus.rs`))*

**Dependencies:** `control-transport` merged (the ownership story is
settled by it) · **Files:** `docs/dbus.md` · **Scope:** S

## Task 2: The member pinning test

**Description:** `every_dbus_member_is_documented`: serve V1 over the
existing p2p harness, `Introspect` the object path, parse the interface
members, and assert each appears in `docs/dbus.md`. A member added to
the interface without a docs row fails the test.

**Acceptance criteria:**
- [x] The test passes and enumerates its expectation from the served
      introspection, not a hand-maintained list

**Verification:**
- [x] Tests pass: `just check` *(mutation-checked ct-3 style:
      de-backticking `TabCount` in the page fails the test with the
      member named, the restored page passes; `just check` green on
      the result)*

**Dependencies:** Task 1 · **Files:** `src/dbus_state.rs` (test module)
· **Scope:** S

## Task 3: Index link and merge

**Description:** Link the page from README's Documentation index;
`just docs-check` green; merge.

**Acceptance criteria:**
- [x] Linked from README's Documentation index; `just docs-check` green

**Verification:**
- [x] Tests pass: `just check` (docs-check included) *(merged 5bf146b,
      local --no-ff, after the `/ship` fan-out review's findings
      landed on the branch as 3e4783c — review verdict GO, no
      Critical findings; `just check` green on the merge result)*

**Dependencies:** Task 2 · **Files:** `README.md` · **Scope:** XS

## Recorded follow-ups (the ship review's optional notes — not taken)

Wait for a real need; none blocks this module's close:

- **Signature-column pinning** — the docs pin asserts member *names*
  on the page; the tables' signature cells are held only by the wire
  pin (`served_contract_surface_is_pinned`) and review. Extending
  `every_dbus_member_is_documented` to assert each row's signature
  would close the gap (the helpers already extract the types).
- **Table-row anchoring** — a member name appearing backticked in
  prose satisfies the pin, so deleting a table row can pass while
  prose keeps the name (same looseness as
  `every_config_key_is_documented`, which has a compensating
  row-level test).
- **The `--cwd` non-UTF-8 encode-drop** — the wire table says "only
  the flags actually given"; a non-UTF-8 `--cwd` path is silently
  dropped at the CLI edge (`src/app/remote.rs`), so the wire can
  carry fewer flags than were given. CLI-internal; a clause on
  either page would close it.
- **Reverse-direction pin** — nothing in the repo fails when a member
  is *removed* from an interface and the page keeps its row
  (forward-only is this test's declared scope; Task 1's
  "no member documented that does not exist" rests on the recorded
  line-by-line review). A page-members ⊆ served-members assert would
  close it if a member is ever removed.
