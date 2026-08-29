//! Apache Parquet export.
//!
//! Parquet needs one fixed physical/logical type per column before the
//! first row is written, which SQLite's dynamic typing doesn't give us for
//! free. Column types come from two signals, in order:
//!
//! 1. [`crate::sqlite::ColumnMeta::decltype`] (present for direct base-table
//!    column references) mapped through SQLite's storage-class affinity
//!    rules ([`affinity_from_decltype`]).
//! 2. Otherwise (or for `NUMERIC` affinity, which has no fixed storage
//!    class), the values in the first row group are sniffed ([`sniff`]).
//!
//! Every column is written `OPTIONAL` (SQL `NULL` -> Parquet null). A value
//! that doesn't fit the chosen type once writing is underway is a hard
//! error ([`ExportError::ParquetTypeMismatch`]) naming the column, the
//! 1-based row, and *why* the type was chosen, so the caller knows whether
//! to fix the data or `CAST` in the query.
//!
//! Memory is bounded to one row group: the first group is buffered (needed
//! for sniffing anyway), subsequent rows are streamed and flushed every
//! [`ROW_GROUP_ROWS`] rows.

use std::io::Write;
use std::sync::Arc;

use parquet::basic::{Compression, LogicalType, Repetition, Type as PhysicalType, ZstdLevel};
use parquet::data_type::{BoolType, ByteArray, ByteArrayType, DoubleType, Int64Type};
use parquet::file::properties::WriterProperties;
use parquet::file::writer::{SerializedFileWriter, SerializedRowGroupWriter};
use parquet::schema::types::Type;

use super::{check_blob_limit, ExportError};
use crate::sqlite::{OwnedValue, Statement, ValueRefX, ValueRefXValue};

/// Number of rows buffered per Parquet row group. Not user-configurable in
/// v1; see [`write_parquet_with_group_size`] for the test-only override.
pub(super) const ROW_GROUP_ROWS: usize = 65_536;

/// SQLite's JSON subtype tag ('J'), mirroring the check in
/// `super::value_to_json`.
const JSON_SUBTYPE: u32 = 74;

/// Write statement results as Parquet to `output`.
///
/// `blob_limit` bounds the raw size of any BLOB cell, same as the other
/// formats (see [`super::check_blob_limit`]).
pub(super) fn write_parquet<W: Write + Send>(
    stmt: &mut Statement,
    output: W,
    blob_limit: Option<u64>,
) -> Result<(), ExportError> {
    write_parquet_with_group_size(stmt, output, blob_limit, ROW_GROUP_ROWS)
}

/// The Parquet physical (+ logical) type resolved for one output column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColumnType {
    Int64,
    Double,
    /// `json` tags the column `LogicalType::Json` (still UTF8 bytes); set
    /// only when every sniffed value carried SQLite's JSON subtype.
    Utf8 { json: bool },
    Bytes,
    Boolean,
}

impl ColumnType {
    /// Name used in [`ExportError::ParquetTypeMismatch`] messages.
    fn expected_name(self) -> &'static str {
        match self {
            ColumnType::Int64 => "INT64",
            ColumnType::Double => "DOUBLE",
            ColumnType::Utf8 { .. } => "UTF8",
            ColumnType::Bytes => "BYTE_ARRAY",
            ColumnType::Boolean => "BOOLEAN",
        }
    }
}

/// SQLite storage-class affinity, per <https://www.sqlite.org/datatype3.html> §3.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Affinity {
    Integer,
    Real,
    Text,
    Blob,
    /// No fixed storage class (e.g. `NUMERIC`, `DECIMAL(10,2)`): SQLite may
    /// store either an integer or a real here, so it's treated like "no
    /// decltype" and sniffed.
    Numeric,
    /// Not one of SQLite's five affinities; solite's own convention for
    /// `BOOL`/`BOOLEAN` declared columns.
    Boolean,
}

