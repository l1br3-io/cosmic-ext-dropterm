// SPDX-License-Identifier: GPL-3.0-only
// Copyright 2023 System76 <info@system76.com>
//
// Adapted from cosmic-term
// (<https://github.com/pop-os/cosmic-term>) src/terminal_theme.rs and
// src/config.rs at commit 9129277 (palettes) and `3ef31d7` (the
// `ColorScheme` representation). The palettes are unmodified;
// cosmic-term's serde/RON-file, per-profile, and import/export
// machinery stays dropped — schemes compile in and resolve by name
// (see docs/research/terminal-theming.md). The ten third-party
// palettes are transcribed from alacritty-theme TOMLs and canonical
// extras, each with its source and license recorded on the const
// below; the COSMIC pair keeps its hand-tuned tables.

use alacritty_terminal::{term::color::Colors, vte::ansi::NamedColor};

// Re-exported because [`SchemePreview`]'s slots name it — the chip
// renderers beyond this module must be able to spell it.
pub use alacritty_terminal::vte::ansi::Rgb;

use crate::config::{COSMIC_THEME_DARK, COSMIC_THEME_LIGHT};

fn auto_colors() -> Colors {
    let mut colors = Colors::default();

    // These colors come from `ransid`: <https://gitlab.redox-os.org/redox-os/ransid/-/blob/master/src/color.rs>
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

// --- scheme representation ---
//
// Adapted from cosmic-term's `ColorScheme`/`ColorSchemeAnsi`
// (src/config.rs @ `3ef31d7`, GPL-3.0-only like DropTerm): same field
// shape, colors as plain `Option<Rgb>` and no serde — schemes compile
// in, nothing parses. User scheme files (ROADMAP "Later") can grow
// serde on this shape additively.

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

impl ColorSchemeAnsi {
    /// Every slot unset — the dim table base for palettes (the ten
    /// third-party ones) whose dims derive at conversion.
    const EMPTY: Self = Self {
        black: None,
        red: None,
        green: None,
        yellow: None,
        blue: None,
        magenta: None,
        cyan: None,
        white: None,
    };
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

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Alacritty's dim contract truncates normal x 0.66; a u8 source stays in [0, 168], never negative"
)]
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
        // dim foreground derives from it.
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
#[expect(
    clippy::cast_possible_truncation,
    reason = "low-byte extraction is the 0xRRGGBB decode contract"
)]
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
    foreground: Some(rgb(0xC4_C4C4)),
    background: None,
    cursor: Some(rgb(0xC4_C4C4)),
    bright_foreground: Some(rgb(0xC4_C4C4)),
    dim_foreground: Some(rgb(0x89_8989)),
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x1B_1B1B)),
        red: Some(rgb(0xF1_6161)),
        green: Some(rgb(0x7C_B987)),
        yellow: Some(rgb(0xDD_C74C)),
        blue: Some(rgb(0x62_96BE)),
        magenta: Some(rgb(0xBE_6DEE)),
        cyan: Some(rgb(0x49_BAC8)),
        white: Some(rgb(0xBE_BEBE)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x80_8080)),
        red: Some(rgb(0xFF_8985)),
        green: Some(rgb(0x97_D5A0)),
        yellow: Some(rgb(0xFA_E365)),
        blue: Some(rgb(0x7D_B1DA)),
        magenta: Some(rgb(0xD6_8EFF)),
        cyan: Some(rgb(0x49_BAC8)),
        white: Some(rgb(0xC4_C4C4)),
    },
    dim: ColorSchemeAnsi {
        black: Some(rgb(0x00_0000)),
        red: Some(rgb(0xA0_4040)),
        green: Some(rgb(0x5D_7D62)),
        yellow: Some(rgb(0x9E_914A)),
        blue: Some(rgb(0x48_6073)),
        magenta: Some(rgb(0x7F_46A1)),
        cyan: Some(rgb(0x3F_7F87)),
        white: Some(rgb(0x89_8989)),
    },
};

/// The COSMIC Dark scheme as alacritty colors.
pub fn cosmic_dark() -> Colors {
    Colors::from(&COSMIC_DARK)
}

/// The COSMIC Light palette — same traits as [`COSMIC_DARK`]:
/// hand-tuned dims, no background.
const COSMIC_LIGHT: ColorScheme = ColorScheme {
    foreground: Some(rgb(0x1B_1B1B)),
    background: None,
    cursor: Some(rgb(0x1B_1B1B)),
    bright_foreground: Some(rgb(0x00_0000)),
    dim_foreground: Some(rgb(0x26_2626)),
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x1B_1B1B)),
        red: Some(rgb(0xA2_4F50)),
        green: Some(rgb(0x43_7E4F)),
        yellow: Some(rgb(0x7D_6E1E)),
        blue: Some(rgb(0x51_6D94)),
        magenta: Some(rgb(0x9A_30CA)),
        cyan: Some(rgb(0x22_7A85)),
        white: Some(rgb(0xD7_D7D7)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x00_0000)),
        red: Some(rgb(0x89_0418)),
        green: Some(rgb(0x18_5529)),
        yellow: Some(rgb(0x53_4800)),
        blue: Some(rgb(0x2E_496D)),
        magenta: Some(rgb(0x6B_0091)),
        cyan: Some(rgb(0x00_525A)),
        white: Some(rgb(0xE8_E8E8)),
    },
    dim: ColorSchemeAnsi {
        black: Some(rgb(0x26_2626)),
        red: Some(rgb(0xB2_5D5E)),
        green: Some(rgb(0x52_8D5E)),
        yellow: Some(rgb(0x8C_7D30)),
        blue: Some(rgb(0x5F_7CA4)),
        magenta: Some(rgb(0xAA_43DB)),
        cyan: Some(rgb(0x35_8994)),
        white: Some(rgb(0xC4_C4C4)),
    },
};

