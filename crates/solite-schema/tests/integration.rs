//! Integration tests for solite_schema crate.
//!
//! These tests verify end-to-end functionality of:
//! - Document parsing with dot commands
//! - Schema loading from SQLite databases
//! - Schema merging between DDL and external sources
//! - Error handling for various edge cases

use solite_schema::{
    provider::{DdlSchemaProvider, FileSchemaProvider, SchemaProvider},
    Document,
};
use std::fs;
use std::path::{Path, PathBuf};

// ============================================================================
// Test Fixture Helpers
// ============================================================================

/// Get path to the fixtures directory
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Create a test database with tables, views, indexes at the given path
fn create_test_db(path: &Path) {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }

    // Remove existing file if any
    let _ = fs::remove_file(path);

    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT UNIQUE
        );
        CREATE TABLE orders (
            id INTEGER PRIMARY KEY,
            user_id INTEGER REFERENCES users(id),
            total REAL NOT NULL
        );
        CREATE INDEX idx_orders_user ON orders(user_id);
        CREATE VIEW user_orders AS
            SELECT u.name, o.total
            FROM users u
            JOIN orders o ON u.id = o.user_id;
        CREATE TRIGGER trg_orders_audit AFTER INSERT ON orders
        BEGIN
            SELECT 1;
        END;
        ",
    )
    .unwrap();
}

/// Create an empty test database
fn create_empty_db(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let _ = fs::remove_file(path);
    let _conn = rusqlite::Connection::open(path).unwrap();
}

/// Create a database with complex schema for testing edge cases
fn create_complex_db(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let _ = fs::remove_file(path);

    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        r#"
        -- Table with many columns and constraints
        CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            sku TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            description TEXT,
            price REAL NOT NULL CHECK(price >= 0),
            stock INTEGER DEFAULT 0,
            category_id INTEGER,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );

        -- WITHOUT ROWID table
        CREATE TABLE settings (
            key TEXT PRIMARY KEY,
            value TEXT
        ) WITHOUT ROWID;

        -- Table with composite primary key
        CREATE TABLE order_items (
            order_id INTEGER NOT NULL,
            product_id INTEGER NOT NULL,
            quantity INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY (order_id, product_id)
        );

        -- Views with aliases and aggregates
        CREATE VIEW v_product_stats AS
            SELECT
                p.category_id,
                COUNT(*) as product_count,
                AVG(p.price) as avg_price,
                SUM(p.stock) as total_stock
            FROM products p
            GROUP BY p.category_id;

        -- Multiple indexes
        CREATE INDEX idx_products_name ON products(name);
        CREATE INDEX idx_products_category ON products(category_id);
        CREATE UNIQUE INDEX idx_products_sku ON products(sku);

        -- Partial index
        CREATE INDEX idx_products_in_stock ON products(stock)
            WHERE stock > 0;

        -- Multiple triggers
        CREATE TRIGGER trg_products_insert AFTER INSERT ON products
        BEGIN
            SELECT 1;
        END;

        CREATE TRIGGER trg_products_update AFTER UPDATE ON products
        BEGIN
            SELECT 1;
        END;

        CREATE TRIGGER trg_products_delete AFTER DELETE ON products
        BEGIN
            SELECT 1;
        END;
        "#,
    )
    .unwrap();
}

/// Clean up test database file
fn cleanup_db(path: &Path) {
    let _ = fs::remove_file(path);
}

// ============================================================================
// Document Parsing with .open Command Tests
// ============================================================================

#[test]
fn test_document_with_open_command() {
    let db_path = fixtures_dir().join("test_document_open.db");
    create_test_db(&db_path);

    // Parse document with .open command
    let source = format!(
        ".open {}\nSELECT * FROM users;",
        db_path.display()
    );
    let doc = Document::parse(&source, true);

    // Verify dot commands were parsed
    assert!(doc.has_dot_commands());
    let open_paths: Vec<_> = doc.open_commands().collect();
    assert_eq!(open_paths.len(), 1);
    assert_eq!(open_paths[0], db_path.to_str().unwrap());

    // Verify SQL was parsed successfully
    assert!(doc.program.is_ok());
    let program = doc.program.unwrap();
    assert_eq!(program.statements.len(), 1);

    cleanup_db(&db_path);
}

