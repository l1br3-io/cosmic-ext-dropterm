# SPEC: control-transport — DropTerm owns its single instance

Status: not started · Phase 1 · lane B, after `config-store` · branch
`port/control-transport` (worktree `../ced2`)

## Objective

DropTerm owns its bus name, its second-launch forwarding, and the V1
contract over zbus, replacing libcosmic's `single-instance` feature, so the
contract and the CLI behave the same under any toolkit. The D-Bus code
becomes the Linux implementation of the `ControlTransport` seam — the
seam's trait arrives only with a second implementation (design rule 5), so
this package writes plain owned code, not a trait.

## Scope

In:

- Bus-name ownership at startup (today
  `cosmic::app::run_single_instance`, `src/main.rs:149`).
- Forwarding a second launch's verb and arguments to the running instance
  (today `ActivateAction` through `Application::dbus_activation`,
  `src/app.rs:2785-2791`, decoded by `src/app/remote.rs`).
- The `--is-visible` query (`src/main.rs:88`).
- Startup order in `src/main.rs`.
- Dropping the `single-instance` feature from `Cargo.toml`'s `terminal`
  feature list.

Out:

- Non-Linux transports (Phase 5).
- Any contract-surface change: the V1 contract is *the* API (owner ruling);
  observable behavior is unchanged apart from the names `app-identity`
  changed.

## Commands

    just check
    just build-release && sudo just install

## Files

`src/main.rs` (`:88`, `:149`, startup order), `src/app.rs:2785-2791`,
`src/app/remote.rs`, `Cargo.toml` (feature list). Shares `src/main.rs` with
`layer-shell-guard` — plan guidance: land the guard first, or rebase it
after. Shares `Cargo.toml`'s feature list with `config-store` (lane B
sequence).

## Constraints

- The contract's observable behavior (interface, methods, properties, exit
  codes) is unchanged apart from `app-identity`'s renames.
- The applet keeps working unmodified.

## Testing strategy

- A test pins the contract surface (interface name, members, object path).
- Manual on COSMIC (Phase 0 checks 12–13): with `single-instance` removed,
  a second launch forwards and exits; every verb in
  [the CLI reference](docs/cli.md) and `--is-visible` behave as before; the
  applet toggles and reports state.
- `just check` green.

## Boundaries

- Always: keep the applet unmodified — it is the contract's canary and
  `contract-docs`' reference consumer.
- Ask first: changing any D-Bus name or member beyond `app-identity`'s
  renames; new dependencies.
- Never: weaken exit-code semantics; commit `SPEC*.md` or `tasks/`.

## Success criteria

- With libcosmic's `single-instance` feature removed, a second launch
  forwards and exits; every verb in [the CLI reference](docs/cli.md) and
  `--is-visible` behave as before; the applet toggles and reports state.
- A test pins the contract surface (interface name, members, object path).
- `just check` green.

## Open questions

Keep the `org.freedesktop.DbusActivation` shape at the object path, or
forward through a V1 method? Proposal: forward through the V1 contract's
existing members (the surface the applet and `--is-visible` already speak)
so there is one code path and no activation-shaped special case — verify
the exact member set against `src/dbus_state.rs` during `/planning` before
committing to it. Decide at review or `/planning`.

## Size

M to L.
