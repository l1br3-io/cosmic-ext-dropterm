// SPDX-License-Identifier: GPL-3.0-only

use alacritty_terminal::term::{self as aterm};
use cosmic::app::Task;

use super::{DropTerm, Message, resolved_profile};
use crate::terminal;
// The pure decisions moved to core (shared-core spec); this file
// keeps the impure shell — the palette reader and the runtime task.
pub(crate) use crate::theme_glue::{
    ThemeOverrideKind, effective_colors, effective_is_dark, theme_override, window_bg_color,
};

/// The user's own system palette for the effective mode — the source
/// of the window-background fallback derivation
/// ([`Self::refresh_window_bg_color`]). Reads the palette files via
/// cosmic-config, with the built-in fallback on failure, exactly like
/// [`theme_override_task`] does. Deliberately NOT
/// [`cosmic::theme::active`]: the process global can sit on the
/// opposite mode's palette while the preference pin is still "set"
/// (see [`Self::refresh_window_bg_color`]'s race note), so the
/// fallback follows the mode the caller decided, the same way
/// [`effective_colors`] does. Free-standing so the mode→palette
/// mapping is headless-pinnable.
pub(super) fn system_palette_for_mode(is_dark: bool) -> cosmic::Theme {
    if is_dark {
        cosmic::theme::system_dark()
    } else {
        cosmic::theme::system_light()
    }
}

/// The impure shell over [`theme_override`]: builds the override
/// theme from the user's own system palettes — never libcosmic's
/// built-in constants — marks it through `ThemeType::prefer_dark`
/// (which only writes through on `System`, the only theme type these
/// constructors produce), and hands it to the runtime's global.
/// Everything decidable is decided in the pure fns above; the
/// chained [`Message::ReapplyColors`] re-runs the color apply once
/// the global has landed, so the window-background derivation — the
/// one consumer that must read the global — sees the new mode.
///
/// The follow-up rides [`Task::chain`], never [`Task::then`]: `then`
/// CONSUMES the first task's output and replaces it with the
/// continuation's, so `set_theme(...).then(...)` would silently drop
/// the `Action::AppThemeChange` message — the pin would never reach
/// the runtime and the theme would stay whatever it was (the
/// "dark mode does nothing" bug). `chain` forwards every output of
/// the first task and only then runs the second, preserving both
/// messages and their order. Pinned by
/// `theme_override_task_delivers_set_theme_then_reapply`.
pub(super) fn theme_override_task(kind: ThemeOverrideKind) -> Task<Message> {
    let (mut theme, prefer_dark) = match kind {
        ThemeOverrideKind::PinDark => (cosmic::theme::system_dark(), Some(true)),
        ThemeOverrideKind::PinLight => (cosmic::theme::system_light(), Some(false)),
        // Restore: re-read the system mode from cosmic-config fresh
        // and clear the marker, so system flips resume driving the
        // app.
        ThemeOverrideKind::FollowSystem => (cosmic::theme::system_preference(), None),
    };
    theme.theme_type.prefer_dark(prefer_dark);
    cosmic::command::set_theme(theme).chain(Task::done(cosmic::Action::App(Message::ReapplyColors)))
}

impl DropTerm {
    /// Applies the current config and theme to every tab's terminal.
    /// Colors and font resolve per tab through the
    /// [`resolved_profile`]/[`effective_colors`] chain — a profile
    /// tab runs its profile's scheme and font, every other tab the
    /// global picks — so one pass can paint a mixed pair differently;
    /// the window background then follows the active tab's
    /// resolution, keeping chrome and unset-bg cells in step with
    /// whichever tab is on screen.
    pub(super) fn apply_config_to_tabs(&mut self) {
        // The window background must move BEFORE any tab's rewalk:
        // `set_config`'s rebuild snapshots the window-bg fallback per
        // cell (`convert_color` reads the process-global live), so the
        // refresh-then-rewalk order inside [`Self::refresh_active_window_bg`]
        // is what makes a theme flip land in one frame. The old
        // refresh-after order left every default-background cell — and
        // the full-surface default quad — wearing the previous theme's
        // background, healed only where later grid damage rewalked a
        // line: the grayish cover over the editor on an Application
        // theme flip, receding on focus and leaving one-line fragments
        // (hardware walk 2026-10-08).
        self.refresh_active_window_bg();
    }

    /// Refreshes the process-global window background from the
    /// ACTIVE tab's resolved colors (the profile-appearance ruling):
    /// the fallback background is one value, so it must belong to the
    /// tab actually on screen — every `active_tab` assignment
    /// and every config pass funnels through here. No tabs
    /// (startup, or momentarily mid-close) resolves the global
    /// pick, the pre-initiative behavior.
    ///
    /// A moved window background also invalidates every tab whose
    /// scheme defers it (the COSMIC pair), whose cell snapshots and
    /// default quad resolved the old value at their last rewalk — so
    /// the store is followed by one `set_config` pass over all tabs.
    /// The engine's `update_default_colors` reads the global live and
    /// only rewalks when a tab's default metadata actually moved:
    /// scheme-owning tabs compare equal and update nothing, and the
    /// pass is what heals a plain tab switched in under a different
    /// window background (the tab-switch door into the same stale
    /// snapshot as the theme flip).
    pub(super) fn refresh_active_window_bg(&self) {
        // The config preference is the source of truth:
        // `effective_is_dark`, not a direct global read, decides the
        // mode, so a pick applies colors in this same pass — before
        // the async `set_theme` task lands and moves the global.
        let is_dark = effective_is_dark(self.config.theme_preference, cosmic::theme::is_dark());
        let active_profile = resolved_profile(
            &self.config,
            self.active().and_then(|tab| tab.profile.as_deref()),
        );
        Self::refresh_window_bg_color(
            &effective_colors(&self.config, active_profile, is_dark),
            is_dark,
        );
        for tab in &self.tabs {
            let profile = resolved_profile(&self.config, tab.profile.as_deref());
            let options = self.terminal_options_for(profile);
            let colors = effective_colors(&self.config, profile, is_dark);
            tab.terminal.lock().unwrap().set_config(&options, &colors);
        }
    }

