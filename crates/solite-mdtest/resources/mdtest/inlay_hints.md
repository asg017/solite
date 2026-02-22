# Inlay Hint Tests

Tests for INSERT VALUES column inlay hints.

## Basic Cases

### Column hints with reordering

Hints should show the INSERT column order, not table definition order.

```sql
create table users(id integer primary key, name text, email text);

insert into users(email, name, id) values (
  'alice@example.com', -- inlay: "email"
  'Alice',             -- inlay: "name"
  1                    -- inlay: "id"
);
```

### Multiple rows

Each row should get its own hints.

```sql
create table points(x real, y real);

insert into points(y, x) values
  (1.0, 2.0), -- inlay: "y"
  (3.0, 4.0), -- inlay: "y"
  (5.0, 6.0); -- inlay: "y"
```

### Single line values

```sql
create table t(a, b, c);

insert into t(c, b, a) values (1, 2, 3); -- inlay: "c"
```

## No Hints Cases

### Without explicit columns

No hints when columns are not explicitly specified.

```sql
create table t(a, b, c);

insert into t values (1, 2, 3);
```

### DEFAULT VALUES

```sql
create table t(a default 1, b default 2);

insert into t default values;
```

### INSERT with SELECT

No hints for INSERT...SELECT (future enhancement).

```sql
create table src(x, y);
create table dst(a, b);

insert into dst(a, b) select x, y from src;
```

## Partial Columns

### Subset of columns

```sql
create table t(a, b, c default 0);

insert into t(b, a) values (
  10, -- inlay: "b"
  20  -- inlay: "a"
);
```

## Complex Expressions

### Expressions as values

```sql
create table t(total real, description text);

insert into t(description, total) values (
  'Item: ' || 'Widget', -- inlay: "description"
  100.0 * 1.1           -- inlay: "total"
);
```

### Subquery as value

```sql
create table counts(n integer);
create table t(value integer);

insert into t(value) values (
  (select max(n) from counts) -- inlay: "value"
);
```

### Function call as value

```sql
create table t(ts text, data text);

insert into t(data, ts) values (
  upper('hello'), -- inlay: "data"
  datetime('now') -- inlay: "ts"
);
```
