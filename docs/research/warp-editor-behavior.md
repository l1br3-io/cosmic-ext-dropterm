# How does Warp's input editor actually behave — the prompt key surface, history, signals, paste, highlighting, live-prompt gating, and the execution path — and what should DropTerm's rich-input module copy?

**Question:** Warp's client is open source; what are the exact mechanics of
the input editor itself (the editing key surface, the history Up/Down lane,
the Ctrl+C/D/Z/L signal semantics, multi-line paste, ghost suppression inside
the editor, typed-line highlighting, the settings keys and defaults, how the
editor stands aside for foreground programs, and what Enter does to the
shell) — the editor behaviors `warp-completion-behavior.md` deliberately did
not cover — so DropTerm's `rich-input` module can replicate them?

**Date:** 2026-11-25

**TL;DR:** Warp's prompt is a real multi-line text editor (undo/redo, word
motions, kill/yank, readline-emulated Ctrl+A/E/B/F/N/P/H/K/U/W, newline
keys) that owns the line until Enter: Ctrl+C clears the editor (it is not a
raw SIGINT while text is held), Ctrl+D is forward-delete, Ctrl+Z is undo,
Ctrl+L clears the screen, multi-line pastes insert verbatim into the buffer
and nothing executes until Enter runs the whole buffer, and Up opens an
inline history menu over a store that merges Warp's own sqlite table with
the shell's history file per host, deduped first-occurrence-wins. Every one
of those behaviors is copyable for DropTerm's rich-input v1 as specified —
editing core, store history, the signal trio, verbatim paste with
Enter-only execution — and none requires anything Warp keeps closed:
behavior only, the AGPL client is never copied. Two things are explicitly
NOT Warp-parallel in DropTerm's ruling: our Enter is a PTY write (the shell
keeps its own line discipline and echo), and our Tab stays the shell's key
outside a showing menu (SP-D10), which Warp's own-editor reality never had
to face.

## Findings

Citations are of two kinds, both first-party. Code citations are into
`github.com/warpdotdev/warp` @ `f8223ce` (committed 2026-11-20), carried
over from the two shipped records, which pinned them against this identical
rev ([warp-completion-behavior](warp-completion-behavior.md),
[warp-completion-depth](warp-completion-depth.md)); docs citations are the
current first-party docs pages (fetched 2026-11-25), used where the chord
table and settings defaults live in user-facing reference pages rather than
in files that render under the 17,492-line `app/src/terminal/input.rs`
ceiling. The docs lag-or-lead `f8223ce` by days, not quarters; the gaps are
in **Unverified**.

### 1. The editor key surface: readline chords over a modern text editor

