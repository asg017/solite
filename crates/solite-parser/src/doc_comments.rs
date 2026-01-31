//! Doc comment parsing for sqlite-docs support.
//!
//! This module provides parsing for special documentation comments that
//! can be attached to tables and columns in CREATE TABLE statements.
//!
//! ## Comment Formats
//!
//! - `--!` - Table-level documentation (must appear after the opening paren)
//! - `---` - Column-level documentation (must appear before a column definition)
//!
//! ## Tags
//!
//! Doc comments can include optional tags:
//! - `@example` - Example value(s)
//! - `@value` - Valid value description
//! - `@details` - Link to more details
//! - `@source` - Data source information
//! - `@schema` - Schema information
//!
//! ## Example
//!
//! ```sql
//! CREATE TABLE students (
//!   --! All students at Foo University.
//!   --! @details https://foo.edu/students
//!
//!   --- Student ID assigned at orientation
//!   --- @example 'S10483'
//!   student_id TEXT PRIMARY KEY,
//!
//!   --- Full name of student
//!   name TEXT
//! );
//! ```

use solite_lexer::{Token, TokenKind};
use std::collections::HashMap;

/// Documentation comment with optional tags.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DocComment {
    /// The main description text.
    pub description: String,
    /// Tags and their values (e.g., `@example` -> vec!["'S10483'", "'S10484'"])
    pub tags: HashMap<String, Vec<String>>,
}

impl DocComment {
    /// Create a new empty doc comment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a doc comment with just a description.
    pub fn with_description(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            tags: HashMap::new(),
        }
    }

    /// Check if the doc comment is empty.
    pub fn is_empty(&self) -> bool {
        self.description.is_empty() && self.tags.is_empty()
    }

    /// Add a tag value to this doc comment.
    pub fn add_tag(&mut self, tag: impl Into<String>, value: impl Into<String>) {
        let tag = tag.into();
        let value = value.into();
        self.tags.entry(tag).or_default().push(value);
    }

    /// Get the first value of a tag, if present.
    pub fn get_tag(&self, tag: &str) -> Option<&str> {
        self.tags.get(tag).and_then(|v| v.first().map(|s| s.as_str()))
    }

    /// Get all values of a tag.
    pub fn get_tag_values(&self, tag: &str) -> Option<&[String]> {
        self.tags.get(tag).map(|v| v.as_slice())
    }
}

/// The kind of doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocCommentKind {
    /// Table-level documentation (`--!`)
    Table,
    /// Column-level documentation (`---`)
    Column,
    /// Not a doc comment
    None,
}

/// Map from byte positions to doc comments.
///
/// The key is the byte position of the token following the doc comment,
/// allowing lookup during parsing.
#[derive(Debug, Clone, Default)]
pub struct DocCommentMap {
    /// Table docs: byte position -> DocComment
    pub table_docs: HashMap<usize, DocComment>,
    /// Column docs: byte position -> DocComment
    pub column_docs: HashMap<usize, DocComment>,
}

impl DocCommentMap {
    /// Create a new empty doc comment map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get table documentation at the given position.
    pub fn get_table_doc(&self, pos: usize) -> Option<&DocComment> {
        self.table_docs.get(&pos)
    }

    /// Get column documentation at the given position.
    pub fn get_column_doc(&self, pos: usize) -> Option<&DocComment> {
        self.column_docs.get(&pos)
    }
}

/// Parse a single comment line and determine its kind.
///
/// Returns the kind of doc comment and the content after the prefix.
pub fn parse_doc_comment_line(comment: &str) -> (DocCommentKind, Option<&str>) {
    let trimmed = comment.trim();

    // Check for table doc comment: --! (exclamation mark)
    if let Some(rest) = trimmed.strip_prefix("--!") {
        return (DocCommentKind::Table, Some(rest.trim()));
    }

    // Check for column doc comment: exactly --- (triple dash, but not more)
    if trimmed.starts_with("---") && !trimmed.starts_with("----") {
        if let Some(rest) = trimmed.strip_prefix("---") {
            return (DocCommentKind::Column, Some(rest.trim()));
        }
    }

    (DocCommentKind::None, None)
}

/// Parse a doc comment line into description or tag.
///
/// If the line starts with `@tag`, returns the tag name and value.
/// Otherwise, returns the line as description text.
fn parse_comment_content(line: &str) -> CommentContent {
    let trimmed = line.trim();

    if let Some(rest) = trimmed.strip_prefix('@') {
        // Find the tag name (until whitespace or end)
        if let Some(space_idx) = rest.find(char::is_whitespace) {
            let tag = &rest[..space_idx];
            let value = rest[space_idx..].trim();
            CommentContent::Tag(tag.to_string(), value.to_string())
        } else {
            // Tag with no value
            CommentContent::Tag(rest.to_string(), String::new())
        }
    } else {
        CommentContent::Description(trimmed.to_string())
    }
}

