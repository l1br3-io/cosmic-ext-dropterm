# Can GPUI carry DropTerm's lifecycle? — the Phase 2 spike report

Date: 2026-11-13 · Verdict: **GO** (with a two-entry carried-patch ledger
and two hardware rows parked) · Module: `gpui-spike` · Scratch repo:
`../dropterm-spike`, branch `spike/gpui-layer-shell` (88a64ac…354af89),
never merged as code.

## TL;DR

GPUI can carry the lifecycle: the plan's GO criteria are met on the
evidence — checks 0, 1, 2, 5, and 6 pass on both target compositors,
check 4 holds, and check 3 has an evidenced workable path (the recreate
redesign re-grabs the keyboard, +141 ms after the swap). The release
build shows the drawer in **34 ms** against the 130 ms slide budget.
The Kit ruling (open ruling 2) remains the owner's and gates `kit-pass`;
nothing here forces it either way. The carried ledger: fractional-scale
**visible output** on COSMIC (geometry lands, pixels do not) and the
integer-rounded display metadata both compositors report; nothing else
survived as a patch to carry (#64994 never fired, #61368 never
disconnected).

## Pin identity

zed git `d2a4fb6` (the `gpui-pre` 0.3.12 line — newest on crates.io as
of 2026-11-12), `default-features = false, features = ["wayland",
"font-kit"]`; `gpui_platform` with `wayland`. The spike crate carries
its own workspace and lockfile on its branch; check 14 proved the root
workspace can absorb it later with one version pin reconciled.

## Findings (per check)

All evidence lives in the scratch repo: the walk and rerun logs and
screenshots under `cosmic-session-logs/`, the scripted sway witness at
`rig/sway-headless.sh`, the harness variants under
`spikes/gpui-layer-shell/src/`. Numbers marked debug are debug builds
on software GL (sway rig: Debian trixie, sway 1.10.1) or the validation
VM's llvmpipe-fallback stack (Pop!_OS 24.04 COSMIC; virsh/QEMU,
snapshot discipline kept); the release numbers are the ones to weigh.

1. **Check 0 — boot & map: PASS both.** Maps, renders, clean exit on
   COSMIC (VM walk, `walk-2026-11-13.log`) and sway (rig). No hint of
   the upstream Wayland-boot panic the check guarded against.
2. **Check 1 — shape at creation: PASS.** Two-surface show
   OPEN→FIRST_FRAME: sway 121/117/119 ms (debug); COSMIC 142 ms debug
   and **34/35/34 ms release** — the release build sits at ~26% of the
   130 ms budget even in the VM. The measure-free variant (193 ms on
   COSMIC) confirms the two-surface path on both compositors. #61368:
   no disconnect at sway 1.5 or COSMIC 1.25 — not carried.
3. **Check 2 — in-place reshape: PASS.** Every height of the 80 ms drag
   cadence landed on both compositors; explicit heights survive the
   compositor's configure — #64994 never fired on either. Not carried.
4. **Check 2b — live geometry edit via recreate: DONE (open ruling 3's
   numbers).** REMOVE→FIRST_FRAME 142/133/146 ms (sway) and 138 ms
   (COSMIC): a geometry edit that recreates costs about one extra show.
   The edited geometry lands exactly, and **the recreated surface
   re-grabs the keyboard** — activation GAINED +141 ms after the swap.
5. **Check 3 — keyboard at runtime: PATH HOLDS via the redesign.** No
   public runtime re-grab exists at this pin (carrying #61420 remains
   the fallback); the direct non-recreate evaluation is the one
   outstanding row (parked below), but 2b evidences the accepted
   redesign: recreate-on-geometry-edit preserves focus-return, which is
   what the GO clause requires.
6. **Check 4 — layer/zone at creation: HOLDS.** Every shaped open logs
   `Top`/zone `-1` honored; mapped bounds match on both compositors.
7. **Check 5 — focus events: PASS.** COSMIC grants the exclusive
   surface activation at +193 ms; sway-headless does too the moment a
   keyboard joins the seat (the earlier "no activation headless" was
   really "no keyboard on the seat" — wtype's virtual keyboard proved
   it). Activation transitions can drive `hide_on_focus_loss`.
8. **Check 6 — output targeting: PASS at integer scale.**
   `cx.displays()` populated at startup, correct size, surface on the
   active output. Carried debt: on fractional outputs the enumeration
   reads the rounded-up integer `wl_output` scale (960×540 at sway 1.5;
   640×400 at COSMIC 1.25) — the measure-free show would mis-size
   there, which is a second reason the two-surface show is the chosen
   path (it never reads displays).
