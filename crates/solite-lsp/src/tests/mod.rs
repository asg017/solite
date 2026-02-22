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

use crate::completions::{
    get_completions_extended, get_completions_for_context, quote_identifier_if_needed,
    CompletionOptions,
};
use crate::context::{detect_context, extract_used_insert_columns, CompletionContext, TableRef};
use solite_analyzer::{build_schema, Schema};
use solite_parser::parse_program;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind};

// ============================================================================
// Shared Test Helpers
// ============================================================================

/// Build schema from SQL source for testing
pub(crate) fn build_test_schema(sql: &str) -> Schema {
    let program = parse_program(sql).expect("Test SQL should parse");
    build_schema(&program)
}

/// Extract the prefix (partial word at cursor) from SQL text.
fn extract_prefix(sql: &str, offset: usize) -> Option<&str> {
    let before = &sql[..offset];
    let start = before
        .rfind(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
        .map(|i| i + 1)
        .unwrap_or(0);
    let prefix = &sql[start..offset];
    if prefix.is_empty() { None } else { Some(prefix) }
}

/// Get completion items for a given SQL at the cursor position (end of string)
pub(crate) fn get_completions_at_end(sql: &str, schema: &Schema) -> Vec<CompletionItem> {
    let offset = sql.len();
    let ctx = detect_context(sql, offset);
    get_completions_for_context(&ctx, Some(schema), extract_prefix(sql, offset))
}

/// Get completion items with document text context (enables SELECT/INSERT column filtering)
pub(crate) fn get_completions_with_text(sql: &str, schema: &Schema) -> Vec<CompletionItem> {
    let offset = sql.len();
    let ctx = detect_context(sql, offset);
    let prefix = extract_prefix(sql, offset);
    let options = CompletionOptions {
        document_text: Some(sql),
        cursor_offset: Some(offset),
        include_documentation: false,
        prefix,
    };
    get_completions_extended(&ctx, Some(schema), &options)
}
