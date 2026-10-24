# Settings drawer

Every DropTerm setting is editable in this in-app drawer, and every edit applies the moment you commit it - there is no Apply button. The one exception is the Profiles page's edit form, a transactional draft with its own Save and Cancel.

## Opening the drawer

Three ways in, all the same toggle:

- press `Ctrl+Shift+I` while the terminal has keyboard focus,
- pick "Settings…" in the terminal's context menu,
- click the gear button at the right end of the tab strip.

The drawer is an overlay pinned to the right edge of the drop-down surface, so the terminal stays visible - and live-editable - the whole time: opening settings never hides or resizes the surface. The tab strip (with its gear) is always present, even with a single tab, so settings are one click away in every state. A second press of `Ctrl+Shift+I` toggles the drawer closed; there is never a second window.

## The five tabs

| Tab | Carries |
|---|---|
| **Appearance** | How the terminal looks: the Appearance card (the font picker - every installed monospace family, each row previewed in its own face; a name no installed family matches stays as a leading row and re-selects, so a hand-edited `font_name` is never lost - plus size, opacity, the dark and light scheme pickers - each previewing its scheme's actual eighteen colors, [themeing](theming.md#color-schemes) - and the Application theme dropdown) and the Window card (height, width, alignment, edge). |
| **Behavior** | How DropTerm acts, one card per concern: Tabs (tab bar position, tab titles, new-tab placement and directory, close confirmation, session persistence), Status bar (the master toggle plus one row per segment, [configuration](configuration.md#status-bar)), Dropdown (hide on focus loss, slide animation), and Terminal (shell, arguments, default directory, scrollback, bell notifications, autosuggestions). |
| **Profiles** | The profile manager - its own section below. |
| **Keybindings** | The shortcut editor - its own section below. |
| **Advanced** | The power controls: `respect_panel`, `layer`, and `keyboard_mode`. |

Every exposed control appears exactly once across the tabs - none lost, none duplicated.

Switching tabs is a page turn only: the drawer never closes, nothing writes, and an in-flight draft survives (start typing in a field, flip tabs, flip back - the text is still there). Close, Escape, and a repeat `Ctrl+Shift+I` close the whole drawer from any tab; an explicit close resets it to Appearance, while a focus-loss hide preserves the open tab and its drafts.

Commits are per control kind: toggles, dropdowns, and the pickers write on change, sliders write on release (the value previews live while dragging), text inputs write on Enter. The profile form is the one buffered island - its writes collect behind an explicit Save.

## Keybindings tab

The editor is a search box on top, then every bindable action grouped (Clipboard, Tabs, Zoom, Other). Each row shows its effective binding: the custom chord if one is set, otherwise every default chord joined (`Ctrl + + / Ctrl + = / Ctrl + Shift + + / Ctrl + Shift + =` for zoom-in), or `None` for actions that ship unbound. The full default table lives in [keybindings](keybindings.md).

Rebinding is two presses: press the row's binding to arm the capture, then press the chord. While armed, the capture owns every key press - nothing reaches the terminal. Escape or the hint's Cancel ends it without changes; capturing a chord another action already holds shows an inline error naming the holder and writes nothing.

Rules worth knowing:

- One binding per action: a new chord replaces the action's previous bindings entirely; the row's undo affordance reverts to the defaults.
- A committed rebind applies to the very next key press and persists across restarts.
- Editing the config file by hand while the app runs has the same effect - the file is watched. Entries with unknown ids or unparseable keys are skipped with a warning, never fatal.
- The on-disk shape is documented in [configuration](configuration.md#keybindings).

The numbered activation rows are one per digit, 1 through 8, with 9 as "Activate last tab"; the per-profile new-tab actions are deliberately not keybindable.

## Profiles tab

The page holds the profile list, the edit form, and the default-profile picker. The list shows one row per profile in config order - the same order the tab bar `+`'s dropdown and the context menu's submenu use. Each row is a thumbnail of the tab it opens: a scheme swatch chip (the profile's palette miniaturized; a hollow frame when it follows the global look), the name above the command as a dimmed `$` prompt line, and a `default` tag when the picker names this row. The row's actions are tooltip'd icon buttons - edit pencil, clone, delete, move up/down - and clicking the identity column selects it.

**Delete is two-step inline, never a dialog.** A trash press arms that row - the trash becomes a Confirm / Keep pair right on the row. Only one row is ever armed. Confirming deletes immediately and, if the deleted profile was the default, clears the default in the same interaction.

**While a form draft is open, the list is locked** - every row button, the add button, and selection itself render disabled until Save or Cancel. The default-profile picker stays live, and an armed delete simply waits out the draft.

### The edit form

The form is the drawer's one buffered island: nothing writes until Save - the footer's button or an Enter in any input. Its fields:

| Field | Rules |
|---|---|
| Name, Command | Text, required. The name must be non-blank and not another profile's. |
| Working directory | Text, optional; never existence-checked. |
| Color scheme | The same swatch picker as the Appearance tab, with a leading **Inherit** row showing the global scheme resolved for the current mode. |
| Font size, Scrollback | Text; empty means inherit the global value - the placeholder names it (`inherits 14`). |
| Environment variables | One key/value pair per entry, with add and per-row remove. |

Save validates the whole draft at once and writes exactly once when every field passes: the command must parse to a program, the scheme (if set) must be a built-in, the font size must sit in the 6-32 band, the scrollback must be a number (clamping like the global input), and every env key must be non-empty and free of `=`. A failed Save writes nothing and shows every offending field's inline error at once; typing in a field clears only its error. An unchanged Edit draft's Save closes the form without writing; a valid Save closes on the row it wrote.

The scheme recolors the profile's running tabs live; command, cwd, font size, scrollback, and env apply to the profile's next spawn - a running tab is never re-launched. Add and Clone open the same form, Clone seeding a copy under a unique `name (copy)` name.

A draft is page-local: a focus-loss hide preserves it with the drawer, while leaving the tab - or closing the drawer - clears it and any armed delete, so a reopened Profiles page starts from the config. Cancel and Escape both drop the draft with no write; the drawer stays open beneath Escape.

## While the drawer is open

Terminal input is inert - keyboard and pointer never reach the shell; the drawer's own inputs type normally. Escape peels one layer at a time: an open picker popover (scheme or font) closes first, then an open profile draft cancels, and only with neither does Escape close the drawer (the surface stays). The drawer's Close button and a repeat `Ctrl+Shift+I` close it too. Focus loss keeps working as configured: the surface hides with the drawer preserved exactly as it was, drafts and all.

## When each edit takes effect

Each control states next to it when its edit applies, and the terminal is visible under the drawer, so the live keys show their effect the moment they land:

| Field | Applies |
|---|---|
| `height_percent`, `width_percent`, `h_align`, `edge` | live - reshapes a shown surface in place (deferred mid-drag and mid-slide) |
| `tab_position` | live - pure relayout, no reshape |
| `status_bar_enabled` | live - the band joins or leaves the column and the terminal's pinned height follows the same frame |
| `status_bar_show_*` segments | live - the hidden segment vanishes from the next render and its inputs stop being gathered |
| `tab_bar_border`, `status_bar_border` | live - the bar's hairline appears or vanishes on the next render, nothing resizes |
| `tab_title_mode` | live - every un-renamed tab re-titles immediately |
| `font_name`, `font_size`, `opacity`, `color_scheme_dark`, `color_scheme_light`, `theme_preference` | live, visible beneath the open drawer. `font_size` and the scheme pair apply to every tab whose profile does not override that field |
| `layer`, `keyboard_mode` | next show - a shown surface is never recreated under itself |
| `custom_key_binds` | instant - the very next key press uses the new chord |
| `settings_drawer_width` | live by drag - written on handle release; double-clicking the handle resets it |
| `shell`, `args`, `scrollback_lines`, `new_tab_cwd`, `default_cwd` | new tabs only |
| `profiles` (color_scheme, font_size) | live - an edit recolors or re-sizes the profile's running tabs without a respawn |
| `profiles` (command, cwd, env, scrollback_lines), `default_profile` | next spawn - a running tab is never re-launched |
| `hide_on_focus_loss`, `session_persistence_enabled`, `slide_animation_enabled`, `bell_notification_enabled`, `new_tab_after_current` | next trigger - the session toggle at the next save/restore, the slide toggle at the next transition, the bell toggle at the next bell, placement at the next new tab |
| `close_confirmation_enabled` | next close - the key is read at request time |
| `autosuggestions_enabled` | live - a showing suggestion disappears on the echo; re-enabling suggests again at the next typed prefix |
| `autosuggestions_delay_ms` | live - the delay resolves at every update: a lowered value can reveal a waiting suggestion, a raised one re-arms the wait |

These toggles (the Tabs and Dropdown cards) default to `true`, which is exactly the pre-settings behavior:

- `session_persistence_enabled` is symmetric: `false` stops session saves *and* skips launch restore. The existing saved session is never deleted or rewritten by disabling.
- `slide_animation_enabled`: `false` restores instant show/hide.
- `new_tab_after_current`: `false` appends new tabs at the end of the list (Guake's default ordering) instead of directly after the active tab. `Ctrl+Shift+T`, `Ctrl+Shift+H`, the tab bar's `+`, and the remote `new-tab` follow it; duplicates always land after the active tab, and session restore keeps the saved order.
- `close_confirmation_enabled`: `false` restores instant, unconfirmed closes ([configuration](configuration.md#close-confirmation)).

## Input ranges and limitations

Controls constrain input to the same ranges as the config (`height_percent` 10-100, `width_percent` 30-100, `opacity` 0-100, `scrollback_lines` non-negative, `autosuggestions_delay_ms` a plain 0-65535 milliseconds - an out-of-band entry rejects rather than clamps); the write path never persists an out-of-range value. One known limitation: the `args` input splits on whitespace with no quoting support - arguments containing spaces stay config-file-only.
