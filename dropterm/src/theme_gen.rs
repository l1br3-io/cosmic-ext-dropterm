// SPDX-License-Identifier: GPL-3.0-only

//! kit-pass C4 — the scheme → theme-JSON generator (a spike artifact,
//! a Phase 3 input). DropTerm's resolved scheme is an Alacritty
//! `Colors` (the type `theme_glue` produces); the Kit's theme entry is
//! a `ThemeConfig` (JSON-shaped, `#[non_exhaustive]` since v0.9.0 — so
//! the generator emits JSON, never struct literals). This module maps
//! the ~18 scheme colors onto the chrome-bearing token keys; every key
//! the scheme cannot source stays absent and inherits the Kit's
//! defaults — the generator invents no colors.

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::NamedColor;
use serde_json::{Map, Value};

/// `#RRGGBB` for a scheme slot, `None` when the scheme leaves it unset.
fn hex_of(colors: &Colors, nc: NamedColor) -> Option<String> {
    colors[nc].map(|rgb| format!("#{:02X}{:02X}{:02X}", rgb.r, rgb.g, rgb.b))
}

/// The DropTerm scheme → gpui-component `ThemeConfig` JSON value.
///
/// The mapping: terminal surfaces (`background`/`foreground`,
/// popover, sidebar, tab bar, list) ride the scheme's own Background/
/// Foreground; the status colors ride their ANSI slots (danger←Red,
/// success←Green, warning←Yellow, info←Cyan, with the Bright variants
/// as hover/active and Background as their text); primary and accent
/// ride Blue/BrightBlue; the dim set (borders, muted, scrollbar thumb,
/// secondary) rides BrightBlack. Absent scheme slots leave keys absent.
pub fn kit_theme_json(colors: &Colors, name: &str, dark: bool) -> Value {
    let mut c = Map::new();
    {
        let bg = hex_of(colors, NamedColor::Background);
        let fg = hex_of(colors, NamedColor::Foreground);
        let dim = hex_of(colors, NamedColor::BrightBlack);
        let blue = hex_of(colors, NamedColor::Blue);
        let bright_blue = hex_of(colors, NamedColor::BrightBlue);
        let on_accent = bg.clone();

        let mut set = |map: &mut Map<String, Value>, key: &str, v: Option<String>| {
            if let Some(v) = v {
                map.insert(key.into(), Value::String(v));
            }
        };

        // Surfaces follow the terminal's own background/foreground.
        for (k, v) in [
            ("background", bg.clone()),
            ("foreground", fg.clone()),
            ("popover.background", bg.clone()),
            ("popover.foreground", fg.clone()),
            ("sidebar.background", bg.clone()),
            ("sidebar.foreground", fg.clone()),
            ("tab_bar.background", bg.clone()),
            ("tab.background", bg.clone()),
            ("tab.foreground", fg.clone()),
            ("tab.active.background", bright_blue.clone()),
            ("tab.active.foreground", on_accent.clone()),
            ("list.background", bg.clone()),
            ("list.hover.background", dim.clone()),
        ] {
            set(&mut c, k, v);
        }

        // The dim set: borders, muted text, scrollbar, secondary chrome.
        for (k, v) in [
            ("muted.foreground", dim.clone()),
            ("border", dim.clone()),
            ("drag.border", dim.clone()),
            ("input.border", dim.clone()),
            ("sidebar.border", dim.clone()),
            ("scrollbar.thumb.background", dim.clone()),
            ("secondary.background", dim.clone()),
            ("secondary.foreground", fg.clone()),
            ("button.secondary.background", dim.clone()),
            ("button.secondary.foreground", fg.clone()),
            ("selection.background", dim.clone()),
            ("caret", fg.clone()),
        ] {
            set(&mut c, k, v);
        }

        // Accents: primary rides Blue, the accent/link family BrightBlue.
        for (k, v) in [
            ("primary.background", blue.clone()),
            ("primary.hover.background", bright_blue.clone()),
            ("primary.active.background", bright_blue.clone()),
            ("primary.foreground", on_accent.clone()),
            ("accent.background", bright_blue.clone()),
            ("accent.foreground", on_accent.clone()),
            ("link", bright_blue.clone()),
            ("link.hover", blue.clone()),
            ("link.active", blue.clone()),
        ] {
            set(&mut c, k, v);
        }

        // Status colors: each ANSI slot with its Bright variant as
        // hover/active and the terminal background as text.
        let status = [
            ("danger", NamedColor::Red, NamedColor::BrightRed),
            ("success", NamedColor::Green, NamedColor::BrightGreen),
            ("warning", NamedColor::Yellow, NamedColor::BrightYellow),
            ("info", NamedColor::Cyan, NamedColor::BrightCyan),
        ];
        for (base, plain, bright) in status {
            set(&mut c, &format!("{base}.background"), hex_of(colors, plain));
            set(
                &mut c,
                &format!("{base}.hover.background"),
                hex_of(colors, bright),
            );
            set(
                &mut c,
                &format!("{base}.active.background"),
                hex_of(colors, bright),
            );
            set(&mut c, &format!("{base}.foreground"), on_accent.clone());
        }
    }

    Value::Object(Map::from_iter([
        ("name".into(), Value::String(name.into())),
        (
            "mode".into(),
            Value::String(if dark { "dark" } else { "light" }.into()),
        ),
        ("colors".into(), Value::Object(c)),
    ]))
}

