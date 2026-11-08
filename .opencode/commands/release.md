---
description: Cut a release end to end - version proposal, notes curation, recipes, and gated pushes (every outward act stays yours)
---

Load the `dropterm-release` skill with the `skill` tool (the project's
release ritual, adapted from airlock-release; it supersedes the pack's
generic `release-orchestration` here).

`/release [x.y.z]` walks the three steps of a DropTerm release — cut,
publish, close of books. The argument, if given, is the version (`v`
prefix optional); without one, the preflight runs a state probe (on
main, not behind origin, the tag free on both sides) and derives the
version from `git log <last-tag>..HEAD` — Conventional Commits
translation: user-facing `feat:` → minor, `fix:` → patch, everything
else inert, `!`/`BREAKING CHANGE` named at the gate; no user-facing
commits at all → an explicit no-op warning — then proposes it with the
driving commits as one flight plan.

Gate discipline (the skill owns the detail):

1. **Version** — confirm or override the flight plan before any work,
   including whether the hardware release-walk has run.
2. **Notes** — the drafted releases.md section and both metainfo
   descriptions are shown in full for approval *before any file is
   written*.
3. **The outward acts are the maintainer's typing, every time**: the
   tag push (which *is* the publish). The agent runs only the one
   recipe that commits locally and never pushes (`just release`),
   announcing it. Before the tag push the agent offers a full-pipeline
   dry-run rehearsal: push main alone, dispatch release.yml's dry-run
   on it, confirm the artifact set assembles green, then push the tag.
   After the public act the agent verifies the remote
   (`git ls-remote`, the release page's asset set), never trusts a
   yes. If release.yml fails after the tag is public, the agent stops
   with the verbatim failure and lays out the fix-forward options — it
   never deletes or moves a published tag.

On any recipe failure: stop, surface the guard's message verbatim, and
do not improvise a workaround.

If the release is partially done (tag already pushed), detect it,
state which gates already passed, and resume at the right step.
