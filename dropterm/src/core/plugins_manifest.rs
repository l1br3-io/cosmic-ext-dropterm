//! The plugin manifest contract (plugin-manifest spec): the dir
//! layout's scan, the `plugin.json` v1 vocabulary, the activation
//! resolution, and the `plugins-settings.json` store. Pure data and
//! validation — no gpui, no mlua — the slot-contract discipline: the
//! host (`lua_host`) consumes the activation set, the shell
//! (`app_shell`) the advertising rows, and the settings window the
//! declared rows, all from one module so the dual scans cannot drift.
//!
//! Every observable step is a log line in the house shape: the scan
//! returns the lines' bodies (the caller prefixes the
//! `dropterm: plugins:` namespace) so tests pin the exact text and
//! the owner reviews the shapes (checkpoint PM-B). Closed
//! vocabularies fail-to-default — one bad row costs its row, one bad
//! manifest costs its plugin, never a sibling.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::core::keybinds::{MAX_COMMAND_NAME_LEN, valid_command_name};

/// The script file every plugin dir carries (`plugins/<id>/plugin.lua`).
pub const SCRIPT_FILE_NAME: &str = "plugin.lua";

/// The optional manifest file (`plugins/<id>/plugin.json`); its
/// absence is today's eager behavior plus a nudge line.
pub const MANIFEST_FILE_NAME: &str = "plugin.json";

/// The settings store's file name, under the config dir — the same
/// dir `plugins/` lives in. User config: the settings window is its
/// only writer, scripts never touch it.
pub const STORE_FILE_NAME: &str = "plugins-settings.json";

/// One command a manifest declares, before any interpreter exists:
/// the slug (the same charset the runtime port enforces, gated by
/// [`valid_command_name`]) and the description shown in rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredCommand {
    pub id: String,
    pub description: String,
}

impl DeclaredCommand {
    /// The derived activation event: declared commands implicitly
    /// carry `on_command:<id>` (VS Code's rule) — a lazy plugin loads
    /// on its first declared-command dispatch.
    pub fn activation_event(&self) -> String {
        format!("on_command:{}", self.id)
    }
}

/// The six declared setting types (the full VS Code set), the row's
/// default riding its variant so a kind and its default cannot
/// disagree. A manifest default of the wrong type for its row costs
/// one log line and falls to the builtin default.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingKind {
    Boolean {
        default: bool,
    },
    String {
        default: String,
    },
    Number {
        default: f64,
    },
    Enum {
        values: Vec<String>,
        default: String,
    },
    Array {
        default: Vec<serde_json::Value>,
    },
    Object {
        default: serde_json::Map<String, serde_json::Value>,
    },
}

impl SettingKind {
    /// The builtin default for this kind — the fail-to-default floor
    /// (`false`, `""`, `0`, the first enum value, `[]`, `{}`).
    fn builtin(&self) -> Self {
        match self {
            SettingKind::Boolean { .. } => SettingKind::Boolean { default: false },
            SettingKind::String { .. } => SettingKind::String {
                default: String::new(),
            },
            SettingKind::Number { .. } => SettingKind::Number { default: 0.0 },
            SettingKind::Enum { values, .. } => SettingKind::Enum {
                values: values.clone(),
                default: values.first().cloned().unwrap_or_default(),
            },
            SettingKind::Array { .. } => SettingKind::Array {
                default: Vec::new(),
            },
            SettingKind::Object { .. } => SettingKind::Object {
                default: serde_json::Map::new(),
            },
        }
    }
}

/// One settings row a manifest declares: the key (unique per plugin,
/// the store's and `ctx.settings`' field name), the display label,
/// the typed kind with its default, and the optional description.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredSetting {
    pub key: String,
    pub label: String,
    pub kind: SettingKind,
    pub description: Option<String>,
}

/// How a plugin's script joins the interpreter (SPEC-plugin-manifest's
/// ruling): derived `on_command:<id>` only — load on first dispatch;
/// `on_startup` — load at host start; `on_startup_finished` — defer
/// past the 50 ms first-painted-frame window. A dir without a
/// manifest resolves to [`Activation::AtStartup`] (today's eager
/// behavior); a manifest resolving both explicit tokens takes the
/// strongest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Activation {
    #[default]
    Lazy,
    AtStartup,
    AfterStartup,
}

/// One scanned plugin: the dir name is the persistent key (log lines,
/// settings grouping, later modules' storage keys); the manifest
/// `"name"` overrides display only. `has_manifest` distinguishes the
/// nudge case; `has_script` lets the host skip an empty dir without
/// a confusing load line.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedPlugin {
    pub id: String,
    pub display_name: Option<String>,
    pub has_script: bool,
    pub has_manifest: bool,
    pub activation: Activation,
    pub commands: Vec<DeclaredCommand>,
    pub settings: Vec<DeclaredSetting>,
}

/// One scan's result: the registry in dir-name order and the log-line
/// bodies, in emission order. Pure — the caller emits.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PluginScan {
    pub plugins: Vec<ScannedPlugin>,
    pub logs: Vec<String>,
}

