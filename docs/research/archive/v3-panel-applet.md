# Research: how should DropTerm V3 build a COSMIC panel applet that toggles the terminal and shows status?

Date: 2026-10-03
Question: *What does the COSMIC panel applet platform actually provide, how do first- and third-party applets structure themselves and talk to other processes, what is the industry precedent in quake-style terminals, and which parts should DropTerm adopt?*

## TL;DR

**Ship a separate applet binary (`cosmic-ext-dropterm-applet`) from the same crate, and give
the main app one small custom D-Bus interface on the bus name it already owns.** The panel
platform is mature and fully documented by source: applets are ordinary processes discovered
by desktop-file stem, launched with a Wayland socket fd pointing at the panel, sized via
`COSMIC_PANEL_SIZE` env + `PanelSize` helpers in libcosmic — and libcosmic at DropTerm's
pinned rev `08a09e3` ships the complete `cosmic::applet` module (Context, popup helpers,
`cosmic::applet::run`, byte-identical on master as of 2026-10-02). The single-instance seam
DropTerm already uses exposes `org.freedesktop.DbusActivation` with fire-and-forget methods
and **no properties or signals** — which is exactly why `--is-visible` is stuck — but
libcosmic provides a sanctioned hook (`ApplicationExt::dbus_connection`) documented as
*"Can be used to expose custom interfaces on the same owned name"*. Add `Toggle/Show/Hide`
methods plus a read-only `Visible` property (PropertiesChanged for free) there; the applet
watches the bus name via `NameOwnerChanged` (the pattern cosmic-applet-status-area and the
third-party now-playing applet both use) to switch between live status and a "launch" state.
Left-click toggles — the unanimous quake precedent (Yakuake and Guake both default their
tray icon on, wired to toggle). Note the honest alternative: a system tray *does* exist on
COSMIC (`cosmic-applet-status-area` implements the full KDE StatusNotifierWatcher, installed
in the default panel config), so an SNI item is a viable smaller sibling — but it cannot
show COSMIC-native status styling and is second choice for the ROADMAP item as written.

## Findings

### 1. cosmic-panel: applets are processes discovered by desktop-file stem

