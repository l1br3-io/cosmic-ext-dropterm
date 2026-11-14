// SPDX-License-Identifier: GPL-3.0-only

//! The settings drawer's libcosmic front: the model lives in
//! [`dropterm_core::settings`] (moved there by the gpui-chrome spec,
//! framework-free — the write contract, the control→key map, the
//! planned writes, the profile manager's pure half, the keybind
//! write); this module re-exports it whole so every
//! `crate::settings::…` path keeps resolving, and keeps the
//! frontend's own halves: the view builders (`view`, with the font
//! and scheme picker widgets), and the draft-lock gate over this
//! frontend's `SettingsEvent` vocabulary.

pub use dropterm_core::settings::*;

#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
use crate::profile::Profile;

mod font_picker;
mod scheme_picker;

pub mod view;

use self::view::SettingsEvent;

/// Whether `event` may act on the Profiles page while a profile
/// draft is open — the draft lock: no list mutation and no draft
/// switch is reachable while a draft is open, so the draft's origin
/// index cannot shift under it (the stale-index class dies at the
/// root instead of by index bookkeeping). The locked view renders
/// the list's controls disabled; the wiring consults this rule so a
/// stale press — a race between a render and a click — is a no-op
/// even if a control flashed enabled. Pure over the event/state
/// pair, headless-pinned. (Stayed with the front: the gate speaks
/// this frontend's `SettingsEvent` — the one view-typed input the
/// model's move left behind; each frontend's drawer gates its own
/// event vocabulary by the same rule.)
pub fn profile_event_reachable(event: &SettingsEvent, draft_open: bool) -> bool {
    if !draft_open {
        return true;
    }
    !matches!(
        event,
        SettingsEvent::SelectProfile(_)
            | SettingsEvent::OpenProfileDraft
            | SettingsEvent::CloneProfile(_)
            | SettingsEvent::ArmProfileDelete(_)
            | SettingsEvent::ConfirmProfileDelete(_)
            | SettingsEvent::KeepProfileDelete
            | SettingsEvent::ReorderProfile(..)
    )
}

