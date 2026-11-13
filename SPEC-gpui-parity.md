# SPEC: gpui-parity — the remaining features and the parity proof

Status: not started · Phase 3 · lane: main + hardware (branch
`port/gpui-parity`, after every UI slice)

## Objective

Two things in one closing slice: the features no earlier slice covered —
the suggest overlay and its menu, inline images, the bell, session restore
— and the parity proof itself: the whole manual checklist
([testing](../docs/testing.md), 337 rows) walked against the GPUI binary on
COSMIC **and** sway, which is Phase 3's exit line.

## Preconditions

`gpui-surface`, `gpui-terminal`, `gpui-tabs`, `gpui-status`, and
`gpui-chrome` merged.

## Scope

- Suggest: the overlay and its menu (core's suggest model and completions
  data; the view and key handling on GPUI).
- Inline images: OSC 1337 / iTerm images (core decodes; the view renders
  and scrolls them; paste-image UX rows included).
- The bell: notify-rust desktop notification (core-side today) firing from
  the GPUI build; the visual-bell rows if any.
- Session restore: core's capture/restore wired to the GPUI build's
  lifecycle (restore on launch, `--is-visible` semantics unchanged).
- The parity walk: the full checklist on both compositors, evidence per
  the standing routes (rig logs and screenshots; validation VM with
  snapshot discipline). Deviations become filed rows, not silent skips;
  anything the GPUI build cannot express goes to the owner as a ruling
  (the open dock-mode question is the known candidate).

## Commands

    cargo run -p dropterm-gpui
    just build-release            # the walk runs against release builds

## Testing strategy

Feature halves: model tests already live in core; view-model tests land
headless per feature. The parity walk is manual by design (there is no
middle tier) — its recording follows the checklist's own row structure,
per compositor.

## Boundaries

Never: weaken a checklist row to pass it (a row that cannot pass is an
owner ruling, filed); skip a compositor (the exit line names both).

## Success criteria

Every checklist row passes on both compositors or carries a filed, owner-
accepted deviation; suggest, images, bell, and session-restore rows pass
outright.

## Size

L.
