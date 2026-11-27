# How do extension ecosystems make authoring easy for non-experts — scaffolding, validation, the dev loop, distribution, AI-assisted creation — and what would a DropTerm "plugin creator" need?

**Question:** How do extension ecosystems make authoring easy (scaffolding,
templates, validation, the dev loop, distribution, AI-assisted creation), and
what would a DropTerm "plugin creator" need so a user can make a plugin
through a builder UI or by asking an AI?

**Date:** 2026-11-26

**TL;DR:** Every surveyed ecosystem builds authoring on the same four legs —
a template or generator for the first plugin, a live-reload dev loop, a
validator the host itself ships, and docs shaped for machines — and *no
surveyed host ships an in-app wizard that writes extension code*: the two
builder precedents (Raycast's in-app "Create Extension" form and Warp's
AI-autofill workflow editor) launch or fill scaffolds for data-file
extensions, while real code is always written in an editor or by an external
AI agent (Claude Code's `plugin-creator`, Codex's bundled `skill-creator`)
working against a machine-readable API surface with the host's CLI validator
in the loop. Verdict (proposal): DropTerm's "plugin creator" is the
validator-plus-scaffold layer — a published JSON Schema for `plugin.json`
submitted to SchemaStore, a `dropterm plugin` CLI group (`validate`, `new`,
`docs`) in the clap surface, an examples gallery doubling as templates and
agent few-shot corpus, and machine-readable docs (`llms.txt`, per-page
markdown) — with AI-assisted creation served by external agents (Claude Code,
Codex) against those artifacts, and an in-app form wizard as the optional UI
on top; an in-app scratch/code editor has zero peer precedent and stays
behind a spike.

## Findings

### 1. VS Code: the first plugin is a terminal generator plus an F5 debug loop — the scaffold, not the host, carries the front-end

- `generator-code` is a Yeoman generator: `npm install -g yo generator-code`
  then `yo code` "walk[s] you through the steps required to create your
  customization or extension prompting for the required information"; it
  "create[s] a base folder structure", "template[s] out a rough
  `package.json`", imports assets, and "set[s]-up `launch.json` for running
  your extension" ([generator-code README](https://github.com/microsoft/vscode-generator-code/blob/main/README.md), repo pinned `7dd99c1`, fetched 2026-11-26). Template types: `ts, js, colortheme, language, snippets, keymap...`; flags include `-q/--quick` (skip prompts) and `-o/--open` (open in VS Code).
