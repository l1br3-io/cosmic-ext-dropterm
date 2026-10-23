# Release notes

What each tagged release changed, newest first - a curated, linkable summary; the GitHub release pages carry the exhaustive auto-generated lists, and the [roadmap](https://github.com/l1br3-io/cosmic-ext-dropterm/blob/main/ROADMAP.md) carries what comes next. A section is added here at release time, by the release commit; nothing in between.

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

## Next (unreleased, on main)

Fish-style autosuggestions - history ghosts, path and command-name completion, the curated subcommand/flag table - shipped to main after v0.2.0 and rides the next release ([autosuggestions](autosuggestions.md)).
