//! SQLite database introspection via rusqlite.
//!
//! This module provides schema introspection for live SQLite databases
//! using the rusqlite library. Only available on non-WASM targets.

use rusqlite::{Connection, OpenFlags};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use thiserror::Error;

/// Error type for introspection operations.
#[derive(Error, Debug)]
pub enum IntrospectError {
    /// Failed to open the database file.
    #[error("Failed to open database: {0}")]
    OpenError(String),

    /// A database query failed.
    #[error("Database query failed: {0}")]
    QueryError(String),

    /// The specified file was not found.
    #[error("File not found: {0}")]
    FileNotFound(String),
}

impl From<rusqlite::Error> for IntrospectError {
    fn from(err: rusqlite::Error) -> Self {
        IntrospectError::QueryError(err.to_string())
    }
}

/// Quote a string as a SQL identifier: wrap in double quotes and double any
/// embedded double quotes (`we"ird` -> `"we""ird"`). This is the same escaping
/// SQLite's printf `%w` conversion performs; we do it inline because this
/// crate links rusqlite directly and does not depend on solite-core. Without
/// it, a table/index/view/module name containing a `"` (legal in SQLite, and
/// attacker-controllable when introspecting an untrusted database file) breaks
/// out of the quoted context and corrupts the generated SQL.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Table information extracted from introspection.
#[derive(Debug, Clone, Default)]
pub struct TableInfo {
    /// Original table name (preserves case).
    pub name: String,
    /// Column names (lowercase for case-insensitive lookup).
    pub columns: HashSet<String>,
    /// Original column names (preserves case for display).
    pub original_columns: Vec<String>,
    /// Whether this table was created with WITHOUT ROWID option.
    pub without_rowid: bool,
    /// The original CREATE TABLE SQL statement.
    pub sql: Option<String>,
}

/// Index information extracted from introspection.
#[derive(Debug, Clone)]
pub struct IndexInfo {
    /// Original index name (preserves case).
    pub name: String,
    /// Table this index is on.
    pub table_name: String,
    /// Columns included in the index (original case).
    pub columns: Vec<String>,
    /// Whether this is a UNIQUE index.
    pub is_unique: bool,
    /// Whether this is a partial index (has a WHERE clause).
    pub is_partial: bool,
    /// The original CREATE INDEX SQL statement.
    pub sql: Option<String>,
}

/// View information extracted from introspection.
#[derive(Debug, Clone)]
pub struct ViewInfo {
    /// Original view name (preserves case).
    pub name: String,
    /// Columns in the view (if determinable).
    pub columns: Vec<String>,
    /// The original CREATE VIEW SQL statement.
    pub sql: Option<String>,
}

/// The event a trigger fires on, parsed from the CREATE TRIGGER header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerEvent {
    /// Fires on INSERT.
    Insert,
    /// Fires on UPDATE (including UPDATE OF columns).
    Update,
    /// Fires on DELETE.
    Delete,
}

/// Trigger information extracted from introspection.
#[derive(Debug, Clone)]
pub struct TriggerInfo {
    /// Original trigger name (preserves case).
    pub name: String,
    /// Table this trigger is on.
    pub table_name: String,
    /// The event the trigger fires on, parsed from the CREATE TRIGGER
    /// header (None when the SQL is unavailable or unparseable).
    pub event: Option<TriggerEvent>,
    /// The original CREATE TRIGGER SQL statement.
    pub sql: Option<String>,
}

