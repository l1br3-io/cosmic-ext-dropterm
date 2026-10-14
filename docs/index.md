---
title: Documentation
---

# DropTerm

A native, Yakuake/Guake-style drop-down terminal for the [COSMIC desktop](https://github.com/pop-os/cosmic-epoch) (Pop!_OS 24.04+, Wayland, cosmic-comp), written in Rust with [libcosmic](https://github.com/pop-os/libcosmic).

The terminal slides down from the top (or bottom) edge of the screen and is toggled with a global keyboard shortcut. It is a true layer-shell overlay — not a minimize/restore window trick — so it stays above other windows, is hidden from Alt-Tab and the dock, and keeps your shell session alive while hidden.

## Start here

- [Configuration](configuration.md) - all 43 config keys, profiles, and direct editing
- [Keybindings](keybindings.md) - every default chord and how to rebind
- [CLI reference](cli.md) - scripting the running instance: new tabs, the tab verbs, `--is-visible`
- [Panel applet](applet.md) - toggling DropTerm and reading its state from the COSMIC panel

## In depth

- [Settings drawer](settings.md) - the five sections and when each edit applies
- [Status bar](status-bar.md) - the 28 px glance band: segments, click-to-copy, selection, its menu
- [Session persistence](sessions.md) - what is saved, when, and how restore behaves
- [Theming](theming.md) - the Application theme pin and the 12 built-in color schemes
- [Slide animation](animation.md) - the 130 ms slide and its reversal rules
- [Bell notifications](bell.md) - hidden-terminal bells and the per-tab latch

## Building and contributing

- [Development](development.md) - the dev loop and running without a shortcut
- [Testing](testing.md) - the manual test checklist every change answers to
- [Architecture](architecture.md) - the module map and the load-bearing seams

## Decisions

- [Research & decision log](research/README.md) - why each non-trivial feature looks the way it does, with the rejected alternatives: peer surveys across terminals, panel-visibility protocol findings, packaging rulings.

Installing from a release or from source, the shortcut setup walk, credits, and the roadmap live in the [README](https://github.com/l1br3-io/cosmic-ext-dropterm#installing) and [ROADMAP](https://github.com/l1br3-io/cosmic-ext-dropterm/blob/main/ROADMAP.md) on GitHub.
