use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Documentation comment with optional tags.
///
/// Used for sqlite-docs style documentation:
/// - `--!` for table-level docs
/// - `---` for column-level docs
///
/// ## Example
///
/// ```sql
/// CREATE TABLE students (
///   --! All students at Foo University.
///   --! @details https://foo.edu/students
///
///   --- Student ID assigned at orientation
///   --- @example 'S10483'
///   student_id TEXT PRIMARY KEY
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DocComment {
    /// The main description text.
    pub description: String,
    /// Tags and their values (e.g., `@example` -> vec!["'S10483'"])
    pub tags: HashMap<String, Vec<String>>,
}

impl DocComment {
    /// Create a new empty doc comment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a doc comment with just a description.
    pub fn with_description(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            tags: HashMap::new(),
        }
    }

    /// Check if the doc comment is empty.
    pub fn is_empty(&self) -> bool {
        self.description.is_empty() && self.tags.is_empty()
    }

    /// Add a tag value to this doc comment.
    pub fn add_tag(&mut self, tag: impl Into<String>, value: impl Into<String>) {
        let tag = tag.into();
        let value = value.into();
        self.tags.entry(tag).or_default().push(value);
    }

    /// Get the first value of a tag, if present.
    pub fn get_tag(&self, tag: &str) -> Option<&str> {
        self.tags.get(tag).and_then(|v| v.first().map(|s| s.as_str()))
    }

    /// Get all values of a tag.
    pub fn get_tag_values(&self, tag: &str) -> Option<&[String]> {
        self.tags.get(tag).map(|v| v.as_slice())
    }
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

impl From<std::ops::Range<usize>> for Span {
    fn from(range: std::ops::Range<usize>) -> Self {
        Self {
            start: range.start,
            end: range.end,
        }
    }
}

/// A SQL program is a sequence of statements
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Statement>,
}

/// SQL statements
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    // DML
    Select(SelectStmt),
    Insert(InsertStmt),
    Update(UpdateStmt),
    Delete(DeleteStmt),

    // DDL
    CreateTable(CreateTableStmt),
    CreateIndex(CreateIndexStmt),
    CreateView(CreateViewStmt),
    CreateTrigger(CreateTriggerStmt),
    AlterTable(AlterTableStmt),
    DropTable(DropTableStmt),
    DropIndex(DropIndexStmt),
    DropView(DropViewStmt),
    DropTrigger(DropTriggerStmt),

    /// EXPLAIN [QUERY PLAN] statement
    Explain {
        query_plan: bool,
        stmt: Box<Statement>,
        span: Span,
    },

    /// CREATE VIRTUAL TABLE statement
    CreateVirtualTable(CreateVirtualTableStmt),

    // TCL (Transaction Control)
    Begin(BeginStmt),
    Commit(CommitStmt),
    Rollback(RollbackStmt),
    Savepoint(SavepointStmt),
    Release(ReleaseStmt),

    // Database management
    Vacuum(VacuumStmt),
    Analyze(AnalyzeStmt),
    Reindex(ReindexStmt),
    Attach(AttachStmt),
    Detach(DetachStmt),
    Pragma(PragmaStmt),
}

// ========================================
// INSERT Statement
// ========================================

/// INSERT [OR conflict_action] INTO [schema.]table [(columns)] source [RETURNING ...]
#[derive(Debug, Clone, PartialEq)]
pub struct InsertStmt {
    pub with_clause: Option<WithClause>,
    /// REPLACE, INSERT OR REPLACE, INSERT OR ABORT, etc.
    pub or_action: Option<ConflictAction>,
    pub schema: Option<String>,
    pub table_name: String,
    pub alias: Option<String>,
    pub columns: Option<Vec<String>>,
    pub source: InsertSource,
    pub upsert: Option<UpsertClause>,
    pub returning: Option<Vec<ResultColumn>>,
    pub span: Span,
}

/// Source for INSERT: VALUES, SELECT, or DEFAULT VALUES
#[derive(Debug, Clone, PartialEq)]
pub enum InsertSource {
    /// VALUES (row1), (row2), ...
    Values(Vec<Vec<Expr>>),
    /// SELECT ...
    Select(Box<SelectStmt>),
    /// DEFAULT VALUES
    DefaultValues,
}

