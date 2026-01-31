//! Smart autocomplete and placeholder-based test framework

use super::*;
use std::collections::HashMap;

// ========================================================================
// Smart Column Autocomplete Tests (SELECT without FROM)
// ========================================================================
//
// When typing `SELECT ` without a FROM clause, autocomplete should suggest
// column names from all known tables. When a column is selected:
//
// - If the column exists in exactly ONE table: insert "column, $1 FROM table"
//   where $1 is a snippet placeholder for the cursor position
//
// - If the column exists in MULTIPLE tables: insert just "column" without
//   a FROM clause (user must specify which table)
//
// This enables a workflow where you type `SELECT `, pick a column, and
// automatically get the FROM clause filled in.

/// Test schema for smart column autocomplete tests.
/// This schema has:
/// - `name` column in BOTH movies and actors (ambiguous)
/// - `released_year` only in movies (unambiguous)
/// - `birth_year` only in actors (unambiguous)
/// - `movie_id` and `actor_id` only in acting_credits (unambiguous)
const SMART_AUTOCOMPLETE_SCHEMA_SQL: &str = r#"
    CREATE TABLE movies (
        name TEXT,
        released_year INTEGER
    );

    CREATE TABLE actors (
        name TEXT,
        birth_year INTEGER
    );

    CREATE TABLE acting_credits (
        movie_id INTEGER REFERENCES movies(rowid),
        actor_id INTEGER REFERENCES actors(rowid)
    );
"#;

#[test]
fn test_smart_autocomplete_unique_column_inserts_from() {
    // When completing `released_year` after SELECT, should insert:
    // "released_year, $1 FROM movies" where $1 is cursor position
    use tower_lsp::lsp_types::InsertTextFormat;

    let schema = build_schema(&parse_program(SMART_AUTOCOMPLETE_SCHEMA_SQL).unwrap());

    // Get completions for SELECT without FROM (empty tables list)
    let ctx = CompletionContext::SelectColumns { tables: vec![], ctes: vec![] };
    let items = get_completions_for_context(&ctx, Some(&schema));

    // Find the released_year completion
    let released_year = items
        .iter()
        .find(|i| i.label == "released_year")
        .expect("Should have released_year completion");

    // Should have snippet format with FROM clause
    assert_eq!(
        released_year.insert_text.as_ref().unwrap(),
        "released_year, $1 FROM movies"
    );
    assert_eq!(
        released_year.insert_text_format,
        Some(InsertTextFormat::SNIPPET)
    );
    assert_eq!(released_year.detail.as_ref().unwrap(), "from movies");
}

#[test]
fn test_smart_autocomplete_ambiguous_column_no_from() {
    // When completing `name` after SELECT, should insert just "name"
    // because it exists in both `movies` and `actors` tables.
    use tower_lsp::lsp_types::InsertTextFormat;

    let schema = build_schema(&parse_program(SMART_AUTOCOMPLETE_SCHEMA_SQL).unwrap());

    // Get completions for SELECT without FROM
    let ctx = CompletionContext::SelectColumns { tables: vec![], ctes: vec![] };
    let items = get_completions_for_context(&ctx, Some(&schema));

    // Find the name completion
    let name_item = items
        .iter()
        .find(|i| i.label == "name")
        .expect("Should have name completion");

    // Should NOT have snippet format (just plain text)
    assert!(
        name_item.insert_text_format.is_none()
            || name_item.insert_text_format == Some(InsertTextFormat::PLAIN_TEXT),
        "Ambiguous column should not use snippet format"
    );

    // Should NOT have FROM clause in insert text
    let insert = name_item
        .insert_text
        .as_deref()
        .unwrap_or("name");
    assert!(
        !insert.contains("FROM"),
        "Ambiguous column should not include FROM"
    );

    // Detail should show both tables
    let detail = name_item.detail.as_ref().unwrap();
    assert!(
        detail.contains("movies") && detail.contains("actors"),
        "Detail should list both tables, got: {}",
        detail
    );
}

