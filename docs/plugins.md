# Plugins

DropTerm runs your own Lua 5.4 scripts from the config dir's `plugins/`
subdirectory (`~/.config/io.github.l1br3_io.dropterm/plugins/` on an
installed build; the dev identity uses its own dir): one `.lua` file per
script, flat - subdirectories and other extensions are ignored. Scripts
load lazily: the host thread spawns on the first work item, and with no
`plugins/` dir nothing Lua ever happens. Two things wake the host: the
status band's boot query - one query within the first second when the
dir stood at launch, so [script segments](status-segments.md) render
without a touch - and the dir itself: editing a script or dropping one
in wakes the host, which scans and loads everything it finds, in
filename order, so registrations across scripts are deterministic.

The dir is watched from launch: create it (and put your scripts in it)
before launching, or restart after creating it - a dir that appears
mid-run is seen at the next launch.

## The trust model, stated plainly

A plugin is your own config executing as you, with the complete Lua 5.4
standard library - `io`, `os`, everything. There is no sandbox and none
is claimed: a plugin can read and write your files, spawn processes, and
block. Treat a plugin the way you treat a shell rc file. The timeout
machinery below is an escape hatch for runaway scripts, not a security
boundary.

## The event bus

Inside a script, `require('dropterm')` yields the bus port:

```lua
local dropterm = require('dropterm')

dropterm.on('my-event', function(payload)
  print('saw my-event') -- a script's print lands in the dropterm log
end)

dropterm.emit('my-event', {}) -- reaches every registered handler
```

`on(event, handler)` registers; `emit(event, payload)` dispatches to every
handler registered for that event, in registration order, script to
script. Events nobody registered for dispatch to nobody, silently. A
handler that raises costs one log line and never stops its siblings.

Every observable step logs one line in the `dropterm: plugins:` namespace
- the host starting, each script loaded or skipped with its reason, each
handler raise, each forwarded `print`.

## Terminal events

The terminal's own life reaches the bus. Seven event names deliver from
real sources - five from the engine's event stream, two from the
window's focus - each with a minimal payload:

| Event | Payload | Fires |
|---|---|---|
| `bell` | `{ tab }` | the terminal bell (`printf '\a'`) |
| `title` | `{ tab, title }` | an OSC 0/2 title report - change-gated: only when the title actually moves, so a shell re-reporting the same title on every prompt stays quiet |
| `title-reset` | `{ tab }` | the title cleared (an empty OSC 0/2 report) - only when one was set |
| `child-exit` | `{ tab, code }` | a command finished (the OSC 133;D report; `code` is its exit status) |
| `exit` | `{ tab }` | the tab's shell exited, as the pane tears down |
| `focus-gained` | `{ tab }` | the DropTerm window became keyboard-active - the active tab |
| `focus-lost` | `{ tab }` | it stopped being - the tab that lost |

`tab` is the pane's tab id as an opaque number: stable for the pane's
life and meaningful only to correlate events (a `title` and the
`child-exit` that followed carry the same `tab`). Delivery is
fire-and-forget on the host thread - no ordering guarantee beyond
per-channel FIFO, no feedback into the terminal.

Terminal events flow only when the `plugins/` dir stood at launch (the
same gate as everything else here: a no-plugins session forwards
nothing, and its channel stays empty). What never reaches the bus: the
per-frame damage and wake traffic, and the machinery arms (clipboard
store/load, color requests, PTY writes) - they are internals, not
script events. Agent notifications join as event names with their own
module later.

## Typed answers

Every slot's answer is one plain string, and a plain string stays valid
forever. The typed-answers contract adds one more shape: a styled
answer - an ordered list of flat spans, no nesting - which the band
paints span by span, each span's color and icon rendered. Any
segment handler may return one:

```lua
dropterm.status_segment('net', function(ctx)
  return { kind = 'styled', spans = {
    { icon = 'arrow-down', text = ' 1.2 MB/s', color = 'accent' },
  } }
end)
```

A span's `icon` is a glyph string, prepended like the band's builtin
sigils (`{icon} {text}` - the band's ⎇ ⊞ ◉ shapes, not an icon
font); a glyph works:

```lua
{ icon = '↓', text = ' 1.2 MB/s', color = 'green' }
```

The vocabulary is closed - every key it declares:

| Key | Where | Meaning |
|---|---|---|
| `kind` | the answer table | optional; when present it must be `'styled'` - anything else costs the answer its one log line |
| `spans` | the answer table | the ordered span list; required for a styled answer, 16 spans at most |
| `side` | the answer table | the side word - required for a text answer exactly as before; a styled answer may omit it and takes `left` |
| `text` | inside each span | the span's text - the part the band renders |
| `color` | inside each span | optional; a color word or a `#rrggbb` literal |
| `icon` | inside each span | optional; a glyph string, prepended like the band's builtin sigils |

A `color` word resolves against the band's palette: `accent` (the
scheme's BrightBlue), `fg` (the pane's foreground), and the ANSI
words - `black`, `red`, `green`, `yellow`, `blue`, `magenta`,
`cyan`, `white`, and their `bright_black`, `bright_red`,
`bright_green`, `bright_yellow`, `bright_blue`, `bright_magenta`,
`bright_cyan`, `bright_white` forms. An unknown word costs one
`dropterm:` log line and renders unstyled (the span still paints its
text, in the row's own color); a `#rrggbb` literal paints directly.
The right side's dim styling is the band's, applied on top - never
the script's. A plain-string answer renders exactly as it always
did.

The evolution rule, stated plainly: the keyset grows additive-only -
a new key arrives as a release, an existing key never changes meaning.
A key outside the vocabulary, a value of the wrong type, or a
malformed `#rrggbb` costs the answer exactly one `dropterm:` log line
and the segment keeps its prior text - never a raise into the
terminal.

Interactive slots report back over the bus under one event name,
`slot-event`, with one payload shape - four keys, no others:

| Key | Meaning |
|---|---|
| `surface` | the slot surface that produced the event (`status`, `dialog`, ...) - the set grows as surfaces land |
| `slot` | the slot's registration id |
| `element_id` | the element inside the slot's content the interaction hit |
| `kind` | the interaction kind - each surface grows its own closed set |

No surface produces one yet; the dialog module arrives with its own
page. The shape is pinned by test on both sides: the Rust payload
serializes exactly these four keys, and a registered handler receives
exactly these four - nothing more.

## Timeouts and reloads

Every unit of Lua work runs under a soft bound (100 ms) and a hard cap
(1 s), enforced by an instruction-counting debug hook. Past the soft
bound a dispatch is marked stale - one log line - and its result will be
discarded when result-carrying surfaces arrive. Past the hard cap the
whole interpreter is torn down and recreated: every registration is lost
by design and the scripts reload lazily, on the next work item. That
reload is the runaway-script escape hatch; a script that hangs itself on
every load costs one interpreter cycle per wake and never takes the
terminal down with it.

The bounds cover Lua running on the host's main thread. A script blocked
inside a C call (say `io.read` on a pipe with no writer) or spinning
inside a coroutine it created executes no hook-checked instructions and
cannot be interrupted - full trust means exactly that; close the app if
you write one.

Editing anything in `plugins/` hot-reloads: the interpreter is recreated
whole (the only sound reload unit - a re-run file cannot un-register its
old handlers), all scripts rescanned at once. Deleting a script removes
its registrations on the next reload; removing the directory itself
leaves the host idle and quiet.

## What is not here yet

Status bar segments (`dropterm.status_segment`) render in the band -
[their own page](status-segments.md) covers the handler contract, the
ctx table, the refresh triggers, and the ordering key - the terminal
events above deliver the terminal's life, script tab titles
(`dropterm.tab_title`) derive the strip's names in `script` mode -
[their own page](tab-titles.md) covers the provider contract, the ask
gating, and the stale ladder - and registered commands
(`dropterm.command`) put a script's own verbs on the keyboard -
[their own page](script-commands.md) covers the slug rules, the
settings rows, fire-and-forget dispatch, and `dropterm.send_text`.
Agent notifications arrive with their own module, with its own doc
page.
