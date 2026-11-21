# Where does bare Tab die on the dev surface? — the key-delivery module

Date: 2026-11-21 · Module: `key-delivery` (local spec approved
2026-11-21) · Record: live while the module runs — evidence lands
here as it is collected (owner ruling 2026-11-14: every runtime walk
is owner-run; this record quotes the owner's reports verbatim and
freezes at closure).

## TL;DR

**The compositor delivers Tab and the press dies client-side — branch
B1 was right about the side, wrong about the crate.** A1's
WAYLAND_DEBUG capture (owner-run, 2026-11-21) shows ten well-formed
Tab press/release pairs reaching DropTerm's focused layer surface
while the app's unconditional `on_key` trace stays silent for every
one of them (`c`/`d`/space trace and print). Task 3's investigation
(read 2026-11-21, runtime-proven the same day) names the eater: the
Kit's Root registers a global-path `"tab"` focus-traversal binding
(gpui-base `root.rs:44`), its handler never propagates, and a consumed
binding returns before any key listener runs — so the pane never sees
Tab. gpui-pre's backend is clean end to end. The fix is in-repo (a
Terminal-context tab binding the pane answers), flipping the branch to
B3's shape; the Checkpoint-1 fourth carry dissolves. Owner
confirmation of the flip is requested before the arm is built.

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

Walk handed 2026-11-21: open the old Nov-19 release surface (the
pre-cutover libcosmic build, installed side by side), type `cd` then
Tab once, report whether completion fires.

**Result (2026-11-21): Tab does nothing there too.** Owner's report,
verbatim: "Tab does nothing there too". The rule as handed: old
surface works → client-side (B1) or a per-surface compositor
difference (still B2); old surface broken too → compositor-side
globally (B2, stronger); on any disagreement, A1's protocol log wins.
This is the disagreement case — A2's behavioral reading points at
B2, but A1's log is direct wire evidence (ten Tab pairs demonstrably
arrived on the dev surface) and it wins. The joint reading stays
honest about what A2 can and cannot say: the old surface is a
different client (libcosmic, not gpui-pre), so its Tab death may
share no cause with the dev surface's; a behavioral break there
names no layer and does not unmake delivery proven on the wire. A2
adds no branch weight against B1.

## Branch selection: B1 — the gpui-pre fourth carry

Selected 2026-11-21, at A2's close, by the spec's Phase A decision
table with A1-wins-on-disagreement applied. The frame evidence that
picked it (A1, archived log): ten well-formed Tab press/release
pairs (keycode 15, evdev's `KEY_TAB`) delivered 02:14:57.590–59.137
to the focused drawer surface `wl_surface#105` (lines 5488–5638)
while the unconditional `on_key` trace stayed silent for every one —
delivery proven, the death client-side inside gpui-pre, prime
suspect the backend's keystroke construction (generic dispatch was
audited clean). Next: Checkpoint 1 — the owner reviews verdict and
branch and, B1 standing, rules on carry shape (lean: git fork) and
the fourth carry itself (ask-first).

## Task 3 (B1 investigation): the site is named — and it is not in gpui-pre

Investigated 2026-11-21, read-only over the pinned sources plus four
runtime probes (scratch tests in the workspace, run under `just test`
on both identity feature sets, reverted after — the evidence is this
section). Verdict first: **gpui-pre-linux 0.3.12's keystroke path is
clean end to end; the eater is the Kit Root's global-path tab
focus-traversal binding in gpui-base 0.9.1, and the defect that makes
it DropTerm's to fix is that the pane never claims Tab.** The branch
flips to B3's shape (in-repo fix under TDD, no carry), recorded below
with its evidence; owner confirmation requested before any arm is
built (the flip dissolves the Checkpoint-1 fourth carry).

### What was read and cleared, layer by layer

- **Wire → keycode → keysym** (gpui-pre-linux
  `src/linux/wayland/client.rs:2178-2185`): `keycode = wire evdev +
  MIN_KEYCODE(8)` (`client.rs:119`) — cross-checked against the
  archived log's own letter frames (46→`c`, 32→`d`, 57→space, each
  followed one or two lines later by the app's matching trace), so
  Tab's wire 15 → xkb 23 under the session's evdev/pc105/us keymap
  (`localectl` read 2026-11-21).
- **The only per-key gate** in the handler is
  `Pressed if !keysym.is_modifier_key()` (`client.rs:2183`); Tab is
  not a modifier keysym by source, not memory:
  `xkeysym-0.2.1/src/lib.rs:168-176` (Shift_L..=Hyper_R,
  ISO_Lock..=ISO_Level5_Lock, Mode_switch, Num_Lock — 0xff09 is in
  none). The Pressed arm runs and calls
  `focused_window.handle_input(KeyDown)`.
