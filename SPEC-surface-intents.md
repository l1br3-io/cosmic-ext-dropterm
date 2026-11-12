# SPEC: surface-intents — a toolkit-neutral surface vocabulary

Status: not started · Phase 1 · lane A · branch `port/surface-intents`
(worktree `../ced`, after `app-identity` merges)

## Objective

The surface state machine and the reshape paths emit toolkit-neutral
intents, and a libcosmic executor turns them into today's layer-shell
commands. The whole lifecycle becomes headless-testable, and the GPUI port
becomes a second executor over the same vocabulary. This is also the
package that makes a spike REDESIGN survivable: the plan's criterion says
"the state machine behind `surface-intents` survives; only its executors
change."

## Scope

In:

- The terminal surface's show, measure, shape, drag, live edit, slide-out,
  reversal, hide, and focus paths: `src/app/surface_lifecycle.rs`; the
  `SurfaceOpened`, `ConfigChanged`-reshape, drag, and slide-end handlers in
  `src/app.rs`; `src/geometry.rs`; `src/surface.rs`.
- The intent trait and a recording executor — the recording executor is the
  second implementation that justifies the trait (design rule 5).

Out:

- The clipboard anchor, and any libcosmic clipboard-binding workaround that
  spike check 8 may later delete: private to the libcosmic executor.
- The `dropterm-platform` crate: it lands with the GPUI executor (design
  rule 5). Until then the trait lives inside the binary (rule 3: traits
  inside the binary, contracts outside it).

## Constraints (design rules, decided in the plan)

1. Intents are declarative desired-state: the desired surface (output
   choice, geometry, layer, keyboard mode, exclusive zone, visibility). Each
   executor reconciles desired against current and chooses to mutate or
   recreate. A vocabulary mirroring libcosmic's requests (`set_anchor`,
   `set_margin`, `set_layer`) would be unimplementable on GPUI, where those
   are create-only.
2. Intents carry a DropTerm surface handle, and the executor maps it to the
   toolkit's window id, so iced's `window::Id` leaves the decision layer.
3. The libcosmic executor reproduces today's request order exactly,
   including the anchor heal chained ahead of the measuring probe at show
   and the stale-echo gate before a reshape
   (`request_geometry_height`).

## Commands

    just check
    just build-release && sudo just install    # the manual surface rows run against this

## Files

`src/app/surface_lifecycle.rs` (with its tests, `:793-1122` today),
`src/app.rs` (the surface handlers), `src/geometry.rs`, `src/surface.rs`,
plus the new executor module and tests. Shares `src/app.rs` with
`config-store` — different regions, second merger rebases.

## Testing strategy

- Headless: a test drives show, first configure, drag, live edit, hide, a
  mid-slide reversal, and a focus-loss hide through the intent layer with a
  recording executor, and asserts the intent sequence. The ~25 existing
  decision-layer tests move onto the intent types (`window::Id`-free).
- Translation pin: a test pins the libcosmic executor's translation of each
  intent into today's command sequence, request order included.
- Manual: the surface rows of the manual checklist in
  [testing](docs/testing.md) pass on COSMIC — no behavior change is the
  contract.
- `just check` green.

## Boundaries

- Always: extend the recording executor rather than mocking iced; keep
  request-order changes out of the executor unless a test deliberately pins
  a new order.
- Ask first: any new dependency (none expected); touching the clipboard
  anchor's lifecycle (out of scope — spike check 8's subject).
- Never: introduce the `dropterm-platform` crate or any GPUI-shaped
  abstraction before the spike rules (Phase 3 assigns those ids); commit
  `SPEC*.md` or `tasks/`.

## Success criteria

- A headless test drives show, first configure, drag, live edit, hide, a
  mid-slide reversal, and a focus-loss hide through the intent layer with a
  recording executor, and asserts the intent sequence.
- A test pins the libcosmic executor's translation of each intent into
  today's command sequence.
- No behavior change: the surface rows of the manual checklist in
  [testing](docs/testing.md) pass on COSMIC.
- `just check` green.

## Open questions

None blocking — the constraints above are the decided design. The intent
vocabulary's exact shape (one enum vs a struct per intent) is a
`/planning`-level decision made inside those constraints.

## Size

L (four source files plus tests).
