# Research: where does the specs table's breadth come from — Fig's spec corpus, carapace's man-generated YAML, fish's completion files, or the tab's own history?

Date: 2026-11-07
Question: *The completions menu knows five commands in-code plus an
audited fourteen-command seed the user copies in. For any command
outside them (`rustc --em`, `node --ver`, `kubectl get`) every provider
is silent and the menu refuses to open. Which completion-knowledge
source should feed the table at scale — hundreds of commands with
subcommands, long flags, and descriptions — inside the standing
rulings: no shell integration, nothing executes at the terminal's
behest beyond the reviewed git generators, nothing reads man pages or
`--help` at runtime, dynamic sources only as fixed reviewed argv in
source, nothing leaves the machine?*

## TL;DR

**Scale the table from the withfig/autocomplete spec corpus — a
one-time, maintainer-run static conversion exactly like the fourteen
already shipped, grown from 14 commands toward the 440 purely static
specs of its 715 — and reimplement the spec-walking parser in Rust from
microsoft/inshellisense, the one actively-maintained MIT engine that
consumes the same format.** The corpus is MIT-licensed with a uniform
notice (one license line to carry, not 755 per-command audits), 61.5%
of its specs are purely static at the pinned commit (440/715 carry no
`generators:` key, no `script:`, no `loadSpec`, no `generateSpec`),
descriptions are a first-class field on every subcommand and option,
and it is the same source the shipped seed already proved the
conversion for — the same source VS Code pins in its cgmanifest, Warp
credits in its open-source README, and inshellisense (600+ tools,
pushed 2026-10-19) runs as a terminal-native PTY wrapper. The upstream
dormancy (frozen at v2.692.3, 2025-05-05, 105 unmerged community PRs)
is priced and acceptable because DropTerm owns the converted snapshot,
never the live repo. Carapace's newly-checked-in `man/cmd/` YAML corpus
(755 commands, serde-trivial, same-day-active upstream) is the runner-up
and the better *format*, but every line of it is verbatim man-page text
whose license varies per command — an unauditable per-content licensing
tail, and man-derived machine truth against the project's curation
frame. fish's 1,097 completion files have the best description coverage
(94% of 33,603 lines) and a uniform line grammar, but are GPL-2 (cannot
ship inside GPL-3.0-only), carry fish code in 51% of lines (`-n`
conditions — where the subcommand structure lives) and 74% of `-a`
values, and would hang breadth on whether the user installed fish.
History-learned flags are the floor, not the answer. zsh compsys and
bash-completion are shell programs, not data — closed with pinned
evidence. No terminal emulator in the field ships its own knowledge
source: Warp and VS Code both stand on the Fig specs; Windows Terminal's
Suggestions UI is shell-integration-powered history; Tabby bundles
Clink; Wave, Ghostty, kitty, WezTerm ship nothing.

This record **extends** [autosuggest-warp-parity](autosuggest-warp-parity.md)
(consumes its Phase B ruling: the loadable data file plus a one-time
audited conversion), **extends** [completions-seed-audit](completions-seed-audit.md)
(the conversion rules are its rules, at scale), and **extends**
[suggest-menu-layering](suggest-menu-layering.md) (breadth feeds the
menu surface it examined). Nothing here supersedes a ruling; the
2026-11-02 record's own rejection ledger (carapace as a *runtime
binary*, Warp *code*, *runtime* TS parsing) all still stands — the
winner is the data path that record already named, now measured.

## What DropTerm has today

Grounded in the working tree at HEAD `be7ae913f0f6` (2026-11-06),
GPL-3.0-only (`Cargo.toml:6`, `LICENSE`):

- **The specs provider is a flat per-command table with two seats.**
  `SpecCommand { name, subcommands: Vec<SpecEntry>, flags: Vec<SpecEntry> }`
  with `SpecEntry { name, description: Option<String> }`, every tier
  `#[serde(deny_unknown_fields)]` (`src/terminal/suggest.rs:774-805`) —
  the structural no-script boundary: a `script` key fails the whole
  file (`src/terminal/suggest.rs:769-773`). The built-in five are the
  `SPECS` const (`src/terminal/suggest.rs:648-762`), file-shaped on
  first use (`BUILTIN_SPECS`, `:810-833`).
