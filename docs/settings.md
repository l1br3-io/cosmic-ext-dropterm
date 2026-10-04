# Settings window

Every DropTerm setting is editable in this in-app window, and every edit applies the moment you commit it - there is no Apply button.

## Opening the window

Every setting is editable in-app: press `Ctrl+Shift+I` while the terminal has keyboard focus, or click the gear button at the right corner of the tab strip. The window is a separate normal window in the same process. The strip is always present whatever the tab count - with a single tab it carries the settings button alone - so settings are one click away in every state; hovering the button shows a `Settings…` tooltip and the click opens the same settings window the keybind opens. The keypress is swallowed from the shell like every other app bind. A second press focuses the existing window; there is never a second one.

## Cards and commit points

The window shows five cards - Appearance (font, size, opacity, the two color scheme dropdowns, and the Application theme dropdown), Geometry, Behavior (hide on focus loss, session persistence, slide animation, bell notifications, new-tab placement), Terminal, and Advanced (`respect_panel`, `layer`, `keyboard_mode`) - and applies instantly: every control writes its key the moment you commit it, with no Apply button. Commit points per control kind: toggles and dropdowns write on change, sliders write on release (the value previews live while dragging), text inputs write on Enter.

## While the window is open

While the window is open the terminal is hidden: a normal window can never stack above a layer surface on COSMIC, so opening settings hides the drop-down through its normal hide path (animated when slides are enabled) - the same resolution as Guake's default hide-on-focus-loss for its prefs. The rules:

- Any show/toggle intent while the window is open (hotkey, second launch) closes the settings window first, then shows the terminal.
- Closing the window does not re-show the terminal and does not exit the app; the next toggle brings the terminal back with every edit already applied.
- The hide on open is unconditional - it does not touch `hide_on_focus_loss`, which keeps governing ordinary focus loss once the window is closed.

## When each edit takes effect

Each control states next to it when its edit takes effect, and because the terminal is hidden while the window is open, even the live keys land visibly at the next show:

| Field | Applies |
|---|---|
| `height_percent`, `width_percent`, `h_align`, `edge` | live - reshapes a shown surface in place (deferred mid-drag and mid-slide); visible at the next show while the window is open |
| `tab_position` | live - pure relayout, no reshape: the strip reorders immediately and nothing resizes; visible at the next show while the window is open |
| `tab_title_mode` | live - every un-renamed tab re-titles from its stored shell title and cwd the moment the pick lands; visible at the next show while the window is open |
| `font_name`, `font_size`, `opacity`, `color_scheme_dark`, `color_scheme_light`, `theme_preference` | live, on every tab; visible at the next show while the window is open - the settings window itself restyles with a theme pick |
| `layer`, `keyboard_mode` | next show - a shown surface is never recreated under itself |
| `shell`, `args`, `scrollback_lines` | new tabs only |
| `hide_on_focus_loss`, `session_persistence_enabled`, `slide_animation_enabled`, `bell_notification_enabled`, `new_tab_after_current` | next trigger - the session toggle at the next save/restore cycle, the slide toggle at the next transition (a running animation is never aborted), the bell toggle at the next bell, the placement toggle at the next new-tab open |

These Behavior toggles default to `true`, which is exactly the pre-settings behavior:

- `session_persistence_enabled` is symmetric: `false` stops session saves *and* skips launch restore. The existing `session.ron` is never deleted or rewritten by disabling - re-enabling resumes saving over it.
- `slide_animation_enabled`: `false` restores instant show/hide with immediate destroy (the pre-slide-animation behavior).
- `new_tab_after_current`: `false` appends new tabs at the end of the tab list (Guake's default ordering) instead of directly after the active tab. `Ctrl+Shift+T`, the tab bar's `+`, and the remote `new-tab` follow it; `duplicate-tab` always lands after the active tab, and session restore keeps the saved order.

## Input ranges and limitations

Controls constrain input to the same ranges as the config (`height_percent` 10-100, `width_percent` 30-100, `opacity` 0-100, `scrollback_lines` non-negative); the write path never persists an out-of-range value. One known limitation: the `args` input splits on whitespace with no quoting support - arguments containing spaces stay config-file-only.
