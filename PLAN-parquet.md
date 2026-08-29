# Plan: Parquet exporter for `.export` and `solite query`

## Context

`.export <path>` and `solite query -o <path>` share one pipeline:
`format_from_path()` picks an `ExportFormat` from the extension,
`write_output()` streams rows from a `Statement` into a `Box<dyn Write>`
(local file, optionally gzip/zstd-wrapped by `output_from_path()`), and for
`s3://`/`t3://` targets `write_output_to_bytes()` buffers the whole export
and `object_store::upload()` does a single `put()`.

Parquet slots into that pipeline: `parquet::file::writer::SerializedFileWriter<W: Write + Send>`
writes strictly sequentially (footer last, no seeking), so it works against a
local file, stdout, the in-memory buffer used for S3 today, and the
`WriteMultipart` adapter proposed in `PLAN-multipart.md` later.

The one hard problem is typing: Parquet needs a fixed schema before the first
row, SQLite doesn't have one. See "Schema inference" below.

### Bug found during research: S3 export is compiled out of the binary

`crates/solite-cli/Cargo.toml` depends on `solite-core` with
`default-features = false` and only forwards `ritestream`. `object_store` is a
default feature of `solite-core`, so it's never enabled for the CLI:
`cargo tree -p solite-cli` has zero hits for `object_store v0.13`. In the
built binary, `.export s3://bucket/out.csv` falls through to
`File::create("s3://bucket/out.csv")` and fails with
`I/O error: No such file or directory`. Step 0 fixes this.

## Decisions (settled)

| Topic | Decision |
|---|---|
| Schema inference | Decltype affinity when declared; otherwise sniff the first row group. Later values that don't fit the chosen type are a hard error naming column and row. |
| Writer API | `parquet` crate, low-level `ColumnWriter` API, **no `arrow`** (`default-features = false, features = ["zstd"]`). ~50 crates / ~31s CPU release build vs 92 crates / ~101s with arrow. |
| Feature gate | `parquet = ["dep:parquet"]` feature on `solite-core`, in its default set, **explicitly forwarded from `solite-cli`'s default features** (same commit fixes `object_store` forwarding). |
| S3 | Fix feature forwarding; keep today's buffered `put()`. Multipart streaming remains a separate follow-up (`PLAN-multipart.md`); the parquet writer is `W: Write`-generic so nothing changes when that lands. |
| Surface | `.export x.parquet`, `solite query -o x.parquet`, and `solite query -f parquet` (stdout OK). `solite exec` stays JSON-only. |
| Compression | Internal ZSTD (level 3) by default. `x.parquet.gz` / `x.parquet.zst` are rejected with an explicit error — never wrap a parquet file, never produce a file whose extension lies. |
| Nullability | Every column is `OPTIONAL`; SQL NULL → Parquet null. |
| Type map | INTEGER→INT64, REAL→DOUBLE, TEXT→BYTE_ARRAY(UTF8), BLOB→BYTE_ARRAY, BOOL/BOOLEAN decltype→BOOLEAN. |
| JSON subtype | If the column's type was decided from a value carrying SQLite subtype `'J'` (74), tag the column `LogicalType::Json` (still UTF8 bytes). Mixed columns fall back to plain UTF8. |
| Dates/times | DATE/DATETIME/TIMESTAMP decltypes export as UTF8 strings exactly as stored. No parsing in v1. |
| Widening | INTEGER values in a DOUBLE column are converted to f64, not errors. |
| BOOLEAN values | Must be integer 0/1; anything else is a mismatch error. |
| Blob limit | The existing `--blob-limit` / `BlobLimit::Default` (10 MiB for file formats) applies to BLOB cells. |
| Tests | Rust round-trip tests using `parquet::file::reader` (no arrow), plus `pyarrow` added to `tests/pyproject.toml` with syrupy snapshots of `pq.read_table(...).to_pylist()`. No byte-for-byte parquet snapshots. |

## Schema inference

Parquet needs one physical/logical type per column up front. SQLite gives us
two signals:

1. `sqlite3_column_decltype` — present for direct base-table column
   references (exposed as `ColumnMeta::decltype`), absent for expressions,
   aggregates, CTE/subquery outputs, and `Buffered` (remote) statements when
   the server didn't send it.
