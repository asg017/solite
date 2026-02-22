//! Symbol resolution for SQL statements.
//!
//! This module provides types and functions for resolving symbols (tables, columns, aliases)
//! at specific positions within SQL statements. It's used by the LSP for hover and
//! goto-definition features.

use solite_ast::{
    Expr, FromClause, Program, ResultColumn, SelectStmt, Span, Statement, TableOrSubquery,
    WithClause,
};
use std::collections::HashMap;

use crate::Schema;

/// Scope information for a single SQL statement.
/// Tracks aliases and their definitions within the statement.
#[derive(Debug, Clone, Default)]
pub struct StatementScope {
    /// Table aliases: alias (lowercase) -> (table_name, alias_span)
    pub table_aliases: HashMap<String, TableAliasInfo>,
    /// Column aliases from SELECT: alias (lowercase) -> definition info
    pub column_aliases: HashMap<String, ColumnAliasInfo>,
    /// CTEs: name (lowercase) -> (columns, definition_span)
    pub ctes: HashMap<String, CteInfo>,
}

/// Information about a table alias
#[derive(Debug, Clone)]
pub struct TableAliasInfo {
    /// The actual table name this alias refers to
    pub table_name: String,
    /// The span where the alias is defined (the alias itself)
    pub alias_span: Span,
    /// The span of the table reference (table name + alias)
    pub full_span: Span,
}

/// Information about a column alias in SELECT
#[derive(Debug, Clone)]
pub struct ColumnAliasInfo {
    /// The alias name
    pub alias: String,
    /// The span of the alias definition
    pub definition_span: Span,
    /// The span of the entire expression with alias
    pub full_span: Span,
}

/// Information about a CTE
#[derive(Debug, Clone)]
pub struct CteInfo {
    /// The CTE name
    pub name: String,
    /// Column names if explicitly provided
    pub columns: Option<Vec<String>>,
    /// Inferred column names from the SELECT statement (when columns is None)
    pub inferred_columns: Vec<String>,
    /// The span of the CTE definition
    pub definition_span: Span,
}

/// Result of resolving a symbol at a position
#[derive(Debug, Clone)]
pub enum ResolvedSymbol {
    /// A table alias (e.g., `u` in `FROM users AS u`)
    TableAlias {
        alias: String,
        table_name: String,
        definition_span: Span,
    },
    /// A column reference
    Column {
        name: String,
        /// Resolved table name (from alias if qualified)
        table_name: Option<String>,
        /// Original qualifier used (alias or table name)
        qualifier: Option<String>,
    },
    /// A table reference (actual table, not alias)
    Table {
        name: String,
        span: Span,
    },
    /// A column alias in SELECT
    ColumnAlias {
        alias: String,
        definition_span: Span,
    },
    /// A CTE reference
    Cte {
        name: String,
        definition_span: Span,
        /// Column names (explicit if provided, or inferred from SELECT)
        columns: Vec<String>,
    },
}

/// Build a StatementScope from a SELECT statement
pub fn build_scope_from_select(select: &SelectStmt, source: &str) -> StatementScope {
    let mut scope = StatementScope::default();

    // Process WITH clause (CTEs)
    if let Some(ref with_clause) = select.with_clause {
        collect_ctes(&mut scope, with_clause);
    }

    // Process FROM clause
    if let Some(ref from) = select.from {
        collect_from_clause(&mut scope, from, source);
    }

    // Process SELECT columns for column aliases
    for col in &select.columns {
        if let ResultColumn::Expr {
            alias: Some(alias),
            span,
            ..
        } = col
        {
            scope.column_aliases.insert(
                alias.to_lowercase(),
                ColumnAliasInfo {
                    alias: alias.clone(),
                    definition_span: span.clone(),
                    full_span: span.clone(),
                },
            );
        }
    }

    scope
}

/// Collect CTEs from a WITH clause
fn collect_ctes(scope: &mut StatementScope, with_clause: &WithClause) {
    for cte in &with_clause.ctes {
        // Infer columns from the SELECT statement if not explicitly provided
        let inferred_columns = infer_cte_columns_from_select(&cte.select);

        scope.ctes.insert(
            cte.name.to_lowercase(),
            CteInfo {
                name: cte.name.clone(),
                columns: cte.columns.clone(),
                inferred_columns,
                definition_span: cte.span.clone(),
            },
        );
    }
}

