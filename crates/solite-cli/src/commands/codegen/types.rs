//! Type definitions for codegen output.

use solite_core::sqlite::ColumnMeta;

/// A SQL parameter with optional type annotation.
///
/// Parameters can be annotated with types using the `::type` syntax:
/// - `$name::text` - parameter named "name" with type "text"
/// - `$id::int` - parameter named "id" with type "int"
#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub struct Parameter {
    /// The full parameter name as it appears in SQL (e.g., "$name::text")
    pub full_name: String,
    /// The parameter name without prefix or type (e.g., "name")
    pub name: String,
    /// The annotated type, if any (e.g., "text")
    pub annotated_type: Option<String>,
}

/// The expected result type of a query.
#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub enum ResultType {
    /// Query returns no results (INSERT, UPDATE, DELETE, etc.)
    Void,
    /// Query returns multiple rows
    Rows,
    /// Query returns exactly one row
    Row,
    /// Query returns a single value
    Value,
    /// Query returns a list of single values
    List,
}

/// An exported query with its metadata.
#[derive(serde::Serialize, Debug, Clone)]
pub struct Export {
    /// The name of the export (from `-- name: xxx`)
    pub name: String,
    /// The result type annotation
    pub result_type: ResultType,
    /// The SQL query text
    pub sql: String,
    /// Parameters used in the query
    pub parameters: Vec<Parameter>,
    /// Column metadata for the result set
    pub columns: Vec<ColumnMeta>,
}

/// The complete codegen report.
#[derive(serde::Serialize, Debug)]
pub struct Report {
    /// Setup SQL statements (CREATE TABLE, etc.)
    pub setup: Vec<String>,
    /// Exported queries
    pub exports: Vec<Export>,
}

impl Report {
    /// Create a new empty report.
    pub fn new() -> Self {
        Self {
            setup: vec![],
            exports: vec![],
        }
    }
}

impl Default for Report {
    fn default() -> Self {
        Self::new()
    }
}
