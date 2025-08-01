use std::{
    fs::OpenOptions,
    io::{stdout, Write},
};

use crate::{
    cli::{DocsCommand, DocsInlineArgs, DocsNamespace},
    errors::{report_error, report_error_string},
    commands::snapshot::{ValueCopy, ValueCopyValue},
    ui::{BORDER, SEPARATOR},
};
use cli_table::{Cell, CellStruct, Table};
use markdown::{
    self,
    mdast::{Node, Text},
};
use mdast_util_to_markdown::to_markdown;
use solite_core::{sqlite::escape_string, Runtime};
use termcolor::ColorChoice;

const BASE_FUNCTIONS_CREATE: &str = r#"
  CREATE TABLE solite_docs.solite_docs_base_functions AS 
    SELECT name 
    FROM pragma_function_list 
    ORDER BY 1
  "#;
const BASE_MODULES_CREATE: &str = r#"
  CREATE TABLE solite_docs.solite_docs_base_modules AS 
    SELECT name 
    FROM pragma_module_list 
    ORDER BY 1
  "#;

const LOADED_FUNCTIONS_CREATE: &str = r#"
  CREATE TABLE solite_docs.solite_docs_loaded_functions AS 
    SELECT name 
    FROM pragma_function_list 
    WHERE name NOT IN (SELECT name FROM solite_docs.solite_docs_base_functions) 
    ORDER BY 1
"#;

const LOADED_MODULES_CREATE: &str = r#"
  CREATE TABLE solite_docs.solite_docs_loaded_modules AS 
    SELECT name 
    FROM pragma_module_list 
    WHERE name NOT IN (SELECT name FROM solite_docs.solite_docs_base_modules) 
    ORDER BY 1
"#;

pub(crate) fn display_value(v: &ValueCopy) -> String {
    match &v.value {
        ValueCopyValue::Null | ValueCopyValue::Pointer => "NULL".to_string(),
        ValueCopyValue::Int(value) => value.to_string(),
        ValueCopyValue::Double(value) => value.to_string(),
        ValueCopyValue::Text(value) => {
            escape_string(String::from_utf8_lossy(&value).to_string().as_str())
        }
        // hex value of u8
        ValueCopyValue::Blob(value) => format!("X'{}'", hex::encode(value).to_uppercase()),
    }
}

fn table(columns: &Vec<String>, results: &Vec<Vec<ValueCopy>>) -> String {
    let rows: Vec<Vec<CellStruct>> = results
        .iter()
        .map(|row| row.iter().map(|v| display_value(v).cell()).collect())
        .collect();
    rows.table()
        .title(columns)
        .border(*BORDER)
        .separator(*SEPARATOR)
        .color_choice(ColorChoice::Never)
        .display()
        .unwrap()
        .to_string()
}

