//! solite_lsp library
//!
//! This module provides the Solite SQL Language Server Protocol implementation.

pub mod completions;
pub mod context;
pub mod server;

#[cfg(test)]
mod tests;

pub use completions::{
    get_completions_extended, get_completions_for_context, quote_identifier_if_needed,
    CompletionOptions,
};
pub use context::{detect_context, extract_used_insert_columns, CompletionContext, CteRef, TableRef};
pub use server::run_server;
