//! Expression formatting
//!
//! Implementations of FormatNode for Expr and related types.

use super::{format_identifier, format_list, FormatNode};
use crate::printer::Printer;
use solite_ast::{
    BinaryOp, Expr, FrameBound, FrameExclude, FrameSpec, FrameUnit, OrderDirection, OrderingTerm,
    RaiseAction, TypeName, UnaryOp, WindowSpec,
};

impl FormatNode for Expr {
    fn format(&self, p: &mut Printer) {
        match self {
            Expr::Integer(n, _) => {
                p.write(&n.to_string());
            }
            Expr::HexInteger(n, _) => {
                p.write(&format!("0x{:X}", n));
            }
            Expr::Float(f, _) => {
                p.write(&f.to_string());
            }
            Expr::String(s, _) => {
                // Escape single quotes
                let escaped = s.replace("'", "''");
                p.write(&format!("'{}'", escaped));
            }
            Expr::Blob(bytes, _) => {
                let hex: String = bytes.iter().map(|b| format!("{:02X}", b)).collect();
                p.write(&format!("X'{}'", hex));
            }
            Expr::Null(_) => {
                p.keyword("NULL");
            }
            Expr::Ident(name, _is_quoted, _) => {
                format_identifier(p, name);
            }
            Expr::Star(_) => {
                p.write("*");
            }
            Expr::BindParam(param, _) => {
                p.write(param);
            }
            Expr::Binary { left, op, right, .. } => {
                // Handle logical operators specially for potential line breaks
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    left.format(p);
                    p.logical_operator(match op {
                        BinaryOp::And => "AND",
                        BinaryOp::Or => "OR",
                        _ => unreachable!(),
                    });
                    right.format(p);
                } else {
                    left.format(p);
                    p.space();
                    format_binary_op(p, op);
                    p.space();
                    right.format(p);
                }
            }
            Expr::Unary { op, expr, .. } => {
                format_unary_op(p, op);
                if matches!(op, UnaryOp::Not) {
                    p.space();
                }
                expr.format(p);
            }
            Expr::Paren(expr, _) => {
                p.write("(");
                expr.format(p);
                p.write(")");
            }
            Expr::Between {
                expr,
                low,
                high,
                negated,
                ..
            } => {
                expr.format(p);
                p.space();
                if *negated {
                    p.keyword("NOT");
                    p.space();
                }
                p.keyword("BETWEEN");
                p.space();
                low.format(p);
                p.space();
                p.keyword("AND");
                p.space();
                high.format(p);
            }
            Expr::InList {
                expr,
                list,
                negated,
                ..
            } => {
                expr.format(p);
                p.space();
                if *negated {
                    p.keyword("NOT");
                    p.space();
                }
                p.keyword("IN");
                p.space();
                p.write("(");
                format_list(p, list, false, |p, e| e.format(p));
                p.write(")");
            }
            Expr::InSelect {
                expr,
                query,
                negated,
                ..
            } => {
                expr.format(p);
                p.space();
                if *negated {
                    p.keyword("NOT");
                    p.space();
                }
                p.keyword("IN");
                p.space();
                p.write("(");
                p.newline();
                p.indent();
                query.format(p);
                p.dedent();
                p.newline();
                p.write(")");
            }
            Expr::Subquery { query, .. } => {
                p.write("(");
                p.newline();
                p.indent();
                query.format(p);
                p.dedent();
                p.newline();
                p.write(")");
            }
            Expr::Exists {
                query, negated, ..
            } => {
                if *negated {
                    p.keyword("NOT");
                    p.space();
                }
                p.keyword("EXISTS");
                p.space();
                p.write("(");
                p.newline();
                p.indent();
                query.format(p);
                p.dedent();
                p.newline();
                p.write(")");
            }
            Expr::IsNull { expr, negated, .. } => {
                expr.format(p);
                p.space();
                p.keyword("IS");
                p.space();
                if *negated {
                    p.keyword("NOT");
                    p.space();
                }
                p.keyword("NULL");
            }
            Expr::Like {
                expr,
                pattern,
                escape,
                op,
                negated,
                ..
            } => {
                expr.format(p);
                p.space();
                if *negated {
                    p.keyword("NOT");
                    p.space();
                }
                p.keyword(match op {
                    BinaryOp::Like => "LIKE",
                    BinaryOp::Glob => "GLOB",
                    BinaryOp::Regexp => "REGEXP",
                    BinaryOp::Match => "MATCH",
                    _ => "LIKE",
                });
                p.space();
                pattern.format(p);
                if let Some(esc) = escape {
                    p.space();
                    p.keyword("ESCAPE");
                    p.space();
                    esc.format(p);
                }
            }
            Expr::Case {
                operand,
                when_clauses,
                else_clause,
                ..
            } => {
                p.keyword("CASE");
                if let Some(op) = operand {
                    p.space();
                    op.format(p);
                }
                p.newline();
                p.indent();
                for (when_expr, then_expr) in when_clauses {
                    p.keyword("WHEN");
                    p.space();
                    when_expr.format(p);
                    p.space();
                    p.keyword("THEN");
                    p.space();
                    then_expr.format(p);
                    p.newline();
                }
                if let Some(else_expr) = else_clause {
                    p.keyword("ELSE");
                    p.space();
                    else_expr.format(p);
                    p.newline();
                }
                p.dedent();
                p.keyword("END");
            }
            Expr::Cast { expr, type_name, .. } => {
                p.keyword("CAST");
                p.write("(");
                expr.format(p);
                p.space();
                p.keyword("AS");
                p.space();
                type_name.format(p);
                p.write(")");
            }
            Expr::FunctionCall {
                name,
                args,
                distinct,
                filter,
                over,
                ..
            } => {
                p.write(name);
                p.write("(");
                if *distinct {
                    p.keyword("DISTINCT");
                    p.space();
                }
                format_list(p, args, false, |p, e| e.format(p));
                p.write(")");
                if let Some(filter_expr) = filter {
                    p.space();
                    p.keyword("FILTER");
                    p.write("(");
                    p.keyword("WHERE");
                    p.space();
                    filter_expr.format(p);
                    p.write(")");
                }
                if let Some(window) = over {
                    p.space();
                    p.keyword("OVER");
                    p.space();
                    window.format(p);
                }
            }
            Expr::Column {
                schema,
                table,
                column,
                ..
            } => {
                if let Some(s) = schema {
                    format_identifier(p, s);
                    p.write(".");
                }
                if let Some(t) = table {
                    format_identifier(p, t);
                    p.write(".");
                }
                format_identifier(p, column);
            }
            Expr::Collate {
                expr, collation, ..
            } => {
                expr.format(p);
                p.space();
                p.keyword("COLLATE");
                p.space();
                p.write(collation);
            }
            Expr::Raise {
                action, message, ..
            } => {
                p.keyword("RAISE");
                p.write("(");
                p.keyword(match action {
                    RaiseAction::Ignore => "IGNORE",
                    RaiseAction::Rollback => "ROLLBACK",
                    RaiseAction::Abort => "ABORT",
                    RaiseAction::Fail => "FAIL",
                });
                if let Some(msg) = message {
                    p.write(", ");
                    msg.format(p);
                }
                p.write(")");
            }
        }
    }
}

