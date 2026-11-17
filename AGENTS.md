# DropTerm — Agent Skills

This project uses the [addyosmani/agent-skills](https://github.com/addyosmani/agent-skills) pack, installed under `.agents/` and `.opencode/`. Reusable workflows (skills), shared checklists (references), lifecycle slash commands, and specialist personas are all wired up and auto-discovered.

## Locations

| Asset | Path |
|-------|------|
| Skills | `.agents/skills/<name>/SKILL.md` |
| Shared checklists | `.agents/references/*.md` |
| Slash commands | `.opencode/commands/*.md`, mirrored to `.zcode/commands/*.md` for ZCode |
| Personas (subagents) | `.opencode/agents/*.md` |

## Core rules

- If a task matches a skill, load it with the `skill` tool before acting.
- Follow the skill workflow strictly — do not partially apply it.
- Never skip required steps (spec, plan, test) a skill demands.
- Every change clears the project-wide Definition of Done in `.agents/references/definition-of-done.md`.
- Commands are silent on success and announce on failure: justfile recipes echo nothing on success (see `set quiet` in the justfile) and fail loudly; slash commands give a concise result on success and surface failures prominently instead of burying them.

## Intent → skill mapping

- New feature / significant change → `spec-driven-development`, then `incremental-implementation` + `test-driven-development`
- Planning / breakdown → `planning-and-task-breakdown`
- Bug / failure / unexpected behavior → `debugging-and-error-recovery`
- Code review → `code-review-and-quality`
- Refactoring / simplification → `code-simplification`
- API or module boundary design → `api-and-interface-design`
- Rust code / review → `rust-best-practices`
- Security-sensitive input, auth, secrets, dependencies → `security-and-hardening`
- Performance requirements or suspected regressions → `performance-optimization`
- Cutting a release / publishing a version → `dropterm-release` (or `/release`) — the project's release ritual; it supersedes the pack's generic `release-orchestration` here
- Runtime verification on a live session (any platform) — `docs/testing.md` rows, `just vm-*` validation-VM walks, portability sessions — → manual, owner-run (owner ruling, 2026-11-14): the agent never runs or dispatches these; at each walk/check it hands the owner one concise instruction — the rows to walk, the build to install, the harness and snapshot, the evidence each clause needs — and records only the owner's reported result
- Making a change (always) → `git-workflow-and-versioning`

## Slash commands

`/spec`, `/plan`, `/build [auto]`, `/test`, `/review`, `/constraints`, `/code-simplify`, `/ship`, `/release [x.y.z]`, `/webperf`, `/roadmap`. Run `/ship` before merge for a parallel fan-out review; `/roadmap` rebuilds the portability status board fresh from `tasks/todo.md` + `SPEC.md` (interactive in ZCode Desktop, a table elsewhere) — it never serves a cached snapshot. `/release` drives the whole release — version proposal, notes curation, recipes, gated pushes — and never pushes for you (see `dropterm-release`). In ZCode the planning command is `/planning` — `/plan` is a reserved built-in there, so its mirror is renamed to `planning.md`; opencode keeps `/plan`.

## Personas

`code-reviewer`, `security-auditor`, `test-engineer`, `web-performance-auditor` — single-role subagents invoked by `/ship` / `/webperf` (or directly). **Personas do not invoke other personas**; orchestration belongs to slash commands and the main session (see `.agents/references/orchestration-patterns.md`).

## Docs

- A shipped feature ships its user doc: a `docs/` page or section, linked from README's Documentation index.
- A feature with a research record gets (or updates) its row in `docs/research/README.md` — status lives in the index; the records themselves are never edited after the fact.
- Two gates hold the docs to the code: `cargo test` pins every serialized `DropTermConfig` key to `docs/configuration.md` (`every_config_key_is_documented`), and `just docs-check` (part of `just check`) verifies every internal markdown link resolves.
- Committed docs stay self-contained: the `SPEC*.md`/`tasks/` set is in-tree since 2026-11-12 and may be linked from docs, but no docs page depends on it for its meaning.

## Rust project notes

- Run every gate through the just recipes, never raw cargo: `just check` (fmt-check, clippy, test, docs-check) before a change counts as done; `just test` / `just clippy` / `just fmt` for a single gate. The recipes run cargo under the justfile's `quietly` wrapper — output is captured and printed only on failure — so a green run is silent. Raw `cargo test` / `cargo clippy` prints on success and is never invoked directly.
- Unit tests live inline in `#[cfg(test)] mod tests`. When a test module outgrows ~500 lines, move it to a sibling `tests.rs` (e.g. `src/app/tests.rs`) declared as `#[cfg(test)] mod tests;` — it stays a child module, keeping private access and cfg gating while keeping whole-file reads cheap for agents. Never move unit tests to a top-level `tests/` dir; that is integration-test territory and loses private access.
- This is a COSMIC Wayland app: runtime behavior is only testable on real COSMIC hardware (see the manual test checklist in `docs/testing.md`); unit tests cover what can be tested headless. Runtime rows are walked manually by the owner (validation VM or platform sessions per `docs/research/portability-plan.md`; owner ruling, 2026-11-14): the agent's whole part is one concise instruction per walk — rows to walk, build to install, harness and snapshot, evidence to capture — and recording the owner's reported result; it never runs `just vm-*` or a platform walk itself and never marks a runtime row green.

## Spec & task artifacts (committed on main)

- `SPEC.md` is the master index: the capability map (module ids, dependency
  direction, build order) and per-module status. Per-module specs live at
  `SPEC-<module-id>.md`; per-module task chunks at `tasks/<module-id>.md`.
  Global aggregates: `tasks/plan.md` and `tasks/todo.md`.
- The set is committed (owner ruling, 2026-11-12; it was gitignored local
  state before, with per-worktree seeded copies). Every change to it is a
  commit, and worktrees see the whole set through git — branch, rebase,
  merge — with no seeding or hand-syncing between checkouts.
- Module ids come from the capability map in `SPEC.md`, are chosen once, and
  are never renamed mid-initiative.
- Before implementing: read `SPEC.md` for orientation, then load the
  module's `SPEC-<id>.md` and `tasks/<id>.md` chunk. Never implement a
  module whose spec is not approved or whose chunk is not written.
- `/build` consumes the chunk files; see `.opencode/commands/build.md` (mirrored at `.zcode/commands/build.md`).
- PRs may link the module's `SPEC-<id>.md` and chunk directly — reviewers
  see them in-tree; copying success criteria into the PR body is welcome
  but no longer required.
- Worktrees: a lane edits only its own modules' spec and chunk (plus the
  shared `SPEC.md` status lines and `tasks/todo.md` rows); status crosses
  worktrees through git. The committed plan
  (`docs/research/portability-plan.md`) remains the durable record of
  merge Status, updated on main.


## File creation

Default to fewer, larger, cohesive files. File boundaries should follow
what changes together, not how much there is.

Extract a new file or subdirectory only when **all** of these are true:

- **The trigger has fired** — during a single feature or task, you keep
  editing unrelated sections of the same file.
- **One concept, one name** — the new unit maps to a concept you can name
  in a word or short phrase (`stringset`, `auth` — never `util`,
  `common`, `misc`), and the name works as a meaningful prefix for
  everything inside it.
- **It stands on its own** — its contents change together and don't need
  what they're being extracted from.

Before extracting, try in-file structure first: grouping related items
together, section comments, or narrowing scope often resolves the
discomfort without a new file.

Length alone is never a reason to split. Size is a lagging indicator —
it confirms a boundary you already noticed, it never creates one. The
typical failure mode: a long file becomes `utils`, `helpers`, and
`types` — same content, names that describe nothing, and every future
edit spans three files. When in doubt, don't extract; a long file is
cheaper to live with than a wrong boundary.

The bias runs in reverse too: sibling files that always change together
are merge candidates.

This guidance assumes module/namespace-oriented languages (Rust, Go,
JavaScript/TypeScript). It does not apply to frameworks that enforce
one-class-per-file patterns.
