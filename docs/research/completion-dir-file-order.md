# Research: should DropTerm's path menu group files above directories, directories above files, or keep one interleaved list?

Date: 2026-11-21

## TL;DR

**The dominant convention across every mainstream path-autocomplete surface
surveyed is no type grouping at all: one interleaved list ordered by name
(line editors and shell menus) or by match score (fuzzy surfaces), with
directories distinguished only by a trailing `/` — readline/bash, zsh, fish,
Warp, Amazon Q's suggester, Nushell, tsserver/VS Code, cmp-path, fzf, and
telescope.nvim all ship that shape; explicit directories-first ordering is a
real but minority pattern (blink.cmp's path source makes it an unconditional
default, Gemini CLI's pattern-filter path applies it, zsh exposes it only as
opt-in styles), and files-first appears in none of the surveyed tools — zero
precedent.** DropTerm's tree at HEAD `124a113b` already ships the dominant
shape: the path tier serves the listing's name-sorted interleaved order, and
nothing downstream regroups it — the "files on top, then directories sorted
per-directory" premise does not match the code at HEAD, and even if some
surface shipped it, every surveyed peer argues against it. Recommendation:
keep the interleaved name-sort exactly as pinned below; no code change.

## What DropTerm has today

Grounded in the working tree at HEAD `124a113bf15295ed315158cb29fb1091c417a7d4`
(2026-11-21):

- **The listing is name-sorted, interleaved**: `read_sorted_entries` builds
  `(name, is_dir)` tuples and plain-sorts them — `entries.sort_unstable()`
  (`dropterm/src/core/terminal/engine.rs:880-893`). The sort key is the name;
  the `is_dir` bit is secondary, so files and directories interleave
  alphabetically with the doc's own words: "One directory's sorted (name,
  is-a-directory) entries" (`engine.rs:873-879`).
- **The path tier preserves that order**: `path_candidates` filters the
  listing and maps it to rows in order — no sort, no regroup
  (`dropterm/src/core/terminal/suggest.rs:1593-1634`); directories carry the
  `/` suffix (`:1618-1624`).
- **The menu concatenates providers by seat and never re-ranks rows**:
  `candidates` orders the five sources by `menu_source_rank` and flattens
  (`suggest.rs:1506-1526`); the menu stores the collection as given — "the
  merged collection's own source order … is the display order … no re-ranking
  at open" (`dropterm/src/core/terminal/suggest/tests.rs:2709-2713`), and the
  renderer reads `menu.rows()` verbatim (`engine.rs:2490-2492`).
- **Nothing in the crate partitions files from directories**: greps for
  `partition(`/`is_dir` ordering keys across `dropterm/src` return no
  grouping sort; the only dir/file asymmetries are the cd-adjacent filters
  from [bare-word-path-completion](bare-word-path-completion.md).
- The user doc describes the menu order as "the keyword's scope first" —
  seat scoping, not file-type grouping (`docs/autosuggestions.md:109`).

The research question arrived describing DropTerm as "files on top, then
directories sorted per-directory". That is not what HEAD's code does (the
pins above); see [Unverified](#unverified).

## Findings

All source pins fetched 2026-11-21.

### 1. bash/readline — one lexical list, no grouping

- Readline sorts completion matches by default, lexically, with no type
  awareness: `/* If non-zero, sort the completion matches.  On by default. */
  int rl_sort_completion_matches = 1;`, and the sorts ride
  `_rl_qsort_string_compare` — a string comparator over the match texts
  (`qsort (matches+1, i-1, sizeof (char *), (QSFUNC
  *)_rl_qsort_string_compare)`, `remove_duplicate_matches` and
  `compute_lcd_of_matches`) — the line editor's own source.
- No directory-vs-file grouping or priority exists anywhere in the ordering;
  directories get cosmetic treatment only — a trailing `/` when displayed or
  inserted (`_rl_complete_mark_directories`, `append_to_match`, and
  `stat_char` under `VISIBLE_STATS`) — never a sort key.
- Pinned via the `gnu-mirror-unofficial/readline` mirror of `complete.c`
  (the canonical savannah cgit was unreachable from here — see Unverified);
  the `rl_sort_completion_matches` text matches readline.h's own declaration
  ("If non-zero, readline will sort the completion matches. On by default.").

### 2. zsh — interleaved by name by default; directory-first is opt-in config

- `_path_files`' default listing order is the name-sort glob qualifier
  `(on)`: the `file-sort` style maps size/links/modification/access/inode to
  other qualifiers and `"*) sort=on;;"` is the fallback
  (`Completion/Unix/Type/_path_files`, master `6415932`); the manual states
  the default: "If the style is set to any other value, or is unset, files
  will be sorted alphabetically by name" (compsys doc, `file-sort`).
- Files and directories are gathered in one attempt: `_path_files` names the
  `directories` tag only under `-/` (the directories-only flag) and
  `_description files` otherwise; the default tag set per the manual is
  `globbed-files`, `directories`, `all-files`, where "The first two
  (`globbed-files` and `directories`) are normally offered together to make
  it easier to complete files in sub-directories" (compsys doc,
  `file-patterns`). Together means interleaved in one `compadd` pass.
