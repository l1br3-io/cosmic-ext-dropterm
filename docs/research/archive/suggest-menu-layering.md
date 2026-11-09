# Research: how should a floating panel drawn inside the terminal widget win z-order over its own grid — and does "scope first, history after" match how shells rank their suggestion lists?

Date: 2026-11-06
Question: *The suggestion menu painted beneath the terminal grid's glyphs on
wgpu because iced_wgpu renders a layer by primitive kind, not by call order.
Is `renderer.with_layer` the proven fix, or should the menu have become an
`Overlay`-trait widget — and is the menu's amended ranking (seat-scoped
completions above history) how fish, zsh, and fzf order theirs?*

## TL;DR

**`with_layer` is not merely acceptable — it is the exact mechanism iced's
own overlay pipeline uses, and the fix matches libcosmic's own menu recipe
line for line.** Every `Overlay`-trait widget (pick_list, combo_box,
libcosmic's popover/menu/dropdown/context_drawer) floats because
`overlay::Nested::draw` wraps each overlay's draw in
`renderer.with_layer(layout.bounds(), …)`
(`iced/core/src/overlay/nested.rs:101-113`) — the Overlay trait is event
routing and layout plumbing layered *on top of* the same child-layer
mechanism DropTerm called directly. iced's first-class `float` widget ("a
widget that can make its contents float over other widgets",
`iced/widget/src/float.rs:15`) draws a shadow quad plus content inside
`renderer.with_layer(self.viewport, …)` (`float.rs:339-369`) — hecrj's own
0.14 recipe, structurally identical to DropTerm's panel quad + rows inside
`with_layer(view, …)`. The libcosmic menu draws its panel quad, highlight
quad, and item rows inside `renderer.with_layer(render_bounds, draw_menu)`
(`src/widget/menu/menu_inner.rs:814-912`) *while itself being an
`Overlay`* — belt and suspenders. The root cause was verified line-by-line
in the pinned sources: one wgpu layer renders all quads, then triangles,
then shader primitives, then images, then all text
(`iced/wgpu/src/lib.rs:476-661`), so a quad can never sit above text in
the same layer regardless of call order or fill opacity — only a child
layer renders wholly after its parent. The one correction this survey
forces: the fix's comment claims tiny_skia "rasterizes in call order
within a layer" — false; iced_tiny_skia batches by kind per layer exactly
like wgpu (`iced/tiny_skia/src/lib.rs:107-195`), so the explicit layer is
load-bearing on both backends, not harmless belt-and-braces (moot in
practice: the applet never renders the terminal). On ranking (light
coverage): explicit-completion surfaces in every surveyed shell list
*scope* completions and never merge history into them, while the
single-choice ghosts are history-first — DropTerm's ghost keeps
history-first (zsh-autosuggestions' exact default) and its menu putting
the seat's scope above history is the completion-surface norm; a combined
scope+history list has no exact peer anywhere, the nearest analog being
fzf's recency-aware `--scheme=history` and zsh-autosuggestions' ordered
strategy fallback.

## What DropTerm has today

- **The fix, uncommitted in the working tree.** The suggestion menu's
  whole draw — panel quad with shadow and border, selected-row highlight
  quad, row glyphs via `fill_raw` — is wrapped in
  `renderer.with_layer(view, |renderer| { … })` at
  `src/terminal/widget.rs:1147-1214`, clipped to the pane's view
  rectangle. The panel's fill/border moved to `menu_panel_style`
  (`src/terminal/widget.rs:2861-2873`): the theme's elevated component
  surface forced opaque — visible once the layer puts the quad on top,
  never the fix itself.
- **Why the grid beat the menu.** The terminal grid draws in one layer:
  cell backgrounds as quads (`BgRect::fill` around
  `src/terminal/widget.rs:947-961`), then all glyphs as a single text
  primitive `renderer.fill_raw(terminal.buffer_weak(), …)`
  (`src/terminal/widget.rs:969-974`). The menu panel was a quad issued
  after that text — and under wgpu, call order between *kinds* is
  meaningless.
- **Why the ghost never had the bug.** The dim autosuggestion ghost is
  also `fill_raw` text (`src/terminal/widget.rs:1032-1034`), drawn after
  the grid's text in the same layer — same kind, same batch, insertion
  order preserved. Text-over-text works in one layer; quad-over-text
  never does.
- **DropTerm's other floating surfaces are already Overlay-trait
  widgets.** The right-click menu wraps the terminal in libcosmic's
  `widget::context_menu` (`src/app.rs:2058-2061`); the settings drawer is
  `widget::context_drawer` (`src/app.rs:2332`). The suggestion menu is the
  one floating surface drawn *inside* the terminal widget's own `draw()`,
  because it is anchored to live grid geometry (the cursor's cell,
  per-frame metrics) rather than to a widget-tree layout.
- **The ranking amendment, same working tree.** `menu_source_rank`
  (`src/terminal/suggest.rs:1343-1350`) orders the menu's
  providers Path → Specs → Generator → History → Catalog, while the ghost
  keeps the locked `source_rank` with history on top.
- **The pinned stack.** `Cargo.lock` resolves `iced`, `iced_wgpu`,
  `iced_graphics`, `iced_runtime`, `iced_core` 0.14.0 and `libcosmic`
  1.0.0 all from `git+https://github.com/pop-os/libcosmic#08a09e3dfa1f…`
  (`Cargo.lock:2249-2253`, `:2406-2409`, `:2936-2939`); the checkout at
  `~/.cargo/git/checkouts/libcosmic-41009aea1d72760b/08a09e3` is commit
  `08a09e3dfa1f729d52585fcedd6e731130235363` (2026-09-22), whose in-repo
  `iced/` is the pop-os fork of iced 0.14.0. All `iced/…` citations below
  are inside that checkout; all `src/widget/…` libcosmic citations too.

## Findings

### 1. The root cause, verified against the exact sources DropTerm builds

- **(a) A wgpu layer batches by primitive kind.** `iced_wgpu`'s `Layer`
  holds five independent batches — `quads`, `triangles`, `primitives`,
  `images`, `text` (`iced/wgpu/src/layer.rs:20-29`). `fill_quad` appends
  to the current layer's quads batch (`iced/wgpu/src/lib.rs:741-748`);
  every text call appends to its text batch (`:791-834`).
  `Renderer::render` walks the layer stack and, *within each layer*,
  draws quads (`iced/wgpu/src/lib.rs:487-499`), then triangles
  (`:501-536`), then shader primitives (`:538-633`), then images
  (`:635-647`), then text (`:649-660`). So a quad issued after text in
  the same layer still paints first — confirming the diagnosis verbatim.
  Within one kind order *is* preserved (text `Batch = Vec<Item>`,
  `iced/wgpu/src/text.rs:22`; quads keep an explicit `order` log,
  `iced/wgpu/src/quad.rs:246-256`) — which is exactly why the
  text-after-text ghost always worked.
- **(b) Child layers render wholly after their parent.** `with_layer` is
  `start_layer` + closure + `end_layer` (`iced/core/src/renderer.rs:24-28`),
  which the wgpu renderer maps to the layer `Stack`'s `push_clip`/`pop_clip`
  (`iced/wgpu/src/lib.rs:725-731`). `push_clip` starts a *new* layer at a
  higher stack index, with bounds = the given bounds ∩ the parent's
  (`iced/graphics/src/layer.rs:85-105`); `layers.iter()` yields stack
  order, so the child's every kind renders after the parent's every kind
  in the `render` loop. The one escape hatch — `Stack::merge()` flattening
  children back (`iced/graphics/src/layer.rs:150-205`, invoked in
  `prepare` at `iced/wgpu/src/lib.rs:315`) — is guarded so it can never
  change visible order: a candidate merges only if its highest kind level
  (`end()`) is ≤ the target's lowest (`start()`) *and* the bounds match
  (`iced/graphics/src/layer.rs:178`); levels are quads=1 … text=5
  (`iced/wgpu/src/layer.rs:336-382`), so a parent holding text (end=5)
  never absorbs a child holding quads (start=1). Even when merging does
  occur, `Layer::merge` appends per-kind in draw order
  (`iced/wgpu/src/layer.rs:384-390`).
- **(c) `with_layer` clips to its bounds in the pinned version.** The
  trait documents it ("The layer will clip its contents to the provided
  `bounds`", `iced/core/src/renderer.rs:21-24`); `push_clip` intersects
  with the parent's bounds (`iced/graphics/src/layer.rs:86-93`); and in
  `render` each layer's bounds become the GPU scissor rect for its quads
  and text (`iced/wgpu/src/lib.rs:477-485`, `:649-658`). DropTerm's
  `with_layer(view, …)` therefore keeps the panel's drop shadow inside
  the pane — the observed behavior is the documented behavior.
- **The tiny_skia correction.** The fix's block comment says the
  tiny_skia applet path "rasterizes in call order within a layer, where
  the explicit layer is harmless". It does not: `iced_tiny_skia`'s
  `draw` walks the same layer stack and renders, per layer, quads →
  primitives → images → text (`iced/tiny_skia/src/lib.rs:107-195`, its
  `Layer` with separate `quads`/`primitives`/`images`/`text` vecs at
  `iced/tiny_skia/src/layer.rs:17-23`). The explicit layer is the
  z-order mechanism on *both* backends. The error is inert here only
  because the applet consumes the lib with `default-features = false`
  (`applet/Cargo.toml:31`) and the terminal widget lives behind the
  default-on `terminal` feature (`Cargo.toml:126-133`) — the applet never
  renders a terminal at all. The comment should be corrected when the
  fix is committed.

### 2. How iced itself draws floating menus

- **The `Overlay` trait is drawn *through* `with_layer`.**
  `UserInterface::draw` draws the whole root widget tree first, then the
  overlay (`iced/runtime/src/user_interface.rs:514-548`). The bridge,
  `overlay::Nested::draw`, wraps every overlay's draw in
  `renderer.with_layer(layout.bounds(), |renderer| overlay.draw(…))`
  (`iced/core/src/overlay/nested.rs:101-113`) — added by PR #1719
  "Nested overlays" (tarkah, merged 2023-06-14) to close issue #30.
- **`overlay::Menu` (what pick_list and combo_box show) is plain draws on
  top.** `Menu::overlay` turns the menu into an `overlay::Element`
  (`iced/widget/src/overlay/menu.rs:128-148`); pick_list
  (`iced/widget/src/pick_list.rs:735-751`) and combo_box
  (`:865-896`) return it from `Widget::overlay`. Its `draw` fills a panel
  quad (border + shadow) then lets the list draw text rows
  (`overlay/menu.rs:314-339`, rows at `:517-590`) — no `with_layer` of
  its own; the z-order is inherited from `Nested`'s wrapper. That is the
  entire difference from DropTerm's menu: same final mechanism, reached
  via the widget-tree overlay plumbing instead of a direct call.
- **iced's `float` widget is DropTerm's shape exactly.** Added in 0.14.0
  by hecrj's PR #2916 ("a new float widget that can be used to make a
  widget float over other widgets"), its `draw` is
  `renderer.with_layer(self.viewport, …)` containing a shadow quad and
  the content widget (`iced/widget/src/float.rs:328-370`). A first-class
  iced 0.14 widget whose whole job is this z-order ships the identical
  recipe to DropTerm's fix.
- **`with_layer` is idiomatic throughout iced's widget crate** — `hover`
  (base/top, `iced/widget/src/helpers.rs:841`), `stack`
  (`iced/widget/src/stack.rs:354`), `pane_grid` (`:948`), `scrollable`
  (`:1368`, `:1433`, `:1845`), `text_input` (`:649`), `svg` (`:354`).

### 3. libcosmic: the popover/menu family, and what DropTerm's closest peer does

- **libcosmic's menu is an Overlay *and* calls `with_layer` itself.**
  `Menu::overlay` returns an `overlay::Element`
  (`src/widget/menu/menu_inner.rs:461`); inside `draw`, per menu level,
  the panel quad (`fill_quad`, `menu_inner.rs:827-841`), the active-path
  highlight quad (`:862-875`), and the item rows (`:877-908`) are all
  closed over in `draw_menu` and rendered by
  `renderer.with_layer(render_bounds, draw_menu)` (`:912`) — a nested
  child layer per level, on top of the layer `Nested` already gave it.
  Every COSMIC menu (menu bar, context menu, dropdown) draws through
  this code; DropTerm's panel + highlight + rows inside one `with_layer`
  is this function with the nesting depth removed.
- **popover, context_menu, dropdown, context_drawer are all
  `Widget::overlay` implementors** (`src/widget/popover.rs:241-288`,
  `src/widget/context_menu.rs:589`, `src/widget/dropdown/widget.rs:342`,
  `src/widget/context_drawer/widget.rs:305`) — each floats by riding
  `Nested`'s `with_layer`, adding nothing of their own.
- **cosmic-term never faces this bug class inside the terminal widget.**
  At master `8a05e91948814afc4162d9fcdc95875c8cc69400`,
  `src/terminal_box.rs` contains zero `with_layer` calls (grep): the grid
  is one `fill_raw` text primitive (`terminal_box.rs:726`) with
  background/underline/scrollbar quads around it (`:449-660`), URL hints
  are in-grid underline quads, and search is in-grid highlighting
  (`src/terminal.rs:52`, `:246-259`) — nothing floats over the grid
  inside the widget. Its menus are libcosmic's `responsive_menu_bar` and
  `context_menu` (`src/menu.rs:42`, `:138-152`), i.e. Overlay-trait
  widgets. cosmic-files is the same pattern
  (`src/app.rs:2502` `context_menu(…)`, `src/menu.rs:50`).
- **Other terminal emulators dodge the class by owning the scene.**
  alacritty ships its own GL renderer (`alacritty/src/renderer/`:
  `mod.rs`, `rects.rs`, `text/atlas.rs`, `text/gles2.rs`); WezTerm's GUI
  crate carries its own shaders and an overlay subsystem
  (`wezterm-gui/src/renderstate.rs`, `quad.rs`, `glyph-frag.glsl`,
  `shader.wgsl`, `overlay/`); kitty has its own OpenGL compositor
  (`kitty/gl.c`, `kitty/glfw.c`). Each draws its floating surfaces by
  ordering its own draw calls — an option DropTerm structurally lacks,
  since its grid lives inside iced's scene graph as one text primitive.

### 4. The upstream paper trail: layers are *the* blessed z-order

- **iced-rs/iced issue #30 "Layers"** (hecrj, 2019-10-23, closed
  2024-02-07 "Solved by #1719") is the canonical tracking issue: iced
  "assumes widgets cannot be laid out on top of each other", and hecrj
  explicitly rejected CSS-style z-index ("using something like absolute
  indices … can be quite painful and does not compose at all") in favor
  of the relative, compositional layering that shipped.
  <https://github.com/iced-rs/iced/issues/30>
- **PR #1719 "Nested overlays"** (tarkah, merged 2023-06-14, 0.10) built
  the nesting that `Nested::draw` rides.
  <https://github.com/iced-rs/iced/pull/1719>
- **PR #2916 "`float` widget and other cool stuff"** (hecrj, merged
  2025-05-01, milestone 0.14) made floating-over-siblings a first-class
  widget in the very version line DropTerm pins, implemented via
  `with_layer`. <https://github.com/iced-rs/iced/pull/2916>
- The vendored `iced/CHANGELOG.md` corroborates the timeline: 0.14.0
  adds the "`float` widget" (`CHANGELOG.md:27`) and "Basic layer merging
  for `graphics::layer::Stack`" (`:38`); 0.10.0 lists "Nested overlays"
  (`:806`).

### 5. Ranking provenance (light pass): scope-first, history-after

- **Explicit-completion surfaces show scope, never history.** fish's
  pager renders completion lists only — zero history references in
  `src/pager.rs` @ master (grep), the list is the completion engine's
  output sorted naturally (`src/complete.rs:686-689`). bash's
  programmable completion likewise: once the command word is settled,
  git's own `contrib/completion/git-completion.bash` completes "git
  'subcommands'" (header, L12) via per-subcommand functions (L36), and
  history exists only in readline's up-arrow. zsh's menu-completion
  cycles the completion system's `compadd` matches
  ([zsh docs, Completion
  System](https://zsh.sourceforge.io/Doc/Release/Completion-System.html)).
  A completion list whose rows are the command's own next words —
  DropTerm's scoped tier — is the shell norm.
- **Single-choice ghosts are history-first, and DropTerm keeps that.**
  zsh-autosuggestions defaults `ZSH_AUTOSUGGEST_STRATEGY=(history)`
  (`zsh-autosuggestions.zsh:44-47`), strategies are tried in array order
  until one yields (`:745-756`), and the README's `history completion`
  example is explicitly "first … from your history, but, if it can't
  find a match … from the completion engine". fish's autosuggestions are
  "based on command history, completions, and valid file paths" (fish
  docs, interactive mode) — history named first; the repo's own
  autosuggest-stage2-crosscheck record pinned the ordering from fish
  3.7.1 source (history-first, one ranked completion on ambiguity).
- **Where completions do rank, relevance — not recency — orders them.**
  fish's autosuggest-completion ranker sorts by case-fold, duplicate-arg
  penalty, and tilde penalty (`src/complete.rs:690-700` @ master); the
  pager's list is natural-sorted (`:686-689`). fzf ranks by match score
  with `--tiebreak` defaults of `length` plus an implicit trailing
  `index` ("Prefers line that appeared earlier in the input stream",
  fzf(1)), and its CTRL-R history widget ships `--scheme=history` —
  "well suited for command history … chronological ordering … sets
  `--tiebreak=index`" — plus `ctrl-r:toggle-sort`
  (`shell/key-bindings.bash:102`).
- **The combined list is DropTerm's own hybrid.** No surveyed shell
  shows scope and history interleaved in one list; each keeps them on
  separate surfaces (Tab vs up-arrow/ghost). Putting the seat's scope
  above history inside one explicit-trigger menu is consistent with the
  completion-surface norm (scope is what a completion list is *for*) and
  keeps recency as the secondary tier, with fzf's scheme/tiebreak pair
  as the nearest single-list analog. Nothing found contradicts it.

## Recommendation

**Keep `with_layer(view, …)` — it is the proven mechanism, reached the
short way.** The alternatives, graded:

1. **Direct `with_layer` inside the terminal widget's `draw()` (what the
   fix does) — adopt.** This is iced's `float` widget's own
   implementation and libcosmic's own menu's inner draw, applied without
   the overlay plumbing. It keeps the menu anchored to live grid state
   (cursor cell, metrics) with zero new widgets, and the keyboard-driven
   menu needs none of the event routing the Overlay trait would buy.
   Matches the ecosystem's most precedented shape for a floating panel
   with shadow, border, and rows.
2. **Promote the menu to an `Overlay`-trait widget — the right *next*
   step, not the right first one.** It would produce the same z-order
   (`Nested::draw` wraps it in the same `with_layer`), plus automatic
   event handling above the content — the day the menu wants mouse
   hover/click on rows, like pick_list and libcosmic's menus. The cost
   is real: a widget `Tree`, an `overlay()` implementation anchored to
   terminal geometry, and state routing through the app — a refactor,
   not a fix. Defer until mouse interaction is actually wanted (and note
   libcosmic's menu still needs its own `with_layer` inside its Overlay
   for multi-level stacking — the trait alone is not always enough even
   for them).
3. **Redraw the grid inside a layer beneath the menu / reorder calls —
   rejected.** Reordering cannot work by construction (§1a); redrawing
   the grid per menu frame would copy the entire terminal buffer for a
   transient popup.
4. **Draw the menu into the grid's own buffer / as text primitives —
   rejected.** The autosuggest records' standing verdict: overlays never
   touch the terminal's buffer (selection, search, scrollback, and copy
   must see nothing). A same-kind (text-after-text) hack would also
   surrender the panel's rounded quad, border, and shadow, which only
   the quad pipeline draws.
5. **Own the whole scene like alacritty/wezterm/kitty — not available.**
   Their escape from this bug class is owning the renderer; DropTerm's
   grid is one text primitive inside iced's scene graph by design (that
   is what buys IME, a11y, theming, and the drawer/context menu for
   free).

