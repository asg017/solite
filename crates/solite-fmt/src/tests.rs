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

#[test]
fn insert_with_nested_json_patch() {
    let sql = "INSERT INTO _history_json_items (timestamp, operation, pk_id, updated_values, [group]) VALUES (strftime('%Y-%m-%d %H:%M:%f', 'now'), 'update', NEW.id, json_patch(json_patch(json_patch('{}', CASE WHEN OLD.name IS NOT NEW.name THEN CASE WHEN NEW.name IS NULL THEN json_object('name', json_object('null', 1)) ELSE json_object('name', NEW.name) END ELSE '{}' END), CASE WHEN OLD.price IS NOT NEW.price THEN CASE WHEN NEW.price IS NULL THEN json_object('price', json_object('null', 1)) ELSE json_object('price', NEW.price) END ELSE '{}' END), CASE WHEN OLD.quantity IS NOT NEW.quantity THEN CASE WHEN NEW.quantity IS NULL THEN json_object('quantity', json_object('null', 1)) ELSE json_object('quantity', NEW.quantity) END ELSE '{}' END), (SELECT id FROM _history_json WHERE current = 1))";
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

// =============================================================================
// DDL - CREATE TABLE (comprehensive tests)
// =============================================================================

#[test]
fn create_table_queue_pattern() {
    let sql = "CREATE TABLE IF NOT EXISTS queue (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id         TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',
    priority        INTEGER DEFAULT 0,
    attempts        INTEGER DEFAULT 0,
    max_attempts    INTEGER DEFAULT 5,
    lease_until     INTEGER,
    leased_by       TEXT,
    created_at      INTEGER NOT NULL,
    available_at    INTEGER NOT NULL,
    completed_at    INTEGER,
    failed_at       INTEGER,
    last_error      TEXT
)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_with_inline_comments() {
    let sql = "CREATE TABLE queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT, -- your payload ID
    status TEXT NOT NULL DEFAULT 'pending',
    lease_until INTEGER, -- unix epoch seconds
    leased_by TEXT -- worker ID
)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_autoincrement() {
    let sql = "create table t (id integer primary key autoincrement, name text)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_multiple_defaults() {
    let sql = "create table t (a integer default 0, b text default 'unknown', c real default 0.0, d integer default null)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_complex_defaults() {
    let sql = "create table t (created_at integer default (strftime('%s', 'now')), updated_at text default (datetime('now')))";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_collate() {
    let sql = "create table t (name text collate nocase, description text collate binary)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_on_conflict() {
    let sql = "create table t (id integer primary key on conflict replace, name text unique on conflict ignore)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_foreign_key_actions() {
    let sql = "create table orders (id integer primary key, user_id integer references users(id) on delete cascade on update set null)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_table_level_foreign_key() {
    let sql = "create table orders (id integer, user_id integer, product_id integer, foreign key (user_id) references users(id) on delete cascade, foreign key (product_id) references products(id))";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_virtual() {
    let sql = "create virtual table docs using fts5(title, content, tokenize='porter')";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_table_strict_without_rowid() {
    let sql = "create table t (a text primary key, b integer not null) strict, without rowid";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// DDL - CREATE INDEX (comprehensive tests)
// =============================================================================

#[test]
fn create_index_many_columns() {
    let sql = "create index idx_many on t(a, b, c, d, e)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_index_collate() {
    let sql = "create index idx_name_nocase on t(name collate nocase)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_index_multiple_with_order() {
    let sql = "create index idx_composite on t(a asc, b desc, c)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_index_partial_complex() {
    let sql = "create index idx_active_users on users(email) where status = 'active' and deleted_at is null";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// DDL - CREATE TRIGGER (comprehensive tests)
// =============================================================================

#[test]
fn create_trigger_if_not_exists() {
    let sql = "create trigger if not exists tr_audit after insert on users begin insert into audit_log values (new.id, 'created'); end";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_trigger_update_of_columns() {
    let sql = "create trigger tr_update after update of name, email on users begin update users set updated_at = datetime('now') where id = new.id; end";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_trigger_instead_of() {
    let sql = "create trigger tr_view instead of insert on user_view begin insert into users (name) values (new.name); end";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_trigger_multiple_statements() {
    let sql = "create trigger tr_cascade after delete on users begin delete from orders where user_id = old.id; delete from sessions where user_id = old.id; insert into deleted_users values (old.id, datetime('now')); end";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_trigger_temp() {
    let sql = "create temp trigger tr_temp before insert on t begin select raise(abort, 'not allowed'); end";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// DDL - CREATE VIRTUAL TABLE
// =============================================================================

#[test]
fn create_virtual_table_fts5() {
    let sql = "create virtual table search using fts5(title, body, content='posts', content_rowid='id')";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn create_virtual_table_rtree() {
    let sql = "create virtual table locations using rtree(id, minX, maxX, minY, maxY)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// Comments with blank lines
// =============================================================================

#[test]
fn comment_blank_line_then_select() {
    let sql = "-- a comment\n\nselect * from queue2;";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn multiple_statements_with_comments_and_blank_lines() {
    let sql = "-- asdf\nselect 1 + 2;\n\n-- zxcv\nselect 3 + 4;";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// Complex queries
// =============================================================================

#[test]
fn recursive_cte_sudoku_solver() {
    let sql = "WITH RECURSIVE input(sud) AS ( SELECT '53..7....6..195....98....6.8...6...34..8.3..17...2...6.6....28....419..5....8..79' ), digits(z, lp) AS ( SELECT '1', 1 UNION ALL SELECT CAST(lp+1 AS TEXT), lp+1 FROM digits WHERE lp<9 ), x(s, ind) AS ( SELECT sud, instr(sud, '.') FROM input UNION ALL SELECT substr(s, 1, ind-1) || z || substr(s, ind+1), instr( substr(s, 1, ind-1) || z || substr(s, ind+1), '.' ) FROM x, digits AS z WHERE ind>0 AND NOT EXISTS ( SELECT 1 FROM digits AS lp WHERE z.z = substr(s, ((ind-1)/9)*9 + lp, 1) OR z.z = substr(s, ((ind-1)%9) + (lp-1)*9 + 1, 1) OR z.z = substr(s, (((ind-1)/3) % 3) * 3 + ((ind-1)/27) * 27 + lp + ((lp-1) / 3) * 6, 1) ) ) SELECT s FROM x WHERE ind=0";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

// =============================================================================
// Keywords as identifiers
// =============================================================================

#[test]
fn keyword_as_column_current() {
    let sql = "select current from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_filter() {
    let sql = "select filter from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_action() {
    let sql = "select action from events";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_key() {
    let sql = "select key, value from settings";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_replace() {
    let sql = "select replace from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_view() {
    let sql = "select view from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_query() {
    let sql = "select query from logs";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_row() {
    let sql = "select row from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_end() {
    let sql = "select end from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_release() {
    let sql = "select release from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_plan() {
    let sql = "select plan from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_partition() {
    let sql = "select partition from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_offset() {
    let sql = "select offset from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_default() {
    let sql = "select default from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_column() {
    let sql = "select column from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_begin() {
    let sql = "select begin from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_do() {
    let sql = "select do from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_nothing() {
    let sql = "select nothing from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_first() {
    let sql = "select first, last from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_conflict() {
    let sql = "select conflict from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_table_name() {
    let sql = "select * from action";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_table_alias() {
    let sql = "select filter.id from foo filter";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_qualified_column() {
    let sql = "select t.current, t.filter, t.action from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_in_where_clause() {
    let sql = "select * from t where current = 1 and action = 'click'";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_insert_column() {
    let sql = "insert into t (current, filter, action) values (1, 2, 3)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_update_target() {
    let sql = "update t set current = 1, action = 'done'";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_create_table_column() {
    let sql = "create table t (current text, filter integer, action text, key text primary key)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_function_call() {
    let sql = "select replace('hello world', 'world', 'there')";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_cte_name() {
    let sql = "with action as (select 1 as id) select * from action";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_in_order_by() {
    let sql = "select * from t order by current asc, action desc";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_in_group_by() {
    let sql = "select action, count(*) from events group by action";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_multiple_in_expression() {
    let sql = "select current + offset + row from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_subquery_column() {
    let sql = "select * from (select current, filter from t)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_in_case_expression() {
    let sql = "select case when current = 1 then action else nothing end from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_index_column() {
    let sql = "create index idx_action on events (action, current)";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_window_keywords_as_columns() {
    let sql = "select rows, range, groups, preceding, following, unbounded, exclude, ties, others from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_trigger_keywords_as_columns() {
    let sql = "select before, after, instead, each, for from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_transaction_keywords_as_columns() {
    let sql = "select begin, commit, rollback, savepoint, release, transaction, deferred, immediate, exclusive from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_constraint_keywords_as_columns() {
    let sql = "select constraint, primary, unique, check, foreign, references, autoincrement, collate from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_conflict_keywords_as_columns() {
    let sql = "select abort, fail, ignore, conflict, do, nothing from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_misc_keywords_as_columns() {
    let sql = "select generated, always, stored, analyze, explain, reindex, returning, vacuum, pragma, database, attach, detach from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_ddl_keywords_as_columns() {
    let sql = "select column, rename, trigger, virtual, temp, temporary, view, indexed, without, add from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_sort_keywords_as_columns() {
    let sql = "select asc, desc, nulls, first, last from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_join_keywords_as_columns() {
    let sql = "select inner, left, right, full, outer, cross, natural, using, join from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_comparison_keywords_as_columns() {
    let sql = "select glob, regexp, match, escape, like from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

#[test]
fn keyword_as_column_with_alias() {
    let sql = "select current as cur, action as act from t";
    let config = FormatConfig::default();
    assert_snapshot!(snapshot(sql, &config));
}