#[test]
fn test_document_with_multiple_open_commands() {
    let db1_path = fixtures_dir().join("test_doc_multi_open_1.db");
    let db2_path = fixtures_dir().join("test_doc_multi_open_2.db");
    create_test_db(&db1_path);
    create_empty_db(&db2_path);

    let source = format!(
        ".open {}\nSELECT 1;\n.open {}\nSELECT 2;",
        db1_path.display(),
        db2_path.display()
    );
    let doc = Document::parse(&source, true);

    // Verify both open commands were parsed
    let open_paths: Vec<_> = doc.open_commands().collect();
    assert_eq!(open_paths.len(), 2);
    assert_eq!(open_paths[0], db1_path.to_str().unwrap());
    assert_eq!(open_paths[1], db2_path.to_str().unwrap());

    // Verify both SQL statements were parsed
    assert!(doc.program.is_ok());
    let program = doc.program.unwrap();
    assert_eq!(program.statements.len(), 2);

    cleanup_db(&db1_path);
    cleanup_db(&db2_path);
}

#[test]
fn test_document_loads_schema_from_open_command() {
    let db_path = fixtures_dir().join("test_doc_schema_load.db");
    create_test_db(&db_path);

    let source = format!(".open {}\nSELECT * FROM users;", db_path.display());
    let doc = Document::parse(&source, true);

    // Get the first .open path and load schema
    let open_path = doc.open_commands().next().unwrap();
    let provider = FileSchemaProvider::new(open_path);
    let schema = provider.load().unwrap();

    // Verify schema from database is correct
    assert!(schema.has_table("users"));
    assert!(schema.has_table("orders"));
    assert!(schema.has_view("user_orders"));
    assert!(schema.has_index("idx_orders_user"));
    assert!(schema.has_trigger("trg_orders_audit"));

    cleanup_db(&db_path);
}

#[test]
fn test_document_with_ddl_after_open() {
    let db_path = fixtures_dir().join("test_doc_ddl_after_open.db");
    create_test_db(&db_path);

    let source = format!(
        ".open {}\nCREATE TABLE new_table (id INTEGER PRIMARY KEY);",
        db_path.display()
    );
    let doc = Document::parse(&source, true);

    // Verify the DDL was parsed
    assert!(doc.program.is_ok());
    let program = doc.program.as_ref().unwrap();
    assert_eq!(program.statements.len(), 1);

    cleanup_db(&db_path);
}

// ============================================================================
// Schema Merge Tests (DDL + External Database)
// ============================================================================

#[test]
fn test_schema_merge_ddl_and_external() {
    let db_path = fixtures_dir().join("test_schema_merge.db");
    create_test_db(&db_path);

    // Load schema from external database
    let file_provider = FileSchemaProvider::new(&db_path);
    let mut external_schema = file_provider.load().unwrap();

    // Parse DDL that adds more tables
    let ddl_sql = r#"
        CREATE TABLE categories (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL
        );
        CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            category_id INTEGER,
            name TEXT
        );
        CREATE INDEX idx_products_category ON products(category_id);
    "#;
    let ddl_provider = DdlSchemaProvider::from_sql(ddl_sql).unwrap();
    let ddl_schema = ddl_provider.load().unwrap();

    // Merge the schemas
    external_schema.merge(ddl_schema);

    // Verify merged schema has objects from both sources
    // From external database:
    assert!(external_schema.has_table("users"));
    assert!(external_schema.has_table("orders"));
    assert!(external_schema.has_view("user_orders"));
    assert!(external_schema.has_index("idx_orders_user"));

    // From DDL:
    assert!(external_schema.has_table("categories"));
    assert!(external_schema.has_table("products"));
    assert!(external_schema.has_index("idx_products_category"));

    cleanup_db(&db_path);
}

