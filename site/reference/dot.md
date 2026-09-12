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

## .export

Export the result of a query to a file. The path goes on the `.export`
line; the query follows on the lines after it:

```
.export users.csv
SELECT * FROM users;
```

The format is picked from the target's extension:

| Extension | Format |
|---|---|
| `.csv` | Comma-separated values |
| `.tsv` | Tab-separated values |
| `.json` | JSON array of objects |
| `.ndjson` | Newline-delimited JSON |
| `.geojson` | GeoJSON `FeatureCollection` |
| `.geojsonl` / `.ndgeojson` | Newline-delimited GeoJSON, one `Feature` per line |
| `.geojsons` | RFC 8142 GeoJSON text sequence |
| `.parquet` | Apache Parquet |

Text formats (`.csv`, `.tsv`, `.json`, `.ndjson`, `.geojson`, `.geojsonl`,
`.geojsons`) can be compressed by adding `.gz` or `.zst` to the path, e.g.
`.export users.csv.gz`. Parquet
has its own built-in compression (internal ZSTD), so `.export
out.parquet.gz` is rejected outright rather than double-compressing or
silently ignoring the extra suffix — write to `out.parquet` instead.

The path supports `:param` substitution from values set with `.param
set`:

```
.param set date 2024-01-01
.export report_:date.csv
SELECT * FROM orders WHERE created_at >= '2024-01-01';
```

Targets can also be `s3://bucket/key` or `t3://bucket/key` URLs, uploaded
directly instead of written to a local file. Credentials come from
`AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`; the endpoint defaults to
Tigris (`https://t3.storage.dev`) and can be overridden with
`AWS_ENDPOINT_URL_S3`, and the region with `AWS_REGION` (defaults to
`auto`):

```
.export s3://my-bucket/exports/users.parquet
SELECT * FROM users;
```

BLOB cells over 10 MiB fail the export with an error naming the column,
size, and limit; pass `--blob-limit` on `solite query` for that path (the
`.export` dot command always uses the 10 MiB default).

### Parquet typing

Parquet needs one fixed type per column before the first row is written,
so `.export out.parquet` infers a schema:

- If the column comes straight from a table (not an expression) and that
  column has a declared type, the declared type wins: `INTEGER` → int64,
  `REAL` → double, `TEXT` → string, `BLOB` → binary, `BOOLEAN`/`BOOL` →
  bool. `NUMERIC`/`DECIMAL` columns are treated as undeclared and sniffed.
- Otherwise (expressions, aggregates, CTE/subquery output, or no declared
  type) the type is sniffed from the first 65,536 rows: all-integer
  columns become int64, a mix of integers and reals becomes double, text
  becomes string, blobs become binary, and anything else — including an
  all-NULL column — falls back to string. In that inferred string column,
  later integers, reals, or blobs are written in their text form (`42`,
  `1.5`, `x'00ff'`) rather than rejected — it's the sniffing that put them
  in a string column in the first place. A **declared** `TEXT` column has
  no such leniency: a non-text value there is still a type mismatch.
- A later value that doesn't fit the chosen type fails the whole export
  with an error naming the column, the row number, and why the column was
  typed that way, e.g. `column 'a' (row 3) is text, but the column was
  typed INT64 from its declared type 'INTEGER'`.
- Integers written into a double/REAL column are widened to floats, not
  rejected.
- `BOOLEAN` columns only accept integer `0`/`1`; anything else is a type
  mismatch.
- Values produced by `json()`/`json_object()`/`json_array()` (SQLite's
  JSON subtype) are written as UTF-8 text tagged with Parquet's JSON
  logical type, so readers that understand it (e.g. pyarrow) can tell the
  column apart from plain text. A column that mixes JSON and non-JSON text
  falls back to plain text.
- `DATE`/`DATETIME`/`TIMESTAMP` columns are exported as plain text, exactly
  as SQLite stored them — no parsing or reformatting.
- Every column is nullable; SQL `NULL` becomes a Parquet null regardless
  of type.

### GeoJSON

`.export out.geojson` (or `.geojsonl`/`.geojsons`) writes one GeoJSON
`Feature` per row:

- The geometry comes from the column named `geometry` (case-insensitive);
  every other column, `id` included, becomes a `properties` member using
  the same value rules as `.json` — JSON-typed text (e.g. from `json()`)
  nests as an object/array, BLOBs are base64-encoded.
- The `geometry` column is required and must already hold GeoJSON text: a
  `json()`/`json_object()`/`->` result, the `geometry` column of a
  jsonx0 GeoJSON virtual table, or the output of sqlite-tg's
  `tg_to_geojson()`. sqlite-tg isn't bundled with Solite yet, so load it
  explicitly with `.load` if you're converting from WKB/WKT geometry
  columns. A plain WKT string or a BLOB (WKB) in the geometry column is
  rejected with an error pointing at `tg_to_geojson()` — neither is
  decoded in v1.
- A `NULL` geometry produces a `Feature` with `"geometry":null` (an
  unlocated feature) rather than an error.
- An empty result set exports `{"type":"FeatureCollection","features":[]}`
  for `.geojson`, or an empty file for `.geojsonl`/`.geojsons`.
- `.geojsonl` writes one `Feature` object per line and no surrounding
  `FeatureCollection`; `.geojsons` is the same but prefixes each line with
  the ASCII record separator (`0x1E`), per RFC 8142.
- `.gz`/`.zst` compression and `s3://`/`t3://` targets work the same as
  for the other text formats.

```
.export places.geojson
select id, name, population, geometry from places;
```

```
.load ./tg0
.export s3://bucket/parcels.geojsonl.gz
select apn, tg_to_geojson(geom) as geometry from parcels;
```

By default the geometry column must be named `geometry` and every other
column (including `id`) stays in `properties`. `--geometry <col>` and
`--id <col>` (space or `--flag=value` form) override that:

```
.export out.geojson --geometry geom --id apn
select apn, name, geom from parcels;
```

`--id` lifts that column to the Feature's top-level `id` member and
removes it from `properties`; the value must be a string or a number
(RFC 7946 §3.2) — `NULL` omits the `id` member entirely, and a real or
BLOB value is an error. The flags work the same way with `solite query -f
geojson*`/`-o x.geojson*` (see below). The `.export` line is only
tokenized for flags when it contains ` --` or starts with `--`, so a bare
target path with spaces still works untouched; a path that itself
contains ` --` must be quoted, e.g. `.export "a --b.csv"`.

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
