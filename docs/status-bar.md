# Status bar

A 28 px strip at the surface's free edge, beside the grab bar, narrating the active terminal - what runs, where it is, and how it is dressed. It is derived, not decorated: colors follow the active color scheme, the text is typed in the terminal's own configured font at 12 px, and nothing about it resizes the terminal.

## The segments

Segments are values without labels - a path reads as a path - separated by gaps, never punctuation. Two clusters:

- **Left**: the state disc - filled while a foreground process runs, a hollow ring over the idle shell - with that process's name (its `/proc` comm; an unreadable read falls back to the recorded shell's basename), then the tab's cwd with `$HOME` collapsed to `~` and elided from its left when the row is squeezed, then the cwd's git branch (a ref name, or a detached commit's first seven characters; read from `.git/HEAD` with one `gitdir:` indirection so worktrees and submodules resolve, absent outside repositories).
- **Right, dimmed**: the tab's profile name (when it runs one), the terminal's cell dimensions (`80×24`), the tab position (`2/5`), the zoom delta (`+200%`) whenever the font is not at its base size, and the wall clock (`HH:MM`, local time, ticking on the minute).

The band always takes the slot adjacent to the grab bar, whatever the `edge` × `tab_position` stacking - it never trades places with the tab strip. The process name and git branch are sampled only when the terminal reports activity, a tab switches, or a spawn/zoom/config event lands - never per frame.

## Reading and copying

**Every segment is click-to-copy**: a press writes the segment's full value to the clipboard - a squeezed path still copies whole - flashes a `✓`, and the cursor points over each segment while the gaps stay idle.

Press-and-drag across the band instead selects the covered characters of the row's display text, and the release copies the selection to both the clipboard and the primary selection (so the terminal's own middle-click paste serves it). The selection is WYSIWYG - the elided display included, segments joined by single spaces - while the click verb stays the full un-elided value. A highlight persists after release, like terminal selection, and clears on the next press; a press that never crosses the 4 px drag threshold is a click, past it a selection.

## The context menu

A right-press on the band opens its menu: **Copy** (the pressed segment's value - muted when the press scoped a gap or an empty segment), **Copy All** (every present segment's value), **Paste**, then a zoom cluster - **Zoom In**, **Zoom Out**, **Reset Zoom** (muted at base size) - and the band's lifecycle rows, **Hide Status Bar** and **Settings…**. Enablement is live, re-derived every frame, so a keybind passing through an open menu cannot leave a stale row; muted rows render disabled and never publish.

## Toggling and tailoring

Flip the whole band with `Ctrl+Shift+B`, the Behavior tab's Status bar card, or the `status_bar_enabled` key - it applies the moment it lands: the band leaves or joins the column and the terminal's pinned height reclaims or cedes its 28 px the same frame. Each segment is individually showable with its own `status_bar_*` key (a hidden segment is never computed - hiding the branch stops the `.git` reads, hiding the clock stops its tick), and the band's terminal-facing hairline has its own `status_bar_border`. The full key inventory lives in [configuration](configuration.md#status-bar).