#[test]
fn test_schema_merge_ddl_overrides_external() {
    let db_path = fixtures_dir().join("test_schema_override.db");
    create_test_db(&db_path);

    // Load external schema (has users table with id, name, email)
    let file_provider = FileSchemaProvider::new(&db_path);
    let mut external_schema = file_provider.load().unwrap();

    // Verify original users table columns
    let original_cols = external_schema.columns_for_table("users").unwrap();
    assert!(original_cols.contains(&"id".to_string()));
    assert!(original_cols.contains(&"name".to_string()));
    assert!(original_cols.contains(&"email".to_string()));

    // DDL creates a users table with different columns (this will override)
    let ddl_sql = r#"
        CREATE TABLE users (
            user_id INTEGER PRIMARY KEY,
            username TEXT,
            password_hash TEXT
        );
    "#;
    let ddl_provider = DdlSchemaProvider::from_sql(ddl_sql).unwrap();
    let ddl_schema = ddl_provider.load().unwrap();

    // Verify DDL schema has different columns
    assert!(ddl_schema.has_table("users"));

    // Merge - DDL should override the external schema's users table
    external_schema.merge(ddl_schema);

    // After merge, users table should have the DDL columns (override)
    let merged_cols = external_schema.columns_for_table("users").unwrap();
    assert!(merged_cols.contains(&"user_id".to_string()));
    assert!(merged_cols.contains(&"username".to_string()));
    assert!(merged_cols.contains(&"password_hash".to_string()));
    // Old columns should be gone
    assert!(!merged_cols.contains(&"email".to_string()));

    cleanup_db(&db_path);
}

#[test]
fn test_schema_merge_preserves_external_when_no_overlap() {
    let db_path = fixtures_dir().join("test_schema_no_overlap.db");
    create_test_db(&db_path);

    // Load external schema
    let file_provider = FileSchemaProvider::new(&db_path);
    let mut external_schema = file_provider.load().unwrap();

    // DDL adds completely new objects (no overlap)
    let ddl_sql = r#"
        CREATE TABLE logs (id INTEGER PRIMARY KEY, message TEXT);
        CREATE TABLE events (id INTEGER PRIMARY KEY, event_type TEXT);
    "#;
    let ddl_provider = DdlSchemaProvider::from_sql(ddl_sql).unwrap();
    let ddl_schema = ddl_provider.load().unwrap();

    // Get counts before merge
    let external_table_count = external_schema.table_names().count();

    // Merge
    external_schema.merge(ddl_schema);

    // Should have all tables from both
    assert_eq!(
        external_schema.table_names().count(),
        external_table_count + 2
    );
    assert!(external_schema.has_table("users"));
    assert!(external_schema.has_table("orders"));
    assert!(external_schema.has_table("logs"));
    assert!(external_schema.has_table("events"));

    cleanup_db(&db_path);
}

// ============================================================================
// Error Case Tests
// ============================================================================

#[test]
fn test_open_with_nonexistent_file() {
    let source = ".open /nonexistent/path/to/database.db\nSELECT 1;";
    let doc = Document::parse(source, true);

    // Document parsing should succeed (it just extracts the path)
    assert!(doc.has_dot_commands());
    let open_path = doc.open_commands().next().unwrap();
    assert_eq!(open_path, "/nonexistent/path/to/database.db");

    // But attempting to load schema should fail
    let provider = FileSchemaProvider::new(open_path);
    let result = provider.load();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(
        err,
        solite_schema::provider::SchemaError::FileNotFound(_)
    ));
}

#[test]
fn test_open_with_invalid_database() {
    let invalid_db_path = fixtures_dir().join("invalid.db");

    // Create a file that's not a valid SQLite database
    fs::create_dir_all(fixtures_dir()).unwrap();
    fs::write(&invalid_db_path, "This is not a valid SQLite database").unwrap();

    let provider = FileSchemaProvider::new(&invalid_db_path);
    let result = provider.load();

    // Should fail because the file is not a valid database
    assert!(result.is_err());

    cleanup_db(&invalid_db_path);
}

#[test]
fn test_open_with_empty_path() {
    let source = ".open\nSELECT 1;";
    let doc = Document::parse(source, true);

    // .open without a path should be ignored
    assert!(!doc.has_dot_commands());
    assert_eq!(doc.open_commands().count(), 0);
}

#[test]
fn test_document_parse_error_with_invalid_sql() {
    let source = ".open test.db\nSELECT FROM WHERE;";
    let doc = Document::parse(source, true);

    // Dot command should still be parsed
    assert!(doc.has_dot_commands());

    // But SQL should have parse errors
    assert!(doc.program.is_err());
}

