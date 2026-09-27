// SPDX-License-Identifier: GPL-3.0-only
// Copyright 2023 System76 <info@system76.com>
//
// Adapted from cosmic-term (https://github.com/pop-os/cosmic-term)
// src/terminal_theme.rs at commit 9129277. The `auto_colors`,
// `cosmic_dark` and `cosmic_light` palettes are unmodified; the
// `ColorScheme`/`ron`-file conversion and export machinery (which
// supported user-editable color-scheme files and a whole profile
// system) has been dropped in favor of a plain by-name lookup over
// the two built-in COSMIC schemes.

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

pub fn cosmic_dark() -> Colors {
    let mut colors = auto_colors();

    let encode_rgb = |data: u32| -> Rgb {
        Rgb {
            r: (data >> 16) as u8,
            g: (data >> 8) as u8,
            b: data as u8,
        }
    };

    colors[NamedColor::Black] = Some(encode_rgb(0x1B1B1B));
    colors[NamedColor::Red] = Some(encode_rgb(0xF16161));
    colors[NamedColor::Green] = Some(encode_rgb(0x7CB987));
    colors[NamedColor::Yellow] = Some(encode_rgb(0xDDC74C));
    colors[NamedColor::Blue] = Some(encode_rgb(0x6296BE));
    colors[NamedColor::Magenta] = Some(encode_rgb(0xBE6DEE));
    colors[NamedColor::Cyan] = Some(encode_rgb(0x49BAC8));
    colors[NamedColor::White] = Some(encode_rgb(0xBEBEBE));

    colors[NamedColor::BrightBlack] = Some(encode_rgb(0x808080));
    colors[NamedColor::BrightRed] = Some(encode_rgb(0xFF8985));
    colors[NamedColor::BrightGreen] = Some(encode_rgb(0x97D5A0));
    colors[NamedColor::BrightYellow] = Some(encode_rgb(0xFAE365));
    colors[NamedColor::BrightBlue] = Some(encode_rgb(0x7DB1DA));
    colors[NamedColor::BrightMagenta] = Some(encode_rgb(0xD68EFF));
    colors[NamedColor::BrightCyan] = Some(encode_rgb(0x49BAC8));
    colors[NamedColor::BrightWhite] = Some(encode_rgb(0xC4C4C4));

    colors[NamedColor::DimBlack] = Some(encode_rgb(0x000000));
    colors[NamedColor::DimRed] = Some(encode_rgb(0xA04040));
    colors[NamedColor::DimGreen] = Some(encode_rgb(0x5D7D62));
    colors[NamedColor::DimYellow] = Some(encode_rgb(0x9E914A));
    colors[NamedColor::DimBlue] = Some(encode_rgb(0x486073));
    colors[NamedColor::DimMagenta] = Some(encode_rgb(0x7F46A1));
    colors[NamedColor::DimCyan] = Some(encode_rgb(0x3F7F87));
    colors[NamedColor::DimWhite] = Some(encode_rgb(0x898989));

    // Set special colors
    colors[NamedColor::Foreground] = colors[NamedColor::BrightWhite];
    // Background comes from theme settings: colors[NamedColor::Background] = colors[NamedColor::Black];
    colors[NamedColor::Cursor] = colors[NamedColor::BrightWhite];
    colors[NamedColor::BrightForeground] = colors[NamedColor::BrightWhite];
    colors[NamedColor::DimForeground] = colors[NamedColor::DimWhite];

    colors
}

pub fn cosmic_light() -> Colors {
    let mut colors = auto_colors();

    let encode_rgb = |data: u32| -> Rgb {
        Rgb {
            r: (data >> 16) as u8,
            g: (data >> 8) as u8,
            b: data as u8,
        }
    };

    colors[NamedColor::Black] = Some(encode_rgb(0x1B1B1B));
    colors[NamedColor::Red] = Some(encode_rgb(0xA24F50));
    colors[NamedColor::Green] = Some(encode_rgb(0x437E4F));
    colors[NamedColor::Yellow] = Some(encode_rgb(0x7D6E1E));
    colors[NamedColor::Blue] = Some(encode_rgb(0x516D94));
    colors[NamedColor::Magenta] = Some(encode_rgb(0x9A30CA));
    colors[NamedColor::Cyan] = Some(encode_rgb(0x227A85));
    colors[NamedColor::White] = Some(encode_rgb(0xD7D7D7));

    colors[NamedColor::BrightBlack] = Some(encode_rgb(0x000000));
    colors[NamedColor::BrightRed] = Some(encode_rgb(0x890418));
    colors[NamedColor::BrightGreen] = Some(encode_rgb(0x185529));
    colors[NamedColor::BrightYellow] = Some(encode_rgb(0x534800));
    colors[NamedColor::BrightBlue] = Some(encode_rgb(0x2E496D));
    colors[NamedColor::BrightMagenta] = Some(encode_rgb(0x6B0091));
    colors[NamedColor::BrightCyan] = Some(encode_rgb(0x00525A));
    colors[NamedColor::BrightWhite] = Some(encode_rgb(0xE8E8E8));

    colors[NamedColor::DimBlack] = Some(encode_rgb(0x262626));
    colors[NamedColor::DimRed] = Some(encode_rgb(0xB25D5E));
    colors[NamedColor::DimGreen] = Some(encode_rgb(0x528D5E));
    colors[NamedColor::DimYellow] = Some(encode_rgb(0x8C7D30));
    colors[NamedColor::DimBlue] = Some(encode_rgb(0x5F7CA4));
    colors[NamedColor::DimMagenta] = Some(encode_rgb(0xAA43DB));
    colors[NamedColor::DimCyan] = Some(encode_rgb(0x358994));
    colors[NamedColor::DimWhite] = Some(encode_rgb(0xC4C4C4));

    // Set special colors
    colors[NamedColor::Foreground] = colors[NamedColor::Black];
    // Background comes from theme settings: colors[NamedColor::Background] = colors[NamedColor::BrightWhite];
    colors[NamedColor::Cursor] = colors[NamedColor::Black];
    colors[NamedColor::BrightForeground] = colors[NamedColor::BrightBlack];
    colors[NamedColor::DimForeground] = colors[NamedColor::DimBlack];

    colors
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
