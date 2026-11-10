# Research: can DropTerm replace its libcosmic UI with GPUI to decouple from COSMIC?

Date: 2026-11-09
Question: *Can DropTerm's UI layer be moved off libcosmic onto GPUI — removing the
COSMIC-desktop coupling — and what must be proven before any rewrite starts?*

## TL;DR

**Yes, GPUI is a viable replacement, but it is a full UI-layer rewrite, not a
dependency swap — and the decision is gated on a hardware spike, not on this
record.** Three layers are bundled under "the COSMIC coupling" and only the
middle one is expensive to remove: the terminal engine (`alacritty_terminal` +
the `vte`/`alacritty_terminal` OSC-133 forks), the config *model* (serde/ron
structs), and the zbus D-Bus contract are already framework-agnostic and survive
untouched. The rewrite is the ~30-file libcosmic UI layer: the `cosmic::app`
lifecycle, the `cosmic::widget` set, the five custom iced widgets, `cosmic-config`,
and `cosmic::theme`.

Two former blockers have closed since this question was first posed. GPUI
**re-landed native wlr-layer-shell** on 2025-10-29 (PR #35610, after PR #32651 was
reverted for crashing Wayland on launch), and its `LayerShellOptions` fields map
almost 1:1 onto `src/surface.rs`. And alacritty_terminal-on-GPUI is **proven and
packaged**: Zed's own terminal is `alacritty_terminal` rendered through GPUI
(`crates/terminal` depends on both), and the community `gpui-terminal` crate
exposes that integration for embedding.

The remaining risk is concentrated in one fact: **DropTerm's drop-down lifecycle
depends on runtime surface mutation, and GPUI's mutation surface is still
partial** — at the rev this record verified (`ec237216460f…`, 2026-11-09),
resize, exclusive zone, and exclusive edge have public runtime APIs
(`Window::resize`, `Window::set_exclusive_zone`, `Window::set_exclusive_edge`,
PR #60163), while anchor, margin, layer, and keyboard-interactivity remain
create-only (a `Window::set_keyboard_interactivity` PR, #61420, is open and
unmerged). The spike's job narrows accordingly: verify the shipped mutations
behave at DropTerm's cadence, and prove or refute the keyboard-interactivity
re-grab. No rewrite starts until it reports.

A second axis — desktop neutrality beyond the toolkit — turns out to be a small,
countable adapter surface (finding 9): two trait seams, one config-store
replacement, one per-DE keybind docs matrix. The record rejects a runtime plugin
system and instead specifies compile-time seams, runtime auto-detection, and the
existing D-Bus contract as the open porting API (finding 10).

## What DropTerm has today

Grounding, in the working tree at this record's date (≈66k lines across `src/` +
`applet/`, 53 `.rs` files; **34 files** reference `cosmic::` or `libcosmic` —
30 under `src/`, 4 under `applet/`):

- **Coupling surface, by use-site count** (non-test: lines outside
  `#[cfg(test)]`-gated items, `tests.rs` module files excluded): `cosmic::iced`
  185, `cosmic::theme` 32, `cosmic::app` 12, `cosmic::widget` 12,
  `cosmic::cosmic_theme` 4, `cosmic::task` 5, `cosmic::cosmic_config` 3, plus
  the sibling crates directly: `cosmic_config::` 9, `cosmic_text::` 27,
  `cosmic_theme::` 2.
- **App framework**: `cosmic::app::run_single_instance::<DropTerm>` in
  `src/main.rs:149`; `cosmic::app::Settings` at `src/main.rs:145-147`;
  `cosmic::app::{Core, Task}` at `src/app.rs:9`.
- **Config**: `CosmicConfigEntry`/`cosmic_config` derive and persist
  (`src/config.rs:10`, `src/settings.rs:17`, `local persist` methods at
  `src/settings.rs:651,2096,2120`).
- **Theme**: system dark/light/preference resolution and the override task
  (`src/app/theme_glue.rs:39-43,143-151`).