- **The loadable file shipped.** `load_completions` resolves
  `completions.toml` beside the config, parses, and merges it over the
  built-in floor — a re-declared command replaces its built-in whole
  (`src/terminal/suggest.rs:862-917`); failures fall back to the
  built-in in silence. Documented at `docs/configuration.md:287-289`.
  The fourteen-command audited seed
  ([`data/completions.toml`](https://github.com/l1br3-io/cosmic-ext-dropterm/blob/main/data/completions.toml),
  32 KB) is a repo artifact the user copies into the config directory —
  it is `include_str!`ed only by the no-shrink test
  (`src/terminal/suggest/tests.rs:2998`), not embedded in the shipped
  binary, so a default install knows the five.
- **Seat rules** (`specs_context`, `src/terminal/suggest.rs:966-986`):
  the subcommand seat is the first argument seat only
  (`completed.len() == 1`); a `--`-prefixed trailing token is the flag
  seat, consulted from any argument seat; short flags never complete;
  the command word resolves through the user alias map. The menu's
  whole-set twin `specs_candidates` (`:1450-1478`) lists every
  strict-prefix entry with its description in the dim column
  (`MenuCandidate.description`, `:1284-1290`), and the menu ranks the
  specs tier second behind paths (`menu_source_rank`, `:1343-1351`).
- **The hard rulings, as written** (`docs/autosuggestions.md`): no
  shell integration by construction — "no scripts shipped, no
  environment injected" (`:9`); "nothing runs `--help`, nothing reads
  man pages, and there is no data file to edit" (`:57` — the last
  clause describes the pre-file table; the loadable file landed
  2026-11-03 with scripts structurally excluded, per the
  [autosuggest-warp-parity](autosuggest-warp-parity.md) index row); the
  generators' contract — "Fixed, reviewed, read-only commands … the
  same argv in the source, reviewed and versioned like any code.
  Nothing you configure — no config key, no data file — can add, alter,
  or script a generator" and "Nothing leaves the machine" (`:86-87`).
  Build-time conversion by maintainers is the established pattern: the
  seed's own header calls regeneration "a one-off, never a build step"
  (`data/completions.toml:20`), and the warp-parity record's Phase B
  named "a one-time offline conversion from withfig/autocomplete (MIT)
  into DropTerm's minimal format … shipped under the per-content
  licensing audit" as the breadth path.
- **The gap.** Outside the five (and the fourteen for users who copy
  the seed), `rustc --em`, `node --ver`, `kubectl get` all fall through
  to history-only matches — `specs_context` finds no table entry and
  every other provider is seat-silent by design (`docs/autosuggestions.md:56`:
  "Commands outside the five are silent - the provider never guesses").

## Findings

### 1. withfig/autocomplete, measured at the pinned commit — the breadth is there and 61.5% of it is purely static

Cloned `--depth 1` into a fresh `/tmp` directory on 2026-11-06 and
pinned: **`aef52acff84c45edde61ae610cc2c964802b9a38`** (v2.692.3,
commit dated 2025-05-05 — the same commit the seed audit pinned; the
repo has not moved since, see finding 3). All numbers below are
commands run against that checkout.

- **715 top-level specs** (`ls src/*.ts | wc -l` → 715), 440,311 total
  lines. The `src/` directory weighs 102 MB but that is mostly spec
  *asset* subdirectories — `src/aws` 46 MB, `src/az` 21 MB,
  `src/gcloud` 20 MB (`du -sh src/*`) — which inshellisense explicitly
  refuses to load for exactly that reason ("Specs for the `az`,
  `gcloud`, & `aws` CLIs are not supported in inshellisense due to
  their large size", README "Unsupported Specs",
  `microsoft/inshellisense` @ `9ba441d`, `README.md:171`).
- **Dynamic-feature fractions, counted by key** (system `grep -lE` over
  `src/*.ts`): `generators:` in 256 files; `script:` in 175; `loadSpec`
  in 30; `generateSpec`/`generateOctopusSpec` in 25; `template:` in 410
  (templates are engine-known generators like `filepaths`/`folders`/
  `history` — they affect argument completion only, never the
  subcommand/flag data). **Purely static — no `generators:`, no
  `script:`, no `loadSpec`, no `generateSpec`: 440 of 715 files
  (61.5%).** Even counting a `template:` anywhere as dynamic, 200 files
  remain.
- **Descriptions are standard.** A purely static sample,
  `src/mkdir.ts`: every option carries one — `name: ["-m", "--mode"],
  description: "Set file mode (as in chmod), not a=rwx - umask"`, and
  the spec itself `description: "Make directories"`
  (`withfig/autocomplete` @ `aef52acf`, `src/mkdir.ts:1-34`). A
  dynamic sample, `src/gh.ts:1-4`: `import { filepaths, keyValue }
  from "@fig/autocomplete-generators"` and real TypeScript
  `postProcess` functions — exactly what the conversion drops.
- **License, precisely.** The repo `LICENSE` is MIT text, "Copyright
  (c) 2021 Hercules Labs Inc. (Fig)" (`LICENSE:1-3`) — what the seed
  audit verified and what the shipped seed carries
  (`data/completions.toml:32-34`). One wrinkle this survey adds: the
  repo's own `package.json` declares `"license": "ISC"`
  (`package.json:77`), and the npm package
  `@withfig/autocomplete@2.692.3` (the compiled specs) also declares
  ISC with a 73.2 MB unpacked size (registry metadata, fetched
  2026-11-06). MIT and ISC are grant-equivalent permissive licenses
  with the same single-notice condition; the seed's header already
  preserves the MIT notice verbatim, and the honest position for a
  scaled conversion is to carry the repo's LICENSE text as the seed
  does. The types package `@withfig/autocomplete-types` is MIT
  (`package/package.json:7` of the 1.31.0 tarball).
- **Rough size of a static extraction.** The shipped seed is 32 KB of
  TOML for 14 commands (~2.1 KB per command, descriptions included —
  and the seed's fourteen are description-dense picks like kubectl's 32
  subcommands). Extrapolated across ~700 commands that is **on the
  order of 1.5 MB of TOML**; a curated top-100/200 subset is 200-450 KB.
  This is an extrapolation from the shipped seed, not a measurement —
  the precise number lands when the conversion script runs.

### 2. The format is typed, documented, and its static subset maps onto DropTerm's schema with no seat changes needed for breadth

