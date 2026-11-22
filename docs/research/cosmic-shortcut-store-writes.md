# Research: how does COSMIC write and execute the custom-shortcuts store, what would depending on `cosmic-settings-config` pull in, and does the installed 1.11.0 match the master behavior the first pass read?

**Question:** The global-toggle-cosmic-writer module is blocked on three facts
its spec marked "not verified": cosmic-config's write behavior (atomicity,
file mode, pretty-printer options), what depending on the real
`cosmic-settings-config` crate would pull into DropTerm's build, and whether
the installed cosmic-settings 1.11.0 / cosmic-comp snapshot match the
main-branch behavior the 2026-11-22 pass read. It also needs cosmic-comp's
`Spawn` execution pinned (shell or direct — it decides the spawn-command
string), and it owes the corrections to the immutable
[global-toggle-hotkeys](global-toggle-hotkeys.md) record. What are the facts?

**Date:** 2026-11-22

## Findings

All source pins fetched 2026-11-22 against the upstream `master` branches and
the `epoch-1.11.0` release tags unless noted.

### 1. TL;DR — the verdict

**cosmic-config writes each entry with `ron::ser::to_string_pretty` under
RON's default `PrettyConfig` and replaces the file through the `atomicwrites`
crate (temp file + rename, no explicit mode); cosmic-comp executes `Spawn`
commands through `/bin/sh -c`; the real `cosmic-settings-config` crate would
drag floating unpinned git dependencies (cosmic-config from libcosmic,
`atomicwrites` from a personal fork) plus `notify`, `dirs`, `xdg`, and a
path/derive coupling that its own manifests never pin; and the installed
epoch-1.11.0 tags carry the same behavior that was read on master.** The
on-disk `custom` file is a per-key file under a versioned directory (the
version lives in the directory name, not a header), and COSMIC Settings'
own add path confirms the first pass's wipe-hazard reading: any read error
other than not-found proceeds with an empty map, so the next edit there
would replace the file with one entry.

### 2. The store on disk — a per-key file, version in the directory

- `Config::id_v` builds the path `.../cosmic/<name>/v<version>/<key>`;
  `sanitize_name` blocks `.`/`..` components for both the config name and
  the keys. The version is the **directory name** — there is no version
  header inside the file (cosmic-config `src/lib.rs`, master).
- The shortcuts store's shape is
  `#[derive(..., CosmicConfigEntry)] #[version = 1] pub struct Config {
  pub defaults: Shortcuts, pub custom: Shortcuts, pub system_actions:
  SystemActions }` — so `defaults` and `custom` are **two sibling files**
  in the same versioned directory, and `Shortcuts(pub HashMap<Binding,
  Action>)` (cosmic-settings-daemon `config/src/shortcuts/mod.rs`,
  master). The effective shortcuts are defaults overridden by custom
  (`HashMap::extend`; custom wins on key collision).
- DropTerm's GU1 reader already matches this: `custom_shortcuts_path`
  ends at `com.system76.CosmicSettings.Shortcuts/v1/custom`, the ported
  `Binding` is `deny_unknown_fields` over the same fields, and the
  action values are opaque RON (`RawValue`) carried verbatim.

### 3. cosmic-config's write path — default pretty RON, atomic rename, no explicit mode

- The value is serialized with `ron::ser::to_string_pretty(&value,
  ron::ser::PrettyConfig::new())` — **no custom options**: RON's default
  pretty config (cosmic-config `src/lib.rs`, master, the
  `ConfigTransaction::set` path).
- The file is replaced through the `atomicwrites` crate: `AtomicFile::new(
  key_path, OverwriteBehavior::AllowOverwrite).write(|file|
  file.write_all(data.as_bytes()))` — temp file + rename, with temp names
  starting `.atomicwrite` (the inotify watcher skips them explicitly).
  No explicit file mode is set anywhere on the path (same file).
