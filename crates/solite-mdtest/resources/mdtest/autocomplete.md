# Autocomplete Tests

Test cases for SQL autocompletion.


## Empty state


### Completely empty

At the start of a new statement, don't suggest anything until they type something.

```sql
<ac1>
```

- `<ac1>`: 

### one letter

```sql
s<ac1>
```
- `<ac1>`: select, savepoint


## Table Completion

### After FROM

Suggest table names after `FROM` keyword.

```sql
create table students(
  student_id integer primary key,
  name text
);

create table classes(id integer, name text);

select * from <ac1>;
```

- `<ac1>`: students, classes

### After JOIN

Suggest table names after `JOIN` keyword.

```sql
create table users(id integer);
create table orders(id integer, user_id integer);

select * from users join <ac1>;
```

- `<ac1>`: orders, users

### After JOIN table name

Suggest ON keyword and AS for alias after join table reference.

```sql
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
  left join genz_actors <ac1>
)

select * from final;
```

- `<ac1>`: on, as

## Column Completion

### Unqualified Columns

Suggest columns from tables in scope.

```sql
create table users(id integer, name text, email text);

select <ac1> from users;
```

- `<ac1>`: id, name, email, rowid

### Qualified Columns

Suggest columns after table qualifier.

```sql
create table users(id integer, name text);
create table orders(id integer, total real);

select users.<ac1>, orders.<ac2>
from users, orders;
```

- `<ac1>`: id, name, rowid
- `<ac2>`: id, total, rowid

### With Alias

Suggest columns using table alias.

```sql
create table users(id integer, name text);

select u.<ac1> from users as u;
```

- `<ac1>`: id, name, rowid

## WHERE Clause

### Column suggestions in WHERE

```sql
create table products(id integer, name text, price real);

select * from products where <ac1>;
```

- `<ac1>`: id, name, price, rowid

### WHERE in CTE with JOIN

Suggest columns from both sides of a JOIN inside a CTE.

```sql
create table movies(id integer, name text, released_year integer);
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
  where <ac1>
)

select * from final;
```

- `<ac1>`: released_year, birth_year, movies_20s.id, genz_actors.id, movies_20s.name, genz_actors.name, movies_20s.rowid, genz_actors.rowid


## More

### lol

```sql
create table t(
 <ac1>
)
```

- `<ac1>`: 

### lol2

```sql
create table <ac1> t(
  a text,
  <ac2>
)
```

- `<ac1>`: if not exists
- `<ac2>`: 


### Column constraints after type

After a column type in CREATE TABLE, suggest column constraints.

```sql
create table t(
  aaa integer <ac1>,
  bbb text <ac2>,
  ccc blob <ac3>
);
```

- `<ac1>`: primary key, not null, unique, default, collate, references, check, generated always as, as
- `<ac2>`: primary key, not null, unique, default, collate, references, check, generated always as, as
- `<ac3>`: primary key, not null, unique, default, collate, references, check, generated always as, as

## Function Completion

### Functions in SELECT expression

Scalar functions should be suggested in SELECT column expressions after typing
at least one character.

```sql
select s<ac1> from generate_series(1, 10);
```

- `<ac1>`: substr, sqlite_version, sum, count, avg, min, max, length, typeof, abs, upper, lower, hex, quote, replace, trim, round, json, coalesce, iif, nullif, printf, unicode, zeroblob, likelihood, likely, unlikely

### Functions alongside columns

Functions should appear alongside columns when tables are in scope,
after typing at least one character.

```sql
create table items(id integer, name text);

select i<ac1> from items;
```

- `<ac1>`: id, name, rowid, substr, abs, length, upper, lower, typeof

### No functions without prefix

Without typing at least one character, only columns should be suggested.

```sql
create table products(id integer, name text, price real);

select <ac1> from products;
```

- `<ac1>`: id, name, price, rowid

### Functions in WHERE clause

Functions should be suggested in WHERE expressions after typing at least
one character.

```sql
create table products(id integer, name text, price real);

select * from products where l<ac1>;
```

- `<ac1>`: id, name, price, rowid, length, upper, lower, abs, typeof, coalesce

### JSON extract operators as operators

The `->` and `->>` operators should appear as operator suggestions after an expression,
not as function suggestions. This works in any expression context, not just WHERE.

```sql
select '{}' <ac1>;
```

- `<ac1>`: ->, ->>