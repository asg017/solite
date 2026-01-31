//! Test result reporting and diagnostics.

use codespan_reporting::diagnostic::{Diagnostic, Label};
use codespan_reporting::files::SimpleFiles;
use codespan_reporting::term::{
    self,
    termcolor::{ColorChoice, StandardStream},
};

/// Report a test assertion mismatch with source location.
///
/// Displays a codespan diagnostic showing the expected vs actual values
/// at the specified line in the source file.
pub fn report_mismatch(
    file_name: &str,
    content: &str,
    line: usize,
    _column: usize,
    expected: &str,
    actual: &str,
) {
    let mut files = SimpleFiles::new();
    let id = files.add(file_name.to_string(), content.to_string());

    let lines: Vec<&str> = content.lines().collect();
    let (start, end) = compute_line_span(&lines, line);

    let diagnostic = Diagnostic::error()
        .with_message("Test assertion failed: expected vs actual mismatch")
        .with_labels(vec![Label::primary(id, start..end)
            .with_message(format!("expected: {}\nactual: {}", expected, actual))]);

    let writer = StandardStream::stderr(ColorChoice::Auto);
    let config = term::Config::default();
    let _ = term::emit(&mut writer.lock(), &config, &files, &diagnostic);
}

/// Compute the byte span for a given line number.
///
/// Returns (start_offset, end_offset) for the line.
fn compute_line_span(lines: &[&str], line: usize) -> (usize, usize) {
    if line == 0 || line > lines.len() {
        return (0, 1);
    }

    let mut start = 0usize;
    for i in 0..(line - 1) {
        start += lines[i].len();
        start += 1; // newline
    }

    let end = start + lines[line - 1].len();
    (start, end)
}

/// Test result statistics.
#[derive(Debug, Default)]
pub struct TestStats {
    /// Number of passing tests.
    pub successes: usize,
    /// Number of failing tests.
    pub failures: usize,
    /// TODO items: (file, line, column, message).
    pub todos: Vec<(String, usize, usize, String)>,
}

impl TestStats {
    /// Create a new empty stats tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a successful test.
    pub fn record_success(&mut self) {
        self.successes += 1;
    }

    /// Record a failed test.
    pub fn record_failure(&mut self) {
        self.failures += 1;
    }

    /// Record a TODO item.
    pub fn record_todo(&mut self, file: String, line: usize, col: usize, msg: String) {
        self.todos.push((file, line, col, msg));
    }

    /// Check if there were any failures or TODOs.
    pub fn has_failures(&self) -> bool {
        self.failures > 0 || !self.todos.is_empty()
    }

    /// Print the final summary.
    pub fn print_summary(&self) {
        println!();
        println!("{} successes", self.successes);
        println!("{} failures", self.failures);

        if !self.todos.is_empty() {
            println!("{} TODO(s):", self.todos.len());
            for (file, line, col, msg) in &self.todos {
                println!(" - {}:{}:{} {}", file, line, col, msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_line_span() {
        let lines = vec!["line1", "line2", "line3"];

        // Line 1
        let (start, end) = compute_line_span(&lines, 1);
        assert_eq!(start, 0);
        assert_eq!(end, 5);

        // Line 2 (after "line1\n")
        let (start, end) = compute_line_span(&lines, 2);
        assert_eq!(start, 6);
        assert_eq!(end, 11);
    }

    #[test]
    fn test_compute_line_span_invalid() {
        let lines = vec!["line1"];

        // Line 0 (invalid)
        let (start, end) = compute_line_span(&lines, 0);
        assert_eq!((start, end), (0, 1));

        // Line 10 (out of bounds)
        let (start, end) = compute_line_span(&lines, 10);
        assert_eq!((start, end), (0, 1));
    }

    #[test]
    fn test_stats_new() {
        let stats = TestStats::new();
        assert_eq!(stats.successes, 0);
        assert_eq!(stats.failures, 0);
        assert!(stats.todos.is_empty());
    }

    #[test]
    fn test_stats_record() {
        let mut stats = TestStats::new();
        stats.record_success();
        stats.record_success();
        stats.record_failure();
        stats.record_todo("file.sql".to_string(), 1, 1, "TODO".to_string());

        assert_eq!(stats.successes, 2);
        assert_eq!(stats.failures, 1);
        assert_eq!(stats.todos.len(), 1);
        assert!(stats.has_failures());
    }

    #[test]
    fn test_stats_no_failures() {
        let mut stats = TestStats::new();
        stats.record_success();
        assert!(!stats.has_failures());
    }
}
