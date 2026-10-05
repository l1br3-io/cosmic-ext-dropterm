# Research: how famous terminals do profile settings — the canonical four in depth, the no-profile camp as contrast

Date: 2026-10-05
Question: *How do the canonical "profile" terminals — Windows Terminal, iTerm2, GNOME
Terminal, Konsole — implement profile settings (model, identity, storage, defaults,
settings UI, default-profile semantics, live-apply), and what should DropTerm's pending
`profile-settings` module (SPEC.md:31, spec not yet written) copy or avoid?*

Scope note: lean pass — the four canonical profile terminals in depth from their
official docs, plus one-paragraph notes on kitty/Alacritty/WezTerm as the "no profiles"
contrast. Terminal.app, Warp, Tilix, Tabby were descoped when the original 10-terminal
background deep-dive was cancelled for taking too long. All sources fetched 2026-10-05;
every claim below cites its primary source. Unverified claims are marked.

## TL;DR

**All four canonical terminals converge on the same core: a profile is a named bundle
whose load-bearing fields are the command line and the starting directory, with
appearance riding along in the same object.** Konsole's handbook literally defines
profiles as a way to "quickly and easily automate the running of common commands,"
exemplified by *ssh into another machine*, *starting an irc session*, and *tail to
watch a file* — one-for-one DropTerm's `Profile { name, command, cwd }` spawn-preset
model. Windows Terminal's minimal hand-written profile example is
`{ commandline, name, icon, startingDirectory }` and nothing else. Identity diverges:
Windows Terminal and iTerm2 use GUIDs (both docs explain why — tracking profiles
across renames and syncing), GNOME Terminal and Konsole use per-profile storage keys
(UUID profile dirs / one file per profile in `$XDG_DATA_HOME/konsole`). None of the
four use the display name as the id, but all four accept names in the places users
type them. Two mechanisms recur everywhere: **defaults inheritance** (WT's
`profiles.defaults` block above the `list`; iTerm2 unspecified attributes inherit
from the default profile or a named parent) and **a guarded default profile**
(GNOME: the default cannot be deleted, only replaced; WT: `defaultProfile` is
*required*, takes a GUID or a name, and backs every unqualified new tab — the same
"default applies to all plain spawns" ruling DropTerm already made in
`profile-launch`). The settings UI pattern is a flat sidebar list + a single edit
pane with a row-arrow or kebab menu for Clone/Rename/Delete/Set-as-default, saved
immediately (GNOME is explicit: "Changes to profile are saved immediately", no
Apply/OK). Live-apply is universally *new-sessions-only* for spawn fields — GNOME's
docs say to "open a new Terminal tab or window to see how the custom command
executes." The no-profile camp (kitty/Alacritty/WezTerm) answers the same need with
composable single-config files plus launcher menus — WezTerm's `launch_menu`
(list of `{label, args}` shown in a launcher overlay) is the closest structural
cousin to DropTerm's planned newtab-button profile dropdown.

## Findings

### 1. Windows Terminal — profiles as first-class JSON objects with GUID identity, `defaults` inheritance, and dynamic generators

All claims: learn.microsoft.com Windows Terminal docs.

