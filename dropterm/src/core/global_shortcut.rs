// SPDX-License-Identifier: GPL-3.0-only

//! The global summon key — the OS-level show/hide hotkey (global-
//! toggle-guidance spec). `DropTerm`'s in-app chords die with focus
//! (they are the `core::keybinds` domain); this module owns the
//! external half: reading the COSMIC custom-shortcuts config that
//! actually summons the overlay while it is hidden, recognizing the
//! entries that spawn this binary's toggle verb, and the onboarding's
//! trigger decision.
//!
//! The config shapes below port `cosmic-settings-config`'s shortcuts
//! module (pop-os/cosmic-settings-daemon, MPL-2.0,
//! `config/src/shortcuts/{binding,sym,modifier}.rs`) — the file format
//! is that crate's contract, and carrying it keeps the port aligned
//! the way `core::keybinds` carries cosmic-term's vocabulary. The
//! action side stays opaque: COSMIC's `custom` key holds every action
//! variant — `Spawn(..)` beside `Disable` and the built-ins' `Close`,
//! `Focus(..)` — so only the binding keys are typed and each action
//! value's own RON is carried verbatim (a store refuses only when it
//! is malformed or a keysym is unresolvable, and nothing is ever
//! written over an unreadable map).

use ron::value::RawValue;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use xkbcommon::xkb::{self, Keysym};

// --- the ported shapes (cosmic-settings-config, MPL-2.0) ---

/// A modifier name in the shortcuts file's array form (`Super`, `Ctrl`,
/// `Alt`, `Shift`) — the same four the house [`crate::core::keybinds`]
/// persists, spelled the way cosmic-settings serializes them.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Super,
}

/// The modifier flags one binding carries.
#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "format contract: the file's four flag fields are the on-disk shape the port carries"
)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
}

impl Modifiers {
    pub const fn new() -> Self {
        Self {
            ctrl: false,
            alt: false,
            shift: false,
            logo: false,
        }
    }

    pub const fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub const fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub const fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub const fn logo(mut self) -> Self {
        self.logo = true;
        self
    }
}

impl std::ops::AddAssign<Modifier> for Modifiers {
    fn add_assign(&mut self, rhs: Modifier) {
        match rhs {
            Modifier::Ctrl => self.ctrl = true,
            Modifier::Alt => self.alt = true,
            Modifier::Shift => self.shift = true,
            Modifier::Super => self.logo = true,
        }
    }
}

/// The file's array form of [`Modifiers`] — `[Super, Ctrl, ...]` — the
/// bridge the crate's `ModifiersDef` carries.
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ModifiersDef(pub Vec<Modifier>);

impl From<Modifiers> for ModifiersDef {
    fn from(src: Modifiers) -> Self {
        let mut modifiers = Vec::new();
        if src.logo {
            modifiers.push(Modifier::Super);
        }
        if src.ctrl {
            modifiers.push(Modifier::Ctrl);
        }
        if src.alt {
            modifiers.push(Modifier::Alt);
        }
        if src.shift {
            modifiers.push(Modifier::Shift);
        }
        Self(modifiers)
    }
}

impl From<ModifiersDef> for Modifiers {
    fn from(src: ModifiersDef) -> Self {
        src.0
            .into_iter()
            .fold(Modifiers::new(), |mut modifiers, modifier| {
                modifiers += modifier;
                modifiers
            })
    }
}

/// One binding in the shortcuts file: the flags, the optional key (a
/// keysym, parsed from its name), an optional raw keycode, and an
/// optional human description — the shape cosmic-settings writes and
/// cosmic-comp reads. Identity (the [`PartialEq`]/[`Hash`] pair) is
/// flags + key, the crate's own rule: two entries differing only in
/// description collide as one binding.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    #[serde(
        deserialize_with = "deserialize_modifiers",
        serialize_with = "serialize_modifiers"
    )]
    pub modifiers: Modifiers,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "sym::deserialize",
        serialize_with = "sym::serialize"
    )]
    pub key: Option<Keysym>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keycode: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl PartialEq for Binding {
    fn eq(&self, other: &Self) -> bool {
        self.modifiers == other.modifiers && self.key == other.key
    }
}