#[test]
fn test_smart_autocomplete_all_unique_columns() {
    // Test that all unique columns get proper FROM insertion:
    // - released_year -> FROM movies
    // - birth_year -> FROM actors
    // - movie_id -> FROM acting_credits
    // - actor_id -> FROM acting_credits
    use tower_lsp::lsp_types::InsertTextFormat;

    let schema = build_schema(&parse_program(SMART_AUTOCOMPLETE_SCHEMA_SQL).unwrap());

    let ctx = CompletionContext::SelectColumns { tables: vec![], ctes: vec![] };
    let items = get_completions_for_context(&ctx, Some(&schema));

    // Helper to check a unique column completion
    let check_unique = |col: &str, table: &str| {
        let item = items
            .iter()
            .find(|i| i.label == col)
            .unwrap_or_else(|| panic!("Should have {} completion", col));
        let expected_insert = format!("{}, $1 FROM {}", col, table);
        assert_eq!(
            item.insert_text.as_ref().unwrap(),
            &expected_insert,
            "Column {} should insert FROM {}",
            col,
            table
        );
        assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
    };

    // Verify all unique columns
    check_unique("released_year", "movies");
    check_unique("birth_year", "actors");
    check_unique("movie_id", "acting_credits");
    check_unique("actor_id", "acting_credits");
}

#[test]
fn test_autocomplete_with_from_clause_only_suggests_table_columns() {
    // When FROM clause exists, only suggest columns from those tables.
    // This tests the exact example from the user:
    //
    // CREATE TABLE movies (name TEXT, released_year INTEGER);
    // CREATE TABLE actors (name TEXT, birth_year INTEGER);
    // CREATE TABLE acting_credits (
    //     movie_id INTEGER REFERENCES movies(rowid),
    //     actor_id INTEGER REFERENCES actors(rowid)
    // );
    //
    // SELECT |
    // FROM acting_credits;
    //
    // Cursor after SELECT should ONLY suggest: movie_id, actor_id, rowid
    // (NOT released_year, birth_year, or name from other tables)

    let schema = build_schema(&parse_program(SMART_AUTOCOMPLETE_SCHEMA_SQL).unwrap());

    // Simulate tables in scope from FROM clause
    let tables = vec![TableRef {
        name: "acting_credits".to_string(),
        alias: None,
    }];

    let ctx = CompletionContext::SelectColumns { tables, ctes: vec![] };
    let items = get_completions_for_context(&ctx, Some(&schema));

    // Should have exactly 3 columns: movie_id, actor_id, rowid
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"movie_id"), "Should suggest movie_id");
    assert!(labels.contains(&"actor_id"), "Should suggest actor_id");
    assert!(
        labels.contains(&"rowid"),
        "Should suggest rowid (table is not WITHOUT ROWID)"
    );

    // Should NOT have columns from other tables
    assert!(
        !labels.contains(&"name"),
        "Should NOT suggest name (from other tables)"
    );
    assert!(
        !labels.contains(&"released_year"),
        "Should NOT suggest released_year (from movies)"
    );
    assert!(
        !labels.contains(&"birth_year"),
        "Should NOT suggest birth_year (from actors)"
    );

    // Verify we have exactly the expected columns
    assert_eq!(
        labels.len(),
        3,
        "Should have exactly 3 columns, got: {:?}",
        labels
    );
}

// ========================================================================
// Placeholder-Based Autocomplete Test Framework
// ========================================================================
//
// Test autocomplete using `$$N` markers in SQL to indicate positions.
//
// Example:
// ```
// create table t (a, b);
// select $$1 from t where $$2;
// ```
//
// Then assert: expect_completions(1, &["a", "b", "rowid"])
//              expect_completions(2, &["a", "b", "rowid"])

