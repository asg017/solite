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
    /// Optional result class hint (from `-> ClassName` on the `-- name:` line).
    ///
    /// Multiple exports may share the same class when their column shapes
    /// match; codegen rejects mismatched shapes with an explicit error.
    pub result_class: Option<String>,
}

/// A SQLite extension loaded via `.load` in the input file.
///
/// Fields mirror what the user wrote, not the resolved artifact: for uv
/// loads, `path` is the package spec (e.g. `sqlite-vec==0.1.0`), not the
/// site-packages path it resolved to on this machine.
#[derive(serde::Serialize, Debug, Clone)]
pub struct Extension {
    /// Path to the extension library, or the package spec for uv loads.
    pub path: String,
    /// Optional entry point function name.
    pub entrypoint: Option<String>,
    /// Whether the extension is loaded from a Python package via uv.
    pub is_uv: bool,
}

/// The complete codegen report.
#[derive(serde::Serialize, Debug)]
pub struct Report {
    /// Setup SQL statements (CREATE TABLE, etc.)
    pub setup: Vec<String>,
    /// Extensions loaded via `.load`; generated code must load these into
    /// its connection before running setup or any exported query.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<Extension>,
    /// Exported queries
    pub exports: Vec<Export>,
}

impl Report {
    /// Create a new empty report.
    pub fn new() -> Self {
        Self {
            setup: vec![],
            extensions: vec![],
            exports: vec![],
        }
    }
}

impl Default for Report {
    fn default() -> Self {
        Self::new()
    }
}
