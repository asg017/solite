# solite-analyzer

Static analysis and symbol resolution for parsed SQL ASTs. Provides semantic diagnostics (unknown tables/columns, duplicate columns), configurable lint rules, and symbol resolution for LSP hover/goto-definition.

## File Layout

```
src/
  lib.rs          - Schema types, analyze/analyze_with_schema, lint_with_config, AST walkers
  symbols.rs      - Symbol resolution: scopes, find_symbol_at_offset, hover/goto
  rules/
    mod.rs         - LintRule trait, LintDiagnostic, Fix, RULES registry, walk_expr/walk_select_stmt
    config.rs      - LintConfig (TOML-based), RuleSeverity, config discovery
    suppressions.rs - Suppressions from `-- solite-ignore: rule1, rule2` comments
    empty_blob.rs  - EmptyBlobLiteral rule
    double_quoted.rs - DoubleQuotedString rule (fixable)
    missing_as.rs  - MissingAsAlias rule (fixable, checks both stmt and expr level)
```

## Key Types

### `Schema` (lib.rs)
Central schema registry. All lookups are case-insensitive (keys stored lowercase).

Fields (all private, accessed via methods):
- `tables: HashMap<String, TableInfo>` - lowercase name -> info
- `original_names: HashMap<String, String>` - lowercase -> original case
- `indexes: HashMap<String, IndexInfo>`
- `views: HashMap<String, ViewInfo>`
- `triggers: HashMap<String, TriggerInfo>`
- `functions: Vec<String>` - scalar function names
- `function_nargs: HashMap<String, Vec<i32>>` - name -> valid arg counts (-1 = variadic)

Key methods: `add_table`, `add_table_with_doc`, `add_view`, `add_index`, `add_trigger`, `merge`, `has_table`, `get_table`, `columns_for_table`, `columns_for_table_with_rowid`, `set_functions`, `set_function_nargs`, `function_nargs`.

### `TableInfo` (lib.rs)
- `columns: HashSet<String>` - lowercase column names
- `original_columns: Vec<String>` - preserves case, used for autocomplete
- `without_rowid: bool`
- `doc: Option<solite_ast::DocComment>` - `--!` table docs
- `column_docs: HashMap<String, solite_ast::DocComment>` - column-level docs

### `Diagnostic` (lib.rs)
- `message: String`, `span: Span`, `severity: Severity` (Error | Warning)
- Constructors: `Diagnostic::error(msg, span)`, `Diagnostic::warning(msg, span)`

### `ResolvedSymbol` (symbols.rs)
```rust
enum ResolvedSymbol {
    TableAlias { alias, table_name, definition_span },
    Column { name, table_name: Option, qualifier: Option },
    Table { name, span },
    ColumnAlias { alias, definition_span },
    Cte { name, definition_span, columns: Vec<String> },
}
```

### `StatementScope` (symbols.rs)
Tracks aliases within a single statement:
- `table_aliases: HashMap<String, TableAliasInfo>`
- `column_aliases: HashMap<String, ColumnAliasInfo>`
- `ctes: HashMap<String, CteInfo>`

## Key Functions

### Semantic Analysis (lib.rs)

- `analyze(program: &Program) -> Vec<Diagnostic>` - analyze without external schema
- `analyze_with_schema(program: &Program, external_schema: Option<&Schema>) -> Vec<Diagnostic>` - main entry point. Iterates statements sequentially, building a local `tables` map from CREATE TABLE statements encountered so far. For each SELECT, calls `analyze_select` which:
  1. Builds CTE tables from WITH clause
  2. Checks for unknown tables in FROM (CTE -> local -> external schema lookup order)
  3. Builds `ExprContext` with available tables/columns
  4. Validates column references in SELECT, WHERE, GROUP BY, HAVING, ORDER BY, JOIN ON
  5. Recursively handles subqueries and compound SELECTs
