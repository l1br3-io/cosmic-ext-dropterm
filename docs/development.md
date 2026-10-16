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

That builds the debug profile and symlinks `~/.local/bin/cosmic-ext-dropterm-dev`
to `target/debug/cosmic-ext-dropterm`. Set the development shortcut's Command
to `cosmic-ext-dropterm-dev` once and leave it. The `-dev` suffix keeps an
installed release (from `sudo just install` or a release tarball) separate:
plain `cosmic-ext-dropterm` on PATH is always the installed copy, and the two
never shadow each other.

One instance runs at a time, whichever owns the bus name: DropTerm is
single-instance, so launching `cosmic-ext-dropterm` while the `-dev` instance
runs just forwards the action to it (and vice versa) — the running instance
parses every verb, whatever binary delivered it. `just dev-reload` kills the
running instance (either one) before starting the new debug build; that is
also how control passes back to the installed copy — exit or kill the dev
instance, and the next press of a shortcut bound to the plain name starts the
release. Config and sessions are shared (same app id), so settings carry
across both.

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

## Command map

Which recipe targets what, in one table:

| Recipe | Target |
| --- | --- |
| `just dev`, `dev-reload`, `dev-install`, `dev-watch` | Development — the debug build, launched as `cosmic-ext-dropterm-dev` |
| `just act …`, `toggle`, `show`, `hide`, `new-tab …` | Whichever instance is running — the verbs ride the `-dev` launcher and the running instance executes them |
| `just dev-applet` | Development — the panel applet's binary, desktop file, and icon under `~/.local` |
| `just build-all` + `sudo just install` | Installed — the release build of both packages into `/usr`; a release tarball's `usr/` tree lands in the same places, and `sudo just uninstall` removes either |
| `just applet-flatpak-build` / `-install` / `-remove` | The flatpaked applet, built from the release manifest into the user flatpak installation |
| `just check` | The gate — fmt + clippy + test across the workspace |
| `just vendor` / `build-vendored` | Release machinery — the offline dependency snapshot and the offline build proof |

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