/// The manifest-derived half of one plugin, [`parse_manifest`]'s
/// output.
#[derive(Debug, Clone, PartialEq)]
struct ManifestData {
    display_name: Option<String>,
    commands: Vec<DeclaredCommand>,
    settings: Vec<DeclaredSetting>,
    activation: Activation,
}

/// Walk `dir` one level: loose `.lua` files and non-slug dirs collect
/// a loud-skip line each (the ruled hard break), dirs parse
/// `plugin.json`, deeper nesting is ignored, and non-dir non-lua
/// entries stay silent (today's behavior for a stray README). A
/// missing dir is the silent first run; another read error is one
/// line and an empty scan. Order is dir-name order, the deterministic
/// registration order today's loader also sorts by.
///
/// # Errors
///
/// None panic: every failure is a log line in the result, never a
/// raise.
pub fn scan(dir: &Path) -> PluginScan {
    let mut names: Vec<(String, PathBuf, bool)> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_name()?.to_string_lossy().into_owned();
                Some((name, path.clone(), path.is_dir()))
            })
            .collect(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return PluginScan::default(),
        Err(err) => {
            return PluginScan {
                plugins: Vec::new(),
                logs: vec![format!("cannot read {}: {err}", dir.display())],
            };
        }
    };
    names.sort_by(|a, b| a.0.cmp(&b.0));

    let mut scan = PluginScan::default();
    for (name, path, is_dir) in names {
        if !is_dir {
            if path.extension().is_some_and(|ext| ext == "lua") {
                scan.logs.push(format!(
                    "skipped loose {name}: plugins are plugin dirs now \
                     (plugins/<id>/{SCRIPT_FILE_NAME}); see docs/plugins.md"
                ));
            }
            continue;
        }
        if !valid_command_name(&name) {
            scan.logs.push(format!(
                "skipped {name}: not a plugin id (lowercase letters, digits, hyphens, \
                 up to {MAX_COMMAND_NAME_LEN} characters); see docs/plugins.md"
            ));
            continue;
        }

        let has_script = path.join(SCRIPT_FILE_NAME).is_file();
        let bytes = match fs::read_to_string(path.join(MANIFEST_FILE_NAME)) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                scan.logs.push(format!(
                    "{name}: no plugin.json manifest; loading eager (see docs/plugins.md)"
                ));
                scan.plugins.push(ScannedPlugin {
                    id: name,
                    display_name: None,
                    has_script,
                    has_manifest: false,
                    activation: Activation::AtStartup,
                    commands: Vec::new(),
                    settings: Vec::new(),
                });
                continue;
            }
            Err(err) => {
                scan.logs
                    .push(format!("skipped {name}: plugin.json unreadable: {err}"));
                continue;
            }
        };

        let (data, logs) = parse_manifest(&name, &bytes);
        scan.logs.extend(logs);
        if let Some(data) = data {
            scan.plugins.push(ScannedPlugin {
                id: name,
                display_name: data.display_name,
                has_script,
                has_manifest: true,
                activation: data.activation,
                commands: data.commands,
                settings: data.settings,
            });
        }
    }
    scan
}

/// The v1 manifest's serde shape — kebab-case keys, closed
/// vocabulary: unknown keys flatten into a map the parse logs and
/// ignores; a structurally wrong manifest (a non-array `commands`, a
/// non-string `name`) fails the parse and skips the plugin.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawManifest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    commands: Vec<RawCommand>,
    #[serde(default)]
    settings: Vec<RawSetting>,
    #[serde(default)]
    activation: Vec<String>,
    #[serde(flatten)]
    unknown: BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawCommand {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(flatten)]
    unknown: BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawSetting {
    #[serde(default)]
    key: Option<String>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    default: Option<serde_json::Value>,
    #[serde(default, rename = "enum")]
    enum_values: Option<Vec<String>>,
    #[serde(default)]
    description: Option<String>,
    #[serde(flatten)]
    unknown: BTreeMap<String, serde_json::Value>,
}

