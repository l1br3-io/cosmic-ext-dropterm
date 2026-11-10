# Research: what does DropTerm's portability require beyond the GPUI migration, and in what order?

Date: 2026-11-09
Question: *The goal is "make this work in other distros" — eventually other desktops
and other operating systems. What is actually blocked, what adapters does the
whole surface need, is cross-platform even possible, and what order should the
work land in?*

Supersedes nothing; extends [gpui-migration](gpui-migration.md) (same day,
owner brainstorm session) with the platform axis that record only sketched in
its findings 9–10. Its corrections to that record are finding 5 below.

## TL;DR

**Yes, cross-platform is possible — GPUI is available and proven on macOS and
Windows — but "portable" hides three goals with wildly different costs, and the
plan is tiered, Wayland-first: all platforms in scope (owner ruling,
2026-11-09), layer-shell Wayland as the only per-release-tested primary, X11 /
GNOME / macOS as milestone-tested secondary, and Windows as builds-only until
test hardware exists.** GPUI's availability condition is confirmed but narrow:
**it exists only as a git pin** — the zed workspace sets `publish = false` for
every member at the surveyed rev, so crates.io `gpui` (frozen at 0.2.2,
2025-10-22) is permanently stale and the pin to zed `main` is the product
(house pattern: same as libcosmic and the alacritty forks).

