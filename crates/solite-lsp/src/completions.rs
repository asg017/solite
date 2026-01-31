//! Completion generation logic for SQL contexts
//!
//! This module provides the core completion logic used by the LSP server,
//! tests, and external consumers like mdtest.

use std::collections::{HashMap, HashSet};

use solite_analyzer::Schema;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, Documentation, InsertTextFormat,
};

use crate::context::{extract_used_insert_columns, CompletionContext, CteRef, TableRef};

/// Extended completion options for LSP-specific features
#[derive(Default)]
pub struct CompletionOptions<'a> {
    /// Full document text (needed for INSERT column filtering)
    pub document_text: Option<&'a str>,
    /// Cursor offset in document (needed for INSERT column filtering)
    pub cursor_offset: Option<usize>,
    /// Include rich documentation on keywords
    pub include_documentation: bool,
}

/// Quote an identifier if it needs quoting (contains special chars or is a keyword)
pub fn quote_identifier_if_needed(name: &str) -> Option<String> {
    // Check if identifier needs quoting
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

/// Generate completion items for a given context and schema (basic API)
///
/// This is the simple API for tests and external consumers. For LSP-specific
/// features like INSERT column filtering or rich documentation, use
/// `get_completions_extended` instead.
pub fn get_completions_for_context(
    ctx: &CompletionContext,
    schema: Option<&Schema>,
) -> Vec<CompletionItem> {
    get_completions_extended(ctx, schema, &CompletionOptions::default())
}

/// Generate completion items with extended options
///
/// This is the full-featured API used by the LSP server. It supports:
/// - INSERT column filtering (hides already-used columns)
/// - Smart SELECT-without-FROM snippets
/// - Rich keyword documentation
pub fn get_completions_extended(
    ctx: &CompletionContext,
    schema: Option<&Schema>,
    options: &CompletionOptions,
) -> Vec<CompletionItem> {
    match ctx {
        // Statement start - suggest SQL keywords filtered by prefix
        CompletionContext::StatementStart { ref prefix } => {
            // If no prefix, return empty (don't suggest until user types something)
            let Some(prefix) = prefix else {
                return vec![];
            };

            statement_start_keywords(options.include_documentation)
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
                items.push(CompletionItem {
                    label: cte.name.clone(),
                    kind: Some(CompletionItemKind::CLASS),
                    detail: Some("CTE".to_string()),
                    ..Default::default()
                });
            }
            // Add table names from schema
            if let Some(schema) = schema {
                for name in schema.table_names() {
                    items.push(CompletionItem {
                        label: name.to_string(),
                        insert_text: quote_identifier_if_needed(name),
                        kind: Some(CompletionItemKind::CLASS),
                        ..Default::default()
                    });
                }
            }
            items
        }

        // After a table name in JOIN clause - suggest ON and AS
        CompletionContext::AfterJoinTable { .. } => {
            vec![
                CompletionItem {
                    label: "on".to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                },
                CompletionItem {
                    label: "as".to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                },
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
                items.push(CompletionItem {
                    label: kw.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                });
            }
            // Add clause keywords
            for kw in &["where", "group by", "order by", "limit"] {
                items.push(CompletionItem {
                    label: kw.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                });
            }
            // Add CTE names (for comma-separated table list)
            for cte in ctes {
                items.push(CompletionItem {
                    label: cte.name.clone(),
                    kind: Some(CompletionItemKind::CLASS),
                    detail: Some("CTE".to_string()),
                    ..Default::default()
                });
            }
            // Add table names from schema
            if let Some(schema) = schema {
                for name in schema.table_names() {
                    items.push(CompletionItem {
                        label: name.to_string(),
                        insert_text: quote_identifier_if_needed(name),
                        kind: Some(CompletionItemKind::CLASS),
                        ..Default::default()
                    });
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
                    .map(|name| CompletionItem {
                        label: name.to_string(),
                        insert_text: quote_identifier_if_needed(name),
                        kind: Some(CompletionItemKind::CLASS),
                        ..Default::default()
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
                suggest_columns_from_tables(schema, tables, ctes)
            } else {
                vec![]
            }
        }

        // Expression context (no CTE support)
        CompletionContext::Expression { ref tables } => {
            if let Some(schema) = schema {
                suggest_columns_from_tables(schema, tables, &[])
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
                suggest_columns_from_tables(schema, &all_tables, ctes)
            } else {
                vec![]
            }
        }

        // INSERT columns - with optional filtering of already-used columns
        CompletionContext::InsertColumns { ref table_name } => {
            if let Some(schema) = schema {
                // Get used columns if we have document context
                let used_columns: HashSet<String> =
                    if let (Some(text), Some(offset)) = (options.document_text, options.cursor_offset)
                    {
                        extract_used_insert_columns(text, offset)
                    } else {
                        HashSet::new()
                    };

                schema
                    .columns_for_table_with_rowid(table_name)
                    .map(|cols| {
                        cols.into_iter()
                            .filter(|col| !used_columns.contains(&col.to_lowercase()))
                            .map(|col| CompletionItem {
                                label: col.clone(),
                                insert_text: quote_identifier_if_needed(&col),
                                kind: Some(CompletionItemKind::FIELD),
                                ..Default::default()
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
                            .map(|col| CompletionItem {
                                label: col.clone(),
                                insert_text: quote_identifier_if_needed(&col),
                                kind: Some(CompletionItemKind::FIELD),
                                ..Default::default()
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
                        cols.iter()
                            .map(|col| CompletionItem {
                                label: col.clone(),
                                insert_text: quote_identifier_if_needed(col),
                                kind: Some(CompletionItemKind::FIELD),
                                ..Default::default()
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            }
        }

        // ALTER TABLE action keywords
        CompletionContext::AlterTableAction { .. } => {
            alter_table_action_keywords(options.include_documentation)
        }

        // Index names for DROP INDEX
        CompletionContext::AfterIndex => {
            if let Some(schema) = schema {
                schema
                    .index_names()
                    .map(|name| CompletionItem {
                        label: name.to_string(),
                        insert_text: quote_identifier_if_needed(name),
                        kind: Some(CompletionItemKind::CLASS),
                        ..Default::default()
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
                    .map(|name| CompletionItem {
                        label: name.to_string(),
                        insert_text: quote_identifier_if_needed(name),
                        kind: Some(CompletionItemKind::CLASS),
                        ..Default::default()
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
                    .map(|col| CompletionItem {
                        label: col.clone(),
                        insert_text: quote_identifier_if_needed(col),
                        kind: Some(CompletionItemKind::FIELD),
                        ..Default::default()
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
                            .map(|col| CompletionItem {
                                label: col.clone(),
                                insert_text: quote_identifier_if_needed(&col),
                                kind: Some(CompletionItemKind::FIELD),
                                ..Default::default()
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            }
        }

        // CREATE TABLE - suggest IF NOT EXISTS
        CompletionContext::AfterCreateTable => {
            after_create_table_keywords(options.include_documentation)
        }

        // After column type in CREATE TABLE - suggest column constraints
        CompletionContext::CreateTableColumnConstraint => {
            let mut items: Vec<CompletionItem> = vec![
                "primary key",
                "not null",
                "unique",
                "default",
                "collate",
                "references",
            ]
            .into_iter()
            .map(|kw| CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            })
            .collect();

            // Constraints that need parens get snippet treatment with cursor inside
            for (label, insert) in [
                ("check", "check ($0)"),
                ("generated always as", "generated always as ($0)"),
                ("as", "as ($0)"),
            ] {
                items.push(CompletionItem {
                    label: label.to_string(),
                    insert_text: Some(insert.to_string()),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                });
            }

            items
        }

        // After CREATE - suggest object types
        CompletionContext::AfterCreate => after_create_keywords(options.include_documentation),

        // INSERT - suggest INTO, OR ABORT/FAIL/IGNORE/REPLACE/ROLLBACK
        CompletionContext::AfterInsert => after_insert_keywords(options.include_documentation),

        // REPLACE - suggest INTO
        CompletionContext::AfterReplace => after_replace_keywords(options.include_documentation),

        // After DROP - suggest object types
        CompletionContext::AfterDrop => after_drop_keywords(options.include_documentation),

        // After ALTER - suggest TABLE
        CompletionContext::AfterAlter => after_alter_keywords(options.include_documentation),

        // After an expression in WHERE clause - suggest operators and clause keywords
        CompletionContext::AfterWhereExpr { .. } => {
            vec![
                // Logical operators
                CompletionItem {
                    label: "and".to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                },
                CompletionItem {
                    label: "or".to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                },
                // Comparison operators
                CompletionItem {
                    label: "=".to_string(),
                    kind: Some(CompletionItemKind::OPERATOR),
                    ..Default::default()
                },
                CompletionItem {
                    label: "<>".to_string(),
                    kind: Some(CompletionItemKind::OPERATOR),
                    ..Default::default()
                },
                CompletionItem {
                    label: "<".to_string(),
                    kind: Some(CompletionItemKind::OPERATOR),
                    ..Default::default()
                },
                CompletionItem {
                    label: "<=".to_string(),
                    kind: Some(CompletionItemKind::OPERATOR),
                    ..Default::default()
                },
                CompletionItem {
                    label: ">".to_string(),
                    kind: Some(CompletionItemKind::OPERATOR),
                    ..Default::default()
                },
                CompletionItem {
                    label: ">=".to_string(),
                    kind: Some(CompletionItemKind::OPERATOR),
                    ..Default::default()
                },
                // SQL operators
                CompletionItem {
                    label: "like".to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                },
                CompletionItem {
                    label: "in".to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                },
                CompletionItem {
                    label: "between".to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                },
                CompletionItem {
                    label: "is null".to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                },
                CompletionItem {
                    label: "is not null".to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                },
                // Clause keywords
                CompletionItem {
                    label: "order by".to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                },
                CompletionItem {
                    label: "group by".to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                },
                CompletionItem {
                    label: "limit".to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                },
            ]
        }

        // Other contexts - return empty for now
        CompletionContext::None => vec![],
    }
}

// ============================================================================
// Column Suggestion Helpers
// ============================================================================

/// Suggest columns from tables in scope
///
/// For ambiguous columns (same name in multiple tables), suggests qualified names like `u.id`.
/// For unambiguous columns, suggests just the column name.
/// When no tables are in scope (SELECT without FROM), suggests all columns with smart
/// FROM clause insertion.
fn suggest_columns_from_tables(
    schema: &Schema,
    tables: &[TableRef],
    ctes: &[CteRef],
) -> Vec<CompletionItem> {
    // No tables in scope (SELECT without FROM) - suggest all columns from all tables
    // with smart FROM clause insertion for unique columns
    if tables.is_empty() && ctes.is_empty() {
        return suggest_columns_with_from_insertion(schema);
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
            items.push(CompletionItem {
                label: col.clone(),
                insert_text: quote_identifier_if_needed(&col),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(format!("from {}", sources[0])),
                ..Default::default()
            });
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
                items.push(CompletionItem {
                    label: qualified,
                    insert_text: insert,
                    kind: Some(CompletionItemKind::FIELD),
                    ..Default::default()
                });
            }
        }
    }

    items
}

/// Suggest columns with automatic FROM clause insertion for SELECT without FROM
fn suggest_columns_with_from_insertion(schema: &Schema) -> Vec<CompletionItem> {
    // Track which tables contain each column
    let mut column_to_tables: HashMap<String, Vec<String>> = HashMap::new();

    for table_name in schema.table_names() {
        if let Some(cols) = schema.columns_for_table_with_rowid(table_name) {
            for col in cols {
                column_to_tables
                    .entry(col)
                    .or_default()
                    .push(table_name.to_string());
            }
        }
    }

    let mut items = Vec::new();
    for (col, tables) in column_to_tables {
        let quoted_col = quote_identifier_if_needed(&col);
        let col_text = quoted_col.as_ref().unwrap_or(&col);

        if tables.len() == 1 {
            // Unique column - insert "column, $1 FROM table" as snippet
            let table = &tables[0];
            let quoted_table = quote_identifier_if_needed(table);
            let table_text = quoted_table.as_ref().unwrap_or(table);

            items.push(CompletionItem {
                label: col.clone(),
                insert_text: Some(format!("{}, $1 FROM {}", col_text, table_text)),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(format!("from {}", table)),
                ..Default::default()
            });
        } else {
            // Ambiguous column - just insert column name, list all tables in detail
            let tables_str = tables.join(", ");
            items.push(CompletionItem {
                label: col.clone(),
                insert_text: quoted_col,
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(format!("from: {}", tables_str)),
                ..Default::default()
            });
        }
    }

    items
}

// ============================================================================
// Keyword Completion Helpers
// ============================================================================

/// Keywords available at the start of a statement.
fn statement_start_keywords(include_docs: bool) -> Vec<CompletionItem> {
    let keywords: Vec<(&str, &str, &str)> = vec![
        ("select", "Query data", "select columns from table [where condition]"),
        ("insert", "Insert data", "insert into table (columns) values (values)"),
        ("update", "Update data", "update table set column = value [where condition]"),
        ("delete", "Delete data", "delete from table [where condition]"),
        ("create", "Create database objects", "create table | index | view | trigger ..."),
        ("drop", "Drop database objects", "drop table | index | view | trigger ..."),
        ("alter", "Alter database objects", "alter table table_name add | drop | rename ..."),
        ("replace", "Replace data (insert or replace)", "replace into table (columns) values (values)"),
        ("begin", "Start a transaction", "begin [deferred | immediate | exclusive] [transaction]"),
        ("commit", "Commit a transaction", "commit [transaction]"),
        ("rollback", "Rollback a transaction", "rollback [transaction] [to savepoint savepoint_name]"),
        ("savepoint", "Create a savepoint", "savepoint savepoint_name"),
        ("release", "Release a savepoint", "release [savepoint] savepoint_name"),
        ("vacuum", "Rebuild the database", "vacuum [schema_name] [into filename]"),
        ("analyze", "Gather statistics", "analyze [schema_name | table_or_index_name]"),
        ("reindex", "Rebuild indexes", "reindex [collation_name | table_name | index_name]"),
        ("attach", "Attach a database", "attach database filename as schema_name"),
        ("detach", "Detach a database", "detach database schema_name"),
        ("pragma", "Query or set pragmas", "pragma pragma_name [= value]"),
        ("explain", "Explain query plan", "explain [query plan] sql_statement"),
        ("with", "Common Table Expression", "with [recursive] cte_name as (select_stmt) ..."),
    ];

    keywords
        .into_iter()
        .enumerate()
        .map(|(i, (label, detail, doc))| {
            let mut item = CompletionItem {
                label: label.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                sort_text: Some(format!("{:02}", i)),
                ..Default::default()
            };
            if include_docs {
                item.detail = Some(detail.to_string());
                item.documentation = Some(Documentation::String(doc.to_string()));
            }
            item
        })
        .collect()
}

/// Keywords available after CREATE.
fn after_create_keywords(include_docs: bool) -> Vec<CompletionItem> {
    let keywords: Vec<(&str, &str, &str)> = vec![
        ("table", "Create a new table", "create table table_name (column_definitions)"),
        ("index", "Create a new index", "create index index_name on table_name (columns)"),
        ("unique index", "Create a unique index", "create unique index index_name on table_name (columns)"),
        ("view", "Create a new view", "create view view_name as select_stmt"),
        ("trigger", "Create a new trigger", "create trigger trigger_name before|after|instead of ..."),
        ("virtual table", "Create a virtual table", "create virtual table table_name using module(args)"),
    ];

    keywords
        .into_iter()
        .map(|(label, detail, doc)| {
            let mut item = CompletionItem {
                label: label.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            };
            if include_docs {
                item.detail = Some(detail.to_string());
                item.documentation = Some(Documentation::String(doc.to_string()));
            }
            item
        })
        .collect()
}

/// Keywords available after CREATE TABLE.
fn after_create_table_keywords(include_docs: bool) -> Vec<CompletionItem> {
    let mut item = CompletionItem {
        label: "if not exists".to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        ..Default::default()
    };
    if include_docs {
        item.detail = Some("Only create if table doesn't exist".to_string());
        item.documentation = Some(Documentation::String(
            "create table if not exists table_name (...)".to_string(),
        ));
    }
    vec![item]
}

/// Keywords available after INSERT.
fn after_insert_keywords(include_docs: bool) -> Vec<CompletionItem> {
    let keywords: Vec<(&str, &str, &str)> = vec![
        ("into", "Insert into table", "insert into table_name (columns) values (...)"),
        ("or abort", "Abort on conflict", "insert or abort into table_name ..."),
        ("or fail", "Fail on conflict", "insert or fail into table_name ..."),
        ("or ignore", "Ignore on conflict", "insert or ignore into table_name ..."),
        ("or replace", "Replace on conflict", "insert or replace into table_name ..."),
        ("or rollback", "Rollback on conflict", "insert or rollback into table_name ..."),
    ];

    keywords
        .into_iter()
        .map(|(label, detail, doc)| {
            let mut item = CompletionItem {
                label: label.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            };
            if include_docs {
                item.detail = Some(detail.to_string());
                item.documentation = Some(Documentation::String(doc.to_string()));
            }
            item
        })
        .collect()
}

/// Keywords available after REPLACE.
fn after_replace_keywords(include_docs: bool) -> Vec<CompletionItem> {
    let mut item = CompletionItem {
        label: "into".to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        ..Default::default()
    };
    if include_docs {
        item.detail = Some("Replace into table".to_string());
        item.documentation = Some(Documentation::String(
            "replace into table_name (columns) values (...)".to_string(),
        ));
    }
    vec![item]
}

/// Keywords available after DROP.
fn after_drop_keywords(include_docs: bool) -> Vec<CompletionItem> {
    let keywords: Vec<(&str, &str, &str)> = vec![
        ("table", "Drop a table", "drop table [if exists] table_name"),
        ("index", "Drop an index", "drop index [if exists] index_name"),
        ("view", "Drop a view", "drop view [if exists] view_name"),
        ("trigger", "Drop a trigger", "drop trigger [if exists] trigger_name"),
    ];

    keywords
        .into_iter()
        .map(|(label, detail, doc)| {
            let mut item = CompletionItem {
                label: label.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            };
            if include_docs {
                item.detail = Some(detail.to_string());
                item.documentation = Some(Documentation::String(doc.to_string()));
            }
            item
        })
        .collect()
}

/// Keywords available after ALTER.
fn after_alter_keywords(include_docs: bool) -> Vec<CompletionItem> {
    let mut item = CompletionItem {
        label: "table".to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        ..Default::default()
    };
    if include_docs {
        item.detail = Some("Alter a table".to_string());
        item.documentation = Some(Documentation::String(
            "alter table table_name add | drop | rename ...".to_string(),
        ));
    }
    vec![item]
}

/// Keywords available for ALTER TABLE actions.
fn alter_table_action_keywords(include_docs: bool) -> Vec<CompletionItem> {
    let keywords: Vec<(&str, &str, &str)> = vec![
        ("add", "Add a new column", "alter table table_name add column_def"),
        ("add column", "Add a new column", "alter table table_name add column column_def"),
        ("drop column", "Remove a column", "alter table table_name drop column column_name"),
        ("rename to", "Rename the table", "alter table table_name rename to new_table_name"),
        ("rename column", "Rename a column", "alter table table_name rename column old_name to new_name"),
    ];

    keywords
        .into_iter()
        .map(|(label, detail, doc)| {
            let mut item = CompletionItem {
                label: label.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            };
            if include_docs {
                item.detail = Some(detail.to_string());
                item.documentation = Some(Documentation::String(doc.to_string()));
            }
            item
        })
        .collect()
}
