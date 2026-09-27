# DropTerm Roadmap

Tracks the feature list from the project requirements against what this
repository actually implements.

## v1 - MVP

- [x] Toggle via second launch of the binary
- [x] Layer-shell window anchored to top edge (or bottom, configurable), full width, above other windows
- [x] Hidden from Alt-Tab and dock
- [x] Exclusive keyboard focus when shown; focus returned when hidden
- [x] Shell session persists while hidden
- [x] Config: height, edge, shell/command, font, font size
- [x] COSMIC theme integration (light/dark; the two built-in COSMIC color schemes)
- [x] Copy/paste and scrollback

## v2 - Yakuake parity

- [ ] Tabs (new/close/rename/reorder, keyboard shortcuts)
- [ ] Rename tabs
- [ ] Save sessions even after shutdown of the machine
- [x] Optional hide on focus loss (`hide_on_focus_loss` config key)
- [ ] Monitor selection: cursor, focused, or fixed output (the `output` config key exists but isn't wired up to a specific output yet - every show currently measures and uses whatever the compositor calls the "active" output)
- [ ] Width percentage and horizontal alignment (currently always full width)
- [ ] Drag-to-resize with remembered height
- [ ] Optional slide animation
- [ ] Settings page UI (libcosmic)

## Later

- [ ] COSMIC panel applet to toggle and show status
- [ ] CLI actions: `--new-tab`, `--cmd`, `--cwd`
- [ ] Session restore on login
- [ ] Per-tab profiles (e.g. SSH)

## Non-goals

- Supporting non-COSMIC compositors as a priority
- X11 support
- Flatpak packaging (until COSMIC exposes required protocols to sandboxes)
- Built-in global hotkey registration (no GlobalShortcuts portal on COSMIC)
- Wrapping an external terminal emulator (dropped from the original plan: the embedded engine, built on `alacritty_terminal` plus a COSMIC-native renderer adapted from cosmic-term, covers the same drop-down use case without the window-management workarounds an external-terminal wrapper would need on COSMIC)
