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
- [ ] Rows render per state on both compositors; title modes honored

**Verification:**
- [ ] Tests pass: `just check`
- [ ] Manual: render walk on rig + VM

**Dependencies:** `gpui-terminal` merged · **Files:** the tab-bar module ·
**Scope:** M

## Task 2: Interactions — reorder, rename, menu, close

**Description:** Drag-reorder through core's move model; inline rename;
the per-tab menu (core's `tab_actions` funnel) on the Kit's popover path;
close with the confirm flow (`CloseDecision`) and attention state.

**Acceptance criteria:**
- [ ] Reorder, pin, rename, and every menu action land in core and
      surface correctly
- [ ] The close-confirm and attention rows match the libcosmic behavior

**Verification:**
- [ ] Manual: tab rows of the checklist on rig + VM

**Dependencies:** Task 1 · **Files:** the tab-bar module · **Scope:** M

## Task 3: Rows and merge

**Description:** The tab rows of the manual checklist walked on COSMIC +
sway; merge.

**Acceptance criteria:**
- [ ] Tab rows pass on both compositors
- [ ] `just check` green on the merge result

**Verification:**
- [ ] Manual: tab rows, both compositors

**Dependencies:** Task 2 · **Files:** the branch's diff · **Scope:** S
