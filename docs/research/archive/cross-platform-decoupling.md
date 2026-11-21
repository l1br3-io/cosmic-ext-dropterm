# Research: how does DropTerm go cross-platform without losing the drop-down?

Date: 2026-10-18
Question: *DropTerm is a COSMIC application today — libcosmic for UI, layer-shell
for the drop-down, D-Bus for single-instance and state, cosmic-config for
settings. The ROADMAP's V4 line asks for "app version and cross platform".
Which path reaches Windows, macOS, and non-COSMIC Linux while keeping COSMIC
the flagship: a wholesale port to stock iced, a rewrite in egui/Slint,
ghostty-style per-platform native GUIs, or staged decoupling behind seams?*

## TL;DR

**DropTerm should decouple in stages — extract the platform-free core behind
seams, keep libcosmic as the Linux/COSMIC frontend unchanged, and add stock
iced as the cross-platform frontend — because the entire UI already speaks
libcosmic's iced dialect, so a stock-iced frontend is a re-pathing of imports
plus a handful of named seams, not a rewrite.** The terminal engine
(`alacritty_terminal` + `cosmic-text`) is already cross-platform — the "cosmic"
in cosmic-text is provenance, not a platform — and the terminal-widget-on-iced
pattern is proven on all three desktop OSes by `iced_term` (iced, MIT,
"tested on macOS, Linux, and Windows", released 2026-03-27). The genuinely
Linux-only surfaces are enumerable: layer-shell windowing, the three D-Bus
seams (single-instance activation, served state, bell notifications),
cosmic-config, and cosmic-theme — each gets a per-platform adapter. The
drop-down's signature mechanism stays untouched on COSMIC: layer-shell is a
Wayland standard (wlroots compositors, KWin, cosmic-comp), not a COSMIC
private protocol; GNOME's Mutter is the one holdout, where the degradation
path is an undecorated always-on-top toplevel — the same shape every
non-layer-shell platform gets. Nothing in the plan requires in-app global
hotkey registration: DropTerm's toggle has always been "launch the binary
again" over the single-instance seam, and every desktop lets a user bind a
key to a command.

## Findings

Numbered, with the evidence.

