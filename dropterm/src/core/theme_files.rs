// SPDX-License-Identifier: GPL-3.0-only

//! User color scheme files (theme-files spec, Tier 0 of the plugins
//! initiative): one RON file per scheme under the config dir's
//! `themes/` subdirectory, loaded beside the built-in registry. The
//! loader is a read-only scan — parse each `.ron`, validate the
//! `foreground` + `background` minimum, skip-and-log every failure
//! with one `dropterm:` line — and `DropTerm` never writes, moves, or
//! renames anything under `themes/`; the files are user-authored
//! (unlike the config store's own sections, no `.bak` quarantine).

use std::fmt;
use std::io;
use std::path::Path;

use serde::Deserialize;

use crate::core::theme_source::{ColorScheme, ColorSchemeAnsi, Rgb, Slot};

// --- the file-facing twin ---

/// The file-facing serde twin of [`ColorScheme`]: the same field set
/// with every slot `Option` and colors as `#RRGGBB` string literals
/// (the notation every palette upstream publishes). Absent fields
/// load as `None`; [`SchemeFile::resolve`] converts into the engine's
/// representation through the existing `From<&ColorScheme>` path, so
/// a preview chip can never disagree with the live scheme.
#[derive(Deserialize)]
struct SchemeFile {
    foreground: Option<String>,
    background: Option<String>,
    cursor: Option<String>,
    bright_foreground: Option<String>,
    dim_foreground: Option<String>,
    normal: Option<AnsiFile>,
    bright: Option<AnsiFile>,
    dim: Option<AnsiFile>,
}

/// One file-facing 8-color table (the normal, bright, or dim
/// variant); `Default` is the all-`None` table an omitted or empty
/// `()` block leaves behind.
#[derive(Deserialize, Default)]
struct AnsiFile {
    black: Option<String>,
    red: Option<String>,
    green: Option<String>,
    yellow: Option<String>,
    blue: Option<String>,
    magenta: Option<String>,
    cyan: Option<String>,
    white: Option<String>,
}

/// Decode one file-facing color literal: `#` + exactly six hex
/// digits, case-insensitive. Anything else is `None` (the caller's
/// [`SkipReason::BadHex`] carries the literal).
fn parse_hex(value: &str) -> Option<Rgb> {
    let digits = value.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |low: usize| {
        u8::from_str_radix(&digits[low..low + 2], 16)
            .expect("two pre-validated ASCII hex digits always parse")
    };
    Some(Rgb {
        r: channel(0),
        g: channel(2),
        b: channel(4),
    })
}

impl AnsiFile {
    /// Resolve one table, naming each slot's field path for the
    /// [`SkipReason::BadHex`] reason (`normal.red`, …).
    fn resolve(&self, table: &str) -> Result<ColorSchemeAnsi, SkipReason> {
        let slot = |field: &str, value: &Option<String>| -> Result<Slot, SkipReason> {
            match value {
                None => Ok(None),
                Some(value) => parse_hex(value).map(Some).ok_or(SkipReason::BadHex {
                    field: format!("{table}.{field}"),
                    value: value.clone(),
                }),
            }
        };
        Ok(ColorSchemeAnsi {
            black: slot("black", &self.black)?,
            red: slot("red", &self.red)?,
            green: slot("green", &self.green)?,
            yellow: slot("yellow", &self.yellow)?,
            blue: slot("blue", &self.blue)?,
            magenta: slot("magenta", &self.magenta)?,
            cyan: slot("cyan", &self.cyan)?,
            white: slot("white", &self.white)?,
        })
    }
}

