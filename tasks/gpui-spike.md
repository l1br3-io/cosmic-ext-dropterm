# Chunk: gpui-spike — can GPUI carry the lifecycle?

Spec: [SPEC-gpui-spike.md](../SPEC-gpui-spike.md) · Module id:
`gpui-spike` · Phase 2, lane spike · branch `spike/gpui-layer-shell`
(scratch dir `../dropterm-spike`, its own git repo) — never merged as code.

> **Done (owner ruling, 2026-11-13):** the spike closed **GO** — the
> report is [docs/research/gpui-spike.md](../docs/research/gpui-spike.md)
> and the plan's Phase 2 results. Rows not run ride dated park notes
> inside their tasks (checks 12–13 hardware, GNOME's detection branch,
> check 3's direct runtime evaluation). The Kit ruling is the owner's
> and gates `kit-pass`.

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
- [x] The window maps and renders on both compositors; evidence
      (photo/log) captured *(sway half green headless 2026-11-12, the
      lg-3 pattern rebuilt: Debian trixie docker, uid-matched user,
      socket shared to the host, sway 1.10.1, `zwlr_layer_shell_v1` v4
      confirmed via `wayland-info`; the window mapped, rendered frames
      for the full 12s run (stdout log), and exited cleanly under
      `timeout`; geometry honored — full-width bottom band, 200px tall,
      bottom margin 40px, clock text dead-center (grim screenshot
      pixel-checked: lit text centered x≈960, y-band 919–967 inside the
      surface's 840–1040); libEGL "failed to get driver name" warnings
      are the headless-EGL environment's, benign — frames flowed; COSMIC
      half green 2026-11-13, agent-run in the validation VM (walk log
      and screenshots at the spike repo's cosmic-session-logs/, spike
      043eb4e): maps, renders the clock at bottom-center over the
      near-transparent band (vm-cosmic-shape.png), clean exit, the same
      benign 0-width-first-frame echo as sway)*

**Verification:**
- [x] Manual on COSMIC and sway *(sway done headless as above; COSMIC
      done agent-run in the validation VM 2026-11-13, snapshot
      pre-spike-cosmic-walk taken before and restored after — the
      owner's real-hardware confirmation remains welcome but nothing
      gates on it)*

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
- [x] Each check recorded pass/fail with evidence; the ledger has an
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
      it; debug build on software GL, same caveat as every sway number;
      check 8 banked sway-side 2026-11-13 (spike d6d9af5,
      `SPIKE_CHECK=clipboard`): GPUI's wayland clipboard is
      focus-gated, not surface-independent — the pinned source
      (gpui_linux wayland/client.rs:1462) shows write_to_clipboard
      silently no-ops without a mouse- or keyboard-focused window, and
      the offer itself further needs a selection serial from a real
      key/pointer press; observed matching (wl-paste saw nothing in
      either phase, headless has no input so serials never come);
      stronger than today's anchor workaround (bound surface); the
      COSMIC row wants a small harness extension — a key-press-
      triggered write in a typeable window — to watch the offer land
      after a real keystroke)*
- [x] Check 1's chosen path has its creation-to-first-frame latency
      measured against the 130 ms slide (`SLIDE_DURATION_MS`,
      `src/slide.rs:44`) *(sway: 121/117/119ms debug+software; COSMIC
      142ms debug+VM 2026-11-13 (zink→llvmpipe-fallback GL, same
      caveat class) with the measure-free variant at 193ms confirming
      the two-surface path on both compositors; COSMIC release build
      in the same VM: **34/35/34ms** across three runs — comfortably
      inside budget, ~4x under debug, VM+software-GL caveat attached
      and the real-hardware release number still the primary)*

**COSMIC-walk run notes (2026-11-13, spike 043eb4e — agent-run in the
validation VM, every row in cosmic-session-logs/):** check 1 two-surface
show 142ms (budget 130, debug/VM caveat); check 2 every height of the
80ms cycle landed within ~100ms — explicit heights survive on COSMIC
too, no #64994; check 5 **GAINED at +193ms** — COSMIC grants the
exclusive surface activation, green where sway-headless could not test;
check 6 displays 1 at startup, 1280x800 correct; check 7 the single
0-width first frame, no double-reporting; check 2b recreate 138ms with
**B activation GAINED at +141ms after the swap — the recreated surface
re-grabs the keyboard**, open ruling 3's missing fact (geometry edit
honored exactly). **Rerun 2026-11-13 (spike c2c992b+9a0e0a4+354af89,
agent-run in the VM, vm-rerun log) closed three of the four:**
check 1's release-build number — OPEN→FIRST_FRAME **34/35/34 ms**
across three runs (budget 130) against debug's 142 in the same VM:
release buys ~4x, VM+llvmpipe caveat, real hardware stays primary;
check 8's key-trigger row — the `clipboard-key` extension (a typeable
Exclusive window where every KeyDownEvent writes a marker; the API
finding banked: `on_key_down` + `track_focus` on one stateful div is
what receives keys, key dispatch walks root→focused-node only) plus
wtype's virtual keyboard closed the row on BOTH compositors — sway
gains activation only when the keyboard joins the seat (recasting
check 5's "headless grants no activation" as "no keyboard on the
seat"), COSMIC gains it at +12ms with the SPICE keyboard, and
wl-paste read the marker on both: **the offer lands after a real
keystroke**, the gate is exactly focus+serial as pinned; the
fractional-scale COSMIC output — cosmic-randr applied 1.25, the
surface **maps at 1024×200** (fractional honored in geometry) but the
console shows **no trace of the band or clock** where 100% showed the
clock (two runs, screenshots held), and `cx.displays()` reads 640×400
— the rounded-up integer wl_output scale, the same mis-read class as
sway 1.5: right geometry, no visible output, integer-rounded
      metadata. Check 3's direct runtime re-grab row parked (owner
      ruling, 2026-11-13, spike marked done on the GO verdict — the 2b
      recreate path evidences the redesign the GO criteria accept);
      the carried ledger lives in the report,
      [docs/research/gpui-spike.md](../docs/research/gpui-spike.md))*