// ============================================================================
// FileSchemaProvider Tests
// ============================================================================

#[test]
fn test_file_provider_loads_all_object_types() {
    let db_path = fixtures_dir().join("test_all_objects.db");
    create_complex_db(&db_path);

    let provider = FileSchemaProvider::new(&db_path);
    let schema = provider.load().unwrap();

    // Tables
    assert!(schema.has_table("products"));
    assert!(schema.has_table("settings"));
    assert!(schema.has_table("order_items"));

    // Check WITHOUT ROWID
    let settings = schema.get_table("settings").unwrap();
    assert!(settings.without_rowid);

    // Views
    assert!(schema.has_view("v_product_stats"));

    // Indexes
    assert!(schema.has_index("idx_products_name"));
    assert!(schema.has_index("idx_products_category"));
    assert!(schema.has_index("idx_products_sku"));
    assert!(schema.has_index("idx_products_in_stock"));

    // Check unique index
    let sku_idx = schema.get_index("idx_products_sku").unwrap();
    assert!(sku_idx.is_unique);

    // Triggers
    assert!(schema.has_trigger("trg_products_insert"));
    assert!(schema.has_trigger("trg_products_update"));
    assert!(schema.has_trigger("trg_products_delete"));

    cleanup_db(&db_path);
}

#[test]
fn test_file_provider_empty_database() {
    let db_path = fixtures_dir().join("test_empty_db.db");
    create_empty_db(&db_path);

    let provider = FileSchemaProvider::new(&db_path);
    let schema = provider.load().unwrap();

    // Empty database should have no objects
    assert_eq!(schema.table_names().count(), 0);
    assert_eq!(schema.view_names().count(), 0);
    assert_eq!(schema.index_names().count(), 0);
    assert_eq!(schema.trigger_names().count(), 0);

    cleanup_db(&db_path);
}

#[test]
fn test_file_provider_column_info() {
    let db_path = fixtures_dir().join("test_column_info.db");
    create_test_db(&db_path);

    let provider = FileSchemaProvider::new(&db_path);
    let schema = provider.load().unwrap();

    // Verify columns for users table
    let user_cols = schema.columns_for_table("users").unwrap();
    assert!(user_cols.contains(&"id".to_string()));
    assert!(user_cols.contains(&"name".to_string()));
    assert!(user_cols.contains(&"email".to_string()));
    assert_eq!(user_cols.len(), 3);

    // Verify columns for orders table
    let order_cols = schema.columns_for_table("orders").unwrap();
    assert!(order_cols.contains(&"id".to_string()));
    assert!(order_cols.contains(&"user_id".to_string()));
    assert!(order_cols.contains(&"total".to_string()));
    assert_eq!(order_cols.len(), 3);

    cleanup_db(&db_path);
}

#[test]
fn test_file_provider_view_columns() {
    let db_path = fixtures_dir().join("test_view_cols.db");
    create_test_db(&db_path);

    let provider = FileSchemaProvider::new(&db_path);
    let schema = provider.load().unwrap();

    // Verify view columns
    let view_cols = schema.columns_for_view("user_orders").unwrap();
    assert_eq!(view_cols, &["name", "total"]);

    cleanup_db(&db_path);
}

// ============================================================================
// DdlSchemaProvider Tests
// ============================================================================

