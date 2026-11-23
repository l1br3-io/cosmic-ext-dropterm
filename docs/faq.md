# FAQ

The questions that recur, answered directly - each with the page that owns the full story. For upstream compositor bugs rather than usage questions, see [Known issues](known-issues.md).

## Setup and the shortcut

**Why doesn't the terminal appear when I press my shortcut?**
First, the toggle must be a *COSMIC custom shortcut* running `dropterm` - DropTerm cannot register hotkeys itself (next question). Second, some Shift-modified custom shortcuts fail to fire on COSMIC; prefer an unshifted key like `F12`. The walk lives in [README: Setting up the shortcut](https://github.com/l1br3-io/dropterm#setting-up-the-shortcut); the Shift bug is tracked in [Known issues](known-issues.md). On COSMIC you can skip the walk entirely: the onboarding's **Configure for me** - also reachable from the Keybindings tab's Global group - writes the custom shortcut for you (**Use F12**, or capture your own chord); [Settings](settings.md) describes the seat and the remove button.

**Why can't DropTerm register its own global hotkey?**
COSMIC does not yet implement the XDG `GlobalShortcuts` portal, so no app can register one; binding a COSMIC custom shortcut is the only path today ([Known issues](known-issues.md)).

**Is there a Flatpak?**
No, and not for want of trying: cosmic-comp does not expose the layer-shell and toplevel-management protocols a true drop-down needs to sandboxed clients. A sandboxed DropTerm could not drop down at all ([Known issues](known-issues.md)).

**Which platforms does it run on?**
DropTerm is built for every platform: COSMIC, GNOME, X11, macOS, and Windows ([portability plan](research/portability-plan.md)).

**How do I uninstall it?**
Remove the two `dropterm*` binaries from `/usr/bin` and the `io.github.l1br3_io.dropterm*` files under `/usr/share/{applications,icons,metainfo}/` - or, from a source checkout, `sudo just uninstall` ([README: Installing](https://github.com/l1br3-io/dropterm#installing)).

## Behavior

**Why does the terminal hide when I click another window? Can I stop that?**
That is `hide_on_focus_loss`, on by default because a hotkey-driven drop-down lives on focus - click away, it is gone; press the key, it is back, shell intact. Write `false` to the key (Behavior tab in settings) to keep the overlay up while you work elsewhere ([configuration](configuration.md#behavior-toggles)).

**Does my shell session survive the hide? What about a reboot?**
Each in its own way. Hiding preserves the *live* session - only the surface is destroyed, the PTY keeps running, so a `sleep` or an editor outlives every toggle. A reboot cannot preserve live processes; what restores is the layout: tabs with their directories, profiles, and renames save on change and come back on launch (`session_persistence_enabled` is the switch, on by default) ([sessions](sessions.md)).

**Does copied text survive hiding the terminal?**
Yes - the copy does not ride the terminal's surface at all: the pane's own copies ride GPUI's clipboard while it holds the keyboard, and every focus-free write (an OSC 52 store from a background tab, the primary selection) rides the data-control protocol, so your last copy outlives every hide/show cycle. And the copy itself no longer cares about focus: copying while the terminal is visible but unfocused (or even hidden) lands all the same, where it used to vanish silently until a restart. The mechanics are in [Known issues](known-issues.md).

**Why can't I select text while vim (or tmux, htop) is running?**
Those programs turn on mouse reporting, and the terminal hands every press to the program - that is the mode working as asked. Hold **Shift** to take the pointer back: Shift+drag selects, Shift+right-click opens the context menu, and Copy (`Ctrl+Shift+C`) copies that selection like any other. The same escape hatch alacritty, kitty, and gnome-terminal offer; the scroll wheel stays the program's even with Shift held.

**Why doesn't DropTerm appear in the dock or Alt-Tab? Can I get it back?**
By design, and no - the running terminal's window is never listed in the dock or the window switcher; it is summoned by its app-library icon (a cold click starts DropTerm and shows the dropdown, a warm click toggles it), the tray icon where the desktop hosts one, your shortcut, or the CLI, and the window stays out of both listings for its whole life. (Older versions had a dock-presence setting for this; a config file still carrying it loads fine, the now-unknown key is simply ignored.)

**Is there a tray icon?**
On desktops that host StatusNotifierItem - the COSMIC panel, KDE, XFCE, waybar - yes: left-click toggles the dropdown, and the menu carries About (version, description, and the support link in an About window) and Quit. Vanilla GNOME without the AppIndicator extension hosts no SNI, so there is simply no icon - the app-library, shortcut, and CLI summons are unaffected.

**Why does the window flash or recreate when it hides?**
Hiding destroys the layer surface and the next show builds a fresh one - deliberately, to sidestep a cosmic-comp bug other layer-shell clients hit when a surface is updated after its role is destroyed. You are seeing the designed path, not a glitch ([Known issues](known-issues.md)).

**How do I keep it from covering my panel or dock?**
It does by default: `respect_panel` ships as `true`, so the drop-down places itself inside the compositor's usable area on its configured layer. With an autohide panel expect a few pixels of reserved handle strip. To cover the panel instead, write `respect_panel: false` ([configuration](configuration.md#layer-focus-and-panels)).

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
