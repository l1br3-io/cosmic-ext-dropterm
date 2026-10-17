# Research: what can the status band honestly know — and what should it do with it?

Date: 2026-10-17
Question: *Cross-referencing every terminal that ships (or refuses) a status
surface — and the OSC protocols that feed them — which UX and capability
improvements are worth making to DropTerm's 28 px glance bar, and which are
feasible given that the engine (`alacritty_terminal` 0.25.1) surfaces no OSC to
the host app?* Successor to [status-bar-ux](status-bar-ux.md) (2026-10-11),
which settled the bar's shape; this record settles its next tier.

## TL;DR

**The band's next step is a command-lifecycle tier: live elapsed time while a
command runs and last-command duration after it finishes, built entirely from
the `tcgetpgrp` transition machine `suggest.rs` already runs — no OSC, no
shell integration, no new syscalls beyond the sanctioned snapshot pair.**
Every OSC-fed capability the peer survey crowns highest-value — exit code from
OSC 133 D, progress from OSC 9;4, authoritative/remote cwd from OSC 7, program
status from OSC 7501 — is **blocked by one seam**: `alacritty_terminal` 0.25.1's
`Event` enum carries no OSC passthrough, the event loop owns the PTY read path
(a second reader would race bytes), and patching vte is twice-rejected
([terminal-de-integration](terminal-de-integration.md),
[terminal-autocomplete](terminal-autocomplete.md)). Those stay rejected until
upstream grows an OSC event — the named revisit trigger. Three smaller honest
wins ship alongside the duration tier: a scroll-position chip (state only the
terminal knows), an ssh-destination stand-in for the path while `ssh` owns the
foreground, and two terminal selection idioms on the band (double-click select,
middle-click paste).

## Findings

### 1. iTerm2 — still the only literal status bar, and its lesson is layout, not data

