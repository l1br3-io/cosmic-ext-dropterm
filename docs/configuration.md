# Configuration

Every DropTerm setting is one RON file in one config directory, reloaded live - no restart needed.

## Location and live reload

DropTerm uses `cosmic-config` and reloads its config live - no restart needed. The file lives at:

```
~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/
```

Each setting is its own RON file inside that directory (`height_percent`, `width_percent`, `h_align`, `edge`, `tab_position`, `layer`, `keyboard_mode`, `respect_panel`, `shell`, `args`, `font_name`, `font_size`, `color_scheme_dark`, `color_scheme_light`, `theme_preference`, `opacity`, `scrollback_lines`, `hide_on_focus_loss`, `session_persistence_enabled`, `slide_animation_enabled`, `bell_notification_enabled`, `tab_title_mode`, `new_tab_after_current`, `profiles`, `default_profile`, `new_tab_cwd`, `default_cwd`). Every one of them is editable in the [settings drawer](settings.md); the files can also be edited directly, e.g.:

```sh
echo '60' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/height_percent
echo 'Left' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/h_align
```

## Geometry

Geometry settings reshape the surface the moment they change - a shown surface is reshaped in place (no destroy/recreate), a hidden one picks up the new geometry on its next show:

- `height_percent` (default `40`, range 10-100): surface height as a percentage of the output height. Also written automatically shortly after a grab-bar drag settles.
- `width_percent` (default `100`, range 30-100): surface width as a percentage of the output width. `100` reproduces the original full-width surface exactly.
- `h_align` (`Left`, `Center`, or `Right`; default `Center`): horizontal placement of the surface within the output. `Left` and `Right` sit flush against that edge; `Center` splits the leftover space into equal left and right margins. Only meaningful below `width_percent` 100.

## Tab bar position

`tab_position` (`Auto`, `Top`, or `Bottom`; default `Auto`): which side of the surface the tab strip sits on, independent of `edge`. `Auto` keeps the strip beside the grab bar at the leading edge - the side opposite the screen edge, which on the default top anchor is the surface's bottom; `Top`/`Bottom` pin the strip to that side regardless of anchor, so on the matching anchor it sits at the screen edge. Unlike the keys above, a change never reshapes the surface - the rows keep their heights - so a shown surface just reorders immediately. Pick it in the Geometry section of the [settings drawer](settings.md).

## Tab titles

`tab_title_mode` (`Shell`, `AbbreviatedPath`, `LastSegment`, `Numbered`, or `Profile`; default `Shell`): how a tab's name is generated - Guake's "Use VTE titles" checkbox and "Display as tab names" dropdown collapsed into one key. `Shell` shows the shell's OSC 0/2 title verbatim (the pre-setting behavior); `AbbreviatedPath` shortens a trailing-cwd suffix, so `jpt@box:~/src/l1br3/ced` renders `jpt@box:~/s/o/l/ced` and an absolute-path suffix abbreviates identically - and with no report yet (a fresh tab, or a shell that never titles) it abbreviates the cwd itself, `~`-spelling preferred, so a new tab opens as e.g. `~/s/o/l/ced`; `LastSegment` shows the cwd's final component (`ced`, and `(root)` at `/`), including at spawn before any report; `Numbered` parks every tab at `Terminal N` and shell reports never land; `Profile` shows the name of the profile a tab runs - the id resolved at retitle time, so an SSH tab reads `ssh-prod` no matter what its shell reports underneath - and a tab running no profile, or whose profile was deleted while it ran (a dangling id never renders a stale name), falls back to exactly `Shell`'s behavior: the raw report, else `Terminal N`. A user rename wins in every mode, a title with no directory suffix is left alone, and an unreadable cwd falls back to the raw title (the default number when no title exists either). Changing the key re-titles every live tab immediately, renamed ones excepted; while in `Profile` mode, a profiles-list change retitles live tabs the same way (rename a profile in settings and its running tabs pick the new name up at once). An older binary reading a `Profile`-valued key it does not know falls back through the store's tolerant load to the `Shell` default - the mode degrades, never errors. Pick it in the Behavior section of the [settings drawer](settings.md).

## Shell, arguments, and the new tab directory

`shell` (default unset) is the binary plain new tabs run: unset falls back to `$SHELL`, then `/bin/sh` - the Terminal section's free-text row keeps the resolved value as its placeholder. The Terminal section also offers a Shell dropdown built per view from `/etc/shells`: one row per basename (duplicates like `/bin/bash` and `/usr/bin/bash` collapse to the first, entries that no longer exist drop), a pick writes the full path under `shell`, and an unreadable file falls back to the static bash/zsh/fish/sh quartet. The free-text row beneath remains the custom path's editor - an off-list stored value simply selects no dropdown row. A set value is used verbatim, so it must be a path (`/usr/bin/fish`), not a bare name. `args` (default empty) is extra arguments for that shell, split on whitespace with no quoting (the same limitation as a `--cmd` line): `--login --noprofile` arrives as two arguments, and the picker never touches it. Both apply to the next spawn only - a running tab is never re-launched.

