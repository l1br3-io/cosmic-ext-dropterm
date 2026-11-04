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

## The COSMIC validation VM

The applet rows at the bottom of [testing.md](testing.md) need a live
COSMIC session; the validation VM is where they walk when no spare
hardware is at hand. The guest is a Pop!_OS 24.04 COSMIC image under
virt-manager/QEMU — `/dev/kvm`, virtio-gpu with virgl: cosmic-comp
renders GLES and libcosmic falls back to GL, so no guest Vulkan is
needed (cosmic-epoch#3831/#3844). One-time guest setup: install
`qemu-guest-agent` (the harness's only channel in — no SSH), auto-login
via greetd so reboots return to the desktop unattended, then snapshot
the stock state and name it; every walk rewinds to it first.

The harness is `scripts/vm.sh` behind `just vm-*` recipes:

- `just vm-status` — the preflight doctor (agent, session, flatpak
  toolchain, disk headroom, snapshots). Run it first, always.
- `just vm-exec` / `just vm-exec-user` — run a command in the guest as
  root, or as the desktop user inside the session env (runtime dir +
  session bus + Wayland socket), which is how the CLI toggles and
  `flatpak --user` behave exactly as from a guest terminal.
- `just vm-put` — transfer a file in, sha256 verified on both sides.
- `just vm-screenshot` — capture the display for evidence.
- `just vm-snapshot` / `just vm-restore` — clean points around each leg.
- `just vm-push` — build both artifacts host-side and install them in
  the guest exactly as the release tarball lays them down.

The walk, in order:

1. Rewind to the stock snapshot; install the native terminal from the
   published release tarball — `sha256sum -c sha256sums`, copy `usr/`
   over `/usr`, reboot (auto-login returns it).
2. Fetch the listing's two files from the fork branch **by their
   recorded sha256s**. Never regenerate `cargo-sources.json` from a
   working tree: main's lock carries the `[patch.crates-io]` forks and
   pairs with the pinned manifest as a mismatch the build rejects.
3. One-time in-guest toolchain: `apt install flatpak-builder`, add the
   flathub remote. Then `flatpak-builder --user --force-clean` over the
   two files; export and install land under `~/.local/share/flatpak`.
4. Add the applet to the panel: COSMIC Settings → Panel (the honest
   human route), or the config route — append the applet id to
   `~/.config/cosmic/com.system76.CosmicPanel.Panel/v1/plugins_center`
   and `pkill -x cosmic-panel`; the session respawns the panel, which
   re-reads its config and spawns the applet.
5. Drive the rows. The applet's toggle and running/not indicator ride
   the session bus — watch them with `dbus-monitor --session` in-guest.
   Scripted input goes through QEMU's monitor: `virsh send-key` for
   keys, `input-send-event` for the pointer. Pointer quirks that
   matter: absolute moves go to the USB tablet (axis 0–32767 across
   the screen), the guest display runs 1280×800, and a batched
   move+click **drops the button** — send the move, then the press,
   then the release as three separate commands.

What the VM cannot prove: real-hardware timing (GPU throughput,
scanout), and the two GUI steps a script should not fake — the
Settings → Panel listing and the custom-keybind press belong to a
human at the SPICE console.

## Releasing

A release is three recipes and a push, split because one edit is necessarily
post-publish: the applet's flatpak manifest pins the *published* source
archive (URL + sha256), and neither exists until CI has published the
release. The store listing rides a third repo altogether -
pop-os/cosmic-flatpak, via the maintainer's fork.

The whole sequence is driven by `/release` (the `release-orchestration`
skill): it derives the version from the commits since the last tag, drafts
the step-1 curation for your review, runs the recipes, and hands you each
push as an exact command — the agent never pushes, and every outward act
(tag push, pin commit, catalog branch) sits behind a confirmation gate.
Before the tag push it can offer a full-pipeline dry-run rehearsal: push
main alone, dispatch release.yml's dry-run on it, and only push the tag
once the artifact set assembles green. The steps below are what the command
does; they remain the manual path.

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
   [release.yml](../.github/workflows/release.yml) runs the test suite on
   the release bits, builds the per-arch tarballs and `vendor.tar`, proves
   the vendored offline build, guards tag against `Cargo.toml`, and
   publishes the GitHub release with `sha256sums`.
4. **`just release-flatpak-pin X.Y.Z`** - once the release page exists:
   downloads the source archive, records its hash, and commits the applet
   manifest's re-pin. Push that commit.
5. **`just release-catalog X.Y.Z`** - the store listing: regenerates
   `cargo-sources.json` from the current lockfile, re-cuts the fork's
   `dropterm-applet` branch as one clean commit on current pop-os
   master, and force-pushes it (needs SSH keys for the fork). Then open
   the PR from the printed compare URL - the click and the review are
   the catalog's two human steps; there is no gh token on this machine
   to assume either.

The version is semver: additive features take the minor, fixes the patch.
A failed gate mid-recipe leaves the version bumps uncommitted in the tree;
fix and re-run.

## Architecture

The module map - the two crates, the module inventory, the load-bearing seams,
and where the reasoning lives - is in [architecture.md](architecture.md).
