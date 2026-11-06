# Research: how can DropTerm resolve shell aliases for completion without shell injection?

Date: 2026-11-02
Question: *[autosuggest-warp-parity](autosuggest-warp-parity.md) (2026-11-02)
rejected alias resolution — "the expansion lives in the user's shell state" —
and Warp's docs still promise it ("gc=git checkout … should give you the same
completion options"). The owner has decided to add it: which mechanism lets
DropTerm complete through aliases while the no-injection verdict of
[terminal-de-integration](terminal-de-integration.md) (2026-10-02) survives
whole?*

## TL;DR

**DropTerm should resolve aliases through a user-declared alias map in its
own config — an `[autosuggestions.aliases]` table mapping alias to
underlying command — because it is the only mechanism that needs zero shell
state, has a shipping terminal precedent (Windows Terminal's user-configured
`sendInput` actions), and leaves both standing verdicts untouched; typing an
aliased command re-targets the specs/catalog providers at its expansion
(`gc --dr` ghosts the same flag `git checkout --dr` would), with history
still outranking everything, exactly as today.** The mechanisms that would
give *true* alias truth are each disqualified on the evidence: shell-state
extraction requires injection (carapace does it with rc-file wrappers and an
env protocol; it is the standing verdict's exact shape), heuristic rc-file
parsing has no precedent anywhere and fails on conditionals, `ZDOTDIR`,
sourced files, and functions-vs-aliases, and even Warp — the feature's
flagship — shipped alias resolution broken for fish for years
([warpdotdev/Warp#2281](https://github.com/warpdotdev/Warp/issues/2281)),
which is what shell-state dependence looks like at the best-funded
implementation. Passive grid observation is priced as the one honest
complement and deferred.

## What DropTerm has today

Grounded in the working tree at `fe1cd72`:

- **The seam an alias re-enters.** `specs_context` extracts the
  command/subcommand pair from the typed line
  (`src/terminal/suggest.rs:703`); `specs_candidate` (`:756`) matches it
  against the curated `SPECS` table (`:575`); `catalog_candidate` (`:528`)
  completes the command word from the `$PATH` snapshot;
  `merged_candidate` (`:850`) keeps history on top via `source_rank`
  (`:431`). An alias map slots in *before* `specs_context`: if the typed
  command word is a mapped alias, its expansion becomes the lookup
  context. No provider changes at all.
- **The quoting and honesty tiers alias results inherit.**
  `escape_for_shell` (`src/terminal/suggest.rs:804`) is the shared
  backslash/single-quote path every suggestion rides; the shell gate
  (silent on ssh and non-shell tabs) already governs every
  local-machine-knowledge provider.
- **Config precedent for user-declared terminal-owned data.** DropTerm's
  config is a serialized key-value surface with per-key docs gates
  (`src/config.rs:408-451`); profiles already carry user-declared command
  strings (the spawn-as-ssh family), so "the user tells the terminal what
  a command means" is an existing shape in this codebase, not a new one.

## Findings

### 1. Warp promises alias resolution and historically shipped it broken — the dependence on shell state is the failure mode

Warp's completions docs (read 2026-11-02) make the promise explicitly:
*"if you have gc=git checkout in alias, typing gc and hitting Tab should
give you the same completion options as for git checkout"* — and
distinguish shell aliases (whole commands) from command aliases
(subcommands). But
[warpdotdev/Warp#2281](https://github.com/warpdotdev/Warp/issues/2281)
(filed Dec 2, 2022) reports completions **not** respecting fish aliases —
"Warp's completions don't resolve aliases in fish shell — completions only
work when the alias is skipped, defeating the alias's purpose." The
flagship implementation of the feature could not read fish's alias table
without per-shell cooperation, and the issue trail shows it as a
long-standing feature request. The lesson for DropTerm is structural, not
a dig at Warp: alias truth lives in a per-shell runtime table, and every
mechanism that reaches for it either owns the input surface (Warp's
editor), injects into the shell, or guesses.

### 2. carapace: the honest version of shell-state extraction — and it is injection, in the standing verdict's exact shape

The one open ecosystem that resolves aliases robustly is carapace
([carapace.sh](https://carapace.sh), read 2026-11-02): completion is
registered by a shell wrapper installed in the user's rc files, and the
identity of the command being completed travels through the shell's own
invocation (`carapace $alias _carapace …`-style with a `$CARAPACE`
environment protocol), so the alias the shell expanded is the alias
carapace completes for. This works *because* carapace lives inside the
shell's runtime — it is shell integration in the full sense: shipped
snippets, rc modification, env protocol. That is precisely the surface
[terminal-de-integration](terminal-de-integration.md) (2026-10-02)
rejected — "a large, security-sensitive surface — shipping shell scripts
for 5 shells, injecting env" — so carapace is the counter-example that
confirms the boundary, not a precedent DropTerm can borrow.

### 3. VS Code's terminal-suggest does not resolve shell aliases at all — it resolves *git's* aliases, by running git

`extensions/terminal-suggest/src/completions/git.ts` @ `main` (MIT; read
2026-11-02) contains exactly one alias mechanism: a generator running
`git config --get-regexp "^alias."` — resolving the aliases *git itself*
stores in config, not the shell's. No shell-alias resolution exists in
the suggest addon's sources surveyed by the 2026-11-02 record, and none
is documented on the shell-integration pages: OSC 633 reports prompt,
command line, and cwd — not the shell's alias table. The richest
terminal-side implementation skipped the feature entirely.

### 4. Windows Terminal's `sendInput` actions: the precedent for user-declared, terminal-owned command data

Windows Terminal's actions surface
([learn.microsoft.com — Terminal actions](https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions),
read 2026-11-02; already surveyed by
[terminal-autocomplete](terminal-autocomplete.md) for the Suggestions
flyout) lets users declare named actions carrying arbitrary input strings
in `settings.json` — `sendInput` snippets like `"input":
"git status\u000D"` bound to chosen keys, with no defaults shipped. The
shape matters more than the feature: a terminal completing from *data the
user declared in the terminal's own config* needs no shell state, no
injection, and no heuristic — it is authoritative because the user wrote
it. DropTerm's profiles already carry user-declared command strings the
same way.

### 5. Rc-file heuristic parsing: no precedent found, and the failure modes are enumerable

No surveyed tool parses `~/.bashrc`, `~/.zshrc`, or fish's config to
recover aliases (searches across the completion ecosystems above; none
does it — an honest gap in the survey, marked as absence-of-evidence
rather than proof-of-absence). The failure modes are why: aliases defined
conditionally (`if` blocks), in sourced files, by functions that wrap
rather than alias, under a relocated config root (`ZDOTDIR`, `BASH_ENV`,
fish's `~/.config/fish/`), or mutated live in the running session
(`alias x=y` typed at the prompt never lands in the rc file at all).
fish's own docs treat the alias table as a *runtime* structure fish
serializes for its sessions — not a file format others are invited to
parse. A heuristic here would break the shipped honesty rule — it
produces *wrong* suggestions, the one failure mode the autosuggest stack
currently cannot produce.

### 6. Passive grid observation: honest but late

DropTerm already scrapes the grid at command-finish for history
(`commit`, `src/terminal/suggest.rs:870`); the same scrape could remember
`alias x=…` / `type x` outputs seen on screen into the alias map's cache.
It is shell-agnostic and injection-free, but it is *late* (an alias
completes only after the user has run something that printed its
expansion), fragile under quoting and paging, and it can never seed a
fresh tab. Priced as a complement that pre-seeds the user's declared map,
not a substitute for it.

## Recommendation

**Ship the config-map mechanism; defer passive observation; reject
extraction and heuristics.** Supersedes the alias rejection in
[autosuggest-warp-parity](autosuggest-warp-parity.md) (2026-11-02);
the no-injection verdict of
[terminal-de-integration](terminal-de-integration.md) (2026-10-02)
survives whole — nothing ships, injects, or parses shell config.

1. **`[autosuggestions.aliases]` in `DropTermConfig`** — a string map,
   alias → expansion (`gc = "git checkout"`). Serialized, documented in
   `docs/configuration.md` under the existing gate, live-flipping like
   the other autosuggest keys. The declared map is authoritative for
   exactly what the user declares — no guessing past it.
2. **Re-targeting, not rewriting.** When the typed command word matches a
   mapped alias, the specs/catalog lookups run against the expansion's
   words: `gc --dr` ghosts git's `--dry-run`, `gc che` ghosts `ckout`
   from the specs table — the same providers, the same merge order, the
   same ghost and keys. History still outranks the map always (the
   shipped rule verbatim); a typed alias absent from the map behaves
   exactly as today. Expansion chains (an alias mapping to another
   alias) resolve recursively with a depth cap, cycle-safe.
3. **Expansion is completion-time knowledge, never input.** The map only
   re-targets *suggestions*; accepting one writes the suggestion's
   suffix through the existing PTY funnel — DropTerm never rewrites what
   the user typed, never sends the expansion unless the user accepted
   ghost text that spelled it.
4. **Ride the shell gate.** The map is local-machine truth the user
   declared for their local shell; on a tab spawned as ssh the remote
   shell's aliases are not necessarily the map's — silent there, the
   same rule paths/commands/specs already follow.
5. **Ship surface.** Settings drawer field later (a small per-profile
   editor); config-file first, matching how profiles' commands already
   work.
6. **Later, if asked: passive pre-seed.** Grid-observed `alias`/`type`
   outputs may *suggest* entries into the map (never silently apply),
   behind its own toggle — the one honest complement, priced above.

## Rejected, and why

- **Shell-state extraction (running `alias`/`functions` in the session,
  or shipping an extraction snippet).** The carapace mechanism is the
  honest version and it is rc-file wrappers + env protocol — the standing
  verdict's exact rejected surface. Rejected; would reopen
  [terminal-de-integration](terminal-de-integration.md) (2026-10-02) for
  a feature a config table delivers.
- **Heuristic rc-file parsing.** No precedent anywhere surveyed; fails on
  conditional/sourced/live-mutated aliases and relocated config roots;
  produces wrong suggestions — the one failure the autosuggest stack's
  honesty rules forbid. Rejected.
- **Fig/Warp-style spec-embedded aliases.** Those resolve *git's*
  config aliases (VS Code's generator) or subcommand aliases in spec
  data — useful, and orthogonal: git-alias completion arrives free with
  [autosuggest-dynamic-generators](autosuggest-dynamic-generators.md)'s
  git generator set if that spec slice is ever adopted. It is not shell
  alias resolution and is not mistaken for it.
- **Silent grid-observation as the primary mechanism.** Late, fragile
  under quoting, cannot seed a fresh tab; only ever a pre-seed for the
  declared map, behind a toggle. Deferred, not adopted as primary.

Relationship to prior records: **supersedes** the alias-resolution
rejection in [autosuggest-warp-parity](autosuggest-warp-parity.md)
(2026-11-02); **extends** [terminal-autocomplete](terminal-autocomplete.md)
(2026-10-17) at the provider seam (`src/terminal/suggest.rs:703-756`);
[terminal-de-integration](terminal-de-integration.md) (2026-10-02) is
untouched — the verdict's boundary is the reason the winning mechanism won.

## Sources

- Warp completions docs (read 2026-11-02) — the alias promise ("gc=git
  checkout … same completion options") and its Full/Partial table:
  <https://docs.warp.dev/terminal/command-completions/completions>
- Warp alias-resolution failure report —
  [warpdotdev/Warp#2281](https://github.com/warpdotdev/Warp/issues/2281)
  ("warp completions do not respect fish aliases", filed 2022-12-02; read
  2026-11-02); custom-spec discussion
  [warpdotdev/warp#434](https://github.com/warpdotdev/warp/discussions/434)
- carapace — alias/command identity via shell wrappers and the `$CARAPACE`
  invocation protocol ([carapace.sh](https://carapace.sh) setup pages,
  read 2026-11-02; repo
  [carapace-sh/carapace](https://github.com/carapace-sh/carapace)) —
  mechanism summarized from its setup docs, not a raw rc snippet; marked
  as such
- VS Code `extensions/terminal-suggest/src/completions/git.ts` @ `main`
  (MIT; read 2026-11-02) — git-alias generator `git config --get-regexp
  "^alias."`; no shell-alias mechanism:
  <https://raw.githubusercontent.com/microsoft/vscode/main/extensions/terminal-suggest/src/completions/git.ts>
- Windows Terminal actions docs — user-declared `sendInput` actions with
  arbitrary input strings, no defaults (read 2026-11-02):
  <https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions>
- DropTerm local tree @ `fe1cd72` (read 2026-11-02):
  `src/terminal/suggest.rs`, `src/config.rs`; prior records
  [autosuggest-warp-parity](autosuggest-warp-parity.md),
  [terminal-autocomplete](terminal-autocomplete.md),
  [terminal-de-integration](terminal-de-integration.md)
- Honest gap: no surveyed tool parses shell rc files to recover aliases;
  recorded as absence of evidence across the surveyed ecosystems, not as
  proof none exists
