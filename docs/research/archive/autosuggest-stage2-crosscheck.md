# Research: do the stage-2 autosuggestion rulings survive a primary-source cross-check?

Date: 2026-10-21
Question: *Stage 2 of DropTerm's terminal-side fish-style autosuggestions adds two
modules — a fourth suggestion provider completing subcommands and long flags from a
hand-curated static in-code table for exactly five commands, and a clip-to-fit change
so an over-wide ghost suffix is truncated in display while still accepted whole. Do
the reference systems (fish, zsh-autosuggestions, Fig/Amazon Q, Warp, iTerm2,
Ghostty, kitty, Atuin, Tabby) support each ruling?*

## TL;DR

**Seven of the nine rulings stand against the primary sources; two do not.** The
clip-to-fit change's core mechanic — display truncation is screen-only, an accept
still writes the full suffix — is exactly fish's behavior, verified in both fish
3.7.1 and current fish (4.x, Rust) source. But fish never truncates "plainly": 3.7.1
truncates horizontally **with an ellipsis character** and reserves two columns
(hiding the ghost entirely below that), and 4.x soft-wraps the ghost onto following
lines and appends an ellipsis when it must cut vertically. The stage-2 ruling of
plain truncation with no marker cites "fish parity" as its premise, and that premise
is false (R8). The static in-code flag table also has no precedent: every peer with
flag knowledge carries it as external, updatable, user-extensible data — fish ships
1,097 completion scripts autoloaded from `share/completions` plus user/vendor
directories; Fig's specs are a community TypeScript repo served from a CDN; Warp's
specs ship with the app and update with it (R6). Those two contradictions do not
break the modules — DropTerm's constraints (no shipped data files, no network) are
real — but the docs should stop claiming fish truncates plainly, and the specs
provider should keep a nameable growth path toward externalized data.

The rest validates. fish's autosuggestion machinery, read from source, is
history-first with completions as fallback — the same ruled order DropTerm's merge
implements; on ambiguity fish shows the single best completion after
`sort_and_prioritize`, not a longest common prefix, not nothing, which is the same
one-choice ghost model; subcommand-vs-flag seat positioning in fish's own git
completion matches the "subcommands at the first argument seat" ruling while being
richer (per-subcommand flag conditions) than the "flags from any seat" simplification;
default-on with an opt-out is fish's own rollout posture. And the re-verification
holds: as of today no terminal emulator among Ghostty, kitty, and Tabby ships
terminal-side command completion or ghost text; Atuin is a shell plugin with an
on-demand search UI, not inline ghost text; Warp's and Fig's completions live on
input surfaces DropTerm has deliberately not built. DropTerm's combination —
terminal-side ghost text over a PTY grid, no shell integration, no OSC, with a
merged multi-provider single-choice suffix — remains without a shipping precedent.

## What DropTerm stage 2 proposes

Self-contained restatement of the two modules under review (stage 1, shipped,
computes fish-style ghost-text autosuggestions entirely terminal-side: prompt
detected via `tcgetpgrp`, the input line scraped off the grid, providers merged in a
ruled order — per-tab history first, then filesystem path completion, then a
spawn-time `$PATH` command-name catalog — Right accepts the whole ghost suffix,
Alt+Right one word).

**Provider 4 — curated flag-specs.** Completion of subcommands and `--`-prefixed
long flags for exactly five commands (git, cargo, docker, systemctl, npm) from a
hand-curated static table compiled into the binary. The rulings under test:

- **R1** — subcommands complete only at the first argument seat (the token right
  after the command word); later argument seats never consult subcommands.
- **R2** — a `--`-prefixed trailing token consults the command's long-flag list from
  **any** argument seat (git accepts flags before and after subcommands); the flag
  list hangs off the command, uniform across its subcommands.
- **R3** — among strict-prefix matches, shortest wins; ties broken alphabetically.
- **R4** — the provider is gated off on tabs spawned as non-shells (e.g. ssh), the
  same gate as the path and catalog providers.
- **R5** — enabled by default in the engine, with no user-facing config toggle yet.
- **R6** — the table is static, hand-curated, in-code: not user-editable, not
  derived from `--help` or man pages at runtime, five commands and no more.
- **R7** — no short-flag (`-x`) completion.

**Clip-to-fit ghost.** A ghost suffix wider than the remaining columns is clipped
in display (truncated to the longest fitting prefix, cut on cell boundaries) instead
of dropping the suggestion. The rulings under test:

