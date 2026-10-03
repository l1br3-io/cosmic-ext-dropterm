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

use alacritty_terminal::{
    term::color::Colors,
    vte::ansi::{NamedColor, Rgb},
};

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

/// The COSMIC Dark scheme as alacritty colors.
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
    foreground: Some(rgb(0x4C4F69)),
    background: Some(rgb(0xEFF1F5)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x5C5F77)),
        red: Some(rgb(0xD20F39)),
        green: Some(rgb(0x40A02B)),
        yellow: Some(rgb(0xDF8E1D)),
        blue: Some(rgb(0x1E66F5)),
        magenta: Some(rgb(0xEA76CB)),
        cyan: Some(rgb(0x179299)),
        white: Some(rgb(0xACB0BE)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x6C6F85)),
        red: Some(rgb(0xD20F39)),
        green: Some(rgb(0x40A02B)),
        yellow: Some(rgb(0xDF8E1D)),
        blue: Some(rgb(0x1E66F5)),
        magenta: Some(rgb(0xEA76CB)),
        cyan: Some(rgb(0x179299)),
        white: Some(rgb(0xBCC0CC)),
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
    foreground: Some(rgb(0xCDD6F4)),
    background: Some(rgb(0x1E1E2E)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x45475A)),
        red: Some(rgb(0xF38BA8)),
        green: Some(rgb(0xA6E3A1)),
        yellow: Some(rgb(0xF9E2AF)),
        blue: Some(rgb(0x89B4FA)),
        magenta: Some(rgb(0xF5C2E7)),
        cyan: Some(rgb(0x94E2D5)),
        white: Some(rgb(0xBAC2DE)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x585B70)),
        red: Some(rgb(0xF38BA8)),
        green: Some(rgb(0xA6E3A1)),
        yellow: Some(rgb(0xF9E2AF)),
        blue: Some(rgb(0x89B4FA)),
        magenta: Some(rgb(0xF5C2E7)),
        cyan: Some(rgb(0x94E2D5)),
        white: Some(rgb(0xA6ADC8)),
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
    foreground: Some(rgb(0xF8F8F2)),
    background: Some(rgb(0x282A36)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x000000)),
        red: Some(rgb(0xFF5555)),
        green: Some(rgb(0x50FA7B)),
        yellow: Some(rgb(0xF1FA8C)),
        blue: Some(rgb(0xBD93F9)),
        magenta: Some(rgb(0xFF79C6)),
        cyan: Some(rgb(0x8BE9FD)),
        white: Some(rgb(0xBBBBBB)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x555555)),
        red: Some(rgb(0xFF5555)),
        green: Some(rgb(0x50FA7B)),
        yellow: Some(rgb(0xF1FA8C)),
        blue: Some(rgb(0xCAA9FA)),
        magenta: Some(rgb(0xFF79C6)),
        cyan: Some(rgb(0x8BE9FD)),
        white: Some(rgb(0xFFFFFF)),
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
    foreground: Some(rgb(0xEBDBB2)),
    background: Some(rgb(0x282828)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x282828)),
        red: Some(rgb(0xCC241D)),
        green: Some(rgb(0x98971A)),
        yellow: Some(rgb(0xD79921)),
        blue: Some(rgb(0x458588)),
        magenta: Some(rgb(0xB16286)),
        cyan: Some(rgb(0x689D6A)),
        white: Some(rgb(0xA89984)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x928374)),
        red: Some(rgb(0xFB4934)),
        green: Some(rgb(0xB8BB26)),
        yellow: Some(rgb(0xFABD2F)),
        blue: Some(rgb(0x83A598)),
        magenta: Some(rgb(0xD3869B)),
        cyan: Some(rgb(0x8EC07C)),
        white: Some(rgb(0xEBDBB2)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

/// Gruvbox Light — `themes/gruvbox_light.toml` in
/// [alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme)
/// (repo Apache-2.0); design MIT (morhetz/gruvbox). Cross-checked
/// against the canonical medium-contrast palette: bg `#fbf1c7`, fg
/// `#3c3836`, bright yellow `#b57614` all match.
const GRUVBOX_LIGHT: ColorScheme = ColorScheme {
    foreground: Some(rgb(0x3C3836)),
    background: Some(rgb(0xFBF1C7)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0xFBF1C7)),
        red: Some(rgb(0xCC241D)),
        green: Some(rgb(0x98971A)),
        yellow: Some(rgb(0xD79921)),
        blue: Some(rgb(0x458588)),
        magenta: Some(rgb(0xB16286)),
        cyan: Some(rgb(0x689D6A)),
        white: Some(rgb(0x7C6F64)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x928374)),
        red: Some(rgb(0x9D0006)),
        green: Some(rgb(0x79740E)),
        yellow: Some(rgb(0xB57614)),
        blue: Some(rgb(0x076678)),
        magenta: Some(rgb(0x8F3F71)),
        cyan: Some(rgb(0x427B58)),
        white: Some(rgb(0x3C3836)),
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
    foreground: Some(rgb(0xD8DEE9)),
    background: Some(rgb(0x2E3440)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x3B4252)),
        red: Some(rgb(0xBF616A)),
        green: Some(rgb(0xA3BE8C)),
        yellow: Some(rgb(0xEBCB8B)),
        blue: Some(rgb(0x81A1C1)),
        magenta: Some(rgb(0xB48EAD)),
        cyan: Some(rgb(0x88C0D0)),
        white: Some(rgb(0xE5E9F0)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x4C566A)),
        red: Some(rgb(0xBF616A)),
        green: Some(rgb(0xA3BE8C)),
        yellow: Some(rgb(0xEBCB8B)),
        blue: Some(rgb(0x81A1C1)),
        magenta: Some(rgb(0xB48EAD)),
        cyan: Some(rgb(0x8FBCBB)),
        white: Some(rgb(0xECEFF4)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

/// Solarized Dark — `themes/solarized_dark.toml` in
/// [alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme)
/// (repo Apache-2.0); design MIT (Ethan Schoonover, solarized).
/// Cross-checked against the canonical palette: bg base03 `#002b36`,
/// fg base0 `#839496`, cyan `#2aa198`, orange `#cb4b16` all match.
const SOLARIZED_DARK: ColorScheme = ColorScheme {
    foreground: Some(rgb(0x839496)),
    background: Some(rgb(0x002B36)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x073642)),
        red: Some(rgb(0xDC322F)),
        green: Some(rgb(0x859900)),
        yellow: Some(rgb(0xB58900)),
        blue: Some(rgb(0x268BD2)),
        magenta: Some(rgb(0xD33682)),
        cyan: Some(rgb(0x2AA198)),
        white: Some(rgb(0xEEE8D5)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x002B36)),
        red: Some(rgb(0xCB4B16)),
        green: Some(rgb(0x586E75)),
        yellow: Some(rgb(0x657B83)),
        blue: Some(rgb(0x839496)),
        magenta: Some(rgb(0x6C71C4)),
        cyan: Some(rgb(0x93A1A1)),
        white: Some(rgb(0xFDF6E3)),
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
    foreground: Some(rgb(0x586E75)),
    background: Some(rgb(0xFDF6E3)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x073642)),
        red: Some(rgb(0xDC322F)),
        green: Some(rgb(0x859900)),
        yellow: Some(rgb(0xB58900)),
        blue: Some(rgb(0x268BD2)),
        magenta: Some(rgb(0xD33682)),
        cyan: Some(rgb(0x2AA198)),
        white: Some(rgb(0xEEE8D5)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x002B36)),
        red: Some(rgb(0xCB4B16)),
        green: Some(rgb(0x586E75)),
        yellow: Some(rgb(0x657B83)),
        blue: Some(rgb(0x839496)),
        magenta: Some(rgb(0x6C71C4)),
        cyan: Some(rgb(0x93A1A1)),
        white: Some(rgb(0xFDF6E3)),
    },
    dim: ColorSchemeAnsi::EMPTY,
};

