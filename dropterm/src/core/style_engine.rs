//! The style engine (style-engine spec): the CSS-convention styling
//! model — a closed declaration vocabulary, compound selectors, and
//! the specificity cascade over layered rules — resolving what every
//! surface paints. Pure data and validation: no gpui, no mlua — the
//! slot-contract discipline. The rules' two sources feed from
//! without: manifest sheets flow from [`crate::core::plugins_manifest`],
//! runtime sheets from the `dropterm.styles` verb, and the builtin
//! layer is the host's own rules. Colors stay [`SlotColor`] — tokens
//! resolve at paint, through the standing palettes — so the engine
//! never touches theme state and a theme flip re-resolves by
//! re-running [`StyleSet::resolve`].

use crate::core::slots::SlotColor;

/// The v1 declaration vocabulary, closed: one kebab-case property
/// with one value. Growth is additive only — each its own ruling.
pub const STYLE_PROPERTIES: [&str; 6] = [
    "color",
    "background",
    "font-weight",
    "font-style",
    "text-decoration",
    "icon",
];

/// The glyph weight a `font-weight` declaration carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FontWeight {
    Normal,
    Bold,
}

/// The slant a `font-style` declaration carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FontSlant {
    Normal,
    Italic,
}

/// The decoration a `text-decoration` declaration carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TextDecoration {
    None,
    Underline,
    LineThrough,
}

/// One declaration: a parsed property with its value, ready for the
/// cascade. Built only by [`parse_declarations`] — the one
/// validator both sources share (the manifest's JSON and the verb's
/// Lua table cross as the same serde object), so the vocabulary
/// cannot drift between them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Declaration {
    Color(SlotColor),
    Background(SlotColor),
    Weight(FontWeight),
    Slant(FontSlant),
    Decoration(TextDecoration),
    Icon(String),
}

/// One compound selector: an optional element name leading, `.class`
/// parts, one optional `#id` — the class and id parts in any order.
/// Matching is exact (CSS case-sensitivity); the addressable names
/// are the spec's tree. Built only by [`parse_selector`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    element: Option<String>,
    classes: Vec<String>,
    id: Option<String>,
}

/// Specificity as the CSS tuple: ids, then classes, then element
/// names — compared lexicographically. Inline declarations are above
/// the tuple entirely (they carry no specificity; they win outright).
pub type Specificity = (usize, usize, usize);

impl Selector {
    /// The selector's specificity tuple.
    pub fn specificity(&self) -> Specificity {
        (
            usize::from(self.id.is_some()),
            self.classes.len(),
            usize::from(self.element.is_some()),
        )
    }

    /// Whether every named part is present on the node, exactly:
    /// the element name, every class, the id. A part the selector
    /// does not name matches anything.
    pub fn matches(&self, node: &StyleNode) -> bool {
        self.element
            .as_deref()
            .is_none_or(|element| element == node.element)
            && self
                .classes
                .iter()
                .all(|class| node.classes.contains(&class.as_str()))
            && self.id.as_deref().is_none_or(|id| node.id == Some(id))
    }
}

/// Parse one compound selector: an optional element name leading,
/// then `.class` parts and one optional `#id` in any order — CSS's
/// compound-selector shape, where a bare identifier is always the
/// element and only leads. Outer whitespace is trimmed; anything
/// else ungrammatical is the `Err` — the caller's one drop line,
/// naming the selector.
pub fn parse_selector(raw: &str) -> Result<Selector, String> {
    let raw = raw.trim();
    let fail = |reason: String| format!("the selector '{raw}' does not parse: {reason}");
    if raw.is_empty() {
        return Err(fail("empty".to_owned()));
    }
    let mut selector = Selector {
        element: None,
        classes: Vec::new(),
        id: None,
    };
    let mut rest = raw;
    while let Some(head) = rest.chars().next() {
        // The part's kind rides its head character; the name scan
        // takes CSS-identifier characters — letters, digits, hyphens,
        // underscores — and must start with a letter. The scan is
        // maximal munch, so after any part the next head is a part
        // marker or a foreign character: a bare identifier is the
        // element and only leads, by construction.
        let (kind, tail) = match head {
            '.' => (Part::Class, &rest[1..]),
            '#' => (Part::Id, &rest[1..]),
            _ if head.is_ascii_alphabetic() => (Part::Element, rest),
            _ => {
                return Err(fail(format!(
                    "the character '{head}' is not part of a selector"
                )));
            }
        };
        let name_end = tail
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_'))
            .map_or(tail.len(), |(i, _)| i);
        let name = &tail[..name_end];
        if name.is_empty() {
            return Err(fail(format!("the '{head}' part has no name")));
        }
        if !name.chars().next().unwrap_or('\0').is_ascii_alphabetic() {
            return Err(fail(format!(
                "the name '{name}' does not start with a letter"
            )));
        }
        rest = &tail[name_end..];
        match kind {
            Part::Element => selector.element = Some(name.to_owned()),
            Part::Class => selector.classes.push(name.to_owned()),
            Part::Id => {
                if selector.id.is_some() {
                    return Err(fail("two ids".to_owned()));
                }
                selector.id = Some(name.to_owned());
            }
        }
    }
    Ok(selector)
}

