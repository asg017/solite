use std::fmt::Write;

use crate::commands::tui::copy_popup::{CopyOption, CopyPopup};
use crate::commands::tui::help_popup::{help_bar_from, HelpPopup, TABLE_KEYS};
use crate::commands::tui::row_page::{get_primary_keys, PrimaryKeyInfo};
use crate::commands::tui::utils::render_value_for_display;
use crate::commands::tui::tui_theme::TuiTheme;
use crate::commands::tui::{
    value_to_string, Frame, HandleKeyResult, NavigateToPage, RowPageData, SharedClipboard,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, HorizontalAlignment, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Text;
use ratatui::widgets::{Cell, Row, Table, TableState};
use solite_core::sqlite::OwnedValue;
use solite_core::Runtime;

#[derive(Debug)]
pub(crate) struct Data {
    pub(crate) columns: Vec<String>,
    pub(crate) rows: Vec<Vec<OwnedValue>>,
}

impl Data {
    fn empty() -> Self {
        Self {
            columns: vec![],
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

/// Maximum number of rows a full-table copy will put on the clipboard.
/// Larger tables are truncated (with an honest footer message) — the
/// clipboard is the wrong channel for huge tables; `.export` exists for that.
const COPY_ROW_LIMIT: usize = 100_000;

/// Rows to count per incremental batch
const COUNT_BATCH_SIZE: usize = 60493;

/// Spinner characters for counting animation
const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Tracks row count with incremental discovery
pub(crate) struct RowCount {
    /// Minimum known row count (from loaded data)
    known: usize,
    /// Whether we've found the actual end
    pub(crate) is_complete: bool,
    /// Next offset to probe when counting
    probe_offset: usize,
    /// Spinner frame for animation
    spinner_frame: usize,
}

impl RowCount {
    fn new(initial_known: usize) -> Self {
        Self {
            known: initial_known,
            is_complete: initial_known == 0, // Empty table is complete
            probe_offset: initial_known,
            spinner_frame: 0,
        }
    }

    /// Update known count from loaded data
    fn update_from_load(&mut self, window_start: usize, loaded_count: usize) {
        let new_known = window_start + loaded_count;
        if new_known > self.known {
            self.known = new_known;
            // If we loaded less than a full window, we've found the end
            if loaded_count < WINDOW_SIZE {
                self.is_complete = true;
            }
        }
    }

    /// Count a batch of rows to discover more. Returns true if still counting.
    fn count_batch(&mut self, runtime: &Runtime, table: &str) -> bool {
        if self.is_complete {
            return false;
        }

        let sql = format!(
            "SELECT 1 FROM \"{}\" LIMIT {} OFFSET {}",
            table.replace('"', "\"\""),
            COUNT_BATCH_SIZE,
            self.probe_offset
        );

        let mut stmt = match runtime.connection.prepare(&sql) {
            Ok((_, Some(stmt))) => stmt,
            _ => {
                self.is_complete = true;
                return false;
            }
        };

        let mut batch_count = 0;
        while let Ok(Some(_)) = stmt.next() {
            batch_count += 1;
        }

        self.probe_offset += batch_count;
        if self.probe_offset > self.known {
            self.known = self.probe_offset;
        }
        // Advance spinner
        self.spinner_frame = (self.spinner_frame + 1) % SPINNER_CHARS.len();

        if batch_count < COUNT_BATCH_SIZE {
            self.is_complete = true;
            false
        } else {
            true // More to count
        }
    }

    /// Get display string for row count with formatting
    fn display(&self) -> String {
        use super::format_number;
        let formatted = format_number(self.known);
        if self.is_complete {
            formatted
        } else {
            let spinner = SPINNER_CHARS[self.spinner_frame];
            format!("{}+ {}", formatted, spinner)
        }
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

    let mut stmt = match runtime.connection.prepare(&sql) {
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
        data: Data { columns, rows },
        error,
    }
}

/// Load up to `cap` rows of the whole table (respecting the active sort
/// order) for a full-table copy. Returns the data plus whether the table was
/// truncated at `cap`.
fn load_table_for_copy(
    runtime: &Runtime,
    table: &str,
    order: Option<Order>,
    cap: usize,
) -> Result<(Data, bool), String> {
    // Fetch one extra row so truncation can be detected without a count.
    let result = load_table_data(runtime, table, order, 0, cap + 1);
    if let Some(err) = result.error {
        return Err(err);
    }
    let mut data = result.data;
    let truncated = data.rows.len() > cap;
    if truncated {
        data.rows.truncate(cap);
    }
    Ok((data, truncated))
}

/// Escape a string for use as a TSV field: embedded tabs and newlines would
/// silently shift columns/rows in the pasted output, so render them as
/// visible `\t`/`\n`/`\r` escapes instead.
fn tsv_escape(s: String) -> String {
    if s.contains(['\t', '\n', '\r']) {
        s.replace('\t', "\\t").replace('\n', "\\n").replace('\r', "\\r")
    } else {
        s
    }
}

/// Generate TSV (header + rows) for the given data.
fn data_to_tsv(data: &Data) -> String {
    let header = data
        .columns
        .iter()
        .map(|c| tsv_escape(c.clone()))
        .collect::<Vec<_>>()
        .join("\t");
    let rows: Vec<String> = data
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|v| tsv_escape(value_to_string(v)))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect();
    format!("{}\n{}", header, rows.join("\n"))
}

/// Generate INSERT statements for the given data.
fn data_to_inserts(table_name: &str, data: &Data) -> String {
    if data.rows.is_empty() {
        return format!("-- No data in table \"{}\"", table_name);
    }

    // Double-quote escaping for identifiers, same idiom as the table name.
    let cols = data
        .columns
        .iter()
        .map(|c| c.replace('"', "\"\""))
        .collect::<Vec<_>>()
        .join("\", \"");
    data.rows
        .iter()
        .map(|row| {
            let values: Vec<String> = row
                .iter()
                .map(|v| match v {
                    OwnedValue::Null => "NULL".to_owned(),
                    OwnedValue::Integer(i) => i.to_string(),
                    OwnedValue::Double(f) => f.to_string(),
                    OwnedValue::Text(s) => {
                        // Single-quote doubling is the correct SQL escaping.
                        // Invalid UTF-8 in TEXT values is silently replaced by
                        // from_utf8_lossy; BLOBs round-trip exactly via hex.
                        let text = String::from_utf8_lossy(s);
                        format!("'{}'", text.replace('\'', "''"))
                    }
                    OwnedValue::Blob(b) => format!("X'{}'", hex::encode(b)),
                })
                .collect();
            format!(
                "INSERT INTO \"{}\" (\"{}\") VALUES ({});",
                table_name.replace('"', "\"\""),
                cols,
                values.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    help_popup: HelpPopup,
    primary_keys: Vec<PrimaryKeyInfo>,
    /// Row count tracker (streams count incrementally)
    pub(crate) row_count: RowCount,
    /// Starting row index of the current window
    window_start: usize,
    /// Current sort order (if any)
    current_order: Option<Order>,
    /// Destination for copy operations
    clipboard: SharedClipboard,
}

impl<'a> TablePage<'a> {
    pub(crate) fn new(
        table_name: &str,
        runtime: &'a Runtime,
        theme: TuiTheme,
        clipboard: SharedClipboard,
    ) -> Self {
        let result = load_table_data(runtime, table_name, None, 0, WINDOW_SIZE);
        let primary_keys = get_primary_keys(runtime, table_name);
        let mut state = TableState::default();
        if !result.data.rows.is_empty() {
            state.select_first();
            state.select_first_column();
        }
        let row_count = RowCount::new(result.data.rows.len());
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
            help_popup: HelpPopup::new(" Help — Table ", TABLE_KEYS),
            primary_keys,
            row_count,
            window_start: 0,
            current_order: None,
            clipboard,
        }
    }

    /// Get the known row count (may be incomplete)
    pub(crate) fn total_rows(&self) -> usize {
        self.row_count.known
    }

    /// Ensure the given absolute row index is loaded in the current window.
    /// If not, reload a window centered around that row.
    fn ensure_row_loaded(&mut self, absolute_row: usize) {
        let window_end = self.window_start + self.data.rows.len();

        // Check if row is already in window with enough buffer
        let near_start = absolute_row < self.window_start + PREFETCH_THRESHOLD;
        let near_end = absolute_row + PREFETCH_THRESHOLD >= window_end;

        // Use row_count.known as estimate, but may load beyond if count is incomplete
        let should_reload = absolute_row < self.window_start
            || absolute_row >= window_end
            || (near_start && self.window_start > 0)
            || (near_end && !self.row_count.is_complete);

        if should_reload {
            // Center the window around the target row
            let new_start = absolute_row.saturating_sub(WINDOW_SIZE / 2);

            let result = load_table_data(
                self.runtime,
                &self.table_name,
                self.current_order.clone(),
                new_start,
                WINDOW_SIZE,
            );

            if result.error.is_none() {
                self.window_start = new_start;
                // Update row count from loaded data
                self.row_count.update_from_load(new_start, result.data.rows.len());
                self.data = result.data;
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
        self.window_start = 0;
        self.current_order = Some(order);
        // Reset row count - will re-discover during navigation
        self.row_count = RowCount::new(result.data.rows.len());
        self.data = result.data;
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
            .map(|v| tsv_escape(value_to_string(v)))
            .collect::<Vec<_>>()
            .join("\t")
    }

    /// Generate a SELECT statement for this table
    fn generate_select(&self) -> String {
        format!("SELECT * FROM \"{}\";", self.table_name.replace('"', "\"\""))
    }

    /// Load the full table (up to COPY_ROW_LIMIT rows, respecting the active
    /// sort) for a whole-table copy. Returns the data plus the success
    /// message describing what was copied.
    fn load_for_full_copy(&self, what: &str) -> Result<(Data, String), String> {
        let (data, truncated) = load_table_for_copy(
            self.runtime,
            &self.table_name,
            self.current_order.clone(),
            COPY_ROW_LIMIT,
        )?;
        let message = if truncated {
            format!(
                "Copied first {} rows as {} (table larger than copy limit)",
                super::format_number(COPY_ROW_LIMIT),
                what
            )
        } else {
            format!("Copied table as {} to clipboard", what)
        };
        Ok((data, message))
    }

    /// Execute a copy operation based on the selected option
    fn execute_copy(&mut self, option: CopyOption) {
        let (content, message) = match option {
            CopyOption::Cell => {
                if let Some((row, col)) = self.state.selected_cell() {
                    let actual_col = col.saturating_add(self.column_idx_offset);
                    if row < self.data.rows.len() && actual_col < self.data.rows[row].len() {
                        let value = &self.data.rows[row][actual_col];
                        (value_to_string(value), "Copied cell to clipboard".to_owned())
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
                        (self.row_to_tsv(row), "Copied row to clipboard".to_owned())
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
            CopyOption::Table => match self.load_for_full_copy("TSV") {
                Ok((data, message)) => (data_to_tsv(&data), message),
                Err(e) => {
                    self.footer_message = Some(format!("Copy failed: {}", e));
                    return;
                }
            },
            CopyOption::SqlSelect => (
                self.generate_select(),
                "Copied SELECT to clipboard".to_owned(),
            ),
            CopyOption::SqlInsert => match self.load_for_full_copy("INSERT statements") {
                Ok((data, message)) => (data_to_inserts(&self.table_name, &data), message),
                Err(e) => {
                    self.footer_message = Some(format!("Copy failed: {}", e));
                    return;
                }
            },
        };

        match self.clipboard.borrow_mut().set_text(content) {
            Ok(()) => {
                self.footer_message = Some(message);
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

impl TablePage<'_> {
    /// Move the selection down one page (PageDown / Ctrl+d).
    fn page_down(&mut self) {
        let page_size = 20; // Approximate visible rows
        if let Some(current) = self.state.selected() {
            let absolute = self.window_to_absolute(current);
            let target = absolute
                .saturating_add(page_size)
                .min(self.row_count.known.saturating_sub(1));
            self.ensure_row_loaded(target);
            if let Some(window_idx) = self.absolute_to_window(target) {
                self.state.select(Some(window_idx));
            }
        }
    }

    /// Move the selection up one page (PageUp / Ctrl+u).
    fn page_up(&mut self) {
        let page_size = 20; // Approximate visible rows
        if let Some(current) = self.state.selected() {
            let absolute = self.window_to_absolute(current);
            let target = absolute.saturating_sub(page_size);
            self.ensure_row_loaded(target);
            if let Some(window_idx) = self.absolute_to_window(target) {
                self.state.select(Some(window_idx));
            }
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> HandleKeyResult {
        // Popups consume keys while visible
        if self.help_popup.visible {
            self.help_popup.handle_key(key);
            return HandleKeyResult::None;
        }
        if self.copy_popup.visible {
            if let Some(option) = self.copy_popup.handle_key(key) {
                self.execute_copy(option);
            }
            return HandleKeyResult::None;
        }

        // Clear footer message on any key press
        self.footer_message = None;

        // Ctrl+d / Ctrl+u page like PageDown / PageUp
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('d') => {
                    self.page_down();
                    return HandleKeyResult::None;
                }
                KeyCode::Char('u') => {
                    self.page_up();
                    return HandleKeyResult::None;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('?') => {
                self.help_popup.show();
                HandleKeyResult::None
            }
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
                    if absolute + 1 < self.row_count.known {
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
            KeyCode::PageDown => {
                self.page_down();
                HandleKeyResult::None
            }
            KeyCode::PageUp => {
                self.page_up();
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
                if self.row_count.known > 0 {
                    self.ensure_row_loaded(0);
                    self.state.select(Some(0));
                }
                HandleKeyResult::None
            }
            KeyCode::Char('G') => {
                // Jump to last row
                if self.row_count.known > 0 {
                    let last_row = self.row_count.known - 1;
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

    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect) {
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
        } else if self.row_count.known > 0 || !self.row_count.is_complete {
            // Show position indicator with streaming count
            use super::format_number;
            use ratatui::style::Color;
            let current_row = self.selected_absolute_row().map(|r| r + 1).unwrap_or(0);
            let current_row_display = format_number(current_row);
            let count_display = self.row_count.display();
            let position_text = format!("Row {} of {}", current_row_display, count_display);
            frame.render_widget(
                Text::from(position_text)
                    .style(Style::new().fg(Color::DarkGray))
                    .centered(),
                message_rect,
            );

            // Continue counting in background if not complete
            if !self.row_count.is_complete {
                self.row_count.count_batch(self.runtime, &self.table_name);
            }
        }

        // Help bar
        help_bar_from(TABLE_KEYS).render(frame, help_rect);

        // Popups (render on top)
        self.copy_popup.render(frame, area);
        self.help_popup.render(frame, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory runtime with a `nums(n)` table of `count` rows (1..=count).
    fn runtime_with_rows(count: usize) -> Runtime {
        let runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(&format!(
                "CREATE TABLE nums AS WITH RECURSIVE c(n) AS \
                 (SELECT 1 UNION ALL SELECT n+1 FROM c LIMIT {}) SELECT n FROM c",
                count
            ))
            .unwrap();
        runtime
    }

    #[test]
    fn test_full_table_copy_covers_more_than_one_window() {
        let count = WINDOW_SIZE * 2 + 50;
        let runtime = runtime_with_rows(count);
        let (data, truncated) =
            load_table_for_copy(&runtime, "nums", None, COPY_ROW_LIMIT).unwrap();
        assert!(!truncated);
        assert_eq!(data.rows.len(), count);

        // TSV: header + every row, not just the 200-row window
        let tsv = data_to_tsv(&data);
        assert_eq!(tsv.lines().count(), count + 1);
        assert_eq!(tsv.lines().next().unwrap(), "n");
        assert_eq!(tsv.lines().last().unwrap(), count.to_string());

        // INSERT statements: one per row
        let inserts = data_to_inserts("nums", &data);
        assert_eq!(inserts.lines().count(), count);
        assert!(inserts
            .lines()
            .last()
            .unwrap()
            .contains(&format!("VALUES ({})", count)));
    }

    #[test]
    fn test_full_table_copy_respects_sort_order() {
        let count = WINDOW_SIZE + 10;
        let runtime = runtime_with_rows(count);
        let order = Order {
            column_idx: 0,
            direction: SortDirection::Descending,
        };
        let (data, truncated) =
            load_table_for_copy(&runtime, "nums", Some(order), COPY_ROW_LIMIT).unwrap();
        assert!(!truncated);
        assert!(matches!(data.rows[0][0], OwnedValue::Integer(i) if i == count as i64));
        assert!(matches!(data.rows[count - 1][0], OwnedValue::Integer(1)));
    }

    #[test]
    fn test_full_table_copy_truncates_at_cap() {
        let runtime = runtime_with_rows(50);
        let (data, truncated) = load_table_for_copy(&runtime, "nums", None, 30).unwrap();
        assert!(truncated);
        assert_eq!(data.rows.len(), 30);
    }

    #[test]
    fn test_inserts_escape_quoted_column_names() {
        let data = Data {
            columns: vec!["a\"b".to_owned(), "plain".to_owned()],
            rows: vec![vec![OwnedValue::Integer(1), OwnedValue::Text(b"x".to_vec())]],
        };
        let inserts = data_to_inserts("t", &data);
        assert_eq!(inserts, "INSERT INTO \"t\" (\"a\"\"b\", \"plain\") VALUES (1, 'x');");
    }

    #[test]
    fn test_tsv_escapes_tabs_and_newlines() {
        let data = Data {
            columns: vec!["col\ta".to_owned(), "b".to_owned()],
            rows: vec![vec![
                OwnedValue::Text(b"has\ttab".to_vec()),
                OwnedValue::Text(b"has\nnewline\rcr".to_vec()),
            ]],
        };
        let tsv = data_to_tsv(&data);
        let lines: Vec<&str> = tsv.lines().collect();
        // One header line + one row line: embedded newlines never split rows
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "col\\ta\tb");
        // Each line has exactly one (separator) tab: embedded tabs are escaped
        assert_eq!(lines[1], "has\\ttab\thas\\nnewline\\rcr");
    }

    #[test]
    fn test_full_table_copy_reports_query_errors() {
        let runtime = Runtime::new(None).unwrap();
        let err = load_table_for_copy(&runtime, "no_such_table", None, 10).unwrap_err();
        assert!(err.contains("no_such_table"));
    }

    #[test]
    fn test_row_count_new() {
        // an empty initial load means the table is empty: counting is done
        let empty = RowCount::new(0);
        assert!(empty.is_complete);
        assert_eq!(empty.known, 0);

        // a full initial window means there may be more rows
        let partial = RowCount::new(WINDOW_SIZE);
        assert!(!partial.is_complete);
        assert_eq!(partial.known, WINDOW_SIZE);
    }

    #[test]
    fn test_row_count_update_from_load() {
        let mut rc = RowCount::new(WINDOW_SIZE);

        // a full window deeper in the table extends the known count
        rc.update_from_load(WINDOW_SIZE, WINDOW_SIZE);
        assert_eq!(rc.known, 2 * WINDOW_SIZE);
        assert!(!rc.is_complete);

        // a short window means the end was found
        rc.update_from_load(2 * WINDOW_SIZE, 50);
        assert_eq!(rc.known, 2 * WINDOW_SIZE + 50);
        assert!(rc.is_complete);

        // stale loads never shrink the known count
        rc.update_from_load(0, WINDOW_SIZE);
        assert_eq!(rc.known, 2 * WINDOW_SIZE + 50);
    }

    #[test]
    fn test_row_count_count_batch_discovers_total() {
        let runtime = runtime_with_rows(500);
        let mut rc = RowCount::new(WINDOW_SIZE);
        let more = rc.count_batch(&runtime, "nums");
        // 500 rows fit in one batch: counting finished in a single call
        assert!(!more);
        assert!(rc.is_complete);
        assert_eq!(rc.known, 500);
    }

    #[test]
    fn test_row_count_count_batch_handles_missing_table() {
        let runtime = Runtime::new(None).unwrap();
        let mut rc = RowCount::new(WINDOW_SIZE);
        let more = rc.count_batch(&runtime, "no_such_table");
        assert!(!more);
        assert!(rc.is_complete);
    }
}
