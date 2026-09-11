use crate::*;

// ---- ColorValue: the four forms ---------------------------------------

#[test]
fn default_emits_no_sgr() {
    assert!(ColorValue::Default.fg_sgr_params().is_empty());
    assert!(ColorValue::Default.bg_sgr_params().is_empty());
    assert_eq!(Style::DEFAULT.ansi_prefix(), "");
    assert!(Style::DEFAULT.is_inert());
    assert_eq!(Style::DEFAULT.paint("abc"), "abc");
}

#[test]
fn ansi_emits_named_sgr() {
    assert_eq!(Style::ansi(AnsiColor::Red).ansi_prefix(), "\x1b[31m");
    assert_eq!(Style::ansi(AnsiColor::White).ansi_prefix(), "\x1b[37m");
    assert_eq!(Style::ansi(AnsiColor::BrightBlack).ansi_prefix(), "\x1b[90m");
    assert_eq!(Style::ansi(AnsiColor::BrightWhite).ansi_prefix(), "\x1b[97m");
    assert_eq!(
        Style::DEFAULT
            .with_bg(ColorValue::Ansi(AnsiColor::Blue))
            .ansi_prefix(),
        "\x1b[44m"
    );
    assert_eq!(
        Style::DEFAULT
            .with_bg(ColorValue::Ansi(AnsiColor::BrightCyan))
            .ansi_prefix(),
        "\x1b[106m"
    );
}

#[test]
fn indexed_emits_256_color_sgr() {
    assert_eq!(Style::new(ColorValue::Indexed(42)).ansi_prefix(), "\x1b[38;5;42m");
    assert_eq!(
        Style::DEFAULT.with_bg(ColorValue::Indexed(42)).ansi_prefix(),
        "\x1b[48;5;42m"
    );
}

#[test]
fn rgb_emits_truecolor_sgr() {
    assert_eq!(
        Style::new(ColorValue::Rgb(255, 128, 64)).ansi_prefix(),
        "\x1b[38;2;255;128;64m"
    );
    assert_eq!(
        Style::DEFAULT
            .with_bg(ColorValue::Rgb(30, 30, 46))
            .ansi_prefix(),
        "\x1b[48;2;30;30;46m"
    );
    // Matches the escape the old solite-table Color::to_ansi_fg produced.
    assert_eq!(
        Style::hex(0xfab387).ansi_prefix(),
        "\x1b[38;2;250;179;135m"
    );
}

#[test]
fn modifiers_precede_colors() {
    let s = Style::ansi(AnsiColor::Magenta)
        .bold()
        .dim()
        .italic()
        .underline()
        .with_bg(ColorValue::Rgb(1, 2, 3));
    assert_eq!(s.ansi_prefix(), "\x1b[1;2;3;4;35;48;2;1;2;3m");
}

#[test]
fn modifiers_alone_are_not_inert() {
    let s = Style::DEFAULT.bold();
    assert_eq!(s.ansi_prefix(), "\x1b[1m");
    assert_eq!(s.paint("x"), "\x1b[1mx\x1b[0m");
}

#[test]
fn paint_wraps_with_reset() {
    assert_eq!(
        Style::ansi(AnsiColor::Green).paint("ok"),
        "\x1b[32mok\x1b[0m"
    );
    assert_eq!(RESET, "\x1b[0m");
}

// ---- hex parsing -------------------------------------------------------

#[test]
fn from_hex_unpacks_channels() {
    assert_eq!(ColorValue::from_hex(0xfab387), ColorValue::Rgb(0xfa, 0xb3, 0x87));
    assert_eq!(ColorValue::from_hex(0x000000), ColorValue::Rgb(0, 0, 0));
    assert_eq!(ColorValue::from_hex(0xffffff), ColorValue::Rgb(255, 255, 255));
}

#[test]
fn parse_hex_forms() {
    assert_eq!(ColorValue::parse_hex("#fab387"), Some(ColorValue::Rgb(0xfa, 0xb3, 0x87)));
    assert_eq!(ColorValue::parse_hex("FAB387"), Some(ColorValue::Rgb(0xfa, 0xb3, 0x87)));
    assert_eq!(ColorValue::parse_hex("#abc"), Some(ColorValue::Rgb(0xaa, 0xbb, 0xcc)));
    assert_eq!(ColorValue::parse_hex("#ffff"), None);
    assert_eq!(ColorValue::parse_hex("#gggggg"), None);
    assert_eq!(ColorValue::parse_hex(""), None);
}

#[test]
fn hex_string_round_trips() {
    let c = ColorValue::from_hex(0xfab387);
    assert_eq!(c.to_hex_string().as_deref(), Some("#FAB387"));
    assert_eq!(ColorValue::parse_hex(&c.to_hex_string().unwrap()), Some(c));
    assert_eq!(ColorValue::Default.to_hex_string(), None);
}

