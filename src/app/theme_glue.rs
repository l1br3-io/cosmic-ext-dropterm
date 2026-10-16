// SPDX-License-Identifier: GPL-3.0-only

use alacritty_terminal::term::{self as aterm};
use alacritty_terminal::vte::ansi::NamedColor;
use cosmic::app::Task;

use super::Message;
use crate::config::{DropTermConfig, ThemePreference};
use crate::profile;
use crate::terminal::theme_source;

/// The window-background source decision: a scheme that resolves a
/// background owns the window background — the tab-bar/grab-bar
/// chrome and the unset-bg cell fallback all read `WINDOW_BG_COLOR`,
/// so they follow by construction. A scheme without one (the COSMIC
/// pair, and any unknown name that fell back to it) defers to the
/// system-theme derivation, passed in already packed so this stays a
/// pure, headless-testable decision.
pub(super) fn window_bg_color(colors: &aterm::color::Colors, system_bg: u32) -> u32 {
    match colors[NamedColor::Background] {
        Some(rgb) => {
            0xFF00_0000 | (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b)
        }
        None => system_bg,
    }
}

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

/// The colors a tab under `profile` renders with — the color arm of
/// the per-tab resolution both application points (spawn and the
/// live config-apply pass) consume:
/// [`profile::effective_scheme_name`] resolves the scheme name (the
/// profile's explicit pick in both system modes, else the global
/// pair for `is_dark`), and the registry lookup falls back to the
/// COSMIC pair for the mode on any miss — unknown profile names and
/// unknown global names alike, exactly as the global pick itself
/// always has. The chain never errors, and the store is never
/// rewritten to "fix" a stored name.
pub(super) fn effective_colors(
    config: &DropTermConfig,
    profile: Option<&profile::Profile>,
    is_dark: bool,
) -> aterm::color::Colors {
    let name = profile::effective_scheme_name(
        profile,
        &config.color_scheme_dark,
        &config.color_scheme_light,
        is_dark,
    );
    theme_source::by_name(name).unwrap_or_else(|| {
        if is_dark {
            theme_source::cosmic_dark()
        } else {
            theme_source::cosmic_light()
        }
    })
}

/// Whether the app resolves colors for dark mode right now. The
/// config preference is the source of truth; the global theme (what
/// widgets read) follows via the `set_theme` task, so color
/// decisions must not wait for it. Call sites inject the current
/// system value — the [`window_bg_color`] pattern — keeping the
/// decision pure and headless-testable.
pub(super) fn effective_is_dark(preference: ThemePreference, system_is_dark: bool) -> bool {
    match preference {
        ThemePreference::System => system_is_dark,
        ThemePreference::Dark => true,
        ThemePreference::Light => false,
    }
}

/// What a theme-preference transition asks the runtime to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThemeOverrideKind {
    /// Pin the app to the user's dark system palette.
    PinDark,
    /// Pin the app to the user's light system palette.
    PinLight,
    /// Clear the pin and follow the system mode again.
    FollowSystem,
}

