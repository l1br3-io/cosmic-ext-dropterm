# Why does DropTerm show up in the window switcher and dock — and what can remove it?

Date: 2026-10-24

## TL;DR

The app's own surfaces never appear in the switcher or dock — its
clipboard anchor does. The anchor is a 1×1 **xdg toplevel** that
winit cannot unmap on Wayland, and COSMIC's toplevel-info protocol
exists to list exactly those windows to "clients such as taskbars or
docks". No protocol request can hide a toplevel from those listings,
so the fix is to make the anchor a **layer surface** — a window kind
the listing protocols structurally cannot see — selected by a
`show_in_dock` key whose `true` keeps today's dock presence. Verdict
shipped as the shell-presence pass.

## Findings (read from the vendored sources, 2026-10-24)

1. **The anchor is a mapped toplevel by construction.**
   `src/app.rs`'s `clipboard_anchor_settings` opens a 1×1
   `window::open` toplevel with `visible: false` — but the pop-os
   winit fork's Wayland backend implements `set_visible` as a literal
   no-op (`winit-wayland/src/window/mod.rs:397`: "Not possible on
   Wayland"), so the toplevel is mapped for the whole process
   lifetime. winit at `0.31.0-beta.2`, rev `9567503` per Cargo.lock.

2. **The listings are toplevel-only, by protocol.** COSMIC's
   `zcosmic_toplevel_info_unstable_v1` describes itself as serving
   "clients such as taskbars or docks" that need "a list of opened
   applications" — and its objects are xdg toplevels
   (`ext_foreign_toplevel_v1` extended). Layer surfaces have no entry
   anywhere in it. The companion `zcosmic_toplevel_management_v1`
   offers no skip request either: its full request set is destroy,
   close, activate, set/unset_maximized, set/unset_minimized,
   set/unset_fullscreen, set_rectangle (read from the
   `cosmic-protocols` checkout at `c0cff4d`). An SNI/status-notifier
   icon is a different mechanism entirely — a presence *added*, never
   one *removed*.

3. **Why the anchor exists at all — the clipboard binding.** iced's
   runtime keeps one app-wide `Clipboard`
   (`iced/winit/src/clipboard.rs`): it is bound via
   `Clipboard::connect(first window)` and reset to `unconnected` only
   when the window it holds is removed from the window manager
   (three destroy paths in
   `iced/winit/src/platform_specific/wayland/sctk_event.rs:740,957,1703`
   all guard `clipboard.window_id() … w.raw.id() == id` before
   resetting). Any never-destroyed window therefore keeps the binding
   — and the copied selection — alive across every hide.

4. **A layer surface can be the anchor.** The connect sites are not
   toplevel-only: `sctk_event.rs:1009` and `:1247` bind the clipboard
   to layer-surface windows as they are created (the guard is
   `clipboard.window_id().is_none()` — first window of *any* kind
   wins). So a layer-surface anchor created before the terminal's own
   surface owns the binding identically.

5. **A layer surface can be made inert.**
   `SctkLayerSurfaceSettings.input_zone` is applied by the runtime
   (`iced/winit/src/platform_specific/wayland/event_loop/state.rs`
   creates a compositor region, adds each rectangle, then
   `wl_surface.set_input_region`) — so `Some(vec![])` is an empty
   input region: no pointer events at all. With
   `keyboard_interactivity: None`, a 1×1 `Bottom`-layer surface with
   `exclusive_zone: -1` neither paints over anything, takes input,
   nor claims space.

## What DropTerm had today (the 2026-10-24 snapshot)

The xdg anchor with its app-id pin — correct on its own terms (the
switcher entry at least carried DropTerm's name and icon instead of
"unknown"), and the ROADMAP's v1 promise "Hidden from Alt-Tab and
dock" had been silently broken by the anchor's arrival. The maintainer
filed it as the bug row "app is not hidden from alt tab" and asked for
configurability keyed to the panel applet.

## Recommendation (shipped)

Give the anchor two bodies behind `show_in_dock` (default `false`):
the hidden body is `surface::anchor` — the layer surface of finding 5,
namespaced `<app id>.anchor` so no namespace-keyed shell rule matches
it; the dock body is today's toplevel. A flip swaps them live,
destroy-before-create so the new body's creation event rebinds the
clipboard immediately (finding 3's reset only fires on removal); the
selection on the clipboard dies once per flip — the documented cost.
The panel applet — the v3 record's out-of-process handle — is the
persistent summon path in hidden mode and needs nothing new.

Rejected alternatives:

- **Asking the compositor to hide the toplevel** — no such request
  exists in any COSMIC or xdg-shell protocol DropTerm can speak
  (finding 2); a skip-taskbar hint would be X11 `_NET_WM_STATE`
  thinking on a Wayland desktop.
- **An SNI tray icon as the hiding mechanism** — an indicator adds a
  presence; it cannot remove the dock/switcher one. The applet already
  plays the indicator role natively.
- **Destroying the anchor while hidden, recreating on show** —
  destroys the clipboard selection at every hide, the exact bug the
  anchor was built to fix.
- **Minimizing the dock body** — minimized toplevels stay in switcher
  and dock listings; `xdg_toplevel.set_minimized` removes nothing.

## Sources

- winit (pop-os fork) `0.31.0-beta.2`, rev `9567503` —
  `winit-wayland/src/window/mod.rs` (`set_visible` no-op), local
  cargo checkout, read 2026-10-24.
- libcosmic git `08a09e3` (Cargo.lock) — `iced/winit/src/clipboard.rs`,
  `iced/winit/src/platform_specific/wayland/sctk_event.rs`,
  `iced/winit/src/platform_specific/wayland/event_loop/state.rs`,
  `iced/runtime/src/platform_specific/wayland/layer_surface.rs`,
  local cargo checkout, read 2026-10-24.
- cosmic-protocols checkout `c0cff4d` —
  `unstable/cosmic-toplevel-info-unstable-v1.xml`,
  `unstable/cosmic-toplevel-management-unstable-v1.xml`, read
  2026-10-24.
