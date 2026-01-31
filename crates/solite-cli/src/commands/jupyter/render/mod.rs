//! Rendering utilities for Jupyter cell output.
//!
//! This module contains:
//! - `html`: Custom HTML builder for generating HTML strings
//! - `syntax`: SQL and JSON syntax highlighting
//! - `table`: SQL result table rendering

pub mod html;
pub mod syntax;
pub mod table;

pub use html::{Element, HtmlDoc};
pub use syntax::{render_json_cell, render_sql_html, STATEMENT_CELL_CSS};
pub use table::{render_statement, UiResponse};
