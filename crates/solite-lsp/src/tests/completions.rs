//! Completion tests for the LSP.
//!
//! Tests for various completion contexts including DDL statements,
//! alias resolution, DML statements, keyword completions, and rowid support.

use super::*;

// ========================================
// Phase 5: DDL Completion Tests
// ========================================

#[test]
fn test_drop_table_completion() {
    let schema = build_test_schema("CREATE TABLE users (id); CREATE TABLE orders (id);");
    let completions = get_completions_at_end("DROP TABLE ", &schema);

    assert!(completions.iter().any(|c| c.label == "users"));
    assert!(completions.iter().any(|c| c.label == "orders"));
}

#[test]
fn test_drop_table_if_exists_completion() {
    let schema = build_test_schema("CREATE TABLE products (sku);");
    let completions = get_completions_at_end("DROP TABLE IF EXISTS ", &schema);

    assert!(completions.iter().any(|c| c.label == "products"));
}

#[test]
fn test_drop_index_completion() {
    let schema = build_test_schema(
        "CREATE TABLE users (id); CREATE INDEX idx_users ON users(id);"
    );
    let completions = get_completions_at_end("DROP INDEX ", &schema);

    assert!(completions.iter().any(|c| c.label == "idx_users"));
}

#[test]
fn test_drop_index_multiple_indexes() {
    let schema = build_test_schema(
        "CREATE TABLE t (a, b); \
         CREATE INDEX idx_a ON t(a); \
         CREATE UNIQUE INDEX idx_b ON t(b);"
    );
    let completions = get_completions_at_end("DROP INDEX ", &schema);

    assert!(completions.iter().any(|c| c.label == "idx_a"));
    assert!(completions.iter().any(|c| c.label == "idx_b"));
    assert_eq!(completions.len(), 2);
}

#[test]
fn test_drop_view_completion() {
    let schema = build_test_schema(
        "CREATE TABLE users (id, name); CREATE VIEW v_users AS SELECT id, name FROM users;"
    );
    let completions = get_completions_at_end("DROP VIEW ", &schema);

    assert!(completions.iter().any(|c| c.label == "v_users"));
}

#[test]
fn test_drop_view_multiple_views() {
    let schema = build_test_schema(
        "CREATE TABLE t (a); \
         CREATE VIEW view_a AS SELECT a FROM t; \
         CREATE VIEW view_b AS SELECT a FROM t;"
    );
    let completions = get_completions_at_end("DROP VIEW ", &schema);

    assert!(completions.iter().any(|c| c.label == "view_a"));
    assert!(completions.iter().any(|c| c.label == "view_b"));
    assert_eq!(completions.len(), 2);
}

#[test]
fn test_alter_table_drop_column() {
    let schema = build_test_schema("CREATE TABLE users (id, name, email);");
    let completions = get_completions_at_end("ALTER TABLE users DROP COLUMN ", &schema);

    assert!(completions.iter().any(|c| c.label == "id"));
    assert!(completions.iter().any(|c| c.label == "name"));
    assert!(completions.iter().any(|c| c.label == "email"));
    assert_eq!(completions.len(), 3);
}

#[test]
fn test_alter_table_action_keywords() {
    let schema = build_test_schema("CREATE TABLE users (id);");
    let completions = get_completions_at_end("ALTER TABLE users ", &schema);

    assert!(completions.iter().any(|c| c.label == "add"));
    assert!(completions.iter().any(|c| c.label == "add column"));
    assert!(completions.iter().any(|c| c.label == "drop column"));
    assert!(completions.iter().any(|c| c.label == "rename to"));
    assert!(completions.iter().any(|c| c.label == "rename column"));
}

#[test]
fn test_create_index_on_completion() {
    let schema = build_test_schema("CREATE TABLE users (id, name); CREATE TABLE orders (id);");
    let completions = get_completions_at_end("CREATE INDEX idx ON ", &schema);

    assert!(completions.iter().any(|c| c.label == "users"));
    assert!(completions.iter().any(|c| c.label == "orders"));
}

