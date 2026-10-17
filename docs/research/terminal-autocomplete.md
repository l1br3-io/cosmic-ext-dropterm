# Research: what is the best industry approach to add autocomplete to DropTerm's terminal?

Date: 2026-10-17
Question: *Warp-style autocomplete and its peers come in three architectural families —
shell-side ghost text (fish), terminal-side overlays over a PTY grid (VS Code,
iTerm2, Windows Terminal), and replacing the input surface entirely (Warp, Fig).
Which of these should DropTerm adopt, given a custom iced widget over
alacritty_terminal and the standing no-shell-injection verdict of
[terminal-de-integration](terminal-de-integration.md)?*

## TL;DR

**DropTerm should ship fish-style inline ghost-text autosuggestions computed
entirely terminal-side — history-prefix suggestions rendered as a dim overlay
after the cursor, accepted by writing the remaining suffix to the existing PTY
writer on Right (full) and Alt+Right (one word) — because it is the only design
that every living peer converges on, needs zero shell cooperation, zero OSC
engine work, and zero new dependencies.** The prompt state DropTerm needs is
already in the engine: the `tcgetpgrp` foreground check that powers
close-confirmation and the status band (`src/terminal/engine.rs:244-265`)
reads "the shell itself holds the terminal foreground" — which is precisely
"the user is sitting at a prompt" — for any shell, with no injection and no
OSC 133. Suggestions come from per-tab command history scraped out of the grid
at command-finish, the same screen-scraping the URL highlighter already does;
the ghost text draws as an overlay in the widget's `draw()` (the cursor quad
and IME preedit are the existing not-in-the-grid precedents), never written
into the grid; and acceptance synthesizes plain bytes through
`Terminal::input_no_scroll` (`src/terminal/engine.rs:647-649`), the exact
path every keystroke already takes. A popup menu and any OSC 133 prompt-mark
engine work stay out of phase 1.

Three premises from the task landscape itself were corrected by primary
sources and are load-bearing for the verdict:

1. **Ghostty does not have a terminal-side completions feature.** As of
   Ghostty 1.3.1 (the current release, Oct 2026) there is no
   `completions-*` config key anywhere in `src/config/Config.zig`, the
   shell-integration feature list is exactly
   `cursor/sudo/title/ssh-env/ssh-terminfo/path`, the 1.2.0 release notes'
   shell-integration headline is SSH compatibility (not completions), and the
   only "completions" in the 1.3.0 notes is *"fish: Add descriptions to fish
   shell completions for Ghostty"* — argument completion for the `ghostty`
   CLI binary itself. The "Ghostty 1.2 shipped Warp-style completions" claim
   that circulates in AI-generated surveys does not exist. DropTerm therefore
   cannot cite Ghostty as precedent; the live terminal-side precedents are VS
   Code, iTerm2, and Windows Terminal.
