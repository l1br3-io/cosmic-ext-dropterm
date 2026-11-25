// SPDX-License-Identifier: GPL-3.0-only

//! The command palette's pure model (the command-palette spec): one
//! [`Command`] per entry, composed fresh on every open over the
//! vocabularies `DropTerm` already owns — the keybind registry, the
//! context-menu verbs, the profile list, the open tabs, the settings
//! drawer's pages, the merged scheme registry — and scored by
//! [`score`] with nucleo-matcher. Toolkit-free like the rest of
//! `core`: the bin's `palette.rs` renders the rows and routes each
//! [`Verb`] into a funnel the keybind map or a menu already drives —
//! the palette introduces no new verb path, exactly the context
//! menu's rule ("Item picks land in each frontend's `perform_action`
//! path"). The research record lives at
//! `docs/research/command-palette.md`; the spec at
//! `SPEC-command-palette.md`.

use crate::core::keybinds::{Binding, binding_rows, effective_binding};
use crate::core::profile::Profile;
use crate::core::settings::{SettingsPage, page_title};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// One palette row's section: the category header the row renders
/// under on the empty query. Registry rows carry their registry
/// group's header (the four titles the keybind editor shows); the
/// dynamic families carry their own; the two active-tab close verbs
/// share the registry's Tabs header — they are tab actions by
/// nature, and a two-row header of their own buys nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// A keybind-registry action group; the header is the registry's
    /// group title ("Clipboard", "Tabs", "Zoom", "Other").
    Actions(&'static str),
    Profiles,
    OpenTabs,
    Settings,
    ColorSchemes,
    /// The script commands (script-commands spec): the merged
    /// declared + runtime rows, one seat per slug, appended through
    /// the caller's `extra` seam (plugin-manifest advertising).
    ScriptCommands,
}

impl Section {
    /// The header text the empty-query view renders.
    pub fn title(self) -> &'static str {
        match self {
            Section::Actions(title) => title,
            Section::Profiles => "Profiles",
            Section::OpenTabs => "Open tabs",
            Section::Settings => "Settings",
            Section::ColorSchemes => "Color schemes",
            Section::ScriptCommands => "Script commands",
        }
    }
}

/// Where a pick lands: the core-owned destination vocabulary — a
/// keybind action id, a profile spawn or switch, a tab select, a
/// settings page, a scheme apply — never a bin type. The bin's
/// `route()` maps each variant into the funnel its keybind or menu
/// twin already drives; its match is exhaustive, so a new variant is
/// a compile error away from an unrouted row (the bin
/// `context_menu`'s `route` discipline, applied bin-side where the
/// bin types live).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verb {
    /// The keybind-registry action with this binding id (the
    /// `custom_key_binds` key).
    Action(&'static str),
    /// Close every tab except the active one — the per-tab menu's
    /// Close Other Tabs sweep, acting on the active tab.
    CloseOtherTabs,
    /// Close every tab to the right of the active one.
    CloseTabsRight,
    /// Spawn a new tab under the named profile — the context menu's
    /// New-tab folder row.
    NewProfileTab(String),
    /// Respawn the active tab under the named profile — the Switch
    /// Profile folder row.
    SwitchProfile(String),
    /// Activate the one-based tab index — the `select_tab_target`
    /// funnel's own input, so a dead index (its tab died while the
    /// palette stood open) resolves to the funnel's `None` no-op.
    SelectTab(usize),
    /// Open settings at this page.
    OpenSettings(SettingsPage),
    /// Apply the named merged-registry scheme to the picker slot
    /// matching the resolved theme mode — the pickers' own rule
    /// (theming's per-mode pairing).
    ApplyScheme(String),
    /// Run the named script command (script-commands spec): the
    /// slug under the `script/` namespace, dispatched through the
    /// same `RunCommand` funnel a bound chord rides. The plugins
    /// initiative's append-by-id row — the reserved seam's first
    /// consumer (plugin-manifest advertising).
    ScriptCommand(String),
}

/// One palette row: pure data the bin renders and routes — the
/// palette's own twin of the context-menu Item. `id` is the stable
/// palette key — `command_palette`, `go-to-tab:3`, `scheme:Dracula`
/// — owned, not borrowed: dynamic rows build it at composition
/// time, and the plugins initiative's script commands append by it
/// (the record's scope ruling), never by index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub id: String,
    pub label: String,
    pub section: Section,
    /// The human chord hint, pre-resolved through the keybind
    /// registry's effective display — `None` for unbound rows.
    pub chord: Option<String>,
    pub verb: Verb,
}