Two discoveries reshape the effort. First, **most of "other distros" is already
shipped or merely unverified**: the packaging record already routes native
deb/AUR/nix, and no COSMIC session detection exists anywhere in the tree — the
open question is only whether today's binary runs on sway/Hyprland/KDE at all,
which Phase 0 (the checklist at the bottom) answers with hardware sessions and
no code. Second, **the per-show destroy/create lifecycle GPUI's create-only
layer-shell feared is already DropTerm's architecture** — every show creates a
measuring surface, every hide destroys it, hidden means no surface — and the
same-day source check shrank the spike's crux again: resize and exclusive
zone/edge have public runtime APIs at the surveyed rev (PR #60163), so the crux
is keyboard-interactivity re-grabs mid-slide (the one create-only mutation with
an open, unmerged upstream PR), compositor behavior under the 80 ms drag
reshape, and live layer changes (finding 4).

The adapter layer grows from gpui-migration's two trait seams to six (finding 6)
— `SurfaceBackend`, `Hotkey`, `ControlTransport`, `ProcessInfo`, clipboard,
`ThemeSource` — all still compile-time. The runtime-plugin rejection is
re-checked and holds, strengthened by cross-platform (finding 10). Greenfield
status (no users) makes the identity rename to plain `dropterm` a clean break
with no migration shims (finding 11).

## What DropTerm has today

Grounding, in the working tree at this record's date, beyond what
[gpui-migration](gpui-migration.md) already inventoried — the Linux-bound code
a cross-platform build would have to seam:

- **`/proc` reads**: `/proc/{pid}/comm` (`src/terminal/engine.rs:430`),
  `/proc/{pid}/cmdline` (`src/terminal/engine.rs:483`), the live cwd
  `read_link("/proc/{shell_pid}/cwd")` (`src/terminal/engine.rs:1706`,
  documented at `src/terminal/suggest.rs:105,136`), and the hostname
  `/proc/sys/kernel/hostname` (`src/app.rs:1985`).
- **`libc` process-group calls**: `tcgetpgrp`/`getpgid` on the PTY master
  (`src/terminal/engine.rs:378-379,411-412`) — POSIX-portable (macOS yes,
  Windows no).
- **Wayland-only clipboard**: `wl-clipboard-rs` data-control owns all terminal
  clipboard I/O (`src/clipboard.rs:28-29` onward).
- **D-Bus-only IPC**: zbus carries single-instance forwarding, `--is-visible`,
  and the V1 contract (`src/main.rs:88`, `src/dbus_state.rs`).
- **Notifications**: `notify-rust` 4.18 (`src/notify.rs:44,91`) — direct
  `org.freedesktop.Notifications` on Linux, but the crate itself has macOS
  (`mac-notification-sys`) and Windows (`winrt-notification`) backends.
- **Identity**: `APP_ID` = `io.github.l1br3_io.CosmicExtDropterm`
  (`src/lib.rs:37`) behind the bus name, config namespace, and state dir.
- **COSMIC-flavored defaults**: theme scheme names `COSMIC Dark/Light`
  (`src/config.rs:33-36`) and font `Noto Sans Mono` (`src/config.rs:587`).
- **The per-show surface lifecycle** (finding 4's evidence): `show()` always
  creates a fresh measuring layer surface (`src/app/surface_lifecycle.rs:243-255`),
  `hide()` destroys it — immediately for a measuring surface
  (`:284-290`), after the slide-out otherwise — and `Hidden` carries no surface
  at all; the clipboard anchor is the only never-destroyed surface
  (`src/app.rs:1976,2024`). The churn is a known issue of its own
  ([known issues](../../known-issues.md), the modifier-state desync).
- **Packaging**: already native-first — deb/AUR/nix, with Flathub ruled out for
  the main binary while cosmic-comp binds layer-shell only for unsandboxed
  clients ([packaging-and-release](packaging-and-release.md)).

## Findings

### 1. "Portable" is three goals, and only one is expensive

| Goal | What actually blocks it | Cost |
|---|---|---|
| Other distros (Arch, Fedora, Debian) | Mostly packaging — already shipped native-first; which *desktop* you run matters far more than which distro | small |
| Other Linux desktops | The window protocol: layer-shell (sway, Hyprland, KDE, Wayfire, labwc, COSMIC), GNOME (no layer-shell), X11 (Cinnamon, Xfce, MATE, i3) | medium, per protocol |
| Other operating systems | Everything below the UI: PTY spawn, process introspection, clipboard, IPC, global hotkey, paths, notifications | large, and a standing per-release cost |

No COSMIC session detection exists anywhere in the tree (verified in
[gpui-migration](gpui-migration.md) finding 9), so nothing *detects* its away
off COSMIC — the question is what breaks passively. That is exactly what the
Phase 0 checklist measures.

### 2. GPUI is available — as a git pin, permanently — and its platform layer just moved

Surveyed at zed `main` rev `ec237216460f8a34a4ac7c6b068e838593976ede`
(2026-11-09):

- The workspace root sets `publish = false` under `[workspace.package]`, and
  crates.io's `gpui` max version remains 0.2.2 (released 2025-10-22) — a year
  stale, predating the mid-2026 layer-shell and output-selection fixes. **The
  git pin is the product.** Zed owes external consumers nothing; treating
  upstream-churn risk as an accepted, watched cost is part of choosing GPUI.
- The platform layer was restructured into per-OS crates: `gpui_linux` 0.1.0
  (features `wayland` and `x11`, separate), `gpui_macos`, `gpui_windows`, and
  the `gpui_platform` aggregator that forwards `wayland`/`x11`. The spike's pin
  must therefore cover the `gpui_*` crate set, not `gpui` alone.
- `cosmic-text` moved into `gpui_wgpu` (`cosmic-text = "0.19.0"`) — the shared
  renderer — so [gpui-migration](gpui-migration.md) finding 6 (GPUI keeps a
  pop-os crate) still holds, relocated.
- `WindowKind` in `crates/gpui/src/platform.rs`: `Normal`, `PopUp` ("appears
  above all other windows"), `AnchoredPopup`, `Floating`, `Dialog`, and
  `LayerShell(..)` gated `cfg(all(target_os = "linux", feature = "wayland"))`.
  `PopUp` is the candidate drop-down surface everywhere layer-shell does not
  exist — macOS, Windows, and plausibly X11. Unverified as a hotkey-window
  substitute (assumption 3).

Consequences: "GPUI if available" resolves to *yes, pin zed `main`*, and the
fallback map tightens — if the layer-shell spike NO-GOs, `iced_layershell`
covers Linux-Wayland only and every other platform would need plain floating
windows anyway, which weakens it relative to a GPUI fork/patch carried like the
existing vte/alacritty forks.

A name disambiguation, so no future reader trips on it: **"GPUI" throughout
this record and [gpui-migration](gpui-migration.md) means Zed's core framework
(`crates/gpui`, gpui.rs) — not [gpui-kit](https://gpui-kit.com), Longbridge's
component ecosystem built *on top of* it** (`gpui-kit` v0.9.0 on crates.io:
`gpui-component` styled widgets, `gpui-base` primitives, `gpui-shell`
JavaScript extensions). The Kit is not part of any decision here. If a later
phase adopts it as an accelerator for the rewrite's chrome (settings forms,
buttons, menus — the terminal surface itself is custom rendering regardless),
two rules travel with it: its own GPUI git pin must be verified against (or
patched to) a rev that passes spike checks 0–4, and its JavaScript extension
layer is the runtime-plugin pattern finding 10 rejects.

### 3. Cross-OS is technically plausible — GPUI, the engine, and even the notifications crate multi-target

- GPUI itself: Zed ships macOS, Windows, Linux/Wayland/X11 from one codebase.
- `alacritty_terminal`: cross-platform (ConPTY on Windows, openpty on
  macOS/Linux).
- `notify-rust`: macOS and Windows backends exist (README, surveyed 2026-11-09)
  — the bell-notification path may port nearly free, unverified.
- The heavy remainder is the grounding inventory above: `/proc`, `tcgetpgrp`,
  `wl-clipboard-rs`, zbus, plus the global hotkey (an external binding on
  Linux, but an in-app expectation on macOS/Windows) and PowerShell shell
  integration for Windows.

### 4. The per-show destroy/create "fallback" is already the architecture — the GPUI crux narrows

[gpui-migration](gpui-migration.md) finding 3 frames the lifecycle as five
runtime operations on *one* live surface. Reading the code, the surface is not
long-lived at all: `show()` always creates a fresh measuring surface
(`src/app/surface_lifecycle.rs:243-255`), `hide()` always destroys it
(`:284-290` after the slide-out; `src/app.rs:3681,3691`), and `Hidden` holds no
surface — the churn is already a documented known issue. What genuinely needs
runtime mutation on a live surface is narrower:

1. **Keyboard interactivity**: release at slide-out *start* so the previous
   window regains focus (`src/app/surface_lifecycle.rs:331`), re-grab on a
   mid-slide reversal (`:403`) — the one true crux: no public path at the
   surveyed rev, `Window::set_keyboard_interactivity` proposed in unmerged PR
   #61420; spike check 3.
2. **In-place reshape during grab-bar drag**: the 80 ms cadence resize storm
   within one show (`src/geometry.rs`, `DRAG_APPLY_INTERVAL`) — the API exists
   (`Window::resize` → `layer_surface.set_size`); what the spike tests is
   compositor behavior at that cadence (check 2).
3. **Layer / exclusive-zone on live config change**
   (`src/app.rs:3091-3099`) — split by the source check: zone and edge are
   live-mutable (`Window::set_exclusive_zone`/`set_exclusive_edge`, PR #60163);
   only the layer change stays create-only, degradable to next-show
   application.

This lowers the GPUI risk profile again: what finding 3's framing called a
create-only options API is, at the surveyed rev, two-thirds shipped; the
residue is one unmerged keyboard mutation (fork material, like the existing
vte/alacritty forks) plus hardware questions about cadence behavior, and the
state machine in `surface_lifecycle.rs` survives any executor behind a
`SurfaceOp` vocabulary (roadmap Phase 1).

### 5. Corrections to [gpui-migration](gpui-migration.md) (immutable; corrected here)

1. Finding 9's claim that option A (keep libcosmic) "cannot use the
   theme/config replacements" is overstated: nothing forces `cosmic-config` for
   persistence or the cosmic theme store for lookups inside a libcosmic app —
   those replacements are toolkit-neutral work available to every option.
2. Its adapter table predates the cross-platform question: it misses the X11
   clipboard path, `ProcessInfo`, `Hotkey`, and `ControlTransport` (finding 6).
3. Finding 3's framing (see finding 4 above).
4. Its GPUI facts predate the platform-crate split and the `publish = false`
   workspace state (finding 2 above); `layer_shell.rs` now lives at
   `crates/gpui/src/platform/layer_shell.rs` in the per-OS layout.

### 6. The all-platform adapter inventory — six seams, all compile-time

| Adapter | Linux Wayland | Linux X11 | macOS | Windows | Code anchor today |
|---|---|---|---|---|---|
| `SurfaceBackend` | layer-shell | `PopUp`/topmost (check 12) | `PopUp` (check 13) | `PopUp` | `src/surface.rs`, `src/app/surface_lifecycle.rs` |
| Clipboard | data-control (keep) | X11 path or toolkit clipboard | GPUI clipboard | GPUI clipboard | `src/clipboard.rs` |
| `ThemeSource` | XDG portal `color-scheme` | portal or toolkit | GPUI `WindowAppearance` likely | same | `src/app/theme_glue.rs`, `src/config.rs:33-36` |
| `Hotkey` | external binding / GlobalShortcuts portal (COSMIC has none today, `README.md:58`) / X11 grab | same | native hotkey API | `RegisterHotKey` | none in code — the summon is external |
| `ControlTransport` | D-Bus (keep) | D-Bus | local socket | named pipe | `src/dbus_state.rs`, `src/main.rs:88` |
| `ProcessInfo` | `/proc` + `tcgetpgrp` | same | macOS API, unnamed | Toolhelp-class API, unnamed | `src/terminal/engine.rs:378-430`, `:483`, `:1706`, `src/app.rs:1985` |
| Config store | replace with XDG ron (`src/session.rs` pattern) — no trait | same | same | same | `src/settings.rs` |
| Paths/hostname | plain per-OS functions, no trait | | | | `src/app.rs:1985,1997` |
| Panel presence | `applet/` stays COSMIC-only; GNOME Shell extension / KDE widget / swaybar module are out-of-core programs over the contract | | n/a | n/a | `applet/` |

**`ControlTransport` is the structural change**: the V1 contract becomes *the*
API and D-Bus its Linux binding, so the CLI (`dropterm toggle`, `--is-visible`)
behaves identically everywhere and the ecosystem surface (the applet's proven
pattern) stops being Linux-bound by construction.

### 7. The product case for cross-OS is weak; the tiering follows from it

- **Windows ships a drop-down terminal free**: Windows Terminal's `quakeMode`
  action toggles a quake-style summon window with monitor/desktop targeting
  (verified, learn.microsoft.com, 2026-11-09).
- **macOS ships iTerm2**: "dedicated hotkey window … a system-wide terminal
  window that you can open with a hotkey", multiple hotkeys, double-tap
  modifiers (verified, iterm2.com, 2026-11-09).
- **Linux is where DropTerm's combination is scarce**: a true layer-shell
  drop-down with the engine depth DropTerm carries.

Plus the standing cost: every platform tier adds a manual hardware pass
([testing](../../testing.md) is already manual-only), and the owner's test matrix
is COSMIC + Linux VMs + one Mac (ruling, 2026-11-09). Hence the tiers in the
recommendation, not uniform support.

### 8. GNOME is the one out-of-process backend — and that is contract, not plugin

GNOME has no layer-shell; a Wayland client cannot self-position a toplevel.
The route is a small GNOME Shell extension (JavaScript) positioning DropTerm's
normal window over the V1/`ControlTransport` contract — ddterm's whole-app
existence as a Shell extension is the existence proof. This is the
subprocess-plus-contract pattern the applet already uses.

### 9. X11 is cheap reach and easy to forget

Cinnamon/Mint, Xfce, MATE, and i3 sessions are large user bases; a drop-down on
X11 is decades-old practice; GPUI has a first-class `x11` feature. The X11 leg
needs its own `SurfaceBackend` impl, clipboard path, and a hotkey answer
(grab or portal), and one spike check (12).

### 10. The plugin rejection, re-checked under cross-platform — holds, strengthened

OS backends are selected at compile time by target triple; a runtime loader
could never carry a macOS backend into a Linux binary, so the plugin idea fails
the one job it might have had. The Rust-ABI, security, and audience arguments
from [gpui-migration](gpui-migration.md) finding 10 all stand. The refined
rule: **traits inside the binary, contracts outside it** — `SurfaceBackend` &
 friends in a `dropterm-platform` crate, the V1 wire contract as the
language-agnostic porting surface (GNOME extension in JS, KDE widget in QML,
swaybar module in shell).

### 11. Greenfield simplifies everything compatibility-shaped (owner ruling, 2026-11-09)

No users exist yet, so: the identity rename to plain `dropterm`
(`io.github.l1br3_io.CosmicExtDropterm` → a neutral app id, `src/lib.rs:37`)
is a clean break — no dual bus-name window, no config-directory migration; the
rename lands in Phase 1 together with the config-store move so even the
one-time break is a single event. Backward-compat shims are rejected.

## The roadmap (the record's plan)

Each phase gates the next; 0 and 1 pay off even if the GPUI spike NO-GOs.

- **Phase 0 — reality check (hardware sessions, no code).** The checklist
  below. Gate: a per-session result table; sway/Hyprland/KDE green on the core
  checks ⇒ "other distros" collapses into packaging/docs; failures become the
  Phase 1 bug list.
- **Phase 1 — toolkit-neutral work.** Identity rename; XDG-ron config store
  (`src/session.rs` pattern); the `SurfaceOp` vocabulary — `surface_lifecycle`
  emits toolkit-neutral surface intents, the libcosmic executor translates, the
  state machine becomes headless-testable and the GPUI port becomes an executor
  swap; per-DE keybind docs matrix; integrator-facing V1 contract docs;
  packaging polish (nix/AUR verification on non-Pop systems).
- **Phase 2 — GPUI spike** ([gpui-migration](gpui-migration.md) checks 0–11,
  plus): **check 12** — X11 `PopUp` window as a drop-down (topmost, positioned,
  focus-grabbing); **check 13** — macOS `PopUp` over a fullscreen app and
  across Spaces, plus a native global-hotkey registration. The pin covers the
  `gpui_*` crate set at one rev with a committed lock.
- **Phase 3 — GPUI rewrite, Wayland layer-shell** (the spike's GO gates this).
- **Phase 4 — Linux reach:** X11 backend; GNOME companion extension over the
  contract.
- **Phase 5 — maybe:** macOS (testable on the owner's Mac), then Windows —
  which stays builds-only in CI until test hardware exists, per the tier table.

**Rejected alternatives**: GTK4 anywhere (largest rewrite, weak macOS/Windows
story — carries gpui-migration option D); `iced_layershell` as *the* framework
(Linux-Wayland only — demoted to the Linux fallback if the spike NO-GOs);
uniform all-OS support (no test hardware, free incumbents on both OSes);
runtime plugins (finding 10); keeping the `CosmicExt` identity (finding 11);
compatibility shims (finding 11).

## Phase 0 — the reality-check checklist (manual, hardware sessions)

Sessions to run (VMs acceptable except where noted), same release build
(`just build`), `RUST_LOG=debug`, logs captured per session; record distro +
compositor + version + session type:

1. **COSMIC** — the known-good baseline; every check below should pass here.
2. **sway** (wlroots witness) and **Hyprland**.
3. **KDE Plasma (Wayland)**.
4. **GNOME (Wayland)** — *expected to fail* (no layer-shell); the deliverable
   is *how* it fails (clean error, hang, invisible surface) to inform the
   companion-extension design.
5. **One X11 session** (Xfce, Cinnamon, or i3 — record which).
6. **One non-Pop distro** (Arch or Fedora) under any session above — the distro
   axis: `ldd` the binary, check `Noto Sans Mono` presence (the default font,
   `src/config.rs:587`).

Per-session checks (core = 1–7 and 12–13):

- [ ] 1. **Launch**: `dropterm` from a terminal — a surface maps, anchored at
      the configured edge, expected width; capture any panic/warn
- [ ] 2. **Keyboard on show**: typing lands without a click first (the
      map-time keyboard-interactivity grab)
- [ ] 3. **Summon bind**: bind `dropterm toggle` in the session's own keybind
      mechanism (sway `bindsym`, Hyprland `bind`, KDE custom shortcut, Xfce
      keyboard settings) — hides and re-shows
- [ ] 4. **Rapid double-toggles**: no crash, no duplicate surface
- [ ] 5. **`hide_on_focus_loss`**: clicking another app hides
- [ ] 6. **Grab-bar drag-resize**: resizes live; the size persists across a
      hide/show
- [ ] 7. **Above fullscreen**: with a fullscreen window, the toggle lands on
      top
- [ ] 8. **Theme fallback**: default config outside COSMIC — what renders when
      `COSMIC Dark/Light` (`src/config.rs:33-36`) resolves to nothing
- [ ] 9. **Config write**: change a setting in the drawer; the file lands and
      survives a restart
- [ ] 10. **Clipboard**: copy in DropTerm → paste elsewhere; middle-click
       primary paste
- [ ] 11. **Bell notification**: `printf '\a'` while hidden with a daemon
       running (mako/dunst/swaync per session) — exactly one notification
- [ ] 12. **CLI**: `dropterm toggle`, `--is-visible` (exit codes), `new-tab`
       all work over the session bus
- [ ] 13. **Second-launch handoff**: a second `dropterm` forwards and exits
       (single instance holds)
- [ ] 14. **Multi-monitor**: which output gets the surface; does it follow
       focus (record semantics per compositor)
- [ ] 15. **Session restore**: tabs and geometry come back after a restart

**Recording template** (one block per session): date, distro, compositor +
version, Wayland/X11, result per check (`pass`/`fail`/`n-a` + one line), log
excerpt for every fail. GNOME and X11 failures are *expected findings*, not
score against the gate.

## Assumptions to validate (the plan's kill risks)

1. **Today's binary runs on sway/Hyprland/KDE** — Phase 0 tests it; if it does
   not, the bug list sizes Phase 1 before any toolkit work.
2. **GPUI's layer-shell can do the live mutations of finding 4** — resize and
   zone/edge have public APIs at the surveyed rev (the spike verifies hardware
   behavior, checks 2 and 4); keyboard-interactivity is the open gate (check 3,
   PR #61420); the fallback map is finding 2's consequence.
3. **`PopUp` works as a hotkey-window substitute** (over fullscreen, across
   Spaces on macOS; topmost + positioned on X11) — spike checks 12–13.
4. **GPUI stays consumable outside Zed under `publish = false`** — not
   pre-verifiable; watched per pin bump, with fork-and-patch as the sanctioned
   escape (house precedent: the vte/alacritty forks).

## Unverified / open questions

- GNOME positioning mechanics for the companion extension (extension-driven
  move vs. any protocol path) — design deferred past Phase 0/spike.
- `notify-rust`'s macOS/Windows backends in practice for the bell path —
  verify when Phase 5 opens.
- The macOS/Windows `ProcessInfo` implementations are now named: macOS libproc
  (`proc_pidpath`/`proc_pidinfo`, `libproc.h` — Apple publishes no online man
  page, so the canonical citation is the on-SDK `man 3` page); Windows
  `CreateToolhelp32Snapshot` + `Process32First`/`Process32Next` for enumeration
  and `QueryFullProcessImageName` for the image path (MS Learn) — noting that
  no documented Win32 API returns another process's command line (the
  `/proc/pid/cmdline` equivalent needs WMI `Win32_Process` or undocumented
  `NtQueryInformationProcess`); the seam exists regardless (finding 6).
- Whether GPUI's X11 backend can position/focus a `PopUp` at all — check 12.
- KDE's layer-shell quirks (it implements the protocol, but with its own
  focus/zone behavior) — Phase 0 session 3 is the first data point.

## Sources

Surveyed 2026-11-09.

- **zed @ `ec237216460f8a34a4ac7c6b068e838593976ede`** (main, 2026-11-09):
  root `Cargo.toml` (`[workspace.package] publish = false`);
  `crates/gpui_linux/Cargo.toml` (features `wayland`/`x11`);
  `crates/gpui_platform/Cargo.toml` (aggregator); `crates/gpui_wgpu/Cargo.toml`
  (`cosmic-text = "0.19.0"`); `crates/gpui/src/platform.rs` (`WindowKind`:
  `PopUp`, `LayerShell` gated linux+wayland);
  `crates/gpui/src/platform/layer_shell.rs` —
  <https://github.com/zed-industries/zed/tree/main/crates/gpui>.
- **Zed PR #60163** (merged 2026-07-15; the runtime
  `Window::set_exclusive_zone`/`set_exclusive_edge`), **PR #61420** (open,
  2026-07-21; proposed `Window::set_keyboard_interactivity`), **PR #50520**
  (merged 2026-03-08; layer-shell display targeting; the earlier #48502 was
  closed unmerged); `crates/gpui/src/window.rs` and
  `crates/gpui_linux/src/linux/wayland/{window,client}.rs` at the rev above —
  <https://github.com/zed-industries/zed/pull/60163>,
  <https://github.com/zed-industries/zed/pull/61420>,
  <https://github.com/zed-industries/zed/pull/50520>.
- **ProcessInfo APIs**: `CreateToolhelp32Snapshot` and
  `QueryFullProcessImageName` (MS Learn) —
  <https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-createtoolhelp32snapshot>,
  <https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-queryfullprocessimagenamea>;
  macOS libproc per the on-SDK `man 3 proc_pidpath`/`proc_pidinfo` (no online
  Apple page exists — 404s verified 2026-11-09).
- **gpui @ crates.io**: max version 0.2.2 (2025-10-22), crate metadata updated
  2026-10-30 — <https://crates.io/crates/gpui>.
- **gpui-kit** @ <https://gpui-kit.com> (Longbridge, surveyed 2026-11-09): the
  component ecosystem built on GPUI — cited for disambiguation only, not a
  dependency of any decision here.
- **Windows Terminal quake mode**: `quakeMode`/`globalSummon` actions docs —
  <https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions>.
- **iTerm2 hotkey window**: "dedicated hotkey window" —
  <https://iterm2.com/documentation-hotkey.html>.
- **notify-rust** README @ main: macOS `mac-notification-sys`, Windows
  `winrt-notification` backends — <https://github.com/hoodie/notify-rust>.
- **DropTerm working tree** (this repo, 2026-11-09):
  `src/terminal/engine.rs`, `src/terminal/suggest.rs`, `src/clipboard.rs`,
  `src/notify.rs`, `src/surface.rs`, `src/geometry.rs`,
  `src/app/surface_lifecycle.rs`, `src/app.rs`, `src/main.rs`, `src/lib.rs`,
  `src/config.rs`, `src/settings.rs`, `README.md`,
  `docs/known-issues.md`, plus the two records cited above.
- **Owner rulings (2026-11-09)**: all platforms in scope with Wayland the
  priority; test hardware = COSMIC + Linux sessions/VMs + one Mac (Windows
  later); identity rename to plain `dropterm` approved; greenfield — no
  backward compatibility; reality check before the spike.