/// Infer column names from a CTE's SELECT statement
fn infer_cte_columns_from_select(select: &SelectStmt) -> Vec<String> {
    let mut columns = Vec::new();

    for col in &select.columns {
        match col {
            ResultColumn::Expr { expr, alias, .. } => {
                if let Some(alias_name) = alias {
                    // Use the alias if provided
                    columns.push(alias_name.clone());
                } else {
                    // Try to extract column name from expression
                    if let Some(name) = extract_column_name_from_expr(expr) {
                        columns.push(name);
                    }
                }
            }
            ResultColumn::Star(_) => {
                // Can't infer from *
                columns.push("*".to_string());
            }
            ResultColumn::TableStar { table, .. } => {
                columns.push(format!("{}.*", table));
            }
        }
    }

    columns
}

/// Extract a column name from an expression (for CTE column inference)
fn extract_column_name_from_expr(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name, _, _) => Some(name.clone()),
        Expr::Column { column, .. } => Some(column.clone()),
        _ => None,
    }
}

/// Collect table aliases from a FROM clause
fn collect_from_clause(scope: &mut StatementScope, from: &FromClause, _source: &str) {
    for table in &from.tables {
        collect_table_or_subquery(scope, table);
    }
}

/// Recursively collect aliases from a TableOrSubquery
fn collect_table_or_subquery(scope: &mut StatementScope, table: &TableOrSubquery) {
    match table {
        TableOrSubquery::Table {
            name,
            alias,
            span,
            ..
        } => {
            if let Some(alias_name) = alias {
                // We need to find the alias span within the full span
                // For now, use the full span - the alias is at the end
                let alias_span = span.clone();
                scope.table_aliases.insert(
                    alias_name.to_lowercase(),
                    TableAliasInfo {
                        table_name: name.clone(),
                        alias_span: alias_span.clone(),
                        full_span: span.clone(),
                    },
                );
            }
            // Also track the table name itself as a potential qualifier
            // (when no alias is used, table.column works)
            if alias.is_none() {
                scope.table_aliases.insert(
                    name.to_lowercase(),
                    TableAliasInfo {
                        table_name: name.clone(),
                        alias_span: span.clone(),
                        full_span: span.clone(),
                    },
                );
            }
        }
        TableOrSubquery::TableFunction {
            name,
            alias,
            span,
            ..
        } => {
            if let Some(alias_name) = alias {
                scope.table_aliases.insert(
                    alias_name.to_lowercase(),
                    TableAliasInfo {
                        table_name: name.clone(),
                        alias_span: span.clone(),
                        full_span: span.clone(),
                    },
                );
            }
            if alias.is_none() {
                scope.table_aliases.insert(
                    name.to_lowercase(),
                    TableAliasInfo {
                        table_name: name.clone(),
                        alias_span: span.clone(),
                        full_span: span.clone(),
                    },
                );
            }
        }
        TableOrSubquery::Subquery { alias, span, .. } => {
            if let Some(alias_name) = alias {
                scope.table_aliases.insert(
                    alias_name.to_lowercase(),
                    TableAliasInfo {
                        table_name: "(subquery)".to_string(),
                        alias_span: span.clone(),
                        full_span: span.clone(),
                    },
                );
            }
        }
        TableOrSubquery::Join {
            left,
            right,
            ..
        } => {
            collect_table_or_subquery(scope, left);
            collect_table_or_subquery(scope, right);
        }
        TableOrSubquery::TableList { tables, .. } => {
            for t in tables {
                collect_table_or_subquery(scope, t);
            }
        }
    }
}

/// Find which statement contains the given offset
pub fn find_statement_at_offset(program: &Program, offset: usize) -> Option<&Statement> {
    for stmt in &program.statements {
        let span = statement_span(stmt);
        if offset >= span.start && offset <= span.end {
            return Some(stmt);
        }
    }
    None
}

