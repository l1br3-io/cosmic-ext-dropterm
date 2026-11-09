# Research: cosmic-term's in-window settings drawer — implementation, libcosmic primitives, and fit for DropTerm

Date: 2026-10-03
Question: *How does pop-os/cosmic-term implement its in-window settings sidebar, which libcosmic primitives back it, and how would DropTerm's current separate settings window map onto the same approach?*

## TL;DR

**cosmic-term has no "sidebar" widget and no second window for settings — its settings UI is a
libcosmic *context drawer*: an iced overlay pinned to the right edge of the main window, built
almost entirely by the framework.** The app's entire contribution is a `ContextPage` enum, a
`context_page` field, one message (`ToggleContextPage`), a `context_drawer()` trait override
returning `Option<app::context_drawer::ContextDrawer>` (title + content + on_close), and a
`core.window.show_context` flip; libcosmic's `view_main` does the composition, clamps the drawer
to 344–480px, wraps it in a scrollable, styles it `Layer::Primary` /
`Container::ContextDrawer`, and gives it a Close button. The settings content is one scrolling
column of `widget::settings` sections — no page list, no nav bar. Every control applies
instantly: a `config_set!` macro writes through cosmic_config-generated `set_<field>` methods,
then `update_config()` re-pushes the config into every live terminal. There is no animation, no
Apply/OK/Cancel, no extra `window::Id`; the terminal underneath is *disabled and unfocused* (not
resized — the drawer overlays it), and Escape closes it (after one unfocus pass). Crucially,
cosmic-term @ `master` pins **the exact libcosmic rev DropTerm pins (`08a09e3`)**, so every API
cited below already exists in DropTerm's dependency tree; `cosmic::widget::sidebar` does *not*
exist at that rev (or anywhere in libcosmic).

## Findings

All cosmic-term citations are from a clone of [pop-os/cosmic-term](https://github.com/pop-os/cosmic-term)
@ `master` (default branch, confirmed via `git remote show origin`), HEAD
`3ef31d750d3b09a73f266923b0d64c5df038cf9f` (2026-10-01). Its `Cargo.lock` pins libcosmic
`git+https://github.com/pop-os/libcosmic.git#08a09e3dfa1f729d52585fcedd6e731130235363` —
byte-identical to DropTerm's pinned rev (DropTerm `Cargo.toml:30-33`), so all libcosmic
citations are from the local cargo checkout of that rev and apply verbatim to DropTerm.

### 1. Overall structure (Q1): `ContextPage` + `ToggleContextPage` + framework drawer

There is no `src/settings.rs` or `src/sidebar.rs` in cosmic-term, and `grep -ri sidebar src/`
returns zero hits. The settings UI lives in `src/main.rs`:

- `pub enum ContextPage { About, ColorSchemes(ColorSchemeKind), KeyboardShortcuts, Profiles,
  Settings, #[cfg(...)] PasswordManager }` — `src/main.rs:466-475`; current page tracked in the
  `context_page: ContextPage` field — `src/main.rs:509`; visibility tracked separately by the
  framework-owned `core.window.show_context: bool`.
- The single message `Message::ToggleContextPage(ContextPage)` — `src/main.rs:454` — is the
  whole open/close/switch protocol. Handler `src/main.rs:3132-3192`: toggling the *same* page
  flips `self.core.window.show_context`, calls `pane_model.update_terminal_focus()` (L3134-3135)
  and `update_focus()` (L3150); switching to a *different* page sets the field, forces
  `show_context = true`, and calls `pane_model.unfocus_all_terminals()` (L3151-3154).
- Triggers, all funnelling into `Action::Settings` → `Message::ToggleContextPage(ContextPage::Settings)`
  (`src/main.rs:321-323`):
  - menu bar, View menu, closing group: `MenuItem::Button(fl!("menu-settings"), None, Action::Settings)`
    — `src/menu.rs:223` (label `menu-settings = Settings...`, `i18n/en/cosmic_term.ftl:141`;
    drawer title `settings = Settings`, `:34`);
  - terminal right-click context menu, **last** item: `items.push(item(fl!("menu-settings"), Action::Settings))`
    — `src/menu.rs:74`, inside `context_menu()` (`src/menu.rs:42-87`);
  - default keybind **Ctrl+,**: `bind!([Ctrl], ",", Settings)` — `src/shortcuts.rs:465`, mapped
    to `Some(Action::Settings)` at `:118`.
- cosmic-term opens **no window** for any of this: `grep -rn "window::open" src/` → 0 hits; the
  only secondary windows are dialogs via `view_window` (`src/main.rs:3344-3349`).

### 2. The widget itself (Q2): framework `widget::ContextDrawer`, an iced overlay — not `widget::sidebar`

- **`cosmic::widget::sidebar` does not exist** in libcosmic @ `08a09e3`: `grep -ril sidebar`
  over the whole crate's `src/` matches no file. cosmic-term uses neither `pane_grid` nor a
  hand-rolled fixed-width row for the drawer (its `pane_grid` is the terminal split layout,
  `src/main.rs:3370`).
- The *app-facing* type is plain data: `app::context_drawer::ContextDrawer { title, actions,
  header, content, footer, on_close }` — libcosmic `src/app/context_drawer.rs:8-15`, built by
  `context_drawer(content, on_close)` (L26-38) with builder methods `.title()` (L42-45),
  `.actions()` (L48-51), `.header()` (L54-57), `.footer()` (L60-63); `about()` preset (L17-24).
- The *rendering* widget is `widget::ContextDrawer { content, drawer, on_close, id }` —
  `src/widget/context_drawer/widget.rs:17-22`. Its `Widget` impl delegates `size`/`layout`/
  `update`/`draw` **to the app content only** (L217-303) and renders the drawer pane through
  iced's overlay mechanism (`fn overlay`, L305-325, returning the `Overlay` from
  `src/widget/context_drawer/overlay.rs:12-150`). So structurally it is an *overlay*, not a
  layout sibling.