- **Model & identity.** A profile is an object in the `profiles.list` array of
  `settings.json`. Identity is a GUID; the display `name` is a separate required
  property ("the name of the profile that will be displayed in the dropdown menu",
  also passed as the initial tab title — Profile - General § Name,
  <https://learn.microsoft.com/en-us/windows/terminal/customize-settings/profile-general>).
  The default profile is referenced *by GUID or profile name*:
  `defaultProfile` "Accepts: GUID or profile name as a string", **Required**, default
  PowerShell's GUID (Startup § Default profile,
  <https://learn.microsoft.com/en-us/windows/terminal/customize-settings/startup>).
- **Fields.** General profile properties: `name` (required), `commandline` (optional,
  default `"cmd.exe"`), `startingDirectory` (default `%USERPROFILE%`; the docs
  distinguish *unset* from explicitly-`null` — unset gives a fixed default, `null`
  inherits from where you launched), `icon`, `tabTitle`, `elevate`, `hidden`
  (Profile - General, same page). Appearance/advanced properties live on sibling doc
  pages (Profile - Appearance, Profile - Advanced — customize-settings TOC).
- **Defaults inheritance.** Settings meant for all profiles go in a `defaults`
  object *above* the `list` ("If you'd like a setting to apply to all of your
  profiles, you can add it to the `defaults` section"), e.g. `elevate` set there
  applies to every profile unless overridden (Profile - General, opening paragraph
  and § Automatically run as Administrator).
- **Ordering is functional.** Position in `list` determines the profile *index*,
  which maps to launch key combos — first entry = Ctrl+Shift+1 (Profile - General §
  Profile Ordering). So reordering is a real feature, not cosmetics.
- **Default-profile semantics.** `defaultProfile` backs "typing Ctrl+Shift+T, typing
  the key binding assigned to `newTab`, running `wt new-tab` without specifying a
  profile, or clicking the '+' icon" — i.e. *every* unqualified spawn (Startup §
  Default profile). Dangling references aren't discussed; the docs assume the
  settings UI/JSON keeps it valid.
- **Dynamic profiles.** Generators auto-create profiles for WSL/PowerShell/Azure/SSH
  with a `source` property marking provenance; users hide them with `hidden: true`
  or disable generators via `disabledProfileSources` (Dynamic profiles,
  <https://learn.microsoft.com/en-us/windows/terminal/dynamic-profiles>; Startup §
  Disable dynamic profiles). The docs' own "add a third party profile" examples are
  minimal `commandline` + `name` + `icon` + `startingDirectory` objects — evidence
  that this field set is the credible minimum.
- **UI/JSON parity.** The Settings UI mirrors the JSON (the startingDirectory section
  explicitly notes the UI's **Browse...** button as the UI equivalent of typing the
  path). JSON remains the escape hatch, documented alongside.

### 2. iTerm2 — Preferences > Profiles with eight subsections; GUID identity; file-based "Dynamic Profiles" with parent inheritance

All claims: iterm2.com/documentation.

- **Model & identity.** Profiles live in the Preferences window's Profiles section
  (Preferences TOC: General/Appearance/**Profiles**/Keys/... —
  <https://iterm2.com/documentation-preferences.html>). Each profile has a Guid
  ("globally unique identifier... used to track changes to the profile over time. No
  other profile should ever have the same guid") plus a required Name (Dynamic
  Profiles § Property List Format,
  <https://iterm2.com/documentation-dynamic-profiles.html>). Regular profiles are
  stored in the macOS settings database (plist); Dynamic Profiles are the file-based
  escape hatch.
- **Fields (Profiles > General.** <https://iterm2.com/documentation-preferences-profiles-general.html>):
  Name, Shortcut key ("opens a new tab; hold option to open a new window"), Tags,
  Badge, Title composition, Icon, **Command** — a mode picker: *login shell*,
  *custom shell* (path, run as login shell), *custom command*, or *SSH* (iTerm2's
  integrated SSH with a Configure dialog) — plus **Run command through your shell**
  ("your dotfiles get a chance to run, so `$PATH`... are in place before the command
  starts"), **Send Text at Start**, and **Initial Directory** (home / same directory
  as the current session / a specified directory). Command-line `$$VAR$$` terms
  prompt for a value at session start. Other Profiles subsections: Colors, Text,
  Window, Terminal, Session, Keys, Advanced
  (<https://iterm2.com/documentation-preferences-profiles.html>).
- **File-based profiles + inheritance.** JSON/XML plists dropped in
  `~/Library/Application Support/iTerm2/DynamicProfiles` are watched live and
  reloaded on change. "Any attribute not specified will inherit its value from the
  default profile, or a specified 'parent' profile" (`Dynamic Profile Parent Name` /
  `Parent GUID`). UI edits to a dynamic profile are *not* written back unless the
  profile opts in with `"Rewritable": true`. The doc's canonical use case is a list
  of ssh host profiles — `Name`/`Guid`/`Custom Command`/`Command: ssh foo.example.com`
  — i.e. exactly DropTerm's spawn preset. Export path: **Other Actions > Save
  Profile as JSON**.
- **Live-apply.** Not addressed on the pages fetched (low confidence, not asserted);
  the parent-inheritance and rewritability rules above are the documented dynamics.

### 3. GNOME Terminal — profiles as settings collections in a Preferences sidebar; UUID-keyed storage; strictest default-profile guardrails

All claims: help.gnome.org/users/gnome-terminal/stable (unless marked).

- **Model.** "A profile is a collection of Terminal settings" — name, font and
  colors, key-compat, scrolling — configurable to "run a custom command or shell,
  set a profile exclusively to connect to remote computers using SSH or set a
  profile that launches a GNU Screen session" (Manage profiles,
  <https://help.gnome.org/users/gnome-terminal/stable/pref-profiles.html>).
- **UI pattern.** Preferences shows a Profiles **sidebar list**; **+** creates;
  a row-arrow menu per profile offers **Clone…, Rename…, Delete…, Set as default**;
  the default is marked with a check mark. One edit pane per selected profile with
  the usual subsections (Command, Colors, Scrolling...). Saved instantly, no Apply
  step: "Changes to profile are saved immediately."
- **Guardrails.** "The default profile cannot be deleted. If you wish to delete that
  profile, first set another profile as default." And a warning that the default
  profile cannot be restored to factory settings — the docs advise cloning the
  default and editing the clone instead (Manage profiles § Edit/Delete).
- **Custom command.** "Run a custom command instead of my shell" checkbox + freeform
  text: "The command will be passed to the terminal exactly as you write it,
  including any arguments"; exit behavior is a separate per-profile setting
  (Custom commands and shells,
  <https://help.gnome.org/users/gnome-terminal/stable/pref-custom-command.html>).
- **Default-profile & live-apply semantics.** The default profile is used "when
  Terminal could not otherwise decide which profile to use. For example... opens its
  first terminal window. In contrast, when you use a menu entry or keyboard shortcut
  of Terminal to open a new terminal, the profile of the existing terminal is used"
  — i.e. fresh windows get the default, in-window spawns inherit the current tab's
  profile (Manage profiles § Set a default profile). Profile *switching* of a live
  tab exists (Terminal ▸ Change Profile) but that's appearance re-theming; spawn
  changes are new-sessions-only: "Open a new Terminal tab or window to see how the
  custom shell or command executes" (Custom commands and shells).
- **Storage (low confidence, unverified here).** Community knowledge says profiles
  persist as UUID-keyed relocatable gsettings/dconf paths under
  `org.gnome.Terminal.Profiles`; the user docs don't cover storage and the schema
  fetch from GNOME's GitLab failed in this pass — treat as plausible-but-unverified.

### 4. Konsole — profiles *are* spawn presets; one INI-ish file per profile under `$XDG_DATA_HOME/konsole`

All claims: Konsole Handbook (KDE Gear 24.05), "Profiles" chapter,
<https://docs.kde.org/stable_kf6/en/konsole/konsole/profiles.html>.

- **Model — the closest match to DropTerm.** "Profiles allow the user to quickly and
  easily automate the running of common commands. Examples could include: ssh into
  another machine; starting an irc session; use tail to watch a file."
- **Fields.** The creation flow asks for exactly: a **name** ("the name that will
  show in the menu, and will be the default label instead of Shell when you start a
  session of this type"), a **command** ("Enter a command just as you normally would
  if you opened a new shell and were going to issue that command... you might type
  `ssh administration`"), and — on the dialog's *other tabs* — per-profile
  appearance: "a different font, color scheme, `TERM` type and many other settings."
  Name + command + (implicitly) cwd is the core; appearance is tabbed alongside.
- **Storage.** "All new and changed profiles are saved in the user's local home
  folder in `$XDG_DATA_HOME/konsole`" — one file per profile (the handbook states
  the directory; the per-file `.profile` format is not detailed on this page).
- **UI pattern.** Settings → Manage Profiles → New Profile → fill tabs → OK; the
  new profile immediately appears in the Manage Profiles list and the menus.

### 5. kitty, Alacritty, WezTerm — the no-profile camp

- **kitty.** No profiles. One `kitty.conf` (created commented on first edit via
  ctrl+shift+f2), auto-reloaded on change (`auto_reload_config`), composed via
  `include`/`globinclude`/`envinclude`/`geninclude` directives; multi-window
  *startup layouts* come from a `startup_session` file; per-instance overrides via
  `kitty --config`/`KITTY_CONFIG_DIRECTORY`
  (<https://sw.kovidgoyal.net/kitty/conf/>, § Configuring kitty).
- **Alacritty.** No profiles and deliberately no GUI editor ("you won't find...
  niceties like a GUI config editor" — README FAQ,
  <https://github.com/alacritty/alacritty#faq>). Single TOML with `import = [...]`
  ("loaded in order... if a field is already present in a previous import, it will
  be replaced"), `working_directory`, `live_config_reload = true` by default, and
  `[terminal] shell = { program, args }` (man alacritty(5),
  `extra/man/alacritty.5.scd` @ master).
- **WezTerm.** No profiles; a Lua config returning a table, auto-reloaded
  (ctrl+shift+r), with `--config` CLI overrides that always win, and per-window
  runtime overrides (<https://wezterm.org/config/files.html>). The profile-shaped
  surface is config-declared: `launch_menu` (a list the launcher overlay shows —
  the structural cousin of DropTerm's planned profile dropdown), `default_prog`,
  `ssh_domains`/`wsl_domains`, and the `gui-startup` event for scripted startup
  workspaces (all entries in the config reference index, same site).

## What DropTerm should take from this (for `profile-settings`, SPEC.md:31)

Module 4 is already specced *in intent* as "Settings-window Profiles page: list /
add / edit / delete / reorder + the default-profile picker dropdown" (SPEC.md:31,
row not yet written — line 98), on top of the shipped `profile-model`
(`Profile { name, command, cwd }`, name-is-id, `default_profile: Option<String>`,
dangling default resolves to plain shell). The findings above say:

- **(a) UI pattern — sidebar list + edit pane + row menu is the universal shape.**
  GNOME's Preferences sidebar with per-row Clone…/Rename…/Delete…/Set as default
  and a check mark on the default is the most directly copyable layout, and it maps
  cleanly onto libcosmic `widget::settings` sections in the existing settings
  window (docs/settings.md). Duplicate/Clone deserves its row-menu slot (GNOME and
  iTerm2 both offer a copy path; hand-editing RON is the DropTerm equivalent of
  iTerm2's dynamic-profile files). Reorder is worth keeping only as list-order =
  newtab-dropdown order — WT is the only one that makes ordering *functional*
  (key-combo indices), and DropTerm's module 3 dropdown maps no indices to keys,
  so drag-and-drop can be cut if it costs anything.
- **(b) Fields — name/command/cwd is validated as the credible minimum.** Konsole's
  create flow asks for name + command (+ appearance on other tabs) and WT's
  hand-written examples are `commandline`/`name`/`startingDirectory`/`icon`. The
  one cheap, high-visibility addition the big terminals all make is **icon** (WT:
  icon shows "within the tab, dropdown menu, jumplist"; iTerm2: tab bar + title
  bar) — directly useful in module 3's dropdown. Appearance-in-profile (GNOME
  style) should stay out of scope: DropTerm themes globally today, and none of
  these fields is in the approved model.
- **(c) Default-profile semantics — DropTerm's existing rulings match WT, the most
  profile-native terminal.** WT applies `defaultProfile` to *every* unqualified
  spawn (+ button, newTab keybind, `wt new-tab`), exactly DropTerm's profile-launch
  ruling that the default applies to all plain spawns. GNOME's alternative (in-window
  new tabs inherit the *current tab's* profile) is a real design fork worth knowing
  about but not adopting now — it requires tracking a per-tab profile, which the
  model already does via session re-resolve-by-id, but the ruling is already in.
  On deletion: GNOME forbids deleting the default; DropTerm's simpler
  already-approved rule (dangling `default_profile` → plain shell, load never
  "fixes" it) is looser but coherent — keep it, and have the settings UI clear the
  key (or warn) when the named profile is deleted, mirroring how WT keeps
  `defaultProfile` valid through its UI.
- **(d) Live-apply — spawn edits are new-sessions-only, everywhere that documents
  it.** GNOME is explicit ("open a new Terminal tab or window to see how the custom
  command executes"); WT/kitty/Alacritty/WezTerm achieve liveness only for
  *appearance* config reloads. For DropTerm this is also the only implementable
  semantics (a running alacritty_terminal PTY can't be re-exec'd), so state it in
  the module-4 spec rather than leaving it implicit.
- **Identity — do not chase GUIDs yet.** GUIDs exist in WT/iTerm2 to track profiles
  across renames and machine sync (and to key generated profiles); Konsole proves a
  small-terminal-scale ecosystem runs fine on file-per-profile without them. With
  name-is-id already approved and uniqueness enforced at write time by this very
  module, GUIDs would buy nothing today — revisit only if profiles ever become
  syncable or generated.

Sources: all URLs above, fetched 2026-10-05. Repo citations: SPEC.md lines 31, 98,
457–460; SPEC-profile-model.md (accepted criteria 1–6, design decisions);
docs/settings.md (current settings window).
