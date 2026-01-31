use solite_lexer::{lex, TokenKind};
use std::collections::{HashMap, HashSet};

/// Tracks which rules are suppressed on which lines
#[derive(Debug, Default)]
pub struct Suppressions {
    /// Rules suppressed for specific lines: line_number -> set of rule_ids
    /// Line numbers are 1-indexed
    suppressed_lines: HashMap<usize, HashSet<String>>,
}

impl Suppressions {
    /// Parse suppressions from source code.
    ///
    /// Supports `-- solite-ignore: rule1, rule2` comments which
    /// suppress the specified rules on the following line.
    pub fn parse(source: &str) -> Self {
        let mut suppressions = Suppressions::default();
        let tokens = lex(source);

        // Track line numbers
        let line_starts: Vec<usize> = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(i, _)| i + 1))
            .collect();

        let offset_to_line = |offset: usize| -> usize {
            line_starts.partition_point(|&start| start <= offset)
        };

        for token in tokens {
            if token.kind != TokenKind::Comment && token.kind != TokenKind::BlockComment {
                continue;
            }

            let comment_text = &source[token.span.start..token.span.end];
            let line = offset_to_line(token.span.start);

            // Check for solite-ignore pattern
            if let Some(rules) = parse_ignore_comment(comment_text) {
                // Suppress on the next line
                for rule in rules {
                    suppressions.suppressed_lines
                        .entry(line + 1)
                        .or_default()
                        .insert(rule);
                }
            }
        }

        suppressions
    }

    /// Check if a rule is suppressed at a given line (1-indexed)
    pub fn is_suppressed(&self, rule_id: &str, line: usize) -> bool {
        self.suppressed_lines
            .get(&line)
            .map(|rules| rules.contains(rule_id))
            .unwrap_or(false)
    }
}

/// Parse a comment for solite-ignore directive.
/// Returns Some(vec of rule ids) if found, None otherwise.
fn parse_ignore_comment(comment: &str) -> Option<Vec<String>> {
    // Remove comment prefix (-- or /* */)
    let text = comment
        .trim_start_matches("--")
        .trim_start_matches("/*")
        .trim_end_matches("*/")
        .trim();

    // Only support solite-ignore:
    let prefix = "solite-ignore:";
    if !text.starts_with(prefix) {
        return None;
    }

    let rules_str = text.strip_prefix(prefix)?.trim();

    // Parse comma-separated rule IDs
    let rules: Vec<String> = rules_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if rules.is_empty() {
        None
    } else {
        Some(rules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ignore_next_line() {
        let source = "-- solite-ignore: empty-blob-literal\nSELECT X''";
        let suppressions = Suppressions::parse(source);
        assert!(suppressions.is_suppressed("empty-blob-literal", 2));
        assert!(!suppressions.is_suppressed("empty-blob-literal", 1));
    }

    #[test]
    fn test_parse_multiple_rules() {
        let source = "-- solite-ignore: rule1, rule2\nSELECT 1";
        let suppressions = Suppressions::parse(source);
        assert!(suppressions.is_suppressed("rule1", 2));
        assert!(suppressions.is_suppressed("rule2", 2));
    }

    #[test]
    fn test_no_suppression_without_directive() {
        let source = "-- just a comment\nSELECT X''";
        let suppressions = Suppressions::parse(source);
        assert!(!suppressions.is_suppressed("empty-blob-literal", 2));
    }
}
