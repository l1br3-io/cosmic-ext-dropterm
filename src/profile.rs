// SPDX-License-Identifier: GPL-3.0-only

//! The profile model: a named preset of spawn parameters and the pure
//! resolution seams every profile consumer (the launch paths, the
//! new-tab dropdown, the settings page) reads. Inert by design — no
//! spawn side effects, no UI, no app wiring; behavior changes begin
//! in `profile-launch`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::tabs::command_line_parts;

/// A named preset of spawn parameters: what a tab runs and where.
///
/// The name is the id (exact, case-sensitive; duplicates in a
/// hand-edited config resolve first-match — see [`find`]). Uniqueness
/// is enforced by the settings UI at write time, not here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Display name; also the lookup id.
    pub name: String,
    /// Command line to run, e.g. `ssh web-prod`. Must survive
    /// [`crate::tabs::command_line_parts`] (non-blank, has a program).
    pub command: String,
    /// Working directory for the spawned program. `None` = default
    /// cwd. Not existence-checked: a profile may be written before
    /// its directory exists; spawn-time fallback is `profile-launch`'s.
    pub cwd: Option<PathBuf>,
    /// Color scheme the profile's tabs run: a built-in registry name
    /// ([`crate::terminal::theme_source::scheme_names`]'s vocabulary,
    /// the same one the global pickers offer), applied per tab at
    /// spawn and in the live config-apply pass. `None` (the default)
    /// follows the global scheme pick. One name, mode-independent —
    /// a profile is a deliberate environment, not a theme follower.
    /// An unknown name degrades to the global look at resolve time;
    /// the chain never errors and the store is never rewritten to
    /// strip it.
    #[serde(default)]
    pub color_scheme: Option<String>,
    /// Extra environment variables the profile's tabs carry, merged
    /// over the inherited environment at pty spawn (alacritty's
    /// `PtyOptions::env`). Spawn-time only by nature — a running
    /// process's environment never changes, so an edit affects the
    /// next spawn, never a live tab. Applied only when the spawn
    /// actually runs the preset's command line; the degraded
    /// blank-line spawn is a plain-shell tab and carries none of the
    /// profile's identity (the Profile arm of the spawn path).
    /// `None` (the default) inherits untouched.
    /// `BTreeMap`, so the RON store round-trips in stable key order.
    /// Values are verbatim — no expansion, ever; keys are validated
    /// only at the editor's write time, load tolerates anything
    /// valid UTF-8.
    #[serde(default)]
    pub env: Option<BTreeMap<String, String>>,
    /// Terminal font size in points the profile's tabs render at —
    /// applied per tab at spawn and in the live config-apply pass.
    /// `None` (the default) follows the global font size.
    #[serde(default)]
    pub font_size: Option<u16>,
    /// Scrollback capacity the profile's terminals start with.
    /// Terminal-creation only (the global key's own timing): a
    /// running terminal's capacity never changes, so an edit affects
    /// the next spawn or respawn, never a live tab. `None` (the
    /// default) follows the global `scrollback_lines`.
    #[serde(default)]
    pub scrollback_lines: Option<u32>,
}

impl Profile {
    /// Whether the preset is well-formed enough to offer: a non-blank
    /// name and a command line that splits into a program plus
    /// arguments — [`command_line_parts`] is the only command-line
    /// authority (whitespace-split, quoting unsupported). A missing
    /// cwd is fine; cwd existence is a spawn-time concern, not a
    /// define-time one.
    // No dead-code expect here, unlike the free-fn seams below: once
    // `DropTermConfig` carried a `Vec<Profile>` (this module's T2),
    // rustc's dead-code analysis stopped firing on this inherent
    // method in every target — a kept `#[expect]` went unfulfilled
    // and broke `-D warnings` — so the attribute was deleted per the
    // unfulfilled-expect rule. Still unconsumed until `profile-launch`.
    pub fn validate(&self) -> bool {
        !self.name.trim().is_empty() && command_line_parts(&self.command).is_some()
    }
}

