# Bell notifications

A bell that rings while the terminal is hidden posts a desktop notification naming the tab - the one desktop-environment feature a hidden drop-down uniquely needs.

A bell that rings while the terminal is *hidden* posts a desktop notification naming the tab's title - the one DE feature a hidden drop-down uniquely needs (`printf '\a'` in a long-running build now finds you). Delivery goes straight to the freedesktop notification interface over D-Bus, which cosmic-notifications serves on COSMIC; COSMIC implements no Notification portal, so no portal is involved. The rules:

- Only while hidden: when the overlay is visible, the tab-bar 🔔 marker alone carries the signal.
- One notification per tab per hide period: a bell storm (`while true; printf '\a'; done`) yields a single notification - the per-tab latch clears the next time the terminal is shown.
- Toggle with `bell_notification_enabled` (default `true`, Behavior section in the [settings drawer](settings.md)); it applies at the next bell - the tab-bar markers are unaffected either way.
- No notification daemon: the post fails with a `warn` log; nothing surfaces and the terminal is unaffected.
