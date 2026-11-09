# Research: what the Fig-spec walker actually does — an REA analysis of microsoft/inshellisense's runtime and withfig/autocomplete's data, as the model for DropTerm's Rust port

Date: 2026-11-07
Question: *[completions-breadth](completions-breadth.md) chose the
withfig/autocomplete static corpus and named microsoft/inshellisense's
spec runtime as the engine to reverse-engineer. What does that
parser→walker→filter pipeline do, token by token; which spec fields does
it actually read (and which of the type's fields does it ignore); where
exactly do the dynamic parts (generators, templates, `loadSpec`) attach,
so a static extraction can drop them without losing the tree; and what
is the minimal Rust port that grows DropTerm's two flat seats into
nested subcommands, per-subcommand options, options-that-take-arguments,
persistent options, and `--flag=value`?*

## TL;DR

**The engine is ~750 lines across three files, and the part DropTerm
needs is a pure function over an immutable tree — no Node, no
execution, no shell.** REA parsed the pinned sources as inert text
(7 TypeScript runtime files, 0 parse failures; 5 representative specs,
0 parse failures) and retained both graphs; the walker itself was read
directly from the pinned checkout.

1. **The lexer** (`parser.ts`, 154 lines) keeps the last command after
   `||`/`&&`/`;`/`|`, then walks characters through four states —
   quoted, quote-continued, flag, word — emitting tokens of
   `{token, complete, isOption}`. A token is *complete* only when a
   space follows it; the trailing unterminated token is the partial the
   menu filters on. `=` terminates a flag token, so `--emit=asm` lexes
   as `--emit` (complete, option) + `asm` — the walker then treats `asm`
   as that option's argument. Escaped spaces and `"quoted"word` forms
   are unwrapped after lexing.
2. **The walker** (`runtime.ts`) is three mutually recursive functions
   over `Fig.Subcommand` — `runSubcommand` / `runOption` / `runArg` —
   each consuming one complete token and recursing, stopping at the
   first incomplete (or absent) token to ask for suggestions. The
   rules it encodes, each a few lines: an option that declares `args`
   consumes the following token(s); `isVariadic` args consume until an
   option or subcommand appears; `isOptional` args let a token be
   re-tried as an option or subcommand first; `isPersistent` options
   are carried from a parent into every child subcommand; an `isCommand`
   arg (sudo-style) restarts the walk on a nested command's own spec.
3. **The stop-point assembly** (`suggestion.ts`) lists the node's
   subcommands + options + argument suggestions, drops options already
   typed (`removeAcceptedSuggestions`) and options excluded by
   `exclusiveOn`, filters case-insensitively by prefix (or substring
   under `filterStrategy: "fuzzy"`), sorts by `priority` (options 45,
   everything else 50 unless the spec says), dedupes, and hides
   `hidden` entries unless typed exactly. Every row carries its
   `description` straight from the spec — the menu's dim column.

**The dynamic half attaches only to `args`.** Across mkdir, rustc,
node, git, gh, cargo, docker, kubectl, every `generators:`, `script:`,
and `template:` key sits on an `Arg` — "what value goes here" — never on
a subcommand or option *name*. Dropping them keeps the entire
subcommand/option tree with descriptions intact; rustc and mkdir carry
zero generators to begin with. `template: "filepaths"`/`"folders"` maps
onto DropTerm's existing Path provider; generators map onto the
contracted git generator or nothing.

**The engine ignores most of the type's exotic fields.** A key-read
census over the runtime finds it never reads `requiresSeparator`,
`requiresEquals`, `optionsMustPrecedeArguments`, `parserDirectives`,
`flagsArePosixNoncompliant`, `isRepeatable`, `dependsOn`, `deprecated`,
`isRequired`, or `isDangerous`. The only constraint field it honors is
`exclusiveOn`. A Rust port that ignores the same set is not a
simplification of the live engine — it *is* the live engine's model.

**Verdict for the port:** grow `SpecCommand` into a recursive tree with
typed options and args (five flags total: `is_persistent`,
`is_optional`, `is_variadic`, `hidden`, `exclusive_on`), port the lexer
(~100 lines of Rust) and the walker (~200 lines, three functions), and
reuse `MenuCandidate` for the stop-point rows. DropTerm's grid-scraped
typed line replaces inshellisense's PTY wrapper; everything downstream
of `parseCommand` is the same pure computation. This record **extends**
[completions-breadth](completions-breadth.md) (its REA target,
executed) and **extends** [completions-seed-audit](completions-seed-audit.md)
(the conversion now has an exact static subset to target).

## Method

- Targets as pinned by the breadth record, verified with `git rev-parse
  HEAD` in the fetched checkouts: microsoft/inshellisense @
  `9ba441d39468a73d4e6aebe8bff9af37fad92e86` at
  `/tmp/dropterm-research/inshellisense`; withfig/autocomplete @
  `aef52acff84c45edde61ae610cc2c964802b9a38` at
  `/tmp/dropterm-research/withfig`; `@withfig/autocomplete-types@1.31.0`
  `index.d.ts` at `/tmp/dropterm-research/fig-types/package/`. Nothing
  from any checkout was executed, installed, or built.
- `analyze_javascript_application` (REA, `detail: "summary"`,
  `source_paths` scoped) on each tree: it parses TypeScript as inert
  text, retains the artifact inventory, module exports with source
  ranges, and the semantic graph. Engine run: parent Evidence
  `ev_72236afbf2b2b96c3078b4f81a858697d173f17e66bccf8c8218a4b138eb84b4`
  (summary `ev_57174e23…`), root artifact SHA-256 `fbca7e1a…`, 7 files
  parsed, 7,706 AST nodes, 129 findings, 0 parse failures. Data run:
  parent `ev_44475db3854058cc6c21a9dfc528dc2e606d80e2ce0d600fbcb87f1f171bd681`
  (summary `ev_a8a089c3…`), root `44b4aa74…`, 5 specs parsed (26,705 AST
  nodes, 572,240 bytes read), 0 parse failures.
- `inspect_analysis_view` (`javascript-facts`) on the engine graph for
  the exported callables and their exact ranges; `trace_javascript_semantics`
  seeded on the `isPersistent` property to locate the spec-field reads.
- A key-read census (`grep -oE '\.(field)\b'` over `runtime.ts`,
  `suggestion.ts`, `generator.ts`, `template.ts`) for which spec fields
  the engine consumes; per-spec counts of `generators:`/`script:`/
  `loadSpec:`/`template:` keys in eight representative specs.
- The walker's control flow read directly from the pinned source,
  cited by line.
- Coverage is static and partial by construction: REA reports the
  semantic graph as partial (dynamic property reads on spec objects
  resolve to no retained container — the specs are external data), and
  the scoped source selection leaves the other 67 inshellisense files
  and 1,487 withfig files inventoried but unanalyzed. Nothing here
  claims runtime behavior; the walker's semantics are read from its
  text.

## Findings

### 1. REA's identity and export inventory of the engine

The engine's three files and their exported surface, as REA recorded
them (artifact SHA-256 per file; ranges exact):

| File | SHA-256 (prefix) | Bytes | Exports observed (range) |
|---|---|---|---|
| `src/runtime/parser.ts` | `712ef3c8…` | 5,758 | `parseCommand` L23-27, `getCommandCacheKey` L29 |
| `src/runtime/runtime.ts` | `eb3d5cef…` | — | `loadLocalSpecsSet` L88-104, `getSuggestions` L106-142, `clearTransientSuggestionState` L148 (plus `getSpecNames` L144-146 by source) |
| `src/runtime/suggestion.ts` | `dc0b0dac…` | 17,194 | `NerdFontIcons` L27-61, `getSubcommandDrivenRecommendation` L358-409, `getArgDrivenRecommendation` L411-457 |

`runtime.ts:4-8` statically imports `@withfig/autocomplete/build/index.js`
— the compiled spec corpus as an npm package — which REA retained as an
*unresolved external* (no `node_modules` in the checkout). That is the
whole data dependency: the engine ships no knowledge of its own.

The `isPersistent` trace (Evidence `ev_8e1f14a5…`) found exactly two
reads, both in `runtime.ts` — L170:133 (inside `getPersistentOptions`)
and L257:30 (inside `getPersistentTokens`) — each on a receiver REA
could not resolve to a retained container ("object identity is
complete; receiver unresolved"). That is the expected shape of a
data-driven walker: every spec field is a property read on an object
that arrives from outside the engine.

### 2. The lexer — `parser.ts`, read in full

- **Token shape** (`parser.ts:8-18`): `token`, `tokenLength` (display
  width), `complete`, `isOption`, and optional `isPersistent`,
  `isPath`, `isPathComplete`, `isQuoted`, `isQuoteContinued`.
- **Command split** (`:20`, `:24`): `/(\|\|)|(&&)|(;)|(\|)/` — the text
  after the last delimiter is the command being completed; everything
  before it is discarded. (DropTerm's seat rules already treat the
  line as one command; this is the generalization.)
- **Four states** (`:62`): `readingQuotedString`,
  `readingQuoteContinuedString`, `readingFlag`, `readingCmd`. A quote
  char opens the first (`:71-73`); `-` opens a flag (`:74-76`); any
  other non-space opens a word (`:77-80`).
- **`complete` means "a space followed it"** — for a closing quote
  (`:87`), a flag ended by whitespace (`:105`), a word ended by
  unescaped whitespace (`:113`). The loop's tail (`:124-151`) pushes
  whatever is still being read as `complete: false`: that single
  trailing token is the partial.
- **`=` closes a flag** (`:105`): `(readingFlag && space) || char === "="`
  pushes the flag as a complete option token; the characters after `=`
  start a fresh word token. So `--emit=asm` → `[--emit (option,
  complete), asm]`, and `--emit=` → `[--emit (option, complete)]` with
  the walker then at the option's argument.
- **Post-passes** (`:31-57`): `unescapeSpaceTokens` turns `my\ file`
  into `my file` for unquoted tokens; `unwrapQuotedTokens` handles
  `"hello"world`.
- **Shell-awareness is one function**: `getShellWhitespaceEscapeChar(shell)`
  (`:35`, `:47`, `:61`) — backslash on POSIX shells, backtick on
  PowerShell. DropTerm has one escape char.

### 3. The walker — `runtime.ts:106-465`

**Entry** (`getSuggestions`, `:106-142`): `parseCommand` (`:109`); an
incomplete *root* token short-circuits to `runCommand` (`:114-116`) —
prefix-match over the sorted spec names (binary search, `:150-164`)
plus alias names (`:437-465`; aliases priority 100, specs 40). Otherwise
`aliasExpand` (`:118`), `loadSpec` for the root command (`:121`), and
`runSubcommand(tokens[1..], …, spec)` (`:133`). An empty result with no
argument description is `undefined` — the "menu refuses to open" rule
(`:135`).

**`runSubcommand`** (`:378-435`) — at a subcommand node with the
remaining tokens:

- No tokens, or an incomplete first token → *stop point*:
  `getSubcommandDrivenRecommendation` (`:391-395`).
- First token is an option → look it up in persistent + node options
  (`:399-402`); found → `runOption`; unknown option → `undefined`
  (`:406`): an unrecognized flag silences the walk, never guesses.
- Else try it as a subcommand name (`genSubcommand`, `:409`); found →
  recurse into the child **carrying the persistent options**
  (`getPersistentOptions(persistentOptions, subcommand.options)`,
  `:417`) and only the persistent accepted tokens (`:418`).
- Else, if the node declares args → `runArg` (`:429-432`); if it
  declares none, skip the token and stay on the node (`:434`).

**`runOption`** (`:264-299`): if the option declares `args`, the next
tokens are consumed by `runArg` with `fromOption = true` (`:280-283`);
otherwise return to `runSubcommand` with the option token appended to
the accepted set, flagged `isPersistent` when it came from the
persistent list (`:284-298`).

**`runArg`** (`:301-376`):

- Args exhausted → back to `runSubcommand` with `argsDepleted = true`
  and `argsUsed = !fromOption` (`:315-316`).
- No tokens / incomplete token → *stop point*:
  `getArgDrivenRecommendation` (`:317-321`).
- If every remaining arg `isOptional`, re-try the token as an option
  (`:325-331`) or a subcommand (`:333-348`) before treating it as an
  argument value.
- `isVariadic` → consume and stay on the same arg list (`:351-352`).
- `isCommand` → `loadSpec` for the token and restart the walk on that
  command's spec (`:353-362`) — the `sudo`/`time`/`xargs` shape.
- Otherwise consume one token and advance to the next arg (`:363-375`).

**`genSubcommand`** (`:188-250`) resolves a child by name (string or
array `name`, `:191`) and, when the child carries `loadSpec`, lazily
loads and splices it. Three `loadSpec` shapes (function / string /
object, `:200-245`) — all dynamic, all dropped by a static extraction;
the breadth record counted them under its 61.5% static measure.

**Persistence** is two helpers: `getPersistentOptions` (`:166-173`)
carries any option with `isPersistent === true` into the child's option
set (deduplicated by name); `getPersistentTokens` (`:256-258`) keeps
only the accepted tokens flagged persistent across the boundary, so a
parent's `--verbose` still counts as "already typed" inside `git
commit`.

### 4. The stop-point assembly — `suggestion.ts`

`getSubcommandDrivenRecommendation` (`:358-409`), the function that
produces what the menu shows at a subcommand node:

1. If args were depleted *and* came from the subcommand, show nothing
   (`:371-373`) — the positional slots are full.
2. The partial is the incomplete token's text; a path partial is
   reduced to its basename (`:374-377`).
3. Unless the walk arrived through a subcommand's own args: push the
   node's `subcommands` and its `options` (persistent + own)
   (`:384-387`). Options are filtered by `exclusiveOn` against the
   options already accepted (`optionSuggestions`, `:298-307`).
4. If the node has args, push the first arg's `generators`,
   `suggestions` (static strings or objects), and `template` results
   (`:388-394`) — the dynamic tier, in that order.
5. Pipeline (`:396-408`, inside-out): sort → fix up path insert values
   → remove already-accepted names → remove `~` folder when `~` was
   typed → remove `hidden` unless typed exactly → remove empty names →
   dedupe by name.

`filter` (`:160-237`): default strategy is a case-insensitive
`startsWith` over each name (array names match if any element does,
and the matched element becomes the displayed name, `:207-220`);
`"fuzzy"` is a case-insensitive `includes` (`:170-202`). No partial →
everything (`:166`).

`sortSuggestions` (`:143-158`): descending `priority` only; path
suggestions additionally re-rank among themselves by match quality
(`getMatchRelevance`, `:130-138`: exact 3 / prefix 2 / substring 1),
length, then name — so subcommands and options keep their
**spec-authored order**. `getSuggestionPriority` (`:119-127`): anything
whose name starts with `-` is 45, otherwise the spec's `priority` or
50.

`toSuggestion` (`:105-117`) is the row: `name`, `description`, `icon`,
`allNames`, `priority`, `insertValue`, `type`, `hidden`. DropTerm's
`MenuCandidate { label, suffix, description, source }` already holds
the three that matter; `insertValue` (what to write when it differs
from the display name, e.g. a folder with a trailing `/`) is the
`suffix` DropTerm already computes.

### 5. The field census — what the engine reads, and what it ignores

Property reads across `runtime.ts`, `suggestion.ts`, `generator.ts`,
`template.ts` (count of `.field` occurrences):

| Read | Count | Where it matters |
|---|---|---|
| `name` | 54 | identity and matching, string or array |
| `subcommands` | 16 | the tree |
| `args` | 15 | option-args and positionals |
| `priority` | 10 | sort |
| `filterStrategy` | 10 | prefix vs fuzzy |
| `description` | 6 | the row's dim column |
| `insertValue`, `hidden` | 6 each | row |
| `options` | 5 | the tree |
| `icon` | 5 | row (inshellisense's Nerd Font glyphs; DropTerm has no icon column) |
| `loadSpec` | 4 | dynamic — dropped |
| `isPersistent` | 2 | walker |
| `isVariadic`, `isOptional` | 2 each | walker |
| `template`, `suggestions`, `generators`, `debounce` | 2 each | arg tier (dynamic except `suggestions`) |
| `isCommand` | 1 | walker (sudo shape) |
| `exclusiveOn` | 1 | option filter |
| `getQueryTerm`, `postProcess` | generator-only | dynamic — dropped |

**Zero reads** of: `requiresSeparator`, `requiresEquals`,
`optionsMustPrecedeArguments`, `parserDirectives`,
`flagsArePosixNoncompliant`, `isRepeatable`, `dependsOn`, `deprecated`,
`isRequired`, `isDangerous`, `isModule`, `isScript`. The type
(`index.d.ts`: `BaseSuggestion` L353, `Subcommand` L524, `Option` L708,
`Arg` L870, `Generator` L1116) declares them; the live engine does not
consult them. The constraint vocabulary a port must honor is therefore
exactly: `isPersistent`, `isOptional`, `isVariadic`, `isCommand`,
`hidden`, `exclusiveOn`, plus `priority` and `filterStrategy` for
ordering.

### 6. Where the dynamic half lives in the data

Key counts in eight specs at the pinned commit:

| Spec | Lines | `generators:` | `script:` | `loadSpec:` | `template:` |
|---|---|---|---|---|---|
| `mkdir.ts` | 36 | 0 | 0 | 0 | 1 |
| `rustc.ts` | 358 | 0 | 0 | 0 | 3 |
| `node.ts` | 503 | 1 | 0 | 0 | 4 |
| `gh.ts` | 3,228 | 26 | 4 | 0 | 11 |
| `kubectl.ts` | 3,866 | 13 | 12 | 0 | 16 |
| `docker.ts` | 6,853 | 44 | 20 | 1 | 11 |
| `cargo.ts` | 7,562 | 190 | 7 | 0 | 13 |
| `git.ts` | 9,812 | 82 | 14 | 1 | 48 |

Every one of those keys is a property of an `Arg` (or of a `Generator`
inside an `Arg`) — the type puts `generators`, `template`, and
`suggestions` on `Arg` (L870), and `loadSpec` on `Subcommand` as a
lazy *replacement* for the node's own static body, never as a
supplement to the names. So the static extraction rule is one
sentence: **keep every `Subcommand`/`Option` with its `name`,
`description`, and the honored flags; keep each `Arg`'s `name`,
`isOptional`, `isVariadic`, `template` kind, and literal `suggestions`;
drop `generators`, `script`, `postProcess`, `loadSpec`, `generateSpec`.**
`mkdir.ts` in full (`:1-36`) is the shape the extraction leaves behind
for every spec: one positional with `template: "folders"`, six options
with descriptions, two of them taking an argument.

What the drop costs, concretely: `git checkout <branch>` loses Fig's
branch generator (DropTerm's contracted git generator already supplies
it); `cargo run --bin <name>` loses the Cargo.toml-reading generator
(nothing replaces it — the option still completes, its value does not);
`kubectl get <resource>` loses the live-cluster generator (nothing
replaces it, by ruling). The names, flags, and descriptions — the
breadth — survive in every case.

### 7. What this adds to DropTerm's seats, exactly

Today (`src/terminal/suggest.rs:966-986`, per the breadth record): a
flat `SpecCommand { name, subcommands: Vec<SpecEntry>, flags:
Vec<SpecEntry> }`; the subcommand seat is the first argument only;
`--` flags from any seat; no option-arguments; no nesting; no
persistence; no `=`. The walker above adds, in order of user-visible
value:

1. **Per-subcommand options** — `git commit --am` lists commit's flags,
   not git's whole flag table (today's table has one flag list per
   command).
2. **Options that take an argument** — after `--emit` or `--emit=`,
   the next seat is the option's arg (its `suggestions` or template),
   not another flag.
3. **Nested subcommands** — `docker compose up`, `gh pr create`,
   `cargo clippy`, to any depth.
4. **Persistent options** — `git --no-pager log --one` still finds
   `--oneline` because `--no-pager` rode through.
5. **Variadic and optional positionals** — `git add a b c --` keeps
   offering paths until an option appears.
6. **The `sudo` shape** (`isCommand`) — `sudo apt ins` walks apt's
   spec.

Each is a branch in one of three ~60-line Rust functions. The
grid-scraped `typed` string is the lexer's input; nothing before
`parseCommand` in inshellisense (the PTY wrapper, `src/isterm/`) is
needed, because DropTerm already has its own answer to "what is on the
line."

## Recommendation

**Port the three functions, not the engine.** The Rust shape:

```rust
struct SpecCommand {            // today's name, grown
    name: Vec<String>,          // Fig allows aliases: ["co", "checkout"]
    description: Option<String>,
    subcommands: Vec<SpecCommand>,
    options: Vec<SpecOption>,
    args: Vec<SpecArg>,
    hidden: bool, priority: Option<u8>,
}
struct SpecOption {
    names: Vec<String>, description: Option<String>,
    args: Vec<SpecArg>, is_persistent: bool,
    exclusive_on: Vec<String>, hidden: bool,
}
struct SpecArg {
    name: String, is_optional: bool, is_variadic: bool,
    is_command: bool,
    template: Option<Template>,          // Filepaths | Folders
    suggestions: Vec<SpecEntry>,         // literal strings/objects only
}
```

plus `lex(typed) -> Vec<Token>` (the four-state loop, one escape char)
and `walk(tokens, &spec) -> StopPoint` (`run_subcommand` / `run_option`
/ `run_arg`), with the stop point rendering into the existing
`MenuCandidate` rows under `menu_source_rank`'s specs tier. The
`#[serde(deny_unknown_fields)]` boundary stays: a `script` or
`generators` key still fails the file whole (`suggest.rs:769-773`); the
converter simply never emits one. Open design choices for the spec, not
this record: whether the recursive tree stays in TOML (nested `[[…]]`
arrays) or moves the generated built-in table to RON; whether
`filterStrategy: "fuzzy"` is honored (the breadth record's pinned
`filter_rank` is the obvious seat) or flattened to prefix everywhere.

**Order of work:** (1) the schema and lexer with the no-shrink tests
pinned to the current seed; (2) the walker, pinned by table-driven
tests transcribed from this record's branch list (each bullet in
finding 7 is one test); (3) the extractor emitting the static subset at
the pinned commit for the top-100 tranche; (4) the embedded table.
Each step ships on its own and the existing five-plus-fourteen keep
working at every step.

## Unverified / open questions

- REA's semantic coverage is partial by construction (external spec
  objects); the walker's semantics here are read from source text, not
  traced. A table-driven Rust test suite is the verification, not a
  runtime probe of inshellisense.
