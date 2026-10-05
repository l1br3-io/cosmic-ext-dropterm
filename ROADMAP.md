# DropTerm Roadmap

High-level feature list.

## v1 - MVP

- [x] Toggle via second launch of the binary
- [x] Layer-shell overlay anchored to the top or bottom edge, full width, above other windows
- [x] Hidden from Alt-Tab and dock
- [x] Exclusive keyboard focus when shown; focus returned when hidden
- [x] Shell session persists while hidden
- [x] Config: height, edge, shell, font, font size
- [x] COSMIC theme integration (light/dark)
- [x] Copy/paste and scrollback
- [x] In-app dismiss (`Ctrl+Shift+Q`)

## v2 - Yakuake parity

- [x] Tabs (new/close/rename/reorder)
- [x] Save sessions across shutdown
- [x] Hide on focus loss
- [x] Drag-to-resize with remembered height
- [x] Width percentage and horizontal alignment
- [x] Slide animation
- [x] Settings page UI
- [x] Built-in color scheme picker


## V3 
- [ ] increase sidebar width size, draggable
- [ ] Settings sidebar as tabs - General, Keybindings, Advance
- [ ] Per-tab profiles (e.g. SSH)
- [ ] Context menu in tab
- [x] Panel applet to toggle and show status

- [x] Rename tab - shortcut key and context menu (`Ctrl+Shift+R` and the "Rename Tab" row, both starting the inline rename on the active tab; below two tabs it is a swallowed no-op)
- [x] Duplicate tab - shortcut key and context menu (`Ctrl+Shift+D` and the "Duplicate Tab" row, the same verb as the remote `duplicate-tab`: a fresh shell in the active tab's cwd right after it, carrying the source's rename pin; the running command and `default_profile` never apply)
- [x] Settings in sidebar instead of separate modal
- [x] CLI actions (`new-tab [--cwd] [--cmd]`, `close-tab`/`rename-tab`/`duplicate-tab` with `[--tab-index]`, `switch-profile-tab --profile NAME [--tab-index]` respawning a live tab under a profile, and strict `select-tab N`, over the single-instance D-Bus seam; indices 1-based; `--is-visible` queries the read-only `Visible` property on the `…CosmicExtDropterm.V1` state interface)

## Later
- [ ] Pin tab
- [ ] test profile and add improvements
- [x] Configurable new-tab placement (`new_tab_after_current`, default after the active tab, Behavior-card toggle; the keybinds (`Ctrl+Shift+T`, `Ctrl+Shift+H`), tab-bar `+`, and remote `new-tab` follow it, `duplicate-tab` stays after-active, session restore keeps saved order)
- [ ] Session restore on login
- [ ] Clean up unnecessary comments
- [ ] Monitor selection (cursor, focused, or fixed output) — blocked: the pinned
      libcosmic rev exposes no focused/fixed output targeting (`IcedOutput` is
      `All | Active | Output(WlOutput)`, no output events or by-name handles)

## Non-goals

- Non-COSMIC compositors
- X11 support
- Flatpak packaging
- Built-in global hotkey registration
- Wrapping an external terminal emulator