- The pane inside (built once in `new_inner_overlay`, `widget.rs:42-150`): header row =
  actions-or-title slot plus a Close button — `button::text(fl!("close"))
  .trailing_icon(icon::from_name("go-next-symbolic")).on_press(on_close)` (L90-94); optional
  `text::title4` title; the app's content in a `scrollable` with `Length::Fill` height
  (L107-119); all wrapped in `LayerContainer::new(pane).layer(cosmic_theme::Layer::Primary)
  .class(crate::style::Container::ContextDrawer { transparent: !overlay })
  .width/height(Length::Fill).max_width(max_width)` inside a Fill container
  `.align_x(Alignment::End)` — i.e. pinned right (L124-137). Horizontal padding is
  theme `space_l`, or `space_s` when `max_width < 392.0` (L74).
- Styling: `Container::ContextDrawer { transparent }` renders as `Container::primary(cosmic,
  transparent)` with a 1px divider border added in high-contrast themes —
  `src/theme/style/iced.rs:392-394` and `:592-600`.
- Width: computed by the framework as `core.context_width(has_nav)` =
  `(window_width/scale - reserved).min(480.0).max(344.0)` — "minimum width is 344px and the
  maximum with is 480px" — `src/core.rs:268-283`.

### 3. Layout integration (Q3): the terminal does NOT shrink — the drawer overlays a disabled terminal

- cosmic-term's own `fn view(&self)` returns just the terminal `PaneGrid` (with per-pane tab
  bars and the find row), `src/main.rs:3352-3548` — it never mentions the drawer. Composition
  happens in libcosmic's `ApplicationExt::view_main` (`src/app/mod.rs:612-741`), which assembles
  `[nav_bar?] + [content and/or drawer] + [footer?]` inside the window template (header bar etc.).
- **Overlay mode** (the default; see finding 7): when `core.window.context_is_overlay &&
  show_context`, `view_main` calls `crate::widget::context_drawer(title, actions, header,
  footer, on_close, main_content, context.content, context_width)` — the widget *wraps* the
  app's content and overlays the drawer on top of it (`src/app/mod.rs:667-690`). The terminal
  keeps its size; the drawer covers its right side. cosmic-term therefore **disables** the
  terminal element while the drawer is open: `terminal_box(...).disabled(self.core.window.show_context)`
  — `src/main.rs:3412`.
