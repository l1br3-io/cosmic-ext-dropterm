# Chunk: gpui-status — the status band on GPUI

Spec: [SPEC-gpui-status.md](../SPEC-gpui-status.md) · Module id:
`gpui-status` · Phase 3 · branch `port/gpui-status` (after
`gpui-terminal`; parallel with `gpui-tabs`).

**Files touched:** the `dropterm-gpui` status-band module. Consumed from
core: the segment model and events, jiff clock arithmetic. Read-only
reference: `src/status_bar.rs` (the libcosmic behavior set).

## Planning rulings (made at /planning; veto at plan review)

- **OSC stays engine-side**: the band renders core events; if a segment
  needs a new event core doesn't emit, the event lands in core first with
  its test — the view never parses escape sequences.

## Task 1: Band layout, clock, and base segments

**Description:** The band's layout and truncation rules, the clock's
minute-boundary tick, and the base segments (working directory, profile,
custom) from core's model. Headless tests pin segment state →
presentation.

**Acceptance criteria:**
- [x] Band renders per config (show/hide, segments) on both compositors;
      clock ticks at the minute boundary
      *(walked 2026-11-14 — rig target/rig-evidence-gn1/ runs a-e, VM
      target/vm-evidence-gn1/ runs a-c: run A holds every base seat —
      the filled disc with `sleep`'s comm (an interactive bash's
      job-control child; the exec-class seed's hollow disc beside it
      is the engine's own pgrp rule reading idle, the honest
      fallback), the `~`-collapsed path, its git branch, the tab
      position, the clock — and two shots straddling a minute
      boundary: the log carries the flip at +25 864 ms (boot
      18:39:34 → 18:40:00.2, the boundary itself) with 631 px
      differing exactly at the clock's corner; run B
      (`status_bar_enabled: false`) drops the band whole — the
      1-tab strip's corner affordances alone at the free edge; run C
      (`clock/path/branch: false`) takes the named seats and only
      them, the grid honestly absent at its spawn default; the VM's
      run A pixels carry the same set — • sleep · ⌂ ~/gn-walk ·
      vm-main · ▤ 2/2 · ◷ 17:33 beside the logged arm — with B/C
      gated silent in the logs)*

**Verification:**
- [x] Tests pass: `just check`
- [x] Manual: band render walk on rig + VM

**Dependencies:** `gpui-terminal` merged · **Files:** the status-band
module · **Scope:** M

## Task 2: OSC-fed segments and config rules

**Description:** OSC-fed content (working directory via OSC 7-class,
command exit codes via the 133;D forks) rendered as it arrives; the
config-driven content rules; click actions where the libcosmic band has
them.