- **Keystroke construction** (`src/linux/platform.rs:1461-1583`):
  `Keysym::Tab => "tab"` (`:1523`); `key_char` is None for Tab
  (`:1575`, control char) — the same shape as Enter, which works.
- **The wayland window's `handle_input`** (`src/linux/wayland/window.rs:1584-1607`):
  no Tab-shaped filter; `is_blocked()` (`:943`) needs a blocking
  child and dies all input, refuted by the space that traced at log
  line 5811 — after all ten Tabs. The compose path
  (`client.rs:2186-2225`) clears `key_char` at most; it never skips
  dispatch.
- **gpui-pre's dispatch** (`src/window.rs:6293-6463`,
  `src/key_dispatch.rs:483-519`, `src/keymap.rs:165-270`): a
  keystroke dies before key listeners exactly when a binding matches
  and is consumed — the pending/chord path was re-read and cannot fit
  ten silent presses (each press after the first flushes its
  predecessor through replay, `window.rs:6716-6752`).

### The eater, named with its chain

- **The binding:** `gpui-base-0.9.1/src/root.rs:44-45` —
  `KeyBinding::new("tab", Tab, Some("Root"))` and
  `"shift-tab" → TabPrev`, registered by `root::init` (`:42`), which
  `gpui_base::init` calls (base `lib.rs:243-259`), which
  `gpui_component::init` calls (`carried/gpui-component/src/lib.rs:138`),
  which DropTerm's boot runs (`dropterm/src/app_shell.rs:5763`).
- **Why it fires on every path:** the Root view's render stamps the
  context on the wrapping div — `root.rs:255` `.key_context("Root")`
  — and the drawer mounts exactly this Root:
  `app_shell.rs:1818` (`KitRoot::new(...)`, where `KitRoot` is
  `gpui_component::Root`, itself the `gpui_base::Root` re-export at
  `carried/gpui-component/src/lib.rs:102`). A context predicate
  matches at the depth it appears (`keymap.rs:274-280`), so a
  root-wrapped div puts "Root" on every focused dispatch path in the
  window.
- **The swallow:** the handler `on_action_tab` (`root.rs:222-224`)
  runs `advance_focus` (focus traversal) and never calls
  `cx.propagate()`. A consumed matched binding returns before key
  listeners run: `gpui-pre-0.3.12/src/window.rs:6438-6451` returns on
  `!cx.propagate_event`, so `finish_dispatch_key_event` (`:6454`)
  and `dispatch_key_down_up_event` (`:6518`) never run — the pane's
  `.on_key_down` (`dropterm/src/terminal/view.rs:1658`) never fires,
  the unconditional trace at `view.rs:1074` stays silent, and
  `input.rs:488`'s `tab → \x09` is never consulted.

### Why Tab and only Tab

- Letters, Enter, Space: no binding on the path matches them; they
  fall through to key listeners (`window.rs:6454`).
- Ctrl+C: the Root also binds `ctrl-c → Copy` on Linux
  (`root.rs:48-49`), but `on_action_copy` (`:230-239`) propagates
  when the selection is empty — the event survives to the pane. Tab's
  handler always acts (advance focus) and never propagates. The
  asymmetry is exact: Ctrl+C lives by the propagate-on-noop it
  happens to have; Tab dies for lacking one.
- The visible nothing: `advance_focus` → `Window::focus_next`
  (`root.rs:222-223`); the drawer's terminal pane is effectively the
  only tab stop, so focus cycles onto itself invisibly — ten presses,
  ten silent no-ops.