/// The one pure transition matrix shared by `init` and
/// `ConfigChanged`: `None` when the preference did not change —
/// steady state issues no task, so a default `System` config never
/// calls `set_theme` and unrelated config echoes never storm one.
pub(super) fn theme_override(
    old: ThemePreference,
    new: ThemePreference,
) -> Option<ThemeOverrideKind> {
    match (old, new) {
        (ThemePreference::System, ThemePreference::System)
        | (ThemePreference::Dark, ThemePreference::Dark)
        | (ThemePreference::Light, ThemePreference::Light) => None,
        (_, ThemePreference::Dark) => Some(ThemeOverrideKind::PinDark),
        (_, ThemePreference::Light) => Some(ThemeOverrideKind::PinLight),
        (ThemePreference::Dark | ThemePreference::Light, ThemePreference::System) => {
            Some(ThemeOverrideKind::FollowSystem)
        }
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

#[cfg(test)]
mod tests {
    use super::super::resolved_profile;
    use super::super::tests::{chain_config, chain_profile, rgb};
    use super::*;

    // --- the window-background source decision ---

    #[test]
    fn scheme_background_wins_over_the_system_derivation() {
        // Dracula owns its background (#282a36, alacritty-theme
        // Dracula.toml): the resolved slot must land in
        // WINDOW_BG_COLOR's packed layout (0xAARRGGBB, opaque), not
        // the system theme's — chrome and the unset-bg fallback then
        // follow by construction.
        let dracula = theme_source::by_name("Dracula").unwrap();
        assert_eq!(window_bg_color(&dracula, 0xFF1E_1E2E), 0xFF28_2A36);
    }

    #[test]
    fn unset_background_passes_the_system_derivation_through() {
        // The COSMIC pair owns no background (the system theme
        // supplies it), so the decision is a plain passthrough —
        // default-config chrome follows the system theme, and a
        // bogus scheme name keeps that too (the `active_colors`
        // fallback resolves to this same pair).
        let cosmic_dark = theme_source::by_name("COSMIC Dark").unwrap();
        assert_eq!(window_bg_color(&cosmic_dark, 0xFF1E_1E2E), 0xFF1E_1E2E);
    }

    #[test]
    fn the_fallback_palette_matches_the_effective_mode() {
        // The window-background fallback derives from the palette of the
        // mode the caller decided (`effective_is_dark`), never from the
        // process-global active theme — a post-pin startup race can leave
        // the global on the opposite palette with the pin still "set"
        // (hardware walk 2026-10-12: dark preference, light global,
        // pale pane). Both arms resolve a palette whose own `is_dark`
        // agrees with the requested mode, whatever cosmic-config returns
        // (built-in fallbacks agree too), so a drifted global cannot
        // leak into the fallback.
        assert!(system_palette_for_mode(true).cosmic().is_dark);
        assert!(!system_palette_for_mode(false).cosmic().is_dark);
    }

    // --- the per-tab appearance resolution chain ---
    //
    // `resolved_profile` + `effective_colors` is the one chain both
    // application points consume (spawn and the config-apply pass):
    // the tab's recorded profile id → the profile → the seams → the
    // built-in registry. Every miss falls back to the global pick;
    // the chain never errors. Nord plays the global below — its
    // slots differ from Dracula's everywhere asserted, so a silent
    // fallback to the scheme (or a scheme leaking into a fallback)
    // cannot pass.

    #[test]
    fn effective_colors_resolves_a_known_scheme_name() {
        let config = chain_config(&[chain_profile("ssh-prod", Some("Dracula"))]);
        let profile = resolved_profile(&config, Some("ssh-prod"));
        let colors = effective_colors(&config, profile, true);
        // Dracula owns bg #282a36 and red #ff5555 (alacritty-theme).
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x28_2A36)));
        assert_eq!(colors[NamedColor::Red], Some(rgb(0xFF_5555)));
    }

    #[test]
    fn effective_colors_falls_back_to_global_for_a_plain_tab() {
        // No recorded id — plain shell tabs, and the degraded preset
        // arm whose blank command line records none (the shared
        // `profile_id = None` rule): the global pick, unchanged.
        let config = chain_config(&[chain_profile("ssh-prod", Some("Dracula"))]);
        let colors = effective_colors(&config, resolved_profile(&config, None), true);
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
    }

    #[test]
    fn effective_colors_falls_back_to_global_on_a_dangling_profile_id() {
        // The tab records a profile the config no longer carries
        // (deleted mid-run): the lookup is `None`, the global pick —
        // never an error, never a rewrite.
        let config = chain_config(&[chain_profile("ssh-prod", Some("Dracula"))]);
        assert_eq!(resolved_profile(&config, Some("ghost")), None);
        let colors = effective_colors(&config, resolved_profile(&config, Some("ghost")), true);
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
    }

    #[test]
    fn effective_colors_falls_back_to_global_when_the_profile_sets_no_scheme() {
        // A profile without a scheme override: its tabs follow the
        // global pick.
        let config = chain_config(&[chain_profile("ssh-prod", None)]);
        let profile = resolved_profile(&config, Some("ssh-prod"));
        let colors = effective_colors(&config, profile, true);
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
    }

    #[test]
    fn effective_colors_falls_back_to_global_on_an_unknown_scheme_name() {
        // A hand-edited config can name a scheme the registry does
        // not ship ("Catppuccin Frappe" is a real upstream variant):
        // resolve global, silently — exactly how an unknown global
        // name falls to the COSMIC pair — never an error, never a
        // rewrite.
        let config = chain_config(&[chain_profile("ssh-prod", Some("Catppuccin Frappe"))]);
        let profile = resolved_profile(&config, Some("ssh-prod"));
        let colors = effective_colors(&config, profile, true);
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
    }

    #[test]
    fn one_config_pass_resolves_mixed_tabs_to_different_colors() {
        // The apply criterion's headless pin: one config, one global
        // pick, two recorded ids (a profile tab and a plain tab) —
        // the per-tab resolutions differ, so one `apply_config_to_tabs`
        // pass paints them differently.
        let config = chain_config(&[chain_profile("ssh-prod", Some("Dracula"))]);
        let profile_tab =
            effective_colors(&config, resolved_profile(&config, Some("ssh-prod")), true);
        let plain_tab = effective_colors(&config, resolved_profile(&config, None), true);
        assert_ne!(
            profile_tab[NamedColor::Background],
            plain_tab[NamedColor::Background]
        );
        assert_ne!(profile_tab[NamedColor::Red], plain_tab[NamedColor::Red]);
    }

    // --- the pure theme-preference decisions ---

    #[test]
    fn effective_is_dark_pins_dark_and_light() {
        // A pin wins regardless of the system mode — that is the
        // whole point of the feature.
        assert!(effective_is_dark(ThemePreference::Dark, false));
        assert!(effective_is_dark(ThemePreference::Dark, true));
        assert!(!effective_is_dark(ThemePreference::Light, false));
        assert!(!effective_is_dark(ThemePreference::Light, true));
    }

    #[test]
    fn effective_is_dark_system_follows_the_injected_mode() {
        // The system value is a parameter, not a global read (the
        // same pattern as `window_bg_color`): the decision stays
        // pure, and tests never depend on the process-wide THEME
        // state.
        assert!(effective_is_dark(ThemePreference::System, true));
        assert!(!effective_is_dark(ThemePreference::System, false));
    }

    #[test]
    fn theme_override_is_none_when_preference_is_unchanged() {
        // Steady state issues nothing: a default `System` config
        // never calls `set_theme`, and unrelated config echoes —
        // including our own theme echo — never storm the task.
        assert_eq!(
            theme_override(ThemePreference::System, ThemePreference::System),
            None
        );
        assert_eq!(
            theme_override(ThemePreference::Dark, ThemePreference::Dark),
            None
        );
        assert_eq!(
            theme_override(ThemePreference::Light, ThemePreference::Light),
            None
        );
    }

    #[test]
    fn theme_override_pins_on_any_transition_to_dark_or_light() {
        assert_eq!(
            theme_override(ThemePreference::System, ThemePreference::Dark),
            Some(ThemeOverrideKind::PinDark)
        );
        assert_eq!(
            theme_override(ThemePreference::Light, ThemePreference::Dark),
            Some(ThemeOverrideKind::PinDark)
        );
        assert_eq!(
            theme_override(ThemePreference::System, ThemePreference::Light),
            Some(ThemeOverrideKind::PinLight)
        );
        assert_eq!(
            theme_override(ThemePreference::Dark, ThemePreference::Light),
            Some(ThemeOverrideKind::PinLight)
        );
    }

    #[test]
    fn theme_override_restores_follow_system_from_a_pin() {
        assert_eq!(
            theme_override(ThemePreference::Dark, ThemePreference::System),
            Some(ThemeOverrideKind::FollowSystem)
        );
        assert_eq!(
            theme_override(ThemePreference::Light, ThemePreference::System),
            Some(ThemeOverrideKind::FollowSystem)
        );
    }

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
