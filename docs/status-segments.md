# Script status segments

The status band's segments are not all built in: a Lua plugin can register its own, and its answer renders beside the builtins. A `plugins/net.lua` like this

```lua
local dropterm = require('dropterm')

dropterm.status_segment('net', function(ctx)
  return { text = '▼ 1.2 MB/s', side = 'right' }
end)
```

puts `▼ 1.2 MB/s` at the end of the band's right side, after the first answer lands. The [plugins page](plugins.md) covers the runtime these scripts run on - the lazy host, hot reload, the timeout machinery - and the trust model stated plainly: a plugin is your own config executing as you, complete standard library, no sandbox.

## The handler contract

`dropterm.status_segment(name, handler)` registers one segment under an id of your choosing. The handler receives the ctx table (below) and returns a table

```lua
{ text = 'any text', side = 'left' }  -- or 'right'
```

or `nil` to render nothing this round. Plain text only in this build: the segment renders under the band's own styling for its side (the right side's dimmer alpha, like the builtins beside it), and no color, icon, or markup is interpreted. A handler that raises, returns an unusable shape, or names a side other than `left`/`right` costs exactly one `dropterm:` log line and keeps its previous text (see below); its siblings are unaffected. Registering the same id twice keeps one live segment - the later handler wins, the first position stays.

Where they render: script segments are their own group per side - after the whole builtin left cluster, or hugging the builtin right cluster - and under width pressure the group yields before any builtin segment does, last-registered first. In this build they are not interactive: no hover tint, no selection, no click-to-copy.

## When segments refresh

The band re-evaluates every registered handler in one interpreter trip when any of three triggers fires:

- **an input change** - the ctx table below changed since the last trip (you switched tabs, the cwd moved, a command finished);
- **the interval** - once a second regardless, so segments that poll their own state (`io.popen`, a file read) refresh anyway;
- **a pull** - a script emitting the reserved event `dropterm.emit('status-refresh', {})` asks for its own refresh after computing new state.

Each trip runs under the plugins runtime's soft bound (100 ms) and hard cap (1 s). A trip past the soft bound is stale: its answers are discarded and every segment keeps what it already shows. Past the hard cap the interpreter reloads per the plugins runtime - registrations re-run, the band keeps its text across the gap, and no answer that never lands ever blanks a segment that had one. That is the stale-degrade story in one line: **a segment keeps its last resolved text until a fresh answer arrives** - a raising or timing-out handler changes nothing on the band, and a `nil` return is a fresh answer of nothing (the segment hides).

## The ctx table

The handler's `ctx` mirrors everything the band already gathered for the tick - no engine state is read on a script's behalf. Absent facts are absent keys:

`home`, `cwd`, `reported_cwd` (`{host, path}`, the shell's OSC 7 report), `hostname`, `shell`, `profile_name`, `zoom_adj`, `zoom_step_mul_100`, `foreground`, `branch`, `grid` (`{cols, rows}`), `spawn_grid`, `tab_position` (`{index, total}`), `clock`, `running_elapsed` (seconds), `frozen` (`{name, duration_seconds}`), `exit_status`, `scrolled_back`, `pasted`, `progress` (`{state, value}`), `program_status` (`{state, progress}`), `ssh_destination`, `missed_activity`.

```lua
dropterm.status_segment('dir', function(ctx)
  if not ctx.cwd then return nil end
  return { text = 'in ' .. ctx.cwd, side = 'left' }
end)
```

## Ordering and toggles

The `status_script_segments` config key ([configuration](configuration.md#status-bar)) shapes what renders: unset, every registered segment renders in registration order - the order the plugin files load in, filename order. Set to a list of ids (`Some(["net", "git"])`), exactly the listed segments render, in the list's order; leaving an id out hides its segment without unregistering it. A listed id nothing registered costs one quiet log line, at most. Changes apply live.
