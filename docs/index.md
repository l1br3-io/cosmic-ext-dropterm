---
title: Documentation
---

# DropTerm

Bind `dropterm` to a key. The terminal drops down from the top (or bottom)
edge of the screen; press the key again and it rises — your shell session
still running behind it.

It is a true layer-shell overlay — not a minimize/restore window trick — so
it stays above other windows and is hidden from Alt-Tab and the dock. Your
session persists: tabs and their directories save on change and restore on
launch, reboots included ([sessions](sessions.md)).

It is built for every platform: COSMIC, GNOME, X11, macOS, and Windows
([the portability plan](research/portability-plan.md)). It follows the
desktop's theme preference, ships twelve built-in color schemes, and honors
the panel and dock unless you tell it not to ([theming](theming.md)).

## Start here

- [Common tasks](howto.md) — the exact key or verb for what you want to do
- [Configuration](configuration.md) — all 68 config keys, profiles, and direct editing
- [Keybindings](keybindings.md) — every default chord, and how to rebind
- [CLI reference](cli.md) — script the running instance: new tabs, the tab verbs, `--is-visible`
- [FAQ](faq.md) — the recurring questions, answered directly

## In depth

- [Settings window](settings.md) — the nine pages, and when each edit applies
- [Status bar](status-bar.md) — the 28 px glance band: segments, click-to-copy, selection
- [Session persistence](sessions.md) — what is saved, and how restore behaves
- [Theming](theming.md) — the theme pin, the 12 built-in schemes, your own scheme files
- [Slide animation](animation.md) — the 150 ms slide and its reversal rules
- [Bell notifications](bell.md) — hidden-terminal bells, and the per-tab latch
- [Inline images](images.md) — `imgcat` and OSC 1337; images render in the terminal and scroll away with the output
- [Release notes](releases.md) — what each tagged release changed

## Building and contributing

- [Development](development.md) — the dev loop, and running without a shortcut
- [Testing](testing.md) — the manual checklist every change answers to
- [Architecture](architecture.md) — the module map, and the load-bearing seams

## Decisions

- [Research & decision log](research/README.md) — why each non-trivial feature looks the way it does, rejected alternatives and all

The install walk, the shortcut setup, credits, and the roadmap live in the [README](https://github.com/l1br3-io/dropterm#installing) and [ROADMAP](https://github.com/l1br3-io/dropterm/blob/main/ROADMAP.md) on GitHub.