/// Map a declared column type to a storage-class affinity, case-insensitive.
fn affinity_from_decltype(decl: &str) -> Affinity {
    let d = decl.to_ascii_uppercase();
    if d.contains("BOOL") {
        Affinity::Boolean
    } else if d.contains("INT") {
        Affinity::Integer
    } else if d.contains("CHAR") || d.contains("CLOB") || d.contains("TEXT") {
        Affinity::Text
    } else if d.contains("BLOB") || d.is_empty() {
        Affinity::Blob
    } else if d.contains("REAL") || d.contains("FLOA") || d.contains("DOUB") {
        Affinity::Real
    } else {
        Affinity::Numeric
    }
}

/// A value copied out of a row, keeping the JSON subtype flag that
/// [`OwnedValue::from_value_ref`] drops.
struct Cell {
    value: OwnedValue,
    /// `true` if this was a `Text` value carrying SQLite's JSON subtype.
    json: bool,
}

impl Cell {
    fn from_value_ref(v: &ValueRefX) -> Self {
        let json = matches!(v.value, ValueRefXValue::Text(_)) && v.subtype() == Some(JSON_SUBTYPE);
        Cell {
            value: OwnedValue::from_value_ref(v),
            json,
        }
    }
}

/// Infer a column type from a set of sampled values, ignoring NULLs.
///
/// All-Int -> Int64; Int/Double mix -> Double (widening); all-Text with
/// every value JSON-subtyped -> `Utf8 { json: true }`; all-Text -> Utf8;
/// all-Blob -> Bytes; anything else (mixed types, or all-NULL) -> Utf8,
/// same fallback CSV uses for mixed columns.
fn sniff<'a>(values: impl Iterator<Item = &'a Cell>) -> ColumnType {
    let (mut saw_int, mut saw_double, mut saw_text, mut saw_blob) = (false, false, false, false);
    let mut all_text_json = true;

    for cell in values {
        match &cell.value {
            OwnedValue::Null => {}
            OwnedValue::Integer(_) => saw_int = true,
            OwnedValue::Double(_) => saw_double = true,
            OwnedValue::Text(_) => {
                saw_text = true;
                all_text_json &= cell.json;
            }
            OwnedValue::Blob(_) => saw_blob = true,
        }
    }

    match (saw_int, saw_double, saw_text, saw_blob) {
        (false, false, false, false) => ColumnType::Utf8 { json: false }, // all-NULL or empty
        (true, false, false, false) => ColumnType::Int64,
        (_, true, false, false) => ColumnType::Double, // double, or int+double mix
        (false, false, true, false) => ColumnType::Utf8 { json: all_text_json },
        (false, false, false, true) => ColumnType::Bytes,
        _ => ColumnType::Utf8 { json: false }, // mixed types: strings, as CSV does
    }
}

/// Resolve one column's type + a human-readable reason for it, used in
/// [`ExportError::ParquetTypeMismatch`] messages.
fn resolve_column_type(
    decl: Option<&str>,
    col_idx: usize,
    first_group: &[Vec<Cell>],
) -> (ColumnType, String) {
    let buffered_rows = first_group.len();
    let sniff_column = || sniff(first_group.iter().map(|row| &row[col_idx]));

    match decl {
        Some(d) => match affinity_from_decltype(d) {
            Affinity::Integer => (ColumnType::Int64, format!("from its declared type '{d}'")),
            Affinity::Real => (ColumnType::Double, format!("from its declared type '{d}'")),
            Affinity::Text => (
                ColumnType::Utf8 { json: false },
                format!("from its declared type '{d}'"),
            ),
            Affinity::Blob => (ColumnType::Bytes, format!("from its declared type '{d}'")),
            Affinity::Boolean => (ColumnType::Boolean, format!("from its declared type '{d}'")),
            Affinity::Numeric => (
                sniff_column(),
                format!(
                    "declared type '{d}' has no fixed storage class; inferred from the values \
                     in the first {buffered_rows} rows"
                ),
            ),
        },
        None => (
            sniff_column(),
            format!("from the values in the first {buffered_rows} rows"),
        ),
    }
}

