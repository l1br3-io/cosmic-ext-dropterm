// SPDX-License-Identifier: GPL-3.0-only
// Copyright 2023 System76 <info@system76.com>
//
// Adapted from cosmic-term (https://github.com/pop-os/cosmic-term)
// src/terminal_theme.rs and src/config.rs at commit 9129277 (palettes)
// and `3ef31d7` (the `ColorScheme` representation, color-schemes C1).
// The palettes are unmodified; cosmic-term's serde/RON-file,
// per-profile, and import/export machinery stays dropped — v1 compiles
// its schemes in and resolves them by name (see
// docs/research/terminal-theming.md).

use alacritty_terminal::{
    term::color::Colors,
    vte::ansi::{NamedColor, Rgb},
};

use crate::config::{COSMIC_THEME_DARK, COSMIC_THEME_LIGHT};

fn auto_colors() -> Colors {
    let mut colors = Colors::default();

    // These colors come from `ransid`: https://gitlab.redox-os.org/redox-os/ransid/-/blob/master/src/color.rs
    /* Indexed colors */
    for value in 16..=231 {
        let convert = |value: u8| -> u8 {
            match value {
                0 => 0,
                _ => value * 0x28 + 0x28,
            }
        };

        let r = convert((value - 16) / 36 % 6);
        let g = convert((value - 16) / 6 % 6);
        let b = convert((value - 16) % 6);
        colors[value as usize] = Some(Rgb { r, g, b });
    }

    /* Grays */
    for value in 232..=255 {
        let gray = (value - 232) * 10 + 8;
        colors[value as usize] = Some(Rgb {
            r: gray,
            g: gray,
            b: gray,
        });
    }

    colors
}

// --- scheme representation (color-schemes C1) ---
//
// Adapted from cosmic-term's `ColorScheme`/`ColorSchemeAnsi`
// (src/config.rs @ `3ef31d7`, GPL-3.0-only like DropTerm): same field
// shape, colors as plain `Option<Rgb>` and no serde — v1 compiles its
// schemes in, nothing parses. Phase 2 (user scheme files, ROADMAP
// "Later") can grow serde on this shape additively.

/// One color of a scheme table; `None` leaves the slot to a default.
type Slot = Option<Rgb>;

/// One 8-color table of a scheme: the normal, bright, or dim variant.
#[derive(Debug, Clone, Copy, Default)]
struct ColorSchemeAnsi {
    black: Slot,
    red: Slot,
    green: Slot,
    yellow: Slot,
    blue: Slot,
    magenta: Slot,
    cyan: Slot,
    white: Slot,
}

/// A complete color scheme: the industry-consensus field set
/// (foreground, background, cursor, bright/dim foreground, and the
/// normal/bright/dim 8-color tables — docs/research/terminal-theming.md
/// §4). A `None` background defers to the system theme via
/// `WINDOW_BG_COLOR`; any other `None` slot falls back to a uniform
/// derivation in the conversion below.
#[derive(Debug, Clone, Copy)]
struct ColorScheme {
    foreground: Slot,
    background: Slot,
    cursor: Slot,
    bright_foreground: Slot,
    dim_foreground: Slot,
    normal: ColorSchemeAnsi,
    bright: ColorSchemeAnsi,
    dim: ColorSchemeAnsi,
}

/// Alacritty's dim factor: a missing dim color derives as
/// `normal * 0.66` per channel, truncating (`alacritty/src/display/
/// color.rs` `DIM_FACTOR`; docs/research/terminal-theming.md §2).
const DIM_FACTOR: f32 = 0.66;

fn dim_color(color: Rgb) -> Rgb {
    Rgb {
        r: (f32::from(color.r) * DIM_FACTOR) as u8,
        g: (f32::from(color.g) * DIM_FACTOR) as u8,
        b: (f32::from(color.b) * DIM_FACTOR) as u8,
    }
}

/// The dim slot for one color: the scheme's explicit dim, else its
/// normal derived, else nothing (matching cosmic-term's conversion,
/// which leaves `None` fields as `None` slots).
fn dim_slot(dim: Slot, normal: Slot) -> Slot {
    dim.or_else(|| normal.map(dim_color))
}

