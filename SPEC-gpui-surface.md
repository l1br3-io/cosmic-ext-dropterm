# SPEC: gpui-surface — the GPUI executor for the surface vocabulary

Status: not started · Phase 3 · lane: main (branch `port/gpui-surface`, after
`gpui-scaffold`)

## Objective

The second real implementation of `SurfaceExecutor`: the GPUI executor that
turns each surface intent into GPUI window-system actions, on the paths the
spike chose. The decision layer runs unchanged from `dropterm-core` — the
whole point of `surface-intents` — and this executor reconciles each intent
against the surfaces it holds: mutating where GPUI allows, recreating where
it does not.

## Preconditions

`gpui-scaffold` merged (the crate, pin, Kit, and carried patch exist);
`dropterm-core`'s decision layer and vocabulary available.

## The executor's reconcile rules (spike-ruled)

- **Show**: the two-surface show — measure, destroy, create shaped — for the
  first-ever show only (no output measured yet). It never reads
  `cx.displays()`, so check 6's integer-rounded-metadata debt is dodged by
  construction and fractional outputs mis-size nothing. Creation carries
  layer, keyboard mode, and zone (create-only on GPUI; check 4's `Top`/zone
  `-1` honored at every shaped open). **Amended 2026-11-18 (the
  smooth-open fix, owner-judged on the unanswered flash ruling)**: the
  decision layer retains the measured output past a destroy, so every
  later show opens the shaped surface outright on a fresh handle — one
  surface, one map. The probe's map → destroy → swap, plus GPUI's
  pre-configure draw and the granted resize repaint, presented every
  toggle open as a flash regardless of the slide setting; the direct
  open removes the churn. The executor derives a windowless Show as
  `OpenShaped` (open + adopt, no swap); a windowed Show still swaps.
  The trade — a monitor change while hidden sizes the next show from the
  retained output — is owned by [the known-issues
  row](docs/known-issues.md).
- **Shape** (resize vs edit): height and width resize in place — every
  80 ms drag-cadence height lands (check 2; #64994 never fired) — while
  anchor/margin/alignment changes recreate the surface: open ruling 3's
  accepted redesign, one extra show's cost (~139 ms debug / well inside
  budget in release), with the keyboard re-grab landing +141 ms after the
  swap. Focus-return is what the GO clause requires, and recreate preserves
  it.
- **Hold**: keyboard mode is create-time on GPUI, so a mid-show
  `Hold { Released }` (slide-out start) releases through whatever the pin
  allows — a recreate at released hold if runtime release is unavailable,
  its cost inside the slide budget; a reversal re-holds the same way. The
  executor chooses; the intent's desired state is the contract.
- **Focus / Destroy**: activation events drive `hide_on_focus_loss` (check
  5); destroy is destroy.
- **The slide**: slide-out/in ported to GPUI's animation with the libcosmic
  build's wire shape — keyboard release at slide-out start, destroy at
  completion (~130 ms later) — and the show-to-first-frame budget the spike
  banked (34 ms release against the 130 ms slide).

## Commands

    cargo run -p dropterm-gpui    # rig + VM walks; wire evidence per si-5's route

## Testing strategy

The decision layer's headless tests stay green unchanged (they live in core
and run against the recording executor — proof the state machine didn't
move). The executor itself is compositor-evidenced: the sway-headless rig
and the validation VM walk show/drag/edit/recreate/slide/focus-loss with
wire logs and screenshots, the same evidence routes the spike used. A
translation pin test fixes intent→action mapping where it is pure (handle
map, reconcile choice), so headless coverage holds the logic the walks
can't.

## Boundaries

Never: change the vocabulary or decision layer to fit GPUI — an executor
that needs a vocabulary change stops and asks (that is a plan-level change
with owner rulings attached); carry #61420 (dormant — recreate covers the
runtime re-grab); accept non-flush edges (the carried patch owns
edge-to-edge).

## Success criteria

The surface rows of the manual checklist pass on COSMIC and sway against
the GPUI binary: show at configured geometry within budget, drag cadence
honored live, live geometry edits landing exactly (recreate path), the
slide-out wire shape matching (release at start, destroy at completion),
focus-loss hiding with the instance surviving, mid-slide reversal holding
the keyboard.

## Size

L.
