# DropTerm — Agent Skills

This project uses the [addyosmani/agent-skills](https://github.com/addyosmani/agent-skills) pack, installed under `.agents/` and `.opencode/`. Reusable workflows (skills), shared checklists (references), lifecycle slash commands, and specialist personas are all wired up and auto-discovered.

## Locations

| Asset | Path |
|-------|------|
| Skills | `.agents/skills/<name>/SKILL.md` |
| Shared checklists | `.agents/references/*.md` |
| Slash commands | `.opencode/commands/*.md` |
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
- Making a change (always) → `git-workflow-and-versioning`

## Slash commands

`/spec`, `/plan`, `/build [auto]`, `/test`, `/review`, `/constraints`, `/code-simplify`, `/ship`, `/webperf`. Run `/ship` before merge for a parallel fan-out review.

## Personas

`code-reviewer`, `security-auditor`, `test-engineer`, `web-performance-auditor` — single-role subagents invoked by `/ship` / `/webperf` (or directly). **Personas do not invoke other personas**; orchestration belongs to slash commands and the main session (see `.agents/references/orchestration-patterns.md`).

## Rust project notes

- Run `cargo clippy` and `cargo fmt --check` before a change counts as done; `cargo test` is the verification gate.
- This is a COSMIC Wayland app: runtime behavior is only testable on real COSMIC hardware (see README's manual test checklist); unit tests cover what can be tested headless.

## Spec & task artifacts (local working docs)

- `SPEC.md` is the master index: the capability map (module ids, dependency
  direction, build order) and per-module status. Per-module specs live at
  `SPEC-<module-id>.md`; per-module task chunks at `tasks/<module-id>.md`.
  Global aggregates: `tasks/plan.md` and `tasks/todo.md`.
- All of the above are local working artifacts, gitignored via `SPEC*.md`
  and `/tasks`. Never commit them.
- Module ids come from the capability map in `SPEC.md`, are chosen once, and
  are never renamed mid-initiative.
- Before implementing: read `SPEC.md` for orientation, then load the
  module's `SPEC-<id>.md` and `tasks/<id>.md` chunk. Never implement a
  module whose spec is not approved or whose chunk is not written.
- `/build` consumes the chunk files; see `.opencode/commands/build.md`.
- PRs must copy the module's success criteria into the PR body, since
  reviewers cannot see the local spec files.
