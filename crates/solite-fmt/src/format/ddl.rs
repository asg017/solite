//! DDL statement formatting
//!
//! Implementations of FormatNode for CREATE TABLE, CREATE INDEX, CREATE VIEW,
//! CREATE TRIGGER, DROP statements, and ALTER TABLE.

use super::{format_identifier, format_list, format_qualified_name, FormatNode};
use crate::printer::Printer;
use solite_ast::{
    AlterTableAction, AlterTableStmt, ColumnConstraint, ColumnDef, ConflictAction,
    CreateIndexStmt, CreateTableStmt, CreateTriggerStmt, CreateViewStmt, CreateVirtualTableStmt,
    DefaultValue, Deferrable, DropIndexStmt, DropTableStmt, DropTriggerStmt, DropViewStmt,
    ForeignKeyAction, IndexedColumn, OrderDirection, TableConstraint, TableOption, TriggerEvent,
    TriggerTiming,
};

impl FormatNode for CreateTableStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("CREATE");

        if self.temporary {
            p.space();
            p.keyword("TEMP");
        }

        p.space();
        p.keyword("TABLE");

        if self.if_not_exists {
            p.space();
            p.keyword("IF");
            p.space();
            p.keyword("NOT");
            p.space();
            p.keyword("EXISTS");
        }

        p.space();
        format_qualified_name(p, &self.schema, &self.table_name);

        if let Some(select) = &self.as_select {
            // CREATE TABLE ... AS SELECT
            p.space();
            p.keyword("AS");
            p.newline();
            select.format(p);
        } else {
            // CREATE TABLE with columns
            p.write("(");
            p.newline();
            p.indent();

            // Column definitions
            let total_items = self.columns.len() + self.table_constraints.len();
            for (i, col) in self.columns.iter().enumerate() {
                if i > 0 {
                    p.newline();
                }
                col.format(p);

                let is_last = i == total_items - 1;
                if !is_last {
                    p.write(",");
                    // Emit trailing comment (attached to the comma)
                    p.emit_trailing_comments_in_range(col.span.end, col.span.end + 2);
                } else {
                    // Last item: comment attached to column span
                    p.emit_trailing_comments(col.span.end);
                }
            }

            // Table constraints
            for (i, constraint) in self.table_constraints.iter().enumerate() {
                p.newline();
                constraint.format(p);

                let is_last = i == self.table_constraints.len() - 1;
                if !is_last {
                    p.write(",");
                    p.emit_trailing_comments_in_range(constraint.span().end, constraint.span().end + 2);
                } else {
                    p.emit_trailing_comments(constraint.span().end);
                }
            }

            p.dedent();
            p.newline();
            p.write(")");

            // Table options
            for opt in &self.table_options {
                match opt {
                    TableOption::WithoutRowid => {
                        p.space();
                        p.keyword("WITHOUT");
                        p.space();
                        p.write("ROWID");
                    }
                    TableOption::Strict => {
                        p.space();
                        p.write("STRICT");
                    }
                }
            }
        }
    }
}

impl FormatNode for ColumnDef {
    fn format(&self, p: &mut Printer) {
        format_identifier(p, &self.name);

        if let Some(type_name) = &self.type_name {
            p.space();
            p.write(type_name);
        }

        for constraint in &self.constraints {
            p.space();
            constraint.format(p);
        }

        // Note: trailing comments are handled by the parent (CreateTableStmt)
        // because they come after the comma, not before it.
    }
}

