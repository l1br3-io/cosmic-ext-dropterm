# SPEC: shared-core — one libcosmic-free crate under both frontends

Status: not started · Phase 3 · lane: main (branch `port/shared-core`, merges
first in Phase 3)

## Objective

Extract the toolkit-neutral half of today's terminal crate into a new crate —
`dropterm-core` — that both frontends consume, so the GPUI frontend (every
later Phase 3 slice) builds on the same engine state, config store, contract,
and surface decision layer the libcosmic binary runs today. Zero behavior
change to the shipped binary and the applet; this is a move-and-split
refactor, nothing more.

## Preconditions

Phase 3 entry is met (plan record, roadmap): spike GO, the Kit ruled adopt,
`surface-intents`, `config-store`, and `control-transport` on main.

## Scope

Into `dropterm-core`:

- The config store and model: `src/config.rs`, `src/config/` (already
  libcosmic-free).
- Session: `src/session.rs` (free).
- Identity and contract: `APP_ID` and `dbus_state` move from the `dropterm`
  lib target; the applet's dependency re-points here (it already consumes
  `default-features = false`, so nothing else changes for it).
- Engine state: `src/tabs.rs`, `src/profile.rs`, and the terminal engine
  (`src/terminal/engine*`, suggest model and data) — minus the engine-side
  `cosmic-text` buffers, which have no role on GPUI's path (spike check 9
  set this boundary) and stay with the libcosmic widget; the render widget
  (`terminal/widget.rs`) stays frontend-side entirely.
- The surface vocabulary and decision layer: `intent.rs` (free) and
  `src/app/surface_lifecycle*`, whose `cosmic::app::Task` continuation
  becomes executor-neutral (the trait's `type Effect` already is).
- The geometry and keybind models, split from their libcosmic action-mapping
  halves (which stay in the frontend).
- Theme glue's pure decisions (`src/app/theme_glue.rs`): scheme resolution
  and window-background logic; the system-palette reader that feeds it
  stays frontend-side, passed in already packed.
- The startup transport's pure and zbus halves from `src/main.rs`: the
  bus-name ownership decision, second-launch forwarding, `forward_outcome`,
  and the platform-data dictionary — both frontends serve the same
  activation shape.

## Commands

    just check                                # the whole gate, all members
    just build-release && sudo just install   # the no-behavior-change walk

## Testing strategy

The existing tests pin the move: the decision-layer tests, the store's
temp-dir tests, the contract pin test, the config↔docs and dbus↔docs pin
tests all keep passing (module paths aside). One new pin: a test asserting
`dropterm-core` admits no libcosmic/iced dependency (grep over its manifest
and `cargo tree` output, so a leaked import fails `just check`). The manual
surface rows re-walked on the validation VM are the no-regression row.

## Boundaries

Never: change behavior, config formats, file locations, or the contract
surface; move the render widget, the slide, the libcosmic executor, or any
other libcosmic-typed half; create a `dropterm-platform` crate or any
GPUI-shaped abstraction (the standing surface-intents rule — the vocabulary
is the seam, and it already exists).

## Success criteria

- `dropterm-core` builds with zero libcosmic/iced dependencies, pinned by a
  test.
- The libcosmic binary and the applet build and behave identically: `just
  check` green; the surface rows of the manual checklist pass on the
  validation VM.
- The applet consumes the contract from `dropterm-core`.

## Size

L (file count; each move is mechanical).
