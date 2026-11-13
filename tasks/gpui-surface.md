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
- [x] Headless tests pin the reconcile choices and handle-map rules
- [x] Show lands the configured geometry on both compositors; wire logs
      show the two-surface sequence

**Verification:**
- [x] Tests pass: `just check`
- [x] Manual: show walk on rig + VM (logs, screenshots)

*Done 2026-11-13 (sr-1): 19 executor tests pin the reconcile split
(resize vs recreate by placement equality — the ruled split, including
center-width-edits-recreate and full-width-alignment-is-moot), the
handle map (adopt at apply, remap at swap, loud miss), and both option
derivations (the full-output measuring stretch, the shaped window's
anchor/margins/size). Walked on rig + VM: the two-surface sequence in
the logs (measure → granted 1280x720/1280x800, never `cx.displays()` →
swap), the configured geometry landing exactly (custom 80%×30% Left
config → 1024x216 sway / 1024x240 COSMIC at origin 0,0; the VM pair
pixel-verified: 216,649 px changed inside the band, 0 right of it, 0
below), release OPEN→FIRST_FRAME 36–46 ms inside the 130 ms slide
budget. Two walk findings made structural: the probe fires on the
configured bounds (open_window's synchronous first draw sees the
degenerate pre-configure 1×1) and reports through a polled slot (an
entity update queued from render flushes inside the caller's borrow —
"already being updated"); plus one lifetime fix in main: GPUI's
default quit mode on Linux quits when the last window closes, wrong
for a single instance that boots hidden — the app now runs
QuitMode::Explicit. A hide on a shown surface logs the task-3
deferral loudly and the surface truthfully stays (the slide owns the
hide). Evidence: target/rig-evidence-gs1-show-*.log,
target/vm-evidence-gs1-{show.log,hidden,shown}.png (snapshot
pre-gpui-surface-walk held, VM restored after).*

**Dependencies:** `gpui-scaffold` merged · **Files:** the crate's executor
module · **Scope:** M

## Task 2: Shape — cadence resize and the recreate edit

**Description:** In-place resize through the 80 ms drag cadence
(height/width); recreate on anchor/margin/alignment edits with the
keyboard re-grab verified (+141 ms class). The config-edit reshape and the
drag both land through `Shape`.

**Acceptance criteria:**
- [x] Every cadence height honored on both compositors (drag-class logs)
- [x] A live geometry edit lands exactly via recreate, with the re-grab
      evidenced

**Verification:**
- [x] Manual: the 2/2b-class walks on rig + VM

*Done 2026-11-13 (sr-2): the drag runs through the machine's own
funnel — a walk driver feeds absolute pointer positions
(`DROPTERM_DRAG_WALK`, strict-parsed, tested) one per tick once shown
and at rest, through `drag_move`/`drag_trailing`/`drag_release`, so
the cadence gate, the deadband, and the release settle are the real
decision paths, logged as drag-class rows (move → applied target →
Shape Pixels → resized → frame bounds echo). The live config edit
rides a notify watcher (the sanctioned crate, version-matched): any
event in the config dir signals the tick, which reloads, recomputes
inputs, and runs `decide_reshape` — geometry-only edits resize in
place, anchor/alignment/width-mode edits recreate through the
executor's reconcile; an absorbed echo opens a 300 ms quiet window so
one write's inotify burst costs one row, not dozens. The shell also
ticks the machine's slide clock (parking finished slide-ins — an
unparked slide would block reshapes and drags by the decision layer's
own guards) and logs activation transitions (the re-grab evidence
rows). Walked on rig + VM: every cadence target applied and echoed on
both compositors (216→470 rig / 240→494 VM, each landing in the next
frame's mapped bounds), the release settled at 65%/62% with the
half-percent snap (470→468, 494→496) and the would-persist row logged
(persistence itself is the config-write slice's), the live edit
recreated at the exact new geometry (1024x360 rig / 1024x400 VM,
bottom-left, 0 px changed right of the band) with the re-grab
evidenced on the VM's real seat (activation LOST at the swap's frame
1, GAINED at frame 2 in the same millisecond — release class; the
headless rig has no seat, so its activation row reads LOST
throughout). Evidence: target/rig-evidence-gs2-shape-instance.log +
target/vm-evidence-gs2-{shape.log,drag-settled,edited}.png (snapshot
held, VM restored after).

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
