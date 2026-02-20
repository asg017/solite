//! Run command for executing SQL files inline.
//!
//! The `.run` command executes a SQL file within the current runtime,
//! yielding all steps (SQL output, dot commands, tracebacks) through
//! the normal execution loop.
//!
//! # Usage
//!
//! ```sql
//! .run file.sql
//! .run file.sql procedureName
//! .run file.sql --name=alex --age 20
//! .run file.sql procedureName --name alex --age=20
//! ```

use serde::Serialize;
use std::collections::HashMap;

/// Command to run a SQL file inline.
#[derive(Serialize, Debug)]
pub struct RunCommand {
    /// Path to the SQL file to execute.
    pub file: String,
    /// Optional procedure name to invoke after loading.
    pub procedure: Option<String>,
    /// Key-value parameters to set for the duration of the run.
    pub parameters: HashMap<String, String>,
}
