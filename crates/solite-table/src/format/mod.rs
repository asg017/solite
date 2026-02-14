//! Cell formatting and syntax highlighting.

pub mod json;
pub mod value;

pub use json::format_json;
pub use value::format_cell;

/// Escape HTML special characters.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