`new_tab_cwd` (`Inherit` or `Default`; default `Inherit`) decides where a plain new tab opens: `Inherit` keeps the exact default-spawn chain - an explicit `--cwd`, else the active tab's working directory, else the pty's default directory - while `Default` *replaces* the inherited leg with the `default_cwd` directory. The mode applies to the plain spawns: `Ctrl+Shift+T`, the tab bar `+`, both context menus' plain New Tab row, a bare remote `new-tab`, and the never-empty startup/last-close fresh tab. It never applies to `Ctrl+Shift+H` (home is that bind's contract), duplicates (the source tab's cwd is theirs), profile launches (a preset's own live `cwd`), or session restore (saved cwds re-validate as always). An explicit `--cwd` wins in both modes.

`default_cwd` (default unset) is the directory `Default` mode opens plain tabs in. It is not checked to exist until the spawn: a dead path logs a `warn` once and the tab opens in the pty's default directory - never an error surface, and the config is never rewritten to drop it. With a `default_profile` set, the profile still owns command, cwd, env, and appearance outright - the mode and `default_cwd` govern only the no-default path. Pick both on the Behavior tab of the [settings drawer](settings.md) (the Tabs card's "New tab directory" dropdown, the Terminal card's "Default directory" row) or write them directly:

```sh
echo 'Default' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/new_tab_cwd
echo '"/srv/work"' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/default_cwd
```

## Hide on focus loss

`hide_on_focus_loss` defaults to `true` - a deliberate choice for a hotkey-driven drop-down (focus is the whole interaction model), and notably the *opposite* of Yakuake, Guake, and Ghostty's Linux quick terminal, which all default to keeping the window open on focus loss. Write `false` to keep the overlay open when it loses focus.

## Panel avoidance

`respect_panel` defaults to `false`: the drop-down covers the panel and dock, Yakuake-style. Write `true` to place it inside the compositor's usable area instead - it sits flush below a top panel (or above a bottom dock, together with `edge`), while staying on its configured layer, so on the default `Overlay` layer it still appears above fullscreen windows. The setting applies on the next show - toggle it in the Advanced section of the [settings drawer](settings.md) or write it directly:

```sh
echo true > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/respect_panel
```

With an autohide panel, the hidden panel still reserves its handle strip, so a respecting drop-down sits a few pixels below the screen edge.

## Profiles

Profiles - named presets of what a tab runs - are managed in-app on the [settings drawer](settings.md)'s Profiles tab; the config file can also be hand-edited in the same shape. A profile is its `name` (also its id, matched exactly and case-sensitively), a `command` line (split on whitespace, like `args`), an optional `cwd` that is not checked to exist until the tab spawns, an optional `color_scheme`, an optional `font_size`, an optional `scrollback_lines`, and an optional `env` map of extra environment variables:

```ron
// ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/profiles
(
    {"name": "web-prod", "command": "ssh web-prod", "cwd": Some("/srv/web"), "color_scheme": Some("Dracula"), "font_size": Some(11), "scrollback_lines": Some(50000), "env": Some({"DROPTERM_PROFILE": "prod", "HTTP_PROXY": "http://bastion:3128"})},
    {"name": "top", "command": "htop"},
)
```

The three appearance fields share one resolution rule, applied per field (never per profile): `profile ?? global` - an unset field inherits the global value, a set field wins, and each field decides on its own. Every field is resolved per tab from its recorded profile id at spawn, restore, and switch, and re-resolved on every config pass; a profile deleted while its tabs run degrades them to the global picks field by field - never rewritten, never an error.

`color_scheme` (default unset) is a built-in scheme name - the same registry the global scheme pickers offer ([themeing](theming.md)) - that the profile's tabs run instead of the global pick: applied live, so editing a profile's scheme recolors its running tabs without a respawn. One name, independent of the light/dark mode - a profile is a deliberate environment ("this is prod"), not a theme follower; the global dark/light pair keeps driving every tab without one. Every miss degrades quietly: an unset scheme, a name the registry does not ship (silently, exactly as an unknown global name falls to the COSMIC pair; the config is never rewritten to strip it), or a profile deleted while its tabs run all fall back to the global pick. The window background follows the active tab's resolution, so the fallback background moves with a switch between a scheme-carrying tab and a plain one.

