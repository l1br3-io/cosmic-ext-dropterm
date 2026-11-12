# Chunk: app-identity — one name everywhere

Spec: [SPEC-app-identity.md](../SPEC-app-identity.md) · Module id:
`app-identity` · Phase 1, lane flag day · branch `port/app-identity`
(worktree `../ced`) · merges first, before every other package.

**Files touched** (about 41 tracked; the plan's "The identity's reach"
inventory in
[docs/research/portability-plan.md](../docs/research/portability-plan.md)
is the checklist): `src/lib.rs`, `src/dbus_state.rs`,
`src/app/remote.rs`, `src/main.rs`, `applet/src/applet.rs`,
`applet/src/main.rs`, `Cargo.toml` (+ `applet/Cargo.toml`), the six
`res/` files, `justfile`, `.github/workflows/{ci,release}.yml`,
`scripts/vm.sh`, `data/completions.toml`, README, ROADMAP, 13 `docs/`
pages, the release skill and its command mirrors.

Code style: a scripted sweep (rg/sed driven) plus hand review of every
hunk. Note the identity literals beyond the spec's named anchors that
the sweep must catch: the clap `name` and argv[0] strings
(`src/main.rs:57`, `src/app/remote.rs:469`) and the eprintln prefix
(`src/main.rs:112`).

## Task 1: Owner-ruling gate — the three rulings

