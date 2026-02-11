//! Call command for invoking registered procedures.
//!
//! The `.call` command executes a previously registered procedure by name,
//! optionally loading procedure definitions from a file first.
//!
//! # Usage
//!
//! ```sql
//! -- Call a procedure already registered in the current session:
//! .call getUserById
//!
//! -- Load procedures from a file, then call one:
//! .call procedures.sql getUserById
//! ```

use serde::Serialize;

/// Command to call a registered procedure.
#[derive(Serialize, Debug)]
pub struct CallCommand {
    /// Optional path to a SQL file to load procedures from.
    pub file: Option<String>,
    /// The name of the procedure to invoke.
    pub procedure_name: String,
}