/// Parse SQL with $$N markers.
///
/// Markers are replaced with `__M_N__` placeholders to make valid SQL.
/// Returns (processed_sql, marker_positions) where positions are byte offsets
/// pointing to the START of each placeholder (where cursor would be for completion).
fn extract_markers(sql: &str) -> (String, HashMap<usize, usize>) {
    // First pass: replace $$N with __M_N__ to make valid SQL
    let mut processed = sql.to_string();
    let mut markers: HashMap<usize, usize> = HashMap::new();

    // Find all markers and replace them
    let marker_re = regex::Regex::new(r"\$\$(\d+)").unwrap();
    for cap in marker_re.captures_iter(sql) {
        let num: usize = cap[1].parse().unwrap();
        let placeholder = format!("__M_{}__", num);
        processed = processed.replacen(&cap[0], &placeholder, 1);
    }

    // Second pass: find positions of __M_N__ in processed string
    for cap in regex::Regex::new(r"__M_(\d+)__")
        .unwrap()
        .captures_iter(&processed.clone())
    {
        let num: usize = cap[1].parse().unwrap();
        let full_match = cap.get(0).unwrap();
        markers.insert(num, full_match.start());
    }

    (processed, markers)
}

/// Run autocomplete test with $$N markers.
///
/// `sql` - SQL with DDL and queries. Use $$1, $$2, etc. to mark completion positions.
///         Markers are replaced with placeholder identifiers to make SQL parse.
/// `expectations` - Vec of (marker_num, expected_column_labels)
///
/// Example:
/// ```sql
/// create table t (a, b);
/// select $$1 from t where $$2;
/// ```
/// Becomes: `select __M_1__ from t where __M_2__;`
fn run_autocomplete_test(sql: &str, expectations: &[(usize, &[&str])]) {
    let (processed_sql, markers) = extract_markers(sql);

    // Build schema from DDL in the processed SQL
    let program = parse_program(&processed_sql).unwrap_or_else(|_| {
        panic!(
            "Test SQL should parse. Processed SQL:\n{}",
            processed_sql
        )
    });
    let schema = build_schema(&program);

    for (marker_num, expected_labels) in expectations {
        let offset = markers
            .get(marker_num)
            .unwrap_or_else(|| panic!("Marker $${} not found in SQL", marker_num));

        // Detect context at this position
        let ctx = detect_context(&processed_sql, *offset);

        // Get completions
        let items = get_completions_for_context(&ctx, Some(&schema));
        let actual_labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

        // Check expected labels are present
        for expected in *expected_labels {
            assert!(
                actual_labels.contains(expected),
                "Marker $${}: Expected '{}' in completions.\nActual: {:?}\nContext: {:?}\nProcessed SQL:\n{}",
                marker_num,
                expected,
                actual_labels,
                ctx,
                processed_sql
            );
        }

        // Check no unexpected labels (strict mode)
        for actual in &actual_labels {
            assert!(
                expected_labels.contains(actual),
                "Marker $${}: Unexpected '{}' in completions.\nExpected: {:?}\nActual: {:?}\nProcessed SQL:\n{}",
                marker_num,
                actual,
                expected_labels,
                actual_labels,
                processed_sql
            );
        }
    }
}

#[test]
fn test_extract_markers() {
    let (processed, markers) = extract_markers("select $$1 from t where $$2");
    assert_eq!(processed, "select __M_1__ from t where __M_2__");
    assert_eq!(markers.get(&1), Some(&7)); // position of __M_1__
    assert_eq!(markers.get(&2), Some(&28)); // position of __M_2__
}

#[test]
fn test_autocomplete_select_from_single_table() {
    // User's exact example
    run_autocomplete_test(
        r#"
        create table movies (
            name text,
            released_year integer
        );

        create table actors (
            name text,
            birth_year integer
        );

        create table acting_credits (
            movie_id integer references movies(rowid),
            actor_id integer references actors(rowid)
        );

        select $$1
        from acting_credits
        where $$2;
        "#,
        &[
            // $$1: After SELECT, before FROM acting_credits
            // Should only suggest columns from acting_credits
            (1, &["movie_id", "actor_id", "rowid"]),
            // $$2: In WHERE clause
            // Should only suggest columns from acting_credits
            (2, &["movie_id", "actor_id", "rowid"]),
        ],
    );
}

