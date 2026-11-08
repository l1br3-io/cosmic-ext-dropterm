# Research: should DropTerm's path provider complete bare argument words (cd src, no ./)?

Date: 2026-11-06

## TL;DR

**Every completion-capable system surveyed completes bare argument words as
paths — fish, zsh, bash, Warp, VS Code, and the ghost feeders ble.sh and
Clink — and every one of them does it as a *fallback*: files answer a bare
word only when the command's own completions do not claim the seat, never in
competition with them; DropTerm should adopt exactly that carve (design D),
extending it to both provider twins (ghost and menu), with directories-only
for `cd`/`pushd`, riding the existing shell gate and `autosuggestions_paths_enabled`
— not menu-only rows (A), not a curated path-command allowlist (B), and not
unconditioned fish-parity ghosting (C).** The pivotal precedent lands harder
than the 2026-10-21 crosscheck spelled out: fish's autosuggestion ghost draws
on the completion engine, and the engine's default file policy is *perform
file completion unless the command's completions say skip* — so `cd sr` ghosts
`c/` in fish today (pinned at `FileCompletionPolicy::Inherit`, the default,
and the ghost's empty-token carve, `src/complete.rs` @ `d92ccd4f8f6f`). What
fish never does is let files contend with a specced seat: `git co` offers no
cwd files because git's completion script declares `--no-files` throughout
(`share/completions/git.fish:120,1055`) — seat ownership is declared data,
not rank order. That last fact is why C is wrong for DropTerm as stated: the
merge ranks Path above Specs (`source_rank`, `src/terminal/suggest.rs:493`),
so unconditioned bare-word ghosting would ghost `nfigure/` over `mmit` for
`git co` in any repo with a `configure` file. The fallback carve — path
answers a bare word only when the specs table does not own the command and
the generator seat is not claimed — is fish's semantic, Warp's
`CompletionsFallbackStrategy::FilePaths` default, and VS Code's
no-items condition, all pinned below. CDPATH stays out (shell state DropTerm
cannot read — the alias-resolution verdict's class).

## What DropTerm has today

Grounded in the working tree at HEAD `c48c5267a35d` (2026-11-06):

- **The bare-word ruling is deliberate and documented**: "A bare word like
  `src` never triggers it - that stays history's (and the command catalog's,
  below) territory" (`docs/autosuggestions.md:25`). The gate is
  `is_path_shaped` — `token == "~" || token.contains('/')`
  (`src/terminal/suggest.rs:1780-1782`) — and `path_token` filters the
  trailing token through it (`:1788-1792`). Only a `~`-root or slash-carrying
  token reaches the provider.
- **The ghost twin** (`path_candidate`, `src/terminal/suggest.rs:527-568`):
  the trailing token's final component against the parent's reduced listing —
  a unique match completes fully (directories suffixed `/`), an ambiguous
  field completes the longest shared prefix, no extension is silence;
  dotfiles match only a `.`-prefixed component; the remainder rides
  `escape_for_shell` (`:1210-1231`).
- **The menu twin** (`path_candidates`, `src/terminal/suggest.rs:1363-1404`):
  the same gates and component carve, every matching entry its own escaped
  row. The menu dissolves the ghost's tiebreak-shaped floors — the catalog's
  one-character floor and the specs' bare-`--` floor are "the single-choice
  ghost's alone… a list of every flag is information, not a guess"
  (`src/terminal/suggest.rs:1408-1410`, and the catalog twin's comment at
  `:1449-1456`).
- **The merge**: first-match-wins in `source_rank` order — History(0),
  Path(1), Specs(2), Catalog(3), Generator(4) (`src/terminal/suggest.rs:493-501`,
  dispatch `:1257-1273`); the menu concatenates the same order
  (`candidates`, `:1300-1320`; the user-facing order at
  `docs/autosuggestions.md:103`).
- **Seat carves already exist**: the specs provider answers the
  first-argument seat (subcommands) and any-seat long flags
  (`specs_context`, `src/terminal/suggest.rs:966-986`); the generator
  provider answers exactly the argument seat after `git checkout|switch|add|restore`
  (`generator_context`, `:1084-1105`).
- **The engine's parent carve cannot take a bare word as written**: a token
  with no `/` carves *itself* as the parent (`None => token`,
  `resolve_token_parent`, `src/terminal/engine.rs:801-820`) and joins onto
  the cwd — so `src` would key the listing cache at `cwd/src`'s children,
  not cwd's entries. And the component carve treats a no-slash token as the
  empty component (`None => ""`, `src/terminal/suggest.rs:535-538` — correct
  for the `~` root, the only no-slash token that passes today). Bare-word
  support needs a cwd carve on both sides: parent = cwd, component = the
  whole token.
- **The gate stack to reuse**: `paths_enabled` + the shell allowlist
  (`paths_eligible`, the ssh gate, `docs/autosuggestions.md:29`), the
  listing cache (`PathListingCache::listing_for`,
  `src/terminal/engine.rs:769-791`, refreshed only when the canonical parent
  key changes), `read_sorted_entries` (`:829-844`).

## Findings

All source pins fetched 2026-11-06.

### 1. fish — the ghost draws on the completion engine, and the engine's default is files

fish @ master `d92ccd4f8f6f08cfc8a0d9fa17333153e2922af1` (2026-11-03):

- **The ghost's source order is history first, completions fallback — and
  the completions fallback includes files.** The autosuggestion performer
  calls the full engine: `complete(&command_line[..], CompletionRequestOptions::Autosuggestion, ctx)`,
  then `sort_and_prioritize(&mut completions, …); let comp = &completions[0];`
  (`src/reader/reader.rs:5479-5492`). Nothing restricts that call to
  command-specific scripts. The docs say the same at the surface: "fish
  suggests commands as you type, based on command history, completions, and
  valid file paths" (fish docs, Interactive use → Autosuggestions,
  read 2026-11-06). This extends — does not contradict — the stage-2
  crosscheck's pipeline pin.
