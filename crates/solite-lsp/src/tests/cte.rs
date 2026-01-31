//! CTE (Common Table Expression) tests for diagnostics, autocomplete, and hover.

use super::*;
use crate::context::{detect_context, CompletionContext, CteRef};
use solite_analyzer::{analyze_with_schema, format_hover_content, find_symbol_at_offset, find_statement_at_offset};
use solite_parser::parse_program;

// ============================================================================
// CTE Context Detection Tests
// ============================================================================

#[test]
fn test_cte_context_detects_cte_name() {
    // WITH foo AS (SELECT 1) SELECT * FROM |
    let ctx = detect_context("WITH foo AS (SELECT 1) SELECT * FROM ", 37);
    match ctx {
        CompletionContext::AfterFrom { ctes } => {
            assert_eq!(ctes.len(), 1);
            assert_eq!(ctes[0].name, "foo");
        }
        other => panic!("Expected AfterFrom with CTEs, got {:?}", other),
    }
}

#[test]
fn test_cte_context_multiple_ctes() {
    let ctx = detect_context(
        "WITH foo AS (SELECT 1 AS x), bar AS (SELECT 2 AS y) SELECT * FROM ",
        66,
    );
    match ctx {
        CompletionContext::AfterFrom { ctes } => {
            assert_eq!(ctes.len(), 2);
            let names: Vec<_> = ctes.iter().map(|c| c.name.as_str()).collect();
            assert!(names.contains(&"foo"));
            assert!(names.contains(&"bar"));
        }
        other => panic!("Expected AfterFrom with CTEs, got {:?}", other),
    }
}

#[test]
fn test_cte_context_explicit_columns() {
    let ctx = detect_context(
        "WITH foo(a, b) AS (SELECT 1, 2) SELECT * FROM foo WHERE ",
        56,
    );
    match ctx {
        CompletionContext::WhereClause { ctes, .. } => {
            assert_eq!(ctes.len(), 1);
            assert_eq!(ctes[0].name, "foo");
            assert_eq!(ctes[0].columns, vec!["a", "b"]);
        }
        other => panic!("Expected WhereClause with CTEs, got {:?}", other),
    }
}

#[test]
fn test_cte_context_in_select_columns() {
    let ctx = detect_context(
        "WITH foo AS (SELECT 1 AS x) SELECT  FROM foo",
        35, // After "SELECT "
    );
    match ctx {
        CompletionContext::SelectColumns { ctes, .. } => {
            assert_eq!(ctes.len(), 1);
            assert_eq!(ctes[0].name, "foo");
        }
        other => panic!("Expected SelectColumns with CTEs, got {:?}", other),
    }
}

#[test]
fn test_cte_context_after_semicolon_resets() {
    // CTEs should be reset after a semicolon
    let ctx = detect_context(
        "WITH foo AS (SELECT 1) SELECT 1; SELECT * FROM ",
        47,
    );
    match ctx {
        CompletionContext::AfterFrom { ctes } => {
            assert!(ctes.is_empty(), "CTEs should be empty after semicolon");
        }
        other => panic!("Expected AfterFrom, got {:?}", other),
    }
}

// ============================================================================
// CTE Autocomplete Tests
// ============================================================================

#[test]
fn test_cte_autocomplete_table_suggestions() {
    // CTEs should appear in table suggestions
    let schema = build_test_schema("CREATE TABLE users (id INTEGER, name TEXT);");
    let ctx = detect_context("WITH foo AS (SELECT 1 AS x) SELECT * FROM ", 42);

    let items = get_completions_for_context(&ctx, Some(&schema));

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"foo"), "CTE 'foo' should be suggested");
    assert!(labels.contains(&"users"), "Regular table 'users' should be suggested");
}

