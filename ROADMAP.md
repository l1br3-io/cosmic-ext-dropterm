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


## v3 - Profiles and desktop integration
- [x] increase sidebar width size, draggable
- [x] Settings sidebar as tabs - General, Keybindings, Advance
- [x] close confirmation setting flag when tab has process in progress - on by default
- [x] default shell should be editable similar to new profiles - like which default directory, "shell" wether zsh, bash, fish (the Terminal card's shell dropdown over `/etc/shells` — basename labels, full-path writes, the free-text row kept for custom paths — plus the `new_tab_cwd` Behavior dropdown and the `default_cwd` Terminal row: `Inherit` is the byte-identical default, `Default` opens plain spawns in a live-checked directory that degrades to the pty default when dead; a `default_profile` still owns plain spawns in every case)
- [x] Per-tab profiles (e.g. SSH) (a profile carries its command, `cwd`, an optional built-in color scheme, and an optional environment-variable map; tabs record the profile id they run under, so colors resolve per tab, env applies at spawn, the `Profile` title mode labels tabs by name, restore re-resolves by id, and the Switch Profile folder respawns the active tab in place - the `switch-profile-tab` verb)
- [x] Context menu in tab (right-click a tab button: a menu scoped to the *clicked* tab — New Tab; Rename, Duplicate, Switch Profile ▸ acting on that tab through the existing funnels; Move Left/Right muted at the edges; Close, Close Other Tabs, Close Tabs to the Right all through the confirmation gate, one prompt at a time for running processes)
- [x] Panel applet to toggle and show status

- [x] Rename tab - shortcut key and context menu (`Ctrl+Shift+R` and the "Rename Tab" row, both starting the inline rename on the active tab; below two tabs it is a swallowed no-op)
- [x] Duplicate tab - shortcut key and context menu (`Ctrl+Shift+D` and the "Duplicate Tab" row, the same verb as the remote `duplicate-tab`: a fresh shell in the active tab's cwd right after it, carrying the source's rename pin; the running command and `default_profile` never apply)
- [x] Settings in sidebar instead of separate modal
- [x] CLI actions (`new-tab [--cwd] [--cmd]`, `close-tab`/`rename-tab`/`duplicate-tab` with `[--tab-index]`, `switch-profile-tab --profile NAME [--tab-index]` respawning a live tab under a profile, and strict `select-tab N`, over the single-instance D-Bus seam; indices 1-based; `--is-visible` queries the read-only `Visible` property on the `…CosmicExtDropterm.V1` state interface)

## V4

- [x] improve tab bar ux
- [ ] improve docs
- [ ] hide app to tray
- [ ] improve autocomplete feature

# bugs

- editable default profile
- app is not hidden from alt tab
