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
- [x] Preconditions hold, or the already-ran case is recorded

*(Held 2026-11-13: `gpui-spike` reported **GO** with checks 0–4 all
evidenced on the zed-git pin ([the report](../docs/research/gpui-spike.md)),
and the Kit ruled **adopt** — owner ruling 1, decided 2026-11-13 at the
`/build auto` gate, which is exactly open ruling 2's recommended timing:
after the base spike, so the ruling read the carried-patch ledger. The
already-ran case does not apply — the base spike pinned zed git and never
pulled Kit crates, so C2a/C2b/C4 genuinely remain.)*

**Dependencies:** `gpui-spike` reported; Kit ruled adopt · **Scope:** XS

## Task 1: C1 — the pin swap and base checks re-run

**Description:** Swap the spike branch's pin to the `gpui-pre`
snapshot (exact-pinned, the whole `gpui-pre-*` set; commit the updated
lockfile), then re-run base checks 0–4 on it. The snapshot self-describes
as cut from the zed commit the base spike pinned, but the published
content was never independently diffed — this re-run is the fidelity
check.

**Acceptance criteria:**
- [x] Checks 0–4 re-run on the snapshot pin; any behavior differing
      from the zed-pin run is recorded as a snapshot-fidelity finding

**Verification:**
- [x] Manual on COSMIC (fractional-scale output for check 1) and sway

*(Done 2026-11-13, spike d722652. The swap: `gpui = { package =
"gpui-pre", version = "=0.3.12" }` + `gpui_platform = { package =
"gpui-pre-platform", version = "=0.3.12" }` — the whole 20-crate set
resolves together exactly as gpui-kit 0.9.1 (still newest) consumes it,
dep keys unchanged so no source edits. Fidelity banked both ways: the
first independent source diff (file-hash mapping + two-direction tree
diff vs the zed `d2a4fb6` checkout still on disk) finds 11/16
Linux-relevant crates byte-identical — including `gpui-pre-linux`, the
entire wayland backend — with every delta banner-marked by the pipeline
and exactly one real semantic delta: `gpui-pre-wgpu`'s 3-file wgpu
30.0.1+ adaptation (lock 30.0.0→30.0.2), the renderer being the only
non-identical subsystem; the behavior re-run (sway via the rig, COSMIC
via the validation VM with snapshot discipline, fractional row included)
then shows that delta invisible — release show 32/36/34 ms vs base
34/35/34, recreate 139 ms (base 138) with the re-grab GAINED +142 ms
(base +141), every cadence height, kb Exclusive/Top/zone −1 on every
open, and the fractional row reproducing the carried ledger exactly
(maps 1024×200, nothing visible, displays 640×400). One fidelity
observation, not a regression: a consistent ~+13 ms same-class **debug**
drift on both compositors (sway 134/135/133 vs 121/117/119; COSMIC 155
vs 142), invisible in release. Evidence: `kit-rig-sway-2026-11-13.log`,
`vm-kit-pass-2026-11-13.log` + 3 screenshots in the scratch repo's
`cosmic-session-logs/`.)*

**Dependencies:** Task 0 · **Scope:** M

## Task 2: C2a — consumer feature wiring

**Description:** The consumer build maps a `LayerShell` window through
gpui-pre's `wayland` features. The manifest-level wiring was confirmed
2026-11-10 (`gpui-pre` 0.3.12 declares `wayland`; `gpui-pre-platform`
forwards to its Linux crate); the consumer build is the remaining
proof.

**Acceptance criteria:**
- [x] The scratch crate, depending only on gpui-pre crates, opens and
      maps a `LayerShell` window

**Verification:**
- [x] Manual on COSMIC

