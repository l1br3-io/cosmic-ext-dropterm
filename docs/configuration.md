# Configuration

Every DropTerm setting is one RON file in one config directory, reloaded live - no restart needed.

## Location and live reload

DropTerm uses `cosmic-config` and reloads its config live - no restart needed. The file lives at:

```
~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/
```

Each setting is its own RON file inside that directory (`height_percent`, `width_percent`, `h_align`, `edge`, `tab_position`, `layer`, `keyboard_mode`, `respect_panel`, `shell`, `args`, `font_name`, `font_size`, `color_scheme_dark`, `color_scheme_light`, `theme_preference`, `opacity`, `scrollback_lines`, `hide_on_focus_loss`, `session_persistence_enabled`, `slide_animation_enabled`, `bell_notification_enabled`, `tab_title_mode`, `new_tab_after_current`, `profiles`, `default_profile`). Every one of them except the two profile keys (documented below - their settings page ships later) is editable in the [settings window](settings.md); the files can also be edited directly, e.g.:

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

`tab_position` (`Auto`, `Top`, or `Bottom`; default `Auto`): which side of the surface the tab strip sits on, independent of `edge`. `Auto` keeps the strip beside the grab bar at the leading edge - the side opposite the screen edge, which on the default top anchor is the surface's bottom; `Top`/`Bottom` pin the strip to that side regardless of anchor, so on the matching anchor it sits at the screen edge. Unlike the keys above, a change never reshapes the surface - the rows keep their heights - so a shown surface just reorders immediately. Pick it in the Geometry card of the [settings window](settings.md).

## Tab titles

`tab_title_mode` (`Shell`, `AbbreviatedPath`, `LastSegment`, or `Numbered`; default `Shell`): how a tab's name is generated - Guake's "Use VTE titles" checkbox and "Display as tab names" dropdown collapsed into one key. `Shell` shows the shell's OSC 0/2 title verbatim (the pre-setting behavior); `AbbreviatedPath` shortens a trailing-cwd suffix, so `jpt@box:~/src/l1br3/ced` renders `jpt@box:~/s/o/l/ced` and an absolute-path suffix abbreviates identically; `LastSegment` shows the cwd's final component (`ced`, and `(root)` at `/`); `Numbered` parks every tab at `Terminal N` and shell reports never land. A user rename wins in every mode, a title with no directory suffix is left alone, and an unreadable cwd falls back to the raw title. Changing the key re-titles every live tab immediately, renamed ones excepted. Pick it in the Behavior card of the [settings window](settings.md).

## Hide on focus loss

`hide_on_focus_loss` defaults to `true` - a deliberate choice for a hotkey-driven drop-down (focus is the whole interaction model), and notably the *opposite* of Yakuake, Guake, and Ghostty's Linux quick terminal, which all default to keeping the window open on focus loss. Write `false` to keep the overlay open when it loses focus.

## Panel avoidance

`respect_panel` defaults to `false`: the drop-down covers the panel and dock, Yakuake-style. Write `true` to place it inside the compositor's usable area instead - it sits flush below a top panel (or above a bottom dock, together with `edge`), while staying on its configured layer, so on the default `Overlay` layer it still appears above fullscreen windows. The setting applies on the next show - toggle it in the Advanced card of the [settings window](settings.md) or write it directly:

```sh
echo true > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/respect_panel
```

With an autohide panel, the hidden panel still reserves its handle strip, so a respecting drop-down sits a few pixels below the screen edge.

## Profiles

Profiles - named presets of what a tab runs - are config-file-only for now: hand-edit them until their settings page ships. A profile is its `name` (also its id, matched exactly and case-sensitively), a `command` line (split on whitespace, like `args`), and an optional `cwd` that is not checked to exist until the tab spawns:

```ron
// ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/profiles
(
    {"name": "web-prod", "command": "ssh web-prod", "cwd": Some("/srv/web")},
    {"name": "top", "command": "htop"},
)
```

`default_profile` (default unset) names the profile plain new tabs launch: with it set, `Ctrl+Shift+T`, the context menu's plain "New Tab" row, a bare `new-tab`, and the fresh-startup tab all run it as full profile tabs (a `duplicate-tab` still spawns a fresh shell - the running command is never copied). A name the list no longer carries quietly falls back to the plain shell, and the config is never rewritten to drop it. Launching under a profile is live three ways: that default application, `new-tab --profile NAME` (above), and the tab context menu - "New Tab" becomes a submenu exactly when the list is non-empty, its first child the plain New Tab, then one row per profile in config order. Config edits apply to future spawns only; a running tab is never re-launched mid-run. Managing profiles in the settings window - creating, editing, deleting, and picking the default - is the one piece still to ship; until then the list is hand-edited as above.
