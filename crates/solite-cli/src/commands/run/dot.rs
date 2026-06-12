//! Dot command handling for the run command.

use solite_core::dot::sh::ShellResult;
use solite_core::dot::DotCommand;
use solite_core::Runtime;

use crate::colors;

/// Handle a dot command during script execution.
///
/// Returns `false` when the command failed (the error is reported here);
/// unsupported-command warnings still count as success.
pub fn handle_dot_command(runtime: &mut Runtime, cmd: &mut DotCommand, timer: &mut bool) -> bool {
    match cmd {
        DotCommand::Ask(_) => {
            eprintln!("Warning: .ask command not supported in run mode");
            true
        }
        DotCommand::Tui(_) => {
            eprintln!("Warning: .tui command not supported in run mode");
            true
        }
        DotCommand::Clear(_) => {
            eprintln!("Warning: .clear command not supported in run mode");
            true
        }
        DotCommand::Dotenv(cmd) => match cmd.execute() {
            Ok(result) => {
                println!(
                    "{} loaded {} variables from {}",
                    colors::green("✓"),
                    result.loaded.len(),
                    result.path.display()
                );
                true
            }
            Err(e) => {
                eprintln!("Error loading .env file: {}", e);
                false
            }
        },
        DotCommand::Tables(cmd) => match cmd.execute(runtime) {
            Ok(tables) => {
                for table in tables {
                    println!("{}", table);
                }
                true
            }
            Err(e) => {
                eprintln!("Error listing tables: {}", e);
                false
            }
        },
        DotCommand::Schema(cmd) => match cmd.execute(runtime) {
            Ok(creates) => {
                for create in creates {
                    println!("{}", create);
                }
                true
            }
            Err(e) => {
                eprintln!("Error getting schema: {}", e);
                false
            }
        },
        DotCommand::Graphviz(cmd) => match cmd.execute(runtime) {
            Ok(graphviz) => {
                println!("{}", graphviz);
                true
            }
            Err(e) => {
                eprintln!("Error generating graphviz: {}", e);
                false
            }
        },
        DotCommand::Print(print_cmd) => {
            print_cmd.execute();
            true
        }
        DotCommand::Help(help_cmd) => {
            println!("{}", help_cmd.execute());
            true
        }
        DotCommand::Load(load_cmd) => match load_cmd.execute(&mut runtime.connection) {
            Ok(_) => {
                println!("{} extension loaded", colors::green("✓"));
                true
            }
            Err(err) => {
                eprintln!("Error loading extension: {:?}", err);
                false
            }
        },
        DotCommand::Open(open_cmd) => match open_cmd.execute(runtime) {
            Ok(()) => {
                println!("{} opened database", colors::green("✓"));
                true
            }
            Err(e) => {
                eprintln!("Error opening database: {}", e);
                false
            }
        },
        DotCommand::Timer(enabled) => {
            *timer = *enabled;
            println!(
                "{} timer set {}",
                colors::green("✓"),
                if *enabled { "on" } else { "off" }
            );
            true
        }
        DotCommand::Parameter(param_cmd) => handle_parameter_command(runtime, param_cmd),
        DotCommand::Env(env_cmd) => {
            let action = env_cmd.execute();
            match action {
                solite_core::dot::EnvAction::Set { name, .. } => {
                    println!("{} environment variable {} set", colors::green("✓"), name);
                }
                solite_core::dot::EnvAction::Unset { name } => {
                    println!(
                        "{} environment variable {} unset",
                        colors::green("✓"),
                        name
                    );
                }
            }
            true
        }
        DotCommand::Export(cmd) => match cmd.execute() {
            Ok(()) => {
                println!(
                    "{} exported results to {}",
                    colors::green("✓"),
                    cmd.target.display()
                );
                true
            }
            Err(e) => {
                eprintln!("Error exporting results to {}: {}", cmd.target.display(), e);
                false
            }
        },
        DotCommand::Shell(shell_command) => match shell_command.execute() {
            Ok(ShellResult::Background(child)) => {
                println!("✓ started background process with PID {}", child.id());
                true
            }
            Ok(ShellResult::Stream(rx)) => {
                while let Ok(msg) = rx.recv() {
                    println!("{}", msg);
                }
                true
            }
            Err(e) => {
                eprintln!("Error executing shell command: {}", e);
                false
            }
        },
        DotCommand::Vegalite(cmd) => match cmd.execute() {
            Ok(spec) => match crate::commands::write_vegalite_spec(&spec) {
                Ok(path) => {
                    println!(
                        "{} wrote Vega-Lite spec to {}",
                        colors::green("✓"),
                        path.display()
                    );
                    true
                }
                Err(e) => {
                    eprintln!("Error writing Vega-Lite spec: {}", e);
                    false
                }
            },
            Err(e) => {
                eprintln!("Error executing vegalite command: {}", e);
                false
            }
        },
        DotCommand::Bench(cmd) => match cmd.execute(None) {
            Ok(result) => {
                println!("{}", result.report());
                if !result.report.is_empty() {
                    println!("{}", result.report);
                }
                true
            }
            Err(e) => {
                eprintln!("Error running benchmark: {}", e);
                false
            }
        },
        #[cfg(feature = "ritestream")]
        DotCommand::Stream(stream_cmd) => match stream_cmd.execute(runtime) {
            Ok(Some(result)) => {
                println!(
                    "{} synced (txid={}, {} pages)",
                    colors::green("✓"),
                    result.txid,
                    result.page_count
                );
                true
            }
            Ok(None) => {
                println!("{} stream command completed", colors::green("✓"));
                true
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                false
            }
        },
        DotCommand::Call(_) => {
            /* resolved to SqlStatement in next_stepx() */
            true
        }
        DotCommand::Run(run_cmd) => {
            if let Some(ref proc_name) = run_cmd.procedure {
                // Procedure mode: load file, then call the procedure with
                // --key=val parameters scoped to this invocation. Parameters
                // are defined only after the file loads and the procedure
                // resolves, so a failed .run leaves nothing behind.
                if let Err(e) = runtime.load_file(&run_cmd.file) {
                    eprintln!("Error loading file '{}': {}", run_cmd.file, e);
                    return false;
                }
                let proc = match runtime.get_procedure(proc_name) {
                    Some(p) => p.clone(),
                    None => {
                        eprintln!("Unknown procedure: '{}'", proc_name);
                        return false;
                    }
                };
                let saved = match runtime.save_and_define_parameters(&run_cmd.parameters) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error setting parameters: {}", e);
                        return false;
                    }
                };
                let success = match runtime.prepare_with_parameters(&proc.sql) {
                    Ok((_, Some(mut stmt))) => {
                        super::sql::handle_sql(runtime, &mut stmt, &run_cmd.file, false, *timer)
                    }
                    Ok((_, None)) => {
                        eprintln!("Procedure '{}' prepared to empty statement", proc_name);
                        false
                    }
                    Err(e) => {
                        eprintln!("Error preparing procedure '{}': {:?}", proc_name, e);
                        false
                    }
                };
                runtime.restore_parameters(saved);
                success
            } else {
                // File mode: run_file_begin, step loop, run_file_end.
                // run_file_begin saved and cleared the stack, so draining
                // execute_steps to completion runs exactly the .run file.
                let saved = match runtime.run_file_begin(&run_cmd.file, &run_cmd.parameters) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        return false;
                    }
                };
                let failures = super::execute_steps(runtime, false, timer);
                runtime.run_file_end(saved);
                failures == 0
            }
        }
    }
}

