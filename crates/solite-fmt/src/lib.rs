//! Solite SQL Formatter
//!
//! A configurable SQL formatter for SQLite SQL, supporting comment preservation,
//! formatting ignore directives, and extensive configuration options.
//!
//! # Example
//!
//! ```
//! use solite_fmt::{format_sql, FormatConfig};
//!
//! let sql = "select a,b,c from t where x=1";
//! let config = FormatConfig::default();
//!
//! match format_sql(sql, &config) {
//!     Ok(formatted) => println!("{}", formatted),
//!     Err(e) => eprintln!("Error: {}", e),
//! }
//! ```

pub mod config;
pub mod comment;
pub mod ignore;
pub mod printer;
pub mod format;

pub use config::{
    CommaPosition, FormatConfig, IndentStyle, KeywordCase, LogicalOperatorPosition,
};
pub use comment::CommentMap;
pub use ignore::IgnoreDirectives;
pub use printer::Printer;

use solite_parser::parse_program;
use solite_schema::{parse_dot_commands, SqlRegion};

/// Format SQL source code according to the given configuration
///
/// # Arguments
///
/// * `source` - The SQL source code to format
/// * `config` - The formatting configuration
///
/// # Returns
///
/// The formatted SQL string, or an error if parsing fails
pub fn format_sql(source: &str, config: &FormatConfig) -> Result<String, FormatError> {
    // Parse the source into an AST
    let program = parse_program(source).map_err(FormatError::ParseError)?;

    // Collect comments from the token stream
    let comment_map = CommentMap::from_source(source);

    // Parse ignore directives
    let ignores = IgnoreDirectives::parse(source);

    // Create printer and format
    let mut printer = Printer::new(config.clone(), comment_map, ignores, source);
    printer.format_program(&program);

    Ok(printer.finish())
}

/// Check if SQL is already formatted according to configuration
///
/// Returns true if the source matches the formatted output
pub fn check_formatted(source: &str, config: &FormatConfig) -> Result<bool, FormatError> {
    let formatted = format_sql(source, config)?;
    Ok(source == formatted)
}

/// Format a document that may contain dot commands (like `.open`)
///
/// This function handles mixed documents with dot commands and SQL regions.
/// Dot command lines are preserved as-is, while SQL regions are formatted.
///
/// # Arguments
///
/// * `source` - The source text (may contain dot commands and SQL)
/// * `config` - The formatting configuration
///
/// # Returns
///
/// The formatted document, or an error if SQL parsing fails
pub fn format_document(source: &str, config: &FormatConfig) -> Result<String, FormatError> {
    let result = parse_dot_commands(source);

    // If there are no dot-prefixed lines, format as plain SQL
    // (blank lines may split into multiple regions, but that's fine for pure SQL)
    if !result.has_dot_lines {
        return format_sql(source, config);
    }

    // Format each SQL region and reconstruct the document
    let mut formatted_regions: Vec<String> = Vec::new();
    for region in &result.sql_regions {
        let sql = &source[region.start..region.end];
        let trimmed = sql.trim();
        if trimmed.is_empty() {
            formatted_regions.push(String::new());
        } else {
            let formatted = format_sql(trimmed, config)?;
            formatted_regions.push(formatted);
        }
    }

    // Reconstruct: interleave non-SQL parts with formatted SQL
    Ok(reconstruct_document(source, &result.sql_regions, &formatted_regions))
}

/// Reconstruct a document by replacing SQL regions with formatted versions
fn reconstruct_document(source: &str, regions: &[SqlRegion], formatted: &[String]) -> String {
    if regions.is_empty() {
        return source.to_string();
    }

    let mut result = String::new();
    let mut last_end = 0;

    for (region, formatted_sql) in regions.iter().zip(formatted.iter()) {
        // Add non-SQL content before this region (dot commands, blank lines, etc.)
        if region.start > last_end {
            result.push_str(&source[last_end..region.start]);
        }

        // Add the formatted SQL
        result.push_str(formatted_sql);

        last_end = region.end;
    }

    // Add any trailing content after the last SQL region
    if last_end < source.len() {
        result.push_str(&source[last_end..]);
    }

    result
}

/// Format error types
#[derive(Debug)]
pub enum FormatError {
    /// SQL parsing failed
    ParseError(Vec<solite_parser::ParseError>),
    /// IO error (for file operations)
    IoError(std::io::Error),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::ParseError(errors) => {
                write!(f, "Parse error: ")?;
                for (i, err) in errors.iter().enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{}", err)?;
                }
                Ok(())
            }
            FormatError::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for FormatError {}

impl From<std::io::Error> for FormatError {
    fn from(e: std::io::Error) -> Self {
        FormatError::IoError(e)
    }
}

#[cfg(test)]
mod tests;