- **R8** — plain truncation: no ellipsis, no fade, no right-edge marker.
- **R9** — accept-whole-despite-clip: Right still writes the **full** suffix into
  the PTY; Alt+Right still takes one word of the **full** suffix; at zero visible
  columns the showing survives for accept while rendering nothing.

## Findings

### 1. fish — the reference implementation, read from source

fish's docs say suggestions come from "command history, completions, and valid file
paths" ([fishshell.com/docs/current/interactive.html](https://fishshell.com/docs/current/interactive.html),
fish 4.9.3 docs, read 2026-10-21). The source shows the precise machinery. Note fish
4.x is a Rust rewrite; `src/reader.cpp` no longer exists — the code is
`src/reader/reader.rs` on `master` (read 2026-10-21), with fish 3.7.1 checked for
the shipping-C++ behavior.

**(a) Source order — history first, completions as fallback, with validation.**
The autosuggestion performer in `src/reader/reader.rs` (background thread,
debounced) tries, in order:

1. **History, case-sensitively**: a `HistorySearch` over `SearchType::Prefix` (whole
   command line) then `SearchType::LinePrefix` (line at cursor); a case-sensitive
   hit is returned immediately.
2. Each history candidate must pass `autosuggest_validate_from_history`
   (`src/highlight/highlight.rs`): the suggested command must still be a builtin,
   defined function, or executable on `PATH`, and every path the historical command
   required must still exist (cd targets are checked against `CDPATH`). A stale
   history entry is skipped, not suggested.
3. Case-insensitive history hits are stashed, not returned — "Only to be used if no
   case-sensitive suggestions are found."
4. Suppression heuristics: "If the line ends with a space, and the cursor is not at
   the end, don't use completion autosuggestions"; a line ending in a quote with the
   cursor at the end suggests nothing.
5. **Completions as fallback**: `complete(&command_line[..], CompletionRequestOptions::Autosuggestion, ctx)` —
   "If there are no completions to suggest, fall back to icase history", and
   "Prefer icase history over smartcase/icase completions." So the precedence is
   exact-case history > exact-case completions > case-insensitive history.

This confirms the shape of DropTerm's merge: history is the backbone; completion
knowledge answers only what history cannot. Two fish behaviors DropTerm's history
provider does not have (worth knowing, not worth copying yet): validation against
command/path existence, and the case-insensitive tier.

**(b) Ambiguity — one ranked completion, not a common prefix, not nothing.** When
completions are consulted, fish does not compute a longest common prefix and does
not decline: `sort_and_prioritize(&mut completions, complete_flags); let comp = &completions[0];`
— the single first completion after sorting, applied append-only to the command
line. `sort_and_prioritize` (`src/complete.rs`) keeps only the best match-rank,
dedupes, natural-sorts (`wcsfilecmp`), then — because this is an autosuggestion —
prefers exact-case matches, avoids completions that duplicate an existing argument,
and penalizes files ending in `~` ("they're frequently autosave files"). Also:
autosuggestion-mode completion requests disable descriptions and **fuzzy matching**
(`fn fuzzy_match(&self) -> bool { !self.autosuggestion() }`) — strict prefix only,
the same ruled match space DropTerm's providers use.

**(c) Truncation — fish truncates with an ellipsis, never plainly.** In fish 3.7.1
(`src/screen.cpp`, `compute_layout`), when prompt + command + suggestion exceed the
screen width, the suggestion is truncated to `available_autosuggest_space - 2` and
then `result.autosuggestion.push_back(get_ellipsis_char())` — under the comment
"Need at least two characters to show an autosuggestion"; with ≤2 columns free the
ghost is not drawn at all. In current fish (`src/screen.rs`, `compute_layout`), the
suggestion **soft-wraps**: "We allow the lines to soft wrap naturally and we only
truncate vertically if we would exceed the screen height" — and the vertical cut
appends `ELLIPSIS_CHAR`. The screen tracks `autosuggestion_is_truncated` for
history-search skip logic. Both versions mark truncation with `…`; neither wraps a
bare cut with no affordance.

**(d) Accept — the full text, display truncation irrelevant.**
`reader_data_t::accept_autosuggestion` (3.7.1, `src/reader.cpp`) does
`replace_substring(&command_line, 0, command_line.size(), autosuggestion.text)` —
the full suggestion from reader state; the screen's truncated copy never feeds
back. Same in 4.x `accept_autosuggestion`. And because the un-displayed suggestion
still exists in reader state (Right at end-of-line accepts whenever
`autosuggestion` is non-empty), "invisible but acceptable" is fish's behavior too.

