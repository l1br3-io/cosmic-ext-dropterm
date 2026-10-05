# Session persistence

DropTerm saves your tab layout and reopens it on every launch - here is exactly what is saved, when saves happen, and how each failure mode behaves.

DropTerm reopens your tab layout on every launch: one fresh shell per saved tab, spawned in its saved working directory, with its title, and the tab that was focused when the session was saved is focused again. Toggle it with `session_persistence_enabled` (default `true`, Behavior section in the [settings drawer](settings.md)): `false` stops saves *and* skips restore at launch - the app starts with a single fresh tab, and the existing `session.ron` is left untouched on disk, so re-enabling picks the session back up.

What is saved - deliberately the same scope Guake and Yakuake settle on:

- The tab list in tab-bar order: per-tab `title`, a user rename (`title_override`) if any, the `shell` recorded when the tab was spawned, the tab's `cwd` at save time, and the `profile` id a tab launched under (if any)
- The focused tab
- A schema `version`

That is all - eight schema fields (`version`, `active`, `tabs`, and per tab `title`, `title_override`, `shell`, `cwd`, `profile`). Scrollback, environment, and running processes are deliberately *not* saved: a restored tab is a fresh spawn - your shell, or a profile's command - in a familiar place, never a resumed program. And because each tab's shell is recorded at spawn, changing the configured `shell` mid-run only affects newly opened tabs.

Profile tabs are the explicit opt-in that makes a command tab persistent where a raw `--cmd` tab deliberately is not: the session saves the id beside the cwd, and restore re-resolves it against the *current* config, so a profile edited between save and relaunch launches the edited command. A saved id the list no longer carries falls back to a plain shell in the re-validated saved cwd - it never chains through `default_profile`.

Saving happens on every layout change (open, close, reorder, rename, tab switch, or a shell's `cd`), debounced to one write within half a second; continuous changes coalesce into one write per interval. Each write is atomic - serialize to a temp file next to the target, fsync, rename over the old file - so a crash mid-write leaves the previous session intact. The trade-off for debouncing: a hard kill can lose the last ~500 ms of layout changes.

The state lives in its own file, fully separate from the config (the directory is created on first save):

```
~/.local/state/io.github.l1br3_io.CosmicExtDropterm/session.ron
```

(`$XDG_STATE_HOME` is honored when set to an absolute path.)

Failure behavior:

- First run (no state file): silent - a single fresh tab, no warnings.
- A corrupt, unreadable, or future-versioned file is moved to `session.ron.bak` (replacing any older backup), logged at `warn`, and the app starts fresh. Rename it back to `session.ron` to give it another chance.
- A saved `cwd` that no longer exists spawns that shell in `$HOME` (a profile tab first tries the profile's current `cwd`, then `$HOME`).