impl FormatNode for ColumnConstraint {
    fn format(&self, p: &mut Printer) {
        match self {
            ColumnConstraint::PrimaryKey {
                order,
                conflict,
                autoincrement,
                ..
            } => {
                p.keyword("PRIMARY");
                p.space();
                p.keyword("KEY");
                if let Some(dir) = order {
                    p.space();
                    p.keyword(match dir {
                        OrderDirection::Asc => "ASC",
                        OrderDirection::Desc => "DESC",
                    });
                }
                format_on_conflict(p, conflict);
                if *autoincrement {
                    p.space();
                    p.keyword("AUTOINCREMENT");
                }
            }
            ColumnConstraint::NotNull { conflict, .. } => {
                p.keyword("NOT");
                p.space();
                p.keyword("NULL");
                format_on_conflict(p, conflict);
            }
            ColumnConstraint::Unique { conflict, .. } => {
                p.keyword("UNIQUE");
                format_on_conflict(p, conflict);
            }
            ColumnConstraint::Check { expr, .. } => {
                p.keyword("CHECK");
                p.write("(");
                expr.format(p);
                p.write(")");
            }
            ColumnConstraint::Default { value, .. } => {
                p.keyword("DEFAULT");
                p.space();
                match value {
                    DefaultValue::Literal(expr) => expr.format(p),
                    DefaultValue::Expr(expr) => {
                        p.write("(");
                        expr.format(p);
                        p.write(")");
                    }
                }
            }
            ColumnConstraint::Collate { collation, .. } => {
                p.keyword("COLLATE");
                p.space();
                p.write(collation);
            }
            ColumnConstraint::ForeignKey {
                foreign_table,
                columns,
                on_delete,
                on_update,
                ..
            } => {
                p.keyword("REFERENCES");
                p.space();
                format_identifier(p, foreign_table);
                if let Some(cols) = columns {
                    p.write("(");
                    format_list(p, cols, false, |p, c| format_identifier(p, c));
                    p.write(")");
                }
                format_fk_action(p, "DELETE", on_delete);
                format_fk_action(p, "UPDATE", on_update);
            }
            ColumnConstraint::Generated { expr, stored, .. } => {
                p.keyword("GENERATED");
                p.space();
                p.keyword("ALWAYS");
                p.space();
                p.keyword("AS");
                p.write("(");
                expr.format(p);
                p.write(")");
                if *stored {
                    p.space();
                    p.keyword("STORED");
                }
            }
        }
    }
}

impl FormatNode for TableConstraint {
    fn format(&self, p: &mut Printer) {
        match self {
            TableConstraint::PrimaryKey {
                name,
                columns,
                conflict,
                ..
            } => {
                if let Some(n) = name {
                    p.keyword("CONSTRAINT");
                    p.space();
                    format_identifier(p, n);
                    p.space();
                }
                p.keyword("PRIMARY");
                p.space();
                p.keyword("KEY");
                p.write("(");
                format_indexed_columns(p, columns);
                p.write(")");
                format_on_conflict(p, conflict);
            }
            TableConstraint::Unique {
                name,
                columns,
                conflict,
                ..
            } => {
                if let Some(n) = name {
                    p.keyword("CONSTRAINT");
                    p.space();
                    format_identifier(p, n);
                    p.space();
                }
                p.keyword("UNIQUE");
                p.write("(");
                format_indexed_columns(p, columns);
                p.write(")");
                format_on_conflict(p, conflict);
            }
            TableConstraint::Check { name, expr, .. } => {
                if let Some(n) = name {
                    p.keyword("CONSTRAINT");
                    p.space();
                    format_identifier(p, n);
                    p.space();
                }
                p.keyword("CHECK");
                p.write("(");
                expr.format(p);
                p.write(")");
            }
            TableConstraint::ForeignKey {
                name,
                columns,
                foreign_table,
                foreign_columns,
                on_delete,
                on_update,
                deferrable,
                ..
            } => {
                if let Some(n) = name {
                    p.keyword("CONSTRAINT");
                    p.space();
                    format_identifier(p, n);
                    p.space();
                }
                p.keyword("FOREIGN");
                p.space();
                p.keyword("KEY");
                p.write("(");
                format_list(p, columns, false, |p, c| format_identifier(p, c));
                p.write(")");
                p.space();
                p.keyword("REFERENCES");
                p.space();
                format_identifier(p, foreign_table);
                if let Some(fcols) = foreign_columns {
                    p.write("(");
                    format_list(p, fcols, false, |p, c| format_identifier(p, c));
                    p.write(")");
                }
                format_fk_action(p, "DELETE", on_delete);
                format_fk_action(p, "UPDATE", on_update);
                if let Some(def) = deferrable {
                    p.space();
                    match def {
                        Deferrable::InitiallyDeferred => {
                            p.keyword("DEFERRABLE");
                            p.space();
                            p.keyword("INITIALLY");
                            p.space();
                            p.keyword("DEFERRED");
                        }
                        Deferrable::InitiallyImmediate => {
                            p.keyword("DEFERRABLE");
                            p.space();
                            p.keyword("INITIALLY");
                            p.space();
                            p.keyword("IMMEDIATE");
                        }
                        Deferrable::NotDeferrable => {
                            p.keyword("NOT");
                            p.space();
                            p.keyword("DEFERRABLE");
                        }
                    }
                }
            }
        }
    }
}

