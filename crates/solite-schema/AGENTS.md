# solite-schema

Schema introspection, dot command parsing, and document handling for Solite. Used by the LSP and analyzer to understand database schemas from multiple sources.

## What This Crate Does

Parses SQL files that may contain dot commands (`.open`) and schema hints (`-- schema:`), provides a unified `SchemaProvider` trait to load schema information from SQLite databases, JSON strings, or parsed DDL, and introspects live SQLite databases for tables/views/indexes/triggers. The `introspect` module is gated behind `#[cfg(not(target_arch = "wasm32"))]`.

## File Layout

- `src/lib.rs` - `Document` struct (primary entry point for parsing files), re-exports
- `src/dotcmd.rs` - Dot command parsing: `DotCommand`, `SchemaHint`, `SqlRegion`, `ParseResult`
- `src/provider.rs` - `SchemaProvider` trait and three implementations
- `src/introspect.rs` - Low-level SQLite introspection via rusqlite (non-WASM only)
- `src/json.rs` - `JsonSchema` and related serde types for JSON-based schema exchange

## Key Types

### Document (`lib.rs`)

Primary entry point. `Document::parse(source, enable_dot_commands)` splits a source string into dot commands, SQL regions, and schema hints, then parses the SQL regions into a `Program`.

Fields: `source`, `dot_commands: Vec<DotCommand>`, `sql_regions: Vec<SqlRegion>`, `program: Result<Program, Vec<ParseError>>`, `schema_hints: Vec<SchemaHint>`.

### DotCommand (`dotcmd.rs`)

Currently only `DotCommand::Open { path, span }`. Unknown dot commands are silently ignored. Command names are case-insensitive. Paths support single/double quoting for spaces.

### SchemaHint (`dotcmd.rs`)

`-- schema: <path>` comments parsed from the file header only (before any SQL or dot command lines). Blank lines and regular comments are allowed in the header region. Once a non-comment, non-blank line appears, header parsing stops.

### SqlRegion (`dotcmd.rs`)

Byte offset range (`start`, `end`) into the original source identifying a contiguous block of SQL (non-dot-command) text.

## SchemaProvider Trait (`provider.rs`)

```rust
pub trait SchemaProvider: Send + Sync {
    fn load(&self) -> Result<Schema, SchemaError>;
}
```

Returns a `solite_analyzer::Schema`. Three implementations:

### DdlSchemaProvider

Builds schema from a parsed `solite_ast::Program` containing DDL statements. Constructed via `DdlSchemaProvider::new(program)` or `DdlSchemaProvider::from_sql(sql)`. Delegates to `solite_analyzer::build_schema()`.

### FileSchemaProvider (non-WASM only)

Loads schema by introspecting a SQLite database file at a given path. Uses `introspect::introspect_sqlite_db()` internally. Opens the database read-only.

### JsonSchemaProvider

Parses a JSON string into `JsonSchema`, validates it, then converts to analyzer `Schema`. Works on all targets including WASM.

## Introspection (`introspect.rs`, non-WASM only)

- `introspect_sqlite_db(path)` - Opens a file read-only and returns `IntrospectedSchema`
- `introspect_connection(conn)` - Introspects from an existing `rusqlite::Connection`
- `discover_virtual_table_columns(conn)` - Queries `pragma_module_list`, then probes each module with `SELECT * FROM "<module>"` to discover eponymous virtual tables and their columns. Returns `Vec<(String, Vec<String>)>`.

`IntrospectedSchema` holds `HashMap`s keyed by lowercase name for `TableInfo`, `IndexInfo`, `ViewInfo`, `TriggerInfo`. All lookups are case-insensitive. `TableInfo` tracks both a `columns: HashSet<String>` (lowercase) and `original_columns: Vec<String>` (preserves declaration order and case).

## JSON Schema Format (`json.rs`)

```json
{
  "tables": [{ "name": "...", "columns": [{ "name": "...", "type": "...", "primary_key": false, "not_null": false }], "without_rowid": false }],
  "views": [{ "name": "...", "columns": ["col1", "col2"] }],
  "indexes": [{ "name": "...", "table_name": "...", "columns": ["col"], "unique": false }],
  "triggers": [{ "name": "...", "table_name": "...", "event": "INSERT" }]
}
```

`JsonSchema::validate()` checks for duplicate table/view/index/trigger names and duplicate columns within tables. Tables and columns support optional `description`, `tags`, and `example` fields for sqlite-docs integration.

## Dot Command Parsing (`dotcmd.rs`)

`parse_dot_commands(source)` scans line-by-line. Lines starting with `.` are treated as dot commands; all other non-empty lines accumulate into `SqlRegion`s. Empty/whitespace-only lines break SQL regions. The parser handles `\r\n` and `\n` line endings.

Header parsing (for `-- schema:` hints) runs until the first non-blank, non-comment line is encountered.
