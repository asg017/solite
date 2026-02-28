//! Statement status tracking from bytecode analysis.

use std::collections::HashMap;
use std::ffi::CStr;

use solite_core::sqlite::{bytecode_steps, sqlite3_sql, sqlite3_stmt, BytecodeStep};

/// A side effect from a trigger firing during the main statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerEffect {
    pub table: String,
    pub operation: TriggerOperation,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TriggerOperation {
    Insert,
    Update,
    Delete,
}

impl std::fmt::Display for TriggerOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TriggerOperation::Insert => write!(f, "inserted into"),
            TriggerOperation::Update => write!(f, "updated in"),
            TriggerOperation::Delete => write!(f, "deleted from"),
        }
    }
}

/// Status of an in-progress SQL statement.
#[derive(Debug)]
pub enum StatementStatus {
    /// INSERT operation in progress.
    Insert {
        num_inserts: i64,
        name: Option<String>,
        trigger_effects: Vec<TriggerEffect>,
    },
    /// DELETE operation in progress.
    Delete {
        num_deletes: i64,
        trigger_effects: Vec<TriggerEffect>,
    },
    /// UPDATE operation in progress.
    Update {
        num_updates: i64,
        trigger_effects: Vec<TriggerEffect>,
    },
    /// DDL statement (CREATE, DROP, ALTER, etc.).
    Ddl { label: String },
    /// Unknown operation type.
    Unknown,
}

impl StatementStatus {
    /// Format as a progress message.
    pub fn progress_message(&self) -> String {
        match self {
            StatementStatus::Delete { num_deletes, .. } => {
                format!("delete: {}", num_deletes)
            }
            StatementStatus::Insert { num_inserts, name, .. } => {
                let count = indicatif::HumanCount(*num_inserts as u64);
                match name {
                    Some(name) => format!("inserting {} rows into {}", count, name),
                    None => format!("inserting {} rows", count),
                }
            }
            StatementStatus::Update { num_updates, .. } => {
                format!("update: {}", num_updates)
            }
            StatementStatus::Ddl { label } => label.clone(),
            StatementStatus::Unknown => "unknown".to_string(),
        }
    }

    /// Format as a completion message (main operation only, no trigger effects).
    pub fn completion_message(&self) -> String {
        match self {
            StatementStatus::Insert {
                num_inserts, name, ..
            } => {
                let count = indicatif::HumanCount(*num_inserts as u64);
                let table = name.as_deref().unwrap_or("???");
                format!("inserted {} rows into {} ", count, table)
            }
            StatementStatus::Delete { num_deletes, .. } => {
                let count = indicatif::HumanCount(*num_deletes as u64);
                format!("deleted {} rows ", count)
            }
            StatementStatus::Update { num_updates, .. } => {
                let count = indicatif::HumanCount(*num_updates as u64);
                format!("updated {} rows ", count)
            }
            StatementStatus::Ddl { label } => format!("{} ", label),
            StatementStatus::Unknown => String::new(),
        }
    }

    /// Return formatted trigger effect lines (empty if none).
    pub fn trigger_effect_lines(&self) -> Vec<String> {
        let effects = match self {
            StatementStatus::Insert { trigger_effects, .. }
            | StatementStatus::Delete { trigger_effects, .. }
            | StatementStatus::Update { trigger_effects, .. } => trigger_effects,
            _ => return vec![],
        };
        effects
            .iter()
            .map(|e| {
                let count = indicatif::HumanCount(e.count as u64);
                format!("{} rows {} {}", count, e.operation, e.table)
            })
            .collect()
    }
}

