# Chunk: distro-reach — what "works on other distros" ships as

Spec: [SPEC-distro-reach.md](../SPEC-distro-reach.md) · Module id:
`distro-reach` · Phase 1, lane C · branch `port/distro-reach` (main
checkout).

**Files touched:** `.github/workflows/release.yml` (shared with
`app-identity`'s tarball-name rename — this package edits after it),
`README.md` (install section), plus whatever recipes the rulings choose.

This chunk is deliberately a shell, as its spec is: the owner rulings
precede the concrete tasks. Tasks 3–4 are contingent on Task 2's
rulings and are rewritten (or dropped) to match them — /build may not
skip that rewrite.

## Task 1: The facts brief

**Description:** Assemble the decision input from `reality-check`'s
session 6 (the plan record's slot): the measured glibc floor (2.39
today, confirmed by `ldd`), missing runtime libraries or fonts
(`Noto Sans Mono`), which distros ran the tarball versus built from
source, and what `just deps` resolved on each package manager.

**Acceptance criteria:**
- [ ] The brief states floor, libraries, fonts, and per-distro build
      results, each traced to the session-6 slot

**Verification:**
- [ ] Manual: every fact cites the plan record

**Dependencies:** `reality-check` session 6 filled; `app-identity`
merged · **Files:** none (the brief lives in this chunk's rewrite and
the ruling discussion) · **Scope:** S

## Task 2: The owner rulings

**Description:** Ask, as structured questions, with the brief in hand:
(1) lower the glibc floor — build the release binaries on an older base
in `release.yml` — or document it? (2) Which native channels to build
(the packaging record named AUR first, with `cosmic-ext-quake-terminal`
as the template), and where do their recipes live? Everything here is
ask-first by nature; the rulings precede the rest of the chunk.

**Acceptance criteria:**
- [ ] Both rulings recorded, with the brief as their evidence

**Verification:**
- [ ] Manual: rulings present before Task 3 starts

**Dependencies:** Task 1 · **Files:** none · **Scope:** XS

## Task 3: The floor ruling, implemented (contingent)

**Description:** If lowered: move the release build to the older base
in `.github/workflows/release.yml`, keeping every existing asset
target; verify the produced binaries reference the older glibc. If
documented instead: README's install section states the floor. Either
way the README install section states the floor — that row is not
contingent.

**Acceptance criteria:**
- [ ] README's install section states the floor
- [ ] If lowered: a dry-run release build proves the older-base binaries

**Verification:**
- [ ] Tests pass: `just check`
- [ ] Manual: the floor claim matches the shipped artifacts

**Dependencies:** Task 2 · **Files:**
`.github/workflows/release.yml`, `README.md` · **Scope:** M

## Task 4: The channel recipes (contingent)

**Description:** Per the channel ruling: write the chosen recipes
(e.g. AUR PKGBUILD from the `cosmic-ext-quake-terminal` template) where
the ruling says they live. Recipes are written and committed; nothing
is published or pushed — publishing is outward-facing, the owner's
hands, governed by the release skill.

**Acceptance criteria:**
- [ ] Each chosen channel has a committed recipe that builds
- [ ] Nothing published by this branch

**Verification:**
- [ ] Manual: each recipe builds in a clean environment matching its
      channel's expectations

**Dependencies:** Task 2 (and Task 3 for the floor the recipes target)
· **Files:** the ruling's recipe locations · **Scope:** M

## Task 5: Release-notes line and merge

**Description:** The notes of the release that changes the floor or
adds channels say so; merge with that note drafted.

**Acceptance criteria:**
- [ ] The release-notes draft names what changed in the install story

**Verification:**
- [ ] Manual: the drafted note matches the implemented ruling

**Dependencies:** Tasks 3–4 · **Files:** the branch's diff · **Scope:**
XS