2. The actual values.

Algorithm, run once per export before the first row group is written:

```
for each column i:
  if decltype is Some(d):
      affinity = sqlite_affinity(d)          // SQLite's 5-rule algorithm, plus BOOL/BOOLEAN → Boolean
      type = match affinity { Integer→Int64, Real→Double, Text→Utf8, Blob→Bytes, Boolean→Boolean, Numeric→sniff }
  else:
      type = sniff(column i over the buffered first row group)

sniff(values):
  ignore NULLs
  all Int                   → Int64
  Int/Double mix            → Double
  all Text, every value has JSON subtype → Utf8 + LogicalType::Json
  all Text                  → Utf8
  all Blob                  → Bytes
  anything else / all NULL  → Utf8   (mixed → strings, as CSV does)
```

`NUMERIC` affinity (e.g. `DECIMAL(10,2)`, `NUMERIC`) is treated like "no
decltype" and sniffed, since SQLite may store ints or reals there.

Type-check on every value during writing (`check_value(column, row_index, value, type)`):

| Column type | Accepted values | Action |
|---|---|---|
| Int64 | Int | write |
| Double | Double, Int | Int widened to f64 |
| Utf8 (± Json) | Text | write bytes; Text must be valid UTF-8 → else `ExportError::InvalidUtf8` |
| Utf8 | Int, Double | **error** (mismatch) — a declared TEXT column with an int is a data bug the user wants to know about |
| Bytes | Blob | write (after blob-limit check) |
| Boolean | Int 0 / Int 1 | write |
| any | NULL | def level 0 |
| otherwise | | `ExportError::ParquetTypeMismatch { column, row, expected, found }` |

The first row group is buffered as `Vec<Vec<OwnedValue>>` (needed for
sniffing anyway); subsequent row groups stream — rows are appended to
per-column typed buffers (`Vec<i64>` + `Vec<i16>` def levels, etc.) and
flushed every `ROW_GROUP_ROWS` rows. Memory is bounded by one row group.

`ROW_GROUP_ROWS = 65_536` as a `const` in `exporter/parquet.rs`. Not
user-configurable in v1.

Empty result set: schema is still built (decltype or Utf8 fallback), a valid
zero-row parquet file is written.

## Implementation steps

Each step is independently committable; 0 is a prerequisite bug fix.

### 0. Fix `object_store` feature forwarding (bug fix, separate commit)

- `crates/solite-cli/Cargo.toml`: `default = ["ritestream", "object_store"]`,
  `object_store = ["solite-core/object_store"]`.
- Rust test in `solite-core/src/dot/export.rs` (behind
  `#[cfg(feature = "object_store")]`): with `AWS_ACCESS_KEY_ID` unset,
  `.export s3://bucket/x.csv` fails with the "AWS_ACCESS_KEY_ID environment
  variable is not set" error, *not* an ENOENT.
- pytest in `tests/test_query.py` or a new `tests/test_export.py`: run
  `solite run` on a script with `.export s3://bucket/x.csv` with AWS env vars
  scrubbed; assert the error message mentions `AWS_ACCESS_KEY_ID`. This is
  what would have caught the missing forwarding.

### 1. Cargo wiring

- `crates/solite-core/Cargo.toml`:
  ```toml
  [features]
  default = ["ritestream", "object_store", "parquet"]
  parquet = ["dep:parquet"]

  [dependencies]
  parquet = { version = "59", default-features = false, features = ["zstd"], optional = true }
  ```
  The `zstd` feature reuses the `zstd` crate already in the tree. No
  `snap`/`brotli`/`lz4`/`flate2-zlib-rs`.
- `crates/solite-cli/Cargo.toml`: `default = ["ritestream", "object_store", "parquet"]`,
  `parquet = ["solite-core/parquet"]`.
- Verify with `cargo tree -p solite-cli | grep -c "parquet v59"` ≥ 1.

### 2. `ExportFormat::Parquet` and path handling (`exporter.rs`)

