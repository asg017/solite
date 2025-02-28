use crate::cli::{SnapshotFlags};
use console::{Key, Term};
use similar::{Algorithm, ChangeTag, TextDiff};
use solite_core::sqlite::{
    escape_string, Statement, ValueRefX, ValueRefXValue, JSON_SUBTYPE, POINTER_SUBTYPE,
};
use solite_core::{advance_through_ignorable, BlockSource, Runtime, StepError, StepResult};
use std::fmt::Write as _;
use std::fs::read_to_string;
use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

enum ValueCopyValue {
    Null,
    Int(i64),
    Double(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
    Pointer(Option<String>),
}
struct ValueCopy {
    subtype: Option<u32>,
    value: ValueCopyValue,
}

fn snapshot_value(v: &ValueCopy) -> String {
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
        ValueCopyValue::Pointer(_) => format!("pointer[]"),
    }
}

fn copy<'a>(value: &ValueRefX<'a>) -> ValueCopy {
    let new_value = match value.value {
        ValueRefXValue::Null => match value.subtype() {
            Some(subtype) if subtype == POINTER_SUBTYPE => ValueCopyValue::Pointer(None),
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
    Created,
    NoMatch,
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
                return Some(snapshot_contents)
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

/*

temp.solite_snapshot_base_functions(name)
temp.solite_snapshot_base_modules(name)
temp.solite_snapshot_loaded_functions(name)
temp.solite_snapshot_loaded_modules(name)
temp.solite_snapshot_snapped_statement_bytecode_steps(sql, reference, /* rest is bytecode() */ addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle)

 */


pub(crate) fn snapshot(flags: SnapshotFlags) -> Result<(), ()> {
    let mut rt = Runtime::new(None);
    if let Some(ext) = flags.extension {
        rt.connection.execute("CREATE TABLE solite_snapshot_base_functions AS SELECT name FROM pragma_function_list ORDER BY 1").unwrap();
        rt.connection.execute("CREATE TABLE solite_snapshot_base_modules AS SELECT name FROM pragma_module_list ORDER BY 1").unwrap();
        rt.connection.load_extension(&ext, &None);
        rt.connection.execute("CREATE TABLE solite_snapshot_loaded_functions AS SELECT name FROM pragma_function_list WHERE name NOT IN (SELECT name FROM solite_snapshot_base_functions) ORDER BY 1").unwrap();
        rt.connection.execute("CREATE TABLE solite_snapshot_loaded_modules AS SELECT name FROM pragma_module_list WHERE name NOT IN (SELECT name FROM solite_snapshot_base_functions) ORDER BY 1").unwrap();
        rt.connection.execute("CREATE TABLE solite_snapshot_snapped_statement_bytecode_steps(sql, reference, /* rest is bytecode() */ addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle)").unwrap();

    }
    let script = Path::new(flags.script.as_str());
    let snapshots_dir = script.parent().unwrap().join("snapshots");
    if !snapshots_dir.exists() {
        std::fs::create_dir_all(&snapshots_dir).unwrap();
    }
    let basename = script.file_stem().unwrap().to_string_lossy().to_string();

    let sql = read_to_string(script).unwrap();
    rt.enqueue(
        flags.script.as_str(),
        sql.as_str(),
        BlockSource::File(script.into()),
    );

    let mut snapshot_idx = 1;
    let mut section = None;
    let mut snapshot_results: Vec<SnapshotResult> = vec![];
    loop {
        match rt.next_sql_step() {
            Ok(Some(step)) => match step.result {
                StepResult::SqlStatement{stmt, raw_sql} => {
                    let mut preamble = None;
                    for line in raw_sql.lines() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        // first non-line comment is probably SQL, so break out
                        if !line.trim().starts_with("--") {
                            break;
                        }
                        if let Some(x) = line.trim().strip_prefix("--- ") {
                            preamble = Some(x.to_owned());
                        }
                    }
                    if let Some(Some(x)) = preamble.as_ref().map(|x| x.strip_prefix("# ")) {
                        section = Some(x.to_owned());
                        snapshot_idx = 0;
                    }
                    let snapshot_path = match section {
                        Some(ref section) => snapshots_dir
                            .join(format!("{}-{}-{:02}.snap", basename, section, snapshot_idx)),
                        None => {
                            snapshots_dir.join(format!("{}-{:02}.snap", basename, snapshot_idx))
                        }
                    };

                    let snapshot_contents = generate_snapshot_contents(
                        pathdiff::diff_paths(script, snapshot_path.as_path().parent().unwrap())
                            .unwrap()
                            .to_string_lossy()
                            .to_string(),
                        &stmt,
                    );
                    let snapshot_contents = match snapshot_contents {
                      Some(x) =>x,
                      None => continue,
                    };
                    {
                      let stmt_bytecode = rt.connection.prepare("INSERT INTO solite_snapshot_snapped_statement_bytecode_steps SELECT 'todo', 'todo', addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle FROM bytecode(?)").unwrap().1.unwrap();
                      stmt_bytecode.bind_pointer(1, stmt.pointer().cast(), c"stmt-pointer");
                      stmt_bytecode.execute().unwrap();
                    }


                    if snapshot_path.exists() {
                        let orignal_snapshot = std::fs::read_to_string(&snapshot_path).unwrap();
                        if orignal_snapshot == snapshot_contents {
                            snapshot_results.push(SnapshotResult::Matches);
                        } else {
                            let mut snapshot_file =
                                std::fs::File::create(snapshot_path.with_extension("snap.new"))
                                    .unwrap();
                            snapshot_file
                                .write_all(snapshot_contents.as_bytes())
                                .unwrap();
                            snapshot_results.push(SnapshotResult::NoMatch);

                            print_diff(&orignal_snapshot, &snapshot_contents);
                            print_decision();

                            let term = Term::stdout();
                            match term.read_key().unwrap() {
                                Key::Char('a') | Key::Enter => {
                                    std::fs::OpenOptions::new()
                                        .read(true)
                                        .write(true)
                                        .create(true)
                                        .open(&snapshot_path)
                                        .unwrap()
                                        .write_all(snapshot_contents.as_bytes())
                                        .unwrap();
                                    std::fs::remove_file(snapshot_path.with_extension("snap.new"))
                                        .unwrap();
                                }
                                Key::Char('r') | Key::Escape => println!("rejected"),
                                _ => todo!(),
                            };
                        }
                    } else {

                      print_diff("", &snapshot_contents);
                      print_decision();

                      let term = Term::stdout();
                      match term.read_key().unwrap() {
                          Key::Char('a') | Key::Enter => {
                            let mut snapshot_file = std::fs::File::create(&snapshot_path).unwrap();
                            snapshot_file
                                .write_all(snapshot_contents.as_bytes())
                                .unwrap();
                            snapshot_results.push(SnapshotResult::Created);
                            println!("created {}", &snapshot_path.display());
                          }
                          Key::Char('r') | Key::Escape => println!("rejected"),
                          _ => todo!(),
                      };

                      
                        
                    }
                    snapshot_idx += 1;
                }
                StepResult::DotCommand(cmd) => todo!("no dot commands"),
            },
            Ok(None) => break,
            Err(step_error) => match step_error {
                StepError::Prepare {
                    error,
                    file_name,
                    src,
                    offset,
                } => {
                    crate::errors::report_error(file_name.as_str(), &src, &error, Some(offset));
                }
                StepError::ParseDot(error) => {
                    todo!("no dot")
                }
            },
        }
    }
    // all unreferences loaded functions:
    // select * from solite_snapshot_loaded_functions where name not in (select regex_find('[a-zA-Z_]+', p4) from solite_snapshot_snapped_statement_bytecode_steps where opcode = 'Function');

    let num_created = snapshot_results
        .iter()
        .filter(|v| matches!(v, SnapshotResult::Created))
        .count();
    let num_matches = snapshot_results
        .iter()
        .filter(|v| matches!(v, SnapshotResult::Matches))
        .count();
    let num_nomatches = snapshot_results
        .iter()
        .filter(|v| matches!(v, SnapshotResult::NoMatch))
        .count();
    println!(
        "{} created, {} matches, {} no match",
        num_created, num_matches, num_nomatches
    );
    rt.connection.execute("vacuum into 'tmp.db'").unwrap();

    Ok(())
}