/// The view tests' shared constructors (rode the model's tests module
/// in the bin; the move left them with the tests that use them).
#[cfg(test)]
pub(crate) fn profile(name: &str, command: &str) -> Profile {
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

#[cfg(test)]
pub(crate) fn env_row(key: &str, value: &str) -> ProfileEnvRow {
    ProfileEnvRow {
        key: key.to_string(),
        value: value.to_string(),
    }
}

#[cfg(test)]
pub(crate) fn sourced_profile() -> Profile {
    Profile {
        name: "web-prod".to_string(),
        command: "ssh web-prod".to_string(),
        cwd: Some(PathBuf::from("/srv/web")),
        color_scheme: None,
        env: None,
        font_size: None,
        scrollback_lines: None,
    }
}

#[cfg(test)]
mod front_tests {
    use super::view::SettingsEvent;
    use super::{
        ConfigKey, PickerId, ProfileField, ReorderDirection, SchemePickerId, TextField,
        profile_event_reachable,
    };

    #[test]
    fn list_mutations_and_draft_switches_are_unreachable_while_a_draft_is_open() {
        // The draft lock (spec criterion 1): while a draft is open, no
        // event that mutates the list or switches the draft is
        // reachable — the origin index is unshiftable by construction.
        // One representative per SettingsEvent variant; exactly the list
        // and draft gestures lock (selection, add, clone, the whole
        // delete cluster, reorder), everything else (the form's own
        // gestures, Save/Cancel, the drawer's controls) stays reachable,
        // and nothing is locked without a draft.
        let events = vec![
            SettingsEvent::Draft(TextField::Shell, String::new()),
            SettingsEvent::Submitted(TextField::Shell, String::new()),
            SettingsEvent::SliderMoved(ConfigKey::Opacity, 50.0),
            SettingsEvent::SliderReleased(ConfigKey::Opacity),
            SettingsEvent::Picked(ConfigKey::Edge, 0),
            SettingsEvent::TogglePicker(PickerId::Scheme(SchemePickerId::Global(
                ConfigKey::ColorSchemeDark,
            ))),
            SettingsEvent::TogglePicker(PickerId::Scheme(SchemePickerId::ProfileForm)),
            SettingsEvent::TogglePicker(PickerId::Font),
            SettingsEvent::ClosePicker,
            SettingsEvent::Toggled(ConfigKey::RespectPanel, true),
            SettingsEvent::KeybindSearchChanged(String::new()),
            SettingsEvent::CaptureStart(crate::terminal::Action::Copy),
            SettingsEvent::CaptureCancel,
            SettingsEvent::RevertKeybind(crate::terminal::Action::Copy),
            SettingsEvent::SelectProfile(0),
            SettingsEvent::PickDefaultProfile(0),
            SettingsEvent::OpenProfileDraft,
            SettingsEvent::CloneProfile(0),
            SettingsEvent::DraftProfileField(ProfileField::Name, String::new()),
            SettingsEvent::SaveProfileDraft,
            SettingsEvent::CancelProfileDraft,
            SettingsEvent::PickProfileScheme(0),
            SettingsEvent::AddProfileEnvRow,
            SettingsEvent::RemoveProfileEnvRow(0),
            SettingsEvent::ArmProfileDelete(0),
            SettingsEvent::ConfirmProfileDelete(0),
            SettingsEvent::KeepProfileDelete,
            SettingsEvent::ReorderProfile(0, ReorderDirection::Up),
            SettingsEvent::DrawerResize(0.0),
            SettingsEvent::DrawerResizeReleased,
            SettingsEvent::DrawerReset,
        ];
        for event in &events {
            let expected = !matches!(
                event,
                SettingsEvent::SelectProfile(_)
                    | SettingsEvent::OpenProfileDraft
                    | SettingsEvent::CloneProfile(_)
                    | SettingsEvent::ArmProfileDelete(_)
                    | SettingsEvent::ConfirmProfileDelete(_)
                    | SettingsEvent::KeepProfileDelete
                    | SettingsEvent::ReorderProfile(..)
            );
            assert_eq!(
                profile_event_reachable(event, true),
                expected,
                "with a draft open: {event:?}"
            );
            assert!(
                profile_event_reachable(event, false),
                "without a draft nothing locks: {event:?}"
            );
        }
    }

    /// The libcosmic view's icon chain pin (rode the model's tests in the
    /// bin; the move left it here — it scans this crate's own sources).
    #[test]
    fn every_static_icon_name_resolves_in_the_theme_chain() {
        // The blank edit-profile button: `document-edit-symbolic`
        // exists only in Adwaita — a grandparent theme (Cosmic → Pop
        // → Pop-Extra → pop-os-branding → Adwaita) the icon lookup
        // never reaches — and a name that resolves nowhere renders an
        // empty button. Construction can't catch it (`from_name` never
        // validates), only the theme lookup knows. So this scans src/
        // for every static icon-name literal passed to `from_name` or
        // `row_icon_button` and requires each to exist somewhere in
        // the chain the lookup actually walks. Machines without
        // /usr/share/icons/Cosmic (headless CI) skip; the machines
        // that render the UI ship it.
        fn walk(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, files);
                } else {
                    files.push(path);
                }
            }
        }

        let icons_root = std::path::Path::new("/usr/share/icons");
        if !icons_root.join("Cosmic").is_dir() {
            eprintln!("skipping: /usr/share/icons/Cosmic is not installed");
            return;
        }

        // The lookup chain, mirroring what `from_name` actually walks
        // (freedesktop-icons searches the active theme, then the
        // DIRECT entries of its Inherits= line — parents' own parents
        // are never recursed into — then hardcoded hicolor/gnome/Yaru
        // fallbacks). An unparseable index.theme just contributes no
        // parents; a theme dir that doesn't exist walks to nothing.
        let mut chain = vec!["Cosmic".to_string()];
        if let Ok(index) = std::fs::read_to_string(icons_root.join("Cosmic").join("index.theme")) {
            for parent in index
                .lines()
                .filter_map(|line| line.strip_prefix("Inherits="))
                .flat_map(|value| value.split(','))
                .map(str::trim)
                .filter(|parent| !parent.is_empty())
            {
                chain.push(parent.to_string());
            }
        }
        chain.extend(["hicolor", "gnome", "Yaru"].map(str::to_string));

        let mut theme_files = Vec::new();
        for theme in &chain {
            walk(&icons_root.join(theme), &mut theme_files);
        }
        let available: std::collections::BTreeSet<String> = theme_files
            .iter()
            .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
            .map(str::to_owned)
            .collect();

        // The markers end in an opening quote so only literal call
        // sites match — prose mentioning the API (like this comment)
        // reads past to no closing quote and contributes nothing.
        let mut src_files = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut src_files,
        );
        let mut used = Vec::new();
        for file in &src_files {
            let is_rust = file.extension().is_some_and(|ext| ext == "rs");
            let Ok(text) = std::fs::read_to_string(file) else {
                continue;
            };
            if !is_rust {
                continue;
            }
            for marker in ["from_name(\"", "row_icon_button(\""] {
                let mut rest = text.as_str();
                while let Some(at) = rest.find(marker) {
                    let after = &rest[at + marker.len()..];
                    let Some(end) = after.find('"') else { break };
                    used.push(after[..end].to_string());
                    rest = &after[end..];
                }
            }
        }
        used.sort();
        used.dedup();
        assert!(!used.is_empty(), "the source scan found no icon names");

        // Lookup matches a filename, extension and all — Cosmic ships
        // svg, the inherited themes mostly png, xpm is the spec's
        // floor. Any of the three resolving counts.
        let resolves = |name: &str| {
            ["svg", "png", "xpm"]
                .iter()
                .any(|ext| available.contains(&format!("{name}.{ext}")))
        };
        let missing: Vec<&String> = used.iter().filter(|name| !resolves(name)).collect();
        assert!(
            missing.is_empty(),
            "icon names used in src/ resolve nowhere in the {} chain: {missing:?}",
            chain.join(" → "),
        );
    }
}
