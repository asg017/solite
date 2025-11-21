mod cli;
mod colors;
mod commands;
mod errors;
mod ui;
mod themes;
use std::{env, path::PathBuf, process::exit};

use clap::Parser;
use cli::ReplArgs;

fn main() {
    let args: Vec<String> = env::args().collect();

    let x = match cli::Cli::try_parse_from(&args) {
        Ok(cli) => cli.command,
        Err(err) => match err.kind() {
            clap::error::ErrorKind::MissingSubcommand => {
                Box::new(cli::Commands::Repl(ReplArgs { database: None }))
            }
            clap::error::ErrorKind::InvalidSubcommand => {
              // if the "invalid subcommand" is actually a path to a database file,
              // then fire up the REPL
                if let Some(path) = args
                    .get(1)
                    .map(PathBuf::from)
                    .filter(|p: &PathBuf| p.extension().map_or(false, |ext| ext == "db"))
                {
                    Box::new(cli::Commands::Repl(ReplArgs {
                        database: Some(path),
                    }))
                } else {
                    err.print().unwrap();
                    exit(1);
                }
            }
            _ => {
                err.print().unwrap();
                exit(1);
            }
        },
    };
    let result = match *x {
        cli::Commands::Run(args) => commands::run::run(args),
        cli::Commands::Query(args) => commands::query::query(args, false),
        cli::Commands::Execute(_args) => todo!(),
        cli::Commands::Repl(args) => commands::repl::repl(args),
        cli::Commands::Snap(cmd) => commands::snapshot::snapshot(cmd),
        cli::Commands::Jupyter(cmd) => commands::jupyter::jupyter(cmd),
        cli::Commands::Docs(cmd) => commands::docs::docs(cmd),
        cli::Commands::Bench(args) => commands::bench::bench(args),
        cli::Commands::Mcp(args) => commands::mcp::mcp(args),
        cli::Commands::Codegen(cmd) => commands::codegen::codegen(cmd),
        cli::Commands::Tui(cmd) => commands::tui::tui(cmd),
        cli::Commands::Rpc(cmd) => commands::rpc::rpc(cmd),
    };
    match result {
        Ok(_) => exit(0),
        Err(err) => {
            //eprintln!("{}", err);
            exit(1);
        }
    }
}
