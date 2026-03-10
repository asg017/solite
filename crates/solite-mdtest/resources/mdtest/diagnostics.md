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

## Function Argument Count

### Too few arguments

```sql
select abs(); -- error: "abs() expects 1 arguments, but 0 were provided"
```

### Too many arguments

```sql
select length('hello', 'world'); -- error: "length() expects 1 arguments, but 2 were provided"
```

### Wrong count for multi-arity function

`substr` accepts 2 or 3 arguments.

```sql
select substr('hello'); -- error: "substr() expects 2 or 3 arguments, but 1 were provided"
```

### Correct argument counts

```sql
select abs(-1); -- ok
select length('hello'); -- ok
select substr('hello', 2); -- ok
select substr('hello', 2, 3); -- ok
select replace('hello', 'l', 'r'); -- ok
select round(3.14); -- ok
select round(3.14, 1); -- ok
select count(*); -- ok
select count(1); -- ok
```

### Variadic functions are not flagged

Functions like `printf` and `coalesce` accept variable arguments.

```sql
select coalesce(1, 2); -- ok
select coalesce(1, 2, 3, 4); -- ok
select printf('%d', 1); -- ok
select printf('%d %d', 1, 2); -- ok
```

## Table-Valued Functions

### Aliased table function reference

Table-valued functions with aliases should not produce "table not found" errors.

```sql
create table t(a);
select li.value from t, json_each(t.a) as li; -- ok
```

### Table function with qualified column and JSON operators

```sql
create table t(a);
select
  li.value ->> 'date' as date
from t, json_each(t.a -> 'line_items') as li; -- ok
```

### CREATE TABLE AS SELECT

```sql
create table t3 as select 1 as value; -- ok
```

## Parse Errors

Tests for parse error detection are handled separately since
the parser returns errors, not diagnostics.
