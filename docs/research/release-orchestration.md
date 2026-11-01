# Research: how should the release-orchestration skill cut releases — what do cargo-release, release-please, changesets, and the terminal/COSMIC peers do that DropTerm should adopt?

Date: 2026-11-01
Question: *How do comparable projects and tool ecosystems orchestrate releases — Rust
release tooling, changelog/release-PR automation, AI-agent release skills with human
gates, COSMIC/desktop-app peers, terminal peers — and which of their practices should
DropTerm's `release-orchestration` skill adopt?*

## TL;DR

**DropTerm's gated skill already implements the strongest pattern in the survey —
cargo-release's "dry-run by default, confirm every outward step, verify everything
before tagging" — and the ecosystems surveyed offer five cheap sharpenings, not a
redesign: a behind-remote pre-flight check, a no-op-release warning in Phase 1,
spec-exact version-derivation wording, tests inside the release CI (alacritty's one
real precedent), and an optional "Next (unreleased)" accumulator.** cargo-release's
`--execute`/`--no-confirm` semantics (`docs/reference.md`: "-x, --execute  Actually
perform a release. Dry-run mode is the default"; the `[y/N]` prompt accepts only a
literal `y`, `src/ops/shell.rs:9-18`) are the closest thing to a primary-source
blessing of DropTerm's gate design, and its pre-flight battery — clean tree, tag
absent, monotonic versions, allowed branch, not-behind-remote — runs *before* the
first confirmation, exactly where DropTerm's Phase 1 sits. The release-PR tools
(release-please, changesets) solve a problem DropTerm solved differently and, for a
single maintainer with a curated five-file curation, better: they separate preparing
a release from publishing via a PR, and DropTerm separates them via gates plus a
human push — same separation, less machinery. No AI-agent release skill exists in
either anthropics/skills (19 skills, none about releasing) or
addyosmani/agent-skills (the 25-skill pack DropTerm already ships; the closest are
the generic `shipping-and-launch` checklist and `/ship` go/no-go fan-out) — the
gated-skill design has **no precedent to copy and none contradicting it**. The COSMIC
peers do *less* than DropTerm (no GitHub Releases, no automation, no appstream
release notes at all), and the terminal peers contribute two conventions worth a
line each: alacritty's "changelog documents the user-perspective, not the technical
background" and wezterm's idempotent `gh release view || gh release create`. Nothing
in the survey argues for adopting a tool; several things argue for tightening the
skill's own derivation and pre-flight wording against the specs it already cites.

## Findings

### 1. cargo-release: dry-run is the default, every step confirms, and the verify battery precedes the first confirmation

cargo-release @ `5f271e4beb11b59f6305fbce1df7f9033ec27042` (cloned 2026-11-01).

**Dry-run/confirm semantics.** The CLI makes rehearsal the default and execution the
opt-in:

- `docs/reference.md:36` — `-x, --execute    Actually perform a release. Dry-run mode is the default`
- `docs/reference.md:37` — `--no-confirm   Skip release confirmation and version preview`
- `README.md:52-53` — "By default, `cargo-release` runs in dry-run mode so you can
  safely run it and verify what it will do."

The confirmation is a per-step gate, not one global prompt: every step calls
`super::confirm(...)` (`src/steps/version.rs:140`, `replace.rs:150`, `hook.rs:154`,
`commit.rs:95`, `publish.rs:160`, `tag.rs:138`, `push.rs:124`), and the gate only
fires when it matters — `src/steps/mod.rs:455`: `if !dry_run && !no_confirm`. The
prompt itself rejects everything but an explicit yes — `src/ops/shell.rs:9-18`:

```rust
pub fn confirm(prompt: &str) -> bool {
    ...
    console_println(&format!("{prompt} [y/N] "), Style::new());
    ...
    input.trim().to_lowercase() == "y"
}
```

That is DropTerm's "Silence, ambiguity, or 'probably' is not consent" rule
(SKILL.md, The Gates), with cargo-release as the prior art.

