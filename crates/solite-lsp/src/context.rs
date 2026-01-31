//! Completion context detection for SQL statements.
//!
//! This module re-exports the context detection types and functions from
//! `solite_completion` for backwards compatibility.

// Re-export everything from solite_completion::context
pub use solite_completion::{
    detect_context, detect_context_from_tokens, extract_used_insert_columns, CompletionContext,
    CteRef, TableRef,
};
