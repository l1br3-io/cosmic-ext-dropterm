# How do mainstream terminals implement plugin/extension systems, and what extension surface areas do they expose?

**Question:** How do mainstream terminals implement plugin/extension systems (Warp, WezTerm, kitty, Zellij, rio, Ghostty, iTerm2, Hyper, Tabby, tmux), and what extension surface areas (status bar, side panels, keybindings, suggestions, themes, tab titles, hooks) do they expose?

**Date:** 2026-11-21

**TL;DR:** No mainstream terminal ships a VS Code-style plugin API; the four real architectures are embedded interpreters (WezTerm/kitty/Hyper/Tabby), out-of-process APIs (iTerm2/kitty's socket), sandboxed WASM (Zellij), and config-only (Ghostty/rio/tmux) — and Warp, REA-inspected at 0.2026.11.18, ships no in-process plugin system at all, extending through files and protocols (workflows, skills, hooks.json, MCP, ACP). Verdict (owner, 2026-11-21): tiered, Lua-first — the WezTerm blueprint (mlua with vendored Lua 5.4, event bus, compute-strings-host-renders) powering status segments, tab titles, script commands, and event hooks; data files first; WASM deferred; native dylibs never; completions stay builtin.

## Findings

### Warp ships no third-party plugin/extension API as of 2026 — extensibility is workflows, MCP servers, rules, themes, and a cloud-agent HTTP API

- A user opened "Extension API for Warp" ([warpdotdev/warp#4053](https://github.com/warpdotdev/warp/issues/4053), 2024-01-10) asking for "an Extensions API a la VS Code"; Warp team member dannyneira (2024-01-11) said Warp was "working on a way for custom completions (no firm eta on this, but it's in progress)" and pointed to a URI scheme for opening configurations plus a tracking discussion (#435). The issue was closed as a duplicate and relabeled "enhancement" (April 2026 relabel visible in the thread). No extension/plugin section exists anywhere in the [docs index](https://docs.warp.dev/llms.txt) (fetched 2026-11-21).
- **Workflows** are parameterized YAML files, not code: fields `name`/`command`/`tags`/`description`/`arguments` (each argument with `name`, `description`, `default_value`), arguments interpolated with `{{name}}`. Files placed in `$HOME/.warp/workflows/` (macOS), `%APPDATA%\warp\Warp\data\workflows\` (Windows), `${XDG_DATA_HOME:-$HOME/.local/share}/warp-terminal/workflows/` (Linux), or a repo's `.warp/workflows` are "automatically load[ed] into the Workflows menu" ([YAML Workflows docs](https://docs.warp.dev/terminal/entry/yaml-workflows), fetched 2026-11-21).
- **MCP client support**: servers are added as JSON snippets (`command`/`args`/`env`/`working_directory` for stdio servers — "Warp will launch this command when starting up and shut it down on exit" — or `url` + optional `headers` for Streamable HTTP/SSE). Warp ships out-of-the-box servers in a `Shared` section and also reads `.warp/.mcp.json` (global/project) plus third-party files (`~/.claude.json`, `~/.codex/config.toml`). Trust model: "Project-scoped servers never auto-spawn" — explicitly to prevent a cloned repo "automatically starting an MCP server that runs arbitrary local commands" — and "Config edits require approval" ([MCP docs](https://docs.warp.dev/agents/capabilities/mcp), fetched 2026-11-21).
- **HTTP API**: [docs.warp.dev/openapi.yaml](https://docs.warp.dev/openapi.yaml) (bearer auth; fetched 2026-11-21) is a cloud-agent management API — spawn/steer/cancel agent runs (`POST /agent/runs`, `.../followups`, `.../interrupt`), read transcripts/timelines/diagnostics, manage Factories, tasks, branches, and cost metrics. It exists for external automation (CI, cron, backend services) driving Warp's hosted agents, not for extending the local terminal UI.
- **Themes** are YAML files installed under `~/.warp/themes/`; the [warpdotdev/themes](https://github.com/warpdotdev/themes) repo (Apache-2.0) holds standard/base16/community sets contributed by PR with a preview-generation script ([themes repo README](https://github.com/warpdotdev/themes), fetched 2026-11-21).
- **Rules** (agent behavior) are markdown: project rules live in `AGENTS.md` (or legacy `WARP.md`, which "takes priority"), global rules are UI-defined; Warp "automatically applies the AGENTS.md (or WARP.md) in the root and in the current directory" ([Rules docs](https://docs.warp.dev/agents/capabilities/rules), fetched 2026-11-21).

### WezTerm: embedded Lua (mlua) with a host event bus, plus a git-based plugin manager

- Config is a Lua script (`wezterm.lua` at `~/.wezterm.lua`, `~/.config/wezterm/wezterm.lua`, or overrides) returning a config table. The Lua runtime is mlua pinned to Lua 5.4 — `mlua = {workspace=true, features=["vendored", "lua54", "async", "send", "serialize"]}` ([config/Cargo.toml on wez/wezterm main](https://github.com/wezterm/wezterm/blob/main/config/Cargo.toml), fetched 2026-11-21). Hot reload is automatic: "if/when it changes, the configuration will be automatically reloaded and the majority of options will take effect immediately" ([config/files docs](https://wezterm.org/config/files.html), fetched 2026-11-21).
- **Plugin manager**: `wezterm.plugin.require 'https://github.com/owner/repo'` clones a git repo (HTTPS or `file://` only — "Only HTTP(S) or local filesystem repos are allowed for the git URL") into the runtime dir under `plugins/NAME`. "Once cloned, the repo is NOT automatically updated when require is called again"; updates are manual via `wezterm.plugin.update_all()`, recommended from the debug-overlay Lua REPL. Available since version `20230320-124340-559cb7b0` ([plugins docs](https://wezterm.org/config/plugins.html), [`wezterm.plugin.require` docs](https://wezterm.org/config/lua/wezterm.plugin/require.html), both fetched 2026-11-21). "A Wezterm plugin is a package of Lua files that provide some predefined functionality not in the core product"; its `init.lua` "must return a module that exports an apply_to_config function", so a plugin is config code that runs in the same interpreter, registering event handlers.
- **Event system**: handlers register with `wezterm.on(event, callback)`; "wezterm.on can register multiple callbacks for the same event", a handler returning `false` short-circuits the rest, and user-defined events work: "You may register handlers for arbitrary events for which wezterm itself has no special knowledge", fired via `wezterm.emit` or the `EmitEvent` key assignment ([`wezterm.on` docs](https://wezterm.org/config/lua/wezterm/on.html), fetched 2026-11-21).
- **Documented window events** ([window-events index](https://wezterm.org/config/lua/window-events/index.html), fetched 2026-11-21): `format-tab-title`, `format-window-title`, `update-status`, `update-right-status`, `window-config-reloaded`, `window-focus-changed`, `window-resized`, `bell`, `open-uri`, `user-var-changed`, `new-tab-button-click`, `augment-command-palette`.
- **Tab titles**: `format-tab-title` is "emitted when the text for a tab title needs to be recomputed", receives `(tab, tabs, panes, config, hover, max_width)`, and returns either a string or "a table holding FormatItems" (per-span color/style). It "must return as quickly as possible in order to avoid blocking the GUI thread", and only the first registered handler wins ([format-tab-title docs](https://wezterm.org/config/lua/window-events/format-tab-title.html), fetched 2026-11-21).
- **Status bar**: `update-right-status`/`update-status` are "emitted periodically (based on the interval specified by the status_update_interval configuration value)"; the handler's "purpose is to allow you the chance to carry out some activity and then ultimately call window:set_right_status" with styled text; wezterm "will ensure that only a single instance of this event is outstanding". `update-right-status` is "considered to be deprecated" since `20220903-194523-3bb1ed61` in favor of `update-status` ([update-right-status docs](https://wezterm.org/config/lua/window-events/update-right-status.html), fetched 2026-11-21).

### kitty: Python kittens (overlay programs + in-host hooks), a socket remote-control protocol, and a Python custom tab bar

- **Kittens** are Python files mapped to keys (`map ctrl+k kitten mykitten.py arg1 arg2`). "When launching a kitten, kitty will open an overlay window over the current window"; the kitten defines `main(args) -> str` (runs as a terminal program in the overlay, can read STDIN — screen/scrollback/selection content via `@result_handler(type_of_input='text'|'ansi'|'history'|'output'|'selection')`) and `handle_result(...)`, which runs "inside the kitty process" and receives a `Boss` object with "full kitty internals" (window map, layouts, `call_remote_control()`) ([custom kittens docs](https://github.com/kovidgoyal/kitty/blob/master/docs/kittens/custom.rst), fetched 2026-11-21).
- **Remote control**: opt-in via `allow_remote_control` plus `--listen-on unix:/tmp/mykitty`; commands sent with `kitten @ --to unix:/tmp/mykitty` (launch, send-text, ls, set-colors, ...). Security model: blanket enable grants "the ability to any program running on your computer or even on remote computers via SSH the ability to use remote control"; since v0.26.0, `allow_remote_control password` + scoped `remote_control_password` entries (e.g. only `get-colors set-colors`) gate subsets of commands, with a time-based nonce "to minimise the potential for replay attacks"; a custom `is_cmd_allowed` Python script can arbitrate every command; unknown passwords trigger an interactive allow/deny prompt ([remote-control docs](https://github.com/kovidgoyal/kitty/blob/master/docs/remote-control.rst), fetched 2026-11-21).
- **Custom tab bar** (status area hook): `tab_bar_style` offers "fade, hidden, powerline, separator, slant, custom"; the `custom` style means "A user-supplied Python function called draw_tab is loaded from the file tab_bar.py in the kitty config directory", drawing directly onto a Screen object; separately, `tab_title_template` supports a `{custom}` placeholder backed by a `draw_title(data)` function in the same file. `tab_bar_edge` takes `top`, `bottom`, `left`, `right` ([kitty options source](https://github.com/kovidgoyal/kitty/blob/master/kitty/options/definition.py), fetched 2026-11-21).
- **Side/docked panels**: the [panel kitten](https://github.com/kovidgoyal/kitty/blob/master/docs/kittens/panel.rst) lets you "Draw the desktop wallpaper or docks and panels using arbitrary terminal programs" — docked windows at screen edges (`--edge`, `--lines`) for "showing status information or notifications on your desktop" (fetched 2026-11-21).
- **Themes**: the `themes` kitten switches among "over three hundred pre-built themes available at kitty-themes" ([github.com/kovidgoyal/kitty-themes](https://github.com/kovidgoyal/kitty-themes)) with live previews, persists by copying the chosen `.conf` to `~/.config/kitty/current-theme.conf` and adding an include to kitty.conf; user themes are plain conf files dropped in `~/.config/kitty/themes` ([themes kitten docs](https://github.com/kovidgoyal/kitty/blob/master/docs/kittens/themes.rst), fetched 2026-11-21).

### Zellij: WASM plugins (wasmi runtime), permission grants, and a plugin-declares-structured-content/host-renders UI model

- The plugin system is "a Webassembly / WASI plugin system" where plugins are "a first class citizen in the workspace, just like a terminal pane" that "can render UI, react to application state changes, and control/change Zellij's behavior"; they are "visual cross-platform scripts that do not need to be installed or compiled", and Zellij's own UI (tab bar, status bar, session manager) is built from plugins ("Rust is the only language officially supported") ([plugins overview](https://zellij.dev/documentation/plugins.html), fetched 2026-11-21).
- **Runtime**: the docs don't name the engine; the source does — `zellij-server/Cargo.toml` declares `wasmi = { version = "1.1.0", default-features = false, features = ["std"] }` and `wasmi_wasi = "1.1.0"` ([zellij-server/Cargo.toml on main](https://github.com/zellij-org/zellij/blob/main/zellij-server/Cargo.toml), fetched 2026-11-21). The runtime was migrated "from wasmtime to wasmi" (PR #4449) and later upgraded to wasmi 1.1.0 (PR #5319) per the [zellij CHANGELOG](https://github.com/zellij-org/zellij/blob/main/CHANGELOG.md).
- **UI contribution**: a plugin draws by printing from its render function — "When a plugin's render function prints to STDOUT, Zellij treats the printed bytes as utf-8 ANSI", and "Every time the render function is called, the previous state of the terminal is cleared". On top of raw ANSI there are cross-language structured components serialized "through the render function as a private terminal DCS extension": display components (Table, Ribbon, Nested List, Text) and interactive ones (Button, Toggle, TextInput, NumberStepper, Dropdown, MenuList, SideMenu, ScrollView, ConfirmDialog) — "These are the components used by the built-in configuration screen, prompts and context menu." The host interprets the serialized instructions; the plugin keeps component state and forwards Key/Mouse events ([plugin UI rendering docs](https://zellij.dev/documentation/plugin-ui-rendering.html), fetched 2026-11-21).
- **Loading/distribution**: plugins load "through a Layout, through the command line, or from a keybinding" (`launch_or_focus_plugin`), or at session start via a `load_plugins` block. "Plugins are referred to by URLs" with four schemes: `file:` (local `.wasm`), built-in `zellij:` (e.g. `zellij:bars`, `zellij:session-manager`, `zellij:filepicker`), `http(s)://path/to/my/plugin.wasm`, and bare aliases. A built-in plugin manager (Ctrl o + p) loads and monitors plugins ([plugin loading docs](https://zellij.dev/documentation/plugin-loading.html), fetched 2026-11-21).
- **Permissions**: "The plugin system provides a permission system to provide extra security and protection to the user." A plugin must call `request_permission` to be granted capabilities; the grantable list includes ReadApplicationState, ChangeApplicationState, OpenFiles, RunCommands, OpenTerminalsOrPlugins, WriteToStdin, WebAccess, ReadCliPipes, MessageAndLaunchOtherPlugins, Reconfigure, FullHdAccess ("Access the full HD on the machine"), StartWebServer, InterceptInput, ReadPaneContents, RunActionsAsUser, WriteToClipboard, ReadSessionEnvironmentVariables. "Built-in plugins are granted every permission"; some commands need combinations (e.g. `set_live_view` needs ReadPaneContents + WriteToStdin) ([plugin permissions docs](https://zellij.dev/documentation/plugin-api-permissions.html), fetched 2026-11-21).

### rio has no plugin/scripting/IPC API as of 2026 — config + themes + a librio embedding C-ABI for host apps

- The [config docs](https://rioterm.com/docs/config) (fetched 2026-11-21) document no IPC key, no plugin, and no scripting surface (keys: theme, colors, cursor, fonts, hints, keyboard, navigation, panel, renderer, scroll, shell, title, window, ...), and the [features page](https://rioterm.com/docs/features) lists no IPC/plugins/scripting either (Vi mode, hints, hyperlinks, graphics protocols, shaders, multi windows, splits, ...).
- The [CHANGELOG on main](https://github.com/raphamorim/rio/blob/main/CHANGELOG.md) (fetched 2026-11-21) contains no entries for IPC, socket, plugin, extension, scripting, or automation. The only programmatic surface is **librio embedding**: recent entries describe C-ABI/embedder changes "with the Rust and C APIs unchanged" (0.5.28, 2026-09-17) — an API for applications embedding the terminal library, not a user plugin system.

### Ghostty: deliberately no embedded scripting language; declarative config + actions, an IPC-leaning position, and AppleScript on macOS since 1.3

- Config is a "custom, but very simplistic key = value syntax" under a "Zero Configuration Philosophy"; keybindings are `keybind = ctrl+z=close_surface` mapping triggers to an action library; config hot-reloads at runtime (Ctrl+Shift+, on Linux). No scripting, plugin, or extension mechanism is mentioned anywhere on the config page ([ghostty config docs](https://ghostty.org/docs/config), fetched 2026-11-21).
- Mitchell Hashimoto's stated position ([discussion #2353 "Scripting API for Ghostty"](https://github.com/ghostty-org/ghostty/discussions/2353)): he is "leaning towards a single-line text protocol in the format of memcached/redis/etc." (2023-11-06) — easy to debug ("You can telnet and just start typing as an interactive repl"), easy to client, secure over TLS with passwords; he rejected D-Bus because "This needs to be cross platform" (2024-04-17); and as of 2026-09-16 he still notes general scripting is hard because "Ghostty isn't a server-based app", plus security concerns. No embedded language is planned.
- What shipped instead: **AppleScript (macOS only)** — "AppleScript support was introduced in Ghostty 1.3.0", exposing the object hierarchy "application -> windows -> tabs -> terminals" with query and control commands (create/split/focus/close, `input text`, `send key`, `perform action` — action strings "match the action names used in keybind configuration"), gated by macOS TCC automation prompts ([AppleScript docs](https://ghostty.org/docs/features/applescript), fetched 2026-11-21).

### iTerm2: out-of-process Python API over websockets, with hooks for tab titles and status bar components

- The API is "a Python package which makes it easy to write Python scripts that control iTerm2 and extend its behavior"; the `iterm2` module (on PyPI) exposes App/Window/Tab/Session/Profile/Status Bar/Tool/Registration classes ([Python API overview](https://iterm2.com/python-api/), fetched 2026-11-21).
- Architecture: scripts run as **external processes** attaching to a running iTerm2 — "The underlying implementation uses Google protobuf and websockets"; "iTerm2 communicates with the script over a websocket connection"; the script's `main(connection)` gets a connection "that holds the link to a running iTerm2 process" ([tutorial example](https://iterm2.com/python-api/tutorial/example.html), fetched 2026-11-21).
- **Hooks**: "iTerm2 provides a mechanism called hooks that allows your Python code to modify the app's default behavior." Two hook kinds: a **Session title provider** (`@iterm2.TitleProviderRPC` returning the tab/window title string, re-run whenever a referenced session variable changes, selectable in Prefs > Profiles > General > Title) and a **custom status bar component** (`iterm2.StatusBarComponent` + `@iterm2.StatusBarRPC`; when registered, "a new status bar component becomes available in Prefs > Profiles > Session > Configure Status Bar"; components can update on variable change or periodically via `update_cadence`, and can define `knobs` and click handlers). Long-running hook daemons install into `~/Library/Application Support/iTerm2/Scripts/AutoLaunch` ([hooks tutorial](https://iterm2.com/python-api/tutorial/hooks.html), fetched 2026-11-21). Session lifecycle is observable from scripts (the bundled example notes "When the session ends, this will raise an exception" in a per-session monitor task).

### Hyper (Electron): npm-installed JS plugins decorating React components and the Redux store

- Plugins are npm package names listed in the `plugins` array of the config; local development uses a `localPlugins` array pointing at directories (e.g. `localPlugins: ['hyper-awesome-plugin']`, cloned/symlinked under `plugins/local`). "To load, your plugin should expose at least one API method"; the full hook list is defined centrally in `app/plugins/extensions.ts` ([PLUGINS.md on vercel/hyper canary](https://github.com/vercel/hyper/blob/canary/PLUGINS.md), fetched 2026-11-21).
- The declared contract ([extensions.ts on canary](https://github.com/vercel/hyper/blob/canary/app/plugins/extensions.ts), fetched 2026-11-21) is ~40 hooks split by Electron process: main-process lifecycle (`onApp`, `onWindow`, `onUnload`), renderer component decoration (`decorateTerm`, `decorateHyper`, `decorateHeader`, `decorateTab`, `decorateNotification`, ...), config/env/keymap transformation (`decorateConfig`, `decorateEnv`, `decorateKeymaps`, `decorateBrowserOptions`, `decorateMenu`), Redux (`middleware`, `reduceUI`, `reduceSessions`, ...), prop providers (`getTabProps`, `getTermProps`, ...), and state mappers (`mapTermsState`, ...). Decorators wrap React components as HOCs — e.g. `exports.decorateTerm = (Term, { React, notify }) => { ... }`; menu click handlers run in the main process and bridge to the renderer via `rpc.emit` ([PLUGINS.md](https://github.com/vercel/hyper/blob/canary/PLUGINS.md)).
- Distribution: plugins/themes are submitted to the Hyper Store ("Want to submit a plugin or theme to the Hyper Store?" — [hyper-site README](https://github.com/vercel/hyper-site), which carries `plugins.json`, fetched 2026-11-21). There is no sandboxing: plugins can `require('electron')` directly ("Hyper doesn't provide a reference to electron", but nothing blocks it) ([PLUGINS.md](https://github.com/vercel/hyper/blob/canary/PLUGINS.md)).

### Tabby: TypeScript/Angular-DI plugins installed from the Settings UI

- "Plugins and themes can be installed directly from the Settings view inside Tabby"; the plugin API docs ([docs.tabby.sh](https://docs.tabby.sh), fetched 2026-11-21) are TypeDoc references over `tabby-core` (AppService, TabsService, ConfigService, ProfilesService, HotkeysService, ToolbarButtonProvider, Theme/ThemesService, BaseTabComponent, ...).
- Structure: a plugin is an Angular `NgModule` that registers multi-providers — `{ provide: TabContextMenuItemProvider, useClass: MyContextMenu, multi: true }` — extending the app through DI rather than a dedicated hook bus. Community plugins are individual GitHub repos/npm packages (e.g. `tabby-docker`, `tabby-save-output`, `tabby-mcp-server`); several retain the legacy `terminus-` prefix from Tabby's former name ([tabby README](https://github.com/Eugeny/tabby/blob/master/README.md), fetched 2026-11-21).

### tmux: the classic minimal model — status bar via `#(...)` shell substitution on a timer

- `status-left`/`status-right` format strings support `#(...)` shell-command substitution whose output is inserted into the status line; the command executes only once per `status-interval` (default 15 seconds; "A setting of zero disables redrawing") and its output is cached across redraws in between ([tmux(1) man page, STATUS LINE/FORMATS sections](https://man7.org/linux/man-pages/man1/tmux.1.html), fetched 2026-11-21; see Unverified for a fetch caveat). There is no plugin system — this command-substitution format language is the entire extension surface, and it predates every other mechanism surveyed here.

### Cross-cutting: surface areas each system exposes

- **Status bar segments/widgets**: WezTerm (`update-status` + `window:set_right_status` styled spans), kitty (custom `tab_bar.py` drawing + panel kitten for desktop docks), Zellij (the status/tab bars *are* plugins — `zellij:bars`), iTerm2 (scriptable `StatusBarComponent` with knobs and cadence), Hyper (`decorateHeader`), tmux (`#(...)` substitution), Tabby (via Angular providers/services). Warp, Ghostty, rio: none.
- **Tab titles**: WezTerm (`format-tab-title` returning styled spans), kitty (`tab_title_template` + `draw_title`), iTerm2 (`TitleProviderRPC` hook), tmux (format variables), Zellij (ChangeApplicationState commands). Ghostty exposes `set_tab_title` only as an AppleScript/keybind action.
- **Side panels**: two shapes — a *docked terminal window* (kitty panel kitten: any terminal program becomes an edge-docked panel) and a *first-class plugin pane* (Zellij: plugins are panes in the layout; Tabby plugins extend tabs/splits as Angular components). WezTerm has no side-panel API (overlays/tables only). iTerm2 exposes a `Tool` class in the API for its toolbelt.
- **Custom keybindings/commands**: WezTerm (config keys + `wezterm.action_callback` + `augment-command-palette` entries), kitty (`map` + remote control), Zellij (`RunCommands`/`RunActionsAsUser` permissions, keybinding-launched plugins), iTerm2 (RPCs invocable from triggers/keystrokes), Hyper (`decorateKeymaps`, `decorateMenu`), Tabby (`HotkeysService`/`HotkeyProvider`).
- **Output/focus/lifecycle hooks**: WezTerm is richest (`user-var-changed` for OSC 1337 vars, `bell`, `open-uri`, `window-focus-changed`, `window-config-reloaded`); kitty kittens can read screen/scrollback/selection content as typed STDIN; iTerm2 scripts get session lifecycle notifications; Hyper plugins get React lifecycle + cursor handlers in docs. No peer exposes raw PTY-output streaming to third-party code except via these typed seams.
- **Suggestions/completion**: essentially nobody has a third-party completion API — Warp's "custom completions" was explicitly "no firm eta... in progress" in 2024 with no shipped API since; WezTerm's `augment-command-palette` extends the command palette (not shell completion); kitty's hints-style overlays are kittens but there is no completion hook.
- **Themes**: universal. Formats are plain files: YAML repos (Warp `warpdotdev/themes`, installed to `~/.warp/themes/`), conf files (kitty-themes + `current-theme.conf` include), config tables (WezTerm/Ghostty/rio/Tabby), `decorateConfig` (Hyper), and an API class (iTerm2 "Color Presets").

### Cross-cutting: three UI contribution models, and the trade each one makes

1. **Embedded interpreter computes content, host renders it** (WezTerm Lua returning strings/FormatItem spans; iTerm2 Python returning title/status strings; tmux format templates). Cheapest to host; the plugin can only paint text+style into slots the host already owns — no arbitrary layout.
2. **Plugin draws freely inside host-granted space** (kitty: kitten gets an overlay window and Screen APIs, `draw_tab` paints cells; Hyper: plugin wraps React components with full DOM/canvas control). Most powerful, most coupled — the plugin speaks the host's widget/rendering internals.
3. **Plugin emits structured content the host interprets** (Zellij: DCS-serialized Table/Text/Button/Toggle/TextInput/etc. components; Warp workflows: YAML command+argument schema rendered as host UI). Language-agnostic and permission-checkable, but the widget vocabulary is fixed by the host — Zellij had to grow a large component set (including SideMenu, ScrollView, ConfirmDialog) to let plugins rebuild its own chrome.

### Cross-cutting: sandboxing exists only where the plugin boundary crosses a process or capability edge

- **Capability grants**: Zellij is the only system with a true manifest-level permission model (17 named capabilities, `request_permission` prompt, built-ins exempt).
- **Socket auth**: kitty gates its remote-control socket three ways (off by default, scoped command passwords with nonce protection, custom `is_cmd_allowed` arbitrator) precisely because anything on the machine — or across SSH — can reach a unix socket.
- **Out-of-process APIs**: iTerm2's websocket API and Warp's bearer-auth HTTP API live at a process/network boundary where auth is natural. Warp adds UI-level trust rules for MCP (project-scoped servers never auto-spawn; config edits require approval).
- **In-process interpreters have no sandbox at all**: WezTerm Lua, kitty kittens' `handle_result` (in-process), Hyper JS, and Tabby Angular plugins all run with full application privileges. The Docs say nothing about restricting them — the isolation story is "the user chose to install it".

### Cross-cutting: distribution follows the plugin's runtime

- **Git repos**: WezTerm plugins (`wezterm.plugin.require` HTTPS/file URLs, cloned to runtime dir), kitty and Warp theme repos (PR to `kitty-themes` / `warpdotdev/themes`).
- **npm + app store**: Hyper (`plugins` array of npm names, Hyper Store), Tabby (npm packages installable from Settings).
- **WASM URLs**: Zellij (`file:`, `http(s)`, built-in `zellij:` aliases — no compiled install step, permission-cached per location).
- **Filesystem dirs**: kitty kittens and `tab_bar.py` (`~/.config/kitty/`), Warp workflows (`~/.warp/workflows/` + repo-local `.warp/workflows`), iTerm2 (`Scripts/AutoLaunch`).
- **None**: Ghostty, rio, tmux — the extension surface is the config file itself (actions, formats, templates), which is also why their docs need no plugin chapter.

## Recommendation

**Adopted (owner ruling, 2026-11-21): tiered, Lua-first — the WezTerm blueprint.**

Tier 0 data files (theme packs from the config dir) ship first; Tier 1 is an
embedded Lua runtime — mlua with vendored Lua 5.4, WezTerm's exact engine
choice — running trusted scripts from the user's config directory against an
event bus, contributing status-bar segments, tab-title providers, bindable
named commands, and event hooks (bell/title/exit/focus), always on the
compute-strings-host-renders model. One deliberate adaptation for DropTerm:
the interpreter lives on a dedicated host thread talking to the gpui
executor over channels, where WezTerm runs Lua on the GUI thread under a
slow-handler warning — DropTerm's executor is sensitive to foreign-thread
wakes (see [pty-test-gate](pty-test-gate.md)) and the UI thread must never
wait on a script. WASM plugins on the Zellij/wasmi model are deferred, not
rejected: they earn their keep only when third-party plugin demand appears.
Specced as the `plugins` initiative (`/spec`, 2026-11-21).

Rejected alternatives:

- **Native dylib plugins** — no stable Rust ABI; the portability plan's
  backend ruling generalizes to feature plugins.
- **WASM-first (Zellij/wasmi)** — the heaviest build (runtime, WASI surface,
  capability system, structured-UI vocabulary) for a demand that doesn't
  exist yet; deferred, not rejected.
- **Completion plugins** — no peer ships them (Warp's 2024 "in progress"
  never landed); the suggest menu is the app's most latency-sensitive
  surface; the TOML tiers already carry the data-pack case.
- **Nothing (the Ghostty/rio stance)** — leaves the status bar and
  agent-adjacent surfaces static; WezTerm proves the Lua tier is cheap to
  carry in a Rust terminal.
- **Main-thread Lua exactly as WezTerm** — rejected for the thread-model
  reason above; the one accuracy-for-feasibility trade in the adaptation.

## Sources

- https://docs.warp.dev/terminal/entry/yaml-workflows — Warp YAML workflow format, fields, per-OS directories (fetched 2026-11-21)
- https://docs.warp.dev/agents/capabilities/mcp — MCP server config (JSON/URL), bundled servers, auto-spawn trust rules (fetched 2026-11-21)
- https://docs.warp.dev/openapi.yaml — Warp HTTP API: bearer-auth cloud-agent/Factories management endpoints (fetched 2026-11-21)
- https://docs.warp.dev/agents/capabilities/rules — AGENTS.md/WARP.md rules model (fetched 2026-11-21)
- https://github.com/warpdotdev/themes — theme YAML format, `~/.warp/themes/`, PR contribution flow (fetched 2026-11-21)
- https://github.com/warpdotdev/warp/issues/4053 — extension-API feature request, team response, closed as duplicate (fetched 2026-11-21)
- https://docs.warp.dev/llms.txt — docs index showing no plugins/extensions section (fetched 2026-11-21)
- https://wezterm.org/config/plugins.html — WezTerm plugin system overview, apply_to_config contract (fetched 2026-11-21)
- https://wezterm.org/config/lua/wezterm.plugin/require.html — git-URL-only loading, runtime-dir storage, no auto-update, version since 20230320 (fetched 2026-11-21)
- https://wezterm.org/config/lua/wezterm/on.html — event registration, multiple handlers, custom events/EmitEvent (fetched 2026-11-21)
- https://wezterm.org/config/lua/window-events/index.html — full window-event list (fetched 2026-11-21)
- https://wezterm.org/config/lua/window-events/format-tab-title.html — tab-title event contract and FormatItem return (fetched 2026-11-21)
- https://wezterm.org/config/lua/window-events/update-right-status.html — periodic status event, set_right_status, deprecation note (fetched 2026-11-21)
- https://wezterm.org/config/files.html — config file paths and automatic reload (fetched 2026-11-21)
- https://github.com/wezterm/wezterm/blob/main/config/Cargo.toml — mlua dependency with lua54/vendored features (fetched 2026-11-21)
- https://github.com/kovidgoyal/kitty/blob/master/docs/kittens/custom.rst — kitten structure, overlay loading, Boss access, typed STDIN (fetched 2026-11-21)
- https://github.com/kovidgoyal/kitty/blob/master/docs/remote-control.rst — socket remote control, security model, scoped passwords (fetched 2026-11-21)
- https://github.com/kovidgoyal/kitty/blob/master/kitty/options/definition.py — tab_bar_style `custom` + `tab_bar.py` `draw_tab`/`draw_title` (fetched 2026-11-21)
- https://github.com/kovidgoyal/kitty/blob/master/docs/kittens/panel.rst — panel kitten for edge-docked panels/docks (fetched 2026-11-21)
- https://github.com/kovidgoyal/kitty/blob/master/docs/kittens/themes.rst — themes kitten, kitty-themes repo, persistence (fetched 2026-11-21)
- https://zellij.dev/documentation/plugins.html — WASM/WASI plugin model, plugins-as-panes (fetched 2026-11-21)
- https://zellij.dev/documentation/plugin-ui-rendering.html — STDOUT ANSI rendering + DCS structured components (fetched 2026-11-21)
- https://zellij.dev/documentation/plugin-loading.html — load paths, URL schemes, built-in plugins (fetched 2026-11-21)
- https://zellij.dev/documentation/plugin-api-permissions.html — permission list and request_permission (fetched 2026-11-21)
- https://github.com/zellij-org/zellij/blob/main/zellij-server/Cargo.toml — wasmi 1.1.0 runtime dependency (fetched 2026-11-21)
- https://github.com/zellij-org/zellij/blob/main/CHANGELOG.md — wasmtime-to-wasmi migration PR #4449 (fetched 2026-11-21)
- https://rioterm.com/docs/config — rio config keys; no IPC/plugin/scripting surface (fetched 2026-11-21)
- https://rioterm.com/docs/features — rio feature list; no IPC/plugins (fetched 2026-11-21)
- https://github.com/raphamorim/rio/blob/main/CHANGELOG.md — no IPC/plugin entries; librio C-ABI embedding notes (fetched 2026-11-21)
- https://ghostty.org/docs/config — key = value config, zero-config philosophy, hot reload (fetched 2026-11-21)
- https://github.com/ghostty-org/ghostty/discussions/2353 — Hashimoto's scripting position across 2023–2026 (fetched 2026-11-21)
- https://ghostty.org/docs/features/applescript — AppleScript object model and actions, since 1.3.0 (fetched 2026-11-21)
- https://iterm2.com/python-api/ — API overview and class index (fetched 2026-11-21)
- https://iterm2.com/python-api/tutorial/example.html — websocket/protobuf architecture, external-process model (fetched 2026-11-21)
- https://iterm2.com/python-api/tutorial/hooks.html — title provider + status bar component hooks, AutoLaunch daemons (fetched 2026-11-21)
- https://github.com/vercel/hyper/blob/canary/PLUGINS.md — plugins/localPlugins arrays, hook dev workflow (fetched 2026-11-21)
- https://github.com/vercel/hyper/blob/canary/app/plugins/extensions.ts — the declared hook contract (~40 names) (fetched 2026-11-21)
- https://github.com/vercel/hyper-site — Hyper Store submission pointer, plugins.json (fetched 2026-11-21)
- https://docs.tabby.sh — tabby-core API, Angular DI multi-provider plugin structure (fetched 2026-11-21)
- https://github.com/Eugeny/tabby/blob/master/README.md — Settings-UI plugin install, plugin examples (fetched 2026-11-21)
- https://man7.org/linux/man-pages/man1/tmux.1.html — tmux STATUS LINE/FORMATS: `#(...)` substitution and status-interval (fetched 2026-11-21)

## Unverified

- **tmux man-page wording**: the man7 page is too large to fetch end-to-end; the `status-interval` default (15 s), "zero disables redrawing", and the cached-`#()`-per-interval behavior were surfaced via search results quoting the man page rather than read directly from the full STATUS LINE/FORMATS sections. The mechanism itself (`#(...)` in `status-left`/`status-right`, refreshed on `status-interval`) is standard man-page content but the exact sentences were not directly quoted from the fetched page.
- **Zellij permission caching**: that granted permissions are cached keyed by the plugin's location/path comes from zellij issues #4982 and #5627 surfaced in search results; I did not fetch those issues directly, and the permissions doc page does not describe the cache.
- **iTerm2 API opt-in**: that the Python API must be enabled in iTerm2 preferences (and any per-connection approval) was not verified from a fetched page; the overview/tutorial pages fetched do not cover the enabling/permission flow.
- **Hyper npm install mechanics**: PLUGINS.md documents the `plugins` array and local dev flow, but the exact install path for npm plugins (e.g. automatic `npm install` on config change) was not verified from a current primary page; the canary README no longer has a Plugins section.
- **Warp custom completions**: announced as "in progress" in January 2024; I found no primary source showing they shipped, but I also did not exhaustively sweep every 2025–2026 changelog entry.
- **Ghostty "nightlies" file-protocol/other scripting features** mentioned in one search result were not verified against primary sources and are excluded from the findings.
