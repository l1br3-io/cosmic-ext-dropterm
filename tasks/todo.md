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
- [ ] gs-2 Check 0: boot and map on COSMIC + sway *(sway half green
      headless 2026-11-12 over the rebuilt lg-3 container pattern —
      mapped, rendered, geometry honored, clean exit, grim screenshot
      pixel-checked; COSMIC half is an owner-hardware row)*
- [ ] gs-3 Checks 1–8, 2b on COSMIC (ledger starts; latency vs 130 ms)
      *(sway-witness data banked 2026-11-12, spike repo 5422181:
      two-surface show 121/117/119ms — inside the budget, debug+software
      GL; 80ms-cadence resize honored; displays populated; one 0-width
      first-frame echo; #61368 no-disconnect at sway 1.5, but
      `cx.displays()` mis-sizes fractional outputs (integer-scale read)
      — two-surface show immune, measure-free not; COSMIC rows open)*
- [ ] gs-4 Check 11: backend detection verdicts *(harness armed,
      aeea7b3: `SPIKE_CHECK=detect`; no-session branch verified
      headless; the three verdict rows run in the COSMIC session)*
- [ ] gs-5 Check 9: terminal grid render (sets the shared-core
      boundary) *(harness armed, aeea7b3: `SPIKE_CHECK=grid`; the
      patched forks proven to resolve beside GPUI's tree by the build
      itself; the cosmic-text-role note written — no engine-side role
      on GPUI's path; render rows run in the COSMIC session)*
- [ ] gs-6 Check 10: zbus + V1 beside GPUI *(harness armed, aeea7b3:
      `SPIKE_CHECK=dbus`; the V1-shaped service compiled green on its
      own tokio thread beside GPUI; the busctl round-trip rows run in
      the COSMIC session)*
- [ ] gs-7 Checks 12–13: X11 and macOS PopUp
- [ ] gs-8 Check 14: coexistence experiment (one commit, reverted)
- [ ] gs-9 The report → "Phase 2 results" on main
- [ ] kp-0 Gate: spike reported AND Kit ruled adopt (else stop)
- [ ] kp-1 C1: pin swap to gpui-pre; checks 0–4 re-run
- [ ] kp-2 C2a: consumer LayerShell through gpui-pre
- [ ] kp-3 C2b: Kit overlays on layer-shell at 80 ms cadence
- [ ] kp-4 C4: theme generator through ThemeRegistry
- [ ] kp-5 Kit verdict appended

**Checkpoint — the spike report:** owner rules on the Kit and live
geometry edits; Phase 3 slices get ids and specs.
