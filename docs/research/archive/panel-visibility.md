# Research: can the COSMIC panel stay visible while the drop-down is open?

Date: 2026-10-01
Question: *Is it possible for DropTerm to make the panel visible while the dropdown terminal is open?*

## TL;DR

**Yes — a small code change, no compositor changes.** The drop-down covers the panel
today only because DropTerm asks for `exclusive_zone = -1` when shown
(`src/app.rs:734`). Per the wlr-layer-shell protocol, `-1` means *"do not move me to
accommodate other surfaces; extend me all the way to my anchored edges."* If the
shown surface instead requests `exclusive_zone = 0`, the compositor (cosmic-comp,
via Smithay's layer map) places it **inside the usable area** — directly below a
top-edge panel's reserved strip — leaving the panel visible and clickable. This
works on the Overlay layer, so the drop-down stays above fullscreen windows, which
is the app's headline behavior. Recommended shape: keep `-1` as the default and put
`0` behind a config knob (e.g. `respect_panel`), since both behaviors are
legitimate preferences for a quake-style terminal.

There is **no config-only path**: switching `layer` to `Top` alone does not help,
because the `-1` zone still overlaps the panel's strip (and Top loses to focused
fullscreen windows).

## What DropTerm does today

- Shown surface: `set_anchor(TOP|LEFT|RIGHT)` (or bottom), `set_layer` from config
  (default `Overlay`), keyboard `Exclusive`, and `set_exclusive_zone(id, -1)`
  — `src/app.rs:725-735`.
- Measuring surface: anchors all four edges with `exclusive_zone: 0`
  — `src/surface.rs:64-77`. Because a zone-0 surface is stretched into the
  *usable* area (see Smithay findings below), the output height DropTerm measures
  **already excludes the panel strip** on systems with an exclusive panel. So
  `height_percent` semantics stay consistent if the shown surface also uses zone 0.

## Findings

### 1. The protocol: zone 0 vs -1 is exactly this choice

