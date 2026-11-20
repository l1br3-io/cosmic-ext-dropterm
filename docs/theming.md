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
- User-defined scheme files are not supported; the built-in registry is the whole feature (see the ROADMAP's "Later" section).
