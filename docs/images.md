# Inline images

`imgcat photo.png` renders the image in the terminal, inline — and it scrolls away with the output like any text, reflows with the window, and clears with the screen. No shell integration, nothing installed into your rc files: the terminal decodes what the command prints.

## What ships

- **OSC 1337** — iTerm2's inline-image protocol, both of its wire forms: the legacy single-shot `File=` sequence and the chunked `MultipartFile` / `FilePart` / `FileEnd` session (what today's `imgcat` actually emits). [WezTerm](https://github.com/wez/wezterm)-family tools speak it natively.
- **PNG, JPEG, GIF, and WebP** decode. Everything else renders nothing — quietly, never half.
- Images ride the **grid like text**: a placement is diced across the cells it covers, so scrollback, reflow, resize, and alt-screen clearing treat it exactly as they treat characters. There is no floating overlay to fall out of sync.
- The kill switch: `images_enabled` (default `true`, Behavior section in the [settings drawer](settings.md)) makes the whole pipeline inert — the sequence stays parsed but nothing decodes or places. A flip applies live.

## Driving it

- **imgcat** (iTerm2's own utility — `curl -L https://iterm2.com/utilities/imgcat > ~/.local/bin/imgcat && chmod +x ~/.local/bin/imgcat`): `imgcat photo.png` uses the chunked session; `imgcat -l photo.png` uses the legacy single-shot. Both render.
- **wezterm imgcat**: same protocol, same result.
- **chafa**: `chafa -f iterm photo.png` forces the iTerm2 backend (its kitty backend needs a protocol DropTerm does not speak yet — see the honest note below).
- **timg** / **viu**: force their iTerm2 mode and they render too.

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
