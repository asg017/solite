//! TUI (Terminal User Interface) module for the Solite database browser.
//!
//! Provides an interactive terminal interface for exploring SQLite databases,
//! viewing tables, and copying data.

mod copy_popup;
mod help_bar;
mod help_popup;
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
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Stylize};
use ratatui::text::{Line, Text};
use ratatui::Frame;
use solite_core::sqlite::OwnedValue;
use solite_core::Runtime;
use std::time::Duration;

/// Format a number with thousand separators
pub(crate) fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

use crate::cli::TuiArgs;

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

/// Destination for copy operations. Injectable so tests can assert what was
/// copied without touching the real (headless-hostile) system clipboard.
pub(crate) trait Clipboard {
    /// Copy text to the clipboard. Returns an error message if it fails.
    fn set_text(&mut self, text: String) -> std::result::Result<(), String>;
}

/// The system clipboard, via arboard.
struct SystemClipboard;

impl Clipboard for SystemClipboard {
    fn set_text(&mut self, text: String) -> std::result::Result<(), String> {
        arboard::Clipboard::new()
            .map_err(|e| format!("Failed to access clipboard: {}", e))?
            .set_text(text)
            .map_err(|e| format!("Failed to copy: {}", e))
    }
}

/// Shared clipboard handle passed to every page that can copy.
pub(crate) type SharedClipboard = std::rc::Rc<std::cell::RefCell<dyn Clipboard>>;

enum Page<'a> {
    Listing(ListingPage),
    Table(TablePage<'a>),
    Row(RowPage),
}

pub(crate) struct App<'a> {
    runtime: &'a Runtime,
    page: Page<'a>,
    theme: TuiTheme,
    clipboard: SharedClipboard,
}
impl<'a> App<'a> {
    /// Returns true if the application should quit.
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        let result = match &mut self.page {
            Page::Listing(page) => page.handle_key(key),
            Page::Table(page) => page.handle_key(key),
            Page::Row(page) => page.handle_key(key),
        };

        match result {
            HandleKeyResult::Quit => return true,
            HandleKeyResult::None => (),
            HandleKeyResult::Navigate(to) => match to {
                NavigateToPage::Listing => {
                    let page = ListingPage::new(self.runtime, &self.theme);
                    self.page = Page::Listing(page);
                }
                NavigateToPage::Table(table_name) => {
                    self.page = Page::Table(TablePage::new(
                        &table_name,
                        self.runtime,
                        self.theme.clone(),
                        self.clipboard.clone(),
                    ));
                }
                NavigateToPage::Row(data) => {
                    let row_page = RowPage::new(
                        data.table_name,
                        data.row_index,
                        data.columns,
                        data.values,
                        data.primary_keys,
                        self.theme.clone(),
                        self.clipboard.clone(),
                    );
                    self.page = Page::Row(row_page);
                }
                NavigateToPage::BackToTable => {
                    // Get table name from current Row page, then navigate back
                    if let Page::Row(row_page) = &self.page {
                        let table_name = row_page.table_name.clone();
                        self.page = Page::Table(TablePage::new(
                            &table_name,
                            self.runtime,
                            self.theme.clone(),
                            self.clipboard.clone(),
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
                let row_count = table_page.total_rows();
                let formatted_count = format_number(row_count);
                let suffix = if table_page.row_count.is_complete { "" } else { "+" };
                let row_text = if row_count == 1 { "row" } else { "rows" };
                format!("{} ({}{} {})", table_page.table_name, formatted_count, suffix, row_text)
            }
            Page::Row(row_page) => {
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

        // Right: quick help (q backs out of table/row pages; only Q hard-quits)
        let keycap: Color = self.theme.keycap.clone().into();
        let help = match &self.page {
            Page::Listing(_) => Line::from(vec![
                "?".bold().fg(keycap),
                " help  ".into(),
                "q".bold().fg(keycap),
                " quit".into(),
            ]),
            Page::Table(_) | Page::Row(_) => Line::from(vec![
                "?".bold().fg(keycap),
                " help  ".into(),
                "q".bold().fg(keycap),
                " back  ".into(),
                "Q".bold().fg(keycap),
                " quit".into(),
            ]),
        }
        .right_aligned();
        frame.render_widget(help, right);

        match &mut self.page {
            Page::Listing(listing_page) => listing_page.render(frame, main),
            Page::Table(table_page) => table_page.render(frame, main),
            Page::Row(row_page) => row_page.render(frame, main),
        }
    }
}

/// Shared app construction + event loop for both TUI entry points.
/// Opens directly on `initial_table` when given (skipping the listing query).
fn run_app(runtime: &Runtime, initial_table: Option<&str>) -> anyhow::Result<()> {
    let theme = CTP_MOCHA_THEME.clone();
    let clipboard: SharedClipboard = std::rc::Rc::new(std::cell::RefCell::new(SystemClipboard));
    let page = match initial_table {
        Some(table_name) => Page::Table(TablePage::new(
            table_name,
            runtime,
            theme.clone(),
            clipboard.clone(),
        )),
        None => Page::Listing(ListingPage::new(runtime, &theme)),
    };
    let mut app = App {
        runtime,
        page,
        theme,
        clipboard,
    };

    ratatui::run(|terminal| loop {
        terminal.draw(|frame| app.render(frame))?;
        // Poll with short timeout to allow UI refresh during counting
        if event::poll(Duration::from_millis(100))? {
            if let Some(key) = event::read()?.as_key_press_event() {
                if app.handle_key(key) {
                    break Ok(());
                }
            }
        }
    })
}

pub fn launch_tui(runtime: &mut Runtime) -> anyhow::Result<()> {
    run_app(runtime, None)
}

pub(crate) fn tui(cmd: TuiArgs) -> Result<(), ()> {
    // Failure means a hook is already installed; degraded panic reports are fine.
    let _ = color_eyre::install();
    let Some(database) = cmd.database.to_str() else {
        eprintln!(
            "Error: database path is not valid UTF-8: {}",
            cmd.database.display()
        );
        return Err(());
    };
    let runtime = Runtime::new_with_options(
        Some(database.to_owned()),
        cmd.remote.remote_bin.as_deref(),
        cmd.remote.transport.as_deref(),
        cmd.remote.allow_ssh,
    ).map_err(|e| {
        eprintln!("Error: {}", e);
    })?;
    run_app(&runtime, cmd.table.as_deref()).map_err(|err| eprintln!("Error: {err}"))
}
