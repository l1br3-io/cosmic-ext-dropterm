# Chunk: gpui-terminal — the terminal view on GPUI

Spec: [SPEC-gpui-terminal.md](../SPEC-gpui-terminal.md) · Module id:
`gpui-terminal` · Phase 3 · branch `port/gpui-terminal` (after
`gpui-scaffold`; parallel with `gpui-surface` — use `../ced2`; the two
integrate when both land).

**Files touched:** the `dropterm-gpui` view modules (grid view, input,
selection, clipboard wiring). Reference: Zed's `crates/terminal` (render
shape only) and the spike's check-9 harness; core's engine and
`mouse_reporter.rs` are consumed, not copied.

## Planning rulings (made at /planning; veto at plan review)

- **Damage-first rendering**: the view renders from grid damage like the
  libcosmic widget; a full-repaint-per-frame view fails the perf row even
  if it looks right.
- **The anchor stays dead**: copy rides GPUI's clipboard (the focus+serial
  gate is satisfied by construction when the terminal is focused — check
  8), and wl-clipboard-rs keeps the data-control path for
  copy-while-not-focused. Any temptation to add a hidden surface for
  clipboard is rejected at planning.

## Task 1: The grid view and resize

**Description:** Cells from the alacritty grid rendered through GPUI's
text primitives — colors/attributes off the resolved scheme, damage-driven
redraw, the block cursor — and resize: view-size → `Term::resize` →
SIGWINCH (check 9's path). A seeded PTY renders and resizes on the rig.

**Acceptance criteria:**
- [x] A live PTY renders cells, attributes, and the block cursor; the
      caret paints in the first shown frame
- [x] 8×20 → wide resize flows through `Term::resize` + SIGWINCH

**Verification:**
- [x] Manual: render + resize walk on rig + VM (screenshots held) —
      walked 2026-11-13: the VM's pixels carry the render proof
      (vm-evidence-gt1-{narrow,wide}.png — colored runs wrapped at
      8 cols in the narrow frame, magenta/yellow at 100 cols after
      the widen, the block caret live in both), both compositors'
      logs carry the resize wire (rig-evidence-gt1-walk.log +
      vm-evidence gt1-vm.log: grid (8,20) → (100,20) through
      Term::resize with the pty's own stty confirming "GRID 20
      100"); WALK FINDINGS, both made structural: (1) an unheld
      font family fails glyph rasterization SILENTLY (quads paint,
      no glyph ever lands — Noto Sans Mono is absent on the VM), so
      the host resolves through a candidate list against the font
      system's real inventory and logs the pick; (2) the sway rig
      presents the drawer's own windows (the band, pixel-proven)
      but never presents this harness's window — across anchors,
      layers, open timing, window order, redraw cadence, and root
      content (the full experiment matrix in the harness docs) — so
      the rig's evidence class for this module is logs, the VM's
      pixels, recorded rather than chased further; also pinned by
      test: alacritty 0.25's grid is TOP-DOWN (Line(0) is the top
      row, display_iter walks top-to-bottom) and the write-then-
      narrow reflow is the engine's own behavior, not the view's.

**Dependencies:** `gpui-scaffold` merged · **Files:** the view modules ·
**Scope:** L

## Task 2: Input, keybinds, selection, scrollback

**Description:** Key events through core's keybind model (chords +
`custom_key_binds`) to actions; IME and paste behavior per the libcosmic
build; pointer selection and URL handling; scrollback navigation. Pure
mapping halves (key event → action, selection model) headless-tested.

**Acceptance criteria:**
- [x] Every default chord and one custom overlay fires its action
- [x] Selection, copy-on-select where configured, and scrollback match
      the libcosmic behavior rows

