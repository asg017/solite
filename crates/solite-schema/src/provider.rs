//! Schema provider traits for database introspection.
//!
//! This module defines the trait that schema providers must implement
//! to supply table, column, and index information to the LSP and analyzer.
//!
//! # Schema Providers
//!
//! There are three main providers:
//!
//! - [`FileSchemaProvider`]: Loads schema from a SQLite database file (non-WASM only)
//! - [`JsonSchemaProvider`]: Loads schema from a JSON string
//! - [`DdlSchemaProvider`]: Builds schema from parsed DDL statements
//!
//! # Example
//!
//! ```no_run
//! use solite_schema::provider::{SchemaProvider, JsonSchemaProvider};
//!
//! let json = r#"{"tables": [{"name": "users", "columns": [{"name": "id"}]}]}"#;
//! let provider = JsonSchemaProvider::new(json.to_string());
//! let schema = provider.load().unwrap();
//! assert!(schema.has_table("users"));
//! ```

use std::path::Path;

use solite_analyzer::Schema;
use thiserror::Error;

/// Error type for schema loading operations.
#[derive(Error, Debug)]
pub enum SchemaError {
    /// Failed to load schema from a source.
    #[error("Failed to load schema: {0}")]
    LoadError(String),

    /// The specified file was not found.
    #[error("File not found: {0}")]
    FileNotFound(String),

    /// Failed to parse schema data.
    #[error("Parse error: {0}")]
    ParseError(String),

    /// Database operation failed.
    #[error("Database error: {0}")]
    DatabaseError(String),
}

/// Trait for providing database schema information from various sources.
///
/// Implementors of this trait can load schema information from different
/// sources such as SQLite database files, JSON files, or parsed DDL statements.
pub trait SchemaProvider: Send + Sync {
    /// Load and return the schema.
    ///
    /// This method reads the schema from the underlying source and returns
    /// an `solite_analyzer::Schema` that can be used for SQL analysis.
    ///
    /// # Errors
    ///
    /// Returns a `SchemaError` if the schema cannot be loaded.
    fn load(&self) -> Result<Schema, SchemaError>;
}

// ============================================================================
// FileSchemaProvider - SQLite database file introspection (non-WASM only)
// ============================================================================

