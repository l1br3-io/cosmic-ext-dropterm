# SPEC: keybind-docs — binding the summon key per desktop

Status: not started · Phase 1 · lane C · branch `port/keybind-docs` (main
checkout)

## Objective

A per-desktop page for binding `dropterm toggle`: sway `bindsym`, Hyprland
`bind`, KDE custom shortcuts, Xfce keyboard settings, beside the COSMIC
shortcut the docs already cover.

## Dependencies

`app-identity` merged (the page names the final command);
`reality-check`'s sessions (a desktop gets a row only after a session
verified it).

## Scope

One docs page (exact name and placement chosen at `/planning`), linked from
README's Documentation index. Rows: COSMIC, sway, Hyprland, KDE, Xfce —
each cites its Phase 0 session; desktops no session verified are marked
untested or unsupported.

## Commands

    just docs-check

## Testing strategy

`just docs-check` (link resolution). Content correctness is gated by the
Phase 0 session citations, not by tests.

## Boundaries

- Always: cite the Phase 0 session on every row; mark untested rows as
  such.
- Never: document a binding no session verified; commit `SPEC*.md` or
  `tasks/`.

## Success criteria

- Each row cites its Phase 0 session.
- The page is linked from README's Documentation index.
- `just docs-check` green.

## Size

S.
