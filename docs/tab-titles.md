# Script tab titles

The tab strip's titles are not all built in: a Lua plugin can derive them, and its answers render as the tabs' names. A `plugins/titles.lua` like this

```lua
local dropterm = require('dropterm')

dropterm.tab_title(function(p)
  return (p.title ~= '' and p.title) or 'shell'
end)
```

switched on with the `script` title mode (the settings drawer's Tabs card, or the [`tab_title_mode` key](configuration.md#tab-titles)), gives every tab either its shell's own OSC title or `shell` - a plain shell that never titles reads as `shell`. The [plugins page](plugins.md) covers the runtime these scripts run on - the lazy host, hot reload, the timeout machinery - and the trust model stated plainly: a plugin is your own config executing as you, complete standard library, no sandbox.

## The provider contract

`dropterm.tab_title(handler)` registers the one provider. One provider globally, not one per tab: the handler derives every tab's title from the ctx it receives, and a later registration replaces an earlier one - one `dropterm:` log line each time, the replacement named as such (titles are stable, keybind-independent identity; a silent swap would read as breakage).

The handler receives `{ tab, title }`:

- `tab` - the tab's id as an opaque number: stable for the tab's life, the same id the [terminal events](plugins.md#terminal-events) carry, meaningful only to correlate;
- `title` - the raw OSC 0/2 report, the same string `shell` mode resolves from, the empty string when the tab has none (the provider decides what an empty title means).

It returns a string - the tab's new title, the empty string honored verbatim - or `nil`, no opinion: the tab falls back to its `Terminal N` default. A handler that raises costs one log line and the tab keeps the title it already had.

## When tabs are re-asked

Asks are change-gated, never per-tick: a tab is asked only when its latched title report moves (a shell setting or clearing its title), when the mode flips into `script`, when a tab spawns, and once at launch when the `plugins/` dir stood before the app started (the boot ask that wakes the host - the load's sync report is what names the provider). One batched dispatch carries every due tab - a mode flip re-asks the whole row in one trip, not one trip per tab - and with no provider registered, nobody is asked at all: `script` mode without a provider shows exactly the default titles.

Background tabs are asked on their own changes, never on the active tab's - the strip renders all tabs, so all titles must resolve, but each on its own material.

## The stale ladder

The strip never blocks on a script: the ask crosses to the host and the answer lands on a later tick, so a tab keeps its last resolved title until then. Per tab:

- no answer yet (or a `nil`) - the `Terminal N` default;
- an answer - that text, until the next answer;
- a raise or a trip past the soft bound (100 ms) - the prior title keeps rendering, one log line, and the next answer lands when it comes.

A reload clears the provider until a script re-registers; tabs keep their last titles across the gap - never a blank - and asks resume with the next registration.

## The rename still wins

A manual rename (the strip's inline rename, a double-click) stays authoritative in every mode, `script` included: title churn, fresh script answers, and mode flips never clobber it. Clearing the rename - committing an empty one - returns to the live mode's title at once: in `script` mode, the standing script answer, never a flicker of the default in between.