#[test]
fn test_autocomplete_join_columns() {
    // Test JOIN scenario - both tables should be in scope
    // When columns exist in multiple tables (like 'id'), they appear as qualified names
    run_autocomplete_test(
        r#"
        create table users (id integer primary key, name text);
        create table orders (id integer primary key, user_id integer, total real);

        select $$1
        from users
        join orders on $$2
        where $$3;
        "#,
        &[
            // $$1: SELECT columns - both tables in scope
            // Unique columns: name (users), user_id (orders), total (orders)
            // Duplicate columns show as qualified: users.id, orders.id, users.rowid, orders.rowid
            (
                1,
                &[
                    "name",
                    "user_id",
                    "total",
                    "users.id",
                    "orders.id",
                    "users.rowid",
                    "orders.rowid",
                ],
            ),
            // $$2: JOIN ON - same columns
            (
                2,
                &[
                    "name",
                    "user_id",
                    "total",
                    "users.id",
                    "orders.id",
                    "users.rowid",
                    "orders.rowid",
                ],
            ),
            // $$3: WHERE - same columns
            (
                3,
                &[
                    "name",
                    "user_id",
                    "total",
                    "users.id",
                    "orders.id",
                    "users.rowid",
                    "orders.rowid",
                ],
            ),
        ],
    );
}

#[test]
fn test_autocomplete_update_set() {
    // UPDATE SET clause - test only WHERE clause since SET syntax requires col=val
    run_autocomplete_test(
        r#"
        create table products (id integer, name text, price real);

        update products set name = 'test' where $$1;
        "#,
        &[
            // $$1: WHERE - suggest columns for condition
            (1, &["id", "name", "price", "rowid"]),
        ],
    );
}

#[test]
fn test_autocomplete_delete_where() {
    run_autocomplete_test(
        r#"
        create table logs (id integer, message text, created_at text);

        delete from logs where $$1;
        "#,
        &[
            // $$1: DELETE WHERE - suggest columns
            (1, &["id", "message", "created_at", "rowid"]),
        ],
    );
}

#[test]
fn test_autocomplete_group_by_order_by() {
    run_autocomplete_test(
        r#"
        create table sales (product text, region text, amount real);

        select product, sum(amount)
        from sales
        group by $$1
        order by $$2;
        "#,
        &[
            // $$1: GROUP BY
            (1, &["product", "region", "amount", "rowid"]),
            // $$2: ORDER BY
            (2, &["product", "region", "amount", "rowid"]),
        ],
    );
}

#[test]
#[ignore = "Subquery scope tracking not yet implemented - context sees outer table"]
fn test_autocomplete_subquery_isolation() {
    // TODO: Subquery should only see its own tables
    // Currently, the context detection doesn't track parenthesis scope for subqueries,
    // so it sees the outer table instead of the inner one.
    run_autocomplete_test(
        r#"
        create table outer_table (outer_col text);
        create table inner_table (inner_col text);

        select * from outer_table where outer_col in (
            select $$1 from inner_table
        );
        "#,
        &[
            // $$1: Inside subquery - should only see inner_table columns
            (1, &["inner_col", "rowid"]),
        ],
    );
}

#[test]
fn test_autocomplete_insert_columns() {
    run_autocomplete_test(
        r#"
        create table items (id integer, name text, qty integer);

        insert into items ($$1) values (1, 'x', 10);
        "#,
        &[
            // $$1: INSERT column list
            (1, &["id", "name", "qty", "rowid"]),
        ],
    );
}

#[test]
fn test_autocomplete_create_table_context() {
    // After CREATE TABLE, should suggest IF NOT EXISTS
    let sql = "CREATE TABLE ";
    let ctx = detect_context(sql, sql.len());

    assert_eq!(ctx, CompletionContext::AfterCreateTable);

    let completions = get_completions_for_context(&ctx, None);
    let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();

    assert!(
        labels.contains(&"if not exists"),
        "Should suggest 'if not exists', got: {:?}",
        labels
    );
}

