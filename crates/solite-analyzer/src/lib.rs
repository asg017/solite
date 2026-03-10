pub mod rules;
pub mod symbols;

pub use rules::{Fix, LintConfig, LintDiagnostic, LintResult, LintRule, RuleSeverity, Suppressions};
pub use symbols::{
    find_statement_at_offset, find_symbol_at_offset, format_hover_content, get_definition_span,
    ResolvedSymbol, StatementScope,
};

use solite_ast::{CommonTableExpr, Expr, FromClause, JoinConstraint, Program, ResultColumn, SelectStmt, Span, Statement, TableOption, TableOrSubquery, TriggerEvent, WithClause};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub message: String,
    pub span: Span,
    pub severity: Severity,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            severity: Severity::Error,
        }
    }

    pub fn warning(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            severity: Severity::Warning,
        }
    }
}

/// Table info tracked during analysis
#[derive(Debug, Clone, Default)]
pub struct TableInfo {
    /// Column names (lowercase for case-insensitive lookup)
    pub columns: HashSet<String>,
    /// Original column names (preserves case for display in autocomplete)
    pub original_columns: Vec<String>,
    /// Whether this table was created with WITHOUT ROWID option
    pub without_rowid: bool,
    /// Table-level documentation from `--!` comments
    pub doc: Option<solite_ast::DocComment>,
    /// Column-level documentation: column name (lowercase) -> DocComment
    pub column_docs: HashMap<String, solite_ast::DocComment>,
}

/// Index info tracked during analysis
#[derive(Debug, Clone)]
pub struct IndexInfo {
    /// Original index name (preserves case)
    pub name: String,
    /// Table this index is on
    pub table_name: String,
    /// Columns included in the index (original case)
    pub columns: Vec<String>,
    /// Whether this is a UNIQUE index
    pub is_unique: bool,
}

/// View info tracked during analysis
#[derive(Debug, Clone)]
pub struct ViewInfo {
    /// Original view name (preserves case)
    pub name: String,
    /// Columns inferred from the SELECT statement
    pub columns: Vec<String>,
}

/// Trigger info tracked during analysis
#[derive(Debug, Clone)]
pub struct TriggerInfo {
    /// Original trigger name (preserves case)
    pub name: String,
    /// Table this trigger is on
    pub table_name: String,
    /// Trigger event type (INSERT, UPDATE, DELETE)
    pub event: TriggerEventType,
}

/// Simplified trigger event type for schema tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerEventType {
    Insert,
    Update,
    Delete,
}

/// Schema containing table definitions for a document
#[derive(Debug, Clone, Default)]
pub struct Schema {
    /// Table registry: lowercase table name -> TableInfo
    tables: HashMap<String, TableInfo>,
    /// Maps lowercase table name -> original case table name
    original_names: HashMap<String, String>,
    /// Index registry: lowercase index name -> IndexInfo
    indexes: HashMap<String, IndexInfo>,
    /// View registry: lowercase view name -> ViewInfo
    views: HashMap<String, ViewInfo>,
    /// Trigger registry: lowercase trigger name -> TriggerInfo
    triggers: HashMap<String, TriggerInfo>,
    /// Scalar function names available in the database
    functions: Vec<String>,
    /// Function argument counts: lowercase name -> sorted list of valid narg values.
    /// narg = -1 means variadic (accepts any number of arguments).
    function_nargs: HashMap<String, Vec<i32>>,
}

impl Schema {
    /// Create a new empty schema
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a table to the schema
    pub fn add_table(
        &mut self,
        name: impl Into<String>,
        columns: Vec<String>,
        without_rowid: bool,
    ) {
        self.add_table_with_doc(name, columns, without_rowid, None, HashMap::new());
    }

    /// Add a table to the schema with documentation.
    pub fn add_table_with_doc(
        &mut self,
        name: impl Into<String>,
        columns: Vec<String>,
        without_rowid: bool,
        doc: Option<solite_ast::DocComment>,
        column_docs: HashMap<String, solite_ast::DocComment>,
    ) {
        let name = name.into();
        let table_key = name.to_lowercase();
        let mut col_set = HashSet::new();
        for col in &columns {
            col_set.insert(col.to_lowercase());
        }
        self.tables.insert(
            table_key.clone(),
            TableInfo {
                columns: col_set,
                original_columns: columns,
                without_rowid,
                doc,
                column_docs,
            },
        );
        self.original_names.insert(table_key, name);
    }

    /// Add a view to the schema
    pub fn add_view(&mut self, name: impl Into<String>, columns: Vec<String>) {
        let name = name.into();
        let view_key = name.to_lowercase();
        self.views.insert(
            view_key,
            ViewInfo {
                name,
                columns,
            },
        );
    }

    /// Add an index to the schema
    pub fn add_index(
        &mut self,
        name: impl Into<String>,
        table_name: impl Into<String>,
        columns: Vec<String>,
        is_unique: bool,
    ) {
        let name = name.into();
        let index_key = name.to_lowercase();
        self.indexes.insert(
            index_key,
            IndexInfo {
                name,
                table_name: table_name.into(),
                columns,
                is_unique,
            },
        );
    }

    /// Add a trigger to the schema
    pub fn add_trigger(
        &mut self,
        name: impl Into<String>,
        table_name: impl Into<String>,
        event: TriggerEventType,
    ) {
        let name = name.into();
        let trigger_key = name.to_lowercase();
        self.triggers.insert(
            trigger_key,
            TriggerInfo {
                name,
                table_name: table_name.into(),
                event,
            },
        );
    }

    /// Merge another schema into this one. Later definitions override earlier ones.
    pub fn merge(&mut self, other: Schema) {
        // Merge tables - other's tables override self's
        for (name, table) in other.tables {
            self.tables.insert(name, table);
        }

        // Merge original_names - other's names override self's
        for (name, original) in other.original_names {
            self.original_names.insert(name, original);
        }

        // Merge indexes - other's indexes override self's
        for (name, index) in other.indexes {
            self.indexes.insert(name, index);
        }

        // Merge views - other's views override self's
        for (name, view) in other.views {
            self.views.insert(name, view);
        }

        // Merge triggers - other's triggers override self's
        for (name, trigger) in other.triggers {
            self.triggers.insert(name, trigger);
        }

        // Merge functions
        if !other.functions.is_empty() {
            let existing: HashSet<String> = self.functions.iter().cloned().collect();
            for f in other.functions {
                if !existing.contains(&f) {
                    self.functions.push(f);
                }
            }
        }

        // Merge function_nargs
        for (name, nargs) in other.function_nargs {
            self.function_nargs.entry(name).or_insert(nargs);
        }
    }

    /// Returns an iterator over all table names (original case)
    pub fn table_names(&self) -> impl Iterator<Item = &str> {
        self.original_names.values().map(|s| s.as_str())
    }

    /// Returns column names for a table (case-insensitive lookup)
    pub fn columns_for_table(&self, table_name: &str) -> Option<&[String]> {
        let key = table_name.to_lowercase();
        self.tables.get(&key).map(|t| t.original_columns.as_slice())
    }

    /// Returns column names for a table, including "rowid" if the table doesn't have WITHOUT ROWID
    pub fn columns_for_table_with_rowid(&self, table_name: &str) -> Option<Vec<String>> {
        let key = table_name.to_lowercase();
        self.tables.get(&key).map(|t| {
            let mut cols = t.original_columns.clone();
            if !t.without_rowid {
                cols.push("rowid".to_string());
            }
            cols
        })
    }

    /// Returns table info for a table (case-insensitive lookup)
    pub fn get_table(&self, table_name: &str) -> Option<&TableInfo> {
        let key = table_name.to_lowercase();
        self.tables.get(&key)
    }

    /// Check if a table exists (case-insensitive)
    pub fn has_table(&self, table_name: &str) -> bool {
        let key = table_name.to_lowercase();
        self.tables.contains_key(&key)
    }

    // ========================================
    // Index methods
    // ========================================

    /// Returns an iterator over all index names (original case)
    pub fn index_names(&self) -> impl Iterator<Item = &str> {
        self.indexes.values().map(|i| i.name.as_str())
    }

    /// Returns index info (case-insensitive lookup)
    pub fn get_index(&self, index_name: &str) -> Option<&IndexInfo> {
        let key = index_name.to_lowercase();
        self.indexes.get(&key)
    }

    /// Check if an index exists (case-insensitive)
    pub fn has_index(&self, index_name: &str) -> bool {
        let key = index_name.to_lowercase();
        self.indexes.contains_key(&key)
    }

    // ========================================
    // View methods
    // ========================================

    /// Returns an iterator over all view names (original case)
    pub fn view_names(&self) -> impl Iterator<Item = &str> {
        self.views.values().map(|v| v.name.as_str())
    }

    /// Returns view info (case-insensitive lookup)
    pub fn get_view(&self, view_name: &str) -> Option<&ViewInfo> {
        let key = view_name.to_lowercase();
        self.views.get(&key)
    }

    /// Check if a view exists (case-insensitive)
    pub fn has_view(&self, view_name: &str) -> bool {
        let key = view_name.to_lowercase();
        self.views.contains_key(&key)
    }

    /// Returns column names for a view (case-insensitive lookup)
    pub fn columns_for_view(&self, view_name: &str) -> Option<&[String]> {
        let key = view_name.to_lowercase();
        self.views.get(&key).map(|v| v.columns.as_slice())
    }

    // ========================================
    // Trigger methods
    // ========================================

    /// Returns an iterator over all trigger names (original case)
    pub fn trigger_names(&self) -> impl Iterator<Item = &str> {
        self.triggers.values().map(|t| t.name.as_str())
    }

    /// Returns trigger info (case-insensitive lookup)
    pub fn get_trigger(&self, trigger_name: &str) -> Option<&TriggerInfo> {
        let key = trigger_name.to_lowercase();
        self.triggers.get(&key)
    }

    /// Check if a trigger exists (case-insensitive)
    pub fn has_trigger(&self, trigger_name: &str) -> bool {
        let key = trigger_name.to_lowercase();
        self.triggers.contains_key(&key)
    }

    // ========================================
    // Function methods
    // ========================================

    /// Set the list of available scalar function names
    pub fn set_functions(&mut self, functions: Vec<String>) {
        self.functions = functions;
    }

    /// Set function argument count metadata.
    /// Each entry maps a lowercase function name to its valid narg values.
    pub fn set_function_nargs(&mut self, nargs: HashMap<String, Vec<i32>>) {
        self.function_nargs = nargs;
    }

    /// Returns the list of function names
    pub fn function_names_list(&self) -> &[String] {
        &self.functions
    }

    /// Returns the valid narg values for a function (case-insensitive).
    /// Returns None if the function is not known.
    pub fn function_nargs(&self, name: &str) -> Option<&[i32]> {
        self.function_nargs.get(&name.to_lowercase()).map(|v| v.as_slice())
    }
}

