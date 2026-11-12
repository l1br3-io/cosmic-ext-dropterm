# SPEC: layer-shell-guard — fail loudly without layer-shell

Status: not started · Phase 1 · lane C · branch `port/layer-shell-guard`
(main checkout, or `../ced` between lane A's packages)

## Objective

Where the compositor offers no `zwlr_layer_shell_v1` (GNOME, a sandbox, a
session without Wayland), DropTerm says so and exits non-zero. Today it
silently never surfaces while `--is-visible` reports true — the 2026-11-08
sandbox walk recorded in the packaging record, logged there as a follow-up.
This is the first slice of backend detection (design rule 3, spike check
11): it reports "unsupported here" where a later build dispatches.

## Scope

In: a startup probe of the Wayland connection and its registry for the
layer-shell global; an error naming the missing protocol and linking the
docs; a truthful `--is-visible` when no surface can exist; a
[known issues](docs/known-issues.md) row.

Out: any fallback surface (a `PopUp` path here is Phase 4's backend, not
this package's); any dispatch beyond the verdict.

## Commands

    just check
    just build-release && sudo just install

## Files

`src/main.rs` (the probe at startup, before surface creation),
[known issues](docs/known-issues.md) (a row), plus wherever
`--is-visible`'s truth needs the verdict plumbed. Shares `src/main.rs` with
`control-transport` — land the guard first, or rebase it after (plan
guidance).

## Constraints

- COSMIC and sway are unchanged (the global exists there).
- The verdict is a pure decision function (global list in, verdict out) so
  it is unit-testable headless.

## Testing strategy

- Unit test over the decision function (global list in, verdict out).
- Manual: under GNOME, or a nested compositor without layer-shell, the
  binary exits non-zero with the message; COSMIC and sway are unchanged.
- `just check` green.

## Boundaries

- Always: the error names the missing protocol (`zwlr_layer_shell_v1`) and
  links the docs.
- Ask first: a direct `wayland-client` dependency for the registry
  roundtrip (transitive today; making it direct is the ask) — the
  alternative is probing through an existing dependency's connection.
- Never: add a fallback surface; commit `SPEC*.md` or `tasks/`.

## Success criteria

- A unit test over the decision function (global list in, verdict out).
- Manual: under GNOME, or a nested compositor without layer-shell, the
  binary exits non-zero with the message; COSMIC and sway are unchanged.
- `just check` green.

## Open questions

The probe's connection (the ask-first item above). Nothing else blocking.

## Size

S to M.
