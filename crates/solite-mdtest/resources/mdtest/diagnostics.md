# Diagnostics Tests

Test cases for parse errors and semantic diagnostics.

## Semantic Errors

### Unknown table

Reference to non-existent table produces a diagnostic.

```sql
create table users(id integer);
select * from nonexistent; -- error: "Unknown table"
```

### Valid table reference

Reference to existing table is OK.

```sql
create table users(id integer);
select * from users; -- ok
```

## Valid SQL

### No errors expected

Simple valid SQL should produce no diagnostics.

```sql
create table users(id integer primary key, name text);
select * from users; -- ok
insert into users(id, name) values (1, 'Alice'); -- ok
```

## Parse Errors

Tests for parse error detection are handled separately since
the parser returns errors, not diagnostics.
