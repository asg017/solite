//! Schema source trait for providing database metadata.
//!
//! This trait abstracts over different sources of schema information,
//! allowing the completion engine to work with both static analyzer schemas
//! and live database connections.

/// A source of database schema information.
///
/// This trait provides access to database metadata needed for SQL completions.
/// Implementations can wrap static schema analysis results or query a live database.
pub trait SchemaSource {
    /// Get all table names in the schema.
    fn table_names(&self) -> Vec<String>;

    /// Get column names for a specific table.
    /// Returns None if the table doesn't exist.
    fn columns_for_table(&self, table: &str) -> Option<Vec<String>>;

    /// Get column names for a table, including the implicit rowid column.
    /// Returns None if the table doesn't exist.
    fn columns_for_table_with_rowid(&self, table: &str) -> Option<Vec<String>> {
        // Default implementation just returns columns without rowid
        self.columns_for_table(table)
    }

    /// Check if a table exists in the schema.
    fn has_table(&self, name: &str) -> bool {
        self.table_names().iter().any(|t| t.eq_ignore_ascii_case(name))
    }

    /// Get all index names in the schema.
    fn index_names(&self) -> Vec<String>;

    /// Get all view names in the schema.
    fn view_names(&self) -> Vec<String>;

    /// Get all scalar function names available in the database.
    fn function_names(&self) -> Vec<String> {
        vec![]
    }

    /// Get valid argument counts for a function (case-insensitive).
    /// Returns None if unknown. A value of -1 means variadic.
    fn function_nargs(&self, _name: &str) -> Option<Vec<i32>> {
        None
    }
}

// Implementation for solite_analyzer::Schema when the analyzer feature is enabled
#[cfg(feature = "analyzer")]
impl SchemaSource for solite_analyzer::Schema {
    fn table_names(&self) -> Vec<String> {
        solite_analyzer::Schema::table_names(self)
            .map(|s| s.to_string())
            .collect()
    }

    fn columns_for_table(&self, table: &str) -> Option<Vec<String>> {
        solite_analyzer::Schema::columns_for_table(self, table)
            .map(|cols| cols.to_vec())
    }

    fn columns_for_table_with_rowid(&self, table: &str) -> Option<Vec<String>> {
        solite_analyzer::Schema::columns_for_table_with_rowid(self, table)
    }

    fn index_names(&self) -> Vec<String> {
        solite_analyzer::Schema::index_names(self)
            .map(|s| s.to_string())
            .collect()
    }

    fn view_names(&self) -> Vec<String> {
        solite_analyzer::Schema::view_names(self)
            .map(|s| s.to_string())
            .collect()
    }

    fn function_names(&self) -> Vec<String> {
        self.function_names_list().to_vec()
    }

    fn function_nargs(&self, name: &str) -> Option<Vec<i32>> {
        solite_analyzer::Schema::function_nargs(self, name).map(|s| s.to_vec())
    }
}
