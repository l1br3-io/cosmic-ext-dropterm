# Settings drawer

Every DropTerm setting is editable in this in-app drawer, and every edit applies the moment you commit it - there is no Apply button.

## Opening the drawer

Every setting is editable in-app: press `Ctrl+Shift+I` while the terminal has keyboard focus, pick "Settings…" in the terminal's context menu, or click the gear button at the right corner of the tab strip - all three hit the same toggle. The drawer is an overlay pinned to the right edge of the drop-down surface itself (the libcosmic context drawer cosmic-term uses), so the terminal stays visible - and live-editable - the whole time: opening settings never hides or resizes the surface, and the pty grid is untouched by drawer open/close. The strip is always present whatever the tab count - with a single tab it carries the settings button alone - so settings are one click away in every state; hovering the button shows a `Settings…` tooltip. The keypress is swallowed from the shell like every other app bind. A second press toggles the drawer closed; there is never a second window.

## Tabs, sections, and commit points

The drawer's pages sit behind a three-tab strip - General, Keybindings, and Advanced - pinned under the drawer title, so the tabs stay put while a page scrolls beneath them. General carries the everyday sections: Appearance (font, size, opacity, the two color scheme dropdowns, and the Application theme dropdown), Geometry, Behavior (hide on focus loss, session persistence, slide animation, bell notifications, new-tab placement, the new-tab-directory dropdown), and Terminal (shell, arguments, default directory, scrollback), plus the Profiles entry row. Keybindings carries the shortcut editor (its own section below). Advanced carries the power controls: `respect_panel`, `layer`, and `keyboard_mode`. Every exposed control appears exactly once across the tabs - none lost, none duplicated.

Switching tabs is a page turn only: the drawer never closes, nothing writes, and an in-flight draft survives on its tab (start typing in a field, flip tabs, flip back - the text is still there). The drawer title stays "Settings" on both tabs; the strip disambiguates. Close, Escape, and a repeat `Ctrl+Shift+I` close the whole drawer from any tab, and an explicit close resets the drawer to General for the next open, while a focus-loss hide preserves the open tab and its drafts exactly as they were.

Every edit applies the moment you commit it, with no Apply button. Commit points per control kind: toggles and dropdowns write on change, sliders write on release (the value previews live while dragging), text inputs write on Enter.

## Keybindings tab

The third tab is the shortcut editor: a search box on top, then every bindable action grouped (Clipboard, Tabs, Zoom, Other), each row showing its effective binding - the custom binding if one is set, otherwise every default chord joined (`Ctrl + Plus / Ctrl + Equal` for zoom-in), or `None` for actions that ship unbound (copy-primary - middle-click paste covers it). One binding per action: setting a new chord replaces the action's previous bindings entirely, and the row's undo affordance (visible only while a custom exists) reverts to the defaults. The numbered tab-activation rows are one row per digit, 1 through 8, with 9 as its own "Activate last tab" row; the per-profile new-tab action is deliberately not keybindable.

Rebinding is a two-press interaction: press the row's binding to arm the capture (a hint row appears under it), then press the chord. While armed, the capture owns every key press - nothing reaches the terminal or any other binding. Escape (or the hint's Cancel button) ends the capture without changes; a later Escape closes the drawer as usual. Capturing a chord another action already holds shows an inline error naming the holder and writes nothing; capturing the action's own current chord just ends the capture.

A committed rebind applies instantly - the very next key press uses the new chord, no restart - and persists to the config file, so it survives a restart. Editing the config file by hand while the app runs has the same effect (the file is watched); entries with unknown action ids or unparseable keys are skipped with a warning, never fatal. The on-disk shape is one `custom_key_binds` entry per action, keyed by the action's id and spelling the binding as modifier names plus a key name (see [configuration](configuration.md#keybindings)).

## Profiles page

The drawer has a drill-down page: the Profiles entry row on the General tab (it carries the profile count) flips the drawer to it, a back affordance returns to General, and the drawer title follows the open page - "Profiles" on the drill-down, "Settings" on the tabs. The tab strip stays pinned in the drawer header on the Profiles page too, highlighting General; picking a tab from there turns straight to that tab. Close, Escape, and a repeat `Ctrl+Shift+I` close the whole drawer from any page.