- Consequence for a hand-rolled writer: serializing our structurally
  identical map with `ron::ser::to_string_pretty(.., PrettyConfig::new())`
  and replacing the file through a same-directory temp file + `rename`
  reproduces the on-disk contract (format by construction; atomicity by
  the same rename semantics). `HashMap` iteration order is not
  stabilized on either side — cosmic's own rewrite reorders entries too,
  so order parity is not part of the contract.

### 4. The keyboard page's add/remove and conflict rule — the verdict rules pinned

All from cosmic-settings
`cosmic-settings/src/pages/input/keyboard/shortcuts/common.rs` and the
daemon's `config/src/shortcuts/binding.rs`, master:

- **Read-modify-write confirmed.** `config_add` reads `custom`
  (`shortcuts_config()`), inserts one entry, writes the whole map back
  (`shortcuts_config_set` → `config.set("custom", shortcuts)`).
  `config_remove` reads, `retain`s out the one binding, writes.
- **The wipe hazard confirmed with the exact error branch.**
  `shortcuts_config()` maps `Err(..)` with `io::ErrorKind::NotFound` to
  the default (empty) map **and every other error to the same empty map**
  (logged, not surfaced) — so a `custom` file that fails to parse is
  silently treated as empty, and the next add/remove there writes that
  one-entry map over the user's whole file.
- **Conflict = present in the effective map with a non-`Disable` action.**
  `submit_binding` gates on `config_contains(&new_binding)`, which looks
  the binding up in `shortcuts_system_config()` — defaults extended by
  custom — and `.filter(|action| *action != Action::Disable)`. A chord
  held only by a `Disable` entry is **not** a conflict; committing over
  it replaces the disable, exactly as COSMIC's own replace flow would.
- **The conflict UI replaces rather than refuses** ("replace-shortcut-dialog",
  naming `binding.description.unwrap_or(action)`; Apply removes the
  holder then adds). DropTerm's writer refuses instead (its spec's
  stated stance), naming the holder the same way: description, else the
  chord.
- **Validity is `Binding::is_set`**: `(has_modifier() && key.is_some())
  || is_super() || key.is_some_and(|key| !is_forbidden_unmodified_keysym(key))`,
  where `is_super()` is **bare logo only** (no key, no other modifier) —
  Super alone is a legal binding — and `has_modifier()` is any of
  logo/shift/alt/ctrl. So a bare Shift/Ctrl/Alt press is not a settable
  binding, and an **unmodified** key is settable unless its keysym is in
  the forbidden set.
- **The forbidden unmodified keysyms** (`is_forbidden_unmodified_keysym`,
  pinned by raw keysym range): A–Z, a–z, 0–9, Space, Tab /
  ISO_Left_Tab / KP_Tab, Return / KP_Enter, Mode_switch, Scroll_Lock,
  Sys_Req, Multi_key, Num_Lock, Caps_Lock, ISO_Lock and the ISO group
  switches, the Virtual_Screen keys, Terminate_Server,
  AudibleBell_Enable, and the Kana/Arabic/Cyrillic/Greek/Hebrew/Thai/
  Hangul national ranges.
- The page's own capture ignores bare modifier presses while listening
  (Super/Alt/Control/Shift `KeyPressed` return early) — the seat's
  "bare modifiers keep the capture armed" contract.

### 5. The real crate's dependency tree (the hand-rolled route's rival)

- `cosmic-settings-config` (cosmic-settings-daemon `config/Cargo.toml`,
  master) depends on: **cosmic-config from
  `git = "https://github.com/pop-os/libcosmic"` — unpinned, no rev**;
  serde 1 (derive); serde_with 3.24; tracing 0.1; xkbcommon 0.9;
  ron 0.12.
- `cosmic-config` (libcosmic `cosmic-config/Cargo.toml`, master) is
  edition **2024**, version 1.0.0, and depends on: **atomicwrites from
  `git = "https://github.com/jackpot51/rust-atomicwrites"` — a personal
  fork, unpinned**; notify 8.2.1; dirs; xdg (unix); tracing; ron;
  serde; cosmic-config-derive (path sibling, behind the `macro`
  feature); and — default-feature `subscription` — **iced_futures from
  the libcosmic workspace's own iced fork** (avoidable only with
  `default-features = false, features = ["macro"]`).
