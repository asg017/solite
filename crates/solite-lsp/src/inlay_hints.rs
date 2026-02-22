//! Inlay hints for SQL statements
//!
//! Provides column name hints for INSERT VALUES clauses.
//! Hints appear before each value expression, showing which column it maps to.
//!
//! ## Example
//!
//! ```sql
//! INSERT INTO t(c, b, a) VALUES (1, 2, 3);
//! -- Shows as: INSERT INTO t(c, b, a) VALUES ([c] 1, [b] 2, [a] 3);
//! ```

use solite_ast::{InsertSource, InsertStmt, Program, Statement};
use solite_lexer::{lex, Token, TokenKind};

/// Information about a single inlay hint
#[derive(Debug, Clone, PartialEq)]
pub struct InlayHintInfo {
    /// Byte offset where hint should appear (BEFORE the expression)
    pub position: usize,
    /// The column name to display
    pub label: String,
}

/// Extract all inlay hints from a parsed program (AST-based)
pub fn get_inlay_hints(program: &Program) -> Vec<InlayHintInfo> {
    let mut hints = Vec::new();

    for stmt in &program.statements {
        if let Statement::Insert(insert) = stmt {
            hints.extend(insert_values_hints(insert));
        }
    }

    hints
}

/// Extract hints using token-based analysis (fault-tolerant for incomplete SQL)
pub fn get_inlay_hints_from_tokens(source: &str) -> Vec<InlayHintInfo> {
    get_inlay_hints_from_tokens_filtered(source, None)
}

/// Extract hints, optionally filtering to only statements containing the given offset
pub fn get_inlay_hints_from_tokens_filtered(source: &str, edit_offset: Option<usize>) -> Vec<InlayHintInfo> {
    let tokens = lex(source);
    let mut hints = Vec::new();
    let mut i = 0;

    while i < tokens.len() {
        // Look for INSERT keyword
        if tokens[i].kind == TokenKind::Insert {
            let insert_start = tokens[i].span.start;
            if let Some((insert_hints, insert_end)) = parse_insert_hints_with_range(&tokens, &mut i, source) {
                // If we have an edit offset, only include hints if the edit is within this INSERT
                if let Some(offset) = edit_offset {
                    if offset >= insert_start && offset <= insert_end {
                        hints.extend(insert_hints);
                    }
                } else {
                    hints.extend(insert_hints);
                }
            }
        }
        i += 1;
    }

    hints
}

/// Parse hints from an INSERT statement starting at the INSERT token
/// Returns hints and the end byte offset of the statement
fn parse_insert_hints_with_range(tokens: &[Token], pos: &mut usize, source: &str) -> Option<(Vec<InlayHintInfo>, usize)> {
    let mut i = *pos + 1;

    // Skip whitespace/comments and look for INTO (optional) then table name
    skip_trivia(tokens, &mut i);

    // Handle optional OR REPLACE/ABORT/etc
    if i < tokens.len() && tokens[i].kind == TokenKind::Or {
        i += 1;
        skip_trivia(tokens, &mut i);
        // Skip the conflict action keyword
        if i < tokens.len() {
            i += 1;
        }
        skip_trivia(tokens, &mut i);
    }

    // Skip INTO if present
    if i < tokens.len() && tokens[i].kind == TokenKind::Into {
        i += 1;
        skip_trivia(tokens, &mut i);
    }

    // Skip table name (might be schema.table)
    if i >= tokens.len() || !is_identifier(&tokens[i].kind) {
        return None;
    }
    i += 1;
    skip_trivia(tokens, &mut i);

    // Handle schema.table
    if i < tokens.len() && tokens[i].kind == TokenKind::Dot {
        i += 1;
        skip_trivia(tokens, &mut i);
        if i < tokens.len() && is_identifier(&tokens[i].kind) {
            i += 1;
            skip_trivia(tokens, &mut i);
        }
    }

    // Skip optional AS alias
    if i < tokens.len() && tokens[i].kind == TokenKind::As {
        i += 1;
        skip_trivia(tokens, &mut i);
        if i < tokens.len() && is_identifier(&tokens[i].kind) {
            i += 1;
            skip_trivia(tokens, &mut i);
        }
    }

    // Now we should be at the column list: (col1, col2, col3)
    if i >= tokens.len() || tokens[i].kind != TokenKind::LParen {
        return None; // No column list
    }
    i += 1;
    skip_trivia(tokens, &mut i);

    // Extract column names
    let mut columns: Vec<String> = Vec::new();
    while i < tokens.len() && tokens[i].kind != TokenKind::RParen {
        if is_identifier(&tokens[i].kind) {
            let col_name = &source[tokens[i].span.clone()];
            columns.push(col_name.to_string());
        }
        i += 1;
        skip_trivia(tokens, &mut i);

        // Skip comma
        if i < tokens.len() && tokens[i].kind == TokenKind::Comma {
            i += 1;
            skip_trivia(tokens, &mut i);
        }
    }

    if columns.is_empty() {
        return None;
    }

    // Skip closing paren
    if i < tokens.len() && tokens[i].kind == TokenKind::RParen {
        i += 1;
        skip_trivia(tokens, &mut i);
    }

    // Look for VALUES keyword
    if i >= tokens.len() || tokens[i].kind != TokenKind::Values {
        return None;
    }
    i += 1;
    skip_trivia(tokens, &mut i);

    // Parse VALUES rows: (expr, expr), (expr, expr), ...
    let mut hints = Vec::new();

    while i < tokens.len() && tokens[i].kind == TokenKind::LParen {
        i += 1; // Skip opening paren
        skip_trivia(tokens, &mut i);

        let mut col_idx = 0;
        let mut paren_depth = 1;

        // Mark start of first expression
        if i < tokens.len() && col_idx < columns.len() {
            hints.push(InlayHintInfo {
                position: tokens[i].span.start,
                label: columns[col_idx].clone(),
            });
            col_idx += 1;
        }

        // Scan through the VALUES row
        while i < tokens.len() && paren_depth > 0 {
            match tokens[i].kind {
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        break;
                    }
                }
                TokenKind::Comma if paren_depth == 1 => {
                    // Top-level comma - next token starts new expression
                    i += 1;
                    skip_trivia(tokens, &mut i);

                    if i < tokens.len() && col_idx < columns.len() {
                        hints.push(InlayHintInfo {
                            position: tokens[i].span.start,
                            label: columns[col_idx].clone(),
                        });
                        col_idx += 1;
                    }
                    continue;
                }
                _ => {}
            }
            i += 1;
        }

        // Skip closing paren and look for comma (another row) or end
        if i < tokens.len() && tokens[i].kind == TokenKind::RParen {
            i += 1;
        }
        skip_trivia(tokens, &mut i);

        // Check for comma (another row)
        if i < tokens.len() && tokens[i].kind == TokenKind::Comma {
            i += 1;
            skip_trivia(tokens, &mut i);
        } else {
            break;
        }
    }

    // Calculate end position (use last token we processed, or end of source)
    let end_pos = if i > 0 && i <= tokens.len() {
        tokens[i.saturating_sub(1)].span.end
    } else {
        source.len()
    };

    *pos = i;
    Some((hints, end_pos))
}

