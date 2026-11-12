# SPEC.md — the portability initiative's master index

Spec set for [the portability plan](docs/research/portability-plan.md), the
committed, canonical evidence base — every worktree has it, so these specs
lean on it rather than duplicating its grounding. This set is itself
committed (owner ruling, 2026-11-12): worktrees see it through git —
branch, rebase, merge — with no seeding or hand-syncing. PRs may link
their module's spec and chunk directly.

## Capability map

Module ids are chosen once and never renamed. The map below is adopted
verbatim from the plan's capability map; the plan's work-package Status
lines are the durable record, this table the working view. Status
vocabulary: `not started` · `in progress (port/<id>)` ·
`merged (<short sha>)` · `blocked: <reason>`; the spike adds
`reported: GO | REDESIGN | NO-GO`.

| Id | Phase | Kind | Depends on | Lane | Shares hot files with | Spec | Status |
|---|---|---|---|---|---|---|---|
| `reality-check` | 0 | manual, no code | none | hardware | none | [SPEC-reality-check.md](SPEC-reality-check.md) | in progress — 3/7 sessions filled (COSMIC/sway/X11; Hyprland+KDE+GNOME+the non-Pop distro session are owner-hardware) |
| `app-identity` | 1 | code, `res/`, CI, docs; mechanical | none — merges first | flag day | every package (mostly docs) | [SPEC-app-identity.md](SPEC-app-identity.md) | merged (f698083) |
| `config-store` | 1 | code | `app-identity`\* | B | `surface-intents` (`src/app.rs`), `control-transport` (`Cargo.toml` features) | [SPEC-config-store.md](SPEC-config-store.md) | merged (f5199ec) |
| `surface-intents` | 1 | refactor + tests | `app-identity`\* | A | `config-store` (`src/app.rs`) | [SPEC-surface-intents.md](SPEC-surface-intents.md) | merged (00cf77d) |
| `control-transport` | 1 | code | `app-identity`\*; after `config-store` in lane B | B | `layer-shell-guard` (`src/main.rs`) | [SPEC-control-transport.md](SPEC-control-transport.md) | merged (e6ad23b) |
| `layer-shell-guard` | 1 | code, small | `app-identity`\* | C | `control-transport` (`src/main.rs`) | [SPEC-layer-shell-guard.md](SPEC-layer-shell-guard.md) | merged (f73cf27) |
| `keybind-docs` | 1 | docs | `app-identity`, `reality-check` | C | none | [SPEC-keybind-docs.md](SPEC-keybind-docs.md) | not started |
| `contract-docs` | 1 | docs | `control-transport` | C | none | [SPEC-contract-docs.md](SPEC-contract-docs.md) | merged (5bf146b) |
| `distro-reach` | 1 | release and packaging | `reality-check`, `app-identity` | C | `.github/workflows/release.yml` | [SPEC-distro-reach.md](SPEC-distro-reach.md) | not started |
| `gpui-spike` | 2 | throwaway crate + report | none | spike | none (own workspace) | [SPEC-gpui-spike.md](SPEC-gpui-spike.md) | in progress — sway-witness green for 0/1/2/6/7 (5422181; show 121ms ≤ 130ms budget; #61368 clear at 1.5, `cx.displays()` mis-sizes fractional outputs); checks 9/10/11 harnesses armed (aeea7b3 — forks resolve under the pin, dbus/detect/grid await the session); COSMIC rows are owner-hardware |
| `kit-pass` | 2 | spike second pass | `gpui-spike`; the Kit ruled "adopt" | spike | none | [SPEC-kit-pass.md](SPEC-kit-pass.md) | not started |

\* A merge-order dependency only: the package may start before
`app-identity` merges and rebases onto it. `app-identity`'s code diff is
small (about twenty identity literals in `src/lib.rs`, `src/dbus_state.rs`,
`applet/src/applet.rs`, and `applet/src/main.rs`, plus the crate names);
most of its 41 files are `res/`, CI, the justfile, and docs.

Phase 3 slices and Phase 4–5 packages get their ids at the spike report /
when each opens (plan rules) — no specs exist for them yet, by design.

**Build order**: `app-identity` merges first and every other package merges
after it; packages may start earlier and rebase, and `config-store` should,
to keep the gap between the two merges short. `reality-check` and `gpui-spike` run
from day one beside everything; Phase 3 follows the spike report.

## Worktree split

The whole set is committed, so every checkout sees all of it through git;
the table maps which lane edits which modules' files (a lane touches its
own specs and chunks plus the shared status lines, nothing else):