#[test]
fn test_autocomplete_insert_context() {
    // After INSERT, should suggest INTO and OR variants
    let sql = "INSERT ";
    let ctx = detect_context(sql, sql.len());

    assert_eq!(ctx, CompletionContext::AfterInsert);

    let completions = get_completions_for_context(&ctx, None);
    let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();

    assert!(labels.contains(&"into"), "Should suggest 'into', got: {:?}", labels);
    assert!(labels.contains(&"or abort"), "Should suggest 'or abort', got: {:?}", labels);
    assert!(labels.contains(&"or fail"), "Should suggest 'or fail', got: {:?}", labels);
    assert!(labels.contains(&"or ignore"), "Should suggest 'or ignore', got: {:?}", labels);
    assert!(labels.contains(&"or replace"), "Should suggest 'or replace', got: {:?}", labels);
    assert!(labels.contains(&"or rollback"), "Should suggest 'or rollback', got: {:?}", labels);
}

#[test]
fn test_autocomplete_insert_into_suggests_tables() {
    // After INSERT INTO, should suggest table names
    let sql = "CREATE TABLE users (id); INSERT INTO ";
    let ctx = detect_context(sql, sql.len());

    assert_eq!(ctx, CompletionContext::AfterInto);
}

#[test]
fn test_autocomplete_replace_context() {
    // After REPLACE, should suggest INTO
    let sql = "REPLACE ";
    let ctx = detect_context(sql, sql.len());

    assert_eq!(ctx, CompletionContext::AfterReplace);

    let completions = get_completions_for_context(&ctx, None);
    let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();

    assert!(labels.contains(&"into"), "Should suggest 'into', got: {:?}", labels);
}

#[test]
fn test_autocomplete_replace_into_suggests_tables() {
    // After REPLACE INTO, should suggest table names
    let sql = "CREATE TABLE users (id); REPLACE INTO ";
    let ctx = detect_context(sql, sql.len());

    assert_eq!(ctx, CompletionContext::AfterInto);
}

// ========================================================================
// CTE Chain Resolution Tests
// ========================================================================

#[test]
fn test_autocomplete_cte_chain_star_expansion() {
    // Test that SELECT * in a CTE resolves columns from the referenced CTE.
    //
    // with foo as (select a, b from t),
    // final as (select *, c from foo)  -- * expands to a, b from foo
    // select $$1 from final;           -- should see a, b, c
    run_autocomplete_test(
        r#"
        create table t (a integer, b text);

        with foo as (
            select a, b from t
        ),
        final as (
            select *, 1 as c from foo
        )
        select $$1 from final;
        "#,
        &[
            // $$1: Columns from `final` CTE which has a, b (from *->foo) and c
            (1, &["a", "b", "c"]),
        ],
    );
}

#[test]
fn test_autocomplete_cte_chain_nested() {
    // Test deeply nested CTE chain: t -> foo -> bar -> final
    run_autocomplete_test(
        r#"
        create table t (x integer);

        with foo as (
            select x, 1 as y from t
        ),
        bar as (
            select *, 2 as z from foo
        ),
        final as (
            select * from bar
        )
        select $$1 from final;
        "#,
        &[
            // $$1: final has x, y, z from the chain t->foo->bar
            (1, &["x", "y", "z"]),
        ],
    );
}

#[test]
fn test_autocomplete_cte_inner_select() {
    // Test autocomplete inside a CTE body that references an earlier CTE
    run_autocomplete_test(
        r#"
        create table libfec_filings (filing_id integer, filer_name text, coverage_from_date text, filer_id text);

        with foo as (
            select filing_id, 'test' as name
            from libfec_filings
        ),
        final as (
            select $$1
            from foo
        )
        select * from final;
        "#,
        &[
            // $$1: Inside final CTE, selecting from foo - should see foo's columns
            (1, &["filing_id", "name"]),
        ],
    );
}