**Verification:**
- [x] Tests pass: `just check`
- [x] Manual: input/selection rows on rig + VM — walked 2026-11-14,
      both compositors: every default chord fired live on the rig's
      logs (rig-evidence-gt2-pass1/-trace/-pass2.log — Copy, Paste,
      PastePrimary, SelectAll, ClearScrollback, ZoomIn/Out/Reset with
      the zoom's metric change visible as the rows re-wrap narrower in
      the log, Hide, and Shift+PageUp×2/PageDown scroll rows; the
      remaining zoom chord forms and Shift+Home/End are the same table
      rows, pinned by test), the custom overlay both ways (select_all
      rebound to Ctrl+Shift+U: the old default arrives traced and
      stays silent, the overlay chord fires), typed input end-to-end
      (wtype → compositor → keystroke → translate → PTY → echo → grid
      → logged rows, "b"→"bc"→"bcX"→"bcXY", including the
      uppercase-through-key_char rule: a capital keysym arrives
      shift-less with the char in key_char and the right byte goes
      out), and the VM carries the pixels: SelectAll's overlay covers
      the whole grid (79.6k sampled px, vm-evidence-gt2-7/-8) and
      Escape clears it, a uinput hold-drag selects through
      on_left_press/drag/release ("left press at (110,128) → cell
      (13,6)", finalized 80 bytes, the overlay band 36.6k px,
      vm-evidence-gt2-4). Copy-on-select's write is Task 3's seam by
      the chunk's own split (the placeholder logs where the write
      lands). WALK FINDINGS, all environment, recorded in the harness
      docs: (1) the rig's never-presenting harness surface DOES hold
      keyboard focus (wtype chords drive it; only pixels and pointer
      never arrive — the press trace stays empty), refining gt-1's
      rig finding; (2) cosmic-comp drops wtype's virtual-keyboard
      modifier state — chords arrive as bare keys — so the VM's
      chords ride ydotool name-sequences instead; (3) uinput wheel
      synthesis (legacy REL_WHEEL, REL_WHEEL_HI_RES, and
      INPUT_PROP_WHEEL) never becomes a scroll event for ANY app on
      the VM — cross-checked against cosmic-term itself — so the
      wheel-with-a-physical-mouse row rides parity's human-at-console
      class: the platform's wheel path is verified in gpui-pre-linux
      source (AxisDiscrete/Value120 → Frame flush → ScrollEvent to
      the pointer-focused window), on_wheel is wired to it, and the
      keyboard scrollback rows drive the same engine `scroll()` call.

**Dependencies:** Task 1 · **Files:** input/selection modules ·
**Scope:** L

## Task 3: Mouse reporting and clipboard