/// Get the span of a statement
fn statement_span(stmt: &Statement) -> &Span {
    match stmt {
        Statement::Select(s) => &s.span,
        Statement::Insert(s) => &s.span,
        Statement::Update(s) => &s.span,
        Statement::Delete(s) => &s.span,
        Statement::CreateTable(s) => &s.span,
        Statement::CreateIndex(s) => &s.span,
        Statement::CreateView(s) => &s.span,
        Statement::CreateTrigger(s) => &s.span,
        Statement::CreateVirtualTable(s) => &s.span,
        Statement::AlterTable(s) => &s.span,
        Statement::DropTable(s) => &s.span,
        Statement::DropIndex(s) => &s.span,
        Statement::DropView(s) => &s.span,
        Statement::DropTrigger(s) => &s.span,
        Statement::Begin(s) => &s.span,
        Statement::Commit(s) => &s.span,
        Statement::Rollback(s) => &s.span,
        Statement::Savepoint(s) => &s.span,
        Statement::Release(s) => &s.span,
        Statement::Vacuum(s) => &s.span,
        Statement::Analyze(s) => &s.span,
        Statement::Reindex(s) => &s.span,
        Statement::Attach(s) => &s.span,
        Statement::Detach(s) => &s.span,
        Statement::Pragma(s) => &s.span,
        Statement::Explain { span, .. } => span,
    }
}

/// Find the symbol at the given offset within a statement
pub fn find_symbol_at_offset(
    stmt: &Statement,
    source: &str,
    offset: usize,
    schema: Option<&Schema>,
) -> Option<(ResolvedSymbol, Span)> {
    match stmt {
        Statement::Select(select) => find_symbol_in_select(select, source, offset, schema),
        Statement::Insert(insert) => {
            // Check column references in source
            if let solite_ast::InsertSource::Select(ref select) = insert.source {
                if let Some(result) = find_symbol_in_select(select, source, offset, schema) {
                    return Some(result);
                }
            }
            None
        }
        Statement::Update(update) => {
            // Check WHERE clause
            if let Some(ref where_expr) = update.where_clause {
                if let Some(result) = find_symbol_in_expr(where_expr, source, offset, &StatementScope::default(), schema) {
                    return Some(result);
                }
            }
            None
        }
        Statement::Delete(delete) => {
            // Check WHERE clause
            if let Some(ref where_expr) = delete.where_clause {
                if let Some(result) = find_symbol_in_expr(where_expr, source, offset, &StatementScope::default(), schema) {
                    return Some(result);
                }
            }
            None
        }
        _ => None,
    }
}

/// Find the symbol at the given offset within a SELECT statement
fn find_symbol_in_select(
    select: &SelectStmt,
    source: &str,
    offset: usize,
    schema: Option<&Schema>,
) -> Option<(ResolvedSymbol, Span)> {
    let scope = build_scope_from_select(select, source);

    // Check SELECT columns
    for col in &select.columns {
        match col {
            ResultColumn::Expr { expr, alias, span, .. } => {
                // Check if cursor is on the alias
                if alias.is_some() {
                    // The alias is typically at the end of the span
                    // This is a simplification - in a real implementation we'd track alias span separately
                    if offset >= span.start && offset <= span.end {
                        // Check if we're on the expression part
                        if let Some(result) = find_symbol_in_expr(expr, source, offset, &scope, schema) {
                            return Some(result);
                        }
                    }
                } else if let Some(result) = find_symbol_in_expr(expr, source, offset, &scope, schema) {
                    return Some(result);
                }
            }
            ResultColumn::TableStar { table, span } => {
                if offset >= span.start && offset <= span.end {
                    // Resolve the table qualifier
                    if let Some(alias_info) = scope.table_aliases.get(&table.to_lowercase()) {
                        return Some((
                            ResolvedSymbol::TableAlias {
                                alias: table.clone(),
                                table_name: alias_info.table_name.clone(),
                                definition_span: alias_info.alias_span.clone(),
                            },
                            span.clone(),
                        ));
                    }
                }
            }
            ResultColumn::Star(_) => {}
        }
    }

    // Check FROM clause
    if let Some(ref from) = select.from {
        if let Some(result) = find_symbol_in_from(from, source, offset, &scope, schema) {
            return Some(result);
        }
    }

    // Check WHERE clause
    if let Some(ref where_expr) = select.where_clause {
        if let Some(result) = find_symbol_in_expr(where_expr, source, offset, &scope, schema) {
            return Some(result);
        }
    }

    // Check GROUP BY
    if let Some(ref group_by) = select.group_by {
        for expr in group_by {
            if let Some(result) = find_symbol_in_expr(expr, source, offset, &scope, schema) {
                return Some(result);
            }
        }
    }

    // Check HAVING
    if let Some(ref having) = select.having {
        if let Some(result) = find_symbol_in_expr(having, source, offset, &scope, schema) {
            return Some(result);
        }
    }

    // Check ORDER BY
    if let Some(ref order_by) = select.order_by {
        for term in order_by {
            if let Some(result) = find_symbol_in_expr(&term.expr, source, offset, &scope, schema) {
                return Some(result);
            }
        }
    }

    None
}

