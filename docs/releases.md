# Release notes

What each tagged release changed, newest first - a curated, linkable summary; the GitHub release pages carry the exhaustive auto-generated lists, and the [roadmap](https://github.com/l1br3-io/dropterm/blob/main/ROADMAP.md) carries what comes next. A section is added here at release time, by the release commit; nothing in between.

## v0.5.0 (2026-11-12)

The identity release: DropTerm drops the `cosmic-ext-` prefix and takes ownership of plain `dropterm` - the binary, the app id (`io.github.l1br3_io.dropterm`), the repository (`l1br3-io/dropterm`), and the release assets all rename, and the configuration moves with them. Settings, profiles, and keybinds now live as RON sections under `$XDG_CONFIG_HOME/io.github.l1br3_io.dropterm/` - `config.ron`, `profiles.ron`, `keybinds.ron`, and the optional `completions.toml` - replacing the one-file-per-key layout under `~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/`. The move is greenfield: nothing reads or imports the old layout, the first start begins from the defaults, and the old files are left untouched on disk. Edits to the new files apply live - a hand edit reaches the running terminal, and a file that does not parse is ignored while the app runs and moved aside to `.bak` at the next start ([how edits apply](configuration.md#how-edits-apply)). Sessions follow the new id (`~/.local/state/io.github.l1br3_io.dropterm/session.ron`).

This release also carries everything from the never-published v0.4.0 tag (its workflow run failed; the section below documents it): pasted images end to end, the status band's full dress, and the always-on autosuggestion generators.

- The identity rename - binary `dropterm`, app id `io.github.l1br3_io.dropterm`, repository `l1br3-io/dropterm`, assets `dropterm-<ver>-<arch>.tar.gz` ([installing](https://github.com/l1br3-io/dropterm#installing))
- The config store - RON sections in the plain XDG directory, live reload, startup quarantine of unparseable files, the watched `completions.toml` ([the default config directory](configuration.md#the-default-config-directory))
- The layer-shell guard - where the compositor offers no `zwlr_layer_shell_v1` the binary names the missing protocol and exits non-zero, instead of silently never surfacing ([known issues](known-issues.md))
- Carried from the v0.4.0 tag - pasted images and the per-tab images panel, the band's leading glyphs, hairlines, and hover tips, the background-loaded completions table ([images](images.md#pasting-images), [status bar](status-bar.md#the-segments))
- Internal - the surface-intent vocabulary (handlers emit declarative intents through a toolkit-neutral executor) and the D-Bus config watch leaves the build; the COSMIC theme follow rides the file watcher

## v0.4.0 (2026-11-09)

*Tag-only: the tag's release workflow run failed and, ruled 2026-11-11, is not retried — no published v0.4.0 release exists. This section documents what the tag carries; the next release supersedes it.*

The pasted-images release: an image on the clipboard - a screenshot tool's copy, or an image file copied in COSMIC Files or Nautilus - never renders in the grid. It lands in `~/.cache/dropterm/paste/` under its sniffed format's extension, its path pastes as text, and a new per-tab images panel keeps every paste findable after the prompt scrolls away. The status band gains its full dress on the way: ten leading glyphs beside the three segments that already own a mark, hairlines dividing adjacent segments, and a hover tip on every segment. Around them, the autosuggestion generators lose their toggle, the completions table loads off the startup path, and a run of clipboard, focus, and compositor fixes lands.

- Pasted images - two clipboards into one pipeline: the image bytes on the clipboard, or exactly one local image file named by a `file://` URI, admitted by its own first bytes (never its name, its extension, or the URI's claim) and refused whole over 32 MiB; the path is the paste's only grid trace ([images](images.md#pasting-images))
- The images panel - per tab, newest-first, each row a thumbnail with its restated stamp and dimensions; Open, Copy path, Copy image, Re-paste, and a two-step Delete whose armed pair is a ✓/✕ with its verbs in the tooltips; three doors, the band's chip, its context menu, and `Ctrl+Shift+P` ([the panel](images.md#pasting-images))
- The pasted-images chip - `▦ 3 images` in the band while the tab has pastes, pluralized at two, yielding first under width pressure ([status bar](status-bar.md#the-segments))
- The status band's dress - ten leading glyphs (branch `⎇`, grid `⊞`, tab `▤`, profile `◉`, duration `⧗`, progress `◐`, path `⌂`, zoom `⇕`, pasted `▦`, clock `◷`) beside the three owned marks, a hairline between each pair of adjacent segments of a cluster, and a hover tip on every segment; a click still copies the bare value ([status bar](status-bar.md#the-segments))
- Autosuggestions - the git generators are always on (the `autosuggestions_generators_enabled` toggle is gone, 51 keys), and the spec table loads on a background thread with a loading row instead of blocking startup ([autosuggestions](autosuggestions.md))
- Packaging - the cosmic-flatpak submission plan is withdrawn: cosmic-comp filters layer-shell from sandboxed clients, so a store install could never carry the drop-down itself; the native release stays the install ([applet](applet.md), [FAQ](faq.md))
- Fixes - copies land while the terminal is unfocused or hidden (the clipboard I/O moves to the data-control protocol), the clipboard anchor reopens when the compositor closes it with its output, a stay-open surface shares the keyboard instead of holding it hostage, the pasted chip's press reaches the panel, and the panel's thumbnails and the paste cache take the ceilings the render path already had

## v0.3.1 (2026-11-07)

The store-listing release: the applet and the terminal both ship their AppStream `<categories>`, clearing the one blocker between DropTerm and the COSMIC Flatpak source — its CI's `appstreamcli compose` drops a category-less component and aborts the export, the 2026-11-04 validation walk's recorded find, now fixed in the shipped bytes (the applet `System`, the terminal `System` + `TerminalEmulator`) so the catalog branch recuts from this tree with no patched manifest. Alongside, the kitty image z-layers tighten under review: eviction keeps the layer with its pixels, re-placing honors an explicit z, and the above-glyph pass composites by z — kitty's own overlap rule.

- The store listing unblocked - both metainfo files carry their categories, the manifest pinning this release's bytes, the catalog submission cleared to file ([applet](applet.md))
- Kitty images, review pass - eviction takes the layer with the pixels, an explicit z moves an existing placement's seat on the wire's ask (an absent z keeps what the seat holds), and the above-glyph pass composites by z ([images](images.md))

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
