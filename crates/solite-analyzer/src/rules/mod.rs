//! Lint rules for SQL analysis
//!
//! This module provides a trait-based system for implementing lint rules
//! that can analyze SQL expressions and statements.

use once_cell::sync::Lazy;
use solite_ast::{Expr, Span, Statement};

// Submodules
pub mod config;
pub mod double_quoted;
pub mod empty_blob;
pub mod missing_as;
pub mod suppressions;

// Re-export submodule types
pub use config::LintConfig;
pub use double_quoted::DoubleQuotedString;
pub use empty_blob::EmptyBlobLiteral;
pub use missing_as::MissingAsAlias;
pub use suppressions::Suppressions;

/// Severity level for lint rules
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuleSeverity {
    /// Rule is disabled
    Off,
    /// Rule produces a warning
    #[default]
    Warning,
    /// Rule produces an error
    Error,
}

/// A diagnostic produced by a lint rule
#[derive(Debug, Clone)]
pub struct LintDiagnostic {
    /// The rule ID that produced this diagnostic (e.g., "empty-blob-literal")
    pub rule_id: &'static str,
    /// The diagnostic message
    pub message: String,
    /// The source span where the issue was found
    pub span: Span,
    /// The severity of this diagnostic
    pub severity: RuleSeverity,
}

/// A fix that can be applied to resolve a lint diagnostic
#[derive(Debug, Clone)]
pub struct Fix {
    /// The span of text to replace
    pub span: Span,
    /// The replacement text
    pub replacement: String,
}

/// Context provided to lint rules during checking
pub struct LintContext<'a> {
    /// The source code being analyzed
    pub source: &'a str,
    /// Active suppressions for the current file
    pub suppressions: &'a Suppressions,
    /// Configuration for lint rules
    pub config: &'a LintConfig,
}

/// Result of running a lint rule, containing the diagnostic and optional fix
#[derive(Debug, Clone)]
pub struct LintResult {
    /// The diagnostic produced by the rule
    pub diagnostic: LintDiagnostic,
    /// An optional fix that can be applied
    pub fix: Option<Fix>,
}

/// Trait for implementing lint rules
///
/// Each lint rule should implement this trait to define its behavior.
/// Rules can check expressions, statements, or both.
pub trait LintRule: Send + Sync {
    /// Returns the unique identifier for this rule (e.g., "empty-blob-literal")
    fn id(&self) -> &'static str;

    /// Returns the human-readable name of this rule (e.g., "Empty Blob Literal")
    fn name(&self) -> &'static str;

    /// Returns a description of what this rule checks for
    fn description(&self) -> &'static str;

    /// Returns the default severity for this rule
    fn default_severity(&self) -> RuleSeverity;

    /// Check an expression for lint issues
    ///
    /// Override this method to implement expression-level checking.
    /// The default implementation returns an empty vec.
    #[allow(unused_variables)]
    fn check_expr(&self, expr: &Expr, ctx: &LintContext) -> Vec<LintDiagnostic> {
        vec![]
    }

    /// Check a statement for lint issues
    ///
    /// Override this method to implement statement-level checking.
    /// The default implementation returns an empty vec.
    #[allow(unused_variables)]
    fn check_stmt(&self, stmt: &Statement, ctx: &LintContext) -> Vec<LintDiagnostic> {
        vec![]
    }

    /// Returns whether this rule can provide automatic fixes
    fn is_fixable(&self) -> bool {
        false
    }

    /// Attempt to generate a fix for a diagnostic
    ///
    /// Override this method if the rule is fixable.
    /// The default implementation returns None.
    #[allow(unused_variables)]
    fn fix(&self, diagnostic: &LintDiagnostic, source: &str) -> Option<Fix> {
        None
    }
}

/// Registry of all available lint rules
pub static RULES: Lazy<Vec<Box<dyn LintRule>>> = Lazy::new(|| {
    vec![
        Box::new(EmptyBlobLiteral),
        Box::new(DoubleQuotedString),
        Box::new(MissingAsAlias),
    ]
});

/// Get a reference to all available lint rules
pub fn get_all_rules() -> &'static [Box<dyn LintRule>] {
    &RULES
}

/// Get a specific rule by its ID
pub fn get_rule_by_id(id: &str) -> Option<&'static dyn LintRule> {
    RULES.iter().find(|r| r.id() == id).map(|r| r.as_ref())
}