- Warp's documented input-editor bindings
  ([keyboard-shortcuts](https://docs.warp.dev/getting-started/keyboard-shortcuts),
  "Input Editor" sections, fetched 2026-11-25): motions `Ctrl+A` "Move to
  Start of Line", `Ctrl+E` "Move to End of Line", `Ctrl+B` "Move Cursor
  Left", `Ctrl+F` "Move Cursor Right / Accept Autosuggestion", `Ctrl+N`/
  `Ctrl+P` cursor down/up, `Meta+B`/`Meta+F` word motions (Linux
  `Ctrl+←`/`Ctrl+→`), `Home`/`End` (mac `⌘+←`/`⌘+→`), `⌘+↓`/`Ctrl+End`
  "Move Cursor to the Bottom"; deletes `Ctrl+H` "Remove the Previous
  Character", `Ctrl+D` "Delete", `Option+Backspace`/`Option+Delete` word
  left/right (Linux `Ctrl+Backspace`/`Ctrl+Delete`), `⌘+Backspace` "Delete
  All Left"; kill/yank-family `Ctrl+W` "Cut Word Left", `Ctrl+K` "Cut All
  Right", `Ctrl+U` "Copy and Clear Selected Lines", `Meta+D` "Cut Word
  Right", `Meta+.` "Insert Last Word of Previous Command"; undo/redo
  `⌘+Z`/`Ctrl+Z` "Undo", `⌘+Shift+Z`/`Ctrl+Shift+Z` "Redo"; newline
  `Ctrl+J` "Insert Newline" plus `Shift+Enter`/`Ctrl+Enter`/`Option+Enter`
  "Insert newline"
  ([editor](https://docs.warp.dev/terminal/editor)); clipboard
  `Ctrl+C/X/V`-family on Linux, `⌘+C/X/V` on mac.
- The editor is a full multi-line text model, not a line discipline: click
  to place the cursor, soft wrap with triple-click spanning wrapped lines,
  word selection, quote/bracket auto-close behind
  `autocomplete_symbols` (default on)
  ([editor](https://docs.warp.dev/terminal/editor); all-settings reference).
  An optional Vim layer exists behind `vim_mode_enabled` (default
  `false`, status bar behind `vim_status_bar` default `true`)
  (all-settings reference; `crates/vim` in the workspace listing;
  [vim](https://docs.warp.dev/terminal/editor/vim)).
- The menu-time key surface is pinned in code and carries over unchanged
  from the shipped record: Tab's LCP-fill-else-cycle
  (`app/src/terminal/input.rs:13662-13758`), Up/Down/Shift+Tab wrap while a
  menu shows (`input.rs:9763-9769`, `13506-13512`), Escape closes the
  buffer untouched (`input.rs:9851-9906`), Tab-while-closed opens
  completions because Warp's input is its own editor and Tab is never a
  shell byte there (`input.rs:13767-13768`).

### 2. History: one model over three sources, Up opens a menu

- The store merges three sources under one `History` model: the sqlite
  persisted-commands table (entries carry command, pwd, timestamps, exit
  code, shell host, session id), the shell's own history file read per
  `ShellHost` in a background task at session init, and the live session's
  commands (`app/src/terminal/history.rs:161-187, 233-256, 798-859`,
  per the depth record; the file's structure verified directly at
  `f8223ce` — `persisted_commands_summary`, `history_file_commands`,
  `session_commands`, `session_skip_indices`).
- Dedupe is first-occurrence-wins: the history file's raw lines go through
  `dedupe_from_last`, then each session renders the histfile+session
  concatenation through a skip-index set that hides any duplicate already
  seen when walking newest-first, and appended session commands are
  deduped the same way against the whole concatenation (verified in the
  `f8223ce` blob of `app/src/terminal/history.rs`;
  `initialize_session_start_and_skip_indices`, `append_commands`).
- Per-directory context rides the entry's pwd field: the ghost's recency
  tier prefers the current working directory, and the similar-context
  tier requires equal pwd + exit code + shell
  (`app/src/terminal/input.rs:10322-10340`,
  [warp-completion-depth](warp-completion-depth.md) finding 3).
- Up/Down at the prompt: `Ctrl+R` is "Command Search"
  (keyboard-shortcuts reference, Warp Essentials), and the Up arrow opens
  the inline up-arrow history menu — filterable, with per-item hide and
  commands-vs-prompts tabs per the shipped changelog — not bare readline
  recall
  ([changelog 2025/2026](https://docs.warp.dev/changelog/2026);
  the menu lives at `app/src/terminal/input/inline_history/` and
  `cloud_mode_v2_history_menu.rs` in the `input/` tree listing). The
  recent commit `b231216` ("Defer inline history navigation to avoid
  circular view updates", Oct 2 2026, in the file's commit history)
  confirms the Up/Down navigation lane is the inline menu machinery.

### 3. Signals: the editor consumes the chord, the shell gets bytes only when the editor stands aside

- Documented editor actions
  (keyboard-shortcuts reference): `Ctrl+C` "Clear Command Editor",
  `Ctrl+D` "Delete" (forward-delete of the character at the cursor),
  `Ctrl+L` "Clear Screen", `Ctrl+Z` "Undo" on Windows/Linux (mac lists
  `⌘+Z`, leaving bare `Ctrl+Z` unbound there — forwarded to the shell as
  the SIGTSTP byte). While the completions menu is open, `Ctrl+C` closes
  the menu instead (`app/src/terminal/input.rs:11584-11586`, shipped
  record finding 4).
- The readline meaning survives only as editor operations: Ctrl+C is
  clear-the-buffer, not SIGINT, while the editor holds the line. The
  doc pages do not state an empty-buffer Ctrl+C special case (SIGINT
  forward) — see **Unverified**.
- During running programs the forwards are explicit and narrow: "During
  long-running or full-screen commands, `Page Up`, `Page Down`, `Home`,
  and `End` are forwarded to the running program"
  ([block-basics](https://docs.warp.dev/terminal/blocks/block-basics));
  mouse, scroll, and focus reporting to full-screen apps are separate
  settings, all defaulting on (`app/src/terminal/alt_screen_reporting.rs`,
  the `AltScreenReporting` `define_settings_group!` —
  `mouse_reporting_enabled`, `scroll_reporting_enabled`,
  `focus_reporting_enabled`, each default `true`).

### 4. Paste: verbatim into the buffer, execution stays Enter-only

- The editor paste is a text-editor paste: multi-line clipboard text
  lands in the buffer as multi-line content (the editor's own multi-line
  model, finding 1), and nothing executes until Enter runs the buffer
  (finding 9). The docs carry no run-N-lines confirmation prompt for the
  terminal input editor, and the all-settings reference lists no
  safe-paste or paste-confirmation key — its only paste-adjacent settings
  are `middle_click_paste_enabled` (default `true`) and
  `right_click_behavior` (default `"context_menu"`, with a paste option)
  (all-settings reference; the right-click setting's landing commit
  `c25ac40`, Aug 20 2026, in the `input.rs` history).
- Bracketed paste is a PTY-line-discipline concept; while Warp's editor
  owns the line it does not apply, and when keys forward to the shell
  (native terminal mode, running programs) the emulator's normal
  bracketed-paste handling takes over. For DropTerm the same split holds:
  owned prompt = editor paste, native mode = today's PTY paste path.

### 5. Ghost text inside the editor: carried over, plus one accept key in the docs

- The shipped records pin the ghost's editor-native mechanics at this
  rev: ghost text is text appended after the buffer
  (`app/src/editor/view/mod.rs:3517-3547`; location always EndOfBuffer,
  `app/src/terminal/input.rs:8413-8427`), generated per keystroke with
  abort + buffer-equality freshness (`input.rs:10165-10395,
  17304-17329`), and suppressed while a *manually* opened menu shows but
  not under as-you-type (`input.rs:13289-13300`) — the suppression rule
  DropTerm already mirrors.
- Accept keys: Right accepts the whole ghost, Alt+Right one word
  (`app/src/editor/view/mod.rs:5997-6477`, depth record finding 3), and
  the docs add `Ctrl+F` "Move Cursor Right / Accept Autosuggestion"
  (keyboard-shortcuts reference) — cursor-right and ghost-accept share a
  chord, exactly DropTerm's own Right-accepts shape.

### 6. Input highlighting: live token coloring plus unknown-command underlining

- Warp colors the typed line in real time: "colors each part of a command
  to help differentiate between sub-commands, options/flags, arguments,
  and variables", with errors underlined — a dashed red underline when
  "the binary for the command you've typed does not exist"
  ([syntax-error-highlighting](https://docs.warp.dev/terminal/editor/syntax-error-highlighting)).
  Token classes: sub-commands, options/flags, arguments, variables — the
  same class set as the completer's HIR `CompletionLocation`s
  (`crates/warp_completer/src/completer/engine/mod.rs:24-51`, shipped
  record finding 2), so the parser drives both completion and paint.
- Both features default on: settings `syntax_highlighting` (default
  `true`) and `error_underlining_enabled` (default `true`), toggled under
  Settings > Features > Terminal Input (all-settings reference; the
  highlighting page). The known-command check rides the once-per-session
  command scan of the user's own shell
  (`crates/warp_terminal/src/shell/mod.rs:615-690`,
  `app/src/terminal/model/session.rs:1265-1367`, depth record finding 6);
  newly installed commands do not highlight correctly until a new session
  (syntax-error-highlighting page).

### 7. Settings: every input-editor key and default, verified

- The shipped record's Unverified flag is settled and stands re-verified:
  `completions_open_while_typing` defaults to **false**
  (`app/src/settings/input.rs:48-87`, depth record finding 6; confirmed
  by the current all-settings reference). The settings keys are declared
  through the `define_settings_group!` macro of the `settings` crate
  (`crates/settings`, workspace listing) into `app/src/settings/*.rs`.
- The input-editor-relevant defaults (all-settings reference, fetched
  2026-11-25): completions `warp_completions_enabled` `true`,
  `native_shell_completions_enabled` `true`,
  `classic_completions_mode` `false`, `completions_open_while_typing`
  `false`; ghost `autosuggestions.enabled` `true`,
  `autosuggestions.keybinding_hint` `true`,
  `autosuggestions.show_ignore_button` `false`,
  `intelligent_autosuggestions_enabled` `true`; editor
  `syntax_highlighting` `true`, `error_underlining_enabled` `true`,
  `autocomplete_symbols` `true`, `vim_mode_enabled` `false`,
  `vim_status_bar` `true`; behavior `input_mode` `"pinned_to_bottom"`,
  `input_box_type_setting` `"classic"` (the terminal-first style; the
  AI-first `"universal"` is opt-in), `alias_expansion_enabled` `false`,
  `command_corrections` `true`,
  `enable_ai_command_search_hash_trigger` `true`,
  `enable_slash_commands_in_terminal` `true`, `show_hint_text` `true`;
  history `include_agent_commands_in_history` `false`.
- The `#` trigger and alias expansion being off by default, while
  Warp's own completions and the ghost are on, is the shipped balance
  DropTerm's spec assumed.

### 8. Show/hide around foreground programs: an app-level editor with explicit forwarding

- Warp's editor is not a shell-line-discipline replacement; it is a
  pane-level element that always renders (default `input_mode`
  `"pinned_to_bottom"`; blocks appear "directly above the input editor",
  block-basics page). "Standing aside" is not a binary hide: at a live
  prompt the editor owns the keys; while a command runs, the terminal
  output area is the interaction surface and specific keys are forwarded
  to the running program (finding 3's PageUp/PageDown/Home/End rule,
  plus the mouse/scroll/focus reporting settings).
- What makes a prompt "live" is shell integration: blocks are created
  from the shell's in-band reports (OSC 9280 native completions and the
  bootstrap scripts' prompt hooks —
  `crates/warp_terminal/src/model/ansi/mod.rs:72-77, 1176-1245`,
  `app/assets/bundled/bootstrap/zsh_body.sh:1560-1641`, depth record
  finding 6), and the completion trigger additionally gates on a live
  command-grid prompt (`app/src/terminal/input.rs:11280-11284`, shipped
  record finding 3). Without integration Warp cannot see prompts; with
  it, the editor is the prompt.
- DropTerm's own reality is the mirror image and its spec already says
  so: the PTY always exists, foreground children are observable via
  `tcgetpgrp` (`dropterm/src/core/terminal/engine.rs:392-413`), OSC 9280
  is the planned prompt-truth source (shell-integration spec), and the
  rich-input ruling is "unsure prompt hides" (SPEC.md rich-input row) —
  strictly safer than Warp's always-rendered editor.

### 9. Execution: Enter runs the buffer, the block replaces the echo

- Enter is two-stage at a showing menu (accept-else-execute when the
  buffer equals the selection, `app/src/terminal/input.rs:13829-13870`)
  and the execute path is `ConfirmAndExecuteSuggestion` — insert, close,
  run (`input.rs:9081-9110`) — otherwise plain Enter runs the buffer.
  The command that appears in the created block is the buffer's content:
  blocks "group your command and command output" and appear above the
  input editor (block-basics page), so Warp renders the typed line from
  its own editor state and the shell's echo is superseded by the block
  rendering.
- The line reaches the shell through Warp's PTY writer (the
  `app/src/terminal/writeable_pty/` tree in the terminal listing); the
  exact write call is not line-pinned here — see **Unverified**. The
  architecture is confirmed by the in-band model everywhere else: shell
  history read "directly from disk or via an in-band command"
  (`app/src/terminal/history.rs`, `init_session` doc comment, verified
  in the blob), native completions riding OSC 9280 (finding 8).
- DropTerm's ruling diverges deliberately: rich-input Enter writes the
  line to the real PTY (SPEC.md rich-input row, "PTY-write, no 9280
  dependency"), so the shell keeps its own line discipline, echo, and
  prompt — the editor is a takeover of input, not of rendering.

## What DropTerm has today

Verified against `dropterm/src` at the working tree (2026-11-25):

| Behavior | Warp (f8223ce + docs) | DropTerm today |
|---|---|---|
| Editing core | full multi-line editor, undo/redo, kill/yank, word motions | none — every key translates straight to PTY bytes (`dropterm/src/terminal/input.rs:1-14`, the translate doc comment; `TermAction` set `:24-77`) |
| Key routing | app-level editor owns keys; explicit forwards during programs | `on_key` → menu routing → translate → bytes (`dropterm/src/terminal/view.rs:1239-1370`) |
| History Up/Down | Up opens inline history menu over merged store | none — no history recall; the store exists (`history.ron`, `autosuggestions_history_context`, `dropterm/src/core/config.rs:548-560`) and feeds ghost/menu only |
| Signals | editor consumes C/D/Z/L as editor ops | all forwarded as bytes; the signal trio is unwritten |
| Paste | verbatim into buffer, Enter-only runs | PTY paste path, bracketed by the emulator (unchanged under native mode) |
| Ghost | editor-native suffix, manual-menu suppression, Right/Alt+Right/Ctrl+F accepts | five-provider ghost, Right/Alt+Right accepts (`dropterm/src/terminal/suggest.rs:121-134`), same suppression shape (shipped record finding 6) |
| Menu keys | Tab LCP/cycle, wrap, two-stage Enter, Escape | landed (suggest-parity; SP-D10 rules Tab/Shift+Tab shell keys in every state, `dropterm/src/terminal/suggest.rs:27-111`) |
| Highlighting | live token colors + unknown-command underline, both default on | none |
| Live-prompt truth | shell integration blocks + completion gate (`input.rs:11280-11284`) | `tcgetpgrp` foreground check (`engine.rs:392-413`, `422-441`), OSC 9280 planned (shell-integration spec) |
| Config keys | the finding-7 table, defaults as listed | the `autosuggestions_*` family with defaults (`dropterm/src/core/config.rs:494-600`, `622-690`); rich-input adds `input_mode` per SPEC.md |

## Recommendation

Build rich-input v1 exactly as specified (SPEC-rich-input.md), with the
Warp findings grading each piece:

- **Editing core** — copy Warp's surface wholesale: insert, Backspace/
  Delete/Ctrl+H, Left/Right, Home/End/Ctrl+A/Ctrl+E, word motions
  (Alt+Left/Right on Linux), word kills Ctrl+W/Alt+D, line kills Ctrl+U/
  Ctrl+K, Ctrl+L clear, newline keys Shift+Enter/Ctrl+J, undo/redo.
  Skip Meta+. (insert-last-word) from v1; Ctrl+R history search stays v2
  per the spec. Warp proves every one of these is editor-local with no
  shell dependency.
- **History Up/Down over DropTerm's own store** — Up walks the store
  most-recent-first with Down returning; Warp's inline menu (filter tabs,
  per-item hide) is v2 chrome. Our store already carries cwd context
  (SP-D2), which is more than Warp exposes on the Up lane.
- **Signal trio** — Ctrl+C clears the owned line (and with an empty line
  forwards the ^C byte — our ruling, since the PTY is ours), Ctrl+D
  EOF on empty / forward-delete on non-empty, Ctrl+Z forwards. Warp's
  documented editor actions (finding 3) are the behavioral target; our
  PTY-write architecture makes the empty-line cases honest where Warp's
  docs stay silent.
- **Multi-line paste** — insert verbatim, only Enter runs, no
  confirmation modal (Warp ships none). This is the spec's exact ruling,
  now Warp-grounded.

**Rejected alternatives:**

- **Copy Warp's always-visible editor** — our drop-down's native mode is
  the bare shell and the spec's "unsure prompt hides" is safer than an
  editor that renders over prompts we cannot verify; keep the takeover
  conditional on prompt truth.
- **Adopt `completions_open_while_typing = false` because Warp ships
  it** — Warp's default is false, but DropTerm's
  `autosuggestions_menu_auto` already ships true by the suggest-parity
  record's exact-Warp directive (OQ-4, owner-overrulable); the record
  settles the factual question (the prior record's Unverified flag)
  without changing our default.
- **Undo/redo over PTY-adjacent state** — undo applies to the editor
  buffer only; never attempt to undo a committed (written) line. Warp's
  undo is buffer-scoped and so is ours.
- **Bracketed-paste machinery in the editor** — while owned, paste is a
  buffer insert; bracketed paste belongs to the native-mode PTY path we
  already have.
- **Copying editor code** — `app/src/editor/` and `crates/vim` are
  AGPL; behavior only, as in both prior records.

## Sources

- `github.com/warpdotdev/warp` @ `f8223ce` (committed 2026-11-20) — code
  citations carried from the shipped records' pinned sweep plus direct
  verification this session of `app/src/terminal/history.rs`,
  `app/src/terminal/input/` (tree listing: `terminal.rs`, `classic.rs`,
  `common.rs`, `inline_history/`, `suggestions_mode_*.rs`),
  `app/src/terminal/alt_screen_reporting.rs` (raw fetch), `keys.rs`,
  `keys_settings.rs`, `input.rs` (17,492 lines; regions pinned via the
  shipped records), `crates/settings` + workspace listing, and the file's
  commit history (`8f4865a` last touched it before `f8223ce`).
- Warp first-party docs, fetched 2026-11-25:
  [keyboard-shortcuts](https://docs.warp.dev/getting-started/keyboard-shortcuts)
  (the input-editor chord table),
  [editor](https://docs.warp.dev/terminal/editor) (multi-line editing,
  newline keys, quote auto-close),
  [syntax-error-highlighting](https://docs.warp.dev/terminal/editor/syntax-error-highlighting)
  (token classes, defaults),
  [block-basics](https://docs.warp.dev/terminal/blocks/block-basics)
  (blocks, forwarding during programs),
  [all-settings](https://docs.warp.dev/terminal/settings/all-settings)
  (every default quoted in finding 7),
  [vim](https://docs.warp.dev/terminal/editor/vim), changelog 2025/2026
  (up-arrow history menu), commit history pages.
- DropTerm working tree (2026-11-25): `dropterm/src/terminal/view.rs`,
  `dropterm/src/terminal/input.rs`, `dropterm/src/terminal/suggest.rs`,
  `dropterm/src/core/terminal/engine.rs`,
  `dropterm/src/core/terminal/suggest.rs`, `dropterm/src/core/config.rs`,
  SPEC.md, SPEC-rich-input.md.
- The shipped records
  [warp-completion-behavior](warp-completion-behavior.md) and
  [warp-completion-depth](warp-completion-depth.md) — same-rev code pins
  carried over; never contradicted, only extended.

**Unverified:**

- **Empty-buffer Ctrl+C in Warp** — whether an empty editor forwards the
  SIGINT byte rather than clearing nothing; no docs row and no code pin
  (the Ctrl+C pins cover the menu-open case, `input.rs:11584-11586`).
  DropTerm rules the forward explicitly (recommendation), so this gap
  does not block v1.
- **Ctrl+D EOF-on-empty in Warp** — the documented action is plain
  forward-delete; no empty-line EOF statement found in docs or pinned
  code. Same disposition as above.
- **The final PTY write call** — the writeable-pty mechanism is confirmed
  structurally (tree, in-band architecture) but the exact write-site
  line in `input.rs` was not pinned; `input.rs` exceeds every render
  path reachable without a local clone (the prior sweeps'
  `/tmp/warp-src` checkout is gone from this host, and GitHub serves no
  sub-file line windows for a 744 KB blob).
- **Up-at-prompt gating (empty vs non-empty buffer)** — Warp's exact
  rule for when Up opens the menu vs walks recall is in the inline
  history machinery, unpinned; our v1 rules plain recall either way.
- **Docs-vs-f8223ce drift** — the chord table and settings defaults come
  from the live docs (fetched 2026-11-25); `f8223ce` is five days older.
  No contradiction appeared against any pinned code, but a chord could
  have moved.
