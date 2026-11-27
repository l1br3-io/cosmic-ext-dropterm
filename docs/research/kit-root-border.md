# How does the drawer stay edge-to-edge under the Kit's Root auto-border?

Date: 2026-11-13 · Ruling: **carried patch** — a private one-const copy
under `[patch]`, retired by an upstream opt-out · Owner ruling on the
debt the Kit pass banked (module `kit-pass`; spike branch
`spike/gpui-layer-shell`, d722652…cd06740) · Status lives in the
[index](README.md); this record is the durable copy that outlives the
initiative's spec-and-task set.

## TL;DR

The drawer stays edge-to-edge by patching one constant in a private
copy of the Kit's source: gpui-component 0.9.1 hard-codes a 20 px
window border with no public opt-out, and the owner ruling
(2026-11-13, on recommendation) is to carry a `[patch]` copy with the
border zeroed rather than accept non-flush edges. The copy retires the
day gpui-component ships an official opt-out — and because that day is
entirely outside our control, this record is the standing reminder to
check, on every Kit bump, indefinitely.

## The debt (facts, from the Kit pass)

1. The Kit's `Root` auto-decorates every Client-decorated window with
   its Linux `window_border` (a 20 px shadow), and Wayland forces
   Client on all surfaces — so every layer surface gets it (spike
   40fe302; observed identically on sway and COSMIC).
2. There is no per-window or public opt-out at gpui-component 0.9.1.
3. The surface inflates 2×20 px on both axes: a 1280×320 drawer
   negotiates 1320×360. Edge-to-edge and full-width layouts cannot be
   expressed with the border in place.
4. `Root` is not merely decoration: it hosts the Kit's popup layer
   (popover, tooltip), which the C2b kill check proved through it —
   so "skip Root, keep the widgets" means hand-rolling the popup
   layer, the very work the Kit was adopted to avoid.

Evidence: [gpui-spike](gpui-spike.md) "Kit pass".

## The ruling

The one-const carried patch wins. A private source copy of
gpui-component under `[patch]` — never a public fork — with the border
constant zeroed; the drawer stays edge-to-edge. It becomes the third
entry in the carried-patch ledger (the vte and alacritty forks
precede it), so the upgrade ritual applies: every Kit bump re-applies
the one-line change to the new source. An upstream ask — proposing a
public per-window border opt-out — is the retirement path; filing it
is outward-facing and stays the owner's. The how-to-maintain doc (a
carried-patches section in [development](../development.md)) ships
with the Phase 3 module that creates the copy, not before code exists.

## Rejected and deferred alternatives

- **Accept the non-flush edges.** Zero maintenance, but the drawer
  permanently floats 20 px inside the screen and full-width layouts
  die. Rejected by the owner.
- **Skip `Root`: raw GPUI window plus Kit widgets.** Untested (the
  pass proved overlays through Root, not Root-less) and likely loses
  the popup layer. Not taken.
- **Drop the Kit, build the chrome on bare GPUI.** Fork-free but the
  largest cost of all; the Kit pass existed to avoid exactly this
  work. Not taken.
- **Upgrade the Kit hoping an opt-out appeared.** Cheap to check on
  any bump (see the checklist), but an upgrade re-opens the Kit
  checks (C1–C4) and nothing commits upstream to ever adding it.
  Deferred; the patch is version-pinned regardless.

## The comeback checklist (why this record exists)

The retirement event is external and unscheduled. On every Kit
version bump — and any time the border is questioned:

1. Check the new gpui-component release for a public border opt-out
   (changelog, `Root` docs, the `window_border` constant).
2. If one exists: drop the private copy, depend on the real crate
   again, and re-run the C2b kill check (Root + Popover/Tooltip
   through the 80 ms cadence, sway and COSMIC) plus one flush-edges
   screenshot. Then this record's row is marked retired in the index.
3. Until then: re-apply the one-const change to the new source per
   the carried-patches section in [development](../development.md)
   once it ships.

## What DropTerm has today

Nothing in the tree: the ruling is recorded, and the patch lands with
Phase 3's frontend module, which also writes the development.md
section. The Kit pass's other Phase 3 input — the scheme → theme-JSON
generator — is committed on the spike branch (cd06740,
`../dropterm-spike`, never merged as code); if that scratch repo is
ever deleted rather than archived, carry the generator into the main
tree first.

## Sources

- [gpui-spike](gpui-spike.md) — "Kit pass" (every fact above)
- Spike repo `../dropterm-spike`, branch `spike/gpui-layer-shell`:
  40fe302 (C2b and the border finding), cd06740 (the generator)
- The initiative's task plan, open questions items 7–8 (the ruling as
  first recorded, 2026-11-13; that set is archived after the
  initiative — this record is the durable copy)