#[test]
fn test_cte_autocomplete_column_suggestions() {
    // CTE columns should be suggested
    let schema = build_test_schema("CREATE TABLE users (id INTEGER, name TEXT);");
    let ctx = CompletionContext::SelectColumns {
        tables: vec![TableRef::new("foo".to_string(), None)],
        ctes: vec![CteRef {
            name: "foo".to_string(),
            columns: vec!["x".to_string(), "y".to_string()],
            star_sources: vec![],
        }],
    };

    let items = get_completions_for_context(&ctx, Some(&schema));

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"x"), "CTE column 'x' should be suggested");
    assert!(labels.contains(&"y"), "CTE column 'y' should be suggested");
}

#[test]
fn test_cte_autocomplete_qualified_column() {
    // cte.column should work
    let schema = build_test_schema("");
    let ctx = CompletionContext::QualifiedColumn {
        qualifier: "foo".to_string(),
        tables: vec![TableRef::new("foo".to_string(), None)],
        ctes: vec![CteRef {
            name: "foo".to_string(),
            columns: vec!["col_a".to_string(), "col_b".to_string()],
            star_sources: vec![],
        }],
    };

    let items = get_completions_for_context(&ctx, Some(&schema));

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"col_a"), "CTE column 'col_a' should be suggested");
    assert!(labels.contains(&"col_b"), "CTE column 'col_b' should be suggested");
}

// ============================================================================
// CTE Diagnostics Tests
// ============================================================================

