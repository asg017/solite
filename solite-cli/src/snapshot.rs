use crate::cli::SnapshotFlags;
use console::{Key, Term};
use indicatif::HumanBytes;
use regex::Regex;
use similar::{Algorithm, ChangeTag, TextDiff};
use solite_core::sqlite::{
    escape_string, Statement, ValueRefX, ValueRefXValue, JSON_SUBTYPE, POINTER_SUBTYPE,
};
use solite_core::{advance_through_ignorable, BlockSource, Runtime, StepError, StepResult};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::read_to_string;
use std::io::Write as _;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

pub(crate) enum ValueCopyValue {
    Null,
    Int(i64),
    Double(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),

    // TODO: eventuall add Option<String> here for recognized pointer names
    Pointer,
}
pub(crate) struct ValueCopy {
    subtype: Option<u32>,
    pub(crate) value: ValueCopyValue,
}

pub(crate) fn snapshot_value(v: &ValueCopy) -> String {
    match &v.value {
        ValueCopyValue::Null => "NULL".to_string(),
        ValueCopyValue::Int(value) => value.to_string(),
        ValueCopyValue::Double(value) => value.to_string(),
        ValueCopyValue::Text(value) => {
            let value = escape_string(String::from_utf8_lossy(&value).to_string().as_str());
            if let Some(subtype) = v.subtype {
                if subtype == JSON_SUBTYPE {
                    // Ⓙ
                    return format!("(json) {}", value);
                }
            }
            value
        }
        // hex value of u8
        ValueCopyValue::Blob(value) => format!("X'{}'", hex::encode(value)),
        ValueCopyValue::Pointer => format!("pointer[]"),
    }
}

pub(crate) fn copy<'a>(value: &ValueRefX<'a>) -> ValueCopy {
    let new_value = match value.value {
        ValueRefXValue::Null => match value.subtype() {
            Some(subtype) if subtype == POINTER_SUBTYPE => ValueCopyValue::Pointer,
            _ => ValueCopyValue::Null,
        },
        ValueRefXValue::Int(value) => ValueCopyValue::Int(value),
        ValueRefXValue::Double(value) => ValueCopyValue::Double(value),
        ValueRefXValue::Text(value) => ValueCopyValue::Text(value.to_vec()),
        ValueRefXValue::Blob(value) => ValueCopyValue::Blob(value.to_vec()),
    };

    ValueCopy {
        subtype: value.subtype(),
        value: new_value,
    }
}
pub fn dedent(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();

    let min_indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty()) // Ignore empty lines
        .map(|line| line.chars().take_while(|c| c.is_whitespace()).count())
        .min()
        .unwrap_or(0);

    lines
        .iter()
        .map(|line| {
            if line.len() >= min_indent {
                &line[min_indent..]
            } else {
                line
            }
        })
        .collect::<Vec<&str>>()
        .join("\n")
}

enum SnapshotResult {
    Matches,
    Accepted,
    Rejected,
    Removed,
}

fn print_diff(original_snapshot: &str, new_snapshot: &str) {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .timeout(Duration::from_millis(500))
        .diff_lines(original_snapshot, new_snapshot);

    let width = console::Term::stdout().size().1 as usize;
    println!("────────────┬{:─^1$}", "", width.saturating_sub(13));
    for (idx, group) in diff.grouped_ops(4).iter().enumerate() {
        if idx > 0 {
            println!("┈┈┈┈┈┈┈┈┈┈┈┈┼{:┈^1$}", "", width.saturating_sub(13));
        }
        for op in group {
            for change in diff.iter_inline_changes(op) {
                match change.tag() {
                    ChangeTag::Insert => {
                        print!(
                            "{:>5} {:>5} │{}",
                            "",
                            console::style(change.new_index().unwrap().to_string())
                                .cyan()
                                .bold()
                                .dim(),
                            console::style("+").green(),
                        );
                        for &(emphasized, change) in change.values() {
                            if emphasized {
                                print!("{}", console::style(change).green().underlined());
                            } else {
                                print!("{}", console::style(change).green());
                            }
                        }
                    }
                    ChangeTag::Delete => {
                        print!(
                            "{:>5} {:>5} │{}",
                            console::style(change.old_index().unwrap().to_string())
                                .cyan()
                                .dim(),
                            "",
                            console::style("-").red(),
                        );
                        for &(emphasized, change) in change.values() {
                            if emphasized {
                                print!("{}", console::style(change).red().underlined());
                            } else {
                                print!("{}", console::style(change).red());
                            }
                        }
                    }
                    ChangeTag::Equal => {
                        print!(
                            "{:>5} {:>5} │ ",
                            console::style(&change.old_index().unwrap().to_string())
                                .cyan()
                                .dim(),
                            console::style(&change.new_index().unwrap().to_string())
                                .cyan()
                                .dim()
                                .bold(),
                            //cyan_bold(change.new_index().unwrap().to_string()),
                        );
                        for &(_, change) in change.values() {
                            print!("{}", console::style(change).dim());
                        }
                    }
                }
            }
        }
    }
    println!("────────────┴{:─^1$}", "", width.saturating_sub(13));
}