- The Ctrl-tap silence (A1's log) is by architecture, not a second
  eater: a bare modifier tap synthesizes a keystroke that only drives
  bindings, and `dispatch_key_down_up_event` receives the
  `ModifiersChangedEvent`, which cannot downcast to a `KeyDownEvent`
  (`window.rs:6303-6344`, `:6518`).
- The suggest ghost visible in the log ("pane ghost now \"src/\"",
  line 5381+) is downstream of `on_key` — it never saw Tab either;
  the suggest menu's Tab-accept (`suggest.rs:64`) is equally dead
  today and revives with the fix.

### The runtime proof (four scratch probes, 2026-11-21, both feature sets)

A focused element with `key_context("Terminal")` and an unconditional
key listener, under four window shapes (probe code reverted after the
run; the printed results are the evidence):

| Probe | Window shape | Listener saw |
|---|---|---|
| bare | plain window, no init | `c, tab, enter, space` |
| component-init | + `gpui_component::init` (bindings live, no Root mounted) | `c, tab, enter, space` |
| **kit-root** | **mounted through the Kit Root — the drawer's shape (`app_shell.rs:1818`)** | **`c, enter, space` — tab gone** |
| pane-binding | Kit Root + a Terminal-context `"tab" → ProbeTab` binding on the focused element | the action fires; Tab reclaimed |

The kit-root row reproduces the symptom exactly — under the Root the
drawer mounts, bare Tab vanishes while letters, Enter, and Space pass;
the pane-binding row proves the reclamation mechanism (a deeper
context outranks the Root binding, `keymap.rs:154-159`'s depth
precedence). The bare and component-init rows re-confirm the audit's
dispatch conclusion empirically: nothing else eats Tab.

### The B3 flip, recorded with its evidence

The chunk's Task 3 clause: if the evidence contradicts the audit's
site, record the flip. The audit cleared "gpui dispatch" against
DropTerm's own registrations — three Escape chords, verified again
today (`app_shell.rs:5767-5770`) — and missed that the Root's
traversal binding rides every path. The named layer (gpui-base's
Root) is not gpui-pre, and the minimal fix needs no carry at all:

- **The defect, precisely:** DropTerm mounts a text surface (the
  terminal pane) under the Kit Root without claiming Tab — no key
  context on the pane's frame, no tab binding anywhere in
  `terminal/view.rs` (grep-verified). Zed's own editor survives the
  same Root design precisely because it binds deeper.
- **The minimal arm (one behavior changed):** give the pane's frame a
  key context and bind `"tab"`/`"shift-tab"` in it to an action whose
  handler forwards the keystroke into the same `on_key` path letters
  take. Every existing rule then applies unchanged — the suggest
  menu's Accept-first routing (`suggest.rs:48-70`), the chord table
  (`input.rs:258-260`), app-cursor modes — and every other surface
  (settings drawer inputs, dialogs) keeps the Root's traversal. The
  pane-binding probe pins the mechanism.
- **Carry consequence:** no gpui-pre fork, no `[patch.crates-io]`
  fourth entry. The Checkpoint-1 ruling (git-fork of gpui-pre 0.3.12)
  is dissolved by the evidence; the fix lands in-repo under TDD —
  Tasks 10-12's shape (failing pin test at the pane, minimal fix,
  owner walk).
- **Owner confirmation requested** before the arm is built: the
  carry was ruled ask-first, so its dissolution is too. The upstream
  ask survives in honest shape: not a defect demand but a design note
  for gpui-base — a Root-level tab-traversal binding makes bare Tab
  unreachable to any focused content that does not install a deeper
  binding; text surfaces must reclaim it, and nothing in the API or
  docs says so.

## Checkpoint 1 re-convened: the flip confirmed

Confirmed 2026-11-21 (owner, via the structured ask on the `/build`
run): B3 accepted — the in-repo Terminal-context tab/shift-tab binding
is the arm; the fourth carry (l1br3-io fork of gpui-pre 0.3.12) is
dissolved, and with it Tasks 4-6 in their carry shape. B1/B2 both
stand as "not taken" for the closure audit: B2's verdict was A1's wire
log (Tab delivered), B1's crate was exonerated by Task 3's
read-plus-probes.

## Task 10 (B3): the pin, red for the proven reason