impl From<&ColorScheme> for Colors {
    fn from(scheme: &ColorScheme) -> Self {
        let mut colors = auto_colors();

        let normal = &scheme.normal;
        colors[NamedColor::Black] = normal.black;
        colors[NamedColor::Red] = normal.red;
        colors[NamedColor::Green] = normal.green;
        colors[NamedColor::Yellow] = normal.yellow;
        colors[NamedColor::Blue] = normal.blue;
        colors[NamedColor::Magenta] = normal.magenta;
        colors[NamedColor::Cyan] = normal.cyan;
        colors[NamedColor::White] = normal.white;

        let bright = &scheme.bright;
        colors[NamedColor::BrightBlack] = bright.black;
        colors[NamedColor::BrightRed] = bright.red;
        colors[NamedColor::BrightGreen] = bright.green;
        colors[NamedColor::BrightYellow] = bright.yellow;
        colors[NamedColor::BrightBlue] = bright.blue;
        colors[NamedColor::BrightMagenta] = bright.magenta;
        colors[NamedColor::BrightCyan] = bright.cyan;
        colors[NamedColor::BrightWhite] = bright.white;

        let dim = &scheme.dim;
        colors[NamedColor::DimBlack] = dim_slot(dim.black, normal.black);
        colors[NamedColor::DimRed] = dim_slot(dim.red, normal.red);
        colors[NamedColor::DimGreen] = dim_slot(dim.green, normal.green);
        colors[NamedColor::DimYellow] = dim_slot(dim.yellow, normal.yellow);
        colors[NamedColor::DimBlue] = dim_slot(dim.blue, normal.blue);
        colors[NamedColor::DimMagenta] = dim_slot(dim.magenta, normal.magenta);
        colors[NamedColor::DimCyan] = dim_slot(dim.cyan, normal.cyan);
        colors[NamedColor::DimWhite] = dim_slot(dim.white, normal.white);

        // Specials: explicit values win; the defaults follow alacritty
        // — cursor and bright foreground default to the foreground,
        // dim foreground derives from it (research §2/§3).
        colors[NamedColor::Foreground] = scheme.foreground;
        colors[NamedColor::Background] = scheme.background;
        colors[NamedColor::Cursor] = scheme.cursor.or(scheme.foreground);
        colors[NamedColor::BrightForeground] = scheme.bright_foreground.or(scheme.foreground);
        colors[NamedColor::DimForeground] = scheme
            .dim_foreground
            .or_else(|| scheme.foreground.map(dim_color));

        colors
    }
}

/// Decode a packed `0xRRGGBB` literal into an `Rgb` (const, for the
/// palette tables).
const fn rgb(data: u32) -> Rgb {
    Rgb {
        r: (data >> 16) as u8,
        g: (data >> 8) as u8,
        b: data as u8,
    }
}

/// The COSMIC Dark palette, byte-identical to the scheme this module
/// always shipped (pinned by test): a hand-tuned dim table the uniform
/// derivation never touches, and no background — the window follows
/// the COSMIC system theme.
const COSMIC_DARK: ColorScheme = ColorScheme {
    foreground: Some(rgb(0xC4C4C4)),
    background: None,
    cursor: Some(rgb(0xC4C4C4)),
    bright_foreground: Some(rgb(0xC4C4C4)),
    dim_foreground: Some(rgb(0x898989)),
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x1B1B1B)),
        red: Some(rgb(0xF16161)),
        green: Some(rgb(0x7CB987)),
        yellow: Some(rgb(0xDDC74C)),
        blue: Some(rgb(0x6296BE)),
        magenta: Some(rgb(0xBE6DEE)),
        cyan: Some(rgb(0x49BAC8)),
        white: Some(rgb(0xBEBEBE)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x808080)),
        red: Some(rgb(0xFF8985)),
        green: Some(rgb(0x97D5A0)),
        yellow: Some(rgb(0xFAE365)),
        blue: Some(rgb(0x7DB1DA)),
        magenta: Some(rgb(0xD68EFF)),
        cyan: Some(rgb(0x49BAC8)),
        white: Some(rgb(0xC4C4C4)),
    },
    dim: ColorSchemeAnsi {
        black: Some(rgb(0x000000)),
        red: Some(rgb(0xA04040)),
        green: Some(rgb(0x5D7D62)),
        yellow: Some(rgb(0x9E914A)),
        blue: Some(rgb(0x486073)),
        magenta: Some(rgb(0x7F46A1)),
        cyan: Some(rgb(0x3F7F87)),
        white: Some(rgb(0x898989)),
    },
};

pub fn cosmic_dark() -> Colors {
    Colors::from(&COSMIC_DARK)
}

