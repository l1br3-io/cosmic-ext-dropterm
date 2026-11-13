# SPEC: gpui-terminal — the terminal view on GPUI

Status: not started · Phase 3 · lane: main (branch `port/gpui-terminal`, after
`gpui-scaffold`; runs beside `gpui-surface`, integrates after both)

## Objective

The terminal view: the alacritty grid rendered through GPUI's text stack,
with everything a DropTerm tab does — input, keybinds, selection,
scrollback, mouse reporting, resize-to-SIGWINCH, clipboard. The engine is
`dropterm-core`'s (unchanged); this slice is the view and input halves, the
way `terminal/widget.rs` is the libcosmic half today. Zed's
`crates/terminal` is the reference for grid→GPUI rendering, not code to
import.

## Preconditions

`gpui-scaffold` merged (a window to render into, the pin in place);
`shared-core`'s engine state available.

## Scope

- Grid render: cells from `alacritty_terminal`'s grid through GPUI text
  primitives — colors and attributes off the resolved scheme (core's theme
  glue), damage-driven redraw, cursor (the block caret's first-frame
  visibility is a pinned manual row).
- Resize: view-size changes → `Term::resize` → SIGWINCH, the check-9 path
  (8×20 → 128×20 proven on the spike).
- Input and keybinds: key events mapped through core's keybind model
  (default chords + `custom_key_binds` overlay) to actions; IME and
  paste-as-type per the libcosmic build's behavior.
- Selection and scrollback: pointer selection, URL detection where the
  libcosmic build has it, scrollback navigation.
- Mouse reporting: the protocol logic in `mouse_reporter.rs` (core-side,
  framework-free) driven by GPUI pointer events.
- Clipboard: copy/paste through GPUI's clipboard — the write gate is
  focus-plus-input-serial and the focused terminal satisfies it by
  construction (check 8; the anchor does not port); wl-clipboard-rs's
  data-control path keeps serving copy-while-not-focused, today's
  behavior.
- Integration: the view wired into the GPUI surface executor's windows —
  the scaffold's placeholder content goes away; the drawer shows a live
  terminal.

## Commands

    cargo run -p dropterm-gpui    # rig + VM walks

## Testing strategy

Pure halves (key event mapping, selection model, resize math) unit-tested
headless in the crate. Rendering and input are compositor-evidenced: the
rig and the validation VM, screenshots and logs, the spike's check-9
evidence routes. The terminal rows of the manual checklist are the
acceptance list.

## Boundaries

Never: fork or modify `alacritty_terminal` beyond the two carried forks
already in the ledger (OSC 133;D); port the engine-side cosmic-text buffers
(check 9 ruled them out); reintroduce any clipboard anchor surface.

## Success criteria

A live PTY session renders, types, selects, scrolls, resizes, and copies
inside the GPUI drawer on COSMIC and sway, passing the terminal rows of the
manual checklist; caret-on-show paints the block cursor in the first shown
frame.

## Size

L.
