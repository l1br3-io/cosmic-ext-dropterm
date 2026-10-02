# Research: terminal color schemes for DropTerm's Appearance settings

Date: 2026-10-02
Question: *How should DropTerm represent, ship, and apply user-selectable terminal
color schemes (16 ANSI colors, fg/bg, cursor, bright/dim variants), and how should
they interact with the system light/dark preference?*

## TL;DR

**Copy cosmic-term's model — it is the same stack solving the same problem.**
cosmic-term (pop-os/cosmic-term, master `3ef31d7`, 2026-10-01) represents a scheme
as a serde struct `ColorScheme` (`name`, `foreground`, `background`, `cursor`,
`bright_foreground`, `dim_foreground`, plus `normal`/`bright`/`dim` tables of 8
named colors each, all `Option<HexColor>`) defined in `src/config.rs`. User schemes
live inside the cosmic-config config file as `BTreeMap<ColorSchemeId, ColorScheme>`
— one map per dark/light kind — and the *selected* scheme per mode is a plain
`String` name (`syntax_theme_dark` / `syntax_theme_light`). Schemes are applied by
converting to `alacritty_terminal::term::color::Colors` (a `[Option<Rgb>; 269]`)
and copying slots into each terminal on config change. DropTerm already vendors
the last two steps (`src/terminal/theme_source.rs`, `engine.rs::set_config`), so
the work is: a real `ColorScheme` type, a named scheme registry (built-ins
compiled in — cosmic-term's repo ships 30 importable RON files under
`color-schemes/`), and a picker in the Appearance settings section instead of the
raw `color_scheme_dark`/`color_scheme_light` strings currently buried in Advanced.
Light/dark auto-switching already works end-to-end in DropTerm via libcosmic's
`system_theme_update` hooks — nothing new is needed there for a scheme list.

Note: **util-linux `terminal-colors.d(5)` does not apply** — it configures color
output of CLI utilities (dmesg, ls, …), not terminal-emulator palettes. There is
no freedesktop standard for emulator schemes; every emulator rolls its own
format. The industry-consensus field set for a minimal-but-complete scheme is:
name/metadata, foreground, background, 16 ANSI colors, cursor (+ optional cursor
text), optional selection fg/bg, optional bright/dim variants (derivable).

## What DropTerm does today

- Two hardcoded schemes — `theme_source::cosmic_dark()` / `cosmic_light()`,
  vendored from cosmic-term `src/terminal_theme.rs` @ `9129277`
  (`src/terminal/theme_source.rs` header, lines 1-17). Both deliberately leave
  `NamedColor::Background` unset so the window background follows the COSMIC
  theme via the `WINDOW_BG_COLOR: AtomicU32` process-global
  (`src/terminal/engine.rs:183`, refreshed in `src/app.rs:354-363`).
- Config keys `color_scheme_dark` / `color_scheme_light: String` (defaults
  `"COSMIC Dark"` / `"COSMIC Light"`, `src/config.rs:16-17,104-106,143-144`),
  resolved by name in `app.rs:323-335 active_colors()` using
  `cosmic::theme::is_dark()`; unknown names fall back to the matching built-in.
- In the settings UI these two keys are **raw text fields in the Advanced
  section** (`src/settings.rs:83-86`), not a picker.
- System light/dark flips already re-apply colors: DropTerm overrides both
  `Application::system_theme_update` and `system_theme_mode_update`
  (`src/app.rs:1866-1883`) → `apply_config_to_tabs()`.
- Selection is rendered by swapping fg/bg (`src/terminal/engine.rs:890,915`) —
  same as cosmic-term (which carries a `//TODO: better handling of selection`).
- Deps in place: `alacritty_terminal = "0.25.1"`, `ron = "0.12"`, `serde`
  (`Cargo.toml`). cosmic-term additionally uses `hex_color = "3"` (MIT OR
  Apache-2.0) for `#RRGGBB` parsing.

## Findings

### 1. cosmic-term: scheme model, storage, and application

