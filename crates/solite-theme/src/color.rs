//! Color values: terminal default, the 16 ANSI names, 256-color indexes, truecolor.

/// The 16 named ANSI colors.
///
/// These intentionally carry no fixed RGB value: rendering them emits the
/// classic SGR codes (30-37 / 90-97) so the user's terminal palette decides
/// what they look like. [`AnsiColor::fallback_rgb`] only exists for surfaces
/// that cannot defer to a terminal (HTML/CSS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AnsiColor {
    Black = 0,
    Red = 1,
    Green = 2,
    Yellow = 3,
    Blue = 4,
    Magenta = 5,
    Cyan = 6,
    White = 7,
    BrightBlack = 8,
    BrightRed = 9,
    BrightGreen = 10,
    BrightYellow = 11,
    BrightBlue = 12,
    BrightMagenta = 13,
    BrightCyan = 14,
    BrightWhite = 15,
}

impl AnsiColor {
    /// Palette index, 0-15.
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// True for the eight "bright"/high-intensity colors.
    pub const fn is_bright(self) -> bool {
        self.index() >= 8
    }

    /// Kebab-case name, used for CSS custom property names.
    pub const fn name(self) -> &'static str {
        match self {
            AnsiColor::Black => "black",
            AnsiColor::Red => "red",
            AnsiColor::Green => "green",
            AnsiColor::Yellow => "yellow",
            AnsiColor::Blue => "blue",
            AnsiColor::Magenta => "magenta",
            AnsiColor::Cyan => "cyan",
            AnsiColor::White => "white",
            AnsiColor::BrightBlack => "bright-black",
            AnsiColor::BrightRed => "bright-red",
            AnsiColor::BrightGreen => "bright-green",
            AnsiColor::BrightYellow => "bright-yellow",
            AnsiColor::BrightBlue => "bright-blue",
            AnsiColor::BrightMagenta => "bright-magenta",
            AnsiColor::BrightCyan => "bright-cyan",
            AnsiColor::BrightWhite => "bright-white",
        }
    }

    /// Build from a palette index, 0-15.
    pub const fn from_index(index: u8) -> Option<Self> {
        Some(match index {
            0 => AnsiColor::Black,
            1 => AnsiColor::Red,
            2 => AnsiColor::Green,
            3 => AnsiColor::Yellow,
            4 => AnsiColor::Blue,
            5 => AnsiColor::Magenta,
            6 => AnsiColor::Cyan,
            7 => AnsiColor::White,
            8 => AnsiColor::BrightBlack,
            9 => AnsiColor::BrightRed,
            10 => AnsiColor::BrightGreen,
            11 => AnsiColor::BrightYellow,
            12 => AnsiColor::BrightBlue,
            13 => AnsiColor::BrightMagenta,
            14 => AnsiColor::BrightCyan,
            15 => AnsiColor::BrightWhite,
            _ => return None,
        })
    }

    /// SGR parameter for this color as a foreground (30-37 / 90-97).
    pub const fn fg_sgr(self) -> u8 {
        if self.is_bright() {
            90 + (self.index() - 8)
        } else {
            30 + self.index()
        }
    }

    /// SGR parameter for this color as a background (40-47 / 100-107).
    pub const fn bg_sgr(self) -> u8 {
        if self.is_bright() {
            100 + (self.index() - 8)
        } else {
            40 + self.index()
        }
    }

    /// Approximate RGB, for surfaces with no terminal palette to defer to
    /// (HTML). These are the traditional xterm defaults and are only ever used
    /// as a CSS fallback behind a `--solite-ansi-*` custom property.
    pub const fn fallback_rgb(self) -> (u8, u8, u8) {
        match self {
            AnsiColor::Black => (0x00, 0x00, 0x00),
            AnsiColor::Red => (0xcd, 0x00, 0x00),
            AnsiColor::Green => (0x00, 0xcd, 0x00),
            AnsiColor::Yellow => (0xcd, 0xcd, 0x00),
            AnsiColor::Blue => (0x00, 0x00, 0xee),
            AnsiColor::Magenta => (0xcd, 0x00, 0xcd),
            AnsiColor::Cyan => (0x00, 0xcd, 0xcd),
            AnsiColor::White => (0xe5, 0xe5, 0xe5),
            AnsiColor::BrightBlack => (0x7f, 0x7f, 0x7f),
            AnsiColor::BrightRed => (0xff, 0x00, 0x00),
            AnsiColor::BrightGreen => (0x00, 0xff, 0x00),
            AnsiColor::BrightYellow => (0xff, 0xff, 0x00),
            AnsiColor::BrightBlue => (0x5c, 0x5c, 0xff),
            AnsiColor::BrightMagenta => (0xff, 0x00, 0xff),
            AnsiColor::BrightCyan => (0x00, 0xff, 0xff),
            AnsiColor::BrightWhite => (0xff, 0xff, 0xff),
        }
    }
}

/// A color, in one of the four forms a theme can express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColorValue {
    /// The terminal's own default foreground/background.
    ///
    /// This is a sentinel, not a color: it emits **no** SGR parameter at all
    /// (so the surrounding terminal colors show through), converts to
    /// [`ratatui::style::Color::Reset`], maps to `None` in a
    /// `termcolor::ColorSpec`, and renders as `currentColor` in CSS.
    #[default]
    Default,
    /// One of the 16 named ANSI colors — resolved by the user's terminal.
    Ansi(AnsiColor),
    /// A 256-color palette index.
    Indexed(u8),
    /// 24-bit truecolor.
    Rgb(u8, u8, u8),
}