- **Custom iced widgets** implementing `cosmic::iced::core::widget::Widget`:
  grab bar (`src/grab_bar.rs`), drawer handle (`src/drawer_handle.rs`), slide
  (`src/slide.rs`), status bar (`src/status_bar.rs`), tab bar (`src/tab_bar.rs`).
- **Layer-shell**: `SctkLayerSurfaceSettings` (`src/surface.rs:95-115` measuring
  surface, `:143-157` clipboard anchor); the `set_anchor` + `set_size` +
  `set_margin` triple in `geometry::surface_tasks` (`src/geometry.rs:150-163`);
  `destroy_layer_surface`/`get_layer_surface`/`set_exclusive_zone`/
  `set_keyboard_interactivity`/`set_layer` imported at `src/app.rs:23-26`.
- **Panel applet**: a separate crate whose `libcosmic` features are `applet`,
  `applet-token`, `tokio`, `wayland` (`applet/Cargo.toml:40-42`) — a COSMIC panel
  applet by definition.
- **Runtime identity**: one `APP_ID` const behind the single-instance key, bus
  name, config namespace, state dir, layer-shell namespace, and wayland app id
  (`src/lib.rs:29-46`).
- **The terminal bin's libcosmic feature set**: `wayland`, `wgpu`, `tokio`,
  `dbus-config`, `single-instance` (`Cargo.toml:160-166`).
- **Already cross-DE today** — no COSMIC code at all in: session state
  (`src/session.rs`, `XDG_STATE_HOME` ron + quarantine + atomic save),
  notifications (`src/notify.rs`, direct `org.freedesktop.Notifications`),
  clipboard (`src/clipboard.rs`, `wl-clipboard-rs` data-control), fonts
  (`src/font_list.rs`, fontdb), remote control (`src/dbus_state.rs`, zbus), and
  shell integration (external suggest-generator binaries). No COSMIC
  session-detection code exists anywhere: every `std::env` read is XDG-standard
  (`HOME`, `PATH`, `SHELL`, `XDG_STATE_HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_DIRS`,
  `FLATPAK_ID`).

## Findings

### 1. The coupling is three layers, and only the framework layer is the rewrite

1. **Engine — already portable.** `alacritty_terminal` 0.25.1 and `vte` (both
   patched via `[patch.crates-io]`, `Cargo.toml:47-49`) have no COSMIC
   dependency. OSC 133;D (status-exit), OSC 52 (clipboard), OSC 1337/kitty
   inline images, and the rest of the protocol work live here and in app logic —
   they are UI-framework-agnostic.
2. **Framework — the rewrite.** `cosmic::app`, `cosmic::widget`, `cosmic-config`,
   `cosmic::theme`, and the layer-shell commands are libcosmic/iced-fork APIs with
   no upstream-iced equivalent.
3. **Layer-shell — the crux.** A drop-down terminal *is* a layer-shell surface,
   and libcosmic's iced fork is where the surface commands live.

### 2. What survives any migration

- The terminal engine and its forks; the config data model (the
  `DropTermConfig` serde structs and `completions.toml` schema); the zbus D-Bus
  contract (`src/dbus_state.rs`, the lib's `APP_ID`); `fontdb` enumeration;
  `wl-clipboard-rs`'s data-control clipboard path (already owns DropTerm's
  clipboard I/O independent of the framework, `src/clipboard.rs`); the
  `notify-rust` bell path. Only the *persistence layer* of config (cosmic-config)
  and the *theme source* are coupled.

### 3. DropTerm's layer-shell lifecycle needs five runtime operations on one surface

These are what the spike must reproduce; all currently come from the libcosmic
fork:

- **Measure-then-reshape-in-place**: open a full-output surface
  (`src/surface.rs:95-115`), read the granted size from `Window::Opened`, then
  reshape the *same* surface with an anchor/size/margin triple
  (`geometry::surface_tasks`, `src/geometry.rs:150-163`).
