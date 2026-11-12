# Chunk: config-store — plain XDG configuration

Spec: [SPEC-config-store.md](../SPEC-config-store.md) · Module id:
`config-store` · Phase 1, lane B · branch `port/config-store` (worktree
`../ced2`, from day one) · ships in the same release as `app-identity`
(one release, no break note — owner ruling 2026-11-12:
pre-distribution, no users).

**Files touched:** `src/config.rs` (the derive at `:10,186` leaves),
`src/config/tests.rs` (the 40 `with_custom_path` fixtures), `src/app.rs`
(regions `:2068` handle, `:2320` watcher, `:1993-2008` completions path),
`src/settings.rs` (`:651,2096,2120` the three writers), a new store
module under `src/config/`, `docs/configuration.md`, `Cargo.toml`.
Shares `src/app.rs` with `surface-intents` (different regions; second
merger rebases) and `Cargo.toml`'s `terminal` feature list with
`control-transport` (lane B sequence: this package first).

## Planning rulings (made at /planning; veto at plan review)

1. **Layout: one file per writer-section** — `$XDG_CONFIG_HOME/<APP_ID>/`
   holds `config.ron` (the settings keys), `profiles.ron`, and
   `keybinds.ron`, one per writer (`PlannedWrite`, `ProfileWrite`,
   `KeybindsWrite`). Rationale: cosmic-config's one-file-per-key meant a
   drawer write and an external edit never collided; per-section files
   keep that property one level coarser with zero merge machinery.
   Last-writer-wins within a single file is the entire merge rule. Keys,
   defaults, and each section's serde shape are unchanged.
2. **The directory derives from `APP_ID`, not a hand-picked short name.**
   The spec proposed `$XDG_CONFIG_HOME/dropterm/`, but a short name
   breaks the dev-identity separation the store must keep (the
   `.Dev` twin gets its own config file — see `Cargo.toml`'s
   `dev-identity` contract). `src/session.rs`'s `state_file_path`
   (`:462-476`) is the pattern; the config twin follows it, so dev
   builds separate automatically.
3. **The store module owns its own helpers** (load/`save_atomic`/
   quarantine per section, lifted from the `src/session.rs:368-447`
   pattern); `src/session.rs` itself is untouched — out of scope.
4. **Watcher: the `notify` crate is the recommendation**, preserving
   today's live reload; it is the spec's ask-first dependency, so Task 4
   gates on that ask. Fallback if declined: re-read at show (external
   edits appear at the next show; the manual checklist carries the
   reload row).

## Task 1: The store module

**Description:** A plain-ron store owning load, atomic write, and
quarantine per section file, lifted from the `src/session.rs` pattern
(read that code, do not paraphrase the spec): write a `.tmp` sibling,
fsync, rename over the target, best-effort parent fsync; quarantine an
unreadable file aside to `.bak` and fall back to defaults; a missing
file is the silent first run. Generic over the section type.

**Acceptance criteria:**
- [x] Round-trip, corrupt-file quarantine, and default-on-missing tests
      pass against a temporary directory, all inline
      `#[cfg(test)] mod tests`
- [x] Tests write only to temp dirs, never real XDG paths

**Verification:**
- [x] Tests pass: `just check`

**Dependencies:** may start before `app-identity` merges; rebases onto
it (paths derive from `APP_ID`) · **Files:** `src/config/<store>.rs`
(new) · **Scope:** M

## Task 2: The config model off the derive

**Description:** Decompose the config model into the three serde
sections (settings, profiles, keybinds) with keys, defaults, and each
section's serde shape unchanged; the cosmic-config derive
(`src/config.rs:10,186`) leaves; the 40 `with_custom_path` fixtures in
`src/config/tests.rs` move onto the new store's temp-dir constructors.

**Acceptance criteria:**
- [x] `every_config_key_is_documented` stays green (adapted to
      enumerate the settings section)
- [x] All config tests pass against temporary directories

**Verification:**
- [x] Tests pass: `just check`

**Dependencies:** Task 1 · **Files:** `src/config.rs`,
`src/config/tests.rs` · **Scope:** L (fixture count, mechanical)

**Run notes (T2):** the CosmicConfigEntry derive's removal rides with
T3 — its two living consumers (init's `get_entry`, the
`config_subscription`) are T3's files; the one fixture whose subject
is `KeybindsWrite::persist` keeps cosmic-config until the writer
changes signature in T3.

## Task 3: Wire the app

**Description:** Load at startup from the new paths; the three writers
(`src/settings.rs:651,2096,2120`) persist through the store; the
completions.toml location (`src/app.rs:1993-2008`) moves to the new
directory (its Flatpak probe behavior preserved); the drawer's own
writes still apply exactly once, keeping the echo-driven apply
`ApplyTiming` relies on. The config handle (`src/app.rs:2068`) becomes
the store handle.

**Acceptance criteria:**
- [x] A drawer edit writes its section file and survives a restart
- [x] The drawer's own writes apply once (no echo loop)

