# Where does bare Tab die on the dev surface? — the key-delivery module

Date: 2026-11-21 · Module: `key-delivery` (local spec approved
2026-11-21) · Record: live while the module runs — evidence lands
here as it is collected (owner ruling 2026-11-14: every runtime walk
is owner-run; this record quotes the owner's reports verbatim and
freezes at closure).

## TL;DR

**The compositor delivers Tab and the press dies client-side inside
gpui-pre — branch B1.** A1's WAYLAND_DEBUG capture (owner-run,
2026-11-21) shows ten well-formed Tab press/release pairs reaching
DropTerm's focused layer surface while the app's unconditional
`on_key` trace stays silent for every one of them (`c`/`d`/space
trace and print). Task 3 names the exact death site in gpui-pre-linux
before any arm is written, under the owner's carry-now policy
(2026-11-21).

## The symptom

On the dev (GPUI) DropTerm surface, bare Tab does nothing: `cd` + Tab
never fires bash's completion, while letters, modifiers, and Ctrl+C
all work. `DROPTERM_TRACE_KEYS=1` settled the shape of the hunt:
`on_key` (`dropterm/src/terminal/view.rs:1074`) traces every
keystroke unconditionally — letters trace and print, Tab traces
nothing. The press dies somewhere between the compositor and the pane
handler, upstream of every line of DropTerm key logic.

## What the headless audit cleared (2026-11-21, accepted as evidence)

Every layer except one was audited clean before this module opened:

- DropTerm's translation (`dropterm/src/terminal/input.rs:488`: bare
  `tab` → `\x09`) — clean.
- The suggest menu's Tab claim — the menu was closed at symptom time.
- Custom keybinds — none in the config.
- gpui-pre 0.3.12's Wayland keysym mapping — generic, no filtering.
- gpui dispatch — no pending multi-key `tab` chord anywhere; the app
  registers three Escape chords, Kit bindings are context-scoped, no
  keystroke interceptors.
- The shell — the config resolves bash, PTY children are bash.

Ecosystem issues that drop all keys don't fit a Tab-only death:
letters work, so every generic layer is clean, and only a Tab-shaped
filter upstream of the pane fits. The one boundary no headless pass
can measure: whether cosmic-comp delivers the Tab key event to this
surface at all. That is A1's question.

## The configured keyboard mode (spec open question 2, settled)

Read headlessly 2026-11-21 from the dev config
(`~/.config/io.github.l1br3_io.dropterm.Dev/config.ron:9`, sha256
`3c2d10df8e46639232c4a84a69489b14ed5a4bcf1b6ea34073e1fe09042bf712`):
`keyboard_mode: Exclusive`. The surface runs exclusive (Overlay,
exclusive_zone −1, `KeyboardInteractivity::Exclusive` via
`dropterm/src/executor.rs:233`/`:313`), exactly as the landscape
assumed — no mode-sensitive delivery conclusion is being misread, and
A1's frames are read against an exclusive-keyboard surface.

## A1 — the WAYLAND_DEBUG capture (decisive, owner-run)

Walk handed to the owner 2026-11-21, verbatim:

```text
DROPTERM_TRACE_KEYS=1 WAYLAND_DEBUG=1 just dev-reload > /tmp/dropterm-tab.log 2>&1
```

then, once the drawer is up: press `z`, press `Tab` once, toggle the
drawer away. (The launch rides a systemd-run `--scope`, which
inherits fds, so the redirect captures libwayland's stderr trace
alongside the app's own.)

**Verdict (2026-11-21): B1 — the compositor delivered Tab; the press
dies client-side inside gpui-pre.** The owner's run deviated from the
handed sequence and captured the literal symptom instead: `cd`,
space, a Ctrl tap, **Tab pressed ten times**, then the toggle chord
(the re-pressing is the frustration itself, and the capture is richer
for it). Frame evidence, line numbers into the archived log:

- **Validity holds.** The keymap event is present (line 307:
  `wl_keyboard#25.keymap(1, fd 32, 64756)`) and real key-press pairs
  arrived on the same focused keyboard — the channel demonstrably
  carries key events. The guard's `z` pair was never pressed; its
  substance (a working output channel, so an absent frame would mean
  something) is met by the `c`/`d`/space pairs, and the Tab frames
  are present anyway, so no misread is possible. Not void.
- **Focus held throughout.** `wl_keyboard#25.enter` onto the drawer's
  surface `wl_surface#105` at 02:14:55.337 (line 4716); no `leave`
  before or during the sequence.
- **Delivered frames** (this trace carries raw evdev codes — the
  letters cross-check the convention: 46→`c`, 32→`d`, 57→space,
  matching the app trace exactly): `c` 46 (line 5366), `d` 32 (5398),
  space 57 (5422), Ctrl 29 (5449/5470), **Tab 15 — ten press/release
  pairs, lines 5488–5638, 02:14:57.590–59.137** — then the Shift+Super
  toggle chord (5693+). Keycode 15 is evdev's `KEY_TAB`, the expected
  number.
- **The app's own trace** (`DROPTERM_TRACE_KEYS`, unconditional at
  `dropterm/src/terminal/view.rs:1074`): `dropterm: key "c"` (5368),
  `"d"` (5399), `"space"` (5423, 5811) — and **zero lines for the ten
  Tab presses** (nor the Ctrl tap). Every Tab pair arrived on the wire
  and none became a keystroke: the death sits between the
  wl_keyboard event and gpui's key dispatch, inside gpui-pre's
  wayland backend. The audit already cleared generic dispatch (no Tab
  chord, no interceptors), so the backend's keystroke construction is
  the prime suspect — confirmed at the named site before any arm is
  written (Task 3, gated at Checkpoint 1).

Archive: `tasks/evidence/dropterm-tab.log` (local, gitignored), 6053
lines, sha256
`c59e4bd64f26e6c928ca086944ea6a6066acbbe9e89be50e9c2041e54f9218ee`.
Owner's completion report, verbatim: "still not worknig".

The decision rule as handed, for the record (frames appear in press
order, so the key-event pair after the letter pair is Tab; keycode 15
on a standard evdev keymap is the cross-check, the sequence is the
matcher):

| Log shows | Verdict | Branch |
|---|---|---|
| Valid; Tab's press/release frames present | Compositor delivered Tab; it dies client-side inside gpui-pre | B1 |
| Valid; `z` present, Tab absent | cosmic-comp never sent Tab to this surface (or consumed it) | B2 |
| Void | No verdict; re-run A1 | — |

## A2 — the old-surface side check (refining, owner-run)

Pending. One line from the owner: does `cd` + Tab complete in the old
Nov-19 release surface? Old surface works → client-side (B1) or a
per-surface compositor difference (still B2); old surface broken too
→ compositor-side globally (B2, stronger). On any disagreement, A1's
protocol log wins — it is direct evidence of delivery, A2 is
behavioral.
