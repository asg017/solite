use crate::{cli::CodegenArgs, errors::report_error};
use solite_core::{sqlite::ColumnMeta, Runtime, StepError};
use std::path::PathBuf;

#[derive(serde::Serialize, Debug)]
struct Parameter {
    full_name: String,
    name: String,
    annotated_type: Option<String>,
}

#[derive(serde::Serialize, Debug)]
enum ResultType {
    Void,
    Rows,
    Row,
    Value,
    List,
}

#[derive(serde::Serialize, Debug)]
struct Export {
    name: String,
    parameters: Vec<Parameter>,
    columns: Vec<ColumnMeta>,
    sql: String,
    result_type: ResultType,
}

#[derive(serde::Serialize, Debug)]
struct Report {
    setup: Vec<String>,
    exports: Vec<Export>,
}

use regex::Regex;
use std::sync::LazyLock;

static LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^--\s+name:\s+(\w+)((?:\s+:\w+)*)").unwrap());

fn parse_line(line: &str) -> Option<(String, Vec<String>)> {
    if let Some(caps) = LINE_RE.captures(line) {
        let name = caps.get(1)?.as_str().to_string();

        let annotations_str = caps.get(2).map_or("", |m| m.as_str());
        let annotations: Vec<String> = annotations_str
            .split_whitespace()
            .filter_map(|s| s.strip_prefix(':').map(|a| a.to_string()))
            .collect();

        Some((name, annotations))
    } else {
        None
    }
}

fn report_from_file(source: &str, filename: &PathBuf) -> anyhow::Result<Report> {
    let mut report = Report {
        setup: vec![],
        exports: vec![],
    };
    let mut rt = Runtime::new(None);
    rt.enqueue(
        &filename.to_string_lossy().to_string(),
        source,
        solite_core::BlockSource::File(filename.to_owned()),
    );
    loop {
        match rt.next_stepx() {
            None => break,
            Some(Err(error)) => {
                match error {
                  StepError::ParseDot(err) => todo!(),
                  StepError::Prepare { file_name, src, offset, error } => {
                    report_error(&file_name, &src, &error, Some(offset));//Some(offset));
                    todo!()
                  }
                }
            }
            Some(Ok(ref step)) => {
                match &step.result {
                    solite_core::StepResult::SqlStatement { stmt, raw_sql } => {
                        if let Some(preamble) = &step.preamble {
                            //println!("preamble: {}", preamble.trim_start());
                            //if let Some(rest) = preamble.trim_start().strip_prefix("-- name:") {
                            if preamble.trim().starts_with("-- name:") {
                                let (name, annotations) = parse_line(preamble.trim()).unwrap();
                                let columns: Vec<ColumnMeta> = stmt.column_meta();

                                let parameters = stmt.parameter_info();
                                let export_parameters = parameters
                                    .iter()
                                    .map(|p| {
                                        if p.starts_with("$") && p.contains("::") {
                                            let idx = p
                                                .find("::")
                                                .expect("pattern to match contains above");
                                            let left = &p[..idx];
                                            let right = &p[idx + "::".len()..];
                                            Parameter {
                                                full_name: p.to_string(),
                                                name: left[1..].to_string(),
                                                annotated_type: Some(right.to_string()),
                                            }
                                        } else {
                                            Parameter {
                                                full_name: p.to_string(),
                                                name: p[1..].to_string(),
                                                annotated_type: None,
                                            }
                                        }
                                    })
                                    .collect::<Vec<_>>();
                                let result_type = if annotations.iter().any(|f| f == "rows") {
                                    ResultType::Rows
                                } else if annotations.iter().any(|f| f == "row") {
                                    ResultType::Row
                                } else if annotations.iter().any(|f| f == "value") {
                                    ResultType::Value
                                } else if annotations.iter().any(|f| f == "list") {
                                    ResultType::List
                                } else {
                                    if columns.len() == 0 {
                                        ResultType::Void
                                    } else {
                                        ResultType::Rows
                                    }
                                };
                                report.exports.push(Export {
                                    name: name.to_string(),
                                    parameters: export_parameters,
                                    columns: columns.clone(),
                                    sql: stmt.sql().to_string(),
                                    result_type,
                                });
                                continue;
                            }
                        }
                        report.setup.push(stmt.sql().to_string());
                        stmt.execute().unwrap();
                    }
                    solite_core::StepResult::DotCommand(dot_command) => todo!(),
                }
            }
        }
    }
    Ok(report)
}

pub(crate) fn codegen(cmd: CodegenArgs) -> Result<(), ()> {
    let src = std::fs::read_to_string(&cmd.file).unwrap();
    let report = report_from_file(&src, &cmd.file).unwrap();
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    Ok(())
}

#[cfg(test)]
mod tests {
    use insta::assert_yaml_snapshot;

    use super::*;

    fn snapshot(src: &str) {
        assert_yaml_snapshot!(report_from_file(src, &PathBuf::from("[fake]")).unwrap());
    }
    #[test]
    fn test_report() {
        let result =
            report_from_file("-- name: xxx\nselect 1, 2, 3;", &PathBuf::from("[fake]")).unwrap();
        assert_yaml_snapshot!(result);
        snapshot(
            r#"
        create table t(a,b text,c int);

        -- name: getA
        select a from t;
        
        -- name: getB
        select b from t;
        
        -- name: getC
        select c from t;

        -- name: withParams :list
        select c from t where a = $a::text and b = $b::text;
      "#,
        );
    }
}