#[test]
fn test_create_unique_index_on_completion() {
    let schema = build_test_schema("CREATE TABLE products (sku);");
    let completions = get_completions_at_end("CREATE UNIQUE INDEX idx_sku ON ", &schema);

    assert!(completions.iter().any(|c| c.label == "products"));
}

#[test]
fn test_create_index_columns_completion() {
    let schema = build_test_schema("CREATE TABLE users (id, name, email);");
    let completions = get_completions_at_end("CREATE INDEX idx ON users(", &schema);

    assert!(completions.iter().any(|c| c.label == "id"));
    assert!(completions.iter().any(|c| c.label == "name"));
    assert!(completions.iter().any(|c| c.label == "email"));
}

#[test]
fn test_create_index_columns_comma() {
    let schema = build_test_schema("CREATE TABLE users (id, name, email);");
    let completions = get_completions_at_end("CREATE INDEX idx ON users(id, ", &schema);

    // Should still suggest all columns (filtering used columns is Phase 4 enhancement)
    assert!(completions.iter().any(|c| c.label == "id"));
    assert!(completions.iter().any(|c| c.label == "name"));
    assert!(completions.iter().any(|c| c.label == "email"));
}

#[test]
fn test_alter_table_name_completion() {
    let schema = build_test_schema("CREATE TABLE users (id);");
    let completions = get_completions_at_end("ALTER TABLE ", &schema);

    assert!(completions.iter().any(|c| c.label == "users"));
}

#[test]
fn test_drop_index_empty_schema() {
    let schema = build_test_schema("CREATE TABLE t (a);");
    let completions = get_completions_at_end("DROP INDEX ", &schema);

    assert!(completions.is_empty());
}

#[test]
fn test_drop_view_empty_schema() {
    let schema = build_test_schema("CREATE TABLE t (a);");
    let completions = get_completions_at_end("DROP VIEW ", &schema);

    assert!(completions.is_empty());
}

#[test]
fn test_create_index_columns_case_preserved() {
    let schema = build_test_schema("CREATE TABLE Users (ID, Name, Email);");
    let completions = get_completions_at_end("CREATE INDEX idx ON users(", &schema);

    // Column names should preserve original case
    assert!(completions.iter().any(|c| c.label == "ID"));
    assert!(completions.iter().any(|c| c.label == "Name"));
    assert!(completions.iter().any(|c| c.label == "Email"));
}

// ========================================
// Phase 3: Alias Resolution Tests
// ========================================

#[test]
fn test_qualified_completion_with_alias() {
    // When user types "u." in WHERE clause, suggest columns from users table (which u aliases)
    let schema = build_test_schema("CREATE TABLE users (id, name, email);");
    let sql = "SELECT * FROM users AS u WHERE u.";
    let ctx = detect_context(sql, sql.len()); // Position after "u."

    let completions = get_completions_for_context(&ctx, Some(&schema), None);
    assert!(completions.iter().any(|c| c.label == "id"), "Should suggest id");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "email"), "Should suggest email");
    assert!(completions.iter().any(|c| c.label == "rowid"), "Should suggest rowid");
    assert_eq!(completions.len(), 4);
}

#[test]
fn test_ambiguous_columns_qualified() {
    // When multiple tables have the same column, suggest qualified names
    let schema = build_test_schema(
        "CREATE TABLE users (id, name); CREATE TABLE orders (id, user_id);"
    );
    // Cursor in WHERE clause, so tables are in scope
    let sql = "SELECT * FROM users u JOIN orders o ON u.id = o.user_id WHERE ";
    let ctx = detect_context(sql, sql.len()); // Position after "WHERE "

    let completions = get_completions_for_context(&ctx, Some(&schema), None);

    // 'id' is ambiguous - should suggest qualified versions
    assert!(completions.iter().any(|c| c.label == "u.id"), "Should suggest u.id");
    assert!(completions.iter().any(|c| c.label == "o.id"), "Should suggest o.id");

    // 'name' is unique to users - can be unqualified
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");

    // 'user_id' is unique to orders - can be unqualified
    assert!(completions.iter().any(|c| c.label == "user_id"), "Should suggest user_id");
}

