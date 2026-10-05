# Development

How to work on DropTerm itself: the dev launcher loop, running without a shortcut, and the manual test checklist every change answers to.

## Development launcher

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

## Running locally without a shortcut

No COSMIC shortcut configured yet? You can still open and dismiss the overlay from a local run:

- `just dev` builds, installs the launcher, and restart-launches the overlay; plain `cargo run` (or `just run`) just launches it.
- `Ctrl+Shift+Q` hides it.
- Clicking another window hides it too (`hide_on_focus_loss` defaults to `true`).
- Running the command a second time still toggles.

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
- [ ] Attention markers: with two tabs, `printf '\a'` in the inactive tab marks it `🔔`; `echo hi` in a third (inactive) tab marks it `•`; activating a marked tab clears its marker; the active tab never gains one; a `while true; printf '\a'; done` bell storm in an inactive tab leaves one steady `🔔` (a state, not a counter); markers follow their tabs through drag-reorder and survive the closing of neighbors
- [ ] Bell notifications: hide the terminal, `printf '\a'` inside it - exactly one desktop notification appears, naming the tab; a bell storm while hidden still yields exactly one; two different tabs belling while hidden yield two; showing and re-hiding re-arms the latch (a new bell notifies again); with the terminal visible a bell posts no notification (marker only); `bell_notification_enabled: false` silences them and the Behavior toggle round-trips to the config file
- [ ] Remote new-tab: with the app running, `cosmic-ext-dropterm new-tab` opens a tab in the running instance, places it per `new_tab_after_current` (default: after the active tab; see the placement item below), activates it, and shows the overlay; with the app not running, the first launch starts with that tab (alongside any restored session tabs)
- [ ] New-tab placement: with `new_tab_after_current: false` (written to the config file or flipped via the Behavior toggle, which round-trips to the file), `Ctrl+Shift+T`, the tab bar's `+`, and `cosmic-ext-dropterm new-tab` append the new tab at the end and activate it - including a first-launch `new-tab` alongside restored session tabs; `duplicate-tab` still lands after the active tab; a restart keeps session order; with the key absent or `true`, every new tab lands after the active tab exactly as the default always did
- [ ] `new-tab --cwd /tmp` opens the shell in `/tmp`; `new-tab --cwd /nonexistent` logs a `warn` and opens in the fallback directory; with the active tab cd'd somewhere, plain `new-tab` inherits that cwd
- [ ] `new-tab --cmd htop` runs htop instead of the shell (no config shell arguments inherited), and closing htop closes the tab; the `--cmd` tab is absent from `session.ron` after the next debounced save, while plain `new-tab` tabs are saved like any other
- [ ] Profile submenu: with at least one profile configured, the terminal context menu's "New Tab" is a submenu - plain New Tab first, then one row per profile in config order - and each profile row launches that profile's command in its `cwd`; with zero profiles the menu renders flat exactly as before
- [ ] Remote profile tab: `new-tab --profile web-prod` opens a tab running that profile, both in the running instance and at first launch; `--profile ghost` logs a `warn` and degrades to a visible plain-shell tab; `--profile` + `--cmd` is rejected by the CLI; an explicit `--cwd` beats the profile's own `cwd`
- [ ] Default profile: with `default_profile` set, `Ctrl+Shift+T` and a fresh startup run it as full profile tabs; a restart restores the profile tab by id, and a profile edited between save and restart launches the edited command; `duplicate-tab` still spawns a fresh shell
- [ ] Profile session semantics: a profile tab is saved to `session.ron` with its id and cwd while a `--cmd` tab stays absent; a saved id since deleted from the config restores a plain shell in the saved cwd
- [ ] Tab bar `+` visibility: the `+` shows beside the gear at the strip's right corner in every tab state (the 1-tab strip carries `+` and gear alone), on both edge placements and both themes; glyph, `New Tab` tooltip, and hover treatment read on light and dark panes alike
- [ ] Tab bar `+` click parity: a plain click on the `+` behaves exactly like `Ctrl+Shift+T` - with `default_profile` set it runs as a full profile tab, `cd /tmp` then click spawns in `/tmp`, and placement follows `new_tab_after_current` both ways (the placement item above covers the shared funnel)
- [ ] Tab bar `+` profile dropdown: with at least one profile configured, right-click on the `+` opens a dropdown listing `New Tab` first then one row per profile in config order (the context-menu submenu's composition); every profile row launches that profile, the plain row runs the default spawn; rows highlight on hover and the dropdown never clips at the surface's right corner
- [ ] Tab bar `+` zero profiles: with an empty profile list, right-click on the `+` no-ops (no visual change beyond the `+` itself) while the plain click still spawns
- [ ] Tab bar `+` dropdown dismissal: while open, a left press on a row picks it and closes; any other press - on the `+` itself (toggle), the strip, or away over the terminal - closes it with no side effects (no spawn, no tab select, no drag armed, nothing reaching the terminal)
- [ ] Tab bar `+` and strip geometry: a many-tab squeeze never underlaps the `+` or the gear (the row clips at the widened corner zone), drag reorder stays clear of both targets, and a `+` press during an inline rename commits the rename
- [ ] Tab bar `+` dropdown lifecycle: hide/show while the dropdown is open returns with it closed (the open state dies with the strip's tree)
- [ ] `toggle`/`show`/`hide` still round-trip through a second launch exactly as before (including via D-Bus activation)
- [ ] A `new-tab` sent during the 130 ms slide-out reverses the slide and ends shown with the new tab active - it never finishes the hide
- [ ] Remote close-tab: with the app running and two tabs, `cosmic-ext-dropterm close-tab` closes the active tab (successor activates) without showing the overlay; `close-tab --tab-index 1` closes the first; `--tab-index 0` is rejected by the CLI; closing the last tab respawns a fresh one; an out-of-range index logs a `warn` and no-ops; with the app not running, the app starts plain
- [ ] Remote select-tab: with three tabs, `cosmic-ext-dropterm select-tab 2` activates the second and shows the overlay (a hidden terminal shows); `select-tab 4` warns and no-ops without showing; `select-tab 0` is rejected by the CLI; a select during the 130 ms slide-out reverses it and ends shown on the target tab; first launch starts plain
- [ ] Remote rename-tab: `cosmic-ext-dropterm rename-tab "work"` pins the title - a shell OSC title change no longer overwrites it - and it persists in `session.ron`; `rename-tab ""` clears the override and shell titles resume; an inline rename open on the target tab is discarded when the remote title lands; the overlay never shows; out-of-range warns and no-ops; first launch starts plain
- [ ] Remote duplicate-tab: with the active tab cd'd somewhere, `cosmic-ext-dropterm duplicate-tab` opens a fresh shell tab in that cwd after it, makes it active, and shows the overlay; duplicating a `--cmd htop` tab yields a plain shell in that tab's cwd; an out-of-range `--tab-index` warns and duplicates from the active tab; the duplicate saves to the session like any plain tab; first launch starts plain
- [ ] Duplicate-tab keybind/menu: `Ctrl+Shift+D` (and the context menu's "Duplicate Tab" row, hint shown) opens a fresh shell tab in the active tab's cwd directly after it and activates it; a tab renamed "work" duplicates already named "work" - the name survives the fresh shell's first prompt title and a restart - while an unrenamed tab's title resolves naturally (no literal pin of a shell-set title); with `default_profile` set the duplicate is still a plain shell, and with `new_tab_after_current: false` it still lands after-active while `Ctrl+Shift+T` appends at the end; `--tab-index` on a renamed non-active tab carries that tab's pin; `Ctrl+D` in the shell still EOFs and bare `d` types
- [ ] Editing `height_percent` in the config reshapes a shown surface immediately, in place - and a hidden surface opens at the new height on its next show
- [ ] `width_percent` below 100 (e.g. 60) sizes the surface to that fraction of the output width; the default 100 is indistinguishable from the full-width surface
- [ ] `h_align = Left` / `Right` sits flush against that edge, `Center` centers the surface with equal left/right margins - on both `edge` values
- [ ] Editing `width_percent`/`h_align`/`edge` while shown also reshapes in place (width 60 → 100 clears the margins and refills); non-geometry edits (font, colors) leave the surface untouched
- [ ] A visible grab bar renders on the free edge, outside the tab bar when one is shown; dragging it resizes the height live, and only the height - width and alignment stay put, including on a narrower-than-full-width surface
- [ ] Dragging respects the 10-100 % height band; with `edge = bottom`, dragging away from the screen edge grows the terminal
- [ ] After a drag is released the new height applies immediately (a hide/show right away opens at the new height) and `height_percent` is written to the config file once, about 400 ms later - no write/reshape storm, and rapid re-drags coalesce into one write
- [ ] With default config, the only visible change from the previous build is the grab bar; tab bar placement is unchanged (from the screen edge: content, tab strip, grab bar - the strip always present, its tab buttons only with more than one tab open)
- [ ] `tab_position: Top` and `tab_position: Bottom` pin the strip to that side on both `edge` values - on the matching anchor the strip sits at the screen edge, on the opposite one it restores the default side; the grab bar stays on the free edge in all four layouts
- [ ] Switching Tab bar position in the settings drawer's Geometry section while the drop-down is shown reorders the strip immediately - no resize jump, the terminal's pixel height is unchanged; a pick while hidden applies at the next show
- [ ] Tab title modes: with tabs whose shells report `user@host:~/path` titles (plus one renamed tab), flip Tab titles in the Behavior section - `Shell title` (default) shows reports verbatim; `Abbreviated path` renders `jpt@box:~/src/oss/l1br3/ced` as `jpt@box:~/s/o/l/ced` (an absolute-path title abbreviates identically, a non-directory title like a static vim title is left alone); `Last segment` shows the cwd's final component (`ced`, `(root)` at `/`); the renamed tab never moves in any mode
- [ ] `Terminal number` parks every un-renamed tab at its `Terminal N` while cwd tracking continues - a `cd` still marks the session dirty and `session.ron` records the new cwd
- [ ] Mode changes apply live: flipping the dropdown (or hand-writing `tab_title_mode` with tabs running) retitles shown tabs immediately; an empty rename commit shows the mode's title right away instead of the cleared text; after a relaunch with restore, the fresh shell's first title report lands per the current mode and a restored rename survives it
- [ ] The settings-gear tooltip pops toward the terminal in all four layouts (edge x pinned position), including the strip pinned at the screen edge - never clipped at the right corner
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
- [ ] Toggling `respect_panel` in the settings drawer or config file while the drop-down is shown applies on the next show
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
- [ ] `Ctrl+Shift+I` opens the settings drawer while the terminal has focus, and the keypress never reaches the shell; a second press closes the drawer - it never opens a second window
- [ ] Drawer layout on the layer surface: the drawer overlays the terminal's right edge with the framework's title + Close button + scrolling sections; the terminal stays visible beneath and its grid does not reflow on open/close (`reset` then toggle the drawer - the same lines stay put)
- [ ] Live edits under the open drawer: drag the opacity slider and the surface transparency changes while dragging; a font-size or scheme pick re-renders the terminal immediately
- [ ] Input capture: keyboard and pointer over the terminal never reach the shell while the drawer is open (type, click, and drag over it - nothing lands in the pty); the drawer's text inputs type normally
- [ ] Focus handoff under `keyboard_mode: Exclusive`: opening the drawer moves keyboard focus off the terminal, closing it returns focus (typing works again with no extra click); repeat under `OnDemand`
- [ ] Escape with the drawer open closes the drawer only - the surface does not hide, and the Escape is not forwarded to the shell; with the drawer closed, Escape still reaches the shell (works in vim) and still cancels an active selection
- [ ] Context-menu "Settings…" and the tab-bar gear button hit the same toggle as `Ctrl+Shift+I`
- [ ] Focus loss with the drawer open (with `hide_on_focus_loss: true`): clicking away hides surface + drawer together; the next show uncovers the drawer exactly as left - still open, an in-flight text draft intact; explicitly closing (Escape / Close button / repeat keybind) then reopening starts fresh from the config values
- [ ] Narrow-output width floor: with `width_percent: 30` on a narrow output, the drawer stays inside the surface (344px floor clamped to the surface width) and remains usable - note how much of the terminal it covers
- [ ] The drawer's Close button is localized (libcosmic's own `fl!("close")` string) in a non-English locale
- [ ] New-tab cwd inheritance: `cd /tmp` in the active tab, press `Ctrl+Shift+T` - the new tab's shell starts in `/tmp`; with an unreadable `/proc` entry for the shell (e.g. under `sudo`) the new tab falls back to the default directory, never an error
- [ ] Opening the drawer never hides or resizes the surface; a remote `new-tab`/`toggle` arriving while it is open shows the terminal with the drawer still up
- [ ] Closing the drawer does not exit the app (the toggle still works); reopening it shows current config values, no stale buffered state
- [ ] All five sections are present with every field; for one field per section, editing it in the drawer persists exactly what a hand-edit of the same value would (compare the config file)
- [ ] No Apply button anywhere: toggles/dropdowns write on change, sliders on release, text inputs on Enter
- [ ] Controls respect the config ranges (height 10-100, width 30-100, opacity 0-100, scrollback non-negative); garbage scrollback text writes nothing
- [ ] Geometry edits from the drawer land at the next show; `layer`/`keyboard_mode`/`respect_panel` at the next show; `shell`/`args`/`scrollback_lines` on newly spawned tabs only; font/colors/opacity on every tab
- [ ] `session_persistence_enabled: false` stops session saves and skips launch restore (fresh single tab); `session.ron` is left untouched; re-enabling resumes saving over it
- [ ] `slide_animation_enabled: false` restores instant show/hide; toggling it mid-animation never aborts the running slide
- [ ] Race check (newest intent wins): drag the grab bar, release, immediately open settings and drag the height slider - the slider's value is what lands in the config file, no write/reshape storm
- [ ] G3/G4 re-pass after the write-path refactor: drag feel unchanged - live pixel tracking, deadband, single write-back ~400 ms after release, no echo storm
- [ ] Default config (toggles `true`, drawer never opened): behavior identical to the previous build except the new keybind
- [ ] Color schemes: the Appearance section's two dropdowns list all twelve schemes - COSMIC pair first, the rest alphabetically; picking one applies live on every tab, visible in the terminal beneath the open drawer, and the pick persists as the scheme's display name
- [ ] Scheme fidelity: pick every scheme in the dark-mode dropdown (light-mode one mirrors) and compare against its canonical palette - e.g. Dracula bg `#282a36` / fg `#f8f8f2`, Nord bg `#2e3440`: cells, cursor, and background render the scheme's look
- [ ] Chrome coherence: with several tabs open, the tab bar and grab bar sit on the same background as the cells - the scheme's own for Dracula & co., the system theme's for the COSMIC pair
- [ ] Per-mode pairing: system dark wears the dark pick, system light the light pick; flipping the system theme switches schemes live (with `Application theme` on `Follow system`)
- [ ] `opacity` still multiplies over a scheme-owned background (e.g. 50 over Dracula)
- [ ] Bogus-name fallback: hand-write a name that isn't in the registry into `color_scheme_dark`, relaunch - the terminal renders the COSMIC dark scheme silently (no error), the dropdown shows no selection, and picking a scheme recovers
- [ ] Scheme keys never touched: colors, cursor, background, and chrome byte-identical to the previous build (COSMIC pair, system-derived background)
- [ ] Application theme: the Appearance section's dropdown lists exactly Follow system / Dark / Light after the scheme rows, and the selection mirrors the config value (default config → Follow system)
- [ ] Pin independence: pin Dark while the system is Light, and Light while it is Dark - scheme, cells, chrome, and the settings drawer all wear the pinned mode
- [ ] System mode flips both ways under a pin leave the terminal unchanged (still the pinned mode)
- [ ] Accent-color edit while pinned with the matching system mode applies to the terminal
- [ ] Restore: switching back to Follow system picks the current system mode on the spot, and system flips resume driving the terminal
- [ ] Untouched-default config behaves byte-identically to the previous build - no pin is ever issued, system flips drive everything as before
- [ ] Settings button: the strip renders in every tab state - with a single tab it carries only the gear button at its right corner, and clicking it opens the settings drawer (the same window `Ctrl+Shift+I` opens); with two or more tabs the button anchors the tab row's right end
- [ ] Crossing 1↔2 tabs resizes nothing: the terminal area keeps its size and the strip its height - opening or closing the second tab changes only the tab row itself
- [ ] Many-tab squeeze: with more tabs than fit, buttons never run under the gear button - including at the minimum-width floor, where the row clips at the button's zone
- [ ] Gear + tooltip legibility: the gear and its `Settings…` tooltip read clearly on light and dark panes; hovering shows the disc and tooltip immediately, with no delay
- [ ] The tooltip never clips at the right corner, on both `edge = top` and `edge = bottom` - it grows leftward from the corner, clearing the strip toward the terminal
- [ ] Clicking the settings button during an inline tab rename commits the rename and opens the drawer
- [ ] Drag-reordering a tab stays clear of the gear button's zone
