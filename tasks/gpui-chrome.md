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

## Task 1: The context menu and pickers — done 2026-11-14

**Description:** The terminal context menu (core's model, Kit popover
presentation) and the pickers (font enumeration from core's `font_list`,
scheme swatches from the registry) rendered through the generated theme.

**Acceptance criteria:**
- [x] Every menu item fires its action through the shared path keybinds
      use
- [x] Pickers list and apply their choices; the font picker's choices
      render

**Verification:**
- [x] Tests pass: `just check`
- [x] Manual: menu/picker rows on rig + VM

**Done record:** the model halves moved to core first —
`context_menu`'s rows/labels/folder composition (nine tests ported,
the libcosmic crate a re-export front whose own half is the
frontend-typed `items()`/`action()` map; 0926b7f) and `font_list`
whole with fontdb version-matched (2271b16). The menu presents through
the Kit popover on the pane's hosting div (eb07d2a): the
clipboard/scrollback cluster lands in the pane's own
`perform_term_action` (the keybind funnel), the tab and settings verbs
in the shell calls the tab menu uses, and a reporting-mode right press
stops propagation so the child keeps it. The pickers are Kit `Select`
components (19ab35d) carrying the libcosmic fidelity — own-family font
rows, sixteen-ANSI scheme strips — with the planner trio's semantics
pinned (synthetic leader, exact match, out-of-range rejection) and two
component tests on gpui's own test harness (seeding + the
confirm-to-callback wiring; gpui's `test-support` joins as a
dev-dependency). Walked on the VM green 2026-11-14, agent-run
(`target/vm-evidence-gc1/`, build 19ab35d, binary sha256 7652e835…):
the menu opens anchored at the cursor in core's row order (each row's
measured y clicked, the matching pick line returned), every item
proved downstream — Copy's empty-selection no-op then 24 bytes after
Select All, Paste executing the pasted lines in bash, ClearScrollback
dropping history, New Tab → 3 tabs, Duplicate → 4 in the source's cwd,
Rename hosted then Escape-canceled, Settings logging the gear stub —
and the reporting-mode gate's counter-proof decoded right from the
child's X10 bytes (`^[[M` press/release at cell 76,7, zero pick
lines). The rig is n-a for all three rows with probe evidence (the
pointer class never reaches a surface there, gt-2's standing finding
re-confirmed). Scope notes: the profile folders stay honestly absent
until a GPUI profile spawn path exists (the gpui-tabs precedent; the
front composes over the empty list, core's folder logic fully pinned),
the pickers have no runtime host until gc-2's drawer (their rows ride
the gc-2/gc-3 walks), and the walk's structural note — the popover
extends past the surface's bottom edge unclipped, the C2b route
holding as for the tab menu — is recorded, not fixed here.

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