/// The COSMIC Light scheme as alacritty colors.
pub fn cosmic_light() -> Colors {
    Colors::from(&COSMIC_LIGHT)
}

// --- third-party palettes ---
//
// Transcription rule: each palette carries exactly what its theme
// file publishes as fg, bg, the 16 ANSI colors, and brights. Dims,
// cursor, and dim/bright foregrounds derive uniformly at conversion —
// no per-scheme tables here.

/// Catppuccin Latte — `themes/catppuccin_latte.toml` in
/// [alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme)
/// (repo Apache-2.0), contributed by the Catppuccin team; design MIT
/// (catppuccin.com). Cross-checked against the canonical palette:
/// base `#eff1f5`, text `#4c4f69`, red `#d20f39`, teal `#179299` all
/// match. The TOML also sets cursor/dim fields; per the uniform
/// derivation rule we transcribe fg/bg/16/brights only.
const CATPPUCCIN_LATTE: ColorScheme = ColorScheme {
    foreground: Some(rgb(0x4C_4F69)),
    background: Some(rgb(0xEF_F1F5)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x5C_5F77)),
        red: Some(rgb(0xD2_0F39)),
        green: Some(rgb(0x40_A02B)),
        yellow: Some(rgb(0xDF_8E1D)),
        blue: Some(rgb(0x1E_66F5)),
        magenta: Some(rgb(0xEA_76CB)),
        cyan: Some(rgb(0x17_9299)),
        white: Some(rgb(0xAC_B0BE)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x6C_6F85)),
        red: Some(rgb(0xD2_0F39)),
        green: Some(rgb(0x40_A02B)),
        yellow: Some(rgb(0xDF_8E1D)),
        blue: Some(rgb(0x1E_66F5)),
        magenta: Some(rgb(0xEA_76CB)),
        cyan: Some(rgb(0x17_9299)),
        white: Some(rgb(0xBC_C0CC)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

/// Catppuccin Mocha — `themes/catppuccin_mocha.toml` in
/// [alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme)
/// (repo Apache-2.0), contributed by the Catppuccin team; design MIT
/// (catppuccin.com). Cross-checked against the canonical palette:
/// base `#1e1e2e`, text `#cdd6f4`, blue `#89b4fa`, pink `#f5c2e7` all
/// match. The TOML also sets cursor/dim fields; per the uniform
/// derivation rule we transcribe fg/bg/16/brights only.
const CATPPUCCIN_MOCHA: ColorScheme = ColorScheme {
    foreground: Some(rgb(0xCD_D6F4)),
    background: Some(rgb(0x1E_1E2E)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x45_475A)),
        red: Some(rgb(0xF3_8BA8)),
        green: Some(rgb(0xA6_E3A1)),
        yellow: Some(rgb(0xF9_E2AF)),
        blue: Some(rgb(0x89_B4FA)),
        magenta: Some(rgb(0xF5_C2E7)),
        cyan: Some(rgb(0x94_E2D5)),
        white: Some(rgb(0xBA_C2DE)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x58_5B70)),
        red: Some(rgb(0xF3_8BA8)),
        green: Some(rgb(0xA6_E3A1)),
        yellow: Some(rgb(0xF9_E2AF)),
        blue: Some(rgb(0x89_B4FA)),
        magenta: Some(rgb(0xF5_C2E7)),
        cyan: Some(rgb(0x94_E2D5)),
        white: Some(rgb(0xA6_ADC8)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

/// Dracula — `themes/dracula.toml` in
/// [alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme)
/// (repo Apache-2.0); design MIT (draculatheme.com). Cross-checked
/// against the canonical palette: bg `#282a36`, fg `#f8f8f2`, purple
/// `#bd93f9`, pink `#ff79c6` all match — cosmic-term's own Dracula
/// copy (`#1e1f29` bg) diverges and loses to canonical
/// (docs/research/terminal-theming.md §Licensing).
const DRACULA: ColorScheme = ColorScheme {
    foreground: Some(rgb(0xF8_F8F2)),
    background: Some(rgb(0x28_2A36)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x00_0000)),
        red: Some(rgb(0xFF_5555)),
        green: Some(rgb(0x50_FA7B)),
        yellow: Some(rgb(0xF1_FA8C)),
        blue: Some(rgb(0xBD_93F9)),
        magenta: Some(rgb(0xFF_79C6)),
        cyan: Some(rgb(0x8B_E9FD)),
        white: Some(rgb(0xBB_BBBB)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x55_5555)),
        red: Some(rgb(0xFF_5555)),
        green: Some(rgb(0x50_FA7B)),
        yellow: Some(rgb(0xF1_FA8C)),
        blue: Some(rgb(0xCA_A9FA)),
        magenta: Some(rgb(0xFF_79C6)),
        cyan: Some(rgb(0x8B_E9FD)),
        white: Some(rgb(0xFF_FFFF)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

/// Gruvbox Dark — `themes/gruvbox_dark.toml` in
/// [alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme)
/// (repo Apache-2.0); design MIT (morhetz/gruvbox). Cross-checked
/// against the canonical medium-contrast palette: bg `#282828`, fg
/// `#ebdbb2`, red `#cc241d`/bright `#fb4934` all match (the TOML's
/// hard/soft contrast variants stay untranscribed).
const GRUVBOX_DARK: ColorScheme = ColorScheme {
    foreground: Some(rgb(0xEB_DBB2)),
    background: Some(rgb(0x28_2828)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x28_2828)),
        red: Some(rgb(0xCC_241D)),
        green: Some(rgb(0x98_971A)),
        yellow: Some(rgb(0xD7_9921)),
        blue: Some(rgb(0x45_8588)),
        magenta: Some(rgb(0xB1_6286)),
        cyan: Some(rgb(0x68_9D6A)),
        white: Some(rgb(0xA8_9984)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x92_8374)),
        red: Some(rgb(0xFB_4934)),
        green: Some(rgb(0xB8_BB26)),
        yellow: Some(rgb(0xFA_BD2F)),
        blue: Some(rgb(0x83_A598)),
        magenta: Some(rgb(0xD3_869B)),
        cyan: Some(rgb(0x8E_C07C)),
        white: Some(rgb(0xEB_DBB2)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

/// Gruvbox Light — `themes/gruvbox_light.toml` in
/// [alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme)
/// (repo Apache-2.0); design MIT (morhetz/gruvbox). Cross-checked
/// against the canonical medium-contrast palette: bg `#fbf1c7`, fg
/// `#3c3836`, bright yellow `#b57614` all match.
const GRUVBOX_LIGHT: ColorScheme = ColorScheme {
    foreground: Some(rgb(0x3C_3836)),
    background: Some(rgb(0xFB_F1C7)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0xFB_F1C7)),
        red: Some(rgb(0xCC_241D)),
        green: Some(rgb(0x98_971A)),
        yellow: Some(rgb(0xD7_9921)),
        blue: Some(rgb(0x45_8588)),
        magenta: Some(rgb(0xB1_6286)),
        cyan: Some(rgb(0x68_9D6A)),
        white: Some(rgb(0x7C_6F64)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x92_8374)),
        red: Some(rgb(0x9D_0006)),
        green: Some(rgb(0x79_740E)),
        yellow: Some(rgb(0xB5_7614)),
        blue: Some(rgb(0x07_6678)),
        magenta: Some(rgb(0x8F_3F71)),
        cyan: Some(rgb(0x42_7B58)),
        white: Some(rgb(0x3C_3836)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

/// Nord — `themes/nord.toml` in
/// [alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme)
/// (repo Apache-2.0); design MIT (nordtheme). Cross-checked against
/// the canonical Nord palette: every value is a canonical color — bg
/// nord0 `#2e3440`, fg nord4 `#d8dee9`, cyan nord8 `#88c0d0`, blue
/// nord9 `#81a1c1` all match.
const NORD: ColorScheme = ColorScheme {
    foreground: Some(rgb(0xD8_DEE9)),
    background: Some(rgb(0x2E_3440)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x3B_4252)),
        red: Some(rgb(0xBF_616A)),
        green: Some(rgb(0xA3_BE8C)),
        yellow: Some(rgb(0xEB_CB8B)),
        blue: Some(rgb(0x81_A1C1)),
        magenta: Some(rgb(0xB4_8EAD)),
        cyan: Some(rgb(0x88_C0D0)),
        white: Some(rgb(0xE5_E9F0)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x4C_566A)),
        red: Some(rgb(0xBF_616A)),
        green: Some(rgb(0xA3_BE8C)),
        yellow: Some(rgb(0xEB_CB8B)),
        blue: Some(rgb(0x81_A1C1)),
        magenta: Some(rgb(0xB4_8EAD)),
        cyan: Some(rgb(0x8F_BCBB)),
        white: Some(rgb(0xEC_EFF4)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

/// Solarized Dark — `themes/solarized_dark.toml` in
/// [alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme)
/// (repo Apache-2.0); design MIT (Ethan Schoonover, solarized).
/// Cross-checked against the canonical palette: bg base03 `#002b36`,
/// fg base0 `#839496`, cyan `#2aa198`, orange `#cb4b16` all match.
const SOLARIZED_DARK: ColorScheme = ColorScheme {
    foreground: Some(rgb(0x83_9496)),
    background: Some(rgb(0x00_2B36)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x07_3642)),
        red: Some(rgb(0xDC_322F)),
        green: Some(rgb(0x85_9900)),
        yellow: Some(rgb(0xB5_8900)),
        blue: Some(rgb(0x26_8BD2)),
        magenta: Some(rgb(0xD3_3682)),
        cyan: Some(rgb(0x2A_A198)),
        white: Some(rgb(0xEE_E8D5)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x00_2B36)),
        red: Some(rgb(0xCB_4B16)),
        green: Some(rgb(0x58_6E75)),
        yellow: Some(rgb(0x65_7B83)),
        blue: Some(rgb(0x83_9496)),
        magenta: Some(rgb(0x6C_71C4)),
        cyan: Some(rgb(0x93_A1A1)),
        white: Some(rgb(0xFD_F6E3)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

/// Solarized Light — `themes/solarized_light.toml` in
/// [alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme)
/// (repo Apache-2.0); design MIT (Ethan Schoonover, solarized).
/// Cross-checked against the canonical palette: bg base3 `#fdf6e3`,
/// fg base01 `#586e75` all match; the 16 ANSI values are shared with
/// Solarized Dark (base02–base1 and the accents are mode-invariant).
const SOLARIZED_LIGHT: ColorScheme = ColorScheme {
    foreground: Some(rgb(0x58_6E75)),
    background: Some(rgb(0xFD_F6E3)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x07_3642)),
        red: Some(rgb(0xDC_322F)),
        green: Some(rgb(0x85_9900)),
        yellow: Some(rgb(0xB5_8900)),
        blue: Some(rgb(0x26_8BD2)),
        magenta: Some(rgb(0xD3_3682)),
        cyan: Some(rgb(0x2A_A198)),
        white: Some(rgb(0xEE_E8D5)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x00_2B36)),
        red: Some(rgb(0xCB_4B16)),
        green: Some(rgb(0x58_6E75)),
        yellow: Some(rgb(0x65_7B83)),
        blue: Some(rgb(0x83_9496)),
        magenta: Some(rgb(0x6C_71C4)),
        cyan: Some(rgb(0x93_A1A1)),
        white: Some(rgb(0xFD_F6E3)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

/// Tokyo Night Day — alacritty extra shipped by the theme's own
/// upstream, [folke/tokyonight.nvim](https://github.com/folke/tokyonight.nvim)
/// `extras/alacritty/tokyonight_day.toml` (Apache-2.0); alacritty-theme
/// has no Day file. Cross-checked against the canonical palette: bg
/// `#e1e2e7`, fg `#3760bf`, cyan `#007197` all match.
const TOKYO_NIGHT_DAY: ColorScheme = ColorScheme {
    foreground: Some(rgb(0x37_60BF)),
    background: Some(rgb(0xE1_E2E7)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0xB4_B5B9)),
        red: Some(rgb(0xF5_2A65)),
        green: Some(rgb(0x58_7539)),
        yellow: Some(rgb(0x8C_6C3E)),
        blue: Some(rgb(0x2E_7DE9)),
        magenta: Some(rgb(0x98_54F1)),
        cyan: Some(rgb(0x00_7197)),
        white: Some(rgb(0x61_72B0)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0xA1_A6C5)),
        red: Some(rgb(0xFF_4774)),
        green: Some(rgb(0x5C_8524)),
        yellow: Some(rgb(0xA2_7629)),
        blue: Some(rgb(0x35_8AFF)),
        magenta: Some(rgb(0xA4_63FF)),
        cyan: Some(rgb(0x00_7EA8)),
        white: Some(rgb(0x37_60BF)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

/// Tokyo Night Storm — `themes/tokyo_night_storm.toml` in
/// [alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme)
/// (repo Apache-2.0), from zatchheems' port; design Apache-2.0
/// (folke/tokyonight.nvim). Cross-checked against the canonical storm
/// palette: bg `#24283b`, fg `#a9b1d6`, blue `#7aa2f7`, red `#f7768e`
/// all match (the port's cyan `#449dab` is its teal choice).
const TOKYO_NIGHT_STORM: ColorScheme = ColorScheme {
    foreground: Some(rgb(0xA9_B1D6)),
    background: Some(rgb(0x24_283B)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x32_344A)),
        red: Some(rgb(0xF7_768E)),
        green: Some(rgb(0x9E_CE6A)),
        yellow: Some(rgb(0xE0_AF68)),
        blue: Some(rgb(0x7A_A2F7)),
        magenta: Some(rgb(0xAD_8EE6)),
        cyan: Some(rgb(0x44_9DAB)),
        white: Some(rgb(0x96_99A8)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x44_4B6A)),
        red: Some(rgb(0xFF_7A93)),
        green: Some(rgb(0xB9_F27C)),
        yellow: Some(rgb(0xFF_9E64)),
        blue: Some(rgb(0x7D_A6FF)),
        magenta: Some(rgb(0xBB_9AF7)),
        cyan: Some(rgb(0x0D_B9D7)),
        white: Some(rgb(0xAC_B0D0)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

// --- registry ---

/// One registry entry: a scheme's display name — identity, persisted
/// under `color_scheme_dark`/`_light`, and shown in the dropdowns —
/// and its palette.
struct SchemeEntry {
    name: &'static str,
    scheme: ColorScheme,
}

/// The built-in scheme registry: the single definition site the
/// `by_name` lookup, the settings dropdowns, and the tests all read,
/// so names, options, and lookup cannot drift. COSMIC Dark/Light
/// first (the defaults), the rest alphabetical.
const SCHEMES: [SchemeEntry; 12] = [
    SchemeEntry {
        name: COSMIC_THEME_DARK,
        scheme: COSMIC_DARK,
    },
    SchemeEntry {
        name: COSMIC_THEME_LIGHT,
        scheme: COSMIC_LIGHT,
    },
    SchemeEntry {
        name: "Catppuccin Latte",
        scheme: CATPPUCCIN_LATTE,
    },
    SchemeEntry {
        name: "Catppuccin Mocha",
        scheme: CATPPUCCIN_MOCHA,
    },
    SchemeEntry {
        name: "Dracula",
        scheme: DRACULA,
    },
    SchemeEntry {
        name: "Gruvbox Dark",
        scheme: GRUVBOX_DARK,
    },
    SchemeEntry {
        name: "Gruvbox Light",
        scheme: GRUVBOX_LIGHT,
    },
    SchemeEntry {
        name: "Nord",
        scheme: NORD,
    },
    SchemeEntry {
        name: "Solarized Dark",
        scheme: SOLARIZED_DARK,
    },
    SchemeEntry {
        name: "Solarized Light",
        scheme: SOLARIZED_LIGHT,
    },
    SchemeEntry {
        name: "Tokyo Night Day",
        scheme: TOKYO_NIGHT_DAY,
    },
    SchemeEntry {
        name: "Tokyo Night Storm",
        scheme: TOKYO_NIGHT_STORM,
    },
];

/// Look up a built-in color scheme by name (its display name is the
/// on-disk config value). Unknown names return `None` — the caller
/// (`active_colors()`) falls back to the COSMIC pair for the active
/// mode; no error surface.
pub fn by_name(name: &str) -> Option<Colors> {
    SCHEMES
        .iter()
        .find(|entry| entry.name == name)
        .map(|entry| Colors::from(&entry.scheme))
}

/// The palette preview a UI renders for one scheme: the scheme's own
/// background (when it has one) and its six normal ANSI accents in
/// `ls`-colors order (red, green, yellow, blue, magenta, cyan). The
/// profile list's swatch chip reads this; it is a *preview* seam, not
/// a spawn seam — it never feeds [`by_name`]'s resolution chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemeSwatch {
    /// The scheme's background, or `None` for the COSMIC pair (their
    /// background follows the system theme — a preview may not invent
    /// one).
    pub background: Option<Rgb>,
    /// The accents the chip paints as cells, in order. Unset normal
    /// slots drop out rather than error: the registry's hole-free pin
    /// makes that unreachable today, but a future palette edit
    /// degrades to a shorter chip, never a panic.
    pub accents: [Option<Rgb>; 6],
}

