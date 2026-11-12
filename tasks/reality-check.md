# Chunk: reality-check — does today's binary work off COSMIC?

Spec: [SPEC-reality-check.md](../SPEC-reality-check.md) · Module id:
`reality-check` · Phase 0, lane hardware · no branch, no worktree, no code.

The checks and the recording template live in the committed plan record —
run them from there, never from this chunk:
[docs/research/portability-plan.md](../docs/research/portability-plan.md),
"Phase 0: the checklist" and "Phase 0 results". Sessions run the v0.4.0
release tarball (or `just build-release` where a session's glibc predates
2.39), with `RUST_LOG=debug` and logs captured per session. Until
`app-identity` lands, the binary is `cosmic-ext-dropterm`.

**Files touched:** `docs/research/portability-plan.md` — only your
session's pre-made slot under "Phase 0 results", per the recording
template. Nothing else; a failure is recorded, then filed as its own bug
package or fixed in a commit named in the session's slot.

Sessions are independent: run in any order, in parallel where hardware
allows. Check numbering below refers to the plan's per-session checks
(core = 1–7 and 12–13; GNOME and X11 failures are expected findings and
do not count against the gate).

## Task 1: Session 1 — COSMIC baseline

The known-good baseline; every check should pass here.
Run all sixteen checks, fill the "Session 1: COSMIC" slot. — *run
2026-11-11 agent-driven in the validation VM (v0.4.0 tag build): every
programmatically exercisable check passes (1/4/8/12/13/15), the
input-bound rows n-a without input injection — the owner's eyes-on
rows; slot filled and committed (9091b19); VM snapshotted and restored*

**Acceptance criteria:**
- [ ] The slot carries date, distro, compositor and version, session
      type, output scales, the build used, and a `pass`/`fail`/`n-a`
      line per check
- [ ] Every failure has a log excerpt and is filed (bug package or fix
      commit) and named in the slot

**Verification:**
- [ ] Manual check: slot complete per the recording template
- [ ] No edit outside the session's own slot

**Dependencies:** None · **Files:** plan record (session-1 slot) ·
**Scope:** S (manual, hardware)

## Task 2: Session 2a — sway (the wlroots witness)

Same shape as Task 1, slot "Session 2a: sway". Check 3 binds
`cosmic-ext-dropterm toggle` via `bindsym`. **Dependencies:** None ·
**Scope:** S (manual, hardware; this session is shared with `gpui-spike`)
— *run 2026-11-11 as sway 1.10.1 headless in docker (v0.4.0 tag build):
green where exercisable — maps + renders the dark default theme,
destroy-on-hide parity (damage-proven), CLI truth table, 8 ms handoff,
surface restore on relaunch; input-bound core rows (2/3/5) and the
in-rig bell failure (zero Notify calls) want one hardware sway session;
slot filled and committed (1deed62)*

## Task 3: Session 2b — Hyprland

Same shape, slot "Session 2b: Hyprland"; check 3 uses `bind`.
**Dependencies:** None · **Scope:** S (manual, hardware)

## Task 4: Session 3 — KDE Plasma (Wayland)

Same shape, slot "Session 3: KDE Plasma (Wayland)". First data point for
KDE's layer-shell quirks (its own focus and zone behavior) — record what
differs from cosmic-comp, especially checks 1, 2, and 16.
**Dependencies:** None · **Scope:** S (manual, hardware)

## Task 5: Session 4 — GNOME (Wayland), expected to fail

Same shape, slot "Session 4: GNOME". The deliverable is *how* it fails
(inform the companion-extension design): which checks surface at all,
what `--is-visible` reports (check 12), logs. The known mode: the
2026-11-08 sandbox walk without `zwlr_layer_shell_v1` never surfaced
while `--is-visible` reported true — confirm or correct it here; this
session's record is `layer-shell-guard`'s baseline.
**Dependencies:** None · **Scope:** S (manual, hardware)

## Task 6: Session 5 — one X11 session, expected to fail

Same shape, slot "Session 5: X11"; record which session ran (Xfce,
Cinnamon, or i3 — a recording choice, not a ruling). Record how the
layer-shell-only build fails. **Dependencies:** None · **Scope:** S
(manual, hardware) — *run 2026-11-11 as a scripted i3-4.24-over-Xvfb
docker rig with the v0.4.0 tag build; slot filled and committed (482a37d):
the desktop mode is a zombie with a degenerate 32 px DOCK band on first
launch (then nothing at all), --is-visible truthful-false, handoff holds;
libXi-less panic documented; agent evidence (logs/xprop/pixel), not
eyes-on*

## Task 7: Session 6 — one non-Pop distro (Arch or Fedora)

Slot "Session 6: non-Pop distro". The distro axis: run the tarball
(`ldd`, confirm the glibc 2.39 floor in practice), then `just deps` and
`just build-release` from source; check `Noto Sans Mono` presence. These
facts are `distro-reach`'s input. **Dependencies:** None · **Scope:** M
(manual, hardware) — *run 2026-11-11 on archlinux:latest (glibc 2.44):
the tarball binary's symbol floor is GLIBC_2.39 exactly (the plan's
floor, confirmed by the artifact), source build green, winit's dlopen'd
Wayland/X11 libs mean ldd under-reports (floor distros need
libwayland-client0 + libxi6), fontless base records the Noto fact; slot
filled and committed (8328d51)*

## Task 8: Close-out — every failure filed

**Description:** Sweep of all seven slots: every `fail` has a log
excerpt, is filed as a bug package or fixed in a commit, and is named in
its slot. Phase 0's exit gate per the plan: sway, Hyprland, and KDE
green on the core checks collapses the distro axis into packaging and
docs; every failure became a Phase 1 item.

**Acceptance criteria:**
- [ ] All seven slots filled per the template
- [ ] Every failure filed and named; no unexplained `fail`

**Verification:**
- [ ] Manual review of the results section against the checklist

**Dependencies:** Tasks 1–7 · **Files:** plan record (slots), the filed
bug packages/commits · **Scope:** S