fn print_decision() {
    println!(
        "  {} accept     {}",
        console::style("a").green().bold(),
        console::style("keep the new snapshot").dim()
    );

    println!(
        "  {} reject     {}",
        console::style("r").red().bold(),
        console::style("reject the new snapshot").dim()
    );
}
fn generate_snapshot_contents(source: String, stmt: &Statement) -> Option<String> {
    let mut snapshot_contents = String::new();
    write!(
        &mut snapshot_contents,
        "Source: {}\n{}\n---\n",
        source,
        (dedent(advance_through_ignorable(&stmt.sql())))
    )
    .unwrap();

    let columns = stmt.column_names().unwrap();
    let mut results: Vec<Vec<ValueCopy>> = vec![];
    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                let row = row.iter().map(|v| copy(v)).collect();
                results.push(row);
            }
            Ok(None) => break,
            Err(err) => {
                writeln!(
                    &mut snapshot_contents,
                    "ERROR[{}] {}\n{}",
                    err.result_code, err.code_description, err.message
                )
                .unwrap();
                return Some(snapshot_contents);
            }
        }
    }

    // single value result (ex `select 1`)
    if columns.len() == 1 && results.len() == 1 {
        write!(&mut snapshot_contents, "{}", snapshot_value(&results[0][0])).unwrap();
    }
    // no columns and no results (ex `create table foo`)
    else if columns.len() == 0 && results.len() == 0 {
        return None;
    }
    // no row results  (but still had columns)
    else if results.len() == 0 {
        write!(&mut snapshot_contents, "[no results]").unwrap();
    }
    // multiple rows
    else {
        for row in results {
            write!(&mut snapshot_contents, "{{\n").unwrap();
            row.iter()
                .zip(&columns)
                .enumerate()
                .for_each(|(_idx, (value, column_name))| {
                    writeln!(
                        &mut snapshot_contents,
                        "\t {}: {}",
                        column_name,
                        snapshot_value(value)
                    )
                    .unwrap();
                });
            write!(&mut snapshot_contents, "}}\n").unwrap();
        }
    }
    writeln!(&mut snapshot_contents).unwrap();
    Some(snapshot_contents)
}

const BASE_FUNCTIONS_CREATE: &str = r#"
  CREATE TABLE solite_snapshot.solite_snapshot_base_functions AS 
    SELECT name 
    FROM pragma_function_list 
    ORDER BY 1
  "#;
const BASE_MODULES_CREATE: &str = r#"
  CREATE TABLE solite_snapshot.solite_snapshot_base_modules AS 
    SELECT name 
    FROM pragma_module_list 
    ORDER BY 1
  "#;

const LOADED_FUNCTIONS_CREATE: &str = r#"
  CREATE TABLE solite_snapshot.solite_snapshot_loaded_functions AS 
    SELECT name 
    FROM pragma_function_list 
    WHERE name NOT IN (SELECT name FROM solite_snapshot_base_functions) 
    ORDER BY 1
"#;

const LOADED_MODULES_CREATE: &str = r#"
  CREATE TABLE solite_snapshot.solite_snapshot_loaded_modules AS 
    SELECT name 
    FROM pragma_module_list 
    WHERE name NOT IN (SELECT name FROM solite_snapshot_base_modules) 
    ORDER BY 1
"#;

const SNAPPED_STATEMENT_CREATE: &str = r#"
  CREATE TABLE solite_snapshot.solite_snapshot_snapped_statement(
    id integer primary key autoincrement,
    sql text,
    reference text,
    execution_start integer,
    execution_end integer
  )
"#;

const SNAPPED_STATEMENT_INSERT: &str = r#"
  INSERT INTO solite_snapshot.solite_snapshot_snapped_statement(sql, reference) VALUES
    (?, ?)
  RETURNING id;
"#;
const SNAPPED_STATEMENT_BYTECODE_STEPS_CREATE: &str = r#"
  CREATE TABLE solite_snapshot.solite_snapshot_snapped_statement_bytecode_steps(
    statement_id integer references solite_snapshot_snapped_statement(id),
    /* rest is bytecode() */ 
    addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle
)"#;

