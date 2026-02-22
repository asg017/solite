//! Completion generation engine.
//!
//! This module contains the core logic for generating completion items
//! based on the detected context and schema information.

use std::collections::HashMap;

use crate::context::{CompletionContext, CteRef, TableRef};
use crate::items::{CompletionItem, CompletionKind};
use crate::schema::SchemaSource;

/// Quote an identifier if it needs quoting (contains special chars or starts with a digit).
pub fn quote_identifier_if_needed(name: &str) -> Option<String> {
    let needs_quoting = name.contains(' ')
        || name.contains('-')
        || name.contains('.')
        || name.chars().next().map(|c| c.is_numeric()).unwrap_or(false);

    if needs_quoting {
        Some(format!("\"{}\"", name))
    } else {
        None
    }
}

/// Generate completion items for a given context and schema.
///
/// This is the main entry point for the completion engine. It takes a context
/// (detected by `detect_context`) and an optional schema source, and returns
/// a list of completion items appropriate for that context.
///
/// `prefix` is the partial word typed at the cursor. Functions are only
/// suggested when at least one character has been typed.
pub fn get_completions(
    ctx: &CompletionContext,
    schema: Option<&dyn SchemaSource>,
    prefix: Option<&str>,
) -> Vec<CompletionItem> {
    let include_functions = prefix.is_some_and(|p| !p.is_empty());
    match ctx {
        // Statement start - suggest SQL keywords filtered by prefix
        CompletionContext::StatementStart { ref prefix } => {
            let Some(prefix) = prefix else {
                return vec![];
            };
            statement_start_keywords()
                .into_iter()
                .filter(|item| item.label.starts_with(prefix.as_str()))
                .collect()
        }

        // Table name contexts - suggest table names (including CTEs)
        CompletionContext::AfterFrom { ref ctes }
        | CompletionContext::AfterJoin { ref ctes } => {
            let mut items = vec![];
            // Add CTE names first
            for cte in ctes {
                items.push(
                    CompletionItem::new(&cte.name, CompletionKind::Cte)
                        .with_detail("CTE")
                );
            }
            // Add table names from schema
            if let Some(schema) = schema {
                for name in schema.table_names() {
                    let mut item = CompletionItem::new(&name, CompletionKind::Table);
                    if let Some(quoted) = quote_identifier_if_needed(&name) {
                        item = item.with_insert_text(quoted);
                    }
                    items.push(item);
                }
            }
            items
        }

        // After a table name in JOIN clause - suggest ON and AS
        CompletionContext::AfterJoinTable { .. } => {
            vec![
                CompletionItem::new("on", CompletionKind::Keyword),
                CompletionItem::new("as", CompletionKind::Keyword),
            ]
        }

        // After a table name in FROM - suggest JOIN keywords, WHERE, tables, etc.
        CompletionContext::AfterFromTable { ref ctes } => {
            let mut items = vec![];
            // Add JOIN keywords
            for kw in &[
                "join",
                "inner join",
                "left join",
                "left outer join",
                "right join",
                "right outer join",
                "full join",
                "full outer join",
                "cross join",
                "natural join",
            ] {
                items.push(CompletionItem::new(*kw, CompletionKind::Keyword));
            }
            // Add clause keywords
            for kw in &["where", "group by", "order by", "limit"] {
                items.push(CompletionItem::new(*kw, CompletionKind::Keyword));
            }
            // Add CTE names (for comma-separated table list)
            for cte in ctes {
                items.push(
                    CompletionItem::new(&cte.name, CompletionKind::Cte)
                        .with_detail("CTE")
                );
            }
            // Add table names from schema
            if let Some(schema) = schema {
                for name in schema.table_names() {
                    let mut item = CompletionItem::new(&name, CompletionKind::Table);
                    if let Some(quoted) = quote_identifier_if_needed(&name) {
                        item = item.with_insert_text(quoted);
                    }
                    items.push(item);
                }
            }
            items
        }

        CompletionContext::AfterInto
        | CompletionContext::AfterUpdate
        | CompletionContext::AfterTable
        | CompletionContext::AfterOn => {
            if let Some(schema) = schema {
                schema
                    .table_names()
                    .into_iter()
                    .map(|name| {
                        let mut item = CompletionItem::new(&name, CompletionKind::Table);
                        if let Some(quoted) = quote_identifier_if_needed(&name) {
                            item = item.with_insert_text(quoted);
                        }
                        item
                    })
                    .collect()
            } else {
                vec![]
            }
        }

        // Column name contexts with tables in scope
        CompletionContext::SelectColumns { ref tables, ref ctes }
        | CompletionContext::WhereClause { ref tables, ref ctes }
        | CompletionContext::GroupByClause { ref tables, ref ctes }
        | CompletionContext::HavingClause { ref tables, ref ctes }
        | CompletionContext::OrderByClause { ref tables, ref ctes } => {
            if let Some(schema) = schema {
                let mut items = suggest_columns_from_tables(schema, tables, ctes);
                if include_functions {
                    items.extend(suggest_functions(schema));
                }
                items
            } else {
                vec![]
            }
        }

        // Expression context (no CTE support)
        CompletionContext::Expression { ref tables } => {
            if let Some(schema) = schema {
                let mut items = suggest_columns_from_tables(schema, tables, &[]);
                if include_functions {
                    items.extend(suggest_functions(schema));
                }
                items
            } else {
                vec![]
            }
        }

        // JOIN ON - include both left tables and the right table being joined
        CompletionContext::JoinOn {
            ref left_tables,
            ref right_table,
            ref ctes,
        } => {
            if let Some(schema) = schema {
                let mut all_tables = left_tables.clone();
                all_tables.push(right_table.clone());
                let mut items = suggest_columns_from_tables(schema, &all_tables, ctes);
                if include_functions {
                    items.extend(suggest_functions(schema));
                }
                items
            } else {
                vec![]
            }
        }

        // INSERT columns
        CompletionContext::InsertColumns { ref table_name } => {
            if let Some(schema) = schema {
                schema
                    .columns_for_table_with_rowid(table_name)
                    .map(|cols| {
                        cols.into_iter()
                            .map(|col| {
                                let mut item = CompletionItem::new(&col, CompletionKind::Column);
                                if let Some(quoted) = quote_identifier_if_needed(&col) {
                                    item = item.with_insert_text(quoted);
                                }
                                item
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            }
        }

        // UPDATE SET and other single-table column contexts
        CompletionContext::UpdateSet { ref table_name }
        | CompletionContext::DeleteWhere { ref table_name }
        | CompletionContext::CreateIndexColumns { ref table_name } => {
            if let Some(schema) = schema {
                schema
                    .columns_for_table_with_rowid(table_name)
                    .map(|cols| {
                        cols.into_iter()
                            .map(|col| {
                                let mut item = CompletionItem::new(&col, CompletionKind::Column);
                                if let Some(quoted) = quote_identifier_if_needed(&col) {
                                    item = item.with_insert_text(quoted);
                                }
                                item
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            }
        }

        // ALTER TABLE column - does NOT include rowid (can't drop/rename implicit rowid)
        CompletionContext::AlterColumn { ref table_name } => {
            if let Some(schema) = schema {
                schema
                    .columns_for_table(table_name)
                    .map(|cols| {
                        cols.into_iter()
                            .map(|col| {
                                let mut item = CompletionItem::new(&col, CompletionKind::Column);
                                if let Some(quoted) = quote_identifier_if_needed(&col) {
                                    item = item.with_insert_text(quoted);
                                }
                                item
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            }
        }

        // ALTER TABLE action keywords
        CompletionContext::AlterTableAction { .. } => alter_table_action_keywords(),

        // Index names for DROP INDEX
        CompletionContext::AfterIndex => {
            if let Some(schema) = schema {
                schema
                    .index_names()
                    .into_iter()
                    .map(|name| {
                        let mut item = CompletionItem::new(&name, CompletionKind::Index);
                        if let Some(quoted) = quote_identifier_if_needed(&name) {
                            item = item.with_insert_text(quoted);
                        }
                        item
                    })
                    .collect()
            } else {
                vec![]
            }
        }

        // View names for DROP VIEW
        CompletionContext::AfterView => {
            if let Some(schema) = schema {
                schema
                    .view_names()
                    .into_iter()
                    .map(|name| {
                        let mut item = CompletionItem::new(&name, CompletionKind::View);
                        if let Some(quoted) = quote_identifier_if_needed(&name) {
                            item = item.with_insert_text(quoted);
                        }
                        item
                    })
                    .collect()
            } else {
                vec![]
            }
        }

        // Qualified column - suggest columns from the specific qualifier (includes rowid)
        CompletionContext::QualifiedColumn {
            ref qualifier,
            ref tables,
            ref ctes,
        } => {
            // Check if qualifier matches a CTE first
            let cte_match = ctes.iter().find(|c| c.name.eq_ignore_ascii_case(qualifier));
            if let Some(cte) = cte_match {
                // Return CTE columns
                cte.columns
                    .iter()
                    .map(|col| {
                        let mut item = CompletionItem::new(col, CompletionKind::Column);
                        if let Some(quoted) = quote_identifier_if_needed(col) {
                            item = item.with_insert_text(quoted);
                        }
                        item
                    })
                    .collect()
            } else if let Some(schema) = schema {
                // Find the table for this qualifier (case-insensitive)
                let table_name = tables
                    .iter()
                    .find(|t| t.matches_qualifier(qualifier))
                    .map(|t| t.name.as_str())
                    .unwrap_or(qualifier);

                schema
                    .columns_for_table_with_rowid(table_name)
                    .map(|cols| {
                        cols.into_iter()
                            .map(|col| {
                                let mut item = CompletionItem::new(&col, CompletionKind::Column);
                                if let Some(quoted) = quote_identifier_if_needed(&col) {
                                    item = item.with_insert_text(quoted);
                                }
                                item
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            }
        }

        // CREATE TABLE - suggest IF NOT EXISTS
        CompletionContext::AfterCreateTable => after_create_table_keywords(),

        // After column type in CREATE TABLE - suggest column constraints
        CompletionContext::CreateTableColumnConstraint => {
            vec![
                CompletionItem::new("primary key", CompletionKind::Keyword),
                CompletionItem::new("not null", CompletionKind::Keyword),
                CompletionItem::new("unique", CompletionKind::Keyword),
                CompletionItem::new("default", CompletionKind::Keyword),
                CompletionItem::new("collate", CompletionKind::Keyword),
                CompletionItem::new("references", CompletionKind::Keyword),
                CompletionItem::new("check", CompletionKind::Keyword),
                CompletionItem::new("generated always as", CompletionKind::Keyword),
                CompletionItem::new("as", CompletionKind::Keyword),
            ]
        }

        // After CREATE - suggest object types
        CompletionContext::AfterCreate => after_create_keywords(),

        // INSERT - suggest INTO, OR ABORT/FAIL/IGNORE/REPLACE/ROLLBACK
        CompletionContext::AfterInsert => after_insert_keywords(),

        // REPLACE - suggest INTO
        CompletionContext::AfterReplace => {
            vec![CompletionItem::new("into", CompletionKind::Keyword)]
        }

        // After DROP - suggest object types
        CompletionContext::AfterDrop => after_drop_keywords(),

        // After ALTER - suggest TABLE
        CompletionContext::AfterAlter => {
            vec![CompletionItem::new("table", CompletionKind::Keyword)]
        }

        // After an expression - suggest operators and clause keywords
        CompletionContext::AfterExpr { .. } => {
            vec![
                // Logical operators
                CompletionItem::new("and", CompletionKind::Keyword),
                CompletionItem::new("or", CompletionKind::Keyword),
                // Comparison operators
                CompletionItem::new("=", CompletionKind::Operator),
                CompletionItem::new("<>", CompletionKind::Operator),
                CompletionItem::new("<", CompletionKind::Operator),
                CompletionItem::new("<=", CompletionKind::Operator),
                CompletionItem::new(">", CompletionKind::Operator),
                CompletionItem::new(">=", CompletionKind::Operator),
                // JSON extract operators
                CompletionItem::new("->", CompletionKind::Operator),
                CompletionItem::new("->>", CompletionKind::Operator),
                // SQL operators
                CompletionItem::new("like", CompletionKind::Keyword),
                CompletionItem::new("in", CompletionKind::Keyword),
                CompletionItem::new("between", CompletionKind::Keyword),
                CompletionItem::new("is null", CompletionKind::Keyword),
                CompletionItem::new("is not null", CompletionKind::Keyword),
                // Clause keywords
                CompletionItem::new("order by", CompletionKind::Keyword),
                CompletionItem::new("group by", CompletionKind::Keyword),
                CompletionItem::new("limit", CompletionKind::Keyword),
            ]
        }

        // Other contexts - return empty
        CompletionContext::None => vec![],
    }
}

// ============================================================================
// Column Suggestion Helpers
// ============================================================================

/// Suggest columns from tables in scope.
///
/// For ambiguous columns (same name in multiple tables), suggests qualified names like `u.id`.
/// For unambiguous columns, suggests just the column name.
fn suggest_columns_from_tables(
    schema: &dyn SchemaSource,
    tables: &[TableRef],
    ctes: &[CteRef],
) -> Vec<CompletionItem> {
    // No tables in scope - suggest all columns from all tables
    if tables.is_empty() && ctes.is_empty() {
        return suggest_all_columns(schema);
    }

    let mut items = Vec::new();

    // Track which columns come from which tables (for CTEs too)
    let mut column_sources: HashMap<String, Vec<String>> = HashMap::new();

    // Process table references, checking if they're CTEs
    for table_ref in tables {
        // Check if this table reference is a CTE
        let cte_match = ctes
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(&table_ref.name));

        if let Some(cte) = cte_match {
            // Use CTE columns if available
            if !cte.columns.is_empty() {
                for col in &cte.columns {
                    let qualifier = table_ref.qualifier().to_string();
                    column_sources
                        .entry(col.clone())
                        .or_default()
                        .push(qualifier);
                }
            } else {
                // Columns are empty - try to resolve from star_sources using schema
                for star_source in &cte.star_sources {
                    // Check if star_source is another CTE
                    if let Some(source_cte) = ctes.iter().find(|c| c.name.eq_ignore_ascii_case(star_source)) {
                        for col in &source_cte.columns {
                            let qualifier = table_ref.qualifier().to_string();
                            column_sources
                                .entry(col.clone())
                                .or_default()
                                .push(qualifier);
                        }
                    } else if let Some(cols) = schema.columns_for_table_with_rowid(star_source) {
                        // star_source is a real table - get columns from schema
                        for col in cols {
                            let qualifier = table_ref.qualifier().to_string();
                            column_sources.entry(col).or_default().push(qualifier);
                        }
                    }
                }
            }
        } else if let Some(cols) = schema.columns_for_table_with_rowid(&table_ref.name) {
            for col in cols {
                let qualifier = table_ref.qualifier().to_string();
                column_sources.entry(col).or_default().push(qualifier);
            }
        }
    }

    // Generate completion items based on ambiguity
    for (col, sources) in column_sources {
        if sources.len() == 1 {
            // Unambiguous - suggest just the column name
            let mut item = CompletionItem::new(&col, CompletionKind::Column)
                .with_detail(format!("from {}", sources[0]));
            if let Some(quoted) = quote_identifier_if_needed(&col) {
                item = item.with_insert_text(quoted);
            }
            items.push(item);
        } else {
            // Ambiguous - suggest qualified names for each source
            for source in sources {
                let qualified = format!("{}.{}", source, col);
                // For qualified names, quote each part if needed
                let quoted_source = quote_identifier_if_needed(&source);
                let quoted_col = quote_identifier_if_needed(&col);
                let insert = match (quoted_source, quoted_col) {
                    (Some(s), Some(c)) => Some(format!("{}.{}", s, c)),
                    (Some(s), None) => Some(format!("{}.{}", s, col)),
                    (None, Some(c)) => Some(format!("{}.{}", source, c)),
                    (None, None) => None,
                };
                let mut item = CompletionItem::new(&qualified, CompletionKind::Column);
                if let Some(text) = insert {
                    item = item.with_insert_text(text);
                }
                items.push(item);
            }
        }
    }

    items
}

/// Suggest all columns from all tables in the schema.
fn suggest_all_columns(schema: &dyn SchemaSource) -> Vec<CompletionItem> {
    // Track which tables contain each column
    let mut column_to_tables: HashMap<String, Vec<String>> = HashMap::new();

    for table_name in schema.table_names() {
        if let Some(cols) = schema.columns_for_table_with_rowid(&table_name) {
            for col in cols {
                column_to_tables
                    .entry(col)
                    .or_default()
                    .push(table_name.clone());
            }
        }
    }

    let mut items = Vec::new();
    for (col, tables) in column_to_tables {
        let detail = if tables.len() == 1 {
            format!("from {}", tables[0])
        } else {
            format!("from: {}", tables.join(", "))
        };

        let mut item = CompletionItem::new(&col, CompletionKind::Column)
            .with_detail(detail);
        if let Some(quoted) = quote_identifier_if_needed(&col) {
            item = item.with_insert_text(quoted);
        }
        items.push(item);
    }

    items
}

// ============================================================================
// Function Suggestion Helpers
// ============================================================================

/// Suggest scalar functions from the schema source.
fn suggest_functions(schema: &dyn SchemaSource) -> Vec<CompletionItem> {
    schema
        .function_names()
        .into_iter()
        .filter(|name| name != "->" && name != "->>")
        .map(|name| {
            let is_zero_arg = schema
                .function_nargs(&name)
                .map(|nargs| nargs.len() == 1 && nargs[0] == 0)
                .unwrap_or(false);
            let insert = if is_zero_arg {
                format!("{}()", name)
            } else {
                format!("{}(", name)
            };
            CompletionItem::new(&name, CompletionKind::Function)
                .with_insert_text(insert)
        })
        .collect()
}

// ============================================================================
// Keyword Completion Helpers
// ============================================================================

/// Keywords available at the start of a statement.
fn statement_start_keywords() -> Vec<CompletionItem> {
    let keywords = [
        "select", "insert", "update", "delete", "create", "drop", "alter",
        "replace", "begin", "commit", "rollback", "savepoint", "release",
        "vacuum", "analyze", "reindex", "attach", "detach", "pragma",
        "explain", "with",
    ];

    keywords
        .iter()
        .enumerate()
        .map(|(i, kw)| {
            CompletionItem::new(*kw, CompletionKind::Keyword)
                .with_sort_order(i as u32)
        })
        .collect()
}

/// Keywords available after CREATE.
fn after_create_keywords() -> Vec<CompletionItem> {
    vec![
        CompletionItem::new("table", CompletionKind::Keyword),
        CompletionItem::new("index", CompletionKind::Keyword),
        CompletionItem::new("unique index", CompletionKind::Keyword),
        CompletionItem::new("view", CompletionKind::Keyword),
        CompletionItem::new("trigger", CompletionKind::Keyword),
        CompletionItem::new("virtual table", CompletionKind::Keyword),
    ]
}

/// Keywords available after CREATE TABLE.
fn after_create_table_keywords() -> Vec<CompletionItem> {
    vec![CompletionItem::new("if not exists", CompletionKind::Keyword)]
}

/// Keywords available after INSERT.
fn after_insert_keywords() -> Vec<CompletionItem> {
    vec![
        CompletionItem::new("into", CompletionKind::Keyword),
        CompletionItem::new("or abort", CompletionKind::Keyword),
        CompletionItem::new("or fail", CompletionKind::Keyword),
        CompletionItem::new("or ignore", CompletionKind::Keyword),
        CompletionItem::new("or replace", CompletionKind::Keyword),
        CompletionItem::new("or rollback", CompletionKind::Keyword),
    ]
}

/// Keywords available after DROP.
fn after_drop_keywords() -> Vec<CompletionItem> {
    vec![
        CompletionItem::new("table", CompletionKind::Keyword),
        CompletionItem::new("index", CompletionKind::Keyword),
        CompletionItem::new("view", CompletionKind::Keyword),
        CompletionItem::new("trigger", CompletionKind::Keyword),
    ]
}

/// Keywords available for ALTER TABLE actions.
fn alter_table_action_keywords() -> Vec<CompletionItem> {
    vec![
        CompletionItem::new("add", CompletionKind::Keyword),
        CompletionItem::new("add column", CompletionKind::Keyword),
        CompletionItem::new("drop column", CompletionKind::Keyword),
        CompletionItem::new("rename to", CompletionKind::Keyword),
        CompletionItem::new("rename column", CompletionKind::Keyword),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestSchema {
        tables: Vec<(String, Vec<String>)>,
    }

    impl SchemaSource for TestSchema {
        fn table_names(&self) -> Vec<String> {
            self.tables.iter().map(|(name, _)| name.clone()).collect()
        }

        fn columns_for_table(&self, table: &str) -> Option<Vec<String>> {
            self.tables
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(table))
                .map(|(_, cols)| cols.clone())
        }

        fn index_names(&self) -> Vec<String> {
            vec![]
        }

        fn view_names(&self) -> Vec<String> {
            vec![]
        }
    }

    #[test]
    fn test_statement_start_completions() {
        let ctx = CompletionContext::StatementStart { prefix: Some("sel".to_string()) };
        let items = get_completions(&ctx, None, None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "select");
    }

    #[test]
    fn test_after_from_completions() {
        let schema = TestSchema {
            tables: vec![
                ("users".to_string(), vec!["id".to_string(), "name".to_string()]),
            ],
        };
        let ctx = CompletionContext::AfterFrom { ctes: vec![] };
        let items = get_completions(&ctx, Some(&schema), None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "users");
        assert_eq!(items[0].kind, CompletionKind::Table);
    }

    #[test]
    fn test_column_completions() {
        let schema = TestSchema {
            tables: vec![
                ("users".to_string(), vec!["id".to_string(), "name".to_string()]),
            ],
        };
        let ctx = CompletionContext::WhereClause {
            tables: vec![TableRef::new("users".to_string(), None)],
            ctes: vec![],
        };
        let items = get_completions(&ctx, Some(&schema), None);
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "id"));
        assert!(items.iter().any(|i| i.label == "name"));
    }
}