impl Eq for Binding {}

impl std::hash::Hash for Binding {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.modifiers.hash(state);
    }
}

/// The keysym name's serde pair — the crate's `sym.rs`: parse via
/// `keysym_from_name` (exact, then case-insensitive, never silently
/// unresolved) and serialize via the canonical `keysym_get_name`.
mod sym {
    use super::{Keysym, xkb};
    use serde::{Deserialize, Deserializer, Serializer};

    /// `keysym_from_name`'s "no such keysym" sentinel (0).
    const NO_SYMBOL: u32 = 0;

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Option<Keysym>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        match xkb::keysym_from_name(&name, xkb::KEYSYM_NO_FLAGS) {
            keysym if keysym.raw() == NO_SYMBOL => {
                match xkb::keysym_from_name(&name, xkb::KEYSYM_CASE_INSENSITIVE) {
                    keysym if keysym.raw() == NO_SYMBOL => {
                        Err(<D::Error as serde::de::Error>::invalid_value(
                            serde::de::Unexpected::Str(&name),
                            &"one of xkbcommon's keysym names",
                        ))
                    }
                    keysym => {
                        log::warn!(
                            "key binding '{}' only matched case-insensitive for {:?}",
                            name,
                            xkb::keysym_get_name(keysym)
                        );
                        Ok(Some(keysym))
                    }
                }
            }
            keysym => Ok(Some(keysym)),
        }
    }

    /// serde's `serialize_with` signature contract pins `&Option<T>`
    /// — both lints below are that signature, not a style choice.
    #[expect(
        clippy::ref_option,
        reason = "serde's serialize_with signature contract"
    )]
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde's serialize_with signature contract"
    )]
    pub(super) fn serialize<S>(key: &Option<Keysym>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match key {
            Some(key) => serializer.serialize_str(&xkb::keysym_get_name(*key)),
            None => serializer.serialize_str("None"),
        }
    }
}

/// The flags' serde pair: deserialize the array form, serialize it
/// back in the crate's canonical order (Super, Ctrl, Alt, Shift).
fn serialize_modifiers<S>(modifiers: &Modifiers, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    ModifiersDef::from(modifiers.clone()).serialize(serializer)
}

fn deserialize_modifiers<'de, D>(deserializer: D) -> Result<Modifiers, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Modifiers::from(ModifiersDef::deserialize(deserializer)?))
}

/// The custom file's action value — opaque by contract: COSMIC writes
/// every action variant into the same `custom` key (`Spawn(..)` beside
/// `Disable` and the built-ins' `Close`, `Focus(..)`), so the value
/// keeps its own RON text verbatim and `DropTerm` probes only the
/// `Spawn` shape it recognizes ([`CustomActionValue::spawn_command`]).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CustomActionValue(Box<RawValue>);

impl<'de> Deserialize<'de> for CustomActionValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self(
            Box::<RawValue>::deserialize(deserializer)?.trim_boxed(),
        ))
    }
}

/// The `Spawn` shape [`CustomActionValue::spawn_command`] probes an
/// opaque value against.
#[derive(Deserialize)]
enum SpawnAction {
    Spawn(String),
}

impl CustomActionValue {
    /// The command this value runs when it is a `Spawn` — `None` for
    /// every other action (`Disable`, a rebound built-in).
    pub fn spawn_command(&self) -> Option<String> {
        self.0
            .into_rust::<SpawnAction>()
            .ok()
            .map(|SpawnAction::Spawn(command)| command)
    }
}

/// The `custom` file's parsed map.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CustomShortcuts(pub HashMap<Binding, CustomActionValue>);

// --- the read seam and its degrade contract ---

/// What reading the custom map yielded — everything but [`ReadCustom::Loaded`]
/// degrades the Global row to plain guidance (never an error surface,
/// never a write).
#[derive(Debug, PartialEq)]
pub enum ReadCustom {
    Loaded(CustomShortcuts),
    /// No shortcuts file: a fresh COSMIC session, or not COSMIC.
    Absent,
    /// The file exists but could not be read (permissions, ...).
    Unreadable,
    /// The file exists but does not parse (malformed RON, an
    /// unresolvable keysym name).
    Unparseable,
}

