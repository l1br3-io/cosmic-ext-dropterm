# How should DropTerm ship a command palette — what do the peers do, and what shape fits the GPUI/Kit frontend?

Date: 2026-11-22

> Correction, 2026-11-22 (same day, before the owner ruled): the
> recommendation's open chord was `Ctrl+Shift+P` — the peers' chord — but
> `images_panel` already holds it in `BINDING_ROWS`
> (`dropterm/src/core/keybinds.rs:134-135`) and the registry is
> one-chord-one-action. The chord reads `Ctrl+Shift+O` throughout now; the
> reasoning sits in the Invocation item below.

## TL;DR

**Ship the palette as a pure core model (rows composed from the vocabularies
DropTerm already owns: the keybind registry, the context-menu/CLI verbs, the
profile list, the scheme registry, the open tabs) scored by `nucleo-matcher`
and rendered as an in-window modal in the close-confirm's proven shape, with
the Kit `List`'s own `escape`/`enter`/`up`/`down` key context as the innards
and `Ctrl+Shift+O` as the default open chord — the peers' `Ctrl+Shift+P` is
already held by DropTerm's images-panel bind, and the registry is
one-chord-one-action.** Every surveyed peer that has a
palette opens it from the Ctrl/Cmd+Shift+P family (WezTerm, Warp, VS Code,
Zed, kitty's remappable Ctrl+Shift+F3, iTerm2's Open Quickly descendant), and
every peer with a palette-sized entry set still runs a real fuzzy scorer
rather than plain substring filtering — WezTerm scores its palette commands
with `nucleo_matcher` over a rayon parallel pass, Zed scores its action
registry through its own nucleo facade, kitty ships a bespoke multi-word
matcher with typo tiers, Helix and Zed standardized the whole editor on
nucleo. The Kit already carries the palette's navigation engine (the `List`
component binds Escape/Enter/secondary-Enter/Up/Down in a `"List"` key
context deeper than the Root) and DropTerm has already implemented
`SearchableListDelegate` twice for the pickers, so the build is enumeration
and wiring, not new UI machinery. The stale `fuzzy-matcher` crate, the
full-TUI `skim` engine, a bespoke in-house scorer, and the Kit Dialog's
button chrome are rejected with evidence below.

## What DropTerm has today

Grounded in the working tree at HEAD `2796e3f50ba4c371be1a84ff3f03cc64326706e3`
(branch `ced2`, 2026-11-22):

- **A registry-shaped action vocabulary already exists and is palette-ready**:
  `core/keybinds.rs` holds one `BindingRow` per keybindable action —
  `id` (the `custom_key_binds` config key), `label`, `group`, `defaults`
  (`dropterm/src/core/keybinds.rs:43-51`) — 33 rows in four display groups,
  Clipboard/Tabs/Zoom/Other (`keybinds.rs:55-57`, `:64-140`). The human chord
  hint per row is already a pure function,
  `effective_binding_display` (`keybinds.rs:167-187`), and the row-vs-query
  test today is case-insensitive substring (`row_matches_search`,
  `keybinds.rs:191-193`) — the keybind editor's own search bar.
- **The context menu is the model for routing**: core's rows are plain data
  and "Item picks land in each frontend's `perform_action` path — the same
  path the keybind map uses" (`dropterm/src/core/context_menu.rs:6-8`); the
  profile folders compose dynamically over the live profile list
  (`context_menu.rs:100-118`) — the same dynamic-composition move a palette
  needs for per-profile rows.
