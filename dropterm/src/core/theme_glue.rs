// SPDX-License-Identifier: GPL-3.0-only

//! Theme glue's pure decisions (shared-core spec): the
//! window-background source decision, the per-tab color resolution
//! chain, the effective-mode decision, and the theme-preference
//! transition matrix. The system-palette reader and the runtime
//! `set_theme` task stay with the libcosmic frontend, which passes
//! their answers in already packed — every decision here is pure and
//! headless-testable.

use alacritty_terminal::term::{self as aterm};
use alacritty_terminal::vte::ansi::NamedColor;

use crate::core::config::{DropTermConfig, ThemePreference};
use crate::core::profile;
use crate::core::theme_source;

/// The window-background source decision: a scheme that resolves a
/// background owns the window background — the chrome and the
/// unset-bg cell fallback all read the engine's `WINDOW_BG_COLOR`,
/// so they follow by construction. A scheme without one (the COSMIC
/// pair, and any unknown name that fell back to it) defers to the
/// system-theme derivation, passed in already packed so this stays a
/// pure, headless-testable decision.
pub fn window_bg_color(colors: &aterm::color::Colors, system_bg: u32) -> u32 {
    match colors[NamedColor::Background] {
        Some(rgb) => {
            0xFF00_0000 | (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b)
        }
        None => system_bg,
    }
}

/// The colors a tab under `profile` renders with — the color arm of
/// the per-tab resolution both application points (spawn and the
/// live config-apply pass) consume:
/// [`profile::effective_scheme_name`] resolves the scheme name (the
/// profile's explicit pick in both system modes, else the global
/// pair for `is_dark`), and the registry lookup falls back to the
/// GitHub pair for the mode on any miss — unknown profile names and
/// unknown global names alike. The chain never errors, and the store
/// is never rewritten to "fix" a stored name.
pub fn effective_colors(
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
            theme_source::github_dark()
        } else {
            theme_source::github_light()
        }
    })
}

/// Whether the app resolves colors for dark mode right now. The
/// config preference is the source of truth; call sites inject the
/// current system value — the [`window_bg_color`] pattern — keeping
/// the decision pure and headless-testable.
pub fn effective_is_dark(preference: ThemePreference, system_is_dark: bool) -> bool {
    match preference {
        ThemePreference::System => system_is_dark,
        ThemePreference::Dark => true,
        ThemePreference::Light => false,
    }
}

/// What a theme-preference transition asks the runtime to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeOverrideKind {
    /// Pin the app to the user's dark system palette.
    PinDark,
    /// Pin the app to the user's light system palette.
    PinLight,
    /// Clear the pin and follow the system mode again.
    FollowSystem,
}

