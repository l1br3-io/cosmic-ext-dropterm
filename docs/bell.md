# Bell notifications

A bell that rings while the terminal is hidden posts a desktop notification naming the tab - the one desktop-environment feature a hidden drop-down uniquely needs.

A bell that rings while the terminal is *hidden* posts a desktop notification naming the tab's title - the one DE feature a hidden drop-down uniquely needs (`printf '\a'` in a long-running build now finds you). Delivery goes straight to the freedesktop notification interface over D-Bus, which cosmic-notifications serves on COSMIC; COSMIC implements no Notification portal, so no portal is involved. The rules:

- Only while hidden: when the overlay is visible, the tab-bar 🔔 marker alone carries the signal.
- One notification per tab per hide period: a bell storm (`while true; printf '\a'; done`) yields a single notification - the per-tab latch clears the next time the terminal is shown.
- Toggle with `bell_notification_enabled` (default `true`, Behavior section in the [settings drawer](settings.md)); it applies at the next bell - the tab-bar markers are unaffected either way.
- No notification daemon: the post fails with a `warn` log; nothing surfaces and the terminal is unaffected.

## Finished-command notifications

A command that finishes while the terminal is hidden posts its own desktop notification - run the build, hide the terminal, get told. The notification names the tab and reports the command's duration in the status band's own `m:ss` / `h:mm:ss` shape, naming the command itself when the app sampled one while it ran ("cargo finished in 2:41"); an unsampled run falls back to "Command finished in 2:41" - the same honesty as the band's frozen pairing. The rules:

- Only while hidden: a visible terminal already carries the signal - the status band's frozen duration segment on the active tab, the activity marker on the rest.
- Only past the threshold: a hidden `ls` that finishes never pings. `command_finish_notification_after_secs` (default `5` seconds) sets the minimum duration; a finish exactly at the threshold counts.
- One notification per tab per hide period: the per-tab latch - the bell's exact shape - clears the next time the terminal is shown, so the next hidden long command notifies again.
- Toggle with `command_finish_notification_enabled` (default `true`, Behavior section in the [settings drawer](settings.md)); both keys apply at the next finish.