- **The CLI verbs are a second action surface**: `toggle/show/hide`,
  `new-tab`, `close-tab`, `rename-tab`, `duplicate-tab`,
  `switch-profile-tab`, `select-tab` (`docs/cli.md:6-15`), mostly overlapping
  the keybind ids; the per-profile New/Switch profile verbs are deliberately
  *not* keybindable ("they act on a config-list index, which is not a stable
  user-facing id", `docs/keybindings.md:40`) — a palette rows them as dynamic
  entries exactly like the context menu's folders do.
- **The proven overlay shapes**: the settings drawer is "the Kit's `Sheet`
  over the live terminal" (`dropterm/src/drawer.rs:3-5`); the context menu is
  a shell-owned Kit `PopupMenu` "deferred above everything"
  (`dropterm/src/app_shell.rs:5476`, built at `:3877-3885`); the pickers are
  Kit `Select` whose delegates are DropTerm's own
  `SearchableListDelegate` impls (`dropterm/src/pickers.rs:40-41`, `:105-132`,
  `:236+`); and the close-confirm modal is an in-surface modal with its own
  key context (`.key_context("close-confirm")`, `app_shell.rs:5436`), its own
  `FocusHandle` that holds the keyboard (`modal_focus`, `app_shell.rs:417`,
  focused at `:2439`), and its Escape bound in that context at boot
  (`app_shell.rs:6083-6085`). The completions menu draws above the grid via
  `with_layer` (`dropterm/src/terminal/view.rs:2274`) — a draw-layer trick
  the palette does not need, since it is chrome, not grid overlay.
- **Key capture while the pane owns the keyboard**: the pane claims Tab in a
  `"Terminal"` key context to beat the Kit Root's global traversal eater
  (`bind_pane_keys`, `dropterm/src/terminal/view.rs:66-71`, registered at
  boot `app_shell.rs:6094`; the full story in
  [key-delivery](key-delivery.md)). A palette inverts that problem: its
  focused input simply holds focus while open — the drawer's rule
  ("Terminal input is inert — keyboard and pointer never reach the shell",
  `docs/settings.md:76`) already describes the wanted behavior for any
  focused overlay, and the Root's Tab-eater is irrelevant because a palette
  binds no Tab.
- **The chord standard**: every default keybind rides `Ctrl+Shift+<key>`
  except four sanctioned families (`docs/keybindings.md:36`); `Ctrl+Space`
  for the completion menu shows a peer-standard non-CS chord is admissible
  when justified. `hide_on_focus_loss` gates the whole surface's hide
  (`app_shell.rs:1303-1312`), not individual overlays; a focus-loss hide
  preserves the drawer and its drafts (`docs/settings.md:29`) and the palette
  should die with the surface rather than re-open on show.
- **No palette exists today**: a case-insensitive grep for `palette` over
  `dropterm/src` returns only color-palette hits (scheme/palette comments and
  the theme sources — `theme_source.rs`, `theme_glue.rs`, `app_shell.rs`'s
  color arm); no command-palette module, no command-search overlay beyond
  the keybind editor's substring filter. The settings drawer's five pages
  and the scheme pickers are reachable only by `Ctrl+Shift+I` navigation or
  right-click.

## Findings

All web sources fetched 2026-11-22 unless noted.

### 1. WezTerm — the closest peer: Rust, GPUI-era terminal, palette over its own action vocabulary

- Opened by the `ActivateCommandPalette` key assignment — "CTRL + SHIFT + P
  is the default key assignment for ActivateCommandPalette" — available
  since `20230320-124340-559cb7b0` (wezterm.org, `ActivateCommandPalette`).
  The docs call it "a modal overlay that enables discovery and activation of
  various commands."
- Entries are the config's key-assignment actions (with their menubar path
  labels) plus Lua-contributed rows: the palette builds from
  `CommandDef::actions_for_palette_and_menubar(&config::configuration())`,
  the internal row is `ExpandedCommand { brief, doc, action, keys, menubar,
  icon }`, CopyMode entries drop out unless the copy overlay is active, and
  the `augment-command-palette` window event — "emitted when the Command
  Palette is shown", synchronous — lets config code append entries with
  `brief`/`doc`/`action`/`icon` fields (wezterm.org `augment-command-palette`;
  source `wezterm-gui/src/termwindow/palette.rs`, main `ee6f3044d09f`). No
  tabs or workspaces in the entry set — actions only.
- Filtering is fuzzy and score-ranked: "Typing text (and using Backspace)
  allows you to fuzzy match possible actions. Each keystroke will reduce the
  list of candidate actions to those that fuzzy match, ranked in decreasing
  order of the match score" (wezterm.org, `ActivateCommandPalette`). The
  scorer is `nucleo_matcher` — `use nucleo_matcher::pattern::Pattern; use
  nucleo_matcher::{Matcher, Utf32Str};` with `CaseMatching::Ignore` and
  `Normalization::Smart`, run over group+brief+doc+action strings in a rayon
  `par_iter`, exact-brief matches pinned at `u32::MAX`
  (`termwindow/palette.rs`; the helpers live in
  `wezterm-gui/src/overlay/selector.rs`). WezTerm — a terminal with a
  palette of at most a few hundred rows — chose a full fuzzy scorer, not
  substring.
- Ranking carries persisted frecency: `Recent { brief, frecency }` records
  saved to `config::DATA_DIR.join("recent-commands.json")` via the `frecency`
  crate; on open, recents sort first by `frecency.score()`, then menubar
  path, then brief (`termwindow/palette.rs`). The docs' own line: "The
  command palette shows a list of possible actions ranked by frecency of use
  from the command palette."
- Capture and dismissal are a modal, not a key table: the palette implements
  the `Modal` trait (`wezterm-gui/src/termwindow/modal.rs`) and handles keys
  inline — Esc or Ctrl+g cancels, Up/Ctrl+p and Down/Ctrl+n move, Ctrl+u
  clears, Enter activates; "Activating the selected item will close the
  command palette and then invoke the action" (wezterm.org). On Enter the
  source saves the recent, calls `cancel_modal()`, then performs the key
  assignment on the active pane (`termwindow/palette.rs`). It renders as a
  computed element at zindex 100, horizontally centered, width clamped
  between 120 cells and full width.
- Keybinding hints: rows carry their chords (`ExpandedCommand.keys`), the
  same row anatomy Zed uses.

### 2. kitty — the palette as an action catalog with a bespoke matcher

- kitty ships a command palette bound to `map kitty_mod+f3` (default
  Ctrl+Shift+F3): "All mapped actions (those with a keyboard shortcut) and
  unmapped actions (those available but not bound to any key) are listed,
  organized by category", with mouse bindings in a separate section and F12
  toggling the unmapped rows, a preference remembered across sessions
  (sw.kovidgoyal.net, `kittens/command-palette`). Entry set: the app's full
  action vocabulary, categorized — not shell commands.
- Filtering is case-insensitive across three columns at once — "the key
  (keyboard shortcut), the action name, and the category" — with multi-word
  query tiers (exact word, then prefix, then typo tolerance: for words of at
  least four characters "a single typo ... still produces a match, and two
  typos give a lower-scoring match"), compound-name splitting on `_ + / -`
  delimiters, and ranking by matched-word count, then action-column score,
  key column, category column, shorter-name tiebreak. kitty's own scoring
  machinery, deliberately not an off-the-shelf fzf score.
- Keys inside: Enter runs, Escape "clears the query, or closes the palette
  if the query is already empty", Up/Ctrl+K/Ctrl+P and Down/Ctrl+J/Ctrl+N,
  PageUp/PageDown, Home/End. With an empty query, `action_alias` entries pin
  to the top — kitty's own recency-substitute ("Pinning frequently used
  actions"). kitty also has the unrelated hints kitten (on-screen text
  selection) — a different mechanism entirely, not a palette.