**Description:** GPUI pointer events driving core's `mouse_reporter`
protocol logic (the app's mouse-reporting modes); copy/paste through
GPUI's clipboard on the focused surface; wl-clipboard-rs's data-control
path wired for copy-while-not-focused (today's behavior).

**Acceptance criteria:**
- [x] Mouse-reporting apps (`htop`-class) receive events per mode
- [x] Copy lands after a real keystroke on the focused terminal
      (focus+serial); paste works; a copy made while unfocused still
      serves

**Verification:**
- [x] Manual: the check-8-class walk (`wl-paste` marker) + mouse rows on
      rig + VM — walked 2026-11-14 on the VM (the rig's pointer never
      reaches the surface, gt-2's finding, so the mouse rows are the
      VM's): the protocol logic moved to core framework-free
      (`dropterm-core/src/terminal/mouse_reporter.rs`, 6 tests pinning
      the X10/UTF-8 and SGR encodings — press, release-as-3, held
      motion +32 with cell dedup, the ≥95 UTF-8 coordinate extension,
      modifier folds, wheel 64/65 + modifier flags, pixel/line
      accumulation) with the libcosmic frontend's `input.rs` now a
      thin iced→neutral adapter (its public surface and 702 tests
      unchanged) and the pane consuming core directly; the walk
      drives a `cat -v` program under DECSET 1000+1006 — the pane's
      own bytes echoed back: the full SGR triple
      `^[[<0;14;7M` press, `^[[<32;18;7M`…`^[[<32;86;15M` held
      motion, `^[[<0;86;15m` release (vm-evidence-gt3-1), and the
      legacy run without 1006 shows `^[[M@…` motion ending
      `^[[M#v/` — release as button 3 (vm-evidence-gt3-3); the
      clipboard rows: after a real keystroke, SelectAll+Copy took
      323 bytes through GPUI's clipboard and Paste read all 323 back
      through the engine's paste (bracketed-paste aware), an
      external `wl-copy EXTERNAL-PASTE-MARKER` landed in the grid
      through the Paste chord, and a timed OSC 52 store — the
      copy-while-unfocused class, riding the new data-control seam —
      served an offer `wl-paste` read as OSC52-UNFOCUSED-MARKER
      while the pane lived; the wheel-under-mouse-mode row rides
      gt-2's parked physical-wheel class (the encoder half is the
      unit-tested `sgr_mouse_wheel_scroll`, the driver is wired);
      walk-procedure note recorded in the harness docs: a
      data-control offer lives only as long as the serving process —
      the marker check must run while the pane lives.

**Dependencies:** Task 2 · **Files:** mouse/clipboard modules ·
**Scope:** M

## Task 4: Wire into the surface executor's windows; merge

**Description:** The view replaces the scaffold's placeholder inside the
GPUI executor's windows (this task is why the slice lands after
`gpui-surface` or coordinates with it); the drawer shows a live terminal
end to end. Terminal rows of the manual checklist walked; merge.

**Acceptance criteria:**
- [x] The drawer opens into a focused, typed-in terminal on both
      compositors
- [x] `just check` green on the merge result

**Verification:**
- [x] Manual: terminal rows, both compositors — walked 2026-11-14:
      the scaffold's clock band is gone, the pane is the drawer's
      whole content under the same slide translation, built once at
      boot through the one shared construction path (`boot_pane` —
      the walk harness and the shell no longer duplicate a spawn),
      the pane's Hide chord rides a channel the shell's tick drains
      into the machine's hide funnel (window work never runs inside
      a pane's event handler — sr-1's lesson), and
      `Action::FocusTerminal` applies as the pane standing focused
      (its handle grabs the keyboard on first render inside the
      surface — the row is the decision landing). THE RIG
      (rig-evidence-gt4-drawer.log): the whole walk inside one
      `dbus-run-session` — the rig has no session bus, so a second
      launch would become a second instance; the wrapper gives the
      forwarded toggle a shared bus — and the drawer cycle ran:
      forwarded toggle → Measure/Show/Focus (first frame 54ms),
      the pane's rows rendering in the drawer (147-col grid across
      the full-width surface), typing advancing the caret, a second
      full cycle on the re-toggle; RIG FINDING, environment: sway
      drops the drawer's keyboard activation the moment wtype
      creates its per-invocation virtual keyboard, which ran the
      focus-loss funnel (Hold → Destroy) correctly mid-walk — the
      hide chord row itself moved to the VM where cosmic-comp
      keeps activation. THE VM
      (vm-evidence-gt4-{1,2,3,4}.png + -drawer.log): the pixel
      proofs — the drawer band appears (102k sampled px vs the
      hidden frame), the typed line renders inside it (VM-DRAWER-
      TYPED echoed through 15 row updates), the band leaves on the
      hide (Ctrl+Shift+Q → "pane hide action -> the hide funnel" →
      Hold → Destroy at +15768ms), and the full cycle repeats
      (re-show first frame 39ms, AFTER-RESHOW typed); SelectAll +
      Copy took 32 bytes through the focused drawer's GPUI
      clipboard (the offer-lifetime note applies there too — the
      marker read must run while the pane lives).
- [x] Tests pass: `just check` — green on the branch; the merge's
      own gate runs on main.

**Dependencies:** Tasks 1–3, and `gpui-surface` merged · **Files:** the
app-shell wiring · **Scope:** S

## Checkpoint: a working terminal in the GPUI drawer

- [x] Type, select, scroll, resize, copy — the drawer is a terminal on
      both compositors — 2026-11-14: Tasks 1–4 walked and merged; the
      evidence index lives in the tasks above and under
      `target/{rig,vm}-evidence-gt*`.
