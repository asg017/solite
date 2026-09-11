//! A styled role: a foreground color, an optional background, and modifiers.

use crate::color::{AnsiColor, ColorValue};

/// The ANSI "reset everything" sequence.
pub const RESET: &str = "\x1b[0m";

/// A foreground color plus optional background and text modifiers.
///
/// A `Style` whose `fg` is [`ColorValue::Default`], whose `bg` is `None`, and
/// which sets no modifiers is *inert*: [`Style::ansi_prefix`] returns an empty
/// string and [`Style::paint`] returns the input untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Style {
    pub fg: ColorValue,
    pub bg: Option<ColorValue>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Style {
    /// The inert style: terminal default foreground, no background, no
    /// modifiers.
    pub const DEFAULT: Style = Style {
        fg: ColorValue::Default,
        bg: None,
        bold: false,
        dim: false,
        italic: false,
        underline: false,
    };

    /// A style with just a foreground color.
    pub const fn new(fg: ColorValue) -> Self {
        Style {
            fg,
            ..Style::DEFAULT
        }
    }

    /// A style with a foreground from a packed `0xRRGGBB` integer.
    pub const fn hex(hex: u32) -> Self {
        Style::new(ColorValue::from_hex(hex))
    }

    /// A style with a foreground from one of the 16 named ANSI colors.
    pub const fn ansi(color: AnsiColor) -> Self {
        Style::new(ColorValue::Ansi(color))
    }

    /// Set the background color.
    pub const fn with_bg(mut self, bg: ColorValue) -> Self {
        self.bg = Some(bg);
        self
    }

    /// Set the background color from a packed `0xRRGGBB` integer.
    pub const fn with_bg_hex(mut self, hex: u32) -> Self {
        self.bg = Some(ColorValue::from_hex(hex));
        self
    }

    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub const fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    pub const fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub const fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    /// True when this style would emit nothing at all.
    pub fn is_inert(&self) -> bool {
        self.ansi_params().is_empty()
    }

    fn ansi_params(&self) -> Vec<String> {
        let mut params: Vec<String> = Vec::new();
        if self.bold {
            params.push("1".into());
        }
        if self.dim {
            params.push("2".into());
        }
        if self.italic {
            params.push("3".into());
        }
        if self.underline {
            params.push("4".into());
        }
        params.extend(self.fg.fg_sgr_params());
        if let Some(bg) = self.bg {
            params.extend(bg.bg_sgr_params());
        }
        params
    }

    /// The ANSI SGR prefix for this style.
    ///
    /// Returns an empty string when the style is inert, so callers can cheaply
    /// skip emitting a matching [`RESET`].
    pub fn ansi_prefix(&self) -> String {
        let params = self.ansi_params();
        if params.is_empty() {
            String::new()
        } else {
            format!("\x1b[{}m", params.join(";"))
        }
    }

    /// Wrap `s` in this style's SGR prefix and a [`RESET`].
    ///
    /// Returns `s` unchanged when the style is inert — no stray escapes in
    /// plain output.
    pub fn paint(&self, s: &str) -> String {
        let prefix = self.ansi_prefix();
        if prefix.is_empty() {
            s.to_string()
        } else {
            format!("{prefix}{s}{RESET}")
        }
    }

    /// CSS declarations for this style, e.g.
    /// `color: #fab387; font-weight: bold`.
    ///
    /// Always emits `color`, so a `Default` foreground explicitly re-states
    /// `currentColor` rather than silently inheriting something else.
    pub fn to_css(&self) -> String {
        let mut decls = vec![format!("color: {}", self.fg.to_css())];
        if let Some(bg) = self.bg {
            decls.push(format!("background-color: {}", bg.to_css()));
        }
        if self.bold {
            decls.push("font-weight: bold".into());
        }
        if self.dim {
            decls.push("opacity: 0.7".into());
        }
        if self.italic {
            decls.push("font-style: italic".into());
        }
        if self.underline {
            decls.push("text-decoration: underline".into());
        }
        decls.join("; ")
    }
}

impl From<ColorValue> for Style {
    fn from(value: ColorValue) -> Self {
        Style::new(value)
    }
}

impl From<AnsiColor> for Style {
    fn from(value: AnsiColor) -> Self {
        Style::ansi(value)
    }
}