/// Resolves a profile by name: exact, case-sensitive match (`"ssh"`
/// is not `"SSH"`); duplicate names in a hand-edited config resolve
/// first-match — uniqueness is a write-time concern of the settings
/// UI, tolerated here. `None` when no profile carries the name.
pub fn find<'a>(profiles: &'a [Profile], name: &str) -> Option<&'a Profile> {
    profiles.iter().find(|profile| profile.name == name)
}

/// Resolves the configured default profile, if one is set and still
/// exists. A dangling `default_profile` (profile deleted, config
/// hand-edited) is `None` — plain shell — never a config rewrite.
pub fn effective_default<'a>(
    profiles: &'a [Profile],
    default_profile: Option<&str>,
) -> Option<&'a Profile> {
    find(profiles, default_profile?)
}

// --- the appearance resolution seams: `profile.override ?? global`,
// per field — the one rule every appearance consumer (spawn, the live
// config-apply pass) resolves through, so it can never fork ---

/// The scheme name a tab under `profile` renders with: the profile's
/// explicit choice in **both** system modes (a profile is a
/// deliberate environment, not a theme follower), else the global
/// pair resolved for `is_dark`. An unknown profile scheme name is
/// ignored — global resolution, exactly as
/// [`theme_source::by_name`] falls back for unknown global names;
/// never an error surface, never a store rewrite.
pub fn effective_scheme_name<'a>(
    profile: Option<&'a Profile>,
    global_dark: &'a str,
    global_light: &'a str,
    is_dark: bool,
) -> &'a str {
    let global = if is_dark { global_dark } else { global_light };
    profile
        .and_then(|profile| profile.color_scheme.as_deref())
        .filter(|name| crate::terminal::theme_source::by_name(name).is_some())
        .unwrap_or(global)
}

/// The font size a tab under `profile` renders at: the profile's
/// override, else the global pick. Applied at spawn and in the live
/// config-apply pass, like [`effective_scheme_name`].
pub fn effective_font_size(profile: Option<&Profile>, global: u16) -> u16 {
    profile
        .and_then(|profile| profile.font_size)
        .unwrap_or(global)
}

