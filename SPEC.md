# Spec: In-app dismiss ("Now - priority" in ROADMAP.md)

## Objective

On a fresh run DropTerm opens a layer surface that cannot be dismissed from
inside it: hiding only works via a second binary launch (toggle) or a COSMIC
custom shortcut, and `hide_on_focus_loss` defaults to `false`. Fix the
first-run dead end so that `cargo run` always leaves the user with a working
way to hide the overlay:

1. An in-app dismiss keybinding.
2. Click-away-to-hide out of the box (Yakuake/Guake behavior).
3. README coverage of the run-locally-without-a-shortcut flow.

Success means: fresh config, `cargo run` → surface opens → user can hide it
with the keyboard and/or by clicking away, and the session survives either
way.

## Assumptions

- COSMIC only (existing non-goal); no X11, no other compositors.
- Config format is per-key RON files via cosmic-config (`CONFIG_VERSION = 1`).
  No version bump: the default flip only affects configs created after the
  change; existing files keep their stored value, which is correct (no
  migration needed).
- The keybinding is fixed, not user-configurable, like every other keybind in
  v1 (no shortcuts settings page yet).

## Design decisions

- **Dismiss key is `Ctrl+Shift+Q` only.** Bare `Escape` was considered and
  rejected: Escape currently falls through to the shell, and TUI apps
  (vim, less, emacs, readline vi-mode) depend on that. Hiding on bare Escape
  would close the whole overlay while editing in vim. `Ctrl+Shift+Q` does not
  collide with any shell/TUI convention. Escape behavior is unchanged:
  cancels an active selection, otherwise reaches the shell.
- **`hide_on_focus_loss` flips to `true` in `Default::default()`.** Matches
  Yakuake/Guake. Users who explicitly wrote `false` are unaffected (config
  files win over defaults).
- **Focus latch guard.** Hide-on-focus-loss must only fire after the surface
  has actually received focus at least once since it was shown. Without this,
  a stray `Unfocused` delivered before any `Focused` (possible with
  `keyboard_mode: OnDemand`, where the compositor may never grant focus)
  would make the surface open-then-instantly-close, which is exactly the
  first-run dead end we're fixing.

## Implementation

### 1. `Action::Hide` + keybind

**`src/terminal/widget.rs`**
- Add `Hide` to the `Action` enum. The widget only uses `Action` values to
  decide which key presses to swallow (via the `key_binds` map passed into
  `TerminalBox`); actual behavior lives in `App::perform_action`. No other
  widget change needed - both swallow sites iterate `key_binds.keys()`.

**`src/keybinds.rs`**
- `default_key_binds()`: insert
  `bind(&[Modifier::Ctrl, Modifier::Shift], "q") → Action::Hide`.
  `KeyBind::matches` already falls back to the physical key for non-Latin
  layouts, same as the existing binds.

**`src/app.rs`**
- `perform_action()`: handle `Action::Hide` **before** the
  `let Some(terminal) = &self.terminal else { ... }` guard - hiding must work
  even when no terminal exists - by returning `self.hide()`.
- Add the `Action::Hide` match arm (match becomes exhaustive again).

Notes:
- `hide()` is idempotent (`SurfaceState::Hidden` → `Task::none()`), so
  key-repeat on `Ctrl+Shift+Q` is harmless and needs no debounce.
- `Action` is `Copy` and used as a `HashMap` value; adding a variant changes
  nothing else.

### 2. Default flip + focus latch

**`src/config.rs`**
- `DropTermConfig::default()`: `hide_on_focus_loss: true`.
- Update the field doc comment ("defaults to `true`").
- `defaults_are_sane` test: `assert!(config.hide_on_focus_loss)`.

**`src/app.rs`**
- Add a `surface_focused: bool` field to `DropTerm`:
  - set `false` in `show()` (every new surface starts unfocused),
  - set `true` in the `Message::LayerFocused` and `Message::WindowFocused`
    handlers,
  - in `Message::WindowUnfocused`: hide only if `self.surface_focused`
    (then reset to `false`).

### 3. README

**`README.md`**
- New subsection under "Setting up the shortcut": running locally without a
  COSMIC shortcut - `cargo run` / `just run` opens the overlay;
  `Ctrl+Shift+Q` hides it; clicking away hides it too (default
  `hide_on_focus_loss: true`); a second launch still toggles.
- "Features (v1)" list: add the `Ctrl+Shift+Q` dismiss shortcut.
- Configuration section: note `hide_on_focus_loss` now defaults to `true`.
- Manual test checklist: add
  - `Ctrl+Shift+Q` hides the surface, session survives re-toggle,
  - clicking another window hides the surface (fresh config),
  - with `hide_on_focus_loss: false` written, clicking away does *not* hide,
  - Escape still reaches the shell (e.g. works in vim) and still cancels an
    active selection.

## Testing strategy

Automated (unit only - runtime behavior needs real COSMIC hardware):
- `keybinds.rs` tests: `Ctrl+Shift+Q` → `Some(Action::Hide)`; bare Escape →
  `None` (regression guard for the decision above).
- `config.rs` tests: updated `defaults_are_sane` asserts the new default.

Manual checklist additions (README) exercised on COSMIC hardware after
building. `just check` (fmt + clippy `-D warnings` + tests) must pass before
any commit.

## Success criteria

- [ ] Fresh config dir, `cargo run`: surface opens; `Ctrl+Shift+Q` destroys it; a second launch re-shows the same shell session.
- [ ] Fresh config: clicking another window hides the overlay.
- [ ] With `hide_on_focus_loss: false` in config: clicking away does not hide.
- [ ] Escape with no selection still sends `\x1B` to the shell; Escape with a selection still cancels the selection and does not hide.
- [ ] `Ctrl+Shift+Q` while hidden does nothing (no crash, no surface).
- [ ] `just check` green; README documents the fresh-run flow.

## Boundaries

- Always: run `just check` before committing; update the README checklist in
  the same change as the code; keep Escape falling through to the shell.
- Ask first: changing the dismiss keybinding, adding new config keys, bumping
  `CONFIG_VERSION`, touching anything outside the three roadmap items.
- Never: hijack bare Escape, require a live terminal to hide the surface,
  change stored user config values.

## Task breakdown (in order)

1. `Action::Hide` + keybind + `perform_action` handling (widget.rs, keybinds.rs, app.rs) with keybind tests.
2. Default flip + focus latch (config.rs, app.rs) with config test update.
3. README: fresh-run docs, features list, config note, checklist additions.
4. `just check` + manual verification on COSMIC hardware against the success criteria; tick the three "Now" boxes in ROADMAP.md.

## Open questions

- None. (Escape semantics and spec location were resolved before writing this spec.)