Per-profile drag-and-drop components (CPU/memory/network graphs, cwd, host,
user, job name with a job-tree popover, git state, clock, progress indicator,
UI buttons, Python-scriptable custom components), each with a **Priority**
(default 5, "used when space runs short") and Compression Resistance
([status bar docs](https://iterm2.com/documentation-status-bar.html)). Two
layout algorithms, explicitly traded off in the docs: *tight packing*
(components as springs, "jumping around whenever a value changes") vs *stable
positioning* (the default). **No exit-code component exists** — exit status
lives in shell-integration marks ("The mark on a command line will turn red if
a command fails"; right-click reveals "the return status code, working
directory, running time" — [shell integration
docs](https://iterm2.com/documentation-shell-integration.html)). Git data is
polled by periodically running sandboxed git — subprocess polling, the
anti-pattern DropTerm already avoids.

### 2. WezTerm — event-fed right status with a 1 s floor and single-flight

Status lives right of the tab bar (`set_right_status`), fed by the
`update-status` Lua event: default **`status_update_interval` 1000 ms**,
guaranteed **only one callback in flight** (an overrunning callback delays the
next), and **immediate firing when a user var changes** ([event
docs](https://wezterm.org/config/lua/window-events/update-right-status.html),
[interval
docs](https://wezterm.org/config/lua/config/status_update_interval.html),
[shell integration](https://wezterm.org/shell-integration.html)). The official
examples are DropTerm's own segment list — cwd + host (from OSC 7, "can pick
up remote hosts"), clock, battery. The data path is push-not-poll: OSC 7 cwd,
OSC 133 marks, OSC 1337 user vars (`WEZTERM_PROG`/`WEZTERM_USER`/
`WEZTERM_HOST`) emitted "each time a prompt appears and before executing a
command".

### 3. kitty — the tab bar carries everything, and progress is default-on

`tab_title_template` defaults to
`{fmt.fg.red}{bell_symbol}{activity_symbol}{secure_input_symbol}{fmt.fg.tab}{tab.last_focused_progress_percent}{title}`
— **bell, activity, and OSC 9;4 progress percent render in every tab by
default**, with bell/activity auto-prepended for older templates
([options/definition.py](https://github.com/kovidgoyal/kitty/blob/master/kitty/options/definition.py),
rendered at [kitty.conf](https://sw.kovidgoyal.net/kitty/conf/)). Perf honesty
is in the docs: `tab.active_wd`/`tab.active_exe` are marked **"(expensive,
requires syscall)"** — external validation of DropTerm's snapshot discipline.
Newest kitty surfaces `tab.program_status` from **OSC 7501** ("blocked on the
user, failed, finished or working"). OSC 9;4 draws an edge bar
(`progress_bar` top/bottom/left/right/hidden).

### 4. Ghostty — refuses the bar, ships progress anyway

The status-bar request sits open and stale ([discussion
#2421](https://github.com/ghostty-org/ghostty/discussions/2421); Hashimoto
wants "semantically separating config from layout" and defers widgets to a
future plugin API) — yet Ghostty 1.2 shipped **OSC 9;4** as a GUI progress
bar with error/indeterminate/paused states, a **hardcoded ~15 s decay**
(terminals cannot detect a dead reporter), and guidance that programs refresh
"at least once per second" ([1.2.0 release
notes](https://ghostty.org/docs/install/release-notes/1-2-0), [ConEmu-OSC
reference](https://ghostty.org/docs/vt/osc/conemu)). Progress is the one
bar-class feature even the chrome-minimal terminal adopted.

### 5. Warp — metadata attached to blocks, and the exit-code complaint

Blocks carry command, output, exit code, cwd, git state, fed by shell hooks
emitting JSON escape payloads (`warp_preexec` command text, `warp_precmd` cwd
+ git, a command-finished event with the exit code) ([block-model
post](https://www.warp.dev/blog/block-model-behind-warps-agentic-development-environment)).
The UX gap is instructive: the block header shows duration + pass/fail but
**not the numeric exit code** — a long-open complaint ([warpdotdev/warp
#13027](https://github.com/warpdotdev/warp/issues/13027)). When metadata does
reach a surface, users want the number, not just the red dot.

### 6. VS Code — DropTerm's model, formalized

`TerminalStatusItem`s register per-terminal, left- or
**right-aligned near the terminal tabs**, with `text`/`tooltip`/`color` and a
clickable `command` ([API
reference](https://code.visualstudio.com/api/references/vscode-api)) — the
same shape as DropTerm's band. Exit codes arrive via OSC 633 D (VS Code's 133
superset, with an anti-spoof nonce and exact command line) and drive command
decorations ([shell integration
docs](https://code.visualstudio.com/docs/terminal/shell-integration)).

### 7. tmux — the convention, and the documented perf model

`status-left`/`status-right` anchors, window flags `* - # ! ~`, and the
canonical mitigation for the script-in-the-bar trap: `#()` runs
stale-while-revalidate, "the status line will not be updated more than once a
second", `status-interval` default 15 s ([tmux(1)](https://man7.org/linux/man-pages/man1/tmux.1.html)).
tmux **drops OSC 133 B/D in passthrough** — "no notion of a command as an
object, and no success/failure or timing information" ([tmux
#5237](https://github.com/tmux/tmux/issues/5237), [tmux
#3064](https://github.com/tmux/tmux/issues/3064)) — the same blind spot
DropTerm's engine seam imposes.

### 8. Windows Terminal, Alacritty, Zed — the no-bar pole, marks instead

No bar ([microsoft/terminal
#3459](https://github.com/microsoft/terminal/issues/3459), open since 2019);
the team's energy went to OSC 133 marks rendered as scrollbar marks ("0 =
success, anything else as an error") plus **OSC 9;9** cwd reporting ([shell
integration
tutorial](https://learn.microsoft.com/en-us/windows/terminal/tutorials/shell-integration)).
Alacritty refuses all chrome ([README
FAQ](https://github.com/alacritty/alacritty)); Zed keeps terminal status
minimal ([terminal docs](https://zed.dev/docs/terminal)).

### 9. The OSC feeding table

| Sequence | Carries | Primary source | Adopters |
|---|---|---|---|
| OSC 7 | cwd (+host) | [WezTerm](https://wezterm.org/shell-integration.html), [iTerm2](https://iterm2.com/documentation-escape-codes.html) | WezTerm, kitty, iTerm2, VTE, WT (as 9;9) |
| OSC 9;4 | progress 0–100, error/indeterminate/paused | [ConEmu](https://conemu.github.io/en/AnsiEscapeCodes.html), [Ghostty](https://ghostty.org/docs/vt/osc/conemu) | ConEmu, WT, kitty ([#3679](https://github.com/kovidgoyal/kitty/issues/3679)), Ghostty 1.2+ |
| OSC 133 A/B/C/D | prompt/command zones, **exit code in D** | [Contour spec](https://contour-terminal.org/vt-extensions/osc-133-shell-integration), [WT tutorial](https://learn.microsoft.com/en-us/windows/terminal/tutorials/shell-integration) | iTerm2, WezTerm, kitty, Ghostty, WT, VS Code (as 633) |
| OSC 633 | 133 + command line + nonce + properties | [VS Code](https://code.visualstudio.com/docs/terminal/shell-integration) | VS Code |
| OSC 777 | desktop notification | [WezTerm](https://wezterm.org/config/lua/config/notification_handling.html) | WezTerm, kitty |
| OSC 1337 | per-pane user vars | [iTerm2](https://iterm2.com/documentation-escape-codes.html) | iTerm2, WezTerm |
| OSC 7501 | program status (working/blocked/done/error + percent) | [Hashimoto's PSP](https://mitchellh.com/writing/program-status-osc7501) | libghostty, kitty; POCs for terraform/claude-code |

Two spec facts that shape the design:

- **Durations are derived, never transmitted.** No primary spec puts duration
  in OSC 133 D (Contour documents only the exit code; the `D;exit;duration`
  variant in third-party summaries is apocryphal). Warp and VS Code derive
  duration from C→D timestamps. Deriving it locally is the standard move —
  DropTerm can do the same from its own observed edges.
- **Progress needs decay** (~15 s in Ghostty) because a dead reporter leaves
  the bar lying. Any future progress segment inherits this rule.

### 10. What DropTerm has today (grounding)

- The band: 28 px custom iced widget, eight segments, two clusters, priority
  `DROP_ORDER`, click-to-copy full values, drag-to-select display text,
  per-band context menu, inline path reveal
  (`src/status_bar.rs:56`, `:109-156`, `:461`, `:1040`).
- Feeds: `refresh_status_snapshot` (`src/app.rs:486`) — the lone
  `/proc/<pid>/comm` + `.git/HEAD` read pair — fires at four piggyback
  points: active-tab `Wakeup` and `Title`/`ResetTitle`
  (`src/app/tab_actions.rs:1118`, `:1152`), the config echo
  (`src/app.rs:1969`), and the show-arm refresh-on-landing
  (`src/app.rs:2093`). The clock ticks minute-aligned and gated
  (`src/app.rs:509`). cwd rides `check_tab_cwd` — a
  `/proc/<pid>/cwd` readlink on every Title event
  (`src/app/tab_actions.rs:960`, `src/terminal/engine.rs:689`).
- The transition machine already exists: `suggest.rs` diffs the foreground on
  every `Terminal::update` — **idle→running is Enter (commits the typed
  line), running→idle is command finish** (`src/terminal/suggest.rs:164-205`,
  observed at `src/terminal/engine.rs:1213-1225`). Output-activity-driven,
  and the tty's own echo of Enter guarantees a `Wakeup` within milliseconds
  of a command starting.
- Bell already aggregates: inactive tabs carry an attention marker;
  a bell while hidden posts one notification per tab per hide period
  (`src/app/tab_actions.rs:1060`, `src/notify.rs`).

### 11. The seam: why the OSC tier is blocked, precisely

`alacritty_terminal` 0.25.1's `EventListener` surfaces exactly
`Title/ResetTitle/Bell/Wakeup/ChildExit/ClipboardStore/ClipboardLoad/
ColorRequest/PtyWrite/TextAreaSizeRequest/CursorBlinkingChange/
MouseCursorDirty/Exit` — **no OSC passthrough** (crate `src/event.rs`,
checked 2026-10-17 against the vendored source). The vte parser consumes OSC
inside the term; DropTerm's `TaggedEventProxy` (`src/terminal/engine.rs:187`)
sees only the enum above. The PTY read path is owned by the event loop thread
— teeing the master fd would race bytes. Observing OSC 7/9;4/133/777/7501
therefore requires a `[patch]` of vte or alacritty_terminal, which
[terminal-de-integration](terminal-de-integration.md) rejected and
[terminal-autocomplete](terminal-autocomplete.md) re-rejected (2026-10-17).
Shell-integration injection is likewise rejected. **Revisit trigger:** an
upstream `alacritty_terminal` release (or accepted PR) adding an OSC event
variant — that single seam unblocks exit codes, progress, remote cwd, and
program status at once, and this record's blocked tier becomes the roadmap.

## Recommendation

Ship, in order:

1. **Command lifecycle segment (the capability tier).** Timestamp the two
   edges the suggest machine already detects: idle→running sets
   `started_at` (echo of Enter makes the edge near-instant), running→idle
   sets `finished_at` and freezes `last_duration`. Display: while running,
   live elapsed beside the process name in the left cluster (`cargo 2:41`,
   dimmed like the right cluster); after finish, the frozen duration until
   the next command starts. Honesty rules (the band's own ethos): a duration
   shows only when both edges were witnessed in this surface; a command
   started before the tab existed or whose start was never observed shows
   nothing, never a guess. Click copies the duration string. This is Warp's
   block metadata and WezTerm's `WEZTERM_PROG` timing without their shell
   integration, because DropTerm owns the PTY and the foreground diff.
   Live elapsed needs a tick: **one scoped second-timer** — armed only while
   the active tab is running, the surface visible, and the segment shown
   (WezTerm's single-flight, ≥1 s precedent; tmux's 1-update/sec ceiling).
   The prior record's "no standing timer" verdict holds for idle periods;
   this timer exists only while a command actually runs.
2. **Command-finish notification while hidden (the sibling capability).** The
   finish edge plus the existing notify path gives Ghostty's
   `notify-on-command-finish` semantics — arguably the single most valuable
   drop-down feature ("run the build, hide the terminal, get told"):
   `command_finish_notification_enabled` + a minimum-duration threshold
   (Ghostty's `notify-on-command-finish-after` 5 s default), one per tab per
   hide period like the bell latch. Zero new machinery: same edge, same
   `notify.rs`, same anti-spam shape.
3. **Scroll-position chip.** `display_offset() > 0` is state only the
   terminal knows — Jaykul's criterion in
   [microsoft/terminal #3459](https://github.com/microsoft/terminal/issues/3459),
   and the exact glance question a drop-down asks on re-landing ("am I
   looking at the end?"). A dim `↑`-prefixed line-count chip in the right
   cluster, conditional like zoom/grid, click = jump to bottom (the one
   deliberate action chip; the prior record's "left-click = copy" rule bends
   exactly once, for the same reason Pin did in the tab menu — the chip has
   no copyable value). In-memory grid read at view assembly; zero fs.
4. **ssh-destination stand-in.** While the foreground process is `ssh`, the
   local cwd the path segment shows is stale. Read
   `/proc/<pid>/cmdline` (snapshot point only) and show the command's own
   destination token (`user@host`) in the path's seat — iTerm2's
   SSH-context host segment without OSC 7. The value is literally what the
   user typed; absent argv shows the segment's local fallback, never a
   guess. Folds into the path segment (no new key).
5. **Selection idioms on the band.** Double-click selects the segment's
   whole display text (terminal word-selection precedent); middle-click
   pastes into the narrated terminal (the band is terminal chrome; the menu
   already carries Paste). Both ride the existing selection machinery.
6. **Config and docs.** New keys follow the house pattern —
   `status_bar_show_duration`, `status_bar_show_scrollback`,
   `command_finish_notification_enabled` (+ threshold key) — each documented
   in `docs/configuration.md` (the `every_config_key_is_documented` gate) and
   toggled in the Behavior tab. Drop order: duration and the scroll chip
   drop late (before the clock) — they answer "what's happening now".
7. **Guardrails restated.** The render path stays syscall-free by
   construction; new data lands in the snapshot pair only
   (`Instant` arithmetic is free, `/proc` reads at piggyback points only,
   hidden segments never gathered); no per-frame work; no subprocesses ever
   (iTerm2's git polling and tmux's `#()` are the documented anti-patterns).

**Rejected, with reasons:**

- **OSC 133 D exit-code chip, OSC 9;4 progress, OSC 7 remote cwd, OSC 777
  attention, OSC 7501 program status** — the peer-optimal features, all
  blocked by the §11 seam; re-litigating the twice-rejected vte patch for
  bar polish is the wrong trade until upstream moves. When it does, exit
  code + duration pair first (Warp #13027 shows the number matters), then
  progress with Ghostty's decay rule, then 7501 state in the disc.
- **Exit code by grid-scraping** (looking for the shell's echoed `$?` or
  prompt glyphs) — the no-bar camp's "heuristics are a trap" objection
  applies; a wrong exit code is worse than none.
- **CPU/memory/network segments** — re-affirmed rejected (status-bar-ux rec 1:
  they answer "how is my machine", not "what am I doing").
- **Custom/scriptable segments** (byobu's ~45-script picker, iTerm2's Python
  components) — the deliverable becomes a picker over an empty default;
  the fixed-segment + per-key toggle model is the sane middle.
- **A second cwd poll on a timer** — the Title-piggybacked `/proc` readlink
  already covers local shells; a timer buys a smaller staleness window than
  refresh-on-show and costs a standing wakeups source.

## Sources

- iTerm2: status bar — <https://iterm2.com/documentation-status-bar.html>;
  shell integration — <https://iterm2.com/documentation-shell-integration.html>;
  escape codes — <https://iterm2.com/documentation-escape-codes.html>
  (fetched 2026-10-17)
- WezTerm: shell integration — <https://wezterm.org/shell-integration.html>;
  update-status — <https://wezterm.org/config/lua/window-events/update-right-status.html>;
  status_update_interval — <https://wezterm.org/config/lua/config/status_update_interval.html>;
  set_right_status — <https://wezterm.org/config/lua/window/set_right_status.html>;
  notification_handling — <https://wezterm.org/config/lua/config/notification_handling.html>
  (fetched 2026-10-17)
- kitty: options/definition.py @ master —
  <https://github.com/kovidgoyal/kitty/blob/master/kitty/options/definition.py>
  (tab_title_template default, progress_bar, program_status); rendered conf —
  <https://sw.kovidgoyal.net/kitty/conf/>; OSC 9;4 adoption —
  <https://github.com/kovidgoyal/kitty/issues/3679>; OSC 9 collision —
  <https://github.com/kovidgoyal/kitty/issues/8011> (fetched 2026-10-17)
- Ghostty: 1.2.0 release notes —
  <https://ghostty.org/docs/install/release-notes/1-2-0>; ConEmu OSC reference —
  <https://ghostty.org/docs/vt/osc/conemu>; status-bar discussion —
  <https://github.com/ghostty-org/ghostty/discussions/2421> (fetched 2026-10-17)
- Warp: block-model post —
  <https://www.warp.dev/blog/block-model-behind-warps-agentic-development-environment>;
  blocks docs — <https://docs.warp.dev/features/blocks>; hidden exit code —
  <https://github.com/warpdotdev/warp/issues/13027> (fetched 2026-10-17)
- VS Code: terminal basics — <https://code.visualstudio.com/docs/terminal/basics>;
  shell integration — <https://code.visualstudio.com/docs/terminal/shell-integration>;
  API reference — <https://code.visualstudio.com/api/references/vscode-api>
  (fetched 2026-10-17)
- tmux(1) — <https://man7.org/linux/man-pages/man1/tmux.1.html>; OSC 133
  passthrough gaps — <https://github.com/tmux/tmux/issues/5237>,
  <https://github.com/tmux/tmux/issues/3064> (fetched 2026-10-17)
- Windows Terminal shell-integration tutorial (OSC 133 + 9;9) —
  <https://learn.microsoft.com/en-us/windows/terminal/tutorials/shell-integration>;
  status-bar issue — <https://github.com/microsoft/terminal/issues/3459>
  (fetched 2026-10-17)
- ConEmu ANSI escape codes (OSC 9;4/9;9) —
  <https://conemu.github.io/en/AnsiEscapeCodes.html> (fetched 2026-10-17)
- Contour OSC 133 spec —
  <https://contour-terminal.org/vt-extensions/osc-133-shell-integration>
  (fetched 2026-10-17)
- Program Status Protocol (OSC 7501) —
  <https://mitchellh.com/writing/program-status-osc7501> (fetched 2026-10-17)
- Alacritty README — <https://github.com/alacritty/alacritty>; Zed terminal —
  <https://zed.dev/docs/terminal> (fetched 2026-10-17)
- DropTerm grounding: `src/status_bar.rs`, `src/app.rs`,
  `src/app/tab_actions.rs`, `src/terminal/engine.rs`,
  `src/terminal/suggest.rs`, `src/notify.rs` @ branch `ced2` (2026-10-17);
  `alacritty_terminal` 0.25.1 vendored `src/event.rs` (checked 2026-10-17)
- Prior records: [status-bar-ux](status-bar-ux.md) (2026-10-11),
  [terminal-de-integration](terminal-de-integration.md) (2026-10-02),
  [terminal-autocomplete](terminal-autocomplete.md) (2026-10-17)
