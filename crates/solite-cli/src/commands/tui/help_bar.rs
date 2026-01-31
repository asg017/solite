//! HelpBar builder for creating consistent help text at the bottom of TUI pages.
//!
//! Inspired by libfec's TUI implementation.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::tui_theme::TuiTheme;

/// A single item in the help bar (key binding + label)
pub struct HelpItem<'a> {
    /// Keys to display (multiple keys are joined with "/")
    pub keys: Vec<&'a str>,
    /// Label describing what the key does
    pub label: &'a str,
}

impl<'a> HelpItem<'a> {
    /// Create a help item with a single key
    pub fn new(key: &'a str, label: &'a str) -> Self {
        Self {
            keys: vec![key],
            label,
        }
    }

    /// Create a help item with multiple keys (displayed as "key1/key2")
    pub fn keys(keys: Vec<&'a str>, label: &'a str) -> Self {
        Self { keys, label }
    }
}

enum HelpBarEntry<'a> {
    Item(HelpItem<'a>),
    /// Plain text displayed in secondary style
    Text(&'a str),
    /// Separator (vertical bar)
    Separator,
}

/// Builder for creating help bar content
pub struct HelpBar<'a> {
    items: Vec<HelpBarEntry<'a>>,
    theme: Option<&'a TuiTheme>,
}

impl<'a> HelpBar<'a> {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            theme: None,
        }
    }

    /// Set the theme for styling
    pub fn theme(mut self, theme: &'a TuiTheme) -> Self {
        self.theme = Some(theme);
        self
    }

    /// Add a key binding with a label
    pub fn item(mut self, key: &'a str, label: &'a str) -> Self {
        self.items
            .push(HelpBarEntry::Item(HelpItem::new(key, label)));
        self
    }

    /// Add multiple keys that do the same thing (displayed as "key1/key2 label")
    pub fn keys(mut self, keys: Vec<&'a str>, label: &'a str) -> Self {
        self.items
            .push(HelpBarEntry::Item(HelpItem::keys(keys, label)));
        self
    }

    /// Add plain text (useful for navigation hints)
    pub fn text(mut self, text: &'a str) -> Self {
        self.items.push(HelpBarEntry::Text(text));
        self
    }

    /// Add a separator between groups
    pub fn separator(mut self) -> Self {
        self.items.push(HelpBarEntry::Separator);
        self
    }

    /// Build into a Line for inline use (e.g., in popups)
    pub fn into_line(self) -> Line<'a> {
        let key_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let label_style = Style::default().fg(Color::DarkGray);

        let mut spans = Vec::new();

        for (i, entry) in self.items.into_iter().enumerate() {
            if i > 0 {
                match &entry {
                    HelpBarEntry::Separator => {}
                    _ => spans.push(Span::styled("  ", label_style)),
                }
            }

            match entry {
                HelpBarEntry::Item(item) => {
                    for (j, key) in item.keys.iter().enumerate() {
                        if j > 0 {
                            spans.push(Span::styled("/", label_style));
                        }
                        spans.push(Span::styled(*key, key_style));
                    }
                    spans.push(Span::styled(item.label, label_style));
                }
                HelpBarEntry::Text(text) => {
                    spans.push(Span::styled(text, label_style));
                }
                HelpBarEntry::Separator => {
                    spans.push(Span::styled("  │  ", label_style));
                }
            }
        }

        Line::from(spans)
    }

    /// Render as a help bar with top border (for bottom of detail views)
    pub fn render(self, f: &mut Frame, area: Rect) {
        let help = Paragraph::new(self.into_line())
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
        f.render_widget(help, area);
    }

    /// Render inline without border
    pub fn render_inline(self, f: &mut Frame, area: Rect) {
        let help = Paragraph::new(self.into_line()).alignment(Alignment::Center);
        f.render_widget(help, area);
    }
}

impl Default for HelpBar<'_> {
    fn default() -> Self {
        Self::new()
    }
}
