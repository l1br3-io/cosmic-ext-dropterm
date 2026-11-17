# Theme override: where does its regression history live?

Date: 2026-11-15

## TL;DR

The theme-override pipeline's three shipped regressions stay
documented, but in this record instead of the source: under the
comment gate added to `rust-best-practices` today, `src/app/theme_glue.rs`
carries only durable whys (the chain-not-then contract, the
background-before-rewalk ordering, the never-`active()` race warning),
and the bug narratives, walk dates, and test pins live here.

## Findings

1. **The pin that never reached the runtime ("dark mode does
   nothing").** `Task::then` consumes the first task's output and
   replaces it with the continuation's, so `set_theme(...).then(...)`
   silently dropped the `Action::AppThemeChange` message: the pin
   never reached the runtime and the theme stayed whatever it was.
   Fixed by riding `Task::chain`, which forwards every output of the
   first task and only then runs the second, preserving both messages
   and their order (commit `ce348e5`). Pinned by
   `theme_override_task_delivers_set_theme_then_reapply` in
   `src/app/theme_glue.rs`.

2. **The refresh-after ordering (the grayish cover, hardware walk
   2026-10-08).** With the window background refreshed after the tab
   rewalks, every default-background cell and the full-surface
   default quad wore the previous theme's background, healed only
   where later grid damage rewalked a line: a grayish cover over the
   editor on an Application theme flip, receding on focus and leaving
   one-line fragments. `set_config`'s rebuild snapshots the fallback
   per cell (`convert_color` reads the process global live), so the
   background must move before any rewalk. The narrative was carried
   by a source comment from commit `71b53cf`; the ordering constraint
   itself survives in the source.

3. **The watcher startup race (white background in dark mode,
   hardware walk 2026-10-12).** When the preference pins a mode,
   libcosmic's theme watcher, built before the pin lands, can deliver
   the opposite mode's `SystemThemeChange`; its handler writes the
   delivered palette into the process global while preserving only
   the `prefer_dark` marker, so `active()` reports the wrong mode with
   the pin still "set" and every later refresh re-derived the
   opposite-mode background under cells that correctly resolved the
   pinned scheme. Fixed by deriving the fallback from the caller's
   `effective_is_dark` decision (commit `d94a0de`). The race warning
   survives in `refresh_window_bg_color`'s doc comment.

4. **The tab-switch door.** The same stale-snapshot mechanism as
   finding 2 bites when a plain tab is switched in under a different
   window background; the `set_config` pass in
   `refresh_active_window_bg` heals it (the surviving doc comment
   carries the why).

## What DropTerm has today

The pure policy lives in `dropterm-core/src/theme_glue.rs` (carved in
commit `e651728`), imported at the frontend's crate root so
`crate::theme_glue` resolves; the impure shell in
`src/app/theme_glue.rs` (orchestration moved in commit `71b53cf`).

## Decision

Source comments pass the gate now written into the project's
`rust-best-practices` skill: the default is no comment; a why, a
warning about a non-local consequence, or a pointer earns a place;
length follows the content. Change history, bug names, walk dates,
and test pins move to this record. Rejected: keeping the narratives
in source (they bloat every read and rot as the code moves) and
deleting them outright (the mechanism stories are the evidence behind
the surviving ordering constraints, so they stay retrievable here).

## Sources

- Repo commits `ce348e5`, `d94a0de`, `e651728`, `71b53cf`
- The pre-trim comments of `src/app/theme_glue.rs`, visible in the
  git history of the commit that lands this record
- Hardware walks 2026-10-08 and 2026-10-12, as dated in those comments