#[test]
fn test_autocomplete_cte_outer_select_with_star() {
    // Test the user's exact example with * expansion
    run_autocomplete_test(
        r#"
        create table libfec_filings (filing_id integer, filer_name text, coverage_from_date text, filer_id text);

        with foo as (
            select filing_id, 'aasdf' as name
            from libfec_filings
            where filer_name like '%YOUN%'
        ),
        final as (
            select *, 1 as aaa
            from foo
        )
        select *, $$1 from final;
        "#,
        &[
            // $$1: Outer SELECT from final - should see filing_id, name (from *->foo) and aaa
            (1, &["filing_id", "name", "aaa"]),
        ],
    );
}

#[test]
fn test_autocomplete_where_clause_after_and() {
    // Test autocomplete after AND in WHERE clause - should suggest columns
    //
    // select *
    // from libfec_filings
    // where comment = 'test' and $$1
    //
    // After AND, should suggest columns (back in column context)
    run_autocomplete_test(
        r#"
        create table libfec_filings (filing_id integer, filer_name text, comment text);

        select *
        from libfec_filings
        where comment = 'test' and $$1
        "#,
        &[
            // $$1: After AND in WHERE, suggest columns
            (1, &["filing_id", "filer_name", "comment", "rowid"]),
        ],
    );
}

#[test]
fn test_context_where_after_expr_suggests_operators() {
    // Direct context test for after expression in WHERE clause
    // This verifies the AfterWhereExpr context is detected and returns correct completions
    use super::{build_test_schema, get_completions_for_context};
    use solite_lsp::context::detect_context;

    let schema = build_test_schema("CREATE TABLE t (id INTEGER, name TEXT);");

    // After an identifier in WHERE, context should suggest operators
    let ctx = detect_context("SELECT * FROM t WHERE name ", 27);
    let items = get_completions_for_context(&ctx, Some(&schema));
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"and"), "Should suggest 'and', got {:?}", labels);
    assert!(labels.contains(&"or"), "Should suggest 'or', got {:?}", labels);
    assert!(labels.contains(&"="), "Should suggest '=', got {:?}", labels);
    assert!(labels.contains(&"like"), "Should suggest 'like', got {:?}", labels);
    assert!(labels.contains(&"order by"), "Should suggest 'order by', got {:?}", labels);
    assert!(labels.contains(&"limit"), "Should suggest 'limit', got {:?}", labels);
}

#[test]
fn test_autocomplete_after_table_name() {
    // Test context right after a table name in FROM clause
    //
    // select *
    // from libfec_filings
    // $$1
    //
    // After FROM table_name, completions should suggest JOIN keywords, WHERE, etc.
    run_autocomplete_test(
        r#"
        create table libfec_filings (filing_id integer, filer_name text);

        select *
        from libfec_filings
        $$1
        "#,
        &[
            // $$1: After table name, context is AfterFromTable
            // Should suggest JOIN keywords, clause keywords, and tables
            (1, &[
                // JOIN keywords
                "join", "inner join", "left join", "left outer join",
                "right join", "right outer join", "full join", "full outer join",
                "cross join", "natural join",
                // Clause keywords
                "where", "group by", "order by", "limit",
                // Tables
                "libfec_filings",
            ]),
        ],
    );
}

#[test]
fn test_autocomplete_after_table_name_typing_alias() {
    // Test context when typing after a table name (e.g., starting an alias or keyword)
    //
    // select *
    // from libfec_filings
    // l$$1
    //
    // After FROM table_name, typing more text could be:
    // - An alias for the table
    // - Start of a keyword like LEFT JOIN, LIMIT
    // Completions should suggest JOIN keywords, WHERE, tables, etc.
    run_autocomplete_test(
        r#"
        create table libfec_filings (filing_id integer, filer_name text);

        select *
        from libfec_filings
        l$$1
        "#,
        &[
            // $$1: After table name + partial "l", context is AfterFromTable
            // Should suggest JOIN keywords, clause keywords, and tables
            (1, &[
                // JOIN keywords
                "join", "inner join", "left join", "left outer join",
                "right join", "right outer join", "full join", "full outer join",
                "cross join", "natural join",
                // Clause keywords
                "where", "group by", "order by", "limit",
                // Tables
                "libfec_filings",
            ]),
        ],
    );
}
