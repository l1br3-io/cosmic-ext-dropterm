# Research: terminal → desktop-environment integration for DropTerm

Date: 2026-10-02
Question: *How do industry-leading terminal emulators — especially drop-down/quick
terminals — integrate/consolidate with their host desktop, and which of those
approaches are most applicable to DropTerm on COSMIC?*

## TL;DR

**The drop-down-specific wins are notifications and attention-signaling, not
window management — DropTerm already has the geometry/animation/tabs core that
Yakuake, Guake, and Ghostty's quick terminal ship, so the highest-value gaps are
(a) telling the user something happened while the overlay was hidden (bell →
desktop notification, per-tab activity/bell indicators), (b) the cheap
shell-integration conveniences every peer has (new tab inherits cwd), and
(c) remote-control surface (CLI/D-Bus actions), all buildable today with zero
libcosmic/portal risk. The two features DropTerm most visibly lacks versus
cosmic-term — search and customizable/numbered tab keybinds — port directly from
cosmic-term on the same engine. Monitor selection and self-registered global
hotkeys stay blocked by COSMIC (pinned libcosmic `IcedOutput`, no
GlobalShortcuts portal).**

Two grounding corrections came out of the primary sources: (1) DropTerm's
README claims `hide_on_focus_loss = true` "behaves like Yakuake/Guake", but all
three peers ship the *opposite* default — Yakuake `KeepOpen=true`
([`app/config/yakuake.kcfg:39-43`](https://invent.kde.org/utilities/yakuake/-/blob/master/app/config/yakuake.kcfg)),
Guake `window-losefocus=false`
([`guake/data/org.guake.gschema.xml:114-118`](https://github.com/Guake/guake/blob/master/guake/data/org.guake.gschema.xml)),
Ghostty `quick-terminal-autohide=false` on Linux
([ghostty config ref](https://ghostty.org/docs/config/reference#quick-terminal-autohide)).
(2) URL opening via the `open` crate (not the OpenURI portal) is provably the
right call today: COSMIC's portal backend implements exactly
`Access, FileChooser, RemoteDesktop, Screenshot, Settings, ScreenCast` — no
OpenURI, no Notification, no GlobalShortcuts
([xdg-desktop-portal-cosmic `data/cosmic.portal` @ `4902f57`](https://github.com/pop-os/xdg-desktop-portal-cosmic)).

## What DropTerm does today

Grounded in the working tree (≈15.7k lines, `src/`):

- **Single-instance + D-Bus activation**: `run_single_instance` in
  `src/main.rs:55`; a second launch forwards its subcommand (`toggle|show|hide`,
  `src/app.rs:49-55`) through libcosmic's `Application::dbus_activation`
  (`src/app.rs:2894-2906`), which parses `ActivateAction` into a
  `ToggleAction`. Startup applies the first launch's own action at init
  (`src/app.rs:2074-2078`). This is the *only* remote-control surface — no
  new-tab/cmd/cwd actions exist (ROADMAP "Later").
- **Layer-shell geometry**: full-width-ish overlay, `height_percent` 10-100,
  `width_percent` 30-100, `h_align`, `edge` top/bottom, `layer`
  overlay/top, `keyboard_mode` exclusive/on-demand, `respect_panel`
  (`src/config.rs:86-145`); always targets `IcedOutput::Active`
  ("output under the cursor") — the `output: Option<String>` config key exists
  (`src/config.rs:109`) but is never consumed for targeting
  (`src/surface.rs:62-71` hardcodes `IcedOutput::Active` with a comment calling
  monitor selection "v2"). Grab-bar drag-resize persists height
  (`src/app.rs:2463-2586`).
- **Slide animation**: ~130 ms eased translate, reversible mid-flight,
  destroy-after-slide-out (`src/slide.rs`, gated by
  `slide_animation_enabled`, `src/app.rs:1278-1311`).
- **Tabs**: open/close/rename (inline)/reorder (drag); cycle via
  Ctrl+PageUp/PageDown **and** Ctrl(+Shift)+Tab; new tab inserts after the
  active one; last-tab-close respawns (`src/keybinds.rs:62-85`,
  `src/app.rs:1022-1078`). No numbered switching (Ctrl+Shift+1-9), no
  tab-move binds, no per-tab activity indicator, no user-customizable binds
  (`src/keybinds.rs:3-9` documents them as fixed).
- **Session persistence**: per-tab title/override/shell/cwd + focused index,
  debounced atomic writes to `$XDG_STATE_HOME/…/session.ron`
  (`src/session.rs`, `src/app.rs:1144-1194`); cwd is sampled from
  `/proc/<pid>/cwd` readlink (`src/terminal/engine.rs:447-458`), gated on title
  events (`src/app.rs:1201-1216`). **New tabs do not inherit the active tab's
  cwd** — `spawn_tab(None)` passes `working_directory: None`
  (`src/app.rs:432-441`).
- **Theming**: 12 built-in schemes per system light/dark + a `ThemePreference`
  pin via `set_theme` (`src/app.rs:1398-1458`); live reaction to
  `system_theme_update`/`system_theme_mode_update` (`src/app.rs:2183-2199`).
  Live config reload via `cosmic_config::config_subscription`
  (`src/app.rs:2157-2169`).
- **Clipboard**: copy/copy-primary/paste/paste-primary keybinds
  (`src/app.rs:958-989`), middle-click primary paste (`src/app.rs:2410-2413`),
  bracketed-paste-aware `Terminal::paste` (`src/terminal/engine.rs:465-484`).
  OSC 52 works **copy-only by engine default**: alacritty_terminal 0.25.1's
  `Config::osc52` defaults to `Osc52::OnlyCopy` (registry
  `src/term/mod.rs:352-386`), and DropTerm builds `TermConfig` with
  `..default()` (`src/app.rs:428-431`) — the `ClipboardLoad` handler exists
  (`src/app.rs:2918-2931`) but the engine denies reads under the default.
  A hidden 1×1 "anchor" window keeps the Wayland clipboard binding alive
  across surface destroys (`src/app.rs:1460-1482,2018`).
- **Hyperlinks/URLs**: OSC 8 cell hyperlinks + a vendored URL regex
  (`src/terminal/engine.rs:116-119`), Ctrl+click opens via
  `open::that_detached` (`src/terminal/widget.rs:1463-1467`,
  `src/app.rs:2657-2660`; `open = "5.3.2"` in `Cargo.toml:21`). No
  click-a-filename-opens-editor ("quick open"), no drag-and-drop of files.
- **Events the app already receives but drops**: `TermEvent::Bell` is a no-op
  (`src/app.rs:2917`) — no audible/visual bell, no notification, no tab
  indicator. Title (OSC 0/2) drives tab titles (`src/app.rs:2953-2959`);
  color queries (OSC 10/11) and text-area-size queries are answered by the
  engine loop (`src/app.rs:2936-2951`). No OSC 7/133/9;4/99/777 handling —
  those OSCs are parsed-and-ignored by the engine.
- **No notifications anywhere** (grep for `notif`/`zbus` in `src/`: only the
  D-Bus activation hits), no panel applet, no autostart entry, no search, no
  splits, no profiles, no i18n.
- **Deps of note**: `libcosmic` pinned at rev `08a09e3` (`Cargo.toml:30-33`),
  `alacritty_terminal 0.25.1`, `open 5.3.2`. libcosmic at this rev has
  `widget::toaster`, `dialog::file_chooser` (ashpd/portal-backed), a
  `cosmic::applet` module, and no notification helper (see §7).

## Findings

### 1. cosmic-term (pop-os/cosmic-term @ master `3ef31d7`, 2026-10-01)

The closest sibling on the identical stack (alacritty_terminal 0.25.1,
libcosmic). Its DE-integration surface beyond DropTerm's:

- **Search**: `Terminal::search(&value, forwards)` — regex search over the grid
  via `RegexSearch`, origin at the current selection, scrolling to matches
  (`src/terminal.rs:541-575`); UI = Ctrl+Shift+F find bar
  (`src/main.rs:2381-2401`, `FindSearchValueChanged` `src/main.rs:391`),
  default bind `bind!([Ctrl, Shift], "F", Find)` (`src/shortcuts.rs:458`).
  DropTerm already vendors the same `RegexSearch` machinery for URL
  highlighting (`src/terminal/engine.rs:116-119,246`).
- **Customizable shortcuts**: `ShortcutsConfig { defaults, custom }` — user
  binds stored in cosmic-config as `BTreeMap<Binding, KeyBindAction>` layered
  over `fallback_shortcuts()`, with a `Disable` action to remove a default and
  per-action reset (`src/shortcuts.rs:154-261,439-516`); a searchable
  shortcuts editor window with type-to-filter (`src/main.rs:1029-1046`).
- **Tab keybinds DropTerm lacks**: `TabActivate0..8` (Ctrl+Shift+1..9) and
  `TabMoveLeft/Right` (Ctrl+Shift+PageUp/PageDown — the newest commit,
  "Add moving tabs left and right with key shortcuts (#700)"), plus
  `CopyOrSigint` (bare Ctrl+C copies when there's a selection, else sends
  SIGINT) and `ToggleFullscreen` (F11) (`src/shortcuts.rs:482-499`).
- **Splits**: pane_grid-based horizontal/vertical splits, focus moves, and
  per-pane maximize (`PaneSplitHorizontal/Vertical`, `PaneFocus*`,
  `PaneToggleMaximized`, `src/shortcuts.rs:468-510`; `src/main.rs:2591`).
- **Profiles**: `BTreeMap<ProfileId, Profile>` in config — name, command,
  per-profile dark/light scheme, tab_title, working_directory, drain_on_exit;
  a default profile for new tabs (`src/config.rs:189-244`) — the model
  DropTerm's "per-tab profiles (e.g. SSH)" roadmap item would copy.
- **CLI**: `--working-directory/-w`, `-e/--command/--`, `--no-daemon`;
  daemonizes via `fork::daemon(true, true)` by default — deliberately *not*
  single-instance: every launch daemonizes and opens a window
  (`src/main.rs:95-145`).
- **New-tab cwd inheritance**: `tab_new_inherit_working_directory` config +
  `active_terminal_working_directory()` = the same `/proc` readlink DropTerm
  already uses (`src/terminal.rs:446-455`, `src/main.rs:1542-1556`).
- **Password manager** (feature-gated `password_manager`, on by default via
  `default = ["dbus-config", "wgpu", "wayland", "password_manager"]`):
  full CRUD against the freedesktop **Secret Service** over D-Bus
  (`secret-service = "5.1.0"`, `Cargo.toml:32,66,70`; `src/password_manager.rs`
  500 lines), bound Ctrl+Alt+P, pastes into the pane that opened it.
- **Drag-and-drop**: accepts `text/uri-list` and `x-special/gnome-copied-files`
  drops, converts to `Vec<PathBuf>` (`src/dnd.rs:5-51`).
- **Bell**: `TermEvent::Bell => { //TODO: audible or visible bell options? }`
  — cosmic-term drops it too (`src/main.rs:2998-3000`). Its primary-selection
  OSC 52/clipboard path is *TODO* (`src/main.rs:3012-3014,3022-3024`) where
  DropTerm already handles both clipboards.
- **URL opening**: identical `open::that_detached(&url)` (`src/main.rs:2484,
  2513`). No notifications, no OSC 7/133 handling, no shell-integration
  injection anywhere in the tree (grep: zero hits).

### 2. Ghostty quick terminal (ghostty.org config reference, docs read 2026-10-02)

The same feature class as DropTerm, fully enumerated
(<https://ghostty.org/docs/config/reference>, sections quoted by anchor):

- `quick-terminal-position`: `top|bottom|left|right|center` — **five edges incl.
  vertical**, vs DropTerm's two. No default keybind; you must bind
  `toggle_quick_terminal` yourself — same "compositor/desktop owns the global
  key" stance as DropTerm.
- `quick-terminal-size`: percent (`20%`) or pixels (`300px`), bare value hits
  the *primary axis* (height for top/bottom, width for left/right, orientation-
  dependent for center); comma-separated pair sizes both axes. DropTerm's
  percent-of-output model is the same shape minus explicit pixel sizes.
- `gtk-quick-terminal-layer`: `overlay|top|bottom|background`, **default `top`**
  — vs DropTerm's default `Overlay` (with `top` as an option).
- `gtk-quick-terminal-namespace`: layer namespace as a scripting/effect hook —
  DropTerm already sets `"cosmic-ext-dropterm"` (`src/surface.rs:30`).
- `quick-terminal-screen`: `main|mouse|macos-menu-bar`, default `main`,
  *"Only implemented on macOS."* — so on Linux, Ghostty's quick terminal has no
  monitor choice at all. DropTerm's "output under cursor" (`Active`) is
  already ahead of Linux Ghostty here.
- `quick-terminal-animation-duration`: *"Only implemented on macOS."* — the
  Linux quick terminal doesn't animate; DropTerm's 130 ms slide exceeds the
  platform's own bar.
- `quick-terminal-autohide`: hide on focus loss; **default `true` on macOS,
  `false` on Linux/BSD**, with the stated reason that global shortcuts on
  Linux are less accessible, so keeping the terminal open is preferable.
- `quick-terminal-keyboard-interactivity`: `none|on-demand|exclusive`, default
  `on-demand`, mapped 1:1 to wlr-layer-shell's enum and documented as
  compositor-dependent — exactly DropTerm's `keyboard_mode`
  (`src/config.rs:59-69`) with a different default (DropTerm: `Exclusive`).
- `quick-terminal-space-behavior` (macOS spaces): `move|remain`, Linux always
  `move` — N/A on COSMIC.
- Shell integration is auto-**injected** (`shell-integration = detect` by
  filename; features `cursor/sudo/title/ssh-env/ssh-terminfo/path`,
  `#shell-integration` + `#shell-integration-features`), enabling
  `cursor-click-to-move` (OSC 133-based click-to-prompt), cwd reporting for
  window/tab/split inheritance (`window/tab/split-inherit-working-directory`,
  all default true), and `notify-on-command-finish`
  (`never|unfocused|always`, needs OSC 133 or shell integration; action
  `bell` and/or `notify`; `notify-on-command-finish-after` 5 s default).
- Notifications/bell: `desktop-notifications = true` (OSC 9 and OSC 777),
  `progress-style` (OSC 9;4), `bell-features` (`system|audio|attention|title|
  border`, `attention` + `title` on by default — 🔔 prefix in the title and
  urgency until focused), `app-notifications` toasts for `clipboard-copy` /
  `config-reload` (GTK).
- Clipboard policy: `clipboard-read`/`clipboard-write` = `ask|allow|deny`,
  *"allow clipboard reading after prompting the user and allow writing
  unconditionally"* by default; `clipboard-paste-protection` against
  paste-with-newlines attacks.
- `gtk-single-instance = detect` — single-instance unless launched from a
  terminal (`TERM_PROGRAM` set) or given CLI args, because the running
  instance's config wins.

### 3. Yakuake (invent.kde.org/utilities/yakuake @ master `9ddf4ba`, 2026-09-26)

The original drop-down terminal (Konsole-based, Qt/KDE):

- **D-Bus is the product**: three registered objects export scriptable slots —
  `/yakuake/window` (`toggleWindowState`),
  `/yakuake/sessions` (45 slots: `addSession`, `addSessionTwoHorizontal/
  Vertical/Quad`, `split*`, `runCommand`, `runCommandInTerminal`,
  `tryGrowTerminal{Left,Right,Top,Bottom}`, keyboard-input broadcast
  `setSessionKeyboardInputEnabled`, activity/silence monitoring
  `setSessionMonitorActivityEnabled`/`Silence`, `isSessionClosable`,
  `sessionIdList`, …), `/yakuake/tabs` (`tabTitle`, `setTabTitle`,
  `sessionAtTab`)
  (`app/mainwindow.cpp:67`, `app/sessionstack.cpp:26`, `app/tabbar.cpp:36`;
  `Q_SCRIPTABLE` lists in `app/sessionstack.h:45-108`, `app/tabbar.h:52-56`,
  `app/mainwindow.h:76`). KDE advertises this on the app page: *"Sophisticated
  D-Bus interface"* (<https://apps.kde.org/yakuake/>).
- **Window semantics** (`app/config/yakuake.kcfg`): `Screen` 0 = "the screen
  the mouse pointer is on", else a fixed screen number (L8-12); `Width` 90%
  (10-100), `Height` 50% (L13-26); `Position` = *center point as a percentage
  from the left edge* (L27-33) — a horizontal-slider model vs DropTerm's
  discrete Left/Center/Right; `KeepOpen` **default true** (L39-43);
  `ToggleToFocus` (toggle key refocuses instead of retracting, default false,
  L49-53); `KeepAbove` true; `ShowSystrayIcon` **default true** (L74-78) — the
  tray icon is how you toggle without the global key; `DynamicTabTitles`;
  `KeepOpenAfterLastSessionCloses` false; `ConfirmQuit` true.
- **Behavior**: `FocusFollowsMouse` (per-terminal within splits, default
  false), `OpenAfterStart` (open the window on launch, default false — i.e.
  Yakuake can start hidden at login), `RememberFullscreen` (L80-96);
  mouse-edge auto-open (`PollMouse`, 500 ms interval, default false, L183-193).
- **Animation**: WM-assisted slide with fallback to progressive window-mask;
  `Frames` = 17 × 10 ms ≈ 170 ms default (L169-181) — DropTerm's 130 ms is
  in-family.
- No session restore on quit in master (a `work/session-save-restore` branch
  exists); sessions = tabs-with-splits; shell integration is Konsole's
  (OSC 7 etc. handled by the embedded Konsole part, not Yakuake itself).

### 4. Guake (github.com/Guake/guake @ master `0911ddf`, 2026-08-19)

The GNOME original, Python/GTK, VTE-based:

- **D-Bus `org.guake3.RemoteControl`** at `/org/guake3/RemoteControl`
  (`guake/dbusiface.py:31-40`) with ~30 methods mirroring its CLI:
  `show_hide/show/hide/get_visibility/fullscreen/unfullscreen`,
  `add_tab(directory)`, `close_tab`, `select_tab(index)`, `get_selected_tab(label)`,
  `get_tab_count`, `select_terminal`, `get_selected_terminal`,
  `execute_command`, `v/h_split_current_terminal(_with_command)`,
  `set_bgcolor/fgcolor(_current_terminal)`, `change_palette_name`,
  `reset_colors`, `rename_tab(_uuid)/rename_current_tab`, `get_selected_uuidtab`,
  `show_about/show_prefs/quit` (`guake/dbusiface.py:42-217`).
- **CLI = the D-Bus surface**: `--toggle-visibility --is-visible --show --hide
  --preferences --about --new-tab --select-tab --selected-tab --selected-tablabel
  --select-terminal --selected-terminal --split-vertical --split-horizontal
  --execute-command --tab-index --bgcolor --fgcolor --change-palette
  --reset-colors --rename-tab --rename-current-tab --quit --no-startup-script
  --save-preferences --restore-preferences` (`guake/main.py:90-398`).
- **Settings** (`guake/data/org.guake.gschema.xml`): `mouse-display` **default
  true** (appear on the mouse's monitor) overriding `display-n` (L19-28);
  `window-width` 100 / `window-height` 50 percent (L189-198);
  `window-halignment` 0 center/1 left/2 right, `window-valignment` 0 top/1
  bottom (L209-218) — DropTerm's geometry model almost exactly;
  `window-losefocus` **default false** + `lazy-losefocus` (debounce the hide,
  for desktop-switch/IME flicker; L114-123); `window-refocus` (toggle
  refocuses instead of hiding, default false); `window-ontop` true;
  `open-tab-cwd` **default true** — new tabs open in the current working
  directory (L299-303); `new-tab-after` default false (append at end vs
  DropTerm's insert-after-active); `prompt-on-quit` true and
  `prompt-on-close-tab` 0 never/1 if process running/2 always; `use-popup`
  (startup notification, default true); `startup-script` — a shell script run
  at launch that configures tabs via the CLI above (L154-158); the homepage
  advertises "Start Guake automatically at login, and define a script … to
  configure Guake tabs" (<https://guake.github.io/#more>).
- **Quick Open**: Ctrl+click a *filename* in the output → opens it in your
  editor at line:col; `quick-open-enable` **default false**, configurable
  command line, "in current terminal" variant (gschema L139-152; homepage
  "Quick Open" feature blurb).

### 5. kitty (sw.kovidgoyal.net, docs read 2026-10-02; kitty 0.49.x)

- **OSC 52 policy** — the reference security model:
  `clipboard_control write-clipboard write-primary read-clipboard-ask
  read-primary-ask` (default): writes allowed, reads *ask*, because *"any
  program, even the ones running on a remote server via SSH can read your
  clipboard"* (<https://sw.kovidgoyal.net/kitty/conf/#opt-kitty.clipboard_control>);
  `clipboard_max_size 512` MB. kitty also extends OSC 52 with **OSC 5522**
  (arbitrary MIME types, chunking, per-request passwords to avoid repeated
  prompts, `EPERM/EFBIG`-style error codes)
  (<https://sw.kovidgoyal.net/kitty/clipboard/>). Peers' defaults agree on
  "write ok, read gated": Alacritty `terminal.osc52 = OnlyCopy` default
  (`extra/man/alacritty.5.scd` L618-624), Ghostty read=`ask`,
  iTerm2 "Allow sending of clipboard contents? … requires you to consent each
  time" (<https://iterm2.com/documentation-one-page.html>, Selection prefs).
  DropTerm's engine-default OnlyCopy is the mainstream position.
- **Notifications**: an extensible **OSC 99** desktop-notification protocol —
  title/body chunking, icons, buttons, sounds, `a=focus|report` click actions
  (focus the originating window and/or reply an escape code), `c=1` close
  events, `f`/`t` app-name/type keys for filtering via `filter_notification`
  (<https://sw.kovidgoyal.net/kitty/desktop-notifications/>). Bell side:
  `enable_audio_bell yes`, `visual_bell_duration/color`,
  `window_alert_on_bell yes` (urgency/demand-attention),
  `bell_on_tab "🔔 "` (tab indicator while unfocused), `command_on_ball` hook
  (conf#opt-kitty.* in the conf reference).
- **Remote control**: `allow_remote_control no|yes|password|socket|socket-only`
  (default `no`) + `listen_on` unix/tcp socket; fine-grained per-action
  permissions and a user authorization program — i.e. remote control is
  powerful (send-text, launch, get-text, screenshot, …) and *deliberately
  off by default* (<https://sw.kovidgoyal.net/kitty/conf/#opt-kitty.allow_remote_control>,
  remote-control doc). Contrast: Yakuake/Guake expose their D-Bus
  unconditionally on the session bus.
- **Shell integration** is auto-injected (`shell_integration enabled`, features
  `no-rc no-cursor no-title no-cwd no-prompt-mark no-complete no-sudo`;
  conf#opt-kitty.shell_integration) — OSC 7 cwd, OSC 133 prompt marks — and
  kitty ≥ 0.42 ships an official **quick-access-terminal kitten** ("Access
  kitty with a single keypress", quake-style layer-shell window; docs TOC
  <https://sw.kovidgoyal.net/kitty/kittens/quick-access-terminal/>).
- `allow_hyperlinks yes` processes OSC 8, with an `ask` value that confirms
  before opening a clicked link (conf#opt-kitty.allow_hyperlinks).

### 6. Escape-sequence specs — who owns what

- **OSC 52** — xterm ctlseqs, "Manipulate Selection Data": base64 payload
  writes the selection; `?` queries it back; gated by xterm's
  `allowWindowOps` resource — i.e. even xterm treats it as disable-able
  window op
  (<https://invisible-island.net/xterm/ctlseqs/ctlseqs.html>, OSC Ps=52; patch
  #411, 2026-08-23). xterm's OSC table has **no OSC 7, 8, 9, 133** — those all
  came from elsewhere.
- **OSC 7 (cwd)** — canonical form `OSC 7 ; file://host/path ST`, from VTE's
  `__vte_osc7` (`src/vte.sh.in:56-58` @ gitlab VTE `1a63c87`, emitted per
  prompt via `PROMPT_COMMAND`/`precmd_functions`); WezTerm's shell-integration
  doc notes the sequence "originated in the macOS Terminal application"
  (<https://wezterm.org/shell-integration.html>). Implemented by: VTE/GNOME
  Terminal, Konsole, kitty (feature `no-cwd`), WezTerm (shipped
  `assets/shell-integration/wezterm.sh`, distro-packaged to auto-activate),
  iTerm2 (as `OSC 1337 ; CurrentDir`), VS Code (`OSC 633 ; P ; Cwd=…`).
  *Not* by xterm; DropTerm gets cwd via `/proc` instead — equivalent locally,
  useless over SSH (which is also the spoofing argument for treating remote
  OSC 7 with care).
- **OSC 8 (hyperlinks)** — already handled by DropTerm's engine (cell
  `hyperlink()` in `src/terminal/widget.rs:1692-1711`); spec-wise it's the
  iterm2/kitty proposal, documented in kitty conf (`allow_hyperlinks`) and
  iTerm2's escape codes page.
- **OSC 133 (prompt marks)** — FinalTerm's "shell integration" sequences,
  documented by iTerm2 as its Shell Integration foundation: `A` prompt start,
  `B` command start, `C` pre-exec, `D ; exit-code` finish
  (<https://iterm2.com/documentation-one-page.html>, "FinalTerm escape
  sequences"; plain-text excerpt verified). VS Code supports 133 **and** its
  own `OSC 633 A/B/C/D/E;cmdline;nonce/P;Cwd=` superset — the nonce
  specifically "to prevent command spoofing" — and spells out the payoff:
  exit-code decorations, command navigation, sticky scroll, quick fixes,
  run-recent-command, go-to-recent-directory, reliable cwd
  (<https://code.visualstudio.com/docs/terminal/shell-integration>,
  "Supported escape sequences"). Ghostty's `notify-on-command-finish`
  explicitly *requires* OSC 133 or shell integration (§2). Nobody implements
  133 in the alacritty_terminal engine DropTerm embeds (vte parses-and-drops
  unknown OSCs), so this would be engine work.
- **OSC 9 (notification) / OSC 9;4 (progress)** — iTerm2: `OSC 9 ; message`
  posts a notification; `OSC 9 ; 4 ; st ; pr` drives a progress bar with
  states 0 clear / 1 percent / 2 error / 3 indeterminate / 4 paused (iterm2
  one-page doc, "Post a notification" + "Progress Bar"). ConEmu-originated;
  Ghostty gates both (`desktop-notifications`, `progress-style`, §2). rxvt's
  **OSC 777 ; notify ; Title ; Body** is the third notification spelling
  (supported by Ghostty's desktop-notifications alongside OSC 9).
- **Desktop notifications, done properly** — kitty's OSC 99 spec is the most
  complete (buttons, activation reports, filtering; §5). The freedesktop
  delivery side is the `org.freedesktop.Notifications` D-Bus spec
  (referenced from kitty's doc for categories).

### 7. The COSMIC/libcosmic environment (what is even possible today)

- **Portals** — xdg-desktop-portal-cosmic @ `4902f57` (2026-09-30) implements
  exactly: `org.freedesktop.impl.portal.Access; FileChooser; RemoteDesktop;
  Screenshot; Settings; ScreenCast` (`data/cosmic.portal`). **No
  GlobalShortcuts, no Notification, no Secret, no OpenURI, no Background.**
  Consequences: (a) DropTerm's custom-shortcut toggle is the only sanctioned
  global-key path (README is right); (b) notifications must go **directly to
  D-Bus `org.freedesktop.Notifications`**, not a portal — served on COSMIC by
  [pop-os/cosmic-notifications](https://github.com/pop-os/cosmic-notifications)
  ("Layer Shell notifications daemon which integrates with COSMIC", per the
  repo); (c) URL opening via `open`/xdg-open is correct because there is no
  OpenURI portal to use; (d) libcosmic's `dialog::file_chooser`
  (ashpd-backed, `src/dialog/file_chooser/open.rs:157`) *does* work on COSMIC
  since FileChooser is implemented.
- **libcosmic @ `08a09e3` (DropTerm's pinned rev; local cargo checkout)**:
  `cosmic::applet` module (panel-applet `Context`, `Size`, `PanelAnchor`,
  column/row layout, tooltips, `cosmic::applet::run` —
  `src/applet/mod.rs:1-60`, plus a complete `examples/applet`); `zbus` 5.15 is
  a first-class (optional) dependency and `single-instance`/`dbus_activation`
  are built on it (`Cargo.toml:68-116`, `src/dbus_activation.rs`);
  `widget::toaster` for in-app toasts (`src/widget/mod.rs:313`);
  `command::file_transfer_send/receive` via ashpd (`src/command.rs:51-67`);
  `desktop.rs` DesktopEntryCache for launching apps. There is **no
  notification helper** at this rev (grep `notif` in `src/`: zero hits) — use
  `notify-rust` (or raw zbus) as a direct dependency, the way cosmic-term
  added `secret-service` rather than waiting for toolkit support.
- **Panel applets** — applets are separate binaries identified by app id
  (`com.system76.CosmicApplet*` listed in cosmic-panel-config's default
  container config, cosmic-panel @ `f4ba4ef`,
  `cosmic-panel-config/src/container_config.rs:151-156`); libcosmic's applet
  module hosts them in the panel via layer-shell. A DropTerm applet would be a
  second process talking to the main one over the existing single-instance
  D-Bus seam.
- **Autostart** — no Background/autostart portal on COSMIC, but the classic
  XDG `~/.config/autostart/*.desktop` mechanism works (no sandbox), matching
  Yakuake `OpenAfterStart`/Guake start-at-login rather than a portal.

### 8. Briefly: WezTerm & Alacritty conventions

- **WezTerm**: shell integration = OSC 7 + OSC 133 + OSC 1337 user vars
  (`WEZTERM_PROG/USER/HOST/IN_TMUX`), shipped as shell scripts, distro packages
  auto-activate them (<https://wezterm.org/shell-integration.html>). Its CLI
  `wezterm start --cwd` explicitly notes the cwd comes from OSC 7; it's a
  multiplexer (cli subcommands `connect/ls/set-tab-title/…`) — the
  remote-control-rich model DropTerm's single-instance D-Bus echoes in
  miniature.
- **Alacritty**: config-first, no D-Bus remote control at all; OSC 52 policy
  enum `Disabled|OnlyCopy|OnlyPaste|CopyPaste` default `OnlyCopy`; visual-bell
  table (animation/color/duration/command); hints with `hyperlinks = true`
  inclusion (`extra/man/alacritty.5.scd` L495-530, 618-624, 685-694). DropTerm
  inherits the engine's defaults verbatim today.

## Ranked applicability

Ranked by applicability × effort × dependency risk (libcosmic rev / portal
status). "Blocked" marks the known constraints.

| # | Integration | Applicability | Effort | Dep. risk | Why |
|---|---|---|---|---|---|
| 1 | **Bell → desktop notification** (BEL and/or OSC 9/777 → `org.freedesktop.Notifications` via notify-rust; config gate like kitty's `window_alert_on_bell`/Ghostty's bell-features) | High | Low | None | The one DE feature a *hidden* terminal uniquely needs: `TermEvent::Bell` is currently discarded (`src/app.rs:2917`); cosmic-notifications serves the D-Bus interface (no portal exists or is needed); precedent everywhere (kitty, iTerm2, Ghostty bell-features/notify-on-command-finish). |
| 2 | **Per-tab activity / bell indicators** (blue-dot on new output in unfocused tab, 🔔 on bell) | High | Low | None | iTerm2 tab indicators + kitty `bell_on_tab 🔔` default; pure `tab_bar.rs`/`tabs.rs` state work — `Wakeup` events already arrive per tab (`src/app.rs:2973-2974`); makes multi-tab + notifications (row 1) coherent. |
| 3 | **New tab inherits active tab's cwd** (reuse `/proc` readlink already in `engine.rs:447`; optional OSC 7 acceptance later) | High | Very low | None | Guake `open-tab-cwd=true` default, cosmic-term `tab_new_inherit_working_directory`, Ghostty `tab-inherit-working-directory=true` — unanimous convention; DropTerm already reads the cwd for session saves, just doesn't feed `spawn_tab(None)` (`src/app.rs:432-441`). |
| 4 | **CLI/D-Bus actions**: `--new-tab [cwd]`, `--cmd`, `--is-visible` (extend `ToggleAction`, `src/app.rs:49-55`, into the existing `ActivateAction` parse `src/app.rs:2900-2903`) | High | Low | None | Guake's whole surface is exactly this (§4); enables scripting, the startup-script pattern, and rows 8/9. Yakuake exposes 45+ such slots. |
| 5 | **Numbered tab switching + tab-move binds** (Ctrl+Shift+1-9, Ctrl+Shift+PageUp/PageDown; port cosmic-term `shortcuts.rs:482-494` semantics) | High | Very low | None | cosmic-term just shipped the exact binds on the same engine; trivially additive to `src/keybinds.rs`'s fixed map. |
| 6 | **Search in scrollback** (Ctrl+Shift+F regex find bar; port cosmic-term `Terminal::search` + find UI) | Med-high | Medium | None | The top cosmic-term feature DropTerm lacks; `RegexSearch` already vendored (`src/terminal/engine.rs:116-119,246`); no engine fork needed (cosmic-term searches through the same `Term` API). |
| 7 | **Customizable keybinds** (cosmic-term's `ShortcutsConfig` defaults+custom+Disable model, cosmic-config-backed) | Med | Medium | None | cosmic-term's model is proven on the same stack (`src/shortcuts.rs:154-261`); unlocks regional-layout users; pairs with rows 5/6. |
| 8 | **Session restore on login** (XDG autostart `.desktop` + a start-hidden action; no portal needed) | Med | Low | None | Yakuake `OpenAfterStart`, Guake start-at-login; DropTerm's default `Toggle` action *shows* on launch (`src/app.rs:2074-2078`) — needs a `--hide`/start-hidden variant; sessions already restore at every launch. |
| 9 | **Panel applet toggle** | Med | Med-high | Low | libcosmic@`08a09e3` ships `cosmic::applet` + `examples/applet` (§7); separate binary toggling via row 4's D-Bus; Yakuake's tray icon (`ShowSystrayIcon=true`) is the precedent; ROADMAP "Later". |
| 10 | **OSC 7 acceptance** (optional shell-reported cwd; prefer `/proc` when local) | Med | Low-med | None | Every VTE/kitty/WezTerm shell script already emits it (§6); strictly *additive* to row 3; mind remote-spoofing (VS Code adds a nonce for 633; kitty gates reads) — validate `file://` and same-host only. |
| 11 | **Bell extras**: urgency/attention + 🔔 tab-title prefix, optional visual bell | Med | Low | None | kitty `bell_on_tab`/`window_alert_on_bell`, Ghostty `bell-features {attention,title}` defaults; layer-shell urgency support on cosmic-comp unverified — ship the tab-title parts first. |
| 12 | **Toasts** for config-reload/clipboard-copy (libcosmic `widget::toaster`) | Low-med | Low | None | Ghostty `app-notifications` precedent; nice-to-have once settings window is open less. |
| 13 | **OSC 52 policy knob** (`Disabled/OnlyCopy/OnlyPaste/CopyPaste` config) | Low | Very low | None | Engine already supports the enum (`alacritty_terminal Config::osc52`); current default = industry majority; only for completeness. |
| 14 | **Password manager** (Secret Service CRUD + paste, cosmic-term `password_manager.rs` pattern) | Low-med | Medium | Low | Direct D-Bus Secret Service (gnome-keyring works on Pop!_OS); no portal involved; ~500 lines proven in cosmic-term; ROADMAP does not list it — pull in only on demand. |
| 15 | **DnD file drop → paste quoted paths** (cosmic-term `dnd.rs` `text/uri-list`) | Med | Medium | Med | cosmic-term does it on the same widget stack; needs iced dnd plumbing in the layer surface; Guake quotes dropped filenames similarly. |
| 16 | **Quick Open** (Ctrl+click filename → editor at line:col) | Med | Medium | Low | Guake feature, **off by default** there; builds on the existing Ctrl+click URL seam (`src/terminal/widget.rs:1463-1467`); needs a path+line regex and config. |
| 17 | **Monitor selection (named output)** | High demand | — | **BLOCKED** | Pinned libcosmic `IcedOutput` = `All|Active|Output(WlOutput)` with no by-name targeting/events (`src/surface.rs:62-71`, ROADMAP); Yakuake/Guake ship it (`Screen`, `display-n`), Ghostty's `quick-terminal-screen` is macOS-only. Revisit at a rev bump. |
| 18 | **Self-registered global hotkey** | — | — | **BLOCKED** | No GlobalShortcuts portal on COSMIC (§7 portal list); COSMIC custom shortcut re-launching is the sanctioned path (README). |

## Rejected, and why

- **Shell-integration auto-injection** (Ghostty `detect`, kitty `enabled`,
  WezTerm distro packaging): powerful but a large, security-sensitive surface —
  shipping shell scripts for 5 shells, injecting env (`TERM_PROGRAM`-gated),
  and maintaining them. DropTerm's `/proc` cwd covers the drop-down's actual
  needs (rows 3/10) without touching user dotfiles. Revisit only if OSC 133
  features (prompt marks, command-finish notify) become a goal.
- **OSC 133 / OSC 633 prompt marks & command awareness**: not rejected on
  merit (VS Code and iTerm2 show the payoff) but on cost — the
  alacritty_terminal grid records no marks, so this is engine-fork territory;
  633 specifically is VS Code-private and `TERM_PROGRAM=vscode`-gated by
  design. Do the bell/notification path (row 1) first, which needs no shell
  cooperation.
- **kitty OSC 5522 advanced clipboard**: single implementer, zero client
  demand on a COSMIC drop-down; OSC 52 (copy-only) is the interoperable
  subset everything speaks.
- **Notification / OpenURI portals**: not implemented by
  xdg-desktop-portal-cosmic (§7) — using them would silently no-op. Direct
  D-Bus notifications and `open`/xdg-open are the correct COSMIC-native
  mechanisms today.
- **Yakuake mouse-edge auto-open** (`PollMouse`): polling the pointer against
  the screen edge to open the window — a gimmick even Yakuake ships disabled;
  conflicts with DropTerm's toggle-only interaction model.
- **Yakuake skins / Guake palette-count one-upmanship**: theming belongs to
  the COSMIC system theme + DropTerm's scheme registry (see
  terminal-theming.md), not per-app skin bundles.
- **Ghostty `quick-terminal-space-behavior`**: macOS Spaces concept; no COSMIC
  equivalent (workspaces are compositor-side).
- **iTerm2-style deep integrations** — tmux control-mode, buried sessions,
  instant replay, coprocesses, Python API: excellent but orthogonal to
  drop-down DE integration; each is a project, none is on the ROADMAP.
- **tmux/multiplexer-style remote control (kitty `@`, WezTerm mux)**: remote
  control powerful enough to read the screen and send keystrokes needs
  kitty-grade permission machinery (passwords, per-action grants); Guake/
  Yakuake's narrow action-menu (row 4) is the right scope for DropTerm.
- **Flatpak packaging**: non-goal per README — layer-shell and
  toplevel-management aren't exposed to sandboxes on cosmic-comp.
- **Fullscreen toggle** (cosmic-term F11): a percent-height drop-down already
  reaches 100 % via config/drag; a distinct fullscreen mode adds a state for
  no new capability.
- **Changing the hide-on-focus-loss default**: not a new integration, but note
  DropTerm ships the minority default (hide; peers keep open — see TL;DR).
  Any behavior change should be a deliberate product decision with the README
  corrected either way, since the current prose mis-cites the peers.

## Sources

- DropTerm local tree: `src/main.rs`, `src/app.rs`, `src/config.rs`,
  `src/keybinds.rs`, `src/surface.rs`, `src/session.rs`,
  `src/terminal/{mod,engine,widget}.rs`, `Cargo.toml`, `README.md`,
  `ROADMAP.md`
- pop-os/cosmic-term @ `3ef31d750d3b09a73f266923b0d64c5df038cf9f` (master,
  2026-10-01): `src/shortcuts.rs` (whole file, 588 L), `src/config.rs`
  L189-244, `src/terminal.rs` L440-575, `src/main.rs` L95-145, L391-531,
  L1029-1046, L2381-2401, L2484-2513, L2996-3024, `src/password_manager.rs`,
  `src/dnd.rs`, `Cargo.toml` — <https://github.com/pop-os/cosmic-term>
- Ghostty config reference (read 2026-10-02):
  <https://ghostty.org/docs/config/reference> — `quick-terminal-position`,
  `-size`, `-screen`, `-animation-duration`, `-autohide`, `-space-behavior`,
  `-keyboard-interactivity`, `gtk-quick-terminal-layer`, `-namespace`,
  `shell-integration(-features)`, `cursor-click-to-move`,
  `notify-on-command-finish(-action/-after)`, `bell-features`,
  `desktop-notifications`, `progress-style`, `app-notifications`,
  `clipboard-read/write`, `gtk-single-instance`,
  `window/tab/split-inherit-working-directory`
- Yakuake @ invent.kde.org master `9ddf4ba3106cdd3cc78f641895c32aec923069bd`
  (2026-09-26): `app/config/yakuake.kcfg` (whole file, 207 L),
  `app/mainwindow.{h,cpp}` L67/L76, `app/sessionstack.h` L45-108,
  `app/tabbar.h` L52-56 — <https://invent.kde.org/utilities/yakuake>;
  feature summary <https://apps.kde.org/yakuake/>
- Guake @ github master `0911ddfe25a0c4019d2516b90982854d192e91cd`
  (2026-08-19): `guake/dbusiface.py` (whole file, 217 L), `guake/main.py`
  L90-398, `guake/data/org.guake.gschema.xml` L19-303,
  `docs/source/user/{cli,dbus}.rst` — <https://github.com/Guake/guake>;
  homepage <https://guake.github.io/>
- kitty docs (read 2026-10-02, current for 0.49.x):
  <https://sw.kovidgoyal.net/kitty/conf/> (`clipboard_control`,
  `clipboard_max_size`, `allow_remote_control`, `listen_on`,
  `shell_integration`, bell options, `allow_hyperlinks`, `confirm_close`),
  <https://sw.kovidgoyal.net/kitty/desktop-notifications/> (OSC 99 spec),
  <https://sw.kovidgoyal.net/kitty/clipboard/> (OSC 5522 spec),
  quick-access-terminal kitten TOC
- xterm ctlseqs (patch #411, 2026-08-23):
  <https://invisible-island.net/xterm/ctlseqs/ctlseqs.html> — OSC table
  (Ps=0..52; no OSC 7/8/9/133), OSC 52 "Manipulate Selection Data"
- iTerm2 documentation (one-page, read 2026-10-02):
  <https://iterm2.com/documentation-one-page.html> — FinalTerm OSC 133
  section, `OSC 9` notification, `OSC 9;4` progress states,
  `OSC 1337;CurrentDir`, hotkey window, Notification Center alerts,
  clipboard consent prefs, tab activity indicators
- VS Code shell integration docs (read 2026-10-02):
  <https://code.visualstudio.com/docs/terminal/shell-integration> — OSC 633
  A/B/C/D/E/P incl. Cwd property and anti-spoofing nonce; OSC 133 support;
  feature list (decorations, sticky scroll, quick fixes, cwd detection)
- GNOME VTE @ gitlab `1a63c87b0702c112406229f3cf98c389d51ca4d4` (2026-09-17):
  `src/vte.sh.in` L56-136 (`__vte_osc7` emits `OSC 7;file://host/path`) —
  <https://gitlab.gnome.org/GNOME/vte>
- WezTerm shell integration: <https://wezterm.org/shell-integration.html>
  (OSC 7 origin note, OSC 133 zones, OSC 1337 user vars, distro activation);
  CLI reference (`wezterm start --cwd` / OSC 7)
- Alacritty @ master: `extra/man/alacritty.5.scd` L495-530 (bell), L618-624
  (`terminal.osc52`), L685-694 (hints hyperlinks) —
  <https://github.com/alacritty/alacritty>
- alacritty_terminal 0.25.1 (local cargo registry, Apache-2.0):
  `src/term/mod.rs` L345-390 (`Config::osc52`, `Osc52` enum, default
  `OnlyCopy`), L1706-1728 (store/load gating); `src/event.rs` L18-51
  (Title/ResetTitle/Bell/ClipboardStore events)
- libcosmic @ `08a09e3dfa1f729d52585fcedd6e731130235363` (DropTerm's pinned
  rev, local cargo checkout): `src/applet/mod.rs` + `examples/applet`,
  `src/dialog/file_chooser/open.rs:157`, `src/command.rs:51-67`,
  `src/widget/mod.rs` (`toaster`), `Cargo.toml` (zbus 5.15, ashpd,
  single-instance, applet features) —
  <https://github.com/pop-os/libcosmic/tree/08a09e3dfa1f729d52585fcedd6e731130235363>
- xdg-desktop-portal-cosmic @ `4902f57242bc79f78588e245e5636e0d722c694a`
  (2026-09-30): `data/cosmic.portal` (Interfaces list),
  `src/{access,app,file_chooser,remote_desktop,screenshot,screencast,main}.rs`
  — <https://github.com/pop-os/xdg-desktop-portal-cosmic>
- cosmic-panel @ `f4ba4efb89bf070df91efe291272788e6d009da7` (2026-10-01):
  `cosmic-panel-config/src/container_config.rs` L151-156 (applet app-ids) —
  <https://github.com/pop-os/cosmic-panel>
- cosmic-notifications (COSMIC notifications daemon serving
  org.freedesktop.Notifications): <https://github.com/pop-os/cosmic-notifications>
