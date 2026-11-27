// SPDX-License-Identifier: GPL-3.0-only
// Copyright 2023 System76 <info@system76.com>
//
// Adapted from cosmic-term
// (<https://github.com/pop-os/cosmic-term>) src/terminal_theme.rs and
// src/config.rs at commit 9129277 (palettes) and `3ef31d7` (the
// `ColorScheme` representation). The palettes are unmodified;
// cosmic-term's serde/RON-file, per-profile, and import/export
// machinery stays dropped — schemes compile in and resolve by name
// (see the terminal-theming record). The ten third-party
// palettes are transcribed from alacritty-theme TOMLs and canonical
// extras, each with its source and license recorded on the const
// below. cosmic-term's own COSMIC Dark/Light pair left the registry
// (2026-11-20): their `None` background deferred to the host system
// theme — OS coupling the cross-platform picker does not carry, and
// every shipped scheme now paints its own canvas.

use alacritty_terminal::{term::color::Colors, vte::ansi::NamedColor};

use crate::core::theme_files::UserScheme;

// Re-exported because [`SchemePreview`]'s slots name it — the chip
// renderers beyond this module must be able to spell it.
pub use alacritty_terminal::vte::ansi::Rgb;

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
// in, nothing parses. User scheme files ride a file-facing twin
// (`theme_files`) that converts into this shape, so the compiled-in
// representation stays serde-free (theme-files spec).

/// One color of a scheme table; `None` leaves the slot to a default.
pub(crate) type Slot = Option<Rgb>;

/// One 8-color table of a scheme: the normal, bright, or dim variant.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ColorSchemeAnsi {
    pub(crate) black: Slot,
    pub(crate) red: Slot,
    pub(crate) green: Slot,
    pub(crate) yellow: Slot,
    pub(crate) blue: Slot,
    pub(crate) magenta: Slot,
    pub(crate) cyan: Slot,
    pub(crate) white: Slot,
}

impl ColorSchemeAnsi {
    /// Every slot unset — the dim table base for every shipped
    /// palette: dims derive at conversion, no scheme ships a table.
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
/// normal/bright/dim 8-color tables — the terminal-theming record
/// §4). Every shipped scheme fills the background; the slot keeps
/// its `Option` shape from the cosmic-term representation, where a
/// `None` background deferred to the host system theme via
/// `WINDOW_BG_COLOR`. Any other `None` slot falls back to a uniform
/// derivation in the conversion below.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ColorScheme {
    pub(crate) foreground: Slot,
    pub(crate) background: Slot,
    pub(crate) cursor: Slot,
    pub(crate) bright_foreground: Slot,
    pub(crate) dim_foreground: Slot,
    pub(crate) normal: ColorSchemeAnsi,
    pub(crate) bright: ColorSchemeAnsi,
    pub(crate) dim: ColorSchemeAnsi,
}

/// Alacritty's dim factor: a missing dim color derives as
/// `normal * 0.66` per channel, truncating (`alacritty/src/display/
/// color.rs` `DIM_FACTOR`; the terminal-theming record §2).
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

// --- the GitHub pair (the defaults) ---
//
// GitHub's own palettes: the anchors and the 16 ANSI slots come from
// @primer/primitives@7.10.0 — the exact version
// [primer/github-vscode-theme](https://github.com/primer/github-vscode-theme)
// pins — through that theme's `src/theme.js` terminal block
// (`terminal.ansiX ← color.ansi.X`, `terminal.foreground ←
// fg.default`). Real backgrounds, like every shipped scheme: a
// fresh install paints GitHub's canvas.

/// The GitHub Dark name — identity, persisted under
/// `color_scheme_dark`, shown in the dropdowns. Lives here (the
/// schemes' definition site); the config defaults and the
/// unknown-name fallback read this one constant.
pub const GITHUB_THEME_DARK: &str = "GitHub Dark";

/// The GitHub Light name — same contract as [`GITHUB_THEME_DARK`]
/// on the light side.
pub const GITHUB_THEME_LIGHT: &str = "GitHub Light";

