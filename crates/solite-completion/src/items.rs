//! Abstract completion item types.
//!
//! These types are used to represent completion items in a format-agnostic way.
//! They can be converted to LSP types, rustyline Pairs, or any other format.

/// The kind of a completion item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    /// A SQL keyword (SELECT, FROM, WHERE, etc.)
    Keyword,
    /// A table name
    Table,
    /// A column name
    Column,
    /// An index name
    Index,
    /// A view name
    View,
    /// A function name
    Function,
    /// An operator (=, <>, LIKE, etc.)
    Operator,
    /// A Common Table Expression (CTE)
    Cte,
}

/// An abstract completion item.
///
/// This type represents a completion suggestion in a format-agnostic way.
/// It can be converted to LSP CompletionItem, rustyline Pair, or other formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    /// The label to display in the completion list
    pub label: String,
    /// The text to insert when this item is selected.
    /// If None, the label is used.
    pub insert_text: Option<String>,
    /// The kind of completion item
    pub kind: CompletionKind,
    /// A short description of the item (e.g., "from users" for a column)
    pub detail: Option<String>,
    /// Sort order for the item (lower numbers appear first)
    pub sort_order: Option<u32>,
}

impl CompletionItem {
    /// Create a new completion item with the given label and kind.
    pub fn new(label: impl Into<String>, kind: CompletionKind) -> Self {
        Self {
            label: label.into(),
            insert_text: None,
            kind,
            detail: None,
            sort_order: None,
        }
    }

    /// Set the text to insert when this item is selected.
    pub fn with_insert_text(mut self, insert_text: impl Into<String>) -> Self {
        self.insert_text = Some(insert_text.into());
        self
    }

    /// Set the detail string.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Set the sort order.
    pub fn with_sort_order(mut self, order: u32) -> Self {
        self.sort_order = Some(order);
        self
    }
}
