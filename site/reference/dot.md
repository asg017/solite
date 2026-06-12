# Dot Commands

Dot commands work in the REPL, in SQL scripts executed with `solite run`,
and in the Jupyter kernel. Run `.help` for the full list; sections for
commands not yet documented here are stubs.

## .schema

Show CREATE statements for the current database.

```
.schema            -- all objects
.schema users      -- only `users` and objects on it (indexes, triggers)
.schema idx_%      -- LIKE pattern matching, as in sqlite3
```

The optional argument is a `LIKE` pattern (`%` and `_` wildcards) matched
against both the object name and the table it belongs to, so `.schema users`
also prints the indexes and triggers on `users`.

Statements are printed in creation order (tables before the indexes,
triggers, and views that reference them) and every statement is terminated
with `;`, so the output can be pasted back into the REPL or a `.sql` file
and executed as-is:

```
> .schema
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
CREATE INDEX idx_users_name ON users(name);
CREATE VIEW v_users AS SELECT * FROM users;
```

Shadow tables backing virtual tables (e.g. the `notes_data`/`notes_idx`
tables behind an fts5 table) are included in the dump, matching
`sqlite3 .schema`.

## .tables

List tables, views, and virtual tables.

```
.tables            -- list tables in the 'main' schema
.tables temp       -- list tables in an attached schema
```

Virtual tables (fts5, R*Tree, extension modules) are listed; their shadow
tables and `sqlite_%` internal tables are hidden, matching `sqlite3
.tables`:

```
> .tables
notes
users
v_users
```

Here `notes` is an fts5 virtual table — it appears, while its
`notes_data`, `notes_idx`, `notes_content`, `notes_docsize`, and
`notes_config` shadow tables do not.