/// The COSMIC custom-shortcuts file, under the user's config home —
/// the file cosmic-settings writes and cosmic-comp watches for live
/// reloads (`com.system76.CosmicSettings.Shortcuts/v1/custom`).
pub fn custom_shortcuts_path(home: &Path) -> PathBuf {
    home.join(".config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom")
}

/// Read and parse the custom map at `path`.
pub fn read_custom(path: &Path) -> ReadCustom {
    match std::fs::read_to_string(path) {
        Ok(text) => match parse_custom(&text) {
            Ok(shortcuts) => ReadCustom::Loaded(shortcuts),
            Err(_) => ReadCustom::Unparseable,
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => ReadCustom::Absent,
        Err(_) => ReadCustom::Unreadable,
    }
}

/// Parse the `custom` file's text.
pub fn parse_custom(text: &str) -> Result<CustomShortcuts, String> {
    ron::from_str(text).map_err(|err| err.to_string())
}

/// Serialize the custom map back to `custom` file text.
pub fn serialize_custom(shortcuts: &CustomShortcuts) -> Result<String, String> {
    ron::to_string(shortcuts).map_err(|err| err.to_string())
}

// --- recognition and display ---

/// Whether a spawn command summons *this* `DropTerm`: its program is our
/// own binary (the same file stem) and its final word is the toggle
/// verb (`dropterm toggle`, `/usr/bin/dropterm toggle`, and — for the
/// dev build — `~/.local/bin/dropterm-dev toggle`).
pub fn is_toggle_spawn(command: &str, exe_stem: &str) -> bool {
    let words: Vec<&str> = command.split_whitespace().collect();
    if words.len() < 2 || *words.last().expect("at least two words") != "toggle" {
        return false;
    }
    Path::new(words[0])
        .file_stem()
        .is_some_and(|stem| stem == std::ffi::OsStr::new(exe_stem))
}

/// The custom map's entries that summon this `DropTerm` — the Global
/// row's read-only display and the onboarding's trigger input — sorted
/// by their display form.
pub fn summon_entries(shortcuts: &CustomShortcuts, exe_stem: &str) -> Vec<Binding> {
    let mut found: Vec<Binding> = shortcuts
        .0
        .iter()
        .filter(|(_, value)| {
            value
                .spawn_command()
                .is_some_and(|command| is_toggle_spawn(&command, exe_stem))
        })
        .map(|(binding, _)| binding.clone())
        .collect();
    found.sort_by_key(binding_display);
    found
}

/// The human form of a binding — `Super + Shift + E` — the house chord
/// display's labels and ` + ` join over the keysym name (single ASCII
/// letters uppercase, the editor rows' vocabulary; a keyless binding
/// renders its modifiers alone).
pub fn binding_display(binding: &Binding) -> String {
    let mut parts: Vec<String> = Vec::new();
    if binding.modifiers.logo {
        parts.push("Super".to_string());
    }
    if binding.modifiers.ctrl {
        parts.push("Ctrl".to_string());
    }
    if binding.modifiers.alt {
        parts.push("Alt".to_string());
    }
    if binding.modifiers.shift {
        parts.push("Shift".to_string());
    }
    if let Some(key) = binding.key {
        let name = xkb::keysym_get_name(key);
        if name.len() == 1 && name.chars().all(|ch| ch.is_ascii_alphabetic()) {
            parts.push(name.to_uppercase());
        } else {
            parts.push(name);
        }
    }
    parts.join(" + ")
}

/// This binary's file stem, for [`is_toggle_spawn`] and
/// [`summon_entries`].
pub fn own_exe_stem() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    exe.file_stem()?.to_str().map(str::to_string)
}

// --- environment, walk, onboarding ---

/// The desktop family the summon guidance speaks to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Environment {
    /// A COSMIC session — the walk is COSMIC's custom-shortcut page,
    /// and the deep link exists.
    Cosmic,
    /// Anything else — the generic DE-shortcut walk.
    #[default]
    Other,
}

