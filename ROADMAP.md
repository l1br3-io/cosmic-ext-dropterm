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
- [x] Slide animation
- [x] Settings page UI

## Later

- [ ] Panel applet to toggle and show status
- [ ] CLI actions (`--new-tab`, `--cmd`, `--cwd`)
- [ ] Session restore on login
- [ ] Per-tab profiles (e.g. SSH)
- [ ] Width percentage and horizontal alignment
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