/// ON CONFLICT clause (UPSERT)
#[derive(Debug, Clone, PartialEq)]
pub struct UpsertClause {
    pub target: Option<ConflictTarget>,
    pub action: ConflictAction,
    pub update_set: Option<Vec<(Vec<String>, Expr)>>,
    pub update_where: Option<Expr>,
    pub span: Span,
}

/// Conflict target for ON CONFLICT: (columns) [WHERE expr]
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictTarget {
    pub columns: Vec<IndexedColumn>,
    pub where_clause: Option<Expr>,
    pub span: Span,
}

/// Indexed column for conflict target: column [COLLATE collation] [ASC|DESC]
#[derive(Debug, Clone, PartialEq)]
pub struct IndexedColumn {
    pub column: Expr,
    pub collation: Option<String>,
    pub direction: Option<OrderDirection>,
    pub span: Span,
}

/// Conflict action for INSERT OR / ON CONFLICT DO
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictAction {
    Rollback,
    Abort,
    Fail,
    Ignore,
    Replace,
    /// DO NOTHING (only in ON CONFLICT)
    Nothing,
    /// DO UPDATE (only in ON CONFLICT)
    Update,
}

// ========================================
// UPDATE Statement
// ========================================

/// UPDATE [OR conflict] [schema.]table SET assignments [FROM ...] [WHERE ...] [RETURNING ...]
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateStmt {
    pub with_clause: Option<WithClause>,
    pub or_action: Option<ConflictAction>,
    pub schema: Option<String>,
    pub table_name: String,
    pub alias: Option<String>,
    pub indexed: Option<IndexedBy>,
    pub assignments: Vec<UpdateAssignment>,
    /// FROM clause for UPDATE ... FROM (SQLite extension)
    pub from: Option<FromClause>,
    pub where_clause: Option<Expr>,
    pub returning: Option<Vec<ResultColumn>>,
    /// ORDER BY clause (requires SQLITE_ENABLE_UPDATE_DELETE_LIMIT)
    pub order_by: Option<Vec<OrderingTerm>>,
    /// LIMIT clause (requires SQLITE_ENABLE_UPDATE_DELETE_LIMIT)
    pub limit: Option<Expr>,
    /// OFFSET clause
    pub offset: Option<Expr>,
    pub span: Span,
}

/// Assignment in UPDATE SET: column = expr or (col1, col2) = (expr1, expr2)
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateAssignment {
    pub columns: Vec<String>,
    pub expr: Expr,
    pub span: Span,
}

// ========================================
// DELETE Statement
// ========================================

/// DELETE FROM [schema.]table [INDEXED BY ... | NOT INDEXED] [WHERE ...] [RETURNING ...]
#[derive(Debug, Clone, PartialEq)]
pub struct DeleteStmt {
    pub with_clause: Option<WithClause>,
    pub schema: Option<String>,
    pub table_name: String,
    pub alias: Option<String>,
    pub indexed: Option<IndexedBy>,
    pub where_clause: Option<Expr>,
    pub returning: Option<Vec<ResultColumn>>,
    /// ORDER BY clause (requires SQLITE_ENABLE_UPDATE_DELETE_LIMIT)
    pub order_by: Option<Vec<OrderingTerm>>,
    /// LIMIT clause (requires SQLITE_ENABLE_UPDATE_DELETE_LIMIT)
    pub limit: Option<Expr>,
    /// OFFSET clause
    pub offset: Option<Expr>,
    pub span: Span,
}

/// DISTINCT or ALL modifier for SELECT
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DistinctAll {
    #[default]
    All,
    Distinct,
}

/// Result column in SELECT: expr [AS alias] | * | table.*
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum ResultColumn {
    /// expr [AS alias]
    Expr {
        expr: Expr,
        alias: Option<String>,
        /// True if alias uses explicit AS keyword
        alias_has_as: bool,
        span: Span,
    },
    /// * - all columns
    Star(Span),
    /// table.* - all columns from a table
    TableStar {
        table: String,
        span: Span,
    },
}

impl ResultColumn {
    pub fn span(&self) -> &Span {
        match self {
            ResultColumn::Expr { span, .. } => span,
            ResultColumn::Star(span) => span,
            ResultColumn::TableStar { span, .. } => span,
        }
    }
}