/// The COSMIC Light palette — same traits as [`COSMIC_DARK`]:
/// hand-tuned dims, no background.
const COSMIC_LIGHT: ColorScheme = ColorScheme {
    foreground: Some(rgb(0x1B1B1B)),
    background: None,
    cursor: Some(rgb(0x1B1B1B)),
    bright_foreground: Some(rgb(0x000000)),
    dim_foreground: Some(rgb(0x262626)),
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x1B1B1B)),
        red: Some(rgb(0xA24F50)),
        green: Some(rgb(0x437E4F)),
        yellow: Some(rgb(0x7D6E1E)),
        blue: Some(rgb(0x516D94)),
        magenta: Some(rgb(0x9A30CA)),
        cyan: Some(rgb(0x227A85)),
        white: Some(rgb(0xD7D7D7)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x000000)),
        red: Some(rgb(0x890418)),
        green: Some(rgb(0x185529)),
        yellow: Some(rgb(0x534800)),
        blue: Some(rgb(0x2E496D)),
        magenta: Some(rgb(0x6B0091)),
        cyan: Some(rgb(0x00525A)),
        white: Some(rgb(0xE8E8E8)),
    },
    dim: ColorSchemeAnsi {
        black: Some(rgb(0x262626)),
        red: Some(rgb(0xB25D5E)),
        green: Some(rgb(0x528D5E)),
        yellow: Some(rgb(0x8C7D30)),
        blue: Some(rgb(0x5F7CA4)),
        magenta: Some(rgb(0xAA43DB)),
        cyan: Some(rgb(0x358994)),
        white: Some(rgb(0xC4C4C4)),
    },
};

pub fn cosmic_light() -> Colors {
    Colors::from(&COSMIC_LIGHT)
}

