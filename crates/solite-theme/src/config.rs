//! TOML theme files: `inherits`, an optional `[palette]`, and a `[roles]`
//! table of partial overrides.
//!
//! ```toml
//! inherits = "terminal"          # a built-in, or another .toml in this dir
//!
//! [palette]                      # optional named-color indirection
//! peach = "#fab387"
//!
//! [roles]                        # every key optional
//! integer = "peach"              # bare string = foreground shorthand
//! keyword = { fg = "magenta", bold = true }
//! border  = "default"            # the terminal-default sentinel
//! ```
//!
//! A role entry defines that role *completely*: unmentioned fields fall back
//! to the [`Style`] defaults (terminal-default foreground, no background, no
//! modifiers), not to the inherited role's. Inheritance is per role, not per
//! field — roles the file doesn't mention are taken from `inherits` verbatim.
//!
//! Color grammar (in resolution order):
//!
//! | form | example |
//! |------|---------|
//! | palette name | `peach` |
//! | terminal default sentinel | `default`, `reset`, `none` |
//! | one of the 16 ANSI names | `red`, `bright-red` (also `brightred`, `bright_red`) |
//! | 256-color index | `0` … `255` |
//! | hex | `#fab387`, `#f8b` |
//!
//! Every failure is a [`ThemeError`] naming the offending key and value; this
//! module never panics on bad input.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::color::{AnsiColor, ColorValue};
use crate::style::Style;
use crate::theme::Theme;

/// A theme file that could not be read, parsed, or resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeError {
    message: String,
}

impl ThemeError {
    fn new(message: impl Into<String>) -> Self {
        ThemeError {
            message: message.into(),
        }
    }