fn format_indexed_columns(p: &mut Printer, cols: &[IndexedColumn]) {
    format_list(p, cols, false, |p, col| {
        col.column.format(p);
        if let Some(collation) = &col.collation {
            p.space();
            p.keyword("COLLATE");
            p.space();
            p.write(collation);
        }
        if let Some(dir) = &col.direction {
            p.space();
            p.keyword(match dir {
                OrderDirection::Asc => "ASC",
                OrderDirection::Desc => "DESC",
            });
        }
    });
}

fn format_on_conflict(p: &mut Printer, conflict: &Option<ConflictAction>) {
    if let Some(action) = conflict {
        p.space();
        p.keyword("ON");
        p.space();
        p.keyword("CONFLICT");
        p.space();
        p.keyword(match action {
            ConflictAction::Rollback => "ROLLBACK",
            ConflictAction::Abort => "ABORT",
            ConflictAction::Fail => "FAIL",
            ConflictAction::Ignore => "IGNORE",
            ConflictAction::Replace => "REPLACE",
            _ => return,
        });
    }
}

fn format_fk_action(p: &mut Printer, event: &str, action: &Option<ForeignKeyAction>) {
    if let Some(act) = action {
        p.space();
        p.keyword("ON");
        p.space();
        p.keyword(event);
        p.space();
        p.keyword(match act {
            ForeignKeyAction::SetNull => "SET NULL",
            ForeignKeyAction::SetDefault => "SET DEFAULT",
            ForeignKeyAction::Cascade => "CASCADE",
            ForeignKeyAction::Restrict => "RESTRICT",
            ForeignKeyAction::NoAction => "NO ACTION",
        });
    }
}

impl FormatNode for CreateIndexStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("CREATE");

        if self.unique {
            p.space();
            p.keyword("UNIQUE");
        }

        p.space();
        p.keyword("INDEX");

        if self.if_not_exists {
            p.space();
            p.keyword("IF");
            p.space();
            p.keyword("NOT");
            p.space();
            p.keyword("EXISTS");
        }

        p.space();
        format_qualified_name(p, &self.schema, &self.index_name);

        p.space();
        p.keyword("ON");
        p.space();
        format_identifier(p, &self.table_name);

        p.write("(");
        format_indexed_columns(p, &self.columns);
        p.write(")");

        if let Some(where_expr) = &self.where_clause {
            p.newline();
            p.keyword("WHERE");
            p.space();
            where_expr.format(p);
        }
    }
}

impl FormatNode for CreateViewStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("CREATE");

        if self.temporary {
            p.space();
            p.keyword("TEMP");
        }

        p.space();
        p.keyword("VIEW");

        if self.if_not_exists {
            p.space();
            p.keyword("IF");
            p.space();
            p.keyword("NOT");
            p.space();
            p.keyword("EXISTS");
        }

        p.space();
        format_qualified_name(p, &self.schema, &self.view_name);

        if let Some(cols) = &self.columns {
            p.write("(");
            format_list(p, cols, false, |p, c| format_identifier(p, c));
            p.write(")");
        }

        p.space();
        p.keyword("AS");
        p.newline();
        self.select.format(p);
    }
}

