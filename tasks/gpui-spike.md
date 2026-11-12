# Chunk: gpui-spike — can GPUI carry the lifecycle?

Spec: [SPEC-gpui-spike.md](../SPEC-gpui-spike.md) · Module id:
`gpui-spike` · Phase 2, lane spike · branch `spike/gpui-layer-shell`
(scratch dir `../dropterm-spike`, its own git repo) — never merged as code.

**Files touched:** `spikes/gpui-layer-shell/**` (new, on the spike
branch only; own `[workspace]`, own lockfile, both committed on the
branch) and `docs/research/portability-plan.md` on main — only the
"Phase 2 results" section, appended by the report.

The sixteen base checks (0, 1, 2, 2b, 3–14) with their pass criteria
and fallbacks live in the committed plan record — run them from
[docs/research/portability-plan.md](../docs/research/portability-plan.md),
"Phase 2: the spike checks", never from this chunk. `just check` does
not apply to the scratch crate; it never runs against the root
`target/`.

## Task 1: Scratch dir and crate

**Description:** Create the scratch dir, its git repo, and branch; scaffold
`spikes/gpui-layer-shell/` (under ~500 lines when grown, `wayland`
feature only): a scratch crate declaring an empty `[workspace]` table
so its lockfile and GPUI's dependency tree never touch the root
workspace. Pin zed git at the commit the then-current `gpui-pre`
snapshot was cut from (gpui-pre 0.3.12 = zed `d2a4fb6` as of the plan
record — re-verify the mapping at start; the pin covers the whole
`gpui_*` crate set) and commit the lockfile. If the Kit was ruled
"adopt" before the spike starts, pin the snapshot instead and fold the
Kit checks (C2a, C2b, C4) into the pass — see the `kit-pass` chunk.

