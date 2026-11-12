# SPEC: app-identity — one name everywhere

Status: not started · Phase 1 · lane: flag day · branch `port/app-identity`
(worktree `../ced`) · merges first, before every other package

## Objective

DropTerm's runtime and packaging identity becomes plain `dropterm`, so no
name ties it to COSMIC. This package merges first because every later doc,
test, and manual step names the final binary (it is the merge-order
dependency of all Phase 1 packages; they may start earlier and rebase onto
it).

## Scope

In:

- `APP_ID` and its `.Dev` twin (`src/lib.rs:37,46`) and everything derived
  from them: the Wayland app id, the bus name, the layer-shell namespace,
  the cosmic-config namespace, the state directory (`src/session.rs:467`),
  the completions data seat.
- The D-Bus literals in `src/dbus_state.rs`: the object paths and the `….V1`
  interface name at `:32-38`, the `interface`/`proxy` attributes, the
  pinning tests.
- The package, binary, and library names (`Cargo.toml:2,12,16`) and every
  `cosmic_ext_dropterm::` path.
- The applet: its dependency name and hardcoded ids
  (`applet/src/applet.rs:23-26,134,138` and its tests; the desktop and icon
  paths in `applet/src/main.rs`).
- The six `res/` files named by the id, renamed, with their `Exec=`,
  `Icon=`, and AppStream ids.
- The justfile: `name`, `appid` (`:25-26`), the dev launcher and kill
  patterns; both CI workflows (desktop-file and metainfo validation paths,
  the release tarball name); `scripts/vm.sh`; `data/completions.toml`.
- README, ROADMAP, the `docs/` pages that carry the old names, and the
  release skill (`.agents/skills/dropterm-release/` plus its `/release`
  command mirrors).

Out:

- The config store's location beyond what `APP_ID` derives (`config-store`
  owns it).
- Renaming the GitHub repository (outward-facing; the owner's — see Open
  questions).
- README's "From a release" section, which keeps describing the published
  v0.4.0 assets until the next release rewrites them.

## Commands

    just check            # fmt, clippy, test, docs-check; dev-identity builds included
    just build-release    # then `sudo just install` for the manual rows
    just dev              # the renamed dev identity's build-and-relaunch loop

## Files

About 41 tracked files (the plan's "The identity's reach" inventory is the
checklist); the code diff itself is small — roughly twenty identity literals
plus the crate names. The rest is `res/`, CI, the justfile, and docs.

## Constraints (owner rulings, 2026-11-09)

- Plain `dropterm`; greenfield: no shims, no migration code, no dual bus
  names.
- Ships in the same release as `config-store`; no release is cut between
  their merges. (2026-11-12: no break note — pre-distribution, no users.)

## Code style

A scripted sweep (rg/sed driven) plus hand review of every hunk; match
surrounding code. The rg audit in Success criteria is the sweep's inverse
check — run it until the only matches are the deliberate survivors.

## Testing strategy

- `just check` green, the `dev-identity` builds included; CI validates the
  renamed desktop and metainfo files.
- The test pinning the object path to the app id passes under the new id.
- Manual, installed build: `dropterm toggle`, `show`, `hide`, and
  `--is-visible` answer over the new bus name; the rebuilt applet toggles
  it.

## Boundaries

- Always: list every deliberate rg-audit survivor in the merge description;
  update the release skill and its command mirrors in the same merge.
- Ask first: the exact app id and the applet's binary name (owner rulings —
  Open questions); renaming the GitHub repository.
- Never: commit `SPEC*.md` or `tasks/`; touch archived research records
  (they keep the old names by design); add compat aliases for any old name.

## Success criteria

- Over tracked files,
  `rg -i 'cosmicextdropterm|cosmic-ext-dropterm|cosmic_ext_dropterm'`
  matches only the deliberate survivors (the released-install section,
  release-notes history, archived research records), each listed in the
  merge's description.
- `just check` green and CI validates the renamed desktop and metainfo
  files.
- The test pinning the object path to the app id passes under the new id.
- Manual: an installed build answers `dropterm toggle`, `show`, `hide`, and
  `--is-visible` over the new bus name, and the rebuilt applet toggles it.
- The release that ships it carries the break note: the old binary, desktop,
  metainfo, and icon files to remove (the v0.4.0 uninstall globs), and the
  old config and state directories that are no longer read. Superseded
  2026-11-12 (owner): pre-distribution, no users — no break note; the
  draft in merge f698083's description is moot.

## Open questions (owner rulings; needed before `/build`)

1. The exact reverse-DNS app id. Proposal: `io.github.l1br3_io.dropterm` —
   keeps the verifiable GitHub-pages namespace and is the minimal delta from
   `io.github.l1br3_io.CosmicExtDropterm`. An owned-domain id
   (e.g. `dev.l1br3.dropterm`) only if you own that domain.
2. Whether the COSMIC-only applet keeps a COSMIC-convention binary/desktop
   name or goes fully plain. Proposal: binary `dropterm-applet`, applet
   desktop id following the main id's `….Applet` convention.
3. Whether the GitHub repository renames now (with the flag-day merge),
   later (at the break release), or never. Outward-facing: owner's call.

## Size

L by file count, mechanical: a scripted sweep plus hand review, one merge.