/// Tokyo Night Day — alacritty extra shipped by the theme's own
/// upstream, [folke/tokyonight.nvim](https://github.com/folke/tokyonight.nvim)
/// `extras/alacritty/tokyonight_day.toml` (Apache-2.0); alacritty-theme
/// has no Day file. Cross-checked against the canonical palette: bg
/// `#e1e2e7`, fg `#3760bf`, cyan `#007197` all match.
const TOKYO_NIGHT_DAY: ColorScheme = ColorScheme {
    foreground: Some(rgb(0x3760BF)),
    background: Some(rgb(0xE1E2E7)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0xB4B5B9)),
        red: Some(rgb(0xF52A65)),
        green: Some(rgb(0x587539)),
        yellow: Some(rgb(0x8C6C3E)),
        blue: Some(rgb(0x2E7DE9)),
        magenta: Some(rgb(0x9854F1)),
        cyan: Some(rgb(0x007197)),
        white: Some(rgb(0x6172B0)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0xA1A6C5)),
        red: Some(rgb(0xFF4774)),
        green: Some(rgb(0x5C8524)),
        yellow: Some(rgb(0xA27629)),
        blue: Some(rgb(0x358AFF)),
        magenta: Some(rgb(0xA463FF)),
        cyan: Some(rgb(0x007EA8)),
        white: Some(rgb(0x3760BF)),
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
    foreground: Some(rgb(0xA9B1D6)),
    background: Some(rgb(0x24283B)),
    cursor: None,
    bright_foreground: None,
    dim_foreground: None,
    normal: ColorSchemeAnsi {
        black: Some(rgb(0x32344A)),
        red: Some(rgb(0xF7768E)),
        green: Some(rgb(0x9ECE6A)),
        yellow: Some(rgb(0xE0AF68)),
        blue: Some(rgb(0x7AA2F7)),
        magenta: Some(rgb(0xAD8EE6)),
        cyan: Some(rgb(0x449DAB)),
        white: Some(rgb(0x9699A8)),
    },
    bright: ColorSchemeAnsi {
        black: Some(rgb(0x444B6A)),
        red: Some(rgb(0xFF7A93)),
        green: Some(rgb(0xB9F27C)),
        yellow: Some(rgb(0xFF9E64)),
        blue: Some(rgb(0x7DA6FF)),
        magenta: Some(rgb(0xBB9AF7)),
        cyan: Some(rgb(0x0DB9D7)),
        white: Some(rgb(0xACB0D0)),
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

/// The registry's scheme names in registry order (COSMIC pair first,
/// the rest alphabetical) — the option list the settings dropdowns
/// feed from. Derived from the same `SCHEMES` table `by_name` reads,
/// so the picker can never offer a name lookup would reject.
pub fn scheme_names() -> Vec<&'static str> {
    SCHEMES.iter().map(|entry| entry.name).collect()
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
            dim: ColorSchemeAnsi::EMPTY,
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

    // --- byte-identity pins: the COSMIC pair's user-visible slots.
    // The route through `ColorScheme` must not move a single one. ---

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

    #[test]
    fn by_name_serves_every_registry_entry() {
        for entry in SCHEMES.iter() {
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
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x282A36)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xF8F8F2)));
        assert_eq!(colors[NamedColor::Blue], Some(rgb(0xBD93F9)));
    }

    #[test]
    fn solarized_dark_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Solarized Dark").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x002B36)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0x839496)));
        assert_eq!(colors[NamedColor::Cyan], Some(rgb(0x2AA198)));
    }

    #[test]
    fn solarized_light_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Solarized Light").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0xFDF6E3)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0x586E75)));
        assert_eq!(colors[NamedColor::Green], Some(rgb(0x859900)));
    }

    #[test]
    fn gruvbox_dark_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Gruvbox Dark").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x282828)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xEBDBB2)));
        assert_eq!(colors[NamedColor::BrightRed], Some(rgb(0xFB4934)));
    }

    #[test]
    fn gruvbox_light_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Gruvbox Light").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0xFBF1C7)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0x3C3836)));
        assert_eq!(colors[NamedColor::BrightYellow], Some(rgb(0xB57614)));
    }

    #[test]
    fn tokyo_night_storm_spot_checks_match_the_theme_toml() {
        let colors = by_name("Tokyo Night Storm").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x24283B)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xA9B1D6)));
        assert_eq!(colors[NamedColor::Blue], Some(rgb(0x7AA2F7)));
    }

    #[test]
    fn tokyo_night_day_spot_checks_match_the_canonical_extra() {
        let colors = by_name("Tokyo Night Day").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0xE1E2E7)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0x3760BF)));
        assert_eq!(colors[NamedColor::Cyan], Some(rgb(0x007197)));
    }

    #[test]
    fn nord_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Nord").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E3440)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xD8DEE9)));
        assert_eq!(colors[NamedColor::Cyan], Some(rgb(0x88C0D0)));
    }

    #[test]
    fn catppuccin_mocha_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Catppuccin Mocha").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x1E1E2E)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0xCDD6F4)));
        assert_eq!(colors[NamedColor::Blue], Some(rgb(0x89B4FA)));
    }

    #[test]
    fn catppuccin_latte_spot_checks_match_the_alacritty_theme_toml() {
        let colors = by_name("Catppuccin Latte").expect("registry entry");
        assert_eq!(colors[NamedColor::Background], Some(rgb(0xEFF1F5)));
        assert_eq!(colors[NamedColor::Foreground], Some(rgb(0x4C4F69)));
        assert_eq!(colors[NamedColor::Red], Some(rgb(0xD20F39)));
    }
}
