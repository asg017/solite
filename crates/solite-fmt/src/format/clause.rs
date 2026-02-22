//! Clause formatting
//!
//! Implementations for FROM, WHERE, JOIN, ORDER BY, and other SQL clauses.

use super::{format_alias, format_identifier, format_list, format_qualified_name, FormatNode};
use crate::printer::Printer;
use solite_ast::{
    CommonTableExpr, FromClause, IndexedBy, JoinConstraint, JoinType, LimitClause, Materialized,
    ResultColumn, TableOrSubquery, WithClause,
};

impl FormatNode for WithClause {
    fn format(&self, p: &mut Printer) {
        p.keyword("WITH");
        if self.recursive {
            p.space();
            p.keyword("RECURSIVE");
        }
        p.newline();
        p.indent();
        for (i, cte) in self.ctes.iter().enumerate() {
            if i > 0 {
                p.write(",");
                p.newline();
            }
            cte.format(p);
        }
        p.dedent();
        p.newline();
    }
}

impl FormatNode for CommonTableExpr {
    fn format(&self, p: &mut Printer) {
        format_identifier(p, &self.name);

        if let Some(cols) = &self.columns {
            p.write("(");
            format_list(p, cols, false, |p, c| format_identifier(p, c));
            p.write(")");
        }

        p.space();
        p.keyword("AS");

        if let Some(mat) = &self.materialized {
            p.space();
            match mat {
                Materialized::Materialized => p.keyword("MATERIALIZED"),
                Materialized::NotMaterialized => {
                    p.keyword("NOT");
                    p.space();
                    p.keyword("MATERIALIZED");
                }
            }
        }

        p.space();
        p.write("(");
        p.newline();
        p.indent();
        self.select.format(p);
        p.dedent();
        p.newline();
        p.write(")");
    }
}

impl FormatNode for FromClause {
    fn format(&self, p: &mut Printer) {
        p.keyword("FROM");
        p.space();
        for (i, table) in self.tables.iter().enumerate() {
            if i > 0 {
                p.write(", ");
            }
            table.format(p);
        }
    }
}

impl FormatNode for TableOrSubquery {
    fn format(&self, p: &mut Printer) {
        match self {
            TableOrSubquery::Table {
                schema,
                name,
                alias,
                alias_has_as,
                indexed,
                ..
            } => {
                format_qualified_name(p, schema, name);
                format_alias(p, alias, *alias_has_as);

                if let Some(idx) = indexed {
                    p.space();
                    match idx {
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
            }
            TableOrSubquery::TableFunction {
                name,
                args,
                alias,
                alias_has_as,
                ..
            } => {
                format_identifier(p, name);
                p.write("(");
                format_list(p, args, false, |p, arg| arg.format(p));
                p.write(")");
                format_alias(p, alias, *alias_has_as);
            }
            TableOrSubquery::Subquery {
                query,
                alias,
                alias_has_as,
                ..
            } => {
                p.write("(");
                p.newline();
                p.indent();
                query.format(p);
                p.dedent();
                p.newline();
                p.write(")");
                format_alias(p, alias, *alias_has_as);
            }
            TableOrSubquery::TableList { tables, .. } => {
                p.write("(");
                format_list(p, tables, false, |p, t| t.format(p));
                p.write(")");
            }
            TableOrSubquery::Join {
                left,
                join_type,
                right,
                constraint,
                ..
            } => {
                left.format(p);
                p.newline();
                format_join_type(p, join_type);
                p.space();
                right.format(p);
                if let Some(c) = constraint {
                    match c {
                        JoinConstraint::On(expr) => {
                            p.newline();
                            p.indent();
                            p.keyword("ON");
                            p.space();
                            expr.format(p);
                            p.dedent();
                        }
                        JoinConstraint::Using(cols) => {
                            p.space();
                            p.keyword("USING");
                            p.write("(");
                            format_list(p, cols, false, |p, c| format_identifier(p, c));
                            p.write(")");
                        }
                    }
                }
            }
        }
    }
}

fn format_join_type(p: &mut Printer, jt: &JoinType) {
    match jt {
        JoinType::Inner => {
            p.keyword("INNER");
            p.space();
            p.keyword("JOIN");
        }
        JoinType::Left => {
            p.keyword("LEFT");
            p.space();
            p.keyword("JOIN");
        }
        JoinType::Right => {
            p.keyword("RIGHT");
            p.space();
            p.keyword("JOIN");
        }
        JoinType::Full => {
            p.keyword("FULL");
            p.space();
            p.keyword("JOIN");
        }
        JoinType::Cross => {
            p.keyword("CROSS");
            p.space();
            p.keyword("JOIN");
        }
        JoinType::Natural => {
            p.keyword("NATURAL");
            p.space();
            p.keyword("JOIN");
        }
        JoinType::NaturalLeft => {
            p.keyword("NATURAL");
            p.space();
            p.keyword("LEFT");
            p.space();
            p.keyword("JOIN");
        }
        JoinType::NaturalRight => {
            p.keyword("NATURAL");
            p.space();
            p.keyword("RIGHT");
            p.space();
            p.keyword("JOIN");
        }
        JoinType::NaturalFull => {
            p.keyword("NATURAL");
            p.space();
            p.keyword("FULL");
            p.space();
            p.keyword("JOIN");
        }
    }
}

impl FormatNode for ResultColumn {
    fn format(&self, p: &mut Printer) {
        match self {
            ResultColumn::Expr {
                expr,
                alias,
                alias_has_as,
                ..
            } => {
                expr.format(p);
                format_alias(p, alias, *alias_has_as);
            }
            ResultColumn::Star(_) => {
                p.write("*");
            }
            ResultColumn::TableStar { table, .. } => {
                format_identifier(p, table);
                p.write(".*");
            }
        }
    }
}

impl FormatNode for LimitClause {
    fn format(&self, p: &mut Printer) {
        p.keyword("LIMIT");
        p.space();
        self.limit.format(p);
        if let Some(offset) = &self.offset {
            p.space();
            p.keyword("OFFSET");
            p.space();
            offset.format(p);
        }
    }
}