/// Compound operator: UNION, UNION ALL, INTERSECT, EXCEPT
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundOp {
    Union,
    UnionAll,
    Intersect,
    Except,
}

/// WITH clause for Common Table Expressions (CTEs)
#[derive(Debug, Clone, PartialEq)]
pub struct WithClause {
    pub recursive: bool,
    pub ctes: Vec<CommonTableExpr>,
    pub span: Span,
}

/// Common Table Expression: name [(columns)] AS [MATERIALIZED|NOT MATERIALIZED] (select)
#[derive(Debug, Clone, PartialEq)]
pub struct CommonTableExpr {
    pub name: String,
    pub columns: Option<Vec<String>>,
    pub materialized: Option<Materialized>,
    pub select: Box<SelectStmt>,
    pub span: Span,
}

/// MATERIALIZED hint for CTEs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Materialized {
    Materialized,
    NotMaterialized,
}

/// [WITH clause] SELECT [DISTINCT|ALL] columns [FROM tables] [WHERE expr] [GROUP BY exprs] [HAVING expr]
/// [compound_op SELECT ...]* [ORDER BY ordering] [LIMIT expr [OFFSET expr]]
#[derive(Debug, Clone, PartialEq)]
pub struct SelectStmt {
    pub with_clause: Option<WithClause>,
    pub distinct: DistinctAll,
    pub columns: Vec<ResultColumn>,
    pub from: Option<FromClause>,
    pub where_clause: Option<Expr>,
    pub group_by: Option<Vec<Expr>>,
    pub having: Option<Expr>,
    /// Compound operations (UNION, INTERSECT, EXCEPT)
    pub compounds: Vec<(CompoundOp, SelectCore)>,
    pub order_by: Option<Vec<OrderingTerm>>,
    pub limit: Option<LimitClause>,
    pub span: Span,
}

/// Core SELECT without ORDER BY / LIMIT (used in compound operations)
#[derive(Debug, Clone, PartialEq)]
pub struct SelectCore {
    pub distinct: DistinctAll,
    pub columns: Vec<ResultColumn>,
    pub from: Option<FromClause>,
    pub where_clause: Option<Expr>,
    pub group_by: Option<Vec<Expr>>,
    pub having: Option<Expr>,
    pub span: Span,
}

/// FROM clause: table references with optional JOINs
#[derive(Debug, Clone, PartialEq)]
pub struct FromClause {
    pub tables: Vec<TableOrSubquery>,
    pub span: Span,
}

/// Table or subquery in FROM clause
#[derive(Debug, Clone, PartialEq)]
pub enum TableOrSubquery {
    /// [schema.]table [AS alias] [INDEXED BY index | NOT INDEXED]
    Table {
        schema: Option<String>,
        name: String,
        alias: Option<String>,
        /// True if alias uses explicit AS keyword
        alias_has_as: bool,
        indexed: Option<IndexedBy>,
        span: Span,
    },
    /// (subquery) [AS alias]
    Subquery {
        query: Box<SelectStmt>,
        alias: Option<String>,
        /// True if alias uses explicit AS keyword
        alias_has_as: bool,
        span: Span,
    },
    /// (table_or_subquery, ...)
    TableList {
        tables: Vec<TableOrSubquery>,
        span: Span,
    },
    /// table_or_subquery JOIN table_or_subquery ON expr
    Join {
        left: Box<TableOrSubquery>,
        join_type: JoinType,
        right: Box<TableOrSubquery>,
        constraint: Option<JoinConstraint>,
        span: Span,
    },
}

/// INDEXED BY index_name | NOT INDEXED
#[derive(Debug, Clone, PartialEq)]
pub enum IndexedBy {
    Index(String),
    NotIndexed,
}

/// JOIN type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JoinType {
    #[default]
    Inner,
    Left,
    Right,
    Full,
    Cross,
    Natural,
    NaturalLeft,
    NaturalRight,
    NaturalFull,
}

/// JOIN constraint: ON expr | USING (col, ...)
#[derive(Debug, Clone, PartialEq)]
pub enum JoinConstraint {
    On(Expr),
    Using(Vec<String>),
}

