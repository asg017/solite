use std::fmt::Write;

use crate::commands::tui::copy_popup::{CopyOption, CopyPopup};
use crate::commands::tui::help_bar::HelpBar;
use crate::commands::tui::row_page::{get_primary_keys, PrimaryKeyInfo};
use crate::commands::tui::tui_theme::TuiTheme;
use crate::commands::tui::{
    copy_to_clipboard, value_to_string, Frame, HandleKeyResult, NavigateToPage, RowPageData,
    TuiPage,
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, HorizontalAlignment, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Text;
use ratatui::widgets::{Cell, Row, Table, TableState};
use solite_core::sqlite::OwnedValue;
use solite_core::Runtime;

pub(crate) struct Data {
    pub(crate) columns: Vec<String>,
    column_widths: Vec<usize>,
    max_row_widths: Vec<usize>,
    pub(crate) rows: Vec<Vec<OwnedValue>>,
}

impl Data {
    fn empty() -> Self {
        Self {
            columns: vec![],
            column_widths: vec![],
            max_row_widths: vec![],
            rows: vec![],
        }
    }
}

#[derive(Clone)]
struct Order {
    column_idx: usize,
    direction: SortDirection,
}

/// Result of loading table data
struct LoadResult {
    data: Data,
    error: Option<String>,
}

/// Configuration for windowed data loading
const WINDOW_SIZE: usize = 200;
const PREFETCH_THRESHOLD: usize = 50;

/// Maximum characters to display in a cell before truncating
const MAX_CELL_DISPLAY_LEN: usize = 200;

/// Render an OwnedValue to a display string, truncating if necessary
fn render_value_for_display(value: &OwnedValue) -> String {
    match value {
        OwnedValue::Null => "NULL".to_owned(),
        OwnedValue::Integer(i) => i.to_string(),
        OwnedValue::Double(f) => f.to_string(),
        OwnedValue::Text(s) => {
            let text = String::from_utf8_lossy(s);
            if text.len() > MAX_CELL_DISPLAY_LEN {
                format!("{}…", &text[..MAX_CELL_DISPLAY_LEN])
            } else {
                text.into_owned()
            }
        }
        OwnedValue::Blob(b) => {
            if b.len() > 20 {
                format!("[BLOB {} bytes]", b.len())
            } else {
                format!("[BLOB]")
            }
        }
    }
}

/// Get total row count for a table
fn get_row_count(runtime: &Runtime, table: &str) -> usize {
    let sql = format!(
        "SELECT COUNT(*) FROM \"{}\"",
        table.replace('"', "\"\"")
    );
    let stmt = match runtime.connection.prepare(&sql) {
        Ok((_, Some(stmt))) => stmt,
        _ => return 0,
    };
    match stmt.next() {
        Ok(Some(row)) => row.first().map(|v| v.as_int64() as usize).unwrap_or(0),
        _ => 0,
    }
}

fn load_table_data(
    runtime: &Runtime,
    table: &str,
    order: Option<Order>,
    offset: usize,
    limit: usize,
) -> LoadResult {
    let mut sql: String = String::new();
    // Use quoted identifier to handle special table names
    let _ = writeln!(&mut sql, "SELECT * FROM \"{}\"", table.replace('"', "\"\""));
    if let Some(order) = order {
        let _ = writeln!(
            &mut sql,
            "ORDER BY {} {}",
            order.column_idx + 1,
            match order.direction {
                SortDirection::Ascending => "ASC",
                SortDirection::Descending => "DESC",
            }
        );
    }
    let _ = writeln!(&mut sql, "LIMIT {} OFFSET {}", limit, offset);

    let stmt = match runtime.connection.prepare(&sql) {
        Ok((_, Some(stmt))) => stmt,
        Ok((_, None)) => {
            return LoadResult {
                data: Data::empty(),
                error: Some("Failed to prepare query".to_owned()),
            }
        }
        Err(e) => {
            return LoadResult {
                data: Data::empty(),
                error: Some(format!("Query error: {}", e)),
            }
        }
    };

    let columns = stmt.column_names().unwrap_or_default();
    let max_row_widths = vec![100; columns.len()];
    let column_widths = columns.iter().map(|c| ansi_width::ansi_width(c)).collect();
    let mut rows = vec![];
    let mut error = None;

    loop {
        match stmt.next() {
            Ok(None) => break,
            Ok(Some(row)) => {
                let row_values: Vec<OwnedValue> = row
                    .iter()
                    .map(|v| OwnedValue::from_value_ref(v))
                    .collect();
                rows.push(row_values);
            }
            Err(e) => {
                error = Some(format!("Error reading row: {}", e));
                break;
            }
        }
    }

    LoadResult {
        data: Data {
            columns,
            column_widths,
            rows,
            max_row_widths,
        },
        error,
    }
}

pub struct TablePage<'a> {
    runtime: &'a Runtime,
    pub(crate) theme: TuiTheme,
    pub(crate) state: TableState,
    pub(crate) table_name: String,
    pub(crate) data: Data,
    pub(crate) column_idx_offset: usize,
    footer_message: Option<String>,
    n_columns_show: usize,
    error: Option<String>,
    copy_popup: CopyPopup,
    primary_keys: Vec<PrimaryKeyInfo>,
    /// Total number of rows in the table (from COUNT(*))
    pub(crate) total_rows: usize,
    /// Starting row index of the current window
    window_start: usize,
    /// Current sort order (if any)
    current_order: Option<Order>,
}