All paths below are [pop-os/cosmic-term](https://github.com/pop-os/cosmic-term)
@ master `3ef31d750d3b09a73f266923b0d64c5df038cf9f` (2026-10-01; `Cargo.toml`
says version 1.9.0, license GPL-3.0-only, `alacritty_terminal = "0.25.1"`,
`cosmic-text = "0.19"`, `hex_color = "3"`, `ron = "0.11"` — same engine and
color-text stack as DropTerm).

**Types** (`src/config.rs`):

- `ColorSchemeKind { Dark, Light }` (L45-49) — every scheme is tagged with the
  mode it belongs to; the picker shows one list per kind (Dark/Light tabs).
- `ColorSchemeId(pub u64)` (L51-53) — opaque id, allocated as
  `last_key_value().id + 1` on import (`src/main.rs` `ColorSchemeImportResult`,
  ~L2126-2146).
- `ColorSchemeAnsi` (L76-127) — `black, red, green, yellow, blue, magenta, cyan,
  white: Option<HexColor>`, with `de_color_opt`/`ser_color_opt` serde shims so
  the on-disk RON is a plain `"#RRGGBB"` string and missing fields are `None`
  (default, `deny_unknown_fields`, skip-if-empty on serialize).
- `ColorScheme` (L142-182) — `name: String` + `foreground, background, cursor,
  bright_foreground, dim_foreground: Option<HexColor>` + `normal`, `bright`,
  `dim: ColorSchemeAnsi`. This is exactly the "16 ANSI + fg/bg/cursor +
  bright/dim fg + dim table" set; there are **no selection fields**.
- `AppTheme { Dark, Light, System }` (L20-43) with `fn theme() -> theme::Theme`
  built from `theme::system_dark()/system_light()/system_preference()` plus a
  `theme_type.prefer_dark(...)` nudge.
- `Config` (L219-244) holds `color_schemes_dark` and `color_schemes_light:
  BTreeMap<ColorSchemeId, ColorScheme>` (user schemes live **inside the
  cosmic-config entry**, not as loose files), `syntax_theme_dark` /
  `syntax_theme_light: String` (selected scheme *name* per mode, defaults
  `COSMIC_THEME_DARK`/`LIGHT`), and per-profile overrides
  `Profile::syntax_theme_dark/light` (L188-217). Resolution:
  `Config::syntax_theme(kind, profile_id)` (L369-386); mode choice:
  `Config::color_scheme_kind(&system_theme)` (L295-307, honoring AppTheme).
  `color_scheme_names()` (L309-330) de-duplicates names ("Name (2)") and sorts
  linguistically.

**Conversion to alacritty colors** (`src/terminal_theme.rs`):

- `impl From<&ColorScheme> for Colors` (L44-96): starts from `auto_colors()`
  (L12-42 — the ransid 256-color base: 6×6×6 cube for 16..=231 with
  `v * 0x28 + 0x28` scaling, grays `(v - 232) * 10 + 8` for 232..=255), then
  fills the `NamedColor` slots. `None` fields leave the slot `None`
  (alacritty_terminal falls back / warns at render).
- `impl From<(&str, &Colors)> for ColorScheme` (L98-146) — the export
  direction; `export()` (L258-290) round-trips and asserts equality over all
  `color::COUNT` slots before writing `color-schemes/{name}.ron`.
- Built-ins `cosmic_dark()`/`cosmic_light()` (L148-242) leave `Background`
  unset with the comment *"Background comes from theme settings"*
  (L188/L236). `terminal_themes()` (L245-256) returns just these two as
  `HashMap<(String, ColorSchemeKind), Colors>`.

**The shipped RON files**: the repo tree has **30 files under
`color-schemes/*.ron`** (COSMIC Dark/Light, Dracula, Gruvbox Dark, Solarized
Dark/Light, Tango Dark/Light, Selenized ×4, Modus ×4, Rose Pine ×3, Pop Dark,
Rxvt, XTerm, Linux Console, OneHalfDark, Everforest, Seashells,
seal-brown-amber, …). Notably **nothing in `src/` reads that directory** (grep
for `read_dir`/`include` over the tree: zero hits) — at master they are an
import source for users (Settings → Color Schemes → import file dialog,
`src/main.rs` ~L2115-2150 `ron::de::from_reader::<_, ColorScheme>`) and the
contribution point for new schemes (recent commits: "Add seal brown and amber
terminal color scheme" `f9d893b8`, 2025-12-20). The two COSMIC RONs have **no
`background` field** (matching the unset-Background behavior), while third-party
ones like `Dracula.ron` set `background: "#1e1f29"`.

**Application at runtime** (`src/terminal.rs`, `src/terminal_box.rs`):

- `Terminal::set_config(config, color_scheme_kind, themes)` (L642-703): looks
  up `themes.get(&config.syntax_theme(kind, profile))`, and if found copies any
  changed slot `for i in 0..color::COUNT`; then `update_default_colors()`
  rebuilds the cosmic-text `Metadata` (default cell bg/fg pair) and requests a
  redraw. Unknown scheme name → colors silently unchanged.
- `WINDOW_BG_COLOR: AtomicU32` (L125) is the fallback when
  `NamedColor::Background` is `None` (`convert_color`, L129-140;
  `effective_color`, L388-399); the widget paints cell backgrounds from
  `Metadata` with opacity as alpha (`terminal_box.rs` ~L461-506) and reads the
  cursor color as `colors[NamedColor::Cursor]` (~L820-821).
- The app keeps `themes: HashMap<(String, ColorSchemeKind), Colors>` built by
  `update_color_schemes()` (`src/main.rs` L546-587): the two built-ins plus
  config-defined schemes; a custom scheme with a built-in's name **replaces**
  it with a `log::warn`.

**Light/dark switching** (`src/main.rs`):

- `Application::system_theme_update` override (L3551-3557) forwards to
  `Message::SystemThemeChange` → `update_config()` (L673-710), which
  recomputes the scheme kind, refreshes `WINDOW_BG_COLOR` from
  `theme.cosmic().background(theme.transparent).base`, calls `set_config` on
  every terminal, and finishes with `cosmic::command::set_theme(theme)` for the
  toolkit palette.
- Import/export: `ColorSchemeExport(…)` opens a save dialog defaulting to
  `{name}.ron` and writes `ron::ser::to_string_pretty(&color_scheme,
  PrettyConfig::new())` (L2000-2096).

**Could DropTerm reuse a crate?** No — `ColorScheme` lives in cosmic-term's
binary crate (not published), and there is no shared scheme crate; libcosmic's
`cosmic-theme` is the *toolkit* palette, not ANSI schemes. The options are
vendor the ~150 lines of `config.rs` types + `terminal_theme.rs` conversions
(DropTerm already vendored the palettes; both projects are GPL-3.0-only), or
write a compatible struct from scratch.

### 2. alacritty_terminal 0.25.1: the color model we embed

From the local cargo registry
(`…/registry/src/index.crates.io-…/alacritty_terminal-0.25.1/src/term/color.rs`,
Apache-2.0):

- `pub const COUNT: usize = 269` (L6) and `pub struct Colors([Option<Rgb>;
  COUNT])` (L21-22) with `Index<usize>`/`Index<NamedColor>` (both `Output =
  Option<Rgb>`). Layout per its doc table (L8-20): 0..16 named ANSI, 16..232
  color cube, 233..256 grayscale ramp (the table's 233 is an upstream
  off-by-one in the comment; both ransid and alacritty fill 232..=255/256),
  256 Foreground, 257 Background, 258 Cursor, 259..267 dims, 267 bright
  foreground, 268 dim background/dim foreground.
- `NamedColor` (vte 0.15.0 `src/ansi.rs` L1013-1072, re-exported as
  `alacritty_terminal::vte::ansi`): `Black = 0 … White = 7`,
  `BrightBlack = 8 … BrightWhite = 15`, `Foreground = 256`, `Background`,
  `Cursor`, `DimBlack … DimWhite` (259..266), `BrightForeground`,
  `DimForeground` (268). `Rgb` is the plain `{r,g,b}: u8` struct from the same
  module.
- There is **no `List` or `TERM_FG`/`TERM_BG` in the library crate** — those
  are render-side concepts in the alacritty *app* crate:
  `alacritty/src/display/color.rs` has `pub struct List([Rgb; COUNT])` built
  via `fill_named`/`fill_cube`/`fill_gray_ramp` from the config `Colors`
  (`alacritty/src/config/color.rs`, `pub struct Colors` at L9). Two details
  worth stealing if we ever derive values: `DIM_FACTOR: f32 = 0.66` (dims are
  auto-derived as `normal * 0.66` when unset, and `dim_foreground` defaults to
  `foreground * 0.66`), and alacritty's cube uses `r * 40 + 55` per channel
  while cosmic-term's ransid base uses `v * 0x28 + 0x28` (= *40 + 40) — a
  minor divergence that only affects indexed colors 16..=231.
- **Selection is not a `NamedColor`** — alacritty applies
  `colors.selection = {text, background}` at render time with
  `CellForeground`/`CellBackground` sentinels (see §3). cosmic-term/DropTerm
  instead swap fg/bg for selected cells (§1).

### 3. Alacritty's `colors:` TOML format

Primary source: `extra/man/alacritty.5.scd` @ alacritty master
(<https://github.com/alacritty/alacritty/blob/master/extra/man/alacritty.5.scd>,
"# COLORS" section at L294+). Field inventory:

- `colors.primary.{foreground, background, dim_foreground, bright_foreground}`
  — bright_foreground only used when `draw_bold_text_with_bright_colors`;
  dim_foreground *"automatically calculated based on the foreground color"*
  when unset.
- `colors.cursor` / `colors.vi_mode_cursor` — `{text, cursor}`, each a hex
  color or the sentinels `CellForeground`/`CellBackground`; default
  `{ text = "CellBackground", cursor = "CellForeground" }`.
- `colors.search.{matches, focused_match}` and
  `colors.hints.{start, end}` — `{foreground, background}` (search UI).
- `colors.line_indicator`, `colors.footer_bar` — `{foreground, background}`.
- `colors.selection` — `{text, background}` with the same sentinels; default
  swaps the cell colors.
- `colors.normal` / `colors.bright` / `colors.dim` — 8 named colors each
  (black, red, green, yellow, blue, magenta, cyan, white); *"If the dim colors
  are not set, they will be calculated automatically based on the normal
  colors"*.
- `colors.indexed_colors` — `[{ index = <16..256>, color = "#…" }]` overriding
  cube/ramp slots; `colors.transparent_background_colors` (L481) and
  `colors.draw_bold_text_with_bright_colors` (L489).
- Colors are strings `#RRGGBB`. `draw_background` **does not exist** in the
  current reference (0 hits in the scd); it was a legacy YAML-era key —
  transparency is `window.opacity` today. Don't plan a config field around it.
- Theme files are imported via the top-level `import = ["path.toml", …]` key
  (L31-48; loaded in order, importing file last, missing files skipped). The
  official collection
  [alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme)
  ships **177 TOMLs, Apache-2.0**, each exactly `# Colors (name)` +
  `colors.primary`/`normal`/`bright` blocks (checked
  `themes/gruvbox_dark.toml`) — i.e. a scheme there is *not* required to
  define cursor/selection/dim.

### 4. Industry conventions for named schemes

| Emulator | Scheme fields | Selected via | Light/dark |
|---|---|---|---|
| **Alacritty** (§3) | primary (fg/bg/dim/bright fg), 16 colors, optional dim table, cursor/vi cursor, selection, search/hints, indexed | theme TOML + `import`, or inline `colors:` | no auto light/dark; users keep two files |
| **kitty** | `foreground`, `background`, `cursor`, `selection_foreground`, `selection_background`, `color0`-`color15` (+ optional `cursor_text_color`, etc.) | themes kitten; a theme is "just a `.conf` file" copied to `~/.config/kitty/current-theme.conf` + `include` in kitty.conf | **auto**: `dark-theme.auto.conf` / `light-theme.auto.conf` / `no-preference-theme.auto.conf` (kitty ≥ 0.38.0) chosen from the OS color scheme; changes live |
| **GNOME Terminal** | profile keys: `palette` (16 hex strings), `foreground-color`, `background-color`, `bold-color`(+`-same-as-fg`), `cursor-colors-set` + `cursor-background/foreground-color`, `highlight-colors-set` + `highlight-background/foreground-color`, `use-theme-colors` | dconf profile; built-in palettes in the "Colors" tab picker | `use-theme-colors` follows the GTK theme |
| **iTerm2** | `.itermcolors` plist: `Ansi 0-15 Color`, `Background/Foreground/Bold/Cursor/Cursor Text/Selection/Selected Text Color` (+ Link), each a dict of `Red/Green/Blue Component` floats 0..1 | "Color Presets…" menu; import/export files; preset name from filename | profiles may carry separate light & dark color sets ("Use separate colors for light and dark mode") |
| **Windows Terminal** | `schemes[]` JSON object: `name`, `cursorColor`, `selectionBackground` (both optional), `background`, `foreground`, 16 colors `black..white` + `brightBlack..brightWhite` (uses `purple`, not `magenta`) | per-profile `colorScheme` string in settings.json | no scheme-level dark/light pairing; themes app / manual |
| **base16 / tinted** | `scheme:`, `author:`, `base00`-`base0F` (00-07 = bg→fg ramp; `base02` selection bg, `base05` fg/caret; 08-0F = accents mapped to ANSI by templates) | templates per app; tinty CLI | scheme is one variant; families ship dark+light as separate files |

Sources: kitty themes docs
(<https://sw.kovidgoyal.net/kitty/kittens/themes/> — mechanism, user-theme
override by name in `~/.config/kitty/themes`, auto light/dark section) and
theme-file shape (`kitty-themes` repo, 413 `.conf` files;
`themes/Solarized_Dark.conf` sets `background/foreground/cursor/
selection_background/selection_foreground/color0-15` plus `## name:/## author:/
## license:/## blurb:` metadata headers; option defaults in
`kitty/options/definition.py` L2523-2800). GNOME Terminal
`src/org.gnome.Terminal.gschema.xml` @ gitlab master (keys and defaults quoted
at L141-200, L315+, L346). iTerm2
<https://iterm2.com/documentation-preferences-profiles-colors.html> + a sample
`.itermcolors` from mbadolato/iTerm2-Color-Schemes (key list above). Windows
Terminal
<https://learn.microsoft.com/en-us/windows/terminal/customize-settings/color-schemes>
and fragment extensions
(<https://learn.microsoft.com/en-us/windows/terminal/json-fragment-extensions> —
a fragment scheme "must define a name … and every color in the color table").
base16: `styling.md` in chriskempson/base16 (role table) and
tinted-theming/base16-schemes `solarized-dark.yaml`; the successor spec
`tinted8` (tinted-theming/home `specs/tinted8/styling.md`, 0.2.0-beta) adds
`variant: dark|light`, `scheme.slug`, and inherits for `syntax`/`ui`.

**Minimal-but-complete field set** (the intersection everyone agrees on, plus
what our engine can consume): `name` (+ ideally author/license metadata),
`foreground`, `background`, 16 ANSI colors, `cursor` (cursor-text optional),
and *optionally* selection fg/bg and dim/bright variants. cosmic-term's
`ColorScheme` = exactly this minus selection; alacritty's = this plus
search/hints/indexed; WT requires name + all 16 and treats cursor/selection as
optional.

### 5. How a scheme list is usually shipped

Four patterns, all in active use:

1. **Compiled-in built-ins** — cosmic-term's two hardcoded palettes; Windows
   Terminal's schemes inside `defaults.json`; kitty's embedded themes
   (overridable by a same-named file in `~/.config/kitty/themes`). Pros: no
   I/O, versioned with the app, always present for fallback.
2. **A repo of importable files** — alacritty-theme (177 TOMLs),
   kitty-themes (413), cosmic-term's `color-schemes/` (30 RONs, imported via
   file dialog), iTerm2's preset gallery. The app doesn't scan them; users
   import.
3. **User files auto-discovered from a config dir** — kitty's
   `~/.config/kitty/themes/*.conf` (overrides builtins by theme *name*); WT
   fragments in defined locations. This is the hand-editable, dotfiles-friendly
   option.
4. **Schemes stored inside the app config** — cosmic-term's
   `color_schemes_dark/light` maps in the cosmic-config entry (created via its
   editor UI or import); WT's `schemes[]` in settings.json. No fs watching,
   atomic saves, but schemes aren't shareable files without import/export.

**freedesktop/XDG note**: the only XDG-adjacent convention,
`terminal-colors.d(5)` (util-linux 2.42.4,
<https://man.archlinux.org/man/terminal-colors.d.5.en>), configures *"output
colorization for various utilities"* — `disable`/`enable`/`scheme` files for
CLI tools like dmesg, keyed per utility and `$TERM*. It is consumed by
util-linux utilities, not terminal emulators, and its scheme files map logical
names (e.g. `error`) to ANSI sequences — nothing to do with a 256-color
palette. **Do not adopt or claim compatibility with it for DropTerm's scheme
files.** Emulator schemes have no standard location; the closest convention is
"files in a subdir of the app's XDG config dir" (kitty).

### 6. System light/dark mode + libcosmic (pinned rev `08a09e3`)

Verified in the local checkout
(`~/.cargo/git/checkouts/libcosmic-41009aea1d72760b/08a09e3/`, i.e.
[pop-os/libcosmic @ 08a09e3](https://github.com/pop-os/libcosmic/tree/08a09e3dfa1f729d52585fcedd6e731130235363)):

- `cosmic::theme` free functions (`src/theme/mod.rs`): `active()` L57,
  `active_type()` L63, `is_dark()` L77, `system_dark()` L88,
  `system_light()` L103, `system_preference()` L119 (reads `ThemeMode::config`
  from cosmic-config), `ThemeType::prefer_dark` L175.
- The `Application` trait hooks (`src/app/mod.rs`):
  `fn system_theme_update(&mut self, keys: &[&'static str], new_theme:
  &cosmic_theme::Theme) -> Task<Message>` (L470-476) and
  `system_theme_mode_update(…, &cosmic_theme::ThemeMode)` (L479-485), both
  default `Task::none()`. They are invoked from the runtime's
  `Action::SystemThemeChange` handler (`src/app/cosmic.rs:971`, mode-gated at
  L961-964) and the mode-change path (`src/app/cosmic.rs:1085-1102`, which
  re-resolves `theme::system_dark()/light()` and calls both hooks). At this rev
  there is **no `window.theme_changed` or `Settings::theme` callback** — those
  names don't exist in `src/` (grep: zero hits); the hooks + `core.system_theme()`
  are the API.
- `cosmic::command::set_theme(theme)` (`src/command.rs:36`) switches the
  app-level toolkit palette; `Core` tracks `system_theme` / `system_theme_mode`.
- Reference apps: **cosmic-term** overrides `system_theme_update` →
  `SystemThemeChange` → `update_config()` (§1) — re-pick scheme kind by
  AppTheme, refresh `WINDOW_BG_COLOR`, re-`set_config` terminals, then
  `set_theme`. **cosmic-edit** (master) uses the same `AppTheme
  {Dark,Light,System}` + `theme()` helper in `src/config.rs` L14-33 and calls
  `cosmic::command::set_theme(self.config.app_theme.theme())` on change
  (`src/main.rs:725`). kitty's `*-theme.auto.conf` trio (§4) is the
  non-libcosmic precedent for the same UX: name a dark scheme and a light
  scheme once, follow the OS forever.

## What applies to DropTerm

### Recommended design (phased)

**Phase 1 — named built-ins + picker (no new subsystems):**

1. **Adopt cosmic-term's `ColorScheme`/`ColorSchemeAnsi`/`ColorSchemeKind`
   types** (vendor ~150 lines from `src/config.rs` @ `3ef31d7`, keeping the
   SPDX/copyright header like `theme_source.rs` already does). Field list is in
   §1; it is a strict superset of what dropterm's engine consumes today and a
   subset of alacritty's TOML, so importing alacritty-style themes later is
   lossless-ish (search/hints colors dropped).
2. **Keep the conversion tail** already vendored: add cosmic-term's
   `From<&ColorScheme> for Colors` to `theme_source.rs` and route the existing
   `cosmic_dark()`/`cosmic_light()` through `ColorScheme` (their palettes are
   byte-identical to `color-schemes/COSMIC Dark.ron` / `COSMIC Light.ron`,
   which conveniently have no `background` field — preserving the
   follow-COSMIC-theme background behavior).
3. **Ship a curated set of built-ins compiled in** (`include_str!` +
   `ron::from_str` into a `once_cell`/const table, or code-generated). Directly
   reusable: the 30 RONs in cosmic-term's `color-schemes/` (GPL-3.0-only, same
   as DropTerm — see licensing below). A scheme with a `background` field
   overrides the COSMIC window background — that's the desired behavior for
   e.g. Dracula; `WINDOW_BG_COLOR` remains the fallback for the two COSMIC
   schemes (and `refresh_window_bg_color()` already keeps it fresh).
4. **UI**: move scheme selection into the Appearance section as a dropdown
   (one choice for dark mode, one for light — or a single choice + "COSMIC
   Dark/Light for the other mode" if the settings spec prefers fewer rows).
   The existing `color_scheme_dark`/`color_scheme_light: String` config fields
   stay as-is — they already store the scheme *name*, and `active_colors()`
   already falls back gracefully on unknown names. That keeps
   `DropTermConfig` at `#[version = 1]` with no migration.
5. **Light/dark**: nothing to build. `system_theme_update` /
   `system_theme_mode_update` → `apply_config_to_tabs()` already re-resolve
   `active_colors()` on OS flips. (If an explicit app theme override is ever
   wanted, copy cosmic-edit/cosmic-term's `AppTheme` tri-state + `set_theme`,
   and consult `config.color_scheme_kind()` instead of raw `is_dark()`.)
6. **Tests**: round-trip `ColorScheme ↔ Colors` over all `color::COUNT` slots
   exactly like cosmic-term's `export()` assert; unit-test name lookup,
   fallback-on-unknown-name, and that COSMIC built-ins leave Background unset.

**Phase 2 — user schemes (pick one):**

- **Option A, config-stored (cosmic-term's choice)**: `BTreeMap<u64,
  ColorScheme>` fields in the config, edited in a future scheme editor UI
  and/or RON import. No directory watching; atomic saves via cosmic-config;
  consistent with how everything else in DropTerm persists.
- **Option B, directory-scanned (kitty's choice)**: load
  `$XDG_CONFIG_HOME/dropterm/color-schemes/*.ron` at startup (and on config
  reload), later-wins/file-name-overrides builtins by scheme name. Better for
  dotfiles and hand-editing; needs a small loader + error surface
  (cosmic-term collects parse errors into `color_scheme_errors` shown in the
  UI).
- Either way, RON is the right format: it's what cosmic-term exchanges, it
  matches the app's serde-based config stack, and `ron` is already a
  dependency. Adding alacritty TOML import later is a small serde matter
  (parse `colors:` tables into the same struct).

**Selection colors**: leave out of v1 — cosmic-term has none either (fg/bg
swap, with an upstream TODO). If added later, they are engine/widget-level
(render-time override like alacritty's `selection {text, background}`), not
`NamedColor` slots, so the scheme struct can grow
`selection_foreground/background: Option<HexColor>` without touching
`alacritty_terminal` types.

### Licensing

- **cosmic-term**: GPL-3.0-only (`Cargo.toml`, file headers). **DropTerm is
  GPL-3.0-only** (`Cargo.toml` L6, `LICENSE`) — copying its structs, conversion
  code, and the 30 RON scheme files is license-compatible. Keep the
  SPDX/copyright attribution header as `theme_source.rs` already does.
  Individual scheme *designs* upstreamed from elsewhere (Dracula — MIT,
  Solarized — Ethan Schoonover, Gruvbox, Rose Pine — MIT, Everforest — MIT,
  base16 schemes — various) are color values with metadata; cosmic-term ships
  them under its repo license, which is what we'd inherit.
- **alacritty / alacritty_terminal**: Apache-2.0 (the 0.25.1 registry
  tarball's `Cargo.toml` says `license = "Apache-2.0"`) —
  GPL-3.0-compatible if we port conversion logic or theme TOMLs from
  [alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme)
  (repo license Apache-2.0, 177 themes).
- **kitty-themes**: each `.conf` carries its own `## license:` header (e.g.
  Solarized Dark = MIT) — per-file compatible with GPL-3.0-only in the
  "MIT-licensed data" sense, but requires per-file checking; not the path of
  least resistance.
- **mbadolato/iTerm2-Color-Schemes**: mixed/unspecified per-scheme attribution
  — **avoid as a bulk source**.
- Deps: `hex_color` 3.0.0 = MIT OR Apache-2.0; `ron` 0.12 = MIT OR Apache-2.0.

## Unverified / open questions

- Whether COSMIC packagers install cosmic-term's `color-schemes/` anywhere for
  users (no runtime consumer exists in the repo; packaging scripts were not
  inspected). Doesn't affect the compiled-in recommendation.
- cosmic-term's `Colors` doc-table says grays occupy "233..256" while both
  fillers use 232..=255/256 — upstream comment inconsistency only; slot 232 is
  the first gray either way. Harmless, but don't copy the comment verbatim.
- Exact behavior when a config-selected scheme name disappears from the
  registry: cosmic-term keeps the previous `Colors` silently;
  DropTerm currently falls back to the COSMIC built-in. Either is defensible —
  pick one and pin it with a test.
- libcosmic's in-flight "theme-v2" work (referenced in cosmic-term's
  `Cargo.toml` patch comments) may change the `cosmic::theme` API at a future
  rev-pick; everything cited here is from the pinned `08a09e3`.

## Sources

- pop-os/cosmic-term @ `3ef31d750d3b09a73f266923b0d64c5df038cf9f`
  (master, 2026-10-01): `src/config.rs` L20-49, L51-53, L76-182, L188-244,
  L295-330, L369-386; `src/terminal_theme.rs` (whole file, 290 lines);
  `src/terminal.rs` L125-140, L388-399, L642-710; `src/terminal_box.rs`
  L461-506, L820-821; `src/main.rs` L519-587, L673-710, L2000-2150, L2802-2812,
  L3551-3557; `Cargo.toml`; `build.rs`; `color-schemes/` (30 files, incl.
  `COSMIC Dark.ron`, `Dracula.ron`) — <https://github.com/pop-os/cosmic-term>
- alacritty_terminal 0.25.1 (local cargo registry, Apache-2.0):
  `src/term/color.rs` L1-60 — plus vte 0.15.0 `src/ansi.rs` L1013-1072
  (`NamedColor`)
- alacritty @ master: `extra/man/alacritty.5.scd` (COLORS section L294-490,
  `import` L31-48); `alacritty/src/config/color.rs`; `alacritty/src/display/
  color.rs` (`List`, `DIM_FACTOR`) — <https://github.com/alacritty/alacritty>;
  themes: <https://github.com/alacritty/alacritty-theme> (Apache-2.0)
- kitty: themes kitten docs
  <https://sw.kovidgoyal.net/kitty/kittens/themes/>; `kitty/options/definition.py`
  L2523-2800; `kittens/themes/main.py` (CLI options); `docs/remote-control.rst`
  L180-184; theme files: <https://github.com/kovidgoyal/kitty-themes>
  (413 files; `themes/Solarized_Dark.conf`)
- GNOME Terminal @ gitlab master: `src/org.gnome.Terminal.gschema.xml`
  L141-200, L315-330, L346 — <https://gitlab.gnome.org/GNOME/gnome-terminal>
- iTerm2: <https://iterm2.com/documentation-preferences-profiles-colors.html>;
  sample `.itermcolors` from
  <https://github.com/mbadolato/iTerm2-Color-Schemes> (`schemes/Solarized Dark
  Patched.itermcolors`)
- Windows Terminal:
  <https://learn.microsoft.com/en-us/windows/terminal/customize-settings/color-schemes>
  and
  <https://learn.microsoft.com/en-us/windows/terminal/json-fragment-extensions>
- base16 / tinted: `styling.md` in <https://github.com/chriskempson/base16>;
  <https://github.com/tinted-theming/base16-schemes> (`solarized-dark.yaml`);
  <https://github.com/tinted-theming/home> `specs/tinted8/styling.md`
- util-linux: `terminal-colors.d(5)` (2.42.4) —
  <https://man.archlinux.org/man/terminal-colors.d.5.en>
- libcosmic @ `08a09e3` (DropTerm's pinned rev, local cargo checkout):
  `src/theme/mod.rs` L50-135, L153-175; `src/app/mod.rs` L470-485;
  `src/app/cosmic.rs` L960-980, L1085-1110; `src/command.rs` L36 —
  <https://github.com/pop-os/libcosmic/tree/08a09e3dfa1f729d52585fcedd6e731130235363>
- cosmic-edit @ master: `src/config.rs` L14-33; `src/main.rs` L725, L1291-1329
  — <https://github.com/pop-os/cosmic-edit>
- DropTerm local: `src/config.rs`, `src/settings.rs`, `src/app.rs`
  (L323-363, L1866-1883), `src/terminal/theme_source.rs`,
  `src/terminal/engine.rs`, `Cargo.toml`, `LICENSE`, `ROADMAP.md`