9. **Check 7 — resize echo: one artifact, no double-reporting.** A
   single 0-width first frame before the compositor's configure on both
   compositors, then clean single reports — the `geometry` pin-debounce
   machinery does not port; it is not needed.
10. **Check 8 — clipboard: RESOLVED, stronger than posed.** The write
    gate is a **focused window plus an input serial from a real key or
    pointer press** (`gpui_linux` wayland/client.rs:1462 at this pin);
    with a focused typeable surface and a real keystroke the offer
    lands — `wl-paste` read the marker on sway and COSMIC
    (`rig-clipboard-key-2026-11-13.log`, `vm-rerun-2026-11-13.log`).
    The anchor-surface workaround as built does **not** carry over; the
    terminal surface being focused at copy time is exactly when the
    gate opens, so the design satisfies it by construction.
    Copy-from-unfocused stays unsupported (peer-normal).
11. **Check 9 — grid render: PASS.** DropTerm's patched
    `alacritty_terminal` forks resolve beside GPUI's tree (the build is
    the proof) and a live PTY rendered on COSMIC — 8×20 → 128×20
    through `Term::resize` + SIGWINCH, seeded ANSI pattern visible.
    Engine-side `cosmic-text` buffers have no role on GPUI's path
    (GPUI's own stack is cosmic-text-based at this pin).
12. **Check 10 — D-Bus interop: PASS both.** The walk's "never owned
    name" was a harness bug — the zbus `Connection` was bound inside a
    match arm and dropped at the arm's end (fix: spike c2c992b, with a
    NameHasOwner self-check row); the full V1-shaped round-trip runs
    beside the render loop on both compositors. `dbus_state` stays
    framework-agnostic; the loops coexist.
13. **Check 11 — backend detection: PASS where runnable.** The registry
    probe returns layer-shell verdicts on COSMIC and sway; the GNOME
    branch rides the parked GNOME session (its no-layer-shell shape is
    already proven headless).
14. **Checks 12–13 — X11 and macOS: PARKED (hardware).** Not run; the
    spike is wayland-only by spec, so an X11 leg additionally needs the
    `x11` feature enabled. They gate Phase 4/5 mechanisms, not the
    Wayland-primary GO. Un-park by running the sessions.
15. **Check 14 — workspace coexistence: ABSORBS.** Dropping the empty
    `[workspace]` table let the root workspace absorb the crate: one
    lockfile, exactly one reconciliation (`unicode-properties`
    `=0.1.3`, `gpui_web`'s exact pin vs libcosmic's resvg chain at
    0.1.4), both halves green, no `links` conflicts. The own-workspace
    fallback stays documented but unneeded.
16. **Fractional-scale COSMIC (the one negative): geometry lands,
    pixels do not.** `cosmic-randr` applied 1.25; the layer surface
    maps at 1024×200 (fractional honored in geometry) and the app runs
    to a clean exit — but the console shows no trace of the band or its
    clock where 100% shows it (two runs, screenshots
    `vm-cosmic-fractional{,2}.png`), and `cx.displays()` reads the
    integer-rounded 640×400. Same mis-read class as sway 1.5; the
    visible-output half is COSMIC-specific at this pin.

## What DropTerm has today

