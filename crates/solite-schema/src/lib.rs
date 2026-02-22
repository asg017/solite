//! Schema awareness and dot command handling for Solite.
//!
//! This crate provides:
//! - Dot command parsing and execution (`.tables`, `.schema`, etc.)
//! - Schema provider traits for database introspection
//! - SQLite database introspection (native only)
//! - JSON-based schema loading
//! - Document parsing combining dot commands with SQL
//!
//! # JSON Schema Loading
//!
//! The [`json`] module provides a way to load database schemas from JSON,
//! which is particularly useful for WASM/browser environments:
//!
//! ```
//! use solite_schema::json::{JsonSchema, JsonTable, JsonColumn};
//!
//! let json = r#"{"tables": [{"name": "users", "columns": [{"name": "id"}]}]}"#;
//! let json_schema = JsonSchema::from_json(json).unwrap();
//!
//! // Convert to analyzer schema for SQL validation
//! let schema = json_schema.to_analyzer_schema();
//! assert!(schema.has_table("users"));
//! ```
//!
//! # Document Parsing
//!
//! The [`Document`] struct is the primary entry point for parsing files that may
//! include SQLite shell-style dot commands like `.open`:
//!
//! ```
//! use solite_schema::Document;
//!
//! let source = ".open mydb.db\nSELECT * FROM users;";
//! let doc = Document::parse(source, true);
//!
//! // Access dot commands
//! assert!(doc.has_dot_commands());
//! let open_paths: Vec<_> = doc.open_commands().collect();
//! assert_eq!(open_paths, vec!["mydb.db"]);
//!
//! // Access parsed SQL
//! assert!(doc.program.is_ok());
//! ```

pub mod dotcmd;
pub mod json;
pub mod provider;

use solite_ast::Program;
use solite_parser::{parse_program, ParseError};

pub use dotcmd::{parse_dot_commands, DotCommand, ParseResult, SchemaHint, SqlRegion};
pub use json::{JsonColumn, JsonIndex, JsonSchema, JsonSchemaError, JsonTable, JsonTrigger, JsonView};
pub use provider::{DdlSchemaProvider, JsonSchemaProvider, SchemaError, SchemaProvider};

#[cfg(not(target_arch = "wasm32"))]
pub mod introspect;

#[cfg(not(target_arch = "wasm32"))]
pub use provider::FileSchemaProvider;

/// A document that may contain both dot commands and SQL.
///
/// This is the primary entry point for parsing files that may include
/// SQLite shell-style dot commands like `.open`.
#[derive(Debug, Clone)]
pub struct Document {
    /// Original source text
    pub source: String,
    /// Dot commands found in the document
    pub dot_commands: Vec<DotCommand>,
    /// Regions of the source that contain SQL
    pub sql_regions: Vec<SqlRegion>,
    /// Parsed SQL program (combined from all SQL regions)
    pub program: Result<Program, Vec<ParseError>>,
    /// `-- schema: <path>` hints from the file header
    pub schema_hints: Vec<SchemaHint>,
}

impl Document {
    /// Parse a document, optionally processing dot commands.
    ///
    /// When `enable_dot_commands` is true, lines starting with `.` are
    /// parsed as dot commands (like `.open mydb.db`).
    ///
    /// When false, the entire source is treated as SQL.
    pub fn parse(source: &str, enable_dot_commands: bool) -> Self {
        if enable_dot_commands {
            let result = parse_dot_commands(source);

            // Extract SQL from regions and combine
            let sql_source: String = result
                .sql_regions
                .iter()
                .map(|r| &source[r.start..r.end])
                .collect::<Vec<_>>()
                .join("\n");

            let program = parse_program(&sql_source);

            Document {
                source: source.to_string(),
                dot_commands: result.dot_commands,
                sql_regions: result.sql_regions,
                program,
                schema_hints: result.schema_hints,
            }
        } else {
            Document {
                source: source.to_string(),
                dot_commands: vec![],
                sql_regions: vec![SqlRegion {
                    start: 0,
                    end: source.len(),
                }],
                program: parse_program(source),
                schema_hints: vec![],
            }
        }
    }

    /// Get all `.open` commands from the document
    pub fn open_commands(&self) -> impl Iterator<Item = &str> {
        self.dot_commands.iter().map(|cmd| match cmd {
            DotCommand::Open { path, .. } => path.as_str(),
        })
    }

    /// Check if this document has any dot commands
    pub fn has_dot_commands(&self) -> bool {
        !self.dot_commands.is_empty()
    }

