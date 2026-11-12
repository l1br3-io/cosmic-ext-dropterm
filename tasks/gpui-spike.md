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
      entry per carried need *(sway-witness data banked 2026-11-12,
      spike repo 5422181, headless Debian-trixie container: check 1's
      two-surface show measured OPEN→FIRST_FRAME 121/117/119ms — inside
      the 130ms budget in a debug build on software GL, the measure
      surface warming wgpu pipelines being what buys it (cold
      single-window: 240ms); check 2's 80ms-cadence resize landed every
      height in one configure round-trip once the request carried a real
      width (explicit heights survive — no #64994 behavior on sway);
      check 6's `cx.displays()` populated at startup; check 7 saw one
      echo artifact — a 0-width first frame before the compositor
      configure — then no double-reporting; check 5 inconclusive
      headless (keyboard-less session grants no activation —
      environment artifact); #61368 fractional variant at sway scale 1.5:
      no disconnect, 119ms, but `cx.displays()` reads the rounded-up
      integer wl_output scale (960x540 vs the compositor's true
      1280x720 logical) — the measure-free show would mis-size on
      fractionally scaled outputs, the two-surface show stays correct; a
      real input to check 1's path choice. COSMIC halves remain
      owner-hardware rows; check 2b banked sway-side 2026-11-13 (spike
      d59eaf4 + 95bff7c, `SPIKE_CHECK=recreate`, rig scripted at
      rig/sway-headless.sh): removing the shown surface and opening the
      edited one in one event-loop turn measured RECREATE
      REMOVE->FIRST_FRAME 142/133/146ms across three runs — the same
      class as the show itself plus ~20ms, so a geometry edit that
      recreates costs about one extra show; the edited geometry landed
      exactly (L|B 800x240, margin 80); focus-return inconclusive
      headless (no seat — check 5's artifact), the COSMIC row decides
      it; debug build on software GL, same caveat as every sway number)*
- [ ] Check 1's chosen path has its creation-to-first-frame latency
      measured against the 130 ms slide (`SLIDE_DURATION_MS`,
      `src/slide.rs:44`) *(sway: 121/117/119ms debug+software; COSMIC's
      release-build number is the primary one pending)*

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
      sandbox equivalent) *(harness armed at spike aeea7b3:
      `SPIKE_CHECK=detect` — layer-shell-guard's registry probe
      re-expressed as the dispatch verdict, no window; the no-session
      branch verified headless 2026-11-13, clean "could not find
      wayland compositor" + exit 0 — and a found quirk recorded:
      `App::quit` during startup needs a deferred tick to take; the
      three verdict rows run in the COSMIC session, sway/weston can
      carry two of them beforehand if the container is rebuilt)*

**Verification:**
- [ ] Manual across the three environments *(no-session branch done;
      the rest await the session)*

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
      evidence recorded; the `cosmic-text`-role note written *(harness
      armed at spike aeea7b3: `SPIKE_CHECK=grid` — a live `/bin/sh`
      PTY feeds an `alacritty_terminal` Term, DropTerm's patched fork
      pair resolving beside GPUI's tree under the pin (the build itself
      proves the forks-resolve half — the crate-choice question), the
      render walks the grid as per-run colored spans, resize drives
      `Term::resize` + `Msg::Resize`/SIGWINCH, an ANSI color pattern
      seeded through the pty gives attributes evidence without keys;
      the `cosmic-text`-role note is written: GPUI's text stack is
      cosmic-text-based at this pin, so DropTerm's engine-side buffers
      have no role on this path — the runtime render rows run in the
      COSMIC session)*

**Verification:**
- [ ] Manual on COSMIC *(build halves green; render rows pending the
      session)*

**Dependencies:** Task 2 · **Scope:** M

## Task 6: Check 10 — D-Bus interop

**Description:** zbus (tokio) single instance + the V1 contract running
beside GPUI's executor — proving the loops coexist
(`dbus_state` is framework-agnostic; this is the coexistence proof).

**Acceptance criteria:**
- [ ] The scratch crate answers `--is-visible`-style queries over the
      bus while rendering *(harness armed at spike aeea7b3:
      `SPIKE_CHECK=dbus` — a zbus 5 + tokio service on its own thread
      owns `io.github.l1br3_io.dropterm.SpikeCheck` with the V1-shaped
      IsVisible/SetVisible pair, the render loop showing the shared
      state; a 30s window for the busctl round-trip, the command
      printed by the harness; zbus version-matched to the 5.19 line
      the gpui tree already compiles — the runtime rows run in the
      COSMIC session)*

**Verification:**
- [ ] Manual on COSMIC *(service code compiled green beside GPUI's
      tree; the round-trip rows pending the session)*

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
- [x] Coexistence result recorded: absorbs, or needs its own workspace
- [x] The experiment commit is reverted; the branch keeps its own
      lockfile

**Verification:**
- [x] Manual: the experiment commit's build outcome on the branch

**Run notes (T8) — RESOLVED 2026-11-13: ABSORBS, after one lockfile
reconciliation.** The one commit on the spike branch is 5c2a1da (table
out), reverted at 974912c; the experiment ran in the main checkout
(working tree only — the copy and the members edit were never
committed, restored after; `git status` clean on both repos at the
end). First resolution failed on exactly one crate:
`unicode-properties` — gpui's `gpui_web` exact-pins `=0.1.3` while the
root lock held 0.1.4 (libcosmic → iced_tiny_skia → resvg → usvg →
rustybuzz wants `^0.1.3`); one lockfile means one version.
`cargo update -p unicode-properties --precise 0.1.3` reconciled it,
the whole gpui tree entered the root lockfile (+2700 lines), and both
halves built green off the one lock: the spike crate (28.5 s) and
dropterm + dropterm-applet (`cargo check`, 14.4 s — the downgrade
costs the existing packages nothing). No `links` conflicts anywhere —
the [patch.crates-io] forks are identical in both manifests, so patch
unification is a non-issue. Runtime was not exercised (no compositor
in the session); the check's criterion is resolution + build.
Verdict for Phase 3: the GPUI frontend can grow on main inside the
root workspace, carrying the 0.1.3 pin as a named condition; the
fallback (own workspace in the same repo, own lockfile) stays the
documented answer if that pin ever stings.

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
