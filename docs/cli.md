# CLI reference

One binary, one running instance: every action below rides DropTerm's single-instance D-Bus seam - a second launch tells the first instance what to do and exits.

```sh
dropterm [toggle]              # default: toggle the overlay
dropterm show | hide           # dedicated show/hide keys
dropterm new-tab [--cwd DIR] [--cmd COMMAND] [--profile NAME]
dropterm close-tab [--tab-index N]
dropterm rename-tab TITLE [--tab-index N]
dropterm duplicate-tab [--tab-index N]
dropterm switch-profile-tab --profile NAME [--tab-index N]
dropterm select-tab N          # strict: no wrap, no 9 = last
dropterm --is-visible          # query only, never shows the overlay
```

## new-tab

Script new tabs with `dropterm new-tab [--cwd DIR] [--cmd COMMAND] [--profile NAME]`:

```sh
dropterm new-tab                 # a shell in the active tab's cwd
dropterm new-tab --cwd ~/src     # a shell in an explicit directory
dropterm new-tab --cmd htop      # a tab running htop
dropterm new-tab --profile web-prod  # a tab running a configured profile
```

A `new-tab` sent while the app runs opens the tab in the running instance and shows the overlay; sent while it is not running, the app starts with that tab. `--cmd` runs the command *instead of* the shell (cosmic-term `-e` semantics): whitespace-split with no quoting support, the tab closes when the command exits, and the tab is deliberately *not* part of the saved session - the session never resumes a program. `--profile NAME` runs that configured profile's command line in its `cwd` instead of the shell, identically remote and at first launch; it cannot be combined with `--cmd`, an explicit `--cwd` wins over the profile's own `cwd`, and - unlike `--cmd` - the tab is part of the saved session. A name the config doesn't carry logs a `warn` and opens a plain shell tab: the request is fire-and-forget, so there is no error to report back. `--cwd` beats inheritance; a nonexistent directory is ignored with a warning and the tab opens in the fallback directory instead. Where the tab lands follows `new_tab_after_current` (default `true`: directly after the active tab; `false`: at the end of the tab list).

Profiles themselves - the RON list, `default_profile`, and their launch paths - are documented in [Configuration](configuration.md).

## Tab verbs

The tab verbs script tab management over the same seam. All indices are 1-based (`0` is rejected at the CLI edge; out-of-range targets warn and no-op):

```sh
dropterm close-tab                       # close the active tab
dropterm close-tab --tab-index 2         # close the second tab
dropterm rename-tab "work"               # pin a title on the active tab
dropterm rename-tab ""                   # clear the override; shell titles resume
dropterm rename-tab "logs" --tab-index 3
dropterm duplicate-tab                   # fresh shell in the active tab's cwd
dropterm switch-profile-tab --profile web-prod  # respawn the active tab under the profile
dropterm select-tab 2                    # activate the second tab, strictly
```

`close-tab` and `rename-tab` are quiet state edits for scripts - they never show the overlay. `duplicate-tab`, `switch-profile-tab`, and `select-tab` are tab-facing: like `new-tab`, they show the overlay, and a request arriving mid-slide-out reverses it instead of finishing the hide. A rename pins a title that no shell OSC title change can overwrite; the empty string is the documented reset (shell titles resume), and a pending inline rename on that tab is discarded when the remote title lands. `duplicate-tab` opens a *fresh shell* in the target tab's cwd - the running command is never copied - lands after the active tab (always, whatever `new_tab_after_current` says), and becomes active; the target's rename pin carries over (duplicating a tab renamed "work" opens the duplicate already named "work"), while an unrenamed tab's title resolves naturally from its own shell; with an unresolvable target it degrades to the active tab's cwd and pin. The `Ctrl+Shift+D` keybind and the context menu's "Duplicate Tab" row are the same verb on the active tab; the per-tab context menu's row (right-click a tab button) is the same verb on the clicked tab. `switch-profile-tab --profile NAME` respawns the target tab *in place* under the profile: its terminal is replaced by a fresh one running the profile's command in its `cwd` - the running command and scrollback are lost, the same cost as closing and reopening the tab - while the tab keeps its slot in the row, its activation state, and a pinned rename; from then on the tab saves to the session as a profile tab, and a restart restores it as that profile. The spawn `cwd` composes like a profile launch's: the profile's own live `cwd`, else the tab's current working directory. An unrenamed tab's default title renumbers after the switch (`Terminal N` follows the fresh terminal); a renamed tab keeps its rename. An unknown `--profile` name or out-of-range `--tab-index` warns and changes nothing - unlike `duplicate-tab` there is no sensible fallback, so it never degrades to a spawn. The context menu's Switch Profile folder (present exactly when ≥1 profile is configured) is the same verb on the active tab, and the per-tab context menu's Switch Profile section the same verb on the clicked tab. `select-tab N` is strict: no wrap, no `9 = last`. Sent while the app is not running, all five verbs just start the app plain - only `new-tab` carries startup meaning.

## Querying visibility

Scripts can also *ask* instead of act: `dropterm --is-visible` prints `true`/`false` and exits with a matching code (0 visible, 1 hidden or not running), without ever summoning the terminal. It cannot be combined with the action subcommands:

```sh
dropterm --is-visible && echo "the terminal is up"
if dropterm --is-visible; then ...; fi   # exit code branch
```

The running instance serves the answer over D-Bus (a read-only `Visible` property on the bus name it already owns), so the query is cheap and works from any process.