/// Parse one manifest's bytes: the plugin id names the log lines.
/// Returns the manifest data — or `None` when the manifest is
/// unusable (unreadable shape), always with the exact log bodies.
fn parse_manifest(id: &str, bytes: &str) -> (Option<ManifestData>, Vec<String>) {
    let raw: RawManifest = match serde_json::from_str(bytes) {
        Ok(raw) => raw,
        Err(err) => {
            return (None, vec![format!("skipped {id}: plugin.json: {err}")]);
        }
    };

    let mut logs = Vec::new();
    for key in raw.unknown.keys() {
        logs.push(format!("{id}: ignoring unknown manifest key '{key}'"));
    }

    let mut commands = Vec::new();
    let mut command_ids: Vec<String> = Vec::new();
    for raw_command in raw.commands {
        for key in raw_command.unknown.keys() {
            logs.push(format!("{id}: ignoring unknown command key '{key}'"));
        }
        let Some(command_id) = raw_command.id else {
            logs.push(format!(
                "{id}: a manifest command is missing its 'id'; skipping the command"
            ));
            continue;
        };
        if !valid_command_name(&command_id) {
            logs.push(format!(
                "{id}: manifest command '{command_id}' would not register: names are \
                 lowercase slugs (letters, digits, hyphens) up to \
                 {MAX_COMMAND_NAME_LEN} characters"
            ));
            continue;
        }
        let Some(description) = raw_command.description else {
            logs.push(format!(
                "{id}: manifest command '{command_id}' is missing its 'description'; \
                 skipping the command"
            ));
            continue;
        };
        if command_ids.contains(&command_id) {
            logs.push(format!(
                "{id}: manifest command '{command_id}' declared twice; keeping the first"
            ));
            continue;
        }
        command_ids.push(command_id.clone());
        commands.push(DeclaredCommand {
            id: command_id,
            description,
        });
    }

    let mut settings = Vec::new();
    let mut setting_keys: Vec<String> = Vec::new();
    for raw_setting in raw.settings {
        for key in raw_setting.unknown.keys() {
            logs.push(format!("{id}: ignoring unknown setting key '{key}'"));
        }
        let Some(key) = raw_setting.key else {
            logs.push(format!(
                "{id}: a settings row is missing its 'key'; skipping the row"
            ));
            continue;
        };
        if setting_keys.contains(&key) {
            logs.push(format!(
                "{id}: setting '{key}' declared twice; keeping the first"
            ));
            continue;
        }
        let Some(kind_word) = raw_setting.kind else {
            logs.push(format!(
                "{id}: setting '{key}' is missing its 'type'; skipping the row"
            ));
            continue;
        };
        let Some(label) = raw_setting.label else {
            logs.push(format!(
                "{id}: setting '{key}' is missing its 'label'; skipping the row"
            ));
            continue;
        };
        let Some(kind) = build_setting_kind(&kind_word, raw_setting.enum_values.as_deref()) else {
            if kind_word == "enum" {
                logs.push(format!(
                    "{id}: setting '{key}' needs a non-empty 'enum' list; \
                     skipping the row"
                ));
            } else {
                logs.push(format!(
                    "{id}: setting '{key}' has an unknown type '{kind_word}'; \
                     skipping the row"
                ));
            }
            continue;
        };
        let kind = match &raw_setting.default {
            None => kind.builtin(),
            Some(value) if default_matches(&kind, value) => with_default(&kind, value),
            Some(_) => {
                logs.push(format!(
                    "{id}: setting '{key}' default is the wrong type; \
                     using the builtin default"
                ));
                kind.builtin()
            }
        };
        setting_keys.push(key.clone());
        settings.push(DeclaredSetting {
            key,
            label,
            kind,
            description: raw_setting.description,
        });
    }

    let mut activation = Activation::Lazy;
    for token in raw.activation {
        match token.as_str() {
            "on_startup" => activation = Activation::AtStartup,
            "on_startup_finished" => {
                if activation == Activation::Lazy {
                    activation = Activation::AfterStartup;
                }
            }
            other => logs.push(format!("{id}: ignoring unknown activation '{other}'")),
        }
    }

    (
        Some(ManifestData {
            display_name: raw.name,
            commands,
            settings,
            activation,
        }),
        logs,
    )
}

/// The six-type vocabulary's parse: `None` is an unknown type word or
/// an enum row without a usable value list.
fn build_setting_kind(kind_word: &str, enum_values: Option<&[String]>) -> Option<SettingKind> {
    Some(match kind_word {
        "boolean" => SettingKind::Boolean { default: false },
        "string" => SettingKind::String {
            default: String::new(),
        },
        "number" => SettingKind::Number { default: 0.0 },
        "enum" => SettingKind::Enum {
            values: enum_values?.to_vec(),
            default: enum_values?.first().cloned()?,
        },
        "array" => SettingKind::Array {
            default: Vec::new(),
        },
        "object" => SettingKind::Object {
            default: serde_json::Map::new(),
        },
        _ => return None,
    })
}

/// Whether `value` is a valid default for `kind` — the enum row's
/// default must also be one of its declared values.
fn default_matches(kind: &SettingKind, value: &serde_json::Value) -> bool {
    match kind {
        SettingKind::Boolean { .. } => value.is_boolean(),
        SettingKind::String { .. } => value.is_string(),
        SettingKind::Number { .. } => value.is_number(),
        SettingKind::Enum { values, .. } => value
            .as_str()
            .is_some_and(|text| values.iter().any(|v| v == text)),
        SettingKind::Array { .. } => value.is_array(),
        SettingKind::Object { .. } => value.is_object(),
    }
}

