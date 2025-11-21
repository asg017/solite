use crate::cli::{ReplArgs, RunArgs};
use crate::colors;
use cli_table::print_stdout;
use crossterm::{
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
};
use indicatif::HumanCount;
use jiff::fmt::friendly::{FractionalUnit, SpanPrinter};
use jiff::{SpanRound, Timestamp, ToSpan};
use nbformat::{parse_notebook, Notebook};
use solite_core::dot::sh::ShellResult;
use solite_core::sqlite::Statement;
use solite_core::{
    dot::DotCommand,
    sqlite::{bytecode_steps, sqlite3_stmt},
    BlockSource, Runtime, StepError, StepResult,
};
use std::ffi::OsStr;
use std::fs::read_to_string;
use std::io::stdout;
use std::time::Duration;

pub(crate) fn format_duration(duration: Duration) -> String {
    if duration < (1 * Duration::from_millis(5)) {
        format!("{:.3}ms", duration.as_secs_f32() / 0.001)
    } else if duration < (1 * Duration::from_secs(1)) {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{:.2}s", duration.as_secs_f32())
    }
}

#[derive(Debug)]
enum InProgressStatementStatus {
    Insert {
        num_inserts: i64,
        name: Option<String>,
    },
    Delete {
        num_deletes: i64,
    },
    Update {
        num_updates: i64,
    },
    Unknown,
}

fn stmt_status(stmt: *mut sqlite3_stmt) -> InProgressStatementStatus {
    let steps = bytecode_steps(stmt);
    let deletes = steps
        .iter()
        .filter(|step| step.opcode == "Delete")
        .collect::<Vec<_>>();
    if deletes.len() > 0 {
        assert!(deletes.len() == 1);
        let num_deletes = deletes[0].nexec;
        return InProgressStatementStatus::Delete { num_deletes };
    }

    let inserts = steps
        .iter()
        .filter(|step| step.opcode == "Insert")
        .collect::<Vec<_>>();
    if inserts.len() > 0 {
        // multiple inserts happen on 'CREATE TABLE AS ...' and others??
        let insert = inserts.iter().max_by_key(|step| step.nexec).unwrap();
        if insert.p5 & 0x04 > 0
        /* OPFLAG_ISUPDATE */
        {
            return InProgressStatementStatus::Update {
                num_updates: insert.nexec,
            };
        } else {
            let name = if insert.p4.is_empty() {
                for step in steps.iter() {
                    if step.opcode == "String" && step.p4.starts_with("CREATE TABLE ") {
                        //dbg!(&step.p4);
                    }
                }
                match steps.iter().find(|step| {
                    (step.opcode == "String" || step.opcode == "String8")
                        && step.p4.starts_with("CREATE TABLE ")
                }) {
                    Some(step) => {
                        // if step.p4 is "CREATE TABLE foo (bar)" then name is "foo"
                        // TODO fix this garbage
                        let table_name = step.p4.split_whitespace().nth(2).unwrap();
                        let table_name = table_name.split('(').next().unwrap().trim();
                        Some(table_name.to_owned())
                    }
                    None => None,
                }
            } else {
                Some(insert.p4.to_owned())
            };
            return InProgressStatementStatus::Insert {
                num_inserts: insert.nexec,
                name,
            };
        }
    }

    InProgressStatementStatus::Unknown
}

fn handle_dot_command(runtime: &mut Runtime, cmd: &mut DotCommand, timer: &mut bool) {
    match cmd {
        DotCommand::Ask(cmd) => todo!(),
        DotCommand::Tui(cmd) => todo!(),
        DotCommand::Clear(cmd) => todo!(),
        DotCommand::Dotenv(cmd) => {
            cmd.execute();
        }
        DotCommand::Tables(cmd) => {
            let tables = cmd.execute(&runtime);
            for table in tables {
                println!("{table}");
            }
        }
        DotCommand::Schema(cmd) => {
            let creates = cmd.execute(&runtime);
            for create in creates {
                println!("{create}");
            }
        }
        DotCommand::Graphviz(cmd) => {
            let creates = cmd.execute(&runtime);
            println!("{}", creates);
        }
        DotCommand::Print(print_cmd) => print_cmd.execute(),
        DotCommand::Load(load_cmd) => match load_cmd.execute(&mut runtime.connection) {
            Ok(_) => {
                println!("{} extension loaded", colors::green("✓"));
            }
            Err(err) => {
                eprintln!("Error loading extension: {err}");
            }
        },
        DotCommand::Open(open_cmd) => open_cmd.execute(runtime),
        DotCommand::Timer(enabled) => {
            *timer = *enabled;
            println!(
                "{} timer set {}",
                colors::green("✓"),
                if *enabled { "on" } else { "off" }
            );
        }
        DotCommand::Parameter(param_cmd) => match param_cmd {
            solite_core::dot::ParameterCommand::Set { key, value } => {
                runtime
                    .define_parameter(key.clone(), value.to_owned())
                    .unwrap();
                println!("{} parameter {} set", colors::green("✓"), key);
            }
            solite_core::dot::ParameterCommand::Unset(_) => todo!(),
            solite_core::dot::ParameterCommand::List => todo!(),
            solite_core::dot::ParameterCommand::Clear => todo!(),
        },
        DotCommand::Export(cmd) => match cmd.execute() {
            Ok(()) => {
                println!(
                    "{} exported results to {}",
                    colors::green("✓"),
                    cmd.target.to_string_lossy()
                );
            }
            Err(e) => {
                eprintln!(
                    "Error exporting results to {}\n {}",
                    cmd.target.to_string_lossy(),
                    e
                );
            }
        },
        DotCommand::Shell(shell_command) => match shell_command.execute() {
            ShellResult::Background(child) => {
                println!("✓ started background process with PID {}", child.id());
            }
            ShellResult::Stream(rx) => {
                while let Ok(msg) = rx.recv() {
                    println!("{}", msg);
                }
            }
        },
        DotCommand::Vegalite(vega_lite_command) => todo!(),
        DotCommand::Bench(cmd) => {
            todo!();
        }
    }
}

