# How does Warp's completion menu actually behave — show rules, keys, ranking — now that its client is open source, and what should DropTerm copy?

**Question:** Warp's client went open source; what are the exact mechanics of
its autocomplete/suggestions surfaces (when the menu shows, how rows rank,
what every key does, where the data lives), and which of those behaviors
should DropTerm's completion menu adopt for parity?

**Date:** 2026-11-23

**TL;DR:** Warp's completions run as one async engine behind two triggers —
as-you-type (default path: recompute fresh rows on every user edit, gated on
setting + single cursor at buffer end + ≥2 typed chars + a live prompt, with
stale results discarded and a 750 ms staleness close, first row preselected,
no path fallback) and manual Tab (filters in place, allows the file-path
fallback, and resolves through single-insert / common-prefix-then-open /
open) — and while the menu is open every navigation key wraps, Tab fills the
longest common prefix of the filtered case-sensitive prefix rows (closing on
the last one) else cycles, Enter accepts-and-closes with a second Enter
executing, Escape closes untouched, and printable keys keep the menu alive
with rows recomputed. DropTerm should copy all of that observable behavior
except the 750 ms timer (our synchronous under-lock collection cannot be
stale; the observable invariant holds by construction), keep history rows
and the Ctrl+Space unconditional opener (our PTY reality diverges from
Warp's own-editor input), and gate Tab-while-closed on qualifying rows so
shell Tab survives. Behavior only: the Warp client is AGPL-3.0-only, so no
code crosses; the one consumable asset — the spec corpus — is MIT in its own
repo.

## Findings

All file:line citations are into
`github.com/warpdotdev/warp` @ `f8223ce` (committed 2026-11-20, sparse
cloned 2026-11-23), path prefix `app/src/` and `crates/` relative to repo
root. Warp's own prior survey legs stand: autosuggest is history-plus-
completions with no AI on the keystroke path
([ai-completion-providers](ai-completion-providers.md), finding 1).

### 1. The premise changed: the client is open source, and no binary leg exists to run

- The repo carries the full client — `crates/warp_completer` (the engine),
  `crates/fuzzy_match` (scoring), `app/src/completer` (session wiring),
  `app/src/input_suggestions.rs` (the menu UI), `app/src/terminal/input.rs`
  (trigger and key routing). Workspace `license = "AGPL-3.0-only"`
  (`Cargo.toml:27`); the `warpui`/`warpui_core` UI crates are MIT per the
  README. Nothing from the AGPL crates may be copied into GPL-3.0-only
  DropTerm — behavior replication only.
- Binary RE is moot twice over: source answers it, and no shippable binary
  is publicly reachable — GitHub releases are dev-channel with empty asset
  arrays (checked 2026-11-23, latest stable `v0.2026.06.03.09.49.stable_00`
  has `"assets":[]`), and the GCS bucket behind `releases.warp.dev` denies
  anonymous listing (AccessDenied on the listObjects probe). An REA objdump
  pass went to our own `target/release/dropterm` instead (ELF64 x86-64,
  `.rodata` 4.51 MiB carrying the embedded `completions.toml.gz`; evidence
  `ev_18b55177…`/`ev_1f30234…`) — the deep pwntools provider is unavailable
  on this host (`REA_PWNTOOLS_PYTHON` unset; REA never installs packages).

### 2. The engine: parse → locate → expand → dispatch → coalesce → filter-at-query

- `suggestions()` slices the line at the cursor, parses it into a typed HIR
  (`parsers/simple`), classifies the command, and flattens the HIR into
  `CompletionLocation`s — Command / Flag / Argument / Variable, each spanned
  (`crates/warp_completer/src/completer/engine/mod.rs:26-51`).
- Aliases expand next and the line re-parses, capped at 5 indirections; the
  replacement span is taken from the *original* parse so an accept replaces
  exactly what was typed (`crates/warp_completer/src/completer/suggest/alias.rs:14-15`,
  `suggest/mod.rs:695-705`).
- Coalescing: `Priority` is a clamped i32 [-100, 100]
  (`suggest/priority/mod.rs:9-13`); results bucket high / default / low,
  default-priority rows keep engine order grouped by suggestion type, and
  the concatenation dedupes by display (`completer/coalesce.rs:17-67`).
- Match policy: case-sensitive prefix → case-insensitive prefix → fuzzy
  fallback via `SkimMatcherV2` (the crates.io `fuzzy_matcher` crate —
  `crates/fuzzy_match/src/lib.rs:81-89` — not proprietary), engaged only
  when the query carries ≥1 alphanumeric ("fuzzy matches on punctuation
  produce poor results", `completer/matchers.rs:72-81`). Query-time order:
  exact(cs) → exact(ci) → prefixes → fuzzy by descending score
  (`suggest/mod.rs:308-441`). Hidden rows surface only on exact match
  (`suggest/mod.rs:362-365`).
- DropTerm's `filter_rank`/`match_tier` is already a hand-rolled port of
  this policy (its own doc cites `matchers.rs`; pinned in
  `dropterm/src/core/terminal/suggest.rs:1888-1944`).

### 3. When the menu shows: the two triggers and their gates

- `CompletionsTrigger` splits `Keybinding` (manual Tab) from `AsYouType`
  (`terminal/input.rs:2406-2411`). Both funnel into one async
  `run_completions_async` that aborts the previous request when as-you-type
  is on (`terminal/input.rs:12769-12773`).
- As-you-type gates, checked per user edit: the
  `completions_open_while_typing` setting, a single cursor at buffer end
  (`single_cursor_at_buffer_end`, `terminal/input.rs:12579-12585`), buffer
  length ≥ 2 (`MIN_BUFFER_LEN_TO_SHOW_COMPLETIONS_WHILE_TYPING = 2`,
  `terminal/input.rs:525`), a live command-grid prompt, and
  `EditOrigin::UserTyped` (`terminal/input.rs:11280-11284`). The setting's
  default value lives in Warp's settings crate (not opened; flagged
  Unverified) — DropTerm's default is its own ruling regardless.
- As-you-type **recomputes** rather than filters — "typing could involve
  moving to a new parameter within a given command, rather than being a
  strict subset as is the case with manual tab completions"
  (`terminal/input.rs:11173-11177`).
- Stale results never apply: the callback no-ops unless buffer text and
  selections are byte-identical to the snapshot taken at trigger time
  (`terminal/input.rs:13210-13224`). A 750 ms staleness timer closes a menu
  that has not refreshed (so the user never sees rows that don't match the
  line), deliberately delayed to avoid close/reopen jitter
  (`terminal/input.rs:11180-11205`).
- The file-path fallback is a manual-trigger privilege: `FilePaths` for
  `Keybinding`/`SlashCommandAutoOpen`, `None` otherwise
  (`terminal/input.rs:12757-12767`).
- Empty or absent results close the menu (`terminal/input.rs:13226-13238`);
  a qualifying set opens with the **first row preselected** (classic
  completions off: `TabCompletionsPreselectOption::First`,
  `terminal/input.rs:13333-13336`). The classic-completions interleave is a
  legacy feature-flagged mode (`terminal/input.rs:12664-12668`); parity
  targets the modern default path.

### 4. Keys while the menu is open: wrap, LCP-Tab, two-stage Enter

- The completions menu cycles: `set_prepared_tab_completions` sets
  `cycle = true` (`input_suggestions.rs:348-366`), so Up/Down/Shift+Tab wrap
  both ends (`(selected + 1) % len` / the mirrored prev,
  `input_suggestions.rs:577-601`; Shift+Tab routes to select-prev at
  `terminal/input.rs:13506-13512`; Up/Down while any menu is visible cycle
  at `terminal/input.rs:9763-9769`).
- **Tab** (`input_tab`, `terminal/input.rs:13662-13758`): compute the LCP
  of the *filtered, case-sensitive* prefix/exact rows; if it extends the
  typed word (`common_prefix.len() > current_word.len()` and starts with
  it), insert it — closing the menu when that was the single remaining row
  — otherwise fall through to select-next. The case-sensitivity note is
  explicit: rows can match case-insensitively while the LCP is
  case-sensitive, and inserting a *shorter* prefix would shorten the input.
- **Enter** is two-stage: `should_enter_accept_completion_suggestion`
  accepts whenever an item is selected, **except** when the buffer already
  equals the selection — then Enter falls through and executes
  (`terminal/input.rs:13829-13870`), with a directory-slash tolerance (a
  selected `dir/` matches a buffer `dir`). Accept emits
  `ConfirmSuggestion` (insert, close); the execute path is
  `ConfirmAndExecuteSuggestion` (insert, close, run)
  (`input_suggestions.rs:634-666`, `terminal/input.rs:9081-9110`).
- **Escape** closes with the buffer untouched (`editor_escape` →
  `exit(true)`, `terminal/input.rs:9851-9906`); **Ctrl+C** closes
  (`terminal/input.rs:11584-11586`); printable keys keep the menu open and
  recompute (the as-you-type edit path, finding 3).

### 5. Tab while closed, and the manual trigger's resolution

- With no menu open, Tab opens completions when bound to it and the cursor
  is positioned (`terminal/input.rs:13767-13768`). Warp's input is its own
  editor, so Tab is never a shell byte there — the direct PTY equivalent
  does not exist, and DropTerm must rule its own Tab-while-closed
  (the spec's OQ-1).
- The manual trigger resolves through `explicit_tab_completion`
  (`suggest/mod.rs:457-510`): a single prefix suggestion inserts directly;
  a case-sensitive common prefix longer than typed inserts **and opens**;
  otherwise it opens. The LCP-open condition is
  `common_prefix.len() > replacement_span.distance() && common_prefix.starts_with(query)`.

### 6. The ghost is a separate lane and already at parity in shape

- Ghost generation rides its own edit hook
  (`maybe_generate_autosuggestion`, `terminal/input.rs:10165-10176`):
  history-context-first (sqlite similar-contexts), background completer
  fallback, previous request aborted per keystroke. Warp suppresses the
  ghost while a *manually* opened menu shows, but not under as-you-type
  (`terminal/input.rs:13289-13300`). DropTerm's five-provider ghost with
  Right/Alt+Right accepts, the idle delay, and the guard chain already
  mirrors this lane; nothing here proposes touching it.

### 7. The data: a MIT corpus, 498 specs, embedded at build time

- The spec corpus lives in its own MIT repo —
  `github.com/warpdotdev/command-signatures` (LICENSE.md: "MIT License …
  Denver Technologies, Inc. (Warp) … Hercules Labs Inc. (Fig)") — pinned by
  Warp's workspace at rev `48fd7aa` (`Cargo.toml:401`), 498 Fig-format JSON
  specs (`command-signatures/json/`), generators progressively native Rust
  (`src/generators/`). The `embed-signatures` feature embeds them; lookup
  is lazy per command with a bounded miss cache
  (`crates/warp_completer/src/signatures/legacy/registry.rs:22-87`,
  `legacy/mod.rs:33-50`).
- DropTerm already owns the same pipeline: `scripts/extract-completions.mjs`
  pins `@withfig/autocomplete@2.692.3` and emits the core tier (92 commands,
  537 KiB gz, `include_bytes!`ed) plus the extended install-time tier.
  Closing the breadth gap is data work through the existing extractor —
  Warp's MIT fork is a legal source where its specs are richer — recorded
  as the follow-up (the spec's OQ-4), not this module.

## What DropTerm has today

Verified against `dropterm/src/core/terminal/suggest.rs`,
`dropterm/src/terminal/suggest.rs`, and
`dropterm/src/core/terminal/engine.rs` at HEAD `bd39c92` (2026-11-23):

| Behavior | Warp (f8223ce) | DropTerm (bd39c92) |
|---|---|---|
| Auto-popup | default path, recompute per keystroke, 1 row opens | opt-in (`autosuggestions_menu_auto`, default off), requires ≥2 rows |
| Typed-length floor | ≥2 chars | none |
| Cursor gate | single cursor at buffer end | cursor-at-line-end ghost gate only; the menu's open path does not check |
| Stale results | snapshot discard + 750 ms close | impossible by construction (synchronous collection under the term lock each update) |
| Path fallback | manual trigger only | providers are gated independently; no trigger distinction |
| Navigation | wraps both ends + Shift+Tab | clamps; Shift+Tab closes and forwards |
| Tab (open) | LCP fill → single-row close → cycle | accepts selected row |
| Enter | accept-and-close; equal line executes | accepts suffix, never executes |
| Typing while open | menu survives, rows recomputed | closes (CloseThenProcess), reopens via auto-popup |
| Opener | Tab (own editor) | Ctrl+Space chord |
| Menu rows | completer rows only | seat-aware: history rides unless a scoped seat leads (2026-11-06 ruling) |
| Ghost | history-context-first + completer fallback | five providers, history first — parity in shape |

## Recommendation

Adopt Warp's observable menu behavior as the `suggest-parity` module
(SPEC-suggest-parity.md, awaiting approval): default-on as-you-type
recompute (single row opens, 2-char floor, line-end gate, accept-echo
suppression), wrap navigation + Shift+Tab, Tab's LCP-fill-else-cycle,
two-stage Enter, typing keeps the menu, Tab-while-closed conditioned open.
The structured rulings ask was dismissed twice (2026-11-23); the
recommended defaults stand as the spec's assumptions, overrule at review.

**Rejected alternatives:**

- **Port the 750 ms staleness timer** — it exists because Warp's completions
  are async; ours collect synchronously under the term lock each update, so
  a stale menu cannot exist. The observable invariant (rows always match the
  line) holds by construction; a timer would be dead code.
- **Unconditional Tab bind (exact Warp)** — Warp's input is its own editor;
  ours feeds a real PTY where Tab is the shell's completion byte. Claiming
  Tab unconditionally makes the shells' own completion unreachable at
  prompts. Conditioned open keeps the muscle memory where it is safe.
- **Exact-Warp row composition (drop history)** — reverses the 2026-11-06
  seat-aware menu ruling on a dismissed ask; kept as one decision the owner
  can make at review.
- **Async completer rewrite for parity's sake** — our synchronous
  under-lock collection is cheaper, already correct, and removes the entire
  staleness/abort class; nothing observable asks for background tasks.
- **Copying engine code** — AGPL; behavior only. (The corpus and
  `fuzzy_matcher`-crate scorer are clean: MIT repo and MIT/Apache crate
  respectively; both recorded as OQ-4 follow-ups, not this module.)

## Sources

- `github.com/warpdotdev/warp` @ `f8223ce` (committed 2026-11-20), sparse
  clone fetched 2026-11-23: `app/src/terminal/input.rs`,
  `app/src/input_suggestions.rs`, `app/src/completer/mod.rs`,
  `crates/warp_completer/src/**`, `crates/fuzzy_match/src/lib.rs`,
  `Cargo.toml`.
- `github.com/warpdotdev/command-signatures` @ `48fd7aa` (the exact rev
  pinned in warp's workspace `Cargo.toml:401`), fetched 2026-11-23.
- DropTerm HEAD `bd39c92` (2026-11-23): the suggest model, routing, and
  engine surfaces cited above.
- GitHub releases API and `releases.warp.dev` GCS probes, 2026-11-23 (the
  no-public-binary finding); REA objdump evidence `ev_18b55177…` /
  `ev_1f30234…` on `target/release/dropterm`.

**Unverified:** Warp's shipped default for
`completions_open_while_typing` (the settings crate was not opened; the
default-on claim rests on the docs' presentation of completions-while-typing
as the standard behavior). DropTerm's own default is an independent ruling
and does not depend on it.
