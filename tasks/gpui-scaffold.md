# Chunk: gpui-scaffold — the GPUI frontend crate, pinned and patched

Spec: [SPEC-gpui-scaffold.md](../SPEC-gpui-scaffold.md) · Module id:
`gpui-scaffold` · Phase 3 · branch `port/gpui-scaffold` (main checkout,
after `shared-core` merges).

**Files touched:** a new `dropterm-gpui` crate (manifest, bin, app-shell
module), root `Cargo.toml` + `Cargo.lock` (member, pins, the carried
patch), a new `carried/gpui-component/` source copy,
[development.md](../docs/development.md) (carried-patches section),
[known-issues](../docs/known-issues.md) untouched. Reference seeds (read
only): `../dropterm-spike` `spikes/gpui-layer-shell/src/` and
`spikes/kit-consumer/` at the spike branch head.

## Planning rulings (made at /planning; veto at plan review)

- **The bin is `dropterm-gpui` for Phase 3** — a second binary beside
  `dropterm`, both claimable bus names kept distinct only by which one is
  running; `dev-identity` applies to it as to every member. Cutover does
  the rename.
- **The carried copy lives at `carried/gpui-component/`** under
  `[patch.crates-io]` with `package = "gpui-component"` — the directory
  name matches the ledger concept (the `vendor/` name is taken by the
  release recipe's output), and the patch is a path patch to the copied
  0.9.1 source with exactly the one-const change.
- **The manifest asserts itself**: a unit test pins the gpui-pre set at
  `=0.3.12` and zero `gpui-shell`/`gpui-kit` edges, so a careless bump
  fails `just check` instead of shipping.

## Task 1: The crate and the pin in the root workspace

**Description:** `dropterm-gpui` as a workspace member with the ruled pin
(`gpui-pre` `=0.3.12`, `gpui-pre-platform` `=0.3.12`, wayland + font-kit,
default features off) and check 14's one lockfile reconciliation
(`unicode-properties` `=0.1.3`). It boots a LayerShell window seeded from
the spike harness (carried as reference, rewritten as the crate's shell)
and exits cleanly.

**Acceptance criteria:**
- [x] One lockfile; `cargo tree` shows the 20-crate set at `=0.3.12`
- [x] A manifest pin test (the `=0.3.12` set, zero gpui-shell/gpui-kit
      edges) runs in `just check`
- [x] The window maps and exits clean on the sway rig and the validation
      VM

**Verification:**
- [x] Build: `cargo build -p dropterm-gpui` and `just check`
- [x] Manual: map + clean exit on rig and VM (logs held)

**Dependencies:** `shared-core` merged · **Files:** the new crate, root
`Cargo.toml`, `Cargo.lock` · **Scope:** M

## Task 2: The Kit and the carried copy

**Description:** `gpui-component` `=0.9.1` enters component-only; the
ruled one-const copy lands at `carried/gpui-component/` (the 0.9.1 source,
`SHADOW_SIZE` zeroed) under `[patch.crates-io]`. Verify exactly as the
comeback checklist will on every bump: the C2b kill check (Root +
programmatic Popover and Tooltip through the full 80 ms reshape cadence,
`on_open_change` silent) on both compositors, plus a flush-edges
screenshot (1280×320 negotiates 1280×320). The carried-patches section of
development.md ships here — the ledger's third entry, with the comeback
checklist and the upgrade ritual.

**Acceptance criteria:**
- [x] Kill check green on COSMIC and sway; flush edges evidenced
- [x] development.md's carried-patches section exists and names all three
      entries (vte, alacritty_terminal, gpui-component)
- [x] `just check` green (docs-check included)

**Verification:**
- [x] Manual: the kill-check walk on rig + VM (screenshots held)
- [x] Tests pass: `just check`

**Dependencies:** Task 1 · **Files:** `carried/gpui-component/`, root
`Cargo.toml`, development.md · **Scope:** M

## Task 3: Startup dispatch and the guard

**Description:** The startup probe (core's transport module hosts the
verdict function; the probe itself re-implemented on the GPUI connection)
decides layer-shell vs refuse-and-exit with the same verdicts as the
libcosmic binary — check 11's shape. The no-layer-shell branch keeps the
guard's exit-code and message contract.

**Acceptance criteria:**
- [x] Layer-shell session: boots; non-layer-shell (weston-headless
      evidence route): the guard's message + exit 1, no probe reason

**Verification:**
- [x] Manual: the lg-2/lg-3 evidence routes replayed for this binary

**Dependencies:** Task 1 · **Files:** the crate's startup module ·
**Scope:** S

## Task 4: The contract beside the loop; CLI parity

**Description:** V1 + `org.freedesktop.DBusActivation` served over zbus on
core's patterns — the `Connection` held for the process lifetime (the
spike's dropped-`_conn` lesson) with a `NameHasOwner` self-check — beside
GPUI's render loop. The CLI surface matches the libcosmic binary
(`toggle`/`show`/`hide`/`--is-visible`, second-launch forwarding through
core's transport, stale-owner refusal exit codes). `SetVisible` flips the
band live — the spike's check-10 shape, in-tree.

**Acceptance criteria:**
- [ ] Contract round-trip on both compositors: name owned, `IsVisible`
      read, `SetVisible` flipping the render live and back
- [ ] A second launch forwards and exits 0; the stale-owner case refuses
      exit 1
- [ ] `--is-visible` answers true/false with matching exit codes

**Verification:**
- [ ] Manual: the round-trip walk on rig + VM (logs held)
- [ ] Tests pass: `just check`

**Dependencies:** Tasks 1–3 · **Files:** the crate's dbus/startup modules ·
**Scope:** M

## Checkpoint: the frontend exists

- [ ] `dropterm-gpui` builds on the ruled pin in the root workspace, maps
      on both compositors, owns the bus name, answers the contract — and
      the carried patch is in place with its docs