/// Parse the firing event out of a CREATE TRIGGER statement header.
///
/// Only the header (everything before the `ON <table>` clause) is
/// considered, so an `AFTER INSERT` trigger whose *body* contains a
/// DELETE statement (the common audit-log pattern) is not misclassified.
/// Quoted identifiers and string literals are skipped so a trigger or
/// table named e.g. `"delete"` cannot be mistaken for the event keyword.
fn parse_trigger_event(sql: &str) -> Option<TriggerEvent> {
    // Strip quoted regions ('...', "...", `...`, [...]), replacing them
    // with a space so surrounding tokens stay separated.
    let mut stripped = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' | '`' => {
                while let Some(c2) = chars.next() {
                    if c2 == c {
                        // doubled quote chars escape themselves
                        if chars.peek() == Some(&c) {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
                stripped.push(' ');
            }
            '[' => {
                for c2 in chars.by_ref() {
                    if c2 == ']' {
                        break;
                    }
                }
                stripped.push(' ');
            }
            _ => stripped.push(c.to_ascii_uppercase()),
        }
    }

    // In a CREATE TRIGGER header the event keyword always precedes the
    // first unquoted ON: CREATE TRIGGER name [BEFORE|AFTER|INSTEAD OF]
    // [INSERT|UPDATE [OF cols]|DELETE] ON table ...
    for token in stripped.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_') {
        match token {
            "INSERT" => return Some(TriggerEvent::Insert),
            "UPDATE" => return Some(TriggerEvent::Update),
            "DELETE" => return Some(TriggerEvent::Delete),
            "ON" => break,
            _ => {}
        }
    }
    None
}

/// Schema information extracted from a SQLite database.
///
/// This struct contains all tables, indexes, views, and triggers
/// found in the database.
#[derive(Debug, Clone, Default)]
pub struct IntrospectedSchema {
    /// Table registry: lowercase table name -> TableInfo
    pub tables: HashMap<String, TableInfo>,
    /// Index registry: lowercase index name -> IndexInfo
    pub indexes: HashMap<String, IndexInfo>,
    /// View registry: lowercase view name -> ViewInfo
    pub views: HashMap<String, ViewInfo>,
    /// Trigger registry: lowercase trigger name -> TriggerInfo
    pub triggers: HashMap<String, TriggerInfo>,
}

impl IntrospectedSchema {
    /// Create an empty schema.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a table exists (case-insensitive).
    pub fn has_table(&self, name: &str) -> bool {
        self.tables.contains_key(&name.to_lowercase())
    }

    /// Get table info (case-insensitive lookup).
    pub fn get_table(&self, name: &str) -> Option<&TableInfo> {
        self.tables.get(&name.to_lowercase())
    }

    /// Get all table names (original case).
    pub fn table_names(&self) -> impl Iterator<Item = &str> {
        self.tables.values().map(|t| t.name.as_str())
    }

    /// Check if an index exists (case-insensitive).
    pub fn has_index(&self, name: &str) -> bool {
        self.indexes.contains_key(&name.to_lowercase())
    }

    /// Get index info (case-insensitive lookup).
    pub fn get_index(&self, name: &str) -> Option<&IndexInfo> {
        self.indexes.get(&name.to_lowercase())
    }

    /// Get all index names (original case).
    pub fn index_names(&self) -> impl Iterator<Item = &str> {
        self.indexes.values().map(|i| i.name.as_str())
    }

    /// Check if a view exists (case-insensitive).
    pub fn has_view(&self, name: &str) -> bool {
        self.views.contains_key(&name.to_lowercase())
    }

    /// Get view info (case-insensitive lookup).
    pub fn get_view(&self, name: &str) -> Option<&ViewInfo> {
        self.views.get(&name.to_lowercase())
    }

    /// Get all view names (original case).
    pub fn view_names(&self) -> impl Iterator<Item = &str> {
        self.views.values().map(|v| v.name.as_str())
    }

    /// Check if a trigger exists (case-insensitive).
    pub fn has_trigger(&self, name: &str) -> bool {
        self.triggers.contains_key(&name.to_lowercase())
    }

    /// Get trigger info (case-insensitive lookup).
    pub fn get_trigger(&self, name: &str) -> Option<&TriggerInfo> {
        self.triggers.get(&name.to_lowercase())
    }

    /// Get all trigger names (original case).
    pub fn trigger_names(&self) -> impl Iterator<Item = &str> {
        self.triggers.values().map(|t| t.name.as_str())
    }
}

/// Introspect a SQLite database file and extract its schema.
///
/// Queries sqlite_master for tables, views, indexes, and triggers.
/// Uses PRAGMA table_info to get column information for tables.
/// Uses PRAGMA index_info to get column information for indexes.
///
/// # Arguments
///
/// * `path` - Path to the SQLite database file
///
/// # Returns
///
/// An `IntrospectedSchema` containing all database objects.
///
/// # Errors
///
/// Returns an error if the file doesn't exist, can't be opened,
/// or if any queries fail.
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// use solite_schema::introspect::introspect_sqlite_db;
///
/// let schema = introspect_sqlite_db(Path::new("my_database.db")).unwrap();
/// for table_name in schema.table_names() {
///     println!("Table: {}", table_name);
/// }
/// ```
pub fn introspect_sqlite_db(path: &Path) -> Result<IntrospectedSchema, IntrospectError> {
    // Check if file exists
    if !path.exists() {
        return Err(IntrospectError::FileNotFound(
            path.display().to_string(),
        ));
    }

    // Open in read-only mode
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    introspect_connection(&conn)
}

/// Introspect a SQLite database from an existing connection.
///
/// This is useful for testing with in-memory databases or when you
/// already have a connection open.
///
/// # Arguments
///
/// * `conn` - An open rusqlite Connection
///
/// # Returns
///
/// An `IntrospectedSchema` containing all database objects.
pub fn introspect_connection(conn: &Connection) -> Result<IntrospectedSchema, IntrospectError> {
    let mut schema = IntrospectedSchema::new();

    // Query sqlite_master for all objects
    let mut stmt = conn.prepare(
        "SELECT type, name, tbl_name, sql FROM sqlite_master
         WHERE type IN ('table', 'index', 'view', 'trigger')
         AND name NOT LIKE 'sqlite_%'
         ORDER BY type, name",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?, // type
            row.get::<_, String>(1)?, // name
            row.get::<_, Option<String>>(2)?, // tbl_name
            row.get::<_, Option<String>>(3)?, // sql
        ))
    })?;

    // Collect rows first to avoid borrowing issues
    let objects: Vec<_> = rows.collect::<Result<Vec<_>, _>>()?;

    for (obj_type, name, tbl_name, sql) in objects {
        match obj_type.as_str() {
            "table" => {
                let table_info = introspect_table(conn, &name, sql.as_deref())?;
                schema.tables.insert(name.to_lowercase(), table_info);
            }
            "index" => {
                if let Some(ref table_name) = tbl_name {
                    let index_info = introspect_index(conn, &name, table_name, sql.as_deref())?;
                    schema.indexes.insert(name.to_lowercase(), index_info);
                }
            }
            "view" => {
                let view_info = introspect_view(conn, &name, sql.as_deref())?;
                schema.views.insert(name.to_lowercase(), view_info);
            }
            "trigger" => {
                if let Some(table_name) = tbl_name {
                    let trigger_info = TriggerInfo {
                        name: name.clone(),
                        table_name,
                        event: sql.as_deref().and_then(parse_trigger_event),
                        sql,
                    };
                    schema.triggers.insert(name.to_lowercase(), trigger_info);
                }
            }
            _ => {}
        }
    }

    Ok(schema)
}