/// Collect trigger effects from bytecode steps that belong to subprograms.
fn collect_trigger_effects(steps: &[BytecodeStep]) -> Vec<TriggerEffect> {
    let mut effects: HashMap<(String, TriggerOperation), i64> = HashMap::new();

    for step in steps.iter().filter(|s| !s.subprog.is_empty()) {
        if step.opcode == "Delete" && !step.p4.is_empty() && step.nexec > 0 {
            let key = (step.p4.clone(), TriggerOperation::Delete);
            *effects.entry(key).or_insert(0) += step.nexec;
        } else if step.opcode == "Insert" && !step.p4.is_empty() && step.nexec > 0 {
            if step.p5 & 0x04 > 0 {
                let key = (step.p4.clone(), TriggerOperation::Update);
                *effects.entry(key).or_insert(0) += step.nexec;
            } else {
                let key = (step.p4.clone(), TriggerOperation::Insert);
                *effects.entry(key).or_insert(0) += step.nexec;
            }
        }
    }

    let mut result: Vec<TriggerEffect> = effects
        .into_iter()
        .map(|((table, operation), count)| TriggerEffect {
            table,
            operation,
            count,
        })
        .collect();

    result.sort_by(|a, b| a.table.cmp(&b.table));
    result
}

/// Detect DDL from bytecode opcodes.
///
/// CREATE statements have `ParseSchema`. DROP statements have `DropTable`,
/// `DropTrigger`, or `DropIndex`. The object type and name come from
/// `String` opcodes that populate the sqlite_master row.
///
/// Returns None for CREATE TABLE ... AS SELECT (which also inserts user data)
/// — detected by the presence of `Yield` (coroutine feeding SELECT rows).
fn detect_ddl(main_steps: &[&BytecodeStep]) -> Option<String> {
    // DROP detection — the Drop* opcodes carry the name in p4
    for s in main_steps.iter() {
        match s.opcode.as_str() {
            "DropTable" => {
                let name = if s.p4.is_empty() { "???" } else { &s.p4 };
                return Some(format!("dropped table {}", name));
            }
            "DropTrigger" => {
                let name = if s.p4.is_empty() { "???" } else { &s.p4 };
                return Some(format!("dropped trigger {}", name));
            }
            "DropIndex" => {
                let name = if s.p4.is_empty() { "???" } else { &s.p4 };
                return Some(format!("dropped index {}", name));
            }
            _ => {}
        }
    }

    // CREATE detection — requires ParseSchema
    let has_parse_schema = main_steps.iter().any(|s| s.opcode == "ParseSchema");
    if !has_parse_schema {
        return None;
    }

    // CREATE TABLE ... AS SELECT uses a Yield coroutine to feed rows.
    // That's DML (it inserts user data), not pure DDL.
    let has_yield = main_steps.iter().any(|s| s.opcode == "Yield");
    if has_yield {
        return None;
    }

    let has_vcreate = main_steps.iter().any(|s| s.opcode == "VCreate");

    // String opcodes loading sqlite_master fields follow a consistent pattern:
    // type ("table"/"trigger"/"index"/"view"), then name, then tbl_name, ...
    let strings: Vec<&BytecodeStep> = main_steps
        .iter()
        .filter(|s| s.opcode == "String" || s.opcode == "String8")
        .copied()
        .collect();

    for (i, s) in strings.iter().enumerate() {
        let kind = match s.p4.as_str() {
            "table" if has_vcreate => "virtual table",
            "table" => "table",
            "trigger" => "trigger",
            "index" => "index",
            "view" => "view",
            _ => continue,
        };
        let name = strings
            .get(i + 1)
            .map(|s| s.p4.as_str())
            .unwrap_or("???");
        return Some(format!("created {} {}", kind, name));
    }

    Some("schema changed".to_string())
}

/// Get the SQL text from a prepared statement.
fn stmt_sql(stmt: *mut sqlite3_stmt) -> Option<String> {
    let z = unsafe { sqlite3_sql(stmt) };
    if z.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(z) };
    Some(s.to_string_lossy().to_string())
}