/// Read the session family from `XDG_CURRENT_DESKTOP`'s value
/// (colon-separated per the spec; `None` is no session variable).
pub fn detect_environment(desktop: Option<&str>) -> Environment {
    match desktop {
        Some(desktop)
            if desktop
                .split(':')
                .any(|part| part.eq_ignore_ascii_case("cosmic")) =>
        {
            Environment::Cosmic
        }
        _ => Environment::Other,
    }
}

/// The summon-setup walk for the environment — the Global row's body
/// and the first-run prompt's guidance half. Every walk names the
/// toggle command verbatim.
pub fn summon_walk(environment: Environment) -> &'static [&'static str] {
    match environment {
        Environment::Cosmic => &[
            "Open COSMIC Settings → Keyboard → Custom Shortcuts.",
            "Add a shortcut whose command is `dropterm toggle`.",
            "Prefer an unshifted key such as F12 — some Shift-modified custom shortcuts fail to fire on COSMIC.",
        ],
        Environment::Other => &[
            "Open your desktop's keyboard-shortcut settings.",
            "Bind a key to the command `dropterm toggle`.",
        ],
    }
}

/// The onboarding's cold-boot decision: show while no summon binding
/// exists and the flow is not retired. Never consulted on a warm
/// launch.
pub fn onboarding_should_show(summon_bound: bool, retired: bool) -> bool {
    !summon_bound && !retired
}

/// Which setup paths a session offers for the summon key — the
/// onboarding's one question per session. `configure_for_me` is off
/// everywhere until its provider exists (the writer turns the COSMIC
/// arm on in its own slice), and off wherever the store cannot be
/// read (an unreadable or unparseable map degrades to guidance).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SetupOptions {
    /// Whether the "Configure for me" path is offered this session.
    pub configure_for_me: bool,
}

/// The session's setup options — the pure answer the onboarding window
/// renders. The parameters are the future decision's inputs: the
/// writer's W3 turns the COSMIC arm on over a loaded store.
pub fn setup_options(environment: Environment, store: &ReadCustom) -> SetupOptions {
    // The "Configure for me" provider is the writer module
    // (SPEC-global-toggle-cosmic-writer); it does not exist yet, so
    // every cell answers the safe default. W3 turns the arm on as
    // `matches!(environment, Environment::Cosmic)
    //     && matches!(store, ReadCustom::Loaded(_))`.
    let _ = (environment, store);
    SetupOptions {
        configure_for_me: false,
    }
}

// --- the writer (global-toggle-cosmic-writer spec): the captured
// chord's conversion, the verdict over the effective map, the commit
// and revert planners, and the write seam ---

/// The description on the one entry this module owns; hand-made
/// entries are never touched.
pub const MARKER_DESCRIPTION: &str = "DropTerm";

/// A captured chord's conversion outcome — what the onboarding's
/// configure-for-me capture hands the verdict path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapturedChord {
    /// The binding the store can hold: the verdict decides its fate.
    Binding(Binding),
    /// A bare modifier press (`shift`, `ctrl`, …) — the capture stays
    /// armed, exactly the keyboard page's own listener behavior
    /// (cosmic-settings filters Super/Alt/Control/Shift presses while
    /// listening).
    BareModifier,
    /// The key name resolves to no keysym: unsupported, the capture
    /// disarms and nothing writes.
    Unresolvable,
}

/// gpui's lowercase platform key names onto xkb's canonical keysym
/// names — the named keys the capture can deliver (the mirror image
/// of `terminal/input.rs`'s `gpui_key_name`). Anything outside this
/// bridge falls through to `keysym_from_name` exact then
/// case-insensitive; a single character routes through
/// `Keysym::from_char` — the daemon's own capture path
/// (`Binding::from_str_partial`).
fn xkb_key_name(key: &str) -> Option<&'static str> {
    match key {
        "space" => Some("space"),
        "pageup" => Some("Page_Up"),
        "pagedown" => Some("Page_Down"),
        "home" => Some("Home"),
        "end" => Some("End"),
        "insert" => Some("Insert"),
        "delete" => Some("Delete"),
        "backspace" => Some("Backspace"),
        "tab" => Some("Tab"),
        "enter" => Some("Return"),
        "escape" => Some("Escape"),
        "left" => Some("Left"),
        "right" => Some("Right"),
        "up" => Some("Up"),
        "down" => Some("Down"),
        _ => None,
    }
}