fn format_binary_op(p: &mut Printer, op: &BinaryOp) {
    match op {
        BinaryOp::Add => p.write("+"),
        BinaryOp::Sub => p.write("-"),
        BinaryOp::Mul => p.write("*"),
        BinaryOp::Div => p.write("/"),
        BinaryOp::Mod => p.write("%"),
        BinaryOp::Concat => p.write("||"),
        BinaryOp::Eq => p.write("="),
        BinaryOp::Ne => p.write("<>"),
        BinaryOp::Lt => p.write("<"),
        BinaryOp::Le => p.write("<="),
        BinaryOp::Gt => p.write(">"),
        BinaryOp::Ge => p.write(">="),
        BinaryOp::Is => p.keyword("IS"),
        BinaryOp::IsNot => {
            p.keyword("IS");
            p.space();
            p.keyword("NOT");
        }
        BinaryOp::BitAnd => p.write("&"),
        BinaryOp::BitOr => p.write("|"),
        BinaryOp::LShift => p.write("<<"),
        BinaryOp::RShift => p.write(">>"),
        BinaryOp::And => p.keyword("AND"),
        BinaryOp::Or => p.keyword("OR"),
        BinaryOp::Like => p.keyword("LIKE"),
        BinaryOp::Glob => p.keyword("GLOB"),
        BinaryOp::Regexp => p.keyword("REGEXP"),
        BinaryOp::Match => p.keyword("MATCH"),
        BinaryOp::JsonExtract => p.write("->"),
        BinaryOp::JsonExtractText => p.write("->>"),
    }
}

