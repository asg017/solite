//! Snapshot state management and SQL registration.

use solite_core::sqlite::Statement;
use solite_core::{Runtime, Step};
use std::path::PathBuf;

/// Result of comparing a snapshot.
pub enum SnapshotResult {
    Matches,
    Accepted,
    Rejected,
    Removed,
}

// SQL constants for tracking snapped statements
pub const BASE_FUNCTIONS_CREATE: &str = r#"
  CREATE TABLE solite_snapshot.solite_snapshot_base_functions AS
    SELECT name
    FROM pragma_function_list
    ORDER BY 1
"#;

pub const BASE_MODULES_CREATE: &str = r#"
  CREATE TABLE solite_snapshot.solite_snapshot_base_modules AS
    SELECT name
    FROM pragma_module_list
    ORDER BY 1
"#;

pub const LOADED_FUNCTIONS_CREATE: &str = r#"
  CREATE TABLE solite_snapshot.solite_snapshot_loaded_functions AS
    SELECT name
    FROM pragma_function_list
    WHERE name NOT IN (SELECT name FROM solite_snapshot_base_functions)
    ORDER BY 1
"#;

pub const LOADED_MODULES_CREATE: &str = r#"
  CREATE TABLE solite_snapshot.solite_snapshot_loaded_modules AS
    SELECT name
    FROM pragma_module_list
    WHERE name NOT IN (SELECT name FROM solite_snapshot_base_modules)
    ORDER BY 1
"#;

pub const SNAPPED_STATEMENT_CREATE: &str = r#"
  CREATE TABLE solite_snapshot.solite_snapshot_snapped_statement(
    id integer primary key autoincrement,
    sql text,
    reference text,
    execution_start integer,
    execution_end integer
  )
"#;

const SNAPPED_STATEMENT_INSERT: &str = r#"
  INSERT INTO solite_snapshot.solite_snapshot_snapped_statement(sql, reference) VALUES
    (?, ?)
  RETURNING id;
"#;

pub const SNAPPED_STATEMENT_BYTECODE_STEPS_CREATE: &str = r#"
  CREATE TABLE solite_snapshot.solite_snapshot_snapped_statement_bytecode_steps(
    statement_id integer references solite_snapshot_snapped_statement(id),
    addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle
  )
"#;

const SNAPPED_STATEMENT_BYTECODE_STEPS_INSERT: &str = r#"
  INSERT INTO solite_snapshot.solite_snapshot_snapped_statement_bytecode_steps
    SELECT ?, addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle
    FROM bytecode(?)
"#;

/// State for a snapshot testing session.
pub struct SnapshotState {
    pub runtime: Runtime,
    pub snapshots_dir: PathBuf,
    pub generated_snapshots: Vec<String>,
    pub results: Vec<SnapshotResult>,
    pub is_review: bool,
    pub verbose: bool,
    pub loaded_extension: bool,
}

/// Register a statement in the snapshot tracking database.
/// Returns the statement ID, or None on error.
pub fn register_statement(rt: &mut Runtime, stmt: &Statement, step: &Step) -> Option<i64> {
    let insert = match rt.connection.prepare(SNAPPED_STATEMENT_INSERT) {
        Ok((_, Some(stmt))) => stmt,
        _ => {
            eprintln!("Warning: Failed to prepare statement registration");
            return None;
        }
    };

    insert.bind_text(1, stmt.sql());
    let reference_str = step.reference.to_string();
    insert.bind_text(2, &reference_str);

    match insert.nextx() {
        Ok(Some(row)) => Some(row.value_at(0).as_int64()),
        _ => {
            eprintln!("Warning: Failed to register statement");
            None
        }
    }
}

/// Register bytecode steps for a statement.
pub fn register_stmt_bytecode(rt: &mut Runtime, stmt: &Statement, statement_id: i64) {
    let stmt_bytecode = match rt.connection.prepare(SNAPPED_STATEMENT_BYTECODE_STEPS_INSERT) {
        Ok((_, Some(stmt))) => stmt,
        _ => {
            eprintln!("Warning: Failed to prepare bytecode registration");
            return;
        }
    };

    stmt_bytecode.bind_int64(1, statement_id);
    stmt_bytecode.bind_pointer(2, stmt.pointer().cast(), c"stmt-pointer");

    if let Err(e) = stmt_bytecode.execute() {
        eprintln!("Warning: Failed to register bytecode: {:?}", e);
    }
}