/// The scrollback capacity a terminal under `profile` starts with:
/// the profile's override, else the global pick. Terminal-creation
/// only — the same timing the global key itself carries.
pub fn effective_scrollback(profile: Option<&Profile>, global: u32) -> u32 {
    profile
        .and_then(|profile| profile.scrollback_lines)
        .unwrap_or(global)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, command: &str) -> Profile {
        Profile {
            name: name.to_string(),
            command: command.to_string(),
            cwd: None,
            color_scheme: None,
            env: None,
            font_size: None,
            scrollback_lines: None,
        }
    }

    #[test]
    fn validate_rejects_a_blank_name() {
        assert!(!profile("", "htop").validate());
    }

    #[test]
    fn validate_rejects_a_whitespace_only_name() {
        assert!(!profile("   ", "htop").validate());
    }

    #[test]
    fn validate_rejects_a_command_line_without_a_program() {
        assert!(!profile("top", "").validate());
        assert!(!profile("top", "   ").validate());
    }

    #[test]
    fn validate_accepts_a_full_profile() {
        let full = Profile {
            cwd: Some(PathBuf::from("/srv/web")),
            ..profile("web-prod", "ssh web-prod")
        };
        assert!(full.validate());
    }

    #[test]
    fn validate_accepts_a_missing_cwd() {
        assert!(profile("web-prod", "ssh web-prod").validate());
    }

    #[test]
    fn validate_never_checks_cwd_existence() {
        // The directory need not exist at define time: a profile may
        // be written before its cwd is created. Spawn-time fallback
        // is `profile-launch`'s.
        let unborn = Profile {
            cwd: Some(PathBuf::from("/dropterm/profile-model/not/yet/created")),
            ..profile("web-prod", "ssh web-prod")
        };
        assert!(unborn.validate());
    }

    #[test]
    fn find_resolves_an_exact_name() {
        let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
        assert_eq!(find(&profiles, "ssh"), Some(&profiles[0]));
    }

    #[test]
    fn find_rejects_a_case_mismatched_name() {
        // Name matching is exact and case-sensitive: `"ssh"` ≠ `"SSH"`.
        let profiles = [profile("ssh", "ssh web-prod")];
        assert_eq!(find(&profiles, "SSH"), None);
    }

    #[test]
    fn find_returns_none_for_an_unknown_name() {
        let profiles = [profile("ssh", "ssh web-prod")];
        assert_eq!(find(&profiles, "ghost"), None);
    }

    #[test]
    fn find_resolves_duplicate_names_to_the_first_match() {
        // Hand-edited configs may repeat a name; read time tolerates
        // it instead of rewriting the file — the first entry wins.
        let profiles = [profile("dupe", "ssh first"), profile("dupe", "ssh second")];
        let found = find(&profiles, "dupe");
        assert_eq!(found.map(|p| p.command.as_str()), Some("ssh first"));
    }

    #[test]
    fn effective_default_is_none_without_a_configured_default() {
        let profiles = [profile("ssh", "ssh web-prod")];
        assert_eq!(effective_default(&profiles, None), None);
    }

    #[test]
    fn effective_default_resolves_a_set_and_existing_default() {
        let profiles = [profile("ssh", "ssh web-prod"), profile("top", "htop")];
        assert_eq!(
            effective_default(&profiles, Some("top")),
            Some(&profiles[1])
        );
    }

    #[test]
    fn effective_default_is_none_on_a_dangling_default() {
        // The default names a profile the list no longer carries
        // (deleted, or hand-edited away): plain shell — and the load
        // path never rewrites the config to "fix" it.
        let profiles = [profile("ssh", "ssh web-prod")];
        assert_eq!(effective_default(&profiles, Some("ghost")), None);
    }

    // --- the color-scheme field's serde shape ---

    #[test]
    fn a_pre_initiative_profile_loads_with_the_scheme_unset() {
        // The pre-initiative on-disk shape — no `color_scheme` key at
        // all — loads as "follow the global pick", never an error:
        // reading tolerates the older format the same way it tolerates
        // every other additive field.
        let loaded: Profile =
            ron::from_str(r#"(name: "ssh", command: "ssh web")"#).expect("old shape loads");
        assert_eq!(loaded.color_scheme, None);
    }

    #[test]
    fn a_profile_with_a_scheme_round_trips_through_ron() {
        // The field's serde shape is the store's: a set name survives
        // a serialize→deserialize cycle unchanged (config.rs's store
        // round-trip exercises the same path through the real store).
        let stored = Profile {
            color_scheme: Some("Dracula".to_string()),
            ..profile("ssh-prod", "ssh web-prod")
        };
        let text = ron::to_string(&stored).expect("serialize");
        let back: Profile = ron::from_str(&text).expect("deserialize");
        assert_eq!(back, stored);
    }

    // --- the env field's serde shape ---

    #[test]
    fn a_pre_initiative_profile_loads_with_env_unset() {
        // The pre-initiative on-disk shape — no `env` key at all —
        // loads as "carries no extra environment", never an error:
        // the same additive-field leniency as every other Profile
        // field.
        let loaded: Profile =
            ron::from_str(r#"(name: "ssh", command: "ssh web")"#).expect("old shape loads");
        assert_eq!(loaded.env, None);
    }

    #[test]
    fn a_profile_with_an_env_map_round_trips_through_ron() {
        // The field's serde shape is the store's, and BTreeMap keeps
        // it deterministic: the same map serializes to the same bytes
        // every time (the `custom_key_binds` precedent), so the store
        // never churns on a write→load→write cycle.
        let stored = Profile {
            env: Some(BTreeMap::from([
                ("DROPTERM_PROFILE".to_string(), "ssh-prod".to_string()),
                ("TERM_PROGRAM".to_string(), "dropterm-ssh".to_string()),
            ])),
            ..profile("ssh-prod", "ssh web-prod")
        };
        let text = ron::to_string(&stored).expect("serialize");
        let back: Profile = ron::from_str(&text).expect("deserialize");
        assert_eq!(back, stored);
    }

    // --- the font/scrollback fields' serde shape (profile-appearance T1) ---

    #[test]
    fn a_pre_initiative_profile_loads_with_font_and_scrollback_unset() {
        // The pre-initiative on-disk shape — neither key present —
        // loads as "follow the global picks", never an error: the
        // same additive-field leniency as every other Profile field.
        let loaded: Profile =
            ron::from_str(r#"(name: "ssh", command: "ssh web")"#).expect("old shape loads");
        assert_eq!(loaded.font_size, None);
        assert_eq!(loaded.scrollback_lines, None);
    }

    #[test]
    fn a_profile_with_font_and_scrollback_overrides_round_trips_through_ron() {
        let stored = Profile {
            font_size: Some(11),
            scrollback_lines: Some(50_000),
            ..profile("ssh-prod", "ssh web-prod")
        };
        let text = ron::to_string(&stored).expect("serialize");
        let back: Profile = ron::from_str(&text).expect("deserialize");
        assert_eq!(back, stored);
    }

    // --- the appearance resolution seams ---

    #[test]
    fn no_profile_resolves_every_field_global() {
        // The None arm of every seam: a plain tab, or a recorded id
        // the list no longer carries — the caller resolves those to
        // `None` alike, so a dangling profile id is all-global.
        assert_eq!(
            effective_scheme_name(None, "Global Dark", "Global Light", true),
            "Global Dark"
        );
        assert_eq!(
            effective_scheme_name(None, "Global Dark", "Global Light", false),
            "Global Light"
        );
        assert_eq!(effective_font_size(None, 14), 14);
        assert_eq!(effective_scrollback(None, 10_000), 10_000);
    }

    #[test]
    fn none_overrides_inherit_the_global_values() {
        // The rule's `None` half, per field: an unset override on the
        // resolved profile is exactly the global value.
        let inheritor = profile("ssh", "ssh web-prod");
        assert_eq!(
            effective_scheme_name(Some(&inheritor), "Global Dark", "Global Light", true),
            "Global Dark"
        );
        assert_eq!(
            effective_scheme_name(Some(&inheritor), "Global Dark", "Global Light", false),
            "Global Light"
        );
        assert_eq!(effective_font_size(Some(&inheritor), 14), 14);
        assert_eq!(effective_scrollback(Some(&inheritor), 10_000), 10_000);
    }

    #[test]
    fn a_profile_scheme_wins_in_both_system_modes() {
        // The scheme rule's other half: one explicit name, applied
        // whatever the system mode — a profile is a deliberate
        // environment, not a theme follower.
        let schemed = Profile {
            color_scheme: Some("Dracula".to_string()),
            ..profile("ssh", "ssh web-prod")
        };
        assert_eq!(
            effective_scheme_name(Some(&schemed), "Global Dark", "Global Light", true),
            "Dracula"
        );
        assert_eq!(
            effective_scheme_name(Some(&schemed), "Global Dark", "Global Light", false),
            "Dracula"
        );
    }

    #[test]
    fn an_unknown_scheme_name_falls_through_to_global_resolution() {
        // Read-time leniency mirroring the global pick's own: an
        // unknown name is ignored — the global pair resolves for the
        // mode — never an error surface, never a store rewrite.
        let mistyped = Profile {
            color_scheme: Some("Dracul".to_string()),
            ..profile("ssh", "ssh web-prod")
        };
        assert_eq!(
            effective_scheme_name(Some(&mistyped), "Global Dark", "Global Light", true),
            "Global Dark"
        );
        assert_eq!(
            effective_scheme_name(Some(&mistyped), "Global Dark", "Global Light", false),
            "Global Light"
        );
    }

    #[test]
    fn each_field_resolves_independently() {
        // The rule is per field, not per profile: an override on one
        // field never drags the others off their global values.
        let only_font = Profile {
            font_size: Some(9),
            ..profile("ssh", "ssh web-prod")
        };
        assert_eq!(
            effective_scheme_name(Some(&only_font), "Global Dark", "Global Light", true),
            "Global Dark"
        );
        assert_eq!(effective_font_size(Some(&only_font), 14), 9);
        assert_eq!(effective_scrollback(Some(&only_font), 10_000), 10_000);

        let only_scrollback = Profile {
            scrollback_lines: Some(2_000),
            ..profile("ssh", "ssh web-prod")
        };
        assert_eq!(effective_font_size(Some(&only_scrollback), 14), 14);
        assert_eq!(effective_scrollback(Some(&only_scrollback), 10_000), 2_000);
    }
}