- `getArgDrivenRecommendation` (`suggestion.ts:411-457`) was inventoried
  but not read line-by-line; it mirrors the subcommand-driven path for
  the option-arg stop point. Confirm during the port.
- Whether any top-100 spec relies on `loadSpec` for its *entire* body
  (so a static extraction would leave only the root name): the breadth
  record counted 275 non-static specs by key presence without
  distinguishing "has one generator on one arg" from "is a loadSpec
  shell". Measure per spec in the extractor.
- The lexer's `=` rule produces two tokens for `--flag=value`; whether
  DropTerm's accept path writes `value` or `=value` after `--emit=` is
  the port's choice (inshellisense's `replacement.ts`, not read here,
  handles the insert).

## Sources

All read 2026-11-07 in the checkouts the breadth record fetched on
2026-11-06; nothing executed.

- microsoft/inshellisense @ `9ba441d39468a73d4e6aebe8bff9af37fad92e86`
  (MIT): `src/runtime/parser.ts` (whole file, 154 lines);
  `src/runtime/runtime.ts:4-8, 88-465`; `src/runtime/suggestion.ts:97-409`;
  key census over `src/runtime/{runtime,suggestion,generator,template}.ts`
  — <https://github.com/microsoft/inshellisense>
- REA Evidence (this session): engine analysis parent
  `ev_72236afbf2b2b96c3078b4f81a858697d173f17e66bccf8c8218a4b138eb84b4`
  (summary `ev_57174e23dd1ad18ad79b8ee33ccf1f8f1e7f43aecfa2b08a5a83019afdb8ca9c`;
  root SHA-256 `fbca7e1af7539801f0792870660090ec676174562f651028f9eac3b5085204c2`);
  `javascript-facts` view `ev_1f6c13f50fce05947916b9b466c68c1f803cd12ce68eca2e2347703fa6395d73`;
  `isPersistent` trace `ev_8e1f14a5d5e9d1b1920295357146682047afa709ddb0e3948bb183e6ef19919c`;
  data analysis parent
  `ev_44475db3854058cc6c21a9dfc528dc2e606d80e2ce0d600fbcb87f1f171bd681`
  (summary `ev_a8a089c3b9a1736151606d46ea340ed2dc02b4e4480acd0c5e214ad76b9347a5`;
  root SHA-256 `44b4aa74d2b25644491a2b886fa24b4ee935ab0fb100e9a950e976f87696e8e0`)
- withfig/autocomplete @ `aef52acff84c45edde61ae610cc2c964802b9a38`
  (v2.692.3, MIT): `src/mkdir.ts:1-36`; line and key counts over
  `src/{mkdir,rustc,node,gh,kubectl,docker,cargo,git}.ts` —
  <https://github.com/withfig/autocomplete>
- `@withfig/autocomplete-types@1.31.0` `index.d.ts` (MIT):
  `BaseSuggestion` L353, `Subcommand` L524, `Option` L708, `Arg` L870,
  `Generator` L1116 — <https://www.npmjs.com/package/@withfig/autocomplete-types>
- DropTerm local tree: `src/terminal/suggest.rs` (SpecCommand/SpecEntry
  :774-805, the deny-unknown-fields boundary :769-773, `specs_context`
  :966-986, `specs_candidates` :1450-1478, `MenuCandidate` :1284-1290,
  `menu_source_rank` :1343-1351); prior records
  [completions-breadth](completions-breadth.md),
  [completions-seed-audit](completions-seed-audit.md),
  [suggest-menu-layering](suggest-menu-layering.md)
