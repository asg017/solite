//! Snapshot tests for SQL formatting
//!
//! Uses insta for snapshot testing to verify formatter output.

use insta::assert_snapshot;

use crate::{format_sql, FormatConfig, KeywordCase};

/// Format a snapshot that includes both input and output for easy comparison
fn snapshot(sql: &str, config: &FormatConfig) -> String {
    let formatted = format_sql(sql, config).unwrap();
    format!("-- Input:\n-- {}\n\n-- Output:\n{}", sql.replace('\n', "\n-- "), formatted)
}

// =============================================================================
// Basic SELECT statements
// =============================================================================

#[test]
fn simple_select() {
    let sql = "select a from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_where() {
    let sql = "select a, b from t where x = 1";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_multiple_columns() {
    let sql = "select a, b, c, d, e from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_aliases() {
    let sql = "select a as col_a, b col_b, c as col_c from t as tbl";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_star() {
    let sql = "select * from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_table_star() {
    let sql = "select t.*, u.* from t, u";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_distinct() {
    let sql = "select distinct a, b from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_group_by() {
    let sql = "select a, count(*) from t group by a";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_having() {
    let sql = "select a, count(*) from t group by a having count(*) > 1";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_order_by() {
    let sql = "select a, b from t order by a asc, b desc";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_limit() {
    let sql = "select a from t limit 10";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_limit_offset() {
    let sql = "select a from t limit 10 offset 5";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_complex_where() {
    let sql = "select a from t where x = 1 and y = 2 or z = 3";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_between() {
    let sql = "select a from t where x between 1 and 10";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_in_list() {
    let sql = "select a from t where x in (1, 2, 3)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_like() {
    let sql = "select a from t where name like '%test%'";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_is_null() {
    let sql = "select a from t where x is null and y is not null";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_case() {
    let sql = "select case when x = 1 then 'one' when x = 2 then 'two' else 'other' end from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_cast() {
    let sql = "select cast(x as integer), cast(y as text) from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_function_calls() {
    let sql = "select count(*), sum(x), avg(y), max(z), min(w) from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_subquery_in_where() {
    let sql = "select a from t where x in (select y from u)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_exists() {
    let sql = "select a from t where exists (select 1 from u where u.id = t.id)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// SELECT with JOINs
// =============================================================================

#[test]
fn select_inner_join() {
    let sql = "select a, b from t inner join u on t.id = u.id";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_left_join() {
    let sql = "select a, b from t left join u on t.id = u.id";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_multiple_joins() {
    let sql = "select a, b, c from t inner join u on t.id = u.id left join v on u.id = v.id";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_cross_join() {
    let sql = "select a, b from t cross join u";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_natural_join() {
    let sql = "select a, b from t natural join u";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_join_using() {
    let sql = "select a, b from t join u using (id, name)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_join_with_aliases() {
    let sql = "select t1.a, t2.b from t as t1 inner join u as t2 on t1.id = t2.id";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// SELECT with CTEs (WITH clause)
// =============================================================================

#[test]
fn select_with_cte() {
    let sql = "with cte as (select a from t) select * from cte";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_multiple_ctes() {
    let sql = "with cte1 as (select a from t), cte2 as (select b from u) select * from cte1, cte2";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_recursive_cte() {
    let sql = "with recursive cnt(x) as (select 1 union all select x+1 from cnt where x < 10) select x from cnt";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_with_cte_columns() {
    let sql = "with cte(col1, col2) as (select a, b from t) select * from cte";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// Compound SELECT (UNION, INTERSECT, EXCEPT)
// =============================================================================

#[test]
fn select_union() {
    let sql = "select a from t union select b from u";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_union_all() {
    let sql = "select a from t union all select b from u";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_intersect() {
    let sql = "select a from t intersect select b from u";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn select_except() {
    let sql = "select a from t except select b from u";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// INSERT statements
// =============================================================================

#[test]
fn insert_values() {
    let sql = "insert into t (a, b, c) values (1, 2, 3)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn insert_multiple_rows() {
    let sql = "insert into t (a, b) values (1, 2), (3, 4), (5, 6)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn insert_default_values() {
    let sql = "insert into t default values";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn insert_select() {
    let sql = "insert into t (a, b) select x, y from u";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn insert_or_replace() {
    let sql = "insert or replace into t (a, b) values (1, 2)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn insert_or_ignore() {
    let sql = "insert or ignore into t (a, b) values (1, 2)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn insert_with_returning() {
    let sql = "insert into t (a, b) values (1, 2) returning *";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn insert_on_conflict_nothing() {
    let sql = "insert into t (a, b) values (1, 2) on conflict do nothing";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn insert_on_conflict_update() {
    let sql = "insert into t (a, b) values (1, 2) on conflict(a) do update set b = excluded.b";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn replace_into() {
    let sql = "replace into t (a, b) values (1, 2)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// UPDATE statements
// =============================================================================

#[test]
fn update_simple() {
    let sql = "update t set a = 1";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn update_with_where() {
    let sql = "update t set a = 1, b = 2 where id = 5";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn update_or_replace() {
    let sql = "update or replace t set a = 1 where id = 5";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn update_with_returning() {
    let sql = "update t set a = 1 where id = 5 returning *";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn update_from_join() {
    let sql = "update t set a = u.b from u where t.id = u.id";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// DELETE statements
// =============================================================================

#[test]
fn delete_all() {
    let sql = "delete from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn delete_with_where() {
    let sql = "delete from t where id = 5";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn delete_with_returning() {
    let sql = "delete from t where id = 5 returning *";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn delete_with_limit() {
    let sql = "delete from t where x > 10 order by x limit 5";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// DDL - CREATE TABLE
// =============================================================================

#[test]
fn create_table_simple() {
    let sql = "create table t (a integer, b text)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_with_constraints() {
    let sql = "create table t (id integer primary key, name text not null, email text unique)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_with_foreign_key() {
    let sql = "create table orders (id integer primary key, user_id integer references users(id))";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_with_table_constraints() {
    let sql = "create table t (a integer, b integer, c integer, primary key (a, b), unique (c))";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_with_check() {
    let sql = "create table t (a integer check(a > 0), b integer, check(a + b < 100))";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_with_default() {
    let sql = "create table t (id integer primary key, count integer default 0, status text default 'pending')";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_if_not_exists() {
    let sql = "create table if not exists t (a integer)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_temp_table() {
    let sql = "create temp table t (a integer)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_as_select() {
    let sql = "create table t as select a, b from u where x = 1";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_without_rowid() {
    let sql = "create table t (a integer primary key, b text) without rowid";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_strict() {
    let sql = "create table t (a integer, b text) strict";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_with_generated() {
    let sql = "create table t (a integer, b integer, c integer generated always as (a + b) stored)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// DDL - CREATE INDEX
// =============================================================================

#[test]
fn create_index() {
    let sql = "create index idx_t_a on t(a)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_unique_index() {
    let sql = "create unique index idx_t_a on t(a)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_index_multiple_columns() {
    let sql = "create index idx_t_ab on t(a, b desc)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_index_if_not_exists() {
    let sql = "create index if not exists idx_t_a on t(a)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_partial_index() {
    let sql = "create index idx_t_a on t(a) where a > 0";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// DDL - CREATE VIEW
// =============================================================================

#[test]
fn create_view() {
    let sql = "create view v as select a, b from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_view_with_columns() {
    let sql = "create view v(col1, col2) as select a, b from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_temp_view() {
    let sql = "create temp view v as select a from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_view_if_not_exists() {
    let sql = "create view if not exists v as select a from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// DDL - CREATE TRIGGER
// =============================================================================

#[test]
fn create_trigger_before_insert() {
    let sql = "create trigger tr_t before insert on t begin select 1; end";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_trigger_after_update() {
    let sql = "create trigger tr_t after update on t for each row begin update audit set modified = datetime('now'); end";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_trigger_with_when() {
    let sql = "create trigger tr_t after delete on t when old.status = 'active' begin insert into deleted values (old.id); end";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// DDL - DROP statements
// =============================================================================

#[test]
fn drop_table() {
    let sql = "drop table t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn drop_table_if_exists() {
    let sql = "drop table if exists t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn drop_index() {
    let sql = "drop index idx_t_a";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn drop_view() {
    let sql = "drop view v";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn drop_trigger() {
    let sql = "drop trigger tr_t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// DDL - ALTER TABLE
// =============================================================================

#[test]
fn alter_table_rename() {
    let sql = "alter table t rename to t2";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn alter_table_rename_column() {
    let sql = "alter table t rename column a to b";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn alter_table_add_column() {
    let sql = "alter table t add column c integer not null default 0";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn alter_table_drop_column() {
    let sql = "alter table t drop column c";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// TCL - Transaction statements
// =============================================================================

#[test]
fn begin_transaction() {
    let sql = "begin";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn begin_deferred() {
    let sql = "begin deferred";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn begin_immediate() {
    let sql = "begin immediate";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn begin_exclusive() {
    let sql = "begin exclusive";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn commit() {
    let sql = "commit";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn rollback() {
    let sql = "rollback";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn rollback_to_savepoint() {
    let sql = "rollback to savepoint sp1";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn savepoint() {
    let sql = "savepoint sp1";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn release_savepoint() {
    let sql = "release savepoint sp1";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// Other statements
// =============================================================================

#[test]
fn vacuum() {
    let sql = "vacuum";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn vacuum_into() {
    let sql = "vacuum into 'backup.db'";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn analyze() {
    let sql = "analyze";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn analyze_table() {
    let sql = "analyze t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn reindex() {
    let sql = "reindex";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn reindex_index() {
    let sql = "reindex idx_t_a";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn attach_database() {
    let sql = "attach database 'other.db' as other";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn detach_database() {
    let sql = "detach database other";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn pragma_get() {
    let sql = "pragma table_info(t)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn pragma_set() {
    let sql = "pragma foreign_keys = 1";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn explain() {
    let sql = "explain select a from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn explain_query_plan() {
    let sql = "explain query plan select a from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// Comment preservation (Note: comments may not be fully preserved in output)
// =============================================================================

#[test]
fn line_comment_before() {
    let sql = "-- This is a comment\nselect a from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn line_comment_after() {
    let sql = "select a from t -- trailing comment";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn block_comment() {
    let sql = "/* block comment */ select a from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn multiple_comments() {
    let sql = "-- first comment\n-- second comment\nselect a from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// Config variations - Lowercase keywords
// =============================================================================

#[test]
fn config_lowercase_select() {
    let sql = "SELECT a, b FROM t WHERE x = 1";
    let config = FormatConfig {
        keyword_case: KeywordCase::Lower,
        ..Default::default()
    };
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn config_lowercase_create_table() {
    let sql = "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL)";
    let config = FormatConfig {
        keyword_case: KeywordCase::Lower,
        ..Default::default()
    };
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn config_lowercase_insert() {
    let sql = "INSERT INTO t (a, b) VALUES (1, 2)";
    let config = FormatConfig {
        keyword_case: KeywordCase::Lower,
        ..Default::default()
    };
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// Config variations - Different indent sizes
// =============================================================================

#[test]
fn config_indent_2_spaces() {
    let sql = "select a, b, c from t";
    let config = FormatConfig {
        indent_size: 2,
        ..Default::default()
    };
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn config_indent_8_spaces() {
    let sql = "select a, b, c from t";
    let config = FormatConfig {
        indent_size: 8,
        ..Default::default()
    };
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// Config variations - Leading commas
// =============================================================================

#[test]
fn config_leading_comma_select() {
    let sql = "select a, b, c, d from t";
    let config = FormatConfig {
        comma_position: crate::CommaPosition::Leading,
        ..Default::default()
    };
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn config_leading_comma_insert() {
    let sql = "insert into t (a, b, c) values (1, 2, 3), (4, 5, 6)";
    let config = FormatConfig {
        comma_position: crate::CommaPosition::Leading,
        ..Default::default()
    };
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// Complex queries
// =============================================================================

#[test]
fn complex_select_with_all_clauses() {
    let sql = "with cte as (select x from y) select distinct a, b, count(*) as cnt from t inner join u on t.id = u.id where a > 1 and b < 10 group by a, b having count(*) > 5 order by cnt desc limit 100 offset 10";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn nested_subqueries() {
    // Note: Deep nested subqueries in FROM clause may not be fully supported
    let sql = "select * from t where x in (select y from u where y in (select z from v))";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn multiple_statements() {
    let sql = "begin; insert into t values (1); commit";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// Edge cases
// =============================================================================

#[test]
fn quoted_identifier() {
    let sql = "select \"select\" from \"from\" where \"where\" = 1";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn identifier_with_spaces() {
    let sql = "select \"column name\" from \"table name\"";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn string_literals() {
    let sql = "select 'hello', 'it''s', 'test' from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn numeric_literals() {
    let sql = "select 123, 45.67, 1e10, 0x1F from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn blob_literal() {
    let sql = "select X'DEADBEEF' from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn null_and_booleans() {
    let sql = "select null, true, false from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn bind_parameters() {
    let sql = "select * from t where a = ? and b = ?1 and c = :name and d = @var and e = $param";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn json_operators() {
    let sql = "select data->'key', data->>'value' from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn collate_expression() {
    let sql = "select name collate nocase from t order by name collate nocase";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn arithmetic_expressions() {
    let sql = "select a + b, c - d, e * f, g / h, i % j from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn comparison_operators() {
    let sql = "select * from t where a = 1 and b <> 2 and c < 3 and d <= 4 and e > 5 and f >= 6";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn bitwise_operators() {
    let sql = "select a & b, c | d, e << 2, f >> 1, ~g from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn string_concatenation() {
    let sql = "select a || b || c from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// Comment preservation - comprehensive tests
// =============================================================================

#[test]
fn comment_between_statements() {
    let sql = "select a from t;\n-- comment between statements\nselect b from u";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn comment_before_first_statement() {
    let sql = "-- header comment\nselect * from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn multiple_comments_before_statement() {
    let sql = "-- first line\n-- second line\n-- third line\nselect * from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn block_comment_before_statement() {
    let sql = "/* This is a\n   multi-line\n   block comment */\nselect * from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn mixed_comments_before_statement() {
    let sql = "-- line comment\n/* block comment */\nselect * from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn comment_before_each_statement() {
    let sql = "-- first query\nselect a from t;\n-- second query\nselect b from u;\n-- third query\nselect c from v";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn comment_before_create_table() {
    let sql = "-- Create the users table\ncreate table users (id integer primary key, name text)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn comment_before_insert() {
    let sql = "-- Insert initial data\ninsert into t (a, b) values (1, 2)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}