/// Find symbol in FROM clause
fn find_symbol_in_from(
    from: &FromClause,
    source: &str,
    offset: usize,
    scope: &StatementScope,
    schema: Option<&Schema>,
) -> Option<(ResolvedSymbol, Span)> {
    for table in &from.tables {
        if let Some(result) = find_symbol_in_table_or_subquery(table, source, offset, scope, schema) {
            return Some(result);
        }
    }
    None
}

/// Find symbol in TableOrSubquery
fn find_symbol_in_table_or_subquery(
    table: &TableOrSubquery,
    source: &str,
    offset: usize,
    scope: &StatementScope,
    schema: Option<&Schema>,
) -> Option<(ResolvedSymbol, Span)> {
    match table {
        TableOrSubquery::Table { name, span, .. } => {
            if offset >= span.start && offset <= span.end {
                // Check if this is a CTE reference first
                if let Some(cte_info) = scope.ctes.get(&name.to_lowercase()) {
                    // Use explicit columns if provided, otherwise use inferred columns
                    let columns = cte_info.columns.clone().unwrap_or_else(|| cte_info.inferred_columns.clone());
                    return Some((
                        ResolvedSymbol::Cte {
                            name: cte_info.name.clone(),
                            definition_span: cte_info.definition_span.clone(),
                            columns,
                        },
                        span.clone(),
                    ));
                }
                // Otherwise return a table reference
                return Some((
                    ResolvedSymbol::Table {
                        name: name.clone(),
                        span: span.clone(),
                    },
                    span.clone(),
                ));
            }
        }
        TableOrSubquery::TableFunction { name, args, span, .. } => {
            if offset >= span.start && offset <= span.end {
                // Check if cursor is on the function name or args
                for arg in args {
                    if let Some(result) = find_symbol_in_expr(arg, source, offset, scope, schema) {
                        return Some(result);
                    }
                }
                return Some((
                    ResolvedSymbol::Table {
                        name: name.clone(),
                        span: span.clone(),
                    },
                    span.clone(),
                ));
            }
        }
        TableOrSubquery::Subquery { query, span, .. } => {
            if offset >= span.start && offset <= span.end {
                // Check inside the subquery
                if let Some(result) = find_symbol_in_select(query, source, offset, schema) {
                    return Some(result);
                }
            }
        }
        TableOrSubquery::Join { left, right, constraint, .. } => {
            if let Some(result) = find_symbol_in_table_or_subquery(left, source, offset, scope, schema) {
                return Some(result);
            }
            if let Some(result) = find_symbol_in_table_or_subquery(right, source, offset, scope, schema) {
                return Some(result);
            }
            // Check JOIN constraint
            if let Some(solite_ast::JoinConstraint::On(expr)) = constraint {
                if let Some(result) = find_symbol_in_expr(expr, source, offset, scope, schema) {
                    return Some(result);
                }
            }
        }
        TableOrSubquery::TableList { tables, .. } => {
            for t in tables {
                if let Some(result) = find_symbol_in_table_or_subquery(t, source, offset, scope, schema) {
                    return Some(result);
                }
            }
        }
    }
    None
}

