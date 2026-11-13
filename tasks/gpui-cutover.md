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
- [ ] Exactly one terminal binary, `dropterm`, GPUI-backed; `just check`
      and `just build-all` green
- [ ] The applet canary: toggle, state watch, and the icon row answer

**Verification:**
- [ ] Tests pass: `just check`
- [ ] Manual: applet canary on the validation VM

**Dependencies:** `gpui-parity` merged · **Files:** the crate moves and
deletions, workflow, justfile · **Scope:** M

## Task 2: Docs true

**Description:** architecture.md rewritten to the one-core-one-bin
reality (the diagrams included); known-issues gains the
fractional-scale-on-COSMIC row (carried ledger, this pin) and the
integer-rounded display-metadata note; README and configuration checked
for anything the swap invalidates.

**Acceptance criteria:**
- [ ] Every doc states the GPUI build's truth; docs-check green; no doc
      describes deleted code

**Verification:**
- [ ] Tests pass: `just check` (docs-check included)

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

## Checkpoint: Phase 3 exits

- [ ] Parity proven, one binary, the release cut — Phase 4 (X11, GNOME
      extension) may open
