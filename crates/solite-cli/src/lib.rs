//! Library target for solite-cli.
//!
//! Exists so the criterion benches (`benches/tui.rs`) can link against the
//! crate internals; the real interface is the `solite` binary
//! (`src/main.rs`), which just calls [`run_main`].

mod cli;
mod colors;
mod commands;
mod errors;
mod themes;

use std::{env, path::PathBuf, process::exit};

use clap::Parser;
use cli::ReplArgs;

/// Hidden re-exports for the criterion benches in `benches/tui.rs`.
/// Not a public API; do not depend on this outside the benches.
#[doc(hidden)]
pub use commands::tui::bench_support as tui_bench_support;

/// The `solite` binary's entire main function.
pub fn run_main() {
    // Shell completion hook. When invoked by a shell's completion integration
    // the `COMPLETE` env var is set: this generates candidates and exits the
    // process. On a normal run `COMPLETE` is unset, so this is a no-op and
    // returns. It MUST run before the bespoke `try_parse_from` fallback below,
    // which would otherwise misinterpret the unusual argv a completion request
    // passes (and trip the bare-REPL / `solite <file>.db` paths).
    clap_complete::CompleteEnv::with_factory(cli::command_for_completion).complete();

    let args: Vec<String> = env::args().collect();

    let (allow_ssh, x) = match cli::Cli::try_parse_from(&args) {
        Ok(cli) => (cli.allow_ssh, cli.command),
        Err(err) => match err.kind() {
            clap::error::ErrorKind::MissingSubcommand => {
                (false, Box::new(cli::Commands::Repl(ReplArgs { database: None, remote: Default::default() })))
            }
            clap::error::ErrorKind::InvalidSubcommand => {
              // if the "invalid subcommand" is actually a path to a database file,
              // then fire up the REPL
                if let Some(path) = args
                    .get(1)
                    .map(PathBuf::from)
                    .filter(|p: &PathBuf| cli::is_database_path(p))
                {
                    (false, Box::new(cli::Commands::Repl(ReplArgs {
                        database: Some(path),
                        remote: Default::default(),
                    })))
                } else {
                    err.exit();
                }
            }
            _ => err.exit(),
        },
    };
    let mut x = x;
    // Propagate top-level --allow-ssh into command RemoteArgs
    match x.as_mut() {
        cli::Commands::Repl(a) => a.remote.allow_ssh = allow_ssh,
        cli::Commands::Query(a) => a.remote.allow_ssh = allow_ssh,
        cli::Commands::Tui(a) => a.remote.allow_ssh = allow_ssh,
        _ => {}
    }
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
        cli::Commands::Dbhash(args) => commands::dbhash::dbhash(args.args),
        cli::Commands::Dbtotxt(args) => commands::dbtotxt::dbtotxt(args.args),
        cli::Commands::Expert(args) => commands::expert::expert(args.args),
        cli::Commands::Schema(args) => {
            commands::schema::schema(args.database, args.pattern, args.format, allow_ssh)
        }
        cli::Commands::Backup(args) => commands::backup::backup(args),
        cli::Commands::Vacuum(args) => commands::vacuum::vacuum(args),
        cli::Commands::Serve(args) => commands::serve::serve(args),
        cli::Commands::Completions(args) => commands::completions::completions(args),
        #[cfg(feature = "ritestream")]
        cli::Commands::Stream(cmd) => commands::stream::stream(cmd),
    };
    // Commands print their own diagnostics before returning Err(());
    // main only translates the result into an exit code.
    match result {
        Ok(()) => exit(0),
        Err(()) => exit(1),
    }
}