/// Find symbol in an expression
fn find_symbol_in_expr(
    expr: &Expr,
    source: &str,
    offset: usize,
    scope: &StatementScope,
    schema: Option<&Schema>,
) -> Option<(ResolvedSymbol, Span)> {
    let span = expr.span();
    if offset < span.start || offset > span.end {
        return None;
    }

    match expr {
        Expr::Ident(name, _is_quoted, span) => {
            // Check if this is a column alias reference
            if let Some(alias_info) = scope.column_aliases.get(&name.to_lowercase()) {
                return Some((
                    ResolvedSymbol::ColumnAlias {
                        alias: alias_info.alias.clone(),
                        definition_span: alias_info.definition_span.clone(),
                    },
                    span.clone(),
                ));
            }
            // Check if this is a table alias being used as a qualifier
            if let Some(alias_info) = scope.table_aliases.get(&name.to_lowercase()) {
                return Some((
                    ResolvedSymbol::TableAlias {
                        alias: name.clone(),
                        table_name: alias_info.table_name.clone(),
                        definition_span: alias_info.alias_span.clone(),
                    },
                    span.clone(),
                ));
            }
            // Check if this is a CTE reference
            if let Some(cte_info) = scope.ctes.get(&name.to_lowercase()) {
                // Use explicit columns if provided, otherwise use inferred columns
                let columns = cte_info.columns.clone().unwrap_or_else(|| cte_info.inferred_columns.clone());
                return Some((
                    ResolvedSymbol::Cte {
                        name: cte_info.name.clone(),
                        definition_span: cte_info.definition_span.clone(),
                        columns,
                    },
                    span.clone(),
                ));
            }
            // It's a column reference without qualifier
            Some((
                ResolvedSymbol::Column {
                    name: name.clone(),
                    table_name: None,
                    qualifier: None,
                },
                span.clone(),
            ))
        }
        Expr::Column { schema: _, table, column, span } => {
            // Qualified column reference: table.column or alias.column
            let resolved_table = table.as_ref().and_then(|t| {
                scope.table_aliases.get(&t.to_lowercase()).map(|info| info.table_name.clone())
            }).or_else(|| table.clone());

            Some((
                ResolvedSymbol::Column {
                    name: column.clone(),
                    table_name: resolved_table,
                    qualifier: table.clone(),
                },
                span.clone(),
            ))
        }
        // Recursively search in binary expressions
        Expr::Binary { left, right, .. } => {
            if let Some(result) = find_symbol_in_expr(left, source, offset, scope, schema) {
                return Some(result);
            }
            if let Some(result) = find_symbol_in_expr(right, source, offset, scope, schema) {
                return Some(result);
            }
            None
        }
        Expr::Unary { expr: inner, .. } => {
            find_symbol_in_expr(inner, source, offset, scope, schema)
        }
        Expr::Paren(inner, _) => {
            find_symbol_in_expr(inner, source, offset, scope, schema)
        }
        Expr::Between { expr: e, low, high, .. } => {
            if let Some(result) = find_symbol_in_expr(e, source, offset, scope, schema) {
                return Some(result);
            }
            if let Some(result) = find_symbol_in_expr(low, source, offset, scope, schema) {
                return Some(result);
            }
            find_symbol_in_expr(high, source, offset, scope, schema)
        }
        Expr::InList { expr: e, list, .. } => {
            if let Some(result) = find_symbol_in_expr(e, source, offset, scope, schema) {
                return Some(result);
            }
            for item in list {
                if let Some(result) = find_symbol_in_expr(item, source, offset, scope, schema) {
                    return Some(result);
                }
            }
            None
        }
        Expr::InSelect { expr: e, query, .. } => {
            if let Some(result) = find_symbol_in_expr(e, source, offset, scope, schema) {
                return Some(result);
            }
            find_symbol_in_select(query, source, offset, schema)
        }
        Expr::Like { expr: e, pattern, escape, .. } => {
            if let Some(result) = find_symbol_in_expr(e, source, offset, scope, schema) {
                return Some(result);
            }
            if let Some(result) = find_symbol_in_expr(pattern, source, offset, scope, schema) {
                return Some(result);
            }
            if let Some(esc) = escape {
                return find_symbol_in_expr(esc, source, offset, scope, schema);
            }
            None
        }
        Expr::IsNull { expr: e, .. } => {
            find_symbol_in_expr(e, source, offset, scope, schema)
        }
        Expr::Case { operand, when_clauses, else_clause, .. } => {
            if let Some(op) = operand {
                if let Some(result) = find_symbol_in_expr(op, source, offset, scope, schema) {
                    return Some(result);
                }
            }
            for (when_expr, then_expr) in when_clauses {
                if let Some(result) = find_symbol_in_expr(when_expr, source, offset, scope, schema) {
                    return Some(result);
                }
                if let Some(result) = find_symbol_in_expr(then_expr, source, offset, scope, schema) {
                    return Some(result);
                }
            }
            if let Some(else_e) = else_clause {
                return find_symbol_in_expr(else_e, source, offset, scope, schema);
            }
            None
        }
        Expr::Cast { expr: e, .. } => {
            find_symbol_in_expr(e, source, offset, scope, schema)
        }
        Expr::FunctionCall { args, filter, .. } => {
            for arg in args {
                if let Some(result) = find_symbol_in_expr(arg, source, offset, scope, schema) {
                    return Some(result);
                }
            }
            if let Some(f) = filter {
                return find_symbol_in_expr(f, source, offset, scope, schema);
            }
            None
        }
        Expr::Collate { expr: e, .. } => {
            find_symbol_in_expr(e, source, offset, scope, schema)
        }
        Expr::Subquery { query, .. } => {
            find_symbol_in_select(query, source, offset, schema)
        }
        Expr::Exists { query, .. } => {
            find_symbol_in_select(query, source, offset, schema)
        }
        // Leaf expressions - no symbols to find
        _ => None,
    }
}