impl ColorValue {
    /// Truecolor from a packed `0xRRGGBB` integer.
    pub const fn from_hex(hex: u32) -> Self {
        ColorValue::Rgb(
            ((hex >> 16) & 0xFF) as u8,
            ((hex >> 8) & 0xFF) as u8,
            (hex & 0xFF) as u8,
        )
    }

    /// Parse `#rgb`, `#rrggbb`, `rgb` or `rrggbb` (case-insensitive).
    pub fn parse_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        let nibble = |c: u8| -> Option<u32> {
            Some(match c {
                b'0'..=b'9' => (c - b'0') as u32,
                b'a'..=b'f' => (c - b'a' + 10) as u32,
                b'A'..=b'F' => (c - b'A' + 10) as u32,
                _ => return None,
            })
        };
        let bytes = s.as_bytes();
        match bytes.len() {
            3 => {
                let mut out = 0u32;
                for &b in bytes {
                    let n = nibble(b)?;
                    out = (out << 8) | (n << 4) | n;
                }
                Some(ColorValue::from_hex(out))
            }
            6 => {
                let mut out = 0u32;
                for &b in bytes {
                    out = (out << 4) | nibble(b)?;
                }
                Some(ColorValue::from_hex(out))
            }
            _ => None,
        }
    }

    /// `#RRGGBB` for truecolor values; falls back to the approximate RGB for
    /// ANSI/indexed values and `None` for [`ColorValue::Default`].
    pub fn to_hex_string(&self) -> Option<String> {
        let (r, g, b) = self.approximate_rgb()?;
        Some(format!("#{r:02X}{g:02X}{b:02X}"))
    }

    /// Best-effort RGB. `None` only for [`ColorValue::Default`].
    pub const fn approximate_rgb(&self) -> Option<(u8, u8, u8)> {
        match *self {
            ColorValue::Default => None,
            ColorValue::Ansi(c) => Some(c.fallback_rgb()),
            ColorValue::Indexed(i) => Some(xterm256_rgb(i)),
            ColorValue::Rgb(r, g, b) => Some((r, g, b)),
        }
    }

    /// SGR parameters for using this color as a foreground.
    ///
    /// Empty for [`ColorValue::Default`] — the whole point of the sentinel.
    pub fn fg_sgr_params(&self) -> Vec<String> {
        match *self {
            ColorValue::Default => vec![],
            ColorValue::Ansi(c) => vec![c.fg_sgr().to_string()],
            ColorValue::Indexed(i) => vec!["38".into(), "5".into(), i.to_string()],
            ColorValue::Rgb(r, g, b) => vec![
                "38".into(),
                "2".into(),
                r.to_string(),
                g.to_string(),
                b.to_string(),
            ],
        }
    }

    /// SGR parameters for using this color as a background. Empty for
    /// [`ColorValue::Default`].
    pub fn bg_sgr_params(&self) -> Vec<String> {
        match *self {
            ColorValue::Default => vec![],
            ColorValue::Ansi(c) => vec![c.bg_sgr().to_string()],
            ColorValue::Indexed(i) => vec!["48".into(), "5".into(), i.to_string()],
            ColorValue::Rgb(r, g, b) => vec![
                "48".into(),
                "2".into(),
                r.to_string(),
                g.to_string(),
                b.to_string(),
            ],
        }
    }

    /// A CSS color value.
    ///
    /// - [`ColorValue::Default`] → `currentColor`, so HTML surfaces inherit the
    ///   host page's foreground exactly the way `Default` inherits the
    ///   terminal's.
    /// - [`ColorValue::Ansi`] / [`ColorValue::Indexed`] → a
    ///   `var(--solite-ansi-NAME, #fallback)` reference, so an embedding page
    ///   can re-point the palette while a standalone page still renders.
    /// - [`ColorValue::Rgb`] → a plain `#RRGGBB` literal.
    pub fn to_css(&self) -> String {
        match *self {
            ColorValue::Default => "currentColor".to_string(),
            ColorValue::Ansi(c) => {
                let (r, g, b) = c.fallback_rgb();
                format!("var(--solite-ansi-{}, #{r:02x}{g:02x}{b:02x})", c.name())
            }
            ColorValue::Indexed(i) => match AnsiColor::from_index(i) {
                Some(c) => ColorValue::Ansi(c).to_css(),
                None => {
                    let (r, g, b) = xterm256_rgb(i);
                    format!("var(--solite-ansi-{i}, #{r:02x}{g:02x}{b:02x})")
                }
            },
            ColorValue::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// RGB for an xterm 256-color palette index.
///
/// 0-15 use the classic ANSI fallbacks, 16-231 the 6x6x6 cube, 232-255 the
/// grayscale ramp.
pub const fn xterm256_rgb(index: u8) -> (u8, u8, u8) {
    if index < 16 {
        match AnsiColor::from_index(index) {
            Some(c) => c.fallback_rgb(),
            None => (0, 0, 0),
        }
    } else if index < 232 {
        let i = index - 16;
        (
            cube_level(i / 36),
            cube_level((i % 36) / 6),
            cube_level(i % 6),
        )
    } else {
        let v = 8 + 10 * (index - 232);
        (v, v, v)
    }
}

/// One axis of the xterm 6x6x6 color cube.
const fn cube_level(v: u8) -> u8 {
    if v == 0 {
        0
    } else {
        55 + 40 * v
    }
}
