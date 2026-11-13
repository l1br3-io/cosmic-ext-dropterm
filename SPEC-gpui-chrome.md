# SPEC: gpui-chrome — settings drawer, context menu, pickers on the Kit

Status: not started · Phase 3 · lane: main (branch `port/gpui-chrome`, after
`gpui-tabs`, `gpui-status`, and `gpui-theme`)

## Objective

The chrome on Kit components (component-only, per C3): the settings drawer
(five pages, the profile manager form, the keybind editor), the terminal
context menu, and the pickers (font picker, scheme picker, and friends) —
everything `settings.rs` (2,539 lines) and `context_menu.rs` (451 lines)
do on libcosmic, rendered through the generated theme and writing through
core's config-store contract.

## Preconditions

`gpui-tabs` and `gpui-status` merged (the drawer opens over a populated
drawer); `gpui-theme` merged (chrome renders themed).

## Scope

- The settings drawer: the write contract unchanged — write-through per
  commit point, no Apply button — every control mapped to its config key,
  the five pages, profile create/edit/delete, the keybind editor with its
  capture UI.
- The context menu: rows and item→action mapping from core
  (`context_menu.rs` is pure model; picks land in the same path keybinds
  use), presented through Kit menu/popover components (the C2b-proven
  overlay path on a layer surface).
- The pickers: the font picker (enumeration from core's `font_list`),
  scheme picker (the registry's swatches), and any other chooser the
  libcosmic chrome has, on Kit list/popover components.
- The drawer handle (`drawer_handle.rs`) and its resize behavior on GPUI.

## Commands

    cargo run -p dropterm-gpui    # rig + VM walks

## Testing strategy

Control→key mapping is headless-testable (the libcosmic drawer's control
map tests port). Config-writing behavior pins against the store's existing
tests. Interaction rows walk on the rig and VM per the manual checklist's
settings rows; the config↔docs pin test (`every_config_key_is_documented`)
must keep passing — the drawer is a writer, and its key set cannot drift.

## Boundaries

Never: add an Apply button or batch writes (the write contract is the
seam); adopt gpui-shell/gpui-kit crates; change config keys or their docs
bindings.

## Success criteria

The settings, context-menu, and picker rows of the manual checklist pass on
COSMIC and sway; drawer writes and hand edits remain one event stream; the
`every_config_key_is_documented` pin stays green.

## Size

L.
