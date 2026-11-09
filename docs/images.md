# Inline images

`imgcat photo.png` renders the image in the terminal, inline — and `kitten icat photo.png` too, detected live. Both scroll away with the output like any text, reflow with the window, and clear with the screen. No shell integration, nothing installed into your rc files: the terminal decodes what the command prints.

## What ships

- **OSC 1337** — iTerm2's inline-image protocol, both of its wire forms: the legacy single-shot `File=` sequence and the chunked `MultipartFile` / `FilePart` / `FileEnd` session (what today's `imgcat` actually emits). [WezTerm](https://github.com/wez/wezterm)-family tools speak it natively.
- **The kitty graphics protocol** — the field's second standard, through the APC escape: the subset real emitters use, with live query detection (`kitten icat` works with nothing but itself installed). See [the kitty section](#the-kitty-graphics-protocol).
- **PNG, JPEG, GIF, and WebP** decode. Everything else renders nothing — quietly, never half.
- Images ride the **grid like text**: a placement is diced across the cells it covers, so scrollback, reflow, resize, and alt-screen clearing treat it exactly as they treat characters. There is no floating overlay to fall out of sync.
- The kill switch: `images_enabled` (default `true`, Behavior section in the [settings drawer](settings.md)) makes the whole pipeline inert — the sequence stays parsed but nothing decodes or places. A flip applies live.

## Pasting images

Pasting an image is a different road than the ones above, and it deliberately never renders. Two clipboards reach the same pipeline: image **bytes** on the clipboard (a screenshot tool's copy — PNG, the interchange format every compositor converts to), and an image **file copied in a file manager** — COSMIC Files, Nautilus, Dolphin — or landed by a screenshot portal, which the paste reads through its `file://` URI. What a paste does:

1. The image bytes are read — from the clipboard offer, or from the one local file a qualifying URI names — and written to `~/.cache/dropterm/paste/paste-YYYYMMDD-HHMMSS-<n>.<ext>`, the extension the image's own format (`.png`, `.jpg`, `.gif`, or `.webp`, decided by the bytes' content).
2. That file's **path pastes as text** — the paste's one and only trace in the grid, ready for the shell, `claude`, `imgcat`, or anything that takes a file argument.
3. The paste is recorded in its tab's **images panel**. Three doors open it: the pasted-images chip in the status band (`3 images` while the tab has pastes; hover it for a tip naming what it counts and what the click does), the band's right-click menu (**Pasted Images…**), and `Ctrl+Shift+P`. The chip yields first when the band gets crowded — the menu row and the keybind are the doors that never hide, and the panel's empty state is the honest answer to a tab with nothing pasted. The panel lists the tab's pastes newest-first with a thumbnail per image, its stamp restated for reading (`2026-11-09 14:30:25`) with the dimensions dimmed beneath; the thumbnail and its stamp are themselves the **Open** button — the image is the row's subject, so the subject is its own largest target for its standard verb. Every row carries five verbs — **Open** (the system viewer), **Copy path**, **Copy image** (the bytes back to the clipboard in their own format — over SSH, where the local path is useless, this is the one that travels), **Re-paste** (the path again, through the same paste channel), and **Delete** — the one destructive verb, styled destructive and held apart at the row's far edge behind a two-step confirm: its press swaps to a ✓/✕ icon pair (tooltips **Confirm delete** and **Keep** — labeled buttons rode past the drawer's narrow edge), and only the ✓ removes the file, row, and thumbnail together. The file it deletes is the paste's only durable copy, which is why one click never erases it. The panel header's **Open Folder** action (live while rows stand) shows the paste cache directory in your file manager.

The file-copy road fires for **exactly one local image**: a single `file://` URI (empty or `localhost` host) whose file's first bytes are one of the four formats above. The file's own content is the only thing that decides — the file's name, its extension, and the URI's claim are never consulted, so a URI naming a non-image file never pastes its bytes, and a URI naming a FIFO or a directory is never opened. Everything else is today's text paste, unchanged: a multi-file selection, a non-image file, a network share (`smb://`), the trash (`trash://`) — the offer's text pastes verbatim, exactly what a paste of those copies did before this road existed.

No terminal in the survey behind this design pastes clipboard images at all; the two apps that do (Warp, Claude Code) collapse the image to a compact marker backed by a file. DropTerm follows: the path is the marker, the panel keeps the pastes findable after the prompt scrolls away, and pixels stay where a program asked for them. The survey is [in the research log](research/paste-image-ux.md); the file-copy road's own survey and compositor observations are [there too](research/paste-uri.md).

Nothing sweeps the paste cache automatically — files stay until you remove them through the panel's **Delete** verb or by clearing the directory, and a row whose file is gone leaves the panel with it. One ceiling applies on both roads: an image paste larger than 32 MiB is refused whole (nothing pastes; one warn names it in the log — never a fallback that would paste a URI where you expected your image). Note the SSH caveat in the other direction: the pasted path is local to where DropTerm runs — a remote shell receives a path it cannot read (copy image is the workaround).

## Driving it

- **imgcat** (iTerm2's own utility — `curl -L https://iterm2.com/utilities/imgcat > ~/.local/bin/imgcat && chmod +x ~/.local/bin/imgcat`): `imgcat photo.png` uses the chunked session; `imgcat -l photo.png` uses the legacy single-shot. Both render.
- **wezterm imgcat**: same protocol, same result.
- **kitten icat**: `kitten icat photo.png` probes with a live query, detects DropTerm, and renders — no environment variables set, nothing configured.
- **chafa**: `chafa -f kitty photo.png` and `chafa -f iterm photo.png` both render.
- **timg** / **viu**: force their iTerm2 mode and they render; `timg photo.png` under `TERM=xterm-kitty` (a manual env override — DropTerm's own `TERM` never claims to be kitty) speaks the kitty path.

## The kitty graphics protocol

The second protocol rides the APC escape (`ESC _ … ESC \`), and its subset is named honestly:

- **Transmission is direct in-band only** (`t=d`, the default). The file, temp-file, and shared-memory forms (`t=f`/`t=t`/`t=s`) are all refused with one identical `EBADF:Failed to read image file` error — DropTerm never reads the filesystem or shared memory on the terminal's behalf, and because every refusal answers byte-for-byte the same, nothing about the machine is probeable through it.
- **Formats**: `f=100` and `f=24` ride the same container decode as OSC 1337 (PNG, JPEG, GIF, WebP under the same ceilings); `f=32` (the default) is raw RGBA sized by `s`×`v`; `o=z` payloads are inflated under the same payload cap.
- **Chunking** (`m=1` … final `m=0`, possibly empty) assembles per image, keyed by the client's `i=` id — interleaved transmissions each deliver their own.
- **Actions**: `a=T` displays at the cursor (the cursor advances below the placement), `a=t` — the default — transmits without displaying, `a=p` re-places a transmitted image at the cursor (a `p=` placement id moves exactly its own cells and reuses its pixels — no flicker; without one, the cells ride the image's own id), `a=q` queries, `a=d` deletes.
- **Queries answer byte-exact, and that is how detection works**: a test-load (`a=q,i=…`) replies `OK` when the image's pixels stand, the uniform error otherwise; `q=1` silences OK replies, `q=2` everything; `a=q,e=T` — and `e=t`/`e=p`/`e=d` — answers with exactly the keys this implementation honors. `kitten icat` sends the query and then a DA1; a DA1 answer with no graphics reply means "no support", so every query answers, always before the DA1.
- **Deletion** (`a=d`): `d=a`/`A` every image, `d=i<id>`/`I<id>` one image, `d=n`/`N` the newest, `d=c`/`C` the visible screen — lowercase removes the placements, uppercase frees the pixels too. Freed cells draw blank, exactly like evicted ones. Region deletes and other `d=` values are ignored.
- **z-index**: `z<0` draws beneath the glyphs, `z≥0` above them and below the cursor, overlapping above-layer images compositing by z (the higher on top). An absent `z` reads as beneath — the drawing OSC 1337 images own — where kitty's own default is `z=0` (above): a deliberate, conservative deviation.

Unknown control keys are ignored per kitty's own spec; malformed control data drops the sequence whole. The same kill switch, the same ceilings, the same grid seat — a kitty placement rides the cells exactly like an OSC 1337 one.

## Sizing

The `width` and `height` arguments accept iTerm2's unit grammar — `N` cells, `Npx` pixels, `N%` of the terminal, or `auto` (the image's own size, the default). Aspect ratio is preserved whenever only one axis is given (the other follows) and when both are given (the image fits inside); `preserveAspectRatio=0` honors both axes exactly instead. A placement wider than the terminal is cut off on the right, like over-long text — never squeezed.

`inline=0` sequences render nothing. In iTerm2 that means "download the file"; DropTerm has no download UI and builds none — the sequence is accepted, and that is all.

## The honest limits

Every payload is hostile until decoded, so the ceilings come first — an image past any of them renders nothing, never a partial frame:

| Ceiling | Value |
|---|---|
| Payload per sequence | 32 MiB decoded bytes |
| Decode allocation | 64 MiB, dimensions capped at 8192×8192 |
| Total decoded pixels across all images | 320 MiB, oldest evicted |

Evicted images leave their cells blank — the placement stays in the grid, the pixels are gone. An image can be re-placed by running the command again.

**The no-Sixel note, stated plainly**: DropTerm does not answer Sixel capability queries (`DA1` replies without the `;4` bit), so well-behaved emitters fall back to text or their iTerm2 backend rather than sending pixels that would render as garbage. Sixel support is deferred, not hidden behind a spoofed reply — a terminal that lies about what it can do breaks every tool that trusts it.

## Where the caps live

`images_enabled` is documented with the rest in [Configuration](configuration.md). The ceiling numbers are constants — changing one is a one-line patch, and they are recorded in the [research log](research/terminal-images.md) alongside the protocol survey that chose this design.
