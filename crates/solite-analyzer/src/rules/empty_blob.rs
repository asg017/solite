use solite_ast::Expr;

use super::{LintContext, LintDiagnostic, LintRule, RuleSeverity};

pub struct EmptyBlobLiteral;

impl LintRule for EmptyBlobLiteral {
    fn id(&self) -> &'static str {
        "empty-blob-literal"
    }

    fn name(&self) -> &'static str {
        "Empty Blob Literal"
    }

    fn description(&self) -> &'static str {
        "Warns about empty blob literals (X'')"
    }

    fn default_severity(&self) -> RuleSeverity {
        RuleSeverity::Warning
    }

    fn check_expr(&self, expr: &Expr, _ctx: &LintContext) -> Vec<LintDiagnostic> {
        match expr {
            Expr::Blob(bytes, span) if bytes.is_empty() => {
                vec![LintDiagnostic {
                    rule_id: self.id(),
                    message: "Empty blob literal".to_string(),
                    span: span.clone(),
                    severity: self.default_severity(),
                }]
            }
            _ => vec![],
        }
    }
}
