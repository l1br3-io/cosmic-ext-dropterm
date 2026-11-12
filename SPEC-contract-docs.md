# SPEC: contract-docs — the V1 contract for integrators

Status: not started · Phase 1 · lane C · branch `port/contract-docs` (main
checkout)

## Objective

An integrator-facing page for the V1 contract (bus name, object path,
interface, methods, properties, the CLI equivalents, the stability
promise), so a GNOME Shell extension, KDE widget, or swaybar module can be
written without reading Rust. The applet is the reference consumer.

## Dependencies

`control-transport` merged (the contract's ownership story is settled by
it).

## Scope

One docs page (exact name chosen at `/planning`), linked from README's
Documentation index. Every member `src/dbus_state.rs` serves appears on the
page. The stability promise states what the plan decided: the V1 D-Bus
contract becomes *the* API, D-Bus its Linux binding.

## Commands

    just docs-check

## Testing strategy

A doc-pinning test in the style of `every_config_key_is_documented`, if
cheap (`/planning` decides); otherwise `just docs-check` plus a review pass
against `src/dbus_state.rs`.

## Boundaries

- Always: derive the member list from `src/dbus_state.rs`, never from
  memory or from this spec.
- Never: document members that do not exist; promise stability beyond the
  plan's ruling; commit `SPEC*.md` or `tasks/`.

## Success criteria

- Every member `src/dbus_state.rs` serves appears on the page, pinned by a
  test in the style of `every_config_key_is_documented` if that is cheap.
- The page is linked from README's Documentation index.
- `just docs-check` green.

## Size

S to M.