/// ORDER BY term: expr [ASC|DESC] [NULLS FIRST|NULLS LAST]
#[derive(Debug, Clone, PartialEq)]
pub struct OrderingTerm {
    pub expr: Expr,
    pub direction: Option<OrderDirection>,
    pub nulls: Option<NullsOrder>,
    pub span: Span,
}

/// ASC or DESC
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDirection {
    Asc,
    Desc,
}

/// NULLS FIRST or NULLS LAST
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NullsOrder {
    First,
    Last,
}

/// LIMIT expr [OFFSET expr] or LIMIT expr, expr
#[derive(Debug, Clone, PartialEq)]
pub struct LimitClause {
    pub limit: Expr,
    pub offset: Option<Expr>,
    pub span: Span,
}

/// Simple table reference (for backwards compatibility)
#[derive(Debug, Clone, PartialEq)]
pub struct TableRef {
    pub schema: Option<String>,
    pub name: String,
    pub span: Span,
}

/// CREATE [TEMP|TEMPORARY] TABLE [IF NOT EXISTS] [schema.]table_name
/// (column_def, ... [, table_constraint, ...]) [table_options]
/// OR: CREATE ... TABLE ... AS select_stmt
#[derive(Debug, Clone, PartialEq)]
pub struct CreateTableStmt {
    pub temporary: bool,
    pub if_not_exists: bool,
    pub schema: Option<String>,
    pub table_name: String,
    /// Column definitions (empty if AS SELECT)
    pub columns: Vec<ColumnDef>,
    /// Table-level constraints (PRIMARY KEY, UNIQUE, CHECK, FOREIGN KEY)
    pub table_constraints: Vec<TableConstraint>,
    /// Table options (WITHOUT ROWID, STRICT)
    pub table_options: Vec<TableOption>,
    /// CREATE TABLE ... AS SELECT
    pub as_select: Option<Box<SelectStmt>>,
    /// Table-level documentation from `--!` comments
    pub doc: Option<DocComment>,
    pub span: Span,
}

/// Table-level constraints
#[derive(Debug, Clone, PartialEq)]
pub enum TableConstraint {
    /// PRIMARY KEY (indexed_columns) [conflict_clause]
    PrimaryKey {
        name: Option<String>,
        columns: Vec<IndexedColumn>,
        conflict: Option<ConflictAction>,
        span: Span,
    },
    /// UNIQUE (indexed_columns) [conflict_clause]
    Unique {
        name: Option<String>,
        columns: Vec<IndexedColumn>,
        conflict: Option<ConflictAction>,
        span: Span,
    },
    /// CHECK (expr)
    Check {
        name: Option<String>,
        expr: Expr,
        span: Span,
    },
    /// FOREIGN KEY (columns) REFERENCES table [(columns)] [actions]
    ForeignKey {
        name: Option<String>,
        columns: Vec<String>,
        foreign_table: String,
        foreign_columns: Option<Vec<String>>,
        on_delete: Option<ForeignKeyAction>,
        on_update: Option<ForeignKeyAction>,
        deferrable: Option<Deferrable>,
        span: Span,
    },
}

impl TableConstraint {
    pub fn span(&self) -> &Span {
        match self {
            TableConstraint::PrimaryKey { span, .. } => span,
            TableConstraint::Unique { span, .. } => span,
            TableConstraint::Check { span, .. } => span,
            TableConstraint::ForeignKey { span, .. } => span,
        }
    }
}

/// Deferrable constraint setting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deferrable {
    /// DEFERRABLE INITIALLY DEFERRED
    InitiallyDeferred,
    /// DEFERRABLE INITIALLY IMMEDIATE
    InitiallyImmediate,
    /// NOT DEFERRABLE
    NotDeferrable,
}

/// Table options (WITHOUT ROWID, STRICT)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableOption {
    WithoutRowid,
    Strict,
}

// ========================================
// CREATE TRIGGER Statement
// ========================================

/// CREATE [TEMP] TRIGGER [IF NOT EXISTS] [schema.]name
/// [BEFORE|AFTER|INSTEAD OF] {DELETE|INSERT|UPDATE [OF columns]}
/// ON table [FOR EACH ROW] [WHEN expr]
/// BEGIN statements END
#[derive(Debug, Clone, PartialEq)]
pub struct CreateTriggerStmt {
    pub temporary: bool,
    pub if_not_exists: bool,
    pub schema: Option<String>,
    pub trigger_name: String,
    pub timing: TriggerTiming,
    pub event: TriggerEvent,
    pub table_name: String,
    pub for_each_row: bool,
    pub when_clause: Option<Expr>,
    pub body: Vec<Statement>,
    pub span: Span,
}