The page holds the profile list, the edit form, and the default-profile picker. The list shows one row per profile in config order - the exact order the tab bar `+`'s dropdown and the context menu's submenu use - each row with the profile's name and a command preview. The selected row's three fields (`name`, `command`, `cwd`) edit in the form below it, joined by a Color scheme dropdown - "None — follow global setting" first, then every built-in scheme - whose pick writes the moment you make it and recolors the profile's running tabs without a respawn, and by an Environment variables editor: one key/value input pair per entry, seeded from the stored map in sorted key order, with an Add variable affordance and a per-row remove. Env rows commit like every other field - an Enter in either input writes the whole map - and the same Enter rule gates them: a wholly blank row drops at the write (an abandoned row holds nothing), while a value without a key or a key carrying `=` rejects with an inline error under that row and writes nothing; add/remove are buffered list edits that land with the next env Enter. The written map applies to the profile's next spawn - a running tab's environment never changes. Add and Clone open a draft form instead, Clone seeding a copy under a unique `name (copy)` name, with the dropdown's pick and the env rows buffered into the draft's first valid commit. Every write follows the drawer's instant rule: each field commits on Enter (an invalid Enter shows an inline error and writes nothing), while picker picks, row deletes, and up/down reorders write the moment you make them. Deleting the profile the default names clears the default in the same interaction, and renaming that profile moves the default along, so the picker never shows a name the list lacks.

A profile draft is page-local: a focus-loss hide preserves it with the drawer (see above), while leaving the page - back or close - clears it, so a reopened page starts from the config.

## While the drawer is open

While the drawer is open, terminal input is inert - keyboard and pointer never reach the shell; the drawer's own inputs type normally. Escape closes the drawer (only the drawer: the surface stays), as does the drawer's Close button and a repeat `Ctrl+Shift+I`. Focus loss keeps working as configured: with `hide_on_focus_loss` the surface hides with the drawer preserved exactly as it was - open, drafts and all - and the next show uncovers it; explicitly closing the drawer clears any in-flight drafts, so a reopen starts fresh from the config file.

## When each edit takes effect

Each control states next to it when its edit takes effect, and because the terminal is visible under the drawer, the live keys are visible the moment they land:

| Field | Applies |
|---|---|
| `height_percent`, `width_percent`, `h_align`, `edge` | live - reshapes a shown surface in place (deferred mid-drag and mid-slide) |
| `tab_position` | live - pure relayout, no reshape: the strip reorders immediately and nothing resizes |
| `tab_title_mode` | live - every un-renamed tab re-titles from its stored shell title, cwd, and (in Profile name mode) profile name the moment the pick lands |
| `font_name`, `font_size`, `opacity`, `color_scheme_dark`, `color_scheme_light`, `theme_preference` | live, on every tab, visible in the terminal beneath the open drawer - drag the opacity slider and watch the surface change while dragging |
| `layer`, `keyboard_mode` | next show - a shown surface is never recreated under itself |
| `custom_key_binds` | instant - the very next key press uses the new chord, in the same session |
| `shell`, `args`, `scrollback_lines`, `new_tab_cwd`, `default_cwd` | new tabs only |
| `profiles`, `default_profile` | next spawn - a page write applies to future spawns only; a running tab is never re-launched |
| `hide_on_focus_loss`, `session_persistence_enabled`, `slide_animation_enabled`, `bell_notification_enabled`, `new_tab_after_current` | next trigger - the session toggle at the next save/restore cycle, the slide toggle at the next transition (a running animation is never aborted), the bell toggle at the next bell, the placement toggle at the next new-tab open |

These Behavior toggles default to `true`, which is exactly the pre-settings behavior:

- `session_persistence_enabled` is symmetric: `false` stops session saves *and* skips launch restore. The existing `session.ron` is never deleted or rewritten by disabling - re-enabling resumes saving over it.
- `slide_animation_enabled`: `false` restores instant show/hide with immediate destroy (the pre-slide-animation behavior).
- `new_tab_after_current`: `false` appends new tabs at the end of the tab list (Guake's default ordering) instead of directly after the active tab. `Ctrl+Shift+T`, `Ctrl+Shift+H` (the home-directory new tab), the tab bar's `+`, and the remote `new-tab` follow it; `duplicate-tab` always lands after the active tab, and session restore keeps the saved order.

## Input ranges and limitations

Controls constrain input to the same ranges as the config (`height_percent` 10-100, `width_percent` 30-100, `opacity` 0-100, `scrollback_lines` non-negative); the write path never persists an out-of-range value. One known limitation: the `args` input splits on whitespace with no quoting support - arguments containing spaces stay config-file-only.
