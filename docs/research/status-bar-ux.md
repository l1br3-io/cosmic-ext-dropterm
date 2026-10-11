# Research: what does the best status bar for a terminal actually look like?

Date: 2026-10-11
Question: *What is the best UX for a terminal emulator's built-in status bar — what
do tmux, iTerm2, WezTerm, zellij, and the no-bar terminals actually show, how do they
update, theme, and interact — and what should DropTerm's 28px glance bar keep, change,
add, and drop as a result?*

## TL;DR

**The status-bar world splits into two camps, and DropTerm's design sits deliberately
between them: the multiplexer camp (tmux, zellij, byobu) shows session identity and
window state in a poll-refreshed strip, while the emulator camp (kitty, foot,
alacritty, Windows Terminal, ghostty) ships no status bar at all because — as the
Windows Terminal maintainers put it — "everything that we can conceivably put in the
status bar is already on the screen somewhere"
([microsoft/terminal #3459](https://github.com/microsoft/terminal/issues/3459)).**
iTerm2 is the one famous GUI terminal that committed to a status bar, and it is the
closest model for DropTerm: an event-fed, per-component strip of *shell-context*
segments (cwd, git state, job name) plus a clock, with priority-based dropping when
space runs out and clicks that copy values or run actions. DropTerm should **keep**
its segment set and click-to-copy model (both have strong iTerm2 precedent), **keep**
its event-piggyback update model (tmux's 15 s default poll and WezTerm's 1 s timer
exist because their bars are always-on for hours; a drop-down landed for seconds
needs refresh-on-show, not a polling loop), **change** the zoom percent and grid size
segments to conditional state indicators (tmux shows the `Z` flag only when zoomed),
**add** hover tooltips with the full un-elided value (VS Code and iTerm2 both
hover-annotate statuses) and an iTerm2-style priority order for dropping segments on
a narrow surface, and **drop nothing** — but resist adding CPU, memory, or network
segments, which only iTerm2 offers and which answer the wrong question for a glance
("what/where am I", not "how is my machine").

## Findings

### 1. tmux: the canonical bar is session identity + window state + time

tmux's status line defaults to `[session-name]` on the left (`status-left` default
`"[#{session_name}] "`), the window list in the middle, and on the right the pane
title in quotes, the time, and the date (default `status-right`
`"\"#{=21:pane_title}\" %H:%M %d-%b-%y"`)
([tmux.1 STATUS LINE OPTIONS](https://github.com/tmux/tmux/blob/master/tmux.1),
[options-table.c](https://github.com/tmux/tmux/blob/master/options-table.c)). The
rationale is visible in the option set: `status-justify` positions the window list,
`status-position` is only top|bottom, and the left/right lengths (10 and 40 by
default) bound fixed identity text while the middle list flexes. Windows carry alert
flags rendered as single characters in `#F` — read from source: `#` activity, `!`
bell, `~` silence, `*` current, `-` last, `M` marked, `Z` zoomed
([window.c `window_printable_flags`](https://github.com/tmux/tmux/blob/master/window.c))
— each with its own style option (`window-status-activity-style`,
`window-status-bell-style`), so a window needing attention *changes appearance in
place* rather than growing a new segment. The window separator defaults to a single
space (`window-status-separator`) — the canonical bar is plain gaps, not glyphs.
Updating is a poll: "Update the status line every *interval* seconds. By default,
updates will occur every 15 seconds. A setting of zero disables redrawing at
interval" ([tmux.1](https://github.com/tmux/tmux/blob/master/tmux.1), `status-interval`).
Clicking is a first-class, scriptable surface: mouse events over the bar route to
dedicated key tables — `Status`, `StatusLeft`, `StatusRight`, `StatusDefault` — with
the target window/pane available to the binding
([tmux.1 key tables](https://github.com/tmux/tmux/blob/master/tmux.1)). What tmux
deliberately leaves out: any process, path, or VCS information — the bar shows only
what tmux itself knows (sessions, windows, alerts, time), the exact division of labor
the no-bar emulators appeal to.

### 2. iTerm2: the emulator that committed to a status bar — and the closest model for DropTerm

iTerm2's docs open with a purpose statement that could be DropTerm's: "The purpose of
the status bar is to show up-to-date information about the environment you're working
in and to offer useful interactions where appropriate"
([iTerm2 Status Bar docs](https://iterm2.com/documentation-status-bar.html)). Its
component taxonomy is instructive — **System Resource Monitors** (battery, CPU,
memory, network throughput, each a graph over time), **Shell Information** (Current
Directory, Host Name, User Name, Job Name, git state), **Miscellaneous** (Clock,
Custom Action, Progress Indicator), and **UI Affordances** (Composer, Search, Filter,
Action, Snippet buttons) ([same page](https://iterm2.com/documentation-status-bar.html)).
The shell-information cluster is what DropTerm duplicates today; iTerm2 gets
cwd/host/user/git *from shell integration variables* rather than /proc — DropTerm's
/proc approach works without user setup, a real advantage for a drop-and-go tool.
Even the most invested status bar treats VCS as a poll: the git component "periodically
runs git commands in the current directory" on a configurable interval, in a
write-sandbox ([same page](https://iterm2.com/documentation-status-bar.html)).
Configuration is drag-and-drop with per-component overrides (background/text color,
min/max width) and a **priority** field: "When there isn't enough space to show all
the components, they will be removed from lowest to highest priority"
([same page](https://iterm2.com/documentation-status-bar.html)) — the single most
transferable idea for a drop-down bar whose width varies with the surface. Two layout
algorithms are offered: "tight packing" (spring constants, highest density but
components "jumping around whenever a value changes") and "stable positioning" (fixed
per-component widths, the default) — iTerm2's own default chooses stability over
density. Interactions: right-click a component for a context menu including **Copy**
"when the component has a value worth copying"; click git state for
Commit/Stash/Log/Push actions; click cwd to pick a recent directory and `cd` to it
([same page](https://iterm2.com/documentation-status-bar.html)). Visuals are
theme-derived defaults with optional overrides plus "Auto-Rainbow" (distinct hues per
component) — optional decoration on a sober default — and the docs warn that custom
fonts may not fit ("Not all fonts will look good because of size and alignment
issues").

### 3. WezTerm: a right-status area, not a bar — event-scheduled with a floor of 1 s

WezTerm has no built-in status bar; it exposes *areas*: the right end of the tab bar,
fed entirely by user Lua via `window:set_right_status`, "displayed right-aligned and
will be clipped from the left edge to fit in the available space"
([set_right_status](https://wezterm.org/config/lua/window/set_right_status.html)).
The `update-right-status` event "is emitted periodically (based on the interval
specified by the `status_update_interval` configuration value)", whose default is
**1000 ms**, with "only a single instance of this event … outstanding"
([update-right-status](https://wezterm.org/config/lua/window-events/update-right-status.html),
[status_update_interval](https://wezterm.org/config/lua/config/status_update_interval.html)).
The official example shows what the community pattern became: cwd and hostname from
the pane (picking up the remote host via **OSC 7** shell integration), a strftime
date, and battery percent, joined with PowerLine arrow glyphs
([set_right_status](https://wezterm.org/config/lua/window/set_right_status.html)).
Note the data source: WezTerm trusts OSC 7 emissions from the shell over probing the
filesystem — the "the shell tells me" stance of the no-bar emulators, with a place to
draw it. The event was renamed `update-status` in 2022 because "right status" was too
narrow a concept
([update-right-status deprecation note](https://wezterm.org/config/lua/window-events/update-right-status.html)).

### 4. zellij: the bar as tutorial — mode indicator and contextual hints

Zellij's bottom bar is not an information display but a *discoverability device*:
"You don't need to read a book, keep cheat-sheets or memorize keybindings to use
Zellij. You just start it and it tells you what to do in the bottom status-bar"
([zellij.dev/news/sixel-search-statusbar](https://zellij.dev/news/sixel-search-statusbar),
Zellij 0.31.0 announcement). That release made the hints follow user keybindings
because the hard-coded hint text "naturally confused" users whose custom bindings no
longer matched the screen ([same page](https://zellij.dev/news/sixel-search-statusbar))
— the bar's content is contractually bound to the UI's actual behavior, a principle
worth copying: segments must never describe state that isn't real. When the UI "takes
up too much space," zellij's documented answer is the compact layout (with an
optional tooltip toggle to summon keybinding hints on demand)
([zellij FAQ](https://zellij.dev/documentation/faq.html)) — even the discoverability
bar is treated as reclaimable vertical space by power users.

### 5. The no-bar emulators: kitty, foot, alacritty, Windows Terminal, ghostty

- **kitty** ships no status bar. Its in-screen furniture is a *tab bar* —
  `tab_bar_style` offers fade, slant, separator, powerline, custom (a user Python
  function), or `hidden` with a `select_tab` mapping as the fallback — plus window
  titles ([kitty conf: The tab bar](https://sw.kovidgoyal.net/kitty/conf/), section
  "The tab bar"). The *absence* is fact; the "deliberate stance" is UNVERIFIED
  (no kitty docs/FAQ entry states it).
- **foot** has no status-bar option anywhere in its shipped `foot.ini`
  ([foot.ini @ master](https://codeberg.org/dnkl/foot/raw/branch/master/foot.ini),
  checked 2026-10-11 — zero matches for "status").
- **alacritty** states its philosophy in the README: "By integrating with other
  applications, rather than reimplementing their functionality, it manages to provide
  a flexible set of features with high performance"
  ([alacritty README](https://github.com/alacritty/alacritty/blob/master/README.md));
  a status-bar request was closed with no maintainer engagement
  ([alacritty #4779](https://github.com/alacritty/alacritty/issues/4779)).
- **Windows Terminal** has an open 6-year-old issue for an optional status bar, still
  unimplemented. It contains the strongest articulation of the emulator camp: "It
  must be optional. Many users *won't* want a status bar" (issue OP), DHowett-MSFT's
  "everything that we can conceivably put in the status bar is already on the screen
  somewhere", and egmontkob's "Terminals don't *understand* a single bit of semantics
  about what is happening inside… where will you get the data to be displayed there?"
  ([microsoft/terminal #3459](https://github.com/microsoft/terminal/issues/3459)).
  Contributor Jaykul adds the sharpest criterion: the only *really* worth-having
  status-bar items are things "the shell doesn't know about, like the zoom level, or
  that change during the execution of a command" ([same thread](https://github.com/microsoft/terminal/issues/3459)).
  j4james notes the VT320 had a hardware status line that *applications* could
  address (DECSASD/DECSSDT) — the historical answer to "whose status is it?" (same thread).
- **ghostty** has none as of 2026: its config reference contains no status-bar option
  ([ghostty config docs](https://ghostty.org/docs/config), searched 2026-10-11); a
  community "Statusbar widget" discussion collected widgets (time, size, cursor pos,
  modifier state, pending key, CPU, memory) with Mitchell Hashimoto deferring widgets
  to a future plugin system ([ghostty discussion #2421](https://github.com/ghostty-org/ghostty/discussions/2421));
  and a full implementation PR was closed unmerged in December 2025
  ([ghostty PR #10088](https://github.com/ghostty-org/ghostty/pull/10088)). Notably,
  ghostty *does* render a hover tooltip/status strip for hyperlink previews
  ([ghostty #2653](https://github.com/ghostty-org/ghostty/issues/2653)) — even the
  purist emulators accept transient overlay strips; they reject persistent ones.

The no-bar camp's objection is not "status bars are bad" but "a *terminal emulator*
has no reliable way to know cwd/branch/process semantics, and heuristics are a trap."
DropTerm escapes that trap the same way iTerm2 does: it owns the PTY lifecycle and
reads the OS directly, so the data-quality objection does not apply.

### 6. byobu: the maximalist data bar, and why it's a menu

byobu is built entirely around the tmux/screen status line: its status scripts
directory ships ~45 toggleable notification segments — arch, battery, cpu_count,
cpu_freq, cpu_temp, disk, disk_io, distro, entropy, fan_speed, hostname, ip_address,
load_average, logo, mail, memory, menu, network, packages, processes, raid,
reboot_required, release, services, session, swap, time, time_binary, time_utc,
trash, updates_available, uptime, users, whoami, wifi_quality
([byobu repo usr/lib/byobu](https://github.com/dustinkirkland/byobu/tree/master/usr/lib/byobu)).
Every segment is individually toggleable and defaults are conservative — byobu's
lesson is the opposite of iTerm2's: when anyone can add any data, the deliverable
becomes a *picker over an empty default*, not a designed bar. DropTerm's 8 fixed
segments with a settings toggle per segment is the sane middle.

### 7. The powerline paradigm: segments, not sentences

Powerline is "a statusline plugin for vim, and provides statuslines and prompts for
several other applications, including zsh, bash, fish, tmux, IPython, Awesome, i3 and
Qtile" ([powerline README](https://github.com/powerline/powerline/blob/develop/README.rst))
— one visual grammar ported across a dozen tools is why it became the lingua franca.
Its grammar: the bar is a row of **sections, each displaying one piece of
information** (vim-airline: "That line consists of several sections, each one
displaying some piece of information"
([vim-airline README](https://github.com/vim-airline/vim-airline/blob/master/README.md))),
delineated by arrow glyphs with alternating background hues, with **mode display**
(normal/insert/visual) as the leading segment and git branch + dirty state as the
signature content. The modern shell-prompt descendant, starship — "minimal,
blazing-fast, and infinitely customizable prompt for any shell… Rust-Powered"
([starship.rs](https://starship.rs/)) — drops the arrows but keeps the segment model.
The paradigm's real contribution to a status bar is not the glyphs: it is *fixed-width
token segments ordered most-volatile→most-stable* (mode → git → path → clock), which
is exactly DropTerm's label-less gap-separated design.

### 8. What Warp and VS Code do instead: status attached to content, not to the window

VS Code's integrated terminal has no status bar; per-terminal status lives as icons
on the tab label: "Icons may appear to the right of the terminal title on the tab
label when a terminal's status changes… Hover the icon to read status information,
which may contain actions"
([VS Code terminal docs](https://code.visualstudio.com/docs/terminal/basics)) —
status *events* become hover-annotated badges on an existing list. Warp abandons the
flat scrollbuffer entirely: "Blocks group your command and command output," rendered
bottom-up with failed commands highlighted in red
([Warp docs: Block basics](https://docs.warp.dev/terminal/blocks/block-basics)) —
status becomes a property of the command, not of the window. Both answer the same
glance question DropTerm asks, and both choose *transient, attached* affordances over
a persistent strip.

## Recommendations for DropTerm

The famous implementations disagree on almost everything except one thing: a status
bar earns its pixels by answering state the surrounding UI cannot show. For a
drop-down terminal — landed for seconds, no window list, no title bar in view — that
is "what is running, where am I, how is it dressed," precisely DropTerm's purpose.
Verdict, per the six axes:

1. **Segment set — keep, with two changes.** The cwd / git branch / process name trio
   matches iTerm2's Shell Information cluster and the powerline signature; profile
   name is DropTerm's analog of tmux's session name. **Change: zoom percent and grid
   size should be conditional state indicators, not constant segments** — tmux
   renders the zoomed flag `Z` only when true, and the strongest pro-status argument
   (Jaykul in microsoft/terminal #3459) reserves the bar for state "the shell doesn't
   know about, or that changes during execution." A constant "100%" wastes the
   scarcest resource on a glance bar: distinguishability. Clock stays (tmux, iTerm2,
   byobu all keep it; it anchors the volatile end). **Deliberately do not add**
   CPU/memory/network: only iTerm2 offers these, as graphs, and they answer "how is
   my machine," not "what am I doing." Host/user stay out: correct, since DropTerm is
   local-first and iTerm2 needs them only for SSH context.
2. **Ordering and clustering — adopt iTerm2's priority model.** tmux's convention is
   fixed identity at the edges, the volatile list in the middle; the powerline
   convention orders most-volatile first. DropTerm's "most-volatile-near-cursor" is
   compatible with both — put the process disc and cwd on the grab-bar side (the
   landing point of the eye as the surface slides down), git/profile/clock trailing.
   Add per-segment priorities so that on a narrow surface segments are *dropped
   lowest-priority-first* rather than compressing uniformly — iTerm2's documented
   removal order, with its "stable positioning" default as the tie-breaker (fixed
   widths over jumping values).
3. **Interaction — keep click-to-copy, add hover tooltips.** iTerm2 validates
   copy-from-the-bar ("a Copy item that copies the relevant information"; DropTerm's
   copy-full-un-elided-value refinement is strictly better than copying elided text),
   and shows the action tier is possible later (click git → run git). Keep
   left-click = copy as the one predictable verb — iTerm2's left-click-runs-git is
   powerful but violates glance-bar predictability — and add a hover tooltip showing
   the full value for elided segments, following VS Code ("Hover the icon to read
   status information, which may contain actions") and ghostty's hover strip
   precedent. A cursor change or underline on hover is the cheap "this is clickable"
   affordance.
4. **Update cadence — keep event-piggyback, add refresh-on-show.** tmux polls at a
   15 s default (zero disables), WezTerm schedules at 1000 ms with a
   single-outstanding guarantee, iTerm2 polls git on a configurable interval — all
   because their bars must be honest during *continuous* presence. A drop-down's bar
   is only ever glanced at fresh after a toggle, so: refresh everything on the show
   event, refresh process/cwd at the existing piggyback points while visible, tick
   the clock only while visible. Do not add a standing timer; it buys a stale-data
   window on re-appear, which refresh-on-show fixes anyway.
5. **Visual language — keep theme-derived colors and plain gaps.** The evidence is
   genuinely split (tmux defaults to a single-space separator; WezTerm's examples and
   kitty's powerline tab style embrace arrow glyphs; iTerm2 offers Auto-Rainbow), so
   this is a taste call with a tie-breaker: a 28px bar at 12px font has no room for
   glyph-only separators, and "theme-derived colors, no new hues" is the iTerm2
   default rather than the Auto-Rainbow exception. Stay with gaps; if a segment ever
   needs alert styling, copy tmux's mechanism (recolor that segment, e.g. the process
   disc) instead of adding glyphs.
6. **When not to show it — never auto-hide, but degrade gracefully.** tmux's
   `status off`, zellij's compact layout, and kitty's `hidden` tab bar all treat the
   bar as user-removable vertical space, and ghostty's discussion expected a
   toggle-keybind. For DropTerm the bar *is* the edge affordance (28px adjacent to
   the grab bar), so hiding it would orphan the grab bar — instead follow the
   multiplexer lesson in reverse: every segment toggleable (already true),
   drop-by-priority when narrow (rec 2), one row tall. No further progressive
   disclosure; the drop-down lifecycle is itself the disclosure mechanism — the bar
   exists only when the user summons it.

## Sources

- tmux @ master: [tmux.1](https://github.com/tmux/tmux/blob/master/tmux.1)
  (STATUS LINE OPTIONS, key tables, FORMATS);
  [options-table.c](https://github.com/tmux/tmux/blob/master/options-table.c)
  (default `status-left`/`status-right` strings);
  [window.c](https://github.com/tmux/tmux/blob/master/window.c)
  (`window_printable_flags`: `# ! ~ * - M O Z`)
- iTerm2 Status Bar documentation — <https://iterm2.com/documentation-status-bar.html>
- WezTerm: [set_right_status](https://wezterm.org/config/lua/window/set_right_status.html),
  [update-right-status](https://wezterm.org/config/lua/window-events/update-right-status.html),
  [status_update_interval](https://wezterm.org/config/lua/config/status_update_interval.html)
- zellij: 0.31.0 announcement —
  <https://zellij.dev/news/sixel-search-statusbar>; FAQ (compact layout) —
  <https://zellij.dev/documentation/faq.html>
- kitty configuration reference, section "The tab bar" —
  <https://sw.kovidgoyal.net/kitty/conf/> (no status-bar option exists anywhere in
  kitty's docs, checked 2026-10-11)
- foot: shipped foot.ini @ master —
  <https://codeberg.org/dnkl/foot/raw/branch/master/foot.ini> (no "status" match)
- alacritty: [README](https://github.com/alacritty/alacritty/blob/master/README.md);
  closed status-bar request — <https://github.com/alacritty/alacritty/issues/4779>
- microsoft/terminal #3459 "Add an optional status bar to the bottom" (open since
  2019; maintainer quotes on data availability, must-be-optional, VT320 status line)
  — <https://github.com/microsoft/terminal/issues/3459>
- ghostty: config reference — <https://ghostty.org/docs/config>; Statusbar widget
  discussion — <https://github.com/ghostty-org/ghostty/discussions/2421>; closed
  unmerged implementation PR — <https://github.com/ghostty-org/ghostty/pull/10088>;
  hyperlink hover strip — <https://github.com/ghostty-org/ghostty/issues/2653>
- byobu status notification scripts —
  <https://github.com/dustinkirkland/byobu/tree/master/usr/lib/byobu>
- powerline README —
  <https://github.com/powerline/powerline/blob/develop/README.rst>; vim-airline
  README — <https://github.com/vim-airline/vim-airline/blob/master/README.md>;
  starship — <https://starship.rs/>
- VS Code terminal docs — <https://code.visualstudio.com/docs/terminal/basics>;
  Warp docs, Block basics — <https://docs.warp.dev/terminal/blocks/block-basics>
