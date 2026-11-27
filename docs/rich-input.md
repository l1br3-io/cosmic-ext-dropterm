# Rich input

DropTerm can finish your command line the way [Warp](https://docs.warp.dev/terminal/editor) does - the rest of the command as dim ghost text, and a completion menu over the line - behind one setting, and without taking the line away from your shell. The default is `native`: the bare shell, every keystroke forwarded verbatim, no ghost, no menu.

This page covers the two modes, where and when the suggestions show, the keys the menu claims, the menu's layout, and the settings. What gets suggested, and from where, is [Autosuggestions'](autosuggestions.md) page.

## The two modes

`input_mode` in [Configuration](configuration.md#auto-complete) (globally, or per profile through the profile field of the same name):

- **`native` (the default)** - the bare shell. Every keystroke reaches the PTY verbatim; nothing suggests and nothing is drawn.
- **`rich`** - your shell's own line with the suggestions over it. Every keystroke still reaches the shell exactly as in `native`, so the shell's editing, history, and key bindings are yours; DropTerm reads the typed text off the screen and draws two overlays after it: the ghost and the completion menu.

There is one input on screen either way - the shell's, with the shell's caret - and the shell keeps its line discipline, echo, and history. No shell integration is required.

## Where and when the overlays show

Both overlays show only in a **live** pane: focused, nothing running in the foreground (vim, ssh, cargo), scrolled to the bottom, and a prompt DropTerm can recognize. A new tab's first prompt, at the top of its still-empty screen, is live. When any of that fails the overlays stand aside and every key is the shell's: a hidden suggestion costs a little richness, a wrongly shown one would type into a program's input. The overlays return on their own when the prompt is live again, and flipping the mode in Settings applies live.

## The keys

With the menu closed, every key is the shell's except the few named here:

| Key | The menu is closed | The menu is showing |
|---|---|---|
| a printable key | the shell's | goes to the shell; the menu stays open and its rows follow the typed text |
| Up / Down | the shell's (its history) | move the highlight, wrapping at both ends |
| Shift+Tab | the shell's | step backwards |
| Tab | opens the menu when rows match the typed word, otherwise the shell's own Tab | fills the longest common prefix of the matching rows (closing when one row is left), otherwise moves the highlight down |
| Enter | the shell's (runs the line) | completes the highlighted row and closes; on a line that already equals the row, closes and runs |
| Escape | the shell's | closes the menu, the line untouched |
| Right / Alt+Right | accepts the ghost whole / one word while it shows, otherwise the shell's | closes the menu first, then the same |
| any other key | the shell's | closes the menu, then the key's own meaning |

No chord opens the menu: it opens as you type and on Tab. Completions see through `$(...)` substitutions, pipes, and `;` chains - the segment the cursor stands in is what completes.

## The menu's layout

The menu is a panel over the prompt row, never a second input:

- **Rows** - two lines each: the label on top, its description in dim text under it, and the type badge (cmd, flag, path, hist, env, sub) at the panel's right edge. `input_compact_rows` makes a row one line, the description right-aligned before the badge.
- **Columns** - the row icons share one fixed column, as wide as the widest of them, so the labels start at one edge whatever their icon; the badges end at one edge. A label or description too long for its room is cut with an ellipsis.
- **The panel** - about 560 px wide at the default font, at the pane's left edge. It sits above the prompt row when it fits there and otherwise below it. It shows at most 8 rows and never grows past about half the pane; the rest scroll under Up and Down.
- **Sizes** - every size follows the character cell, so zooming the font scales the whole menu with the text.

## The limits of the screen read

DropTerm reads your line off the screen, which sets three limits:

- Suggestions work only on a prompt row that has not wrapped and whose text starts with the prompt. The honest-silence rules in [Autosuggestions](autosuggestions.md) apply where the shell's prompt cannot be recognized.
- A row completes the typed text only when its label starts with that text exactly, case included. A row matched loosely still lists, but inserts nothing; Enter on it closes the menu.
- DropTerm's view of the line is the shell's echo of it. In the instant between a keystroke and its echo, a menu key can act on the line as it was before the keystroke.

## The settings

The Settings window's Auto complete page carries every key (see [Configuration](configuration.md#auto-complete) for the full table): the mode, the ghost and menu toggles, the auto-popup, and the menu's chrome - row descriptions, type badges, row icons, the argument hint line, each its own key, all default on, with `compact_rows` as the density switch.
