//! Statement formatting
//!
//! Implementations of FormatNode for SELECT, INSERT, UPDATE, DELETE,
//! and other DML/TCL statements.

use super::{format_alias, format_identifier, format_list, format_qualified_name, FormatNode};
use crate::printer::Printer;
use solite_ast::{
    AnalyzeStmt, AttachStmt, BeginStmt, CommitStmt, CompoundOp, ConflictAction, DeleteStmt,
    DetachStmt, DistinctAll, IndexedBy, InsertSource, InsertStmt, PragmaStmt, PragmaValue,
    ReindexStmt, ReleaseStmt, ResultColumn, RollbackStmt, SavepointStmt, SelectCore, SelectStmt,
    Statement, TransactionType, UpdateAssignment, UpdateStmt, UpsertClause, VacuumStmt,
};

impl FormatNode for Statement {
    fn format(&self, p: &mut Printer) {
        match self {
            Statement::Select(s) => s.format(p),
            Statement::Insert(s) => s.format(p),
            Statement::Update(s) => s.format(p),
            Statement::Delete(s) => s.format(p),
            Statement::CreateTable(s) => s.format(p),
            Statement::CreateIndex(s) => s.format(p),
            Statement::CreateView(s) => s.format(p),
            Statement::CreateTrigger(s) => s.format(p),
            Statement::AlterTable(s) => s.format(p),
            Statement::DropTable(s) => s.format(p),
            Statement::DropIndex(s) => s.format(p),
            Statement::DropView(s) => s.format(p),
            Statement::DropTrigger(s) => s.format(p),
            Statement::Explain {
                query_plan, stmt, ..
            } => {
                p.keyword("EXPLAIN");
                if *query_plan {
                    p.space();
                    p.keyword("QUERY");
                    p.space();
                    p.keyword("PLAN");
                }
                p.newline();
                stmt.format(p);
            }
            Statement::CreateVirtualTable(s) => s.format(p),
            Statement::Begin(s) => s.format(p),
            Statement::Commit(s) => s.format(p),
            Statement::Rollback(s) => s.format(p),
            Statement::Savepoint(s) => s.format(p),
            Statement::Release(s) => s.format(p),
            Statement::Vacuum(s) => s.format(p),
            Statement::Analyze(s) => s.format(p),
            Statement::Reindex(s) => s.format(p),
            Statement::Attach(s) => s.format(p),
            Statement::Detach(s) => s.format(p),
            Statement::Pragma(s) => s.format(p),
        }
    }
}

/// Check if a result column is simple enough to stay on the same line as SELECT
fn is_simple_column(col: &ResultColumn) -> bool {
    
    match col {
        ResultColumn::Star(_) => true,
        ResultColumn::TableStar { .. } => true,
        ResultColumn::Expr { expr, .. } => is_simple_expr(expr),
    }
}

/// Check if an expression is simple enough to stay on the same line
fn is_simple_expr(expr: &solite_ast::Expr) -> bool {
    use solite_ast::Expr;
    match expr {
        // Literals and identifiers are simple
        Expr::Ident(_, _, _)
        | Expr::Column { .. }
        | Expr::Star(_)
        | Expr::Integer(_, _)
        | Expr::HexInteger(_, _)
        | Expr::Float(_, _)
        | Expr::String(_, _)
        | Expr::Blob(_, _)
        | Expr::Null(_)
        | Expr::BindParam(_, _) => true,

        // Binary expressions are simple if both sides are simple
        Expr::Binary { left, right, .. } => is_simple_expr(left) && is_simple_expr(right),

        // Unary expressions are simple if the operand is simple
        Expr::Unary { expr, .. } => is_simple_expr(expr),

        // Parenthesized expressions are simple if the inner is simple
        Expr::Paren(inner, _) => is_simple_expr(inner),

        // Collate expressions are simple if the inner is simple
        Expr::Collate { expr, .. } => is_simple_expr(expr),

        // Function calls are simple (unless they have complex nested expressions)
        Expr::FunctionCall { args, filter, over, .. } => {
            filter.is_none() && over.is_none() && args.iter().all(is_simple_expr)
        }

        // Cast is simple if the inner is simple
        Expr::Cast { expr, .. } => is_simple_expr(expr),

        // Complex expressions that should be on their own line
        Expr::Case { .. }
        | Expr::Subquery { .. }
        | Expr::InSelect { .. }
        | Expr::Exists { .. } => false,

        // Other expressions - treat as simple for now
        Expr::Between { .. }
        | Expr::InList { .. }
        | Expr::IsNull { .. }
        | Expr::Like { .. }
        | Expr::Raise { .. } => true,
    }
}

