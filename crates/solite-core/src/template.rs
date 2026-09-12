//! Render query results and registered procedures through the [`knap`]
//! template engine.
//!
//! This module is the single bridge between solite's SQLite runtime and
//! knap's `Value` model. It is deliberately narrow:
//!
//! - [`cell_to_value`] maps one SQLite cell to a [`knap::Value`], mirroring
//!   [`crate::exporter`]'s JSON mapping so a rendered template and a JSON
//!   export agree on what a value looks like.
//! - [`statement_to_value`] runs a prepared statement to completion and
//!   shapes the result according to a procedure's [`ResultType`].
//! - [`render`] wires the above into a knap render: `rows` (when given) and
//!   every `.param`-table entry become top-level template variables, and
//!   registered procedures are exposed through a resolver that runs each
//!   procedure's SQL at most once per render (cached by root name) and
//!   reports a SQL failure as a render error naming the procedure.
//!
//! Solite always renders with `strict_variables = true`: an undefined name
//! in value position is a render error, not empty output. No custom knap
//! filters are added here; callers (the `.render` dot command and the
//! `render` CLI subcommand) consume this module but are not defined here.

use crate::procedure::ResultType;
use crate::sqlite::{SQLiteError, Statement, ValueRefX, ValueRefXValue};
use crate::Runtime;
use knap::{Engine, ErrorCode, Map, RenderInput, RenderOptions, RuntimeError as KnapRuntimeError, TemplateWarning, Value};
use std::collections::HashMap;
use std::fmt;

/// The largest magnitude integer that still round-trips through an f64
/// without loss (2^53), matching the JSON exporter's boundary
/// (`exporter::value_to_json`).
const MAX_SAFE_INTEGER: u64 = 1u64 << 53;

/// Convert one SQLite cell to a knap value (mirrors `exporter::value_to_json`).
///
/// - `Null` -> `Value::Null`.
/// - `Int` -> `Value::Number` when its magnitude fits losslessly in an f64
///   (`|v| <= 2^53`), else `Value::String` of its decimal digits.
/// - `Double` -> `Value::Number`, or `Value::Null` for NaN/+-infinity.
/// - `Text` tagged with the JSON subtype (74) -> parsed as JSON and
///   converted to a knap value (falls back to `Value::String` on a parse
///   failure); other text -> `Value::String` (lossy UTF-8).
/// - `Blob` -> a standard-alphabet base64 `Value::String`.
pub fn cell_to_value(v: &ValueRefX) -> Value {
    match &v.value {
        ValueRefXValue::Null => Value::Null,
        ValueRefXValue::Int(n) => {
            if n.unsigned_abs() <= MAX_SAFE_INTEGER {
                Value::Number(*n as f64)
            } else {
                Value::String(n.to_string())
            }
        }
        ValueRefXValue::Double(d) => {
            if d.is_finite() {
                Value::Number(*d)
            } else {
                Value::Null
            }
        }
        ValueRefXValue::Text(bytes) => {
            if v.subtype() == Some(74) {
                match serde_json::from_slice::<serde_json::Value>(bytes) {
                    Ok(json) => json_to_knap_value(json),
                    Err(_) => Value::String(String::from_utf8_lossy(bytes).into_owned()),
                }
            } else {
                Value::String(String::from_utf8_lossy(bytes).into_owned())
            }
        }
        ValueRefXValue::Blob(bytes) => {
            use base64::Engine as _;
            Value::String(base64::engine::general_purpose::STANDARD.encode(bytes))
        }
    }
}

/// Recursively convert a `serde_json::Value` (built with `preserve_order`)
/// into a knap value, keeping object key order.
fn json_to_knap_value(json: serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Value::String(s),
        serde_json::Value::Array(items) => Value::Array(items.into_iter().map(json_to_knap_value).collect()),
        serde_json::Value::Object(obj) => {
            let mut map = Map::with_capacity(obj.len());
            for (key, value) in obj {
                map.insert(key, json_to_knap_value(value));
            }
            Value::Object(map)
        }
    }
}

