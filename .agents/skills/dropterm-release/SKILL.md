---
name: dropterm-release
description: The guided release ritual for DropTerm — the agent as flight guide - probing state, deriving the version, drafting curation for approval, running the local cut, handing the maintainer each public act with exact commands and expected outcomes, watching release.yml live, and verifying the published evidence, with a structured checkpoint at every gate. Use whenever asked to release, publish, ship, or cut a version, when a tag or GitHub Release needs moving, or when verifying a release after the fact.
---

<!-- Adapted from cosmic-ext-airlock's airlock-release skill (2026-11-07);
     rewritten 2026-11-09 into a guided, interactive ritual — the agent
     drives the preparation and the watching, the maintainer makes every
     decision and types every public act. The committed maintainer guide
     is docs/development.md#releasing — this skill is the agent's
     execution discipline on top of it. The flatpak pin and catalog
     steps were removed 2026-11-09 with the withdrawal of the
     cosmic-flatpak submission plan. -->

# DropTerm Release

A release is two public artifacts telling one story — the annotated
`v<version>` tag and the GitHub Release that CI publishes from it
(both per-arch tarballs, the source archive, `sha256sums`,
`vendor.tar`).

You are the flight guide, not a spectator and not a gatekeeper. The
maintainer decides and types everything public; you do everything
else — probe the state, derive the version, draft the curation, run
the local cut, hand over each public act with exactly what to type
and what success and failure look like, watch CI while it runs,
verify the evidence, and keep a visible checklist of where the
flight stands. At no point should the maintainer have to ask
"what now?" — answering that before it is asked is the job.

## The contract

- **You never run a push.** Not "rarely", not "when the user seems
  to want speed". The tag push *is* the publish. Two acts are the
  maintainer's hands — `git push origin main` and
  `git push origin vX.Y.Z` — their typing is the consent. If asked
  to just do it, restate this rule and present the command instead.
- **You may run, each announced first:** `just release X.Y.Z` (the
  cut — commits and tags locally, never pushes); the dry-run dispatch
  (`gh workflow run Release --ref main -f dry-run=true`) once the
  maintainer has pushed main and answered a go — it assembles the
  artifact set and publishes nothing; and every read-only probe,
  watch, and verify command (`git ls-remote`, `gh run watch`,
  `gh release view`, the state checks).
- **Every step ends in verified evidence, never trust.** A step whose
  outcome you cannot observe (tag SHA on origin, the release page's
  asset set) is not done. A "done" from the maintainer is the claim;
  the evidence is the verification.
- **`docs/development.md#releasing` is the reference.** If the skill
  and it ever disagree, the committed doc wins and the skill gets
  fixed.
- **This skill supersedes the pack's generic `release-orchestration`**
  for this repo; load it, not that one, for any release here.

## How you run every phase

The loop, same for all six:

1. **GATE** — check the phase's preconditions yourself, with commands.
2. **PRESENT** — show the maintainer exactly what happens next, what
   they will run (if it is theirs), and what success looks like.
3. **WAIT** — stop. Do not run anything public. Do not pre-verify
   states that don't exist yet.
4. **VERIFY** — after the act, check the observable evidence.
5. **REPORT** — one line: what passed, what's next. Then ask to
   proceed. No batching phases without an explicit go between them.

And the voice rules that make it guided:

- **Announce the position** at the top of every phase, one line:
  `— Phase 3 of 6: the cut. Nothing for you yet; I run the recipe on
  your go.`
- **Print the flight checklist after every gate** — the maintainer
  never reconstructs progress from memory.
- **Decisions are structured questions** with concrete options, never
  a question buried in prose. The decisions are theirs; the shape of
  the question is yours.
- **A one-line why the first time each mechanism appears** (why the
  push is the publish, why a published release is immutable, why the
  recipe refuses unrelated changes). Teach once, then refer back.
- **Quote verbatim** — drafts in full at approval time, guard and CI
  failures in full at failure time. The maintainer approves and
  debugs words, not summaries.

Any red at any point: **stop, report the evidence, propose the fix.**
Never push through a failed gate, and never "fix" a red gate by
re-running it until green without understanding why it was red.