    /// Mirrors cosmic-term's own `update_config`: the effective window
    /// background color is a process-global (`terminal::WINDOW_BG_COLOR`)
    /// used as a fallback wherever a color scheme leaves "background"
    /// unset, so it must be refreshed whenever the active tab's
    /// resolution, the global scheme, or the COSMIC theme changes.
    ///
    /// The fallback is derived from the palette matching `is_dark` — the
    /// caller's `effective_is_dark` decision — via
    /// [`cosmic::theme::system_dark`]/[`cosmic::theme::system_light`],
    /// the same palettes [`theme_override_task`] pins. It must NOT read
    /// [`cosmic::theme::active`]: when the preference pins a mode, a
    /// startup race in libcosmic's theme watcher can deliver the other
    /// mode's `SystemThemeChange` (the watcher is built before the pin
    /// lands), and its handler writes the delivered palette into the
    /// process global while preserving only the `prefer_dark` marker —
    /// `active()` then reports the wrong mode with the pin still "set",
    /// and every later refresh re-derives the opposite-mode background
    /// under cells that correctly resolve the pinned scheme (the
    /// white-background-in-dark-mode bug, hardware walk 2026-10-12).
    /// Deriving from the mode the same way `effective_colors` does
    /// keeps the fallback and the cells in step whatever the global
    /// says.
    pub(super) fn refresh_window_bg_color(colors: &aterm::color::Colors, is_dark: bool) {
        let theme = system_palette_for_mode(is_dark);
        let color = cosmic::iced::Color::from(theme.cosmic().background(theme.transparent).base);
        let bytes = color.into_rgba8();
        let system_bg = u32::from(bytes[2])
            | (u32::from(bytes[1]) << 8)
            | (u32::from(bytes[0]) << 16)
            | 0xFF00_0000;
        let data = window_bg_color(colors, system_bg);
        terminal::WINDOW_BG_COLOR.store(data, std::sync::atomic::Ordering::SeqCst);
    }

    /// The theme override landed: re-run the idempotent
    /// color apply so `refresh_window_bg_color` derives
    /// the fallback background from the new global mode;
    /// cells were already correct via `effective_is_dark`.
    pub(super) fn reapply_colors(&mut self) {
        self.apply_config_to_tabs();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives a [`Task`](cosmic::app::Task)'s stream to completion on
    /// the current thread, collecting every runtime
    /// [`Action::Output`] message in delivery order. `Task::then`
    /// consumes the first task's output and replaces it with the
    /// continuation's, so only a stream-level check can prove a
    /// message actually ships.
    fn delivered_outputs<T: Send + 'static>(task: cosmic::app::Task<T>) -> Vec<cosmic::Action<T>> {
        use cosmic::iced::futures::StreamExt;
        use cosmic::iced::futures::executor::block_on;

        let mut stream = cosmic::iced::runtime::task::into_stream(task)
            .expect("theme override task has a stream");
        let mut outputs = Vec::new();
        block_on(async {
            while let Some(action) = stream.next().await {
                if let cosmic::iced::runtime::Action::Output(message) = action {
                    outputs.push(message);
                }
            }
        });
        outputs
    }

    #[test]
    fn theme_override_task_delivers_set_theme_then_reapply() {
        // `Task::then` would CONSUME the `set_theme` output — the
        // pin message would never reach the runtime, the process
        // theme would never flip, and pinning Dark would leave a
        // light, frosted terminal. The task must deliver BOTH
        // messages: the theme override first, the `ReapplyColors`
        // echo second — `Task::chain`'s contract.
        for (kind, expected_prefer) in [
            (ThemeOverrideKind::PinDark, Some(true)),
            (ThemeOverrideKind::PinLight, Some(false)),
            // FollowSystem's palette is whatever the system prefers —
            // only the cleared marker is environment-independent.
            (ThemeOverrideKind::FollowSystem, None),
        ] {
            let outputs = delivered_outputs(theme_override_task(kind));
            assert_eq!(
                outputs.len(),
                2,
                "expected exactly the set_theme + ReapplyColors pair for {kind:?}"
            );
            let cosmic::Action::Cosmic(cosmic::app::Action::AppThemeChange(theme)) = &outputs[0]
            else {
                panic!("first delivered message must be the theme override for {kind:?}");
            };
            // The pins construct their System theme outright, so they
            // must always be System-typed with the exact marker.
            // FollowSystem re-reads the system mode, and on a host
            // with no theme-mode source (headless CI)
            // `system_preference()` falls back to a plain
            // `Theme::dark()` — no System typing, no marker to clear.
            match (&theme.theme_type, expected_prefer) {
                (cosmic::theme::ThemeType::System { prefer_dark, .. }, _) => {
                    assert_eq!(*prefer_dark, expected_prefer, "marker for {kind:?}");
                }
                (_, None) => {}
                _ => panic!("override theme must be System-typed for {kind:?}"),
            }
            assert!(
                matches!(&outputs[1], cosmic::Action::App(Message::ReapplyColors)),
                "second delivered message must be the ReapplyColors echo for {kind:?}"
            );
        }
    }
}
