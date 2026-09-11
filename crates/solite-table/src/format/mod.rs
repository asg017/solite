//! Cell formatting and syntax highlighting.

pub mod json;
pub mod theme_vars;
pub mod value;

pub use json::format_json;
pub use theme_vars::theme_css_vars;
pub use value::format_cell;

/// Escape HTML special characters.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
