# Panel applet

A COSMIC panel applet - one icon in the panel that toggles the drop-down and shows at a glance whether DropTerm runs. It is a separate binary (`dropterm-applet`) shipped beside the terminal, not a mode of it: panel applets and layer-shell terminals live in different processes by design (building both into one binary would unify wgpu features and break panel rendering - the full reasoning is in the [research record](research/archive/v3-panel-applet.md)).

## What the button does

The icon is DropTerm's symbolic terminal glyph, tinted by state in three opacity tiers - dimmest when the terminal isn't running, dimmed while it runs hidden, full while the drop-down is visible. The tooltip spells the same state out: `DropTerm — not running`, or `DropTerm — visible, 3 tabs`.

A left click is state-driven:

- **Terminal running** - the click sends the same toggle a second CLI launch would, over the session bus. Fire-and-forget by design; debouncing is the terminal's job.
- **Terminal not running** - the click launches it with an xdg activation token, so the newly shown layer surface takes keyboard focus properly rather than appearing unfocused.

The applet watches the terminal's bus name and its read-only state interface (`Visible` and `TabCount` on the `…dropterm.V1` object), so the icon follows the terminal through its whole lifecycle - including a restart of either process - and stale property flips racing the terminal's exit can never resurrect a dead state.

## Installing

**From a release or `sudo just install`** - the applet binary and its desktop file (`io.github.l1br3_io.dropterm.Applet.desktop`) install alongside the terminal; nothing extra to do.

**Then add it to a panel**: COSMIC Settings → Desktop → Panel → Applets → add DropTerm. Like any COSMIC applet it can sit on any panel, in any slot you drag it to.

There is no Flatpak of the applet: cosmic-comp does not expose the layer-shell and toplevel-management protocols a drop-down needs to sandboxed clients, and the applet alone, without its terminal, is not worth a store listing ([Known issues](known-issues.md)).

## Building from source

`just build-applet` builds the applet crate alone; `just dev-applet` runs it standalone for development. The crate is a workspace member that depends on the terminal crate's library for the shared D-Bus state contract, so the two can never drift on what the applet reads.
