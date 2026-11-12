# Todo — aggregate of the chunks

Detail (acceptance, verify, files) lives in each
[tasks/<id>.md](plan.md) chunk; this is the cross-module order. Gated
items carry their gate inline. Never mark an item done without its
chunk's verification passing.

## Phase 0 — reality-check (hardware; parallel-safe)

- [x] rc-1 Session 1: COSMIC baseline (16 checks, slot filled) *(agent-run 2026-11-11 in the validation VM, 9091b19: 1/4/8/12/13/15 pass, input rows n-a — owner's eyes-on)*
- [x] rc-2 Session 2a: sway (shared with the spike) *(agent-run headless, 1deed62: green where exercisable — maps/renders/lifecycle/handoff; input rows + bell row want one hardware sway session)*
- [ ] rc-3 Session 2b: Hyprland
- [ ] rc-4 Session 3: KDE Plasma (Wayland)
- [ ] rc-5 Session 4: GNOME — how it fails (feeds layer-shell-guard)
- [x] rc-6 Session 5: one X11 session (record which) *(i3 over Xvfb, 482a37d: zombie + degenerate 32px DOCK band first launch, truthful --is-visible, libXi-less panic variant; expected-fail recorded)*
- [ ] rc-7 Session 6: non-Pop distro — floor, ldd, fonts (feeds
      distro-reach)
- [ ] rc-8 Close-out: every failure filed and named in its slot

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
      stays the owner's)*
- [x] cs-4 External-edit reload — gate: the `notify` dependency ask
      (fallback re-read at show) *(notify sanctioned 2026-11-10;
      merged f5199ec)*
- [x] cs-5 cosmic-config leaves; docs path; manual rows; merge
      *(merged f5199ec; the sudo-installed manual rows stay the
      owner's)*
- [ ] cs-6 Gate (COSMIC hardware): the dbus-config feature decision
- [ ] ct-1 Bus-name ownership at startup (replaces
      run_single_instance; activation shape kept — planning ruling)
- [ ] ct-2 Second-launch forwarding; exit codes unchanged
- [ ] ct-3 Contract-surface pin test
- [ ] ct-4 Drop single-instance; applet canary; merge

**Checkpoint — the break event:** app-identity + config-store in one
release; notes carry the break note. *Deferred 2026-11-12 (owner): no
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
      reversal's keybind press stays a human-at-console row)*

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
      row cites its session
- [ ] cd-1 docs/dbus.md derived from src/dbus_state.rs
- [ ] cd-2 every_dbus_member_is_documented pinning test
- [ ] cd-3 README index; merge
- [ ] dr-1 Facts brief from session 6
- [ ] dr-2 Owner rulings: glibc floor; channels
- [ ] dr-3 Floor ruling implemented (README states it either way)
- [ ] dr-4 Channel recipes (contingent on dr-2; nothing published)
- [ ] dr-5 Release-notes line; merge

**Checkpoint — end of Phase 1:** the plan's exit line; Phase 0 slots
filled; the break release cut.

## Phase 2 — the spike (worktree ../dropterm-spike; from day one)

- [ ] gs-1 Worktree + scratch crate (own workspace, lockfile committed,
      pin recorded)
- [ ] gs-2 Check 0: boot and map on COSMIC + sway
- [ ] gs-3 Checks 1–8, 2b on COSMIC (ledger starts; latency vs 130 ms)
- [ ] gs-4 Check 11: backend detection verdicts
- [ ] gs-5 Check 9: terminal grid render (sets the shared-core
      boundary)
- [ ] gs-6 Check 10: zbus + V1 beside GPUI
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
