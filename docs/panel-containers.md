# Panel containers

The host-owned panel containers are shell-side chrome the plugins
dock into - never a plugin surface themselves. Two kinds:

- The **right edge rail** (`panel_rail_enabled`, default off): the
  declared container plugin views dock into through
  `dropterm.panel_view` - [panel views](plugins.md#panel-views) is
  the plugin-facing contract. Off, nothing in the composition
  changes; on, the rail stands beside the drawer's column at its
  remembered width (280 px; nothing resizes it until a tenant needs
  to). The rail is display + click only - the terminal keeps the
  keyboard.
- **Overlay panels** (the images panel's genus): shell chrome behind
  the same container model (`src/panels.rs`), no plugin surface.

The container model's maintainer note lives in
[development.md](development.md); the walk rows in
[testing.md](testing.md).
