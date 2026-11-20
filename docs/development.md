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

That builds the debug profile and installs `~/.local/bin/dropterm-dev` — a
small launcher script that starts the dev image
`target/debug/dropterm-dev-image` (a copy of the dev-identity build that only
the `just dev*` recipes ever rewrite) under the memory fence below. Set the
development shortcut's Command to `dropterm-dev` once and leave it.
The `-dev` suffix keeps an installed release (from `sudo just install` or a
release tarball) separate: plain `dropterm` on PATH is always the
installed copy, and the two never shadow each other.

The dev build also carries its own runtime identity —
`io.github.l1br3_io.dropterm.Dev`, selected by the `dev-identity`
cargo feature the dev recipes build with: its own single-instance slot, bus
name, config file, state directory, and wayland namespace. A dev instance and
an installed release run at the same time without seeing each other —
launching one while the other runs never forwards to it, and each answers
only its own launcher and keybind. `just dev` (via `dev-reload`) kills only
the dev instance before starting the new build; an installed release keeps
running untouched, and `just act` / `just toggle` always drive the dev
instance.

The identity reaches the desktop integration too: `just dev-install` also
writes `~/.local/share/applications/io.github.l1br3_io.dropterm.Dev.desktop`
(hidden from the app grid, like the release's own entry), so the window
switcher labels the dev build's anchor window "DropTerm (Dev)" instead of
"unknown" — and a hidden-tab bell notification from the dev build says
"DropTerm (Dev)" as its app name, so with both instances running you can
always tell which one rang.

The separation reaches the files: the dev build reads and writes
`~/.config/io.github.l1br3_io.dropterm.Dev.ron` and
`~/.local/state/io.github.l1br3_io.dropterm.Dev/`, so config-schema
churn on main cannot migrate the daily driver's settings. To develop against
your real settings, copy them over once:

```sh
cp ~/.config/io.github.l1br3_io.dropterm.ron \
   ~/.config/io.github.l1br3_io.dropterm.Dev.ron
```

Deliberately a copy, not a share — dev experiments never write back to the
real config.

One caveat: the launcher runs the dev image, whose identity only the
`just dev*` recipes can change. A plain `cargo build` (or `cargo test`, or
`just check`) refreshes the *shared* debug bin `target/debug/dropterm`
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
plus launcher. Builds are silent on success; `V=1 just dev` shows the output.

Every local launch of the app rides a memory fence: the `just dev*` recipes,
the `dropterm-dev` launcher itself (so a COSMIC shortcut, `just act` /
`just toggle` cold starts, and manual runs are fenced too), and `just run`
all start the app inside a systemd-run scope (`MemoryMax=4G`,
`MemoryHigh=3G`, no swap by default; `DROPTERM_DEV_MEM_MAX` /
`DROPTERM_DEV_MEM_HIGH` override for a session that legitimately needs
more). The instance and every shell it spawns stay inside the cap, so a
runaway dies at the cgroup instead of OOM-ing the host — the
`just mem-churn` probe's hard-pinned, smaller cap is the same fence sized
for the test process alone. The boundary: raw `cargo run` / `cargo test`
outside just, and the installed release copy, are outside the fence —
fencing the release is an in-app concern, not the justfile's.

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
| `just dev`, `dev-reload`, `dev-install`, `dev-watch` | Development — the debug build under the `.Dev` identity, launched as `dropterm-dev`, plus its (app-grid-hidden) desktop file |
| `just act …`, `toggle`, `show`, `hide`, `new-tab …` | The dev instance — the verbs ride the `-dev` launcher and the dev instance executes them |
| `just dev-uninstall` | Development — removes the `-dev` launcher and the dev desktop file |
| `just deps` | Prerequisites — checks the C toolchain, `pkg-config`, the system libraries, and cargo, installing what is missing via the distro's package manager (apt/dnf/pacman/zypper) or rustup; on macOS it verifies Xcode's tools, the Metal Toolchain component, and the rustc 1.98 floor instead (verifies and instructs, never installs). The build and install recipes run it first, and it is a silent no-op when everything is present |
| `just build-all` + `sudo just install` | Installed — the release build into `/usr`; a release tarball's `usr/` tree lands in the same places, and `sudo just uninstall` removes it |
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

## Building on macOS

The crate builds from source on macOS (aarch64 primary, x86_64
cross-checked) and its toolkit-neutral core tests run there — the
[macOS initiative](research/portability-plan.md) pulled the platform ahead
of the X11/GNOME phase. What exists today is the build gate, not a usable
app: the terminal's drop-down surface is Wayland layer-shell, so on the Mac
every surface verb refuses honestly (see
[known issues](known-issues.md)) until the `macos-surface` module lands.
No `.app`, no DMG, no signing — build from source, that is the ruling.

Prerequisites (nothing from Homebrew):

- **Xcode Command Line Tools** — `xcode-select --install` if `xcodebuild`
  is missing.
- **The Metal Toolchain component** — Xcode 26+ ships it as a separate
  download (`xcodebuild -downloadComponent MetalToolchain`), and the
  GPUI pin's macOS backend compiles Metal shaders in its build script.
- **Rust 1.98 or newer** — the macOS floor (the linux gate's 1.97 does
  not compile the apple backend; `rustup update stable` if `just deps`
  says so).

`just deps` verifies all three (and downloads the Metal component if
missing); then:

```sh
cargo build --locked          # the arm64 binary
cargo test --locked           # core tests green on the Mac
rustup target add x86_64-apple-darwin
cargo check --locked --target x86_64-apple-darwin   # the second arch
```

CI runs the same shape on a `macos-latest` runner beside the ubuntu jobs.
The dev recipes below this section are Linux's own (systemd fences,
`/proc`, the COSMIC desktop file) — they are not the Mac's workflow.