    /// The human-readable message, without any trailing newline.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ThemeError {}

/// Parse a theme document whose `inherits` may only name a built-in theme.
///
/// `origin` is used in error messages (a path, or something like
/// `<string>`).
pub fn parse_theme(src: &str, origin: &str) -> Result<Theme, ThemeError> {
    let mut stack = Vec::new();
    parse_theme_inner(src, origin, None, &mut stack)
}

/// Parse a theme document whose `inherits` may name a built-in theme *or* a
/// sibling `<name>.toml` in `dir`.
pub fn parse_theme_in_dir(src: &str, origin: &str, dir: &Path) -> Result<Theme, ThemeError> {
    let mut stack = Vec::new();
    parse_theme_inner(src, origin, Some(dir), &mut stack)
}

/// Read and parse a theme file. `inherits` resolves against the built-ins
/// first, then against `<name>.toml` next to `path`.
pub fn load_theme_file(path: &Path) -> Result<Theme, ThemeError> {
    let mut stack = Vec::new();
    load_theme_file_inner(path, &mut stack)
}

fn identity(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn load_theme_file_inner(path: &Path, stack: &mut Vec<String>) -> Result<Theme, ThemeError> {
    let id = identity(path);
    if stack.contains(&id) {
        let mut chain = stack.clone();
        chain.push(id);
        return Err(ThemeError::new(format!(
            "theme inheritance cycle: {}",
            chain.join(" -> ")
        )));
    }
    let src = std::fs::read_to_string(path).map_err(|e| {
        ThemeError::new(format!("could not read theme file {}: {e}", path.display()))
    })?;
    stack.push(id);
    let dir = path.parent().map(PathBuf::from);
    let result = parse_theme_inner(&src, &path.display().to_string(), dir.as_deref(), stack);
    stack.pop();
    result
}

fn parse_theme_inner(
    src: &str,
    origin: &str,
    dir: Option<&Path>,
    stack: &mut Vec<String>,
) -> Result<Theme, ThemeError> {
    let doc: toml::Value =
        toml::from_str(src).map_err(|e| ThemeError::new(format!("{origin}: invalid TOML: {e}")))?;
    let table = doc
        .as_table()
        .ok_or_else(|| ThemeError::new(format!("{origin}: expected a TOML table")))?;

    for key in table.keys() {
        if !matches!(key.as_str(), "inherits" | "palette" | "roles" | "name") {
            return Err(ThemeError::new(format!(
                "{origin}: unknown top-level key `{key}` (expected `inherits`, `palette`, `roles`)"
            )));
        }
    }

    // ---- base ----------------------------------------------------------
    let mut theme = match table.get("inherits") {
        None => Theme::terminal(),
        Some(toml::Value::String(name)) => resolve_base(name, origin, dir, stack)?,
        Some(other) => {
            return Err(ThemeError::new(format!(
                "{origin}: `inherits` must be a theme name string, got {}",
                type_name(other)
            )))
        }
    };

    // ---- palette -------------------------------------------------------
    let mut palette: HashMap<String, ColorValue> = HashMap::new();
    match table.get("palette") {
        None => {}
        Some(toml::Value::Table(entries)) => {
            for (name, value) in entries {
                let toml::Value::String(spec) = value else {
                    return Err(ThemeError::new(format!(
                        "{origin}: palette entry `{name}` must be a color string, got {}",
                        type_name(value)
                    )));
                };
                // Palette entries may not reference other palette entries, so
                // ordering never matters.
                let color = parse_color(spec, &HashMap::new())
                    .map_err(|e| ThemeError::new(format!("{origin}: palette `{name}`: {e}")))?;
                palette.insert(name.clone(), color);
            }
        }
        Some(other) => {
            return Err(ThemeError::new(format!(
                "{origin}: `palette` must be a table, got {}",
                type_name(other)
            )))
        }
    }

    // ---- roles ---------------------------------------------------------
    match table.get("roles") {
        None => {}
        Some(toml::Value::Table(entries)) => {
            for (role, value) in entries {
                if Theme::ROLE_NAMES.iter().all(|r| *r != role.as_str()) {
                    return Err(ThemeError::new(format!(
                        "{origin}: unknown role `{role}`. Known roles: {}",
                        Theme::ROLE_NAMES.join(", ")
                    )));
                }
                let style = parse_style(value, &palette)
                    .map_err(|e| ThemeError::new(format!("{origin}: role `{role}`: {e}")))?;
                theme.set_role(role, style);
            }
        }
        Some(other) => {
            return Err(ThemeError::new(format!(
                "{origin}: `roles` must be a table, got {}",
                type_name(other)
            )))
        }
    }

    Ok(theme)
}

/// Resolve an `inherits = "..."` value: a built-in, else `<name>.toml` beside
/// the inheriting file.
fn resolve_base(
    name: &str,
    origin: &str,
    dir: Option<&Path>,
    stack: &mut Vec<String>,
) -> Result<Theme, ThemeError> {
    if let Some(builtin) = Theme::builtin(name) {
        return Ok(builtin);
    }
    if let Some(dir) = dir {
        let candidate = if name.ends_with(".toml") {
            dir.join(name)
        } else {
            dir.join(format!("{name}.toml"))
        };
        if candidate.is_file() {
            return load_theme_file_inner(&candidate, stack);
        }
        return Err(ThemeError::new(format!(
            "{origin}: `inherits = \"{name}\"` matches no built-in theme ({}) and no file {}",
            Theme::BUILTIN_NAMES.join(", "),
            candidate.display()
        )));
    }
    Err(ThemeError::new(format!(
        "{origin}: `inherits = \"{name}\"` matches no built-in theme ({})",
        Theme::BUILTIN_NAMES.join(", ")
    )))
}

fn type_name(value: &toml::Value) -> &'static str {
    match value {
        toml::Value::String(_) => "a string",
        toml::Value::Integer(_) => "an integer",
        toml::Value::Float(_) => "a float",
        toml::Value::Boolean(_) => "a boolean",
        toml::Value::Datetime(_) => "a datetime",
        toml::Value::Array(_) => "an array",
        toml::Value::Table(_) => "a table",
    }
}

/// A role value: either a bare color string (foreground shorthand) or an
/// inline table of `fg`/`bg`/modifiers.
fn parse_style(value: &toml::Value, palette: &Palette) -> Result<Style, String> {
    match value {
        toml::Value::String(spec) => Ok(Style::new(parse_color(spec, palette)?)),
        toml::Value::Integer(i) => Ok(Style::new(parse_index(*i)?)),
        toml::Value::Table(entries) => {
            let mut style = Style::DEFAULT;
            for (key, value) in entries {
                match key.as_str() {
                    "fg" => style.fg = color_field(key, value, palette)?,
                    "bg" => style.bg = Some(color_field(key, value, palette)?),
                    "bold" => style.bold = bool_field(key, value)?,
                    "dim" => style.dim = bool_field(key, value)?,
                    "italic" => style.italic = bool_field(key, value)?,
                    "underline" => style.underline = bool_field(key, value)?,
                    other => {
                        return Err(format!(
                            "unknown key `{other}` (expected fg, bg, bold, dim, italic, underline)"
                        ))
                    }
                }
            }
            Ok(style)
        }
        other => Err(format!(
            "expected a color string or a table of fg/bg/modifiers, got {}",
            type_name(other)
        )),
    }
}

fn color_field(key: &str, value: &toml::Value, palette: &Palette) -> Result<ColorValue, String> {
    match value {
        toml::Value::String(spec) => {
            parse_color(spec, palette).map_err(|e| format!("`{key}`: {e}"))
        }
        toml::Value::Integer(i) => parse_index(*i).map_err(|e| format!("`{key}`: {e}")),
        other => Err(format!(
            "`{key}` must be a color string, got {}",
            type_name(other)
        )),
    }
}

fn bool_field(key: &str, value: &toml::Value) -> Result<bool, String> {
    value
        .as_bool()
        .ok_or_else(|| format!("`{key}` must be true or false, got {}", type_name(value)))
}

fn parse_index(i: i64) -> Result<ColorValue, String> {
    u8::try_from(i)
        .map(index_color)
        .map_err(|_| format!("color index {i} is out of range (0-255)"))
}

/// 0-15 are the named ANSI colors (so `1` and `red` mean the same thing and
/// emit the same short SGR code); 16-255 are palette indexes.
fn index_color(index: u8) -> ColorValue {
    match AnsiColor::from_index(index) {
        Some(c) => ColorValue::Ansi(c),
        None => ColorValue::Indexed(index),
    }
}

type Palette = HashMap<String, ColorValue>;

/// Parse one color string. Palette names win over built-in names, so a theme
/// can redefine `red` for its own vocabulary.
pub fn parse_color(spec: &str, palette: &Palette) -> Result<ColorValue, String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("empty color value".to_string());
    }
    if let Some(color) = palette.get(spec) {
        return Ok(*color);
    }
    if let Some(hex) = spec.strip_prefix('#') {
        return ColorValue::parse_hex(hex).ok_or_else(|| {
            format!("`{spec}` is not a valid hex color (expected #rgb or #rrggbb)")
        });
    }
    if spec.bytes().all(|b| b.is_ascii_digit()) {
        return match spec.parse::<u16>() {
            Ok(v) if v <= 255 => Ok(index_color(v as u8)),
            _ => Err(format!("color index `{spec}` is out of range (0-255)")),
        };
    }
    if let Some(color) = named_color(spec) {
        return Ok(color);
    }
    Err(format!(
        "unknown color `{spec}` (expected an ANSI name, `default`, 0-255, #rrggbb, or a [palette] entry)"
    ))
}

