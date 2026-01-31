use std::fmt::Write;

use crate::commands::tui::copy_popup::{CopyOption, CopyPopup};
use crate::commands::tui::help_bar::HelpBar;
use crate::commands::tui::tui_theme::TuiTheme;
use crate::commands::tui::{
    copy_to_clipboard, value_to_string, Frame, HandleKeyResult, NavigateToPage, TuiPage,
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

struct Order {
    column_idx: usize,
    direction: SortDirection,
}
/// Result of loading table data
struct LoadResult {
    data: Data,
    error: Option<String>,
}

fn load_table_data(runtime: &Runtime, table: &str, order: Option<Order>) -> LoadResult {
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
}

impl<'a> TablePage<'a> {
    pub(crate) fn new(table_name: &str, runtime: &'a Runtime, theme: TuiTheme) -> Self {
        let result = load_table_data(runtime, table_name, None);
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
        }
    }

    fn sort(&mut self, direction: SortDirection) {
        let col_idx = self
            .state
            .selected_column()
            .unwrap_or(0)
            .saturating_add(self.column_idx_offset);
        let result = load_table_data(
            self.runtime,
            &self.table_name,
            Some(Order {
                column_idx: col_idx,
                direction,
            }),
        );
        self.data = result.data;
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
                self.state.select_next();
                HandleKeyResult::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.state.select_previous();
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
                self.state.select_first();
                HandleKeyResult::None
            }
            KeyCode::Char('G') => {
                self.state.select_last();
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
                Cell::default()
                    .content(match value {
                        OwnedValue::Null => Text::from("NULL"),
                        OwnedValue::Integer(i) => {
                            Text::from(i.to_string()).alignment(HorizontalAlignment::Right)
                        }
                        OwnedValue::Double(f) => {
                            Text::from(f.to_string()).alignment(HorizontalAlignment::Right)
                        }
                        OwnedValue::Text(s) => {
                            Text::from(String::from_utf8_lossy(s).into_owned())
                        }
                        OwnedValue::Blob(_) => Text::from("[BLOB]"),
                    })
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

        // Footer message (copy confirmation, errors, etc.)
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
        }

        // Help bar
        HelpBar::new()
            .keys(vec!["h", "j", "k", "l"], " navigate")
            .item("[", " sort asc")
            .item("]", " sort desc")
            .separator()
            .keys(vec!["y", "c"], " copy")
            .item("q", " back")
            .render(frame, help_rect);

        // Copy popup (renders on top)
        self.copy_popup.render(frame, area);
    }
}
