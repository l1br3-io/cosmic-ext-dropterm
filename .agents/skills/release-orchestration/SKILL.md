---
name: release-orchestration
description: Cuts a DropTerm release end to end behind confirmation gates. Use when it is time to release a version — version selection, release-notes curation, running the release recipes, and handing the maintainer each push. The agent never pushes.
---

# Release Orchestration

## Overview

A DropTerm release is three just recipes and three pushes, split because one
edit (the applet manifest's archive pin) can only exist after CI publishes.
The recipes automate the mechanics; this skill automates the curation and
sequences the legs behind gates. The division of labor is fixed: **the agent
curates and runs recipes; the maintainer pushes.** Every push is an
outward-facing act — the tag push *is* the publish — so each gate hands over
the exact command, waits for the maintainer to run it, and then verifies the
remote state instead of trusting a yes.

## When to Use

- Cutting any release (`/release`, or "release", "ship 0.x.y", "cut a version")
- Asked what the next release should be (Phase 1 answers it, gated)
- Resuming a half-done release — an unpushed tag, an unpinned manifest, a
  catalog leg left undone

## The Gates

| Gate | Held at | Act it protects |
|---|---|---|
| Version | before any work | the number everything else derives from |
| Notes | before any file is written | the user-facing prose (releases.md, appstream) |
| Publish | before the tag push | the public release itself |
| Pin push | before the flatpak-pin commit's push | the manifest the store builds from |
| Catalog | before *running* release-catalog | the force-push to the fork's branch |

A gate is passed only by the maintainer's explicit answer to a direct
question (the `AskUserQuestion` tool where the host has it, a plain question
otherwise). Silence, ambiguity, or "probably" is not consent. A failed recipe
is a stop, never a prompt to improvise.

## Phase 1 — Version (gated)

1. **State probe** (before anything else — cargo-release runs its whole
   verify battery before its first confirmation, and so does this):
   - `git fetch origin`; must be on `main` with
     `git rev-parse main..origin/main` empty. A cut on a stale main would
     publish a release that silently reverts merged work; the fix is
     `git pull --rebase origin main`.
   - Latest tag locally (`git tag --sort=-v:refname | head -1`) and the
     remote tag list (`git ls-remote --tags origin`): the derived tag must
     be free on *both* sides — a remote-only tag is a half-release someone
     started, and it must be surfaced, not tripped over at push time.
   - The working tree: the recipe allows only release files in flight, so
     name anything now that would block it later.
2. **Derive the number** from `git log --oneline <tag>..HEAD`, by the
   Conventional Commits translation: any user-facing `feat:` → **minor**;
   otherwise user-facing `fix:` → **patch**; every other type is inert —
   and tooling commits (recipes, CI, manifest pins) are invisible to the
   number *whatever their prefix*, since a user-facing fix landed in a
   recipe file still ships to users. Scan subjects for `!` and bodies for
   `BREAKING CHANGE:` — a hit takes the minor at 0.x (SemVer's MUSTs bind
   only at `x > 0`; its FAQ names the minor ladder as the 0.x convention)
   and the break is named at the gate. The same rule, unchanged, becomes a
   MUST-level major at 1.0.0.
3. **No-op check**: if nothing since the tag is user-facing, say so at the
   gate — a release shipping nothing user-facing is the maintainer's call
   to make explicitly, not a derivation result (cargo-release warns the
   same: "updating … despite no changes made since tag …").
4. **The gate**: present the flight plan — derived number, driving
   commits, the five files curation will touch, the recipes that will run,
   the pushes that stay the maintainer's, and whether the current
   release-walk ([testing.md](../../docs/testing.md)) has run on hardware;
   cutting without a hardware walk is allowed, but it must be said aloud.
   Ask the maintainer to confirm or override. An override skips the
   derivation, never the gate.

## Phase 2 — Curation (gated)

Draft the five-file curation the `release` recipe machine-verifies, but
touch nothing yet. The date is always **today**, and the sections must carry
today's date or the recipe refuses to run:

- `docs/releases.md` — a `## vX.Y.Z (YYYY-MM-DD)` section at the
  newest-first position; a "Next (unreleased)" pointer, if present, is
  consumed by it. One framing paragraph, then bullets with doc links.
- both `res/*.metainfo.xml` — a `<release version="X.Y.Z" date="…">` entry
  each, newest first inside `<releases>`. The applet's description is the
  formula sentence ("Tracking DropTerm X.Y.Z; the applet itself …").
- `README.md` — the Status line and every `releases/download/v…` URL
  re-pointed.
- `docs/testing.md` — the release-walk rows re-versioned.
- `ROADMAP.md` — only if the fixed bugs actually appear in its bugs list.

Source material, in order: the driving commits' messages (this repo writes
them release-grade), the docs pages they touched, and the previous release's
sections as the house-style template. Never invent content beyond these. If
releases.md carries a "Next (unreleased)" section, the new section consumes
it; keeping one between releases — capture-when-fresh, the one changesets
idea worth having without its tooling — is the maintainer's habit to adopt
or not, never a skill requirement.