#[test]
fn test_implicit_alias_completion() {
    // Table name itself can be used as a qualifier (table name without alias)
    let schema = build_test_schema("CREATE TABLE users (id, name, email);");
    let sql = "SELECT * FROM users WHERE users.";
    let ctx = detect_context(sql, sql.len()); // Position after "users."

    let completions = get_completions_for_context(&ctx, Some(&schema), None);
    assert!(completions.iter().any(|c| c.label == "id"), "Should suggest id");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "email"), "Should suggest email");
    assert!(completions.iter().any(|c| c.label == "rowid"), "Should suggest rowid");
    assert_eq!(completions.len(), 4);
}

#[test]
fn test_qualified_completion_in_where_clause() {
    // Qualified completion should work in WHERE clause too
    let schema = build_test_schema("CREATE TABLE users (id, name); CREATE TABLE orders (id, total);");
    let sql = "SELECT * FROM users u JOIN orders o ON u.id = o.id WHERE u.";
    let ctx = detect_context(sql, sql.len()); // Position at end after "u."

    let completions = get_completions_for_context(&ctx, Some(&schema), None);
    assert!(completions.iter().any(|c| c.label == "id"), "Should suggest id");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "rowid"), "Should suggest rowid");
    assert_eq!(completions.len(), 3);
}

#[test]
fn test_qualified_completion_case_insensitive() {
    // Alias matching should be case-insensitive
    let schema = build_test_schema("CREATE TABLE Users (ID, Name);");
    let sql = "SELECT * FROM Users AS u WHERE U.";
    let ctx = detect_context(sql, sql.len()); // Position after "U."

    let completions = get_completions_for_context(&ctx, Some(&schema), None);
    // Should find columns from Users table (U matches u case-insensitively)
    assert!(completions.iter().any(|c| c.label == "ID"), "Should suggest ID");
    assert!(completions.iter().any(|c| c.label == "Name"), "Should suggest Name");
}

// ========================================
// Phase 4: DML Statement Support Tests
// ========================================

/// Helper to get completions with INSERT column filtering
fn get_insert_completions(sql: &str, schema: &Schema) -> Vec<CompletionItem> {
    let offset = sql.len();
    let ctx = detect_context(sql, offset);

    if let CompletionContext::InsertColumns { ref table_name } = ctx {
        let used_columns = extract_used_insert_columns(sql, offset);
        schema
            .columns_for_table(table_name)
            .map(|cols| {
                cols.iter()
                    .filter(|col| !used_columns.contains(&col.to_lowercase()))
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
        panic!("Expected InsertColumns context, got {:?}", ctx);
    }
}

#[test]
fn test_insert_column_completion() {
    let schema = build_test_schema("CREATE TABLE users (id, name, email);");
    let completions = get_insert_completions("INSERT INTO users (", &schema);

    assert!(completions.iter().any(|c| c.label == "id"), "Should suggest id");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "email"), "Should suggest email");
    assert_eq!(completions.len(), 3);
}

#[test]
fn test_insert_remaining_columns() {
    // 'id' already used, should NOT suggest it again
    let schema = build_test_schema("CREATE TABLE users (id, name, email);");
    let completions = get_insert_completions("INSERT INTO users (id, ", &schema);

    assert!(!completions.iter().any(|c| c.label == "id"), "Should NOT suggest id (already used)");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "email"), "Should suggest email");
    assert_eq!(completions.len(), 2);
}

#[test]
fn test_insert_remaining_columns_multiple() {
    // 'id' and 'name' already used
    let schema = build_test_schema("CREATE TABLE users (id, name, email);");
    let completions = get_insert_completions("INSERT INTO users (id, name, ", &schema);

    assert!(!completions.iter().any(|c| c.label == "id"), "Should NOT suggest id");
    assert!(!completions.iter().any(|c| c.label == "name"), "Should NOT suggest name");
    assert!(completions.iter().any(|c| c.label == "email"), "Should suggest email");
    assert_eq!(completions.len(), 1);
}