fn skip_trivia(tokens: &[Token], pos: &mut usize) {
    while *pos < tokens.len() {
        match tokens[*pos].kind {
            TokenKind::Comment | TokenKind::BlockComment => *pos += 1,
            _ => break,
        }
    }
}

fn is_identifier(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Ident
            | TokenKind::QuotedIdent
            | TokenKind::BracketIdent
            | TokenKind::BacktickIdent
    )
}

/// Extract hints from INSERT statement VALUES clause (AST-based)
fn insert_values_hints(insert: &InsertStmt) -> Vec<InlayHintInfo> {
    // Only emit hints when columns are explicitly specified
    let Some(columns) = &insert.columns else {
        return Vec::new();
    };

    let InsertSource::Values(rows) = &insert.source else {
        return Vec::new();
    };

    let mut hints = Vec::new();

    for row in rows {
        for (i, expr) in row.iter().enumerate() {
            if let Some(col_name) = columns.get(i) {
                hints.push(InlayHintInfo {
                    position: expr.span().start, // BEFORE the expression
                    label: col_name.clone(),
                });
            }
        }
    }

    hints
}

#[cfg(test)]
mod tests {
    use super::*;
    use solite_parser::parse_program;

    // ============ AST-based tests ============

    #[test]
    fn test_basic_insert_hints() {
        let sql = "INSERT INTO t(c, b, a) VALUES (1, 2, 3)";
        let program = parse_program(sql).unwrap();
        let hints = get_inlay_hints(&program);

        assert_eq!(hints.len(), 3);
        assert_eq!(hints[0].label, "c");
        assert_eq!(hints[1].label, "b");
        assert_eq!(hints[2].label, "a");
    }

    #[test]
    fn test_no_hints_without_columns() {
        let sql = "INSERT INTO t VALUES (1, 2, 3)";
        let program = parse_program(sql).unwrap();
        let hints = get_inlay_hints(&program);

        assert!(hints.is_empty());
    }

    #[test]
    fn test_multiple_rows() {
        let sql = "INSERT INTO t(a, b) VALUES (1, 2), (3, 4)";
        let program = parse_program(sql).unwrap();
        let hints = get_inlay_hints(&program);

        assert_eq!(hints.len(), 4);
        assert_eq!(hints[0].label, "a");
        assert_eq!(hints[1].label, "b");
        assert_eq!(hints[2].label, "a");
        assert_eq!(hints[3].label, "b");
    }

    #[test]
    fn test_default_values_no_hints() {
        let sql = "INSERT INTO t DEFAULT VALUES";
        let program = parse_program(sql).unwrap();
        let hints = get_inlay_hints(&program);

        assert!(hints.is_empty());
    }

    #[test]
    fn test_insert_select_no_hints() {
        let sql = "INSERT INTO t(a, b) SELECT x, y FROM other";
        let program = parse_program(sql).unwrap();
        let hints = get_inlay_hints(&program);

        assert!(hints.is_empty());
    }

