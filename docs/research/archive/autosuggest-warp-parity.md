# Research: how should DropTerm close the gap between its shipped autosuggestions and Warp's autocomplete — the interactive surface, spec breadth, dynamic values, fuzzy matching, and aliases — without breaking the no-injection verdict or the terminal-side architecture?

Date: 2026-11-02
Question: *Warp's autocomplete is a spec-driven popup menu with descriptions,
dynamic value candidates (branches, containers), fuzzy matching, and alias
resolution. DropTerm ships terminal-side ghost text from four merged
providers ([terminal-autocomplete](terminal-autocomplete.md), stages 1–2
complete). Which slices of the remaining Warp delta should DropTerm adopt,
in what order, and which must be rejected?*

## TL;DR

**DropTerm should add a second, explicit-trigger completion menu beside the
shipped ghost — Ctrl+Space to open, fuzzy-filterable, descriptions from spec
data, accept via Tab/Enter only while the menu is open (VS Code's scoped
mechanism, which leaves bare Tab owned by the shell) — and this supersedes
the 2026-10-17 record's "completion-spec menus … deferred indefinitely"
rejection on that record's own terms: its stated condition ("value arrives
only after inline suggestions prove the prompt-detection layer") is now met,
and the Tab-conflict objection now has a pinned, shipping solution.** The
ghost itself stays single-choice and strict-prefix — fish deliberately turns
fuzzy matching and descriptions *off* in autosuggestion mode, and those two
features belong to the menu surface. Spec breadth grows down the path the
stage-2 record already named: the curated table moves behind a loadable data
file, and the eventual broad library is derived from withfig/autocomplete —
MIT, 715 specs, the one source both Warp and VS Code build on — under the
per-content licensing audit; carapace as a runtime dependency and any Warp
AGPL client code are rejected. Dynamic value candidates split: file-derived
dynamics are allowed (git branches/tags by reading `.git/refs` in the tab's
live cwd — the path provider's own category), while **spawn-based generators
(`git branch`, `docker ps` run by DropTerm) are rejected for now** — Warp
executes them through a live shell session it owns and VS Code through
session env vars its shell integration reports; DropTerm has neither the
session env nor the shell's alias/function state, so a generator run by
DropTerm is a command DropTerm chose, in an environment it cannot reproduce.
Alias resolution is rejected on the same grounds (the expansion lives in the
user's shell state). AI stays rejected — re-verified: neither of Warp's two
completion pages mentions AI, cloud, or models today; its completions are
specs plus local generators.

This record **extends** [terminal-de-integration](terminal-de-integration.md)
(no shipped scripts, no env injection, no OSC engine fork — nothing here
needs that verdict revisited), **extends** the display-mode record
([autosuggest-display-mode](autosuggest-display-mode.md), 2026-10-24: the
menu is a separate surface, not a display mode), and **supersedes exactly one
ruling** of [terminal-autocomplete](terminal-autocomplete.md) (2026-10-17):
the phase-1 rejection of completion-spec menus. That record's "a menu is a
later, separate decision with zero regression risk to the inline path" is
the decision now made; both records stay on file. The stage-2 crosscheck's
named growth path ("the flag-specs table keeps a nameable path to a loadable
data file" — [autosuggest-stage2-crosscheck](autosuggest-stage2-crosscheck.md),
2026-10-21) is consumed here, not superseded.

## What DropTerm has today

Grounded in the working tree at HEAD `fe1cd72` (2026-11-01):

- **Four providers, merged first-match-wins in a fixed order** — History →
  Path → Specs → Catalog (`source_rank`, `src/terminal/suggest.rs:431-438`;
  the dispatch `merged_candidate`, `src/terminal/suggest.rs:850-865`).
  Exactly one suggestion or none; the ghost is single-choice by design.
- **The curated specs table**: five commands (git, cargo, docker, systemctl,
  npm), subcommands and long flags, static in-code (`SPECS`,
  `src/terminal/suggest.rs:575-689`; lookup `specs_candidate`,
  `src/terminal/suggest.rs:756-775`) — strict prefix, must extend, shortest
  wins, ties alphabetical; subcommands only at the first argument seat, long
  flags from any seat, no short flags, no descriptions, no per-subcommand
  flag trees. Gated behind the shell allowlist
  (`SHELL_ALLOWLIST`/`is_allowlisted_shell`,
  `src/terminal/suggest.rs:937-950`).
- **Match policy everywhere is strict prefix** — catalog `min_by_key(len)`
  (`src/terminal/suggest.rs:541-545`), specs the same
  (`src/terminal/suggest.rs:770-773`); no fuzzy tier, no case-insensitive
  tier (fish has one; DropTerm does not).
- **Guard chain and display gate**: enabled, focused, no foreground child,
  not scrolled, anchored, non-empty typed segment, then the idle-delay gate
  (`showing`, `src/terminal/suggest.rs:389-424`; the delay compare
  `:411-413`); ghost-fit clipping with `…` (`clip_visible`,
  `src/terminal/suggest.rs:1101-1122`); the dim theme-derived tint
  (`ghost_color`, `src/terminal/suggest.rs:1130-1144`).
- **Accept keys are soft and exactly two**: bare Right (whole) and Alt+Right
  (one fish-shaped word), only while a showing exists — `accept_kind`
  (`src/terminal/widget.rs:2383-2393`), routed ahead of the plain mapping
  (`src/terminal/widget.rs:1252-1259`); the tests pin that Ctrl/Shift-Right
  and Tab are never stolen (`src/terminal/widget.rs:2580-2604`).
- **The ghost renders as a pure overlay** after the text pass, never the
  grid (`src/terminal/widget.rs:920-970`) — the drawing precedent any popup
  reuses. `src/context_menu.rs` is the app-level libcosmic menu precedent
  (rows → existing actions).
- **Config surface**: `autosuggestions_enabled`,
  `autosuggestions_paths_enabled`, `autosuggestions_commands_enabled`,
  `autosuggestions_delay_ms` (`src/config.rs:408-451`); the specs provider
  is engine-level only (`autosuggestions_specs_enabled`,
  `src/terminal/engine.rs:127-133`) — still no key ("No toggle yet",
  `docs/autosuggestions.md:55`).
- **The Tab constraint, in the project's own words**: "Bare Ctrl+I must stay
  unbound: in a terminal it is the TAB byte (0x09), so swallowing it would
  break tab completion" (`src/keybinds.rs:90-92`).
- **Live cwd is readable without the shell**: the engine already readlinks
  `/proc/<pid>/cwd` (the session-save and new-tab-inherit mechanism, per
  [terminal-de-integration](terminal-de-integration.md) finding and
  `src/terminal/engine.rs` cwd sampling) — the fact file-derived dynamic
  candidates build on.

## Findings

### 1. Warp today — the delta decomposed against docs and, since April 2026, open source

Warp's two completion surfaces, read 2026-11-02
([autosuggestions](https://docs.warp.dev/terminal/command-completions/autosuggestions),
[completions](https://docs.warp.dev/terminal/command-completions/completions)):

- **Autosuggestions** (the ghost): "Warp will automatically suggest commands
  as you type based on shell history and possible completions." Accept
  →/Ctrl+F; partial Ctrl+→. No AI, no cloud, no model on the page — the
  2026-10-17 record's correction still holds.
- **Completions** (the menu): Tab opens a spec-driven popup with "fuzzy
  search capability" returning "approximate matches" — suggestions appear
  "even if it's not an exact match." The worked example is a dynamic value:
  `git checkout ` + Tab and "a menu will show all of your local branches."
  Two alias layers: shell aliases ("typing gc and hitting Tab should give
  you the same completion options" as the expanded command) and subcommand
  aliases (`git st` → `git status`'s completions). The Full-support table
  today: bosh, brew, cargo, conda, defaults, docker, firebase, flutter, gh,
  git, go, heroku, killall, kubectl, kubectx, kubens, make, man, ng, node,
  npm, nx, phpunit-watcher, pip, pyenv, react-native, ssh, tar, terraform,
  tmux, tmuxinator — everything else Partial. Auto-open is opt-in: the
  setting "Open completions menu as you type" (Settings > Features >
  Terminal Input); rebinding Tab away "will update the keybinding for
  opening the completions menu to Ctrl+Space."

Two facts upgrade this survey from docs-only to source-pinned:

- **Warp's client is open source since 2026-04-28** — "the Warp client is
  now open-source" ([warp.dev blog](https://www.warp.dev/blog/warp-is-now-open-source),
  2026-04-28), at [warpdotdev/warp](https://github.com/warpdotdev/warp)
  under **AGPL-3.0** (UI crates MIT; repo license API, checked 2026-11-02).
  The blog post does not mention completions. License consequence for
  DropTerm (GPL-3.0-only): AGPL client code cannot be copied in; only the
  spec *data* path is usable, and that path is not Warp's — the repo README
  credits "[Fig Completion Specs]
  (https://github.com/withfig/autocomplete)" as the open-source dependency
  its completions build on (`README.md` @ master `d57d9760`, read
  2026-11-02). Warp's own completions code (`crates/warp_completer/`) is an
  engine, not a data set.
- **Warp's fuzzy matching is a subsequence fallback, prefix first** —
  `MatchStrategy::Fuzzy` in `crates/warp_completer/src/completer/matchers.rs`
  @ master `d57d9760` (last touched `fc4d563b8b63`, 2026-08-27): case-
  insensitive exact/prefix wins; only then "it will try to find a fuzzy
  (i.e. approximate) match result," and "Only fall back to a subsequence
  match for a query with at least one alphanumeric character. Fuzzy matches
  on punctuation produce poor results." Descriptions are a first-class spec
  field rendered in the inspector (`describe.rs`: `Description { token,
  description_text, suggestion_type }`, same commit).

### 2. The surface precedent is now unanimous: always-on ghost + explicit menu — nobody auto-pops by default

Every system surveyed puts the interactive list behind an explicit trigger
(or a deliberate opt-in), keeping the zero-UI ghost as the ambient surface:

- **fish** (source @ master `bcccefe566dc93`, 2026-10-27): the ghost is one
  ranked completion — `sort_and_prioritize(&mut completions, complete_flags);
  let comp = &completions[0];` (`src/reader/reader.rs:5491-5492`). The menu
  exists only on Tab: `compute_and_apply_completions` → `handle_completions`,
  whose doc states the shell-classic split — "If the list contains one
  element, write the whole element…; If the list contains multiple elements,
  insert their common prefix, if any and display the list in the pager."
- **Warp**: ghost always-on; menu on Tab or opt-in as-you-type (finding 1).
- **VS Code** (`microsoft/vscode` @ main `67710841ad061a`, 2026-11-02): the
  popup's manual trigger is the *default* — `terminal.integrated.suggest.quickSuggestions`
  defaults to `{commands:'off', arguments:'off', unknown:'off'}`
  (`terminalSuggestConfiguration.ts:118-124`), `suggestOnTriggerCharacters`
  defaults `false` (:128); the manual trigger is Ctrl+Space ("To manually
  trigger the suggestions, use the ⌃Space (Ctrl+Space) keyboard shortcut" —
  shell-integration docs, read 2026-11-02). This sharpens the 2026-10-17
  record's "as you type, a list of suggestions will appear": that is the
  opt-in quick-suggestions mode, not the shipped default.
- **Clink** (docs v1.9.35, read 2026-11-02; repo @ master `4bce300b24758f`,
  2026-11-02): the interactive list is `clink-select-complete` on
  Ctrl+Space — "performs completion by selecting from an interactive list of
  possible completions" — with `match.max_rows`/`match.preview_rows` sizing;
  the inline ghost (`autosuggest.inline`) defaults **false** and hides
  "when the suggestion list is active." Correction to the task's premise:
  there is no `completion_popup` setting in current Clink; the popup/list
  family is `clink-select-complete` plus the F7-style popup lists with
  `clink.popup_search_mode` (find vs filter).
- **ble.sh** (@ master `152942758f567e`, 2026-10-31; wiki Manual §7): ghost
  on by default (`complete_auto_complete 1`,
  `lib/core-complete-def.sh:52`); the auto-menu is opt-in and idle-gated —
  `complete_auto_menu` defaults `''` (disabled; `:56`), and "When a
  non-empty string is set, auto-menu is enabled… ble.sh will automatically
  show the menu of completions after the idle time… reaches the delay"
  (wiki §7.3.6). Tab enters menu-complete only "when the menu is shown"
  (`complete_menu_complete`).

For DropTerm: the ghost is shipped and correct as the ambient surface; the
menu is the missing second surface, and it should ship explicit-trigger —
matching every peer's default, with ble.sh's idle-delayed auto-menu and
Warp's as-you-type both available as later opt-ins if ever asked for.

### 3. The Tab tension has a pinned, shipping solution — VS Code's scoped keybinding

DropTerm's own constraint is written down (`src/keybinds.rs:90-92`). VS Code
solves the same conflict with a *state-scoped* binding rather than a global
one — `terminal.suggest.contribution.ts:558-563` @ `67710841ad061a`:

```ts
{
    primary: KeyCode.Tab,
    // Tab is bound to other workbench keybindings that this needs to beat
    weight: KeybindingWeight.WorkbenchContrib + 2,
    when: ContextKeyExpr.and(SimpleSuggestContext.HasFocusedSuggestion)
},
```

Tab is swallowed only while the suggest widget is open with a focused item;
with no widget visible, Tab reaches the shell untouched. Enter carries
additional conditions (accept when explicitly invoked, or not in `partial`
selection mode, or after navigation — `:565-569`), the default
`selectionMode: 'partial'` being "Tab can be used to accept the first
suggestion, only after navigating the suggestions via Down will Enter also
accept" (`terminalSuggestConfiguration.ts:148-152`). DropTerm's widget
already routes soft keys exactly this way — `accept_kind` consults
`terminal.suggestion()` before swallowing Right (`src/terminal/widget.rs:1252-1259`)
— so "while the completion menu is open, Tab/Enter accept; otherwise Tab
falls through to `\x09`" is the same pattern with one more state bit. Warp's
version of the same trade: rebinding Tab to ghost-accept moves the menu
trigger to Ctrl+Space (docs, finding 1) — the menu trigger is the movable
piece, the shell's Tab never is.

### 4. Spec data — the whole industry builds on one dormant MIT repo; licenses verified

- **withfig/autocomplete** (checked 2026-11-02): not archived, **MIT**
  ("Copyright (c) 2021 Hercules Labs Inc. (Fig)", LICENSE via GitHub license
  API), 715 top-level `src/*.ts` specs, **dormant — last push 2025-05-05**
  ("ci: version bump to 2.692.3"), 247 open issues. The format is typed TS
  (`Fig.Spec`) — a small example, `src/mkdir.ts` @ master: `const
  completionSpec: Fig.Spec = { name: "mkdir", description: "Make
  directories", args: { name: "directory name", template: "folders",
  suggestCurrentToken: true }, options: [{ name: ["-m", "--mode"],
  description: "Set file mode (as in chmod)…", args: { name: "mode" } }, …] }`.
  Generators (dynamic candidates), templates (`folders`, `filepaths`,
  `history`, `help`), short+long options, per-option descriptions.
- **VS Code consumes it, pinned**: `extensions/terminal-suggest/cgmanifest.json`
  @ `67710841ad061a` registers withfig/autocomplete @ commit
  `1cc34dc1ba530bb620bd380c25cfb9eb2264be89` (version 2.684.0, MIT); the
  synced specs live in `extensions/terminal-suggest/src/completions/upstream/`
  (101 `.ts` files) beside Microsoft-authored specs (`git.ts`, `cd.ts`, …)
  under MIT headers. So the Fig format is the de-facto interchange format
  with three live consumers (VS Code, Warp, and Microsoft's
  [inshellisense](https://github.com/microsoft/inshellisense) — MIT, active
  2026-10-18, "a terminal native runtime for autocomplete which has support
  for 600+ command line tools").
- **Carapace** (checked 2026-11-02): `carapace-sh/carapace-bin` (redirected
  from `rsteube/`) is **MIT** — LICENSE "Copyright (c) 2020 rsteube",
  unchanged since the initial 2020 commit — *correction: the believed
  GPL-3.0 is wrong today*. 1,556 per-command completers under
  `completers/<platform>/<cmd>/`, actively maintained (pushed 2026-11-02).
  But it is a *completion binary* shells invoke; its completers are Go
  programs, not data. No terminal emulator embeds it (kitty's carapace
  mention is a completer *for* kitty; Ghostty 1.4.0's is a Nushell
  external-completer fix — finding 8). Using it means a runtime dependency
  on an external binary.
- **Warp's specs**: no public spec repo; the client (AGPL) credits Fig. The
  data path DropTerm could license-cleanly derive from is Fig's, not
  Warp's; Warp's own engine code is unusable under DropTerm's GPL-3.0-only.

License verdict table (the per-content audit precedent from
[terminal-theming](terminal-theming.md) applies): Fig specs MIT — usable
with attribution; VS Code's bundled specs MIT — usable; carapace-bin MIT —
usable as data *format* reference, rejected as runtime dependency; Warp
client AGPL-3.0 — not usable.

### 5. Dynamic values — who spawns what, and what it costs

- **Warp executes generator commands at the active pwd, through a session**:
  the engine's seam is `GeneratorContext::execute_command_at_pwd` —
  "Execute a given command at the active pwd. If no session exist in the
  source, return None."
  (`crates/warp_completer/src/completer/context/mod.rs` @ master
  `d57d9760`), and the app implements it by `session.execute_command(
  shell_command, self.pwd()…, run_command_in_same_shell_as_session…)`
  (`app/src/completer/mod.rs:213-243` @ `40b791c35137`, 2026-10-01). Note
  the env problem Warp already handles by force: "We need to run the
  command with the PATH var set explicitly… because if the user opened Warp
  through a parent process that didn't have the PATH var set… the subshell
  won't inherit the PATH var" (`app/src/completer/mod.rs:220-224`). Aliases
  come from the same session (`CompletionContext::aliases()` /
  `alias_command()`, `context/mod.rs`).
- **VS Code spawns too — the strongest terminal-side precedent**: Fig
  generator `script` arrays execute via `spawnHelper2(input.command,
  input.args, { env: input.env ?? fallbacks.env, cwd: input.cwd ??
  fallbacks.cwd, timeout })` with a default timeout of "osIsWindows() ?
  20000 : 5000" ms (`extensions/terminal-suggest/src/fig/execute.ts`,
  "About to run shell command '…'"), driven by
  `scriptSuggestionsGenerator.ts` (`getScriptSuggestions`). The bundled
  `git.ts` carries real generators: `script: ["git", "--no-optional-locks",
  "log", "--oneline", "-n", "1000"]` (commits), `script: ["git",
  "--no-optional-locks", "config", "--get-regexp", "^alias."]` (aliases),
  `script: ["git", "rev-list", "--all", "--oneline"]`. Crucially, VS Code
  knows the cwd and prompt from its *shell integration* (OSC 633 P;Cwd),
  and the fallback env comes from the terminal's own process environment —
  context its architecture supplies and DropTerm's does not.
- **Termius does not do command-derived values today**: its sources are
  "command history, file and directory paths, built-in commands and
  arguments, and argument descriptions when available"
  ([docs.termius.com](https://docs.termius.com/terminal/autocomplete-and-shell-integration),
  read 2026-11-02); for remote hosts it "opens additional `exec` channels
  next to the main `shell` channel" to fetch paths/history, with the honest
  cost on the page: "Some servers reject `exec` channels and may drop the
  connection."
- **fish's dynamic candidates run inside the shell** (`complete -a
  '(…)'` command substitutions), which is exactly the architecture DropTerm
  does not have.

The objections, graded: (a) *side effects* — a spec file that says
`make clean` is indistinguishable from one that says `git branch` unless
DropTerm ships an allowlist of commands *and arguments*; VS Code's specs
mark dangerous *options* (`isDangerous: true` on `rm -f` in
`upstream/rm.ts`) but nothing gates the spawn itself. (b) *wrong
environment* — DropTerm's process env is the desktop session's, not the
tab's shell's: no rc-file aliases/functions, no direnv, possibly a stale
PATH; Warp patches PATH by hand and Termius pays with exec channels. (c)
*wrong directory* — solvable locally: `/proc/<shell_pid>/cwd` readlink is
the same local truth the path provider's gate already reasons about. (d)
*revocation/trust* — this is the first DropTerm feature where the terminal
would execute a command *it chose from a data file* rather than bytes the
user typed; the autosuggest invariant so far is "inert pixels until an
accept key sends it," and spawns break that frame.

### 6. Fuzzy matching and descriptions belong to the menu, not the ghost

Warp's fuzzy is a menu/list behavior (finding 1: subsequence fallback in
`matchers.rs`). fish *disables* fuzzy and descriptions precisely in
autosuggestion mode — `fn fuzzy_match(&self) -> bool { !self.autosuggestion() }`
(`src/complete.rs`, pinned in
[autosuggest-stage2-crosscheck](autosuggest-stage2-crosscheck.md) R2/R3) —
because a single silent choice that must be accepted whole pays a wrong
accept for an approximate guess. Descriptions are the same story: they are
rendered in menus and inspectors (Warp's `describe.rs`; VS Code's widget
details pane), never in ghost text. DropTerm should therefore keep both
out of the ghost (the shipped behavior is already correct) and add both
*with* the menu, where every peer has them. Within the menu, Warp's exact
policy is worth copying verbatim: prefix/case-insensitive first, subsequence
fallback only for queries with ≥1 alphanumeric character.

### 7. AI — still no vendor puts a model in the completion loop; Warp's open-sourcing changes nothing here

Re-verified 2026-11-02: neither Warp completion page mentions AI, cloud,
models, or telemetry — the mechanism is specs plus locally executed
generators. VS Code's suggest docs mention no AI (its AI features are
separate extensions; the `inlineSuggestion` setting only *integrates shell-
provided* ghost text such as PredictiveIntelliSense). Warp's 2026-04-28
open-sourcing post is agent-first ("agentic development environment") and
does not mention completions. DropTerm's standing local-only stance
([terminal-autocomplete](terminal-autocomplete.md) §8 privacy) is therefore
not merely defensible but the same line Warp and VS Code themselves draw
for this feature: completions are deterministic lookups; AI lives elsewhere.

### 8. Peer re-check since 2026-10-17 — the "no terminal-side peer" claims still hold, with version bumps

Checked 2026-11-02: **Ghostty** has shipped v1.4.0 (after v1.3.1) — no
completions feature; the release notes' only completion mention is "nu: Tab
completion for ssh and sudo now works with external completers such as
carapace" (shell integration for the `ghostty` CLI wrapper), and
`src/config/Config.zig` @ master `042b16b6146a` still contains zero
completion/autosuggest keys. **kitty** v0.49.2 (2026-10-01): unchanged,
still CLI-only completion. **Alacritty** still v0.17.0 (2026-04-06).
**WezTerm** @ `e68a8ec277` (2026-10-26): no completion/ghost code. **Windows
Terminal**: the Suggestions UI is still "only available in Windows Terminal
Preview" (actions docs, updated 2025-08-21) and `recentCommands` is still
"powered by shell integration." **Termius**: per finding 5. So DropTerm's
shipped feature remains without a terminal-emulator precedent; VS Code (an
editor) and Warp (an editor-paradigm terminal, now AGPL) remain the only
implementations with a menu, and Clink/ble.sh the line-editor analogues.

## Recommendation

**Phase A — the completion menu (the supersession).** A second surface,
explicit-trigger, drawing on machinery that all exists:

1. **Trigger**: a new `Action::ShowCompletions` bound to **Ctrl+Space**
   (VS Code and Clink's trigger; Warp's fallback), customizable through the
   existing keybind overlay (`src/keybinds.rs` defaults + custom model).
   Note the desktop caveat both VS Code documents (IME conflict) — a
   customizable default is the mitigation. The action is a *soft* trigger:
   with no completion context (the guard chain's own gates — not idle,
   scrolled, unanchored), it falls through to the shell byte.
2. **Eligibility** = the shipped guard chain verbatim (`showing`'s gates
   minus the single-candidate requirement): the menu opens only at an idle
   prompt with a typed segment, exactly where the ghost already computes.
3. **Contents** = the merged providers' *full candidate sets*, not the
   merged single choice: history prefix matches, path listing matches,
   spec subcommands/flags, catalog names — ranked by the same
   `source_rank` order, descriptions shown where the data has them (the
   loadable specs of phase B; the curated table gains descriptions as it
   grows). This is fish's Tab split (one ghost vs the whole ranked list)
   rendered terminal-side.
4. **Keys inside the menu**: Up/Down navigate; **Tab and Enter accept only
   while the menu is open** — the VS Code scoped-when mechanism, DropTerm's
   `accept_kind` pattern with a menu-open state bit; Esc/any other
   key/scroll/focus-loss closes and forwards. Bare Tab with the menu closed
   remains the shell's `\x09`, preserving `src/keybinds.rs:90-92` verbatim.
5. **Filtering**: Warp's pinned policy — case-insensitive prefix first,
   subsequence fallback only for queries containing an alphanumeric. The
   ghost stays strict-prefix single-choice, unchanged (finding 6).
6. **Rendering**: an overlay in the widget's `draw()`, the ghost overlay's
   own pattern (`src/terminal/widget.rs:920-970`) — background quad, rows,
   selection highlight, never the grid. `src/context_menu.rs` stays the
   app-menu precedent; this is a terminal-surface popup and belongs in the
   widget.
7. **Acceptance**: the chosen completion's suffix (minus the typed prefix)
   through `Terminal::input_no_scroll`, the ghost-accept path — the VS Code
   addon's synthesis model DropTerm already proved with Right/Alt+Right.
8. **Config**: `autosuggestions_menu_enabled` (default `true` — the ghost
   and the trigger are separable; `false` restores today's surface exactly),
   documented under the `every_config_key_is_documented` gate; the
   long-pending `autosuggestions_specs_enabled` key lands with phase B.

**Phase B — spec breadth down the named path.** Move `SPECS` behind a
loader: a versioned data file (TOML or JSON, one table per command —
subcommands, long flags, descriptions) built-in as compiled-in defaults
*plus* a user-writable location for overrides/additions, the exact shape
the stage-2 record reserved ("a loadable data file … without touching the
merge", `src/terminal/suggest.rs`'s growth-path comments at `:568-574`).
Breadth beyond hand-curation: a one-time offline conversion from
**withfig/autocomplete** (MIT) into DropTerm's minimal format — subcommands,
flags, descriptions, *without* generators/templates (see phase C) — for a
chosen top-N command set, shipped under the per-content licensing audit
with the MIT notice preserved, exactly as the theming record governs
schemes. Rejected alternatives for the data source: carapace as a runtime
binary (external dependency, Go-program completers, no terminal precedent),
parsing Fig TS at runtime (a TS parser in a Rust terminal), and any Warp
AGPL-derived table. The upstream dormancy (2025-05-05) is a maintenance
risk priced in: the conversion is a snapshot DropTerm owns, not a live
dependency.

**Phase C — dynamic values, split.** *Allowed: file-derived candidates* —
git branches and tags by reading `.git/HEAD`, `.git/refs/heads/**`, and
`.git/packed-refs` under the tab's live cwd (`/proc` readlink, the local
truth the shell gate already keys on), offered at the known argument seats
of the specs data (e.g. `git checkout <branch>`). No execution, no env
problem, wrong-directory solved by construction; the same category as the
path provider. *Rejected now: spawn-based generators* (`git branch`,
`docker ps --format`) — the security/correctness ledger of finding 5: no
session env to reproduce the user's shell (Warp runs generators through its
session and even then force-sets PATH; VS Code passes session env its
shell integration reports), no way to prove a data-file command read-only,
and it breaks the "inert until accepted" frame by making DropTerm the
actor. The named revisit trigger: a per-command user opt-in *and* an env
story (e.g. accepting a documented, user-invoked env-reporting mechanism —
which would be a new record superseding part of the no-injection verdict's
rationale, not this one). *Rejected with it: alias resolution* — aliases
live in shell state DropTerm cannot read without executing `alias` or
parsing rc files; note honestly that DropTerm's history seed cannot learn
them either (the grid sees the alias word, never the expansion).

**AI: rejected, confirmed.** No model, no network, no cloud fallback — even
Warp's completions, re-verified on both pages and in the open-source
engine, are deterministic spec+generator lookups; the industry's AI work
touches agents and shell-provided ghost text, not terminal-computed
completions.

## Rejected, and why

- **Auto-opening the menu as you type (the Warp opt-in, ble.sh's
  auto-menu).** Possible later as an opt-in setting; no peer ships it as
  the default (Warp opt-in, VS Code `quickSuggestions` all-off, ble.sh
  `complete_auto_menu` empty), and a persistent popup over a grid the
  terminal does not own is the exact hostility the 2026-10-17 record named.
  Explicit-trigger first.
- **Upgrading the ghost into a multi-choice or fuzzy surface.** fish turns
  fuzzy and descriptions *off* in autosuggestion mode by construction; a
  single whole-accept choice made approximately is a wrong accept waiting
  to happen. The menu is where multiplicity goes.
- **Shipping carapace as a completion backend.** A runtime external-binary
  dependency (zero terminal precedents; shells invoke it), Go completers
  rather than data, and an architecture where DropTerm would spawn a
  program to parse its output — the phase-C rejection with extra steps.
  Its MIT license is *not* the blocker (corrected premise); the dependency
  shape is.
- **Copying Warp engine code.** AGPL-3.0 into a GPL-3.0-only project is a
  one-way door that does not open this direction; and the engine is Warp's,
  while the *data* everyone shares is Fig's MIT.
- **Runtime TS/fig-spec parsing.** VS Code ships a TS toolchain; DropTerm
  would be embedding a TypeScript-shaped parser for data it can convert
  once, offline, under license audit.
- **Spawn-based generators and alias resolution now.** Finding 5's ledger;
  revisit only with a session-env story and per-command opt-in.
- **Terminal-side `~/.bashrc` alias scraping.** Fragile across five shells,
  wrong under ssh profiles, and buys the one thing (alias expansion) that
  still cannot complete correctly without the shell's own definition of
  the alias target.
- **Doing nothing (ghost-only forever).** The user ask is Warp's
  *interactivity*; the delta is real (breadth, descriptions, dynamics,
  fuzzy browsing), the machinery exists (providers, merge, guards, overlay,
  soft-key routing), and the one blocking objection (Tab) now has a
  shipping, source-pinned solution. The 2026-10-17 deferral's own
  precondition — "after inline suggestions prove the prompt-detection
  layer" — is met.

## Relation to prior records

- **Supersedes**: the completion-spec-menu rejection in
  [terminal-autocomplete](terminal-autocomplete.md) (2026-10-17) — one
  ruling, on its own stated condition; everything else in that record
  stands.
- **Extends**: [terminal-de-integration](terminal-de-integration.md)
  (2026-10-02) — no shipped scripts, no env injection, no OSC engine work;
  [autosuggest-stage2-crosscheck](autosuggest-stage2-crosscheck.md)
  (2026-10-21) — consumes its named loadable-file growth path and its
  fish-ranked-ghost/one-choice rulings; [autosuggest-display-mode](autosuggest-display-mode.md)
  (2026-10-24) — the menu is a separate surface, the delay key untouched.

## Sources

All fetched 2026-11-02 unless noted.

- Warp: docs — Autosuggestions
  <https://docs.warp.dev/terminal/command-completions/autosuggestions>,
  Completions
  <https://docs.warp.dev/terminal/command-completions/completions>;
  open-source announcement (2026-04-28) —
  <https://www.warp.dev/blog/warp-is-now-open-source>;
  `warpdotdev/warp` @ master `d57d9760dba059` (2026-10-30; license AGPL-3.0
  + MIT UI crates via repo API): `README.md` (Fig Completion Specs credit),
  `crates/warp_completer/src/completer/matchers.rs` (MatchStrategy::Fuzzy;
  subsequence fallback; last commit `fc4d563b8b63`, 2026-08-27),
  `crates/warp_completer/src/completer/context/mod.rs`
  (GeneratorContext::execute_command_at_pwd; CompletionContext::aliases;
  last commit `a06279712f83`, 2026-09-12),
  `crates/warp_completer/src/completer/describe.rs` (Description struct),
  `app/src/completer/mod.rs` (impl GeneratorContext; PATH force-set comment;
  last commit `40b791c35137`, 2026-10-01) —
  <https://github.com/warpdotdev/warp>
- withfig/autocomplete @ master (last push 2025-05-05, `aef52acff84c`,
  v2.692.3; MIT, "Copyright (c) 2021 Hercules Labs Inc. (Fig)"; 715
  top-level src/*.ts; `src/mkdir.ts` quoted) —
  <https://github.com/withfig/autocomplete>
- VS Code: `microsoft/vscode` @ main `67710841ad061a` (2026-11-02):
  `extensions/terminal-suggest/cgmanifest.json` (withfig pinned @
  `1cc34dc1ba530bb620bd380c25cfb9eb2264be89`, v2.684.0, MIT),
  `extensions/terminal-suggest/src/completions/{cd.ts,git.ts,upstream/}`
  (101 upstream specs; git generators `git --no-optional-locks log/config
  --get-regexp ^alias./rev-list`; `upstream/rm.ts` `isDangerous`),
  `extensions/terminal-suggest/src/fig/execute.ts` (spawnHelper2; 5 s/20 s
  timeouts), `…/fig/autocomplete/generators/{helpers,scriptSuggestionsGenerator}.ts`,
  `src/vs/workbench/contrib/terminalContrib/suggest/browser/terminal.suggest.contribution.ts`
  (Tab scoped to HasFocusedSuggestion :558-563; Enter conditions :565-569;
  Ctrl+Space TriggerSuggest),
  `…/suggest/common/terminalSuggestConfiguration.ts` (enabled default true;
  quickSuggestions all-'off' default; suggestOnTriggerCharacters false;
  selectionMode 'partial'); docs —
  <https://code.visualstudio.com/docs/terminal/shell-integration> —
  <https://github.com/microsoft/vscode>
- fish: `fish-shell/fish-shell` @ master `bcccefe566dc93` (2026-10-27) —
  `src/reader/reader.rs` (:5491-5492 `sort_and_prioritize` +
  `completions[0]`; `compute_and_apply_completions`;
  `handle_completions` doc: one → insert whole, many → common prefix +
  pager) — <https://github.com/fish-shell/fish-shell>; fuzzy-off-in-
  autosuggestion per [autosuggest-stage2-crosscheck](autosuggest-stage2-crosscheck.md)
- Clink: `chrisant996/clink` @ master `4bce300b24758f` (2026-11-02); docs
  v1.9.35 — clink-select-complete (Ctrl+Space), match.max_rows/
  preview_rows, clink.popup_search_mode, autosuggest.inline default false —
  <https://chrisant996.github.io/clink/clink.html>
- ble.sh: `akinomyoga/ble.sh` @ master `152942758f567e` (2026-10-31) —
  `lib/core-complete-def.sh:52-57` (auto_complete 1; auto_delay 1;
  auto_menu ''); wiki Manual §7 (auto-menu idle delay; menu-complete when
  menu shown) —
  <https://github.com/akinomyoga/ble.sh/wiki/Manual-%C2%A77-Completion>
- Carapace: `carapace-sh/carapace-bin` @ master `19f114de7c5f` (2026-11-02;
  MIT "Copyright (c) 2020 rsteube", unchanged since 2020 initial commit;
  1,556 completers), `carapace-sh/carapace` (Apache-2.0),
  `rsteube/carapace-spec` (MIT) —
  <https://github.com/carapace-sh/carapace-bin>
- inshellisense: `microsoft/inshellisense` @ `9ba441d39468` (2026-10-18;
  MIT; "a terminal native runtime for autocomplete … 600+ command line
  tools") — <https://github.com/microsoft/inshellisense>
- Peers: Ghostty v1.4.0 release notes (no completions feature; nu/carapace
  external-completer fix) —
  <https://ghostty.org/docs/install/release-notes/1-4-0>; `src/config/Config.zig`
  @ main `042b16b6146a` (zero completion keys); kitty v0.49.2 (2026-10-01);
  Alacritty v0.17.0 (2026-04-06); WezTerm @ `e68a8ec277` (2026-10-26);
  Windows Terminal actions docs (Suggestions still Preview; recentCommands
  shell-integration-powered; updated 2025-08-21) —
  <https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions>;
  Termius autocomplete & shell integration (exec channels; source list) —
  <https://docs.termius.com/terminal/autocomplete-and-shell-integration>
- DropTerm local tree @ HEAD `fe1cd72` (2026-11-01): `src/terminal/suggest.rs`,
  `src/terminal/widget.rs`, `src/terminal/engine.rs`, `src/keybinds.rs`,
  `src/config.rs`, `src/context_menu.rs`, `docs/autosuggestions.md`; prior
  records [terminal-autocomplete](terminal-autocomplete.md),
  [autosuggest-stage2-crosscheck](autosuggest-stage2-crosscheck.md),
  [autosuggest-display-mode](autosuggest-display-mode.md),
  [terminal-de-integration](terminal-de-integration.md),
  [terminal-theming](terminal-theming.md) (the per-content licensing audit
  precedent)
