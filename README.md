# DropTerm

A native, Yakuake/Guake-style cross-platform drop-down terminal, written in Rust on [GPUI](https://github.com/zed-industries/gpui).

The terminal slides down from the top (or bottom) edge of the screen and is toggled with a global keyboard shortcut. It is a true layer-shell overlay - not a minimize/restore window trick - so it stays above other windows, is hidden from Alt-Tab and the dock, and keeps your shell session alive while hidden.

- App ID: `io.github.l1br3_io.dropterm`
- License: GPL-3.0-only (see [Credits](#credits))

## Installing

### From a release

Grab the tarball for your architecture from the
[v0.5.0 release](https://github.com/l1br3-io/dropterm/releases/tag/v0.5.0) — it carries the binary plus the desktop file, icon, and metainfo as a `usr/` tree — then verify and copy it into place:

```sh
curl -LO https://github.com/l1br3-io/dropterm/releases/download/v0.5.0/dropterm-0.5.0-x86_64.tar.gz
curl -LO https://github.com/l1br3-io/dropterm/releases/download/v0.5.0/sha256sums
sha256sum -c --ignore-missing sha256sums
tar -xzf dropterm-0.5.0-x86_64.tar.gz
sudo cp -a usr/. /usr/
```

ARM machines take the `dropterm-0.5.0-aarch64.tar.gz` asset instead. The release also ships `vendor.tar`, the complete dependency snapshot (git-pinned libcosmic included) for `just build-vendored` offline rebuilds. To uninstall, remove the two `dropterm*` binaries from `/usr/bin` and the `io.github.l1br3_io.dropterm*` files under `/usr/share/{applications,icons,metainfo}/` — or, from a source checkout, `sudo just uninstall`.

### From source

See [Building](#building) below.

## Status

DropTerm v0.5.0 is released — see [Installing](#installing); [release notes](docs/releases.md) curate what each version changed. Development continues toward the next release on the [roadmap](ROADMAP.md)'s V4 and Later lines; the feature list below describes the current main build.

## Building

### Build dependencies

Nothing to install by hand: the build and install recipes run a prerequisite gate first (`just deps` runs it standalone) — it checks the C toolchain, `pkg-config`, the system libraries, and cargo, and installs whatever is missing, through apt, dnf, pacman, or zypper. Rust comes via [rustup](https://rustup.rs/) (stable, minimal profile) when `cargo` is absent — never as root, so `sudo just install` never writes a root-owned toolchain. With everything already present the gate is a silent no-op that touches neither `sudo` nor the network, so CI and packaging environments are unaffected. Only `just` itself must be present up front — the official installer at [just.systems](https://just.systems/) is the reliable source (Ubuntu 24.04's apt `just` predates this justfile's `set quiet`).

The manual equivalent on Pop!_OS / Ubuntu, for reference:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev libfontconfig-dev libfreetype-dev libexpat1-dev
```

### Build and install

```sh
just build-all        # the release binary
sudo just install
```

`sudo just uninstall` removes the installed files again.

## Setting up the shortcut

COSMIC has no XDG `GlobalShortcuts` portal, so DropTerm cannot register its own hotkey. Bind it to a COSMIC custom shortcut instead:

**COSMIC Settings → Keyboard → Keyboard Shortcuts → Custom Shortcuts → Add shortcut**

```
Command: dropterm
Key: F12
```

On COSMIC, DropTerm can do this for you: open settings and the Keybindings tab's **Global** group (or answer the first-run onboarding), take **Configure for me**, and press **Use F12** - or choose your own chord. DropTerm writes the custom shortcut itself (COSMIC Settings lists it as "DropTerm"), and the Global row's **Remove the key DropTerm set** undoes exactly that entry later.

Running the command again toggles the terminal (single-instance: the second launch just tells the first one to show/hide, then exits). You can also bind separate shortcuts to `dropterm show` / `dropterm hide` if you want dedicated show/hide keys instead of a toggle. On desktops that host status icons there is a tray icon too: left-click toggles the dropdown, and its menu adds Help and Quit.

The other layer-shell desktops bind the same command through their own settings. DropTerm never installs or seeds a shortcut on any desktop - the bind is always yours to make:

- **sway**: `bindsym F12 exec dropterm` in `~/.config/sway/config`
- **Hyprland**: `bind = , F12, exec, dropterm` in `~/.config/hypr/hyprland.conf`
- **KDE Plasma**: System Settings → Keyboard → Shortcuts → Add New → Command or Script, command `dropterm`, then assign it a key

For everything else the command line can do - scripting new tabs, the tab verbs, and `--is-visible` - see the [CLI reference](docs/cli.md).

Prefer an unshifted key like `F12` over combinations like `Ctrl+Shift+<digit>` - see [Known risks](docs/known-issues.md).

No COSMIC shortcut configured yet? You can still open and dismiss the overlay from a local run - see [Running locally without a shortcut](docs/development.md#running-locally-without-a-shortcut).

## Documentation

The details live in the `docs/` directory:

- [Common tasks](docs/howto.md) - the task-shaped entry point: the exact key or verb for what you want to do, one hop from the detail
- [CLI reference](docs/cli.md) - every verb and flag, with full semantics
- [The D-Bus contract](docs/dbus.md) - the V1 surface for integrators: the address, both interfaces' members, the CLI verbs' wire encoding, and the stability promise
- [Configuration](docs/configuration.md) - all 54 config keys, profiles, and direct editing
- [Keybindings](docs/keybindings.md) - every default chord and how to rebind
- [Command palette](docs/command-palette.md) - `Ctrl+Shift+P` over every app verb: fuzzy search, every action one keystroke away
- [Settings window](docs/settings.md) - the nine pages and when each edit applies
- [Status bar](docs/status-bar.md) - the 28 px glance band: segments, click-to-copy, selection, its menu
- [Session persistence](docs/sessions.md) - what is saved, when, and how restore behaves
- [Theming](docs/theming.md) - the Application theme pin, the 12 built-in color schemes, and your own scheme files under `~/.config/io.github.l1br3_io.dropterm/themes/`
- [Plugins](docs/plugins.md) - your own Lua 5.4 scripts under `~/.config/io.github.l1br3_io.dropterm/plugins/`: the `dropterm.on`/`emit` event bus, hot reload, timeouts, and the full-trust model stated plainly
- [Script status segments](docs/status-segments.md) - plugins' own band segments: `dropterm.status_segment`, the ctx table it receives, the refresh triggers, and the `status_script_segments` ordering key
- [Script tab titles](docs/tab-titles.md) - plugins deriving the tabs' names: `dropterm.tab_title`, the `{ tab, title }` ctx, the change-gated asks, and the stale ladder; a manual rename still wins in every mode
- [Script commands](docs/script-commands.md) - plugins registering keybindable commands: `dropterm.command` rows beside the built-ins in the Keybindings editor, fire-and-forget dispatch with `{ tab }`, and `dropterm.send_text` as v1's one host verb
- [Styling](docs/styling.md) - the CSS-convention style engine: quoted-kebab declarations, compound selectors, the specificity cascade over the whole surface, and the two sources (manifest `styles`, the `dropterm.styles` verb)
- [Styling](docs/styling.md) - the CSS-convention style engine: quoted-kebab declarations, compound selectors, the specificity cascade over the whole surface, and the two sources (manifest `styles`, the `dropterm.styles` verb)
- [Script dialogs and panel views](docs/plugins.md) - the typed-slots surfaces: `dropterm.dialog` opens a host-owned modal and presses return as `slot-event`s, `dropterm.panel_view` docks a titled view into the right rail (`panel_rail_enabled`), and [the panel containers](docs/panel-containers.md) are the shell side
- [Slide animation](docs/animation.md) - the 130 ms slide and its reversal rules
- [Bell notifications](docs/bell.md) - hidden-terminal bells and the per-tab latch
- [Autosuggestions](docs/autosuggestions.md) - fish-style ghost text from the tab's own history; Right accepts, Alt+Right takes a word
- [Inline images](docs/images.md) - `imgcat` and OSC 1337: images render in the terminal and scroll away with the output
- [FAQ](docs/faq.md) - the recurring questions, answered directly
- [Known issues](docs/known-issues.md) - the rough edges, with upstream links
- [Memory diagnostics](docs/memory-diagnostics.md) - the always-on RAM log: where it lives, how to read the curve, and the capture ladder for the day it climbs
- [Release notes](docs/releases.md) - what each tagged release changed, curated and linked
- [Research & decision log](docs/research/README.md) - why each non-trivial feature looks the way it does, with the rejected alternatives
- [Development](docs/development.md) - the dev loop and running without a shortcut
- [Testing](docs/testing.md) - the manual test checklist every change answers to
- [Architecture](docs/architecture.md) - the module map and the load-bearing seams

## Features

- **The drop-down core** - a layer-shell window anchored to the top (or bottom) edge, full width, above other windows and hidden from Alt-Tab and the dock; exclusive keyboard focus while shown, with focus returned to the previous window on hide; the shell session stays alive while hidden (the PTY is never torn down - only the surface). COSMIC theme integration ([theming](docs/theming.md)), copy/paste with primary-selection paste, scrollback, mouse reporting, `Ctrl`-click to open URLs, zoom, select-all and clear-scrollback.
- **Tabs and sessions** - open/close, duplicate, inline rename, drag reorder, cycle and jump chords ([keybindings](docs/keybindings.md)), and title modes including by-profile ([configuration](docs/configuration.md#tab-titles)); attention markers for bells and background output; sessions save and restore across shutdowns ([sessions](docs/sessions.md)); drag-to-resize with remembered geometry; the 130 ms slide ([animation](docs/animation.md)); and the [Settings window](docs/settings.md) with its nine-page sidebar.
- **Profiles** - named presets like `ssh web-prod` as a first-class tab: each carries its own command, working directory, color scheme, font size, scrollback, and environment variables, and the context menu's Switch Profile respawns any tab under another profile in place ([configuration](docs/configuration.md#profiles)). New tabs inherit the active tab's working directory, with a configurable default directory and editable spawn defaults.
- **The status band** - a 28 px strip beside the grab bar narrating the active terminal: foreground process, working directory with git branch, profile, cell dimensions, tab position, zoom delta, and clock - every segment click-to-copy, each one individually toggleable ([status bar](docs/status-bar.md)).
- **Desktop integration** - the per-tab context menu with pin/unpin; a close-confirmation guardrail for tabs running foreground processes; desktop notifications when the bell rings while hidden ([bell](docs/bell.md)); remote control from the CLI - `new-tab` and the tab verbs over the single-instance D-Bus seam ([CLI reference](docs/cli.md)); a status-area tray icon on desktops that host one (left-click toggles the dropdown, the menu carries Help/About and a first-class Quit; a session without an SNI host simply shows no icon - nothing else changes); and desktop integration end to end.

## Known risks

A few known rough edges — the surface recreate-on-hide workaround, Shift-modified custom shortcuts failing to fire, no GlobalShortcuts portal (the toggle must be a COSMIC custom shortcut), and no main-app Flatpak — each detailed in [Known issues](docs/known-issues.md).

## Non-goals

- Flatpak packaging (COSMIC does not expose layer-shell/toplevel protocols to sandboxed clients)
- Built-in global hotkey registration (no GlobalShortcuts portal on COSMIC yet)
- Wrapping an external terminal emulator (may be reconsidered later; see the ROADMAP)

## Contributing

The dev loop, the verification bar, and the module map: [Development](docs/development.md), [Testing](docs/testing.md), [Architecture](docs/architecture.md). Runtime behavior is only testable on real COSMIC hardware, so the manual checklist is the contract every change answers to.

## Credits

DropTerm's terminal engine is adapted from [cosmic-term](https://github.com/pop-os/cosmic-term) (Copyright System76, GPL-3.0-only), reused under the same license per the file-level attributions in `dropterm/src/core/terminal/`. The single-instance/toggle pattern was cross-checked against [cosmic-ext-quake-terminal](https://github.com/M0Rf30/cosmic-ext-quake-terminal).
