/**
 * 1. REPL colors (syntax highlighting, prompts, etc)
 * 2. TUI colors 
 * 3. Jupyter outputs?

 */
use std::sync::LazyLock;

use color_eyre::owo_colors;


#[derive(Clone)]
pub struct SoliteColor {
  red: u8,
  green: u8,
  blue: u8,
}

impl SoliteColor {
  pub fn new(red: u8, green: u8, blue: u8) -> Self {
    Self { red, green, blue }
  }
  pub fn hex(value: u32) -> Self {
    let red = ((value >> 16) & 0xFF) as u8;
    let green = ((value >> 8) & 0xFF) as u8;
    let blue = (value & 0xFF) as u8;
    Self { red, green, blue }
  }

  pub fn to_hex_string(&self) -> String {
    format!("#{:02X}{:02X}{:02X}", self.red, self.green, self.blue)
  }
}

impl Into<ratatui::style::Color> for SoliteColor {
  fn into(self) -> ratatui::style::Color {
    ratatui::style::Color::Rgb(self.red, self.green, self.blue)
  }
}

impl Into<termcolor::Color> for SoliteColor {
  fn into(self) -> termcolor::Color {
    termcolor::Color::Rgb(self.red, self.green, self.blue)
  }
}

impl Into<owo_colors::Rgb> for SoliteColor {
  fn into(self) -> owo_colors::Rgb {
    owo_colors::Rgb(self.red, self.green, self.blue)
  }
}

impl Into<crossterm::style::Color> for SoliteColor {
  fn into(self) -> crossterm::style::Color {
    crossterm::style::Color::Rgb { r: self.red, g: self.green, b: self.blue }
  }
} 

pub(crate) mod ctp_mocha_colors {
  use super::SoliteColor;
  use std::sync::LazyLock;

  // Macro to define color constants more concisely
  macro_rules! define_color {
    ($name:ident, $hex:literal) => {
      pub static $name: LazyLock<SoliteColor> = LazyLock::new(|| SoliteColor::hex($hex));
    };
  }
  define_color!(ROSEWATER, 0xf5e0dc);
  define_color!(FLAMINGO, 0xf2cdcd);
  define_color!(PINK, 0xf5c2e7);
  define_color!(MAUVE, 0xcba6f7);
  define_color!(RED, 0xf38ba8);
  define_color!(MAROON, 0xeba0ac);
  define_color!(PEACH, 0xfab387);
  define_color!(YELLOW, 0xf9e2af);
  define_color!(GREEN, 0xa6e3a1);
  define_color!(TEAL, 0x94e2d5);
  define_color!(SKY, 0x89dceb);
  define_color!(SAPPHIRE, 0x74c7ec);
  define_color!(BLUE, 0x89b4fa);
  define_color!(LAVENDER, 0xb4befe);
  define_color!(TEXT, 0xcdd6f4);
  define_color!(SUBTEXT1, 0xbac2de);
  define_color!(SUBTEXT0, 0xa6adc8);
  define_color!(OVERLAY2, 0x9399b2);
  define_color!(OVERLAY1, 0x7f849c);
  define_color!(OVERLAY0, 0x6c7086);
  define_color!(SURFACE2, 0x585b70);
  define_color!(SURFACE1, 0x45475a);
  define_color!(SURFACE0, 0x313244);
  define_color!(BASE, 0x1e1e2e);
  define_color!(MANTLE, 0x181825);
  define_color!(CRUST, 0x11111b);
}