Present the complete drafted prose — the releases.md section and both
metainfo descriptions in full — and ask. Revise until approved. Only then
write the files.

## Phase 3 — Cut

Run `just release X.Y.Z`. The recipe's own guards take it from there: dated
sections exist, no stale download URLs, only release files in flight,
versions bumped, lock refreshed, `just check` + appstream validation, one
release-hygiene commit, the annotated tag. On failure, report the guard's
message verbatim and stop. The tree may then carry uncommitted version bumps
— that is the recipe's designed half-release state: fix and re-run, never
hand-roll the remainder.

## Phase 4 — Publish (gated — the maintainer's push)

**Offer the rehearsal first** (cargo-release's dry-run default, on machinery
release.yml already has): split the push — `git push origin main` alone,
then `gh workflow run Release --ref main -f dry-run=true`. The dispatch
assembles the full artifact set from main — which now carries the release
commit, so these are the exact bits the tag would point at — without
publishing and without the tag guard (the workflow skips it for dispatch
events by design). A green dry-run is the last cheap answer before the
irreversible step.

Then hand over exactly:

    git push origin vX.Y.Z

(or the one-liner `git push origin main && git push origin vX.Y.Z` when the
rehearsal was skipped). Never run it, not even on an explicit "go ahead" —
the push is the maintainer's act by design; if asked to push, restate this
and hand the command back. After the maintainer says it is done, **verify,
don't trust**: `git ls-remote origin refs/tags/vX.Y.Z` must show the tag's
SHA. Then release.yml builds (test, vendor.tar, per-arch tarballs, the
offline proof) — this takes minutes. Poll `gh release view vX.Y.Z` (or the
API URL) when gh is authenticated, and confirm the asset set — both
per-arch tarballs, `sha256sums`, `vendor.tar`; the page body is GitHub's
auto-generated notes, so the curated prose lives in releases.md and the
metainfo, not there. Otherwise ask the maintainer to say when the release
page exists. A premature Phase 5 self-corrects: `release-flatpak-pin` fails
until the archive is actually downloadable.

**If release.yml fails after the tag is public**: stop, report the failed
job verbatim, and lay out the two honest recoveries — if nothing published
yet, the maintainer may delete the tag and re-cut the same version; once
the artifacts are out, fix forward and release X.Y.(Z+1). Deleting or
moving a published tag is destructive and maintainer-only; the agent never
does it.

## Phase 5 — Flatpak pin (gated push)

Run `just release-flatpak-pin X.Y.Z` — it downloads the published archive,
hashes it, re-pins the manifest, and commits locally. Then hand
`git push origin main` and wait; verify origin moved with
`git ls-remote origin refs/heads/main`.

## Phase 6 — Catalog (gated — the recipe *is* the outward act)

Unlike the other legs, **running** `just release-catalog X.Y.Z` performs the
outward act itself: it re-cuts the fork's `dropterm-applet` branch as one
clean commit on current pop-os master and force-pushes it. Ask before
running. It needs SSH keys for the fork; report any failure verbatim. On
success it prints the compare URL — opening the PR is the maintainer's
click, the first of the catalog's two human steps (the review is the second).

## Rules

1. **The agent never pushes.** Not the tag, not main, not the fork branch.
   Hand the exact command, wait, verify the remote afterwards.
2. **No gate is skipped.** Resuming mid-release means re-running Phase 1's
   state probe (tag on origin? manifest pinned? catalog branch current?)
   and saying which gates already passed before continuing.
3. **Recipe failure = stop + verbatim output.** The recipes encode the real
   invariants; improvising around them is how a release goes wrong.
4. **Silent on success, loud on failure** — the house convention. A green
   leg gets a one-line confirmation; a red one gets the full error.
5. **Never invent release notes.** Commits, docs, and ROADMAP are the only
   sources; the prose is compression, not authorship.

## Verification

Before declaring the release done, all of these hold:

- [ ] tag `vX.Y.Z` visible on origin and `Cargo.toml` reads `X.Y.Z`
      (release.yml's own tag==Cargo.toml guard passed)
- [ ] the GitHub release page exists with the full asset set — both
      per-arch tarballs, `sha256sums`, `vendor.tar` — confirmed via
      `gh release view vX.Y.Z --json assets`
- [ ] the applet manifest pins the published archive URL + sha256, and that
      commit is pushed
- [ ] the catalog branch is pushed at the recipe's printed SHA, and the
      compare URL is in the maintainer's hands
- [ ] the working tree is clean

## See Also

- For the project-wide floor every release must clear, see
  `../../references/definition-of-done.md`
- For the recipes and the manual path this flow automates, see
  `docs/development.md#releasing` in the repo root
- For the generic launch checklist underneath (rollout, rollback, error
  budget), see `shipping-and-launch`