/// A matched default promoted into the kind, keeping the declared
/// vocabulary (`values`) on enum rows.
fn with_default(kind: &SettingKind, value: &serde_json::Value) -> SettingKind {
    match kind {
        SettingKind::Boolean { .. } => SettingKind::Boolean {
            default: value.as_bool().unwrap_or_default(),
        },
        SettingKind::String { .. } => SettingKind::String {
            default: value.as_str().unwrap_or_default().to_owned(),
        },
        SettingKind::Number { .. } => SettingKind::Number {
            default: value.as_f64().unwrap_or_default(),
        },
        SettingKind::Enum { values, .. } => SettingKind::Enum {
            values: values.clone(),
            default: value.as_str().unwrap_or_default().to_owned(),
        },
        SettingKind::Array { .. } => SettingKind::Array {
            default: value.as_array().cloned().unwrap_or_default(),
        },
        SettingKind::Object { .. } => SettingKind::Object {
            default: value.as_object().cloned().unwrap_or_default(),
        },
    }
}

/// The settings store's typed shape: `{ "<plugin-id>": { "<key>":
/// <typed value> } }`. Row-type validation happens where the rows are
/// known (the merge, the editors); the store is the transport.
pub type StoreValues = BTreeMap<String, BTreeMap<String, serde_json::Value>>;

/// Read the store file, tolerantly: a missing file is the empty store
/// (the first run), a malformed file or a wrong-shaped entry is one
/// log line and the rest of the store still loads.
pub fn read_store(path: &Path) -> (StoreValues, Vec<String>) {
    let bytes = match fs::read_to_string(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return (StoreValues::new(), Vec::new());
        }
        Err(err) => {
            return (
                StoreValues::new(),
                vec![format!(
                    "{STORE_FILE_NAME} unreadable: {err}; starting empty"
                )],
            );
        }
    };
    let root: serde_json::Value = match serde_json::from_str(&bytes) {
        Ok(root) => root,
        Err(err) => {
            return (
                StoreValues::new(),
                vec![format!(
                    "{STORE_FILE_NAME} is not valid JSON: {err}; starting empty"
                )],
            );
        }
    };
    let Some(root) = root.as_object() else {
        return (
            StoreValues::new(),
            vec![format!(
                "{STORE_FILE_NAME} is not an object; starting empty"
            )],
        );
    };
    let mut values = StoreValues::new();
    let mut logs = Vec::new();
    for (plugin_id, entry) in root {
        let Some(rows) = entry.as_object() else {
            logs.push(format!(
                "{STORE_FILE_NAME}: dropping '{plugin_id}' entries (expected an object)"
            ));
            continue;
        };
        values.insert(plugin_id.clone(), rows.clone().into_iter().collect());
    }
    (values, logs)
}