The spec type definition ships as `@withfig/autocomplete-types` —
registry tarball `autocomplete-types-1.31.0.tgz` fetched and extracted
2026-11-06 (no scripts run; `npm pack` equivalent via direct registry
curl; 1.31.0 is the latest published version and matches the repo's
devDependency `^1.31.0`, `withfig/package.json` devDependencies). Key
shapes in `package/index.d.ts` (1,300 lines):

- `interface Subcommand extends BaseSuggestion` — `name:
  SingleOrArray<string>` (aliases as arrays), optional recursive
  `subcommands: Subcommand[]`, `options: Option[]`,
  `args: SingleOrArray<Arg>`, `requiresSubcommand`,
  `filterStrategy: "fuzzy" | "prefix" | "default"`, `loadSpec`
  (`index.d.ts:524-643`). The doc comment on `subcommands` names the
  nesting pattern DropTerm would one day grow: "To support large CLI
  tools, `Subcommands` can be nested recursively" (`:546-550`).
- `interface Option extends BaseSuggestion` — `name:
  SingleOrArray<string>` (short+long pairs like `["-m", "--message"]`),
  `args`, `isPersistent` (cobra-style persistent flags: "it will still
  be available as an option for all child subcommands", `:708-760`),
  `isDangerous`, `exclusiveOn`, `insertValue` (`index.d.ts:708+`).
- `interface Generator` — `template` (filepaths/folders/history/help),
  `script` (a command array or a *function* building one), default
  `scriptTimeout: 5000`, `postProcess` (`index.d.ts:1116-1175`). This
  is the executable half; the destination schema's
  `deny_unknown_fields` already refuses it structurally
  (`src/terminal/suggest.rs:769-773`).

**What the static subset keeps and loses.** Keeps: subcommand names and
descriptions, long-flag names and descriptions (short flags exist but
DropTerm's rulings exclude them — `docs/autosuggestions.md:53`), the
multi-name alias convention (the seed's rule 2 already collapses it to
the primary/first-long name), `requiresSubcommand` (mappable to the
subcommand seat), `isPersistent` (mappable to the flag seat's current
"flags from any argument seat" approximation — DropTerm already treats
git's flags as position-independent for exactly this reason,
`src/terminal/suggest.rs:952-958`). Loses: generators/templates (the
dynamic values — for git, DropTerm's own reviewed generators already
ship under a stricter contract than Fig's), `loadSpec` sub-files (the
cloud spec trees inshellisense also refuses), nested subcommands and
per-subcommand options (a *walker* feature, not a data feature — the
top-level tables alone already answer `kubectl get`, `rustc --em`,
`node --ver`), and `args` templates (argument seats stay the path
provider's and git generators' territory). Crucially, **going from 14
to hundreds of commands needs zero changes to `specs_context`,
`specs_candidates`, or the menu**: the seats, the match policy, and the
description column are all per-command table lookups that already work;
only the table grows.

### 3. The precedent chain is now three shipping consumers plus one removal — the data is the standard, the engines churn

- **Warp builds on the specs.** The open-source client's README credits
  "[Fig Completion Specs](https://github.com/withfig/autocomplete)" in
  its dependency list (`warpdotdev/warp` README.md:81 @ main, fetched
  2026-11-06; first pinned by [autosuggest-warp-parity](autosuggest-warp-parity.md)
  at master `d57d9760` with the engine internals: the
  prefix-then-subsequence fuzzy matcher in
  `crates/warp_completer/src/completer/matchers.rs`, the
  `Description` struct in `describe.rs`). Warp's *engine* is AGPL-3.0 —
  unusable as code, irrelevant as data: the data path is Fig's.
- **VS Code pins the specs in its manifest.**
  `extensions/terminal-suggest/cgmanifest.json` @ `67710841ad061a`
  registers withfig/autocomplete @ `1cc34dc1ba530bb620bd380c25cfb9eb2264be89`
  (v2.684.0, MIT), with the synced specs under
  `src/completions/upstream/` (101 `.ts` files) beside Microsoft's own
  MIT-licensed specs (pinned by
  [autosuggest-warp-parity](autosuggest-warp-parity.md) finding 4).
- **inshellisense is the live engine, and the architecture DropTerm
  would reimplement.** `microsoft/inshellisense` @
  **`9ba441d39468a73d4e6aebe8bff9af37fad92e86`** (2026-10-19, MIT,
  active). Its README: "a terminal native runtime for autocomplete
  which has support for 600+ command line tools" (`README.md:3`).
  How it loads specs: it depends on the *compiled* npm package
  `"@withfig/autocomplete": "2.675.0"` (`package.json:47`) and lazily
  resolves each command's spec from the package's resource files,
  indexed by a route trie built at startup (`loadSpecsSet`,
  `src/runtime/runtime.ts:30-52`), with `lazyLoadSpec` for `loadSpec`
  references (`runtime.ts:75`). How it tracks the buffer: it is a PTY
  wrapper *outside* the shell — `src/isterm/pty.ts` spawns the shell
  through `node-pty` and mirrors the byte stream through
  `@xterm/headless` (`pty.ts:9-19`), with `CommandManager` maintaining
  prompt/command state off the headless buffer
  (`src/isterm/commandManager.ts:23-44`) — DropTerm's own grid-scrape
  plus `tcgetpgrp` truth is the same terminal-side stance arrived at
  from the other direction. The parser to reimplement in Rust:
  `src/runtime/parser.ts` (154 lines — the lexer into `CommandToken`s:
  quotes, flags, escape-aware word splitting, `||`/`&&`/`;`/`|`
  command splitting), `src/runtime/runtime.ts` (`getSuggestions`
  `:106-141` — parse → alias-expand → load spec → the recursive
  `runSubcommand`/`runOption`/`runArg` walk handling persistent
  options, options-with-args, variadic and optional args,
  `exclusiveOn`, `loadSpec`), and `src/runtime/suggestion.ts` (the
  prefix default with per-spec fuzzy opt-in `:160-239`, the
  priority-then-relevance sort `:119-158`, accepted-token removal).
- **Amazon Q Developer CLI: the Rust/TS split, and the exit.** The Fig
  product became the Q CLI ("Amazon Q Developer CLI, formerly known as
  [Fig](https://fig.io), is open source" — withfig/autocomplete
  README.md:10 @ `aef52acf`). In the Q CLI the *engine was TypeScript*
  and the *integration was Rust*: at the last commit before removal —
  parent `5c621df6fd3112eb44da02132cde06cad25cb418` (2025-07-03) — the
  tree carries `packages/autocomplete/src` (45 TS files),
  `packages/autocomplete-parser/src/loadSpec.ts` and
  `tryResolveSpecToSubcommand.ts` (the spec walker),
  `packages/autocomplete-app/src` (45), `packages/shell-parser/src`,
  beside Rust crates `figterm` (PTY, input, term —
  `crates/figterm/src/{pty,input,term}/…`), `fig_ipc`, `fig_proto`,
  and the rest of the `fig_*` family (repo tree via GitHub API,
  fetched 2026-11-06; the repo is dual Apache-2.0/MIT,
  `LICENSE.APACHE` + `LICENSE.MIT`). On 2025-07-03 PR #2224 "chore:
  remove autocomplete" deleted it (commit-search API: `09aa3cbad570`,
  2025-07-03); the repo's HEAD at clone (`15cc8f3cd18c`, 2026-04-23)
  is an agentic chat CLI ("Agentic chat experience in your terminal",
  repo description) with zero autocomplete or fig mentions in README
  or docs (grep), still unarchived (pushed_at 2026-08-24). The lesson
  for DropTerm: engines come and go — Amazon deleted theirs — but the
  *data format* outlived every one of them (three live consumers
  after the deletion).
- **Stewardship of the data, checked 2026-11-06:** withfig/autocomplete
  `pushed_at` 2025-05-05, not archived, 247 open issues, 105 open PRs
  (API pagination; the first is a community spec contribution, "feat
  (micromamba): add completion spec"), 25,202 stars. Frozen, not dead:
  community contributions still arrive and still merge nothing. For a
  one-time audited snapshot this is stability, not risk — the
  warp-parity record's own pricing ("the conversion is a snapshot
  DropTerm owns, not a live dependency").

### 4. carapace: the Go completers are still programs — but the new `man/cmd/` YAML corpus is real static data with a licensing tail

`carapace-sh/carapace-bin` @ master
**`63b7842024e8c5651e8a61fcbb799778235ca4e4`** (2026-11-06 — the
morning of this survey; MIT, "Copyright (c) 2020 rsteube"; actively
maintained). Measured from the full git tree (GitHub API, fetched
2026-11-06):

- **The 1,556 completers under `completers/<platform>/<cmd>/` are Go
  programs** (bucketed: 551 common, 286 darwin, 239 linux, 134 unix,
  121 windows, plus per-shell wrappers) — confirming the prior
  record's verdict: using them means running the carapace binary, a
  runtime dependency shape DropTerm rejected.
- **The new corpus: `man/cmd/` carries 6,668 checked-in YAML files
  across 755 distinct commands** — machine-generated from man pages by
  [carapace-spec-man](https://github.com/carapace-sh/carapace-spec-man)
  ("Spec generation for manpages", README; its worked example shows
  `touch`'s flags extracted verbatim, `-a: change only the access
  time`). Sample `man/cmd/rustc/rustc.yaml`: `name: rustc`,
  `description: compiler for the Rust programming language`, 25 flags
  with descriptions — `--emit=: Comma separated list of types of
  output for the compiler to emit`, `-o=: Write output to FILENAME` —
  plus carapace-spec's value-taking (`=`), repeatable (`*`), and
  short/long pairing annotations (`-A, --allow=`). The format
  (`carapace-sh/carapace-spec`, MIT, pushed 2026-11-05) is flat YAML:
  name, description, flags, persistentflags, commands (subcommands
  with nesting), completion macros (`$files`, `$directories`) that a
  reader can simply skip.
- **Why it ranks second, not first.** (a) *Provenance*: every
  description is verbatim man-page text. Man pages are licensed
  per-command — GNU coreutils pages are GPL, BSD tools' pages BSD,
  many undocumented — so shipping this corpus means a per-content
  audit across 755 upstreams (the
  [terminal-theming](terminal-theming.md)/[completions-seed-audit](completions-seed-audit.md)
  pattern at 54× the scale), and GPL-2-only man text cannot ship
  inside a GPL-3.0-only project at all. carapace's repo-level MIT
  covers the *files*; it cannot relicense the *content*. (b) *Truth
  model*: it is man-page truth scraped by a parser — against
  DropTerm's "curated, not derived" frame, where the Fig specs are
  hand-written by their community even if DropTerm's conversion is
  mechanical. (c) Staleness: regenerated when maintainers run it, not
  tied to releases. What it *is*: the best-format fallback and a
  supplement pool for commands Fig lacks — serde_yaml against a flat
  schema is a weekend, not a Node toolchain.

### 5. fish completion files: the best descriptions, a uniform grammar — and three structural blockers

Measured from `fish-shell/fish-shell` @ master
**`d92ccd4f8f6f08cfc8a0d9fa17333153e2922af1`** (2026-11-03;
sparse-checked `share/completions` into `/tmp`, 2026-11-06). This
machine has fish barely installed (`/usr/share/fish/completions`
holds 1 file, `fd.fish`; `~/.local/share/fish/generated_completions`
is empty) — the runtime-dependency cost is not hypothetical.

- **Coverage and descriptions: 1,097 files, 33,603 `complete` lines,
  31,487 of them (93.7%) carrying `-d "description"`.** The line
  grammar is genuinely uniform — every completion is
  `complete -c CMD [-s short] [-l long] [-d desc] [-a args] [-n cond]`
  (`share/completions/rustc.fish` shows both shapes in one file:
  static `complete -c rustc -x -l check-cfg -d "Provide list of
  expected cfgs for checking"` beside dynamic `complete -c rustc -x
  -l target -xa "(rustc --print target-list)"`).
- **The structural problem: half the file is fish code.** 17,107 lines
  (50.9%) carry `-n` conditions, and the subcommand *structure lives
  inside them* — git's nested subcommands are
  `complete -f -c git -n "__fish_git_using_command remote" -n
  "__fish_seen_subcommand_from $remotecommands" -a '(__fish_git_remotes)'`
  (`share/completions/git.fish:1118-1120`). A static parser that drops
  conditioned lines loses the tree; one that keeps them cannot know
  when they apply. 7,552 lines carry `-a` argument lists and 5,588 of
  those (74%) embed fish code (command substitutions or variables) —
  safe to drop only because their values are unknowable.
- **License, precisely.** fish's COPYING: "Most of fish is licensed
  under the GNU General Public License version 2" (with PSF/MIT/LGPL
  components; `COPYING:1-13`). *Shipping* converted fish data inside
  this GPL-3.0-only repo is not available — GPL-2-only and GPL-3 are
  incompatible for a merged distribution, and the completion files are
  part of "most of fish". *Reading* `/usr/share/fish/completions` at
  runtime on the user's machine raises no distribution question at all
  — no copying occurs, so no license triggers; this is the "data read
  at runtime, not linked" answer, and it is clean. What it buys is the
  option, not the recommendation: correctness would depend on fish
  being installed (a terminal emulator that completes better only
  under fish inverts fish's own universality pitch), on the user's
  fish version, and on a hand grammar robust to 51% condition noise.
  The generated_completions tree (`~/.local/share/fish/
  generated_completions/`, man-derived by `fish_update_completions`)
  inherits the man-page licensing tail of finding 4 on top.

### 6. History-learned flags: honest evaluation — a floor, not a breadth source

Mining `--flags` per command from the tab's ring is free (the ring
already exists, `src/terminal/suggest.rs:251-262`), needs no license,
no parser, no dependency — and answers none of the question. Coverage
is exactly "what this user already typed in this tab": a command never
run completes nothing (the fresh-tab case the breadth ask is about),
a flag never typed is invisible, and there are no descriptions, ever
(the menu's dim column stays empty). It also adds little the history
provider does not have: `git che` already ghosts `ckout main` from the
ring's whole-line prefix match (`history_candidates`,
`src/terminal/suggest.rs:1373-1384`); per-word mining would only
re-slice the same lines into more, smaller rows. Where it genuinely
helps is as a menu *enrichment* — the distinct flags of this command
surfaced as one row each even when whole-line history rows bury them —
a later module at most, not the breadth source.

### 7. zsh compsys and bash-completion are programs, not data — the door closed with pinned files

- **zsh**: a completion is a shell function registered by a `#compdef`
  tag. `Completion/Unix/Command/_gcc` @ zsh master
  `30e8be913521523d226789e53a4a2202dabff61c` (2026-11-01): after the
  `#compdef gcc g++ cc c++ …` header it is imperative shell code —
  `local curcontext="$curcontext" state line ret=1 expl i`, `typeset
  -A opt_args`, branching on `$service` — that *constructs* its
  argument list in variables and hands it to one giant `_arguments -C
  -M 'L:|-{fWm}no-=-{fWm} r:|[_-]=* r:|=*' …` call at line 2161. The
  spec is not in the file; it is the *result of running* the file.
  (zsh's tree has no `_rustc` at all — rustc completion ships with
  rustup, i.e. in yet another per-project format.)
- **bash-completion**: `/usr/share/bash-completion/completions/git` on
  this machine (bash-completion 2.11-8) is 3,661 lines of bash
  functions under a GPL-2.0 header ("Distributed under the GNU General
  Public License, version 2.0", `:4`). `__git_main` dispatches on
  `$prev`/`$cur` word positions, calls `__git_ ...` helpers, runs git
  itself (`ls-remote`, `for-each-ref`) to build `COMPREPLY`. Same
  verdict: the knowledge is the execution.

Neither can be parsed by any grammar; both require a shell evaluator,
which is the integration DropTerm ruled out by construction
(`docs/autosuggestions.md:9`).

### 8. The field check: no terminal ships its own knowledge source — the ones that ship anything stand on the Fig specs

- **Warp**: spec-driven menu; credits Fig Completion Specs
  (`warpdotdev/warp` README.md:81 @ main; engine internals pinned by
  [autosuggest-warp-parity](autosuggest-warp-parity.md) finding 1).
- **VS Code (terminal)**: Fig specs pinned in cgmanifest, shipped under
  `extensions/terminal-suggest/src/completions/upstream/` (finding 3).
- **Windows Terminal**: the Suggestions UI (`showSuggestions`) is
  sourced from `"recentCommands"` — "powered by shell integration, so
  they will only be available if you have your shell configured to
  support shell integration" — and `"tasks"` (the user's `sendInput`
  actions); still Preview-only
  ([actions docs](https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions),
  updated 2025-08-21). No spec knowledge.
- **Tabby**: a full-tree scan of `Eugeny/tabby` @ master (995 entries,
  GitHub API, 2026-11-06) finds no completion code — the only
  completion-shaped content is bundled **Clink binaries** for Windows
  (`extras/clink/clink_x64.exe`, `clink_dll_x64.dll`, …), i.e. a
  line-editor extension, not terminal-side knowledge.
- **Wave**: full-tree scan of `wavetermdev/waveterm` @ main (1,332
  entries): zero paths matching complet/autosuggest.
- **Ghostty (v1.4.0), kitty (v0.49.2), WezTerm, Alacritty (v0.17.0)**:
  nothing shipped, re-verified 2026-11-02 by
  [autosuggest-warp-parity](autosuggest-warp-parity.md) finding 8.

So the field's answer to "where does the knowledge come from" is,
where it exists at all: Fig's corpus. DropTerm joining that path is
not an outlier bet on a dormant repo; it is adopting the interchange
format with the widest living consumer base (VS Code, Warp,
inshellisense) after its original vendor exited.

### 9. Seat fit and shipping shape — what actually changes in DropTerm

- **Nothing, for breadth itself.** `specs_context`,
  `specs_candidates`, the match policy, the description column, the
  alias re-target, and the menu ranking are all per-command table
  lookups that already work at any table size
  (`src/terminal/suggest.rs:966-986`, `:1450-1478`). The conversion
  grows `data/completions.toml` (or a generated sibling) from 14
  commands to N under the seed audit's existing four rules
  ([completions-seed-audit](completions-seed-audit.md): top-level
  arrays only, one name per entry, descriptions verbatim, the built-in
  five re-declared complete) — with the audit's per-command table
  replaced by rule-based spot audits at scale, since 700 hand-audited
  rows is not a thing.
- **The embedding decision.** Today the seed is user-copied and the
  binary carries the five. Hundreds of commands should ship *in* the
  binary: a checked-in generated table loaded through
  `include_str!`-into-`parse_completions` at the existing load points
  (`src/app.rs:1747`, `src/app/spawn.rs:517`) as a second built-in
  source under the user file, keeping single-binary packaging (the
  deb/AUR/nix shapes of
  [packaging-and-release](packaging-and-release.md)) — a checked-in
  file is still "a one-off, never a build step"
  (`data/completions.toml:20`); only the *regeneration* is
  maintainer-run. ~1.5 MB of TOML parsed once per load point is well
  inside the existing cost (the parse already happens per live config
  flip, `src/app.rs:2416-2418`).
- **The walker is the second step, and it has a pinned model.** The
  current seats are deliberately flat approximations
  (`specs_context`'s own doc: "the flat table's approximation of
  fish's per-subcommand gating … is where the growth path refines",
  `src/terminal/suggest.rs:952-958`). When per-subcommand flags,
  nested subcommands, options-that-take-arguments, and `--flag=value`
  arrive, the shape to copy is inshellisense's
  parser→walker→filter pipeline (finding 3) — ~750 lines of TypeScript
  whose static half ports to Rust without a Node anywhere near the
  runtime. That is precisely the REA target below.

## Recommendation

**Rank 1 — the withfig/autocomplete static corpus (adopt).** One-time
maintainer-run conversion at the pinned commit, grown in tranches
(top-100 first, then long-tail), shipped embedded as a built-in table
under the seed audit's rules and notice, with inshellisense's walker
as the reverse-engineering model for the seat refinement that follows.
Decisive reasons: (1) *licensing* — one uniform MIT/ISC notice, the
audit precedent already proven on this exact source at this exact
commit, versus 755 per-command man-page audits (carapace) or a GPL-2
workload (fish); (2) *provenance fit* — hand-written curated specs,
not machine-scraped man text, in a typed format whose static subset
maps 1:1 onto DropTerm's shipped schema with zero seat changes;
(3) *the field* — the format's living consumers (VS Code, Warp,
inshellisense) make this the interchange standard, and the one
actively-maintained engine is MIT and source-readable.

**Rank 2 — carapace's `man/cmd/` YAML (the named fallback/supplement).**
If the Fig extraction ever proves too TS-flavored at the margins, or
for the specific commands Fig lacks, `man/cmd/*.yaml` is
serde-trivial, description-rich, same-day-active upstream. Blockers:
per-command man-page licensing (GPL-2-only man text cannot ship in
GPL-3.0-only at all), man-derived staleness, and the curation-frame
mismatch. Revisit only as a per-command, per-license supplement.

**Rank 3 — fish files at runtime (optional user-side source, if ever
asked for).** License-clean *to read* (no distribution occurs) and
description-dense, but gated on fish being installed, half-noisy with
embedded fish code, and structurally unable to yield the subcommand
tree without a condition evaluator. Not the breadth path; a possible
later `completions_from_fish`-style opt-in at most.

**Rank 4 — history-learned flags (a menu enrichment, not a source).**
Mine the ring for the command's own seen flags as extra menu rows;
accept that it has no descriptions and no fresh-tab value. A later
module inside the existing history provider, not a data program.

**Rejected:** zsh compsys and bash-completion as data (finding 7 —
they are programs; no grammar exists to parse); carapace *the binary*
as a runtime backend (the standing
[autosuggest-warp-parity](autosuggest-warp-parity.md) rejection —
external Go dependency, zero terminal precedents); Warp engine code
(AGPL); any runtime TS/fig-spec parsing (conversion once, offline —
the standing ruling); and *shipping* fish-derived or man-derived data
without the per-content audit (licensing, findings 4-5).

## Unverified / open questions

- The 61.5% static fraction counts *files* containing dynamic keys;
  the per-*entry* fraction (static subcommands/options inside
  partially-dynamic specs) is higher and only the conversion run will
  measure it precisely — the gh/kubectl seed rows already demonstrate
  partial extraction from dynamic files.
- The ~1.5 MB shipped-size figure is an extrapolation from the
  fourteen-command seed (32 KB), labeled as such in finding 1; the
  real number lands with the first tranche.
- Whether Amazon or the community revives withfig/autocomplete (105
  open PRs are waiting) — immaterial to the snapshot strategy but
  worth re-checking before each tranche.
- The npm `@withfig/autocomplete` package declares ISC while the repo
  LICENSE is MIT text (finding 1); the conversion carries the repo
  LICENSE, and if the npm package is ever used as the extraction
  input instead of the git checkout, the ISC declaration should be
  carried too.
- inshellisense's `runSubcommand` walk was read for structure, not
  line-verified end to end; the REA pass below is where the edge
  cases (`=`-attached values, `exclusiveOn`, variadic bounds) get
  pinned before any Rust port.

## Next: REA target

The winner's implementation to reverse-engineer is **microsoft/
inshellisense's spec runtime** — the parser/engine that walks a Fig
spec against a typed line, which is what DropTerm would reimplement
in Rust — analyzed beside the **withfig data** it consumes.

- **Engine**: repo `https://github.com/microsoft/inshellisense`,
  pinned commit **`9ba441d39468a73d4e6aebe8bff9af37fad92e86`**
  (observed 2026-11-06; already fetched read-only to
  `/tmp/dropterm-research/inshellisense`). Fetch fresh:
  `git clone --depth 1 https://github.com/microsoft/inshellisense /tmp/rea/inshellisense && git -C /tmp/rea/inshellisense rev-parse HEAD`
  (verify `9ba441d39468…`; nothing in it is ever executed).
- **Data**: repo `https://github.com/withfig/autocomplete`, pinned
  commit **`aef52acff84c45edde61ae610cc2c964802b9a38`** (v2.692.3;
  already fetched to `/tmp/dropterm-research/withfig`):
  `git clone --depth 1 https://github.com/withfig/autocomplete /tmp/rea/withfig && git -C /tmp/rea/withfig rev-parse HEAD`.
  Package alternative (no install scripts): the registry tarball —
  `curl -sfL -o types.tgz 'https://registry.npmjs.org/@withfig/autocomplete-types/-/autocomplete-types-1.31.0.tgz' && tar -xzf types.tgz`
  (`@withfig/autocomplete-types@1.31.0`; the full compiled specs ship
  as `@withfig/autocomplete@2.692.3`, 73.2 MB unpacked, same
  registry, same one-liner with its tarball URL).
- **Files that matter most inside inshellisense**: `src/runtime/
  parser.ts` (the CommandToken lexer — quotes, flags, escapes,
  command splitting), `src/runtime/runtime.ts` (`getSuggestions`
  L106-141; the recursive `runSubcommand`/`runOption`/`runArg` walk
  with persistent options and option-args; `loadSpecsSet` L30-52 —
  how specs are bundled and lazily loaded), `src/runtime/suggestion.ts`
  (filter/sort/toSuggestion — the description field's path to the
  UI), `src/runtime/alias.ts` (alias expansion ahead of spec lookup),
  and `src/isterm/{pty,commandManager}.ts` only as architecture
  context (the PTY wrapper — DropTerm's grid-scrape replaces it).
- **Files inside withfig**: `src/mkdir.ts` (a purely static spec),
  `src/gh.ts` and `src/git.ts` (generator-bearing specs whose static
  halves still convert), plus `LICENSE` (the MIT notice the
  conversion preserves).
- **Secondary target** (the deleted Amazon engine, same walker in TS):
  `https://github.com/aws/amazon-q-developer-cli` @
  `5c621df6fd3112eb44da02132cde06cad25cb418` (the parent of the
  2025-07-03 removal), specifically `packages/autocomplete-parser/
  src/{loadSpec,tryResolveSpecToSubcommand}.ts` and
  `packages/autocomplete/src/`.

## Sources

All fetched 2026-11-06 unless noted.

- withfig/autocomplete @ `aef52acff84c45edde61ae610cc2c964802b9a38`
  (v2.692.3, 2025-05-05; MIT LICENSE, ISC package.json:77; 715
  src/*.ts; 440,311 lines; key-fraction greps and du run in the
  checkout): `src/mkdir.ts`, `src/gh.ts`, `README.md:5-10` (the
  "formerly known as Fig" notice), `LICENSE`, `package.json` —
  <https://github.com/withfig/autocomplete>; npm registry metadata for
  `@withfig/autocomplete` (latest 2.692.3, 73.2 MB unpacked, ISC) and
  the `@withfig/autocomplete-types@1.31.0` tarball (MIT,
  `index.d.ts` Subcommand L524, Option L708, Generator L1116) —
  <https://www.npmjs.com/package/@withfig/autocomplete-types>
- microsoft/inshellisense @ `9ba441d39468a73d4e6aebe8bff9af37fad92e86`
  (2026-10-19; MIT): `README.md:3,171`; `package.json:47`
  (@withfig/autocomplete 2.675.0); `src/runtime/parser.ts`;
  `src/runtime/runtime.ts:30-52,75,106-141,246-330`;
  `src/runtime/suggestion.ts:97-158,160-239,274-457`;
  `src/isterm/pty.ts:9-19`; `src/isterm/commandManager.ts:23-44` —
  <https://github.com/microsoft/inshellisense>
- aws/amazon-q-developer-cli: HEAD `15cc8f3cd18c4272925ce1c7053268eedff1ea0a`
  (2026-04-23, cloned; repo API pushed_at 2026-08-24, not archived,
  description "Agentic chat experience in your terminal"); pre-removal
  parent `5c621df6fd3112eb44da02132cde06cad25cb418` tree via API
  (`packages/autocomplete*`, `packages/shell-parser`, `crates/fig*`);
  removal commit `09aa3cbad570` (2025-07-03, "chore: remove
  autocomplete (#2224)") via the commit-search API —
  <https://github.com/aws/amazon-q-developer-cli>
- carapace: `carapace-sh/carapace-bin` @ master
  `63b7842024e8c5651e8a61fcbb799778235ca4e4` (2026-11-06; MIT) —
  full-tree API scan (1,556 Go completers; `man/cmd/` 6,668 YAML /
  755 commands; `man/cmd/rustc/rustc.yaml` fetched and quoted);
  `carapace-sh/carapace-spec` (MIT, pushed 2026-11-05) README (the
  YAML grammar); `carapace-sh/carapace-spec-man` README ("Spec
  generation for manpages") — <https://github.com/carapace-sh/carapace-bin>
- fish-shell @ `d92ccd4f8f6f08cfc8a0d9fa17333153e2922af1` (2026-11-03;
  sparse clone of `share/completions`): file/line/-n/-a/-d counts run
  in the checkout; `share/completions/rustc.fish`;
  `share/completions/git.fish:1118-1120`; `COPYING:1-13` —
  <https://github.com/fish-shell/fish-shell>; local machine:
  `/usr/share/fish/completions` (1 file),
  `~/.local/share/fish/generated_completions` (empty)
- zsh @ master `30e8be913521523d226789e53a4a2202dabff61c` (2026-11-01):
  `Completion/Unix/Command/_gcc` (compdef header; `_arguments` at
  L2161) — <https://github.com/zsh-users/zsh>; bash-completion 2.11-8
  local: `/usr/share/bash-completion/completions` (962 files), the
  git file (3,661 lines, GPL-2.0 header at `:4`)
- Peers: Windows Terminal actions docs (Suggestions UI:
  recentCommands/tasks sources, Preview-only; updated 2025-08-21) —
  <https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions>;
  `Eugeny/tabby` @ master full tree (Clink binaries in `extras/clink/`,
  no completion code); `wavetermdev/waveterm` @ main full tree (zero
  hits); `warpdotdev/warp` README.md:81 @ main (Fig credit); Warp
  engine internals, VS Code cgmanifest pin
  (`1cc34dc1ba530bb620bd380c25cfb9eb2264be89`, v2.684.0), and the
  Ghostty/kitty/WezTerm/Alacritty re-checks as pinned by this repo's
  [autosuggest-warp-parity](autosuggest-warp-parity.md) (2026-11-02)
- DropTerm local tree @ HEAD `be7ae913f0f6` (2026-11-06):
  `src/terminal/suggest.rs` (SPECS :648-762; SpecEntry/SpecCommand
  :774-805; load_completions :892-917; specs_context :966-986;
  specs_candidates :1450-1478; menu_source_rank :1343-1351),
  `src/app.rs:1747,2416-2418`, `src/app/spawn.rs:517`,
  `src/terminal/suggest/tests.rs:2998`,
  `data/completions.toml:1-34` (header, MIT notice, "a one-off, never
  a build step" at :20), `docs/autosuggestions.md:9,53-57,86-92,98-109`,
  `docs/configuration.md:287-289`, `Cargo.toml:6`, `LICENSE`;
  prior records [completions-seed-audit](completions-seed-audit.md),
  [autosuggest-warp-parity](autosuggest-warp-parity.md),
  [autosuggest-dynamic-generators](autosuggest-dynamic-generators.md),
  [suggest-menu-layering](suggest-menu-layering.md),
  [terminal-theming](terminal-theming.md) (the per-content audit
  precedent), [packaging-and-release](packaging-and-release.md)