/// Look up one of the two built-in COSMIC color schemes by name.
///
/// v1 ships only these two; custom on-disk color-scheme files are not yet
/// supported (see the project ROADMAP's "Later" section).
pub fn by_name(name: &str) -> Option<Colors> {
    if name == COSMIC_THEME_DARK {
        Some(cosmic_dark())
    } else if name == COSMIC_THEME_LIGHT {
        Some(cosmic_light())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scheme with foreground, background, normal and bright tables,
    /// and nothing else — the shape alacritty-theme TOMLs give us.
    fn plain_scheme() -> ColorScheme {
        ColorScheme {
            foreground: Some(rgb(0xF8F8F2)),
            background: Some(rgb(0x282A36)),
            cursor: None,
            bright_foreground: None,
            dim_foreground: None,
            normal: ColorSchemeAnsi {
                black: Some(rgb(0x21222C)),
                red: Some(rgb(0xFF5555)),
                green: Some(rgb(0x50FA7B)),
                yellow: Some(rgb(0xF1FA8C)),
                blue: Some(rgb(0xBD93F9)),
                magenta: Some(rgb(0xFF79C6)),
                cyan: Some(rgb(0x8BE9FD)),
                white: Some(rgb(0xF8F8F2)),
            },
            bright: ColorSchemeAnsi {
                black: Some(rgb(0x6272A4)),
                red: Some(rgb(0xFF6E6E)),
                green: Some(rgb(0x69FF94)),
                yellow: Some(rgb(0xFFFFA5)),
                blue: Some(rgb(0xD6ACFF)),
                magenta: Some(rgb(0xFF92DF)),
                cyan: Some(rgb(0xA4FFFF)),
                white: Some(rgb(0xFFFFFF)),
            },
            dim: ColorSchemeAnsi::default(),
        }
    }

    #[test]
    fn conversion_fills_the_named_slots_from_the_palette() {
        let colors = Colors::from(&plain_scheme());
        assert_eq!(colors[NamedColor::Red], Some(rgb(0xFF5555)));
        assert_eq!(colors[NamedColor::BrightBlue], Some(rgb(0xD6ACFF)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xF8F8F2)));
        // A scheme-owned background reaches the slot (the engine only
        // falls back to WINDOW_BG_COLOR while it is None).
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x282A36)));
    }

    #[test]
    fn conversion_derives_missing_dims_from_normals_at_the_alacritty_factor() {
        // Alacritty derives dims as `normal * 0.66` per channel
        // (DIM_FACTOR, docs/research/terminal-theming.md §2); the cast
        // truncates. 0xFF * 0.66 = 168.3 -> 168 = 0xA8.
        let colors = Colors::from(&plain_scheme());
        assert_eq!(colors[NamedColor::DimRed], Some(rgb(0xA83838)));
        assert_eq!(colors[NamedColor::DimWhite], Some(rgb(0xA3A39F)));
    }

    #[test]
    fn conversion_defaults_cursor_and_foregrounds_to_the_foreground() {
        let colors = Colors::from(&plain_scheme());
        assert_eq!(colors[NamedColor::Cursor], Some(rgb(0xF8F8F2)));
        assert_eq!(colors[NamedColor::BrightForeground], Some(rgb(0xF8F8F2)));
        // Dim foreground defaults to foreground * 0.66:
        // 0xF8 * 0.66 = 163.68 -> 163 = 0xA3, 0xF2 * 0.66 = 159.7 -> 0x9F.
        assert_eq!(colors[NamedColor::DimForeground], Some(rgb(0xA3A39F)));
    }

    #[test]
    fn conversion_keeps_explicit_dims_and_specials_over_derived_defaults() {
        // The COSMIC pair ships hand-tuned dim tables; derivation must
        // never overwrite a value the scheme actually provides.
        let mut scheme = plain_scheme();
        scheme.dim.red = Some(rgb(0xA04040));
        scheme.cursor = Some(rgb(0xFFCC00));
        scheme.bright_foreground = Some(rgb(0xFFFFFF));
        scheme.dim_foreground = Some(rgb(0x898989));
        let colors = Colors::from(&scheme);
        assert_eq!(colors[NamedColor::DimRed], Some(rgb(0xA04040)));
        assert_eq!(colors[NamedColor::Cursor], Some(rgb(0xFFCC00)));
        assert_eq!(colors[NamedColor::BrightForeground], Some(rgb(0xFFFFFF)));
        assert_eq!(colors[NamedColor::DimForeground], Some(rgb(0x898989)));
        // Unrelated slots still derive.
        assert_eq!(colors[NamedColor::DimGreen], Some(rgb(0x34A551)));
    }

    // --- byte-identity pins: the COSMIC pair's user-visible slots,
    // literal from the pre-refactor construction. The reroute through
    // `ColorScheme` must not move a single one. ---

    #[test]
    fn cosmic_dark_normal_and_bright_slots_are_pinned() {
        let colors = cosmic_dark();
        assert_eq!(colors[NamedColor::Black], Some(rgb(0x1B1B1B)));
        assert_eq!(colors[NamedColor::Red], Some(rgb(0xF16161)));
        assert_eq!(colors[NamedColor::Green], Some(rgb(0x7CB987)));
        assert_eq!(colors[NamedColor::Yellow], Some(rgb(0xDDC74C)));
        assert_eq!(colors[NamedColor::Blue], Some(rgb(0x6296BE)));
        assert_eq!(colors[NamedColor::Magenta], Some(rgb(0xBE6DEE)));
        assert_eq!(colors[NamedColor::Cyan], Some(rgb(0x49BAC8)));
        assert_eq!(colors[NamedColor::White], Some(rgb(0xBEBEBE)));
        assert_eq!(colors[NamedColor::BrightBlack], Some(rgb(0x808080)));
        assert_eq!(colors[NamedColor::BrightRed], Some(rgb(0xFF8985)));
        assert_eq!(colors[NamedColor::BrightGreen], Some(rgb(0x97D5A0)));
        assert_eq!(colors[NamedColor::BrightYellow], Some(rgb(0xFAE365)));
        assert_eq!(colors[NamedColor::BrightBlue], Some(rgb(0x7DB1DA)));
        assert_eq!(colors[NamedColor::BrightMagenta], Some(rgb(0xD68EFF)));
        assert_eq!(colors[NamedColor::BrightCyan], Some(rgb(0x49BAC8)));
        assert_eq!(colors[NamedColor::BrightWhite], Some(rgb(0xC4C4C4)));
    }

    #[test]
    fn cosmic_dark_dim_and_special_slots_are_pinned() {
        let colors = cosmic_dark();
        assert_eq!(colors[NamedColor::DimBlack], Some(rgb(0x000000)));
        assert_eq!(colors[NamedColor::DimRed], Some(rgb(0xA04040)));
        assert_eq!(colors[NamedColor::DimGreen], Some(rgb(0x5D7D62)));
        assert_eq!(colors[NamedColor::DimYellow], Some(rgb(0x9E914A)));
        assert_eq!(colors[NamedColor::DimBlue], Some(rgb(0x486073)));
        assert_eq!(colors[NamedColor::DimMagenta], Some(rgb(0x7F46A1)));
        assert_eq!(colors[NamedColor::DimCyan], Some(rgb(0x3F7F87)));
        assert_eq!(colors[NamedColor::DimWhite], Some(rgb(0x898989)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xC4C4C4)));
        assert_eq!(colors[NamedColor::Cursor], Some(rgb(0xC4C4C4)));
        assert_eq!(colors[NamedColor::BrightForeground], Some(rgb(0xC4C4C4)));
        assert_eq!(colors[NamedColor::DimForeground], Some(rgb(0x898989)));
    }

    #[test]
    fn cosmic_light_normal_and_bright_slots_are_pinned() {
        let colors = cosmic_light();
        assert_eq!(colors[NamedColor::Black], Some(rgb(0x1B1B1B)));
        assert_eq!(colors[NamedColor::Red], Some(rgb(0xA24F50)));
        assert_eq!(colors[NamedColor::Green], Some(rgb(0x437E4F)));
        assert_eq!(colors[NamedColor::Yellow], Some(rgb(0x7D6E1E)));
        assert_eq!(colors[NamedColor::Blue], Some(rgb(0x516D94)));
        assert_eq!(colors[NamedColor::Magenta], Some(rgb(0x9A30CA)));
        assert_eq!(colors[NamedColor::Cyan], Some(rgb(0x227A85)));
        assert_eq!(colors[NamedColor::White], Some(rgb(0xD7D7D7)));
        assert_eq!(colors[NamedColor::BrightBlack], Some(rgb(0x000000)));
        assert_eq!(colors[NamedColor::BrightRed], Some(rgb(0x890418)));
        assert_eq!(colors[NamedColor::BrightGreen], Some(rgb(0x185529)));
        assert_eq!(colors[NamedColor::BrightYellow], Some(rgb(0x534800)));
        assert_eq!(colors[NamedColor::BrightBlue], Some(rgb(0x2E496D)));
        assert_eq!(colors[NamedColor::BrightMagenta], Some(rgb(0x6B0091)));
        assert_eq!(colors[NamedColor::BrightCyan], Some(rgb(0x00525A)));
        assert_eq!(colors[NamedColor::BrightWhite], Some(rgb(0xE8E8E8)));
    }

    #[test]
    fn cosmic_light_dim_and_special_slots_are_pinned() {
        let colors = cosmic_light();
        assert_eq!(colors[NamedColor::DimBlack], Some(rgb(0x262626)));
        assert_eq!(colors[NamedColor::DimRed], Some(rgb(0xB25D5E)));
        assert_eq!(colors[NamedColor::DimGreen], Some(rgb(0x528D5E)));
        assert_eq!(colors[NamedColor::DimYellow], Some(rgb(0x8C7D30)));
        assert_eq!(colors[NamedColor::DimBlue], Some(rgb(0x5F7CA4)));
        assert_eq!(colors[NamedColor::DimMagenta], Some(rgb(0xAA43DB)));
        assert_eq!(colors[NamedColor::DimCyan], Some(rgb(0x358994)));
        assert_eq!(colors[NamedColor::DimWhite], Some(rgb(0xC4C4C4)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0x1B1B1B)));
        assert_eq!(colors[NamedColor::Cursor], Some(rgb(0x1B1B1B)));
        assert_eq!(colors[NamedColor::BrightForeground], Some(rgb(0x000000)));
        assert_eq!(colors[NamedColor::DimForeground], Some(rgb(0x262626)));
    }

    #[test]
    fn cosmic_schemes_leave_background_unset_and_share_the_color_cube() {
        // The COSMIC pair defers the background to the system theme
        // (WINDOW_BG_COLOR fallback); every other scheme owns its bg.
        for colors in [cosmic_dark(), cosmic_light()] {
            assert_eq!(colors[NamedColor::Background], None);
        }
        // Slots 16..=255 come from the shared ransid cube/ramp —
        // identical across schemes by construction.
        let dark = cosmic_dark();
        let base = auto_colors();
        for slot in 16..256 {
            assert_eq!(dark[slot], base[slot], "cube slot {slot} drifted");
        }
    }

    #[test]
    fn by_name_resolves_the_cosmic_pair_and_rejects_unknown_names() {
        assert!(by_name(COSMIC_THEME_DARK).is_some());
        assert!(by_name(COSMIC_THEME_LIGHT).is_some());
        // Unknown names are `active_colors()`'s fallback signal, not an
        // error (spec criterion 4).
        assert!(by_name("Dracula").is_none());
        assert!(by_name("").is_none());
    }
}
