# Settings drawer

Every DropTerm setting is editable in this in-app drawer, and every edit applies the moment you commit it - there is no Apply button.

## Opening the drawer

Every setting is editable in-app: press `Ctrl+Shift+I` while the terminal has keyboard focus, pick "Settings…" in the terminal's context menu, or click the gear button at the right corner of the tab strip - all three hit the same toggle. The drawer is an overlay pinned to the right edge of the drop-down surface itself (the libcosmic context drawer cosmic-term uses), so the terminal stays visible - and live-editable - the whole time: opening settings never hides or resizes the surface, and the pty grid is untouched by drawer open/close. The strip is always present whatever the tab count - with a single tab it carries the settings button alone - so settings are one click away in every state; hovering the button shows a `Settings…` tooltip. The keypress is swallowed from the shell like every other app bind. A second press toggles the drawer closed; there is never a second window.

## Sections and commit points

The drawer shows five sections - Appearance (font, size, opacity, the two color scheme dropdowns, and the Application theme dropdown), Geometry, Behavior (hide on focus loss, session persistence, slide animation, bell notifications, new-tab placement), Terminal, and Advanced (`respect_panel`, `layer`, `keyboard_mode`) - and applies instantly: every control writes its key the moment you commit it, with no Apply button. Commit points per control kind: toggles and dropdowns write on change, sliders write on release (the value previews live while dragging), text inputs write on Enter.

## While the drawer is open

While the drawer is open, terminal input is inert - keyboard and pointer never reach the shell; the drawer's own inputs type normally. Escape closes the drawer (only the drawer: the surface stays), as does the drawer's Close button and a repeat `Ctrl+Shift+I`. Focus loss keeps working as configured: with `hide_on_focus_loss` the surface hides with the drawer preserved exactly as it was - open, drafts and all - and the next show uncovers it; explicitly closing the drawer clears any in-flight drafts, so a reopen starts fresh from the config file.

## When each edit takes effect

Each control states next to it when its edit takes effect, and because the terminal is visible under the drawer, the live keys are visible the moment they land:

| Field | Applies |
|---|---|
| `height_percent`, `width_percent`, `h_align`, `edge` | live - reshapes a shown surface in place (deferred mid-drag and mid-slide) |
| `tab_position` | live - pure relayout, no reshape: the strip reorders immediately and nothing resizes |
| `tab_title_mode` | live - every un-renamed tab re-titles from its stored shell title and cwd the moment the pick lands |
| `font_name`, `font_size`, `opacity`, `color_scheme_dark`, `color_scheme_light`, `theme_preference` | live, on every tab, visible in the terminal beneath the open drawer - drag the opacity slider and watch the surface change while dragging |
| `layer`, `keyboard_mode` | next show - a shown surface is never recreated under itself |
| `shell`, `args`, `scrollback_lines` | new tabs only |
| `hide_on_focus_loss`, `session_persistence_enabled`, `slide_animation_enabled`, `bell_notification_enabled`, `new_tab_after_current` | next trigger - the session toggle at the next save/restore cycle, the slide toggle at the next transition (a running animation is never aborted), the bell toggle at the next bell, the placement toggle at the next new-tab open |

These Behavior toggles default to `true`, which is exactly the pre-settings behavior:

- `session_persistence_enabled` is symmetric: `false` stops session saves *and* skips launch restore. The existing `session.ron` is never deleted or rewritten by disabling - re-enabling resumes saving over it.
- `slide_animation_enabled`: `false` restores instant show/hide with immediate destroy (the pre-slide-animation behavior).
- `new_tab_after_current`: `false` appends new tabs at the end of the tab list (Guake's default ordering) instead of directly after the active tab. `Ctrl+Shift+T`, the tab bar's `+`, and the remote `new-tab` follow it; `duplicate-tab` always lands after the active tab, and session restore keeps the saved order.

## Input ranges and limitations

Controls constrain input to the same ranges as the config (`height_percent` 10-100, `width_percent` 30-100, `opacity` 0-100, `scrollback_lines` non-negative); the write path never persists an out-of-range value. One known limitation: the `args` input splits on whitespace with no quoting support - arguments containing spaces stay config-file-only.
