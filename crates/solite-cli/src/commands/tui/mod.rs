//! TUI (Terminal User Interface) module for the Solite database browser.
//!
//! Provides an interactive terminal interface for exploring SQLite databases,
//! viewing tables, and copying data.

mod copy_popup;
mod help_bar;
mod listing_page;
mod row_page;
mod table_page;
mod tui_theme;
mod utils;

#[cfg(test)]
mod test_tui;

use crate::commands::tui::tui_theme::{CTP_MOCHA_THEME, TuiTheme};
use crate::commands::tui::{listing_page::ListingPage, row_page::RowPage, table_page::TablePage};
use color_eyre::Result;
use crossterm::event::{self, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Stylize};
use ratatui::text::{Line, Text};
use ratatui::Frame;
use solite_core::sqlite::OwnedValue;
use solite_core::Runtime;

use crate::cli::TuiArgs;

pub use copy_popup::{CopyOption, CopyPopup};
pub use help_bar::HelpBar;
pub use utils::{popup_area, popup_area_fixed, truncate_string};

enum NavigateToPage {
    Listing,
    Table(String),
    Row(RowPageData),
    BackToTable,
}

/// Data needed to create a RowPage
struct RowPageData {
    table_name: String,
    row_index: usize,
    columns: Vec<String>,
    values: Vec<OwnedValue>,
    primary_keys: Vec<row_page::PrimaryKeyInfo>,
}

enum HandleKeyResult {
    None,
    Quit,
    Navigate(NavigateToPage),
    ShowMessage(String),
}

/// Convert an OwnedValue to a string for clipboard operations
fn value_to_string(value: &OwnedValue) -> String {
    match value {
        OwnedValue::Null => String::new(),
        OwnedValue::Integer(i) => i.to_string(),
        OwnedValue::Double(f) => f.to_string(),
        OwnedValue::Text(s) => String::from_utf8_lossy(s).into_owned(),
        OwnedValue::Blob(_) => "[BLOB]".to_owned(),
    }
}

/// Copy text to the system clipboard. Returns an error message if it fails.
fn copy_to_clipboard(content: &str) -> std::result::Result<(), String> {
    arboard::Clipboard::new()
        .map_err(|e| format!("Failed to access clipboard: {}", e))?
        .set_text(content.to_owned())
        .map_err(|e| format!("Failed to copy: {}", e))
}

/// Copy an OwnedValue to the clipboard
fn copy(value: &OwnedValue) -> std::result::Result<(), String> {
    copy_to_clipboard(&value_to_string(value))
}

trait TuiPage {
    fn handle_key(&mut self, key: KeyEvent) -> HandleKeyResult;
    fn render(&mut self, frame: &mut Frame, area: Rect);
}

enum Page<'a> {
    Listing(ListingPage),
    Table(TablePage<'a>),
    Row(RowPage, String), // RowPage + table_name for back navigation
}

pub(crate) struct App<'a> {
    runtime: &'a Runtime,
    page: Page<'a>,
    theme: TuiTheme,
}
impl<'a> App<'a> {
    /// Returns true if the application should quit.
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        let result = match &mut self.page {
            Page::Listing(page) => page.handle_key(key),
            Page::Table(page) => page.handle_key(key),
            Page::Row(page, _) => page.handle_key(key),
        };

        match result {
            HandleKeyResult::Quit => return true,
            HandleKeyResult::None => (),
            HandleKeyResult::ShowMessage(msg) => {
                // Messages are handled by individual pages via their footer_message
                // This variant exists for consistency but the page already set the message
                let _ = msg;
            }
            HandleKeyResult::Navigate(to) => match to {
                NavigateToPage::Listing => {
                    let page = ListingPage::new(self.runtime, &self.theme);
                    self.page = Page::Listing(page);
                }
                NavigateToPage::Table(table_name) => {
                    self.page =
                        Page::Table(TablePage::new(&table_name, self.runtime, self.theme.clone()));
                }
                NavigateToPage::Row(data) => {
                    let row_page = RowPage::new(
                        data.table_name.clone(),
                        data.row_index,
                        data.columns,
                        data.values,
                        data.primary_keys,
                        self.theme.clone(),
                    );
                    self.page = Page::Row(row_page, data.table_name);
                }
                NavigateToPage::BackToTable => {
                    // Get table name from current Row page, then navigate back
                    if let Page::Row(_, table_name) = &self.page {
                        let table_name = table_name.clone();
                        self.page = Page::Table(TablePage::new(
                            &table_name,
                            self.runtime,
                            self.theme.clone(),
                        ));
                    }
                }
            },
        }

        false
    }

    fn render(&mut self, frame: &mut Frame) {
        frame.render_widget(
            ratatui::widgets::Block::new().bg::<Color>(self.theme.base.clone().into()),
            frame.area(),
        );

        let layout = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]);
        let [top, main] = frame.area().layout(&layout);

        let top_layout = Layout::horizontal(vec![
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Fill(1),
        ]);
        let [left, center, right] = top_layout.areas(top);

        // Left: context info
        let context_text = match &self.page {
            Page::Listing(listing) => listing.database_name.clone(),
            Page::Table(table_page) => {
                let row_count = table_page.total_rows;
                let row_text = if row_count == 1 { "row" } else { "rows" };
                format!("{} ({} {})", table_page.table_name, row_count, row_text)
            }
            Page::Row(row_page, _) => {
                format!(
                    "{} > row {}",
                    row_page.table_name,
                    row_page.row_index + 1
                )
            }
        };
        frame.render_widget(Text::from(context_text).left_aligned(), left);

        // Center: title
        frame.render_widget(
            Text::from("Solite")
                .bold()
                .fg::<Color>(self.theme.keycap.clone().into())
                .centered(),
            center,
        );

        // Right: quick help
        let help = Line::from(vec![
            "?".bold().fg::<Color>(self.theme.keycap.clone().into()),
            " help  ".into(),
            "q".bold().fg::<Color>(self.theme.keycap.clone().into()),
            " quit".into(),
        ])
        .right_aligned();
        frame.render_widget(help, right);

        match &mut self.page {
            Page::Listing(listing_page) => listing_page.render(frame, main),
            Page::Table(table_page) => table_page.render(frame, main),
            Page::Row(row_page, _) => row_page.render(frame, main),
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
