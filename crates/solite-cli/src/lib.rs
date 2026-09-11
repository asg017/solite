//! Library target for solite-cli.
//!
//! Exists so the criterion benches (`benches/tui.rs`) can link against the
//! crate internals; the real interface is the `solite` binary
//! (`src/main.rs`), which just calls [`run_main`].

mod cli;
mod colors;
mod commands;
mod errors;
mod sql_tokens;

use std::{env, path::PathBuf, process::exit};

use clap::{CommandFactory, FromArgMatches};
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

    // Pre-scan argv for `--color` so clap's own help/usage/error styling
    // honors it too, before the full (authoritative) parse below resolves
    // `cli.color` and calls `colors::init()`.
    let pre_scanned_color = colors::scan_color_flag(&args);
    let command = cli::Cli::command().color(pre_scanned_color);

    // Mirrors what the derived `Cli::try_parse_from` does internally (match
    // raw args, then build the `Cli` struct from the resulting matches) as a
    // single combined `Result`, so the fallback-to-REPL error handling below
    // (which must catch failures from *either* stage: a missing/invalid
    // subcommand is a "no subcommand matched" `ArgMatches` in some cases and
    // a `from_arg_matches` construction error in others) stays exactly as it
    // was before splitting the two stages to apply `.color()` first.
    let cli_result: Result<cli::Cli, clap::Error> = command
        .try_get_matches_from(&args)
        .and_then(|matches| cli::Cli::from_arg_matches(&matches));

    let (allow_ssh, x) = match cli_result {
        Ok(cli) => {
            colors::init(cli.color);
            (cli.allow_ssh, cli.command)
        }
        Err(err) => match err.kind() {
            clap::error::ErrorKind::MissingSubcommand => {
                colors::init(pre_scanned_color);
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
                    colors::init(pre_scanned_color);
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
        cli::Commands::Docgen(args) => commands::docgen::docgen(args),
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
