# DropTerm

A native, Yakuake/Guake-style drop-down terminal for the [COSMIC desktop](https://github.com/pop-os/cosmic-epoch) (Pop!_OS 24.04+, Wayland, cosmic-comp), written in Rust with [libcosmic](https://github.com/pop-os/libcosmic).

The terminal slides down from the top (or bottom) edge of the screen and is toggled with a global keyboard shortcut. It is a true layer-shell overlay - not a minimize/restore window trick - so it stays above other windows, is hidden from Alt-Tab and the dock, and keeps your shell session alive while hidden.

- App ID: `io.github.l1br3_io.CosmicExtDropterm`
- License: GPL-3.0-only (see [Credits](#credits))

## Installing

### From a release

Grab the tarball for your architecture from the
[v0.1.0 release](https://github.com/l1br3-io/cosmic-ext-dropterm/releases/tag/v0.1.0) — it carries both binaries (terminal and [panel applet](docs/applet.md)) plus the desktop files, icons, and metainfo as a `usr/` tree — then verify and copy it into place:

```sh
curl -LO https://github.com/l1br3-io/cosmic-ext-dropterm/releases/download/v0.1.0/cosmic-ext-dropterm-0.1.0-x86_64.tar.gz
curl -LO https://github.com/l1br3-io/cosmic-ext-dropterm/releases/download/v0.1.0/sha256sums
sha256sum -c --ignore-missing sha256sums
tar -xzf cosmic-ext-dropterm-0.1.0-x86_64.tar.gz
sudo cp -a usr/. /usr/
```

ARM machines take the `cosmic-ext-dropterm-0.1.0-aarch64.tar.gz` asset instead. The release also ships `vendor.tar`, the complete dependency snapshot (git-pinned libcosmic included) for `just build-vendored` offline rebuilds. To uninstall, remove the two `cosmic-ext-dropterm*` binaries from `/usr/bin` and the `io.github.l1br3_io.CosmicExtDropterm*` files under `/usr/share/{applications,icons,metainfo}/` — or, from a source checkout, `sudo just uninstall`.

### From source

See [Building](#building) below.

## Status

DropTerm v0.1.0 is released — see [Installing](#installing). Development continues toward the next release on the [roadmap](ROADMAP.md)'s v3 and Later lines; the feature list below describes the current main build.

## Building

### Build dependencies (Pop!_OS / Ubuntu)

```sh
sudo apt install just libxkbcommon-dev libwayland-dev libfontconfig-dev libfreetype-dev pkg-config
```

Rust via [rustup](https://rustup.rs/) (stable).

### Build and install

```sh
just build-all        # both release binaries: terminal + applet
sudo just install
```

`sudo just uninstall` removes the installed files again.

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
- [Configuration](docs/configuration.md) - all 43 config keys, profiles, and direct editing
- [Keybindings](docs/keybindings.md) - every default chord and how to rebind
- [Settings drawer](docs/settings.md) - the five sections and when each edit applies
- [Status bar](docs/status-bar.md) - the 28 px glance band: segments, click-to-copy, selection, its menu
- [Panel applet](docs/applet.md) - toggling DropTerm and reading its state from the COSMIC panel
- [Session persistence](docs/sessions.md) - what is saved, when, and how restore behaves
- [Theming](docs/theming.md) - the Application theme pin and the 12 built-in color schemes
- [Slide animation](docs/animation.md) - the 130 ms slide and its reversal rules
- [Bell notifications](docs/bell.md) - hidden-terminal bells and the per-tab latch
- [Known issues](docs/known-issues.md) - the rough edges, with upstream links
- [Research & decision log](docs/research/README.md) - why each non-trivial feature looks the way it does, with the rejected alternatives
- [Development](docs/development.md) - the dev loop and running without a shortcut
- [Testing](docs/testing.md) - the manual test checklist every change answers to
- [Architecture](docs/architecture.md) - the module map and the load-bearing seams

## Features

- **The drop-down core** - a layer-shell window anchored to the top (or bottom) edge, full width, above other windows and hidden from Alt-Tab and the dock; exclusive keyboard focus while shown, with focus returned to the previous window on hide; the shell session stays alive while hidden (the PTY is never torn down - only the surface). COSMIC theme integration ([theming](docs/theming.md)), copy/paste with primary-selection paste, scrollback, mouse reporting, `Ctrl`-click to open URLs, zoom, select-all and clear-scrollback.
- **Tabs and sessions** - open/close, duplicate, inline rename, drag reorder, cycle and jump chords ([keybindings](docs/keybindings.md)), and title modes including by-profile ([configuration](docs/configuration.md#tab-titles)); attention markers for bells and background output; sessions save and restore across shutdowns ([sessions](docs/sessions.md)); drag-to-resize with remembered geometry; the 130 ms slide ([animation](docs/animation.md)); and the in-app [settings drawer](docs/settings.md) with its five tabs.
- **Profiles** - named presets like `ssh web-prod` as a first-class tab: each carries its own command, working directory, color scheme, font size, scrollback, and environment variables, and the context menu's Switch Profile respawns any tab under another profile in place ([configuration](docs/configuration.md#profiles)). New tabs inherit the active tab's working directory, with a configurable default directory and editable spawn defaults.
- **The status band** - a 28 px strip beside the grab bar narrating the active terminal: foreground process, working directory with git branch, profile, cell dimensions, tab position, zoom delta, and clock - every segment click-to-copy, each one individually toggleable ([status bar](docs/status-bar.md)).
- **Desktop integration** - the per-tab context menu with pin/unpin; a close-confirmation guardrail for tabs running foreground processes; desktop notifications when the bell rings while hidden ([bell](docs/bell.md)); remote control from the CLI - `new-tab` and the tab verbs over the single-instance D-Bus seam ([CLI reference](docs/cli.md)); and a COSMIC [panel applet](docs/applet.md) that toggles the drop-down and shows its running state from the panel.

## Known risks

A few known rough edges — the clipboard's invisible anchor window, the surface recreate-on-hide workaround, Shift-modified custom shortcuts failing to fire, no GlobalShortcuts portal (the toggle must be a COSMIC custom shortcut), and no main-app Flatpak — each detailed in [Known issues](docs/known-issues.md).

## Non-goals

- Non-COSMIC compositors as a priority, and X11 support
- Flatpak packaging (until COSMIC exposes layer-shell/toplevel protocols to sandboxes)
- Built-in global hotkey registration (no GlobalShortcuts portal on COSMIC yet)
- Wrapping an external terminal emulator (may be reconsidered later; see the ROADMAP)

## Credits

DropTerm's terminal engine and widget are adapted from [cosmic-term](https://github.com/pop-os/cosmic-term) (Copyright System76, GPL-3.0-only), reused under the same license per the file-level attributions in `src/terminal/`. The single-instance/toggle pattern was cross-checked against [cosmic-ext-quake-terminal](https://github.com/M0Rf30/cosmic-ext-quake-terminal).
