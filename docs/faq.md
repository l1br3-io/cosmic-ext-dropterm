# FAQ

The questions that recur, answered directly - each with the page that owns the full story. For upstream compositor bugs rather than usage questions, see [Known issues](known-issues.md).

## Setup and the shortcut

**Why doesn't the terminal appear when I press my shortcut?**
First, the toggle must be a *COSMIC custom shortcut* running `cosmic-ext-dropterm` - DropTerm cannot register hotkeys itself (next question). Second, some Shift-modified custom shortcuts fail to fire on COSMIC; prefer an unshifted key like `F12`. The walk lives in [README: Setting up the shortcut](https://github.com/l1br3-io/cosmic-ext-dropterm#setting-up-the-shortcut); the Shift bug is tracked in [Known issues](known-issues.md).

**Why can't DropTerm register its own global hotkey?**
COSMIC does not yet implement the XDG `GlobalShortcuts` portal, so no app can register one; binding a COSMIC custom shortcut is the only path today ([Known issues](known-issues.md)).

**Is there a Flatpak?**
No, and not for want of trying: cosmic-comp does not expose the layer-shell and toplevel-management protocols a true drop-down needs to sandboxed clients. A sandboxed DropTerm could not drop down at all ([Known issues](known-issues.md)).

**How do I uninstall it?**
Remove the two `cosmic-ext-dropterm*` binaries from `/usr/bin` and the `io.github.l1br3_io.CosmicExtDropterm*` files under `/usr/share/{applications,icons,metainfo}/` - or, from a source checkout, `sudo just uninstall` ([README: Installing](https://github.com/l1br3-io/cosmic-ext-dropterm#installing)).

## Behavior

**Why does the terminal hide when I click another window? Can I stop that?**
That is `hide_on_focus_loss`, on by default because a hotkey-driven drop-down lives on focus - click away, it is gone; press the key, it is back, shell intact. Write `false` to the key (Behavior tab in settings) to keep the overlay up while you work elsewhere ([configuration](configuration.md#behavior-toggles)).

**Does my shell session survive the hide? What about a reboot?**
Each in its own way. Hiding preserves the *live* session - only the surface is destroyed, the PTY keeps running, so a `sleep` or an editor outlives every toggle. A reboot cannot preserve live processes; what restores is the layout: tabs with their directories, profiles, and renames save on change and come back on launch (`session_persistence_enabled` is the switch, on by default) ([sessions](sessions.md)).

**Does copied text survive hiding the terminal?**
Yes - DropTerm keeps an invisible anchor window alive for exactly this reason, so the clipboard binding (and your last copy) outlives every hide/show cycle. The mechanics are in [Known issues](known-issues.md).

**Why doesn't DropTerm appear in the dock or Alt-Tab? Can I get it back?**
That is the default: the app hides from both, and you summon it with your shortcut, the [panel applet](applet.md), or the CLI. If you would rather have a DropTerm entry to click, write `show_in_dock: true` - the trade is that the entry also joins Alt-Tab, because the dock icon and the switcher window are the same mapped window underneath ([configuration](configuration.md#layer-focus-and-panels)).

**Why does the window flash or recreate when it hides?**
Hiding destroys the layer surface and the next show builds a fresh one - deliberately, to sidestep a cosmic-comp bug other layer-shell clients hit when a surface is updated after its role is destroyed. You are seeing the designed path, not a glitch ([Known issues](known-issues.md)).

**How do I keep it from covering my panel or dock?**
Write `respect_panel` and the drop-down places itself inside the compositor's usable area on its configured layer. With an autohide panel expect a few pixels of reserved handle strip ([configuration](configuration.md#layer-focus-and-panels)).

## Tabs and suggestions

**Which shells does it run?**
Whatever you point it at: the `shell` key takes a path (`/usr/bin/fish`), the settings picker lists `/etc/shells`, and unset falls back to `$SHELL`, then `/bin/sh`. Profiles can run anything - `ssh web-prod`, `htop` - with their own directory, colors, and environment ([configuration](configuration.md#shell-and-directories), [profiles](configuration.md#profiles)).

**Why did my tab title change to "Terminal N"?**
Titles follow `tab_title_mode`: by default the shell's own title (OSC 0/2) shows, and a program that clears its title - or never sets one - falls back to `Terminal N`. A manual rename always wins; the five modes are in [configuration](configuration.md#tab-titles).

**Why don't autosuggestions work over ssh?**
They are local-machine knowledge by design: history comes from this tab and the spawn-time shell, paths and command names from this machine's filesystem and `$PATH`, so a tab running ssh stays silent rather than suggesting your local directories on the remote host ([autosuggestions](autosuggestions.md)).

**Why is the suggestion cut off with an `…`? When there is no room, will it still accept?**
The ghost clips to what fits - the longest prefix plus a two-cell ellipsis - exactly fish's rule, and below three free columns nothing draws. The clip is display-only: pressing Right always accepts the *full* suggestion, never the truncated text ([autosuggestions](autosuggestions.md)).

**Can I turn the suggestions off?**
`autosuggestions_enabled: false` is the kill switch; path, command-name, and flag completion each have their own key if you want history-only or any subset ([configuration](configuration.md#behavior-toggles)).

## Running two copies

**Can I run two instances at once?**
Not as the same identity: one running instance owns the surface, and a second launch just toggles it - that is the single-instance design. Development builds get their own `-dev` identity (separate config, bus name, and launcher), so a dev copy and your installed DropTerm do run side by side without seeing each other ([development](development.md#development-launcher)).