/// Introspect a single table to get its column information.
fn introspect_table(
    conn: &Connection,
    table_name: &str,
    sql: Option<&str>,
) -> Result<TableInfo, IntrospectError> {
    let mut columns = HashSet::new();
    let mut original_columns = Vec::new();

    // Use PRAGMA table_info to get column details
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", quote_ident(table_name)))?;
    let rows = stmt.query_map([], |row| {
        row.get::<_, String>(1) // column name is at index 1
    })?;

    for col_result in rows {
        let col_name = col_result?;
        let col_lower = col_name.to_lowercase();
        if !columns.contains(&col_lower) {
            columns.insert(col_lower);
            original_columns.push(col_name);
        }
    }

    // WITHOUT ROWID is reported authoritatively by pragma_table_list.wr;
    // substring-matching the CREATE SQL misfires on e.g. a column whose
    // DEFAULT string or a comment contains "without rowid".
    let without_rowid: bool = conn
        .query_row(
            "SELECT wr FROM pragma_table_list WHERE \"schema\" = 'main' AND name = ?1",
            [table_name],
            |row| row.get(0),
        )
        .unwrap_or(false);

    Ok(TableInfo {
        name: table_name.to_string(),
        columns,
        original_columns,
        without_rowid,
        sql: sql.map(String::from),
    })
}

