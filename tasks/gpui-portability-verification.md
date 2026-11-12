# Verification: external-source claims in gpui-migration.md and portability-roadmap.md

Date fetched: 2026-11-09. Pinned zed rev: `ec237216460f8a34a4ac7c6b068e838593976ede`
(verified on main: commit dated 2026-11-09T09:41:13Z, "agent_ui: Render ACP config
options right to left in received order (#66733)" — api.github.com/repos/zed-industries/zed/commits/ec237216460f8a34a4ac7c6b068e838593976ede).

Primary sources only: raw.githubusercontent.com at the pinned rev, api.github.com,
crates.io API, learn.microsoft.com, iterm2.com, and the named crates' GitHub repos.

## Verdicts

### 1. Root Cargo.toml `[workspace.package]` sets `publish = false` — CONFIRMED
Verbatim: `[workspace.package]` / `publish = false` / `edition = "2024"`.
Citation: https://raw.githubusercontent.com/zed-industries/zed/ec237216460f8a34a4ac7c6b068e838593976ede/Cargo.toml

### 2. Platform layer split into per-OS crates — CONFIRMED
`gpui_linux` 0.1.0 defines separate features `wayland = [...]` and `x11 = [...]`
(`default = ["wayland", "x11"]`); `gpui_macos` and `gpui_windows` exist; `gpui_platform`
features forward: `wayland = ["gpui_linux/wayland"]`, `x11 = ["gpui_linux/x11"]`.
Each sets `publish.workspace = true`, inheriting the workspace `publish = false`.
Citations: https://raw.githubusercontent.com/zed-industries/zed/ec237216460f8a34a4ac7c6b068e838593976ede/crates/gpui_linux/Cargo.toml ;
.../crates/gpui_platform/Cargo.toml ; .../crates/gpui_macos/Cargo.toml ; .../crates/gpui_windows/Cargo.toml

### 3. gpui_wgpu depends on cosmic-text 0.19.0 — CONFIRMED
Verbatim `[dependencies]` line: `cosmic-text = "0.19.0"`.
Citation: https://raw.githubusercontent.com/zed-industries/zed/ec237216460f8a34a4ac7c6b068e838593976ede/crates/gpui_wgpu/Cargo.toml

### 4. WindowKind variants — CONFIRMED (listing order differs, no factual error)
`crates/gpui/src/platform.rs` defines `Normal`, `PopUp`, `AnchoredPopup(popup::PopupOptions)`,
`Floating`, `Dialog`, and `LayerShell(layer_shell::LayerShellOptions)` gated
`#[cfg(all(target_os = "linux", feature = "wayland"))]`. Source order places
`LayerShell` between `Floating` and `Dialog`; the roadmap lists Dialog first —
same variant set, cosmetic only.
Citation: https://raw.githubusercontent.com/zed-industries/zed/ec237216460f8a34a4ac7c6b068e838593976ede/crates/gpui/src/platform.rs

### 5. Layer-shell types at crates/gpui/src/platform/layer_shell.rs — CONFIRMED
platform.rs: `#[cfg(all(target_os = "linux", feature = "wayland"))] pub mod layer_shell;`;
the file exists at that path. (A second, crate-internal mapping module also exists at
`crates/gpui_linux/src/linux/wayland/layer_shell.rs`.)
Citation: https://raw.githubusercontent.com/zed-industries/zed/ec237216460f8a34a4ac7c6b068e838593976ede/crates/gpui/src/platform/layer_shell.rs

### 6. LayerShellOptions fields and enum vocabulary — CONFIRMED
Fields verbatim: `namespace: String`, `layer: Layer`, `anchor: Anchor`,
`exclusive_zone: Option<Pixels>`, `exclusive_edge: Option<Anchor>`,
`margin: Option<(Pixels, Pixels, Pixels, Pixels)>` ("Specified in CSS order: top,
right, bottom, left"), `keyboard_interactivity: KeyboardInteractivity`.
Enums: `Layer { Background, Bottom, Top, Overlay }`, `Anchor` bitflags
`{ TOP, BOTTOM, LEFT, RIGHT }`, `KeyboardInteractivity { None, Exclusive, OnDemand }`
— 1:1 with zwlr_layer_surface_v1 (layers, anchor bitmask, keyboard_interactivity).
Citation: same layer_shell.rs URL as claim 5.

### 7. The "create-only" claim — PARTIALLY CONTRADICTED
At this rev the record's framing ("options are defined at window creation and have
no public setters yet") is stale for three of the five operations:

- EXISTS as public runtime mutation, all on `Window` (crates/gpui/src/window.rs):
  - `pub fn resize(&mut self, size: Size<Pixels>)` — on Wayland dispatches to
    `PlatformWindow::resize` → `WaylandSurfaceState::set_geometry`, whose LayerShell
    arm is `layer_surface.set_size(width as u32, height as u32)` (no explicit commit;
    applied on the next frame's commit).
  - `pub fn set_exclusive_zone(&self, zone: Pixels)` and
    `#[cfg(all(target_os = "linux", feature = "wayland"))] pub fn set_exclusive_edge(&self, edge: crate::layer_shell::Anchor)`
    — the Wayland impl calls `layer_surface.set_exclusive_zone(...)` /
    `apply_exclusive_edge(...)` then `state.surface.commit()` immediately.
    These landed in PR #60163 (merged 2026-07-15), i.e. before the pin date.
- STILL CREATE-ONLY: anchor, margin, layer, keyboard_interactivity. No function
  named set_keyboard_interactivity / set_anchor / set_margin / set_layer exists in
  crates/gpui_linux/src/linux/wayland/{window.rs,client.rs} nor in window.rs; the
  only creation-time block is in `WaylandSurfaceState::new`
  (`layer_surface.set_size/set_anchor/set_keyboard_interactivity/set_margin/set_exclusive_zone/set_exclusive_edge`).
  PR #61420 (open, not merged, 2026-07-21) exists to add exactly this, and states:
  "Layer-shell keyboard interactivity can currently only be selected when a GPUI
  window is created."
- The `PlatformWindow` trait defaults (`fn set_exclusive_zone(&self, _zone: Pixels) {}`,
  `fn set_exclusive_edge(&self, _edge: layer_shell::Anchor) {}`) are no-ops, but
  `Window` publicly wraps them and the Wayland backend implements them — the record's
  "not the clean runtime mutation" understates what is available.

Citations: https://raw.githubusercontent.com/zed-industries/zed/ec237216460f8a34a4ac7c6b068e838593976ede/crates/gpui/src/window.rs ;
.../crates/gpui_linux/src/linux/wayland/window.rs ; https://api.github.com/repos/zed-industries/zed/pulls/60163 ; https://api.github.com/repos/zed-industries/zed/pulls/61420

### 8. Output-selection fix present at this rev — CONFIRMED IN SUBSTANCE; the record's citation is WRONG
- PR #48502 ("gpui: Allow targeting output for layer shell windows on Wayland")
  was **closed unmerged**: created 2026-02-05, `merged_at: null`, closed 2026-02-24.
- The fix that actually landed is PR #50520 "gpui(linux): Pass display_id to layer
  shell get_layer_surface", **merged 2026-03-08**: `open_window` derives
  `target_output` from `params.display_id` matched against bound wl_outputs
  (`let target_output = params.display_id.and_then(|display_id| ... )`).
- `cx.displays()` IS populated at startup at this rev: the Wayland client's `attach`
  binds outputs from the registry and roundtrips, with the comment "Receive the
  outputs' descriptions now, so displays are known before the app's first windows
  open." Issue #46378 remains open only for further hardening (open PR #61578
  "Enforce Wayland output discovery during application startup", 2026-07-24).
- The 2026-05-08 comment on #46378 says verbatim: "layer-shell output selection was
  fixed!" — referring to the landed display-targeting work, not to #48502.

Citations: https://api.github.com/repos/zed-industries/zed/pulls/48502 ; https://api.github.com/repos/zed-industries/zed/pulls/50520 ; https://raw.githubusercontent.com/zed-industries/zed/ec237216460f8a34a4ac7c6b068e838593976ede/crates/gpui_linux/src/linux/wayland/client.rs ; https://api.github.com/repos/zed-industries/zed/issues/46378/comments

### 9. PR #32651 merged 2025-07-29, reverted by PR #35331 — CONFIRMED
#32651 "gpui: Implement support for wlr layer shell": `merged_at`
2025-07-29T21:26:30Z, merge commit `c110f7801516a1948ade4a51213f1fc8ea7f8efc`.
#35331 "Revert \"gpui: Implement support for wlr layer shell (#32651)\"": merged
2025-07-30T13:36:22Z. Contradicting-evidence quote (the crash), verbatim from the
revert body: "On Linux Wayland, that causes a panic:" followed by
"already mutably borrowed: BorrowError". The body does not literally say "on
launch"; it is a Linux-Wayland panic — the record's "crashed Wayland on launch"
is consistent but is the record's characterization.
Citations: https://api.github.com/repos/zed-industries/zed/pulls/32651 ; https://api.github.com/repos/zed-industries/zed/pulls/35331

### 10. PR #35610 re-land, merged 2025-10-29 — CONFIRMED
`merged_at` 2025-10-29T15:32:02Z; `merge_commit_sha`
`b92664c52d7c49bb49acd06d2d40eb23b4e8c601` (exact match with the record). Body
verbatim: "it allows setting options for the created surface, restricts the enum
variant to the `wayland` feature, and adds an example that renders a clock widget
using the protocol." The example file exists at the pinned rev
(crates/gpui/examples/layer_shell.rs) and sets everything at creation, with no
post-creation mutation calls.
Citations: https://api.github.com/repos/zed-industries/zed/pulls/35610 ; https://raw.githubusercontent.com/zed-industries/zed/ec237216460f8a34a4ac7c6b068e838593976ede/crates/gpui/examples/layer_shell.rs

### 11. crates.io gpui: 0.2.2 / 2025-10-22 / Apache-2.0 / updated 2026-10-30 — CONFIRMED
API: `max_version`/`max_stable_version`/`newest_version` = "0.2.2"; version 0.2.2
`created_at` 2025-10-22T03:56:29Z; `license` "Apache-2.0"; crate `updated_at`
2026-10-30T17:21:27Z.
Citation: https://crates.io/api/v1/crates/gpui

### 12. iced 0.14.0 wayland feature is the winit backend; no layer-shell surface type — CONFIRMED
iced 0.14.0 published 2025-12-07. Feature verbatim (root Cargo.toml):
`wayland = ["iced_renderer/wayland", "iced_winit/wayland"]`; iced_winit's Cargo.toml:
`wayland = ["winit/wayland", "arboard/wayland-data-control"]`. iced_core's window
module (the public window API surface) has no occurrences of "layer", "shell", or
"zwlr" (its types: Settings, Id, Position, Mode, RedrawRequest, etc.).
Citations: https://crates.io/api/v1/crates/iced ; https://raw.githubusercontent.com/iced-rs/iced/master/Cargo.toml ; https://raw.githubusercontent.com/iced-rs/iced/master/winit/Cargo.toml ; https://raw.githubusercontent.com/iced-rs/iced/master/core/src/window.rs

### 13. iced_layershell 0.19.1 (2026-07-12, waycrate) — CONFIRMED
`max_version` "0.19.1", created 2026-07-12T09:14:36Z, repository
https://github.com/waycrate/exwlshelleventloop, license MIT.
Citation: https://crates.io/api/v1/crates/iced_layershell

### 14. smithay-client-toolkit 0.21.1 — CONFIRMED
`max_version`/`newest_version` "0.21.1", created 2026-07-23, license MIT.
Citation: https://crates.io/api/v1/crates/smithay-client-toolkit

### 15. gtk4-layer-shell 0.8.1 — CONFIRMED
`max_version` "0.8.1", created 2026-08-10, license MIT, repository
https://github.com/pentamassiv/gtk4-layer-shell-gir/tree/main/gtk4-layer-shell.
Citation: https://crates.io/api/v1/crates/gtk4-layer-shell

### 16. gpui-terminal — CONFIRMED, with the requested details
Exists on crates.io/docs.rs: single version 0.1.0, published 2025-12-24; license
"MIT OR Apache-2.0" (Cargo.toml; GitHub license detection: Apache-2.0); repository
https://github.com/zortax/gpui-terminal; description "Terminal emulator component
for gpui". It exposes a public `TerminalView` (`pub use view::{ TerminalConfig,
TerminalView, TitleCallback, };`) wrapping alacritty_terminal 0.25.1
(Cargo.toml: `alacritty_terminal = "0.25.1"`; the view code uses
`alacritty_terminal::grid::Dimensions` via `TermSize`, locks `state.term_arc()`,
and calls `term.resize(TermSize { cols, rows })`).
Maintenance signals: not archived, 50 stars, 2 open issues; last commit on main
2026-01-11 ("update readme"); repo `pushed_at` 2026-07-30 (likely a non-default
branch); **depends on the frozen crates.io `gpui = "0.2.2"`, not a zed-main git
pin** — it does not track the layer-shell-era API and would need patching to move
to a git pin.
Citations: https://crates.io/api/v1/crates/gpui-terminal ; https://raw.githubusercontent.com/zortax/gpui-terminal/main/Cargo.toml ; https://raw.githubusercontent.com/zortax/gpui-terminal/main/src/lib.rs ; https://raw.githubusercontent.com/zortax/gpui-terminal/main/src/view.rs ; https://api.github.com/repos/zortax/gpui-terminal ; https://api.github.com/repos/zortax/gpui-terminal/commits?per_page=5

### 17. notify-rust README backends; current version — CONFIRMED
README verbatim: "**`NSUserNotificationCenter`** (default) via
[mac-notification-sys](https://crates.io/crates/mac-notification-sys)." and "macOS
is supported via two backends:"; "Similar to macOS we support windows via the help
of [winrt-notification](https://crates.io/crates/winrt-notification)." Current
version: **4.19.0**, published 2026-11-08, license MIT OR Apache-2.0.
Citations: https://raw.githubusercontent.com/hoodie/notify-rust/master/README.md ; https://crates.io/api/v1/crates/notify-rust

### 18. Windows Terminal `quakeMode` — CONFIRMED
learn.microsoft.com actions docs: "### Open the quake mode window — This action is
a special variation of the `globalSummon` action. It specifically summons the quake
window ... **Command name:**`quakeMode`". The same page documents `globalSummon`
with `monitor` (`any`/`toCurrent`/`toMouse`) and `desktop`
(`any`/`toCurrent`/`onCurrent`) targeting.
Citation: https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions

### 19. iTerm2 dedicated hotkey window — CONFIRMED
iterm2.com/documentation-hotkey.html verbatim: "a window that is associated with a
profile and has a hotkey attached to it. By pressing the hotkey, the window opens
or closes"; "In its simplest form, it's a system-wide terminal window that you can
open with a hotkey"; "If you want to assign multiple hotkeys to a single dedicated
hotkey window, add them by clicking Additional Hotkeys"; "You can also assign the
double-tap of a modifier."
Citation: https://iterm2.com/documentation-hotkey.html

### 20. gpui-terminal license and maintenance — ANSWERED (see 16)
License MIT OR Apache-2.0; one release (0.1.0, 2025-12-24); main last touched
2026-01-11; not archived; pins the frozen crates.io gpui 0.2.2.

### 21. Layer-shell runtime-mutation work since the 2025-10-29 re-land — ANSWERED
Searches: `set_keyboard_interactivity`, "layer shell" resize, PRs referencing
46378. Findings (all dates from the GitHub API):

- **#50520** (PR, merged 2026-03-08): "gpui(linux): Pass display_id to layer shell
  get_layer_surface" — output targeting; the fix that landed for #46378.
- **#60161** (PR, merged 2026-07-07): input region support for Wayland windows.
- **#60163** (PR, merged 2026-07-15): "gpui: Linux wayland set exclusive zone and
  edge" — adds the public runtime `Window::set_exclusive_zone`/`set_exclusive_edge`;
  body: "The exclusive zone and exclusive edge of a wlr-layer-shell surface could
  only be set once, at creation, through `LayerShellOpt...".
- **#61368** (PR, open, 2026-07-20): fix compositor-assigned layer-shell dimensions
  (zero-size crash).
- **#61420** (PR, open, 2026-07-21): "gpui: Allow changing wayland layer-shell
  keyboard interactivity" — adds `Window::set_keyboard_interactivity`; NOT merged.
- **#61578** (PR, open, 2026-07-24): "gpui_linux: Enforce Wayland output discovery
  during application startup" (fixes #46378's startup half); NOT merged.
- **#64994** (PR, open, 2026-09-30): "Fix wayland layer shell sizing" — treat
  compositor configure as suggestion, not authoritative.
- **#65055** (PR, closed unmerged, 2026-10): session-lock layer-shell surface.

Bottom line for the records: since the re-land, size + exclusive zone/edge gained
public runtime mutation; keyboard-interactivity mutation is proposed but unmerged
(#61420); no anchor/margin/layer mutation PR exists at all.

### 22. crates/terminal deps + terminal_view README at the pinned rev — CONFIRMED
`crates/terminal/Cargo.toml`: `name = "terminal"`, `alacritty_terminal.workspace = true`,
`gpui.workspace = true` (workspace inheritance; the workspace root pins
`alacritty_terminal = { git = "https://github.com/zed-industries/alacritty", rev =
"4c129667ce56611becdc82de6e28218c80e2e88f" }`). `crates/terminal_view/README.md`
contains verbatim: "The current implementation is still Alacritty-backed, but the
abstraction boundary keeps backend details".
Citations: https://raw.githubusercontent.com/zed-industries/zed/ec237216460f8a34a4ac7c6b068e838593976ede/crates/terminal/Cargo.toml ; .../crates/terminal_view/README.md ; .../Cargo.toml (root)

### 23. Canonical per-process introspection APIs — CONFIRMED (Windows); macOS substance confirmed, online primary source not published
- **Windows — the standard documented pair**: `CreateToolhelp32Snapshot`
  (tlhelp32.h, Kernel32.dll; "Takes a snapshot of the specified processes, as well
  as the heaps, modules, and threads used by these processes"; enumeration via
  `Process32First`) plus `QueryFullProcessImageName` (winbase.h; "Retrieves the
  full name of the executable image for the specified process"). The Toolhelp page
  itself cross-references: "you can use the QueryFullProcessImageName function to
  retrieve the full name of an executable image". `NtQueryInformationProcess` is
  NOT the standard pick — it has no Microsoft reference page in the Win32 docs and
  is the undocumented-native-API route (e.g. for reading another process's command
  line, which no documented Win32 API returns directly).
  Citations: https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-createtoolhelp32snapshot ; https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-queryfullprocessimagenamea
- **macOS — libproc family (`proc_pidpath`, `proc_pidinfo`, header
  `/usr/include/libproc.h`, man sections 3)**: this is the canonical macOS
  introspection family, but Apple does not publish these man pages on
  developer.apple.com — `developer.apple.com/documentation/kernel/proc_pidpath` and
  `/documentation/kernel/proc_pidinfo` return 404, opensource.apple.com
  `/source/libproc/` 404s, and Apple's GitHub `apple-oss-distributions` org does not
  expose a browsable libproc repo. Per the method rules this half is
  **UNVERIFIABLE online**; the canonical citation is the on-system macOS SDK man
  page (`man 3 proc_pidpath`, `man 3 proc_pidinfo`) and `libproc.h`, which the
  spike/Mac phase can read directly. A web search surfaced Apple's own
  Endpoint Security docs naming the libproc functions, but the specific page could
  not be fetched to quote.

## Discrepancies to fix

1. **gpui-migration.md, finding 4 (lines 147-149), "The gap"** — Suggested
   correction: replace "options are defined at window creation and have no public
   setters yet (the PR author flagged this explicitly). The platform trait exposes
   low-level `set_exclusive_zone`/`set_exclusive_edge` no-op defaults, but this is
   not the clean runtime mutation DropTerm's `surface_lifecycle.rs` assumes." with:
   "As of the pinned rev, three of the five operations have public runtime APIs:
   `Window::resize` (reaches `layer_surface.set_size` on Wayland),
   `Window::set_exclusive_zone`, and `Window::set_exclusive_edge` (both committed
   immediately; PR #60163, merged 2026-07-15). Still create-only: anchor, margin,
   layer, and keyboard_interactivity — `Window::set_keyboard_interactivity` is an
   unmerged PR (#61420, 2026-07-21) whose author states keyboard interactivity
   'can currently only be selected when a GPUI window is created.' The re-land PR's
   'no setters yet' note (#35610) was true in 2025-10 but is stale."
2. **gpui-migration.md, finding 4 (lines 150-153), output selection** — Suggested
   correction: replace "A comment on #46378 dated 2026-05-08 reports output
   selection fixed (PR #48502); this must be re-verified at the spike rev." with:
   "PR #48502 was closed unmerged (2026-02-24); the landed fix is PR #50520
   (merged 2026-03-08), which resolves `WindowParams.display_id` to a `wl_output`
   for `get_layer_surface`. At the pinned rev `cx.displays()` is populated at
   startup (outputs bound and roundtripped before the first window; comment in
   `gpui_linux/src/linux/wayland/client.rs`), and issue #46378 stays open only for
   further enforcement (open PR #61578)."
3. **gpui-migration.md, spike table check 2 (line 279)** — Suggested correction:
   the "candidate: window-bounds update; unknown — that is the point" cell: the
   path exists at the rev — `Window::resize` → `layer_surface.set_size`. Reframe
   the check as hardware verification that the compositor honors repeated
   `set_size` on the live surface at drag cadence (and protocol semantics: a size
   axis anchored to both edges is compositor-controlled; only the un-anchored axis
   is client-settable).
4. **gpui-migration.md, spike table check 4 (line 281) and Unverified (lines
   334-335)** — Suggested correction: exclusive_zone/edge runtime change is
   source-confirmed (claim 7); only the `Layer::Top`/`Overlay` layer change and
   keyboard-interactivity remain create-only. The "central unknown" narrows to:
   does keyboard-interactivity mutation land (#61420) or need a fork, and does
   `Window::resize` behave under compositor configure storms.
5. **gpui-migration.md, finding 5 / Unverified (lines 162-164, 341-342),
   gpui-terminal** — Suggested correction: fill in the deferred facts: license
   MIT OR Apache-2.0; v0.1.0 (2025-12-24, only release); main last touched
   2026-01-11; 50 stars; pins crates.io gpui 0.2.2 (does not track zed main) — so
   the spike would vendor/patch it rather than depend on it directly.
6. **portability-roadmap.md, finding 4 item 3 (lines 164-166)** — Suggested
   correction: "Layer / exclusive-zone on live config change ... degradable to
   next-show application" — exclusive-zone is now live-mutable via
   `Window::set_exclusive_zone`/`set_exclusive_edge` (PR #60163); only the layer
   change is create-only. Also item 2: the reshape path exists (`Window::resize`);
   what remains unknown is compositor behavior under the 80 ms cadence.
7. **portability-roadmap.md, finding 4 item 1 / assumption 2 (lines 159-161, 345-
   347)** — Suggested correction: keyboard-interactivity remains the single
   create-only crux (open PR #61420 proposes the API); the spike's check 3 is the
   only remaining source-unresolved gate.
8. **portability-roadmap.md, Unverified (lines 359-361), ProcessInfo naming** —
   Suggested correction: name them now: macOS = libproc (`proc_pidpath` /
   `proc_pidinfo`, `libproc.h`, `man 3`; online Apple doc pages do not exist —
   404s verified, cite the SDK man page); Windows = `CreateToolhelp32Snapshot` +
   `Process32First`/`Process32Next` for enumeration and
   `QueryFullProcessImageName` for the image path (MS Learn; note: no documented
   Win32 API returns another process's command line — the `/proc/pid/cmdline`
   equivalent needs WMI `Win32_Process` or undocumented `NtQueryInformationProcess`).
9. **Cosmetic, no action required**: gpui-migration.md line 144 and roadmap line
   113-114 list `Dialog` before `LayerShell(..)`; the source order is Normal, PopUp,
   AnchoredPopup, Floating, LayerShell, Dialog. Same variant set.

## Answers found (section E)

- **20 (gpui-terminal)**: license MIT OR Apache-2.0; v0.1.0 published 2025-12-24
  (single release); main last commit 2026-01-11, repo pushed 2026-07-30; 50 stars,
  2 open issues, not archived; depends on crates.io gpui 0.2.2, so it does not
  track zed main — usable as a reference/porting source, not as a dependency that
  follows the spike's git pin. (Claim 16, citations above.)
- **21 (runtime-mutation work since the re-land)**: #50520 (merged 2026-03-08,
  output targeting), #60161 (merged 2026-07-07, input region), #60163 (merged
  2026-07-15, exclusive zone/edge setters), #61368 (open, 2026-07-20, layer-shell
  sizing crash), #61420 (open, 2026-07-21, set_keyboard_interactivity), #61578
  (open, 2026-07-24, output discovery at startup), #64994 (open, 2026-09-30,
  configure-as-suggestion sizing). No anchor/margin/layer mutation PR exists.
  This updates the records' "create-only" gap: only keyboard-interactivity
  (+anchor/margin/layer) remains create-only at the pinned rev.
- **22**: confirmed at the pinned rev — `crates/terminal` depends on
  `alacritty_terminal.workspace = true` and `gpui.workspace = true` (alacritty
  resolved from the zed fork rev 4c129667), and `crates/terminal_view/README.md`
  says "The current implementation is still Alacritty-backed".
- **23**: Windows standard = `CreateToolhelp32Snapshot` (enumeration) +
  `QueryFullProcessImageName` (image path), both MS Learn (cited in claim 23);
  `NtQueryInformationProcess` is undocumented native API, not the standard pick.
  macOS = libproc (`proc_pidpath`/`proc_pidinfo`) in substance, but no fetchable
  Apple primary source exists online (404s on developer.apple.com and
  opensource.apple.com verified); cite the on-SDK man pages when the Mac phase
  opens — this half is UNVERIFIABLE online, not unverified in substance.