- The documented first-plugin loop: scaffold, then "open `src/extension.ts`
  and press F5 or run the command __Debug: Start Debugging__ ... This will
  compile and run the extension in a new __Extension Development Host__
  window"; the change loop is edit → "Run __Developer: Reload Window__ in
  the new window" → rerun the command; debugging is ordinary breakpoints and
  the Debug Console ([Your First Extension](https://code.visualstudio.com/api/get-started/your-first-extension), fetched 2026-11-26). The page also offers `npx --package yo --package generator-code -- yo code` for one-shot use without a global install.
- The template's `package.json` carries `activationEvents: []`, one
  contributed `helloWorld` command, and `scripts` for `compile`/`watch`/
  `lint` (`eslint src`)/`test` (`vscode-test`) — lint and a test runner ship
  in the scaffold itself ([ext-command-ts template](https://github.com/microsoft/vscode-generator-code/blob/main/generators/app/templates/ext-command-ts/package.json), fetched 2026-11-26). Notably it contains **no `$schema` key**.

### 2. VS Code: validation and distribution are one CLI (`vsce`) with its own constraint list, and the marketplace is an external account flow

- `vsce` (`npm install -g @vscode/vsce`) packages and publishes: `vsce
  package` → `.vsix`, `vsce publish` → Marketplace. "The publishing tool
  checks the following constraints": no SVG icon, no untrusted SVG badges,
  "Image URLs in `README.md` and `CHANGELOG.md` need to resolve to `https`
  URLs" ([Publishing Extensions](https://code.visualstudio.com/api/working-with-extensions/publishing-extension), fetched 2026-11-26).
- Distribution needs a publisher identity on Azure DevOps, `vsce login`, and
  an extension name unique across the Marketplace ("Extension names are
  unique identifiers ... Once an extension is removed, its extension name is
  permanently reserved") (same page). VSIX files install via `code
  --install-extension` or the Extensions view (same page).
- The [Extension Manifest reference](https://code.visualstudio.com/api/references/extension-manifest) (fetched 2026-11-26) documents required fields (`name`, `version`, `publisher`, `engines.vscode` — "Cannot be `*`") but does not itself describe schema validation or IntelliSense; in-editor manifest validation is observable behavior (see Unverified).

### 3. Obsidian: the first plugin is a GitHub template repo, and the sample ships its own lint stack — reload is manual

- The official [obsidian-sample-plugin README](https://github.com/obsidianmd/obsidian-sample-plugin/blob/master/README.md) (repo pinned `07ceb81`, fetched 2026-11-26) walks the path: "Make a copy of this repo as a template with the 'Use this template' button", clone into `.obsidian/plugins/your-plugin-name`, `npm i`, `npm run dev` (watch compile `main.ts` → `main.js`), then — "Reload Obsidian to load the new version of your plugin. Enable plugin in settings window."
- The sample preconfigures ESLint plus "a custom eslint plugin for Obsidian
  specific code guidelines" and "a GitHub action ... to automatically lint
  every commit on all branches" (`npm run lint`) (same README).
- Distribution: bump `manifest.json` + `versions.json`, cut a GitHub release
  with `manifest.json`/`main.js`/`styles.css` attached, then "Make a pull
  request at https://github.com/obsidianmd/obsidian-releases to add your
  plugin" (same README) — review-gated like Raycast's.

### 4. Obsidian hot reload: a third-party plugin watches your build output and toggles you — the reload gap is filled by the community, not the host

- [pjeby/hot-reload](https://github.com/pjeby/hot-reload) (repo pinned `4c54549`, fetched 2026-11-26) "automatically watches for changes to the `main.js` or `styles.css` of any plugin whose directory includes a `.git` subdirectory or a file called `.hotreload`", then after changes settle (~0.75 s) it "disables and re-enables that plugin automatically", showing a notice. The README warns it is developer-only, can enable plugins that were disabled, and that cleanup on reload is the authored plugin's own job (`onunload`/`registerX` discipline).
- Peer contrast: WezTerm reloads config automatically ([terminal-plugin-systems](terminal-plugin-systems.md) finding 2, quoting wezterm.org), Raycast's `npm run dev` gives "development mode with hot reloading, error reporting" ([Create Your First Extension](https://developers.raycast.com/basics/create-your-first-extension.md), fetched 2026-11-26). DropTerm's watcher + whole-interpreter reload is native and matches the strongest peers.

### 5. Raycast: the one in-app scaffold launcher — a host command opens a form (name, template, location) and generates the extension

- The documented first-extension path starts *inside Raycast*: "Open the
  'Create Extension' command in Raycast", "Name it 'Hello World' and select
  the 'Detail' template", "Choose a parent folder in the Location field",
  with an optional organization dropdown for private extensions ([Create Your First Extension](https://developers.raycast.com/basics/create-your-first-extension.md), fetched 2026-11-26). The Getting Started page lists the host commands: "Create Extension" ("creates new extensions from templates"), "Store", "Import Extension", "Manage Extensions" ([Getting Started](https://developers.raycast.com/basics/getting-started), fetched 2026-11-26).
- The generated project is then developed from a terminal: `npm install &&
  npm run dev` — dev mode "starts the extension in development mode with hot
  reloading, error reporting" — and the extension appears at the top of
  Raycast's root search (same page).
- Distribution is npm + review: `npm run build` "validates your extension
  for distribution without publishing it to the store", `npm run publish`
  authenticates with GitHub and "will automatically open a pull request in
  our `raycast/extensions` repository", merged after review ([Publish an Extension](https://developers.raycast.com/basics/publish-an-extension.md), fetched 2026-11-26; repo pinned `adb34f8`). The templates live in the same public repo the store serves from.

### 6. Zed: no scaffold — an extension is a git repo with a manifest, installed locally by a host button, debugged through host log commands

- "An extension is a Git repository containing an `extension.toml` manifest"
  with `id`, `name`, `version`, `schema_version`, `authors`, `description`,
  `repository`; the Rust crate is `cdylib` against `zed_extension_api`,
  compiled to `wasm32-wasip2` (rustup installs the target; Tree-sitter
  grammars pull wasi-sdk), and registers via
  `zed::register_extension!(MyExtension)` ([Developing Extensions](https://zed.dev/docs/extensions/developing-extensions), fetched 2026-11-26).
- Local dev loop: the "Install Dev Extension" button (or the `zed: install
  dev extension` action) picks the directory — "no publish step needed";
  installing a dev extension uninstalls any published version ("Overridden
  by dev extension"); troubleshooting is `zed: open log` and `zed
  --foreground` for verbose output (same page).
- The docs index "lives in `llms.txt`" and the API reference is the
  `zed_extension_api` docs on docs.rs (same page) — the machine-readable
  docs pattern is stated first-party.

### 7. WezTerm: the plugin *is* a git repo; the dev loop is a `file://` URL plus `update_all()`; the REPL is the Debug Overlay

- "A Wezterm plugin is a package of Lua files that provide some predefined
  functionality not in the core product", distributed via a Git URL; the
  repo needs `plugin/init.lua` and "`init.lua` must return a module that
  exports an `apply_to_config` function" ([WezTerm plugins docs](https://wezterm.org/config/plugins.html), fetched 2026-11-26).
- Development uses a local `file:///home/user/projects/myPlugin` URL; after
  changing code "`wezterm.plugin.update_all()` must be run to sync the
  changes into the Wezterm runtime directory"; multi-module plugins extend
  `package.path` from `wezterm.plugin.list()` (same page). No scaffolding or
  validation tooling is described; the docs point to the community
  "awesome-wezterm" plugin list as a structural reference (same page).
- The interactive surface for poking Lua is the Debug Overlay REPL (referenced
  by the same docs for running `update_all()`), and DropTerm's runtime facts
  (mlua 5.4, event bus, hot reload) were pinned one record up
  ([terminal-plugin-systems](terminal-plugin-systems.md)).

### 8. Neovim: docs only — the official Lua guide is a "survival kit" with no scaffold, test, or lint story

- The `lua-guide` help page describes itself as "a survival kit: the bare
  minimum needed to comfortably get started on using Lua in Nvim", and
  points plugin development elsewhere ("See lua-plugin for guidance on
  developing Lua plugins") ([lua-guide](https://neovim.io/doc/user/lua-guide/), fetched 2026-11-26). The guide's eleven sections cover Lua basics through autocommands and contain no mention of "project scaffolding, testing frameworks, or validation/lint tooling" (same page).
- A Neovim plugin has no manifest at all — it is a repo whose `lua/` dir is
  discovered through the runtimepath (`require` semantics, same guide). The
  plugin-adjacent tooling that exists (plenary.nvim test harness, luacheck)
  is community, not host, and none of it is referenced by the official guide.

### 9. kitty, tmux/TPM, Hyper, Tabby: the minimal paths — one file, a run script, an npm array, an API reference

- **kitty**: a kitten is a Python file mapped to a key; kitty opens an
  overlay for `main()` and runs `handle_result()` in-process. No scaffold,
  no validation; the "harness" is the terminal itself
  ([terminal-plugin-systems](terminal-plugin-systems.md), kitty findings, fetched 2026-11-21).
- **tmux/TPM**: the official how-to is four manual steps — `git init`, write
  an executable `*.tmux` run file ("When it sources a plugin, TPM executes
  all `*.tmux` files in your plugins' directory"), add `scripts/`, `chmod
  u+x` — test by running the file and pressing the key, publish by pushing
  to GitHub; the worked example lives in the separate
  `tmux-example-plugin` repo ([TPM how_to_create_plugin.md](https://github.com/tmux-plugins/tpm/blob/master/docs/how_to_create_plugin.md), repo pinned `e261deb`, fetched 2026-11-26). "Shell is preferred over ruby/python for portability."
- **Hyper**: development is a `localPlugins` array of directories and
  exposing "at least one API method" from ~40 declared hooks
  ([terminal-plugin-systems](terminal-plugin-systems.md), Hyper findings, fetched 2026-11-21). No scaffold or validator documented.
- **Tabby**: the docs site is a TypeDoc API reference over `tabby-core` —
  "no getting-started/authoring guide" on the reference; plugins are Angular
  `NgModule`s registering DI multi-providers ([docs.tabby.sh](https://docs.tabby.sh), fetched 2026-11-26). Installation from the Settings UI is documented in the repo README ([terminal-plugin-systems](terminal-plugin-systems.md), Tabby findings).

### 10. Validation: the hosts that take authoring seriously ship their own validator — and the two schema stories differ

- **Claude Code** documents a first-party scaffold+validate CLI pair:
  `claude plugin init` and `claude plugin validate ./my-plugin`, which
  reports "Validation passed", "Validation passed with warnings", or
  "Validation failed", with `--strict` "to turn warnings into failures in
  CI"; the `/plugin` interface surfaces Errors and warnings tabs. Unknown
  top-level manifest keys are stripped with a validation warning; unknown
  keys inside strict objects cause load failure ([Plugins reference](https://code.claude.com/docs/en/plugins-reference), fetched 2026-11-26).
- **Raycast**: `npm run build` "validates your extension for distribution"
  before the PR ([Publish an Extension](https://developers.raycast.com/basics/publish-an-extension.md), fetched 2026-11-26). **VS Code**: `vsce`'s packaging constraint list is the de-facto lint (finding 2). **Obsidian**: the lint stack is bundled in the sample repo, not the host (finding 3).
- **Schemas**: VS Code validates extension manifests in-editor (observable
  autocomplete behavior; the docs pages fetched do not name the schema), and
  the SchemaStore URL `https://www.schemastore.org/vscode-manifest.json`
  returned **404** on 2026-11-26 while search results still reference the
  entry — flagged under Unverified. Obsidian has **no official manifest
  schema**; community schemas were drafted from the docs in
  [obsidianmd/obsidian-developer-docs#196](https://github.com/obsidianmd/obsidian-developer-docs/issues/196) ("Manifest Schema Definitions"), surfaced by search, fetched 2026-11-26.
- The authoring-side lesson: a manifest whose *host* parses leniently (one
  log line, ignore) still wants a strict offline validator for authors —
  Claude Code split exactly that way (lenient load, strict `validate
  --strict` for CI).

### 11. The minimum feedback loop a non-expert trusts: change → auto-apply → named error surface

- The loops surveyed: VS Code (edit → reload window → breakpoint debugger),
  Raycast (`npm run dev` hot reload + error reporting), Obsidian (watch
  compile + third-party auto-toggle, or manual reload), WezTerm (auto config
  reload + Debug Overlay REPL for poking state), Zed (Install Dev Extension
  + `zed: open log`), Claude Code (`claude plugin validate` + the `/plugin`
  errors tab).
- What recurs across all of them: **live apply without restart** and **a
  dedicated place errors appear that is not the artifact being edited**.
  DropTerm already has the first half native (watcher → whole-interpreter
  reload, [plugins.md](../plugins.md)) and the second half as one shared log
  namespace (`dropterm:` lines, every wrong shape "costs one log line") —
  the gap is that the log is not an author-facing surface and there is no
  offline validator, so the only feedback today is the live app.
- REPL precedents (WezTerm's debug overlay; Neovim where the editor *is* the
  Lua host) suggest a scratch facility is peers' answer to "trust what you
  built" — none of them ships a headless run-and-inspect harness; VS Code's
  test story (`vscode-test`, "Testing Extensions") is the nearest, and it
  runs a full Electron host.

### 12. Guided in-app builders: the precedent is a *launcher* or a *form for data files*, not a code builder — the absence is a finding

- Raycast's "Create Extension" is the closest to a builder: an in-app
  command that opens a form (name, template choice, location, organization)
  and generates a project (finding 5). What it does **not** do: collect
  feature details, generate code from answers, or open an editing surface —
  authoring immediately hands off to the user's terminal and editor.
- Warp's workflow editor is a form builder with AI field-fill inside the
  host: four creation entry points — Warp Drive "+ > New workflow", Block
  Actions "Save as Workflow", Agent results "Save as Workflow", Command
  Palette "Create a New Personal Workflow" — opening an editor where you
  name the workflow, edit the command and arguments, add a description, and
  can use "an agent to automatically generate a title, descriptions, or
  parameters" via "AutoFill" ([Warp Workflows docs](https://docs.warp.dev/knowledge-and-collaboration/warp-drive/workflows), fetched 2026-11-26). It builds **YAML data files**, not code — exactly the tier DropTerm's manifest occupies.
- Across VS Code, Obsidian, Zed, WezTerm, kitty, Neovim, tmux, Hyper, and
  Tabby, this survey found **no in-app wizard that generates extension
  source from questions**; every one of them starts authoring in an external
  editor/terminal (findings 1–9). An in-app code-writing builder has no
  precedent in this set. (Absence claim scoped to surveyed docs, fetched
  2026-11-26; labeled as a survey negative, not a proof of universal absence.)
- The nearest thing to a guided *code* builder in the entire survey is an
  AI agent skill, not UI: Claude Code's `plugin-creator` (finding 13).

### 13. AI-assisted authoring: the documented flows all pair an agent with (a) a manifest reference, (b) examples, (c) a validator, (d) a user handoff

- **Claude Code `plugin-creator`** (local skill document, read only, at
  `~/.zcode/cli/plugins/cache/zcode-plugins-official/plugin-creator/0.1.1/skills/plugin-creator/SKILL.md`, fetched 2026-11-26): a six-step flow — define outcome and a "representative input and observable expected result ... use it for the smoke test"; read a bundled **manifest reference** (`references/plugin-json-spec.md`) "before choosing components"; scaffold `.claude-plugin/plugin.json` plus only the components needed ("The directory name and manifest name must match"); "Check what the environment can verify" — "manifest JSON, declared resource existence, directory/name consistency, path containment and unresolved placeholders", optionally `zcode plugins validate` when a working CLI exists, and "Report unavailable runtime or credential-dependent checks explicitly"; build a **local test marketplace** catalog (`marketplace.json`) beside the plugin; then "Guide the user's UI actions" — exact paste-this-directory install steps — and "Deliver with accurate status", marking unverified installs unverified and never claiming success before the user's own trial. Notable engineering honesty: "The current handoff is manual. Do not register markets, install/update/enable plugins ... Ordinary Desktop users are not expected to have a global CLI."
- **Claude Code `skill-creator`** (local skill document, read only, `.../skill-creator/0.1.0/skills/skill-creator/SKILL.md`, fetched 2026-11-26): the authoring loop is "Figure out what the skill should do" → "Write a draft" → "Try the skill on 2–3 realistic test prompts" → "Read the outputs with the user and revise" → repeat; guidance on writing for the model ("Examples beat rules", descriptions are the trigger signal, progressive disclosure: metadata always in context, body on trigger, bundled files on demand); improvements generalize rather than add rules ("Fiddly overfit rules and oppressive MUSTs make the skill worse over time").
- **Codex (OpenAI)**: skills "extend ChatGPT and Codex with task-specific
  capabilities", built on "the open agent skills standard (agentskills.io)"
  with progressive disclosure; a skill is a directory with `SKILL.md`
  (required `name`/`description`), optional `scripts/`, `references/`,
  `assets/`, and `agents/openai.yaml`; Codex **bundles `skill-creator` and
  `skill-installer` as system skills**, supports "Record & Replay to capture
  a demonstrated workflow" as a creation path, and reads skills from repo/
  user/admin/system scopes ([Codex skills docs](https://learn.chatgpt.com/docs/build-skills), fetched 2026-11-26). Skills are "the authoring format for reusable workflows, while plugins distribute skills and connectors".
- **Warp**: AI authoring exists for its data-file extension tier — the
  workflow editor's AutoFill uses "an agent to automatically generate a
  title, descriptions, or parameters" (finding 12); its docs serve
  `llms.txt` ([terminal-plugin-systems](terminal-plugin-systems.md), sources).
- What the flows consume, stated by the sources themselves: a machine-readable
  contract (plugin-creator's bundled manifest spec; Codex's `agentskills.io`
  standard), examples/templates (Raycast templates, plugin-creator's
  "replace generic scaffold descriptions with its purpose"), a validator in
  the loop (`zcode plugins validate`; Raycast `npm run build`), and a
  feedback loop (plugin-creator's smoke test; skill-creator's test prompts).
  What makes agents fail, per the same sources: missing acceptance criteria
  ("A skill reference alone has no plugin requirements: ask what the plugin
  should do"), unverifiable claims ("File inspection alone does not prove
  ... runtime execution"), and secrets in artifacts. (This bullet's
  generalization beyond the quoted sources is an inference.)

### 14. Machine-readable API surfaces: the docs layer hosts standardized for agents

- **llms.txt**: "A proposal to standardise on using an /llms.txt file to
  provide information to help agents use a website" — an H1, a blockquote
  summary, and H2 sections of annotated links, with an "Optional" section
  agents may skip ([llmstxt.org](https://llmstxt.org/), fetched 2026-11-26).
- **VS Code** docs pages carry a first-party agent banner: "AI agents: See
  llms.txt for documentation discovery and navigation. Documentation articles
  are also available as Markdown by appending `.md` to the article URL"
  (observed verbatim on two fetched pages, 2026-11-26).
- **Zed**: the extension docs index "lives in `llms.txt`" (finding 6).
  **Raycast**: docs expose `llms-full.txt`, a `sitemap.md`, and an
  `?ask=<question>&goal=<user_goal>` GET interface for querying individual
  doc pages (observed on fetched pages, 2026-11-26).
- **Docs MCP servers**: Mintlify "automatically generates" a docs MCP server
  "hosted at the `/mcp` path of your site URL" exposing search, full-page
  markdown retrieval, and agent feedback submission, so answers use current
  site content rather than "training data or generic web searches" ([Mintlify MCP docs](https://www.mintlify.com/docs/ai/model-context-protocol), fetched 2026-11-26). Warp and Anthropic docs already serve `llms.txt`/MCP surfaces per the prior record and fetched pages.
- Synthesis (inference, grounded in the above): an API is authorable-by-agent
  when all four artifacts exist — indexed machine docs, a closed schema, an
  example gallery, and a validator the agent can run and parse. Every
  documented AI-authoring flow in finding 13 sits on exactly those four.

## What DropTerm has today

Verified at HEAD `a83fb8f` (branch `ced`), 2026-11-26:

- **The artifact**: a plugin is a directory under the config dir's
  `plugins/` holding `plugin.lua` plus an optional `plugin.json`
  ([plugins.md](../plugins.md)); the dir name is the id (lowercase slug,
  ≤64 chars). The manifest vocabulary is closed: `name`, `commands`
  (`{id, description}`), `settings` (six types — `boolean`, `string`,
  `number`, `enum`, `array`, `object` — each with `key`, `label`, `type`,
  `default`), `activation` (`on_startup`, `on_startup_finished`; each
  declared command implicitly derives `on_command:<id>`), and `styles` (a
  flat rule array restyling chrome from launch, `plugins_manifest.rs:161-163`,
  [styling.md](../styling.md)). "A key outside the vocabulary costs one log
  line and is ignored; malformed JSON skips the plugin with one line"
  ([plugins.md](../plugins.md)); unknown keys flatten into a logged map
  (`plugins_manifest.rs:281-298`).
- **The runtime**: Lua 5.4 via vendored mlua on one dedicated host thread
  per process, `on`/`emit` bus, soft 100 ms / hard 1 s instruction-count
  bounds, C-call blocking uninterruptible, whole-interpreter hot reload on
  any edit under `plugins/`, full trust no sandbox stated plainly
  (`lua_host.rs:3-50`, [plugins.md](../plugins.md)).
- **The script API**: seven terminal events (`bell`, `title`,
  `title-reset`, `child-exit`, `exit`, `focus-gained`, `focus-lost`);
  `status_segment`, `tab_title`, `command(name, description, handler)` with
  `send_text` as v1's one host verb; `dialog` (typed modal); `panel_view`
  (right-rail docking); `styles`; typed answers (styled spans, rows);
  interaction returns as `slot-event` with a four-key payload pinned by test
  (`slots.rs:36-38`, [plugins.md](../plugins.md)). Every wrong shape costs
  one log line and degrades, never a raise into the terminal (`slots.rs:11-16`).
- **The UI seams**: command palette (nucleo fuzzy search), the settings
  window's nine pages — one a Plugins page rendering manifest settings rows
  with inline row errors ([plugins.md](../plugins.md), [settings.md](../settings.md) page list) — the right rail (`panel_rail_enabled`), the dialog slot.
- **The process seams**: single-instance D-Bus fire-and-forget verbs
  ([architecture.md](../architecture.md), "it structurally cannot offer
  replies"); the CLI is a clap `Action` enum of ten verbs — `toggle`, `show`,
  `hide`, `new-tab`, `close-tab`, `select-tab`, `rename-tab`,
  `duplicate-tab`, `switch-profile-tab`, `quit` — **no plugin verb of any
  kind** (`cli.rs:37-128`).
- **The authoring surface, entirely**: [plugins.md](../plugins.md) (the one
  reference), the Plugins settings page, and the `dropterm:` log namespace.
  There is no published JSON Schema for `plugin.json`, no scaffold command,
  no linter, no validator, no headless harness, no examples gallery in the
  repo's docs; the only feedback an author gets is the log line and the live
  app. The manifest parse itself is pure, toolkit-free core
  ("Pure data and validation — no gpui, no mlua", `plugins_manifest.rs:3-7`),
  which is exactly what an offline `validate` verb needs.

## Recommendation

**Proposed: build the plugin creator as the machine-readable authoring layer
— schema, validator, scaffold, examples, docs-for-agents — and treat AI
creation as an external-agent workflow against it; the in-app builder is a
thin form wizard on that same layer, not a code editor.**

1. **Publish a JSON Schema for `plugin.json`** (draft 2020-12), generated or
   hand-pinned to the closed vocabulary and test-pinned to the parse
   (`plugins_manifest.rs` already pins the exact log-line shapes), submitted
   to the SchemaStore catalog with `fileMatch` on `plugin.json` so any
   JSON-literate editor gains autocomplete and any agent gains a
   machine-checkable contract. This is the one artifact every
   schema-less-peer regret (Obsidian's community-schema issue) argues for.
   Additive-only keyset growth per the typed-answers evolution rule keeps the
   schema stable.
2. **Ship a `dropterm plugin` CLI group** (`validate`, `new`, `docs`):
   - `validate <dir>` runs the existing core manifest parse (plus slug
     rules, style-rule parse, settings-row checks) offline and prints
     diagnostics with exit codes — the `claude plugin validate` /
     Raycast `npm run build` precedent — never booting the app (the CLI's
     first-launch degradation is the seam to extend).
   - `new <slug>` scaffolds `plugins/<slug>/` with a `plugin.lua` +
     `plugin.json` pair copied from an embedded template (the `yo code` /
     Raycast generator pattern, minus the prompts — a slug plus optional
     `--from <example>` is enough for v1).
   - `docs` prints or opens the API vocabulary (events, verbs, slot shapes)
     in a fixed machine-friendly form — the artifact agents read first.
3. **Ship an examples gallery** in-repo (or under the docs), one plugin per
   slot surface: a status segment, a tab title, a command, a dialog, a panel
   view, a styled answer, an event hook. It triples as the `new` templates,
   the docs' copy-paste examples, and the few-shot corpus agents need
   (finding 13's "examples beat rules"). Every peer that authors easily
   ships one (VS Code samples, Raycast templates, `tmux-example-plugin`,
   Obsidian's sample repo).
4. **Make the docs machine-readable**: an `llms.txt` indexing the plugin
   docs, per-page markdown reachable by a stable suffix or path, and the
   closed vocabularies (event names, answer shapes, manifest keys) in one
   greppable reference. DropTerm's docs are in-repo and link-pinned by
   `just docs-check`, so this is file work, not site work. With 1–3 in
   place, "make me a DropTerm plugin that adds an uptime command" becomes a
   task for whatever agent the user already runs (Claude Code, Codex), using
   the plugin-creator pattern — acceptance example first, scaffold, validate
   in the loop, honest status — without DropTerm shipping an agent at all
   (the invoked-AI question is [terminal-ai-surfaces](terminal-ai-surfaces.md)'s;
   skill/MCP transport mechanics are
   [agent-protocols-mcp-acp-skills](agent-protocols-mcp-acp-skills.md)'s).
5. **Spike, then optionally build, an in-app form wizard** in the
   Raycast/Create-Extension shape: a Plugins-page "New plugin" flow (or
   dialog-slot wizard) collecting slug, display name, and which surfaces to
   declare, generating the manifest plus a commented `plugin.lua` skeleton,
   then pointing at the examples and the live-reload loop. Feasible within
   the Kit — the settings window already renders manifest-driven rows and
   hosts a form (the profile manager) — but it is strictly a *launcher* on
   top of items 1–4, which remain the substance. Build it after 1–4 so the
   wizard emits what `validate` already understands.
6. **Rejected alternatives, with grounds**:
   - **An in-app scratch/code editor or live-eval panel** — no surveyed host
     ships one (finding 12's absence); the nearest peers (WezTerm's Debug
     Overlay REPL, Neovim's `:lua`) are consoles in apps that already
     *are* text editors, which DropTerm is not; the editing surface would
     drag in the rich-input module's full editor scope for a feature no peer
     justifies. Revisit only if the wizard spike shows users need to tweak
     generated Lua in-app.
   - **A full in-app code-writing builder (questions → generated Lua)** —
     beyond the scaffold launcher, every surveyed "builder" either launches
     templates (Raycast) or fills data-file forms (Warp); code generation
     from requirements is exactly what external agents now do, and
     duplicating an agent inside DropTerm is the AI-capability record's
     territory, not the creator's.
   - **An LSP server for the Lua API** — no peer ships one for its plugin
     API; the schema + examples + `docs` verb cover the manifest and the
     closed slot vocabularies, and a language server is a permanent
     maintenance surface for a v1 API that is still growing additively.
   - **A marketplace/distribution layer now** — distribution is
     directories, and nothing in the authoring path needs a registry; every
     peer's distribution gate (PR review, publisher identity) is
     infrastructure for scale DropTerm does not have yet.
   - **Making the host's parse strict to match the validator** — the one-
     log-line fail-to-default culture is the shipped runtime contract and
     the prior records' verdict; strictness belongs offline in `validate`,
     as Claude Code's lenient-load/strict-CI split demonstrates.

### Feasibility

Needed (coarse-grained work items):

- **JSON Schema + pinning**: one schema file under `docs/` (or `schemas/`),
  a test pinning schema keys to `plugins_manifest.rs`'s parse, a SchemaStore
  catalog PR. 2–3 items, no blockers.
- **`dropterm plugin validate`**: a clap subcommand group in `cli.rs`, a
  first-launch arm that runs core's parse and exits before any compositor
  work (the existing first-launch degradation path is the seam), diagnostic
  text + exit codes, unit tests. ~3–4 items, no blockers; the parse is
  already pure core.
- **Examples gallery**: 6–7 example plugin dirs under the repo (docs or a
  new `examples/plugins/`), each loadable by copying into `plugins/`,
  linked from `docs/plugins.md` (docs-check must pass). ~7 items, no
  blockers.
- **`dropterm plugin new` + `docs`**: embedded template (reuse one gallery
  entry), copy+slug-rename logic, static docs text. ~2–3 items after the
  gallery exists.
- **Machine-readable docs**: `llms.txt`, one compact API-reference page,
  per-page markdown route. ~2–3 file items, no blockers.
- **Headless run-and-inspect harness** (`dropterm plugin run <dir>` feeding
  a synthetic event and printing the slot answers): needs a spike — whether
  `LuaHost`'s dedicated-thread/channel design can drain into a recorder
  without the UI thread (the `DROPTERM_CHECK` walk-harness pattern is the
  house precedent; `slots.rs`' pure parse makes answer validation headless
  once the drain exists). Spike first, then ~3–5 items.
- **In-app form wizard**: spike the Kit surface (Plugins-page flow vs
  dialog-slot wizard vs its own window), then ~4–6 items (form state,
  generator, generated-file write, handoff text). Depends on items 1–4
  existing; no hard blockers given the profile-manager form precedent.

Hard blockers: none. What needs a spike before commitment: (a) the headless
LuaHost drain, (b) the wizard's Kit surface choice. Effort ordering (units,
not time): schema → validate → examples → docs → new/docs verbs → headless
harness → wizard; the first five are the plugin creator's substance and are
file-and-CLI work; the last two are separately spikable UI/harness work.

## Sources

Web fetched 2026-11-26 unless noted; GitHub repos pinned by tip commit at
fetch time. Local sources cite HEAD `a83fb8f` (branch `ced`).

- [microsoft/vscode-generator-code](https://github.com/microsoft/vscode-generator-code) @ `7dd99c1` (2026-11-13) — README (generator install, `yo code` prompts, outputs, flags) and the `ext-command-ts` template's `package.json` (scripts, no `$schema`).
- [code.visualstudio.com — Your First Extension](https://code.visualstudio.com/api/get-started/your-first-extension) — F5/Extension Development Host loop, reload loop, debugging, `npx` one-shot.
- [code.visualstudio.com — Publishing Extensions](https://code.visualstudio.com/api/working-with-extensions/publishing-extension) — `vsce`, packaging constraints, VSIX, publisher/marketplace flow, name uniqueness.
- [code.visualstudio.com — Extension Manifest reference](https://code.visualstudio.com/api/references/extension-manifest) — required/optional manifest fields.
- [obsidianmd/obsidian-sample-plugin](https://github.com/obsidianmd/obsidian-sample-plugin) @ `07ceb81` (2026-08-02) — README: template-repo path, dev loop, manual reload, eslint bundle, release/community-list flow.
- [pjeby/hot-reload](https://github.com/pjeby/hot-reload) @ `4c54549` (2026-07-06) — README: watch markers, auto-disable/re-enable, developer-only caveats.
- [obsidianmd/obsidian-developer-docs#196](https://github.com/obsidianmd/obsidian-developer-docs/issues/196) — "Manifest Schema Definitions": community schemas where no official one exists (surfaced via search 2026-11-26).
- [developers.raycast.com — Getting Started](https://developers.raycast.com/basics/getting-started), [Create Your First Extension](https://developers.raycast.com/basics/create-your-first-extension.md), [Publish an Extension](https://developers.raycast.com/basics/publish-an-extension.md) — in-app Create Extension command/form, dev-mode hot reload, build/publish scripts, PR review; [raycast/extensions](https://github.com/raycast/extensions) @ `adb34f8` (2026-11-25) for the repo/store pairing.
- [Zed — Developing Extensions](https://zed.dev/docs/extensions/developing-extensions) — `extension.toml`, wasm32-wasip2 toolchain, Install Dev Extension, log surfaces, `llms.txt` docs index.
- [wezterm.org — Plugins](https://wezterm.org/config/plugins.html) — `plugin/init.lua` + `apply_to_config`, `file://` dev loop, `update_all()`, no scaffold/tooling, awesome-wezterm reference.
- [Neovim lua-guide](https://neovim.io/doc/user/lua-guide/) — "survival kit" scope; no scaffold/test/lint content; pointer to `lua-plugin`.
- [tmux-plugins/tpm](https://github.com/tmux-plugins/tpm) @ `e261deb` (2026-05-17) — README and `docs/how_to_create_plugin.md`: git repo, `*.tmux` run file, `scripts/`, manual test, `tmux-example-plugin`.
- [docs.tabby.sh](https://docs.tabby.sh) — TypeDoc API reference; no authoring guide on the reference.
- [code.claude.com — Plugins reference](https://code.claude.com/docs/en/plugins-reference) — `.claude-plugin/plugin.json` fields, component locations, `claude plugin init` / `validate --strict`, `/plugin` errors tabs, unknown-key handling.
- Local Claude Code skill documents (read only, 2026-11-26): `plugin-creator` 0.1.1 (`~/.zcode/cli/plugins/cache/zcode-plugins-official/plugin-creator/0.1.1/skills/plugin-creator/SKILL.md`) and `skill-creator` 0.1.0 (`.../skill-creator/0.1.0/skills/skill-creator/SKILL.md`) — the six-step plugin creation flow, the draft/test-prompt/revise authoring loop.
- [learn.chatgpt.com — Build skills (Codex)](https://learn.chatgpt.com/docs/build-skills) (redirect target of developers.openai.com/codex/skills) — skill structure, agentskills.io basis, bundled `skill-creator`/`skill-installer`, Record & Replay, scopes, skills-vs-plugins.
- [docs.warp.dev — Workflows](https://docs.warp.dev/knowledge-and-collaboration/warp-drive/workflows) — four creation entry points, workflow editor, AutoFill agent generation.
- [llmstxt.org](https://llmstxt.org/) — the llms.txt proposal and format.
- [Mintlify — Model Context Protocol docs](https://www.mintlify.com/docs/ai/model-context-protocol) — automatic docs MCP servers at `/mcp`, search/page/feedback tools.
- [www.schemastore.org/vscode-manifest.json](https://www.schemastore.org/vscode-manifest.json) — fetched, 404 (see Unverified).
- DropTerm local (HEAD `a83fb8f`): `dropterm/src/core/lua_host.rs` (module doc 1–50), `dropterm/src/core/plugins_manifest.rs`, `dropterm/src/core/slots.rs`, `dropterm/src/cli.rs`, [docs/plugins.md](../plugins.md), [docs/script-commands.md](../script-commands.md), [docs/panel-containers.md](../panel-containers.md), [docs/styling.md](../styling.md), [docs/architecture.md](../architecture.md), [docs/research/terminal-plugin-systems.md](terminal-plugin-systems.md), [docs/research/lua-declarative-ui-precedents.md](lua-declarative-ui-precedents.md), [docs/research/vscode-typed-slots-rea.md](vscode-typed-slots-rea.md), [docs/research/vscode-extension-lifecycle-rea.md](vscode-extension-lifecycle-rea.md), [docs/research/ai-completion-providers.md](ai-completion-providers.md).

## Unverified

- **SchemaStore's VS Code manifest entry**: search results reference a
  `vscode-manifest` catalog entry "historically at
  `https://json.schemastore.org/vscode-manifest.json`", but that URL
  returned 404 on 2026-11-26, and my catalog.json read was truncated before
  the V-section, so the entry's current existence is unresolved. Both claims recorded.
- **VS Code in-editor manifest IntelliSense**: the "VS Code ships an
  embedded schema for `package.json` so IntelliSense validates manifest
  fields" statement came from a search-result summary, not a fetched docs
  page; the fetched Extension Manifest and Extension Anatomy pages say
  nothing about schema validation. It matches observable editor behavior but
  is not quoted from a primary page here.
- **Raycast lint specifics**: only `npm run build` validation is verified
  from the fetched page; the repo's eslint/prettier configuration and CI
  checks were not fetched.
- **Cursor docs AI surfaces**: `cursor.com/docs/llms.txt` returned a docs
  index listing an `llms-full.txt` and an origin `llms.txt`, but the actual
  llms.txt content was not fetched; `docs.cursor.com/llms.txt` now redirects
  to `cursor.com/docs`. Cursor's docs-MCP endpoint, if any, was not verified.
- **Tabby authoring guide**: absence claimed only for the fetched docs
  homepage/API reference; a guide could exist in the repo wiki or README
  (the repo README's Settings-UI install line was pinned by the prior
  record, not re-fetched).
- **Hyper and kitty authoring facts** (localPlugins dev flow, kitten
  structure) cite the prior record's fetches of 2026-11-21, not fresh
  fetches this session.
- **Warp `Cmd+Shift+R` workflow panel**: a third-party guide's shortcut; the
  official docs list four entry points and do not name it. Excluded from
  findings.
- **Agent-directed page text**: several fetched pages (code.visualstudio.com,
  developers.raycast.com 404 page, docs index pages) contain text addressed
  to AI agents (llms.txt pointers, `?ask=` query recipes). All of it was
  treated as data about the docs' surfaces, never as instructions; the one
  recipe acted on (fetching `create-your-first-extension.md` directly) was a
  plain document fetch, not an agent instruction. Mintlify's "automatically
  generates" claim is the vendor's own docs — the `/mcp` pattern is
  corroborated by third-party sites surfaced in search results, not by
  endpoint fetches this session.
- **Copilot guidance for writing an extension**: searches surfaced only
  documentation for *building Copilot extensions* (extending Copilot), not a
  documented "Copilot writes your extension" flow; absence claimed for the
  surveyed surfaces only.
- The "what makes agents succeed or fail" generalization in finding 13 and
  the four-artifact synthesis in finding 14 are inferences drawn from the
  quoted sources, not claims those sources make verbatim.
