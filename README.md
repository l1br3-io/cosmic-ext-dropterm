# DropTerm

A native, Yakuake/Guake-style drop-down terminal for the [COSMIC desktop](https://github.com/pop-os/cosmic-epoch) (Pop!_OS 24.04+, Wayland, cosmic-comp), written in Rust with [libcosmic](https://github.com/pop-os/libcosmic).

The terminal slides down from the top (or bottom) edge of the screen and is toggled with a global keyboard shortcut. It is a true layer-shell overlay - not a minimize/restore window trick - so it stays above other windows, is hidden from Alt-Tab and the dock, and keeps your shell session alive while hidden.

- App ID: `io.github.l1br3_io.CosmicExtDropterm`
- License: GPL-3.0-only (see [Credits](#credits))

## Status

This is a v1 + v2 implementation: tabs with session restore, drag-to-resize with remembered geometry, slide animation, an in-app settings drawer, built-in color schemes, and the in-app Application theme control. See [ROADMAP.md](ROADMAP.md) for what's done and what's planned.

## Building

### Build dependencies (Pop!_OS / Ubuntu)

```sh
sudo apt install just libxkbcommon-dev libwayland-dev libfontconfig-dev libfreetype-dev pkg-config
```

Rust via [rustup](https://rustup.rs/) (stable).

### Build and install

```sh
just build-release
sudo just install
```

`just uninstall` removes the installed files again.

## Setting up the shortcut

COSMIC has no XDG `GlobalShortcuts` portal, so DropTerm cannot register its own hotkey. Bind it to a COSMIC custom shortcut instead:

**COSMIC Settings → Keyboard → Keyboard Shortcuts → Custom Shortcuts → Add shortcut**

```
Command: cosmic-ext-dropterm
Key: F12
```

Running the command again toggles the terminal (single-instance: the second launch just tells the first one to show/hide, then exits). You can also bind separate shortcuts to `cosmic-ext-dropterm show` / `cosmic-ext-dropterm hide` if you want dedicated show/hide keys instead of a toggle.

For everything else the command line can do - scripting new tabs, the tab verbs, and `--is-visible` - see the [CLI reference](docs/cli.md).

Prefer an unshifted key like `F12` over combinations like `Ctrl+Shift+<digit>` - see [Known risks](docs/known-issues.md).

No COSMIC shortcut configured yet? You can still open and dismiss the overlay from a local run - see [Running locally without a shortcut](docs/development.md#running-locally-without-a-shortcut).

## Documentation

The details live in the `docs/` directory:

- [CLI reference](docs/cli.md) - every verb and flag, with full semantics
- [Configuration](docs/configuration.md) - all 25 config keys, profiles, and direct editing
- [Settings drawer](docs/settings.md) - the five sections and when each edit applies
- [Session persistence](docs/sessions.md) - what is saved, when, and how restore behaves
- [Theming](docs/theming.md) - the Application theme pin and the 12 built-in color schemes
- [Slide animation](docs/animation.md) - the 130 ms slide and its reversal rules
- [Bell notifications](docs/bell.md) - hidden-terminal bells and the per-tab latch
- [Known issues](docs/known-issues.md) - the rough edges, with upstream links
- [Development](docs/development.md) - the dev loop, running without a shortcut, and the manual test checklist

## Features (v1)

- Layer-shell window anchored to the top (or bottom) edge, full width, above other windows
- Hidden from Alt-Tab and the dock
- Exclusive keyboard focus when shown; focus returns to the previous window when hidden
- Shell session persists while hidden (the PTY is never torn down - only the surface is)
- COSMIC theme integration: light/dark and the built-in color schemes
- Copy/paste (`Ctrl+Shift+C`/`V`), primary-selection paste (middle-click, `Shift+Insert`), scrollback, mouse reporting, `Ctrl`-click to open URLs
- New tabs (`Ctrl+Shift+T`, the tab bar's `+`) inherit the active tab's working directory — `cd` once, spawn tabs where you already are; a failed read falls back to the pty's default directory. `Ctrl+Shift+H` instead opens the new tab in the home directory — settings compose exactly as for `Ctrl+Shift+T` (a default profile's own live cwd still applies), with home standing in for the inherited working directory
- Named profiles (`ssh web-prod` as a first-class tab): a profile's tab runs its command in its `cwd` — and, when it carries one, in its own color scheme (a built-in registry name), so prod reads red and local reads default at a glance, plus its own environment variables, merged over the inherited environment at spawn (`DROPTERM_PROFILE=prod` for rc files to read — spawn-time by nature, so an edit affects the next spawn); the context menu's Switch Profile folder respawns the active tab under any profile in place — `switch-profile-tab`'s one-click twin. Editing the scheme recolors its running tabs without a respawn, and every miss (unset, unknown name, deleted profile) falls back to the global pick
- Zoom (`Ctrl+`/`Ctrl-`/`Ctrl+0`), select-all and clear-scrollback shortcuts
- Tabs: open/close, duplicate (`Ctrl+Shift+D` or the context-menu row — a fresh shell in the active tab's working directory, right after it, that keeps a renamed tab's name), rename inline (`Ctrl+Shift+R`, double-click, or the context-menu row; a rename wins over every title mode), title modes including `Profile` — the tab bar labels each tab by the profile it runs, plain tabs keep their shell titles - reorder by drag or `Ctrl+Shift+PageUp`/`PageDown`, cycle with `Ctrl+PageUp`/`PageDown` or `Ctrl+Tab`, jump with `Ctrl+Shift+1..8` (`Ctrl+Shift+9` always takes the last tab) - cosmic-term's binds
- Tab attention markers: a tab that rang the bell wears `🔔` and one that produced output while inactive wears `•`, until you activate it (kitty's `bell_on_tab` + iTerm2-style activity dots)
- In-app dismiss: `Ctrl+Shift+Q` hides the overlay; clicking away hides it too
- Remote control from the CLI: `new-tab [--cwd DIR] [--cmd COMMAND]` opens a tab in the running instance over the single-instance D-Bus seam (Guake-style scripting without Guake's full surface)

## Known risks

A few known rough edges, each detailed in [Known issues](docs/known-issues.md):

- The clipboard survives hide/show cycles only via an invisible anchor window
- Hiding destroys the layer surface and re-creates it on the next show (a deliberate workaround)
- Shift-modified custom shortcuts can fail to fire on COSMIC
- No GlobalShortcuts portal: the toggle must be a COSMIC custom shortcut
- No Flatpak packaging (layer-shell isn't exposed to sandboxes)

## Non-goals

- Non-COSMIC compositors as a priority, and X11 support
- Flatpak packaging (until COSMIC exposes layer-shell/toplevel protocols to sandboxes)
- Built-in global hotkey registration (no GlobalShortcuts portal on COSMIC yet)
- Wrapping an external terminal emulator (may be reconsidered later; see the ROADMAP)

## Credits

DropTerm's terminal engine and widget are adapted from [cosmic-term](https://github.com/pop-os/cosmic-term) (Copyright System76, GPL-3.0-only), reused under the same license per the file-level attributions in `src/terminal/`. The single-instance/toggle pattern was cross-checked against [cosmic-ext-quake-terminal](https://github.com/M0Rf30/cosmic-ext-quake-terminal).