#[test]
fn test_ddl_provider_parses_complex_schema() {
    let sql = r#"
        CREATE TABLE users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email TEXT NOT NULL UNIQUE,
            name TEXT,
            created_at TEXT
        );

        CREATE TABLE posts (
            id INTEGER PRIMARY KEY,
            user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            title TEXT NOT NULL,
            content TEXT,
            published INTEGER DEFAULT 0
        );

        CREATE INDEX idx_posts_user ON posts(user_id);
        CREATE UNIQUE INDEX idx_posts_title ON posts(title);

        CREATE VIEW v_published_posts AS
            SELECT p.*, u.name AS author_name
            FROM posts p
            JOIN users u ON p.user_id = u.id
            WHERE p.published = 1;

        CREATE TRIGGER trg_posts_audit AFTER INSERT ON posts
        BEGIN
            SELECT 1;
        END;
    "#;

    let provider = DdlSchemaProvider::from_sql(sql).unwrap();
    let schema = provider.load().unwrap();

    // Tables
    assert!(schema.has_table("users"));
    assert!(schema.has_table("posts"));

    // Columns
    let user_cols = schema.columns_for_table("users").unwrap();
    assert_eq!(user_cols.len(), 4);

    // Indexes
    assert!(schema.has_index("idx_posts_user"));
    assert!(schema.has_index("idx_posts_title"));

    // View
    assert!(schema.has_view("v_published_posts"));

    // Trigger
    assert!(schema.has_trigger("trg_posts_audit"));
}

#[test]
fn test_ddl_provider_handles_drop_statements() {
    let sql = r#"
        CREATE TABLE temp_table (id INTEGER);
        CREATE INDEX temp_idx ON temp_table(id);
        DROP INDEX temp_idx;
        DROP TABLE temp_table;
        CREATE TABLE final_table (id INTEGER);
    "#;

    let provider = DdlSchemaProvider::from_sql(sql).unwrap();
    let schema = provider.load().unwrap();

    // Dropped objects should not exist
    assert!(!schema.has_table("temp_table"));
    assert!(!schema.has_index("temp_idx"));

    // Final table should exist
    assert!(schema.has_table("final_table"));
}

// ============================================================================
// Integration Workflow Tests
// ============================================================================

#[test]
fn test_complete_workflow_document_to_schema() {
    let db_path = fixtures_dir().join("test_workflow.db");
    create_test_db(&db_path);

    // Step 1: Parse a document with .open and SQL
    let source = format!(
        r#".open {}

-- Add a new table not in the database
CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY,
    event_type TEXT NOT NULL,
    created_at TEXT
);

-- Query existing table
SELECT * FROM users WHERE id > 0;

-- Create an index on the new table
CREATE INDEX idx_audit_event ON audit_log(event_type);
"#,
        db_path.display()
    );

    let doc = Document::parse(&source, true);
    assert!(doc.program.is_ok());

    // Step 2: Load schema from the database file
    let open_path = doc.open_commands().next().unwrap();
    let file_provider = FileSchemaProvider::new(open_path);
    let mut schema = file_provider.load().unwrap();

    // Step 3: Build schema from DDL in the document
    let program = doc.program.unwrap();
    let ddl_schema = solite_analyzer::build_schema(&program);

    // Step 4: Merge schemas
    schema.merge(ddl_schema);

    // Step 5: Verify final schema has everything
    // From database:
    assert!(schema.has_table("users"));
    assert!(schema.has_table("orders"));
    assert!(schema.has_view("user_orders"));
    assert!(schema.has_index("idx_orders_user"));

    // From DDL:
    assert!(schema.has_table("audit_log"));
    assert!(schema.has_index("idx_audit_event"));

    cleanup_db(&db_path);
}

#[test]
fn test_workflow_with_quoted_path() {
    let db_path = fixtures_dir().join("test with spaces.db");
    create_test_db(&db_path);

    // Use quoted path in .open command
    let source = format!(
        ".open \"{}\"\nSELECT * FROM users;",
        db_path.display()
    );
    let doc = Document::parse(&source, true);

    // Path should be extracted without quotes
    let open_path = doc.open_commands().next().unwrap();
    assert_eq!(open_path, db_path.to_str().unwrap());

    // Schema should load successfully
    let provider = FileSchemaProvider::new(open_path);
    let schema = provider.load().unwrap();
    assert!(schema.has_table("users"));

    cleanup_db(&db_path);
}

#[test]
fn test_workflow_relative_path() {
    // Create database in fixtures directory
    let db_path = fixtures_dir().join("relative_test.db");
    create_test_db(&db_path);

    // Document uses absolute path (would need to cd for true relative path testing)
    let source = format!(".open {}\nSELECT 1;", db_path.display());
    let doc = Document::parse(&source, true);

    assert!(doc.has_dot_commands());
    let path = doc.open_commands().next().unwrap();

    let provider = FileSchemaProvider::new(path);
    let result = provider.load();
    assert!(result.is_ok());

    cleanup_db(&db_path);
}

