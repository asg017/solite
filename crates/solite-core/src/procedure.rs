//! Procedure types and parsing utilities.
//!
//! Procedures are named SQL blocks annotated with `-- name: xxx :annotation`
//! that can be registered in the runtime and invoked via `.call`.

use crate::sqlite::ColumnMeta;
use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

/// Regex for parsing `-- name: xxx :annotation` lines.
static NAME_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^--\s+name:\s+(\w+)((?:\s+:\w+)*)").expect("valid regex"));

/// The expected result type of a procedure.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub enum ResultType {
    /// Procedure returns no results (INSERT, UPDATE, DELETE, etc.)
    Void,
    /// Procedure returns multiple rows
    Rows,
    /// Procedure returns exactly one row
    Row,
    /// Procedure returns a single value
    Value,
    /// Procedure returns a list of single values
    List,
}

/// A SQL parameter with optional type annotation.
///
/// Parameters can be annotated with types using the `::type` syntax:
/// - `$name::text` - parameter named "name" with type "text"
/// - `$id::int` - parameter named "id" with type "int"
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct ProcedureParam {
    /// The full parameter name as it appears in SQL (e.g., "$name::text")
    pub full_name: String,
    /// The parameter name without prefix or type (e.g., "name")
    pub name: String,
    /// The annotated type, if any (e.g., "text")
    pub annotated_type: Option<String>,
}

/// A named SQL procedure with metadata.
#[derive(Serialize, Debug, Clone)]
pub struct Procedure {
    /// The name of the procedure (from `-- name: xxx`)
    pub name: String,
    /// The SQL query text
    pub sql: String,
    /// The result type annotation
    pub result_type: ResultType,
    /// Parameters used in the query
    pub parameters: Vec<ProcedureParam>,
    /// Column metadata for the result set
    pub columns: Vec<ColumnMeta>,
}

/// Parse a `-- name: xxx :annotation` line.
///
/// Returns the name and list of annotations (without the leading colon).
pub fn parse_name_line(line: &str) -> Option<(String, Vec<String>)> {
    let caps = NAME_LINE_RE.captures(line)?;
    let name = caps.get(1)?.as_str().to_string();

    let annotations_str = caps.get(2).map_or("", |m| m.as_str());
    let annotations: Vec<String> = annotations_str
        .split_whitespace()
        .filter_map(|s| s.strip_prefix(':').map(|a| a.to_string()))
        .collect();

    Some((name, annotations))
}

/// Parse a parameter string into a ProcedureParam struct.
///
/// Handles both simple parameters (`$name`) and typed parameters (`$name::text`).
pub fn parse_parameter(param: &str) -> ProcedureParam {
    // Check if it starts with $ or : and contains ::
    if (param.starts_with('$') || param.starts_with(':')) && param.contains("::") {
        if let Some(idx) = param.find("::") {
            let prefix_and_name = &param[..idx];
            let type_annotation = &param[idx + 2..];
            return ProcedureParam {
                full_name: param.to_string(),
                name: prefix_and_name[1..].to_string(),
                annotated_type: Some(type_annotation.to_string()),
            };
        }
    }

    // Simple parameter without type annotation
    ProcedureParam {
        full_name: param.to_string(),
        name: if param.is_empty() {
            String::new()
        } else {
            param[1..].to_string()
        },
        annotated_type: None,
    }
}

/// Determine the result type from annotations and column count.
pub fn determine_result_type(annotations: &[String], column_count: usize) -> ResultType {
    if annotations.iter().any(|f| f == "rows") {
        ResultType::Rows
    } else if annotations.iter().any(|f| f == "row") {
        ResultType::Row
    } else if annotations.iter().any(|f| f == "value") {
        ResultType::Value
    } else if annotations.iter().any(|f| f == "list") {
        ResultType::List
    } else if column_count == 0 {
        ResultType::Void
    } else {
        ResultType::Rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_name_line_simple() {
        let result = parse_name_line("-- name: getUsers");
        assert!(result.is_some());
        let (name, annotations) = result.unwrap();
        assert_eq!(name, "getUsers");
        assert!(annotations.is_empty());
    }

    #[test]
    fn test_parse_name_line_with_annotation() {
        let result = parse_name_line("-- name: getUsers :rows");
        assert!(result.is_some());
        let (name, annotations) = result.unwrap();
        assert_eq!(name, "getUsers");
        assert_eq!(annotations, vec!["rows"]);
    }

    #[test]
    fn test_parse_name_line_multiple_annotations() {
        let result = parse_name_line("-- name: insertUser :row :returning");
        assert!(result.is_some());
        let (name, annotations) = result.unwrap();
        assert_eq!(name, "insertUser");
        assert_eq!(annotations, vec!["row", "returning"]);
    }

    #[test]
    fn test_parse_name_line_invalid() {
        assert!(parse_name_line("-- not a name line").is_none());
        assert!(parse_name_line("select * from users").is_none());
        assert!(parse_name_line("").is_none());
    }

    #[test]
    fn test_parse_parameter_simple() {
        let param = parse_parameter("$name");
        assert_eq!(param.full_name, "$name");
        assert_eq!(param.name, "name");
        assert!(param.annotated_type.is_none());
    }

    #[test]
    fn test_parse_parameter_with_type() {
        let param = parse_parameter("$name::text");
        assert_eq!(param.full_name, "$name::text");
        assert_eq!(param.name, "name");
        assert_eq!(param.annotated_type, Some("text".to_string()));
    }

    #[test]
    fn test_parse_parameter_colon_prefix() {
        let param = parse_parameter(":id::int");
        assert_eq!(param.full_name, ":id::int");
        assert_eq!(param.name, "id");
        assert_eq!(param.annotated_type, Some("int".to_string()));
    }

    #[test]
    fn test_determine_result_type() {
        assert_eq!(
            determine_result_type(&["rows".to_string()], 3),
            ResultType::Rows
        );
        assert_eq!(
            determine_result_type(&["row".to_string()], 3),
            ResultType::Row
        );
        assert_eq!(
            determine_result_type(&["value".to_string()], 1),
            ResultType::Value
        );
        assert_eq!(
            determine_result_type(&["list".to_string()], 1),
            ResultType::List
        );
        assert_eq!(determine_result_type(&[], 0), ResultType::Void);
        assert_eq!(determine_result_type(&[], 3), ResultType::Rows);
    }
}
