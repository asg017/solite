//! Comment attachment for AST nodes
//!
//! This module handles extracting comments from the token stream and
//! attaching them to AST nodes based on span proximity.

use solite_lexer::{lex, Token, TokenKind};
use std::collections::HashMap;

/// Map of comments attached to AST node spans
#[derive(Debug, Clone, Default)]
pub struct CommentMap {
    /// Comments that appear before a node (keyed by span start)
    pub leading: HashMap<usize, Vec<Comment>>,
    /// Comments that appear after a node on the same line (keyed by span end)
    pub trailing: HashMap<usize, Vec<Comment>>,
    /// All comments in order of appearance
    pub all_comments: Vec<Comment>,
}

/// A comment with its content and location
#[derive(Debug, Clone)]
pub struct Comment {
    /// The comment text (including -- or /* */)
    pub text: String,
    /// Start byte offset
    pub start: usize,
    /// End byte offset
    pub end: usize,
    /// Whether this is a block comment (/* */)
    pub is_block: bool,
    /// Line number (0-indexed)
    pub line: usize,
}

impl CommentMap {
    /// Create a CommentMap from source code by extracting all comments
    pub fn from_source(source: &str) -> Self {
        let tokens = lex(source);
        let mut comments = Vec::new();

        // Extract all comments from tokens
        for token in &tokens {
            if token.kind == TokenKind::Comment || token.kind == TokenKind::BlockComment {
                let text = source[token.span.clone()].to_string();
                let is_block = token.kind == TokenKind::BlockComment;
                let line = source[..token.span.start].matches('\n').count();

                comments.push(Comment {
                    text,
                    start: token.span.start,
                    end: token.span.end,
                    is_block,
                    line,
                });
            }
        }

        // Build the leading/trailing maps
        let mut leading: HashMap<usize, Vec<Comment>> = HashMap::new();
        let mut trailing: HashMap<usize, Vec<Comment>> = HashMap::new();

        // Get non-comment tokens for proximity calculation
        let code_tokens: Vec<&Token> = tokens
            .iter()
            .filter(|t| t.kind != TokenKind::Comment && t.kind != TokenKind::BlockComment)
            .collect();

        for comment in &comments {
            // Find the nearest code token before and after this comment
            let prev_token = code_tokens
                .iter()
                .filter(|t| t.span.end <= comment.start)
                .next_back();
            let next_token = code_tokens
                .iter()
                .find(|t| t.span.start >= comment.end);

            // Determine if this is a trailing comment (same line as previous token)
            let is_trailing = if let Some(prev) = prev_token {
                let prev_line = source[..prev.span.end].matches('\n').count();
                prev_line == comment.line
            } else {
                false
            };

            if is_trailing {
                if let Some(prev) = prev_token {
                    trailing
                        .entry(prev.span.end)
                        .or_default()
                        .push(comment.clone());
                }
            } else if let Some(next) = next_token {
                // Leading comment for the next token
                leading
                    .entry(next.span.start)
                    .or_default()
                    .push(comment.clone());
            }
        }

        Self {
            leading,
            trailing,
            all_comments: comments,
        }
    }

    /// Get leading comments for a span
    pub fn get_leading(&self, span_start: usize) -> Option<&Vec<Comment>> {
        self.leading.get(&span_start)
    }

    /// Get trailing comments for a span
    pub fn get_trailing(&self, span_end: usize) -> Option<&Vec<Comment>> {
        self.trailing.get(&span_end)
    }

    /// Check if a byte offset is inside any comment
    pub fn is_in_comment(&self, offset: usize) -> bool {
        self.all_comments
            .iter()
            .any(|c| offset >= c.start && offset < c.end)
    }

    /// Get comment at a specific position, if any
    pub fn get_comment_at(&self, offset: usize) -> Option<&Comment> {
        self.all_comments
            .iter()
            .find(|c| offset >= c.start && offset < c.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_line_comments() {
        let source = "-- comment\nSELECT 1";
        let map = CommentMap::from_source(source);
        assert_eq!(map.all_comments.len(), 1);
        assert_eq!(map.all_comments[0].text, "-- comment");
        assert!(!map.all_comments[0].is_block);
    }

    #[test]
    fn test_extract_block_comments() {
        let source = "/* block */ SELECT 1";
        let map = CommentMap::from_source(source);
        assert_eq!(map.all_comments.len(), 1);
        assert_eq!(map.all_comments[0].text, "/* block */");
        assert!(map.all_comments[0].is_block);
    }

    #[test]
    fn test_trailing_comment() {
        let source = "SELECT 1 -- trailing";
        let map = CommentMap::from_source(source);
        assert_eq!(map.all_comments.len(), 1);
        // The comment should be attached as trailing to the "1" token
        assert!(!map.trailing.is_empty());
    }

    #[test]
    fn test_leading_comment() {
        let source = "-- leading\nSELECT 1";
        let map = CommentMap::from_source(source);
        assert_eq!(map.all_comments.len(), 1);
        // The comment should be attached as leading to SELECT
        assert!(!map.leading.is_empty());
    }

    #[test]
    fn test_multiple_comments() {
        let source = "-- first\n-- second\nSELECT 1 -- trailing";
        let map = CommentMap::from_source(source);
        assert_eq!(map.all_comments.len(), 3);
    }
}
