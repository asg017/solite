//! Single-table/view introspection.
//!
//! This module implements the `.describe` (alias `.d`) command: given a
//! `[schema.]name`, it answers "tell me about this table" — kind and flags,
//! columns with types/affinity, foreign keys in and out, indexes, DDL, and a
//! guarded row count. It also holds a prepared (unstepped) sample statement
//! for the caller to render.
//!
//! # Usage
//!
//! ```sql
//! .describe users        -- resolves in 'main'
//! .d temp.scratch        -- explicit schema
//! .describe "my table"   -- quoted identifier
//! ```
//!
//! Unqualified names resolve in `main` only — `temp`/attached tables must be
//! qualified, matching `.tables`. This module returns pure data
//! ([`TableDescription`]); rendering it as text or HTML is a front-end
//! concern (core cannot depend on `solite-table`).

use crate::dot::DotError;
use crate::sqlite::{quote_identifier, Statement, ValueRefXValue};
use crate::{ParseDotError, Runtime};
use serde::Serialize;

/// `.describe [schema.]name` — single-table introspection.
#[derive(Serialize, Debug, PartialEq)]
pub struct DescribeCommand {
    /// Explicit schema (`temp`, an attached db). `None` means `main` —
    /// unqualified names are NOT searched in temp/attached (same as `.tables`).
    pub schema: Option<String>,
    pub name: String,
}

/// What kind of object `.describe` resolved to.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub enum TableKind {
    Table,
    View,
    Virtual { module: String },
    Shadow,
}

/// A table/virtual-table row count, guarded by [`COUNT_CAP`]. Views are
/// never counted (`Skipped`) since the sample already scans them once.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub enum RowCount {
    Exact(u64),
    AtLeast(u64),
    Skipped,
}

/// One column from `pragma_table_xinfo`, including hidden/generated columns
/// (unlike `solite-schema`'s introspection, which drops them — the whole
/// point here is to show them).
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct ColumnDesc {
    pub cid: i64,
    pub name: String,
    /// "" when undeclared.
    pub declared_type: String,
    /// INTEGER | TEXT | BLOB | REAL | NUMERIC (SQLite §3.1 rules).
    pub affinity: &'static str,
    pub not_null: bool,
    /// 0 = not part of the primary key, else 1-based position in it.
    pub pk: i64,
    pub default: Option<String>,
    /// table_xinfo hidden flag: 0 normal, 1 hidden (vtab), 2 generated
    /// virtual, 3 generated stored.
    pub hidden: i64,
}

/// One row from `pragma_foreign_key_list`, either declared on the described
/// table (`foreign_keys_out`) or discovered by scanning the schema for
/// tables that reference it (`foreign_keys_in`).
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct ForeignKeyDesc {
    pub id: i64,
    pub seq: i64,
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    /// `None` when the FK references the target's primary key implicitly.
    pub to_column: Option<String>,
    pub on_update: String,
    pub on_delete: String,
}

/// One index from `pragma_index_list` plus its member columns from
/// `pragma_index_info`.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct IndexDesc {
    pub name: String,
    /// "<expr>" for expression members (`pragma_index_info.name` is NULL).
    pub columns: Vec<String>,
    pub unique: bool,
    pub partial: bool,
    /// "c" CREATE INDEX | "u" UNIQUE constraint | "pk" primary key.
    pub origin: String,
}

/// Everything `.describe` knows about a table or view.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct TableDescription {
    /// Resolved schema: "main" when unqualified.
    pub schema: String,
    pub name: String,
    pub kind: TableKind,
    pub without_rowid: bool,
    pub strict: bool,
    pub row_count: RowCount,
    pub columns: Vec<ColumnDesc>,
    /// This table → others.
    pub foreign_keys_out: Vec<ForeignKeyDesc>,
    /// Other tables → this one (same schema only).
    pub foreign_keys_in: Vec<ForeignKeyDesc>,
    pub indexes: Vec<IndexDesc>,
    /// `sqlite_master.sql` + ";" (`None` for eponymous vtabs etc.).
    pub ddl: Option<String>,
}

/// What `.describe` hands to front-ends: the data plus a prepared, unstepped
/// sample statement. Not `Serialize` (a `Statement` can't be serialized
/// meaningfully) — serialize `.description` instead.
pub struct DescribeOutput {
    pub description: TableDescription,
    /// `SELECT * FROM "schema"."name" LIMIT 10`, unstepped.
    pub sample: Statement,
}

pub const SAMPLE_ROWS: usize = 10;
pub const COUNT_CAP: u64 = 100_000;