**Description:** The spec's three open questions, asked as structured
questions before any file changes. Defaults if overriden by nothing
else: (1) app id `io.github.l1br3_io.dropterm` (minimal delta, keeps the
verifiable GitHub-pages namespace; an owned-domain id only if the owner
holds the domain); (2) applet binary `dropterm-applet`, desktop id
following the main id's `.Applet` convention; (3) repository rename
timing — now, at the break release, or never (outward-facing; the
owner's hands either way, and the repo rename itself is not part of this
branch's diff).

**Ruled (2026-11-10, owner, before distribution):**
1. App id: `io.github.l1br3_io.dropterm`.
2. Applet: binary `dropterm-applet`, desktop id
   `io.github.l1br3_io.dropterm.Applet`.
3. Repo rename: now, alongside this merge — the sweep writes the new
   repo URLs (org `l1br3-io`, repo `dropterm`); the rename act itself
   remains the owner's and stays out of this branch's diff.

**Acceptance criteria:**
- [x] App id decided; applet names decided; rename timing decided
- [x] Every remaining task uses the decided literals

**Verification:**
- [x] The rulings are recorded in the merge description (merge f698083)

**Dependencies:** None · **Files:** none · **Scope:** XS

## Task 2: Code identity sweep

**Description:** `APP_ID` and its `.Dev` twin (`src/lib.rs:37,46`) and
everything derived from them (Wayland app id, bus name, layer-shell
namespace, state directory at `src/session.rs:467`, completions data
seat); the D-Bus literals in `src/dbus_state.rs` (`:32-38`, the
`interface`/`proxy` attributes, the pinning tests — including the
hardcoded identity strings in `release_state_carries_the_plain_identity`
/ `dev_state_carries_the_dev_identity`); the package, binary, and
library names (`Cargo.toml:2,12,16`) and every `cosmic_ext_dropterm::`
path; the applet's dependency name and hardcoded ids
(`applet/src/applet.rs:23-26,134,138` and its tests; the desktop and
icon paths in `applet/src/main.rs`); the clap/argv literals noted above.
No compat aliases for any old name (greenfield ruling).

**Acceptance criteria:**
- [x] `cargo test` green with the renamed crate paths; the object-path
      pinning test passes under the new id
- [x] `just check` green, `dev-identity` builds included

**Verification:**
- [x] Tests pass: `just check`
- [x] Manual: `dropterm --help`, `--is-visible` compile-path sanity
  (headless: `Usage: dropterm`; `--is-visible` answers `false`, exit 1)

**Dependencies:** Task 1 · **Files:** the code subset of the inventory
(≈6 files) · **Scope:** M (mechanical)

## Task 3: Packaging, CI, and build recipes

**Description:** Rename the six `res/` files named by the id, rewriting
their `Exec=`, `Icon=`, and AppStream ids; the justfile's `name`,
`appid` (`:25-26`), dev launcher and kill patterns; both CI workflows
(desktop-file and metainfo validation paths, the release tarball name);
`scripts/vm.sh`; `data/completions.toml`.

**Acceptance criteria:**
- [x] CI's desktop-file and metainfo validation point at the renamed
      files and would pass
- [x] `just build-release` produces the renamed tarball name

**Verification:**
- [x] Build succeeds: `just build-release`
- [x] Manual: the renamed res/ files validate (desktop-file-validate /
      the CI recipe's own tooling locally)

**Dependencies:** Task 2 · **Files:** `res/`, `justfile`, both workflows,
`scripts/vm.sh`, `data/completions.toml` · **Scope:** M (mechanical)

## Task 4: Docs and the release ritual

**Description:** README (except "From a release", which keeps describing
the published v0.4.0 assets), ROADMAP, the 13 `docs/` pages carrying the
old names, and the release skill (`.agents/skills/dropterm-release/`
plus its `/release` command mirrors — same merge, per the spec's
boundaries).

**Acceptance criteria:**
- [x] Every docs page and the skill name the new identity; README's
      "From a release" section untouched
- [x] `just docs-check` green

**Verification:**
- [x] Tests pass: `just docs-check`

**Dependencies:** Task 2 · **Files:** README, ROADMAP, `docs/*`,
`.agents/skills/dropterm-release/**`, `.opencode/commands/release.md`,
`.zcode/commands/release.md` · **Scope:** M (mechanical)

## Task 5: The rg audit

**Description:** The sweep's inverse check, run to completion: over
tracked files, `rg -i 'cosmicextdropterm|cosmic-ext-dropterm|cosmic_ext_dropterm'`
matches only deliberate survivors (the released-install section,
release-notes history, archived research records — records keep the old
names by design). Every survivor listed for the merge description.

**Acceptance criteria:**
- [x] The rg command's only matches are deliberate survivors, each
      listed
- [x] Any straggler the audit finds is renamed or justified
  (none found: 61 matches, all survivors; the survivor list lives in
  the T4 commit message and is re-stated in the run summary for the
  merge description)

**Verification:**
- [x] Manual: the audit output is attached to the merge description
      (merge f698083 carries the 61-match survivors list)

**Dependencies:** Tasks 2–4 · **Files:** whatever the audit surfaces ·
**Scope:** S

## Task 6: Manual verification and merge

**Description:** Installed-build verification, then the merge. `just
build-release && sudo just install`; then `dropterm toggle`, `show`,
`hide`, and `--is-visible` answer over the new bus name, and the rebuilt
applet toggles it. The merge description carries the survivors list
(Task 5) and drafts the break note the shipping release must carry: the
old binary, desktop, metainfo, and icon files to remove (the v0.4.0
uninstall globs), and the old config and state directories no longer
read. No release is cut between this merge and `config-store`'s (the
break event is one release).

**Acceptance criteria:**
- [x] Manual rows pass on an installed build (verbs + applet) — walked
      2026-11-11, agent-driven in the validation VM (pre-airlock stock
      rewind, `vm-push` install, no host sudo available): not-running
      `--is-visible` → false/exit 1; `toggle` first-launch starts + shows
      (true/exit 0); `hide` → false; `show` → true; `toggle` → false;
      NameHasOwner true on io.github.l1br3_io.dropterm while running,
      false before, and false on the old name throughout; applet
      panel-spawned via the config route, its indicator read V1 props
      over the new bus, and its click was wire-verified as
      ActivateAction("toggle") at /io/github/l1br3_io/dropterm → shown
      (dbus-monitor log + screenshots under target/vm-evidence/). One
      scripted-input quirk noted: a repeat click while shown didn't
      re-fire (overlay z-order; one wire-verified toggle stands)
- [x] Merged to main; description lists survivors and the break note —
      merged f698083 (2026-11-11, local `--no-ff` merge; push stays the
      owner's); the description carries the rulings record, the
      61-match survivors list, and the break-note draft (old
      binaries/desktop/metainfo/icons under /usr, the old config and
      state directories, the bus-name move)

**Verification:**
- [x] Manual: the four verbs and the applet over the new bus name
      (VM evidence above; the host `sudo just install` walk remains the
      owner's — sudo needs a password here)
- [x] `just check` green on the merge result — re-asserted on f698083
      in the main checkout (tree byte-identical to 269fbdd's, verified
      by `rev-parse HEAD^{tree}` match; check exit 0)

**Dependencies:** Task 5 · **Files:** the branch's diff · **Scope:** S

## Checkpoint: after app-identity (from the plan record)

- [ ] `just check` and CI green — `just check` green locally on the
      merge result (f698083); CI green pending the owner's push of main
      (now 6 ahead of origin/main)
- [x] An installed build toggles under the new name; the applet follows
      (validation-VM walk of 2026-11-11, Task 6's manual rows)
