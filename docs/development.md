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
to the dev image `target/debug/cosmic-ext-dropterm-dev-image` — a copy of the
dev-identity build that only the `just dev*` recipes ever rewrite. Set the
development shortcut's Command to `cosmic-ext-dropterm-dev` once and leave it.
The `-dev` suffix keeps an installed release (from `sudo just install` or a
release tarball) separate: plain `cosmic-ext-dropterm` on PATH is always the
installed copy, and the two never shadow each other.

The dev build also carries its own runtime identity —
`io.github.l1br3_io.CosmicExtDropterm.Dev`, selected by the `dev-identity`
cargo feature the dev recipes build with: its own single-instance slot, bus
name, config file, state directory, and wayland namespace. A dev instance and
an installed release run at the same time without seeing each other —
launching one while the other runs never forwards to it, and each answers
only its own launcher and keybind. `just dev` (via `dev-reload`) kills only
the dev instance before starting the new build; an installed release keeps
running untouched, and `just act` / `just toggle` always drive the dev
instance.

The identity reaches the desktop integration too: `just dev-install` also
writes `~/.local/share/applications/io.github.l1br3_io.CosmicExtDropterm.Dev.desktop`
(hidden from the app grid, like the release's own entry), so the window
switcher labels the dev build's anchor window "DropTerm (Dev)" instead of
"unknown" — and a hidden-tab bell notification from the dev build says
"DropTerm (Dev)" as its app name, so with both instances running you can
always tell which one rang.

The separation reaches the files: the dev build reads and writes
`~/.config/io.github.l1br3_io.CosmicExtDropterm.Dev.ron` and
`~/.local/state/io.github.l1br3_io.CosmicExtDropterm.Dev/`, so config-schema
churn on main cannot migrate the daily driver's settings. To develop against
your real settings, copy them over once:

```sh
cp ~/.config/io.github.l1br3_io.CosmicExtDropterm.ron \
   ~/.config/io.github.l1br3_io.CosmicExtDropterm.Dev.ron
```

Deliberately a copy, not a share — dev experiments never write back to the
real config.

One caveat: the launcher runs the dev image, whose identity only the
`just dev*` recipes can change. A plain `cargo build` (or `cargo test`, or
`just check`) refreshes the *shared* debug bin `target/debug/cosmic-ext-dropterm`
without the `dev-identity` feature — the launcher ignores it. Running that
shared bin directly (`cargo run`, `just run`) is still a release-identity
launch that forwards to (and tugs at) the installed copy; the dev loop always
goes through the launcher or the `just dev*` recipes.

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
| `just dev`, `dev-reload`, `dev-install`, `dev-watch` | Development — the debug build under the `.Dev` identity, launched as `cosmic-ext-dropterm-dev`, plus its (app-grid-hidden) desktop file |
| `just act …`, `toggle`, `show`, `hide`, `new-tab …` | The dev instance — the verbs ride the `-dev` launcher and the dev instance executes them |
| `just dev-applet` | Development — a second panel entry ("DropTerm (Dev)") under `~/.local`, tracking the dev terminal only |
| `just dev-uninstall` | Development — removes the `-dev` launcher, the dev desktop files, and the dev applet's footprint |
| `just deps` | Prerequisites — checks the C toolchain, `pkg-config`, the system libraries, and cargo, installing what is missing via the distro's package manager (apt/dnf/pacman/zypper) or rustup; the build and install recipes run it first, and it is a silent no-op when everything is present |
| `just build-all` + `sudo just install` | Installed — the release build of both packages into `/usr`; a release tarball's `usr/` tree lands in the same places, and `sudo just uninstall` removes either |
| `just applet-flatpak-build` / `-install` / `-remove` | The flatpaked applet, built from the release manifest into the user flatpak installation |
| `just check` | The gate — fmt + clippy + test across the workspace, each package also once under `dev-identity` |
| `just vendor` / `build-vendored` | Release machinery — the offline dependency snapshot and the offline build proof |

## Running locally without a shortcut

No COSMIC shortcut configured yet? You can still open and dismiss the overlay from a local run:

- `just dev` builds, installs the launcher, and restart-launches the overlay under the dev identity; plain `cargo run` (or `just run`) launches the release identity — it forwards to an installed copy's instance if one is running.
- `Ctrl+Shift+Q` hides it.
- Clicking another window hides it too (`hide_on_focus_loss` defaults to `true`).
- Running the command a second time still toggles.


## Testing

Runtime behavior is only testable on real COSMIC hardware; the manual test
checklist every change answers to lives in [testing.md](testing.md). Headless
concerns stay in `#[cfg(test)]` module tests - `just test` runs them, and
`just check` (format, clippy, test) is the change Definition of Done.

## Releasing

A release is two recipes and a push, split because one edit is necessarily
post-publish: the applet's flatpak manifest pins the *published* source
archive (URL + sha256), and neither exists until CI has published the
release.

1. **Curate the release content** (uncommitted or committed, the recipe
   takes either):
   - [releases.md](releases.md) - a `## vX.Y.Z (date)` section for today,
     house style; the "Next (unreleased)" pointer, if any, is consumed by it
   - both `res/*.metainfo.xml` - a `<release version="X.Y.Z" date="…">`
     entry each
   - [README.md](../README.md) - the Status line and every
     `releases/download/v…` install URL re-pointed at the new version
   - [testing.md](testing.md) - the release-walk rows re-versioned
   - [ROADMAP.md](../ROADMAP.md) - fixed bugs retired from the bugs list
2. **`just release X.Y.Z`** - verifies all of the above exists for today's
   date, bumps both crates' versions, refreshes `Cargo.lock`, runs
   `just check` plus the appstream validation CI enforces, commits
   everything as one release-hygiene commit, and tags `vX.Y.Z`. It never
   pushes - the push is the publish.
3. **Publish**: `git push origin main && git push origin vX.Y.Z`.
   [release.yml](../.github/workflows/release.yml) builds the per-arch
   tarballs and `vendor.tar`, proves the vendored offline build, guards
   tag against `Cargo.toml`, and publishes the GitHub release with
   `sha256sums`.
4. **`just release-flatpak-pin X.Y.Z`** - once the release page exists:
   downloads the source archive, records its hash, and commits the applet
   manifest's re-pin. Push that commit and the release is whole.

The version is semver: additive features take the minor, fixes the patch.
A failed gate mid-recipe leaves the version bumps uncommitted in the tree;
fix and re-run.

## Architecture

The module map - the two crates, the module inventory, the load-bearing seams,
and where the reasoning lives - is in [architecture.md](architecture.md).
