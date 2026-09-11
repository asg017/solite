//! Shared color theming for Solite.
//!
//! One [`ColorValue`] type, one [`Style`] type, and one semantic-role
//! [`Theme`], used by every Solite surface: the table renderer, the REPL
//! highlighter, the TUI, and HTML/Jupyter output.
//!
//! The key idea is [`ColorValue::Default`]: a sentinel meaning "whatever the
//! terminal already uses". It emits no SGR parameter, becomes
//! `ratatui::style::Color::Reset`, becomes `None` in a `termcolor::ColorSpec`,
//! and renders as `currentColor` in CSS. That makes [`Theme::terminal`] — the
//! default theme, written entirely in ANSI-16 names and `Default` — legible in
//! any terminal, light or dark, with no configuration.
//!
//! ```
//! use solite_theme::{Theme, ColorValue, Style};
//!
//! let theme = Theme::terminal();
//! assert_eq!(theme.integer.ansi_prefix(), "\x1b[33m");
//! assert_eq!(theme.text.paint("hello"), "hello"); // inert: no escapes
//!
//! let mocha = Theme::catppuccin_mocha();
//! assert_eq!(mocha.integer.fg, ColorValue::from_hex(0xfab387));
//! assert_eq!(Style::DEFAULT.to_css(), "color: currentColor");
//! ```
//!
//! ## Features
//!
//! - `ratatui` — `impl From<&Style> for ratatui::style::Style`
//! - `termcolor` — `impl From<&Style> for termcolor::ColorSpec`
//!
//! Both are off by default so dependency-light crates (solite-table) can use
//! the core types without pulling a TUI framework in.

mod color;
mod convert;
mod style;
mod theme;

pub use color::{xterm256_rgb, AnsiColor, ColorValue};
pub use style::{Style, RESET};
pub use theme::{catppuccin_mocha_palette, Theme};

#[cfg(test)]
mod tests;