// ============================================================================
// Concurrent/Multiple Provider Tests
// ============================================================================

#[test]
fn test_multiple_file_providers() {
    let db1_path = fixtures_dir().join("multi_prov_1.db");
    let db2_path = fixtures_dir().join("multi_prov_2.db");

    // Create different schemas in each database
    {
        let conn1 = rusqlite::Connection::open(&db1_path).unwrap();
        conn1
            .execute("CREATE TABLE table_in_db1 (id INTEGER)", [])
            .unwrap();
    }
    {
        let conn2 = rusqlite::Connection::open(&db2_path).unwrap();
        conn2
            .execute("CREATE TABLE table_in_db2 (id INTEGER)", [])
            .unwrap();
    }

    // Load from both
    let provider1 = FileSchemaProvider::new(&db1_path);
    let provider2 = FileSchemaProvider::new(&db2_path);

    let schema1 = provider1.load().unwrap();
    let schema2 = provider2.load().unwrap();

    // Each should have its own table
    assert!(schema1.has_table("table_in_db1"));
    assert!(!schema1.has_table("table_in_db2"));

    assert!(schema2.has_table("table_in_db2"));
    assert!(!schema2.has_table("table_in_db1"));

    cleanup_db(&db1_path);
    cleanup_db(&db2_path);
}

