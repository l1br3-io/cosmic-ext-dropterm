# Configuration

DropTerm keeps every setting in RON files under one directory and reloads them live - changes apply without a restart, whether made in the [settings drawer](settings.md) or by editing the file directly.

```
~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/
```

Each of the 51 keys is its own file in that directory. Edit one directly:

```sh
echo '60' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/height_percent
echo 'Left' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/h_align
```

Most keys are editable in the [settings drawer](settings.md); the exceptions write themselves (`settings_drawer_width`) or wait for behavior that does not ship yet ([Parked and reserved keys](#parked-and-reserved-keys)).

## The default config directory

What a fresh configuration carries - every key that has a default, with the file's whole content as its value (a genuine string quoted; an enum, a number, or a bool bare). Pinned to the code by a test, so it cannot drift:

| Key file | Content |
|---|---|
| `height_percent` | `40` |
| `width_percent` | `100` |
| `h_align` | `Center` |
| `edge` | `Top` |
| `tab_position` | `Auto` |
| `tab_title_mode` | `Shell` |
| `layer` | `Overlay` |
| `keyboard_mode` | `Exclusive` |
| `respect_panel` | `false` |
| `show_in_dock` | `false` |
| `font_name` | `"Noto Sans Mono"` |
| `font_size` | `14` |
| `color_scheme_dark` | `"COSMIC Dark"` |
| `color_scheme_light` | `"COSMIC Light"` |
| `opacity` | `100` |
| `scrollback_lines` | `10000` |
| `hide_on_focus_loss` | `true` |
| `session_persistence_enabled` | `true` |
| `slide_animation_enabled` | `true` |
| `bell_notification_enabled` | `true` |
| `command_finish_notification_enabled` | `true` |
| `command_finish_notification_after_secs` | `5` |
| `new_tab_after_current` | `true` |
| `close_confirmation_enabled` | `true` |
| `settings_drawer_click_close` | `true` |
| `theme_preference` | `System` |
| `new_tab_cwd` | `Inherit` |
| `status_bar_enabled` | `true` |
| `status_bar_show_process` | `true` |
| `status_bar_show_path` | `true` |
| `status_bar_show_branch` | `true` |
| `status_bar_show_profile` | `true` |
| `status_bar_show_grid` | `true` |
| `status_bar_show_tab` | `true` |
| `status_bar_show_zoom` | `true` |
| `status_bar_show_clock` | `true` |
| `status_bar_show_duration` | `true` |
| `status_bar_show_exit_status` | `true` |
| `status_bar_show_progress` | `true` |
| `status_bar_show_program_status` | `true` |
| `status_bar_show_scrollback` | `true` |
| `tab_bar_border` | `true` |
| `status_bar_border` | `true` |
| `status_bar_activity_mark` | `true` |
| `autosuggestions_enabled` | `true` |
| `autosuggestions_paths_enabled` | `true` |
| `autosuggestions_commands_enabled` | `true` |
| `autosuggestions_generators_enabled` | `true` |
| `autosuggestions_completions_extended` | `true` |
| `autosuggestions_menu_enabled` | `true` |
| `autosuggestions_menu_auto` | `false` |
| `autosuggestions_delay_ms` | `0` |

The remaining keys have no default to write, so their files do not exist until you set them: `shell` (unset falls back to `$SHELL`, then `/bin/sh`), `args`, `default_profile`, `default_cwd`, `profiles` and `custom_key_binds` (empty lists write nothing), `autosuggestions_aliases` (empty by default - no alias re-targets anything), `output` (reserved - nothing reads it yet), and `settings_drawer_width` (unset until the drawer's drag handle first writes it).

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
| `font_name` | font family name | `Noto Sans Mono` | The terminal's typeface. The settings drawer's picker lists the installed monospace families; a name the renderer cannot resolve falls back to its default while keeping your value in the file. |
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
| `Shell` | The shell's own title (OSC 0/2), verbatim - `Terminal N` until it reports one. An empty report (a program clearing the title it set, e.g. opencode on Ctrl+C) reads as no report, so the tab falls back to `Terminal N` instead of going blank. |
| `AbbreviatedPath` | The title's trailing path, middle segments shortened: `~/src/l1br3/ced` renders `~/s/o/l/ced`. With no title yet, the cwd itself, abbreviated the same way. |
| `LastSegment` | The cwd's final component - `ced`, or `(root)` at `/`. |
| `Numbered` | Always `Terminal N`; shell titles never land. |
| `Profile` | The profile's name ([profiles](#profiles)). A tab with no profile - or a deleted one - falls back to `Shell`'s behavior. |

A manual rename wins in every mode. Changing the mode re-titles every live tab immediately, renamed ones excepted; while in `Profile` mode, renaming a profile retitles its running tabs at once.

## Status bar

`status_bar_enabled` (default `true`) shows the 28 px band at the surface's free edge, beside the grab bar. The band's segments, click-to-copy, selection, and context menu are covered in their own page: [Status bar](status-bar.md). Toggle it with `Ctrl+Shift+B`, on the Behavior tab, or by writing the key.

| Key | Shows | Default |
|---|---|---|
| `status_bar_activity_mark` | The missed-activity halo - a thin attention ring around the process disc while output landed on the active tab under a hidden surface, until a touch (input, paste, scroll, pane click) or a tab switch acknowledges it. | `true` |
| `status_bar_show_process` | Foreground process name - filled disc while one runs, hollow ring over the idle shell. | `true` |
| `status_bar_show_path` | Working directory, `$HOME` collapsed to `~`. | `true` |
| `status_bar_show_branch` | Git branch of the cwd (worktrees and submodules resolve). | `true` |
| `status_bar_show_profile` | The tab's profile name. | `true` |
| `status_bar_show_grid` | Cell dimensions (`80×24`) - only after a resize away from the tab's spawn size. | `true` |
| `status_bar_show_tab` | Tab position (`2/5`). | `true` |
| `status_bar_show_zoom` | Zoom delta (`+200%`) while the font is off its base size. | `true` |
| `status_bar_show_clock` | Wall clock (`HH:MM`). | `true` |
| `status_bar_show_duration` | Command time (`2:41` while a command runs, `cargo 2:41` after it finishes) - derived from the foreground edges the engine already diffs, no shell integration. Hiding it stops its 1 s tick. | `true` |
| `status_bar_show_exit_status` | Exit status (`✗1`) of the last finished command, beside its duration, in the scheme's own red - sourced solely from the shell's OSC 133;D report (opt-in shell integration; see the status band page). Success renders nothing; without integration the band never guesses. Clicking the chip copies the bare code. | `true` |
| `status_bar_show_progress` | The running command's own progress (`42%`) beside the exit chip - sourced solely from the shell's OSC 9;4 report (opt-in shell integration; see the status band page): `…` while indeterminate, the value in the scheme's own red on error (`!` if none was ever given), paused plain. State 0, the command's finish, and ~15 s without a refresh each clear it - a dead reporter never leaves its last number lying. Clicking the chip copies the bare percent. | `true` |
| `status_bar_show_program_status` | The program's own report on the process disc and name - sourced solely from the shell's OSC 7501 report (opt-in shell integration; see the status band page): `blocked` (the program waits on you) and `error` tint the disc's fill in the scheme's own red, any state's progress percent rides the name (`cargo 40%`), and `working`/`done`/`idle` leave the disc alone. The command's finish drops `working`/`blocked`, `done`/`error` persist until replaced or cleared, and a full terminal reset clears everything. Only the root report is consumed - child records and the `app`/`title`/`msg`/`kind` keys are ignored by design. | `true` |
| `status_bar_show_scrollback` | Lines scrolled back from the bottom (`↑12`) - shown only while the viewport is off-bottom; clicking the chip jumps to the bottom. | `true` |

All live. A hidden segment is never computed - hiding the branch stops the `.git` reads, hiding the clock stops its tick, hiding the duration stops its 1 s tick while a command runs, hiding the exit status stops its verdict sampling.

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
| `command_finish_notification_enabled` | `true` | Post a desktop notification when a command finishes while the terminal is hidden, carrying its duration and its name when one was sampled ([bell](bell.md)). One notification per tab per hide period; `false` silences them. |
| `command_finish_notification_after_secs` | `5` | Minimum command duration, in seconds, that posts the finished-command notification. A hidden `ls` never pings; a finish exactly at the threshold counts. |
| `autosuggestions_enabled` | `true` | Fish-style autosuggestions: dim ghost text after the cursor completing the most recent matching command from this tab's own history ([autosuggestions](autosuggestions.md)). Right accepts it, Alt+Right takes one word, anything else dismisses it - nothing executes without an accept. `false` is the kill switch. |
| `autosuggestions_paths_enabled` | `true` | Path completion joins the autosuggestions: a typed path token (`~/sr`, `../co`, `./te`, `/usr/bi`) ghosts its first sorted match from the parent directory, directories suffixed `/`, names the shell would mangle backslash-escaped or not suggested at all ([autosuggestions](autosuggestions.md)). Local-machine knowledge - silent over ssh. `false` restores history-only suggestions. |
| `autosuggestions_commands_enabled` | `true` | Command-name completion joins the autosuggestions: at the command position the typed word ghosts its shortest match among the executables on the tab's spawn-time `$PATH` (ties alphabetical; the match must extend the word) - so a fresh tab with an empty history finishes `gi` → `git`, and after one real run history takes over invisibly ([autosuggestions](autosuggestions.md)). A spawn-time snapshot, never rescanned; local-machine knowledge - silent over ssh. `false` restores history-only suggestions. |
| `autosuggestions_generators_enabled` | `true` | The git value generators join the autosuggestions: cached branch, tag, and working-tree-change names completing the argument seats of `git checkout`, `git switch`, `git add`, and `git restore` - `git checkout ma` ghosts `in` from a cached `main` ([autosuggestions](autosuggestions.md)). The names are read from the repository the tab's shell sits in by fixed, reviewed, read-only git commands DropTerm owns (nothing you can configure runs anything), with an environment scrubbed to `HOME` and a pinned `C` locale, the tab's spawn-time `$PATH` git by absolute path, and a five-second kill deadline - silence on every failure, nothing leaves the machine, and ssh or non-shell tabs never run a generator at all. Results cache stale-while-revalidate and refresh when the completions menu opens; plain typing never spawns anything. `false` stops every spawn and serves nothing from the cache. Applies live. |
| `autosuggestions_completions_extended` | `true` | The installed extended completions tier: the packaged `completions-extended.toml` - every corpus command beyond the embedded floor's core tranche, found in the XDG data dirs - adds beside the embedded floor while the key is on, so a default install knows ~624 commands' subcommands, flags, and argument suggestions instead of the core tranche's ninety-two ([autosuggestions](autosuggestions.md)). Defaults to `true` - the file ships inside the package, so generosity costs no download and no network; the toggle is the kill switch, not the opt-in. An absent, unreadable, or invalid file answers the embedded floor whatever the key says, and the user's own `completions.toml` overrides both tiers. Flips apply live. |
| `autosuggestions_menu_enabled` | `true` | The Ctrl+Space completion menu: an explicit-trigger candidate list over the tab's current input - every history prefix match, path entry, PATH name, and curated subcommand/flag at once, where the ghost must settle for one choice. Up/Down navigate, Tab and Enter accept only while it shows (bare Tab stays the shell's own completion key whenever no menu shows; at an ineligible prompt the chord forwards to the shell verbatim) ([autosuggestions](autosuggestions.md)). `false` removes the menu surface exactly - the ghost is untouched. Flips apply live: a showing menu closes the moment the switch lands. |
| `autosuggestions_menu_auto` | `false` | The auto-popup opt-in for the completion menu: with the key on, an idle prompt whose typed segment collects two or more candidates pops the menu without the chord, closing again when narrowing leaves one or nothing and on every ineligibility ([autosuggestions](autosuggestions.md)). Defaults to `false` - no surveyed peer ships auto-popup on; `false` is today exactly (the chord is the only opener). Applies live. |
| `autosuggestions_delay_ms` | `0` | The autosuggest idle delay, in milliseconds: a qualifying suggestion is withheld until the typed segment has been stable this long - so no ghost churns while you type fast, it appears once at the pause (200-300 suits that complaint; `0` is fish parity, today's behavior). Any keystroke restarts the wait and dismisses a showing ghost instantly; Right/Alt+Right are unchanged the moment the ghost shows, and the knob changes *when* a suggestion is seen, never *which* one wins ([autosuggestions](autosuggestions.md)). Applies live. |
| `autosuggestions_aliases` | `{}` | The user-declared alias map, hand-edited in the config file (the map is author-sized, not toggle-sized): alias → expansion, `gc = "git checkout"`, re-targeting the specs and catalog completions at the expansion's words - a mapped alias ghosts what its expansion ghosts (`gc --dr` ghosts git's `--dry-run`, `gc che` ghosts `ckout`), with history still outranking always and silent over ssh. Authoritative for exactly what you declare: DropTerm never reads or parses your shell rc files and never runs anything in the session to discover aliases, and an accept still writes only the suggestion's suffix - never the expansion unless you accepted ghost text that spelled it ([autosuggestions](autosuggestions.md)). Chains resolve recursively (an alias mapping to another alias), depth-capped at 3 and cycle-safe. Applies live at the next typed prefix. |
| `new_tab_after_current` | `true` | New tabs land directly after the active tab. `false` appends at the end of the list (Guake's ordering); `duplicate-tab` always lands after the active tab. |
| `scrollback_lines` | `10000` | Scrollback capacity per terminal. Applies to the next spawn - capacity is fixed when a terminal is created. |

The toggles apply at their next natural trigger - the session toggle at the next save/restore cycle, the slide toggle at the next transition, the bell toggle at the next bell, placement at the next new tab.

The alias map is written by hand, one expansion per declared alias:

```ron
// ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/autosuggestions_aliases
{
    "gc": "git checkout",
}
```

## Layer, focus, and panels

| Key | Values | Default | What it does |
|---|---|---|---|
| `layer` | `Overlay`, `Top` | `Overlay` | `Overlay` sits above fullscreen windows; `Top` sits below them but behaves better with input methods. |
| `keyboard_mode` | `Exclusive`, `OnDemand` | `Exclusive` | How the surface asks for keyboard focus. `Exclusive` always grabs the keyboard while shown; `OnDemand` is gentler but not auto-focused by every compositor. |
| `respect_panel` | `true`/`false` | `false` | `true` places the drop-down inside the compositor's usable area instead of covering panels and docks, staying on its configured layer. Applies on the next show. |
| `show_in_dock` | `true`/`false` | `false` | Whether the app keeps a presence in the dock and the window switcher. The invisible clipboard anchor that holds your copies across every hide is, on Wayland, always a mapped window — `false` makes it a layer surface no switcher or dock can list (summon by shortcut, the [panel applet](applet.md), or the CLI); `true` keeps it a normal window, so DropTerm shows in both. A flip applies immediately; whatever is on the clipboard at that moment is lost once. |

The anchor itself never takes input or focus in either mode — it exists to keep a copied selection alive while the drop-down is hidden.

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

## The completions data file

One more file can live in the config directory beside the keys: `completions.toml`, the autosuggestions' subcommand and flag table. The curated five commands (`git`, `cargo`, `docker`, `systemctl`, `npm`) are built in; this file extends the table with your own commands - a command you declare adds beside the built-ins, and a command you re-declare replaces its built-in entry whole (the one way to give a built-in's entries descriptions). DropTerm ships a generated core tier to start from: [`data/completions.toml`](https://github.com/l1br3-io/cosmic-ext-dropterm/blob/main/data/completions.toml) in the repository - ninety-two commands with menu descriptions, converted from withfig/autocomplete at a pinned version (MIT; the licensing audit in the [seed audit](research/completions-seed-audit.md), the conversion and provenance in the [breadth record](research/completions-breadth.md)) - copy it to the config directory and edit from there.

```toml
# ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/completions.toml

# One table per command. `name` is the command word the table
# answers for - through the alias map too, so a mapped alias
# re-targets here exactly as it does the built-ins.
[[commands]]
name = "gh"

# Subcommands complete the first argument seat (`gh pr` ghosts
# and lists them). Every entry is a `name` plus an optional
# `description` - the completions menu shows it beside the row,
# dimmed; the ghost ignores descriptions (it is text only).
[[commands.subcommands]]
name = "pr"
description = "Manage pull requests"

[[commands.subcommands]]
name = "pr-check"

# Long flags complete the flag seat from any argument position
# (`gh --rep` finishes `--repo`). A flag's other forms - short
# ones included - belong in its `aliases`, never as their own
# entries: short forms are recognized, never suggested.
[[commands.flags]]
name = "--repo"
description = "Select another repository"
```

The shape is recursive: a `[[commands.subcommands]]` entry is itself a command, taking the same keys and its own `subcommands`, `flags`, and positional `args`, so `docker compose` and everything under it are described exactly as `docker` is. A command or subcommand also takes `aliases` (its other names, `co` for `checkout`), `hidden` (suggested only when typed exactly), `priority` (a 0-100 ordering hint), and `filter_strategy` (`"prefix"`, the default, or `"fuzzy"`). A flag entry takes `aliases` for its other forms - short forms included, so the table knows a typed `-d` is `--detach`, while short forms themselves are never suggested - plus `persistent` (the flag rides into every subcommand), `exclusive_on` (flags that hide it once typed), `hidden`, `priority`, and the `args` it consumes. An `args` entry takes `optional`, `variadic` (consumes every following token), `command` (the argument is itself a command word - the `sudo` shape), `template` (`"filepaths"` or `"folders"`, a path-shaped argument), and literal `suggestions` (a list of `name` + `description` entries). Every key at every depth is optional with a default - an entry carrying none of them is the plain `name`-plus-`description` shape above - and the same rejection rule holds at every depth: an unknown key anywhere fails the whole file. The top level's `subcommands` and `flags` complete as they always have; the nested tiers ride the table for the completion walker that reads them.

Rules the loader holds:

- **No executable content - ever.** The schema rejects unknown keys, so a file carrying a `script` field (or any key the format does not define) fails whole: DropTerm logs at debug level and answers with the built-in table, never a half-loaded one. Data imports; scripts never arrive.
- The file is optional. No file, an unreadable file, or an invalid file is exactly the built-in behavior - the curated five, unchanged.
- Entries sort by name at load (the match policy - strict prefix, must extend, shortest wins, ties alphabetical - rides sorted lists); an unsorted file is fine.
- An edit applies at the next tab you open (the file reads fresh at spawn) or the next settings change (the file re-reads on every config pass); tabs already open pick it up at that pass's next typed prefix.