- **Side-by-side mode** (`context_is_overlay = false`): `view_main` instead pushes the content
  and then `ContextDrawer::new_inner(...)` as **siblings in a row**, the drawer sized
  `.width(context_width)` (`src/app/mod.rs:698-733`) — the content area genuinely shrinks next
  to it. cosmic-files runs this mode (finding 9); cosmic-term touches it only to keep the
  shortcut-conflict *dialog* uncovered: `set_context_overlay(&mut self, overlay: bool)` flips
  the public field live and re-runs `set_show_context` (`src/main.rs:640-645`), invoked from
  `begin_shortcut_conflict`/`clear_shortcut_conflict` (`:647-660`). The Settings drawer itself
  always runs overlay-mode.
- Overlay geometry and input: the overlay lays the pane out at full available height, pinned to
  the right/bottom with 8px insets (`src/widget/context_drawer/overlay.rs:23-47`), and
  **captures mouse/touch events over its bounds** (`shell.capture_event()`,
  `overlay.rs:68-80`) so clicks never fall through to the terminal beneath. Keyboard events are
  not captured — hence the explicit disable/unfocus of terminals (finding 8).

### 4. Navigation / sections (Q4): one scrolling column of `widget::settings` sections — no page list

- The Settings drawer's content, `fn settings(&self)` — `src/main.rs:1326-1537` — is exactly:
  `widget::settings::view_column(vec![appearance_section, font_section, splits_section,
  advanced_section])` (L1530-1536). Each section is `widget::settings::section().title(fl!(...))
  .add(widget::settings::item::builder(label).control(widget))` — e.g. the Appearance section
  with theme/syntax/zoom-step dropdowns and an opacity slider at L1373-1417, Font at L1419-1446,
  Splits at L1503-1512, Advanced at L1514-1528.
- There is **no two-pane page list and no nav bar** inside the drawer. The only in-drawer
  sub-navigation is a collapsible group: "advanced font settings" expand/collapse driven by a
  plain `show_advanced_font_settings: bool` (`src/main.rs:525`) toggled by go-up/go-down icon
  buttons (`src/main.rs:1436-1446`), with the extra section appended when true
  (`:1499-1501`).
- Coarser pages exist as *separate drawers*, not tabs inside one: `ContextPage::ColorSchemes`,
  `KeyboardShortcuts`, `Profiles`, `About` are their own `ContextPage` variants with their own
  titles (`src/main.rs:3262-3300`). State = `context_page` (`:509`) + framework `show_context`.
- libcosmic's `nav_bar` (`src/widget/nav_bar.rs`) is a different pattern — persistent
  app-level page navigation through `ApplicationExt::nav_model()` (`src/app/mod.rs:381-419`),
  rendered by `nav_bar()` at the window's left edge (`view_main`, `src/app/mod.rs:644-660`).
  cosmic-term implements neither (`nav_model`/`nav_bar`: 0 uses in its `src/`).

### 5. Apply semantics (Q5): instant apply, per-control `config_set!`, then re-config every terminal

- **No Apply/OK/Cancel anywhere.** The settings view contains only dropdowns, sliders,
  togglers, and the expand/collapse icon buttons (`src/main.rs:1326-1537`); the only "dismiss"
  control is the framework Close button (finding 2).
- Every control emits a message whose handler runs the `config_set!` macro — `src/main.rs:1955-1974`:
  it calls `self.config.[<set_ $field>](config_handler, $value)` (methods generated by
  `#[derive(CosmicConfigEntry)]`, `src/config.rs:4` import, `pub struct Config` at `:220`),
  logs a warning on write failure, and falls back to plain in-memory assignment when no config
  handler exists. Example: `Message::AppTheme` → `config_set!(app_theme, app_theme); return
  self.update_config();` — `src/main.rs:1976-1979`; font analogous at `:2285`, `:2298`.
- Echo into running terminals: `fn update_config(&mut self)` — `src/main.rs:671-704` — updates
  color schemes, stores the window background color, then loops **all panes × all tabs** and
  calls `terminal.set_config(&self.config, color_scheme_kind, &self.themes)` per live terminal
  (L688-697), sets `core.window.show_headerbar` from config (L700), and finishes with
  `cosmic::command::set_theme(theme)` (L703). External config edits flow back through
  `cosmic_config::config_subscription` (`src/main.rs:3597-3601`).