**Acceptance criteria:**
- [x] Exit-code segments surface per the libcosmic behavior; OSC updates
      land live
      *(walked 2026-11-14 — rig target/rig-evidence-gn2/ and VM
      target/vm-evidence-gn2/, one seeded run each holding every
      OSC-fed seat at once: the OSC 7 report winning the path seat
      (`~/gn-osc` rendered, not the /proc cwd's `~/gn-walk`) with the
      local-report branch rule standing (`gn-rig`/`vm-main`), the
      133;D;3 verdict's `✗3` in the scheme's red, the 9;4 progress
      `◐ 42%`, and the 7501 blocked record tinting the hollow disc
      attention with `40%` riding the name seat — the pane already
      forwarded every OSC event into the engine, so the slice's work
      was the gathering: each in-memory read under its own key's
      gate beside the reference's own split, the decay landed at the
      tick, the ssh stand-in's read gated on its three keys, the
      frozen pair's name kept per-tab, and the missed-activity latch
      armed at the poll (active + hidden output) with both clears —
      switch and show; clicks: the copy verb (GPUI clipboard + the
      ✓ flash, two more VM pins) and the scrolled chip's jump
      through the engine's own scroll-to-bottom; the click and
      duration-glyph ROWS ride gn-3's walk)*

**Verification:**
- [x] Manual: status-band rows of the checklist on rig + VM
      *(the OSC-fed rows above; the interactive copy/jump rows and
      the full checklist walk are gn-3's)*

**Dependencies:** Task 1 · **Files:** the status-band module · **Scope:**
M

## Task 4: The interaction families (follow-on; the merge's scope notes)

**Description:** The families the merge's scope notes deferred — the
band's context menu, the drag-selection WYSIWYG pair with its
double-click and middle-click idioms, the per-segment hover tint, the
elided path's inline reveal, and the hover tips — ported from the
reference band's walked semantics (`git show
55ba623:src/status_bar.rs`, the cutover's parent) onto the GPUI
band. The menu rides a shell-owned Kit `PopupMenu` (the owner's
ruling, 2026-11-18): the shell holds the entity, tracks
`DismissEvent`, closes it on hide/show, and suppresses through-presses
while it stands — the checklist's press-dismissal row demands nothing
reach the terminal under the bubble, and the Kit ext's outside-press
dismissal captures nothing. Tips ride the same pass (owner ruling,
same day). The pure halves land in core framework-free: the selection
math (4 px click discrimination, span rects, the per-char WYSIWYG
assembly joined by single spaces), the reveal arming, the menu rows
with their live enablement, and the laid-band VM the selection and
tips read.

**Acceptance criteria:**
- [ ] Every segment click-copies its full value with the `✓` flash;
      drag-select is WYSIWYG; double-click selects the display;
      middle-click pastes; hover tints one segment
- [ ] Squeeze inline reveal on hover; the hover tip bubbles name each
      segment's verb and vanish with the cursor
- [ ] Context menu: right composition, full-value copies, keybind-
      equivalent actions, press-dismissal, closed on hide/show
- [ ] The implemented rows' audit: the border seam holds one hairline
      through the four stackings; the walked rows stay untouched

      *(the owner's walk began 2026-11-18 on the new `just dev`
      hardware loop — the first look at the merged band — and found
      three failures before any row could pass: the band's rows
      painted at gpui's 20 px default text size over the 12 px laid
      rects (segments touching and overlapping; fixed the same day —
      the row divs now carry `text_size(SEGMENT_FONT_SIZE)` the tab
      strip's own way); and the keyboard family, diagnosed to two
      findings the same day from the traced run — the real-hardware
      key_char-less shape (`key_char: None` on every modified press,
      a shape the rig's synthesized keys never produce) swallowed
      every Ctrl/Alt byte-press in the input table's character gate,
      fixed same-day by deriving the control code from the printed
      key, and the tab chords (Ctrl+Shift+T & family) never bound on
      this frontend at all — the pane's map owns fifteen chords and
      the tab family stays the shell's to claim, [filed in
      known-issues](../docs/known-issues.md); the caret's verdict
      still rides the owner's eyes — every upstream state and paint
      step traces green)*

**Verification:**
- [ ] `just check` green with the new core pins
- [ ] Manual: rows 49–56 plus the tips/suppression full rows, owner-
      run walk

**Dependencies:** Task 3 (merged) · **Files:** core status model, the
status-band view, the shell's band machine · **Scope:** L

## Task 3: Rows and merge

**Description:** The status-band rows walked on COSMIC + sway; merge.

**Acceptance criteria:**
- [x] Band rows pass on both compositors
      *(walked 2026-11-14 — VM target/vm-evidence-gn3/ (the
      interaction rows, uinput-driven), rig target/rig-evidence-gn3/
      (the timer rows beside gn-1/gn-2's pixels): the copy verb
      end-to-end — the path's click logged (`status copy Path (20
      bytes)`), the payload served by an external `wl-paste`
      (`~/gn-osc-destination`, the full un-elided value), the ✓
      flash re-laying the band in its window and the value
      returning after; the duration rows — `live=true` at the
      flood's first witness, the frozen pair `("yes", 4.11s)` at
      the kill's finish edge, the chip reading `yes 0:04` in the
      pixels; and the walk's structural finding, fixed with it: the
      pane's own needs_update flag was consumed by nothing while
      paint gated on the engine's — the observation pass (the
      foreground witness, the timer, autosuggest) dead past frame
      one, its own commit; the scrolled chip's rows ride the
      platform's wheel class (gt-2's finding 3: uinput wheel
      synthesis reaches no app on the VM, the rig's pointer never
      reaches the surface) — the chip and its jump verb are wired
      and headless-pinned, their live row belongs to parity's
      human-at-console class)*
- [x] `just check` green on the merge result
      *(35ea4cf, green on main)*

**Verification:**
- [x] Manual: band rows, both compositors

**Dependencies:** Task 2 · **Files:** the branch's diff · **Scope:** S
