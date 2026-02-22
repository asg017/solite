//! Type definitions for codegen output.

use solite_core::sqlite::ColumnMeta;

pub use solite_core::procedure::ProcedureParam as Parameter;
pub use solite_core::procedure::ResultType;

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