Written 2026-11-21, `terminal::view::tests::
a_dispatched_tab_reaches_the_pane_under_the_kit_root` — A1's headless
twin. A real pane boots against a PTY whose child is `cat` persisting
its stdin to a temp file, mounts through `gpui_component::Root` (the
drawer's own shape, `app_shell.rs:1818`), takes focus, and receives
`tab`, `shift-tab`, `c`, `enter` through gpui's real keystroke
dispatch. The witness file after the run:

```
left: "c\n"   right: "\t\x1b[Zc\n"
```

The letter and the enter landed (the passing clauses, exactly as A1's
`c`/`d`/space traced); the tab and shift-tab bytes never reached the
PTY — the Root's traversal binding ate them upstream of the pane's
`on_key`, the same death the archived capture showed ten times over.
Red on the full suite: 211 passed, 1 failed (the pin), nothing else
touched.

## Task 11 (B3): the fix works, parked at the suite gate

The arm landed 2026-11-21 and turns the pin green exactly as drawn:
`terminal/view.rs` gains the pane's Tab claim (`bind_pane_keys`:
`"tab"`/`"shift-tab"` bound in a new `"Terminal"` key context on the
pane's frame, the handlers forwarding platform-shaped tab keystrokes
into `on_key` — the suggest menu's Accept-first and the chord table
apply unchanged), and `app_shell.rs`'s boot registers it once beside
the Kit init (the chunk's file list gains that one line; flagged at
review). The pin passes 20/20 standalone with its own harness
defusals (the engine Arc leaked past the gate, the witness booted
echo-silent — the record's anatomy section says why).

The landing stopped at `just check`, which cannot run green today:
the suite carries a pre-existing flake class — pane-booting gpui
tests tripping the test scheduler's thread-affinity gate whenever a
foreign-thread wake (PTTY reader send or drop, a generator waiter's
nudge) lands inside an executor poll window. Measured: the repaste
pair 0/10 alone but 5/8 even on the tree without this fix; the pin's
concurrency widens the windows. Per-test defusal moved the flake
(font test, unnamed threads) rather than closing it. Owner call
(2026-11-21, structured ask): **root-fix the class first** —
diagnosed and parked in its own record,
[pty-test-gate](pty-test-gate.md), awaiting `/spec`. Task 11's diff
sits uncommitted in the checkout; its commit lands on a truly green
gate, then Task 12 (owner walk, Checkpoint 2) follows.

## Task 11 (B3): landed on the owner's re-ruling

The park lifted the same day it was set. The owner re-ruled in the
landing session and confirmed it on the 2026-11-21 `/build` run —
"Yes, I OK'd committing it" — so `124a113` is the blessed landing,
with `d539604` as its clippy follow-up (the pin's harness binding was
underscore-prefixed but used, tripping `used_underscore_binding`).
What shipped is the drawn arm verbatim: `terminal/view.rs`'s
`bind_pane_keys` (the `"Terminal"` key context on the pane's frame,
`"tab"`/`"shift-tab"` bound to actions that rebuild the
platform-shaped keystroke and forward it into `on_key`), registered
once at boot beside the Kit init (`app_shell.rs`), with the pin's
harness defused on the three measured fronts — autosuggestions off,
`stty -echo`, and the engine Arc leaked past the gate (the
[pty-test-gate](pty-test-gate.md) anatomy says why each is needed).
That session died before the local bookkeeping caught up; the chunk,
todo, SPEC, and this record were reconciled on the `/build` run the
same day.

The gate's honest standing on reconciliation day: fmt, clippy (both
identity feature sets), and docs-check green; the test gate red
three-of-three full-suite runs, each aborting on
`a_repaste_verb_pastes_with_the_panel_standing_down`'s
nondeterminism alone — every other test passing each run, the named
test green standalone, the pin green. That is the pre-existing,
load-sensitive pty-test-gate class (measured 5/8 on the tree before
this fix existed); the owner landed with it open, and it stays parked
for its own `/spec` with the two `#[ignore]`d tests as its standing
debt. Checkpoint 2 (the landed arm's review) and Task 12 (the owner
walk and closure) follow.

## Task 12 (B3): owner acceptance and closure

Closed 2026-11-21 by the owner's blanket verification directive, issued
on the `/build auto` run that closed the module alongside tray and
icon-activation — verbatim: "skip tray, I've verified them and key
delivery, same with icon activation. they are all done". No per-clause
walk detail was requested or reported; the directive is the acceptance
evidence. Checkpoints 2 and 3 close with it — the landed arm's review
and this closure audit ride the same ruling.

The spec's five success criteria, audited:

1. **A1 archived with its verdict** — the log at
   `tasks/evidence/dropterm-tab.log` (sha256 cited in the A1 section),
   ten Tab pairs on the wire against a silent `on_key` trace; the
   decision rule never met a void capture.
2. **Owner acceptance walk** — closed by the directive above: `cd` +
   Tab completion on the dev surface, letters and chords unchanged.
3. **B3 obligations** — the in-repo fix with its pinning test:
   `124a113` (+ `d539604`, its clippy follow-up), pin
   `a_dispatched_tab_reaches_the_pane_under_the_kit_root`.
4. **`just check` green, both identity feature sets** — green except
   the pre-existing, load-sensitive [pty-test-gate](pty-test-gate.md)
   class (fmt/clippy/docs-check green; the abort lands on
   `a_repaste_verb` under load, green standalone), parked for its own
   `/spec` under the owner's landing ruling.
5. **This record with its README row** — the row flips to complete;
   this section is the record's last write.

B1 (gpui-pre carry) and B2 (cosmic-comp upstream) stand as "not taken"
(recorded at the re-convened Checkpoint 1). No carry exists, so there
is no retirement condition. The upstream ask survives as the gpui-base
design note in the Task 3 section — a Root-level tab-traversal binding
makes bare Tab unreachable to any focused content without a deeper
binding — for the owner to file if and when they choose.
