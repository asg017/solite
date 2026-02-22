//! Color theme for table rendering.

/// RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const fn from_hex(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xFF) as u8,
            g: ((hex >> 8) & 0xFF) as u8,
            b: (hex & 0xFF) as u8,
        }
    }

    /// Convert to ANSI foreground escape code.
    pub fn to_ansi_fg(&self) -> String {
        format!("\x1b[38;2;{};{};{}m", self.r, self.g, self.b)
    }

    /// Convert to hex string for HTML.
    pub fn to_hex_string(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

/// Theme for table colors.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Color for null values.
    pub null: Color,
    /// Color for integer values.
    pub integer: Color,
    /// Color for double/float values.
    pub double: Color,
    /// Color for text values.
    pub text: Color,
    /// Color for blob values.
    pub blob: Color,
    /// Color for JSON keys.
    pub json_key: Color,
    /// Color for JSON string values.
    pub json_string: Color,
    /// Color for JSON numbers.
    pub json_number: Color,
    /// Color for JSON booleans.
    pub json_boolean: Color,
    /// Color for borders.
    pub border: Color,
    /// Color for headers.
    pub header: Color,
    /// Color for footer text.
    pub footer: Color,
}

impl Theme {
    /// Catppuccin Mocha theme (default).
    pub fn catppuccin_mocha() -> Self {
        Self {
            null: Color::from_hex(0xbac2de),     // SUBTEXT1
            integer: Color::from_hex(0xfab387),  // PEACH
            double: Color::from_hex(0xfab387),   // PEACH
            text: Color::from_hex(0xcdd6f4),     // TEXT
            blob: Color::from_hex(0x94e2d5),     // TEAL
            json_key: Color::from_hex(0x89b4fa), // BLUE
            json_string: Color::from_hex(0xa6e3a1), // GREEN
            json_number: Color::from_hex(0xfab387), // PEACH
            json_boolean: Color::from_hex(0xeba0ac), // MAROON
            border: Color::from_hex(0x6c7086),   // OVERLAY0
            header: Color::from_hex(0xcdd6f4),   // TEXT
            footer: Color::from_hex(0xa6adc8),   // SUBTEXT0
        }
    }
}

/// ANSI reset code.
pub const RESET: &str = "\x1b[0m";

/// ANSI bold code.
pub const BOLD: &str = "\x1b[1m";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_from_hex() {
        let c = Color::from_hex(0xfab387);
        assert_eq!(c.r, 0xfa);
        assert_eq!(c.g, 0xb3);
        assert_eq!(c.b, 0x87);
    }

    #[test]
    fn test_color_to_hex_string() {
        let c = Color::new(255, 128, 64);
        assert_eq!(c.to_hex_string(), "#FF8040");
    }

    #[test]
    fn test_color_to_ansi_fg() {
        let c = Color::new(255, 128, 64);
        assert_eq!(c.to_ansi_fg(), "\x1b[38;2;255;128;64m");
    }
}