| Checkout | Branch | Modules | Spec files it edits |
|---|---|---|---|
| this checkout (main) | lane C, hardware sessions, coordination | `reality-check`, `layer-shell-guard`, `keybind-docs`, `contract-docs`, `distro-reach` | those modules' specs and chunks, plus the shared set |
| `../ced` (worktree, reused) | lane A — complete | `app-identity` (merged f698083), `surface-intents` (merged 00cf77d) | `SPEC-app-identity.md`, `SPEC-surface-intents.md` |
| `../ced2` (worktree, reused) | lane B — `config-store` merged; `control-transport` in flight (T1–T4 done — d4856e5, merge pending) | `config-store` (merged f5199ec), `control-transport` | `SPEC-config-store.md`, `SPEC-control-transport.md` |
| `../dropterm-spike` (scratch dir, own repo) | `spike/gpui-layer-shell` (never merged) | `gpui-spike`, `kit-pass` | `SPEC-gpui-spike.md`, `SPEC-kit-pass.md` |

Editing rules (AGENTS.md): lane status crosses worktrees through git now —
the seeding ritual (per-worktree trimmed `SPEC.md` copies, copied-in
chunks) retired with the gitignore entries on 2026-11-12. Lane C's
packages are small enough for the main checkout (or `../ced` between
lane A's packages); hardware sessions need no worktree.

```sh
git -C ../ced branch -m ced port/app-identity    # reuse: rename, never create
git -C ../ced2 branch -m ced2 port/config-store
git -C ../ced merge --ff-only main
git -C ../ced2 merge --ff-only main
mkdir ../dropterm-spike && git -C ../dropterm-spike init -b spike/gpui-layer-shell
# the next package in a lane, once the previous one merged into main:
git -C ../ced switch -c port/surface-intents main
git branch -d port/app-identity
# the worktrees persist as the lanes' permanent homes; no removal
```

A lane's worktree is reused across its packages, keeping its `target/`
warm (both arrived warm from the pre-portability work). The spike's
scratch dir builds an unrelated dependency tree (GPUI's), so it never
shares a `target/` with the others. Both worktrees persist as the lanes'
permanent homes.

## Conflict hot spots (from the plan)

- `src/app.rs`: `surface-intents` (the surface handlers) and `config-store`
  (the config handle, watcher, and writers) touch different regions;
  whichever merges second rebases.
- `src/main.rs`: `control-transport` and `layer-shell-guard`. Land the small
  guard first, or rebase it after.
- `Cargo.toml`'s `terminal` feature list: `config-store` may drop
  `dbus-config` and `control-transport` drops `single-instance`; both sit in
  lane B, in sequence.
- Docs pages that name the binary or the app id (configuration, CLI, known
  issues, testing): `app-identity` rewrites them, so later packages edit
  them after it merges.

## Release boundary

`app-identity` and `config-store` ship in one release, and no release is
cut between their merges. No break note (owner ruling, 2026-11-12):
pre-distribution — no released version, no users — nothing to break, so
the notes are ordinary release notes. Every other Phase 1 package ships
in whatever release follows its merge. Phase 3's GPUI binary ships
nothing until its cutover slice. Deferred (owner ruling, 2026-11-12):
the release is not cut yet — prep sits on main uncut, building continues,
and the eventual release carries everything merged by then; the walk gate
stands whenever it is cut (plan record, Release boundaries).

## Workflow state

- All eleven specs drafted 2026-11-10; `/planning` run for all of them
  the same day on the owner's go-ahead, so the set is past the review
  gate into build readiness: every module's `tasks/<id>.md` chunk is
written, `tasks/plan.md` and `tasks/todo.md` aggregate them. The four
day-one owner rulings are decided (2026-11-10, before distribution):
app id `io.github.l1br3_io.dropterm`; applet `dropterm-applet` +
`io.github.l1br3_io.dropterm.Applet`; repo rename now (the sweep writes
the new URLs, the act stays the owner's); watcher `notify` sanctioned.
Still open by design: distro-reach's two, the Kit ruling, live
geometry — see `tasks/plan.md`'s Open questions.
- Never implement a module whose spec is not approved or whose chunk is not
  written. Chunks are `/build`'s only source of tasks.
- `tasks/gpui-portability-verification.md` is the research-phase
  source-verification record backing the plan record — evidence, not a
  chunk; it stays where it is.