### 6. Animation / transition (Q6): none

- No animation code exists in the drawer stack: `grep -rn "animat"` over libcosmic
  `src/widget/context_drawer/`, `src/app/mod.rs`, and `src/app/context_drawer.rs` @ `08a09e3`
  → 0 hits. The drawer appears and disappears instantly when `show_context` flips; iced simply
  re-lays-out. (Same in the sibling apps; nothing animates the drawer anywhere surveyed.)

### 7. libcosmic primitives @ `08a09e3` (Q7): the exact contract DropTerm would code against

From the local checkout of [pop-os/libcosmic @ `08a09e3`](https://github.com/pop-os/libcosmic/tree/08a09e3dfa1f729d52585fcedd6e731130235363)
(the rev both DropTerm and cosmic-term pin):

- **Trait hooks** (`src/app/mod.rs`):
  - `fn context_drawer(&self) -> Option<ContextDrawer<'_, Self::Message>> { None }` — doc:
    *"Displays a context drawer on the side of the application window when `Some`. Use the
    [`ApplicationExt::set_show_context`] function for this to take effect."* (L350-354).
  - `fn on_context_drawer(&mut self) -> Task<Self::Message>` — *"Called when context drawer is
    toggled"* (L431-434). Framework entry: `Action::ContextDrawer(show)` →
    `core_mut().set_show_context(show)` then `on_context_drawer()` (`src/app/cosmic.rs:882-885`).
  - `fn on_escape(&mut self)` (L436-439), reached via `keyboard_nav::Action::Escape`
    (`src/app/cosmic.rs:876`).
- **State/flags** (`src/core.rs`): `pub struct Window { … pub context_is_overlay: bool, … pub
  show_context: bool, … }` (L29-46); defaults `context_is_overlay: true`, `show_context: false`
  (L160-176). `pub fn set_show_context(&mut self, show: bool)` sets the flag, recomputes the
  condensed breakpoint, and closes a condensed nav bar on conflict (L285-294).
  `context_width(has_nav)` clamps to 344–480px (L268-283). Apps may also assign
  `core.window.context_is_overlay` directly (public field) — cosmic-files does (finding 9).
- **App-side builder** (`src/app/context_drawer.rs`):
  `context_drawer(content: impl Into<Element>, on_close: Message) -> ContextDrawer` (L26-38)
  plus `.title(impl Into<Cow<str>>)` (L42-45), `.actions(Element)` (L48-51), `.header(Element)`
  (L54-57), `.footer(Element)` (L60-63), `.map(fn(Message) -> Out)` (L65-77); preset
  `about(&About, on_url_press, on_close)` (L17-24).
- **Widget-level** (`src/widget/context_drawer/`): free fn
  `context_drawer(title, actions, header, footer, on_close, content, drawer, max_width) ->
  ContextDrawer` (mod.rs:16-33); `ContextDrawer::new(...)` overlay variant (widget.rs:153-177),
  `new_inner(...)` side-by-side pane builder (widget.rs:25-40) delegating to
  `new_inner_overlay(..., overlay: bool)` (widget.rs:42-150) which assembles header+Close
  button+scrollable+`LayerContainer`/`Container::ContextDrawer{transparent}`+`max_width`+
  right alignment (L66-137). Theme style `Container::ContextDrawer { transparent }` →
  `Container::primary` + high-contrast border (`src/theme/style/iced.rs:392-394, 592-600`).
  Overlay input capture and geometry as in finding 3 (`overlay.rs:23-47, 68-80`).
- **Nonexistence note**: `cosmic::widget::sidebar` — 0 hits crate-wide; the closest distinct
  primitive is `widget::nav_bar` (left-edge page navigation, `src/app/mod.rs:381-419`), which is
  *not* what cosmic-term uses for settings.

### 8. Window-level concerns (Q8): one window, disabled terminals, Escape closes

- **Same window.** The drawer is composed inside `view_main` for the main window
  (`src/app/mod.rs:614-741`); cosmic-term opens no other window for it (0 `window::open` hits),
  and `view_window` only serves dialogs (`src/main.rs:3344-3349`). One `window::Id` for
  terminals + drawer + menu bar.
- **Focus/keyboard while open.** The framework overlay swallows pointer events over its bounds
  (`overlay.rs:68-80`) but not keyboard events, so cosmic-term pushes input away from the
  terminals itself: the terminal element is built `.disabled(self.core.window.show_context)`
  (`src/main.rs:3412`); opening a different page calls `pane_model.unfocus_all_terminals()`
  (`:3154`); toggling calls `pane_model.update_terminal_focus()` (`:3135`). `fn update_focus`
  (`src/main.rs:764-785`) focuses a drawer input only for the shortcuts-search special case,
  and on close re-focuses the focused terminal via `widget::text_input::focus(terminal_id)`
  (L779-781).
- **Escape.** `fn on_escape` — with cosmic-term's own caveat, quoted: *"//TODO: currently the
  first escape unfocuses, and the second calls this function"* (`src/main.rs:1913-1914`) —
  closes the drawer whenever `core.window.show_context` is set (after canceling an in-progress
  shortcut capture or conflict dialog on that page): `return
  self.update(Message::ToggleContextPage(self.context_page));` (`:1929`). `on_context_drawer`
  (`:1940-1950`) is the post-close hook that restores focus.

### 9. The same pattern in other first-party COSMIC apps

- [pop-os/cosmic-edit](https://github.com/pop-os/cosmic-edit) @ `master` (fetched 2026-10-03):
  identical shape — `Action::ToggleSettingsPage` → `Message::ToggleContextPage(ContextPage::Settings)`
  (`src/main.rs:289`), `pub enum ContextPage` (`:433`), `context_page` field (`:472`), settings
  drawer content in `fn settings()` (`:1289`, six `settings::section` uses), menu trigger
  `MenuItem::Button(fl!("menu-settings"), None, Action::ToggleSettingsPage)` (`src/menu.rs:337`).
- [pop-os/cosmic-files](https://github.com/pop-os/cosmic-files) @ `master` (fetched
  2026-10-03): same trigger/message shape (`src/app.rs:263`; menu item `src/menu.rs:707`;
  drawer match `src/app.rs:5406-5473`, Settings arm `.title(fl!("settings"))` at `:5470-5473`)
  — and it demonstrates the **side-by-side** mode: `core.window.context_is_overlay = false;`
  set directly in `init` (`src/app.rs:2346`), with the drawer-open state even persisted into
  config (`core.window.show_context = flags.config.show_details`, `src/app.rs:2348-2350`).
- The pattern's actual name across the suite is **context drawer** (`ContextPage` +
  `ToggleContextPage` + `ApplicationExt::context_drawer`); "sidebar" is not a term that appears
  in any of these codebases.

## Fit for DropTerm

DropTerm today (verified in this repo): settings is a second window — `open_settings` opens a
700×760 normal window with `exit_on_close_request: false`, one per process (`src/app.rs:1229-1246`),
tracked by `settings_window: Option<window::Id>` (`src/app.rs:398`); because a normal window can
never stack above the layer surface, opening it **hides the drop-down** (*"the settings window
is a normal window and can never stack above the terminal's layer surface, so opening it hides
the drop-down first"* — `src/app.rs:1247-1253`), and a close-first rule
closes it whenever terminal actions re-show the surface (`close_settings_if_open`,
`src/app.rs:1194-1206`). Content is `settings::view(&self.config, &self.settings_buffer)`
(`src/app.rs:2920`; `src/settings.rs:708`), a scrollable column of per-`Section` cards
(`Section` = Appearance, Geometry, Behavior, Terminal, Advanced — `src/settings.rs:41-48`,
`ALL` at `:54-60`, `keys()` at `:62+`), instant-apply with a `SettingsBuffer` for in-flight
drafts (`src/app.rs:402`; slider release vs move at `src/settings.rs:793-794`). Trigger is
Ctrl+Shift+I (`src/keybinds.rs:54-61`) plus the planned context-menu item (settings-trigger.md).

**What transfers directly (most of it):**

- **The whole content layer.** `settings::view`'s scrollable column of section cards is the
  same shape as cosmic-term's `settings()` column of `widget::settings` sections
  (`src/main.rs:1530-1536`). The `Section` grouping, `keys()` control-map, and the pure
  write-planning functions behind it are host-agnostic: the drawer is just a different host
  element. Either keep DropTerm's custom cards or adopt `widget::settings::{section, item,
  view_column}` for suite consistency — both fit the drawer unchanged.
- **Instant-apply semantics.** cosmic-term's model (per-control message → `config_set!` →
  `update_config()` re-configs every terminal) is what DropTerm already does through its
  single write seam (`src/settings.rs` module docs, U3); DropTerm's config-write functions
  play the role of `config_set!`, and DropTerm gains cosmic-term's "echo to every running
  terminal" step naturally because terminals now stay alive and visible while settings change.
- **The `SettingsBuffer`** stays as-is — it covers text-entry/slider-debounce cases cosmic-term
  simply doesn't have, and nothing about the drawer conflicts with it.
- **Framework fit is already in place.** DropTerm implements `cosmic::app::Application`
  (`src/app.rs:2630`) with `view()` (`:2910`), so libcosmic's `view_main` composition — drawer
  included — applies without new plumbing: implement `context_drawer()` + a `ToggleContextPage`-
  style message, set `core.window.show_context`, done. No new dependency, no rev bump (same
  `08a09e3` as cosmic-term).

**What the sidebar approach deletes or changes:**

- **Deleted**: `settings_window`, `window::open`, `exit_on_close_request`, the
  `SettingsCloseRequested` handshake (`src/app.rs:255, 3744`), the hide-the-terminal dance
  (`src/app.rs:1247-1253`), and the entire close-first rule (`src/app.rs:1194-1206`) — the quake
  surface never hides for settings, and "re-shown terminal must never slide over the window"
  becomes moot.
- **Terminal must yield input, not visibility.** cosmic-term's answer is overlay-mode + disable:
  drawer covers the terminal's right side and `terminal_box.disabled(show_context)` plus
  unfocus keep keyboard/pointer away (`src/main.rs:3412, 3154`). DropTerm's adapted terminal
  widget needs the equivalent guard (ignore input while the drawer is open) — the overlay
  already captures pointer events over its bounds (`overlay.rs:68-80`), but keyboard focus
  must be handled app-side exactly as cosmic-term does.
- **Do not shrink the terminal (keep overlay mode).** In side-by-side mode the content area
  resizes next to the drawer (`src/app/mod.rs:698-733`), which for a terminal means a grid
  reflow + pty resize on every open/close — churn DropTerm already fights in its drag
  debounce (`src/app.rs` `offer_window_height` comments). Overlay mode (the default) avoids
  that entirely: the terminal keeps its size, matching cosmic-term's own choice for settings.
- **Escape gets a drawer-first branch.** DropTerm's quake Escape (hide surface) must check
  `core.window.show_context` first and close the drawer instead — a small inversion of today's
  close-first rule. Expect cosmic-term's documented quirk that the first Escape unfocuses the
  focused drawer control and only the second reaches `on_escape` (`src/main.rs:1913-1914`).
- **Trigger routing**: the decided right-click "Settings…" item (settings-trigger.md) and the
  Ctrl+Shift+I keybind both route to the new toggle message instead of `open_settings`; no
  gear button, consistent with the suite.

**Open risks:**

- **Width on narrow outputs.** The drawer's floor is 344px + 8px padding
  (`src/core.rs:268-283, 246-256`); on a half-width output a quake terminal could lose most of
  its visible columns to the overlay, and libcosmic's condensed-breakpoint logic assumes
  app windows, not layer surfaces. Needs a hardware-checklist pass (per `AGENTS.md`).
- **Window template assumptions.** `view_main` composes content inside the window template
  (header bar / border padding / corner radius scaffolding, `src/app/mod.rs:627-789`); DropTerm
  runs a borderless layer surface, so which of those pieces apply (and whether
  `content_container`/`border_padding` need overriding) must be verified on hardware.
- **Focus handoff latency.** Two focus systems meet (terminal IME/focus vs iced widget focus);
  cosmic-term's `update_focus` special-casing (`src/main.rs:764-785`) hints this seam needs
  care, especially with DropTerm's exclusive-keyboard layer focus.
- **i18n of framework strings.** The drawer's Close label comes from libcosmic's own `fl!`
  (`widget.rs:91`); confirm DropTerm's localize setup leaves it translated (it should — it is
  libcosmic-internal — but worth one hardware glance).
