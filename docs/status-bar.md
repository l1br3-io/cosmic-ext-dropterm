# Status bar

A 28 px strip at the surface's free edge, beside the grab bar, narrating the active terminal - what runs, where it is, and how it is dressed. It is derived, not decorated: colors follow the active color scheme, the text is typed in the terminal's own configured font at 12 px, and nothing about it resizes the terminal.

## The segments

Segments are values without labels - a path reads as a path - separated by gaps, never punctuation. Two clusters:

- **Left**: the state disc - filled while a foreground process runs, a hollow ring over the idle shell - with that process's name (its `/proc` comm; an unreadable read falls back to the recorded shell's basename), then the command duration, then the tab's cwd with `$HOME` collapsed to `~` and elided from its left when the row is squeezed, then the cwd's git branch (a ref name, or a detached commit's first seven characters; read from `.git/HEAD` with one `gitdir:` indirection so worktrees and submodules resolve, absent outside repositories).
- **Right, dimmed**: the tab's profile name (when it runs one), the terminal's cell dimensions (`80×24`), the tab position (`2/5`), the zoom delta (`+200%`) whenever the font is not at its base size, the scrolled-back chip, and the wall clock (`HH:MM`, local time, ticking on the minute).

The band always takes the slot adjacent to the grab bar, whatever the `edge` × `tab_position` stacking - it never trades places with the tab strip. The process name and git branch are sampled only when the terminal reports activity, a tab switches, or a spawn/zoom/config event lands - never per frame.

While `ssh` owns the foreground, the path's seat stops lying: the local cwd froze the moment ssh started, so the seat shows the ssh command's own destination token instead - `user@host`, a bare alias, exactly as typed, read from the process's argv with the option values skipped (`ssh -p 2222 box` shows `box`). The branch hides for the same staleness (the local repo is as stale as the path was), and a click copies the token. No toggle of its own governs this: the path and branch rules (`status_bar_show_path` / `status_bar_show_branch`) cover it, and when the argv cannot certify a destination the band keeps the local path rather than guess a host.

## Command time

While a foreground command runs, the duration segment beside the process name counts it once per second (`0:04`, `2:41`; `1:00:00` past the hour) - the tick alone drives it, so a command that prints nothing (`sleep 30`) still counts. When the command finishes the reading freezes as the last command's cost, paired with its name - `cargo 2:41` - because by then the process segment already shows the shell again; the pair stands until the next command starts.

The timing is derived, never guessed: it comes from the two moments the terminal itself observes the foreground change (a process taking the terminal over from the shell, and handing it back), with no shell integration. A command whose start was never observed - the initial command of a profile tab, wrapping into its shell at spawn - shows no duration at all rather than a wrong one, and a respawned tab starts clean. The 1-second tick exists only while the active tab actually runs a command on a visible surface with the segment shown; it stands down within a second of the command finishing, the surface hiding, or the toggle flipping - hiding the segment (`status_bar_show_duration`) is the kill switch, and because the timing itself always runs, re-enabling it mid-command shows the live count immediately.

## The scrolled-back chip

When the viewport rests somewhere other than the bottom, a dim chip between the zoom and the clock says how far up it sits - `↑12` for twelve lines back. At the bottom it is absent entirely: the chip exists only while there is somewhere to return to, the one glance question a drop-down asks on every re-landing - am I looking at the end, or at what scrolled past while I was away? It drops under width pressure after the command duration and before the clock.

The chip is the band's one **action chip**: clicking it jumps the viewport to the bottom and the chip vanishes the same frame. Every other segment copies its value on click - this one has nothing to copy, so its click is the jump; a deliberate, one-time bend of the copy rule, not a precedent (there is no menu row for it either - the click is the whole verb). The count rides the same in-memory read as the terminal size, taken whenever the band re-renders. Hide it with `status_bar_show_scrollback`.

## Reading and copying

**Every segment is click-to-copy**: a press writes the segment's full value to the clipboard - a squeezed path still copies whole - flashes a `✓`, and the cursor points over each segment while the gaps stay idle.

Press-and-drag across the band instead selects the covered characters of the row's display text, and the release copies the selection to both the clipboard and the primary selection (so the terminal's own middle-click paste serves it). The selection is WYSIWYG - the elided display included, segments joined by single spaces - while the click verb stays the full un-elided value. A highlight persists after release, like terminal selection, and clears on the next press; a press that never crosses the 4 px drag threshold is a click, past it a selection.

## The context menu

A right-press on the band opens its menu: **Copy** (the pressed segment's value - muted when the press scoped a gap or an empty segment), **Copy All** (every present segment's value), **Paste**, then a zoom cluster - **Zoom In**, **Zoom Out**, **Reset Zoom** (muted at base size) - and the band's lifecycle rows, **Hide Status Bar** and **Settings…**. Enablement is live, re-derived every frame, so a keybind passing through an open menu cannot leave a stale row; muted rows render disabled and never publish.

## Toggling and tailoring

Flip the whole band with `Ctrl+Shift+B`, the Behavior tab's Status bar card, or the `status_bar_enabled` key - it applies the moment it lands: the band leaves or joins the column and the terminal's pinned height reclaims or cedes its 28 px the same frame. Each segment is individually showable with its own `status_bar_*` key (a hidden segment is never computed - hiding the branch stops the `.git` reads, hiding the clock stops its tick), and the band's terminal-facing hairline has its own `status_bar_border`. The full key inventory lives in [configuration](configuration.md#status-bar).
