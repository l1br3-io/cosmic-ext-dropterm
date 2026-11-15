# Chunk: gpui-parity — the remaining features and the parity proof

Spec: [SPEC-gpui-parity.md](../SPEC-gpui-parity.md) · Module id:
`gpui-parity` · Phase 3 · branch `port/gpui-parity` (after every UI
slice; hardware-heavy).

**Files touched:** the `dropterm-gpui` suggest view, image view, bell
wiring, and session-restore wiring; core only if an event seam is missing
(rulings below). The checklist walks produce evidence under the lane's
`target/vm-evidence-gpui/` and rig logs.

## Planning rulings (made at /planning; veto at plan review)

- **Feature order: suggest → images → bell → session restore**, each a
  complete path (model in core, view here, rows walked) before the next —
  the parity walk is its own task, after all four.
- **The walk is the deliverable**: both compositors, the whole checklist,
  row-named evidence. A row that cannot pass stops the task and files an
  owner ruling; it is never marked with a shrug.
- **The dock-mode rows carry the known open question** (plan open
  questions item 9): `show_in_dock` needs a persistent-toplevel answer on
  GPUI or an owner-accepted degradation. The ruling lands before this
  slice's walk reaches those rows.

## Task 1: The suggest overlay and menu

**Description:** Core's suggest model and completions data (embedded +
extended tiers) drive the GPUI overlay: trigger, navigate, accept, the
menu's rows. View-model tests headless; rows on the rig + VM.

**Acceptance criteria:**
- [x] Suggest triggers, navigates, and accepts per the libcosmic rows;
      the embedded floor works on a fontless base

**Verification:**
- [x] Tests pass: `just check`
- [x] Manual: autosuggest rows of the checklist *(owner-run
      2026-11-15: reported done, no deviations filed)*

**Dependencies:** all UI slices merged · **Files:** the suggest module ·
**Scope:** M

## Task 2: Inline images

**Description:** OSC 1337/iTerm images: core decodes (strict limits,
four formats); the view renders, places, and scrolls them, including the
paste-image path.

**Acceptance criteria:**
- [x] Images render in-grid and survive scrollback navigation per the
      images rows

**Verification:**
- [x] Manual: images rows of the checklist *(owner-run 2026-11-15:
      reported done, no deviations filed)*

**Dependencies:** Task 1 · **Files:** the image view · **Scope:** M

## Task 3: The bell and session restore

**Description:** notify-rust notification firing from the GPUI build's
engine events; session capture/restore wired to the GPUI lifecycle
(restore on launch, quarantine on corrupt files, the `--is-visible`
semantics unchanged).

**Acceptance criteria:**
- [x] Bell rows pass; session restore restores tabs, order, active, and
      working directories per the sessions rows

**Verification:**
- [x] Manual: bell + sessions rows of the checklist *(owner-run
      2026-11-15: reported done, no deviations filed)*

**Dependencies:** Task 2 · **Files:** the bell/restore wiring ·
**Scope:** S

## Task 4: The parity walk — the full checklist, both compositors

**Description:** The whole manual checklist (337 rows) walked against a
release `dropterm-gpui` on COSMIC (validation VM, snapshot discipline)
and sway (rig), evidence per row class as the checklist's own structure
directs. Deviations filed; the dock-mode ruling applied per item 9.

**Acceptance criteria:**
- [ ] Every row passes on both compositors or carries a filed,
      owner-accepted deviation
- [ ] The walk's evidence lands (logs/screenshots, row-named)

**Verification:**
- [ ] Manual: the checklist, both compositors — this task *is* the
      verification

**Dependencies:** Task 3, open question item 9 ruled · **Files:** evidence
artifacts; core fixes if rows demand them · **Scope:** L

## Checkpoint: parity

- [ ] Phase 3's exit line holds: parity with the libcosmic build on COSMIC
      and sway, evidenced row by row
