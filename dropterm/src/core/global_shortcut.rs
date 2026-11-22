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

    /// Whether this value is the store's `Disable` — an entry the
    /// keyboard page's conflict rule does not count as a holder
    /// (`config_contains` filters it), so the marker may commit over
    /// one exactly as COSMIC's own replace would.
    pub fn is_disable(&self) -> bool {
        self.0.into_rust::<DisableAction>().is_ok()
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

/// The `Disable` shape [`CustomActionValue::is_disable`] probes an
/// opaque value against.
#[derive(Deserialize)]
enum DisableAction {
    Disable,
}

/// The effective shortcut map: the system defaults overridden by the
/// user's custom entries.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct EffectiveShortcuts(pub HashMap<Binding, CustomActionValue>);

/// The effective map over the two files — defaults extended by
/// custom, custom winning at equal bindings (the keyboard pages' own
/// `shortcuts_system_config` merge). Pure; the verdict's input.
pub fn effective_shortcuts(
    defaults: &CustomShortcuts,
    custom: &CustomShortcuts,
) -> EffectiveShortcuts {
    let mut effective = defaults.0.clone();
    effective.extend(
        custom
            .0
            .iter()
            .map(|(binding, value)| (binding.clone(), value.clone())),
    );
    EffectiveShortcuts(effective)
}

/// What committing a captured chord would do over the effective map —
/// the keyboard page's own conflict rule, refused instead of replaced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureVerdict {
    /// Free: the commit planner may add the marker here.
    Commit,
    /// The chord is the writer's own marker already: nothing writes.
    Own,
    /// Another effective entry — a foreign custom, a system default —
    /// holds the chord: nothing writes, and the holder is named (its
    /// description, else its chord, the replace dialog's own label
    /// rule).
    Conflict(String),
    /// Not a settable binding — a bare modifier (bare Super aside) or
    /// an unmodified key from COSMIC's forbidden set: nothing writes.
    Unsupported,
}

/// COSMIC's forbidden unmodified keysyms
/// (`is_forbidden_unmodified_keysym`, pinned keysym ranges): the
/// printable text keys, the tab/return family, and the lock and
/// group-switch keys a bare press must not steal — plus the national
/// layout ranges.
fn is_forbidden_unmodified_keysym(key: Keysym) -> bool {
    let raw = key.raw();
    matches!(raw,
        0x0041..=0x005a   // A-Z
        | 0x0061..=0x007a // a-z
        | 0x0030..=0x0039 // 0-9
        | 0x0020          // Space
        | 0xff09          // Tab
        | 0xfe20          // ISO_Left_Tab
        | 0xff89          // KP_Tab
        | 0xff0d          // Return
        | 0xff8d          // KP_Enter
        | 0xff7e          // Mode_switch
        | 0xff14          // Scroll_Lock
        | 0xff15          // Sys_Req
        | 0xff20          // Multi_key (Compose)
        | 0xff7f          // Num_Lock
        | 0xffe5          // Caps_Lock
        | 0xfe01          // ISO_Lock
        | 0xfe08 | 0xfe0a | 0xfe0c | 0xfe0e // ISO group switches
        | 0xfed0..=0xfed2 // First/Prev/Next_Virtual_Screen
        | 0xfed4          // Last_Virtual_Screen
        | 0xfed5          // Terminate_Server
        | 0xfe7a          // AudibleBell_Enable
        | 0x04a1..=0x04df // Kana
        | 0x05ac..=0x05f2 // Arabic
        | 0x06a1..=0x06ff // Cyrillic
        | 0x07a1..=0x07f9 // Greek
        | 0x0cdf..=0x0cfa // Hebrew
        | 0x0da1..=0x0df9 // Thai
        | 0x0ea1..=0x0efa // Hangul
    )
}

/// Whether this entry is the writer's own marker — the
/// [`MARKER_DESCRIPTION`]-described toggle spawn of `exe_stem`. A
/// hand-made own-stem entry beside it stays foreign (never touched;
/// Open Question 2's default).
pub fn is_marker(binding: &Binding, value: &CustomActionValue, exe_stem: &str) -> bool {
    binding.description.as_deref() == Some(MARKER_DESCRIPTION)
        && value
            .spawn_command()
            .is_some_and(|command| is_toggle_spawn(&command, exe_stem))
}

