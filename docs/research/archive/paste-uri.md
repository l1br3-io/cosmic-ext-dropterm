# Research: should a paste read the local file a copied image's URI names — and how is that gate built?

Date: 2026-11-09
Question: *A file-manager copy (COSMIC Files, Nautilus, Dolphin) or a screenshot
portal lands `file://` URIs on the clipboard alongside text offers, and DropTerm's
paste reads the text: the user who copies a photo and pastes gets
`file:///home/you/photo.jpg` typed at their prompt — or nothing, from a manager
that offers no text type. Should the paste instead read the named local file and
feed it through the paste-image pipeline — and if the terminal starts opening
files the clipboard names, what does that gate have to be?*

## TL;DR

**One road, content-gated: the paste's first read prioritizes `text/uri-list`, and
a qualifying list — exactly one URI, `file` scheme, empty-or-`localhost` host —
names the one file the terminal may open, admitted only by its own first bytes
(never its name, extension, or the URI's say-so).** The verified ground makes the
priority read mandatory rather than optional: COSMIC Files serves `text/plain`
alongside `text/uri-list` on a file copy (historically *first*, per
[cosmic-files#1659], reordered in #1705), so DropTerm's text read succeeds today
and a URI road bolted onto the empty-clipboard fallthrough would never fire. The
file road rides the existing paste-image pipeline unchanged — cache copy, path
paste, panel row — extending the [paste-image-ux](../paste-image-ux.md) verdict
(the path is the marker) to the copy that arrives as a name instead of bytes.

## Findings

1. **The offer sets are real and named.** [cosmic-files#1659] documents a file
   copy offering `text/plain` before `text/uri-list` (so text-first readers
   pasted the URI); #1705 reordered the offers. The screenshot portal lands
   `file://` URIs too ([xdg-desktop-portal-cosmic#295]). Either ordering, the
   reader must *ask* for the list — which is what `TextWithPriority` does.

2. **wl-clipboard-rs 0.9.5's selection rules (read from the pinned source,
   `paste.rs`):** `TextWithPriority(p)` matches `p` by name anywhere in the
   offer list, then falls back to the `Text` chain — `text/plain;charset=utf-8`
   → `UTF8_STRING` → any `text/*`. Offer order is irrelevant to the priority
   match, so both old and new cosmic-files behave alike under it. `Text` alone
   prefers the decorated `text/plain;charset=utf-8` — which matters below.

3. **Live observations, cosmic-comp, 2026-11-09** (a probe serving real
   data-control offers and reading them back on the running session; reproduced
   by the committed `live_uri_paste_smoke` test):
   - The priority read answers **`text/uri-list` verbatim** in every offer
     shape — uri-list plus text/plain (cosmic-files), uri-list only (portal),
     text only. This resolves the spec's OQ-1: exact match, no decoration.
   - The plain `Text` read answers **`text/plain;charset=utf-8`**, and when a
     uri-list is on the clipboard the content is the compositor's *conversion
     of the list* (the `file://…` lines, CRLF) — not the offered bare
     `text/plain` content (OQ-2's observation: what "today's text paste" means
     for a file copy here is the list text itself). The fallback read is the
     same `Text` request, so it reproduces today byte-for-byte by
     construction.

4. **The safety gate is the point.** "The terminal reads files the clipboard
   names" is the risky sentence, and the gate answers it: regular files only
   (`metadata()` before any open — a FIFO would block the paste forever), one
   open and one bounded read (no TOCTOU re-read; the sniffed bytes are the
   pasted bytes), a 32 MiB ceiling refused whole (never a truncation, never a
   text fallback that would paste the URI where the image was expected), and
   the four-format magic-byte sniff as the only admission rule. This is the
   [terminal-images](../terminal-images.md) record's own ethic — DropTerm refused
   kitty's `t=f`/`t=s` file reads rather than open files a *program* named;
   here the gesture is the user's own paste, and the read stays bounded,
   sniff-gated, and confined to what the pipeline can decode.

5. **The precedent field stays text-only; this is an extension of DropTerm's
   own model, not a divergence.** No terminal in the
   [paste-image-ux](../paste-image-ux.md) survey pastes clipboard images at all;
   the two products that handle clipboard images (Warp, Claude Code) collapse
   them to a marker backed by a file. The file-copy road lands the same marker
   (the cache path) for the copy that arrives as a URI — no new presentation,
   no pixels in the scrollback, the panel unchanged.

## What DropTerm has today

The paste-image pipeline (2026-11-08): clipboard PNG bytes → a `0600` cache copy
under a generated name → the path pasted as text → the per-tab images panel row
with thumbnail and verbs. The read cascade is text-first with an image fallthrough
on the empty classification; a file-manager copy pastes its text offer verbatim
(finding 3: on cosmic-comp, the converted list text). The panel's Copy image
offers `image/png`; the cache holds only PNG.

## Recommendation

As shipped (paste-uri, 2026-11-09): the first paste read becomes
`TextWithPriority("text/uri-list")`; the served mime picks the road — URI list →
parse (RFC 2483: lines, CRLF, `#` comments) → qualify (exactly one URI, `file`
scheme, empty-or-`localhost` host, percent-decoded UTF-8 path) → the file gate
→ sniffed bytes into the pipeline; a text type → today's paste; empty → today's
image attempt. `write_paste_image` names the cache file by the sniffed format
(`.png`/`.jpg`/`.gif`/`.webp`); the panel's Copy image offers the sniffed mime
(`image/png` the floor). The whole road rides `PASTE_IMAGE_ENABLED`. All of it
in `src/clipboard.rs` — no message, signature, or call-site change elsewhere.

Rejected:

- **PNG-only** (the conservative fork) — the owner approved all four decodable
  formats; the sniff already knows them.
- **Reading `x-special/gnome-copied-files`** — it carries a cut/move verb the
  read must never act on; `text/uri-list` carries none.
- **Trusting the filename or the offer's mime claim** — a `photo.png` named
  text file must never paste; content is the gate.
- **Multi-image copies** — exactly one fires; two or more is a file list, and
  the text fallback is today's behavior for it.
- **Quoted path-pasting for non-image copies** — a separate feature with its
  own quoting rules; today's verbatim text stands.

## Sources

- [cosmic-files#1659] — the text/plain-first offer order on a file copy
  (https://github.com/pop-os/cosmic-files/issues/1659; via the paste-uri spec's
  survey).
- [xdg-desktop-portal-cosmic#295] — the screenshot portal landing `file://`
  URIs (https://github.com/pop-os/xdg-desktop-portal-cosmic/issues/295).
- wl-clipboard-rs 0.9.5, `src/paste.rs` — `get_contents` and `select_mime_type`,
  read from the pinned registry source (selection chain and priority match).
- Live cosmic-comp session, 2026-11-09 — served-mime and conversion
  observations (findings 3), reproducible via `cargo test --bin
  cosmic-ext-dropterm live_uri_paste_smoke -- --ignored`.
- DropTerm's own [paste-image-ux](../paste-image-ux.md) and
  [terminal-images](../terminal-images.md) records — the marker model and the
  no-program-named-file-reads ethic this road extends.

[cosmic-files#1659]: https://github.com/pop-os/cosmic-files/issues/1659
[xdg-desktop-portal-cosmic#295]: https://github.com/pop-os/xdg-desktop-portal-cosmic/issues/295
