# Chunk: kit-pass — the Kit on the snapshot

Spec: [SPEC-kit-pass.md](../SPEC-kit-pass.md) · Module id: `kit-pass` ·
Phase 2, lane spike (the `gpui-spike` scratch dir, `../dropterm-spike`) ·
runs only if the Kit is ruled "adopt" after the base spike.

**Files touched:** the spike branch's pin (swap zed git → the
`gpui-pre` snapshot) and, through the report, "Phase 2 results" in
[docs/research/portability-plan.md](../docs/research/portability-plan.md)
on main. Never merged as code.

The conditions C1–C5 and the Kit checks' canonical statement live in
the plan record's "gpui-kit" section; run them from there.

## Task 0: Preconditions gate

**Description:** Both preconditions verified before any work:
`gpui-spike` reported (at least checks 0–4 evidenced on the zed-git
pin) and the Kit ruled "adopt" (open ruling 1; open ruling 2's
recommended timing is exactly this moment — after the base spike, so
the ruling reads the carried-patch ledger). If the Kit was ruled adopt
*before* the spike and the Kit checks already ran inside it, this chunk
is complete by definition — record that and stop.

**Acceptance criteria:**
- [ ] Preconditions hold, or the already-ran case is recorded

**Dependencies:** `gpui-spike` reported; Kit ruled adopt · **Scope:** XS

## Task 1: C1 — the pin swap and base checks re-run

**Description:** Swap the spike branch's pin to the `gpui-pre`
snapshot (exact-pinned, the whole `gpui-pre-*` set; commit the updated
lockfile), then re-run base checks 0–4 on it. The snapshot self-describes
as cut from the zed commit the base spike pinned, but the published
content was never independently diffed — this re-run is the fidelity
check.

**Acceptance criteria:**
- [ ] Checks 0–4 re-run on the snapshot pin; any behavior differing
      from the zed-pin run is recorded as a snapshot-fidelity finding

**Verification:**
- [ ] Manual on COSMIC (fractional-scale output for check 1) and sway

**Dependencies:** Task 0 · **Scope:** M

## Task 2: C2a — consumer feature wiring

**Description:** The consumer build maps a `LayerShell` window through
gpui-pre's `wayland` features. The manifest-level wiring was confirmed
2026-11-10 (`gpui-pre` 0.3.12 declares `wayland`; `gpui-pre-platform`
forwards to its Linux crate); the consumer build is the remaining
proof.

**Acceptance criteria:**
- [ ] The scratch crate, depending only on gpui-pre crates, opens and
      maps a `LayerShell` window

**Verification:**
- [ ] Manual on COSMIC

**Dependencies:** Task 1 · **Scope:** S

## Task 3: C2b — Kit overlays on a layer-shell surface

**Description:** Root + Kit overlays (menu, tooltip) render and behave
on a `LayerShell` surface during the 80 ms reshape cadence. This is the
Kit's kill check (no field evidence existed either way; kill-risk 5) —
a failure here is a Kit NO-GO *for this pin* and leaves GPUI's verdict
as it stands.

**Acceptance criteria:**
- [ ] Overlays render and behave through sustained reshapes; evidence
      recorded

**Verification:**
- [ ] Manual on COSMIC

**Dependencies:** Task 2 · **Scope:** M

## Task 4: C4 — the theme generator

**Description:** The scheme → theme-JSON generator (a spike artifact)
applies through `ThemeRegistry`/`apply_config` on the snapshot pin —
DropTerm's scheme colors feeding the Kit's theme tokens. (Themes are
`#[non_exhaustive]` since v0.9.0; no struct literals.)

**Acceptance criteria:**
- [ ] A DropTerm scheme renders the terminal chrome through the
      generated theme; the generator's code is committed on the spike
      branch as a Phase 3 input

**Verification:**
- [ ] Manual on COSMIC

**Dependencies:** Task 2 · **Scope:** M

## Task 5: The Kit verdict

**Description:** A Kit GO or NO-GO for the pin, appended to the spike
report ("Phase 2 results" on main) with per-check evidence. A NO-GO
leaves GPUI's verdict as it stands and the chrome hand-built (the
plan's chrome inventory is that backlog). Either outcome leaves the six
seams and the roadmap unchanged. Component-only adoption holds
throughout: gpui-shell never enters the graph (C3).

**Acceptance criteria:**
- [ ] Verdict appended with evidence; C3 (no gpui-shell) holds in the
      spike's manifest

**Verification:**
- [ ] Manual: the report arms Phase 3's pin decision (snapshot vs zed
      git per the Kit ruling)

**Dependencies:** Tasks 1–4 · **Files:** plan record ("Phase 2
results" only) · **Scope:** S