const SNAPPED_STATEMENT_BYTECODE_STEPS_INSERT: &str = r#"
  INSERT INTO solite_snapshot.solite_snapshot_snapped_statement_bytecode_steps 
    SELECT ?, addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle 
    FROM bytecode(?)
"#;

const SNAPSHOT_FUNCTIONS_REPORT_SQL: &str = include_str!("snapshot-functions-report.sql");
const SNAPSHOT_MODULES_REPORT_SQL: &str = include_str!("snapshot-modules-report.sql");

static SQL_COMMENT_REGION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*--\s*#region\s+(\w*)").unwrap());
static SQL_COMMENT_ENDREGION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*--\s*#endregion").unwrap());

fn sql_comment_region_name(sql: &str) -> Option<&str> {
    //SQL_COMMENT_REGION.captures_at(sql, 1).map(|x| x)
    SQL_COMMENT_REGION
        .captures(sql)
        .and_then(|captures| captures.get(1).map(|m| m.as_str()))
}

struct ExtensionsReport {
    num_functions_loaded: usize,
    missing_functions: Vec<String>,
    num_modules_loaded: usize,
    missing_modules: Vec<String>,
}

struct Report {
    num_matches: usize,
    num_updated: usize,
    num_rejected: usize,
    num_removed: usize,
    extensions_report: Option<ExtensionsReport>,
}

fn snapshot_report(
    rt: &Runtime,
    snapshot_results: &Vec<SnapshotResult>,
    loaded_extension: bool,
) -> Report {
    let num_matches = snapshot_results
        .iter()
        .filter(|v| matches!(v, SnapshotResult::Matches))
        .count();
    let num_updated = snapshot_results
        .iter()
        .filter(|v| matches!(v, SnapshotResult::Accepted))
        .count();
    let num_rejected = snapshot_results
        .iter()
        .filter(|v| matches!(v, SnapshotResult::Rejected))
        .count();
    let num_removed = snapshot_results
        .iter()
        .filter(|v| matches!(v, SnapshotResult::Removed))
        .count();

    let extensions_report = if loaded_extension {
        let stmt = rt
            .connection
            .prepare(SNAPSHOT_FUNCTIONS_REPORT_SQL)
            .unwrap()
            .1
            .unwrap();
        let row = stmt.nextx().unwrap().unwrap();
        let num_functions_loaded = row.value_at(0).as_int64() as usize;
        let missing_functions: Vec<String> =
            serde_json::from_str(row.value_at(1).as_str()).unwrap();
        drop(stmt);

        let stmt = rt
            .connection
            .prepare(SNAPSHOT_MODULES_REPORT_SQL)
            .unwrap()
            .1
            .unwrap();
        let row = stmt.nextx().unwrap().unwrap();
        let num_modules_loaded = row.value_at(0).as_int64() as usize;
        let missing_modules: Vec<String> = serde_json::from_str(row.value_at(1).as_str()).unwrap();

        drop(stmt);

        Some(ExtensionsReport {
            num_functions_loaded,
            missing_functions,
            num_modules_loaded,
            missing_modules,
        })
    } else {
        None
    };

    Report {
        num_matches,
        num_updated,
        num_rejected,
        num_removed,
        extensions_report,
    }
}

