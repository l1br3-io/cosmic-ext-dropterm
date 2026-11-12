# SPEC: config-store — plain XDG configuration

Status: not started · Phase 1 · lane B · branch `port/config-store`
(worktree `../ced2`, from day one)

## Objective

Settings persist as plain ron owned by DropTerm under `$XDG_CONFIG_HOME`,
following the `src/session.rs` pattern, and `cosmic-config` leaves the
terminal crate. Configuration behaves the same on any desktop and under any
toolkit, and the store no longer names a COSMIC namespace.

Merge order: starts day one beside `app-identity` (its paths derive from
`APP_ID`, so it follows the rename) and ships in the same release, with
no release cut between the two merges (no break note — owner ruling
2026-11-12: pre-distribution, no users).

## Scope

In:

- Load at startup; atomic writes and quarantine of an unreadable file, as
  `src/session.rs` does.
- The three writers (`PlannedWrite`, `ProfileWrite`, `KeybindsWrite` —
  `src/settings.rs:651,2096,2120`).
- Live reload of external edits, replacing
  `cosmic_config::config_subscription` (`src/app.rs:2320`) while keeping the
  echo-driven apply that `ApplyTiming` relies on.
- The `completions.toml` location (`src/app.rs:1993-2008`).
- The 40 test fixtures (`src/config/tests.rs`, the `with_custom_path` uses).
- The paths in [configuration](docs/configuration.md).

Out:

- Migrating the cosmic-config files (greenfield ruling, 2026-11-09).
- Any change to config semantics beyond the store: keys, defaults, and the
  serialization shape stay as they are.

## Commands

    just check
    just build-release && sudo just install    # for the manual rows

## Files

`src/config.rs` (the derive at `:10,186` leaves), `src/app.rs:2068,2320`
(handle and watcher), `src/settings.rs:651,2096,2120` (the writers),
`src/app.rs:1993-2008` (the completions path), `src/config/tests.rs`
(fixtures), `docs/configuration.md`, `Cargo.toml` (the `cosmic-config`
dependency; possibly the `terminal` feature list's `dbus-config` — Open
questions).

Shares `src/app.rs` with `surface-intents` (different regions: config
handle/watcher/writers vs surface handlers; whichever merges second
rebases). Shares `Cargo.toml`'s `terminal` feature list with
`control-transport` (lane B sequence: `config-store` first).

## Code style

The store pattern to lift already exists — do not paraphrase it from this
spec, read it:

- `src/session.rs:422` `save_atomic`: write a `.tmp` sibling, fsync, rename
  over the target (the module doc at `:8` names the contract), with parent
  directories created on demand.
- `src/session.rs:397` `quarantine`: rename an unreadable file aside and
  fall back to defaults; a failed quarantine rename is logged and survived.
- `src/session.rs:368` `load`.

Unit tests inline in `#[cfg(test)] mod tests`, temp-dir backed, per the
repo's Rust notes in AGENTS.md.

## Testing strategy

- Headless, against a temporary directory: round-trip, corrupt-file
  quarantine, and default-on-missing tests.
- External edit applies live (a headless test where the watcher allows it,
  a manual checklist row otherwise), and the drawer's own writes still
  apply exactly once.
- `rg cosmic_config src/` matches nothing; `just check` green; the
  configuration doc names the new path (`docs-check`).

## Boundaries

- Always: keep `every_config_key_is_documented` green; tests write only to
  temp dirs, never the real XDG paths.
- Ask first: any new direct dependency — a file-watcher crate (e.g.
  `notify`) is the known candidate; dropping libcosmic's `dbus-config`
  feature (also a behavior question — Open questions).
- Never: read or migrate the old cosmic-config files; commit `SPEC*.md` or
  `tasks/`.

## Success criteria

- Round-trip, corrupt-file quarantine, and default-on-missing tests pass
  against a temporary directory.
- An external edit applies live (a headless test where the watcher allows, a
  manual row otherwise), and the drawer's own writes still apply once.
- `cosmic_config` appears nowhere under `src/`; `just check` green; the
  configuration doc names the new path.

## Open questions (settle at review or `/planning`)

1. One file or one per section? cosmic-config stores one file per key, so a
   drawer write and an external edit never collided; a single file needs a
   merge rule (last-writer-wins vs section merge). Decide with the
   drawer-write-races-external-edit case in mind.
2. The watcher: a crate (ask-first dependency) vs re-reading at show (no
   dependency; external edits appear at the next show instead of live).
   The choice changes which testing-strategy row is headless vs manual.
3. The exact path and file name. Proposal:
   `$XDG_CONFIG_HOME/dropterm/config.ron` — follows the `session.rs`
   pattern and the `app-identity` name.
4. Whether libcosmic's `dbus-config` feature goes too: it also routes
   libcosmic's own config and theme watching through
   cosmic-settings-daemon, so dropping it changes how the COSMIC theme
   follow is watched — verify on COSMIC hardware before deciding.

## Size

L.
