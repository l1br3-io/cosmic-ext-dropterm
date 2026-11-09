# Research: how should DropTerm generate tab names?

Date: 2026-10-04
Question: *What setting should control how a tab's name is generated — and what do famous terminals (Guake first) actually expose for automatic tab-title generation, with which precedence rules, defaults, template languages, and rename interactions?*

## TL;DR

**Convergence across every surveyed terminal: the shell's OSC 0/2 title is the
default automatic source; a mode setting decides whether/how it is applied; a
manual rename, where it exists, is sticky (never clobbered) and cleared by an
explicit empty/sentinel rename; and a length cap with truncation is the one
garnish almost everyone ships.** Concretely:

- **Guake — the reference — exposes exactly three title settings** on the
  General page of its preferences: a `use-vte-titles` bool ("Use VTE titles
  for tab names", default **on**), a `display-tab-names` dropdown ("Display as
  tab names:" → `Full Path` / `Abbreviated Path` / `Last Segment`, default
  **Full Path**) that post-processes the shell title using the terminal's
  cwd, and a `max-tab-name-length` spinner (default **100**, `0` disables;
  truncation keeps the *tail* prefixed by `...`). A user rename freezes the
  label (`custom_label_set`) and is reset by renaming to exactly `-`.
- The vte-based family shares one heritage: Guake's settings, Tilix's
  per-terminal title template, and Terminator's editable titlebar all echo
  GNOME Terminal's old "initial title + replace/before/after/ignore" pair —
  which GNOME Terminal itself **removed** in 3.14 (2014) and now shows the
  raw vte title. The KDE family (Konsole + Yakuake) instead bet on
  **per-session format strings with `%` placeholders** re-snapshotted from
  `/proc` foreground-process info.
- The modern template camp (kitty `tab_title_template`, Tilix
  `${title}`/`${directory}`/…, WezTerm `format-tab-title` Lua, iTerm2 title
  components, Windows Terminal `tabTitle`+`suppressApplicationTitle`) all put
  the **shell title in as one variable** and layer user text/other signals
  around it — with manual/user title always winning over the automatic one.

