# DropTerm

A native, Yakuake/Guake-style drop-down terminal for the [COSMIC desktop](https://github.com/pop-os/cosmic-epoch) (Pop!_OS 24.04+, Wayland, cosmic-comp), written in Rust with [libcosmic](https://github.com/pop-os/libcosmic).

The terminal slides down from the top (or bottom) edge of the screen and is toggled with a global keyboard shortcut. It is a true layer-shell overlay - not a minimize/restore window trick - so it stays above other windows, is hidden from Alt-Tab and the dock, and keeps your shell session alive while hidden.

- App ID: `io.github.l1br3_io.CosmicExtDropterm`
- License: GPL-3.0-only (see [Credits](#credits))

## Status

This is a v1 / Milestone-1 implementation: a single terminal, no tabs. See [ROADMAP.md](ROADMAP.md) for what's done and what's planned.

## Building

### Build dependencies (Pop!_OS / Ubuntu)

```sh
sudo apt install just libxkbcommon-dev libwayland-dev libfontconfig-dev libfreetype-dev libexpat1-dev pkg-config
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

Prefer an unshifted key like `F12` over combinations like `Ctrl+Shift+<digit>` - see [Known risks](#known-risks).

### Development launcher

Don't point the shortcut at `cargo run`. COSMIC spawns a custom shortcut from
the compositor's working directory (not this repo), so cargo can't find
`Cargo.toml`, and every keypress would pay for a build check on top. Install a
launcher that always runs the current debug build instead:

```sh
just dev-install
```

That builds the debug profile and symlinks `~/.local/bin/cosmic-ext-dropterm`
to `target/debug/cosmic-ext-dropterm`. Set the shortcut's Command to the plain
`cosmic-ext-dropterm` once and leave it - `~/.local/bin` precedes `/usr/bin` on
the COSMIC session PATH, so it shadows a release copy installed with
`sudo just install`.

The edit-build-see loop is then one command:

```sh
just dev
```

`just dev` rebuilds, refreshes the launcher, kills any running instance and
starts the new binary. Killing first matters: a plain relaunch only toggles the
already-running process, which is still the code from before your edit.
`just dev-reload` does just the restart, and `just dev-install` just the build
plus launcher.

To skip the manual step, `just dev-watch` reruns that loop whenever a file
under `src/` (or `Cargo.toml`) changes:

```sh
just dev-watch
```

It needs [`cargo-watch`](https://github.com/watchexec/cargo-watch)
(`cargo install cargo-watch`). Each save rebuilds and restarts the overlay; a
failed build prints the compiler error, and the next save that compiles brings
it back.

### Running locally without a shortcut

No COSMIC shortcut configured yet? You can still open and dismiss the overlay from a local run:

- `just dev` builds, installs the launcher, and restart-launches the overlay; plain `cargo run` (or `just run`) just launches it.
- `Ctrl+Shift+Q` hides it.
- Clicking another window hides it too (`hide_on_focus_loss` defaults to `true`).
- Running the command a second time still toggles.

## Configuration

DropTerm uses `cosmic-config` and reloads its config live - no restart needed. The file lives at:

```
~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/
```

Each setting is its own RON file inside that directory (`height_percent`, `edge`, `layer`, `keyboard_mode`, `respect_panel`, `shell`, `args`, `font_name`, `font_size`, `color_scheme_dark`, `color_scheme_light`, `opacity`, `scrollback_lines`, `hide_on_focus_loss`). There is no settings UI yet (see the ROADMAP); edit the files directly, e.g.:

```sh
echo '60' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/height_percent
```

`hide_on_focus_loss` defaults to `true` (click away to hide); write `false` to keep the overlay open when it loses focus.

`respect_panel` defaults to `false`: the drop-down covers the panel and dock, Yakuake-style. Write `true` to place it inside the compositor's usable area instead - it sits flush below a top panel (or above a bottom dock, together with `edge`), while staying on its configured layer, so on the default `Overlay` layer it still appears above fullscreen windows. The setting applies on the next show:

```sh
echo true > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/respect_panel
```

With an autohide panel, the hidden panel still reserves its handle strip, so a respecting drop-down sits a few pixels below the screen edge.

## Features (v1)

- Layer-shell window anchored to the top (or bottom) edge, full width, above other windows
- Hidden from Alt-Tab and the dock
- Exclusive keyboard focus when shown; focus returns to the previous window when hidden
- Shell session persists while hidden (the PTY is never torn down - only the surface is)
- COSMIC theme integration: light/dark and the two built-in COSMIC color schemes
- Copy/paste (`Ctrl+Shift+C`/`V`), primary-selection paste (middle-click, `Shift+Insert`), scrollback, mouse reporting, `Ctrl`-click to open URLs
- Zoom (`Ctrl+`/`Ctrl-`/`Ctrl+0`), select-all and clear-scrollback shortcuts
- In-app dismiss: `Ctrl+Shift+Q` hides the overlay; clicking away hides it too

## Known risks

- **Hiding a layer surface**: DropTerm hides by *destroying* its layer surface and creates a fresh one on the next show, rather than unmapping/hiding it in place. This sidesteps a compositor-connection-drop bug some other layer-shell clients (via gtk4-layer-shell) have hit on cosmic-comp when a surface is updated after its role is destroyed ([cosmic-comp#2159](https://github.com/pop-os/cosmic-comp/issues/2159)) - libcosmic's own destroy path doesn't do that.
- **Custom shortcuts with Shift**: some Shift-modified custom shortcuts can fail to fire on COSMIC ([cosmic-epoch#2481](https://github.com/pop-os/cosmic-epoch/issues/2481)). Prefer an unshifted key such as `F12` for the toggle shortcut.
- **No GlobalShortcuts portal**: COSMIC doesn't yet implement the XDG `GlobalShortcuts` portal, so there is no way for DropTerm to register its own hotkey - it must be bound as a COSMIC custom shortcut (above).
- **No Flatpak packaging**: layer-shell and toplevel-management protocols aren't exposed to sandboxed (Flatpak) clients on cosmic-comp, so a true drop-down terminal isn't possible in a sandbox. This isn't planned until COSMIC exposes those protocols to sandboxes.

## Manual test checklist

Runtime behavior can only be exercised on real COSMIC hardware. After building, check:

- [ ] First launch shows the overlay at the configured edge, focused and ready for input
- [ ] The bound shortcut hides and re-shows it
- [ ] A background job (`sleep 100 &`, shell history) survives a hide/show cycle
- [ ] It does not appear in Alt-Tab or the dock
- [ ] Rapid repeated toggles (double key-press) don't crash or duplicate the surface
- [ ] It appears correctly above a fullscreen window
- [ ] `Ctrl+Shift+C` / `Ctrl+Shift+V` copy/paste, and middle-click / `Shift+Insert` paste the primary selection
- [ ] `Shift+PageUp`/`PageDown` scroll the scrollback
- [ ] Editing `height_percent` in the config live-resizes the surface on the next show
- [ ] Switching the system between light and dark updates the terminal colors
- [ ] On a multi-monitor setup, the surface appears on the output under the cursor
- [ ] `Ctrl+Shift+Q` hides the surface, and the session survives a re-toggle
- [ ] Clicking another window hides the surface (fresh config, `hide_on_focus_loss` default `true`)
- [ ] With `hide_on_focus_loss: false` written in config, clicking away does *not* hide
- [ ] Escape still reaches the shell (e.g. works in vim) and still cancels an active selection
- [ ] With `respect_panel: true` and a top panel, the panel stays fully visible and clickable while the drop-down is open, and the terminal sits flush below it
- [ ] With `respect_panel: true`, `edge: bottom`, and a bottom dock, the drop-down sits above the dock
- [ ] With `respect_panel: true`, the drop-down still appears above a fullscreen window
- [ ] With a config directory that has no `respect_panel` key (or `false` written), the drop-down still covers the panel
- [ ] Toggling `respect_panel` in the config while the drop-down is shown applies on the next show
- [ ] With an autohide panel and `respect_panel: true`, the drop-down lands below the panel's handle strip

## Non-goals

- Non-COSMIC compositors as a priority, and X11 support
- Flatpak packaging (until COSMIC exposes layer-shell/toplevel protocols to sandboxes)
- Built-in global hotkey registration (no GlobalShortcuts portal on COSMIC yet)
- Wrapping an external terminal emulator (may be reconsidered later; see the ROADMAP)

## Credits

DropTerm's terminal engine and widget are adapted from [cosmic-term](https://github.com/pop-os/cosmic-term) (Copyright System76, GPL-3.0-only), reused under the same license per the file-level attributions in `src/terminal/`. The single-instance/toggle pattern was cross-checked against [cosmic-ext-quake-terminal](https://github.com/M0Rf30/cosmic-ext-quake-terminal).