#[test]
fn xterm256_palette() {
    assert_eq!(xterm256_rgb(0), (0, 0, 0));
    assert_eq!(xterm256_rgb(16), (0, 0, 0));
    assert_eq!(xterm256_rgb(231), (255, 255, 255));
    assert_eq!(xterm256_rgb(232), (8, 8, 8));
    assert_eq!(xterm256_rgb(255), (238, 238, 238));
}

// ---- CSS ---------------------------------------------------------------

#[test]
fn css_for_each_color_form() {
    assert_eq!(ColorValue::Default.to_css(), "currentColor");
    assert_eq!(
        ColorValue::Ansi(AnsiColor::Red).to_css(),
        "var(--solite-ansi-red, #cd0000)"
    );
    // Indexed values inside 0..16 reuse the named ANSI variables.
    assert_eq!(
        ColorValue::Indexed(1).to_css(),
        "var(--solite-ansi-red, #cd0000)"
    );
    assert_eq!(
        ColorValue::Indexed(232).to_css(),
        "var(--solite-ansi-232, #080808)"
    );
    assert_eq!(ColorValue::Rgb(0xfa, 0xb3, 0x87).to_css(), "#fab387");
}

#[test]
fn css_for_styles() {
    assert_eq!(Style::DEFAULT.to_css(), "color: currentColor");
    assert_eq!(
        Style::hex(0xcba6f7).bold().to_css(),
        "color: #cba6f7; font-weight: bold"
    );
    assert_eq!(
        Style::hex(0xcdd6f4).with_bg_hex(0x1e1e2e).to_css(),
        "color: #cdd6f4; background-color: #1e1e2e"
    );
}

// ---- Themes ------------------------------------------------------------

#[test]
fn terminal_theme_uses_no_truecolor() {
    let t = Theme::terminal();
    for name in Theme::ROLE_NAMES {
        let style = t.role(name).unwrap_or_else(|| panic!("missing role {name}"));
        assert!(
            !matches!(style.fg, ColorValue::Rgb(..)),
            "role {name} fg is truecolor"
        );
        if let Some(bg) = style.bg {
            assert!(!matches!(bg, ColorValue::Rgb(..)), "role {name} bg is truecolor");
        }
    }
}

#[test]
fn terminal_theme_leaves_background_to_the_terminal() {
    let t = Theme::terminal();
    assert_eq!(t.background, Style::DEFAULT);
    assert_eq!(t.text.fg, ColorValue::Default);
    assert_eq!(t.integer.fg, ColorValue::Ansi(AnsiColor::Yellow));
    assert_eq!(t.string_literal.fg, ColorValue::Ansi(AnsiColor::Green));
    assert_eq!(t.keyword.fg, ColorValue::Ansi(AnsiColor::Magenta));
    assert_eq!(t.comment.fg, ColorValue::Ansi(AnsiColor::BrightBlack));
    assert_eq!(t.success.fg, ColorValue::Ansi(AnsiColor::Green));
    assert_eq!(t.error.fg, ColorValue::Ansi(AnsiColor::Red));
    assert!(t.border.dim);
    assert!(t.footer.dim);
}

#[test]
fn mocha_theme_matches_previous_hex_values() {
    let t = Theme::catppuccin_mocha();
    // from solite-table/src/theme.rs
    assert_eq!(t.null.fg, ColorValue::from_hex(0xbac2de));
    assert_eq!(t.integer.fg, ColorValue::from_hex(0xfab387));
    assert_eq!(t.double.fg, ColorValue::from_hex(0xfab387));
    assert_eq!(t.text.fg, ColorValue::from_hex(0xcdd6f4));
    assert_eq!(t.blob.fg, ColorValue::from_hex(0x94e2d5));
    assert_eq!(t.json_key.fg, ColorValue::from_hex(0x89b4fa));
    assert_eq!(t.json_string.fg, ColorValue::from_hex(0xa6e3a1));
    assert_eq!(t.json_number.fg, ColorValue::from_hex(0xfab387));
    assert_eq!(t.json_boolean.fg, ColorValue::from_hex(0xeba0ac));
    assert_eq!(t.border.fg, ColorValue::from_hex(0x6c7086));
    assert_eq!(t.header.fg, ColorValue::from_hex(0xcdd6f4));
    assert_eq!(t.footer.fg, ColorValue::from_hex(0xa6adc8));

    // from repl/highlighter.rs SqlTheme
    assert_eq!(t.keyword.fg, ColorValue::from_hex(0xcba6f7));
    assert!(t.keyword.bold);
    assert_eq!(t.dot_command.fg, ColorValue::from_hex(0x89b4fa));
    assert_eq!(t.comment.fg, ColorValue::from_hex(0x9399b2));
    assert_eq!(t.parameter.fg, ColorValue::from_hex(0xeba0ac));
    assert_eq!(t.type_name.fg, ColorValue::from_hex(0xf9e2af));
    assert_eq!(t.string_literal.fg, ColorValue::from_hex(0xa6e3a1));
    assert_eq!(t.function.fg, ColorValue::from_hex(0x89b4fa));
    assert_eq!(t.function_builtin.fg, ColorValue::from_hex(0x89b4fa));
    assert!(t.function_builtin.bold);
    assert_eq!(t.punctuation.fg, ColorValue::from_hex(0x9399b2));
    assert_eq!(t.operator.fg, ColorValue::from_hex(0x89dceb));
    assert_eq!(t.number.fg, ColorValue::from_hex(0xfab387));

    // from tui/tui_theme.rs TuiTheme
    assert_eq!(t.background.bg, Some(ColorValue::from_hex(0x1e1e2e)));
    assert_eq!(t.keycap.fg, ColorValue::from_hex(0xa6e3a1));
    assert_eq!(t.selection.bg, Some(ColorValue::from_hex(0x313244)));
    assert_eq!(t.highlight.bg, Some(ColorValue::from_hex(0x11111b)));
    assert_eq!(t.highlight.fg, ColorValue::from_hex(0xcdd6f4));
    assert_eq!(t.header_selected.bg, Some(ColorValue::from_hex(0x6c7086)));
}