/// Build a schema from a parsed program by extracting DDL statements
pub fn build_schema(program: &Program) -> Schema {
    let mut schema = Schema::default();

    for stmt in &program.statements {
        match stmt {
            Statement::CreateTable(create) => {
                let table_key = create.table_name.to_lowercase();
                let mut columns = HashSet::new();
                let mut original_columns = Vec::new();
                let mut column_docs = HashMap::new();

                for col in &create.columns {
                    let col_lower = col.name.to_lowercase();
                    if !columns.contains(&col_lower) {
                        columns.insert(col_lower.clone());
                        original_columns.push(col.name.clone());

                        // Extract column documentation if present
                        if let Some(ref doc) = col.doc {
                            column_docs.insert(col_lower, doc.clone());
                        }
                    }
                }

                // Check if table has WITHOUT ROWID option
                let without_rowid = create.table_options.contains(&TableOption::WithoutRowid);

                schema.tables.insert(
                    table_key.clone(),
                    TableInfo {
                        columns,
                        original_columns,
                        without_rowid,
                        doc: create.doc.clone(),
                        column_docs,
                    },
                );
                schema.original_names.insert(table_key, create.table_name.clone());
            }

            Statement::CreateIndex(create) => {
                let index_key = create.index_name.to_lowercase();

                // Extract column names from IndexedColumn
                let columns: Vec<String> = create
                    .columns
                    .iter()
                    .filter_map(|ic| extract_column_name(&ic.column))
                    .collect();

                schema.indexes.insert(
                    index_key,
                    IndexInfo {
                        name: create.index_name.clone(),
                        table_name: create.table_name.clone(),
                        columns,
                        is_unique: create.unique,
                    },
                );
            }

            Statement::CreateView(create) => {
                let view_key = create.view_name.to_lowercase();

                // If explicit columns are provided, use them
                // Otherwise, infer from the SELECT statement
                let columns = if let Some(ref cols) = create.columns {
                    cols.clone()
                } else {
                    infer_columns_from_select(&create.select)
                };

                schema.views.insert(
                    view_key,
                    ViewInfo {
                        name: create.view_name.clone(),
                        columns,
                    },
                );
            }

            Statement::CreateTrigger(create) => {
                let trigger_key = create.trigger_name.to_lowercase();

                let event = match &create.event {
                    TriggerEvent::Insert => TriggerEventType::Insert,
                    TriggerEvent::Update { .. } => TriggerEventType::Update,
                    TriggerEvent::Delete => TriggerEventType::Delete,
                };

                schema.triggers.insert(
                    trigger_key,
                    TriggerInfo {
                        name: create.trigger_name.clone(),
                        table_name: create.table_name.clone(),
                        event,
                    },
                );
            }

            Statement::CreateVirtualTable(create) => {
                let table_key = create.table_name.to_lowercase();
                schema.tables.insert(
                    table_key.clone(),
                    TableInfo {
                        columns: HashSet::new(),
                        original_columns: Vec::new(),
                        without_rowid: true,
                        doc: None,
                        column_docs: HashMap::new(),
                    },
                );
                schema.original_names.insert(table_key, create.table_name.clone());
            }

            Statement::DropTable(drop) => {
                let table_key = drop.table_name.to_lowercase();
                schema.tables.remove(&table_key);
                schema.original_names.remove(&table_key);
            }

            Statement::DropIndex(drop) => {
                let index_key = drop.index_name.to_lowercase();
                schema.indexes.remove(&index_key);
            }

            Statement::DropView(drop) => {
                let view_key = drop.view_name.to_lowercase();
                schema.views.remove(&view_key);
            }

            Statement::DropTrigger(drop) => {
                let trigger_key = drop.trigger_name.to_lowercase();
                schema.triggers.remove(&trigger_key);
            }

            // Other statements don't affect the schema
            _ => {}
        }
    }

    schema
}

/// Extract column name from an expression (for IndexedColumn)
fn extract_column_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name, _, _) => Some(name.clone()),
        Expr::Column { column, .. } => Some(column.clone()),
        _ => None,
    }
}

/// Infer column names from a SELECT statement
fn infer_columns_from_select(select: &solite_ast::SelectStmt) -> Vec<String> {
    let mut columns = Vec::new();

    for col in &select.columns {
        match col {
            ResultColumn::Expr { expr, alias, .. } => {
                if let Some(alias) = alias {
                    // If there's an alias, use it
                    columns.push(alias.clone());
                } else {
                    // Try to extract the column name from the expression
                    if let Some(name) = extract_column_name(expr) {
                        columns.push(name);
                    }
                    // If we can't determine the name, skip it
                }
            }
            ResultColumn::Star(_) => {
                // Can't infer column names from *
                // The actual columns depend on the FROM clause tables
            }
            ResultColumn::TableStar { .. } => {
                // Can't infer column names from table.*
            }
        }
    }

    columns
}

/// Build a map of CTE names to their TableInfo from a WITH clause.
/// CTEs are processed in order, so earlier CTEs are visible to later ones.
fn build_cte_tables(with_clause: &WithClause) -> HashMap<String, TableInfo> {
    let mut cte_tables = HashMap::new();

    for cte in &with_clause.ctes {
        let table_info = build_table_info_from_cte(cte);
        let cte_key = cte.name.to_lowercase();
        cte_tables.insert(cte_key, table_info);
    }

    cte_tables
}

/// Build TableInfo from a single CTE definition
fn build_table_info_from_cte(cte: &CommonTableExpr) -> TableInfo {
    // Use explicit columns if provided, otherwise infer from SELECT
    let original_columns = if let Some(ref cols) = cte.columns {
        cols.clone()
    } else {
        infer_cte_columns(&cte.select)
    };

    let mut columns = HashSet::new();
    for col in &original_columns {
        columns.insert(col.to_lowercase());
    }

    TableInfo {
        columns,
        original_columns,
        without_rowid: true, // CTEs don't have rowid
        doc: None,           // CTEs don't have docs
        column_docs: HashMap::new(),
    }
}

/// Infer column names from a CTE's SELECT statement
fn infer_cte_columns(select: &SelectStmt) -> Vec<String> {
    let mut columns = Vec::new();

    for col in &select.columns {
        match col {
            ResultColumn::Expr { expr, alias, .. } => {
                if let Some(alias) = alias {
                    // If there's an alias, use it
                    columns.push(alias.clone());
                } else {
                    // Try to extract the column name from the expression
                    if let Some(name) = extract_column_name(expr) {
                        columns.push(name);
                    }
                    // If we can't determine the name, we skip it
                    // This happens for complex expressions without aliases
                }
            }
            ResultColumn::Star(_) => {
                // For SELECT *, we can't infer column names without knowing the source tables
                // We add a special marker that means "all columns from source"
                columns.push("*".to_string());
            }
            ResultColumn::TableStar { table, .. } => {
                // For table.*, we note the table prefix
                columns.push(format!("{}.*", table));
            }
        }
    }

    columns
}

/// Context for expression validation, tracking available tables and columns
#[derive(Debug, Clone, Default)]
struct ExprContext<'a> {
    /// Maps table name/alias (lowercase) to (original_name, TableInfo)
    /// The original_name is used for error messages
    available_tables: HashMap<String, (String, &'a TableInfo)>,
    /// All columns from all available tables (lowercase) for unqualified column lookup
    /// Maps column name -> vec of (table_alias, table_info)
    all_columns: HashMap<String, Vec<(String, &'a TableInfo)>>,
    /// Function argument counts: lowercase name -> valid narg values.
    /// narg = -1 means variadic. None if not available.
    function_nargs: Option<&'a HashMap<String, Vec<i32>>>,
    /// Table-valued function aliases whose columns are unknown at analysis time.
    /// Qualified references to these aliases (e.g. alias.col) are accepted without validation.
    opaque_aliases: HashSet<String>,
}

impl<'a> ExprContext<'a> {
    fn new() -> Self {
        Self::default()
    }

    /// Register a table-valued function alias with unknown columns.
    fn add_table_function_alias(&mut self, alias: &str, _function_name: &str) {
        self.opaque_aliases.insert(alias.to_lowercase());
    }

    /// Add a table to the context
    fn add_table(&mut self, name_or_alias: &str, original_name: &str, info: &'a TableInfo) {
        let key = name_or_alias.to_lowercase();
        self.available_tables.insert(key.clone(), (original_name.to_string(), info));

        // Add all columns from this table to the all_columns map
        for col in &info.columns {
            self.all_columns
                .entry(col.clone())
                .or_default()
                .push((name_or_alias.to_string(), info));
        }

        // Also add "rowid" if not a WITHOUT ROWID table
        if !info.without_rowid {
            self.all_columns
                .entry("rowid".to_string())
                .or_default()
                .push((name_or_alias.to_string(), info));
        }
    }

    /// Check if a column exists in a specific table.
    /// Returns true if the table's columns are unknown (empty set, e.g. virtual tables
    /// or table-valued functions) since we can't validate columns we don't know about.
    fn column_exists_in_table(&self, table_name: &str, column_name: &str) -> bool {
        let table_key = table_name.to_lowercase();
        if self.opaque_aliases.contains(&table_key) {
            return true;
        }
        let col_key = column_name.to_lowercase();

        if let Some((_, info)) = self.available_tables.get(&table_key) {
            info.columns.is_empty() || info.columns.contains(&col_key) || (col_key == "rowid" && !info.without_rowid)
        } else {
            false
        }
    }

    /// Check if a table exists in the context
    fn has_table(&self, table_name: &str) -> bool {
        let key = table_name.to_lowercase();
        self.available_tables.contains_key(&key) || self.opaque_aliases.contains(&key)
    }

    /// Get the original table name for error messages
    fn get_original_table_name(&self, table_name: &str) -> Option<&str> {
        self.available_tables
            .get(&table_name.to_lowercase())
            .map(|(orig, _)| orig.as_str())
    }

    /// Check if an unqualified column exists in any available table
    fn unqualified_column_exists(&self, column_name: &str) -> bool {
        let col_key = column_name.to_lowercase();
        self.all_columns.contains_key(&col_key)
    }

    /// Check if context is empty (no tables available)
    fn is_empty(&self) -> bool {
        self.available_tables.is_empty()
    }
}

/// Build expression context from a FROM clause
fn build_expr_context_from_from<'a>(
    from: &FromClause,
    cte_tables: &'a HashMap<String, TableInfo>,
    local_tables: &'a HashMap<String, TableInfo>,
    external_schema: Option<&'a Schema>,
) -> ExprContext<'a> {
    let mut ctx = ExprContext::new();
    for table in &from.tables {
        add_table_to_context(table, &mut ctx, cte_tables, local_tables, external_schema);
    }
    ctx
}

/// Recursively add tables from a TableOrSubquery to the context
fn add_table_to_context<'a>(
    table: &TableOrSubquery,
    ctx: &mut ExprContext<'a>,
    cte_tables: &'a HashMap<String, TableInfo>,
    local_tables: &'a HashMap<String, TableInfo>,
    external_schema: Option<&'a Schema>,
) {
    match table {
        TableOrSubquery::Table { name, alias, .. } => {
            let table_key = name.to_lowercase();
            // Look up table info: CTEs first, then local tables, then external schema
            let table_info = cte_tables.get(&table_key)
                .or_else(|| local_tables.get(&table_key))
                .or_else(|| external_schema.and_then(|s| s.get_table(&table_key)));

            if let Some(info) = table_info {
                // Use alias if provided, otherwise use table name
                let effective_name = alias.as_ref().unwrap_or(name);
                ctx.add_table(effective_name, name, info);
            }
        }
        TableOrSubquery::TableFunction { name, alias, .. } => {
            let table_key = name.to_lowercase();
            let table_info = cte_tables.get(&table_key)
                .or_else(|| local_tables.get(&table_key))
                .or_else(|| external_schema.and_then(|s| s.get_table(&table_key)));

            if let Some(info) = table_info {
                let effective_name = alias.as_ref().unwrap_or(name);
                ctx.add_table(effective_name, name, info);
            } else if let Some(alias) = alias {
                // Table functions are runtime entities — register the alias with
                // an empty column set so qualified references (e.g. alias.col)
                // don't produce false "table not found" errors.
                ctx.add_table_function_alias(alias, name);
            }
        }
        TableOrSubquery::Subquery { .. } => {
            // Subqueries create their own scope - we don't add their columns
            // to the outer context. They're validated separately.
        }
        TableOrSubquery::TableList { tables, .. } => {
            for t in tables {
                add_table_to_context(t, ctx, cte_tables, local_tables, external_schema);
            }
        }
        TableOrSubquery::Join { left, right, .. } => {
            add_table_to_context(left, ctx, cte_tables, local_tables, external_schema);
            add_table_to_context(right, ctx, cte_tables, local_tables, external_schema);
        }
    }
}

