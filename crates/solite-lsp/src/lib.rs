//! solite_lsp library
//!
//! This module exports the completion context detection logic for use by other crates.

pub mod completions;
pub mod context;

pub use completions::{
    get_completions_extended, get_completions_for_context, quote_identifier_if_needed,
    CompletionOptions,
};
pub use context::{detect_context, CompletionContext, CteRef, TableRef};