2. **kitty's `no-complete` shell-integration feature is also CLI completion
   for kitty itself** — *"Turn off completion for the kitty command."*
   ([kitty shell-integration docs](https://sw.kovidgoyal.net/kitty/shell-integration/))
   — not prompt autosuggestions.
3. **No shipping terminal has the shell send completion candidates.** Not
   even VS Code: its OSC 633 scripts report prompt/command-line/cwd state
   only (`633;E` is the command line, not candidates); the suggest popup's
   items come from VS Code's own providers and a built-in spec library. The
   "shell-emits-candidates protocol" is an experiment, not an industry
   approach.

The recommendation works strictly within the standing verdict of
[terminal-de-integration](terminal-de-integration.md) (2026-10-02), which
rejected shipping and injecting shell scripts; nothing here needs that
verdict revisited, so this record extends rather than supersedes it.

## What DropTerm has today

Grounded in the working tree (~29k lines `src/`):

- **Engine**: the grid is `alacritty_terminal 0.25.1`'s `Term` behind a
  `FairMutex` (`src/terminal/engine.rs:350`), fed by the crate's own
  `EventLoop` — the PTY reader thread runs
  `state.parser.advance(&mut **terminal, &buf[..unprocessed])` inside
  alacritty_terminal (`src/event_loop.rs:154` @ 0.25.1), so DropTerm never
  sees the raw byte stream. All writes to the PTY go through one funnel:
  `Terminal::input_no_scroll` → `self.notifier.notify(input)`
  (`src/terminal/engine.rs:647-649`), with `input_scroll`
  (`src/terminal/engine.rs:681-684`) as the scroll-to-bottom wrapper and
  the bracketed-paste-aware `paste` (`src/terminal/engine.rs:686-705`) as
  the existing "synthesize a byte sequence into the shell" precedent.
- **Prompt-state knowledge already in the engine, no OSC involved**: the
  dup'd master fd is captured at construction (`src/terminal/engine.rs:486`)
  and `foreground_child_running()` (`src/terminal/engine.rs:244-265,669-679`)
  compares `tcgetpgrp(master)` against the recorded shell's process group —
  false means *the shell itself owns the foreground*, i.e. the user is at an
  idle prompt. The live-pty unit test pins the whole lifecycle: idle →
  `sleep 30` takes the foreground → SIGINT returns it
  (`src/terminal/engine.rs:1600-1646`). `foreground_process_name()`
  (`src/terminal/engine.rs:274-300`) already feeds the status band from the
  same check.
- **Grid reading**: `Terminal::update()` rebuilds the cosmic-text `Buffer`
  from `term.grid().display_iter()` every damage
  (`src/terminal/engine.rs:1041-1291`, iteration at `:1081-1082`); the URL
  highlighter already regex-scrapes visible grid content through
  `visible_regex_match_iter` (`src/terminal/engine.rs:118-122,1381-1400`).
  So "read the text between prompt and cursor" is a solved shape in this
  codebase.
- **Draw path**: `TerminalBox::draw` (`src/terminal/widget.rs:558`) renders
  cell backgrounds and underline decorations from the buffer's layout runs
  (`src/terminal/widget.rs:676-889`), then the glyphs via
  `renderer.fill_raw(Raw { buffer, … })` (`src/terminal/widget.rs:895-900`),
  then the scrollbar, then the **cursor quad drawn as a pure overlay after
  the text** (`src/terminal/widget.rs:988-1065`), and IME preedit is another
  overlay outside the grid (`src/terminal/widget.rs:380-…,1425-1445`).
  Overlays that never touch the grid are an established pattern here.
- **Key routing**: named keys are mapped to escape codes in the widget —
  Right arrow sends `csi2("C", mod_no)` or `ss3("C", mod_no)` in app-cursor
  mode (`src/terminal/widget.rs:1195-1200`), Tab sends `\x09`
  (`src/terminal/widget.rs:1278-1282`) — then `terminal.input_scroll(...)`
  (`src/terminal/widget.rs:1243-1244`). App-level keybinds are checked and
  swallowed *before* the shell sees the key
  (`src/terminal/widget.rs:1154-1159,1350-1356`); the map is a defaults +
  per-action custom overlay (`src/keybinds.rs:39-141,319-339`) with stable
  action ids. Right/Alt-Right are **not** bound by any default action.
- **Event funnel**: the engine's `TaggedEventProxy`
  (`src/terminal/engine.rs:184-189`) forwards `alacritty_terminal` events
  into the app (`src/app.rs:83-84`), handled in
  `App::handle_term_event` (`src/app/tab_actions.rs:1042`) — `Wakeup` sets
  `needs_update` (`src/app/tab_actions.rs:1140-1154`), `PtyWrite` writes
  bytes back into the PTY (`src/app/tab_actions.rs:1097-1103`).
- **OSC handling**: unknown OSCs are dropped inside the vendored `vte`
  parser, not by DropTerm. `vte 0.15.0`'s `osc_dispatch`
  (`src/ansi.rs:1329` @ 0.15.0) handles exactly OSC 0/2 (title), 4 (color
  set), 8 (hyperlink), 10/11/12 (dynamic colors), 22 (cursor icon), 50
  (cursor shape), 52 (clipboard), 104/110/111/112 (color resets); every
  other OSC falls into `_ => unhandled(params)` at `src/ansi.rs:1523`,
  which only emits a `debug!` log. The `Handler` trait
  (`vte src/ansi.rs:495`) has named callbacks only — there is no
  unknown-OSC hook, and `alacritty_terminal` re-exports `vte`
  (`src/lib.rs:20`) but its event loop constructs the parser itself. Seeing
  OSC 133 would require forking/patching `vte`'s `ansi.rs` (via a cargo
  `[patch]`) or `alacritty_terminal` — exactly the "engine work" the 2026-10-02
  record priced.
- **No autocomplete anywhere**: no config key, no suggestion state, no
  overlay machinery beyond cursor/preedit; the only match for
  suggest/complet in the terminal-completion sense is the Tab-key comment
  in `src/keybinds.rs:60-66` ("Bare Ctrl+I must stay unbound … so
  swallowing it would break tab completion" — the project has already
  reasoned once about not stealing completion keys from the shell).
  cosmic-term, the closest sibling on the same engine, is equally bare (no
  autocomplete feature or open issue).

## Findings

### 1. fish autosuggestions — the UX everything else copies

fish is the origin of the interaction every later implementation imitates
([fishshell.com, "Autosuggestions"](https://fishshell.com/docs/current/interactive.html),
read 2026-10-17):

- **Source**: fish *"suggests commands as you type, based on command history,
  completions, and valid file paths"* — a merged, ranked source set, with
  history the backbone.
- **Presentation**: suggestions appear *"after the cursor, in a muted gray
  color"* (tunable via `fish_color_autosuggestion`) — inline ghost text, not
  a menu.
- **Acceptance**: Right arrow or Ctrl+F accepts the whole suggestion;
  *"To accept the first suggested word, press alt-right (→) or alt-f"*;
  Ctrl+Right accepts *"one token of the autosuggestion - the part they'd
  move over"*; and *"it won't execute unless you accept it"* — the
  safety property that makes ghost text trustworthy: ignore it and it is
  inert.
- **Opt-out**: `set -g fish_autosuggestion_enabled 0`.

The terminal's entire role is rendering dim text after the cursor correctly.
Everything else — knowing the prompt, the input, the history — lives in the
shell. Any terminal-side clone is re-implementing this UX over a PTY it
cannot natively understand, which is the central difficulty of family 2.

### 2. zsh-autosuggestions — proof the UX is shell-agnostic and that terminals need do nothing

[zsh-users/zsh-autosuggestions](https://github.com/zsh-users/zsh-autosuggestions)
(MIT; README @ master, read 2026-10-17) ports the fish UX to zsh: *"you will
see a completion offered after the cursor in a muted gray color"*, default
style `fg=8` from the 256-color palette. Strategies feed the suggestion —
`history` ("Chooses the most recent match from history."), `completion`,
`match_prev_cmd` — tried in order until one yields. Acceptance is →/End at
buffer end; *"If you invoke the forward-word widget, it will partially accept
the suggestion up to the point that the cursor moves to."* The plugin works
by hooking zle widgets, and needs no terminal support at all beyond 8/256
colors — the one documented rendering caveat is an iTerm2 color-scheme issue
where "Bright Black" equals "Background", making ghost text invisible.

Two lessons for DropTerm: (a) the fish UX is what users expect regardless of
shell — zsh users install a plugin to get it; (b) any DropTerm-side ghost
text must be visually distinguishable from the background *by construction*
(theme-derived dim color, not a hardcoded 8), or it will reproduce the exact
bug zsh-autosuggestions documents.

### 3. Ghostty — the claimed precedent does not exist (corrected result)

The claim "Ghostty 1.2 (Sept 2025) shipped terminal-side fish-style
Completions" fails at every primary source, checked 2026-10-17:

- **Config surface**: `src/config/Config.zig` @ `main` (11,385 lines,
  fetched from
  [raw.githubusercontent.com/ghostty-org/ghostty/main/src/config/Config.zig](https://raw.githubusercontent.com/ghostty-org/ghostty/main/src/config/Config.zig))
  contains zero occurrences of "completions"/"suggestion"/"autocomplete".
  The `shell-integration-features` doc block enumerates exactly `cursor`,
  `sudo`, `title`, `ssh-env` (1.2.0), `ssh-terminfo` (1.2.0), `path` — no
  completions feature.
- **1.2.0 release notes**
  ([ghostty.org/docs/install/release-notes/1-2-0](https://ghostty.org/docs/install/release-notes/1-2-0)):
  the shell-integration headline is *"Ghostty now has opt-in shell
  integration features to make Ghostty more compatible with SSH"* (`ssh-env`,
  `ssh-terminfo`). No completions feature is mentioned anywhere.
- **1.3.0 release notes**
  ([ghostty.org/docs/install/release-notes/1-3-0](https://ghostty.org/docs/install/release-notes/1-3-0)):
  headline features are Scrollback Search, Scrollbars, Click to Move Cursor
  In Shell Prompts, Command Finished Notifications, key tables, Unicode
  rendering — no completions. The single "completions" mention is *"fish:
  Add descriptions to fish shell completions for Ghostty"* — argument
  completion for the `ghostty` CLI (issue
  [ghostty-org/ghostty#9531](https://github.com/ghostty-org/ghostty/issues/9531)
  is about exactly that).
- **Repo state**: latest tag `v1.3.1`; issue searches for
  autosuggest/fish-style/inline suggestions surface no such feature or
  tracking issue.

What Ghostty actually demonstrates is the *adjacent* pattern: OSC 133 prompt
marks enable `jump_to_prompt` and 1.3's click-to-move-cursor, and
`notify-on-command-finish` builds on marks — command *awareness* without
command *completion*. A completions feature may yet arrive (Mitchell
Hashimoto has publicly admired fish's UX), but as of this record it is not a
shipped precedent anyone can grade against. The correct reading for DropTerm
is that even the most actively developed new terminal has shipped prompt
marks, search, and notifications — but not autocomplete — which says
something about the difficulty-to-value ratio of terminal-side completion.

### 4. VS Code terminal suggest — the richest terminal-side implementation, and it does *not* have the shell send candidates

VS Code is the deepest terminal-side implementation shipping today
([code.visualstudio.com/docs/terminal/shell-integration](https://code.visualstudio.com/docs/terminal/shell-integration),
read 2026-10-17; source @ `microsoft/vscode` `main`, read 2026-10-17):

- **UX**: *"IntelliSense in the terminal enables you to receive suggestions
  for files, folders, commands, command arguments and options"* — an
  IntelliSense-style **popup** (*"As you type, a list of suggestions will
  appear"*; Ctrl+Space triggers manually; *"By default, Tab inserts the
  suggestion. Once you navigate the list, Enter inserts the suggestion."*).
  Inline ghost text exists only as an *integration with shell-provided*
  ghost text: the `terminal.integrated.suggest.inlineSuggestion` setting
  controls how to *"integrate with shell 'ghost text' and how to present
  it"* — pwsh's PredictiveIntelliSense, zsh-autosuggestions, fish.
- **The protocol**: shell integration scripts (auto-injected for bash, zsh,
  fish, pwsh) emit OSC 633 sequences — `terminalEscapeSequences.ts` @ main
  (last commit `6ec31b78`) documents the full set: `A` PromptStart, `B`
  CommandStart, `C` CommandExecuted, `D [;ExitCode]` CommandFinished, `E`
  CommandLine, `F/G` continuations, `H/I` right prompt, `P` properties
  (`Cwd`, `Task`, `HasRichCommandDetection`). 633 was chosen over 133
  deliberately: *"The decision to move to entirely custom sequences was to
  try to improve reliability and prevent the possibility of applications
  confusing the terminal."* The `E` sequence — *"Explicitly set the command
  line with an optional nonce"* — is what gives VS Code an exact prompt
  input model; the nonce exists "to prevent command spoofing".
- **The scripts send state, not candidates**: `shellIntegration-rc.zsh` @
  main emits `633;A`, `633;B`, `633;P;Cwd=…`, `633;E;commandline;nonce`,
  `633;C/D` — grep for "complet" finds nothing. Suggest candidates are
  computed **by VS Code**: built-in providers (executables from PATH,
  files/folders under the cwd) plus the built-in `extensions/terminal-suggest`
  extension, which ships Fig-style completion specs for git, gh, npm, npx,
  pnpm, azd, code, cd, … (`extensions/terminal-suggest/src/completions/*.ts`
  @ main). The `terminalSuggestAddon` (`terminalContrib/suggest/browser/terminalSuggestAddon.ts`
  @ main, last commit `17b7f793`, 2026-06-29) tracks the prompt via a
  `PromptInputModel` fed by those sequences — including a `ghostTextIndex`
  so the shell's own ghost text is detected, trimmed before filtering
  (*"Add any 'ghost text' suggestion suggested by the shell"* is surfaced as
  one completion item, with the comment *"Right arrow is used to accept the
  completion. This is a common keybinding in pwsh, zsh"*).
- **Acceptance synthesizes keystrokes**: on accept, the addon builds a byte
  sequence — `'\x7F'.repeat(replacement minus common prefix)` (backspaces),
  `'\x1b[3~'.repeat(...)` (deletes), the completion suffix, optional `'\r'`
  — and sends it into the terminal input. This is the PTY-synthesis model
  DropTerm would use, already proven at scale.
- **Rollout discipline**: suggest shipped experimental in 1.89 (2024) and
  was opt-in through 1.93 (Aug 2024); the November 2025 v1.107 release notes
  state *"Terminal Suggest is now enabled for stable users"* — eighteen
  months of iteration before default-on.

### 5. iTerm2 — shell integration for state, a popup for completion

([iterm2.com/documentation-one-page.html](https://iterm2.com/documentation-one-page.html),
read 2026-10-17.) iTerm2's Cmd+; **Autocomplete** is a popup over the PTY
grid: *"Any text that exists in a tab or its scrollback buffer can be
autocompleted in that tab"* — type the beginning of a word, press Cmd+;,
*"An autocomplete window opens showing the top 20 choices"*; Return enters
the choice, Tab extends it. The candidate source is striking in its
simplicity: everything on screen (scrollback included), not history files.
Separately, **Shell Integration** (a script iTerm2 can auto-load —
*"available for zsh, bash, and fish"* — that *"modifies your prompt so
iTerm2 knows where it is"*) provides the state layer: navigate between
prompts, *"record your command history, suggest most used directories,
helps you re-run commands"*. iTerm2's AI composer has inline suggestion of
a different kind (*"what you have entered and relevant files or commands
are sent to AI so it can make suggested completions"*) — explicitly off by
default for privacy. iTerm2 therefore demonstrates the two-layer split —
shell integration for prompt/command state, terminal UI for the completion
surface — with the least sophisticated candidate engine (screen text) and a
manual-invocation popup rather than always-on ghost text.

### 6. Windows Terminal — a suggestions flyout, deliberately not ghost text

Windows Terminal's Suggestions UI (introduced Preview 1.19, expanded 1.21/1.22)
is an extension point, not an autocomplete engine
([learn.microsoft.com — Terminal actions, "Suggestions"](https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions),
updated 2025-08-21; [1.22 preview release post](https://devblogs.microsoft.com/commandline/windows-terminal-preview-1-22-release/),
read 2026-10-17):

- The `showSuggestions` action opens a palette-like flyout (*"behaves much
  like the command palette"*); `source` selects `"recentCommands"` —
  *"powered by shell integration, so they will only be available if you
  have your shell configured to support shell integration"* — and/or
  `"tasks"` (named `sendInput` snippets); `useCommandline: true`
  pre-populates the filter box from the current command line.
- 1.22 added *"Ctrl+Shift+. will now open the Suggestions UI which contains
  Snippets and Suggestions"* and the telling fix: *"The Suggestions UI will
  no longer try to grab the PowerShell 'ghost text' out of your input
  line"* — WT tried to harvest pwsh's inline ghost text as a suggestion
  source and backed it out, where VS Code kept it as an opt-in integration.
  On-demand flyout + never-touch-ghost-text is a coherent minimalist
  position, and the docs still flag the feature as Preview-only.

For DropTerm the precedent is: a terminal can ship completion-adjacent UX
with zero prompt heuristics by being explicitly on-demand — but WT's
recentCommands still leans on shell integration for its data, which is the
part DropTerm's verdict rules out.

### 7. kitty's `complete` — CLI completion for kitty itself (corrected result)

kitty's shell-integration feature list includes `no-complete`, verified
against
[sw.kovidgoyal.net/kitty/shell-integration](https://sw.kovidgoyal.net/kitty/shell-integration/)
(read 2026-10-17): *"Turn off completion for the kitty command."* — the
feature registers *"Sophisticated completion for the kitty command in the
shell"* (kitty/kitten/clone-in-kitty; *"for the fish shell this does not
take effect, since fish already comes with a kitty completion script"*).
It is argument completion for kitty's own binary, identical in kind to
Ghostty's fish/zsh completions. kitty has no prompt autosuggestion or
completion overlay feature; like WezTerm it leaves that entirely to the
shell (fish, or plugins). Any survey that lists kitty's `complete` among
"terminal autocomplete integrations" is misreading it.

### 8. Alacritty — the engine DropTerm embeds has a stated position, and it is not "add terminal features"

Alacritty remains at v0.17.0 (2026-04-06) with no autocomplete feature; its
one "Add Support for AI Autocomplete" issue is closed unanswered. The
maintainer position on the shell-integration family is on record in
[alacritty/alacritty#5554](https://github.com/alacritty/alacritty/issues/5554)
(closed): *"Many of these things are just patches to the shell which has
nothing to do with Alacritty. The whole concept of 'shell integration' is
pretty vague. Alacritty communicates with the program running inside it
through escape sequences and those are either supported or they aren't. If
there is a specific escape sequence you're missing, please mention that one
specifically. Though some are just out of scope for the project."*

Two implications for DropTerm. Upstream will not add OSC 133 support for
DropTerm to inherit (no specific-sequence request for it has been accepted),
and DropTerm should not expect the vendored parser to grow an unknown-OSC
hook. And philosophically, DropTerm's standing verdict — escape sequences
either supported or not, no script shipping — is the same line Alacritty's
maintainer draws; DropTerm diverges only in choosing to *detect* shell state
out-of-band (tcgetpgrp, /proc) rather than not at all.

### 9. Warp — replacing the input surface buys the best completions at the cost of no longer being a terminal-first terminal

([docs.warp.dev](https://docs.warp.dev), read 2026-10-17.) Warp's input line
is a native text editor — the "modern text editing" feature set (multi-cursor
selection, alias expansion, command inspector, syntax and error highlighting,
Vim keybindings) exists because typing does not go through a PTY grid at
all; commands run on Execute and render as blocks (*"Blocks group your
command and command output"*, per the docs' Block basics cited in
[status-bar-ux](status-bar-ux.md)). On that editor sit two completion
systems:

- **Autosuggestions** (always-on, toggle from the Command Palette): *"Warp
  will automatically suggest commands as you type based on shell history and
  possible completions"* — inline ghost text; accept with →/Ctrl+F, partial
  with Ctrl+→ (macOS) / Ctrl+Shift+→ (Windows/Linux); acceptance key
  remappable to Tab.
- **Tab completions**: a spec-driven popup menu ("fuzzy search capability
  … approximate matches"), with a supported-specs table spanning hundreds of
  commands (git, docker, npm, kubectl full; most others "Partial") and
  shell-alias resolution into the underlying spec.

Correction to the circulating claim: Warp's current docs make **no** AI,
local-model, or cloud claims on the autosuggestions page — the "AI
Autocomplete (local ML model, cloud fallback)" description dates from
2023-24 marketing and does not match today's documented behavior
(history + completions). Warp's AI surfaces now live in its agent features,
which do involve cloud models (and community pushback about session data
leaving the machine). The costs of the paradigm are structural: because the
editor is native, TUI apps, SSH sessions, and non-Warp-aware shells all
need accommodation machinery Warp has spent years building (per-shell
integration, shells wired into the editor model), and Warp 2.0 (2025)
repositioned the product around agents — the input-surface bet was a
product pivot, not a feature. DropTerm, a drop-down for *quick* shell
access on COSMIC, has no reason to pay that price.

### 10. Fig / Amazon Q — the TTY-interception approach is a dead project

Fig's architecture was the third way: intercept keystrokes at the TTY level,
render an out-of-process GUI overlay, drive completions from a TypeScript
spec format. The approach is now doubly dead: the
[withfig/fig](https://github.com/withfig/fig) repository is **archived**
(description now "Public issue tracker for Fig."), the client stopped
working 2024-09-01 after the AWS acquisition, and AWS's successor
[amazon-q-developer-cli](https://github.com/aws/amazon-q-developer-cli)
README (read 2026-10-17) states: *"This open source project is no longer
being actively maintained and will only receive critical security fixes.
Amazon Q Developer CLI is now available as Kiro CLI, a closed-source
product."* AWS's end-of-support notice dates the IDE-plugin family's
discontinuation at 2027-04-30. The Fig spec format lives on only as an
input vocabulary (VS Code's terminal-suggest extension ships Fig-style
specs), not as a runtime. For DropTerm this is the cautionary precedent:
an architecture that fights the terminal (separate overlay process, TTY
wrappers) died even with a company behind it.

### 11. The negative peers — and one proprietary positive

- **WezTerm**: no built-in completion; shell integration is OSC 7 + OSC 133
  zones + OSC 1337 user vars
  ([wezterm.org/shell-integration](https://wezterm.org/shell-integration.html),
  read 2026-10-17). The only completion mention on that page is Clink, a
  shell-side tool for cmd.exe — WezTerm explicitly routes completion to the
  shell.
- **Rio**: shell integration is OSC 7 cwd reporting
  ([rioterm.com/docs/features/shell-integration](https://rioterm.com/docs/features/shell-integration));
  no autocomplete feature.
- **Contour**: recent work added *"OSC 133 prompt marks (A, C and D) to the
  bundled shell integration, so Contour can tell a prompt apart from a
  command's output"* (contour-terminal.org release notes) — command
  awareness only, no completion UI.
- **Termius** (proprietary, worth noting as the newest terminal-side
  implementer): Settings → Terminal → Autocomplete; *"Autocomplete in
  Termius uses data from many sources to suggest snippets, shell history
  commands, paths, and built-in commands"*, with shell integration to
  *"detect the current directory, command history, prompts, and active
  command state"* — and for SSH it opens extra `exec` channels beside the
  session to fetch paths/history
  ([docs.termius.com](https://docs.termius.com/terminal/autocomplete-and-shell-integration),
  read 2026-10-17). Termius pays, per host, in exactly the integration
  machinery DropTerm's verdict rejects.

## The three families, graded

| Family | Members | Prompt knowledge | Candidate source | UI | Cost profile |
|---|---|---|---|---|---|
| Shell-side ghost text | fish, zsh-autosuggestions, Clink | shell's own | shell (history/completions/paths) | inline dim text | zero terminal work; per-shell plugins; DropTerm would ship none of it |
| Terminal-side overlay | VS Code, iTerm2, Windows Terminal, Termius | shell scripts (OSC 633/133) or screen heuristics | terminal-computed (PATH/files/specs/screen text/history) | popup (all four) + inline passthrough (VS Code) | prompt detection is the hard problem; every member either ships shell scripts or accepts degraded detection |
| Input-surface replacement | Warp (alive), Fig/Amazon Q (dead) | owns the editor | history + specs + AI | inline + menu in a native editor | rebuild the terminal around it; TUI/SSH accommodation forever; product-pivot scale |

Every surviving terminal-side implementer converges on the same division of
labor: the *state layer* (where is the prompt, what is typed) comes from
either shell cooperation or out-of-band detection, and the *suggestion
layer* is computed terminal-side, accepted by synthesizing keystrokes.
None has the shell send candidates. DropTerm's unusual advantage is that its
engine already owns an out-of-band state signal — `tcgetpgrp` — that the
others don't use for this purpose (VS Code/iTerm2/WT run on OSes where a
terminal-side tcgetpgrp against the *child shell* is less straightforward,
and they want exact command lines anyway).

## Recommendation

**Phase 1 — fish-style history ghost text, terminal-side, opt-out.** Graded
against precedent: the interaction model is fish/zsh-autosuggestions/Warp's
autosuggestions (inline, dim, inert until accepted); the detection model is
DropTerm-specific and stronger than any heuristic peer (kernel truth, any
shell, SSH-transparent); the acceptance model is VS Code's (synthesize
keystrokes into the PTY); the rollout model is fish/Warp default-on with a
kill switch (VS Code's eighteen-month opt-in was about a *popup* over
unknown shells — DropTerm's inline text has a smaller blast radius, and
`fish_autosuggestion_enabled` defaults on).

1. **Prompt state — no OSC, no injection.** A suggestion may show only when
   `!terminal.foreground_child_running()` (`src/terminal/engine.rs:669-679`)
   *and* the cursor sits on the bottom grid line at display offset 0. The
   tcgetpgrp check is exactly "the shell owns the foreground," which is the
   idle-prompt state; it already drives close-confirmation and the status
   band, is Linux-native, and follows SSH transparently (the remote shell
   still holds the pgrp locally through the ssh client). This *strengthens*
   the standing verdict rather than testing it: detection without
   injection, no `TERM_PROGRAM` gating, no scripts shipped. Fail-closed by
   construction — a shell without job control degrades to "no suggestions,"
   never to wrong suggestions.
2. **Input tracking — read the grid, don't intercept keys.** On each
   `Wakeup`-driven `needs_update` cycle, scrape the cursor line from
   `term.grid()` (the `display_iter` walk `update()` already performs,
   `src/terminal/engine.rs:1081-1082`) from the line start to the cursor
   column; that string is the typed input for prefix matching. Cells left
   of the first non-prompt character is a heuristic phase 1 accepts: a
   suggestion that is wrong about the prompt *prefix* is still accepted as
   "append this suffix," so only the suffix must be exact. Multi-line and
   right prompts degrade to no-suggestion, the honest failure.
3. **History source — scrape at command-finish, per tab.** When the
   foreground returns to the shell (the same transition the engine's test
   pins, `src/terminal/engine.rs:1600-1646`), record the just-typed line
   into a bounded per-tab ring (fish and zsh both cap effective history;
   Warp indexes history; VS Code caches shell-history globals). This is the
   iTerm2 observation (screen text is a legitimate candidate source)
   without its popup, and the Warp autosuggestion source (history +
   completions → phase 1 keeps only history) without its editor. No history
   files are read; `~/.bash_history` parsing is a non-goal (VS Code does
   it; it buys cross-session coverage DropTerm's per-tab ring already
   covers for the drop-down's own usage pattern — session persistence
   already restores tabs).
4. **Match policy — prefix, most-recent-first.** Fish's backbone and
   zsh-autosuggestions' default `history` strategy: the most recent history
   entry with the typed line as a strict prefix. One suggestion, inline —
   the graded precedent is unanimous that inline is the always-on surface
   (fish, zsh-autosuggestions, Warp autosuggestions); every popup
   implementation (VS Code, iTerm2, WT) makes it on-demand because a
   persistent popup over an uncertain prompt is hostile. A menu is a later,
   separate decision with zero regression risk to the inline path.
5. **Rendering — overlay in `draw()`, never the grid.** After the
   `fill_raw` text pass (`src/terminal/widget.rs:895-900`), lay the
   suggestion's glyphs starting at the cell after the cursor, clipped at
   the line end, in a theme-derived dim foreground (composited from the
   scheme's foreground/background — never palette index 8, the
   zsh-autosuggestions invisible-text lesson). The cursor quad
   (`src/terminal/widget.rs:988-1065`) and IME preedit are the two
   existing overlays proving the pattern; the grid and the cosmic-text
   buffer stay untouched, so selection, search, copy, and scrollback see
   nothing. Wrapping: clip, don't wrap (a suggestion that would wrap is
   dropped — showing wrong-cell ghost text is worse than none).
6. **Acceptance — synthesize into the existing PTY funnel.** While a
   suggestion is displayed: Right (and only then — Right is otherwise
   forwarded verbatim, `src/terminal/widget.rs:1195-1200`) writes the
   remaining suffix through `terminal.input_no_scroll(suffix)`
   (`src/terminal/engine.rs:647-649`), the identical path `paste` and every
   keypress use; Alt+Right accepts one shell-word of it (fish's
   alt-right/ctrl-right family, zsh's forward-word, Warp's Ctrl+→). Any
   other key, focus loss, resize, or foreground change dismisses the
   suggestion first and then processes the key normally — fish's *"it
   won't execute unless you accept it"* invariant. No bracketed-paste
   wrapping on the suffix: it is typed characters, not a paste, matching
   how VS Code's addon emits raw suffix bytes.
7. **Config — `autosuggestions_enabled` (default true), pinned to the docs
   contract.** One serialized key, documented in `docs/configuration.md`
   (the `every_config_key_is_documented` gate), a Behavior-card toggle
   beside `bell_notification_enabled` (`src/config.rs:262`). Keybind
   surface: none in phase 1 — the accept keys are soft (only meaningful
   while a suggestion shows, never swallowed otherwise), which avoids
   spending `action_id` stability on keys whose default no peer customizes.
   If users ask, Right/Alt-Right can join the overlay later as the
   customization layer already models (`src/keybinds.rs:319-339`).
8. **Privacy — local-only, by construction.** No network, no model, no
   telemetry; suggestions are a prefix search over the tab's own visible
   past. The contrast is the survey's own negative space: Warp's agent
   features send session data to cloud models, iTerm2's AI composer is
   off by default for privacy, and Fig's spec network died with the
   product. DropTerm's stance costs nothing because phase 1's source
   (history) is the one thing that never needed a model.

**Phase 2 (conditional, not scheduled): prompt marks via OSC 133 — only if
phase 1's heuristics demonstrably fail.** The honest trigger would be
widespread wrong-suffix suggestions from multi-line/right prompts that the
prefix heuristic cannot see. The work is real and nameable: the vendored
`vte 0.15.0` parser drops unknown OSCs at `src/ansi.rs:1523` with no
extension hook, and `alacritty_terminal`'s event loop owns the parser
(`src/event_loop.rs:154`), so DropTerm would carry a `[patch]`-pinned fork
of `vte` (add an unknown-OSC callback to `osc_dispatch`) or of
`alacritty_terminal` — plus mark storage in/alongside the grid. That is
engine work the standing verdict already priced as high, for marginal gain
over the tcgetpgrp signal; it should not be pre-paid. If it ever lands, the
no-injection verdict still holds: marks are *accepted*, never injected —
the docs would carry a copy-paste OSC 133 snippet (the same sequences
WezTerm ships as `wezterm.sh` and VS Code as `633`), detection-only, for
users who opt in. No shipped scripts, no env injection, no supersession of
the 2026-10-02 record.

## Rejected, and why

- **Replacing the input surface (the Warp model).** The best completions in
  the industry live on a native editor, but the price is everything
  DropTerm is: a PTY-grid terminal where vim, ssh, and every TUI app just
  work. Warp spent years building per-shell integration and block
  machinery, then pivoted the product to agents; Fig and Amazon Q's
  terminal overlay died entirely. A drop-down terminal's value is
  immediacy, not editor-hood. Rejected as a paradigm, not a schedule.
- **Fig/Amazon Q TTY interception + out-of-process overlay.** Archived
  upstream (`withfig/fig` archived; Q CLI "no longer being actively
  maintained … Kiro CLI, a closed-source product"). Also architecturally
  absurd for DropTerm, which owns its own widget — there is nothing to
  overlay out-of-process. Dead precedent, rejected.
- **A shell-emits-candidates protocol (the OSC 633;E-as-completions
  idea).** No shipping terminal does this — VS Code's scripts send state
  (prompt, command line, cwd), and its candidates come from its own
  providers and spec library. Designing a DropTerm-private candidate
  protocol would have zero implementors and directly contradict the
  no-injection verdict (someone must ship the emitting scripts). Rejected;
  633 itself remains worth *accepting* someday for state, alongside 133.
- **Completion-spec menus in phase 1 (Warp/Fig/VS Code specs).** Even VS
  Code — with a dedicated built-in extension — needed a spec library
  curated per CLI (git "Full", most "Partial") and 18 months to
  default-on. A popup over DropTerm's grid plus a spec format is a
  project, not a feature, and its value arrives only after inline
  suggestions prove the prompt-detection layer. Deferred indefinitely, not
  adopted.
- **On-demand flyout only (the Windows Terminal position).** Viable and
  cheap, but WT's own data sources still lean on shell integration
  (`recentCommands` is *"powered by shell integration"*), and a flyout
  answers "what did I mean?" while ghost text answers "finish this for me"
  — the latter is the drop-down's question. Inline-first, flyout-later if
  ever.
- **Reading shell history files (`~/.bash_history`, zsh histfile).** VS
  Code does this for cross-session coverage; DropTerm's tabs already
  restore per session, history files are written lazily by shells (stale
  and interleaved), and the format zoo (with/without timestamps,
  HIST_IGNORE_DUPS semantics) is a correctness tax for marginal gain.
  Per-tab scraped history only.
- **OSC 133 engine fork now.** Pre-paying engine work (vte `[patch]` +
  event-loop surgery) for knowledge the kernel already hands DropTerm via
  tcgetpgrp. See phase 2 above.
- **Shell-integration auto-injection.** Unchanged from the standing
  verdict — *"a large, security-sensitive surface — shipping shell scripts
  for 5 shells, injecting env"*. This record's design was chosen
  specifically so that verdict never has to be revisited.

## Sources

- fish docs, "Autosuggestions" + "Shared bindings" —
  <https://fishshell.com/docs/current/interactive.html> (read 2026-10-17)
- zsh-users/zsh-autosuggestions README @ master (MIT) —
  <https://github.com/zsh-users/zsh-autosuggestions> (read 2026-10-17)
- Ghostty: `src/config/Config.zig` @ `main` (fetched 2026-10-17; no
  `completions-*` keys; feature list `cursor/sudo/title/ssh-env/ssh-terminfo/path`)
  <https://raw.githubusercontent.com/ghostty-org/ghostty/main/src/config/Config.zig>;
  release notes 1.2.0 / 1.3.0 —
  <https://ghostty.org/docs/install/release-notes/1-2-0>,
  <https://ghostty.org/docs/install/release-notes/1-3-0>;
  repo tags through `v1.3.1` (checked 2026-10-17);
  fish CLI-completions issue
  <https://github.com/ghostty-org/ghostty/issues/9531>
- VS Code: shell-integration docs —
  <https://code.visualstudio.com/docs/terminal/shell-integration> (read
  2026-10-17); `microsoft/vscode` @ `main` (read 2026-10-17):
  `src/vs/workbench/contrib/terminal/browser/terminalEscapeSequences.ts`
  (633 A–P enums; last commit `6ec31b78`),
  `src/vs/workbench/contrib/terminal/common/scripts/shellIntegration-rc.zsh`
  (emits `633;A/B/C/D/E`, `P;Cwd`, nonce; no completion candidates),
  `src/vs/workbench/contrib/terminalContrib/suggest/browser/terminalSuggestAddon.ts`
  (PromptInputModel, inline-ghost-text passthrough, keystroke-synthesis
  acceptance; last commit `17b7f793`, 2026-06-29),
  `extensions/terminal-suggest/src/completions/` (Fig-style specs: git, gh,
  npm, azd, code, cd, …); v1.107 release notes (Nov 2025, suggest
  default-on for stable) —
  <https://code.visualstudio.com/updates/v1_107>
- iTerm2 documentation (one-page, read 2026-10-17) — Autocomplete (Cmd+;),
  Shell Integration and Utilities, AI/Composer privacy note —
  <https://iterm2.com/documentation-one-page.html>
- Windows Terminal: actions docs "Suggestions" (learn.microsoft.com,
  updated 2025-08-21) —
  <https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions>;
  Preview 1.22 release post —
  <https://devblogs.microsoft.com/commandline/windows-terminal-preview-1-22-release/>
  (read 2026-10-17)
- kitty shell-integration docs (`no-complete`: "Turn off completion for the
  kitty command.") —
  <https://sw.kovidgoyal.net/kitty/shell-integration/> (read 2026-10-17)
- Alacritty: maintainer position in
  <https://github.com/alacritty/alacritty/issues/5554> (closed); releases
  through v0.17.0 (2026-04-06), issue search checked 2026-10-17
- Warp docs (read 2026-10-17): Autosuggestions —
  <https://docs.warp.dev/terminal/command-completions/autosuggestions>;
  Completions —
  <https://docs.warp.dev/terminal/command-completions/completions>;
  features index — <https://docs.warp.dev/features>
- Fig / Amazon Q: `withfig/fig` (archived) —
  <https://github.com/withfig/fig>; `aws/amazon-q-developer-cli` README @
  main (read 2026-10-17: "no longer being actively maintained … Kiro CLI,
  a closed-source product") —
  <https://github.com/aws/amazon-q-developer-cli>; AWS Q Developer
  end-of-support (2027-04-30) —
  <https://docs.aws.amazon.com/amazonq/latest/qdeveloper-ug/q-developer-ide-end-of-support.html>
- WezTerm shell integration (OSC 7/133/1337; Clink as shell-side) —
  <https://wezterm.org/shell-integration.html>; Rio shell integration —
  <https://rioterm.com/docs/features/shell-integration>; Contour release
  notes (OSC 133 marks) — <https://contour-terminal.org/> (all read
  2026-10-17); Termius Autocomplete & shell integration —
  <https://docs.termius.com/terminal/autocomplete-and-shell-integration>
- DropTerm local tree (read 2026-10-17): `src/terminal/engine.rs`,
  `src/terminal/widget.rs`, `src/app/tab_actions.rs`, `src/app.rs`,
  `src/keybinds.rs`, `src/config.rs`, `Cargo.toml`;
  `docs/research/terminal-de-integration.md` (the standing no-injection
  verdict, 2026-10-02)
- alacritty_terminal 0.25.1 (crates.io, local cargo registry):
  `src/event_loop.rs:154` (`state.parser.advance` — the event loop owns
  the parser), `src/lib.rs:20` (`pub use vte`);
  vte 0.15.0 (local cargo registry): `src/ansi.rs:1329` (`osc_dispatch`),
  `:1523` (`_ => unhandled(params)` catch-all), `:495` (`pub trait
  Handler` — no unknown-OSC hook)