/// Introspect a single index to get its column information.
fn introspect_index(
    conn: &Connection,
    index_name: &str,
    table_name: &str,
    sql: Option<&str>,
) -> Result<IndexInfo, IntrospectError> {
    let mut columns = Vec::new();

    // Use PRAGMA index_info to get indexed columns
    let mut stmt = conn.prepare(&format!("PRAGMA index_info({})", quote_ident(index_name)))?;
    let rows = stmt.query_map([], |row| {
        row.get::<_, Option<String>>(2) // column name is at index 2 (can be NULL for expressions)
    })?;

    for col_result in rows {
        if let Some(col_name) = col_result? {
            columns.push(col_name);
        }
    }

    // `unique` and `partial` come from pragma_index_list (authoritative).
    // Substring-matching the CREATE SQL misclassifies e.g. a non-unique
    // index named "uniqueness_check" or an indexed expression containing
    // the string ' WHERE '. Both the table name and index name are bound
    // parameters, so no identifier quoting is needed here.
    let (is_unique, is_partial) = conn
        .query_row(
            "SELECT \"unique\", partial FROM pragma_index_list(?1) WHERE name = ?2",
            [table_name, index_name],
            |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?)),
        )
        .unwrap_or((false, false));

    Ok(IndexInfo {
        name: index_name.to_string(),
        table_name: table_name.to_string(),
        columns,
        is_unique,
        is_partial,
        sql: sql.map(String::from),
    })
}

/// Introspect a single view to get its column information.
fn introspect_view(
    conn: &Connection,
    view_name: &str,
    sql: Option<&str>,
) -> Result<ViewInfo, IntrospectError> {
    let mut columns = Vec::new();

    // Use PRAGMA table_info on the view to get column names
    // Views respond to table_info just like tables
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", quote_ident(view_name)))?;
    let rows = stmt.query_map([], |row| {
        row.get::<_, String>(1) // column name is at index 1
    })?;

    for col_result in rows {
        columns.push(col_result?);
    }

    Ok(ViewInfo {
        name: view_name.to_string(),
        columns,
        sql: sql.map(String::from),
    })
}