/// Handle parameter subcommands.
///
/// Returns `false` when the subcommand failed.
fn handle_parameter_command(runtime: &mut Runtime, cmd: &solite_core::dot::ParameterCommand) -> bool {
    match cmd {
        solite_core::dot::ParameterCommand::Set { key, value } => {
            // Same integer/real inference as the CLI `-p` flag and the REPL
            let value = solite_core::infer_parameter_value(value);
            match runtime.define_parameter_value(key.clone(), value) {
                Ok(_) => {
                    println!("{} parameter {} set", colors::green("✓"), key);
                    true
                }
                Err(e) => {
                    eprintln!("Error setting parameter {}: {}", key, e);
                    false
                }
            }
        }
        solite_core::dot::ParameterCommand::Unset(key) => {
            runtime.delete_parameter(key);
            println!("{} parameter {} unset", colors::green("✓"), key);
            true
        }
        solite_core::dot::ParameterCommand::List => {
            match solite_core::dot::param::list_parameters_statement(runtime) {
                Some(mut stmt) => {
                    let config = solite_table::TableConfig::terminal();
                    if let Err(e) = solite_table::print_statement(&mut stmt, &config) {
                        eprintln!("Error listing parameters: {}", e);
                        return false;
                    }
                    true
                }
                None => {
                    println!("No parameters set");
                    true
                }
            }
        }
        solite_core::dot::ParameterCommand::Clear => {
            let cleared = solite_core::dot::param::clear_parameters(runtime);
            println!("{} cleared {} parameter(s)", colors::green("✓"), cleared);
            true
        }
    }
}