/// The V1 catalog: every registry row, the two non-keybound tab
/// verbs, the dynamic families, and the caller's `extra` rows (the
/// plugins initiative's append-by-id slot, unused in V1), in section
/// order. Composition-deterministic: identical inputs render
/// identically, and the dynamic rows track their live sources — the
/// profile list in config order, the tab fleet in tab order, the
/// merged scheme registry in its order. Cheap by design: every
/// source is in-memory state, so the bin composes fresh on every
/// open.
pub fn compose(
    custom_binds: &std::collections::BTreeMap<String, Binding>,
    profiles: &[Profile],
    tab_titles: &[String],
    scheme_names: &[String],
    extra: &[Command],
) -> Vec<Command> {
    let mut commands: Vec<Command> = Vec::with_capacity(
        binding_rows().len()
            + 2
            + profiles.len() * 2
            + tab_titles.len()
            + SettingsPage::ALL.len()
            + scheme_names.len()
            + extra.len(),
    );
    // 1. The keybind registry, in its own display order, each row's
    //    chord the effective one (a custom override wins wholesale).
    for registry_row in binding_rows() {
        commands.push(Command {
            id: registry_row.id.to_string(),
            label: registry_row.label.to_string(),
            section: Section::Actions(registry_row.group),
            chord: effective_binding(registry_row.id, custom_binds),
            verb: Verb::Action(registry_row.id),
        });
    }
    // 2. The two non-keybound tab verbs ride the registry's Tabs
    //    header, appended after the group's rows so the section
    //    stays contiguous. They act on the active tab through the
    //    same close gate the per-tab menu uses.
    let tabs_end = commands
        .iter()
        .rposition(|command| matches!(command.section, Section::Actions("Tabs")))
        .map_or(commands.len(), |last| last + 1);
    commands.splice(
        tabs_end..tabs_end,
        [
            Command {
                id: "close-others".to_string(),
                label: "Close Other Tabs".to_string(),
                section: Section::Actions("Tabs"),
                chord: None,
                verb: Verb::CloseOtherTabs,
            },
            Command {
                id: "close-right".to_string(),
                label: "Close Tabs to the Right".to_string(),
                section: Section::Actions("Tabs"),
                chord: None,
                verb: Verb::CloseTabsRight,
            },
        ],
    );
    // 3. Per profile, in config order: the context-menu folders'
    //    pairing, one spawn row and one switch row each.
    for profile in profiles {
        commands.push(Command {
            id: format!("new-tab:{}", profile.name),
            label: format!("New tab: {}", profile.name),
            section: Section::Profiles,
            chord: None,
            verb: Verb::NewProfileTab(profile.name.clone()),
        });
        commands.push(Command {
            id: format!("switch-profile:{}", profile.name),
            label: format!("Switch to profile: {}", profile.name),
            section: Section::Profiles,
            chord: None,
            verb: Verb::SwitchProfile(profile.name.clone()),
        });
    }
    // 4. One row per open tab, one-based like the keybound slots
    //    and the CLI's select-tab — positions beyond the nine
    //    keybound slots reach a searchable surface here.
    for (offset, title) in tab_titles.iter().enumerate() {
        let one_based = offset + 1;
        commands.push(Command {
            id: format!("go-to-tab:{one_based}"),
            label: format!("Go to tab: {title}"),
            section: Section::OpenTabs,
            chord: None,
            verb: Verb::SelectTab(one_based),
        });
    }
    // 5. One row per settings page, in page order, opening
    //    settings at that page.
    for page in SettingsPage::ALL {
        commands.push(Command {
            id: format!("settings:{}", page_title(page).to_lowercase()),
            label: format!("Settings: {}", page_title(page)),
            section: Section::Settings,
            chord: None,
            verb: Verb::OpenSettings(page),
        });
    }
    // 6. One row per merged-registry scheme, applying live to the
    //    picker slot matching the resolved theme mode.
    for name in scheme_names {
        commands.push(Command {
            id: format!("scheme:{name}"),
            label: format!("Scheme: {name}"),
            section: Section::ColorSchemes,
            chord: None,
            verb: Verb::ApplyScheme(name.clone()),
        });
    }
    // 7. The caller's rows last — the plugins seam, append-by-id.
    commands.extend_from_slice(extra);
    commands
}