**Acceptance criteria:**
- [x] `cd spikes/gpui-layer-shell && cargo run` builds and runs against
      the pin *(built green in 1m46s; ran against headless sway — see
      Task 2's note)*
- [x] Lockfile committed on the branch; pin identity recorded in the
      scratch crate's README or main.rs doc comment *(commit 88a64ac on
      `spike/gpui-layer-shell`; pin in both `Cargo.toml` comments and
      `main.rs`'s doc comment)*

**Verification:**
- [x] Manual: the crate builds in its own workspace; the root workspace
      still `just check`s clean *(the scaffold touched no file inside
      the main repo — the scratch crate lives in `../dropterm-spike`'s
      own repo and declares its own empty `[workspace]` — so the root
      workspace's inputs are identical to the `just check`-green tree at
      62aa648; the pin re-verification also held: gpui-pre 0.3.12 still
      newest on crates.io 2026-11-12, = zed `d2a4fb6` per the plan
      record)*

**Dependencies:** none (runs from day one beside everything) ·
**Files:** `spikes/gpui-layer-shell/*` · **Scope:** S

## Task 2: Check 0 — boot and map

**Description:** A `LayerShell` window maps and renders on COSMIC *and*
sway, no crash — the regression check for the 2025-07-30 upstream
revert. Failing this is GPUI NO-GO outright.

**Acceptance criteria:**
- [ ] The window maps and renders on both compositors; evidence
      (photo/log) captured *(sway half green headless 2026-11-12, the
      lg-3 pattern rebuilt: Debian trixie docker, uid-matched user,
      socket shared to the host, sway 1.10.1, `zwlr_layer_shell_v1` v4
      confirmed via `wayland-info`; the window mapped, rendered frames
      for the full 12s run (stdout log), and exited cleanly under
      `timeout`; geometry honored — full-width bottom band, 200px tall,
      bottom margin 40px, clock text dead-center (grim screenshot
      pixel-checked: lit text centered x≈960, y-band 919–967 inside the
      surface's 840–1040); libEGL "failed to get driver name" warnings
      are the headless-EGL environment's, benign — frames flowed; the
      COSMIC half is an owner-hardware row)*

**Verification:**
- [ ] Manual on COSMIC and sway *(sway done headless as above; COSMIC
      awaits the owner's session)*

**Dependencies:** Task 1 · **Scope:** S

## Task 3: Lifecycle checks 1–8 and 2b on COSMIC

**Description:** The crux table, on COSMIC with one fractionally scaled
output: measure-and-shape at creation (check 1, the two-surface show vs
the measure-free show; the #61368 disconnect risk), in-place reshape at
the 80 ms drag cadence (2, #64994), live geometry edit via recreation
(2b), keyboard interactivity at runtime (3, carried #61420 vs redesign
vs keep-held), layer and zone at creation (4), focus events (5), output
targeting (6), resize echo (7), and clipboard surface-independence (8).
Start the carried-patch ledger here: every check that needs #61368,
#64994, #61420, or a self-written setter gets its ledger entry.

**Acceptance criteria:**
- [ ] Each check recorded pass/fail with evidence; the ledger has an
      entry per carried need
- [ ] Check 1's chosen path has its creation-to-first-frame latency
      measured against the 130 ms slide (`SLIDE_DURATION_MS`,
      `src/slide.rs:44`)

**Verification:**
- [ ] Manual on COSMIC (fractional-scale output included); sway as the
      cross-compositor witness for the checks that differ there

**Dependencies:** Task 2 · **Scope:** L (hardware-session work)

## Task 4: Check 11 — backend detection

**Description:** Startup dispatch picks the layer-shell impl on COSMIC
and sway and reports `gnome-toplevel (unimplemented)` on GNOME —
`layer-shell-guard`'s probe, extended to dispatch.

**Acceptance criteria:**
- [ ] Detection verdicts recorded on COSMIC, sway, and GNOME (or the
      sandbox equivalent)

**Verification:**
- [ ] Manual across the three environments

**Dependencies:** Task 2 · **Scope:** S

## Task 5: Check 9 — terminal grid render

**Description:** `alacritty_terminal` grid → GPUI text render: PTY,
colors/attributes, damage redraw, resize→SIGWINCH; the patched
vte/alacritty forks resolve under the pin; record whether DropTerm's
engine-side `cosmic-text` buffers keep any role (this sets the shared
core's boundary — Phase 3's slice 1 input). Zed's `crates/terminal` is
the reference shape.

**Acceptance criteria:**
- [ ] A grid renders from a live PTY with attributes and resize;
      evidence recorded; the `cosmic-text`-role note written

**Verification:**
- [ ] Manual on COSMIC

**Dependencies:** Task 2 · **Scope:** M

## Task 6: Check 10 — D-Bus interop

**Description:** zbus (tokio) single instance + the V1 contract running
beside GPUI's executor — proving the loops coexist
(`dbus_state` is framework-agnostic; this is the coexistence proof).

**Acceptance criteria:**
- [ ] The scratch crate answers `--is-visible`-style queries over the
      bus while rendering

**Verification:**
- [ ] Manual on COSMIC

**Dependencies:** Task 2 · **Scope:** S

## Task 7: Checks 12–13 — X11 and macOS PopUp

**Description**: The off-Wayland assumption: a `PopUp` window works
topmost + positioned + focus-grabbing on an X11 session (12); `PopUp`
over a fullscreen app and across Spaces plus a native global-hotkey
registration on the Mac (13). Kill-risk 3 in the plan's assumptions.

**Acceptance criteria:**
- [ ] Both checks recorded with evidence (these inform Phase 4's X11
      backend and Phase 5's macOS design either way)

**Verification:**
- [ ] Manual on an X11 session and on the Mac

**Dependencies:** Task 1 · **Scope:** M (two hardware environments)

## Task 8: Check 14 — workspace coexistence experiment

**Description:** The one-commit experiment: drop the scratch crate's
empty `[workspace]` table on the spike branch and let the root
workspace absorb the crate beside the libcosmic packages — does it
resolve and build (one lockfile, no conflicting `links` between the two
dependency trees)? Revert the commit after; the result decides how the
GPUI frontend shares the repository during Phase 3.

**Acceptance criteria:**
- [ ] Coexistence result recorded: absorbs, or needs its own workspace
- [ ] The experiment commit is reverted; the branch keeps its own
      lockfile

**Verification:**
- [ ] Manual: the experiment commit's build outcome on the branch

**Dependencies:** Task 2 · **Scope:** S

## Task 9: The report

**Description:** Append the report to "Phase 2 results" on main: the
verdict per the plan's decision criteria (GO: 0, 1, 2, 5, 6 pass with
at most the ledger's patches, 3 workable, 4 holds; REDESIGN: reworked
show or degraded drag, keyboard re-grab redesigned — the state machine
behind `surface-intents` survives, executors change; NO-GO/fork: 0
fails, or 3 fails with no focus-return-preserving redesign); the pin
identity; per-check evidence; the carried-patch ledger; the measured
show latency against the 130 ms slide; the coexistence result; the
recreate cost open ruling 3 needs (flicker, re-slide, focus — from
check 2b). Only "Phase 2 results" is edited on main.

**Acceptance criteria:**
- [ ] Every report element above present; verdict matches the recorded
      check results
- [ ] If the Kit was not yet ruled, the report arms the Kit ruling
      (open ruling 2's recommended timing is after this report)

**Verification:**
- [ ] Manual: the owner reads the report and rules on the Kit (if not
      yet) and on live geometry edits; Phase 3's slices get ids and
      specs

**Dependencies:** Tasks 2–8 · **Files:**
`docs/research/portability-plan.md` ("Phase 2 results" only) ·
**Scope:** S

## Checkpoint: the spike report

- [ ] Verdict + ledger + evidence on main; the Kit ruling made; Phase 3
      slice ids assigned