/// Analyze a SELECT statement with column validation
fn analyze_select(
    select: &SelectStmt,
    local_tables: &HashMap<String, TableInfo>,
    external_schema: Option<&Schema>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Check for empty SELECT (no columns)
    if select.columns.is_empty() {
        diagnostics.push(Diagnostic::error(
            "SELECT statement must have at least one column",
            select.span.clone(),
        ));
    }

    // Build CTE tables from WITH clause
    let cte_tables = if let Some(ref with_clause) = select.with_clause {
        build_cte_tables(with_clause)
    } else {
        HashMap::new()
    };

    // Build expression context from FROM clause
    let mut expr_ctx = if let Some(ref from_clause) = select.from {
        // First, check for unknown tables (including CTEs)
        check_unknown_tables_in_from(from_clause, &cte_tables, local_tables, external_schema, diagnostics);
        // Then build the context
        build_expr_context_from_from(from_clause, &cte_tables, local_tables, external_schema)
    } else {
        ExprContext::new()
    };
    if let Some(schema) = external_schema {
        expr_ctx.function_nargs = Some(&schema.function_nargs);
    }

    // Check each result column
    for col in &select.columns {
        match col {
            ResultColumn::Expr { expr, .. } => {
                check_expr_with_context(expr, &expr_ctx, diagnostics);
            }
            ResultColumn::Star(_) | ResultColumn::TableStar { .. } => {}
        }
    }

    // Check WHERE clause expression
    if let Some(ref where_expr) = select.where_clause {
        check_expr_with_context(where_expr, &expr_ctx, diagnostics);
    }

    // Check GROUP BY expressions
    if let Some(ref group_by) = select.group_by {
        for expr in group_by {
            check_expr_with_context(expr, &expr_ctx, diagnostics);
        }
    }

    // Check HAVING expression
    if let Some(ref having) = select.having {
        check_expr_with_context(having, &expr_ctx, diagnostics);
    }

    // Check ORDER BY expressions
    if let Some(ref order_by) = select.order_by {
        for term in order_by {
            check_expr_with_context(&term.expr, &expr_ctx, diagnostics);
        }
    }

    // Check JOIN ON conditions
    if let Some(ref from_clause) = select.from {
        check_join_conditions(from_clause, &expr_ctx, diagnostics);
    }

    // Check compound SELECT statements (CTEs are also visible to compound parts)
    for (_, core) in &select.compounds {
        let mut compound_ctx = if let Some(ref from_clause) = core.from {
            check_unknown_tables_in_from(from_clause, &cte_tables, local_tables, external_schema, diagnostics);
            build_expr_context_from_from(from_clause, &cte_tables, local_tables, external_schema)
        } else {
            ExprContext::new()
        };
        if let Some(schema) = external_schema {
            compound_ctx.function_nargs = Some(&schema.function_nargs);
        }

        for col in &core.columns {
            if let ResultColumn::Expr { expr, .. } = col {
                check_expr_with_context(expr, &compound_ctx, diagnostics);
            }
        }

        if let Some(ref where_expr) = core.where_clause {
            check_expr_with_context(where_expr, &compound_ctx, diagnostics);
        }

        if let Some(ref group_by) = core.group_by {
            for expr in group_by {
                check_expr_with_context(expr, &compound_ctx, diagnostics);
            }
        }

        if let Some(ref having) = core.having {
            check_expr_with_context(having, &compound_ctx, diagnostics);
        }

        if let Some(ref from_clause) = core.from {
            check_join_conditions(from_clause, &compound_ctx, diagnostics);
        }
    }
}

/// Check for unknown tables in a FROM clause
fn check_unknown_tables_in_from(
    from: &FromClause,
    cte_tables: &HashMap<String, TableInfo>,
    local_tables: &HashMap<String, TableInfo>,
    external_schema: Option<&Schema>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for table in &from.tables {
        check_unknown_tables_recursive(table, cte_tables, local_tables, external_schema, diagnostics);
    }
}

/// Recursively check for unknown tables in a TableOrSubquery
fn check_unknown_tables_recursive(
    table: &TableOrSubquery,
    cte_tables: &HashMap<String, TableInfo>,
    local_tables: &HashMap<String, TableInfo>,
    external_schema: Option<&Schema>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match table {
        TableOrSubquery::Table { name, span, .. } => {
            let table_key = name.to_lowercase();
            // Check CTEs first, then local tables, then external schema
            let table_info = cte_tables.get(&table_key)
                .or_else(|| local_tables.get(&table_key))
                .or_else(|| external_schema.and_then(|s| s.get_table(&table_key)));

            if table_info.is_none() {
                diagnostics.push(Diagnostic::error(
                    format!("Unknown table '{}'", name),
                    span.clone(),
                ));
            }
        }
        TableOrSubquery::TableFunction { .. } => {
            // Table functions are runtime functions, not schema tables — skip validation
        }
        TableOrSubquery::Subquery { query, .. } => {
            // Recursively analyze subquery with its own scope
            analyze_select(query, local_tables, external_schema, diagnostics);
        }
        TableOrSubquery::TableList { tables, .. } => {
            for t in tables {
                check_unknown_tables_recursive(t, cte_tables, local_tables, external_schema, diagnostics);
            }
        }
        TableOrSubquery::Join { left, right, .. } => {
            check_unknown_tables_recursive(left, cte_tables, local_tables, external_schema, diagnostics);
            check_unknown_tables_recursive(right, cte_tables, local_tables, external_schema, diagnostics);
        }
    }
}

/// Check JOIN ON conditions for column validity
fn check_join_conditions(from: &FromClause, ctx: &ExprContext, diagnostics: &mut Vec<Diagnostic>) {
    for table in &from.tables {
        check_join_conditions_recursive(table, ctx, diagnostics);
    }
}

/// Recursively check JOIN conditions
fn check_join_conditions_recursive(
    table: &TableOrSubquery,
    ctx: &ExprContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match table {
        TableOrSubquery::Join { left, right, constraint, .. } => {
            // Check left and right recursively
            check_join_conditions_recursive(left, ctx, diagnostics);
            check_join_conditions_recursive(right, ctx, diagnostics);

            // Check the ON condition
            if let Some(JoinConstraint::On(expr)) = constraint {
                check_expr_with_context(expr, ctx, diagnostics);
            }
        }
        TableOrSubquery::TableList { tables, .. } => {
            for t in tables {
                check_join_conditions_recursive(t, ctx, diagnostics);
            }
        }
        _ => {}
    }
}

/// Analyze a SQL program and return diagnostics.
/// Tracks CREATE TABLE statements to validate column references in SELECT...FROM.
pub fn analyze(program: &Program) -> Vec<Diagnostic> {
    analyze_with_schema(program, None)
}

/// Analyze a SQL program with an optional external schema.
/// The external schema provides table definitions from outside this program
/// (e.g., from other notebook cells).
pub fn analyze_with_schema(program: &Program, external_schema: Option<&Schema>) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    // Table registry: lowercase table name -> TableInfo
    let mut tables: HashMap<String, TableInfo> = HashMap::new();

    for stmt in &program.statements {
        match stmt {
            Statement::Select(select) => {
                analyze_select(select, &tables, external_schema, &mut diagnostics);
            }
            Statement::CreateTable(create) => {
                // Check for empty table (no columns), but not for CREATE TABLE ... AS SELECT
                if create.columns.is_empty() && create.as_select.is_none() {
                    diagnostics.push(Diagnostic::error(
                        "CREATE TABLE must have at least one column",
                        create.span.clone(),
                    ));
                }

                // Check for duplicate column names and build column set
                let mut seen_columns: HashSet<String> = HashSet::new();
                for col in &create.columns {
                    let col_lower = col.name.to_lowercase();
                    if seen_columns.contains(&col_lower) {
                        diagnostics.push(Diagnostic::error(
                            format!("Duplicate column name '{}'", col.name),
                            col.span.clone(),
                        ));
                    } else {
                        seen_columns.insert(col_lower);
                    }
                }

                // Register the table for future SELECT validation
                let table_key = create.table_name.to_lowercase();
                let original_columns: Vec<String> = create.columns.iter().map(|c| c.name.clone()).collect();
                let without_rowid = create.table_options.contains(&TableOption::WithoutRowid);

                // Extract column docs
                let mut column_docs = HashMap::new();
                for col in &create.columns {
                    if let Some(ref doc) = col.doc {
                        column_docs.insert(col.name.to_lowercase(), doc.clone());
                    }
                }

                tables.insert(table_key, TableInfo {
                    columns: seen_columns,
                    original_columns,
                    without_rowid,
                    doc: create.doc.clone(),
                    column_docs,
                });
            }
            Statement::CreateVirtualTable(create) => {
                // Register virtual table so subsequent SELECTs can reference it.
                // Columns are unknown at parse time, so leave them empty.
                let table_key = create.table_name.to_lowercase();
                tables.insert(table_key, TableInfo {
                    columns: HashSet::new(),
                    original_columns: Vec::new(),
                    without_rowid: true,
                    doc: None,
                    column_docs: HashMap::new(),
                });
            }
            // Other statement types - no specific analysis yet
            Statement::Insert(_)
            | Statement::Update(_)
            | Statement::Delete(_)
            | Statement::CreateIndex(_)
            | Statement::CreateView(_)
            | Statement::CreateTrigger(_)
            | Statement::AlterTable(_)
            | Statement::DropTable(_)
            | Statement::DropIndex(_)
            | Statement::DropView(_)
            | Statement::DropTrigger(_)
            | Statement::Begin(_)
            | Statement::Commit(_)
            | Statement::Rollback(_)
            | Statement::Savepoint(_)
            | Statement::Release(_)
            | Statement::Vacuum(_)
            | Statement::Analyze(_)
            | Statement::Reindex(_)
            | Statement::Attach(_)
            | Statement::Detach(_)
            | Statement::Pragma(_)
            | Statement::Explain { .. } => {}
        }
    }

    diagnostics
}

/// Lint a SQL program with configuration and return lint results
pub fn lint_with_config(
    program: &Program,
    source: &str,
    config: &rules::LintConfig,
    _external_schema: Option<&Schema>,
) -> Vec<rules::LintResult> {
    use rules::{LintContext, Suppressions, RULES};

    let suppressions = Suppressions::parse(source);
    let ctx = LintContext {
        source,
        suppressions: &suppressions,
        config,
    };

    // Compute line numbers for suppression checking
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let offset_to_line = |offset: usize| -> usize {
        line_starts.partition_point(|&start| start <= offset)
    };

    let mut results = Vec::new();

    // Check each statement and its expressions
    for stmt in &program.statements {
        for rule in RULES.iter() {
            let severity = config.get_severity(rule.id(), rule.default_severity());
            if severity == rules::RuleSeverity::Off {
                continue;
            }

            // Check statement-level rules
            for mut diag in rule.check_stmt(stmt, &ctx) {
                let line = offset_to_line(diag.span.start);
                if suppressions.is_suppressed(rule.id(), line) {
                    continue;
                }
                diag.severity = severity;
                let fix = rule.fix(&diag, source);
                results.push(rules::LintResult { diagnostic: diag, fix });
            }
        }

        // Walk expressions in the statement
        walk_statement_exprs(stmt, |expr| {
            for rule in RULES.iter() {
                let severity = config.get_severity(rule.id(), rule.default_severity());
                if severity == rules::RuleSeverity::Off {
                    continue;
                }

                for mut diag in rule.check_expr(expr, &ctx) {
                    let line = offset_to_line(diag.span.start);
                    if suppressions.is_suppressed(rule.id(), line) {
                        continue;
                    }
                    diag.severity = severity;
                    let fix = rule.fix(&diag, source);
                    results.push(rules::LintResult { diagnostic: diag, fix });
                }
            }
        });
    }

    results
}

