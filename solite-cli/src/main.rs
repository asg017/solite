mod bench;
mod cli;
mod colors;
mod docs;
mod errors;
mod jupyter;
mod query;
mod repl;
mod run;
mod snapshot;
mod ui;

use std::{env, process::exit};

use clap::Parser;
use cli::ReplArgs;

fn main() {
    let args: Vec<String> = env::args().collect();

    let x = match cli::Cli::try_parse_from(args) {
        Ok(cli) => cli.command,
        Err(err) => match err.kind() {
            clap::error::ErrorKind::MissingSubcommand => {
                Box::new(cli::Commands::Repl(ReplArgs { database: None }))
            }
            _ => {
                err.print().unwrap();
                exit(1);
            }
        },
    };
    let result = match *x {
        cli::Commands::Run(args) => crate::run::run(args),
        cli::Commands::Query(args) => crate::query::query(args, false),
        cli::Commands::Execute(args) => todo!(),
        cli::Commands::Repl(args) => crate::repl::repl(args),
        cli::Commands::Snap(cmd) => crate::snapshot::snapshot(cmd),
        cli::Commands::Jupyter(cmd) => crate::jupyter::jupyter(cmd),
        cli::Commands::Docs(cmd) => crate::docs::docs(cmd),
        cli::Commands::Bench(args) => crate::bench::bench(args),
    };
    match result {
        Ok(_) => exit(0),
        Err(err) => {
            //eprintln!("{}", err);
            exit(1);
        }
    }
}