/// The capture verdict for `binding` over the effective map (system
/// defaults overridden by the user's custom entries) — COSMIC
/// Settings' own conflict rule, refused instead of replaced:
/// `Commit`; `Conflict(holder)` — nothing written, the inline error
/// names the holder; `Unsupported` — nothing written.
pub fn capture_verdict(
    binding: &Binding,
    effective: &EffectiveShortcuts,
    exe_stem: &str,
) -> CaptureVerdict {
    // The validity rule, mirroring `Binding::is_set`: a bare modifier
    // press is not a settable binding; bare Super is.
    let bare_super = binding.key.is_none()
        && binding.modifiers.logo
        && !binding.modifiers.shift
        && !binding.modifiers.alt
        && !binding.modifiers.ctrl;
    if binding.key.is_none() && !bare_super {
        return CaptureVerdict::Unsupported;
    }
    if let Some(key) = binding.key {
        let has_modifier = binding.modifiers.logo
            || binding.modifiers.shift
            || binding.modifiers.alt
            || binding.modifiers.ctrl;
        if !has_modifier && is_forbidden_unmodified_keysym(key) {
            return CaptureVerdict::Unsupported;
        }
    }
    // The conflict rule, mirroring `config_contains`: an effective
    // entry with a non-`Disable` action holds the chord; a `Disable`
    // is committable over, and the writer's own marker is `Own`. The
    // stored key carries the holder's identity (description and all)
    // — the captured chord is the query, not the label.
    match effective.0.get_key_value(binding) {
        Some((stored, value)) if is_marker(stored, value, exe_stem) => CaptureVerdict::Own,
        Some((stored, value)) if !value.is_disable() => CaptureVerdict::Conflict(
            stored
                .description
                .clone()
                .unwrap_or_else(|| binding_display(stored)),
        ),
        _ => CaptureVerdict::Commit,
    }
}

/// The marker action's serializable shape — the one-variant enum so
/// `ron` spells the variant's name exactly as the store's own
/// `Spawn("<command>")`, the command escaped by the serializer (a
/// newtype struct would lose the name and serialize as a bare
/// tuple).
#[derive(Serialize)]
enum MarkerAction<'a> {
    Spawn(&'a str),
}

/// The marker entry's Spawn command: the running binary's absolute
/// path plus the toggle verb (Open Question 1's default). cosmic-comp
/// runs it through `/bin/sh -c` — a whitespace-free first word needs
/// no quoting — and `is_toggle_spawn` recognizes exactly that shape.
pub fn marker_spawn_command(exe: &Path) -> String {
    format!("{} toggle", exe.display())
}

/// The binding shape the marker is written under: the committed
/// chord, the marker description, `keycode` unset — the shape
/// cosmic-settings writes for a new custom shortcut.
fn marker_binding(binding: &Binding) -> Binding {
    Binding {
        modifiers: binding.modifiers.clone(),
        key: binding.key,
        keycode: None,
        description: Some(MARKER_DESCRIPTION.to_string()),
    }
}

/// The commit planner: the custom map with exactly one marker entry
/// at `binding`. An existing marker moves (its old chord goes — the
/// update path, never a second marker), the chord's own slot takes
/// the marker (a `Disable` is replaced, the verdict's committable
/// case), and every foreign entry — hand-made own-stem entries
/// included — carries over untouched. Whole map in, whole map out;
/// the write seam re-parses the output before it touches disk.
pub fn commit_plan(
    custom: &CustomShortcuts,
    binding: &Binding,
    exe_stem: &str,
    command: &str,
) -> CustomShortcuts {
    let mut planned: HashMap<Binding, CustomActionValue> = custom
        .0
        .iter()
        .filter(|(existing, value)| !is_marker(existing, value, exe_stem))
        .map(|(existing, value)| (existing.clone(), value.clone()))
        .collect();
    // Total over the contract: serializing a plain string, then
    // re-parsing the serializer's own output — neither can fail here.
    let text =
        ron::to_string(&MarkerAction::Spawn(command)).expect("ron serializes a plain string");
    let action = RawValue::from_boxed_ron(text.into_boxed_str())
        .expect("the serializer's own output parses as a value");
    planned.insert(marker_binding(binding), CustomActionValue(action));
    CustomShortcuts(planned)
}

/// The revert planner: the custom map without every entry that is
/// this binary's marker — foreign entries and other stems' markers
/// carry over untouched. Whole map in, whole map out.
pub fn revert_plan(custom: &CustomShortcuts, exe_stem: &str) -> CustomShortcuts {
    CustomShortcuts(
        custom
            .0
            .iter()
            .filter(|(existing, value)| !is_marker(existing, value, exe_stem))
            .map(|(existing, value)| (existing.clone(), value.clone()))
            .collect(),
    )
}

// --- the write seam and the defaults loader ---

/// Why a write refused.
#[derive(Debug, PartialEq, Eq)]
pub enum WriteRefusal {
    /// The fresh `custom` read was unreadable or unparseable — the
    /// wipe hazard's gate: nothing is ever written over a store in
    /// that state (an Absent store is a fresh session, not a
    /// refusal).
    StoreNotLoaded,
    /// The planned output failed to re-parse with the same shapes:
    /// the file must parse under COSMIC's own reader, so drift
    /// refuses, never a guess.
    OutputUnparseable(String),
    /// The output re-parsed but did not equal the plan: refused.
    OutputMismatch,
}

