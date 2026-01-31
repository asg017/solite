# Hover Tests

Test cases for hover information.

## Table Hover

### Basic table hover

Hover over a table name shows table information.

```sql
create table users(
  id integer primary key,
  name text,
  email text
);

select * from users<hv1>;
```

- `<hv1>`: "users"

### Table with doc comment

Hover should show table documentation.

```sql
create table students(
  --! All students at Foo University.
  --! @details https://foo.edu/students

  student_id integer primary key,
  name text
);

select * from students<hv1>;
```

- `<hv1>`: "students", "All students at Foo University"

## Column Hover

### Basic column hover

Hover over a column name shows column info.

```sql
create table users(
  id integer primary key,
  name text
);

select id<hv1> from users;
```

- `<hv1>`: "id"

### Qualified column hover

Hover over qualified column reference.

```sql
create table users(
  id integer primary key
);

select users.id<hv1> from users;
```

- `<hv1>`: "id"