/// Walk all expressions in a statement and call the visitor function
fn walk_statement_exprs<F>(stmt: &Statement, mut visitor: F)
where
    F: FnMut(&Expr),
{
    // Implementation depends on statement type - walk through all expression fields
    if let Statement::Select(select) = stmt {
        for col in &select.columns {
            if let ResultColumn::Expr { expr, .. } = col {
                walk_expr(expr, &mut visitor);
            }
        }
        if let Some(ref where_clause) = select.where_clause {
            walk_expr(where_clause, &mut visitor);
        }
        if let Some(ref group_by) = select.group_by {
            for expr in group_by {
                walk_expr(expr, &mut visitor);
            }
        }
        if let Some(ref having) = select.having {
            walk_expr(having, &mut visitor);
        }
        // ... other SELECT parts
    }
}

fn walk_expr<F>(expr: &Expr, visitor: &mut F)
where
    F: FnMut(&Expr),
{
    visitor(expr);

    // Recursively walk child expressions
    match expr {
        Expr::Binary { left, right, .. } => {
            walk_expr(left, visitor);
            walk_expr(right, visitor);
        }
        Expr::Unary { expr: inner, .. } => {
            walk_expr(inner, visitor);
        }
        Expr::Paren(inner, _) => {
            walk_expr(inner, visitor);
        }
        Expr::Between { expr, low, high, .. } => {
            walk_expr(expr, visitor);
            walk_expr(low, visitor);
            walk_expr(high, visitor);
        }
        Expr::InList { expr, list, .. } => {
            walk_expr(expr, visitor);
            for item in list {
                walk_expr(item, visitor);
            }
        }
        Expr::InSelect { expr, .. } => {
            walk_expr(expr, visitor);
        }
        Expr::Like { expr, pattern, escape, .. } => {
            walk_expr(expr, visitor);
            walk_expr(pattern, visitor);
            if let Some(esc) = escape {
                walk_expr(esc, visitor);
            }
        }
        Expr::IsNull { expr, .. } => {
            walk_expr(expr, visitor);
        }
        Expr::Case { operand, when_clauses, else_clause, .. } => {
            if let Some(op) = operand {
                walk_expr(op, visitor);
            }
            for (when_expr, then_expr) in when_clauses {
                walk_expr(when_expr, visitor);
                walk_expr(then_expr, visitor);
            }
            if let Some(else_e) = else_clause {
                walk_expr(else_e, visitor);
            }
        }
        Expr::Cast { expr: inner, .. } => {
            walk_expr(inner, visitor);
        }
        Expr::FunctionCall { args, filter, .. } => {
            for arg in args {
                walk_expr(arg, visitor);
            }
            if let Some(f) = filter {
                walk_expr(f, visitor);
            }
        }
        Expr::Collate { expr: inner, .. } => {
            walk_expr(inner, visitor);
        }
        _ => {} // Leaf expressions
    }
}

/// Check an expression for semantic errors, including column validation
/// Format a list of valid narg values for an error message.
fn format_nargs(nargs: &[i32]) -> String {
    match nargs.len() {
        0 => "0".to_string(),
        1 => nargs[0].to_string(),
        _ => {
            let strs: Vec<String> = nargs.iter().map(|n| n.to_string()).collect();
            format!("{} or {}", strs[..strs.len() - 1].join(", "), strs.last().unwrap())
        }
    }
}