- **Runtime keyboard-interactivity change**: `None` at slide-out start
  (`src/app/surface_lifecycle.rs:331`), re-grab per config on a mid-slide
  reversal (`:403`).
- **Runtime `set_exclusive_zone` / `set_layer`**: the `respect_panel` zone toggle
  and the `Layer::Top`/`Overlay` config change (`src/app.rs:23-26`).
- **Cadenced resize storm**: grab-bar drags re-issue size at an 80 ms cadence with
  a 16 px deadband (`DRAG_APPLY_INTERVAL`, `src/geometry.rs:280`; debounce at
  `:288`), and the granted-size echo is debounced at 150 ms
  (`WINDOW_PIN_INTERVAL`, `src/geometry.rs:318`).
- **Focus events**: `LayerFocused`/`LayerUnfocused` drive `hide_on_focus_loss`
  (`src/app/surface_lifecycle.rs:167-189`).

`.set_anchor/.set_size/.set_margin/.set_exclusive_zone/.set_layer/
.set_keyboard_interactivity` are all runtime commands in the current stack; the
state machine in `surface_lifecycle.rs` is built on them.

### 4. GPUI's layer-shell exists, maps to `surface.rs`, but is create-only

- GPUI is published on crates.io as `gpui` v0.2.2 (2025-10-22, Apache-2.0,
  "Zed's GPU-accelerated UI framework"). The crates.io release predates the
  mid-2026 layer-shell fixes, so a git pin to zed `main` is mandatory.
- Layer-shell history: PR #32651 first added wlr-layer-shell (merged 2025-07-29,
  then reverted in PR #35331 the next day — it crashed Wayland on launch). PR
  #35610 re-landed it (merged 2025-10-29, merge commit
  `b92664c52d7c49bb49acd06d2d40eb23b4e8c601`), restricted to the `wayland`
  feature, and added `crates/gpui/examples/layer_shell.rs`.
- `WindowKind::LayerShell(layer_shell::LayerShellOptions)` carries `namespace`,
  `layer`, `anchor`, `exclusive_zone`, `exclusive_edge`,
  `keyboard_interactivity`, and `margin` (top/right/bottom/left) — the same
  vocabulary as `SctkLayerSurfaceSettings`. The mapping is conceptually clean.
