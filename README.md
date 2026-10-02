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

Each setting is its own RON file inside that directory (`height_percent`, `width_percent`, `h_align`, `edge`, `layer`, `keyboard_mode`, `respect_panel`, `shell`, `args`, `font_name`, `font_size`, `color_scheme_dark`, `color_scheme_light`, `opacity`, `scrollback_lines`, `hide_on_focus_loss`, `session_persistence_enabled`, `slide_animation_enabled`). Every one of them is editable in the [settings window](#settings-window); the files can also be edited directly, e.g.:

```sh
echo '60' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/height_percent
echo 'Left' > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/h_align
```

Geometry settings reshape the surface the moment they change - a shown surface is reshaped in place (no destroy/recreate), a hidden one picks up the new geometry on its next show:

- `height_percent` (default `40`, range 10-100): surface height as a percentage of the output height. Also written automatically shortly after a grab-bar drag settles.
- `width_percent` (default `100`, range 30-100): surface width as a percentage of the output width. `100` reproduces the original full-width surface exactly.
- `h_align` (`Left`, `Center`, or `Right`; default `Center`): horizontal placement of the surface within the output. `Left` and `Right` sit flush against that edge; `Center` splits the leftover space into equal left and right margins. Only meaningful below `width_percent` 100.

`hide_on_focus_loss` defaults to `true` (click away to hide); write `false` to keep the overlay open when it loses focus.

`respect_panel` defaults to `false`: the drop-down covers the panel and dock, Yakuake-style. Write `true` to place it inside the compositor's usable area instead - it sits flush below a top panel (or above a bottom dock, together with `edge`), while staying on its configured layer, so on the default `Overlay` layer it still appears above fullscreen windows. The setting applies on the next show - toggle it in the Advanced card of the [settings window](#settings-window) or write it directly:

```sh
echo true > ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/respect_panel
```

With an autohide panel, the hidden panel still reserves its handle strip, so a respecting drop-down sits a few pixels below the screen edge.

## Session persistence

DropTerm reopens your tab layout on every launch: one fresh shell per saved tab, spawned in its saved working directory, with its title, and the tab that was focused when the session was saved is focused again. Toggle it with `session_persistence_enabled` (default `true`, Behavior card in the [settings window](#settings-window)): `false` stops saves *and* skips restore at launch - the app starts with a single fresh tab, and the existing `session.ron` is left untouched on disk, so re-enabling picks the session back up.

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

Showing and hiding slide the whole content column - terminal, tab bar when one is shown, grab bar - in from, and back out toward, the anchored screen edge over about 130 ms: an ease-out curve on the way in, ease-in on the way out, pure translation with no fade. Toggle it with `slide_animation_enabled` (default `true`, Behavior card in the [settings window](#settings-window)) - `false` restores instant show/hide; either way the animation is deliberately cheap: the surface's geometry never changes during the animation, only the content's painted offset does, so the compositor sees no resize traffic. A toggle arriving mid-animation reverses the slide from wherever the content stands - nothing teleports, nothing is dropped or queued - and PTYs keep running throughout: keyboard focus returns to the previously focused window the moment a hide *starts*, and the surface is destroyed only after the slide-out completes. A hide that catches the surface still measuring (a toggle within the show's measuring window) skips the animation and destroys immediately.

## Color schemes

The terminal wears a built-in color scheme picked per system theme mode: the Appearance card of the [settings window](#settings-window) has two dropdowns - "Dark color scheme" (worn while the system theme is dark) and "Light color scheme" (while it is light). Flipping the system theme switches between the two picks; a config edit or a theme flip re-applies live.

Twelve built-in schemes ship: `COSMIC Dark` and `COSMIC Light` (the defaults), `Dracula`, `Solarized Dark`, `Solarized Light`, `Gruvbox Dark`, `Gruvbox Light`, `Tokyo Night Storm`, `Tokyo Night Day`, `Nord`, `Catppuccin Mocha`, and `Catppuccin Latte` - values transcribed from their canonical upstream palettes, with the source and license recorded per scheme in `src/terminal/theme_source.rs`.

- Every scheme except the COSMIC pair owns its background: picking Dracula renders Dracula's `#282a36` behind the cells, and the chrome - tab bar, grab bar - follows the scheme background, so cells and chrome cannot disagree. The COSMIC pair keeps following the system theme's background, as always. `opacity` multiplies on top of either background.
- A scheme's name is the config format: a pick persists the display name under `color_scheme_dark`/`color_scheme_light`. A name not in the registry (stale, hand-typed, or renamed upstream) silently falls back to the COSMIC pair for that mode - no error, no migration - and the dropdown shows no selection until you pick again.
- Dim variants are derived, not shipped: each normal color's channels are scaled by 0.66 (Alacritty's dim rule), one uniform rule for every non-COSMIC scheme. Bright variants come from the palettes themselves.
- User-defined scheme files are not supported; the built-in registry is the whole feature (see the ROADMAP's "Later" section).

## Settings window

Every setting above is editable in-app: press `Ctrl+Shift+I` while the terminal has keyboard focus to open the settings window - a separate normal window in the same process. The keypress is swallowed from the shell like every other app bind. A second press focuses the existing window; there is never a second one.

The window shows five cards - Appearance (font, size, opacity, and the two color scheme dropdowns), Geometry, Behavior, Terminal, and Advanced (`respect_panel`, `layer`, `keyboard_mode`) - and applies instantly: every control writes its key the moment you commit it, with no Apply button. Commit points per control kind: toggles and dropdowns write on change, sliders write on release (the value previews live while dragging), text inputs write on Enter.

While the window is open the terminal is hidden: a normal window can never stack above a layer surface on COSMIC, so opening settings hides the drop-down through its normal hide path (animated when slides are enabled) - the same resolution as Guake's default hide-on-focus-loss for its prefs. The rules:

- Any show/toggle intent while the window is open (hotkey, second launch) closes the settings window first, then shows the terminal.
- Closing the window does not re-show the terminal and does not exit the app; the next toggle brings the terminal back with every edit already applied.
- The hide on open is unconditional - it does not touch `hide_on_focus_loss`, which keeps governing ordinary focus loss once the window is closed.

Each control states next to it when its edit takes effect, and because the terminal is hidden while the window is open, even the live keys land visibly at the next show:

| Field | Applies |
|---|---|
| `height_percent`, `width_percent`, `h_align`, `edge` | live - reshapes a shown surface in place (deferred mid-drag and mid-slide); visible at the next show while the window is open |
| `font_name`, `font_size`, `opacity`, `color_scheme_dark`, `color_scheme_light` | live, on every tab; visible at the next show while the window is open |
| `layer`, `keyboard_mode` | next show - a shown surface is never recreated under itself |
| `shell`, `args`, `scrollback_lines` | new tabs only |
| `hide_on_focus_loss`, `session_persistence_enabled`, `slide_animation_enabled` | next trigger - the session toggle at the next save/restore cycle, the slide toggle at the next transition (a running animation is never aborted) |

The two Behavior toggles default to `true`, which is exactly the pre-settings behavior:

- `session_persistence_enabled` is symmetric: `false` stops session saves *and* skips launch restore. The existing `session.ron` is never deleted or rewritten by disabling - re-enabling resumes saving over it.
- `slide_animation_enabled`: `false` restores instant show/hide with immediate destroy (the pre-slide-animation behavior).

Controls constrain input to the same ranges as the config (`height_percent` 10-100, `width_percent` 30-100, `opacity` 0-100, `scrollback_lines` non-negative); the write path never persists an out-of-range value. One known limitation: the `args` input splits on whitespace with no quoting support - arguments containing spaces stay config-file-only.

## Features (v1)

- Layer-shell window anchored to the top (or bottom) edge, full width, above other windows
- Hidden from Alt-Tab and the dock
- Exclusive keyboard focus when shown; focus returns to the previous window when hidden
- Shell session persists while hidden (the PTY is never torn down - only the surface is)
- COSMIC theme integration: light/dark and the two built-in COSMIC color schemes
- Copy/paste (`Ctrl+Shift+C`/`V`), primary-selection paste (middle-click, `Shift+Insert`), scrollback, mouse reporting, `Ctrl`-click to open URLs
- New tabs (`Ctrl+Shift+T`, the tab bar's `+`) inherit the active tab's working directory — `cd` once, spawn tabs where you already are; a failed read falls back to the pty's default directory
- Zoom (`Ctrl+`/`Ctrl-`/`Ctrl+0`), select-all and clear-scrollback shortcuts
- Tabs: open/close/rename inline/reorder by drag or `Ctrl+Shift+PageUp`/`PageDown`, cycle with `Ctrl+PageUp`/`PageDown` or `Ctrl+Tab`, jump with `Ctrl+Shift+1..8` (`Ctrl+Shift+9` always takes the last tab) - cosmic-term's binds
- In-app dismiss: `Ctrl+Shift+Q` hides the overlay; clicking away hides it too

## Known risks

- **Clipboard ownership vs. destroyed surfaces**: on Wayland a copied selection lives only as long as the client connection that offered it, and libcosmic binds its runtime clipboard to the first window created — for a layer-shell-only app, the drop-down surface itself, which every hide destroys. Copying out of DropTerm therefore died the moment you focused another app to paste into it. DropTerm now opens an invisible, never-closed anchor window before the first layer surface, so the clipboard binding (and your last copy) survives every hide/show cycle.
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
- [ ] Numbered tab switching: with three tabs, `Ctrl+Shift+1`/`2`/`3` activate the first/second/third; `Ctrl+Shift+9` activates the last tab; `Ctrl+Shift+5` with three tabs does nothing (no wrap, no crash); bare `Ctrl+2` reaches the shell (only `Ctrl+=`/`-`/`0` zoom)
- [ ] Tab moves: `Ctrl+Shift+PageUp`/`PageDown` shift the active tab one slot, clamping at the list edges (no wrap); the moved tab stays active and focused, and `session.ron` records the new order
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
- [ ] With `respect_panel: true` and a top panel, the panel stays fully visible and clickable while the drop-down is open, and the terminal sits flush below it
- [ ] With `respect_panel: true`, `edge: bottom`, and a bottom dock, the drop-down sits above the dock
- [ ] With `respect_panel: true`, the drop-down still appears above a fullscreen window
- [ ] With a config directory that has no `respect_panel` key (or `false` written), the drop-down still covers the panel
- [ ] Toggling `respect_panel` in the settings window or config file while the drop-down is shown applies on the next show
- [ ] With an autohide panel and `respect_panel: true`, the drop-down lands below the panel's handle strip
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
- [ ] `Ctrl+Shift+I` opens the settings window while the terminal has focus, and the keypress never reaches the shell; a second press focuses the existing window - never a second one
- [ ] New-tab cwd inheritance: `cd /tmp` in the active tab, press `Ctrl+Shift+T` - the new tab's shell starts in `/tmp`; with an unreadable `/proc` entry for the shell (e.g. under `sudo`) the new tab falls back to the default directory, never an error
- [ ] Opening the window hides a shown terminal (animated when slides are enabled); toggling the terminal while the window is open (hotkey, second launch) closes the window first and then shows the terminal; closing the window leaves the terminal hidden until the next toggle - which shows it with every edit applied
- [ ] Closing the settings window does not exit the app (the toggle still works); reopening the window shows current config values, no stale buffered state
- [ ] All five cards are present with every field; for one field per card, editing it in the window persists exactly what a hand-edit of the same value would (compare the config file)
- [ ] No Apply button anywhere: toggles/dropdowns write on change, sliders on release, text inputs on Enter
- [ ] Controls respect the config ranges (height 10-100, width 30-100, opacity 0-100, scrollback non-negative); garbage scrollback text writes nothing
- [ ] Geometry edits from the window land at the next show; `layer`/`keyboard_mode`/`respect_panel` at the next show; `shell`/`args`/`scrollback_lines` on newly spawned tabs only; font/colors/opacity on every tab
- [ ] `session_persistence_enabled: false` stops session saves and skips launch restore (fresh single tab); `session.ron` is left untouched; re-enabling resumes saving over it
- [ ] `slide_animation_enabled: false` restores instant show/hide; toggling it mid-animation never aborts the running slide
- [ ] Race check (newest intent wins): drag the grab bar, release, immediately open settings and drag the height slider - the slider's value is what lands in the config file, no write/reshape storm
- [ ] G3/G4 re-pass after the write-path refactor: drag feel unchanged - live pixel tracking, deadband, single write-back ~400 ms after release, no echo storm
- [ ] Default config (toggles `true`, window never opened): behavior identical to the previous build except the new keybind
- [ ] Color schemes: the Appearance card's two dropdowns list all twelve schemes - COSMIC pair first, the rest alphabetically; picking one applies live on every tab (visible at the next show while the settings window is open), and the pick persists as the scheme's display name
- [ ] Scheme fidelity: pick every scheme in the dark-mode dropdown (light-mode one mirrors) and compare against its canonical palette - e.g. Dracula bg `#282a36` / fg `#f8f8f2`, Nord bg `#2e3440`: cells, cursor, and background render the scheme's look
- [ ] Chrome coherence: with several tabs open, the tab bar and grab bar sit on the same background as the cells - the scheme's own for Dracula & co., the system theme's for the COSMIC pair
- [ ] Per-mode pairing: system dark wears the dark pick, system light the light pick; flipping the system theme switches schemes live
- [ ] `opacity` still multiplies over a scheme-owned background (e.g. 50 over Dracula)
- [ ] Bogus-name fallback: hand-write a name that isn't in the registry into `color_scheme_dark`, relaunch - the terminal renders the COSMIC dark scheme silently (no error), the dropdown shows no selection, and picking a scheme recovers
- [ ] Scheme keys never touched: colors, cursor, background, and chrome byte-identical to the previous build (COSMIC pair, system-derived background)

## Non-goals

- Non-COSMIC compositors as a priority, and X11 support
- Flatpak packaging (until COSMIC exposes layer-shell/toplevel protocols to sandboxes)
- Built-in global hotkey registration (no GlobalShortcuts portal on COSMIC yet)
- Wrapping an external terminal emulator (may be reconsidered later; see the ROADMAP)

## Credits

DropTerm's terminal engine and widget are adapted from [cosmic-term](https://github.com/pop-os/cosmic-term) (Copyright System76, GPL-3.0-only), reused under the same license per the file-level attributions in `src/terminal/`. The single-instance/toggle pattern was cross-checked against [cosmic-ext-quake-terminal](https://github.com/M0Rf30/cosmic-ext-quake-terminal).
