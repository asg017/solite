use solite_ast::Expr;

use super::{Fix, LintContext, LintDiagnostic, LintRule, RuleSeverity};

pub struct DoubleQuotedString;

impl LintRule for DoubleQuotedString {
    fn id(&self) -> &'static str {
        "double-quoted-string"
    }

    fn name(&self) -> &'static str {
        "Double-Quoted String"
    }

    fn description(&self) -> &'static str {
        "Warns about double-quoted strings which are identifiers in SQL, not string literals"
    }

    fn default_severity(&self) -> RuleSeverity {
        RuleSeverity::Warning
    }

    fn check_expr(&self, expr: &Expr, _ctx: &LintContext) -> Vec<LintDiagnostic> {
        match expr {
            Expr::Ident(_, is_double_quoted, span) if *is_double_quoted => {
                vec![LintDiagnostic {
                    rule_id: self.id(),
                    message: "Double-quoted string is an identifier, not a string literal. Use single quotes for strings.".to_string(),
                    span: span.clone(),
                    severity: self.default_severity(),
                }]
            }
            _ => vec![],
        }
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn fix(&self, diagnostic: &LintDiagnostic, source: &str) -> Option<Fix> {
        // Get the original text
        let original = &source[diagnostic.span.start..diagnostic.span.end];
        // Remove double quotes and add single quotes
        // Handle escaped double quotes "" -> '
        let inner = original.trim_start_matches('"').trim_end_matches('"');
        let fixed = format!("'{}'", inner.replace("\"\"", "'"));
        Some(Fix {
            span: diagnostic.span.clone(),
            replacement: fixed,
        })
    }
}