impl FormatNode for SelectStmt {
    fn format(&self, p: &mut Printer) {
        // WITH clause
        if let Some(with) = &self.with_clause {
            with.format(p);
        }

        // SELECT
        p.keyword("SELECT");

        // DISTINCT / ALL
        match self.distinct {
            DistinctAll::Distinct => {
                p.space();
                p.keyword("DISTINCT");
            }
            DistinctAll::All => {}
        }

        // Determine formatting style:
        // - Single simple column (identifier, *, etc.): stays on same line as SELECT
        // - Single complex column (CASE, subquery, etc.): on its own line
        // - Multiple columns: each on its own line, indented
        let single_simple_column =
            self.columns.len() == 1 && is_simple_column(&self.columns[0]);

        // Is this a truly simple query? (single simple column, just FROM, nothing else)
        let is_simple = single_simple_column
            && self.where_clause.is_none()
            && self.group_by.is_none()
            && self.having.is_none()
            && self.order_by.is_none()
            && self.limit.is_none()
            && self.compounds.is_empty();

        // Columns formatting
        if single_simple_column {
            // Single simple column stays on same line as SELECT
            p.space();
            self.columns[0].format(p);
        } else {
            // Multiple columns or complex single column: each on its own line
            p.newline();
            p.indent();
            for (i, col) in self.columns.iter().enumerate() {
                if i > 0 {
                    p.list_separator(true);
                }
                col.format(p);
            }
            p.dedent();
        }

        // FROM clause
        if let Some(from) = &self.from {
            if is_simple {
                p.space();
            } else {
                p.newline();
            }
            from.format(p);
        }

        // WHERE clause
        if let Some(where_expr) = &self.where_clause {
            p.newline();
            p.keyword("WHERE");
            p.space();
            where_expr.format(p);
        }

        // GROUP BY
        if let Some(group_by) = &self.group_by {
            p.newline();
            p.keyword("GROUP");
            p.space();
            p.keyword("BY");
            p.space();
            format_list(p, group_by, false, |p, e| e.format(p));
        }

        // HAVING
        if let Some(having) = &self.having {
            p.newline();
            p.keyword("HAVING");
            p.space();
            having.format(p);
        }

        // Compound operations
        for (op, core) in &self.compounds {
            p.newline();
            p.keyword(match op {
                CompoundOp::Union => "UNION",
                CompoundOp::UnionAll => "UNION ALL",
                CompoundOp::Intersect => "INTERSECT",
                CompoundOp::Except => "EXCEPT",
            });
            p.newline();
            core.format(p);
        }

        // ORDER BY
        if let Some(order_by) = &self.order_by {
            p.newline();
            p.keyword("ORDER");
            p.space();
            p.keyword("BY");
            p.space();
            format_list(p, order_by, false, |p, o| o.format(p));
        }

        // LIMIT
        if let Some(limit) = &self.limit {
            p.newline();
            limit.format(p);
        }
    }
}