fn inline(args: DocsInlineArgs) -> Result<(), ()> {
    let rt = Runtime::new(None);
    rt.connection
        .execute("ATTACH DATABASE ':memory:' AS solite_docs")
        .unwrap();

    if let Some(ext) = args.extension {
        rt.connection.execute(BASE_FUNCTIONS_CREATE).unwrap();
        rt.connection.execute(BASE_MODULES_CREATE).unwrap();
        rt.connection.load_extension(&ext, &None);
        rt.connection.execute(LOADED_FUNCTIONS_CREATE).unwrap();
        rt.connection.execute(LOADED_MODULES_CREATE).unwrap();
    }
    let docs_in = std::fs::read_to_string(&args.input).unwrap();
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.frontmatter = true;
    let mut ast = markdown::to_mdast(&docs_in, &options).unwrap();

    for node in ast.children_mut().unwrap() {
        node.position().map(|p| p.start.offset);
        match node {
            Node::Code(code) => {
                let sql = code.value.clone();
                let mut new_value = String::new();
                let mut curr = sql.as_str();
                loop {
                    match rt.prepare_with_parameters(curr) {
                        Ok((rest, Some(stmt))) => {
                            new_value.push_str(&stmt.sql());
                            new_value.push('\n');

                            let columns = stmt.column_names().unwrap();
                            match columns.len() {
                                0 => {
                                    stmt.execute().unwrap();
                                }
                                _n => {
                                    let mut results: Vec<Vec<ValueCopy>> = vec![];
                                    loop {
                                        match stmt.next() {
                                            Ok(Some(row)) => {
                                                let row = row
                                                    .iter()
                                                    .map(|v| crate::commands::snapshot::copy(v))
                                                    .collect();
                                                results.push(row);
                                            }
                                            Ok(None) => break,
                                            Err(error) => {
                                                report_error(
                                                    args.input.to_string_lossy().as_ref(),
                                                    &stmt.sql(),
                                                    &error,
                                                    None,
                                                );
                                                return Err(());
                                            }
                                        }
                                    }

                                    match results.len() {
                                        0 => new_value.push_str("No results\n"),
                                        1 => {
                                            new_value.push_str(
                                                format!(
                                                    "-- {}",
                                                    crate::commands::snapshot::snapshot_value(&results[0][0])
                                                )
                                                .as_str(),
                                            );
                                        }
                                        _n => {
                                            new_value.push_str("/*\n");
                                            new_value.push_str(&table(&columns, &results));
                                            new_value.push_str("*/");
                                        }
                                    }
                                }
                            }

                            if let Some(rest) = rest {
                                if let Some(x) = curr.get(rest..) {
                                    curr = x;
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        Ok((_, None)) => break,
                        Err(error) => {
                            println!("{}", report_error_string("TODO", &sql, &error, None));
                            panic!();
                        }
                    }
                }
                code.value = new_value;
            }
            _ => (),
        }
    }

    let documented_funcs: Vec<String> = ast
        .children_mut()
        .unwrap()
        .iter_mut()
        .filter_map(|mut n| match n {
            Node::Heading(heading) => {
                if heading.depth == 3 || heading.depth == 4 {
                    let t = n.children().unwrap().first().unwrap();
                    let function = match t {
                        Node::InlineCode(c) => match c.value.split_once('(') {
                            Some((f, _)) => Some(f.to_owned()),
                            None => Some(c.value.clone()),
                        },
                        _ => None,
                    };

                    if let Some(f) = &function {
                        n.children_mut().unwrap().push(Node::Text(Text {
                            value: format!(" {{#{}}}", f.replace('_', "ю")),
                            position: None,
                        }));
                    }

                    function
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();
    let loaded_funcs: Vec<String> = {
        let stmt = rt
            .connection
            .prepare("SELECT name FROM solite_docs.solite_docs_loaded_functions")
            .unwrap()
            .1
            .unwrap();
        let mut funcs = vec![];
        loop {
            match stmt.next() {
                Ok(Some(row)) => {
                    funcs.push(row[0].as_str().to_owned());
                }
                Ok(None) => break,
                Err(_err) => todo!(),
            }
        }
        funcs
    };
    // get vec of items in loaded_funcs but not in documented_funcs
    let mut undocumented_funcs: Vec<String> = loaded_funcs
        .iter()
        .filter(|f| !documented_funcs.contains(f))
        .cloned()
        .collect();

    let out_md = to_markdown(&ast).unwrap().replace("ю", "_");
    match args.output {
        Some(output) => {
            let mut f = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&output)
                .unwrap();
            f.write_all(out_md.as_bytes()).unwrap();

            println!("Wrote docs to {}", output.to_string_lossy());
        }
        None => {
            writeln!(stdout(), "{}", out_md).unwrap();
        }
    }

    if undocumented_funcs.len() > 0 {
        undocumented_funcs.sort();
        eprintln!("The following functions are not documented:");
        for func in undocumented_funcs {
            eprintln!("  - {func}");
        }
        return Err(());
    }
    Ok(())
}

pub(crate) fn docs(cmd: DocsNamespace) -> Result<(), ()> {
    match cmd.command {
        DocsCommand::Inline(args) => inline(args),
    }
}