enum Part {
    Element,
    Class,
    Id,
}

/// One rule's declarations, parsed from the shared table form — the
/// serde object both sources cross as (the manifest's
/// `declarations`, the verb's `style` table through mlua's serde
/// path). Unknown properties and values drop their one declaration
/// with one line; the rest of the rule stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedDeclarations {
    pub declarations: Vec<Declaration>,
    pub drops: Vec<String>,
}

/// Parse one declarations table by the closed vocabulary.
pub fn parse_declarations(value: &serde_json::Value) -> ParsedDeclarations {
    let Some(map) = value.as_object() else {
        return ParsedDeclarations {
            drops: vec!["the declarations are not a table".to_owned()],
            declarations: Vec::new(),
        };
    };
    let mut parsed = ParsedDeclarations::default();
    for (property, value) in map {
        match parse_declaration(property, value) {
            Ok(declaration) => parsed.declarations.push(declaration),
            Err(line) => parsed.drops.push(line),
        }
    }
    parsed
}

fn parse_declaration(property: &str, value: &serde_json::Value) -> Result<Declaration, String> {
    fn as_string<'a>(property: &str, value: &'a serde_json::Value) -> Result<&'a str, String> {
        value
            .as_str()
            .ok_or_else(|| format!("ignoring '{property}': the value is not a string"))
    }
    match property {
        "color" | "background" => {
            let raw = as_string(property, value)?;
            SlotColor::parse(raw)
                .map(|color| {
                    if property == "color" {
                        Declaration::Color(color)
                    } else {
                        Declaration::Background(color)
                    }
                })
                .map_err(|err| format!("ignoring '{property}': {err}"))
        }
        "font-weight" => match as_string(property, value)? {
            "bold" => Ok(Declaration::Weight(FontWeight::Bold)),
            "normal" => Ok(Declaration::Weight(FontWeight::Normal)),
            other => Err(format!(
                "ignoring 'font-weight' value '{other}': bold or normal"
            )),
        },
        "font-style" => match as_string(property, value)? {
            "italic" => Ok(Declaration::Slant(FontSlant::Italic)),
            "normal" => Ok(Declaration::Slant(FontSlant::Normal)),
            other => Err(format!(
                "ignoring 'font-style' value '{other}': italic or normal"
            )),
        },
        "text-decoration" => match as_string(property, value)? {
            "underline" => Ok(Declaration::Decoration(TextDecoration::Underline)),
            "line-through" => Ok(Declaration::Decoration(TextDecoration::LineThrough)),
            "none" => Ok(Declaration::Decoration(TextDecoration::None)),
            other => Err(format!(
                "ignoring 'text-decoration' value '{other}': underline, line-through, or none"
            )),
        },
        "icon" => Ok(Declaration::Icon(as_string(property, value)?.to_owned())),
        other => Err(format!("ignoring unknown style property '{other}'")),
    }
}

/// A rule's layer: the cascade's origin strata, source order fixed —
/// builtin first, manifest (dir-sorted) second, runtime (arrival)
/// third. The plugin id rides the plugin-owned origins so one
/// caller's sheet can be replaced or dropped without touching its
/// neighbors'.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleOrigin {
    /// The host's own rules — the lowest layer, the shipped look.
    Builtin,
    /// One plugin's manifest sheet.
    Manifest { plugin: String },
    /// One plugin's runtime sheet (`dropterm.styles`).
    Runtime { plugin: String },
}

