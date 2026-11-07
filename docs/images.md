# Inline images

`imgcat photo.png` renders the image in the terminal, inline — and `kitten icat photo.png` too, detected live. Both scroll away with the output like any text, reflow with the window, and clear with the screen. No shell integration, nothing installed into your rc files: the terminal decodes what the command prints.

## What ships

- **OSC 1337** — iTerm2's inline-image protocol, both of its wire forms: the legacy single-shot `File=` sequence and the chunked `MultipartFile` / `FilePart` / `FileEnd` session (what today's `imgcat` actually emits). [WezTerm](https://github.com/wez/wezterm)-family tools speak it natively.
- **The kitty graphics protocol** — the field's second standard, through the APC escape: the subset real emitters use, with live query detection (`kitten icat` works with nothing but itself installed). See [the kitty section](#the-kitty-graphics-protocol).
- **PNG, JPEG, GIF, and WebP** decode. Everything else renders nothing — quietly, never half.
- Images ride the **grid like text**: a placement is diced across the cells it covers, so scrollback, reflow, resize, and alt-screen clearing treat it exactly as they treat characters. There is no floating overlay to fall out of sync.
- The kill switch: `images_enabled` (default `true`, Behavior section in the [settings drawer](settings.md)) makes the whole pipeline inert — the sequence stays parsed but nothing decodes or places. A flip applies live.

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
- **z-index**: `z<0` draws beneath the glyphs (the default drawing, what OSC 1337 images do), `z≥0` above them and below the cursor.

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
