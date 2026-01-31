//! Statement status tracking from bytecode analysis.

use solite_core::sqlite::{bytecode_steps, sqlite3_stmt};

/// Status of an in-progress SQL statement.
#[derive(Debug)]
pub enum StatementStatus {
    /// INSERT operation in progress.
    Insert {
        num_inserts: i64,
        name: Option<String>,
    },
    /// DELETE operation in progress.
    Delete { num_deletes: i64 },
    /// UPDATE operation in progress.
    Update { num_updates: i64 },
    /// Unknown operation type.
    Unknown,
}

impl StatementStatus {
    /// Format as a progress message.
    pub fn progress_message(&self) -> String {
        match self {
            StatementStatus::Delete { num_deletes } => {
                format!("delete: {}", num_deletes)
            }
            StatementStatus::Insert { num_inserts, name } => {
                let count = indicatif::HumanCount(*num_inserts as u64);
                match name {
                    Some(name) => format!("inserting {} rows into {}", count, name),
                    None => format!("inserting {} rows", count),
                }
            }
            StatementStatus::Update { num_updates } => {
                format!("update: {}", num_updates)
            }
            StatementStatus::Unknown => "unknown".to_string(),
        }
    }

    /// Format as a completion message.
    pub fn completion_message(&self) -> String {
        match self {
            StatementStatus::Insert { num_inserts, name } => {
                let count = indicatif::HumanCount(*num_inserts as u64);
                let table = name.as_deref().unwrap_or("???");
                format!("inserted {} rows into {} ", count, table)
            }
            StatementStatus::Delete { num_deletes } => {
                let count = indicatif::HumanCount(*num_deletes as u64);
                format!("deleted {} rows ", count)
            }
            StatementStatus::Update { num_updates } => {
                let count = indicatif::HumanCount(*num_updates as u64);
                format!("updated {} rows ", count)
            }
            StatementStatus::Unknown => String::new(),
        }
    }
}

/// Extract the status of a statement from its bytecode.
pub fn get_statement_status(stmt: *mut sqlite3_stmt) -> StatementStatus {
    let steps = bytecode_steps(stmt);

    // Check for DELETE operations
    let deletes: Vec<_> = steps
        .iter()
        .filter(|step| step.opcode == "Delete")
        .collect();

    if !deletes.is_empty() {
        // Take the first delete operation
        let num_deletes = deletes[0].nexec;
        return StatementStatus::Delete { num_deletes };
    }

    // Check for INSERT operations
    let inserts: Vec<_> = steps
        .iter()
        .filter(|step| step.opcode == "Insert")
        .collect();

    if !inserts.is_empty() {
        // Find the insert with the most executions
        let insert = match inserts.iter().max_by_key(|step| step.nexec) {
            Some(i) => i,
            None => return StatementStatus::Unknown,
        };

        // Check if this is actually an UPDATE (OPFLAG_ISUPDATE = 0x04)
        if insert.p5 & 0x04 > 0 {
            return StatementStatus::Update {
                num_updates: insert.nexec,
            };
        }

        // Determine the table name
        let name = if insert.p4.is_empty() {
            // Try to extract from CREATE TABLE statement
            steps
                .iter()
                .find(|step| {
                    (step.opcode == "String" || step.opcode == "String8")
                        && step.p4.starts_with("CREATE TABLE ")
                })
                .and_then(|step| {
                    // Extract table name from "CREATE TABLE foo (...)"
                    step.p4
                        .split_whitespace()
                        .nth(2)
                        .map(|s| s.split('(').next().unwrap_or(s).trim().to_owned())
                })
        } else {
            Some(insert.p4.to_owned())
        };

        return StatementStatus::Insert {
            num_inserts: insert.nexec,
            name,
        };
    }

    StatementStatus::Unknown
}