`font_size` (default unset) is a font size in points the profile's tabs render at instead of the global `font_size`. Like the scheme it applies live: a config pass re-resolves it per tab, so editing the profile's size re-renders its running tabs while every other tab keeps following the global pick - and a global font edit moves only the tabs whose profile does not override it.

`scrollback_lines` (default unset) is the scrollback capacity the profile's terminals start with instead of the global `scrollback_lines`. Capacity is fixed when the terminal is created - the same timing the global key itself carries - so an edit lands on the profile's next spawn or respawn, never on a live tab; `switch-profile-tab` onto the profile starts the fresh terminal at it.

`env` (default unset) is a map of extra environment variables its tabs carry, merged over the inherited environment when the pty spawns (`TERM_PROGRAM=dropterm-ssh`, a jump profile's `HTTP_PROXY`, `DROPTERM_PROFILE=prod` for rc files to read). Environment is spawn-time by nature - a running process's environment never changes - so an edit affects the profile's next spawn, never a live tab, and there is no live-apply pass for it. The map applies exactly when the spawn actually runs the preset's command line: a profile whose `command` is blank (a hand-edited config) degrades to a plain shell and applies no profile env, for the same reason it records no profile id - a degraded tab carries none of the profile's identity. Plain, `--cmd`, and duplicate tabs never see the field; a session-restored or `switch-profile-tab`-switched tab runs the current definition's env through the same id re-resolution every other profile fact uses. Values are verbatim strings - no expansion or interpolation, ever (`=` in a value is fine; shell expansion belongs to the shell) - and keys must be non-empty and free of `=` (environ entry names cannot carry it); the editor enforces that at write time, while loading tolerates a hand-edited file as-is and never rewrites it.

`default_profile` (default unset) names the profile plain new tabs launch: with it set, `Ctrl+Shift+T`, the tab bar's `+` (its plain click), the context menu's plain "New Tab" row, a bare `new-tab`, and the fresh-startup tab all run it as full profile tabs (a `duplicate-tab` still spawns a fresh shell - the running command is never copied - and carries the source's rename pin). A name the list no longer carries quietly falls back to the plain shell, and the config is never rewritten to drop it. Launching under a profile is live six ways: that default application, `new-tab --profile NAME` (above), the tab context menu - "New Tab" becomes a submenu exactly when the list is non-empty, its first child the plain New Tab, then one row per profile in config order - the tab bar `+`'s right-click dropdown, the same composition opening beside the gear exactly when the list is non-empty, the context menu's Switch Profile folder (after Duplicate Tab, one row per profile, again exactly when the list is non-empty), and the per-tab context menu's Switch Profile section (right-click a tab button: the same one row per profile, respawning *that* tab - clicked, not active) - the last two acting on a *live* tab: a pick respawns its tab in place under the chosen profile, the exact semantics of `switch-profile-tab --profile NAME` on that tab (its terminal is replaced - running command and scrollback lost - while the slot, the activation, and a pinned rename stay). Config edits apply to future spawns only; a running tab is never re-launched mid-run. Managing profiles - creating, editing, deleting, reordering, cloning, and picking the default - all happens on the Profiles tab of the [settings drawer](settings.md), with the same instant write-through as every other setting.

## Keybindings

Keybindings are customized in-app on the [settings drawer](settings.md)'s Keybindings tab; the config file can also be hand-edited in the same shape. `custom_key_binds` (default empty - all defaults) maps one action id to one binding: modifier names (`Ctrl`, `Shift`, `Alt`, `Super`) plus a key name (single letters are case-insensitive; named keys spell their names - `Tab`, `PageUp`, `F1`, `Space`, `Insert`). The action ids are the file-format vocabulary - `copy`, `paste`, `paste_primary`, `select_all`, `zoom_in`/`zoom_out`/`zoom_reset`, `hide`, `settings`, `new_tab`, `new_tab_home`, `close_tab`, `duplicate_tab`, `rename_tab`, `next_tab`, `prev_tab`, `move_tab_left`, `move_tab_right`, `activate_tab_1` through `activate_tab_8`, `activate_last_tab`, `clear_scrollback`, `copy_primary`:

```ron
// ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/custom_key_binds
{
    "copy": (modifiers: [Ctrl, Alt], key: "C"),
    "activate_tab_3": (modifiers: [], key: "F2"),
}
```

A custom binding replaces *all* of that action's default chords; removing the entry reverts to the defaults. Entries with unknown ids or an empty key are skipped at load with a warning - never fatal. A chord two actions claim resolves deterministically to the later id. The live map follows the file while the app runs: the very next key press after a write - from the editor or a hand edit - uses the new bindings.
