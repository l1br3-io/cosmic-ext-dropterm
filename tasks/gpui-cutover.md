# Chunk: gpui-cutover — the GPUI build becomes DropTerm

Spec: [SPEC-gpui-cutover.md](../SPEC-gpui-cutover.md) · Module id:
`gpui-cutover` · Phase 3 · branch `port/gpui-cutover` (after
`gpui-parity`; owner-gated beyond the code swap).

**Files touched:** `dropterm-gpui` (bin rename to `dropterm`), the
libcosmic frontend's deletion (app composition, executor, slide, widget,
bands, settings, menus — `dropterm`'s src tree reduces to nothing but the
crate's remaining role, if any), `applet/Cargo.toml` (already on core),
release workflow and justfile recipes,
[architecture](../docs/architecture.md), [known-issues](../docs/known-issues.md),
README.

## Planning rulings (made at /planning; veto at plan review)

- **The crate survives the swap**: `dropterm-gpui` renames its bin to
  `dropterm` (or the crate itself renames — whichever leaves one obvious
  terminal crate); the libcosmic frontend crate is deleted outright, not
  parked. Git history keeps it; a parked second frontend is a maintenance
  debt the plan never sanctioned.
- **The applet is untouched functionally**: one canary row proves the
  contract still answers; no applet code changes unless the canary fails.
- **The release is the deferred one** (owner ruling, 2026-11-12): notes
  staged from everything merged since the last tag, the walk gate, the
  owner fires `/release`. Nothing here pushes or tags on its own.

## Task 1: The swap

**Description:** The GPUI bin becomes `dropterm`; the libcosmic frontend
is deleted; workspace, justfile, and release workflow carry one terminal
binary; `dropterm-core` and the applet build clean.

**Acceptance criteria:**
- [x] Exactly one terminal binary, `dropterm`, GPUI-backed; `just check`
      and `just build-all` green *(a2cd89f: the GPUI crate renamed
      `dropterm`, the libcosmic frontend deleted — 31,672 lines — the
      root a virtual workspace, the release workflow's version read
      re-pointed; `just check` and `just build-all` green on the
      result, `target/release/dropterm` the GPUI binary)*
- [ ] The applet canary: toggle, state watch, and the icon row answer
      *(parked with py-4, owner ruling 2026-11-15 — un-park by the
      owner running the canary row)*

**Verification:**
- [x] Tests pass: `just check`
- [ ] Manual: applet canary on the validation VM *(parked with py-4)*

**Dependencies:** `gpui-parity` merged · **Files:** the crate moves and
deletions, workflow, justfile · **Scope:** M

## Task 2: Docs true

**Description:** architecture.md rewritten to the one-core-one-bin
reality (the diagrams included); known-issues gains the
fractional-scale-on-COSMIC row (carried ledger, this pin) and the
integer-rounded display-metadata note; README and configuration checked
for anything the swap invalidates.

**Acceptance criteria:**
- [x] Every doc states the GPUI build's truth; docs-check green; no doc
      describes deleted code *(7d98912: architecture rewritten to the
      one-core-one-bin map, known-issues gains the two carried rows
      and restates the mechanism rows on the GPUI truth, README's
      tagline/risks/attribution re-truthed, images.md ships the
      labeled pair and names the chip as the one wired door, the stale
      `src/` paths re-pointed)*

**Verification:**
- [x] Tests pass: `just check` (docs-check included)

**Dependencies:** Task 1 · **Files:** the docs pages · **Scope:** S

## Task 3: The release — owner-gated

**Description:** The deferred release cut from the result: version bump,
notes merging the staged set with the cutover, the `/release` ritual, the
owner-pass halves and the post-publish tarball walk before the tag moves.

**Acceptance criteria:**
- [ ] The release carries everything merged, per the standing ruling
- [ ] The walk gate passes before the tag moves

**Verification:**
- [ ] Manual: the release ritual's own gates (`dropterm-release` skill)

**Dependencies:** Task 2, owner go · **Files:** version, notes, tag ·
**Scope:** M

## Task 4: The collapse — one package, no applet

**Description:** The owner's ruling (2026-11-15, after cu-1 left three
packages): one crate `dropterm`, one CLI, the COSMIC applet gone. The
`dropterm-core` package dissolves into the terminal package as a lib
target (the `core` module, its toolkit-freedom re-pinned by a
source-walking test now that the manifest is shared with the bin), the
`applet/` package and its res/ files delete, the justfile and the
release workflow lose every applet leg, and the docs tell the truth.

**Acceptance criteria:**
- [x] One package `dropterm` — the bin plus the `core` lib half — one
      CLI; libcosmic out of the dependency graph entirely; `just check`
      and `just build-all` green *(the same day's commits: the rename
      05072a4, the merge + applet deletion + docs pass together;
      clippy clean across both targets, 1225 tests green under
      `dev-identity` too; Cargo.lock −2.3k lines with the libcosmic
      tree gone)*
- [x] No doc describes the deleted crates or applet *(architecture
      rewritten to the one-package-two-halves map, README/dbus/howto/
      FAQ/configuration/index/theming/ROADMAP re-truthed, the release
      notes' history sections keep their past tense, testing.md's
      applet rows and summon clauses removed, the site nav drops the
      applet page; docs-check green)*

**Verification:**
- [x] Tests pass: `just check`
- [x] Release build: `just build-all` *(target/release/dropterm, no
      second artifact)*

**Dependencies:** Task 2 · **Files:** the whole tree — the crate merge,
the applet deletion, justfile, workflow, the docs pages · **Scope:** L

**Owner ruling (2026-11-15, at the desk):** the applet drops — the one
crate is worth more than the panel button; the research archive keeps
its record. The canary row (Task 1) dies with the applet: it walks
nothing anymore.

## Checkpoint: Phase 3 exits

- [ ] Parity proven, one binary, the release cut — Phase 4 (X11, GNOME
      extension) may open
