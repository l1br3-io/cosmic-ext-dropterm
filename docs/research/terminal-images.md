# Research: how modern terminals display images inline — and what should DropTerm adopt?

Date: 2026-11-05
Question: *iTerm2's OSC 1337 inline images, kitty's APC graphics protocol, and
Sixel's DCS bitmaps are the three ways a program puts pixels in a terminal —
which of them should DropTerm implement, through which seams, and with which
grid-integration model?*

## TL;DR

**Implement the iTerm2 inline-images protocol first, as image cells living in
the scrollback grid, riding the exact OSC fork seam that already carries
OSC 133-D / 7 / 9;4 — then add the query-capable subset of the kitty graphics
protocol (the one new parser surface the fork needs: vte 0.15 drops APC
payload bytes on the floor today); defer Sixel.** The iTerm2 protocol is the
only one of the three that arrives as OSC — bytes vte already delivers to
DropTerm's forks — and it is the lingua franca of emitters: `imgcat` emits it
unconditionally with no detection, WezTerm/Konsole/mintty/rio/VS Code/xterm.js
all accept it, and `timg` prefers it even on WezTerm and Konsole. The
grid-integration model with the strongest Rust precedent is WezTerm's: every
image (all three protocols) is sliced into one `ImageCell` per covered cell,
stored inside the cell's attributes — precisely the `CellExtra` seam
(`zerowidth`/`underline_color`/`hyperlink`) that alacritty_terminal 0.25.1
already carries and that DropTerm's fork can extend. Kitty's protocol then
reuses the same cells for its cursor-anchored placements. Sixel buys
xterm/foot/mlterm-class emitters only, costs a DCS palette+RLE decoder plus
the ansi-adapter hook/put/unhook forwarding the fork would have to add, and
its DA1 `;4` detection is the one signal DropTerm could answer "no" to
honestly for years — every surveyed terminal that ships images ships either
iTerm2 or kitty first.

## What DropTerm has today

- **No image support at all.** Grepping `src/terminal/widget.rs` (3,480 lines)
  and `src/terminal/engine.rs` (3,428 lines) for image/sixel/imgcat/1337:
  zero hits. The word "image" appears in widget.rs only in comments about
  "cell indices … exact in f32" precision clamps
  (`src/terminal/widget.rs:616`, `:1021`, `:1109`).
- **The fork seam is established and three arms deep.** `[patch.crates-io]`
  pins `vte` at rev `7cd0e4bdf164f56712e44cd5019b98d494e1d59d` and
  `alacritty_terminal` at rev `c9eb7de00f5f88431526357528ed171f59ebdc41`
  (both branch `dropterm-osc133d`; `Cargo.toml:47-49`). The alacritty fork's
  `Event` enum carries the three passthrough arms `PromptExit(u32)`,
  `CurrentDirectory(String)`, `ProgressReport(u8, Option<u8>)`
  (fork `alacritty_terminal/src/event.rs:52-64`), forwarded from `Term`
  (`fork src/term/mod.rs:1445-1460`), delivered to the app through
  `TaggedEventProxy` (`src/terminal/engine.rs:298-308`).
- **How vte 0.15.0 routes the three sequence classes** (read from the fork
  checkout at `7cd0e4b`, which is upstream 0.15.0 plus the OSC arms):
  - **OSC — already delivered.** The state machine calls
    `performer.osc_dispatch(params, bell_terminated)` (`src/lib.rs:587`),
    and the `ansi` feature's adapter forwards to the `Handler` trait
    (`src/ansi.rs:1342`) where the three fork arms live. An iTerm2 1337 arm
    needs zero parser changes.
  - **DCS — parsed, then dropped at the ansi adapter.** The state machine
    fully implements `hook`/`put`/`unhook` (`src/lib.rs:471` hook with
    params/intermediates, `:319` per-byte `put`, `:321-332` `unhook` on
    ST/CAN/SUB/ESC; `Perform` trait defaults at `:774-792`). But the
    `Performer` adapter in `ansi.rs` logs-and-drops all three:
    `debug!("[unhandled hook] …")` (`src/ansi.rs:1324-1340`). Sixel needs
    forwarding added there plus `Handler` methods — parser itself is done.
  - **APC — never delivered.** `ESC _` (0x5F) enters
    `State::SosPmApcString` (`src/lib.rs:377`), and that state's only
    action is `self.anywhere(performer, byte)` (`src/lib.rs:182`) — the
    payload bytes go nowhere; there is no `Perform` method for APC strings.
    Kitty graphics needs a new dispatch surface in the state machine plus a
    new `Perform`→`Handler` method. This is the largest single fork delta of
    the three protocols.
- **The grid already has the cell-extension precedent.** alacritty_terminal's
  `Cell` is `{c, fg, bg, flags, extra: Option<Arc<CellExtra>>}` with
  `CellExtra { zerowidth: Vec<char>, underline_color: Option<Color>,
  hyperlink: Option<Hyperlink> }` (fork `src/term/cell.rs:123-141`) — rare
  per-cell payload rides `Arc<CellExtra>` behind an `Option`, exactly the
  shape WezTerm uses for image slices (§6).
