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
`reported: GO | REDESIGN | NO-GO`; `parked (<date>, owner ruling)`
marks deliberately deferred work — it is out of the workflow's way
until its note's un-park condition is met.

| Id | Phase | Kind | Depends on | Lane | Shares hot files with | Spec | Status |
|---|---|---|---|---|---|---|---|
| `reality-check` | 0 | manual, no code | none | hardware | none | [SPEC-reality-check.md](SPEC-reality-check.md) | parked (owner ruling, 2026-11-13) — 4/7 sessions filled (COSMIC/sway/X11/non-Pop distro); Hyprland+KDE+GNOME+close-out parked with it |
| `app-identity` | 1 | code, `res/`, CI, docs; mechanical | none — merges first | flag day | every package (mostly docs) | [SPEC-app-identity.md](SPEC-app-identity.md) | merged (f698083) |
| `config-store` | 1 | code | `app-identity`\* | B | `surface-intents` (`src/app.rs`), `control-transport` (`Cargo.toml` features) | [SPEC-config-store.md](SPEC-config-store.md) | merged (f5199ec) |
| `surface-intents` | 1 | refactor + tests | `app-identity`\* | A | `config-store` (`src/app.rs`) | [SPEC-surface-intents.md](SPEC-surface-intents.md) | merged (00cf77d) |
| `control-transport` | 1 | code | `app-identity`\*; after `config-store` in lane B | B | `layer-shell-guard` (`src/main.rs`) | [SPEC-control-transport.md](SPEC-control-transport.md) | merged (e6ad23b) |
| `layer-shell-guard` | 1 | code, small | `app-identity`\* | C | `control-transport` (`src/main.rs`) | [SPEC-layer-shell-guard.md](SPEC-layer-shell-guard.md) | merged (f73cf27) |
| `keybind-docs` | 1 | docs | `app-identity`, `reality-check` | C | none | [SPEC-keybind-docs.md](SPEC-keybind-docs.md) | parked (owner ruling, 2026-11-13) — rides the parked Phase 0 sessions |
| `contract-docs` | 1 | docs | `control-transport` | C | none | [SPEC-contract-docs.md](SPEC-contract-docs.md) | merged (5bf146b) |
| `distro-reach` | 1 | release and packaging | `reality-check`, `app-identity` | C | `.github/workflows/release.yml` | [SPEC-distro-reach.md](SPEC-distro-reach.md) | parked (owner ruling, 2026-11-13) — the floor and channels rulings deferred with the release |
| `gpui-spike` | 2 | throwaway crate + report | none | spike | none (own workspace) | [SPEC-gpui-spike.md](SPEC-gpui-spike.md) | done (2026-11-13, owner ruling) — **GO** reported in [docs/research/gpui-spike.md](docs/research/gpui-spike.md) and the plan's Phase 2 results: 0/1/2/5/6 pass on both compositors (release show 34ms vs the 130 budget; heights honored; activation works), 4 holds, 3's path evidenced (recreate re-grabs at +141ms, ruling 3's numbers), 8 resolved (gate = focus+serial, anchor dies), 9/10/11 green (dbus "bug" was a dropped connection in the harness), 14 ABSORBS; carried ledger: fractional visible output on COSMIC + integer-rounded display metadata, #61420 dormant; checks 12–13 and check 3's direct runtime row parked on their sessions (owner ruling, 2026-11-13); the Kit ruling gates `kit-pass` |
| `kit-pass` | 2 | spike second pass | `gpui-spike`; the Kit ruled "adopt" | spike | none | [SPEC-kit-pass.md](SPEC-kit-pass.md) | done (2026-11-13) — **Kit GO for the pin** (spike d722652…cd06740, verdict in the report's "Kit pass" section + the plan's Phase 2 results): C1 passes twice over (gpui-pre =0.3.12 sole GPUI; source-diff fidelity — wayland backend byte-identical, one real delta, wgpu 30.0.2, invisible in the checks 0–4 re-run); C2a consumer build maps LayerShell through gpui-pre; C2b kill check green on both compositors (Root + Popover/Tooltip through the 80 ms cadence); C3 holds (zero gpui-shell/gpui-kit edges); C4's generator applies via apply_config (Phase 3 input committed). Carried debt: the Kit's Root auto-border inflates a layer surface 2×20 px (1320×360 for a 1280×320 drawer, no opt-out at this pin) — ruled 2026-11-13: the carried patch (private copy, upstream ask as retirement; plan open questions item 7); the pin ruled the same day on recommendation: gpui-pre =0.3.12 is Phase 3's GPUI source (item 8); durable record: [kit-root-border](docs/research/kit-root-border.md) |
| `shared-core` | 3 | refactor, no behavior change | Phase 3 entry (met) | main | every Phase 1 package (it moves their files) | [SPEC-shared-core.md](SPEC-shared-core.md) | merged (0db7f38) — the si-5 VM walk green 2026-11-13 (show 40 % pixel-exact with the first-frame block caret, the background job surviving a hide/show cycle, drag live-apply + settle + single write-back, live in-place edit at the same pid, focus-loss hide, CLI round-trips; evidence in the lane's `target/vm-evidence-sc5/`); the crate + movers stand as specced — pure movers + pin test, split movers behind the TextHost seam, dbus_state home + retired lib target + applet on core, decision layer + slide model + transport halves with `ToggleIntent` the one vocabulary seam |
| `gpui-scaffold` | 3 | new crate | `shared-core` | main | root `Cargo.toml`/lock | [SPEC-gpui-scaffold.md](SPEC-gpui-scaffold.md) | merged (1a25154) — all four tasks walked 2026-11-13 on rig + VM (evidence under the lane's `target/`: the crate on the ruled pin with the manifest self-asserting, the carried one-const Kit copy kill-checked green with flush edges exact on both compositors, the guard re-homed to `core::transport` refusing identically, and the full contract round-trip — name owned with self-check, `Visible` read, `ActivateAction` flipping the render live and back, second-launch exit 0, stale-owner refusal 1 after the walk's fix: zbus 5 has no default method timeout, so core's forward now bounds the wait at 5 s for both frontends); the GPUI parallel slices build next |
| `gpui-surface` | 3 | new code | `gpui-scaffold` | main (∥) | none (own crate region) | [SPEC-gpui-surface.md](SPEC-gpui-surface.md) | merged (05ac2ba) — the four tasks walked 2026-11-13 on rig + VM (evidence under the lane's `target/`: rig-evidence-gs{1,2,3}*, vm-evidence-gs{1,2,3,4}*; just check green on the merge result): the GPUI executor with the ruled reconcile split and two-surface show, the drag cadence through the machine's real funnel, the live config-edit recreate with the re-grab, the holds/slide/focus-loss wire (release at slide-out start, destroy at 142–152 ms in release, focus-loss hide on COSMIC's real seat), and the release budget banked (OPEN→FIRST_FRAME 34–49 ms inside the 130 ms slide) — dropterm-core untouched (empty diff, 819 tests green); seven walk findings made structural along the way (the pre-configure 1×1 read, render-queued entity updates flushing inside the caller's borrow, last-window-close quit, apply-time retirement, transition-gated focus-loss, the transition-order disarm, the completion destroy's phase publish) |
| `gpui-terminal` | 3 | new code | `gpui-scaffold` | main (∥) | `gpui-surface` (the integration task) | [SPEC-gpui-terminal.md](SPEC-gpui-terminal.md) | merged (761547d) — Tasks 1–4 walked 2026-11-13/14 on rig + VM (evidence under target/: rig-evidence-gt{1,2,4}*, vm-evidence-gt{1,2,3,4}*); the mouse-tracking encodings moved to core framework-free with both frontends consuming them; just check green on the merge result — Task 1 walked 2026-11-13 (grid view + resize: VM pixels vm-evidence-gt1-*, rig logs rig-evidence-gt1-*, 8×20→100×20 through Term::resize + SIGWINCH with stty confirming); findings made structural: the font-family candidate resolution (an unheld family rasterizes glyphs SILENTLY into nothing) and the rig's harness-window silence (presents the band's windows, never the check harness's — logs class for this module, matrix in the harness docs); Task 2 walked 2026-11-14 (input/keybinds/selection/scrollback: every default chord + the custom overlay fired live on the rig's logs — rig-evidence-gt2-pass1/-trace/-pass2 — typed input end-to-end through translate→PTY→echo, and the VM's pixels hold SelectAll's full-grid overlay + Escape clear + the uinput hold-drag's 80-byte selection — vm-evidence-gt2-*; three environment findings recorded in the harness docs: the never-presenting rig surface holds keyboard (only pixels/pointer absent), cosmic-comp drops wtype's modifiers, and uinput wheel synthesis reaches no app on the VM — the physical-wheel row rides parity's human-at-console class); Task 3 walked 2026-11-14 (mouse reporting + clipboard: the xterm encodings moved to core framework-free — both frontends consume one implementation, the libcosmic input.rs an adapter with its 702 tests unchanged — and the VM's walk echoes the pane's own tracking bytes through `cat -v`, SGR's press/motion/release triple and the legacy X10 forms, with the clipboard rows proven by copy-after-keystroke through GPUI's clipboard, an external wl-copy pasted through the chord, and an OSC 52 store serving over data-control — evidence under target/{rig,vm}-evidence-gt3-*)
| `gpui-theme` | 3 | spike carry | `gpui-scaffold` | main (∥) | none | [SPEC-gpui-theme.md](SPEC-gpui-theme.md) | merged (36648c1) — both tasks walked 2026-11-14 on rig + VM (evidence under target/{rig,vm}-evidence-gm2-*): the C4 generator carried byte-identical from the spike (cd06740 + SPDX, diff-checked; four tests green; the pedantic adaptation on main.rs's mod declaration so the file stays auditable), and the apply path green end to end — the Kit initialized and the scheme applied in boot() before any window exists (token read-back: Dracula background l=0.184 = #282A36, primary riding its Blue), a live scheme edit re-themes without restart through the tick's apply point (identity-gated: geometry-only edits never re-theme; the theme-applied count held 2 through the height edit), the pane re-themes on the same resolution (resolve/boot_colors one-chain agreement pinned field-wise on every generator input slot), and the kill check themed through the same route with flush edges exact (320→320) and overlays silent — the standing C4 chrome harness on every Kit bump |
| `gpui-tabs` | 3 | new code | `gpui-terminal` | main (∥) | none | [SPEC-gpui-tabs.md](SPEC-gpui-tabs.md) | merged (1dbbeb0, on main per the owner's 2026-11-14 ruling) — Tasks 1–3 walked 2026-11-14 on rig + VM (evidence under target/{rig,vm}-evidence-gtabs{1,2}/): the strip is view-only over core's model (Task 1's nine headless pins + the render rules ported), and the interactions land over core's calls (twelve more headless pins): drag-reorder and the menu's moves through `reorder_tabs`/`move_target`, the inline rename through `commit_rename` (the Kit input hosted in the row, Enter/Escape/blur and click-away commits), the per-tab menu on the Kit's popover path through the Root-wrapped window (Pin/Unpin, Duplicate seeded from the source, the sweeps each through the gated front door), the close gate (`CloseDecision`) with the in-surface modal holding the keyboard (the pane's claim turned cooperative, a render-side repair) and the never-empty respawn answering the last-tab close and shell exits, and attention markers landing on inactive tabs only through the pane's polled latch; walk finding made structural: an absolute overlay needs `inset_0` (1dbbeb0); scope notes: no Switch Profile menu section or `+` dropdown (no GPUI profile spawn path yet), no session-persistence rows |
| `gpui-status` | 3 | new code | `gpui-terminal` | main (∥) | none | [SPEC-gpui-status.md](SPEC-gpui-status.md) | not started |
| `gpui-chrome` | 3 | new code | `gpui-tabs`, `gpui-status`, `gpui-theme` | main | none | [SPEC-gpui-chrome.md](SPEC-gpui-chrome.md) | not started |
| `gpui-parity` | 3 | new code + hardware | every UI slice | main + hardware | none | [SPEC-gpui-parity.md](SPEC-gpui-parity.md) | not started — gate: open question item 9 ruled before the shell-presence rows |
| `gpui-cutover` | 3 | deletion + release | `gpui-parity` | main | deletes the libcosmic frontend | [SPEC-gpui-cutover.md](SPEC-gpui-cutover.md) | not started — owner-gated beyond the code swap |

\* A merge-order dependency only: the package may start before
`app-identity` merges and rebases onto it. `app-identity`'s code diff is
small (about twenty identity literals in `src/lib.rs`, `src/dbus_state.rs`,
`applet/src/applet.rs`, and `applet/src/main.rs`, plus the crate names);
most of its 41 files are `res/`, CI, the justfile, and docs.

Phase 3's ids were assigned at the spike-report checkpoint (2026-11-13,
`/planning` over the GO + Kit-GO verdicts and rulings items 7–8); Phase
4–5 packages still get theirs when each opens (plan rules).

**Build order**: `app-identity` merges first and every other package merges
after it; packages may start earlier and rebase, and `config-store` should,
to keep the gap between the two merges short. `reality-check` and `gpui-spike` run
from day one beside everything. Phase 3's spine is sequential —
`shared-core` (merges alone; it moves everything) → `gpui-scaffold` →
the parallel UI slices (`gpui-surface` ∥ `gpui-terminal` ∥ `gpui-theme`,
then `gpui-tabs` ∥ `gpui-status`) → `gpui-chrome` → `gpui-parity` →
`gpui-cutover`.

## Worktree split

The whole set is committed, so every checkout sees all of it through git;
the table maps which lane edits which modules' files (a lane touches its
own specs and chunks plus the shared status lines, nothing else):

| Checkout | Branch | Modules | Spec files it edits |
|---|---|---|---|
| this checkout (main) | lane C, hardware sessions, coordination; Phase 3's sequential spine | `reality-check`, `layer-shell-guard`, `keybind-docs`, `contract-docs`, `distro-reach`; Phase 3: `shared-core`, `gpui-scaffold`, `gpui-chrome`, `gpui-parity`, `gpui-cutover` | those modules' specs and chunks, plus the shared set |
| `../ced` (worktree, reused) | lane A complete; Phase 3 parallel branch 1 | `app-identity` (merged f698083), `surface-intents` (merged 00cf77d); Phase 3: one of `gpui-surface`/`gpui-terminal`/`gpui-tabs` | its Phase 3 module's spec and chunk |
| `../ced2` (worktree, reused) | lane B complete; Phase 3 parallel branch 2 | `config-store` (merged f5199ec), `control-transport` (merged e6ad23b); Phase 3: another of `gpui-surface`/`gpui-terminal`/`gpui-theme`/`gpui-status` | its Phase 3 module's spec and chunk |
| `../dropterm-spike` (scratch dir, own repo) | `spike/gpui-layer-shell` (never merged) | `gpui-spike`, `kit-pass` (done; `theme_gen.rs` at cd06740 is `gpui-theme`'s carry source until the carry lands) | `SPEC-gpui-spike.md`, `SPEC-kit-pass.md` |

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
  Still open by design: the Kit ruling, live geometry (both Phase 2).
  Parked by owner ruling (2026-11-13): distro-reach's two rulings,
  deferred with the release — see `tasks/plan.md`'s Open questions.
- Phase 3 opened 2026-11-13 at the spike-report checkpoint: `/planning`
  assigned the ten ids, wrote the specs and chunks, and extended
  `tasks/plan.md`/`tasks/todo.md` (planning rulings + open question
  item 9, dock mode under GPUI). Owner review passed 2026-11-13
  (`/build auto`): the set is committed and `shared-core` builds
  first. Contributor diagrams (connection
  graph, adapter surfaces, class diagram) live in
  [docs/architecture.md](docs/architecture.md).
- Never implement a module whose spec is not approved or whose chunk is not
  written. Chunks are `/build`'s only source of tasks.
- `tasks/gpui-portability-verification.md` is the research-phase
  source-verification record backing the plan record — evidence, not a
  chunk; it stays where it is.