- Neither upstream manifest pins a rev: the released
  cosmic-settings-daemon's `Cargo.toml` at `epoch-1.11.0` carries the
  same floating `git = "https://github.com/pop-os/libcosmic"` line. A
  DropTerm build would have to pin the revs itself, and every upgrade
  re-pins them — against a module whose spec demands it "stay cheap to
  delete when the portal lands".

### 6. Spawn execution — `/bin/sh -c`, quoting matters

`spawn_command` (cosmic-comp `src/input/actions.rs`) runs the raw command
string through `std::process::Command::new("/bin/sh").arg("-c").arg(&command)`
with the Wayland/X11 displays and an XDG activation token in the
environment, spawned on a background thread. Verified identical at the
`epoch-1.11.0` tag (the `/bin/sh` call site). So: a first word without
whitespace survives unquoted; a path containing whitespace must be
quoted to survive the shell — and DropTerm's own recognition
(`is_toggle_spawn` splits on whitespace) can only recognize unquoted
single-word program heads either way.

### 7. Release match — epoch-1.11.0 carries the same behavior

- The daemon at `epoch-1.11.0`: `binding.rs` carries
  `is_forbidden_unmodified_keysym`, `is_set`, and `is_super`;
  `shortcuts/mod.rs` carries `#[version = 1] pub struct Config` with
  `pub custom: Shortcuts`.
- cosmic-comp at `epoch-1.11.0`: the `/bin/sh -c` spawn path (finding 6).
- The remaining gap is upstream's own: cosmic-config rides libcosmic
  master with no rev even in the released daemon (finding 5), so "the
  installed release" links a master snapshot whose exact commit is
  unstated upstream. The master read is the format contract; the
  snapshot identity is not knowable from upstream's manifests alone.

### 8. Corrections to the global-toggle-hotkeys record (immutable; superseding where they conflict)

- **"COSMIC's own config story is transitional" does not apply to the
  store the writer targets.** That ground cites cosmic-epoch#288, which
  concerns the old `~/.config/cosmic/compositor/config.ron`; its
  maintainer closed it naming cosmic-config and cosmic-settings-daemon
  as the proper implementation — the versioned
  `com.system76.CosmicSettings.Shortcuts/v1` store (findings 2–3), which
  is what the writer writes and cosmic-comp watches.
- **The zero-peer-precedent ground stands.** This pass found no new
  runtime config-writer among COSMIC's own components either: cosmic
  writes it from its Settings app (the user's seat), and nothing in the
  surveyed ecosystem writes another app's store at runtime.
- The first pass's "Not verified" items are now verified: write
  behavior and pretty-printer (finding 3), the dependency tree
  (finding 5), Spawn execution (finding 6), release match (finding 7).

## What DropTerm has today

Grounded in the working tree (2026-11-22): the GU1 reader
(`dropterm/src/core/global_shortcut.rs`, committed `b91c1bd`) with the
degrade contract (`ReadCustom`), the GU4 opaque action values (`5210df7`),
and the onboarding (O1–O4, `038a9ef`..`6f20ee3`) whose configure-for-me
seat awaits this module. `ron` 0.12, `serde`, and `xkbcommon` 0.8 are
already direct dependencies; the workspace builds edition 2024 at
rust-version 1.93.

## Recommendation

**The writer's dependency route: hand-rolled (the spec's earlier
default), not the real crate.** The on-disk v1 format is stable and
versioned by directory; DropTerm's shapes are already the on-disk
contract (same serde call: `ron::ser::to_string_pretty(..,
PrettyConfig::new())`); the wipe hazard — the real crate's strongest
argument — is closed by the spec's own discipline (fresh read
immediately before the write, refuse every write on any parse failure,
re-parse the output before writing, foreign action text verbatim), all
pinned by tests; and the discipline's failure mode on future format
drift is graceful by design: a `deny_unknown_fields` read of an unknown
v1 shape degrades to `Unparseable`, `configure_for_me` turns off, and
the session falls back to "Set up manually" — never a guess, never a
wiped file. The real crate's residual advantage (their own types keep
parsing a drifted v1) buys adaptivity this module does not need at the
cost of two unpinned git dependencies, the notify tree, and an exit
cost against the spec's cheap-to-delete requirement.

