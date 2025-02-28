mod cli;
mod colors;
mod errors;
mod jupyter;
mod query;
mod repl;
mod run;
mod ui;
mod snapshot;

use std::{env, process::exit};

fn main() {
    let args: Vec<String> = env::args().collect();
    let flags = match cli::flags_from_vec(args) {
        Ok(flags) => flags,
        Err(err @ clap::Error { .. })
            if err.kind() == clap::error::ErrorKind::DisplayHelp
                || err.kind() == clap::error::ErrorKind::DisplayVersion =>
        {
            err.print().unwrap();
            exit(0);
        }
        Err(err) => {
            err.print().unwrap();
            exit(1);
        }
    };
    cli::launch(flags)
}