#[test]
fn default_theme_is_the_terminal_theme() {
    assert_eq!(Theme::default(), Theme::terminal());
}

#[test]
fn role_lookup_covers_every_name() {
    let t = Theme::terminal();
    assert_eq!(Theme::ROLE_NAMES.len(), 32);
    for name in Theme::ROLE_NAMES {
        assert!(t.role(name).is_some(), "role {name} not reachable by name");
    }
    assert!(t.role("not_a_role").is_none());
}

// ---- feature-gated conversions ----------------------------------------

#[cfg(feature = "ratatui")]
mod ratatui_tests {
    use crate::*;
    use ratatui::style::{Color, Modifier, Style as RStyle};

    #[test]
    fn default_becomes_reset() {
        let s: RStyle = (&Style::DEFAULT).into();
        assert_eq!(s.fg, Some(Color::Reset));
        assert_eq!(s.bg, None);
        assert_eq!(s.add_modifier, Modifier::empty());
    }

    #[test]
    fn all_four_forms_convert() {
        assert_eq!(Color::from(ColorValue::Default), Color::Reset);
        assert_eq!(Color::from(ColorValue::Ansi(AnsiColor::Red)), Color::Red);
        assert_eq!(
            Color::from(ColorValue::Ansi(AnsiColor::BrightBlack)),
            Color::DarkGray
        );
        assert_eq!(Color::from(ColorValue::Indexed(42)), Color::Indexed(42));
        assert_eq!(Color::from(ColorValue::Rgb(1, 2, 3)), Color::Rgb(1, 2, 3));
    }

    #[test]
    fn modifiers_and_bg_convert() {
        let s: RStyle = (&Style::hex(0xfab387)
            .with_bg_hex(0x1e1e2e)
            .bold()
            .italic())
            .into();
        assert_eq!(s.fg, Some(Color::Rgb(0xfa, 0xb3, 0x87)));
        assert_eq!(s.bg, Some(Color::Rgb(0x1e, 0x1e, 0x2e)));
        assert!(s.add_modifier.contains(Modifier::BOLD));
        assert!(s.add_modifier.contains(Modifier::ITALIC));
        assert!(!s.add_modifier.contains(Modifier::DIM));
    }
}

#[cfg(feature = "termcolor")]
mod termcolor_tests {
    use crate::*;
    use termcolor::{Color, ColorSpec};

    #[test]
    fn default_becomes_no_color() {
        let spec: ColorSpec = (&Style::DEFAULT).into();
        assert_eq!(spec.fg(), None);
        assert_eq!(spec.bg(), None);
        assert!(!spec.bold());
    }

    #[test]
    fn all_four_forms_convert() {
        let fg = |c: ColorValue| -> Option<Color> {
            let spec: ColorSpec = (&Style::new(c)).into();
            spec.fg().cloned()
        };
        assert_eq!(fg(ColorValue::Default), None);
        assert_eq!(fg(ColorValue::Ansi(AnsiColor::Red)), Some(Color::Red));
        assert_eq!(
            fg(ColorValue::Ansi(AnsiColor::BrightBlack)),
            Some(Color::Ansi256(8))
        );
        assert_eq!(fg(ColorValue::Indexed(42)), Some(Color::Ansi256(42)));
        assert_eq!(fg(ColorValue::Rgb(1, 2, 3)), Some(Color::Rgb(1, 2, 3)));
    }

    #[test]
    fn modifiers_and_bg_convert() {
        let spec: ColorSpec = (&Style::hex(0xcba6f7).with_bg_hex(0x1e1e2e).bold()).into();
        assert_eq!(spec.fg(), Some(&Color::Rgb(0xcb, 0xa6, 0xf7)));
        assert_eq!(spec.bg(), Some(&Color::Rgb(0x1e, 0x1e, 0x2e)));
        assert!(spec.bold());
    }
}
