# SPEC: kit-pass — the Kit on the snapshot

Status: not started; runs only if the Kit is ruled "adopt" after the base
spike · Phase 2 · lane: spike (the `gpui-spike` scratch dir,
`../dropterm-spike`)

## Objective

In the spike's scratch dir with the pin swapped to the `gpui-pre` snapshot,
re-run base checks 0–4 (condition C1) and run the Kit checks C2a, C2b, and
C4, so the Kit verdict is read against the snapshot the project would
actually consume — not against the zed commit it was cut from.

## Preconditions

`gpui-spike` reported (at least checks 0–4 evidenced on the zed-git pin);
the Kit ruled "adopt" (open ruling 1; timing per open ruling 2).

## Commands

    cd spikes/gpui-layer-shell && cargo run    # pin swapped to gpui-pre

## Testing strategy

The plan's "gpui-kit" conditions, as checks (that section is the canonical
statement):

- **C1**: base checks 0–4 re-run and passing on the snapshot pin.
- **C2a**: a consumer build maps a `LayerShell` window through gpui-pre's
  `wayland` features (manifest-level wiring was confirmed 2026-11-10; the
  consumer build is the remaining proof).
- **C2b**: Root + Kit overlays (menu, tooltip) render and behave on a
  `LayerShell` surface during the 80 ms reshape cadence.
- **C4**: the scheme → theme-JSON generator applies through
  `ThemeRegistry`/`apply_config` on the snapshot pin.

## Boundaries

- Never: adopt gpui-shell (rejected; component-only adoption per C3); merge
  the scratch crate; edit the plan record beyond appending to the spike
  report.

## Success criteria

A Kit GO or NO-GO for that pin, appended to the spike report. A NO-GO
leaves GPUI's verdict as it stands and the chrome hand-built (the plan's
chrome inventory is that backlog). Either outcome leaves the six seams and
the roadmap unchanged.

## Size

S.
