# Research: how should the settings window be visually triggered?

Date: 2026-10-02
Question: *What visual affordance should open DropTerm's existing settings window — and what do quake terminals and COSMIC apps actually use?*

## TL;DR

**Use the right-click context menu on the terminal as the primary trigger — a
"Settings" item in a libcosmic `context_menu`, exactly like cosmic-term, the app
DropTerm's terminal widget is adapted from.** Across every primary source
surveyed, quake/drop-down terminals expose settings through *whichever chrome is
always present*: Yakuake through a menu button on its persistent title bar,
Guake/Tilda/QTerminal exclusively through right-click context menus (they have no
other chrome), and Windows Terminal through the chevron at the end of its
*always-present* tab row. DropTerm's only always-present chrome is an 8px drag
handle (the grab bar), and its tab bar vanishes with a single tab — so the tab
bar cannot be the sole trigger without becoming persistent, and the grab bar is
too small and semantically wrong for a button. The right-click on the terminal
area is today completely unused in DropTerm (outside mouse-reporting passthrough)
and is the one surface every comparable app converges on. No COSMIC app surveyed
uses a gear *button* — COSMIC apps (cosmic-term, cosmic-edit, cosmic-files) put
"Settings" in menus (menu bar and/or context menu). If a gear icon is ever used,
the Pop icon name is `preferences-system`(-symbolic); **`settings-configure` does
not exist in Pop's icon theme** (it is the KDE/Breeze name).

## Findings

### 1. Yakuake (KDE) — menu button on the title bar; not the context menus