/// Trigger timing: BEFORE, AFTER, or INSTEAD OF
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerTiming {
    Before,
    After,
    InsteadOf,
}

/// Trigger event: DELETE, INSERT, or UPDATE [OF columns]
#[derive(Debug, Clone, PartialEq)]
pub enum TriggerEvent {
    Delete,
    Insert,
    Update { columns: Option<Vec<String>> },
}

// ========================================
// CREATE VIRTUAL TABLE Statement
// ========================================

/// CREATE VIRTUAL TABLE [IF NOT EXISTS] [schema.]table_name USING module_name [(args)]
#[derive(Debug, Clone, PartialEq)]
pub struct CreateVirtualTableStmt {
    pub if_not_exists: bool,
    pub schema: Option<String>,
    pub table_name: String,
    pub module_name: String,
    /// Module arguments as raw strings (module-specific syntax)
    pub module_args: Option<Vec<String>>,
    pub span: Span,
}

// ========================================
// DROP Statements
// ========================================

/// DROP TABLE [IF EXISTS] [schema.]table_name
#[derive(Debug, Clone, PartialEq)]
pub struct DropTableStmt {
    pub if_exists: bool,
    pub schema: Option<String>,
    pub table_name: String,
    pub span: Span,
}

/// DROP INDEX [IF EXISTS] [schema.]index_name
#[derive(Debug, Clone, PartialEq)]
pub struct DropIndexStmt {
    pub if_exists: bool,
    pub schema: Option<String>,
    pub index_name: String,
    pub span: Span,
}

/// DROP VIEW [IF EXISTS] [schema.]view_name
#[derive(Debug, Clone, PartialEq)]
pub struct DropViewStmt {
    pub if_exists: bool,
    pub schema: Option<String>,
    pub view_name: String,
    pub span: Span,
}

/// DROP TRIGGER [IF EXISTS] [schema.]trigger_name
#[derive(Debug, Clone, PartialEq)]
pub struct DropTriggerStmt {
    pub if_exists: bool,
    pub schema: Option<String>,
    pub trigger_name: String,
    pub span: Span,
}

// ========================================
// CREATE INDEX Statement
// ========================================

/// CREATE [UNIQUE] INDEX [IF NOT EXISTS] [schema.]index ON table (columns) [WHERE expr]
#[derive(Debug, Clone, PartialEq)]
pub struct CreateIndexStmt {
    pub unique: bool,
    pub if_not_exists: bool,
    pub schema: Option<String>,
    pub index_name: String,
    pub table_name: String,
    pub columns: Vec<IndexedColumn>,
    pub where_clause: Option<Expr>,
    pub span: Span,
}

// ========================================
// CREATE VIEW Statement
// ========================================

/// CREATE [TEMP|TEMPORARY] VIEW [IF NOT EXISTS] [schema.]view [(columns)] AS select
#[derive(Debug, Clone, PartialEq)]
pub struct CreateViewStmt {
    pub temporary: bool,
    pub if_not_exists: bool,
    pub schema: Option<String>,
    pub view_name: String,
    pub columns: Option<Vec<String>>,
    pub select: Box<SelectStmt>,
    pub span: Span,
}

// ========================================
// ALTER TABLE Statement
// ========================================

/// ALTER TABLE [schema.]table action
#[derive(Debug, Clone, PartialEq)]
pub struct AlterTableStmt {
    pub schema: Option<String>,
    pub table_name: String,
    pub action: AlterTableAction,
    pub span: Span,
}

/// ALTER TABLE action
#[derive(Debug, Clone, PartialEq)]
pub enum AlterTableAction {
    /// RENAME TO new_name
    RenameTo(String),
    /// RENAME [COLUMN] old_name TO new_name
    RenameColumn { old_name: String, new_name: String },
    /// ADD [COLUMN] column_def
    AddColumn(ColumnDef),
    /// DROP [COLUMN] column_name
    DropColumn(String),
}