pub(crate) fn snapshot(flags: SnapshotFlags) -> Result<(), ()> {
    let _started = std::time::Instant::now();
    let mut rt = Runtime::new(None);
    rt.connection
        .execute("ATTACH DATABASE ':memory:' AS solite_snapshot")
        .unwrap();

  /*
    if let Some(ext) = &flags.extension {
        rt.connection.execute(BASE_FUNCTIONS_CREATE).unwrap();
        rt.connection.execute(BASE_MODULES_CREATE).unwrap();
        rt.connection.load_extension(&ext, &None);
        rt.connection.execute(LOADED_FUNCTIONS_CREATE).unwrap();
        rt.connection.execute(LOADED_MODULES_CREATE).unwrap();
        rt.connection.execute(SNAPPED_STATEMENT_CREATE).unwrap();
        rt.connection
            .execute(SNAPPED_STATEMENT_BYTECODE_STEPS_CREATE)
            .unwrap();
    } */
    let script = Path::new(flags.script.as_str());
    let snapshots_dir = script.parent().unwrap().join("__snapshots__");
    if !snapshots_dir.exists() {
        std::fs::create_dir_all(&snapshots_dir).unwrap();
    }
    let preexisting_snapshots: Vec<String> = std::fs::read_dir(&snapshots_dir)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                Some(entry.file_name().to_str().unwrap().to_owned())
            } else {
                None
            }
        })
        .collect();
    let mut generated_snapshots = vec![];

    let basename = script.file_stem().unwrap().to_string_lossy().to_string();

    let sql = read_to_string(script).unwrap();
    rt.enqueue(
        flags.script.as_str(),
        sql.as_str(),
        BlockSource::File(script.into()),
    );

    let mut snapshot_results: Vec<SnapshotResult> = vec![];
    let mut snapshot_idx_map:HashMap<String, usize> = HashMap::new();
    loop {
        match rt.next_stepx() {
            Some(Ok(step)) => match step.result {
                StepResult::SqlStatement { stmt, raw_sql } => {
                    let region_section = step.reference.region.join("-");
                    let snapshot_idx = snapshot_idx_map.entry(region_section.clone()).or_insert(0);
                    let snapshot_path =snapshots_dir.join(format!("{}-{}{:02}.snap", basename, if region_section.is_empty() {"".to_owned()} else {format!("{region_section}-")}, snapshot_idx));
                    *snapshot_idx += 1;

                    let mut statement_id = 0;
                    {
                        let insert = rt
                            .connection
                            .prepare(SNAPPED_STATEMENT_INSERT)
                            .unwrap()
                            .1
                            .unwrap();
                        insert.bind_text(1, stmt.sql());
                        insert.bind_text(2, step.reference.to_string());
                        statement_id = insert.nextx().unwrap().unwrap().value_at(0).as_int64();
                    }
                    if flags.verbose {
                        println!("{}", stmt.sql());
                    }

                    let snapshot_contents = generate_snapshot_contents(
                        pathdiff::diff_paths(script, snapshot_path.as_path().parent().unwrap())
                            .unwrap()
                            .to_string_lossy()
                            .to_string(),
                        &stmt,
                    );

                    {
                        let stmt_bytecode = rt
                            .connection
                            .prepare(SNAPPED_STATEMENT_BYTECODE_STEPS_INSERT)
                            .unwrap()
                            .1
                            .unwrap();
                        stmt_bytecode.bind_int64(1, statement_id);
                        stmt_bytecode.bind_pointer(2, stmt.pointer().cast(), c"stmt-pointer");
                        stmt_bytecode.execute().unwrap();
                    }

                    let snapshot_contents = match snapshot_contents {
                        Some(x) => x,
                        None => continue,
                    };

                    if snapshot_path.exists() {
                        generated_snapshots.push(
                            snapshot_path
                                .file_name()
                                .unwrap()
                                .to_string_lossy()
                                .to_string(),
                        );
                        let orignal_snapshot = std::fs::read_to_string(&snapshot_path).unwrap();
                        if orignal_snapshot == snapshot_contents {
                            snapshot_results.push(SnapshotResult::Matches);
                        } else {
                            println!(
                                "{} changed at {}",
                                step.reference.to_string(),
                                &snapshot_path.display()
                            );
                            print_diff(&orignal_snapshot, &snapshot_contents);
                            print_decision();

                            let term = Term::stdout();
                            match term.read_key().unwrap() {
                                Key::Char('a') | Key::Char('A') | Key::Enter => {
                                    snapshot_results.push(SnapshotResult::Accepted);

                                    std::fs::OpenOptions::new()
                                        .read(true)
                                        .write(true)
                                        .create(true)
                                        .truncate(true)
                                        .open(&snapshot_path)
                                        .unwrap()
                                        .write_all(snapshot_contents.as_bytes())
                                        .unwrap();
                                }
                                Key::Char('r') | Key::Char('R') | Key::Escape => {
                                    snapshot_results.push(SnapshotResult::Rejected);
                                }
                                _ => todo!(),
                            };
                        }
                    } else {
                        println!("{}", &snapshot_path.display());
                        print_diff("", &snapshot_contents);
                        print_decision();

                        let term = Term::stdout();
                        match term.read_key().unwrap() {
                            Key::Char('a') | Key::Enter => {
                                let mut snapshot_file = std::fs::OpenOptions::new()
                                    .write(true)
                                    .truncate(true)
                                    .create_new(true)
                                    .open(&snapshot_path)
                                    .unwrap();
                                snapshot_file
                                    .write_all(snapshot_contents.as_bytes())
                                    .unwrap();
                                snapshot_results.push(SnapshotResult::Accepted);
                                println!("created {}", &snapshot_path.display());
                                generated_snapshots.push(
                                    snapshot_path
                                        .file_name()
                                        .unwrap()
                                        .to_string_lossy()
                                        .to_string(),
                                );
                            }
                            Key::Char('r') | Key::Escape => {
                                snapshot_results.push(SnapshotResult::Rejected);
                            }
                            _ => todo!(),
                        };
                    }
                }
                StepResult::DotCommand(cmd) => match cmd {
                    solite_core::dot::DotCommand::Load(load_command) => {
                      
                      rt.connection.execute(BASE_FUNCTIONS_CREATE).unwrap();
                      rt.connection.execute(BASE_MODULES_CREATE).unwrap();
                      load_command.execute(&mut rt.connection);
                      rt.connection.execute(LOADED_FUNCTIONS_CREATE).unwrap();
                      rt.connection.execute(LOADED_MODULES_CREATE).unwrap();
                      rt.connection.execute(SNAPPED_STATEMENT_CREATE).unwrap();
                      rt.connection
                          .execute(SNAPPED_STATEMENT_BYTECODE_STEPS_CREATE)
                          .unwrap();
                    }
                    solite_core::dot::DotCommand::Tables(tables_command) => todo!(),
                    solite_core::dot::DotCommand::Open(open_command) => {
                        open_command.execute(&mut rt);
                    }
                    solite_core::dot::DotCommand::Print(print_command) => print_command.execute(),
                    solite_core::dot::DotCommand::Parameter(parameter_command) => todo!(),
                    solite_core::dot::DotCommand::Timer(_) => todo!(),
                },
            },
            None => break,
            Some(Err(step_error)) => match step_error {
                StepError::Prepare {
                    error,
                    file_name,
                    src,
                    offset,
                } => {
                    crate::errors::report_error(file_name.as_str(), &src, &error, Some(offset));
                }
                StepError::ParseDot(_error) => {
                    todo!("no dot")
                }
            },
        }
    }

    let x: HashSet<String> = generated_snapshots.into_iter().collect();
    let y: HashSet<String> = preexisting_snapshots.into_iter().collect();
    // preexisting snapshots that were not generated
    let diff = y.difference(&x);
    for d in diff {
        print_diff(&std::fs::read_to_string(snapshots_dir.join(d)).unwrap(), "");
        println!("Remove {}? [y/n]", snapshots_dir.join(d).display());
        let term = Term::stdout();
        match term.read_key().unwrap() {
            Key::Char('y') | Key::Char('Y') => {
                std::fs::remove_file(snapshots_dir.join(d)).unwrap();
                snapshot_results.push(SnapshotResult::Removed);
            }
            Key::Char('n') | Key::Char('N') | Key::Escape => {
                println!("AAAAAHHHH; ");
            }
            _ => todo!(),
        };
    }

    let report = snapshot_report(&rt, &snapshot_results, true);//flags.extension.is_some());

    println!(
        "{:>4} passing snapshot{}",
        report.num_matches,
        if report.num_matches == 1 { "" } else { "s" }
    );
    if report.num_updated > 0 {
        println!(
            "{:>4} updated snapshot{}",
            report.num_updated,
            if report.num_updated == 1 { "" } else { "s" }
        );
    }
    if report.num_rejected > 0 {
        println!(
            "{:>4} rejected snapshot{}",
            report.num_rejected,
            if report.num_rejected == 1 { "" } else { "s" }
        );
    }
    if report.num_removed > 0 {
        println!(
            "{:>4} removed snapshot{}",
            report.num_removed,
            if report.num_removed == 1 { "" } else { "s" }
        );
    }

    if let Some(report) = report.extensions_report {
        println!(
            "{}/{} functions loaded from extension",
            report.num_functions_loaded,
            report.num_functions_loaded + report.missing_functions.len()
        );
        if report.missing_functions.len() > 0 {
            println!(
                "{} function{} missing from extension",
                report.missing_functions.len(),
                if report.missing_functions.len() == 1 {
                    ""
                } else {
                    "s"
                }
            );

            for missing in report.missing_functions {
                println!("  - {}", missing);
            }
        }

        println!(
            "{}/{} modules tested{}",
            report.num_modules_loaded,
            report.num_modules_loaded + report.missing_modules.len(),
            if  report.missing_modules.len() > 0 {
              ", missing:"
            }else {""}
        );
        for missing in report.missing_modules {
            println!("  - {}", missing);
        }
    }

    if let Some(output) = &flags.output {
        if output.exists() {
            std::fs::remove_file(output).unwrap();
        }
        let stmt = rt
            .connection
            .prepare("vacuum solite_snapshot into ?")
            .unwrap()
            .1
            .unwrap();
        stmt.bind_text(1, output.to_str().unwrap());
        stmt.execute().unwrap();
        let len = output.metadata().unwrap().len();
        println!(
            "Wrote tracing output to {} ({})",
            output.display(),
            HumanBytes(len)
        );
    }

    Ok(())
}
