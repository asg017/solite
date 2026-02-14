//! Rendering utilities for Jupyter cell output.
//!
//! This module contains:
//! - `html`: Custom HTML builder for generating HTML strings
//! - `syntax`: SQL and JSON syntax highlighting
//! - `table`: SQL result table rendering

pub mod html;
pub mod syntax;
pub mod table;

pub use syntax::render_sql_html;
pub use table::render_statement;