#[test]
fn test_insert_columns_case_insensitive() {
    // Column filtering should be case-insensitive
    let schema = build_test_schema("CREATE TABLE users (ID, Name, Email);");
    let completions = get_insert_completions("INSERT INTO users (id, ", &schema);

    // 'id' matches 'ID' case-insensitively, so ID should not be suggested
    assert!(!completions.iter().any(|c| c.label == "ID"), "Should NOT suggest ID (id already used)");
    assert!(completions.iter().any(|c| c.label == "Name"), "Should suggest Name");
    assert!(completions.iter().any(|c| c.label == "Email"), "Should suggest Email");
    assert_eq!(completions.len(), 2);
}

#[test]
fn test_update_set_completion() {
    let schema = build_test_schema("CREATE TABLE users (id, name, email);");
    let completions = get_completions_at_end("UPDATE users SET ", &schema);

    assert!(completions.iter().any(|c| c.label == "id"), "Should suggest id");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "email"), "Should suggest email");
    assert!(completions.iter().any(|c| c.label == "rowid"), "Should suggest rowid");
    assert_eq!(completions.len(), 4);
}

#[test]
fn test_update_set_completion_comma() {
    // After setting one column, still suggest all columns for next assignment
    let schema = build_test_schema("CREATE TABLE users (id, name, email);");
    let completions = get_completions_at_end("UPDATE users SET name = 'test', ", &schema);

    // Note: UPDATE SET doesn't filter used columns like INSERT does
    // because you might want to SET the same column based on different conditions
    assert!(completions.iter().any(|c| c.label == "id"), "Should suggest id");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "email"), "Should suggest email");
}

#[test]
fn test_delete_where_completion() {
    let schema = build_test_schema("CREATE TABLE users (id, name);");
    let completions = get_completions_at_end("DELETE FROM users WHERE ", &schema);

    assert!(completions.iter().any(|c| c.label == "id"), "Should suggest id");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "rowid"), "Should suggest rowid");
    assert_eq!(completions.len(), 3);
}

#[test]
fn test_delete_where_completion_with_expression() {
    // Should continue to suggest columns after first expression
    let schema = build_test_schema("CREATE TABLE users (id, name, active);");
    let completions = get_completions_at_end("DELETE FROM users WHERE active = 1 AND ", &schema);

    assert!(completions.iter().any(|c| c.label == "id"), "Should suggest id");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "active"), "Should suggest active");
}

#[test]
fn test_insert_into_table_completion() {
    // INSERT INTO should suggest table names
    let schema = build_test_schema("CREATE TABLE users (id); CREATE TABLE orders (id);");
    let completions = get_completions_at_end("INSERT INTO ", &schema);

    assert!(completions.iter().any(|c| c.label == "users"), "Should suggest users");
    assert!(completions.iter().any(|c| c.label == "orders"), "Should suggest orders");
}

#[test]
fn test_update_table_completion() {
    // UPDATE should suggest table names
    let schema = build_test_schema("CREATE TABLE users (id); CREATE TABLE orders (id);");
    let completions = get_completions_at_end("UPDATE ", &schema);

    assert!(completions.iter().any(|c| c.label == "users"), "Should suggest users");
    assert!(completions.iter().any(|c| c.label == "orders"), "Should suggest orders");
}

#[test]
fn test_quoted_identifier_completion() {
    // Identifiers with spaces should have insert_text with quotes
    let schema = build_test_schema("CREATE TABLE \"Game Settings\" (\"user ID\" INT, \"Auto save\" BOOL);");
    let completions = get_completions_at_end("SELECT * FROM ", &schema);

    let game_settings = completions.iter().find(|c| c.label == "Game Settings");
    assert!(game_settings.is_some(), "Should suggest 'Game Settings'");
    assert_eq!(
        game_settings.unwrap().insert_text.as_deref(),
        Some("\"Game Settings\""),
        "Should quote table name with space"
    );

    // Column completions - need to use exact table name as stored in schema
    let col_completions = get_completions_at_end("SELECT * FROM \"Game Settings\" WHERE ", &schema);
    let user_id = col_completions.iter().find(|c| c.label == "user ID");
    assert!(user_id.is_some(), "Should suggest 'user ID'");
    assert_eq!(
        user_id.unwrap().insert_text.as_deref(),
        Some("\"user ID\""),
        "Should quote column name with space"
    );
}