**Recommended setting shape for DropTerm:** one enum dropdown, config key
`tab_title_mode`, variants `Shell` (default; today's behavior), `ShellDirectory`
(shell title with its trailing cwd abbreviated / reduced to the last segment,
Guake-style), and `Numbered` (ignore shell titles; always `Terminal N`) —
re-titling live tabs on change like Guake's `recompute_tabs_titles`, never
touching `title_override`, keeping the bell/activity prefix outside the mode's
concern, plus an optional `max_tab_title_length` clamp later if wanted.
Details and edge cases in [Recommendation](#recommendation).

A mechanism primer first, since every design below rides on it:

- **OSC 0 vs OSC 2.** xterm's control sequences define `OSC 0 Ps` = "Change
  both Icon Name and Window Title" and `OSC 2 Ps` = "Change Window Title"
  (`OSC 1` = icon name only) —
  [xterm ctlseqs](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html),
  Operating System Commands section. Most shells and apps (bash via
  `PROMPT_COMMAND` precmd hooks — "executed before printing the primary
  prompt", [GNU Bash manual](https://www.gnu.org/software/bash/manual/bash.html);
  zsh via `precmd`; vim via `titlestring`) emit OSC 2 (or 0) continuously, so
  "the terminal title" is really "the last title the child reported".
- **Title stacks.** xterm's XTWINOPS `CSI 22;0|1|2 t` saves and
  `CSI 23;0|1|2 t` restores icon/window titles on a stack (optional third
  parameter addresses slots 1-10 directly) — same ctlseqs page. kitty
  implements this per window with a 10-deep stack
  (`kitty/window.py:794`, `:1933-1942` @ master).
- **DropTerm already rides this layer.** alacritty_terminal 0.25.1 — the
  crate DropTerm pins (`Cargo.lock`) — parses OSC `0` and `2` identically
  into `set_title` (vte 0.15.0 `src/ansi.rs:1348-1353`, local cargo checkout)
  and surfaces `Event::Title(String)` / `Event::ResetTitle` (on RIS)
  (`src/term/mod.rs:2221-2231`, with a CSI 22/23 t title stack capped at
  `TITLE_STACK_MAX_DEPTH = 4096`, `src/term/mod.rs:42,2235-2256`). DropTerm's
  `TermEvent::Title` handling is exactly this feed (`src/app.rs:7,3162-3168`).
  Konsole-style OSC 30 ("session name", non-standard — `Session.h:473`) and
  OSC 7 (cwd report) are *not* what shells typically emit for titles, but
  Konsole and GNOME-family terminals treat OSC 7 as a cwd source for
  directory-derived titles.

## Findings

### 1. Guake — the reference: one checkbox, one dropdown, one spinner, and a sticky rename

From [Guake/guake](https://github.com/Guake/guake) @ `master`
(fetched 2026-10-04; schema `guake/data/org.guake.gschema.xml`, UI
`guake/data/prefs.glade`, logic in `guake/guake_app.py`, `guake/notebook.py`,
`guake/utils.py`, `guake/boxes.py`, `guake/main.py`):

**Settings exposed** (all on the *General* page of the preferences notebook,
`prefs.glade` — page tab label "General" at L809, widgets L926-1088):

| Setting (schema key) | Control + exact strings | Default |
|---|---|---|
| `use-vte-titles` (b) | checkbox **"Use VTE titles for tab names"** (`use_vte_titles`, L1078-1088) | `true` |
| `display-tab-names` (i) | dropdown labeled **"Display as tab names:"** (`tab_name_display`, L945-953) with items **"Full Path"**, **"Abbreviated Path"**, **"Last Segment"** | `0` (Full Path) |
| `max-tab-name-length` (i) | spinner **"Max tab name length:"** (`max_tab_name_length`, L988-998) | `100` |
| `set-window-title` (b) | checkbox **"Set window title to current tab name"** (L1063) | `false` |
| `rename-current-tab` keybinding | `<Control><Shift>R`, "Rename current tab." | — |

Schema verbatim: `display-tab-names` — *"Controls how the tab names are
displayed. 0: Full Path, 1: Abbreviated Path, 2: Only display the last segment
of the path"*; `use-vte-titles` — *"Any terminal program can set the
terminal's title via special escape sequences… you can have easy
automatically updating meaningful tab titles with this option turned on."*
The dropdown, its label, and the spinner are all desensitized when "Use VTE
titles" is unchecked (`guake/prefs.py:855-864`, `:1132-1145`). The combo maps
positionally: `set_int("display-tab-names", combo.get_active())`
(`prefs.py:415-418`). Note `max-tab-name-length` carries literal
`#TODO PORT` markers in summary/description — the key works but was migrated
without polish.

**Semantics — the title pipeline** (`guake/guake_app.py`):

1. VTE's `window-title-changed` signal (GTK-VTE's OSC 0/2 feed) fires
   `on_terminal_title_changed` (`guake_app.py:1195-1225`). It returns
   immediately if `use-vte-titles` is false (L1204-1206) — with the setting
   off, the tab keeps whatever name it got at creation and is never
   retitled automatically.
2. If the tab has a user rename (`custom_label_set`), the shell title is
   dropped (L1217-1218); the existing custom text still updates the *window*
   title.
3. Otherwise `compute_tab_title(vte)` runs (`guake_app.py:1160-1182`):
   - a `.guake.yml` `title:` in the cwd (when `load-guake-yml` is on) wins
     over everything (L1163-1166);
   - the base is `vte.get_window_title() or _("Terminal")` (L1168) — the
     i18n'd literal "Terminal" is the no-title fallback;
   - `display_tab_names == 1` (Abbreviated Path): *if* the title ends with
     the current directory, every parent component is cut to its first
     character and the last stays whole (L1171-1174) — e.g.
     `~/src/oss/l1br3/ced` → `~/s/o/l/ced`;
   - `display_tab_names == 2` (Last Segment): the title is replaced by the
     last path component of the cwd, `"(root)"` for `/` (L1175-1178);
   - finally `TabNameUtils.shorten` applies the length cap.
4. `TabNameUtils.shorten` (`guake/utils.py:158-167`): lengths beyond
   `max-tab-name-length` become `"..." + text[-max:]` — the **tail is kept**,
   head elided; `0` disables truncation; and the whole clamp is skipped when
   `use-vte-titles` is off (a quirk: the spinner is meaningless then anyway).
5. A new tab gets this same computation once at creation
   (`guake/notebook.py:438-456`, `new_page_with_focus` → `rename_page(…,
   self.guake.compute_tab_title(terminal), False)`).

**Rename interaction (sticky + sentinel):** `rename_page(page_index,
new_text, user_set)` (`guake/notebook.py:459-477`) applies a title only when
`not getattr(page, "custom_label_set", False) or user_set` — a user rename
permanently blocks VTE-driven retitling; and `setattr(page,
"custom_label_set", new_text != "-")` — renaming to exactly **`-`** clears
the lock so automatic titles resume. The `-` reset is documented in the CLI
help: `--rename-tab` / `--rename-current-tab` — *"Reset to default if TITLE
is a single dash '-'."* (`guake/main.py:347-364`). The rename UI is the
`RenameDialog` (`guake/dialogs.py:11-41`) opened from the tab label
(`guake/boxes.py:698-707`) or the keybind (`guake_app.py:1077-1084`).

**Live re-titling on setting change:** the GSettings handler wires
`max-tab-name-length` and `display-tab-names` (`guake/gsettings.py:93-94`,
`:470-474`) and `recompute_tabs_titles` (`guake_app.py:1128-1140`) re-runs
`compute_tab_title` over all terminals. Window title follow-along is the
separate `set-window-title` bool (`update_window_title`,
`guake_app.py:1227-1231`: tab name or the saved `default_window_title`).

**What Guake does *not* have:** no per-tab-format template, no
process-name mode (the "title" is whatever the shell reports; the dropdown
merely reshapes it using the *directory*), no prepend/append combine mode.

### 2. GNOME Terminal (vte) — the ancestor dropdown, since removed

From [GNOME/gnome-terminal](https://github.com/GNOME/gnome-terminal)
(read-only GitLab mirror; fetched 2026-10-04):

- **≤ 3.6 (GConf era) — the classic pair.** Profile keys
  `/apps/gnome-terminal/profiles/Default/title` (string, default
  `Terminal`) and `…/title_mode` (string, **default `"replace"`**), schema
  text verbatim: *"the dynamically-set title can erase the configured title,
  go before it, go after it, or replace it. The possible values are
  `"replace"`, `"before"`, `"after"`, and `"ignore"`."*
  (`src/gnome-terminal.schemas.in` @ tag `3.6.2`).
- **The dropdown strings** (still present as a GtkListStore at 3.12,
  `src/profile-preferences.ui:43-62`): **"Replace initial title"**, **"Append
  initial title"**, **"Prepend initial title"**, **"Keep initial title"**
  (translatable comment: "When terminal commands set their own titles"),
  under the profile Title section alongside the "Initial title" entry
  (`title-entry`, bound to the `title` key — `src/profile-editor.c:970-972`
  @ 3.12.0).
- **Removal.** By 3.12 the GSettings schema has no `title-mode` key at all —
  only `title` survives, whose description still gestures at "the
  title_mode setting" (stale text), and the mode dropdown was already
  vestigial in the UI (the four-option liststore remains in
  `profile-preferences.ui` but no widget binds it; the Initial-title entry
  still binds `title`). The entry and the whole Title section were then
  removed on 2014-04-16 — commits `e9cb8fea14` "Remove the static title
  setting from profile preferences" and `9bbe19e98f` "Remove the action and
  shortcut to set a static title from the menus" — in the GNOME 3.13 dev
  cycle; verified: tag `3.14.0` has neither `title-entry` nor any `model2`
  usage. The old dialog's reset semantics survive in `9cb8d243dd` "revert to
  dynamically setting the title if the user sets it to an empty".
- **Today (`master`).** No title keys in
  `src/org.gnome.Terminal.gschema.xml` at all. The tab title is the raw vte
  window title: `terminal-notebook.cc:121-122,146-147` binds the screen's
  `title` property straight to the notebook page; the window title is the
  active screen's title with an i18n `"Terminal"` fallback
  (`src/terminal-window.cc:2482-2490`). "Set _Title…" menu items exist in the
  hamburger (`src/terminal-headermenu.ui:61-64`, action `win.set-title`) and
  the tab context menu (`src/terminal-notebook.ui:66-70`,
  `notebook.notebook-set-title`) but both carry
  `<attribute name="hidden-when">action-missing</attribute>` — and I could
  **not** find either action installed in `terminal-window.cc`'s action
  tables (`:2061-2123`), so on stock master these items stay hidden
  (unverified whether a build flag installs them).

So the vte heritage Guake shares is exactly this initial-title + four-way
mode idea — Guake kept a simplified cousin (use/don't-use + reshape) after
GNOME Terminal abandoned it.

### 3. Konsole — per-session format strings with `%` placeholders, local and remote

From [KDE/konsole](https://github.com/KDE/konsole) @ `master` (fetched
2026-10-04) and the
[Konsole Handbook, "Configure Tab Settings Dialog"](https://docs.kde.org/trunk_kf6/en/konsole/konsole/console-dialogs.html):

**Settings exposed.** Two format strings per profile — and per session,
overridable: profile keys `LocalTabTitleFormat` and `RemoteTabTitleFormat`
(`src/profile/Profile.cpp:77-79`), edited on the profile's *Tabs* page
(`src/widgets/EditProfileDialog.cpp:707-713`). The handbook: *"The name
format, the remote tab title format, and the color of the current tab can be
changed from this dialog. The dialog can be displayed via the menu, the
shortcut **Ctrl+Alt+S** or by double-clicking on the tab in the tab bar.
These changes are temporary and can be made permanent by editing the current
profile."*

**Defaults:** `LocalTabTitleFormat` = **`%d : %n`** (e.g. `src : bash`),
`RemoteTabTitleFormat` = **`(%u) %H`** (`Profile.cpp:77-79`).

**Placeholder mini-language** (handbook table, confirmed against the
implementation in `src/session/Session.cpp:1351-1430`):

| Token (local) | Meaning | Token (remote) | Meaning |
|---|---|---|---|
| `%n` | program name (foreground process) | `%c` | current program (ssh command) |
| `%d` | current directory (short) | `%h` | remote host (short) |
| `%D` | current directory (long) | `%H` | remote host (long, `host:port`) |
| `%h` | local host (short) | `%u` | user name |
| `%u` | user name | `%U` | `user@` if given |
| `%B` | Bourne prompt sigil (`$`/`#`) | `%w` | window title set by shell |
| `%w` | window title set by shell | `%#` | session number |
| `%#` | session number | | |

Remote tokens resolve from the parsed `ssh` command line
(`src/SSHProcessInfo.cpp:202-244`); `%u` in the local format also arms a
"user name required" check (`Session.cpp:750-756`).

**Semantics / precedence:**

- **Local vs remote switch:** the *foreground* process is inspected via
  `/proc` (`ProcessInfo`); when it is `ssh`, the Remote format + ssh-derived
  tokens are used, otherwise the Local format + local process info
  (`Session::getDynamicTitle`, `Session.cpp:1325-1348`, `isRemote`
  `:1317-1323`).
- **The shell's title is just `%w`.** OSC 0 and OSC 2 land in `_userTitle`
  (`Session::setSessionAttribute`, `Session.cpp:665-670`; OSC number mapping
  in `src/session/Session.h:461-479` — `IconNameAndWindowTitle = 0`,
  `IconName = 1`, `WindowTitle = 2`, plus Konsole's non-standard
  `SessionName = 30`, `SessionIcon = 32`, `SessionColor = 34`). A format
  without `%w` simply never shows the shell title — the mode axis *is* the
  format string.
- **OSC 30 replaces the format.** A program sending Konsole's non-standard
  OSC 30 ("session name") overwrites the whole *local tab title format* for
  that session (`Session.cpp:691-697`) — the strongest program-side override
  any surveyed terminal offers.
- **Recompute cadence:** the title is re-snapshotted on session start and
  after user interaction (a 2 s debounce timer), not on every output byte
  (`SessionController::snapshot`, `src/session/SessionController.cpp:335-352`,
  connects at `:201,214-222`); OSC-driven changes also re-apply immediately
  via `sessionAttributeChanged` (`:2048-2075`, which substitutes `%w`/`%#`
  into the displayed title).
- **Rename = editing the session's formats, sticky against the profile.**
  The tab context menu / Ctrl+Alt+S "**&Configure or Rename Tab...**"
  (`SessionController.cpp:868-872`) opens the "Tab Properties" dialog
  (`src/RenameTabDialog.cpp:25`) pre-filled with the session's local/remote
  formats (`SessionController.cpp:1086-1124`); accepting sets the per-session
  formats and marks `tabTitleSetByUser(true)` (`Session.cpp:765-772`), after
  which profile-level format changes no longer apply to that session
  (`src/session/SessionManager.cpp:231-234`). There is no separate
  "clear rename" — you just edit the format back.

### 4. Yakuake — Konsole's machinery, a thin sticky-rename shell around it

From [KDE/yakuake](https://github.com/KDE/yakuake) @ `master` (fetched
2026-10-04):

- Yakuake embeds `konsolepart` and takes titles from it: the part's
  `setWindowCaption` signal feeds the session's title
  (`app/terminal.cpp:35-43`), which flows to the tab bar as automated titles
  (`app/mainwindow.cpp:917` — `titleChanged(int,QString)` →
  `setTabTitleAutomated`). So all of Konsole's behavior above — formats,
  `%` tokens, OSC handling — applies verbatim; Yakuake adds **no** title
  settings of its own (its skins, `data/skins/*/title.skin`, only paint the
  bar).
- Its one own contribution is the rename gate: `setTabTitle(sessionId,
  newTitle, interactive)` (`app/tabbar.cpp:867-885`) ignores automated
  (program) titles once `m_tabTitlesSetInteractive` is set; an interactive
  rename sets the flag, and an **empty** interactive title clears it so
  automatic titles resume. Affordances: double-clicking a tab opens an
  inline line edit (`tabbar.cpp:693,747-775`), and the tab context menu
  leases Konsole's `rename-session` action (`tabbar.cpp:114`) — the same
  "sticky rename + empty clears" shape as DropTerm's `commit_rename`, and
  Guake's with `-` instead of empty.

### 5. kitty — `tab_title_template`, the richest template language

From [kitty conf docs](https://sw.kovidgoyal.net/kitty/conf/) (fetched
2026-10-04) and [kovidgoyal/kitty](https://github.com/kovidgoyal/kitty) @
`master`:

**Settings exposed** (all in the *Tab bar* group of `kitty.conf`):
`tab_bar_style` (default `fade`; `fade`/`slant`/`separator`/`powerline`/`custom`/`hidden`),
`tab_bar_min_tabs` (default `2`),
`tab_title_template` (a template string),
`active_tab_title_template` (default `none` → falls back to
`tab_title_template`),
`tab_title_max_length` (default `0` = no limit; vertical tab bars size for
~20 title cells when unset),
`tab_activity_symbol` (default `none`),
`bell_on_tab` (default `"🔔 "` — "Some text or a Unicode symbol to show on
the tab if a window in the tab that does not have focus has a bell"),
plus the bar color/style keys.

**Default template, verbatim:**

```
tab_title_template "{fmt.fg.red}{bell_symbol}{activity_symbol}{secure_input_symbol}{fmt.fg.tab}{tab.last_focused_progress_percent}{title}"
```

and the doc's backward-compat rule: *"if `{bell_symbol}` or
`{activity_symbol}` are not present in the template, they are prepended to
it."* So the shipped default title = (bell 🔔) + (activity) + (progress %) +
the tab's title, colored red-then-normal.

**Template mini-language.** Python format-string machinery with styling
directives — *"you can use, for instance, `{layout_name[:2].upper()}`"* and
`{fmt.fg.red}`, `{fmt.bold}`, `{fmt.fg.color0..255}`, `{fmt.bg.tab}` etc.
Available variables (docs list, verbatim names): `title`, `index` (usable
with `goto_tab N`), `layout_name`, `session_name`, `active_session_name`,
`num_windows`, `num_window_groups`, `secure_input_symbol`, `tab.active_wd`,
`tab.active_exe` (and `…_oldest_…` variants; both marked "expensive,
requires syscall"), `max_title_length`, `keyboard_mode`,
`tab.last_focused_progress_percent`, `tab.progress_percent`,
`tab.children_mem_usage`, and `{custom}` (calls `draw_title(data)` from
`tab_bar.py` in the config dir). There is also a window-title counterpart
(`window_title_template`, with `{title}`, `{bell_symbol}`,
`{activity_symbol}`, `{progress_percent}`, `{custom}`, `{fmt}`,
`{is_active}`) for per-window title bars.

**Precedence.** A tab's effective title is `self.name or self.title`
(`kitty/tabs.py:561-563`): the **user title wins**, and `self.title` is the
active window's OSC title (`tabs.py:556-559`; a window's own title is
`override_title or child_title`, `kitty/window.py:906-908`, where the
no-OSC default is the *program basename* — `window.py:792`). The remote
control command `kitten @ set-tab-title [options] TITLE ...` sets `name`;
*"If you do not specify a title, the title of the currently active window in
the tab is used"* — i.e. **an empty call resets to automatic**
([remote-control docs](https://sw.kovidgoyal.net/kitty/remote-control/);
`Tab::set_title` does `self.name = title or ''`, `tabs.py:573-574`). kitty
also implements the xterm title stack per window (10-deep,
`window.py:794,1933-1942`).

### 6. WezTerm — default "index: active pane title", full control via the `format-tab-title` Lua event

From [wezterm docs](https://wezterm.org/config/lua/window-events/format-tab-title.html)
(markdown source at [wezterm/wezterm](https://github.com/wezterm/wezterm)
`docs/config/lua/window-events/format-tab-title.md` @ `main`, fetched
2026-10-04) and `wezterm-gui/src/tabbar.rs` @ `main`:

**Default title (no config):** `tab.tab_title` if non-empty, else the active
pane's title (the pane's OSC title), prefixed `"N: "` when
`show_tab_index_in_tab_bar` is true (the default) —
`wezterm-gui/src/tabbar.rs:220-242`. Numbering base toggled by
`tab_and_split_indices_are_zero_based`. Progress glyphs (ConEmu-style OSC 9;4
percent) are appended when the pane reports progress (`tabbar.rs:244-250`).

**Settings:** `show_tab_index_in_tab_bar = true` (default),
`hide_tab_bar_if_only_one_tab = false` (default; "If set to true, when there
is only a single tab, the tab bar is hidden"), `tab_max_width` (default 16
glyphs, retro tab bar only), `tab_bar_style` (styling of new-tab button etc.).

**The template escape hatch — `format-tab-title`:** a *synchronous* Lua
event; params `tab, tabs, panes, config, hover, max_width`; the handler
returns a string or a table of `FormatItem`s (styled spans, as in
`wezterm.format`), or `nil` to fall back to the built-in computation ("If
the event encounters an error… the default tab title text will be computed
and used instead"). It is called twice per tab (measure pass, then render
pass with final `max_width`). The doc's canonical helper encodes the
precedence explicitly:

```lua
function tab_title(tab_info)
  local title = tab_info.tab_title
  if title and #title > 0 then return title end   -- explicit tab title wins
  return tab_info.active_pane.title               -- else the active pane's title
end
```

**Rename:** `wezterm cli set-tab-title TITLE` (with `--tab-id`/`--pane-id`;
since 20230408-112425) sets `tab_title`; `tab:set_title()` in Lua is the
in-process equivalent. `TabInformation` exposes `tab_id`, `tab_index`,
`is_active`, `is_last_active`, `active_pane`, `panes`, `window_id`,
`window_title`, `tab_title` for template authors.

### 7. Tilix — vte heritage plus a full `${token}` mini-language on the title bar

From [gnunn1/tilix](https://github.com/gnunn1/tilix) @ `master` (fetched
2026-10-04; schema `data/gsettings/com.gexperts.Tilix.gschema.xml`, logic
`source/gx/tilix/terminal/terminal.d`, tokens `source/gx/tilix/constants.d`):

**Settings exposed.** Profile key `terminal-title` (schema L753-757): *"The
string to display as the terminal title … supports tokens which are replaced
at runtime, additionally pango markup can be used as well."* — **default
`${id}: ${title}`**, edited on the profile's Title page with a
variables-dropdown helper (`source/gx/tilix/prefeditor/titleeditor.d:41-77`
inserts the chosen token into the entry). Chrome keys: `terminal-title-style`
(`normal`/`small`/`none`, default `normal`), `terminal-title-show-when-single`
(default `true`), `control-click-titlebar` (default `false`), and global
`process-monitor` (default `false`) which gates `${process}`.

**Token mini-language** (`constants.d:122-133`, substitution in
`terminal.d:1306-1336`):

| Token | Value |
|---|---|
| `${title}` | the vte window title; empty → i18n **"Terminal"** (`terminal.d:1307-1309`) |
| `${iconTitle}` | the vte icon title (OSC 1) |
| `${id}` | the terminal's session-assigned number |
| `${directory}` | current working directory |
| `${columns}` / `${rows}` | grid size |
| `${hostname}` / `${username}` | resolved per terminal state |
| `${process}` | foreground process name — only with `process-monitor` on, else **"Not Enabled"** |
| `${status.readonly}` / `${status.silence}` / `${status.input-sync}` | booleans as strings |

Session-level tokens (`constants.d:167-174`): `${terminalCount}`,
`${terminalNumber}`, `${activeTerminalTitle}` — the sidebar session name
defaults to `${title}` (`session-name`, schema L205-209), and the window
title template `app-title` defaults to `${appName}: ${sessionName}`
(L175-179).

**Precedence / rename.** The computed title is `_overrideTitle` if set, else
the profile `terminal-title` template, then token substitution
(`terminal.d:1273-1284`). The per-terminal title bar's menu opens an edit
dialog (title/badge/command) that sets `_overrideTitle`
(`terminal.d:617-629`) — a user override that beats the template until
cleared; it is persisted in the session JSON (`terminal.d:4086-4088`) and can
also be set by *triggers* (`TriggerAction.UPDATE_TITLE`,
`terminal.d:1722-1725`).

### 8. Terminator — per-terminal titlebar with an inline-editable custom title

From [gnome-terminator/terminator](https://github.com/gnome-terminator/terminator)
@ `master` (fetched 2026-10-04):

- Each terminal can wear a **titlebar** (`show_titlebar`, default `True` —
  `terminatorlib/config.py:244`; hidden automatically when there's a single
  ungrouped terminal — `titlebar.py:196-204`) whose label shows
  `heldmarker + termtext + " COLSxROWS"` (the size suffix toggle is
  `title_hide_sizetext`, `titlebar.py:109-113`).
- `termtext` is fed by vte's `window-title-changed` (`terminal.py:521-522`
  → `:178`), with the fallback `vte.get_window_title() or str(self.command)`
  — the *configured command* stands in when the shell sets nothing
  (`terminal.py:1014-1016`).
- The label itself is an `EditableLabel`: *"On double-click or key binding
  the label is editable, entering an empty [string] will revert back to
  automatic text"* (`terminatorlib/editablelabel.py:25-29`; the empty-commit
  reset is `:118-129`). Custom titles persist in the layout as `title`
  (`terminal.py:1957,1981`). Supporting config: `title_font` (`Sans 9`),
  `title_use_system_font`, `title_at_bottom`, and the
  `title_receive_/inactive_/transmit_` fg/bg color pairs for broadcast
  states.

### 9. iTerm2 (macOS) — profile Title components + the "apps may change title" toggle

From [iterm2.com](https://iterm2.com/documentation-preferences-profiles-general.html)
and [Session Title docs](https://iterm2.com/documentation-session-title.html)
(fetched 2026-10-04), plus [gnachman/iTerm2](https://github.com/gnachman/iTerm2)
@ `master`:

- **Settings > Profiles > General > Title** is a menu of separately
  enableable items, "combined to form the session's title": **Session Name**
  (defaults to the profile name), **Profile Name**, **Profile & Session
  Name** (the first three mutually exclusive), **Job** (foreground job),
  **User**, **Host**, **PWD**, **TTY** — plus items offered by running
  Python *Session Title Providers*. *"The session's title is shown in
  per-pane title bars, when visible; it is also the default tab title. The
  current tab title also serves as the window title."* A per-tab override is
  available via **Window > Edit Tab Title**.
- **Shipped default: `Job` only** — the profile-default dictionary ships
  `KEY_TITLE_COMPONENTS : @(iTermTitleComponentsJob)`
  (`sources/Settings/iTermProfilePreferences.m:1276`; migration from the
  deprecated `ShowJobName` default-of-YES at `:1750-1801`).
- **The OSC toggle.** Checkbox **"Applications in terminal may change the
  title"** — *"When enabled, a control sequence can change a session's or
  window's title."* Notably, the control-sequence title lands in the
  **Session Name** component: *"Receiving a control sequence that updates
  the 'icon title'. These are OSC 0 and OSC 1."* (session-title docs) — so
  unlike the vte family, OSC does not replace the whole composed title, it
  feeds one component of it.
- Tab **subtitles** (a second line, settable by control sequence or as an
  interpolated string) exist as a separate feature.

### 10. Windows Terminal — fixed `tabTitle` vs OSC, gated by `suppressApplicationTitle`

From [learn.microsoft.com](https://learn.microsoft.com/en-us/windows/terminal/)
and [microsoft/terminal](https://github.com/microsoft/terminal) @ `main`
(`doc/cascadia/profiles.schema.json`, fetched 2026-10-04):

- **Profile `tabTitle`** (schema): *"If set, will replace the name as the
  title to pass to the shell on startup. Some shells (like bash) may choose
  to ignore this initial value, while others (cmd, powershell) may use this
  value over the lifetime of the application."*
- **Profile `suppressApplicationTitle`** (boolean, **default `false`**): *"When
  set to true, tabTitle overrides the default title of the tab and any title
  change messages from the application will be suppressed. When set to false,
  tabTitle behaves as normal."* The
  [troubleshooting page](https://learn.microsoft.com/en-us/windows/terminal/troubleshooting)
  spells out the recipe: adding only `suppressApplicationTitle: true` pins
  the tab to the *profile name*; add `tabTitle` for a custom string.
- **`newTab` action args** ([actions page](https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions)):
  `tabTitle` ("Title of the new tab") and `suppressApplicationTitle`
  (`true`/`false`; "If not provided, the behavior is inherited from the
  profile's settings. **In order to enter a new tab title and have that title
  persist, this must be set to true**") — i.e. a title passed without
  suppression is exactly the "initial title" idea: handed to the shell and
  immediately overwritable by OSC.
- **Rename stickiness.** The `renameTab` action
  (`{ "command": { "action": "renameTab", "title": "Foo" } }`) resets with
  `"title": null` — "If omitted, this command will revert the tab title back
  to its original value" — and `openTabRenamer` (`Terminal.OpenTabRenamer`)
  turns the tab title into a text field: *"Clearing the text field will
  reset the tab title back to the default for the current shell instance"*
  (same actions page). Renames are runtime tab state, not persisted profile
  config.

### 11. Counterpoints: Alacritty, foot, Ghostty — minimal or none

- **Alacritty** has **no tabs at all**; titles are window-only. Current
  config (man page `extra/man/alacritty.5.scd` @ `master`): `title` —
  "Window title. Default: *Alacritty*" — and `dynamic_title` (under
  `window`) — *"Allow terminal applications to change Alacritty's window
  title. Default: true"*. History: a top-level `dynamic_title` existed until
  it moved into the `window` table ([CHANGELOG.md](https://github.com/alacritty/alacritty/blob/master/CHANGELOG.md),
  0.5.0/0.6.0-era entries). The `false` value is the primitive
  `suppressApplicationTitle`/`locked-title`-style opt-out, applied to a
  window instead of a tab.
- **foot** also has **no tab bar** (one window, one terminal). Its
  `foot.ini` `[main]` section has `title` ("Initial window title. Default:
  *foot*") and `locked-title` — *"If enabled, applications are not allowed
  to change the title at run-time. Default: no"* (`doc/foot.ini.5.scd` @
  master). No template, no modes — the OSC title just lands.
- **Ghostty** documents a single `title` key: *"This will force the title of
  the window to be this title at all times and Ghostty will ignore any set
  title escape sequences programs (such as Neovim) may send"*; unset, titles
  follow the running program ([config reference](https://ghostty.org/docs/config/reference)).
  No tab-title options exist (Linux GTK builds have no tabs; macOS tabs are
  native window tabs wearing the window title).

## What DropTerm has to work with

Verified in this repo:

- **The pipeline today** (`src/tabs.rs`): a `Tab` stores `title` — "the last
  title its shell reported, falling back to the default from
  `next_default_title`" — and `title_override` — "a user rename, which shell
  title changes never clobber" (`src/tabs.rs:81-108`). Defaults are
  `Terminal N`, numbered from the tab's id (`next_default_title`,
  `src/tabs.rs:304-308`). `apply_shell_title` lands a shell report only
  while `title_override.is_none()` (`src/tabs.rs:310-318`), and
  `commit_rename` makes a non-empty commit the sticky override while **an
  empty commit clears it, letting shell titles resume**
  (`src/tabs.rs:320-332`) — already the exact Guake/kitty/Yakuake/Terminator
  shape, minus the `-` sentinel (empty is the reset, like Yakuake).
- **The feed**: alacritty_terminal's `Event::Title` / `Event::ResetTitle`
  reach the app as `TermEvent` (`src/app.rs:7`); `Title` applies via
  `apply_shell_title` and doubles as the cwd-change sample point
  (`src/app.rs:3162-3168`); `ResetTitle` (shell RIS) restores
  `next_default_title` under the same override gate
  (`src/app.rs:3170-3181`). The rename remote command funnels through
  `commit_rename` (`src/app.rs:1129-1135`), as does the tab bar's inline
  rename overlay (`src/app.rs:1505`).
- **Attention markers are render-time prefixes**, not part of the stored
  title: `Attention::marker()` yields `"• "`/`"🔔 "` and the tab bar
  concatenates marker + title when laying out paragraphs
  (`src/tabs.rs:66-73`, `src/tab_bar.rs:807-827`) — so any title-mode change
  leaves them untouched for free. Long titles already clip at a per-button
  max width (`src/tab_bar.rs:188-191`); there is no character-count cap.
- **Session persistence** saves/restores both `title` and `title_override`
  (`src/session.rs:36-43,51-57,93-95,123-124`; restore reapplies them so the
  fresh shell's first OSC title can't clobber a saved rename,
  `src/app.rs:575-585`) — a mode that *ignores* shell titles must keep
  writing `title` (it stays the display value) but simply stop applying OSC
  reports.
- **A directory signal already exists**: `check_tab_cwd` resolves the shell
  process's cwd via `/proc/{pid}/cwd` on every title event
  (`src/app.rs:1615-1634`, `Terminal::working_directory` at
  `src/terminal/engine.rs:486-490`) — the same fact Guake's vte
  `get_current_directory()` feeds `display-tab-names`, so a
  Guake-style abbreviate/last-segment mode is implementable without new
  process machinery.
- **Where the setting slots in**: the config is one enum-per-key cosmic-config
  file with serde RON values (`src/config.rs:3-11,121-197`) — the
  `TabPosition` enum + `tab_position` field + Display impl
  (`src/config.rs:103-119,137-140`) is the exact pattern to copy, default
  wired in `DropTermConfig::default` (`src/config.rs:199-226`). The settings
  window renders enum dropdowns from `&[(value, label)]` option tables with
  pure `dropdown_selected`/`dropdown_write` planners
  (`src/settings.rs:430-433,457-497,807-840`); a new key joins `ConfigKey`
  + `PlannedWrite` and lands in a section card (`Section::title`,
  `src/settings.rs:642-652`) — Geometry already hosts tab-related keys
  (`Tab bar position`, `src/settings.rs:664`). Missing keys load as defaults
  (additive, `CONFIG_VERSION` stays 1 — pinned by tests like
  `src/config.rs:572-612`), and the README documents keys in prose
  (`README.md:137,150`) with a live-apply table (`README.md:241`).

## Recommendation

A single dropdown in the settings window, shaped after Guake (its General
page is the direct precedent) but collapsed to what DropTerm can honestly
compute today:

1. **Enum + config key**: `TabTitleMode` in `src/config.rs` (mirror
   `TabPosition`), serialized as RON variants, config key
   **`tab_title_mode`** (one file under
   `~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/`, listed in
   `README.md:137`'s key inventory). Variants and display strings for the
   dropdown (`src/settings.rs` option-table pattern):
   - **`Shell` — "Shell title"** *(default)*: exactly today's behavior —
     OSC 0/2 lands in `Tab.title` unless a rename is set. This matches the
     shipped default of every surveyed peer that has a choice (Guake
     `use-vte-titles=true`, kitty/WezTerm `{title}`-centric defaults, GNOME
     Terminal's implicit replace).
   - **`ShellDirectory` — "Shell title, shortened path"**: keep the shell
     title, but post-process it Guake-style (`compute_tab_title`'s
     display-tab-names logic, `guake_app.py:1171-1178`): when the title ends
     with the tab's cwd (already tracked via `check_tab_cwd`), abbreviate
     parent components to one character; when the title has no directory
     information, reduce to the cwd's last segment. This gives the "tabs
     that stay readable at 3-word width" Guake users actually select.
   - **`Numbered` — "Terminal number"**: ignore shell titles entirely;
     `Tab.title` stays/parks at `next_default_title(id)` (`Terminal N`) —
     Guake's `use-vte-titles=false` (title frozen at creation) and GNOME
     Terminal's `ignore`/"Keep initial title" mode; also the
     `suppressApplicationTitle` audience (people whose shells spam junk
     titles). `TermEvent::Title` still fires `check_tab_cwd` — the cwd
     gate is not a title decision and must keep working.
2. **Placement/label**: a "Tab titles" row in the **Behavior** card
   (`Section::Behavior`, `src/settings.rs:648`) — or Geometry next to
   "Tab bar position"; Behavior fits better since this is semantics, not
   layout. Label "Tab title source" (keystone: Guake labels the group
   "Display as tab names:", kitty "tab_title_template").
3. **Rename stays orthogonal and sticky**: `title_override` continues to win
   over every mode (all four peers with renames agree: Guake
   `custom_label_set`, kitty `name or title`, WezTerm `tab_title` first,
   Yakuake `m_tabTitlesSetInteractive`), and the empty-commit clear
   (`commit_rename`, `src/tabs.rs:325-332`) remains the documented reset —
   no `-` sentinel needed (empty is already the idiom in Yakuake, Windows
   Terminal's renamer, and Terminator).
4. **Live re-title on change** like Guake's `recompute_tabs_titles`
   (`guake_app.py:1128-1140`): on `ConfigChanged`, recompute every tab's
   `title` from its new-mode rule (for `Numbered`, snap to
   `next_default_title(id)`; for `ShellDirectory`, re-derive from the cached
   cwd; for `Shell`, re-apply the last reported OSC title — which means the
   app must keep *storing* the raw report even in other modes; the cheapest
   honest version stores the raw report in a new `shell_title` field or
   simply re-derives on next event). Renamed tabs are skipped entirely.
5. **Bell/activity prefix**: unchanged — markers are render-time
   (`src/tab_bar.rs:821-827`), exactly kitty's model where `bell_symbol` is
   template chrome, not title content.
6. **Edge cases**:
   - **Empty reported titles**: keep the current `Terminal N` fallback chain
     (`next_default_title`); peers hard-fallback too (Guake `_("Terminal")`,
     GNOME Terminal `_("Terminal")`, kitty `appname`, WezTerm pane title).
   - **Truncation/max length**: *defer*. Guake's `max-tab-name-length=100`
     tail-truncation is the only peer precedent with a char cap, and
     DropTerm already clips at the button level (`src/tab_bar.rs:190-191`).
     If ever added: a `max_tab_title_length: u16` with `0` = unlimited and
     Guake's `"..." + tail` semantics, clamped in `DropTermConfig::clamped`.
   - **Session persistence**: `session.rs` keeps writing `title` +
     `title_override` unchanged; a restored tab re-titles per the live mode
     like any other tab.
   - **Remote `rename-tab`** (`src/app.rs:945-951,1129-1135`) is a rename
     and keeps its override semantics in all modes.

Not recommended now: a full template mini-language (Konsole `%`, kitty/foot
`{}`/`${}` tokens). Every template peer needs data feeds DropTerm doesn't
maintain (foreground process name per tab, host/user state, per-window
enumeration); Guake's evidence says a quake terminal's users are served by
the shell title + directory reshaping + a length cap, and a `format-tab-title`-class
escape hatch can be added later without breaking the enum (it would be a
fourth variant, `Custom`, carrying a template string — the WezTerm/kitty
convergence point).

## Sources

- Guake @ master (fetched 2026-10-04): gschema
  `guake/data/org.guake.gschema.xml` (`use-vte-titles`, `display-tab-names`,
  `max-tab-name-length`, `set-window-title`, `rename-current-tab`); prefs UI
  `guake/data/prefs.glade` L809, L926-1088; wiring `guake/prefs.py`
  L403-421, L855-864, L1132-1145; title pipeline `guake/guake_app.py`
  L1128-1140, L1160-1182, L1195-1231, L1341-1342; sticky rename
  `guake/notebook.py` L438-477; truncation `guake/utils.py` L158-167;
  rename dialog `guake/boxes.py` L698-707, `guake/dialogs.py` L11-41; CLI
  sentinel `guake/main.py` L347-364; change handler `guake/gsettings.py`
  L93-94, L470-474 — <https://github.com/Guake/guake>
- GNOME Terminal @ tags 3.6.2 / 3.8.0 / 3.12.0 / 3.14.0 / 3.44.1 / master
  (GitHub read-only mirror, fetched 2026-10-04): `src/gnome-terminal.schemas.in`
  (3.6.2, `title_mode`); `src/org.gnome.Terminal.gschema.xml` (3.12.0
  `title` key; 3.44.1/master absence); `src/profile-preferences.ui`
  (3.12.0 L43-62 mode strings; 3.14.0 absence); `src/profile-editor.c`
  L970-972 (3.12.0); master `src/terminal-notebook.cc` L121-122, L146-147,
  `src/terminal-window.cc` L2482-2490, L2061-2123,
  `src/terminal-headermenu.ui` L61-64, `src/terminal-notebook.ui` L66-70;
  history commits `e9cb8fea14`, `9bbe19e98f`, `9cb8d243dd`,
  `51ccbaa13a` — <https://github.com/GNOME/gnome-terminal>
  (mirror of <https://gitlab.gnome.org/GNOME/gnome-terminal>)
- Konsole @ master (fetched 2026-10-04): defaults
  `src/profile/Profile.cpp` L77-79; tokens + OSC mapping
  `src/session/Session.h` L461-479, `src/session/Session.cpp`
  L659-697, L750-756, L1317-1348, L1351-1430; ssh tokens
  `src/SSHProcessInfo.cpp` L202-244; recompute `src/session/SessionController.cpp`
  L196-232, L335-352, L868-872, L1086-1124, L2048-2075; rename stickiness
  `src/session/SessionManager.cpp` L231-234; dialog `src/RenameTabDialog.cpp`
  L25; profile UI `src/widgets/EditProfileDialog.cpp` L707-713 —
  <https://github.com/KDE/konsole>; handbook "Configure Tab Settings Dialog"
  (token tables, Ctrl+Alt+S, double-click, temporary-vs-profile):
  <https://docs.kde.org/trunk_kf6/en/konsole/konsole/console-dialogs.html>
- Yakuake @ master (fetched 2026-10-04): `app/terminal.cpp` L35-43;
  `app/mainwindow.cpp` L917; `app/tabbar.cpp` L114, L693, L747-775, L867-885;
  skins `data/skins/*/{title,tabs}.skin` — <https://github.com/KDE/yakuake>
- kitty: conf docs (fetched 2026-10-04) — Tab bar group (`tab_bar_style`,
  `tab_bar_min_tabs`, `tab_title_max_length`, `tab_activity_symbol`,
  `bell_on_tab`, `tab_title_template` default + variable list, Python-format
  note, prepend rule, `active_tab_title_template`, `window_title_template`):
  <https://sw.kovidgoyal.net/kitty/conf/>; remote control
  (`kitten @ set-tab-title`):
  <https://sw.kovidgoyal.net/kitty/remote-control/>; source @ master:
  `kitty/tabs.py` L552-563, L573-574; `kitty/window.py` L792-794, L906-908,
  L1933-1942 — <https://github.com/kovidgoyal/kitty>
- WezTerm: `format-tab-title` docs (params, return, fallback, canonical
  tab_title-vs-pane-title helper, two-pass layout):
  <https://wezterm.org/config/lua/window-events/format-tab-title.html>
  (markdown @ `docs/config/lua/window-events/format-tab-title.md` in
  [wezterm/wezterm](https://github.com/wezterm/wezterm) @ main, fetched
  2026-10-04); `show_tab_index_in_tab_bar`, `hide_tab_bar_if_only_one_tab`,
  `tab_max_width` (`docs/config/lua/config/*.md`); CLI
  `docs/cli/cli/set-tab-title.md` + `docs/examples/cmd-synopsis-wezterm-cli-set-tab-title--help.txt`;
  default computation `wezterm-gui/src/tabbar.rs` L203-250; `TabInformation`
  `docs/config/lua/TabInformation.md`
- Tilix @ master (fetched 2026-10-04): schema
  `data/gsettings/com.gexperts.Tilix.gschema.xml` L175-179, L205-209,
  L215-224, L490-501, L753-757; tokens `source/gx/tilix/constants.d`
  L122-174; pipeline `source/gx/tilix/terminal/terminal.d` L617-629,
  L1273-1284, L1306-1336, L1722-1725, L4086-4088; prefs helper
  `source/gx/tilix/prefeditor/titleeditor.d` L41-77 —
  <https://github.com/gnunn1/tilix>
- Terminator @ master (fetched 2026-10-04): `terminatorlib/titlebar.py`
  L102-121, L185-204; `terminatorlib/terminal.py` L175-191, L521-522,
  L1014-1016, L1957-1981; `terminatorlib/editablelabel.py` L25-29, L118-129;
  `terminatorlib/config.py` L244, L279 —
  <https://github.com/gnome-terminator/terminator>
- iTerm2: Preferences → Profiles → General (Title menu, "Applications in
  terminal may change the title"):
  <https://iterm2.com/documentation-preferences-profiles-general.html>;
  Session Title (components, OSC 0/1 → Session Name, Window > Edit Tab
  Title, title providers):
  <https://iterm2.com/documentation-session-title.html> (both fetched
  2026-10-04); default component `sources/Settings/iTermProfilePreferences.m`
  L1276, L1750-1801 @ master — <https://github.com/gnachman/iTerm2>
- Windows Terminal: profile schema (`tabTitle`, `suppressApplicationTitle`
  with `default: false`) `doc/cascadia/profiles.schema.json` @ main
  (fetched 2026-10-04) — <https://github.com/microsoft/terminal>; Actions
  (`newTab` args incl. the persist-requires-suppress note; `renameTab` with
  null reset; `openTabRenamer` clearing note):
  <https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions>;
  troubleshooting (suppress → profile name; + tabTitle for custom):
  <https://learn.microsoft.com/en-us/windows/terminal/troubleshooting>
- Counterpoints: Alacritty man `extra/man/alacritty.5.scd` @ master
  (`title`, `dynamic_title` default true) and `CHANGELOG.md` (option moves)
  — <https://github.com/alacritty/alacritty>; foot `doc/foot.ini.5.scd` @
  master (`title`, `locked-title` default no) —
  <https://codeberg.org/dnkl/foot>; Ghostty config reference (`title`
  forces/ignores OSC) — <https://ghostty.org/docs/config/reference>
- Mechanism: xterm ctlseqs (OSC 0/1/2 definitions; XTWINOPS `CSI 22/23 ; 0|1|2 t`
  save/restore with 1-10 stack slots):
  <https://invisible-island.net/xterm/ctlseqs/ctlseqs.html>; GNU Bash manual
  (`PROMPT_COMMAND` executes before the primary prompt):
  <https://www.gnu.org/software/bash/manual/bash.html>; DropTerm's own
  dependency chain: alacritty_terminal 0.25.1 `src/term/mod.rs` L42,
  L2221-2256 and vte 0.15.0 `src/ansi.rs` L1348-1353, L1742-1743 (local
  cargo checkout, `Cargo.lock`)
- DropTerm itself: `src/tabs.rs` L66-108, L304-332; `src/app.rs` L7,
  L575-585, L945-951, L1123-1135, L1505, L1615-1634, L3162-3181;
  `src/session.rs` L36-57, L93-95, L123-124; `src/tab_bar.rs` L188-191,
  L807-827; `src/terminal/engine.rs` L486-490; `src/config.rs` L103-119,
  L121-226, L572-612; `src/settings.rs` L403-497, L642-682, L807-840;
  `README.md` L137, L150, L172, L241
