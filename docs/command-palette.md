# Command palette

One search box over every app verb: press `Ctrl+Shift+P`, type a fragment, press Enter. The palette opens over the live terminal as an in-window panel - the terminal stays visible beneath the dim scrim, and typing goes to the palette, never to the shell under it.

## What it lists

The catalog composes fresh at every open, so it always matches the session as it is right now:

- **Every keybindable action** - the whole registry the [keybindings](keybindings.md) table documents, grouped under its four groups, each row showing its effective chord beside the label (your rebinds included)
- **Close Other Tabs** and **Close Tabs to the Right** - the per-tab menu's verbs, acting on the active tab
- **New tab: <profile>** and **Switch to profile: <profile>** - one pair per profile, in config order
- **Go to tab: <title>** - one row per open tab, covering positions beyond the nine keybound slots
- **Settings: <page>** - each of the [Settings window](settings.md)'s nine pages, opening the window directly at it
- **Scheme: <name>** - every scheme in the merged registry (the built-ins plus your own [theme files](theming.md#user-defined-scheme-files)), applied live

With an empty query the rows show under their section headers in registry order. Typing replaces the sections with one flat list of the matches, best first.

## Searching

The match is fuzzy, multi-word, and case-insensitive - the same matcher WezTerm, Zed, and Helix converged on: `scroll clear` finds Clear scrollback, `drac` finds Dracula, and a dropped letter (`scrllback`) still finds its row. Every keystroke re-scores the whole catalog: the top match is selected, Down/Up move, Enter or a click runs the selected row. Clearing the query returns the sectioned catalog with nothing selected, exactly as it opened.

## Dismissal and the chord

Four ways out: Escape, a click on the scrim outside the panel, the keyboard leaving the palette (a click on the terminal), or the open chord again - `Ctrl+Shift+P` is a toggle. The palette always closes before a picked action runs; nothing executes under the modal.

While the palette holds the keyboard, other app chords wait - typing filters, it never reaches the terminal. The chord is a normal bindable action (`command_palette`): rebind it in the [keybindings editor](keybindings.md#customizing) like any other, and the palette answers the chord your config holds, not the default. The default is the peers' `P`; the images panel rides `Ctrl+Shift+O`, and one chord is one action.

## Not in it (yet)

The palette runs no shell commands - no surveyed terminal's palette does. Your own rows are coming: the catalog reserves a slot for the plugins initiative's script commands, so a script will add palette entries the same way it adds [status segments](status-segments.md) today. The [research record](research/command-palette.md) carries the survey and that seam's shape.