**Verification:**
- [x] Manual on COSMIC (fractional-scale output included); sway as the
      cross-compositor witness for the checks that differ there
      *(integer-scale COSMIC rows done agent-run in the VM 2026-11-13;
      the fractional-scale output (1.25, applied via cosmic-randr,
      evidence held — geometry honored, visible output missing,
      integer-rounded metadata) and the release-build number
      (34/35/34ms) banked by the same-day rerun; check 3's runtime
      re-grab row parked with the done ruling above)*

**Dependencies:** Task 2 · **Scope:** L (hardware-session work)

## Task 4: Check 11 — backend detection

**Description:** Startup dispatch picks the layer-shell impl on COSMIC
and sway and reports `gnome-toplevel (unimplemented)` on GNOME —
`layer-shell-guard`'s probe, extended to dispatch.

**Acceptance criteria:**
- [x] Detection verdicts recorded on COSMIC, sway, and GNOME (or the
      sandbox equivalent) *(harness armed at spike aeea7b3:
      `SPIKE_CHECK=detect` — layer-shell-guard's registry probe
      re-expressed as the dispatch verdict, no window; the no-session
      branch verified headless 2026-11-13, clean "could not find wayland
      compositor" + exit 0 — and a found quirk recorded:
      `App::quit` during startup needs a deferred tick to take; verdicts
      landed 2026-11-13: **layer-shell on COSMIC** (validation VM walk,
      spike 043eb4e) and **layer-shell on sway** (the scripted rig,
      spike be1b1e8) — the GNOME no-layer-shell branch parked with the
      spike's done ruling (2026-11-13): it rides the parked GNOME
      session or a weston-headless sandbox leg whenever wanted, and the
      dispatch probe's shape is proven regardless)*

**Verification:**
- [x] Manual across the three environments *(no-session, COSMIC, and
      sway done; GNOME parked with the parked session — un-park by
      running it)*

**Dependencies:** Task 2 · **Scope:** S

## Task 5: Check 9 — terminal grid render

**Description:** `alacritty_terminal` grid → GPUI text render: PTY,
colors/attributes, damage redraw, resize→SIGWINCH; the patched
vte/alacritty forks resolve under the pin; record whether DropTerm's
engine-side `cosmic-text` buffers keep any role (this sets the shared
core's boundary — Phase 3's slice 1 input). Zed's `crates/terminal` is
the reference shape.

**Acceptance criteria:**
- [x] A grid renders from a live PTY with attributes and resize;
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
      have no role on this path — **rendered on COSMIC 2026-11-13,
      agent-run in the validation VM (spike 043eb4e)**: the forks
      resolved and ran, the grid drew its first frame 8x20 then resized
      to 128x20 through Term::resize + PTY SIGWINCH, and row0 shows the
      seeded ANSI pattern ("BLUE…" and the RED/GREEN/BLUE printf) —
      vm-cosmic-grid.png and the walk log held in cosmic-session-logs/;
      the optional owner-typing pass adds color but nothing gates on
      it)*

