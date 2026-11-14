// SPDX-License-Identifier: GPL-3.0-only

//! The font picker's enumeration seam (font-picker spec): the pure
//! collector over `(family, monospaced)` face pairs — filter, dedupe,
//! case-insensitive sort — and the one fontdb scan that feeds it. All
//! fontdb knowledge lives here; the settings drawer consumes only the
//! resulting family list.

/// Collect the distinct monospace family names from a font
/// database's `(family, monospaced)` face pairs: proportional faces
/// drop, a family is listed once however many faces (weights, styles)
/// carry it, and the order is case-insensitive with the raw name
/// breaking ties — so a system's list is stable and deterministic
/// whatever the scan order.
pub fn collect_monospace_families(faces: &[(String, bool)]) -> Vec<String> {
    let mut families: Vec<String> = faces
        .iter()
        .filter(|(_, monospaced)| *monospaced)
        .map(|(family, _)| family.clone())
        .collect();
    families.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    families.dedup();
    families
}

/// The system's installed monospace families, scanned once at startup:
/// fontdb walks the system font dirs (fontconfig included) and every
/// face's `monospaced` flag feeds [`collect_monospace_families`]. An
/// empty result stays empty — the picker's synthetic leading row
/// covers it, so a barren scan still never offers an empty picker.
pub fn load_monospace_families() -> Vec<String> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    // A face's first family is its English-US name — the name a
    // `Family::Name` lookup matches against.
    let faces: Vec<(String, bool)> = database
        .faces()
        .filter_map(|face| {
            face.families
                .first()
                .map(|(family, _)| (family.clone(), face.monospaced))
        })
        .collect();
    collect_monospace_families(&faces)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- the enumeration seam's pure collector (font-picker T1) ---

    #[test]
    fn proportional_families_drop_and_monospaced_families_keep() {
        // The `monospaced` flag is the whole filter: a family whose
        // every face is proportional never lists, a family with any
        // monospaced face does.
        assert_eq!(
            collect_monospace_families(&[
                ("Noto Sans Mono".to_string(), true),
                ("Noto Sans".to_string(), false),
                ("DejaVu Sans Mono".to_string(), true),
            ]),
            vec!["DejaVu Sans Mono".to_string(), "Noto Sans Mono".to_string()]
        );
        assert_eq!(
            collect_monospace_families(&[("Cantarell".to_string(), false)]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_family_dedupes_across_its_faces() {
        // Weights and styles are separate faces carrying one family
        // name; the picker lists the family once.
        assert_eq!(
            collect_monospace_families(&[
                ("JetBrainsMono Nerd Font Mono".to_string(), true),
                ("JetBrainsMono Nerd Font Mono".to_string(), true),
                ("JetBrainsMono Nerd Font Mono".to_string(), true),
            ]),
            vec!["JetBrainsMono Nerd Font Mono".to_string()]
        );
    }

    #[test]
    fn ordering_is_case_insensitive_with_the_raw_name_breaking_ties() {
        // Case-insensitive overall order (a `z` family sorts under Z,
        // not after it), and two families equal apart from case keep
        // both — `Family::Name` matches exactly, so they are different
        // families — ordered deterministically by their raw names
        // (byte order: the uppercase variant leads).
        assert_eq!(
            collect_monospace_families(&[
                ("adwaita mono".to_string(), true),
                ("Ubuntu Mono".to_string(), true),
                ("Adwaita Mono".to_string(), true),
                ("Zed Mono".to_string(), true),
            ]),
            vec![
                "Adwaita Mono".to_string(),
                "adwaita mono".to_string(),
                "Ubuntu Mono".to_string(),
                "Zed Mono".to_string(),
            ]
        );
    }
}