**On ranking:** keep the amendment. The ghost stays history-first
(zsh-autosuggestions' and fish's exact single-choice shape, and the
locked merge tests), and the explicit menu leads with the seat's scope
(every shell's Tab-surface behavior) with history as the second tier —
fzf's recency-aware history scheme is the closest single-list analog.
The combined list is novel but contradicted nowhere.

**One commit-time correction:** rewrite the draw-site comment's
tiny_skia sentence — iced_tiny_skia also batches by kind per layer
(`iced/tiny_skia/src/lib.rs:107-195`), so the explicit layer is
necessary on both backends (and the applet renders no terminal at all).

## Unverified / open questions

- Whether the pop-os fork changed any of the layer machinery relative to
  upstream iced 0.14.0 — not diffed; the findings are from the fork's
  own tree, which is the code DropTerm builds, so this affects
  attribution only, not the verdict.
- hecrj's PR #2916 description was fetched from the PR page, not
  `git show`; the implementation cited is the vendored `float.rs`
  itself, which is authoritative for behavior.
- Warp's completion-menu ordering was not re-verified here (the
  autosuggest-warp-parity record pinned its prefix-then-subsequence
  ranker from source); this record's ranking section deliberately covers
  only fish, zsh, fzf, and bash, as tasked.
- fish's autosuggestion *selection* order (history first) was verified
  from the fish 3.7.1 C++ source by the autosuggest-stage2-crosscheck
  record; today's Rust port exposes the autosuggestion completion
  ranker (`src/complete.rs:690-700`) but the history-first selection
  site was not re-located in the Rust tree — cited via the docs
  ("based on command history, completions, and valid file paths") plus
  the prior record's pinned source.