impl TableDescription {
    /// Human label for `kind`: "table", "view", "virtual table (fts5)",
    /// "shadow table". Shared by the text (ticket 02) and HTML (ticket 03)
    /// front-ends so the two stay in sync.
    pub fn kind_label(&self) -> String {
        match &self.kind {
            TableKind::Table => "table".to_string(),
            TableKind::View => "view".to_string(),
            TableKind::Virtual { module } => format!("virtual table ({module})"),
            TableKind::Shadow => "shadow table".to_string(),
        }
    }

    /// Guarded row count as a thousands-separated string ("1,234",
    /// "100,000+"), or `None` for views (never counted — see [`RowCount`]).
    pub fn row_count_label(&self) -> Option<String> {
        match self.row_count {
            RowCount::Exact(n) => Some(format_thousands(n)),
            RowCount::AtLeast(cap) => Some(format!("{}+", format_thousands(cap))),
            RowCount::Skipped => None,
        }
    }
}

/// Render `n` with `,` thousands separators (e.g. `1234` -> `"1,234"`).
fn format_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

impl DescribeCommand {
    /// Parse `.describe` arguments: `name`, `schema.name`, or quoted forms
    /// of either part (`"n"`, `` `n` ``, `[n]`). Splits on the first
    /// unquoted `.` so `main."my.table"` treats the dot inside quotes as
    /// part of the name, not a schema separator.
    pub fn parse_args(args: &str) -> Result<Self, ParseDotError> {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            return Err(ParseDotError::InvalidArgument(
                "usage: .describe [schema.]name".to_string(),
            ));
        }

        Ok(match split_unquoted_dot(trimmed) {
            Some((schema_part, name_part)) => DescribeCommand {
                schema: Some(unquote_identifier(schema_part.trim())),
                name: unquote_identifier(name_part.trim()),
            },
            None => DescribeCommand {
                schema: None,
                name: unquote_identifier(trimmed),
            },
        })
    }

    /// Execute the describe command, returning the description plus a
    /// prepared sample statement.
    pub fn execute(&self, runtime: &Runtime) -> Result<DescribeOutput, DotError> {
        let schema = self.schema.clone().unwrap_or_else(|| "main".to_string());
        let name = self.name.clone();

        let (type_str, without_rowid, strict) = table_list_row(runtime, &schema, &name)?
            .ok_or_else(|| DotError::InvalidData(format!("no such table: {}.{}", schema, name)))?;
        let kind = resolve_kind(runtime, &schema, &name, &type_str)?;

        let columns = columns_of(runtime, &schema, &name)?;
        let foreign_keys_out = foreign_keys_of(runtime, &schema, &name)?;
        let foreign_keys_in = foreign_keys_in_of(runtime, &schema, &name)?;
        let indexes = indexes_of(runtime, &schema, &name)?;
        let ddl = ddl_of(runtime, &schema, &name)?;
        let row_count = row_count_of(runtime, &schema, &name, &kind)?;

        // Build the sample last so an error in any introspection query
        // above surfaces before a statement is held open.
        let sample_sql = format!(
            "SELECT * FROM {}.{} LIMIT {}",
            quote_identifier(&schema),
            quote_identifier(&name),
            SAMPLE_ROWS
        );
        let (_, stmt) = runtime.connection.prepare(&sample_sql)?;
        let sample = stmt.ok_or_else(|| {
            DotError::InvalidData("internal: describe sample query produced no statement".into())
        })?;

        Ok(DescribeOutput {
            description: TableDescription {
                schema,
                name,
                kind,
                without_rowid,
                strict,
                row_count,
                columns,
                foreign_keys_out,
                foreign_keys_in,
                indexes,
                ddl,
            },
            sample,
        })
    }
}

/// Kind of the named object, or `None` if it doesn't exist. Used by the
/// Jupyter kernel's bare-identifier check (ticket 04) — cheap: one
/// `pragma_table_list` query (plus, for virtual tables only, a DDL lookup
/// for the module name), no row count.
pub fn table_kind(
    runtime: &Runtime,
    schema: Option<&str>,
    name: &str,
) -> Result<Option<TableKind>, DotError> {
    let schema = schema.unwrap_or("main");
    match table_list_row(runtime, schema, name)? {
        None => Ok(None),
        Some((type_str, _, _)) => Ok(Some(resolve_kind(runtime, schema, name, &type_str)?)),
    }
}

