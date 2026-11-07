---
description: Cut a release end to end - version proposal, notes curation, recipes, and gated pushes (every outward act stays yours)
---

Load the `dropterm-release` skill with the `skill` tool (the project's
release ritual, adapted from airlock-release; it supersedes the pack's
generic `release-orchestration` here).

`/release [x.y.z]` walks the five steps of a DropTerm release — cut,
publish, flatpak pin, catalog, PR/close of books. The argument, if
given, is the version (`v` prefix optional); without one, the preflight
runs a state probe (on main, not behind origin, the tag free on both
sides) and derives the version from `git log <last-tag>..HEAD` —
Conventional Commits translation: user-facing `feat:` → minor, `fix:` →
patch, everything else inert, `!`/`BREAKING CHANGE` named at the gate;
no user-facing commits at all → an explicit no-op warning — then
proposes it with the driving commits as one flight plan.

Gate discipline (the skill owns the detail):

1. **Version** — confirm or override the flight plan before any work,
   including whether the hardware release-walk has run.
2. **Notes** — the drafted releases.md section and both metainfo
   descriptions are shown in full for approval *before any file is
   written*.
3. **The outward acts are the maintainer's typing, every time**: the
   tag push (which *is* the publish), the pin commit's push, and
   `just release-catalog` (a public force-push — never agent-run, its
   presentation says what it rewrites). The agent runs only the two
   recipes that commit locally and never push (`just release`,
   `just release-flatpak-pin`), announcing each. Before the tag push
   the agent offers a full-pipeline dry-run rehearsal: push main alone,
   dispatch release.yml's dry-run on it, confirm the artifact set
   assembles green, then push the tag. After each public act the agent
   verifies the remote (`git ls-remote`, the release page's asset
   set), never trusts a yes. If release.yml fails after the tag is
   public, the agent stops with the verbatim failure and lays out the
   fix-forward options — it never deletes or moves a published tag.

On any recipe failure: stop, surface the guard's message verbatim, and
do not improvise a workaround.

If the release is partially done (tag already pushed, manifest already
pinned, catalog branch current), detect it, state which gates already
passed, and resume at the right step.
