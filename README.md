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

Each setting is its own RON file inside that directory (`height_percent`, `width_percent`, `h_align`, `edge`, `layer`, `keyboard_mode`, `shell`, `args`, `font_name`, `font_size`, `color_scheme_dark`, `color_scheme_light`, `opacity`, `scrollback_lines`, `hide_on_focus_loss`). There is no settings UI yet (see the ROADMAP); edit the files directly, e.g.:

```sh
echo '60' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/height_percent
echo 'Left' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/h_align
```

Geometry settings reshape the surface the moment they change - a shown surface is reshaped in place (no destroy/recreate), a hidden one picks up the new geometry on its next show:

- `height_percent` (default `40`, range 10-100): surface height as a percentage of the output height. Also written automatically shortly after a grab-bar drag settles.
- `width_percent` (default `100`, range 30-100): surface width as a percentage of the output width. `100` reproduces the original full-width surface exactly.
- `h_align` (`Left`, `Center`, or `Right`; default `Center`): horizontal placement of the surface within the output. `Left` and `Right` sit flush against that edge; `Center` splits the leftover space into equal left and right margins. Only meaningful below `width_percent` 100.

`hide_on_focus_loss` defaults to `true` (click away to hide); write `false` to keep the overlay open when it loses focus.

## Session persistence

DropTerm reopens your tab layout on every launch: one fresh shell per saved tab, spawned in its saved working directory, with its title, and the tab that was focused when the session was saved is focused again. This is always on - there is no toggle (one may come with the settings UI).

What is saved - deliberately the same scope Guake and Yakuake settle on:

- The tab list in tab-bar order: per-tab `title`, a user rename (`title_override`) if any, the `shell` recorded when the tab was spawned, and the tab's `cwd` at save time
- The focused tab
- A schema `version`

That is all - seven schema fields (`version`, `active`, `tabs`, and per tab `title`, `title_override`, `shell`, `cwd`). Scrollback, environment, and running processes are deliberately *not* saved: a restored tab is a fresh shell in a familiar place, never a resumed program. And because each tab's shell is recorded at spawn, changing the configured `shell` mid-run only affects newly opened tabs.

