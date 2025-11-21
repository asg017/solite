//! # adapted from Ratatui Table example
mod listing_page;
mod table_page;
mod tui_theme;

#[cfg(test)]
mod test_tui;
use crate::commands::tui::tui_theme::{CTP_MOCHA_THEME, TuiTheme};
use crate::commands::tui::{listing_page::ListingPage, table_page::TablePage};
use crate::themes;
use color_eyre::Result;
use crossterm::event::{self, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Stylize};
use ratatui::text::{Line, Text};
use ratatui::Frame;
use solite_core::sqlite::OwnedValue;
use solite_core::Runtime;

use crate::cli::TuiArgs;

enum NavigateToPage {
    Listing,
    Table(String),
}
enum HandleKeyResult {
    None,
    Quit,
    Navigate(NavigateToPage),
}

fn copy(value: &OwnedValue) {
    let content = match value {
        OwnedValue::Null => "".to_owned(),
        OwnedValue::Integer(i) => i.to_string(),
        OwnedValue::Double(f) => f.to_string(),
        OwnedValue::Text(s) => unsafe { std::str::from_utf8_unchecked(&s).to_owned() },
        OwnedValue::Blob(_) => "[BLOB]".to_owned(),
    };
    arboard::Clipboard::new()
        .unwrap()
        .set_text(content)
        .unwrap();
}

trait TuiPage {
    fn handle_key(&mut self, key: KeyEvent) -> HandleKeyResult;
    fn render(&mut self, frame: &mut Frame, area: Rect);
}

enum Page<'a> {
    Listing(ListingPage),
    Table(TablePage<'a>),
}
pub(crate) struct App<'a> {
    runtime: &'a Runtime,
    page: Page<'a>,
    theme: TuiTheme,
}
impl<'a> App<'a> {
    /// returns true if the application should quit.
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        let result = match &mut self.page {
            Page::Listing(page) => page.handle_key(key),
            Page::Table(page) => page.handle_key(key),
        };

        match result {
            HandleKeyResult::Quit => return true,
            HandleKeyResult::None => (),
            HandleKeyResult::Navigate(to) => match to {
                NavigateToPage::Listing => {
                    let mut page = ListingPage::new(self.runtime, &self.theme);
                    page.state.select_first();
                    self.page = Page::Listing(page);
                }
                NavigateToPage::Table(table_name) => {
                    self.page = Page::Table(TablePage::new(&table_name, self.runtime, self.theme.clone()));
                }
            },
        }

        false
    }

    fn render(&mut self, frame: &mut Frame) {
        frame.render_widget(
            ratatui::widgets::Block::new()
                .bg::<Color>(self.theme.base.clone().into()),
            frame.area()
        );
        
        let layout = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]);
        let [top, main] = frame.area().layout(&layout);

        let x = Layout::horizontal(vec![
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Fill(1),
        ]);
        let [l, m, r] = x.areas(top);
        frame.render_widget(
            Text::from(match &self.page {
                Page::Listing(_) => "Listing".to_owned(),
                Page::Table(table_page) => format!(
                    "{} {} rows",
                    table_page.table_name,
                    table_page.data.rows.len()
                ),
            })
            .left_aligned(),
            l,
        );
        frame.render_widget(Text::from("Solite TUI").bold().centered(), m);
        let l = Line::from(vec![
            "q".bold()
                .fg::<Color>(self.theme.keycap.clone().into()),
            " to quit".italic(),
        ])
        .right_aligned();
        frame.render_widget(l, r);

        match &mut self.page {
            Page::Listing(listing_page) => {
                listing_page.render(frame, main);
            }
            Page::Table(table_page) => {
                table_page.render(frame, main);
            }
        }
    }
}

pub fn launch_tui(runtime: &mut Runtime) -> anyhow::Result<()> {
    let theme = CTP_MOCHA_THEME.clone();
    let page = Page::Listing(ListingPage::new(&runtime, &theme));
    let mut app = App { runtime, page, theme: theme.clone() };

    ratatui::run(|terminal| loop {
        terminal.draw(|frame| app.render(frame))?;
        if let Some(key) = event::read()?.as_key_press_event() {
            if app.handle_key(key) {
                break Ok(());
            }
        }
    })
}

pub(crate) fn tui(cmd: TuiArgs) -> Result<(), ()> {
    color_eyre::install().unwrap();
    let mut runtime = Runtime::new(Some(cmd.database.to_str().unwrap().to_owned()));
    let theme = CTP_MOCHA_THEME.clone();
    let page = Page::Listing(ListingPage::new(&runtime, &theme));
    let mut app = App {
        runtime: &mut runtime,
        page,
        theme: theme.clone(),
    };
    if let Some(table_name) = cmd.table {
        app.page = Page::Table(TablePage::new(&table_name, &app.runtime, theme.clone()));
    } 

    let result: anyhow::Result<()> = ratatui::run(|terminal| loop {
        terminal.draw(|frame| app.render(frame))?;
        if let Some(key) = event::read()?.as_key_press_event() {
            if app.handle_key(key) {
                break Ok(());
            }
        }
    });
    result.map_err(|_| ())
}