    #[test]
    fn test_multiline_values() {
        let sql = r#"INSERT INTO t(name, age, city) VALUES (
  'Alice',
  30,
  'NYC'
)"#;
        let program = parse_program(sql).unwrap();
        let hints = get_inlay_hints(&program);

        assert_eq!(hints.len(), 3);
        assert_eq!(hints[0].label, "name");
        assert_eq!(hints[1].label, "age");
        assert_eq!(hints[2].label, "city");
    }

    #[test]
    fn test_hint_positions() {
        let sql = "INSERT INTO t(a, b) VALUES (1, 2)";
        let program = parse_program(sql).unwrap();
        let hints = get_inlay_hints(&program);

        // Position should be at the start of each value expression
        assert_eq!(hints[0].position, 28); // position of '1'
        assert_eq!(hints[1].position, 31); // position of '2'
    }

    // ============ Token-based tests ============

    #[test]
    fn test_token_basic_insert() {
        let sql = "INSERT INTO t(c, b, a) VALUES (1, 2, 3)";
        let hints = get_inlay_hints_from_tokens(sql);

        assert_eq!(hints.len(), 3);
        assert_eq!(hints[0].label, "c");
        assert_eq!(hints[1].label, "b");
        assert_eq!(hints[2].label, "a");
    }

    #[test]
    fn test_token_no_columns() {
        let sql = "INSERT INTO t VALUES (1, 2, 3)";
        let hints = get_inlay_hints_from_tokens(sql);

        assert!(hints.is_empty());
    }

    #[test]
    fn test_token_multiple_rows() {
        let sql = "INSERT INTO t(a, b) VALUES (1, 2), (3, 4)";
        let hints = get_inlay_hints_from_tokens(sql);

        assert_eq!(hints.len(), 4);
        assert_eq!(hints[0].label, "a");
        assert_eq!(hints[1].label, "b");
        assert_eq!(hints[2].label, "a");
        assert_eq!(hints[3].label, "b");
    }

    #[test]
    fn test_token_incomplete_sql() {
        // Incomplete SQL - still typing
        let sql = "INSERT INTO t(a, b, c) VALUES (1, 2, ";
        let hints = get_inlay_hints_from_tokens(sql);

        // Should still get hints for what's there
        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].label, "a");
        assert_eq!(hints[1].label, "b");
    }

    #[test]
    fn test_token_incomplete_values() {
        // VALUES clause started but no values yet
        let sql = "INSERT INTO t(a, b, c) VALUES (";
        let hints = get_inlay_hints_from_tokens(sql);

        // Should get no hints since there's nothing after the paren
        assert!(hints.is_empty());
    }

    #[test]
    fn test_token_nested_parens() {
        // Function calls in values
        let sql = "INSERT INTO t(a, b) VALUES (func(1, 2), 3)";
        let hints = get_inlay_hints_from_tokens(sql);

        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].label, "a");
        assert_eq!(hints[1].label, "b");
    }

    #[test]
    fn test_token_with_schema() {
        let sql = "INSERT INTO myschema.t(a, b) VALUES (1, 2)";
        let hints = get_inlay_hints_from_tokens(sql);

        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].label, "a");
        assert_eq!(hints[1].label, "b");
    }

    #[test]
    fn test_token_or_replace() {
        let sql = "INSERT OR REPLACE INTO t(a, b) VALUES (1, 2)";
        let hints = get_inlay_hints_from_tokens(sql);

        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].label, "a");
        assert_eq!(hints[1].label, "b");
    }

    #[test]
    fn test_token_multiline() {
        let sql = r#"INSERT INTO t(name, age) VALUES (
            'Alice',
            30
        )"#;
        let hints = get_inlay_hints_from_tokens(sql);

        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].label, "name");
        assert_eq!(hints[1].label, "age");
    }

    #[test]
    fn test_filtered_by_edit_position() {
        let sql = r#"CREATE TABLE t(x);
INSERT INTO t1(a, b) VALUES (1, 2);
INSERT INTO t2(x, y) VALUES (3, 4);"#;

        // No filter - should get all hints
        let all_hints = get_inlay_hints_from_tokens_filtered(sql, None);
        assert_eq!(all_hints.len(), 4);

        // Filter to first INSERT (offset 50 is inside "VALUES (1, 2)")
        let first_hints = get_inlay_hints_from_tokens_filtered(sql, Some(50));
        assert_eq!(first_hints.len(), 2);
        assert_eq!(first_hints[0].label, "a");
        assert_eq!(first_hints[1].label, "b");

        // Filter to second INSERT (offset 80 is inside second INSERT)
        let second_hints = get_inlay_hints_from_tokens_filtered(sql, Some(80));
        assert_eq!(second_hints.len(), 2);
        assert_eq!(second_hints[0].label, "x");
        assert_eq!(second_hints[1].label, "y");

        // Filter to CREATE TABLE position (offset 5) - outside any INSERT
        let no_hints = get_inlay_hints_from_tokens_filtered(sql, Some(5));
        assert_eq!(no_hints.len(), 0);
    }
}