- **The default file policy is *perform files unless declared otherwise*.**
  `FileCompletionPolicy` defaults to `Inherit` ("Defer to surrounding
  completion logic", `src/complete.rs:69-75`), and the argument-seat decision
  is `do_file = arg_data.file_policy != FileCompletionPolicy::Skip`
  (`:941,978`). A command with no completion script at all inherits → files
  complete. The completions doc states it: "fish will also offer files by
  default, in addition to the arguments you specified" (fish docs, Writing
  your own completions, read 2026-11-06) — with `--no-files`/`-f`,
  `--exclusive`/`-x`, and `--force-files`/`-F` as the per-command overrides.
  So `mycmd sr` ghosts/bare-word-completes as a file in fish.
- **The ghost's one carve: an empty token never file-ghosts.** "If we're
  autosuggesting, and the token is empty, don't do file suggestions"
  (`src/complete.rs:980-983`) — Tab after `cd ` lists directories; the ghost
  after `cd ` shows nothing from files. DropTerm already matches by
  construction (an empty trailing token never passes `path_token`; the menu
  requires a token to match).
- **`cd` is special-cased to directories only, through CDPATH, in the core.**
  "Hack. If we're cd, handle it specially (issue #1059…)" → `handle_as_special_cd`
  (`src/complete.rs:986-988`) sets `PathFilter::Directory { for_cd_argument:
  Some(ForCdArgument { for_autosuggestion }) }` (`:1765-1771`); the expansion
  resolves against `$CDPATH` with cwd always included — "The current
  directory is always valid" (`src/expand.rs:1517-1561`). The shipped
  `share/completions/cd.fish` only adds CDPATH matches; its helper says "We
  rely on the core file name completion logic to include all other possible
  matches" (`share/functions/__fish_complete_cd.fish`). And the cd ghost is
  *richer* than Tab: "Autosuggestions for cd. Descend the deepest unique
  hierarchy we can, and then append any components to each new result. Not
  for cd tab completion (issue #4402)" (`src/wildcard.rs:965-978`).
- **Seat contention is settled by per-command data, not rank.** git's script
  wraps nearly every declaration in `--no-files`: a helper builds
  `complete -f -c git …` (`share/completions/git.fish:120`) and subcommand
  entries carry it (`complete -f -c git -n '__fish_git_needs_command' -a fetch …`,
  `:1055`). So `git co` in a repo with a `configure` file offers `commit`
  and never the file — files are *excluded from git's completion entirely*,
  a declaration DropTerm's flat specs table has no channel for.

### 2. zsh — the fallback chain ends at `_files`; cd is directories-only

zsh @ master `30e8be913521523d226789e53a4a2202dabff61c` (2026-11-01):

- With compinit, any word no specific completion claims falls to
  `Completion/Zsh/Context/_default` (`#compdef -default-`), whose body is
  `… _files "$@" && return 0` — filename completion is the terminal fallback
  of the whole system.
- Without compsys, the manual pins the same default: "-D controls default
  completion behavior for the arguments of commands not assigned any special
  behavior. If no compctl -D command has been issued, **filenames are
  completed**" (zsh manual, Completion Using compctl, read 2026-11-06).
- `cd`/`chdir`/`pushd` complete through `Completion/Zsh/Command/_cd`:
  "Normally just completes directories. Uses cdpath if that's set and the
  string doesn't begin with ~, /, ./ or ../" — `_path_files -/` (directories
  only).

