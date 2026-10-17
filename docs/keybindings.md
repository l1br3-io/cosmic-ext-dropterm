# Keybindings

Every app-level shortcut and its default chords, in the same groups the settings drawer's editor uses; each row names the action's config id (the `custom_key_binds` vocabulary).

## The defaults

| Action | Default chords | Action id |
|---|---|---|
| Select all | `Ctrl+Shift+A` | `select_all` |
| Copy | `Ctrl+Shift+C` | `copy` |
| Copy (primary selection) | *None* - ships unbound | `copy_primary` |
| Paste | `Ctrl+Shift+V` | `paste` |
| Paste primary selection | `Shift+Insert` | `paste_primary` |
| New tab | `Ctrl+Shift+T` | `new_tab` |
| New tab (home directory) | `Ctrl+Shift+H` | `new_tab_home` |
| Duplicate tab | `Ctrl+Shift+D` | `duplicate_tab` |
| Close tab | `Ctrl+Shift+W` | `close_tab` |
| Rename tab | `Ctrl+Shift+R` | `rename_tab` |
| Next tab | `Ctrl+PageDown` / `Ctrl+Tab` | `next_tab` |
| Previous tab | `Ctrl+PageUp` / `Ctrl+Shift+Tab` | `prev_tab` |
| Move tab left | `Ctrl+Shift+PageUp` | `move_tab_left` |
| Move tab right | `Ctrl+Shift+PageDown` | `move_tab_right` |
| Activate tab 1-8 | `Ctrl+Shift+1` … `Ctrl+Shift+8` | `activate_tab_1` … `activate_tab_8` |
| Activate last tab | `Ctrl+Shift+9` | `activate_last_tab` |
| Zoom in | `Ctrl+=` / `Ctrl++` / `Ctrl+Shift+=` / `Ctrl+Shift++` | `zoom_in` |
| Zoom out | `Ctrl+-` / `Ctrl+Shift+-` | `zoom_out` |
| Zoom reset | `Ctrl+0` | `zoom_reset` |
| Clear scrollback | `Ctrl+Shift+K` | `clear_scrollback` |
| Hide the overlay | `Ctrl+Shift+Q` | `hide` |
| Open settings | `Ctrl+Shift+I` | `settings` |
| Toggle the status bar | `Ctrl+Shift+B` | `toggle_status_bar` |

The numbered activation binds are cosmic-term's exact rule: digits 1-8 activate that tab strictly, and 9 always takes the last tab. The cycling pair is browser-style - `Ctrl+Tab` / `Ctrl+Shift+Tab` walk the tab list alongside `Ctrl+PageUp`/`PageDown`. `copy_primary` ships unbound because middle-click already pastes the primary selection; it is still bindable like every other action.

The per-profile verbs - New profile tab and Switch profile tab - are deliberately not keybindable: they act on a config-list index, which is not a stable user-facing id, so they live only in the context menus.

## Why some bare keys stay free

Several shortcuts sit behind `Ctrl+Shift+` because the bare `Ctrl+` chord is a byte the shell owns: `Ctrl+I` is the TAB byte (0x09, tab completion), `Ctrl+H` the backspace byte (0x08), `Ctrl+B` readline's cursor-back, `Ctrl+R` reverse history search, and `Ctrl+D` end-of-file. Swallowing any of them would break the shell it runs in, so the app never binds them. Likewise bare `Ctrl+digit` stays free - only the `Ctrl+=`/`-`/`0` zoom cluster and its Shift-held forms bind.

The zoom chords come in Shift-optional pairs because matching is exact on modifiers and typing `+` holds Shift on US-style layouts: without the Shift-held forms the literal `Ctrl++` press would arrive dead. `Ctrl+Shift+_` is the one form deliberately left unbound - on those layouts it is how a Shift-held `Ctrl+-` arrives, and that byte belongs to the shell (0x1F, readline undo).

## Customizing

One binding per action, set in the [settings drawer](settings.md#keybindings-tab)'s editor (two-press capture: arm, then press the chord - a chord another action holds is refused with the holder named) or hand-edited in the config file ([configuration](configuration.md#keybindings)). A custom binding replaces *all* of that action's default chords; removing the entry reverts to the defaults. A committed rebind applies to the very next key press, no restart.