- `build_schema(program: &Program) -> Schema` - extracts DDL (CREATE TABLE/INDEX/VIEW/Trigger, DROP) into a Schema. Handles virtual tables (empty column set). Used by callers to build external schemas.
- `lint_with_config(program: &Program, source: &str, config: &LintConfig, _external_schema: Option<&Schema>) -> Vec<LintResult>` - runs all lint rules with suppression and config support

### Symbol Resolution (symbols.rs)

- `find_statement_at_offset(program: &Program, offset: usize) -> Option<&Statement>` - finds which statement contains a byte offset
- `find_symbol_at_offset(stmt: &Statement, source: &str, offset: usize, schema: Option<&Schema>) -> Option<(ResolvedSymbol, Span)>` - resolves the symbol at a byte offset. Supports SELECT, INSERT (source SELECT), UPDATE (WHERE), DELETE (WHERE).
- `build_scope_from_select(select: &SelectStmt, source: &str) -> StatementScope` - builds scope from FROM clause aliases, SELECT column aliases, and CTEs
- `format_hover_content(symbol: &ResolvedSymbol, schema: Option<&Schema>) -> String` - formats markdown hover text. Includes table docs, column docs, doc tags (details, source, schema, example, value).
- `get_definition_span(symbol: &ResolvedSymbol) -> Option<Span>` - returns definition location for aliases and CTEs; None for tables/columns (they live in schema, not in-document).

## Lint Rules System

### `LintRule` trait (rules/mod.rs)
```rust
trait LintRule: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn default_severity(&self) -> RuleSeverity;
    fn check_expr(&self, expr: &Expr, ctx: &LintContext) -> Vec<LintDiagnostic>;  // default: vec![]
    fn check_stmt(&self, stmt: &Statement, ctx: &LintContext) -> Vec<LintDiagnostic>;  // default: vec![]
    fn is_fixable(&self) -> bool;  // default: false
    fn fix(&self, diagnostic: &LintDiagnostic, source: &str) -> Option<Fix>;  // default: None
}
```

### Rule Registry
`static RULES: Lazy<Vec<Box<dyn LintRule>>>` - contains all rules. Access via `get_all_rules()` or `get_rule_by_id(id)`.

Current rules:
| ID | Struct | Level | Fixable | Checks |
|----|--------|-------|---------|--------|
| `empty-blob-literal` | `EmptyBlobLiteral` | Warning | No | `Expr::Blob` with empty bytes |
| `double-quoted-string` | `DoubleQuotedString` | Warning | Yes | `Expr::Ident` with `is_double_quoted` |
| `missing-as` | `MissingAsAlias` | Warning | Yes | `check_stmt` on SELECT column/table aliases missing AS |

### Configuration (rules/config.rs)
`LintConfig` loads from TOML (`solite-lint.toml`). Discovery order: walk up from cwd, then `~/.config/solite/lint.toml`, then defaults. Format:
```toml
[rules]
empty-blob-literal = "off"
double-quoted-string = "error"
```
Severities: `"off"`, `"warning"`/`"warn"`, `"error"`.

### Suppressions (rules/suppressions.rs)
`-- solite-ignore: rule1, rule2` suppresses rules on the *next* line. Uses `solite-lexer::lex` to find comment tokens.

### Adding a New Rule
1. Create `rules/my_rule.rs` implementing `LintRule`
2. Add `pub mod my_rule;` and `pub use my_rule::MyRule;` in `rules/mod.rs`
3. Add `Box::new(MyRule)` to the `RULES` lazy static

## AST Walking

Two separate walkers exist:
- `rules::walk_expr` / `rules::walk_select_stmt` (rules/mod.rs) - used by lint system, takes `LintContext`, traverses full AST including FROM clauses and window functions
- `walk_expr` / `walk_statement_exprs` (lib.rs, private) - used by `lint_with_config` to visit expressions in statements

## Integration Points

- **Depends on:** `solite-ast` (AST types: Program, Statement, Expr, Span, DocComment), `solite-lexer` (tokenization for suppressions)
- **Dev dependency:** `solite-parser` (for tests)
- **Used by:** LSP server (hover, goto-definition, diagnostics), notebook analysis, any consumer needing SQL validation against a schema
