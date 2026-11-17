# Spec: UX-audit fix pass (2026-11-15)

Four fixes from the desktop-UI audit (settings drawer, status band,
images panel). Scope ruling by the owner 2026-11-15: all four, core +
GPUI frontend; the libcosmic frontend keeps current behavior wherever
the fix would double drawer/panel work — the divergence is recorded in
`tasks/gpui-parity.md` and dies at cutover.

## Objective

Close the audit's four actionable findings:

1. **F7 (P1) — paste refusals fail silently.** An image paste over the
   32 MiB cap (or an unreadable image offer) pastes nothing and the
   only trace is a log warn neither frontend shows. The user's paste
   action must produce visible feedback.
2. **F4 (P1) — the band's verbs have no keyboard path.** The scrolled
   chip's jump-to-bottom exists only as a click. It gets a keybind in
   the shared registry so both frontends consume it.
3. **F1 (P2) — drawer text edits that never commit stay on screen.**
   Global-page text inputs commit on Enter only; on blur the field
   keeps showing an uncommitted (or invalid) edit with no cue.
4. **F2 (P2) — the images panel's delete confirm is two unlabeled
   icons.** Discovered during the fix pass: the GPUI frontend has no
   images panel yet (`app_shell.rs` "No GPUI images panel yet"); the
   icon pair lives in the libcosmic panel. **No code change** — the
   ruling records instead: the future GPUI panel ships its confirm
   pair labeled (`Confirm delete` / `Keep`, matching the profile
   delete's drawer.rs shape), and the divergence note covers the
   libcosmic pair until cutover.

## Design decisions

- **F7's feedback surface is the status band.** The band is the app's
  narration strip (status-bar spec: "derived, not decorated") and its
  `✓` copy flash is the established transient-verdict channel. The
  refusal verdict rides the same channel: a transient red verdict chip
  at the pasted-images seat's position (`✗ image over 32 MiB cap` /
  `✗ image unreadable`), held ~3 s (readable, unlike the 800 ms copy
  flash), cleared early by any successful paste. When the band is
  toggled off the refusal stays log-only — documented limitation; the
  band is default-on.
  - Core model: `dropterm-core/src/status.rs` gains a paste-refusal
    verdict slot (cause + deadline), not a new SegmentId.
  - The gpui paste road distinguishes refusal states instead of
    collapsing them into `None` (`terminal/clipboard.rs`
    `read_image_blocking` returns an outcome; `write_paste_image`'s
    refusal maps through too).
  - Libcosmic band renders nothing new (divergence note).
- **F4's chord is `Ctrl+Shift+End`**, mnemonic (End = end), inside the
  sanctioned Ctrl+Shift app-chord family, byte-safe: readline binds no
  `Ctrl+End`, and unlike `Ctrl+Shift+End`-as-byte concerns there is no
  shell byte on the End escape. Action id `scroll_to_bottom`, group
  "Other", label "Scroll to bottom". The engine's scroll-to-bottom (the
  band chip's route) is the one implementation both frontends call.
- **F1's blur rule is commit-or-revert.** On focus leaving a global-page
  text input: a valid edit commits (same `submit_write` path Enter
  uses); an invalid edit reverts the displayed text to the stored
  value. The display never lies in either direction. The profile form's
  transactional draft is untouched (every keystroke already feeds it).
- **F4 lands in both frontends** (the core registry change surfaces the
  row in libcosmic's editor regardless, and the jump verb already
  exists there); F1/F7 are gpui-only.

## Commands

- Build/test/lint gates run only through the just recipes:
  `just test` (per gate), `just check` (full gate before done).

## Files (per fix, ≤5 each)

- F4: `dropterm-core/src/keybinds.rs` (+tests),
  `dropterm-gpui/src/terminal/input.rs` (+tests),
  `dropterm-gpui/src/terminal/view.rs` (funnel arm),
  `src/keybinds.rs` (+tests) — libcosmic Action mapping,
  libcosmic pane funnel arm (`src/terminal/widget.rs` or its action
  dispatch site).
