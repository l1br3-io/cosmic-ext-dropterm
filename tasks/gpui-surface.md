# Chunk: gpui-surface — the GPUI executor for the surface vocabulary

Spec: [SPEC-gpui-surface.md](../SPEC-gpui-surface.md) · Module id:
`gpui-surface` · Phase 3 · branch `port/gpui-surface` (after
`gpui-scaffold`; may run in `../ced` while the main checkout takes another
slice).

**Files touched:** `dropterm-gpui`'s executor module (new) and app-shell
wiring. Read-only reference: the spike harness's two-surface-show and
recreate paths (`../dropterm-spike` at the branch head) and
`src/executor.rs`'s libcosmic translation (the intent semantics).

## Planning rulings (made at /planning; veto at plan review)

- **The reconcile split is resize-vs-recreate**: `Shape` with a
  height/width change resizes in place (check 2 proved the 80 ms cadence);
  `Shape` with an anchor/margin/alignment change recreates (open ruling
  3's numbers). A pure reconcile-choice function is unit-tested headless.
- **`Hold { Released }` on GPUI may ride a recreate** at released hold —
  keyboard mode is create-time at this pin — chosen by the executor, cost
  inside the slide budget. If the pin turns out to allow a cheaper
  runtime release, take it; the wire evidence either way lands in the
  walk.
- **The slide ports behavior, not mechanics**: GPUI's animation replaces
  `slide.rs`'s draw-time translation, but the wire shape (keyboard
  release at slide-out start, destroy at completion, ~130 ms) is the
  pinned parity.

## Task 1: The executor skeleton and the two-surface show

**Description:** `GpuiExecutor` implements core's `SurfaceExecutor`; the
handle map and reconcile-choice logic land headless-tested. `Measure` →
`Show` runs the two-surface show (measure, destroy, create shaped) — never
reading `cx.displays()` — with layer, keyboard mode, and zone at creation.

**Acceptance criteria:**
- [ ] Headless tests pin the reconcile choices and handle-map rules
- [ ] Show lands the configured geometry on both compositors; wire logs
      show the two-surface sequence

**Verification:**
- [ ] Tests pass: `just check`
- [ ] Manual: show walk on rig + VM (logs, screenshots)

**Dependencies:** `gpui-scaffold` merged · **Files:** the crate's executor
module · **Scope:** M

## Task 2: Shape — cadence resize and the recreate edit

**Description:** In-place resize through the 80 ms drag cadence
(height/width); recreate on anchor/margin/alignment edits with the
keyboard re-grab verified (+141 ms class). The config-edit reshape and the
drag both land through `Shape`.

**Acceptance criteria:**
- [ ] Every cadence height honored on both compositors (drag-class logs)
- [ ] A live geometry edit lands exactly via recreate, with the re-grab
      evidenced

**Verification:**
- [ ] Manual: the 2/2b-class walks on rig + VM

**Dependencies:** Task 1 · **Files:** the executor module · **Scope:** M

## Task 3: Holds, the slide, and focus-loss

**Description:** `Hold` variants (released at slide-out start, re-held on
reversal, downgraded on stay-open flip); the slide-out/in ported to GPUI
animation with the pinned wire shape; activation-loss drives
`hide_on_focus_loss` (check 5); `Focus` routes to the terminal input;
`Destroy` destroys.

**Acceptance criteria:**
- [ ] Wire shape matches: keyboard release at slide-out start, destroy at
      completion (~130 ms later), both compositors
- [ ] Focus-loss hides with the instance surviving; the caret returns on
      re-show

**Verification:**
- [ ] Manual: the si-5-class walk on rig + VM

**Dependencies:** Task 2 · **Files:** the executor + animation modules ·
**Scope:** M

## Task 4: Budget, integration rows, and merge

**Description:** The show budget banked (release numbers against the
130 ms slide, the VM's caveat noted); the decision layer's headless tests
confirmed unchanged (core, untouched by this slice); the surface rows of
the manual checklist walked on COSMIC + sway; merge.

**Acceptance criteria:**
- [ ] Release show-to-first-frame numbers banked on the VM
- [ ] The manual surface rows pass on both compositors
- [ ] `just check` green on the merge result

**Verification:**
- [ ] Manual: surface rows, both compositors
- [ ] Tests pass: `just check`

**Dependencies:** Task 3 · **Files:** the branch's diff · **Scope:** S

## Checkpoint: the drawer lives on GPUI

- [ ] Show, drag, edit, slide, focus-loss — the surface lifecycle runs on
      GPUI through the unchanged decision layer