From [KDE/yakuake](https://github.com/KDE/yakuake) @ `master` (fetched 2026-10-02):

- Yakuake's always-present title bar carries three buttons: keep-open, **"Open
  Menu"** (a `QPushButton` with `mainWindow->menu()`, tooltip *"Open Menu"*), and
  quit — `app/titlebar.cpp:33-49` (menu button at L41-46).
- That main menu's "Settings" section contains *Manage Profiles*, *Configure
  Keyboard Shortcuts*, *Configure Notifications*, and **"Configure Yakuake"**
  (`KStandardAction::preferences` → `MainWindow::configureApp`) —
  `app/mainwindow.cpp:243` (action creation), `:741-746` (`setupMenu`, the
  "Settings" section), `:876-896` (`configureApp` builds the KPageWidget dialog).
- The *same* menu doubles as the system-tray icon's context menu
  (`app/mainwindow.cpp:943` `m_notifierItem->setContextMenu(m_menu)`).
- The **tab context menu does NOT contain** Configure Yakuake — it is
  split/rename/move/close actions (`app/tabbar.cpp:107-128`). The terminal
  (`app/terminal.cpp`) has no context-menu handling at all: right-click goes to
  the embedded Konsole part.

### 2. Guake — Preferences reachable only via context menus

From [Guake/guake](https://github.com/Guake/guake) @ `master` (fetched 2026-10-02):

- The **terminal right-click context menu** ends with `gtk-preferences` →
  `on_show_preferences`, plus About and Quit — `guake/menus.py:167-186`
  (`mk_terminal_context_menu`). It is triggered from `guake/boxes.py:524-547`:
  button 3, but only **after VTE declines the event** — i.e. mouse-reporting
  applications consume right-click first, exactly the interaction DropTerm's
  `mouse_reporter` must preserve.
- Right-clicking the **notebook tab strip** opens `mk_notebook_context_menu`,
  which *also* contains `gtk-preferences` → `on_show_preferences` —
  `guake/notebook.py:131-132` and `guake/menus.py:35-53`.
- The **tab** context menu (`guake/menus.py:13-32`) has no Preferences; there is
  **no visible button anywhere** in Guake's window — the context menu is the only
  visual trigger.
- `show_prefs` opens the dialog — `guake/guake_app.py:561-568`.

### 3. cosmic-ext-quake-terminal — settings window exists, but no visual trigger

From [M0Rf30/cosmic-ext-quake-terminal](https://github.com/M0Rf30/cosmic-ext-quake-terminal)
@ `main` (fetched 2026-10-02):

- It *does* have a settings window (`src/app.rs:195-208` opens a second window
  titled "Settings"; the content is built from libcosmic's
  `widget::settings::{section, item, view_column}` — `src/app.rs:255-284`).
- Its only triggers are **non-visual**: a CLI subcommand (`src/app.rs:147-151`)
  and a D-Bus activation action (`src/app.rs:355-367`). There is no gear button
  and no context menu — the app hosts a *third-party terminal window*
  (`README.md`: "spawns your configured terminal emulator"), so it owns no
  terminal surface to hang a trigger on. This is the discoverability failure
  mode DropTerm is trying to avoid, not a pattern to copy.

### 4. QTerminal and Tilda (brief) — right-click, both

- QTerminal: terminal right-click context menu includes the `PREFERENCES`
  action ("Preferences…") as its last item —
  [lxqt/qterminal](https://github.com/lxqt/qterminal) `src/termwidget.cpp:224-258`
  (actions leased from the main window), also in the File menu
  (`src/mainwindow.cpp:426`).
- Tilda: right-click context menu has a preferences section —
  [lanoxx/tilda](https://github.com/lanoxx/tilda) `src/tilda-context-menu.c:123-130`
  (callback opens the config wizard), `:213-217` (`_Preferences` menu entry).

### 5. Windows Terminal — chevron at the end of the tab row (the always-present bar)

From [microsoft/terminal](https://github.com/microsoft/terminal) @ `main`
(fetched 2026-10-02):

- The dropdown attached to the **new-tab/chevron button at the end of the tab
  row** lists profiles, a separator, then static items: **Settings (with
  `Symbol::Setting` — a gear — and the `Terminal.OpenSettingsUI` key chord shown
  as an accelerator)**, Command Palette, About —
  `src/cascadia/TerminalApp/TerminalPage.cpp:1022-1148` (`_CreateNewTabFlyout`;
  settings item at L1048-1067).
- Default binding for the settings UI: **`ctrl+,`** —
  `src/cascadia/TerminalSettingsModel/defaults.json:728` (command ids at
  L522-525).
- Official docs describe the same path: *"To customize the settings of your
  Windows Terminal, select Settings in the dropdown menu."*
  ([Windows Terminal installation, Microsoft Learn](https://learn.microsoft.com/en-us/windows/terminal/install);
  the `openSettings` action and defaults:
  [Actions page](https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions)).
- Note the precondition: Windows Terminal's **tab row is always visible**, so the
  chevron is always reachable. DropTerm's tab bar is not (see below).

### 6. cosmic-term — the direct ancestor: menu bar item + right-click, no gear button

From [pop-os/cosmic-term](https://github.com/pop-os/cosmic-term) @ `master`
(fetched 2026-10-02) — the app DropTerm's terminal widget is adapted from
(`src/terminal/widget.rs` in this repo):

- **Right-click context menu on the terminal** contains New Tab, Copy/Paste,
  Select All, Clear Scrollback, splits, the window-header checkbox, and — last
  item — **"Settings…"** — `src/menu.rs:42-93` (`context_menu`, settings push at
  L74; label `menu-settings = Settings...` in `i18n/en/cosmic_term.ftl:141`).
  The menu is opened through libcosmic's `widget::context_menu` wrapper
  (`src/main.rs:3444-3450`), fed by `terminal_box`'s right-click handling
  (`src/terminal_box.rs:1354-1376`, including hyperlink detection).
- The same `menu-settings` is the closing item of the **menu bar**, which
  cosmic-term places in its header bar — `src/menu.rs:223` and
  `src/main.rs:3329-3333` (`header_start` returns `vec![menu_bar(...)]`).
- Both routes funnel to `Message::ToggleContextPage(ContextPage::Settings)`
  (`src/main.rs:323`), rendered as a libcosmic **context drawer**
  (`src/main.rs:3262-3293`) — a slide-in panel inside the same window, not a
  separate window.
- cosmic-term's tab bar is **also ephemeral**: rendered only
  `if tab_model.iter().count() > 1` (`src/main.rs:3383-3385`) — the behavior
  DropTerm copied. Its persistent header chrome is only the menu bar and a "+"
  icon button (`header_end`, `list-add-symbolic`, `src/main.rs:3335-3344`).
  **There is no gear/settings icon button anywhere in cosmic-term.**

### 7. cosmic-edit and cosmic-files — Settings in the menu bar, context drawer

Both @ `master` (fetched 2026-10-02):

- [pop-os/cosmic-edit](https://github.com/pop-os/cosmic-edit):
  `MenuItem::Button(fl!("menu-settings"), None, Action::ToggleSettingsPage)` in
  its `responsive_menu_bar` (`src/menu.rs:337`) →
  `Message::ToggleContextPage(ContextPage::Settings)` (`src/main.rs:289`) →
  context drawer (`src/main.rs:2991-3008`).
- [pop-os/cosmic-files](https://github.com/pop-os/cosmic-files):
  `menu::Item::Button(fl!("menu-settings"), None, Action::Settings)` in the
  View menu (`src/menu.rs:707`) → `ToggleContextPage(ContextPage::Settings)`
  (`src/app.rs:263`) → context drawer (`src/app.rs:5470`).

Pattern: **every surveyed COSMIC app puts "Settings" in a menu** (menu bar
item and/or right-click context menu) and never behind a standalone icon
button; the settings UI itself is a context drawer inside the app window.

### 8. libcosmic @ DropTerm's pinned rev — what the toolkit already provides

Verified in the local checkout of [pop-os/libcosmic @ `08a09e3`]
(https://github.com/pop-os/libcosmic/tree/08a09e3dfa1f729d52585fcedd6e731130235363)
(DropTerm pins this rev — `Cargo.toml:30-33`):

- **`widget::context_menu`** (`src/widget/context_menu.rs:23-72`): wraps any
  content element; opens a `menu::Tree` on right-click; Wayland popup creation
  built in (`create_popup`, L94+); `on_open`/`on_close` messages,
  `close_on_escape`, `item_width` — all public. This is the exact widget
  cosmic-term wraps its terminal box in.
- **`widget::menu`** (`src/widget/menu.rs` exports; `MenuAction` trait at
  `src/widget/menu/action.rs:41-52`: a `Copy + Eq` action type plus a
  `message()` mapping — the cosmic-term `TabAction` pattern,
  `src/menu.rs:32-40`), and `responsive_menu_bar` for a header-bar menu.
- **`context_drawer`** (`src/app/context_drawer.rs:9-32`) — the settings-panel
  container cosmic-term/edit/files use.
- `widget::settings::{section, item, view_column}` — the settings-page building
  blocks (`src/widget/settings/section.rs:10-20`), also used by
  cosmic-ext-quake-terminal's settings window (`src/app.rs:255-284`).
- Styling reference for DropTerm's tab bar — `SegmentedButton::TabBar` preset,
  `src/theme/style/segmented_button.rs:19,82,137,224-243` (already mirrored by
  `src/tab_bar.rs:34-38` in this repo).
- Popups from an exclusive layer surface are explicitly supported by
  cosmic-comp: *"Popups are judged by their root surface, so an exclusive
  surface can still open grabbing popups (menus, dropdowns, context menus)"* —
  cosmic-comp `src/shell/focus/mod.rs` ~L655-660, as quoted with citation in
  this repo's own [panel-visibility research note](panel-visibility.md)
  (§5). So a context menu popup works on DropTerm's layer surface while it
  holds exclusive keyboard focus.

### 9. Icon names — `preferences-system` on Pop, not `settings-configure`

From [pop-os/icon-theme](https://github.com/pop-os/icon-theme) @ `master`
(fetched 2026-10-02; the repo `pop-os/pop-icon-theme` no longer exists — the
theme lives here):

- Present: `preferences-system.svg` in 8 sizes (e.g. `Pop/16x16/categories/`,
  `Pop/128x128/categories/`), `preferences-system-symbolic.svg`
  (`Pop/scalable/categories/`), and `emblem-system` (7 sizes).
- **Absent: `settings-configure`** (0 hits) — that name is KDE/Breeze's, codified
  by the KDE HIG icon pages for its own theme
  ([KDE HIG: Category and Preferences Icons](https://develop.kde.org/hig/icons/colorful/category_preferences/)),
  and it will fall back/blank on Pop.
- Flagship usage: cosmic-settings names every page icon with
  `preferences-*-symbolic` strings (`page::Info::new("dock",
  "preferences-dock-symbolic")` etc.) —
  [pop-os/cosmic-settings](https://github.com/pop-os/cosmic-settings)
  `cosmic-settings/src/pages/desktop/dock/mod.rs`, `.../sound/mod.rs`,
  `.../applications/mod.rs` (@ `master`).

### 10. HIG guidance (brief)

- **GNOME HIG** — primary menus are where standard app-wide features live:
  *"Most apps have primary menus, and they are the location for standard
  features which every app is expected to have."* The standard-items table
  reads: *"Preferences — Opens the app's preferences dialog, if it has one"*,
  placed *"in a group at the end of the menu"*; the trigger is a menu button
  (`open-menu-symbolic`) at the end of the header bar —
  [GNOME HIG: Menus](https://developer.gnome.org/hig/patterns/controls/menus.html).
  GNOME's answer is always "menu item", not "gear button".
- **KDE** — the configuration dialog is a standard component (*"If you need a
  configuration dialog for your QtWidgets application, use KConfigDialog"* —
  [KConfigDialog docs](https://develop.kde.org/docs/features/configuration/configdialog/));
  apps attach it via `KStandardAction::preferences`, which is exactly what
  Yakuake does (finding 1). KDE HIG's settings philosophy ("apply immediately",
  don't make users relaunch) is at
  [Powerful when needed](https://develop.kde.org/hig/powerful_when_needed/).
  Neither HIG prescribes a gear *button*; both prescribe a labeled menu item.
- **COSMIC has no published HIG** for this — the operative convention is the
  first-party app suite (findings 6-7). Stated plainly: no primary source
  prescribes anything for COSMIC; the convention is inferred from pop-os's own
  apps, which are unanimous (menu item + context drawer).

## What DropTerm has to work with

Verified in this repo:

- **Ephemeral tab bar**: rendered only `if self.tabs.len() > 1` —
  `src/app.rs:2428-2460`; module doc: *"shown only while more than one tab
  exists"* (`src/tab_bar.rs:3-4`). Any trigger placed in the tab bar disappears
  in the default single-tab state — precisely the state where a new user is
  stuck without the keybind. The bar is also a fully custom widget
  (custom-rendered pills, drag-reorder, rename overlay — `src/tab_bar.rs`), so
  adding a trailing control means extending that widget by hand.
- **Persistent grab bar**: always present on the free edge
  (`src/app.rs:2423,2452-2467`), but it is an **8px** drag handle
  (`src/grab_bar.rs:47` `BAR_HEIGHT = 8.0`) whose entire semantics are "this is
  a resize handle" (`src/grab_bar.rs:3-6`); it swallows left presses for
  dragging. A click target inside it would fight the drag gesture and is far
  below any reasonable touch/click target size.
- **Unused right-click**: no context menu exists anywhere in the app (no
  `context_menu`/`Button::Right` handling outside mouse reporting). Right-click
  is only forwarded to the child process when it enables mouse reporting —
  `src/terminal/widget.rs:1272-1279,1417-1443` and
  `src/terminal/mouse_reporter.rs:51` (`Button::Right => Some(2)`). When mouse
  reporting is off, right-click falls through unhandled. Guake's precedent for
  this exact conflict: pass the event to the terminal first, show the menu only
  if the terminal declines (Shift bypass optional) — `guake/boxes.py:524-547`.
- **Settings window already exists**: `DropTerm::open_settings`
  (`src/app.rs:811-849`, one window per process, hides the drop-down because a
  normal window can never stack above the layer surface — comment at
  `src/app.rs:828-836`), currently reachable only via the `Action::Settings`
  keybind **Ctrl+Shift+I** (`src/keybinds.rs:54-61`) and the CLI startup flag.

## Recommendation

Ranked options for the visual trigger, fitted to DropTerm's anatomy:

| Rank | Option | Precedent | Fits DropTerm because | Cost |
|---|---|---|---|---|
| **1** | **"Settings" item in a right-click context menu on the terminal** (`widget::context_menu` + `menu::Tree`, settings last, plus Copy/Paste-type items if desired) | cosmic-term `src/menu.rs:74`; Guake `guake/menus.py:167-186`; QTerminal `termwidget.cpp:257`; Tilda `tilda-context-menu.c:216` | Available in **every** surface state (single tab, multi-tab, mid-drag never); zero new chrome; uses the pinned libcosmic widget as-is; popups work from exclusive layer surfaces (finding 8); preserves quake minimalism | Low: wrap the terminal element, add a message that calls `open_settings` |
| 2 | Persistent menu/gear button at the end of the tab bar | Yakuake title-bar menu button (`titlebar.cpp:41-46`); Windows Terminal chevron (`TerminalPage.cpp:1022-1148`) | Most discoverable affordance in the genre *when the bar is always there* | High: must make the tab bar permanent (breaking the deliberate single-tab-no-chrome rule, `src/tab_bar.rs:3-4`) or accept the trigger vanishing with one tab; no COSMIC app does this |
| 3 | Button inside the grab bar | none found in any surveyed app | Only always-present strip | Rejected: 8px handle (`src/grab_bar.rs:47`), drag-gesture conflict, no precedent |

Supporting decisions for option 1, all evidence-backed:

- **Menu shape**: follow cosmic-term's exact composition — terminal-scoped
  actions first, `Settings…` last, preceded by a divider
  (`cosmic-term src/menu.rs:42-74`); Guake and QTerminal also end with
  Preferences/Settings (findings 2, 4). GNOME HIG's "group standard items at
  the end" (finding 10) agrees. Label it "Settings…" (cosmic-term and
  cosmic-files use `Settings...`; Yakuake "Configure Yakuake…"; Guake
  "Preferences") — "Settings…" is the COSMIC-suite name.
- **Mouse-reporting conflict**: keep DropTerm's existing passthrough first
  (`src/terminal/mouse_reporter.rs:51`) and open the menu only when the child
  isn't capturing the mouse — Guake's documented order of checks
  (`guake/boxes.py:524-534`). cosmic-term reaches the same design from the
  other side (its terminal box opens the menu on the release, from state the
  right-press recorded — `src/main.rs:3437-3443`, comment included).
- **Keyboard shortcut stays**: every surveyed app pairs the visual trigger with
  a keybind (Windows Terminal `ctrl+,`; Yakuake's KStandardAction; DropTerm
  already has Ctrl+Shift+I, `src/keybinds.rs:58-61`). The menu item and keybind
  should route to the same `open_settings` funnel (`src/app.rs:811`).
- **If any icon is ever wanted** (e.g. a future persistent bar): use
  `preferences-system-symbolic` / `preferences-system` — **not**
  `settings-configure`, which does not exist in Pop's theme (finding 9).

Open questions (flagged, not blocking):

- **Context drawer vs separate window**: cosmic-term/edit/files render settings
  *inside* the app window as a context drawer (finding 6-7), which sidesteps
  DropTerm's hide-the-terminal dance (`src/app.rs:828-837`). DropTerm already
  shipped a separate settings window with its own rationale (a normal window
  can never stack above the layer surface). Whether a drawer-style settings
  pane inside the drop-down would serve better is a design question for the
  app's owner — the answer here concerns only the *trigger*, and both UIs would
  use the same one.
- **Right-click on the tab bar / grab bar**: Guake puts Preferences on the
  notebook-strip context menu too (`guake/notebook.py:131-132`); Yakuake's tab
  context menu pointedly omits it (`app/tabbar.cpp:107-128`). If DropTerm later
  adds a tab context menu (rename/close are already gestures there), cosmic-term
  suggests keeping Settings terminal-scoped, not tab-scoped.
- Whether `emblem-system` or `preferences-system-symbolic` reads better at
  16px in DropTerm's custom-painted bars is a hardware-checklist question
  (per `AGENTS.md`, rendering needs real COSMIC hardware).

## Sources

- Yakuake @ master (fetched 2026-10-02): `app/titlebar.cpp` L33-49;
  `app/mainwindow.cpp` L243, L713-750, L876-896, L943; `app/tabbar.cpp`
  L107-128; `app/terminal.cpp` (no menu handling) —
  <https://github.com/KDE/yakuake>
- Guake @ master (fetched 2026-10-02): `guake/menus.py` L13-32, L35-66,
  L69-186; `guake/boxes.py` L524-547; `guake/notebook.py` L131-132;
  `guake/guake_app.py` L561-568 — <https://github.com/Guake/guake>
- cosmic-ext-quake-terminal @ main (fetched 2026-10-02): `src/app.rs`
  L147-151, L195-208, L255-284, L355-367 —
  <https://github.com/M0Rf30/cosmic-ext-quake-terminal>
- QTerminal @ master (fetched 2026-10-02): `src/termwidget.cpp` L224-265;
  `src/mainwindow.cpp` L426 — <https://github.com/lxqt/qterminal>
- Tilda @ master (fetched 2026-10-02): `src/tilda-context-menu.c`
  L123-130, L213-217 — <https://github.com/lanoxx/tilda>
- Windows Terminal @ main (fetched 2026-10-02):
  `src/cascadia/TerminalApp/TerminalPage.cpp` L1022-1148;
  `src/cascadia/TerminalSettingsModel/defaults.json` L522-525, L728 —
  <https://github.com/microsoft/terminal>; docs:
  <https://learn.microsoft.com/en-us/windows/terminal/install>,
  <https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions>
- cosmic-term @ master (fetched 2026-10-02): `src/menu.rs` L32-40, L42-93,
  L223; `src/main.rs` L323, L3329-3344, L3383-3385, L3444-3450;
  `src/terminal_box.rs` L1354-1376; `i18n/en/cosmic_term.ftl` L141 —
  <https://github.com/pop-os/cosmic-term>
- cosmic-edit @ master (fetched 2026-10-02): `src/menu.rs` L337;
  `src/main.rs` L289, L2991-3008 — <https://github.com/pop-os/cosmic-edit>
- cosmic-files @ master (fetched 2026-10-02): `src/menu.rs` L707;
  `src/app.rs` L263, L5470 — <https://github.com/pop-os/cosmic-files>
- libcosmic @ `08a09e3` (DropTerm's pinned rev, local cargo checkout):
  `src/widget/context_menu.rs` L23-72, L94+; `src/widget/menu.rs` (exports);
  `src/widget/menu/action.rs` L41-52; `src/app/context_drawer.rs` L9-32;
  `src/widget/settings/section.rs` L10-20;
  `src/theme/style/segmented_button.rs` L19, L82, L137, L224-243 —
  <https://github.com/pop-os/libcosmic/tree/08a09e3dfa1f729d52585fcedd6e731130235363>
- cosmic-comp popup-from-exclusive-surface rule: `src/shell/focus/mod.rs`
  ~L655-660, as quoted (with citation) in this repo's
  `docs/research/panel-visibility.md` §5 — <https://github.com/pop-os/cosmic-comp>
- pop-icon-theme → [pop-os/icon-theme](https://github.com/pop-os/icon-theme)
  @ master (fetched 2026-10-02): `Pop/*/categories/preferences-system*.svg`,
  `Pop/*/emblems/emblem-system.svg`; zero hits for `settings-configure`
- cosmic-settings @ master (fetched 2026-10-02): page icon strings in
  `cosmic-settings/src/pages/**/mod.rs` (`preferences-*-symbolic`) —
  <https://github.com/pop-os/cosmic-settings>
- GNOME HIG, Menus (primary menus / standard items table):
  <https://developer.gnome.org/hig/patterns/controls/menus.html>
- KDE: KConfigDialog docs <https://develop.kde.org/docs/features/configuration/configdialog/>;
  HIG Powerful when needed <https://develop.kde.org/hig/powerful_when_needed/>;
  HIG Category and Preferences Icons
  <https://develop.kde.org/hig/icons/colorful/category_preferences/>
- DropTerm itself: `src/app.rs` L811-849, L2423-2472; `src/tab_bar.rs`
  L3-4, L34-38; `src/grab_bar.rs` L3-6, L47; `src/keybinds.rs` L54-61;
  `src/terminal/widget.rs` L1272-1279, L1417-1443;
  `src/terminal/mouse_reporter.rs` L51; `Cargo.toml` L30-33