#[test]
fn test_normal_identifier_no_insert_text() {
    // Normal identifiers without special chars should have no insert_text
    let schema = build_test_schema("CREATE TABLE users (id INT, name TEXT);");
    let completions = get_completions_at_end("SELECT * FROM ", &schema);

    let users = completions.iter().find(|c| c.label == "users");
    assert!(users.is_some(), "Should suggest 'users'");
    assert!(
        users.unwrap().insert_text.is_none(),
        "Normal identifier should not have insert_text"
    );
}

// ========================================
// Phase 6: Keyword Completions Tests
// ========================================

use crate::completions::{get_completions_extended, CompletionOptions};

#[test]
fn test_statement_start_keywords() {
    // At the start of a statement, should suggest all statement keywords
    let ctx = CompletionContext::StatementStart { prefix: Some("".to_string()) };
    let options = CompletionOptions {
        document_text: None,
        cursor_offset: None,
        include_documentation: true,
        prefix: None,
    };
    let keywords = get_completions_extended(&ctx, None, &options);
    let labels: Vec<&str> = keywords.iter().map(|k| k.label.as_str()).collect();

    // DML keywords
    assert!(labels.contains(&"select"), "Should suggest select");
    assert!(labels.contains(&"insert"), "Should suggest insert");
    assert!(labels.contains(&"update"), "Should suggest update");
    assert!(labels.contains(&"delete"), "Should suggest delete");

    // DDL keywords
    assert!(labels.contains(&"create"), "Should suggest create");
    assert!(labels.contains(&"drop"), "Should suggest drop");
    assert!(labels.contains(&"alter"), "Should suggest alter");
    assert!(labels.contains(&"replace"), "Should suggest replace");

    // TCL keywords
    assert!(labels.contains(&"begin"), "Should suggest begin");
    assert!(labels.contains(&"commit"), "Should suggest commit");
    assert!(labels.contains(&"rollback"), "Should suggest rollback");
    assert!(labels.contains(&"savepoint"), "Should suggest savepoint");
    assert!(labels.contains(&"release"), "Should suggest release");

    // Other keywords
    assert!(labels.contains(&"vacuum"), "Should suggest vacuum");
    assert!(labels.contains(&"analyze"), "Should suggest analyze");
    assert!(labels.contains(&"reindex"), "Should suggest reindex");
    assert!(labels.contains(&"attach"), "Should suggest attach");
    assert!(labels.contains(&"detach"), "Should suggest detach");
    assert!(labels.contains(&"pragma"), "Should suggest pragma");
    assert!(labels.contains(&"explain"), "Should suggest explain");
    assert!(labels.contains(&"with"), "Should suggest with");

    // All should be KEYWORD type
    assert!(
        keywords.iter().all(|k| k.kind == Some(CompletionItemKind::KEYWORD)),
        "All should be KEYWORD type"
    );

    // All should have documentation when include_documentation is true
    assert!(
        keywords.iter().all(|k| k.documentation.is_some()),
        "All should have documentation"
    );
}

#[test]
fn test_after_create_keywords() {
    // After CREATE, should suggest object types
    let ctx = CompletionContext::AfterCreate;
    let options = CompletionOptions {
        document_text: None,
        cursor_offset: None,
        include_documentation: true,
        prefix: None,
    };
    let keywords = get_completions_extended(&ctx, None, &options);
    let labels: Vec<&str> = keywords.iter().map(|k| k.label.as_str()).collect();

    assert!(labels.contains(&"table"), "Should suggest table");
    assert!(labels.contains(&"index"), "Should suggest index");
    assert!(labels.contains(&"unique index"), "Should suggest unique index");
    assert!(labels.contains(&"view"), "Should suggest view");
    assert!(labels.contains(&"trigger"), "Should suggest trigger");
    assert!(labels.contains(&"virtual table"), "Should suggest virtual table");

    // All should be KEYWORD type
    assert!(
        keywords.iter().all(|k| k.kind == Some(CompletionItemKind::KEYWORD)),
        "All should be KEYWORD type"
    );

    // All should have detail and documentation
    assert!(
        keywords.iter().all(|k| k.detail.is_some() && k.documentation.is_some()),
        "All should have detail and documentation"
    );
}

