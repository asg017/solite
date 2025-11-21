use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{ List, ListDirection, ListItem, ListState};
use ratatui::Frame;
use solite_core::Runtime;
use crate::commands::tui::tui_theme::TuiTheme;
use crate::commands::tui::{HandleKeyResult, NavigateToPage};


pub struct ListingPage {
  pub(crate) theme: TuiTheme,
    pub(crate) state: ListState,
    pub(crate) database_name: String,
    pub(crate) tables: Vec<String>,
}

impl ListingPage {
    pub(crate)  fn new(runtime: &Runtime, theme: &TuiTheme) -> Self {
        let stmt = runtime
            .connection
            .prepare("select name, sql from sqlite_master where type='table'")
            .unwrap()
            .1
            .unwrap();
        let mut tables = vec![];
        loop {
            match stmt.next() {
                Ok(Some(row)) => {
                    tables.push(row[0].as_str().to_owned());
                }
                Ok(None) => break,
                Err(_) => todo!(),
            }
        }
        let mut state = ListState::default();
        state.select_first();
        Self {
            theme: theme.clone(),
            state,
            tables,
            database_name: runtime.connection.db_name().unwrap(),
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> HandleKeyResult {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return HandleKeyResult::Quit,
            KeyCode::Char('j') | KeyCode::Down => self.state.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.state.select_previous(),
            KeyCode::Char('g') => self.state.select_first(),
            KeyCode::Char('G') => self.state.select_last(),
            KeyCode::Char('1'..'9') => {
                let idx = (key.code.as_char().unwrap() as u16 - ('1' as u16)) as usize;
                let table = self.tables[idx].clone();
                return HandleKeyResult::Navigate(NavigateToPage::Table(table));
            }
            KeyCode::Enter => {
                if let Some(selected) = self.state.selected() {
                    let table = self.tables[selected].clone();
                    return HandleKeyResult::Navigate(NavigateToPage::Table(table));
                } else {
                    panic!()
                }
            }
            _ => {}
        }
        HandleKeyResult::None
    }

    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect) {
        let layout = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Min(24),
            Constraint::Fill(1),
        ]);
        let [_, area, _] = area.layout(&layout);
        let list = List::new(
            self.tables
                .iter()
                .enumerate()
                .map(|(idx, table)| {
                    ListItem::new(Line::from_iter([
                        if idx < 9 {
                            Span::from(format!(" {} ", idx + 1)).bold().fg(Color::Gray)
                        } else {
                            Span::from("")
                        },
                        Span::from(table.clone()),
                    ]))
                })
                .collect::<Vec<ListItem>>(),
        )
        .block(
            ratatui::widgets::Block::default()
                .title(format!("Tables in {}", self.database_name))
                .borders(ratatui::widgets::Borders::ALL),
        )
        .bg::<Color>(self.theme.base.clone().into())
        .highlight_style(Style::new().bg(Color::Gray).fg(Color::Black))
        .highlight_symbol("❱ ")
        .direction(ListDirection::TopToBottom);
        frame.render_stateful_widget(list, area, &mut self.state);
    }
}