// ========================================
// TCL (Transaction Control) Statements
// ========================================

/// Transaction type for BEGIN statement
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionType {
    Deferred,
    Immediate,
    Exclusive,
}

/// BEGIN [DEFERRED|IMMEDIATE|EXCLUSIVE] [TRANSACTION]
#[derive(Debug, Clone, PartialEq)]
pub struct BeginStmt {
    pub transaction_type: Option<TransactionType>,
    pub span: Span,
}

/// COMMIT [TRANSACTION] | END [TRANSACTION]
#[derive(Debug, Clone, PartialEq)]
pub struct CommitStmt {
    pub span: Span,
}

/// ROLLBACK [TRANSACTION] [TO [SAVEPOINT] savepoint_name]
#[derive(Debug, Clone, PartialEq)]
pub struct RollbackStmt {
    pub savepoint: Option<String>,
    pub span: Span,
}

/// SAVEPOINT savepoint_name
#[derive(Debug, Clone, PartialEq)]
pub struct SavepointStmt {
    pub name: String,
    pub span: Span,
}

/// RELEASE [SAVEPOINT] savepoint_name
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseStmt {
    pub name: String,
    pub span: Span,
}

// ========================================
// Database Management Statements
// ========================================

/// VACUUM [schema_name] [INTO filename]
#[derive(Debug, Clone, PartialEq)]
pub struct VacuumStmt {
    pub schema: Option<String>,
    pub into_file: Option<String>,
    pub span: Span,
}

/// ANALYZE [schema_name | table_or_index_name | schema_name.table_or_index_name]
#[derive(Debug, Clone, PartialEq)]
pub struct AnalyzeStmt {
    pub target: Option<QualifiedName>,
    pub span: Span,
}

/// REINDEX [collation_name | [schema.]table_or_index_name]
#[derive(Debug, Clone, PartialEq)]
pub struct ReindexStmt {
    pub target: Option<QualifiedName>,
    pub span: Span,
}

/// ATTACH [DATABASE] expr AS schema_name
#[derive(Debug, Clone, PartialEq)]
pub struct AttachStmt {
    pub expr: Expr,
    pub schema_name: String,
    pub span: Span,
}

/// DETACH [DATABASE] schema_name
#[derive(Debug, Clone, PartialEq)]
pub struct DetachStmt {
    pub schema_name: String,
    pub span: Span,
}

/// PRAGMA [schema.]pragma_name [= value | (value)]
#[derive(Debug, Clone, PartialEq)]
pub struct PragmaStmt {
    pub schema: Option<String>,
    pub name: String,
    pub value: Option<PragmaValue>,
    pub span: Span,
}

/// Pragma value: either assigned (=) or called with parens
#[derive(Debug, Clone, PartialEq)]
pub enum PragmaValue {
    /// pragma_name = value
    Assign(Expr),
    /// pragma_name(value)
    Call(Expr),
}

/// Qualified name: [schema.]name
#[derive(Debug, Clone, PartialEq)]
pub struct QualifiedName {
    pub schema: Option<String>,
    pub name: String,
    pub span: Span,
}

/// Column definition: name [type] [constraints...]
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef {
    pub name: String,
    pub type_name: Option<String>,
    pub constraints: Vec<ColumnConstraint>,
    /// Column-level documentation from `---` comments
    pub doc: Option<DocComment>,
    pub span: Span,
}

/// Column constraint
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnConstraint {
    /// PRIMARY KEY [ASC|DESC] [conflict-clause] [AUTOINCREMENT]
    PrimaryKey {
        order: Option<OrderDirection>,
        conflict: Option<ConflictAction>,
        autoincrement: bool,
        span: Span,
    },
    /// NOT NULL [conflict-clause]
    NotNull {
        conflict: Option<ConflictAction>,
        span: Span,
    },
    /// UNIQUE [conflict-clause]
    Unique {
        conflict: Option<ConflictAction>,
        span: Span,
    },
    /// CHECK (expr)
    Check {
        expr: Expr,
        span: Span,
    },
    /// DEFAULT (expr) | literal | signed-number
    Default {
        value: DefaultValue,
        span: Span,
    },
    /// COLLATE collation-name
    Collate {
        collation: String,
        span: Span,
    },
    /// REFERENCES table [(columns)] [on-delete] [on-update] [match] [deferrable]
    ForeignKey {
        foreign_table: String,
        columns: Option<Vec<String>>,
        on_delete: Option<ForeignKeyAction>,
        on_update: Option<ForeignKeyAction>,
        span: Span,
    },
    /// GENERATED ALWAYS AS (expr) [STORED|VIRTUAL]
    Generated {
        expr: Expr,
        stored: bool,
        span: Span,
    },
}

