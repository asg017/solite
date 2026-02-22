//! Configuration for SQL formatting
//!
//! This module handles loading format configuration from TOML files,
//! with support for project-local and global configuration.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Keyword case transformation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeywordCase {
    /// UPPERCASE keywords
    Upper,
    /// lowercase keywords (default)
    #[default]
    Lower,
    /// Preserve original case
    Preserve,
}

impl<'de> Deserialize<'de> for KeywordCase {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "upper" | "uppercase" => Ok(KeywordCase::Upper),
            "lower" | "lowercase" => Ok(KeywordCase::Lower),
            "preserve" => Ok(KeywordCase::Preserve),
            _ => Err(serde::de::Error::custom(format!(
                "unknown keyword_case: {}",
                s
            ))),
        }
    }
}

/// Indentation style
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndentStyle {
    /// Use spaces for indentation (default)
    #[default]
    Spaces,
    /// Use tabs for indentation
    Tabs,
}

impl<'de> Deserialize<'de> for IndentStyle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "spaces" | "space" => Ok(IndentStyle::Spaces),
            "tabs" | "tab" => Ok(IndentStyle::Tabs),
            _ => Err(serde::de::Error::custom(format!(
                "unknown indent_style: {}",
                s
            ))),
        }
    }
}

/// Comma position in lists
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommaPosition {
    /// Comma at end of line (default)
    #[default]
    Trailing,
    /// Comma at start of line
    Leading,
}

impl<'de> Deserialize<'de> for CommaPosition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "trailing" | "end" => Ok(CommaPosition::Trailing),
            "leading" | "start" => Ok(CommaPosition::Leading),
            _ => Err(serde::de::Error::custom(format!(
                "unknown comma_position: {}",
                s
            ))),
        }
    }
}

/// Logical operator (AND/OR) position in WHERE clauses
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogicalOperatorPosition {
    /// Operator at start of line (default)
    #[default]
    Before,
    /// Operator at end of line
    After,
}

impl<'de> Deserialize<'de> for LogicalOperatorPosition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "before" | "start" => Ok(LogicalOperatorPosition::Before),
            "after" | "end" => Ok(LogicalOperatorPosition::After),
            _ => Err(serde::de::Error::custom(format!(
                "unknown logical_operator_position: {}",
                s
            ))),
        }
    }
}

/// Format configuration loaded from TOML files
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FormatConfig {
    /// Keyword case transformation
    pub keyword_case: KeywordCase,
    /// Indentation style (spaces or tabs)
    pub indent_style: IndentStyle,
    /// Number of spaces per indentation level (when using spaces)
    pub indent_size: usize,
    /// Target line width for wrapping decisions
    pub line_width: usize,
    /// Comma position in column lists
    pub comma_position: CommaPosition,
    /// AND/OR position in WHERE clauses
    pub logical_operator_position: LogicalOperatorPosition,
    /// Number of blank lines between statements
    pub statement_separator_lines: usize,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            keyword_case: KeywordCase::Lower,
            indent_style: IndentStyle::Spaces,
            indent_size: 2,
            line_width: 80,
            comma_position: CommaPosition::Trailing,
            logical_operator_position: LogicalOperatorPosition::Before,
            statement_separator_lines: 2,
        }
    }
}

impl FormatConfig {
    /// Create a new config with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Discover config file by looking in current dir, parent dirs, then ~/.config/solite/fmt.toml
    pub fn discover() -> Self {
        Self::discover_from(std::env::current_dir().ok())
    }

    /// Discover config file starting from a specific directory
    pub fn discover_from(start_dir: Option<PathBuf>) -> Self {
        // Look for solite-fmt.toml in current and parent directories
        if let Some(mut dir) = start_dir {
            loop {
                let config_path = dir.join("solite-fmt.toml");
                if config_path.exists() {
                    if let Ok(config) = Self::load(&config_path) {
                        return config;
                    }
                }
                if !dir.pop() {
                    break;
                }
            }
        }

        // Try global config
        if let Some(home) = home_dir() {
            let global_config = home.join(".config/solite/fmt.toml");
            if global_config.exists() {
                if let Ok(config) = Self::load(&global_config) {
                    return config;
                }
            }
        }

        // Return default if no config found
        Self::default()
    }

    /// Load configuration from a specific TOML file
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: FormatConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Get the indentation string for one level
    pub fn indent_string(&self) -> String {
        match self.indent_style {
            IndentStyle::Spaces => " ".repeat(self.indent_size),
            IndentStyle::Tabs => "\t".to_string(),
        }
    }
}

/// Get the user's home directory using environment variables
fn home_dir() -> Option<PathBuf> {
    // Try HOME first (Unix)
    if let Ok(home) = std::env::var("HOME") {
        return Some(PathBuf::from(home));
    }

    // Try USERPROFILE (Windows)
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return Some(PathBuf::from(profile));
    }

    // Try HOMEDRIVE + HOMEPATH (Windows alternative)
    if let (Ok(drive), Ok(path)) = (std::env::var("HOMEDRIVE"), std::env::var("HOMEPATH")) {
        return Some(PathBuf::from(format!("{}{}", drive, path)));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = FormatConfig::default();
        assert_eq!(config.keyword_case, KeywordCase::Lower);
        assert_eq!(config.indent_size, 2);
        assert_eq!(config.line_width, 80);
    }

    #[test]
    fn test_indent_string_spaces() {
        let config = FormatConfig {
            indent_style: IndentStyle::Spaces,
            indent_size: 4,
            ..Default::default()
        };
        assert_eq!(config.indent_string(), "    ");
    }

    #[test]
    fn test_indent_string_tabs() {
        let config = FormatConfig {
            indent_style: IndentStyle::Tabs,
            ..Default::default()
        };
        assert_eq!(config.indent_string(), "\t");
    }

    #[test]
    fn test_parse_toml_config() {
        let toml = r#"
            keyword_case = "lower"
            indent_style = "tabs"
            indent_size = 2
            line_width = 120
            comma_position = "leading"
            logical_operator_position = "after"
            statement_separator_lines = 2
        "#;

        let config: FormatConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.keyword_case, KeywordCase::Lower);
        assert_eq!(config.indent_style, IndentStyle::Tabs);
        assert_eq!(config.indent_size, 2);
        assert_eq!(config.line_width, 120);
        assert_eq!(config.comma_position, CommaPosition::Leading);
        assert_eq!(config.logical_operator_position, LogicalOperatorPosition::After);
        assert_eq!(config.statement_separator_lines, 2);
    }

    #[test]
    fn test_parse_partial_config() {
        let toml = r#"
            keyword_case = "preserve"
        "#;

        let config: FormatConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.keyword_case, KeywordCase::Preserve);
        // Other fields should use defaults
        assert_eq!(config.indent_size, 2);
    }

    #[test]
    fn test_parse_invalid_keyword_case() {
        let toml = r#"
            keyword_case = "invalid"
        "#;

        let result: Result<FormatConfig, _> = toml::from_str(toml);
        assert!(result.is_err());
    }
}
