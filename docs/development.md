# Development

How to work on DropTerm itself: the dev launcher loop and running without a shortcut. The manual test checklist lives in [testing](testing.md), the module map in [architecture](architecture.md).

The reasoning behind non-trivial design decisions — peer surveys, recommendations, rejected alternatives — lives in the research records; start from the [decision log](research/README.md).

## Development launcher

Don't point the shortcut at `cargo run`. COSMIC spawns a custom shortcut from
the compositor's working directory (not this repo), so cargo can't find
`Cargo.toml`, and every keypress would pay for a build check on top. Install a
launcher that always runs the current debug build instead:

```sh
just dev-install
```

That builds the debug profile and symlinks `~/.local/bin/cosmic-ext-dropterm`
to `target/debug/cosmic-ext-dropterm`. Set the shortcut's Command to the plain
`cosmic-ext-dropterm` once and leave it - `~/.local/bin` precedes `/usr/bin` on
the COSMIC session PATH, so it shadows a release copy installed with
`sudo just install`.

The edit-build-see loop is then one command:

```sh
just dev
```

`just dev` rebuilds, refreshes the launcher, kills any running instance and
starts the new binary. Killing first matters: a plain relaunch only toggles the
already-running process, which is still the code from before your edit.
`just dev-reload` does just the restart, and `just dev-install` just the build
plus launcher.

To skip the manual step, `just dev-watch` reruns that loop whenever a file
under `src/` (or `Cargo.toml`) changes:

```sh
just dev-watch
```

It needs [`cargo-watch`](https://github.com/watchexec/cargo-watch)
(`cargo install cargo-watch`). Each save rebuilds and restarts the overlay; a
failed build prints the compiler error, and the next save that compiles brings
it back.

## Running locally without a shortcut

No COSMIC shortcut configured yet? You can still open and dismiss the overlay from a local run:

- `just dev` builds, installs the launcher, and restart-launches the overlay; plain `cargo run` (or `just run`) just launches it.
- `Ctrl+Shift+Q` hides it.
- Clicking another window hides it too (`hide_on_focus_loss` defaults to `true`).
- Running the command a second time still toggles.


## Testing

Runtime behavior is only testable on real COSMIC hardware; the manual test
checklist every change answers to lives in [testing.md](testing.md). Headless
concerns stay in `#[cfg(test)]` module tests - `just test` runs them, and
`just check` (format, clippy, test) is the change Definition of Done.

## Architecture

The module map - the two crates, the module inventory, the load-bearing seams,
and where the reasoning lives - is in [architecture.md](architecture.md).
