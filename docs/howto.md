# Common tasks

The task-shaped entry point for DropTerm, the drop-down terminal for COSMIC: what you most often want to do, the exact key or verb that does it, and a link to the page that owns the detail. "Writing" a key below means either of the two edit paths - a toggle or field in the [settings drawer](settings.md), or one file per key under `~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/` ([configuration](configuration.md)). Nothing here repeats the reference - every entry is one hop from the full story.

## First run

- **Install it and bring it down once** - take the release tarball or build from source ([README: Installing](https://github.com/l1br3-io/cosmic-ext-dropterm#installing)), then bind `cosmic-ext-dropterm` to a COSMIC custom shortcut (`F12` is a safe pick) and press it - the walk, including why it must be a COSMIC shortcut, is [README: Setting up the shortcut](https://github.com/l1br3-io/cosmic-ext-dropterm#setting-up-the-shortcut). No shortcut configured yet? Just launch the binary from a terminal - one instance runs, and every further launch toggles the overlay ([development](development.md#running-locally-without-a-shortcut)).

## Shape and placement

- **Drop from the bottom edge instead of the top** - write `edge` (`Top`/`Bottom`); geometry applies live ([configuration](configuration.md#geometry)).
- **Take less than the full width, or center it** - `width_percent` (30-100) plus `h_align` (`Left`/`Center`/`Right`); drag the grab bar (the drag handle at the surface's edge) to set the height and it is remembered ([configuration](configuration.md#geometry)).
- **Stop covering the panel and dock** - write `respect_panel` and the drop-down sits inside the compositor's usable area ([configuration](configuration.md#layer-focus-and-panels)).
- **Sit above fullscreen windows - or below them** - `layer` picks `Overlay` (above) or `Top` (below, friendlier to input methods) ([configuration](configuration.md#layer-focus-and-panels)).

## Tabs, profiles, and directories

- **Open an ssh session (or any preset) as a one-click tab** - create a profile carrying its command, directory, scheme, font size, scrollback, and environment; launch it from the context menu, the `+` dropdown, or as the default every plain new tab runs ([configuration: profiles](configuration.md#profiles)).
- **Make every new tab start in one fixed directory** - set `new_tab_cwd` to `Default` and `default_cwd` to the path; `Inherit` (the default) instead follows the active tab's directory ([configuration](configuration.md#shell-and-directories)).
- **Keep the session across reboots** - on by default: tabs, their directories and profiles save on change and restore on launch; `session_persistence_enabled` is the switch ([sessions](sessions.md)).
- **Close a tab without the confirmation prompt** - the guardrail asks before closing tabs that run a foreground process; `close_confirmation_enabled: false` restores instant closes ([configuration](configuration.md#close-confirmation)).

## Look and feel

- **Pin light or dark regardless of the system theme** - `theme_preference` (`System`/`Dark`/`Light`), with a scheme per mode (`color_scheme_dark`/`color_scheme_light`) from the twelve built-ins ([configuration](configuration.md#appearance), [theming](theming.md#color-schemes)).
- **Turn off the slide animation** - `slide_animation_enabled: false` makes show/hide instant ([configuration](configuration.md#behavior-toggles), [animation](animation.md)).
- **Tailor the status band** - hide it (`Ctrl+Shift+B`), or keep it and hide individual segments - clock, branch, zoom, any of them - each with its own `status_bar_show_*` key ([status bar](status-bar.md#toggling-and-tailoring)).
- **Run a tab in its own colors and font size** - the same profile fields: a set field wins for that tab, an unset one inherits the global look ([configuration: profiles](configuration.md#profiles)).

## Behavior and feedback

- **Stop it hiding when I click another window** - that is `hide_on_focus_loss` (default `true`, the hotkey-driven interaction model); write `false` to keep the overlay up ([configuration](configuration.md#behavior-toggles)).
- **Silence the bell - or its desktop notifications** - `bell_notification_enabled` gates the hidden-terminal notification and `command_finish_notification_enabled` the finished-command one; a visible terminal never notifies, it only marks the tab ([bell](bell.md)).
- **Get fish-style suggestions - or turn them off** - new on main, shipping in the release after v0.2.0: dim ghost text after the cursor completes the most recent matching command from this tab's own history - Right accepts it, Alt+Right takes one word; path, command-name, and flag completion ride the same gate, and `autosuggestions_enabled` is the kill switch ([autosuggestions](autosuggestions.md)).
- **Stop the suggestion ghost from appearing at all over ssh** - no action needed: suggestions are local-machine knowledge and stay silent over ssh by design ([autosuggestions](autosuggestions.md)).

## Keys

- **See every default chord** - the full table with each action's config id ([keybindings](keybindings.md)).
- **Rebind one** - the settings drawer's Keybindings editor (arm, press the chord) or hand-edit `custom_key_binds`; a committed rebind applies to the very next key press ([keybindings: customizing](keybindings.md#customizing)).

## Scripting and control

- **Open or manage tabs from a script or keybind** - `cosmic-ext-dropterm new-tab` with `--cwd`/`--cmd`/`--profile`, the tab verbs (`close-tab`, `rename-tab`, `duplicate-tab`, `switch-profile-tab`, `select-tab`), and `--is-visible` for scripts that ask - all delivered to the one running instance (a second launch never starts a second app) ([CLI reference](cli.md)).
- **Give show and hide their own keys** - bind `cosmic-ext-dropterm show` and `hide` to dedicated shortcuts instead of one toggle ([CLI reference](cli.md)).
- **Run a development copy beside the installed one** - the `just dev*` recipes run a separate `-dev` identity: own config, own bus name, own launcher - the installed release never sees it ([development: launcher](development.md#development-launcher)).
- **Toggle it from the COSMIC panel** - install the panel applet for a click target that also shows whether DropTerm is running ([applet](applet.md)).
