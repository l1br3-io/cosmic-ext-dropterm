# Status bar

A 28 px strip at the surface's free edge, beside the grab bar, narrating the active terminal - what runs, where it is, and how it is dressed. It is derived, not decorated: colors follow the active color scheme, the text is typed in the terminal's own configured font at 12 px, and nothing about it resizes the terminal.

## The segments

Segments are values without labels - a path reads as a path - separated by gaps, never punctuation. Two clusters:

- **Left**: the state disc - filled while a foreground process runs, a hollow ring over the idle shell, its fill tinted the scheme's red while the program's own OSC 7501 report says `blocked` or `error` - with that process's name (its `/proc` comm; an unreadable read falls back to the recorded shell's basename; the program's own progress percent rides beside it - `cargo 40%` - while its report carries one; see [Shell integration](#shell-integration-optional)), then the command duration, then the exit chip (`✗1`, only after a failed command - see [Shell integration](#shell-integration-optional)), then the progress chip (`42%`, the running command's own OSC 9;4 report - `…` while indeterminate, the value in the scheme's red on error; see [Shell integration](#shell-integration-optional)), then the tab's cwd with `$HOME` collapsed to `~` (the shell's own OSC 7 report outranks it - and the ssh stand-in - while one stands, `host:path` on a remote host; see [Shell integration](#shell-integration-optional)) and elided from its left when the row is squeezed, then the cwd's git branch (a ref name, or a detached commit's first seven characters; read from `.git/HEAD` with one `gitdir:` indirection so worktrees and submodules resolve, absent outside repositories - hidden while a remote report or the ssh stand-in owns the path seat, when the local branch is as stale as the local path was).
- **Right, dimmed**: the tab's profile name (when it runs one), the terminal's cell dimensions (`80×24`), the tab position (`2/5`), the zoom delta (`+200%`) whenever the font is not at its base size, the scrolled-back chip, and the wall clock (`HH:MM`, local time, ticking on the minute).

The band always takes the slot adjacent to the grab bar, whatever the `edge` × `tab_position` stacking - it never trades places with the tab strip. The process name and git branch are sampled only when the terminal reports activity, a tab switches, or a spawn/zoom/config event lands - never per frame.

While `ssh` owns the foreground, the path's seat stops lying: the local cwd froze the moment ssh started, so the seat shows the ssh command's own destination token instead - `user@host`, a bare alias, exactly as typed, read from the process's argv with the option values skipped (`ssh -p 2222 box` shows `box`). The branch hides for the same staleness (the local repo is as stale as the path was), and a click copies the token. No toggle of its own governs this: the path and branch rules (`status_bar_show_path` / `status_bar_show_branch`) cover it, and when the argv cannot certify a destination the band keeps the local path rather than guess a host.

## Command time

While a foreground command runs, the duration segment beside the process name counts it once per second (`0:04`, `2:41`; `1:00:00` past the hour) - the tick alone drives it, so a command that prints nothing (`sleep 30`) still counts. When the command finishes the reading freezes as the last command's cost, paired with its name - `cargo 2:41` - because by then the process segment already shows the shell again; the pair stands until the next command starts.

The timing is derived, never guessed: it comes from the two moments the terminal itself observes the foreground change (a process taking the terminal over from the shell, and handing it back), with no shell integration. A command whose start was never observed - the initial command of a profile tab, wrapping into its shell at spawn - shows no duration at all rather than a wrong one, and a respawned tab starts clean. The 1-second tick exists only while the active tab actually runs a command on a visible surface with the segment shown; it stands down within a second of the command finishing, the surface hiding, or the toggle flipping - hiding the segment (`status_bar_show_duration`) is the kill switch, and because the timing itself always runs, re-enabling it mid-command shows the live count immediately.

A command that *failed* says so beside its cost - `cargo 2:41 ✗1` - a small chip in the scheme's own red, the same attention red the missed-activity halo rings with. Success renders nothing (exit code 0 is the unmarked default), and the chip stands exactly as long as the frozen duration does: the next command's start clears both facts together. Clicking the chip copies the bare code (`1`, never the glyph), and it drops under width pressure right after the duration it verdicts. The verdict itself is never guessed either - it comes solely from the shell's own report, via optional [shell integration](#shell-integration-optional); without it the band renders exactly what it renders today. Hide the chip with `status_bar_show_exit_status` (the engine keeps recording verdicts either way, so a re-enable shows whatever still stands).

## Shell integration (optional)

The exit chip's one source is the shell *telling* the terminal a command finished and what it cost: `OSC 133 ; D ; <code>` - the "command finished" mark of the semantic prompt zones, the FinalTerm/contour shell-integration standard. DropTerm reads only this `D` mark; the prompt/output boundary marks (`A`/`B`/`C`) other terminals build on are ignored, so any integration that emits `D` works as-is.

- **fish 4.0 and later** need nothing: fish marks its prompts natively (`A`, `C` and `D` from 4.0.0). `set -Ua fish_features no-mark-prompt` (fish 4.0.6+) turns fish's own marks off - unset it and the chip returns.
- **zsh** - two hooks in `~/.zshrc`, the shape of contour's own integration reduced to the one mark DropTerm reads: `$?` captured before anything can clobber it, and the report sent only once a command has actually run (an unconditional report would verdict the shell's startup line):

  ```zsh
  _dropterm_preexec() { _dropterm_ran=1 }
  _dropterm_precmd() {
      local exit_status=$?  # first line: nothing may clobber the status
      if [[ -n ${_dropterm_ran:-} ]]; then
          print -n "\e]133;D;${exit_status}\e\\"
          unset _dropterm_ran
      fi
  }
  autoload -Uz add-zsh-hook
  add-zsh-hook preexec _dropterm_preexec
  add-zsh-hook precmd _dropterm_precmd
  ```

- **bash** has no prompt hook of its own; [bash-preexec](https://github.com/rcaloras/bash-preexec) supplies the same pair (at the end of `~/.bashrc`):

  ```bash
  preexec() { _dropterm_ran=1; }
  precmd() {
      local exit_status=$?
      if [[ -n ${_dropterm_ran:-} ]]; then
          printf '\e]133;D;%s\e\\' "$exit_status"
          unset _dropterm_ran
      fi
  }
  ```

  contour's shipped integration scripts (which vendor bash-preexec and emit the full A/B/C/D set) work as-is if you already run them. The reference for the sequence is contour's [OSC 133 shell-integration documentation](https://github.com/contour-terminal/contour/blob/master/docs/vt-extensions/osc-133-shell-integration.md), and its shipped hooks are [the source these snippets are reduced from](https://github.com/contour-terminal/contour/tree/master/src/contour/cli/shell-integration).

The same opt-in channel carries the path's authoritative source: `OSC 7 ; file://host/path` - the shell reporting its own working directory (WezTerm/iTerm2/VTE semantics; Fedora's bash and zsh already emit it by default through the sourced `vte.sh`). While a report stands it outranks both layers the path seat otherwise reads - the ssh stand-in and the `/proc`-derived cwd - so an SSH tab whose remote shell reports shows the true remote path, rendered `host:path` and never `~`-collapsed (no remote home is known, and a guessed collapse is a lie). A report naming this machine renders bare and collapses like a local one, case-insensitively (the DNS rule); the git branch hides under a remote report, by the same staleness rule that hides it under the stand-in.

The report stands until a newer one replaces it, and a tab's respawn starts clean - the peer lifetime (WezTerm and kitty take the last report the same way): staleness belongs to the emitter, and an integration that reports at every prompt self-corrects the moment it can.

One rule for emitters: percent-encode at least `%` in the path (`%` → `%25`, as `vte.sh` does). A bare `%` makes the report malformed, and DropTerm drops it whole rather than display a wrong path.

- **fish** - one line inside the prompt function:

  ```fish
  function fish_prompt
      printf '\e]7;file://%s%s\e\\' (hostname) (string replace --all '%' '%25' -- $PWD)
      # ... the prompt itself
  end
  ```

- **zsh** - a `precmd` hook in `~/.zshrc` (composes with the `D`-mark hook above; both ride `precmd`):

  ```zsh
  _dropterm_osc7() { print -n "\e]7;file://${HOST}${PWD//\%/%25}\e\\" }
  autoload -Uz add-zsh-hook
  add-zsh-hook precmd _dropterm_osc7
  ```

- **bash** - add the same report to the `precmd` hook above, before the `D`-mark `printf`:

  ```bash
  printf '\e]7;file://%s%s\e\\' "$HOSTNAME" "${PWD//\%/%25}"
  ```

The same opt-in channel carries the running command's own progress: `OSC 9 ; 4 ; <state> [; <percent>]` - the ConEmu-family progress report (Windows Terminal, kitty, and Ghostty 1.2+ read the same sequence; the reference is Ghostty's [ConEmu-style progress documentation](https://ghostty.org/docs/runtime-ansi-escapes/conemu-progress)). While a report stands the chip renders beside the exit chip: `42%` while a value stands, `…` when the reporter says indeterminate (it owns the chip - a stale number is not a number worth narrating), the value in the scheme's own attention red when the reporter says error, `!` in that red when error never had a value, and paused plain - the percent is the truth, paused is not a hue. A report that omits its percent keeps the standing one (the protocol's leave-unchanged rule, states 2-4). Clicking the chip copies the bare percent.

Three deaths keep the number honest, and each is true to its source: the protocol's own `state 0` clears; the chip dies at the command's finish (the foreground handoff the duration timer already observes - a finished command's percent is history the exit chip narrates better); and a report expires ~15 s after its last refresh (Ghostty's own hardcoding, inherited), so a reporter that dies mid-command cannot leave its last number lying. The expiry clock runs on any tab whose report stands, hidden or not - the band can switch to a background tab, and its number must not have outlived its window there either.

To see each state from a shell by hand:

```sh
printf '\e]9;4;1;42\e\\'   # 42%   - in progress
printf '\e]9;4;1;84\e\\'   # 84%   - the value moves with each report
printf '\e]9;4;3\e\\'      # …     - indeterminate owns the chip
printf '\e]9;4;2;42\e\\'   # 42%   - error, in the scheme's red
printf '\e]9;4;4;42\e\\'   # 42%   - paused, plain
printf '\e]9;4;0\e\\'      # gone  - the protocol's own clear
printf '\e]9;4;2\e\\'      # !     - error with no standing value, in the red
printf '\e]9;4;0\e\\'      # gone
```

A real integration emits from inside the long-running command - a deployment script reporting as it walks its steps - not from rc files; anything already speaking ConEmu progress (tooling built for Windows Terminal, kitty, or Ghostty) works as-is.

The same channel also carries the program's own status: `OSC 7501 ; key=value[:key=value...]` - Hashimoto's Program Status Protocol (libghostty and kitty read it; the reference is the [specification](https://mitchellh.com/writing/program-status-osc7501)). The report's one required key is `state` - `idle`, `working`, `done`, `blocked`, or `error` - with an optional `progress` 0-100, and the protocol's grammar holds exactly as it specifies: unknown keys ignored, malformed pairs skipped, a repeated key's last value winning, and an unrecognized state discarding the whole report. What it moves is what the process segment already renders: `blocked` (the program waits on *you* - a permission prompt, a question) and `error` tint the disc's fill in the scheme's own attention red; `working`, `done`, and `idle` leave the disc to its `/proc` truth; and any state carrying a progress percent shows it beside the process name - `cargo 40%` - the name's copy payload staying the name. `done` deliberately tints nothing: finishes are already narrated by their own honest channels (the exit chip, the missed-activity halo), and a second finish signal would be a second narrator, not a second truth.

The lifetime rides DropTerm's own observed edges, which are stronger than the protocol's prompt-mark proxies: the foreground handoff drops `working`/`blocked` (the program ended - DropTerm watches it happen), `done`/`error` persist until a newer report replaces them, `state=clear` clears, a full terminal reset clears everything, and a tab's respawn starts clean. One record is consumed: the root report alone - the band has one disc and one name seat, and the root is the program's own summary. Child records (reports carrying an `id`) and the `app`, `title`, `msg`, and `kind` keys are ignored by design: `title` and `msg` are the protocol's text channels and a band segment has no room for them - and not rendering them inherits the protocol's bidirectional and invisible-stripping worries away. DropTerm answers nothing to the feature probe (`OSC 7501 ; ?`): a probing program gets silence and sends unconditionally.

To see each state from a shell by hand:

```sh
printf '\e]7501;state=working:progress=40\e\\'    # cargo 40%, the disc unchanged
printf '\e]7501;state=blocked:kind=question\e\\'  # the disc's fill, in the scheme's red
printf '\e]7501;state=error:progress=70\e\\'      # cargo 70%, the fill still red
printf '\e]7501;state=done\e\\'                   # the disc and name alone, again
printf '\e]7501;state=idle\e\\'                   # the same - idle is not a hue
printf '\e]7501;state=clear\e\\'                  # gone - the protocol's own clear
```

A real emitter reports from inside the program itself - terraform and claude-code already speak it (the protocol's own early adopters); anything built for libghostty's or kitty's 7501 support works as-is.

Integration is strictly opt-in by your own rc files - DropTerm injects nothing (`ZDOTDIR` and `PROMPT_COMMAND` tricks are out). Without it, no `D` mark ever arrives, and the band never guesses a verdict: a bell is not failure, a title is not a status. Without an OSC 7 report the path seat reads exactly what it always did. Without an OSC 9;4 report the band renders exactly what it renders without the chip. Without an OSC 7501 report the disc and the name render exactly what they always did.

## The scrolled-back chip

When the viewport rests somewhere other than the bottom, a dim chip between the zoom and the clock says how far up it sits - `↑12` for twelve lines back. At the bottom it is absent entirely: the chip exists only while there is somewhere to return to, the one glance question a drop-down asks on every re-landing - am I looking at the end, or at what scrolled past while I was away? It drops under width pressure after the command duration and before the clock.

The chip is the band's one **action chip**: clicking it jumps the viewport to the bottom and the chip vanishes the same frame. Every other segment copies its value on click - this one has nothing to copy, so its click is the jump; a deliberate, one-time bend of the copy rule, not a precedent (there is no menu row for it either - the click is the whole verb). The count rides the same in-memory read as the terminal size, taken whenever the band re-renders. Hide it with `status_bar_show_scrollback`.

## The missed-activity halo

Output landing on the **active** tab while the surface is hidden leaves a trace nothing else records - the tab strip's markers accrue only for inactive tabs, and a desktop toast retires the moment the daemon drops it. The band answers: a thin attention ring around the process disc, in the live scheme's own red - the bright variant on dark strips, the plain on light, the same luminance flip the band's own text performs - that survives the show and clears only when the user acknowledges the news. It is the in-band sibling of the desktop notification: the toast expires, the halo persists until acknowledged.

The acknowledgment set is every touch: typed input, a paste, a scroll (wheel or key), a click in the terminal pane, the scrolled chip's jump to the bottom, and activating another tab - a switch clears both tabs' marks (the outgoing tab's because it ceased to be active, the incoming's because activation is a touch). The show itself clears nothing: a mark cleared on show would never be seen. There is no timed expiry, and respawned tabs start clean.

The ring renders with or without the process segment - with it, the ring circles the disc; without it, it stands alone at the band's left edge: the mark is the surface's news, not the segment's decoration. It never becomes a click target and never moves a rect. Hide it with `status_bar_activity_mark` (key off means never gathered and never rendered; the latch keeps arming either way, so a later flip-on shows whatever the surface still holds).

## Reading and copying

**Every segment is click-to-copy**: a press writes the segment's full value to the clipboard - a squeezed path still copies whole - flashes a `✓`, and the cursor points over each segment while the gaps stay idle.

Press-and-drag across the band instead selects the covered characters of the row's display text, and the release copies the selection to both the clipboard and the primary selection (so the terminal's own middle-click paste serves it). The selection is WYSIWYG - the elided display included, segments joined by single spaces - while the click verb stays the full un-elided value. A highlight persists after release, like terminal selection, and clears on the next press; a press that never crosses the 4 px drag threshold is a click, past it a selection.

Double-click selects a whole segment: a second press within 500 ms and 4 px of the first arms that segment's entire display text as the selection, and its release copies it WYSIWYG to the clipboard and the primary selection - the highlight persisting until the next press is the feedback, no `✓` flashes (the flash means "copied the full value", and this verb copies the display). The verbs compose in the terminal's own order: a double-click's first release is still a click - full value plus `✓` - and its second press-and-release selects.

Middle-click anywhere on the band - segment or gap alike - pastes into the narrated terminal through the context menu's own Paste funnel: the band is terminal chrome, and middle-click-paste is the terminal's native verb.

## The context menu

A right-press on the band opens its menu: **Copy** (the pressed segment's value - muted when the press scoped a gap or an empty segment), **Copy All** (every present segment's value), **Paste**, then a zoom cluster - **Zoom In**, **Zoom Out**, **Reset Zoom** (muted at base size) - and the band's lifecycle rows, **Hide Status Bar** and **Settings…**. Enablement is live, re-derived every frame, so a keybind passing through an open menu cannot leave a stale row; muted rows render disabled and never publish.

## Toggling and tailoring

Flip the whole band with `Ctrl+Shift+B`, the Behavior tab's Status bar card, or the `status_bar_enabled` key - it applies the moment it lands: the band leaves or joins the column and the terminal's pinned height reclaims or cedes its 28 px the same frame. Each segment is individually showable with its own `status_bar_*` key (a hidden segment is never computed - hiding the branch stops the `.git` reads, hiding the clock stops its tick), the band's terminal-facing hairline has its own `status_bar_border`, and its missed-activity halo its own `status_bar_activity_mark`. The full key inventory lives in [configuration](configuration.md#status-bar).
