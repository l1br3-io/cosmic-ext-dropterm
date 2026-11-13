# SPEC: gpui-scaffold — the GPUI frontend crate, pinned and patched

Status: not started · Phase 3 · lane: main (branch `port/gpui-scaffold`, after
`shared-core` merges)

## Objective

The GPUI frontend exists as a second binary beside the libcosmic one —
`dropterm-gpui`, a new root-workspace member, shipping nothing (the release
boundary holds until cutover). It lands the ruled pin, the Kit, and the ruled
carried patch, then boots the app shell: a LayerShell window that serves the
V1 contract and single-instance activation over zbus beside GPUI's executor —
the spike's check-10 shape, grown into a tree. End state: `dropterm-gpui`
owns the bus name on either compositor and a `SetVisible` flip (CLI or
applet) toggles its band live.

## Preconditions

`shared-core` merged (the crate consumes core's contract and transport);
the rulings recorded as plan open questions items 7–8.

## The pin (item 8, ruled 2026-11-13)

Published crates, never a zed git pin: `gpui = { package = "gpui-pre",
version = "=0.3.12", default-features = false, features = ["wayland",
"font-kit"] }` and `gpui_platform = { package = "gpui-pre-platform", version
= "=0.3.12" }` with `wayland` — exactly the set the Kit consumes, the whole
20-crate family resolving at `=0.3.12`. Check 14's one reconciliation rides
in the root lockfile (`unicode-properties` `=0.1.3`); the frontend shares
the workspace's single lockfile (ABSORBS — no separate workspace unless
something regresses).

## The Kit and the carried patch (item 7, ruled 2026-11-13)

`gpui-component` `=0.9.1`, component-only: zero `gpui-shell` and
`gpui-kit` edges (C3), windows stay embedder-owned. The Root auto-border is
carried as a private source copy under `[patch.crates-io]` at
`carried/gpui-component/` with the one-const change (`SHADOW_SIZE` zeroed)
— never a public fork; the upstream opt-out ask stays the owner's outward
action, and [kit-root-border](docs/research/kit-root-border.md) is the
durable record. The copy's verification, at creation and on every bump, is
the comeback checklist's: the C2b kill check (Root + programmatically
opened Popover and Tooltip through the full 80 ms reshape cadence, closing
never) plus one flush-edges screenshot (a 1280×320 drawer negotiates
1280×320, not 1320×360). The carried-patches section of
[development](docs/development.md) ships here — the docs-to-code gates
forbid it earlier — as the ledger's third entry.

## Scope

- The crate: manifest with the pins above, `dropterm-gpui` bin, `dev-identity`
  feature riding the same pattern as the other members.
- The app shell: LayerShell window booting and mapping on COSMIC and sway,
  seeded from the spike harness (`spikes/gpui-layer-shell` and
  `spikes/kit-consumer` on the spike branch, never merged as code — carried
  as reference); clean exit.
- Startup dispatch: the layer-shell guard's probe decides layer-shell vs
  refuse, same verdicts as the libcosmic binary (check 11's shape).
- The contract beside the render loop: V1 + `org.freedesktop.DBusActivation`
  served over zbus on GPUI's executor-coexistence pattern (the Connection
  held for the process lifetime — the spike's dropped-`_conn` lesson — with
  a `NameHasOwner` self-check); CLI parity (`toggle`/`show`/`hide`/
  `--is-visible`, second-launch forwarding through core's transport).

## Commands

    cargo run -p dropterm-gpui        # sway-headless rig or the validation VM
    just check                        # the gate, all members

## Testing strategy

Headless: the transport and guard tests come from core (already pinned);
manifest assertions (the `=0.3.12` set, zero `gpui-shell`/`gpui-kit` edges)
are unit-testable. Compositor rows follow the spike's evidence routes: the
sway-headless rig container and the validation VM with snapshot discipline.

## Boundaries

Never: enable GPUI's `x11`/macOS features (wayland-only, the spike's scope;
Phase 4–5 own those); make the GPUI binary the default or ship it (cutover's
call); patch anything in the Kit beyond the one const; write the
carried-patches doc before the copy exists.

## Success criteria

- `dropterm-gpui` builds in the root workspace: one lockfile, the exact pin
  set, `just check` green.
- Maps and renders on COSMIC and sway with a clean exit; contract round-trip
  (name owned, `IsVisible` read, `SetVisible` flipping the band live and
  back) on both compositors; second-launch forwarding works; the stale-owner
  refusal keeps its exit code.
- The carried copy verifies: kill check green on both compositors, flush
  edges evidenced, development.md's carried-patches section in place.

## Size

M–L.