/// Format hover content for a resolved symbol
pub fn format_hover_content(symbol: &ResolvedSymbol, schema: Option<&Schema>) -> String {
    match symbol {
        ResolvedSymbol::TableAlias { alias, table_name, .. } => {
            let mut content = format!("**{}** (alias for `{}`)", alias, table_name);

            // Add table documentation if available
            if let Some(schema) = schema {
                if let Some(table_info) = schema.get_table(table_name) {
                    // Add table description
                    if let Some(ref doc) = table_info.doc {
                        if !doc.description.is_empty() {
                            content.push_str("\n\n");
                            content.push_str(&doc.description);
                        }
                        // Add doc tags
                        format_doc_tags(&mut content, doc);
                    }

                    // Add column information with docs
                    content.push_str("\n\n**Columns:**\n");
                    for col in &table_info.original_columns {
                        let col_doc = table_info.column_docs.get(&col.to_lowercase());
                        if let Some(doc) = col_doc {
                            content.push_str(&format!("- {} — {}\n", col, doc.description));
                        } else {
                            content.push_str(&format!("- {}\n", col));
                        }
                    }
                }
            }
            content
        }
        ResolvedSymbol::Column { name, table_name, qualifier } => {
            let mut content = if let Some(table) = table_name {
                format!("**{}** (column from `{}`)", name, table)
            } else if let Some(q) = qualifier {
                format!("**{}** (column, qualified as `{}.{}`)", name, q, name)
            } else {
                format!("**{}** (column)", name)
            };

            // Add column documentation if we have table info
            if let (Some(table), Some(schema)) = (table_name, schema) {
                if let Some(table_info) = schema.get_table(table) {
                    if let Some(doc) = table_info.column_docs.get(&name.to_lowercase()) {
                        if !doc.description.is_empty() {
                            content.push_str("\n\n");
                            content.push_str(&doc.description);
                        }
                        // Add doc tags (especially @example)
                        format_doc_tags(&mut content, doc);
                    }
                }
            }

            content
        }
        ResolvedSymbol::Table { name, .. } => {
            let mut content = format!("**{}** (table)", name);

            // Add table documentation and column information if available
            if let Some(schema) = schema {
                if let Some(table_info) = schema.get_table(name) {
                    // Add table description
                    if let Some(ref doc) = table_info.doc {
                        if !doc.description.is_empty() {
                            content.push_str("\n\n");
                            content.push_str(&doc.description);
                        }
                        // Add doc tags
                        format_doc_tags(&mut content, doc);
                    }

                    // Add column information with docs
                    content.push_str("\n\n**Columns:**\n");
                    for col in &table_info.original_columns {
                        let col_doc = table_info.column_docs.get(&col.to_lowercase());
                        if let Some(doc) = col_doc {
                            content.push_str(&format!("- {} — {}\n", col, doc.description));
                        } else {
                            content.push_str(&format!("- {}\n", col));
                        }
                    }
                }
            }
            content
        }
        ResolvedSymbol::ColumnAlias { alias, .. } => {
            format!("**{}** (column alias)", alias)
        }
        ResolvedSymbol::Cte { name, columns, .. } => {
            let mut content = format!("**{}** (Common Table Expression)", name);
            if !columns.is_empty() {
                content.push_str("\n\n**Columns:**\n");
                for col in columns {
                    content.push_str(&format!("- {}\n", col));
                }
            }
            content
        }
    }
}