/// De-duplicate column names (`SELECT 1, 1` both name their column `1`):
/// Parquet field names should be unique, so repeats get `_2`, `_3`, ...
/// suffixes. (If a suffixed name collides with another original name, the
/// schema still builds; readers may see two similarly-named fields, but
/// each is a distinct, positionally-correct column.)
fn dedupe_names(names: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashMap::<String, usize>::new();
    names
        .into_iter()
        .map(|name| {
            let count = seen.entry(name.clone()).or_insert(0);
            *count += 1;
            if *count == 1 {
                name
            } else {
                format!("{name}_{count}")
            }
        })
        .collect()
}

/// Build the Parquet message schema: one `OPTIONAL` primitive field per
/// column.
fn build_schema(names: &[String], types: &[ColumnType]) -> Result<Arc<Type>, ExportError> {
    let mut fields = Vec::with_capacity(names.len());
    for (name, ty) in names.iter().zip(types) {
        let physical = match ty {
            ColumnType::Int64 => PhysicalType::INT64,
            ColumnType::Double => PhysicalType::DOUBLE,
            ColumnType::Utf8 { .. } | ColumnType::Bytes => PhysicalType::BYTE_ARRAY,
            ColumnType::Boolean => PhysicalType::BOOLEAN,
        };
        let logical = match ty {
            ColumnType::Utf8 { json: true } => Some(LogicalType::Json),
            ColumnType::Utf8 { json: false } => Some(LogicalType::String),
            _ => None,
        };
        let field = Type::primitive_type_builder(name, physical)
            .with_repetition(Repetition::OPTIONAL)
            .with_logical_type(logical)
            .build()?;
        fields.push(Arc::new(field));
    }
    let schema = Type::group_type_builder("schema").with_fields(fields).build()?;
    Ok(Arc::new(schema))
}

/// A human-readable description of an [`OwnedValue`]'s kind, for
/// [`ExportError::ParquetTypeMismatch::found`].
fn found_name(value: &OwnedValue) -> &'static str {
    match value {
        OwnedValue::Null => "null",
        OwnedValue::Integer(_) => "integer",
        OwnedValue::Double(_) => "real",
        OwnedValue::Text(_) => "text",
        OwnedValue::Blob(_) => "blob",
    }
}

/// Per-column typed buffer, accumulating one row group's worth of values
/// and definition levels (`0` = null, `1` = present; these columns are
/// never repeated, so no repetition levels are needed).
enum ColumnBuffer {
    Int64 { values: Vec<i64>, def_levels: Vec<i16> },
    Double { values: Vec<f64>, def_levels: Vec<i16> },
    Bytes { values: Vec<ByteArray>, def_levels: Vec<i16> },
    Boolean { values: Vec<bool>, def_levels: Vec<i16> },
}

impl ColumnBuffer {
    fn new(ty: ColumnType) -> Self {
        match ty {
            ColumnType::Int64 => ColumnBuffer::Int64 {
                values: Vec::new(),
                def_levels: Vec::new(),
            },
            ColumnType::Double => ColumnBuffer::Double {
                values: Vec::new(),
                def_levels: Vec::new(),
            },
            ColumnType::Utf8 { .. } | ColumnType::Bytes => ColumnBuffer::Bytes {
                values: Vec::new(),
                def_levels: Vec::new(),
            },
            ColumnType::Boolean => ColumnBuffer::Boolean {
                values: Vec::new(),
                def_levels: Vec::new(),
            },
        }
    }

    fn push_null(&mut self) {
        match self {
            ColumnBuffer::Int64 { def_levels, .. }
            | ColumnBuffer::Double { def_levels, .. }
            | ColumnBuffer::Bytes { def_levels, .. }
            | ColumnBuffer::Boolean { def_levels, .. } => def_levels.push(0),
        }
    }