**Verification:**
- [x] Manual on COSMIC *(agent-run in the validation VM 2026-11-13,
      snapshot discipline kept; an owner's typeable pass on real
      hardware remains welcome)*

**Dependencies:** Task 2 · **Scope:** M

## Task 6: Check 10 — D-Bus interop

**Description:** zbus (tokio) single instance + the V1 contract running
beside GPUI's executor — proving the loops coexist
(`dbus_state` is framework-agnostic; this is the coexistence proof).

**Acceptance criteria:**
- [x] The scratch crate answers `--is-visible`-style queries over the
      bus while rendering *(harness armed at spike aeea7b3:
      `SPIKE_CHECK=dbus` — a zbus 5 + tokio service on its own thread
      owns `io.github.l1br3_io.dropterm.SpikeCheck` with the V1-shaped
      IsVisible/SetVisible pair, the render loop showing the shared
      state; zbus version-matched to the 5.19 line the gpui tree
      already compiles. The 2026-11-13 COSMIC walk found a harness
      bug — "service up" printed but the name never owned — and the
      rerun pinned the root cause: **not GPUI, a dropped connection** —
      the zbus `Connection` was bound as `_conn` inside a match arm,
      so it dropped microseconds after the println, releasing the
      name; the no-GPUI isolate bin worked because its binding lives
      across the sleep, and the executor-drop theory retired. Fix
      (spike c2c992b) holds the connection across the serve loop and
      self-verifies with a NameHasOwner row. Round-trip green on BOTH
      compositors 2026-11-13: sway rig (rig-dbus-fix log) and COSMIC
      in the validation VM (vm-rerun log) — name owned, IsVisible
      `b true`, SetVisible flips the render live and back while
      rendering, clean exits. The coexistence proof holds: zbus's
      tokio loop and GPUI's executor run beside each other. Run-sheet
      correction also landed: IsVisible is a method, so the read side
      is `busctl call`, not `get-property`)*

**Verification:**
- [x] Manual on COSMIC *(agent-run in the validation VM 2026-11-13,
      spike c2c992b+354af89, snapshot discipline kept; sway rig as the
      cross-compositor witness)*

**Dependencies:** Task 2 · **Scope:** S

## Task 7: Checks 12–13 — X11 and macOS PopUp

> **Parked (owner ruling, 2026-11-13):** the spike was ruled done on
> the GO verdict — the Wayland-primary criteria never required these
> rows. Un-park by running an X11 session (the crate must also enable
> gpui's `x11` feature, dropped per the spec's wayland-only scope) and
> a Mac sitting; the findings gate Phase 4/5 mechanisms either way.

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
- [x] Every report element above present; verdict matches the recorded
      check results *(reported 2026-11-13 as
      [docs/research/gpui-spike.md](../docs/research/gpui-spike.md) —
      GO per the criteria: 0/1/2/5/6 pass on both compositors, 4 holds,
      3 workable via the evidenced recreate redesign; the pin identity,
      per-check evidence, two-entry carried ledger, show latency
      (34/35/34ms release vs the 130 budget), coexistence (ABSORBS),
      and 2b's recreate cost + focus-return all present; the plan
      record's "Phase 2 results" filled from it)*
- [x] If the Kit was not yet ruled, the report arms the Kit ruling
      (open ruling 2's recommended timing is after this report)
      *(the report's recommendation names the Kit ruling as the owner's
      remaining gate on `kit-pass`/kp-0; no timing forced)*

**Verification:**
- [ ] Manual: the owner reads the report and rules on the Kit (if not
      yet) and on live geometry edits; Phase 3's slices get ids and
      specs *(the report is in; the reading and the rulings are the
      owner's — the chunk closes on the done ruling 2026-11-13 with
      this row named as the handoff)*

**Dependencies:** Tasks 2–8 · **Files:**
`docs/research/portability-plan.md` ("Phase 2 results" only) ·
**Scope:** S

## Checkpoint: the spike report

- [ ] Verdict + ledger + evidence on main; the Kit ruling made; Phase 3
      slice ids assigned
