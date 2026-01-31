use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListDirection, ListItem, ListState};
use ratatui::Frame;
use solite_core::Runtime;

use crate::commands::tui::help_bar::HelpBar;
use crate::commands::tui::tui_theme::TuiTheme;
use crate::commands::tui::{HandleKeyResult, NavigateToPage};

pub struct ListingPage {
    pub(crate) theme: TuiTheme,
    pub(crate) state: ListState,
    pub(crate) database_name: String,
    pub(crate) tables: Vec<String>,
    pub(crate) error: Option<String>,
}

impl ListingPage {
    pub(crate) fn new(runtime: &Runtime, theme: &TuiTheme) -> Self {
        let mut tables = vec![];
        let mut error = None;

        match runtime
            .connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        {
            Ok((_, Some(stmt))) => loop {
                match stmt.next() {
                    Ok(Some(row)) => {
                        tables.push(row[0].as_str().to_owned());
                    }
                    Ok(None) => break,
                    Err(e) => {
                        error = Some(format!("Error reading tables: {}", e));
                        break;
                    }
                }
            },
            Ok((_, None)) => {
                error = Some("Failed to prepare query".to_owned());
            }
            Err(e) => {
                error = Some(format!("Query error: {}", e));
            }
        }

        let mut state = ListState::default();
        if !tables.is_empty() {
            state.select_first();
        }

        Self {
            theme: theme.clone(),
            state,
            tables,
            database_name: runtime.connection.db_name().unwrap_or_default(),
            error,
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> HandleKeyResult {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => HandleKeyResult::Quit,
            KeyCode::Char('j') | KeyCode::Down => {
                self.state.select_next();
                HandleKeyResult::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.state.select_previous();
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
            KeyCode::Char('1'..='9') => {
                let idx = (key.code.as_char().unwrap() as usize) - ('1' as usize);
                if idx < self.tables.len() {
                    let table = self.tables[idx].clone();
                    HandleKeyResult::Navigate(NavigateToPage::Table(table))
                } else {
                    HandleKeyResult::None
                }
            }
            KeyCode::Enter => {
                if let Some(selected) = self.state.selected() {
                    if selected < self.tables.len() {
                        let table = self.tables[selected].clone();
                        HandleKeyResult::Navigate(NavigateToPage::Table(table))
                    } else {
                        HandleKeyResult::None
                    }
                } else {
                    HandleKeyResult::None
                }
            }
            _ => HandleKeyResult::None,
        }
    }

    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect) {
        let layout = Layout::vertical([Constraint::Fill(1), Constraint::Length(2)]);
        let [main_area, help_area] = area.layout(&layout);

        // Center the list horizontally
        let h_layout = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Min(32),
            Constraint::Fill(1),
        ]);
        let [_, list_area, _] = main_area.layout(&h_layout);

        // Build title with table count
        let title = if let Some(ref error) = self.error {
            format!("Tables (error: {})", error)
        } else {
            let count = self.tables.len();
            let table_word = if count == 1 { "table" } else { "tables" };
            format!("{} {}", count, table_word)
        };

        let items: Vec<ListItem> = self
            .tables
            .iter()
            .enumerate()
            .map(|(idx, table)| {
                let number = if idx < 9 {
                    Span::from(format!(" {} ", idx + 1))
                        .bold()
                        .fg(Color::DarkGray)
                } else {
                    Span::from("   ")
                };
                ListItem::new(Line::from_iter([number, Span::from(table.clone())]))
            })
            .collect();

        let base_color: Color = self.theme.base.clone().into();
        let hl_bg: Color = self.theme.row_hl_bg.clone().into();
        let hl_fg: Color = self.theme.keycap.clone().into();

        let list = List::new(items)
            .block(
                ratatui::widgets::Block::default()
                    .title(title)
                    .borders(ratatui::widgets::Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .bg(base_color)
            .highlight_style(Style::new().bg(hl_bg).fg(hl_fg).bold())
            .highlight_symbol("› ")
            .direction(ListDirection::TopToBottom);

        frame.render_stateful_widget(list, list_area, &mut self.state);

        // Render help bar
        HelpBar::new()
            .keys(vec!["j", "k"], " navigate")
            .item("Enter", " open")
            .item("1-9", " jump")
            .separator()
            .item("q", " quit")
            .render(frame, help_area);
    }
}