impl<'a> TablePage<'a> {
    pub(crate) fn new(table_name: &str, runtime: &'a Runtime, theme: TuiTheme) -> Self {
        let total_rows = get_row_count(runtime, table_name);
        let result = load_table_data(runtime, table_name, None, 0, WINDOW_SIZE);
        let primary_keys = get_primary_keys(runtime, table_name);
        let mut state = TableState::default();
        if !result.data.rows.is_empty() {
            state.select_first();
            state.select_first_column();
        }
        Self {
            runtime,
            theme,
            state,
            table_name: table_name.to_owned(),
            data: result.data,
            n_columns_show: 5,
            column_idx_offset: 0,
            footer_message: None,
            error: result.error,
            copy_popup: CopyPopup::new(),
            primary_keys,
            total_rows,
            window_start: 0,
            current_order: None,
        }
    }

    /// Ensure the given absolute row index is loaded in the current window.
    /// If not, reload a window centered around that row.
    fn ensure_row_loaded(&mut self, absolute_row: usize) {
        let window_end = self.window_start + self.data.rows.len();

        // Check if row is already in window with enough buffer
        let near_start = absolute_row < self.window_start + PREFETCH_THRESHOLD;
        let near_end = absolute_row + PREFETCH_THRESHOLD >= window_end;

        if absolute_row < self.window_start || absolute_row >= window_end ||
           (near_start && self.window_start > 0) ||
           (near_end && window_end < self.total_rows) {
            // Center the window around the target row
            let new_start = absolute_row.saturating_sub(WINDOW_SIZE / 2);
            let new_start = new_start.min(self.total_rows.saturating_sub(WINDOW_SIZE));

            let result = load_table_data(
                self.runtime,
                &self.table_name,
                self.current_order.clone(),
                new_start,
                WINDOW_SIZE,
            );

            if result.error.is_none() {
                self.data = result.data;
                self.window_start = new_start;
            } else {
                self.error = result.error;
            }
        }
    }

    /// Convert absolute row index to window-relative index
    fn absolute_to_window(&self, absolute: usize) -> Option<usize> {
        if absolute >= self.window_start && absolute < self.window_start + self.data.rows.len() {
            Some(absolute - self.window_start)
        } else {
            None
        }
    }

    /// Convert window-relative index to absolute row index
    fn window_to_absolute(&self, window_idx: usize) -> usize {
        self.window_start + window_idx
    }

    /// Get the currently selected absolute row index
    fn selected_absolute_row(&self) -> Option<usize> {
        self.state.selected().map(|window_idx| self.window_to_absolute(window_idx))
    }

    fn sort(&mut self, direction: SortDirection) {
        let col_idx = self
            .state
            .selected_column()
            .unwrap_or(0)
            .saturating_add(self.column_idx_offset);
        let order = Order {
            column_idx: col_idx,
            direction,
        };
        let result = load_table_data(
            self.runtime,
            &self.table_name,
            Some(order.clone()),
            0,
            WINDOW_SIZE,
        );
        self.data = result.data;
        self.window_start = 0;
        self.current_order = Some(order);
        // Reset selection to first row after sort
        self.state.select_first();
        if let Some(err) = result.error {
            self.footer_message = Some(format!("Sort error: {}", err));
        }
    }