#[test]
fn test_after_drop_keywords() {
    // After DROP, should suggest droppable object types
    let ctx = CompletionContext::AfterDrop;
    let options = CompletionOptions {
        document_text: None,
        cursor_offset: None,
        include_documentation: true,
        prefix: None,
    };
    let keywords = get_completions_extended(&ctx, None, &options);
    let labels: Vec<&str> = keywords.iter().map(|k| k.label.as_str()).collect();

    assert!(labels.contains(&"table"), "Should suggest table");
    assert!(labels.contains(&"index"), "Should suggest index");
    assert!(labels.contains(&"view"), "Should suggest view");
    assert!(labels.contains(&"trigger"), "Should suggest trigger");

    // Should not suggest virtual table (it's table to drop)
    assert!(!labels.contains(&"virtual table"), "Should not suggest virtual table");

    // All should be KEYWORD type
    assert!(
        keywords.iter().all(|k| k.kind == Some(CompletionItemKind::KEYWORD)),
        "All should be KEYWORD type"
    );
}

#[test]
fn test_after_alter_keywords() {
    // After ALTER, should only suggest TABLE (only alterable object in SQLite)
    let ctx = CompletionContext::AfterAlter;
    let options = CompletionOptions {
        document_text: None,
        cursor_offset: None,
        include_documentation: true,
        prefix: None,
    };
    let keywords = get_completions_extended(&ctx, None, &options);
    let labels: Vec<&str> = keywords.iter().map(|k| k.label.as_str()).collect();

    assert_eq!(labels.len(), 1, "Should only suggest one option");
    assert!(labels.contains(&"table"), "Should suggest table");
}

#[test]
fn test_alter_table_action_keywords_complete() {
    // After ALTER TABLE name, should suggest all action keywords
    let ctx = CompletionContext::AlterTableAction { table_name: "t".to_string() };
    let options = CompletionOptions {
        document_text: None,
        cursor_offset: None,
        include_documentation: true,
        prefix: None,
    };
    let keywords = get_completions_extended(&ctx, None, &options);
    let labels: Vec<&str> = keywords.iter().map(|k| k.label.as_str()).collect();

    assert!(labels.contains(&"add"), "Should suggest add");
    assert!(labels.contains(&"add column"), "Should suggest add column");
    assert!(labels.contains(&"drop column"), "Should suggest drop column");
    assert!(labels.contains(&"rename to"), "Should suggest rename to");
    assert!(labels.contains(&"rename column"), "Should suggest rename column");

    // All should be KEYWORD type
    assert!(
        keywords.iter().all(|k| k.kind == Some(CompletionItemKind::KEYWORD)),
        "All should be KEYWORD type"
    );

    // All should have detail and documentation
    assert!(
        keywords.iter().all(|k| k.detail.is_some() && k.documentation.is_some()),
        "All should have detail and documentation"
    );
}

#[test]
fn test_keyword_completions_have_documentation() {
    // Verify keyword completions return items with documentation when include_documentation is true
    let options = CompletionOptions {
        document_text: None,
        cursor_offset: None,
        include_documentation: true,
        prefix: None,
    };

    let contexts = [
        CompletionContext::StatementStart { prefix: Some("".to_string()) },
        CompletionContext::AfterCreate,
        CompletionContext::AfterDrop,
        CompletionContext::AfterAlter,
        CompletionContext::AlterTableAction { table_name: "t".to_string() },
    ];

    for ctx in contexts {
        let keywords = get_completions_extended(&ctx, None, &options);
        for keyword in keywords {
            assert!(
                keyword.documentation.is_some(),
                "Keyword {} should have documentation",
                keyword.label
            );
            assert!(
                keyword.detail.is_some(),
                "Keyword {} should have detail",
                keyword.label
            );
        }
    }
}

