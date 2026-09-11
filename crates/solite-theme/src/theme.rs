//! The semantic-role theme.
//!
//! Roles are *semantic* (what the text means), not per-widget (where it is
//! drawn), so the same role drives the table renderer, the REPL highlighter,
//! the TUI and the HTML surfaces.

use crate::color::{AnsiColor::*, ColorValue};
use crate::style::Style;

/// Catppuccin Mocha palette, defined once for the whole workspace.
pub mod catppuccin_mocha_palette {
    pub const ROSEWATER: u32 = 0xf5e0dc;
    pub const FLAMINGO: u32 = 0xf2cdcd;
    pub const PINK: u32 = 0xf5c2e7;
    pub const MAUVE: u32 = 0xcba6f7;
    pub const RED: u32 = 0xf38ba8;
    pub const MAROON: u32 = 0xeba0ac;
    pub const PEACH: u32 = 0xfab387;
    pub const YELLOW: u32 = 0xf9e2af;
    pub const GREEN: u32 = 0xa6e3a1;
    pub const TEAL: u32 = 0x94e2d5;
    pub const SKY: u32 = 0x89dceb;
    pub const SAPPHIRE: u32 = 0x74c7ec;
    pub const BLUE: u32 = 0x89b4fa;
    pub const LAVENDER: u32 = 0xb4befe;
    pub const TEXT: u32 = 0xcdd6f4;
    pub const SUBTEXT1: u32 = 0xbac2de;
    pub const SUBTEXT0: u32 = 0xa6adc8;
    pub const OVERLAY2: u32 = 0x9399b2;
    pub const OVERLAY1: u32 = 0x7f849c;
    pub const OVERLAY0: u32 = 0x6c7086;
    pub const SURFACE2: u32 = 0x585b70;
    pub const SURFACE1: u32 = 0x45475a;
    pub const SURFACE0: u32 = 0x313244;
    pub const BASE: u32 = 0x1e1e2e;
    pub const MANTLE: u32 = 0x181825;
    pub const CRUST: u32 = 0x11111b;
}

/// A complete set of semantic role styles.
///
/// Every field is a [`Style`], so a role can carry a background and modifiers
/// as well as a foreground (e.g. `selection` is background-only: its `fg` is
/// [`ColorValue::Default`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    // ---- SQLite value types --------------------------------------------
    /// `NULL` values.
    pub null: Style,
    /// Integer values.
    pub integer: Style,
    /// Real/float values.
    pub double: Style,
    /// Text values.
    pub text: Style,
    /// Blob values.
    pub blob: Style,

    // ---- SQL tokens ----------------------------------------------------
    /// SQL keywords (LSP `keyword`).
    pub keyword: Style,
    /// String and blob literals (LSP `string`).
    pub string_literal: Style,
    /// Line and block comments (LSP `comment`).
    pub comment: Style,
    /// Bind parameters `$x` / `:x` / `@x` / `?` (LSP `variable`).
    pub parameter: Style,
    /// Operators (LSP `operator`).
    pub operator: Style,
    /// Numeric literals (LSP `number`).
    pub number: Style,
    /// Function names (LSP `function`).
    pub function: Style,
    /// Built-in / stdlib function names.
    pub function_builtin: Style,
    /// Type names in DDL, and the declared-type column in the TUI (LSP `type`).
    pub type_name: Style,
    /// Parentheses, commas, semicolons.
    pub punctuation: Style,
    /// Solite dot commands (`.tables`, `.export`, …).
    pub dot_command: Style,

    // ---- JSON tokens ---------------------------------------------------
    /// JSON object keys.
    pub json_key: Style,
    /// JSON string values.
    pub json_string: Style,
    /// JSON numbers.
    pub json_number: Style,
    /// JSON `true`/`false`/`null`.
    pub json_boolean: Style,

    // ---- Chrome --------------------------------------------------------
    /// Overall surface background (fg + optional bg). Terminal themes leave the
    /// background unset so the terminal shows through.
    pub background: Style,
    /// Table/box borders and rules.
    pub border: Style,
    /// Column headers.
    pub header: Style,
    /// The currently selected/sorted column header.
    pub header_selected: Style,
    /// Footers, row counts, timing lines.
    pub footer: Style,
    /// De-emphasized chrome text (hints, placeholders, ellipses).
    pub muted: Style,

    // ---- UI state ------------------------------------------------------
    /// The selected row (background-only in the built-in themes).
    pub selection: Style,
    /// The focused cell / search match (foreground **and** background).
    pub highlight: Style,
    /// Success messages.
    pub success: Style,
    /// Error messages.
    pub error: Style,
    /// Warnings.
    pub warning: Style,
    /// Keyboard-shortcut hints ("press <q> to quit").
    pub keycap: Style,
}

impl Default for Theme {
    /// The terminal theme — see [`Theme::terminal`].
    fn default() -> Self {
        Theme::terminal()
    }
}