/// A scheme for tests and the `theme` variant: distinctive values so
/// applied-state evidence (logs, screenshots) is unambiguous.
pub fn dropterm_dark() -> Colors {
    let mut c = Colors::default();
    let mut set = |c: &mut Colors, nc: NamedColor, (r, g, b): (u8, u8, u8)| {
        c[nc] = Some(alacritty_terminal::vte::ansi::Rgb { r, g, b });
    };
    set(&mut c, NamedColor::Background, (0x1A, 0x1B, 0x26));
    set(&mut c, NamedColor::Foreground, (0xC0, 0xC5, 0xCE));
    set(&mut c, NamedColor::Black, (0x16, 0x18, 0x22));
    set(&mut c, NamedColor::Red, (0xF7, 0x76, 0x8C));
    set(&mut c, NamedColor::Green, (0x9E, 0xCE, 0x6A));
    set(&mut c, NamedColor::Yellow, (0xE0, 0xAF, 0x68));
    set(&mut c, NamedColor::Blue, (0x7A, 0xA2, 0xF7));
    set(&mut c, NamedColor::Magenta, (0xBB, 0x9A, 0xF7));
    set(&mut c, NamedColor::Cyan, (0x7D, 0xCF, 0xAF));
    set(&mut c, NamedColor::White, (0xA9, 0xB1, 0xD6));
    set(&mut c, NamedColor::BrightBlack, (0x41, 0x47, 0x5C));
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn colors_of(v: &Value) -> &serde_json::Map<String, Value> {
        v["colors"].as_object().expect("colors object")
    }

    #[test]
    fn core_mapping_carries_the_scheme_into_the_token_keys() {
        let v = kit_theme_json(&dropterm_dark(), "dropterm-dark", true);
        let c = colors_of(&v);
        assert_eq!(c["background"], json!("#1A1B26"));
        assert_eq!(c["foreground"], json!("#C0C5CE"));
        // primary rides the scheme's Blue, with Background for its text
        assert_eq!(c["primary.background"], json!("#7AA2F7"));
        assert_eq!(c["primary.foreground"], json!("#1A1B26"));
        // the status colors ride their ANSI slots
        assert_eq!(c["danger.background"], json!("#F7768C"));
        assert_eq!(c["success.background"], json!("#9ECE6A"));
        assert_eq!(c["warning.background"], json!("#E0AF68"));
        assert_eq!(c["info.background"], json!("#7DCFAF"));
        // chrome surfaces follow the terminal's own background
        assert_eq!(c["popover.background"], json!("#1A1B26"));
        assert_eq!(c["popover.foreground"], json!("#C0C5CE"));
        assert_eq!(c["sidebar.background"], json!("#1A1B26"));
        // the dim/border set rides BrightBlack
        assert_eq!(c["muted.foreground"], json!("#41475C"));
        assert_eq!(c["border"], json!("#41475C"));
    }

    #[test]
    fn the_github_pair_generates_the_anchor_accent_blues() {
        // theme-defaults spec criterion 3: the chrome accent lands
        // on GitHub blue — light `#0969DA`, dark `#2F81F7` — through
        // the existing scheme→accent mapping (accent/link ride
        // BrightBlue, primary rides Blue), with no new setting. The
        // entries pin BrightBlue to the anchor so the accent is
        // exact; dark's primary keeps the port's terminal blue.
        let dark =
            dropterm::core::theme_source::by_name(dropterm::core::theme_source::GITHUB_THEME_DARK)
                .expect("registry entry");
        let v = kit_theme_json(&dark, "github-dark", true);
        let c = colors_of(&v);
        assert_eq!(c["accent.background"], json!("#2F81F7"));
        assert_eq!(c["link"], json!("#2F81F7"));
        assert_eq!(c["primary.background"], json!("#58A6FF"));
        assert_eq!(c["background"], json!("#0D1117"));

        let light =
            dropterm::core::theme_source::by_name(dropterm::core::theme_source::GITHUB_THEME_LIGHT)
                .expect("registry entry");
        let v = kit_theme_json(&light, "github-light", false);
        let c = colors_of(&v);
        assert_eq!(c["accent.background"], json!("#0969DA"));
        assert_eq!(c["link"], json!("#0969DA"));
        assert_eq!(c["primary.background"], json!("#0969DA"));
        assert_eq!(c["background"], json!("#FFFFFF"));
    }

    #[test]
    fn name_and_mode_pass_through() {
        let v = kit_theme_json(&dropterm_dark(), "dropterm-dark", true);
        assert_eq!(v["name"], json!("dropterm-dark"));
        assert_eq!(v["mode"], json!("dark"));
        let v = kit_theme_json(&dropterm_dark(), "dropterm-light", false);
        assert_eq!(v["mode"], json!("light"));
    }

    #[test]
    fn unset_scheme_slots_leave_keys_absent() {
        let empty = Colors::default();
        let v = kit_theme_json(&empty, "empty", true);
        assert!(
            colors_of(&v).is_empty(),
            "the generator must not invent colors: {v}"
        );
    }

    #[test]
    fn output_parses_as_a_kit_theme_config() {
        let v = kit_theme_json(&dropterm_dark(), "dropterm-dark", true);
        let config: gpui_component::ThemeConfig =
            serde_json::from_value(v).expect("valid ThemeConfig");
        assert_eq!(config.name.as_ref(), "dropterm-dark");
        assert_eq!(
            config.colors.background.as_deref(),
            Some("#1A1B26"),
            "the generated background must survive the parse"
        );
    }
}