#[test]
fn test_statement_start_context_detection() {
    // Verify context detection works for statement start
    let ctx = detect_context("", 0);
    assert!(
        matches!(ctx, CompletionContext::StatementStart { prefix: None }),
        "Empty string should be StatementStart with no prefix"
    );

    let ctx = detect_context("   ", 3);
    assert!(
        matches!(ctx, CompletionContext::StatementStart { prefix: None }),
        "Whitespace only should be StatementStart with no prefix"
    );

    // Typing a partial keyword should give a prefix
    let ctx = detect_context("s", 1);
    assert!(
        matches!(ctx, CompletionContext::StatementStart { prefix: Some(ref p) } if p == "s"),
        "Partial 's' should be StatementStart with prefix 's'"
    );
}

#[test]
fn test_after_create_context_detection() {
    let ctx = detect_context("CREATE ", 7);
    assert!(
        matches!(ctx, CompletionContext::AfterCreate),
        "After CREATE should be AfterCreate context"
    );
}

#[test]
fn test_after_drop_context_detection() {
    let ctx = detect_context("DROP ", 5);
    assert!(
        matches!(ctx, CompletionContext::AfterDrop),
        "After DROP should be AfterDrop context"
    );
}

#[test]
fn test_after_alter_context_detection() {
    let ctx = detect_context("ALTER ", 6);
    assert!(
        matches!(ctx, CompletionContext::AfterAlter),
        "After ALTER should be AfterAlter context"
    );
}

// ========================================
// Rowid Column Support Tests
// ========================================

#[test]
fn test_rowid_included_for_regular_table() {
    // Regular tables (without WITHOUT ROWID) should include rowid in completions
    let schema = build_test_schema("CREATE TABLE users (id, name);");
    let completions = get_completions_at_end("SELECT * FROM users WHERE ", &schema);

    assert!(completions.iter().any(|c| c.label == "id"), "Should suggest id");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "rowid"), "Should suggest rowid for regular table");
}

#[test]
fn test_rowid_excluded_for_without_rowid_table() {
    // WITHOUT ROWID tables should NOT include rowid in completions
    let schema = build_test_schema("CREATE TABLE kv (k TEXT PRIMARY KEY, v TEXT) WITHOUT ROWID;");
    let completions = get_completions_at_end("SELECT * FROM kv WHERE ", &schema);

    assert!(completions.iter().any(|c| c.label == "k"), "Should suggest k");
    assert!(completions.iter().any(|c| c.label == "v"), "Should suggest v");
    assert!(!completions.iter().any(|c| c.label == "rowid"), "Should NOT suggest rowid for WITHOUT ROWID table");
}

#[test]
fn test_rowid_in_insert_regular_table() {
    // Regular table INSERT should include rowid
    let schema = build_test_schema("CREATE TABLE users (id, name);");
    let completions = get_completions_at_end("INSERT INTO users (", &schema);

    assert!(completions.iter().any(|c| c.label == "id"), "Should suggest id");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "rowid"), "Should suggest rowid for INSERT");
}

#[test]
fn test_rowid_not_in_insert_without_rowid_table() {
    // WITHOUT ROWID table INSERT should NOT include rowid
    let schema = build_test_schema("CREATE TABLE kv (k TEXT PRIMARY KEY, v TEXT) WITHOUT ROWID;");
    let completions = get_completions_at_end("INSERT INTO kv (", &schema);

    assert!(completions.iter().any(|c| c.label == "k"), "Should suggest k");
    assert!(completions.iter().any(|c| c.label == "v"), "Should suggest v");
    assert!(!completions.iter().any(|c| c.label == "rowid"), "Should NOT suggest rowid for WITHOUT ROWID table");
}