## The COSMIC validation VM

The checklist's runtime rows in [testing.md](testing.md) need a live
COSMIC session; the validation VM is where they walk when no spare
hardware is at hand. The guest is a Pop!_OS 24.04 COSMIC image under
virt-manager/QEMU — `/dev/kvm`, virtio-gpu with virgl: cosmic-comp
renders GLES and libcosmic falls back to GL, so no guest Vulkan is
needed (cosmic-epoch#3831/#3844). One-time guest setup: install
`qemu-guest-agent` (the harness's only channel in — no SSH), auto-login
via greetd so reboots return to the desktop unattended, then snapshot
the stock state and name it; every walk rewinds to it first.

The harness is `scripts/vm.sh` behind `just vm-*` recipes:

- `just vm-status` — the preflight doctor (agent, session, disk
  headroom, snapshots). Run it first, always.
- `just vm-list` — the libvirt domains on this host with their state
  and guest users, i.e. the targets any recipe accepts and the
  candidates for `VM_USER`.
- `just vm-exec` / `just vm-exec-user` — run a command in the guest as
  root, or as the desktop user inside the session env (runtime dir +
  session bus + Wayland socket), which is how the CLI toggles behave
  exactly as from a guest terminal.
- `just vm-put` — transfer a file in, sha256 verified on both sides.
- `just vm-screenshot` — capture the display for evidence.
- `just vm-snapshot` / `just vm-restore` — clean points around each leg.
- `just vm-push` — build both artifacts host-side, stage the `usr/`
  tree as release CI does, and transfer the tarball into the guest
  (left at `/root/dropterm-vm.tar.gz`; nothing the guest runs changes
  yet).
- `just vm-install` — unpack the last pushed tarball over `/`,
  refresh the desktop database, and stop processes still running a
  replaced binary (the guest-side half of `stop-stale-release`).
  Re-running it needs no rebuild; `vm-push` again only for a new build.

Every recipe targets the `PopOs24` domain by default (the value
`VM_NAME` names, if set; `just vm-list` shows the choices). To target
another libvirt domain, pass it as an optional argument — `just
vm-push OtherVm`, `just vm-exec 'pgrep -a cosmic-comp' OtherVm` — or
set the variable with `VM_NAME=OtherVm just vm-push` (or `just
vm-name=OtherVm vm-push`; the assignment precedes the recipe).
Recipes that use the guest user take it next — `just vm-status OtherVm
alice` — or read `VM_USER` (`VM_USER=alice just vm-status`, `just
vm-user=alice vm-status`).

### When `vm-push` fails with "guest could not download"

The preflight probes the transfer server from the host over loopback,
so it passes while the guest still can't reach the host; the guest's
`curl -sf`/`wget -q` then swallow the real error. Get it:

    just vm-exec 'curl -v --max-time 5 -o /dev/null http://192.168.122.1:8000/dropterm-vm.tar.gz'

- Instant `Permission denied` (EACCES) = **guest MAC** rejecting the
  agent's outbound `connect`.
- Hang/timeout = **host firewall** dropping guest-to-host traffic.

**Host firewall** (ufw/firewalld/nftables): the transfer port must
accept the guest subnet on the bridge. Pop!_OS ships none, so enabling
one after it worked breaks this silently. ufw:

    sudo ufw allow from 192.168.122.0/24

**Guest MAC** (SELinux on Fedora/RHEL/openSUSE; AppArmor the analog on
Ubuntu/Debian): the agent runs confined and its `guest-exec` children
inherit that, so the guest's own `curl` is denied. Confirm with
`just vm-exec 'id -Z'` (confined = `virt_qemu_ga_t`, not `unconfined`).
On SELinux the `virt_qemu_ga_run_unconfined` boolean does **not** unconfine
plain `guest-exec`; make the domain permissive instead, from the guest:

    sudo semanage permissive -a virt_qemu_ga_t

Install `policycoreutils-python-utils` first if `semanage` is missing.
AppArmor needs the same outcome (agent profile unconfined or its connect
rule added); no verified recipe yet, so none is recorded. Permissive
persists across reboots.

The walk, in order:

1. Rewind to the stock snapshot; install the native terminal from the
   published release tarball — `sha256sum -c sha256sums`, copy `usr/`
   over `/usr`, reboot (auto-login returns it).
2. Drive the rows. Scripted input goes through QEMU's monitor:
   `virsh send-key` for keys, `input-send-event` for the pointer.
   Pointer quirks that matter: absolute moves go to the USB tablet
   (axis 0–32767 across the screen), the guest display runs 1280×800,
   and a batched move+click **drops the button** — send the move, then
   the press, then the release as three separate commands.

What the VM cannot prove: real-hardware timing (GPU throughput,
scanout), and the two GUI steps a script should not fake — the
Settings → Panel listing and the custom-keybind press belong to a
human at the SPICE console.

## Releasing

A release is one recipe and a push: `just release X.Y.Z` cuts and tags
locally, the push publishes (release.yml fires on the tag).

The whole sequence is driven by `/release` (the `dropterm-release`
skill, adapted from cosmic-ext-airlock's release ritual): it derives the
version from the commits since the last tag, drafts the curation for
your approval before anything is written, and runs the recipe that
commits locally and never pushes. Before the tag push it offers a
full-pipeline dry-run rehearsal — you push main alone, it dispatches
release.yml's dry-run (which assembles the artifact set and publishes
nothing) — and the tag goes up only once two gates are green: the
assembled set, and the walk — [testing](testing.md)'s checklist run
end-to-end on the cut tree (validation VM or hardware). The walk is a
gate, not a record: no green walk, no publish, and a red row is fixed
on main before the tag moves (owner ruling, 2026-11-11: end-to-end
green before any release). After the
push it watches release.yml through its jobs and verifies the published
asset set, re-printing the flight checklist at every gate so "what
now" is answered before it is asked. Everything that reaches the
public world is typed by you, every time — the two pushes. The steps
below are what the command does; they remain the manual path.

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
3. **Walk the cut tree** - the [testing](testing.md) checklist runs
   end-to-end on the release bits (`just build-release` output in the
   validation VM, or hardware) before any push. End-to-end green
   before any release; a red row is fixed on main and the cut re-made.
4. **Publish**: `git push origin main && git push origin vX.Y.Z`.
   [release.yml](../.github/workflows/release.yml) runs the test suite on
   the release bits, builds the per-arch tarballs and `vendor.tar`, proves
   the vendored offline build, guards tag against `Cargo.toml`, and
   publishes the GitHub release with `sha256sums`.

The version is semver: additive features take the minor, fixes the patch.
A failed gate mid-recipe leaves the version bumps uncommitted in the tree;
fix and re-run.

## Carried patches

Private source copies that replace a published crate through the root
manifest's `[patch.crates-io]` — never public forks. The ledger, and each
entry's retirement path:

| # | Crate | Carries | Retirement |
|---|---|---|---|
| 1 | `vte` 0.15.0 (git fork, branch `dropterm-osc133d`) | the OSC 133;D status-exit passthrough upstream logs away — parsed into a default-no-op `Handler` method | upstream PRs (the root manifest's patch comment names them) |
| 2 | `alacritty_terminal` 0.25.1 (git fork, branch `dropterm-osc133d`) | `Event::PromptExit(code)` implementing that parse on `Term` | same |
| 3 | `gpui-component` 0.9.1 (path copy, `carried/gpui-component/`) | one const: the Linux `SHADOW_SIZE` in `src/window_border.rs` zeroed — Root's auto-border inflated every layer surface 2×20 px (a 1280×320 drawer negotiated 1320×360, no opt-out at this pin) | an upstream per-window border opt-out; [kit-root-border](research/kit-root-border.md) is the durable record and the standing reminder |

### The gpui-component upgrade ritual (entry 3)

On every Kit bump — and any time the border is questioned:

1. Replace `carried/gpui-component/`'s sources with the new release
   (the copy keeps the published layout: manifest, `src/`, `locales/`,
   `build.rs`).
2. Re-apply the one-const change: `src/window_border.rs`, the
   `#[cfg(target_os = "linux")]` `SHADOW_SIZE` arm, to `px(0.0)`. The
   `dropterm` crate's `the_carried_copy_is_the_patched_kit` test
   fails `just check` if this is forgotten.
3. Check the release for a public border opt-out (changelog, `Root`
   docs, the `window_border` constant). If one exists: drop the copy,
   depend on the real crate again, re-run the verification below, and
   retire the entry — kit-root-border's row moves to its archive.
4. Re-run the verification on the sway-headless rig and the COSMIC
   validation VM (the walk routes are the ones above):

   ```sh
   DROPTERM_CHECK=kit-c2b DROPTERM_EXIT_AFTER_MS=9000 dropterm
   ```

   The pass rows, in the log: every cadence height echoes back exactly
   (`kit resize #3 requested 1280x320` followed by
   `kit frame … drew size=1280x320` — any 2×20 px inflation means the
   carried const regressed), `kit popover open_changed` absent (the
   overlays survived the full cadence), and the event loop exits
   cleanly. Hold one screenshot per compositor beside the log.

The vte/alacritty forks (entries 1–2) re-apply their one arm on their
own bump; each branch is cut from the exact crates.io publish commit,
so the patched sources stay the lockfile's versions plus the one arm.

## Architecture

The module map - the two crates, the module inventory, the load-bearing seams,
and where the reasoning lives - is in [architecture.md](architecture.md).