/// Write the store atomically: serialize, write the sibling temp
/// file, rename over the target — a torn write never lands.
///
/// # Errors
///
/// The serialization or the filesystem's write/rename, verbatim.
pub fn write_store(path: &Path, values: &StoreValues) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(values)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(STORE_FILE_NAME);
    let temp = path.with_file_name(format!("{file_name}.tmp"));
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A per-test scratch directory under the system temp dir with a
    /// process- and test-unique name, removed (best-effort) on drop —
    /// the slots/theme-files tests' own shape.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let unique = format!(
                "dropterm-plugins-manifest-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir_all(&path).expect("create scratch dir");
            Self(path)
        }

        fn plugin(&self, id: &str) -> PathBuf {
            let dir = self.0.join(id);
            fs::create_dir_all(&dir).expect("create plugin dir");
            dir
        }

        fn write(&self, relative: &str, contents: &str) {
            let path = self.0.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent dir");
            }
            fs::write(path, contents).expect("write fixture file");
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    /// The spec's example manifest, extended with one row of every
    /// remaining type — the full-vocabulary parse pin.
    const FULL_MANIFEST: &str = r#"{
        "name": "Uptime helpers",
        "commands": [
            { "id": "paste-uptime", "description": "Paste system uptime into the pane" }
        ],
        "settings": [
            { "key": "uptime-format", "type": "enum", "label": "Format",
              "default": "pretty", "enum": ["pretty", "seconds"],
              "description": "How the uptime renders" },
            { "key": "prefix", "type": "string", "label": "Prefix", "default": "" },
            { "key": "enabled", "type": "boolean", "label": "Enabled", "default": true },
            { "key": "interval", "type": "number", "label": "Interval", "default": 2.5 },
            { "key": "tags", "type": "array", "label": "Tags", "default": ["a", 1] },
            { "key": "layout", "type": "object", "label": "Layout",
              "default": { "side": "left" } }
        ],
        "activation": ["on_startup"]
    }"#;

    #[test]
    fn empty_dir_scans_empty_silently() {
        let dir = TempDir::new("empty");
        let scan = scan(&dir.0);
        assert!(scan.plugins.is_empty());
        assert!(scan.logs.is_empty());
    }

    #[test]
    fn stray_non_lua_files_stay_silent() {
        let dir = TempDir::new("stray");
        dir.write("README.md", "notes");
        dir.write("notes.txt", "notes");
        let scan = scan(&dir.0);
        assert!(scan.plugins.is_empty());
        assert!(scan.logs.is_empty());
    }

    #[test]
    fn loose_lua_skipped_with_one_line() {
        let dir = TempDir::new("loose");
        dir.write("loose.lua", "print('hi')");
        let scan = scan(&dir.0);
        assert!(scan.plugins.is_empty());
        assert_eq!(
            scan.logs,
            vec![
                "skipped loose loose.lua: plugins are plugin dirs now \
                 (plugins/<id>/plugin.lua); see docs/plugins.md"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn non_slug_dir_skipped_with_one_line() {
        let dir = TempDir::new("non-slug");
        dir.plugin("Uptime");
        dir.plugin("up_time");
        let scan = scan(&dir.0);
        assert!(scan.plugins.is_empty());
        assert_eq!(
            scan.logs,
            vec![
                "skipped Uptime: not a plugin id (lowercase letters, digits, hyphens, \
                 up to 64 characters); see docs/plugins.md"
                    .to_owned(),
                "skipped up_time: not a plugin id (lowercase letters, digits, hyphens, \
                 up to 64 characters); see docs/plugins.md"
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn manifest_less_dir_is_eager_with_nudge() {
        let dir = TempDir::new("eager");
        let plugin = dir.plugin("foo");
        fs::write(plugin.join(SCRIPT_FILE_NAME), "print('hi')").expect("write script");
        let scan = scan(&dir.0);
        assert_eq!(scan.logs.len(), 1);
        assert!(
            scan.logs[0].contains("docs/plugins.md"),
            "the nudge names the docs anchor"
        );
        assert_eq!(scan.plugins.len(), 1);
        let plugin = &scan.plugins[0];
        assert_eq!(plugin.id, "foo");
        assert!(!plugin.has_manifest);
        assert!(plugin.has_script);
        assert_eq!(plugin.activation, Activation::AtStartup);
        assert!(plugin.commands.is_empty());
        assert!(plugin.settings.is_empty());
    }

    #[test]
    fn malformed_manifest_skips_plugin_with_one_line() {
        let dir = TempDir::new("malformed");
        let plugin = dir.plugin("bad");
        fs::write(plugin.join(MANIFEST_FILE_NAME), "{ not json").expect("write manifest");
        let scan = scan(&dir.0);
        assert!(scan.plugins.is_empty());
        assert_eq!(scan.logs.len(), 1);
        assert!(
            scan.logs[0].starts_with("skipped bad: plugin.json: "),
            "the skip names the plugin and the manifest: {:?}",
            scan.logs[0]
        );
    }

    #[test]
    fn unreadable_manifest_file_skips_with_one_line() {
        let dir = TempDir::new("unreadable");
        let plugin = dir.plugin("weird");
        fs::create_dir_all(plugin.join(MANIFEST_FILE_NAME)).expect("manifest as a dir");
        let scan = scan(&dir.0);
        assert!(scan.plugins.is_empty());
        assert_eq!(scan.logs.len(), 1);
        assert!(
            scan.logs[0].starts_with("skipped weird: plugin.json unreadable: "),
            "{:?}",
            scan.logs[0]
        );
    }

    #[test]
    fn full_vocabulary_manifest_parses_exactly() {
        let dir = TempDir::new("full");
        let plugin = dir.plugin("uptime-helpers");
        fs::write(plugin.join(MANIFEST_FILE_NAME), FULL_MANIFEST).expect("write manifest");
        let scan = scan(&dir.0);
        assert!(scan.logs.is_empty(), "{:?}", scan.logs);
        assert_eq!(scan.plugins.len(), 1);
        let plugin = &scan.plugins[0];
        assert_eq!(plugin.id, "uptime-helpers");
        assert_eq!(plugin.display_name.as_deref(), Some("Uptime helpers"));
        assert!(plugin.has_manifest);
        assert!(!plugin.has_script);
        assert_eq!(plugin.activation, Activation::AtStartup);
        assert_eq!(
            plugin.commands,
            vec![DeclaredCommand {
                id: "paste-uptime".to_owned(),
                description: "Paste system uptime into the pane".to_owned(),
            }]
        );
        assert_eq!(plugin.settings.len(), 6);
        assert_eq!(
            plugin.settings[0].kind,
            SettingKind::Enum {
                values: vec!["pretty".to_owned(), "seconds".to_owned()],
                default: "pretty".to_owned(),
            }
        );
        assert_eq!(
            plugin.settings[1].kind,
            SettingKind::String {
                default: String::new()
            }
        );
        assert_eq!(
            plugin.settings[2].kind,
            SettingKind::Boolean { default: true }
        );
        assert_eq!(
            plugin.settings[3].kind,
            SettingKind::Number { default: 2.5 }
        );
        assert_eq!(
            plugin.settings[4].kind,
            SettingKind::Array {
                default: vec![json!("a"), json!(1)]
            }
        );
        let mut layout = serde_json::Map::new();
        layout.insert("side".to_owned(), json!("left"));
        assert_eq!(
            plugin.settings[5].kind,
            SettingKind::Object { default: layout }
        );
    }

    #[test]
    fn derived_activation_event_carries_the_slug() {
        let command = DeclaredCommand {
            id: "paste-uptime".to_owned(),
            description: String::new(),
        };
        assert_eq!(command.activation_event(), "on_command:paste-uptime");
    }

    #[test]
    fn unknown_manifest_keys_ignored_with_one_line_each() {
        let dir = TempDir::new("unknown-keys");
        let plugin = dir.plugin("p");
        fs::write(
            plugin.join(MANIFEST_FILE_NAME),
            r#"{
                "name": "P",
                "on-startup-note": true,
                "commands": [
                    { "id": "go", "description": "d", "extra": 1 }
                ],
                "settings": [
                    { "key": "k", "type": "string", "label": "L", "widget": {} }
                ]
            }"#,
        )
        .expect("write manifest");
        let scan = scan(&dir.0);
        assert_eq!(scan.plugins.len(), 1);
        assert_eq!(
            scan.logs,
            vec![
                "p: ignoring unknown manifest key 'on-startup-note'".to_owned(),
                "p: ignoring unknown command key 'extra'".to_owned(),
                "p: ignoring unknown setting key 'widget'".to_owned(),
            ]
        );
    }

    #[test]
    fn duplicate_command_ids_keep_the_first_with_one_line() {
        let dir = TempDir::new("dup-command");
        let plugin = dir.plugin("p");
        fs::write(
            plugin.join(MANIFEST_FILE_NAME),
            r#"{
                "commands": [
                    { "id": "go", "description": "first" },
                    { "id": "go", "description": "second" }
                ]
            }"#,
        )
        .expect("write manifest");
        let scan = scan(&dir.0);
        assert_eq!(
            scan.plugins[0].commands,
            vec![DeclaredCommand {
                id: "go".to_owned(),
                description: "first".to_owned(),
            }]
        );
        assert_eq!(
            scan.logs,
            vec!["p: manifest command 'go' declared twice; keeping the first".to_owned()]
        );
    }

    #[test]
    fn duplicate_setting_keys_keep_the_first_with_one_line() {
        let dir = TempDir::new("dup-setting");
        let plugin = dir.plugin("p");
        fs::write(
            plugin.join(MANIFEST_FILE_NAME),
            r#"{
                "settings": [
                    { "key": "k", "type": "string", "label": "L" },
                    { "key": "k", "type": "boolean", "label": "M" }
                ]
            }"#,
        )
        .expect("write manifest");
        let scan = scan(&dir.0);
        assert_eq!(scan.plugins[0].settings.len(), 1);
        assert_eq!(
            scan.logs,
            vec!["p: setting 'k' declared twice; keeping the first".to_owned()]
        );
    }

    #[test]
    fn invalid_command_ids_reject_the_entry_with_one_line() {
        let dir = TempDir::new("bad-command");
        let plugin = dir.plugin("p");
        fs::write(
            plugin.join(MANIFEST_FILE_NAME),
            r#"{ "commands": [ { "id": "Bad Id", "description": "d" } ] }"#,
        )
        .expect("write manifest");
        let scan = scan(&dir.0);
        assert!(scan.plugins[0].commands.is_empty());
        assert_eq!(
            scan.logs,
            vec![
                "p: manifest command 'Bad Id' would not register: names are lowercase \
                 slugs (letters, digits, hyphens) up to 64 characters"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn commands_missing_required_fields_reject_the_entry_with_one_line() {
        let dir = TempDir::new("missing-command");
        let plugin = dir.plugin("p");
        fs::write(
            plugin.join(MANIFEST_FILE_NAME),
            r#"{
                "commands": [
                    { "description": "no id" },
                    { "id": "ok" }
                ]
            }"#,
        )
        .expect("write manifest");
        let scan = scan(&dir.0);
        assert!(scan.plugins[0].commands.is_empty());
        assert_eq!(
            scan.logs,
            vec![
                "p: a manifest command is missing its 'id'; skipping the command".to_owned(),
                "p: manifest command 'ok' is missing its 'description'; skipping the command"
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn settings_rows_missing_required_fields_reject_the_row_with_one_line() {
        let dir = TempDir::new("missing-setting");
        let plugin = dir.plugin("p");
        fs::write(
            plugin.join(MANIFEST_FILE_NAME),
            r#"{
                "settings": [
                    { "type": "string", "label": "no key" },
                    { "key": "k", "label": "no type" },
                    { "key": "k2", "type": "string" },
                    { "key": "k3", "type": "widget", "label": "L" }
                ]
            }"#,
        )
        .expect("write manifest");
        let scan = scan(&dir.0);
        assert!(scan.plugins[0].settings.is_empty());
        assert_eq!(
            scan.logs,
            vec![
                "p: a settings row is missing its 'key'; skipping the row".to_owned(),
                "p: setting 'k' is missing its 'type'; skipping the row".to_owned(),
                "p: setting 'k2' is missing its 'label'; skipping the row".to_owned(),
                "p: setting 'k3' has an unknown type 'widget'; skipping the row".to_owned(),
            ]
        );
    }

    #[test]
    fn enum_row_without_values_is_skipped_with_one_line() {
        let dir = TempDir::new("enum-empty");
        let plugin = dir.plugin("p");
        fs::write(
            plugin.join(MANIFEST_FILE_NAME),
            r#"{
                "settings": [
                    { "key": "e", "type": "enum", "label": "E" },
                    { "key": "e2", "type": "enum", "label": "E", "enum": [] }
                ]
            }"#,
        )
        .expect("write manifest");
        let scan = scan(&dir.0);
        assert!(scan.plugins[0].settings.is_empty());
        assert_eq!(
            scan.logs,
            vec![
                "p: setting 'e' needs a non-empty 'enum' list; skipping the row".to_owned(),
                "p: setting 'e2' needs a non-empty 'enum' list; skipping the row".to_owned(),
            ]
        );
    }

    #[test]
    fn wrong_typed_defaults_fall_to_the_builtin_with_one_line() {
        let dir = TempDir::new("bad-default");
        let plugin = dir.plugin("p");
        fs::write(
            plugin.join(MANIFEST_FILE_NAME),
            r#"{
                "settings": [
                    { "key": "b", "type": "boolean", "label": "B", "default": "yes" },
                    { "key": "s", "type": "string", "label": "S", "default": 5 },
                    { "key": "n", "type": "number", "label": "N", "default": "3" },
                    { "key": "e", "type": "enum", "label": "E", "default": "bogus",
                      "enum": ["one", "two"] },
                    { "key": "a", "type": "array", "label": "A", "default": {} },
                    { "key": "o", "type": "object", "label": "O", "default": [] }
                ]
            }"#,
        )
        .expect("write manifest");
        let scan = scan(&dir.0);
        let settings = &scan.plugins[0].settings;
        assert_eq!(settings[0].kind, SettingKind::Boolean { default: false });
        assert_eq!(
            settings[1].kind,
            SettingKind::String {
                default: String::new()
            }
        );
        assert_eq!(settings[2].kind, SettingKind::Number { default: 0.0 });
        assert_eq!(
            settings[3].kind,
            SettingKind::Enum {
                values: vec!["one".to_owned(), "two".to_owned()],
                default: "one".to_owned(),
            }
        );
        assert_eq!(
            settings[4].kind,
            SettingKind::Array {
                default: Vec::new()
            }
        );
        assert_eq!(
            settings[5].kind,
            SettingKind::Object {
                default: serde_json::Map::new()
            }
        );
        assert_eq!(scan.logs.len(), 6, "{:?}", scan.logs);
        assert!(
            scan.logs
                .iter()
                .all(|line| line.ends_with("default is the wrong type; using the builtin default")),
            "{:?}",
            scan.logs
        );
    }

    #[test]
    fn missing_defaults_use_the_builtin_silently() {
        let dir = TempDir::new("no-default");
        let plugin = dir.plugin("p");
        fs::write(
            plugin.join(MANIFEST_FILE_NAME),
            r#"{
                "settings": [
                    { "key": "b", "type": "boolean", "label": "B" },
                    { "key": "s", "type": "string", "label": "S" },
                    { "key": "e", "type": "enum", "label": "E", "enum": ["x"] }
                ]
            }"#,
        )
        .expect("write manifest");
        let scan = scan(&dir.0);
        let settings = &scan.plugins[0].settings;
        assert_eq!(settings[0].kind, SettingKind::Boolean { default: false });
        assert_eq!(
            settings[1].kind,
            SettingKind::String {
                default: String::new()
            }
        );
        assert_eq!(
            settings[2].kind,
            SettingKind::Enum {
                values: vec!["x".to_owned()],
                default: "x".to_owned(),
            }
        );
        assert!(scan.logs.is_empty(), "{:?}", scan.logs);
    }

    #[test]
    fn activation_tokens_resolve_with_the_strongest_winning() {
        let manifest = |activation: &str| {
            format!(
                r#"{{ "commands": [{{ "id": "go", "description": "d" }}], "activation": {activation} }}"#
            )
        };
        let resolve = |activation: &str| {
            let dir = TempDir::new("activation");
            let plugin = dir.plugin("p");
            fs::write(plugin.join(MANIFEST_FILE_NAME), manifest(activation))
                .expect("write manifest");
            scan(&dir.0)
        };
        assert_eq!(
            resolve(r#"["on_startup"]"#).plugins[0].activation,
            Activation::AtStartup
        );
        assert_eq!(
            resolve(r#"["on_startup_finished"]"#).plugins[0].activation,
            Activation::AfterStartup
        );
        assert_eq!(
            resolve(r#"["on_startup", "on_startup_finished"]"#).plugins[0].activation,
            Activation::AtStartup
        );
        assert_eq!(
            resolve(r#"["on_startup_finished", "on_startup"]"#).plugins[0].activation,
            Activation::AtStartup
        );
        // No explicit token with declared commands: lazy.
        assert_eq!(resolve("[]").plugins[0].activation, Activation::Lazy);
        // Unknown tokens log and are ignored.
        let scan = resolve(r#"["on_gaze"]"#);
        assert_eq!(scan.plugins[0].activation, Activation::Lazy);
        assert_eq!(
            scan.logs,
            vec!["p: ignoring unknown activation 'on_gaze'".to_owned()]
        );
    }

    #[test]
    fn scan_orders_plugins_by_dir_name() {
        let dir = TempDir::new("ordering");
        for id in ["zeta", "alpha", "mid"] {
            let plugin = dir.plugin(id);
            fs::write(plugin.join(SCRIPT_FILE_NAME), "print('hi')").expect("write script");
        }
        let scan = scan(&dir.0);
        let ids: Vec<&str> = scan.plugins.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["alpha", "mid", "zeta"]);
    }

    #[test]
    fn deeper_nesting_is_ignored() {
        let dir = TempDir::new("nesting");
        let plugin = dir.plugin("outer");
        fs::write(plugin.join(MANIFEST_FILE_NAME), r#"{ "name": "Outer" }"#)
            .expect("write manifest");
        fs::create_dir_all(plugin.join("inner")).expect("create nested dir");
        fs::write(
            plugin.join("inner").join(SCRIPT_FILE_NAME),
            "print('nested')",
        )
        .expect("write nested script");
        let scan = scan(&dir.0);
        assert_eq!(scan.plugins.len(), 1);
        assert_eq!(scan.plugins[0].id, "outer");
        assert!(!scan.plugins[0].has_script, "nested plugin.lua is not seen");
    }

    #[test]
    fn missing_store_reads_empty_silently() {
        let dir = TempDir::new("store-missing");
        let (values, logs) = read_store(&dir.0.join(STORE_FILE_NAME));
        assert!(values.is_empty());
        assert!(logs.is_empty());
    }

    #[test]
    fn malformed_store_reads_empty_with_one_line() {
        let dir = TempDir::new("store-bad");
        dir.write(STORE_FILE_NAME, "{");
        let (values, logs) = read_store(&dir.0.join(STORE_FILE_NAME));
        assert!(values.is_empty());
        assert_eq!(logs.len(), 1);
        assert!(
            logs[0].starts_with("plugins-settings.json is not valid JSON: "),
            "{logs:?}"
        );
    }

    #[test]
    fn non_object_store_root_reads_empty_with_one_line() {
        let dir = TempDir::new("store-array");
        dir.write(STORE_FILE_NAME, "[]");
        let (values, logs) = read_store(&dir.0.join(STORE_FILE_NAME));
        assert!(values.is_empty());
        assert_eq!(
            logs,
            vec!["plugins-settings.json is not an object; starting empty".to_owned()]
        );
    }

    #[test]
    fn non_object_store_entry_dropped_with_one_line() {
        let dir = TempDir::new("store-entry");
        dir.write(STORE_FILE_NAME, r#"{ "good": { "k": true }, "p": 5 }"#);
        let (values, logs) = read_store(&dir.0.join(STORE_FILE_NAME));
        assert_eq!(values.len(), 1);
        assert!(values.contains_key("good"));
        assert_eq!(
            logs,
            vec!["plugins-settings.json: dropping 'p' entries (expected an object)".to_owned()]
        );
    }

    #[test]
    fn store_round_trips_every_settings_type() {
        let dir = TempDir::new("store-roundtrip");
        let path = dir.0.join(STORE_FILE_NAME);
        let mut values = StoreValues::new();
        let mut rows = BTreeMap::new();
        rows.insert("b".to_owned(), json!(true));
        rows.insert("s".to_owned(), json!("text"));
        rows.insert("n".to_owned(), json!(2.5));
        rows.insert("e".to_owned(), json!("pretty"));
        rows.insert("a".to_owned(), json!([1, "x"]));
        rows.insert("o".to_owned(), json!({ "k": null }));
        values.insert("p".to_owned(), rows);

        write_store(&path, &values).expect("write store");
        let (read_back, logs) = read_store(&path);
        assert!(logs.is_empty(), "{logs:?}");
        assert_eq!(read_back, values);
    }

    #[test]
    fn store_write_is_atomic_and_overwrites() {
        let dir = TempDir::new("store-atomic");
        let path = dir.0.join(STORE_FILE_NAME);
        let mut first = StoreValues::new();
        first.insert(
            "p".to_owned(),
            BTreeMap::from([("k".to_owned(), json!("first"))]),
        );
        write_store(&path, &first).expect("first write");
        let mut second = StoreValues::new();
        second.insert(
            "p".to_owned(),
            BTreeMap::from([("k".to_owned(), json!("second"))]),
        );
        write_store(&path, &second).expect("second write");

        let (values, _) = read_store(&path);
        assert_eq!(
            values["p"]["k"],
            json!("second"),
            "the rename replaced the file"
        );
        let leftovers: Vec<_> = fs::read_dir(&dir.0)
            .expect("read dir")
            .filter_map(|entry| {
                let name = entry.expect("entry").file_name().into_string().ok()?;
                Path::new(&name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
                    .then_some(name)
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temp file left behind: {leftovers:?}"
        );
    }
}