Everything above is spike evidence, not shipped code: the drawer today
is libcosmic + `surface-intents`' executors, and every finding above
describes the GPUI executors Phase 3 would write. The two design facts
that change Phase 3 regardless of the Kit: the recreate path is the
geometry-edit mechanism (ruling 3's numbers now exist), and the
clipboard anchor dies in favor of terminal-focus-by-construction.

## Recommendation

**GO.** Checks 0/1/2/5/6 pass, 4 holds, 3 has an evidenced workable
path; the carried ledger is two entries. Rejected alternatives, for the
record: the measure-free show (slower, and mis-sizes on fractional
outputs — check 6's debt); carrying #61420 now (no runtime re-grab is
needed while recreate covers it); the anchor clipboard workaround (the
gate it worked around does not exist — focus+serial is the real gate);
NO-GO/fork and the `iced_layershell` fallback (no criterion fired).

**Carried-patch ledger:** (1) fractional-scale visible output on COSMIC
— geometry honored, nothing visible, cause not yet isolated (viewport/
scale mismatch is the suspect; the display-metadata integer-rounding on
both compositors rides along); (2) #61420, dormant — carried only if a
runtime (non-recreate) keyboard re-grab is ever required.

## Kit pass — the verdict on the snapshot (kit-pass, 2026-11-13)

The owner ruled the Kit **adopt** (open ruling 1, 2026-11-13, at open
ruling 2's recommended timing); kit-pass ran the same day on the
scratch branch (`spike/gpui-layer-shell` d722652…cd06740, never
merged). **Kit GO for the pin** — every condition passed on
`gpui-pre` =0.3.12 with `gpui-component` =0.9.1, component-only:

- **C1 PASS, twice over.** The whole 20-crate set pins at =0.3.12
  exactly as the Kit consumes it; the first independent source diff
  (file-hash mapping + two-direction tree diff against the zed
  `d2a4fb6` checkout still on disk) finds 11 of the 16 Linux-relevant
  crates byte-identical — including `gpui-pre-linux`, the entire
  wayland backend — with every delta banner-marked by the pipeline and
  exactly one real semantic delta: `gpui-pre-wgpu`'s wgpu 30.0.2
  adaptation. The behavior re-run shows it invisible: release show
  32/36/34 ms (base 34/35/34), recreate 139 ms with the keyboard
  re-grab +142 ms, every cadence height, and the fractional row
  identical (geometry honored, nothing visible, displays 640×400).
  One honest observation: a consistent ~+13 ms same-class debug drift
  on both compositors, invisible in release.
- **C2a PASS.** A consumer crate whose graph is exactly
  `gpui-pre` + `gpui-pre-platform` maps the drawer-shaped LayerShell
  window on both compositors (`kit-c2a-2026-11-13.log`, screenshot
  pixel-verified against the no-band fractional shot).
- **C2b PASS — the kill check, green on both compositors.** Root (the
  manual embedder-owned wrap) plus a programmatically-opened Popover
  menu and a Tooltip view rendered on the layer surface through the
  full 80 ms cadence without closing (`on_open_change` silent; the
  screenshot shows the popover's items on the surface).
- **C3 HOLDS.** Zero `gpui-shell` and zero `gpui-kit` edges in the
  manifest; one GPUI in the graph; windows stay embedder-owned.
- **C4 PASS.** The scheme → theme-JSON generator (committed on the
  spike branch as a Phase 3 input, four RED-first unit tests) maps
  DropTerm's resolved scheme onto 37 ThemeConfig keys, inventing no
  colors, and applies through `Theme::update(apply_config)`; the
  tokens read back the scheme and the chrome renders through them —
  the screenshot shows a dark popover where the un-themed run showed
  the Kit's default light one.

**The one carried debt (the GO's asterisk):** the Kit's Root
auto-decorates every Client-decorated window with its Linux
`window_border` — SHADOW_SIZE 20 px, `pub(crate)` const, no per-window
opt-out — and the wayland backend forces `Decorations::Client` on
every surface (initialized Client at gpui-pre-linux wayland/window.rs
:626; `request_decorations`' no-protocol arm forces Client back), so a
would-be 1280×320 drawer negotiates **1320×360** on both compositors.
DropTerm's edge-to-edge drawer needs a one-const/config carried patch
into the Kit (multiplying the plan's pipeline-fork maintenance cost)
or accepted non-flush edges — Phase 3's first Kit decision. The
plan's snapshot-fidelity open question is answered by C1's source
diff; the code-constructed-Theme question stands (C4 used the
sanctioned JSON path).

Phase 3's pin decision is armed: the snapshot is the GPUI source
(gpui-pre 0.3.12, exact-pinned with the Kit at its release boundary
per C5), per the plan's "the snapshot replaces the git pin".

## Sources

Scratch repo `../dropterm-spike` branch `spike/gpui-layer-shell`
(88a64ac…354af89; harness, rig, and all logs cited above), run
2026-11-12…13 · zed pin `d2a4fb6` (`gpui-pre` 0.3.12), `wayland`
feature only · validation VM `PopOs24.04` (Pop!_OS 24.04 COSMIC
session; COSMIC applets observed at 1.11.0; virsh/QEMU with snapshot
discipline) · sway witness: Debian trixie container, sway 1.10.1,
headless backend · keystroke injection: `wtype` (zwp virtual keyboard)
on both compositors · clipboard gate source-read at
`gpui_linux/src/linux/wayland/client.rs:1462` of the pin.