/// The one pure transition matrix shared by init and config echoes:
/// `None` when the preference did not change — steady state issues
/// no task, so a default `System` config never calls `set_theme` and
/// unrelated config echoes never storm one.
pub fn theme_override(old: ThemePreference, new: ThemePreference) -> Option<ThemeOverrideKind> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The test chain's config: a global dark pick (Nord) plus one
    /// profile per call.
    fn config_with(profile_scheme: Option<&str>) -> DropTermConfig {
        DropTermConfig {
            color_scheme_dark: "Nord".to_string(),
            profiles: vec![profile::Profile {
                name: "ssh-prod".to_string(),
                command: "ssh web-prod".to_string(),
                cwd: None,
                color_scheme: profile_scheme.map(str::to_string),
                env: None,
                font_size: None,
                scrollback_lines: None,
            }],
            ..DropTermConfig::default()
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "fixture: the low 24 bits of a packed 0xRRGGBB literal"
    )]
    fn rgb(v: u32) -> alacritty_terminal::vte::ansi::Rgb {
        alacritty_terminal::vte::ansi::Rgb {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
        }
    }

    #[test]
    fn scheme_background_wins_over_the_system_derivation() {
        let dracula = crate::core::theme_source::by_name("Dracula").unwrap();
        assert_eq!(window_bg_color(&dracula, 0xFF1E_1E2E), 0xFF28_2A36);
    }

    #[test]
    fn unset_background_passes_the_system_derivation_through() {
        // No shipped scheme leaves its background unset any more
        // (the OS-deferring COSMIC pair left the registry); the
        // pass-through arm stays for a raw `Colors` that has not
        // resolved a scheme yet.
        let unset = aterm::color::Colors::default();
        assert_eq!(window_bg_color(&unset, 0xFF1E_1E2E), 0xFF1E_1E2E);
    }

    #[test]
    fn effective_colors_resolves_a_known_scheme_name() {
        let config = config_with(Some("Dracula"));
        let colors = effective_colors(&config, profile::find(&config.profiles, "ssh-prod"), true);
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x28_2A36)));
        assert_eq!(colors[NamedColor::Red], Some(rgb(0xFF_5555)));
    }

    #[test]
    fn effective_colors_falls_back_to_global_for_a_plain_tab() {
        let config = config_with(Some("Dracula"));
        let colors = effective_colors(&config, None, true);
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
    }

    #[test]
    fn effective_colors_falls_back_to_global_on_a_dangling_profile_id() {
        let config = config_with(Some("Dracula"));
        assert!(profile::find(&config.profiles, "ghost").is_none());
        let colors = effective_colors(&config, profile::find(&config.profiles, "ghost"), true);
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
    }

    #[test]
    fn effective_colors_falls_back_to_global_when_the_profile_sets_no_scheme() {
        let config = config_with(None);
        let colors = effective_colors(&config, profile::find(&config.profiles, "ssh-prod"), true);
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
    }

    #[test]
    fn effective_colors_falls_back_to_global_on_an_unknown_scheme_name() {
        let config = config_with(Some("Catppuccin Frappe"));
        let colors = effective_colors(&config, profile::find(&config.profiles, "ssh-prod"), true);
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
    }

    #[test]
    fn one_config_pass_resolves_mixed_tabs_to_different_colors() {
        let config = config_with(Some("Dracula"));
        let profile_tab =
            effective_colors(&config, profile::find(&config.profiles, "ssh-prod"), true);
        let plain_tab = effective_colors(&config, None, true);
        assert_ne!(
            profile_tab[NamedColor::Background],
            plain_tab[NamedColor::Background]
        );
        assert_ne!(profile_tab[NamedColor::Red], plain_tab[NamedColor::Red]);
    }

    #[test]
    fn unknown_names_fall_back_to_the_github_pair() {
        // The unknown-name fallback is the GitHub pair. The arm is
        // reached when the *global* name is unknown — an unknown
        // profile pick is ignored in favor of global resolution
        // (pinned above), so it can never reach it. A profile
        // without its own scheme does not shield the tab either.
        for (dark, background) in [(true, 0x0D_1117), (false, 0xFF_FFFF)] {
            let config = DropTermConfig {
                color_scheme_dark: "Stale Name".to_string(),
                color_scheme_light: "Stale Name".to_string(),
                profiles: vec![profile::Profile {
                    name: "ssh-prod".to_string(),
                    command: "ssh web-prod".to_string(),
                    cwd: None,
                    color_scheme: None,
                    env: None,
                    font_size: None,
                    scrollback_lines: None,
                }],
                ..DropTermConfig::default()
            };
            let colors = effective_colors(&config, None, dark);
            assert_eq!(colors[NamedColor::Background], Some(rgb(background)));
            let colors =
                effective_colors(&config, profile::find(&config.profiles, "ssh-prod"), dark);
            assert_eq!(colors[NamedColor::Background], Some(rgb(background)));
        }
    }

    #[test]
    fn registry_defaults_and_fallback_agree_on_the_github_pair() {
        // The one-agreement pin (theme-defaults spec): the registry's
        // first two rows, the config defaults, and the unknown-name
        // fallback all name the same pair — by construction through
        // the shared constants, pinned here so a second definition
        // can never drift in.
        let names = theme_source::scheme_names();
        assert_eq!(names[0], crate::core::theme_source::GITHUB_THEME_DARK);
        assert_eq!(names[1], crate::core::theme_source::GITHUB_THEME_LIGHT);
        assert_eq!(
            DropTermConfig::default().color_scheme_dark,
            crate::core::theme_source::GITHUB_THEME_DARK
        );
        assert_eq!(
            DropTermConfig::default().color_scheme_light,
            crate::core::theme_source::GITHUB_THEME_LIGHT
        );
        // The fallback serves exactly the registry entries those
        // names resolve — same background slot either route.
        let config = DropTermConfig {
            color_scheme_dark: "Stale Name".to_string(),
            color_scheme_light: "Stale Name".to_string(),
            ..DropTermConfig::default()
        };
        let fallback_dark = effective_colors(&config, None, true);
        let registry_dark = theme_source::by_name(crate::core::theme_source::GITHUB_THEME_DARK)
            .expect("registry name");
        assert_eq!(
            fallback_dark[NamedColor::Background],
            registry_dark[NamedColor::Background]
        );
        let fallback_light = effective_colors(&config, None, false);
        let registry_light = theme_source::by_name(crate::core::theme_source::GITHUB_THEME_LIGHT)
            .expect("registry name");
        assert_eq!(
            fallback_light[NamedColor::Background],
            registry_light[NamedColor::Background]
        );
    }

    #[test]
    fn effective_is_dark_pins_dark_and_light() {
        assert!(effective_is_dark(ThemePreference::Dark, false));
        assert!(effective_is_dark(ThemePreference::Dark, true));
        assert!(!effective_is_dark(ThemePreference::Light, false));
        assert!(!effective_is_dark(ThemePreference::Light, true));
    }

    #[test]
    fn effective_is_dark_system_follows_the_injected_mode() {
        assert!(effective_is_dark(ThemePreference::System, true));
        assert!(!effective_is_dark(ThemePreference::System, false));
    }

    #[test]
    fn theme_override_is_none_when_preference_is_unchanged() {
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
}
