# Research: per-tab context menus in peer terminals

Date: 2026-10-08
Question: *What do peer terminals — and Firefox/Chromium as the canonical
general-purpose tab menus — actually put in their tab(-header) context menus,
with which behavior nuances, and which items should DropTerm's per-tab context
menu carry when consolidated with its existing verbs?*

Scope (maintainer-directed): GNOME Terminal, Konsole, Yakuake, kitty, WezTerm,
Guake, Windows Terminal, Firefox, Chromium. Primary sources only — official
docs and project source at `master`/`main` (state as of 2026-10-08). Where a
menu does not exist (kitty, WezTerm), the tab-bar mouse grammar is documented
instead, since that is what a right-click would compete with.

## TL;DR

Seven peers have a per-tab right-click menu; two (kitty, WezTerm) deliberately
do not. Six findings drive the recommendation:

1. **Rename and Close are the only universal pair.** Every peer with a menu
   closes the clicked tab; every *terminal* peer with a menu renames it
   (browsers never do — their "rename" is the page's own title).
2. **The close cluster is the browser lineage's signature, and Windows
   Terminal adopted it wholesale.** Close Other Tabs / Close Tabs to the
   Right (Firefox, Chromium) appear in WT's tab menu as a "Close" submenu
   ([`Tab.cpp`](https://github.com/microsoft/terminal/blob/main/src/cascadia/TerminalApp/Tab.cpp)).
   No VTE/KDE/Guake peer has either item: GNOME Terminal, Konsole, Yakuake,
   Guake, and kitty all ship only single-tab close in the UI.
3. **Move Left/Right is a terminal-peer item, not a browser one.** GNOME
   Terminal has menu items, Yakuake menu items *disabled at the edges*, WT a
   submenu with the same edge-disabling (`_moveLeftMenuItem.IsEnabled(tabIndex
   > 0)`); Konsole and kitty keep it keybind/action-only with edge clamps.
   Browsers move to start/end/new-window instead.
4. **Every menu is scoped to the clicked tab, never the active one.** Konsole
   captures `_contextMenuTabIndex` from the hit test; Yakuake resolves
   `sessionAtTab(index)`; Guake resolves the page by its label event box;
   WT's menu is built by the clicked tab's own `Tab` object; Chromium's
   `TabMenuModel::Build(index)` takes the context index; Firefox dispatches on
   `TabContextMenu.contextTab`. Konsole even makes the clicked tab current
   before renaming it.
5. **Rename modality splits inline vs. dialog, with inline in the minority.**
   WT renames via a TextBox in the tab header (double-click does the same);
   Yakuake positions a `QLineEdit` directly over the tab; kitty's double-click
   opens an interactive `set_tab_title` prompt. Guake and Konsole use dialogs
   (Konsole's `RenameTabDialog` edits title formats plus tab/activity colors).
   DropTerm's inline rename matches the WT/Yakuake lineage.
6. **Nobody puts terminal-scoped verbs in the tab menu.** Copy/Paste/Clear
   Scrollback live in terminal-area menus in every peer (Guake's terminal
   menu, kitty's mouse map, GNOME Terminal's context). The only page-scoped
   verb browsers add is Reload Tab — a page notion with no terminal analog
   beyond Clear Scrollback, which stays terminal-scoped everywhere.

## Per-peer findings

### GNOME Terminal (master, GTK4/libadwaita rewrite)

Menu model in [`terminal-notebook.ui`](https://gitlab.gnome.org/GNOME/gnome-terminal/-/blob/master/src/terminal-notebook.ui)
(`page_menu` on the `AdwTabView`), actions installed in
[`terminal-window.cc`](https://gitlab.gnome.org/GNOME/gnome-terminal/-/blob/master/src/terminal-window.cc):

- **Items, in order**: Pin/Unpin Terminal · Move Terminal Left · Move Terminal
  Right · Detach Terminal · Set Title… · Close Terminal. Pin exists here (of
  all terminal peers, only GNOME Terminal puts pin in the tab menu).
- **Set Title…** sits in the menu model behind
  `hidden-when="action-missing"` on action `notebook-set-title`; the working
  verb in master is `win.set-title` in the header menu
  ([`terminal-headermenu.ui`](https://gitlab.gnome.org/GNOME/gnome-terminal/-/blob/master/src/terminal-headermenu.ui)).
  Its dialog implementation was not located in the files surveyed — unverified.
- **Move** is `terminal_notebook_reorder_tab(±1)` with RTL-aware direction
  inversion; **Detach** is enabled only when `n_pages > 1`
  ([`terminal-notebook.cc`](https://gitlab.gnome.org/GNOME/gnome-terminal/-/blob/master/src/terminal-notebook.cc)).
- **No Close Other Tabs / Close to Right** anywhere in the notebook actions.
- **Close confirmation**: close requests funnel through
  `confirm_close_window_or_tab()` and the window owns a
  `confirm_close_dialog` (terminal-window.cc) — a single dialog, not per-tab
  prompts.
- The menu targets the tab passed to the `setup-menu` signal
  (`terminal_notebook_setup_menu` extracts the clicked page's tab) — clicked,
  not active.

### Konsole (master)

Tab-bar context menu built in
[`src/widgets/ViewContainer.cpp`](https://invent.kde.org/utilities/konsole/-/blob/master/src/widgets/ViewContainer.cpp):

- **Items**: Detach Tab · "Configure or Rename Tab…" · Close Tab, plus a
  dynamically inserted read-only toggle. Detach is disabled when only one tab
  exists. A profile/container submenu hangs off the *new-tab button*, not
  this menu.
- **Scoping**: `openTabContextMenu` stores `_contextMenuTabIndex =
  tabBar()->tabAt(point)`; every action uses that index, and `renameTab()`
  also calls `setCurrentIndex(index)` first — right-click-rename activates
  the clicked tab.
- **Rename** is a dialog: `SessionController::rename()` opens
  `RenameTabDialog` with local/remote title formats, tab color, and activity
  color ([`src/session/SessionController.cpp`](https://invent.kde.org/utilities/konsole/-/blob/master/src/session/SessionController.cpp)).
- **Move Left/Right** exist as `moveTabLeft()/moveTabRight()` methods used by
  keyboard shortcuts, with hard early-returns at the edges — not menu items.
- **No Close Other Tabs**; the only close action anywhere is `close-session`.
  Monitor activity/silence exist as session-level `monitor-*` notifications,
  surfaced in the session (terminal-area) menu domain, not the tab menu.

### Yakuake (master)

Drop-down peer. Menu built in `readyTabContextMenu()` in
[`app/tabbar.cpp`](https://invent.kde.org/utilities/yakuake/-/blob/master/app/tabbar.cpp):

- **Items, in order**: Split Left/Right · Split Top/Bottom · — · Edit Current
  Profile… · Rename Session… · Prevent Closing (checkable) · Disable Keyboard
  Input (submenu when split) · Monitor for Activity (submenu) · Monitor for
  Silence (submenu) · — · Move Session Left · Move Session Right · — · Close
  Active Terminal · Close Session. Right-click *past* the tabs opens the
  new-session menu instead (new session, 2-horizontal, 2-vertical, quad).
- **Move left/right disabled at the edges**: `updateMoveActions()` re-enables
  left only off the first tab, right only off the last.
- **Rename is inline**: `interactiveRename()` positions a `QLineEdit` exactly
  over the tab; Return applies, Escape cancels; double-click does the same.
- **Scoping**: `sessionAtTab(index)` resolves the clicked session for every
  action. No close-others, no detach.

### kitty — no tab context menu at all

The tab bar's mouse grammar is in `TabManager.handle_tab_bar_mouse()`
([`kitty/tabs.py`](https://github.com/kovidgoyal/kitty/blob/master/kitty/tabs.py)):
left click activates, **double-click opens the interactive rename prompt**
(`set_tab_title`), **middle click closes the tab**, a click on the `+`/empty
area makes a new tab, and **right-click is not handled** (only LEFT and
MIDDLE appear in the handler). The documented
[actions](https://sw.kovidgoyal.net/kitty/actions/) carry the rest:
`close_tab` ("Close the current tab"), `close_other_tabs_in_os_window`,
`detach_tab`, `move_tab_forward`/`move_tab_backward`, `new_tab_with_cwd`,
`set_tab_title` ("Change the title of the active tab interactively").

### WezTerm — right-click is a navigator, not a menu

In [`wezterm-gui/src/termwindow/mouseevent.rs`](https://github.com/wezterm/wezterm/blob/main/wezterm-gui/src/termwindow/mouseevent.rs):
right-click on a tab runs `show_tab_navigator()`; middle-click runs
`close_specific_tab`; left-click activates. The `+` button's right-click is
`KeyAssignment::ShowLauncher` (the profile/domain launcher). The
[ShowTabNavigator docs](https://wezterm.org/config/lua/keyassignment/ShowTabNavigator.html):
"displays a list of tabs and allows you to select and activate a tab from
that list" — a fuzzy switcher, so rename/close are not menu items anywhere.

### Guake (master)

Drop-down peer. Menu in
[`guake/menus.py`](https://github.com/Guake/guake/blob/master/guake/menus.py)
(`mk_tab_context_menu`), wired via `TabLabelEventBox` right-click in
[`guake/boxes.py`](https://github.com/Guake/guake/blob/master/guake/boxes.py):

- **Items**: New Tab · Rename · Reset custom colors · Close. That is all.
- **Rename** opens `RenameDialog` (double-click does too) — dialog, not
  inline; renaming sets a sticky custom label (`custom_label_set`).
- **Close** honors the `prompt-on-close-tab` setting: `delete_page` shows
  `PromptQuitDialog` listing running foreground processes before killing.
  Middle-click closes too. Scoped to the clicked label's page via
  `find_tab_index_by_label(self)`.
- The **notebook-area** right-click is a different, app-level menu (New Tab ·
  Save Tabs · Restore Tabs · Preferences · About · Quit).

### Windows Terminal (main)

Menu built per-tab in
[`Tab.cpp`](https://github.com/microsoft/terminal/blob/main/src/cascadia/TerminalApp/Tab.cpp)
(`_CreateContextMenu`, enable states in `_EnableMenuItems`):

- **Items, in order**: Color… · Rename · Duplicate · Split · Move ▸ (Move to
  new window / Move right / Move left) · Export · Find · Restart session · — ·
  Close ▸ (Close tabs after / Close other tabs / Close pane) · Close.
- **Enable conditions**: `_closeOtherTabsMenuItem.IsEnabled(numOfTabs > 1)`;
  `_closeTabsAfterMenuItem.IsEnabled(tabIndex < numOfTabs - 1)`;
  **move left/right disabled at the edges** (`tabIndex > 0` / `< numOfTabs -
  1`); Duplicate/Export/Find/Restart only when the active content is a
  terminal.
- **Rename is inline**: `ActivateTabRenamer()` shows a TextBox in the tab
  header (double-click on the tab does the same); empty text resets
  (`ResetTabText`). No prompt on any close variant.

### Firefox (mozilla-central)

Markup in
[`main-popupset.inc.xhtml`](https://searchfox.org/mozilla-central/source/browser/base/content/main-popupset.inc.xhtml)
(`tabContextMenu`), handlers in
[`main-popupset.js`](https://searchfox.org/mozilla-central/source/browser/base/content/main-popupset.js):

- **Core items** (browser-relevant): Open New Tab · Reload Tab(s) · Mute ·
  Pin/Unpin · Duplicate · Bookmark Tab(s) · Move ▸ (Move to Start · Move to
  End · Move to New Window) · … · Close Tab(s) · Close Duplicate Tabs ·
  Close Multiple Tabs ▸ (Close Tabs to the Start · Close Tabs to the End ·
  Close Other Tabs) · Undo Close Tab.
- **Scoping**: every handler dispatches on `TabContextMenu.contextTab` — the
  clicked tab. **Close-tabs-to-the-end/other close immediately**, no
  confirmation (`gBrowser.removeTabsToTheEndFrom/removeAllTabsBut` with
  metrics args only); the safety net is Undo Close Tab sitting right below.

### Chromium (main)

Model built per context index in
[`chrome/browser/ui/tabs/tab_menu_model.cc`](https://chromium.googlesource.com/chromium/src/+/main/chrome/browser/ui/tabs/tab_menu_model.cc)
(`TabMenuModel::Build`), enablement in
[`tab_strip_model.cc`](https://chromium.googlesource.com/chromium/src/+/main/chrome/browser/ui/tabs/tab_strip_model.cc):

- **Core items**: New tab adjacent · Move to another window ▸ / Move to New
  Window · — · Reload · Duplicate · Pin/Unpin · Mute site · — · Close Tab ·
  Close Other Tabs · Close Tabs to the Right.
- **Enablement**: Close Tabs to the Right is `SetEnabledAt(...,
  IsContextMenuCommandEnabled(index, CommandCloseTabsToRight))`, and both
  close commands reduce to `HasIndicesClosedByCommand(...)` — enabled only
  when the command would actually close something (nothing at/after the last
  tab, or no other tabs). Duplicate and Close Other Tabs are unconditional
  in the model.

## Comparison

| Item | GNOME Term | Konsole | Yakuake | kitty | WezTerm | Guake | Win Terminal | Firefox | Chromium |
|---|---|---|---|---|---|---|---|---|---|
| Right-click menu? | yes | yes | yes | **no** | **no** (navigator) | yes | yes | yes | yes |
| Rename | model item | dialog | **inline** | dbl-click prompt | ✗ | dialog | **inline** | ✗ | ✗ |
| Close Tab | yes | yes | yes | middle-click | middle-click | yes | yes (+Close pane ▸) | yes | yes |
| Close Other Tabs | ✗ | ✗ | ✗ | action only | ✗ | ✗ | yes (submenu) | yes (submenu) | yes |
| Close to the Right | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | yes ("tabs after") | yes (+to start) | yes |
| Move Left / Right | yes | keybind only | yes, edge-off | actions only | ✗ | ✗ | submenu, edge-off | ✗ (start/end) | ✗ |
| Detach / Move to new window | Detach | Detach (off at 1 tab) | ✗ | `detach_tab` | ✗ | ✗ | Move to new window | Move to New Window | submenu |
| Duplicate | ✗ | ✗ | ✗ | compose (`launch --cwd`) | ✗ | ✗ | yes | yes | yes |
| Pin | **yes** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | yes | yes |
| New Tab in tab menu | ✗ (on `+`) | ✗ (on `+`) | ✗ (bar menu) | ✗ (on `+`) | ✗ (`+` → launcher) | yes | ✗ | yes | yes ("adjacent") |
| Watch activity/silence | ✗ | session menu | **yes** (submenus) | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Split | ✗ | ✗ | yes | ✗ | ✗ | ✗ | yes | (split-view items) | (split items) |
| Color on tab | ✗ | in rename dialog | ✗ | ✗ | ✗ | reset only | yes (picker) | ✗ | ✗ |
| Terminal verbs (copy/paste/…) | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ (Reload is page) | ✗ |

## Recommended item set for DropTerm

Ranked by peer precedent and checked against DropTerm's existing verbs
(`src/context_menu.rs`, `src/keybinds.rs`, `src/terminal/widget.rs`).
Every item must target the **clicked** tab — the one universal rule.

3+ peer precedents:

1. **Rename Tab** (6 terminal peers; inline in WT/Yakuake like DropTerm's).
   Route to the existing `start_rename` on the clicked tab — the menu is the
   keyboard-less twin of double-click, exactly WT's design. Only nuance:
   Konsole activates the clicked tab first; DropTerm need not, since its
   rename overlay is per-tab by `TabId` already.
2. **Close Tab** (every peer). The clicked tab's existing close path. Peers
   put the close cluster last (WT, Firefox, Chromium all do); keep it at the
   bottom behind the same "destructive cluster goes last" HIG instinct the
   terminal menu already follows.
3. **Close Other Tabs** and **Close Tabs to the Right** (WT + both browsers).
   One-shot, no per-tab prompts — no surveyed peer prompts for these
   (Firefox closes immediately and offers Undo; WT never prompts). Enable
   like WT/Chromium: others > 1 for close-others, not-last-tab for
   close-right; both disabled outright when unsatisfiable, and both pivot on
   the clicked tab, not the active one.
4. **Move Left / Move Right** (GT/Yakuake/WT menus; Konsole/kitty keybinds).
   DropTerm already has `Action::MoveTabLeft/Right` on
   `Ctrl+Shift+PageUp/PageDown` that clamp at the edges
   (`src/terminal/widget.rs:116-120`) — the menu is a third surface for an
   existing verb. Disable at the edges per WT/Yakuake rather than relying on
   the clamp silently no-oping.
5. **Duplicate Tab** (WT/Firefox/Chromium). Existing `open_duplicate_tab`
   verb (`Ctrl+Shift+D`), applied to the clicked tab's cwd.
6. **New Tab (+ per-profile launches)** (Guake/Firefox/Chromium). Rides the
   existing `ProfilesFolder` shape from `src/context_menu.rs`; equal-weight
   counterpoint: GT/Konsole/WT keep it off the tab menu since their `+` or
   bar handles it, and DropTerm's `+` right-click already does. Recommended
   for menu completeness — right-clicking a tab shouldn't force a trip to
   the far end of the bar.

1-2 or 0 precedents:

7. **Switch Profile ▸** (0 precedents — DropTerm-specific). Keep, as the
   terminal-area menu already carries it; in the per-tab menu it gains the
   clicked-tab targeting the terminal menu cannot offer (that one respawns
   the *active* tab). Flag in the spec as an extension beyond the peer norm,
   not a convention-follow.
8. **Terminal-scoped verbs (Copy/Paste/Select All/Clear Scrollback)** —
   0 precedents in any tab menu surveyed; every peer keeps them in the
   terminal-area menu, and they act on a *terminal surface*, not a tab
   identity. Exclude: the terminal context menu already owns them, and
   Firefox/Chromium's lone page-scoped verb (Reload) maps to nothing in
   DropTerm — Clear Scrollback is the nearest cousin and stays
   terminal-scoped everywhere.
9. **Out of scope / deferred**: Pin Tab (GT/Firefox/Chromium precedents
   exist, but ROADMAP.md:44 pins it to "Later" — note, don't design for it);
   Detach/Move to New Window (no multi-window DropTerm); watch
   activity/silence (Yakuake-only among drop-downs; no monitoring
   infrastructure); color-per-tab and export/find (WT/GNOME-specific).

## Sources

- GNOME Terminal (master): `src/terminal-notebook.ui`, `terminal-notebook.cc`,
  `terminal-window.cc`, `terminal-headermenu.ui`:
  https://gitlab.gnome.org/GNOME/gnome-terminal
- Konsole (master): `src/widgets/ViewContainer.cpp`,
  `src/session/SessionController.cpp`: https://invent.kde.org/utilities/konsole
- Yakuake (master): `app/tabbar.cpp`, `app/mainwindow.cpp`:
  https://invent.kde.org/utilities/yakuake
- kitty: tab-bar mouse handling (`kitty/tabs.py`):
  https://github.com/kovidgoyal/kitty · actions reference:
  https://sw.kovidgoyal.net/kitty/actions/
- WezTerm (main): `wezterm-gui/src/termwindow/mouseevent.rs`:
  https://github.com/wezterm/wezterm · ShowTabNavigator:
  https://wezterm.org/config/lua/keyassignment/ShowTabNavigator.html
- Guake (master): `guake/menus.py`, `guake/boxes.py`, `guake/notebook.py`:
  https://github.com/Guake/guake
- Windows Terminal (main): `src/cascadia/TerminalApp/Tab.cpp`:
  https://github.com/microsoft/terminal
- Firefox (mozilla-central): `browser/base/content/main-popupset.inc.xhtml`,
  `main-popupset.js`: https://searchfox.org/mozilla-central/
- Chromium (main): `chrome/browser/ui/tabs/tab_menu_model.cc`,
  `tab_strip_model.cc`:
  https://chromium.googlesource.com/chromium/src/+/main/chrome/browser/ui/tabs/
- DropTerm grounding: `src/context_menu.rs`, `src/keybinds.rs:127-132`,
  `src/terminal/widget.rs:116-120`, `src/tabs.rs:221`, `ROADMAP.md:44`.
