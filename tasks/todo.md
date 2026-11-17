# Todo — aggregate of the chunks

Detail (acceptance, verify, files) lives in each
[tasks/<id>.md](plan.md) chunk; this is the cross-module order. Gated
items carry their gate inline. Never mark an item done without its
chunk's verification passing.

## Phase 0 — reality-check (hardware; parallel-safe)

- [x] rc-1 Session 1: COSMIC baseline (16 checks, slot filled) *(agent-run 2026-11-11 in the validation VM, 9091b19: 1/4/8/12/13/15 pass, input rows n-a — owner's eyes-on, parked 2026-11-13)*
- [x] rc-2 Session 2a: sway (shared with the spike) *(agent-run headless, 1deed62: green where exercisable — maps/renders/lifecycle/handoff; input rows + bell row want one hardware sway session — parked
      2026-11-13)*
- [x] rc-3 Session 2b: Hyprland *(waived — owner ruling 2026-11-15
      closing Phase 0 with the sessions unrun; the parked note stands: a
      headless-docker attempt may still carry it the way sway's session
      went if Hyprland is ever questioned)*
- [x] rc-4 Session 3: KDE Plasma (Wayland) *(waived with the phase,
      owner ruling 2026-11-15 — the smoke 2026-11-15 owner-run on a
      Fedora KDE VM against the GPUI frontend remains the only KDE
      evidence: layer-shell works, hidden mode clean with no frame; with
      dock mode on, a KWin rounded frame drew around the drawer —
      dock-on/off correlation, mechanism unconfirmed; an informal smoke,
      not a session fill, and still not one)*
- [x] rc-5 Session 4: GNOME — how it fails (feeds layer-shell-guard)
      *(waived with the phase, owner ruling 2026-11-15 — expected-fail
      and confirmatory; the guard's weston-headless evidence stands as
      the no-layer-shell record)*
- [x] rc-6 Session 5: one X11 session (record which) *(i3 over Xvfb, 482a37d: zombie + degenerate 32px DOCK band first launch, truthful --is-visible, libXi-less panic variant; expected-fail recorded)*
- [x] rc-7 Session 6: non-Pop distro — floor, ldd, fonts (feeds
      distro-reach) *(run 2026-11-11 on archlinux:latest, slot filled
      8328d51: the artifact's own floor is GLIBC_2.39, source build
      green, ldd under-reports through winit's dlopen, fontless base;
      this box only catches todo up — the chunk and plan record
      already carried it)*
- [x] rc-8 Close-out: every failure filed and named in its slot
      *(parked 2026-11-13 with the three open sessions — it sweeps
      what they fill — closed by the same 2026-11-15 ruling: the waived
      sessions filed nothing to sweep; the filled slots' failures are
      named and the known class carries the layer-shell-guard fix)*

## Phase 1 — app-identity (flag day; merges first)

- [x] ai-1 Owner rulings: app id, applet names, repo-rename timing
      *(decided 2026-11-10, pre-distribution; recorded in the chunk and
      the merge description)*
- [x] ai-2 Code identity sweep (APP_ID, dbus literals, crate names,
      applet ids, clap/argv literals)
- [x] ai-3 Packaging/CI/justfile (res/ renames, workflows, vm.sh,
      completions.toml)
- [x] ai-4 Docs + release skill (README "From a release" untouched)
- [x] ai-5 The rg audit; survivors listed *(61 matches, all
      deliberate survivors — list in the chunk and the merge
      description)*
- [x] ai-6 Manual verification; merge with survivors + break note
      *(merged f698083, 2026-11-11, local --no-ff; verbs + applet
      walked in the validation VM per the chunk; the push of main and
      CI green stay the owner's)*

**Checkpoint after app-identity:** just check + CI green; installed
build toggles under the new name; applet follows.

## Phase 1 — lane B (worktree ../ced2; starts day one)

- [x] cs-1 Store module (load/atomic/quarantine per section; temp-dir
      tests) *(merged f5199ec)*
- [x] cs-2 Config model off the derive; three sections; 40 fixtures
      moved *(merged f5199ec)*
- [x] cs-3 Wire the app (writers, completions path, echo apply kept)
      *(merged f5199ec; the chunk's drawer-write/restart manual row
      stays the owner's — parked 2026-11-13)*
- [x] cs-4 External-edit reload — gate: the `notify` dependency ask
      (fallback re-read at show) *(notify sanctioned 2026-11-10;
      merged f5199ec)*
- [x] cs-5 cosmic-config leaves; docs path; manual rows; merge
      *(merged f5199ec; the sudo-installed manual rows stay the
      owner's — parked 2026-11-13)*
- [x] cs-6 Gate (COSMIC hardware): the dbus-config feature decision
      *(decided and landed 2026-11-11, 576d3bc: the feature drops —
      theme follow verified live on the fallback watcher in the
      validation VM; the chunk's boxes were already checked, this box
      only catches todo up)*
- [x] ct-1 Bus-name ownership at startup (replaces
      run_single_instance; activation shape kept — planning ruling)
      *(e16a976)*
- [x] ct-2 Second-launch forwarding; exit codes unchanged *(c7e2a4f;
      the one deliberate delta — stale owner refuses exit 1, not a
      duplicate — pinned by forward_outcome after the ship review)*
- [x] ct-3 Contract-surface pin test *(616cba1, mutation-checked)*
- [x] ct-4 Drop single-instance; applet canary; merge *(d4856e5 +
      c5de59a review fixes; merged e6ad23b, plan record updated
      206d6a7; applet canary an owner row — parked 2026-11-13)*

**Checkpoint — the one release:** app-identity + config-store in one
release; no break note (owner ruling 2026-11-12: pre-distribution, no
users). *Deferred 2026-11-12 (owner): no
release cut yet — building continues; the release, when cut, carries
everything merged by then.*

## Phase 1 — lane A (worktree ../ced, after app-identity)

- [x] si-1 Intent vocabulary + trait + recording executor; first
      headless show/hide *(ba58934)*
- [x] si-2 Libcosmic executor; translation pin (request order included)
      *(5005948)*
- [x] si-3 Decision layer migrates; ~25 tests window::Id-free
      *(08a5c9e)*
- [x] si-4 Handlers emit intents; full-lifecycle headless test
      *(818fbcb; docs fd5f155)*
- [x] si-5 Manual surface rows on COSMIC (no behavior change); merge
      *(merged 00cf77d, plan record e4a9ac7; ship review's findings
      landed first as 3e30c35, just check green; the surface rows were
      walked on the validation VM per the chunk — the chunk's
      merge-result check row predates the merge and the mid-slide
      reversal's keybind press stays a human-at-console row — parked
      2026-11-13)*

**Checkpoint:** the lifecycle is headless — a spike REDESIGN survives.

## Phase 1 — lane C (main checkout)

- [x] lg-1 Verdict function + unit tests (pure, headless)
- [x] lg-2 Startup probe — gate: the wayland-client dependency ask;
      known-issues row *(direct version-matched dep taken on best
      judgment — the ask went unanswered mid-run, flagged to the
      owner; headless smoke green, and 2026-11-11 the weston-headless
      smoke closed the layer-shell-absent row: message + exit 1, no
      probe reason, registry confirmed 19 globals without the
      protocol; the dep ask ratified 2026-11-11 by the owner — the direct wayland-client edge stands as merged)
- [x] lg-3 COSMIC/sway unchanged; merge (before ct-1 on main.rs, or
      rebase after) *(done 2026-11-11: rebased onto app-identity's
      main — guard prefix/URL carry the new identity — `/ship` fan-out
      clean, sway verified headless in docker (truth table identical
      to COSMIC's, serve race identical to a main-built control),
      merged f73cf27, just check green on the result; the
      wayland-client dep ask still stands open to the owner's veto
      before any push)*
- [ ] kd-1 docs/summon.md — gate: Phase 0 rows + app-identity; every
      row cites its session *(parked 2026-11-13, owner ruling — rides
      the parked sessions; the least-work un-park writes it with the
      three filled sessions' rows and untested marks for the rest)*
- [x] cd-1 docs/dbus.md derived from src/dbus_state.rs *(every claim
      traced to the source; the review itself caught two draft
      errors — a bare `dropterm` and the applet's click both send
      `ActivateAction("toggle", [])`, the CLI never fires the plain
      `Activate` member)*
- [x] cd-2 every_dbus_member_is_documented pinning test *(served
      introspection over the p2p harness is the expectation list;
      mutation-checked)*
- [x] cd-3 README index; merge *(merged 5bf146b, local --no-ff; the
      ship fan-out's findings landed first as 3e4783c — the applet
      proxy claim truthed, the "additive growth" promise dropped, the
      docs pin vacuous-pass-guarded; optional notes recorded in the
      chunk as follow-ups)*
- [ ] dr-1 Facts brief from session 6 *(parked 2026-11-13, owner
      ruling — its gate is actually filled (rc-7), so this is free
      desk work whenever un-parked)*
- [ ] dr-2 Owner rulings: glibc floor; channels *(parked 2026-11-13,
      owner ruling — deferred with the release)*
- [ ] dr-3 Floor ruling implemented (README states it either way)
      *(parked with dr-2, 2026-11-13)*
- [ ] dr-4 Channel recipes (contingent on dr-2; nothing published)
      *(parked with dr-2, 2026-11-13)*
- [ ] dr-5 Release-notes line; merge *(parked with dr-2, 2026-11-13 —
      rides the deferred release)*

**Checkpoint — end of Phase 1:** the plan's exit line; Phase 0 slots
filled; the release cut. *(The exit line itself is parked with the
2026-11-13 ruling — the release stays deferred, the open rows carry
dated park notes, and Phase 2 proceeds independently.)*

## Phase 2 — the spike (worktree ../dropterm-spike; from day one)

- [x] gs-1 Worktree + scratch crate (own workspace, lockfile committed,
      pin recorded) *(88a64ac on the spike repo's
      `spike/gpui-layer-shell`; pin zed `d2a4fb6` re-verified current —
      gpui-pre 0.3.12 still newest on crates.io 2026-11-12; built green
      in 1m46s)*
- [x] gs-2 Check 0: boot and map on COSMIC + sway *(sway half green
      headless 2026-11-12 over the rebuilt lg-3 container pattern —
      mapped, rendered, geometry honored, clean exit, grim screenshot
      pixel-checked; COSMIC half green 2026-11-13 agent-run in the
      validation VM — spike 043eb4e, walk log + screenshots in
      cosmic-session-logs/, snapshot restored after)*
- [x] gs-3 Checks 1–8, 2b on COSMIC (ledger starts; latency vs 130 ms)
      *(sway-witness data banked 2026-11-12, spike repo 5422181:
      two-surface show 121/117/119ms — inside the budget, debug+software
      GL; 80ms-cadence resize honored; displays populated; one 0-width
      first-frame echo; #61368 no-disconnect at sway 1.5, but
      `cx.displays()` mis-sizes fractional outputs (integer-scale read)
      — two-surface show immune, measure-free not; 2b banked sway-side
      2026-11-13, spike d59eaf4+95bff7c: recreate 142/133/146ms — one
      extra show's worth, geometry edit honored, focus the COSMIC row;
      check 8 sway-side: clipboard is focus-gated, not
      surface-independent (source-read client.rs:1462 + observed);
      COSMIC walk 2026-11-13 agent-run in the validation VM (spike
      043eb4e): show 142ms debug/VM, reshape all heights, focus GAINED
      +193ms, recreate 138ms with the re-grab GAINED +141ms — open
      ruling 3's fact; COSMIC rerun 2026-11-13 (spike c2c992b+
      9a0e0a4+354af89, vm-rerun log): release-build number banked —
      34/35/34ms across three runs, comfortably inside the 130ms
      budget (~4x under debug in the same VM), VM+software-GL caveat;
      check 8's key-trigger closed on BOTH compositors — the
      clipboard-key extension + wtype's virtual keyboard, wl-paste
      read the marker on sway and COSMIC, the gate is focus+serial
      exactly as pinned (and sway-headless activation recast: no
      keyboard on the seat, not no activation); fractional COSMIC
      output banked — 1.25 maps the surface at 1024x200 but nothing
      visible lands on screen (screenshots held) with displays
      integer-rounded to 640x400, the same mis-read class as sway 1.5;
      check 3's direct runtime re-grab row parked (owner ruling,
      2026-11-13, with the spike marked done — the 2b recreate path
      evidences the redesign the GO criteria accept; un-park by running
      the non-recreate evaluation if a runtime re-grab is ever wanted)*
- [x] gs-4 Check 11: backend detection verdicts *(harness armed,
      aeea7b3: `SPIKE_CHECK=detect`; no-session branch verified
      headless; verdicts landed 2026-11-13 — layer-shell on COSMIC
      (validation VM) and on sway (the scripted rig, be1b1e8); the
      GNOME no-layer-shell branch parked with the spike's done ruling
      (2026-11-13) — it rides the parked GNOME session, and the
      dispatch probe's shape is proven regardless)*
- [x] gs-5 Check 9: terminal grid render (sets the shared-core
      boundary) *(harness armed, aeea7b3: `SPIKE_CHECK=grid`; the
      patched forks proven to resolve beside GPUI's tree by the build
      itself; the cosmic-text-role note written — no engine-side role
      on GPUI's path; rendered on COSMIC 2026-11-13 agent-run in the
      validation VM — forks ran, grid drew 8x20→128x20 through
      Term::resize+SIGWINCH, seeded ANSI pattern visible in row0,
      screenshot held)*
- [x] gs-6 Check 10: zbus + V1 beside GPUI *(harness armed, aeea7b3:
      `SPIKE_CHECK=dbus`; the V1-shaped service compiled green on its
      own tokio thread beside GPUI; the 2026-11-13 walk found a
      harness bug — "service up" printed but the name never owned —
      and the rerun pinned it: the zbus Connection was bound as
      `_conn` inside a match arm and dropped at the arm's end,
      releasing the name (not GPUI; the isolate bin's binding lived
      across the sleep, the executor-drop theory retired); fix spike
      c2c992b holds the connection and self-verifies with a
      NameHasOwner row; round-trip green on BOTH compositors — sway
      rig (rig-dbus-fix log) and COSMIC in the VM (vm-rerun log):
      name owned, IsVisible b true, SetVisible flipping the render
      live and back while rendering, clean exits — zbus's tokio loop
      and GPUI's executor coexist; also corrected: IsVisible is a
      method, not a property, so the read side is busctl call)*
- [ ] gs-7 Checks 12–13: X11 and macOS PopUp *(parked, owner ruling
      2026-11-13, with the spike marked done on the GO verdict — the
      Wayland-primary criteria never required them; un-park by running
      an X11 session (the crate must also enable gpui's `x11` feature,
      dropped per the spec's wayland-only scope) and a Mac sitting;
      they gate Phase 4/5 mechanisms, not Phase 3)*
- [x] gs-8 Check 14: coexistence experiment (one commit, reverted)
      *(run 2026-11-13, spike 5c2a1da reverted 974912c: ABSORBS — one
      lockfile reconciliation (unicode-properties 0.1.3, gpui_web's
      exact pin vs libcosmic's resvg chain), both trees built green off
      the one lock, no links conflicts, the identical [patch] forks a
      non-issue; Phase 3's frontend can live in the root workspace
      carrying that pin)*
- [x] gs-9 The report → "Phase 2 results" on main *(reported
      2026-11-13: [docs/research/gpui-spike.md](../docs/research/gpui-spike.md)
      carries the GO verdict, pin identity, per-check evidence, and the
      two-entry carried ledger; the plan record's Phase 2 results
      section filled from it; the Kit ruling stays with the owner and
      gates kit-pass/kp-0)*
- [x] kp-0 Gate: spike reported AND Kit ruled adopt (else stop) *(held
      2026-11-13: spike reported GO — checks 0–4 evidenced on the zed-git
      pin per the report; Kit ruled adopt, owner ruling 1 made at the
      `/build auto` gate at open ruling 2's recommended timing (after the
      base spike read the carried ledger); already-ran case n/a — the base
      spike never pulled Kit crates)*
- [x] kp-1 C1: pin swap to gpui-pre; checks 0–4 re-run *(done 2026-11-13,
      spike d722652 — whole 20-crate set at =0.3.12 as the Kit consumes
      it; fidelity twice over: first independent source diff = 11/16
      byte-identical incl. the whole wayland backend, one real delta
      (gpui-pre-wgpu's wgpu 30.0.0→30.0.2 adaptation), and the behavior
      re-run shows it invisible — release 32/36/34ms (base 34/35/34),
      recreate 139ms + re-grab +142ms (base +141), fractional row
      identical; checks 0–4 pass on both compositors)*
- [x] kp-2 C2a: consumer LayerShell through gpui-pre *(done 2026-11-13,
      spike fef2790 — spikes/kit-consumer/, graph exactly gpui-pre +
      gpui-pre-platform, maps the drawer shape green on sway-headless
      and COSMIC-VM, screenshot pixel-verified; the manifest
      confirmation + this build close C2a)*
- [x] kp-3 C2b: Kit overlays on layer-shell at 80 ms cadence *(done
      2026-11-13, spike 40fe302 — GREEN on both compositors: menu
      popover + tooltip render on the layer surface through the full
      cadence, never closing; C3 holds (no gpui-shell/gpui-kit edges);
      adoption debt found: the Kit's Root auto-borders every
      Client-decorated window and wayland forces Client on all
      surfaces, inflating the layer surface 2×20 both axes (1320×360
      for a 1280×320 drawer, both compositors) with no public opt-out
      — edge-to-edge needs a Kit carried patch or accepted non-flush
      edges; goes to the verdict)*
- [x] kp-4 C4: theme generator through ThemeRegistry *(done 2026-11-13,
      spike cd06740 — theme_gen.rs maps the Alacritty Colors scheme onto
      37 ThemeConfig JSON keys, no invented colors, 4 RED-first unit
      tests green; applied via from_value → Theme::update(apply_config)
      on sway + COSMIC with tokens reading back the scheme, and the
      chrome rendering through them (dark band, blue link, dark popover
      on the screenshot) — the generator is committed as a Phase 3
      input)*
- [x] kp-5 Kit verdict appended *(done 2026-11-13 — **Kit GO for the
      pin**: the report's "Kit pass" section + the plan record's Phase 2
      results carry the per-check evidence (C1 twice over, C2a, C2b kill
      check green both compositors, C3 manifest-clean, C4 generator), the
      one carried debt (the Root auto-border's 2×20 px surface inflation,
      no opt-out at this pin), and the armed Phase 3 pin decision — the
      snapshot is the GPUI source; research index row updated)*

**Checkpoint — the spike report:** owner rules on the Kit and live
geometry edits; Phase 3 slices get ids and specs. *(Held 2026-11-13: GO +
Kit GO for the pin; rulings items 7–8; ids and specs below, from
`/planning` the same day.)*

## Phase 3 — the GPUI rewrite (ids from the spike report; main lane)

- [x] sc-1 `dropterm-core` crate; the pure movers (config store, session,
      tabs, profiles, intent vocabulary); zero-toolkit pin test
- [x] sc-2 The split movers — engine minus cosmic-text buffers, geometry,
      keybinds, theme-glue models (sc-1 note: `tabs.rs` deferred here — it
      holds `Mutex<Terminal>`, the spec's engine-state grouping)
- [x] sc-3 Identity + contract move; applet re-points to core
      *(APP_ID already stood in core from sc-1; `dbus_state` moved with
      its 19 tests — the wire tests' futures import swapped off
      `cosmic::iced`, the dbus↔docs pin re-pathed — and the lib target
      retired entirely: `font_list` became a bin module, the applet
      depends on `dropterm-core` with `default-features = false` and
      forwards `dev-identity` straight to core; zero new lock packages)*
- [x] sc-4 Decision layer + transport halves move (Task continuation
      goes executor-neutral) *(the machine, guards, `TOGGLE_DEBOUNCE`,
      `shown_keyboard_mode`, and the slide model moved with 44
      unchanged tests — `ToggleIntent` is the funnel's 3-verb
      vocabulary, the clap `ToggleAction` maps onto it frontend-side;
      the transport halves moved as `core::transport` with
      `forward_to_running` parameterized over the verb's name+words —
      the frontend serializes through `CosmicFlags::args`)*
- [x] sc-5 No-behavior-change VM walk; merge (first Phase 3 merge)
      *(walk green 2026-11-13 on the validation VM, evidence in
      `target/vm-evidence-sc5/`: show 40 % pixel-exact with the block
      caret in the first frame, `sleep 97 &` surviving a hide/show
      cycle, drag live-apply + settle + single write-back (52), live
      in-place edit (70→35, same pid), focus-loss hide, CLI round-trips
      exit 0; harness notes for the next walk: `virsh send-key` keys are
      one chord — type one key per call — and pointer input rides QMP
      `input-send-event`, per-axis, unbatched)*

**Checkpoint — the core stands alone:** zero toolkit deps pinned by
test; libcosmic binary + applet identical behavior.

- [x] sf-1 `dropterm-gpui` crate; the ruled pin in the root workspace;
      manifest pin test; LayerShell boot on rig + VM *(walked 2026-11-13:
      rig 327 ms debug first frame, frame 2 = 1280×200 full width, clean
      loop exit 0 (target/rig-evidence-sf1.log); VM 186 ms first frame,
      band pixel-evidenced over the desktop (target/vm-evidence-sf1.log +
      -boot.png), snapshot pre-gpui-scaffold-walk taken and VM rewound
      after; the lockfile took the ruled unicode-properties =0.1.3 plus
      one automatic icu_* 2.3→2.4 unification, git pins untouched)*
- [x] sf-2 The Kit + `carried/gpui-component` one-const copy; kill check
      + flush edges both compositors; development.md carried-patches
      section (ledger entry 3) *(walked 2026-11-13: rig and VM both
      echo every cadence height exactly — 320 requested, 320 drawn (no
      2×20 inflation), `on_open_change` silent through the full 80 ms
      cycle, clean exits; logs + VM screenshot held at
      target/rig-evidence-sf2-kill.log / target/vm-evidence-sf2-kill.{log,png},
      VM rewound after; the copy is drift-gated by test — a bump that
      forgets the const fails just check)*
- [x] sf-3 Startup dispatch: the guard's verdicts on the GPUI connection
      *(walked 2026-11-13: the verdict function moved to
      core::transport (7 tests re-homed, frontend probe consumes it —
      one decision, both frontends); lg-2 replay on the rig boots
      through the guard (target/rig-evidence-sf3-boot.log); lg-3
      replay on weston 13 headless over docker — 19 globals, no zwlr —
      refuses with core's message + exit 1 and no probe reason, the
      cannot-ask control carries the probe reason
      (target/weston-evidence-sf3{,-control}.log))*
- [x] sf-4 V1 + activation beside the render loop (Connection held,
      self-check); CLI parity; SetVisible flips live — merge
      *(walked 2026-11-13 on rig + VM: name owned with the
      name_has_owner self-check row, the V1 `Visible` property read
      true/false, `ActivateAction` hide/show/toggle flipping the render
      live and back — render-state rows in the logs, the two VM
      screenshots pixel-differing through the flip (band mean 67
      visible vs 84 hidden) — second launch exits 0 with the verb
      answered, `--is-visible` answers with matching codes, and the
      stale-owner row (a SIGSTOPped holder) refuses exit 1 after the
      walk found and fixed a real hole: zbus 5 carries no default
      method timeout, so a wedged-but-alive owner hung every summon
      forever — core's forward_to_running now bounds the wait at 5 s
      (`FORWARD_DEADLINE_SECS`), both frontends; evidence in
      target/rig-evidence-sf4-{roundtrip,busctl,stale}.log and
      target/vm-evidence-sf4-{roundtrip.log,visible,hidden}.png, VM
      rewound after; the "SetVisible"/"IsVisible" vocabulary is the
      spike's shorthand — on the contract it is the `Visible` property
      read and the `ActivateAction` verbs both CLIs and the applet
      send)*

**Checkpoint — the frontend exists:** pinned, patched, boots, owns the
name, answers the contract on both compositors.

- [x] sr-1 GPUI executor skeleton; reconcile tests; two-surface show
      (never reads displays) *(done 2026-11-13 on port/gpui-surface: 19
      executor tests; walked rig + VM — configured geometry exact in
      logs and pixels (1024x216 sway / 1024x240 COSMIC, band
      pixel-verified), release 36–46 ms inside the budget; three walk
      findings made structural: probe fires on configured bounds +
      reports via polled slot + QuitMode::Explicit)*
- [x] sr-2 Shape: 80 ms cadence resize in place; anchor/margin edits via
      recreate with the re-grab *(done 2026-11-13 on port/gpui-surface:
      drag through the machine's real funnel (walk-fed positions),
      config edits through a notify watcher + decide_reshape with a
      300 ms echo quiet window, slide clock ticked by the shell;
      walked rig + VM — cadence targets echoed on both, settle 65%/62%
      with the snap, the edit recreated at 1024x360/1024x400 with
      activation GAINED on the VM's seat the same millisecond as the
      swap)*
- [x] sr-3 Holds; the slide ported (release at slide-out start, destroy
      at completion); focus-loss hide *(done 2026-11-13 on
      port/gpui-surface: Hold recreates at the pin's create-only
      keyboard, the slide clock destroys at ~137-146 ms in release, the
      reversal re-holds mid-slide, activation loss runs the focus-loss
      gate into the hide; four walk findings made structural —
      apply-time handle retirement, transition-gated focus-loss flag,
      transition-order fix, phase publish on the completion destroy;
      walked rig + VM with the focus-loss steal evidenced on COSMIC's
      real seat)*
- [x] sr-4 Budget banked; surface rows both compositors; merge
      *(done 2026-11-13: release 34–49 ms over five VM shows inside
      the 130 ms budget, destroy 142–152 ms; core untouched (empty
      diff, 819 tests green); all six surface rows walked on both
      compositors with evidence, the reversal VM-window note recorded
      (debounce vs tick — rides parity's human row as si-5 parked
      it); the merge's own gate runs on main)*
- [x] gt-1 Grid view + damage redraw + resize→SIGWINCH (check-9 path)
      *(done 2026-11-13 on port/gpui-terminal, committed 81ad238:
      walked rig + VM — VM pixels vm-evidence-gt1-*, rig logs
      rig-evidence-gt1-*, 8×20→100×20 through Term::resize with the
      pty's stty confirming; two findings made structural — the font
      candidate resolution and the rig's harness-window silence)*
- [x] gt-2 Input + keybinds; selection; scrollback *(done 2026-11-14
      on port/gpui-terminal: every default chord fired live on the
      rig's logs plus the custom overlay both ways; the VM holds the
      pixels — SelectAll's full-grid overlay and its Escape clear, a
      uinput hold-drag selecting 80 bytes with the overlay band;
      three environment findings recorded in the harness docs — the
      rig surface holds keyboard though it never presents, cosmic-comp
      drops wtype's modifiers, and uinput wheel synthesis reaches no
      app on the VM, parking the physical-wheel row on parity's
      human-at-console class)*
- [x] gt-3 Mouse reporting; clipboard (focus+serial by construction;
      data-control for not-focused) *(done 2026-11-14 on
      port/gpui-terminal: the protocol encodings moved to core
      framework-free with the libcosmic frontend consuming them
      through an adapter, both frontends one implementation now; the
      VM's walk echoes the pane's own bytes through `cat -v` — the
      SGR press/motion/release triple and the legacy X10 forms — and
      the clipboard rows landed: copy-after-keystroke through GPUI's
      clipboard (323 bytes, pasted back whole), an external
      wl-copy's marker through the Paste chord, and an OSC 52 store
      serving over data-control while the pane lived)*
- [x] gt-4 Wire into the executor's windows; terminal rows both
      compositors; merge — after sr-4 or coordinated with it
      *(done 2026-11-14 on port/gpui-terminal: the clock band is
      gone, the pane is the drawer's content under the slide
      translation, one shared boot_pane constructs it for the shell
      and the walk harness alike, the pane's Hide chord drains
      through the shell's tick into the machine's funnel, and the
      drawer cycle walked both compositors — the rig inside a
      dbus-run-session (forwarded toggle, pane rows rendering,
      typing, the re-show; sway's wtype-induced activation loss ran
      the focus-loss funnel correctly), the VM with pixels (the
      band appearing, the typed line, the hide-by-chord destroy,
      the re-show) and the hide chord + copy rows logged)*
- [x] gm-1 Carry `theme_gen.rs` from the spike (before any scratch-repo
      cleanup); four tests green *(done 2026-11-14 on port/gpui-theme:
      byte-identical to the spike's cd06740 apart from the SPDX header —
      the verbatim-plus-compile ruling holds, diff-checked; the pedantic
      adaptation (doc-markdown + unused-mut allows, dead_code until
      Task 2 wires the apply path) sits on main.rs's mod declaration so
      the carried file stays auditable against the spike; serde_json =1
      the one new direct dep, already in the lock — the lockfile took
      only the dep edge)*
- [x] gm-2 Apply path: before first frame + live re-theme; C4-class
      evidence — merge *(merged 36648c1; done 2026-11-14 on
      port/gpui-theme, just check green on the merge result: the Kit
      initialized and the scheme applied in boot() before any window
      exists, the config echo resolves and compares a theme identity
      (scheme name + mode — geometry-only edits never re-theme, live
      on both compositors), and the tick's own apply point runs the
      sanctioned route plus the pane's retheme (the zoom action's
      set_config seam) before its notify; the kill check now themes
      through the same route, doubling as the standing C4 chrome
      harness; WALKED 2026-11-14 on rig + VM (evidence under
      target/{rig,vm}-evidence-gm2-*): the boot row lands before
      first frame with tokens reading the scheme (Dracula bg l=0.184
      = #282A36, primary rides its Blue), the live Dracula→Nord edit
      re-applies with Nord tokens (#2E3440) while shown and the
      drawer band's pixels follow (rig 40,42,54→46,52,64; VM the
      same), the height-only edit reshapes with the theme-applied
      count staying 2, and the themed kill check holds its flush
      edges (320→320) with overlays silent on both; 2 new unit pins:
      the identity detector and the resolve/boot_colors one-chain
      agreement)*

**Checkpoint — a working, themed terminal drawer on GPUI** (sr ∥ gt ∥ gm
after sf; gb ∥ gn after gt). *(Held 2026-11-14: all three parallel
slices merged — 05ac2ba · 761547d · 36648c1.)*

- [x] gb-1 Tab bar view model + rendering *(done 2026-11-14 on main,
      owner ruling — the pair builds here, worktrees parked: the strip
      is view-only over core's model, labels composing core's
      title_prefix ahead of resolve_title's output pinned over every
      pinned×attention combination, rows rendering from two tabs on
      (the 1-tab strip carries `+` and gear alone), the strip paint
      deriving from the pane background (×0.92, luminance-readable
      fg), the TabPosition auto/top/bottom rule and the
      terminal-facing border hairline ported — nine headless pins,
      walked on rig + VM: rig target/rig-evidence-gtabs1/ (three
      seeded Shell titles retitle live; Numbered parks at Terminal N
      with the strip pinned top and border off), VM pixels target/
      vm-evidence-gtabs1/ (three labeled rows with × glyphs and the
      active pill, `+`/⚙ at the right corner; the 1-tab strip bare
      but the corner pair))* · gb-2 reorder/rename/menu/close *(done
      2026-11-14 on main, 6d3df41 → 1dbbeb0: the model surface over
      core first — rename override through commit_rename, drag and
      menu moves through reorder_tabs/move_target, spawn_duplicate
      seeded from the source, close_sweep descending with pins
      skipped, note_attention on inactive tabs, close retiring the
      successor's marker, the menu rows and the confirm gate's truth
      table headless-pinned (twelve pins, tests to tab_bar/tests.rs) —
      then the pane signals (PaneSignal Hide|Exited, the polled
      AttentionFlags latch, foreground_child_running), the close flow
      (the gated front door, the in-surface modal holding the keyboard
      through its focus handle with the pane's claim turned
      cooperative and a render-side repair handing it back, the
      never-empty respawn, the shell-exit close ungated), and the view
      gestures (drag with a floating chip, the Kit context menu on
      the popover path through the Root-wrapped window, the Kit input
      hosting the inline rename with Enter/Escape/blur and click-away
      commits, the × glyph and middle-click through the gate); walked
      on rig + VM: the VM's uinput-driven rows under target/
      vm-evidence-gtabs2/ (attention • then 🔔 on inactive tabs only,
      rename commit "work" + cancel, the menu's Pin, the pinned ×
      refusal, drag to index, the modal's three dismissals + proceed,
      middle-click gated, toggle-off instant, exit closes into the
      respawn), sway's attention rows + strip pixels under target/
      rig-evidence-gtabs2/ (the emoji markers need a font the rig
      lacks — pixels are the VM's); walk finding made structural: an
      absolute overlay needs inset_0 (size_full alone paints nothing —
      the modal's first frames), fixed in 1dbbeb0; scope notes: no
      Switch Profile section (no GPUI profile spawn path yet), no
      session-persistence rows, rename/modal state dies with the
      window on hide)* · gb-3 rows + merge *(done 2026-11-14 — the
      rows' record is gb-2's walk on both compositors, `just check`
      green on the final tree; the module builds on main per the
      owner's ruling, no merge)*
- [x] gn-1 Band layout + clock + base segments *(done 2026-11-14 —
      the framework-free model moved to core first, unchanged, the
      libcosmic suite green as the proof; the GPUI band renders it:
      rig + VM walks green, the clock's boundary flip logged and
      pixel-diffed)* · gn-2 OSC-fed segments *(done 2026-11-14 —
      every OSC seat walked on both compositors: the report winning
      the path, the red ✗ verdict, progress, the tinted disc; the
      gated gathering + the copy/jump verbs landed with two more
      pins)* · gn-3 rows + merge *(rows walked 2026-11-14 — the copy
      verb end-to-end with an external wl-paste serving the payload,
      the ✓ flash and its return, the duration's live/frozen chain
      proven after the walk's structural find: the pane's starved
      engine-flag fix, its own commit; the scrolled rows ride the
      wheel class; merged 35ea4cf with `just check` green on
      the result)
- [x] gc-1 Context menu + pickers on the Kit popover path (done
      2026-11-14: the models in core — context_menu's rows/labels and
      font_list whole — the menu on the pane's right click through the
      Kit popover with every pick in the keybinds' own funnel, the
      pickers as Kit Select components with the libcosmic preview
      fidelity, all suites green; walked green on the VM agent-run
      (each row's pick line plus its downstream observable, the
      reporting gate decoded from the child's X10 bytes; rig n-a, the
      pointer class); profile folders deferred to a spawn path,
      pickers' runtime rows ride gc-2/gc-3)
- [x] gc-2 Settings drawer (write contract unchanged; control map
      ported; config↔docs pin green) (done 2026-11-14: the drawer's
      framework-free half in core with its 162 tests, the five pages
      write-through over it — Profiles' transactional form, the keybind
      editor over a core registry pinned chord-for-chord — and the
      drawer on a shell-rendered overlay with the resize handle; the
      walk's reentrancy find fixed in its own commit and re-walked
      green (vm-evidence-gc2b; owner-ruled standing 2026-11-15); `just
      check` green on the result 2026-11-15; the scheme picker's and
      cadence rows ride gc-3)
- [x] gc-3 Cadence row + chrome rows + merge (done 2026-11-15: walked
      owner-run on COSMIC + sway per the manual ruling — the cadence
      row (drawer + popover across the 80 ms reshape, never closing)
      and all eight chrome clusters green, no deviations filed; the
      owner's reported result is the record, no target/ artifacts;
      merged with `just check` green on the result)

**Checkpoint — full UI parity feature set.**

- [x] py-1 Suggest overlay + menu (done 2026-11-15, a2f4172: the
      view-model's routing/accept/geometry pins, the pane's key
      funnel with the focus-dismissed ghost and the reveal timer, the
      ghost + menu painting on the Kit's popover tokens, the
      completions load with live pane push, and config.shell owning
      the GPUI pty; autosuggest rows walked owner-run 2026-11-15 —
      reported done, no deviations filed)
- [x] py-2 Inline images (done 2026-11-15, 6f9c648: the sub-rect
      math and handle cache over the engine's store with the
      two-pass z-layer paint, the paste cache moved to core (one
      exclusive-create write path, the libcosmic crate a re-export
      front), and the paste-image road pasting the cache file's path
      with the ledger recorded; images rows walked owner-run
      2026-11-15 — reported done, no deviations filed)
- [x] py-3 Bell + session restore (done 2026-11-15, d5acb47: the
      notify gate/latch at the attention poll with the show
      transition's re-arm, the restore arm (chrome replayed, cwd
      honored, shell degradation, the walk fleet outranking the
      plan) and the shape-detected debounced save with a seeded
      baseline; bell + sessions rows walked owner-run 2026-11-15 —
      reported done, no deviations filed)
- [ ] py-4 The parity walk: the whole manual checklist, COSMIC + sway,
      row-named evidence; deviations filed *(parked, owner ruling
      2026-11-15 — the walks no longer gate building; un-park by the
      owner running the walk; the feature tasks below land first)*
- [x] py-5 The tab-verb and profile spawn surface: core spawn
      resolution + GPUI profile launches (env/cwd/per-tab appearance),
      default_profile, --cmd, the remote tab verbs (CLI + dbus + the
      shell funnel), the menu folders, the `+` dropdown, Switch
      Profile, session profile ids, the band's profile seat *(done
      2026-11-15: 986e765 · 113566b · 847b99c · 2028c7f, `just check`
      green across the four; the manual rows park with py-4)*
- [x] py-6 The GPUI images panel over core's paste cache and ledger,
      with the labeled confirm pair *(done 2026-11-15, b33cf05, `just
      check` green — the manual rows park with py-4)*

**Checkpoint — parity:** Phase 3's exit line holds on both compositors.

- [x] cu-1 The swap: GPUI bin becomes `dropterm`; libcosmic frontend
      deleted; applet canary — the code swap proceeds under the
      2026-11-15 walk-skip ruling *(done 2026-11-15, a2cd89f: package
      and bin renamed, the frontend deleted, the root a virtual
      workspace, the workflow's version read re-pointed; `just check`
      and `just build-all` green — the canary row died with the
      applet, cu-4)*
- [x] cu-2 Docs true (architecture rewrite incl. diagrams;
      known-issues fractional row) *(done 2026-11-15, 7d98912)*
- [x] cu-4 The collapse: one package `dropterm` (bin + the `core`
      lib half), one CLI, the applet and `dropterm-core` gone
      *(owner ruling 2026-11-15 — "one cli command and crate; drop
      it"; done the same day: the gpui directory renamed `dropterm/`,
      the core dissolved into the package's lib target with the
      toolkit pin re-homed as a source-walking test, the applet
      package + res + justfile + workflow legs deleted, docs and
      testing.md re-truthed, Cargo.lock −2.3k lines with libcosmic
      out of the graph; `just check` and `just build-all` green,
      clippy clean, 1225 tests green both identities)*
- [ ] cu-3 The deferred release, `/release` ritual, walk gate — owner
      fires it *(owner-gated: the release stays the owner's to cut;
      nothing here pushes or tags)*

**Checkpoint — Phase 3 exits:** parity proven, one binary, the release
cut; Phase 4 may open. *(The code is one binary; parity's proof is
py-4's parked walk and the release is cu-3's owner gate — the
checkpoint rides both.)*

## Hardening pass — toggle-ram (main; /planning 2026-11-16) — done 2026-11-17

- [x] tr-1 The attribution probe: per-mapping RSS + fds + threads,
      paced 1 s + spam 100 ms/50 ms profiles, ≥150 toggles each, build
      identity per run; debug-trace growth class; the owner's crash
      signature captured
- [x] tr-2 The fix *(gate: tr-1's attribution; root cause named with
      before/after numbers; upstream findings filed ask-first)* —
      **narrowed (owner 2026-11-17): the crashing build was
      pre-74e6d85, so the cause is already fixed and pinned;
      remaining is evidence only — the 2×-duration pass and the
      hardware walkthrough, owner-run; no new code fix**
- [x] tr-3 Bank it: testing.md row both-regime, known-issues residue,
      probe outputs in the chunk

**Checkpoint — the crash is fixed and evidenced:** flat tail both
regimes, crash gone at 2× duration, rows green, owner has the numbers.
*(Done 2026-11-17 — rows were left unticked here at the time; ticked
retroactively to match the committed chunk and SPEC status.)*

## Hardening pass — resource-audit (main; after toggle-ram) — done 2026-11-17

- [x] ra-1 Descriptors/processes/threads/disk sweep (paste-cache
      retention ask-first, session save, PTY reaping, watcher, debug
      log bound, fd/thread under spam)
- [x] ra-2 Memory/GPU sweep (scrollback cap verified + cost documented,
      atlas/GPU retention, images ledger, resize churn)
- [x] ra-3 Concurrency/panic census + input bombs (unwrap/expect
      classification, tick queues, zbus bound, config/paste/decode/
      OSC 52/copy/flood)
- [x] ra-4 The P1 fix batch *(gate: ra-1..3 findings; one commit per
      fix, bound as a number, pinned)*
- [x] ra-5 The report filed (docs/research/resource-audit.md, index
      row, docs rows, just check)

**Checkpoint — the sweep is dispositioned:** every vector fixed+pinned,
filed, or deferred by ruling; no P1 open; report filed and reviewed.
*(Closed 2026-11-17 by owner direction: both instructed confirmations
run agent-side under that direction and green — the saturated tab
0.04 % off the pinned math, the verb flood bounded with P2-2's
one-tick clause corrected; the report's Confirmations section carries
the numbers.)*

## RAM initiative — ram-leak (main; /planning 2026-11-17)

- [x] rl-1 The capped reproduction formalized (`#[ignore]` real-rc
      churn test, the curve recorded agent-run under the cap, the
      probe's panic-shape gone from the tree)
- [x] rl-2 Attribution (heaptrack under the cap / dhat fallback; the
      stack, the GB/s arithmetic, the fix-site verdict)
- [x] rl-3 The fix + pins *(gate: rl-2; root cause named with
      before/after numbers, synthetic pin in `just check`, capped
      rerun flat; fork path ask-first)* — done 2026-11-17: the
      carried path ruled by the owner, two upstream-bug guards in
      carried/alacritty-terminal + dropterm's own `visible_rows`
      viewport fix; pin (≤1024 grid rows) green in `just check`,
      capped real-rc rerun flat 50.1→51.2 MiB / 22 cycles (was
      49 MB → the cap inside one resize; 47.1 GB / 12 s uncapped)
- [x] rl-4 Bank it (incident record + index row, testing row,
      residue — including what the fix does not explain of daily
      growth) — done 2026-11-17: docs/research/ram-leak.md filed
      (journal lines verbatim, the attribution, the numbers, the
      upstream PR draft); known-issues carries the daily-growth
      open thread for ram-telemetry's curve

**Checkpoint — the bomb is fixed and evidenced:** capped repro flat
with a stated bound, pin green, owner has the numbers; ram-telemetry
opens on it.

## RAM initiative — ram-telemetry (main; after ram-leak)

- [x] rt-1 The sampler module (`core/mem.rs`: parse / rotate /
      fail-safe, unit-pinned, zero new deps) — done 2026-11-18: 19
      tests green in `just check`; the realistic rollup fixture
      caught a real parser bug (smaps_rollup writes no colons,
      status does) before it ever hit the every-30th read
- [x] rt-2 The 1 Hz tick hook + children feed + capped smoke
      (agent-run; the owner's daily-session instruction issued) —
      done 2026-11-18: `tick_mem_watch` on the shell's 100 ms pump
      (every 10th tick, pinned), children fed from the panes' engine
      pids, smoke `ticks=20600 writes=2060 A=2048 B=12 rollup=68`
      green in `just check`; the owner's daily-session row issued in
      the chunk, verdict pending
- [x] rt-3 Docs + banking (schema + agent-extraction one-liners +
      capture ladder, README index link, testing row, residue) —
      done 2026-11-18: docs/memory-diagnostics.md landed (schema
      table, copy-runnable one-liners, the tooling record's
      five-step ladder), linked from README's index; the testing row
      open for the owner's verdict (the module's close — flat
      closes the known-issues daily-growth row); the tooling
      record's research row moved Proposed → Adopted; `just check`
      green with docs-check

**Checkpoint — the watch is live:** sampler in every build, the log
verified on disk, docs landed; the owner's daily-session verdict
closes the module.
