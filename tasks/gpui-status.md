# Chunk: gpui-status — the status band on GPUI

Spec: [SPEC-gpui-status.md](../SPEC-gpui-status.md) · Module id:
`gpui-status` · Phase 3 · branch `port/gpui-status` (after
`gpui-terminal`; parallel with `gpui-tabs`).

**Files touched:** the `dropterm-gpui` status-band module. Consumed from
core: the segment model and events, jiff clock arithmetic. Read-only
reference: `src/status_bar.rs` (the libcosmic behavior set).

## Planning rulings (made at /planning; veto at plan review)

- **OSC stays engine-side**: the band renders core events; if a segment
  needs a new event core doesn't emit, the event lands in core first with
  its test — the view never parses escape sequences.

## Task 1: Band layout, clock, and base segments

**Description:** The band's layout and truncation rules, the clock's
minute-boundary tick, and the base segments (working directory, profile,
custom) from core's model. Headless tests pin segment state →
presentation.

**Acceptance criteria:**
- [x] Band renders per config (show/hide, segments) on both compositors;
      clock ticks at the minute boundary
      *(walked 2026-11-14 — rig target/rig-evidence-gn1/ runs a-e, VM
      target/vm-evidence-gn1/ runs a-c: run A holds every base seat —
      the filled disc with `sleep`'s comm (an interactive bash's
      job-control child; the exec-class seed's hollow disc beside it
      is the engine's own pgrp rule reading idle, the honest
      fallback), the `~`-collapsed path, its git branch, the tab
      position, the clock — and two shots straddling a minute
      boundary: the log carries the flip at +25 864 ms (boot
      18:39:34 → 18:40:00.2, the boundary itself) with 631 px
      differing exactly at the clock's corner; run B
      (`status_bar_enabled: false`) drops the band whole — the
      1-tab strip's corner affordances alone at the free edge; run C
      (`clock/path/branch: false`) takes the named seats and only
      them, the grid honestly absent at its spawn default; the VM's
      run A pixels carry the same set — • sleep · ⌂ ~/gn-walk ·
      vm-main · ▤ 2/2 · ◷ 17:33 beside the logged arm — with B/C
      gated silent in the logs)*

**Verification:**
- [x] Tests pass: `just check`
- [x] Manual: band render walk on rig + VM

**Dependencies:** `gpui-terminal` merged · **Files:** the status-band
module · **Scope:** M

## Task 2: OSC-fed segments and config rules

**Description:** OSC-fed content (working directory via OSC 7-class,
command exit codes via the 133;D forks) rendered as it arrives; the
config-driven content rules; click actions where the libcosmic band has
them.

**Acceptance criteria:**
- [ ] Exit-code segments surface per the libcosmic behavior; OSC updates
      land live

**Verification:**
- [ ] Manual: status-band rows of the checklist on rig + VM

**Dependencies:** Task 1 · **Files:** the status-band module · **Scope:**
M

## Task 3: Rows and merge

**Description:** The status-band rows walked on COSMIC + sway; merge.

**Acceptance criteria:**
- [ ] Band rows pass on both compositors
- [ ] `just check` green on the merge result

**Verification:**
- [ ] Manual: band rows, both compositors

**Dependencies:** Task 2 · **Files:** the branch's diff · **Scope:** S
