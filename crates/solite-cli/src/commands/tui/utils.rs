//! Shared TUI utility functions.

use ratatui::layout::{Constraint, Flex, Layout, Rect};

/// Helper function to create a centered rect using certain percentage of available rect
pub fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)]).flex(Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}

/// Create a centered popup with fixed dimensions
pub fn popup_area_fixed(area: Rect, width: u16, height: u16) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}

/// Truncate a string to max_len characters, adding "..." if truncated
pub fn truncate_string(s: &str, max_len: usize) -> String {
    if s.chars().count() > max_len {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    } else {
        s.to_string()
    }
}

/// Format a value for display, respecting max width
pub fn format_cell_value(value: &str, max_width: usize) -> String {
    // Replace newlines with visible marker
    let single_line = value.replace('\n', "\\n").replace('\r', "\\r");
    truncate_string(&single_line, max_width)
}