impl SchemeFile {
    /// Convert the twin into the engine's scheme representation.
    /// Color literals resolve first (a bad one names its field), then
    /// the minimum-viable rule applies: `foreground` and
    /// `background` are both required, everything else derives at
    /// conversion.
    fn resolve(self) -> Result<ColorScheme, SkipReason> {
        let special = |field: &str, value: &Option<String>| -> Result<Slot, SkipReason> {
            match value {
                None => Ok(None),
                Some(value) => parse_hex(value).map(Some).ok_or(SkipReason::BadHex {
                    field: field.to_string(),
                    value: value.clone(),
                }),
            }
        };
        let scheme = ColorScheme {
            foreground: special("foreground", &self.foreground)?,
            background: special("background", &self.background)?,
            cursor: special("cursor", &self.cursor)?,
            bright_foreground: special("bright_foreground", &self.bright_foreground)?,
            dim_foreground: special("dim_foreground", &self.dim_foreground)?,
            normal: self.normal.unwrap_or_default().resolve("normal")?,
            bright: self.bright.unwrap_or_default().resolve("bright")?,
            dim: self.dim.unwrap_or_default().resolve("dim")?,
        };
        if scheme.foreground.is_none() {
            return Err(SkipReason::MissingAnchor("foreground"));
        }
        if scheme.background.is_none() {
            return Err(SkipReason::MissingAnchor("background"));
        }
        Ok(scheme)
    }
}

// --- the loader ---

/// One loaded user scheme: the file stem as its name (identity,
/// persisted under `color_scheme_dark`/`_light`, shown in the
/// dropdowns) and the scheme resolved through the same conversion
/// the built-ins ride. A file that fails the minimum never becomes
/// one — it is skipped with a single `dropterm:` line, the file
/// itself never touched.
pub struct UserScheme {
    /// The scheme's display name — the trimmed file stem.
    pub name: String,
    /// The validated scheme.
    pub(crate) scheme: ColorScheme,
}

/// Why one file was skipped — the named reason its `dropterm:` line
/// carries; data, so the tests pin it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SkipReason {
    /// Not parseable RON (the parser's own detail rides the line).
    UnparseableRon(String),
    /// The file could not be read (permissions, or not UTF-8 text).
    Unreadable(String),
    /// A color literal is not `#RRGGBB`; `field` names its path.
    BadHex { field: String, value: String },
    /// The minimum-viable rule: this anchor is required.
    MissingAnchor(&'static str),
    /// The file stem holds no usable scheme name.
    BadName,
    /// The name names a built-in scheme; the built-in wins.
    BuiltinCollision,
    /// Another file already claimed the name (first in sorted order
    /// wins).
    DuplicateName,
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnparseableRon(detail) => write!(f, "not valid RON: {detail}"),
            Self::Unreadable(detail) => write!(f, "unreadable: {detail}"),
            Self::BadHex { field, value } => {
                write!(f, "field {field} holds {value:?}, not #RRGGBB")
            }
            Self::MissingAnchor(field) => write!(f, "missing the required {field}"),
            Self::BadName => write!(f, "the file name holds no scheme name"),
            Self::BuiltinCollision => write!(f, "names the built-in scheme; the built-in wins"),
            Self::DuplicateName => {
                write!(
                    f,
                    "another file already claimed this name; the first in sorted order wins"
                )
            }
        }
    }
}

/// One parseable candidate ahead of the merge rules: everything the
/// skip lines name and the sort orders by.
struct Candidate {
    /// The file's name in the dir, for the log lines.
    file: String,
    /// The trimmed stem.
    name: String,
    /// The parsed scheme.
    scheme: ColorScheme,
}

/// Parse one file's text into the engine's scheme representation:
/// RON into the twin, the twin through [`SchemeFile::resolve`]. The
/// named [`SkipReason`] is the loader's log line.
///
/// The parse carries `IMPLICIT_SOME`: the file shape is the bare
/// literal (`foreground: "#e6edf3"`, never `Some("#e6edf3")`) — the
/// hand-written notation the docs teach — while an explicit `None`
/// slot still loads as `None`.
fn parse_scheme(text: &str) -> Result<ColorScheme, SkipReason> {
    let options = ron::options::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    let file: SchemeFile = options
        .from_str(text)
        .map_err(|err| SkipReason::UnparseableRon(err.to_string()))?;
    file.resolve()
}