    /// Generate TSV for a single row
    fn row_to_tsv(&self, row_idx: usize) -> String {
        self.data.rows[row_idx]
            .iter()
            .map(value_to_string)
            .collect::<Vec<_>>()
            .join("\t")
    }

    /// Generate TSV for the entire table
    fn table_to_tsv(&self) -> String {
        let header = self.data.columns.join("\t");
        let rows: Vec<String> = self
            .data
            .rows
            .iter()
            .map(|row| row.iter().map(value_to_string).collect::<Vec<_>>().join("\t"))
            .collect();
        format!("{}\n{}", header, rows.join("\n"))
    }

    /// Generate a SELECT statement for this table
    fn generate_select(&self) -> String {
        format!("SELECT * FROM \"{}\";", self.table_name.replace('"', "\"\""))
    }

    /// Generate INSERT statements for the data
    fn generate_inserts(&self) -> String {
        if self.data.rows.is_empty() {
            return format!("-- No data in table \"{}\"", self.table_name);
        }

        let cols = self.data.columns.join("\", \"");
        self.data
            .rows
            .iter()
            .map(|row| {
                let values: Vec<String> = row
                    .iter()
                    .map(|v| match v {
                        OwnedValue::Null => "NULL".to_owned(),
                        OwnedValue::Integer(i) => i.to_string(),
                        OwnedValue::Double(f) => f.to_string(),
                        OwnedValue::Text(s) => {
                            let text = String::from_utf8_lossy(s);
                            format!("'{}'", text.replace('\'', "''"))
                        }
                        OwnedValue::Blob(b) => format!("X'{}'", hex::encode(b)),
                    })
                    .collect();
                format!(
                    "INSERT INTO \"{}\" (\"{}\") VALUES ({});",
                    self.table_name.replace('"', "\"\""),
                    cols,
                    values.join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Execute a copy operation based on the selected option
    fn execute_copy(&mut self, option: CopyOption) {
        let (content, description) = match option {
            CopyOption::Cell => {
                if let Some((row, col)) = self.state.selected_cell() {
                    let actual_col = col.saturating_add(self.column_idx_offset);
                    if row < self.data.rows.len() && actual_col < self.data.rows[row].len() {
                        let value = &self.data.rows[row][actual_col];
                        (value_to_string(value), "cell")
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
            CopyOption::Row => {
                if let Some((row, _)) = self.state.selected_cell() {
                    if row < self.data.rows.len() {
                        (self.row_to_tsv(row), "row")
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
            CopyOption::Table => (self.table_to_tsv(), "table"),
            CopyOption::SqlSelect => (self.generate_select(), "SELECT"),
            CopyOption::SqlInsert => (self.generate_inserts(), "INSERT statements"),
        };

        match copy_to_clipboard(&content) {
            Ok(()) => {
                self.footer_message = Some(format!("Copied {} to clipboard", description));
            }
            Err(e) => {
                self.footer_message = Some(e);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum SortDirection {
    Ascending,
    Descending,
}

impl TuiPage for TablePage<'_> {
    fn handle_key(&mut self, key: KeyEvent) -> HandleKeyResult {
        // Handle copy popup first if visible
        if self.copy_popup.visible {
            if let Some(option) = self.copy_popup.handle_key(key) {
                self.execute_copy(option);
            }
            return HandleKeyResult::None;
        }

        // Clear footer message on any key press
        self.footer_message = None;

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => HandleKeyResult::Navigate(NavigateToPage::Listing),
            KeyCode::Char('Q') => HandleKeyResult::Quit,
            KeyCode::Char('[') => {
                self.sort(SortDirection::Ascending);
                HandleKeyResult::None
            }
            KeyCode::Char(']') => {
                self.sort(SortDirection::Descending);
                HandleKeyResult::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(current) = self.state.selected() {
                    let absolute = self.window_to_absolute(current);
                    if absolute + 1 < self.total_rows {
                        let next_absolute = absolute + 1;
                        // Check if we need to load a new window
                        if next_absolute >= self.window_start + self.data.rows.len() {
                            self.ensure_row_loaded(next_absolute);
                        }
                        // Update selection to new window-relative position
                        if let Some(new_window_idx) = self.absolute_to_window(next_absolute) {
                            self.state.select(Some(new_window_idx));
                        }
                    }
                } else {
                    self.state.select_first();
                }
                HandleKeyResult::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(current) = self.state.selected() {
                    let absolute = self.window_to_absolute(current);
                    if absolute > 0 {
                        let prev_absolute = absolute - 1;
                        // Check if we need to load a new window
                        if prev_absolute < self.window_start {
                            self.ensure_row_loaded(prev_absolute);
                        }
                        // Update selection to new window-relative position
                        if let Some(new_window_idx) = self.absolute_to_window(prev_absolute) {
                            self.state.select(Some(new_window_idx));
                        }
                    }
                } else {
                    self.state.select_first();
                }
                HandleKeyResult::None
            }
            // Page down (Ctrl+d or PageDown)
            KeyCode::PageDown => {
                let page_size = 20; // Approximate visible rows
                if let Some(current) = self.state.selected() {
                    let absolute = self.window_to_absolute(current);
                    let target = (absolute + page_size).min(self.total_rows.saturating_sub(1));
                    self.ensure_row_loaded(target);
                    if let Some(window_idx) = self.absolute_to_window(target) {
                        self.state.select(Some(window_idx));
                    }
                }
                HandleKeyResult::None
            }
            // Page up (Ctrl+u or PageUp)
            KeyCode::PageUp => {
                let page_size = 20; // Approximate visible rows
                if let Some(current) = self.state.selected() {
                    let absolute = self.window_to_absolute(current);
                    let target = absolute.saturating_sub(page_size);
                    self.ensure_row_loaded(target);
                    if let Some(window_idx) = self.absolute_to_window(target) {
                        self.state.select(Some(window_idx));
                    }
                }
                HandleKeyResult::None
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if let Some(idx) = self.state.selected_column() {
                    if idx >= (self.n_columns_show - 1)
                        && self.column_idx_offset + self.n_columns_show < self.data.columns.len()
                    {
                        self.column_idx_offset += 1;
                    } else {
                        self.state.select_next_column();
                    }
                } else {
                    self.state.select_next_column();
                }
                HandleKeyResult::None
            }
            KeyCode::Char('h') | KeyCode::Left => {
                if let Some(idx) = self.state.selected_column() {
                    if idx == 0 && self.column_idx_offset > 0 {
                        self.column_idx_offset -= 1;
                    } else {
                        self.state.select_previous_column();
                    }
                } else {
                    self.state.select_previous_column();
                }
                HandleKeyResult::None
            }
            KeyCode::Char('g') => {
                // Jump to first row
                if self.total_rows > 0 {
                    self.ensure_row_loaded(0);
                    self.state.select(Some(0));
                }
                HandleKeyResult::None
            }
            KeyCode::Char('G') => {
                // Jump to last row
                if self.total_rows > 0 {
                    let last_row = self.total_rows - 1;
                    self.ensure_row_loaded(last_row);
                    if let Some(window_idx) = self.absolute_to_window(last_row) {
                        self.state.select(Some(window_idx));
                    }
                }
                HandleKeyResult::None
            }
            KeyCode::Char('L') => {
                self.state.select_last_column();
                if self.data.columns.len() > self.n_columns_show {
                    self.column_idx_offset =
                        self.data.columns.len().saturating_sub(self.n_columns_show);
                }
                HandleKeyResult::None
            }
            KeyCode::Char('H') => {
                self.state.select_first_column();
                self.column_idx_offset = 0;
                HandleKeyResult::None
            }
            // Open copy popup
            KeyCode::Char('y') | KeyCode::Char('c') => {
                self.copy_popup.show();
                HandleKeyResult::None
            }
            // Navigate to row detail view
            KeyCode::Enter => {
                if let Some((window_idx, _)) = self.state.selected_cell() {
                    if window_idx < self.data.rows.len() {
                        let absolute_row = self.window_to_absolute(window_idx);
                        let data = RowPageData {
                            table_name: self.table_name.clone(),
                            row_index: absolute_row,
                            columns: self.data.columns.clone(),
                            values: self.data.rows[window_idx].clone(),
                            primary_keys: self.primary_keys.clone(),
                        };
                        return HandleKeyResult::Navigate(NavigateToPage::Row(data));
                    }
                }
                HandleKeyResult::None
            }
            _ => HandleKeyResult::None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let layout = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(2),
        ]);
        let [table_rect, message_rect, help_rect] = area.layout(&layout);

        let selected_header_idx = self
            .state
            .selected_column()
            .unwrap_or(0)
            .saturating_add(self.column_idx_offset);

        let header = Row::new(self.data.columns.iter().skip(self.column_idx_offset).enumerate().map(
            |(idx, c)| {
                Cell::from(Text::from(c.as_str())).style(
                    Style::new()
                        .bold()
                        .fg(self.theme.header_fg.clone().into())
                        .bg(
                            if selected_header_idx == idx.saturating_add(self.column_idx_offset) {
                                self.theme.header_selected_bg.clone().into()
                            } else {
                                self.theme.header_bg.clone().into()
                            },
                        ),
                )
            },
        ))
        .style(
            Style::new()
                .bold()
                .fg(self.theme.header_style_fg.clone().into()),
        );

        let rows = self.data.rows.iter().map(|r| {
            Row::new(r.iter().skip(self.column_idx_offset).map(|value| {
                let display_text = render_value_for_display(value);
                let text = match value {
                    OwnedValue::Integer(_) | OwnedValue::Double(_) => {
                        Text::from(display_text).alignment(HorizontalAlignment::Right)
                    }
                    _ => Text::from(display_text),
                };
                Cell::default()
                    .content(text)
                    .style(match value {
                        OwnedValue::Null => Style::new().fg(self.theme.null.clone().into()),
                        OwnedValue::Integer(_) => {
                            Style::new().fg(self.theme.integer.clone().into())
                        }
                        OwnedValue::Double(_) => Style::new().fg(self.theme.double.clone().into()),
                        OwnedValue::Text(_) => Style::new().fg(self.theme.text.clone().into()),
                        OwnedValue::Blob(_) => Style::new().fg(self.theme.blob.clone().into()),
                    })
            }))
        });

        let widths: Vec<Constraint> = self
            .data
            .columns
            .iter()
            .skip(self.column_idx_offset)
            .take(self.n_columns_show)
            .map(|_| Constraint::Fill(1))
            .collect();

        let table = Table::new(rows, widths)
            .header(header)
            .column_spacing(1)
            .style(Style::new().fg(self.theme.table_fg.clone().into()))
            .row_highlight_style(Style::new().bold().bg(self.theme.row_hl_bg.clone().into()))
            .cell_highlight_style(
                Style::new()
                    .bold()
                    .fg(self.theme.cell_hl_fg.clone().into())
                    .bg(self.theme.cell_hl_bg.clone().into()),
            );

        frame.render_stateful_widget(table, table_rect, &mut self.state);

        // Footer message (copy confirmation, errors, position indicator)
        if let Some(msg) = &self.footer_message {
            use ratatui::style::Color;
            let style = if msg.starts_with("Copied") || msg.starts_with("✓") {
                Style::new().fg(Color::Green)
            } else {
                Style::new().fg(Color::Red)
            };
            frame.render_widget(
                Text::from(msg.as_str()).style(style).centered(),
                message_rect,
            );
        } else if let Some(ref error) = self.error {
            use ratatui::style::Color;
            frame.render_widget(
                Text::from(format!("Error: {}", error))
                    .style(Style::new().fg(Color::Red))
                    .centered(),
                message_rect,
            );
        } else if self.total_rows > 0 {
            // Show position indicator
            use ratatui::style::Color;
            let current_row = self.selected_absolute_row().map(|r| r + 1).unwrap_or(0);
            let position_text = format!("Row {} of {}", current_row, self.total_rows);
            frame.render_widget(
                Text::from(position_text)
                    .style(Style::new().fg(Color::DarkGray))
                    .centered(),
                message_rect,
            );
        }

        // Help bar
        HelpBar::new()
            .keys(vec!["h", "j", "k", "l"], " navigate")
            .item("Enter", " view row")
            .item("[", " sort asc")
            .item("]", " desc")
            .separator()
            .keys(vec!["y", "c"], " copy")
            .item("q", " back")
            .render(frame, help_rect);

        // Copy popup (renders on top)
        self.copy_popup.render(frame, area);
    }
}
