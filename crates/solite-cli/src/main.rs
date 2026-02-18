mod cli;
mod colors;
mod commands;
mod errors;
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
                    .filter(|p: &PathBuf| p.extension().is_some_and(|ext| ext == "db"))
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
        cli::Commands::Query(args) => commands::query::query(args),
        cli::Commands::Execute(args) => commands::exec::exec(args),
        cli::Commands::Repl(args) => commands::repl::repl(args),
        cli::Commands::Test(cmd) => commands::test::test(cmd),
        cli::Commands::Jupyter(cmd) => commands::jupyter::jupyter(cmd),
        cli::Commands::Docs(cmd) => commands::docs::docs(cmd),
        cli::Commands::Bench(args) => commands::bench::bench(args),
        cli::Commands::Codegen(cmd) => commands::codegen::codegen(cmd),
        cli::Commands::Tui(cmd) => commands::tui::tui(cmd),
        cli::Commands::Format(args) => commands::fmt::fmt(args),
        cli::Commands::Lint(args) => commands::lint::lint(args),
        cli::Commands::Lsp(args) => commands::lsp::lsp(args),
        cli::Commands::Sqlite3(args) => commands::sqlite3::sqlite3(args.args),
        cli::Commands::Diff(args) => commands::diff::diff(args.args),
        cli::Commands::Rsync(args) => commands::rsync::rsync(args.args),
        cli::Commands::Schema(args) => commands::schema::schema(args.database),
    };
    match result {
        Ok(_) => exit(0),
        Err(_err) => {
            //eprintln!("{}", _err);
            exit(1);
        }
    }
}