    /// Push one value, applying the acceptance/widening rules from
    /// `PLAN-parquet.md`'s "Schema inference" section. `row` is the
    /// 1-based row number within the whole result set, for error messages.
    fn push(
        &mut self,
        cell: &Cell,
        ty: ColumnType,
        column: &str,
        row: usize,
        reason: &str,
    ) -> Result<(), ExportError> {
        if matches!(cell.value, OwnedValue::Null) {
            self.push_null();
            return Ok(());
        }

        match (ty, &mut *self, &cell.value) {
            (ColumnType::Int64, ColumnBuffer::Int64 { values, def_levels }, OwnedValue::Integer(v)) => {
                values.push(*v);
                def_levels.push(1);
            }
            (ColumnType::Double, ColumnBuffer::Double { values, def_levels }, OwnedValue::Double(v)) => {
                values.push(*v);
                def_levels.push(1);
            }
            // INTEGER values in a DOUBLE column widen to f64 rather than erroring.
            (ColumnType::Double, ColumnBuffer::Double { values, def_levels }, OwnedValue::Integer(v)) => {
                values.push(*v as f64);
                def_levels.push(1);
            }
            (ColumnType::Utf8 { .. }, ColumnBuffer::Bytes { values, def_levels }, OwnedValue::Text(bytes)) => {
                if std::str::from_utf8(bytes).is_err() {
                    return Err(ExportError::InvalidUtf8);
                }
                values.push(ByteArray::from(bytes.clone()));
                def_levels.push(1);
            }
            (ColumnType::Bytes, ColumnBuffer::Bytes { values, def_levels }, OwnedValue::Blob(bytes)) => {
                values.push(ByteArray::from(bytes.clone()));
                def_levels.push(1);
            }
            // BOOLEAN accepts integer 0/1 only.
            (ColumnType::Boolean, ColumnBuffer::Boolean { values, def_levels }, OwnedValue::Integer(0)) => {
                values.push(false);
                def_levels.push(1);
            }
            (ColumnType::Boolean, ColumnBuffer::Boolean { values, def_levels }, OwnedValue::Integer(1)) => {
                values.push(true);
                def_levels.push(1);
            }
            (_, _, value) => {
                return Err(ExportError::ParquetTypeMismatch {
                    column: column.to_string(),
                    row,
                    expected: ty.expected_name(),
                    found: found_name(value),
                    reason: reason.to_string(),
                });
            }
        }
        Ok(())
    }

    /// Write the buffered batch to the next column chunk and clear it.
    fn flush<W: Write + Send>(&mut self, rg: &mut SerializedRowGroupWriter<'_, W>) -> Result<(), ExportError> {
        let mut col_writer = rg
            .next_column()?
            .expect("one column writer per schema field: buffers and schema fields are built together");
        match self {
            ColumnBuffer::Int64 { values, def_levels } => {
                col_writer
                    .typed::<Int64Type>()
                    .write_batch(values.as_slice(), Some(def_levels.as_slice()), None)?;
                values.clear();
                def_levels.clear();
            }
            ColumnBuffer::Double { values, def_levels } => {
                col_writer
                    .typed::<DoubleType>()
                    .write_batch(values.as_slice(), Some(def_levels.as_slice()), None)?;
                values.clear();
                def_levels.clear();
            }
            ColumnBuffer::Bytes { values, def_levels } => {
                col_writer
                    .typed::<ByteArrayType>()
                    .write_batch(values.as_slice(), Some(def_levels.as_slice()), None)?;
                values.clear();
                def_levels.clear();
            }
            ColumnBuffer::Boolean { values, def_levels } => {
                col_writer
                    .typed::<BoolType>()
                    .write_batch(values.as_slice(), Some(def_levels.as_slice()), None)?;
                values.clear();
                def_levels.clear();
            }
        }
        col_writer.close()?;
        Ok(())
    }
}

