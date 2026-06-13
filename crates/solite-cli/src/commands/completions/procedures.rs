//! Procedure-name completion for `solite run <db> queries.sql <TAB>`.
//!
//! Procedures are declared with `-- name: <proc> :<type>` comment lines, so
//! they can be enumerated from a `.sql`/`.ipynb` file's text without a live
//! database (via [`solite_core::procedure::parse_name_line`]).
//!
//! The wrinkle: a per-arg `ValueCompleter` only receives the partial value
//! being completed, not its sibling args — so the completer can't be handed the
//! file path directly. During a `COMPLETE` invocation the shell hook passes the
//! whole command line as process args, so we recover the referenced
//! `.sql`/`.ipynb` file from `std::env::args_os()`. The file-scanning core
//! ([`procedure_names_in_file`]) is kept pure and unit-tested independently of
//! that wiring.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};

use super::files::script_or_database_candidates;
use crate::cli::is_script_path;

/// Procedure names declared via `-- name:` lines in a script file.
///
/// Best-effort: returns an empty vec on any read error (a completer must never
/// panic or surface an error).
pub(crate) fn procedure_names_in_file(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| solite_core::procedure::parse_name_line(line).map(|(name, ..)| name))
        .collect()
}

/// Procedure candidates from the `.sql`/`.ipynb` file present on the current
/// completion command line, if any.
fn procedure_candidates_from_argv() -> Vec<CompletionCandidate> {
    // During a COMPLETE invocation the full command line is in the process
    // args (the shell hook forwards it). Pick the first existing script file
    // among the words and list its procedures. Requiring existence avoids
    // mistaking a half-typed path for a defined file.
    let script = std::env::args_os()
        .map(PathBuf::from)
        .find(|p| is_script_path(p) && p.is_file());
    match script {
        Some(path) => procedure_names_in_file(&path)
            .into_iter()
            .map(CompletionCandidate::new)
            .collect(),
        None => Vec::new(),
    }
}

/// Completer for `run.args`: script + database files (ticket 03) unioned with
/// procedure names from the referenced script file.
pub(crate) fn run_args_completer() -> ArgValueCompleter {
    ArgValueCompleter::new(|current: &OsStr| {
        let mut candidates = script_or_database_candidates(current);
        // Filter procedure names by the partial value so they narrow as typed.
        let prefix = current.to_string_lossy();
        candidates.extend(
            procedure_candidates_from_argv()
                .into_iter()
                .filter(|c| c.get_value().to_string_lossy().starts_with(prefix.as_ref())),
        );
        candidates
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_procedures() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queries.sql");
        std::fs::write(
            &path,
            "-- name: getUser :row\n\
             SELECT * FROM users WHERE id = $id;\n\
             \n\
             -- name: listUsers :rows\n\
             SELECT * FROM users;\n\
             \n\
             SELECT 1; -- not a procedure\n",
        )
        .unwrap();
        assert_eq!(
            procedure_names_in_file(&path),
            vec!["getUser".to_string(), "listUsers".to_string()]
        );
    }

    #[test]
    fn empty_on_no_procedures() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plain.sql");
        std::fs::write(&path, "SELECT 1;\nCREATE TABLE t(a);\n").unwrap();
        assert!(procedure_names_in_file(&path).is_empty());
    }

    #[test]
    fn empty_on_missing_file_no_panic() {
        let missing = Path::new("/no/such/file/queries.sql");
        assert!(procedure_names_in_file(missing).is_empty());
    }
}
