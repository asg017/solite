//! Row detail page - shows a single row with columns listed vertically.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use solite_core::sqlite::OwnedValue;
use solite_core::Runtime;

use crate::commands::tui::help_bar::HelpBar;
use crate::commands::tui::tui_theme::TuiTheme;
use crate::commands::tui::{copy_to_clipboard, value_to_string, HandleKeyResult, NavigateToPage};

/// Information about a primary key column
#[derive(Clone)]
pub struct PrimaryKeyInfo {
    pub column_name: String,
    pub column_index: usize,
}

/// Get primary key columns for a table
pub fn get_primary_keys(runtime: &Runtime, table_name: &str) -> Vec<PrimaryKeyInfo> {
    use solite_core::sqlite::ValueRefXValue;

    let sql = format!(
        "PRAGMA table_info(\"{}\")",
        table_name.replace('"', "\"\"")
    );

    let mut pks = Vec::new();

    if let Ok((_, Some(stmt))) = runtime.connection.prepare(&sql) {
        loop {
            match stmt.next() {
                Ok(Some(row)) => {
                    // PRAGMA table_info returns: cid, name, type, notnull, dflt_value, pk
                    // pk is 1-based index for primary key columns (0 if not pk)
                    let pk_index = match &row[5].value {
                        ValueRefXValue::Int(i) => *i,
                        _ => 0,
                    };
                    if pk_index > 0 {
                        let col_name = row[1].as_str().to_owned();
                        let col_idx = match &row[0].value {
                            ValueRefXValue::Int(i) => *i as usize,
                            _ => 0,
                        };
                        pks.push(PrimaryKeyInfo {
                            column_name: col_name,
                            column_index: col_idx,
                        });
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    }

    // Sort by pk index (the pk field is 1-based order)
    pks.sort_by_key(|p| p.column_index);
    pks
}

pub struct RowPage {
    pub theme: TuiTheme,
    pub table_name: String,
    pub row_index: usize,
    pub columns: Vec<String>,
    pub values: Vec<OwnedValue>,
    pub primary_keys: Vec<PrimaryKeyInfo>,
    pub state: ListState,
    pub footer_message: Option<String>,
}

impl RowPage {
    pub fn new(
        table_name: String,
        row_index: usize,
        columns: Vec<String>,
        values: Vec<OwnedValue>,
        primary_keys: Vec<PrimaryKeyInfo>,
        theme: TuiTheme,
    ) -> Self {
        let mut state = ListState::default();
        if !columns.is_empty() {
            state.select(Some(0));
        }
        Self {
            theme,
            table_name,
            row_index,
            columns,
            values,
            primary_keys,
            state,
            footer_message: None,
        }
    }

    /// Format the primary key display string
    fn primary_key_display(&self) -> String {
        if self.primary_keys.is_empty() {
            format!("Row {}", self.row_index + 1)
        } else {
            let pk_parts: Vec<String> = self
                .primary_keys
                .iter()
                .filter_map(|pk| {
                    if pk.column_index < self.values.len() {
                        let val = value_to_string(&self.values[pk.column_index]);
                        Some(format!("{}={}", pk.column_name, val))
                    } else {
                        None
                    }
                })
                .collect();
            if pk_parts.is_empty() {
                format!("Row {}", self.row_index + 1)
            } else {
                pk_parts.join(", ")
            }
        }
    }

    /// Copy the currently selected value
    fn copy_selected(&mut self) {
        if let Some(idx) = self.state.selected() {
            if idx < self.values.len() {
                let content = value_to_string(&self.values[idx]);
                match copy_to_clipboard(&content) {
                    Ok(()) => {
                        self.footer_message = Some(format!(
                            "Copied {} to clipboard",
                            self.columns[idx]
                        ));
                    }
                    Err(e) => {
                        self.footer_message = Some(e);
                    }
                }
            }
        }
    }

    /// Copy the primary key value(s) to clipboard
    fn copy_primary_key(&mut self) {
        if self.primary_keys.is_empty() {
            self.footer_message = Some("No primary key defined".to_owned());
            return;
        }

        let pk_values: Vec<String> = self
            .primary_keys
            .iter()
            .filter_map(|pk| {
                if pk.column_index < self.values.len() {
                    Some(value_to_string(&self.values[pk.column_index]))
                } else {
                    None
                }
            })
            .collect();

        if pk_values.is_empty() {
            self.footer_message = Some("No primary key values".to_owned());
            return;
        }

        let content = if pk_values.len() == 1 {
            pk_values[0].clone()
        } else {
            pk_values.join(", ")
        };

        match copy_to_clipboard(&content) {
            Ok(()) => {
                self.footer_message = Some(format!("Copied primary key: {}", content));
            }
            Err(e) => {
                self.footer_message = Some(e);
            }
        }
    }

    /// Copy the entire row as JSON
    fn copy_as_json(&mut self) {
        let mut json = String::from("{");
        for (i, (col, val)) in self.columns.iter().zip(self.values.iter()).enumerate() {
            if i > 0 {
                json.push_str(", ");
            }
            let val_str = match val {
                OwnedValue::Null => "null".to_owned(),
                OwnedValue::Integer(i) => i.to_string(),
                OwnedValue::Double(f) => f.to_string(),
                OwnedValue::Text(s) => {
                    let text = String::from_utf8_lossy(s);
                    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
                }
                OwnedValue::Blob(b) => format!("\"<blob {} bytes>\"", b.len()),
            };
            json.push_str(&format!("\"{}\": {}", col, val_str));
        }
        json.push('}');

        match copy_to_clipboard(&json) {
            Ok(()) => {
                self.footer_message = Some("Copied row as JSON".to_owned());
            }
            Err(e) => {
                self.footer_message = Some(e);
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> HandleKeyResult {
        self.footer_message = None;

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                HandleKeyResult::Navigate(NavigateToPage::BackToTable)
            }
            KeyCode::Char('Q') => HandleKeyResult::Quit,
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(idx) = self.state.selected() {
                    let next = if idx >= self.columns.len() - 1 {
                        0
                    } else {
                        idx + 1
                    };
                    self.state.select(Some(next));
                }
                HandleKeyResult::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(idx) = self.state.selected() {
                    let prev = if idx == 0 {
                        self.columns.len().saturating_sub(1)
                    } else {
                        idx - 1
                    };
                    self.state.select(Some(prev));
                }
                HandleKeyResult::None
            }
            KeyCode::Char('g') => {
                self.state.select(Some(0));
                HandleKeyResult::None
            }
            KeyCode::Char('G') => {
                self.state.select(Some(self.columns.len().saturating_sub(1)));
                HandleKeyResult::None
            }
            KeyCode::Char('y') | KeyCode::Enter => {
                self.copy_selected();
                HandleKeyResult::None
            }
            KeyCode::Char('Y') | KeyCode::Char('J') => {
                self.copy_as_json();
                HandleKeyResult::None
            }
            KeyCode::Char('p') => {
                self.copy_primary_key();
                HandleKeyResult::None
            }
            _ => HandleKeyResult::None,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let layout = Layout::vertical([
            Constraint::Length(3), // Header with PK info
            Constraint::Fill(1),   // Column/value list
            Constraint::Length(1), // Message
            Constraint::Length(2), // Help bar
        ]);
        let [header_rect, list_rect, message_rect, help_rect] = area.layout(&layout);

        // Header showing primary key
        let pk_display = self.primary_key_display();
        let header_fg: Color = self.theme.header_fg.clone().into();
        let keycap_color: Color = self.theme.keycap.clone().into();

        let header_text = Line::from(vec![
            Span::styled(
                format!("{} ", self.table_name),
                Style::default().fg(header_fg).add_modifier(Modifier::BOLD),
            ),
            Span::styled(pk_display, Style::default().fg(keycap_color)),
        ]);

        let header = Paragraph::new(header_text)
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .centered();
        frame.render_widget(header, header_rect);

        // Pre-compute colors
        let pk_col_color: Color = self.theme.keycap.clone().into();
        let normal_col_color: Color = self.theme.header_fg.clone().into();
        let null_color: Color = self.theme.null.clone().into();
        let int_color: Color = self.theme.integer.clone().into();
        let double_color: Color = self.theme.double.clone().into();
        let text_color: Color = self.theme.text.clone().into();
        let blob_color: Color = self.theme.blob.clone().into();

        // Build list items
        let items: Vec<ListItem> = self
            .columns
            .iter()
            .zip(self.values.iter())
            .enumerate()
            .map(|(idx, (col, val))| {
                let is_pk = self
                    .primary_keys
                    .iter()
                    .any(|pk| pk.column_index == idx);

                let col_style = if is_pk {
                    Style::default()
                        .fg(pk_col_color)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(normal_col_color)
                        .add_modifier(Modifier::BOLD)
                };

                let val_style = match val {
                    OwnedValue::Null => Style::default().fg(null_color),
                    OwnedValue::Integer(_) => Style::default().fg(int_color),
                    OwnedValue::Double(_) => Style::default().fg(double_color),
                    OwnedValue::Text(_) => Style::default().fg(text_color),
                    OwnedValue::Blob(_) => Style::default().fg(blob_color),
                };

                let val_display = match val {
                    OwnedValue::Null => "NULL".to_owned(),
                    OwnedValue::Integer(i) => i.to_string(),
                    OwnedValue::Double(f) => f.to_string(),
                    OwnedValue::Text(s) => String::from_utf8_lossy(s).into_owned(),
                    OwnedValue::Blob(b) => format!("<blob {} bytes>", b.len()),
                };

                let pk_marker = if is_pk { " PK" } else { "" };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("{:<20}", col), col_style),
                    Span::styled(pk_marker, Style::default().fg(Color::Yellow)),
                    Span::raw("  "),
                    Span::styled(val_display, val_style),
                ]))
            })
            .collect();

        let hl_bg: Color = self.theme.row_hl_bg.clone().into();
        let list = List::new(items)
            .block(Block::default())
            .highlight_style(Style::default().bg(hl_bg).add_modifier(Modifier::BOLD))
            .highlight_symbol("› ");

        frame.render_stateful_widget(list, list_rect, &mut self.state);

        // Footer message
        if let Some(msg) = &self.footer_message {
            let style = if msg.starts_with("Copied") {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            };
            frame.render_widget(
                Paragraph::new(msg.as_str()).style(style).centered(),
                message_rect,
            );
        }

        // Help bar
        HelpBar::new()
            .keys(vec!["j", "k"], " navigate")
            .keys(vec!["y", "Enter"], " copy value")
            .item("p", " copy PK")
            .item("J", " copy JSON")
            .separator()
            .item("q", " back")
            .render(frame, help_rect);
    }
}