fn handle_sql(
    runtime: &mut Runtime,
    stmt: &Statement,
    step_reference: &str,
    is_trace: bool,
    timer: bool,
) {
    let pb = indicatif::ProgressBar::new_spinner();
    pb.set_style(
        indicatif::ProgressStyle::with_template("{spinner:.cyan} {elapsed} {wide_msg}")
            .unwrap()
            .tick_chars("⣾⣷⣯⣟⡿⢿⣻⣽"),
    );

    let trace_stmt_id = if is_trace {
        let x = runtime
            .connection
            .prepare("INSERT INTO solite_trace.statements (sql) VALUES (?) RETURNING id")
            .unwrap()
            .1
            .unwrap();
        x.bind_text(1, stmt.sql());
        let id = x.nextx().unwrap().unwrap().value_at(0).as_int64();
        Some(id)
    } else {
        None
    };

    let start = jiff::Timestamp::now();
    let preamble2 = stmt.sql();
    let p = stmt.pointer();
    let r = step_reference.to_string();
    let pbx = pb.clone();
    runtime.connection.set_progress_handler(
        500_000,
        Some(move |(stmt, start): &(*mut sqlite3_stmt, Timestamp)| {
            if (*start - Timestamp::now())
                .compare(42.milliseconds())
                .unwrap()
                .is_gt()
            {
                return false;
            }
            let msg = match stmt_status(*stmt) {
                InProgressStatementStatus::Delete { num_deletes } => {
                    format!("delete: {num_deletes}")
                }
                InProgressStatementStatus::Insert { num_inserts, name } => {
                    format!(
                        "inserting {} rows{}",
                        HumanCount(num_inserts.try_into().unwrap()),
                        match name {
                            Some(name) => format!(" into {name}"),
                            None => "".to_string(),
                        }
                    )
                }
                InProgressStatementStatus::Update { num_updates } => {
                    format!("update: {num_updates}")
                }
                InProgressStatementStatus::Unknown => format!("unknown"),
            };
            let duration = Timestamp::now() - *start;
            let round = SpanRound::new();
            round.largest(jiff::Unit::Millisecond);
            let mut printer = SpanPrinter::new()
                .hours_minutes_seconds(true)
                .fractional(Some(FractionalUnit::Second));
            let mut buf = String::new();
            printer.print_span(&duration, &mut buf).unwrap();
            pbx.clone()
                .with_elapsed(Duration::from_millis(
                    duration.total(jiff::Unit::Millisecond).unwrap() as u64,
                ))
                .with_message(format!("{r} {msg}"))
                .tick();
            false
        }),
        (p, start),
    );

    let start = std::time::Instant::now();
    pb.finish_and_clear();
    let table = crate::ui::table_from_statement(&stmt, Some(&crate::ui::CTP_MOCHA_THEME));

    match table {
        Ok(Some(table)) => print_stdout(table).unwrap(),
        Ok(None) => {}
        Err(err) => {
            eprintln!("Error: {err}");
        }
    }

    let status = stmt_status(stmt.pointer());
    let msg = match status {
        InProgressStatementStatus::Insert { num_inserts, name } => {
            format!(
                "inserted {} rows into {} ",
                HumanCount(num_inserts as u64),
                name.unwrap_or("???".to_string())
            )
        }
        InProgressStatementStatus::Delete { num_deletes } => {
            format!("deleted {} rows ", HumanCount(num_deletes as u64))
        }
        InProgressStatementStatus::Update { num_updates } => {
            format!("updated {} rows ", HumanCount(num_updates as u64))
        }
        InProgressStatementStatus::Unknown => format!(""),
    };

    if timer {
        execute!(
            stdout(),
            SetForegroundColor(Color::Green),
            Print("✓ "),
            SetForegroundColor(Color::Grey),
            Print(format!("{} ", step_reference.to_string())),
            SetForegroundColor(Color::White),
            Print(msg),
            Print(format!("in {}", format_duration(start.elapsed()))),
            ResetColor,
            Print("\n")
        )
        .unwrap();
    }

    /*if timer {
        println!(
            "{}",
            colors::italic(format!(
                "Finished in {}\n",
                format_duration(start.elapsed())
            ))
        );
    }*/
    if let Some(trace_stmt_id) = trace_stmt_id {
        let mut x = runtime.connection.prepare(r#"
                          INSERT INTO solite_trace.steps (statement_id, addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle) 
                          SELECT ?, addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle
                          FROM bytecode(?)
                          "#).unwrap().1.unwrap();
        x.bind_int64(1, trace_stmt_id);
        x.bind_pointer(2, stmt.pointer().cast(), c"stmt-pointer");
        x.nextx().unwrap();
    }
}

pub(crate) fn run(flags: RunArgs) -> Result<(), ()> {
    let (database, script) = match (flags.database, flags.script) {
        (Some(a), Some(b)) => {
            if a.extension().and_then(OsStr::to_str) == Some("sql") {
                (Some(b), a)
            } else {
                (Some(a), b)
            }
        }
        (Some(input), None) | (None, Some(input)) => {
            let ext = input.extension().and_then(OsStr::to_str);
            match ext {
                Some("sql") | Some("ipynb") => (None, input),
                Some("db") | Some("sqlite") | Some("sqlite3") => {
                    return crate::commands::repl::repl(ReplArgs {
                        database: Some(input),
                    })
                }
                _ => todo!(),
            }
        }
        (None, None) => return crate::commands::repl::repl(ReplArgs { database: None }),
    };

    let mut rt = Runtime::new(database.as_ref().map(|p| p.to_string_lossy().to_string()));
    if flags.trace.is_some() {
        rt.connection
            .execute("attach database ':memory:' as solite_trace;")
            .unwrap();
        rt.connection.execute(r#"CREATE TABLE solite_trace.statements(id integer primary key autoincrement, sql text)"#).unwrap();
        rt.connection
            .execute(
                r#"CREATE TABLE solite_trace.steps(
        id integer primary key autoincrement, 
        statement_id integer references statements(id),
        addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle
        )"#,
            )
            .unwrap();
    }

    for chunk in flags.parameters.chunks(2) {
        rt.define_parameter(chunk[0].clone(), chunk[1].clone())
            .unwrap();
    }

    match script.extension().and_then(OsStr::to_str) {
        Some("sql") => {
            let sql = read_to_string(&script).unwrap();
            rt.enqueue(
                &script.to_string_lossy().to_string(),
                sql.as_str(),
                BlockSource::File(script.into()),
            );
        }
        Some("ipynb") => {
            let nb: Notebook = parse_notebook(&read_to_string(&script).unwrap()).unwrap();
            let cells: Vec<(usize, String)> = match nb {
                Notebook::V4(nb) => nb
                    .cells
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, cell)| match cell {
                        nbformat::v4::Cell::Code { source, .. } => Some((idx, source.join("\n"))),
                        _ => None,
                    })
                    .collect(),
                Notebook::Legacy(nb) => nb
                    .cells
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, cell)| match cell {
                        nbformat::legacy::Cell::Code { source, .. } => {
                            Some((idx, source.join("\n")))
                        }
                        _ => None,
                    })
                    .collect(),
            };
            for (idx, code) in cells {
                rt.enqueue(
                    format!("{}:{}", script.to_string_lossy(), idx).as_str(),
                    code.as_str(),
                    BlockSource::JupyerCell,
                );
            }
        }
        Some(x) => todo!("Unknown file type {x}"),
        None => todo!("No file extension"),
    }

    let mut timer = true;

    loop {
        match rt.next_stepx() {
            None => break,
            Some(Ok(step)) => match step.result {
                StepResult::SqlStatement { stmt, .. } => handle_sql(
                    &mut rt,
                    &stmt,
                    &step.reference.to_string(),
                    flags.trace.is_some(),
                    timer,
                ),
                StepResult::DotCommand(mut cmd) => {
                    handle_dot_command(&mut rt, &mut cmd, &mut timer)
                }
            },
            Some(Err(step_error)) => match step_error {
                StepError::Prepare {
                    error,
                    file_name,
                    src,
                    offset,
                } => {
                    crate::errors::report_error(file_name.as_str(), &src, &error, Some(offset));
                }
                StepError::ParseDot(error) => {
                    eprintln!("parse dot error {error}")
                }
            },
        }
    }
    if let Some(trace) = flags.trace {
        let x = rt
            .connection
            .prepare("vacuum solite_trace into ?;")
            .unwrap()
            .1
            .unwrap();
        x.bind_text(1, trace.to_string_lossy());
        x.nextx().unwrap();
    }

    Ok(())
}