/// The loader's one skip line.
fn skip(file: &str, reason: &SkipReason) {
    println!("dropterm: themes: skipped {file}: {reason}");
}

/// Scan `dir` for `.ron` scheme files and load every valid one, the
/// merge rules applied: schemes sort by name, a name colliding with a
/// built-in is shadowed (the built-in wins, the file ignored), and
/// two files resolving to one name keep the first in file-name sorted
/// order. Each loaded scheme and each skip is one `dropterm:` line; a
/// missing or empty dir is a silent empty registry (first-run quiet,
/// the `load_from` precedent); the dir and every file in it stay
/// untouched.
pub fn load_dir(dir: &Path) -> Vec<UserScheme> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // A missing dir is the silent first run, not an error.
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(err) => {
            println!("dropterm: themes: cannot read {}: {err}", dir.display());
            return Vec::new();
        }
    };

    let mut candidates: Vec<Candidate> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // Only `.ron` files participate; anything else in the dir is
        // silently ignored.
        if path.extension().is_none_or(|ext| ext != "ron") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            // Not UTF-8: the file cannot be addressed as a name.
            continue;
        };
        let file = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = stem.trim();
        if name.is_empty() {
            skip(&file, &SkipReason::BadName);
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => {
                skip(&file, &SkipReason::Unreadable(err.to_string()));
                continue;
            }
        };
        match parse_scheme(&text) {
            Ok(scheme) => candidates.push(Candidate {
                file,
                name: name.to_string(),
                scheme,
            }),
            Err(reason) => skip(&file, &reason),
        }
    }

    // Sorted by name, then file name, so every rule below is
    // deterministic — a shared name keeps its first file.
    candidates.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.file.cmp(&b.file)));

    let mut loaded: Vec<UserScheme> = Vec::new();
    for candidate in candidates {
        if loaded.iter().any(|kept| kept.name == candidate.name) {
            skip(&candidate.file, &SkipReason::DuplicateName);
            continue;
        }
        // The built-in registry is the documented vocabulary: a user
        // file naming one is shadowed, never an override.
        if crate::core::theme_source::by_name(&candidate.name).is_some() {
            skip(&candidate.file, &SkipReason::BuiltinCollision);
            continue;
        }
        println!(
            "dropterm: themes: loaded {} from {}",
            candidate.name, candidate.file
        );
        loaded.push(UserScheme {
            name: candidate.name,
            scheme: candidate.scheme,
        });
    }
    loaded
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use alacritty_terminal::vte::ansi::NamedColor;

    use super::*;
    use crate::core::theme_source;

    /// A per-test scratch directory under the system temp dir with a
    /// process- and test-unique name, removed (best-effort) on drop
    /// even when an assertion fails. Std only — no dev-dependency.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let unique = format!(
                "dropterm-theme-files-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir_all(&path).expect("create scratch dir");
            Self(path)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
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

    fn colors_of(scheme: &UserScheme) -> alacritty_terminal::term::color::Colors {
        alacritty_terminal::term::color::Colors::from(&scheme.scheme)
    }

    /// The minimal valid scheme text — fg + bg, nothing else.
    const MINIMAL: &str = r##"(foreground: "#e6edf3", background: "#0d1117")"##;

    // --- the file minimum ---

    #[test]
    fn a_minimal_fg_bg_file_loads() {
        let dir = TempDir::new("minimal");
        fs::write(dir.join("mine.ron"), MINIMAL).expect("write scheme");
        let loaded = load_dir(&dir.0);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "mine");
        let colors = colors_of(&loaded[0]);
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xE6_EDF3)));
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x0D_1117)));
        // Derivation owns the rest of the specials: the cursor follows
        // the foreground through the shared conversion.
        assert_eq!(colors[NamedColor::Cursor], Some(rgb(0xE6_EDF3)));
    }

    #[test]
    fn a_full_file_loads_every_slot() {
        let dir = TempDir::new("full");
        fs::write(
            dir.join("full.ron"),
            r##"(
                foreground: "#f8f8f2",
                background: "#282a36",
                cursor: "#ffcc00",
                bright_foreground: "#ffffff",
                dim_foreground: "#898989",
                normal: (
                    black: "#21222c",
                    red: "#ff5555",
                    green: "#50fa7b",
                    yellow: "#f1fa8c",
                    blue: "#bd93f9",
                    magenta: "#ff79c6",
                    cyan: "#8be9fd",
                    white: "#f8f8f2",
                ),
                bright: (
                    black: "#6272a4",
                    red: "#ff6e6e",
                    green: "#69ff94",
                    yellow: "#ffffa5",
                    blue: "#d6acff",
                    magenta: "#ff92df",
                    cyan: "#a4ffff",
                    white: "#ffffff",
                ),
                dim: (
                    green: "#34a551",
                ),
            )"##,
        )
        .expect("write scheme");
        let loaded = load_dir(&dir.0);
        assert_eq!(loaded.len(), 1);
        let colors = colors_of(&loaded[0]);
        assert_eq!(colors[NamedColor::Cursor], Some(rgb(0xFF_CC00)));
        assert_eq!(colors[NamedColor::BrightForeground], Some(rgb(0xFF_FFFF)));
        assert_eq!(colors[NamedColor::DimForeground], Some(rgb(0x89_8989)));
        assert_eq!(colors[NamedColor::Red], Some(rgb(0xFF_5555)));
        assert_eq!(colors[NamedColor::BrightCyan], Some(rgb(0xA4_FFFF)));
        // An explicit dim wins over the derivation...
        assert_eq!(colors[NamedColor::DimGreen], Some(rgb(0x34_A551)));
        // ...while the untouched dim slots still derive (0xFF * 0.66
        // truncates to 0xA8, 0x55 * 0.66 to 0x38).
        assert_eq!(colors[NamedColor::DimRed], Some(rgb(0xA8_3838)));
    }

    #[test]
    fn slots_a_file_leaves_out_stay_none_at_conversion() {
        // The minimum is fg + bg; every other slot is optional and the
        // existing conversion derives what it derives — a missing
        // normal or bright slot is not invented, it stays `None`.
        let scheme = parse_scheme(MINIMAL).expect("minimal scheme parses");
        let colors = alacritty_terminal::term::color::Colors::from(&scheme);
        assert_eq!(colors[NamedColor::Red], None);
        assert_eq!(colors[NamedColor::BrightBlack], None);
    }

    // --- skip-and-log ---

    #[test]
    fn bad_ron_is_skipped_with_its_reason() {
        // The parser's own detail rides the reason's payload (and the
        // log line); the pin is the variant.
        assert!(matches!(
            parse_scheme("garbage((").unwrap_err(),
            SkipReason::UnparseableRon(_)
        ));
        let dir = TempDir::new("bad-ron");
        fs::write(dir.join("broken.ron"), "garbage((").expect("write scheme");
        assert!(load_dir(&dir.0).is_empty());
    }

    #[test]
    fn bad_hex_is_skipped_with_its_reason() {
        assert_eq!(
            parse_scheme(r##"(foreground: "#12345", background: "#0d1117")"##).unwrap_err(),
            SkipReason::BadHex {
                field: "foreground".to_string(),
                value: "#12345".to_string(),
            }
        );
        // Table slots name their own field path.
        assert_eq!(
            parse_scheme(
                r##"(foreground: "#e6edf3", background: "#0d1117", normal: (red: "#zz5555"))"##
            )
            .unwrap_err(),
            SkipReason::BadHex {
                field: "normal.red".to_string(),
                value: "#zz5555".to_string(),
            }
        );
        let dir = TempDir::new("bad-hex");
        fs::write(
            dir.join("bad-hex.ron"),
            r##"(foreground: "#e6edf3", background "#0d1117")"##,
        )
        .expect("write scheme");
        assert!(load_dir(&dir.0).is_empty());
    }

    #[test]
    fn a_missing_anchor_is_skipped_with_its_reason() {
        assert_eq!(
            parse_scheme(r##"(background: "#0d1117")"##).unwrap_err(),
            SkipReason::MissingAnchor("foreground")
        );
        assert_eq!(
            parse_scheme(r##"(foreground: "#e6edf3")"##).unwrap_err(),
            SkipReason::MissingAnchor("background")
        );
    }

    #[test]
    fn a_stem_that_trims_to_nothing_is_skipped() {
        let dir = TempDir::new("bad-name");
        fs::write(dir.join(" .ron"), MINIMAL).expect("write scheme");
        assert!(load_dir(&dir.0).is_empty());
    }

    #[test]
    fn non_ron_files_are_ignored() {
        let dir = TempDir::new("non-ron");
        fs::write(dir.join("notes.txt"), "not a scheme").expect("write noise");
        fs::write(dir.join("UPPER.RON"), "not a scheme").expect("write noise");
        fs::write(dir.join("real.ron"), MINIMAL).expect("write scheme");
        let loaded = load_dir(&dir.0);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "real");
    }

    #[test]
    fn a_missing_or_empty_dir_loads_zero_schemes() {
        // First-run quiet, the `load_from` precedent: no dir and no
        // files are both a silent empty registry.
        let dir = TempDir::new("quiet");
        assert!(load_dir(&dir.join("absent")).is_empty());
        assert!(load_dir(&dir.0).is_empty());
    }

    // --- the merge rules ---

    #[test]
    fn user_schemes_sort_alphabetically_behind_the_registry() {
        let dir = TempDir::new("sorted");
        fs::write(
            dir.join("b-second.ron"),
            r##"(foreground: "#ffffff", background: "#121212")"##,
        )
        .expect("write scheme");
        fs::write(
            dir.join("a-first.ron"),
            r##"(foreground: "#ffffff", background: "#131313")"##,
        )
        .expect("write scheme");
        let loaded = load_dir(&dir.0);
        let names: Vec<&str> = loaded.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["a-first", "b-second"]);

        let merged = theme_source::merged_scheme_names(&loaded);
        let registry: Vec<&str> = theme_source::scheme_names();
        assert_eq!(&merged[..registry.len()], &registry);
        assert_eq!(&merged[registry.len()..], &["a-first", "b-second"]);
    }

    #[test]
    fn a_name_colliding_with_a_builtin_is_shadowed() {
        // The built-in registry is the documented vocabulary: a user
        // file naming one is dropped at load, and the built-in still
        // serves the lookup.
        let dir = TempDir::new("collision");
        fs::write(
            dir.join("Nord.ron"),
            r##"(foreground: "#ffffff", background: "#000000")"##,
        )
        .expect("write scheme");
        assert!(load_dir(&dir.0).is_empty());
    }

    #[test]
    fn same_name_duplicates_resolve_deterministically() {
        // Two files whose stems trim to the same name ("dup " and
        // "dup"): file-name sorted order picks the winner — "dup .ron"
        // sorts before "dup.ron" — and the loser is skipped.
        let dir = TempDir::new("duplicate");
        fs::write(
            dir.join("dup .ron"),
            r##"(foreground: "#ffffff", background: "#111111")"##,
        )
        .expect("write scheme");
        fs::write(
            dir.join("dup.ron"),
            r##"(foreground: "#ffffff", background: "#222222")"##,
        )
        .expect("write scheme");
        let loaded = load_dir(&dir.0);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "dup");
        assert_eq!(
            colors_of(&loaded[0])[NamedColor::Background],
            Some(rgb(0x11_1111))
        );
    }
}