/// Default value for column
#[derive(Debug, Clone, PartialEq)]
pub enum DefaultValue {
    /// Literal value (number, string, blob, NULL, TRUE, FALSE, CURRENT_TIME, etc.)
    Literal(Expr),
    /// Parenthesized expression
    Expr(Expr),
}

/// Foreign key action (ON DELETE / ON UPDATE)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignKeyAction {
    SetNull,
    SetDefault,
    Cascade,
    Restrict,
    NoAction,
}

/// Type name with optional size parameters: VARCHAR(255), DECIMAL(10,2)
#[derive(Debug, Clone, PartialEq)]
pub struct TypeName {
    pub name: String,
    pub args: Option<(i64, Option<i64>)>,
    pub span: Span,
}

// ========================================
// Window Function Types
// ========================================

/// Window specification for window functions
#[derive(Debug, Clone, PartialEq)]
pub struct WindowSpec {
    pub base_window: Option<String>,
    pub partition_by: Option<Vec<Expr>>,
    pub order_by: Option<Vec<OrderingTerm>>,
    pub frame: Option<FrameSpec>,
    pub span: Span,
}

/// Frame specification: ROWS/RANGE/GROUPS BETWEEN start AND end
#[derive(Debug, Clone, PartialEq)]
pub struct FrameSpec {
    pub unit: FrameUnit,
    pub start: FrameBound,
    pub end: Option<FrameBound>,
    pub exclude: Option<FrameExclude>,
    pub span: Span,
}

/// Frame unit: ROWS, RANGE, or GROUPS
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameUnit {
    Rows,
    Range,
    Groups,
}

/// Frame bound: UNBOUNDED PRECEDING, n PRECEDING, CURRENT ROW, n FOLLOWING, UNBOUNDED FOLLOWING
#[derive(Debug, Clone, PartialEq)]
pub enum FrameBound {
    UnboundedPreceding,
    Preceding(Box<Expr>),
    CurrentRow,
    Following(Box<Expr>),
    UnboundedFollowing,
}

/// Frame exclude clause
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameExclude {
    NoOthers,
    CurrentRow,
    Group,
    Ties,
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    // Arithmetic (precedence 3-4)
    Add,
    Sub,
    Mul,
    Div,
    Mod,

    // String concatenation (precedence 2)
    Concat,

    // Comparison (precedence 6-7)
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Is,
    IsNot,

    // Bitwise (precedence 5)
    BitAnd,
    BitOr,
    LShift,
    RShift,

    // Logical (precedence 10-11)
    And,
    Or,

    // Pattern matching (precedence 7)
    Like,
    Glob,
    Regexp,
    Match,

    // JSON operators (highest precedence, like member access)
    /// -> JSON extract (returns JSON)
    JsonExtract,
    /// ->> JSON extract text (returns SQL text)
    JsonExtractText,
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// Unary minus: -x
    Neg,
    /// Unary plus: +x
    Pos,
    /// Logical NOT
    Not,
    /// Bitwise NOT: ~x
    BitNot,
}