impl FormatNode for CreateTriggerStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("CREATE");

        if self.temporary {
            p.space();
            p.keyword("TEMP");
        }

        p.space();
        p.keyword("TRIGGER");

        if self.if_not_exists {
            p.space();
            p.keyword("IF");
            p.space();
            p.keyword("NOT");
            p.space();
            p.keyword("EXISTS");
        }

        p.space();
        format_qualified_name(p, &self.schema, &self.trigger_name);

        p.newline();
        p.keyword(match self.timing {
            TriggerTiming::Before => "BEFORE",
            TriggerTiming::After => "AFTER",
            TriggerTiming::InsteadOf => "INSTEAD OF",
        });
        p.space();

        match &self.event {
            TriggerEvent::Delete => p.keyword("DELETE"),
            TriggerEvent::Insert => p.keyword("INSERT"),
            TriggerEvent::Update { columns } => {
                p.keyword("UPDATE");
                if let Some(cols) = columns {
                    p.space();
                    p.keyword("OF");
                    p.space();
                    format_list(p, cols, false, |p, c| format_identifier(p, c));
                }
            }
        }

        p.space();
        p.keyword("ON");
        p.space();
        format_identifier(p, &self.table_name);

        if self.for_each_row {
            p.newline();
            p.keyword("FOR");
            p.space();
            p.keyword("EACH");
            p.space();
            p.keyword("ROW");
        }

        if let Some(when) = &self.when_clause {
            p.newline();
            p.keyword("WHEN");
            p.space();
            when.format(p);
        }

        p.newline();
        p.keyword("BEGIN");
        p.newline();
        p.indent();
        for stmt in &self.body {
            stmt.format(p);
            p.write(";");
            p.newline();
        }
        p.dedent();
        p.keyword("END");
    }
}

impl FormatNode for CreateVirtualTableStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("CREATE");
        p.space();
        p.keyword("VIRTUAL");
        p.space();
        p.keyword("TABLE");

        if self.if_not_exists {
            p.space();
            p.keyword("IF");
            p.space();
            p.keyword("NOT");
            p.space();
            p.keyword("EXISTS");
        }

        p.space();
        format_qualified_name(p, &self.schema, &self.table_name);

        p.space();
        p.keyword("USING");
        p.space();
        p.write(&self.module_name);

        if let Some(args) = &self.module_args {
            p.write("(");
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    p.write(", ");
                }
                p.write(arg);
            }
            p.write(")");
        }
    }
}

// DROP statements

impl FormatNode for DropTableStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("DROP");
        p.space();
        p.keyword("TABLE");
        format_if_exists(p, self.if_exists);
        p.space();
        format_qualified_name(p, &self.schema, &self.table_name);
    }
}

impl FormatNode for DropIndexStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("DROP");
        p.space();
        p.keyword("INDEX");
        format_if_exists(p, self.if_exists);
        p.space();
        format_qualified_name(p, &self.schema, &self.index_name);
    }
}

impl FormatNode for DropViewStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("DROP");
        p.space();
        p.keyword("VIEW");
        format_if_exists(p, self.if_exists);
        p.space();
        format_qualified_name(p, &self.schema, &self.view_name);
    }
}

impl FormatNode for DropTriggerStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("DROP");
        p.space();
        p.keyword("TRIGGER");
        format_if_exists(p, self.if_exists);
        p.space();
        format_qualified_name(p, &self.schema, &self.trigger_name);
    }
}

fn format_if_exists(p: &mut Printer, if_exists: bool) {
    if if_exists {
        p.space();
        p.keyword("IF");
        p.space();
        p.keyword("EXISTS");
    }
}

impl FormatNode for AlterTableStmt {
    fn format(&self, p: &mut Printer) {
        p.keyword("ALTER");
        p.space();
        p.keyword("TABLE");
        p.space();
        format_qualified_name(p, &self.schema, &self.table_name);
        p.space();

        match &self.action {
            AlterTableAction::RenameTo(new_name) => {
                p.keyword("RENAME");
                p.space();
                p.keyword("TO");
                p.space();
                format_identifier(p, new_name);
            }
            AlterTableAction::RenameColumn { old_name, new_name } => {
                p.keyword("RENAME");
                p.space();
                p.keyword("COLUMN");
                p.space();
                format_identifier(p, old_name);
                p.space();
                p.keyword("TO");
                p.space();
                format_identifier(p, new_name);
            }
            AlterTableAction::AddColumn(col_def) => {
                p.keyword("ADD");
                p.space();
                p.keyword("COLUMN");
                p.space();
                col_def.format(p);
            }
            AlterTableAction::DropColumn(col_name) => {
                p.keyword("DROP");
                p.space();
                p.keyword("COLUMN");
                p.space();
                format_identifier(p, col_name);
            }
        }
    }
}
