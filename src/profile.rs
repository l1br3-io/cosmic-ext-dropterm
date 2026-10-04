// SPDX-License-Identifier: GPL-3.0-only

//! The profile model: a named preset of spawn parameters and the pure
//! resolution seams every profile consumer (the launch paths, the
//! new-tab dropdown, the settings page) reads. Inert by design — no
//! spawn side effects, no UI, no app wiring; behavior changes begin
//! in `profile-launch`.

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
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "transient: consumed by profile-launch (profiles module 2)"
    )
)]
pub fn effective_default<'a>(
    profiles: &'a [Profile],
    default_profile: Option<&str>,
) -> Option<&'a Profile> {
    find(profiles, default_profile?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, command: &str) -> Profile {
        Profile {
            name: name.to_string(),
            command: command.to_string(),
            cwd: None,
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
}
