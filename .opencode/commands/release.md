---
description: Cut a release end to end - version proposal, notes curation, recipes, and gated pushes (every push stays yours)
---

Load the `release-orchestration` skill with the `skill` tool.

`/release [x.y.z]` walks the five phases of a DropTerm release — version,
curation, cut, publish, post-publish (flatpak pin, then catalog). The
argument, if given, is the version (`v` prefix optional); without one,
Phase 1 runs a state pre-flight (on main, not behind origin, the tag free
on both sides) and derives the version from `git log <last-tag>..HEAD` —
Conventional Commits translation: user-facing `feat:` → minor, `fix:` →
patch, everything else inert, `!`/`BREAKING CHANGE` named at the gate; no
user-facing commits at all → an explicit no-op warning — then proposes it
with the driving commits as one flight plan.

Gate discipline (the skill owns the detail):

1. **Version** — confirm or override the flight plan before any work,
   including whether the hardware release-walk has run.
2. **Notes** — the drafted releases.md section and both metainfo
   descriptions are shown in full for approval *before any file is
   written*.
3. **Publish / pin push / catalog** — every outward-facing act is gated.
   Before the tag push, the agent offers a full-pipeline dry-run rehearsal:
   push main alone, dispatch release.yml's dry-run on it, confirm the
   artifact set assembles green, then push the tag. The agent hands over
   each exact push command and waits; after the maintainer runs it, the
   agent verifies the remote (`git ls-remote`), never trusts a yes.
   `just release-catalog` force-pushes the fork branch itself, so its gate
   sits *before running the recipe*. If release.yml fails after the tag is
   public, the agent stops with the verbatim failure and lays out the
   fix-forward options — it never deletes or moves a published tag.

The agent curates and runs recipes; it never pushes. On any recipe failure:
stop, surface the guard's message verbatim, and do not improvise a
workaround.

If the release is partially done (tag already pushed, manifest already
pinned, catalog branch current), detect it, state which gates already
passed, and resume at the right phase.