impl Theme {
    /// The default theme: ANSI-16 names and the terminal-default sentinel
    /// only, never truecolor.
    ///
    /// Everything here resolves against the user's own terminal palette, so it
    /// is legible on light and dark backgrounds alike with zero configuration.
    pub const fn terminal() -> Self {
        Theme {
            null: Style::ansi(BrightBlack),
            integer: Style::ansi(Yellow),
            double: Style::ansi(Yellow),
            text: Style::DEFAULT,
            blob: Style::ansi(Cyan),

            keyword: Style::ansi(Magenta).bold(),
            string_literal: Style::ansi(Green),
            comment: Style::ansi(BrightBlack),
            parameter: Style::ansi(Red),
            operator: Style::ansi(Cyan),
            number: Style::ansi(Yellow),
            function: Style::ansi(Blue),
            function_builtin: Style::ansi(Blue).bold(),
            type_name: Style::ansi(Yellow),
            punctuation: Style::DEFAULT,
            dot_command: Style::ansi(Blue),

            json_key: Style::ansi(Blue),
            json_string: Style::ansi(Green),
            json_number: Style::ansi(Yellow),
            json_boolean: Style::ansi(Red),

            background: Style::DEFAULT,
            border: Style::DEFAULT.dim(),
            header: Style::DEFAULT.bold(),
            header_selected: Style::DEFAULT
                .bold()
                .with_bg(ColorValue::Ansi(BrightBlack)),
            footer: Style::DEFAULT.dim(),
            muted: Style::ansi(BrightBlack),

            selection: Style::DEFAULT.with_bg(ColorValue::Ansi(BrightBlack)),
            highlight: Style::ansi(BrightWhite).with_bg(ColorValue::Ansi(Blue)),
            success: Style::ansi(Green),
            error: Style::ansi(Red),
            warning: Style::ansi(Yellow),
            keycap: Style::ansi(Green).bold(),
        }
    }

    /// Catppuccin Mocha, the truecolor palette Solite shipped before the
    /// terminal theme became the default.
    ///
    /// Hex values match the previous per-crate copies exactly. Where two of
    /// those copies disagreed the table renderer's value wins:
    /// `blob` is TEAL (the TUI used FLAMINGO) and `header` is TEXT (the TUI
    /// used BLUE).
    pub const fn catppuccin_mocha() -> Self {
        use catppuccin_mocha_palette as p;
        Theme {
            null: Style::hex(p::SUBTEXT1),
            integer: Style::hex(p::PEACH),
            double: Style::hex(p::PEACH),
            text: Style::hex(p::TEXT),
            blob: Style::hex(p::TEAL),

            keyword: Style::hex(p::MAUVE).bold(),
            string_literal: Style::hex(p::GREEN),
            comment: Style::hex(p::OVERLAY2),
            parameter: Style::hex(p::MAROON),
            operator: Style::hex(p::SKY),
            number: Style::hex(p::PEACH),
            function: Style::hex(p::BLUE),
            function_builtin: Style::hex(p::BLUE).bold(),
            type_name: Style::hex(p::YELLOW),
            punctuation: Style::hex(p::OVERLAY2),
            dot_command: Style::hex(p::BLUE),

            json_key: Style::hex(p::BLUE),
            json_string: Style::hex(p::GREEN),
            json_number: Style::hex(p::PEACH),
            json_boolean: Style::hex(p::MAROON),

            background: Style::hex(p::TEXT).with_bg_hex(p::BASE),
            border: Style::hex(p::OVERLAY0),
            header: Style::hex(p::TEXT).bold(),
            header_selected: Style::hex(p::TEXT).with_bg_hex(p::OVERLAY0),
            footer: Style::hex(p::SUBTEXT0),
            muted: Style::hex(p::OVERLAY2),

            selection: Style::DEFAULT.with_bg_hex(p::SURFACE0),
            highlight: Style::hex(p::TEXT).with_bg_hex(p::CRUST),
            success: Style::hex(p::GREEN),
            error: Style::hex(p::RED),
            warning: Style::hex(p::YELLOW),
            keycap: Style::hex(p::GREEN),
        }
    }

    /// Look a role up by its field name, for config files and tests.
    pub fn role(&self, name: &str) -> Option<Style> {
        Some(match name {
            "null" => self.null,
            "integer" => self.integer,
            "double" => self.double,
            "text" => self.text,
            "blob" => self.blob,
            "keyword" => self.keyword,
            "string_literal" => self.string_literal,
            "comment" => self.comment,
            "parameter" => self.parameter,
            "operator" => self.operator,
            "number" => self.number,
            "function" => self.function,
            "function_builtin" => self.function_builtin,
            "type_name" => self.type_name,
            "punctuation" => self.punctuation,
            "dot_command" => self.dot_command,
            "json_key" => self.json_key,
            "json_string" => self.json_string,
            "json_number" => self.json_number,
            "json_boolean" => self.json_boolean,
            "background" => self.background,
            "border" => self.border,
            "header" => self.header,
            "header_selected" => self.header_selected,
            "footer" => self.footer,
            "muted" => self.muted,
            "selection" => self.selection,
            "highlight" => self.highlight,
            "success" => self.success,
            "error" => self.error,
            "warning" => self.warning,
            "keycap" => self.keycap,
            _ => return None,
        })
    }

    /// Every role name, in declaration order.
    pub const ROLE_NAMES: &'static [&'static str] = &[
        "null",
        "integer",
        "double",
        "text",
        "blob",
        "keyword",
        "string_literal",
        "comment",
        "parameter",
        "operator",
        "number",
        "function",
        "function_builtin",
        "type_name",
        "punctuation",
        "dot_command",
        "json_key",
        "json_string",
        "json_number",
        "json_boolean",
        "background",
        "border",
        "header",
        "header_selected",
        "footer",
        "muted",
        "selection",
        "highlight",
        "success",
        "error",
        "warning",
        "keycap",
    ];
}