*(Done 2026-11-13, spike fef2790. `spikes/kit-consumer/` is the
consumer: its graph is exactly `gpui-pre` + `gpui-pre-platform`
(`cargo tree --depth 1` shows two edges; the wayland path reads
`gpui-pre → gpui-pre-linux → gpui-pre-platform`), opening DropTerm's
drawer shape through the same `gpui_platform` boot the harness uses.
Green both compositors: sway-headless maps (0-width echo → 1280×200,
287 ms debug, clean exit); COSMIC in the VM maps identically at 203 ms
with the band's pixels verified by region-differencing the screenshot
against the no-band fractional shot (−94 mean, −175 darkest row, −30
drift control). The 2026-11-10 manifest confirmation + this build
close C2a. Evidence: `kit-c2a-2026-11-13.log`, `vm-kit-c2a.png`.)*

**Dependencies:** Task 1 · **Scope:** S

## Task 3: C2b — Kit overlays on a layer-shell surface

**Description:** Root + Kit overlays (menu, tooltip) render and behave
on a `LayerShell` surface during the 80 ms reshape cadence. This is the
Kit's kill check (no field evidence existed either way; kill-risk 5) —
a failure here is a Kit NO-GO *for this pin* and leaves GPUI's verdict
as it stands.

**Acceptance criteria:**
- [x] Overlays render and behave through sustained reshapes; evidence
      recorded

**Verification:**
- [x] Manual on COSMIC

*(Done 2026-11-13, spike 40fe302. The kill check is GREEN on both
compositors: the drawer's root view is the Kit's Root (the manual wrap
C3 names), a Popover-hosted PopupMenu opened programmatically plus a
built Tooltip stayed on the surface through the full 80 ms cadence
(`on_open_change` silent, every height landed, clean exits), and the
COSMIC screenshot shows the popover's three items and the tooltip pill
rendering on the layer surface. C3 holds in the manifest: zero
gpui-shell/gpui-kit edges, one GPUI in the graph. **One adoption-debt
finding for the verdict**: the Kit's Root auto-decorates every
Client-decorated window with its Linux `window_border` (SHADOW_SIZE
20 px, `pub(crate)` const, no per-window opt-out), and
`set_client_inset(20)` inflates the negotiated layer surface by 2×20
on both axes — a would-be 1280×320 drawer configures at 1320×360 on
both compositors — unreachable at this pin because the wayland backend
initializes every surface `Decorations::Client` (gpui-pre-linux
wayland/window.rs:626) and `request_decorations`' no-protocol arm
forces Client back. DropTerm's edge-to-edge drawer needs a Kit carried
patch (one const/config — multiplying the plan's pipeline-fork cost)
or non-flush edges accepted. Evidence: `kit-c2b-2026-11-13.log`,
`vm-kit-c2b-{mid,late}.png`.)*

**Dependencies:** Task 2 · **Scope:** M

## Task 4: C4 — the theme generator

**Description:** The scheme → theme-JSON generator (a spike artifact)
applies through `ThemeRegistry`/`apply_config` on the snapshot pin —
DropTerm's scheme colors feeding the Kit's theme tokens. (Themes are
`#[non_exhaustive]` since v0.9.0; no struct literals.)

**Acceptance criteria:**
- [x] A DropTerm scheme renders the terminal chrome through the
      generated theme; the generator's code is committed on the spike
      branch as a Phase 3 input

**Verification:**
- [x] Manual on COSMIC

*(Done 2026-11-13, spike cd06740. `src/theme_gen.rs` is the generator:
input is DropTerm's resolved scheme type (Alacritty `Colors`, what
theme_glue produces), output a ThemeConfig JSON value (`#[non_exhaustive]`
since v0.9.0 — JSON, never struct literals), 37 token keys with unset
slots left absent; four unit tests written RED-first pin it (including
output-parses-as-ThemeConfig). Applied on both compositors via
`serde_json::from_value::<ThemeConfig>` → `Theme::update(apply_config)`
before any window opens; tokens read back the scheme (background
#1A1B26, primary #7AA2F7, foreground #C0C5CE) and the LayerShell chrome
rendered through them — the COSMIC screenshot shows the dark band, the
blue link token, and a DARK popover where un-themed C2b showed the
default light one. Evidence: `kit-c4-2026-11-13.log`,
`vm-kit-c4-theme.png`, `cargo test` green.)*

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
