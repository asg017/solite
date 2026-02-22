//! Printer for generating formatted SQL output
//!
//! The Printer handles indentation, keyword case transformation,
//! comment emission, and other formatting concerns.

use crate::comment::CommentMap;
use crate::config::{CommaPosition, FormatConfig, KeywordCase, LogicalOperatorPosition};
use crate::format::FormatNode;
use crate::ignore::IgnoreDirectives;
use solite_ast::{Program, Statement};

/// Printer for generating formatted SQL output
pub struct Printer<'a> {
    /// Output buffer
    output: String,
    /// Current indentation level
    indent_level: usize,
    /// Format configuration
    pub config: FormatConfig,
    /// Comment map for preserving comments
    comment_map: CommentMap,
    /// Ignore directives
    ignores: IgnoreDirectives,
    /// Original source (for preserving ignored regions)
    source: &'a str,
    /// Whether we're at the start of a line
    at_line_start: bool,
    /// Current line length (for line width decisions)
    current_line_len: usize,
}

impl<'a> Printer<'a> {
    /// Create a new Printer
    pub fn new(
        config: FormatConfig,
        comment_map: CommentMap,
        ignores: IgnoreDirectives,
        source: &'a str,
    ) -> Self {
        Self {
            output: String::new(),
            indent_level: 0,
            config,
            comment_map,
            ignores,
            source,
            at_line_start: true,
            current_line_len: 0,
        }
    }

    /// Get the finished output
    pub fn finish(self) -> String {
        let mut output = self.output;
        // Trim trailing whitespace from each line and ensure single trailing newline
        output = output
            .lines()
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output
    }

    /// Format an entire program
    pub fn format_program(&mut self, program: &Program) {
        for (i, stmt) in program.statements.iter().enumerate() {
            if i > 0 {
                // Add blank lines between statements
                for _ in 0..self.config.statement_separator_lines {
                    self.newline();
                }
            }

            // Check if this statement should be ignored
            let stmt_span = Self::get_statement_span(stmt);
            if let Some((start, end)) = stmt_span {
                // Emit leading comments before the statement
                self.emit_leading_comments(start);

                if self.ignores.overlaps_ignored_region(start, end) {
                    // Preserve original source
                    self.write_raw(&self.source[start..end]);
                    continue;
                }
            }

            stmt.format(self);
            self.write(";");
        }
    }

    /// Get the span of a statement
    fn get_statement_span(stmt: &Statement) -> Option<(usize, usize)> {
        match stmt {
            Statement::Select(s) => Some((s.span.start, s.span.end)),
            Statement::Insert(s) => Some((s.span.start, s.span.end)),
            Statement::Update(s) => Some((s.span.start, s.span.end)),
            Statement::Delete(s) => Some((s.span.start, s.span.end)),
            Statement::CreateTable(s) => Some((s.span.start, s.span.end)),
            Statement::CreateIndex(s) => Some((s.span.start, s.span.end)),
            Statement::CreateView(s) => Some((s.span.start, s.span.end)),
            Statement::CreateTrigger(s) => Some((s.span.start, s.span.end)),
            Statement::AlterTable(s) => Some((s.span.start, s.span.end)),
            Statement::DropTable(s) => Some((s.span.start, s.span.end)),
            Statement::DropIndex(s) => Some((s.span.start, s.span.end)),
            Statement::DropView(s) => Some((s.span.start, s.span.end)),
            Statement::DropTrigger(s) => Some((s.span.start, s.span.end)),
            Statement::Explain { span, .. } => Some((span.start, span.end)),
            Statement::CreateVirtualTable(s) => Some((s.span.start, s.span.end)),
            Statement::Begin(s) => Some((s.span.start, s.span.end)),
            Statement::Commit(s) => Some((s.span.start, s.span.end)),
            Statement::Rollback(s) => Some((s.span.start, s.span.end)),
            Statement::Savepoint(s) => Some((s.span.start, s.span.end)),
            Statement::Release(s) => Some((s.span.start, s.span.end)),
            Statement::Vacuum(s) => Some((s.span.start, s.span.end)),
            Statement::Analyze(s) => Some((s.span.start, s.span.end)),
            Statement::Reindex(s) => Some((s.span.start, s.span.end)),
            Statement::Attach(s) => Some((s.span.start, s.span.end)),
            Statement::Detach(s) => Some((s.span.start, s.span.end)),
            Statement::Pragma(s) => Some((s.span.start, s.span.end)),
        }
    }

    /// Write text to output
    pub fn write(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        // Add indentation if at line start
        if self.at_line_start && !text.starts_with('\n') {
            let indent = self.config.indent_string().repeat(self.indent_level);
            self.output.push_str(&indent);
            self.current_line_len = indent.len();
            self.at_line_start = false;
        }

        self.output.push_str(text);
        self.current_line_len += text.len();
    }

    /// Write raw text without any processing
    pub fn write_raw(&mut self, text: &str) {
        self.output.push_str(text);
        self.at_line_start = text.ends_with('\n');
        if let Some(last_line) = text.lines().last() {
            self.current_line_len = last_line.len();
        }
    }

