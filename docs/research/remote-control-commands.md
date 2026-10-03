# Research: remote tab-management commands for DropTerm

Date: 2026-10-03
Question: *Which remote/CLI tab-management commands — close, duplicate, rename,
and the adjacent navigation set — do the terminals with proven remote-control
surfaces ship, with what argument and error conventions, and which subset maps
onto DropTerm's fire-and-forget single-instance seam?*

Scope (maintainer-directed): only the three terminals with proven, documented
remote-control surfaces — **kitty**, **WezTerm**, **Guake**. For the wider
inventory (Yakuake/Konsole D-Bus, GNOME Terminal open-only, Ghostty/foot
remote-control-free) see `terminal-de-integration.md`, row 4.

## TL;DR

All three peers converge on the same small grammar — **open** (spawn/launch
with a cwd), **close** (kill a *targeted* tab), **title** (set a sticky custom
title, with a reset escape) — and diverge only on addressing and reply
contracts. Four findings drive the recommendation:

1. **Close always defaults to the current/active tab; targeting is opt-in.**
   kitty `close-tab` closes "the active tab" unless `--match`/`--self` is
   given ([close-tab docs](https://sw.kovidgoyal.net/kitty/remote-control/#close-tab));
   WezTerm `kill-pane` kills the current pane unless `--pane-id` is given
   ([kill-pane docs](https://wezterm.org/cli/cli/kill-pane.html)); Guake's
   D-Bus `close_tab()` takes no argument at all — it closes the current page
   ([`dbusiface.py`](https://github.com/Guake/guake/blob/master/guake/dbusiface.py)).
2. **Nobody ships a literal remote "duplicate" verb.** Duplication is the open
   verb plus "take the cwd from the source": kitty composes it as
   `launch --type=tab --cwd=current` (`--copy-cmdline`/`--copy-env` exist as
   *optional* extras), WezTerm as `spawn --cwd …`, Guake's `add_tab(directory)`
   just takes a directory. DropTerm's `duplicate-tab` should be sugar over
   `new-tab` + the existing cwd-inherit tracking.
3. **Rename = set a sticky override on a target tab, with an escape hatch.**
   Guake resets with the sentinel `-`
   ([`main.py`, `--rename-tab`/`--rename-current-tab` help](https://github.com/Guake/guake/blob/master/guake/main.py));
   kitty resets by *omitting* the title — "If you do not specify a title, the
   title of the currently active window in the tab is used"
   ([set-tab-title docs](https://sw.kovidgoyal.net/kitty/remote-control/#set-tab-title)).
   DropTerm's `commit_rename` already implements exactly this shape
   (`src/tabs.rs:299`): non-empty sets `title_override`, empty clears it and
   shell titles resume.
4. **Error reporting requires a reply-capable client, which DropTerm doesn't
   have.** kitty errors on no-match (non-zero exit) *unless* you pass
   `--ignore-no-match`/`--no-response`; Guake does the bounds check in the
   CLI client itself (queries `get_tab_count()`, writes `invalid index` to
   stderr — and still exits 0). Over DropTerm's forwarded `ActivateAction`
   seam the only faithful translation is no-op + `warn` log, which is already
   the established degradation for `--cwd /nonexistent` and out-of-range
   `ActivateTab` (SPEC-remote-newtab, SPEC-tab-binds).

Index base is the one deliberate divergence: peers that expose indices are
0-based (Guake `--tab-index`, WezTerm `--tab-index` with negative-from-end;
kitty uses match expressions, base undocumented in the RC docs). DropTerm's
own `Ctrl+Shift+1..9` binds are 1-based, so a 1-based CLI stays internally
consistent — flagged below as a known peer divergence.

## What DropTerm has today

Grounded in the working tree:

- Subcommands `toggle | show | hide | new-tab [--cwd DIR] [--cmd CMD]`;
  `ToggleAction` at `src/app.rs:51`, `Display` mapping at `src/app.rs:72-78`.
- Transport: second launch forwards `ActivateAction { action, args }`; the
  primary re-parses through clap (`parse_forwarded_action`, `src/app.rs:3119-3137`)
  — malformed forwards degrade to a plain toggle, never an error surface.
  Fire-and-forget: the forwarding process exits before the action runs, so no
  reply/read-back is possible (SPEC-remote-newtab "Never" boundary).
- First launch applies the action at startup after `ensure_tabs`
  (SPEC-remote-newtab); `--cmd` tabs are ephemeral (excluded from session
  saves).
- Tab model: `title` + `title_override` (user rename; shell OSC titles never
  clobber it, `src/tabs.rs:71-80`), inline rename UI, drag reorder, per-tab
  cwd tracking (cwd-inherit), 1-based `Ctrl+Shift+1..9` activation with 9 =
  last (SPEC-tab-binds).

## kitty — `kitten @` remote control

Reference model. CLI (`kitten @ cmd`) talks over inherited window pipes or
`--to unix:SOCKET`, gated by `allow_remote_control`/`remote_control_password`
([docs](https://sw.kovidgoyal.net/kitty/remote-control/)). Commands wait for a
response by default (`--no-response` to opt out; "you will not be notified of
failures"), and no-match is an error unless `--ignore-no-match`.

- **Close**: `close-tab` — "The --match option can be used to specify complex
  sets of tabs to close"; default is the active tab (`--self` exists to close
  "the tab of the window this command is run in, rather than the active tab").
- **Rename**: `set-tab-title [TITLE...]` — sets the title of matched tabs;
  "By default, only the tab in which the command is run is affected. If you do
  not specify a title, the title of the currently active window in the tab is
  used" — i.e. the reset path is *omit the title*.
- **Open/duplicate**: `launch --type=tab --tab-title T --cwd current [CMD]` —
  `--cwd` accepts `current`/`last_reported`/`oldest`/`root` or a path;
  `--copy-cmdline`, `--copy-env`, `--copy-colors`, `--source-window` compose a
  full clone. Prints the new window id (a reply).
- **Navigate**: `focus-tab --match …` (match fields: `id`, `index`, `title`,
  `state`, `recent`, …). No remote tab-*move*.
- **Query**: `ls` — JSON tree of OS windows/tabs/windows with ids, titles,
  cwds, pids.

## WezTerm — `wezterm cli`

Namespace subcommands ([index](https://wezterm.org/cli/cli/index.html)):
`activate-tab`, `kill-pane`, `list`, `set-tab-title`, `spawn`, `send-text`,
`split-pane`, … Everything addresses **panes** by id; a tab is reached via its
panes. Default target: the `$WEZTERM_PANE` env var, else the focused pane of
the most-recently-interacted client.

- **Close**: `kill-pane [--pane-id N]` — "Immediately and without prompting,
  kills either the current pane, or the pane specified via the --pane-id
  parameter" ([docs](https://wezterm.org/cli/cli/kill-pane.html)). There is no
  `close-tab` verb; the tab goes when its last pane dies.
- **Rename**: `set-tab-title TITLE` (since 20230408; targets the current
  pane's tab; no documented reset sentinel —
  [source doc](https://github.com/wezterm/wezterm/blob/main/docs/cli/cli/set-tab-title.md)).
  Also `set-window-title`.
- **Open/duplicate**: `spawn [--cwd CWD] [--new-window] [-- PROG...]` — "When
  run with no arguments, it will spawn a new tab running the default program…
  Outputs the pane-id for the newly created pane on success"
  ([docs](https://wezterm.org/cli/cli/spawn.html)). No copy flags; duplicate =
  `spawn --cwd <from list>`.
- **Navigate**: `activate-tab` — `--tab-id`, `--tab-index`, `--tab-relative`.
  "Indices are 0-based, with 0 being the left-most tab. Negative numbers can
  be used to reference the right-most tab, so -1 is the right-most tab, -2 is
  the penultimate tab and so on"
  ([docs](https://wezterm.org/cli/cli/activate-tab.html)). (WezTerm even ships
  a `tab_and_split_indices_are_zero_based` config option.)
- **Query**: `list [--format json]` — WINID/TABID/PANEID/workspace/size/
  title/cwd per pane ([docs](https://wezterm.org/cli/cli/list.html)).

## Guake — flat CLI flags over `org.guake3.RemoteControl`

Closest structural peer: a drop-down terminal whose flat CLI flags map 1:1 to
D-Bus methods. Both layers read from source below.

- **Close**: D-Bus `close_tab()` (no arg — current tab) exists, but **no CLI
  flag exposes it** — `main.py` defines no close option
  ([`main.py`](https://github.com/Guake/guake/blob/master/guake/main.py),
  [`dbusiface.py`](https://github.com/Guake/guake/blob/master/guake/dbusiface.py)).
  Close is UI-only in practice.
- **Open**: `--new-tab DIR` → `add_tab(directory)`; positional `path` also
  opens a tab there; `--execute-command CMD` opens a tab *and* types the
  command into it (`execute_command`).
- **Rename**: `--rename-tab TITLE` targeted by `--tab-index` ("Specify the tab
  to rename. Default is 0. Can be used to select tab by UUID") or
  `--rename-current-tab TITLE`; both reset to default when TITLE is `-`.
  D-Bus: `rename_tab(i, s)`, `rename_tab_uuid(s, s)`, `rename_current_tab(s)`
  — the rename is a sticky user title (Vte custom title).
- **Navigate**: `--select-tab N` → `select_tab(tab_index)` — 0-based GTK
  notebook page. Invalid index handled **client-side**: the CLI fetches
  `get_tab_count()`, and on `N` outside `0..count` writes `invalid index: N`
  to stderr (exit code stays 0).
- **Query**: `--is-visible`, `--selected-tab` (prints index), `--selected-tablabel`,
  `--uuid-index UUID` — all print to the *client's* stdout, which works because
  the CLI itself is the D-Bus caller.
- **Duplicate**: absent at both layers.

## Comparison

| Verb | kitty | WezTerm | Guake |
|---|---|---|---|
| New tab | `launch --type=tab [--cwd …] [--tab-title …]` | `spawn [--cwd …] [PROG]` | `--new-tab DIR` / positional path |
| Close tab | `close-tab` (default active; `--match` sets) | `kill-pane` (default current pane) | D-Bus `close_tab()` only — **no CLI flag** |
| Duplicate | `launch --type=tab --cwd=current` (+`--copy-cmdline/env/colors`) | ✗ (compose `spawn --cwd`) | ✗ |
| Rename | `set-tab-title T [--match …]` (default: caller's tab) | `set-tab-title T` (current tab) | `--rename-tab T` + `--tab-index` / `--rename-current-tab T` |
| Rename reset | omit title → active window's title | undocumented | title `-` |
| Select nth | `focus-tab -m index:N` | `activate-tab --tab-index N` (0-based; `-1` = last) | `--select-tab N` (0-based) |
| Select relative | `focus-tab -m recent:N` | `activate-tab --tab-relative ±N` | ✗ |
| Move tab | ✗ (keybind action only) | ✗ (`move-pane-to-new-tab` ≠ move) | ✗ |
| List/query | `@ ls` (JSON) | `cli list` (JSON) | `--selected-tab*`, `get_tab_count/name` |
| Input | `send-text` / `send-key` | `send-text` | `execute_command` (new tab) |

## Recommended command set for DropTerm

Ranked by peer precedent, all over the existing seam as new `ToggleAction`
variants (`close-tab [--tab-index N]`, `rename-tab TITLE [--tab-index N]`,
`duplicate-tab`, `select-tab N`), reusing the clap re-parse round trip:

1. **`close-tab [--tab-index N]`** — no flag = active tab (3/3 peers default
   to current). `N` is 1-based, matching `Ctrl+Shift+1..9`; out-of-range or
   missing tab = no-op + `warn` (the seam cannot reply; peers error only
   because their clients get responses). Note the flag name follows the
   Guake/WezTerm `--tab-index` spelling.
2. **`rename-tab TITLE [--tab-index N]`** — non-empty TITLE sets the sticky
   `title_override` via the existing `commit_rename` (`src/tabs.rs:299`);
   empty TITLE clears it and dynamic shell titles resume. Peer groundings:
   Guake `-r`/`-i` + `-` reset; kitty set-tab-title active-default + omit-to-
   reset; WezTerm set-tab-title current-default.
3. **`duplicate-tab`** — new tab in the target (default active) tab's cwd,
   fresh shell, dynamic title; never copies the command. This is the peer
   norm (kitty `--cwd=current`, WezTerm/Guake cwd-only); kitty's deeper clone
   flags are the outlier, and DropTerm's `--cmd` tabs are ephemeral by spec,
   so command-copy is explicitly out.
4. **`select-tab N`** — 1-based positional index, out-of-range no-op, reusing
   the `ActivateTab` resolution already shipped by tab-binds. Precedent:
   Guake `--select-tab`, WezTerm `activate-tab`, kitty `focus-tab`. Optional
   refinement: WezTerm's negative-index convention (`-1` = last) composes
   naturally with the existing "9 = last" bind semantics.
5. **Not recommended now**: `move-tab` (keybind-only in every peer — keep the
   existing `Ctrl+Shift+PageUp/PageDown` binds as the only surface), and any
   query (`list`, `get-title`, `--is-visible`) — those need a reply-capable
   channel; even Guake implements them as direct D-Bus calls that print in the
   client, which the fire-and-forget seam structurally cannot offer
   (user-approved boundary in SPEC-remote-newtab).

First-launch rule: only *open-style* actions make sense when no instance is
running (remote-newtab already applies `new-tab` at startup); `close-tab`,
`rename-tab`, `duplicate-tab` of a nonexistent instance, and `select-tab`
should degrade to a plain start rather than fabricate state.

## Sources

- kitty remote control (official docs, incl. `close-tab`, `set-tab-title`,
  `launch`, `focus-tab`, `ls`, matching + response semantics):
  https://sw.kovidgoyal.net/kitty/remote-control/
- WezTerm CLI reference (official docs + repo docs):
  https://wezterm.org/cli/cli/index.html · `kill-pane` · `spawn` · `activate-tab` ·
  `list` ·
  https://github.com/wezterm/wezterm/blob/main/docs/cli/cli/set-tab-title.md
- Guake source (master): `guake/main.py` (CLI flags), `guake/dbusiface.py`
  (`org.guake3.RemoteControl`): https://github.com/Guake/guake
- DropTerm grounding: `src/app.rs:51-78,3119-3137`, `src/tabs.rs:71-80,285-299`,
  `SPEC-remote-newtab.md`, `SPEC-tab-binds.md`, `SPEC.md`.