/// The [`SchemeSwatch`] for a registry name. Unknown names — the same
/// names [`by_name`] rejects — return `None`; callers render their
/// "follows global" preview for both that and an unset scheme.
pub fn scheme_swatch(name: &str) -> Option<SchemeSwatch> {
    let entry = SCHEMES.iter().find(|entry| entry.name == name)?;
    let normal = &entry.scheme.normal;
    Some(SchemeSwatch {
        background: entry.scheme.background,
        accents: [
            normal.red,
            normal.green,
            normal.yellow,
            normal.blue,
            normal.magenta,
            normal.cyan,
        ],
    })
}

/// The registry's scheme names in registry order (COSMIC pair first,
/// the rest alphabetical) — the option list the settings dropdowns
/// feed from. Derived from the same `SCHEMES` table `by_name` reads,
/// so the picker can never offer a name lookup would reject.
pub fn scheme_names() -> Vec<&'static str> {
    SCHEMES.iter().map(|entry| entry.name).collect()
}

/// The ANSI slots a preview chip strip renders, in strip order:
/// normal 0–7 then bright 8–15 — the sixteen colors every scheme
/// fully defines, everything the terminal ever shows beyond
/// background and foreground.
const PREVIEW_ANSI: [NamedColor; 16] = [
    NamedColor::Black,
    NamedColor::Red,
    NamedColor::Green,
    NamedColor::Yellow,
    NamedColor::Blue,
    NamedColor::Magenta,
    NamedColor::Cyan,
    NamedColor::White,
    NamedColor::BrightBlack,
    NamedColor::BrightRed,
    NamedColor::BrightGreen,
    NamedColor::BrightYellow,
    NamedColor::BrightBlue,
    NamedColor::BrightMagenta,
    NamedColor::BrightCyan,
    NamedColor::BrightWhite,
];