enum CommentContent {
    Description(String),
    Tag(String, String),
}

/// Build a doc comment map from a token stream.
///
/// This function processes the token stream before comments are filtered out,
/// identifying doc comments and mapping them to the byte positions of the
/// tokens that follow them.
pub fn build_doc_comment_map(tokens: &[Token], source: &str) -> DocCommentMap {
    let mut map = DocCommentMap::new();

    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];

        if token.kind == TokenKind::Comment {
            let comment_text = &source[token.span.clone()];
            let (kind, content) = parse_doc_comment_line(comment_text);

            if kind != DocCommentKind::None {
                // Collect consecutive doc comments of the same kind
                let mut doc = DocComment::new();
                let mut description_parts = Vec::new();

                // Process this comment
                if let Some(text) = content {
                    match parse_comment_content(text) {
                        CommentContent::Description(desc) if !desc.is_empty() => {
                            description_parts.push(desc);
                        }
                        CommentContent::Tag(tag, value) => {
                            doc.add_tag(tag, value);
                        }
                        _ => {}
                    }
                }

                // Look for consecutive doc comments of the same kind
                let mut j = i + 1;
                while j < tokens.len() {
                    let next_token = &tokens[j];

                    if next_token.kind == TokenKind::Comment {
                        let next_text = &source[next_token.span.clone()];
                        let (next_kind, next_content) = parse_doc_comment_line(next_text);

                        if next_kind == kind {
                            // Same kind, merge
                            if let Some(text) = next_content {
                                match parse_comment_content(text) {
                                    CommentContent::Description(desc) if !desc.is_empty() => {
                                        description_parts.push(desc);
                                    }
                                    CommentContent::Tag(tag, value) => {
                                        doc.add_tag(tag, value);
                                    }
                                    _ => {}
                                }
                            }
                            j += 1;
                        } else if next_kind == DocCommentKind::None {
                            // Regular comment, skip it but keep looking
                            j += 1;
                        } else {
                            // Different doc comment kind, stop
                            break;
                        }
                    } else {
                        // Non-comment token, stop
                        break;
                    }
                }

                // Build the description from collected parts
                doc.description = description_parts.join(" ");

                // Find the next non-comment token
                let target_pos = tokens[j..]
                    .iter()
                    .find(|next| next.kind != TokenKind::Comment && next.kind != TokenKind::BlockComment)
                    .map(|next| next.span.start);

                // Store the doc at the position of the following token
                if let Some(pos) = target_pos {
                    if !doc.is_empty() {
                        match kind {
                            DocCommentKind::Table => {
                                map.table_docs.insert(pos, doc);
                            }
                            DocCommentKind::Column => {
                                map.column_docs.insert(pos, doc);
                            }
                            DocCommentKind::None => unreachable!(),
                        }
                    }
                }

                // Skip to after the last doc comment we processed
                i = j;
                continue;
            }
        }

        i += 1;
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use solite_lexer::lex;

    #[test]
    fn test_parse_table_doc_comment() {
        let (kind, content) = parse_doc_comment_line("--! This is a table doc");
        assert_eq!(kind, DocCommentKind::Table);
        assert_eq!(content, Some("This is a table doc"));
    }

    #[test]
    fn test_parse_column_doc_comment() {
        let (kind, content) = parse_doc_comment_line("--- This is a column doc");
        assert_eq!(kind, DocCommentKind::Column);
        assert_eq!(content, Some("This is a column doc"));
    }

    #[test]
    fn test_parse_regular_comment() {
        let (kind, content) = parse_doc_comment_line("-- Regular comment");
        assert_eq!(kind, DocCommentKind::None);
        assert!(content.is_none());
    }

    #[test]
    fn test_parse_four_dashes_not_column_doc() {
        // Four dashes should not be treated as column doc
        let (kind, _) = parse_doc_comment_line("---- Four dashes");
        assert_eq!(kind, DocCommentKind::None);
    }

    #[test]
    fn test_parse_empty_table_doc() {
        let (kind, content) = parse_doc_comment_line("--!");
        assert_eq!(kind, DocCommentKind::Table);
        assert_eq!(content, Some(""));
    }

    #[test]
    fn test_parse_empty_column_doc() {
        let (kind, content) = parse_doc_comment_line("---");
        assert_eq!(kind, DocCommentKind::Column);
        assert_eq!(content, Some(""));
    }

    #[test]
    fn test_doc_comment_with_tag() {
        let content = "@example 'S10483'";
        match parse_comment_content(content) {
            CommentContent::Tag(tag, value) => {
                assert_eq!(tag, "example");
                assert_eq!(value, "'S10483'");
            }
            _ => panic!("Expected tag"),
        }
    }

    #[test]
    fn test_doc_comment_with_tag_no_value() {
        let content = "@deprecated";
        match parse_comment_content(content) {
            CommentContent::Tag(tag, value) => {
                assert_eq!(tag, "deprecated");
                assert_eq!(value, "");
            }
            _ => panic!("Expected tag"),
        }
    }

    #[test]
    fn test_doc_comment_description() {
        let content = "This is a description";
        match parse_comment_content(content) {
            CommentContent::Description(desc) => {
                assert_eq!(desc, "This is a description");
            }
            _ => panic!("Expected description"),
        }
    }

    #[test]
    fn test_build_doc_comment_map_simple() {
        let sql = r#"
            CREATE TABLE students (
                --! All students.
                --- Student ID
                student_id TEXT PRIMARY KEY
            );
        "#;

        let tokens = lex(sql);
        let map = build_doc_comment_map(&tokens, sql);

        // Should have one table doc and one column doc
        assert_eq!(map.table_docs.len(), 1);
        assert_eq!(map.column_docs.len(), 1);
    }

    #[test]
    fn test_build_doc_comment_map_with_tags() {
        let sql = r#"
            CREATE TABLE t (
                --- Student ID
                --- @example 'S10483'
                id TEXT
            );
        "#;

        let tokens = lex(sql);
        let map = build_doc_comment_map(&tokens, sql);

        // Find the column doc
        assert_eq!(map.column_docs.len(), 1);
        let doc = map.column_docs.values().next().unwrap();
        assert_eq!(doc.description, "Student ID");
        assert_eq!(doc.get_tag("example"), Some("'S10483'"));
    }

    #[test]
    fn test_build_doc_comment_map_consecutive_lines() {
        let sql = r#"
            CREATE TABLE t (
                --! First line of table doc.
                --! Second line of table doc.
                id TEXT
            );
        "#;

        let tokens = lex(sql);
        let map = build_doc_comment_map(&tokens, sql);

        assert_eq!(map.table_docs.len(), 1);
        let doc = map.table_docs.values().next().unwrap();
        assert_eq!(doc.description, "First line of table doc. Second line of table doc.");
    }

    #[test]
    fn test_build_doc_comment_map_multiple_tags() {
        let sql = r#"
            CREATE TABLE t (
                --- ID column
                --- @example 'A'
                --- @example 'B'
                id TEXT
            );
        "#;

        let tokens = lex(sql);
        let map = build_doc_comment_map(&tokens, sql);

        let doc = map.column_docs.values().next().unwrap();
        let examples = doc.get_tag_values("example").unwrap();
        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0], "'A'");
        assert_eq!(examples[1], "'B'");
    }

    #[test]
    fn test_doc_comment_struct() {
        let mut doc = DocComment::new();
        assert!(doc.is_empty());

        doc.description = "Test".to_string();
        assert!(!doc.is_empty());

        doc.add_tag("example", "'value'");
        assert_eq!(doc.get_tag("example"), Some("'value'"));

        doc.add_tag("example", "'value2'");
        let values = doc.get_tag_values("example").unwrap();
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn test_doc_comment_with_description() {
        let doc = DocComment::with_description("Test description");
        assert_eq!(doc.description, "Test description");
        assert!(!doc.is_empty());
    }

    #[test]
    fn test_regular_comments_between_doc_comments() {
        let sql = r#"
            CREATE TABLE t (
                --- First column doc
                -- Regular comment (ignored)
                --- Second column doc
                id TEXT
            );
        "#;

        let tokens = lex(sql);
        let map = build_doc_comment_map(&tokens, sql);

        // Should merge the two column docs
        assert_eq!(map.column_docs.len(), 1);
        let doc = map.column_docs.values().next().unwrap();
        assert_eq!(doc.description, "First column doc Second column doc");
    }

    #[test]
    fn test_table_and_column_docs_separate() {
        let sql = r#"
            CREATE TABLE t (
                --! Table documentation
                --- Column documentation
                id TEXT
            );
        "#;

        let tokens = lex(sql);
        let map = build_doc_comment_map(&tokens, sql);

        // Should have separate entries
        assert_eq!(map.table_docs.len(), 1);
        assert_eq!(map.column_docs.len(), 1);

        let table_doc = map.table_docs.values().next().unwrap();
        assert_eq!(table_doc.description, "Table documentation");

        let col_doc = map.column_docs.values().next().unwrap();
        assert_eq!(col_doc.description, "Column documentation");
    }
}
