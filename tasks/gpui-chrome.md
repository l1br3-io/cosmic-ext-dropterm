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

## Task 2: The settings drawer — done 2026-11-14

**Description:** The five pages, profile manager form, keybind editor
with capture, and the drawer handle's resize — write-through per commit
point, mapped to config keys exactly as the libcosmic drawer
(`every_config_key_is_documented` must stay green; the control map tests
port).

**Acceptance criteria:**
- [x] Every drawer control writes its key per the write contract; no
      Apply button exists
- [x] The config↔docs pin test green

**Verification:**
- [x] Tests pass: `just check`
- [x] Manual: settings rows on rig + VM (drawer edit ↔ hand edit = one
      stream)

**Done record:** landed as five increments over the moved model — the
whole framework-free half of the libcosmic drawer re-homed to
dropterm-core with its 162 tests (the write contract, `ConfigKey` and
the section→key map, `PlannedWrite`'s clamped/persist, the option
tables and shell enumeration, the profile manager's pure half, the
keybinds write, the picker planners; the libcosmic crate a re-export
front, its own halves the view builders and the draft-lock gate;
56f4ca7); the drawer itself over the live terminal with one
`SettingsState` and the one write path every commit rides — clamp,
persist through `PlannedWrite`, the watcher's echo driving every apply
so drawer writes and hand edits stay one event stream, an echo-refresh
pass re-seeding un-drafted controls after every reload (01eaf6e); the
Profiles page (the transactional form through `plan_profile_save`, the
delete/reorder clusters, the default cascade; 40879dc); the Keybindings
page over a registry that became core's, pinned chord-for-chord against
the libcosmic firing tables, the capture gate's verdicts committing one
whole-map write (acc9dd1); and the drawer's own hosting — off the Kit's
fixed-width Sheet onto a shell-rendered overlay holding every pointer
event over the live band (the libcosmic context_drawer capture rule)
with the resize handle writing `SettingsDrawerWidth` once per release
and a double-click resetting it (bfaea85). Walked on the VM 2026-11-14
(`vm-evidence-gc2/` then `vm-evidence-gc2b/` after the fix): the first
walk caught the drawer's constructor reading the shell inside its own
update — the panic verbatim three times from both settings entry
points — fixed in its own commit (the build a pure read pass outside
any update, the menu's Settings pick routed before the update begins;
20631e7); the re-walk green across drawer open/close by the gear, the
menu's Settings row, and repeat-press, the Behavior and Advanced pages,
the wheel rows (pane, drawer page, picker popover, over the live band),
the font picker through its popover, the keybind capture, and the
slider's buffered release, with the seeded panes' rows holding under
the open drawer (the surface stays live beneath) and the watcher armed
on the walk's own config dir (the one-stream rule's witness). Owner
ruling 2026-11-15: this walk stands as the task's manual verification
(it ran minutes before the walks-go-manual ruling landed); no separate
rig walk ran — the remaining chrome verification rides gc-3's
both-compositor rows. `just check` green on the result (2026-11-15),
the config↔docs pin among it. Scope carried to gc-3: the scheme
picker's runtime row and the popover-through-cadence row.

**Dependencies:** Task 1 · **Files:** the drawer modules · **Scope:** L

## Task 3: Cadence row, checklist rows, merge — done 2026-11-15

**Description:** One explicit popover-through-cadence row for the drawer
(the Kit's close regression guard); the settings/context-menu/picker rows
walked on COSMIC + sway; merge.

**Acceptance criteria:**
- [x] The cadence row passes (drawer open + popover through the 80 ms
      cadence, never closing)
- [x] Chrome rows pass on both compositors; `just check` green on the
      merge result

**Verification:**
- [x] Manual: chrome rows, both compositors

**Done record:** walked owner-run 2026-11-15 — the first module walk
under the 2026-11-14 manual ruling, the owner at the console for every
row. The handed-over instruction named the eight clusters: the cadence
row (the drawer held open with a picker popover across the 80 ms
reshape cadence, the popover staying and the drawer never closing), the
drawer surface/strip rows, the write contract with per-write config
dumps (drawer edit ↔ hand edit one stream), the keybind editor rows,
the scheme and font pickers (gc-2's carried rows), the context-menu
cluster with the profile folders' honestly-flat zero-profile shape, the
profile-manager rows minus the two already-green env rows, and the gear
cluster — walked on COSMIC then sway. Owner verdict: all clusters green
on both compositors, no deviations filed. No evidence artifacts were
placed under target/ — the owner's reported result is the record, per
the ruling. Merged and closed with `just check` green on the merge
result (see the SPEC status line for the sha).

**Dependencies:** Task 2 · **Files:** the branch's diff · **Scope:** S
