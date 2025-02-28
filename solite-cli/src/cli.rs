use clap::error::Result;
use clap::{Arg, ArgAction, ArgMatches, Command, ValueHint};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::exit;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SoliteSubcommand {
    Run(RunFlags),
    Snapshot(SnapshotFlags),
    Query(QueryFlags),
    Help(HelpFlags),
    Repl(ReplFlags),
    Jupyter(JupyterFlags),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flags {
    pub subcommand: SoliteSubcommand,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JupyterFlags {
    pub install: bool,
    pub connection: Option<String>,
}

pub fn jupyter_subcommand() -> Command {
    Command::new("jupyter")
        .arg(
            Arg::new("install")
                .long("install")
                .action(ArgAction::SetTrue)
                .help("Install the Solite Jupyter kernel"),
        )
        .arg(
            Arg::new("connection")
                .long("connection")
                .value_name("CONNECTION")
                .required(false)
                .help("Connection file, supplied by Jupyter"),
        )
        .about("Jupyter")
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunFlags {
    pub database: Option<String>,
    pub script: String,
    pub verbose: bool,
}

pub fn run_subcommand() -> Command {
    Command::new("run")
        .allow_missing_positional(true)
        .arg(
            Arg::new("database")
                .help("Path to SQLite database")
                .value_name("DATABASE")
                .value_hint(ValueHint::FilePath)
                .required(false),
        )
        .arg(
            Arg::new("script")
                .help("Path to SQL or PRQL file")
                .value_name("SCRIPT")
                .value_hint(ValueHint::FilePath)
                .required(true),
        )
        .arg(Arg::new("verbose").long("verbose").required(false))
        .about("Run SQL or PRQL scripts ")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotFlags {
    pub script: String,
    pub extension: Option<String>,
}
pub fn snapshot_subcommand() -> Command {
  Command::new("snapshot")
  .allow_missing_positional(true)
      .arg(
          Arg::new("script")
              .help("Path to SQL file")
              .value_name("SCRIPT")
              .value_hint(ValueHint::FilePath)
              .required(true),
      )
      
      .arg(
          Arg::new("extension")
              .help("Path to SQLite extension")
              .value_name("EXT")
              .required(false)
      )
      
      .about("Snapshot results of SQL commands")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReplFlags {
    pub database: Option<String>,
    // --read-only/--ro
    // --safe
}

pub fn repl_subcommand() -> Command {
    Command::new("repl")
        .arg(
            Arg::new("database")
                .help("Path to SQLite database")
                .value_name("DATABASE")
                .value_hint(ValueHint::FilePath)
                .required(false),
        )
        .about("reh pull")
}

// solite query "select q1" "select s1"

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum QueryFormat {
    Csv,
    Tsv,
    Json,
    Ndjson,
    Value,
    Clipboard,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueryFlags {
    pub database: Option<String>,
    pub statement: String,
    pub parameters: HashMap<String, String>,
    /// -f json, -f csv, -f value, etc.
    pub format: Option<QueryFormat>,
    /// output file location.
    pub output: Option<PathBuf>,
    // TODO:
    // --pb (output to clipboard)
    // all | row | value
    // output to file
}

pub fn query_subcommand() -> Command {
    Command::new("query")
        .allow_missing_positional(true)
        .alias("q")
        .arg(
            Arg::new("database")
                .help("Path to SQLite database")
                .value_name("DATABASE")
                .value_hint(ValueHint::FilePath)
                .required(false),
        )
        .arg(
            Arg::new("statement")
                .help("statement")
                .value_name("SQL STATEMENT")
                .num_args(1)
                .required(true),
        )
        .arg(
            Arg::new("parameters")
                .help("parameter")
                .short('p')
                .value_name("PARAMETERS")
                .required(false)
                .num_args(2)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("format")
                .help("Output format")
                .short('f')
                .value_name("FORMAT")
                .num_args(1)
                .required(false),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .help("Output file")
                .value_name("PATH")
                .value_hint(ValueHint::FilePath)
                .num_args(1)
                .required(false),
        )
        .about("execute sql")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HelpFlags {}

fn parameters_from_matches(m: &ArgMatches) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    let x: Vec<_> = match m.get_many::<String>("parameters") {
        Some(x) => x.collect(),
        None => return map,
    };
    for pair in x.chunks(2) {
        let k = pair.get(0).unwrap();
        let v = pair.get(1).unwrap();
        map.insert(k.to_string(), v.to_string());
    }
    map
}

pub fn flags_from_vec(args: Vec<String>) -> Result<Flags> {
    let app = clap::Command::new("solite")
        .bin_name("solite")
        .max_term_width(80)
        .allow_external_subcommands(true)
        .subcommand(run_subcommand())
        .subcommand(snapshot_subcommand())
        .subcommand(query_subcommand())
        .subcommand(repl_subcommand())
        .subcommand(jupyter_subcommand());

    let matches = app.get_matches_from(args);
    let subcommand = match matches.subcommand() {
        Some(("run", m)) => SoliteSubcommand::Run(RunFlags {
            database: m.get_one::<String>("database").cloned(),
            script: m.get_one::<String>("script").unwrap().to_string(),
            verbose: *m.get_one::<bool>("verbose").unwrap_or(&false),
        }),
        Some(("snapshot", m)) => SoliteSubcommand::Snapshot(SnapshotFlags {
            script: m.get_one::<String>("script").unwrap().to_string(),
            extension: m.get_one::<String>("extension").cloned().map(String::from),
        }),
        Some(("query", m)) => {
            let output = m.get_one::<String>("output").cloned().map(PathBuf::from);
            SoliteSubcommand::Query(QueryFlags {
                database: m.get_one::<String>("database").cloned(),
                statement: m.get_one::<String>("statement").unwrap().to_string(),
                parameters: parameters_from_matches(m),
                format: match m.get_one::<String>("format") {
                    Some(format) => match format.to_lowercase().as_str() {
                        "csv" => Some(QueryFormat::Csv),
                        "json" => Some(QueryFormat::Json),
                        "ndjson" | "jsonl" => Some(QueryFormat::Ndjson),
                        "value" => Some(QueryFormat::Value),
                        "clipboard" | "copy" => Some(QueryFormat::Clipboard),
                        _ => todo!("unknown format"),
                    },
                    None => None,
                },
                output,
            })
        }
        Some(("repl", m)) => SoliteSubcommand::Repl(ReplFlags {
            database: m.get_one::<String>("database").cloned(),
        }),
        Some(("jupyter", m)) => SoliteSubcommand::Jupyter(JupyterFlags {
            install: m.get_flag("install"),
            connection: m.get_one::<String>("connection").cloned(),
        }),
        Some((name, _m)) => {
            if name.ends_with(".db") || name.ends_with(".sqlite3") || name.ends_with(".sqlite") {
                SoliteSubcommand::Repl(ReplFlags {
                    // TODO maybe only "unrecognized subcommands" that end in
                    // .db/.sqlite/.sqlite3 should launch a repl?
                    database: Some(name.to_string()),
                })
            } else {
                todo!("unrecognized command")
            }
        }

        None => SoliteSubcommand::Repl(ReplFlags { database: None }),
    };
    let flags = Flags { subcommand };

    Ok(flags)
}

pub(crate) fn launch(flags: Flags) {
    match flags.subcommand.clone() {
        SoliteSubcommand::Run(run_flags) => match crate::run::run(run_flags) {
            Ok(()) => exit(0),
            Err(()) => exit(1),
        },
        SoliteSubcommand::Snapshot(snapshot_flags) => match crate::snapshot::snapshot(snapshot_flags) {
            Ok(()) => exit(0),
            Err(()) => exit(1),
        },
        SoliteSubcommand::Query(query_flags) => match crate::query::query(query_flags) {
            Ok(()) => exit(0),
            Err(()) => exit(1),
        },
        SoliteSubcommand::Repl(repl_flags) => match repl(repl_flags) {
            Ok(()) => exit(0),
            Err(()) => exit(1),
        },
        SoliteSubcommand::Jupyter(flags) => match crate::jupyter::cli_jupyter(flags) {
            Ok(()) => exit(0),
            Err(()) => exit(1),
        },
        SoliteSubcommand::Help(_help_flags) => {
            //println!("{:?}", help_flags);
            todo!("Help???");
        }
    }
}

fn repl(flags: ReplFlags) -> Result<(), ()> {
    crate::repl::repl(flags).map_err(|_| ())
}
#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! svec {
    ($($x:expr),* $(,)?) => (vec![$($x.to_string()),*]);
  }
    #[test]
    fn normal_run() {
        assert_eq!(
            flags_from_vec(svec!["solite", "run", "file.sql"]).unwrap(),
            Flags {
                subcommand: SoliteSubcommand::Run(RunFlags {
                    database: None,
                    script: "file.sql".to_string(),
                    verbose: false,
                }),
            }
        );
        assert_eq!(
            flags_from_vec(svec!["solite", "run", "a.db", "file.sql"]).unwrap(),
            Flags {
                subcommand: SoliteSubcommand::Run(RunFlags {
                    database: Some("a.db".to_owned()),
                    script: "file.sql".to_string(),
                    verbose: false,
                }),
            }
        );
        assert_eq!(
            flags_from_vec(svec!["solite"]).unwrap(),
            Flags {
                subcommand: SoliteSubcommand::Repl(ReplFlags { database: None }),
            }
        );
        assert_eq!(
            flags_from_vec(svec!["solite", "repl"]).unwrap(),
            Flags {
                subcommand: SoliteSubcommand::Repl(ReplFlags { database: None }),
            }
        );
        // TODO
        //assert_eq!(
        //    flags_from_vec(svec!["solite", "a.db"]).unwrap(),
        //    Flags {
        //        subcommand: SoliteSubcommand::Repl(ReplFlags {
        //            db: Some("a.db".to_owned())
        //        }),
        //    }
        //);
        assert_eq!(
            flags_from_vec(svec!["solite", "repl", "a.db"]).unwrap(),
            Flags {
                subcommand: SoliteSubcommand::Repl(ReplFlags {
                    database: Some("a.db".to_owned())
                }),
            }
        );
    }
}