/// Push one buffered group of rows into fresh [`ColumnBuffer`]s and flush
/// them as a single Parquet row group. `row_number` is the running
/// 1-based row counter across the whole export (for error messages).
fn write_row_group<W: Write + Send>(
    writer: &mut SerializedFileWriter<W>,
    rows: &[Vec<Cell>],
    names: &[String],
    col_types: &[(ColumnType, String)],
    row_number: &mut usize,
) -> Result<(), ExportError> {
    let mut buffers: Vec<ColumnBuffer> =
        col_types.iter().map(|(ty, _)| ColumnBuffer::new(*ty)).collect();

    for row in rows {
        *row_number += 1;
        for (col_idx, cell) in row.iter().enumerate() {
            let (ty, reason) = &col_types[col_idx];
            buffers[col_idx].push(cell, *ty, &names[col_idx], *row_number, reason)?;
        }
    }

    let mut rg_writer = writer.next_row_group()?;
    for buf in &mut buffers {
        buf.flush(&mut rg_writer)?;
    }
    rg_writer.close()?;
    Ok(())
}

/// Step `stmt` for up to `limit` rows, copying each into an owned [`Cell`]
/// row (running the existing blob-limit check on the raw values first).
/// Returns `(rows, exhausted)`, where `exhausted` is `true` if the
/// statement ran out of rows before `limit` was reached.
fn buffer_rows(
    stmt: &mut Statement,
    names: &[String],
    blob_limit: Option<u64>,
    limit: usize,
) -> Result<(Vec<Vec<Cell>>, bool), ExportError> {
    let mut rows = Vec::new();
    loop {
        if rows.len() >= limit {
            return Ok((rows, false));
        }
        match stmt.next() {
            Ok(Some(row)) => {
                check_blob_limit(&row, names, blob_limit)?;
                rows.push(row.iter().map(Cell::from_value_ref).collect());
            }
            Ok(None) => return Ok((rows, true)),
            Err(e) => return Err(ExportError::Sql(e.to_string())),
        }
    }
}

