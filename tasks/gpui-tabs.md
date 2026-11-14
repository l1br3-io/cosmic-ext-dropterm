# Chunk: gpui-tabs — the tab bar on GPUI

Spec: [SPEC-gpui-tabs.md](../SPEC-gpui-tabs.md) · Module id: `gpui-tabs` ·
Phase 3 · branch `port/gpui-tabs` (after `gpui-terminal`; parallel with
`gpui-status`).

**Files touched:** the `dropterm-gpui` tab-bar module. Consumed from core:
`tabs.rs` (model), `app/tab_actions.rs` (menu funnel). Read-only
reference: `src/tab_bar.rs` (the libcosmic behavior set).

## Planning rulings (made at /planning; veto at plan review)

- **The bar is view-only over core's model** — every behavior (move,
  close decision, title fallback, attention) is a core call; a test pins
  that no tab-model logic is duplicated in the view.

## Task 1: The bar's view model and rendering

**Description:** Tab rows from core's model: active/close states, pins,
borders, `TabPosition` layout, the `+` and gear affordances, title modes
(`TabTitleMode`). Headless tests pin state → presentation.

**Acceptance criteria:**
- [x] Rows render per state on both compositors; title modes honored
      *(walked 2026-11-14 — rig target/rig-evidence-gtabs1/ and VM
      pixels target/vm-evidence-gtabs1/: Shell titles live from seeded
      OSC 2 reports, Numbered parks at `Terminal N`, the strip at
      bottom/top per `TabPosition`, border on/off, the 1-tab strip
      carrying the corner affordances alone)*

**Verification:**
- [x] Tests pass: `just check`
- [x] Manual: render walk on rig + VM

**Dependencies:** `gpui-terminal` merged · **Files:** the tab-bar module ·
**Scope:** M

## Task 2: Interactions — reorder, rename, menu, close

**Description:** Drag-reorder through core's move model; inline rename;
the per-tab menu (core's `tab_actions` funnel) on the Kit's popover path;
close with the confirm flow (`CloseDecision`) and attention state.

**Acceptance criteria:**
- [x] Reorder, pin, rename, and every menu action land in core and
      surface correctly
- [x] The close-confirm and attention rows match the libcosmic behavior

**Verification:**
- [x] Manual: tab rows of the checklist on rig + VM *(walked
      2026-11-14 — VM pixels + logs under target/vm-evidence-gtabs2/
      (runs g/h/x/b, driven through a uinput injector whose pointer
      and keyboard the COSMIC session delivers whole), rig logs +
      strip pixels under target/rig-evidence-gtabs2/; the deep rows:
      attention markers land on inactive tabs only — the seeded bell
      marks both inactive tabs • then 🔔 while the active tab's
      signals drop at the poll, and activating a marked tab clears it;
      the inline rename commits a typed draft through core
      (double-click → the Kit input in the row's label slot → "work"
      shown at once) and Escape cancels the draft; the per-tab menu
      opens on the Kit's popover path through the Root-wrapped window
      (nine rows composed per the reference's rules) and its Pin row
      arms a pin the row's label then prefixes; the pinned × refuses
      with the warn row and no dialog; drag-reorder moves a tab
      through core's reorder (the chip follows, the drop's row decides
      the index, the active tab's identity survives); the close gate
      holds per the reference — a tab with a foreground child arms the
      in-surface modal (Escape, the scrim press, and the Cancel button
      each dismiss; Close Tab proceeds), an idle tab closes instantly,
      the middle-click rides the same gate, the toggle off closes
      everything instantly (run b), and a shell exit closes ungated —
      the fleet exhausting into the never-empty respawn (run x); the
      same engine paths walked green on sway's logs (attention rows +
      the strip's render — the emoji markers' glyphs need a font the
      rig lacks, gt-1's silent-glyph class, so their pixels are the
      VM's); one combination row not re-captured in the saved logs:
      the pinned tab closed by its own shell exit — both halves are
      individually walked (the pinned refusal, the ungated exit close)
      and core pins the decision; known scope notes: no Switch Profile
      menu section or +-dropdown (no profile spawn path on GPUI yet —
      chrome's), no session-persistence rows (no GPUI session wiring
      yet), the rename/modal state dies with the window on hide (the
      drawer destroys on hide — the libcosmic surface keeps it))*

**Dependencies:** Task 1 · **Files:** the tab-bar module · **Scope:** M

## Task 3: Rows and merge

**Description:** The tab rows of the manual checklist walked on COSMIC +
sway; merge.

**Acceptance criteria:**
- [x] Tab rows pass on both compositors
- [x] `just check` green on the merge result

**Verification:**
- [x] Manual: tab rows, both compositors *(the module builds on main
      per the owner's 2026-11-14 ruling — no branch to merge; the
      Task-2 walk above is the rows' record on COSMIC + sway, `just
      check` green on the final tree (1dbbeb0) after the walk's
      inset_0 fix; the physical-pointer-feel rows (the drag's lift
      weight, hover states) remain the owner's at-console class, the
      gt-2 precedent)*

**Dependencies:** Task 2 · **Files:** the branch's diff · **Scope:** S
