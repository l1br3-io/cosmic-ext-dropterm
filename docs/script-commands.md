# Script commands

A Lua plugin can register named commands, and those commands become keybindable exactly like the built-in verbs: a row in the Settings window's Keybindings page, a chord you bind there, and a press that runs the script's handler. A `plugins/uptime.lua` like this

```lua
local dropterm = require('dropterm')

dropterm.command('paste-uptime', 'Paste system uptime into the pane', function(p)
  dropterm.send_text(io.popen('uptime'):read())
end)
```

shows a `paste-uptime` row in settings; bind it to a chord, press the chord, and the uptime lands in the focused pane as if typed. The [plugins page](plugins.md) covers the runtime these scripts run on - the lazy host, hot reload, the timeout machinery - and the trust model stated plainly: a plugin is your own config executing as you, complete standard library, no sandbox.

## Registration

`dropterm.command(name, description, handler)` registers. The name is a slug - lowercase letters, digits, hyphens, at most 64 characters - because it becomes the command's persisted identity: a binding stores it under `script/<name>` in the [`custom_key_binds`](keybindings.md) map, exactly where a built-in verb's binding lives. Anything else rejects at the port with one `dropterm:` log line, never a raise. A duplicate registration replaces the earlier one in place - one live seat per name, the description and handler of the last registration - and the replacement logs one line, because a command silently changing under a bound chord would read as breakage.

The rows follow the live registry: the host reports the registered set at every load and reload, and the editor renders one row per live command under a **Script commands** section beside the built-ins. A reload that leaves a command unregistered removes its row; a binding stored for it stays in the keybinds file, inert - no row, no error, and a press dispatches to nobody with one log line. Deleting the script and reloading is the same story: the row goes, the stored binding stays harmless in the file.

## Binding and dispatch

Bind in settings, the same capture flow every built-in row uses: click the row's binding chip, press the chord, Escape cancels. A command ships unbound - only your binding can hold its chord - and conflicts resolve against every row's effective binding, script rows included. The chord persists under `script/<name>` and re-applies at every launch through the same overlay the built-ins ride.

A press resolves exactly as a built-in verb's does - the pane's key handler matches the chord and hands the verb up - and dispatch is fire-and-forget into the host: the handler runs there under the timeout machinery, the terminal never waits, and there is no reply to wait on. The handler receives `{ tab }` - the id of the pane the chord landed in, the same opaque number the [terminal events](plugins.md#terminal-events) carry. A handler that raises costs one log line; the terminal and sibling commands are unaffected. A command registered then reloaded away between binding and press - or never registered - dispatches to nobody with one log line.

Scripts never bind chords themselves: the user binds, in settings, only. There is no other invocation surface in v1 - no palette, no CLI verb - and an unbound command simply has no invocation until bound.

## `send_text` - the one verb

`dropterm.send_text(text)` is v1's one and only host-side verb (a larger verb set was deliberately declined - each verb is a permanent API commitment). It is bus-global: every handler holds it, not only command handlers - an [event](plugins.md#terminal-events) handler or a [segment](status-segments.md) handler can call it too, though only a dispatch that knows a tab can send anywhere:

- from a command handler, the write lands on the pane the command was invoked on;
- from a terminal-event handler, on the event's own tab;
- from anywhere with no ctx tab - a load-time call, a segment handler - the call drops with one log line.

The bytes are raw input, exactly the typed-key path: no bracketed-paste wrapping, no newline handling - a script wanting a return includes `\n` itself. The call returns immediately - fire-and-forget both directions, the script never blocks on the terminal - and the write lands on the next UI tick. A dead tab (the pane exited since the dispatch) drops the write with one log line, nothing written. There is no return value and no ordering guarantee beyond channel FIFO.
