//! GeoJSON export: `FeatureCollection`, newline-delimited Features, and
//! RFC 8142 text sequences.
//!
//! One row becomes one GeoJSON `Feature`: the column named `geometry` (or
//! [`GeoJsonOptions::geometry_column`]) is the Feature's geometry, every
//! other column becomes a member of `properties` through
//! [`super::value_to_json`], so the property rules are exactly the ones
//! `.json`/`.ndjson` already use (JSON-subtyped text nests, blobs are
//! base64, NULL is `null`).
//!
//! v1 is **GeoJSON text in, GeoJSON text out**: a geometry cell must
//! already be a GeoJSON geometry object, either carrying SQLite's JSON
//! subtype or starting with `{` (the same sniffing sqlite-tg does). Such a
//! cell is syntax-checked and then copied to the output verbatim — never
//! re-serialized through a `serde_json::Value`, so a 175 MB polygon file
//! doesn't get parsed into a tree per row. WKB blobs and WKT text are
//! errors pointing at `tg_to_geojson()`; decoding them is a later
//! workstream (sqlite-tg is not in the stdlib yet).

use std::borrow::Cow;
use std::io::Write;

use super::{check_blob_limit, value_to_json, ExportError, GeoJsonOptions};
use crate::sqlite::{Statement, ValueRefX, ValueRefXValue};

/// Column name used as the geometry when [`GeoJsonOptions::geometry_column`]
/// is `None`. Matched case-insensitively.
const DEFAULT_GEOMETRY_COLUMN: &str = "geometry";

/// SQLite's JSON subtype tag ('J'), mirroring the check in
/// [`super::value_to_json`].
const JSON_SUBTYPE: u32 = 74;

/// The seven geometry types of RFC 7946 §1.4. `Feature` and
/// `FeatureCollection` are deliberately absent: they are not geometries.
const GEOMETRY_TYPES: [&str; 7] = [
    "Point",
    "LineString",
    "Polygon",
    "MultiPoint",
    "MultiLineString",
    "MultiPolygon",
    "GeometryCollection",
];

/// ASCII record separator, the RFC 8142 Feature prefix.
const RECORD_SEPARATOR: u8 = 0x1e;

/// How Features are arranged in the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GeoJsonLayout {
    /// A single `FeatureCollection` envelope wrapping an array of Features
    /// (`.geojson`).
    Collection,
    /// One Feature per line, no envelope (`.geojsonl`, `.ndgeojson`).
    Lines,
    /// RFC 8142 GeoJSON text sequence (`.geojsons`): every Feature is
    /// preceded by an ASCII record separator (0x1E) and followed by a
    /// newline.
    Seq,
}

/// Cheap probe for a geometry object's `"type"` member.
///
/// Deserializing this parses the whole document once (that *is* the syntax
/// check, and it is O(bytes)) without building a `serde_json::Value` tree.
/// Unknown members are ignored, which is what we want: the probe only
/// answers "is this a geometry object, and of what type".
#[derive(serde::Deserialize)]
struct TypeProbe<'a> {
    #[serde(borrow, rename = "type")]
    ty: Option<Cow<'a, str>>,
}

