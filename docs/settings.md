# Settings window

Every DropTerm setting is editable in the Settings window, and every edit applies the moment you commit it - there is no Apply button. The one exception is the Profiles page's edit form, a transactional draft with its own Save and Cancel.

While the window is open the drop-down terminal hides - the two are an exclusive pair - so the terminal is not visible while a setting is changed. Edits still apply to the running tabs immediately; the result is seen on return. Closing the window (any route below) brings the drop-down back. The pair has an opt-out: [`settings_coexist`](configuration.md) severs it, so the window and the drop-down stand together - open Settings without hiding the drop-down, summon the drop-down without closing the window, and close the window without the drop-down returning. To watch an edit live (the opacity slider with the terminal on screen), pair it with `hide_on_focus_loss: false`, since focusing the window otherwise retracts the drop-down on its own rule.

## Opening Settings

Four ways in:

- press `Ctrl+Shift+I` while the terminal has keyboard focus,
- pick "Settings…" in the terminal's context menu,
- click the gear button at the right end of the tab strip,
- pick a `Settings: …` row in the command palette (ten, one per page, [command palette](command-palette.md)).

Every trigger opens exactly one window at the requested page (Appearance when none is named) and hides the drop-down in both `hide_on_focus_loss` modes - unless `settings_coexist` is on, which skips the hide. A trigger while the window is open switches the page and asks for focus instead of opening a second window. Summoning the terminal while the window is open - the summon key, the tray toggle, any forwarded verb - closes the window first and queues no return: the show that closed it is the show. With `settings_coexist` on the summon closes nothing: both windows stand together.

## The sidebar

The window's left column lists ten pages under two headers, the open page marked:

| Header | Pages |
|---|---|
| **Terminal** | Appearance (the font picker - every installed monospace family, each row previewed in its own face; a name no installed family matches stays as a leading row and re-selects, so a hand-edited `font_name` is never lost - plus size, opacity, the dark and light scheme pickers - each previewing its scheme's actual eighteen colors, [themeing](theming.md#color-schemes) - and the Application theme dropdown), Window (height, width, alignment, edge), Tabs (tab bar position, tab titles, the minimum tab count, new-tab placement and directory, close confirmation, session persistence), Status bar (the master toggle plus one row per segment, [configuration](configuration.md#status-bar)), Dropdown (hide on focus loss, keep Settings open with the terminal, slide animation), Terminal (shell, arguments, default directory, scrollback, bell notifications, inline images, shell integration), Auto complete (the prompt-line surfaces - input mode and position, the ghost, the menu and its pop, the per-surface chrome toggles, and the provider knobs, [configuration](configuration.md#auto-complete)), Advanced (`respect_panel`, `layer`, `keyboard_mode`). |
| **Profiles & keys** | Profiles (the profile manager - its own section below), Keybindings (the shortcut editor - its own section below). |

Every exposed control appears exactly once across the ten pages - none lost, none duplicated. A section page renders its title over one bordered group of its rows; nothing folds away.

Clicking a page swaps the content pane and writes nothing - navigation, never a write path. An in-flight draft elsewhere survives the turn (start typing in a field, flip pages, flip back - the text is still there); the one exception is leaving Profiles, which clears its draft and any armed delete, so a reopened Profiles page starts from the config.

## Keybindings page

At the top, above the in-app groups, the **Global** group holds the summon key's row - "Toggle the overlay" - with the chords that currently summon DropTerm read-only (`None` while unbound), the setup walk for your desktop beneath, on COSMIC an "Open keyboard settings" button that opens the page the walk describes, and a "Set up the summon key…" button that opens the onboarding (below) over the standing Settings window. While DropTerm set a key itself, the row also carries **Remove the key DropTerm set**: it deletes exactly the entry DropTerm wrote and touches nothing you bound by hand. The global key is a different animal from every row below it: it must fire while the terminal is hidden, so it lives in the desktop's own shortcut config rather than in `custom_key_binds` - this row shows what is bound there and how to bind more, it never captures.

On first run with nothing bound, the **onboarding** window opens instead of the old dialog: one step today, the summon key, with two paths. **Set up manually** shows the walk and - on COSMIC - an "Open keyboard settings" button plus **Check again**, which re-reads the shortcut store: a found chord lands on a confirmation ("DropTerm is summoned with <chord>.") and the Global row's display updates to show it; nothing found leaves the step standing with a "No summon key was found yet." line. **Configure for me** is the writer's own seat: on COSMIC it appears wherever the shortcut store can carry a write - a store already holding entries, or a fresh session with none yet - and stays away on other desktops and wherever the store cannot be read, never as a dead control. **Use F12** assigns Yakuake's default in one press; **Choose another key…** arms a capture, and the next chord takes the same path - a bare modifier press keeps the capture armed, Escape cancels. A chord another entry already holds is refused with that entry's name and nothing is written; the system defaults' own chords are refused the same way; a chord you `Disable`d in COSMIC Settings is taken over. A commit writes exactly one entry into COSMIC's custom shortcuts - it appears in COSMIC Settings → Keyboard → Custom Shortcuts listed as "DropTerm" and running your installed `dropterm` - refreshes the Global row's display, and lands on the confirmation with the chord. Nothing writes except on one of the seat's button presses. The footer follows the origin: on first run it carries the "Don't ask again" checkbox and **Skip** - unticked with no key set, the flow re-opens at the next cold start; ticked, it retires for good (persisted as [`onboarding_dismissed`](configuration.md)); a bound key retires it through any path. Opened from the Global group instead, the same step shows only **Close**, and closing persists nothing. A warm launch (a D-Bus forward, a toggle summon) never opens the flow.

The editor is a search box on top, then every bindable action grouped (Clipboard, Tabs, Zoom, Other). Each row shows its effective binding: the custom chord if one is set, otherwise every default chord joined (`Ctrl + + / Ctrl + = / Ctrl + Shift + + / Ctrl + Shift + =` for zoom-in), or `None` for actions that ship unbound. The full default table lives in [keybindings](keybindings.md).

Rebinding is two presses: press the row's binding to arm the capture, then press the chord. While armed, the capture owns every key press inside the window - nothing reaches the terminal. Escape or the hint's Cancel ends it without changes; capturing a chord another action already holds shows an inline error naming the holder and writes nothing.

Rules worth knowing:

- One binding per action: a new chord replaces the action's previous bindings entirely; the row's undo affordance reverts to the defaults.
- A committed rebind applies to the very next key press and persists across restarts.
- Editing the config file by hand while the app runs has the same effect - the file is watched. Entries with unknown ids or unparseable keys are skipped with a warning, never fatal.
- The on-disk shape is documented in [configuration](configuration.md#keybindings).

The numbered activation rows are one per digit, 1 through 8, with 9 as "Activate last tab"; the per-profile new-tab actions are deliberately not keybindable.

## Profiles page

The page holds the profile list, the edit form, and the default-profile picker. The list shows one row per profile in config order - the same order the tab bar `+`'s dropdown and the context menu's submenu use. Each row is a thumbnail of the tab it opens: a scheme swatch chip (the profile's palette miniaturized; a hollow frame when it follows the global look), the name above the command as a dimmed `$` prompt line, and a `default` tag when the picker names this row. The row's actions are tooltip'd icon buttons - edit pencil, clone, delete, move up/down - and clicking the identity column selects it.

**Delete is two-step inline, never a dialog.** A trash press arms that row - the trash becomes a Confirm / Keep pair right on the row. Only one row is ever armed. Confirming deletes immediately and, if the deleted profile was the default, clears the default in the same interaction.

**While a form draft is open, the list is locked** - every row button, the add button, and selection itself render disabled until Save or Cancel. The default-profile picker stays live, and an armed delete simply waits out the draft.

### The edit form

The form is the window's one buffered island: nothing writes until Save - the footer's button or an Enter in any input. Its fields:

| Field | Rules |
|---|---|
| Name, Command | Text, required. The name must be non-blank and not another profile's. |
| Working directory | Text, optional; never existence-checked. |
| Color scheme | The same swatch picker as the Appearance page, with a leading **Inherit** row showing the global scheme resolved for the current mode. |
| Font size, Scrollback | Text; empty means inherit the global value - the placeholder names it (`inherits 14`). |
| Environment variables | One key/value pair per entry, with add and per-row remove. |

Save validates the whole draft at once and writes exactly once when every field passes: the command must parse to a program, the scheme (if set) must be a built-in, the font size must sit in the 6-32 band, the scrollback must be a number (clamping like the global input), and every env key must be non-empty and free of `=`. A failed Save writes nothing and shows every offending field's inline error at once; typing in a field clears only its error. An unchanged Edit draft's Save closes the form without writing; a valid Save closes on the row it wrote.

The scheme recolors the profile's running tabs live; command, cwd, font size, scrollback, and env apply to the profile's next spawn - a running tab is never re-launched. Add and Clone open the same form, Clone seeding a copy under a unique `name (copy)` name.

A draft is page-local: leaving the page - or closing the window - clears it and any armed delete, so a reopened Profiles page starts from the config. Cancel and Escape both drop the draft with no write; the window stays open beneath Escape.

## While the window is open

The drop-down is hidden, so the terminal takes no input; the window's own inputs type normally. Escape peels one layer at a time: an open picker popover (scheme or font) closes first, then an open profile draft cancels, and only with neither does Escape close the window and bring the drop-down back. The title bar's ✕ closes it too, as does the summon chord (`Ctrl+Shift+I`) pressed inside the window. A user close - ✕, Escape, the chord - drops the window's drafts and per-open page state (the reopen-fresh rule) and returns the terminal.

## When each edit takes effect

Each control states next to it when its edit applies. With the terminal hidden while the window is open, "live" means it applies to the running tabs at once - the effect is seen on return:

| Field | Applies |
|---|---|
| `height_percent`, `width_percent`, `h_align`, `edge` | live - reshapes the surface in place, seen on return (deferred mid-drag and mid-slide) |
| `tab_position` | live - pure relayout, no reshape |
| `status_bar_enabled` | live - the band joins or leaves the column and the terminal's pinned height follows the same frame |
| `status_bar_show_*` segments | live - the hidden segment vanishes from the next render and its inputs stop being gathered |
| `tab_bar_border`, `status_bar_border` | live - the bar's hairline appears or vanishes on the next render, nothing resizes |
| `tab_title_mode` | live - every un-renamed tab re-titles immediately |
| `font_name`, `font_size`, `opacity`, `color_scheme_dark`, `color_scheme_light`, `theme_preference` | live, applied to the running tabs at once. `font_size` and the scheme pair apply to every tab whose profile does not override that field |
| `respect_panel` | live - a shown drop-down re-places itself in place (one recreate with the shape standing, the anchor-edit path); a hidden one picks the value up at its next show |
| `layer`, `keyboard_mode` | next show - a shown surface is never recreated for these |
| `custom_key_binds` | instant - the very next key press uses the new chord |
| `shell`, `args`, `scrollback_lines`, `new_tab_cwd`, `default_cwd` | new tabs only |
| `shell_integration` | next spawn - the key reads once per spawn, a flip governing the shells spawned after it (a running shell keeps the integration it was spawned with) |
| `profiles` (color_scheme, font_size) | live - an edit recolors or re-sizes the profile's running tabs without a respawn |
| `profiles` (command, cwd, env, scrollback_lines), `default_profile` | next spawn - a running tab is never re-launched |
| `hide_on_focus_loss` | live - a shown drop-down's keyboard hold re-resolves in place (the stay-open flip downgrades an exclusive grab to shared before the next focus loss); a hidden one resolves at its next show. The hide gate itself reads the flip at the next focus loss |
| `session_persistence_enabled`, `slide_animation_enabled`, `bell_notification_enabled`, `new_tab_after_current` | next trigger - the session toggle at the next save/restore, the slide toggle at the next transition, the bell toggle at the next bell, placement at the next new tab |
| `close_confirmation_enabled` | next close - the key is read at request time |
| `input_ghost` | live - a showing ghost disappears on the echo; re-enabling suggests again at the next typed prefix (the menu untouched) |
| `input_menu` | live - a showing menu closes the moment the switch lands; re-enabling re-arms Tab and the auto-pop at the next typed prefix |
| `input_auto_popup` | live - the next typed prefix pops (or declines) the menu on its own; off leaves Tab the only opener |
| `input_mode`, `input_descriptions`, `input_badges`, `input_icons`, `input_argument_hint`, `input_compact_rows` | live - the next frame repaints the menu and its chrome in place (a mode flip stands the overlays down to the bare shell, or brings them back over the prompt row) |
| `autosuggestions_paths_enabled`, `autosuggestions_commands_enabled`, `autosuggestions_history_context` | live - the next typed prefix answers under the new providers |
| `autosuggestions_delay_ms` | live - the delay resolves at every update: a lowered value can reveal a waiting suggestion, a raised one re-arms the wait |
| `autosuggestions_completions_extended` | live - the table reloads on the echo: off answers the embedded floor alone (the installed file's commands stop completing), on adds the tier beside it again (a re-toggle reuses the already-parsed file; a system data file does not change under a running app) |
| `images_enabled` | live - off makes the next OSC 1337 sequence inert (nothing decodes or places, no error; already-placed images keep rendering), on renders from the next sequence ([images](images.md)) |

These toggles (the Tabs and Dropdown pages) default to `true`, which is exactly the pre-settings behavior:

- `session_persistence_enabled` is symmetric: `false` stops session saves *and* skips launch restore. The existing saved session is never deleted or rewritten by disabling.
- `slide_animation_enabled`: `false` restores instant show/hide.
- `new_tab_after_current`: `false` appends new tabs at the end of the list (Guake's default ordering) instead of directly after the active tab. `Ctrl+Shift+T`, `Ctrl+Shift+H`, the tab bar's `+`, and the remote `new-tab` follow it; duplicates always land after the active tab, and session restore keeps the saved order.
- `close_confirmation_enabled`: `false` restores instant, unconfirmed closes ([configuration](configuration.md#close-confirmation)).

## Input ranges and limitations

Controls constrain input to the same ranges as the config (`height_percent` 10-100, `width_percent` 30-100, `opacity` 0-100, `scrollback_lines` non-negative, `autosuggestions_delay_ms` a plain 0-65535 milliseconds - an out-of-band entry rejects rather than clamps); the write path never persists an out-of-range value. One known limitation: the `args` input splits on whitespace with no quoting support - arguments containing spaces stay config-file-only.
