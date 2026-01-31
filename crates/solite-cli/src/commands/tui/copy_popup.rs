//! Copy/yank popup for selecting what to copy to clipboard.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};

use super::help_bar::HelpBar;
use super::utils::popup_area_fixed;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyOption {
    Cell,
    Row,
    Table,
    SqlSelect,
    SqlInsert,
}

impl CopyOption {
    pub fn label(&self) -> &'static str {
        match self {
            CopyOption::Cell => "Copy cell",
            CopyOption::Row => "Copy row (TSV)",
            CopyOption::Table => "Copy table (TSV)",
            CopyOption::SqlSelect => "Copy as SELECT",
            CopyOption::SqlInsert => "Copy as INSERT",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            CopyOption::Cell => "Copy the selected cell value",
            CopyOption::Row => "Copy the current row as tab-separated values",
            CopyOption::Table => "Copy all rows as tab-separated values",
            CopyOption::SqlSelect => "Copy a SELECT statement for this table",
            CopyOption::SqlInsert => "Copy INSERT statements for the data",
        }
    }
}

pub struct CopyPopup {
    pub visible: bool,
    pub state: ListState,
    options: Vec<CopyOption>,
}

impl CopyPopup {
    pub fn new() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            visible: false,
            state,
            options: vec![
                CopyOption::Cell,
                CopyOption::Row,
                CopyOption::Table,
                CopyOption::SqlSelect,
                CopyOption::SqlInsert,
            ],
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.state.select(Some(0));
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn selected_option(&self) -> Option<CopyOption> {
        self.state.selected().map(|i| self.options[i])
    }

    /// Handle key event. Returns Some(CopyOption) if an option was selected.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<CopyOption> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.hide();
                None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let i = self.state.selected().unwrap_or(0);
                let next = if i >= self.options.len() - 1 { 0 } else { i + 1 };
                self.state.select(Some(next));
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let i = self.state.selected().unwrap_or(0);
                let prev = if i == 0 { self.options.len() - 1 } else { i - 1 };
                self.state.select(Some(prev));
                None
            }
            KeyCode::Enter => {
                let option = self.selected_option();
                self.hide();
                option
            }
            KeyCode::Char('1'..='5') => {
                let idx = (key.code.as_char().unwrap() as usize) - ('1' as usize);
                if idx < self.options.len() {
                    let option = self.options[idx];
                    self.hide();
                    Some(option)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        let popup_area = popup_area_fixed(area, 40, 10);

        // Clear the background
        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .title(" Copy to Clipboard ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .style(Style::default().bg(Color::Black));

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        // Layout: list + help
        let layout = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]);
        let [list_area, help_area] = layout.areas(inner);

        let items: Vec<ListItem> = self
            .options
            .iter()
            .enumerate()
            .map(|(idx, opt)| {
                let number = Span::styled(
                    format!(" {} ", idx + 1),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                );
                let label = Span::raw(opt.label());
                ListItem::new(Line::from(vec![number, label]))
            })
            .collect();

        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› ");

        frame.render_stateful_widget(list, list_area, &mut self.state);

        HelpBar::new()
            .keys(vec!["j", "k"], " nav")
            .item("Enter", " copy")
            .item("Esc", " cancel")
            .render_inline(frame, help_area);
    }
}

impl Default for CopyPopup {
    fn default() -> Self {
        Self::new()
    }
}
