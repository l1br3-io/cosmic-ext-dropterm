// SPDX-License-Identifier: GPL-3.0-only

use alacritty_terminal::term::{self as aterm};
use cosmic::app::Task;

use super::{DropTerm, Message, resolved_profile};
use crate::terminal;
// Only the impure shell lives here; the pure decisions sit in
// crate::theme_glue so they stay headless-testable.
pub(crate) use crate::theme_glue::{
    ThemeOverrideKind, effective_colors, effective_is_dark, theme_override, window_bg_color,
};

/// The system palette for `is_dark`: the palette source for the
/// window-background fallback in [`Self::refresh_window_bg_color`].
/// Deliberately not [`cosmic::theme::active`]; that fn's race note is
/// why the caller's mode decision, never the process global, picks
/// the palette. Free-standing so the mapping is headless-testable.
pub(super) fn system_palette_for_mode(is_dark: bool) -> cosmic::Theme {
    if is_dark {
        cosmic::theme::system_dark()
    } else {
        cosmic::theme::system_light()
    }
}

/// The impure shell over [`theme_override`]: hands the runtime the
/// user's own system palette (never libcosmic's built-in constants),
/// marked through `ThemeType::prefer_dark` (which only writes
/// through on the `System` themes these constructors produce), then
/// chains a [`Message::ReapplyColors`] echo so the window-background
/// derivation, the one consumer that reads the global, sees the new
/// mode. The follow-up rides [`Task::chain`], never [`Task::then`]:
/// `then` consumes the first task's output, so the
/// `Action::AppThemeChange` message would never reach the runtime;
/// `chain` forwards every output, preserving both messages and their
/// order.
pub(super) fn theme_override_task(kind: ThemeOverrideKind) -> Task<Message> {
    let (mut theme, prefer_dark) = match kind {
        ThemeOverrideKind::PinDark => (cosmic::theme::system_dark(), Some(true)),
        ThemeOverrideKind::PinLight => (cosmic::theme::system_light(), Some(false)),
        ThemeOverrideKind::FollowSystem => (cosmic::theme::system_preference(), None),
    };
    theme.theme_type.prefer_dark(prefer_dark);
    cosmic::command::set_theme(theme).chain(Task::done(cosmic::Action::App(Message::ReapplyColors)))
}

impl DropTerm {
    /// Applies the current config and theme to every tab's terminal:
    /// each tab resolves its own colors and font (a profile tab its
    /// profile's picks, every other tab the global ones), so one pass
    /// can paint a mixed pair differently; the window background
    /// follows the active tab's resolution.
    pub(super) fn apply_config_to_tabs(&mut self) {
        // The window background must refresh BEFORE any tab's rewalk:
        // `set_config`'s rebuild snapshots the fallback background per
        // cell (`convert_color` reads the process global live), so a
        // stale global at rewalk time bakes previous-theme cells that
        // only later grid damage heals.
        self.refresh_active_window_bg();
    }

    /// Refreshes the process-global window background from the
    /// ACTIVE tab's resolved colors: the fallback is one value, so it
    /// belongs to the tab on screen; every active-tab change and
    /// every config pass funnels through here. No tabs (startup, or
    /// momentarily mid-close) resolves the global pick.
    ///
    /// A moved background invalidates every tab whose scheme defers
    /// it, so the store is followed by one `set_config` pass over all
    /// tabs; the engine's `update_default_colors` rewalks only a tab
    /// whose default metadata actually moved, which is what heals a
    /// plain tab switched in under a different window background.
    pub(super) fn refresh_active_window_bg(&self) {
        // The config preference decides the mode here, not a global
        // read: a pick must apply its colors in this same pass,
        // before the async `set_theme` task lands and moves the
        // global.
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

    /// Stores the effective window background into the process global
    /// (`terminal::WINDOW_BG_COLOR`), the fallback for cells whose
    /// color scheme leaves background unset (mirrors cosmic-term's
    /// `update_config`); refresh it whenever the active tab's
    /// resolution, the global scheme, or the COSMIC theme changes.
    ///
    /// The fallback derives from the palette matching `is_dark`, the
    /// caller's `effective_is_dark` decision, never from
    /// [`cosmic::theme::active`]: with a pinned mode, libcosmic's
    /// theme watcher (built before the pin lands) can deliver the
    /// opposite mode's palette, and its handler writes that palette
    /// into the process global while keeping the `prefer_dark`
    /// marker, so `active()` reports the wrong mode with the pin
    /// still set and later refreshes would derive the opposite-mode
    /// background under cells that correctly resolve the pinned
    /// scheme. Following the caller's decision keeps the fallback
    /// and the cells in step whatever the global says.
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

    /// Drives a task's stream to completion, collecting every output
    /// in delivery order: `Task::then` consumes the first task's
    /// output, so only a stream-level check can prove a message
    /// actually ships.
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
        // `Task::then` would consume the `set_theme` output: the pin
        // never reaches the runtime, and pinning Dark leaves a light,
        // frosted terminal. The task must deliver both messages,
        // override first, echo second.
        for (kind, expected_prefer) in [
            (ThemeOverrideKind::PinDark, Some(true)),
            (ThemeOverrideKind::PinLight, Some(false)),
            // FollowSystem's palette follows the host; only the
            // cleared marker is environment-independent.
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
            // must be System-typed with the exact marker;
            // FollowSystem on a host with no theme-mode source
            // (headless CI) falls back to a plain `Theme::dark()`,
            // with no System typing and no marker to clear.
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
