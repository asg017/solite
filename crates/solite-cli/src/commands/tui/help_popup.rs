//! `?` help overlay listing the complete keymap for the current page.
//!
//! Each page has a single static keymap table that feeds both this overlay
//! and the bottom [`HelpBar`](super::help_bar::HelpBar), so the two can't
//! drift apart: the overlay always shows every binding, the bar shows the
//! subset flagged `in_bar` (bottom-row space is limited).

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

use super::help_bar::HelpBar;
use super::utils::popup_area_fixed;

/// One key binding: the keys that trigger it and what it does.
///
/// `label` carries a leading space (the [`HelpBar`] convention); the overlay
/// trims it.
pub(crate) struct KeyBinding {
    pub keys: &'static [&'static str],
    pub label: &'static str,
    /// Also shown in the bottom help bar (the overlay shows everything).
    pub in_bar: bool,
}

const fn bar(keys: &'static [&'static str], label: &'static str) -> KeyBinding {
    KeyBinding {
        keys,
        label,
        in_bar: true,
    }
}

const fn overlay_only(keys: &'static [&'static str], label: &'static str) -> KeyBinding {
    KeyBinding {
        keys,
        label,
        in_bar: false,
    }
}

/// Keymap for the listing page.
pub(crate) const LISTING_KEYS: &[KeyBinding] = &[
    bar(&["j", "k"], " navigate"),
    overlay_only(&["g", "G"], " first/last table"),
    bar(&["1-9"], " jump"),
    bar(&["Enter"], " open"),
    overlay_only(&["?"], " help"),
    bar(&["q", "Esc"], " quit"),
];

/// Keymap for the table page.
pub(crate) const TABLE_KEYS: &[KeyBinding] = &[
    bar(&["h", "j", "k", "l"], " navigate"),
    bar(&["Enter"], " view row"),
    bar(&["["], " sort asc"),
    bar(&["]"], " sort desc"),
    overlay_only(&["g", "G"], " first/last row"),
    overlay_only(&["H", "L"], " first/last column"),
    overlay_only(&["PgUp", "Ctrl+u"], " page up"),
    overlay_only(&["PgDn", "Ctrl+d"], " page down"),
    bar(&["y", "c"], " copy"),
    overlay_only(&["?"], " help"),
    bar(&["q", "Esc"], " back"),
    overlay_only(&["Q"], " quit"),
];

/// Keymap for the row detail page.
pub(crate) const ROW_KEYS: &[KeyBinding] = &[
    bar(&["j", "k"], " navigate"),
    overlay_only(&["g", "G"], " first/last column"),
    bar(&["y", "Enter"], " copy value"),
    bar(&["p"], " copy PK"),
    bar(&["Y", "J"], " copy JSON"),
    overlay_only(&["?"], " help"),
    bar(&["q", "Esc"], " back"),
    overlay_only(&["Q"], " quit"),
];

/// Build the bottom help bar from a page keymap (the `in_bar` subset).
pub(crate) fn help_bar_from(bindings: &'static [KeyBinding]) -> HelpBar<'static> {
    let mut help_bar = HelpBar::new();
    for binding in bindings.iter().filter(|b| b.in_bar) {
        help_bar = help_bar.keys(binding.keys.to_vec(), binding.label);
    }
    help_bar
}

/// Modal overlay listing every key binding for the current page.
pub(crate) struct HelpPopup {
    pub visible: bool,
    title: &'static str,
    bindings: &'static [KeyBinding],
}

impl HelpPopup {
    pub fn new(title: &'static str, bindings: &'static [KeyBinding]) -> Self {
        Self {
            visible: false,
            title,
            bindings,
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    /// Handle a key while the popup is visible. Consumes every key;
    /// `Esc`/`q`/`?` dismiss the popup.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?')
        ) {
            self.visible = false;
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        // borders + one row per binding + dismiss hint
        let height = (self.bindings.len() as u16) + 3;
        let popup_area = popup_area_fixed(area, 40, height);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .title(self.title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let key_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let label_style = Style::default().fg(Color::DarkGray);

        let mut items: Vec<ListItem> = self
            .bindings
            .iter()
            .map(|binding| {
                let keys = binding.keys.join("/");
                let label = binding.label.trim_start();
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {:>12}  ", keys), key_style),
                    Span::styled(label, label_style),
                ]))
            })
            .collect();
        items.push(ListItem::new(Line::from(Span::styled(
            " press ? or Esc to close",
            label_style,
        ))));

        frame.render_widget(List::new(items), inner);
    }
}