/// Raw foreign keys declared on `table`. Shared with `graphviz.rs`.
pub fn foreign_keys_of(
    runtime: &Runtime,
    schema: &str,
    table: &str,
) -> Result<Vec<ForeignKeyDesc>, DotError> {
    let (_, stmt) = runtime.connection.prepare(
        r#"SELECT id, seq, "table", "from", "to", on_update, on_delete FROM pragma_foreign_key_list(?1, ?2)"#,
    )?;
    let mut stmt = stmt.ok_or_else(|| {
        DotError::InvalidData("internal: describe foreign key query produced no statement".into())
    })?;
    stmt.bind_text(1, table)?;
    stmt.bind_text(2, schema)?;

    let mut fks = Vec::new();
    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                let id = row.first().map(|v| v.as_int64()).unwrap_or_default();
                let seq = row.get(1).map(|v| v.as_int64()).unwrap_or_default();
                let to_table = row
                    .get(2)
                    .map(|v| v.as_str().to_owned())
                    .unwrap_or_default();
                let from_column = row
                    .get(3)
                    .map(|v| v.as_str().to_owned())
                    .unwrap_or_default();
                let to_column = row.get(4).and_then(|v| match &v.value {
                    ValueRefXValue::Null => None,
                    _ => Some(v.as_str().to_owned()),
                });
                let on_update = row
                    .get(5)
                    .map(|v| v.as_str().to_owned())
                    .unwrap_or_default();
                let on_delete = row
                    .get(6)
                    .map(|v| v.as_str().to_owned())
                    .unwrap_or_default();
                fks.push(ForeignKeyDesc {
                    id,
                    seq,
                    from_table: table.to_string(),
                    from_column,
                    to_table,
                    to_column,
                    on_update,
                    on_delete,
                });
            }
            Ok(None) => break,
            // propagate instead of silently returning a truncated list
            Err(e) => return Err(e.into()),
        }
    }
    Ok(fks)
}

/// SQLite affinity rules (§3.1 of the file format doc): substring match,
/// case-insensitive, checked in this order.
pub fn affinity_of(declared: &str) -> &'static str {
    let upper = declared.to_ascii_uppercase();
    if upper.contains("INT") {
        "INTEGER"
    } else if upper.contains("CHAR") || upper.contains("CLOB") || upper.contains("TEXT") {
        "TEXT"
    } else if upper.contains("BLOB") || upper.is_empty() {
        "BLOB"
    } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
        "REAL"
    } else {
        "NUMERIC"
    }
}

/// Unquote a SQLite-style identifier: `"…"` (with `""` escaping embedded
/// quotes), `` `…` `` (same escaping with backticks), `[…]` (no escaping —
/// `]` cannot appear inside), or an unquoted identifier returned as-is.
/// `pub(crate)` so the Jupyter bare-identifier grammar (ticket 04) can reuse
/// it. Unlike `strip_surrounding_quotes` in `dot/mod.rs`, this un-doubles
/// `""`/`` `` `` escapes and understands bracket quoting.
pub(crate) fn unquote_identifier(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        match (bytes[0], bytes[bytes.len() - 1]) {
            (b'"', b'"') => return s[1..s.len() - 1].replace("\"\"", "\""),
            (b'`', b'`') => return s[1..s.len() - 1].replace("``", "`"),
            (b'[', b']') => return s[1..s.len() - 1].to_string(),
            _ => {}
        }
    }
    s.to_string()
}

/// Find the first `.` that is not inside a `"…"`, `` `…` ``, or `[…]`
/// quoted span, and split the string there (both halves excluding the dot).
fn split_unquoted_dot(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(open) => {
                let close = if open == b'[' { b']' } else { open };
                if b == close {
                    quote = None;
                }
            }
            None => match b {
                b'"' | b'`' | b'[' => quote = Some(b),
                b'.' => return Some((&s[..i], &s[i + 1..])),
                _ => {}
            },
        }
        i += 1;
    }
    None
}

