//! Configuration for table rendering.

use crate::theme::Theme;

/// Output mode for table rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Terminal output with ANSI colors.
    Terminal,
    /// String output with ANSI codes.
    StringAnsi,
    /// Plain string output without any formatting.
    StringPlain,
    /// HTML output for Jupyter notebooks.
    Html,
}

impl Default for OutputMode {
    fn default() -> Self {
        Self::Terminal
    }
}

/// Configuration for table rendering.
#[derive(Debug, Clone)]
pub struct TableConfig {
    /// Output mode (terminal, string, HTML).
    pub output_mode: OutputMode,
    /// Maximum width for output. None means auto-detect terminal width.
    pub max_width: Option<usize>,
    /// Number of rows to show at the head of large results.
    pub head_rows: usize,
    /// Number of rows to show at the tail of large results.
    pub tail_rows: usize,
    /// Maximum width for a single cell before truncation.
    pub max_cell_width: usize,
    /// Theme for colors. None means no colors.
    pub theme: Option<Theme>,
    /// Whether to show the footer with row/column counts.
    pub show_footer: bool,
}

impl Default for TableConfig {
    fn default() -> Self {
        Self {
            output_mode: OutputMode::Terminal,
            max_width: None,
            head_rows: 20,
            tail_rows: 20,
            max_cell_width: 100,
            theme: Some(Theme::catppuccin_mocha()),
            show_footer: true,
        }
    }
}

impl TableConfig {
    /// Create a new config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a config for terminal output with auto-detected width.
    pub fn terminal() -> Self {
        Self::default()
    }

    /// Create a config for plain string output (no colors, no terminal detection).
    pub fn plain() -> Self {
        Self {
            output_mode: OutputMode::StringPlain,
            max_width: Some(120),
            theme: None,
            ..Self::default()
        }
    }

    /// Create a config for HTML output (Jupyter).
    /// Shows all rows and full cell contents without truncation.
    pub fn html() -> Self {
        Self {
            output_mode: OutputMode::Html,
            max_width: None,
            // Show all rows - use large value that won't cause allocation issues
            head_rows: 1_000_000,
            tail_rows: 0,
            // Show full cell contents without truncation
            max_cell_width: 100_000,
            theme: Some(Theme::catppuccin_mocha()),
            show_footer: true,
        }
    }

    /// Get the effective max width, auto-detecting terminal if needed.
    /// For HTML output, returns a large value to show all columns.
    pub fn effective_width(&self) -> usize {
        match self.max_width {
            Some(w) => w,
            None => match self.output_mode {
                // HTML should show all columns - no width limit
                OutputMode::Html => usize::MAX / 2,
                // Terminal modes auto-detect or use default
                _ => term_size::dimensions().map(|(w, _)| w).unwrap_or(120),
            },
        }
    }

    /// Get the total number of rows to retain (head + tail).
    pub fn total_retained_rows(&self) -> usize {
        self.head_rows + self.tail_rows
    }

    /// Builder method to set output mode.
    pub fn with_output_mode(mut self, mode: OutputMode) -> Self {
        self.output_mode = mode;
        self
    }

    /// Builder method to set max width.
    pub fn with_max_width(mut self, width: Option<usize>) -> Self {
        self.max_width = width;
        self
    }

    /// Builder method to set theme.
    pub fn with_theme(mut self, theme: Option<Theme>) -> Self {
        self.theme = theme;
        self
    }

    /// Builder method to set head/tail rows.
    pub fn with_row_limits(mut self, head: usize, tail: usize) -> Self {
        self.head_rows = head;
        self.tail_rows = tail;
        self
    }

    /// Builder method to set footer visibility.
    pub fn with_footer(mut self, show: bool) -> Self {
        self.show_footer = show;
        self
    }
}