/// Run a prepared statement to completion and shape it by `ResultType`
/// (settled result-shaping table):
///
/// - `Rows` -> array of objects (keys are `Statement::column_names`, in
///   select order; a duplicate column name has its value overwritten by the
///   later column but keeps its first position, matching
///   `exporter::write_json_row`'s `serde_json::Map` behavior under
///   `preserve_order`).
/// - `Row` -> the first row's object, or `Value::Null` if there is none.
/// - `Value` -> the first cell of the first row, or `Value::Null` if there
///   is no row.
/// - `List` -> array of first cells.
/// - `Void` -> run to completion, `Value::Undefined`.
pub fn statement_to_value(stmt: &mut Statement, result_type: &ResultType) -> Result<Value, SQLiteError> {
    match result_type {
        ResultType::Void => {
            stmt.execute()?;
            Ok(Value::Undefined)
        }
        ResultType::Value => match stmt.nextx()? {
            Some(row) => Ok(cell_to_value(&row.value_at(0))),
            None => Ok(Value::Null),
        },
        ResultType::List => {
            let mut items = Vec::new();
            while let Some(row) = stmt.nextx()? {
                items.push(cell_to_value(&row.value_at(0)));
            }
            Ok(Value::Array(items))
        }
        ResultType::Row => {
            let columns = column_names(stmt)?;
            match stmt.nextx()? {
                Some(row) => Ok(Value::Object(row_to_object(&row, &columns))),
                None => Ok(Value::Null),
            }
        }
        ResultType::Rows => {
            let columns = column_names(stmt)?;
            let mut items = Vec::new();
            while let Some(row) = stmt.nextx()? {
                items.push(Value::Object(row_to_object(&row, &columns)));
            }
            Ok(Value::Array(items))
        }
    }
}

fn column_names(stmt: &Statement) -> Result<Vec<String>, SQLiteError> {
    stmt.column_names()
        .map_err(|e| SQLiteError::custom("UTF8", format!("column name is not valid UTF-8: {e}")))
}

fn row_to_object(row: &crate::sqlite::Row<'_>, columns: &[String]) -> Map {
    let mut map = Map::with_capacity(columns.len());
    for (idx, name) in columns.iter().enumerate() {
        // `Map::insert` replaces an existing key's value in place, keeping
        // its original position - the same "last value wins, first position
        // sticks" behavior `serde_json::Map` gives under `preserve_order`.
        map.insert(name.clone(), cell_to_value(&row.value_at(idx)));
    }
    map
}

/// Everything a render needs besides the template text.
pub struct RenderRequest<'a> {
    pub runtime: &'a mut Runtime,
    /// Bound as top-level `rows` when present (dot-command body).
    pub rows: Option<Value>,
}

/// A failed render: every error knap reported, ready to display.
#[derive(Debug)]
pub struct TemplateRenderError {
    name: String,
    errors: Vec<knap::TemplateError>,
}

impl TemplateRenderError {
    /// The individual knap errors that made up this failure.
    pub fn errors(&self) -> &[knap::TemplateError] {
        &self.errors
    }
}

impl fmt::Display for TemplateRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, error) in self.errors.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{}:{}:{}: {}: {}", self.name, error.line, error.column, error.code.as_str(), error.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for TemplateRenderError {}

/// Walk the segments of a dotted name (e.g. `["x"]` for `total.x`) into an
/// already-resolved root value. A numeric segment indexes an array;
/// otherwise it is an object key. Any miss (wrong container, out-of-range
/// index, unknown key) returns `Value::Undefined`.
fn resolve_path(root: Value, path: &[&str]) -> Value {
    let mut current = root;
    for segment in path {
        current = match &current {
            Value::Array(items) => segment.parse::<usize>().ok().and_then(|i| items.get(i)).cloned().unwrap_or(Value::Undefined),
            Value::Object(map) => map.get(segment).cloned().unwrap_or(Value::Undefined),
            _ => Value::Undefined,
        };
        if current.is_undefined() {
            return Value::Undefined;
        }
    }
    current
}