/// Scores every row's label against the query: nucleo multi-word
/// fuzzy (`Pattern::new` — each whitespace-separated word matches
/// individually, no `fzf`-style atom syntax), `CaseMatching::Ignore`
/// so `drac` and `DRAC` score alike, `Normalization::Smart` for the
/// accented query. A row scores only when every word lands; the
/// hits come back score-descending with ties in registry order (the
/// stable sort keeps the composer's order), as `(index, score)`
/// pairs into `rows`. The empty query is the composer's job: every
/// row scores 0 in composition order, so the caller renders the
/// catalog instead of a scored list.
pub fn score(query: &str, rows: &[Command]) -> Vec<(usize, u32)> {
    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut haystack = Vec::new();
    let mut hits: Vec<(usize, u32)> = rows
        .iter()
        .enumerate()
        .filter_map(|(index, command)| {
            pattern
                .score(Utf32Str::new(&command.label, &mut haystack), &mut matcher)
                .map(|points| (index, points))
        })
        .collect();
    // Stable: equal scores keep registry (composition) order.
    hits.sort_by_key(|(_, points)| std::cmp::Reverse(*points));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::keybinds::ModifierName;

    fn no_binds() -> std::collections::BTreeMap<String, Binding> {
        std::collections::BTreeMap::default()
    }

    fn profile(name: &str) -> Profile {
        Profile {
            name: name.to_string(),
            command: format!("run {name}"),
            cwd: None,
            color_scheme: None,
            env: None,
            font_size: None,
            scrollback_lines: None,
            input_mode: None,
        }
    }

    /// A fixed catalog over two profiles, three tabs, two schemes —
    /// the inputs the composition pins walk.
    fn catalog() -> Vec<Command> {
        compose(
            &no_binds(),
            &[profile("ssh-web"), profile("htop")],
            &[
                "zsh".to_string(),
                "htop".to_string(),
                "ssh web-prod".to_string(),
            ],
            &["Dracula".to_string(), "Nord".to_string()],
            &[],
        )
    }

    fn index_of(catalog: &[Command], id: &str) -> usize {
        catalog
            .iter()
            .position(|command| command.id == id)
            .unwrap_or_else(|| panic!("no row {id} in the catalog"))
    }

    fn row(id: &str, label: &str) -> Command {
        Command {
            id: id.to_string(),
            label: label.to_string(),
            section: Section::Actions("Other"),
            chord: None,
            verb: Verb::Action("select_all"),
        }
    }

    // --- composition pins ---

    #[test]
    fn compose_is_deterministic_for_fixed_inputs() {
        assert_eq!(catalog(), catalog());
    }

    #[test]
    fn every_registry_row_appears_exactly_once_with_its_effective_chord() {
        let catalog = catalog();
        for registry_row in binding_rows() {
            let occurrences: Vec<&Command> = catalog
                .iter()
                .filter(|command| command.id == registry_row.id)
                .collect();
            assert_eq!(
                occurrences.len(),
                1,
                "registry row {} appears {} times",
                registry_row.id,
                occurrences.len()
            );
            assert_eq!(
                occurrences[0].chord,
                effective_binding(registry_row.id, &no_binds()),
                "row {} carries its effective chord",
                registry_row.id
            );
        }
    }

    #[test]
    fn a_custom_binding_is_the_row_chord() {
        let custom = no_binds();
        let mut custom = custom;
        custom.insert(
            "copy".to_string(),
            Binding {
                modifiers: vec![ModifierName::Alt],
                key: "X".to_string(),
            },
        );
        let catalog = compose(&custom, &[], &[], &[], &[]);
        let copy = &catalog[index_of(&catalog, "copy")];
        assert_eq!(copy.chord, Some("Alt + X".to_string()));
    }

    #[test]
    fn the_unbound_row_ships_no_chord() {
        let catalog = catalog();
        let copy_primary = &catalog[index_of(&catalog, "copy_primary")];
        assert_eq!(copy_primary.chord, None);
    }

    #[test]
    fn dynamic_families_track_their_sources() {
        let catalog = catalog();
        // Profiles: the context-menu folders' pairing, in config
        // order — spawn row then switch row per profile.
        let pairs = [
            (
                index_of(&catalog, "new-tab:ssh-web"),
                index_of(&catalog, "switch-profile:ssh-web"),
            ),
            (
                index_of(&catalog, "new-tab:htop"),
                index_of(&catalog, "switch-profile:htop"),
            ),
        ];
        assert!(
            pairs[0].0 < pairs[0].1 && pairs[0].1 < pairs[1].0 && pairs[1].0 < pairs[1].1,
            "profile rows keep config order with spawn before switch"
        );
        assert_eq!(
            catalog[pairs[1].0].label, "New tab: htop",
            "the label names the profile"
        );
        assert_eq!(
            catalog[pairs[1].1].verb,
            Verb::SwitchProfile("htop".to_string()),
            "the switch row names its destination"
        );
        // Open tabs: one row per tab, one-based like the keybound
        // slots, in tab order.
        for (offset, title) in ["zsh", "htop", "ssh web-prod"].iter().enumerate() {
            let one_based = offset + 1;
            let row = &catalog[index_of(&catalog, &format!("go-to-tab:{one_based}"))];
            assert_eq!(row.label, format!("Go to tab: {title}"));
            assert_eq!(row.verb, Verb::SelectTab(one_based));
        }
        // Schemes: one row per merged-registry name, in registry
        // order.
        assert_eq!(
            catalog[index_of(&catalog, "scheme:Dracula")].verb,
            Verb::ApplyScheme("Dracula".to_string())
        );
        assert_eq!(
            catalog[index_of(&catalog, "scheme:Nord")].verb,
            Verb::ApplyScheme("Nord".to_string())
        );
    }

    #[test]
    fn the_settings_rows_cover_every_page_in_page_order() {
        let catalog = catalog();
        let settings: Vec<&Command> = catalog
            .iter()
            .filter(|command| command.id.starts_with("settings:"))
            .collect();
        let expected: Vec<&str> = SettingsPage::ALL
            .iter()
            .map(|page| page_title(*page))
            .collect();
        assert_eq!(settings.len(), expected.len(), "one row per page");
        for (row, title) in settings.iter().zip(&expected) {
            assert_eq!(row.label, format!("Settings: {title}"));
        }
        // The verb carries the core vocabulary the window's seam
        // consumes. (Index follows the ten-page ALL — the Auto
        // complete page joined at rich-input RI-1.)
        assert_eq!(
            settings[9].verb,
            Verb::OpenSettings(SettingsPage::Keybindings)
        );
    }

    #[test]
    fn the_close_verbs_ride_the_tabs_section_after_the_registry_rows() {
        let catalog = catalog();
        let others = index_of(&catalog, "close-others");
        let right = index_of(&catalog, "close-right");
        // Both sit in the registry's Tabs section, after the group's
        // registry rows and before the next section begins.
        assert_eq!(catalog[others].section, Section::Actions("Tabs"));
        assert_eq!(catalog[others].label, "Close Other Tabs");
        assert_eq!(catalog[right].label, "Close Tabs to the Right");
        let last_registry_tabs = catalog
            .iter()
            .rposition(|command| command.id == "activate_last_tab")
            .expect("the registry carries the tabs group");
        let first_zoom = index_of(&catalog, "zoom_in");
        assert!(
            last_registry_tabs < others && others < right && right < first_zoom,
            "the close verbs slot between the Tabs rows and the next group"
        );
    }

    #[test]
    fn sections_are_contiguous() {
        let catalog = catalog();
        let mut seen: Vec<Section> = Vec::new();
        for command in &catalog {
            if seen.last() != Some(&command.section) {
                assert!(
                    !seen.contains(&command.section),
                    "section {:?} is not contiguous",
                    command.section
                );
                seen.push(command.section);
            }
        }
    }

    #[test]
    fn ids_are_unique_across_the_catalog() {
        let catalog = catalog();
        let mut ids: Vec<&str> = catalog.iter().map(|command| command.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate palette ids");
    }

    #[test]
    fn extra_rows_append_verbatim() {
        let plugin_row = Command {
            id: "plugin:hello".to_string(),
            label: "Say hello".to_string(),
            section: Section::ColorSchemes,
            chord: None,
            verb: Verb::ApplyScheme("Dracula".to_string()),
        };
        let catalog = compose(
            &no_binds(),
            &[],
            &[],
            &[],
            std::slice::from_ref(&plugin_row),
        );
        assert_eq!(catalog.last(), Some(&plugin_row));
    }

    // --- scoring pins ---

    #[test]
    fn the_multi_word_query_ranks_clear_scrollback_first() {
        let catalog = catalog();
        let hits = score("scroll clear", &catalog);
        assert!(!hits.is_empty(), "the query matches");
        assert_eq!(
            hits[0].0,
            index_of(&catalog, "clear_scrollback"),
            "scroll clear ranks Clear scrollback first"
        );
    }

    #[test]
    fn smart_casing_scores_drac_and_drac_alike() {
        let catalog = catalog();
        let lower = score("drac", &catalog);
        let upper = score("DRAC", &catalog);
        let dracula = index_of(&catalog, "scheme:Dracula");
        assert_eq!(lower[0].0, dracula, "drac finds Dracula");
        assert_eq!(upper[0].0, dracula, "DRAC finds Dracula");
        assert_eq!(lower[0].1, upper[0].1, "the two casings score identically");
    }

    #[test]
    fn non_matches_drop() {
        assert!(score("zzz", &catalog()).is_empty());
    }

    #[test]
    fn equal_scores_keep_registry_order() {
        let rows = [row("first", "Alpha"), row("second", "Alpha")];
        let hits = score("alp", &rows);
        assert_eq!(
            hits,
            vec![(0, hits[0].1), (1, hits[0].1)],
            "identical labels score alike and keep their order"
        );
    }

    #[test]
    fn the_empty_query_is_the_composer_s_no_op() {
        let catalog = catalog();
        let hits = score("", &catalog);
        assert_eq!(
            hits,
            (0..catalog.len())
                .map(|index| (index, 0))
                .collect::<Vec<_>>(),
            "every row scores zero in composition order"
        );
    }
}