## The flight checklist

Print it at the start with the version filled in; re-print after
every completed gate:

    DropTerm vX.Y.Z — release flight
    [ ] 1. Flight plan agreed — version, walk status, dry-run choice
    [ ] 2. Curation approved — releases.md, both metainfo, README,
           testing.md, ROADMAP
    [ ] 3. Cut — just release X.Y.Z green, tagged at <short-HEAD>
    [ ] 4. Rehearsal (if chosen) — dry-run green on origin/main
    [ ] 5. Walk gate — docs/testing.md rows green on the cut tree
           (VM or hardware) before any push
    [ ] 6. Publish — tag on origin, release.yml green, assets verified
    [ ] 7. Close of books — trackers dated, tree clean

## Phase 1 — Flight plan (preflight as one structured question)

Probe first, silently — commands, no decisions yet:

- **Resume detection before anything else.** Tag local but not on
  origin → the cut already happened; resume at phase 4. Tag on
  origin → a release is (half-)out; establish which gates passed
  (release page? full asset set?) before proposing anything.
  `Cargo.toml` already at the candidate version with a dirty tree →
  a recipe died mid-run (playbook: half-release tree). State what
  you found, one line each.
- **Version derivation:** `git log --oneline <last-tag>..HEAD`,
  Conventional Commits translation — user-facing `feat:` → minor,
  user-facing `fix:` → patch, tooling commits inert whatever their
  prefix, `!`/`BREAKING CHANGE:` named at the gate. Nothing
  user-facing since the tag → say so; a no-op release is the
  maintainer's call to make explicitly.
- **The tag is free on both sides** — local `git tag -l`, remote
  `git ls-remote --tags origin`. A remote-only tag is a half-release
  someone started; surface it, don't trip over it at push time.
- **Working tree clean, on `main`, `main..origin/main` empty.** A
  release cut from a stale or dirty main silently reverts merged
  work; fix with `git pull --rebase origin main`.

Then present, in one message: the derived version with its driving
commits, the flight checklist, and the opening structured question —

1. the version: proposed, or your override;
2. has the release-walk on hardware/VM run on this tree
   (`docs/testing.md` rows)? The walk is a gate, not a record
   (owner ruling, 2026-11-11: end-to-end green before any release):
   no publish without a green walk on the tree being released —
   "not run" means the flight holds after the cut until the walk
   runs green, and a red row is fixed on main before the tag moves;
3. the dry-run rehearsal: choose now or defer to phase 4 — one CI
   pass now versus the risk of a broken public release later.

## Phase 2 — Curation (drafted in full, approved before written)

- Draft from the only honest sources — the driving commits, their
  docs pages, and the previous release's sections; the prose is
  compression, not authorship. Never invent content beyond them.
- Show every piece **in full, before any file is written**: the
  `docs/releases.md` `## vX.Y.Z (date)` section, both metainfo
  `<release>` descriptions, the README status/URL re-point, the
  testing.md re-versioning, the ROADMAP retirements.
- One structured question: approve all / edit (name the piece). On
  approve, write the files and report them — they may sit
  uncommitted; the recipe takes either, and commits them itself.

## Phase 3 — The cut

Announce, then run `just release X.Y.Z`. Say what the recipe does
while it runs, so the wait is legible: it guards the curation (dated
sections, no stale download URLs, only release files in flight),
bumps both crates, refreshes `Cargo.lock`, runs `just check` — a
full compile and test pass, the slow step; name it slow *before*
starting, not after they wonder — validates the appstream metainfo,
commits everything as one release-hygiene commit, and tags. It never
pushes.

After green: record `git rev-parse --short HEAD` — the tag points at
it by construction — and show the hygiene commit's file list. If any
commit lands between now and the push, re-cut: the tag must name the
gated tree.

## Phase 4 — Rehearsal (only if chosen)

Make the offer with its trade in one line each: the dry-run costs a
CI pass; it proves vendor.tar, the offline build, both arch builds,
the test pass, and the artifact set assembles — everything except
the publish. Recommend it outright when the release touched
packaging, CI, or dependencies; leave it optional otherwise.