**Verification:**
- [x] Tests pass: `just check`
- [ ] Manual: change a setting in the drawer; the file lands; restart
      keeps it  *(owner's row — needs an installed build)*

**Run notes (T3):** the notify watcher landed here, not in T4: a
write only persists and the watcher's echo is the apply, so the
drawer criteria above are unreachable without it. Also here: the
derive and `CONFIG_VERSION` left; `store_dir` took cosmic-config's
Flatpak host probe (else a sandboxed build's config would relocate);
the drawer's shell write was found serializing a bare String into an
Option-typed key its reader rejected — fixed, pinned by test.

**Dependencies:** Task 2 · **Files:** `src/app.rs` (its regions),
`src/settings.rs`, `docs/configuration.md` · **Scope:** L

## Task 4: External-edit reload (ask-first gate)

**Ruled (2026-11-10, owner):** `notify` sanctioned — take the sanctioned
path; the ask is resolved.

**Description:** Ask the owner about the `notify` dependency (the
spec's known candidate). If sanctioned: replace
`cosmic_config::config_subscription` (`src/app.rs:2320`) with a watcher
over the store directory; an external edit applies live, preserving the
echo-driven apply. If declined: re-read at show — an external edit
applies at the next show, and the manual checklist gains the reload
row. A drawer write re-reads its own section before serializing either
way (cheap race mitigation).

**Acceptance criteria:**
- [x] External edit applies live (headless test where the watcher
      allows; manual row otherwise)
- [x] The drawer's own writes still apply exactly once

**Verification:**
- [x] Tests pass: `just check` plus the watcher's headless test if
      sanctioned

**Run notes (T4):** T3's watcher reloaded through the quarantining
load — a non-atomic external write would have been renamed to `.bak`
mid-write. Fixed: live reloads `peek` (never touch the file) and keep
the last-known section; quarantine is startup-only. Tests proven by
mutation against T3's behavior.

**Dependencies:** Task 3 · **Files:** `src/app.rs` (`:2320` region),
`Cargo.toml` if sanctioned · **Scope:** M

## Task 5: cosmic-config leaves; docs and audit

**Description:** Drop the `cosmic-config` dependency from `Cargo.toml`;
`rg cosmic_config src/` matches nothing; the configuration doc names
the new path and files; `just check` green. Manual verification against
an installed build, then merge — remembering no release is cut between
this merge and `app-identity`'s (one break release).

**Acceptance criteria:**
- [x] `rg cosmic_config src/` empty; `just check` green; docs name the
      path (`just docs-check`)
- [ ] Manual rows on an installed build (`just build-release && sudo
      just install`): drawer write, restart survival, external edit
      *(owner's rows - six added to `docs/testing.md`)*

**Verification:**
- [x] Tests pass: `just check`

**Run notes (T5):** three commits. Nothing to drop from `Cargo.toml`:
neither manifest ever had a direct cosmic-config entry (`cargo tree`
shows it arriving through libcosmic and cosmic-theme, where it
stays). The audit found and fixed two defects from earlier tasks:
the store wrote compact one-line RON (now pretty), and a drawer write
over a missing or unreadable section fell back to the defaults,
resetting every other setting (now the live config).

**Dependencies:** Task 4 · **Files:** `Cargo.toml`,
`docs/configuration.md`, `Cargo.lock` (regenerates) · **Scope:** S

## Task 6: The dbus-config feature decision (hardware-gated)

**Description:** Whether libcosmic's `dbus-config` feature
(`Cargo.toml`'s `terminal` list) goes too. It also routes libcosmic's
own config and theme watching through cosmic-settings-daemon, so
dropping it changes how the COSMIC theme follow is watched. On COSMIC
hardware: with the feature dropped, verify the theme follow; drop it if
clean, restore and record if not. Independent of Tasks 1–5 — can land
as its own commit after the merge.

**Acceptance criteria:**
- [x] Hardware-verified decision recorded in the commit message
- [x] `just check` green either way

**Run notes (T6) — RESOLVED 2026-11-11, feature dropped.** Verified in
the COSMIC validation VM (the `PopOs24.04` domain, `vm-push` route),
on the dbus-config-off build: theme follow is live both ways — the
dark flip via the Mode entry (`is_dark`, atomic write, light→dark→light
round trip) and the accent via the active Light palette entry
(`accent`/`accent_button`, per-key atomic writes, crimson→blue) — the
terminal followed every flip with no restart (four screenshots:
vm-t6-light/dark/light2/accent-blue). Faithfulness grounded in the
locked libcosmic 08a09e3 source (`src/app/cosmic.rs`): apps watch
exactly CosmicTk, the active DARK/LIGHT_THEME_ID palette, and
THEME_MODE_ID — no Builder watch — so the file writes cover the same
entries the GUI path reaches; with the feature off those watches run
cosmic-config's file watcher, which is the fallback this check exists
to prove. Decision recorded in the drop commit. The six manual rows
(T3/T5) are still open: the walk was interrupted by concurrent work
inside the guest (the installed binary was removed mid-walk), so
rows 334–339 need a quiet VM; 335's atomic half reached "file landed
intact, no .bak" but the live-apply observation was blocked by the
same interference. Earlier headless facts, probed reversibly:
`dbus-config = []` is an empty libcosmic feature - no dependency or
lockfile change; with it removed from the `terminal` list the
terminal package builds and its 1470 tests pass, and
`cargo tree -e features` confirms it is off. It gates five cfg sites
(libcosmic `src/core.rs`, `src/app/cosmic.rs`): a startup `block_on`
for the cosmic-settings-daemon proxy, and the `watch_config`/
`watch_state` subscriptions returning the daemon's D-Bus watcher
instead of falling through to the file watcher.

**Verification:**
- [ ] Manual on COSMIC: theme follow behaves with the decided feature
      set

**Dependencies:** Task 5 merged (or beside it) · **Files:** `Cargo.toml`
· **Scope:** S

## Checkpoint: the one release

- [ ] `app-identity` + `config-store` both merged, one release carrying
      both; its notes are ordinary release notes (owner ruling
      2026-11-12: pre-distribution — no released version, no users —
      so no break note; the draft in app-identity's merge description
      is moot)
