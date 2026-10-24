# Release notes

What each tagged release changed, newest first - a curated, linkable summary; the GitHub release pages carry the exhaustive auto-generated lists, and the [roadmap](https://github.com/l1br3-io/cosmic-ext-dropterm/blob/main/ROADMAP.md) carries what comes next. A section is added here at release time, by the release commit; nothing in between.

## v0.3.0 (2026-10-24)

The autosuggestions release: fish-style ghost text computed entirely terminal-side - no shell integration, no scripts shipped, no environment injected. As you type, the rest of the most recent matching command from this tab's own history renders as dim ghost text after the cursor; Right accepts all of it, Alt+Right accepts one shell-word, and nothing ever executes without an explicit accept. Beyond history, typed path tokens complete from the filesystem, command names complete from the spawn-time PATH snapshot, and the curated table finishes subcommands and long flags for git, cargo, docker, systemctl, and npm; an optional idle delay holds the ghost back until typing pauses, and every provider carries its own settings toggle. Around it: the status band's command-lifecycle tier, the settings font picker, the howto/FAQ/sample-config docs layer, and the empty-desktop and caret fixes.

- Autosuggestions - history ghosts, paths, command names, the subcommand/flag table ([autosuggestions](autosuggestions.md))
- The idle delay - `autosuggestions_delay_ms`, the ghost at the pause instead of the churn ([the delay](autosuggestions.md#the-delay))
- The status band's command lifecycle - the duration segment, the finished-command notification, the scrolled-back chip, the band's pointer idioms ([command time](status-bar.md#command-time), [the scrolled-back chip](status-bar.md#the-scrolled-back-chip), [reading and copying](status-bar.md#reading-and-copying))
- The font picker - the Appearance card's family list, each row previewed in its own face ([settings](settings.md#the-five-tabs))
- Desktop presence - hidden from the dock and window switcher by default, `show_in_dock` to restore it ([configuration](configuration.md#layer-focus-and-panels))
- The docs layer - Common tasks, the FAQ, the complete sample config ([common tasks](howto.md), [FAQ](faq.md))
- Fixes - toggle-show on an empty desktop lands focused with keybinds live, the block caret paints in the first shown frame, font zoom applies immediately, an empty title report clears the title
- Build - `just deps`, the prerequisite gate that installs whatever a fresh machine lacks ([development](development.md))

## v0.2.0 (2026-10-17)

The status band release: a 28 px glance strip beside the grab bar narrating the active terminal - foreground process, working directory with git branch, profile, cell dimensions, tab position, zoom, clock, command time, scroll position - every segment click-to-copy, drag-selectable, individually toggleable, with its own context menu. Profiles grew a full manager (transactional edit form, armed two-step delete, inherit-aware pickers), the tab strip and status band gained border hairlines, and the documentation you are reading shipped: this site, the research decision log, and the two drift gates that pin docs to code. Development installs split from the installed release under a `-dev` identity so the two never tug each other.

- The status band - segments, click-to-copy, selection, context menu, per-segment toggles ([status bar](status-bar.md))
- The profiles manager - the drawer's Profiles tab: form, clone, delete, the default picker ([settings](settings.md#profiles-tab), [profiles](configuration.md#profiles))
- Bar border hairlines - `tab_bar_border`, `status_bar_border` ([configuration](configuration.md#bar-borders))
- The docs site and its gates - this Jekyll tree, the config-key test, the link gate ([development](development.md))
- Dev/installed split - the `-dev` launcher identity and its own config and state ([development](development.md#development-launcher))
- The applet's packaging - metainfo and a local Flatpak build path ([applet](applet.md))
- Fixes - profile commands wrap into their shell with an interactive tail (a finishing `ls` no longer flash-kills the tab), the window background follows the effective theme mode, every surface names its app so the shell never shows "unknown"

## v0.1.0 (2026-10-10)

First release: the drop-down core and parity with Yakuake (the KDE drop-down terminal DropTerm takes its shape from). A layer-shell overlay anchored to the top or bottom edge - above other windows, hidden from Alt-Tab and the dock, shell session alive across hides - toggled by a second launch of the binary or a bound COSMIC shortcut. Tabs with everything that implies (rename, duplicate, reorder, cycling, numbered jumps, per-tab context menu), sessions saved and restored across shutdowns, drag-to-resize with remembered geometry, width and alignment, the 130 ms slide, an in-app settings drawer, twelve built-in color schemes following the system theme, and the CLI seam: `new-tab` and the tab verbs over the single-instance D-Bus surface, plus the COSMIC panel applet.

- The drop-down core - layer shell, exclusive focus, session-alive hides ([README](https://github.com/l1br3-io/cosmic-ext-dropterm))
- Tabs - open/close/rename/duplicate/reorder, title modes, per-tab menu ([keybindings](keybindings.md), [configuration](configuration.md#tab-titles))
- Session persistence ([sessions](sessions.md))
- Profiles - command, directory, scheme, font size, scrollback, environment per profile ([profiles](configuration.md#profiles))
- Settings drawer, theming, the twelve schemes ([settings](settings.md), [theming](theming.md))
- CLI remote control and `--is-visible` ([CLI reference](cli.md))
- The panel applet ([applet](applet.md))