From [pop-os/cosmic-panel](https://github.com/pop-os/cosmic-panel) @ `master`
(`f4ba4ef`, 2026-10-01, cloned 2026-10-03):

- The panel's config (`CosmicPanelContainerConfig`) stores applet lists as **desktop-file
  IDs** (file stems) in `plugins_wings` / `plugins_center` —
  `cosmic-panel-config/src/container_config.rs` L145-163 (defaults: workspaces/app button
  left; status area, tiling, audio, bluetooth, network, battery, notifications, power right;
  time center) and L191-197 (dock defaults).
- `spawn_clients` (`cosmic-panel-bin/src/space/wrapper_space.rs` L375-519) resolves each ID
  by iterating `freedesktop_desktop_entry::default_paths()` and matching `path.file_stem()
  == name` — *"each applet is at most started once, even if multiple desktop files in
  different directories match"* (comment at L463-464). No `X-CosmicApplet` key is consulted
  at spawn time; the stem *is* the identity.
- Each applet is spawned with (`Process::new()` from cosmic-process-manager, L637-660):
  - `WAYLAND_SOCKET` = an fd to the panel's own Wayland display (L601) — the panel acts as
    a nested compositor host (xdg-shell-wrapper); applet surfaces render *inside* the panel.
  - env `COSMIC_PANEL_NAME`, `COSMIC_PANEL_OUTPUT`, `COSMIC_PANEL_SPACING`,
    `COSMIC_PANEL_ANCHOR`, `COSMIC_PANEL_BACKGROUND`, `COSMIC_PANEL_PADDING_OVERLAP`
    (RON-serialized, L440-453) and `COSMIC_PANEL_SIZE` = the *effective* size for that
    slot's side (L562-565), so a per-side `size_wings`/`size_center` override in panel
    config reaches the applet transparently.
  - optionally `X_PRIVILEGED_WAYLAND_SOCKET` — a security-context listener fd
    (wayland-security-context protocol) when the desktop file sets `X-HostWaylandDisplay`
    (L480, L566-593); app-set sandbox metadata is `app_id` = the applet name, `instance_id`
    = `name.output` (L571-576).
  - Flatpak applets get `--die-with-parent`, `--socket=inherit-wayland-socket`, and
    `--env=` arg forwarding (L622-631).
- **Lifecycle**: applets that die abnormally are respawned with a fresh socket —
  `with_on_exit` closure with `should_restart = is_restarting && err_code.is_some()`
  (L661-681, restart path L716-779). The client side of the contract: an applet exits when
  its panel connection breaks — libcosmic's token handler logs *"Connection to panel has
  ended. The applet will now exit with it."* and breaks the loop
  (libcosmic `src/applet/token/wayland_handler.rs` L168-174).
- **Adding an applet at runtime** needs no panel API: cosmic-settings writes the panel
  config; the panel watches it via cosmic-config watchers and rebuilds —
  `cosmic-panel-bin/src/config_watching.rs` L100-194 (`ConfigUpdate::Entries` →
  `update_space`, entry watchers → `ConfigUpdate::EntryChanged`).

### 2. The desktop-file contract: exact keys and the sizing mechanism

Verified from cosmic-panel `wrapper_space.rs` L473-514 (all keys the panel reads) and the
shipped desktop files in [pop-os/cosmic-applets](https://github.com/pop-os/cosmic-applets)
@ `master` (`57ace74`, 2026-09-30, cloned 2026-10-03):

| Key | Read at | Meaning |
|---|---|---|
| `Exec` | wrapper_space.rs:475 | required; Shlex-split, first word = executable |
| `X-CosmicApplet=true` | *not read by the panel* | marker consumed by cosmic-settings: applets list (`cosmic-settings/src/pages/desktop/panel/applets_inner.rs` L571) and hidden from startup apps (`.../startup_apps.rs` L498) |
| `X-HostWaylandDisplay` | wrapper_space.rs:480 | request the privileged security-context socket |
| `X-CosmicShrinkable` | wrapper_space.rs:490-493 | padding may shrink before overflow |
| `X-OverflowMinSize` / `X-CosmicOverflowMinSize` | wrapper_space.rs:481-485 | min size (applet units) before the panel overflows the applet into its popup |
| `X-OverflowPriority` / `X-CosmicOverflowPriority` | wrapper_space.rs:486-489 | who overflows first when the panel is full |
| `X-CosmicHoverPopup` | wrapper_space.rs:509-511 | panel may open the applet's popup on hover/press; value is a placement anchor (`AppletAutoClickAnchor`: Auto/Left/Right/Top/Bottom/Center/Start/End, panel_space.rs L150-160) |
| `X-MinimizeApplet` | wrapper_space.rs:495-507 | minimize-to-dock participant (one per panel) |
| `X-NotificationsApplet` | wrapper_space.rs:513-514 | notification fd participant |

Real example — `cosmic-applet-audio/data/com.system76.CosmicAppletAudio.desktop`:
`X-CosmicApplet=true`, `X-CosmicShrinkable=true`, `X-CosmicHoverPopup=End`,
`X-OverflowPriority=10`, `NoDisplay=true`, `Icon=...-symbolic`, `StartupNotify=true`.

**Sizing**: there is no `X-CosmicApplet-Size` key. Size hints flow at runtime:
panel → `COSMIC_PANEL_SIZE` (RON `PanelSize`) → libcosmic `applet::Context` →
`PanelSize::get_applet_icon_size/padding/shrinkable_padding`
(`cosmic-panel-config/src/panel_config.rs` L143-227). Concrete numbers: symbolic icon
sizes XS=16, S=20, M=28, L=32, XL=48 (Custom `s/2`); symbolic padding 8/10/14/16/16;
shrinkable padding 12/14/18/20/20. Applets call `core.applet.suggested_size(true)` and
never hardcode pixels — that is the entire size-hint mechanism.

### 3. libcosmic applet support at DropTerm's pinned rev

Verified in the local checkout of [pop-os/libcosmic @ `08a09e3`]
(https://github.com/pop-os/libcosmic/tree/08a09e3dfa1f729d52585fcedd6e731130235363)
(DropTerm pins this rev — `Cargo.toml`):

- **`cosmic::applet` module** (`src/applet/mod.rs`), enabled by the `applet` feature
  (`Cargo.toml` L36-44: `autosize, winit, wayland, tokio, cosmic-panel-config, ron,
  multi-window`; `applet-token = ["applet"]` at L45):
  - `applet::Context` (`core.applet`, always present on `app::Core` — `src/core.rs` L102)
    populated from the panel's env vars in `Default` (mod.rs L89-119).
  - `suggested_size` / `suggested_padding` / `suggested_window_size` (L122-171),
    `window_settings()` (L178-202: transparent, non-resizable, correct min size),
    `theme()` honoring `COSMIC_PANEL_BACKGROUND` Dark/Light override (L488-503),
    text style scaling with panel size (L505-528).
  - Widget helpers: `icon_button(name)` (L280-291), `text_button` (L253-278),
    `applet_tooltip` (L293-356, a Wayland subsurface tooltip anchored per panel edge),
    `popup_container` (L358-402, rounded 360px-max autosize popup chrome), and
    `get_popup_settings(parent, id, …)` (L404-457) producing the `SctkPopupSettings` with
    anchor/gravity flipped per panel edge — the standard way applet popups are made.
  - `menu_button` / `padded_control` / `menu_control_padding` (L605-626) for popup menu
    rows, and `applet::style()` (L595-603) returning a transparent-background style.
  - **`cosmic::applet::run::<App>(flags)`** (L536-593): runs the app as an iced *daemon*
    with `AppType::Applet`, no decorations, `exit_on_close_request = true`. There is **no
    separate `cosmic::applet::App` trait at this rev** — applets implement the ordinary
    `cosmic::Application`.
  - `cosmic::applet::token::activation_token_subscription` (behind `applet-token`,
    `src/applet/token/subscription.rs` L13-25): a subscription that lets an applet request
    an **xdg-activation token** for an app it wants to launch
    (`TokenRequest { app_id, exec }` → `TokenUpdate::ActivationToken { token, exec }`),
    connecting either via the privileged socket or `WAYLAND_DISPLAY`.
- **Stability check**: cloned libcosmic `master` (`5a8bd94`, 2026-10-02) and diffed —
  `src/applet/mod.rs` public surface and `src/dbus_activation.rs` are **byte-identical**
  to the pinned rev. The applet API DropTerm would build on has not drifted.

### 4. Canonical applet anatomy and the IPC patterns applets use

From pop-os/cosmic-applets @ `57ace74`:

- **Simple applet — time** (`cosmic-applet-time/`): crate with `src/main.rs` (init
  logging) → `lib.rs` `run()` → **`cosmic::applet::run::<Window>(())`**; the app type
  implements `cosmic::Application` with its own `APP_ID`
  (`src/window.rs` L317-321), `style() = Some(cosmic::applet::style())` (L359-361).
  Left-click toggles a popup: `Message::TogglePopup` → `cosmic::surface::action::app_popup`
  + `core.applet.get_popup_settings(main_window_id, new_id, …)` with the applet's tracked
  rectangle as anchor (L519-557); popup content is `core.applet.popup_container(...)` in
  `view_window` (L760-763), ending with the settings `menu_button` item.
  `Cargo.toml`: libcosmic + `zbus` + `timedate-zbus` (system bus `TimeDateProxy`,
  `receive_timezone_changed`, L436-454) + `logind-zbus` (`receive_prepare_for_sleep`,
  L478-489) + `core.watch_config(APP_ID)` — i.e. **plain zbus proxies inside iced
  subscriptions** is the accepted way applets consume D-Bus.
- **Launching another app** (the pattern DropTerm's applet needs for "not running →
  start"): the time applet's "Date & Time Settings…" item sends a `TokenRequest`
  (`window.rs` L607-618), and on `TokenUpdate::ActivationToken` spawns
  `std::process::Command::new("cosmic-settings").arg("time")` with `XDG_ACTIVATION_TOKEN`
  and `DESKTOP_STARTUP_ID` env set from the token, via `tokio::spawn(cosmic::process::spawn(cmd))`
  (L627-635). The power applet does the simpler fire-and-forget variant:
  `process::Command::new("cosmic-osd").arg("log-out").spawn()` (`cosmic-applet-power/src/lib.rs`
  L159-171) and `cosmic-settings` (L196-201).
- **Applet ↔ daemon over D-Bus**: the notifications applet talks to a *separate daemon*
  over a peer-to-peer zbus connection on a Unix socket fd passed as `COSMIC_NOTIFICATIONS`
  (`src/subscriptions/notifications.rs` L134-169 — custom interface
  `com.system76.NotificationsApplet` with `notify` signal + `invoke_action`), and when the
  daemon is unreachable it retries 5× and then **exits, relying on the panel's respawn**
  (L60-80, comment: *"the applet needs the notifications daemon in order to work
  properly"*). The audio applet instead uses `zlink` (Pulse/PipeWire wire protocol) +
  `mpris2-zbus` (`Cargo.toml`), and the tiling applet drives cosmic-comp over custom
  Wayland protocols (`cosmic-applet-tiling/src/wayland.rs` L4-40, `ext_workspace` +
  `cosmic_protocols` workspace-v2 `TilingState`). Pattern summary: **whatever transport
  the service speaks, the applet hosts a subscription and renders state; D-Bus proxies
  are the default.**

### 5. A system tray exists on COSMIC (2026 state)

`cosmic-applet-status-area` in pop-os/cosmic-applets @ `57ace74` implements the **full
KDE StatusNotifierItem host stack**:

- A socket-activated daemon owns `org.kde.StatusNotifierWatcher` at
  `/StatusNotifierWatcher` with `RegisterStatusNotifierItem`,
  `RegisteredStatusNotifierItems`/`IsStatusNotifierHostRegistered`/`ProtocolVersion`
  properties and the four standard signals
  (`src/subscriptions/status_notifier_watcher/server.rs` L17-85). It is split into a
  separate `com.system76.CosmicStatusNotifierWatcher` daemon so panel restarts don't race
  to re-own the name (`src/status_notifier_watcher.rs` L1-51).
- Hosts watch `org.freedesktop.DBus.NameOwnerChanged` to reap items whose owners vanished
  (server.rs L98-110; `src/unique_names.rs` L1-114, which cites
  `pop-os/cosmic-comp/src/dbus/name_owners.rs` as its model — cosmic-comp itself uses the
  same watching pattern for its D-Bus clients).
- Items are consumed as SNI clients (`Status`, `IconName`/`IconPixmap`, `Menu` via
  DBusMenu, `Activate`/`SecondaryActivate`, `ItemIsMenu` —
  `src/subscriptions/status_notifier_item.rs` L13-167).
- The status area ships **in the default panel config**
  (`cosmic-panel/.../container_config.rs` L153), so every stock COSMIC install renders
  tray icons. Third parties also ship SNI hosts (`marcelogomes90/cosmic-ext-applet-status-hub`,
  listed in the cosmic-utils collection). A Rust app can publish an SNI item with the
  maintained `ksni` crate ([iovxw/ksni](https://github.com/iovxw/ksni), Unlicense —
  GPL-compatible; pushed 2026-09-30).

So SNI is *not* missing; it is simply not the native panel surface. A panel applet
remains the only route with COSMIC-native theming, sizing, and popup behavior — but a
tray icon is a legitimate low-effort complement.

### 6. libcosmic single-instance D-Bus API — exact surface and the sanctioned extension hook

Verified at the pinned rev `08a09e3` (byte-identical on master `5a8bd94`):

- **Server side** (`src/dbus_activation.rs`): session bus; bus name = `App::APP_ID`
  (requested after `object_server().at()`, L37-44); object path = `/` + APP_ID with `.`
  → `/` (L24); **interface `org.freedesktop.DbusActivation`** with exactly three methods:
  `activate(platform_data)`, `open(uris, platform_data)`,
  `activate_action(action_name, parameter: Vec<&str>, platform_data)` (proxy trait
  L131-150, impl L152-229). **No properties. No signals.** `activate_action`'s action
  string is documented as *"can be deserialized as Flags"* (L106-107) — this is the seam
  DropTerm's `toggle/show/hide/new-tab` CLI already rides.
- **Client side** (`src/app/mod.rs` L189-257, `run_single_instance`): on launch, a
  blocking proxy to `destination = APP_ID`, same path/interface; if the proxy call
  succeeds the second process exits immediately — the caller learns only success/failure
  of a fire-and-forget call, nothing about state (L243-256). `XDG_ACTIVATION_TOKEN` and
  `DESKTOP_STARTUP_ID` env are forwarded as `activation-token`/`desktop-startup-id`
  platform data (L205, L229-235). `COSMIC_SINGLE_INSTANCE=false` disables the forwarding
  (L207-212).
- **The extension hook**: when the single-instance subscription connects, it delivers the
  live `zbus::Connection` to the app as `Action::Cosmic(Action::DbusConnection(conn))`
  (`dbus_activation.rs` L46-50), dispatched to **`ApplicationExt::dbus_connection(&mut
  self, conn)`** (`src/app/cosmic.rs` L1250-1253), whose doc reads verbatim:
  *"Invoked on connect to dbus session socket used for dbus activation. Can be used to
  expose custom interfaces on the same owned name."* (`src/app/mod.rs` L506-512).
  zbus object paths can host multiple interfaces, so a custom DropTerm interface can live
  at the same path as `org.freedesktop.DbusActivation` without touching libcosmic.

Conclusion for the ROADMAP's deferred note: correct — nothing queryable exists today;
a custom interface is required, and it is explicitly supported.

### 7. Third-party applets: structure and cross-process control

The ecosystem hub is [cosmic-utils](https://github.com/cosmic-utils) (org +
[project collection](https://github.com/cosmic-utils/cosmic-project-collection), fetched
2026-10-03; ~50 applets listed). Verified examples, cloned 2026-10-03:

- **[Hasmolam/cosmic-ext-applet-scratchpad](https://github.com/Hasmolam/cosmic-ext-applet-scratchpad)**
  @ `master` (2026-09-28): the canonical modern third-party shape — standalone crate,
  GPL-3.0-only, `cosmic::applet::run`, libcosmic pinned by rev with features
  `applet, applet-token, dbus-config, multi-window, tokio, wayland, …` (deliberately
  **without** `wgpu`), desktop file with `X-CosmicApplet=true` + `NoDisplay=true`, own
  `-symbolic` icon, `install.sh`/justfile.
- **[cosmic-utils/cosmic-ext-applet-now-playing](https://github.com/cosmic-utils/cosmic-ext-applet-now-playing)**
  @ `master` (2026-09-01) — *applet reflecting a separate service that may vanish*: it
  subscribes to MPRIS players and adds a raw `MatchRule` for
  `org.freedesktop.DBus.NameOwnerChanged`, filtering names starting with
  `org.mpris.MediaPlayer2.` (`src/media.rs` L189-202, L239-241) to re-enumerate players
  when one appears/quits; with no player it renders a placeholder icon
  (`src/ui.rs` L123-131, fallback `audio-x-generic-symbolic`).
- **[glima/mare-player](https://github.com/glima/mare-player)** @ `master` (2026-10-02) —
  *one codebase, two faces*: a `panel-applet` cargo feature (default) selects
  `libcosmic/applet` + `applet-token`; disabling it builds a standalone windowed app
  (`Cargo.toml` L22-32). Its Cargo.toml also documents a real pitfall DropTerm should
  copy the conclusion of: **wgpu on a layer-shell applet surface fails to bind the GPU
  and falls back to llvmpipe** (~350 MB of software GL), so the applet build uses
  tiny_skia only (comment block L33-40). A companion video-window helper lives as a
  second workspace member binary talking over a stdin/stdout pipe.

No third-party applet was found that embeds its target app's UI — cross-process control
is always D-Bus/launch. That matches the platform's shape: an applet is its own
libcosmic process.

### 8. Quake-terminal precedent: tray icons are the norm

- **Yakuake** ([KDE/yakuake](https://github.com/KDE/yakuake) @ `master`, 2026-09-26):
  ships a `KStatusNotifierItem` — `app/mainwindow.cpp` L938-950: icon
  `yakuake-symbolic`, `setStatus(Active)`, context menu = the main window menu, and —
  the load-bearing lines — `setAssociatedWindow(nullptr)` *"Prevent the default
  implementation of showing and instead run toggleWindowState"* followed by
  `connect(m_notifierItem, &KStatusNotifierItem::activateRequested, this,
  &MainWindow::toggleWindowState)` (L947-948). **Left-click on the tray toggles the
  drop-down.** The tray icon is on by default: `ShowSystrayIcon` `<default>true</default>`
  (`app/config/yakuake.kcfg` L74-78).
- **Guake** ([Guake/guake](https://github.com/Guake/guake) @ `master`, 2026-08-19):
  tray icon on by default (`use-trayicon` `<default>true</default>`,
  `guake/data/org.guake.gschema.xml` L59-62). Left-click `activate` → `show_hide`
  (toggle); the fallback `Gtk.StatusIcon` path is identical
  (`guake/guake_app.py` L163-182). The AppIndicator path serves the same "Show" menu.
- **Tilda** ([lanoxx/tilda](https://github.com/lanoxx/tilda) @ `master`, 2026-01-10): **no
  tray icon** — its answer is D-Bus: interface `com.github.lanoxx.tilda.Actions` at
  `/com/github/lanoxx/tilda/Actions` with a single `Toggle` method
  (`src/tilda-dbus-actions.xml`, bus name in `src/tilda-dbus-actions.c` L6), which the
  CLI invokes (`tilda -T` → `tilda_dbus_actions_toggle`, `src/tilda.c` L256); instance
  IDs are appended to the bus name for multi-instance (`tilda-dbus-actions.c` L140).
- **QTerminal** ([lxqt/qterminal](https://github.com/lxqt/qterminal) @ `master`,
  2026-09-29): no tray icon anywhere in `src/` (grep for tray/TrayIcon: zero hits); its
  drop-down mode is layer-shell native
  (`src/mainwindow.cpp` L193 `m_layerWindow->setScope(QStringLiteral("dropdown_terminal"))`),
  toggled by hotkey only — the closest architectural sibling to DropTerm, and it has
  neither tray nor applet today.
- **iTerm2** ([gnachman/iTerm2](https://github.com/gnachman/iTerm2) @ `master`,
  2026-09-30): the hotkey window is toggled by global hotkey only. A menu-bar
  `NSStatusItem` exists but is **not** a toggle: it appears only when the app is
  excluded from the Dock, guarded by advanced setting `statusBarIcon` (default YES,
  `sources/AppKit/iTermApplication.m` L965-988), and shows a menu of
  Settings / Bring All Windows to Front / Buried Sessions / Check For Updates
  (`sources/AppKit/iTermApplicationDelegate.m` L1853-1877). Official docs match: the
  Appearance page describes the menu-bar icon as a way *"to get back to iTerm2's
  preferences… useful if you only use hotkey windows"*.

### 9. GNOME ddterm: the shell-extension contrast, and the D-Bus shape DropTerm needs

[ddterm/gnome-shell-extension-ddterm](https://github.com/ddterm/gnome-shell-extension-ddterm)
@ `master` (2026-10-01) is the maintained GNOME drop-down terminal: the **GNOME Shell
itself hosts the UI** (JavaScript extension + a VTE app process), and control is exposed
three ways (`docs/CommandLine.md`): a CLI tool, `gapplication action … show/hide/toggle`,
and a dedicated D-Bus interface. The last one is the blueprint —
`data/com.github.amezin.ddterm.Extension.xml`:

- methods `Toggle`, `Activate`, `Hide`, `Service`;
- read-only **properties** `HasWindow` (b), `TargetRect` (iiii), `TargetMonitorScale`
  (d), `Version`, `Revision` — with `PropertiesChanged` emitted on change
  (`ddterm/shell/dbusapi.js` L133-163, `emit_property_changed('HasWindow', …)` L310),
  exported at `/org/gnome/Shell/Extensions/ddterm` on the session bus.

The contrast DropTerm must respect: COSMIC has **no shell extensions** — the sanctioned
equivalent of "integrate with the shell chrome" is a panel applet (finding 1-3), and the
sanctioned equivalent of the extension's queryable control interface is a custom
interface on the app's owned bus name (finding 6). ddterm shows both halves done well.

## What applies to DropTerm

### Recommended architecture

1. **Same crate, second `[[bin]]`: `cosmic-ext-dropterm-applet`.** Precedent: pop-os
   keeps every applet a separate *crate* in a workspace; third parties ship standalone
   crates; mare-player proves dual-targeting one codebase is fine — but via a compile-time
   feature, which cannot run app and applet *concurrently*. DropTerm needs both alive at
   once (app owns the layer surface; applet lives in the panel), so two process entry
   points are required. A `cosmic-ext-dropterm applet` subcommand would also work (the
   panel Shlex-splits `Exec=cosmic-ext-dropterm applet`), but it drags applet mode into
   the user-facing clap CLI whose default subcommand is `toggle`; a dedicated bin target
   is cleaner, matches the stem-matching identity scheme, and keeps logs unambiguous.
   The applet bin gets its own APP_ID (`io.github.l1br3_io.CosmicExtDropterm.Applet`),
   its own desktop file, and calls `cosmic::applet::run` — **never**
   `run_single_instance`, which would just forward-activate the terminal and exit,
   leaving a blank panel slot.
2. **Control channel: keep `activate_action` for commands, add one tiny custom interface
   for state.** Toggle/show/hide/new-tab already ride
   `org.freedesktop.DbusActivation.activate_action` — the applet reuses them verbatim
   (same messages the CLI produces, so one code path). For status, implement in the app:
   - bus name `io.github.l1br3_io.CosmicExtDropterm` (already owned),
   - path `/io/github/l1br3_io/CosmicExtDropterm` (same object as the activation
     interface),
   - interface e.g. `io.github.l1br3_io.CosmicExtDropterm.V1` served from the
     `ApplicationExt::dbus_connection` hook (zbus `#[interface]`; `object_server().at()`
     on the handed-over connection),
   - methods `Toggle()`, `Show()`, `Hide()`, `NewTab(cwd, cmd)` (mirroring the CLI —
     convenient for non-libcosmic callers; optional if `activate_action` suffices),
   - read-only properties `Visible: bool`, `TabCount: u32`, `BellPending: bool` —
     zbus emits `PropertiesChanged` automatically when properties mutate, which is the
     `VisibilityChanged` signal the ROADMAP sketched, for free.
   Effort **S-M**: zbus 5 is already in the tree; the hook exists; it's one file plus
   property updates on show/hide/tab/bell transitions.
3. **`--is-visible` unblocks for free**: read `Visible` off the proxy; if the bus name
   has no owner, the app isn't running → print `false` / exit non-zero. Effort **S**.
4. **Applet runtime state**: subscribe to `NameOwnerChanged` filtered on
   `io.github.l1br3_io.CosmicExtDropterm` (status-area `unique_names.rs` /
   now-playing `media.rs` pattern, both zbus 5). Not running → dimmed button, click
   spawns the CLI binary with an activation token obtained via
   `applet-token` (time-applet pattern) so the summoned terminal is properly activated.
   Running → reflect `Visible`; left-click calls `Toggle`. Optional popup
   (`popup_container`) with New Tab / Settings / Quit, right-click or a small popup
   button — but keep V1 minimal: an icon button that toggles, a tooltip, done.
5. **Desktop file for the applet** (keys from finding 2): `Exec=cosmic-ext-dropterm-applet`,
   `X-CosmicApplet=true`, `NoDisplay=true`, `Icon=io.github.l1br3_io.CosmicExtDropterm-symbolic`
   (ship a symbolic SVG to `icons/hicolor/scalable/apps/`; a generic fallback exists —
   `utilities-terminal-symbolic` is in Pop's theme, `Pop/scalable/apps/`, verified in the
   [pop-os/icon-theme](https://github.com/pop-os/icon-theme) tree 2026-10-03). Skip
   `X-HostWaylandDisplay` (DropTerm needs no privileged socket), set
   `X-CosmicShrinkable=true`. Install desktop file to `share/applications`, icon to
   `share/icons/hicolor/...` (cosmic-applets justfile L37-51 pattern). Effort **S**.
6. **Applet build config**: enable libcosmic `applet` + `applet-token` features for the
   applet bin only. Do **not** enable `wgpu` for the applet (mare-player's documented
   llvmpipe fallback on layer-shell surfaces); tiny_skia is what stock applets use.
   Effort **M** overall for the applet UI (Context-driven sizing, popup, tooltip, state
   machine, tests for the pure logic).
7. **Licensing/packaging**: everything verified compatible — libcosmic MPL-2.0, ksni
   Unlicense, applets typically GPL-3.0-only like DropTerm. No new copyleft obligations.

**Status content worth showing** in a ~20-32px slot: running/not (dim), visible/hidden
(the Yakuake/Guake tray parity), tab count as a small badge or in the tooltip, and
bell/attention state if the existing notification path exposes it — the now-playing
applet proves text+icon is possible, but V1 should be icon + tooltip (`applet_tooltip`)
with the popup carrying anything richer. `Visible` and running-state are the two with
primary-source precedent (ddterm `HasWindow`; Yakuake tooltip).

### Risks / open questions

- **Pinned-rev API stability**: verified non-issue today (applet module and
  dbus_activation byte-identical between `08a09e3` and master `5a8bd94`), but the applet
  module carries `// TODO make multi-window not mandatory` and the sizing helpers have
  changed historically — keep the applet thin and re-diff on any libcosmic bump.
- **Panel-side hover-popup semantics**: `X-CosmicHoverPopup` values (`Auto`, `End`) are
  anchors for a panel-initiated hover popup; the exact UX (hover-show of the applet's
  popup) is inferred from `panel_space.rs` L150-160 + `auto_popup_hover_press` naming —
  not traced end-to-end. Skip the key in V1; nothing depends on it.
- **Respawn semantics**: the restart condition is `is_restarting && err_code.is_some()`
  (wrapper_space.rs L681); the exact meaning of `err_code`/`is_restarting` in
  cosmic-process-manager was not read — whether a clean `exit(0)` triggers respawn (the
  notifications applet seems to rely on it) is unverified. DropTerm's applet shouldn't
  depend on either behavior; exit only when the panel connection dies.
- **Token forwarding value** (resolved on review): `run_single_instance` forwards
  `XDG_ACTIVATION_TOKEN` as platform data (verified, `src/app/mod.rs` L205-231) and the
  server side unpacks it into the activation `Message` struct (`src/dbus_activation.rs`
  L155-229) — but at the pinned rev nothing in libcosmic *applies* it, and DropTerm's
  `src/` never references it (grep: zero hits). The token is currently delivered and
  dropped. Harmless today — the shown layer surface takes exclusive keyboard focus on its
  own, so it never needed an xdg-activation raise — and it means the applet's
  launch-with-token path (finding 4) gains nothing *until* DropTerm starts consuming the
  token; the time-applet pattern is still the correct one to copy when it does.
- **Flatpak**: if DropTerm ever ships as a flatpak applet, the panel's flatpak arg
  injection (wrapper_space.rs L622-631) applies — the cosmic-flatpak issue #259 (an
  applet removed because it "can't work inside the Flatpak sandbox") is a warning to
  test D-Bus + process spawning under sandboxing before promising it.

## Sources

- cosmic-panel @ master `f4ba4ef` (cloned 2026-10-03):
  `cosmic-panel-bin/src/space/wrapper_space.rs` L375-519, L528-603, L622-631, L637-779;
  `cosmic-panel-bin/src/config_watching.rs` L100-194;
  `cosmic-panel-bin/src/space/panel_space.rs` L150-160;
  `cosmic-panel-config/src/container_config.rs` L131-216;
  `cosmic-panel-config/src/panel_config.rs` L126-227 —
  <https://github.com/pop-os/cosmic-panel>
- cosmic-applets @ master `57ace74` (cloned 2026-10-03):
  `cosmic-applet-time/src/window.rs` L317-321, L359-361, L436-489, L519-557, L607-637;
  `cosmic-applet-time/src/lib.rs`; `cosmic-applet-time/data/com.system76.CosmicAppletTime.desktop`;
  `cosmic-applet-audio/data/com.system76.CosmicAppletAudio.desktop`;
  `cosmic-applet-power/src/lib.rs` L159-201;
  `cosmic-applet-notifications/src/subscriptions/notifications.rs` L60-80, L134-169;
  `cosmic-applet-status-area/src/status_notifier_watcher.rs` L1-51,
  `src/subscriptions/status_notifier_watcher/server.rs` L17-110,
  `src/unique_names.rs` L1-114,
  `src/subscriptions/status_notifier_item.rs` L13-167;
  `cosmic-applet-tiling/src/wayland.rs` L4-40; workspace `Cargo.toml` L57 (zbus 5.14);
  `justfile` L37-51 — <https://github.com/pop-os/cosmic-applets>
- cosmic-settings @ master (cloned 2026-10-03):
  `cosmic-settings/src/pages/desktop/panel/applets_inner.rs` L540-583;
  `cosmic-settings/src/pages/applications/startup_apps.rs` L498 —
  <https://github.com/pop-os/cosmic-settings>
- libcosmic @ `08a09e3` (DropTerm's pinned rev, local cargo checkout):
  `src/applet/mod.rs` (entire module; run at L536-593, Context L42-119, helpers
  L122-528); `src/applet/token/subscription.rs` L13-80;
  `src/applet/token/wayland_handler.rs` L93-184; `src/dbus_activation.rs` L1-229;
  `src/app/mod.rs` L189-257, L506-512; `src/app/cosmic.rs` L1250-1253;
  `src/core.rs` L102; `Cargo.toml` L36-45, L72 —
  <https://github.com/pop-os/libcosmic/tree/08a09e3dfa1f729d52585fcedd6e731130235363>
- libcosmic @ master `5a8bd94` (2026-10-02, cloned 2026-10-03): diff vs pinned rev of
  `src/applet/mod.rs` and `src/dbus_activation.rs` — identical —
  <https://github.com/pop-os/libcosmic>
- cosmic-applet-template README (fetched 2026-10-03):
  <https://github.com/pop-os/cosmic-applet-template>
- cosmic-utils project collection README (fetched 2026-10-03):
  <https://github.com/cosmic-utils/cosmic-project-collection>
- Hasmolam/cosmic-ext-applet-scratchpad @ master (2026-09-28, cloned 2026-10-03):
  `Cargo.toml`, `data/…desktop` — <https://github.com/Hasmolam/cosmic-ext-applet-scratchpad>
- cosmic-utils/cosmic-ext-applet-now-playing @ master (2026-09-01, cloned 2026-10-03):
  `src/media.rs` L22-24, L189-241; `src/ui.rs` L95-138; desktop file —
  <https://github.com/cosmic-utils/cosmic-ext-applet-now-playing>
- glima/mare-player @ master (2026-10-02, cloned 2026-10-03): `Cargo.toml` L1-40,
  L130-175 — <https://github.com/glima/mare-player>
- Yakuake @ master (2026-09-26, cloned 2026-10-03): `app/mainwindow.cpp` L938-950;
  `app/config/yakuake.kcfg` L74-78 — <https://github.com/KDE/yakuake>
- Guake @ master (2026-08-19, cloned 2026-10-03): `guake/guake_app.py` L148-182;
  `guake/data/org.guake.gschema.xml` L59-62 — <https://github.com/Guake/guake>
- Tilda @ master (2026-01-10, cloned 2026-10-03): `src/tilda-dbus-actions.xml`;
  `src/tilda-dbus-actions.c` L3-30, L90-113; `src/tilda.c` L256, L330-351 —
  <https://github.com/lanoxx/tilda>
- QTerminal @ master (2026-09-29, cloned 2026-10-03): zero tray hits in `src/`;
  `src/mainwindow.cpp` L193 — <https://github.com/lxqt/qterminal>
- iTerm2 @ master (2026-09-30, cloned 2026-10-03):
  `sources/AppKit/iTermApplication.m` L965-993;
  `sources/AppKit/iTermApplicationDelegate.m` L1853-1877;
  `sources/Settings/iTermAdvancedSettingsModel.m` L508;
  docs <https://iterm2.com/3.4/documentation-preferences-appearance.html> —
  <https://github.com/gnachman/iTerm2>
- ddterm @ master (2026-10-01, cloned 2026-10-03): `README.md`;
  `docs/CommandLine.md`; `data/com.github.amezin.ddterm.Extension.xml`;
  `ddterm/shell/dbusapi.js` L55-110, L133-188, L282-310 —
  <https://github.com/ddterm/gnome-shell-extension-ddterm>
- ksni (API check 2026-10-03): <https://github.com/iovxw/ksni> — Unlicense, active
- pop-os/icon-theme (tree listing 2026-10-03):
  `Pop/scalable/apps/utilities-terminal-symbolic.svg` —
  <https://github.com/pop-os/icon-theme>
- DropTerm itself: `Cargo.toml` L1-12; `src/main.rs` L45-69; `res/io.github.l1br3_io.CosmicExtDropterm.desktop`; `ROADMAP.md` L31, L35