/// SQL expressions
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Integer literal: 123
    Integer(i64, Span),
    /// Hex integer literal: 0x1F
    HexInteger(i64, Span),
    /// Float literal: 1.5
    Float(f64, Span),
    /// String literal: 'hello'
    String(String, Span),
    /// Blob literal: X'AABBCCDD'
    Blob(Vec<u8>, Span),
    /// NULL
    Null(Span),
    /// Column/identifier reference
    /// The bool indicates if it was double-quoted ("identifier")
    Ident(String, bool, Span),
    /// Star: *
    Star(Span),
    /// Bind parameter: ?, ?1, :name, @name, $name
    BindParam(String, Span),

    /// Binary expression: left op right
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
        span: Span,
    },

    /// Unary expression: op expr
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        span: Span,
    },

    /// Parenthesized expression: (expr)
    Paren(Box<Expr>, Span),

    /// BETWEEN expression: expr [NOT] BETWEEN low AND high
    Between {
        expr: Box<Expr>,
        low: Box<Expr>,
        high: Box<Expr>,
        negated: bool,
        span: Span,
    },

    /// IN expression with list: expr [NOT] IN (value, ...)
    InList {
        expr: Box<Expr>,
        list: Vec<Expr>,
        negated: bool,
        span: Span,
    },

    /// IN expression with subquery: expr [NOT] IN (SELECT ...)
    InSelect {
        expr: Box<Expr>,
        query: Box<SelectStmt>,
        negated: bool,
        span: Span,
    },

    /// Scalar subquery: (SELECT ...)
    Subquery {
        query: Box<SelectStmt>,
        span: Span,
    },

    /// EXISTS expression: [NOT] EXISTS (SELECT ...)
    Exists {
        query: Box<SelectStmt>,
        negated: bool,
        span: Span,
    },

    /// IS NULL / IS NOT NULL
    IsNull {
        expr: Box<Expr>,
        negated: bool,
        span: Span,
    },

    /// LIKE/GLOB/REGEXP/MATCH with optional ESCAPE
    Like {
        expr: Box<Expr>,
        pattern: Box<Expr>,
        escape: Option<Box<Expr>>,
        op: BinaryOp, // Like, Glob, Regexp, or Match
        negated: bool,
        span: Span,
    },

    /// CASE expression
    Case {
        operand: Option<Box<Expr>>,
        when_clauses: Vec<(Expr, Expr)>,
        else_clause: Option<Box<Expr>>,
        span: Span,
    },

    /// CAST expression: CAST(expr AS type)
    Cast {
        expr: Box<Expr>,
        type_name: TypeName,
        span: Span,
    },

    /// Function call: name(args) [FILTER (WHERE expr)] [OVER window_spec]
    FunctionCall {
        name: String,
        args: Vec<Expr>,
        distinct: bool,
        filter: Option<Box<Expr>>,
        over: Option<WindowSpec>,
        span: Span,
    },

    /// Qualified column reference: [schema.][table.]column
    Column {
        schema: Option<String>,
        table: Option<String>,
        column: String,
        span: Span,
    },

    /// COLLATE expression
    Collate {
        expr: Box<Expr>,
        collation: String,
        span: Span,
    },

    /// RAISE function: RAISE(action [, message])
    /// Used in trigger bodies
    Raise {
        action: RaiseAction,
        message: Option<Box<Expr>>,
        span: Span,
    },
}

/// RAISE function action types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaiseAction {
    /// RAISE(IGNORE) - skip the current row
    Ignore,
    /// RAISE(ROLLBACK, message) - rollback entire transaction
    Rollback,
    /// RAISE(ABORT, message) - abort current statement
    Abort,
    /// RAISE(FAIL, message) - fail at point of error
    Fail,
}

impl Expr {
    pub fn span(&self) -> &Span {
        match self {
            Expr::Integer(_, span) => span,
            Expr::HexInteger(_, span) => span,
            Expr::Float(_, span) => span,
            Expr::String(_, span) => span,
            Expr::Blob(_, span) => span,
            Expr::Null(span) => span,
            Expr::Ident(_, _, span) => span,
            Expr::Star(span) => span,
            Expr::BindParam(_, span) => span,
            Expr::Binary { span, .. } => span,
            Expr::Unary { span, .. } => span,
            Expr::Paren(_, span) => span,
            Expr::Between { span, .. } => span,
            Expr::InList { span, .. } => span,
            Expr::InSelect { span, .. } => span,
            Expr::Subquery { span, .. } => span,
            Expr::Exists { span, .. } => span,
            Expr::IsNull { span, .. } => span,
            Expr::Like { span, .. } => span,
            Expr::Case { span, .. } => span,
            Expr::Cast { span, .. } => span,
            Expr::FunctionCall { span, .. } => span,
            Expr::Column { span, .. } => span,
            Expr::Collate { span, .. } => span,
            Expr::Raise { span, .. } => span,
        }
    }
}