**The spawn command string: the running binary's absolute path plus
`toggle`** (`std::env::current_exe()`), unquoted — the spec's stated
default. `/bin/sh -c` (finding 6) executes it directly, `is_toggle_spawn`
recognizes it (stem match, whitespace-free first word), and the dev
build stays self-consistent (its own path, its own stem). A whitespace
install path would break both the shell split and DropTerm's
recognition; shell-quoting fixes the former but breaks the latter, so
it is rejected rather than partial. Both the onboarding walk and the
README keep naming `dropterm toggle` as the manual form; the
PATH-dependent string remains what a user types by hand, while the
writer pins the binary it actually runs.

## Unverified

- **The exact temp-file mode atomicwrites ends up with** (crate default,
  not pinned from its source); DropTerm's own temp+rename will match
  observed behavior (`0644`-class) but the byte-level parity claim is
  not pinned.
- **The exact libcosmic master snapshot the installed 1.11 binaries
  link** (upstream pins no rev, finding 7) — the release-match claim is
  tag-level, not commit-level.
- **Whether the installed cosmic-settings 1.11.0 keyboard page matches
  master's `common.rs` line-for-line** — the verdict rules were checked
  at master and the daemon-side shapes at the 1.11.0 tag; the page file
  itself was not re-read at the tag (the daemon's shapes are the
  file-format contract; the page is behavior DropTerm mirrors, and its
  master form is what the onboarding's seat copies).
- **HashMap ordering parity** — neither side stabilizes iteration order,
  so byte-identity of a rewritten file against a cosmic-written one is
  not a contract on either side (semantic identity is).
- **The first-run onboarding precedent leg** (Zellij's flow, among
  peers) named in summon-onboarding's Open Question 4 is **not in this
  record** — it informs the onboarding, which already shipped its shape,
  and is left for its own record if that shape is ever revisited.

## Sources

All fetched 2026-11-22, `master` unless a tag is named.

- cosmic-config @ libcosmic master: `cosmic-config/src/lib.rs`
  (`id_v`/`sanitize_name` path layout, `to_string_pretty` +
  `PrettyConfig::new()`, `AtomicFile` write, `.atomicwrite` watcher
  skip, no explicit mode), `cosmic-config/Cargo.toml` (dependency list,
  edition 2024, features) — <https://github.com/pop-os/libcosmic>
- cosmic-settings-daemon @ master and `epoch-1.11.0`:
  `config/src/shortcuts/mod.rs` (`#[version = 1] Config`, `Shortcuts`
  map, `defaults`+`custom` siblings), `config/src/shortcuts/binding.rs`
  (`is_set`, `is_super`, `has_modifier`,
  `is_forbidden_unmodified_keysym` full list, `from_str` rules),
  `config/Cargo.toml` (`cosmic-settings-config` dependency list) —
  <https://github.com/pop-os/cosmic-settings-daemon>
- cosmic-settings @ master:
  `cosmic-settings/src/pages/input/keyboard/shortcuts/common.rs`
  (`config_add`/`config_remove` read-modify-write,
  `shortcuts_config()`'s empty-map fallback on any read error,
  `shortcuts_system_config()` defaults-over-custom merge,
  `config_contains` with the `Disable` filter, `submit_binding`'s
  replace dialog naming `description.unwrap_or(action)`, the capture's
  bare-modifier early return) — <https://github.com/pop-os/cosmic-settings>
- cosmic-comp @ master and `epoch-1.11.0`: `src/input/actions.rs`
  (`spawn_command`: `/bin/sh -c`, display env, activation token,
  background thread) — <https://github.com/pop-os/cosmic-comp>
- cosmic-epoch issue #288 (the closed "transitional" ground, named in
  the corrections) — <https://github.com/pop-os/cosmic-epoch/issues/288>