    /// Write a keyword with proper case transformation
    pub fn keyword(&mut self, kw: &str) {
        let transformed = match self.config.keyword_case {
            KeywordCase::Upper => kw.to_uppercase(),
            KeywordCase::Lower => kw.to_lowercase(),
            KeywordCase::Preserve => kw.to_string(),
        };
        self.write(&transformed);
    }

    /// Write a space
    pub fn space(&mut self) {
        self.write(" ");
    }

    /// Write a newline and reset to line start
    pub fn newline(&mut self) {
        self.output.push('\n');
        self.at_line_start = true;
        self.current_line_len = 0;
    }

    /// Increase indentation level
    pub fn indent(&mut self) {
        self.indent_level += 1;
    }

    /// Decrease indentation level
    pub fn dedent(&mut self) {
        if self.indent_level > 0 {
            self.indent_level -= 1;
        }
    }

    /// Write a comma, respecting comma position config
    pub fn comma(&mut self) {
        match self.config.comma_position {
            CommaPosition::Trailing => {
                self.write(",");
            }
            CommaPosition::Leading => {
                // Leading comma is written at the start of the next item
                // So we just record that a comma is needed
                self.write(",");
            }
        }
    }

    /// Write a list separator (comma + newline or space)
    pub fn list_separator(&mut self, multiline: bool) {
        if multiline {
            match self.config.comma_position {
                CommaPosition::Trailing => {
                    self.write(",");
                    self.newline();
                }
                CommaPosition::Leading => {
                    self.newline();
                    self.write(", ");
                }
            }
        } else {
            self.write(", ");
        }
    }

    /// Write a logical operator (AND/OR) with proper positioning
    pub fn logical_operator(&mut self, op: &str) {
        match self.config.logical_operator_position {
            LogicalOperatorPosition::Before => {
                self.newline();
                self.write(&self.config.indent_string());
                self.keyword(op);
                self.space();
            }
            LogicalOperatorPosition::After => {
                self.space();
                self.keyword(op);
                self.newline();
            }
        }
    }

    /// Emit leading comments for a span
    pub fn emit_leading_comments(&mut self, span_start: usize) {
        // Clone to avoid borrow conflict
        let comments = self.comment_map.get_leading(span_start).cloned();
        if let Some(comments) = comments {
            for comment in comments {
                self.write(&comment.text);
                self.newline();
            }
        }
    }

    /// Emit trailing comments for a span
    pub fn emit_trailing_comments(&mut self, span_end: usize) {
        // Clone to avoid borrow conflict
        let comments = self.comment_map.get_trailing(span_end).cloned();
        if let Some(comments) = comments {
            for comment in comments {
                self.space();
                self.write(&comment.text);
            }
        }
    }

    /// Emit trailing comments within a range of positions.
    /// Useful for catching comments attached to punctuation like commas.
    pub fn emit_trailing_comments_in_range(&mut self, start: usize, end: usize) {
        let comments: Vec<_> = self
            .comment_map
            .get_trailing_in_range(start, end)
            .into_iter()
            .cloned()
            .collect();
        for comment in comments {
            self.space();
            self.write(&comment.text);
        }
    }

    /// Check if adding text would exceed line width
    pub fn would_exceed_line_width(&self, additional: usize) -> bool {
        self.current_line_len + additional > self.config.line_width
    }

    /// Get current line length
    pub fn line_length(&self) -> usize {
        self.current_line_len
    }

    /// Check if we should use multiline formatting for a list
    pub fn should_multiline_list(&self, items: &[impl AsRef<str>]) -> bool {
        // Estimate total length
        let total_len: usize = items.iter().map(|s| s.as_ref().len() + 2).sum();
        total_len + self.current_line_len > self.config.line_width
    }

    /// Get the indent string for current level
    pub fn current_indent(&self) -> String {
        self.config.indent_string().repeat(self.indent_level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_printer() -> Printer<'static> {
        Printer::new(
            FormatConfig::default(),
            CommentMap::default(),
            IgnoreDirectives::default(),
            "",
        )
    }

    #[test]
    fn test_write() {
        let mut p = test_printer();
        p.write("SELECT");
        assert_eq!(p.output, "SELECT");
    }

    #[test]
    fn test_keyword_upper() {
        let config = FormatConfig {
            keyword_case: KeywordCase::Upper,
            ..Default::default()
        };
        let mut p = Printer::new(config, CommentMap::default(), IgnoreDirectives::default(), "");
        p.keyword("select");
        assert_eq!(p.output, "SELECT");
    }

    #[test]
    fn test_keyword_lower() {
        let mut p = test_printer();
        p.keyword("SELECT");
        assert_eq!(p.output, "select");
    }

    #[test]
    fn test_indentation() {
        let mut p = test_printer();
        p.indent();
        p.newline();
        p.write("test");
        assert!(p.output.contains("  test")); // 2 spaces (default)
    }

    #[test]
    fn test_newline() {
        let mut p = test_printer();
        p.write("a");
        p.newline();
        p.write("b");
        assert_eq!(p.output, "a\nb");
    }
}