wlr-layer-shell-unstable-v1, `zwlr_layer_surface_v1.set_exclusive_zone`
(rendered from the protocol XML at <https://wayland.app/protocols/wlr-layer-shell-unstable-v1>):

> "Surfaces that do not wish to have an exclusive zone may instead specify how they
> should interact with surfaces that do. **If set to zero, the surface indicates
> that it would like to be moved to avoid occluding surfaces with a positive
> exclusive zone. If set to -1, the surface indicates that it would not like to be
> moved to accommodate for other surfaces, and the compositor should extend it all
> the way to the edges it is anchored to.**" … "For example, a panel might set its
> exclusive zone to 10 … **A notification might set its exclusive zone to 0, so that
> it is moved to avoid occluding the panel** … A wallpaper or lock screen might set
> their exclusive zone to -1, so that they stretch below or over the panel. The
> default value is 0."

The spec's "notification" example is precisely DropTerm's situation. Also from the
spec: layers are ordered background < bottom < top < overlay, "ordering within a
single layer is undefined", and fullscreen shell surfaces are only "typically" at
the top layer — layer/zone are independent knobs.

### 2. Smithay: where cosmic-comp actually implements placement

cosmic-comp's layer-shell handler is a thin shim — `new_layer_surface` pushes a
`PendingLayer`, later mapped via Smithay's `LayerMap::map_layer`
([cosmic-comp `src/wayland/handlers/layer_shell.rs`](https://github.com/pop-os/cosmic-comp/blob/master/src/wayland/handlers/layer_shell.rs),
[`src/shell/mod.rs` `map_layer` L3066-3089](https://github.com/pop-os/cosmic-comp/blob/master/src/shell/mod.rs#L3066-L3089)).
The placement algorithm is
[`LayerMap::arrange` in `Smithay/smithay` `src/desktop/wayland/layer.rs`](https://github.com/Smithay/smithay/blob/master/src/desktop/wayland/layer.rs):

- Surfaces are placed in two passes: **exclusive surfaces first, then
  non-exclusive** (`exclusive_surfaces.chain(non_exclusive_surfaces)`).
- Each exclusive surface then shrinks the remaining zone, verbatim:
  `Some(Anchor::TOP) => { let sum = amount + margin.top; zone.loc.y += sum; zone.size.h -= sum; }`
  (and symmetric for bottom/left/right).
- The layout source rectangle for each surface is chosen **only** by its zone,
  not by its layer:
  `Exclusive(_) | Neutral => zone` (the remaining usable area), `DontCare => full output_rect`.

So a zone-0 ("Neutral") surface is laid out inside the area that survives after the
panel took its strip — **regardless of whether the surface is on the Top or the
Overlay layer**. Placement is re-run on every commit:
[cosmic-comp `src/wayland/handlers/compositor.rs` L372](https://github.com/pop-os/cosmic-comp/blob/master/src/wayland/handlers/compositor.rs#L372)
calls `layer_map_for_output(&output).arrange()`, so a runtime
`set_exclusive_zone` re-arranges the surface live, no remap needed.

### 3. cosmic-panel: what the panel asks for

From [pop-os/cosmic-panel](https://github.com/pop-os/cosmic-panel) (`master`,
checked out 2026-10-01):

- The panel's layer is **configurable, default `Layer::Top`**
  (`cosmic-panel-config/src/panel_config.rs` L421 `pub layer: Layer`, L506 default).
  It is temporarily forced to `Overlay` only while the workspace-overview is open
  (`cosmic-panel-bin/src/space/panel_space.rs` L1082, with comment
  *"Force panel to `Overlay` layer, so it will be above workspaces overlay. TODO: Better solution?"*).
  The root surface creation, including layer and keyboard interactivity, is in
  `cosmic-panel-bin/src/space/wrapper_space.rs` L1535-1563.
- Default `exclusive_zone: true` (panel_config.rs L518), and
  `exclusive_zone() == self.exclusive_zone || !self.autohide_enabled()`
  (panel_config.rs L654-655) — **a panel without autohide always reserves its
  strip** by setting zone = its thickness (panel_space.rs L1268, L1282).
- `AutoHide` is `Never` (default) / `OnOverlap` (intellihide) / `Always`
  (panel_config.rs L314-321).
- Overlap bookkeeping is split: `has_toplevel_overlap()` for windows vs
  `insert_layer_overlap` / `has_layer_overlap()` for layer surfaces
  (panel_space.rs L492-512). A layer overlap does **not** hide the panel; the panel
  computes which end-segment is covered and re-lays-out its content around it
  (`apply_layer_overlaps`, panel_space.rs L514-548).

### 4. cosmic-comp: how overlap-notify feeds the panel's intellihide

`OverlapNotifyState::refresh`
([`src/wayland/protocols/overlap_notify.rs` L80-165](https://github.com/pop-os/cosmic-comp/blob/master/src/wayland/protocols/overlap_notify.rs))
intersects each subscribed layer surface (the panel) with (a) toplevels on active
workspaces or sticky, and (b) **other layer surfaces** (`add_layer`). The panel
client handles those events in
`cosmic-panel-bin/src/xdg_shell_wrapper/client/handlers/overlap.rs` L99-106.

Consequence for DropTerm: with today's zone `-1`, an open drop-down geometrically
intersects the panel and is reported as a layer overlap (panel reflows content if
intellihide is on). With zone `0` there is no intersection at all, so nothing is
reported — the panel is simply untouched.

### 5. cosmic-comp: stacking and keyboard focus (the trade-offs)

From `src/shell/focus/order.rs` (`render_input_order`, the canonical z-order walk):

- Overlay: *"overlay is above everything"* (comment at L181-182) — no
  fullscreen gate.
- Top layer: popups and surfaces are skipped when a focused fullscreen window
  exists — `if !has_focused_fullscreen { … layer_popups(output, Layer::Top …)`
  (L298-306) and `if !has_focused_fullscreen { // top-layer shell … }` (L408-417).
  This confirms the comment in DropTerm's `src/config.rs` (`Top` sits below
  fullscreen windows) — so **keep the default `Overlay` layer** for this feature.
- Within one layer, iteration is `layers_on(layer).rev()` (L522): last-mapped is
  topmost. The protocol leaves same-layer order undefined; cosmic-comp uses map
  order. (Relevant only if DropTerm shared the panel's Top layer — not recommended.)

Keyboard focus, verbatim comment in `src/shell/focus/mod.rs` (~L655-660):

> "If an exclusive layer shell surface exists (on any output), only exclusive
> shell surfaces can have focus, on the highest layer with exclusive surfaces.
> Popups are judged by their root surface, so an exclusive surface can still open
> grabbing popups (menus, dropdowns, context menus)."

So while DropTerm holds `KeyboardInteractivity::Exclusive`, panel popovers cannot
take *keyboard* focus (panel is on Top, DropTerm's exclusive surface is on
Overlay). Pointer input is geometric and unaffected: panel buttons remain
clickable next to an open drop-down. This is arguably desirable for a quake
terminal (keys go to the shell), but users who want to type into panel popovers
while the terminal is open would need `keyboard_mode: OnDemand`, which already
exists as a config option.

### 6. libcosmic plumbing (DropTerm's pinned rev `08a09e3`)

Verified in the local checkout
(`~/.cargo/git/checkouts/libcosmic-41009aea1d72760b/08a09e3/`, i.e.
[pop-os/libcosmic @ 08a09e3](https://github.com/pop-os/libcosmic/tree/08a09e3dfa1f729d52585fcedd6e731130235363)):

- `iced/winit/src/platform_specific/wayland/commands/layer_surface.rs` L60-70:
  `pub fn set_exclusive_zone<Message>(id: SurfaceId, zone: i32)` is a thin
  passthrough whose doc comment links the protocol section — `0` / `-1` mean
  exactly the spec semantics. It forwards to the SCTK layer surface in
  `iced/winit/src/platform_specific/wayland/event_loop/state.rs` (L1056, L1262).
- `iced/runtime/src/platform_specific/wayland/layer_surface.rs` L44-69:
  `SctkLayerSurfaceSettings { layer, keyboard_interactivity, anchor, margin, size,
  exclusive_zone: i32, … }` — everything needed already exists, and `src/app.rs`
  already imports `set_exclusive_zone`, `set_anchor`, `set_layer`,
  `set_keyboard_interactivity`, `set_size`.

### 7. Prior art: cosmic-ext-quake-terminal

[cosmic-ext-quake-terminal](https://github.com/M0Rf30/cosmic-ext-quake-terminal)
is **not** a layer-shell app: `src/wayland.rs` drives a regular cosmic-term
*toplevel* via `cosmic_client_toolkit` toplevel-info/toplevel-management
(minimize/activate). It is the "minimize/restore window trick" DropTerm's README
explicitly avoids, so it has no layer/exclusive-zone decision to compare. As a
normal window it appears in Alt-Tab, and — fun asymmetry — the panel's intellihide
*would* react to it, because it counts as a toplevel overlap.

## Options for DropTerm

| Option | Change | Panel visible? | Above fullscreen? | Notes |
|---|---|---|---|---|
| **A. zone `0` when shown (recommended)** | `src/app.rs:734`: `set_exclusive_zone(id, 0)` | ✅ sits flush below panel strip (top edge) or above dock (bottom edge) | ✅ stays on `Overlay` | `height_percent` keeps meaning "share of usable area" (the measuring surface already measures the usable area) |
| B. config-only `layer: Top` | none | ❌ still overlaps panel (zone stays `-1`) | ❌ loses to focused fullscreen | Rejected — doesn't achieve the goal |
| C. config knob for A | new field (e.g. `respect_panel: bool`, default `false`) → zone `0` when set | ✅ opt-in | ✅ | Best of both worlds; `false` preserves current Yakuake-like cover behavior |

Details worth knowing for option A/C:

- **Autohide panels**: while hidden, an autohide panel keeps a small exclusive
  strip (its handle) — panel sets a negative margin so the reserved zone shrinks
  to the handle size (panel_space.rs L1276-1282 + Smithay's
  `sum = amount + margin` inset). A zone-0 drop-down lands below that handle strip.
  Cosmetic at worst (a few px), and only with autohide enabled.
- **Edge interaction**: with `edge: Bottom` and a *top* panel, the panel is already
  visible today (no overlap); zone `0` additionally respects a bottom dock.
- **Intellihide**: zone `0` produces no overlap notification at all; today's `-1`
  reports a layer overlap (panel reflows content around it if configured).
- **Keyboard**: nothing changes vs today — panel popovers can't take keyboard focus
  while DropTerm holds `Exclusive` either way (cosmic-comp rule above). Pointer
  interaction with the panel keeps working since the surfaces no longer overlap.

### What would change in the code (descriptive, not a patch)

- `src/app.rs` ~L734: choose the zone value from config instead of the hardcoded
  `-1`.
- `src/config.rs`: new bool (or tri-state) field with a default preserving today's
  behavior; `DropTermConfig` is `#[version = 1]` — adding a field should be picked
  up leniently by cosmic-config (missing key → default), but **verify** against
  cosmic-config's deserialization behavior before shipping (see open questions).
- `README.md` config docs + manual test checklist entries (panel visible while
  open, both edges, with/without autohide panel).
- Tests: unit-test the config mapping only; visual behavior needs the manual
  COSMIC checklist per `AGENTS.md`.

## Unverified / open questions

- Exact default of the panel *root* surface's `keyboard_interactivity`
  (it comes from `xdg_shell_wrapper_config` in wrapper_space.rs L1554-1563; the
  default value was not pinned down). Doesn't change any conclusion — cosmic-comp's
  exclusive-focus rule applies regardless of what the panel requests.
- cosmic-config's exact behavior when a new field appears in a `#[version = 1]`
  `CosmicConfigEntry` (expected: missing key falls back to the field default;
  verify in cosmic-config source or with a live config before relying on it).
- Whether the panel's `expand_to_edges` / margin settings shift the reserved strip
  on any edge case; Smithay adds margins into the reserved zone
  (`sum = amount + margin`), so the general case is covered, but a real-hardware
  pass with a margin-configured panel is worth a checklist line.

## Sources

- wlr-layer-shell-unstable-v1 protocol text (rendered XML):
  <https://wayland.app/protocols/wlr-layer-shell-unstable-v1>
- cosmic-comp @ master (cloned 2026-10-01):
  `src/wayland/handlers/layer_shell.rs`; `src/shell/mod.rs` L3066-3089;
  `src/shell/focus/order.rs` L75-200, L298-306, L408-417, L510-535;
  `src/shell/focus/mod.rs` L440-460, L650-700; `src/wayland/handlers/compositor.rs` L372;
  `src/wayland/protocols/overlap_notify.rs` L80-165 —
  <https://github.com/pop-os/cosmic-comp>
- cosmic-panel @ master (cloned 2026-10-01):
  `cosmic-panel-config/src/panel_config.rs` L291-321, L421, L449-455, L506, L518-520, L654-655;
  `cosmic-panel-bin/src/space/panel_space.rs` L492-548, L940-1040, L1060-1092, L1268-1282;
  `cosmic-panel-bin/src/space/wrapper_space.rs` L1535-1563;
  `cosmic-panel-bin/src/xdg_shell_wrapper/client/handlers/overlap.rs` L99-106 —
  <https://github.com/pop-os/cosmic-panel>
- Smithay @ master: `src/desktop/wayland/layer.rs` (`LayerMap::arrange`,
  `map_layer`, `effective_exclusive_zone`) —
  <https://github.com/Smithay/smithay/blob/master/src/desktop/wayland/layer.rs>
- libcosmic @ `08a09e3` (DropTerm's pinned rev, local cargo checkout):
  `iced/winit/src/platform_specific/wayland/commands/layer_surface.rs` L40-100;
  `iced/winit/src/platform_specific/wayland/event_loop/state.rs` L1056, L1262;
  `iced/runtime/src/platform_specific/wayland/layer_surface.rs` L44-69 —
  <https://github.com/pop-os/libcosmic/tree/08a09e3dfa1f729d52585fcedd6e731130235363>
- cosmic-ext-quake-terminal @ master (cloned 2026-10-01): `src/wayland.rs`,
  `Cargo.toml` — <https://github.com/M0Rf30/cosmic-ext-quake-terminal>
