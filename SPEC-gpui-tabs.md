# SPEC: gpui-tabs — the tab bar on GPUI

Status: not started · Phase 3 · lane: main (branch `port/gpui-tabs`, after
`gpui-terminal`)

## Objective

The tab strip on GPUI: buttons, `+`, gear, drag-reorder, inline rename,
per-tab menu, pins, borders — everything `tab_bar.rs` (2,778 lines) does on
libcosmic, over `dropterm-core`'s unchanged tab model (`tabs.rs`:
insert/close/reorder/cycle, title fallback, attention, close decisions).

## Preconditions

`gpui-terminal` merged (tabs contain live terminal sessions).

## Scope

The bar's view and interaction on GPUI primitives (or Kit list/popover
components where they fit): the active/close states, drag-reorder through
core's move model, pinning, the inline rename, the per-tab context menu
(core's `app/tab_actions.rs` funnel), the new-tab `+` and the gear into the
chrome slice's settings entry point, and the `TabPosition` layout rule.

## Commands

    cargo run -p dropterm-gpui    # rig + VM walks

## Testing strategy

Model-level behavior is already pinned headless in core (move, close,
title, attention). This slice's tests pin the view-model mapping (tab
state → row presentation, reorder gesture → move intent) headless;
interaction rows walk on the rig and VM per the manual checklist's tab
rows.

## Boundaries

Never: duplicate tab model logic in the view (core owns it); take a
dependency beyond the ruled pin set.

## Success criteria

The tab rows of the manual checklist pass on COSMIC and sway: reorder,
pin, rename, per-tab menu actions, close-confirm flow, attention state,
title modes.

## Size

M.