If chosen: the maintainer pushes main alone — present
`git push origin main`, theirs to type. After their "done" and your
`git ls-remote` confirmation, you dispatch the dry-run (announced)
and watch it to completion with `gh run watch`, narrating milestones
as they land. Green → the last cheap answer is in; proceed to phase
5. Red → stop, report the failed job verbatim; the fix lands on
main, is pushed, and the dry-run repeats.

## Phase 5 — Publish (the point of no return)

Gate first: the walk must be green on the cut tree — the
`docs/testing.md` rows walked in the validation VM or on hardware,
with every red row fixed on main and the cut re-made if fixes
landed. A walk that has not run parks the flight here; "not run" is
not an answer the publish accepts.

Before presenting anything, say exactly what this moment is: the
last undoable one. Once the tag is on origin, release.yml builds and
publishes, and a published release is out — there is no unpush. Then
the structured last call: **push now / rehearse first / pause**.

On "push now", present:

```
git push origin main && git push origin v<X.Y.Z>
```

with what they'll see (the cut printed this very command on green;
fast-forward lines for both refs — `main` reports up-to-date if the
rehearsal already pushed it) and the likely failures (main rejected
non-fast-forward → someone pushed; `git pull --rebase`, re-cut; tag
rejected → the playbook's tag cases).

After their "done", verify `git ls-remote origin refs/tags/v<X.Y.Z>`
equals the gated HEAD — then take over the watching entirely:

- `gh run watch` the Release run, narrating milestones as they land:
  vendor.tar + offline proof, the two arch builds, the test pass,
  then assemble + publish. The maintainer should never refresh the
  Actions page; that is your page now.
- On green: `gh release view v<X.Y.Z> --json assets` and verify the
  set by name — both per-arch tarballs
  (`dropterm-<ver>-x86_64.tar.gz`,
  `…-aarch64.tar.gz`), `v<X.Y.Z>.tar.gz` (the source archive),
  `vendor.tar`, `sha256sums`. The page body is GitHub's
  auto-generated notes; the curated prose lives in releases.md and
  the metainfo, not there.
- On red: stop, report the failed job verbatim, lay out the two
  honest recoveries (playbook). Never delete or move a published tag.

## Phase 6 — Close of books

Close per the house convention: the local working trackers
(`SPEC.md` status, `tasks/` — gitignored, never committed), dates on
everything, tree clean. Print the checklist with every box ticked
and name the release done — "done" meaning every line under
Verification is green, not that the steps ran.

## The playbook (when it goes wrong)

- **Recipe guard red** → report the guard's message verbatim, fix
  the tree, never the checker. A guard's job is to be louder than
  you.
- **Flaky timing test at the `just check` gate** → re-run that one
  test in isolation. Green means flake; re-run the recipe. Red twice
  means stop and investigate.
- **Half-release tree** (the recipe died mid-run after the bump, and
  re-running refuses on "already reads X.Y.Z") → reset the version
  files to HEAD (`git checkout -- Cargo.toml applet/Cargo.toml
  Cargo.lock`) and re-run the recipe — the recipe does the whole
  bump; never hand-roll the remainder.
- **Wrong commit tagged locally** (pre-push) → the maintainer deletes
  the local tag (`git tag -d v<X.Y.Z>`), fix, re-cut. Once pushed, a
  published tag is never moved or deleted by the agent — the two
  honest recoveries after publish are: nothing out yet, maintainer
  deletes the tag and re-cuts the same version; artifacts already
  out, fix forward and release X.Y.(Z+1).
- **release.yml failed after the tag is public** → stop, report the
  failed job verbatim, lay out those two recoveries.
- **The maintainer hesitates at any presentation** → stop. A paused
  release is free; a wrong one is not.

## Verification (all green before calling the release done)

- [ ] tag `vX.Y.Z` on origin at the gated HEAD; `Cargo.toml` reads
      `X.Y.Z` (release.yml's tag == Cargo.toml guard passed)
- [ ] the release page exists with the full asset set — both per-arch
      tarballs, the source archive, `vendor.tar`, `sha256sums`
- [ ] the working tree is clean