/// One scheme's preview palette: the resolved background,
/// foreground, and ANSI 0–15 the terminal renders with — through the
/// same `From<&ColorScheme>` conversion `by_name` serves, so a chip
/// can never disagree with the real scheme (pinned by test). The
/// background is `Option` with teeth: the COSMIC pair ships none
/// (the window follows the system theme), so its chip previews
/// exactly that — the view draws a `None` background from the live
/// theme, the honest "what the terminal shows" color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemePreview {
    /// The scheme's background, or `None` where the terminal falls
    /// back to the system theme.
    pub background: Option<Rgb>,
    /// The scheme's foreground.
    pub foreground: Option<Rgb>,
    /// Normal 0–7 then bright 8–15, strip order.
    pub ansi: [Option<Rgb>; 16],
}

/// The preview palette of every registry scheme, registry order —
/// one definition site with [`scheme_names`] (entry-for-entry, pinned
/// by test). The scheme picker's rows and the closed control's mini
/// strip both render from this; the settings drawer is its first
/// consumer and the profile scheme field (`profile-manager`) reuses
/// it without new swatch code.
pub fn scheme_previews() -> Vec<(&'static str, SchemePreview)> {
    SCHEMES
        .iter()
        .map(|entry| {
            let colors = Colors::from(&entry.scheme);
            (
                entry.name,
                SchemePreview {
                    background: colors[NamedColor::Background],
                    foreground: colors[NamedColor::Foreground],
                    ansi: std::array::from_fn(|slot| colors[PREVIEW_ANSI[slot]]),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scheme with foreground, background, normal and bright tables,
    /// and nothing else — the shape alacritty-theme TOMLs give us.
    fn plain_scheme() -> ColorScheme {
        ColorScheme {
            foreground: Some(rgb(0xF8_F8F2)),
            background: Some(rgb(0x28_2A36)),
            cursor: None,
            bright_foreground: None,
            dim_foreground: None,
            normal: ColorSchemeAnsi {
                black: Some(rgb(0x21_222C)),
                red: Some(rgb(0xFF_5555)),
                green: Some(rgb(0x50_FA7B)),
                yellow: Some(rgb(0xF1_FA8C)),
                blue: Some(rgb(0xBD_93F9)),
                magenta: Some(rgb(0xFF_79C6)),
                cyan: Some(rgb(0x8B_E9FD)),
                white: Some(rgb(0xF8_F8F2)),
            },
            bright: ColorSchemeAnsi {
                black: Some(rgb(0x62_72A4)),
                red: Some(rgb(0xFF_6E6E)),
                green: Some(rgb(0x69_FF94)),
                yellow: Some(rgb(0xFF_FFA5)),
                blue: Some(rgb(0xD6_ACFF)),
                magenta: Some(rgb(0xFF_92DF)),
                cyan: Some(rgb(0xA4_FFFF)),
                white: Some(rgb(0xFF_FFFF)),
            },
            dim: ColorSchemeAnsi::EMPTY,
        }
    }

    #[test]
    fn conversion_fills_the_named_slots_from_the_palette() {
        let colors = Colors::from(&plain_scheme());
        assert_eq!(colors[NamedColor::Red], Some(rgb(0xFF_5555)));
        assert_eq!(colors[NamedColor::BrightBlue], Some(rgb(0xD6_ACFF)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xF8_F8F2)));
        // A scheme-owned background reaches the slot (the engine only
        // falls back to WINDOW_BG_COLOR while it is None).
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x28_2A36)));
    }

    #[test]
    fn conversion_derives_missing_dims_from_normals_at_the_alacritty_factor() {
        // Alacritty derives dims as `normal * 0.66` per channel
        // (DIM_FACTOR, docs/research/terminal-theming.md §2); the cast
        // truncates. 0xFF * 0.66 = 168.3 -> 168 = 0xA8.
        let colors = Colors::from(&plain_scheme());
        assert_eq!(colors[NamedColor::DimRed], Some(rgb(0xA8_3838)));
        assert_eq!(colors[NamedColor::DimWhite], Some(rgb(0xA3_A39F)));
    }

    #[test]
    fn conversion_defaults_cursor_and_foregrounds_to_the_foreground() {
        let colors = Colors::from(&plain_scheme());
        assert_eq!(colors[NamedColor::Cursor], Some(rgb(0xF8_F8F2)));
        assert_eq!(colors[NamedColor::BrightForeground], Some(rgb(0xF8_F8F2)));
        // Dim foreground defaults to foreground * 0.66:
        // 0xF8 * 0.66 = 163.68 -> 163 = 0xA3, 0xF2 * 0.66 = 159.7 -> 0x9F.
        assert_eq!(colors[NamedColor::DimForeground], Some(rgb(0xA3_A39F)));
    }

    #[test]
    fn conversion_keeps_explicit_dims_and_specials_over_derived_defaults() {
        // The COSMIC pair ships hand-tuned dim tables; derivation must
        // never overwrite a value the scheme actually provides.
        let mut scheme = plain_scheme();
        scheme.dim.red = Some(rgb(0xA0_4040));
        scheme.cursor = Some(rgb(0xFF_CC00));
        scheme.bright_foreground = Some(rgb(0xFF_FFFF));
        scheme.dim_foreground = Some(rgb(0x89_8989));
        let colors = Colors::from(&scheme);
        assert_eq!(colors[NamedColor::DimRed], Some(rgb(0xA0_4040)));
        assert_eq!(colors[NamedColor::Cursor], Some(rgb(0xFF_CC00)));
        assert_eq!(colors[NamedColor::BrightForeground], Some(rgb(0xFF_FFFF)));
        assert_eq!(colors[NamedColor::DimForeground], Some(rgb(0x89_8989)));
        // Unrelated slots still derive.
        assert_eq!(colors[NamedColor::DimGreen], Some(rgb(0x34_A551)));
    }

    // --- byte-identity pins: the COSMIC pair's user-visible slots.
    // The route through `ColorScheme` must not move a single one. ---

    #[test]
    fn cosmic_dark_normal_and_bright_slots_are_pinned() {
        let colors = cosmic_dark();
        assert_eq!(colors[NamedColor::Black], Some(rgb(0x1B_1B1B)));
        assert_eq!(colors[NamedColor::Red], Some(rgb(0xF1_6161)));
        assert_eq!(colors[NamedColor::Green], Some(rgb(0x7C_B987)));
        assert_eq!(colors[NamedColor::Yellow], Some(rgb(0xDD_C74C)));
        assert_eq!(colors[NamedColor::Blue], Some(rgb(0x62_96BE)));
        assert_eq!(colors[NamedColor::Magenta], Some(rgb(0xBE_6DEE)));
        assert_eq!(colors[NamedColor::Cyan], Some(rgb(0x49_BAC8)));
        assert_eq!(colors[NamedColor::White], Some(rgb(0xBE_BEBE)));
        assert_eq!(colors[NamedColor::BrightBlack], Some(rgb(0x80_8080)));
        assert_eq!(colors[NamedColor::BrightRed], Some(rgb(0xFF_8985)));
        assert_eq!(colors[NamedColor::BrightGreen], Some(rgb(0x97_D5A0)));
        assert_eq!(colors[NamedColor::BrightYellow], Some(rgb(0xFA_E365)));
        assert_eq!(colors[NamedColor::BrightBlue], Some(rgb(0x7D_B1DA)));
        assert_eq!(colors[NamedColor::BrightMagenta], Some(rgb(0xD6_8EFF)));
        assert_eq!(colors[NamedColor::BrightCyan], Some(rgb(0x49_BAC8)));
        assert_eq!(colors[NamedColor::BrightWhite], Some(rgb(0xC4_C4C4)));
    }

    #[test]
    fn cosmic_dark_dim_and_special_slots_are_pinned() {
        let colors = cosmic_dark();
        assert_eq!(colors[NamedColor::DimBlack], Some(rgb(0x00_0000)));
        assert_eq!(colors[NamedColor::DimRed], Some(rgb(0xA0_4040)));
        assert_eq!(colors[NamedColor::DimGreen], Some(rgb(0x5D_7D62)));
        assert_eq!(colors[NamedColor::DimYellow], Some(rgb(0x9E_914A)));
        assert_eq!(colors[NamedColor::DimBlue], Some(rgb(0x48_6073)));
        assert_eq!(colors[NamedColor::DimMagenta], Some(rgb(0x7F_46A1)));
        assert_eq!(colors[NamedColor::DimCyan], Some(rgb(0x3F_7F87)));
        assert_eq!(colors[NamedColor::DimWhite], Some(rgb(0x89_8989)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xC4_C4C4)));
        assert_eq!(colors[NamedColor::Cursor], Some(rgb(0xC4_C4C4)));
        assert_eq!(colors[NamedColor::BrightForeground], Some(rgb(0xC4_C4C4)));
        assert_eq!(colors[NamedColor::DimForeground], Some(rgb(0x89_8989)));
    }

    #[test]
    fn cosmic_light_normal_and_bright_slots_are_pinned() {
        let colors = cosmic_light();
        assert_eq!(colors[NamedColor::Black], Some(rgb(0x1B_1B1B)));
        assert_eq!(colors[NamedColor::Red], Some(rgb(0xA2_4F50)));
        assert_eq!(colors[NamedColor::Green], Some(rgb(0x43_7E4F)));
        assert_eq!(colors[NamedColor::Yellow], Some(rgb(0x7D_6E1E)));
        assert_eq!(colors[NamedColor::Blue], Some(rgb(0x51_6D94)));
        assert_eq!(colors[NamedColor::Magenta], Some(rgb(0x9A_30CA)));
        assert_eq!(colors[NamedColor::Cyan], Some(rgb(0x22_7A85)));
        assert_eq!(colors[NamedColor::White], Some(rgb(0xD7_D7D7)));
        assert_eq!(colors[NamedColor::BrightBlack], Some(rgb(0x00_0000)));
        assert_eq!(colors[NamedColor::BrightRed], Some(rgb(0x89_0418)));
        assert_eq!(colors[NamedColor::BrightGreen], Some(rgb(0x18_5529)));
        assert_eq!(colors[NamedColor::BrightYellow], Some(rgb(0x53_4800)));
        assert_eq!(colors[NamedColor::BrightBlue], Some(rgb(0x2E_496D)));
        assert_eq!(colors[NamedColor::BrightMagenta], Some(rgb(0x6B_0091)));
        assert_eq!(colors[NamedColor::BrightCyan], Some(rgb(0x00_525A)));
        assert_eq!(colors[NamedColor::BrightWhite], Some(rgb(0xE8_E8E8)));
    }

    #[test]
    fn cosmic_light_dim_and_special_slots_are_pinned() {
        let colors = cosmic_light();
        assert_eq!(colors[NamedColor::DimBlack], Some(rgb(0x26_2626)));
        assert_eq!(colors[NamedColor::DimRed], Some(rgb(0xB2_5D5E)));
        assert_eq!(colors[NamedColor::DimGreen], Some(rgb(0x52_8D5E)));
        assert_eq!(colors[NamedColor::DimYellow], Some(rgb(0x8C_7D30)));
        assert_eq!(colors[NamedColor::DimBlue], Some(rgb(0x5F_7CA4)));
        assert_eq!(colors[NamedColor::DimMagenta], Some(rgb(0xAA_43DB)));
        assert_eq!(colors[NamedColor::DimCyan], Some(rgb(0x35_8994)));
        assert_eq!(colors[NamedColor::DimWhite], Some(rgb(0xC4_C4C4)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0x1B_1B1B)));
        assert_eq!(colors[NamedColor::Cursor], Some(rgb(0x1B_1B1B)));
        assert_eq!(colors[NamedColor::BrightForeground], Some(rgb(0x00_0000)));
        assert_eq!(colors[NamedColor::DimForeground], Some(rgb(0x26_2626)));
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
        // Unknown names are `active_colors()`'s fallback signal, not
        // an error. "Catppuccin Frappe" is a real upstream variant
        // the registry does not ship.
        assert!(by_name("Catppuccin Frappe").is_none());
        assert!(by_name("").is_none());
    }

    // --- registry ---
    //
    // The registry is the single definition site for the scheme names:
    // `by_name`, the dropdown options, and these pins all read the
    // same table, so UI, file format, and lookup cannot drift.

    #[test]
    fn registry_pins_the_shipped_scheme_order() {
        // COSMIC pair first (the defaults), the rest alphabetical.
        let names: Vec<&str> = SCHEMES.iter().map(|entry| entry.name).collect();
        assert_eq!(
            names,
            [
                "COSMIC Dark",
                "COSMIC Light",
                "Catppuccin Latte",
                "Catppuccin Mocha",
                "Dracula",
                "Gruvbox Dark",
                "Gruvbox Light",
                "Nord",
                "Solarized Dark",
                "Solarized Light",
                "Tokyo Night Day",
                "Tokyo Night Storm",
            ]
        );
    }

    #[test]
    fn registry_names_are_unique() {
        let mut names: Vec<&str> = SCHEMES.iter().map(|entry| entry.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), SCHEMES.len());
    }

    // --- the swatch preview seam ---

    #[test]
    fn a_swatch_carries_the_scheme_background_and_six_accents() {
        // Dracula's canonical values, in ls-colors cell order.
        let swatch = scheme_swatch("Dracula").expect("registry name");
        assert_eq!(swatch.background, Some(rgb(0x28_2A36)));
        assert_eq!(
            swatch.accents,
            [
                Some(rgb(0xFF_5555)),
                Some(rgb(0x50_FA7B)),
                Some(rgb(0xF1_FA8C)),
                Some(rgb(0xBD_93F9)),
                Some(rgb(0xFF_79C6)),
                Some(rgb(0x8B_E9FD)),
            ]
        );
    }

    #[test]
    fn the_cosmic_pair_previews_accents_without_inventing_a_background() {
        // The COSMIC schemes defer background to the system theme; the
        // preview must not fabricate one.
        for name in [COSMIC_THEME_DARK, COSMIC_THEME_LIGHT] {
            let swatch = scheme_swatch(name).expect("registry name");
            assert_eq!(swatch.background, None);
            assert!(swatch.accents.iter().all(Option::is_some));
        }
    }

    #[test]
    fn an_unknown_name_has_no_swatch() {
        // The preview degrades exactly where resolution does: unknown
        // names render the "follows global" chip.
        assert!(scheme_swatch("Catppuccin Frappe").is_none());
        assert!(scheme_swatch("").is_none());
    }

    #[test]
    fn by_name_serves_every_registry_entry() {
        for entry in &SCHEMES {
            assert!(
                by_name(entry.name).is_some(),
                "registry name {:?} does not resolve",
                entry.name
            );
        }
    }

    #[test]
    fn third_party_schemes_resolve_with_no_holes_in_the_named_slots() {
        let normal_and_bright = [
            NamedColor::Black,
            NamedColor::Red,
            NamedColor::Green,
            NamedColor::Yellow,
            NamedColor::Blue,
            NamedColor::Magenta,
            NamedColor::Cyan,
            NamedColor::White,
            NamedColor::BrightBlack,
            NamedColor::BrightRed,
            NamedColor::BrightGreen,
            NamedColor::BrightYellow,
            NamedColor::BrightBlue,
            NamedColor::BrightMagenta,
            NamedColor::BrightCyan,
            NamedColor::BrightWhite,
        ];
        let dims = [
            NamedColor::DimBlack,
            NamedColor::DimRed,
            NamedColor::DimGreen,
            NamedColor::DimYellow,
            NamedColor::DimBlue,
            NamedColor::DimMagenta,
            NamedColor::DimCyan,
            NamedColor::DimWhite,
        ];
        // The COSMIC pair deliberately leaves Background to the system
        // theme; every other scheme must come through complete.
        for entry in SCHEMES.iter().skip(2) {
            let colors = by_name(entry.name).expect("pinned by the test above");
            for slot in normal_and_bright {
                assert!(colors[slot].is_some(), "{}: {slot:?} unset", entry.name);
            }
            for slot in dims {
                assert!(colors[slot].is_some(), "{}: {slot:?} unset", entry.name);
            }
            assert!(colors[NamedColor::Foreground].is_some());
            assert!(colors[NamedColor::Background].is_some());
            assert!(colors[NamedColor::Cursor].is_some());
        }
    }

    #[test]
    fn dracula_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Dracula").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x28_2A36)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xF8_F8F2)));
        assert_eq!(colors[NamedColor::Blue], Some(rgb(0xBD_93F9)));
    }

    #[test]
    fn solarized_dark_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Solarized Dark").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x00_2B36)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0x83_9496)));
        assert_eq!(colors[NamedColor::Cyan], Some(rgb(0x2A_A198)));
    }

    #[test]
    fn solarized_light_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Solarized Light").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0xFD_F6E3)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0x58_6E75)));
        assert_eq!(colors[NamedColor::Green], Some(rgb(0x85_9900)));
    }

    #[test]
    fn gruvbox_dark_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Gruvbox Dark").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x28_2828)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xEB_DBB2)));
        assert_eq!(colors[NamedColor::BrightRed], Some(rgb(0xFB_4934)));
    }

    #[test]
    fn gruvbox_light_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Gruvbox Light").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0xFB_F1C7)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0x3C_3836)));
        assert_eq!(colors[NamedColor::BrightYellow], Some(rgb(0xB5_7614)));
    }

    #[test]
    fn tokyo_night_storm_spot_checks_match_the_theme_toml() {
        let colors = by_name("Tokyo Night Storm").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x24_283B)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xA9_B1D6)));
        assert_eq!(colors[NamedColor::Blue], Some(rgb(0x7A_A2F7)));
    }

    #[test]
    fn tokyo_night_day_spot_checks_match_the_canonical_extra() {
        let colors = by_name("Tokyo Night Day").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0xE1_E2E7)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0x37_60BF)));
        assert_eq!(colors[NamedColor::Cyan], Some(rgb(0x00_7197)));
    }

    #[test]
    fn nord_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Nord").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xD8_DEE9)));
        assert_eq!(colors[NamedColor::Cyan], Some(rgb(0x88_C0D0)));
    }

    #[test]
    fn catppuccin_mocha_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Catppuccin Mocha").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x1E_1E2E)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xCD_D6F4)));
        assert_eq!(colors[NamedColor::Blue], Some(rgb(0x89_B4FA)));
    }

    #[test]
    fn catppuccin_latte_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Catppuccin Latte").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0xEF_F1F5)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0x4C_4F69)));
        assert_eq!(colors[NamedColor::Red], Some(rgb(0xD2_0F39)));
    }

    // --- the preview accessor (scheme-picker T1) ---

    #[test]
    fn scheme_previews_names_match_the_registry_entry_for_entry() {
        // The single-definition pin: the preview list is the registry
        // list — same length, same order, same names — so a second
        // name list can never drift from `scheme_names`.
        let names = scheme_names();
        let previews = scheme_previews();
        assert_eq!(previews.len(), names.len());
        for ((preview_name, _), registry_name) in previews.iter().zip(names.iter()) {
            assert_eq!(preview_name, registry_name);
        }
    }

    #[test]
    fn every_registry_scheme_resolves_a_complete_preview() {
        // Every entry previews foreground plus all sixteen ANSI
        // slots. The background is the one honest exception: the
        // COSMIC pair ships no background (the window follows the
        // system theme), so its chip previews exactly that — `None`,
        // which the view renders from the live theme.
        let previews = scheme_previews();
        assert_eq!(previews.len(), 12);
        for (name, preview) in previews {
            assert!(
                preview.foreground.is_some(),
                "{name} must preview a foreground"
            );
            assert!(
                preview.ansi.iter().all(Option::is_some),
                "{name} must preview all sixteen ANSI colors"
            );
            if name == "COSMIC Dark" || name == "COSMIC Light" {
                assert_eq!(preview.background, None, "{name} follows the system theme");
            } else {
                assert!(
                    preview.background.is_some(),
                    "{name} must preview a background"
                );
            }
        }
    }

    #[test]
    fn scheme_previews_resolve_through_the_same_conversion_as_by_name() {
        // The chip-can't-disagree pin: every preview slot equals the
        // slot `by_name` serves the live terminal for that scheme —
        // the same `From<&ColorScheme>` path, not a parallel
        // transcription.
        for (name, preview) in scheme_previews() {
            let colors = by_name(name).expect("every preview name resolves");
            assert_eq!(preview.background, colors[NamedColor::Background], "{name}");
            assert_eq!(preview.foreground, colors[NamedColor::Foreground], "{name}");
            for (slot, color) in preview.ansi.iter().enumerate() {
                assert_eq!(
                    *color, colors[PREVIEW_ANSI[slot]],
                    "{name} ANSI slot {slot}"
                );
            }
        }
    }

    #[test]
    fn preview_spot_hexes_pin_the_transcription() {
        // A palette transcription regression lands here: known hexes
        // straight off the pinned tables above.
        let previews: Vec<_> = scheme_previews();
        let (_, cosmic_dark) = previews[0];
        assert_eq!(cosmic_dark.background, None);
        assert_eq!(cosmic_dark.foreground, Some(rgb(0xC4_C4C4)));
        assert_eq!(cosmic_dark.ansi[0], Some(rgb(0x1B_1B1B)));
        assert_eq!(cosmic_dark.ansi[8], Some(rgb(0x80_8080)));

        let dracula = previews
            .iter()
            .find(|(name, _)| *name == "Dracula")
            .expect("Dracula registered");
        assert_eq!(dracula.1.background, Some(rgb(0x28_2A36)));
        assert_eq!(dracula.1.foreground, Some(rgb(0xF8_F8F2)));
        assert_eq!(dracula.1.ansi[4], Some(rgb(0xBD_93F9)));
    }
}
