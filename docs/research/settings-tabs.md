# Research: settings drawer as tabs (General / Keybindings / Advanced) — libcosmic primitives, first-party patterns, and a plan for DropTerm

Date: 2026-10-05
Question: *ROADMAP.md:31 asks for "Settings sidebar as tabs - General, Keybindings, Advance". Which libcosmic widgets at the pinned rev actually implement tabbed navigation, how do the first-party COSMIC apps do it, and what is the concrete design for DropTerm's in-drawer settings pages — including the new Keybindings editor and the existing Profiles sub-page?*

## Recommendation

**Put a horizontal `widget::tab_bar` (`segmented_button::Model<SingleSelect>`, `SegmentedButton::TabBar`
style) in the context drawer's `header` slot — the pinned slot above the drawer's scrollable — and drive
it from a small `SettingsTab` enum stored next to today's `settings_page`. Regroup by *view* only:
General = the existing Appearance + Geometry + Behavior + Terminal cards plus the Profiles entry row,
Advanced = the existing Advanced card, Keybindings = a brand-new editor page transplanted from
cosmic-term's KeyboardShortcuts drawer. Profiles stays a drill-down page layered over the tabs, exactly
like today.**

Why this shape:

- **It is the only one that fits the drawer.** The drawer is 344–480px wide
  (`drawer_max_width`, src/settings.rs:1407-1419, mirroring libcosmic `Core::context_width` at
  `src/core.rs:268-283` @ `08a09e3`). A vertical sidebar — the `widget::nav_bar` look cosmic-settings and
  cosmic-files use — is a ~200-280px column (cosmic-files clamps its own to `max_width(280)`,
  cosmic-files `src/app.rs:2535-2536`), which in this drawer would leave the controls 64-280px. A
  horizontal 3-tab strip costs one 32-44px row. The roadmap's word "sidebar" describes where the tabs
  live (the settings sidebar/drawer), not a second navigation column; three tabs across the top is the
  natural reading at this width.
- **It is a view-only change for the regrouping.** `Section::ALL`/`Section::keys()`
  (src/settings.rs:54-102) — the control→key map pinned by tests — is untouched. A tab becomes just
  `fn tab_sections(tab) -> &'static [Section]` plus the Profiles entry, so the write contract, the
  planners, and the section-map test survive verbatim. (The `ConfigKey` enum counts 23 keys;
  the code comment at src/settings.rs:1430 still says "the all-21 construction test" — stale wording,
  not a different map.)
- **The header slot exists for exactly this.** libcosmic's drawer pane is
  `column![header(title-row, title, header_slot), scrollable(content), footer?]`
  (`src/widget/context_drawer/widget.rs:95-119` @ `08a09e3`): whatever is passed as the third argument of
  `widget::context_drawer(...)` stays pinned while the page content scrolls beneath it. DropTerm already
  hand-composes that call (src/app.rs:4012-4021) and currently passes `None` there.
- **Keybindings is necessarily new build-out, and cosmic-term is a same-rev template.** DropTerm's
  keybinds are documented as non-customizable (src/keybinds.rs:5-7) and `DropTermConfig` stores no
  binding fields; cosmic-term's `src/shortcuts.rs` (588 lines: bindings model, defaults/custom overlay,
  display strings, groups) plus its `keyboard_shortcuts()` view (cosmic-term `src/main.rs:1010-1128`)
  are the editor to port, and cosmic-term pins the *same libcosmic rev* (`08a09e3` in its `Cargo.lock`).

Alternatives considered and rejected:

- **Vertical `widget::nav_bar` inside the drawer** — the cosmic-settings/cosmic-files pattern. Rejected
  on width (above); also its `From<NavBar>` impl hard-codes `height(Length::Fill)` and a primary-layer
  background (`src/widget/nav_bar.rs:169-193` @ `08a09e3`), which fights a drawer pane that already has
  its own layer background.