/// Write statement results as GeoJSON in the given `layout`.
///
/// `blob_limit` bounds the raw size of any BLOB cell, same as the other
/// formats (see [`super::check_blob_limit`]).
///
/// # Errors
///
/// - [`ExportError::GeometryColumnMissing`] if the result set has no
///   geometry column. This is checked from `column_names()` before the
///   first row is stepped, so a zero-row result still errors.
/// - [`ExportError::InvalidGeometry`] if a geometry cell isn't GeoJSON
///   text.
pub(super) fn write_geojson<W: Write>(
    stmt: &mut Statement,
    mut out: W,
    blob_limit: Option<u64>,
    opts: &GeoJsonOptions,
    layout: GeoJsonLayout,
) -> Result<(), ExportError> {
    let columns = stmt
        .column_names()
        .map_err(|e| ExportError::Sql(format!("{:?}", e)))?;

    let wanted = opts
        .geometry_column
        .as_deref()
        .unwrap_or(DEFAULT_GEOMETRY_COLUMN);
    let geometry_idx = columns
        .iter()
        .position(|c| c.eq_ignore_ascii_case(wanted))
        .ok_or_else(|| ExportError::GeometryColumnMissing {
            wanted: wanted.to_owned(),
            columns: columns.clone(),
        })?;

    if layout == GeoJsonLayout::Collection {
        out.write_all(b"{\"type\":\"FeatureCollection\",\"features\":[")?;
    }

    let mut row_number = 0usize;
    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                check_blob_limit(&row, &columns, blob_limit)?;
                row_number += 1;
                match layout {
                    GeoJsonLayout::Collection if row_number > 1 => out.write_all(b",")?,
                    GeoJsonLayout::Seq => out.write_all(&[RECORD_SEPARATOR])?,
                    _ => {}
                }
                write_feature(&mut out, &columns, &row, geometry_idx, row_number)?;
                if matches!(layout, GeoJsonLayout::Lines | GeoJsonLayout::Seq) {
                    out.write_all(b"\n")?;
                }
            }
            Ok(None) => break,
            Err(e) => return Err(ExportError::Sql(e.to_string())),
        }
    }

    if layout == GeoJsonLayout::Collection {
        out.write_all(b"]}\n")?;
    }
    Ok(())
}

/// Write one row as a GeoJSON `Feature`.
///
/// `row_number` is 1-based over the result set and only used for error
/// messages.
fn write_feature<W: Write>(
    out: &mut W,
    columns: &[String],
    row: &[ValueRefX],
    geometry_idx: usize,
    row_number: usize,
) -> Result<(), ExportError> {
    out.write_all(b"{\"type\":\"Feature\",\"geometry\":")?;
    let geometry = row
        .get(geometry_idx)
        .ok_or(ExportError::ColumnIndexOutOfBounds {
            index: geometry_idx,
            count: row.len(),
        })?;
    write_geometry(out, geometry, &columns[geometry_idx], row_number)?;

    out.write_all(b",\"properties\":")?;
    // Duplicate column names collapse, last one wins, exactly as
    // `super::write_json_row` does for `.json`/`.ndjson`.
    let mut props = serde_json::Map::new();
    for (idx, value) in row.iter().enumerate() {
        if idx == geometry_idx {
            continue;
        }
        let key = columns
            .get(idx)
            .ok_or(ExportError::ColumnIndexOutOfBounds {
                index: idx,
                count: columns.len(),
            })?
            .to_owned();
        props.insert(key, value_to_json(value)?);
    }
    // An empty object, never `null`: a Feature with no properties is still
    // a Feature with properties.
    serde_json::to_writer(&mut *out, &serde_json::Value::Object(props))?;

    out.write_all(b"}")?;
    Ok(())
}

/// Write a single geometry cell.
///
/// NULL is a valid (unlocated) Feature geometry. GeoJSON text is validated
/// and then copied byte for byte. Everything else is an error naming the
/// column and the 1-based `row`.
fn write_geometry<W: Write>(
    out: &mut W,
    value: &ValueRefX,
    column: &str,
    row: usize,
) -> Result<(), ExportError> {
    let invalid = |reason: String| ExportError::InvalidGeometry {
        column: column.to_owned(),
        row,
        reason,
    };
    match &value.value {
        ValueRefXValue::Null => out.write_all(b"null").map_err(ExportError::Io),
        ValueRefXValue::Int(_) => Err(invalid("integer is not a geometry".to_owned())),
        ValueRefXValue::Double(_) => Err(invalid("real is not a geometry".to_owned())),
        ValueRefXValue::Blob(_) => Err(invalid(
            "blob is not a GeoJSON geometry (WKB is not supported yet; \
             wrap it in tg_to_geojson())"
                .to_owned(),
        )),
        ValueRefXValue::Text(bytes) => {
            let text = std::str::from_utf8(bytes).map_err(|_| ExportError::InvalidUtf8)?;
            // Same sniffing rule as sqlite-tg's `geomValue`: the JSON
            // subtype, or a leading `{`, means GeoJSON. Anything else in a
            // TEXT cell is WKT, which v1 doesn't decode.
            let is_geojson =
                value.subtype() == Some(JSON_SUBTYPE) || first_non_space(bytes) == Some(b'{');
            if !is_geojson {
                return Err(invalid(
                    "text is not a GeoJSON geometry object (WKT is not supported; \
                     wrap it in tg_to_geojson())"
                        .to_owned(),
                ));
            }
            validate_geometry_text(bytes).map_err(invalid)?;
            out.write_all(text.as_bytes())?;
            Ok(())
        }
    }
}

