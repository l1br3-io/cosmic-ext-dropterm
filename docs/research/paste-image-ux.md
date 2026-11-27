# Research: how should a pasted clipboard image present — inline pixels, a text pointer, or a per-tab images panel?

Date: 2026-11-08
Question: *Paste-image PI-1/PI-2 (commits `cb39a39`, `d9b8151`) landed the full
stack — clipboard image bytes → cache file → path pasted as text → full inline
render at the cursor — and it works but is not presentable. Which presentation
should DropTerm ship: the inline grid render, a text placeholder pointing
elsewhere, or a status-bar chip opening a per-tab images panel — and what do the
other terminals and terminal-adjacent apps actually do?*

## TL;DR

**Ship the maintainer's proposal (candidate C): keep the pasted path as the
grid's only trace, drop the full-size auto-render from the paste path, and add
a per-tab images panel behind a conditional status-bar count chip.** The
precedent field is unanimous on the part that matters: none of the ten surveyed
terminals pastes clipboard images at all — text-only everywhere — and the two
products that do handle clipboard images at the GUI layer (Warp's input editor,
Claude Code in any terminal) both collapse the image to a compact chip/token
and never put pixels into a scrollback. The one thing every consumer of a
pasted image wants from the terminal is the file path (the Ghostty and Windows
Terminal communities describe exactly DropTerm's PI-1 as the clean solve), and
the inline render — the part no peer ships — is also the part with the honest
defects: evicted pixels leave blank cells (docs/images.md:51), a tall screenshot
displaces the whole viewport, and the scrollback ends up holding terminal-dressed
output that no program produced. The panel is not novel-afraid design either:
iTerm2's Toolbelt carries Paste History and Recent Directories lists with
click/paste/type-path actions, and CopyQ/Klipper are the OS-level history-list
analogs — a per-tab list with thumbnails and per-item actions has three
independent precedents. DropTerm's own band already has the exact chip this
needs: the Scrolled chip is conditional (hidden when idle) and its click runs
an action verb instead of copy (`src/status_bar.rs:997`).

## What DropTerm has today

PI-1 and PI-2 are done (`tasks/paste-image.md`: cb39a39 "The read seam and the
file path", d9b8151 "The inline render"); PI-3 (config gate + docs) and PI-4
(retention + manual checklist) are not started. The state, by seam:

- **Read seam** — `clipboard::read_for_paste(primary) -> Task<Option<PasteContents>>`
  (`src/clipboard.rs:293`): the text read first; only its empty/no-mime
  classification falls through to `load_image` pinned to
  `PasteMime::Specific("image/png")` (`src/clipboard.rs:171-180`). The gate is
  the engine-true `const PASTE_IMAGE_ENABLED: bool = true`
  (`src/clipboard.rs:88`) pending PI-3.
- **Cache** — `write_paste_image` (`src/clipboard.rs:376-406`) writes
  `~/.cache/dropterm/paste/paste-YYYYMMDD-HHMMSS-<n>.png`
  (`paste_cache_dir` `:347-349`, `paste_stamp` `:353-355`,
  `next_paste_name` `:360-369`). The directory is global, with no per-tab
  attribution and no retention yet (PI-4's 7-day startup sweep is still only a
  spec ruling, SPEC-paste-image.md design item 4).
- **Handler** — `Message::ClipboardImagePasted` (`src/app.rs:3257-3273`):
  `write_paste_image(&bytes)`, then `terminal.paste(&path)` (the same channel
  text pastes use, bracketed paste riding unchanged), then
  `terminal.place_pasted_image(&bytes)`. Paste arms route through
  `tab_actions.rs:352-359` and `paste_contents_message` (`:1401-1404`).
- **Render seam** — `place_pasted_image` (`src/terminal/engine.rs:1936`) decodes
  under the OSC ceilings (32 MiB payload cap, `engine.rs:4122`), and
  `place_decoded_image` (`:1961`) dices the pixels into grid cells via the
  fork's `Term::place_image`. Pixels live in the shared store
  (`images: Arc<Mutex<ImageStore>>`, `engine.rs:720`) under the whole-terminal
  budget — "320 MiB total decoded pixels across all images, oldest evicted"
  and "Evicted images leave their cells blank — the placement stays in the
  grid, the pixels are gone" (docs/images.md:49-51). The paste entry is gated
  by the same `images_enabled` flag as OSC rendering (`engine.rs:1937-1939`).
- **Status bar** — `SegmentId` (`src/status_bar.rs:133`), the two clusters
  (`LEFT_CLUSTER` `:175-181`, `RIGHT_CLUSTER` `:188-194`), the width-pressure
  drop order (`DROP_ORDER` `:210-220`), the band's transient write
  confirmation (`pub enum Copied`, `:118-121`), and one action chip: `click_jumps(id) == SegmentId::Scrolled`
  (`:997-999`) — a conditional segment whose click runs an app verb instead of
  the copy verb.
- **Panel seams that already exist** — the settings drawer (a libcosmic
  context drawer), and the in-widget
  overlay panel the completion menu uses: `with_layer` stacking at
  `src/terminal/widget.rs:1146-1230` with `menu_panel_bounds` (`:1208`) and
  `menu_panel_style` (`:1229`). One escalation path is
  named for it: "the Overlay-trait route is the same z-order plus
  event routing — the right next step when the menu wants mouse."
- **Rulings in force** — SPEC-paste-image.md OQ-5 ruled the pasted text is the
  bare path ("no `[Image #1]` wrapper — the token means nothing to a shell;
  the inline render is the visible confirmation"). OQ-2 ruled XDG cache + 7-day
  cleanup. The module's motivating user is "anyone pasting a screenshot or
  copied image into a terminal: the path feeds AI CLIs that accept file
  arguments; the inline render proves the paste landed."

This record does not touch the terminal-images decision (2026-11-05): OSC 1337
and kitty placements of *program-driven* images stay grid cells end to end.
What it rules is the one *paste-driven* placement — whether the terminal should
spontaneously render pixels the user copied, on every paste, at full size.

## Findings

### 1. No surveyed terminal pastes images — the paste action is text-only in all ten

| Terminal | Clipboard-image paste | Source |
|---|---|---|
| iTerm2 | Text-only. Paste Special is text-shaped (escape special characters, paste slowly "in batches of 16 bytes with a 125ms delay"); Paste History "shows recently pasted strings" | iterm2.com 3.0 menu-items docs (fetched 2026-11-08); Ghostty #10478, Mitchell Hashimoto (Ghostty maintainer), 2026-03-10: "Fwiw I can't get Cmd+V to work in any macOS terminal I've tried yet... Terminal, iTerm2, Kitty (ctrl+v works fine in all). EDIT: works in Warp." |
| WezTerm | Text-only. `PasteFrom(source)` pastes "the specified clipboard"; no mime selection anywhere. Issue #7272 "Pasting Images" (opened 2025-09-30) closed with no implementation | wezterm.org PasteFrom docs; wezterm/wezterm#7272 (both fetched 2026-11-08) |
| kitty | Text-only paste (`paste_actions` are text-shaped). `kitten clipboard` reads/writes the clipboard *including images* — but as a CLI pipe ("kitten clipboard -g picture.png"), never a paste-into-terminal path | sw.kovidgoyal.net/kitty/kittens/clipboard/ (fetched 2026-11-08) |
| Ghostty | Text + file URLs only. Root cause read from source by a contributor: `getOpinionatedStringContents()` in `NSPasteboard+Extension.swift` "only handles file URLs and strings... raw PNG/TIFF image data... falls through both branches and returns nil. That is why the paste silently does nothing." A proposed fix (PR #11571, 2026-03) is exactly PI-1's shape: "check for image data... write it to a temp file, and return the escaped path" | ghostty-org/ghostty discussion #10478 (fetched 2026-11-08) |
| Windows Terminal | Text-only; the request (discussion #19397, opened 2025-09-30, unanswered) proposes DropTerm's exact design: "paste a temp file path that point to the image is enough... We just need a standard way to implement" | microsoft/terminal discussion #19397 (fetched 2026-11-08) |
| rio | Text-only paste; image *rendering* ships (all three protocols) but clipboard image paste does not exist | rio repo/search, no image-paste feature in tracker (checked 2026-11-08) |
| Konsole | Text-only ("Pasting images doesn't work on Konsole", issue Dec 2025; a KDE Brainstorm post states PNG paste is unsupported by design even though Konsole *renders* Sixel/iTerm2/kitty images) | KDE forum/issue tracker via search (issue not fetched directly — see Unverified) |
| Alacritty | Text-only; the paste path has never carried anything but text | alacritty config docs; nothing in the tracker (checked 2026-11-08) |
| xterm.js | Text-only by construction: the browser `paste` event on xterm.js's hidden textarea "only delivers text, not image data" (issue #1411) | xtermjs/xterm.js#1411 (fetched via search, 2026-11-08) |
| Hyper | Text-only; no image-paste feature or issue in the tracker | vercel/hyper issue search (checked 2026-11-08) |

Two facts sharpen the table. First, the Ctrl+V image paste that works "in
iTerm2/kitty/Terminal" in the wild is Claude Code reading the clipboard itself
when it sees a bare ^V — Windows Terminal collaborator j4james, 2025-10-11:
"I suspect Claude Code may be reading the image directly from the clipboard
with an OS API call, and that's triggered when you press a certain keyboard
shortcut... on Windows you need to use Alt+V" (terminal #19397). The terminal
is never the reader. Second, Warp is the sole GUI-layer exception, and it
routes images to its own input editor, never to a grid: the 2026 changelog
reads "Added support for attaching images to prompts in Warp's TUI" and
"Fixed Ctrl-V in the TUI to paste text when the clipboard does not contain an
image" (2026.07.23), "Restored image pasting into CLI agents when macOS
provides TIFF clipboard data" (2026.09.23) — docs.warp.dev/changelog/2026
(fetched 2026-11-08).

Consequence: DropTerm's PI-1 — the terminal reads the image, writes a file,
pastes the path — is *ahead of every peer*, and it is precisely what the peer
communities ask their maintainers for (Ghostty PR #11571, terminal #19397,
and the Ghostty thread's "just save to `$TMPDIR` and write the path into the
PTY — would solve this cleanly"). The read/file/path half of the module is
past-preserving and precedent-confirming; only the presentation of the result
is unsettled.

### 2. The one precedent that ships clipboard images (Warp) shows compact chips, never pixels in scrollback

Warp's docs describe copy-paste as an attachment route: "Copy and paste images
directly (e.g. right-click an image > 'Copy image' or copy from a file manager)
into Warp", formats ".jpg, .jpeg, .png, .gif, and .webp", consumed as agent
context with automatic resizing ("Warp will intelligently resize images before
passing them as context") — docs.warp.dev, "Images as Context" (updated
2026-10-26, fetched 2026-11-08). Warp owns an input editor, so the image
becomes a compact object in that editor — the community and issue tracker
describe them as "image chips" (warpdotdev/warp #6648, 2025-06) — and a 2025
issue records the older behavior Warp replaced: "Dropping an image now shows
'Switch to Agent Mode to paste images' instead of inserting the file path"
(user-reported issue, Aug 2025). Warp notably never renders a pasted clipboard
image into its terminal grid, despite shipping all three image-render
protocols (terminal-images.md §4).

Claude Code, the consumer DropTerm's module explicitly targets ("the Claude
Code contract", SPEC-paste-image.md), is the second chip precedent, and its
docs state the shape exactly: "Ctrl+V or Cmd+V (iTerm2) or Alt+V (Windows and
WSL) — Paste image from clipboard — Inserts an `[Image #N]` chip at the cursor
so you can reference it positionally in your prompt" —
code.claude.com/docs/en/interactive-mode (fetched 2026-11-08). In the Ghostty
thread, users show what that chip is backed by: Claude Code writes its own
cache file and the chip links it — "it appears as
`[[Image #1]](file:///Users/q/.claude/image-cache/f048…/1.png)`", while the
pi-mono agent "displays a full path, e.g.,
`/var/folders/.../pi-clipboard-a9b5d55c….png`" (#10478, comment 2026-02-28).
Every shipped example therefore keeps the text stream short and puts the bytes
in a named, durable file — the same division DropTerm already built.

### 3. Editors that paste images insert a file plus a reference — the path *is* the placeholder

- **VS Code** (v1.79 release notes, May 2023, fetched 2026-11-08): pasting an
  image into a Markdown file copies it into the workspace — "If the file
  currently isn't part of the workspace, VS Code will automatically copy the
  file into your workspace and insert a link to it" — with the destination
  templated by `markdown.copyFiles.destination` globs and duplicates made
  distinct ("image-1.png, image-2.png", no overwrite by default). The inserted
  marker is a *relative path inside a Markdown image link*.
- **JetBrains**: built-in Markdown ships an Insert Image dialog but not a
  clipboard-paste path; the capability exists through plugins —
  holgerbrandl/pasteimages: "Images files are deposited in a configurable
  directory relative to the markdown... [the plugin] will deposit the file
  relative to your source-directory and insert the markdown image element at
  the current cursor position" (README, fetched 2026-11-08).
- **Claude Code / pi**: positional token with a cache file behind it (Finding 2).

The common shape across all of them: a generated name (`image-1.png`,
`pi-clipboard-<uuid>.png`, DropTerm's `paste-YYYYMMDD-HHMMSS-<n>.png`), a file
in a managed location, and a short text marker that references it. None of
them makes the marker a bare invented token without a file, and none embeds
pixels into the text stream. DropTerm's bare-path marker (OQ-5) sits inside
this family — it is the terminal-flavored version of VS Code's inserted
reference, with the honesty property that a shell can consume it directly.

### 4. The panel precedents: iTerm2's Toolbelt lists, and the OS clipboard managers

- **iTerm2 Toolbelt** (iterm2.com 3.0 menu-items docs, fetched 2026-11-08):
  a right-side dock of per-session tools, among them **Paste History** —
  "the Paste History tool, which shows recently pasted strings in the
  toolbelt" — with the window twin "Open Paste History" showing "up to the
  last 20 values that were copied or pasted in iTerm2", filtered by a typed
  subsequence, chosen by arrow keys/enter "or you can click on an item", which
  pastes it into the session; persistence via "Save copy/paste history to
  disk". **Recent Directories** — "shows recently used directories sorted by a
  combination of recency and frequency of use", with per-item actions that
  reach into the terminal: double-click types the path, option-double-click
  enters `cd`, right-click stars it (shell-integration docs, fetched
  2026-11-08). iTerm2's Downloads feature ("lets you view downloaded files and
  track their progress") is a menu-bar item rather than a Toolbelt tool — the
  task's lead of a "Recent Files/Downloads toolbelt tool" did not verify (see
  Unverified). Every one of these lists is text and paths; iTerm2 ships no
  image list and no image paste.
- **CopyQ** (hluk/CopyQ README, fetched 2026-11-08): the OS-level history
  panel — "Copying text or image to the clipboard will create a new item in
  the list. Selected items can be: edited (F2), removed (Delete), sorted";
  image items live in the same list as text.
- **KDE Klipper** (KDE handbook, "Using Klipper", fetched 2026-11-08): a tray
  popup whose history has a search field ("results are updated as you type")
  and four per-item hover actions — "four icons appear which enable you to
  invoke an action, show the barcode, edit the content or remove the entry
  from the history."

So the maintainer's panel has three independent precedents — a terminal-native
dock of per-session history lists with click-to-act items (iTerm2), and two
OS-level clipboard history panels with per-item actions (CopyQ, Klipper).
What none of them does is show *thumbnails*: iTerm2's lists are text, CopyQ's
and Klipper's are list rows (CopyQ renders image items as previews per its
README/issues; Klipper's HTML handbook documents no thumbnails). A
thumbnail-grid panel would exceed every precedent; a list panel with small
previews matches CopyQ.

### 5. The grid-integrity facts about inline paste renders

- **The mechanism is sound** — a pasted image rides cells exactly like an OSC
  placement: scrollback, reflow, resize, alt-screen clearing treat it as text
  (docs/images.md:10, "There is no floating overlay to fall out of sync";
  terminal-images.md §6, WezTerm's sliced-`ImageCell` model DropTerm adopted).
  Selection, search, and copy behave as if the image were text.
- **The pixels are perishable** — the store evicts oldest-first at 320 MiB
  total and "evicted images leave their cells blank — the placement stays in
  the grid, the pixels are gone" (docs/images.md:49-51). A paste render is
  the placement most likely to be scrolled far and revisited blank, and the
  file behind it (the cache path) survives while the pixels do not — an
  inversion of what a user expects from something that looked like output.
- **The viewport cost is unbounded** — sizing defaults to `auto`/`auto`
  (engine.rs:1950-1953), the image's inherent size. A phone screenshot pastes
  a render many rows tall; the rows above the cursor are pushed out of view
  by content no program printed. Every peer that renders images does so
  because a *program asked* (imgcat, icat, chafa) — user-chosen display, not
  an automatic consequence of copying pixels.
- **Scrollback semantics** — the render occupies scrollback that selection,
  search, and session-persistence treat as program output. A paste render is
  terminal UI dressed as output; the honest trace of the paste (the path) is
  already on the command line one row below it.

### 6. Status-bar indicators: the conditional chip with an action verb is an established shape

- **DropTerm's own band already contains the pattern three times**: the exit
  chip renders only after a failed command (docs/status-bar.md:22, "Success
  renders nothing — exit code 0 is the unmarked default"); the Scrolled chip
  appears only while scrolled back and its click performs the jump verb
  (`src/status_bar.rs:997`, docs/status-bar.md "the scrolled-back chip"); the
  `Copied` report is the band's transient confirmation of a completed
  clipboard action (`src/status_bar.rs:118`). A "N pasted" chip is the same
  genus: conditional, click-carries-a-verb, droppable under width pressure
  through `DROP_ORDER` (`src/status_bar.rs:210`).
- **VS Code** attaches per-terminal status to existing chrome: "Icons may
  appear to the right of the terminal title on the tab label when a
  terminal's status changes... Hover the icon to read status information,
  which may contain actions" (code.visualstudio.com terminal basics; the same
  citation anchors the status-bar-ux record §8).
- **iTerm2** lets status-bar components carry click actions (git state click
  offers Commit/Stash/Log/Push; cwd click opens a recent-directory picker) —
  the status-bar-ux record §2 documents this against iTerm2's own docs.
- The browser download-badge-opens-a-panel pattern (Chrome/Firefox) matches
  this shape too, but the Mozilla support page would not load on the fetch
  date, so that precedent rests on search-level description only (Unverified)
  — it is not load-bearing here.

## Design candidates, evaluated

**A. Status quo — path as text plus full inline render.** Grid integrity is
the best of the four mechanically (cells all the way down), and it is what
shipped. Rejected as the resting state on the evidence: it is the only
behavior in the surveyed field that auto-renders clipboard pastes (every peer
renders only program-requested images — Finding 1); the pixels evict to blank
cells while the file survives (Finding 5); the render displaces the viewport
by the image's own height (Finding 5); and it doubles the representation of
one paste (pixels in scrollback, path on the command line) where every
precedent keeps exactly one compact marker plus one file (Findings 2-3).

**B. Placeholder-only — a short marker, viewing elsewhere.** Grid-clean and
precedent-matched on the marker (Finding 3), but "elsewhere" is unspecified,
and for a terminal that means the user must find `~/.cache/dropterm/paste/`
in a file manager. The editor precedents survive path-only because the file
lands *inside the user's project* (VS Code's workspace copy); an XDG cache
dir with a 7-day sweep has no such visibility. Rejected without a panel.

**C. Status-bar chip + per-tab images panel; the grid keeps the path.**
Precedent: three independent list-panel models (iTerm2 Toolbelt, CopyQ,
Klipper — Finding 4) plus Warp/Claude Code's chip-keeps-grid-compact rule
(Finding 2). Grid integrity: the grid holds only text — every concern in
Finding 5 disappears while the mechanism's honesty (cells, not overlays) is
unaffected, since OSC/kitty rendering is untouched. Feasibility, seam by seam:
the ledger is a small per-tab append (path, stamp, dimensions) written by the
existing handler (`src/app.rs:3266-3272` already holds the bytes and the
returned path); thumbnails decode lazily from the cache PNGs with the `image`
crate and render through libcosmic's image primitives (the same handles the
grid placement pipeline uses — terminal-images.md "What DropTerm has today");
the panel surface has two proven seams (the settings drawer at app level, or
the completion menu's `with_layer` panel in `src/terminal/widget.rs:1146-1230`
— the Overlay-trait route being the named escalation for
mouse-interactive surfaces); the chip is a new `SegmentId` riding
`DROP_ORDER` with `click_jumps`-style verb routing (`src/status_bar.rs:997`),
hidden at zero count per the exit-chip rule, with the paste confirmation
riding the `Copied` report pattern (`:118`). Discoverability: the chip is the
count, the panel is the list, the path in the grid points at the file — three
routes to the same object.

**D. Hybrid — a bounded inline preview (a few rows) plus the panel.**
Technically cheap: `resolve_placement` already accepts cell-unit
width/height, so `place_pasted_image` could place a 3-row-fitting render
instead of `auto` sizing (engine.rs:1950). It preserves instant visual
confirmation and bounds the viewport cost but not the eviction-blank one, and
it still spends scrollback on non-output. No peer ships even this for pastes;
the nearest analog is xterm.js's image addon treating a *marker* as the honest
end state — its storage is bounded and "placeholder-on-eviction" replaces
pixels with markers (terminal-images.md §7). Deferred, not the default.

## Recommendation

**Adopt candidate C**, in this order of pieces:

1. **The grid keeps the bare path, unchanged.** OQ-5's ruling stands and
   gains field support: the path is the marker family's terminal member
   (VS Code inserts a path reference, pi displays its temp path — Finding 3),
   and it is the one marker a shell or an AI CLI can consume as-is, which is
   the module's stated purpose. No wrapper token: `[Image #1]` works for
   Claude Code because Claude Code is the reader of its own input; a shell is
   the reader of DropTerm's paste, and a token it cannot parse is noise.
2. **Remove the auto-render from the paste handler** (the
   `place_pasted_image` call at `src/app.rs:3271`; keep the engine entry for
   the deferred preview). This supersedes the working spec's premise that
   "the inline render is the visible confirmation" (SPEC-paste-image.md,
   Objective and OQ-5) — the visible confirmation moves to the band (piece 3).
   The terminal-images decision is untouched: program-driven OSC/kitty
   placements keep rendering into cells exactly as docs/images.md documents.
3. **A conditional band chip**: `N img` (or icon + count) when the tab has
   pasted images, absent at zero, click opens the panel, dropped
   lowest-priority under width pressure. Paste itself confirms transiently
   through the `Copied`-report mechanism — one chip, one pulse, no scrollback
   disturbance.
4. **A per-tab images panel**: a list (CopyQ's shape) of that tab's pasted
   images with small thumbnails decoded lazily from the cache files, each row
   carrying the actions the precedents imply — open (the `OpenHyperlink`
   precedent, `src/app.rs:3247-3251` via `open::that_detached`), copy path
   (`clipboard::write`), copy image back to the clipboard (a data-control
   `image/png` write through wl-clipboard-rs — the focus-free write path
   already shipped, and the remote-friendly complement to the SSH
   caveat: bytes travel where a local path cannot), re-paste the path, and
   remove (delete the cache file and the ledger row, bounded by PI-4's
   retention). Panel surface (drawer vs in-widget overlay) is a spec-phase
   choice between two proven seams; a mouse-interactive panel with thumbnails
   leans toward the app-level drawer, since the in-widget panel's route to
   mouse input is the heavier escalation.
5. **Ordering note**: this record lands before PI-3/PI-4, so `paste_image_enabled`
   (the config gate) and the retention sweep should ship governing the new
   shape — the gate's documented scope ("an image clipboard pastes nothing")
   is unaffected by where the render went.

**Rejected alternatives:**

- **A (status quo, full inline render)** — the only unprefedented behavior in
  the field (Finding 1); evicts to blank cells while the file survives;
  displaces the viewport by image height; doubles each paste's representation.
  The pieces that made it (read, file, path paste) are kept whole.
- **B (placeholder with no panel)** — a path into an XDG cache dir is not
  discoverable; every path-only precedent parks the file where the user works
  (a workspace), and DropTerm's cache is deliberately hidden. B without the
  panel trades a presentability problem for a findability hole.
- **D (bounded inline preview as the default)** — answers "did it land" but
  not "where is it and what can I do with it", still spends scrollback on
  non-output, still exposes the eviction-blank failure, and has zero peer
  precedent. Deferred with a named trigger: if hardware-pass users report
  missing the instant visual, `place_pasted_image` gains a bounded preview
  (cell-unit sizing already exists in `resolve_placement`) as an opt-in key,
  with xterm.js's placeholder-on-eviction as the honest-edges model.

**Relations to existing records:** builds on terminal-images (2026-11-05,
unchanged — this record rules only the paste-driven placement), the status-bar
records (conditional-chip vocabulary, `DROP_ORDER`, the action-verb precedent),
the shipped data-control write path (which the panel's copy-image action
reuses), and the shipped drawer seam. It
supersedes the *working spec's* render-confirmation premise only; records and
shipped OSC behavior are untouched.

## Unverified / open questions

- **iTerm2's current Toolbelt tool list**: `documentation-toolbelt.html` is
  404 on the fetch date and the Wayback Machine rate-limited; the "Recent
  Files / Downloads toolbelt tool" lead comes from search summaries and did
  not verify against iTerm2's own pages — what verified is Paste History and
  Recent Directories as Toolbelt tools (3.0 menu-items docs) and Downloads as
  a menu-bar item (shell-integration docs). The 3.6/3.7 Toolbelt may carry
  more tools than the 3.0 docs show.
- **Warp's exact chip visuals** (thumbnail vs filename-only, click behavior):
  Warp's docs page documents paste-as-context but not the editor's chip
  rendering; the chip description rests on issue titles and community posts.
- **Klipper image thumbnails**: the handbook PDF reportedly shows previews in
  the history, but the PDF was not readable on the fetch date and the HTML
  handbook documents no thumbnails.
- **Konsole's "by design" stance**: from a KDE Brainstorm post surfaced by
  search; the underlying issue (#6331, Dec 2025) was not fetched directly.
- **WezTerm #7272's closure rationale**: the fetch showed the issue closed
  with labels (`enhancement`, `needs:triage`) but rendered no comments.
- **Firefox/Chrome download-badge pattern**: the Mozilla support page failed
  to load on the fetch date; the pattern is asserted at search level only and
  is not load-bearing for the verdict.
- Whether PI-4's retention sweep should extend to the ledger (a panel row
  whose file aged out must disappear, not render broken) — a spec-phase
  question the record flags but does not rule.

## Sources

- DropTerm local (branch `ced`, HEAD 2026-11-08): `src/clipboard.rs:88`,
  `:93-98`, `:171-180`, `:293-343`, `:347-406`; `src/app.rs:3257-3273`,
  `:3247-3251`; `src/app/tab_actions.rs:352-359`, `:1401-1404`;
  `src/terminal/engine.rs:720`, `:1936-1974`, `:4122`; `src/status_bar.rs:118`,
  `:133`, `:175-220`, `:997`, `:1004`; `src/terminal/widget.rs:1146-1230`;
  `docs/images.md:10`, `:45-51`; `docs/status-bar.md:9-22`;
  `SPEC-paste-image.md` (Objective, design items 4-5, OQ-2, OQ-5);
  `tasks/paste-image.md` (PI-1 done `cb39a39`, PI-2 done `d9b8151`,
  checkpoint state 2026-11-08); sibling records `terminal-images.md`,
  `status-bar-ux.md`, `status-bar-capability.md`
- WezTerm: wezterm/wezterm issue #7272 "Pasting Images" (opened 2025-09-30,
  closed; fetched 2026-11-08); PasteFrom documentation —
  <https://wezterm.org/config/lua/keyassignment/PasteFrom.html> (fetched
  2026-11-08)
- kitty: clipboard kitten documentation —
  <https://sw.kovidgoyal.net/kitty/kittens/clipboard/> ("It even works over
  SSH"; `kitten clipboard -g picture.png`; no interactive viewer) (fetched
  2026-11-08)
- Ghostty: ghostty-org/ghostty discussion #10478 "[Feature Request] Support
  pasting images captured directly to clipboard" (opened 2026-01-28;
  maintainer quotes 2026-02-18 and 2026-03-10 and 2026-03-16; root-cause and
  PR #11571 comment 2026-03-16; Claude Code/pi chip comments 2026-02-28;
  fetched 2026-11-08)
- Windows Terminal: microsoft/terminal discussion #19397 "Is it possible to
  implement a feature that can 'Paste Image'" (opened 2025-09-30; j4james
  comment 2025-10-11; fetched 2026-11-08)
- Warp: docs.warp.dev changelog 2026 (entries 2026.07.23 and 2026.09.23,
  fetched 2026-11-08); "Images as Context" —
  <https://docs.warp.dev/agents/local-agents/agent-context/images-as-context>
  (updated 2026-10-26, fetched 2026-11-08); warpdotdev/warp issues #6648
  (2025-06, image chips request) and the Aug 2025 drag-drop behavior-change
  issue (titles/summaries via search, fetched 2026-11-08)
- Claude Code: interactive-mode documentation —
  <https://code.claude.com/docs/en/interactive-mode> ("Ctrl+V or Cmd+V
  (iTerm2) or Alt+V (Windows and WSL) — Paste image from clipboard — Inserts
  an `[Image #N]` chip at the cursor") (fetched 2026-11-08); terminal-config
  documentation (text-paste placeholder `[Pasted text #1 +120 lines]`; no
  image-paste coverage) (fetched 2026-11-08)
- VS Code: v1.79 release notes —
  <https://code.visualstudio.com/updates/v1_79> ("automatically copy the file
  into your workspace and insert a link"; `markdown.copyFiles.destination`;
  image-1.png duplicates) (fetched 2026-11-08)