## Sources

- Pinned local checkout: pop-os/libcosmic @
  `08a09e3dfa1f729d52585fcedd6e731130235363` (2026-09-22) at
  `~/.cargo/git/checkouts/libcosmic-41009aea1d72760b/08a09e3`; DropTerm
  `Cargo.lock:2249-2253, 2406-2409, 2936-2939` pins every iced crate to
  it. Files read: `iced/core/src/renderer.rs` L13-28;
  `iced/core/src/overlay/nested.rs` L63-131;
  `iced/core/src/overlay/element.rs` L119-128;
  `iced/graphics/src/layer.rs` L1-229; `iced/runtime/src/user_interface.rs`
  L505-549; `iced/wgpu/src/layer.rs` L1-407; `iced/wgpu/src/lib.rs`
  L301-418, L420-676, L725-840; `iced/wgpu/src/text.rs` L22;
  `iced/wgpu/src/quad.rs` L246-262; `iced/tiny_skia/src/lib.rs` L67-205;
  `iced/tiny_skia/src/layer.rs` L17-23; `iced/widget/src/float.rs`
  L1-80, L300-370; `iced/widget/src/helpers.rs` L741-757, L820-845;
  `iced/widget/src/overlay/menu.rs` L1-339, L517-590;
  `iced/widget/src/pick_list.rs` L735-751; `iced/widget/src/combo_box.rs`
  L865-896; `iced/widget/src/{stack,pane_grid,scrollable,text_input,svg}.rs`
  (with_layer sites); `iced/CHANGELOG.md` L28, L38, L806;
  `src/widget/popover.rs` L241-288; `src/widget/context_menu.rs` L589;
  `src/widget/dropdown/widget.rs` L342; `src/widget/context_drawer/widget.rs`
  L305; `src/widget/menu/menu_inner.rs` L461, L795-921 (all read
  2026-11-06)