/// The GitHub Dark palette. Upstream citations per slot live in the
/// spot-check tests; the one deliberate deviation from the port's
/// table: bright blue is the theme's dark `accent.fg` `#2f81f7`
/// (its `src/colors.js` override), not the port's `blueBright`
/// `#79c0ff` — `theme_gen` rides the chrome accent on `BrightBlue`,
/// and the entry pins it to GitHub's dark accent blue. Dims,
/// cursor, and dim/bright foregrounds derive at conversion (the
/// port's terminal block publishes none).
const GITHUB_DARK: ColorScheme = ColorScheme {
    foreground: Some(rgb(0xE6_EDF3)),
    background: Some(rgb(0x0D_1117)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x48_4F58)),
        red: Some(rgb(0xFF_7B72)),
        green: Some(rgb(0x3F_B950)),
        yellow: Some(rgb(0xD2_9922)),
        blue: Some(rgb(0x58_A6FF)),
        magenta: Some(rgb(0xBC_8CFF)),
        cyan: Some(rgb(0x39_C5CF)),
        white: Some(rgb(0xB1_BAC4)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x6E_7681)),
        red: Some(rgb(0xFF_A198)),
        green: Some(rgb(0x56_D364)),
        yellow: Some(rgb(0xE3_B341)),
        blue: Some(rgb(0x2F_81F7)),
        magenta: Some(rgb(0xD2_A8FF)),
        cyan: Some(rgb(0x56_D4DD)),
        white: Some(rgb(0xFF_FFFF)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

/// The GitHub Light palette — same sources and the same one
/// deviation (bright blue pinned to the light `accent.fg`
/// `#0969da`, not the port's `blueBright` `#218bff`).
const GITHUB_LIGHT: ColorScheme = ColorScheme {
    foreground: Some(rgb(0x1F_2328)),
    background: Some(rgb(0xFF_FFFF)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x24_292F)),
        red: Some(rgb(0xCF_222E)),
        green: Some(rgb(0x11_6329)),
        yellow: Some(rgb(0x4D_2D00)),
        blue: Some(rgb(0x09_69DA)),
        magenta: Some(rgb(0x82_50DF)),
        cyan: Some(rgb(0x1B_7C83)),
        white: Some(rgb(0x6E_7781)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x57_606A)),
        red: Some(rgb(0xA4_0E26)),
        green: Some(rgb(0x1A_7F37)),
        yellow: Some(rgb(0x63_3C01)),
        blue: Some(rgb(0x09_69DA)),
        magenta: Some(rgb(0xA4_75F9)),
        cyan: Some(rgb(0x31_92AA)),
        white: Some(rgb(0x8C_959F)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

/// The GitHub Dark scheme as alacritty colors.
pub fn github_dark() -> Colors {
    Colors::from(&GITHUB_DARK)
}

/// The GitHub Light scheme as alacritty colors.
pub fn github_light() -> Colors {
    Colors::from(&GITHUB_LIGHT)
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
/// (the terminal-theming record §Licensing).
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
/// so names, options, and lookup cannot drift. GitHub Dark/Light
/// first (the defaults), the rest alphabetical.
const SCHEMES: [SchemeEntry; 12] = [
    SchemeEntry {
        name: GITHUB_THEME_DARK,
        scheme: GITHUB_DARK,
    },
    SchemeEntry {
        name: GITHUB_THEME_LIGHT,
        scheme: GITHUB_LIGHT,
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
/// (`active_colors()`) falls back to the GitHub pair for the active
/// mode; no error surface.
pub fn by_name(name: &str) -> Option<Colors> {
    SCHEMES
        .iter()
        .find(|entry| entry.name == name)
        .map(|entry| Colors::from(&entry.scheme))
}

/// The registry's scheme names in registry order (GitHub pair first,
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
/// background is `Option` so an out-of-range preview row can carry
/// no strip at all; every registry scheme resolves one, and no
/// shipped scheme defers its background to the system theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemePreview {
    /// The scheme's background, or `None` where no scheme resolves
    /// (the fallback preview row renders no chip).
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
        .map(|entry| (entry.name, preview_of(&entry.scheme)))
        .collect()
}

/// One scheme's preview, resolved through the same `From<&ColorScheme>`
/// conversion `by_name` serves — shared by the registry and merged
/// list builders so every preview rides one path.
fn preview_of(scheme: &ColorScheme) -> SchemePreview {
    let colors = Colors::from(scheme);
    SchemePreview {
        background: colors[NamedColor::Background],
        foreground: colors[NamedColor::Foreground],
        ansi: std::array::from_fn(|slot| colors[PREVIEW_ANSI[slot]]),
    }
}

// --- the merged registry (theme-files F1) ---

/// Look up a scheme by name across the merged registry: the
/// built-ins first (a user file naming one is shadowed at load; the
/// registry-first order keeps every lookup safe regardless), then the
/// user schemes as loaded. Unknown names return `None` — the caller
/// falls back to the GitHub pair exactly as for a stale built-in
/// name; no error surface.
pub fn merged_by_name(name: &str, user: &[UserScheme]) -> Option<Colors> {
    if let Some(colors) = by_name(name) {
        return Some(colors);
    }
    user.iter()
        .find(|scheme| scheme.name == name)
        .map(|scheme| Colors::from(&scheme.scheme))
}

/// The merged scheme-name list: the registry in its order and
/// identity, then the user schemes in the loader's sorted order —
/// owned names, because a user scheme's name is runtime data, not a
/// compile-time literal. The option list the settings dropdowns feed
/// from once user schemes ride.
pub fn merged_scheme_names(user: &[UserScheme]) -> Vec<String> {
    SCHEMES
        .iter()
        .map(|entry| entry.name.to_string())
        .chain(user.iter().map(|scheme| scheme.name.clone()))
        .collect()
}

/// The merged preview list, one definition site with
/// [`merged_scheme_names`] (entry-for-entry, pinned by test): the
/// registry's previews then the user schemes', every one through the
/// same conversion — so the chip-parity pin extends over user
/// schemes by construction.
pub fn merged_scheme_previews(user: &[UserScheme]) -> Vec<(String, SchemePreview)> {
    SCHEMES
        .iter()
        .map(|entry| (entry.name.to_string(), preview_of(&entry.scheme)))
        .chain(
            user.iter()
                .map(|scheme| (scheme.name.clone(), preview_of(&scheme.scheme))),
        )
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
        // (DIM_FACTOR, the terminal-theming record §2); the cast
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
        // A scheme carrying explicit values wins over derivation —
        // the representation keeps the slots, so a future palette
        // that ships its own dims lands here untouched.
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

    #[test]
    fn every_scheme_shares_the_color_cube_and_owns_a_background() {
        // Slots 16..=255 come from the shared ransid cube/ramp —
        // identical across schemes by construction — and every
        // shipped scheme paints its own background (none defers to
        // the system theme any more).
        for entry in &SCHEMES {
            let colors = by_name(entry.name).expect("pinned by the test above");
            assert!(
                colors[NamedColor::Background].is_some(),
                "{}: no background",
                entry.name
            );
            for slot in 16..256 {
                assert_eq!(
                    colors[slot],
                    auto_colors()[slot],
                    "cube slot {slot} drifted"
                );
            }
        }
    }

    #[test]
    fn by_name_rejects_unknown_names() {
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
        // GitHub pair first (the defaults), the rest alphabetical.
        let names: Vec<&str> = SCHEMES.iter().map(|entry| entry.name).collect();
        assert_eq!(
            names,
            [
                GITHUB_THEME_DARK,
                GITHUB_THEME_LIGHT,
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
    fn every_scheme_resolves_with_no_holes_in_the_named_slots() {
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
        // Every shipped scheme must come through complete — owned
        // background included; nothing defers to the system theme.
        for entry in &SCHEMES {
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
    fn github_dark_spot_checks_match_primer_and_the_vscode_theme_port() {
        // Upstream: @primer/primitives@7.10.0 (the version
        // primer/github-vscode-theme pins) `dark.json` via the
        // theme's own `src/theme.js` terminal block — background
        // canvas.default `#0d1117`, foreground fg.default
        // `#e6edf3` (the theme's `src/colors.js` override),
        // ANSI slots color.ansi.*. The one deviation: bright blue
        // is the theme's dark accent.fg `#2f81f7` (also a
        // colors.js override), not the port's blueBright
        // `#79c0ff` — theme_gen rides the chrome accent on
        // BrightBlue, and the entries pin it to GitHub's dark
        // accent blue (theme-defaults spec, owner answers
        // 2026-11-18).
        let colors = by_name(GITHUB_THEME_DARK).expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x0D_1117)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xE6_EDF3)));
        assert_eq!(colors[NamedColor::Black], Some(rgb(0x48_4F58)));
        assert_eq!(colors[NamedColor::Red], Some(rgb(0xFF_7B72)));
        assert_eq!(colors[NamedColor::Green], Some(rgb(0x3F_B950)));
        assert_eq!(colors[NamedColor::Blue], Some(rgb(0x58_A6FF)));
        assert_eq!(colors[NamedColor::Magenta], Some(rgb(0xBC_8CFF)));
        assert_eq!(colors[NamedColor::Cyan], Some(rgb(0x39_C5CF)));
        assert_eq!(colors[NamedColor::BrightBlack], Some(rgb(0x6E_7681)));
        assert_eq!(colors[NamedColor::BrightBlue], Some(rgb(0x2F_81F7)));
        assert_eq!(colors[NamedColor::BrightWhite], Some(rgb(0xFF_FFFF)));
        // Derived slots (no upstream in the terminal block):
        // cursor defaults to the foreground at conversion.
        assert_eq!(colors[NamedColor::Cursor], Some(rgb(0xE6_EDF3)));
    }

    #[test]
    fn github_light_spot_checks_match_primer_and_the_vscode_theme_port() {
        // Upstream: @primer/primitives@7.10.0 `light.json` via the
        // theme's terminal block — background canvas.default
        // `#ffffff`, foreground fg.default `#1f2328` (the theme's
        // colors.js override), ANSI slots color.ansi.*. The one
        // deviation: bright blue is accent.fg `#0969da`, not the
        // port's blueBright `#218bff` — same reason as the dark
        // entry's, pinning theme_gen's accent on GitHub's light
        // accent blue.
        let colors = by_name(GITHUB_THEME_LIGHT).expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0xFF_FFFF)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0x1F_2328)));
        assert_eq!(colors[NamedColor::Black], Some(rgb(0x24_292F)));
        assert_eq!(colors[NamedColor::Red], Some(rgb(0xCF_222E)));
        assert_eq!(colors[NamedColor::Green], Some(rgb(0x11_6329)));
        assert_eq!(colors[NamedColor::Yellow], Some(rgb(0x4D_2D00)));
        assert_eq!(colors[NamedColor::Blue], Some(rgb(0x09_69DA)));
        assert_eq!(colors[NamedColor::White], Some(rgb(0x6E_7781)));
        assert_eq!(colors[NamedColor::BrightBlack], Some(rgb(0x57_606A)));
        assert_eq!(colors[NamedColor::BrightBlue], Some(rgb(0x09_69DA)));
        assert_eq!(colors[NamedColor::BrightWhite], Some(rgb(0x8C_959F)));
        assert_eq!(colors[NamedColor::Cursor], Some(rgb(0x1F_2328)));
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
        // Every entry previews foreground, background, and all
        // sixteen ANSI slots — no shipped scheme leaves a slot to
        // the system theme.
        let previews = scheme_previews();
        assert_eq!(previews.len(), SCHEMES.len());
        for (name, preview) in previews {
            assert!(
                preview.foreground.is_some(),
                "{name} must preview a foreground"
            );
            assert!(
                preview.background.is_some(),
                "{name} must preview a background"
            );
            assert!(
                preview.ansi.iter().all(Option::is_some),
                "{name} must preview all sixteen ANSI colors"
            );
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
        let (_, github_dark) = previews[0];
        assert_eq!(github_dark.background, Some(rgb(0x0D_1117)));
        assert_eq!(github_dark.foreground, Some(rgb(0xE6_EDF3)));
        assert_eq!(github_dark.ansi[0], Some(rgb(0x48_4F58)));
        assert_eq!(github_dark.ansi[8], Some(rgb(0x6E_7681)));

        let dracula = previews
            .iter()
            .find(|(name, _)| *name == "Dracula")
            .expect("Dracula registered");
        assert_eq!(dracula.1.background, Some(rgb(0x28_2A36)));
        assert_eq!(dracula.1.foreground, Some(rgb(0xF8_F8F2)));
        assert_eq!(dracula.1.ansi[4], Some(rgb(0xBD_93F9)));
    }

    // --- the merged registry (theme-files F1) ---

    /// One user scheme for the merged-surface pins: the plain fixture
    /// under a caller-chosen name.
    fn user_scheme(name: &str, scheme: ColorScheme) -> crate::core::theme_files::UserScheme {
        crate::core::theme_files::UserScheme {
            name: name.to_string(),
            scheme,
        }
    }

    #[test]
    fn merged_by_name_resolves_a_user_scheme_and_still_rejects_unknowns() {
        let user = [user_scheme("mine", plain_scheme())];
        let colors = merged_by_name("mine", &user).expect("user scheme resolves");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x28_2A36)));
        assert!(merged_by_name("Catppuccin Frappe", &user).is_none());
        assert!(merged_by_name("", &user).is_none());
    }

    #[test]
    fn merged_by_name_serves_the_builtin_when_a_user_entry_names_one() {
        // Defense in depth under the loader's shadow rule: even a user
        // entry that slipped past the load-time collision filter can
        // never override a built-in — lookup checks the registry
        // first, always.
        let mut palette = plain_scheme();
        palette.background = Some(rgb(0x00_0000));
        let user = [user_scheme("Dracula", palette)];
        let merged = merged_by_name("Dracula", &user).expect("resolves");
        assert_eq!(
            merged[NamedColor::Background],
            Some(rgb(0x28_2A36)),
            "the built-in palette wins, not the user entry's"
        );
    }

    #[test]
    fn merged_scheme_names_list_the_registry_then_the_user_schemes() {
        // The registry's order and identity stay untouched; the user
        // names follow as owned strings in the order given (the
        // loader's sorted order is the contract).
        let user = [
            user_scheme("zz-custom", plain_scheme()),
            user_scheme("aa-custom", plain_scheme()),
        ];
        let merged = merged_scheme_names(&user);
        let registry = scheme_names();
        assert_eq!(merged.len(), registry.len() + user.len());
        for (merged_name, registry_name) in merged.iter().take(registry.len()).zip(registry.iter())
        {
            assert_eq!(merged_name, registry_name);
        }
        assert_eq!(
            &merged[registry.len()..],
            &["zz-custom".to_string(), "aa-custom".to_string()]
        );
    }

    #[test]
    fn merged_scheme_previews_carry_the_chip_parity_over_user_schemes() {
        // The standing pin extends by construction: every merged
        // preview slot equals the slot `merged_by_name` serves the
        // live terminal — the same `From<&ColorScheme>` path for user
        // schemes and built-ins alike.
        let user = [user_scheme("mine", plain_scheme())];
        let previews = merged_scheme_previews(&user);
        let names = merged_scheme_names(&user);
        assert_eq!(previews.len(), names.len());
        for ((preview_name, _), list_name) in previews.iter().zip(names.iter()) {
            assert_eq!(preview_name, list_name);
        }
        for (name, preview) in previews {
            let colors = merged_by_name(&name, &user).expect("every merged name resolves");
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
}