/// Discover eponymous virtual table modules and their visible columns.
///
/// Queries `PRAGMA module_list` for all registered modules, then attempts to
/// prepare `SELECT * FROM <module>` for each one. Modules that support
/// eponymous access (like `generate_series`, `json_each`, pragma vtabs, etc.)
/// will succeed, and their column names are extracted from the prepared statement.
/// Non-eponymous modules are silently skipped.
///
/// The caller is responsible for initializing extensions on the connection
/// (e.g. via `solite_stdlib_init`) before calling this function.
///
/// Returns a vec of `(module_name, column_names)` pairs.
pub fn discover_virtual_table_columns(conn: &Connection) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();

    // Get all registered modules
    let Ok(mut stmt) = conn.prepare("SELECT name FROM pragma_module_list") else {
        return result;
    };
    let Ok(modules) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
        return result;
    };
    let module_names: Vec<String> = modules.filter_map(|r| r.ok()).collect();

    for module in &module_names {
        // Try to prepare a SELECT to discover visible columns.
        // This works for eponymous virtual tables without actually executing anything.
        let sql = format!("SELECT * FROM {}", quote_ident(module));
        if let Ok(probe) = conn.prepare(&sql) {
            let columns: Vec<String> = probe
                .column_names()
                .iter()
                .map(|s| s.to_string())
                .collect();
            if !columns.is_empty() {
                result.push((module.clone(), columns));
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_db() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn test_introspect_empty_database() {
        let conn = create_test_db();
        let schema = introspect_connection(&conn).unwrap();

        assert!(schema.tables.is_empty());
        assert!(schema.indexes.is_empty());
        assert!(schema.views.is_empty());
        assert!(schema.triggers.is_empty());
    }

    #[test]
    fn test_introspect_single_table() {
        let conn = create_test_db();
        conn.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT)",
            [],
        )
        .unwrap();

        let schema = introspect_connection(&conn).unwrap();

        assert!(schema.has_table("users"));
        assert!(schema.has_table("USERS")); // case insensitive
        assert!(!schema.has_table("nonexistent"));

        let table = schema.get_table("users").unwrap();
        assert_eq!(table.name, "users");
        assert!(table.columns.contains("id"));
        assert!(table.columns.contains("name"));
        assert!(table.columns.contains("email"));
        assert_eq!(table.original_columns.len(), 3);
        assert!(!table.without_rowid);
    }

    #[test]
    fn test_introspect_table_without_rowid() {
        let conn = create_test_db();
        conn.execute(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT) WITHOUT ROWID",
            [],
        )
        .unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let table = schema.get_table("settings").unwrap();

        assert!(table.without_rowid);
    }

    #[test]
    fn test_introspect_rowid_table_with_without_rowid_text() {
        // A DEFAULT string containing "without rowid" must not be
        // misclassified (wr comes from pragma_table_list, not the SQL).
        let conn = create_test_db();
        conn.execute(
            "CREATE TABLE notes (id INTEGER PRIMARY KEY, kind TEXT DEFAULT 'without rowid')",
            [],
        )
        .unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let table = schema.get_table("notes").unwrap();

        assert!(!table.without_rowid);
    }

    #[test]
    fn test_introspect_multiple_tables() {
        let conn = create_test_db();
        conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)", [])
            .unwrap();
        conn.execute(
            "CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER, total REAL)",
            [],
        )
        .unwrap();

        let schema = introspect_connection(&conn).unwrap();

        let mut names: Vec<_> = schema.table_names().collect();
        names.sort();
        assert_eq!(names, vec!["orders", "users"]);
    }

    #[test]
    fn test_introspect_simple_index() {
        let conn = create_test_db();
        conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT)", [])
            .unwrap();
        conn.execute("CREATE INDEX idx_email ON users(email)", [])
            .unwrap();

        let schema = introspect_connection(&conn).unwrap();

        assert!(schema.has_index("idx_email"));
        let idx = schema.get_index("idx_email").unwrap();
        assert_eq!(idx.name, "idx_email");
        assert_eq!(idx.table_name, "users");
        assert_eq!(idx.columns, vec!["email"]);
        assert!(!idx.is_unique);
        assert!(!idx.is_partial);
    }

    #[test]
    fn test_introspect_unique_index() {
        let conn = create_test_db();
        conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT)", [])
            .unwrap();
        conn.execute("CREATE UNIQUE INDEX idx_unique_email ON users(email)", [])
            .unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let idx = schema.get_index("idx_unique_email").unwrap();

        assert!(idx.is_unique);
    }

    #[test]
    fn test_introspect_multi_column_index() {
        let conn = create_test_db();
        conn.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, first_name TEXT, last_name TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE INDEX idx_name ON users(last_name, first_name)",
            [],
        )
        .unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let idx = schema.get_index("idx_name").unwrap();

        assert_eq!(idx.columns, vec!["last_name", "first_name"]);
    }

    #[test]
    fn test_introspect_partial_index() {
        let conn = create_test_db();
        conn.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT, active INTEGER)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE INDEX idx_active_users ON users(email) WHERE active = 1",
            [],
        )
        .unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let idx = schema.get_index("idx_active_users").unwrap();

        assert!(idx.is_partial);
    }

    #[test]
    fn test_introspect_non_unique_index_named_unique() {
        // A non-unique index whose name contains "unique" must not be
        // misclassified (regression for the SQL substring heuristic).
        let conn = create_test_db();
        conn.execute("CREATE TABLE t (a INTEGER)", []).unwrap();
        conn.execute("CREATE INDEX uniqueness_check ON t(a)", [])
            .unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let idx = schema.get_index("uniqueness_check").unwrap();

        assert!(!idx.is_unique);
        assert!(!idx.is_partial);
    }

    #[test]
    fn test_introspect_non_partial_index_with_where_in_expression() {
        // An indexed expression containing the string ' WHERE ' must not
        // be misclassified as a partial index.
        let conn = create_test_db();
        conn.execute("CREATE TABLE t (x TEXT DEFAULT ' WHERE ')", [])
            .unwrap();
        conn.execute("CREATE INDEX i ON t(iif(x=' WHERE ',1,2))", [])
            .unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let idx = schema.get_index("i").unwrap();

        assert!(!idx.is_partial);
        assert!(!idx.is_unique);
    }

    #[test]
    fn test_introspect_view() {
        let conn = create_test_db();
        conn.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, email TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE VIEW v_users AS SELECT id, name FROM users",
            [],
        )
        .unwrap();

        let schema = introspect_connection(&conn).unwrap();

        assert!(schema.has_view("v_users"));
        let view = schema.get_view("v_users").unwrap();
        assert_eq!(view.name, "v_users");
        assert_eq!(view.columns, vec!["id", "name"]);
    }

    #[test]
    fn test_introspect_identifiers_with_embedded_quotes() {
        // Identifiers containing double quotes are legal in SQLite and can
        // appear in an untrusted database file. Introspection must escape them
        // (PRAGMA table_info/index_info interpolate the name) rather than emit
        // broken SQL and silently drop the object.
        let conn = create_test_db();
        conn.execute(r#"CREATE TABLE "we""ird" ("c""ol" INTEGER, plain TEXT)"#, [])
            .unwrap();
        conn.execute(r#"CREATE INDEX "id""x" ON "we""ird"("c""ol")"#, [])
            .unwrap();
        conn.execute(r#"CREATE VIEW "v""w" AS SELECT plain FROM "we""ird""#, [])
            .unwrap();

        let schema = introspect_connection(&conn).unwrap();

        // table + its columns discovered (exercises PRAGMA table_info escaping)
        let table = schema.get_table(r#"we"ird"#).unwrap();
        assert!(table.original_columns.contains(&r#"c"ol"#.to_string()));
        assert!(table.original_columns.contains(&"plain".to_string()));

        // index + its column discovered (exercises PRAGMA index_info escaping)
        let idx = schema.get_index(r#"id"x"#).unwrap();
        assert_eq!(idx.columns, vec![r#"c"ol"#.to_string()]);

        // view + its column discovered (exercises PRAGMA table_info on a view)
        let view = schema.get_view(r#"v"w"#).unwrap();
        assert_eq!(view.columns, vec!["plain".to_string()]);
    }

    #[test]
    fn test_introspect_view_with_aliases() {
        let conn = create_test_db();
        conn.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE VIEW v_renamed AS SELECT id AS user_id, name AS user_name FROM users",
            [],
        )
        .unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let view = schema.get_view("v_renamed").unwrap();

        assert_eq!(view.columns, vec!["user_id", "user_name"]);
    }

    #[test]
    fn test_introspect_trigger() {
        let conn = create_test_db();
        conn.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE audit_log (id INTEGER PRIMARY KEY, action TEXT, ts TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TRIGGER trg_user_insert AFTER INSERT ON users BEGIN
                INSERT INTO audit_log(action, ts) VALUES ('insert', datetime('now'));
            END",
            [],
        )
        .unwrap();

        let schema = introspect_connection(&conn).unwrap();

        assert!(schema.has_trigger("trg_user_insert"));
        let trigger = schema.get_trigger("trg_user_insert").unwrap();
        assert_eq!(trigger.name, "trg_user_insert");
        assert_eq!(trigger.table_name, "users");
        assert!(trigger.sql.is_some());
    }

    #[test]
    fn test_trigger_event_parsed_from_header_not_body() {
        let conn = create_test_db();
        conn.execute("CREATE TABLE t (x INTEGER)", []).unwrap();
        conn.execute("CREATE TABLE log (x INTEGER)", []).unwrap();
        // audit-log pattern: an INSERT trigger whose body runs a DELETE
        conn.execute(
            "CREATE TRIGGER trg_audit AFTER INSERT ON t BEGIN DELETE FROM log; END",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TRIGGER trg_upd BEFORE UPDATE OF x ON t BEGIN INSERT INTO log VALUES (1); END",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TRIGGER trg_del AFTER DELETE ON t BEGIN SELECT 1; END",
            [],
        )
        .unwrap();

        let schema = introspect_connection(&conn).unwrap();
        assert_eq!(
            schema.get_trigger("trg_audit").unwrap().event,
            Some(TriggerEvent::Insert)
        );
        assert_eq!(
            schema.get_trigger("trg_upd").unwrap().event,
            Some(TriggerEvent::Update)
        );
        assert_eq!(
            schema.get_trigger("trg_del").unwrap().event,
            Some(TriggerEvent::Delete)
        );
    }

    #[test]
    fn test_parse_trigger_event_skips_quoted_identifiers() {
        // a trigger named "delete" firing on INSERT
        assert_eq!(
            parse_trigger_event(
                r#"CREATE TRIGGER "delete" AFTER INSERT ON t BEGIN SELECT 1; END"#
            ),
            Some(TriggerEvent::Insert)
        );
        // INSTEAD OF on a view
        assert_eq!(
            parse_trigger_event(
                "CREATE TRIGGER trg INSTEAD OF UPDATE ON v BEGIN SELECT 1; END"
            ),
            Some(TriggerEvent::Update)
        );
        // unparseable header
        assert_eq!(parse_trigger_event("CREATE TRIGGER trg ON t"), None);
    }

    #[test]
    fn test_introspect_ignores_sqlite_internal_tables() {
        let conn = create_test_db();
        conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY)", [])
            .unwrap();

        let schema = introspect_connection(&conn).unwrap();

        // Should have users but not sqlite_sequence or other internal tables
        assert!(schema.has_table("users"));
        assert!(!schema.has_table("sqlite_sequence"));
        assert!(!schema.has_table("sqlite_master"));
    }

    #[test]
    fn test_introspect_preserves_column_order() {
        let conn = create_test_db();
        conn.execute(
            "CREATE TABLE ordered_cols (zebra TEXT, alpha TEXT, middle TEXT)",
            [],
        )
        .unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let table = schema.get_table("ordered_cols").unwrap();

        // Original columns should preserve declaration order
        assert_eq!(table.original_columns, vec!["zebra", "alpha", "middle"]);
    }

    #[test]
    fn test_introspect_case_insensitive_column_lookup() {
        let conn = create_test_db();
        conn.execute("CREATE TABLE test (MyColumn TEXT)", [])
            .unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let table = schema.get_table("test").unwrap();

        // Lowercase lookup should work
        assert!(table.columns.contains("mycolumn"));
        // Original case preserved
        assert_eq!(table.original_columns, vec!["MyColumn"]);
    }

    #[test]
    fn test_introspect_complex_schema() {
        let conn = create_test_db();

        // Create a realistic schema
        conn.execute_batch(
            r#"
            CREATE TABLE users (
                id INTEGER PRIMARY KEY,
                email TEXT NOT NULL UNIQUE,
                name TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE orders (
                id INTEGER PRIMARY KEY,
                user_id INTEGER NOT NULL REFERENCES users(id),
                total REAL NOT NULL,
                status TEXT DEFAULT 'pending'
            );

            CREATE INDEX idx_orders_user ON orders(user_id);
            CREATE INDEX idx_orders_status ON orders(status) WHERE status != 'completed';

            CREATE VIEW v_order_summary AS
                SELECT u.name, COUNT(o.id) as order_count, SUM(o.total) as total_spent
                FROM users u
                LEFT JOIN orders o ON u.id = o.user_id
                GROUP BY u.id;

            CREATE TRIGGER trg_order_created AFTER INSERT ON orders BEGIN
                SELECT 1;
            END;
            "#,
        )
        .unwrap();

        let schema = introspect_connection(&conn).unwrap();

        // Check tables
        assert!(schema.has_table("users"));
        assert!(schema.has_table("orders"));
        assert_eq!(schema.tables.len(), 2);

        // Check users table columns
        let users = schema.get_table("users").unwrap();
        assert_eq!(users.original_columns.len(), 4);
        assert!(users.columns.contains("email"));
        assert!(users.columns.contains("created_at"));

        // Check indexes (excluding auto-created ones)
        assert!(schema.has_index("idx_orders_user"));
        assert!(schema.has_index("idx_orders_status"));
        let status_idx = schema.get_index("idx_orders_status").unwrap();
        assert!(status_idx.is_partial);

        // Check view
        assert!(schema.has_view("v_order_summary"));
        let view = schema.get_view("v_order_summary").unwrap();
        assert_eq!(view.columns, vec!["name", "order_count", "total_spent"]);

        // Check trigger
        assert!(schema.has_trigger("trg_order_created"));
        let trigger = schema.get_trigger("trg_order_created").unwrap();
        assert_eq!(trigger.table_name, "orders");
    }

    #[test]
    fn test_introspect_file_not_found() {
        let result = introspect_sqlite_db(Path::new("/nonexistent/path/to/db.sqlite"));
        assert!(matches!(result, Err(IntrospectError::FileNotFound(_))));
    }

    #[test]
    fn test_table_names_iterator() {
        let conn = create_test_db();
        conn.execute("CREATE TABLE alpha (id INTEGER)", []).unwrap();
        conn.execute("CREATE TABLE beta (id INTEGER)", []).unwrap();
        conn.execute("CREATE TABLE gamma (id INTEGER)", []).unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let mut names: Vec<_> = schema.table_names().collect();
        names.sort();

        assert_eq!(names, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn test_index_names_iterator() {
        let conn = create_test_db();
        conn.execute("CREATE TABLE t (a TEXT, b TEXT)", []).unwrap();
        conn.execute("CREATE INDEX idx_a ON t(a)", []).unwrap();
        conn.execute("CREATE INDEX idx_b ON t(b)", []).unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let mut names: Vec<_> = schema.index_names().collect();
        names.sort();

        assert_eq!(names, vec!["idx_a", "idx_b"]);
    }

    #[test]
    fn test_view_names_iterator() {
        let conn = create_test_db();
        conn.execute("CREATE TABLE t (x INTEGER)", []).unwrap();
        conn.execute("CREATE VIEW v1 AS SELECT * FROM t", []).unwrap();
        conn.execute("CREATE VIEW v2 AS SELECT * FROM t", []).unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let mut names: Vec<_> = schema.view_names().collect();
        names.sort();

        assert_eq!(names, vec!["v1", "v2"]);
    }

    #[test]
    fn test_trigger_names_iterator() {
        let conn = create_test_db();
        conn.execute("CREATE TABLE t (x INTEGER)", []).unwrap();
        conn.execute(
            "CREATE TRIGGER t1 AFTER INSERT ON t BEGIN SELECT 1; END",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TRIGGER t2 AFTER DELETE ON t BEGIN SELECT 1; END",
            [],
        )
        .unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let mut names: Vec<_> = schema.trigger_names().collect();
        names.sort();

        assert_eq!(names, vec!["t1", "t2"]);
    }

    #[test]
    fn test_sql_is_captured() {
        let conn = create_test_db();
        conn.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)",
            [],
        )
        .unwrap();

        let schema = introspect_connection(&conn).unwrap();
        let table = schema.get_table("users").unwrap();

        assert!(table.sql.is_some());
        let sql = table.sql.as_ref().unwrap();
        assert!(sql.contains("CREATE TABLE"));
        assert!(sql.contains("users"));
    }

    #[test]
    fn test_discover_virtual_table_columns_finds_builtins() {
        let conn = create_test_db();
        let vtabs = discover_virtual_table_columns(&conn);

        // json_each is built into modern SQLite
        let json_each = vtabs.iter().find(|(name, _)| name == "json_each");
        assert!(json_each.is_some(), "Should discover json_each");
        let (_, cols) = json_each.unwrap();
        assert!(cols.contains(&"key".to_string()));
        assert!(cols.contains(&"value".to_string()));
        assert!(cols.contains(&"type".to_string()));
    }

    #[test]
    fn test_discover_virtual_table_columns_returns_vec() {
        let conn = create_test_db();
        let vtabs = discover_virtual_table_columns(&conn);

        // Should find at least some built-in modules
        assert!(!vtabs.is_empty(), "Should discover at least some virtual tables");
    }
}