**The verify battery runs BEFORE the first confirmation.** `src/steps/release.rs`
labels it explicitly — line 195: `// STEP 0: Help the user make the right decisions.`
— and lines 197-258 run, in order: `verify_dependencies`, `verify_git_is_clean`
(`src/steps/mod.rs:29`: "uncommitted changes detected, please resolve before
release"), `verify_tags_missing`, `verify_monotonically_increasing`, a
double-publish check, `warn_changed`, `verify_git_branch` (the `allow-branch`
allowlist), `verify_if_behind`, `verify_metadata`, `verify_rate_limit`. Only then
does line 269 run `// STEP 1: Release Confirmation`. Two of these are directly
adaptable:

- **Behind-remote check** (`verify_if_behind`, `src/steps/mod.rs:151-189`): it
  fetches the push remote and, when the branch is behind, emits help text — line
  173: "to update your release branch, run `git pull --rebase {git_remote}
  {branch}`". Rationale in code: if you are not pushing, you cannot be behind
  (`if !ws_config.push() { return Ok(success); }`, line 158). DropTerm's Phase 3
  has no equivalent — `just release` checks branch name and tag existence but never
  whether local `main` is behind `origin/main`, so a cut made on a stale main would
  push a release that silently reverts merged work.
- **No-op-release warning** (`warn_changed`, `src/steps/mod.rs:348-395`): when
  nothing but `Cargo.lock` changed since the prior tag it warns — line 374:
  `"updating {} to {} despite no changes made since tag {}"`. This is the exact
  guard for DropTerm Phase 1's blind spot: a `feat:`-prefixed release-tooling
  commit would currently derive "minor" even though the release ships nothing user
  facing. (DropTerm's rule "release-tooling commits are invisible to the number"
  handles the tooling case; warn_changed catches the residual "nothing at all
  changed" case.)

**Sign/tag/push ordering.** After the confirmation the steps run
(`src/steps/release.rs`): STEP 2 (line 272) version bump + lock update +
`pre-release-replacements` + `pre-release-hook` + commit; STEP 3 (line 329)
`cargo publish`; owner (line 332); STEP 5 (line 333) `// STEP 5: Tag`; STEP 6 (line
336) `// STEP 6: git push`. So the artifact is *published* before the tag and push —
cargo-release is crates.io-shaped and tags after publish so a failed publish never
leaves a tag pointing at an unreleasable commit. DropTerm inverts this by design
(the tag push *is* the publish, because release.yml fires on the tag), which is the
right shape for a GitHub-Releases app; the transferable part is only that each
outward act is separately gated, which DropTerm already does.

**What it refuses to do by default.** `verify = true` by default
(`docs/reference.md:417-420`: "Verify the contents by building them" — via
`cargo publish`'s own build); dirty tree, existing tag, non-monotonic version, or a
disallowed branch abort before any mutation (`allow-branch` default is
`["*", "!HEAD"]`, `docs/reference.md:169-170` — detached HEAD is always refused).
One deliberate wart worth knowing: the pre-release hook runs even in dry-run mode,
receiving `DRY_RUN=true` so the script can self-skip — `src/steps/hook.rs:209-216`
sets `OsStr::new("DRY_RUN") => ... if dry_run { "true" } else { "false" }` and
comments "we use dry_run environmental variable to run the script // so here we set
dry_run=false and always execute the command." A hook with side effects therefore
runs during rehearsal; DropTerm's recipes avoid this class of surprise by refusing
rather than running.

**Changelog handling: deliberately unopinionated.** `docs/faq.md:31-37`: "At the
moment, `cargo release` is unopinionated in its support for CHANGELOGs due to the
complexities and the different approaches people might want to take", suggesting
git-cliff as a `pre-release-hook` for generated logs, and
`pre-release-replacements` for hand-written ones. The replacement mechanism carries
occurrence-count guards DropTerm's recipes will recognize —
`docs/reference.md:248-251`: `min` (default 1), `max`, `exactly` — the same
"re-pin exactly once or fail" shape as `release-flatpak-pin`'s `grep -c` checks
(justfile lines 672-679).

### 2. release-please: the release-PR pattern — preparation as a reviewable PR, GitHub as the state machine, and a documented resume procedure

release-please @ `c589293ca38aee267b329f9bf9b26c9d6516a687` (cloned 2026-11-01).

**The pattern.** `docs/design.md:18-21`: "release-please will maintain a pull
request ('release pull request') which proposes version bumps in your code and
appends release notes to your `CHANGELOG.md`. **Releases are not created until
after this 'release pull request' is merged to your release branch.**" The human
gate is the PR review itself; the merge is the trigger. Accumulation is native —
`docs/design.md:23-24`: "Maintainers can merge additional commits and
release-please will update the existing release pull request." And the tool stops
at the release: `docs/design.md:63`: "**Note**: `release-please` is not responsible
for publishing your package or application." DropTerm's gates are the same
separation with the PR replaced by an interactive review: the Notes gate *is* the
release-PR review, the Publish gate *is* the merge.

**The manifest and the load-bearing PR.** State lives in two source-controlled
files plus GitHub labels — `docs/manifest-releaser.md:3-5`: "release-please can be
setup to use source-controlled files containing releaser-specific configuration
(the `release-please-config.json`) as well as package version tracking (the
`.release-please-manifest.json`)". Because "release-please operates without a
database of information and so it relies on GitHub as the source of its
information. Due to this, the release pull request is heavily formatted and its
structure is load-bearing." (`docs/design.md:210-212`). The label state machine is
`autorelease: pending` on creation (`design.md:75`) → `autorelease: tagged` after
the GitHub release is cut (`design.md:106`; `README.md:38-39` repeats both).

**Resume after partial failure is a documented human procedure.** A stuck
`autorelease: pending` label blocks all new release PRs — `README.md:153-161`: if
the previous release half-finished, "release-please thinks that the previous
release is still pending. If you're certain that there's no pending release, remove
the `autorelease: pending` ... label and re-run Release Please" (force-run via the
`release-please:force-run` label, `README.md:174-176`; the closed-PR variant is
`docs/troubleshooting.md:112`). DropTerm's Rules #2 ("Resuming mid-release means
first establishing which gates already passed") is the same idea executed against
git state instead of GitHub labels — and DropTerm's is stronger because each leg's
state is checkable by command (`git ls-remote`, the manifest grep).