impl FormatNode for SelectCore {
    fn format(&self, p: &mut Printer) {
        p.keyword("SELECT");

        match self.distinct {
            DistinctAll::Distinct => {
                p.space();
                p.keyword("DISTINCT");
            }
            DistinctAll::All => {}
        }

        p.newline();
        p.indent();
        for (i, col) in self.columns.iter().enumerate() {
            if i > 0 {
                p.list_separator(true);
            }
            col.format(p);
        }
        p.dedent();

        if let Some(from) = &self.from {
            p.newline();
            from.format(p);
        }

        if let Some(where_expr) = &self.where_clause {
            p.newline();
            p.keyword("WHERE");
            p.space();
            where_expr.format(p);
        }

        if let Some(group_by) = &self.group_by {
            p.newline();
            p.keyword("GROUP");
            p.space();
            p.keyword("BY");
            p.space();
            format_list(p, group_by, false, |p, e| e.format(p));
        }

        if let Some(having) = &self.having {
            p.newline();
            p.keyword("HAVING");
            p.space();
            having.format(p);
        }
    }
}

impl FormatNode for InsertStmt {
    fn format(&self, p: &mut Printer) {
        // WITH clause
        if let Some(with) = &self.with_clause {
            with.format(p);
        }

        // INSERT or REPLACE
        if let Some(action) = &self.or_action {
            if matches!(action, ConflictAction::Replace) {
                p.keyword("REPLACE");
            } else {
                p.keyword("INSERT");
                p.space();
                p.keyword("OR");
                p.space();
                format_conflict_action(p, action);
            }
        } else {
            p.keyword("INSERT");
        }

        p.space();
        p.keyword("INTO");
        p.space();

        // Table name
        format_qualified_name(p, &self.schema, &self.table_name);

        // Alias
        format_alias(p, &self.alias, true);

        // Column list
        if let Some(cols) = &self.columns {
            p.write("(");
            format_list(p, cols, false, |p, c| format_identifier(p, c));
            p.write(")");
        }

        // Source
        match &self.source {
            InsertSource::Values(rows) => {
                p.newline();
                p.keyword("VALUES");
                p.newline();
                p.indent();
                for (i, row) in rows.iter().enumerate() {
                    if i > 0 {
                        p.list_separator(true);
                    }
                    p.write("(");
                    format_list(p, row, false, |p, e| e.format(p));
                    p.write(")");
                }
                p.dedent();
            }
            InsertSource::Select(query) => {
                p.newline();
                query.format(p);
            }
            InsertSource::DefaultValues => {
                p.space();
                p.keyword("DEFAULT");
                p.space();
                p.keyword("VALUES");
            }
        }

        // UPSERT clause
        if let Some(upsert) = &self.upsert {
            p.newline();
            upsert.format(p);
        }

        // RETURNING clause
        if let Some(returning) = &self.returning {
            p.newline();
            p.keyword("RETURNING");
            p.space();
            format_list(p, returning, false, |p, r| r.format(p));
        }
    }
}

impl FormatNode for UpsertClause {
    fn format(&self, p: &mut Printer) {
        p.keyword("ON");
        p.space();
        p.keyword("CONFLICT");

        if let Some(target) = &self.target {
            p.write("(");
            format_list(p, &target.columns, false, |p, c| c.column.format(p));
            p.write(")");
            if let Some(where_expr) = &target.where_clause {
                p.space();
                p.keyword("WHERE");
                p.space();
                where_expr.format(p);
            }
        }

        p.space();
        p.keyword("DO");
        p.space();

        match self.action {
            ConflictAction::Nothing => {
                p.keyword("NOTHING");
            }
            ConflictAction::Update => {
                p.keyword("UPDATE");
                p.space();
                p.keyword("SET");

                if let Some(updates) = &self.update_set {
                    p.newline();
                    p.indent();
                    for (i, (cols, expr)) in updates.iter().enumerate() {
                        if i > 0 {
                            p.list_separator(true);
                        }
                        if cols.len() == 1 {
                            format_identifier(p, &cols[0]);
                        } else {
                            p.write("(");
                            format_list(p, cols, false, |p, c| format_identifier(p, c));
                            p.write(")");
                        }
                        p.write(" = ");
                        expr.format(p);
                    }
                    p.dedent();
                }

                if let Some(where_expr) = &self.update_where {
                    p.newline();
                    p.keyword("WHERE");
                    p.space();
                    where_expr.format(p);
                }
            }
            _ => {}
        }
    }
}

