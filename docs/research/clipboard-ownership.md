# Research: who owns the clipboard write in a drop-down terminal

Date: 2026-11-02
Question: *Why do DropTerm's copies — chord, context menu, band click, and
OSC 52 from an editor — silently vanish on a long-lived instance, and who
should own the write?*

## TL;DR

**Every clipboard write the app made was coupled to keyboard focus, and the
coupling lives below DropTerm in libcosmic's clipboard stack — the fix is for
DropTerm to own its terminal clipboard I/O over the data-control protocol
(`src/clipboard.rs`), which needs no focus and no input serial.** The chain
`cosmic::iced::clipboard` → iced's winit runtime → `window_clipboard`
(pop-os sctk-0.20 fork) → smithay-clipboard 0.7.3 sets selections through
the legacy `wl_data_device`, and smithay-clipboard's worker drops every
store when its seat lacks keyboard focus — silently, at every layer. A
second bug rode along: DropTerm computed the OSC 52 query reply and
discarded it, so programs that asked for the clipboard never got an answer.

## Findings

The evidence is a live hardware investigation on cosmic-comp (COSMIC,
Wayland), not a source survey — each numbered finding names where it was
observed.

1. **The drop gate.** smithay-clipboard 0.7.3's worker,
   `store_selection` (`src/state.rs:112-118` in the crates.io source):
   `if !seat.has_focus { return None; }` — and `set_selection` is called
   with `seat.latest_serial`, which the `Leave` handler zeroes
   (`src/state.rs:531`). A Wayland `set_selection` needs a valid serial
   from a focused input event; focus loss invalidates both preconditions.
   The store is fire-and-forget: `Command::Store` is sent into the
   worker's channel and the `None` is dropped — no error path exists.

2. **The silence is total.** iced's winit runtime maps
   `Action::Clipboard(Write)` to `Clipboard::write`
   (`iced/winit/src/clipboard.rs:151-170` in libcosmic 08a09e3); a
   `State::Unavailable` runtime no-ops the write with no log, and a
   `Connected` write returns `Ok(())` regardless because
   `smithay-clipboard::store` returns `()`. DropTerm's handler called
   `clipboard::write(text)` and had no way to learn anything failed.

3. **A drop-down terminal lives in the failure window.** DropTerm
   destroys and recreates its layer surface on every hide/show (the
   cosmic-comp role-destroy workaround), `hide_on_focus_loss` is on by
   default, and background/CLI shows map without an activation token.
   Measured on hardware: a fresh instance copies correctly (6/6
   launches); the user's 9-hour instance failed every visible-but-
   unfocused copy; the *same* instance copied correctly immediately
   after a hide→show re-map re-steals focus. The only variable
   explaining every outcome is the focus state at the write instant.

4. **The clipboard anchor solved a different half.** The never-closed
   anchor window (shell-presence spec) keeps the *connection* — and
   thus an already-landed offer — alive across hide/show. It cannot
   help a *new* store through the focus-coupled path; the 2026-11-02
   instance carried a clipboard offer while every new write vanished.

5. **Data-control is focus-free on this compositor, both directions.**
   Test binaries built from wl-clipboard-rs 0.9 (which speaks
   `zwlr_data_control`/ext-data-control) read and wrote the session
   clipboard with no window and no focus; a smithay (wl_data_device)
   write was readable from data-control and vice versa. wl-clipboard-rs
   `copy()` spawns its own detached serving thread per copy — no fork,
   serving until replaced (`src/copy.rs`, `copy_internal`).

6. **The OSC 52 reply was discarded.** DropTerm's
   `TermEvent::ClipboardLoad` handler computed
   `format(&data.unwrap_or_default())` and dropped the string
   (`src/app/tab_actions.rs`, the pre-fix arm) — a conforming terminal
   writes the reply to the PTY. Queries were never answered, focus
   aside. (The engine's default `Osc52::OnlyCopy` also denies loads —
   alacritty's default — so the reply path is latent until a config
   key allows it, but the discard was a bug either way.)

## What DropTerm has today

Pre-fix: every clipboard operation routed through
`cosmic::iced::clipboard` (seven Task-returning sites) plus four
widget-layer `dyn Clipboard` writes in the status band; the OSC 52
reply was computed and discarded.

## Recommendation

**Own the terminal clipboard I/O in `src/clipboard.rs` over
wl-clipboard-rs (data-control), falling back to the iced clipboard once
per failure with a `warn!`.** Writes: `copy()` per store, verbatim bytes
(`trim_newline(false)` pinned — a copied shell line keeps its newline).
Reads: `get_contents` with `Seat::Unspecified`; an empty clipboard
answers `None` silently, transport errors warn and fall back. The band's
copies invert to app-side writes through a `Copied` report so nothing
writes at the widget layer. The OSC 52 reply reaches the asking tab's
PTY by id (`Message::Osc52LoadReply`).

Rejected alternatives:

- **A `[patch]` of smithay-clipboard or libcosmic** — vendoring the fix
  into every build; the boundary says upstream fixes are filed, not
  patched, without a separate decision. An upstream issue is the
  follow-up (this record is its evidence).
- **Queueing writes until focus returns** — pastes happen *after* the
  copy leaves the terminal; a queue cannot deliver to a user who
  already pasted. Data-control removes the dependency entirely.
- **Waiting on an iced/libcosmic fix** — the coupling is by design in
  the `wl_data_device` protocol; the fix is a protocol change
  (data-control), not a bug fix upstream.

## Sources

- smithay-clipboard 0.7.3, crates.io source (`state.rs`, `worker.rs`,
  `lib.rs`) — read 2026-11-02.
- libcosmic @ 08a09e3 (`iced/winit/src/clipboard.rs`,
  `iced/winit/src/lib.rs` Action::Clipboard handling,
  `iced/runtime/src/clipboard.rs`) — read 2026-11-02.
- window_clipboard pop-os fork @ f68595e (`wayland/src/lib.rs`,
  `src/platform/linux.rs`) — read 2026-11-02.
- wl-clipboard-rs 0.9.5 (`copy.rs`, `paste.rs`) — read and exercised
  via test binaries on cosmic-comp, 2026-11-02.
- Live experiments: 6 fresh-instance launches (6/6 copies land), a
  9-hour instance (copies fail while visible-but-unfocused; land inside
  a focused re-map window), hidden-instance stores (fail pre-fix, land
  post-fix), cross-protocol reads both directions.
