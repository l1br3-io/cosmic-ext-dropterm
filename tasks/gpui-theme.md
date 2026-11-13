# Chunk: gpui-theme — the scheme→theme generator in-tree

Spec: [SPEC-gpui-theme.md](../SPEC-gpui-theme.md) · Module id:
`gpui-theme` · Phase 3 · branch `port/gpui-theme` (after `gpui-scaffold`;
parallel with `gpui-surface`/`gpui-terminal`).

**Files touched:** the `dropterm-gpui` theme module (the carried
`theme_gen` + its four tests) and the app-shell's apply path. Source:
`../dropterm-spike` branch `spike/gpui-layer-shell` @ `cd06740`,
`src/theme_gen.rs` — carried before any scratch-repo cleanup
([kit-root-border](../docs/research/kit-root-border.md) names this).

## Planning rulings (made at /planning; veto at plan review)

- **The carry is verbatim-plus-compile**: the generator and its four
  RED-first tests arrive as written (module path aside); any mapping change
  is a separate, justified diff — the tests are the C4 proof, not a
  starting sketch.

## Task 1: Carry the generator

**Description:** `theme_gen.rs` moves from the spike branch into
`dropterm-gpui`, adapted only to import core's resolved-scheme type
(theme glue's output). The four tests run green in `just check`.

**Acceptance criteria:**
- [ ] The four carried tests green (core mapping, name/mode pass-through,
      absent-stays-absent, parses-as-`ThemeConfig`)
- [ ] `just check` green

**Verification:**
- [ ] Tests pass: `just check`

**Dependencies:** `gpui-scaffold` merged · **Files:** the theme module ·
**Scope:** S

## Task 2: The apply path, first frame and live

**Description:** Scheme → theme JSON → `ThemeConfig` → `Theme::update(
apply_config)` before any window opens; config-pass re-resolution
regenerates on live scheme edits. Token read-back and themed-chrome
screenshots on the rig + VM (C4's evidence class).

**Acceptance criteria:**
- [ ] Tokens read back the scheme before first frame; chrome renders
      through them on both compositors
- [ ] A live scheme edit re-themes without restart

**Verification:**
- [ ] Manual: the C4-class walk on rig + VM (screenshots held)

**Dependencies:** Task 1 · **Files:** the apply path in the app shell ·
**Scope:** S