// ========================================
// AST Walker
// ========================================

/// Recursively walk an expression and apply lint rules
///
/// This helper traverses the entire expression tree, calling the provided
/// check function on each expression node encountered.
pub fn walk_expr<F>(expr: &Expr, ctx: &LintContext, mut check: F)
where
    F: FnMut(&Expr, &LintContext),
{
    walk_expr_recursive(expr, ctx, &mut check);
}

fn walk_expr_recursive<F>(expr: &Expr, ctx: &LintContext, check: &mut F)
where
    F: FnMut(&Expr, &LintContext),
{
    // First check the current expression
    check(expr, ctx);

    // Then recurse into child expressions
    match expr {
        // Leaf nodes - no children to visit
        Expr::Integer(_, _)
        | Expr::HexInteger(_, _)
        | Expr::Float(_, _)
        | Expr::String(_, _)
        | Expr::Blob(_, _)
        | Expr::Null(_)
        | Expr::Ident(_, _, _)
        | Expr::Star(_)
        | Expr::BindParam(_, _)
        | Expr::Column { .. } => {}

        // Single child expressions
        Expr::Unary { expr: child, .. } => {
            walk_expr_recursive(child, ctx, check);
        }
        Expr::Paren(child, _) => {
            walk_expr_recursive(child, ctx, check);
        }
        Expr::IsNull { expr: child, .. } => {
            walk_expr_recursive(child, ctx, check);
        }
        Expr::Cast { expr: child, .. } => {
            walk_expr_recursive(child, ctx, check);
        }
        Expr::Collate { expr: child, .. } => {
            walk_expr_recursive(child, ctx, check);
        }

        // Binary expressions
        Expr::Binary { left, right, .. } => {
            walk_expr_recursive(left, ctx, check);
            walk_expr_recursive(right, ctx, check);
        }

        // BETWEEN expression
        Expr::Between {
            expr, low, high, ..
        } => {
            walk_expr_recursive(expr, ctx, check);
            walk_expr_recursive(low, ctx, check);
            walk_expr_recursive(high, ctx, check);
        }

        // IN with list
        Expr::InList { expr, list, .. } => {
            walk_expr_recursive(expr, ctx, check);
            for item in list {
                walk_expr_recursive(item, ctx, check);
            }
        }

        // IN with subquery
        Expr::InSelect { expr, query, .. } => {
            walk_expr_recursive(expr, ctx, check);
            walk_select_stmt(query, ctx, check);
        }

        // Scalar subquery
        Expr::Subquery { query, .. } => {
            walk_select_stmt(query, ctx, check);
        }

        // EXISTS subquery
        Expr::Exists { query, .. } => {
            walk_select_stmt(query, ctx, check);
        }

        // LIKE/GLOB/REGEXP/MATCH
        Expr::Like {
            expr,
            pattern,
            escape,
            ..
        } => {
            walk_expr_recursive(expr, ctx, check);
            walk_expr_recursive(pattern, ctx, check);
            if let Some(esc) = escape {
                walk_expr_recursive(esc, ctx, check);
            }
        }

        // CASE expression
        Expr::Case {
            operand,
            when_clauses,
            else_clause,
            ..
        } => {
            if let Some(op) = operand {
                walk_expr_recursive(op, ctx, check);
            }
            for (when_expr, then_expr) in when_clauses {
                walk_expr_recursive(when_expr, ctx, check);
                walk_expr_recursive(then_expr, ctx, check);
            }
            if let Some(else_expr) = else_clause {
                walk_expr_recursive(else_expr, ctx, check);
            }
        }

        // Function call
        Expr::FunctionCall {
            args, filter, over, ..
        } => {
            for arg in args {
                walk_expr_recursive(arg, ctx, check);
            }
            if let Some(f) = filter {
                walk_expr_recursive(f, ctx, check);
            }
            if let Some(window) = over {
                if let Some(partition) = &window.partition_by {
                    for part_expr in partition {
                        walk_expr_recursive(part_expr, ctx, check);
                    }
                }
                if let Some(order) = &window.order_by {
                    for term in order {
                        walk_expr_recursive(&term.expr, ctx, check);
                    }
                }
                if let Some(frame) = &window.frame {
                    if let solite_ast::FrameBound::Preceding(e)
                    | solite_ast::FrameBound::Following(e) = &frame.start
                    {
                        walk_expr_recursive(e, ctx, check);
                    }
                    if let Some(solite_ast::FrameBound::Preceding(e) | solite_ast::FrameBound::Following(e)) = &frame.end {
                    walk_expr_recursive(e, ctx, check);
                }
                }
            }
        }

        // RAISE function
        Expr::Raise { message, .. } => {
            if let Some(msg) = message {
                walk_expr_recursive(msg, ctx, check);
            }
        }
    }
}