**Version derivation and human overrides.** `docs/design.md:153-157`: "a breaking
change will increment the semver major version / a `feat` change will increment the
semver minor version / a `fix` change will increment the semver patch version."
Releasable units are exactly the conventional prefixes — `README.md:146-148`: "A
releasable unit is a commit to the branch with one of the following prefixes:
'feat', 'fix', and 'deps'. (A 'chore' or 'build' commit is not a releasable
unit.)" — matching DropTerm Phase 1's "release-tooling commits ... are invisible to
the number". The maintainer override is a commit footer, `README.md:106`: "When a
commit to the main branch has `Release-As: x.x.x` (case insensitive) in the
**commit body**, Release Please will open a new pull request for the specified
version." Release-note fixes have an escape hatch too — the `BEGIN_COMMIT_OVERRIDE`
block in a merged PR body (`README.md:122-132`). DropTerm's Phase 1 gate ("ask the
maintainer to confirm or override. An override skips the derivation, never the
gate") is the interactive equivalent of `Release-As`, and its "revise until
approved" Notes gate is the equivalent of editing the release PR before merge.

### 3. changesets: capture the intent when the change is fresh; version by the maximum bump; review between version and publish

changesets @ `b09779a3d503a581df98a81384d5849db969eb03` (cloned 2026-11-01; the
repo docs carry a warning that the canonical version is changesets.dev — the
concepts below are the repo's own text).

**The rationale is the timestamp of capture, not the machinery.**
`docs/detailed-explanation.md:10-11`: "The best time to capture this information is
when submitting a PR (when it is fresh in your mind), not when you eventually go to
batch and release these changes." A changeset is "an 'intent to change'"
(`:18`) — a markdown file in `.changeset/` whose YAML front matter carries the bump
level and whose body is the future changelog entry. This "breaks versioning into
two steps: 1. Adding a changeset - can be done in a PR, by a contributor, while the
change is fresh in their mind. 2. Versioning - combines all changesets, creates one
version bump for each package based on the **maximum** version bump of each
package" (`:52-56`) — the max-bump rule being the changesets counterpart of
DropTerm's "any user-facing `feat:` → minor".

**Human review sits between versioning and publishing.** `docs/intro-to-using-changesets.md:60-62`:
"`changeset version` ... consumes all changesets, and updates to the most
appropriate semver version ... We recommend at this step reviewing both the
changelog entries and the version changes for packages. Once you are confident that
these are correct, and have made any necessary tweaks to changelogs, you can
publish." The version PR automation keeps that review PR-shaped —
`docs/automating-changesets.md:44-45`: the action "creates a `version` PR, then
keeps it up to date, recreating it when merged. This PR always has an up-to-date
run of `changeset version`". Publishing is idempotent (`intro:73-74`: publish runs
"npm publish in each package that is of a later version than the one currently
listed on npm"), a missing-changeset PR can be gated in CI (`changeset status
--since=main` "will exit with exit code 1 if there are changed packages but no new
changesets", `automating-changesets.md:31-33`), and release-noop merges carry
`changeset --empty` (`:36`). For pre-release channels there are snapshot versions
`0.0.0-{tag}-DATETIMESTAMP` (`docs/snapshot-releases.md:6`).

**The gap for DropTerm:** changesets requires npm packaging —
`docs/versioning-apps.md:8`: "The only requirement is that the project has a
package.json file to manage the versions and dependencies within the repo." Not
adaptable as tooling; adaptable as one idea (finding 8).

### 4. AI-agent release skills: none exist in the primary skill repos — DropTerm's gated design is novel, and cargo-release is its closest validation

- **anthropics/skills** @ `ca45fc496a3436251f191354091d557da31c15b5` (cloned
  2026-11-01): 19 skills (`skills/`: academy-guide, algorithmic-art,
  brand-guidelines, canvas-design, claude-api, discernment-nudge, doc-coauthoring,
  docx, frontend-design, internal-comms, mcp-builder, pdf, pptx, skill-creator,
  slack-gif-creator, theme-factory, webapp-testing, web-artifacts-builder, xlsx).
  **No release, shipping, versioning, or deployment skill exists.** Stated plainly
  because the research question asks: beyond document-skill and testing examples,
  Anthropic's repo offers nothing to copy for release orchestration.
- **addyosmani/agent-skills** @ `8bfcc1f9916f2eea55494d8c1fa7f4bde867dd9e` (cloned
  2026-11-01; this is the pack DropTerm has installed under `.agents/` — diffed:
  the local copy differs only cosmetically, e.g. the upstream's improved secrets
  scan and React class syntax). 25 skills, identical names. The two adjacent to
  releasing are:
  - `shipping-and-launch` — a generic web-deploy pre-launch checklist ("All tests
    pass", "Changelog updated", feature flags, staged rollout percentages, error
    budgets, rollback plan). Its release-relevant line is the post-launch verify
    loop (`SKILL.md:226-236`): "In the first hour after launch: ... Test the
    critical user flow manually ... Confirm rollback mechanism works". Nothing
    about cutting a version, tagging, or gates.
  - `git-workflow-and-versioning` — owns the semver ladder
    (`SKILL.md:274-284`: "MAJOR breaking change — consumers must change their code
    to upgrade / MINOR new functionality, backward-compatible / PATCH bug fix,
    backward-compatible", plus "When unsure whether a change is breaking, assume it
    is") and the tag discipline (`:286-293`: "Tag the release, and let the tag be
    the source of truth"). This is the skill `release-orchestration` already
    inherits its vocabulary from.
  - `/ship` (`.claude/commands/ship.md`) is a **review** orchestrator, not a
    releaser: "Run the pre-launch checklist via parallel fan-out to specialist
    personas, then synthesize a go/no-go decision" — it ends at "Ship Decision:
    GO | NO-GO", never at a tag.

**Conclusion of this finding:** no primary source in the AI-skills ecosystem
encodes an agent-cut release with human gates, never-push rules, or
verify-don't-trust remote checks. Those constructs have exactly one mature analog
in the survey — cargo-release's per-step `[y/N]` confirmations over a dry-run
default (finding 1) — which is a CLI tool talking to a human, not an agent talking
to a maintainer. DropTerm's skill is precedent-setting here; the survey validates
its individual mechanisms (gates, default-safe, verify-after-act) but cannot be
cited for the agent-level composition.

### 5. COSMIC peers: everything DropTerm already exceeds, plus three conventions it lacks (pre-bumped dev version, FlakeHub rolling, debian lockstep)

Cloned 2026-11-01. All three peers share the epoch model verified in
[packaging-and-release](./packaging-and-release.md) finding 3.2, still current:

| Repo | Commit | Cargo version | Latest tag | Workflows |
|---|---|---|---|---|
| pop-os/cosmic-term | `8a05e91` (2026-10-27) | `1.11.0` | `epoch-1.9.0` | rust-checks, validate-desktop-files, require-checklist, flakehub-publish-rolling |
| pop-os/cosmic-files | `09bdd82` (2026-10-09) | `1.11.0` | `epoch-1.9.0` | ci, require-checklist, validate-desktop-files |
| pop-os/cosmic-edit | `ec8375c` (2026-10-27) | `1.11.0` | `epoch-1.9.0` | ci, require-checklist, validate-desktop-files, flakehub-publish-rolling |

- **No release automation anywhere.** No release workflow in any of the three
  repos; releases are the `epoch-X.Y.0` annotated tag + `debian/` lockstep. cosmic-term's
  `debian/changelog` head: `cosmic-term (1.11.0) noble; urgency=medium ... * Epoch
  1.11.0 version update -- Michael Murphy ... Tue, 20 Oct 2026` — Cargo.toml,
  tag, and changelog move together, with the Cargo version already at 1.11.0 while
  the newest tag is epoch-1.9.0.
- **Master carries the next version before it is released.** The Cargo version is
  pre-bumped (1.11.0 on master, last released epoch-1.9.0) — the pop-os equivalent
  of alacritty's `-dev` version (finding 6). DropTerm bumps at release time in the
  release commit instead; both are coherent, and DropTerm's choice keeps
  `Cargo.toml == latest tag` true at every commit that is not a release.
- **No appstream release notes.** cosmic-term's
  `res/com.system76.CosmicTerm.metainfo.xml` contains **zero** `<release>` elements
  (verified: `grep -c "<release"` → 0) — the deb pipeline never reads them.
  DropTerm's dated `<release>` entries in both metainfo files are already beyond
  every peer.
- **The one thing peers have that DropTerm does not:** a rolling nix channel —
  `flakehub-publish-rolling.yml`: `name: "Publish every Git push to master to
  FlakeHub"`, on push to master, `rolling: true`. DropTerm has no nix story at all
  yet (deferred by packaging-and-release's recommendation 5), so this is a
  downstream-ecosystem option, not a skill change.

**What do they do that DropTerm doesn't? Nothing in the release-orchestration
skill's scope.** The peers' process is a human runbook DropTerm automated past.

### 6. Terminal peers: alacritty documents a 14-step RC process and a changelog-voice rule; wezterm names releases by date and creates releases idempotently

**alacritty** @ `bbb2cbf` (2026-10-26):

- **The changelog-voice rule** — `CONTRIBUTING.md:96-97`: "The change log should be
  used to document changes from a user-perspective, instead of explaining the
  technical background (like commit messages)". This is DropTerm's "the prose is
  compression, not authorship" rule with sharper wording; DropTerm's Phase 2 source
  hierarchy (commits → docs pages → prior sections) already implements it.
- **Section order** — `CHANGELOG.md:3`: "The sections should follow the order
  `Packaging`, `Added`, `Changed`, `Fixed` and `Removed`" on a Keep a Changelog
  base. DropTerm's house style (one framing paragraph + doc-linked bullets) is a
  deliberate departure — tighter than category buckets for an app whose docs carry
  the detail.
- **The release process is an RC pipeline** — `CONTRIBUTING.md:112-117`: "a new
  branch is created for every major release. Both the release candidates and the
  final version are only committed and tagged in this branch. The master branch
  only tracks development versions"; the 14-step sequence at `:119-139` runs
  `0.2.0-dev` → `v0.2` branch → `0.2.0-rc1` tag → crates.io + GitHub release →
  cherry-pick fixes → repeat RCs → final `v0.2.0`. Master versioning rule at
  `:142-144`: "On master and with new planned releases, only the minor version is
  bumped."
- **Tests run inside the release pipeline** — `.github/workflows/release.yml`
  triggers on `tags: ["v[0-9]+.[0-9]+.[0-9]+*"]` and **every** matrix job (macOS,
  Windows, Linux) runs a `Test` step (`cargo test --release`, e.g. the macOS job's
  `- name: Test`) *before* building and uploading. DropTerm's release.yml builds
  and vendors but never runs the test suite — tests run in the Phase 3 local `just
  check`, then CI re-builds without re-testing; alacritty is the surveyed
  precedent for closing that gap.

**wezterm** @ `e68a8ec` (2026-10-26):

- **Versioning philosophy** — `docs/changelog.md:10`: "Releases are named using the
  date, time and git commit hash", with a nightly channel: `:13-15`: "A bleeding
  edge build is produced continually (as commits are made, and at least a daily
  scheduled build) from the `main` branch. It *may* not be usable ... since @wez
  uses this as a daily driver, its usually the best available version."
- **Changelog accumulation** — `:17-18`: "As features stabilize some brief notes
  about them will accumulate here" — the changesets capture-early model
  implemented in a single markdown file, newest notes accumulating under a
  Continuous/Nightly heading until a release cuts them into a section.
- **Idempotent release creation** — `ci/create-release.sh:12`:
  `gh release view "$name" || gh release create --prerelease --notes "$notes"
  --title "$name" "$name"` — check-first, create-only-if-absent. DropTerm's
  Phase 4 verify (`git ls-remote` for the tag, then `gh release view` polling) is
  the same instinct; release.yml's tag-triggered `gh release create` cannot
  double-fire on one tag, so the pattern is already effectively present.

### 7. The specs: Conventional Commits and SemVer — what Phase 1's derivation can lean on, and the 0.x caveat

**Conventional Commits v1.0.0** (conventionalcommits.org/en/v1.0.0/, fetched
2026-11-01):

- The translation rule, verbatim: "fix type commits should be translated to PATCH
  releases. feat type commits should be translated to MINOR releases." Breaking
  changes are the third input: "Breaking changes MUST be indicated in the
  type/scope prefix of a commit, or as an entry in the footer" — `!` before the
  colon, or a `BREAKING CHANGE:` footer — and map to MAJOR.
- Non-feat/fix types are explicitly inert: "Types other than feat and fix MAY be
  used in your commit messages" and "Additional types are not mandated by the
  Conventional Commits specification, and have no implicit effect in Semantic
  Versioning." This is spec backing for DropTerm's "release-tooling commits
  (recipes, CI, manifest pins) are invisible to the number" — but note DropTerm's
  current rule keys on *what the commit is* (tooling), which is stricter and
  better than keying on the prefix (`chore:`), since a user-facing `fix:` in a
  recipe file would still count as a fix under the repo's rule.
- **The spec is silent on 0.x.** It only ties types to the three bump levels
  "generally"; pre-1.0 handling is left to tooling.

**SemVer 2.0.0** (semver.org, fetched 2026-11-01):

- 0.x is defined by exemption: "Major version zero (0.y.z) is for initial
  development. Anything MAY change at any time. The public API SHOULD NOT be
  considered stable." and "Version 1.0.0 defines the public API."
- Every increment MUST is conditioned on `x > 0`: "Patch version Z (x.y.Z | x > 0)
  MUST be incremented if only backward compatible bug fixes are introduced.";
  "Minor version Y (x.Y.z | x > 0) MUST be incremented if new, backward compatible
  functionality is introduced to the public API."; "Major version X (X.y.z | X >
  0) MUST be incremented if any backward incompatible changes are introduced to
  the public API."
- The FAQ supplies the convention DropTerm follows: "The simplest thing to do is
  start your initial development release at 0.1.0 and then increment the minor
  version for each subsequent release."

**Validation of DropTerm's Phase 1:** "fixes only → patch; any user-facing `feat:`
→ minor; at 0.x a breaking change still takes the minor" is exactly
Conventional-Commits translation plus the SemVer FAQ 0.x convention, and it is
*not* in tension with the spec because the spec's MUSTs do not bind at 0.y.z. Two
tightenings the specs support: (a) the derivation should also scan for `!` /
`BREAKING CHANGE` markers so "name the break at the gate" triggers on evidence
rather than memory; (b) "fixes only" is better stated as "no `feat:`/`fix:` commit
with user-visible effect" — cargo-release's warn_changed covers the empty case
(finding 1). At 1.0.0 the same derivation flips to spec-mandatory behavior with no
wording change: breaking → major becomes a MUST.

## What DropTerm has today

Read from the skill (`.agents/skills/release-orchestration/SKILL.md`), the justfile,
`.github/workflows/release.yml`, and `docs/development.md#releasing` at HEAD
(2026-11-01):

- **Five gates** (Version, Notes, Publish, Pin push, Catalog), each "passed only by
  the maintainer's explicit answer to a direct question ... Silence, ambiguity, or
  'probably' is not consent" — and the fixed division of labor: "the agent curates
  and runs recipes; the maintainer pushes."
- **Phase 1 derivation** — `git log <tag>..HEAD` scan; fixes→patch, user-facing
  `feat:`→minor, 0.x breaking→minor-but-named; tooling commits invisible;
  maintainer confirm-or-override ("An override skips the derivation, never the
  gate").
- **Phase 2 curation** — the five-file draft (releases.md section, both metainfo
  `<release>` entries, README Status/URLs, testing.md rows, ROADMAP retirement)
  presented in full before any write; "Never invent release notes."
- **Phase 3** — `just release X.Y.Z`, whose recipe guards (justfile lines 555-649)
  already implement cargo-release's pre-flight in DropTerm-specific form: version
  regex, `main`-branch check, tag-absence check, only-release-files-in-flight
  allowlist, dated-section greps, stale-URL grep, sed-bump-with-readback, lock
  refresh + `cargo metadata --locked`, `just check` + appstreamcli, one hygiene
  commit, annotated tag, never pushes.
- **Phase 4/5/6** — hand-over-the-command push + "verify, don't trust" remote
  checks; `release-flatpak-pin` with exactly-once grep-count guards;
  `release-catalog` re-cutting the fork branch and printing the compare URL.
- **release.yml** — tag push → vendor.tar + offline proof, per-arch builds,
  tag↔Cargo.toml guard, sha256sums, `gh release create`; plus a
  `workflow_dispatch` dry-run input ("Assemble the full artifact set without
  publishing a release").
- **Skill rules already matching the survey's best practice:** never pushes; no
  gate skipped; recipe failure = stop + verbatim output; silent on success;
  five-item completion verification (tag on origin, release page with artifacts,
  manifest pinned and pushed, catalog branch at the printed SHA, clean tree).

Gaps the survey exposes, none currently covered: no behind-remote check before
cutting; no nothing-changed-since-tag warning; no `!`/`BREAKING CHANGE` scan in
Phase 1; release CI builds but does not test; the `workflow_dispatch` dry-run leg
exists but the skill never mentions it as a rehearsal option; releases.md is
explicitly release-time-only ("A section is added here at release time, by the
release commit; nothing in between", `docs/releases.md:5`), so there is no
accumulator between releases.

## Recommendation

Graded by the precedent strength of the source:

1. **Add a behind-remote pre-flight to Phase 1/3 — STRONG precedent (cargo-release
   `verify_if_behind` runs before its first confirmation).** One check before
   Phase 3's recipe: `git fetch origin main` and require
   `git rev-parse main..origin/main` to be empty (cargo-release's own help text
   prescribes `git pull --rebase` when it fails). This is the only finding where a
   surveyed tool guards against a failure mode DropTerm's recipe genuinely lacks.
2. **Add a no-op-release warning to Phase 1 — STRONG precedent (cargo-release
   `warn_changed`: "updating X to Y despite no changes made since tag Z").** If
   `git log <tag>..HEAD` contains no user-facing commits at all, say so at the
   Version gate before proposing a number. Cheap, and it makes the
   "tooling-invisible" rule self-checking.
3. **Tighten Phase 1's derivation wording to the specs and scan for break markers —
   STRONG precedent (Conventional Commits v1.0.0 translation rule; SemVer FAQ 0.x
   convention).** State the mapping as the spec does (feat→minor, fix→patch,
   other types inert), and let the Phase 1 commit scan surface `!` /
   `BREAKING CHANGE` so "name the break at the gate" fires on evidence. Document
   that at 1.0.0 the same rule becomes spec-mandatory unchanged.
4. **Run tests in release CI — MEDIUM precedent (alacritty's release.yml runs
   `cargo test --release` in every matrix job before uploading).** DropTerm's
   build job could run `just test --locked` (or the cheaper
   `cargo test --workspace --locked`) once before staging, closing the gap
   between the local `just check` and the artifacts CI publishes.
5. **Make the skill name the existing dry-run rehearsal — MEDIUM precedent
   (cargo-release's dry-run default; release.yml already has the
   `workflow_dispatch` dry-run input).** Before the Phase 4 push, offer the
   maintainer the dispatch run as a final full-pipeline rehearsal; it assembles
   artifacts and proves the offline build without publishing. Zero new machinery.
6. **Optionally adopt a "Next (unreleased)" accumulator in docs/releases.md —
   MEDIUM precedent (changesets' capture-when-fresh rationale; wezterm's
   accumulating changelog; release-please's evergreen release PR).** DropTerm's
   skill already consumes such a pointer if present; making it the norm between
   releases moves curation from recall-at-release-time to capture-at-change-time.
   This is the one changesets idea worth having without any of its tooling.

**Rejected alternatives, and why:**

- **Adopting cargo-release as the release engine** — wrong shape. It is
  crates.io-publishing-first (`cargo publish` is STEP 3, ahead of tag and push),
  its changelog support is "unopinionated" (recommends external git-cliff), and its
  `pre-release-replacements` would have to re-implement guards the justfile
  recipes already encode against DropTerm's actual five-file curation. Its value
  here is the semantics audit (finding 1), not the tool.
- **Adopting release-please (bot or action)** — solves PR-review-shaped release
  prep for teams; it needs its manifest files, its label state machine, and (for
  notes overrides) squash-merge history, and it would take the curation away from
  the Notes gate, which reviews prose release-please cannot draft to DropTerm's
  house style. DropTerm's gate already provides the same prepare/publish
  separation with less state.
- **Adopting changesets** — npm-only by requirement ("The only requirement is that
  the project has a package.json file", versioning-apps.md). Non-starter for a
  Rust workspace; only the capture-early idea transfers (recommendation 6).
- **An alacritty-style RC branch pipeline (`-rc1` tags, `v0.x` release branches,
  `-dev` master versions)** — a testing cadence for a large user base and a
  crates.io-published terminal crate; DropTerm's releases are cheap (three recipes
  and pushes) and its hardware-test reality (docs/testing.md) means an RC channel
  would not get COSMIC-hardware coverage a stable cut doesn't already get.
- **A wezterm-style nightly/rolling channel** — wezterm's nightly works because
  the maintainer dogfoods it daily; DropTerm's `-dev` identity launcher already
  serves the dogfooding role locally, and a nightly tag stream would fight the
  flatpak pin (which pins exactly one published archive). Revisit only alongside a
  nix/FlakeHub story if ever.
- **`-dev`-suffixed version-on-master (pop-os/alacritty convention)** — would
  break DropTerm's own invariants: the tag↔Cargo.toml guard in release.yml and
  the recipe's "Cargo.toml already reads X" check both assume master carries the
  released version between releases.

## Adaptable to DropTerm, ranked (value × precedent / effort)

1. **Behind-remote pre-flight before Phase 3** — high value, tiny effort
   (one fetch + one rev-range check, message with the `git pull --rebase` hint).
   cargo-release precedent.
2. **No-op-release warning in Phase 1** — medium value, tiny effort (one more
   question of the commit list Phase 1 already gathers). cargo-release precedent.
3. **Spec-exact derivation wording + `!`/`BREAKING CHANGE` scan in Phase 1** —
   medium value, small effort (wording + one grep pattern in the commit
   presentation). Conventional Commits + SemVer precedent; future-proofs 1.0.0.
4. **Tests in release CI** — medium value, small effort (one step in the build
   job). alacritty precedent.
5. **Name the `workflow_dispatch` dry-run as the pre-push rehearsal option** —
   medium value, tiny effort (one sentence in Phase 4). cargo-release
   dry-run-default precedent; machinery already exists.
6. **"Next (unreleased)" accumulator as the standing norm** — medium value,
   small ongoing effort (one entry per merged feature, consumed at release).
   changesets/wezterm/release-please precedent; opt-in, since releases.md's
   current release-time-only rule is also defensible.
7. **FlakeHub rolling channel** — low value today, medium effort (needs a flake
   first; packaging-and-release already defers nix). pop-os precedent; park it
   with the nix story.
8. **RC/nightly channels, release-please, changesets, cargo-release adoption** —
   rejected (see above); listed only so the rejection is on record.

## Sources

All repositories cloned 2026-11-01 at the commits named; web specs fetched
2026-11-01. Line numbers are into the files at those commits.

- crate-ci/cargo-release @ `5f271e4beb11b59f6305fbce1df7f9033ec27042`:
  `docs/reference.md:12-27` (steps incl. `changes` — "Print commits since last
  tag"), `:36-37` (`--execute` / `--no-confirm`), `:165-173` (allow-branch),
  `:234-259` (pre-release-replacements, min/max/exactly), `:260-281`
  (pre-release-hook abort + env), `:414-420` (verify=true "Verify the contents by
  building them"); `README.md:50-61` (dry-run by default); `docs/faq.md:29-54`
  (changelog unopinionated, git-cliff hook);
  `src/steps/mod.rs:19-38` (verify_git_is_clean), `:111-189` (verify_git_branch,
  verify_if_behind + `git pull --rebase` help at :173), `:348-395` (warn_changed,
  "despite no changes" at :374), `:451-456` (confirm gates on `!dry_run &&
  !no_confirm`);
  `src/steps/release.rs:195` (STEP 0), `:269-336` (STEP 1 confirmation → STEP 2
  version/commit → STEP 3 publish → owner → STEP 5 tag → STEP 6 push);
  `src/ops/shell.rs:9-18` (`[y/N]`, `== "y"`); `src/steps/hook.rs:209-216`
  (DRY_RUN env, hooks run in dry-run) —
  <https://github.com/crate-ci/cargo-release>
- googleapis/release-please @ `c589293ca38aee267b329f9bf9b26c9d6516a687`:
  `docs/design.md:18-24` (release PR, merge-gates-release, accumulation),
  `:63` (not responsible for publishing), `:74-75,100-106` (autorelease labels),
  `:133-157` (DefaultVersioningStrategy), `:210-212` (GitHub as source of truth,
  load-bearing PR), `:243-255` (manifest files);
  `docs/manifest-releaser.md:3-5` (`.release-please-manifest.json` version
  tracking); `README.md:38-39` (label states), `:106-115` (`Release-As`),
  `:122-132` (BEGIN_COMMIT_OVERRIDE), `:142-176` (releasable units feat/fix/deps;
  "A 'chore' or 'build' commit is not a releasable unit"; stuck-label resume);
  `docs/troubleshooting.md:112` (closed-PR label fix);
  `src/strategies/rust.ts` (Cargo.toml/Cargo.lock updaters) —
  <https://github.com/googleapis/release-please>
- changesets/changesets @ `b09779a3d503a581df98a81384d5849db969eb03`:
  `docs/detailed-explanation.md:10-11` (capture at PR time), `:18` ("intent to
  change"), `:52-56` (two steps, maximum-bump rule);
  `docs/intro-to-using-changesets.md:49-74` (version → review → publish,
  idempotent publish); `docs/automating-changesets.md:24-45` (changeset status
  gate, `--empty`, version PR); `docs/snapshot-releases.md:6`;
  `docs/versioning-apps.md:8` (package.json requirement) —
  <https://github.com/changesets/changesets>
- anthropics/skills @ `ca45fc496a3436251f191354091d557da31c15b5`: `skills/`
  listing (19 skills; none release/shipping/versioning), `README.md` —
  <https://github.com/anthropics/skills>
- addyosmani/agent-skills @ `8bfcc1f9916f2eea55494d8c1fa7f4bde867dd9e`:
  `skills/` listing (25 skills); `skills/shipping-and-launch/SKILL.md`
  (pre-launch checklist, post-launch verification `:226-236`);
  `skills/git-workflow-and-versioning/SKILL.md:274-293` (semver ladder, "the
  number is a promise", tag-the-release); `.claude/commands/ship.md` (go/no-go
  fan-out, not a releaser) — <https://github.com/addyosmani/agent-skills>
- pop-os/cosmic-term @ `8a05e91` (2026-10-27): `.github/workflows/` (no release
  workflow), `Cargo.toml:3` (version = "1.11.0"), `debian/changelog` head
  (1.11.0 / "Epoch 1.11.0 version update", 2026-10-20),
  `res/com.system76.CosmicTerm.metainfo.xml` (zero `<release>` elements),
  `.github/workflows/flakehub-publish-rolling.yml` ("Publish every Git push to
  master to FlakeHub", `rolling: true`); tags via `git ls-remote` (latest
  `epoch-1.9.0`) — <https://github.com/pop-os/cosmic-term>
- pop-os/cosmic-files @ `09bdd82` (2026-10-09): same shape verified (Cargo 1.11.0,
  epoch-1.9.0, debian/, no release workflow) — <https://github.com/pop-os/cosmic-files>
- pop-os/cosmic-edit @ `ec8375c` (2026-10-27): same shape verified —
  <https://github.com/pop-os/cosmic-edit>
- alacritty/alacritty @ `bbb2cbf` (2026-10-26): `CONTRIBUTING.md:92-98`
  (changelog user-perspective rule), `:112-144` (RC release process, `-dev`
  versions, minor-only on master); `CHANGELOG.md:1-7` (section order, Keep a
  Changelog, rolling `0.18.0-dev` section); `.github/workflows/release.yml`
  (tag-regex trigger, `Test` step before every matrix build/upload) —
  <https://github.com/alacritty/alacritty>
- wez/wezterm @ `e68a8ec` (2026-10-26): `docs/changelog.md:10-18` (date/time/hash
  release names, nightly channel, accumulating notes);
  `ci/create-release.sh:12` (`gh release view || gh release create --prerelease`) —
  <https://github.com/wez/wezterm>
- Conventional Commits v1.0.0 (fetched 2026-11-01): type→bump translation
  ("fix ... PATCH ... feat ... MINOR"), `!`/`BREAKING CHANGE` footer rules, "no
  implicit effect in Semantic Versioning" for other types —
  <https://www.conventionalcommits.org/en/v1.0.0/>
- SemVer 2.0.0 (fetched 2026-11-01): "Major version zero (0.y.z) is for initial
  development. Anything MAY change at any time"; the `x > 0`-conditioned MUSTs;
  FAQ 0.x minor-increment convention — <https://semver.org/>
- DropTerm itself @ HEAD (2026-11-01): `.agents/skills/release-orchestration/SKILL.md`;
  `justfile` (`release` :555-649, `release-flatpak-pin` :657-682, `release-catalog`
  :706-742); `.github/workflows/release.yml` (tag guard, dispatch dry-run, no test
  step); `docs/development.md#releasing` (:124-160); `docs/releases.md:5`
  ("A section is added here at release time ... nothing in between")
