//! Test utilities and module declarations for solite_lsp
//!
//! Tests are organized into separate files by feature area:
//! - `completions.rs` - DDL, DML, keywords, alias, rowid completion tests
//! - `semantic_tokens.rs` - Snapshot tests for syntax highlighting
//! - `hover.rs` - Hover information and goto-definition tests
//! - `autocomplete.rs` - Smart autocomplete and placeholder-based test framework

mod autocomplete;
mod completions;
mod cte;
mod hover;
mod lsp_integration;
mod semantic_tokens;

use super::*;
use solite_lsp::completions::{get_completions_for_context, quote_identifier_if_needed};
use solite_lsp::context::{detect_context, extract_used_insert_columns, CompletionContext, TableRef};
use solite_analyzer::build_schema;
use solite_parser::parse_program;

// ============================================================================
// Shared Test Helpers
// ============================================================================

/// Build schema from SQL source for testing
pub(crate) fn build_test_schema(sql: &str) -> Schema {
    let program = parse_program(sql).expect("Test SQL should parse");
    build_schema(&program)
}

/// Get completion items for a given SQL at the cursor position (end of string)
pub(crate) fn get_completions_at_end(sql: &str, schema: &Schema) -> Vec<CompletionItem> {
    let offset = sql.len();
    let ctx = detect_context(sql, offset);
    get_completions_for_context(&ctx, Some(schema))
}
