# Theming

How DropTerm picks its theme mode and color scheme - the Application theme dropdown and the two per-mode scheme picks.

## Application theme

Which theme mode is active - and with it which of the two color schemes wears, the window background derivation, and all chrome - comes from the "Application theme" dropdown in the Appearance section of the [settings drawer](settings.md), so you don't need COSMIC Appearance to flip the terminal's mode:

- `Follow system` (the default): DropTerm tracks the system light/dark mode. Flipping COSMIC Appearance switches the terminal's scheme, background, chrome, and the settings drawer itself, exactly as before - a config that never touches this key behaves byte-identically to a build without the dropdown.
- `Dark` / `Light`: the app pins that mode independently of the system. Everything resolves from the pin - scheme selection, window background, tab/grab-bar chrome, and the settings drawer - built on your own system palettes (accent included), and the pin survives system theme flips: COSMIC Appearance can flip back and forth while the terminal keeps wearing the pinned mode. One upstream edge: while pinned, palette edits made while the system sits in the opposite mode are dropped until the next re-pin or theme change.
- Switching back to `Follow system` re-derives the current system mode on the spot, and system flips resume driving the app.

Picks apply live (config key `theme_preference`: `System`, `Dark`, or `Light` - a missing key loads as `System`). With the settings drawer open the terminal is right there beneath it, so a theme pick is visible immediately - the terminal's cells, background, and chrome restyle in the same pass, and the drawer restyles along with them.

## Color schemes

The terminal wears a built-in color scheme picked per theme mode: the Appearance section of the [settings drawer](settings.md) has two dropdowns - "Dark color scheme" (worn while the app resolves dark mode) and "Light color scheme" (while it resolves light). The active mode comes from the [Application theme](#application-theme) dropdown - `Follow system` tracks COSMIC Appearance, `Dark`/`Light` pin one - and a mode change from either side switches between the two picks; a config edit or a mode change re-applies live.

Twelve built-in schemes ship: `COSMIC Dark` and `COSMIC Light` (the defaults), `Dracula`, `Solarized Dark`, `Solarized Light`, `Gruvbox Dark`, `Gruvbox Light`, `Tokyo Night Storm`, `Tokyo Night Day`, `Nord`, `Catppuccin Mocha`, and `Catppuccin Latte` - values transcribed from their canonical upstream palettes, with the source and license recorded per scheme in `src/terminal/theme_source.rs`.

- Every scheme except the COSMIC pair owns its background: picking Dracula renders Dracula's `#282a36` behind the cells, and the chrome - tab bar, grab bar - follows the scheme background, so cells and chrome cannot disagree. The COSMIC pair keeps following the system theme's background, as always. `opacity` multiplies on top of either background.
- A scheme's name is the config format: a pick persists the display name under `color_scheme_dark`/`color_scheme_light`. A name not in the registry (stale, hand-typed, or renamed upstream) silently falls back to the COSMIC pair for that mode - no error, no migration - and the dropdown shows no selection until you pick again.
- Dim variants are derived, not shipped: each normal color's channels are scaled by 0.66 (Alacritty's dim rule), one uniform rule for every non-COSMIC scheme. Bright variants come from the palettes themselves.
- User-defined scheme files are not supported; the built-in registry is the whole feature (see the ROADMAP's "Later" section).
