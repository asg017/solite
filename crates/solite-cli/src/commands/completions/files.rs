//! Extension-aware value completers for path arguments.
//!
//! `ValueHint` (ticket 02) makes the engine list *all* files for a path arg;
//! these completers narrow that to the extensions a given argument actually
//! accepts, mirroring `solite run`'s own positional classification:
//! `.sql`/`.ipynb` are scripts, `.db`/`.sqlite`/`.sqlite3` are databases.
//!
//! Directories are always offered (so multi-segment paths can be navigated):
//! `clap_complete`'s path walker keeps filtered-out directories as lower-ranked
//! candidates, so a filename-extension filter never hides them.

use std::ffi::OsStr;

use clap_complete::engine::{ArgValueCompleter, CompletionCandidate, PathCompleter, ValueCompleter};

use crate::cli::{is_database_path, is_script_path};

/// Complete script arguments: `.sql`/`.ipynb` files (plus directories).
pub(crate) fn sql_script_completer() -> ArgValueCompleter {
    ArgValueCompleter::new(PathCompleter::any().filter(|p| is_script_path(p)))
}

/// Complete database arguments: `.db`/`.sqlite`/`.sqlite3` files (plus
/// directories and the literal `:memory:`).
pub(crate) fn database_completer() -> ArgValueCompleter {
    ArgValueCompleter::new(|current: &OsStr| {
        let mut candidates = PathCompleter::any()
            .filter(|p| is_database_path(p))
            .complete(current);
        push_memory(&mut candidates, current);
        candidates
    })
}

/// Complete mixed positionals (`run`/`execute` args, `query.statement`,
/// `bench.sql`) that accept either a script or a database file. Procedure-name
/// and inline-SQL candidates for these args come from later tickets.
pub(crate) fn script_or_database_completer() -> ArgValueCompleter {
    ArgValueCompleter::new(|current: &OsStr| {
        PathCompleter::any()
            .filter(|p| is_script_path(p) || is_database_path(p))
            .complete(current)
    })
}

/// Offer `:memory:` while it still matches what's been typed. Valid anywhere a
/// database path is accepted.
fn push_memory(candidates: &mut Vec<CompletionCandidate>, current: &OsStr) {
    if let Some(cur) = current.to_str() {
        if ":memory:".starts_with(cur) {
            candidates.push(CompletionCandidate::new(":memory:"));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    /// Run a completer against `dir` with the given partial value and return the
    /// candidate strings. Uses `PathCompleter::current_dir` indirectly by
    /// passing an absolute partial value so the test doesn't mutate cwd.
    fn candidates_in(completer: &ArgValueCompleter, dir: &std::path::Path, partial: &str) -> Vec<String> {
        let value = dir.join(partial);
        completer
            .complete(&OsString::from(value))
            .into_iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            // Reduce absolute paths back to their final component for assertions.
            .map(|s| {
                std::path::Path::new(&s)
                    .file_name()
                    .map(|f| {
                        let mut out = f.to_string_lossy().into_owned();
                        if s.ends_with('/') {
                            out.push('/');
                        }
                        out
                    })
                    .unwrap_or(s)
            })
            .collect()
    }

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.sql"), "SELECT 1;").unwrap();
        std::fs::write(dir.path().join("b.ipynb"), "{}").unwrap();
        std::fs::write(dir.path().join("c.db"), "").unwrap();
        std::fs::write(dir.path().join("d.txt"), "").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        dir
    }

    #[test]
    fn script_completer_offers_scripts_and_dirs_only() {
        let dir = fixture();
        let got = candidates_in(&sql_script_completer(), dir.path(), "");
        assert!(got.contains(&"a.sql".to_string()), "{got:?}");
        assert!(got.contains(&"b.ipynb".to_string()), "{got:?}");
        assert!(got.contains(&"sub/".to_string()), "{got:?}");
        assert!(!got.contains(&"c.db".to_string()), "{got:?}");
        assert!(!got.contains(&"d.txt".to_string()), "{got:?}");
    }

    #[test]
    fn database_completer_offers_dbs_dirs_and_memory() {
        let dir = fixture();
        let got = candidates_in(&database_completer(), dir.path(), "");
        assert!(got.contains(&"c.db".to_string()), "{got:?}");
        assert!(got.contains(&"sub/".to_string()), "{got:?}");
        assert!(!got.contains(&"a.sql".to_string()), "{got:?}");
        assert!(!got.contains(&"d.txt".to_string()), "{got:?}");

        // `:memory:` is offered for an empty value (and any matching prefix).
        let raw: Vec<String> = database_completer()
            .complete(&OsString::from(""))
            .into_iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect();
        assert!(raw.iter().any(|c| c == ":memory:"), "{raw:?}");
    }

    #[test]
    fn union_completer_offers_scripts_and_dbs() {
        let dir = fixture();
        let got = candidates_in(&script_or_database_completer(), dir.path(), "");
        assert!(got.contains(&"a.sql".to_string()), "{got:?}");
        assert!(got.contains(&"c.db".to_string()), "{got:?}");
        assert!(!got.contains(&"d.txt".to_string()), "{got:?}");
    }
}