/// `(type, without_rowid, strict)` for `schema.name`, or `None` if it
/// doesn't exist. `type` is the raw `pragma_table_list.type` string
/// (table/view/virtual/shadow).
fn table_list_row(
    runtime: &Runtime,
    schema: &str,
    name: &str,
) -> Result<Option<(String, bool, bool)>, DotError> {
    let (_, stmt) = runtime
        .connection
        .prepare(r#"SELECT type, wr, strict FROM pragma_table_list WHERE "schema" = ?1 AND name = ?2"#)?;
    let mut stmt = stmt.ok_or_else(|| {
        DotError::InvalidData("internal: describe kind query produced no statement".into())
    })?;
    stmt.bind_text(1, schema)?;
    stmt.bind_text(2, name)?;

    match stmt.next()? {
        Some(row) => {
            let type_str = row
                .first()
                .map(|v| v.as_str().to_owned())
                .unwrap_or_default();
            let without_rowid = row.get(1).is_some_and(|v| v.as_int64() != 0);
            let strict = row.get(2).is_some_and(|v| v.as_int64() != 0);
            Ok(Some((type_str, without_rowid, strict)))
        }
        None => Ok(None),
    }
}

/// Map a raw `pragma_table_list.type` string to a [`TableKind`], resolving
/// the module name for virtual tables from the DDL.
fn resolve_kind(
    runtime: &Runtime,
    schema: &str,
    name: &str,
    type_str: &str,
) -> Result<TableKind, DotError> {
    Ok(match type_str {
        "view" => TableKind::View,
        "virtual" => TableKind::Virtual {
            module: virtual_table_module(runtime, schema, name)?,
        },
        "shadow" => TableKind::Shadow,
        _ => TableKind::Table,
    })
}

/// Case-insensitive `USING <ident>` module name from a `CREATE VIRTUAL
/// TABLE` statement in `sqlite_master`.
fn virtual_table_module(runtime: &Runtime, schema: &str, name: &str) -> Result<String, DotError> {
    let ddl = ddl_of(runtime, schema, name)?.unwrap_or_default();
    let lower = ddl.to_ascii_lowercase();
    if let Some(idx) = lower.find("using") {
        let after = ddl[idx + "using".len()..].trim_start();
        let module: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !module.is_empty() {
            return Ok(module);
        }
    }
    Ok(String::new())
}

fn columns_of(runtime: &Runtime, schema: &str, name: &str) -> Result<Vec<ColumnDesc>, DotError> {
    let (_, stmt) = runtime.connection.prepare(
        r#"SELECT cid, name, type, "notnull", dflt_value, pk, hidden FROM pragma_table_xinfo(?1, ?2)"#,
    )?;
    let mut stmt = stmt.ok_or_else(|| {
        DotError::InvalidData("internal: describe columns query produced no statement".into())
    })?;
    stmt.bind_text(1, name)?;
    stmt.bind_text(2, schema)?;

    let mut columns = Vec::new();
    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                let cid = row.first().map(|v| v.as_int64()).unwrap_or_default();
                let col_name = row
                    .get(1)
                    .map(|v| v.as_str().to_owned())
                    .unwrap_or_default();
                let declared_type = row
                    .get(2)
                    .map(|v| v.as_str().to_owned())
                    .unwrap_or_default();
                let not_null = row.get(3).is_some_and(|v| v.as_int64() != 0);
                let default = row.get(4).and_then(|v| match &v.value {
                    ValueRefXValue::Null => None,
                    _ => Some(v.as_str().to_owned()),
                });
                let pk = row.get(5).map(|v| v.as_int64()).unwrap_or_default();
                let hidden = row.get(6).map(|v| v.as_int64()).unwrap_or_default();
                let affinity = affinity_of(&declared_type);
                columns.push(ColumnDesc {
                    cid,
                    name: col_name,
                    declared_type,
                    affinity,
                    not_null,
                    pk,
                    default,
                    hidden,
                });
            }
            Ok(None) => break,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(columns)
}

/// Foreign keys declared on other tables in `schema` that point back at
/// `name`. O(tables) pragma calls — acceptable; it's what `.graphviz`
/// already does for the whole database.
fn foreign_keys_in_of(
    runtime: &Runtime,
    schema: &str,
    name: &str,
) -> Result<Vec<ForeignKeyDesc>, DotError> {
    let (_, stmt) = runtime.connection.prepare(
        r#"SELECT name FROM pragma_table_list WHERE "schema" = ?1 AND type = 'table' AND name <> ?2"#,
    )?;
    let mut stmt = stmt.ok_or_else(|| {
        DotError::InvalidData("internal: describe incoming FK scan produced no statement".into())
    })?;
    stmt.bind_text(1, schema)?;
    stmt.bind_text(2, name)?;

    let mut other_tables = Vec::new();
    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                if let Some(v) = row.first() {
                    other_tables.push(v.as_str().to_owned());
                }
            }
            Ok(None) => break,
            Err(e) => return Err(e.into()),
        }
    }

    let mut incoming = Vec::new();
    for other in other_tables {
        for fk in foreign_keys_of(runtime, schema, &other)? {
            if fk.to_table.eq_ignore_ascii_case(name) {
                incoming.push(fk);
            }
        }
    }
    Ok(incoming)
}