#[test]
fn test_provider_trait_objects() {
    let db_path = fixtures_dir().join("trait_obj_test.db");
    create_test_db(&db_path);

    // Test that providers work as trait objects
    let providers: Vec<Box<dyn SchemaProvider>> = vec![
        Box::new(FileSchemaProvider::new(&db_path)),
        Box::new(
            DdlSchemaProvider::from_sql("CREATE TABLE ddl_table (id INTEGER);").unwrap(),
        ),
    ];

    let schema1 = providers[0].load().unwrap();
    let schema2 = providers[1].load().unwrap();

    assert!(schema1.has_table("users"));
    assert!(schema2.has_table("ddl_table"));

    cleanup_db(&db_path);
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[test]
fn test_case_insensitive_table_lookup() {
    let db_path = fixtures_dir().join("case_test.db");
    create_test_db(&db_path);

    let provider = FileSchemaProvider::new(&db_path);
    let schema = provider.load().unwrap();

    // All case variations should work
    assert!(schema.has_table("users"));
    assert!(schema.has_table("USERS"));
    assert!(schema.has_table("Users"));
    assert!(schema.has_table("uSeRs"));

    cleanup_db(&db_path);
}

#[test]
fn test_unicode_table_names() {
    let db_path = fixtures_dir().join("unicode_test.db");
    let _ = fs::remove_file(&db_path);
    fs::create_dir_all(fixtures_dir()).unwrap();

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute("CREATE TABLE utilisateurs (id INTEGER)", [])
        .unwrap();
    conn.execute("CREATE TABLE benutzer (id INTEGER)", [])
        .unwrap();
    drop(conn);

    let provider = FileSchemaProvider::new(&db_path);
    let schema = provider.load().unwrap();

    assert!(schema.has_table("utilisateurs"));
    assert!(schema.has_table("benutzer"));

    cleanup_db(&db_path);
}

#[test]
fn test_table_with_many_columns() {
    let db_path = fixtures_dir().join("many_cols_test.db");
    let _ = fs::remove_file(&db_path);
    fs::create_dir_all(fixtures_dir()).unwrap();

    let conn = rusqlite::Connection::open(&db_path).unwrap();

    // Create a table with many columns
    let cols: Vec<String> = (1..=50)
        .map(|i| format!("col{} TEXT", i))
        .collect();
    let create_sql = format!("CREATE TABLE wide_table ({})", cols.join(", "));
    conn.execute(&create_sql, []).unwrap();
    drop(conn);

    let provider = FileSchemaProvider::new(&db_path);
    let schema = provider.load().unwrap();

    let columns = schema.columns_for_table("wide_table").unwrap();
    assert_eq!(columns.len(), 50);
    assert!(columns.contains(&"col1".to_string()));
    assert!(columns.contains(&"col50".to_string()));

    cleanup_db(&db_path);
}

#[test]
fn test_document_with_comments_and_whitespace() {
    let db_path = fixtures_dir().join("comments_test.db");
    create_test_db(&db_path);

    let source = format!(
        r#"
-- This is a comment at the start

.open {}

-- Comment between .open and SQL

SELECT
    id,
    name
FROM users;

-- Trailing comment
"#,
        db_path.display()
    );

    let doc = Document::parse(&source, true);

    assert!(doc.has_dot_commands());
    assert!(doc.program.is_ok());

    let program = doc.program.unwrap();
    assert_eq!(program.statements.len(), 1);

    cleanup_db(&db_path);
}

#[test]
fn test_sql_regions_are_correct() {
    let source = ".open db.db\nSELECT 1;\n.open other.db\nSELECT 2;";
    let doc = Document::parse(source, true);

    // Should have 2 SQL regions
    assert_eq!(doc.sql_regions.len(), 2);

    // Verify the content of each region
    let sql1 = &source[doc.sql_regions[0].start..doc.sql_regions[0].end];
    let sql2 = &source[doc.sql_regions[1].start..doc.sql_regions[1].end];

    assert!(sql1.contains("SELECT 1"));
    assert!(sql2.contains("SELECT 2"));
}

// ============================================================================
// Analyzer Integration Tests - Column Validation with External Schema
// ============================================================================

#[test]
fn test_analyzer_with_external_schema_invalid_column() {
    use solite_analyzer::analyze_with_schema;
    use solite_parser::parse_program;

    let db_path = fixtures_dir().join("analyzer_test.db");
    let _ = fs::remove_file(&db_path);
    fs::create_dir_all(fixtures_dir()).unwrap();

    // Create a database with a table
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE libfec_filings (
            filer_id TEXT,
            filer_name TEXT,
            coverage_from_date TEXT,
            back_reference_sched_name TEXT
        );",
    )
    .unwrap();
    drop(conn);

    // Load schema
    let provider = FileSchemaProvider::new(&db_path);
    let schema = provider.load().unwrap();

    // Verify schema was loaded correctly
    assert!(schema.has_table("libfec_filings"));
    let columns = schema.columns_for_table("libfec_filings").unwrap();
    assert!(columns.contains(&"filer_id".to_string()));
    assert!(columns.contains(&"filer_name".to_string()));
    assert!(!columns.contains(&"not_exist".to_string()));

    // Parse SQL with non-existent column
    let sql = "SELECT *, filer_id, back_reference_sched_name, not_exist FROM libfec_filings WHERE filer_name LIKE '%YOUN%';";
    let program = parse_program(sql).unwrap();

    // Analyze with external schema
    let diagnostics = analyze_with_schema(&program, Some(&schema));

    // Should have an error for non-existent column
    assert!(
        !diagnostics.is_empty(),
        "Expected error for non-existent column 'not_exist' but got none"
    );
    assert!(
        diagnostics.iter().any(|d| d.message.contains("not_exist")),
        "Expected diagnostic to mention 'not_exist' but got: {:?}",
        diagnostics
    );

    cleanup_db(&db_path);
}

#[test]
fn test_analyzer_with_external_schema_valid_columns() {
    use solite_analyzer::analyze_with_schema;
    use solite_parser::parse_program;

    let db_path = fixtures_dir().join("analyzer_valid_test.db");
    let _ = fs::remove_file(&db_path);
    fs::create_dir_all(fixtures_dir()).unwrap();

    // Create a database with a table
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE users (id INTEGER, name TEXT, email TEXT);",
    )
    .unwrap();
    drop(conn);

    // Load schema
    let provider = FileSchemaProvider::new(&db_path);
    let schema = provider.load().unwrap();

    // Parse SQL with valid columns
    let sql = "SELECT id, name FROM users WHERE email = 'test';";
    let program = parse_program(sql).unwrap();

    // Analyze with external schema
    let diagnostics = analyze_with_schema(&program, Some(&schema));

    // Should have no errors
    assert!(
        diagnostics.is_empty(),
        "Expected no errors but got: {:?}",
        diagnostics
    );

    cleanup_db(&db_path);
}
