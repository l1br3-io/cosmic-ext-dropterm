---
name: dropterm-release
description: The owner-run release ritual for DropTerm — walking a version through curation, the release recipe, and the tag push with a verification checkpoint at every step, where every outward act is typed by the maintainer, never the agent. Use whenever asked to release, publish, ship, or cut a version, when a tag or GitHub Release needs moving, or when verifying a release after the fact.
---

<!-- Adapted from cosmic-ext-airlock's airlock-release skill (2026-11-07);
     the committed maintainer guide is docs/development.md#releasing —
     this skill is the agent's execution discipline on top of it.
     The flatpak pin and catalog steps were removed 2026-11-09 with the
     withdrawal of the cosmic-flatpak submission plan. -->

# DropTerm Release

A release is two public artifacts telling one story — the annotated
`v<version>` tag and the GitHub Release that CI publishes from it (both
per-arch tarballs, `sha256sums`, `vendor.tar`). One recipe is safe for
you to run; everything that reaches the public world is typed by the
maintainer, every time. Your job is the part around their typing:
curation before, presentation during, verification after.

## The contract

- **You never run the pushes.** Not "rarely", not "when the user seems
  to want speed". The tag push *is* the publish. Two acts are the
  maintainer's hands — `git push origin main` and
  `git push origin vX.Y.Z` — their typing is the consent. If asked to
  just do it, restate this rule and present the command instead.
- **You may run `just release X.Y.Z`** — the cut, which commits locally
  and never pushes — but announce it before running it.
- **Every step ends in verified evidence, never trust.** A step whose
  outcome you cannot observe (tag SHA on origin, the release page's
  asset set) is not done. A "done" from the maintainer is the claim;
  the evidence is the verification.
- **`docs/development.md#releasing` is the reference.** If the skill
  and it ever disagree, the committed doc wins and the skill gets fixed.
- **This skill supersedes the pack's generic `release-orchestration`**
  for this repo; load it, not that one, for any release here.

## The step loop

Run every step the same way:

1. **GATE** — check the step's preconditions yourself, with commands.
2. **PRESENT** — show the maintainer exactly what to run and what
   success looks like. For the public steps this is a hand-off, not a
   suggestion.
3. **WAIT** — stop. Do not run anything public. Do not pre-verify
   states that don't exist yet.
4. **VERIFY** — after they report back, check the observable evidence.
5. **REPORT** — one line: what passed, what's next. Then ask to
   proceed. No batching steps without an explicit go between them.

Any red at any point: **stop, report the evidence, propose the fix.**
Never push through a failed gate, and never "fix" a red gate by
re-running it until green without understanding why it was red.

## Preflight (before step 1)

- Which version, and how it was derived: `git log --oneline
  <last-tag>..HEAD`, Conventional Commits translation — user-facing
  `feat:` → minor, user-facing `fix:` → patch, tooling commits inert
  whatever their prefix, `!`/`BREAKING CHANGE:` taking the minor at 0.x
  and named at the gate. Nothing user-facing since the tag → say so; a
  no-op release is the maintainer's call to make explicitly.
- The tag is free on **both** sides: local (`git tag -l`) and remote
  (`git ls-remote --tags origin`) — a remote-only tag is a half-release
  someone started; surface it, don't trip over it at push time.
- Working tree clean, on `main`, `git rev-parse main..origin/main`
  empty. A release cut from a stale or dirty main silently reverts
  merged work; fix with `git pull --rebase origin main`.
- The curation is drafted and **approved before any file is written**:
  the `docs/releases.md` section and both metainfo descriptions shown
  in full, plus the README status/URL re-point and the testing.md
  re-versioning. The driving commits, their docs pages, and the
  previous release's sections are the only sources — the prose is
  compression, not authorship; never invent content beyond them.
- Whether the release-walk on hardware/VM (`docs/testing.md` rows) has
  run on this tree. Cutting without it is allowed, but say it aloud at
  the gate.

## The steps

**1 — Cut (agent-runnable; announce first).** Run
`just release X.Y.Z`. The recipe's guards do the rest: dated curation
sections exist, no stale download URLs, only release files in flight,
versions bumped, lock refreshed, `just check` plus the appstream
validation, one release-hygiene commit, the annotated tag. It never
pushes. Record `git rev-parse --short HEAD` alongside the green result
— the gate proves *that* commit; the tag it just made points at it by
construction. If any commit lands between now and the push, re-cut: the
tag must name the gated tree.

**2 — Publish (public, irreversible — the tag push *is* the publish).**
Before presenting it, say exactly that: once the tag is on origin, CI
builds and publishes the release, and a published release's artifacts
are out. Offer the rehearsal first — the maintainer pushes `main`
alone, then `gh workflow run Release --ref main -f dry-run=true`
assembles the full artifact set from main without publishing; a green
dry-run is the last cheap answer. Then present:

```
git push origin main && git push origin v<X.Y.Z>
```

After their "done", verify, don't trust: `git ls-remote origin
refs/tags/v<X.Y.Z>` matches the gated HEAD, then poll
`gh release view v<X.Y.Z> --json assets` until the asset set is
complete — both per-arch tarballs, `sha256sums`, `vendor.tar`. The
page body is GitHub's auto-generated notes; the curated prose lives in
releases.md and the metainfo, not there.

**3 — Close of books.** Close the books per the house convention: the
local working trackers (`SPEC.md` status, `tasks/` — gitignored, never
committed), dates on everything, and the tree clean.

## Verification (all of these before declaring the release done)

- [ ] tag `vX.Y.Z` on origin at the gated HEAD; `Cargo.toml` reads
      `X.Y.Z` (release.yml's own tag == Cargo.toml guard passed)
- [ ] the GitHub release page exists with the full asset set — both
      per-arch tarballs, `sha256sums`, `vendor.tar`
- [ ] the working tree is clean

## When it goes wrong

- **Recipe guard red** → report the guard's message verbatim, fix the
  tree, never the checker. A guard's job is to be louder than you.
- **Flaky timing test at the `just check` gate** → re-run that one test
  in isolation. Green means flake; re-run the recipe. Red twice means
  stop and investigate.
- **Half-release tree** (the recipe died mid-run after the bump, and
  re-running refuses on "already reads X.Y.Z") → reset the version
  files to HEAD (`git checkout -- Cargo.toml applet/Cargo.toml
  Cargo.lock`) and re-run the recipe — the recipe does the whole bump;
  never hand-roll the remainder.
- **Wrong commit tagged locally** (pre-push) → the maintainer deletes the
  local tag (`git tag -d v<X.Y.Z>`), fix, re-cut. Once pushed, a
  published tag is never moved or deleted by the agent — the two honest
  recoveries after publish are: nothing out yet, maintainer deletes the
  tag and re-cuts the same version; artifacts already out, fix forward
  and release X.Y.(Z+1).
- **release.yml failed after the tag is public** → stop, report the
  failed job verbatim, lay out those two recoveries.
- **The maintainer hesitates at any presentation** → stop. A paused
  release is free; a wrong one is not.
