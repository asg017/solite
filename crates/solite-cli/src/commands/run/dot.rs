//! Dot command handling for the run command.

use solite_core::dot::sh::ShellResult;
use solite_core::dot::DotCommand;
use solite_core::Runtime;

use crate::colors;

/// Handle a dot command during script execution.
pub fn handle_dot_command(runtime: &mut Runtime, cmd: &mut DotCommand, timer: &mut bool) {
    match cmd {
        DotCommand::Ask(_) => {
            eprintln!("Warning: .ask command not supported in run mode");
        }
        DotCommand::Tui(_) => {
            eprintln!("Warning: .tui command not supported in run mode");
        }
        DotCommand::Clear(_) => {
            eprintln!("Warning: .clear command not supported in run mode");
        }
        DotCommand::Dotenv(cmd) => {
            cmd.execute();
        }
        DotCommand::Tables(cmd) => {
            let tables = cmd.execute(runtime);
            for table in tables {
                println!("{}", table);
            }
        }
        DotCommand::Schema(cmd) => {
            let creates = cmd.execute(runtime);
            for create in creates {
                println!("{}", create);
            }
        }
        DotCommand::Graphviz(cmd) => {
            let graphviz = cmd.execute(runtime);
            println!("{}", graphviz);
        }
        DotCommand::Print(print_cmd) => {
            print_cmd.execute();
        }
        DotCommand::Load(load_cmd) => {
            match load_cmd.execute(&mut runtime.connection) {
                Ok(_) => {
                    println!("{} extension loaded", colors::green("✓"));
                }
                Err(err) => {
                    eprintln!("Error loading extension: {:?}", err);
                }
            }
        }
        DotCommand::Open(open_cmd) => {
            open_cmd.execute(runtime);
        }
        DotCommand::Timer(enabled) => {
            *timer = *enabled;
            println!(
                "{} timer set {}",
                colors::green("✓"),
                if *enabled { "on" } else { "off" }
            );
        }
        DotCommand::Parameter(param_cmd) => {
            handle_parameter_command(runtime, param_cmd);
        }
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
        }
        DotCommand::Export(cmd) => {
            match cmd.execute() {
                Ok(()) => {
                    println!(
                        "{} exported results to {}",
                        colors::green("✓"),
                        cmd.target.display()
                    );
                }
                Err(e) => {
                    eprintln!("Error exporting results to {}: {}", cmd.target.display(), e);
                }
            }
        }
        DotCommand::Shell(shell_command) => {
            match shell_command.execute() {
                ShellResult::Background(child) => {
                    println!("✓ started background process with PID {}", child.id());
                }
                ShellResult::Stream(rx) => {
                    while let Ok(msg) = rx.recv() {
                        println!("{}", msg);
                    }
                }
            }
        }
        DotCommand::Vegalite(_) => {
            eprintln!("Warning: .vegalite command not supported in run mode");
        }
        DotCommand::Bench(_) => {
            eprintln!("Warning: .bench command not supported in run mode");
        }
    }
}

/// Handle parameter subcommands.
fn handle_parameter_command(runtime: &mut Runtime, cmd: &solite_core::dot::ParameterCommand) {
    match cmd {
        solite_core::dot::ParameterCommand::Set { key, value } => {
            match runtime.define_parameter(key.clone(), value.to_owned()) {
                Ok(_) => {
                    println!("{} parameter {} set", colors::green("✓"), key);
                }
                Err(e) => {
                    eprintln!("Error setting parameter {}: {}", key, e);
                }
            }
        }
        solite_core::dot::ParameterCommand::Unset(key) => {
            eprintln!("Warning: .parameter unset {} not yet implemented", key);
        }
        solite_core::dot::ParameterCommand::List => {
            eprintln!("Warning: .parameter list not yet implemented");
        }
        solite_core::dot::ParameterCommand::Clear => {
            eprintln!("Warning: .parameter clear not yet implemented");
        }
    }
}
