# SPEC: gpui-status — the status band on GPUI

Status: not started · Phase 3 · lane: main (branch `port/gpui-status`, after
`gpui-terminal`; parallel with `gpui-tabs`)

## Objective

The status band on GPUI: the segment model rendered as a band (28 px
default) with the clock, the OSC-fed segments (working directory, command
exit codes via the OSC 133;D forks, profile name, custom segments), and
whatever `status_bar.rs` (3,445 lines) surfaces today — over core's
unchanged segment model and events.

## Preconditions

`gpui-terminal` merged (the band reads engine events).

## Scope

The band's view on GPUI: segment layout and truncation rules, the clock's
minute-boundary tick (core's jiff arithmetic), click actions where the
libcosmic band has them, and the config-driven show/hide and content rules
(`status_bar` config keys as they exist). OSC-fed content arrives as core
events; this slice only renders and routes it.

## Commands

    cargo run -p dropterm-gpui    # rig + VM walks

## Testing strategy

Segment-model behavior is already pinned headless in core. View-model
mapping tests headless; the band rows of the manual checklist walk on the
rig and VM (clock, OSC-fed updates, exit-code segments, config toggles).

## Boundaries

Never: parse OSC sequences in the view (core's engine does); change config
keys or segment semantics.

## Success criteria

The status-band rows of the manual checklist pass on COSMIC and sway; OSC
133;D exit codes surface as they do on libcosmic.

## Size

M.
