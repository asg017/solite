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
            Some(Ok(step)) => {
                match step.result {
                    StepResult::SqlStatement { stmt, .. } => {
                        /*
                        println!(
                            "{} {}",
                            colors::green(step.reference.to_string()),
                            colors::gray(stmt.sql().trim())
                        );*/

                        /*
                         execute!(
                             stdout(),
                             SetForegroundColor(Color::Green),
                             Print(step.reference.to_string()),
                             Print(" "),
                             SetForegroundColor(Color::Blue),
                             Print(stmt.sql().replace("\n", " ").trim()),
                             //Print("\n"),
                             ResetColor
                         )
                         .unwrap();
                        */

                        let pb = indicatif::ProgressBar::new_spinner();
                        //pb.enable_steady_tick(Duration::from_millis(200));
                        pb.set_style(
                            indicatif::ProgressStyle::with_template(
                                "{spinner:.cyan} {elapsed} {wide_msg}",
                            )
                            .unwrap()
                            .tick_chars("⣾⣷⣯⣟⡿⢿⣻⣽"),
                        );

                        let trace_stmt_id = if flags.trace.is_some() {
                            let x = rt.connection.prepare("INSERT INTO solite_trace.statements (sql) VALUES (?) RETURNING id").unwrap().1.unwrap();
                            x.bind_text(1, stmt.sql());
                            let id = x.nextx().unwrap().unwrap().value_at(0).as_int64();
                            Some(id)
                        } else {
                            None
                        };

                        let mut x = 4;
                        //let xx = Rc::new(stmt);
                        //let safe_ptr = UnsafeSendPtr(stmt_ptr);
                        let preamble2 = stmt.sql();
                        let preamble2 = preamble2.replace("\n", " ");
                        let p = stmt.pointer();
                        let start = jiff::Timestamp::now();
                        let r = step.reference.to_string();
                        let pbx = pb.clone();
                        rt.connection.set_progress_handler(
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
                                ///std::io::Write::flush(&mut std::io::stdout()).unwrap();
                                let duration = Timestamp::now() - *start;
                                let round = SpanRound::new();
                                round.largest(jiff::Unit::Millisecond);
                                //duration.round(round);
                                let mut printer = SpanPrinter::new()
                                    .hours_minutes_seconds(true)
                                    .fractional(Some(FractionalUnit::Second));
                                //printer;
                                let mut buf = String::new();
                                printer.print_span(&duration, &mut buf).unwrap();
                                pbx.clone()
                                    .with_elapsed(Duration::from_millis(
                                        duration.total(jiff::Unit::Millisecond).unwrap() as u64,
                                    ))
                                    .with_message(format!("{r} {msg}"))
                                    .tick();
                                /*
                                  execute!(
                                      stdout(),
                                      cursor::MoveToColumn(0),
                                      Clear(ClearType::CurrentLine),
                                      Print(format!("◯ {r} ")),
                                      Print(format!("{buf} ")),
                                      SetForegroundColor(Color::Grey),
                                      Print(r.clone()),
                                      Print(" "),
                                      SetForegroundColor(Color::Blue),
                                      Print(&preamble2[0..10]),
                                      SetForegroundColor(Color::White),
                                      Print(msg),
                                      ResetColor
                                  )
                                  .unwrap();
                                */
                                false
                            }),
                            (p, start),
                        );

                        let start = std::time::Instant::now();
                        let table = crate::ui::table_from_statement(&stmt, true);
                        pb.finish_and_clear();

                        /*execute!(
                            stdout(),
                            cursor::MoveToColumn(0),
                            Clear(ClearType::CurrentLine),
                        )
                        .unwrap();*/
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
                                Print(format!("{} ", step.reference.to_string())),
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
                            let mut x = rt.connection.prepare(r#"
                          INSERT INTO solite_trace.steps (statement_id, addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle) 
                          SELECT ?, addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle
                          FROM bytecode(?)
                          "#).unwrap().1.unwrap();
                            x.bind_int64(1, trace_stmt_id);
                            x.bind_pointer(2, stmt.pointer().cast(), c"stmt-pointer");
                            x.nextx().unwrap();
                        }
                    }
                    StepResult::DotCommand(cmd) => match cmd {
                        DotCommand::Ask(cmd) => todo!(),
                        DotCommand::Tables(cmd) => {
                            let tables = cmd.execute(&rt);
                            for table in tables {
                                println!("{table}");
                            }
                        }
                        DotCommand::Print(print_cmd) => print_cmd.execute(),
                        DotCommand::Load(load_cmd) => match load_cmd.execute(&mut rt.connection) {
                            Ok(_) => {
                                println!("{} extension loaded", colors::green("✓"));
                            }
                            Err(err) => {
                                eprintln!("Error loading extension: {err}");
                            }
                        },
                        DotCommand::Open(open_cmd) => open_cmd.execute(&mut rt),
                        DotCommand::Timer(enabled) => {
                            timer = enabled;
                            println!(
                                "{} timer set {}",
                                colors::green("✓"),
                                if enabled { "on" } else { "off" }
                            );
                        }
                        DotCommand::Parameter(param_cmd) => match param_cmd {
                            solite_core::dot::ParameterCommand::Set { key, value } => {
                                rt.define_parameter(key.clone(), value).unwrap();
                                println!("{} parameter {} set", colors::green("✓"), key);
                            }
                            solite_core::dot::ParameterCommand::Unset(_) => todo!(),
                            solite_core::dot::ParameterCommand::List => todo!(),
                            solite_core::dot::ParameterCommand::Clear => todo!(),
                        },
                        DotCommand::Export(mut cmd) => match cmd.execute() {
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
                        DotCommand::Shell(shell_command) => {
                          let rx = shell_command.execute();
                          while let Ok(msg) = rx.recv() {
                              println!("{}", msg);
                          }
                        },
                        DotCommand::Vegalite(vega_lite_command) => todo!(),
                        DotCommand::Bench(cmd) => {
                            todo!();
                        }
                    },
                }
            }
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
