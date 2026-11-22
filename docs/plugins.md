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

The first scriptable surface is here: status bar segments
(`dropterm.status_segment`) render in the band - [their own
page](status-segments.md) covers the handler contract, the ctx table,
the refresh triggers, and the ordering key. Tab titles, registered
commands, and terminal lifecycle events arrive with their own modules,
each with its own doc page. Until then the bus is exercised by the
scripts themselves (`emit` between scripts), by the loader, and by the
segment queries; no terminal event reaches Lua yet.