- Peer-genus note: kitty proves the *catalog* shape — an empty-query open
  shows everything, categorized; the search is a filter over the catalog.

### 3. Warp — the palette as global search over typed entry kinds

- Opened with Cmd+P (macOS) / Ctrl+Shift+P (Windows/Linux); the docs call
  it "global search to quickly locate Workflows, Notebooks, keyboard
  shortcuts, or other actions within Warp", and "Start typing to search for
  Workflows, Notebooks, keyboard shortcuts, actions, toggles, etc."
  (docs.warp.dev, `terminal/command-palette`).
- Entry kinds are explicit and prefix-filterable: `workflows:`/`w:`,
  `prompts:`, `notebook:`, `env_vars:`, `files:`, `drive:`, `actions:` (for
  "settings and features"), `sessions:`, `launch_configs:` — the palette is
  one search box over heterogeneous kinds with a namespace prefix per kind.
  The separately-opened Workflow palette (Ctrl+Shift+R) selects a workflow
  and pastes its command — that is the "palette can launch shell commands"
  case, and it is a different surface from the app-action palette.
- Ranking/recency and the activation behavior are not documented on the
  page (see Unverified).

### 4. iTerm2 3.5 — the palette absorbed "Open Quickly"

- iTerm2 3.5 (2024) shipped a Command Palette as a headline feature; the
  release-notes trail describes "Open Quickly" being "reworked into a command
  palette", opened via Cmd+Shift+O, covering menu commands, paste history,
  locations, and snippets. The specifics rest on search-result snippets of
  iterm2.com pages this fetch could not re-reach in detail — the news page
  today renders only a terse 3.5 summary ("improved navigation, filtering,
  light/dark mode color schemes, ChatGPT integration, and integration with
  1Password") and the downloads page paginates to 3.7.x — so the entry-set
  detail is carried under Unverified.

### 5. tmux — the negative precedent: prompts and choosers, no palette

- tmux has no command palette. Its interactive surfaces are the `:`
  command-prompt (with `-i` incremental input mode used by the copy-mode
  search commands), and the choose modes: "Put a pane into tree mode, where
  a session, window or pane may be chosen interactively from a tree"
  (choose-tree) and "Put a pane into customize mode, where options and key
  bindings may be browsed and modified from a list" (customize-mode)
  (tmux.1, master `78a19c87c833`). Community palettes are fzf popups over
  tmux commands (tmux-fzf) — third-party, not built in. The genus
  difference: tmux's choosers pick *targets* (session/window/pane); no
  tmux surface is a searchable list of app actions.

### 6. VS Code — the canonical "one chord, all functionality"

- "⇧⌘P (Windows, Linux Ctrl+Shift+P)" is introduced as "The most important
  key combination to know", and from the palette "you have access to all
  functionality within VS Code, including keyboard shortcuts for the most
  common operations" (code.visualstudio.com, User interface). The same quick
  input hosts the sibling palettes — Ctrl+P files, Ctrl+Shift+O symbols,
  Ctrl+G line — one modal, many modes distinguished by query prefix.
- The palette lists commands from the whole command registry (built-in plus
  every extension's contributions) with their keybinding hints; the docs
  pages fetched do not state the filter algorithm or the MRU behavior (see
  Unverified).

### 7. Zed — the GPUI reference implementation, read at source level

- Opened by `command_palette::Toggle`, bound `"f1"` and `"ctrl-shift-p"` in
  the Workspace context of `assets/keymaps/default-linux.json` (main
  `5e3cf96193ca`); the file finder rides Ctrl+P on the same layer, the
  VS Code arrangement.
- Enumeration is the action registry, statically: the palette calls
  `window.available_actions(cx)` — every action registered in the window's
  dispatch tree — filters through an optional `CommandPaletteFilter`,
  humanizes names (`humanize_action_name`), sorts by usage recency then
  name, and lets registered interceptors inject extra entries
  (`GlobalCommandPaletteInterceptor::intercept`, plus Zed-link parsing into
  `OpenZedUrl`); `command_aliases` settings rewrite queries
  (`crates/command_palette/src/command_palette.rs`, main). No argument entry
  — dispatch is `window.dispatch_action(action, cx)`, argument-free.
- Matching is nucleo through Zed's own facade: `use
  fuzzy_nucleo::{StringMatch, StringMatchCandidate};` then
  `fuzzy_nucleo::match_strings_async(&candidates, &query,
  fuzzy_nucleo::StringMatchOptions { ... }, executor).await` per query
  update, the query pre-cleaned by `normalize_action_query` (whitespace and
  double-colon collapsing, underscores to spaces)
  (`command_palette.rs`). Zed's action palette scores a few hundred rows
  through a real fuzzy engine, batch per keystroke — the exact scale
  DropTerm's palette would have.
- Modal plumbing is a workspace modal layer, read in full
  (`crates/workspace/src/modal_layer.rs`): views implement `ModalView`;
  `toggle_modal` opens one modal at a time, keeping `previous_focus_handle`
  and restoring it on close; dismissal is driven by the view emitting
  `DismissEvent`; the layer renders a full-screen `.occlude()` overlay whose
  `on_mouse_down` dismisses on click-away (the modal's inner content stops
  propagation). Focus-out dismissal is a per-modal option
  (`dismiss_on_focus_lost`).
- The list is a virtualized picker: `crates/picker/src/picker.rs` "displays
  its matches using `gpui::uniform_list`", with a `PickerDelegate`
  (`match_count`, `selected_index`, `update_matches`, `confirm`,
  `dismissed`, `render_match`), an editor head that takes focus and
  dismisses on blur, hover-select, and modal height capped at
  `DEFAULT_MODAL_MAX_HEIGHT` (24 rems). Rows are `ListItem` + fuzzy
  `HighlightedLabel` + `KeyBinding::for_action_in(...)` chord hints, with
  Change-Keybinding/Run footer affordances (`command_palette.rs`).

### 8. Helix, telescope, fzf-lua — the picker-genus precedents

- Helix exposes `command_palette` on Space+`?` ("Open command palette"),
  alongside its picker family (Space+f file picker, Space+b buffer picker,
  ...), in-picker keys Tab/Down/Ctrl+n next, Enter open, Esc close, and a
  `last_picker` reopen binding (`docs.helix-editor.com`, keymap; fetched
  2026-11-22). Helix's fuzzy layer is nucleo (see Finding 9).
- telescope.nvim is the generic scored picker: sorters return one integer
  score per line, `FILTERED = -1`, with a filename-tail boost heuristic and
  no category grouping (`lua/telescope/sorters.lua`, master `1e1bab73`,
  fetched 2026-11-21, pinned in
  [completion-dir-file-order](completion-dir-file-order.md)).
- fzf-lua registers as Neovim's `vim.ui.select` provider and matches through
  the external fzf binary (or `sk`) — the ranking brain lives in fzf's
  algorithm ("skim_v2, the default algorithm [of skim, is] loosely based on
  fzf's algorithm" is skim's own line; fzf-lua README, main `a8312697c30b`).

### 9. Fuzzy matching in Rust — the field has converged on nucleo

- **nucleo** (helix-editor/nucleo, master `8c16d47cdfa9`): "a highly
  performant fuzzy matcher written in Rust. It aims to fill the same use
  case as fzf and skim"; published under the helix-editor org, "used in the
  helix-editor". Two crates: `nucleo` (a fully managed streaming picker
  engine with worker threads) and **`nucleo-matcher`** — "If you are looking
  for a replacement of the fuzzy-matcher crate and not a fully managed fuzzy
  picker, you should use the nucleo-matcher crate", which is "finished and
  ready for widespread use" (README). License MPL-2.0. Multibyte handling is
  explicit: "nucleo will always use the first codepoint of the grapheme for
  matching" with grapheme indices reported for highlight positions.
  `nucleo-matcher` 0.3.1 (crates.io, updated 2024-02-20) is a ~50 KB crate
  with roughly three unicode dependencies (casefold, normalization,
  segmentation) — a small, self-contained dependency.
- **fuzzy-matcher** (skardos): the former default, MIT, `SkimMatcherV2` and
  a clangd matcher — but crates.io shows `updated_at: 2020-10-04`, max
  version 0.3.7, six years without a release (39.4M downloads of legacy
  usage). nucleo's README positions nucleo-matcher as its replacement, and
  the ecosystem moved: Zed and WezTerm both migrated off it (WezTerm's
  palette now imports `nucleo_matcher`; Zed built `fuzzy_nucleo`).
- **skim**: the fzf clone ("Fuzzy Finder in rust!", MIT, master
  `2160abbd0510`) is embeddable as a library but is a full TUI engine —
  crossbeam item channels, `SkimItemReader`, its own event loop — sized for
  filtering arbitrary streams, not for scoring a palette's row table.
- **Zed's own crates**: `crates/fuzzy` keeps Zed's original in-house scorer
  (`CharBag`-based, GPL-licensed crate) still used for path matching;
  `crates/fuzzy_nucleo` is the newer facade wrapping nucleo
  (`AtomKind::Fuzzy`, `Normalization::Smart`, case-insensitive matching with
  a `SMART_CASE_PENALTY_PER_MISMATCH = 0.9` smart-case penalty, batch
  execution with cancellation) — and Zed's *palette* uses the nucleo
  facade, not the in-house scorer (`crates/fuzzy_nucleo/src/fuzzy_nucleo.rs`,
  main `5e3cf96193ca`).
- **The feasibility answer**: DropTerm's palette is one to two hundred rows,
  and every peer at that scale still runs a real fuzzy scorer — WezTerm's
  palette scores its command list with nucleo over a rayon pass; Zed scores
  its whole action registry through nucleo per keystroke; kitty ships a
  bespoke tiered matcher with typo tolerance; Helix's small pickers (the
  buffer picker is often a dozen rows) run nucleo too. Nobody at palette
  scale ships bare substring filtering, and the cost is trivial: scoring
  200 rows with nucleo-matcher is microseconds, batch, no worker thread
  needed. DropTerm's in-house completion engine precedent argues for
  keeping dependencies out when the domain logic is DropTerm's own — but
  ranked fuzzy scoring is a solved, Unicode-subtle problem (graphemes,
  normalization, case folding) where every serious peer standardized on one
  maintained crate; a second in-house matcher would be a fork of nucleo's
  correctness work.

### 10. What the Kit (gpui-component 0.9.1, carried copy at `carried/gpui-component`) already provides

- **A palette-shaped navigation engine**: the `List` component binds its own
  key context — `KeyBinding::new("escape", Cancel, context)`,
  `"enter" → Confirm { secondary: false }`, secondary-modified Enter →
  `Confirm { secondary: true }`, `"up" → SelectUp`, `"down" → SelectDown`
  (`carried/gpui-component/src/list/list.rs:28-41`), stamped
  `.key_context("List")` on the rendered frame (`list.rs:685`), with
  `on_action_cancel` emitting `ListEvent::Cancel` (`list.rs:369-376`) and
  `on_action_confirm` delegating to the delegate and emitting
  `ListEvent::Confirm` (`list.rs:380-397`), plus mouse-click activation
  (`list.rs:513-521`). Deeper than the Root, so the tab-traversal eater
  never applies — and a palette binds no Tab anyway.
- **A search-input + list state with blur dismissal**: `SearchableListState`
  carries its own `FocusHandle`, the query input's and the list's focus
  handles, `on_confirm`/`on_cancel`/`on_render_empty` forwarding, and
  subscriptions on blur of all three handles — focus-loss closes the popover
  (`carried/gpui-component/src/searchable_list/state.rs:21`, `:53`, `:76-82`,
  `:107-109`, `:151-152`). The delegate contract is the palette contract:
  `SearchableListItem` (title/value/render/disabled, with a default
  case-insensitive substring `matches` at
  `searchable_list/delegate.rs:43-45`) and `SearchableListDelegate`
  (`perform_search`, `render_item`, `render_section_header`, `on_confirm`
  — `delegate.rs:54-207`). DropTerm already implements both traits for the
  font and scheme pickers (`dropterm/src/pickers.rs:105-132`, `:236+`) —
  the codebase's own precedent for building on this machinery.
- **Modal containers it did NOT build the palette with**: `Sheet` (the
  drawer's side panel, `sheet.rs:41-56`), `Dialog` with ok/cancel button
  anatomy and a 0.25 s entrance animation (`dialog/dialog.rs:24`), and the
  Root's `open_dialog`/`close_dialog`/`open_sheet_at` plumbing
  (`root.rs:375`, `:447`, `:514`, `:545`). A palette wants none of the
  button chrome; its container is the close-confirm modal's shape — a
  shell-owned entity state, an overlay div with a scrim, a key context, and
  a held `FocusHandle` (`app_shell.rs:5300+`, `:5436`, `:417`, `:2439`).
  Zed's palette is the same arrangement (`ModalView` + occluded scrim +
  focused editor head), reached from the other direction.
- **Keybinding hints**: the Kit carries a `kbd` component
  (`carried/gpui-component/src/kbd.rs`) for rendering chord text, and
  DropTerm's own `chord_display` already produces the human form
  (`core/keybinds.rs:144-156`).

## The verdict table

| Tool | Open chord | Entry set | Filtering | Ranking | Can launch shell commands |
|---|---|---|---|---|---|
| WezTerm | Ctrl+Shift+P (`ActivateCommandPalette`) | key-assignment actions + Lua `augment-command-palette` entries | fuzzy, nucleo-matcher, score-ranked | persisted frecency (`recent-commands.json`) | actions only (PromptInputLine can ask, then act) |
| Warp | Cmd+P / Ctrl+Shift+P | workflows, notebooks, prompts, shortcuts/actions, env vars, files, drive, sessions, launch configs | typed filter + kind prefixes | not documented | workflows paste/pause for args; separate Ctrl+Shift+R palette |
| iTerm2 3.5 | Cmd+Shift+O (Open Quickly reworked) | menu commands, paste history, locations, snippets (detail unverified) | fuzzy (unverified) | unverified | — |
| kitty | Ctrl+Shift+F3 (`command_palette`) | all mapped + unmapped actions, categorized | case-insensitive multi-word, 3 columns, typo tiers | matched-word count, column priority, alias pinning | actions only |
| tmux | — | no palette | — | — | command-prompt runs tmux commands |
| VS Code | Ctrl+Shift+P | all commands (built-in + extensions), settings, quick-input modes | filter-as-you-type (algorithm undocumented) | MRU (unverified) | tasks/debug via commands |
| Zed | Ctrl+Shift+P / F1 (`command_palette::Toggle`) | window action registry, filtered, interceptors can inject | fuzzy, nucleo via fuzzy_nucleo, batch per keystroke | usage recency, then name | Zed-link/interceptor entries only |
| Helix | Space+`?` | all typed commands (`command_palette`) + the picker family | fuzzy (nucleo) | picker-score | no |
| telescope / fzf-lua | user-bound | any list, incl. `vim.ui.select` | fzf-algorithm score | score + tiebreaks | no (picker only) |

## Recommendation

Build the palette in four pieces, in the house split:

1. **The pure model, `core/palette.rs`** (new): one `Command` row type —
   stable `id`, `label`, section/group, an optional chord hint resolved
   through `effective_binding_display`, and a dispatch tag naming the
   existing verb path — plus the row composition: every
   `BindingRow` (`core/keybinds.rs:64-140`), the context-menu verbs that are
   not keybound, the CLI's toggle/show/hide, the five settings pages, one
   row per scheme from the merged registry
   (`core/theme_source.rs`), one row per open tab, and the per-profile
   New/Switch rows composed over the live profile list exactly as the
   context-menu folders compose (`core/context_menu.rs:100-118`). Picks
   route to the shell's existing verb paths — the context-menu rule:
   "Item picks land in each frontend's `perform_action` path — the same
   path the keybind map uses" (`core/context_menu.rs:6-8`). Matching is a
   pure function over labels scored by `nucleo-matcher` (batch per query,
   no worker): `Pattern::new(query, CaseMatching::Ignore,
   Normalization::Smart)` scored over each row's label, rows kept on
   `Some(score)` and sorted by score then registry order; the empty query
   returns the full catalog in section order (kitty's catalog-open shape).
   Every row carries its section so the empty-query view renders kitty-style
   category headers and the filtered view renders one scored flat list.
2. **The bin overlay, `dropterm/src/palette.rs`** (new): the close-confirm
   modal's proven container — a shell-owned open flag, a scrim div with
   click-away dismiss, a `"palette"` key context, and a held
   `FocusHandle` focused on open (`app_shell.rs:417`, `:2439`, `:5436` give
   every line of the pattern) — hosting a centered, top-third panel:
   a Kit text `Input` for the query and the Kit `List` (or
   `SearchableListState`) for the rows, so the escape/enter/up/down
   bindings arrive for free in the `"List"` context
   (`carried/gpui-component/src/list/list.rs:28-41`, `:685`) and focus-loss
   dismiss arrives from the state's blur subscriptions
   (`searchable_list/state.rs:107-109`). Rows render with the Kit `kbd`
   chord hint from the model's `effective_binding_display` string.
   Registered at boot beside the Kit init and `bind_pane_keys`
   (`app_shell.rs:6082-6094`).
3. **Invocation**: a new `command_palette` row in `BINDING_ROWS`, default
   `Ctrl+Shift+O` — inside the house `Ctrl+Shift+<key>` standard
   (`docs/keybindings.md:36`). The chord the peers converge on,
   `Ctrl+Shift+P`, is not available: `images_panel` holds it
   (`core/keybinds.rs:134-135`), the registry is one-chord-one-action
   (`capture_verdict` refuses a chord another row holds,
   `keybinds.rs:221-248`), and re-homing a shipped binding is not this
   feature's call. `Ctrl+Shift+O` is the best-evidenced free letter —
   iTerm2's palette rides Cmd+Shift+O (Finding 4) — and the chord stays
   customizable through the existing `custom_key_binds` machinery like every
   action; a later owner ruling can re-home `images_panel` and move the
   palette onto P. V1 toggles (open again while open dismisses, Zed's
   `toggle_modal` rule).
4. **Capture, dismissal, and portability**: the palette's input holds focus
   while open — terminal input is inert by the drawer's existing rule
   (`docs/settings.md:76`); no Terminal-context interaction is needed, the
   pane's claim (`view.rs:66-71`) simply never fires because focus is
   elsewhere. Escape (the List's Cancel), the scrim press, and blur dismiss;
   a whole-surface focus-loss hide follows `hide_on_focus_loss` as today and
   the palette dies with the surface, never re-opening on show. The overlay
   lives inside the window root — never a second layer surface — so the
   GNOME plain-window fallback inherits it unchanged; the exclusive
   layer-shell keyboard needs no extra claim because the palette rides the
   surface's existing grab.

**Scope ruling**: V1 is built-in actions only, but the row type's stable
`id` + the composition function's signature should accept appended rows so
the plugins initiative's Lua "script commands" slot in as extra entries —
WezTerm's `augment-command-palette` is the exact precedent of a palette that
grew a third-party row source without changing its shape
([terminal-plugin-systems](terminal-plugin-systems.md) already plans the
surface; this record only fixes its entry point). Palette-launches-shell
(Warp workflows) is out of scope: no surveyed terminal palette runs shell
commands directly — the Workflow-style surfaces that do are separate,
command-pasting features.

- **Rejected: the `fuzzy-matcher` crate.** Six years without a release
  (crates.io `updated_at: 2020-10-04`, max 0.3.7) and nucleo's README names
  nucleo-matcher as its replacement; both WezTerm and Zed already migrated
  off it. Adopting it in 2026 would be adopting the ecosystem's past.
- **Rejected: `skim` as an embedded library.** A full fzf-clone engine —
  crossbeam channels, item readers, its own event loop — to score ~150
  rows. Dependency weight with no matching benefit.
- **Rejected: an in-house scorer (kitty-style tiers).** kitty built its own
  three-column typo-tolerant matcher, but every Rust peer (Helix, WezTerm,
  Zed) standardized on nucleo instead of maintaining one; the Unicode
  subtleties nucleo handles (grapheme-indexed highlights, normalization,
  case folding) are exactly where a hand-rolled scorer rots. DropTerm's
  own-engine precedent covers shell-completion semantics — DropTerm's
  domain — not ranked fuzzy text scoring, which is not.
- **Rejected: substring-only filtering (the Kit default and the keybind
  editor's current search).** Both are real precedents *inside this repo*
  (`searchable_list/delegate.rs:43-45`, `core/keybinds.rs:191-193`), but no
  peer palette ships bare substring at this scale, and it silently fails the
  two queries palettes exist for: multi-word ("scroll clear" → Clear
  scrollback) and typo tolerance. nucleo-matcher is ~50 KB with ~3 unicode
  deps — cheaper than the ranking quality given up.
- **Rejected: the Kit `Dialog` (or `Sheet`) as the container.** Dialog's
  anatomy is title + buttons + a 0.25 s entrance
  (`carried/gpui-component/src/dialog/dialog.rs:24`); Sheet is a side panel
  for settings, already DropTerm's drawer. A palette is a top-centered,
  button-less, instant modal — the close-confirm shape DropTerm already
  ships (`app_shell.rs:5436`'s key context, `:417`'s held handle). Zed's
  palette is that same arrangement on GPUI.
- **Rejected: persisted recency in V1.** WezTerm persists frecency to a JSON
  file and sorts recents first; Zed sorts by session usage recency; kitty
  pins aliases. All three are post-V1 refinements — V1 ships registry order
  (empty query) and score order (querying), with session-scoped recency the
  natural first increment and disk persistence deferred until the session
  model has a home for it.
- **Rejected: a second layer surface for the palette.** It would tie the
  palette to layer-shell, break the GNOME plain-window parity the
  gnome-window initiative is building, and buy nothing over an in-window
  overlay — the drawer, the context menu, the close-confirm modal, and the
  images panel are all in-window chrome and none needs a surface.

## Relation to prior records

- **Consumes** [key-delivery](key-delivery.md) (2026-11-21): the Tab-eater
  analysis defines the capture terrain the palette lands in; the palette's
  answer is the inverse of the pane's (hold focus rather than claim keys) and
  the record's `"List"`-context facts show the Kit already equips deeper
  contexts.
- **Extends** [terminal-plugin-systems](terminal-plugin-systems.md)
  (2026-11-21): that record ruled the Lua tier's "script commands" surface;
  this record fixes where those commands surface (palette rows appended by
  id) without building them now — WezTerm's `augment-command-palette` is the
  blueprint both records cite independently.
- **Touches** [bare-word-path-completion](bare-word-path-completion.md) and
  [completion-dir-file-order](completion-dir-file-order.md) only by genus:
  those pin the completions menu's ordering; this record's palette is a
  different surface and shares none of its code. The nucleo recommendation
  here does not touch the completion engine.
- Supersedes nothing; no prior record ruled on a palette.

## Sources

All fetched 2026-11-22 unless noted.

- DropTerm local tree @ HEAD `2796e3f50ba4c371be1a84ff3f03cc64326706e3`
  (branch `ced2`): `dropterm/src/core/keybinds.rs` (:43-51 BindingRow;
  :55-57, :64-140 the 33-row registry; :144-187 display; :191-193
  substring search), `dropterm/src/core/context_menu.rs` (:6-8 the routing
  rule; :66-76 ROWS; :100-118 profile composition),
  `dropterm/src/terminal/view.rs` (:66-71 `bind_pane_keys`; :2274
  `with_layer` note), `dropterm/src/app_shell.rs` (:417, :2439
  `modal_focus`; :5436 `close-confirm` context; :6082-6094 boot
  registrations; :5476, :3877-3885 the shell-owned PopupMenu; :1303-1312
  `hide_on_focus_loss`), `dropterm/src/drawer.rs` (:3-5 the Kit Sheet),
  `dropterm/src/pickers.rs` (:40-41, :105-132, :236+ the
  SearchableListDelegate impls), `dropterm/src/tab_bar.rs` (:960-962 the
  inline rename input state), `carried/gpui-component/src/list/list.rs`
  (:4, :28-41 the List key bindings; :369-397 cancel/confirm; :513-521
  click activation; :685 the `"List"` context),
  `carried/gpui-component/src/searchable_list/state.rs` (:21, :53, :76-82,
  :107-109, :151-152 focus + blur dismissal),
  `carried/gpui-component/src/searchable_list/delegate.rs` (:8-51 item
  trait, substring default at :43-45; :54-207 delegate trait),
  `carried/gpui-component/src/sheet.rs` (:41-56),
  `carried/gpui-component/src/dialog/dialog.rs` (:24),
  `carried/gpui-component/src/root.rs` (:375, :447, :514, :545),
  `carried/gpui-component/src/kbd.rs`; docs `keybindings.md:36,40`,
  `settings.md:13,29,76`, `cli.md:6-15`, `theming.md:19,27`
- WezTerm docs, wezterm.org: `ActivateCommandPalette` (default chord,
  "modal overlay", frecency ranking, fuzzy match, Esc/arrows/Ctrl+u/Enter,
  "Activating the selected item will close the command palette and then
  invoke the action", since `20230320-124340-559cb7b0`),
  `augment-command-palette` (the sync event, brief/doc/action/icon fields)
  — <https://wezterm.org/config/lua/keyassignment/ActivateCommandPalette.html>,
  <https://wezterm.org/config/lua/window-events/augment-command-palette.html>
- WezTerm source @ main `ee6f3044d09f` (2026-11-20):
  `wezterm-gui/src/termwindow/palette.rs` (ExpandedCommand;
  actions_for_palette_and_menubar; nucleo_matcher imports; rayon par_iter;
  frecency via `recent-commands.json`; Modal-trait key handling; zindex-100
  overlay), `wezterm-gui/src/overlay/selector.rs` (matcher_pattern /
  matcher_score helpers), `wezterm-gui/src/termwindow/modal.rs` (the Modal
  trait) — <https://github.com/wezterm/wezterm>
- Warp docs, docs.warp.dev `terminal/command-palette` (Cmd+P /
  Ctrl+Shift+P; "global search"; entry kinds and filter prefixes) —
  <https://docs.warp.dev/terminal/command-palette>
- kitty command-palette kitten docs, sw.kovidgoyal.net
  (`kittens/command-palette/`: ctrl+shift+f3, mapped+unmapped by category,
  three-column matching, typo tiers, ranking order, key controls, F12,
  alias pinning) — <https://sw.kovidgoyal.net/kitty/kittens/command-palette/>
- tmux @ master `78a19c87c833` (2026-11-22): `tmux.1` (choose-tree and
  customize-mode quotes; command-prompt `-i` cross-reference; no palette) —
  <https://github.com/tmux/tmux>, rendered man page at man7.org
- VS Code docs, code.visualstudio.com `docs/getstarted/userinterface`
  ("The most important key combination to know"; "access to all
  functionality"; the sibling quick inputs) —
  <https://code.visualstudio.com/docs/getstarted/userinterface>
- Zed @ main `5e3cf96193ca` (2026-11-22):
  `crates/command_palette/src/command_palette.rs` (available_actions
  enumeration, filter, humanize, recency sort, fuzzy_nucleo call, row
  rendering with KeyBinding hints, DismissEvent + focus restore, Zed links),
  `crates/workspace/src/modal_layer.rs` (ModalView, toggle_modal,
  previous-focus restore, occluded scrim click-away),
  `crates/picker/src/picker.rs` (uniform_list quote, PickerDelegate, editor
  head, blur dismiss, 24-rem cap), `assets/keymaps/default-linux.json`
  (f1/ctrl-shift-p), `crates/fuzzy_nucleo/src/fuzzy_nucleo.rs` (nucleo
  facade, smart-case penalty), `crates/fuzzy/` (the in-house CharBag
  scorer, GPL) — <https://github.com/zed-industries/zed>
- Helix docs, docs.helix-editor.com `keymap.html` (Space+`?`
  command_palette; picker keys; `last_picker`) —
  <https://docs.helix-editor.com/keymap.html>
- nucleo @ master `8c16d47cdfa9` (2026-06-22): README ("highly performant
  fuzzy matcher ... fill the same use case as fzf and skim"; used in helix;
  nucleo-matcher as the fuzzy-matcher replacement, "finished and ready for
  widespread use"; grapheme matching; MPL-2.0) —
  <https://github.com/helix-editor/nucleo>; nucleo-matcher 0.3.1 via
  crates.io API (updated 2024-02-20, ~50 KB, unicode deps)
- fuzzy-matcher via crates.io API (updated 2020-10-04, max 0.3.7, MIT per
  docs.rs) — <https://crates.io/api/v1/crates/fuzzy-matcher>,
  <https://docs.rs/fuzzy-matcher/latest/fuzzy_matcher/>
- skim @ master `2160abbd0510` (2026-11-21): README ("Fuzzy Finder in
  rust!", `sk`, skim_v2 "loosely based on fzf's algorithm", library
  embedding shape, MIT) — <https://github.com/skim-rs/skim>
- fzf-lua @ main `a8312697c30b` (2026-11-19): README ("Improved fzf.vim
  written in lua", `register_ui_select`, fzf-binary matching) —
  <https://github.com/ibhagwan/fzf-lua>
- telescope.nvim @ master `1e1bab73` (fetched 2026-11-21, pinned in
  [completion-dir-file-order](completion-dir-file-order.md)):
  `lua/telescope/sorters.lua` — <https://github.com/nvim-telescope/telescope.nvim>
- iTerm2, iterm2.com `news.html` (the 3.5 headline summary line) —
  <https://iterm2.com/news.html>; the 3.5 command-palette specifics cite
  search results of iterm2.com pages (see Unverified)

## Unverified

- **iTerm2 3.5's palette detail is the survey's weakest pin.** The news
  page today renders only a terse 3.5 summary, and the downloads page
  paginates to 3.7.x; the Cmd+Shift+O shortcut, the Open-Quickly-reworked
  wording, and the entry set (menu commands, paste history, locations,
  snippets) come from search-result snippets of those pages, not from a
  fetched primary. The feature's existence in 3.5 is corroborated
  multi-source; the details should be re-pinned from a 3.5.0 changelog
  before being quoted as fact.
- **VS Code's filter algorithm and MRU behavior**: the fetched docs pages
  do not state whether the command palette filters fuzzily or by substring,
  nor the `workbench.commandPalette.history` default. Stated as unknown, not
  guessed.
- **Warp's ranking/recency and activation behavior** are not documented on
  the command-palette page fetched; only the entry kinds and prefixes are
  primary.
- **tmux's `command-prompt` full description** could not be quoted (the man
  page fetch truncated before its entry); the `-i` incremental-input fact is
  pinned from the copy-mode cross-reference, and tmux's no-palette verdict
  rests on the commands the fetched sections do document plus the plugin
  ecosystem.
- **Whether the pinned gpui-pre 0.3.12 exposes `Window::available_actions`**
  (the API Zed's palette enumerates through) was not verified — no vendored
  copy of gpui-pre exists in this checkout and the registry cache was not
  reachable. Immaterial to the recommendation: DropTerm's palette enumerates
  from its own core registries, not from a GPUI action registry.
- **kitty's palette ships as a kitten**: the docs page pinned above is
  primary for behavior, but the implementation source (the Python kitten in
  the kitty repo) was not read; the matcher description is the docs' own,
  not source-verified line-level.
- **gpui-component version note**: the carried copy under
  `carried/gpui-component` is the patched 0.9.1 pin (the kit-root-border
  ruling); upstream longbridge/gpui-component was not fetched this pass —
  all Kit citations are to the carried sources DropTerm actually builds
  against, which is the authoritative copy for this codebase.
