use std::fmt::Write;

use crate::commands::tui::tui_theme::TuiTheme;
use crate::commands::tui::{copy, Frame, HandleKeyResult, NavigateToPage, TuiPage};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, HorizontalAlignment, Layout, Rect};
use ratatui::style::{Style};
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

struct Order {
    column_idx: usize,
    direction: SortDirection,
}
fn load_table_data(runtime: &Runtime, table: &str, order: Option<Order>) -> Data {
    let mut sql: String = String::new();
    let _ = writeln!(&mut sql, "select * from {}", table);
    if let Some(order) = order {
        let _ = writeln!(
            &mut sql,
            "order by {} {}",
            order.column_idx + 1,
            match order.direction {
                SortDirection::Ascending => "asc",
                SortDirection::Descending => "desc",
            }
        );
    }
    let stmt: solite_core::sqlite::Statement = runtime.connection.prepare(&sql).unwrap().1.unwrap();
    let columns = stmt.column_names().unwrap();
    let mut max_row_widths = vec![100; columns.len()];
    // unicode width
    let column_widths = columns.iter().map(|c| ansi_width::ansi_width(c)).collect();
    let mut rows = vec![];
    loop {
        match stmt.next() {
            Ok(None) => break,
            Ok(Some(row)) => {
                let mut row_values = vec![];
                row_values.reserve(columns.len());
                for value in row {
                    row_values.push(OwnedValue::from_value_ref(&value));
                }
                rows.push(row_values);
            }
            Err(error) => todo!("{error}"),
        }
    }
    Data {
        columns,
        column_widths,
        rows,
        max_row_widths,
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
}

impl<'a> TablePage<'a> {
    pub(crate) fn new(table_name: &str, runtime: &'a Runtime, theme: TuiTheme) -> Self {
        let data = load_table_data(runtime, &table_name, None);
        let mut state = TableState::default();
        state.select_first();
        state.select_first_column();
        Self {
            runtime,
            theme,
            state,
            table_name: table_name.to_owned(),
            data,
            n_columns_show: 5,
            column_idx_offset: 0,
            footer_message: None,
        }
    }

    fn sort(&mut self, direction: SortDirection) {
        self.data = load_table_data(
            self.runtime,
            &self.table_name,
            Some(Order {
                column_idx: self.state.selected_column().unwrap().saturating_add(self.column_idx_offset),
                direction: direction,
            }),
        );
    }
}

enum SortDirection {
    Ascending,
    Descending,
}

impl TuiPage for TablePage<'_> {
    fn handle_key(&mut self, key: KeyEvent) -> HandleKeyResult {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                return HandleKeyResult::Navigate(NavigateToPage::Listing)
            }
            KeyCode::Char('Q') => return HandleKeyResult::Quit,
            KeyCode::Char('[') => self.sort(SortDirection::Ascending),
            KeyCode::Char(']') => self.sort(SortDirection::Descending),
            KeyCode::Char('j') | KeyCode::Down => self.state.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.state.select_previous(),
            KeyCode::Char('l') | KeyCode::Right => {
                if let Some(idx) = self.state.selected_column() {
                    if idx >= (self.n_columns_show - 1) {
                        self.column_idx_offset += 1;
                    } else {
                        self.state.select_next_column()
                    }
                } else {
                    self.state.select_next_column()
                }
            }
            KeyCode::Char('h') | KeyCode::Left => {
                if let Some(idx) = self.state.selected_column() {
                    if idx == 0 && self.column_idx_offset > 0 {
                        self.column_idx_offset -= 1;
                    } else {
                        self.state.select_previous_column()
                    }
                } else {
                    self.state.select_previous_column()
                }
            }
            KeyCode::Char('g') => self.state.select_first(),
            KeyCode::Char('G') => self.state.select_last(),
            KeyCode::Char('L') => {
                self.state.select_last_column();
                if self.data.columns.len() > self.n_columns_show {
                    self.column_idx_offset = self.data.columns.len().saturating_sub(self.n_columns_show);
                }
            }
            KeyCode::Char('H') => {
                self.state.select_first_column();
                self.column_idx_offset = 0;
            }
            // copy current cell to clipboard
            KeyCode::Char('y') => {
                if let Some(cell) = self.state.selected_cell() {
                    copy(&self.data.rows[cell.0][cell.1.saturating_add(self.column_idx_offset)]);
                    self.footer_message = Some(format!("✓ copied cell to clipboard"));
                }
            }
            // copy current row to clipboard
            KeyCode::Char('r') => {
                if let Some((_, _)) = self.state.selected_cell() {
                    //copy(&self.data.rows[y].iter()join("\t"));
                }
            }
            // copy entire table to clipboard
            KeyCode::Char('a') => {
                let contents = self
                    .data
                    .rows
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|value| match value {
                                OwnedValue::Null => "".to_owned(),
                                OwnedValue::Integer(i) => i.to_string(),
                                OwnedValue::Double(f) => f.to_string(),
                                OwnedValue::Text(s) => unsafe {
                                    std::str::from_utf8_unchecked(&s).to_owned()
                                },
                                OwnedValue::Blob(_) => "[BLOB]".to_owned(),
                            })
                            .collect::<Vec<String>>()
                            .join("\t")
                    })
                    .collect::<Vec<String>>()
                    .join("\n");
                arboard::Clipboard::new()
                    .unwrap()
                    .set_text(contents)
                    .unwrap();
                self.footer_message = Some(format!(
                    "Copied {} rows from {} to clipboard",
                    self.data.rows.len(),
                    self.table_name
                ));
            }
            // copy sql to clipboard
            KeyCode::Char('s') => {
                //copy(&format!("select * from {}", table_page.table_name));
                todo!();
            }
            _ => {}
        }
        HandleKeyResult::None
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let layout = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]);
        let [table_rect, footer_rect] = area.layout(&layout);

        let selected_header_idx = self.state.selected_column().unwrap_or(0).saturating_add(self.column_idx_offset);
        let header = Row::new(
            self.data
                .columns
                .iter()
                .skip(self.column_idx_offset)
                .enumerate()
                .map(|(idx , c)| {
                    Cell::from(Text::from(c.as_str())).style(
                        Style::new()
                            .bold()
                            .fg(self.theme.header_fg.clone().into())
                            .bg(if selected_header_idx == idx.saturating_add(self.column_idx_offset) {
                                self.theme.header_selected_bg.clone().into()
                            } else {
                                self.theme.header_bg.clone().into()
                            }),
                    )
                }),
        )
        .style(
            Style::new()
                .bold()
                .fg(self.theme.header_style_fg.clone().into()),
        );

        let rows = self.data.rows.iter().map(|r| {
            Row::new(r.iter().skip(self.column_idx_offset).map(|value| {
                Cell::default()
                    .content(match value {
                        OwnedValue::Null => Text::from("NULL".to_owned()),
                        OwnedValue::Integer(i) => {
                            Text::from(i.to_string()).alignment(HorizontalAlignment::Right)
                        }
                        OwnedValue::Double(f) => {
                            Text::from(f.to_string()).alignment(HorizontalAlignment::Right)
                        }
                        OwnedValue::Text(s) => unsafe {
                            Text::from(std::str::from_utf8_unchecked(&s).to_owned())
                        },
                        OwnedValue::Blob(_) => Text::from("[BLOB]".to_owned()),
                    })
                    .style(match value {
                        OwnedValue::Null => {
                            Style::new().fg(
                              self.theme.null.clone().into(),
                            )
                        }
                        OwnedValue::Integer(_) => Style::new().fg(
                            self.theme.integer.clone().into()
                        ),
                        OwnedValue::Double(_) => Style::new().fg(
                            self.theme.double.clone().into()
                        ),
                        OwnedValue::Text(_) => Style::new().fg(
                            self.theme.text.clone().into(),
                        ),
                        OwnedValue::Blob(_) => Style::new().fg(
                            self.theme.blob.clone().into(),
                        ),
                    })
                    .to_owned()
            }))
        });
        let widths = self
            .data
            .columns
            .iter()
            .take(self.n_columns_show)
            .map(|_| Constraint::Fill(1))
            .collect::<Vec<Constraint>>();
        let table = Table::new(rows, widths)
            .header(header)
            .column_spacing(1)
            .style(
                Style::new()
                    .fg(
                        self.theme.table_fg.clone().into(),
                    ),
            )
            .row_highlight_style(
                Style::new()
                    .bold()
                    .bg(
                        self.theme.row_hl_bg.clone().into(),
                    ),
            )
            .cell_highlight_style(
                Style::new()
                    .bold()
                    .fg(
                        self.theme.cell_hl_fg.clone().into(),
                    )
                    .bg(
                        self.theme.cell_hl_bg.clone().into()
                    ),
            );
        if let Some(msg) = &self.footer_message {
            frame.render_widget(Text::from(msg.as_str()), footer_rect);
        }
        frame.render_stateful_widget(table, table_rect, &mut self.state);
    }
}