- F7: `dropterm-core/src/status.rs` (+tests),
  `dropterm-core/src/paste_cache.rs` (outcome type if owed),
  `dropterm-gpui/src/terminal/clipboard.rs`,
  `dropterm-gpui/src/terminal/view.rs`, `dropterm-gpui/src/status_bar.rs`,
  `dropterm-gpui/src/app_shell.rs` (verdict routing).
- F1: `dropterm-gpui/src/drawer.rs` (+tests).
- Docs: `docs/keybindings.md` (new row), the divergence note in
  `tasks/gpui-parity.md`.

## Testing strategy

Inline `#[cfg(test)] mod tests` per project rules: core registry pin
for the new row; core verdict-slot unit tests (deadline, clear-on-paste);
gpui mapping round-trip for the new id; drawer blur-commit tests on the
gpui test harness where the harness reaches the input events, else the
pure `submit_write` split pinned; libcosmic `action_id` pin. Every
runtime row (band verdict visible, chord firing live, blur commit on a
live drawer) is owner-run per the 2026-11-14 manual ruling — the
agent's deliverable is one concise walk instruction.

## Boundaries

- Always: run gates through just; docs updates in the same change (the
  `every_config_key_is_documented` and `docs-check` gates hold).
- Docs scope note (2026-11-15, discovered during the pass): `docs/`
  describes the shipped libcosmic binary, and the F7 verdict chip and
  F1 blur rule are GPUI-only — so `docs/images.md`, `docs/status-bar.md`,
  and `docs/settings.md` keep their current text (still true of the
  released app) and the GPUI behaviors live in the parity note until
  cutover. The `scroll_to_bottom` row **is** documented: both frontends
  received it through the shared registry.
- Ask first: any engine (grid) change — none is planned; if F7 turns
  out to need one, stop and report.
- Never: touch the libcosmic drawer/images-panel behavior, invent a
  sixth fix, or mark a runtime row green.

## Success criteria

- [ ] A refused image paste produces a visible verdict chip in the band
      for ~3 s (owner-walked); the log warn stays.
- [ ] `scroll_to_bottom` appears in the keybind editor's Other group,
      defaults `Ctrl+Shift+End`, fires the jump in both frontends
      (chord firing owner-walked; mapping pinned by tests).
- [ ] Blur on a valid edit commits it; blur on an invalid edit reverts
      the field (harness-testable parts pinned; live blur owner-walked).
- [x] The F2 ruling (labeled pair in the future GPUI panel) is recorded
      in `tasks/gpui-parity.md`.
- [x] Docs pages name every changed behavior; `just check` green.

## Status (2026-11-15)

The code and the harness pins are in; the three runtime rows are
owner-run and stay unchecked until walked (the 2026-11-14 ruling).

- **F4** — `c9df940`. The registry row, both id mappings, both funnels
  and `docs/keybindings.md`'s row. Pinned: the core registry test (id,
  group "Other", default display `Ctrl + Shift + End`), the GPUI
  round-trip and the 15-entry map count, the libcosmic round-trip and
  the 33-row group-coverage count. Owed: the chord firing live in each
  frontend.
- **F7** — `a9cd6f0`. `ImageRead` replaces the collapsed `None`, the
  cache's refusals join the verdict, and the shell holds it 3 s with
  the early clear on a successful paste. Pinned: the cap split, the
  seat and display helpers, the band's swap + attention + expiry.
  Owed: the chip on a live paste.
  Deviation from this chunk's design note: the deadline slot lives on
  the GPUI shell (`status_paste_refusal`) rather than in
  `dropterm-core/src/status.rs`; core keeps the pure display and seat.
  The outcome type sits in `dropterm-gpui/src/terminal/clipboard.rs`,
  where the road is, not in `dropterm-core/src/paste_cache.rs`.
- **F1** — `a286ffc`. `BlurDecision`/`blur_decision` in core settings,
  the drawer subscriber's blur arm, and the render-path reseed. Pinned:
  the three decision branches (no-op, commit clamped, revert). Owed:
  the live blur on an open drawer.
- **F2** — recorded in `tasks/gpui-parity.md`, no code change, as ruled.
- **Docs** — `docs/keybindings.md` gains the `scroll_to_bottom` row;
  the F7/F1 behaviors stay in the parity note per the docs scope note
  above. `just check` green (exit 0) on the full change set.