1. **The UI code is already iced — the fork's dialect.** `src/terminal/widget.rs:455`
   implements `Widget<Message, cosmic::Theme, Renderer> for TerminalBox` using
   only the advanced-widget API (`Shell`, `Tree`, `layout`, `overlay`,
   `renderer::Quad`) that upstream iced has shipped since 0.12; every import
   is a `cosmic::iced::*` re-export (`src/app.rs:22-31`,
   `src/terminal/widget.rs:24-52`). libcosmic is a fork of iced maintained by
   System76 precisely because upstream would not take the Wayland
   shell-integration changes fast enough ([System76 blog, "libcosmic
   rebasing"](https://system76.com/blog/post/cosmic-de-tiling-redesign-and-libcosmic-rebasing);
   [r/pop_os discussion](https://www.reddit.com/r/pop_os/comments/1d4abvo/why_libcosmic_uses_a_fork_of_icedrs)).
   Porting cost to stock iced therefore scales with the *divergence* — theme
   types, config system, layer-shell commands — not with the widget code
   itself.
2. **Stock iced is mature and cross-platform.** iced 0.14.0 (released
   2025-12-07; latest as of fetch 2026-10-18 — [crates.io/iced](https://crates.io/crates/iced),
   [releases](https://github.com/iced-rs/iced/releases)) runs on Windows,
   macOS, Linux, and wasm over winit; native multi-window landed in 0.12
   ([Phoronix](https://www.phoronix.com/news/Iced-Rust-GUI-Toolkit-v0.12));
   0.14 adds reactive rendering. What upstream iced does *not* have is
   layer-shell — that is the fork's headline divergence (finding 4).
3. **A terminal widget over `alacritty_terminal` on iced is proven on all
   three desktop platforms, MIT-licensed.** [`iced_term` 0.8.0](https://crates.io/crates/iced_term)
   (released 2026-03-27, fetched 2026-10-18; repo
   [Harzu/iced_term](https://github.com/Harzu/iced_term)) describes itself as
   "terminal emulator widget powered by ICED framework and alacritty terminal
   backend", tested on macOS, Linux, and Windows. The same author ships
   [`egui_term`](https://github.com/Harzu/egui_term) for egui. These are
   feasibility precedents and styling references for the widget port — not
   replacements: DropTerm's `TerminalBox` carries autosuggestions, the status
   band's probes, and the context-menu publish seam that no off-the-shelf
   widget has.
4. **Layer-shell is a Wayland standard with exactly one holdout.**
   [wlr-layer-shell-unstable-v1](https://wayland.app/protocols/wlr-layer-shell-unstable-v1)
   is implemented by the wlroots family (sway, Hyprland, river, labwc), by
   KDE's KWin, and by cosmic-comp — a DropTerm built on stock iced keeps its
   exact drop-down semantics on all of them. Mutter refuses the protocol on
   principle ([GNOME/mutter#973](https://gitlab.gnome.org/GNOME/mutter/-/issues/973),
   open since 2019); GNOME users get quake behavior through Shell extensions
   that manage ordinary windows ([quake-mode → its maintained
   fork](https://github.com/repsac-by/gnome-shell-extension-quake-mode)) —
   i.e. on GNOME the honest degradation is a normal undecorated window an
   extension *or* the user can manage. For stock iced,
   [`iced_layershell` 0.19.1](https://crates.io/crates/iced_layershell)
   (released 2026-07-12, fetched 2026-10-18; repo
   [waycrate/exwlshelleventloop](https://github.com/waycrate/exwlshelleventloop))
   binds layer-shell to iced — 74 releases since March 2024, actively
   maintained. The open integration risk: iced_layershell replaces iced's
   window runtime rather than augmenting it, so one process mixing ordinary
   winit windows and layer surfaces is the seam to de-risk early; the
   safe staging is layer-shell-only or toplevel-only per build until that
   lands.
5. **The toggle never required in-app hotkey registration — on any platform.**
   DropTerm's design is second-launch-toggle: a second run of the binary
   reaches the first instance over the single-instance seam
   (`src/main.rs:148`), and the user binds F12 to the command in DE settings
   (README, "Setting up the shortcut"). That pattern ports as-is: every
   desktop can bind a key to a command (KDE/GNOME custom shortcuts, Windows
   PowerToys Keyboard Manager, macOS third-party launchers). If direct
   registration is ever wanted: the XDG GlobalShortcuts portal
   ([spec](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html),
   xdg-desktop-portal ≥ 1.16) is implemented by KDE and Hyprland backends;
   GNOME's backend lagged for years
   ([xdg-desktop-portal-gnome#183](https://gitlab.gnome.org/GNOME/xdg-desktop-portal-gnome/-/issues/183),
   Sep 2025) with reports of it landing around GNOME 50; COSMIC implements
   none (README's known-risks). Outside portals, the tauri-maintained
   [`global-hotkey`](https://github.com/tauri-apps/global-hotkey) crate covers
   Windows, macOS, and X11 — but not Wayland, where portals are the only
   sanctioned route. Conclusion: hotkeys are a non-blocker; keep the
   bind-a-command design.
6. **The D-Bus seams are the Linux-only surface, and they are few.** Three
   seams ride freedesktop infrastructure: single-instance activation
   (libcosmic's `run_single_instance`, `src/main.rs:148`), the served state +
   CLI query contract (zbus, `src/dbus_state.rs`, the `--is-visible` exit at
   `src/main.rs:137-138`), and bell notifications (notify-rust,
   `src/notify.rs:44`). On Windows/macOS the equivalents are OS-native:
   named pipes / Unix-domain sockets for the single-instance and remote
   verbs, OS notifiers for bells. Notably, a UDS/named-pipe seam *can* carry
   replies — the structural limitation the
   [remote-control-commands](remote-control-commands.md) verdict accepted —
   so the cross-platform port turns that constraint into an opportunity
   without relitigating the Linux seam.
7. **Peer terminals show exactly two credible shapes.** (a) Shared engine +
   per-platform GUI — ghostty's documented architecture: one Zig core, AppKit
   on macOS, GTK4 on Linux ([ghostty docs, "About"](https://ghostty.org/docs/about));
   the engine/GUI separation is the precedent worth copying, the two-native-
   GUIs cost is not (ghostty is a company-funded project). Even ghostty's
   drop-down (quick-terminal) is macOS-first, with Linux/Wayland still an
   open discussion ([ghostty#11118](https://github.com/ghostty-org/ghostty/discussions/11118))
   — territory DropTerm already owns on its platform. (b) One cross-platform
   GUI over winit + custom rendering — Alacritty and WezTerm's shape: a
   single codebase, at the price of hand-drawn chrome. A stock-iced frontend
   is (b) with the toolkit already shared by lineage — the custom-drawing
   Alacritty does by hand (grid, glyphs) is exactly what DropTerm's iced
   widget already encapsulates.
8. **The tree is closer to portable than the import counts suggest.**
   Non-test source is ~33.2k lines across `src/`. Already pure (no
   cosmic/iced-toolkit types): `profile.rs` (439 lines), `font_list.rs`,
   `session.rs`, the `tabs.rs` model operations, the spawn-resolution core in
   `app/spawn.rs`. Already cross-platform crates: `alacritty_terminal`
   (`src/terminal/engine.rs:18`) and `cosmic-text` (`src/terminal/engine.rs:37`)
   both build on Windows and macOS today. The leaks into otherwise-pure
   modules are enumerable at line level: `geometry.rs:13-17` (layer-surface
   commands + iced `Task`), `context_menu.rs:14` (`cosmic::widget::menu`),
   `keybinds.rs:17-19` (iced keyboard types + cosmic `KeyBind` — stock iced
   has the keyboard types), `config.rs:186` (`CosmicConfigEntry` derive), and
   the widget halves of `tab_bar.rs`, `status_bar.rs`, and `settings.rs`.
   Everything else COSMIC-shaped is concentrated where it belongs:
   `app.rs`, `surface.rs`, `app/theme_glue.rs`, `app/surface_lifecycle.rs`.

## What DropTerm has today

Grounded in the working tree (v0.2.0 line, ~33.2k non-test lines in `src/`):

- **Composition root**: `src/app.rs` is a `cosmic::Application`
  (`src/app.rs:31`) with libcosmic `Core`, cosmic-config subscription
  (`src/app.rs:1585`), and layer-surface commands imported at
  `src/app.rs:22-31`; `src/main.rs:148` enters through
  `cosmic::app::run_single_instance`.
- **Surface**: `src/surface.rs` builds `SctkLayerSurfaceSettings` surfaces
  (`src/surface.rs:12,62`) with the two-step measuring pass; every reshape
  funnels through one geometry path (`geometry::apply_geometry`).
- **Terminal**: `engine.rs` wraps `alacritty_terminal` + `cosmic-text`;
  `widget.rs` is the iced `Widget` (`src/terminal/widget.rs:455`) with
  autosuggest overlay, mouse reporting, IME, and URL highlight.
- **Config/theme**: the 43-key `DropTermConfig` derives
  `CosmicConfigEntry` (`src/config.rs:186`); themes resolve by name in
  `terminal/theme_source.rs` with the Application-theme pin driving
  light/dark through `app/theme_glue.rs`.
- **D-Bus**: single-instance activation, the V1 state interface
  (`src/dbus_state.rs`), fire-and-forget tab verbs (`src/app/remote.rs`),
  bell notifications (`src/notify.rs`), and the COSMIC panel applet
  (`applet/`) — the applet consumes the lib target's contract.
- **Test posture**: pure modules are headless-tested; compositor behavior is
  verified on COSMIC hardware only (`docs/testing.md`).

## Recommendation

**Decouple in stages, each independently shippable and verifiable; the
COSMIC experience never regresses; the panel applet stays COSMIC-only.**

- **Stage 1 — core extraction (no user-visible change).** Move the engine
  and the model modules into a platform-free core crate (or an
  enforced-internal boundary first): engine, suggest, tabs/session/profile
  models, spawn resolution, status-segment model, keybinds model, config
  *model* (serde types, clamping, defaults) behind a small config-source and
  theme-source trait that the COSMIC frontend implements over cosmic-config
  and cosmic-theme today. Patch the four line-level leaks (finding 8) by
  moving the layer-surface/`Task` half of `geometry.rs` into the frontend
  and swapping `cosmic::widget::menu`/`KeyBind` types for frontend-side
  adapters. Verification is headless: the existing `cargo test` suite moves
  with the code, plus a guard test that the core crate admits no
  `libcosmic`/`cosmic-config`/`cosmic-theme` import.
- **Stage 2 — a stock-iced frontend crate for Windows/macOS/non-COSMIC
  Linux.** Port `TerminalBox` against iced 0.14's advanced widget API
  (re-path imports; swap `cosmic::Theme` styling for iced theme/styling —
  `iced_term` is the reference for the API shape), rebuild tab strip,
  status band, and settings as plain iced widgets, and stage the drop-down
  as an undecorated, always-on-top, skip-taskbar toplevel that
  shows/hides in place (slide animation preserved). The second frontend
  ships behind its own crate in the workspace, exactly the way `applet/`
  already isolates a platform persona.
- **Stage 3 — per-platform adapters.** Single-instance + remote verbs over
  UDS/named pipes on non-Linux (replies become possible — note the upgrade,
  keep the Linux D-Bus contract frozen); bell notifications per OS; theme
  detection per OS; and the Wayland layer-shell variant for the iced
  frontend via `iced_layershell` once the mixed-runtime question (finding 4)
  is de-risked — until then the iced frontend's Wayland story is the
  toplevel shape, with the COSMIC frontend carrying layer-shell duty.
- **Docs**: when stage 2 ships, the README non-goal "Non-COSMIC compositors
  as a priority" flips to a platform statement, and this record's index row
  moves to Shipped.

**Rejected alternatives, spelled out:**

- **Wholesale egui rewrite** — `egui_term` proves feasibility, but the tab
  strip (drag reorder, inline rename, per-tab menus), settings drawer (five
  tabs, write-through contract), status band, and grab bar would all be
  rebuilt in immediate mode from zero; the retained-widget investment — the
  heart of the codebase — is discarded for a styling preference, and egui
  has no layer-shell story at all.
- **Ghostty-style per-native GUIs (AppKit + GTK4)** — two non-Rust toolkits
  and two frontends to maintain, against the "pure Rust UI" goal the
  question names; ghostty funds that shape with a company.
- **Tauri/webview frontends** — a webview hosting a terminal grid is the
  wrong shape (process weight, text-rendering latency), cousin of the
  standing "no wrapping an external emulator" non-goal.
- **Wait for upstream iced to grow layer-shell** — no such plan exists on
  the iced roadmap; the fork exists precisely because it was refused as a
  pace matter. `iced_layershell` already fills the gap for stock iced.
- **Immediate full libcosmic removal** — breaking COSMIC to prove
  portability inverts the priorities; COSMIC remains the flagship and the
  reference frontend. Decoupling must land as addition, not subtraction.

**Standing-record relationships**: extends
[terminal-de-integration](terminal-de-integration.md) (its D-Bus verdicts
carry to Linux unchanged; the portal rejections need no revisit since the
cross-platform seams replace, not wrap, D-Bus); does not supersede
[packaging-and-release](packaging-and-release.md) (per-OS packaging is a
follow-on record once stage 2 exists); leaves
[remote-control-commands](remote-control-commands.md)' fire-and-forget
verdict intact on Linux while noting the reply-capable upgrade the new
seams enable elsewhere.

## Sources

Fetched 2026-10-18 unless noted.

- [iced-rs/iced](https://github.com/iced-rs/iced) — releases page; iced
  0.14.0 (2025-12-07) latest; [crates.io/iced](https://crates.io/crates/iced).
- [pop-os/libcosmic](https://github.com/pop-os/libcosmic) — toolkit repo;
  System76, ["COSMIC DE tiling redesign and libcosmic rebasing"](https://system76.com/blog/post/cosmic-de-tiling-redesign-and-libcosmic-rebasing)
  (2023) on the iced fork; [r/pop_os thread](https://www.reddit.com/r/pop_os/comments/1d4abvo/why_libcosmic_uses_a_fork_of_icedrs).
- [Harzu/iced_term](https://github.com/Harzu/iced_term) /
  [crates.io/iced_term](https://crates.io/crates/iced_term) — 0.8.0
  (2026-03-27), "tested on macOS, Linux, and Windows", MIT.
- [Harzu/egui_term](https://github.com/Harzu/egui_term) — egui terminal
  widget, same backend, same platforms.
- [waycrate/exwlshelleventloop](https://github.com/waycrate/exwlshelleventloop)
  / [crates.io/iced_layershell](https://crates.io/crates/iced_layershell) —
  0.19.1 (2026-07-12); 74 releases since 2024-03;
  [iced_exwlshell](https://docs.rs/iced_exwlshell) sibling crate.
- [wayland.app — wlr-layer-shell-unstable-v1](https://wayland.app/protocols/wlr-layer-shell-unstable-v1).
- [GNOME/mutter#973 — "implement layer_shell protocol"](https://gitlab.gnome.org/GNOME/mutter/-/issues/973);
  [repsac-by/gnome-shell-extension-quake-mode](https://github.com/repsac-by/gnome-shell-extension-quake-mode)
  (unmaintained; points to its maintained fork).
- [GlobalShortcuts portal spec](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html);
  [GNOME/xdg-desktop-portal-gnome#183](https://gitlab.gnome.org/GNOME/xdg-desktop-portal-gnome/-/issues/183);
  [Fedora discussion](https://discussion.fedoraproject.org/t/xdg-global-keybinds-portal-in-gnome/121019);
  [emersion/xdg-desktop-portal-wlr#240](https://github.com/emersion/xdg-desktop-portal-wlr/issues/240);
  [bilelmoussaoui/ashpd](https://github.com/bilelmoussaoui/ashpd).
- [tauri-apps/global-hotkey](https://github.com/tauri-apps/global-hotkey) —
  Windows/macOS/X11, no Wayland.
- [ghostty docs — About](https://ghostty.org/docs/about) (engine/GUI
  separation, GTK4-on-Linux rationale);
  [ghostty#11118 — quick-terminal on Linux](https://github.com/ghostty-org/ghostty/discussions/11118).
- [Phoronix — iced 0.12 multi-window](https://www.phoronix.com/news/Iced-Rust-GUI-Toolkit-v0.12).
- DropTerm working tree @ main (8ec32ac): file:line citations inline above.
