//! WASM bindings for solite_fmt SQL formatter
//!
//! Exposes the SQL formatting API to JavaScript via WebAssembly.

use serde::Deserialize;
use solite_fmt::{
    CommaPosition, FormatConfig, IndentStyle, KeywordCase, LogicalOperatorPosition,
};
use wasm_bindgen::prelude::*;

/// Initialize panic hook for better error messages in browser console
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// JavaScript-friendly format configuration
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct JsFormatConfig {
    keyword_case: Option<String>,
    indent_style: Option<String>,
    indent_size: Option<usize>,
    line_width: Option<usize>,
    comma_position: Option<String>,
    logical_operator_position: Option<String>,
    statement_separator_lines: Option<usize>,
}

impl JsFormatConfig {
    fn into_format_config(self) -> FormatConfig {
        let mut config = FormatConfig::default();

        if let Some(kw) = self.keyword_case {
            config.keyword_case = match kw.to_lowercase().as_str() {
                "upper" | "uppercase" => KeywordCase::Upper,
                "preserve" => KeywordCase::Preserve,
                _ => KeywordCase::Lower,
            };
        }

        if let Some(style) = self.indent_style {
            config.indent_style = match style.to_lowercase().as_str() {
                "tabs" | "tab" => IndentStyle::Tabs,
                _ => IndentStyle::Spaces,
            };
        }

        if let Some(size) = self.indent_size {
            config.indent_size = size;
        }

        if let Some(width) = self.line_width {
            config.line_width = width;
        }

        if let Some(pos) = self.comma_position {
            config.comma_position = match pos.to_lowercase().as_str() {
                "leading" | "start" => CommaPosition::Leading,
                _ => CommaPosition::Trailing,
            };
        }

        if let Some(pos) = self.logical_operator_position {
            config.logical_operator_position = match pos.to_lowercase().as_str() {
                "after" | "end" => LogicalOperatorPosition::After,
                _ => LogicalOperatorPosition::Before,
            };
        }

        if let Some(lines) = self.statement_separator_lines {
            config.statement_separator_lines = lines;
        }

        config
    }
}

/// Format SQL source code
///
/// # Arguments
///
/// * `source` - The SQL source code to format
/// * `config` - Optional configuration object with formatting options
///
/// # Returns
///
/// The formatted SQL string, or throws an error if parsing fails
#[wasm_bindgen]
pub fn format(source: &str, config: JsValue) -> Result<String, JsError> {
    let format_config = parse_config(config)?;
    solite_fmt::format_sql(source, &format_config)
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Check if SQL is already formatted according to configuration
///
/// # Arguments
///
/// * `source` - The SQL source code to check
/// * `config` - Optional configuration object with formatting options
///
/// # Returns
///
/// True if the source matches the formatted output, false otherwise
#[wasm_bindgen]
pub fn check(source: &str, config: JsValue) -> Result<bool, JsError> {
    let format_config = parse_config(config)?;
    solite_fmt::check_formatted(source, &format_config)
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Parse optional config from JsValue
fn parse_config(config: JsValue) -> Result<FormatConfig, JsError> {
    if config.is_undefined() || config.is_null() {
        return Ok(FormatConfig::default());
    }

    let js_config: JsFormatConfig = serde_wasm_bindgen::from_value(config)
        .map_err(|e| JsError::new(&format!("Invalid config: {}", e)))?;

    Ok(js_config.into_format_config())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_js_config_conversion() {
        let js_config = JsFormatConfig {
            keyword_case: Some("upper".to_string()),
            indent_style: Some("tabs".to_string()),
            indent_size: Some(4),
            line_width: Some(120),
            comma_position: Some("leading".to_string()),
            logical_operator_position: Some("after".to_string()),
            statement_separator_lines: Some(2),
        };

        let config = js_config.into_format_config();
        assert!(matches!(config.keyword_case, KeywordCase::Upper));
        assert!(matches!(config.indent_style, IndentStyle::Tabs));
        assert_eq!(config.indent_size, 4);
        assert_eq!(config.line_width, 120);
        assert!(matches!(config.comma_position, CommaPosition::Leading));
        assert!(matches!(config.logical_operator_position, LogicalOperatorPosition::After));
        assert_eq!(config.statement_separator_lines, 2);
    }

    #[test]
    fn test_js_config_defaults() {
        let js_config = JsFormatConfig::default();
        let config = js_config.into_format_config();

        // Should use defaults from FormatConfig
        assert!(matches!(config.keyword_case, KeywordCase::Lower));
        assert!(matches!(config.indent_style, IndentStyle::Spaces));
        assert_eq!(config.indent_size, 2);
    }

    #[test]
    fn test_format_via_solite_fmt() {
        // Test the underlying formatter directly (not through wasm bindings)
        let config = FormatConfig::default();
        let result = solite_fmt::format_sql("SELECT a,b FROM t", &config).unwrap();
        assert!(result.contains("select"));
        assert!(result.contains("from"));
    }

    #[test]
    fn test_check_via_solite_fmt() {
        let config = FormatConfig::default();

        // Formatted SQL should pass check
        let formatted = "select * from t;\n";
        assert!(solite_fmt::check_formatted(formatted, &config).unwrap());

        // Unformatted SQL should fail check
        let unformatted = "SELECT    *    FROM    t";
        assert!(!solite_fmt::check_formatted(unformatted, &config).unwrap());
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn test_format_simple() {
        let result = format("SELECT a,b FROM t", JsValue::UNDEFINED).unwrap();
        assert!(result.contains("select"));
        assert!(result.contains("from"));
    }

    #[wasm_bindgen_test]
    fn test_check_formatted() {
        let formatted = "select * from t;\n";
        let result = check(formatted, JsValue::UNDEFINED).unwrap();
        assert!(result);
    }

    #[wasm_bindgen_test]
    fn test_check_unformatted() {
        let unformatted = "SELECT    *    FROM    t";
        let result = check(unformatted, JsValue::UNDEFINED).unwrap();
        assert!(!result);
    }
}