- Add `ExportFormat::Parquet` (gated `#[cfg(feature = "parquet")]`).
- Replace `format_from_path(&Path) -> Option<ExportFormat>` with:
  ```rust
  pub enum FormatFromPathError { Unknown(String), CompressedParquet(String) }
  pub fn format_from_path(path: &Path) -> Result<ExportFormat, FormatFromPathError>
  ```
  - `.parquet` → `Parquet`
  - `.parquet.gz` / `.parquet.zst` → `Err(CompressedParquet)` with message
    "parquet has built-in compression; write to `x.parquet` instead of `x.parquet.gz`"
  - unknown → `Err(Unknown)` (today's `None`)
  - Callers: `dot/export.rs` maps both errors to `DotError::InvalidData`;
    `query.rs::determine_format` maps `Unknown` → `Json` (preserving today's
    silent fallback) and propagates `CompressedParquet` as a `QueryError`.
- `BlobLimit::resolve`: Parquet uses `DEFAULT_FILE_BLOB_LIMIT` (falls into the
  existing `_ =>` arm; add it to the test loop in
  `test_blob_limit_resolve_defaults`).
- `write_output` / `write_output_to_bytes`: add the `Parquet` arm calling
  `parquet::write_parquet(stmt, output, limit)`.
- `output_from_path`: unchanged — with `.parquet.gz` rejected upstream it can
  never wrap a parquet stream.

### 3. The writer: `crates/solite-core/src/exporter/parquet.rs` (new, `#[cfg(feature = "parquet")]`)

Move `exporter.rs` to `exporter/mod.rs` and add `exporter/parquet.rs`.

```rust
pub(super) fn write_parquet<W: Write + Send>(
    stmt: &mut Statement,
    output: W,
    blob_limit: Option<u64>,
) -> Result<(), ExportError>
```

Internals:

- `enum ColumnType { Int64, Double, Utf8 { json: bool }, Bytes, Boolean }`
- `fn affinity_from_decltype(&str) -> Affinity` — SQLite's documented rules
  (contains "INT" → Integer; "CHAR"/"CLOB"/"TEXT" → Text; "BLOB" or empty →
  Blob; "REAL"/"FLOA"/"DOUB" → Real; else Numeric) with a `BOOL` check first.
- `fn sniff(values: impl Iterator<Item=&OwnedValueWithSubtype>) -> ColumnType`
- `fn build_schema(names, types) -> Arc<Type>` — group type with one
  `OPTIONAL` primitive per column; `Utf8` gets `LogicalType::String`,
  `Utf8 { json: true }` gets `LogicalType::Json`.
- `WriterProperties::builder().set_compression(Compression::ZSTD(ZstdLevel::try_new(3)?)).set_created_by(format!("solite {}", env!("CARGO_PKG_VERSION")))`
- Per-column typed buffers, one `ColumnBuffer` enum with `push(value)`,
  `flush(&mut SerializedRowGroupWriter)`.
- Row loop: `stmt.next()` → `check_blob_limit` (existing) → `check_value` →
  push; every `ROW_GROUP_ROWS` rows flush a row group; flush the tail;
  `writer.close()`.
- Because `ValueRefX` borrows the statement, the first row group is copied
  into `OwnedValue`s (plus the JSON subtype flag, captured before stepping).
  Note `OwnedValue::from_value_ref` drops the subtype — capture
  `value.subtype() == Some(74)` alongside it.

New `ExportError` variants:
- `Parquet(parquet::errors::ParquetError)` (+ `From`)
- `ParquetTypeMismatch { column: String, row: usize, expected: &'static str, found: &'static str }`
  with a Display like: `column 'age' (row 1041) is text, but the column was
  typed INT64 from its declared type 'INTEGER'` — include *why* the type was
  chosen (declared vs inferred) so the user knows whether to fix data or
  cast in the query.

### 4. CLI surface

- `cli.rs`: `QueryFormat::Parquet` (gated) + `From` arm; update the `-o` doc
  comment to list `.parquet`.
- `query.rs`: `determine_format` error handling per step 2. No TTY guard
  for `-f parquet` (consistent with `-o /dev/stdout` already being possible;
  csv/json don't guard either).
- `dot/help.rs`: export description → "Export query results to a file
  (format from extension: csv, tsv, json, ndjson, parquet; .gz/.zst
  supported for text formats)".
- `site/reference/dot.md`: add a `## .export` section (currently missing
  entirely) covering formats, compression, parameter substitution in paths,
  `s3://`/`t3://` targets and env vars, and the parquet typing rules above in
  user terms.

### 5. Tests

Rust (`exporter/parquet.rs` `#[cfg(test)]`), reading back with
`parquet::file::reader::{FileReader, SerializedFileReader}` and
`get_row_iter` (available without `arrow`):

- declared types: `create table t(a integer, b real, c text, d blob, e boolean)` →
  physical types INT64/DOUBLE/BYTE_ARRAY(String)/BYTE_ARRAY/BOOLEAN, all OPTIONAL
- sniffed expression columns: `select count(*), 1.5*2, 'x', x'00'`
- int/real mix → DOUBLE with widening; declared REAL holding `1` → 1.0
- NULLs in every type; all-NULL sniffed column → UTF8
- JSON subtype: `select json_object('a',1) as j` → `LogicalType::Json`;
  mixed json/plain text → plain String
- mismatch errors: declared INTEGER with `'abc'` in row N; BOOLEAN with `2`;
  message names column, row, expected/found, and declared-vs-inferred
- blob over limit → existing `BlobTooLarge`
- zero rows → valid file, correct schema
- multi-row-group: > `ROW_GROUP_ROWS` rows (use a small const override
  via `#[cfg(test)]` or a `write_parquet_with_group_size` inner fn) →
  `metadata.num_row_groups() == 2`, all rows read back
- `format_from_path`: `x.parquet` ok, `x.parquet.gz`/`x.parquet.zst` →
  `CompressedParquet`, `x.PARQUET`? (keep case-sensitive, matching csv today)
- `write_output_to_bytes` with Parquet produces `PAR1` magic at both ends
  (covers the S3 buffered path without network)

pytest (`tests/test_query.py` or new `tests/test_export.py`; add
`pyarrow>=17` to `tests/pyproject.toml`):

- `solite q "select * from json_tree('[1,2,3,4]')" -o a.parquet` →
  `pq.read_table("a.parquet").to_pylist() == snapshot`
- `solite q ... -f parquet` → stdout bytes parse with `pq.read_table(io.BytesIO(...))`
- `solite run` script with `.export out.parquet` + `-- name:` procedure params
- `-o a.parquet.gz` → non-zero exit, message mentions built-in compression
- from step 0: `.export s3://bucket/x.csv` with AWS env scrubbed →
  credentials error, not ENOENT

Run everything through `make test` (cargo + pytest + snap suites).

## Files

| File | Change |
|---|---|
| `crates/solite-cli/Cargo.toml` | forward `object_store` and `parquet` features |
| `crates/solite-core/Cargo.toml` | `parquet` feature + optional dep |
| `crates/solite-core/src/exporter.rs` → `exporter/mod.rs` | `ExportFormat::Parquet`, `FormatFromPathError`, new `ExportError` variants, dispatch arms |
| `crates/solite-core/src/exporter/parquet.rs` | new: schema inference + writer |
| `crates/solite-core/src/dot/export.rs` | error mapping for `FormatFromPathError`; S3 regression test |
| `crates/solite-core/src/dot/help.rs` | `.export` description |
| `crates/solite-cli/src/cli.rs` | `QueryFormat::Parquet`, `-o` help text |
| `crates/solite-cli/src/commands/query.rs` | `determine_format` error propagation |
| `site/reference/dot.md` | new `## .export` section |
| `tests/pyproject.toml`, `tests/test_export.py` | pyarrow + CLI tests |

Not touched: `run/dot.rs`, `repl/mod.rs`, `jupyter/handlers.rs`,
`test/mod.rs`, `snap/file.rs` — they call `cmd.execute()` and print
`cmd.target`, which is format-agnostic.

## Out of scope / follow-ups

- Streaming multipart S3 upload (`PLAN-multipart.md`) — parquet is ready for it.
- Timestamp/Date logical types for DATE/DATETIME columns (opt-in flag later).
- User-configurable codec / row-group size (`.export --compression snappy`).
- Reading parquet via replacement scans (`select * from 'x.parquet'`) — would
  motivate adding the `arrow` feature at that point.
- `DECIMAL` logical type for `NUMERIC(p,s)` decltypes.