- **The renderer already has the primitive.** At DropTerm's pinned libcosmic
  `08a09e3dfa1f729d52585fcedd6e731130235363`, the wgpu `Renderer` implements
  `draw_image(image, bounds, clip_bounds)` (`iced/wgpu/src/lib.rs:863-871`)
  and the geometry `Frame` has `draw_image(bounds, image)`
  (`iced/graphics/src/geometry/frame.rs:98`). The terminal widget's `draw()`
  already positions everything in cell units
  (`src/terminal/widget.rs:632-747` backgrounds via `renderer.fill_quad`,
  glyphs via cosmic-text layout runs) — a covered-cell image is one more
  draw call at known cell coordinates. No GPU texture protocol work.

## Findings

### 1. The iTerm2 inline-images protocol (OSC 1337 File)

Primary sources: [iTerm2 images
docs](https://iterm2.com/documentation-images.html), the shipped
[`imgcat`](https://iterm2.com/utilities/imgcat) script, and iTerm2 source at
master `40a75e3b32bbd697802135d13dc3cb78d2c65c1a` (fetched 2026-11-05).

- **Wire form:** `ESC ] 1337 ; File = [args] : base64 payload ^G` (BEL or
  ST both accepted; no spaces). Chunked variant for tmux (iTerm2 3.5+):
  `1337;MultipartFile=<args>`, then `1337;FilePart=<base64>` chunks, then
  `1337;FileEnd`. Documented caps: older tmux truncates at 256 bytes;
  newer tmux and iTerm2 cap sequences at 1,048,576 bytes.
- **Arguments** (docs table): `name` (base64 filename, defaults "Unnamed
  file"), `size` (byte count, "only used by the progress indicator"),
  `width`/`height` ("N character cells, Npx pixels, N% of the session's
  size, or `auto`" — the image's inherent size), `preserveAspectRatio`
  (default 1; 0 = stretch to fill), `inline` (default **0** — without
  `inline=1` the file is *downloaded*, not displayed).
- **iTerm2's parsed defaults from source** (`sources/VT100/VT100Terminal.m`):
  `size=0, width="auto", height="auto", preserveAspectRatio=YES,
  inline=NO` (L4178-4182); `name` base64-decoded at L4222. **No
  cursor-movement argument exists in iTerm2** — the image is written into
  the grid at the cursor and the cursor lands below it
  (`sources/InlineImages/VT100InlineImageHelper.m` L244-293: rows are
  appended at the cursor, `setCharsFrom` fills cells, L553). WezTerm's
  `doNotMoveCursor=1` is a WezTerm-side extension to the protocol
  ([wezterm docs/imgcat.md](https://wezterm.org/imgcat.html), "WezTerm
  supports an extension to the protocol"), parsed in
  `wezterm-escape-parser/src/osc.rs:966,1044-1046`.
- **What imgcat actually emits** (verified by PTY capture of the script
  fetched from iterm2.com/utilities/imgcat on 2026-11-05; REA evidence
  `ev_a86cb038` chunked, `ev_7d39342` legacy): **the chunked mode is the
  default** — `1337;MultipartFile=inline=1;size=<decoded byte
  count>;name=<b64 of the absolute path>` BEL first (no payload), then
  `1337;FilePart=<200-char b64 chunk>` BEL for each `fold -w 200` part,
  then `1337;FileEnd` BEL and a newline. The single-sequence form —
  `1337;File=inline=1;size=…;name=…:<base64>` BEL — is the `-l/--legacy`
  opt-in (`imgcat -l`, the script's own flag; capture shows the whole
  image in one OSC). Optional args in both: `[;width=…][;height=…]
  [;preserveAspectRatio=…][;type=<mime/.ext>]` — `type` (mime/extension
  hint) is not on iTerm2's docs page. Under `TERM=screen*|tmux*` every
  OSC is wrapped in the tmux passthrough DCS
  (`\033Ptmux;\033\033]…\a\033\\`). A receiving terminal that wants
  imgcat output on day one must therefore parse **both** the legacy
  `File=` form and the `MultipartFile`/`FilePart`/`FileEnd` session —
  each `FilePart` is an independent OSC arrival that must be
  accumulated, not a payload inside one dispatch.
- **iTerm2 also implements the kitty protocol** — kitty's spec credits
  iTerm2 commit `4fe5b2173193b6c3e45234b6b2ab7a144a5cfa01`, and
  `sources/InlineImages/KittyImageCommand.swift` / `KittyImageController.swift`
  are in the tree. iTerm2 additionally grew **Sixel in the 3.7.0 beta line
  (2026)** — `sources/VT100/VT100SixelParser.m` exists and
  `docs/notes-3.7.0beta6.txt` L45-47 fixes a CVE in "the bundled libsixel".

### 2. The kitty graphics protocol (APC G)

Primary source: `docs/graphics-protocol.rst` in kovidgoyal/kitty @ master
(last commit touching it `3dc4f8d5aa0092b77c839145aff5602ca8664cd4`,
2026-10-29; master head `3e4ffc109891`, fetched 2026-11-05).

- **Wire:** `<ESC>_G<control data>;<payload><ESC>\` — an APC string;
  control data is comma-separated `key=value`; payload is base64. The spec's
  stated reason for APC: "Most terminal emulators ignore APC codes, making
  it safe to use."
- **Transmission:** `f` = format `8, 16, 24, 32, 100` (default 32 = RGBA;
  100 = PNG with dimensions read from the file; 8/16 indexed added in kitty
  0.50); `t` = medium `d` direct in-band (default), `f` regular file, `t`
  temp file (path must contain `tty-graphics-protocol` in a known temp
  dir; terminal deletes it), `s` POSIX shared-memory object (terminal must
  read then unlink). `S`/`O` = size/offset for partial reads. `o=z` zlib
  compression.
- **Chunking:** base64 split into chunks of ≤4096 bytes (all but last a
  multiple of 4); `m=1` on every chunk but the last; only the first chunk
  carries full control data.
- **Actions and responses:** `a=t` transmit, `a=T` transmit+display,
  `a=p` place a previously transmitted image at the cursor, `a=q` query
  (test-load; replies `<ESC>_Gi=31;OK<ESC>\` or an error string; with
  `e=<action>` (kitty 0.50+) the reply echoes every supported non-default
  key — the feature-negotiation query). **Documented support detection:**
  send the query followed by `ESC[c`; "a DA1 reply without a graphics reply
  means no support". `q=1`/`q=2` suppress OK/failure replies. `i` = image
  id, `I` = terminal-assigned number, `p` = placement id; `U=1` = unicode
  placeholder mode (print U+10EEEE with the image id in the foreground
  color + combining diacritics encoding row/col — images that survive
  tmux/vim redraws as plain text); `z` = z-index (negative renders under
  text); relative placements `P`/`Q`/`H`/`V` (kitty 0.31+).
- **Deletion:** `a=d` with `d=` one of `a/A i/I n/N c/C f/F p/P q/Q r/R
  x/X y/Y z/Z` (lowercase removes placements, uppercase frees data too).
- **Scrolling/selection/resize:** "images must be scrolled along with text"
  and clipped at page margins; terminal reset, alt-screen switch, and
  `<ESC>[2J` clear visible images; **other** text-erase operations "must
  have no effect on graphics"; moving/resizing is done by re-placing the
  same image+placement id pair "without flicker". (The image data itself is
  referenced by cells — a placement occupies cells; scrolling is tracked
  per cell movement.)
- **Limits and security:** terminals "should have a maximum storage quota
  for image data… For example the quota in kitty is 320MB per buffer" with
  animation frames on disk at 5×. The security section is explicit that
  `t=f`/`t=t`/`t=s` paths come "from potentially untrusted sources":
  regular files only (no devices/pipes), symlinks followed safely,
  sensitive paths (/proc, /sys, /dev) may be refused, and every read
  failure must return "a single, identical, error response" (kitty uses
  `EBADF:Failed to read image file`) so untrusted programs cannot probe
  the filesystem through error differences.
- **The spec's own implementors list** (verbatim list, "Other terminals
  that have implemented the graphics protocol"): AbsoluteTelnet/SSH,
  **Ghostty, Konsole**, Mobile SSH, st (patched), SSH Client for Android,
  **Warp**, wayst, **WezTerm, iTerm2, xterm.js**. Consumer tools it names:
  mpv, neovim, ranger, timg, viu, Yazi, termpdf.py, chafa.

### 3. Sixel (DCS q)

Primary sources: xterm's ctlseqs
(<https://invisible-island.net/xterm/ctlseqs/ctlseqs.html>, fetched
2026-11-05) and parser sources (WezTerm, mintty, Windows Terminal); the
classic vt100.net mirror was unreachable (404) on the fetch date.

- **Wire:** `DCS Pa ; Pb ; Ph q Ps..Ps ST` — per xterm's Sixel Graphics
  section: Pa = pixel aspect ratio, Pb = background color option, Ph =
  horizontal grid size (ignored). Inside: `#` color introducer
  (`#n` select register n; `#n;2;r;g;b` define RGB), `" pan;pad;ph;pw`
  raster attributes, sixel characters `?`–`~` each encoding **6 vertical
  pixels** (bit 0 = top row), `!n` repeat, `$` carriage-return within the
  band, `-` newline to the next 6-row band (encoding verified in WezTerm's
  parser: `for bitno in 0..6 { … (d & (1 << bitno)) != 0 … }`,
  `term/src/terminalstate/sixel.rs:46-56`, and mintty's
  `src/sixel.c`).
- **Capability reporting:** DA1 (Primary Device Attributes) — xterm answers
  with "Ps = 4 ⇒ Sixel graphics" when configured for graphics
  (ctlseqs L795, L3225-3228). DECSDM (`CSI ?80 h`, "Sixel Display Mode")
  toggles inline-vs-fullscreen placement; xterm private mode 8452 leaves
  the cursor to the right of the graphic.
- **Detection flow used by emitters:** send `CSI c`, look for `;4` in the
  `CSI ?…c` response (timg implements exactly this, with a comment citing
  vt100.net DA1; `src/term-query.cc:326-344`). `XTSMGRAPHICS`
  (`CSI ? Pi ; Ps ; Pv S`) additionally queries/sets sixel geometry
  (register count, max pixel geometry).
- **Where it is used today:** the hardware-terminal legacy channel — xterm
  (compiled/configured for graphics), foot, mlterm, RLogin, Windows
  Terminal (SixelParser at `src/terminal/adapter/SixelParser.hpp`; the
  1.22 line's release notes carry sixel scaling/copy fixes —
  microsoft/terminal v1.22.11751.0 notes, fetched 2026-11-05), VTE
  (build-time opt-in: meson option `sixel` `value: false`), contour
  (`src/vtbackend/graphics/SixelParser.cpp`; `docs/features.md` "Sixel
  Image support"), Konsole, WezTerm, mintty, rio, iTerm2 (3.7 betas,
  bundled libsixel), VS Code/xterm.js. Driven mostly by older
  scientific/plotting tools (gnuplot `set term sixel`, lsix, img2sixel).

### 4. The adoption matrix (each cell from that project's own docs/source)

| Terminal | iTerm2 1337 | kitty APC | Sixel DCS | Source for the row |
|---|---|---|---|---|
| kitty | — | ✓ (author) | — | graphics-protocol.rst; no sixel parser anywhere in the repo (grep, master `3e4ffc10`) |
| WezTerm | ✓ | ✓ | ✓ | docs/imgcat.md; `term/src/terminalstate/{iterm,kitty,sixel}.rs` |
| iTerm2 | ✓ (author) | ✓ | ✓ (3.7 beta line, 2026) | VT100Terminal.m L4178+; KittyImageCommand.swift; VT100SixelParser.m |
| Konsole | ✓ | ✓ | ✓ | `src/Vt102Emulation.cpp` L643 (`1337;File=:`), L2411 `processGraphicsToken`, L693 `processSixel`; `src/Screen.h` L38-49 `TerminalGraphicsPlacement_t` with `enum source { Sixel, iTerm, Kitty }` |
| foot | — | — | ✓ | README "Sixel image support" (codeberg dnkl/foot master) |
| ghostty | — | ✓ | — | `include/ghostty/vt/kitty_graphics.h`; no sixel parser in tree; DA1 tests answer `?62;22c` without `;4` (`src/terminal/device_attributes.zig:178-185`) |
| mintty | ✓ | — | ✓ | `src/termout.c:5091` `when 1337: { // iTerm2 image protocol`; `src/sixel.c` |
| xterm | — | — | ✓ (opt-in via decTerminalID/decGraphicsID) | ctlseqs Sixel Graphics section |
| VS Code terminal | ✓ | ✓ (partial) | ✓ | xterm.js `addons/addon-image` README ("Supports SIXEL, iTerm's inline image protocol (IIP) and partially Kitty's"); wired behind `terminal.integrated.enableImages` **and** the WebGL renderer (`xtermTerminal.ts:1046-1050`, vscode main `ae231882`) |
| rio | ✓ | ✓ | ✓ | `rio-vt/src/ansi/{iterm2_image_protocol,kitty_graphics_protocol,sixel}.rs` + `graphics.rs` |
| contour | ? | ? | ✓ | `docs/features.md` "Sixel Image support" (iTerm2/kitty cells unverified) |
| GNOME Terminal / VTE | — | — | build-time opt-in, upstream default **off** | `src/image.hh` (saitoha/hpj attribution), `vte.cc:3664` `#if WITH_SIXEL`; `meson.options` `sixel: value: false` |
| Ptyxis | — | — | (follows the system VTE build) | no image code in its own repo (search) |
| **cosmic-term** (closest peer) | — | — | — | no sixel/image/kitty code anywhere in pop-os/cosmic-term @ `8a05e9194881` (tree grep + code search, 2026-11-05) |
| Alacritty | — | — | — | issue #910 "Add support for libsixel" **open since 2017-11-21** (238 comments, last activity 2026-10-06); PR #4763 "Support for graphics in the terminal" open and described by regulars as "most likely never be merged"; maintainer chrisduerr (2026-07-07): "The way this 'should' be implemented is **not**." Note: alacritty/alacritty **#3037 is not the sixel issue** — it is a closed OSC52/tmux set-clipboard bug |
| tmux | no image support of its own — passes bytes through `DCS tmux; … ST` behind `allow-passthrough [on|off|all]` (tmux(1), man7.org, fetched 2026-11-05); timg auto-runs `tmux set -p allow-passthrough on` (needs tmux ≥ 3.3) |

Two matrix facts that shape the verdict: **iTerm2+kitty is the modern pair**
(every 2024+ terminal above ships at least one, and VS Code/rio/Konsole ship
all three), and **the closest peers ship none** — cosmic-term has no image
code, and VTE keeps sixel behind an off-by-default build flag.

### 5. How emitters pick a protocol (this is what buys working software)

- **`imgcat`** (iTerm2's own): emits OSC 1337 **unconditionally — no
  detection at all**. Any terminal that implements 1337 renders it.
- **`kitten icat`** (kitty @ master, `kittens/icat/main.go` +
  `detect.go`): **active probe only** — writes test payloads to shared
  memory and a temp file, sends `a=q` queries with unique ids, reads the
  replies (`detect.go:20-44` `DetectSupport`); env vars are not consulted.
  Errors otherwise: "This terminal does not support the graphics protocol
  use a terminal such as kitty, WezTerm or Konsole that does" (main.go:292).
  Under tmux it assumes unsupported (no query replies through tmux).
- **`timg`** (hzeller/timg @ `a40148448f2b`, `src/term-query.cc:226-344`):
  (1) env guesses — `TERM=xterm-kitty|xterm-ghostty` → kitty,
  `TERM_PROGRAM=vscode|WarpTerminal` → iTerm2; (2) **XTVERSION query**
  `CSI >q` fenced by `CSI 5n` — matches "iTerm2" or "Konsole 2" → iTerm2
  protocol, "WezTerm" → **iTerm2**, "kitty"/"ghostty" → kitty,
  "foot"/"mlterm" → sixel, "tmux" → in-tmux; (3) DA1 `;4` → sixel.
  The comment is explicit that the kitty `a=q` query is *avoided* because
  it "messes up some other terminals". Default when nothing answers:
  quarter blocks.
- **`viu`/`viuer`** (atanunq/viuer @ `ed329e5c01d0`): iTerm check is
  env-only — `TERM_PROGRAM` contains iTerm/WezTerm/mintty/rio/WarpTerminal,
  or `LC_TERMINAL` contains iTerm (`src/printer/iterm.rs:94-106`); kitty
  check is an **active probe** (`check_kitty_support` → `a=q` baseline +
  temp-file local test, `src/printer/kitty.rs:80-95`); Sixel is
  experimental behind feature flags; fallback half blocks.
- **`chafa`** (hpjansson/chafa @ `c897fce60cc4`): has kitty, sixel and
  iTerm backends (`chafa/internal/chafa-kitty-renderer.c` etc.); its NEWS
  documents DA1-based detection supplementing env heuristics, foot+tmux
  detection fixes, and enabling kitty for Warp.

Consequences for DropTerm: **the iTerm2 protocol is detected through
channels a new terminal can actually join** (TERM_PROGRAM string, XTVERSION
answer, or no detection at all with imgcat), while the kitty protocol is
detected by a live query only DropTerm itself can answer. Shipping 1337
first means `imgcat`, `wezterm imgcat`, forced-mode timg/viu/chafa all work
on day one; shipping kitty means `kitten icat` works the day the `a=q`
round-trip lands.

### 6. Rust prior art: WezTerm's one-model-for-three-protocols design

wez/wezterm @ main `e68a8ec27727fbbda15e07f0a0737cd5e16481f6` (2026-10-26;
file/line citations from that tree):

- **Every image becomes grid cells.**
  `wezterm-cell/src/lib.rs:98` — `CellAttributes` holds
  `image: Vec<Box<ImageCell>>` (attached via `attach_image`, L396). The
  doc comment on `ImageCell` (`wezterm-cell/src/image.rs:88-115`): "Since
  an Image can span multiple cells, we need to logically slice up the
  image and track each slice of it… This models the Kitty graphics
  protocol, where an image placement can span several cells: the placement
  is sliced into one `ImageCell` per covered cell". Each `ImageCell`
  carries floating texture coordinates into the shared `Arc<ImageData>`
  (`top_left`/`bottom_right`, (0,0)–(1,1)), `z_index`, cell padding, and
  for kitty the `image_id`/`placement_id`.
- **One normalization point.** All three protocols funnel into
  `TerminalState::assign_image_to_cells`
  (`term/src/terminalstate/image.rs:65`) with an `ImageAttachStyle`
  (Iterm/Kitty): it divides the pixel rect by the cell size into full
  cells plus remainder pixels, and writes the slice cells. It contains
  honest guards from real crashes: refuse to place when per-cell pixel
  dimensions are zero (headless/tmux domains, wezterm issue #6344) and
  when the drawable region is zero.
- **The three front doors.** OSC 1337 → `set_image`
  (`terminalstate/iterm.rs:11-153`): decodes, enforces
  `check_image_dimensions` (RAM ceiling, image.rs:302-308), resolves
  width/height units incl. `auto` and aspect preservation, downscales
  with CatmullRom but keeps `EncodedFile` passthrough for
  animation-capable formats, honors WezTerm's `do_not_move_cursor`.
  Kitty APC → `KittyImageState` (`terminalstate/kitty.rs:18-60`):
  `id_to_data`/`number_to_id` maps, `placements: HashMap<(image_id,
  placement_id), PlacementInfo>`, `used_memory` with a
  **320 MiB budget** (`let budget = 320 * 1024 * 1024;`) and
  LRU-style `prune_unreferenced` — mirroring kitty's own documented
  quota. Sixel DCS → `sixel()` (`terminalstate/sixel.rs:13-60`): builds an
  `RgbaImage` from the parsed `SixelData`, honoring the private-color-
  registers mode. Routing: their own parser produces `Action::Sixel` /
  kitty `Action` / `ITermProprietary::File`, dispatched in
  `terminalstate/performer.rs:279-284, 832`.
- **The other Rust precedent: rio.** raphamorim/rio @ main
  `0d9ede193ace8a81c0b7976bedf0ebb4cd6da718` (2026-11-04) implements all
  three protocols **inside a fork of vte itself** (`rio-vt`):
  `rio-vt/src/ansi/{sixel,kitty_graphics_protocol,
  iterm2_image_protocol}.rs` — the exact route DropTerm's forks would take.
  Its `graphics.rs` header states it "was retired from a alacritty PR made
  by ayosec" (alacritty PR #4763, Apache-2.0) — i.e. the community's
  unmerged alacritty image work lives on in Rio. Rio's model is hybrid:
  sixel/iTerm2 become atlas-backed **grid graphics**; kitty images are
  kept as **overlay placements** (`struct KittyPlacement`) with a separate
  texture pipeline, plus the U+10EEEE placeholder constant.
- **The overlay counter-model.** kitty itself, Ghostty (per-screen image
  storage + placements, its C-API docs: images placed "At a cell, so the
  image scrolls with the text" or via unicode placeholders), and Konsole
  (`TerminalGraphicsPlacement_t` — a `QPixmap` with col/row/cols/rows and
  a `scrolling` flag, one struct fed by all three protocols) keep images
  **outside** the cell grid and re-anchor them on scroll. Konsole's
  `enum source { Sixel, iTerm, Kitty }` (src/Screen.h:48) shows the same
  three-to-one normalization, different storage.

### 7. Size limits and security posture across implementors

| Implementation | Documented/measured ceilings |
|---|---|
| kitty | 320 MB image data per buffer; animation frames on disk at 5×; temp-file path must contain `tty-graphics-protocol`; identical `EBADF` error for all read failures (anti-probing) |
| WezTerm | 320 MiB budget in `KittyImageState::prune_unreferenced` (kitty.rs:33); `check_image_dimensions` rejects images whose "required RAM > max allowed" |
| xterm.js addon-image (defaults) | `pixelLimit` 16,777,216 px per image; `sixelSizeLimit`/`iipSizeLimit`/`kittySizeLimit` 33,554,432 bytes per sequence; `storageLimit` 128 MB FIFO with placeholder-on-eviction |
| iTerm2 | sequence cap 1 MiB (documented); no published pixel quota found |
| imgcat chunk mode | 200-byte chunks (its own choice, tmux-safe) |

Security gradient (kitty spec, its own words + matrix reality): **in-band
base64 over the PTY** (iTerm2; kitty `t=d`) costs only PTY bandwidth and
parser memory; **file paths and shared memory** (`t=f/t/t=s`) make the
terminal a filesystem/shm reader on behalf of possibly-untrusted programs —
hence kitty's mandated refusals and uniform error. DropTerm can ship kitty
`t=d`-only and answer `a=q,e=T` honestly with only `t=d` supported;
`kitten icat`'s `DetectSupport` then picks direct mode.

## Recommendation

**Ship in this order:**

1. **Phase 1 — iTerm2 inline images (OSC 1337 File) through the existing
   OSC fork arm.** Parser cost is zero (vte already delivers OSC to the
   fork's `Handler::osc_dispatch`); the arm follows the PromptExit/
   CurrentDirectory/ProgressReport pattern exactly. Decode base64, decode
   PNG/JPEG/GIF/WebP (the `image` crate, as WezTerm does), resolve the
   documented unit grammar (`N`/`Npx`/`N%`/`auto`,
   `preserveAspectRatio`, `inline`), enforce ceilings (WezTerm-style
   dimension check + a per-image byte cap at the documented 1 MiB sequence
   size and a WezTerm/xterm.js-style total store, evicting oldest).
2. **Store images as cells in the scrollback grid** — WezTerm's model,
   through alacritty_terminal's own `CellExtra` precedent: the fork grows
   an image field on `CellExtra` (like `hyperlink`), the placement is
   sliced per covered cell, and the cursor advances below the image. This
   makes scrollback, selection-exclusion, reflow, alt-screen clearing, and
   search "just work" the way text does — every protocol normalizes into
   the same cells, and the widget draws each covered cell with the
   `renderer.draw_image` primitive that already exists at the pinned
   libcosmic (`iced/wgpu/src/lib.rs:863`). Konsole's placement enum and
   rio's split prove the overlay model is viable too, but for DropTerm the
   cell model has strictly less new state to keep coherent with
   alacritty_terminal's grid.
3. **Phase 2 — the kitty graphics subset through a new APC arm in the
   vte fork.** The fork's one genuinely new parser surface: accumulate
   `SosPmApcString` payload and dispatch it. Implement the subset the
   tooling actually uses and kitty's own clients negotiate: `f=32`/`f=100`
   PNG, `t=d` only, `m=1` chunking, `a=t/T/p/q/d`, `i`/`p` ids, `c`/`r`
   sizing, `z`, and the `a=q`/`e=T` reply (plus the DA1 fence). Reject
   `t=f/t/s` with the spec's uniform error — no filesystem or shm reads,
   the security posture iTerm2-mode already has. The 320 MB-class budget
   from Phase 1 is reused; placements render through the same cells.
   Unicode placeholders (`U=1`) come later if tmux/vim users ask.
4. **Phase 3 — Sixel, deferred with named triggers.** Cost: forward
   hook/put/unhook through the ansi adapter (`src/ansi.rs:1324-1340`) into
   new `Handler` methods, plus a palette/RLE decoder — none of it reusable
   with Phases 1–2 except the final cell write. Trigger it only when a
   DropTerm user base shows up with sixel-only tools (gnuplot sixel,
   lsix) that matter to them; until then DropTerm's DA1 simply does not
   report `;4`, which every emitter treats as "not sixel, try something
   else". iTerm2's 3.7 and Windows Terminal's 1.22 show the legacy channel
   still moves, but neither is the first protocol a modern terminal ships.
5. **Be findable.** Export a `TERM_PROGRAM` value of DropTerm's own, answer
   the `CSI >q` XTVERSION query with it, and answer the OSC 1337
   `Capabilities` feature-report query (mintty's
   `termout.c:4753-4759` shows the reply shape). That is how timg/viuer's
   string-matching detection lists grow — the same road ghostty
   (`TERM=xterm-ghostty` is special-cased in timg) and Warp
   (`TERM_PROGRAM=WarpTerminal`) took. Do **not** spoof another terminal's
   `TERM_PROGRAM`.

**Rejected, with reasons:**

- **Sixel first** — the DCS+palette decoder is the most protocol-specific
  work with the least modern-emitter pull: the flagship emitters of the
  field (kitty, Ghostty) don't implement it, and every current emitter
  that probes for it also speaks iTerm2 or kitty.
- **Kitty first** — the biggest fork delta (new APC dispatch surface in
  vte's state machine) as the first project, for a protocol whose
  detection (`a=q` round-trip) only works once that machinery is live.
  Right second, wrong first.
- **Overlay placements only** (the kitty/Ghostty/Konsole model) — viable
  but strictly more bookkeeping against DropTerm's borrowed grid: every
  scroll/reflow/resize must re-anchor external state that alacritty_terminal
  already tracks for cells. Konsole needs it because its grid predates
  images; DropTerm can fork the cell instead.
- **ueberzug/overlayer-style external image windows** (the workaround
  Alacritty users run today, per issue #910 comments) — rejected outright:
  DropTerm *is* the terminal; an out-of-process overlay beside a
  drop-down layer-shell window is a different product.
- **Alacritty's answer — nothing, on principle** ("Terminals weren't
  designed to display graphics… Almost every usage for image protocols
  I've seen in the wild, is bad", chrisduerr, issue #910, 2026-07-07) —
  a defensible stance for a minimalist's terminal, but DropTerm already
  crossed the escape-sequence passthrough Rubicon three arms ago, and its
  nearest ecosystem (WezTerm/Konsole/VS Code/rio) all ship images.
- **Reading local files / shared memory on day one** (kitty `t=f/t/s`) —
  deferred permanently until a need is shown; the anti-probing error
  contract exists precisely so a terminal can say no safely.

## Unverified / open questions

- The exact iTerm2 release that first grew Sixel: `VT100SixelParser.m`
  exists at master and `docs/notes-3.7.0beta6.txt` fixes its bundled
  libsixel CVE, but beta1–beta5 notes carry no sixel bullet — "3.7.0 beta
  line (2026)" is as precise as the sources allow.
- contour's iTerm2/kitty cells: only its Sixel support was verified from
  `docs/features.md`; the iTerm2/kitty columns are marked `?` rather than
  guessed.
- Whether iTerm2's own client honors any cursor-suppression argument for
  1337 File: no such argument exists in `VT100Terminal.m`'s parser — the
  claim above is "iTerm2 has none", which is what the source shows; it is
  always possible an undocumented build differs.
- Ghostty's *first* release with kitty graphics: 1.2.0's notes carry only
  Kitty-image fixes (delete-by-range, gamma blending), so it predates 1.2;
  the introducing release was not pinned.
- vt100.net's VT330/VT340 Sixel chapter was unreachable (404) on the fetch
  date; Sixel byte encoding is cited from xterm's ctlseqs plus the
  WezTerm/mintty parsers instead. The classic PDF remains the archival
  reference.

## Sources

- iTerm2: images documentation —
  <https://iterm2.com/documentation-images.html>; imgcat script —
  <https://iterm2.com/utilities/imgcat>; source gnachman/iTerm2 @ master
  `40a75e3b32bbd697802135d13dc3cb78d2c65c1a`: `sources/VT100/VT100Terminal.m`
  L4178-4238, `sources/InlineImages/VT100InlineImageHelper.m` L229-293,
  L540-553, `sources/InlineImages/iTermImageInfo.m`,
  `sources/InlineImages/KittyImageCommand.swift`,
  `sources/VT100/VT100SixelParser.m`, `docs/notes-3.7.0beta6.txt` L45-47
  (all fetched 2026-11-05)
- kitty: kovidgoyal/kitty @ master (head `3e4ffc109891`,
  `docs/graphics-protocol.rst` last touched `3dc4f8d5aa0092b77c839145aff5602ca8664cd4`,
  2026-10-29): control-data reference, transmission/chunking, queries,
  unicode placeholders, deletion, scrolling/"Interaction with other
  terminal actions", "Image persistence and storage quotas" (320 MB),
  security section, implementors list, `kittens/icat/main.go` + `detect.go`
  (fetched 2026-11-05); rendered page —
  <https://sw.kovidgoyal.net/kitty/graphics-protocol/>
- xterm: ctlseqs —
  <https://invisible-island.net/xterm/ctlseqs/ctlseqs.html> (Sixel
  Graphics section L3218-3242: `DCS Pa;Pb;Ph q`, DA1 Ps=4, DECSDM ?80,
  mode 8452; XTSMGRAPHICS L679-684) (fetched 2026-11-05)
- WezTerm: wez/wezterm @ main `e68a8ec27727fbbda15e07f0a0737cd5e16481f6`
  (2026-10-26): `wezterm-cell/src/lib.rs` L58-98, L371-396, L719;
  `wezterm-cell/src/image.rs` L88-206, L226-320, L537-560;
  `term/src/terminalstate/image.rs` L13-130, L286-308;
  `term/src/terminalstate/iterm.rs` L11-153;
  `term/src/terminalstate/kitty.rs` L18-60;
  `term/src/terminalstate/sixel.rs` L13-60;
  `term/src/terminalstate/performer.rs` L279-284, L832;
  `wezterm-escape-parser/src/osc.rs` L950-1096; `docs/imgcat.md` —
  <https://wezterm.org/imgcat.html> (fetched 2026-11-05)
- rio: raphamorim/rio @ main `0d9ede193ace8a81c0b7976bedf0ebb4cd6da718`
  (2026-11-04): `rio-vt/src/ansi/{mod,graphics,sixel,
  kitty_graphics_protocol,iterm2_image_protocol}.rs` (fetched 2026-11-05)
- Konsole: KDE/konsole @ master `5b6645070e06` (2026-11-05):
  `src/Vt102Emulation.cpp` L643-651, L691-708, L759-760, L1773, L2411-2530;
  `src/Screen.h` L35-58 (fetched 2026-11-05)
- foot: codeberg.org/dnkl/foot @ master README L66-70 "Sixel image
  support" (fetched 2026-11-05)
- Ghostty: ghostty-org/ghostty @ main (pushed 2026-11-04):
  `include/ghostty/vt/kitty_graphics.h` (storage/placement/drawing model),
  `src/terminal/c/kitty_graphics.zig`,
  `src/terminal/device_attributes.zig` L53, L178-185; release notes 1.1.0
  and 1.2.0 (Kitty image protocol fixes) —
  <https://ghostty.org/docs/install/release-notes> (fetched 2026-11-05)
- mintty: mintty/mintty @ master `9b69382541d2` (2026-11-03):
  `src/termout.c` L4753-4759, L5091+; `src/sixel.c` (fetched 2026-11-05)
- xterm.js: xtermjs/xterm.js @ master `08829c7eb627` (2026-10-29):
  `addons/addon-image/README.md` (protocol list + default limits)
  (fetched 2026-11-05)
- VS Code: microsoft/vscode @ main `ae2318821f749b21eca5254fd2cf16ee667a9516`
  (2026-11-05): `src/vs/workbench/contrib/terminal/browser/xterm/
  xtermTerminal.ts` L12, L159, L1043-1055 (image addon gated on
  `enableImages` + WebGL) (fetched 2026-11-05)
- contour: contour-terminal/contour @ master:
  `src/vtbackend/graphics/SixelParser.cpp`; `docs/features.md` "Sixel
  Image support" (fetched 2026-11-05)
- VTE: gitlab.gnome.org/GNOME/vte @ master (2026-11-05): `src/image.hh`
  (saitoha/hpjansson copyright headers), `src/vte.cc` L3664-3699, L4055-4057,
  L4470-4492 (`#if WITH_SIXEL`); `meson.options` (`sixel` default false);
  issue #2717 "Regression: Sixel rendering broken since native GTK4
  drawing" (open, 2023-12-25)
- Windows Terminal: microsoft/terminal @ main
  `src/terminal/adapter/SixelParser.{hpp,cpp}`; v1.22.11751.0 release
  notes (sixel scaling/copy fixes) (fetched 2026-11-05)
- cosmic-term: pop-os/cosmic-term @ master `8a05e9194881` (2026-10-29) —
  no image/sixel/kitty code (tree + code search, 2026-11-05)
- Alacritty: alacritty/alacritty issue #910 "Add support for libsixel"
  (open, 2017-11-21, 238 comments; maintainer position quoted 2026-07-07);
  PR #4763 "Support for graphics in the terminal" (open); issue #3037
  (closed — OSC52/tmux, *not* sixel) (fetched 2026-11-05)
- tmux(1) — <https://man7.org/linux/man-pages/man1/tmux.1.html>
  (`allow-passthrough`, fetched 2026-11-05)
- timg: hzeller/timg @ main `a40148448f2b` (2026-08-05):
  `src/term-query.cc` L226-344, `src/timg.cc` L112-130, L315-345, L455+,
  `src/kitty-canvas.cc` L85-115 (tmux passthrough) (fetched 2026-11-05)
- viu/viuer: atanunq/viuer @ master `ed329e5c01d0` (2025-12-09):
  `src/printer/iterm.rs` L94-106, `src/printer/kitty.rs` L80-95;
  atanunq/viu README (fetched 2026-11-05)
- chafa: hpjansson/chafa @ master `c897fce60cc4` (2026-11-01):
  `chafa/internal/chafa-kitty-renderer.c`, NEWS (detection-heuristic
  entries) (fetched 2026-11-05)
- DropTerm local: `Cargo.toml`, `src/terminal/engine.rs`,
  `src/terminal/widget.rs` @ HEAD; fork checkouts
  `~/.cargo/git/checkouts/vte-a8c42e20f33cfc88/7cd0e4b` (`src/lib.rs`
  L182, L300-332, L377, L471, L587, L774-792; `src/ansi.rs` L1297-1360)
  and `~/.cargo/git/checkouts/alacritty-d05b18960149e89e/c9eb7de`
  (`alacritty_terminal/src/event.rs` L13-64, `src/term/mod.rs` L1445-1460,
  `src/term/cell.rs` L123-141); libcosmic @ pinned
  `08a09e3dfa1f729d52585fcedd6e731130235363` (`iced/wgpu/src/lib.rs`
  L863-871, `iced/graphics/src/geometry/frame.rs` L98-104)