/// The modifier names gpui delivers for a bare press — the same
/// refusal list `core::keybinds`' capture carries.
fn is_modifier_name(key: &str) -> bool {
    matches!(
        key,
        "control" | "ctrl" | "shift" | "alt" | "super" | "meta" | "logo"
    )
}

/// A captured chord — gpui's modifier flags and key name — as the
/// store can hold it. Single ASCII letters lowercase to the exact
/// form cosmic-settings itself writes (the shipped store's `key:
/// "e"` entries beside `modifiers: [Super, Shift]`).
pub fn captured_chord(modifiers: Modifiers, key: &str) -> CapturedChord {
    if is_modifier_name(key) {
        return CapturedChord::BareModifier;
    }
    let keysym = match key {
        "" => return CapturedChord::Unresolvable,
        single if single.chars().count() == 1 => {
            // The daemon's capture route: one character is its own
            // keysym, lowercased to the shipped entries' form.
            let lowered = single.to_lowercase();
            Some(xkb::Keysym::from_char(
                lowered.chars().next().expect("one character"),
            ))
        }
        named => xkb_key_name(named)
            .and_then(|name| {
                let keysym = xkb::keysym_from_name(name, xkb::KEYSYM_NO_FLAGS);
                (keysym.raw() != 0).then_some(keysym)
            })
            .or_else(|| {
                let keysym = xkb::keysym_from_name(named, xkb::KEYSYM_CASE_INSENSITIVE);
                (keysym.raw() != 0).then_some(keysym)
            }),
    };
    match keysym {
        Some(key) => CapturedChord::Binding(Binding {
            modifiers,
            key: Some(key),
            keycode: None,
            description: None,
        }),
        None => CapturedChord::Unresolvable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sanitized shape of a real COSMIC custom-shortcuts file:
    /// two `DropTerm` summon entries, two foreign spawns, two disables
    /// (one keyless), an escaped apostrophe in a foreign description.
    const FIXTURE: &str = r#"{
    (
        modifiers: [
            Super,
            Shift,
        ],
        key: "e",
        description: Some("dropterm"),
    ): Spawn("dropterm toggle"),
    (
        modifiers: [],
        key: "F12",
        description: Some("dropterm-dev"),
    ): Spawn("~/.local/bin/dropterm-dev toggle"),
    (
        modifiers: [
            Alt,
        ],
        key: "grave",
        description: Some("Assistant: every workspace\'s windows"),
    ): Spawn("assistant show-all-windows"),
    (
        modifiers: [
            Alt,
            Shift,
        ],
        key: "grave",
        description: Some("Assistant: same app windows, backwards"),
    ): Spawn("assistant same-app-previous"),
    (
        modifiers: [
            Super,
        ],
    ): Disable,
    (
        modifiers: [
            Super,
            Ctrl,
        ],
        key: "k",
    ): Disable,
}"#;

    #[test]
    fn the_fixture_parses_and_the_summon_entries_surface() {
        let shortcuts = parse_custom(FIXTURE).expect("the fixture parses");
        assert_eq!(shortcuts.0.len(), 6, "every fixture entry parsed");

        let installed = summon_entries(&shortcuts, "dropterm");
        let installed: Vec<String> = installed.iter().map(binding_display).collect();
        assert_eq!(
            installed,
            vec!["Super + Shift + E".to_string()],
            "the installed summon chord surfaces for its own stem"
        );

        let dev = summon_entries(&shortcuts, "dropterm-dev");
        let dev: Vec<String> = dev.iter().map(binding_display).collect();
        assert_eq!(dev, vec!["F12".to_string()], "the dev launcher owns F12");

        let foreign = summon_entries(&shortcuts, "assistant");
        assert_eq!(
            foreign.len(),
            0,
            "the foreign spawns are not toggles: recognition needs the verb, not just the stem"
        );
    }

    #[test]
    fn foreign_entries_survive_the_round_trip() {
        let parsed = parse_custom(FIXTURE).expect("the fixture parses");
        let text = serialize_custom(&parsed).expect("the map serializes");
        let reparsed = parse_custom(&text).expect("the round-trip parses");

        assert_eq!(reparsed.0.len(), parsed.0.len(), "no entry lost or gained");
        for (binding, value) in &parsed.0 {
            assert_eq!(
                reparsed.0.get(binding),
                Some(value),
                "entry {binding:?} survives the round-trip unchanged"
            );
        }
    }

    /// A store where built-in shortcuts were rebound beside the custom
    /// entries — the shape cosmic-settings' shared `custom` key holds
    /// on any machine with a rebound built-in (the defect GU4 fixes:
    /// the typed enum rejected these, so the store read `Unparseable`
    /// and a bound user was nagged at every cold boot).
    const BUILT_IN_FIXTURE: &str = r#"{
    (
        modifiers: [
            Super,
        ],
        key: "e",
        description: Some("dropterm"),
    ): Spawn("dropterm toggle"),
    (
        modifiers: [
            Super,
        ],
        key: "d",
    ): Close,
    (
        modifiers: [
            Super,
        ],
        key: "Left",
    ): Focus(Left),
    (
        modifiers: [
            Super,
            Ctrl,
        ],
        key: "k",
    ): Disable,
    (
        modifiers: [],
        key: "F12",
    ): System(Launcher),
}"#;

    #[test]
    fn built_in_variants_parse_beside_the_summon_entries() {
        let shortcuts = parse_custom(BUILT_IN_FIXTURE)
            .unwrap_or_else(|err| panic!("the rebound store parses as Loaded: {err}"));
        assert_eq!(
            shortcuts.0.len(),
            5,
            "every entry survives: spawn, two built-ins, disable, foreign"
        );
        let summon: Vec<String> = summon_entries(&shortcuts, "dropterm")
            .iter()
            .map(binding_display)
            .collect();
        assert_eq!(
            summon,
            vec!["Super + E".to_string()],
            "the summon entry is still recognized among the built-ins"
        );
    }

    #[test]
    fn action_values_carry_verbatim_through_a_round_trip() {
        let parsed = parse_custom(BUILT_IN_FIXTURE).expect("the rebound store parses");
        let text = serialize_custom(&parsed).expect("the map serializes");
        let reparsed = parse_custom(&text).expect("the round-trip parses");

        for (binding, value) in &parsed.0 {
            let carried = reparsed
                .0
                .get(binding)
                .unwrap_or_else(|| panic!("entry {binding:?} lost in the round-trip"));
            assert_eq!(
                carried.0.get_ron(),
                value.0.get_ron(),
                "entry {binding:?}'s action carried byte-for-byte"
            );
        }
    }

    #[test]
    fn spawn_command_probes_the_opaque_value() {
        let value = |action: &str| {
            let map = format!(r"{{ (modifiers: []): {action} }}");
            let parsed = parse_custom(&map).expect("the one-entry map parses");
            parsed.0.into_iter().next().expect("one entry").1
        };
        assert_eq!(
            value(r#"Spawn("dropterm toggle")"#)
                .spawn_command()
                .as_deref(),
            Some("dropterm toggle"),
            "the Spawn shape probes out"
        );
        assert_eq!(value("Close").spawn_command(), None, "a built-in");
        assert_eq!(value("Disable").spawn_command(), None, "a disable");
        assert_eq!(value("Focus(Left)").spawn_command(), None, "a built-in arg");
        assert_eq!(
            value("System(Launcher)").spawn_command(),
            None,
            "a foreign variant"
        );
    }

    #[test]
    fn an_unparseable_map_and_a_bad_keysym_never_parse() {
        assert!(parse_custom("not ron at all").is_err(), "corrupt text");
        let bad_key = r#"{ (modifiers: [], key: "NotARealKeyname"): Spawn("x") }"#;
        assert!(parse_custom(bad_key).is_err(), "an unresolvable keysym");
    }

    #[test]
    fn the_read_states_degrade_explicitly() {
        let missing = std::env::temp_dir().join(format!(
            "dropterm-global-shortcut-missing-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&missing);
        assert_eq!(read_custom(&missing), ReadCustom::Absent);

        let present = std::env::temp_dir().join(format!(
            "dropterm-global-shortcut-present-{}",
            std::process::id()
        ));
        std::fs::write(&present, FIXTURE).expect("the temp fixture writes");
        let loaded = read_custom(&present);
        assert!(
            matches!(loaded, ReadCustom::Loaded(_)),
            "the fixture file loads"
        );
        std::fs::write(&present, "not ron at all").expect("the garbage writes");
        assert_eq!(read_custom(&present), ReadCustom::Unparseable);
        let _ = std::fs::remove_file(&present);

        let unreadable = std::env::temp_dir().join(format!(
            "dropterm-global-shortcut-dir-{}",
            std::process::id()
        ));
        std::fs::create_dir(&unreadable).expect("the temp dir creates");
        assert_eq!(read_custom(&unreadable), ReadCustom::Unreadable);
        std::fs::remove_dir(&unreadable).expect("the temp dir cleans");
    }

    #[test]
    fn the_config_path_lands_under_the_settings_id() {
        let path = custom_shortcuts_path(Path::new("/home/tester"));
        let path = path.to_str().expect("a utf-8 path");
        assert!(
            path.ends_with("com.system76.CosmicSettings.Shortcuts/v1/custom"),
            "the watched file: {path}"
        );
    }

    #[test]
    fn toggle_spawn_recognition_owns_its_own_stem() {
        assert!(is_toggle_spawn("dropterm toggle", "dropterm"));
        assert!(is_toggle_spawn("/usr/bin/dropterm toggle", "dropterm"));
        assert!(is_toggle_spawn(
            "~/.local/bin/dropterm-dev toggle",
            "dropterm-dev"
        ));
        assert!(!is_toggle_spawn("dropterm toggle", "dropterm-dev"));
        assert!(!is_toggle_spawn("other-app toggle", "dropterm"));
        assert!(!is_toggle_spawn("dropterm show", "dropterm"));
        assert!(!is_toggle_spawn("dropterm", "dropterm"));
    }

    #[test]
    fn the_onboarding_trigger_matrix() {
        assert!(
            onboarding_should_show(false, false),
            "unbound and not retired: ask"
        );
        assert!(!onboarding_should_show(false, true), "retired: never again");
        assert!(!onboarding_should_show(true, false), "bound: never again");
        assert!(
            !onboarding_should_show(true, true),
            "bound and retired: never again"
        );
    }

    #[test]
    fn setup_options_keep_the_configure_arm_off_in_every_cell() {
        // The shipped constant: the "Configure for me" provider is the
        // writer module (SPEC-global-toggle-cosmic-writer), which does
        // not exist yet — so every cell of the matrix degrades to the
        // manual path alone.
        let loaded = ReadCustom::Loaded(parse_custom(FIXTURE).expect("the fixture parses"));
        let cells: [(&str, Environment, &ReadCustom); 5] = [
            ("COSMIC with a loaded store", Environment::Cosmic, &loaded),
            (
                "COSMIC with no store",
                Environment::Cosmic,
                &ReadCustom::Absent,
            ),
            (
                "COSMIC with an unreadable store",
                Environment::Cosmic,
                &ReadCustom::Unreadable,
            ),
            (
                "COSMIC with an unparseable store",
                Environment::Cosmic,
                &ReadCustom::Unparseable,
            ),
            ("off-COSMIC", Environment::Other, &loaded),
        ];
        for (cell, environment, store) in cells {
            assert!(
                !setup_options(environment, store).configure_for_me,
                "{cell}: the configure arm stays off until the writer's W3 turns it on"
            );
        }
    }

    #[test]
    fn every_walk_names_the_toggle_command() {
        for environment in [Environment::Cosmic, Environment::Other] {
            let walk = summon_walk(environment);
            assert!(
                walk.iter().any(|step| step.contains("`dropterm toggle`")),
                "{environment:?}'s walk names the command"
            );
        }
        assert!(
            summon_walk(Environment::Cosmic)
                .iter()
                .any(|step| step.contains("F12")),
            "the COSMIC walk carries the unshifted-key advice"
        );
    }

    // --- the writer: the captured chord's conversion (W1) ---

    /// The gpui key name a conversion pins, resolved to its store
    /// keysym name (what `serialize_custom` writes into the file).
    fn chord_key(modifiers: Modifiers, key: &str) -> String {
        match captured_chord(modifiers, key) {
            CapturedChord::Binding(binding) => {
                let key = binding.key.expect("a converted chord carries a key");
                xkb::keysym_get_name(key)
            }
            CapturedChord::BareModifier => "BareModifier".to_string(),
            CapturedChord::Unresolvable => "Unresolvable".to_string(),
        }
    }

    #[test]
    fn the_captured_chord_converts_the_pinned_vocabulary() {
        // Single ASCII letters resolve through the daemon's own
        // from_char route, lowercased like the entries cosmic-settings
        // itself writes (the GU1 fixture's `key: "e"` form).
        assert_eq!(chord_key(Modifiers::new(), "c"), "c");
        assert_eq!(chord_key(Modifiers::new().shift(), "C"), "c", "gpui's shifted form lands on the same keysym");
        // Digits and the printable punctuation the zoom rows use.
        assert_eq!(chord_key(Modifiers::new(), "1"), "1");
        assert_eq!(chord_key(Modifiers::new(), "="), "equal");
        assert_eq!(chord_key(Modifiers::new(), "+"), "plus");
        assert_eq!(chord_key(Modifiers::new(), "-"), "minus");
        // The named keys, in gpui's lowercase platform vocabulary.
        // PageUp/PageDown carry their historical X11 names ("Prior",
        // "Next") — `keysym_get_name`'s canonical forms, the exact
        // strings the store holds and the daemon's own serializer
        // writes.
        assert_eq!(chord_key(Modifiers::new(), "space"), "space");
        assert_eq!(chord_key(Modifiers::new(), "pageup"), "Prior");
        assert_eq!(chord_key(Modifiers::new(), "pagedown"), "Next");
        assert_eq!(chord_key(Modifiers::new(), "end"), "End");
        assert_eq!(chord_key(Modifiers::new(), "tab"), "Tab");
        assert_eq!(chord_key(Modifiers::new(), "escape"), "Escape");
        assert_eq!(chord_key(Modifiers::new(), "f12"), "F12");
    }

    #[test]
    fn a_modifier_press_keeps_the_capture_armed_and_a_bad_name_stays_out() {
        // The modifier names gpui delivers for a bare press: the
        // capture stays armed (the seat's contract).
        for name in ["shift", "control", "ctrl", "alt", "super", "meta", "logo"] {
            assert_eq!(
                captured_chord(Modifiers::new(), name),
                CapturedChord::BareModifier,
                "bare {name} keeps the capture armed"
            );
        }
        // Anything else that resolves to no keysym is refused — an
        // empty name and an unknown word both.
        assert_eq!(captured_chord(Modifiers::new(), ""), CapturedChord::Unresolvable);
        assert_eq!(
            captured_chord(Modifiers::new(), "not-a-real-key"),
            CapturedChord::Unresolvable
        );
    }

    #[test]
    fn environment_detection_reads_the_session_variable() {
        assert_eq!(detect_environment(Some("COSMIC")), Environment::Cosmic);
        assert_eq!(detect_environment(Some("cosmic")), Environment::Cosmic);
        assert_eq!(detect_environment(Some("COSMIC:foo")), Environment::Cosmic);
        assert_eq!(detect_environment(Some("GNOME")), Environment::Other);
        assert_eq!(detect_environment(None), Environment::Other);
    }
}
