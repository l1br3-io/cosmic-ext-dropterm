# Chunk: gpui-parity — the remaining features and the parity proof

Spec: [SPEC-gpui-parity.md](../SPEC-gpui-parity.md) · Module id:
`gpui-parity` · Phase 3 · branch `port/gpui-parity` (after every UI
slice; hardware-heavy).

**Files touched:** the `dropterm-gpui` suggest view, image view, bell
wiring, and session-restore wiring; core only if an event seam is missing
(rulings below). The checklist walks produce evidence under the lane's
`target/vm-evidence-gpui/` and rig logs.

## Planning rulings (made at /planning; veto at plan review)

- **Feature order: suggest → images → bell → session restore**, each a
  complete path (model in core, view here, rows walked) before the next —
  the parity walk is its own task, after all four.
- **The walk is the deliverable**: both compositors, the whole checklist,
  row-named evidence. A row that cannot pass stops the task and files an
  owner ruling; it is never marked with a shrug.
- **The dock-mode rows carry the known open question** (plan open
  questions item 9): `show_in_dock` needs a persistent-toplevel answer on
  GPUI or an owner-accepted degradation. The ruling lands before this
  slice's walk reaches those rows.
- **The walks no longer gate building** (owner ruling, 2026-11-15):
  every owner-run walk parked below waits for the owner and never
  blocks the feature work or cutover's code swap; only the release
  (`gpui-cutover` Task 3) keeps its own owner gate.

## Task 1: The suggest overlay and menu

**Description:** Core's suggest model and completions data (embedded +
extended tiers) drive the GPUI overlay: trigger, navigate, accept, the
menu's rows. View-model tests headless; rows on the rig + VM.

**Acceptance criteria:**
- [x] Suggest triggers, navigates, and accepts per the libcosmic rows;
      the embedded floor works on a fontless base

**Verification:**
- [x] Tests pass: `just check`
- [x] Manual: autosuggest rows of the checklist *(owner-run
      2026-11-15: reported done, no deviations filed)*

**Dependencies:** all UI slices merged · **Files:** the suggest module ·
**Scope:** M

## Task 2: Inline images

**Description:** OSC 1337/iTerm images: core decodes (strict limits,
four formats); the view renders, places, and scrolls them, including the
paste-image path.

**Acceptance criteria:**
- [x] Images render in-grid and survive scrollback navigation per the
      images rows

**Verification:**
- [x] Manual: images rows of the checklist *(owner-run 2026-11-15:
      reported done, no deviations filed)*

**Dependencies:** Task 1 · **Files:** the image view · **Scope:** M

## Task 3: The bell and session restore

**Description:** notify-rust notification firing from the GPUI build's
engine events; session capture/restore wired to the GPUI lifecycle
(restore on launch, quarantine on corrupt files, the `--is-visible`
semantics unchanged).

**Acceptance criteria:**
- [x] Bell rows pass; session restore restores tabs, order, active, and
      working directories per the sessions rows

**Verification:**
- [x] Manual: bell + sessions rows of the checklist *(owner-run
      2026-11-15: reported done, no deviations filed)*

**Dependencies:** Task 2 · **Files:** the bell/restore wiring ·
**Scope:** S

## Task 4: The parity walk — the full checklist, both compositors

**Description:** The whole manual checklist (337 rows) walked against a
release `dropterm-gpui` on COSMIC (validation VM, snapshot discipline)
and sway (rig), evidence per row class as the checklist's own structure
directs. Deviations filed; the dock-mode ruling applied per item 9.

**Acceptance criteria:**
- [ ] Every row passes on both compositors or carries a filed,
      owner-accepted deviation
- [ ] The walk's evidence lands (logs/screenshots, row-named)

**Verification:**
- [ ] Manual: the checklist, both compositors — this task *is* the
      verification

**Dependencies:** Task 3, open question item 9 ruled · **Files:** evidence
artifacts; core fixes if rows demand them · **Scope:** L

**Parked (owner ruling, 2026-11-15):** the owner-run walks no longer
gate building — py-4 waits here until the owner runs it (un-park by
running the walk), while the feature work the walk would have caught
keeps landing (py-5, py-6) and cutover's code swap proceeds under the
same ruling. The walk, when it runs, walks the then-current build with
every feature that landed by then.