/// Format doc tags for hover content
fn format_doc_tags(content: &mut String, doc: &solite_ast::DocComment) {
    // Priority order for displaying tags
    let tag_labels = [
        ("details", "Details"),
        ("source", "Source"),
        ("schema", "Schema"),
        ("example", "Example"),
        ("value", "Value"),
    ];

    for (tag, label) in tag_labels {
        if let Some(values) = doc.tags.get(tag) {
            for value in values {
                if !value.is_empty() {
                    content.push_str(&format!("\n\n**{}:** {}", label, value));
                }
            }
        }
    }
}

/// Get the definition location for a resolved symbol (for goto-definition)
pub fn get_definition_span(symbol: &ResolvedSymbol) -> Option<Span> {
    match symbol {
        ResolvedSymbol::TableAlias { definition_span, .. } => Some(definition_span.clone()),
        ResolvedSymbol::ColumnAlias { definition_span, .. } => Some(definition_span.clone()),
        ResolvedSymbol::Cte { definition_span, .. } => Some(definition_span.clone()),
        // Tables and columns don't have in-document definitions (they're in schema)
        ResolvedSymbol::Table { .. } => None,
        ResolvedSymbol::Column { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solite_parser::parse_program;

    fn find_symbol(sql: &str, offset: usize) -> Option<(ResolvedSymbol, Span)> {
        let program = parse_program(sql).ok()?;
        let stmt = find_statement_at_offset(&program, offset)?;
        find_symbol_at_offset(stmt, sql, offset, None)
    }

    #[test]
    fn test_resolve_simple_column() {
        let sql = "SELECT id FROM users";
        //                ^7
        let result = find_symbol(sql, 7);
        assert!(result.is_some());
        let (symbol, _) = result.unwrap();
        match symbol {
            ResolvedSymbol::Column { name, table_name, qualifier } => {
                assert_eq!(name, "id");
                assert!(table_name.is_none());
                assert!(qualifier.is_none());
            }
            _ => panic!("Expected Column, got {:?}", symbol),
        }
    }

    #[test]
    fn test_resolve_qualified_column() {
        let sql = "SELECT u.id FROM users AS u";
        //                ^9 (on 'id' after 'u.')
        let result = find_symbol(sql, 9);
        assert!(result.is_some());
        let (symbol, _) = result.unwrap();
        match symbol {
            ResolvedSymbol::Column { name, table_name, qualifier } => {
                assert_eq!(name, "id");
                assert_eq!(table_name, Some("users".to_string()));
                assert_eq!(qualifier, Some("u".to_string()));
            }
            _ => panic!("Expected Column, got {:?}", symbol),
        }
    }

    #[test]
    fn test_resolve_table_alias_in_from() {
        let sql = "SELECT * FROM users AS u";
        //                           ^22 (on 'u')
        // Note: This tests finding the table reference, which includes the alias
        let result = find_symbol(sql, 14);
        assert!(result.is_some());
        let (symbol, _) = result.unwrap();
        match symbol {
            ResolvedSymbol::Table { name, .. } => {
                assert_eq!(name, "users");
            }
            _ => panic!("Expected Table, got {:?}", symbol),
        }
    }

    #[test]
    fn test_resolve_column_in_where() {
        let sql = "SELECT * FROM users WHERE name = 'test'";
        //                                ^26 (on 'name')
        let result = find_symbol(sql, 26);
        assert!(result.is_some());
        let (symbol, _) = result.unwrap();
        match symbol {
            ResolvedSymbol::Column { name, .. } => {
                assert_eq!(name, "name");
            }
            _ => panic!("Expected Column, got {:?}", symbol),
        }
    }

    #[test]
    fn test_resolve_qualified_column_in_join() {
        let sql = "SELECT * FROM users u JOIN orders o ON u.id = o.user_id";
        //                                                   ^44 (on 'user_id')
        let result = find_symbol(sql, 48);
        assert!(result.is_some());
        let (symbol, _) = result.unwrap();
        match symbol {
            ResolvedSymbol::Column { name, table_name, qualifier } => {
                assert_eq!(name, "user_id");
                assert_eq!(table_name, Some("orders".to_string()));
                assert_eq!(qualifier, Some("o".to_string()));
            }
            _ => panic!("Expected Column, got {:?}", symbol),
        }
    }

    #[test]
    fn test_hover_content_table_alias() {
        let symbol = ResolvedSymbol::TableAlias {
            alias: "u".to_string(),
            table_name: "users".to_string(),
            definition_span: Span::new(0, 1),
        };
        let content = format_hover_content(&symbol, None);
        assert!(content.contains("**u**"));
        assert!(content.contains("users"));
    }

    #[test]
    fn test_hover_content_column() {
        let symbol = ResolvedSymbol::Column {
            name: "id".to_string(),
            table_name: Some("users".to_string()),
            qualifier: Some("u".to_string()),
        };
        let content = format_hover_content(&symbol, None);
        assert!(content.contains("**id**"));
        assert!(content.contains("users"));
    }

    #[test]
    fn test_definition_span_for_alias() {
        let symbol = ResolvedSymbol::TableAlias {
            alias: "u".to_string(),
            table_name: "users".to_string(),
            definition_span: Span::new(20, 21),
        };
        let span = get_definition_span(&symbol);
        assert!(span.is_some());
        assert_eq!(span.unwrap().start, 20);
    }

    #[test]
    fn test_definition_span_for_column() {
        let symbol = ResolvedSymbol::Column {
            name: "id".to_string(),
            table_name: None,
            qualifier: None,
        };
        let span = get_definition_span(&symbol);
        assert!(span.is_none()); // Columns don't have in-document definitions
    }

    #[test]
    fn test_format_hover_content_table_with_doc() {
        use std::collections::HashMap;
        use solite_ast::DocComment;

        // Create a schema with a documented table
        let mut schema = Schema::new();
        let mut table_doc = DocComment::with_description("All students at Foo University.");
        table_doc.tags.insert("details".to_string(), vec!["https://foo.edu/students".to_string()]);

        let mut column_docs = HashMap::new();
        let mut col_doc = DocComment::with_description("Student ID assigned at orientation");
        col_doc.tags.insert("example".to_string(), vec!["'S10483'".to_string()]);
        column_docs.insert("student_id".to_string(), col_doc);

        schema.add_table_with_doc(
            "students",
            vec!["student_id".to_string(), "name".to_string()],
            false,
            Some(table_doc),
            column_docs,
        );

        // Test table hover
        let table_symbol = ResolvedSymbol::Table {
            name: "students".to_string(),
            span: Span::new(0, 8),
        };
        let hover = format_hover_content(&table_symbol, Some(&schema));
        assert!(hover.contains("**students** (table)"));
        assert!(hover.contains("All students at Foo University."));
        assert!(hover.contains("**Details:** https://foo.edu/students"));
        assert!(hover.contains("student_id — Student ID assigned at orientation"));
    }

    #[test]
    fn test_format_hover_content_column_with_doc() {
        use std::collections::HashMap;
        use solite_ast::DocComment;

        // Create a schema with documented columns
        let mut schema = Schema::new();
        let mut column_docs = HashMap::new();
        let mut col_doc = DocComment::with_description("Student ID assigned at orientation");
        col_doc.tags.insert("example".to_string(), vec!["'S10483'".to_string()]);
        column_docs.insert("student_id".to_string(), col_doc);

        schema.add_table_with_doc(
            "students",
            vec!["student_id".to_string()],
            false,
            None,
            column_docs,
        );

        // Test column hover
        let col_symbol = ResolvedSymbol::Column {
            name: "student_id".to_string(),
            table_name: Some("students".to_string()),
            qualifier: None,
        };
        let hover = format_hover_content(&col_symbol, Some(&schema));
        assert!(hover.contains("**student_id** (column from `students`)"));
        assert!(hover.contains("Student ID assigned at orientation"));
        assert!(hover.contains("**Example:** 'S10483'"));
    }

    #[test]
    fn test_format_hover_content_no_doc() {
        // Create a schema without documentation
        let mut schema = Schema::new();
        schema.add_table("users", vec!["id".to_string(), "name".to_string()], false);

        // Test table hover without docs
        let table_symbol = ResolvedSymbol::Table {
            name: "users".to_string(),
            span: Span::new(0, 5),
        };
        let hover = format_hover_content(&table_symbol, Some(&schema));
        assert!(hover.contains("**users** (table)"));
        assert!(hover.contains("**Columns:**"));
        assert!(hover.contains("- id"));
        assert!(hover.contains("- name"));
        // Should not have any documentation text
        assert!(!hover.contains("**Details:**"));
        assert!(!hover.contains("**Example:**"));
    }
}