/// Render `template` (already read; `name` is used in error messages and as
/// the `<file>` part of [`TemplateRenderError`]'s Display).
///
/// Builds the top-level variable map (`rows`, then every parameter), then
/// renders with a resolver that looks up registered procedures by the root
/// segment of a (possibly dotted) name, running each procedure's SQL at
/// most once per render and caching the shaped result by that root name.
/// Always renders with `strict_variables = true`.
pub fn render(
    name: &str,
    template: &str,
    req: RenderRequest<'_>,
) -> Result<(String, Vec<TemplateWarning>), TemplateRenderError> {
    let RenderRequest { runtime, rows } = req;

    // Build the variable map first: it borrows nothing from `runtime`, so
    // `runtime` can be moved into the resolver closure below.
    let mut variables = Map::new();
    if let Some(rows) = rows {
        variables.insert("rows", rows);
    }
    if let Some(mut stmt) = crate::dot::param::list_parameters_statement(runtime) {
        while let Ok(Some(row)) = stmt.nextx() {
            let key = row.value_at(0).as_str().to_string();
            let value = cell_to_value(&row.value_at(1));
            variables.insert(key, value);
        }
    }

    // Procedures are run lazily, at most once per render, and cached by the
    // root name of whatever dotted path first referenced them.
    let mut cache: HashMap<String, Value> = HashMap::new();
    let mut resolver = move |full_name: &str| -> Result<Value, KnapRuntimeError> {
        let mut segments = full_name.split('.');
        let root = segments.next().unwrap_or(full_name);
        let path: Vec<&str> = segments.collect();

        if let Some(cached) = cache.get(root) {
            return Ok(resolve_path(cached.clone(), &path));
        }

        let Some(proc) = runtime.get_procedure(root) else {
            return Ok(Value::Undefined);
        };
        let sql = proc.sql.clone();
        let result_type = proc.result_type.clone();

        let stmt = runtime
            .prepare_with_parameters(&sql)
            .map_err(|e| KnapRuntimeError::new(format!("procedure {root}: {e}"), ErrorCode::ResolveError))?
            .1
            .ok_or_else(|| {
                KnapRuntimeError::new(format!("procedure {root}: query produced no statement"), ErrorCode::ResolveError)
            })?;
        let mut stmt = stmt;

        let value = statement_to_value(&mut stmt, &result_type)
            .map_err(|e| KnapRuntimeError::new(format!("procedure {root}: {e}"), ErrorCode::ResolveError))?;

        cache.insert(root.to_string(), value.clone());
        Ok(resolve_path(value, &path))
    };

    let engine = Engine::standard();
    let options = RenderOptions { strict_variables: true, trim_output: false, limits: Default::default() };
    let input = RenderInput::new(&variables).resolver(&mut resolver);
    let result = engine.render(template, input, &options);

    if !result.errors.is_empty() {
        return Err(TemplateRenderError { name: name.to_string(), errors: result.errors });
    }
    Ok((result.output, result.warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procedure::{Procedure, ResultType};
    use crate::sqlite::{Statement, ValueRefX, ValueRefXValue};
    use crate::{BlockSource, Runtime};

    fn runtime() -> Runtime {
        Runtime::new(None).unwrap()
    }

    fn prepare(rt: &Runtime, sql: &str) -> Statement {
        rt.prepare_with_parameters(sql).unwrap().1.unwrap()
    }

    // ---- cell mapping ----

    #[test]
    fn cell_mapping_null() {
        let v = ValueRefX::from_owned(ValueRefXValue::Null, None);
        assert_eq!(cell_to_value(&v), Value::Null);
    }

    #[test]
    fn cell_mapping_int_at_safe_boundary_is_number() {
        let v = ValueRefX::from_owned(ValueRefXValue::Int(9_007_199_254_740_992), None); // 2^53
        assert_eq!(cell_to_value(&v), Value::Number(9_007_199_254_740_992.0));
    }

    #[test]
    fn cell_mapping_int_beyond_safe_boundary_is_string() {
        let v = ValueRefX::from_owned(ValueRefXValue::Int(9_007_199_254_740_993), None);
        assert_eq!(cell_to_value(&v), Value::String("9007199254740993".to_string()));
    }

    #[test]
    fn cell_mapping_negative_safe_boundary_is_number() {
        let v = ValueRefX::from_owned(ValueRefXValue::Int(-9_007_199_254_740_992), None);
        assert_eq!(cell_to_value(&v), Value::Number(-9_007_199_254_740_992.0));
    }

    #[test]
    fn cell_mapping_double_nan_and_inf_are_null() {
        assert_eq!(cell_to_value(&ValueRefX::from_owned(ValueRefXValue::Double(f64::NAN), None)), Value::Null);
        assert_eq!(cell_to_value(&ValueRefX::from_owned(ValueRefXValue::Double(f64::INFINITY), None)), Value::Null);
        assert_eq!(cell_to_value(&ValueRefX::from_owned(ValueRefXValue::Double(f64::NEG_INFINITY), None)), Value::Null);
    }

    #[test]
    fn cell_mapping_double_finite_is_number() {
        assert_eq!(cell_to_value(&ValueRefX::from_owned(ValueRefXValue::Double(1.5), None)), Value::Number(1.5));
    }

    #[test]
    fn cell_mapping_text_is_string() {
        let v = ValueRefX::from_owned(ValueRefXValue::Text(b"hello"), None);
        assert_eq!(cell_to_value(&v), Value::String("hello".to_string()));
    }

    #[test]
    fn cell_mapping_json_subtype_is_object() {
        let bytes: &[u8] = br#"{"a":1}"#;
        let v = ValueRefX::from_owned(ValueRefXValue::Text(bytes), Some(74));
        match cell_to_value(&v) {
            Value::Object(map) => assert_eq!(map.get("a"), Some(&Value::Number(1.0))),
            other => panic!("expected object, got {other:?}"),
        }
    }

    #[test]
    fn cell_mapping_invalid_json_subtype_falls_back_to_string() {
        let bytes: &[u8] = b"not json";
        let v = ValueRefX::from_owned(ValueRefXValue::Text(bytes), Some(74));
        assert_eq!(cell_to_value(&v), Value::String("not json".to_string()));
    }

    #[test]
    fn cell_mapping_blob_is_base64() {
        let bytes: &[u8] = &[0u8, 1, 2];
        let v = ValueRefX::from_owned(ValueRefXValue::Blob(bytes), None);
        assert_eq!(cell_to_value(&v), Value::String("AAEC".to_string()));
    }

    // ---- shaping ----

    #[test]
    fn shaping_rows_is_array_of_objects() {
        let rt = runtime();
        rt.connection
            .execute_script("create table t(a int, b text); insert into t values (1,'x'),(2,'y'),(3,'z');")
            .unwrap();
        let mut stmt = prepare(&rt, "select a, b from t order by a");
        let value = statement_to_value(&mut stmt, &ResultType::Rows).unwrap();
        let Value::Array(rows) = value else { panic!("expected array") };
        assert_eq!(rows.len(), 3);
        let Value::Object(first) = &rows[0] else { panic!("expected object") };
        assert_eq!(first.get("a"), Some(&Value::Number(1.0)));
        assert_eq!(first.get("b"), Some(&Value::String("x".to_string())));
    }

    #[test]
    fn shaping_row_is_first_object() {
        let rt = runtime();
        rt.connection.execute_script("create table t(a int); insert into t values (1),(2),(3);").unwrap();
        let mut stmt = prepare(&rt, "select a from t order by a");
        let value = statement_to_value(&mut stmt, &ResultType::Row).unwrap();
        match value {
            Value::Object(map) => assert_eq!(map.get("a"), Some(&Value::Number(1.0))),
            other => panic!("expected object, got {other:?}"),
        }
    }

    #[test]
    fn shaping_value_is_first_cell() {
        let rt = runtime();
        rt.connection.execute_script("create table t(a int); insert into t values (5),(6);").unwrap();
        let mut stmt = prepare(&rt, "select a from t order by a");
        let value = statement_to_value(&mut stmt, &ResultType::Value).unwrap();
        assert_eq!(value, Value::Number(5.0));
    }

    #[test]
    fn shaping_list_is_array_of_first_cells() {
        let rt = runtime();
        rt.connection.execute_script("create table t(a int); insert into t values (1),(2),(3);").unwrap();
        let mut stmt = prepare(&rt, "select a from t order by a");
        let value = statement_to_value(&mut stmt, &ResultType::List).unwrap();
        assert_eq!(value, Value::Array(vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)]));
    }

    #[test]
    fn shaping_void_runs_to_completion_and_is_undefined() {
        let rt = runtime();
        rt.connection.execute_script("create table t(a int);").unwrap();
        let mut stmt = prepare(&rt, "insert into t values (1)");
        let value = statement_to_value(&mut stmt, &ResultType::Void).unwrap();
        assert!(value.is_undefined());
        let mut check = prepare(&rt, "select count(*) from t");
        let row = check.nextx().unwrap().unwrap();
        assert_eq!(row.value_at(0).as_int64(), 1);
    }

    #[test]
    fn shaping_empty_result() {
        let rt = runtime();
        rt.connection.execute_script("create table t(a int);").unwrap();

        let mut stmt = prepare(&rt, "select a from t");
        assert_eq!(statement_to_value(&mut stmt, &ResultType::Row).unwrap(), Value::Null);

        let mut stmt = prepare(&rt, "select a from t");
        assert_eq!(statement_to_value(&mut stmt, &ResultType::Value).unwrap(), Value::Null);

        let mut stmt = prepare(&rt, "select a from t");
        assert_eq!(statement_to_value(&mut stmt, &ResultType::List).unwrap(), Value::Array(vec![]));

        let mut stmt = prepare(&rt, "select a from t");
        assert_eq!(statement_to_value(&mut stmt, &ResultType::Rows).unwrap(), Value::Array(vec![]));
    }

    // ---- resolver ----

    #[test]
    fn resolver_runs_registered_procedure_at_most_once_per_render() {
        let mut rt = runtime();
        rt.connection.execute_script("create table counter(id integer primary key);").unwrap();
        rt.enqueue(
            "[test]",
            "-- name: n :rows\ninsert into counter default values returning id;",
            BlockSource::Repl,
        );
        rt.execute_to_completion().unwrap();
        assert!(rt.get_procedure("n").is_some());

        let (output, warnings) =
            render("[test]", "{{ n | length }}{{ n | length }}", RenderRequest { runtime: &mut rt, rows: None })
                .unwrap();
        assert_eq!(output, "11");
        assert!(warnings.is_empty());

        let mut count_stmt = prepare(&rt, "select count(*) from counter");
        let row = count_stmt.nextx().unwrap().unwrap();
        assert_eq!(row.value_at(0).as_int64(), 1, "procedure SQL should have run exactly once");
    }

    // ---- errors ----

    #[test]
    fn undefined_name_is_a_strict_error_with_position() {
        let mut rt = runtime();
        let err = render("[test]", "hello {{ missing }}", RenderRequest { runtime: &mut rt, rows: None })
            .unwrap_err();
        let display = err.to_string();

        let mut parts = display.splitn(4, ':');
        let file = parts.next().unwrap();
        let line: usize = parts.next().unwrap().parse().expect("line number");
        let column: usize = parts.next().unwrap().parse().expect("column number");
        let rest = parts.next().unwrap();

        assert_eq!(file, "[test]");
        assert!(line >= 1);
        assert!(column >= 1);
        assert!(rest.trim_start().starts_with("UNDEFINED_VARIABLE:"), "{display}");
        assert!(display.contains("missing"), "{display}");
    }

    #[test]
    fn procedure_with_bad_sql_error_mentions_procedure_name() {
        let mut rt = runtime();
        rt.register_procedure(Procedure {
            name: "broken".to_string(),
            sql: "select * from no_such_table".to_string(),
            result_type: ResultType::Rows,
            annotations: vec![],
            parameters: vec![],
            columns: vec![],
            result_class: None,
        });

        let err = render("[test]", "{{ broken }}", RenderRequest { runtime: &mut rt, rows: None }).unwrap_err();
        assert!(err.to_string().contains("broken"), "{err}");
    }

    #[test]
    fn parse_error_aborts_before_any_procedure_runs() {
        let mut rt = runtime();
        rt.connection.execute_script("create table counter(id integer primary key);").unwrap();
        rt.enqueue(
            "[test]",
            "-- name: n :void\ninsert into counter default values;",
            BlockSource::Repl,
        );
        rt.execute_to_completion().unwrap();

        // Unclosed tag: a template parse error that short-circuits before
        // any node (and so the `n` reference) is ever evaluated.
        let err = render("[test]", "{{ n", RenderRequest { runtime: &mut rt, rows: None }).unwrap_err();
        assert!(err.to_string().contains("PARSE_ERROR"), "{err}");

        let mut count_stmt = prepare(&rt, "select count(*) from counter");
        let row = count_stmt.nextx().unwrap().unwrap();
        assert_eq!(row.value_at(0).as_int64(), 0, "procedure should not have run");
    }

    // ---- params ----

    #[test]
    fn param_set_renders_as_top_level_variable() {
        let mut rt = runtime();
        rt.define_parameter("who".to_string(), "world".to_string()).unwrap();
        let (output, _) =
            render("[test]", "{{ who }}", RenderRequest { runtime: &mut rt, rows: None }).unwrap();
        assert_eq!(output, "world");
    }
}