#[test]
fn test_cte_diagnostics_no_unknown_table_error() {
    // CTE names should not be flagged as unknown tables
    let source = "WITH foo AS (SELECT 1 AS x) SELECT * FROM foo";
    let program = parse_program(source).unwrap();
    let diagnostics = analyze_with_schema(&program, None);

    assert!(
        diagnostics.is_empty(),
        "CTE 'foo' should not be flagged as unknown table, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_cte_diagnostics_column_validation() {
    // CTE columns should be validated
    let source = "WITH foo AS (SELECT 1 AS x) SELECT x FROM foo";
    let program = parse_program(source).unwrap();
    let diagnostics = analyze_with_schema(&program, None);

    assert!(
        diagnostics.is_empty(),
        "Column 'x' from CTE should be valid, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_cte_diagnostics_invalid_column() {
    // Invalid columns on CTEs should be flagged
    let source = "WITH foo AS (SELECT 1 AS x) SELECT z FROM foo";
    let program = parse_program(source).unwrap();
    let diagnostics = analyze_with_schema(&program, None);

    assert_eq!(
        diagnostics.len(),
        1,
        "Column 'z' should be flagged as unknown"
    );
    assert!(diagnostics[0].message.contains("Column 'z' does not exist"));
}

#[test]
fn test_cte_diagnostics_earlier_cte_visible() {
    // Earlier CTEs should be visible to later CTEs
    let source = "WITH foo AS (SELECT 1 AS x), bar AS (SELECT * FROM foo) SELECT * FROM bar";
    let program = parse_program(source).unwrap();
    let diagnostics = analyze_with_schema(&program, None);

    assert!(
        diagnostics.is_empty(),
        "Earlier CTE 'foo' should be visible to 'bar', got: {:?}",
        diagnostics
    );
}

// ============================================================================
// CTE Hover Tests
// ============================================================================

#[test]
fn test_cte_hover_shows_columns() {
    // Hovering over a CTE reference should show its columns
    let source = "WITH foo AS (SELECT 1 AS a, 2 AS b) SELECT * FROM foo";
    let program = parse_program(source).unwrap();
    let stmt = find_statement_at_offset(&program, 50).unwrap();

    // Find symbol at "foo" in FROM clause (offset ~50)
    if let Some((symbol, _span)) = find_symbol_at_offset(stmt, source, 50, None) {
        let content = format_hover_content(&symbol, None);
        assert!(
            content.contains("foo"),
            "Hover should contain CTE name"
        );
        assert!(
            content.contains("Common Table Expression"),
            "Hover should identify as CTE"
        );
        assert!(
            content.contains("a"),
            "Hover should show inferred column 'a'"
        );
        assert!(
            content.contains("b"),
            "Hover should show inferred column 'b'"
        );
    } else {
        panic!("Should find CTE symbol at offset 50");
    }
}

#[test]
fn test_cte_hover_explicit_columns() {
    // CTEs with explicit columns should show those
    let source = "WITH foo(x, y) AS (SELECT 1, 2) SELECT * FROM foo";
    let program = parse_program(source).unwrap();
    let stmt = find_statement_at_offset(&program, 46).unwrap();

    if let Some((symbol, _span)) = find_symbol_at_offset(stmt, source, 46, None) {
        let content = format_hover_content(&symbol, None);
        assert!(content.contains("x"), "Hover should show explicit column 'x'");
        assert!(content.contains("y"), "Hover should show explicit column 'y'");
    } else {
        panic!("Should find CTE symbol");
    }
}

#[test]
fn test_cte_where_in_join_context() {
    // Test: WHERE inside CTE with JOIN should suggest columns from both CTE sources
    let ddl = "create table movies(id integer, name text, released_year integer);
create table actors(id integer, name text, birth_year integer);";

    let sql_with_marker = r#"create table movies(id integer, name text, released_year integer);
create table actors(id integer, name text, birth_year integer);

with movies_20s as (
  select *
  from movies
  where released_year >= 2000
),
genz_actors as (
  select *
  from actors
  where birth_year between 1997 and 2010
),
final as (
  select *
  from movies_20s
  left join genz_actors on genz_actors.id = movies_20s.id
  where CURSOR_HERE
)

select * from final;"#;

    let offset = sql_with_marker.find("CURSOR_HERE").unwrap();
    let test_sql = sql_with_marker.replace("CURSOR_HERE", "");

    let ctx = detect_context(&test_sql, offset);

    // Verify context is detected correctly
    match &ctx {
        CompletionContext::WhereClause { tables, ctes } => {
            // Should have both CTEs as tables
            assert_eq!(tables.len(), 2, "Should have 2 tables in scope");
            assert!(tables.iter().any(|t| t.name == "movies_20s"), "movies_20s should be in tables");
            assert!(tables.iter().any(|t| t.name == "genz_actors"), "genz_actors should be in tables");

            // CTEs should have star_sources pointing to real tables
            assert_eq!(ctes.len(), 2, "Should have 2 CTEs");
            let movies_cte = ctes.iter().find(|c| c.name == "movies_20s").expect("movies_20s CTE");
            let actors_cte = ctes.iter().find(|c| c.name == "genz_actors").expect("genz_actors CTE");
            assert!(movies_cte.star_sources.contains(&"movies".to_string()), "movies_20s should have star_source 'movies'");
            assert!(actors_cte.star_sources.contains(&"actors".to_string()), "genz_actors should have star_source 'actors'");
        }
        other => {
            panic!("Expected WhereClause context, got {:?}", other);
        }
    }

    // Build schema from DDL only
    let schema = build_test_schema(ddl);
    let items = get_completions_for_context(&ctx, Some(&schema));
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // Should have columns from both movies (via movies_20s) and actors (via genz_actors)
    // Unique columns
    assert!(labels.contains(&"released_year"), "Should suggest released_year from movies");
    assert!(labels.contains(&"birth_year"), "Should suggest birth_year from actors");

    // Duplicate columns should be qualified
    assert!(labels.iter().any(|l| l.contains("movies_20s") && l.contains("id")), "Should have qualified movies_20s.id");
    assert!(labels.iter().any(|l| l.contains("genz_actors") && l.contains("id")), "Should have qualified genz_actors.id");
    assert!(labels.iter().any(|l| l.contains("movies_20s") && l.contains("name")), "Should have qualified movies_20s.name");
    assert!(labels.iter().any(|l| l.contains("genz_actors") && l.contains("name")), "Should have qualified genz_actors.name");
}
