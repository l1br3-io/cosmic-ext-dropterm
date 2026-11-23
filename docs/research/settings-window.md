# Should settings move from the drawer to its own window?

2026-11-23

**TL;DR — yes: the settings surface becomes a separate `WindowKind::Normal`
toplevel (the About and onboarding windows' shape) laid out like Obsidian's
Settings window — a navigation column of nine pages under two headers beside
a content pane — with the drop-down hiding while it is open and returning
when it closes. The in-surface drawer is capped at half the drop-down's
width inside a surface that is itself 40% of the screen tall by default; a
window has the room for one page of rows instead of five tabs of fold-away
cards. An OS-level popup is rejected (menu-grade: no title bar, no resize,
compositor dismissal), an in-surface dialog rejected (capped at the
drop-down's ~430 px height at 1080p), and the Kit's composite `setting::Settings`
rejected (its per-window keyed state bypasses the shell-owned page model the
triggers and tests read; its sidebar is a flat list with no group headers).
The decision reverses [settings-sidebar](archive/settings-sidebar.md)
(2026-10-03, "the second window deleted") knowing why it was made, and
supersedes [settings-tabs](archive/settings-tabs.md) (2026-10-05, five tabs)
and the settings-navigation parts of [settings-trigger](archive/settings-trigger.md)
(2026-10-02); the archive is never edited, this row is the supersession by
link. The cost, accepted by the owner: the drop-down hides while Settings is
open, so the terminal is not visible while a setting is changed — edits
still apply to the running tabs immediately and are seen on return.**

## Findings

1. **Stacking forces the hide.** The drop-down is a layer-shell Overlay
   surface, which the compositor stacks above every ordinary window
   (`core/surface_lifecycle.rs`, the `Verdict::LayerShell` arm). An ordinary
   toplevel opened while the drop-down stands would draw *beneath* it —
   the same fact the archived settings-sidebar record documented for the
   libcosmic era (its "hide-first rule"), and the one
   `Shell::dismiss_for_summon_setup` encoded for the onboarding window.
   The exclusive-pair lifecycle is therefore not a choice but a constraint:
   open hides, user close returns, any re-show of the drop-down supersedes.

2. **Window kinds.** In `gpui-pre-linux` 0.3.12's Wayland backend every
   `WindowKind` except `AnchoredPopup` opens a regular xdg toplevel
   (`src/linux/wayland/window.rs:258` — "All other WindowKinds result in a
   regular xdg surface", read 2026-11-23). `AnchoredPopup` is an `xdg_popup`
   parented through the layer surface (`window.rs:210`) — a menu-grade
   surface with no title bar, no resize, and compositor dismissal: rejected
   for a settings surface. "Window" and "dialog" are therefore the same
   toplevel here; the distinction is cosmetic.

3. **Why not in-surface.** `DEFAULT_HEIGHT_PERCENT = 40` and
   `DEFAULT_WIDTH_PERCENT = 100` (`core/config.rs`): a dialog drawn inside
   the drop-down gets about 430 px of height at 1080p — less than the
   Profiles page's form alone. The drawer-era width cap
   (`WIDTH_CAP_FRACTION`, 616 px by default) was why the 2026-10-05 tabs
   record rejected a vertical nav inside the drawer on width. A window has
   neither cap.

4. **Precedents in-tree.** `tray.rs::open_about` and
   `onboarding.rs::open_onboarding` already open plain toplevels with the
   app id set, and `QuitMode::Explicit` keeps the daemon alive when they
   close — the windowed-settings lifecycle needs no new process semantics.
   `WindowOptions::titlebar` (`TitlebarOptions { title }`),
   `window_min_size`, and `is_resizable` cover the shape (960×720 default,
   640×420 floor, "Settings - DropTerm"); Wayland gives clients no say in
   placement.

5. **The Kit survey.** The carried `gpui-component` 0.9.1 ships both a
   `sidebar` family (`Sidebar`/`SidebarGroup`/`SidebarMenu`/
   `SidebarMenuItem` with `.active(..)`) and a composite
   `setting::Settings` (search header, page list, `SettingPage` →
   `SettingGroup` → `SettingItem`, filter-as-you-type, a resizable split).
   Two mismatches ruled the composite out, from its source: its sidebar is
   a flat page list with no group headers, and it owns selection and search
   state in per-window keyed state (`SettingsState`, `pub(super)`) — which
   would bypass the shell-owned page that `open_settings_at` and the bin
   tests read. The `Sidebar` family fails separately: its active-row colors
   come from `sidebar_accent` tokens the scheme generator does not produce
   (`theme_gen.rs` maps `sidebar.background`, `.foreground`, `.border`
   only), so the highlight would fall back to the Kit's neutral default —
   near-black on near-black in dark mode. The nav column is hand-composed
   from the tab strip's idiom instead; search arrives as its own module
   (settings-search) once the window stands.

6. **Two documented Profiles behaviors were never wired in the drawer.**
   `docs/settings.md` promised that Escape cancels an open profile draft
   and that leaving the Profiles page clears it; core had
   `apply_profile_draft_transition` with a test and no caller. The window's
   SW7 task wired both — the plan found them, not the walk.

## What DropTerm has today

The window (SW1–SW7): `SettingsPage` with nine variants and `NAV_GROUPS`
(Terminal, then Profiles & keys) in core; the window in
`dropterm/src/settings.rs` rendered inside the Kit `Root` in a plain
toplevel; the lifecycle on the shell — open queues the explicit hide in
both `hide_on_focus_loss` modes (plan amendment 2: idempotent where the
focus handoff also fires, decisive where it never does), every close route
meets in the `on_window_closed` observer through `CloseCause`/`terminal_effect`,
and supersede rides `Action::CloseSettingsWindow` queued ahead of the
surface's own actions. The two drawer-only config keys
(`settings_drawer_width`, `settings_drawer_click_close`) are removed whole;
a file still carrying either line loads and ignores it, pinned by test.

## Recommendation

The separate window as built. Rejected along the way:

- **The OS-level popup** (`WindowKind::AnchoredPopup`): menu-grade — no
  title bar, no resize, compositor dismissal. Wrong for a surface users
  resize and keep open.
- **The in-surface dialog**: capped at the drop-down's height; re-creates
  the exact constraint that produced the five-tab drawer.
- **The wider drawer**: still capped at half the surface and still inside
  the 40%-height drop-down; the tabs record already rejected the nav on
  drawer width.
- **The composite `setting::Settings`**: bypasses the shell-owned state
  (its `SettingsState` is per-window keyed), no group headers, and adopts
  the generator-missing `sidebar_accent` tokens.
- **Persisting geometry or the open page**: a file-format decision (state
  file vs config keys) deferred to a follow-up module if the owner wants
  it; every open starts at the requested page.

The two unwired behaviors (finding 6) shipped with the window rather than
being ported as bugs.

## Unverified (the walk's)

- Whether the returned drop-down holds keyboard focus (`keyboard_mode` and
  the compositor decide) — the module's largest unknown, judged in the walk.
- Popovers, the three pickers, the keybind capture, and the onboarding
  inside a second window under the scheme-derived theme.
- The compositor's placement and title bar rendering.
- The onboarding's stacking over the Settings window (two toplevels).
- On a plain-window session (no layer-shell), whether the new toplevel
  becomes transient for the drop-down (gpui parents to the focused window);
  recorded for the portability plan — the exclusive-pair rule is meant to
  hold there unchanged.

## Sources

- `gpui-pre-linux` 0.3.12 (the carried pin), `src/linux/wayland/window.rs`
  lines 210, 258 — window kinds; read 2026-11-23.
- `gpui-component` 0.9.1 (the carried `[patch]` copy), `src/root.rs`,
  `src/setting/`, `src/sidebar/` — the Kit survey; read 2026-11-23.
- `dropterm/src/core/config.rs` — `DEFAULT_HEIGHT_PERCENT`,
  `DEFAULT_WIDTH_PERCENT`; read 2026-11-23.
- `dropterm/src/tray.rs::open_about`, `dropterm/src/onboarding.rs::open_onboarding`
  — the toplevel precedents; read 2026-11-23.
- The owner's Obsidian Settings screenshot, 2026-11-23 — the layout reference.
- [settings-sidebar](archive/settings-sidebar.md) (2026-10-03) and
  [settings-tabs](archive/settings-tabs.md) (2026-10-05) — the reversed
  predecessors, quoted from the archive.