- JetBrains: holgerbrandl/pasteimages README —
  <https://raw.githubusercontent.com/holgerbrandl/pasteimages/master/README.md>
  ("deposited in a configurable directory relative to the markdown... insert
  the markdown image element") (fetched 2026-11-08); IntelliJ Markdown docs
  (Insert Image dialog; no built-in clipboard paste) via search,
  fetched 2026-11-08
- iTerm2: 3.0 menu-items documentation —
  <https://iterm2.com/3.0/documentation-menu-items.html> (Paste History tool
  "shows recently pasted strings"; Open Paste History "up to the last 20
  values"; Recent Directories frecency list) (fetched 2026-11-08);
  shell-integration documentation —
  <https://iterm2.com/documentation-shell-integration.html> (Downloads menu
  item "lets you view downloaded files and track their progress"; Recent
  Directories click verbs) (fetched 2026-11-08); 2.1 menu-items documentation
  (Open Paste History window, subsequence filter, click-to-paste; Paste
  Special/Paste Slowly) (fetched 2026-11-08)
- CopyQ: hluk/CopyQ README ("Copying text or image to the clipboard will
  create a new item in the list...") (fetched 2026-11-08)
- KDE Klipper: handbook, "Using Klipper" —
  <https://docs.kde.org/trunk_kf6/en/plasma-workspace/klipper/using-klipper.html>
  (search field; "four icons... invoke an action, show the barcode, edit the
  content or remove the entry") (fetched 2026-11-08)
- xterm.js: xtermjs/xterm.js issue #1411 (browser paste event delivers text
  only) and the image addon's storage/placeholder model (via
  terminal-images.md §7, fetched 2026-11-08)
- Alacritty, rio, Hyper, Konsole: no clipboard-image-paste feature exists in
  any of their trackers or docs (searches run 2026-11-08); Konsole issue and
  KDE Brainstorm post surfaced by search, not fetched directly (see
  Unverified)