fn indexes_of(runtime: &Runtime, schema: &str, name: &str) -> Result<Vec<IndexDesc>, DotError> {
    let (_, stmt) = runtime
        .connection
        .prepare(r#"SELECT name, "unique", origin, partial FROM pragma_index_list(?1, ?2)"#)?;
    let mut stmt = stmt.ok_or_else(|| {
        DotError::InvalidData("internal: describe index list query produced no statement".into())
    })?;
    stmt.bind_text(1, name)?;
    stmt.bind_text(2, schema)?;

    let mut index_rows = Vec::new();
    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                let idx_name = row
                    .first()
                    .map(|v| v.as_str().to_owned())
                    .unwrap_or_default();
                let unique = row.get(1).is_some_and(|v| v.as_int64() != 0);
                let origin = row
                    .get(2)
                    .map(|v| v.as_str().to_owned())
                    .unwrap_or_default();
                let partial = row.get(3).is_some_and(|v| v.as_int64() != 0);
                index_rows.push((idx_name, unique, origin, partial));
            }
            Ok(None) => break,
            Err(e) => return Err(e.into()),
        }
    }

    let mut indexes = Vec::new();
    for (idx_name, unique, origin, partial) in index_rows {
        let columns = index_columns(runtime, schema, &idx_name)?;
        indexes.push(IndexDesc {
            name: idx_name,
            columns,
            unique,
            partial,
            origin,
        });
    }
    Ok(indexes)
}

fn index_columns(
    runtime: &Runtime,
    schema: &str,
    index_name: &str,
) -> Result<Vec<String>, DotError> {
    let (_, stmt) = runtime
        .connection
        .prepare(r#"SELECT name FROM pragma_index_info(?1, ?2) ORDER BY seqno"#)?;
    let mut stmt = stmt.ok_or_else(|| {
        DotError::InvalidData("internal: describe index info query produced no statement".into())
    })?;
    stmt.bind_text(1, index_name)?;
    stmt.bind_text(2, schema)?;

    let mut columns = Vec::new();
    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                let name = match row.first() {
                    Some(v) => match &v.value {
                        ValueRefXValue::Null => "<expr>".to_string(),
                        _ => v.as_str().to_owned(),
                    },
                    None => "<expr>".to_string(),
                };
                columns.push(name);
            }
            Ok(None) => break,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(columns)
}

/// `sqlite_master.sql` for `name`, terminated with `;` like `.schema` does
/// (`dot/schema.rs`). `None` for objects with no `sqlite_master` row
/// (eponymous virtual tables).
fn ddl_of(runtime: &Runtime, schema: &str, name: &str) -> Result<Option<String>, DotError> {
    // sqlite_master is not a pragma table-valued function, so the schema
    // must be spliced into the query text — the one place `.describe` does
    // this, via `quote_identifier`.
    let query = format!(
        "SELECT sql FROM {}.sqlite_master WHERE name = ?1",
        quote_identifier(schema)
    );
    let (_, stmt) = runtime.connection.prepare(&query)?;
    let mut stmt = stmt.ok_or_else(|| {
        DotError::InvalidData("internal: describe DDL query produced no statement".into())
    })?;
    stmt.bind_text(1, name)?;

    match stmt.next()? {
        Some(row) => match row.first() {
            Some(v) => match &v.value {
                ValueRefXValue::Null => Ok(None),
                _ => Ok(Some(format!("{};", v.as_str()))),
            },
            None => Ok(None),
        },
        None => Ok(None),
    }
}

