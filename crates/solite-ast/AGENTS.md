# solite-ast

Pure data-structure crate defining the typed AST for SQLite SQL. No parser, no codegen -- just the node types, enums, and spans that other crates produce and consume.

## File layout

Single source file: `src/lib.rs` (no submodules). `Cargo.toml` has zero external dependencies.

## Span tracking

Every AST node carries a `Span { start: usize, end: usize }` recording its byte range in the source text. `Span` implements `From<Range<usize>>`. Use the `.span()` helper on `Expr`, `ResultColumn`, and `TableConstraint` to get the span from any variant.

## `Program` and `Statement`

`Program` is a `Vec<Statement>`. `Statement` is the top-level enum with these variants:

### DML
- `Select(SelectStmt)` -- queries
- `Insert(InsertStmt)` -- insert/replace with upsert support
- `Update(UpdateStmt)` -- update with optional FROM, ORDER BY, LIMIT
- `Delete(DeleteStmt)` -- delete with optional ORDER BY, LIMIT

### DDL
- `CreateTable(CreateTableStmt)` -- columns, constraints, table options, or AS SELECT
- `CreateIndex(CreateIndexStmt)` -- unique flag, indexed columns, partial WHERE
- `CreateView(CreateViewStmt)` -- temp flag, optional column list, AS SELECT
- `CreateTrigger(CreateTriggerStmt)` -- timing, event, body (Vec<Statement>)
- `CreateVirtualTable(CreateVirtualTableStmt)` -- USING module with raw string args
- `AlterTable(AlterTableStmt)` -- action enum: RenameTo, RenameColumn, AddColumn, DropColumn
- `DropTable(DropTableStmt)`, `DropIndex(DropIndexStmt)`, `DropView(DropViewStmt)`, `DropTrigger(DropTriggerStmt)`

### Inline variant
- `Explain { query_plan: bool, stmt: Box<Statement>, span }` -- wraps any other statement

### TCL (Transaction Control)
- `Begin(BeginStmt)` -- optional `TransactionType` (Deferred/Immediate/Exclusive)
- `Commit(CommitStmt)`, `Rollback(RollbackStmt)` -- rollback has optional savepoint name
- `Savepoint(SavepointStmt)`, `Release(ReleaseStmt)`

### Database management
- `Vacuum(VacuumStmt)` -- optional INTO filename
- `Analyze(AnalyzeStmt)`, `Reindex(ReindexStmt)` -- optional `QualifiedName` target
- `Attach(AttachStmt)`, `Detach(DetachStmt)`
- `Pragma(PragmaStmt)` -- value is `PragmaValue::Assign(Expr)` or `PragmaValue::Call(Expr)`

## Key structs

### `SelectStmt`
Fields: `with_clause`, `distinct` (DistinctAll), `columns` (Vec<ResultColumn>), `from` (FromClause), `where_clause`, `group_by`, `having`, `compounds` (Vec<(CompoundOp, SelectCore)>), `order_by`, `limit` (LimitClause), `span`.

`SelectCore` is the same shape minus `with_clause`, `compounds`, `order_by`, and `limit` -- used for the right side of UNION/INTERSECT/EXCEPT.

### `CreateTableStmt`
Fields: `temporary`, `if_not_exists`, `schema`, `table_name`, `columns` (Vec<ColumnDef>), `table_constraints` (Vec<TableConstraint>), `table_options` (Vec<TableOption>: WithoutRowid, Strict), `as_select`, `doc` (DocComment), `span`.

### `ColumnDef`
Fields: `name`, `type_name` (Option<String>), `constraints` (Vec<ColumnConstraint>), `doc` (DocComment), `span`.

`ColumnConstraint` variants: PrimaryKey (with autoincrement), NotNull, Unique, Check, Default (DefaultValue), Collate, ForeignKey, Generated.

### `InsertStmt`
Fields: `with_clause`, `or_action` (ConflictAction), `schema`, `table_name`, `alias`, `columns`, `source` (InsertSource: Values/Select/DefaultValues), `upsert` (UpsertClause), `returning`, `span`.

