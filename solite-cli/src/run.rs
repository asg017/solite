use crate::cli::RunFlags;
use crate::colors;
use crate::jupyter::notebook::{Cell, RawNotebook, SourceValue};
use cli_table::print_stdout;
use solite_core::dot::DotCommand;
use solite_core::{BlockSource, Runtime, StepError, StepResult};
use std::ffi::OsStr;
use std::fs::read_to_string;
use std::path::Path;
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

pub(crate) fn run(flags: RunFlags) -> Result<(), ()> {
    let mut rt = Runtime::new(flags.database);
    let script = Path::new(flags.script.as_str());
    match script.extension().and_then(OsStr::to_str) {
        Some("sql") => {
            let sql = read_to_string(script).unwrap();
            rt.enqueue(
                flags.script.as_str(),
                sql.as_str(),
                BlockSource::File(script.into()),
            );
        }
        Some("ipynb") => {
            let nb: RawNotebook = serde_json::from_str(&read_to_string(script).unwrap()).unwrap();
            let xxx = nb
                .cells
                .into_iter()
                .enumerate()
                .filter_map(|(idx, cell)| match cell {
                    Cell::Code(c) => Some((idx, c)),
                    _ => None,
                })
                .rev();
            for (cell_idx, code_cell) in xxx {
                match code_cell.source {
                    SourceValue::String(code) => {
                        rt.enqueue(
                            format!("{}:{}", flags.script, cell_idx).as_str(),
                            code.as_str(),
                            BlockSource::JupyerCell,
                        );
                    }
                    SourceValue::StringArray(strings) => {
                        let code = strings.join("\n");
                        rt.enqueue(
                            format!("{}:{}", flags.script, cell_idx).as_str(),
                            code.as_str(),
                            BlockSource::JupyerCell,
                        );
                    }
                }
            }
        }
        Some(_) | None => todo!(),
    }

    let mut timer = true;

    loop {
        match rt.next_step() {
            Ok(Some(step)) => match step.result {
                StepResult::SqlStatement(stmt) => {
                    println!(
                        "{} {}",
                        colors::green(step.source),
                        colors::italic_gray(stmt.sql().trim())
                    );
                    let start = std::time::Instant::now();
                    if let Some(table) = crate::ui::table_from_statement(stmt, true) {
                        print_stdout(table).unwrap()
                    }
                    if timer {
                        println!(
                            "{}",
                            colors::italic(format!(
                                "Finished in {}\n",
                                format_duration(start.elapsed())
                            ))
                        );
                    }
                }
                StepResult::DotCommand(cmd) => match cmd {
                    DotCommand::Tables(cmd) => cmd.execute(&rt),
                    DotCommand::Print(print_cmd) => print_cmd.execute(),
                    DotCommand::Load(load_cmd) => load_cmd.execute(&mut rt.connection),
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
                },
            },
            Ok(None) => break,
            Err(error) => match error {
                StepError::Prepare { error: _, context } => {
                    eprintln!("{context}");
                }
                StepError::ParseDot(error) => {
                    eprintln!("parse dot error {error}")
                }
            },
        }
    }

    Ok(())
}
