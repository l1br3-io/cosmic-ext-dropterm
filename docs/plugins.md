# Plugins

A DropTerm plugin is a directory under the config dir's `plugins/`
(`~/.config/io.github.l1br3_io.dropterm/plugins/` on an installed
build; the dev identity uses its own dir) carrying one Lua script and
one optional manifest:

```
plugins/
  uptime-helpers/
    plugin.lua        # the script - what runs
    plugin.json       # the manifest - what the host advertises without running anything
```

The directory's name is the plugin's id: a lowercase slug - letters,
digits, hyphens, up to 64 characters. It is the persistent key
everywhere the plugin is named: log lines, settings grouping, and the
id a command binds under. A manifest `"name"` changes the display
name only, never the id.

The layout is the format. A loose `.lua` file dropped directly in
`plugins/` is skipped with one log line - plugins are directories now.
A non-slug directory (say `Uptime` or `my_plugin`) is skipped the same
way. Deeper nesting is ignored. A directory with a script but no
manifest still loads, exactly as scripts always did - it just
advertises nothing, and the log names the manifest it could have
declared.

## The trust model, stated plainly

A plugin is your own config executing as you, with the complete Lua
5.4 standard library - `io`, `os`, everything. There is no sandbox and
none is claimed: a plugin can read and write your files, spawn
processes, and block. Treat a plugin the way you treat a shell rc
file. The timeout machinery below is an escape hatch for runaway
scripts, not a security boundary.

The same plainness applies to manifests and dialogs: a plugin can
advertise commands and dialogs that look exactly like DropTerm's own.
Read what asks before you answer.

## The manifest

`plugin.json` declares what the host can say about the plugin without
ever running its script:

```json
{
  "name": "Uptime helpers",
  "commands": [
    { "id": "paste-uptime", "description": "Paste system uptime into the pane" }
  ],
  "settings": [
    { "key": "uptime-format", "type": "enum", "label": "Format",
      "default": "pretty", "enum": ["pretty", "seconds"],
      "description": "How the uptime renders" },
    { "key": "prefix", "type": "string", "label": "Prefix", "default": "" }
  ],
  "activation": []
}
```

Keys are kebab-case, the vocabulary closed. A key outside the
vocabulary costs one log line and is ignored; malformed JSON skips the
plugin with one line. Duplicate command ids or setting keys inside one
manifest keep the first and log once; a default of the wrong type for
its row falls to that type's builtin default (`false`, `""`, `0`, the
first enum value, `[]`, `{}`) with one line.

### Commands

`commands[].id` is a command slug (the same rules the runtime port
enforces - lowercase letters, digits, hyphens, 64 max); `description`
is required and shown in rows. A declared command's row renders
bindable (`script/<id>` on the Keybindings page) and palette-searchable
from launch - with no interpreter alive, if the plugin's activation
has not fired. Declared and runtime registrations merge one seat per
slug: the manifest wins the row when both stand, the runtime handler
is what dispatch runs.

### Settings rows

`settings[]` declares one row each: `key` (unique per plugin - the
field name in the store and in `ctx.settings`), `label`, `type`, and
`default`. `type` is one of the six:

| Type | Default | Editor on the Plugins page |
|---|---|---|
| `boolean` | `false` | a toggle |
| `string` | `""` | a text field |
| `number` | `0` | a number field |
| `enum` | the first `enum` value | a choice strip - the `enum` list is required |
| `array` | `[]` | a JSON text area |
| `object` | `{}` | a JSON text area |

Rows render on the settings window's Plugins page, grouped by plugin,
with no interpreter alive. A save writes the store and pushes the
values to the host; an invalid JSON edit or a wrong-typed value keeps
the prior value and shows the row's inline error. An enum default
outside its value list, or an enum row without the list, is a row
error - logged once, the row skipped.

### Activation

`activation` lists the extra events that load the script. Each
declared command implicitly derives `on_command:<id>` - pressing the
command loads the plugin first, then runs it (the first press pays the
load). Beyond that:

| Token | Load timing |
|---|---|
| *(none)* | lazy - nothing loads until a declared command's first press |
| `on_startup` | at host start, eagerly - today's behavior |
| `on_startup_finished` | at the first painted frame plus 50 ms - and if boot took longer than 50 ms to get there, at the first idle moment after (VS Code's measured rule) |

Unknown tokens log once and are ignored; a manifest resolving both
tokens takes the stronger (`on_startup`). Once a lazy plugin has
activated it stays active for the session - teardowns and reloads
recreate the interpreter with the current active set. A directory
without a manifest loads eagerly (the activation story predates it);
the log nudges toward declaring one.

## Settings in ctx

Every dispatch carries the dispatching plugin's current settings as a
Lua table on the ctx - command handlers, event handlers, segment
handlers, tab titles, panel views:

```lua
dropterm.command('paste-uptime', 'Paste system uptime', function(p)
  local prefix = p.settings.prefix       -- the declared row's name
  local fmt = p.settings['uptime-format']
  ...
end)
dropterm.on('x', function(payload, settings_ctx)
  print(settings_ctx.settings.prefix)
end)
```

Every declared row appears, filled with the stored value or the
row's default - never missing, an empty table for a plugin with no
rows. The event handler's payload stays exactly the documented shape;
the settings ride the second argument. Values persist across relaunch
in `plugins-settings.json` under the config dir - user config the
Plugins page writes (atomically), scripts never touch. Hand edits to
the file load at the next launch.

## The runtime

Inside a script, `require('dropterm')` yields the bus port:

```lua
local dropterm = require('dropterm')

dropterm.on('my-event', function(payload)
  print('saw my-event') -- a script's print lands in the dropterm log
end)

dropterm.emit('my-event', {}) -- reaches every registered handler
```

`on(event, handler)` registers; `emit(event, payload)` dispatches to
every handler registered for that event, in registration order, script
to script. Events nobody registered for dispatch to nobody, silently.
A handler that raises costs one log line and never stops its siblings.

Every observable step logs one line in the `dropterm: plugins:`
namespace - the host starting, each plugin activated or skipped with
its reason, each handler raise, each forwarded `print`.

### Terminal events

The terminal's own life reaches the bus. Seven event names deliver
from real sources - five from the engine's event stream, two from the
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
life and meaningful only to correlate events. Delivery is
fire-and-forget on the host thread. Terminal events flow only when the
`plugins/` dir stood at launch - a no-plugins session forwards
nothing, and its channel stays empty.

### Typed answers

Every slot's answer is one plain string, and a plain string stays
valid forever. The typed-answers contract adds one more shape: a
styled answer - an ordered list of flat spans, no nesting - which the
painters resolve through the style engine: each span's
classes, id, and inline declarations compete by the CSS cascade, the
glyph rides as an `icon` declaration:

```lua
dropterm.status_segment('net', function(ctx)
  return { kind = 'styled', spans = {
    { text = ' 1.2 MB/s', class = 'net metric', id = 'rx',
      style = { ['font-weight'] = 'bold', color = 'accent', icon = '↓' } },
  } }
end)
```

The `icon` declaration's glyph is prepended like the band's builtin
sigils (`{icon} {text}` - the band's ⎇ ⊞ ◉ shapes, not an icon
font); a glyph works:

```lua
{ text = ' 1.2 MB/s', style = { color = 'green', icon = '↓' } }
```

The vocabulary is closed - every key it declares:

| Key | Where | Meaning |
|---|---|---|
| `kind` | the answer table | optional; when present it must be `'styled'` - anything else costs the answer its one log line |
| `spans` | the answer table | the ordered span list; required for a styled answer, 16 spans at most |
| `side` | the answer table | the side word - required for a text answer exactly as before; a styled answer may omit it and takes `left` |
| `text` | inside each span | the span's text - the part the band renders |
| `class` | inside each span | optional; space-separated class names, the rules' `.class` parts |
| `id` | inside each span | optional; the span's `#id` |
| `style` | inside each span | optional; the inline declarations table - one kebab-case property with one value, the engine's closed vocabulary (`color`, `background`, `font-weight`, `font-style`, `text-decoration`, `icon`) |
| `color` | (removed) | was a span key; one `dropterm:` line and ignored - `style = { color = ... }` or a rule now |
| `icon` | (removed) | was a span key; one `dropterm:` line and ignored - the `icon` declaration now |

The `style` table's words resolve against the band's palette:
`accent` (the scheme's BrightBlue), `fg` (the pane's foreground),
`bg` (the worn scheme's background), and the ANSI words - `black`,
`red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`, and
their `bright_black`, `bright_red`, `bright_green`,
`bright_yellow`, `bright_blue`, `bright_magenta`, `bright_cyan`,
`bright_white` forms. An unknown word costs one `dropterm:` log line
and renders unstyled (the span still paints its text, in the row's
own color); a `#rrggbb` literal paints directly. The right side's
dim styling is the host builtin layer's `band.right` rule now - a
plugin overriding `color` there replaces it by ordinary cascade.
A plain-string answer renders exactly as it always did.

The evolution rule, stated plainly: the keyset grows additive-only -
a new key arrives as a release, an existing key never changes meaning.
Anything outside the vocabulary costs the answer exactly one log line
and the slot keeps its prior - never a raise into the terminal.

Interactive slots report back over the bus under one event name,
`slot-event`, with one payload shape - four keys, no others:
`surface`, `slot`, `element_id`, `kind`. The dialog surface produces
them today; the panel surfaces follow. The shape is pinned by test on
both sides.

## Dialogs

A script can open a modal dialog - the command palette's shape: a
full-surface scrim, a top-centered panel, the keyboard held while it
stands. One dialog at a time, one live seat: a second open replaces
the first, logged. An interpreter reload closes the standing dialog
on the next drain (its script died with its interpreter).

```lua
dropterm.dialog({
  title = 'Build finished',
  body  = { { text = 'release build done in 12.4s',
             style = { color = 'green' } } },
  buttons = {
    { id = 'restart', label = 'Restart', class = 'accent' },
    { id = 'dismiss', label = 'Dismiss' },
  },
})
dropterm.on('slot-event', function(p)
  if p.slot == 'dialog' and p.element_id == 'restart' then ... end
end)
```

The spec is closed - every key it declares:

| Key | Where | Meaning |
|---|---|---|
| `title` | the dialog table | the panel's title bar text |
| `body` | the dialog table | optional; the message, styled spans under the typed-answers rules |
| `buttons` | the dialog table | one to three, unique non-empty ids; a dialog with zero or four opens nothing |
| `id` | inside each button | the press event's `element_id`, and the button's `#id` for the cascade |
| `label` | inside each button | the visible label |
| `class` | inside each button | optional; the `accent` class marks the primary action - the host builtin rule on `dialog-button.accent` carries the look, and a plugin's own rule out-styling it replaces it by cascade |
| `accent` | (removed) | was a button flag; one `dropterm:` line and ignored - `class = 'accent'` now |

A wrong shape costs the call one `dropterm:` log line and opens
nothing. A button press emits `slot-event` with
`{ surface = 'dialog', slot = 'dialog', element_id = <the button's
id>, kind = 'press' }`; Escape, the scrim's press, and a focus escape
each emit `kind = 'dismiss'` with no element id. The dialog holds the
keyboard while it stands - Tab cycles the buttons, Enter presses the
focused one.

The trust model, stated as plainly as the plugins page's first: a
plugin dialog mimics app UI by design - read before binding. A
script's dialog can look exactly like DropTerm's own prompts, so read
what asks before you answer, the same way you read a shell rc before
you paste into it.

## Panel views

`dropterm.panel_view(view_id, config, handler)` docks a view into
the host-owned right rail - `panel_view` is the registration verb,
the `panel_rail_enabled` config key's container the target, the
`container` word naming it (`right-rail`, v1's one declared rail -
[the panel containers' page](panel-containers.md) covers the shell
side). The host asks:
opening the rail (and the standing cadence - the segments' interval,
plus a script's status-refresh pull) runs every registered view's
handler once, batched in one interpreter trip, and the answer paints
as the view's titled section. One seat per view id: a replacement
takes it, logged. An interpreter reload clears the seats; the rail
keeps its last painted content across the gap (the stale-degrade
story, now for panels).

```lua
dropterm.panel_view('tasks', {
  container = 'right-rail',
  title = 'Tasks',
}, function(ctx)
  return { kind = 'panel', rows = {
    { text = 'ship the spec review' },
    { text = 'walk the dialog', class = 'task done',
      style = { color = 'green', icon = '◉' } },
  } }
end)
dropterm.on('slot-event', function(p)
  if p.slot == 'tasks' and p.element_id == '0' then ... end
end)
```

The answer's shapes are the typed-answers contract's, additive: a
flat answer (a string or styled spans) paints as one row; the `rows`
shape paints one row per entry - either the spec example's span keys
directly on the row, or a `spans` array for many. Rows may carry an
optional numeric `id` key; presses carry it as the event's
`element_id`, the positional index (`0`, the first row) standing in
when it is absent.

A wrong shape costs the answer one `dropterm:` log line and the view
keeps its prior content - never a raise into the terminal. A `nil`
hides the view this round. The rail scrolls its own content; a long
view scrolls within it (no per-view scroll machinery in v1). The
rail is display + click only: the terminal keeps the keyboard while
the rail stands.

### Status segments and tab titles

`dropterm.status_segment` renders in the band - [their own
page](status-segments.md) covers the handler contract, the ctx table,
the refresh triggers, and the ordering key. `dropterm.tab_title`
derives the strip's names in `script` mode - [their own
page](tab-titles.md) covers the provider contract, the ask gating, and
the stale ladder.

### Commands and send_text

`dropterm.command(name, description, handler)` puts a script's own
verbs on the keyboard - [their own page](script-commands.md) covers
the slug rules, fire-and-forget dispatch, and `dropterm.send_text`.
Declared commands (the manifest section above) advertise without
running; a runtime registration from the loaded script is what
dispatch runs.

## Timeouts and reloads

Every unit of Lua work runs under a soft bound (100 ms) and a hard cap
(1 s), enforced by an instruction-counting debug hook. Past the soft
bound a dispatch is marked stale - one log line - and its result will
be discarded when result-carrying surfaces arrive. Past the hard cap
the whole interpreter is torn down and recreated: every registration
is lost by design and the scripts reload with the current active set
on the next work item. That reload is the runaway-script escape hatch;
a script that hangs itself on every load costs one interpreter cycle
per wake and never takes the terminal down with it.

The bounds cover Lua running on the host's main thread. A script
blocked inside a C call (say `io.read` on a pipe with no writer) or
spinning inside a coroutine it created executes no hook-checked
instructions and cannot be interrupted - full trust means exactly
that; close the app if you write one.

Editing anything in `plugins/` hot-reloads: the interpreter is
recreated whole (the only sound reload unit - a re-run file cannot
un-register its old handlers), all manifests and scripts rescanned at
once, and the advertising rows re-merge. Deleting a plugin's manifest
drops its declared rows on the next reload; deleting the directory
drops its registrations. Removing the directory itself leaves the host
idle and quiet.