/// Extract the table name from a DML SQL statement.
/// Used only for virtual table operations where the bytecode
/// doesn't contain the table name (VUpdate p4 is a pointer).
fn extract_table_name_from_sql(sql: &str) -> Option<String> {
    let words: Vec<&str> = sql.trim_start().split_whitespace().collect();
    let upper: Vec<String> = words.iter().map(|w| w.to_ascii_uppercase()).collect();

    // INSERT [OR ...] INTO <name>
    if let Some(pos) = upper.iter().position(|w| w == "INTO") {
        return words
            .get(pos + 1)
            .map(|s| s.split('(').next().unwrap_or(s).to_string());
    }

    // UPDATE <name> or DELETE FROM <name>
    if let Some(pos) = upper.iter().position(|w| w == "FROM") {
        return words.get(pos + 1).map(|s| s.to_string());
    }

    // UPDATE <name> SET ...
    if upper.first().map(|s| s.as_str()) == Some("UPDATE") {
        return words.get(1).map(|s| s.to_string());
    }

    None
}

/// Extract the status of a statement from its bytecode.
pub fn get_statement_status(stmt: *mut sqlite3_stmt) -> StatementStatus {
    let steps = unsafe { bytecode_steps(stmt) };

    // Only consider main program opcodes (subprog is empty for main program)
    let main_steps: Vec<_> = steps.iter().filter(|s| s.subprog.is_empty()).collect();

    // DDL detection: ParseSchema opcode + String opcodes for type/name
    if let Some(label) = detect_ddl(&main_steps) {
        return StatementStatus::Ddl { label };
    }

    let trigger_effects = collect_trigger_effects(&steps);

    // Check for DELETE operations in main program
    let deletes: Vec<_> = main_steps
        .iter()
        .filter(|step| step.opcode == "Delete")
        .collect();

    if !deletes.is_empty() {
        let num_deletes = deletes[0].nexec;
        return StatementStatus::Delete {
            num_deletes,
            trigger_effects,
        };
    }

    // Check for INSERT operations in main program.
    // Prefer Insert opcodes with a real table name (non-empty p4), since
    // RETURNING and other internal operations create ephemeral Insert opcodes
    // with empty p4 and potentially unreliable nexec counts.
    let inserts: Vec<_> = main_steps
        .iter()
        .filter(|step| step.opcode == "Insert")
        .collect();

    if !inserts.is_empty() {
        let named_inserts: Vec<_> = inserts.iter().filter(|s| !s.p4.is_empty()).collect();
        let insert = if !named_inserts.is_empty() {
            named_inserts.iter().max_by_key(|step| step.nexec).unwrap()
        } else {
            match inserts.iter().max_by_key(|step| step.nexec) {
                Some(i) => i,
                None => return StatementStatus::Unknown,
            }
        };

        // Check if this is actually an UPDATE (OPFLAG_ISUPDATE = 0x04)
        if insert.p5 & 0x04 > 0 {
            return StatementStatus::Update {
                num_updates: insert.nexec,
                trigger_effects,
            };
        }

        // Determine the table name
        let name = if insert.p4.is_empty() {
            // CREATE TABLE ... AS SELECT: extract name from the String opcode
            main_steps
                .iter()
                .find(|step| {
                    (step.opcode == "String" || step.opcode == "String8")
                        && step.p4.starts_with("CREATE TABLE ")
                })
                .and_then(|step| {
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
            trigger_effects,
        };
    }

    // Virtual table DML: VUpdate opcode. Table name is not in the bytecode
    // (p4 is a vtab pointer), so we extract it from the SQL text.
    let vupdates: Vec<_> = main_steps
        .iter()
        .filter(|step| step.opcode == "VUpdate" && step.nexec > 0)
        .collect();

    if !vupdates.is_empty() {
        let count: i64 = vupdates.iter().map(|s| s.nexec).sum();
        let name = stmt_sql(stmt).and_then(|sql| extract_table_name_from_sql(&sql));

        // Determine operation type from SQL (VUpdate is used for all three)
        if let Some(sql) = stmt_sql(stmt) {
            let upper = sql.trim_start().to_ascii_uppercase();
            if upper.starts_with("DELETE ") {
                return StatementStatus::Delete {
                    num_deletes: count,
                    trigger_effects,
                };
            }
            if upper.starts_with("UPDATE ") {
                return StatementStatus::Update {
                    num_updates: count,
                    trigger_effects,
                };
            }
        }

        return StatementStatus::Insert {
            num_inserts: count,
            name,
            trigger_effects,
        };
    }

    StatementStatus::Unknown
}