    /// Get `-- schema: <path>` hints from the file header
    pub fn schema_hints(&self) -> &[SchemaHint] {
        &self.schema_hints
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solite_ast::Statement;

    #[test]
    fn test_document_parse_with_dot_commands_enabled() {
        let source = ".open mydb.db\nSELECT 1;";
        let doc = Document::parse(source, true);

        assert_eq!(doc.source, source);
        assert_eq!(doc.dot_commands.len(), 1);
        assert_eq!(doc.sql_regions.len(), 1);
        assert!(doc.has_dot_commands());

        // Check the program parsed successfully
        let program = doc.program.unwrap();
        assert_eq!(program.statements.len(), 1);
        assert!(matches!(program.statements[0], Statement::Select(_)));
    }

    #[test]
    fn test_document_parse_with_dot_commands_disabled() {
        let source = ".open mydb.db\nSELECT 1;";
        let doc = Document::parse(source, false);

        assert_eq!(doc.source, source);
        assert!(doc.dot_commands.is_empty());
        assert_eq!(doc.sql_regions.len(), 1);
        assert!(!doc.has_dot_commands());

        // When dot commands are disabled, ".open" is treated as SQL
        // which will likely cause a parse error
        assert!(doc.program.is_err());
    }

    #[test]
    fn test_document_mixed_sql_and_dot_commands() {
        let source = ".open db1.db\nSELECT 1;\n.open db2.db\nSELECT 2;";
        let doc = Document::parse(source, true);

        assert_eq!(doc.dot_commands.len(), 2);
        assert_eq!(doc.sql_regions.len(), 2);

        // Verify open commands extraction
        let open_paths: Vec<_> = doc.open_commands().collect();
        assert_eq!(open_paths, vec!["db1.db", "db2.db"]);

        // Both SELECT statements should parse
        let program = doc.program.unwrap();
        assert_eq!(program.statements.len(), 2);
    }

    #[test]
    fn test_document_pure_sql_file() {
        let source = "SELECT 1;\nSELECT 2;\nSELECT 3;";
        let doc = Document::parse(source, true);

        assert!(doc.dot_commands.is_empty());
        assert_eq!(doc.sql_regions.len(), 1);
        assert!(!doc.has_dot_commands());

        // All statements should parse
        let program = doc.program.unwrap();
        assert_eq!(program.statements.len(), 3);
    }

    #[test]
    fn test_document_extract_open_commands() {
        let source = ".open first.db\nSELECT * FROM t;\n.open second.db\n.open third.db";
        let doc = Document::parse(source, true);

        let paths: Vec<_> = doc.open_commands().collect();
        assert_eq!(paths, vec!["first.db", "second.db", "third.db"]);
    }

    #[test]
    fn test_document_empty_source() {
        let doc = Document::parse("", true);

        assert!(doc.dot_commands.is_empty());
        assert!(doc.sql_regions.is_empty());
        assert!(!doc.has_dot_commands());

        // Empty source should result in empty program
        let program = doc.program.unwrap();
        assert!(program.statements.is_empty());
    }

    #[test]
    fn test_document_only_dot_commands() {
        let source = ".open db1.db\n.open db2.db";
        let doc = Document::parse(source, true);

        assert_eq!(doc.dot_commands.len(), 2);
        assert!(doc.sql_regions.is_empty());
        assert!(doc.has_dot_commands());

        // No SQL means empty program
        let program = doc.program.unwrap();
        assert!(program.statements.is_empty());
    }

    #[test]
    fn test_document_complex_scenario() {
        let source = r#"-- Setup
.open myapp.db

CREATE TABLE users (id INTEGER PRIMARY KEY);

.open backup.db

SELECT * FROM users;
INSERT INTO users VALUES (1);"#;

        let doc = Document::parse(source, true);

        assert_eq!(doc.dot_commands.len(), 2);
        assert_eq!(doc.sql_regions.len(), 3);

        let paths: Vec<_> = doc.open_commands().collect();
        assert_eq!(paths, vec!["myapp.db", "backup.db"]);

        // Should have 3 statements: CREATE TABLE, SELECT, INSERT
        let program = doc.program.unwrap();
        assert_eq!(program.statements.len(), 3);
    }

    #[test]
    fn test_document_preserves_source() {
        let source = ".open test.db\nSELECT 1;";
        let doc = Document::parse(source, true);

        assert_eq!(doc.source, source);
    }

    #[test]
    fn test_document_sql_regions_without_dot_commands_disabled() {
        let source = "SELECT 1;";
        let doc = Document::parse(source, false);

        // When dot commands are disabled, entire source is one SQL region
        assert_eq!(doc.sql_regions.len(), 1);
        assert_eq!(doc.sql_regions[0].start, 0);
        assert_eq!(doc.sql_regions[0].end, source.len());
    }

    #[test]
    fn test_document_schema_hints() {
        let source = "-- schema: schema.sql\n-- schema: tmp.db\nSELECT 1;";
        let doc = Document::parse(source, true);

        let hints = doc.schema_hints();
        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].path, "schema.sql");
        assert_eq!(hints[1].path, "tmp.db");

        // SQL should still parse fine
        let program = doc.program.unwrap();
        assert_eq!(program.statements.len(), 1);
    }

    #[test]
    fn test_document_schema_hints_disabled() {
        let source = "-- schema: schema.sql\nSELECT 1;";
        let doc = Document::parse(source, false);

        // When dot commands are disabled, no schema hints are parsed
        assert!(doc.schema_hints().is_empty());
    }
}