/// First byte that isn't ASCII whitespace, if any.
fn first_non_space(bytes: &[u8]) -> Option<u8> {
    bytes.iter().copied().find(|b| !b.is_ascii_whitespace())
}

/// Check that `bytes` parses as a JSON object whose `"type"` is one of the
/// seven GeoJSON geometry types, returning the reason it isn't.
fn validate_geometry_text(bytes: &[u8]) -> Result<(), String> {
    if first_non_space(bytes) != Some(b'{') {
        return Err("expected a JSON object".to_owned());
    }
    let probe: TypeProbe =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid JSON: {e}"))?;
    let Some(ty) = probe.ty else {
        return Err("JSON object has no \"type\" member".to_owned());
    };
    if GEOMETRY_TYPES.contains(&ty.as_ref()) {
        return Ok(());
    }
    if ty == "Feature" || ty == "FeatureCollection" {
        return Err(format!(
            "GeoJSON type '{ty}' is not a geometry; select its geometry member \
             (e.g. geometry ->> '$.geometry') instead"
        ));
    }
    Err(format!("unknown GeoJSON type '{ty}'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::Connection;
    use serde_json::Value;

    /// Export `sql` as GeoJSON into an in-memory buffer.
    fn write_bytes(
        sql: &str,
        opts: &GeoJsonOptions,
        layout: GeoJsonLayout,
    ) -> Result<Vec<u8>, ExportError> {
        write_bytes_limit(sql, opts, layout, None)
    }

    fn write_bytes_limit(
        sql: &str,
        opts: &GeoJsonOptions,
        layout: GeoJsonLayout,
        blob_limit: Option<u64>,
    ) -> Result<Vec<u8>, ExportError> {
        let conn = Connection::open_in_memory().unwrap();
        let (_, stmt) = conn.prepare(sql).unwrap();
        let mut stmt = stmt.unwrap();
        let mut buf = Vec::new();
        write_geojson(&mut stmt, &mut buf, blob_limit, opts, layout)?;
        Ok(buf)
    }

    fn collection(sql: &str) -> Value {
        let bytes = write_bytes(sql, &GeoJsonOptions::default(), GeoJsonLayout::Collection)
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    const TWO_POINTS: &str = r#"select 1 as id, 'a' as name,
            json('{"type":"Point","coordinates":[1,2]}') as geometry
        union all
        select 2, 'b', '{"type":"Point","coordinates":[3,4]}'"#;

    #[test]
    fn test_collection_basic() {
        let doc = collection(TWO_POINTS);
        assert_eq!(doc["type"], "FeatureCollection");
        let features = doc["features"].as_array().unwrap();
        assert_eq!(features.len(), 2);

        assert_eq!(features[0]["type"], "Feature");
        // `id` stays in properties; lifting it is the later --id flag
        assert_eq!(
            features[0]["properties"],
            serde_json::json!({"id": 1, "name": "a"})
        );
        assert_eq!(
            features[0]["geometry"],
            serde_json::json!({"type": "Point", "coordinates": [1, 2]})
        );
        // second row has no JSON subtype, only a leading `{`
        assert_eq!(
            features[1]["geometry"],
            serde_json::json!({"type": "Point", "coordinates": [3, 4]})
        );
        assert_eq!(
            features[1]["properties"],
            serde_json::json!({"id": 2, "name": "b"})
        );
    }

    #[test]
    fn test_geometry_text_is_written_verbatim() {
        let raw = r#"{"type":"Point", "coordinates": [1, 2]}"#;
        let bytes = write_bytes(
            &format!("select '{raw}' as geometry"),
            &GeoJsonOptions::default(),
            GeoJsonLayout::Collection,
        )
        .unwrap();
        let out = String::from_utf8(bytes).unwrap();
        assert!(out.contains(raw), "{out}");
    }

    #[test]
    fn test_lines_layout() {
        let bytes = write_bytes(TWO_POINTS, &GeoJsonOptions::default(), GeoJsonLayout::Lines)
            .unwrap();
        let out = String::from_utf8(bytes).unwrap();
        assert!(out.ends_with('\n'), "{out}");
        assert!(!out.contains("FeatureCollection"), "{out}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            let feature: Value = serde_json::from_str(line).unwrap();
            assert_eq!(feature["type"], "Feature");
        }
    }

    #[test]
    fn test_seq_layout() {
        let bytes =
            write_bytes(TWO_POINTS, &GeoJsonOptions::default(), GeoJsonLayout::Seq).unwrap();
        let out = String::from_utf8(bytes).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            assert!(line.starts_with('\u{1e}'), "{line:?}");
            let feature: Value = serde_json::from_str(&line[1..]).unwrap();
            assert_eq!(feature["type"], "Feature");
        }
    }

    #[test]
    fn test_zero_rows() {
        let sql = "select 1 as id, null as geometry where 0";
        let bytes = write_bytes(sql, &GeoJsonOptions::default(), GeoJsonLayout::Collection)
            .unwrap();
        assert_eq!(
            bytes.as_slice(),
            b"{\"type\":\"FeatureCollection\",\"features\":[]}\n".as_slice()
        );

        for layout in [GeoJsonLayout::Lines, GeoJsonLayout::Seq] {
            let bytes = write_bytes(sql, &GeoJsonOptions::default(), layout).unwrap();
            assert!(bytes.is_empty(), "{layout:?}: {bytes:?}");
        }
    }

    #[test]
    fn test_no_property_columns_is_empty_object() {
        let bytes = write_bytes(
            r#"select json('{"type":"Point","coordinates":[1,2]}') as geometry"#,
            &GeoJsonOptions::default(),
            GeoJsonLayout::Lines,
        )
        .unwrap();
        let out = String::from_utf8(bytes).unwrap();
        assert!(out.contains(r#""properties":{}"#), "{out}");
    }

    #[test]
    fn test_null_geometry() {
        let bytes = write_bytes(
            "select 1 as id, null as geometry",
            &GeoJsonOptions::default(),
            GeoJsonLayout::Lines,
        )
        .unwrap();
        let out = String::from_utf8(bytes).unwrap();
        assert!(out.contains(r#""geometry":null"#), "{out}");
    }

    #[test]
    fn test_geometry_column_is_case_insensitive() {
        let doc = collection(r#"select json('{"type":"Point","coordinates":[1,2]}') as GEOMETRY"#);
        assert_eq!(doc["features"][0]["geometry"]["type"], "Point");
    }

    #[test]
    fn test_geometry_column_override() {
        let opts = GeoJsonOptions {
            geometry_column: Some("geom".into()),
        };
        let bytes = write_bytes(
            r#"select 1 as geometry, json('{"type":"Point","coordinates":[1,2]}') as geom"#,
            &opts,
            GeoJsonLayout::Lines,
        )
        .unwrap();
        let feature: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(feature["geometry"]["type"], "Point");
        // the column named `geometry` is now just another property
        assert_eq!(feature["properties"], serde_json::json!({"geometry": 1}));
    }

    #[test]
    fn test_missing_geometry_column() {
        // fires even though the result has rows...
        let err = write_bytes(
            "select 1 as id, 'x' as name, 'y' as geom",
            &GeoJsonOptions::default(),
            GeoJsonLayout::Collection,
        )
        .unwrap_err();
        match &err {
            ExportError::GeometryColumnMissing { wanted, columns } => {
                assert_eq!(wanted, "geometry");
                assert_eq!(columns, &["id".to_string(), "name".into(), "geom".into()]);
            }
            other => panic!("expected GeometryColumnMissing, got {other:?}"),
        }
        let msg = err.to_string();
        assert!(msg.contains("no geometry column 'geometry'"), "{msg}");
        assert!(msg.contains("id, name, geom"), "{msg}");

        // ...and with zero rows, since it's checked before stepping
        let err = write_bytes(
            "select 1 as id where 0",
            &GeoJsonOptions::default(),
            GeoJsonLayout::Lines,
        )
        .unwrap_err();
        assert!(
            matches!(err, ExportError::GeometryColumnMissing { .. }),
            "{err:?}"
        );

        // the override name is the one reported
        let opts = GeoJsonOptions {
            geometry_column: Some("shape".into()),
        };
        let err = write_bytes("select 1 as geometry", &opts, GeoJsonLayout::Lines).unwrap_err();
        assert!(err.to_string().contains("'shape'"), "{err}");
    }

    /// `sql` must have a `geometry` column; returns the `InvalidGeometry`
    /// (column, row, reason).
    fn invalid_geometry(sql: &str) -> (String, usize, String) {
        let err = write_bytes(sql, &GeoJsonOptions::default(), GeoJsonLayout::Lines)
            .unwrap_err();
        match err {
            ExportError::InvalidGeometry {
                column,
                row,
                reason,
            } => (column, row, reason),
            other => panic!("expected InvalidGeometry, got {other:?}"),
        }
    }

    #[test]
    fn test_invalid_geometry_wkt() {
        let (column, row, reason) = invalid_geometry("select 'POINT(1 2)' as geometry");
        assert_eq!(column, "geometry");
        assert_eq!(row, 1);
        assert!(reason.contains("WKT is not supported"), "{reason}");
        assert!(reason.contains("tg_to_geojson()"), "{reason}");
    }

    #[test]
    fn test_invalid_geometry_wkb_blob() {
        let (_, _, reason) = invalid_geometry("select x'0101' as geometry");
        assert!(reason.contains("WKB is not supported yet"), "{reason}");
        assert!(reason.contains("tg_to_geojson()"), "{reason}");
    }

    #[test]
    fn test_invalid_geometry_feature_object() {
        let (_, _, reason) = invalid_geometry(
            r#"select '{"type":"Feature","geometry":null,"properties":{}}' as geometry"#,
        );
        assert!(reason.contains("'Feature' is not a geometry"), "{reason}");
        assert!(reason.contains("geometry member"), "{reason}");
    }

    #[test]
    fn test_invalid_geometry_not_an_object() {
        // JSON subtype, but an array: the leading-`{` sniff never fires, so
        // this is the subtype path
        let (_, _, reason) = invalid_geometry("select json('[1,2]') as geometry");
        assert_eq!(reason, "expected a JSON object");
    }

    #[test]
    fn test_invalid_geometry_missing_type() {
        let (_, _, reason) = invalid_geometry(r#"select '{"coordinates":[1,2]}' as geometry"#);
        assert!(reason.contains("no \"type\" member"), "{reason}");
    }

    #[test]
    fn test_invalid_geometry_unknown_type() {
        let (_, _, reason) = invalid_geometry(r#"select '{"type":"Pointy"}' as geometry"#);
        assert_eq!(reason, "unknown GeoJSON type 'Pointy'");
    }

    #[test]
    fn test_invalid_geometry_malformed_json() {
        let (_, _, reason) = invalid_geometry(r#"select '{not json' as geometry"#);
        assert!(reason.starts_with("invalid JSON: "), "{reason}");
    }

    #[test]
    fn test_invalid_geometry_integer() {
        let (_, _, reason) = invalid_geometry("select 42 as geometry");
        assert_eq!(reason, "integer is not a geometry");
    }

    #[test]
    fn test_invalid_geometry_names_the_1_based_row() {
        let (column, row, reason) = invalid_geometry(
            r#"select json('{"type":"Point","coordinates":[1,2]}') as geometry
               union all select 'POINT(1 2)'"#,
        );
        assert_eq!(column, "geometry");
        assert_eq!(row, 2);
        assert!(reason.contains("WKT"), "{reason}");
        // and the Display string names both
        let err = ExportError::InvalidGeometry {
            column,
            row,
            reason,
        };
        let msg = err.to_string();
        assert!(msg.starts_with("column 'geometry' (row 2): "), "{msg}");
    }

    #[test]
    fn test_property_rules_are_inherited_from_json() {
        let doc = collection(
            r#"select json_object('a', 1) as meta, x'DEADBEEF' as payload,
                      null as note,
                      json('{"type":"Point","coordinates":[1,2]}') as geometry"#,
        );
        let props = &doc["features"][0]["properties"];
        // JSON-subtyped text nests as an object, not a string
        assert_eq!(props["meta"], serde_json::json!({"a": 1}));
        // blobs are base64
        assert_eq!(props["payload"], "3q2+7w==");
        assert_eq!(props["note"], Value::Null);
    }

    #[test]
    fn test_property_column_order_is_preserved() {
        let bytes = write_bytes(
            r#"select 1 as zeta, 2 as alpha,
                      json('{"type":"Point","coordinates":[1,2]}') as geometry,
                      3 as mid"#,
            &GeoJsonOptions::default(),
            GeoJsonLayout::Lines,
        )
        .unwrap();
        let out = String::from_utf8(bytes).unwrap();
        assert!(
            out.contains(r#""properties":{"zeta":1,"alpha":2,"mid":3}"#),
            "{out}"
        );
    }

    #[test]
    fn test_non_finite_property_is_invalid_float() {
        let err = write_bytes(
            r#"select 1e999 as huge, json('{"type":"Point","coordinates":[1,2]}') as geometry"#,
            &GeoJsonOptions::default(),
            GeoJsonLayout::Lines,
        )
        .unwrap_err();
        assert!(matches!(err, ExportError::InvalidFloat(_)), "{err:?}");
    }

    /// The S3 path buffers the whole export through `write_output_to_bytes`
    /// instead of a file writer; all three layouts must be reachable there.
    #[cfg(feature = "object_store")]
    #[test]
    fn test_write_output_to_bytes_covers_every_layout() {
        use crate::exporter::{write_output_to_bytes, BlobLimit, ExportFormat};

        let opts = GeoJsonOptions::default();
        for (format, expected_features) in [
            (ExportFormat::GeoJson(opts.clone()), 2),
            (ExportFormat::GeoJsonl(opts.clone()), 2),
            (ExportFormat::GeoJsonSeq(opts), 2),
        ] {
            let conn = Connection::open_in_memory().unwrap();
            let (_, stmt) = conn.prepare(TWO_POINTS).unwrap();
            let mut stmt = stmt.unwrap();
            let bytes =
                write_output_to_bytes(&mut stmt, format.clone(), BlobLimit::Default).unwrap();
            let text = String::from_utf8(bytes).unwrap();
            assert_eq!(
                text.matches("\"type\":\"Feature\"").count(),
                expected_features,
                "{format:?}: {text}"
            );
        }
    }

    #[test]
    fn test_blob_limit_trips_before_geometry_validation() {
        // the geometry cell is WKT (invalid), but the oversized blob
        // property is checked first
        let err = write_bytes_limit(
            "select zeroblob(32) as payload, 'POINT(1 2)' as geometry",
            &GeoJsonOptions::default(),
            GeoJsonLayout::Lines,
            Some(4),
        )
        .unwrap_err();
        match err {
            ExportError::BlobTooLarge { column, .. } => assert_eq!(column, "payload"),
            other => panic!("expected BlobTooLarge, got {other:?}"),
        }
    }
}
