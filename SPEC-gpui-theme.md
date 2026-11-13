# SPEC: gpui-theme — the scheme→theme generator in-tree

Status: not started · Phase 3 · lane: main (branch `port/gpui-theme`, after
`gpui-scaffold`; parallel with `gpui-surface`/`gpui-terminal`)

## Objective

DropTerm's resolved scheme drives the Kit's theme: the scheme → theme-JSON
generator — proven by kit-pass C4 (spike `cd06740`, committed there as a
Phase 3 input) — lands in the main tree and applies through the sanctioned
path (`from_value::<ThemeConfig>` → `Theme::update(apply_config)`) before
any window opens, so every Kit surface and token reads the DropTerm scheme.

## Preconditions

`gpui-scaffold` merged (the Kit is in the graph). The carry happens before
any scratch-repo cleanup — [kit-root-border](docs/research/kit-root-border.md)
names the generator as the thing to carry out of `../dropterm-spike` first.

## Scope

- Carry `theme_gen.rs` from the spike branch (`spike/gpui-layer-shell` @
  `cd06740`, never merged as code) into the GPUI frontend crate, with its
  four RED-first unit tests (core mapping, name/mode pass-through,
  absent-stays-absent, output-parses-as-`ThemeConfig` — the `#[non_exhaustive]`
  struct means JSON, never struct literals).
- The mapping's rules stay C4's: DropTerm's resolved scheme (Alacritty
  `Colors`, core's theme glue) onto the 37 `ThemeConfig` token keys;
  surfaces ride Background/Foreground; status colors ride their ANSI slots
  with Bright variants as hover/active; primary/accent/link ride
  Blue/BrightBlue; the dim set rides BrightBlack; unset slots stay absent —
  no invented colors.
- The apply path: scheme changes (config pass) regenerate the theme and
  update before the next frame; live tabs and chrome follow edits the way
  the libcosmic build's re-resolution does.

## Commands

    cargo test -p dropterm-gpui theme_gen    # the carried tests
    cargo run -p dropterm-gpui               # the apply-path walk

## Testing strategy

The four carried unit tests pin the mapping headless. The apply path is
compositor-evidenced on the rig and VM: tokens read back the scheme
(background `#1A1B26`, primary `#7AA2F7` class rows), chrome renders
through them — C4's dark-band/blue-link/dark-popover screenshot class —
and a live scheme edit re-themes without restart.

## Boundaries

Never: construct `ThemeConfig` in code (the JSON path is the sanctioned
one; code-construction stays unproven); invent colors for unset slots;
change the scheme registry or resolution rules (core owns them).

## Success criteria

The generator lives in-tree with its tests green; scheme → theme applies
before first frame and on live edits on both compositors, evidenced by
token read-back and themed-chrome screenshots.

## Size

S.
