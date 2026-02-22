//! Table rendering implementations.

mod terminal;
mod string;
mod html;

pub use terminal::render_terminal;
pub use string::{render_string, render_string_plain};
pub use html::render_html;

use crate::types::CellValue;

/// A rendered row, either data or ellipsis indicator.
#[derive(Debug)]
pub enum RenderedRow {
    Data(Vec<CellValue>),
    Ellipsis { skipped: usize },
}
