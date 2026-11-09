---
description: Cut a release end to end - version proposal, notes curation, recipes, and gated pushes (every outward act stays yours)
---

Load the `dropterm-release` skill with the `skill` tool (the project's
guided release ritual, adapted from airlock-release; it supersedes the
pack's generic `release-orchestration` here).

`/release [x.y.z]` walks the six phases of a DropTerm release — flight
plan, curation, cut, rehearsal, publish, close of books. The argument,
if given, is the version (`v` prefix optional); without one, the
preflight probes state (resuming a half-done release if it finds one),
derives the version from `git log <last-tag>..HEAD` — Conventional
Commits translation: user-facing `feat:` → minor, `fix:` → patch,
everything else inert, `!`/`BREAKING CHANGE` named at the gate; no
user-facing commits at all → an explicit no-op warning — then presents
the whole flight as one plan: the version with its driving commits, a
living checklist re-printed after every gate, and the opening
structured question (version, hardware-walk status, dry-run choice).

Gate discipline (the skill owns the detail):

1. **Version** — confirm or override the flight plan before any work.
2. **Notes** — the drafted releases.md section and both metainfo
   descriptions are shown in full for approval *before any file is
   written*.
3. **The outward acts are yours, every time**: the two pushes —
   `git push origin main` and the tag push, where the tag push *is*
   the publish. The agent runs only what never reaches the public
   world, announcing each: `just release` (commits and tags locally),
   the release.yml dry-run dispatch (only after your main push and an
   explicit go — it assembles artifacts and publishes nothing), and
   the read-only probes and watches.
4. **The agent watches so you don't**: after the tag push it verifies
   the tag SHA on origin, then `gh run watch`es release.yml through
   its jobs, narrating milestones, and verifies the published asset
   set by name (both per-arch tarballs, the source archive,
   vendor.tar, sha256sums). A "done" is never taken on trust.

Before the tag push the agent offers the dry-run rehearsal and
recommends it outright when the release touched packaging, CI, or
dependencies. On any recipe or CI failure: stop, surface the failure
verbatim, lay out the honest recoveries — the agent never deletes or
moves a published tag.

If the release is partially done (tag already local, or already
pushed), detect it, state which gates already passed, and resume at
the right phase.
