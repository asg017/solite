//! Shared SQL completion engine for Solite.
//!
//! This crate provides context-aware SQL completion that can be used by both
//! the LSP server and the REPL. It handles:
//!
//! - Context detection: Determining what kind of completion is appropriate
//!   at a given cursor position (tables, columns, keywords, etc.)
//! - Completion generation: Producing completion items based on context and schema
//!
//! The crate is intentionally abstract, using trait-based schema access so it
//! can work with both static analyzer schemas and live database connections.

mod context;
mod engine;
mod items;
mod schema;

pub use context::{
    detect_context, detect_context_from_tokens, extract_used_insert_columns,
    extract_used_select_columns, CompletionContext, CteRef, TableRef,
};
pub use engine::get_completions;
pub use items::{CompletionItem, CompletionKind};
pub use schema::SchemaSource;