/// Provides schema from a SQLite database file.
///
/// This provider uses rusqlite to introspect a SQLite database and extract
/// its schema information. Only available on non-WASM targets.
///
/// # Example
///
/// ```no_run
/// use solite_schema::provider::{SchemaProvider, FileSchemaProvider};
///
/// let provider = FileSchemaProvider::new("database.db");
/// let schema = provider.load().unwrap();
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub struct FileSchemaProvider {
    path: std::path::PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileSchemaProvider {
    /// Create a new FileSchemaProvider for the given database path.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Get the database path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl SchemaProvider for FileSchemaProvider {
    fn load(&self) -> Result<Schema, SchemaError> {
        use crate::introspect::{introspect_sqlite_db, IntrospectError};

        let introspected = introspect_sqlite_db(&self.path).map_err(|e| match e {
            IntrospectError::FileNotFound(path) => SchemaError::FileNotFound(path),
            IntrospectError::OpenError(msg) => SchemaError::LoadError(msg),
            IntrospectError::QueryError(msg) => SchemaError::DatabaseError(msg),
        })?;

        // Convert IntrospectedSchema to analyzer Schema
        Ok(introspected_to_analyzer_schema(&introspected))
    }
}

/// Convert an IntrospectedSchema to an analyzer Schema.
#[cfg(not(target_arch = "wasm32"))]
fn introspected_to_analyzer_schema(
    introspected: &crate::introspect::IntrospectedSchema,
) -> Schema {
    use solite_analyzer::TriggerEventType;

    let mut schema = Schema::new();

    // Add tables
    for table in introspected.tables.values() {
        schema.add_table(
            &table.name,
            table.original_columns.clone(),
            table.without_rowid,
        );
    }

    // Add views
    for view in introspected.views.values() {
        schema.add_view(&view.name, view.columns.clone());
    }

    // Add indexes
    for index in introspected.indexes.values() {
        schema.add_index(
            &index.name,
            &index.table_name,
            index.columns.clone(),
            index.is_unique,
        );
    }

    // Add triggers
    // Note: IntrospectedSchema doesn't track trigger event types, so we default to Insert
    // In a more complete implementation, we could parse the SQL to determine the event type
    for trigger in introspected.triggers.values() {
        // Try to determine event type from SQL if available
        let event = trigger
            .sql
            .as_ref()
            .map(|sql| {
                let upper = sql.to_uppercase();
                if upper.contains(" DELETE ") {
                    TriggerEventType::Delete
                } else if upper.contains(" UPDATE ") {
                    TriggerEventType::Update
                } else {
                    TriggerEventType::Insert
                }
            })
            .unwrap_or(TriggerEventType::Insert);

        schema.add_trigger(&trigger.name, &trigger.table_name, event);
    }

    schema
}

// ============================================================================
// JsonSchemaProvider - JSON-based schema loading
// ============================================================================

/// Provides schema from a JSON string.
///
/// This provider parses a JSON string containing schema definitions and
/// converts it to an analyzer Schema. Useful for WASM/browser environments
/// or when schema is provided externally.
///
/// # Example
///
/// ```
/// use solite_schema::provider::{SchemaProvider, JsonSchemaProvider};
///
/// let json = r#"{"tables": [{"name": "users", "columns": [{"name": "id"}]}]}"#;
/// let provider = JsonSchemaProvider::new(json.to_string());
/// let schema = provider.load().unwrap();
/// assert!(schema.has_table("users"));
/// ```
pub struct JsonSchemaProvider {
    json: String,
}

impl JsonSchemaProvider {
    /// Create a new JsonSchemaProvider with the given JSON string.
    pub fn new(json: String) -> Self {
        Self { json }
    }

    /// Create a new JsonSchemaProvider from a JSON string slice.
    pub fn from_json_str(json: &str) -> Self {
        Self {
            json: json.to_string(),
        }
    }

    /// Get the JSON string.
    pub fn json(&self) -> &str {
        &self.json
    }
}

impl SchemaProvider for JsonSchemaProvider {
    fn load(&self) -> Result<Schema, SchemaError> {
        use crate::json::{JsonSchema, JsonSchemaError};

        let json_schema =
            JsonSchema::from_json(&self.json).map_err(|e| match e {
                JsonSchemaError::ParseError(msg) => SchemaError::ParseError(msg),
                JsonSchemaError::ValidationError(msg) => SchemaError::ParseError(msg),
                JsonSchemaError::SerializeError(msg) => SchemaError::ParseError(msg),
            })?;

        // Optionally validate the schema
        json_schema.validate().map_err(|e| match e {
            JsonSchemaError::ParseError(msg) => SchemaError::ParseError(msg),
            JsonSchemaError::ValidationError(msg) => SchemaError::ParseError(msg),
            JsonSchemaError::SerializeError(msg) => SchemaError::ParseError(msg),
        })?;

        Ok(json_schema.to_analyzer_schema())
    }
}

// ============================================================================
// DdlSchemaProvider - Schema from parsed DDL statements
// ============================================================================

/// Provides schema from parsed DDL statements.
///
/// This provider extracts schema information from a parsed SQL program
/// containing DDL statements (CREATE TABLE, CREATE INDEX, etc.).
///
/// # Example
///
/// ```
/// use solite_schema::provider::{SchemaProvider, DdlSchemaProvider};
/// use solite_parser::parse_program;
///
/// let sql = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);";
/// let program = parse_program(sql).unwrap();
/// let provider = DdlSchemaProvider::new(program);
/// let schema = provider.load().unwrap();
/// assert!(schema.has_table("users"));
/// ```
#[derive(Debug, Clone)]
pub struct DdlSchemaProvider {
    program: solite_ast::Program,
}

impl DdlSchemaProvider {
    /// Create a new DdlSchemaProvider with the given parsed program.
    pub fn new(program: solite_ast::Program) -> Self {
        Self { program }
    }

    /// Create a new DdlSchemaProvider by parsing the given SQL.
    ///
    /// # Errors
    ///
    /// Returns an error if the SQL cannot be parsed.
    pub fn from_sql(sql: &str) -> Result<Self, SchemaError> {
        let program = solite_parser::parse_program(sql).map_err(|errors| {
            let messages: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
            SchemaError::ParseError(messages.join("; "))
        })?;
        Ok(Self { program })
    }

    /// Get a reference to the program.
    pub fn program(&self) -> &solite_ast::Program {
        &self.program
    }
}

impl SchemaProvider for DdlSchemaProvider {
    fn load(&self) -> Result<Schema, SchemaError> {
        Ok(solite_analyzer::build_schema(&self.program))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // JsonSchemaProvider tests
    // ========================================

    #[test]
    fn test_json_provider_simple() {
        let json = r#"{"tables": [{"name": "users", "columns": [{"name": "id"}]}]}"#;
        let provider = JsonSchemaProvider::new(json.to_string());
        let schema = provider.load().unwrap();

        assert!(schema.has_table("users"));
        assert!(schema.has_table("USERS")); // case insensitive
    }

    #[test]
    fn test_json_provider_from_str() {
        let json = r#"{"tables": [{"name": "items", "columns": [{"name": "sku"}]}]}"#;
        let provider = JsonSchemaProvider::from_json_str(json);
        let schema = provider.load().unwrap();

        assert!(schema.has_table("items"));
    }

    #[test]
    fn test_json_provider_with_views() {
        let json = r#"{
            "tables": [{"name": "users", "columns": [{"name": "id"}]}],
            "views": [{"name": "v_users", "columns": ["id"]}]
        }"#;
        let provider = JsonSchemaProvider::new(json.to_string());
        let schema = provider.load().unwrap();

        assert!(schema.has_table("users"));
        assert!(schema.has_view("v_users"));
    }

    #[test]
    fn test_json_provider_with_indexes() {
        let json = r#"{
            "tables": [{"name": "users", "columns": [{"name": "id"}, {"name": "email"}]}],
            "indexes": [{"name": "idx_email", "table_name": "users", "columns": ["email"], "unique": true}]
        }"#;
        let provider = JsonSchemaProvider::new(json.to_string());
        let schema = provider.load().unwrap();

        assert!(schema.has_index("idx_email"));
        let idx = schema.get_index("idx_email").unwrap();
        assert!(idx.is_unique);
        assert_eq!(idx.table_name, "users");
    }

    #[test]
    fn test_json_provider_with_triggers() {
        let json = r#"{
            "tables": [{"name": "users", "columns": [{"name": "id"}]}],
            "triggers": [{"name": "trg_audit", "table_name": "users", "event": "INSERT"}]
        }"#;
        let provider = JsonSchemaProvider::new(json.to_string());
        let schema = provider.load().unwrap();

        assert!(schema.has_trigger("trg_audit"));
    }

    #[test]
    fn test_json_provider_empty_schema() {
        let json = "{}";
        let provider = JsonSchemaProvider::new(json.to_string());
        let schema = provider.load().unwrap();

        assert_eq!(schema.table_names().count(), 0);
    }

    #[test]
    fn test_json_provider_invalid_json() {
        let json = "{ not valid json }";
        let provider = JsonSchemaProvider::new(json.to_string());
        let result = provider.load();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SchemaError::ParseError(_)));
    }

    #[test]
    fn test_json_provider_validation_error() {
        // Duplicate table names should fail validation
        let json = r#"{
            "tables": [
                {"name": "users", "columns": [{"name": "id"}]},
                {"name": "users", "columns": [{"name": "name"}]}
            ]
        }"#;
        let provider = JsonSchemaProvider::new(json.to_string());
        let result = provider.load();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SchemaError::ParseError(_)));
        assert!(err.to_string().contains("Duplicate table"));
    }

    #[test]
    fn test_json_provider_full_schema() {
        let json = r#"{
            "tables": [
                {
                    "name": "users",
                    "columns": [
                        {"name": "id", "type": "INTEGER", "primary_key": true},
                        {"name": "email", "type": "TEXT", "not_null": true}
                    ]
                },
                {
                    "name": "settings",
                    "columns": [{"name": "key"}, {"name": "value"}],
                    "without_rowid": true
                }
            ],
            "views": [
                {"name": "v_users", "columns": ["id", "email"]}
            ],
            "indexes": [
                {"name": "idx_users_email", "table_name": "users", "columns": ["email"], "unique": true}
            ],
            "triggers": [
                {"name": "trg_users_insert", "table_name": "users", "event": "INSERT"},
                {"name": "trg_users_update", "table_name": "users", "event": "UPDATE"},
                {"name": "trg_users_delete", "table_name": "users", "event": "DELETE"}
            ]
        }"#;
        let provider = JsonSchemaProvider::new(json.to_string());
        let schema = provider.load().unwrap();

        // Check tables
        assert!(schema.has_table("users"));
        assert!(schema.has_table("settings"));
        assert!(schema.get_table("settings").unwrap().without_rowid);

        // Check columns
        let user_cols = schema.columns_for_table("users").unwrap();
        assert_eq!(user_cols.len(), 2);

        // Check views
        assert!(schema.has_view("v_users"));
        let view_cols = schema.columns_for_view("v_users").unwrap();
        assert_eq!(view_cols, &["id", "email"]);

        // Check indexes
        assert!(schema.has_index("idx_users_email"));

        // Check triggers
        assert!(schema.has_trigger("trg_users_insert"));
        assert!(schema.has_trigger("trg_users_update"));
        assert!(schema.has_trigger("trg_users_delete"));
    }

    // ========================================
    // DdlSchemaProvider tests
    // ========================================

    #[test]
    fn test_ddl_provider_simple_table() {
        let sql = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);";
        let program = solite_parser::parse_program(sql).unwrap();
        let provider = DdlSchemaProvider::new(program);
        let schema = provider.load().unwrap();

        assert!(schema.has_table("users"));
        let cols = schema.columns_for_table("users").unwrap();
        assert_eq!(cols.len(), 2);
        assert!(cols.contains(&"id".to_string()));
        assert!(cols.contains(&"name".to_string()));
    }

    #[test]
    fn test_ddl_provider_from_sql() {
        let sql = "CREATE TABLE items (sku TEXT PRIMARY KEY) WITHOUT ROWID;";
        let provider = DdlSchemaProvider::from_sql(sql).unwrap();
        let schema = provider.load().unwrap();

        assert!(schema.has_table("items"));
        assert!(schema.get_table("items").unwrap().without_rowid);
    }

    #[test]
    fn test_ddl_provider_from_sql_invalid() {
        let sql = "CREATE TABLE (invalid syntax";
        let result = DdlSchemaProvider::from_sql(sql);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SchemaError::ParseError(_)));
    }

    #[test]
    fn test_ddl_provider_multiple_statements() {
        let sql = r#"
            CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT);
            CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER);
            CREATE INDEX idx_orders_user ON orders(user_id);
            CREATE VIEW v_order_count AS SELECT user_id, COUNT(*) as cnt FROM orders GROUP BY user_id;
        "#;
        let provider = DdlSchemaProvider::from_sql(sql).unwrap();
        let schema = provider.load().unwrap();

        // Check tables
        assert!(schema.has_table("users"));
        assert!(schema.has_table("orders"));

        // Check index
        assert!(schema.has_index("idx_orders_user"));
        let idx = schema.get_index("idx_orders_user").unwrap();
        assert_eq!(idx.table_name, "orders");

        // Check view
        assert!(schema.has_view("v_order_count"));
    }

    #[test]
    fn test_ddl_provider_with_triggers() {
        let sql = r#"
            CREATE TABLE users (id INTEGER PRIMARY KEY);
            CREATE TRIGGER trg_users_insert AFTER INSERT ON users BEGIN SELECT 1; END;
        "#;
        let provider = DdlSchemaProvider::from_sql(sql).unwrap();
        let schema = provider.load().unwrap();

        assert!(schema.has_trigger("trg_users_insert"));
        let trg = schema.get_trigger("trg_users_insert").unwrap();
        assert_eq!(trg.table_name, "users");
    }

    #[test]
    fn test_ddl_provider_drop_removes_objects() {
        let sql = r#"
            CREATE TABLE users (id INTEGER PRIMARY KEY);
            CREATE INDEX idx_users_id ON users(id);
            DROP INDEX idx_users_id;
            DROP TABLE users;
        "#;
        let provider = DdlSchemaProvider::from_sql(sql).unwrap();
        let schema = provider.load().unwrap();

        // Both should be removed by DROP statements
        assert!(!schema.has_table("users"));
        assert!(!schema.has_index("idx_users_id"));
    }

    #[test]
    fn test_ddl_provider_empty_program() {
        let sql = "-- Just a comment\n";
        let program = solite_parser::parse_program(sql).unwrap();
        let provider = DdlSchemaProvider::new(program);
        let schema = provider.load().unwrap();

        assert_eq!(schema.table_names().count(), 0);
    }

    #[test]
    fn test_ddl_provider_program_accessor() {
        let sql = "CREATE TABLE t (a INTEGER);";
        let program = solite_parser::parse_program(sql).unwrap();
        let provider = DdlSchemaProvider::new(program);

        // Verify we can access the program
        assert_eq!(provider.program().statements.len(), 1);
    }

    // ========================================
    // FileSchemaProvider tests (non-WASM only)
    // ========================================

    #[cfg(not(target_arch = "wasm32"))]
    mod file_provider_tests {
        use super::*;
        use rusqlite::Connection;
        use std::fs;
        use std::path::PathBuf;

        fn temp_db_path(name: &str) -> PathBuf {
            let path = std::env::temp_dir().join(format!("solite_test_{}.db", name));
            // Clean up any existing file
            let _ = fs::remove_file(&path);
            path
        }

        fn create_test_db(path: &Path) -> Connection {
            Connection::open(path).unwrap()
        }

        #[test]
        fn test_file_provider_simple() {
            let path = temp_db_path("file_provider_simple");
            let conn = create_test_db(&path);
            conn.execute(
                "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)",
                [],
            )
            .unwrap();
            drop(conn);

            let provider = FileSchemaProvider::new(&path);
            let schema = provider.load().unwrap();

            assert!(schema.has_table("users"));
            let cols = schema.columns_for_table("users").unwrap();
            assert!(cols.contains(&"id".to_string()));
            assert!(cols.contains(&"name".to_string()));

            // Clean up
            let _ = fs::remove_file(&path);
        }

        #[test]
        fn test_file_provider_path_accessor() {
            let provider = FileSchemaProvider::new("/some/path.db");
            assert_eq!(provider.path().to_str().unwrap(), "/some/path.db");
        }

        #[test]
        fn test_file_provider_file_not_found() {
            let provider = FileSchemaProvider::new("/nonexistent/path/to/database.db");
            let result = provider.load();

            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(matches!(err, SchemaError::FileNotFound(_)));
        }

        #[test]
        fn test_file_provider_with_views() {
            let path = temp_db_path("file_provider_views");
            let conn = create_test_db(&path);
            conn.execute("CREATE TABLE users (id INTEGER, name TEXT)", [])
                .unwrap();
            conn.execute(
                "CREATE VIEW v_users AS SELECT id, name FROM users",
                [],
            )
            .unwrap();
            drop(conn);

            let provider = FileSchemaProvider::new(&path);
            let schema = provider.load().unwrap();

            assert!(schema.has_view("v_users"));
            let cols = schema.columns_for_view("v_users").unwrap();
            assert_eq!(cols, &["id", "name"]);

            let _ = fs::remove_file(&path);
        }

        #[test]
        fn test_file_provider_with_indexes() {
            let path = temp_db_path("file_provider_indexes");
            let conn = create_test_db(&path);
            conn.execute("CREATE TABLE users (id INTEGER, email TEXT)", [])
                .unwrap();
            conn.execute("CREATE UNIQUE INDEX idx_email ON users(email)", [])
                .unwrap();
            drop(conn);

            let provider = FileSchemaProvider::new(&path);
            let schema = provider.load().unwrap();

            assert!(schema.has_index("idx_email"));
            let idx = schema.get_index("idx_email").unwrap();
            assert!(idx.is_unique);
            assert_eq!(idx.table_name, "users");

            let _ = fs::remove_file(&path);
        }

        #[test]
        fn test_file_provider_with_triggers() {
            let path = temp_db_path("file_provider_triggers");
            let conn = create_test_db(&path);
            conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY)", [])
                .unwrap();
            conn.execute(
                "CREATE TRIGGER trg_insert AFTER INSERT ON users BEGIN SELECT 1; END",
                [],
            )
            .unwrap();
            conn.execute(
                "CREATE TRIGGER trg_delete AFTER DELETE ON users BEGIN SELECT 1; END",
                [],
            )
            .unwrap();
            drop(conn);

            let provider = FileSchemaProvider::new(&path);
            let schema = provider.load().unwrap();

            assert!(schema.has_trigger("trg_insert"));
            assert!(schema.has_trigger("trg_delete"));

            // Check that trigger event types are detected from SQL
            let trg_insert = schema.get_trigger("trg_insert").unwrap();
            assert_eq!(trg_insert.event, solite_analyzer::TriggerEventType::Insert);

            let trg_delete = schema.get_trigger("trg_delete").unwrap();
            assert_eq!(trg_delete.event, solite_analyzer::TriggerEventType::Delete);

            let _ = fs::remove_file(&path);
        }

        #[test]
        fn test_file_provider_without_rowid() {
            let path = temp_db_path("file_provider_without_rowid");
            let conn = create_test_db(&path);
            conn.execute(
                "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT) WITHOUT ROWID",
                [],
            )
            .unwrap();
            drop(conn);

            let provider = FileSchemaProvider::new(&path);
            let schema = provider.load().unwrap();

            assert!(schema.has_table("settings"));
            assert!(schema.get_table("settings").unwrap().without_rowid);

            let _ = fs::remove_file(&path);
        }

        #[test]
        fn test_file_provider_complex_schema() {
            let path = temp_db_path("file_provider_complex");
            let conn = create_test_db(&path);
            conn.execute_batch(
                r#"
                CREATE TABLE users (
                    id INTEGER PRIMARY KEY,
                    email TEXT NOT NULL UNIQUE,
                    name TEXT
                );
                CREATE TABLE orders (
                    id INTEGER PRIMARY KEY,
                    user_id INTEGER REFERENCES users(id),
                    total REAL
                );
                CREATE INDEX idx_orders_user ON orders(user_id);
                CREATE VIEW v_order_totals AS
                    SELECT u.name, SUM(o.total) as total
                    FROM users u
                    LEFT JOIN orders o ON u.id = o.user_id
                    GROUP BY u.id;
                CREATE TRIGGER trg_orders_audit AFTER INSERT ON orders BEGIN SELECT 1; END;
            "#,
            )
            .unwrap();
            drop(conn);

            let provider = FileSchemaProvider::new(&path);
            let schema = provider.load().unwrap();

            // Verify tables
            assert!(schema.has_table("users"));
            assert!(schema.has_table("orders"));

            // Verify columns
            let user_cols = schema.columns_for_table("users").unwrap();
            assert_eq!(user_cols.len(), 3);

            // Verify index
            assert!(schema.has_index("idx_orders_user"));

            // Verify view
            assert!(schema.has_view("v_order_totals"));

            // Verify trigger
            assert!(schema.has_trigger("trg_orders_audit"));

            let _ = fs::remove_file(&path);
        }
    }

    // ========================================
    // SchemaProvider trait object tests
    // ========================================

    #[test]
    fn test_provider_as_trait_object() {
        // Verify that providers can be used as trait objects
        let json = r#"{"tables": [{"name": "users", "columns": [{"name": "id"}]}]}"#;
        let provider: Box<dyn SchemaProvider> = Box::new(JsonSchemaProvider::new(json.to_string()));
        let schema = provider.load().unwrap();

        assert!(schema.has_table("users"));
    }

    #[test]
    fn test_multiple_providers_in_vec() {
        // Verify that different provider types can be stored together
        let json = r#"{"tables": [{"name": "from_json", "columns": [{"name": "id"}]}]}"#;
        let sql = "CREATE TABLE from_ddl (id INTEGER);";

        let providers: Vec<Box<dyn SchemaProvider>> = vec![
            Box::new(JsonSchemaProvider::new(json.to_string())),
            Box::new(DdlSchemaProvider::from_sql(sql).unwrap()),
        ];

        let schema1 = providers[0].load().unwrap();
        let schema2 = providers[1].load().unwrap();

        assert!(schema1.has_table("from_json"));
        assert!(schema2.has_table("from_ddl"));
    }
}
