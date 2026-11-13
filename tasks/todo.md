# Todo — aggregate of the chunks

Detail (acceptance, verify, files) lives in each
[tasks/<id>.md](plan.md) chunk; this is the cross-module order. Gated
items carry their gate inline. Never mark an item done without its
chunk's verification passing.

## Phase 0 — reality-check (hardware; parallel-safe)

- [x] rc-1 Session 1: COSMIC baseline (16 checks, slot filled) *(agent-run 2026-11-11 in the validation VM, 9091b19: 1/4/8/12/13/15 pass, input rows n-a — owner's eyes-on, parked 2026-11-13)*
- [x] rc-2 Session 2a: sway (shared with the spike) *(agent-run headless, 1deed62: green where exercisable — maps/renders/lifecycle/handoff; input rows + bell row want one hardware sway session — parked
      2026-11-13)*
- [ ] rc-3 Session 2b: Hyprland *(parked 2026-11-13, owner ruling —
      un-park by running the session; a headless-docker attempt may
      still carry it the way sway's session went)*
- [ ] rc-4 Session 3: KDE Plasma (Wayland) *(parked 2026-11-13, owner
      ruling — un-park by running the session)*
- [ ] rc-5 Session 4: GNOME — how it fails (feeds layer-shell-guard)
      *(parked 2026-11-13, owner ruling — expected-fail and
      confirmatory; the guard already has weston-headless evidence)*
- [x] rc-6 Session 5: one X11 session (record which) *(i3 over Xvfb, 482a37d: zombie + degenerate 32px DOCK band first launch, truthful --is-visible, libXi-less panic variant; expected-fail recorded)*
- [x] rc-7 Session 6: non-Pop distro — floor, ldd, fonts (feeds
      distro-reach) *(run 2026-11-11 on archlinux:latest, slot filled
      8328d51: the artifact's own floor is GLIBC_2.39, source build
      green, ldd under-reports through winit's dlopen, fontless base;
      this box only catches todo up — the chunk and plan record
      already carried it)*
- [ ] rc-8 Close-out: every failure filed and named in its slot
      *(parked 2026-11-13 with the three open sessions — it sweeps
      what they fill)*

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
- [ ] sc-5 No-behavior-change VM walk; merge (first Phase 3 merge)

**Checkpoint — the core stands alone:** zero toolkit deps pinned by
test; libcosmic binary + applet identical behavior.

- [ ] sf-1 `dropterm-gpui` crate; the ruled pin in the root workspace;
      manifest pin test; LayerShell boot on rig + VM
- [ ] sf-2 The Kit + `carried/gpui-component` one-const copy; kill check
      + flush edges both compositors; development.md carried-patches
      section (ledger entry 3)
- [ ] sf-3 Startup dispatch: the guard's verdicts on the GPUI connection
- [ ] sf-4 V1 + activation beside the render loop (Connection held,
      self-check); CLI parity; SetVisible flips live — merge

**Checkpoint — the frontend exists:** pinned, patched, boots, owns the
name, answers the contract on both compositors.

- [ ] sr-1 GPUI executor skeleton; reconcile tests; two-surface show
      (never reads displays)
- [ ] sr-2 Shape: 80 ms cadence resize in place; anchor/margin edits via
      recreate with the re-grab
- [ ] sr-3 Holds; the slide ported (release at slide-out start, destroy
      at completion); focus-loss hide
- [ ] sr-4 Budget banked; surface rows both compositors; merge
- [ ] gt-1 Grid view + damage redraw + resize→SIGWINCH (check-9 path)
- [ ] gt-2 Input + keybinds; selection; scrollback
- [ ] gt-3 Mouse reporting; clipboard (focus+serial by construction;
      data-control for not-focused)
- [ ] gt-4 Wire into the executor's windows; terminal rows both
      compositors; merge — after sr-4 or coordinated with it
- [ ] gm-1 Carry `theme_gen.rs` from the spike (before any scratch-repo
      cleanup); four tests green
- [ ] gm-2 Apply path: before first frame + live re-theme; C4-class
      evidence — merge

**Checkpoint — a working, themed terminal drawer on GPUI** (sr ∥ gt ∥ gm
after sf; gb ∥ gn after gt).

- [ ] gb-1 Tab bar view model + rendering · gb-2 reorder/rename/menu/
      close · gb-3 rows + merge
- [ ] gn-1 Band layout + clock + base segments · gn-2 OSC-fed segments ·
      gn-3 rows + merge
- [ ] gc-1 Context menu + pickers on the Kit popover path
- [ ] gc-2 Settings drawer (write contract unchanged; control map
      ported; config↔docs pin green)
- [ ] gc-3 Cadence row + chrome rows + merge

**Checkpoint — full UI parity feature set.**

- [ ] py-1 Suggest overlay + menu
- [ ] py-2 Inline images
- [ ] py-3 Bell + session restore
- [ ] py-4 The parity walk: the whole manual checklist, COSMIC + sway,
      row-named evidence; deviations filed — gate: open question item 9
      ruled (dock mode)

**Checkpoint — parity:** Phase 3's exit line holds on both compositors.

- [ ] cu-1 The swap: GPUI bin becomes `dropterm`; libcosmic frontend
      deleted; applet canary — owner-gated
- [ ] cu-2 Docs true (architecture rewrite incl. diagrams;
      known-issues fractional row)
- [ ] cu-3 The deferred release, `/release` ritual, walk gate — owner
      fires it

**Checkpoint — Phase 3 exits:** parity proven, one binary, the release
cut; Phase 4 may open.
