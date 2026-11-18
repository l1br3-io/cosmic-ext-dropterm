# SPEC: theme-defaults — first install opens GitHub-themed, following the system

Status: not started · post-cutover feature · main · spec drafted 2026-11-18,
approval pending

## Objective

How the theme works on first install today: a fresh install writes no
config, `DropTermConfig::default()` supplies `theme_preference =
System` with the COSMIC scheme pair — and then "System" resolves dark
at every call site, because the system input is the literal `true`
(`dropterm/src/app_shell.rs:133` `resolve_theme`,
`dropterm/src/terminal/view.rs` `boot_colors` and
`PaneRequest::for_command` — each commented "until the live
system-mode watch ships"). So every fresh install opens COSMIC Dark on
the opaque-black background fallback regardless of the desktop's
light/dark setting, the OS accent is never read (the accent rides the
active scheme's blue through `theme_gen`), and no "github" scheme
exists in the twelve-entry registry.

Owner answers 2026-11-18 (this module's shape):

1. Add a **GitHub Dark / GitHub Light pair** from GitHub's Primer
   palette and make them the default schemes.
2. Make **Follow system real** — read the OS dark/light preference and
   follow it, runtime flips included. "Which one it should choose"
   answers itself: dark OS → GitHub Dark, light OS → GitHub Light, and
   `theme_preference` stays defaulted to `System`.
3. When the OS preference is unreadable (no portal, no preference,
   read error): resolve **dark** (GitHub Dark).
4. The accent needs no new setting — it derives from the active
   scheme's blue (`theme_gen.rs:83`), so the GitHub schemes make the
   accent GitHub blue automatically.

## Scope

In:

- The two scheme entries in `core/theme_source.rs` (`SCHEMES`), full
  `ColorScheme` shape — foreground, **background** (real, not the
  `None` system-defer the COSMIC pair carries), cursor,
  normal/bright tables; dims derive through the existing 0.66
  conversion. Palette anchored on Primer: dark canvas `#0D1117`,
  light canvas `#FFFFFF`, the light/dark accent blues; the 16 ANSI
  slots cited from GitHub's own terminal palette
  (github/github-vscode-theme, also GitHub-maintained). Every color
  carries an upstream citation in the tests, the existing per-scheme
  spot-check pattern.
- The defaults flip: the config default pair (`color_scheme_dark` /
  `color_scheme_light`) and the unknown-scheme-name fallback in
  `theme_glue`'s `effective_colors` chain both become the GitHub
  pair, from **one** constant pair — registry, config defaults, and
  fallback agree by construction.
- The live system-mode watch: a system-mode source reading
  `org.freedesktop.portal.Settings`
  (`org.freedesktop.appearance` / `color-scheme`: 0 no-preference,
  1 prefer-dark, 2 prefer-light) — `ReadOne` at boot, the
  `SettingChanged` signal for runtime flips — with the pure
  value→dark mapping framework-free in core and the impure portal
  shell at the edge. `resolve_theme`, `boot_colors`, and
  `for_command` read it instead of the literal `true`; a flip
  re-themes through the existing identity-gated funnel (the tick's
  apply point), so geometry-only echoes still never re-theme.
- zbus only — 5.19 is already a dependency; no new crates.
- Docs: `docs/theming.md` (Follow system becomes true in the GPUI
  build; scheme list; the new defaults), `docs/configuration.md`
  default values, and the sweep of the "until the live system-mode
  read lands" comments.

Out: any accent picker or new accent setting; per-scheme accent
overrides; renaming or removing existing schemes; light/dark
scheduling; making the COSMIC pair's `None` background read the OS
palette (its system-defer semantics stay as they are).

## Commands

    just check
    just test
    just fmt

No new recipes.

## Files

`dropterm/src/core/theme_source.rs` (entries, order pin, citation
tests), `dropterm/src/core/config.rs` (the default-pair constants),
`dropterm/src/core/theme_glue.rs` or a sibling `core/system_mode.rs`
(the value mapping + fallback), `dropterm/src/app_shell.rs`
(`resolve_theme`, boot, the watch's wiring onto the tick),
`dropterm/src/terminal/view.rs` (`boot_colors`, `for_command`),
`docs/theming.md`, `docs/configuration.md`. Per-task ≤5-file lists
come at chunk time.

## Constraints

- Primer official palette; anchors above, every slot cited in tests
  to its upstream token name. If a dim/bright slot has no upstream,
  the existing derivation fills it and the test says so.
- The GitHub pair sets real backgrounds — a fresh install paints
  GitHub's canvas, never the opaque-black fallback the COSMIC pair's
  `None` background falls to.
- Boot never blocks on the portal read: the OPEN→FIRST_FRAME budget
  (the 130 ms slide) stands. If the read's answer has not landed when
  the boot apply runs, dark applies and the landed read corrects
  through the identity gate; a small bounded synchronous read is the
  plan's call, with the budget as the constraint.
- Fallback semantics: no-preference (0), portal absent, or read
  error all resolve dark — byte-identical to today's behavior for
  the bare-sway case.
- Existing scheme names are never renamed or removed; the COSMIC
  pair stays selectable in the pickers.
- House style: pure halves framework-free in core, impure shells at
  the edge; inline `#[cfg(test)]` tests; comment density per the
  tree.

## Testing strategy

Headless pins: the registry order with the GitHub entries; upstream
citation spot-checks per scheme; config defaults (GitHub pair,
`theme_preference` still `System`); the portal value mapping
(0/1/2/error → dark/dark/light/dark) and the fallback chain flip in
`theme_glue`; `resolve_theme` reading the source (no literal);
an identity flip re-theming exactly once; the accent landing on
GitHub blue through `theme_gen`'s existing Blue/BrightBlue mapping.

Owner-run manual rows (2026-11-14 ruling): fresh no-config install on
COSMIC light → GitHub Light; on dark → GitHub Dark; an OS flip
mid-run re-themes without restart; bare sway (no portal) → GitHub
Dark; a new tab opened under a light system carries light colors.

`just check` green on the result.

## Boundaries

- Always: `just check` before done; upstream citations with the
  palette; docs rows land in the same change.
- Ask first: new dependencies (none expected); touching the release
  workflow.
- Never: rename or remove a scheme name; run vm/platform walks
  agent-side or mark a runtime row green the owner did not report;
  block boot indefinitely on the portal read.

## Success criteria

1. Fresh install (no config) on a dark OS opens GitHub Dark
   (background `#0D1117`); on a light OS, GitHub Light (`#FFFFFF`) —
   VM row evidence, owner-run.
2. `theme_preference` still defaults to `System` (existing pins
   unchanged) and `System` now resolves through the real OS read;
   the `Dark`/`Light` pins are untouched.
3. The chrome accent is GitHub blue — light `#0969DA`, dark
   `#2F81F7` — through the existing scheme→accent mapping, no new
   setting.
4. A runtime OS flip re-themes without restart through the identity
   gate; geometry-only config echoes never re-theme (the existing
   gate's pin still passes).
5. No portal, no preference, or a read error resolves GitHub Dark.
6. An unknown scheme name in a config or profile resolves to the
   GitHub pair.
7. The COSMIC pair remains present and selectable; `just check`
   green; `docs/theming.md` and `docs/configuration.md` updated.

## Open questions

1. Boot-read shape: fallback-then-correct (no boot block, one
   possible corrective flip) versus a small bounded synchronous read
   (no flip, budget risk). Plan decides; the 130 ms budget is the
   constraint either way.
2. Registry position of the GitHub pair — proposal: directly after
   the COSMIC pair, the defaults region; whichever is chosen, the
   order test pins it.
3. Where the watch lives: on `dbus.rs`'s existing connection or its
   own — plan-level.

## Size

M — the registry pair and defaults flip are S; the watch with the
re-theme wiring is the M in it.