/// What a write attempt yielded — the seat's branch on it.
#[derive(Debug, PartialEq)]
pub enum WriteOutcome {
    /// The store now holds the planned map (the file replaced
    /// atomically through the house write helper).
    Written(CustomShortcuts),
    /// Refused before touching the file.
    Refused(WriteRefusal),
    /// The file I/O failed after the gates passed (permissions, disk,
    /// ...). The store's previous file is intact — the helper writes
    /// a temp sibling and renames, so a failure never clobbers.
    Io(String),
}

/// The read-modify-write seam: a fresh read immediately before the
/// write, refusal on any store state that must not be written over,
/// the planned output re-parsed and re-verified before it touches
/// the file, and an atomic same-directory replace through the house
/// write helper (`config::store::save_atomic` — temp sibling, fsync,
/// rename, dir fsync; its pretty RON is the exact serialization
/// cosmic-config's own writer makes). No logic lives here — the
/// planner comes from the caller (`commit_plan` / `revert_plan`);
/// this function only gates and writes.
pub fn write_custom<F>(path: &Path, plan: F) -> WriteOutcome
where
    F: FnOnce(&CustomShortcuts) -> CustomShortcuts,
{
    // The fresh read immediately before the write.
    let store = match read_custom(path) {
        ReadCustom::Loaded(store) => store,
        ReadCustom::Absent => CustomShortcuts::default(),
        ReadCustom::Unreadable | ReadCustom::Unparseable => {
            return WriteOutcome::Refused(WriteRefusal::StoreNotLoaded);
        }
    };
    let planned = plan(&store);
    // The output gate: the same serialization the write will make
    // (`PrettyConfig::default()` is what `save_atomic` and
    // cosmic-config's own writer both run) must parse with these
    // shapes and equal the plan.
    let text = match ron::ser::to_string_pretty(&planned, ron::ser::PrettyConfig::default()) {
        Ok(text) => text,
        Err(err) => {
            return WriteOutcome::Refused(WriteRefusal::OutputUnparseable(err.to_string()));
        }
    };
    match parse_custom(&text) {
        Ok(reparsed) if reparsed == planned => {}
        Ok(_) => return WriteOutcome::Refused(WriteRefusal::OutputMismatch),
        Err(err) => {
            return WriteOutcome::Refused(WriteRefusal::OutputUnparseable(err));
        }
    }
    match crate::core::config::store::save_atomic(path, &planned) {
        Ok(()) => WriteOutcome::Written(planned),
        Err(err) => WriteOutcome::Io(err.to_string()),
    }
}

/// The `defaults` file's candidate paths, nearest first: the user's
/// config dir, then the XDG data dirs under the `cosmic` prefix —
/// `data_home` (an absolute `XDG_DATA_HOME`; anything else is
/// ignored per the base-directory spec) then every `data_dirs`
/// entry, falling back to the spec defaults when empty.
/// cosmic-config's own `Config::new` lookup, in its own order.
pub fn defaults_path_candidates(
    home: &Path,
    data_home: Option<&Path>,
    data_dirs: &[PathBuf],
) -> Vec<PathBuf> {
    let mut candidates =
        vec![home.join(".config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/defaults")];
    let mut data_roots: Vec<PathBuf> = data_home
        .map(|home| vec![home.to_path_buf()])
        .unwrap_or_default();
    if data_dirs.is_empty() {
        data_roots.push(PathBuf::from("/usr/local/share"));
        data_roots.push(PathBuf::from("/usr/share"));
    } else {
        data_roots.extend(data_dirs.iter().cloned());
    }
    for root in data_roots {
        candidates.push(root.join("cosmic/com.system76.CosmicSettings.Shortcuts/v1/defaults"));
    }
    candidates
}

/// The session's defaults candidates — [`defaults_path_candidates`]
/// over the environment (`HOME`, `XDG_DATA_HOME`, `XDG_DATA_DIRS`).
pub fn defaults_paths() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let data_dirs: Vec<PathBuf> = std::env::var_os("XDG_DATA_DIRS")
        .map(|dirs| {
            dirs.to_string_lossy()
                .split(':')
                .filter(|part| !part.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default();
    defaults_path_candidates(&home, data_home.as_deref(), &data_dirs)
}

/// The system defaults as they stand — the first candidate that
/// loads; an unreadable or unparseable candidate answers the empty
/// map, exactly the keyboard pages' own fallback (a broken defaults
/// file never blocks a session). Nothing here is ever written.
pub fn load_defaults(candidates: &[PathBuf]) -> CustomShortcuts {
    for path in candidates {
        match read_custom(path) {
            ReadCustom::Loaded(defaults) => return defaults,
            ReadCustom::Absent => {}
            ReadCustom::Unreadable | ReadCustom::Unparseable => {
                log::warn!(
                    "the COSMIC shortcuts defaults at {} do not parse; treating them as empty",
                    path.display()
                );
                return CustomShortcuts::default();
            }
        }
    }
    CustomShortcuts::default()
}

#[cfg(test)]
mod tests;