- **`widget::segmented_control::horizontal`** (Control style, 32px, dividers —
  `src/widget/segmented_control.rs:17-37` @ `08a09e3`). Fits and looks fine, but libcosmic's own style
  docs draw the distinction: `TabBar` is *"A tabbed widget for switching between views in an interface"*,
  `Control` is *"A widget for multiple choice selection"*
  (`src/theme/style/segmented_button.rs:16-22` @ `08a09e3`). Settings pages are views, not choices; the
  TabBar preset also gives the selected tab the semibold active font for free
  (`font_active: crate::font::semibold()`, `src/widget/segmented_button/widget.rs:239` @ `08a09e3`).
- **Separate context pages like cosmic-term's `ContextPage::{Settings, KeyboardShortcuts}`** — no tabs,
  each page a drawer of its own. Rejected because the roadmap explicitly asks for tabs in the settings
  UI; also cosmic-term's page switches rebuild nothing shared, while tabs keep one composed drawer.
- **Reuse DropTerm's own `src/tab_bar.rs`** — it is a fully hand-drawn `Widget` impl built for
  drag-reorder and inline rename (src/tab_bar.rs:881, 1282); three static settings tabs need none of
  that machinery.

## Findings

All libcosmic citations are from the local cargo checkout of [pop-os/libcosmic @
`08a09e3dfa1f729d52585fcedd6e731130235363`](https://github.com/pop-os/libcosmic/tree/08a09e3dfa1f729d52585fcedd6e731130235363)
at `~/.cargo/git/checkouts/libcosmic-41009aea1d72760b/08a09e3/` — the rev DropTerm's `Cargo.toml` pins.
Upstream app citations are from fresh clones (fetched 2026-10-05): cosmic-edit @
`13987165088d991d3c024b5bbb8cd7eb89b4c2ad`, cosmic-files @ `f7679791f4fbf82ba7c9a01cf3f8ae0e21265975`,
cosmic-term @ `3ef31d750d3b09a73f266923b0d64c5df038cf9f` — all three pin libcosmic `08a09e3`
byte-identical to DropTerm — and cosmic-settings @ `82492410584c7c010892d877ed7d19c10578aadb`
(master; pins the *newer* `d77e99f` — cited for pattern only, see finding 2's caveat).

### 1. What "sidebar as tabs" maps to in libcosmic @ `08a09e3` (Q1)

There is no `widget::sidebar` and no free-standing `widget::tabs`. Every tabbed/sidebar UI in the
ecosystem composes from one engine: `widget::segmented_button` (`src/widget/segmented_button/mod.rs`),
whose `Model<SelectionMode>` holds rows (text, icon, arbitrary `data<Data>` payloads, dividers,
activation) and whose single `SegmentedButton` widget renders horizontally or vertically via type-state
variants. Everything else is a preset over it:

| Preset | Constructor | Look (theme variant) | Sizing defaults | Fits a 344-480px drawer? |
|---|---|---|---|---|
| App sidebar | `widget::nav_bar(model, on_activate)` — `src/widget/nav_bar.rs:25-32`; types `nav_bar::Id`/`nav_bar::Model` = `segmented_button::{Entity, SingleSelectModel}` — `:17-19` | `SegmentedButton::NavBar`, primary-layer bg, own scrollable — `src/widget/nav_bar.rs:169-193` | 32px buttons, `height(Length::Fill)`, width shrink-to-content (`From<NavBar> for Container`, `:169-193`) | **No** — a real sidebar column; needs a window-sized pane next to it |
| Document tabs | `widget::tab_bar::horizontal(model)` — `src/widget/tab_bar.rs:17-34`; `vertical(model)` at `:36-54` | `SegmentedButton::TabBar` | min button width 76, max 250, height 44, padding `space_s`/`space_xs` (overridable via derived setters) | **Yes** — 3 × ~76-100px ≈ 230-300px |
| Choice control | `widget::segmented_control::horizontal(model)` — `src/widget/segmented_control.rs:17-37` | `SegmentedButton::Control` | height 32, dividers | Yes, but wrong semantics (see Recommendation) |
| Raw engine | `segmented_button::horizontal/vertical(model)` — `src/widget/segmented_button/{horizontal,vertical}.rs:24/:23` | any via `.style(theme::SegmentedButton::…)` (variants: `TabBar` default, `Control`, `NavBar`, `FileNav`, `Custom` — `src/theme/style/segmented_button.rs:15-26`) | `new()`: width `Length::Fill`, height `Length::Shrink`, font 14, `font_active` semibold — `src/widget/segmented_button/widget.rs:216-264` | Yes — but the presets already encode the house look |

API shape that matters for wiring (all from the checkout):

- `on_activate` on the raw widget takes any `impl Fn(Entity) -> Message + 'static`
  (`src/widget/segmented_button/widget.rs:333-338`); the `nav_bar()` convenience constructor narrows it
  to a plain `fn` pointer (`src/widget/nav_bar.rs:25-32`). `tab_bar::horizontal` keeps the closure form.
- The model is app-owned state, not per-frame: `Model::insert()`, `.text()/.icon()/.data<T>()`
  (builder: `src/widget/segmented_button/model/builder.rs:26-141`), `activate(id)`
  (`model/mod.rs:94-98`), `data::<T>(id)` (`:167-175`), `position(id)` (`:369-380`),
  `entity_at(position)` (`:246-257`). `SingleSelectModel` = `Model<SingleSelect>` with
  `active_data()` accessors (cosmic-edit `src/main.rs:503-507` uses them).
- Keyboard: the widget participates in the iced focus chain and there is a
  `segmented_button::focus(id)` task (`src/widget/segmented_button/widget.rs:52`).

**Drawer vs full window.** `nav_bar` is designed for the window's left edge: libcosmic's `view_main`
inserts the `nav_bar()` trait hook's element there (`src/app/mod.rs:644-660`), and its `From` impl
pins `height(Length::Fill)` with a primary background (`src/widget/nav_bar.rs:169-193`) — a pane, not a
strip. `tab_bar`/`segmented_control` are embeddable rows that size to their buttons; they are the only
one of the three that belongs inside a 344-480px drawer pane. Crucially, the drawer already provides a
home for a pinned strip: the pane is `column![header, scrollable(content), footer]` where `header` =
close-row + optional title + **the caller's `header` element** (`src/widget/context_drawer/widget.rs:95-106`,
pane assembly `:107-119`) — so a tab strip passed as the header never scrolls away. The pane's scrollable
is the framework's (`:115-118`); apps do not add their own around page content (confirmed in use:
cosmic-term's settings/shortcuts pages are bare `settings::view_column`s).

### 2. How the first-party apps do it (Q2)

- **pop-os/cosmic-settings** @ `8249241` (sidebar of categories): a *full-window* vertical
  `nav_bar::Model` — field `nav_model: nav_bar::Model` (cosmic-settings `cosmic-settings/src/app.rs:62`),
  served through the `ApplicationExt::nav_model()` hook (`:252-253`), so the framework renders it.
  Rows are registered with `.text(title).icon(icon).data(page_entity)` (`navbar_insert`, `:1044-1054`).
  Selected state lives twice: the model's `SingleSelect` activation, moved by
  `activate_navbar` → `nav_model.activate(nav_id)` (`:1011-1018`), and the app's own
  `active_page: page::Entity` (`:57`) which is also *persisted to config* and restored on launch
  (`activate_page`, `:959-991`). Input path: framework `Action::NavBar(entity)` → the app's
  `on_nav_select` override maps entity→page via `nav_model.data::<page::Entity>()` (`:292-294`; the
  framework side is libcosmic `src/app/cosmic.rs:893-896` @ `08a09e3`). *Caveat: master pins libcosmic
  `d77e99f` (newer than DropTerm's); the same nav_model/on_nav_select shape existed at `08a09e3`-era
  revisions, but treat exact APIs as pattern, not verbatim contract.*
- **pop-os/cosmic-edit** @ `1398716` (app "sidebar" = document tab strip; settings = context drawer):
  its horizontal tabs are `widget::tab_bar::horizontal(&self.tab_model)` with
  `.button_height(32).button_spacing(space_xxs)` (cosmic-edit `src/main.rs:3055-3060`) — the TabBar
  preset at DropTerm's exact rev, shrunk to 32px. State: `tab_model:
  segmented_button::SingleSelectModel` field (`:459`); activation message `TabActivate(Entity)`
  (`:410`) resolved in update against the stored model's data (`:2729`). Its settings UI is the plain
  context-drawer pattern (`ContextPage` enum `:433`, `context_page` field `:472`,
  `ToggleContextPage` `:423`/`289`, `fn settings()` `:1289`, `context_drawer()` `:2991`) — no tabs
  inside the drawer.
- **pop-os/cosmic-files** @ `f767979` (app sidebar = locations/places): overrides the
  `ApplicationExt::nav_bar()` hook itself (not just `nav_model()`), composing
  `cosmic::widget::nav_bar(nav_model, …)` manually with DnD/context-menu/close handlers, then
  `.into_container()` — the `From<NavBar>` pane — capped `.max_width(280)` on non-condensed widths (`:2534`) and
  `.width(Length::Shrink).height(Length::Fill)` (cosmic-files `src/app.rs:2500-2537`). Selection flows
  through the framework's `Action::NavBar` like cosmic-settings. This is the proof that `nav_bar` is
  manually embeddable at `08a09e3` — and simultaneously the proof of its cost: it is a 280px pane in a
  ≥1000px window.
- **pop-os/cosmic-term** @ `3ef31d7` (no sidebar anywhere): settings-adjacent pages are *sibling
  context drawers*, never tabs — `ContextPage { About, ColorSchemes(…), KeyboardShortcuts, Profiles,
  Settings, … }` (`src/main.rs:466-475`), one `context_page` field (`:509`), one
  `ToggleContextPage(ContextPage)` message (`:454`) whose handler swaps the page and re-shows the
  drawer (`:3132-3192`). What cosmic-term *does* contribute is the Keybindings editor (finding 4).

### 3. What the tabs ride in DropTerm (verified locally)

The drawer hosting, page-turn, and input seams the roadmap item needs already exist:

- **Hosting**: the drawer is hand-composed per layer-surface in `view_window` —
  `widget::context_drawer(Some(title), None, None /*header*/, None, on_close, content, drawer_content,
  drawer_max_width(surface_width))` (src/app.rs:4012-4021; the framework `context_drawer()` hook cannot
  reach a layer-shell window, comment at `:3967-3978`). The `header` argument is the unused third
  parameter — the tab strip's slot.
- **Page turn precedent**: `settings_page: SettingsPage` (src/app.rs:454; enum `SettingsPage::{Settings,
  Profiles}` at `:2270-2276`). `SettingsEvent::OpenProfiles`/`BackFromProfiles`
  (src/settings.rs:1314-1319 — *"a page turn, never a drawer close"*) just re-assign the field
  (src/app.rs:4726-4731); the view match swaps title + content (src/app.rs:3998-4011). Tabs are the same
  move with more destinations.
- **Close/reset semantics**: explicit close routes through `close_settings_drawer`
  (src/app.rs:1620-1626) which calls `leave_profiles_page` (`:1632-1639`) — page state drops, drawer
  reopens on the base page; focus-loss *hides* preserve everything (docs/settings.md, "While the drawer
  is open"). Escape is drawer-first via `drawer_escape_routing` (src/app.rs:2445-2451) on the raw
  keyboard subscription, so Escape closes the drawer regardless of which widget holds iced focus — the
  cosmic-term "first Escape unfocuses" quirk (cosmic-term `src/main.rs:1913-1914`) does not apply.
- **Terminal inertness while open**: `TerminalBox` is built `.disabled(self.settings_open)`
  (src/app.rs:3828) — a keybinding editor's capture row must also win over terminal matching, which it
  does because app-level matching happens in `Message::KeyPressed` *after* the drawer routing
  (src/app.rs:4381, 4397, 4402).
- **Keyboard seam for capture**: DropTerm already has the subscription cosmic-term uses for capture —
  `listen_raw` → `Message::KeyPressed(Modifiers, Key, Physical)` (src/app.rs:3673-3703) →
  `keybinds::matched_action(&self.key_binds, …)` (`:4402`). The `key_binds: HashMap<KeyBind, Action>`
  field (src/app.rs:434) is built once from `default_key_binds()` at init (`:3615`) and read per-frame
  by `TerminalBox::new(&tab.terminal, &self.key_binds)` (`:3818`) — a rebuilt map propagates on the
  next frame with no extra plumbing.

### 4. The Keybindings tab: brand-new, ported from cosmic-term (Q3, part 2)

Nothing existing covers it: `keybinds.rs`'s own module doc says *"Unlike cosmic-term, these are not
user-customizable: there is no shortcuts settings page yet"* (src/keybinds.rs:5-7); `DropTermConfig`
has no binding fields (the `ConfigKey` list at src/settings.rs:108-133 is the whole UI surface, 23 keys,
none a binding); there is no capture UI, display-string helper, or groups/labels anywhere in the repo.
So the tab is new build-out in three layers, each with a same-rev template:

- **Model (port of cosmic-term `src/shortcuts.rs`, 588 lines):** `Binding`, `KeyBindAction` (DropTerm
  already has the action half — `crate::terminal::Action`, 23 variants, src/terminal/widget.rs:77), a
  custom map `Shortcuts(BTreeMap<Binding, Action>)` (cosmic-term `:156`), `BindingSource::{Default,
  Custom}` (`:159-163`), `ShortcutsConfig { defaults, custom }` with `bindings_for_action` overlay
  resolution (`:171-233`), `binding_display` (`:383`), `binding_from_key(modifiers, key)` (`:390`),
  and group/label metadata (`shortcut_groups()` `:311`, `action_label()` `:263`). In DropTerm this is a
  natural extension of src/keybinds.rs (which already owns the default map and `matched_action`,
  src/keybinds.rs:34, 141) plus one new `DropTermConfig` field (`custom_key_binds`) persisted through
  the existing `CosmicConfigEntry` write path — the settings module's `PlannedWrite`/`persist` contract
  (src/settings.rs:174-251) then covers it with a `ProfileWrite`-style dedicated write, since it is a
  whole-map write like `profiles`, not one of the 23 scalar keys.
- **View (port of `keyboard_shortcuts()`, cosmic-term `src/main.rs:1010-1128`):** a
  `settings::view_column` of `settings::section::with_column(list_column)` per group — the search input
  on top, then per action a heading row with reset (when changed) and add icon buttons, per binding a
  `settings::item::builder(binding_display)` row with a delete icon, and while capturing an extra hint
  + cancel row (`:1101-1115`). All the widgets used exist at `08a09e3`:
  `settings::section::with_column` (`src/widget/settings/section.rs:20-26`), `list::list_column`
  (`src/widget/list/list_column.rs:110`), `settings::view_column`
  (`src/widget/settings/mod.rs:15`), `widget::text_input::search_input`, icon `button::custom` with
  `style::Button::Icon` — cosmic-term pins the same rev, so this compiles as-is.
- **Capture flow (port of the update arm, cosmic-term `src/main.rs:2438-2474`):** `ShortcutCaptureStart`
  sets `shortcut_capture: Some(action)` (`:2533-2535`); the shared `Message::Key` handler checks the
  capture state *before* normal matching, parses via `binding_from_key`, and either applies or opens a
  conflict (`:2455-2473`). DropTerm's equivalent hook is the `Message::KeyPressed` arm
  (src/app.rs:4381-4402): check `keybind_capture` first (while `settings_open`, before
  `matched_action`), then commit the custom map, persist, and rebuild `self.key_binds`. Conflict
  policy: cosmic-term raises a `ShortcutConflict` dialog (`:2462-2466`); DropTerm should prefer the
  drawer's inline-error convention (the profile form's red `danger_text_style` row, src/settings.rs:1594-1619)
  — a second window can never stack above the layer surface (the lesson that killed the old settings
  window, docs/research/settings-sidebar.md "Fit for DropTerm").

Scope trim worth making explicit: DropTerm needs one binding per action (its `Action`s are single-key
semantics today), so the cosmic-term multi-binding list plus `Disable` marker can be simplified to
replace-on-capture with per-row delete restoring the default — or kept verbatim if multi-bind is
wanted later. Decide in the module spec, not here.

### 5. The design, concretely (Q3, part 1)

**Widget.** `widget::tab_bar::horizontal(&model)` (TabBar preset) at 32px height like cosmic-edit
(src/main.rs:3056 override), placed in the drawer's `header` slot:

```rust
// settings.rs — the tab strip, built from the stored model
pub fn tab_header(model: &TabModel, on_pick: fn(segmented_button::Entity) -> SettingsEvent)
    -> Element<'static, SettingsEvent>
{
    widget::tab_bar::horizontal(model)
        .button_height(32)
        .on_activate(on_pick)  // emits PickTab(Entity)
        .into()
}
```

passed as the third argument at src/app.rs:4014 (today `None`), so it renders pinned under the title
and above the scrollable (libcosmic `src/widget/context_drawer/widget.rs:95-119` @ `08a09e3`).

**State.** One enum plus one stored model field, mirroring cosmic-edit's `tab_model`
(cosmic-edit `src/main.rs:459`):

- `enum SettingsTab { General, Keybindings, Advanced }` — plain enum, the source of truth (like
  cosmic-settings' `active_page` field, cosmic-settings `src/app.rs:57`, and DropTerm's own
  `settings_page`).
- `settings_tab_model: segmented_button::SingleSelectModel` — built once in init, three rows with
  `.data(SettingsTab::…)`, activated in lockstep on every pick (cosmic-settings does the same dual
  write in `activate_navbar`, `:1011-1018`). The model must be a stored field, not rebuilt per frame:
  `on_activate` delivers an `Entity`, and the handler resolves it via
  `self.settings_tab_model.data::<SettingsTab>(entity)` (cosmic-edit's `TabActivate` pattern,
  `src/main.rs:2729`).
- Page shape: `settings_page` becomes `enum SettingsPage { Tab(SettingsTab), Profiles }` — one field,
  exhaustive match at src/app.rs:3998, and Profiles remains a page layered over whatever tab opened it.
  Tab picks are page turns under the OpenProfiles precedent: never a drawer close, never a
  `DrawerTransition` (src/settings.rs:1314-1319 carries the rule verbatim).

**Regrouping.** Tab membership is a pure function, the section registry untouched:

- **General** → `Section::Appearance`, `Section::Geometry`, `Section::Behavior`, `Section::Terminal`
  (17 keys) + the Profiles entry card (src/settings.rs:1451-1460). Everyday controls; the Profiles
  entry stays on General because it is the drill-down's only door.
- **Keybindings** → the new editor (finding 4).
- **Advanced** → `Section::Advanced` (`RespectPanel`, `Layer`, `KeyboardMode` — src/settings.rs:96-100),
  i.e. exactly the current Advanced card moves to its own tab, matching the roadmap's three names
  ("Advance" = the existing "Advanced" title, src/settings.rs:1365).

Rejected regrouping: hiding `Terminal` under Advanced (Shell/Args is everyday configuration) — and any
regrouping that edits `Section::ALL`/`keys()`, which would churn the pinned control→key map for zero
user value. `drawer_view` becomes `general_view` (loop over the tab's sections + entry card); the
`drawer_section`/`drawer_item`/`drawer_control` chain (src/settings.rs:1664-1707) is reused verbatim.

**Profiles interaction.** Unchanged mechanics, one new rule: `OpenProfiles` flips
`settings_page = Tab(<current>) → Profiles` (remember nothing — Back always returns to *General*,
where the entry row lives; if the entry row ever appears on other tabs, store the origin tab instead).
`BackFromProfiles` sets `Tab(General)`; `leave_profiles_page` (src/app.rs:1632-1639) resets to
`Tab(General)` on explicit close; focus-loss hides still preserve tab + selection + draft
(docs/settings.md). The drawer title follows the *page*, as today (src/app.rs:3998-4011): "Settings"
while on any tab (the tabs disambiguate), "Profiles" on the drill-down.

**Docs.** docs/settings.md's "Sections and commit points" and "Profiles page" sections need a rewrite
for the tab structure; the "When each edit takes effect" table gains the keybinding row
("instant — the very next key press", mirroring cosmic-term's `save_shortcuts_custom` → immediate
`key_binds` rebuild, cosmic-term `src/main.rs:614-630`).

### 6. File layout (Q4): stay one `src/settings.rs`

Keep settings.rs a single file; do not create `src/settings/`. The case fails the repo's own bar
(AGENTS.md "File creation": split only when sections are unrelated, a one-word concept names the unit,
and contents change independently). What the file actually is: one cohesive module — *the settings
drawer's write contract plus every page it hosts* — whose halves change together:

- The write contract (`WriteKind`, `ConfigKey`, `PlannedWrite`, clamps, planners —
  src/settings.rs:31-580) is consumed by the view builders (`drawer_view` et al., `:1421-1845`) and
  pinned by ~1,600 lines of inline tests (`#[cfg(test)]` at `:1834` to EOF) that cross the pure/view
  boundary freely (`// --- the Profiles page's view (construction …) ---` at `:2966` sits *inside* the
  test module's domain comments).
- The Profiles page already set the precedent: its pure half (drafts, commits, reorder planners,
  `:668-1289`), its events (`:1320-1339`), and its view (`:1478-1845`) all live here — because they
  are *this drawer's page* logic, the same reason the model-side `Profile` stays in src/profile.rs.
  The Keybindings tab follows the same seam: model/matching extend src/keybinds.rs (which owns
  `default_key_binds` and `matched_action`); the config field lands in src/config.rs; the editor's
  pure half (custom-map planners, capture resolution, conflict verdict — all pure functions in the
  module's headless-testable style, module doc src/settings.rs:3-12), events, and view land in
  src/settings.rs. The tab regrouping itself is ~a `SettingsTab` enum + `tab_header` + a `tab_sections`
  function — dozens of lines, not a module.
- Size is the lagging indicator the AGENTS.md warns about: settings.rs grows to roughly 4,100-4,400
  lines, which is large but no boundary has appeared — every added piece changes *the settings drawer*
  and only it. Revisit only if a future page (e.g. per-profile keybinds) starts needing unrelated
  sections of the file at once.

The one extraction that *would* be justified later: the keybinding *model* if it grows
grouping/labeling/search metadata beyond the editor's needs — but that lands in keybinds.rs, not a new
file.

### 7. Risks / gotchas (Q5)

- **Drawer width floor vs the strip.** Three TabBar buttons at the preset's minimum width 76 = 228px
  before spacing; the drawer's inner width at the 344px floor is ~328px after `space_s` padding
  (libcosmic `src/widget/context_drawer/widget.rs:74` @ `08a09e3` — `space_s` when `max_width < 392`).
  Fits, but with no headroom for longer translations: "Keybindings" is already the widest label
  (~11 chars at font 14). The strip ellipsizes nothing by default (paragraphs use `Wrapping::None`,
  `src/widget/segmented_button/widget.rs:246`), so overflow would clip — cap with
  `.maximum_button_width(...)` or keep labels short; hardware-check at the 344px floor (AGENTS.md
  checklist, as with OQ-3 in settings-sidebar.md).
- **Scroll position across tab switches.** The scrollable belongs to the drawer pane, not the page
  (`src/widget/context_drawer/widget.rs:115-118` @ `08a09e3`), and it has no id — iced keys its state
  by tree position, so switching tabs under it may carry the old scroll offset to the new page
  (cosmic-edit's editors live with the same behavior). If it reads badly, give each page's top element
  an explicit scroll target or accept the offset; verify on hardware.
- **Focus/keyboard nav.** The strip joins the iced focus chain (arrows move within a focused
  `SegmentedButton`); Escape is already drawer-close-first regardless of focus
  (`drawer_escape_routing`, src/app.rs:2445-2451). The Keybindings capture row must swallow *all*
  presses while armed — including bare Escape? Decide: cosmic-term's capture consumes every parsed
  binding and leaves Escape to `on_escape` (cosmic-term `src/main.rs:2447-2450`); DropTerm's
  `KeyPressed` arm should cancel capture on Escape instead, since its raw subscription sees the press
  before any widget (src/app.rs:3673-3703). Pin the ruling in the module spec.
- **Instant-apply semantics carry over unchanged.** Every control on every tab keeps commit-on-change/
  release/Enter (docs/settings.md, "Sections and commit points"); tab switching itself writes nothing
  and clears nothing — the OpenProfiles rule (src/settings.rs:1314-1319). The Keybindings tab inherits
  the same rule per cosmic-term (`save_shortcuts_custom` writes config then rebuilds the live map,
  cosmic-term `src/main.rs:614-630`); the one new wrinkle is rebuilding `self.key_binds` after each
  write so `TerminalBox` stops swallowing the old chord on the *next* frame (src/app.rs:3615, 3818).
- **i18n.** DropTerm has no fluent setup — every label is a hardcoded `&'static str`
  (`Section::title`, `ConfigKey::label`, src/settings.rs:1359-1400) — so tab titles follow the same
  convention with no constraint beyond label length (width risk above). The drawer's own Close label
  is libcosmic-internal and already localized (settings-sidebar.md, "i18n of framework strings").
  cosmic-term's shortcuts page is fully `fl!`-based (e.g. `fl!("type-to-search")`,
  cosmic-term `src/main.rs:1029`); when porting, hardcode English first to match this repo, and leave
  the fluent strings as a marker for the eventual i18n pass.
- **Config-version bump.** A new `custom_key_binds` field on `DropTermConfig` is additive
  (`CosmicConfigEntry` defaults it), but the field name becomes file-format contract — spell it once,
  like every `ConfigKey::key_name` (src/settings.rs:135-166), and add the round-trip test beside the
  existing config tests.
- **Related roadmap coupling.** ROADMAP.md:30 ("increase sidebar width size, draggable") widens the
  same pane; the tab strip benefits but must not assume the wider floor. And ROADMAP.md:32
  ("Per-tab profiles") will re-touch the Profiles page — one more reason to keep Profiles a page over
  the tabs rather than folding it in as a fourth tab.

## Sources

- Local checkout, pop-os/libcosmic @ `08a09e3dfa1f729d52585fcedd6e731130235363`:
  `~/.cargo/git/checkouts/libcosmic-41009aea1d72760b/08a09e3/` — `src/widget/{nav_bar,tab_bar,
  segmented_control,segmented_button/*,settings/*,context_drawer/widget}.rs`,
  `src/theme/style/segmented_button.rs`, `src/app/{mod,cosmic}.rs`, `src/core.rs`.
- pop-os/cosmic-settings @ `82492410584c7c010892d877ed7d19c10578aadb` (master, libcosmic `d77e99f`):
  `cosmic-settings/src/app.rs`.
- pop-os/cosmic-edit @ `13987165088d991d3c024b5bbb8cd7eb89b4c2ad` (libcosmic `08a09e3`): `src/main.rs`.
- pop-os/cosmic-files @ `f7679791f4fbf82ba7c9a01cf3f8ae0e21265975` (libcosmic `08a09e3`): `src/app.rs`.
- pop-os/cosmic-term @ `3ef31d750d3b09a73f266923b0d64c5df038cf9f` (libcosmic `08a09e3`):
  `src/main.rs`, `src/shortcuts.rs`.
- This repo: `ROADMAP.md`, `src/{app,settings,keybinds,config,tab_bar}.rs`,
  `src/terminal/widget.rs`, `docs/settings.md`, `docs/research/settings-sidebar.md`.
