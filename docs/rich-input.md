# Rich input

DropTerm can own your prompt line the way [Warp](https://docs.warp.dev/terminal/editor) does - editing, highlighting, completion, and execution in DropTerm's own input strip - behind one setting. The default is `native`: the bare shell you have today, every keystroke forwarded verbatim, no ghost, no menu, no highlight.

This page covers the two modes, the keys the rich editor claims, and the settings. The completion menu's behavior is [Autosuggestions'](autosuggestions.md) contract restated inside the editor.

## The two modes

`input_mode` in [Configuration](configuration.md) (globally, or per profile through the profile field of the same name):

- **`native` (the default)** - the bare shell. Every keystroke reaches the PTY verbatim; the whole suggestion lane is dark. There is no v1 release yet, so this is a deliberate clean cut, not a regression: the old overlay surfaces retired when the editor shipped, and `native` is genuinely bare.
- **`rich`** - the editor. At a live prompt (focused, nothing running in the foreground, viewport at the bottom) DropTerm owns the line: it buffers your typing, edits it, highlights it, completes it, and runs it. While a program owns the screen (vim, ssh, cargo), or when DropTerm cannot decide whether the cursor sits at a real prompt, the editor stands aside and every key passes through - a hidden editor costs a little richness, a wrongly-shown one would eat a program's input.

Enter runs the line by writing it to the PTY with a single newline - the shell keeps its own line discipline, echo, and history. No shell integration is required.

## The keys the editor claims

Only while the line is owned (rich mode at a live prompt); everywhere else every key forwards as `native` sends it. Tab is the shell's key in the native lane by standing ruling - inside the editor it is restored to the menu (see below).

| Key | What it does |
|---|---|
| printable keys | insert at the caret |
| Backspace / Delete | delete left / right |
| Left / Right, Home / End | move (Ctrl+B/F, Ctrl+A/E are their twins) |
| Alt+Left / Alt+Right | move a word (Ctrl+Left/Right too) |
| Ctrl+W | kill the word left (Ctrl+Backspace too) |
| Alt+D | kill the word right (Ctrl+Delete too) |
| Ctrl+U / Ctrl+K | kill to the line's start / end |
| Shift+Enter, Ctrl+J | insert a newline - multi-line editing; a pasted block lands whole and only Enter runs |
| Up / Down | walk DropTerm's own command history (Ctrl+P/N their twins); your draft is preserved and comes back |
| Ctrl+Z / Ctrl+Shift+Z | undo / redo, buffer-scoped - a run line is never undoable |
| Ctrl+C | clear the owned line; on an empty line, interrupt |
| Ctrl+D | delete forward; on an empty buffer, EOF |
| Ctrl+L | clear the scrollback, the line standing |

The ghost and the completion menu behave as [Autosuggestions](autosuggestions.md) documents - the menu pops as you type from two characters, Right accepts the ghost whole and Alt+Right one word, Enter accepts a menu row and a second Enter on the equal line runs it - and inside the editor Tab fills the longest common prefix of the matching rows (closing on the last one standing) or cycles the selection; with the menu closed, Tab opens it when rows qualify, and forwards to the shell otherwise.

Completions see through `$(...)` substitutions, pipes, and `;` chains: the segment the cursor stands in is what completes.

## The strip and its docks

`input_position` (default `bottom`) moves the strip: `bottom` sits under the grid where the prompt is; `top` pins it at the head of the grid region - every Enter drops the executed command into the flow below while the grid keeps scrolling like a normal terminal. The menu re-anchors with the strip.

## The settings

The Settings window's Auto complete page carries every key (see [Configuration](configuration.md#auto-complete) for the full table): the mode and position, the ghost and menu toggles, the auto-popup, the syntax highlight, and the menu's chrome - descriptions, type badges, row icons, the argument hint line, each its own key, all default on, with `compact_rows` as the density switch.
