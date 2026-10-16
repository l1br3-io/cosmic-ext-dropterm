# Configuration

DropTerm keeps every setting in RON files under one directory and reloads them live - changes apply without a restart, whether made in the [settings drawer](settings.md) or by editing the file directly.

```
~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/
```

Each of the 43 keys is its own file in that directory. Edit one directly:

```sh
echo '60' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/height_percent
echo 'Left' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/h_align
```

Most keys are editable in the [settings drawer](settings.md); the exceptions write themselves (`settings_drawer_width`) or wait for behavior that does not ship yet ([Parked and reserved keys](#parked-and-reserved-keys)).

## Geometry

| Key | Values | Default | What it does |
|---|---|---|---|
| `height_percent` | 10-100 | `40` | Surface height as a percentage of the output height. Written automatically after a grab-bar drag settles. |
| `width_percent` | 30-100 | `100` | Surface width as a percentage of the output width. `100` is the original full-width surface. |
| `h_align` | `Left`, `Center`, `Right` | `Center` | Horizontal placement of the surface. Only meaningful below `width_percent` 100. |
| `edge` | `Top`, `Bottom` | `Top` | The screen edge the terminal drops from. |

Geometry applies live: a shown surface is reshaped in place, a hidden one picks the new geometry up on its next show.

## Appearance

| Key | Values | Default | What it does |
|---|---|---|---|
| `font_name` | font family name | `Noto Sans Mono` | The terminal's typeface. |
| `font_size` | points | `14` | The terminal's font size. |
| `opacity` | 0-100 | `100` | Background opacity. |
| `color_scheme_dark` | scheme name | `COSMIC Dark` | Scheme used while the system is in dark mode. |
| `color_scheme_light` | scheme name | `COSMIC Light` | Scheme used while the system is in light mode. |
| `theme_preference` | `System`, `Dark`, `Light` | `System` | Follow the OS light/dark setting, or pin one. |

Scheme names come from the built-in registry - the same names the drawer's pickers offer ([Theming](theming.md#color-schemes)). A name the registry does not know falls back to the COSMIC pair and is never rewritten. All six keys apply live.

## Tab bar position

| Key | Values | Default | What it does |
|---|---|---|---|
| `tab_position` | `Auto`, `Top`, `Bottom` | `Auto` | Which side of the surface the tab strip sits on, independent of `edge`. `Auto` keeps it beside the grab bar. |

A change never resizes anything - the strip reorders immediately on a shown surface. Pick it in the Geometry section of the [settings drawer](settings.md).

## Tab titles

| Key | Values | Default | What it does |
|---|---|---|---|
| `tab_title_mode` | see below | `Shell` | How a tab's name is generated. |

| Mode | Tab name |
|---|---|
| `Shell` | The shell's own title (OSC 0/2), verbatim - `Terminal N` until it reports one. |
| `AbbreviatedPath` | The title's trailing path, middle segments shortened: `~/src/l1br3/ced` renders `~/s/o/l/ced`. With no title yet, the cwd itself, abbreviated the same way. |
| `LastSegment` | The cwd's final component - `ced`, or `(root)` at `/`. |
| `Numbered` | Always `Terminal N`; shell titles never land. |
| `Profile` | The profile's name ([profiles](#profiles)). A tab with no profile - or a deleted one - falls back to `Shell`'s behavior. |

A manual rename wins in every mode. Changing the mode re-titles every live tab immediately, renamed ones excepted; while in `Profile` mode, renaming a profile retitles its running tabs at once.

## Status bar

`status_bar_enabled` (default `true`) shows the 28 px band at the surface's free edge, beside the grab bar. The band's segments, click-to-copy, selection, and context menu are covered in their own page: [Status bar](status-bar.md). Toggle it with `Ctrl+Shift+B`, on the Behavior tab, or by writing the key.

| Key | Shows | Default |
|---|---|---|
| `status_bar_show_process` | Foreground process name - filled disc while one runs, hollow ring over the idle shell. | `true` |
| `status_bar_show_path` | Working directory, `$HOME` collapsed to `~`. | `true` |
| `status_bar_show_branch` | Git branch of the cwd (worktrees and submodules resolve). | `true` |
| `status_bar_show_profile` | The tab's profile name. | `true` |
| `status_bar_show_grid` | Cell dimensions (`80×24`) - only after a resize away from the tab's spawn size. | `true` |
| `status_bar_show_tab` | Tab position (`2/5`). | `true` |
| `status_bar_show_zoom` | Zoom delta (`+200%`) while the font is off its base size. | `true` |
| `status_bar_show_clock` | Wall clock (`HH:MM`). | `true` |

All live. A hidden segment is never computed - hiding the branch stops the `.git` reads, hiding the clock stops its tick.

## Bar borders

| Key | Default | What it does |
|---|---|---|
| `tab_bar_border` | `true` | A 1 px hairline on the tab strip's terminal-facing edge (it moves with `tab_position`). |
| `status_bar_border` | `true` | The same hairline on the status band's side away from the grab bar. |

The color follows the active scheme, so a scheme flip recolors both lines. Where the strips adjoin, only one line renders - no doubled seam. Pure display: no bar changes height, and both keys are independent. Toggle them on the Behavior tab.

## Close confirmation

`close_confirmation_enabled` (default `true`) asks before closing a tab whose terminal runs a foreground process - through an in-surface modal, one prompt at a time, including the tab context menu's Close Other Tabs and Close Tabs to the Right sweeps (pinned tabs never close). `false` restores instant, unconfirmed closes.

A shell exiting on its own never confirms. The key is read at request time, so a flip applies to the very next close.

## Shell and directories

| Key | Values | Default | What it does |
|---|---|---|---|
| `shell` | path to a binary | unset | The shell plain new tabs run. Unset falls back to `$SHELL`, then `/bin/sh`. Must be a path (`/usr/bin/fish`), not a bare name. |
| `args` | extra arguments | empty | Arguments for that shell, split on whitespace with no quoting. |
| `new_tab_cwd` | `Inherit`, `Default` | `Inherit` | Where plain new tabs open. `Inherit` follows the default chain (an explicit `--cwd`, else the active tab's directory, else the pty default); `Default` opens in `default_cwd` instead. |
| `default_cwd` | directory | unset | The directory `Default` mode opens in. Not existence-checked until the spawn: a dead path logs a warning and the tab opens in the pty default. |

`shell` and `args` apply to the next spawn only - a running tab is never re-launched. `new_tab_cwd` covers the plain spawns (`Ctrl+Shift+T`, the tab bar `+`, the context menus' plain New Tab, a bare remote `new-tab`, the fresh-startup tab); it never applies to `Ctrl+Shift+H` (home is that bind's contract), duplicates, profile launches, or session restore. An explicit `--cwd` wins in both modes.

```sh
echo 'Default' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/new_tab_cwd
echo '"/srv/work"' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/default_cwd
```

## Behavior toggles

| Key | Default | What it does |
|---|---|---|
| `hide_on_focus_loss` | `true` | Hide the terminal when it loses keyboard focus. The opposite of Yakuake, Guake, and Ghostty's quick terminal - for a hotkey-driven drop-down, focus is the whole interaction model. Write `false` to keep the overlay open. |
| `session_persistence_enabled` | `true` | Save the session on change and restore it on launch ([sessions](sessions.md)). `false` stops saves and skips restore; the saved file is never deleted, so re-enabling resumes over it. |
| `slide_animation_enabled` | `true` | Animate show/hide with the 130 ms slide ([animation](animation.md)). `false` restores instant transitions. |
| `bell_notification_enabled` | `true` | Post a desktop notification when a bell rings while the terminal is hidden ([bell](bell.md)). |
| `autosuggestions_enabled` | `true` | Fish-style autosuggestions: dim ghost text after the cursor completing the most recent matching command from this tab's own history. Right accepts it, Alt+Right takes one word, anything else dismisses it - nothing executes without an accept. `false` is the kill switch. |
| `new_tab_after_current` | `true` | New tabs land directly after the active tab. `false` appends at the end of the list (Guake's ordering); `duplicate-tab` always lands after the active tab. |
| `scrollback_lines` | `10000` | Scrollback capacity per terminal. Applies to the next spawn - capacity is fixed when a terminal is created. |

The toggles apply at their next natural trigger - the session toggle at the next save/restore cycle, the slide toggle at the next transition, the bell toggle at the next bell, placement at the next new tab.

## Layer, focus, and panels

| Key | Values | Default | What it does |
|---|---|---|---|
| `layer` | `Overlay`, `Top` | `Overlay` | `Overlay` sits above fullscreen windows; `Top` sits below them but behaves better with input methods. |
| `keyboard_mode` | `Exclusive`, `OnDemand` | `Exclusive` | How the surface asks for keyboard focus. `Exclusive` always grabs the keyboard while shown; `OnDemand` is gentler but not auto-focused by every compositor. |
| `respect_panel` | `true`/`false` | `false` | `true` places the drop-down inside the compositor's usable area instead of covering panels and docks, staying on its configured layer. Applies on the next show. |

With an autohide panel, the hidden panel still reserves its handle strip, so a respecting drop-down sits a few pixels below the screen edge.

```sh
echo true > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/respect_panel
```

## Settings drawer width

`settings_drawer_width` (default unset) is the settings pane's width in pixels, as last dragged. The drawer's drag handle writes it on release; double-clicking the handle resets it to unset. Stored values are clamped at render into 344 px to half the surface, so a stale or hand-edited value can never outgrow the surface.

## Profiles

Profiles are named presets of what a tab runs, managed on the [settings drawer](settings.md)'s Profiles tab - or hand-edited in the `profiles` file, in this shape:

```ron
// ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/profiles
(
    {"name": "web-prod", "command": "ssh web-prod", "cwd": Some("/srv/web"), "color_scheme": Some("Dracula"), "font_size": Some(11), "scrollback_lines": Some(50000), "env": Some({"DROPTERM_PROFILE": "prod", "HTTP_PROXY": "http://bastion:3128"})},
    {"name": "top", "command": "htop"},
)
```

| Field | Default | What it does | Takes effect |
|---|---|---|---|
| `name` | required | The profile's id - matched exactly, case-sensitively. | - |
| `command` | required | The run line, split on whitespace like `args`. | next spawn |
| `cwd` | unset | Working directory; existence-checked at the spawn, never rewritten. | next spawn |
| `color_scheme` | unset | A built-in scheme name for this profile's tabs - one name, independent of light/dark mode. | live |
| `font_size` | unset | Font size in points for this profile's tabs. | live |
| `scrollback_lines` | unset | Scrollback capacity; capacity is fixed when the terminal is created. | next spawn |
| `env` | unset | Extra environment variables, merged over the inherited environment at spawn. | next spawn |

The appearance fields resolve per field, never per profile: a set field wins, an unset field inherits the global value - and each decides on its own. A profile deleted while its tabs run degrades them to the global picks field by field, never rewritten, never an error. The window background follows the active tab's resolution, so the fallback background moves with a tab switch.

`env` values are verbatim strings - no expansion or interpolation, ever; keys must be non-empty and free of `=`. A profile whose `command` is blank (a hand-edited file) degrades to a plain shell and carries none of the profile's identity.

`default_profile` (default unset) names the profile plain new tabs launch: `Ctrl+Shift+T`, the tab bar `+`, the context menu's plain New Tab, a bare remote `new-tab`, and the fresh-startup tab all run it. A name the list no longer carries quietly falls back to the plain shell. Profiles are also launched six other ways - `new-tab --profile NAME`, the context menus' profile submenus and Switch Profile folders, and the tab bar `+`'s right-click dropdown. A Switch Profile pick respawns *that* tab in place under the chosen profile: the running command and scrollback are lost, while the slot, the activation, and a pinned rename stay.

## Keybindings

Keybindings are customized in the drawer's Keybindings tab or by hand. `custom_key_binds` (default empty - all defaults) maps one action id to one binding; the full action-id table lives in [keybindings](keybindings.md):

```ron
// ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/custom_key_binds
{
    "copy": (modifiers: [Ctrl, Alt], key: "C"),
    "activate_tab_3": (modifiers: [], key: "F2"),
}
```

A binding spells modifier names (`Ctrl`, `Shift`, `Alt`, `Super`) plus a key name - single letters case-insensitive, named keys spelled out (`Tab`, `PageUp`, `F1`, `Space`). Rules:

- A custom binding replaces *all* of that action's default chords; removing the entry reverts to the defaults.
- A chord two actions claim resolves to the later id in the file.
- Entries with unknown ids or an empty key are skipped with a warning - never fatal.
- The map is watched live: the very next key press after a write uses the new bindings.

## Parked and reserved keys

Two keys load and persist today but change no behavior:

- `settings_drawer_click_close` (default `true`) is **parked**: the planned click-to-dismiss behavior cannot ship until a capture mechanism exists. A hand-edited value is inert.
- `output` (default unset) is **reserved** for monitor selection - naming the Wayland output the drop-down shows on. Nothing reads it yet; see the [roadmap](https://github.com/l1br3-io/cosmic-ext-dropterm/blob/main/ROADMAP.md).