/// Walk a SELECT statement and check all expressions within it
fn walk_select_stmt<F>(stmt: &solite_ast::SelectStmt, ctx: &LintContext, check: &mut F)
where
    F: FnMut(&Expr, &LintContext),
{
    // WITH clause
    if let Some(with) = &stmt.with_clause {
        for cte in &with.ctes {
            walk_select_stmt(&cte.select, ctx, check);
        }
    }

    // Result columns
    for col in &stmt.columns {
        if let solite_ast::ResultColumn::Expr { expr, .. } = col {
            walk_expr_recursive(expr, ctx, check);
        }
    }

    // FROM clause
    if let Some(from) = &stmt.from {
        walk_from_clause(from, ctx, check);
    }

    // WHERE clause
    if let Some(where_expr) = &stmt.where_clause {
        walk_expr_recursive(where_expr, ctx, check);
    }

    // GROUP BY
    if let Some(group_by) = &stmt.group_by {
        for expr in group_by {
            walk_expr_recursive(expr, ctx, check);
        }
    }

    // HAVING
    if let Some(having) = &stmt.having {
        walk_expr_recursive(having, ctx, check);
    }

    // Compound operations
    for (_, core) in &stmt.compounds {
        walk_select_core(core, ctx, check);
    }

    // ORDER BY
    if let Some(order_by) = &stmt.order_by {
        for term in order_by {
            walk_expr_recursive(&term.expr, ctx, check);
        }
    }

    // LIMIT
    if let Some(limit) = &stmt.limit {
        walk_expr_recursive(&limit.limit, ctx, check);
        if let Some(offset) = &limit.offset {
            walk_expr_recursive(offset, ctx, check);
        }
    }
}

/// Walk a SELECT core (used in compound operations)
fn walk_select_core<F>(core: &solite_ast::SelectCore, ctx: &LintContext, check: &mut F)
where
    F: FnMut(&Expr, &LintContext),
{
    for col in &core.columns {
        if let solite_ast::ResultColumn::Expr { expr, .. } = col {
            walk_expr_recursive(expr, ctx, check);
        }
    }

    if let Some(from) = &core.from {
        walk_from_clause(from, ctx, check);
    }

    if let Some(where_expr) = &core.where_clause {
        walk_expr_recursive(where_expr, ctx, check);
    }

    if let Some(group_by) = &core.group_by {
        for expr in group_by {
            walk_expr_recursive(expr, ctx, check);
        }
    }

    if let Some(having) = &core.having {
        walk_expr_recursive(having, ctx, check);
    }
}

/// Walk a FROM clause
fn walk_from_clause<F>(from: &solite_ast::FromClause, ctx: &LintContext, check: &mut F)
where
    F: FnMut(&Expr, &LintContext),
{
    for table in &from.tables {
        walk_table_or_subquery(table, ctx, check);
    }
}

/// Walk a table or subquery
fn walk_table_or_subquery<F>(
    table: &solite_ast::TableOrSubquery,
    ctx: &LintContext,
    check: &mut F,
) where
    F: FnMut(&Expr, &LintContext),
{
    match table {
        solite_ast::TableOrSubquery::Table { .. } => {}
        solite_ast::TableOrSubquery::Subquery { query, .. } => {
            walk_select_stmt(query, ctx, check);
        }
        solite_ast::TableOrSubquery::TableList { tables, .. } => {
            for t in tables {
                walk_table_or_subquery(t, ctx, check);
            }
        }
        solite_ast::TableOrSubquery::Join {
            left,
            right,
            constraint,
            ..
        } => {
            walk_table_or_subquery(left, ctx, check);
            walk_table_or_subquery(right, ctx, check);
            if let Some(solite_ast::JoinConstraint::On(expr)) = constraint {
                walk_expr_recursive(expr, ctx, check);
            }
        }
    }
}