impl RuleOrigin {
    fn is_runtime_of(&self, plugin: &str) -> bool {
        matches!(self, RuleOrigin::Runtime { plugin: owner } if owner == plugin)
    }
}

/// One rule: where it came from, what it matches, what it says.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub origin: RuleOrigin,
    pub selector: Selector,
    pub declarations: Vec<Declaration>,
}

/// One addressable node: what a selector matches against — the
/// element name, the classes, the id, and the node's own inline
/// declarations (the `style` table, above everything).
#[derive(Debug, Clone, Copy)]
pub struct StyleNode<'a> {
    pub element: &'a str,
    pub classes: &'a [&'a str],
    pub id: Option<&'a str>,
    pub inline: &'a [Declaration],
}

/// The cascade's answer for one node — `None` means "unset, inherit
/// from the base layer" (the scheme's value for that surface).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedStyle {
    pub color: Option<SlotColor>,
    pub background: Option<SlotColor>,
    pub weight: Option<FontWeight>,
    pub slant: Option<FontSlant>,
    pub decoration: Option<TextDecoration>,
    pub icon: Option<String>,
}

impl ResolvedStyle {
    fn apply_inline(&mut self, declaration: &Declaration) {
        match declaration {
            Declaration::Color(color) => self.color = Some(color.clone()),
            Declaration::Background(color) => self.background = Some(color.clone()),
            Declaration::Weight(weight) => self.weight = Some(*weight),
            Declaration::Slant(slant) => self.slant = Some(*slant),
            Declaration::Decoration(decoration) => self.decoration = Some(*decoration),
            Declaration::Icon(icon) => self.icon = Some(icon.clone()),
        }
    }
}

/// The per-property cascade's ledger: each property's standing
/// winner with its rule's specificity. A later declaration at equal
/// or higher specificity replaces it — the walk is source order, so
/// `>=` is the whole rule.
#[derive(Default)]
struct Winners {
    color: Option<(Specificity, SlotColor)>,
    background: Option<(Specificity, SlotColor)>,
    weight: Option<(Specificity, FontWeight)>,
    slant: Option<(Specificity, FontSlant)>,
    decoration: Option<(Specificity, TextDecoration)>,
    icon: Option<(Specificity, String)>,
}

fn offer<T>(slot: &mut Option<(Specificity, T)>, specificity: Specificity, value: T) {
    if slot.as_ref().is_none_or(|(best, _)| specificity >= *best) {
        *slot = Some((specificity, value));
    }
}

impl Winners {
    fn offer_declaration(&mut self, declaration: &Declaration, specificity: Specificity) {
        match declaration {
            Declaration::Color(color) => offer(&mut self.color, specificity, color.clone()),
            Declaration::Background(color) => {
                offer(&mut self.background, specificity, color.clone());
            }
            Declaration::Weight(weight) => offer(&mut self.weight, specificity, *weight),
            Declaration::Slant(slant) => offer(&mut self.slant, specificity, *slant),
            Declaration::Decoration(decoration) => {
                offer(&mut self.decoration, specificity, *decoration);
            }
            Declaration::Icon(icon) => offer(&mut self.icon, specificity, icon.clone()),
        }
    }

    fn into_resolved(self) -> ResolvedStyle {
        ResolvedStyle {
            color: self.color.map(|(_, value)| value),
            background: self.background.map(|(_, value)| value),
            weight: self.weight.map(|(_, value)| value),
            slant: self.slant.map(|(_, value)| value),
            decoration: self.decoration.map(|(_, value)| value),
            icon: self.icon.map(|(_, value)| value),
        }
    }
}

/// The ordered rule layers. `resolve` walks builtin → manifest →
/// runtime in that fixed source order; within a layer, order is the
/// feed's arrival.
#[derive(Debug, Default)]
pub struct StyleSet {
    builtin: Vec<Rule>,
    manifest: Vec<Rule>,
    runtime: Vec<Rule>,
}

impl StyleSet {
    /// The host's own rules — set once at boot (the builtin layer).
    pub fn set_builtin(&mut self, rules: Vec<Rule>) {
        self.builtin = rules;
    }

    /// The manifest sheets' rules, wholesale — the shell feeds the
    /// whole scan at boot and on every watcher reload, so the layer
    /// is always the fresh scan's sheet.
    pub fn replace_manifest(&mut self, rules: Vec<Rule>) {
        self.manifest = rules;
    }

