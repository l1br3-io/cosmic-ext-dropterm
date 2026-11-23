# Lua declarative UI precedents (WezTerm, awesome, Zellij, kitty, Neovim, RmlUi, Elixir gpui)

**Question:** Which existing applications already let user scripts draw UI declaratively, and which of their designs should DropTerm adapt for its planned "Lua tables → GPUI widgets" plugin schema?

**Date:** 2026-11-24

**TL;DR:** All seven surveyed precedents converge on the snapshot/event
contract DropTerm already chose — a script-side declarative description
the host interprets, with interaction returning as host-routed, typed,
element-identified events — and none of them validates building a JSX-like
markup engine first. DropTerm should start tiny: a closed vocabulary
(spans, rows, icons, one modal surface) shaped like WezTerm's flat items
for cheap surfaces and awesome's nesting-as-layout for trees, closed
keysets that evolve additively (Neovim's contract), update-by-id for
refreshing surfaces, host hit-testing that delivers click events by
element id (never coordinates), the script owning all state (Elixir
gpui's rule), and a headless test display from day one. The component
system (RmlUi's end-state) stays deferred until real plugins pull it.

## Findings

### WezTerm: Lua returns flat styled-text item lists, evaluated synchronously on the GUI thread

WezTerm never hands Lua a widget tree — every script-facing surface that shapes UI returns either a plain string or a *flat list of `FormatItem` tables* that WezTerm converts into styled text. `format-tab-title` is registered via `wezterm.on('format-tab-title', function(tab, tabs, panes, config, hover, max_width)` and may return "a string, holding the text to use for the tab title" or "a table holding FormatItems as used in the wezterm.format function" ([format-tab-title](https://wezterm.org/config/lua/window-events/format-tab-title.html)). The canonical return shape:

```lua
return {
  { Background = { Color = 'blue' } },
  { Text = ' ' .. title .. ' ' },
}
```

The full `FormatItem` vocabulary (from the docs source on main): `{Text=...}`, `{Attribute={Underline='Single'|'Double'|'Curly'|'Dotted'|'Dashed', Intensity='Normal'|'Bold'|'Half', Italic=true/false}}`, `{Foreground={AnsiColor='Fuchsia'}|{Color='#ffffff'}}`, `{Background=...}`, and the plain string `'ResetAttributes'` ([wezterm.format source](https://raw.githubusercontent.com/wezterm/wezterm/main/docs/config/lua/wezterm/format.md)). `wezterm.format` returns "a string with wezterm compatible escape sequences embedded" — the *host*, not the script, owns the final rendering step.

Threading is the key constraint: the event fires "when the text for a tab title needs to be recomputed," is synchronous — it "must return fast or it blocks the GUI thread; calling async functions like `run_child_process` yields an error" — and is invoked *twice per tab per pass* (first with `hover=false` and `max_width` so WezTerm can size tabs, then again with final values). If the handler errors or returns a wrong type, "the default tab title text will be computed and used instead"; only the first registered handler runs ([format-tab-title](https://wezterm.org/config/lua/window-events/format-tab-title.html)).

Status bar: `window:set_right_status` takes only "a string that can contain escape sequences," with the docs recommending `wezterm.format` to compose it; content is "displayed right-aligned and will be clipped from the left edge to fit in the available space," redrawn on the `update-right-status` event whose cadence is `status_update_interval` ([set_right_status](https://wezterm.org/config/lua/window/set_right_status.html)). The `update-status` event has no return value — the handler's job is to "carry out some activity and then ultimately call `window:set_right_status`" — and overlap is prevented: "only a single instance of this event is outstanding" ([update-status](https://wezterm.org/config/lua/window-events/update-status.html)).

Custom events: a keybinding `action = act.EmitEvent 'trigger-vim-with-scrollback'` "causes the equivalent of `wezterm.emit(name, window, pane)` to be called in the context of the current pane" ([EmitEvent](https://raw.githubusercontent.com/wezterm/wezterm/main/docs/config/lua/keyassignment/EmitEvent.md)). Handlers register with `wezterm.on(name, fn)`; "You may register handlers for arbitrary events for which wezterm itself has no special knowledge"; handlers run in registration order, returning `false` stops later handlers, and "simply reloading the configuration will clear any existing event handlers" ([wezterm.on](https://raw.githubusercontent.com/wezterm/wezterm/main/docs/config/lua/wezterm/on.md)).

**Assessment for DropTerm:** WezTerm is the proof that a *tiny closed vocabulary* (4–5 item kinds) covers tab titles and status bars, and that "return a table, host renders, wrong shape falls back to default" is a robust contract. Its ceiling is exactly where DropTerm wants to go past it: no layout, no nesting, no interaction — styling only. DropTerm should copy the flat-item escape hatch (a string or short item list for cheap surfaces like status segments) and the double-pass sizing handshake, but will need a nested schema for anything more.

### awesome WM's wibox: the original Lua-table widget tree — nesting as layout, `widget=`/`layout=` as type tags

awesome's declarative system is the closest ancestor of "Lua tables → widget tree." `wibox.widget{ ... }` builds the tree from table nesting: "the hierarchy is related to the table nesting (indentation)"; each node carries a `widget = <class>` key (what to instantiate), a `layout = <class>` key (how children are arranged), and arbitrary property keys. The canonical example ([declarative layout doc](https://awesomewm.org/apidoc/documentation/03-declarative-layout.md.html)):

```lua
local bg = wibox.widget {
  {
    { text = "foo", widget = wibox.widget.textbox },
    { text = "bar", widget = wibox.widget.textbox },
    layout = wibox.layout.fixed.vertical
  },
  bg = "#ff0000",
  widget = wibox.container.background
}
```

Three design facts matter for DropTerm:

1. **It is pure sugar, evaluated once.** Imperative and declarative forms "produce the exact same code"; the declarative API is "compiled into the imperative syntax when loaded," and — unlike QML — it "is interpreted only once and isn't automatically updated when values change." The docs recommend declarative for static layouts and imperative for dynamic ones.
2. **Property resolution is a fixed fallback chain.** When setting an attribute, awesome "will first check if `set_myproperty` exists," then a `myproperty` method, and finally "will just set the `mywidget.myproperty` directly" — which is why user code can inject custom keys (the docs demonstrate a `vicious = {...}` key caught by a metatable). Trivial custom widgets can be declared *inline* as bare `fit`/`draw` function tables.
3. **Templates = declarative tables for future instances.** `widget_template` (used by taglist/tasklist/notifications) is "a table using the exact same syntax as the declarative widgets" that the parent instantiates later, because "the widget must create many instances of the template at different points in time."

Events are imperative signals layered on the same objects. `gears.object` is "The object oriented programming base class used by various Awesome widgets and components," providing "basic observer pattern, signaling and dynamic properties"; widgets connect with `:connect_signal(name, func)` and emit with `:emit_signal(name, ...)` ([gears.object](https://awesomewm.org/apidoc/utility_libraries/gears.object.html)). Every widget inherits six base signals — `widget::layout_changed`, `widget::redraw_needed`, `button::press`, `button::release`, `mouse::enter`, `mouse::leave` — and mouse events are routed by the host: `wibox:find_widgets` hit-tests the widget hierarchy and delivers per-widget coordinates plus a `find_widgets_result` geometry table to the signal ([wibox.widget.base](https://awesomewm.org/apidoc/classes/wibox.widget.base.html)). Named nodes (`id = "second"`) stay addressable after construction via `my_wibox:get_children_by_id("third")[1]` ([declarative layout doc](https://awesomewm.org/apidoc/documentation/03-declarative-layout.md.html)).

**Assessment for DropTerm:** this is the model to imitate and the trap to avoid. The adaptable parts: nesting = layout (one key per node decides the container), `widget =` as the single type tag instead of per-type wrappers, a strict property→setter fallback chain so unknown keys fail loudly or inertly by rule rather than by guesswork, and host-side hit-testing so the script never receives raw coordinates. The trap: awesome's vocabulary is the *entire* widget library — anything with a `new` constructor is declarable — which is only tractable because awesome ships the widgets itself. DropTerm's fixed data vocabulary should stay much smaller and enumerate allowed keys explicitly instead of relying on the "just set the field" fallback, which turns typos into silent no-ops.

### Zellij: state snapshots flow in, a serialized component stream flows out — and it grew real components

Zellij plugins are WASM/WASI modules ("allowing plugin developers to develop plugins in many different languages," Rust officially supported) that are "a first class citizen in the workspace, just like a terminal pane" ([plugins](https://zellij.dev/documentation/plugins.html)). The interface is two methods, not a widget tree:

- **State in:** `fn update(&mut self, event: Event) -> bool` receives typed snapshots after `subscribe(&[EventType::...])` — `ModeUpdate(ModeInfo)` ("the current input mode... bound keys, the active theme colors, and the session name"), `TabUpdate(Vec<TabInfo>)` (position, name, active status, pane counts), `PaneUpdate(PaneManifest)` (all panes in all tabs: title, position, size, command, focus state), plus `Key`, `Mouse`, `Timer`, `SessionUpdate`, `RunCommandResult` and more ([plugin events](https://zellij.dev/documentation/plugin-api-events.html)).
- **UI out:** `render(&mut self, rows: usize, cols: usize)` prints to STDOUT; "Zellij treats the printed bytes as utf-8 ANSI" and "every time the render function is called, the previous state of the terminal is cleared" ([plugin rendering](https://zellij.dev/documentation/plugin-ui-rendering.html)).

On top of raw ANSI, Zellij ships a closed component vocabulary — display components `Table`, `Ribbon`, `Nested List`, `Text`, and interactive components `Button`, `Toggle`, `TextInput`, `NumberStepper`, `Dropdown`, `MenuList`, `SideMenu`, `ScrollView`, `ConfirmDialog`, composed by `FocusGroup` — "interpreted through serialized STDOUT in the render function as a private terminal DCS extension" (`<ESC>Pz<name>;...<ESC>\`, with coordinates `x/y/width/height`, a selected flag, an opaque flag, `$`-separated color indices, and `|`-prefixed indentation documented as a public protocol for SDK authors) ([plugin rendering](https://zellij.dev/documentation/plugin-ui-rendering.html)). The docs note these are "the components used by the built-in configuration screen" — Zellij's own UI is plugin-built, which forces the vocabulary to stay honest.

Interaction is round-trip but *host-routed*: interactive components implement a `Widget` trait with `handle_key(&KeyWithModifier) -> UiResponse` and `handle_mouse(Mouse) -> UiResponse`; the plugin forwards `Event::Key`/`Event::Mouse` into the component it rendered, and "mouse input only works on the same component that was rendered, and only at the position where it was last drawn" — components remember their draw rectangles, and handlers return typed responses (`Changed`, `Submitted`, `Activated`, `Cancelled`, `Consumed`, `NotHandled`) ([plugin rendering](https://zellij.dev/documentation/plugin-ui-rendering.html)).

**Assessment for DropTerm:** Zellij validates the two-channel design DropTerm already has (bus events in, declarative payload out) and adds the crucial third leg: *interaction returning as typed responses tied to a component identity the host remembers*. Adapt: a small closed component set whose interactive members return one of a handful of `UiResponse`-style outcomes, with the host doing hit-testing against last-drawn areas. Avoid: positional `print_table_with_coordinates(x, y, ...)`-style absolute coordinates for v0 — DropTerm has a real layout engine in GPUI and should not import TUI coordinate math.

### kitty kittens: no declarative surface at all — the script *is* the terminal program

A kitten is "just terminal programs written in Python. When launching a kitten, kitty will open an overlay window over the current window and optionally pass the contents of the current window/scrollback to the kitten over its STDIN" ([custom kittens](https://sw.kovidgoyal.net/kitty/kittens/custom/)). The contract is two functions:

```python
def main(args: list[str]) -> str:
    answer = input('Enter some text: ')
    return answer

def handle_result(args: list[str], answer: str, target_window_id: int, boss: Boss) -> None:
    w = boss.window_id_map.get(target_window_id)
    if w is not None:
        w.paste_text(answer)
```

`main()` runs in the overlay (the kitten draws by being an ordinary TUI — print/ANSI, no widget API), then `handle_result()` runs *inside the kitty process* with a `Boss` handle for "arbitrary actions such as closing windows, pasting text" and remote control via `boss.call_remote_control()`. `@result_handler(type_of_input='text'|'ansi'|'screen'|'history'|...)` selects what screen/scrollback capture lands on STDIN, and `@result_handler(no_ui=True)` skips the UI entirely for pure scripting.

**Assessment for DropTerm:** the counter-example. kitty's extension UI is "whatever the subprocess draws" plus a post-hoc action callback — zero structural contract, so no ecosystem of composable UI. It confirms DropTerm's instinct that a host-owned declarative vocabulary (rather than "run your own TUI in an overlay") is the actual feature.

### Neovim: API-as-surface — real windows addressed by id, content painted by buffer text + extmark annotations

Neovim's Lua gets no widget objects either: it composes real host surfaces through data. `nvim_open_win(buffer, enter, config)` opens a float ("Floats are fully functional windows supporting user editing, common api-window calls, and most window options") with a closed config keyset — `relative` (`cursor`/`editor`/`mouse`/`win`/`bufpos`), `row`/`col` (cell units, fractional), `width`/`height` (cells), `anchor` (`NW/NE/SW/SE`), `border` (8-char clockwise array, each side optionally `[char, highlight]`), `title`/`footer` (+`_pos`), `style='minimal'`, `zindex` (default 50, with hardcoded 100/200/250 for popupmenu/messages/cmdline), `focusable`, `mouse`, `hide` — and returns a `window-ID` that every later API call addresses ([api.txt](https://neovim.io/doc/user/api/)). Minimal example from the docs:

```lua
let opts = {'relative': 'cursor', 'width': 10, 'height': 2, 'col': 0,
      \ 'row': 1, 'anchor': 'NW', 'style': 'minimal'}
let win = nvim_open_win(buf, 0, opts)
```

The *content* of the surface is declared separately, as annotations anchored to buffer positions: `nvim_buf_set_extmark(buf, ns_id, line, col, opts)` takes a closed keyset — `end_row`/`end_col`, `hl_group` (stackable array), `virt_text` (list of `[text, hl]` chunks), `virt_text_pos` (`eol`/`overlay`/`inline`/`right_align`/`win_col`), `virt_lines`, `sign_text`, `line_hl_group`, `priority`, `conceal`, `ui_watched` — and returns an id that updates the mark in place ([api.txt](https://neovim.io/doc/user/api/)). Extmarks carry gravity (`right_gravity`, forward-shift on insert) and namespace scoping so plugins never collide. Neovim pins the schema evolution contract explicitly: "Map/list parameters/results may be EXTENDED (new fields may be added)... old clients MAY ignore them. Existing fields will not be removed."

Process model: Lua runs in-process; "Most API functions are deferred: they are queued ('scheduled') on the main loop," with `api-fast` functions and redraw-time decoration-provider callbacks (`ephemeral` extmarks valid for one redraw only) as the exceptions ([api.txt](https://neovim.io/doc/user/api/)).

**Assessment for DropTerm:** Neovim's lesson is *split the schema in two*: placement/geometry as one declarative table, content as annotation lists anchored to a surface — rather than one mega-tree. The "every field name is a closed keyset + ids are opaque integers + updates by passing the id back" pattern maps directly onto DropTerm's per-surface tables and bus events. The extmark `id`-for-update pattern is worth adopting for modal/panel surfaces that refresh. Avoid: exposing editability — DropTerm's surfaces are display+click, so the whole `focusable`/editing machinery is out of scope.

### RmlUi: real declarative markup (RML/RCSS) with Lua bolted on as *imperative* controller

RmlUi's script-facing UI is HTML/CSS-like RML markup, not Lua tables. The Lua integration embeds scripts per document — "Lua code can be embedded into an RML document with the `<script>` tag" (inline or `<script src="test.lua">`), compiled with the document — and inline attribute handlers (`<button onclick="Test()">`) are "executed as Lua code" with three handler globals: `event`, `element`, `document` ([embedding script](https://mikke89.github.io/RmlUiDoc/pages/lua_manual/embedding_script.html)). From Lua, construction is imperative method calls on that tree: `document:CreateElement('input')`, `document:AppendChild(input_element)`, `element:SetAttribute('type', 'radio')`, plus proxy properties `child_nodes`/`attributes`/`style` (e.g. `element.style.width`) and readable `tag_name`/`id`/`class_name`/`inner_rml` ([elements](https://mikke89.github.io/RmlUiDoc/pages/lua_manual/elements.html)). Events attach JavaScript-style: `element:AddEventListener('click', OnClick, true)` — "modeled directly on Javascript," handler being a function or a code string, third arg the capture-phase flag ([attaching to events](https://mikke89.github.io/RmlUiDoc/pages/lua_manual/attaching_to_events.html)). Custom decorators "can only be created in C++" ([Lua manual](https://mikke89.github.io/RmlUiDoc/pages/lua_manual.html)).

**Assessment for DropTerm:** RmlUi is the "JSX-like" end-state DropTerm explicitly rejected for v0: a full markup+stylesheet engine with scripts as imperative controllers inside it. Take one thing only — per-element handler binding where the *host* resolves which element was clicked and hands Lua `(event, element-id)` — and skip the DOM model, the CSS cascade, and script-in-markup entirely.

### Elixir `gpui` package (elixir-crab): the same snapshot/event bridge, stated as a contract

The Hex package `gpui` ("Renderer-independent application, session, snapshot, and declarative UI contracts for GPUI," v0.2.0, MIT, last updated Sep 08, 2026) is the nearest published neighbor of DropTerm's plan: a managed-language app owning state, a Rust GPUI host owning pixels. Its README states the split flatly: "Elixir renders bounded, serializable snapshots; displays interpret them and return strict typed events," and "Elixir owns the application. GPUI owns the native interaction. Snapshots connect them." — Rust keeps "the native event loop, windows, rendering, focus, IME, accessibility, and latency-sensitive interaction"; Elixir keeps "application state, window topology, UI declarations, and event handling" ([README](https://github.com/elixir-crab/gpui)). Its declarative surface is Phoenix-LiveView-style markup in a `~GPUI` sigil:

```elixir
def render(assigns) do
  ~GPUI"""
  <div class="flex grow flex-col items-center justify-center w-full gap-4 bg-slate-950">
    <text class="text-3xl font-semibold text-white">Count: {assigns.count}</text>
    <UI.button id="increment" label="Increment" variant="primary" phx-click="increment" />
  </div>
  """
end
```

Events come back as typed callbacks, not raw messages: "Pointer, keyboard, and accessible activation return through the same typed event path," surfaced as `handle_event("increment", _event, assigns)` — with the rule "Rust does not become a second application state system": `assigns` stays authoritative in Elixir and `render` is a pure function of it ([README](https://github.com/elixir-crab/gpui)). The umbrella also ships a deterministic `GPUI.Test` display that "require[s] neither Rust nor a native library or display server," letting the snapshot contract be tested headless.

**Assessment for DropTerm:** this is DropTerm's design with the paint dried. Three things to steal: (1) the slogan-level rule that the script side owns all state and the host interprets snapshots *without keeping app state of its own*; (2) `id` on every interactive element so the event back over the bus names the element, not a coordinate; (3) a headless test display — DropTerm can unit-test "Lua table → interpreted element tree" with zero GPUI windows, matching the existing headless test culture.

## Sources

- https://wezterm.org/config/lua/window-events/format-tab-title.html — format-tab-title contract: return types (string or FormatItem list), GUI-thread sync requirement, twice-per-pass sizing handshake, fallback on error, single-handler rule
- https://raw.githubusercontent.com/wezterm/wezterm/main/docs/config/lua/wezterm/format.md — full FormatItem vocabulary (Text/Attribute/Foreground/Background/ResetAttributes) and string return
- https://wezterm.org/config/lua/window/set_right_status.html — status bar contract: string input, format-composition, clipping, redraw cadence
- https://wezterm.org/config/lua/window-events/update-status.html — update-status: no return value, single-outstanding-instance rule
- https://raw.githubusercontent.com/wezterm/wezterm/main/docs/config/lua/keyassignment/EmitEvent.md — EmitEvent = wezterm.emit(name, window, pane) in current pane context
- https://raw.githubusercontent.com/wezterm/wezterm/main/docs/config/lua/wezterm/on.md — custom events: arbitrary handlers, ordered dispatch, false stops chain, reload clears handlers, example wiring
- https://awesomewm.org/apidoc/documentation/03-declarative-layout.md.html — wibox declarative system: nesting, widget=/layout= keys, property fallback chain, evaluated-once semantics, widget_template, id access
- https://awesomewm.org/apidoc/utility_libraries/gears.object.html — signal system: connect/emit/disconnect, property:: auto-signals
- https://awesomewm.org/apidoc/classes/wibox.widget.base.html — widget base signals (button::press etc.) and host-side find_widgets/hierarchy mouse routing
- https://zellij.dev/documentation/plugins.html — WASM/WASI plugin model, plugin-as-pane
- https://zellij.dev/documentation/plugin-ui-rendering.html — render(rows, cols), ANSI STDOUT, DCS component protocol, component list, Widget trait, UiResponse variants, mouse-vs-last-drawn-area rule
- https://zellij.dev/documentation/plugin-api-events.html — update(Event) contract, EventType list, snapshot payloads (ModeInfo/TabInfo/PaneManifest), subscribe()
- https://sw.kovidgoyal.net/kitty/kittens/custom/ — kitten contract: main()/handle_result(), STDIN screen capture types, no_ui, Boss/remote control, in-overlay vs in-process split
- https://neovim.io/doc/user/api/ (generated 2026-10-12 from `2f047b5`) — nvim_open_win full config keyset, float semantics, nvim_buf_set_extmark full opts keyset, API extension contract, deferred vs api-fast execution
- https://mikke89.github.io/RmlUiDoc/pages/lua_manual.html — Lua plugin overview, C++-only decorators
- https://mikke89.github.io/RmlUiDoc/pages/lua_manual/embedding_script.html — script tags in RML, onclick-as-Lua, event/element/document globals
- https://mikke89.github.io/RmlUiDoc/pages/lua_manual/elements.html — imperative element construction from Lua, proxy properties
- https://mikke89.github.io/RmlUiDoc/pages/lua_manual/attaching_to_events.html — AddEventListener(event, handler, capture) modeled on JavaScript
- https://hex.pm/packages/gpui — package identity: description, v0.2.0, Sep 08 2026, elixir-crab/gpui, MIT
- https://github.com/elixir-crab/gpui (README) — snapshot/event bridge architecture, ~GPUI markup example, typed handle_event path, test display

## Unverified

- **WezTerm `FormatItem` `{Format=...}` field:** a search-result summary claimed a `Format` field with `{}`-placeholders exists; the current docs source for `wezterm.format` (main branch) does not document it on the fetched page. Do not rely on it without checking the source.
- **WezTerm custom-event handler threading:** the `wezterm.on` page does not state which thread custom-event handlers run on; only `format-tab-title` explicitly documents GUI-thread synchronous execution.
- **RmlUi Lua runtime details:** the fetched Lua Manual pages do not name the Lua version or binding library (sol2/sol3 claims are unconfirmed); the Lua integration's provenance (upstream vs community contribution) was not checked.
- **Zellij protobuf:** Zellij's plugin *UI* protocol is the documented DCS ANSI stream; whether Zellij uses protobufs elsewhere (e.g. session-server IPC) was not investigated and is not claimed here.
- **Elixir gpui maturity:** the README describes the snapshot/test architecture but the package is at v0.2.0 with 3 releases; production-readiness was not assessed.
- The first `web_reader`/search attempts returned model-generated "helpful guesses" rather than fetched content (WezTerm FormatItem list, EmitEvent URL); those were discarded — every claim above comes from a fetched primary page.

## What DropTerm should adapt

Against DropTerm's design — a Lua table in on the host thread, the Rust UI thread interpreting it into GPUI elements, click/key/dismiss flowing back as bus events:

1. **Keep the vocabulary closed and tiny, WezTerm-style, for cheap surfaces; go nested, awesome-style, for real trees.** WezTerm proves 4–5 item kinds suffice for spans/status text, with "wrong shape → render default, log one line" as the failure mode (matches DropTerm's one-log-line error culture). awesome proves nesting-as-layout works: one `layout`-type key per container node, children in positional array order, `widget =` as the type tag. DropTerm's spans/rows/icons/modals should follow this shape: `{ kind = "row", direction = ..., children = {...} }` rather than per-type wrapper tables.
2. **Fail by rule, not by guess.** awesome's "check setter, then method, then silently set the raw field" chain is the trap — silent fields hide typos. Neovim's contract is the better model: closed keysets, unknown fields ignored-but-documented, existing fields never removed, and *any change is an additive extension* DropTerm can version per surface.
3. **Split placement/geometry from content.** Neovim separates window config from buffer-annotated content; DropTerm should keep each surface's table focused (a modal's position/sizing/dismiss behavior in its table, content as the child tree), and adopt extmark-style **update-by-id** so a refreshing panel re-renders by passing back the id it got, instead of unregistering/re-registering.
4. **Host owns hit-testing and routes events by element id.** All three interactive precedents converge: awesome's `find_widgets` hierarchy hit-test, Zellij's components remembering their last-drawn area ("mouse input only works... at the position where it was last drawn"), Elixir gpui's `phx-click` ids. DropTerm's UI thread should resolve click→element-id and deliver `element-click { surface, element_id }` on the bus — scripts never see coordinates.
5. **Typed, small event responses.** Zellij's `UiResponse` enum (Changed/Submitted/Activated/Cancelled/...) and gpui's `handle_event` show interaction results should be a handful of named outcomes per component kind, not free-form payloads. This fits DropTerm's existing `on(event, payload)` bus: event names stay stable, payloads stay small tables.
6. **The script side owns all state; the host never becomes a second state system** (gpui's rule). Render tables are pure functions of script state; DropTerm's interpreter should hold nothing but the last snapshot per surface, so a hard-cap interpreter teardown (which already loses all registrations by design) loses nothing else.
7. **Ship a headless test display.** gpui's `GPUI.Test` display that needs no native library mirrors DropTerm's headless-test culture: "Lua table → interpreted element tree" should be unit-testable with zero COSMIC windows, and the `every_config_key_is_documented`-style gate can pin the schema vocabulary to docs.
8. **Skip, deliberately:** RmlUi's markup+CSS engine and script-in-markup (the JSX-like end DropTerm deferred), kitty's run-your-own-TUI overlay (no contract, no composability), Zellij's absolute x/y print coordinates (DropTerm has GPUI layout; never import TUI math), and WezTerm's double-pass *synchronous-on-GUI-thread* evaluation — DropTerm's host-thread/channel split is strictly better; keep only the sizing handshake idea (host asks with available width, script returns content) not the threading.