### `UpdateStmt`
Fields: `with_clause`, `or_action`, `schema`, `table_name`, `alias`, `indexed`, `assignments` (Vec<UpdateAssignment>), `from`, `where_clause`, `returning`, `order_by`, `limit`, `offset`, `span`.

### `DeleteStmt`
Fields: `with_clause`, `schema`, `table_name`, `alias`, `indexed`, `where_clause`, `returning`, `order_by`, `limit`, `offset`, `span`.

### `FromClause` and `TableOrSubquery`
`FromClause` wraps `Vec<TableOrSubquery>`. `TableOrSubquery` variants: Table, Subquery, TableFunction, TableList, Join (recursive with JoinType and JoinConstraint).

`JoinType`: Inner, Left, Right, Full, Cross, Natural, NaturalLeft, NaturalRight, NaturalFull.

### `WithClause` / `CommonTableExpr`
`WithClause` has `recursive` flag and `ctes: Vec<CommonTableExpr>`. Each CTE has `name`, optional `columns`, optional `Materialized` hint, and a `select`.

## `Expr` enum

All expression variants, each carrying a `Span`:

| Variant | Fields | Notes |
|---------|--------|-------|
| `Integer(i64, Span)` | | |
| `HexInteger(i64, Span)` | | 0x prefix literals |
| `Float(f64, Span)` | | |
| `String(String, Span)` | | Single-quoted |
| `Blob(Vec<u8>, Span)` | | X'...' literals |
| `Null(Span)` | | |
| `Ident(String, bool, Span)` | | bool = was double-quoted |
| `Star(Span)` | | Bare `*` |
| `BindParam(String, Span)` | | ?, ?N, :name, @name, $name |
| `Binary { left, op, right, span }` | BinaryOp | Arithmetic, comparison, logical, JSON (->/->>), pattern matching |
| `Unary { op, expr, span }` | UnaryOp | Neg, Pos, Not, BitNot |
| `Paren(Box<Expr>, Span)` | | |
| `Between { expr, low, high, negated, span }` | | |
| `InList { expr, list, negated, span }` | | |
| `InSelect { expr, query, negated, span }` | | |
| `Subquery { query, span }` | | Scalar subquery |
| `Exists { query, negated, span }` | | |
| `IsNull { expr, negated, span }` | | IS NULL / IS NOT NULL |
| `Like { expr, pattern, escape, op, negated, span }` | | op is Like/Glob/Regexp/Match |
| `Case { operand, when_clauses, else_clause, span }` | | |
| `Cast { expr, type_name, span }` | TypeName | |
| `FunctionCall { name, args, distinct, filter, over, span }` | | Window functions via `over: Option<WindowSpec>` |
| `Column { schema, table, column, span }` | | Qualified column ref |
| `Collate { expr, collation, span }` | | |
| `Raise { action, message, span }` | RaiseAction | Trigger bodies only |

## `BinaryOp` variants
Arithmetic: Add, Sub, Mul, Div, Mod. String: Concat. Comparison: Eq, Ne, Lt, Le, Gt, Ge, Is, IsNot. Bitwise: BitAnd, BitOr, LShift, RShift. Logical: And, Or. Pattern: Like, Glob, Regexp, Match. JSON: JsonExtract (->), JsonExtractText (->>).

## `DocComment`
Used on `CreateTableStmt.doc` (from `--!` comments) and `ColumnDef.doc` (from `---` comments). Stores a `description` string and a `tags: HashMap<String, Vec<String>>` for `@tag value` annotations.

## Window functions
`WindowSpec` has `base_window`, `partition_by`, `order_by`, and `frame` (FrameSpec). `FrameSpec` uses `FrameUnit` (Rows/Range/Groups), `FrameBound` (UnboundedPreceding/Preceding/CurrentRow/Following/UnboundedFollowing), and `FrameExclude`.