## Task 5: The tab-verb and profile spawn surface

**Description:** The parity gap three chunks deferred to "a spawn
path": the pure spawn resolution moved to core (one implementation,
both frontends — `SpawnCommand`, the command-then-shell wrap, the
degradations, the cwd compositions), and the GPUI side consuming it —
profile launches with env, cwd, and per-tab appearance (scheme, font,
scrollback at spawn and in the live apply pass), `default_profile`
owning plain spawns, `--cmd` ephemeral tabs, the remote tab verbs the
GPUI CLI/dbus never served (`new-tab` with `--cwd`/`--cmd`/`--profile`,
`close-tab`, `select-tab`, `rename-tab`, `duplicate-tab`,
`switch-profile-tab` — CLI subcommands, forward words, dbus decode,
the shell funnel, the TabCount mirror), first-launch `new-tab`, the
context menu's two profile folders, the `+` right-click dropdown, the
per-tab menu's Switch Profile section through `plan_respawn`, the
session saving profile ids, and the band's profile seat fed.

**Acceptance criteria:**
- [x] Every profile/tab-verb row of the checklist passes headless or
      carries the code path that walks it (the rows themselves are
      py-4's, parked with it)

**Verification:**
- [x] Tests pass: `just check` *(green 2026-11-15 across the four
      landing commits: 986e765 the core spawn move, 113566b the pane
      + funnel, 847b99c the remote verbs, 2028c7f the menus)*
- [ ] Manual: profile + remote rows *(parked with py-4, owner ruling
      2026-11-15)*

**Dependencies:** Task 3 · **Files:** core `spawn` module, the GPUI
CLI/dbus/app-shell/pane spawn funnel, menus, band feed · **Scope:** L

## Task 6: The GPUI images panel

**Description:** The pasted-images panel on GPUI (the libcosmic panel's
port): the `▦ N images` chip's click verb opening it, the thumbnail
row, delete with the labeled confirm pair (`Confirm delete` / `Keep` —
the 2026-11-15 scope ruling, recorded in the walk note below), over
core's paste cache and ledger.

**Acceptance criteria:**
- [x] The panel opens from the chip, lists the ledger, deletes with
      the labeled pair; the paste-image rows carry it

**Verification:**
- [x] Tests pass: `just check` *(green 2026-11-15, b33cf05)*
- [ ] Manual: images-panel rows *(parked with py-4, owner ruling
      2026-11-15)*

**Dependencies:** Task 2 · **Files:** the GPUI panel module, the band
chip's verb · **Scope:** M

## Checkpoint: parity

- [ ] Phase 3's exit line holds: parity with the libcosmic build on COSMIC
      and sway, evidenced row by row *(parked with py-4, owner ruling
      2026-11-15 — the walk that proves it is the owner's to run)*

## Walk note: the UX-audit parity divergences (2026-11-15, owner-approved scope)

The UX-audit fix pass ([tasks/ux-audit-fixes.md](ux-audit-fixes.md)) lands
its fixes in core + the GPUI frontend only, by scope ruling. Two deliberate
divergences stand until cutover deletes the libcosmic frontend:

- **Images panel, delete-confirm pair.** The libcosmic panel's Confirm/Keep
  pair is the ✓/✕ icon pair with tooltips (the row-width reason recorded in
  [docs/images.md](../docs/images.md)); the drawer's profile delete shows
  the labeled shape (`Confirm delete` / `Keep`). Ruling: **no code change
  on the libcosmic side** — when the GPUI images panel is built (its
  chrome slice), its confirm pair ships labeled, matching the drawer's
  shape. The parity walk's images-panel rows read against the libcosmic
  pair until then.
- **Paste-refusal verdict chip.** A refused image paste narrates a
  transient verdict chip in the GPUI band (3 s, attention red, the
  pasted-images seat — or the clock's); the libcosmic band keeps the
  log-only behavior. The checklist's paste rows walk the verdict on GPUI
  and the silence on libcosmic.

The `scroll_to_bottom` keybind is **not** a divergence: it landed in the
shared core registry and both frontends' funnels.