/// [`write_parquet`] with a caller-chosen row group size, for tests that
/// need multiple row groups without buffering 65,536 rows.
pub(super) fn write_parquet_with_group_size<W: Write + Send>(
    stmt: &mut Statement,
    output: W,
    blob_limit: Option<u64>,
    group_rows: usize,
) -> Result<(), ExportError> {
    let names = dedupe_names(stmt.column_names().map_err(|e| ExportError::Sql(e.to_string()))?);
    let metas = stmt.column_meta();

    // The first row group is buffered up front: schema inference needs to
    // see the values before the writer (which needs the schema before its
    // first row) can be created.
    let (first_group, mut exhausted) = buffer_rows(stmt, &names, blob_limit, group_rows)?;

    let col_types: Vec<(ColumnType, String)> = (0..names.len())
        .map(|i| {
            let decl = metas.get(i).and_then(|m| m.decltype.as_deref());
            resolve_column_type(decl, i, &first_group)
        })
        .collect();
    let types: Vec<ColumnType> = col_types.iter().map(|(ty, _)| *ty).collect();

    let schema = build_schema(&names, &types)?;
    let props = Arc::new(
        WriterProperties::builder()
            .set_compression(Compression::ZSTD(ZstdLevel::try_new(3)?))
            .set_created_by(format!("solite {}", env!("CARGO_PKG_VERSION")))
            .build(),
    );

    let mut writer = SerializedFileWriter::new(output, schema, props)?;
    let mut row_number = 0usize;

    if !first_group.is_empty() {
        write_row_group(&mut writer, &first_group, &names, &col_types, &mut row_number)?;
    }

    while !exhausted {
        let (group, done) = buffer_rows(stmt, &names, blob_limit, group_rows)?;
        exhausted = done;
        if !group.is_empty() {
            write_row_group(&mut writer, &group, &names, &col_types, &mut row_number)?;
        }
    }

    writer.close()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exporter::write_output_to_bytes;
    use crate::sqlite::Connection;
    use ::parquet::basic::Repetition as ParquetRepetition;
    use ::parquet::file::reader::{FileReader, SerializedFileReader};
    use ::parquet::record::{Field, RowAccessor};
    use bytes::Bytes;

    fn stmt_for(conn: &Connection, sql: &str) -> Statement {
        let (_, stmt) = conn.prepare(sql).unwrap();
        stmt.unwrap()
    }

    fn write_bytes(sql: &str) -> Vec<u8> {
        write_bytes_setup(sql, |_| {})
    }

    /// Run `setup` (typically `CREATE TABLE` + `INSERT`) then export `sql`
    /// as Parquet to an in-memory buffer.
    fn write_bytes_setup(sql: &str, setup: impl FnOnce(&Connection)) -> Vec<u8> {
        let conn = Connection::open_in_memory().unwrap();
        setup(&conn);
        let mut stmt = stmt_for(&conn, sql);
        let mut buf = Vec::new();
        write_parquet(&mut stmt, &mut buf, None).unwrap();
        buf
    }

    fn reader_for(bytes: Vec<u8>) -> SerializedFileReader<Bytes> {
        SerializedFileReader::new(Bytes::from(bytes)).unwrap()
    }

    #[test]
    fn test_declared_types_round_trip() {
        let bytes = write_bytes_setup(
            "select * from t",
            |conn| {
                conn.execute_script(
                    "create table t(a integer, b real, c text, d blob, e boolean);
                     insert into t values (1, 1.5, 'hi', x'0102', 1);
                     insert into t values (null, null, null, null, null);",
                )
                .unwrap();
            },
        );
        assert!(bytes.starts_with(b"PAR1"));
        assert!(bytes.ends_with(b"PAR1"));

        let reader = reader_for(bytes);
        let schema = reader.metadata().file_metadata().schema_descr();
        assert_eq!(schema.column(0).physical_type(), PhysicalType::INT64);
        assert_eq!(schema.column(1).physical_type(), PhysicalType::DOUBLE);
        assert_eq!(schema.column(2).physical_type(), PhysicalType::BYTE_ARRAY);
        assert_eq!(schema.column(3).physical_type(), PhysicalType::BYTE_ARRAY);
        assert_eq!(schema.column(4).physical_type(), PhysicalType::BOOLEAN);
        for i in 0..5 {
            assert_eq!(
                schema.column(i).self_type().get_basic_info().repetition(),
                ParquetRepetition::OPTIONAL
            );
        }
        assert_eq!(
            schema.column(2).logical_type_ref(),
            Some(&LogicalType::String)
        );

        let mut rows = reader.get_row_iter(None).unwrap();
        let row1 = rows.next().unwrap().unwrap();
        assert_eq!(row1.get_long(0).unwrap(), 1);
        assert_eq!(row1.get_double(1).unwrap(), 1.5);
        assert_eq!(row1.get_string(2).unwrap(), "hi");
        assert_eq!(row1.get_bytes(3).unwrap().data(), &[0x01, 0x02]);
        assert!(row1.get_bool(4).unwrap());

        let row2 = rows.next().unwrap().unwrap();
        for i in 0..5 {
            assert!(matches!(row2.get_column_iter().nth(i).unwrap().1, Field::Null));
        }
        assert!(rows.next().is_none());
    }

    #[test]
    fn test_sniffed_expression_columns() {
        let bytes = write_bytes("select count(*) as n, 1.5*2 as f, 'x' as s, x'00' as b");
        let reader = reader_for(bytes);
        let schema = reader.metadata().file_metadata().schema_descr();
        assert_eq!(schema.column(0).physical_type(), PhysicalType::INT64);
        assert_eq!(schema.column(1).physical_type(), PhysicalType::DOUBLE);
        assert_eq!(schema.column(2).physical_type(), PhysicalType::BYTE_ARRAY);
        assert_eq!(schema.column(3).physical_type(), PhysicalType::BYTE_ARRAY);

        let mut rows = reader.get_row_iter(None).unwrap();
        let row = rows.next().unwrap().unwrap();
        assert_eq!(row.get_long(0).unwrap(), 1);
        assert_eq!(row.get_double(1).unwrap(), 3.0);
        assert_eq!(row.get_string(2).unwrap(), "x");
        assert_eq!(row.get_bytes(3).unwrap().data(), &[0x00]);
    }

    #[test]
    fn test_int_real_mix_widens_to_double() {
        let bytes = write_bytes("select 1 as x union all select 1.5");
        let reader = reader_for(bytes);
        let schema = reader.metadata().file_metadata().schema_descr();
        assert_eq!(schema.column(0).physical_type(), PhysicalType::DOUBLE);

        let mut rows = reader.get_row_iter(None).unwrap();
        assert_eq!(rows.next().unwrap().unwrap().get_double(0).unwrap(), 1.0);
        assert_eq!(rows.next().unwrap().unwrap().get_double(0).unwrap(), 1.5);
    }

    #[test]
    fn test_declared_real_holding_integer_is_1_point_0() {
        let bytes = write_bytes_setup(
            "select b from t",
            |conn| {
                conn.execute_script(
                    "create table t(b real); insert into t values (1);",
                )
                .unwrap();
            },
        );
        let reader = reader_for(bytes);
        let schema = reader.metadata().file_metadata().schema_descr();
        assert_eq!(schema.column(0).physical_type(), PhysicalType::DOUBLE);
        let mut rows = reader.get_row_iter(None).unwrap();
        assert_eq!(rows.next().unwrap().unwrap().get_double(0).unwrap(), 1.0);
    }

    #[test]
    fn test_all_null_sniffed_column_is_utf8() {
        let bytes = write_bytes("select null as x from (select 1) union all select null");
        let reader = reader_for(bytes);
        let schema = reader.metadata().file_metadata().schema_descr();
        assert_eq!(schema.column(0).physical_type(), PhysicalType::BYTE_ARRAY);
        assert_eq!(
            schema.column(0).logical_type_ref(),
            Some(&LogicalType::String)
        );
    }

    #[test]
    fn test_json_subtype_gets_json_logical_type() {
        let bytes = write_bytes("select json_object('a', 1) as j");
        let reader = reader_for(bytes);
        let schema = reader.metadata().file_metadata().schema_descr();
        assert_eq!(
            schema.column(0).logical_type_ref(),
            Some(&LogicalType::Json)
        );
        let mut rows = reader.get_row_iter(None).unwrap();
        assert_eq!(rows.next().unwrap().unwrap().get_string(0).unwrap(), "{\"a\":1}");
    }

    #[test]
    fn test_mixed_json_and_plain_text_falls_back_to_string() {
        let bytes = write_bytes("select json('{}') as j union all select 'plain'");
        let reader = reader_for(bytes);
        let schema = reader.metadata().file_metadata().schema_descr();
        assert_eq!(
            schema.column(0).logical_type_ref(),
            Some(&LogicalType::String)
        );
    }

    #[test]
    fn test_mismatch_declared_integer_with_text() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_script(
            "create table t(a integer);
             insert into t values (1);
             insert into t values (2);
             insert into t values ('abc');",
        )
        .unwrap();
        let mut stmt = stmt_for(&conn, "select a from t");
        let mut buf = Vec::new();
        let err = write_parquet(&mut stmt, &mut buf, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("'a'"), "{msg}");
        assert!(msg.contains("row 3"), "{msg}");
        assert!(msg.contains("INT64"), "{msg}");
        assert!(msg.contains("declared type 'INTEGER'"), "{msg}");
    }

    #[test]
    fn test_mismatch_boolean_with_2() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_script(
            "create table t(e boolean); insert into t values (2);",
        )
        .unwrap();
        let mut stmt = stmt_for(&conn, "select e from t");
        let mut buf = Vec::new();
        let err = write_parquet(&mut stmt, &mut buf, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("BOOLEAN"), "{msg}");
        assert!(msg.contains("row 1"), "{msg}");
    }

    #[test]
    fn test_mismatch_sniffed_int_then_text_says_inferred() {
        // Type is sniffed as Int64 from the first row group (group size 1,
        // so only the leading `1` is sampled); the text value two rows
        // later, in a subsequent streamed group, is the mismatch.
        let conn = Connection::open_in_memory().unwrap();
        let mut stmt = stmt_for(
            &conn,
            "select 1 as x union all select 2 union all select 'abc'",
        );
        let mut buf = Vec::new();
        let err = write_parquet_with_group_size(&mut stmt, &mut buf, None, 1).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("'x'"), "{msg}");
        assert!(msg.contains("row 3"), "{msg}");
        assert!(msg.contains("INT64"), "{msg}");
        assert!(msg.contains("values in the first 1 rows"), "{msg}");
    }

    #[test]
    fn test_blob_over_limit_errors() {
        let conn = Connection::open_in_memory().unwrap();
        let mut stmt = stmt_for(&conn, "select zeroblob(32) as payload");
        let mut buf = Vec::new();
        let err = write_parquet(&mut stmt, &mut buf, Some(16)).unwrap_err();
        assert!(matches!(err, ExportError::BlobTooLarge { .. }), "{err:?}");
    }

    #[test]
    fn test_zero_rows_valid_file_with_schema() {
        let conn = Connection::open_in_memory().unwrap();
        let mut stmt = stmt_for(&conn, "select 1 as a, 'x' as b where 0");
        let mut buf = Vec::new();
        write_parquet(&mut stmt, &mut buf, None).unwrap();
        assert!(buf.starts_with(b"PAR1"));
        assert!(buf.ends_with(b"PAR1"));

        let reader = reader_for(buf);
        assert_eq!(reader.metadata().num_row_groups(), 0);
        assert_eq!(reader.metadata().file_metadata().schema_descr().num_columns(), 2);
    }

    #[test]
    fn test_multi_row_group() {
        let conn = Connection::open_in_memory().unwrap();
        let mut stmt = stmt_for(
            &conn,
            "with recursive c(x) as (select 1 union all select x + 1 from c where x < 10) \
             select x from c",
        );
        let mut buf = Vec::new();
        write_parquet_with_group_size(&mut stmt, &mut buf, None, 4).unwrap();

        let reader = reader_for(buf);
        assert_eq!(reader.metadata().num_row_groups(), 3);

        let mut rows = reader.get_row_iter(None).unwrap();
        for expected in 1..=10 {
            assert_eq!(rows.next().unwrap().unwrap().get_long(0).unwrap(), expected);
        }
        assert!(rows.next().is_none());
    }

    #[test]
    fn test_write_output_to_bytes_produces_par1() {
        let conn = Connection::open_in_memory().unwrap();
        let mut stmt = stmt_for(&conn, "select 1 as a, 'x' as b");
        let bytes = write_output_to_bytes(
            &mut stmt,
            crate::exporter::ExportFormat::Parquet,
            crate::exporter::BlobLimit::Default,
        )
        .unwrap();
        assert!(bytes.starts_with(b"PAR1"));
        assert!(bytes.ends_with(b"PAR1"));
    }

    #[test]
    fn test_dedupe_names() {
        assert_eq!(
            dedupe_names(vec!["1".to_string(), "1".to_string(), "1".to_string()]),
            vec!["1".to_string(), "1_2".to_string(), "1_3".to_string()]
        );
        assert_eq!(
            dedupe_names(vec!["a".to_string(), "b".to_string()]),
            vec!["a".to_string(), "b".to_string()]
        );
    }
}