**(e) Completion data — 1,097 shipped scripts, user-extensible.** fish's completion
knowledge is scripts: 1,097 files in `share/completions` on `master` (GitHub tree
API, counted 2026-10-21), each a series of `complete` builtin calls ("complete
defines, removes or lists completions for a command" —
[docs/cmds/complete.html](https://fishshell.com/docs/current/cmds/complete.html)),
autoloaded on first use via `fish_complete_path`. Users extend the set in
`~/.config/fish/completions`; vendors in `vendor_completions.d`
([Writing your own completions](https://fishshell.com/docs/current/completions.html)).
The shipped `git.fish` is 3,068 lines with 1,694 `complete` calls, and its
positioning is condition-based: subcommands are offered only while
`__fish_git_needs_command` (no subcommand typed yet), and each subcommand's flags
are offered only when `__fish_git_using_command <sub>` matches — per-subcommand
precision, alias-aware. Short options are completed too (`-s`), and
`fish_autosuggestion_enabled` defaults to on with `set -g fish_autosuggestion_enabled 0`
as the opt-out.

### 2. zsh + zsh-autosuggestions — shell-side, history-only by default

[zsh-users/zsh-autosuggestions](https://github.com/zsh-users/zsh-autosuggestions)
README @ master (read 2026-10-21): "It suggests commands as you type based on
history and completions" — but the default strategy list in `src/config.zsh` is
`ZSH_AUTOSUGGEST_STRATEGY=(history)`: **completions do not feed the ghost unless
the user opts in** by adding the `completion` strategy ("Chooses a suggestion based
on what tab-completion would suggest", requires zpty) or `match_prev_cmd`.
Acceptance: →/End at buffer end; "If you invoke the forward-word widget, it will
partially accept the suggestion up to the point that the cursor moves to." Default
style `fg=8`. Everything renders inside zle — the shell draws the ghost as part of
its own line, so there is no terminal screen-edge truncation question at all; the
shell wraps its own buffer. That is the structural difference to keep in mind:
DropTerm's clip ruling exists only because DropTerm ghosts over a grid it does not
own.

### 3. Fig / Amazon Q — specs authored in the open, served from a CDN, rendered as a popup

- **Authoring**: specs are TypeScript in the open
  [withfig/autocomplete](https://github.com/withfig/autocomplete) repo — 715
  top-level `src/*.ts` command specs (tree API count, 2026-10-21; `git.ts` alone is
  9,812 lines). README: "A completion spec is a _declarative_ schema that specifies
  the `subcommands`, `options` and `args` for a CLI tool"; contribution is fork +
  PR ("Contribute your first spec in < 3 minutes"); specs compile from `src/` to
  `build/` and dev mode reads the local build. Specs include short **and** long
  options, per-subcommand argument trees, generators (dynamic candidates), icons,
  priorities.
- **Delivery**: the engine fetches compiled specs at runtime from CDNs —
  `crates/fig_desktop/src/protocol/spec.rs` in
  [aws/amazon-q-developer-cli-autocomplete](https://github.com/aws/amazon-q-developer-cli-autocomplete)
  lists `https://specs.q.us-east-1.amazonaws.com` (public) and an
  internal-Amazon CDN, downloads a spec index, caches in memory. Not bundled.
- **Render**: an IDE-style **popup menu** in a webview (`fig_desktop`
  autocomplete webview + `packages/autocomplete-app`), not inline ghost text; a
  separate AI inline-suggestion module exists (`figterm/src/inline`). The
  architecture is a PTY wrapper (`figterm`) plus an out-of-process desktop app —
  terminal-wrapper, not terminal-side.
- **Status**: [withfig/fig](https://github.com/withfig/fig) is archived;
  [aws/amazon-q-developer-cli](https://github.com/aws/amazon-q-developer-cli)
  README: "This open source project is no longer being actively maintained and will
  only receive critical security fixes. Amazon Q Developer CLI is now available as
  Kiro CLI, a closed-source product"; AWS's docs page now says only "The Q CLI has
  become the Kiro CLI" ([docs.aws.amazon.com](https://docs.aws.amazon.com/amazonq/latest/qdeveloper-ug/command-line.html),
  read 2026-10-21).

Fig is the closest analog to the flag-specs provider and the clearest contrast on
every data-plumbing ruling: open authoring, runtime delivery, user contribution —
versus a compiled-in hand-curated table.

### 4. Warp — inline ghost from history + completions; spec-driven popup for the rest

[Warp autosuggestions docs](https://docs.warp.dev/terminal/command-completions/autosuggestions)
(read 2026-10-21): "Warp will automatically suggest commands as you type based on
shell history and possible completions" — inline ghost text; "Complete an
autosuggestion using the → arrow or Ctrl+F"; partial accept Ctrl+→ (macOS) /
Ctrl+Shift+→ (Windows/Linux); toggle from the Command Palette; Tab can be remapped
to accept. [Completions](https://docs.warp.dev/terminal/command-completions/completions)
are a different surface: Tab opens a spec-driven **popup menu** ("A menu will show
all of your local branches") with a supported-specs table — Full support for git,
docker, brew, cargo, npm, gh, kubectl, make, ssh, tar, terraform, tmux, and
hundreds more marked Partial. So even Warp, with the best completions in the
industry, keeps the inline ghost fed by history+completion sources and pushes
spec browsing into a menu. Warp's input is a native editor, not a PTY grid (per
[terminal-autocomplete](terminal-autocomplete.md)).

### 5. iTerm2 — popup, screen text as the candidate source

[iTerm2 documentation](https://iterm2.com/documentation-one-page.html) (read
2026-10-21): "To use autocomplete, type the beginning of a word and then press
cmd-;. An autocomplete window opens showing the top 20 choices" — a popup, not
inline. "Any text that exists in a tab or its scrollback buffer can be
autocompleted in that tab" — the candidate database is visible screen text, no
shell integration required. Shell Integration is a separate, optional layer that
lets iTerm2 "navigate from one shell prompt to another, record your command
history, suggest most used directories". No flag/spec knowledge anywhere.

### 6. Ghostty and kitty — re-verified: still no terminal-side completion

- **Ghostty** (`main`, read 2026-10-21): `src/config/Config.zig` (431,718 bytes)
  contains **zero** occurrences of "completions"/"autosuggest"/"autocomplete";
  `ShellIntegrationFeatures` is exactly `{cursor, sudo, title, ssh-env,
  ssh-terminfo, path}`; latest tag `v1.3.1`. Unchanged from the 2026-10-17 finding
  in [terminal-autocomplete](terminal-autocomplete.md).
- **kitty** ([shell-integration docs](https://sw.kovidgoyal.net/kitty/shell-integration/),
  read 2026-10-21): the feature list's only completion item is "Sophisticated
  completion for the kitty command in the shell"; `no-complete` "Turn off
  completion for the kitty command." — CLI completion for kitty's own binary. No
  autosuggestions, no ghost text, no completion UI.

### 7. Atuin and Tabby — neither fills the gap

- **Atuin** ([docs.atuin.sh](https://docs.atuin.sh/latest/), read 2026-10-21): a
  shell plugin (zsh, bash, fish, nushell, xonsh, PowerShell) that "replaces your
  shell history with a SQLite database, and records extra context for every
  command"; usage is "press Ctrl+R or the Up arrow to search" — an on-demand
  interactive search UI. No inline ghost-text autosuggestion feature exists in its
  docs.
- **Tabby** ([Eugeny/tabby](https://github.com/Eugeny/tabby), `master` tree read
  2026-10-21): zero paths matching "autosuggest" or "ghost"; the built-in packages
  (local, ssh, serial, telnet, linkifier, plugin-manager, …) include no completion
  plugin. The fish-style UX inside Tabby comes from the shell, e.g.
  zsh-autosuggestions.

## The verdict table

| Ruling | Verdict | Basis (primary source, read 2026-10-21) |
|---|---|---|
| **Architecture**: terminal-side ghost, no shell integration, no OSC | **VALIDATED — still novel** | No terminal emulator checked (Ghostty `Config.zig`, kitty docs, Tabby tree) ships it; inline ghost text exists only shell-side (fish source, zsh-autosuggestions, Atuin-as-search) or editor-side (Warp, Fig). DropTerm's tcgetpgrp + grid-scrape + merged-provider single-choice ghost has no shipping precedent. |
| **R1** subcommands complete only at the first argument seat | **VALIDATED** | fish's git completion offers subcommands only while `__fish_git_needs_command` (no subcommand present) — `share/completions/git.fish` @ master; same first-seat rule, fish expresses it via conditions. |
| **R2** `--` long flags complete from any argument seat | **NUANCED** | fish gates flags per-subcommand (`__fish_git_using_command`), so a flag completes in the seats where its subcommand context is active — richer than any-seat. Any-seat is the honest flat-table approximation, but it can offer a command's flags in seats fish would route through a different, narrower rule set. |
| **R3** shortest wins, ties alphabetical | **NUANCED** | fish also picks one deterministic choice — `completions[0]` after `sort_and_prioritize` (`src/complete.rs`) — but its order is match-rank then natural-alphabetical, not shortest-first. No peer prefers "shortest"; the ruling is DropTerm's own, defensible, untested elsewhere. |
| **R4** gated off on non-shell (ssh) tabs | **NO PRECEDENT (stand)** | No comparator has the concept — shells are local truth; Warp/Fig follow you everywhere (Fig even ships SSH autocomplete). The gate is DropTerm-conservative, not contradicted; note it also forfeits the one provider whose knowledge is universal. |
| **R5** engine-true default, no config key yet | **VALIDATED (fish), gap noted** | fish defaults autosuggestions on (`fish_autosuggestion_enabled`, reader.rs:832) — but ships the opt-out variable from day one; DropTerm defers the key, a temporary divergence from its own cited precedent. |
| **R6** static hand-curated in-code table, not user-editable, not `--help`-derived | **CONTRADICTED** | Every peer with flag knowledge externalizes it: fish 1,097 autoloaded scripts + `~/.config/fish/completions` + vendor dir; Fig open TS repo served from CDN; Warp specs bundled-with-updates; VS Code terminal-suggest extension specs (prior record). None compiles the table in. DropTerm's choice is a deliberate scope trade — keep a growth path in mind. |
| **R7** no short-flag completion | **CONTRADICTED (defensible)** | fish completes short options (`complete -s`); Fig and Warp specs carry them. All of those are multi-choice menus, where a short flag costs a glance; in a single-choice ghost a short-flag guess is wrong more often — the exclusion is defensible for ghost text specifically, but no peer shares it. |
| **R8** plain truncation, no ellipsis | **CONTRADICTED** | fish 3.7.1 truncates to fit − 2 columns and appends `get_ellipsis_char()` (screen.cpp `compute_layout`); fish 4.x soft-wraps and appends `ELLIPSIS_CHAR` on vertical truncation (screen.rs). Fish never truncates plainly; the "fish parity" premise for plain truncation is false. |
| **R9** accept-whole-despite-clip (full suffix on Right/Alt+Right; zero-column showing survives) | **VALIDATED** | fish `accept_autosuggestion` replaces the command line with the full `autosuggestion.text` from reader state — the screen's truncated copy never feeds back (3.7.1 reader.cpp; master reader.rs); an un-rendered suggestion is still acceptable (accept checks reader state, not the display). |

## What should change in the specs

1. **Fix the R8 premise** (the one outright error): both fish versions mark a
   truncated ghost with `…` and 3.7.1 hides it below a 3-column threshold rather
   than clipping to 1-2 characters. Either adopt the ellipsis (fish parity, and a
   visible "more exists" affordance for a feature whose whole accept path depends
   on the user knowing there is more), or keep plain truncation but re-justify it
   honestly (minimal overlay, clip cases are rare in a wide pane) — not as fish
   parity. Note also fish 4.x soft-wraps rather than clips; DropTerm's clip is an
   overlay-constraint adaptation, which the docs should say.
2. **Note the R6 growth path now**: a static five-command table is fine as v1, but
   every successful flag-knowledge system (fish scripts, Fig specs, Warp specs)
   ended up externalized and community-fed. When the table grows past a handful of
   commands, the next step in every precedent is a data file loaded at startup —
   not a bigger `const`. The specs module should be written so the table's shape
   could move behind a loader without touching the merge.
3. **Worth stealing from fish (cheap, later)**: history-candidate validation
   (DropTerm's history provider happily re-suggests commands whose binaries have
   disappeared; fish checks), and the suppression rule for trailing-quote lines.
   Both are provider-level, no architecture change.

## Sources

All read 2026-10-21 unless noted.

- fish @ `master` (Rust, 4.x line): `src/reader/reader.rs` (autosuggest performer:
  history Prefix/LinePrefix search, icase fallback, suppression heuristics,
  `complete(...)` fallback taking `completions[0]`, `accept_autosuggestion` full
  text; `can_autosuggest` / `fish_autosuggestion_enabled` default);
  `src/complete.rs` (`CompletionRequestOptions::Autosuggestion` — no descriptions,
  no fuzzy match; `sort_and_prioritize` — best rank, dedupe, natural sort, then
  exact-case / no-duplicate-arg / tilde penalty); `src/screen.rs`
  (`compute_layout` — soft wrap, vertical truncation with `ELLIPSIS_CHAR`,
  `autosuggestion_is_truncated`); `src/highlight/highlight.rs`
  (`autosuggest_validate_from_history` — command on PATH/builtin/function, paths
  valid, cd/CDPATH); `share/completions/` — 1,097 scripts (GitHub tree API),
  `git.fish` (`__fish_git_needs_command` / `__fish_git_using_command` conditions)
  — <https://github.com/fish-shell/fish-shell>
- fish @ `3.7.1`: `src/screen.cpp` (`compute_layout` — truncate to
  `available - 2`, `push_back(get_ellipsis_char())`, "Need at least two
  characters", multi-line → no suggestion); `src/reader.cpp`
  (`accept_autosuggestion` replaces with full `autosuggestion.text`) —
  <https://github.com/fish-shell/fish-shell/tree/3.7.1>
- fish docs (4.9.3): Autosuggestions ("based on command history, completions, and
  valid file paths"; accept →/Ctrl+F; alt-right one word;
  `fish_autosuggestion_enabled 0` opt-out) —
  <https://fishshell.com/docs/current/interactive.html>; `complete` builtin —
  <https://fishshell.com/docs/current/cmds/complete.html>; Writing your own
  completions (`~/.config/fish/completions`, `vendor_completions.d`,
  `share/completions`) — <https://fishshell.com/docs/current/completions.html>
- zsh-users/zsh-autosuggestions @ master: README (strategies history / completion /
  match_prev_cmd; forward-word partial accept; `fg=8`); `src/config.zsh`
  (`ZSH_AUTOSUGGEST_STRATEGY=(history)` default) —
  <https://github.com/zsh-users/zsh-autosuggestions>
- Fig / Amazon Q: withfig/autocomplete @ master (715 top-level `src/*.ts` specs;
  README "declarative schema… subcommands, options and args"; "Contribute your
  first spec in < 3 minutes"; git.ts 9,812 lines) —
  <https://github.com/withfig/autocomplete>;
  aws/amazon-q-developer-cli-autocomplete @ main
  (`crates/fig_desktop/src/protocol/spec.rs` — CDN spec sources incl.
  `https://specs.q.us-east-1.amazonaws.com`, in-memory index cache; webview
  autocomplete render; figterm PTY wrapper) —
  <https://github.com/aws/amazon-q-developer-cli-autocomplete>;
  aws/amazon-q-developer-cli README ("no longer being actively maintained…
  Kiro CLI, a closed-source product") —
  <https://github.com/aws/amazon-q-developer-cli>;
  AWS docs ("The Q CLI has become the Kiro CLI") —
  <https://docs.aws.amazon.com/amazonq/latest/qdeveloper-ug/command-line.html>;
  withfig/fig (archived, per record of 2026-10-17) —
  <https://github.com/withfig/fig>
- Warp docs: Autosuggestions ("based on shell history and possible completions";
  →/Ctrl+F; Ctrl+→ partial; palette toggle) —
  <https://docs.warp.dev/terminal/command-completions/autosuggestions>;
  Completions (Tab popup; Full/Partial spec table) —
  <https://docs.warp.dev/terminal/command-completions/completions>
- iTerm2 documentation (one-page; Cmd+; popup, top 20; "Any text that exists in a
  tab or its scrollback buffer"; shell integration layer) —
  <https://iterm2.com/documentation-one-page.html>
- Ghostty @ main: `src/config/Config.zig` (0 completion/autosuggest keys;
  `ShellIntegrationFeatures` = cursor/sudo/title/ssh-env/ssh-terminfo/path); tags
  through v1.3.1 — <https://github.com/ghostty-org/ghostty>
- kitty shell-integration docs ("Sophisticated completion for the kitty command in
  the shell"; `no-complete`) —
  <https://sw.kovidgoyal.net/kitty/shell-integration/>
- Atuin docs ("replaces your shell history with a SQLite database"; "press Ctrl+R
  or the Up arrow to search"; zsh/bash/fish/nushell/xonsh/PowerShell; no inline
  ghost) — <https://docs.atuin.sh/latest/>
- Tabby @ master (tree: no autosuggest/ghost paths; no completion package) —
  <https://github.com/Eugeny/tabby>
- DropTerm local record [terminal-autocomplete](terminal-autocomplete.md)
  (2026-10-17: VS Code terminal-suggest Fig-style specs and keystroke-synthesis
  acceptance; Warp native input editor; Alacritty maintainer position; Termius)
