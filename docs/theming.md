# Theming

How DropTerm picks its theme mode and color scheme - the Application theme dropdown and the two per-mode scheme picks.

## Application theme

Which theme mode is active - and with it which of the two color schemes wears, the window background derivation, and all chrome - comes from the "Application theme" dropdown in the Appearance section of the [settings drawer](settings.md), so you don't need COSMIC Appearance to flip the terminal's mode:

- `Follow system` (the default): DropTerm reads the OS light/dark preference through the desktop portal's `color-scheme` setting - any compliant desktop, COSMIC included - and follows it live: an OS flip re-themes the terminal's scheme, background, chrome, and the settings drawer itself without a restart. When the preference is unreadable (no portal, no preference set, or a read error), DropTerm resolves dark.
- `Dark` / `Light`: the app pins that mode independently of the system. Everything resolves from the pin - scheme selection, window background, tab/grab-bar chrome, and the settings drawer - and the pin survives OS flips: the desktop can flip back and forth while the terminal keeps wearing the pinned mode.
- Switching back to `Follow system` re-derives the current system mode on the spot, and system flips resume driving the app.

Picks apply live (config key `theme_preference`: `System`, `Dark`, or `Light` - a missing key loads as `System`). With the settings drawer open the terminal is right there beneath it, so a theme pick is visible immediately - the terminal's cells, background, and chrome restyle in the same pass, and the drawer restyles along with them.

## Color schemes

The terminal wears a built-in color scheme picked per theme mode: the Appearance card of the [settings drawer](settings.md) has two scheme pickers - "Dark color scheme" (worn while the app resolves dark mode) and "Light color scheme" (while it resolves light). The palette is the preview: the closed control shows the configured scheme's name over a strip of its actual colors, and opening it lists every scheme as its name plus swatches - the background, the foreground, then the sixteen ANSI colors the terminal renders with. Those eighteen colors are literally everything a scheme ever shows, so there is no name-as-proxy and no separate preview pane; the chips resolve through the same conversion the terminal renders with, so a chip can never disagree with the live scheme. A row pick applies live, Escape or a click away closes without picking, and the picked row is marked. The active mode comes from the [Application theme](#application-theme) dropdown - `Follow system` tracks the OS preference through the portal, `Dark`/`Light` pin one - and a mode change from either side switches between the two picks; a config edit or a mode change re-applies live.

Twelve built-in schemes ship, all community standards that render the same on any platform: `GitHub Dark` and `GitHub Light` (the defaults, transcribed from GitHub's Primer palettes - the anchors and ANSI table `primer/github-vscode-theme` itself publishes), then the ten third-party schemes - `Catppuccin Latte`, `Catppuccin Mocha`, `Dracula`, `Gruvbox Dark`, `Gruvbox Light`, `Nord`, `Solarized Dark`, `Solarized Light`, `Tokyo Night Day`, and `Tokyo Night Storm` - values transcribed from their canonical upstream palettes, with the source and license recorded per scheme in `dropterm/src/core/theme_source.rs`. The stock COSMIC Dark/Light pair left the registry (2026-11-20): their `None` background deferred to the host system theme - OS coupling a cross-platform picker does not carry. A config still storing `COSMIC Dark`/`COSMIC Light` resolves like any other unknown name (below).

- Every scheme owns its background: picking Dracula renders Dracula's `#282a36` behind the cells, and the chrome - tab bar, grab bar - follows the scheme background, so cells and chrome cannot disagree, and nothing tracks the host system's palette. `opacity` multiplies on top of the background.
- A scheme's name is the config format: a pick persists the display name under `color_scheme_dark`/`color_scheme_light`. A name not in the registry (stale, hand-typed, or renamed upstream) silently falls back to the GitHub pair for that mode - no error, no migration - and the picker marks no row until you pick again (the closed control still shows the stored name, honest about what the file says).
- Dim variants are derived, not shipped: each normal color's channels are scaled by 0.66 (Alacritty's dim rule), one uniform rule for every scheme. Bright variants come from the palettes themselves.

## User-defined scheme files

Your own schemes load from the config dir's `themes/` subdirectory (`~/.config/io.github.l1br3_io.dropterm/themes/` on an installed build; the dev identity uses its own dir): one RON file per scheme, named by its file stem - `Evening.ron` is the scheme "Evening". Both per-mode pickers and the profile form's scheme select list user schemes behind the twelve built-ins, preview strips and all; a pick persists and applies exactly like a built-in's, per profile too.

The format is the same field set the built-ins carry, every slot optional except the two anchors - `foreground` and `background` are required, everything left out derives (dims by Alacritty's rule, unset ANSI slots by the conversion's own fallbacks). Colors are `#RRGGBB` string literals:

```ron
(
    foreground: "#e6edf3",
    background: "#1a1b26",
    cursor: "#c0caf5",            // optional, like every slot below
    normal: (
        black: "#15161e",
        red: "#f7768e",
        green: "#9ece6a",
        yellow: "#e0af68",
        blue: "#7aa2f7",
        magenta: "#bb9af7",
        cyan: "#7dcfff",
        white: "#a9b1d6",
    ),
    bright: (
        black: "#414868",
        red: "#ff7a93",
        green: "#b9f27c",
        yellow: "#ff9e64",
        blue: "#7da6ff",
        magenta: "#bb9af7",
        cyan: "#0db9d7",
        white: "#c0caf5",
    ),
    // left-out slots derive: `bright_foreground`, `dim_foreground`,
    // and a `dim: ()` table all mean "derive it" (`None` also parses,
    // same meaning).
)
```

The minimum viable file is just `foreground` and `background`; a snippet of the shape above with only the anchors and the `normal` table is already a complete scheme.

The rules around the dir:

- **Merged, never overriding**: user schemes list behind the built-ins; a file naming a built-in ("Nord.ron") is skipped with one log line - the built-in wins. Two files resolving to one name keep the first in file-name sorted order, also one line.
- **Skip-and-log, never block**: a file that is not valid RON, holds a color that is not `#RRGGBB`, or misses an anchor is skipped with one `dropterm:` line naming the reason; every valid sibling still loads. A missing or empty `themes/` dir is the silent first run - zero schemes, no output.
- **Live**: a dropped, edited, or deleted file lands on the config-echo pass (the dir is watched); an edited worn scheme re-themes, a deleted worn scheme falls back to the GitHub pair for the mode while the config keeps storing its name - exactly the unknown-built-in-name rule above. The settings drawer's pickers read the list when the drawer opens, so a change made while it is open appears on reopen.
- **Yours, untouched**: DropTerm never writes, moves, renames, or quarantines anything under `themes/` - the files are your authored config, not DropTerm's store.