#[test]
fn test_rowid_not_in_alter_table() {
    // ALTER TABLE DROP COLUMN should NOT include rowid (can't drop implicit rowid)
    let schema = build_test_schema("CREATE TABLE users (id, name, email);");
    let completions = get_completions_at_end("ALTER TABLE users DROP COLUMN ", &schema);

    assert!(completions.iter().any(|c| c.label == "id"), "Should suggest id");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "email"), "Should suggest email");
    assert!(!completions.iter().any(|c| c.label == "rowid"), "Should NOT suggest rowid for ALTER TABLE");
    assert_eq!(completions.len(), 3, "Should only have 3 columns, no rowid");
}

#[test]
fn test_rowid_in_create_index() {
    // CREATE INDEX should include rowid for regular tables
    let schema = build_test_schema("CREATE TABLE users (id, name);");
    let completions = get_completions_at_end("CREATE INDEX idx ON users(", &schema);

    assert!(completions.iter().any(|c| c.label == "id"), "Should suggest id");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "rowid"), "Should suggest rowid for CREATE INDEX");
}

#[test]
fn test_rowid_qualified_column() {
    // Qualified column completion (table.column) should include rowid
    let schema = build_test_schema("CREATE TABLE users (id, name);");
    let sql = "SELECT * FROM users WHERE users.";
    let ctx = detect_context(sql, sql.len());

    let completions = get_completions_for_context(&ctx, Some(&schema), None);
    assert!(completions.iter().any(|c| c.label == "id"), "Should suggest id");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "rowid"), "Should suggest rowid for qualified column");
}

#[test]
fn test_strict_table_still_has_rowid() {
    // STRICT tables (without WITHOUT ROWID) should still have rowid
    let schema = build_test_schema("CREATE TABLE users (id INTEGER, name TEXT) STRICT;");
    let completions = get_completions_at_end("SELECT * FROM users WHERE ", &schema);

    assert!(completions.iter().any(|c| c.label == "id"), "Should suggest id");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "rowid"), "Should suggest rowid for STRICT table");
}

#[test]
fn test_strict_without_rowid_no_rowid() {
    // STRICT WITHOUT ROWID tables should NOT have rowid
    let schema = build_test_schema("CREATE TABLE kv (k TEXT PRIMARY KEY, v TEXT) STRICT, WITHOUT ROWID;");
    let completions = get_completions_at_end("SELECT * FROM kv WHERE ", &schema);

    assert!(completions.iter().any(|c| c.label == "k"), "Should suggest k");
    assert!(completions.iter().any(|c| c.label == "v"), "Should suggest v");
    assert!(!completions.iter().any(|c| c.label == "rowid"), "Should NOT suggest rowid for STRICT WITHOUT ROWID table");
}

// ========================================
// SELECT Column Deduplication Tests
// ========================================

#[test]
fn test_select_column_filtering() {
    let schema = build_test_schema("CREATE TABLE t (id, name, email);");
    let completions = get_completions_with_text("SELECT id, ", &schema);

    assert!(!completions.iter().any(|c| c.label == "id"), "Should NOT suggest id (already used)");
    assert!(completions.iter().any(|c| c.label == "name"), "Should suggest name");
    assert!(completions.iter().any(|c| c.label == "email"), "Should suggest email");
}

#[test]
fn test_select_column_filtering_with_star() {
    let schema = build_test_schema("CREATE TABLE t (id, name, email);");
    let completions = get_completions_with_text("SELECT *, name, ", &schema);

    // All columns already selected via *
    assert!(!completions.iter().any(|c| c.label == "id"), "Should NOT suggest id (* already selects all)");
    assert!(!completions.iter().any(|c| c.label == "name"), "Should NOT suggest name (* already selects all)");
    assert!(!completions.iter().any(|c| c.label == "email"), "Should NOT suggest email (* already selects all)");
}

#[test]
fn test_select_column_filtering_with_from() {
    // The exact scenario from the issue: SELECT *, value, <cursor> FROM generate_series(1, 10)
    let mut schema = build_test_schema("");
    schema.add_table("generate_series", vec!["value".to_string()], true);
    let completions = get_completions_with_text(
        "SELECT *, value, ",
        &schema,
    );

    assert!(!completions.iter().any(|c| c.label == "value"), "Should NOT suggest value (already used)");
}