impl FormatNode for UpdateStmt {
    fn format(&self, p: &mut Printer) {
        // WITH clause
        if let Some(with) = &self.with_clause {
            with.format(p);
        }

        p.keyword("UPDATE");

        // OR action
        if let Some(action) = &self.or_action {
            p.space();
            p.keyword("OR");
            p.space();
            format_conflict_action(p, action);
        }

        p.space();
        format_qualified_name(p, &self.schema, &self.table_name);
        format_alias(p, &self.alias, true);

        // INDEXED BY
        if let Some(indexed) = &self.indexed {
            p.space();
            match indexed {
                IndexedBy::Index(name) => {
                    p.keyword("INDEXED");
                    p.space();
                    p.keyword("BY");
                    p.space();
                    format_identifier(p, name);
                }
                IndexedBy::NotIndexed => {
                    p.keyword("NOT");
                    p.space();
                    p.keyword("INDEXED");
                }
            }
        }

        // SET
        p.newline();
        p.keyword("SET");
        p.newline();
        p.indent();
        for (i, assignment) in self.assignments.iter().enumerate() {
            if i > 0 {
                p.list_separator(true);
            }
            assignment.format(p);
        }
        p.dedent();

        // FROM
        if let Some(from) = &self.from {
            p.newline();
            from.format(p);
        }

        // WHERE
        if let Some(where_expr) = &self.where_clause {
            p.newline();
            p.keyword("WHERE");
            p.space();
            where_expr.format(p);
        }

        // RETURNING
        if let Some(returning) = &self.returning {
            p.newline();
            p.keyword("RETURNING");
            p.space();
            format_list(p, returning, false, |p, r| r.format(p));
        }

        // ORDER BY
        if let Some(order_by) = &self.order_by {
            p.newline();
            p.keyword("ORDER");
            p.space();
            p.keyword("BY");
            p.space();
            format_list(p, order_by, false, |p, o| o.format(p));
        }

        // LIMIT/OFFSET
        if let Some(limit) = &self.limit {
            p.newline();
            p.keyword("LIMIT");
            p.space();
            limit.format(p);
            if let Some(offset) = &self.offset {
                p.space();
                p.keyword("OFFSET");
                p.space();
                offset.format(p);
            }
        }
    }
}

impl FormatNode for UpdateAssignment {
    fn format(&self, p: &mut Printer) {
        if self.columns.len() == 1 {
            format_identifier(p, &self.columns[0]);
        } else {
            p.write("(");
            format_list(p, &self.columns, false, |p, c| format_identifier(p, c));
            p.write(")");
        }
        p.write(" = ");
        self.expr.format(p);
    }
}

impl FormatNode for DeleteStmt {
    fn format(&self, p: &mut Printer) {
        // WITH clause
        if let Some(with) = &self.with_clause {
            with.format(p);
        }

        p.keyword("DELETE");
        p.space();
        p.keyword("FROM");
        p.space();

        format_qualified_name(p, &self.schema, &self.table_name);
        format_alias(p, &self.alias, true);

        // INDEXED BY
        if let Some(indexed) = &self.indexed {
            p.space();
            match indexed {
                IndexedBy::Index(name) => {
                    p.keyword("INDEXED");
                    p.space();
                    p.keyword("BY");
                    p.space();
                    format_identifier(p, name);
                }
                IndexedBy::NotIndexed => {
                    p.keyword("NOT");
                    p.space();
                    p.keyword("INDEXED");
                }
            }
        }

        // WHERE
        if let Some(where_expr) = &self.where_clause {
            p.newline();
            p.keyword("WHERE");
            p.space();
            where_expr.format(p);
        }

        // RETURNING
        if let Some(returning) = &self.returning {
            p.newline();
            p.keyword("RETURNING");
            p.space();
            format_list(p, returning, false, |p, r| r.format(p));
        }

        // ORDER BY
        if let Some(order_by) = &self.order_by {
            p.newline();
            p.keyword("ORDER");
            p.space();
            p.keyword("BY");
            p.space();
            format_list(p, order_by, false, |p, o| o.format(p));
        }

        // LIMIT/OFFSET
        if let Some(limit) = &self.limit {
            p.newline();
            p.keyword("LIMIT");
            p.space();
            limit.format(p);
            if let Some(offset) = &self.offset {
                p.space();
                p.keyword("OFFSET");
                p.space();
                offset.format(p);
            }
        }
    }
}