- iced upstream: issue #30 "Layers" (opened 2019-10-23, closed
  2024-02-07 "Solved by #1719", hecrj's z-index rejection quoted) —
  <https://github.com/iced-rs/iced/issues/30>; PR #1719 "Nested
  overlays" (tarkah, merged 2023-06-14) —
  <https://github.com/iced-rs/iced/pull/1719>; PR #2916 "float widget
  and other cool stuff" (hecrj, merged 2025-05-01, 0.14) —
  <https://github.com/iced-rs/iced/pull/2916> (all fetched 2026-11-06)
- DropTerm local (working tree, uncommitted): `src/terminal/widget.rs`
  (with_layer site L1147-1214, `menu_panel_style` L2861-2873, grid
  `fill_raw` L969-974, ghost L1032-1034, draw-order comment L1042-1082),
  `src/terminal/suggest.rs` (`menu_source_rank`), `src/app.rs` L2058-2061,
  L2332, `Cargo.toml` L126-133, `applet/Cargo.toml` L31 (read 2026-11-06)
- cosmic-term: pop-os/cosmic-term @ master
  `8a05e91948814afc4162d9fcdc95875c8cc69400` — `src/terminal_box.rs`
  (no with_layer; grid `fill_raw` L726; underline quads L602-662),
  `src/terminal.rs` L52, L246-259, `src/menu.rs` L42, L138-152
  (fetched 2026-11-06)
