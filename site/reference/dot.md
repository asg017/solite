# Dot Commands

Dot commands work in the REPL, in SQL scripts executed with `solite run`,
and in the Jupyter kernel. Run `.help` for the full list; sections for
commands not yet documented here are stubs.

## .describe

Describe a single table or view: kind and flags, a 10-row sample, columns
with declared types and affinity, foreign keys in both directions, indexes,
and the `CREATE` statement. Alias: `.d`.

```
.describe users          -- table in the 'main' schema
.describe temp.scratch   -- qualified: 'temp' or an attached schema
.describe "my table"     -- quoted names ("…", `…`, […])
```

Like `.tables`, unqualified names resolve in the `main` schema only — use
`temp.x` or `<attached>.x` for anything else. Views are never row-counted
(the header omits the count; the sample footer reads `N rows shown` instead
of `N of M rows`); row counts on tables and virtual tables cap at
`100,000+` rather than scanning the whole table. Hidden virtual-table
columns and generated columns are shown, flagged `hidden`, `generated
virtual`, or `generated stored`. Foreign keys are listed both ways —
declared on the table (`→`) and discovered by scanning the rest of the
schema for tables that reference it (`←`), the latter limited to the
resolved schema. `.describe` is not available in `solite test`.

```
> .describe accounts
main.accounts — table · 2 rows
┌─────┬──────────┬───────┐
│ id  │ owner_id │ label │
├─────┼──────────┼───────┤
│   1 │        1 │ a1    │
│   2 │        2 │ a2    │
└─────┴──────────┴───────┘
3 columns × 2 rows
2 rows shown

Columns
  id        INTEGER  PK
  owner_id  INTEGER
  label     TEXT  default 'default'

Foreign keys
  →  owner_id → main.users(id)  ON DELETE CASCADE
  ←  orders(account_id) → id

Indexes
  idx_accounts_owner  (owner_id)  unique, partial

DDL
  CREATE TABLE accounts (id INTEGER PRIMARY KEY, owner_id INTEGER REFERENCES users(id) ON DELETE CASCADE, label TEXT DEFAULT 'default');
```

In Jupyter, `.describe` renders as sectioned HTML instead of this text
layout — see [Jupyter Kernel](/jupyter#previewing-a-table).

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
.tables temp       -- list tables in another schema ('temp' or an attached one)
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