    /// One plugin's runtime sheet, replacing that plugin's whole
    /// sheet (the status-segment replacement-seat discipline): its
    /// old rules leave, the new rules join at the layer's arrival
    /// end.
    pub fn replace_runtime_sheet(&mut self, plugin: &str, rules: Vec<Rule>) {
        self.runtime
            .retain(|rule| !rule.origin.is_runtime_of(plugin));
        self.runtime.extend(rules);
    }

    /// One plugin's runtime sheet, dropped — the interpreter
    /// teardown's step. Manifest sheets stand.
    pub fn drop_runtime_sheet(&mut self, plugin: &str) {
        self.runtime
            .retain(|rule| !rule.origin.is_runtime_of(plugin));
    }

    /// The cascade's answer for one node: every matching rule
    /// competes per property by specificity, source order breaks
    /// ties (the layers' fixed order is that order), and the node's
    /// inline declarations take their properties above everything.
    pub fn resolve(&self, node: &StyleNode) -> ResolvedStyle {
        let mut winners = Winners::default();
        for rule in self
            .builtin
            .iter()
            .chain(self.manifest.iter())
            .chain(self.runtime.iter())
        {
            if !rule.selector.matches(node) {
                continue;
            }
            let specificity = rule.selector.specificity();
            for declaration in &rule.declarations {
                winners.offer_declaration(declaration, specificity);
            }
        }
        let mut resolved = winners.into_resolved();
        for declaration in node.inline {
            resolved.apply_inline(declaration);
        }
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The three layers assembled for a fixture set.
    fn layered(builtin: Vec<Rule>, manifest: Vec<Rule>, runtime: Vec<Rule>) -> StyleSet {
        StyleSet {
            builtin,
            manifest,
            runtime,
        }
    }

    /// One fixture rule: parses the selector and declarations,
    /// panicking on the fixture's own drift (fixture strings are
    /// pinned by the parse tests).
    fn rule(origin: RuleOrigin, selector: &str, declarations: &serde_json::Value) -> Rule {
        let selector = parse_selector(selector).expect("the fixture selector parses");
        let parsed = parse_declarations(declarations);
        assert!(parsed.drops.is_empty(), "{:?}", parsed.drops);
        Rule {
            origin,
            selector,
            declarations: parsed.declarations,
        }
    }

    fn make_node<'a>(
        element: &'a str,
        classes: &'a [&'a str],
        id: Option<&'a str>,
        inline: &'a [Declaration],
    ) -> StyleNode<'a> {
        StyleNode {
            element,
            classes,
            id,
            inline,
        }
    }

    // --- selector parse ---

    #[test]
    fn selector_parses_every_compound_shape_in_any_order() {
        assert_eq!(
            parse_selector("tab.active"),
            Ok(Selector {
                element: Some("tab".to_owned()),
                classes: vec!["active".to_owned()],
                id: None,
            })
        );
        assert_eq!(
            parse_selector(".warn"),
            Ok(Selector {
                element: None,
                classes: vec!["warn".to_owned()],
                id: None,
            })
        );
        assert_eq!(
            parse_selector("#rx"),
            Ok(Selector {
                element: None,
                classes: Vec::new(),
                id: Some("rx".to_owned()),
            })
        );
        assert_eq!(
            parse_selector("dialog-button.accent"),
            Ok(Selector {
                element: Some("dialog-button".to_owned()),
                classes: vec!["accent".to_owned()],
                id: None,
            })
        );
        // The element leads; the class and id parts ride in any
        // order after it.
        assert_eq!(
            parse_selector("tab.active#main"),
            Ok(Selector {
                element: Some("tab".to_owned()),
                classes: vec!["active".to_owned()],
                id: Some("main".to_owned()),
            })
        );
        assert_eq!(
            parse_selector("#main.tab.active"),
            parse_selector(".tab.active#main")
        );
        assert_eq!(
            parse_selector("#main.tab.active"),
            Ok(Selector {
                element: None,
                classes: vec!["tab".to_owned(), "active".to_owned()],
                id: Some("main".to_owned()),
            })
        );
        // Outer whitespace is trimmed; inner whitespace is not part
        // of the grammar.
        assert_eq!(parse_selector("  .warn  "), parse_selector(".warn"));
    }

    #[test]
    fn junk_selectors_drop_with_one_line_naming_it() {
        let drop = |raw: &str| parse_selector(raw).expect_err("drops");
        assert_eq!(drop(""), "the selector '' does not parse: empty");
        assert_eq!(drop("   "), "the selector '' does not parse: empty");
        assert_eq!(
            drop("tab."),
            "the selector 'tab.' does not parse: the '.' part has no name"
        );
        assert_eq!(
            drop("#"),
            "the selector '#' does not parse: the '#' part has no name"
        );
        assert_eq!(drop("#a#b"), "the selector '#a#b' does not parse: two ids");
        assert_eq!(
            drop(".9warn"),
            "the selector '.9warn' does not parse: the name '9warn' does not start with a letter"
        );
        assert_eq!(
            drop("tab span"),
            "the selector 'tab span' does not parse: the character ' ' is not part of a selector"
        );
        assert_eq!(
            drop("a>span"),
            "the selector 'a>span' does not parse: the character '>' is not part of a selector"
        );
        assert_eq!(
            drop("!important"),
            "the selector '!important' does not parse: the character '!' is not part of a selector"
        );
    }

    // --- declarations ---

    #[test]
    fn declarations_parse_the_closed_vocabulary() {
        let parsed = parse_declarations(&json!({
            "color": "accent",
            "background": "#1f9e7b",
            "font-weight": "bold",
            "font-style": "italic",
            "text-decoration": "line-through",
            "icon": "↓",
        }));
        assert!(parsed.drops.is_empty(), "{:?}", parsed.drops);
        assert_eq!(parsed.declarations.len(), 6);
        for declaration in [
            Declaration::Color(SlotColor::Token("accent".to_owned())),
            Declaration::Background(SlotColor::Hex("#1f9e7b".to_owned())),
            Declaration::Weight(FontWeight::Bold),
            Declaration::Slant(FontSlant::Italic),
            Declaration::Decoration(TextDecoration::LineThrough),
            Declaration::Icon("↓".to_owned()),
        ] {
            assert!(
                parsed.declarations.contains(&declaration),
                "missing {declaration:?}"
            );
        }
        // Every value word of the closed vocabulary parses.
        let decoration = |raw: &str| parse_declarations(&json!({ "text-decoration": raw }));
        assert!(decoration("underline").drops.is_empty());
        assert_eq!(
            decoration("none").declarations,
            vec![Declaration::Decoration(TextDecoration::None)]
        );
        let weight = |raw: &str| parse_declarations(&json!({ "font-weight": raw }));
        assert_eq!(
            weight("normal").declarations,
            vec![Declaration::Weight(FontWeight::Normal)]
        );
        let slant = |raw: &str| parse_declarations(&json!({ "font-style": raw }));
        assert_eq!(
            slant("normal").declarations,
            vec![Declaration::Slant(FontSlant::Normal)]
        );
        // An icon is any string — a glyph or a Lucide name.
        let icon = |raw: &str| parse_declarations(&json!({ "icon": raw }));
        assert_eq!(
            icon("arrow-down").declarations,
            vec![Declaration::Icon("arrow-down".to_owned())]
        );
    }

    #[test]
    fn unknown_property_value_and_bad_hex_each_drop_one_line_and_the_rest_stands() {
        let parsed = parse_declarations(&json!({
            "color": "accent",
            "frob": "x",
            "font-weight": "heavy",
            "background": "#gg",
            "font-style": "italic",
        }));
        assert_eq!(parsed.declarations.len(), 2);
        for declaration in [
            Declaration::Color(SlotColor::Token("accent".to_owned())),
            Declaration::Slant(FontSlant::Italic),
        ] {
            assert!(
                parsed.declarations.contains(&declaration),
                "missing {declaration:?}"
            );
        }
        // Each bad entry cost exactly its one line, naming the
        // declaration (order-independent: the compare sorts).
        let mut drops = parsed.drops.clone();
        drops.sort();
        assert_eq!(
            drops,
            vec![
                "ignoring 'background': the color '#gg' is neither a theme token nor #rrggbb"
                    .to_owned(),
                "ignoring 'font-weight' value 'heavy': bold or normal".to_owned(),
                "ignoring unknown style property 'frob'".to_owned(),
            ]
        );
    }

    #[test]
    fn a_non_string_value_drops_one_line() {
        let parsed = parse_declarations(&json!({ "icon": 5 }));
        assert!(parsed.declarations.is_empty());
        assert_eq!(
            parsed.drops,
            vec!["ignoring 'icon': the value is not a string".to_owned()]
        );
    }

    #[test]
    fn non_table_declarations_drop_one_line() {
        let parsed = parse_declarations(&json!("not a table"));
        assert!(parsed.declarations.is_empty());
        assert_eq!(
            parsed.drops,
            vec!["the declarations are not a table".to_owned()]
        );
    }

    // --- the cascade ---

    #[test]
    fn the_cascade_ladder_runs_a_node_through_all_four_strata() {
        fn ladder(inline: &[Declaration]) -> StyleNode<'_> {
            make_node("span", &["warn"], Some("rx"), inline)
        }
        let inline = [Declaration::Color(SlotColor::Token("green".to_owned()))];
        let builtin = vec![rule(
            RuleOrigin::Builtin,
            "span",
            &json!({ "color": "black" }),
        )];
        let manifest = vec![rule(
            RuleOrigin::Manifest {
                plugin: "m".to_owned(),
            },
            ".warn",
            &json!({ "color": "red" }),
        )];
        let runtime = vec![rule(
            RuleOrigin::Runtime {
                plugin: "m".to_owned(),
            },
            "#rx",
            &json!({ "color": "blue" }),
        )];

        // The builtin layer alone: the element rule stands.
        let set = layered(builtin.clone(), vec![], vec![]);
        assert_eq!(
            set.resolve(&ladder(&[])).color,
            Some(SlotColor::Token("black".to_owned()))
        );

        // The manifest sheet joins at equal specificity, later in
        // source order: the class rule wins.
        let set = layered(builtin.clone(), manifest.clone(), vec![]);
        assert_eq!(
            set.resolve(&ladder(&[])).color,
            Some(SlotColor::Token("red".to_owned()))
        );

        // The runtime sheet joins: the id rule wins over the class.
        let set = layered(builtin, manifest, runtime);
        assert_eq!(
            set.resolve(&ladder(&[])).color,
            Some(SlotColor::Token("blue".to_owned()))
        );

        // And the node's inline declarations sit above everything.
        assert_eq!(
            set.resolve(&ladder(&inline)).color,
            Some(SlotColor::Token("green".to_owned()))
        );
    }

    #[test]
    fn an_id_rule_wins_across_layers_over_a_later_class_rule() {
        let set = layered(
            vec![rule(
                RuleOrigin::Builtin,
                "#main",
                &json!({ "color": "black" }),
            )],
            vec![rule(
                RuleOrigin::Manifest {
                    plugin: "m".to_owned(),
                },
                ".warn",
                &json!({ "color": "red" }),
            )],
            vec![],
        );
        let node = make_node("span", &["warn"], Some("main"), &[]);
        assert_eq!(
            set.resolve(&node).color,
            Some(SlotColor::Token("black".to_owned()))
        );
    }

    #[test]
    fn equal_specificity_breaks_by_source_order_within_a_sheet() {
        let set = layered(
            vec![],
            vec![],
            vec![
                rule(
                    RuleOrigin::Runtime {
                        plugin: "m".to_owned(),
                    },
                    ".warn",
                    &json!({ "color": "black" }),
                ),
                rule(
                    RuleOrigin::Runtime {
                        plugin: "m".to_owned(),
                    },
                    ".warn",
                    &json!({ "color": "blue" }),
                ),
            ],
        );
        let node = make_node("span", &["warn"], None, &[]);
        assert_eq!(
            set.resolve(&node).color,
            Some(SlotColor::Token("blue".to_owned()))
        );
    }

    #[test]
    fn rules_compete_per_property_not_per_rule() {
        // The builtin accent look carries background and color; a
        // plugin rule out-orders it for color only — the builtin
        // background stands.
        let set = layered(
            vec![rule(
                RuleOrigin::Builtin,
                "dialog-button.accent",
                &json!({ "background": "accent", "color": "fg" }),
            )],
            vec![rule(
                RuleOrigin::Manifest {
                    plugin: "m".to_owned(),
                },
                "dialog-button.accent",
                &json!({ "color": "red" }),
            )],
            vec![],
        );
        let node = make_node("dialog-button", &["accent"], None, &[]);
        let resolved = set.resolve(&node);
        assert_eq!(resolved.color, Some(SlotColor::Token("red".to_owned())));
        assert_eq!(
            resolved.background,
            Some(SlotColor::Token("accent".to_owned()))
        );
    }

    #[test]
    fn a_non_matching_rule_never_applies() {
        let set = layered(
            vec![
                rule(RuleOrigin::Builtin, "tab", &json!({ "color": "black" })),
                rule(RuleOrigin::Builtin, ".missing", &json!({ "color": "red" })),
                rule(RuleOrigin::Builtin, "#other", &json!({ "color": "blue" })),
            ],
            vec![],
            vec![],
        );
        let node = make_node("span", &["warn"], Some("rx"), &[]);
        assert_eq!(set.resolve(&node), ResolvedStyle::default());
    }

    #[test]
    fn unset_properties_resolve_none() {
        let set = layered(vec![], vec![], vec![]);
        let node = make_node("span", &[], None, &[]);
        assert_eq!(set.resolve(&node), ResolvedStyle::default());
    }

    // --- the layers ---

    #[test]
    fn the_manifest_sheet_replaces_wholesale() {
        let mut set = layered(
            vec![],
            vec![rule(
                RuleOrigin::Manifest {
                    plugin: "m".to_owned(),
                },
                ".old",
                &json!({ "color": "black" }),
            )],
            vec![],
        );
        set.replace_manifest(vec![rule(
            RuleOrigin::Manifest {
                plugin: "m".to_owned(),
            },
            ".new",
            &json!({ "color": "blue" }),
        )]);
        let node = make_node("span", &["old"], None, &[]);
        assert_eq!(set.resolve(&node).color, None);
        let node = make_node("span", &["new"], None, &[]);
        assert_eq!(
            set.resolve(&node).color,
            Some(SlotColor::Token("blue".to_owned()))
        );
    }

    #[test]
    fn the_runtime_sheet_replaces_per_caller() {
        let mut set = layered(
            vec![],
            vec![],
            vec![
                rule(
                    RuleOrigin::Runtime {
                        plugin: "a".to_owned(),
                    },
                    ".old",
                    &json!({ "color": "black" }),
                ),
                rule(
                    RuleOrigin::Runtime {
                        plugin: "b".to_owned(),
                    },
                    ".b",
                    &json!({ "color": "red" }),
                ),
            ],
        );
        set.replace_runtime_sheet(
            "a",
            vec![rule(
                RuleOrigin::Runtime {
                    plugin: "a".to_owned(),
                },
                ".new",
                &json!({ "color": "blue" }),
            )],
        );
        // Caller a's old rules left; caller b's rules stand.
        let node = make_node("span", &["old"], None, &[]);
        assert_eq!(set.resolve(&node).color, None);
        let node = make_node("span", &["new"], None, &[]);
        assert_eq!(
            set.resolve(&node).color,
            Some(SlotColor::Token("blue".to_owned()))
        );
        let node = make_node("span", &["b"], None, &[]);
        assert_eq!(
            set.resolve(&node).color,
            Some(SlotColor::Token("red".to_owned()))
        );
        // And the replacement arrived after b's rules: at equal
        // specificity on a shared selector, caller a's sheet wins.
        set.replace_runtime_sheet(
            "a",
            vec![rule(
                RuleOrigin::Runtime {
                    plugin: "a".to_owned(),
                },
                ".b",
                &json!({ "color": "blue" }),
            )],
        );
        let node = make_node("span", &["b"], None, &[]);
        assert_eq!(
            set.resolve(&node).color,
            Some(SlotColor::Token("blue".to_owned()))
        );
    }

    #[test]
    fn the_runtime_sheet_drops_at_teardown_and_manifest_stands() {
        let mut set = layered(
            vec![],
            vec![rule(
                RuleOrigin::Manifest {
                    plugin: "m".to_owned(),
                },
                ".warn",
                &json!({ "color": "red" }),
            )],
            vec![rule(
                RuleOrigin::Runtime {
                    plugin: "m".to_owned(),
                },
                ".warn",
                &json!({ "color": "blue" }),
            )],
        );
        set.drop_runtime_sheet("m");
        let node = make_node("span", &["warn"], None, &[]);
        assert_eq!(
            set.resolve(&node).color,
            Some(SlotColor::Token("red".to_owned()))
        );
    }
}
