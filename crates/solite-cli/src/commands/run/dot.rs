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
        DotCommand::Dotenv(cmd) => match cmd.execute() {
            Ok(result) => {
                println!(
                    "{} loaded {} variables from {}",
                    colors::green("✓"),
                    result.loaded.len(),
                    result.path.display()
                );
            }
            Err(e) => {
                eprintln!("Error loading .env file: {}", e);
            }
        },
        DotCommand::Tables(cmd) => match cmd.execute(runtime) {
            Ok(tables) => {
                for table in tables {
                    println!("{}", table);
                }
            }
            Err(e) => {
                eprintln!("Error listing tables: {}", e);
            }
        },
        DotCommand::Schema(cmd) => match cmd.execute(runtime) {
            Ok(creates) => {
                for create in creates {
                    println!("{}", create);
                }
            }
            Err(e) => {
                eprintln!("Error getting schema: {}", e);
            }
        },
        DotCommand::Graphviz(cmd) => match cmd.execute(runtime) {
            Ok(graphviz) => {
                println!("{}", graphviz);
            }
            Err(e) => {
                eprintln!("Error generating graphviz: {}", e);
            }
        },
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
        DotCommand::Open(open_cmd) => match open_cmd.execute(runtime) {
            Ok(()) => {
                println!("{} opened database", colors::green("✓"));
            }
            Err(e) => {
                eprintln!("Error opening database: {}", e);
            }
        },
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
        DotCommand::Shell(shell_command) => match shell_command.execute() {
            Ok(ShellResult::Background(child)) => {
                println!("✓ started background process with PID {}", child.id());
            }
            Ok(ShellResult::Stream(rx)) => {
                while let Ok(msg) = rx.recv() {
                    println!("{}", msg);
                }
            }
            Err(e) => {
                eprintln!("Error executing shell command: {}", e);
            }
        },
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
