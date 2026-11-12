# SPEC: gpui-spike — can GPUI carry the lifecycle?

Status: not started · Phase 2 · lane: spike · branch
`spike/gpui-layer-shell` (scratch dir `../dropterm-spike`, its own git
repo — not a worktree) — never merged as code

## Objective

Prove or refute on hardware that GPUI's layer-shell can carry DropTerm's
surface lifecycle, and price it in carried patches and redesigns, before
any rewrite starts. The deliverable is the report, appended to "Phase 2
results" in the plan record on main.

## Where and what

A throwaway branch in its own git repo (`../dropterm-spike`, a standalone
scratch dir, not a worktree). The scratch crate
(`spikes/gpui-layer-shell/`, under ~500 lines, `wayland` feature only,
lockfile committed on the branch) declares an empty `[workspace]` table, so
its lockfile and GPUI's dependency tree never touch the root workspace.
Check 14's experiment — dropping that empty table for one commit on the
branch — is the coexistence test.

## Pin

zed git at the commit the then-current `gpui-pre` snapshot was cut from
(`gpui-pre` 0.3.12 = zed `d2a4fb6` today), covering the whole `gpui_*`
crate set. If the Kit was ruled "adopt" before the spike starts, pin the
snapshot instead and run the Kit checks (C2a, C2b, C4) in the same pass.

## Commands

    cd spikes/gpui-layer-shell && cargo run    # own workspace, own lockfile

The scratch crate is outside the root workspace: `just check` does not
apply to it, and it never runs against the root `target/`.

## Hardware

COSMIC, including one fractionally scaled output; sway as the
cross-compositor witness; an X11 session for check 12; the Mac for check 13.
The checks concern compositor behavior (focus, timing, configure handling)
the project verifies manually in real sessions, as with every runtime
behavior ([testing](docs/testing.md)).

## The checks

The sixteen base checks (0, 1, 2, 2b, 3–14) with their pass criteria and
fallbacks live in the plan's "Phase 2: the spike checks" table — committed
and canonical; run from there, not from this spec. The Kit checks
(C2a, C2b, C4) run in this pass too if the Kit was ruled adopt first.

## Decision criteria (from the plan)

- **GO**: checks 0, 1, 2, 5, and 6 pass, with at most the ledger's named
  carried patches; 3 has a workable path (carried #61420 or an accepted
  redesign); 4 holds.
- **REDESIGN**: no shape-at-creation path fits the slide as designed, or
  in-place resize fails, but a reworked show (the slide starts after the
  shaped surface maps) or a degraded drag is acceptable; the keyboard
  re-grab is redesigned. The state machine behind `surface-intents`
  survives; only its executors change.
- **NO-GO / fork**: check 0 fails, or check 3 fails with no redesign
  preserving focus-return. The choice becomes forking GPUI's layer-shell
  (carrying patches, the vte/alacritty precedent) versus falling back to
  `iced_layershell`.
- Every verdict carries the carried-patch ledger.

## Testing strategy

The checks are the tests (manual, hardware). The report is the artifact:
verdict, pin identity, per-check evidence, the carried-patch ledger, the
measured show latency of the chosen shape-at-creation path against the
130 ms slide (`SLIDE_DURATION_MS`, `src/slide.rs:44`), the coexistence
result (check 14), and the recreate cost that open ruling 3 needs.

## Boundaries

- Always: commit the scratch crate's lockfile on the branch; record the pin
  identity in the report.
- Never: merge the scratch crate into main; let its dependency tree touch
  the root workspace outside check 14's one-commit experiment; edit the
  plan record beyond "Phase 2 results".

## Success criteria (the report)

A GO, REDESIGN, or NO-GO verdict per the decision criteria; the pin
identity; per-check evidence; the carried-patch ledger; the measured show
latency against the 130 ms slide; the coexistence result (check 14); the
recreate cost that open ruling 3 needs.

## Feeds

Phase 3's slices (ids assigned at the report), the Kit ruling (if not yet
made, per open ruling 2), and open ruling 3 (live geometry edits).

## Effort

(1) scaffold and boot (check 0); (2) lifecycle checks 1–8 and detection
(11); (3) grid render (9); (4) D-Bus interop (10); (5) coexistence (14);
(6) the report. Items 1–4 are hardware-session work.
