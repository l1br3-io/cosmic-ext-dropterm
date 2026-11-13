# Chunk: gpui-chrome — settings drawer, context menu, pickers on the Kit

Spec: [SPEC-gpui-chrome.md](../SPEC-gpui-chrome.md) · Module id:
`gpui-chrome` · Phase 3 · branch `port/gpui-chrome` (after `gpui-tabs`,
`gpui-status`, `gpui-theme`).

**Files touched:** the `dropterm-gpui` chrome modules (drawer, context
menu, pickers, drawer handle). Consumed from core: config sections and
writers, `context_menu.rs` (pure model), `font_list`. Read-only reference:
`src/settings.rs`, `src/context_menu.rs`, `src/drawer_handle.rs`.

## Planning rulings (made at /planning; veto at plan review)

- **The drawer's control→key map is ported, not redesigned**: the page
  set, control set, and write contract are the libcosmic drawer's; a
  contributor-facing visual change is out of scope for parity.
- **Menus ride the C2b-proven overlay path** (Kit popover components on
  the layer surface through Root) — the kill check already covers this
  path; the drawer's opening/closing through the 80 ms cadence gets one
  explicit row so the popover-close regression the Kit pass guarded
  against stays guarded.

## Task 1: The context menu and pickers

**Description:** The terminal context menu (core's model, Kit popover
presentation) and the pickers (font enumeration from core's `font_list`,
scheme swatches from the registry) rendered through the generated theme.

**Acceptance criteria:**
- [ ] Every menu item fires its action through the shared path keybinds
      use
- [ ] Pickers list and apply their choices; the font picker's choices
      render

**Verification:**
- [ ] Tests pass: `just check`
- [ ] Manual: menu/picker rows on rig + VM

**Dependencies:** `gpui-tabs`/`gpui-status`/`gpui-theme` merged ·
**Files:** the chrome modules · **Scope:** M

## Task 2: The settings drawer

**Description:** The five pages, profile manager form, keybind editor
with capture, and the drawer handle's resize — write-through per commit
point, mapped to config keys exactly as the libcosmic drawer
(`every_config_key_is_documented` must stay green; the control map tests
port).

**Acceptance criteria:**
- [ ] Every drawer control writes its key per the write contract; no
      Apply button exists
- [ ] The config↔docs pin test green

**Verification:**
- [ ] Tests pass: `just check`
- [ ] Manual: settings rows on rig + VM (drawer edit ↔ hand edit = one
      stream)

**Dependencies:** Task 1 · **Files:** the drawer modules · **Scope:** L

## Task 3: Cadence row, checklist rows, merge

**Description:** One explicit popover-through-cadence row for the drawer
(the Kit's close regression guard); the settings/context-menu/picker rows
walked on COSMIC + sway; merge.

**Acceptance criteria:**
- [ ] The cadence row passes (drawer open + popover through the 80 ms
      cadence, never closing)
- [ ] Chrome rows pass on both compositors; `just check` green on the
      merge result

**Verification:**
- [ ] Manual: chrome rows, both compositors

**Dependencies:** Task 2 · **Files:** the branch's diff · **Scope:** S