- cosmic-files: pop-os/cosmic-files @ master
  `3868a8e3ac65f7572861c4668cee4564ac473974` — `src/app.rs:2502`,
  `src/menu.rs:50` (fetched 2026-11-06)
- Other terminals (structure only): alacritty/alacritty master —
  `alacritty/src/renderer/{mod.rs,rects.rs,shader.rs,text/}`; wez/wezterm
  main — `wezterm-gui/src/{renderstate.rs,quad.rs,uniforms.rs,overlay/}`,
  `glyph-frag.glsl`, `shader.wgsl`; kovidgoyal/kitty master —
  `kitty/{gl.c,glfw.c}` (directory listings fetched 2026-11-06)
- fish: fish-shell/fish-shell @ master
  `d92ccd4f8f6f08cfc8a0d9fa17333153e2922af1` — `src/complete.rs`
  L336-346, L686-700; `src/pager.rs` (no history refs); autosuggestion
  docs — <https://fishshell.com/docs/current/interactive.html>
  (fetched 2026-11-06); fish 3.7.1 ordering as pinned by this repo's
  autosuggest-stage2-crosscheck record
- zsh-autosuggestions: zsh-users/zsh-autosuggestions @ master
  `85919cd1ffa7d2d5412f6d3fe437ebdbeeec4fc5` —
  `zsh-autosuggestions.zsh` L44-47, L745-756; `README.md` L52-58
  (fetched 2026-11-06); zsh Completion System docs —
  <https://zsh.sourceforge.io/Doc/Release/Completion-System.html>
- fzf: junegunn/fzf @ master `96028e2a54023a8ff8240e985ae9c8e4f25c0089`
  — `man/man1/fzf.1` L66-101, L186-214; `shell/key-bindings.bash` L102
  (fetched 2026-11-06)
- bash/git completion: git/git @ master
  `0ffa3e35c7c82ab8d0de226149fb03b3f7674d05` —
  `contrib/completion/git-completion.bash` L12, L36 (fetched 2026-11-06)
