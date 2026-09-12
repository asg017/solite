//! Feature-gated conversions into the styling types of other crates.

#[cfg(feature = "ratatui")]
mod ratatui_impl {
    use crate::color::{AnsiColor, ColorValue};
    use crate::style::Style;
    use ratatui::style::{Color, Modifier, Style as RStyle};

    impl From<AnsiColor> for Color {
        fn from(value: AnsiColor) -> Self {
            match value {
                AnsiColor::Black => Color::Black,
                AnsiColor::Red => Color::Red,
                AnsiColor::Green => Color::Green,
                AnsiColor::Yellow => Color::Yellow,
                AnsiColor::Blue => Color::Blue,
                AnsiColor::Magenta => Color::Magenta,
                AnsiColor::Cyan => Color::Cyan,
                AnsiColor::White => Color::Gray,
                AnsiColor::BrightBlack => Color::DarkGray,
                AnsiColor::BrightRed => Color::LightRed,
                AnsiColor::BrightGreen => Color::LightGreen,
                AnsiColor::BrightYellow => Color::LightYellow,
                AnsiColor::BrightBlue => Color::LightBlue,
                AnsiColor::BrightMagenta => Color::LightMagenta,
                AnsiColor::BrightCyan => Color::LightCyan,
                AnsiColor::BrightWhite => Color::White,
            }
        }
    }

    impl From<ColorValue> for Color {
        fn from(value: ColorValue) -> Self {
            match value {
                // The terminal-default sentinel: ratatui's Reset restores the
                // terminal's own fg/bg.
                ColorValue::Default => Color::Reset,
                ColorValue::Ansi(c) => c.into(),
                ColorValue::Indexed(i) => Color::Indexed(i),
                ColorValue::Rgb(r, g, b) => Color::Rgb(r, g, b),
            }
        }
    }

    impl From<&Style> for RStyle {
        fn from(value: &Style) -> Self {
            let mut style = RStyle::default().fg(value.fg.into());
            if let Some(bg) = value.bg {
                style = style.bg(bg.into());
            }
            let mut modifiers = Modifier::empty();
            if value.bold {
                modifiers |= Modifier::BOLD;
            }
            if value.dim {
                modifiers |= Modifier::DIM;
            }
            if value.italic {
                modifiers |= Modifier::ITALIC;
            }
            if value.underline {
                modifiers |= Modifier::UNDERLINED;
            }
            if !modifiers.is_empty() {
                style = style.add_modifier(modifiers);
            }
            style
        }
    }

    impl From<Style> for RStyle {
        fn from(value: Style) -> Self {
            (&value).into()
        }
    }
}

#[cfg(feature = "termcolor")]
mod termcolor_impl {
    use crate::color::{AnsiColor, ColorValue};
    use crate::style::Style;
    use termcolor::{Color, ColorSpec};

    impl From<AnsiColor> for Color {
        fn from(value: AnsiColor) -> Self {
            match value {
                AnsiColor::Black => Color::Black,
                AnsiColor::Red => Color::Red,
                AnsiColor::Green => Color::Green,
                AnsiColor::Yellow => Color::Yellow,
                AnsiColor::Blue => Color::Blue,
                AnsiColor::Magenta => Color::Magenta,
                AnsiColor::Cyan => Color::Cyan,
                AnsiColor::White => Color::White,
                // termcolor has no bright variants; the 256-color palette
                // indexes 8..=15 are exactly the bright ANSI colors.
                bright => Color::Ansi256(bright.index()),
            }
        }
    }

    impl From<ColorValue> for Option<Color> {
        fn from(value: ColorValue) -> Self {
            match value {
                // No color at all — termcolor leaves the terminal default.
                ColorValue::Default => None,
                ColorValue::Ansi(c) => Some(c.into()),
                ColorValue::Indexed(i) => Some(Color::Ansi256(i)),
                ColorValue::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
            }
        }
    }

    impl From<&Style> for ColorSpec {
        fn from(value: &Style) -> Self {
            let mut spec = ColorSpec::new();
            let fg: Option<Color> = value.fg.into();
            spec.set_fg(fg);
            if let Some(bg) = value.bg {
                let bg: Option<Color> = bg.into();
                spec.set_bg(bg);
            }
            spec.set_bold(value.bold);
            spec.set_dimmed(value.dim);
            spec.set_italic(value.italic);
            spec.set_underline(value.underline);
            spec
        }
    }

    impl From<Style> for ColorSpec {
        fn from(value: Style) -> Self {
            (&value).into()
        }
    }
}
