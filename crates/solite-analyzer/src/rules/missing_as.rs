use solite_ast::{Expr, ResultColumn, Statement, TableOrSubquery};

use super::{Fix, LintContext, LintDiagnostic, LintRule, RuleSeverity};

/// Rule that detects missing AS keyword in aliases.
///
/// Writing `SELECT foo bar` instead of `SELECT foo AS bar` is confusing
/// and can lead to bugs when a comma is accidentally omitted.
pub struct MissingAsAlias;

impl LintRule for MissingAsAlias {
    fn id(&self) -> &'static str {
        "missing-as"
    }

    fn name(&self) -> &'static str {
        "Missing AS Keyword"
    }

    fn description(&self) -> &'static str {
        "Warns when an alias is used without the AS keyword"
    }

    fn default_severity(&self) -> RuleSeverity {
        RuleSeverity::Warning
    }

    fn check_expr(&self, _expr: &Expr, _ctx: &LintContext) -> Vec<LintDiagnostic> {
        // This rule checks statements, not expressions
        vec![]
    }

    fn check_stmt(&self, stmt: &Statement, ctx: &LintContext) -> Vec<LintDiagnostic> {
        let mut diagnostics = Vec::new();

        if let Statement::Select(select) = stmt {
            // Check column aliases
            for col in &select.columns {
                if let ResultColumn::Expr { expr: _, alias: Some(alias), alias_has_as, span } = col {
                    if !alias_has_as {
                        diagnostics.push(LintDiagnostic {
                            rule_id: self.id(),
                            message: format!("Alias '{}' should use AS keyword", alias),
                            span: span.clone(),
                            severity: ctx.config.get_severity(self.id(), self.default_severity()),
                        });
                    }
                }
            }

            // Check table aliases in FROM clause
            if let Some(from) = &select.from {
                for table in &from.tables {
                    self.check_table_alias(table, ctx, &mut diagnostics);
                }
            }
        }

        diagnostics
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn fix(&self, diagnostic: &LintDiagnostic, source: &str) -> Option<Fix> {
        // The fix is to insert "AS " before the alias name
        // We need to find the alias in the span and insert AS before it
        let span_text = &source[diagnostic.span.start..diagnostic.span.end];

        // Find the last whitespace followed by the alias (which is at the end)
        // The alias is the last word in the span
        if let Some(last_space_idx) = span_text.rfind(|c: char| c.is_whitespace()) {
            // Insert "AS " after the whitespace
            let insert_pos = diagnostic.span.start + last_space_idx + 1;
            let alias_text = span_text[last_space_idx + 1..].trim();

            // Build replacement: original text up to alias + "AS " + alias
            let prefix = &source[diagnostic.span.start..insert_pos];
            let replacement = format!("{}AS {}", prefix, alias_text);

            Some(Fix {
                span: diagnostic.span.clone(),
                replacement,
            })
        } else {
            None
        }
    }
}

impl MissingAsAlias {
    fn check_table_alias(&self, table: &TableOrSubquery, ctx: &LintContext, diagnostics: &mut Vec<LintDiagnostic>) {
        match table {
            TableOrSubquery::Table { alias: Some(alias), alias_has_as, span, .. } => {
                if !alias_has_as {
                    diagnostics.push(LintDiagnostic {
                        rule_id: self.id(),
                        message: format!("Table alias '{}' should use AS keyword", alias),
                        span: span.clone(),
                        severity: ctx.config.get_severity(self.id(), self.default_severity()),
                    });
                }
            }
            TableOrSubquery::Subquery { alias: Some(alias), alias_has_as, span, .. } => {
                if !alias_has_as {
                    diagnostics.push(LintDiagnostic {
                        rule_id: self.id(),
                        message: format!("Subquery alias '{}' should use AS keyword", alias),
                        span: span.clone(),
                        severity: ctx.config.get_severity(self.id(), self.default_severity()),
                    });
                }
            }
            TableOrSubquery::Join { left, right, .. } => {
                self.check_table_alias(left, ctx, diagnostics);
                self.check_table_alias(right, ctx, diagnostics);
            }
            TableOrSubquery::TableList { tables, .. } => {
                for t in tables {
                    self.check_table_alias(t, ctx, diagnostics);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{lint_with_config, LintConfig};

    #[test]
    fn test_missing_as_in_column_alias() {
        let source = "SELECT foo bar FROM t";
        let program = solite_parser::parse_program(source).unwrap();
        let config = LintConfig::default();
        let results = lint_with_config(&program, source, &config, None);

        // Should find 2 warnings: column alias "bar" and table alias "t"
        // Wait, "t" is the table name, not an alias
        // So just 1 warning for column alias
        let missing_as: Vec<_> = results.iter().filter(|r| r.diagnostic.rule_id == "missing-as").collect();
        assert_eq!(missing_as.len(), 1);
        assert!(missing_as[0].diagnostic.message.contains("bar"));
    }

    #[test]
    fn test_as_in_column_alias_no_warning() {
        let source = "SELECT foo AS bar FROM t";
        let program = solite_parser::parse_program(source).unwrap();
        let config = LintConfig::default();
        let results = lint_with_config(&program, source, &config, None);

        let missing_as: Vec<_> = results.iter().filter(|r| r.diagnostic.rule_id == "missing-as").collect();
        assert_eq!(missing_as.len(), 0);
    }

    #[test]
    fn test_missing_as_in_table_alias() {
        let source = "SELECT * FROM users u";
        let program = solite_parser::parse_program(source).unwrap();
        let config = LintConfig::default();
        let results = lint_with_config(&program, source, &config, None);

        let missing_as: Vec<_> = results.iter().filter(|r| r.diagnostic.rule_id == "missing-as").collect();
        assert_eq!(missing_as.len(), 1);
        assert!(missing_as[0].diagnostic.message.contains("'u'"));
    }

    #[test]
    fn test_as_in_table_alias_no_warning() {
        let source = "SELECT * FROM users AS u";
        let program = solite_parser::parse_program(source).unwrap();
        let config = LintConfig::default();
        let results = lint_with_config(&program, source, &config, None);

        let missing_as: Vec<_> = results.iter().filter(|r| r.diagnostic.rule_id == "missing-as").collect();
        assert_eq!(missing_as.len(), 0);
    }

    #[test]
    fn test_fix_column_alias() {
        let source = "SELECT foo bar";
        let program = solite_parser::parse_program(source).unwrap();
        let config = LintConfig::default();
        let results = lint_with_config(&program, source, &config, None);

        let missing_as: Vec<_> = results.iter().filter(|r| r.diagnostic.rule_id == "missing-as").collect();
        assert_eq!(missing_as.len(), 1);

        let fix = missing_as[0].fix.as_ref().unwrap();
        assert_eq!(fix.replacement, "foo AS bar");
    }

    #[test]
    fn test_fix_table_alias() {
        let source = "SELECT * FROM users u";
        let program = solite_parser::parse_program(source).unwrap();
        let config = LintConfig::default();
        let results = lint_with_config(&program, source, &config, None);

        let missing_as: Vec<_> = results.iter().filter(|r| r.diagnostic.rule_id == "missing-as").collect();
        assert_eq!(missing_as.len(), 1);

        let fix = missing_as[0].fix.as_ref().unwrap();
        assert_eq!(fix.replacement, "users AS u");
    }
}