- The tag-order machinery's default is declaration order: `_tags` reads
  `zstyle -a ":completion:${curcontext}:" tag-order order || …` and, when
  unset, tries the tags in the order the completion function declared —
  untried tags are appended in their original sequence (`[[ -z "$nodef" ]]
  && comptry "$@"`) (`Completion/Base/Core/_tags`).
- The grouping knobs are real but opt-in: `group-name` ("The completion
  system can group different types of matches, which appear in separate
  lists") plus `group-order` ("specify the order for display of the
  groups") — a `zstyle ':completion:*' tag-order directories files` idiom is
  a user configuration, not the shipped default. The popular belief that
  "zsh lists directories first" is this idiom, not zsh's default.

### 3. fish — natural name sort only; directory-ness filters, never orders

- The pager adds no ordering: `pager.rs` contains no comparator and no sort
  call — it renders the completions slice it is given ("PagerComp entries
  in the list order as given") (`src/pager.rs`, master `f0d874ad`).
- The engine's terminal ordering step is `sort_and_prioritize` →
  `natural_compare_completions` → `wcsfilecmp(&a.completion, &b.completion)`
  — a natural sort over the completion *text* (`src/complete.rs`). The
  comparator has no dir/file term; `PathFilter::Directory` and
  `ForCdArgument` appear only as expansion *filters* (the cd special-case,
  issue #1059), never in any `Ordering`/sort logic.
- fish's only type-adjacent ordering tweaks are content heuristics, not
  dir/file: a tilde-suffix penalty ("frequently autosave files from e.g.
  emacs") and a duplicate-argument penalty — both documented in the same
  comparator block.

### 4. Warp — display-name sort, no grouping

- `sorted_paths_relative_to` maps the directory listing through
  `.sorted_by(|a, b| a.suggestion.cmp_by_display(&b.suggestion))` —
  comparison by display string only; no comment, no dir/file priority
  (`crates/warp_completer/src/completer/engine/path.rs`, master `f8223cef`).
  The directories-only variant for the `folders` template *filters*
  (`file_type == EngineFileType::Directory`) rather than demoting files —
  the same filter-not-order pattern as fish's cd.

### 5. Amazon Q CLI (Fig) — spec priority plus recency/alphabetical; no dir/file ordering found

- The suggester's sorter works on numeric priorities: default `priority =
  suggestion.priority || 50`, boosted by a recency index
  (`recencyBoost = recency / 10000000000000`), under `// Default setting for
  autocomplete.sortMethod is "recency"`; the only path-specific carve is
  excluding `"../"` from recency boosts (`packages/autocomplete/src/
  suggestions/sorting.ts`, main `b654a1be`). No directory-before-files or
  files-before-directories logic exists in the sorter.
- Path arguments are declared data (Fig templates `filepaths`/`folders` — a
  `folders` argument shows only directories, a filter), but the template's
  own executor could not be located in the open TS engine (it is not in
  `templateSuggestionsGenerator.ts`, which implements only `history` and
  `help`); see Unverified.

### 6. Gemini CLI — the one dirs-first engine path among the shells, with a score-only fuzzy twin

- In the pattern (picomatch/glob) path of `fileSearch.ts`, results sort
  **directories first**, then lexically: `const aIsDir = a.endsWith('/')` …
  `if (aIsDir && !bIsDir) return -1; if (!aIsDir && bIsDir) return 1;`
  followed by a string compare — the comment notes localeCompare was avoided
  for speed, and directories carry a trailing `/` throughout
  (`packages/core/src/utils/filesearch/fileSearch.ts`, main `eecccfcd`).
- The fuzzy path has **no** dir/file priority: ordering comes from fzf
  tiebreakers only — `byBasenamePrefix` (a match on the filename itself
  beats a match on the path), `byMatchPosFromEnd` ("Prefers matches closer
  to the end of the path"), `byLengthAsc` ("Prefers shorter paths").
- Genus note: Gemini's `@`-completion is a workspace-wide fuzzy *file
  search* (`useAtCompletion.ts` merges agent, file, and resource
  suggestions; the search crawls the whole project root), not a
  per-directory completion menu — its dirs-first default therefore governs
  a different surface than DropTerm's path tier, but it is still the
  clearest shipped dirs-first default found in any CLI.

### 7. Nushell — interleaved by explicit, maintainer-merged ruling

- The slash-skew bug report (#10020, Aug 2023) showed directory names
  sorting out of line with files because the trailing `/` joined the sort
  key; the fix merged a week later — PR #10102, "Compare names ignoring
  trailing slashes in folder names", closed as completed 2023-08-24 for the
  v0.85.0 milestone, so that "Users get the same order of suggestions in
  the tab completion for both file and directory". The project's intent is
  one consistent interleaved list, not grouping.
- Current main confirms the shape: `complete_paths` produces matcher order
  (prefix: case-insensitive alphabetical; fuzzy: `CompletionSort` default
  `Smart` — score descending, then alphabetical tiebreak; the
  `Alphabetical` variant sorts purely by haystack), then a single stable
  pass `hidden_files_last` moves dotfiles to the end. "Directories-before-
  files ordering — there is **none**"; `directories_only` merely filters
  for cd (`crates/nu-cli/src/completions/completion_common.rs`,
  `completion_options.rs`, `file_completions.rs`, main `f6e23c4e`;
  `crates/nu-protocol/src/config/completions.rs`).

### 8. Editors — the strongest dirs-first precedent is blink.cmp; VS Code's own paths do not group

- **TypeScript (tsserver), which powers VS Code's JS/TS path completions**:
  `convertPathCompletions` gives *every* path entry the identical
  `sortText: SortText.LocationPriority` — files, directories, and module
  names alike; the only precedence code (`kindPrecedence`) settles duplicate
  *names*, never ordering; no `isRecommended` for directories
  (`src/services/stringCompletions.ts`, tag v5.9.3). The VS Code extension
  forwards `sortText` through untouched
  (`extensions/typescript-language-features/src/languageFeatures/
  completions.ts`, main) — no dir-vs-file ranking exists there either.
- **Path Intellisense** (the third-party extension most users mean by "VS
  Code path autocomplete"): the provider maps the directory listing onto
  completion items in readdir order — `childrenOfPath.map((child) =>
  createPathCompletionItem(child, config, context))` with no sort and no
  `isDirectory` ordering branch (`src/providers/javascript/
  javascript.provider.ts`, master `4402a794`).
- **blink.cmp's path source** is the explicit counter-example, and the
  record's key editor precedent: every entry's rank key encodes the type —
  `sortText = (is_dir and '1' or '2') .. entry.name:lower(), -- Sort
  directories before files` — unconditional, with no option to disable it
  (`lua/blink/cmp/sources/path/lib.lua`, main `3d3631f6`). This is exactly
  the "sortText tier by type" pattern, shipped as a default by a modern
  mainstream completion engine.
- **cmp-path (nvim-cmp)**: no sorting at all — entries arrive in
  `fs_scandir_next` order (OS order), directories get a `/` insert text and
  optional `/` label; "no directory-before-files priority" anywhere
  (`lua/cmp_path/init.lua`, main `c6424870`).
- **Sublime Text**: no built-in path completion to order — its completions
  come from "Words from the current file", completion files, snippets, and
  plugins; nothing in the docs suggests files or directories while typing a
  path (official completions docs, fetched 2026-11-21). N/A by construction.

### 9. Fuzzy pickers — the deliberate score-only counterpoint

- **fzf**: results are ranked by match score, and the man page's only
  ordering knob is `--tiebreak` — "Comma-separated list of sort criteria to
  apply when the scores are tied", "Default is `length` (or equivalently
  `length`,index)". No option or statement groups directories before files;
  `--walker` pools `file` and `dir` entries into one list, and
  `--scheme=path` only adds scoring bonuses "to the characters after path
  separator" (`man/man1/fzf.1`, master `bf4869b7`).
- **telescope.nvim**: sorters are pure score — "Sorter sorts a list of
  results by return a single integer for a line … Lower number is better",
  `FILTERED = -1` below zero; "There is no code anywhere in this file that
  inspects file types, extensions, or directory components for grouping or
  ordering". The file sorter's only path-awareness is a filename-tail
  boost (`make_cached_tail` over `[^<sep>]*$`) — a match-shape heuristic,
  not a type grouping (`lua/telescope/sorters.lua`, master `1e1bab73`).

### 10. Claude Code CLI — not inspectable

Claude Code ships as a closed, minified bundle; no official documentation
states an ordering for its file-path suggestions. Unverified, stated as
such.

## The verdict table

| Tool | Default ordering | Explicit dir-vs-file priority | Files first? |
|---|---|---|---|
| bash / readline | lexical (sort on by default) | none — cosmetic `/` only | no |
| zsh | name-sorted, interleaved (`(on)`) | none by default; opt-in `tag-order`/`group-name`+`group-order`/`file-patterns` | no |
| fish | natural-sorted, interleaved (`wcsfilecmp`) | none — directory-ness filters (cd), never orders | no |
| Warp | display-name sorted | none | no |
| Amazon Q CLI (Fig) | spec priority + recency (or alphabetical) | none found | no |
| Gemini CLI | dirs-first (pattern path); score-only (fuzzy path) | **yes, dirs-first** in the filter comparator | no |
| Nushell | interleaved (PR #10102 made dirs sort *as files do*) | none | no |
| VS Code JS/TS (tsserver) | uniform sortText, editor fuzzy ranking | none | no |
| Path Intellisense ext. | readdir order, unsorted | none | no |
| blink.cmp path source | dirs-first, then lowercase name | **yes, dirs-first** — unconditional sortText tier | no |
| cmp-path | scandir (OS) order | none | no |
| fzf | score, tiebreak `length,index` | none | no |
| telescope.nvim | score | none (filename-tail heuristic only) | no |
| Sublime Text | N/A — no built-in path completion | — | — |
| Claude Code CLI | unknown (uninspectable) | — | — |

## Recommendation

Keep DropTerm's HEAD ordering exactly as pinned: the path tier serves
`read_sorted_entries`'s interleaved name-sort, the menu preserves provider
order, directories carry the `/` suffix. That shape *is* the surveyed
convention, and it needs no change to match peers.

- **Rejected: files-first grouping.** Zero occurrences across fifteen
  surveyed tools (line editors, shells, Warp, Q, Gemini, Nushell, three
  editors/completers, two fuzzy pickers). If the reported files-on-top
  behavior exists on any surface or branch, it has no peer precedent and no
  rationale in any surveyed source; drop it rather than shipping it.
- **Rejected: dirs-first grouping for now.** Real but minority: blink.cmp
  (unconditional default, editor genus), Gemini's pattern path (a
  workspace-search genus, and its own fuzzy path drops the priority), and
  opt-in zsh styles. No terminal or line editor ships it as the default, and
  it buys nothing DropTerm's `/` suffix does not already signal. If a
  navigation-first rationale ever emerges, blink.cmp's two-tier `sortText`
  is the precedent to copy — a one-line key on the existing `(name, is_dir)`
  tuple — but nothing in this survey argues for it today.
- **No new config key**: a grouping toggle would gate a behavior no peer
  ships by default; the standing `autosuggestions_paths_enabled` gate already
  bounds the tier.

## Relation to prior records

- **Extends** [bare-word-path-completion](bare-word-path-completion.md)
  (2026-11-06): that record pinned *where* path rows sit (the provider
  ranks and the fallback carve); this record pins *how rows order within*
  the path tier, and confirms the same filter-not-order pattern it saw in
  fish's cd and Warp's folders template.
- **Consumes** [autosuggest-warp-parity](archive/autosuggest-warp-parity.md)
  and the menu spec rulings (2026-11-02/2026-11-06): the seat-scoped menu
  order (`menu_source_rank`) is untouched; this record surveys only the
  intra-tier order no prior record pinned.
- Supersedes nothing.

## Sources

All fetched 2026-11-21 unless noted.

- DropTerm local tree @ HEAD `124a113bf15295ed315158cb29fb1091c417a7d4`:
  `dropterm/src/core/terminal/engine.rs` (:873-893 the listing sort;
  :2490-2492 `menu_rows`), `dropterm/src/core/terminal/suggest.rs`
  (:1506-1526 `candidates`; :1593-1634 `path_candidates`),
  `dropterm/src/core/terminal/suggest/tests.rs` (:2709-2713 no re-ranking at
  open), `docs/autosuggestions.md:109`
- GNU readline: `complete.c` via the `gnu-mirror-unofficial/readline` mirror
  (canonical savannah cgit timed out from here): `rl_sort_completion_matches
  = 1` ("On by default"), `_rl_qsort_string_compare` sorts, no dir/file
  grouping, `_rl_complete_mark_directories`/`stat_char` cosmetic `/` —
  <https://github.com/gnu-mirror-unofficial/readline/blob/master/complete.c>
- zsh @ master `64159322d0e78f78d17d4c61a87d21256eb532e9` (2026-11-18):
  `Completion/Unix/Type/_path_files` (file-sort → `(on)` default;
  `_description files`/`directories`), `Completion/Base/Core/_tags`
  (tag-order default = declaration order) — <https://github.com/zsh-users/zsh>;
  zsh manual, Completion System (`file-sort` "or is unset, files will be
  sorted alphabetically by name"; `file-patterns` default tags
  globbed-files/directories/all-files "normally offered together";
  `group-name`/`group-order`) —
  <https://zsh.sourceforge.io/Doc/Release/Completion-System.html>
- fish @ master `f0d874ad924bd582a039f5a5d5a018913c7d8b6a` (2026-11-16):
  `src/complete.rs` (`sort_and_prioritize` → `natural_compare_completions`
  → `wcsfilecmp`; `PathFilter::Directory` filters, never orders; tilde-file
  penalty), `src/pager.rs` (no sorting; order preserved) —
  <https://github.com/fish-shell/fish-shell>
- Warp @ master `f8223cef2d167a5858bf233df8a1fb6f74537fa5` (2026-11-21;
  AGPL-3.0, survey only): `crates/warp_completer/src/completer/engine/path.rs`
  (`sorted_paths_relative_to` → `cmp_by_display`; directory-only variant
  filters) — <https://github.com/warpdotdev/warp>
- Amazon Q CLI autocomplete @ main `b654a1beb880951dd2343120bb867659622eecd9`
  (2025-11-15): `packages/autocomplete/src/suggestions/sorting.ts`
  (priority/recency sorter, `sortMethod` default "recency", `"../"` carve),
  `packages/autocomplete/src/generators/templateSuggestionsGenerator.ts`
  (history/help only; filepaths/folders elsewhere) —
  <https://github.com/aws/amazon-q-developer-cli-autocomplete>
- Gemini CLI @ main `eecccfcd5aff25de5e78135ca20f7abe3f566955` (2026-11-20):
  `packages/core/src/utils/filesearch/fileSearch.ts` (filter path: dirs
  first, then string compare; fuzzy path: fzf tiebreakers
  basename-prefix/match-pos-from-end/length),
  `packages/cli/src/ui/hooks/useAtCompletion.ts` (merge order agents → files
  → resources) — <https://github.com/google-gemini/gemini-cli>
- Nushell @ main `f6e23c4ec629c9c5f218113358ce7af04f85181e` (2026-11-20):
  `crates/nu-cli/src/completions/completion_common.rs` (matcher order +
  `hidden_files_last`; "Directories-before-files ordering — there is none"),
  `crates/nu-cli/src/completions/completion_options.rs` (`CompletionSort`
  Smart/Alphabetical semantics), `crates/nu-cli/src/completions/
  file_completions.rs`, `crates/nu-protocol/src/config/completions.rs`
  (defaults Prefix/Smart) — <https://github.com/nushell/nushell>; issue
  #10020 (2023-08-16) and PR #10102 (merged 2023-08-24, "Compare names
  ignoring trailing slashes in folder names") —
  <https://github.com/nushell/nushell/issues/10020>,
  <https://github.com/nushell/nushell/pull/10102>
- TypeScript @ tag `v5.9.3`: `src/services/stringCompletions.ts`
  (`convertPathCompletions` uniform `SortText.LocationPriority`;
  `kindPrecedence` dedup-only) — <https://github.com/microsoft/TypeScript>;
  VS Code @ main (fetched 2026-11-21):
  `extensions/typescript-language-features/src/languageFeatures/
  completions.ts` (sortText passthrough; only auto-imports de-prioritized) —
  <https://github.com/microsoft/vscode>
- blink.cmp @ main `3d3631f641e6e9b60066f269f435d23c524ceeb0` (2026-11-16):
  `lua/blink/cmp/sources/path/lib.lua` (`sortText = (is_dir and '1' or '2')
  .. entry.name:lower(), -- Sort directories before files`; unconditional) —
  <https://github.com/Saghen/blink.cmp>
- cmp-path @ main `c642487086dbd9a93160e1679a1327be111cbc25` (2025-07-30):
  `lua/cmp_path/init.lua` (scandir order preserved; no sort; `/` insert for
  dirs) — <https://github.com/hrsh7th/cmp-path>
- Path Intellisense @ master `4402a794c50639bb7ed54524b3e136b541880d4d`
  (2024-11-29): `src/providers/javascript/javascript.provider.ts` (listing
  mapped unsorted; no dir/file branch) —
  <https://github.com/ChristianKohler/PathIntellisense>
- fzf @ master `bf4869b7fa7748d973eeb414da4dde0d7fe4cd3c` (2026-11-19):
  `man/man1/fzf.1` (score ranking; `--tiebreak` default `length`;
  `--walker` files+dirs pooled; `--scheme=path` separator bonus) —
  <https://github.com/junegunn/fzf>
- telescope.nvim @ master `1e1bab730218eaf97fce743be3f56e51075928ac`
  (2026-11-08): `lua/telescope/sorters.lua` (integer-score sorters,
  `FILTERED = -1`, `get_fuzzy_file` tail boost; no file-type grouping) —
  <https://github.com/nvim-telescope/telescope.nvim>
- Sublime Text official completions docs (built-in sources: words,
  completion files, snippets, plugins; no path completion) —
  <https://www.sublimetext.com/docs/completions.html>

## Unverified

- **The premise that DropTerm "puts files on top, then directories sorted
  per-directory" does not match the tree at HEAD `124a113b`**: the listing
  is name-sorted interleaved (`engine.rs:880-893`), the path tier preserves
  it (`suggest.rs:1593-1634`), and no code anywhere in the crate regroups
  files above directories. Either the description came from an older build,
  a different branch, or a misremembering — the record pins HEAD's truth and
  leaves the reconciliation to the owner.
- **Claude Code CLI**: closed source; no primary statement of its
  file-suggestion ordering exists. Not surveyed, honestly absent.
- **Amazon Q's `filepaths`/`folders` template executor** was not located in
  the open engine (the TS suggester implements only `history`/`help`
  templates; the executor is presumably host-side in the Rust crates). The
  suggester's sorter — the part that would carry a dir/file priority — is
  pinned above; the template executor itself is not.
- **readline was pinned via an unofficial mirror** (`gnu-mirror-unofficial`);
  the canonical savannah cgit was unreachable from this environment
  (connection timeouts on repeated attempts). The mirror's
  `rl_sort_completion_matches` text matches readline.h's documented wording,
  which is the cross-check.
- **zsh's `tag-order` doc entry** could not be quoted verbatim (the doc page
  truncated in both fetch attempts before the entry); the default-order
  behavior is pinned from the `_tags` source instead, which is the owner of
  the behavior.
- **VS Code's markdown path-completions file** has moved or been renamed in
  current main (the `languageFeatures` directory lists no
  `pathCompletions.ts`); it was not re-located within budget. JS/TS and
  Path Intellisense cover the VS Code question.
- **Gemini's filter-path comparator** was read through a page-render of the
  file; the two decisive lines (`if (aIsDir && !bIsDir) return -1;` and its
  mirror) are consistent across both reads, but one adjacent line arrived
  garbled in the render — the dirs-first fact is solid, individual
  neighboring lines should be re-checked against a fresh checkout before
  being quoted as exact code.