- **The gap, as verified at the surveyed rev** (zed `ec237216460f…`, 2026-11-09):
  three of the five operations already have public runtime APIs on `Window` —
  `resize` (dispatches to `layer_surface.set_size` on Wayland),
  `set_exclusive_zone`, and `set_exclusive_edge` (both committed immediately;
  PR #60163, merged 2026-07-15). Still create-only: anchor, margin, layer, and
  keyboard-interactivity — `Window::set_keyboard_interactivity` is proposed in
  an open, unmerged PR (#61420, 2026-07-21) whose author states keyboard
  interactivity "can currently only be selected when a GPUI window is created."
  The re-land's "no setters yet" note (#35610) was true in 2025-10 and is stale.
- Output selection was broken at first: `cx.displays()` returns empty until after
  a window exists (issue #46378), and the layer surface was created with a `null`
  output (discussion #43749). The landed fix is PR #50520 (merged 2026-03-08;
  the earlier #48502 was closed unmerged), which resolves
  `WindowParams.display_id` to a `wl_output` for `get_layer_surface`, and at the
  surveyed rev `cx.displays()` is populated at startup (outputs bound and
  roundtripped before the first window;
  `gpui_linux/src/linux/wayland/client.rs`). Issue #46378 stays open only for
  further enforcement (open PR #61578); spike check 6 re-verifies on hardware.

### 5. alacritty_terminal on GPUI is proven and packaged

- Zed's own terminal is `alacritty_terminal` + GPUI: `crates/terminal/Cargo.toml`
  depends on both `alacritty_terminal.workspace` and `gpui.workspace`, and
  `crates/terminal_view/README.md` states "The current implementation is still
  Alacritty-backed." `TerminalElement` walks the grid and paints cells through
  GPUI's text batching (`BatchedTextRun`).
- A standalone embeddable crate, `gpui-terminal`, exists: a `TerminalView` GPUI
  entity wrapping `alacritty_terminal` 0.25.1 — verified as reference material,
  not a dependency: MIT OR Apache-2.0, one release (v0.1.0, 2025-12-24; main
  last touched 2026-01-11), and it pins the frozen crates.io `gpui` 0.2.2
  rather than a zed rev, so following the spike's pin means vendoring or
  patching it.
- Consequence: the hardest half of a GPUI terminal — grid → GPU rendering — is a
  reference-implementation problem, not research. The `[patch.crates-io]`
  entries resolve workspace-wide, so the OSC-133 forks still apply.

### 6. GPUI re-introduces `cosmic-text` (a correction to the naive assumption)

GPUI's `wayland` and `x11` feature sets both list `cosmic-text` as a dependency
(gpui v0.2.2 features; `crates/gpui` text system). A GPUI migration therefore
**removes libcosmic and the COSMIC desktop coupling but keeps a pop-os crate in
the tree.** If the goal is literally "zero pop-os code," GPUI fails that specific
test; if the goal is "no libcosmic / no COSMIC desktop," it passes. Note that
`cosmic-text` is a reusable shaping crate (the same class as `alacritty_terminal`),
not desktop integration.

### 7. The alternatives, for the record

- **Upstream `iced` + `iced_layershell`** (v0.19.1, 2026-07-12,
  waycrate/exwlshelleventloop, "layershell binding for iced"): iced 0.14's own
  `wayland` feature is only the winit backend — upstream iced still has no
  layer-shell surface type — so `iced_layershell` is the substitute. A rename-
  heavy port; the custom widgets move to `iced::core::widget`.
- **GTK4 + `gtk4-layer-shell`** (v0.8.1, actively updated): the most mature
  cross-compositor option, but the largest rewrite; the terminal engine becomes a
  decision (adopt `vte4`, or hand-render `alacritty_terminal` into a GL area).
- **`smithay-client-toolkit`** (v0.21.1): the low-level fallback both layer-shell
  options build on.

### 8. Two constraints outside the framework question

- **The panel applet cannot be decoupled.** `applet/` is a COSMIC panel applet by
  construction; a non-COSMIC target drops it or leaves it COSMIC-only.
- **The compositor ceiling is protocol, not toolkit.** A drop-down depends on
  wlr-layer-shell (or kwin's equivalent). Fully supported on COSMIC and wlroots
  compositors (sway, Hyprland, river, Wayfire); GNOME has no first-class support.
  This bound is identical for libcosmic, `iced_layershell`, and `gtk4-layer-shell`.
- **Identity entanglement**: `APP_ID` and the config namespace carry "Cosmic"
  (`src/lib.rs:36-46`). A brand/desktop-independence goal is a separate rename
  touching packaging, desktop file, and bus name.

### 9. The desktop-neutral adapter surface is small and countable

Verified against the tree: no COSMIC session detection exists anywhere (see the
grounding above), so "works on other distros" is a short, named list — not a
hidden-assumptions problem:

| Component | Coupling today | Adapter? | Shape |
|---|---|---|---|
| Surface lifecycle + geometry (`src/surface.rs`, `src/geometry.rs`, `src/app/surface_lifecycle.rs`) | wlr-layer-shell via libcosmic's iced commands | **yes — the real one** | `SurfaceBackend` trait; impls: `layer-shell` (COSMIC, sway, Hyprland, river, Wayfire, labwc, KDE), `gnome-toplevel` (Guake-style; GNOME has no layer-shell) |
| Theme / dark mode (`src/app/theme_glue.rs`, `src/settings.rs`) | cosmic theme store, `system_light/dark` | **yes, small** | `ThemeSource` trait; impls: cosmic store, XDG portal `color-scheme`, static config; the `COSMIC Dark/Light` default scheme names (`src/config.rs:33-36`) need a per-DE default pair |
| Config persistence (`src/settings.rs`, cosmic-config) | cosmic-config crate; works on any DE but is pop-os code under COSMIC paths | **replace, not adapt** | plain `XDG_CONFIG_HOME` ron — `src/session.rs` already runs this pattern |
| Global summon keybind | none in code — an external binding to `dropterm toggle` (`README.md:67`, the `toggle` recipe at `justfile:257`) | **not code** | a per-DE integration matrix (docs); optional XDG GlobalShortcuts portal impl later |
| Panel presence (`applet/`) | COSMIC panel applet | **out of core** | separate per-DE programs (GNOME extension, KDE widget, swaybar module) consuming the D-Bus contract |
| Remote control (`src/dbus_state.rs`, CLI) | none | **no — it is the extension surface** | the V1 contract is the language-agnostic API the integrations above port against |
| Clipboard, notifications, fonts, session state, terminal engine | none (data-control, `org.freedesktop.Notifications`, fontdb, XDG ron, alacritty_terminal) | **no** | portable as-is |
| Shell integration (autosuggest/completions) | none | **no** | cross-shell already; external suggest-generator binaries are an existing subprocess-extension precedent |
| Identity (`src/lib.rs:36-46`, desktop file, bus name) | the "Cosmic" brand string | **decision, not adapter** | a rename with a config/D-Bus migration story |
| App framework (`src/app.rs`, widgets) | libcosmic | **yes** | the GPUI rewrite (findings 1–5) |

Two structural notes. First: **compositor quirks live even inside one protocol** —
`src/config.rs:216` already documents cosmic-comp's focus-fixup special case, and
the `geometry` echo-debounce machinery exists for cosmic-comp's configure
behavior — so `SurfaceBackend` should be one impl *per protocol* with
per-compositor tuning knobs, not one impl per compositor. Second: the adapter
layer is **orthogonal to the toolkit choice** — options A–D all carry the same
seams, except A (keep libcosmic) cannot use the theme/config replacements.

### 10. Extension strategy: compile-time seams, not a runtime plugin system

For the backend seams, a runtime plugin system (`.so`/WASM loading, stable ABI,
manifests) is rejected:

- **Rust has no stable ABI**; dynamic backends force `abi_stable` or a C ABI
  across a stateful, event-driven surface contract — a maintenance burden and a
  type straitjacket for the smallest part of the app.
- **Even Zed keeps platform backends compile-time.** Zed's extension system is
  WASM and serves languages/themes, not window backends.
- **The project's own precedent is subprocess + contract, not dynamic loading.**
  The applet is a separate process driving DropTerm over the V1 contract; suggest
  generators are external binaries.

What "others port their own openly" needs instead:

1. **Small stable trait seams** — `SurfaceBackend`, `ThemeSource` — in a
   `dropterm-platform` crate with the contract documented.
2. **Compile-time backend crates** (workspace members or cargo features);
   out-of-tree crates can implement the traits via a path/git dependency, and GPL
   makes that open. The honest caveat: shipping a backend still means a fork or a
   PR (one Cargo.toml line + feature) since the app is one binary — a runtime
   loader would remove only that, at the costs above.
3. **Runtime auto-detection, not runtime loading.** One binary can ship both
   backends and dispatch at startup — check for the `zwlr_layer_shell_v1` global
   plus `XDG_CURRENT_DESKTOP`/`DESKTOP_SESSION` — no plugin loader, no ABI to
   freeze. (Spike check 11.)

The genuinely open ecosystem surface already exists: **D-Bus V1 + the CLI**.
Anyone can build a GNOME extension, KDE widget, or swaybar module against it
today, in any language, without touching Rust; the applet is its proof. What it
lacks is integrator-facing contract docs — today the applet is the only
documented consumer.

## The spike (the record's deliverable)

One question gates the rewrite: **can GPUI's layer-shell reproduce the five
runtime operations in finding 3?** The spike is a standalone scratch crate
(`spikes/gpui-layer-shell/`, gitignored or a throwaway branch) pinning `gpui` from
zed `main` at a fresh rev — with a committed lock, same discipline as the
libcosmic pin — `wayland` feature only. Every check is manual on COSMIC hardware
(layer-shell has no headless path); sway is the cross-compositor witness. Same
constraint as [`../testing.md`](../../testing.md).

| # | Check | Pass criterion | Fallback if it fails |
|---|---|---|---|
| 0 | Boot & map | `LayerShell` window maps and renders on COSMIC *and* sway; no crash (the first PR crashed Wayland on launch — this is a regression check) | NO-GO for GPUI |
| 1 | Measure | Full-anchor, no-size surface reports granted bounds matching the output | — |
| 2 | Reshape in place | The API exists at the surveyed rev (`Window::resize` → `layer_surface.set_size`); verify on hardware that the compositor honors repeated `set_size` at the 80 ms drag cadence, and record the protocol caveat (an axis anchored at both edges is compositor-controlled; only the un-anchored axis is client-settable) | **REDESIGN path**: per-show destroy/create; measure create→mapped latency and flicker against the 130 ms slide budget |
| 3 | Keyboard interactivity at runtime | No public path at the surveyed rev; `Window::set_keyboard_interactivity` is proposed in unmerged PR #61420 — evaluate carrying that patch, and measure whether keeping keyboard held during hide degrades acceptably instead | Hardest failure: `reverse_show` re-grabs focus on the live surface mid-hide. No path = fork GPUI (carrying #61420 like the vte/alacritty forks) or redesign (keep keyboard held during hide; lose focus-return) |
| 4 | exclusive_zone / layer at runtime | `set_exclusive_zone`/`set_exclusive_edge` are public and implemented (PR #60163) — verify their behavior; `set_layer` stays create-only — verify the recreate-on-change fallback | Redesign: recreate on layer change, or shrink the feature |
| 5 | Focus events | Layer window emits focus/unfocus that can drive `hide_on_focus_loss` | NO-GO without a fork |
| 6 | Output targeting | `cx.displays()` populated at startup; surface lands on the active output on both compositors | Known workaround: ~100 ms delay + retry (issue #46378); record as debt if needed |
| 7 | Resize echo | Determine whether GPUI double-reports each resize (libcosmic's stale synthetic configure) | Tells us whether the `geometry` pin-debounce machinery ports or can be deleted |
| 8 | Clipboard | Whether GPUI's Wayland clipboard needs a bound surface (the anchor hack) or is surface-independent | Anchor likely deleted; the `wl-clipboard-rs` path is untouched either way |
| 9 | Terminal grid render | `alacritty_terminal` grid → GPUI text render: PTY, colors/attributes, damage redraw, resize→SIGWINCH; verify the patched forks resolve and evaluate `gpui-terminal` (license, maintenance, API fit) | Zed's terminal proves the shape; this is a crate-choice check |
| 10 | D-Bus interop | zbus (tokio) single-instance + the V1 contract running beside GPUI's executor | `dbus_state` is framework-agnostic; this only proves the loops coexist |
| 11 | Backend detection | Startup dispatch picks the layer-shell impl on COSMIC and sway and reports `gnome-toplevel (unimplemented)` on GNOME — proving the `SurfaceBackend` seam compiles and dispatches without deciding the GNOME design | The seam is rewrite work, not spike work; detection can be deferred entirely |

**Decision criteria**

- **GO**: checks 0–2 pass; 3 and 4 have workable runtime paths or acceptable
  fallbacks; 5–6 pass.
- **REDESIGN**: in-place reshape impossible but per-show create/destroy fits the
  latency budget; keyboard re-grab redesigned. The `surface_lifecycle.rs` state
  machine survives; only its executors change.
- **NO-GO / fork**: check 3 fails with no redesign preserving focus-return; the
  choice becomes forking GPUI's layer-shell (carrying a patch, like the existing
  vte/alacritty forks) versus falling back to `iced_layershell`.

**Not in the spike**: widgets, tabs, settings UI, config watch, theming, slide
animation, the applet. Those are either framework-agnostic or known rewrite work
with no open questions.

**Effort** (work units, not wall-clock): (1) scaffold + boot check, (2) lifecycle
checks 1–8 + backend detection (11), (3) grid render, (4) D-Bus interop,
(5) findings report with the GO/REDESIGN/NO-GO verdict and the pin rev. Items
1–4 are hardware-session work; the scratch crate should stay under ~500 lines.

## Recommendation

| Option | What it removes | UI work | Engine | Compositor reach | Risk |
|---|---|---|---|---|---|
| **A. Session-decouple, keep libcosmic** | COSMIC-session assumptions (theme/config/identity) | none | kept | wlroots + COSMIC | low |
| **B. GPUI rewrite (this record's subject)** | libcosmic + COSMIC desktop (not `cosmic-text`) | full rewrite of the UI layer | kept; grid render proven by Zed/`gpui-terminal` | wlroots + COSMIC | medium — young create-only layer-shell API |
| C. `iced_layershell` rewrite | libcosmic + `cosmic-text` | rename-heavy port of ~30 files | kept | wlroots + COSMIC | medium — fork bus-factor |
| D. GTK4 + `gtk4-layer-shell` | everything pop-os | full rewrite | kept only with a GL grid, else `vte4` | wlroots + COSMIC | high — largest, engine question |

**Recommended next step: run the spike (option B gate), not the rewrite.** GPUI is
now the strongest "drop libcosmic" candidate on paper because its two former
blockers (layer-shell, alacritty integration) are both closed — and the risk
this record opened with has shrunk: resize, exclusive zone, and exclusive edge
gained public runtime APIs before the surveyed rev. What remains is a young API
surface that is still partially create-only (keyboard-interactivity above all),
and it does not remove `cosmic-text`. If the spike passes option B is the path;
if it fails on check 2 only, a per-show create/destroy redesign is likely; if it
fails on check 3 with no redesign, `iced_layershell` (option C) is the fallback.

The adapter layer (findings 9–10) is orthogonal to that choice and ships with
whichever option wins: two trait seams, one config-store replacement, one per-DE
keybind docs matrix, and integrator-facing D-Bus contract docs — no runtime
plugin system.

## Unverified / open questions

- Whether keyboard-interactivity mutation lands upstream (#61420) or the spike
  must carry it as a patch — the one remaining source-unresolved mutation
  (resize/zone/edge are public as of the surveyed rev); checks 2–4 verify
  compositor behavior on hardware.
- Whether the 2026-05-08 output-selection fix is present in the rev the spike
  pins; and whether the active-output semantics (IcedOutput::Active today) have a
  GPUI equivalent without the documented startup race.
- Whether GPUI's resize stream double-reports each `set_size` as libcosmic's does
  (check 7); the `geometry` debounce machinery's necessity depends on it.
- `gpui-terminal`'s API fit versus adapting Zed's `terminal_view` as a reference
  only — license (MIT OR Apache-2.0) and maintenance (one release, 2025-12-24;
  pins crates.io gpui 0.2.2) are now known, and the pin mismatch makes it
  vendored/patched material at best.
- GPUI's API stability and external-consumer support: the crates.io surface is
  best-effort and Zed-driven (v0.2.2 is the last release at this date, with real
  work on `main`); the spike pins one rev and its findings are true for that rev.
- The `gnome-toplevel` backend's feasibility: Wayland clients cannot
  self-position toplevel windows, so Guake-style placement needs protocol-side
  positioning (xdg-positioner) or compositor cooperation — an open design
  question deferred past the layer-shell spike.

## Sources

Surveyed 2026-11-09.

- **DropTerm working tree** (this repo, 2026-11-09): `src/surface.rs`,
  `src/geometry.rs`, `src/app/surface_lifecycle.rs`, `src/app.rs`, `src/main.rs`,
  `src/config.rs`, `src/settings.rs`, `src/app/theme_glue.rs`, `src/lib.rs`,
  `src/session.rs`, `src/notify.rs`, `src/clipboard.rs`, `src/font_list.rs`,
  `src/keybinds.rs`, `src/terminal/suggest.rs`, `README.md`, `Cargo.toml`,
  `applet/Cargo.toml`.
- **gpui** @ crates.io v0.2.2 (published 2025-10-22, Apache-2.0):
  <https://crates.io/crates/gpui>.
- **Zed GPUI layer-shell**, `crates/gpui/src/platform/linux/wayland/layer_shell.rs`
  (`LayerShellOptions`, `Layer`, `Anchor`, `KeyboardInteractivity`) —
  <https://github.com/zed-industries/zed/tree/main/crates/gpui>.
- **Zed PR #32651** "gpui: Implement support for wlr layer shell" (merged
  2025-07-29, reverted in #35331) —
  <https://github.com/zed-industries/zed/pull/32651>.
- **Zed PR #35610** re-land (merged 2025-10-29, `b92664c5`) —
  <https://github.com/zed-industries/zed/pull/35610>.
- **Zed issue #46378** `displays()` empty at startup / layer-shell output
  selection (fixed by PR #50520, merged 2026-03-08; the earlier #48502 was
  closed unmerged; open follow-up #61578) —
  <https://github.com/zed-industries/zed/issues/46378>.
- **Zed PR #60163** "gpui: Linux wayland set exclusive zone and edge" (merged
  2026-07-15; the runtime `Window::set_exclusive_zone`/`set_exclusive_edge`) —
  <https://github.com/zed-industries/zed/pull/60163>.
- **Zed PR #61420** "gpui: Allow changing wayland layer-shell keyboard
  interactivity" (open, unmerged, 2026-07-21) —
  <https://github.com/zed-industries/zed/pull/61420>.
- **zed @ `ec237216460f8a34a4ac7c6b068e838593976ede`** (main, 2026-11-09):
  `crates/gpui/src/window.rs` (`resize`, `set_exclusive_zone`,
  `set_exclusive_edge`), `crates/gpui_linux/src/linux/wayland/{window,client}.rs`
  (the Wayland impls; outputs bound before the first window).
- **Zed discussion #43749** choosing the monitor for a layer surface —
  <https://github.com/zed-industries/zed/discussions/43749>.
- **Zed terminal** `crates/terminal/Cargo.toml` (`alacritty_terminal.workspace`,
  `gpui.workspace`) and `crates/terminal_view/README.md` ("still
  Alacritty-backed") —
  <https://github.com/zed-industries/zed/tree/main/crates/terminal>.
- **gpui-terminal** crate (`TerminalView`, `alacritty_terminal` 0.25.1-backed;
  v0.1.0, 2025-12-24, MIT OR Apache-2.0; pins crates.io `gpui` 0.2.2) —
  <https://docs.rs/gpui-terminal>, <https://github.com/zortax/gpui-terminal>.
- **iced** @ crates.io v0.14.0 (2025-12-07; `wayland` feature is the winit
  backend, no layer-shell surface type) — <https://crates.io/crates/iced>.
- **iced_layershell** @ crates.io v0.19.1 (2026-07-12, waycrate) —
  <https://crates.io/crates/iced_layershell>.
- **smithay-client-toolkit** @ crates.io v0.21.1 —
  <https://crates.io/crates/smithay-client-toolkit>.
- **gtk4-layer-shell** @ crates.io v0.8.1 —
  <https://crates.io/crates/gtk4-layer-shell>.