/// The 16 ANSI names plus the terminal-default sentinel. Liberal about
/// spelling: case, `-`, `_` and no separator at all are equivalent, so
/// `bright-red`, `bright_red`, `brightred` and `BrightRed` all work.
fn named_color(spec: &str) -> Option<ColorValue> {
    let normalized: String = spec
        .chars()
        .filter(|c| *c != '-' && *c != '_' && *c != ' ')
        .flat_map(|c| c.to_lowercase())
        .collect();
    let ansi = match normalized.as_str() {
        "default" | "reset" | "none" | "inherit" => return Some(ColorValue::Default),
        "black" => AnsiColor::Black,
        "red" => AnsiColor::Red,
        "green" => AnsiColor::Green,
        "yellow" => AnsiColor::Yellow,
        "blue" => AnsiColor::Blue,
        "magenta" | "purple" => AnsiColor::Magenta,
        "cyan" => AnsiColor::Cyan,
        "white" => AnsiColor::White,
        "brightblack" | "gray" | "grey" => AnsiColor::BrightBlack,
        "brightred" => AnsiColor::BrightRed,
        "brightgreen" => AnsiColor::BrightGreen,
        "brightyellow" => AnsiColor::BrightYellow,
        "brightblue" => AnsiColor::BrightBlue,
        "brightmagenta" | "brightpurple" => AnsiColor::BrightMagenta,
        "brightcyan" => AnsiColor::BrightCyan,
        "brightwhite" => AnsiColor::BrightWhite,
        _ => return None,
    };
    Some(ColorValue::Ansi(ansi))
}
