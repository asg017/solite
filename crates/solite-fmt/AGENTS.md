# solite-fmt

SQL formatter for SQLite. Parses SQL into an AST (via `solite-parser`), then pretty-prints it through a `Printer` that handles indentation, keyword casing, comment preservation, and ignore directives.

## Public API (`lib.rs`)

- **`format_sql(source, config) -> Result<String, FormatError>`** -- Format a pure-SQL string.
- **`format_document(source, config) -> Result<String, FormatError>`** -- Format a document that may contain dot commands (`.open`, etc.). Dot-command lines are preserved verbatim; SQL regions between them are formatted individually.
- **`check_formatted(source, config) -> Result<bool, FormatError>`** -- Returns `true` if the source already matches formatted output (useful for CI checks).

`FormatError` has two variants: `ParseError(Vec<ParseError>)` and `IoError`.

## FormatConfig (`config.rs`)

Loaded from TOML (`solite-fmt.toml` searched up from cwd, then `~/.config/solite/fmt.toml`). All fields have defaults via `#[serde(default)]`.

| Field                       | Type                      | Default      |
|-----------------------------|---------------------------|--------------|
| `keyword_case`              | `Upper` / `Lower` / `Preserve` | `Lower` |
| `indent_style`              | `Spaces` / `Tabs`         | `Spaces`     |
| `indent_size`               | `usize`                   | `2`          |
| `line_width`                | `usize`                   | `80`         |
| `comma_position`            | `Trailing` / `Leading`    | `Trailing`   |
| `logical_operator_position` | `Before` / `After`        | `Before`     |
| `statement_separator_lines` | `usize`                   | `2`          |

Key methods: `FormatConfig::discover()`, `FormatConfig::load(path)`, `indent_string()`.

## Printer (`printer.rs`)

The `Printer` struct accumulates formatted output. It tracks indentation level, current line length, and line-start state. Core methods:

- `write(text)` -- write text, auto-prepending indentation at line starts.
- `write_raw(text)` -- write without indentation processing (used for ignored regions).
- `keyword(kw)` -- write a keyword with case transformation per config.
- `newline()`, `space()`, `indent()`, `dedent()` -- whitespace control.
- `list_separator(multiline)` -- comma + newline or comma + space, respecting `comma_position`.
- `logical_operator(op)` -- AND/OR with position per `logical_operator_position`.
- `emit_leading_comments(span_start)`, `emit_trailing_comments(span_end)` -- emit attached comments.
- `would_exceed_line_width(additional)` / `should_multiline_list(items)` -- line-width heuristics.
- `finish()` -- consume the printer, trim trailing whitespace per line, ensure trailing newline.
- `format_program(program)` -- top-level entry: iterates statements, inserts separator blank lines, checks ignore directives, calls `stmt.format(self)`.

## FormatNode trait and format modules (`format/`)

`FormatNode` is the trait all AST nodes implement: `fn format(&self, p: &mut Printer)`.

- **`format/mod.rs`** -- trait definition plus helpers: `format_qualified_name`, `format_identifier` (with quoting logic), `format_list`, `format_alias`.
- **`format/stmt.rs`** -- `FormatNode` impls for `Statement`, `Select`, `Insert`, `Update`, `Delete`, transaction statements, etc.
- **`format/clause.rs`** -- `FormatNode` impls for clauses: `FROM`, `WHERE`, `GROUP BY`, `ORDER BY`, `LIMIT`, `JOIN`, `RETURNING`, CTEs.
- **`format/expr.rs`** -- `FormatNode` impls for expressions: binary ops, unary ops, function calls, `CASE`, `CAST`, subqueries, `IN` lists, `BETWEEN`, etc.
- **`format/ddl.rs`** -- `FormatNode` impls for DDL: `CREATE TABLE`, `CREATE INDEX`, `CREATE VIEW`, `CREATE TRIGGER`, `ALTER TABLE`, `DROP`, column defs, constraints.

## Comment preservation (`comment.rs`)

`CommentMap::from_source(source)` lexes the source and builds two maps:

- `leading: HashMap<span_start, Vec<Comment>>` -- comments before a code token (on a preceding line).
- `trailing: HashMap<span_end, Vec<Comment>>` -- comments after a code token (same line).

Comments are attached by proximity to the nearest non-comment token. The `Printer` emits them via `emit_leading_comments` / `emit_trailing_comments` during formatting.

## Ignore directives (`ignore.rs`)

`IgnoreDirectives::parse(source)` scans for special comment directives:

- `-- solite-fmt: off` / `-- solite-fmt: on` -- mark regions to skip formatting (preserved verbatim). Unclosed regions extend to EOF.
- `-- solite-fmt-ignore` or `-- solite-fmt: ignore` -- skip formatting for the next statement.

The `Printer` checks `overlaps_ignored_region(start, end)` during `format_program` and uses `write_raw` to preserve original source in ignored regions.

## File layout

```
src/
  lib.rs              -- public API: format_sql, format_document, check_formatted
  config.rs           -- FormatConfig, enums (KeywordCase, IndentStyle, etc.), TOML loading
  printer.rs          -- Printer struct: output buffer, indentation, keyword casing
  comment.rs          -- CommentMap: lexer-based comment extraction and attachment
  ignore.rs           -- IgnoreDirectives: off/on regions, per-statement ignore
  format/
    mod.rs            -- FormatNode trait, identifier quoting, list/alias helpers
    stmt.rs           -- Statement-level formatting
    clause.rs         -- Clause formatting (FROM, WHERE, JOIN, etc.)
    expr.rs           -- Expression formatting
    ddl.rs            -- DDL formatting (CREATE, ALTER, DROP)
  tests.rs            -- Snapshot tests (insta)
  snapshots/          -- insta snapshot files
```

## Testing

Uses `insta` snapshot tests. The `snapshot()` helper in `tests.rs` renders both input and output for easy comparison. Run with `cargo test -p solite-fmt` and update snapshots with `cargo insta review`.