/// Guarded row count: only tables and virtual/shadow tables are counted
/// (views are `Skipped` — never scan a view twice, the sample already does
/// once).
fn row_count_of(
    runtime: &Runtime,
    schema: &str,
    name: &str,
    kind: &TableKind,
) -> Result<RowCount, DotError> {
    if matches!(kind, TableKind::View) {
        return Ok(RowCount::Skipped);
    }

    let query = format!(
        "SELECT count(*) FROM (SELECT 1 FROM {}.{} LIMIT {})",
        quote_identifier(schema),
        quote_identifier(name),
        COUNT_CAP + 1
    );
    let (_, stmt) = runtime.connection.prepare(&query)?;
    let mut stmt = stmt.ok_or_else(|| {
        DotError::InvalidData("internal: describe row count query produced no statement".into())
    })?;

    let n = match stmt.next()? {
        Some(row) => row.first().map(|v| v.as_int64()).unwrap_or_default(),
        None => 0,
    };
    let n = n.max(0) as u64;
    Ok(if n > COUNT_CAP {
        RowCount::AtLeast(COUNT_CAP)
    } else {
        RowCount::Exact(n)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt() -> Runtime {
        Runtime::new(None).unwrap()
    }

    #[test]
    fn test_plain_table_with_declared_types() {
        let runtime = rt();
        runtime
            .connection
            .execute_script(
                "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INT);
                 INSERT INTO users (name, age) VALUES ('a', 1), ('b', 2);",
            )
            .unwrap();

        let cmd = DescribeCommand {
            schema: None,
            name: "users".to_string(),
        };
        let output = cmd.execute(&runtime).unwrap();
        let d = output.description;

        assert_eq!(d.schema, "main");
        assert_eq!(d.kind, TableKind::Table);
        assert_eq!(d.row_count, RowCount::Exact(2));
        assert!(!d.without_rowid);
        assert!(!d.strict);
        assert!(d.foreign_keys_out.is_empty());
        assert!(d.foreign_keys_in.is_empty());

        let names: Vec<&str> = d.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["id", "name", "age"]);
        assert_eq!(d.columns[0].affinity, "INTEGER");
        assert_eq!(d.columns[1].affinity, "TEXT");
        assert_eq!(d.columns[2].affinity, "INTEGER");
        assert!(d.columns[1].not_null);
        assert_eq!(d.columns[0].pk, 1);

        let ddl = d.ddl.unwrap();
        assert!(ddl.contains("CREATE TABLE users"));
        assert!(ddl.ends_with(';'));
    }

    #[test]
    fn test_without_rowid_and_strict_flags() {
        let runtime = rt();
        runtime
            .connection
            .execute_script(
                "CREATE TABLE kv (k TEXT PRIMARY KEY, v TEXT) WITHOUT ROWID, STRICT;",
            )
            .unwrap();

        let cmd = DescribeCommand {
            schema: None,
            name: "kv".to_string(),
        };
        let d = cmd.execute(&runtime).unwrap().description;
        assert!(d.without_rowid);
        assert!(d.strict);
    }

    #[test]
    fn test_view_is_never_counted() {
        let runtime = rt();
        runtime
            .connection
            .execute_script(
                "CREATE TABLE base (id INTEGER);
                 INSERT INTO base VALUES (1), (2), (3);
                 CREATE VIEW v_base AS SELECT * FROM base;",
            )
            .unwrap();

        let cmd = DescribeCommand {
            schema: None,
            name: "v_base".to_string(),
        };
        let d = cmd.execute(&runtime).unwrap().description;
        assert_eq!(d.kind, TableKind::View);
        assert_eq!(d.row_count, RowCount::Skipped);
        assert!(d.ddl.unwrap().contains("CREATE VIEW v_base"));
    }

    #[test]
    fn test_virtual_table_module_and_hidden_columns_and_shadow_table() {
        let runtime = rt();
        runtime
            .connection
            .execute_script("CREATE VIRTUAL TABLE notes USING fts5(body);")
            .unwrap();

        let cmd = DescribeCommand {
            schema: None,
            name: "notes".to_string(),
        };
        let d = cmd.execute(&runtime).unwrap().description;
        assert_eq!(
            d.kind,
            TableKind::Virtual {
                module: "fts5".to_string()
            }
        );
        // fts5 exposes hidden columns (e.g. `rank`) alongside `body`
        assert!(d.columns.iter().any(|c| c.hidden == 1));

        let shadow_cmd = DescribeCommand {
            schema: None,
            name: "notes_data".to_string(),
        };
        let shadow = shadow_cmd.execute(&runtime).unwrap().description;
        assert_eq!(shadow.kind, TableKind::Shadow);
    }

    #[test]
    fn test_generated_columns_hidden_flags() {
        let runtime = rt();
        runtime
            .connection
            .execute_script(
                "CREATE TABLE g (
                    a INTEGER,
                    b INTEGER GENERATED ALWAYS AS (a + 1) VIRTUAL,
                    c INTEGER GENERATED ALWAYS AS (a + 1) STORED
                );",
            )
            .unwrap();

        let cmd = DescribeCommand {
            schema: None,
            name: "g".to_string(),
        };
        let d = cmd.execute(&runtime).unwrap().description;
        let hidden = |name: &str| d.columns.iter().find(|c| c.name == name).unwrap().hidden;
        assert_eq!(hidden("a"), 0);
        assert_eq!(hidden("b"), 2);
        assert_eq!(hidden("c"), 3);
    }

    #[test]
    fn test_foreign_keys_out_and_in() {
        let runtime = rt();
        runtime
            .connection
            .execute_script(
                "CREATE TABLE users (id INTEGER PRIMARY KEY);
                 CREATE TABLE orders (
                    id INTEGER PRIMARY KEY,
                    user_id INTEGER REFERENCES users(id),
                    other_id INTEGER REFERENCES users
                 );",
            )
            .unwrap();

        let orders = DescribeCommand {
            schema: None,
            name: "orders".to_string(),
        }
        .execute(&runtime)
        .unwrap()
        .description;
        assert_eq!(orders.foreign_keys_out.len(), 2);
        let explicit = orders
            .foreign_keys_out
            .iter()
            .find(|fk| fk.from_column == "user_id")
            .unwrap();
        assert_eq!(explicit.to_table, "users");
        assert_eq!(explicit.to_column, Some("id".to_string()));

        let implicit = orders
            .foreign_keys_out
            .iter()
            .find(|fk| fk.from_column == "other_id")
            .unwrap();
        assert_eq!(implicit.to_column, None);

        let users = DescribeCommand {
            schema: None,
            name: "users".to_string(),
        }
        .execute(&runtime)
        .unwrap()
        .description;
        assert_eq!(users.foreign_keys_in.len(), 2);
        assert!(users
            .foreign_keys_in
            .iter()
            .all(|fk| fk.from_table == "orders"));
    }

    #[test]
    fn test_indexes_unique_partial_expression_and_autoindex_origins() {
        let runtime = rt();
        runtime
            .connection
            .execute_script(
                "CREATE TABLE t (a INTEGER, b INTEGER, c INTEGER, UNIQUE(a, b));
                 CREATE UNIQUE INDEX idx_partial ON t(c) WHERE c IS NOT NULL;
                 CREATE INDEX idx_expr ON t(a + b);
                 CREATE TABLE pk_t (a INTEGER, b INTEGER, PRIMARY KEY (a, b)) WITHOUT ROWID;",
            )
            .unwrap();

        let t = DescribeCommand {
            schema: None,
            name: "t".to_string(),
        }
        .execute(&runtime)
        .unwrap()
        .description;

        let autoindex = t
            .indexes
            .iter()
            .find(|i| i.origin == "u")
            .expect("expected autoindex from UNIQUE(a, b)");
        assert_eq!(autoindex.columns, vec!["a", "b"]);
        assert!(autoindex.unique);

        let partial = t.indexes.iter().find(|i| i.name == "idx_partial").unwrap();
        assert!(partial.unique);
        assert!(partial.partial);
        assert_eq!(partial.origin, "c");

        let expr = t.indexes.iter().find(|i| i.name == "idx_expr").unwrap();
        assert_eq!(expr.columns, vec!["<expr>"]);
        assert_eq!(expr.origin, "c");

        let pk_t = DescribeCommand {
            schema: None,
            name: "pk_t".to_string(),
        }
        .execute(&runtime)
        .unwrap()
        .description;
        assert!(pk_t.indexes.iter().any(|i| i.origin == "pk"));
    }

    #[test]
    fn test_quoted_and_awkward_names() {
        let runtime = rt();
        runtime
            .connection
            .execute_script("CREATE TABLE \"my table\" (id INTEGER);")
            .unwrap();
        runtime
            .connection
            .execute_script("CREATE TABLE \"o'brien.data\" (id INTEGER);")
            .unwrap();

        let d = DescribeCommand {
            schema: None,
            name: "my table".to_string(),
        }
        .execute(&runtime)
        .unwrap()
        .description;
        assert_eq!(d.name, "my table");

        let d2 = DescribeCommand {
            schema: None,
            name: "o'brien.data".to_string(),
        }
        .execute(&runtime)
        .unwrap()
        .description;
        assert_eq!(d2.name, "o'brien.data");
    }

    #[test]
    fn test_temp_qualification_required() {
        let runtime = rt();
        runtime
            .connection
            .execute_script("CREATE TEMP TABLE scratch (id INTEGER);")
            .unwrap();

        let qualified = DescribeCommand {
            schema: Some("temp".to_string()),
            name: "scratch".to_string(),
        }
        .execute(&runtime);
        assert!(qualified.is_ok());

        let unqualified = DescribeCommand {
            schema: None,
            name: "scratch".to_string(),
        }
        .execute(&runtime);
        assert!(unqualified.is_err());
    }

    #[test]
    fn test_not_found_error_message() {
        let runtime = rt();
        let result = DescribeCommand {
            schema: None,
            name: "nope".to_string(),
        }
        .execute(&runtime);
        let err = match result {
            Ok(_) => panic!("expected 'no such table' error"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("no such table: main.nope"));
    }

    #[test]
    fn test_row_count_cap() {
        let runtime = rt();
        runtime
            .connection
            .execute_script(
                "CREATE TABLE big (id INTEGER);
                 INSERT INTO big
                 WITH RECURSIVE cnt(x) AS (
                    SELECT 1 UNION ALL SELECT x + 1 FROM cnt WHERE x < 100001
                 )
                 SELECT x FROM cnt;",
            )
            .unwrap();

        let d = DescribeCommand {
            schema: None,
            name: "big".to_string(),
        }
        .execute(&runtime)
        .unwrap()
        .description;
        assert_eq!(d.row_count, RowCount::AtLeast(100_000));
    }

    #[test]
    fn test_affinity_of_five_rules() {
        let cases = [
            ("INTEGER", "INTEGER"),
            ("INT", "INTEGER"),
            ("BIGINT", "INTEGER"),
            ("CHARACTER(20)", "TEXT"),
            ("VARCHAR(255)", "TEXT"),
            ("CLOB", "TEXT"),
            ("TEXT", "TEXT"),
            ("BLOB", "BLOB"),
            ("", "BLOB"),
            ("REAL", "REAL"),
            ("FLOAT", "REAL"),
            ("DOUBLE PRECISION", "REAL"),
            ("NUMERIC", "NUMERIC"),
            ("DECIMAL(10,5)", "NUMERIC"),
            ("BOOLEAN", "NUMERIC"),
            ("DATE", "NUMERIC"),
        ];
        for (declared, expected) in cases {
            assert_eq!(affinity_of(declared), expected, "declared type: {declared}");
        }
    }

    #[test]
    fn test_parse_args_cases() {
        let cmd = DescribeCommand::parse_args("users").unwrap();
        assert_eq!(cmd.schema, None);
        assert_eq!(cmd.name, "users");

        let cmd = DescribeCommand::parse_args("temp.users").unwrap();
        assert_eq!(cmd.schema, Some("temp".to_string()));
        assert_eq!(cmd.name, "users");

        let cmd = DescribeCommand::parse_args("\"my table\"").unwrap();
        assert_eq!(cmd.schema, None);
        assert_eq!(cmd.name, "my table");

        let cmd = DescribeCommand::parse_args("main.\"my.table\"").unwrap();
        assert_eq!(cmd.schema, Some("main".to_string()));
        assert_eq!(cmd.name, "my.table");

        let cmd = DescribeCommand::parse_args("`x`").unwrap();
        assert_eq!(cmd.schema, None);
        assert_eq!(cmd.name, "x");

        let cmd = DescribeCommand::parse_args("[x]").unwrap();
        assert_eq!(cmd.schema, None);
        assert_eq!(cmd.name, "x");

        assert!(DescribeCommand::parse_args("").is_err());
        assert!(DescribeCommand::parse_args("   ").is_err());
    }

    #[test]
    fn test_kind_label_and_row_count_label() {
        let d = TableDescription {
            schema: "main".to_string(),
            name: "t".to_string(),
            kind: TableKind::Virtual {
                module: "fts5".to_string(),
            },
            without_rowid: false,
            strict: false,
            row_count: RowCount::AtLeast(COUNT_CAP),
            columns: Vec::new(),
            foreign_keys_out: Vec::new(),
            foreign_keys_in: Vec::new(),
            indexes: Vec::new(),
            ddl: None,
        };
        assert_eq!(d.kind_label(), "virtual table (fts5)");
        assert_eq!(d.row_count_label(), Some("100,000+".to_string()));

        let mut d2 = d.clone();
        d2.kind = TableKind::View;
        d2.row_count = RowCount::Skipped;
        assert_eq!(d2.kind_label(), "view");
        assert_eq!(d2.row_count_label(), None);

        let mut d3 = d.clone();
        d3.kind = TableKind::Table;
        d3.row_count = RowCount::Exact(1234);
        assert_eq!(d3.kind_label(), "table");
        assert_eq!(d3.row_count_label(), Some("1,234".to_string()));
    }

    #[test]
    fn test_table_kind_missing_and_view() {
        let runtime = rt();
        assert_eq!(table_kind(&runtime, None, "nope").unwrap(), None);

        runtime
            .connection
            .execute_script(
                "CREATE TABLE base (id INTEGER);
                 CREATE VIEW v_base AS SELECT * FROM base;",
            )
            .unwrap();
        assert_eq!(
            table_kind(&runtime, None, "v_base").unwrap(),
            Some(TableKind::View)
        );
    }
}
