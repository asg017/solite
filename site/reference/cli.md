# CLI Reference

Reference documentation for the `solite` command line interface. Sections
for commands not yet documented here are stubs; run `solite <command> --help`
for the authoritative flag list.

## solite schema

Print the schema of a database.

```
solite schema <DATABASE> [PATTERN] [--format <sql|json>]
```

### Arguments

- `<DATABASE>` — database file to print CREATE statements for. The file is
  opened **read-only**; a path that does not exist is an error (exit code 1)
  and no file is created. `ssh://` URLs are supported with the top-level
  `--allow-ssh` flag.
- `[PATTERN]` — optional `LIKE` pattern (`%` and `_` wildcards). Only objects
  whose name *or owning table* matches are shown, so a table pattern also
  prints the table's indexes and triggers, like sqlite3's `.schema ?PATTERN?`.
- `--format <sql|json>` (`-f`) — output format, default `sql`.

### Output

With the default `sql` format, CREATE statements are printed in **creation
order** (sqlite_master rowid order, matching `sqlite3 .schema`), each
terminated with `;`. Creation order means tables always precede the indexes,
triggers, and views that reference them, so the dump is replayable into a
fresh database. Shadow tables backing virtual tables are included, matching
sqlite3.

```
$ solite schema app.db
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
CREATE INDEX idx_users_name ON users(name);
CREATE VIEW v_users AS SELECT * FROM users;
CREATE VIRTUAL TABLE notes USING fts5(body);
CREATE TABLE 'notes_data'(id INTEGER PRIMARY KEY, block BLOB);
CREATE TABLE 'notes_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;
CREATE TABLE 'notes_content'(id INTEGER PRIMARY KEY, c0);
CREATE TABLE 'notes_docsize'(id INTEGER PRIMARY KEY, sz BLOB);
CREATE TABLE 'notes_config'(k PRIMARY KEY, v) WITHOUT ROWID;
```

Filter to one table and the objects on it:

```
$ solite schema app.db users
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
CREATE INDEX idx_users_name ON users(name);
```

Because the dump is replayable, it round-trips into a new database:

```
$ solite schema app.db > schema.sql
$ solite run fresh.db schema.sql
```

### JSON output

`--format json` emits a deterministic, machine-readable description of the
schema (objects sorted by name) with tables, columns, views, indexes, and
triggers. Each object carries its original CREATE statement in `sql`.
Useful for CI schema-change checks, docs generation, and structural diffing
with `jq`. The pattern argument is not supported with `--format json`.

Column metadata (declared type, primary key membership, NOT NULL) comes
from `PRAGMA table_info`; `type` is `null` when the declaration omits it
(e.g. fts5 virtual table columns).

```
$ solite schema app.db --format json
{
  "tables": [
    ...
    {
      "name": "users",
      "columns": [
        {
          "name": "id",
          "type": "INTEGER",
          "primary_key": true,
          "not_null": false
        },
        {
          "name": "name",
          "type": "TEXT",
          "primary_key": false,
          "not_null": false
        }
      ],
      "without_rowid": false,
      "sql": "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)"
    }
  ],
  "views": [...],
  "indexes": [...],
  "triggers": [...]
}
```

### Exit codes

- `0` — success (including an empty database, which prints nothing).
- `1` — missing file, unreadable/corrupt database, or query failure. A
  typo'd path never creates an empty database file:

```
$ solite schema nope.db
Error: no such file: nope.db
$ echo $?
1
```

## solite run

Execute a SQL file against a database. See `solite run --help`.

## solite query

Run a single query and print results. See `solite query --help`.

## solite repl

Start an interactive REPL. See `solite repl --help`.