fn format_unary_op(p: &mut Printer, op: &UnaryOp) {
    match op {
        UnaryOp::Neg => p.write("-"),
        UnaryOp::Pos => p.write("+"),
        UnaryOp::Not => p.keyword("NOT"),
        UnaryOp::BitNot => p.write("~"),
    }
}

impl FormatNode for TypeName {
    fn format(&self, p: &mut Printer) {
        p.write(&self.name);
        if let Some((arg1, arg2)) = &self.args {
            p.write("(");
            p.write(&arg1.to_string());
            if let Some(a2) = arg2 {
                p.write(", ");
                p.write(&a2.to_string());
            }
            p.write(")");
        }
    }
}

impl FormatNode for WindowSpec {
    fn format(&self, p: &mut Printer) {
        p.write("(");
        let mut needs_space = false;

        if let Some(base) = &self.base_window {
            p.write(base);
            needs_space = true;
        }

        if let Some(partition) = &self.partition_by {
            if needs_space {
                p.space();
            }
            p.keyword("PARTITION");
            p.space();
            p.keyword("BY");
            p.space();
            format_list(p, partition, false, |p, e| e.format(p));
            needs_space = true;
        }

        if let Some(order) = &self.order_by {
            if needs_space {
                p.space();
            }
            p.keyword("ORDER");
            p.space();
            p.keyword("BY");
            p.space();
            format_list(p, order, false, |p, o| o.format(p));
            needs_space = true;
        }

        if let Some(frame) = &self.frame {
            if needs_space {
                p.space();
            }
            frame.format(p);
        }

        p.write(")");
    }
}

impl FormatNode for FrameSpec {
    fn format(&self, p: &mut Printer) {
        p.keyword(match self.unit {
            FrameUnit::Rows => "ROWS",
            FrameUnit::Range => "RANGE",
            FrameUnit::Groups => "GROUPS",
        });
        p.space();

        if let Some(end) = &self.end {
            p.keyword("BETWEEN");
            p.space();
            format_frame_bound(p, &self.start);
            p.space();
            p.keyword("AND");
            p.space();
            format_frame_bound(p, end);
        } else {
            format_frame_bound(p, &self.start);
        }

        if let Some(exclude) = &self.exclude {
            p.space();
            p.keyword("EXCLUDE");
            p.space();
            p.keyword(match exclude {
                FrameExclude::NoOthers => "NO OTHERS",
                FrameExclude::CurrentRow => "CURRENT ROW",
                FrameExclude::Group => "GROUP",
                FrameExclude::Ties => "TIES",
            });
        }
    }
}

fn format_frame_bound(p: &mut Printer, bound: &FrameBound) {
    match bound {
        FrameBound::UnboundedPreceding => {
            p.keyword("UNBOUNDED");
            p.space();
            p.keyword("PRECEDING");
        }
        FrameBound::Preceding(expr) => {
            expr.format(p);
            p.space();
            p.keyword("PRECEDING");
        }
        FrameBound::CurrentRow => {
            p.keyword("CURRENT");
            p.space();
            p.keyword("ROW");
        }
        FrameBound::Following(expr) => {
            expr.format(p);
            p.space();
            p.keyword("FOLLOWING");
        }
        FrameBound::UnboundedFollowing => {
            p.keyword("UNBOUNDED");
            p.space();
            p.keyword("FOLLOWING");
        }
    }
}

impl FormatNode for OrderingTerm {
    fn format(&self, p: &mut Printer) {
        self.expr.format(p);
        if let Some(dir) = &self.direction {
            p.space();
            p.keyword(match dir {
                OrderDirection::Asc => "ASC",
                OrderDirection::Desc => "DESC",
            });
        }
        if let Some(nulls) = &self.nulls {
            p.space();
            p.keyword("NULLS");
            p.space();
            p.keyword(match nulls {
                solite_ast::NullsOrder::First => "FIRST",
                solite_ast::NullsOrder::Last => "LAST",
            });
        }
    }
}