fn check_expr_with_context(expr: &Expr, ctx: &ExprContext, diagnostics: &mut Vec<Diagnostic>) {
    // Note: Lint-style checks (empty blobs, double-quoted strings) are now handled
    // by the lint system in rules/. This function only does semantic analysis.
    match expr {
        // Check unqualified column reference
        Expr::Ident(col_name, is_double_quoted, span) => {
            // Skip if context is empty (no FROM clause) or if it's a double-quoted identifier
            // Double-quoted identifiers might be string literals in SQLite
            if !ctx.is_empty() && !*is_double_quoted && !ctx.unqualified_column_exists(col_name) {
                // Check if there's exactly one table for a better error message
                let mut tables = ctx.available_tables.values();
                if let (Some((table_name, _)), None) = (tables.next(), tables.next()) {
                    diagnostics.push(Diagnostic::error(
                        format!(
                            "Column '{}' does not exist in table '{}'",
                            col_name, table_name
                        ),
                        span.clone(),
                    ));
                } else {
                    diagnostics.push(Diagnostic::error(
                        format!("Column '{}' does not exist in any available table", col_name),
                        span.clone(),
                    ));
                }
            }
        }
        // Check qualified column reference (table.column or schema.table.column)
        Expr::Column { table, column, span, .. } => {
            if let Some(table_name) = table {
                if !ctx.is_empty() {
                    if !ctx.has_table(table_name) {
                        diagnostics.push(Diagnostic::error(
                            format!("Table '{}' not found in FROM clause", table_name),
                            span.clone(),
                        ));
                    } else if !ctx.column_exists_in_table(table_name, column) {
                        let original_name = ctx.get_original_table_name(table_name).unwrap_or(table_name);
                        diagnostics.push(Diagnostic::error(
                            format!(
                                "Column '{}' does not exist in table '{}'",
                                column, original_name
                            ),
                            span.clone(),
                        ));
                    }
                }
            } else {
                // No table qualifier - check as unqualified
                if !ctx.is_empty() && !ctx.unqualified_column_exists(column) {
                    let mut tables = ctx.available_tables.values();
                    if let (Some((table_name, _)), None) = (tables.next(), tables.next()) {
                        diagnostics.push(Diagnostic::error(
                            format!(
                                "Column '{}' does not exist in table '{}'",
                                column, table_name
                            ),
                            span.clone(),
                        ));
                    } else {
                        diagnostics.push(Diagnostic::error(
                            format!("Column '{}' does not exist in any available table", column),
                            span.clone(),
                        ));
                    }
                }
            }
        }
        // Recursively check nested expressions
        Expr::Binary { left, right, .. } => {
            check_expr_with_context(left, ctx, diagnostics);
            check_expr_with_context(right, ctx, diagnostics);
        }
        Expr::Unary { expr, .. } => {
            check_expr_with_context(expr, ctx, diagnostics);
        }
        Expr::Paren(inner, _) => {
            check_expr_with_context(inner, ctx, diagnostics);
        }
        Expr::Between { expr, low, high, .. } => {
            check_expr_with_context(expr, ctx, diagnostics);
            check_expr_with_context(low, ctx, diagnostics);
            check_expr_with_context(high, ctx, diagnostics);
        }
        Expr::InList { expr, list, .. } => {
            check_expr_with_context(expr, ctx, diagnostics);
            for item in list {
                check_expr_with_context(item, ctx, diagnostics);
            }
        }
        Expr::InSelect { expr, .. } => {
            check_expr_with_context(expr, ctx, diagnostics);
            // Subquery is checked separately with its own context
        }
        Expr::Like { expr, pattern, escape, .. } => {
            check_expr_with_context(expr, ctx, diagnostics);
            check_expr_with_context(pattern, ctx, diagnostics);
            if let Some(esc) = escape {
                check_expr_with_context(esc, ctx, diagnostics);
            }
        }
        Expr::IsNull { expr, .. } => {
            check_expr_with_context(expr, ctx, diagnostics);
        }
        Expr::Case { operand, when_clauses, else_clause, .. } => {
            if let Some(op) = operand {
                check_expr_with_context(op, ctx, diagnostics);
            }
            for (when_expr, then_expr) in when_clauses {
                check_expr_with_context(when_expr, ctx, diagnostics);
                check_expr_with_context(then_expr, ctx, diagnostics);
            }
            if let Some(else_e) = else_clause {
                check_expr_with_context(else_e, ctx, diagnostics);
            }
        }
        Expr::Cast { expr, .. } => {
            check_expr_with_context(expr, ctx, diagnostics);
        }
        Expr::FunctionCall { name, args, filter, span, .. } => {
            // Check argument count against known function signatures
            if let Some(function_nargs) = ctx.function_nargs {
                let fn_lower = name.to_lowercase();
                if let Some(valid_nargs) = function_nargs.get(&fn_lower) {
                    // Negative narg means variadic, skip check
                    if !valid_nargs.iter().any(|&n| n < 0) {
                        // count(*) parses as args=[Star] but the narg=0 signature
                        let effective_nargs = if args.len() == 1 && matches!(args[0], Expr::Star(_)) {
                            0
                        } else {
                            args.len() as i32
                        };
                        if !valid_nargs.contains(&effective_nargs) {
                            let expected = format_nargs(valid_nargs);
                            diagnostics.push(Diagnostic::error(
                                format!(
                                    "{}() expects {} arguments, but {} were provided",
                                    name, expected, effective_nargs
                                ),
                                span.clone(),
                            ));
                        }
                    }
                }
            }
            for arg in args {
                check_expr_with_context(arg, ctx, diagnostics);
            }
            if let Some(f) = filter {
                check_expr_with_context(f, ctx, diagnostics);
            }
        }
        Expr::Collate { expr, .. } => {
            check_expr_with_context(expr, ctx, diagnostics);
        }
        Expr::Exists { .. } => {
            // Subqueries are checked separately with their own context
        }
        Expr::Subquery { .. } => {
            // Subqueries are checked separately with their own context
        }
        // Literals and other terminal expressions
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solite_ast::{ColumnDef, CreateTableStmt, DistinctAll, FromClause, SelectStmt, TableOrSubquery};

    fn make_program(statements: Vec<Statement>) -> Program {
        Program { statements }
    }

    /// Helper to create a simple SelectStmt for tests
    fn make_select(columns: Vec<ResultColumn>, from: Option<FromClause>) -> SelectStmt {
        let span_end = from.as_ref().map(|f| f.span.end).or_else(|| columns.last().map(|c| c.span().end)).unwrap_or(0);
        SelectStmt {
            with_clause: None,
            distinct: DistinctAll::All,
            columns,
            from,
            where_clause: None,
            group_by: None,
            having: None,
            compounds: Vec::new(),
            order_by: None,
            limit: None,
            span: Span::new(0, span_end),
        }
    }

    /// Helper to create a FromClause from a table name
    fn make_from(table_name: &str, span: Span) -> FromClause {
        FromClause {
            tables: vec![TableOrSubquery::Table {
                schema: None,
                name: table_name.to_string(),
                alias: None,
                alias_has_as: false,
                indexed: None,
                span: span.clone(),
            }],
            span,
        }
    }

    /// Helper to wrap an Expr as a ResultColumn
    fn expr_col(expr: Expr) -> ResultColumn {
        let span = expr.span().clone();
        ResultColumn::Expr { expr, alias: None, alias_has_as: false, span }
    }

    #[test]
    fn test_valid_select() {
        let program = make_program(vec![Statement::Select(
            make_select(vec![expr_col(Expr::Integer(1, Span::new(7, 8)))], None)
        )]);

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_empty_blob_warning() {
        // Empty blob warnings are now handled by the lint system
        let source = "SELECT X''";
        let program = solite_parser::parse_program(source).unwrap();
        let config = LintConfig::default();

        let results = lint_with_config(&program, source, &config, None);
        assert_eq!(results.len(), 1);
        assert!(results[0].diagnostic.message.contains("Empty blob"));
        assert_eq!(results[0].diagnostic.rule_id, "empty-blob-literal");
    }

    #[test]
    fn test_double_quoted_identifier_warning() {
        // Double-quoted identifier warnings are now handled by the lint system
        let source = r#"SELECT "hello""#;
        let program = solite_parser::parse_program(source).unwrap();
        let config = LintConfig::default();

        let results = lint_with_config(&program, source, &config, None);
        assert_eq!(results.len(), 1);
        assert!(results[0].diagnostic.message.contains("Double-quoted"));
        assert!(results[0].diagnostic.message.contains("single quotes"));
        assert_eq!(results[0].diagnostic.rule_id, "double-quoted-string");
        assert_eq!(results[0].diagnostic.severity, RuleSeverity::Warning);
    }

    #[test]
    fn test_suppression_next_line() {
        // solite-ignore suppresses the warning on the following line
        let source = "-- solite-ignore: empty-blob-literal\nSELECT X''";
        let program = solite_parser::parse_program(source).unwrap();
        let config = LintConfig::default();

        let results = lint_with_config(&program, source, &config, None);
        assert_eq!(results.len(), 0, "Next-line suppression should suppress the warning");
    }

    #[test]
    fn test_suppression_multiple_rules() {
        // Can suppress multiple rules with comma separation
        let source = "-- solite-ignore: empty-blob-literal, double-quoted-string\nSELECT X'', \"hello\"";
        let program = solite_parser::parse_program(source).unwrap();
        let config = LintConfig::default();

        let results = lint_with_config(&program, source, &config, None);
        assert_eq!(results.len(), 0, "Both rules should be suppressed");
    }

    #[test]
    fn test_suppression_does_not_affect_other_rules() {
        // Suppressing one rule should not affect other rules
        let source = "-- solite-ignore: empty-blob-literal\nSELECT X'', \"hello\"";
        let program = solite_parser::parse_program(source).unwrap();
        let config = LintConfig::default();

        let results = lint_with_config(&program, source, &config, None);
        // empty-blob-literal is suppressed, but double-quoted-string should still fire
        assert_eq!(results.len(), 1, "Only one warning should remain");
        assert_eq!(results[0].diagnostic.rule_id, "double-quoted-string");
    }

    #[test]
    fn test_single_quoted_no_warning() {
        // Regular identifiers (unquoted or single-quoted) should NOT produce a warning
        let program = make_program(vec![Statement::Select(
            make_select(
                vec![expr_col(Expr::Ident("hello".to_string(), false, Span::new(7, 12)))],
                None
            )
        )]);

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_valid_create_table() {
        let program = make_program(vec![Statement::CreateTable(CreateTableStmt {
            temporary: false,
            if_not_exists: false,
            schema: None,
            table_name: "foo".to_string(),
            columns: vec![ColumnDef {
                name: "id".to_string(),
                type_name: Some("INTEGER".to_string()),
                constraints: vec![],
                doc: None,
                span: Span::new(18, 28),
            }],
            table_constraints: vec![],
            table_options: vec![],
            as_select: None,
            doc: None,
            span: Span::new(0, 30),
        })]);

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_duplicate_column_name() {
        let program = make_program(vec![Statement::CreateTable(CreateTableStmt {
            temporary: false,
            if_not_exists: false,
            schema: None,
            table_name: "foo".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".to_string(),
                    type_name: Some("INTEGER".to_string()),
                    constraints: vec![],
                    doc: None,
                    span: Span::new(18, 28),
                },
                ColumnDef {
                    name: "ID".to_string(), // duplicate (case-insensitive)
                    type_name: Some("TEXT".to_string()),
                    constraints: vec![],
                    doc: None,
                    span: Span::new(30, 37),
                },
            ],
            table_constraints: vec![],
            table_options: vec![],
            as_select: None,
            doc: None,
            span: Span::new(0, 39),
        })]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Duplicate column name"));
    }

    #[test]
    fn test_select_from_known_table_valid_column() {
        // CREATE TABLE users (id, name); SELECT id FROM users;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![
                    ColumnDef {
                        name: "id".to_string(),
                        type_name: None,
                        constraints: vec![],
                        doc: None,
                        span: Span::new(20, 22),
                    },
                    ColumnDef {
                        name: "name".to_string(),
                        type_name: None,
                        constraints: vec![],
                        doc: None,
                        span: Span::new(24, 28),
                    },
                ],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 30),
            }),
            Statement::Select(make_select(
                vec![expr_col(Expr::Ident("id".to_string(), false, Span::new(38, 40)))],
                Some(make_from("users", Span::new(46, 51))),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_select_from_known_table_invalid_column() {
        // CREATE TABLE users (id); SELECT email FROM users;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 24),
            }),
            Statement::Select(make_select(
                vec![expr_col(Expr::Ident("email".to_string(), false, Span::new(32, 37)))],
                Some(make_from("users", Span::new(43, 48))),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Column 'email' does not exist"));
    }

    #[test]
    fn test_select_from_unknown_table() {
        // SELECT id FROM nonexistent;
        let program = make_program(vec![Statement::Select(make_select(
            vec![expr_col(Expr::Ident("id".to_string(), false, Span::new(7, 9)))],
            Some(make_from("nonexistent", Span::new(15, 26))),
        ))]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Unknown table 'nonexistent'"));
    }

    #[test]
    fn test_rowid_valid_for_regular_table() {
        // CREATE TABLE t (a); SELECT rowid FROM t;
        // rowid is valid for regular tables (without WITHOUT ROWID)
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "t".to_string(),
                columns: vec![ColumnDef {
                    name: "a".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(16, 17),
                }],
                table_constraints: vec![],
                table_options: vec![], // No WITHOUT ROWID
                as_select: None,
                doc: None,
                span: Span::new(0, 19),
            }),
            Statement::Select(make_select(
                vec![expr_col(Expr::Ident("rowid".to_string(), false, Span::new(27, 32)))],
                Some(make_from("t", Span::new(38, 39))),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 0, "rowid should be valid for regular tables");
    }

    #[test]
    fn test_rowid_invalid_for_without_rowid_table() {
        // CREATE TABLE t (a PRIMARY KEY) WITHOUT ROWID; SELECT rowid FROM t;
        // rowid is NOT valid for WITHOUT ROWID tables
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "t".to_string(),
                columns: vec![ColumnDef {
                    name: "a".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(16, 17),
                }],
                table_constraints: vec![],
                table_options: vec![TableOption::WithoutRowid],
                as_select: None,
                doc: None,
                span: Span::new(0, 45),
            }),
            Statement::Select(make_select(
                vec![expr_col(Expr::Ident("rowid".to_string(), false, Span::new(54, 59)))],
                Some(make_from("t", Span::new(65, 66))),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1, "rowid should be invalid for WITHOUT ROWID tables");
        assert!(diagnostics[0].message.contains("Column 'rowid' does not exist"));
    }

    #[test]
    fn test_select_star_from_table() {
        // CREATE TABLE users (id); SELECT * FROM users;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 24),
            }),
            Statement::Select(make_select(
                vec![ResultColumn::Star(Span::new(32, 33))],
                Some(make_from("users", Span::new(39, 44))),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_column_case_insensitive() {
        // CREATE TABLE users (ID); SELECT id FROM users;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "ID".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 24),
            }),
            Statement::Select(make_select(
                vec![expr_col(Expr::Ident("id".to_string(), false, Span::new(32, 34)))],
                Some(make_from("users", Span::new(40, 45))),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty());
    }

    // Tests for external schema support (cross-cell notebook scenarios)

    fn make_external_schema(tables: Vec<(&str, Vec<&str>)>) -> Schema {
        let mut schema = Schema::default();
        for (table_name, columns) in tables {
            let table_key = table_name.to_lowercase();
            let mut col_set = HashSet::new();
            let mut original_columns = Vec::new();
            for col in columns {
                col_set.insert(col.to_lowercase());
                original_columns.push(col.to_string());
            }
            schema.tables.insert(
                table_key.clone(),
                TableInfo {
                    columns: col_set,
                    original_columns,
                    without_rowid: false,
                    doc: None,
                    column_docs: HashMap::new(),
                },
            );
            schema.original_names.insert(table_key, table_name.to_string());
        }
        schema
    }

    #[test]
    fn test_external_schema_table_lookup() {
        // External schema has "users" table, SELECT from it should succeed
        let external = make_external_schema(vec![("users", vec!["id", "name"])]);

        // SELECT id FROM users; (no CREATE TABLE in this program)
        let program = make_program(vec![Statement::Select(make_select(
            vec![expr_col(Expr::Ident("id".to_string(), false, Span::new(7, 9)))],
            Some(make_from("users", Span::new(15, 20))),
        ))]);

        let diagnostics = analyze_with_schema(&program, Some(&external));
        assert!(diagnostics.is_empty(), "Expected no errors but got: {:?}", diagnostics);
    }

    #[test]
    fn test_external_schema_invalid_column() {
        // External schema has "users" table with "id" column
        // SELECT email FROM users; should fail (no email column)
        let external = make_external_schema(vec![("users", vec!["id", "name"])]);

        let program = make_program(vec![Statement::Select(make_select(
            vec![expr_col(Expr::Ident("email".to_string(), false, Span::new(7, 12)))],
            Some(make_from("users", Span::new(18, 23))),
        ))]);

        let diagnostics = analyze_with_schema(&program, Some(&external));
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Column 'email' does not exist"));
    }

    #[test]
    fn test_external_schema_unknown_table() {
        // External schema has "users" table, but we SELECT from "orders"
        let external = make_external_schema(vec![("users", vec!["id"])]);

        let program = make_program(vec![Statement::Select(make_select(
            vec![expr_col(Expr::Ident("id".to_string(), false, Span::new(7, 9)))],
            Some(make_from("orders", Span::new(15, 21))),
        ))]);

        let diagnostics = analyze_with_schema(&program, Some(&external));
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Unknown table 'orders'"));
    }

    #[test]
    fn test_local_table_shadows_external() {
        // External schema has "users" with columns (id)
        // Local CREATE TABLE has "users" with columns (email)
        // SELECT email FROM users; should succeed (local takes precedence)
        let external = make_external_schema(vec![("users", vec!["id"])]);

        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "email".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 25),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 27),
            }),
            Statement::Select(make_select(
                vec![expr_col(Expr::Ident("email".to_string(), false, Span::new(35, 40)))],
                Some(make_from("users", Span::new(46, 51))),
            )),
        ]);

        let diagnostics = analyze_with_schema(&program, Some(&external));
        assert!(diagnostics.is_empty(), "Expected no errors but got: {:?}", diagnostics);
    }

    #[test]
    fn test_build_schema_extracts_tables() {
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "Users".to_string(),
                columns: vec![
                    ColumnDef {
                        name: "ID".to_string(),
                        type_name: None,
                        constraints: vec![],
                        doc: None,
                        span: Span::new(20, 22),
                    },
                    ColumnDef {
                        name: "Name".to_string(),
                        type_name: None,
                        constraints: vec![],
                        doc: None,
                        span: Span::new(24, 28),
                    },
                ],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 30),
            }),
            Statement::Select(make_select(
                vec![expr_col(Expr::Integer(1, Span::new(38, 39)))],
                None,
            )),
        ]);

        let schema = build_schema(&program);

        // Check table exists (case-insensitive)
        assert!(schema.has_table("users"));
        assert!(schema.has_table("USERS"));
        assert!(schema.has_table("Users"));
        assert!(!schema.has_table("orders"));

        // Check original table name is preserved
        let names: Vec<_> = schema.table_names().collect();
        assert_eq!(names, vec!["Users"]);

        // Check columns (case-insensitive lookup, original case preserved)
        let cols = schema.columns_for_table("users").unwrap();
        assert_eq!(cols, &["ID".to_string(), "Name".to_string()]);
    }

    #[test]
    fn test_schema_get_table() {
        let schema = make_external_schema(vec![("Products", vec!["sku", "Price"])]);

        let table = schema.get_table("products").unwrap();
        assert!(table.columns.contains("sku"));
        assert!(table.columns.contains("price"));
        assert!(!table.columns.contains("name"));
    }

    // ========================================
    // Tests for Index tracking
    // ========================================

    use solite_ast::{CreateIndexStmt, IndexedColumn, DropIndexStmt};

    fn make_indexed_column(name: &str) -> IndexedColumn {
        IndexedColumn {
            column: Expr::Ident(name.to_string(), false, Span::new(0, name.len())),
            collation: None,
            direction: None,
            span: Span::new(0, name.len()),
        }
    }

    #[test]
    fn test_schema_tracks_indexes() {
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: Some("INTEGER".to_string()),
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 30),
            }),
            Statement::CreateIndex(CreateIndexStmt {
                unique: false,
                if_not_exists: false,
                schema: None,
                index_name: "idx_users_id".to_string(),
                table_name: "users".to_string(),
                columns: vec![make_indexed_column("id")],
                where_clause: None,
                span: Span::new(35, 70),
            }),
        ]);

        let schema = build_schema(&program);

        assert!(schema.has_index("idx_users_id"));
        assert!(schema.has_index("IDX_USERS_ID")); // case insensitive
        assert!(!schema.has_index("nonexistent_idx"));

        let idx = schema.get_index("idx_users_id").unwrap();
        assert_eq!(idx.name, "idx_users_id");
        assert_eq!(idx.table_name, "users");
        assert_eq!(idx.columns, vec!["id".to_string()]);
        assert!(!idx.is_unique);
    }

    #[test]
    fn test_schema_tracks_unique_indexes() {
        let program = make_program(vec![
            Statement::CreateIndex(CreateIndexStmt {
                unique: true,
                if_not_exists: false,
                schema: None,
                index_name: "idx_unique_email".to_string(),
                table_name: "users".to_string(),
                columns: vec![make_indexed_column("email")],
                where_clause: None,
                span: Span::new(0, 50),
            }),
        ]);

        let schema = build_schema(&program);
        let idx = schema.get_index("idx_unique_email").unwrap();
        assert!(idx.is_unique);
    }

    #[test]
    fn test_schema_tracks_multi_column_indexes() {
        let program = make_program(vec![
            Statement::CreateIndex(CreateIndexStmt {
                unique: false,
                if_not_exists: false,
                schema: None,
                index_name: "idx_multi".to_string(),
                table_name: "users".to_string(),
                columns: vec![
                    make_indexed_column("first_name"),
                    make_indexed_column("last_name"),
                ],
                where_clause: None,
                span: Span::new(0, 60),
            }),
        ]);

        let schema = build_schema(&program);
        let idx = schema.get_index("idx_multi").unwrap();
        assert_eq!(idx.columns, vec!["first_name".to_string(), "last_name".to_string()]);
    }

    #[test]
    fn test_index_names_iterator() {
        let program = make_program(vec![
            Statement::CreateIndex(CreateIndexStmt {
                unique: false,
                if_not_exists: false,
                schema: None,
                index_name: "idx_a".to_string(),
                table_name: "t".to_string(),
                columns: vec![make_indexed_column("a")],
                where_clause: None,
                span: Span::new(0, 30),
            }),
            Statement::CreateIndex(CreateIndexStmt {
                unique: true,
                if_not_exists: false,
                schema: None,
                index_name: "idx_b".to_string(),
                table_name: "t".to_string(),
                columns: vec![make_indexed_column("b")],
                where_clause: None,
                span: Span::new(35, 65),
            }),
        ]);

        let schema = build_schema(&program);
        let mut names: Vec<_> = schema.index_names().collect();
        names.sort();
        assert_eq!(names, vec!["idx_a", "idx_b"]);
    }

    // ========================================
    // Tests for View tracking
    // ========================================

    use solite_ast::CreateViewStmt;

    #[test]
    fn test_schema_tracks_views_with_explicit_columns() {
        let program = make_program(vec![
            Statement::CreateView(CreateViewStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                view_name: "v_users".to_string(),
                columns: Some(vec!["user_id".to_string(), "user_name".to_string()]),
                select: Box::new(make_select(
                    vec![
                        expr_col(Expr::Ident("id".to_string(), false, Span::new(0, 2))),
                        expr_col(Expr::Ident("name".to_string(), false, Span::new(4, 8))),
                    ],
                    Some(make_from("users", Span::new(14, 19))),
                )),
                span: Span::new(0, 50),
            }),
        ]);

        let schema = build_schema(&program);

        assert!(schema.has_view("v_users"));
        assert!(schema.has_view("V_USERS")); // case insensitive
        assert!(!schema.has_view("nonexistent_view"));

        let view = schema.get_view("v_users").unwrap();
        assert_eq!(view.name, "v_users");
        assert_eq!(view.columns, vec!["user_id".to_string(), "user_name".to_string()]);

        // Also test columns_for_view
        let cols = schema.columns_for_view("v_users").unwrap();
        assert_eq!(cols, &["user_id".to_string(), "user_name".to_string()]);
    }

    #[test]
    fn test_schema_tracks_views_inferred_columns() {
        // When no explicit columns are provided, infer from SELECT
        let program = make_program(vec![
            Statement::CreateView(CreateViewStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                view_name: "v_test".to_string(),
                columns: None, // No explicit columns
                select: Box::new(make_select(
                    vec![
                        expr_col(Expr::Ident("a".to_string(), false, Span::new(0, 1))),
                        expr_col(Expr::Ident("b".to_string(), false, Span::new(3, 4))),
                    ],
                    None,
                )),
                span: Span::new(0, 40),
            }),
        ]);

        let schema = build_schema(&program);
        let cols = schema.columns_for_view("v_test").unwrap();
        assert_eq!(cols, &["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn test_schema_tracks_views_with_aliases() {
        // SELECT columns with aliases should use the alias as column name
        let program = make_program(vec![
            Statement::CreateView(CreateViewStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                view_name: "v_aliased".to_string(),
                columns: None,
                select: Box::new(SelectStmt {
                    with_clause: None,
                    distinct: DistinctAll::All,
                    columns: vec![
                        ResultColumn::Expr {
                            expr: Expr::Ident("id".to_string(), false, Span::new(0, 2)),
                            alias: Some("user_id".to_string()),
                            alias_has_as: true,
                            span: Span::new(0, 12),
                        },
                        ResultColumn::Expr {
                            expr: Expr::Ident("name".to_string(), false, Span::new(14, 18)),
                            alias: None,
                            alias_has_as: false,
                            span: Span::new(14, 18),
                        },
                    ],
                    from: None,
                    where_clause: None,
                    group_by: None,
                    having: None,
                    compounds: vec![],
                    order_by: None,
                    limit: None,
                    span: Span::new(0, 20),
                }),
                span: Span::new(0, 50),
            }),
        ]);

        let schema = build_schema(&program);
        let cols = schema.columns_for_view("v_aliased").unwrap();
        assert_eq!(cols, &["user_id".to_string(), "name".to_string()]);
    }

    #[test]
    fn test_view_names_iterator() {
        let program = make_program(vec![
            Statement::CreateView(CreateViewStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                view_name: "view_alpha".to_string(),
                columns: Some(vec!["x".to_string()]),
                select: Box::new(make_select(vec![expr_col(Expr::Integer(1, Span::new(0, 1)))], None)),
                span: Span::new(0, 30),
            }),
            Statement::CreateView(CreateViewStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                view_name: "view_beta".to_string(),
                columns: Some(vec!["y".to_string()]),
                select: Box::new(make_select(vec![expr_col(Expr::Integer(2, Span::new(0, 1)))], None)),
                span: Span::new(35, 65),
            }),
        ]);

        let schema = build_schema(&program);
        let mut names: Vec<_> = schema.view_names().collect();
        names.sort();
        assert_eq!(names, vec!["view_alpha", "view_beta"]);
    }

    // ========================================
    // Tests for Trigger tracking
    // ========================================

    use solite_ast::{CreateTriggerStmt, TriggerTiming};

    #[test]
    fn test_schema_tracks_triggers_insert() {
        let program = make_program(vec![
            Statement::CreateTrigger(CreateTriggerStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                trigger_name: "trg_users_insert".to_string(),
                timing: TriggerTiming::After,
                event: TriggerEvent::Insert,
                table_name: "users".to_string(),
                for_each_row: true,
                when_clause: None,
                body: vec![],
                span: Span::new(0, 60),
            }),
        ]);

        let schema = build_schema(&program);

        assert!(schema.has_trigger("trg_users_insert"));
        assert!(schema.has_trigger("TRG_USERS_INSERT")); // case insensitive
        assert!(!schema.has_trigger("nonexistent"));

        let trg = schema.get_trigger("trg_users_insert").unwrap();
        assert_eq!(trg.name, "trg_users_insert");
        assert_eq!(trg.table_name, "users");
        assert_eq!(trg.event, TriggerEventType::Insert);
    }

    #[test]
    fn test_schema_tracks_triggers_update() {
        let program = make_program(vec![
            Statement::CreateTrigger(CreateTriggerStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                trigger_name: "trg_update".to_string(),
                timing: TriggerTiming::Before,
                event: TriggerEvent::Update { columns: Some(vec!["name".to_string()]) },
                table_name: "users".to_string(),
                for_each_row: true,
                when_clause: None,
                body: vec![],
                span: Span::new(0, 60),
            }),
        ]);

        let schema = build_schema(&program);
        let trg = schema.get_trigger("trg_update").unwrap();
        assert_eq!(trg.event, TriggerEventType::Update);
    }

    #[test]
    fn test_schema_tracks_triggers_delete() {
        let program = make_program(vec![
            Statement::CreateTrigger(CreateTriggerStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                trigger_name: "trg_delete".to_string(),
                timing: TriggerTiming::After,
                event: TriggerEvent::Delete,
                table_name: "orders".to_string(),
                for_each_row: false,
                when_clause: None,
                body: vec![],
                span: Span::new(0, 50),
            }),
        ]);

        let schema = build_schema(&program);
        let trg = schema.get_trigger("trg_delete").unwrap();
        assert_eq!(trg.event, TriggerEventType::Delete);
        assert_eq!(trg.table_name, "orders");
    }

    #[test]
    fn test_trigger_names_iterator() {
        let program = make_program(vec![
            Statement::CreateTrigger(CreateTriggerStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                trigger_name: "trg_a".to_string(),
                timing: TriggerTiming::After,
                event: TriggerEvent::Insert,
                table_name: "t".to_string(),
                for_each_row: false,
                when_clause: None,
                body: vec![],
                span: Span::new(0, 30),
            }),
            Statement::CreateTrigger(CreateTriggerStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                trigger_name: "trg_b".to_string(),
                timing: TriggerTiming::Before,
                event: TriggerEvent::Delete,
                table_name: "t".to_string(),
                for_each_row: false,
                when_clause: None,
                body: vec![],
                span: Span::new(35, 65),
            }),
        ]);

        let schema = build_schema(&program);
        let mut names: Vec<_> = schema.trigger_names().collect();
        names.sort();
        assert_eq!(names, vec!["trg_a", "trg_b"]);
    }

    // ========================================
    // Tests for DROP statements
    // ========================================

    use solite_ast::{DropTableStmt, DropViewStmt, DropTriggerStmt};

    #[test]
    fn test_drop_table_removes_from_schema() {
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 30),
            }),
            Statement::DropTable(DropTableStmt {
                if_exists: false,
                schema: None,
                table_name: "users".to_string(),
                span: Span::new(35, 50),
            }),
        ]);

        let schema = build_schema(&program);
        assert!(!schema.has_table("users"));
    }

    #[test]
    fn test_drop_index_removes_from_schema() {
        let program = make_program(vec![
            Statement::CreateIndex(CreateIndexStmt {
                unique: false,
                if_not_exists: false,
                schema: None,
                index_name: "idx_test".to_string(),
                table_name: "users".to_string(),
                columns: vec![make_indexed_column("id")],
                where_clause: None,
                span: Span::new(0, 40),
            }),
            Statement::DropIndex(DropIndexStmt {
                if_exists: false,
                schema: None,
                index_name: "idx_test".to_string(),
                span: Span::new(45, 60),
            }),
        ]);

        let schema = build_schema(&program);
        assert!(!schema.has_index("idx_test"));
    }

    #[test]
    fn test_drop_view_removes_from_schema() {
        let program = make_program(vec![
            Statement::CreateView(CreateViewStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                view_name: "v_test".to_string(),
                columns: Some(vec!["a".to_string()]),
                select: Box::new(make_select(vec![expr_col(Expr::Integer(1, Span::new(0, 1)))], None)),
                span: Span::new(0, 40),
            }),
            Statement::DropView(DropViewStmt {
                if_exists: false,
                schema: None,
                view_name: "v_test".to_string(),
                span: Span::new(45, 60),
            }),
        ]);

        let schema = build_schema(&program);
        assert!(!schema.has_view("v_test"));
    }

    #[test]
    fn test_drop_trigger_removes_from_schema() {
        let program = make_program(vec![
            Statement::CreateTrigger(CreateTriggerStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                trigger_name: "trg_test".to_string(),
                timing: TriggerTiming::After,
                event: TriggerEvent::Insert,
                table_name: "users".to_string(),
                for_each_row: false,
                when_clause: None,
                body: vec![],
                span: Span::new(0, 50),
            }),
            Statement::DropTrigger(DropTriggerStmt {
                if_exists: false,
                schema: None,
                trigger_name: "trg_test".to_string(),
                span: Span::new(55, 75),
            }),
        ]);

        let schema = build_schema(&program);
        assert!(!schema.has_trigger("trg_test"));
    }

    #[test]
    fn test_drop_nonexistent_is_safe() {
        // Dropping something that doesn't exist shouldn't panic
        let program = make_program(vec![
            Statement::DropTable(DropTableStmt {
                if_exists: true,
                schema: None,
                table_name: "nonexistent".to_string(),
                span: Span::new(0, 30),
            }),
            Statement::DropIndex(DropIndexStmt {
                if_exists: true,
                schema: None,
                index_name: "nonexistent_idx".to_string(),
                span: Span::new(35, 65),
            }),
            Statement::DropView(DropViewStmt {
                if_exists: true,
                schema: None,
                view_name: "nonexistent_view".to_string(),
                span: Span::new(70, 100),
            }),
            Statement::DropTrigger(DropTriggerStmt {
                if_exists: true,
                schema: None,
                trigger_name: "nonexistent_trigger".to_string(),
                span: Span::new(105, 135),
            }),
        ]);

        let schema = build_schema(&program);
        // Should not panic, schema should just be empty
        assert!(!schema.has_table("nonexistent"));
        assert!(!schema.has_index("nonexistent_idx"));
        assert!(!schema.has_view("nonexistent_view"));
        assert!(!schema.has_trigger("nonexistent_trigger"));
    }

    #[test]
    fn test_drop_case_insensitive() {
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "Users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 30),
            }),
            Statement::DropTable(DropTableStmt {
                if_exists: false,
                schema: None,
                table_name: "USERS".to_string(), // Different case
                span: Span::new(35, 50),
            }),
        ]);

        let schema = build_schema(&program);
        assert!(!schema.has_table("users"));
        assert!(!schema.has_table("Users"));
        assert!(!schema.has_table("USERS"));
    }

    #[test]
    fn test_recreate_after_drop() {
        // Create, drop, then recreate with different columns
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "old_col".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 27),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 30),
            }),
            Statement::DropTable(DropTableStmt {
                if_exists: false,
                schema: None,
                table_name: "users".to_string(),
                span: Span::new(35, 50),
            }),
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "new_col".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(70, 77),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(55, 85),
            }),
        ]);

        let schema = build_schema(&program);
        assert!(schema.has_table("users"));
        let cols = schema.columns_for_table("users").unwrap();
        assert_eq!(cols, &["new_col".to_string()]);
    }

    // ========================================
    // Tests for complex scenarios
    // ========================================

    #[test]
    fn test_schema_with_all_object_types() {
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![
                    ColumnDef {
                        name: "id".to_string(),
                        type_name: Some("INTEGER".to_string()),
                        constraints: vec![],
                        doc: None,
                        span: Span::new(20, 22),
                    },
                    ColumnDef {
                        name: "email".to_string(),
                        type_name: Some("TEXT".to_string()),
                        constraints: vec![],
                        doc: None,
                        span: Span::new(24, 29),
                    },
                ],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 35),
            }),
            Statement::CreateIndex(CreateIndexStmt {
                unique: true,
                if_not_exists: false,
                schema: None,
                index_name: "idx_users_email".to_string(),
                table_name: "users".to_string(),
                columns: vec![make_indexed_column("email")],
                where_clause: None,
                span: Span::new(40, 80),
            }),
            Statement::CreateView(CreateViewStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                view_name: "v_users".to_string(),
                columns: None,
                select: Box::new(make_select(
                    vec![
                        expr_col(Expr::Ident("id".to_string(), false, Span::new(0, 2))),
                        expr_col(Expr::Ident("email".to_string(), false, Span::new(4, 9))),
                    ],
                    Some(make_from("users", Span::new(15, 20))),
                )),
                span: Span::new(85, 130),
            }),
            Statement::CreateTrigger(CreateTriggerStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                trigger_name: "trg_users_audit".to_string(),
                timing: TriggerTiming::After,
                event: TriggerEvent::Insert,
                table_name: "users".to_string(),
                for_each_row: true,
                when_clause: None,
                body: vec![],
                span: Span::new(135, 200),
            }),
        ]);

        let schema = build_schema(&program);

        // Verify all objects exist
        assert!(schema.has_table("users"));
        assert!(schema.has_index("idx_users_email"));
        assert!(schema.has_view("v_users"));
        assert!(schema.has_trigger("trg_users_audit"));

        // Verify table columns
        let table_cols = schema.columns_for_table("users").unwrap();
        assert_eq!(table_cols, &["id".to_string(), "email".to_string()]);

        // Verify index details
        let idx = schema.get_index("idx_users_email").unwrap();
        assert!(idx.is_unique);
        assert_eq!(idx.table_name, "users");

        // Verify view columns (inferred from SELECT)
        let view_cols = schema.columns_for_view("v_users").unwrap();
        assert_eq!(view_cols, &["id".to_string(), "email".to_string()]);

        // Verify trigger details
        let trg = schema.get_trigger("trg_users_audit").unwrap();
        assert_eq!(trg.event, TriggerEventType::Insert);
    }

    // ========================================
    // Tests for column validation in expressions
    // ========================================

    use solite_ast::{BinaryOp, JoinType, JoinConstraint};

    /// Helper to create a SelectStmt with WHERE clause
    fn make_select_with_where(columns: Vec<ResultColumn>, from: Option<FromClause>, where_clause: Option<Expr>) -> SelectStmt {
        SelectStmt {
            with_clause: None,
            distinct: DistinctAll::All,
            columns,
            from,
            where_clause,
            group_by: None,
            having: None,
            compounds: Vec::new(),
            order_by: None,
            limit: None,
            span: Span::new(0, 100),
        }
    }

    #[test]
    fn test_invalid_column_in_where_clause() {
        // CREATE TABLE users (id); SELECT id FROM users WHERE email = 'test';
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 24),
            }),
            Statement::Select(make_select_with_where(
                vec![expr_col(Expr::Ident("id".to_string(), false, Span::new(32, 34)))],
                Some(make_from("users", Span::new(40, 45))),
                Some(Expr::Binary {
                    left: Box::new(Expr::Ident("email".to_string(), false, Span::new(52, 57))),
                    op: BinaryOp::Eq,
                    right: Box::new(Expr::String("test".to_string(), Span::new(60, 66))),
                    span: Span::new(52, 66),
                }),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Column 'email' does not exist in table 'users'"));
    }

    #[test]
    fn test_valid_column_in_where_clause() {
        // CREATE TABLE users (id, email); SELECT id FROM users WHERE email = 'test';
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![
                    ColumnDef {
                        name: "id".to_string(),
                        type_name: None,
                        constraints: vec![],
                        doc: None,
                        span: Span::new(20, 22),
                    },
                    ColumnDef {
                        name: "email".to_string(),
                        type_name: None,
                        constraints: vec![],
                        doc: None,
                        span: Span::new(24, 29),
                    },
                ],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 31),
            }),
            Statement::Select(make_select_with_where(
                vec![expr_col(Expr::Ident("id".to_string(), false, Span::new(39, 41)))],
                Some(make_from("users", Span::new(47, 52))),
                Some(Expr::Binary {
                    left: Box::new(Expr::Ident("email".to_string(), false, Span::new(59, 64))),
                    op: BinaryOp::Eq,
                    right: Box::new(Expr::String("test".to_string(), Span::new(67, 73))),
                    span: Span::new(59, 73),
                }),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty(), "Expected no errors but got: {:?}", diagnostics);
    }

    #[test]
    fn test_invalid_column_in_group_by() {
        // CREATE TABLE users (id); SELECT id FROM users GROUP BY category;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 24),
            }),
            Statement::Select(SelectStmt {
                with_clause: None,
                distinct: DistinctAll::All,
                columns: vec![expr_col(Expr::Ident("id".to_string(), false, Span::new(32, 34)))],
                from: Some(make_from("users", Span::new(40, 45))),
                where_clause: None,
                group_by: Some(vec![Expr::Ident("category".to_string(), false, Span::new(55, 63))]),
                having: None,
                compounds: Vec::new(),
                order_by: None,
                limit: None,
                span: Span::new(0, 65),
            }),
        ]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Column 'category' does not exist in table 'users'"));
    }

    #[test]
    fn test_invalid_qualified_column() {
        // CREATE TABLE users (id); SELECT t.email FROM users AS t;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 24),
            }),
            Statement::Select(make_select(
                vec![expr_col(Expr::Column {
                    schema: None,
                    table: Some("t".to_string()),
                    column: "email".to_string(),
                    span: Span::new(32, 39),
                })],
                Some(FromClause {
                    tables: vec![TableOrSubquery::Table {
                        schema: None,
                        name: "users".to_string(),
                        alias: Some("t".to_string()),
                        alias_has_as: true,
                        indexed: None,
                        span: Span::new(45, 55),
                    }],
                    span: Span::new(45, 55),
                }),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Column 'email' does not exist in table 'users'"));
    }

    #[test]
    fn test_valid_qualified_column_with_alias() {
        // CREATE TABLE users (id, email); SELECT t.email FROM users AS t;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![
                    ColumnDef {
                        name: "id".to_string(),
                        type_name: None,
                        constraints: vec![],
                        doc: None,
                        span: Span::new(20, 22),
                    },
                    ColumnDef {
                        name: "email".to_string(),
                        type_name: None,
                        constraints: vec![],
                        doc: None,
                        span: Span::new(24, 29),
                    },
                ],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 31),
            }),
            Statement::Select(make_select(
                vec![expr_col(Expr::Column {
                    schema: None,
                    table: Some("t".to_string()),
                    column: "email".to_string(),
                    span: Span::new(39, 46),
                })],
                Some(FromClause {
                    tables: vec![TableOrSubquery::Table {
                        schema: None,
                        name: "users".to_string(),
                        alias: Some("t".to_string()),
                        alias_has_as: true,
                        indexed: None,
                        span: Span::new(52, 62),
                    }],
                    span: Span::new(52, 62),
                }),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty(), "Expected no errors but got: {:?}", diagnostics);
    }

    #[test]
    fn test_multiple_tables_column_from_any() {
        // CREATE TABLE users (id); CREATE TABLE orders (user_id);
        // SELECT id, user_id FROM users, orders;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 24),
            }),
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "orders".to_string(),
                columns: vec![ColumnDef {
                    name: "user_id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(45, 52),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(26, 54),
            }),
            Statement::Select(make_select(
                vec![
                    expr_col(Expr::Ident("id".to_string(), false, Span::new(62, 64))),
                    expr_col(Expr::Ident("user_id".to_string(), false, Span::new(66, 73))),
                ],
                Some(FromClause {
                    tables: vec![
                        TableOrSubquery::Table {
                            schema: None,
                            name: "users".to_string(),
                            alias: None,
                            alias_has_as: false,
                            indexed: None,
                            span: Span::new(79, 84),
                        },
                        TableOrSubquery::Table {
                            schema: None,
                            name: "orders".to_string(),
                            alias: None,
                            alias_has_as: false,
                            indexed: None,
                            span: Span::new(86, 92),
                        },
                    ],
                    span: Span::new(79, 92),
                }),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty(), "Expected no errors but got: {:?}", diagnostics);
    }

    #[test]
    fn test_multiple_tables_invalid_column() {
        // CREATE TABLE users (id); CREATE TABLE orders (user_id);
        // SELECT unknown_col FROM users, orders;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 24),
            }),
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "orders".to_string(),
                columns: vec![ColumnDef {
                    name: "user_id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(45, 52),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(26, 54),
            }),
            Statement::Select(make_select(
                vec![expr_col(Expr::Ident("unknown_col".to_string(), false, Span::new(62, 73)))],
                Some(FromClause {
                    tables: vec![
                        TableOrSubquery::Table {
                            schema: None,
                            name: "users".to_string(),
                            alias: None,
                            alias_has_as: false,
                            indexed: None,
                            span: Span::new(79, 84),
                        },
                        TableOrSubquery::Table {
                            schema: None,
                            name: "orders".to_string(),
                            alias: None,
                            alias_has_as: false,
                            indexed: None,
                            span: Span::new(86, 92),
                        },
                    ],
                    span: Span::new(79, 92),
                }),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Column 'unknown_col' does not exist in any available table"));
    }

    #[test]
    fn test_join_with_on_condition_validation() {
        // CREATE TABLE users (id); CREATE TABLE orders (user_id);
        // SELECT * FROM users JOIN orders ON users.id = orders.invalid_col;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 24),
            }),
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "orders".to_string(),
                columns: vec![ColumnDef {
                    name: "user_id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(45, 52),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(26, 54),
            }),
            Statement::Select(make_select(
                vec![ResultColumn::Star(Span::new(62, 63))],
                Some(FromClause {
                    tables: vec![TableOrSubquery::Join {
                        left: Box::new(TableOrSubquery::Table {
                            schema: None,
                            name: "users".to_string(),
                            alias: None,
                            alias_has_as: false,
                            indexed: None,
                            span: Span::new(69, 74),
                        }),
                        join_type: JoinType::Inner,
                        right: Box::new(TableOrSubquery::Table {
                            schema: None,
                            name: "orders".to_string(),
                            alias: None,
                            alias_has_as: false,
                            indexed: None,
                            span: Span::new(80, 86),
                        }),
                        constraint: Some(JoinConstraint::On(Expr::Binary {
                            left: Box::new(Expr::Column {
                                schema: None,
                                table: Some("users".to_string()),
                                column: "id".to_string(),
                                span: Span::new(90, 98),
                            }),
                            op: BinaryOp::Eq,
                            right: Box::new(Expr::Column {
                                schema: None,
                                table: Some("orders".to_string()),
                                column: "invalid_col".to_string(),
                                span: Span::new(101, 119),
                            }),
                            span: Span::new(90, 119),
                        })),
                        span: Span::new(69, 119),
                    }],
                    span: Span::new(69, 119),
                }),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Column 'invalid_col' does not exist in table 'orders'"));
    }

    #[test]
    fn test_invalid_column_in_case_expression() {
        // CREATE TABLE users (id); SELECT CASE WHEN unknown = 1 THEN 'a' END FROM users;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 24),
            }),
            Statement::Select(make_select(
                vec![expr_col(Expr::Case {
                    operand: None,
                    when_clauses: vec![(
                        Expr::Binary {
                            left: Box::new(Expr::Ident("unknown".to_string(), false, Span::new(42, 49))),
                            op: BinaryOp::Eq,
                            right: Box::new(Expr::Integer(1, Span::new(52, 53))),
                            span: Span::new(42, 53),
                        },
                        Expr::String("a".to_string(), Span::new(59, 62)),
                    )],
                    else_clause: None,
                    span: Span::new(32, 66),
                })],
                Some(make_from("users", Span::new(72, 77))),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Column 'unknown' does not exist in table 'users'"));
    }

    #[test]
    fn test_invalid_column_in_function_arg() {
        // CREATE TABLE users (id); SELECT COUNT(unknown) FROM users;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 24),
            }),
            Statement::Select(make_select(
                vec![expr_col(Expr::FunctionCall {
                    name: "COUNT".to_string(),
                    args: vec![Expr::Ident("unknown".to_string(), false, Span::new(38, 45))],
                    distinct: false,
                    filter: None,
                    over: None,
                    span: Span::new(32, 46),
                })],
                Some(make_from("users", Span::new(52, 57))),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Column 'unknown' does not exist in table 'users'"));
    }

    #[test]
    fn test_unknown_table_in_qualified_column() {
        // CREATE TABLE users (id); SELECT unknown_table.col FROM users;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 24),
            }),
            Statement::Select(make_select(
                vec![expr_col(Expr::Column {
                    schema: None,
                    table: Some("unknown_table".to_string()),
                    column: "col".to_string(),
                    span: Span::new(32, 48),
                })],
                Some(make_from("users", Span::new(54, 59))),
            )),
        ]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Table 'unknown_table' not found in FROM clause"));
    }

    #[test]
    fn test_invalid_column_in_having() {
        // CREATE TABLE users (id); SELECT id FROM users GROUP BY id HAVING unknown > 5;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 24),
            }),
            Statement::Select(SelectStmt {
                with_clause: None,
                distinct: DistinctAll::All,
                columns: vec![expr_col(Expr::Ident("id".to_string(), false, Span::new(32, 34)))],
                from: Some(make_from("users", Span::new(40, 45))),
                where_clause: None,
                group_by: Some(vec![Expr::Ident("id".to_string(), false, Span::new(55, 57))]),
                having: Some(Expr::Binary {
                    left: Box::new(Expr::Ident("unknown".to_string(), false, Span::new(65, 72))),
                    op: BinaryOp::Gt,
                    right: Box::new(Expr::Integer(5, Span::new(75, 76))),
                    span: Span::new(65, 76),
                }),
                compounds: Vec::new(),
                order_by: None,
                limit: None,
                span: Span::new(0, 78),
            }),
        ]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Column 'unknown' does not exist in table 'users'"));
    }

    use solite_ast::OrderingTerm;

    #[test]
    fn test_invalid_column_in_order_by() {
        // CREATE TABLE users (id); SELECT id FROM users ORDER BY unknown;
        let program = make_program(vec![
            Statement::CreateTable(CreateTableStmt {
                temporary: false,
                if_not_exists: false,
                schema: None,
                table_name: "users".to_string(),
                columns: vec![ColumnDef {
                    name: "id".to_string(),
                    type_name: None,
                    constraints: vec![],
                    doc: None,
                    span: Span::new(20, 22),
                }],
                table_constraints: vec![],
                table_options: vec![],
                as_select: None,
                doc: None,
                span: Span::new(0, 24),
            }),
            Statement::Select(SelectStmt {
                with_clause: None,
                distinct: DistinctAll::All,
                columns: vec![expr_col(Expr::Ident("id".to_string(), false, Span::new(32, 34)))],
                from: Some(make_from("users", Span::new(40, 45))),
                where_clause: None,
                group_by: None,
                having: None,
                compounds: Vec::new(),
                order_by: Some(vec![OrderingTerm {
                    expr: Expr::Ident("unknown".to_string(), false, Span::new(55, 62)),
                    direction: None,
                    nulls: None,
                    span: Span::new(55, 62),
                }]),
                limit: None,
                span: Span::new(0, 64),
            }),
        ]);

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Column 'unknown' does not exist in table 'users'"));
    }

    // ========================================
    // CTE (Common Table Expression) tests
    // ========================================

    #[test]
    fn test_cte_not_flagged_as_unknown_table() {
        // WITH foo AS (SELECT 1 AS x) SELECT * FROM foo;
        // "foo" should NOT be flagged as an unknown table
        let source = "WITH foo AS (SELECT 1 AS x) SELECT * FROM foo";
        let program = solite_parser::parse_program(source).unwrap();

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty(), "CTE 'foo' should be recognized as a valid table, got: {:?}", diagnostics);
    }

    #[test]
    fn test_cte_column_validation() {
        // WITH foo AS (SELECT 1 AS x, 2 AS y) SELECT x FROM foo;
        // Column 'x' should be valid
        let source = "WITH foo AS (SELECT 1 AS x, 2 AS y) SELECT x FROM foo";
        let program = solite_parser::parse_program(source).unwrap();

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty(), "Column 'x' should be valid in CTE 'foo', got: {:?}", diagnostics);
    }

    #[test]
    fn test_cte_invalid_column() {
        // WITH foo AS (SELECT 1 AS x) SELECT z FROM foo;
        // Column 'z' should be flagged as unknown
        let source = "WITH foo AS (SELECT 1 AS x) SELECT z FROM foo";
        let program = solite_parser::parse_program(source).unwrap();

        let diagnostics = analyze(&program);
        assert_eq!(diagnostics.len(), 1, "Column 'z' should be flagged as unknown");
        assert!(diagnostics[0].message.contains("Column 'z' does not exist"));
    }

    #[test]
    fn test_earlier_cte_visible_to_later_cte() {
        // WITH
        //   foo AS (SELECT 1 AS x),
        //   bar AS (SELECT * FROM foo)
        // SELECT * FROM bar;
        let source = "WITH foo AS (SELECT 1 AS x), bar AS (SELECT * FROM foo) SELECT * FROM bar";
        let program = solite_parser::parse_program(source).unwrap();

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty(), "Earlier CTE 'foo' should be visible to later CTE 'bar', got: {:?}", diagnostics);
    }

    #[test]
    fn test_cte_with_explicit_columns() {
        // WITH foo(a, b) AS (SELECT 1, 2) SELECT a, b FROM foo;
        let source = "WITH foo(a, b) AS (SELECT 1, 2) SELECT a, b FROM foo";
        let program = solite_parser::parse_program(source).unwrap();

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty(), "Explicit CTE columns 'a' and 'b' should be valid, got: {:?}", diagnostics);
    }

    #[test]
    fn test_cte_shadows_real_table() {
        // CREATE TABLE users (id);
        // WITH users AS (SELECT 1 AS x) SELECT x FROM users;
        // The CTE should shadow the real table
        let source = "CREATE TABLE users (id); WITH users AS (SELECT 1 AS x) SELECT x FROM users";
        let program = solite_parser::parse_program(source).unwrap();

        let diagnostics = analyze(&program);
        // 'x' should be valid because CTE shadows the real table
        assert!(diagnostics.is_empty(), "CTE should shadow real table, got: {:?}", diagnostics);
    }

    #[test]
    fn test_cte_in_compound_select() {
        // WITH foo AS (SELECT 1 AS x) SELECT x FROM foo UNION SELECT x FROM foo;
        let source = "WITH foo AS (SELECT 1 AS x) SELECT x FROM foo UNION SELECT x FROM foo";
        let program = solite_parser::parse_program(source).unwrap();

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty(), "CTE should be visible in both parts of compound SELECT, got: {:?}", diagnostics);
    }

    #[test]
    fn test_cte_column_inferred_from_expression() {
        // WITH foo AS (SELECT id FROM users) SELECT id FROM foo;
        // Column name 'id' should be inferred from the expression
        let source = "CREATE TABLE users (id); WITH foo AS (SELECT id FROM users) SELECT id FROM foo";
        let program = solite_parser::parse_program(source).unwrap();

        let diagnostics = analyze(&program);
        assert!(diagnostics.is_empty(), "Column 'id' should be inferred from CTE expression, got: {:?}", diagnostics);
    }
}