### 3. bash — filename completion is the documented default fallback

Bash Reference Manual, Edition 5.3 (18 May 2025; Chet Ramey's official
mirror at tiswww.case.edu, read 2026-11-06):

- The TAB command: "Bash attempts completion by first checking for any
  programmable completions for the command word …, otherwise treating the
  text as a variable …, username …, hostname …, or command … in turn. **If
  none of these produces a match, it falls back to filename completion.**"
  (§8.4.6, *Letting Readline Type For You*).
- The compspec chain ends the same way: "If there is no default compspec,
  Bash performs alias expansion … If a compspec is not found, Bash performs
  its default completion described above" (§8.6, *Programmable Completion*).
  `complete -D` defines the default for "a command for which no completion
  has previously been defined"; `complete -E` the empty line; `-o bashdefault`
  then `-o default` re-enable Bash's own completions and "Readline's default
  completion" (filename) when a compspec yields nothing — the shape
  bash-completion's own loader uses (`complete -D -F _completion_loader -o
  bashdefault -o default`, §8.7 example).

### 4. Warp — a named, default-on FilePaths fallback, condition-gated

Warp is open source (AGPL-3.0; survey-only, no code copying) @ master
`f7fc2f2a08a3aa446baa23960710e630c5defd6e` (2026-11-06):

- `CompletionsFallbackStrategy { FilePaths, None }` — "The fallback strategy
  used to generate completion suggestions when no `CommandSignature` exists
  for the command" — and the default is `FilePaths`
  (`crates/warp_completer/src/completer/suggest/mod.rs:582-617`).
- The gate is exactly fallback-only: "If we were unsuccessful in getting
  completions *and* there was not a completion spec we attempted, fallback to
  the fallback type (if any)" → `sorted_paths_relative_to(parsed_argument, …)`
  (`crates/warp_completer/src/completer/engine/argument/legacy.rs:93-107`);
  and a spec argument with no argument types also falls to files (`:577-598`).
- Per-argument path types are Fig templates: `TemplateType::Files`/
  `FilesAndFolders` → `sorted_paths_relative_to`, `Folders` →
  `sorted_cd_directories`, which walks `$CDPATH` and "falls back to pwd when
  no `$CDPATH` entry matches" (`legacy.rs:673-727`;
  `…/engine/path.rs:139-…`). The public docs state only "Get fuzzy-matched
  suggestions for commands, options, and paths by pressing Tab anywhere"
  (docs.warp.dev completions page, read 2026-11-06) — the fallback itself is
  source-pinned, not documented.

### 5. Fig / Amazon Q — path arguments are declared per-argument; the fallback is the engine's, not the format's

- The spec format declares path arguments as templates:
  "filepaths: show folders and filepaths…; folders: show folders only", with
  the canonical examples "`cd` uses the "folders" template" and "`ls` used
  ["filepaths", "folders"]" (`@withfig/autocomplete-types` 1.31.0,
  `index.d.ts:5-29`, npm via unpkg, read 2026-11-06 — the
  withfig/autocomplete-types GitHub repo is gone, 404).
- The format itself has no fallback notion — fallback is engine policy, and
  the two live Fig-format engines implement it (Warp above; VS Code below).
  Whether the original Fig desktop engine fell back to files for un-specced
  commands could not be verified from a primary source (fig.io docs are
  dead; the engine repo is archived) — unverified, stated as such.

### 6. VS Code terminal suggest — files-and-folders fallback for bare words, menu-surface, setting-gated

`microsoft/vscode` @ main `03d396a6e01bf958211fde9f2b279619881d68b2`
(2026-11-06):

- The fallback, in code: for a non-command token, "else if (!items.length &&
  !showFiles && !showDirectories && !hasCurrentArg) { showFiles = true;
  showDirectories = true; }" — no spec items and no argument context →
  local files and folders complete
  (`extensions/terminal-suggest/src/terminalSuggestMain.ts:556-559`; the
  spec-side template flags at `…/fig/figInterface.ts:186-275`). A bare
  prefix with no slash resolves the cwd itself
  (`resolveCwdFromCurrentCommandString`, `terminalSuggestMain.ts:384-427`)
  — bare words complete against cwd.
- The setting names it: `quickSuggestions.unknown` — "Enable quick
  suggestions when it's unclear what the best suggestion is, if this is on
  **files and folders will be suggested as a fallback**"
  (`…/suggest/common/terminalSuggestConfiguration.ts:112`), default off;
  the fallback always runs on explicit invocation —
  `allowFallbackCompletions = explicitlyInvoked ||
  quickSuggestionsConfig.unknown === 'on'`
  (`…/suggest/browser/terminalSuggestAddon.ts:307-312`).
- `cd` is declared folders-only in an authored spec
  (`extensions/terminal-suggest/src/completions/cd.ts`: `args: { template:
  'folders', … }`), and CDPATH is its own setting — `terminal.integrated.suggest.cdPath`
  (default `'absolute'`), possible because shell integration reports the
  environment (`terminalSuggestConfiguration.ts:169-177`). This is a popup
  surface; VS Code's terminal has no file ghost.

### 7. carapace — no stated file fallback; unknown commands delegate to the shell's default

carapace-bin @ master `63b7842024e8c5651e8a61fcbb799778235ca4e4` (2026-11-06):
an unknown command fails `completers.Lookup` ("unknown completer/variant",
`cmd/carapace/cmd/completers/completers.go:304-314`), and `invokeCompleter`
then returns an empty snippet — carapace contributes nothing
(`cmd/carapace/cmd/invoke.go`). Neither the README nor the docs site states
a file-fallback policy (both checked, read 2026-11-06); the effective
behavior for unknown commands is the host shell's own default — filename
completion in every supported shell (findings 1-3). Weaker pin, honestly
so: carapace neither confirms nor contradicts the fallback consensus; it
sidesteps it.

### 8. Terminal emulators — still nobody else completes; re-verified

- **Ghostty** @ main (read 2026-11-06): `src/config/Config.zig` contains
  zero occurrences of completion/autosuggest/autocomplete (grep); latest
  tag v1.4.0 — unchanged from the prior records.
- **kitty** v0.50.0 (2026-11-03): shell-integration docs still offer only
  "Sophisticated completion for the kitty command in the shell" (the kitty
  CLI's own completion) — no terminal-side completion or ghost.
- **Alacritty** v0.17.0 (2026-04-06): no release since the prior records'
  pin; no completion feature.
- **WezTerm** @ `e68a8ec27727fbbda15e07f0a0737cd5e16481f6` (2026-10-26):
  unchanged from the warp-parity record's pin; no completion/ghost code.

### 9. The other ghost feeders — ble.sh and Clink both ghost completions, files included

- **ble.sh** (wiki Manual §7, read 2026-11-06): auto-complete (the inline
  ghost) defaults on (`complete_auto_complete=1`); its sources are history
  *and* "the auto-complete source based on the syntax and programmable
  completion", and file-name ghosting is explicit — `complete_timeout_auto`
  (default 5000 ms) "controls the timeout for the pathname expansions
  performed in auto-complete".
- **Clink** (docs v1.9.35, read 2026-11-06): `autosuggest.enable` defaults
  true; `autosuggest.strategy` defaults `match_prev_cmd history completion` —
  generators "are tried in the order listed… until one provides a
  suggestion", and the `completion` strategy "chooses the first of the
  matching completions" — the completion engine (Readline file completion
  included) is the ghost's last-resort source, exactly fish's shape.

## The verdict table

| Design | Verdict | Basis |
|---|---|---|
| **A. Menu-only bare-word rows** | Rejected as the answer; its safety insight is kept | No surveyed system restricts bare-word file completion to a list surface; VS Code's fallback is list-surface but it has no ghost at all — DropTerm does, and fish (the cited parity) ghosts bare words. A also splits the path twins' shared gates against the provider-seam pattern, and still needs the same seat stand-down for correct row order. |
| **B. Curated path-argument commands** | Rejected as the gate; its data notion survives as the growth path | Fig's per-argument `template: filepaths/folders` and VS Code's `cd.ts` are declarations of *known* path arguments, but every one of those engines still falls back to files for everything un-declared; an allowlist cannot answer `mycmd src` and duplicates filesystem truth in data. The directories-only carve and any future per-argument types belong in the completions.toml tier, not as the v1 gate. |
| **C. fish-parity ghost, unconditioned** | Rejected — wrong for DropTerm's merge specifically | fish settles seats by per-command `--no-files` data inside one engine pass (git never file-completes); DropTerm merges independent providers by rank with Path above Specs (`source_rank`) — unconditioned, `git co` over a cwd `configure` ghosts the file. Made safe, C becomes D. |
| **D. Fallback-only bare-word paths (recommended)** | **Adopt** | The unanimous shape: fish's `Inherit` policy (files unless the command's completions claim/skip the seat), Warp's `!arg_has_spec` FilePaths default, VS Code's no-items condition, zsh's `_default` → `_files`, bash's documented filename fallback. Ghost + menu twins together, existing rules intact. |

## Recommendation

Adopt **D**, in one module:

1. **The carve**: the path provider accepts a *bare argument word* — the
   trailing token when it is non-empty, not in command position, not
   `-`-prefixed, and not path-shaped — with cwd as parent and the whole
   token as the component. This must be a new carve, not a loosening of
   `is_path_shaped`: the catalog and command-position logic key on that
   predicate (`src/terminal/suggest.rs:601,1465`), and path-shaped tokens
   keep their explicit-parent behavior unchanged.
2. **Both twins take it** (`path_candidate` and `path_candidates`): the
   ghost reuses its existing trust envelope whole — unique completes
   (directories suffixed `/`), ambiguous completes the shared prefix, no
   extension silences, dotfiles follow the shell convention, the suffix
   rides `escape_for_shell`. No new floors: the common-prefix rule already
   bounds one-character bare words exactly as it bounds `~/s` today, and
   the menu dissolves floors by design. An empty trailing token never
   completes — fish's own ghost carve (`src/complete.rs:980-983`), already
   true here by construction.
3. **The fallback gate — the seat stand-down**: the bare-word carve is
   silent whenever the specs table has the command (its data owns the
   command's seats, the flat-table analog of fish's per-command `-f`) or
   `generator_context` claims the seat. Path-shaped tokens are unaffected.
   Consequence: `git co` keeps ghosting `mmit` and listing subcommands
   only; `cat sr`, `vim ha`, `codeinte …` — any un-specced command — gain
   both surfaces. History keeps outranking everything (rank 0), as in fish.
4. **Directories-only for `cd` and `pushd`**: a built-in reviewed carve
   (the `GeneratorSeat` precedent), matching fish's core special-case,
   zsh's `_cd`, Fig's `folders` template for cd, and VS Code's `cd.ts`.
   CDPATH stays out: it is shell state DropTerm cannot read without the
   injection verdict's machinery — the alias-resolution rejection's exact
   class; cd completes cwd's directories. fish's descend-unique-hierarchy
   cd ghost (`src/wildcard.rs:965-978`) is a named, accepted delta —
   DropTerm's ghost stops at directory boundaries everywhere by existing
   ruling.
5. **Engine change**: `resolve_token_parent` gains the bare-word arm —
   no `/`, not `~` → parent is the cwd (the `""`-relative join already
   exists), component is the whole token; the listing cache keys on the
   canonical cwd, so `cd sr` reads once per directory as today
   (`src/terminal/engine.rs:769-844`).
6. **No new config key**: the carve rides `autosuggestions_paths_enabled`
   and the standing shell gate (`paths_eligible`) — both false-positions
   keep restoring today's behavior exactly. `docs/autosuggestions.md:25`
   is rewritten when this ships (the toggle's documented promise stays
   true).
7. **Growth path (named, not v1)**: a per-argument tier in
   `completions.toml` — Fig's `template` notion — so specced commands can
   declare path/folder arguments and re-enable or shape path answers at
   their seats; the seed-audit conversion rules would then decide what
   crosses from withfig specs. Until that tier exists, a specced command's
   seats belong to its specs alone.

## Rejected, and why

- **A (menu-only rows)**: leaves the flagship case — `cd sr` ghosting —
  unsolved while the tier's own premise is fish parity, and fish ghosts it
  (finding 1); creates two divergent carves inside one provider (the twins
  share gates by design, `path_candidates`' doc); and buys no safety that
  the existing ghost rules do not already bound, while still requiring the
  seat stand-down for honest row ordering under `source_rank`.
- **B (curated path-taking commands)**: contradicts the unanimous
  universal-fallback precedent (findings 1-4, 6, 9) — no surveyed system
  gates bare-word path completion on an allowlist, because the long tail
  (`mycmd src`) is exactly what the fallback exists for; it would duplicate
  filesystem truth in a maintained table; and it needs the same engine cwd
  carve anyway, so it costs more and answers less.
- **C (unconditioned fish-parity ghost)**: unsafe under DropTerm's merge
  specifically — Path outranks Specs, so `git co` in a repo with a
  `configure` file would ghost the file's suffix over the subcommand,
  violating the ghost's never-a-wrong-suffix spine. fish avoids this by
  per-command `--no-files` declarations evaluated inside one completion
  pass, a channel DropTerm's flat table lacks; the stand-down of D is the
  honest flat-table equivalent.
- **Doing nothing**: the current ruling now stands against every surveyed
  peer — including fish, whose ghost the tier claims as its model; the gap
  is the parity debt this record closes, and every risk it carried (wrong
  suffix at a claimed seat, ssh leakage, ambiguity) is already governed by
  existing rules plus the stand-down.

## Relation to prior records

- **Extends** [autosuggest-stage2-crosscheck](autosuggest-stage2-crosscheck.md)
  (2026-10-21): its fish pipeline pin (history-first, completions fallback,
  `completions[0]`) stands; this record pins what that fallback *contains*
  for bare words (files, by default policy) and fish's empty-token ghost
  carve.
- **Extends** [autosuggest-warp-parity](autosuggest-warp-parity.md)
  (2026-11-02): the menu's design is untouched; this adds the source-pinned
  `CompletionsFallbackStrategy::FilePaths` default and VS Code's
  files-and-folders fallback, neither of which that record covered.
- **Consumes** [completions-seed-audit](completions-seed-audit.md)
  (2026-11-03): today's `completions.toml` schema (subcommands/flags,
  `deny_unknown_fields`) is exactly why per-argument path declarations are
  a named growth path here, not v1.
- **Extends** [autosuggest-alias-resolution](autosuggest-alias-resolution.md)
  (2026-11-02): CDPATH stays rejected on its shell-state grounds.
- Supersedes nothing: the bare-word ruling at `docs/autosuggestions.md:25`
  was a ruling, not a record; this is its first review against primary
  sources.

## Sources

All fetched 2026-11-06.

- fish @ master `d92ccd4f8f6f08cfc8a0d9fa17333153e2922af1` (2026-11-03):
  `src/complete.rs` (:69-75 `FileCompletionPolicy` default `Inherit`;
  :941,978-983 `do_file` + "If we're autosuggesting, and the token is empty,
  don't do file suggestions"; :986-988 cd special-case; :1765-1771
  `PathFilter::Directory`/`ForCdArgument`), `src/reader/reader.rs`
  (:5479-5492 completions fallback, `completions[0]`),
  `src/expand.rs` (:1517-1561 CDPATH working dirs, "The current directory is
  always valid"), `src/wildcard.rs` (:965-978 cd autosuggest
  descend-unique-hierarchy, issue #4402), `share/completions/cd.fish`,
  `share/functions/__fish_complete_cd.fish`, `share/completions/git.fish`
  (:120 `-f` helper, :1055 subcommand entries) —
  <https://github.com/fish-shell/fish-shell>; fish docs: Autosuggestions
  ("based on command history, completions, and valid file paths") —
  <https://fishshell.com/docs/current/interactive.html>; Writing your own
  completions ("fish will also offer files by default…") —
  <https://fishshell.com/docs/current/completions.html>
- zsh @ master `30e8be913521523d226789e53a4a2202dabff61c` (2026-11-01):
  `Completion/Zsh/Context/_default` (`#compdef -default-` → `_files "$@"`),
  `Completion/Zsh/Command/_cd` ("Normally just completes directories. Uses
  cdpath…", `_path_files -/`) — <https://github.com/zsh-users/zsh>; zsh
  manual, Completion Using compctl ("-D controls default completion
  behavior… If no compctl -D command has been issued, filenames are
  completed.") —
  <https://zsh.sourceforge.io/Doc/Release/Completion-Using-compctl.html>
- Bash Reference Manual, Edition 5.3 (18 May 2025, Bash 5.3), official
  mirror: §8.4.6 `complete (TAB)` ("If none of these produces a match, it
  falls back to filename completion."), §8.6 (no-compspec fallback chain),
  §8.7 (`complete -D`/`-E`, `-o bashdefault`/`-o default`, the
  `_completion_loader` example) —
  <https://tiswww.case.edu/php/chet/bash/bashref.html>
- Warp @ master `f7fc2f2a08a3aa446baa23960710e630c5defd6e` (2026-11-06;
  AGPL-3.0, survey only): `crates/warp_completer/src/completer/suggest/mod.rs`
  (:582-617 `CompletionsFallbackStrategy { FilePaths, None }`, default
  `FilePaths`), `…/engine/argument/legacy.rs` (:93-107 no-spec fallback →
  `sorted_paths_relative_to`; :577-598 no-argument-types fallback; :673-727
  Files/Folders templates), `…/engine/path.rs` (`sorted_cd_directories`,
  CDPATH) — <https://github.com/warpdotdev/warp>; Warp completions docs
  ("commands, options, and paths by pressing Tab") —
  <https://docs.warp.dev/terminal/command-completions/completions>
- Fig: `@withfig/autocomplete-types` 1.31.0 (npm), `index.d.ts:5-29`
  (filepaths/folders templates; "`cd` uses the "folders" template") via
  <https://unpkg.com/@withfig/autocomplete-types>; withfig/autocomplete @
  `aef52acff84c` per [completions-seed-audit](completions-seed-audit.md);
  the Fig desktop engine's own fallback: **unverified** (no live primary
  source)
- VS Code @ main `03d396a6e01bf958211fde9f2b279619881d68b2` (2026-11-06):
  `extensions/terminal-suggest/src/terminalSuggestMain.ts` (:556-559 the
  files-and-folders fallback; :384-427 cwd resolution for bare prefixes),
  `…/src/fig/figInterface.ts` (:186-275 template → showFiles/showDirectories),
  `…/src/completions/cd.ts` (`template: 'folders'`),
  `src/vs/workbench/contrib/terminalContrib/suggest/browser/terminalSuggestAddon.ts`
  (:307-312 `allowFallbackCompletions`), `…/suggest/common/terminalSuggestConfiguration.ts`
  (:112 `quickSuggestions.unknown` "files and folders will be suggested as a
  fallback"; :169-177 `cdPath`, default `'absolute'`) —
  <https://github.com/microsoft/vscode>
- carapace-bin @ master `63b7842024e8c5651e8a61fcbb799778235ca4e4`
  (2026-11-06): `cmd/carapace/cmd/completers/completers.go` (:304-314
  unknown-completer error), `cmd/carapace/cmd/invoke.go`
  (`invokeCompleter` empty on lookup failure); README and docs site — no
  stated fallback policy — <https://github.com/carapace-sh/carapace-bin>,
  <https://carapace-sh.github.io/carapace-bin/>
- Peers: Ghostty @ main `src/config/Config.zig` (zero completion/
  autosuggest/autocomplete keys; latest tag v1.4.0) —
  <https://github.com/ghostty-org/ghostty>; kitty v0.50.0 (2026-11-03)
  shell-integration docs ("Sophisticated completion for the kitty command
  in the shell") — <https://sw.kovidgoyal.net/kitty/shell-integration/>;
  Alacritty v0.17.0 (2026-04-06, unchanged) —
  <https://github.com/alacritty/alacritty>; WezTerm @
  `e68a8ec27727fbbda15e07f0a0737cd5e16481f6` (2026-10-26, unchanged) —
  <https://github.com/wezterm/wezterm>
- ble.sh wiki Manual §7 (auto-complete default on; syntax/programmable-
  completion source; `complete_timeout_auto` pathname-expansion timeout) —
  <https://github.com/akinomyoga/ble.sh/wiki/Manual-%C2%A77-Completion>;
  Clink docs v1.9.35 (`autosuggest.enable` default true;
  `autosuggest.strategy` default `match_prev_cmd history completion`;
  "completion chooses the first of the matching completions") —
  <https://chrisant996.github.io/clink/clink.html>
- DropTerm local tree @ HEAD `c48c5267a35d` (2026-11-06):
  `src/terminal/suggest.rs`, `src/terminal/engine.rs`,
  `docs/autosuggestions.md`; prior records
  [autosuggest-stage2-crosscheck](autosuggest-stage2-crosscheck.md),
  [autosuggest-warp-parity](autosuggest-warp-parity.md),
  [completions-seed-audit](completions-seed-audit.md),
  [autosuggest-alias-resolution](autosuggest-alias-resolution.md)