Saving happens on every layout change (open, close, reorder, rename, tab switch, or a shell's `cd`), debounced to one write within half a second; continuous changes coalesce into one write per interval. Each write is atomic - serialize to a temp file next to the target, fsync, rename over the old file - so a crash mid-write leaves the previous session intact. The trade-off for debouncing: a hard kill can lose the last ~500 ms of layout changes.

The state lives in its own file, fully separate from the config above (the directory is created on first save):

```
~/.local/state/io.github.l1br3_io.CosmicExtDropterm/session.ron
```

(`$XDG_STATE_HOME` is honored when set to an absolute path.)

Failure behavior:

- First run (no state file): silent - a single fresh tab, no warnings.
- A corrupt, unreadable, or future-versioned file is moved to `session.ron.bak` (replacing any older backup), logged at `warn`, and the app starts fresh. Rename it back to `session.ron` to give it another chance.
- A saved `cwd` that no longer exists spawns that shell in `$HOME`.

## Slide animation

Showing and hiding slide the whole content column - terminal, tab bar when one is shown, grab bar - in from, and back out toward, the anchored screen edge over about 130 ms: an ease-out curve on the way in, ease-in on the way out, pure translation with no fade. This is always on - there is no config key for it yet (one may come with the settings UI) - and it is deliberately cheap: the surface's geometry never changes during the animation, only the content's painted offset does, so the compositor sees no resize traffic. A toggle arriving mid-animation reverses the slide from wherever the content stands - nothing teleports, nothing is dropped or queued - and PTYs keep running throughout: keyboard focus returns to the previously focused window the moment a hide *starts*, and the surface is destroyed only after the slide-out completes. A hide that catches the surface still measuring (a toggle within the show's measuring window) skips the animation and destroys immediately.

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
- [ ] Editing `height_percent` in the config reshapes a shown surface immediately, in place - and a hidden surface opens at the new height on its next show
- [ ] `width_percent` below 100 (e.g. 60) sizes the surface to that fraction of the output width; the default 100 is indistinguishable from the full-width surface
- [ ] `h_align = Left` / `Right` sits flush against that edge, `Center` centers the surface with equal left/right margins - on both `edge` values
- [ ] Editing `width_percent`/`h_align`/`edge` while shown also reshapes in place (width 60 → 100 clears the margins and refills); non-geometry edits (font, colors) leave the surface untouched
- [ ] A visible grab bar renders on the free edge, outside the tab bar when one is shown; dragging it resizes the height live, and only the height - width and alignment stay put, including on a narrower-than-full-width surface
- [ ] Dragging respects the 10-100 % height band; with `edge = bottom`, dragging away from the screen edge grows the terminal
- [ ] After a drag is released the new height applies immediately (a hide/show right away opens at the new height) and `height_percent` is written to the config file once, about 400 ms later - no write/reshape storm, and rapid re-drags coalesce into one write
- [ ] With default config, the only visible change from the previous build is the grab bar; tab bar placement is unchanged (from the screen edge: content, tab bar when more than one tab is open, grab bar)
- [ ] Switching the system between light and dark updates the terminal colors
- [ ] On a multi-monitor setup, the surface appears on the output under the cursor
- [ ] `Ctrl+Shift+Q` hides the surface, and the session survives a re-toggle
- [ ] Clicking another window hides the surface (fresh config, `hide_on_focus_loss` default `true`)
- [ ] With `hide_on_focus_loss: false` written in config, clicking away does *not* hide
- [ ] Escape still reaches the shell (e.g. works in vim) and still cancels an active selection
- [ ] Arrange several tabs (distinct titles and cwds, one renamed, a middle tab focused), quit, relaunch: the same tabs return in the same order, each a fresh shell in its saved cwd, the rename intact, the same tab focused
- [ ] A saved cwd that no longer exists (delete the directory, or hand-edit the file): that shell respawns in `$HOME`
- [ ] Open/close/reorder/rename/switch tabs and `cd` around: `session.ron` updates within ~500 ms of the first change, at most one write per interval under continuous change (watch the state dir + logs)
- [ ] `kill -9` mid-write at any point: the previous session file is intact, no `session.ron.tmp` litter, and the next launch restores the pre-kill layout
- [ ] No `session.ron` (first ever run, or deleted): a single fresh tab, no warnings
- [ ] Hand-corrupt `session.ron` (truncate or garble it), relaunch: the file becomes `session.ron.bak`, a `warn` is logged, and the app starts fresh
- [ ] Hand-edit `session.ron` to `version: 2`, relaunch: same quarantine (`.bak` + fresh start) - a newer schema is never guessed at
- [ ] Restore spawns exactly the saved number of tabs - no default tab appears alongside them
- [ ] Right after a relaunch with otherwise idle tabs, the session file is not rewritten (a write soon after restore is still legitimate when restored shells report new titles)
- [ ] With several tabs open, change the configured `shell` and trigger a save: existing tabs' saved shells are unchanged; only new tabs use the new shell
- [ ] Slide animation: showing slides the content column in from the anchored edge over ~130 ms (ease-out) and hiding slides it back out (ease-in), removing the surface only after the slide completes - on both `edge = top` and `edge = bottom`, with horizontal placement never animating
- [ ] During the slide the surface's size and position never change (no visible resize or jump), and the content's opacity never fades
- [ ] Keyboard focus is grabbed the instant the surface shows, and returns to the previously focused window the moment a hide *starts* - not when the surface disappears
- [ ] `Ctrl+Shift+Q` and click-away (focus loss) hides animate exactly like the hotkey hide
- [ ] A background job (`sleep 100 &`) survives a hide/show cycle toggled mid-animation (toggle again within the 130 ms)
- [ ] Rapid-toggle spam (10+ toggles inside 1 s): the surface always ends in the state the last toggle implies - no teleports between endpoints, no stuck or half-animated surfaces, focus correct at rest - and a toggle arriving mid-hide visibly reverses the slide on the same surface
- [ ] A toggle arriving inside the measuring window (hide immediately after show) destroys the surface immediately: no slide, no stuck state
- [ ] Pressing or dragging the grab bar during the 130 ms slide-in does nothing to the animating surface; the drag only takes once the content rests

## Non-goals

- Non-COSMIC compositors as a priority, and X11 support
- Flatpak packaging (until COSMIC exposes layer-shell/toplevel protocols to sandboxes)
- Built-in global hotkey registration (no GlobalShortcuts portal on COSMIC yet)
- Wrapping an external terminal emulator (may be reconsidered later; see the ROADMAP)

## Credits

DropTerm's terminal engine and widget are adapted from [cosmic-term](https://github.com/pop-os/cosmic-term) (Copyright System76, GPL-3.0-only), reused under the same license per the file-level attributions in `src/terminal/`. The single-instance/toggle pattern was cross-checked against [cosmic-ext-quake-terminal](https://github.com/M0Rf30/cosmic-ext-quake-terminal).