fn format_conflict_action(p: &mut Printer, action: &ConflictAction) {
    p.keyword(match action {
        ConflictAction::Rollback => "ROLLBACK",
        ConflictAction::Abort => "ABORT",
        ConflictAction::Fail => "FAIL",
        ConflictAction::Ignore => "IGNORE",
        ConflictAction::Replace => "REPLACE",
        ConflictAction::Nothing => "NOTHING",
        ConflictAction::Update => "UPDATE",
    });
}

// TCL Statements

impl FormatNode for BeginStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("BEGIN");
        if let Some(tt) = &self.transaction_type {
            p.space();
            p.keyword(match tt {
                TransactionType::Deferred => "DEFERRED",
                TransactionType::Immediate => "IMMEDIATE",
                TransactionType::Exclusive => "EXCLUSIVE",
            });
        }
    }
}

impl FormatNode for CommitStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("COMMIT");
    }
}

impl FormatNode for RollbackStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("ROLLBACK");
        if let Some(savepoint) = &self.savepoint {
            p.space();
            p.keyword("TO");
            p.space();
            p.keyword("SAVEPOINT");
            p.space();
            format_identifier(p, savepoint);
        }
    }
}

impl FormatNode for SavepointStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("SAVEPOINT");
        p.space();
        format_identifier(p, &self.name);
    }
}

impl FormatNode for ReleaseStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("RELEASE");
        p.space();
        p.keyword("SAVEPOINT");
        p.space();
        format_identifier(p, &self.name);
    }
}

// Database Management Statements

impl FormatNode for VacuumStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("VACUUM");
        if let Some(schema) = &self.schema {
            p.space();
            format_identifier(p, schema);
        }
        if let Some(file) = &self.into_file {
            p.space();
            p.keyword("INTO");
            p.space();
            p.write(&format!("'{}'", file.replace("'", "''")));
        }
    }
}

impl FormatNode for AnalyzeStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("ANALYZE");
        if let Some(target) = &self.target {
            p.space();
            format_qualified_name(p, &target.schema, &target.name);
        }
    }
}

impl FormatNode for ReindexStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("REINDEX");
        if let Some(target) = &self.target {
            p.space();
            format_qualified_name(p, &target.schema, &target.name);
        }
    }
}

impl FormatNode for AttachStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("ATTACH");
        p.space();
        p.keyword("DATABASE");
        p.space();
        self.expr.format(p);
        p.space();
        p.keyword("AS");
        p.space();
        format_identifier(p, &self.schema_name);
    }
}

impl FormatNode for DetachStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("DETACH");
        p.space();
        p.keyword("DATABASE");
        p.space();
        format_identifier(p, &self.schema_name);
    }
}

impl FormatNode for PragmaStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("PRAGMA");
        p.space();
        format_qualified_name(p, &self.schema, &self.name);

        if let Some(value) = &self.value {
            match value {
                PragmaValue::Assign(expr) => {
                    p.write(" = ");
                    expr.format(p);
                }
                PragmaValue::Call(expr) => {
                    p.write("(");
                    expr.format(p);
                    p.write(")");
                }
            }
        }
    }
}
